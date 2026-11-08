//! The approval record and the ledger (spec 094 §3.2).
//!
//! Everything in a `Record` except the attestation and the quote is **derived** from git and `gh` at the moment of the merge;
//! nobody types a head commit or a path list. The ledger is append-only JSON Lines under `$DELPHI_HOME/merge/` (directory
//! `0700`, file `0600`). Each line says what was decided; it is evidence, not an authority.

use regex::Regex;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// How the operator's approval reached the command (spec 094 §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attestation {
    /// The operator typed the PR number at a terminal.
    Tty,
    /// An agent ran the command because the operator's turn said to, passing the operator's words. **Self-attested by the
    /// caller**, and recorded as such wherever it appears.
    ChatQuote(String),
}

impl Attestation {
    pub fn label(&self) -> &'static str {
        match self {
            Attestation::Tty => "tty",
            Attestation::ChatQuote(_) => "chat-quote",
        }
    }
    pub fn quote(&self) -> Option<&str> {
        match self {
            Attestation::Tty => None,
            Attestation::ChatQuote(q) => Some(q),
        }
    }
}

/// How the linked spec stood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecState {
    /// Approved on the base branch; `blake3` is the hash of the file at the head.
    Approved { path: String, blake3: String },
    /// The pull request itself approves the spec it implements (a Stage 1 spec PR).
    SpecPr { path: String, blake3: String },
    /// No `Implements-Spec:` trailer, and the change is inside the bounded exemption of `specs/003` §3.1.2.
    Exempt,
    /// No trailer, and every changed file is Markdown outside the governed `agents/`, `skills/`, `themes/` and `memory/`
    /// directories: documents and Stage 1 spec pull requests are not software (`specs/070`, AGENTS.md).
    MarkdownOnly,
    /// Not mergeable: the reason.
    Missing(String),
}

impl SpecState {
    pub fn to_json(&self) -> Value {
        match self {
            SpecState::Approved { path, blake3 } => json!({"kind": "approved-on-base", "path": path, "blake3": blake3}),
            SpecState::SpecPr { path, blake3 } => json!({"kind": "spec-pr", "path": path, "blake3": blake3}),
            SpecState::Exempt => json!({"kind": "bounded-exemption"}),
            SpecState::MarkdownOnly => json!({"kind": "markdown-only"}),
            SpecState::Missing(why) => json!({"kind": "missing", "why": why}),
        }
    }
}

/// What was derived about one pull request at the moment of the decision.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub pr: u64,
    pub head_sha: String,
    pub base_ref: String,
    pub base_sha: String,
    pub paths_count: usize,
    pub paths_hash: String,
    pub spec: SpecState,
    /// `(check name, bucket)`.
    pub checks: Vec<(String, String)>,
    pub gate_changing: bool,
    pub attestation: Attestation,
    pub session: Option<String>,
    pub time: String,
}

impl Record {
    pub fn to_json(&self, kind: &str) -> Value {
        json!({
            "kind": kind,
            "time": self.time,
            "pr": self.pr,
            "head_sha": self.head_sha,
            "base_ref": self.base_ref,
            "base_sha": self.base_sha,
            "paths_count": self.paths_count,
            "paths_hash": self.paths_hash,
            "spec": self.spec.to_json(),
            "checks": self.checks.iter().map(|(n, b)| json!({"name": n, "bucket": b})).collect::<Vec<_>>(),
            "gate_changing": self.gate_changing,
            "attestation": self.attestation.label(),
            "self_attested": matches!(self.attestation, Attestation::ChatQuote(_)),
            "quote": self.attestation.quote(),
            "session": self.session,
        })
    }

    /// The comment left on the pull request, so the decision is visible in plain GitHub without Delphi.
    pub fn comment(&self) -> String {
        let how = match &self.attestation {
            Attestation::Tty => "the operator typed the PR number at a terminal (`tty`)".to_string(),
            Attestation::ChatQuote(q) => format!("an agent ran the command on the operator's instruction, quoted verbatim: \"{}\" (`chat-quote`, **self-attested by the caller**)", q.replace('"', "'")),
        };
        format!(
            "Merged by `delphi merge` (spec 094).\n\n- head commit: `{}`\n- base: `{}` at `{}`\n- changed paths: {} (hash `{}`)\n- approval: {}\n{}",
            self.head_sha,
            self.base_ref,
            self.base_sha,
            self.paths_count,
            self.paths_hash,
            how,
            self.session.as_ref().map(|s| format!("- session: `{s}`\n")).unwrap_or_default()
        )
    }
}

/// A stable hash of a set of paths: sorted, de-duplicated, newline-joined.
pub fn paths_hash(paths: &[String]) -> String {
    let mut p: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    p.sort_unstable();
    p.dedup();
    blake3::hash(p.join("\n").as_bytes()).to_hex().to_string()
}

