//! The merge error log (spec 094 §3.6): every pull request that cannot merge for a reason somebody must act on leaves an entry,
//! with a class, so the builder can fix it and the same trouble seen twice can become a rule.
//!
//! `$DELPHI_HOME/merge/errors.jsonl` is append-only JSON Lines (directory `0700`, file `0600`). A line is one of: `error` (a new
//! entry), `seen` (the same trouble again on the same head commit: one more attempt, not a second entry) or `state` (the entry
//! moved: `open` > `fixing` > `fixed-pending-ci` > `resolved`, or `needs-human`). The current state of an entry is the fold of its
//! lines. What is a *state of the world* and not a problem (a draft, a pull request that is not queued, checks still running, GitHub
//! still working out mergeability) is never logged: waiting is not an error.

use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub const FILE: &str = "errors.jsonl";
pub const STATES: &[&str] = &["open", "fixing", "fixed-pending-ci", "needs-human", "resolved"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Conflict,
    CiRed,
    NoChecks,
    Spec,
    GateList,
    StaleRecord,
    Forge,
    Ledger,
    MergeFailed,
    /// A step after a merge could not finish (a locked worktree, a branch another pull request is based on). Recorded when the
    /// cleanup after a merge returns a problem; it stays open, because only a person can settle it.
    Cleanup,
}

impl Class {
    pub fn slug(self) -> &'static str {
        match self {
            Class::Conflict => "conflict",
            Class::CiRed => "ci-red",
            Class::NoChecks => "no-checks",
            Class::Spec => "spec",
            Class::GateList => "gate-list",
            Class::StaleRecord => "stale-record",
            Class::Forge => "forge",
            Class::Ledger => "ledger",
            Class::MergeFailed => "merge-failed",
            Class::Cleanup => "cleanup",
        }
    }
    pub fn from_slug(s: &str) -> Option<Class> {
        [Class::Conflict, Class::CiRed, Class::NoChecks, Class::Spec, Class::GateList, Class::StaleRecord, Class::Forge, Class::Ledger, Class::MergeFailed, Class::Cleanup].into_iter().find(|c| c.slug() == s)
    }
    /// Can a builder fix this by changing the branch? (Others are for a person: a missing `gh`, a damaged list.)
    pub fn builder_can_fix(self) -> bool {
        matches!(self, Class::Conflict | Class::CiRed | Class::NoChecks | Class::Spec | Class::StaleRecord | Class::MergeFailed)
    }
}

/// What a refusal reason is, if it is a problem. `None` means a state of the world to wait out or act on, not an error.
pub fn classify_refusal(reason: &str) -> Option<Class> {
    if reason.contains("conflicts with") {
        Some(Class::Conflict)
    } else if reason.starts_with("checks failed") {
        Some(Class::CiRed)
    } else if reason.starts_with("no checks are reported") {
        Some(Class::NoChecks)
    } else if reason.contains("gate list") {
        Some(Class::GateList)
    } else if reason.contains("Implements-Spec") || reason.contains("is not approved on") || reason.contains("does not exist on the pull request") || reason.contains("changes it to a state that is not approved") {
        Some(Class::Spec)
    } else {
        // A draft, a closed pull request, no queue label, checks still running, GitHub still computing: not problems.
        None
    }
}

/// What a failed attempt to merge is.
pub fn classify_failure(detail: &str) -> Class {
    let d = detail.to_lowercase();
    if d.contains("head branch was modified") || d.contains("head ref") || d.contains("expected head") {
        Class::StaleRecord
    } else if d.contains("cannot run `gh`") || d.contains("gh auth") || d.contains("authentication") || d.contains("could not resolve host") || d.contains("network") || d.contains("timed out") || d.contains("http 5") {
        Class::Forge
    } else if d.contains("ledger") {
        Class::Ledger
    } else {
        Class::MergeFailed
    }
}

/// One entry, folded from its lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub pr: u64,
    pub head_sha: String,
    pub class: String,
    pub detail: String,
    pub attempts: u32,
    pub state: String,
    pub first: String,
    pub last: String,
    pub note: String,
}

impl Entry {
    pub fn open(&self) -> bool {
        self.state != "resolved"
    }
    pub fn line(&self) -> String {
        format!("{} #{} {} [{}] x{} {}{}", self.id, self.pr, self.class, self.state, self.attempts, self.detail, if self.note.is_empty() { String::new() } else { format!(" ({})", self.note) })
    }
}

pub struct Errors {
    path: PathBuf,
    home: PathBuf,
}

impl Errors {
    pub fn at(home: &Path) -> Errors {
        Errors { path: home.join("merge").join(FILE), home: home.to_path_buf() }
    }

