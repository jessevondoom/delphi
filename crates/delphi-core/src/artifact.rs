//! Scope of *No Spec, No Build* and the artifact prompt log (`specs/070`).
//!
//! [`classify`] places a write in one of four classes. Only [`Class::Software`] and [`Class::Governed`]
//! gate on a spec or ratification. [`Class::Artifact`] is a closed allow-list; anything unlisted, or
//! unreadable, is software (fail closed). [`PromptLog`] is the daemon-written, append-only,
//! hash-chained record of the operator prompts behind artifacts.

use crate::redact::Redactor;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Code, tests, scripts, configuration and anything executable or loaded: needs an approved spec.
    Software,
    /// Documents, images, audio and video on the closed allow-list: needs no spec, takes a prompt-log entry.
    Artifact,
    /// Markdown, scratch files and other regular operation: always allowed within permissions.
    System,
    /// Constitution, specs, memory, agents, skills, themes: ratified proposals only.
    Governed,
}

const GOVERNED_PREFIXES: [&str; 6] = ["specs/", "memory/", "agents/", "skills/", "themes/", "context/heuristics"];
const GOVERNED_FILES: [&str; 8] = ["governance.md", "spec.md", "review.md", "agents.md", "claude.md", "gemini.md", ".github/copilot-instructions.md", "heuristics.md"];
/// Plain types that cannot run and need no content check.
const ARTIFACT_PLAIN: [&str; 24] = [
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "avif", "mp3", "wav", "m4a", "flac", "ogg", "mp4", "mov", "webm", "mkv", "csv", "tsv", "odt", "rtf", "epub", "heic",
];
/// Types that can carry script or macros: an artifact only when the content is readable and clean.
const ARTIFACT_CHECKED: [&str; 9] = ["svg", "html", "htm", "docx", "xlsx", "pptx", "odp", "ods", "key"];
const SYSTEM_TEXT: [&str; 3] = ["md", "markdown", "txt"];
const ACTIVE_MARKERS: [&str; 9] = ["<script", "javascript:", "vbaproject", "macros/", "<foreignobject", "<iframe", "<object", "<embed", "onload="];

fn normalize(path: &str) -> Option<String> {
    let p = path.replace('\\', "/").to_lowercase();
    if p.is_empty() || p.starts_with('/') || p.starts_with('~') || p.contains('\0') {
        return None;
    }
    let mut segs: Vec<&str> = Vec::new();
    for s in p.split('/') {
        match s {
            "" | "." => {}
            ".." => return None,
            s => segs.push(s),
        }
    }
    if segs.is_empty() { None } else { Some(segs.join("/")) }
}

fn has_active_content(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes).to_lowercase();
    ACTIVE_MARKERS.iter().any(|m| text.contains(m)) || text.contains(" on") && has_event_handler(&text)
}

fn has_event_handler(text: &str) -> bool {
    text.match_indices(" on").any(|(i, _)| {
        let rest = &text[i + 3..];
        let name: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        !name.is_empty() && rest[name.len()..].trim_start().starts_with('=')
    })
}

/// Class of a write to `path` (a logical path, relative to its root). `content` is the bytes about to
/// be written, when known; a type that can carry script is an artifact only when its content is given
/// and holds none. The class comes from the path and content, never from an agent's claim.
pub fn classify(path: &str, content: Option<&[u8]>) -> Class {
    let Some(key) = normalize(path) else { return Class::Software };
    if GOVERNED_FILES.contains(&key.as_str()) || GOVERNED_PREFIXES.iter().any(|p| key.starts_with(p)) {
        return Class::Governed;
    }
    let name = key.rsplit('/').next().unwrap_or("");
    if name.starts_with('.') {
        return Class::Software;
    }
    let ext = name.rsplit_once('.').map(|x| x.1).unwrap_or("");
    if let Some(rest) = key.strip_prefix("scratch/") {
        return if crate::pathguard::scratch_refused(rest).is_none() { Class::System } else { Class::Software };
    }
    if SYSTEM_TEXT.contains(&ext) {
        return Class::System;
    }
    if ARTIFACT_PLAIN.contains(&ext) {
        return Class::Artifact;
    }
    if ARTIFACT_CHECKED.contains(&ext) {
        return match content {
            Some(b) if !has_active_content(b) => Class::Artifact,
            _ => Class::Software,
        };
    }
    Class::Software
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Create,
    Edit,
}

/// One operator prompt behind one artifact. Nothing here comes from an agent or a model reply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub ts: String,
    pub task_id: String,
    pub artifact: String,
    pub op: Op,
    pub prompt: String,
    pub artifact_hash: Option<String>,
    pub prev: String,
    pub hash: String,
}

pub const LOG_FILE: &str = "artifact-prompts.jsonl";
const GENESIS: &str = "genesis";

