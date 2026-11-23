//! The merge commands refreshing a behind pull request themselves (spec 094 §11.3.3, AC-11, V1): a fake forge, a fake refresh
//! that pushes a new head with checks pending, and a fake clock on which CI finishes while the command waits. No network.

use super::approve::FakeTerminal;
use super::auto::{self, Caps, Clock};
use super::errors::Errors;
use super::fake::{FakeForge, APPROVED_SPEC, SPEC_PATH};
use super::forge::Check;
use super::record::{Attestation, Ledger};
use super::refresh::{Done, Report, Stop};
use super::wait::{Refresher, Refreshing, MAX_ROUNDS};
use super::{merge_many, merge_one, status_lines, Ctx, Outcome};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Duration;

/// What CI does each time the command sleeps.
#[derive(Clone, Copy, PartialEq)]
enum Ci {
    /// Every pending check passes.
    Passes,
    /// Every pending check passes, and the base moves under every open pull request.
    PassesAndTheBaseMoves,
    /// Nothing finishes.
    Hangs,
    /// Every pending check fails.
    Fails,
    /// Checks pass, but somebody else pushes to every open pull request's branch.
    SomebodyPushes,
    /// As GitHub on 2026-11-23 (#887, #889, #892): the refreshed head has no checks at all at first; they register at the next
    /// sleep (pending) and pass at the one after.
    RegistersLate,
    /// The refreshed head has no checks, and none ever register.
    NeverRegisters,
}

struct World<'a> {
    forge: &'a FakeForge,
    ci: Cell<Ci>,
    t: Cell<u64>,
    /// Pull requests whose refresh is handed over to `merge fix`.
    refuse: RefCell<Vec<u64>>,
    refreshed: RefCell<Vec<u64>>,
    /// With `RegistersLate`: sleeps that pass before the checks register.
    quiet: Cell<u32>,
}

impl<'a> World<'a> {
    fn new(forge: &'a FakeForge) -> World<'a> {
        World { forge, ci: Cell::new(Ci::Passes), t: Cell::new(1_000_000), refuse: RefCell::new(vec![]), refreshed: RefCell::new(vec![]), quiet: Cell::new(0) }
    }
}

impl Refresher for World<'_> {
    /// As `refresh::refresh` does on GitHub: a new head (a merge commit of the base), current, with its checks not yet run.
    fn refresh(&self, n: u64) -> Result<Done, Stop> {
        self.refreshed.borrow_mut().push(n);
        if self.refuse.borrow().contains(&n) {
            return Err(Stop::HandOver("a conflict outside the files it settles by rule: README.md".into()));
        }
        let mut s = self.forge.st.borrow_mut();
        let p = s.prs.get_mut(&n).expect("pr");
        let old = p.head_sha.clone();
        let new = format!("{old}-r");
        p.head_sha = new.clone();
        let checks = if matches!(self.ci.get(), Ci::RegistersLate | Ci::NeverRegisters) { vec![] } else { vec![Check { name: "test".into(), bucket: "pending".into() }] };
        s.checks.insert(n, checks);
        s.files.insert((new.clone(), SPEC_PATH.into()), APPROVED_SPEC.into());
        Ok(Done::Refreshed(Report { pr: n, old_head: old, new_head: new, base_sha: "base1".into(), settled: vec![], regenerated: vec![] }))
    }
}

impl Clock for World<'_> {
    fn now_secs(&self) -> u64 {
        self.t.get()
    }
    fn sleep(&self, d: Duration) {
        self.t.set(self.t.get() + d.as_secs());
        let mut s = self.forge.st.borrow_mut();
        let ci = self.ci.get();
        if matches!(ci, Ci::Hangs | Ci::NeverRegisters) {
            return;
        }
        for checks in s.checks.values_mut() {
            for c in checks.iter_mut().filter(|c| c.bucket == "pending") {
                c.bucket = if ci == Ci::Fails { "fail" } else { "pass" }.into();
            }
        }
        if ci == Ci::RegistersLate && self.quiet.get() > 0 {
            self.quiet.set(self.quiet.get() - 1);
        } else if ci == Ci::RegistersLate {
            for checks in s.checks.values_mut().filter(|c| c.is_empty()) {
                checks.push(Check { name: "test".into(), bucket: "pending".into() });
            }
        }
        if ci == Ci::SomebodyPushes {
            s.prs.values_mut().filter(|p| p.state == "OPEN").for_each(|p| p.head_sha = "theirs".into());
        }
        if ci == Ci::PassesAndTheBaseMoves {
            let heads: Vec<String> = s.prs.values().filter(|p| p.state == "OPEN").map(|p| p.head_sha.clone()).collect();
            for h in heads {
                *s.behind.entry(h).or_insert(0) += 1;
            }
        }
    }
}

