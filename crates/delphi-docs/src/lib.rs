//! Dependency-free document readers and writers for Delphi agents (`specs/014`).
//!
//! The crate is a leaf: it knows nothing of the engine, the path guard or provenance. A caller hands it bytes
//! it has already read through the guard and gets text back. Every entry point treats its input as hostile:
//! bounded work, bounded output, and a panic becomes an error result instead of taking the daemon down.

pub mod archive;
mod cache;
mod csv;
mod data;
mod docx;
mod epub;
mod html;
mod image;
pub mod limits;
mod mail;
mod notebook;
mod odf;
mod ooxml;
mod pdf;
mod pdffont;
mod pdftext;
mod pptx;
mod rtf;
pub mod sniff;
mod sqlite;
pub mod table;
pub mod tarfile;
mod xlsx;
pub mod xml;
pub mod zipfile;
mod zipwriter;

pub use zipwriter::{safe_member_name, ZipWriter};

pub use cache::{extract_cached, stats as cache_stats};
pub use limits::Limits;
pub use sniff::{detect, Format};

use archive::{human, listing, resolve, Archive};
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// What a call asks for.
#[derive(Debug, Clone, Default)]
pub struct Request {
    /// Archive member (`name` or `#N`); `outer.zip!/inner.txt` reaches into a nested archive.
    pub member: Option<String>,
    /// First unit of a page, inclusive: a member position (0-based, as in `#N`) or a slide number (1-based).
    pub start: Option<usize>,
    /// Last unit of a page, inclusive.
    pub end: Option<usize>,
    /// Workbook sheet, by name or number from 1.
    pub sheet: Option<String>,
    /// Rows and columns of a sheet or CSV: `A1:F200`, `5:20`, `B:D`.
    pub range: Option<String>,
    /// A `SELECT` to run against a SQLite database.
    pub query: Option<String>,
}

/// The unit a document is paged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Line,
    Page,
    Sheet,
    Slide,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extract {
    pub format: Format,
    pub text: String,
    pub unit: Unit,
    /// Units in the whole document, when known.
    pub total_units: Option<usize>,
    /// The output hit a limit and more exists.
    pub truncated: bool,
    /// What this read lost, one line each (`specs/014` §2.5). The agent must not believe it read more than it did.
    pub loss: Vec<String>,
    /// False when no reader exists for the format yet; `text` then says so.
    pub supported: bool,
}

/// Why nothing could be extracted. The text is shown to the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocError(pub String);

impl std::fmt::Display for DocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Run a reader so that a panic in it is an error. A hostile file must not be able to take the daemon down.
pub fn guarded<T>(what: &str, f: impl FnOnce() -> Result<T, DocError>) -> Result<T, DocError> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(DocError(format!("the {} reader failed on this file (internal error); the file is not shown", what))),
    }
}

/// Readers that exist. Grows slice by slice (`specs/014` §10); anything else answers "not built yet".
pub fn is_supported(format: Format) -> bool {
    matches!(format, Format::Pdf | Format::Zip | Format::Docx | Format::Xlsx | Format::Pptx | Format::Odt | Format::Ods | Format::Odp | Format::Epub | Format::Sqlite | Format::Tar | Format::Gzip | Format::Png | Format::Jpeg | Format::Gif | Format::Webp) || format.is_text()
}

/// Extract text from `bytes`. `name` is the file name (a hint only).
pub fn extract(bytes: &[u8], name: &str, req: &Request, limits: &Limits) -> Result<Extract, DocError> {
    extract_at(bytes, name, req, limits, 0)
}