fn entry_hash(e: &Entry) -> String {
    let mut h = blake3::Hasher::new();
    for part in [&e.prev, &e.ts, &e.task_id, &e.artifact, &format!("{:?}", e.op), &e.prompt, e.artifact_hash.as_deref().unwrap_or("")] {
        h.update(&(part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
    }
    h.finalize().to_hex().to_string()
}

/// The daemon's append-only prompt log, in a daemon-only directory (`local/…`, never agent-writable).
pub struct PromptLog {
    path: PathBuf,
}

impl PromptLog {
    pub fn open(dir: &Path) -> std::io::Result<PromptLog> {
        fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(PromptLog { path: dir.join(LOG_FILE) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read_entries(&self) -> Result<Vec<Entry>, String> {
        let mut s = String::new();
        match fs::File::open(&self.path) {
            Ok(mut f) => f.read_to_string(&mut s).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        s.lines().enumerate().map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {}", i + 1, e))).collect()
    }

    /// Append one entry for an operator prompt. The prompt is redacted first; an empty prompt is
    /// refused, because a system-started write belongs in the daemon audit log, not here.
    pub fn append(&self, redactor: &Redactor, task_id: &str, artifact: &str, op: Op, operator_prompt: &str, artifact_hash: Option<&str>) -> Result<Entry, String> {
        if operator_prompt.trim().is_empty() {
            return Err("no operator prompt: system-started work is recorded in the daemon audit log, not the prompt log".into());
        }
        let prev = self.read_entries()?.last().map(|e| e.hash.clone()).unwrap_or_else(|| GENESIS.into());
        let mut e = Entry {
            ts: crate::time::now_rfc3339(),
            task_id: task_id.into(),
            artifact: artifact.into(),
            op,
            prompt: redactor.redact_str(operator_prompt).0,
            artifact_hash: artifact_hash.map(String::from),
            prev,
            hash: String::new(),
        };
        e.hash = entry_hash(&e);
        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&self.path).map_err(|e| e.to_string())?;
        let mut line = serde_json::to_string(&e).map_err(|e| e.to_string())?;
        line.push('\n');
        f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        Ok(e)
    }

    /// Check the hash chain end to end. Returns the entry count, or the first break.
    pub fn verify(&self) -> Result<usize, String> {
        let entries = self.read_entries()?;
        let mut prev = GENESIS.to_string();
        for (i, e) in entries.iter().enumerate() {
            if e.prev != prev || entry_hash(e) != e.hash {
                return Err(format!("hash chain broken at entry {}", i + 1));
            }
            prev = e.hash.clone();
        }
        Ok(entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_is_the_default_and_the_floor() {
        for p in ["src/main.rs", "build.rs", "ci/run.sh", ".github/workflows/ci.yml", "Cargo.toml", "notebook.ipynb", "q.sql", "tool.py", "unknown.xyz", "noext", "../x.md", "/abs/x.md", "a/../x.md", ".env"] {
            assert_eq!(classify(p, None), Class::Software, "{p}");
        }
    }

    #[test]
    fn markdown_and_scratch_are_system_operation() {
        assert_eq!(classify("notes/plan.md", None), Class::System);
        assert_eq!(classify("scratch/a.txt", None), Class::System);
        assert_eq!(classify("scratch/run.sh", None), Class::Software);
    }

    #[test]
    fn governed_paths_stay_ratified_even_for_markdown() {
        for p in ["specs/070-x.md", "memory/a.md", "agents/b/AGENT.md", "skills/s/SKILL.md", "themes/t/theme.md", "GOVERNANCE.md", "AGENTS.md", "Specs/X.md"] {
            assert_eq!(classify(p, None), Class::Governed, "{p}");
        }
    }

    #[test]
    fn artifacts_are_a_closed_allow_list() {
        for p in ["deck.pdf", "img/a.PNG", "clip.mp4", "data.csv"] {
            assert_eq!(classify(p, None), Class::Artifact, "{p}");
        }
    }

    #[test]
    fn active_content_and_unreadable_content_are_software() {
        assert_eq!(classify("a.svg", Some(b"<svg><rect/></svg>")), Class::Artifact);
        assert_eq!(classify("a.svg", Some(b"<svg><script>x()</script></svg>")), Class::Software);
        assert_eq!(classify("a.svg", Some(b"<svg onload=\"x()\"/>")), Class::Software);
        assert_eq!(classify("a.html", Some(b"<a href='javascript:x()'>")), Class::Software);
        assert_eq!(classify("a.html", None), Class::Software);
        assert_eq!(classify("a.docx", Some(b"PK word/vbaProject.bin")), Class::Software);
        assert_eq!(classify("a.docx", Some(b"PK word/document.xml")), Class::Artifact);
    }

    #[test]
    fn log_holds_only_the_redacted_operator_prompt_and_chains() {
        let d = tempfile::tempdir().unwrap();
        let log = PromptLog::open(&d.path().join("local/artifact-log")).unwrap();
        let r = Redactor::new();
        let e = log.append(&r, "t1", "deck.pdf", Op::Create, "make a deck, key ghp_abcdefghijklmnopqrstuvwxyz0123456789", Some("ab12")).unwrap();
        assert!(e.prompt.contains("[REDACTED]") && !e.prompt.contains("ghp_"));
        log.append(&r, "t1", "deck.pdf", Op::Edit, "shorten it", None).unwrap();
        assert_eq!(log.verify(), Ok(2));
        let raw = fs::read_to_string(log.path()).unwrap();
        let keys: Vec<String> = serde_json::from_str::<serde_json::Value>(raw.lines().next().unwrap()).unwrap().as_object().unwrap().keys().cloned().collect();
        for k in &keys {
            assert!(["ts", "task_id", "artifact", "op", "prompt", "artifact_hash", "prev", "hash"].contains(&k.as_str()), "{k}");
        }
    }

    #[test]
    fn empty_prompt_is_refused_and_tampering_is_detected() {
        let d = tempfile::tempdir().unwrap();
        let log = PromptLog::open(d.path()).unwrap();
        let r = Redactor::new();
        assert!(log.append(&r, "t", "a.pdf", Op::Create, "  ", None).is_err());
        log.append(&r, "t", "a.pdf", Op::Create, "one", None).unwrap();
        log.append(&r, "t", "a.pdf", Op::Edit, "two", None).unwrap();
        let raw = fs::read_to_string(log.path()).unwrap().replace("\"one\"", "\"uno\"");
        fs::write(log.path(), raw).unwrap();
        assert!(log.verify().unwrap_err().contains("entry 1"));
    }
}
