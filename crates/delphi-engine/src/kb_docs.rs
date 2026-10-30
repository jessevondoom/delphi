//! Documents that are not Markdown, in the index (`specs/043` §6.1).
//!
//! PDF, DOCX and EPUB files under `context/` are read by the in-tree readers of `specs/014` (pure code: nothing is
//! spawned, no macro or script runs) and indexed as Markdown-shaped text under the same path with its extension.
//! Pictures, audio and video are listed by name, path, size and type only. A file over 20 MB, or one whose
//! extraction fails, is listed by name and size and says why it is not searched by content.
//!
//! The text is made here and nowhere else, so the reader (`kb.get`) shows exactly what the index holds.

use crate::kb::{Kb, SearchOpts, SourceFile};
use std::path::{Path, PathBuf};

/// A document over this is listed, not read.
pub const MAX_EXTRACT_BYTES: u64 = 20 * 1024 * 1024;
/// Text kept from one document; the rest is dropped from the index (and the header says so).
const MAX_TEXT_BYTES: usize = 1 << 20;

const READABLE: [&str; 3] = ["pdf", "docx", "epub"];
const MEDIA: [(&str, &str); 14] = [
    ("png", "image"),
    ("jpg", "image"),
    ("jpeg", "image"),
    ("gif", "image"),
    ("webp", "image"),
    ("svg", "image"),
    ("bmp", "image"),
    ("mp3", "audio"),
    ("m4a", "audio"),
    ("wav", "audio"),
    ("flac", "audio"),
    ("mp4", "video"),
    ("mov", "video"),
    ("webm", "video"),
];

pub fn extension(path: &str) -> String {
    Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// A file the index holds by what its extension says it is, not as Markdown.
pub fn is_binary_doc(path: &str) -> bool {
    let e = extension(path);
    READABLE.contains(&e.as_str()) || MEDIA.iter().any(|(m, _)| *m == e)
}

fn media_class(ext: &str) -> Option<&'static str> {
    MEDIA.iter().find(|(m, _)| *m == ext).map(|(_, c)| *c)
}

/// The non-Markdown documents under `root`, by logical path. Symlinks and dot files are never listed.
pub fn doc_files(root: &Path, logical_prefix: &str, zone: &'static str, default_source: &'static str, out: &mut Vec<SourceFile>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() || name.starts_with('.') {
            continue;
        }
        let rel = if logical_prefix.is_empty() { name.clone() } else { format!("{}/{}", logical_prefix, name) };
        if ft.is_dir() {
            doc_files(&e.path(), &rel, zone, default_source, out);
        } else if ft.is_file() && is_binary_doc(&name) {
            out.push(SourceFile { rel_path: rel, zone, path: e.path(), default_source, bundle: None, vouched_by: None });
        }
    }
}

pub fn human_size(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1u64 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{} bytes", n)
    }
}

/// The Markdown-shaped text the index holds for `path` (a file `is_binary_doc` names). `None` when it cannot be read.
pub fn text_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let ext = extension(&name);
    let size = std::fs::metadata(path).ok()?.len();
    if let Some(class) = media_class(&ext) {
        return Some(format!("{} file {}, {}, {} type {}\n\nlisted by name only: Delphi does not read pictures, sound or video (no OCR, no transcription). A note beside it named `{}.md` is indexed as an ordinary document.\n", class, name, human_size(size), class, ext, name));
    }
    let unsearched = |why: &str| Some(format!("{}, {}: listed by name and size, not searched by content: {}\n", name, human_size(size), why));
    if size > MAX_EXTRACT_BYTES {
        return unsearched(&format!("the file is over {} MB", MAX_EXTRACT_BYTES >> 20));
    }
    let bytes = std::fs::read(path).ok()?;
    let limits = delphi_docs::Limits { max_output: MAX_TEXT_BYTES, ..delphi_docs::Limits::default() };
    match delphi_docs::extract(&bytes, &name, &delphi_docs::Request::default(), &limits) {
        Ok(x) if x.supported && !x.text.trim().is_empty() => {
            let pages = match (x.unit, x.total_units) {
                (delphi_docs::Unit::Page, Some(n)) => format!(", {} pages", n),
                _ => String::new(),
            };
            let cut = if x.truncated { "; only the first part is indexed" } else { "" };
            Some(format!("text extracted from {}{}{}\n\n{}\n", name, pages, cut, x.text.trim_end()))
        }
        Ok(_) => unsearched("no text could be extracted (a scan or a picture-only file)"),
        Err(e) => unsearched(&format!("extraction failed: {}", e.0.lines().next().unwrap_or("unreadable"))),
    }
}

