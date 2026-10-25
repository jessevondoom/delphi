//! A zip reader: central directory (including zip64), stored and deflated members, bounded inflation and a
//! CRC check. It never touches the filesystem. The writer arrives with slice C0 (`specs/014` §10).

use crate::archive::{clean_name, Archive, Item, Kind};
use crate::{DocError, Limits};
use std::io::Read;

fn le16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o.checked_add(2)?)?.try_into().ok()?))
}
fn le32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o.checked_add(4)?)?.try_into().ok()?))
}
fn le64(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o.checked_add(8)?)?.try_into().ok()?))
}

struct Raw {
    method: u16,
    crc: u32,
    csize: u64,
    usize_: u64,
    offset: u64,
}

pub struct Zip<'a> {
    bytes: &'a [u8],
    items: Vec<Item>,
    raw: Vec<Raw>,
    /// Bytes in front of the first local header (a self-extracting stub, for instance).
    base: u64,
    omitted: usize,
    damaged: bool,
}

const EOCD: u32 = 0x0605_4b50;
const CEN: u32 = 0x0201_4b50;
const LOC: u32 = 0x0403_4b50;

impl<'a> Zip<'a> {
    pub fn parse(bytes: &'a [u8], limits: &Limits) -> Result<Zip<'a>, DocError> {
        let bad = |what: &str| DocError(format!("not a readable zip archive ({})", what));
        if bytes.len() < 22 {
            return Err(bad("too short"));
        }
        // End-of-central-directory record: scan back over a possible comment of up to 64 KB.
        let lo = bytes.len().saturating_sub(22 + 65_535);
        let eocd = (lo..=bytes.len() - 22)
            .rev()
            .find(|&i| le32(bytes, i) == Some(EOCD) && le16(bytes, i + 20).is_some_and(|c| i + 22 + c as usize <= bytes.len()))
            .ok_or_else(|| bad("no end-of-directory record; the file may be cut off"))?;
        let mut total = le16(bytes, eocd + 10).unwrap_or(0) as u64;
        let mut cd_size = le32(bytes, eocd + 12).unwrap_or(0) as u64;
        let mut cd_off = le32(bytes, eocd + 16).unwrap_or(0) as u64;
        if (total == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF) && eocd >= 20 && le32(bytes, eocd - 20) == Some(0x0706_4b50) {
            let z64 = le64(bytes, eocd - 12).ok_or_else(|| bad("zip64 locator"))? as usize;
            if le32(bytes, z64) != Some(0x0606_4b50) {
                return Err(bad("zip64 record missing"));
            }
            total = le64(bytes, z64 + 32).ok_or_else(|| bad("zip64 record"))?;
            cd_size = le64(bytes, z64 + 40).ok_or_else(|| bad("zip64 record"))?;
            cd_off = le64(bytes, z64 + 48).ok_or_else(|| bad("zip64 record"))?;
        }
        // The directory sits just before the end record. If the stored offset does not land on one, the file
        // has bytes prepended; shift by the difference.
        let mut base = 0u64;
        let mut start = cd_off;
        if le32(bytes, start as usize) != Some(CEN) {
            let actual = (eocd as u64).checked_sub(cd_size).ok_or_else(|| bad("directory size"))?;
            if le32(bytes, actual as usize) == Some(CEN) {
                base = actual.wrapping_sub(cd_off);
                start = actual;
            } else if total > 0 {
                return Err(bad("central directory not found"));
            }
        }
        let want = total.min(limits.max_members as u64) as usize;
        let mut items = Vec::with_capacity(want.min(4096));
        let mut raw = Vec::with_capacity(want.min(4096));
        let mut pos = start as usize;
        let mut damaged = false;
        for _ in 0..want {
            let Some(rec) = read_entry(bytes, pos) else {
                damaged = true;
                break;
            };
            pos = rec.next;
            items.push(rec.item);
            raw.push(rec.raw);
        }
        if !damaged && (items.len() as u64) < total.min(limits.max_members as u64) {
            damaged = true;
        }
        let omitted = (total as usize).saturating_sub(items.len());
        Ok(Zip { bytes, items, raw, base, omitted: if damaged { 0 } else { omitted }, damaged })
    }
}

struct Parsed {
    item: Item,
    raw: Raw,
    next: usize,
}

fn read_entry(b: &[u8], p: usize) -> Option<Parsed> {
    if le32(b, p)? != CEN {
        return None;
    }
    let made = le16(b, p + 4)?;
    let flags = le16(b, p + 8)?;
    let method = le16(b, p + 10)?;
    let crc = le32(b, p + 16)?;
    let mut csize = le32(b, p + 20)? as u64;
    let mut usize_ = le32(b, p + 24)? as u64;
    let nlen = le16(b, p + 28)? as usize;
    let xlen = le16(b, p + 30)? as usize;
    let clen = le16(b, p + 32)? as usize;
    let ext_attr = le32(b, p + 38)?;
    let mut offset = le32(b, p + 42)? as u64;
    let name = b.get(p + 46..p + 46 + nlen)?;
    let extra = b.get(p + 46 + nlen..p + 46 + nlen + xlen)?;
    let next = p + 46 + nlen + xlen + clen;
    if next > b.len() {
        return None;
    }
    // zip64 extended information: only the fields that overflowed are present, in this order.
    let mut e = 0;
    while e + 4 <= extra.len() {
        let id = le16(extra, e)?;
        let len = le16(extra, e + 2)? as usize;
        if id == 1 {
            let mut o = e + 4;
            if usize_ == 0xFFFF_FFFF {
                usize_ = le64(extra, o)?;
                o += 8;
            }
            if csize == 0xFFFF_FFFF {
                csize = le64(extra, o)?;
                o += 8;
            }
            if offset == 0xFFFF_FFFF {
                offset = le64(extra, o)?;
            }
        }
        e += 4 + len;
    }
    let (display, unsafe_name) = clean_name(name);
    let unix_mode = if made >> 8 == 3 { ext_attr >> 16 } else { 0 };
    let kind = if name.ends_with(b"/") {
        Kind::Dir
    } else if unix_mode & 0o170000 == 0o120000 {
        // The target is the member's content; it is shown only if the member is read.
        Kind::Symlink("(target stored as the member's content)".to_string())
    } else {
        Kind::File
    };
    Some(Parsed {
        item: Item { name: display, size: usize_, compressed: Some(csize), kind, unsafe_name, encrypted: flags & 1 != 0 },
        raw: Raw { method, crc, csize, usize_, offset },
        next,
    })
}

impl Archive for Zip<'_> {
    fn kind(&self) -> &'static str {
        "zip archive"
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
        let r = &self.raw[i];
        let corrupt = |what: &str| DocError(format!("member `{}` is corrupt ({})", it.name, what));
        if it.encrypted {
            return Err(DocError(format!("member `{}` is encrypted; encrypted archives are not read", it.name)));
        }
        match it.kind {
            Kind::Dir => return Err(DocError(format!("`{}` is a directory", it.name))),
            Kind::Symlink(_) => return Err(DocError(format!("`{}` is a symbolic link; links are never followed", it.name))),
            _ => {}
        }
        let cap = crate::archive::member_cap(r.csize, limits);
        if r.usize_ > cap as u64 {
            return Err(DocError(format!("member `{}` declares {} bytes, over the {} byte limit for its compressed size", it.name, r.usize_, cap)));
        }
        let lo = r.offset.checked_add(self.base).and_then(|o| usize::try_from(o).ok()).ok_or_else(|| corrupt("offset"))?;
        if le32(self.bytes, lo) != Some(LOC) {
            return Err(corrupt("local header missing"));
        }
        let n = le16(self.bytes, lo + 26).ok_or_else(|| corrupt("local header"))? as usize;
        let x = le16(self.bytes, lo + 28).ok_or_else(|| corrupt("local header"))? as usize;
        let start = lo + 30 + n + x;
        let end = usize::try_from(r.csize).ok().and_then(|c| start.checked_add(c)).ok_or_else(|| corrupt("size"))?;
        let data = self.bytes.get(start..end).ok_or_else(|| corrupt("data runs past the end of the file"))?;
        let out = match r.method {
            0 => data.to_vec(),
            8 => {
                let mut out = Vec::new();
                let mut dec = flate2::read::DeflateDecoder::new(data).take(cap as u64 + 1);
                dec.read_to_end(&mut out).map_err(|_| corrupt("deflate stream"))?;
                if out.len() > cap {
                    return Err(DocError(format!("member `{}` expands past {} bytes (decompression-bomb guard); not read", it.name, cap)));
                }
                out
            }
            m => return Err(DocError(format!("member `{}` uses compression method {}, which is not supported", it.name, m))),
        };
        if out.len() as u64 != r.usize_ {
            return Err(corrupt("size does not match the directory"));
        }
        if crc32fast::hash(&out) != r.crc {
            return Err(corrupt("checksum mismatch"));
        }
        Ok(out)
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod testutil {
    use std::io::Write;

    /// A minimal zip builder for tests; the real writer is slice C0.
    pub fn make_zip(entries: &[(&str, &[u8], u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cen = Vec::new();
        for (name, data, method) in entries {
            let comp: Vec<u8> = if *method == 8 {
                let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.to_vec()
            };
            let crc = crc32fast::hash(data);
            let off = out.len() as u32;
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(comp.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&comp);
            cen.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cen.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
            cen.extend_from_slice(&method.to_le_bytes());
            cen.extend_from_slice(&[0, 0, 0, 0]);
            cen.extend_from_slice(&crc.to_le_bytes());
            cen.extend_from_slice(&(comp.len() as u32).to_le_bytes());
            cen.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cen.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cen.extend_from_slice(&[0; 12]);
            cen.extend_from_slice(&off.to_le_bytes());
            cen.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cen);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cen.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::make_zip;
    use super::*;

    fn limits() -> Limits {
        Limits::default()
    }

    #[test]
    fn lists_and_reads_stored_and_deflated_members() {
        let z = make_zip(&[("a.txt", b"hello", 0), ("dir/b.txt", &b"world ".repeat(500), 8), ("d/", b"", 0)]);
        let a = Zip::parse(&z, &limits()).unwrap();
        assert_eq!(a.items().len(), 3);
        assert_eq!(a.items()[1].name, "dir/b.txt");
        assert_eq!(a.items()[2].kind, Kind::Dir);
        assert_eq!(a.read(0, &limits()).unwrap(), b"hello");
        assert_eq!(a.read(1, &limits()).unwrap(), b"world ".repeat(500));
        assert!(a.read(2, &limits()).unwrap_err().0.contains("directory"));
    }

    #[test]
    fn an_empty_archive_lists_nothing() {
        let z = make_zip(&[]);
        assert_eq!(Zip::parse(&z, &limits()).unwrap().items().len(), 0);
    }

    #[test]
    fn hostile_names_are_flagged_and_readable_but_never_paths() {
        let names = ["../../etc/passwd", "/abs/file", "C:\\win\\x", "a\\..\\b", "ok/../up", "ctl\u{7}char", "fine/name.txt"];
        let entries: Vec<(&str, &[u8], u16)> = names.iter().map(|n| (*n, &b"x"[..], 0)).collect();
        let z = make_zip(&entries);
        let a = Zip::parse(&z, &limits()).unwrap();
        let flags: Vec<bool> = a.items().iter().map(|i| i.unsafe_name).collect();
        assert_eq!(flags, vec![true, true, true, true, true, true, false]);
        assert!(!a.items()[5].name.contains('\u{7}'), "control characters never reach the output");
        assert_eq!(a.read(0, &limits()).unwrap(), b"x");
    }

    #[test]
    fn a_decompression_bomb_stops_at_the_limit() {
        let z = make_zip(&[("zeros", &vec![0u8; 8 << 20], 8)]);
        let a = Zip::parse(&z, &limits()).unwrap();
        let e = a.read(0, &limits()).unwrap_err();
        assert!(e.0.contains("limit"), "{}", e);
        // A small, highly compressible member is not a bomb.
        let z = make_zip(&[("zeros", &vec![0u8; 100_000], 8)]);
        assert_eq!(Zip::parse(&z, &limits()).unwrap().read(0, &limits()).unwrap().len(), 100_000);
    }

    #[test]
    fn a_lying_directory_cannot_make_the_reader_allocate_or_overrun() {
        let mut z = make_zip(&[("a", b"hello", 0)]);
        // Inflate the declared sizes in the central directory.
        let cen = z.windows(4).position(|w| w == [0x50, 0x4b, 0x01, 0x02]).unwrap();
        z[cen + 24..cen + 28].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
        let a = Zip::parse(&z, &limits()).unwrap();
        assert!(a.read(0, &limits()).is_err());
        let mut z = make_zip(&[("a", b"hello", 0)]);
        let cen = z.windows(4).position(|w| w == [0x50, 0x4b, 0x01, 0x02]).unwrap();
        z[cen + 20..cen + 24].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
        assert!(Zip::parse(&z, &limits()).unwrap().read(0, &limits()).is_err());
    }

    #[test]
    fn corruption_is_reported_not_returned_as_content() {
        let mut z = make_zip(&[("a", b"hello world", 0)]);
        let at = z.windows(11).position(|w| w == b"hello world").unwrap();
        z[at] ^= 0xff;
        let e = Zip::parse(&z, &limits()).unwrap().read(0, &limits()).unwrap_err();
        assert!(e.0.contains("checksum"), "{}", e);
    }

    #[test]
    fn member_count_is_capped_and_the_rest_counted() {
        let names: Vec<String> = (0..20).map(|i| format!("f{}", i)).collect();
        let entries: Vec<(&str, &[u8], u16)> = names.iter().map(|n| (n.as_str(), &b"x"[..], 0)).collect();
        let z = make_zip(&entries);
        let l = Limits { max_members: 5, ..limits() };
        let a = Zip::parse(&z, &l).unwrap();
        assert_eq!((a.items().len(), a.omitted()), (5, 15));
    }

    #[test]
    fn bytes_before_the_archive_are_tolerated() {
        let z = make_zip(&[("a", b"hi", 0)]);
        let mut p = b"#!/bin/sh\nexit\n".to_vec();
        p.extend_from_slice(&z);
        let a = Zip::parse(&p, &limits()).unwrap();
        assert_eq!(a.read(0, &limits()).unwrap(), b"hi");
    }

    #[test]
    fn truncated_and_bit_flipped_files_never_panic() {
        let z = make_zip(&[("a.txt", b"hello hello hello", 8), ("b.txt", b"second", 0), ("c/", b"", 0)]);
        for cut in 0..z.len() {
            if let Ok(a) = Zip::parse(&z[..cut], &limits()) {
                for i in 0..a.items().len() {
                    let _ = a.read(i, &limits());
                }
            }
        }
        let mut x: u64 = 0x2545f4914f6cdd1d;
        for _ in 0..3000 {
            let mut f = z.clone();
            for _ in 0..3 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let at = (x as usize) % f.len();
                f[at] ^= (x >> 40) as u8 | 1;
            }
            if let Ok(a) = Zip::parse(&f, &limits()) {
                for i in 0..a.items().len() {
                    let _ = a.read(i, &limits());
                }
            }
        }
    }
}
