//! Which checks speak for the head (spec 094 §18): only the newest run of each workflow on the pull request's current head is
//! judged, as GitHub's required check does; what was left out is said. Fake forge, no network.

use super::approve::FakeTerminal;
use super::check::evaluate;
use super::errors::Errors;
use super::fake::FakeForge;
use super::forge::Check;
use super::record::Ledger;
use super::{merge_one, status_lines, wait, Ctx, Outcome};
use std::time::Duration;

const HEAD: &str = "head7";
/// The ready run (#909's 45431007078) and the draft-time run GitHub cancelled when it started (45430975730).
const NEW: u64 = 45431007078;
const OLD: u64 = 45430975730;

fn ci(name: &str, bucket: &str, run: u64) -> Check {
    Check { name: name.into(), bucket: bucket.into(), workflow: "ci".into(), event: "pull_request".into(), run: Some(run) }
}

/// A ready pull request #7 whose checks are `checks`, every run on `HEAD` unless `elsewhere` says otherwise.
fn forge(checks: Vec<Check>, elsewhere: &[(u64, &str)]) -> FakeForge {
    let f = FakeForge::default();
    f.with_ready_pr(7, HEAD, &["a.rs"]);
    let mut s = f.st.borrow_mut();
    for r in checks.iter().filter_map(|c| c.run) {
        s.run_heads.insert(r, HEAD.into());
    }
    for (r, h) in elsewhere {
        s.run_heads.insert(*r, h.to_string());
    }
    s.checks.insert(7, checks);
    drop(s);
    f
}

/// #909 as `gh pr checks` listed it on 2026-11-24: the ready run all green, beside one CANCELLED entry of the draft run.
fn pr_909() -> Vec<Check> {
    vec![ci("ci-gate", "pass", NEW), ci("macos tests (1)", "pass", NEW), ci("macos tests (2)", "pass", NEW), ci("lint (ubuntu-latest)", "pass", NEW), ci("macos tests", "cancel", OLD)]
}

fn merge(f: &FakeForge) -> (Outcome, Vec<String>) {
    let home = tempfile::tempdir().unwrap();
    let (ledger, errors) = (Ledger::at(home.path()), Errors::at(home.path()));
    let mut term = FakeTerminal::new(false, &[]);
    let mut ctx = Ctx { forge: f, ledger: &ledger, errors: &errors, term: &mut term, quote: Some("merge #7".into()), dry_run: false, session: None, git: None, settle: Duration::ZERO, now: || "2026-11-24T00:00:00Z".to_string(), refresh: None, claims: None, rollup: None };
    let out = merge_one(&mut ctx, 7, None);
    (out, term.shown)
}

#[test]
fn the_909_case_a_stale_cancelled_entry_beside_a_green_newest_run_merges_and_says_what_it_ignored() {
    let f = forge(pr_909(), &[]);
    let v = evaluate(&f, 7).unwrap();
    assert!(v.ready(), "{:?}", v.refusals);
    assert!(v.checks.iter().all(|c| c.run == Some(NEW)) && v.checks.len() == 4, "{:?}", v.checks);
    assert_eq!(v.check_notes.len(), 1);
    assert!(v.check_notes[0].contains("ignored 1 check entry from superseded run 45430975730"), "{:?}", v.check_notes);
    assert_eq!(f.st.borrow().run_head_reads, 2, "one read per distinct run");
    let line = status_lines(&f).unwrap().remove(0);
    assert!(line.starts_with("#7: ready") && line.contains("superseded run 45430975730"), "{line}");
    let (out, shown) = merge(&f);
    assert_eq!(out, Outcome::Merged { pr: 7, head: HEAD.into() });
    assert!(shown.iter().any(|l| l.contains("#7: ignored 1 check entry from superseded run 45430975730")), "{shown:?}");
}

#[test]
fn a_cancelled_or_failed_entry_in_the_newest_run_still_refuses() {
    for bucket in ["cancel", "fail"] {
        let mut checks = pr_909();
        checks.push(ci("macos build", bucket, NEW));
        let f = forge(checks, &[]);
        let (out, _) = merge(&f);
        match out {
            Outcome::Refused { reasons, .. } => {
                assert!(reasons.iter().any(|r| r.starts_with("checks failed: macos build") && r.contains("superseded run 45430975730")), "{bucket}: {reasons:?}")
            }
            other => panic!("{bucket}: {other:?}"),
        }
        assert!(f.st.borrow().merged.is_empty());
    }
}

