//! The `.xbundle` archive (`specs/027` §3): a POSIX ustar tar of the bundle tree, uncompressed, entries sorted bytewise,
//! regular files only, mode `0644`, no owner names, no extended headers. Two archives of the same tree are byte-identical.
//!
//! The writer uses the `tar` crate. **The reader does not**: it reads the 512-byte headers itself, because a hostile
//! archive is the input and the `tar` crate quietly interprets what a validator must refuse (GNU long names, pax
//! extensions, links, sparse files, absolute paths). Every limit is checked *as it reads*: a header's declared size is
//! compared with the caps before a byte of its data is read, and the first violation aborts. Extraction goes only into a
//! directory the caller just created; a name is checked component by component before anything is created, files are
//! made with `create_new` (never following a link, never overwriting) and given mode `0644` whatever the header says.
//! The caller then runs the full `validate` on the extracted directory, which is the real gate.

use crate::tree::{Tree, MAX_FILES, MAX_FILE_BYTES, MAX_TREE_BYTES};
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

const BLOCK: usize = 512;
/// A path is at most this long and this deep; `validate` caps each name at 64 characters (step 1).
const MAX_PATH_BYTES: usize = 255;
const MAX_DEPTH: usize = 12;

/// The caps an import enforces while reading. Outside `vectors/` they are the bundle standard's (`specs/023` §2); vectors
/// have their own (`specs/023` §8): 256 MiB per cache, and here at most 64 files and 1 GiB in all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub files: usize,
    pub file_bytes: u64,
    pub tree_bytes: u64,
    pub vector_files: usize,
    pub vector_file_bytes: u64,
    pub vector_bytes: u64,
}

impl Limits {
    pub const STANDARD: Limits = Limits {
        files: MAX_FILES,
        file_bytes: MAX_FILE_BYTES,
        tree_bytes: MAX_TREE_BYTES,
        vector_files: 64,
        vector_file_bytes: 256 * 1024 * 1024,
        vector_bytes: 1024 * 1024 * 1024,
    };
}

/// What was extracted: the paths in order, and the bytes outside and inside `vectors/`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Extracted {
    pub paths: Vec<String>,
    pub bytes: u64,
    pub vector_bytes: u64,
}

#[cfg(test)]
fn put_octal(field: &mut [u8], v: u64) {
    let s = format!("{:0width$o}", v, width = field.len() - 1);
    field[..s.len()].copy_from_slice(s.as_bytes());
    field[s.len()] = 0;
}

/// Write the tree's regular files as a deterministic ustar archive. The mtime is 0: the spec says "files.json's write
/// time", which differs between two checkouts of one tree and would break byte-reproducibility, which is what it is for.
pub fn write<W: Write>(tree: &Tree, out: W) -> Result<(), String> {
    let mut b = tar::Builder::new(out);
    for e in &tree.entries {
        let bytes = fs::read(&e.abs).map_err(|err| format!("{}: {}", e.path, err))?;
        let mut h = tar::Header::new_ustar();
        h.set_path(&e.path).map_err(|err| format!("{}: {}", e.path, err))?;
        h.set_entry_type(tar::EntryType::Regular);
        h.set_size(bytes.len() as u64);
        h.set_mode(0o644);
        h.set_uid(0);
        h.set_gid(0);
        h.set_mtime(0);
        h.set_cksum();
        b.append(&h, bytes.as_slice()).map_err(|err| format!("{}: {}", e.path, err))?;
    }
    b.finish().map_err(|e| e.to_string())
}

fn is_zero(block: &[u8]) -> bool {
    block.iter().all(|&b| b == 0)
}

fn field(block: &[u8; BLOCK], from: usize, len: usize) -> &[u8] {
    &block[from..from + len]
}

/// A NUL-terminated string field.
fn cstr(f: &[u8]) -> &[u8] {
    &f[..f.iter().position(|&b| b == 0).unwrap_or(f.len())]
}