struct Run {
    home: tempfile::TempDir,
}

impl Run {
    fn rows(&self) -> Vec<serde_json::Value> {
        Ledger::at(self.home.path()).read()
    }
    fn open_classes(&self) -> Vec<String> {
        Errors::at(self.home.path()).entries().into_iter().filter(|e| e.open()).map(|e| e.class).collect()
    }
}

/// Run `f` with a context that refreshes through `w` and waits `wait` (`None`: `--no-wait`).
fn with<R>(w: &World, wait: Option<Duration>, f: impl FnOnce(&mut Ctx) -> R) -> (R, Run) {
    let run = Run { home: tempfile::tempdir().unwrap() };
    let (ledger, errors) = (Ledger::at(run.home.path()), Errors::at(run.home.path()));
    let made = RefCell::new(HashMap::new());
    let refresh = Refreshing { refresher: w, clock: w, wait, poll: Duration::from_secs(60), made: &made };
    let mut term = FakeTerminal::new(false, &[]);
    let mut ctx = Ctx { forge: w.forge, ledger: &ledger, errors: &errors, term: &mut term, quote: Some("merge #7".into()), dry_run: false, git: None, session: None, settle: Duration::ZERO, now: || "2026-11-21T00:00:00Z".to_string(), refresh: Some(refresh), claims: None, rollup: None };
    let out = f(&mut ctx);
    (out, run)
}

const WAIT: Option<Duration> = Some(Duration::from_secs(40 * 60));

fn behind_pr(forge: &FakeForge, n: u64, by: u64) {
    forge.with_ready_pr(n, &format!("head{n}"), &["a.rs"]);
    forge.st.borrow_mut().behind.insert(format!("head{n}"), by);
}

#[test]
fn a_behind_pull_request_is_refreshed_waited_for_and_its_new_head_merged_with_the_approval_carried_over() {
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 2);
    let w = World::new(&forge);
    let (out, run) = with(&w, WAIT, |c| merge_one(c, 7, None));
    assert_eq!(out, Outcome::Merged { pr: 7, head: "head7-r".into() });
    assert_eq!(forge.st.borrow().merged, vec![(7, "head7-r".to_string())], "only the new head, which CI passed, merges");
    // The record is derived anew for the new head; the attestation carried over says from which head.
    let rows = run.rows();
    let merged = rows.iter().find(|r| r["kind"] == "merged").unwrap();
    assert_eq!((merged["head_sha"].as_str(), merged["carried_from"].as_str()), (Some("head7-r"), Some("head7")));
    assert_eq!(merged["checks"][0]["bucket"], "pass");
    assert!(forge.st.borrow().comments.iter().any(|(_, c)| c.contains("carried over") && c.contains("head7")));
    assert!(run.open_classes().is_empty(), "a merge resolves what was open");
}

#[test]
fn a_base_that_keeps_moving_gets_a_second_round_and_then_a_stop() {
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    let w = World::new(&forge);
    w.ci.set(Ci::PassesAndTheBaseMoves);
    let (out, run) = with(&w, WAIT, |c| merge_one(c, 7, None));
    assert_eq!(w.refreshed.borrow().len(), MAX_ROUNDS as usize);
    assert!(matches!(&out, Outcome::Refused { reasons, head, .. } if reasons[0].contains("the base kept moving") && head == "head7-r-r"), "{out:?}");
    assert!(forge.st.borrow().merged.is_empty());
    assert_eq!(run.open_classes(), vec!["behind"]);
}

#[test]
fn all_merges_each_pull_request_against_the_tip_it_lands_on() {
    let forge = FakeForge::default();
    for n in 1..=3 {
        forge.with_ready_pr(n, &format!("h{n}"), &[&format!("f{n}.rs")]);
    }
    forge.st.borrow_mut().behind_others_on_merge = true;
    let w = World::new(&forge);
    let att = Attestation::ChatQuote("merge all of them".into());
    let (outs, run) = with(&w, WAIT, |c| merge_many(c, &[1, 2, 3], Some(&att)));
    assert!(outs.iter().all(|o| matches!(o, Outcome::Merged { .. })), "{outs:?}");
    // The first was current; each later one was behind after the merge before it, so it was refreshed and re-tested.
    assert_eq!(forge.st.borrow().merged, vec![(1, "h1".into()), (2, "h2-r".into()), (3, "h3-r".into())]);
    let carried: Vec<Option<String>> = run.rows().iter().filter(|r| r["kind"] == "merged").map(|r| r["carried_from"].as_str().map(String::from)).collect();
    assert_eq!(carried, vec![None, Some("h2".into()), Some("h3".into())]);
}