fn extract_at(bytes: &[u8], name: &str, req: &Request, limits: &Limits, depth: usize) -> Result<Extract, DocError> {
    if bytes.len() > limits.max_input {
        return Err(DocError(format!("the file is {} bytes; documents larger than {} bytes are not read", bytes.len(), limits.max_input)));
    }
    let format = detect(bytes, name);
    guarded(format.name(), || {
        if req.query.is_some() && format != Format::Sqlite {
            return Err(DocError(format!("`query` applies only to SQLite database files, not to {}", format.name())));
        }
        if req.member.is_some() && !matches!(format, Format::Zip | Format::Tar | Format::Gzip | Format::Json | Format::Yaml | Format::Toml | Format::Mbox) {
            return Err(DocError(format!("this {} has no members; read it without `member`", format.name())));
        }
        match format {
            Format::Zip => {
                let z = zipfile::Zip::parse(bytes, limits)?;
                // Naming a member asks for the package's raw parts; otherwise an office file reads as a document.
                // `member: "."` lists the parts.
                match ooxml::kind_of(&z, limits).filter(|_| req.member.is_none()) {
                    Some(kind) => {
                        let pkg = ooxml::Pkg { zip: &z, limits };
                        let mut e = match kind {
                            Format::Docx => docx::extract(&pkg)?,
                            Format::Xlsx => xlsx::extract(&pkg, req)?,
                            Format::Pptx => pptx::extract(&pkg, req)?,
                            Format::Odt => odf::odt(&pkg)?,
                            Format::Ods => odf::ods(&pkg, req)?,
                            Format::Odp => odf::odp(&pkg, req)?,
                            _ => epub::extract(&pkg, req)?,
                        };
                        cap_output(&mut e, limits);
                        Ok(e)
                    }
                    None if req.member.as_deref() == Some(".") => archive_extract(&z, format, &Request { member: None, ..req.clone() }, limits, depth),
                    None => archive_extract(&z, format, req, limits, depth),
                }
            }
            Format::Tar => {
                let t = tarfile::Tar::parse(bytes, limits)?;
                archive_extract(&t, format, req, limits, depth)
            }
            Format::Gzip => gzip_extract(bytes, name, req, limits, depth),
            Format::Pdf => {
                let mut e = pdftext::extract(bytes, req, limits)?;
                cap_output(&mut e, limits);
                Ok(e)
            }
            Format::Csv | Format::Tsv => {
                let mut e = csv::extract(bytes, format, req)?;
                cap_output(&mut e, limits);
                Ok(e)
            }
            Format::Html | Format::Json | Format::Yaml | Format::Toml | Format::Notebook | Format::Email | Format::Mbox | Format::Calendar | Format::Rtf => {
                let mut e = match format {
                    Format::Html => html::extract(bytes, limits)?,
                    Format::Json | Format::Yaml | Format::Toml => data::extract(bytes, format, req, limits)?,
                    Format::Notebook => notebook::extract(bytes, limits)?,
                    Format::Email => mail::eml(bytes, limits)?,
                    Format::Mbox => mail::mbox(bytes, req, limits)?,
                    Format::Calendar => mail::ics(bytes, limits)?,
                    _ => rtf::extract(bytes, limits)?,
                };
                cap_output(&mut e, limits);
                Ok(e)
            }
            Format::Png | Format::Jpeg | Format::Gif | Format::Webp => image::extract(bytes, format),
            Format::Sqlite => sqlite::extract(bytes, req, limits),
            f if f.is_text() => Ok(text_extract(bytes, f, limits)),
            _ => Ok(Extract {
                format,
                text: if format == Format::LegacyOffice {
                    format!("{} ({} bytes): the old binary Office formats are not supported; ask for the file to be saved as .docx, .xlsx or .pptx.", format.name(), bytes.len())
                } else if format == Format::Unknown {
                    format!("{} data ({} bytes): not a format this reader knows.", format.name(), bytes.len())
                } else {
                    format!("{} ({} bytes): reading this format is not built yet.", format.name(), bytes.len())
                },
                unit: Unit::Line,
                total_units: None,
                truncated: false,
                loss: vec!["no reader for this format yet".to_string()],
                supported: false,
            }),
        }
    })
}

/// Keep `e.text` within the output limit, on a character boundary.
fn cap_output(e: &mut Extract, limits: &Limits) {
    if e.text.len() > limits.max_output {
        let mut n = limits.max_output;
        while !e.text.is_char_boundary(n) {
            n -= 1;
        }
        e.text.truncate(n);
        e.truncated = true;
    }
}

