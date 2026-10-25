//! A zip writer (`specs/014` §7, slice C0): stored or deflated members, UTF-8 names, a fixed timestamp so the
//! same input always produces the same bytes. No zip64: an archive is limited to 65,535 members and 4 GB,
//! far above anything the artifact tools produce (their output is capped at a few MB).

use crate::DocError;
use std::collections::HashSet;
use std::io::Write;

/// A member name that is safe to put in an archive: relative, no `..`, no backslashes, no control characters.
pub fn safe_member_name(name: &str) -> Result<String, DocError> {
    let n = name.trim();
    let bad = |why: &str| DocError(format!("`{}` is not a usable archive member name ({})", name.chars().take(80).collect::<String>(), why));
    if n.is_empty() {
        return Err(bad("empty"));
    }
    if n.len() > 512 {
        return Err(bad("longer than 512 bytes"));
    }
    if n.starts_with('/') || n.starts_with('~') || n.as_bytes().get(1) == Some(&b':') {
        return Err(bad("absolute"));
    }
    if n.contains('\\') || n.chars().any(|c| c.is_control()) {
        return Err(bad("backslash or control character"));
    }
    if n.split('/').any(|p| p == ".." || p.is_empty()) {
        return Err(bad("empty or `..` path segment"));
    }
    Ok(n.to_string())
}

struct Entry {
    name: String,
    method: u16,
    crc: u32,
    csize: u32,
    usize_: u32,
    offset: u32,
}

#[derive(Default)]
pub struct ZipWriter {
    out: Vec<u8>,
    entries: Vec<Entry>,
    names: HashSet<String>,
}

impl ZipWriter {
    pub fn new() -> ZipWriter {
        ZipWriter::default()
    }

    /// Add a member, deflated when that makes it smaller. Duplicate names are refused.
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<(), DocError> {
        self.add_inner(name, data, false)
    }

    /// Add a member stored uncompressed (a `mimetype` member must be, and first).
    pub fn add_stored(&mut self, name: &str, data: &[u8]) -> Result<(), DocError> {
        self.add_inner(name, data, true)
    }

    fn add_inner(&mut self, name: &str, data: &[u8], force_stored: bool) -> Result<(), DocError> {
        let name = safe_member_name(name)?;
        if !self.names.insert(name.clone()) {
            return Err(DocError(format!("`{}` is added twice", name)));
        }
        if self.entries.len() >= 65_535 || self.out.len() as u64 + data.len() as u64 > u32::MAX as u64 / 2 {
            return Err(DocError("the archive would be too large".into()));
        }
        let crc = crc32fast::hash(data);
        let mut method = 0u16;
        let mut body = data.to_vec();
        if !force_stored && data.len() > 32 {
            let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            if e.write_all(data).is_ok() {
                if let Ok(c) = e.finish() {
                    if c.len() < data.len() {
                        method = 8;
                        body = c;
                    }
                }
            }
        }
        let offset = self.out.len() as u32;
        let o = &mut self.out;
        o.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        o.extend_from_slice(&20u16.to_le_bytes());
        o.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        o.extend_from_slice(&method.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes()); // time 00:00
        o.extend_from_slice(&0x0021u16.to_le_bytes()); // date 1980-01-01
        o.extend_from_slice(&crc.to_le_bytes());
        o.extend_from_slice(&(body.len() as u32).to_le_bytes());
        o.extend_from_slice(&(data.len() as u32).to_le_bytes());
        o.extend_from_slice(&(name.len() as u16).to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes());
        o.extend_from_slice(name.as_bytes());
        o.extend_from_slice(&body);
        self.entries.push(Entry { name, method, crc, csize: body.len() as u32, usize_: data.len() as u32, offset });
        Ok(())
    }

