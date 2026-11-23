//! `delphi merge --auto --batch` (spec 094 §13, S22): a bounded session that merges through rollups, against real git
//! repositories with a local bare remote and a fake forge whose CI is red exactly when a rollup holds a "bad" set. No network.

use super::auto::{run, Caps, Clock, Summary};
use super::auto_batch::status_lines;
use super::bisect_tests::{merged, opened, red, world};
use super::check::REVIEW_LABEL;
use super::fake::{FakeForge, SPEC_PATH};
use super::forge::Check;
use super::record::{Attestation, Ledger};
use super::refresh_tests::Fixture;
use super::rollup_pr::batch;
use super::rollup_pr_tests::{with_ctx, ID};
use super::rollup_tests::ready;
use std::cell::Cell;
use std::time::Duration;

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
    Caps { max_prs: 10, run_for: Duration::from_secs(4 * 3600), pause_after_errors: 10, idle: Duration::from_secs(1800), poll: Duration::from_secs(60), batch: Some(4) }
}

/// Run a session over `f` and `forge`. `probe` is asked whether to stop, every second the session sleeps and between pull
/// requests; it finishes whatever CI is running, so a rollup opened at one poll is judged at the next.
fn session(f: &Fixture, forge: &FakeForge, caps: &Caps, probe: &dyn Fn()) -> Summary {
    let clock = FakeClock(Cell::new(1_000_000));
    let stop = || {
        probe();
        forge.finish_ci();
        false
    };
    with_ctx(f, forge, None, |ctx| run(ctx, caps, &Attestation::ChatQuote("run auto with batches tonight".into()), &clock, &stop, &mut |_| {}))
}

fn ci_later(forge: &FakeForge) {
    forge.st.borrow_mut().ci_later = true;
}

fn merged_numbers(forge: &FakeForge) -> Vec<u64> {
    forge.st.borrow().merged.iter().map(|m| m.0).collect()
}

/// CI that is never red.
fn green() -> Vec<Vec<u64>> {
    vec![vec![99]]
}

#[test]
fn a_green_batch_is_opened_at_one_poll_and_merged_at_the_next() {
    let (f, forge, _) = world(3, green());
    ci_later(&forge);
    let s = session(&f, &forge, &caps(), &|| {});
    assert_eq!(opened(&forge), vec![vec![1, 2, 3]]);
    // The rollup's CI was still running at the poll that opened it; the next poll merged it.
    assert_eq!(merged_numbers(&forge), vec![900]);
    assert_eq!((merged(&f), s.merged.clone()), (vec![1, 2, 3], vec![1, 2, 3]));
    assert_eq!((s.rollups.clone(), s.rollups_open.clone(), s.errors), (vec![900], vec![], 0));
    assert!(s.reason.starts_with("nothing to do"), "{}", s.reason);
    // Each constituent's rows carry the session; the grant records the batch size; status shows the session's batches.
    let rows = Ledger::at(f.home.path()).read();
    let id = rows[0]["session"].as_str().unwrap().to_string();
    assert_eq!(rows[0]["batch"], 4);
    assert!(rows.iter().filter(|r| r["kind"] == "merged").all(|r| r["session"] == id.as_str() && r["carried_into"] == 900), "{rows:?}");
    let lines = status_lines(&Ledger::at(f.home.path()));
    assert!(lines[0].starts_with(&format!("session {id} (ended: nothing to do")) && lines[0].ends_with("1 rollup(s) opened, 1 merged carrying 3 pull request(s), 0 bisected"), "{lines:?}");
}

#[test]
fn a_red_batch_is_bisected_across_polls_without_blocking_other_work() {
    let (f, forge, _) = world(4, vec![vec![4]]);
    ci_later(&forge);
    // Once the first rollup is open, a gate-changing pull request is queued: the serial path labels it for a human while
    // rollups are still in flight.
    let labelled_in_flight = Cell::new(false);
    let probe = || {
        let opened = forge.st.borrow().opened.len();
        if opened == 1 && !forge.st.borrow().prs.contains_key(&5) {
            ready(&forge, 5, "b5", "h5", &["AGENTS.md"], Some(SPEC_PATH));
        }
        let s = forge.st.borrow();
        if s.labels.iter().any(|l| l.0 == 5 && l.1 == REVIEW_LABEL) && s.prs.range(900..).any(|(_, p)| p.state == "OPEN") {
            labelled_in_flight.set(true);
        }
    };
    let s = session(&f, &forge, &caps(), &probe);
    assert_eq!(opened(&forge), vec![vec![1, 2, 3, 4], vec![1, 2], vec![3, 4], vec![3], vec![4]]);
    assert_eq!(merged_numbers(&forge), vec![901, 903], "only green halves merge");
    assert_eq!((merged(&f), red(&f)), (vec![1, 2, 3], vec![4]));
    assert_eq!((s.left_for_a_human.clone(), s.errors), (vec![5], 1), "{s:?}");
    assert!(labelled_in_flight.get(), "the gate-changing pull request waited for the bisection");
    assert!(s.rollups_open.is_empty() && s.reason.starts_with("nothing to do"), "{s:?}");
}

