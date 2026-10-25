//! A tar reader: ustar and GNU/pax long names, base-256 sizes. Entries are listed, regular files are
//! readable, links and devices are never followed. Compression (`.tar.gz`) is handled by the caller.

use crate::archive::{clean_name, Archive, Item, Kind};
use crate::{DocError, Limits};

/// Largest header-extension record (long name, pax) honoured.
const MAX_EXT: u64 = 64 * 1024;

pub struct Tar<'a> {
    bytes: &'a [u8],
    items: Vec<Item>,
    /// (start, length) of each listed entry's data.
    data: Vec<(usize, usize)>,
    omitted: usize,
    damaged: bool,
}

fn octal(field: &[u8]) -> Option<u64> {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        // base-256: big-endian, high bit of the first byte is the marker
        let mut v: u64 = (field[0] & 0x7f) as u64;
        for &b in &field[1..] {
            v = v.checked_mul(256)?.checked_add(b as u64)?;
        }
        return Some(v);
    }
    let s: Vec<u8> = field.iter().copied().take_while(|&b| b != 0).collect();
    let t = std::str::from_utf8(&s).ok()?.trim();
    if t.is_empty() { Some(0) } else { u64::from_str_radix(t, 8).ok() }
}

fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// `len key=value\n` records of a pax extended header.
fn pax(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    let mut p = 0;
    while p < data.len() {
        let Some(sp) = data[p..].iter().position(|&c| c == b' ') else { break };
        let Some(len) = std::str::from_utf8(&data[p..p + sp]).ok().and_then(|s| s.parse::<usize>().ok()) else { break };
        if len <= sp + 1 || p + len > data.len() {
            break;
        }
        let rec = &data[p + sp + 1..p + len];
        let rec = rec.strip_suffix(b"\n").unwrap_or(rec);
        if let Some(eq) = rec.iter().position(|&c| c == b'=') {
            out.push((String::from_utf8_lossy(&rec[..eq]).into_owned(), rec[eq + 1..].to_vec()));
        }
        p += len;
    }
    out
}