    fn append(&self, row: &Value) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        let mut f = OpenOptions::new().create(true).append(true).mode(0o600).open(&self.path)?;
        let mut line = serde_json::to_string(row).map_err(std::io::Error::other)?;
        line.push('\n');
        f.write_all(line.as_bytes())?;
        f.sync_all()
    }

    /// Record a problem. The same problem on the same head commit while its entry is still open is one more attempt, not a new
    /// entry. Returns the entry as it now stands.
    pub fn record(&self, pr: u64, head_sha: &str, class: Class, detail: &str, now: &str) -> std::io::Result<Entry> {
        let detail = delphi_core::elog::sanitize(detail, 400, Some(&self.home));
        if let Some(e) = self.entries().into_iter().find(|e| e.open() && e.pr == pr && e.class == class.slug() && e.head_sha == head_sha) {
            self.append(&json!({"kind": "seen", "id": e.id, "time": now, "detail": detail}))?;
            return Ok(self.entries().into_iter().find(|x| x.id == e.id).expect("just written"));
        }
        let n = self.read().iter().filter(|r| r["kind"] == "error").count() + 1;
        let id = format!("e{n}");
        self.append(&json!({"kind": "error", "id": id, "time": now, "pr": pr, "head_sha": head_sha, "class": class.slug(), "detail": detail}))?;
        Ok(self.entries().into_iter().find(|x| x.id == id).expect("just written"))
    }

    pub fn set_state(&self, id: &str, state: &str, note: &str, now: &str) -> Result<(), String> {
        if !STATES.contains(&state) {
            return Err(format!("`{state}` is not a state (one of {})", STATES.join(", ")));
        }
        if !self.entries().iter().any(|e| e.id == id) {
            return Err(format!("no error entry `{id}`"));
        }
        let note = delphi_core::elog::sanitize(note, 300, Some(&self.home));
        self.append(&json!({"kind": "state", "id": id, "state": state, "note": note, "time": now})).map_err(|e| e.to_string())
    }

    /// The pull request merged: everything open for it is resolved.
    pub fn resolve_pr(&self, pr: u64, now: &str) {
        for e in self.entries().into_iter().filter(|e| e.open() && e.pr == pr) {
            let _ = self.set_state(&e.id, "resolved", "merged", now);
        }
    }

    fn read(&self) -> Vec<Value> {
        fs::read_to_string(&self.path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    /// Every entry, oldest first, in its current state.
    pub fn entries(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        for r in self.read() {
            let id = r["id"].as_str().unwrap_or("").to_string();
            let time = r["time"].as_str().unwrap_or("").to_string();
            match r["kind"].as_str() {
                Some("error") => out.push(Entry {
                    id,
                    pr: r["pr"].as_u64().unwrap_or(0),
                    head_sha: r["head_sha"].as_str().unwrap_or("").into(),
                    class: r["class"].as_str().unwrap_or("").into(),
                    detail: r["detail"].as_str().unwrap_or("").into(),
                    attempts: 1,
                    state: "open".into(),
                    first: time.clone(),
                    last: time,
                    note: String::new(),
                }),
                Some("seen") => {
                    if let Some(e) = out.iter_mut().find(|e| e.id == id) {
                        e.attempts += 1;
                        e.last = time;
                        if let Some(d) = r["detail"].as_str() {
                            e.detail = d.to_string();
                        }
                    }
                }
                Some("state") => {
                    if let Some(e) = out.iter_mut().find(|e| e.id == id) {
                        e.state = r["state"].as_str().unwrap_or("open").to_string();
                        e.note = r["note"].as_str().unwrap_or("").to_string();
                        e.last = time;
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// Write the problem to the central error log too (`delphi logs`), with only allow-listed context keys.
pub fn log_central(pr: u64, class: Class, attempts: u32) {
    delphi_core::elog::error("cli", &format!("merge_{}", class.slug().replace('-', "_")), &format!("pull request #{pr} could not merge: {}", class.slug()), json!({"command": "merge", "kind": class.slug(), "count": attempts}));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_that_are_problems_are_classed_and_states_of_the_world_are_not() {
        for (r, want) in [
            ("it conflicts with `main`: merge the base into the branch and resolve", Some(Class::Conflict)),
            ("checks failed: ui, test", Some(Class::CiRed)),
            ("no checks are reported on the head commit (a pull request that conflicts ...)", Some(Class::NoChecks)),
            ("the gate list on `main` cannot be used, so nothing can be judged: paths is empty", Some(Class::GateList)),
            ("specs/094.md is not approved on main (it needs `status: approved`)", Some(Class::Spec)),
            ("no `Implements-Spec:` trailer, and the change is neither only Markdown nor inside the bounded exemption", Some(Class::Spec)),
            ("it is a draft", None),
            ("it is merged, not open", None),
            ("it is not queued (no `merge-queue` label)", None),
            ("checks are still running: test", None),
            ("GitHub has not finished working out whether it merges (`UNKNOWN`)", None),
        ] {
            assert_eq!(classify_refusal(r), want, "{r}");
        }
    }

    #[test]
    fn every_class_round_trips_through_its_slug() {
        for c in [Class::Conflict, Class::CiRed, Class::NoChecks, Class::Spec, Class::GateList, Class::StaleRecord, Class::Forge, Class::Ledger, Class::MergeFailed, Class::Cleanup] {
            assert_eq!(Class::from_slug(c.slug()), Some(c));
        }
        assert_eq!(Class::from_slug("nonsense"), None);
    }

    #[test]
    fn failures_are_classed_by_what_happened() {
        assert_eq!(classify_failure("Head branch was modified. Review and try the merge again."), Class::StaleRecord);
        assert_eq!(classify_failure("cannot run `gh`: No such file or directory"), Class::Forge);
        assert_eq!(classify_failure("could not resolve host: api.github.com"), Class::Forge);
        assert_eq!(classify_failure("cannot write the ledger (disk full); not merging without a record"), Class::Ledger);
        assert_eq!(classify_failure("something else went wrong"), Class::MergeFailed);
        assert!(Class::Conflict.builder_can_fix() && Class::StaleRecord.builder_can_fix());
        assert!(!Class::Forge.builder_can_fix() && !Class::GateList.builder_can_fix() && !Class::Ledger.builder_can_fix());
    }

    #[test]
    fn an_entry_moves_through_its_states_and_resolves_when_the_pull_request_merges() {
        let t = tempfile::tempdir().unwrap();
        let log = Errors::at(t.path());
        let e = log.record(7, "head1", Class::Conflict, "it conflicts with `main`", "t1").unwrap();
        assert_eq!((e.id.as_str(), e.state.as_str(), e.attempts), ("e1", "open", 1));
        log.set_state("e1", "fixing", "builder started", "t2").unwrap();
        log.set_state("e1", "fixed-pending-ci", "pushed abc", "t3").unwrap();
        let e = &log.entries()[0];
        assert_eq!((e.state.as_str(), e.note.as_str(), e.last.as_str()), ("fixed-pending-ci", "pushed abc", "t3"));
        log.resolve_pr(7, "t4");
        let e = &log.entries()[0];
        assert!(!e.open() && e.state == "resolved" && e.note == "merged");
    }

    #[test]
    fn the_same_trouble_on_the_same_head_is_one_more_attempt_and_a_new_head_is_a_new_entry() {
        let t = tempfile::tempdir().unwrap();
        let log = Errors::at(t.path());
        log.record(7, "head1", Class::CiRed, "checks failed: ui", "t1").unwrap();
        let again = log.record(7, "head1", Class::CiRed, "checks failed: ui, test", "t2").unwrap();
        assert_eq!((again.id.as_str(), again.attempts, again.detail.as_str()), ("e1", 2, "checks failed: ui, test"));
        // A different class, a different head and a different pull request are each their own entry.
        log.record(7, "head1", Class::Conflict, "x", "t3").unwrap();
        log.record(7, "head2", Class::CiRed, "y", "t4").unwrap();
        log.record(8, "head1", Class::CiRed, "z", "t5").unwrap();
        assert_eq!(log.entries().len(), 4);
        // Once resolved, the same trouble later is a new entry, not a revived one.
        log.resolve_pr(7, "t6");
        let fresh = log.record(7, "head1", Class::CiRed, "again", "t7").unwrap();
        assert_eq!((fresh.attempts, fresh.state.as_str()), (1, "open"));
        assert!(fresh.id != "e1");
    }

    #[test]
    fn a_bad_state_or_an_unknown_entry_is_refused_and_details_are_sanitized() {
        let t = tempfile::tempdir().unwrap();
        let log = Errors::at(t.path());
        log.record(7, "h", Class::Forge, &format!("failed at {}/secret/place with token ghp_abcdefghijklmnopqrstuvwxyz0123456789", t.path().display()), "t").unwrap();
        assert!(log.set_state("e1", "weird", "", "t").unwrap_err().contains("not a state"));
        assert!(log.set_state("e99", "fixing", "", "t").unwrap_err().contains("no error entry"));
        let e = &log.entries()[0];
        assert!(!e.detail.contains(&t.path().display().to_string()), "{}", e.detail);
    }

    #[test]
    fn the_file_is_private_and_unreadable_lines_are_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let log = Errors::at(t.path());
        log.record(1, "h", Class::Spec, "d", "t").unwrap();
        let p = t.path().join("merge").join(FILE);
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(b"not json\n").unwrap();
        assert_eq!(log.entries().len(), 1);
    }
}
