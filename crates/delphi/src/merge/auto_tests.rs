//! The `--auto` session against a fake forge and a fake clock (spec 094 AC-5): every stop condition, and the one thing a session
//! must never do, merge a pull request that changes a gate.

use super::approve::FakeTerminal;
use super::auto::{run, Caps, Clock, Summary};
use super::errors::Errors;
use super::fake::FakeForge;
use super::record::{Attestation, Ledger};
use super::Ctx;
use std::cell::Cell;
use std::time::Duration;

/// Time that only moves when the session sleeps.
struct FakeClock(Cell<u64>);

impl Clock for FakeClock {
    fn now_secs(&self) -> u64 {
        self.0.get()
    }
    fn sleep(&self, d: Duration) {
        self.0.set(self.0.get() + d.as_secs());
    }
}

fn caps() -> Caps {
    Caps { max_prs: 10, run_for: Duration::from_secs(4 * 3600), pause_after_errors: 2, idle: Duration::from_secs(1800), poll: Duration::from_secs(60) }
}

fn session(forge: &FakeForge, caps: &Caps, stop: &dyn Fn() -> bool) -> (Summary, tempfile::TempDir, Vec<String>) {
    let home = tempfile::tempdir().unwrap();
    let ledger = Ledger::at(home.path());
    let errors = Errors::at(home.path());
    let mut term = FakeTerminal::new(false, &[]);
    let mut ctx = Ctx { forge, ledger: &ledger, errors: &errors, term: &mut term, quote: None, dry_run: false, git: None, session: None, settle: Duration::ZERO, now: || "2026-11-07T00:00:00Z".to_string() };
    let clock = FakeClock(Cell::new(1_000_000));
    let mut said = Vec::new();
    let s = run(&mut ctx, caps, &Attestation::ChatQuote("run auto for the evening".into()), &clock, stop, &mut |l| said.push(l.to_string()));
    (s, home, said)
}

fn never() -> bool {
    false
}

#[test]
fn it_merges_the_queue_oldest_first_and_then_stops_when_nothing_is_left_to_do() {
    let f = FakeForge::default();
    f.with_ready_pr(3, "h3", &["docs/a.md"]);
    f.with_ready_pr(2, "h2", &["docs/b.md"]);
    let (s, home, _) = session(&f, &caps(), &never);
    assert_eq!(s.merged, vec![2, 3], "oldest created first");
    assert!(s.reason.starts_with("nothing to do"), "{}", s.reason);
    assert_eq!(f.st.borrow().merged.len(), 2);
    // The grant is written first, every merge carries the session id, and the end is recorded.
    let rows = Ledger::at(home.path()).read();
    assert_eq!(rows.first().unwrap()["kind"], "session_start");
    assert_eq!(rows.first().unwrap()["self_attested"], true);
    assert_eq!(rows.last().unwrap()["kind"], "session_end");
    let id = rows[0]["session"].as_str().unwrap().to_string();
    assert!(rows.iter().filter(|r| r["kind"] == "merged").all(|r| r["session"] == id.as_str()), "{rows:?}");
}

#[test]
fn it_never_merges_a_pull_request_that_changes_a_gate() {
    let f = FakeForge::default();
    f.with_ready_pr(2, "h2", &[".github/workflows/ci.yml"]);
    f.with_ready_pr(3, "h3", &["docs/a.md"]);
    let (s, _, _) = session(&f, &caps(), &never);
    assert_eq!(s.merged, vec![3]);
    assert_eq!(s.left_for_a_human, vec![2]);
    assert!(f.st.borrow().merged.iter().all(|(n, _)| *n != 2), "the gate-changing pull request was merged");
}

#[test]
fn it_stops_at_the_cap_on_merged_pull_requests() {
    let f = FakeForge::default();
    for n in 1..=5 {
        f.with_ready_pr(n, &format!("h{n}"), &["docs/a.md"]);
    }
    let c = Caps { max_prs: 2, ..caps() };
    let (s, _, _) = session(&f, &c, &never);
    assert_eq!(s.merged.len(), 2);
    assert!(s.reason.contains("cap of 2"), "{}", s.reason);
    assert_eq!(f.st.borrow().merged.len(), 2);
}

#[test]
fn a_stop_request_ends_it_before_it_merges_anything() {
    let f = FakeForge::default();
    f.with_ready_pr(2, "h2", &["docs/a.md"]);
    let (s, _, _) = session(&f, &caps(), &|| true);
    assert!(s.merged.is_empty() && s.reason.contains("stopped by the operator"), "{s:?}");
    assert!(f.st.borrow().merged.is_empty());
}