#[test]
fn an_older_failed_run_superseded_by_a_newer_green_run_merges() {
    let checks = vec![ci("test", "fail", OLD), ci("lint", "fail", OLD), ci("test", "pass", NEW), ci("lint", "pass", NEW)];
    let f = forge(checks, &[]);
    let v = evaluate(&f, 7).unwrap();
    assert!(v.ready(), "{:?}", v.refusals);
    assert!(v.check_notes[0].contains("ignored 2 check entries from superseded run"), "{:?}", v.check_notes);
    assert!(matches!(merge(&f).0, Outcome::Merged { .. }));
}

#[test]
fn a_newer_run_still_running_is_pending_even_when_the_older_one_was_green() {
    let checks = vec![ci("test", "pass", OLD), ci("ci-gate", "pass", OLD), ci("test", "pending", NEW)];
    let f = forge(checks, &[]);
    let v = evaluate(&f, 7).unwrap();
    assert!(!v.ready());
    assert!(v.refusals.iter().any(|r| r.starts_with("checks are still running: test") && wait::not_yet(r)), "{:?}", v.refusals);
    assert!(v.checks.iter().all(|c| c.run == Some(NEW)), "the older run's green ci-gate never counts: {:?}", v.checks);
}

#[test]
fn checks_from_a_run_on_another_head_never_count() {
    // A green run on an older commit beside a red one on the head: red.
    let f = forge(vec![ci("test", "pass", NEW), ci("test", "fail", OLD)], &[(NEW, "older-head")]);
    let v = evaluate(&f, 7).unwrap();
    assert!(v.refusals.iter().any(|r| r.starts_with("checks failed: test")), "{:?}", v.refusals);
    assert!(v.check_notes.iter().any(|n| n.contains("from run 45431007078 on head `older-he") && n.contains("not the current head")), "{:?}", v.check_notes);
    // Only stale successes from another head: no checks yet, never green.
    let f = forge(vec![ci("test", "pass", NEW), ci("ci-gate", "pass", NEW)], &[(NEW, "older-head")]);
    let v = evaluate(&f, 7).unwrap();
    assert!(v.checks.is_empty() && v.refusals.iter().any(|r| r.starts_with(super::check::NO_CHECKS_MARK)), "{:?}", v.refusals);
    assert!(matches!(merge(&f).0, Outcome::Refused { .. }));
}

#[test]
fn a_run_whose_head_cannot_be_read_is_an_error_not_a_pass() {
    let f = forge(pr_909(), &[]);
    f.st.borrow_mut().run_heads.remove(&OLD);
    assert!(evaluate(&f, 7).is_err());
    assert!(matches!(merge(&f).0, Outcome::Failed { .. }));
    assert!(f.st.borrow().merged.is_empty());
}

#[test]
fn checks_without_a_run_are_judged_as_they_are_and_workflows_and_events_are_grouped_apart() {
    // A status from another service has no run: always kept. A red one refuses.
    let mut checks = pr_909();
    checks.push(Check { name: "external".into(), bucket: "fail".into(), ..Default::default() });
    let v = evaluate(&forge(checks, &[]), 7).unwrap();
    assert!(v.refusals.iter().any(|r| r.starts_with("checks failed: external")), "{:?}", v.refusals);
    // An older run of another workflow, or of the same workflow for another event, is not superseded by `ci`'s newer run.
    let other_wf = Check { workflow: "docs".into(), ..ci("docs build", "fail", OLD) };
    let other_event = Check { event: "push".into(), ..ci("push job", "fail", OLD - 1) };
    let v = evaluate(&forge(vec![ci("test", "pass", NEW), other_wf, other_event], &[]), 7).unwrap();
    assert!(v.refusals.iter().any(|r| r.contains("docs build") && r.contains("push job")), "{:?}", v.refusals);
    assert!(v.check_notes.is_empty(), "{:?}", v.check_notes);
}

#[test]
fn too_many_runs_is_an_error_not_a_pass() {
    let checks: Vec<Check> = (1..=17).map(|r| ci(&format!("j{r}"), "pass", r)).collect();
    let f = forge(checks, &[]);
    assert!(evaluate(&f, 7).is_err());
    assert_eq!(f.st.borrow().run_head_reads, 0, "refused before reading any run");
}