#[test]
fn halves_are_decided_together_and_red_together_green_apart_goes_serial() {
    let (f, forge, _) = world(4, vec![vec![1, 3]]);
    ci_later(&forge);
    let s = session(&f, &forge, &caps(), &|| {});
    assert_eq!(opened(&forge), vec![vec![1, 2, 3, 4], vec![1, 2], vec![3, 4]]);
    assert_eq!(red(&f), vec![1, 2, 3, 4]);
    // No rollup merged; each pull request then merged on its own, and none was batched again.
    assert_eq!(merged_numbers(&forge), vec![1, 2, 3, 4]);
    assert_eq!(s.errors, 4, "each `rollup-red` entry counts");
}

#[test]
fn one_eligible_pull_request_takes_the_serial_path() {
    let (f, forge, _) = world(1, green());
    let s = session(&f, &forge, &caps(), &|| {});
    assert_eq!((merged_numbers(&forge), s.merged), (vec![1], vec![1]));
    assert!(opened(&forge).is_empty() && s.rollups.is_empty());
}

#[test]
fn max_prs_counts_constituents() {
    let (f, forge, _) = world(4, green());
    let s = session(&f, &forge, &Caps { max_prs: 3, ..caps() }, &|| {});
    assert_eq!(opened(&forge), vec![vec![1, 2, 3]], "a batch never holds more than the merges left");
    assert_eq!(s.merged, vec![1, 2, 3]);
    assert!(s.reason.contains("cap of 3"), "{}", s.reason);
    assert_eq!(forge.st.borrow().prs[&4].state, "OPEN");
}

#[test]
fn a_rollup_red_entry_counts_toward_pause_after_errors() {
    let (f, forge, _) = world(4, vec![vec![4]]);
    let s = session(&f, &forge, &Caps { pause_after_errors: 1, ..caps() }, &|| {});
    assert_eq!(red(&f), vec![4]);
    assert!(s.reason.starts_with("paused after 1 error(s)"), "{}", s.reason);
    assert_eq!(s.merged, vec![1, 2, 3]);
}

#[test]
fn a_gate_changing_pull_request_is_never_batched() {
    let (f, forge, _) = world(3, green());
    forge.st.borrow_mut().paths.insert(2, vec!["AGENTS.md".into()]);
    let s = session(&f, &forge, &caps(), &|| {});
    assert_eq!(opened(&forge), vec![vec![1, 3]]);
    assert_eq!((s.merged.clone(), s.left_for_a_human.clone()), (vec![1, 3], vec![2]));
    assert!(merged_numbers(&forge).iter().all(|n| *n != 2));
}

#[test]
fn the_session_batches_only_what_it_would_merge_serially() {
    let (f, forge, _) = world(3, vec![]);
    // A rollup of #1 and #2 opened earlier on the operator's words, its CI still running, and then (wrongly) queued.
    forge.st.borrow_mut().checks_on_open = vec![Check { name: "ci-gate".into(), bucket: "pending".into() }];
    with_ctx(&f, &forge, Some("batch #1 and #2"), |ctx| batch(ctx, &[1, 2], 4, false, ID)).unwrap();
    forge.edit(900, |p| p.labels = vec!["merge-queue".into()]);
    // #3 is ready but not queued.
    forge.edit(3, |p| p.labels.clear());
    forge.st.borrow_mut().red_when = green();
    let s = session(&f, &forge, &caps(), &|| {});
    // The session made its own rollup of #1 and #2; it never merged the earlier one, nor batched or merged #3.
    assert_eq!(opened(&forge), vec![vec![1, 2], vec![1, 2]]);
    assert_eq!(merged_numbers(&forge), vec![901]);
    assert_eq!(forge.st.borrow().prs[&3].state, "OPEN");
    assert!(!s.merged.contains(&3) && s.errors == 1, "the queued rollup is refused once, then parked: {s:?}");
}