#[test]
fn a_stop_request_between_two_pull_requests_leaves_the_second_alone() {
    let f = FakeForge::default();
    f.with_ready_pr(2, "h2", &["docs/a.md"]);
    f.with_ready_pr(3, "h3", &["docs/b.md"]);
    let checks = Cell::new(0);
    // Not asked at the top of the loop or before the first pull request; asked before the second.
    let stop = || {
        checks.set(checks.get() + 1);
        checks.get() >= 3
    };
    let (s, _, _) = session(&f, &caps(), &stop);
    assert_eq!(s.merged, vec![2]);
    assert!(s.reason.contains("stopped"), "{}", s.reason);
}

#[test]
fn it_stops_at_the_time_cap() {
    let f = FakeForge::default();
    f.with_ready_pr(2, "h2", &["docs/a.md"]);
    // Checks never finish: the pull request waits, and the clock runs out before the idle limit does.
    f.st.borrow_mut().checks.insert(2, vec![super::forge::Check { name: "test".into(), bucket: "pending".into() }]);
    let c = Caps { run_for: Duration::from_secs(600), idle: Duration::from_secs(3600), ..caps() };
    let (s, _, _) = session(&f, &c, &never);
    assert_eq!(s.reason, "reached the time cap");
    assert!(s.merged.is_empty() && s.errors == 0, "waiting is not an error: {s:?}");
}

#[test]
fn waiting_for_checks_is_not_an_error_and_it_merges_once_they_are_green() {
    let f = FakeForge::default();
    f.with_ready_pr(2, "h2", &["docs/a.md"]);
    f.st.borrow_mut().checks.insert(2, vec![super::forge::Check { name: "test".into(), bucket: "pending".into() }]);
    let polls = Cell::new(0);
    // Turn the checks green after a few polls by way of the stop probe, which is asked every second of sleeping.
    let probe = || {
        polls.set(polls.get() + 1);
        if polls.get() == 150 {
            f.st.borrow_mut().checks.insert(2, vec![super::forge::Check { name: "test".into(), bucket: "pass".into() }]);
        }
        false
    };
    let (s, _, _) = session(&f, &caps(), &probe);
    assert_eq!(s.merged, vec![2]);
    assert_eq!(s.errors, 0);
}

#[test]
fn two_errors_in_a_row_pause_it_and_a_parked_pull_request_is_not_retried() {
    let f = FakeForge::default();
    f.with_ready_pr(2, "h2", &["docs/a.md"]);
    f.with_ready_pr(3, "h3", &["docs/b.md"]);
    f.with_ready_pr(4, "h4", &["docs/c.md"]);
    f.edit(2, |p| p.mergeable = "CONFLICTING".into());
    f.edit(3, |p| p.mergeable = "CONFLICTING".into());
    let (s, home, _) = session(&f, &caps(), &never);
    assert!(s.reason.starts_with("paused after 2 error(s)"), "{}", s.reason);
    assert_eq!(s.errors, 2);
    assert!(f.st.borrow().merged.is_empty(), "the session stopped before reaching #4");
    // Both are in the error log for the builder.
    let open: Vec<u64> = Errors::at(home.path()).entries().iter().filter(|e| e.state == "open").map(|e| e.pr).collect();
    assert_eq!(open, vec![2, 3]);
}

#[test]
fn a_success_resets_the_run_of_errors() {
    let f = FakeForge::default();
    f.with_ready_pr(2, "h2", &["docs/a.md"]);
    f.with_ready_pr(3, "h3", &["docs/b.md"]);
    f.with_ready_pr(4, "h4", &["docs/c.md"]);
    f.edit(2, |p| p.mergeable = "CONFLICTING".into());
    f.edit(4, |p| p.mergeable = "CONFLICTING".into());
    let (s, _, _) = session(&f, &caps(), &never);
    assert_eq!(s.merged, vec![3]);
    assert_eq!(s.errors, 2, "one before and one after the success, never two in a row");
    assert!(s.reason.starts_with("nothing to do"), "{}", s.reason);
}

#[test]
fn defaults_match_the_spec_and_ceilings_hold() {
    let d = Caps::default();
    assert_eq!((d.max_prs, d.run_for.as_secs(), d.pause_after_errors, d.idle.as_secs()), (10, 4 * 3600, 2, 1800));
    assert!(d.validate().is_ok());
    assert!(Caps { max_prs: 101, ..Caps::default() }.validate().is_err());
    assert!(Caps { run_for: Duration::from_secs(25 * 3600), ..Caps::default() }.validate().is_err());
    assert!(Caps { pause_after_errors: 0, ..Caps::default() }.validate().is_err());
    assert_eq!(super::auto::parse_duration("90s"), Ok(Duration::from_secs(90)));
    assert_eq!(super::auto::parse_duration("4h"), Ok(Duration::from_secs(14400)));
    assert!(super::auto::parse_duration("0m").is_err() && super::auto::parse_duration("soon").is_err());
}