fn text_extract(bytes: &[u8], format: Format, limits: &Limits) -> Extract {
    let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let mut text = String::from_utf8_lossy(body).into_owned();
    let truncated = text.len() > limits.max_output;
    if truncated {
        let mut n = limits.max_output;
        while !text.is_char_boundary(n) {
            n -= 1;
        }
        text.truncate(n);
    }
    let lines = text.lines().count();
    Extract { format, text, unit: Unit::Line, total_units: Some(lines), truncated, loss: vec![], supported: true }
}

fn archive_extract(ar: &dyn Archive, format: Format, req: &Request, limits: &Limits, depth: usize) -> Result<Extract, DocError> {
    let Some(wanted) = &req.member else {
        let (text, cut, loss) = listing(ar, req.start.unwrap_or(0), req.end, limits);
        return Ok(Extract { format, text, unit: Unit::Member, total_units: Some(ar.items().len() + ar.omitted()), truncated: cut, loss, supported: true });
    };
    if depth >= limits.max_depth {
        return Err(DocError(format!("archives nested more than {} deep are not opened", limits.max_depth)));
    }
    // An exact name wins; otherwise `outer!/rest` reaches into a nested archive.
    let (idx, rest) = match resolve(ar.items(), wanted) {
        Ok(i) => (i, None),
        Err(e) => match wanted.split_once("!/") {
            Some((head, rest)) => (resolve(ar.items(), head)?, Some(rest.to_string())),
            None => return Err(e),
        },
    };
    let item = &ar.items()[idx];
    let data = ar.read(idx, limits)?;
    let inner_req = Request { member: rest, ..req.clone() };
    let inner = extract_at(&data, &item.name, &inner_req, limits, depth + 1)?;
    let head = format!("member #{} `{}` ({})\n", idx, item.name, human(data.len() as u64));
    Ok(Extract { text: head + &inner.text, ..inner })
}

fn gzip_extract(bytes: &[u8], name: &str, req: &Request, limits: &Limits, depth: usize) -> Result<Extract, DocError> {
    if depth >= limits.max_depth {
        return Err(DocError(format!("compressed files nested more than {} deep are not opened", limits.max_depth)));
    }
    let cap = archive::member_cap(bytes.len() as u64, limits);
    let mut data = Vec::new();
    flate2::read::GzDecoder::new(bytes).take(cap as u64 + 1).read_to_end(&mut data).map_err(|_| DocError("the gzip stream is corrupt".into()))?;
    if data.len() > cap {
        return Err(DocError(format!("the gzip file expands past {} bytes (decompression-bomb guard); not read", cap)));
    }
    let inner_name = name.strip_suffix(".gz").or_else(|| name.strip_suffix(".tgz").map(|_| "archive.tar")).unwrap_or(name);
    let inner = extract_at(&data, inner_name, req, limits, depth + 1)?;
    Ok(Extract { text: format!("gzip, {} expanded\n{}", human(data.len() as u64), inner.text), ..inner })
}

