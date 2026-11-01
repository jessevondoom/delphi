//! Workspace documents as library sources, and the library's pictures (`specs/061` §4.2, §4.3).
//!
//! A search page is written from the library, and the checker knows only library records. So a workspace document that
//! answers a query (a spec, a doc, saved research) is captured into the library first, the way `delphi sources add
//! <file>` would capture it: as a source that is *noted, not validated*, with the licence `internal`. Pictures the document
//! points at, next to it and inside the workspace, are copied into the source's `assets/` so a page can show them as
//! `lib:<source>/<file>`. Nothing here raises a status; the page that results is a generated view like any other.

use super::*;
use std::path::{Path, PathBuf};

/// The text of one workspace document the library will take.
pub const MAX_DOC_BYTES: u64 = 512 * 1024;
const MAX_PICTURES: usize = 4;
const MAX_PICTURE_BYTES: u64 = 2 * 1024 * 1024;

/// The relative `![alt](href)` image targets of a Markdown text, in order, without a scheme, query or fragment.
pub fn image_hrefs(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut rest = line;
        while let Some(p) = rest.find("![") {
            let after = &rest[p + 2..];
            let Some(close) = after.find("](") else { break };
            let target = &after[close + 2..];
            let Some(end) = target.find(')') else { break };
            let href = target[..end].split_whitespace().next().unwrap_or("").trim_matches(['<', '>']);
            let href = href.split(['#', '?']).next().unwrap_or("");
            if !href.is_empty() && !href.contains(':') && !href.starts_with('/') && !out.iter().any(|h| h == href) {
                out.push(href.to_string());
            }
            rest = &target[end..];
        }
    }
    out
}

fn plain_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect()
}

fn is_picture(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif"].iter().any(|e| l.ends_with(e))
}

impl Library {
    /// Take the workspace document at `path` (known to the index as `rel`) into the library as an internal, noted source.
    /// It must be a text or Markdown file of at most 512 KiB inside the workspace `ws` (no symlink out of it). Pictures it
    /// points at, inside the same directory tree and the workspace, come with it. Returns the source's id.
    pub fn capture_workspace_doc(&self, ws: &Path, path: &Path, rel: &str, budgets: &Budgets) -> Result<Stored, String> {
        let root = ws.canonicalize().map_err(|e| format!("{}: {}", ws.display(), e))?;
        let canon = path.canonicalize().map_err(|e| format!("{}: {}", path.display(), e))?;
        if !canon.starts_with(&root) || !canon.is_file() {
            return Err(format!("`{}` is not a file inside the workspace", rel));
        }
        let lower = rel.to_ascii_lowercase();
        if !(lower.ends_with(".md") || lower.ends_with(".txt")) {
            return Err(format!("`{}` is not a text or Markdown document", rel));
        }
        if std::fs::metadata(&canon).map_err(|e| e.to_string())?.len() > MAX_DOC_BYTES {
            return Err(format!("`{}` is over {} bytes", rel, MAX_DOC_BYTES));
        }
        let bytes = std::fs::read(&canon).map_err(|e| e.to_string())?;
        let text = String::from_utf8(bytes.clone()).map_err(|_| "not a UTF-8 text or Markdown document".to_string())?;
        let name = canon.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let title = text.lines().find_map(|l| l.strip_prefix("# ")).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| name.to_string());
        let opts = AddOpts { licence: Some("internal".into()), level: Some(Level::Full), title: Some(title), ..Default::default() };
        let id = source_id(&format!("file:{}", canon.display()));
        let mut meta = Self::base_meta(&opts, "document", name);
        meta.insert("retrieved".into(), json!(delphi_core::time::now_rfc3339()));
        meta.insert("content_hash".into(), json!(format!("sha256:{}", sha256_hex(&bytes))));
        meta.insert("tags".into(), json!([]));
        meta.insert("workspace_path".into(), json!(rel));

        let dir = canon.parent().map(Path::to_path_buf).unwrap_or_else(|| root.clone());
        let stage = self.root.join(".staging").join(format!("ws-assets-{}-{}", id, std::process::id()));
        let _ = std::fs::remove_dir_all(&stage);
        let mut kept = 0;
        for href in image_hrefs(&text) {
            if kept >= MAX_PICTURES {
                break;
            }
            if !is_picture(&href) || href.split('/').any(|s| s.is_empty() || s == "." || s == "..") || href.contains('\\') {
                continue;
            }
            let Ok(file) = dir.join(&href).canonicalize() else { continue };
            let small = std::fs::metadata(&file).map(|m| m.len() <= MAX_PICTURE_BYTES).unwrap_or(false);
            if !file.starts_with(&root) || !file.is_file() || !small {
                continue;
            }
            let name = plain_name(file.file_name().and_then(|n| n.to_str()).unwrap_or("image"));
            if stage.join(&name).exists() || std::fs::create_dir_all(&stage).is_err() || std::fs::copy(&file, stage.join(&name)).is_err() {
                continue;
            }
            kept += 1;
        }
        let assets = (kept > 0).then(|| stage.clone());
        let stored = self.store(Prepared { id, meta, text: Some(text), assets, requested: Some(Level::Full), excerpts: None, imported: false }, budgets);
        let _ = std::fs::remove_dir_all(&stage);
        stored
    }

    /// The file behind `lib:<source>/<file>`: a plain-named file in the source's `assets/`, if the source exists and is not
    /// purged. The console's picture reader serves nothing else out of the library.
    pub fn asset_file(&self, id: &str, file: &str) -> Option<PathBuf> {
        let plain = !file.is_empty() && file != "." && file != ".." && file.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !plain || !is_picture(file) || !self.read_meta(id).is_some_and(|m| m["purged"].is_null()) {
            return None;
        }
        let dir = self.source_dir(id).join("assets").canonicalize().ok()?;
        let f = dir.join(file).canonicalize().ok()?;
        (f.starts_with(&dir) && f.is_file()).then_some(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_hrefs_are_relative_unique_and_outside_fences() {
        let t = "![a](img/one.png) text ![b](https://x/y.png) ![c](img/one.png)\n```\n![d](skip.png)\n```\n![e](<two.gif> \"t\") ![f](/abs.png) ![g](three.png#frag)";
        assert_eq!(image_hrefs(t), vec!["img/one.png", "two.gif", "three.png"]);
    }
}