/// An octal number padded with NULs or spaces. A base-256 field (high bit set) is refused: no real bundle needs a size
/// that large, and it is the other way to spell a number.
fn octal(f: &[u8]) -> Result<u64, String> {
    let digits: Vec<u8> = f.iter().copied().take_while(|&b| b != 0 && b != b' ').collect();
    if digits.is_empty() || digits.len() > 12 || !digits.iter().all(|b| (b'0'..=b'7').contains(b)) || f[digits.len()..].iter().any(|&b| b != 0 && b != b' ') {
        return Err("a number field is not plain octal".into());
    }
    Ok(digits.iter().fold(0u64, |acc, d| (acc << 3) | u64::from(d - b'0')))
}

fn check_header(block: &[u8; BLOCK]) -> Result<(String, u64), String> {
    if field(block, 257, 6) != b"ustar\0" || field(block, 263, 2) != b"00" {
        return Err("not a POSIX ustar header (a pax, GNU or old-format entry)".into());
    }
    let stored = octal(field(block, 148, 8)).map_err(|_| "the header checksum is not a number".to_string())?;
    let sum: u64 = block.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { u64::from(b) }).sum();
    if sum != stored {
        return Err("the header checksum is wrong".into());
    }
    if block[156] != b'0' {
        // 1 hard link, 2 symbolic link, 3/4 devices, 5 directory, 6 FIFO, x/g pax, L/K GNU long names, S sparse, ...
        return Err(format!("entry type `{}` is not a regular file (links, directories, devices and extensions are refused)", (block[156] as char).escape_default()));
    }
    if !cstr(field(block, 157, 100)).is_empty() {
        return Err("a regular file with a link name".into());
    }
    let size = octal(field(block, 124, 12))?;
    let name = cstr(field(block, 0, 100));
    let prefix = cstr(field(block, 345, 155));
    let joined = if prefix.is_empty() { name.to_vec() } else { [prefix, b"/", name].concat() };
    let path = String::from_utf8(joined).map_err(|_| "a path that is not UTF-8".to_string())?;
    Ok((path, size))
}

/// The same name rules as `validate` step 1, applied before anything touches the disk.
fn check_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES {
        return Err(format!("a path of {} bytes (cap {})", path.len(), MAX_PATH_BYTES));
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > MAX_DEPTH {
        return Err(format!("`{}` is nested {} deep (cap {})", path, parts.len(), MAX_DEPTH));
    }
    for p in &parts {
        let plain = !p.is_empty() && p.len() <= 64 && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if !plain || *p == "." || *p == ".." || *p == ".git" {
            return Err(format!("`{}` has a component that is not a plain name (`..`, absolute and empty components are refused)", path));
        }
    }
    Ok(())
}

fn read_block<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<(), String> {
    r.read_exact(buf).map_err(|e| if e.kind() == std::io::ErrorKind::UnexpectedEof { "the archive is truncated".to_string() } else { e.to_string() })
}