#[test]
fn a_refresh_that_is_handed_over_is_skipped_with_its_reason_and_the_batch_carries_on() {
    let forge = FakeForge::default();
    behind_pr(&forge, 1, 1);
    behind_pr(&forge, 2, 1);
    let w = World::new(&forge);
    w.refuse.borrow_mut().push(1);
    let att = Attestation::ChatQuote("merge all".into());
    let (outs, _) = with(&w, WAIT, |c| merge_many(c, &[1, 2], Some(&att)));
    assert!(matches!(&outs[0], Outcome::Refused { reasons, .. } if reasons[0].contains("delphi merge fix 1")), "{outs:?}");
    assert!(matches!(outs[1], Outcome::Merged { .. }), "{outs:?}");
}

#[test]
fn behind_and_red_or_behind_and_pending_is_refused_as_before_and_not_refreshed() {
    for bucket in ["fail", "pending"] {
        let forge = FakeForge::default();
        behind_pr(&forge, 7, 1);
        forge.st.borrow_mut().checks.insert(7, vec![Check { name: "test".into(), bucket: bucket.into() }]);
        let w = World::new(&forge);
        let (out, _) = with(&w, WAIT, |c| merge_one(c, 7, None));
        assert!(matches!(&out, Outcome::Refused { reasons, .. } if reasons.iter().any(|r| r.contains("behind"))), "{bucket}: {out:?}");
        assert!(w.refreshed.borrow().is_empty(), "{bucket}: a CI run on a head that will be replaced is wasted");
    }
    // A dry run and a gate-changing pull request are never refreshed either.
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    let w = World::new(&forge);
    let (out, _) = with(&w, WAIT, |c| {
        c.dry_run = true;
        merge_one(c, 7, None)
    });
    assert!(matches!(out, Outcome::Refused { .. }) && w.refreshed.borrow().is_empty(), "{out:?}");
    forge.st.borrow_mut().paths.insert(7, vec!["AGENTS.md".into()]);
    let (out, _) = with(&w, WAIT, |c| merge_one(c, 7, None));
    assert!(matches!(out, Outcome::Refused { .. }) && w.refreshed.borrow().is_empty(), "{out:?}");
}

#[test]
fn no_wait_refreshes_and_stops_and_a_wait_that_runs_out_leaves_it_refreshed() {
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    let w = World::new(&forge);
    let (out, _) = with(&w, None, |c| merge_one(c, 7, None));
    assert_eq!(out, Outcome::Refreshed { pr: 7, head: "head7-r".into(), waited: false });
    assert!(out.ok() && out.line().contains("refreshed") && forge.st.borrow().merged.is_empty());

    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    let w = World::new(&forge);
    w.ci.set(Ci::Hangs);
    let start = w.t.get();
    let (out, _) = with(&w, WAIT, |c| merge_one(c, 7, None));
    assert_eq!(out, Outcome::Refreshed { pr: 7, head: "head7-r".into(), waited: true });
    assert!(w.t.get() - start >= 40 * 60 && w.t.get() - start <= 41 * 60, "bounded by the wait");
    assert!(forge.st.borrow().merged.is_empty());
}

#[test]
fn a_new_head_that_goes_red_or_that_somebody_else_moves_is_refused_and_not_merged() {
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    let w = World::new(&forge);
    w.ci.set(Ci::Fails);
    let (out, run) = with(&w, WAIT, |c| merge_one(c, 7, None));
    assert!(matches!(&out, Outcome::Refused { reasons, head, .. } if reasons[0].starts_with("checks failed") && head == "head7-r"), "{out:?}");
    assert_eq!(run.open_classes(), vec!["ci-red"]);

    // A push by somebody else while it waits: that head was not made by this run, so the approval does not cover it.
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    let w = World::new(&forge);
    w.ci.set(Ci::SomebodyPushes);
    let (out, _) = with(&w, WAIT, |c| merge_one(c, 7, None));
    assert!(matches!(&out, Outcome::Refused { reasons, head, .. } if reasons[0].contains("did not make") && head == "theirs"), "{out:?}");
    assert!(forge.st.borrow().merged.is_empty());
}

