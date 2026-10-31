//! Research held back from the shared knowledge base, and the operator's review of it (`specs/058` §13, §14).
//!
//! Two places hold such entries: the default scope's `local/pending/web/` (a weak page) and a registered project's `research/web/`.
//! Neither is indexed. The operator lists them and either ratifies an entry, which puts it under `knowledge/web/` where the index
//! reads it at the research tier (a held page is moved; a project's is copied, the project keeps its own), or discards a held one.
//! Everything here is code over files the research job wrote; a path is checked, never trusted, and nothing raises a tier.

use crate::research::{Wrote, DIR, LOGICAL, PENDING_DIR};
use serde::Serialize;
use std::path::{Path, PathBuf};

const MAX_LISTED: usize = 200;

/// Where an entry is held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// The workspace's pending folder.
    Pending,
    /// A registered project's `research/web/`, by canonical root.
    Project(PathBuf),
}

impl Held {
    fn root(&self, ws: &Path) -> PathBuf {
        match self {
            Held::Pending => ws.join(PENDING_DIR),
            Held::Project(p) => p.join(crate::research::PROJECT_DIR),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    /// `host/slug.md` below the folder that holds it.
    pub rel: String,
    pub title: String,
    pub url: String,
    pub class: String,
    pub retrieved: String,
}

/// `host/slug.md`, exactly: a validated host and a slug of lowercase letters, digits and dashes.
fn valid_rel(rel: &str) -> bool {
    let mut parts = rel.split('/');
    let (Some(host), Some(file), None) = (parts.next(), parts.next(), parts.next()) else { return false };
    let host_ok = !host.is_empty() && host.len() <= 253 && !host.starts_with(['.', '-']) && !host.contains("..") && host.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
    let slug_ok = file.strip_suffix(".md").is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    host_ok && slug_ok
}

/// A regular file, not a link and not through one.
fn plain_file(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.file_type().is_file()).unwrap_or(false)
}

fn front_value(text: &str, key: &str) -> String {
    let head = text.strip_prefix("---\n").and_then(|t| t.split("\n---\n").next()).unwrap_or("");
    let prefix = format!("{}: ", key);
    let raw = head.lines().find_map(|l| l.strip_prefix(&prefix)).unwrap_or("");
    serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_string()).chars().take(300).collect()
}

/// The pictures an entry names (`file: "images/<hash>.<ext>"`), only those of the shape the job writes.
fn image_files(text: &str) -> Vec<String> {
    let head = text.strip_prefix("---\n").and_then(|t| t.split("\n---\n").next()).unwrap_or("");
    head.lines()
        .filter_map(|l| l.trim_start().strip_prefix("- file: ").or_else(|| l.trim_start().strip_prefix("file: ")))
        .filter_map(|v| serde_json::from_str::<String>(v).ok())
        .filter(|f| f.strip_prefix("images/").and_then(|n| n.split_once('.')).is_some_and(|(h, ext)| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) && matches!(ext, "png" | "jpg" | "jpeg" | "gif")))
        .collect()
}

pub fn list(ws: &Path, held: &Held) -> Vec<Item> {
    let root = held.root(ws);
    let mut hosts: Vec<_> = std::fs::read_dir(&root).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    hosts.sort();
    let mut out = Vec::new();
    for h in hosts {
        let mut files: Vec<String> = std::fs::read_dir(root.join(&h)).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
        files.sort();
        for f in files {
            let rel = format!("{}/{}", h, f);
            let path = root.join(&rel);
            if !valid_rel(&rel) || !plain_file(&path) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            out.push(Item { title: front_value(&text, "title"), url: front_value(&text, "source_url"), class: front_value(&text, "source_class"), retrieved: front_value(&text, "retrieved"), rel });
            if out.len() >= MAX_LISTED {
                return out;
            }
        }
    }
    out
}

/// Put one held entry under `knowledge/web/`. A held page is moved; a project's is copied. Returns its logical path in the index.
pub fn ratify(ws: &Path, held: &Held, rel: &str) -> Result<(Wrote, String), String> {
    if !valid_rel(rel) {
        return Err("that is not a held entry".into());
    }
    let (src_root, dst_root) = (held.root(ws), ws.join(DIR));
    let src = src_root.join(rel);
    if !plain_file(&src) {
        return Err("no such held entry".into());
    }
    let text = std::fs::read_to_string(&src).map_err(|e| format!("could not read the entry: {}", e))?;
    if !text.starts_with("---\n") || front_value(&text, "source_url").is_empty() {
        return Err("not an entry the research job wrote".into());
    }
    let dst = dst_root.join(rel);
    let prior = std::fs::read_to_string(&dst).ok();
    let host = rel.split('/').next().unwrap_or("");
    for f in image_files(&text) {
        let (from, to) = (src_root.join(host).join(&f), dst_root.join(host).join(&f));
        if plain_file(&from) && !to.exists() {
            std::fs::create_dir_all(to.parent().ok_or("no parent")?).map_err(|e| format!("could not create the folder: {}", e))?;
            std::fs::copy(&from, &to).map_err(|e| format!("could not copy a picture: {}", e))?;
        }
    }
    let wrote = if prior.as_deref() == Some(text.as_str()) {
        Wrote::Unchanged
    } else {
        std::fs::create_dir_all(dst.parent().ok_or("no parent")?).map_err(|e| format!("could not create the folder: {}", e))?;
        std::fs::write(&dst, &text).map_err(|e| format!("could not write the entry: {}", e))?;
        if prior.is_some() { Wrote::Updated } else { Wrote::New }
    };
    if *held == Held::Pending {
        drop_entry(&src_root, rel);
    }
    Ok((wrote, format!("{}/{}", LOGICAL, rel)))
}