    pub fn finish(mut self) -> Vec<u8> {
        let cd_start = self.out.len() as u32;
        for e in &self.entries {
            let o = &mut self.out;
            o.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            o.extend_from_slice(&0x031eu16.to_le_bytes()); // made by: Unix, 3.0
            o.extend_from_slice(&20u16.to_le_bytes());
            o.extend_from_slice(&0x0800u16.to_le_bytes());
            o.extend_from_slice(&e.method.to_le_bytes());
            o.extend_from_slice(&0u16.to_le_bytes());
            o.extend_from_slice(&0x0021u16.to_le_bytes());
            o.extend_from_slice(&e.crc.to_le_bytes());
            o.extend_from_slice(&e.csize.to_le_bytes());
            o.extend_from_slice(&e.usize_.to_le_bytes());
            o.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            o.extend_from_slice(&[0; 6]); // extra, comment, disk number
            o.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
            o.extend_from_slice(&((0o100644u32) << 16).to_le_bytes()); // a regular file, rw-r--r--
            o.extend_from_slice(&e.offset.to_le_bytes());
            o.extend_from_slice(e.name.as_bytes());
        }
        let cd_size = self.out.len() as u32 - cd_start;
        let n = self.entries.len() as u16;
        let o = &mut self.out;
        o.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        o.extend_from_slice(&[0; 4]);
        o.extend_from_slice(&n.to_le_bytes());
        o.extend_from_slice(&n.to_le_bytes());
        o.extend_from_slice(&cd_size.to_le_bytes());
        o.extend_from_slice(&cd_start.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes());
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{Archive, Kind};
    use crate::zipfile::Zip;
    use crate::Limits;

    #[test]
    fn what_is_written_reads_back_exactly() {
        let big = "repeat me ".repeat(500);
        let mut w = ZipWriter::new();
        w.add_stored("mimetype", b"application/x-test").unwrap();
        w.add("dir/notes.txt", big.as_bytes()).unwrap();
        w.add("tiny", b"x").unwrap();
        w.add("ünï/çode.txt", "unicode names".as_bytes()).unwrap();
        w.add("empty", b"").unwrap();
        let z = w.finish();
        let l = Limits::default();
        let a = Zip::parse(&z, &l).unwrap();
        let names: Vec<&str> = a.items().iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["mimetype", "dir/notes.txt", "tiny", "ünï/çode.txt", "empty"]);
        assert!(a.items().iter().all(|i| i.kind == Kind::File && !i.unsafe_name));
        assert_eq!(a.read(0, &l).unwrap(), b"application/x-test");
        assert_eq!(a.read(1, &l).unwrap(), big.as_bytes());
        assert_eq!(a.read(3, &l).unwrap(), "unicode names".as_bytes());
        assert_eq!(a.read(4, &l).unwrap(), b"");
        assert!(a.items()[1].compressed.unwrap() < big.len() as u64 / 4, "repetitive data is deflated");
        assert_eq!(a.items()[2].compressed, Some(1), "small data is stored");
    }

    /// `DELPHI_DOCS_ZIP_OUT=/tmp/sample.zip cargo test -p delphi-docs write_a_sample -- --ignored`, then check it
    /// with `unzip -t` and `python3 -m zipfile -t`: an independent reader's opinion of what we write.
    #[test]
    #[ignore = "writes a file for external tools to check"]
    fn write_a_sample_for_external_readers() {
        let Ok(path) = std::env::var("DELPHI_DOCS_ZIP_OUT") else { return };
        let mut w = ZipWriter::new();
        w.add_stored("mimetype", b"application/x-test").unwrap();
        w.add("docs/readme.txt", "deflate me please ".repeat(200).as_bytes()).unwrap();
        w.add("ünï/çode.txt", "unicode names".as_bytes()).unwrap();
        w.add("empty", b"").unwrap();
        std::fs::write(path, w.finish()).unwrap();
    }

    #[test]
    fn the_same_input_gives_the_same_bytes() {
        let make = || {
            let mut w = ZipWriter::new();
            w.add("a.txt", b"hello hello hello hello hello hello hello hello").unwrap();
            w.add("b/c.txt", b"world").unwrap();
            w.finish()
        };
        assert_eq!(make(), make());
    }

    #[test]
    fn unsafe_and_duplicate_names_are_refused() {
        let mut w = ZipWriter::new();
        for bad in ["", "/etc/passwd", "../x", "a/../b", "a//b", "a\\b", "C:/x", "~/x", "ctl\u{7}", &"n".repeat(600)] {
            assert!(w.add(bad, b"x").is_err(), "{:?} was accepted", bad);
        }
        w.add("ok", b"1").unwrap();
        assert!(w.add("ok", b"2").unwrap_err().0.contains("twice"));
        assert!(Zip::parse(&w.finish(), &Limits::default()).unwrap().items().len() == 1);
    }

    #[test]
    fn an_empty_archive_is_valid_and_many_members_work() {
        let z = ZipWriter::new().finish();
        assert_eq!(Zip::parse(&z, &Limits::default()).unwrap().items().len(), 0);
        let mut w = ZipWriter::new();
        for i in 0..2000 {
            w.add(&format!("f/{}.txt", i), format!("content {}", i).as_bytes()).unwrap();
        }
        let z = w.finish();
        let a = Zip::parse(&z, &Limits::default()).unwrap();
        assert_eq!(a.items().len(), 2000);
        assert_eq!(a.read(1234, &Limits::default()).unwrap(), b"content 1234");
    }
}