#[cfg(test)]
mod corpus_tests;
#[cfg(test)]
mod office_tests;
#[cfg(test)]
mod pdf_tests;
#[cfg(test)]
mod r4_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tarfile::testutil as tar;
    use crate::zipfile::testutil::make_zip;
    use std::io::Write;

    fn ex(bytes: &[u8], name: &str, member: Option<&str>) -> Result<Extract, DocError> {
        extract(bytes, name, &Request { member: member.map(String::from), ..Request::default() }, &Limits::default())
    }

    fn gz(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn an_unbuilt_format_is_named_and_flagged_not_hidden() {
        let e = extract(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 0, 0], "a.xls", &Request::default(), &Limits::default()).unwrap();
        assert_eq!(e.format, Format::LegacyOffice);
        assert!(!e.supported);
        assert!(e.text.contains("legacy Office") && e.text.contains("saved as .docx"));
        let s = extract(&[0, 159, 146, 150, 1, 2, 3], "a.bin", &Request::default(), &Limits::default()).unwrap();
        assert!(!s.supported && s.text.contains("not a format this reader knows"), "{}", s.text);
        let g = extract(b"GIF89a", "a.gif", &Request::default(), &Limits::default()).unwrap();
        assert!(g.supported && g.loss.iter().any(|l| l.contains("damaged or truncated")), "{:?}", g.loss);
        assert_eq!(e.loss.len(), 1);
    }

    #[test]
    fn input_over_the_limit_is_refused_before_any_parsing() {
        let l = Limits { max_input: 8, ..Limits::default() };
        let err = extract(&[b'x'; 9], "a.bin", &Request::default(), &l).unwrap_err();
        assert!(err.0.contains("9 bytes"), "{}", err);
        assert!(extract(&[b'x'; 8], "a.bin", &Request::default(), &l).is_ok());
    }

    #[test]
    fn a_panicking_reader_becomes_an_error() {
        let r: Result<(), DocError> = guarded("test", || panic!("boom"));
        assert!(r.unwrap_err().0.contains("internal error"));
    }

    #[test]
    fn extract_never_panics_on_junk() {
        let mut x: u64 = 88172645463325252;
        for n in 0..500usize {
            let buf: Vec<u8> = (0..n % 300)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    x as u8
                })
                .collect();
            let _ = extract(&buf, "x.pdf", &Request::default(), &Limits::default());
            let mut z = b"PK\x03\x04".to_vec();
            z.extend_from_slice(&buf);
            let _ = extract(&z, "x.zip", &Request::default(), &Limits::default());
        }
    }

    #[test]
    fn a_zip_lists_members_with_sizes_and_flags() {
        let z = make_zip(&[("readme.md", b"# hi\n", 0), ("../escape.txt", b"x", 0), ("data/", b"", 0)]);
        let e = ex(&z, "a.zip", None).unwrap();
        assert!(e.supported && e.unit == Unit::Member && e.total_units == Some(3));
        assert!(e.text.contains("zip archive · 3 members"), "{}", e.text);
        assert!(e.text.contains("#0    readme.md") && e.text.contains("#2    data/  [directory]"), "{}", e.text);
        assert!(e.text.contains("../escape.txt") && e.text.contains("[unsafe name]"), "{}", e.text);
        assert!(e.loss.iter().any(|l| l.contains("unsafe as paths")), "{:?}", e.loss);
    }

    #[test]
    fn a_member_is_read_by_name_or_position_and_is_never_a_path() {
        let z = make_zip(&[("a/b.txt", b"line1\nline2\n", 8), ("../../etc/passwd", b"root:x", 0), ("dup", b"first", 0), ("dup", b"second", 0)]);
        let e = ex(&z, "a.zip", Some("a/b.txt")).unwrap();
        assert!(e.text.contains("member #0 `a/b.txt`") && e.text.contains("line2"), "{}", e.text);
        assert!(ex(&z, "a.zip", Some("../../etc/passwd")).unwrap().text.contains("root:x"));
        assert!(ex(&z, "a.zip", Some("dup")).unwrap().text.contains("first"));
        assert!(ex(&z, "a.zip", Some("#3")).unwrap().text.contains("second"));
        assert!(ex(&z, "a.zip", Some("#9")).unwrap_err().0.contains("no member"));
        assert!(ex(&z, "a.zip", Some("nope")).unwrap_err().0.contains("no member named"));
    }

    #[test]
    fn nested_archives_open_to_a_bounded_depth() {
        let inner = make_zip(&[("deep.txt", b"found me", 0)]);
        let mid = make_zip(&[("inner.zip", &inner, 0)]);
        let outer = make_zip(&[("mid.zip", &mid, 0)]);
        assert!(ex(&outer, "o.zip", Some("mid.zip")).unwrap().text.contains("inner.zip"));
        let e = ex(&outer, "o.zip", Some("mid.zip!/inner.zip!/deep.txt")).unwrap();
        assert!(e.text.contains("found me"), "{}", e.text);
        let l = Limits { max_depth: 2, ..Limits::default() };
        let r = extract(&outer, "o.zip", &Request { member: Some("mid.zip!/inner.zip!/deep.txt".into()), ..Request::default() }, &l);
        assert!(r.unwrap_err().0.contains("nested more than 2"));
    }

    #[test]
    fn a_member_that_is_not_an_archive_cannot_be_asked_for_members() {
        let z = make_zip(&[("a.txt", b"x", 0)]);
        let r = ex(&z, "a.zip", Some("a.txt!/b"));
        assert!(r.unwrap_err().0.contains("has no members"));
    }

    #[test]
    fn a_binary_member_reports_its_format_instead_of_dumping_bytes() {
        let z = make_zip(&[("old.xls", &[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 0, 0], 0), ("img.png", b"\x89PNG\r\n\x1a\n\0\0", 0)]);
        assert!(ex(&z, "a.zip", Some("old.xls")).unwrap().text.contains("legacy Office"));
        let e = ex(&z, "a.zip", Some("img.png")).unwrap();
        assert!(e.text.contains("PNG image"), "{}", e.text);
    }

    #[test]
    fn listing_pages_and_caps_its_output() {
        let names: Vec<String> = (0..1200).map(|i| format!("file-{:04}.txt", i)).collect();
        let entries: Vec<(&str, &[u8], u16)> = names.iter().map(|n| (n.as_str(), &b"x"[..], 0)).collect();
        let z = make_zip(&entries);
        let e = ex(&z, "a.zip", None).unwrap();
        assert!(e.truncated && e.text.contains("members #0-#499 of 1200 shown"), "{}", &e.text[e.text.len() - 120..]);
        let p = extract(&z, "a.zip", &Request { start: Some(500), end: Some(504), ..Request::default() }, &Limits::default()).unwrap();
        assert!(p.text.contains("#500") && p.text.contains("#504") && !p.text.contains("#505 "), "{}", p.text);
    }

    #[test]
    fn tar_and_tgz_list_and_read() {
        let mut t = tar::entry("notes.txt", b'0', b"tar content", "");
        t.extend(tar::entry("link", b'2', b"", "/etc/shadow"));
        let t = tar::finish(t);
        let e = ex(&t, "a.tar", None).unwrap();
        assert!(e.text.contains("tar archive · 2 members") && e.text.contains("-> /etc/shadow  [symlink, not followed]"), "{}", e.text);
        assert!(ex(&t, "a.tar", Some("notes.txt")).unwrap().text.contains("tar content"));
        assert!(ex(&t, "a.tar", Some("link")).unwrap_err().0.contains("never followed"));
        let g = gz(&t);
        assert!(ex(&g, "a.tar.gz", Some("notes.txt")).unwrap().text.contains("tar content"));
        assert!(ex(&g, "a.tgz", None).unwrap().text.contains("tar archive"));
    }

    #[test]
    fn a_plain_gzip_reads_as_its_content_and_a_bomb_is_stopped() {
        let g = gz(b"just a log line\n");
        let e = ex(&g, "app.log.gz", None).unwrap();
        assert!(e.text.contains("just a log line"), "{}", e.text);
        let bomb = gz(&vec![0u8; 16 << 20]);
        assert!(ex(&bomb, "b.gz", None).unwrap_err().0.contains("bomb"));
        let nested = gz(&gz(&gz(&gz(b"x"))));
        assert!(ex(&nested, "n.gz", None).unwrap_err().0.contains("nested"));
    }

    #[test]
    fn text_members_keep_their_text_and_are_capped() {
        let big = "line of text\n".repeat(40_000);
        let z = make_zip(&[("big.txt", big.as_bytes(), 8)]);
        let e = ex(&z, "a.zip", Some("big.txt")).unwrap();
        assert!(e.truncated && e.text.len() <= Limits::default().max_output + 100);
    }
}
