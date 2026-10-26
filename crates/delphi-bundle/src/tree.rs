//! Steps 1 to 3 of `specs/023` §10: the shape of the tree, its bytes, and the entries a plugin brings that Delphi ignores.

use crate::Report;
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

pub const MAX_FILES: usize = 2000;
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const MAX_TREE_BYTES: u64 = 64 * 1024 * 1024;
/// Entries the standard defines at the top level; anything else is ignored with a note (step 3).
const KNOWN_TOP: [&str; 13] = [
    "bundle.toml", "README.md", "LICENSE", "CHANGELOG.md", ".delphi-bundle", ".claude-plugin", "knowledge", "skills", "agents", "behavior", "evals", "vectors", ".mcp.json",
];
/// What a Claude Code plugin brings that a bundle ignores (`specs/025` §2.3).
const PLUGIN_IGNORED: [&str; 9] = ["commands", "hooks", "output-styles", "workflows", "monitors", "themes", ".lsp.json", "settings.json", "CLAUDE.md"];

/// One regular file of the bundle, by its path relative to the root with `/` separators.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub abs: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Default)]
pub struct Tree {
    pub root: PathBuf,
    /// Regular files only, sorted by path. Anything else was reported by step 1 and left out.
    pub entries: Vec<Entry>,
}

impl Tree {
    /// The regular files of the directory, as step 1 sees them (problems are `validate`'s to report, not this).
    pub fn open(dir: &Path) -> Tree {
        scan(dir, &mut Report::default())
    }

    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.entries.binary_search_by(|e| e.path.as_str().cmp(path)).ok().map(|i| &self.entries[i])
    }

    pub fn has_dir(&self, dir: &str) -> bool {
        let p = format!("{}/", dir);
        self.entries.iter().any(|e| e.path.starts_with(&p))
    }

    pub fn under<'a>(&'a self, prefix: &str) -> impl Iterator<Item = &'a Entry> + 'a {
        let prefix = prefix.to_string();
        self.entries.iter().filter(move |e| e.path.starts_with(&prefix))
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        fs::read(&self.get(path)?.abs).ok()
    }

    /// The file as text, or `None` when it is missing or not UTF-8 (step 2 reports the latter).
    pub fn text(&self, path: &str) -> Option<String> {
        String::from_utf8(self.read(path)?).ok()
    }

    /// Every text file under `prefix` ending in `ext`, as path to text.
    pub fn texts(&self, prefix: &str, ext: &str) -> BTreeMap<String, String> {
        self.under(prefix).filter(|e| e.path.ends_with(ext)).filter_map(|e| Some((e.path.clone(), self.text(&e.path)?))).collect()
    }
}

/// Files outside `vectors/` are subject to the file and size caps; vectors have their own (step 12).
pub fn in_vectors(path: &str) -> bool {
    path.starts_with("vectors/")
}

pub fn scan(dir: &Path, r: &mut Report) -> Tree {
    let mut tree = Tree { root: dir.to_path_buf(), entries: Vec::new() };
    let name_ok = Regex::new(r"^[A-Za-z0-9._-]{1,64}$").unwrap();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut top: Vec<String> = Vec::new();
    walk(dir, "", &name_ok, &mut tree, &mut seen, &mut top, r);
    tree.entries.sort_by(|a, b| a.path.cmp(&b.path));

    let outside: Vec<&Entry> = tree.entries.iter().filter(|e| !in_vectors(&e.path)).collect();
    if outside.len() > MAX_FILES {
        r.fail(1, "", format!("{} files outside vectors/ (cap {})", outside.len(), MAX_FILES));
    }
    let total: u64 = outside.iter().map(|e| e.bytes).sum();
    if total > MAX_TREE_BYTES {
        r.fail(1, "", format!("{} MiB outside vectors/ (cap {} MiB)", total / (1024 * 1024), MAX_TREE_BYTES / (1024 * 1024)));
    }
    for e in &outside {
        if e.bytes > MAX_FILE_BYTES {
            r.fail(1, &e.path, format!("{} bytes (cap {} bytes outside vectors/)", e.bytes, MAX_FILE_BYTES));
        }
    }
    if tree.get("README.md").is_none() {
        r.fail(1, "README.md", "missing (it is what the operator agrees to when adding, specs/024 §4.2)");
    }
    check_bytes(&tree, r);
    for t in &top {
        if PLUGIN_IGNORED.contains(&t.as_str()) {
            r.note(3, t, "a plugin entry Delphi ignores (specs/025 §2.3)");
        } else if !KNOWN_TOP.contains(&t.as_str()) && t != "bin" {
            r.note(3, t, "not part of the bundle standard; ignored");
        }
    }
    tree
}

