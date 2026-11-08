//! A fake forge for tests: a pull-request world in memory, with a log of everything that was done to it.

use super::forge::{Check, Forge, ForgeError, PrInfo};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct State {
    pub prs: BTreeMap<u64, PrInfo>,
    pub paths: BTreeMap<u64, Vec<String>>,
    pub checks: BTreeMap<u64, Vec<Check>>,
    /// `(ref or sha, path)` to content.
    pub files: BTreeMap<(String, String), String>,
    pub merged: Vec<(u64, String)>,
    pub comments: Vec<(u64, String)>,
    pub labels: Vec<(u64, String, bool)>,
    /// When set, every merge fails with this detail (as if the head moved).
    pub fail_merge: Option<String>,
    /// The first N reads of a pull request report `UNKNOWN` mergeability.
    pub unknown_reads: BTreeMap<u64, usize>,
    pub pr_reads: usize,
    pub diff: String,
    /// After the next read of a pull request, its head moves to this commit (a push lands after the record was derived).
    pub push_after_read: Option<String>,
    /// After any merge, every other open pull request becomes `CONFLICTING` (the base moved under them).
    pub conflict_others_on_merge: bool,
}

#[derive(Default)]
pub struct FakeForge {
    pub st: RefCell<State>,
}

pub const APPROVED_SPEC: &str = "---\nid: spec-094\nstatus: approved\napproved_by: operator (2026-11-07)\n---\n# Spec\n";
pub const DRAFT_SPEC: &str = "---\nid: spec-094\nstatus: draft\n---\n# Spec\n";
pub const SPEC_PATH: &str = "specs/094-core-repo-merge-flow.md";

impl FakeForge {
    /// An open, queued, mergeable pull request with green checks, an approved spec on `main`, and these changed paths.
    pub fn with_ready_pr(&self, n: u64, head: &str, paths: &[&str]) {
        let mut s = self.st.borrow_mut();
        s.prs.insert(
            n,
            PrInfo {
                number: n,
                title: format!("PR {n}"),
                state: "OPEN".into(),
                is_draft: false,
                mergeable: "MERGEABLE".into(),
                head_sha: head.into(),
                head_ref: format!("branch-{n}"),
                base_ref: "main".into(),
                base_sha: "base0".into(),
                labels: vec!["merge-queue".into()],
                additions: 500,
                deletions: 10,
                body: String::new(),
                commit_messages: vec![format!("Do the thing\n\nImplements-Spec: {SPEC_PATH}\n\nCo-Authored-By: x")],
                created_at: format!("2026-11-07T10:{:02}:00Z", n % 60),
            },
        );
        s.paths.insert(n, paths.iter().map(|p| p.to_string()).collect());
        s.checks.insert(n, vec![Check { name: "test".into(), bucket: "pass".into() }, Check { name: "ui".into(), bucket: "pass".into() }]);
        s.files.insert(("main".into(), SPEC_PATH.into()), APPROVED_SPEC.into());
        s.files.insert((head.into(), SPEC_PATH.into()), APPROVED_SPEC.into());
    }

    pub fn edit(&self, n: u64, f: impl FnOnce(&mut PrInfo)) {
        f(self.st.borrow_mut().prs.get_mut(&n).expect("pr"));
    }
}

impl Forge for FakeForge {
    fn pr(&self, n: u64) -> Result<PrInfo, ForgeError> {
        let mut s = self.st.borrow_mut();
        s.pr_reads += 1;
        let left = s.unknown_reads.get(&n).copied().unwrap_or(0);
        let mut p = s.prs.get(&n).cloned().ok_or(ForgeError::Failed { what: "read the pull request".into(), detail: format!("no pull request #{n}") })?;
        if left > 0 {
            s.unknown_reads.insert(n, left - 1);
            p.mergeable = "UNKNOWN".into();
        }
        if let Some(new_head) = s.push_after_read.take() {
            if let Some(stored) = s.prs.get_mut(&n) {
                stored.head_sha = new_head;
            }
        }
        Ok(p)
    }
    fn changed_paths(&self, n: u64) -> Result<Vec<String>, ForgeError> {
        Ok(self.st.borrow().paths.get(&n).cloned().unwrap_or_default())
    }
    fn checks(&self, n: u64) -> Result<Vec<Check>, ForgeError> {
        Ok(self.st.borrow().checks.get(&n).cloned().unwrap_or_default())
    }
    fn file_at(&self, git_ref: &str, path: &str) -> Result<Option<String>, ForgeError> {
        Ok(self.st.borrow().files.get(&(git_ref.to_string(), path.to_string())).cloned())
    }
    fn queued(&self, label: &str) -> Result<Vec<u64>, ForgeError> {
        let s = self.st.borrow();
        let mut rows: Vec<(String, u64)> = s.prs.values().filter(|p| p.state == "OPEN" && p.labels.iter().any(|l| l == label)).map(|p| (p.created_at.clone(), p.number)).collect();
        rows.sort();
        Ok(rows.into_iter().map(|(_, n)| n).collect())
    }
    fn diff(&self, _n: u64) -> Result<String, ForgeError> {
        Ok(self.st.borrow().diff.clone())
    }
    fn merge(&self, n: u64, expect_head: &str) -> Result<(), ForgeError> {
        let mut s = self.st.borrow_mut();
        if let Some(d) = s.fail_merge.clone() {
            return Err(ForgeError::Failed { what: "merge".into(), detail: d });
        }
        let head = s.prs.get(&n).map(|p| p.head_sha.clone()).unwrap_or_default();
        if head != expect_head {
            return Err(ForgeError::Failed { what: "merge".into(), detail: "Head branch was modified. Review and try the merge again.".into() });
        }
        s.merged.push((n, expect_head.to_string()));
        if let Some(p) = s.prs.get_mut(&n) {
            p.state = "MERGED".into();
        }
        if s.conflict_others_on_merge {
            for (k, p) in s.prs.iter_mut() {
                if *k != n && p.state == "OPEN" {
                    p.mergeable = "CONFLICTING".into();
                }
            }
        }
        Ok(())
    }
    fn comment(&self, n: u64, body: &str) -> Result<(), ForgeError> {
        self.st.borrow_mut().comments.push((n, body.to_string()));
        Ok(())
    }
    fn set_label(&self, n: u64, label: &str, on: bool) -> Result<(), ForgeError> {
        let mut s = self.st.borrow_mut();
        s.labels.push((n, label.to_string(), on));
        if let Some(p) = s.prs.get_mut(&n) {
            p.labels.retain(|l| l != label);
            if on {
                p.labels.push(label.to_string());
            }
        }
        Ok(())
    }
}