/// Delete one held page from the workspace's pending folder. A project's own files are never deleted from here.
pub fn discard(ws: &Path, held: &Held, rel: &str) -> Result<(), String> {
    if *held != Held::Pending {
        return Err("a project's research is the project's to delete".into());
    }
    if !valid_rel(rel) {
        return Err("that is not a held entry".into());
    }
    let root = held.root(ws);
    if !plain_file(&root.join(rel)) {
        return Err("no such held entry".into());
    }
    drop_entry(&root, rel);
    Ok(())
}

/// Remove the entry, and its host folder (with the pictures kept for it) once no entry is left there.
fn drop_entry(root: &Path, rel: &str) {
    let _ = std::fs::remove_file(root.join(rel));
    let host = root.join(rel.split('/').next().unwrap_or("-"));
    let left = std::fs::read_dir(&host).map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".md"))).unwrap_or(false);
    if !left && host.starts_with(root) && host != *root {
        let _ = std::fs::remove_dir_all(&host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ws: &Path, rel: &str, extra: &str) {
        let p = ws.join(PENDING_DIR).join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("---\ntitle: \"T\"\nsource_url: \"https://commons.wikimedia.org/x\"\nsource_class: weak\nretrieved: 2026-10-31\n{}---\n\n# T\n", extra)).unwrap();
    }

    #[test]
    fn a_held_entry_is_listed_then_ratified_into_the_index_folder_or_discarded() {
        let ws = tempfile::tempdir().unwrap();
        entry(ws.path(), "commons.wikimedia.org/a.md", "");
        entry(ws.path(), "commons.wikimedia.org/b.md", "");
        let l = list(ws.path(), &Held::Pending);
        assert_eq!((l.len(), l[0].class.as_str(), l[0].url.as_str()), (2, "weak", "https://commons.wikimedia.org/x"));
        let (wrote, logical) = ratify(ws.path(), &Held::Pending, "commons.wikimedia.org/a.md").unwrap();
        assert_eq!((wrote, logical.as_str()), (Wrote::New, "knowledge/web/commons.wikimedia.org/a.md"));
        assert!(ws.path().join(DIR).join("commons.wikimedia.org/a.md").exists());
        assert_eq!(list(ws.path(), &Held::Pending).len(), 1, "a held page is moved");
        discard(ws.path(), &Held::Pending, "commons.wikimedia.org/b.md").unwrap();
        assert!(!ws.path().join(PENDING_DIR).join("commons.wikimedia.org").exists(), "an emptied host folder goes with its last entry");
        assert!(ratify(ws.path(), &Held::Pending, "commons.wikimedia.org/b.md").is_err());
    }

    #[test]
    fn a_project_entry_is_copied_and_the_project_keeps_its_own_and_may_not_be_discarded_from_here() {
        let (ws, proj) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let hash = "a".repeat(64);
        let p = proj.path().join(crate::research::PROJECT_DIR).join("h.example");
        std::fs::create_dir_all(p.join("images")).unwrap();
        std::fs::write(p.join("images").join(format!("{}.png", hash)), b"png").unwrap();
        std::fs::write(p.join("x.md"), format!("---\ntitle: \"T\"\nsource_url: \"https://h.example/x\"\nimages:\n  - file: \"images/{}.png\"\n---\n\nbody\n", hash)).unwrap();
        let held = Held::Project(proj.path().to_path_buf());
        assert_eq!(list(ws.path(), &held).len(), 1);
        ratify(ws.path(), &held, "h.example/x.md").unwrap();
        assert!(ws.path().join(DIR).join("h.example/x.md").exists() && ws.path().join(DIR).join("h.example/images").join(format!("{}.png", hash)).exists());
        assert!(p.join("x.md").exists(), "the project keeps its own copy");
        assert!(discard(ws.path(), &held, "h.example/x.md").is_err());
    }

    #[test]
    fn a_path_that_is_not_host_slash_slug_is_refused() {
        let ws = tempfile::tempdir().unwrap();
        for bad in ["../x.md", "a/../b.md", "a/b/c.md", "a/B.md", "a/b.txt", "/etc/passwd", "a/.md", ".a/b.md", ""] {
            assert!(ratify(ws.path(), &Held::Pending, bad).is_err() && discard(ws.path(), &Held::Pending, bad).is_err(), "{}", bad);
        }
    }

    #[test]
    fn a_linked_entry_is_never_followed() {
        let ws = tempfile::tempdir().unwrap();
        let outside = ws.path().join("secret.md");
        std::fs::write(&outside, "---\nsource_url: \"u\"\n---\nsecret").unwrap();
        let dir = ws.path().join(PENDING_DIR).join("h.example");
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, dir.join("l.md")).unwrap();
        assert!(list(ws.path(), &Held::Pending).is_empty());
        assert!(ratify(ws.path(), &Held::Pending, "h.example/l.md").is_err());
    }
}