pub fn blake3_hex(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

/// The specs a pull request says it implements, from `Implements-Spec:` trailers in its commit messages and its body.
pub fn spec_links(commit_messages: &[String], body: &str) -> Vec<String> {
    let re = Regex::new(r"(?m)^Implements-Spec:\s*(specs/[A-Za-z0-9._/-]+\.md)\s*$").expect("static pattern");
    let mut out: Vec<String> = Vec::new();
    for text in commit_messages.iter().map(|s| s.as_str()).chain(std::iter::once(body)) {
        for c in re.captures_iter(text) {
            let p = c[1].to_string();
            if !p.contains("..") && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// `status: approved` in a spec's frontmatter, and whether `approved_by:` is present.
pub fn spec_approval(text: &str) -> (bool, bool) {
    let mut approved = false;
    let mut by = false;
    let mut in_front = false;
    for (i, line) in text.lines().enumerate() {
        if line.trim_end() == "---" {
            if i == 0 {
                in_front = true;
                continue;
            }
            break;
        }
        if in_front {
            let l = line.trim_start();
            if let Some(rest) = l.strip_prefix("status:") {
                approved = rest.split('#').next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'') == "approved";
            }
            if l.starts_with("approved_by:") {
                by = true;
            }
        }
    }
    (approved, by)
}

/// The append-only ledger.
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub fn at(home: &Path) -> Ledger {
        Ledger { path: home.join("merge").join("ledger.jsonl") }
    }
    // `path` and `read` are used by tests now and by `delphi merge log` in slice S5 (spec 094 §3.6).
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn append(&self, entry: &Value) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        let mut f = OpenOptions::new().create(true).append(true).mode(0o600).open(&self.path)?;
        let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
        line.push('\n');
        f.write_all(line.as_bytes())?;
        f.sync_all()
    }
    /// Every entry, oldest first; a line that does not parse is skipped, not fatal.
    #[allow(dead_code)]
    pub fn read(&self) -> Vec<Value> {
        fs::read_to_string(&self.path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_hash_ignores_order_and_duplicates_and_changes_with_content() {
        let a = paths_hash(&["b".into(), "a".into(), "a".into()]);
        assert_eq!(a, paths_hash(&["a".into(), "b".into()]));
        assert_ne!(a, paths_hash(&["a".into(), "c".into()]));
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn spec_links_are_read_from_trailers_in_commits_and_the_body_only() {
        let msgs = vec!["Subject\n\nBody\n\nImplements-Spec: specs/094-core-repo-merge-flow.md\n\nCo-Authored-By: x".to_string(), "No trailer here".to_string()];
        assert_eq!(spec_links(&msgs, ""), vec!["specs/094-core-repo-merge-flow.md".to_string()]);
        assert_eq!(spec_links(&[], "Implements-Spec: specs/093-a.md"), vec!["specs/093-a.md".to_string()]);
        // Prose that merely mentions a spec is not a link; neither is a path that climbs out.
        assert!(spec_links(&["see Implements-Spec: specs/x.md in the text".to_string()], "").is_empty());
        assert!(spec_links(&["Implements-Spec: specs/../etc/x.md".to_string()], "").is_empty());
        // The same spec twice is one link.
        assert_eq!(spec_links(&["Implements-Spec: specs/a.md".to_string(), "Implements-Spec: specs/a.md".to_string()], "").len(), 1);
    }

    #[test]
    fn approval_is_read_from_the_frontmatter_status_and_approved_by() {
        let ok = "---\nid: spec-1\nstatus: approved   # by the operator\napproved_by: operator (2026-11-07)\n---\n# T\nstatus: draft\n";
        assert_eq!(spec_approval(ok), (true, true));
        assert_eq!(spec_approval("---\nstatus: draft\n---\nbody"), (false, false));
        assert_eq!(spec_approval("---\nstatus: approved\n---\nbody"), (true, false));
        // A `status: approved` line outside the frontmatter proves nothing.
        assert_eq!(spec_approval("# T\nstatus: approved\napproved_by: x\n"), (false, false));
        assert_eq!(spec_approval("---\nstatus: \"approved\"\n---\n"), (true, false));
    }

    #[test]
    fn the_comment_labels_a_chat_quote_as_self_attested_and_carries_the_derived_facts() {
        let r = Record {
            pr: 7,
            head_sha: "abc".into(),
            base_ref: "main".into(),
            base_sha: "def".into(),
            paths_count: 2,
            paths_hash: "h".into(),
            spec: SpecState::Exempt,
            checks: vec![("test".into(), "pass".into())],
            gate_changing: false,
            attestation: Attestation::ChatQuote("merge 7 \"now\"".into()),
            session: Some("s1".into()),
            time: "t".into(),
        };
        let c = r.comment();
        assert!(c.contains("self-attested") && c.contains("`abc`") && c.contains("merge 7 'now'") && c.contains("session: `s1`"), "{c}");
        let j = r.to_json("approved");
        assert_eq!((j["self_attested"].as_bool(), j["attestation"].as_str(), j["pr"].as_u64()), (Some(true), Some("chat-quote"), Some(7)));
    }

    #[test]
    fn the_ledger_appends_privately_and_reads_back_in_order() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let l = Ledger::at(t.path());
        l.append(&json!({"kind": "approved", "pr": 1})).unwrap();
        l.append(&json!({"kind": "merged", "pr": 1})).unwrap();
        let rows = l.read();
        assert_eq!(rows.iter().map(|r| r["kind"].as_str().unwrap()).collect::<Vec<_>>(), vec!["approved", "merged"]);
        assert_eq!(fs::metadata(l.path()).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(l.path().parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