fn walk(dir: &Path, rel: &str, name_ok: &Regex, tree: &mut Tree, seen: &mut BTreeMap<String, String>, top: &mut Vec<String>, r: &mut Report) {
    let Ok(rd) = fs::read_dir(dir) else {
        r.fail(1, rel, "cannot read the directory");
        return;
    };
    let mut names: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for name in names {
        let path = if rel.is_empty() { name.clone() } else { format!("{}/{}", rel, name) };
        let abs = dir.join(&name);
        if rel.is_empty() {
            top.push(name.clone());
        }
        if name == ".git" {
            r.fail(1, &path, "a `.git` entry (a bundle is a tree, not a repository)");
            continue;
        }
        if !name_ok.is_match(&name) || name == "." || name == ".." {
            r.fail(1, &path, "name outside ^[A-Za-z0-9._-]{1,64}$");
            continue;
        }
        let key: String = path.nfc().collect::<String>().to_lowercase();
        if let Some(prev) = seen.insert(key, path.clone()) {
            r.fail(1, &path, format!("differs from `{}` only by case or normalization", prev));
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&abs) else {
            r.fail(1, &path, "cannot stat");
            continue;
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            r.fail(1, &path, "a symbolic link");
        } else if ft.is_dir() {
            if rel.is_empty() && name == "bin" {
                r.fail(1, &path, "a `bin/` directory (Claude Code puts bin/ on the shell PATH; a bundle has none)");
                continue;
            }
            walk(&abs, &path, name_ok, tree, seen, top, r);
        } else if ft.is_file() {
            if hard_linked(&meta) {
                r.fail(1, &path, "a hard link");
                continue;
            }
            tree.entries.push(Entry { path, abs, bytes: meta.len() });
        } else {
            r.fail(1, &path, "not a regular file (device, FIFO or socket)");
        }
    }
}

#[cfg(unix)]
fn hard_linked(m: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    m.nlink() > 1
}

#[cfg(not(unix))]
fn hard_linked(_: &fs::Metadata) -> bool {
    false
}

/// A character a text file may not contain: controls other than `\n` and `\t`, bidi controls, tag characters.
pub fn bad_char(c: char) -> Option<&'static str> {
    match c {
        '\n' | '\t' => None,
        c if c.is_control() => Some("a control character"),
        '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => Some("a bidi control"),
        '\u{e0000}'..='\u{e007f}' => Some("a tag character"),
        _ => None,
    }
}

/// Binary files are allowed only where a bundle needs them, and only as these types (`specs/023` §2).
fn binary_allowed(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("");
    let kind_ok = matches!(ext, "png" | "pdf");
    let place_ok = path.starts_with("knowledge/") || (path.starts_with("skills/") && (path.contains("/templates/") || path.contains("/assets/")));
    kind_ok && place_ok
}

fn check_bytes(tree: &Tree, r: &mut Report) {
    for e in &tree.entries {
        if in_vectors(&e.path) {
            continue;
        }
        let ext = e.path.rsplit('.').next().unwrap_or("");
        if matches!(ext, "png" | "pdf") {
            if !binary_allowed(&e.path) {
                r.fail(2, &e.path, format!("a .{} file outside knowledge/ and skills/*/{{templates,assets}}/", ext));
            }
            continue;
        }
        if e.bytes > MAX_FILE_BYTES {
            continue; // already reported by step 1; do not read it
        }
        let Ok(bytes) = fs::read(&e.abs) else { continue };
        let Ok(text) = String::from_utf8(bytes) else {
            r.fail(2, &e.path, "not UTF-8 text (binary files are allowed only as .png/.pdf in knowledge/ and skill templates/assets)");
            continue;
        };
        if text.contains('\r') {
            r.fail(2, &e.path, "carriage returns: line endings must be LF (the packager normalizes them before hashing)");
        }
        if !text.nfc().eq(text.chars()) {
            r.fail(2, &e.path, "not NFC-normalized");
        }
        if let Some((line, what)) = text.lines().enumerate().find_map(|(i, l)| l.chars().find_map(bad_char).map(|w| (i + 1, w))) {
            r.fail(2, &e.path, format!("line {} has {}", line, what));
        }
    }
}