/// Read an archive from `r` into the existing, empty directory `dest`. Aborts at the first violation; the caller removes
/// `dest` on any error. Nothing is executed and nothing outside `dest` is touched.
pub fn extract<R: Read>(mut r: R, dest: &Path, limits: &Limits) -> Result<Extracted, String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut out = Extracted::default();
    let (mut files, mut vector_files) = (0usize, 0usize);
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    let mut prev: Option<String> = None;
    let mut block = [0u8; BLOCK];
    loop {
        read_block(&mut r, &mut block)?;
        if is_zero(&block) {
            // The end of the archive is two zero blocks; nothing but zeros may follow.
            read_block(&mut r, &mut block)?;
            if !is_zero(&block) {
                return Err("a single zero block in the middle of the archive".into());
            }
            let mut rest = [0u8; 4096];
            loop {
                let n = r.read(&mut rest).map_err(|e| e.to_string())?;
                if n == 0 {
                    return Ok(out);
                }
                if rest[..n].iter().any(|&b| b != 0) {
                    return Err("data after the end of the archive".into());
                }
            }
        }
        let (path, size) = check_header(&block)?;
        check_path(&path).map_err(|e| format!("entry {}: {}", out.paths.len() + 1, e))?;
        // Strictly increasing: sorted, and no entry twice.
        if prev.as_deref().is_some_and(|p| p.as_bytes() >= path.as_bytes()) {
            return Err(format!("{}: entries are not in strictly increasing bytewise order (or repeated)", path));
        }
        let in_vectors = path.starts_with("vectors/");
        // The caps, before any data is read.
        if in_vectors {
            if vector_files + 1 > limits.vector_files {
                return Err(format!("{}: more than {} files under vectors/", path, limits.vector_files));
            }
            if size > limits.vector_file_bytes {
                return Err(format!("{}: {} bytes (cap {} bytes under vectors/)", path, size, limits.vector_file_bytes));
            }
            if out.vector_bytes.saturating_add(size) > limits.vector_bytes {
                return Err(format!("{}: vectors/ would pass {} bytes in all", path, limits.vector_bytes));
            }
        } else {
            if files + 1 > limits.files {
                return Err(format!("{}: more than {} files outside vectors/", path, limits.files));
            }
            if size > limits.file_bytes {
                return Err(format!("{}: {} bytes (cap {} bytes outside vectors/)", path, size, limits.file_bytes));
            }
            if out.bytes.saturating_add(size) > limits.tree_bytes {
                return Err(format!("{}: the tree would pass {} bytes in all", path, limits.tree_bytes));
            }
        }
        // Directories are made by us, one component at a time, never through a path an archive could steer.
        let target = dest.join(&path);
        if let Some(parent) = path.rsplit_once('/').map(|(p, _)| p) {
            let mut so_far = String::new();
            for part in parent.split('/') {
                if !so_far.is_empty() {
                    so_far.push('/');
                }
                so_far.push_str(part);
                if dirs.insert(so_far.clone()) {
                    fs::create_dir(dest.join(&so_far)).map_err(|e| format!("{}: {}", so_far, e))?;
                }
            }
        }
        // A file where an earlier entry made a directory (or the reverse) fails here: `create_new` and `create_dir` refuse.
        let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(&target).map_err(|e| format!("{}: {}", path, e))?;
        let mut left = size;
        let mut buf = [0u8; 64 * 1024];
        while left > 0 {
            let n = buf.len().min(left as usize);
            r.read_exact(&mut buf[..n]).map_err(|e| if e.kind() == std::io::ErrorKind::UnexpectedEof { format!("{}: the archive is truncated", path) } else { e.to_string() })?;
            f.write_all(&buf[..n]).map_err(|e| format!("{}: {}", path, e))?;
            left -= n as u64;
        }
        drop(f);
        // Umask can only remove bits; this makes the mode exactly 0644 (no execute bit, ever).
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).map_err(|e| format!("{}: {}", path, e))?;
        let pad = (BLOCK - (size as usize % BLOCK)) % BLOCK;
        if pad > 0 {
            let mut p = [0u8; BLOCK];
            read_block(&mut r, &mut p[..pad])?;
            if p[..pad].iter().any(|&b| b != 0) {
                return Err(format!("{}: non-zero padding after the data", path));
            }
        }
        if in_vectors {
            vector_files += 1;
            out.vector_bytes += size;
        } else {
            files += 1;
            out.bytes += size;
        }
        prev = Some(path.clone());
        out.paths.push(path);
    }
}