impl<'a> Tar<'a> {
    pub fn parse(bytes: &'a [u8], limits: &Limits) -> Result<Tar<'a>, DocError> {
        if bytes.len() < 512 {
            return Err(DocError("not a readable tar archive (too short)".into()));
        }
        let mut items = vec![];
        let mut data = vec![];
        let mut omitted = 0;
        let mut damaged = false;
        let mut pos = 0usize;
        let mut long_name: Option<Vec<u8>> = None;
        let mut long_link: Option<Vec<u8>> = None;
        let mut px_path: Option<Vec<u8>> = None;
        let mut px_link: Option<Vec<u8>> = None;
        let mut px_size: Option<u64> = None;
        while pos + 512 <= bytes.len() {
            let h = &bytes[pos..pos + 512];
            if h.iter().all(|&b| b == 0) {
                break;
            }
            let stored = octal(&h[148..156]);
            let unsigned: u64 = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u64 }).sum();
            if stored != Some(unsigned) {
                damaged = true;
                break;
            }
            let Some(mut size) = octal(&h[124..136]) else {
                damaged = true;
                break;
            };
            let typeflag = h[156];
            let data_start = pos + 512;
            let padded = size.checked_add(511).map(|s| s / 512 * 512);
            let next = padded.and_then(|p| usize::try_from(p).ok()).and_then(|p| data_start.checked_add(p));
            let Some(next) = next else {
                damaged = true;
                break;
            };
            if matches!(typeflag, b'L' | b'K' | b'x' | b'g') {
                if size > MAX_EXT || data_start + size as usize > bytes.len() {
                    damaged = true;
                    break;
                }
                let body = &bytes[data_start..data_start + size as usize];
                match typeflag {
                    b'L' => long_name = Some(cstr(body).to_vec()),
                    b'K' => long_link = Some(cstr(body).to_vec()),
                    b'x' => {
                        for (k, v) in pax(body) {
                            match k.as_str() {
                                "path" => px_path = Some(v),
                                "linkpath" => px_link = Some(v),
                                "size" => px_size = std::str::from_utf8(&v).ok().and_then(|s| s.trim().parse().ok()),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                pos = next;
                continue;
            }
            if let Some(s) = px_size.take() {
                size = s;
            }
            let mut name = cstr(&h[0..100]).to_vec();
            if &h[257..262] == b"ustar" && h[345] != 0 {
                let mut full = cstr(&h[345..500]).to_vec();
                full.push(b'/');
                full.extend_from_slice(&name);
                name = full;
            }
            if let Some(n) = long_name.take() {
                name = n;
            }
            if let Some(n) = px_path.take() {
                name = n;
            }
            let link = px_link.take().or(long_link.take()).unwrap_or_else(|| cstr(&h[157..257]).to_vec());
            let kind = match typeflag {
                0 | b'0' | b'7' => Kind::File,
                b'5' => Kind::Dir,
                b'2' => Kind::Symlink(String::from_utf8_lossy(&link).into_owned()),
                b'1' => Kind::Other("hard link"),
                b'3' | b'4' | b'6' => Kind::Other("device or pipe"),
                _ => Kind::Other("special entry"),
            };
            let on_disk = if matches!(kind, Kind::File) { size } else { 0 };
            let end = usize::try_from(on_disk).ok().and_then(|s| data_start.checked_add(s));
            if end.is_none_or(|e| e > bytes.len()) {
                damaged = true;
                break;
            }
            if items.len() >= limits.max_members {
                omitted += 1;
            } else {
                let (shown, unsafe_name) = clean_name(&name);
                items.push(Item { name: shown, size: on_disk, compressed: None, kind, unsafe_name, encrypted: false });
                data.push((data_start, on_disk as usize));
            }
            // A directory or link carries no data blocks even if its size field is nonzero in the wild.
            pos = if matches!(typeflag, 0 | b'0' | b'7') { next } else { data_start };
        }
        Ok(Tar { bytes, items, data, omitted, damaged })
    }
}

impl Archive for Tar<'_> {
    fn kind(&self) -> &'static str {
        "tar archive"
    }
    fn items(&self) -> &[Item] {
        &self.items
    }
    fn omitted(&self) -> usize {
        self.omitted
    }
    fn damaged(&self) -> bool {
        self.damaged
    }
    fn read(&self, i: usize, limits: &Limits) -> Result<Vec<u8>, DocError> {
        let it = self.items.get(i).ok_or_else(|| DocError("no such member".into()))?;
        match &it.kind {
            Kind::File => {}
            Kind::Dir => return Err(DocError(format!("`{}` is a directory", it.name))),
            Kind::Symlink(_) => return Err(DocError(format!("`{}` is a symbolic link; links are never followed", it.name))),
            Kind::Other(w) => return Err(DocError(format!("`{}` is a {}; it has no readable content", it.name, w))),
        }
        let (start, len) = self.data[i];
        if len > limits.max_input.min(limits.max_expanded) {
            return Err(DocError(format!("member `{}` is {} bytes, over the {} byte limit", it.name, len, limits.max_input.min(limits.max_expanded))));
        }
        Ok(self.bytes[start..start + len].to_vec())
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod testutil {
    fn field(h: &mut [u8], at: usize, s: &[u8]) {
        h[at..at + s.len()].copy_from_slice(s);
    }

    /// One tar entry: header + padded data. `typeflag` is the raw byte.
    pub fn entry(name: &str, typeflag: u8, data: &[u8], link: &str) -> Vec<u8> {
        let mut h = vec![0u8; 512];
        field(&mut h, 0, name.as_bytes());
        field(&mut h, 100, b"0000644\0");
        field(&mut h, 124, format!("{:011o}\0", data.len()).as_bytes());
        h[156] = typeflag;
        field(&mut h, 157, link.as_bytes());
        field(&mut h, 257, b"ustar\0");
        field(&mut h, 148, b"        ");
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        field(&mut h, 148, format!("{:06o}\0 ", sum).as_bytes());
        h.extend_from_slice(data);
        h.resize(512 + data.len().div_ceil(512) * 512, 0);
        h
    }

    pub fn finish(mut v: Vec<u8>) -> Vec<u8> {
        v.extend_from_slice(&[0u8; 1024]);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::{entry, finish};
    use super::*;

    fn limits() -> Limits {
        Limits::default()
    }

    #[test]
    fn lists_and_reads_files_and_flags_links() {
        let mut t = entry("a.txt", b'0', b"hello", "");
        t.extend(entry("d/", b'5', b"", ""));
        t.extend(entry("l", b'2', b"", "../../etc/passwd"));
        t.extend(entry("h", b'1', b"", "a.txt"));
        t.extend(entry("../evil", b'0', b"x", ""));
        let t = finish(t);
        let a = Tar::parse(&t, &limits()).unwrap();
        assert_eq!(a.items().len(), 5);
        assert_eq!(a.read(0, &limits()).unwrap(), b"hello");
        assert_eq!(a.items()[1].kind, Kind::Dir);
        assert!(matches!(&a.items()[2].kind, Kind::Symlink(t) if t == "../../etc/passwd"));
        assert!(a.read(2, &limits()).unwrap_err().0.contains("never followed"));
        assert!(a.read(3, &limits()).is_err());
        assert!(a.items()[4].unsafe_name);
        assert_eq!(a.read(4, &limits()).unwrap(), b"x");
        assert!(!a.damaged());
    }

    #[test]
    fn gnu_long_names_and_pax_paths_are_honoured() {
        let long = "very/".repeat(40) + "deep.txt";
        let mut t = entry("././@LongLink", b'L', format!("{}\0", long).as_bytes(), "");
        t.extend(entry("truncated-name", b'0', b"one", ""));
        let rec = "path=pax/name.txt\n";
        let mut n = rec.len() + 2;
        while format!("{} {}", n, rec).len() != n {
            n = format!("{} {}", n, rec).len();
        }
        t.extend(entry("PaxHeader", b'x', format!("{} {}", n, rec).as_bytes(), ""));
        t.extend(entry("ignored", b'0', b"two", ""));
        let t = finish(t);
        let a = Tar::parse(&t, &limits()).unwrap();
        assert_eq!(a.items()[0].name, long);
        assert_eq!(a.items()[1].name, "pax/name.txt");
        assert_eq!(a.read(1, &limits()).unwrap(), b"two");
    }

    #[test]
    fn a_bad_checksum_stops_the_listing_and_says_so() {
        let mut t = entry("a", b'0', b"x", "");
        t.extend(entry("b", b'0', b"y", ""));
        t[1024 + 10] ^= 0xff; // corrupt the second header (the first entry is a header block plus a data block)
        let t = finish(t);
        let a = Tar::parse(&t, &limits()).unwrap();
        assert_eq!(a.items().len(), 1);
        assert!(a.damaged());
    }

    #[test]
    fn a_size_that_overruns_the_file_is_damage_not_a_panic_or_an_allocation() {
        let mut t = entry("a", b'0', b"x", "");
        t[124..136].copy_from_slice(b"77777777777\0");
        let sum: u32 = t[..512].iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u32 }).sum();
        t[148..156].copy_from_slice(format!("{:06o}\0 ", sum).as_bytes());
        let a = Tar::parse(&t, &limits()).unwrap();
        assert!(a.items().is_empty() && a.damaged());
        // base-256 with the maximum value
        let mut t = entry("a", b'0', b"x", "");
        t[124] = 0xff;
        for b in &mut t[125..136] {
            *b = 0xff;
        }
        let _ = Tar::parse(&t, &limits());
    }

    #[test]
    fn truncated_and_bit_flipped_tars_never_panic() {
        let mut t = entry("a.txt", b'0', b"hello hello", "");
        t.extend(entry("d/", b'5', b"", ""));
        t.extend(entry("b", b'0', &vec![7u8; 700], ""));
        let t = finish(t);
        for cut in 0..t.len() {
            if let Ok(a) = Tar::parse(&t[..cut], &limits()) {
                for i in 0..a.items().len() {
                    let _ = a.read(i, &limits());
                }
            }
        }
        let mut x: u64 = 0x9e3779b97f4a7c15;
        for _ in 0..3000 {
            let mut f = t.clone();
            for _ in 0..3 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let at = (x as usize) % f.len();
                f[at] ^= (x >> 40) as u8 | 1;
            }
            if let Ok(a) = Tar::parse(&f, &limits()) {
                for i in 0..a.items().len() {
                    let _ = a.read(i, &limits());
                }
            }
        }
    }
}