#[test]
fn an_auto_session_refreshes_and_moves_on_then_merges_the_new_head_at_a_later_poll() {
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    forge.with_ready_pr(8, "head8", &["b.rs"]);
    let w = World::new(&forge);
    let caps = Caps { max_prs: 10, run_for: Duration::from_secs(4 * 3600), pause_after_errors: 2, idle: Duration::from_secs(1800), poll: Duration::from_secs(60), batch: None };
    let (s, run) = with(&w, None, |c| auto::run(c, &caps, &Attestation::ChatQuote("auto for the evening".into()), &w, &|| false, &mut |_| {}));
    // #7 was refreshed, #8 merged in the same pass, and #7's new head merged at a later poll once green.
    assert_eq!(w.refreshed.borrow().as_slice(), &[7]);
    assert_eq!(s.merged, vec![8, 7]);
    assert_eq!(s.errors, 0, "a refresh that worked is not an error");
    let m7 = run.rows().into_iter().find(|r| r["kind"] == "merged" && r["pr"] == 7).unwrap();
    assert_eq!((m7["head_sha"].as_str(), m7["carried_from"].as_str()), (Some("head7-r"), Some("head7")));

    // A refresh that fails counts toward the pause, and the pull request is parked.
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    behind_pr(&forge, 8, 1);
    let w = World::new(&forge);
    w.refuse.borrow_mut().extend([7, 8]);
    let (s, run) = with(&w, None, |c| auto::run(c, &caps, &Attestation::ChatQuote("auto".into()), &w, &|| false, &mut |_| {}));
    assert!(s.reason.starts_with("paused after 2 error(s)"), "{}", s.reason);
    assert_eq!(w.refreshed.borrow().as_slice(), &[7, 8], "each tried once, then parked");
    assert!(run.open_classes().is_empty(), "the fake refresh writes no entry and the merge does not write a second one");
}

#[test]
fn no_checks_yet_on_a_refreshed_head_is_waited_for_then_merged_once_green() {
    // `<PR> --wait`: polls through "no checks yet" and "running", and merges only once green (§16.2).
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    let w = World::new(&forge);
    w.ci.set(Ci::RegistersLate);
    w.quiet.set(2);
    let start = w.t.get();
    let (out, _) = with(&w, WAIT, |c| merge_one(c, 7, None));
    assert_eq!(out, Outcome::Merged { pr: 7, head: "head7-r".into() });
    assert_eq!(w.t.get() - start, 4 * 60, "three looks with no checks, one running, then green");

    // Never green: a wait that runs out with no checks at all leaves it refreshed and merges nothing.
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    let w = World::new(&forge);
    w.ci.set(Ci::NeverRegisters);
    let (out, _) = with(&w, WAIT, |c| merge_one(c, 7, None));
    assert!(matches!(out, Outcome::Refreshed { waited: true, .. }), "{out:?}");
    assert!(forge.st.borrow().merged.is_empty());

    // `--auto`: a refreshed head with no checks yet is neither an error nor parked; a later poll merges it.
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 1);
    let w = World::new(&forge);
    w.ci.set(Ci::RegistersLate);
    w.quiet.set(2);
    let caps = Caps { max_prs: 1, run_for: Duration::from_secs(4 * 3600), pause_after_errors: 1, idle: Duration::from_secs(1800), poll: Duration::from_secs(60), batch: None };
    let (s, _) = with(&w, None, |c| auto::run(c, &caps, &Attestation::ChatQuote("auto for the evening".into()), &w, &|| false, &mut |_| {}));
    assert_eq!((s.merged.as_slice(), s.errors), (&[7u64][..], 0), "{}", s.reason);
}

#[test]
fn no_checks_at_decision_time_refuses_with_the_new_words_and_merges_nothing() {
    let forge = FakeForge::default();
    forge.with_ready_pr(7, "head7", &["a.rs"]);
    forge.st.borrow_mut().checks.insert(7, vec![]);
    let w = World::new(&forge);
    let (out, _) = with(&w, None, |c| merge_one(c, 7, None));
    let want = "no checks have reported on head `head7` yet: CI may not have started";
    assert!(matches!(&out, Outcome::Refused { reasons, .. } if reasons.iter().any(|r| r.starts_with(want))), "{out:?}");
    assert!(forge.st.borrow().merged.is_empty() && w.refreshed.borrow().is_empty());
}

#[test]
fn status_says_a_behind_pull_request_will_be_refreshed_at_merge() {
    let forge = FakeForge::default();
    behind_pr(&forge, 7, 3);
    let lines = status_lines(&forge).unwrap();
    assert!(lines[0].contains("3 commit(s) behind `main`") && lines[0].contains("will be refreshed at merge"), "{lines:?}");
}