/// A header with these fields, as a hostile archive (and the tests) can make it. Not used by the writer.
#[cfg(test)]
pub(crate) fn raw_header(path: &str, size: u64, typeflag: u8) -> [u8; BLOCK] {
    let mut b = [0u8; BLOCK];
    let (prefix, name) = match path.rfind('/') {
        Some(i) if path.len() > 100 => (&path[..i], &path[i + 1..]),
        _ => ("", path),
    };
    b[..name.len()].copy_from_slice(name.as_bytes());
    b[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    put_octal(&mut b[100..108], 0o644);
    put_octal(&mut b[108..116], 0);
    put_octal(&mut b[116..124], 0);
    put_octal(&mut b[124..136], size);
    put_octal(&mut b[136..148], 0);
    b[156] = typeflag;
    b[257..263].copy_from_slice(b"ustar\0");
    b[263..265].copy_from_slice(b"00");
    reseal(&mut b);
    b
}

#[cfg(test)]
pub(crate) fn reseal(b: &mut [u8; BLOCK]) {
    b[148..156].copy_from_slice(b"        ");
    let sum: u64 = b.iter().map(|&x| u64::from(x)).sum();
    let s = format!("{:06o}", sum);
    b[148..154].copy_from_slice(s.as_bytes());
    b[154] = 0;
    b[155] = b' ';
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_of(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Tree) {
        let t = tempfile::tempdir().unwrap();
        for (p, bytes) in files {
            let path = t.path().join(p);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let tree = Tree::open(t.path());
        (t, tree)
    }

    fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let (_t, tree) = tree_of(files);
        let mut out = Vec::new();
        write(&tree, &mut out).unwrap();
        out
    }

    fn entry(path: &str, data: &[u8]) -> Vec<u8> {
        let mut v = raw_header(path, data.len() as u64, b'0').to_vec();
        v.extend_from_slice(data);
        v.resize(v.len().div_ceil(BLOCK) * BLOCK, 0);
        v
    }

    fn end(mut v: Vec<u8>) -> Vec<u8> {
        v.extend_from_slice(&[0u8; 1024]);
        v
    }

    fn extract_to(bytes: &[u8], limits: &Limits) -> (tempfile::TempDir, Result<Extracted, String>) {
        let d = tempfile::tempdir().unwrap();
        let r = extract(bytes, d.path(), limits);
        (d, r)
    }

    fn refused(bytes: &[u8], limits: &Limits, why: &str) {
        let (d, r) = extract_to(bytes, limits);
        let e = r.expect_err("must be refused");
        assert!(e.contains(why), "wanted `{why}` in `{e}`");
        // Whatever was made stays inside the directory it was given; the caller removes it.
        assert!(d.path().exists());
    }

    #[test]
    fn an_archive_round_trips_and_is_byte_reproducible() {
        let files: &[(&str, &[u8])] = &[("bundle.toml", b"a = 1\n"), ("knowledge/index.md", b"# i\n"), ("README.md", &[b'x'; 700]), (".delphi-bundle/files.json", b"{}\n")];
        let a = archive(files);
        let b = archive(files);
        assert_eq!(a, b, "two archives of one tree are byte-identical");
        assert_eq!(a.len() % BLOCK, 0);
        let (d, r) = extract_to(&a, &Limits::STANDARD);
        let got = r.unwrap();
        assert_eq!(got.paths, vec![".delphi-bundle/files.json", "README.md", "bundle.toml", "knowledge/index.md"], "sorted bytewise");
        assert_eq!(fs::read(d.path().join("README.md")).unwrap(), vec![b'x'; 700]);
        assert_eq!(fs::read(d.path().join("knowledge/index.md")).unwrap(), b"# i\n");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(d.path().join("bundle.toml")).unwrap().permissions().mode() & 0o777, 0o644);
    }

    #[test]
    fn the_archive_is_plain_ustar_that_the_system_tar_reads() {
        let a = archive(&[("bundle.toml", b"a = 1\n"), ("knowledge/decisions/0042-postgres-over-dynamo.md", b"# d\n")]);
        let f = tempfile::NamedTempFile::new().unwrap();
        fs::write(f.path(), &a).unwrap();
        let Ok(o) = std::process::Command::new("tar").arg("-tf").arg(f.path()).output() else { return };
        if !o.status.success() {
            return; // no usable tar on this machine
        }
        assert_eq!(String::from_utf8_lossy(&o.stdout), "bundle.toml\nknowledge/decisions/0042-postgres-over-dynamo.md\n");
        // no pax or GNU extension entries, owners or times
        assert_eq!(&a[257..263], b"ustar\0");
        assert!(a[265..329].iter().all(|&b| b == 0), "no owner names");
        assert_eq!(octal(&a[136..148]).unwrap(), 0);
    }

    #[test]
    fn a_long_path_uses_the_ustar_prefix_not_an_extension() {
        let long = format!("knowledge/{}/{}/{}.md", "a".repeat(60), "b".repeat(60), "c".repeat(60));
        assert!(long.len() > 100);
        let a = archive(&[(&long, b"x\n")]);
        assert_eq!(a.len(), 512 + 512 + 1024, "one header, one data block, the end marker: no extension entry");
        let (_d, r) = extract_to(&a, &Limits::STANDARD);
        assert_eq!(r.unwrap().paths, vec![long]);
    }

    #[test]
    fn links_directories_devices_and_extensions_abort_at_that_entry() {
        // symbolic link, hard link, directory, device, FIFO, pax, global pax, GNU long name, GNU long link, sparse
        for flag in *b"21536xgLKS" {
            let mut v = entry("a.md", b"fine\n");
            v.extend_from_slice(&raw_header("b.md", 0, flag));
            refused(&end(v), &Limits::STANDARD, "is not a regular file");
        }
        // a regular file with a link name
        let mut h = raw_header("a.md", 0, b'0');
        h[157..160].copy_from_slice(b"/et");
        reseal(&mut h);
        refused(&end(h.to_vec()), &Limits::STANDARD, "link name");
    }

    #[test]
    fn dot_dot_absolute_and_odd_names_are_refused_before_anything_is_created() {
        let (wide, deep) = ("x".repeat(65), format!("{}/f.md", vec!["d"; 13].join("/")));
        for bad in ["../evil.md", "a/../../evil.md", "/etc/passwd", "a//b.md", "a/./b.md", "a/", ".git/config", "a\u{e9}.md", "sp ace.md", wide.as_str(), deep.as_str()] {
            let (d, r) = extract_to(&end(entry(bad, b"x")), &Limits::STANDARD);
            assert!(r.is_err(), "{bad}");
            assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0, "nothing was created for {bad}");
        }
    }

    #[test]
    fn a_bad_checksum_a_base_256_size_and_a_foreign_magic_are_refused() {
        let mut h = raw_header("a.md", 1, b'0');
        h[0] = b'b';
        refused(&end([h.to_vec(), vec![0u8; BLOCK]].concat()), &Limits::STANDARD, "checksum");
        let mut h = raw_header("a.md", 1, b'0');
        h[124] = 0x80;
        reseal(&mut h);
        refused(&end([h.to_vec(), vec![0u8; BLOCK]].concat()), &Limits::STANDARD, "plain octal");
        let mut h = raw_header("a.md", 1, b'0');
        h[257..265].copy_from_slice(b"ustar  \0"); // GNU magic
        reseal(&mut h);
        refused(&end([h.to_vec(), vec![0u8; BLOCK]].concat()), &Limits::STANDARD, "ustar header");
    }

    #[test]
    fn entries_must_be_sorted_and_unique_and_a_file_cannot_become_a_directory() {
        let mut v = entry("b.md", b"1");
        v.extend(entry("a.md", b"2"));
        refused(&end(v), &Limits::STANDARD, "increasing");
        let mut v = entry("a.md", b"1");
        v.extend(entry("a.md", b"2"));
        refused(&end(v), &Limits::STANDARD, "increasing");
        let mut v = entry("a", b"1");
        v.extend(entry("a/b.md", b"2"));
        let (_d, r) = extract_to(&end(v), &Limits::STANDARD);
        assert!(r.is_err(), "`a` is a file, so `a/b.md` cannot be made");
    }

    #[test]
    fn every_cap_aborts_at_its_boundary_before_the_data_is_read() {
        let l = Limits { files: 2, file_bytes: 10, tree_bytes: 15, vector_files: 1, vector_file_bytes: 20, vector_bytes: 25 };
        // exactly at each cap passes
        let mut ok = entry("a.md", &[b'x'; 10]);
        ok.extend(entry("b.md", &[b'x'; 5]));
        ok.extend(entry("vectors/v.dlphvec", &[b'x'; 20]));
        let (_d, r) = extract_to(&end(ok.clone()), &l);
        assert_eq!(r.unwrap(), Extracted { paths: vec!["a.md".into(), "b.md".into(), "vectors/v.dlphvec".into()], bytes: 15, vector_bytes: 20 });
        // one over each
        refused(&end(entry("a.md", &[b'x'; 11])), &l, "cap 10 bytes outside vectors/");
        let mut v = entry("a.md", &[b'x'; 10]);
        v.extend(entry("b.md", &[b'x'; 6]));
        refused(&end(v), &l, "the tree would pass 15");
        let mut v = entry("a.md", b"1");
        v.extend(entry("b.md", b"2"));
        v.extend(entry("c.md", b"3"));
        refused(&end(v), &l, "more than 2 files outside");
        refused(&end(entry("vectors/v.dlphvec", &[b'x'; 21])), &l, "cap 20 bytes under vectors/");
        let mut v = entry("vectors/a", b"1");
        v.extend(entry("vectors/b", b"2"));
        refused(&end(v), &l, "more than 1 files under vectors/");
        let l2 = Limits { vector_files: 5, ..l };
        let mut v = entry("vectors/a", &[b'x'; 20]);
        v.extend(entry("vectors/b", &[b'x'; 6]));
        refused(&end(v), &l2, "vectors/ would pass 25");
        // a declared size over the cap is refused with no data behind it at all: nothing is read to find out
        let lone = raw_header("a.md", 1 << 30, b'0');
        refused(&lone, &l, "cap 10 bytes");
    }

    #[test]
    fn truncation_trailing_garbage_and_a_missing_end_marker_are_refused() {
        let good = end(entry("a.md", b"hello"));
        extract_to(&good, &Limits::STANDARD).1.unwrap();
        refused(&good[..BLOCK + 3], &Limits::STANDARD, "truncated");
        refused(&good[..BLOCK * 2], &Limits::STANDARD, "truncated");
        refused(&entry("a.md", b"hello"), &Limits::STANDARD, "truncated"); // no end marker
        let mut one_zero = entry("a.md", b"hello");
        one_zero.extend_from_slice(&[0u8; BLOCK]);
        one_zero.extend(entry("b.md", b"x"));
        refused(&end(one_zero), &Limits::STANDARD, "single zero block");
        let mut trailing = good.clone();
        trailing.extend_from_slice(b"junk");
        refused(&trailing, &Limits::STANDARD, "after the end");
        // trailing zero padding (a tape blocking factor) is fine
        let mut padded = good;
        padded.extend_from_slice(&[0u8; 8192]);
        extract_to(&padded, &Limits::STANDARD).1.unwrap();
        // data padding must be zero
        let mut dirty = end(entry("a.md", b"hello"));
        dirty[BLOCK + 10] = 1;
        refused(&dirty, &Limits::STANDARD, "padding");
    }

    #[test]
    fn an_empty_archive_extracts_to_nothing_and_never_reads_past_a_declared_size() {
        let (_d, r) = extract_to(&[0u8; 1024], &Limits::STANDARD);
        assert_eq!(r.unwrap(), Extracted::default());
        assert!(extract_to(&[], &Limits::STANDARD).1.is_err());
    }
}