/// The only extensions handed to the operator's viewer (`specs/043` §6.3). `svg`, `html`, `htm`, `xml`, `js`, programs,
/// archives and everything else are never handed over; they are shown as text in the reader.
pub const OPENABLE: [&str; 11] = ["pdf", "png", "jpg", "jpeg", "gif", "webp", "mp3", "m4a", "wav", "mp4", "mov"];

/// What the daemon vouches for when the operator asks to open a file: the canonical path, and what to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub file: PathBuf,
    pub extension: String,
    pub blake3: String,
    pub trust: String,
}

/// May `rel_path` be handed to the operator's viewer? Only a document the index lists as visible (the path is looked up
/// there, never taken from a document's text or a link), with an allow-listed extension, that is a regular file whose
/// canonical path lies under the workspace. Reads nothing but the file's bytes, to name its hash for the event.
pub fn openable(ws: &Path, kb: &Kb, rel_path: &str) -> Result<Opened, String> {
    let extension = extension(rel_path);
    if !OPENABLE.contains(&extension.as_str()) {
        return Err(format!("a .{} file is never handed to a viewer; the reader shows what Delphi can read of it", extension));
    }
    let view = SearchOpts { role: None, include_untrusted: true, ..SearchOpts::default() };
    let info = kb.document(rel_path, &view).map_err(|e| e.to_string())?.ok_or("not in the knowledge base")?;
    let src = crate::kb::sources(ws).into_iter().find(|s| s.rel_path == info.rel_path).ok_or("no longer on disk")?;
    if std::fs::symlink_metadata(&src.path).map_err(|e| e.to_string())?.file_type().is_symlink() {
        return Err("a link is never opened".into());
    }
    let file = src.path.canonicalize().map_err(|e| e.to_string())?;
    let root = ws.canonicalize().map_err(|e| e.to_string())?;
    if !file.starts_with(&root) || !file.is_file() {
        return Err("the file is not inside the workspace".into());
    }
    let mut h = blake3::Hasher::new();
    h.update_reader(std::fs::File::open(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(Opened { file, extension, blake3: h.finalize().to_hex().to_string(), trust: info.source_type })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_is_named_and_a_corrupt_pdf_says_why_it_is_not_searched() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("photo.png"), b"not really a png").unwrap();
        std::fs::write(d.path().join("broken.pdf"), b"%PDF-1.4 garbage").unwrap();
        let photo = text_of(&d.path().join("photo.png")).unwrap();
        assert!(photo.contains("image file photo.png") && photo.contains("no OCR"));
        let broken = text_of(&d.path().join("broken.pdf")).unwrap();
        assert!(broken.starts_with("broken.pdf, ") && broken.contains("not searched by content"), "{broken}");
    }

    #[test]
    fn only_the_listed_extensions_are_collected_and_symlinks_are_not() {
        let d = tempfile::tempdir().unwrap();
        for n in ["a.pdf", "b.docx", "c.epub", "d.mp3", "e.exe", "f.html", "g.md"] {
            std::fs::write(d.path().join(n), b"x").unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.path().join("a.pdf"), d.path().join("link.pdf")).unwrap();
        let mut out = Vec::new();
        doc_files(d.path(), "context", "context", "external_untrusted", &mut out);
        let names: Vec<&str> = out.iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(names, ["context/a.pdf", "context/b.docx", "context/c.epub", "context/d.mp3"]);
    }

    #[test]
    fn only_an_indexed_file_with_an_allowed_extension_inside_the_workspace_is_opened() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        let ctx = ws.join("base/context");
        std::fs::create_dir_all(&ctx).unwrap();
        for n in ["a.pdf", "b.mp3", "c.svg"] {
            std::fs::write(ctx.join(n), b"x").unwrap();
        }
        std::fs::write(d.path().join("outside.pdf"), b"secret").unwrap();
        std::os::unix::fs::symlink(d.path().join("outside.pdf"), ctx.join("link.pdf")).unwrap();
        let mut kb = Kb::open_in_memory().unwrap();
        kb.sync(&crate::kb::sources(&ws), "t").unwrap();
        let ok = openable(&ws, &kb, "context/a.pdf").unwrap();
        assert_eq!((ok.extension.as_str(), ok.trust.as_str()), ("pdf", "external_untrusted"));
        assert!(ok.file.starts_with(ws.canonicalize().unwrap()) && ok.blake3.len() == 64);
        assert!(openable(&ws, &kb, "context/b.mp3").is_ok());
        assert!(openable(&ws, &kb, "context/c.svg").unwrap_err().contains("never handed"), "indexed, listed, but not on the allow-list");
        assert!(openable(&ws, &kb, "context/link.pdf").is_err(), "a symlink is not even indexed");
        for path in ["../outside.pdf", "/etc/hosts", "context/../../outside.pdf", "context/nothing.pdf", "readme.html", "context/run.exe"] {
            assert!(openable(&ws, &kb, path).is_err(), "{path}");
        }
    }
}
