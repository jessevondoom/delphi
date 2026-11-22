//! Bisecting a red rollup (spec 094 §13.5, S21, AC-21) against real git repositories with a local bare remote and a fake forge
//! whose CI is red exactly when a rollup holds a "bad" set of pull requests. No network.

use super::bisect::{bisect, status_lines};
use super::errors::Errors;
use super::fake::{FakeForge, SPEC_PATH};
use super::forge::Check;
use super::record::{Attestation, Ledger};
use super::refresh_tests::{fixture, git, Fixture};
use super::rollup::parse_manifest;
use super::rollup_pr::{batch, recognise, Approval};
use super::rollup_pr_tests::{with_ctx, ID, ROLLUP};
use super::rollup_tests::{branch, ready};
use super::{merge_one, Outcome};

/// `n` ready pull requests, #k adding `Fk.md`; CI is red on any rollup that holds every pull request of one of `red_when`.
fn world(n: u64, red_when: Vec<Vec<u64>>) -> (Fixture, FakeForge, Vec<String>) {
    let f = fixture();
    let forge = FakeForge::default();
    let mut heads = Vec::new();
    for k in 1..=n {
        let (name, file) = (format!("b{k}"), format!("F{k}.md"));
        let h = branch(&f, &name, |d| std::fs::write(d.join(&file), format!("{k}\n")).unwrap());
        ready(&forge, k, &name, &h, &[file.as_str()], Some(SPEC_PATH));
        heads.push(h);
    }
    let mut s = forge.st.borrow_mut();
    s.origin = Some(f.origin.clone());
    s.red_when = red_when;
    s.land_on_merge = true;
    drop(s);
    (f, forge, heads)
}

fn quote(n: u64) -> String {
    format!("batch {}", (1..=n).map(|k| format!("#{k}")).collect::<Vec<_>>().join(" "))
}

/// Batch all `n` and return the summary line of the (red) rollup's outcome.
fn run(f: &Fixture, forge: &FakeForge, n: u64) -> String {
    let all: Vec<u64> = (1..=n).collect();
    match with_ctx(f, forge, Some(&quote(n)), |ctx| batch(ctx, &all, n as usize, false, ID)).unwrap() {
        Outcome::Refused { pr: ROLLUP, reasons, .. } => reasons.join("; "),
        other => panic!("{other:?}"),
    }
}

/// Constituents with a `merged` ledger row, and those with a `rollup-red` entry.
fn merged(f: &Fixture) -> Vec<u64> {
    let mut v: Vec<u64> = Ledger::at(f.home.path()).read().iter().filter(|r| r["kind"] == "merged" && r["carried_into"].is_u64()).filter_map(|r| r["pr"].as_u64()).collect();
    v.sort();
    v
}
fn red(f: &Fixture) -> Vec<u64> {
    let mut v: Vec<u64> = Errors::at(f.home.path()).entries().iter().filter(|e| e.class == "rollup-red").map(|e| e.pr).collect();
    v.sort();
    v
}

/// The rollups merged, and the constituents of each rollup opened, by number.
fn merged_rollups(forge: &FakeForge) -> Vec<u64> {
    forge.st.borrow().merged.iter().map(|m| m.0).collect()
}
fn opened(forge: &FakeForge) -> Vec<Vec<u64>> {
    forge.st.borrow().opened.iter().map(|o| parse_manifest(&o.3).unwrap().prs.iter().map(|p| p.0).collect()).collect()
}

/// No rollup branch is left on the origin.
fn no_rollup_branches(f: &Fixture) {
    assert_eq!(git(&f.origin, &["branch", "--list", "delphi-rollup/*"]), "", "a rollup branch was left behind");
}

#[test]
fn a_red_rollup_of_four_with_one_bad_pull_request_merges_three_and_isolates_one_in_two_rounds() {
    let (f, forge, _) = world(4, vec![vec![4]]);
    let line = run(&f, &forge, 4);
    assert!(line.contains("merged #1, #2, #3") && line.contains("(rollup-red, for `merge fix`) #4"), "{line}");
    // 900 {1..4} red; 901 {1,2} green, 902 {3,4} red; 902 split on the new tip: 903 {3} green, 904 {4} red.
    assert_eq!(opened(&forge), vec![vec![1, 2, 3, 4], vec![1, 2], vec![3, 4], vec![3], vec![4]]);
    assert_eq!(merged_rollups(&forge), vec![901, 903], "only green halves merge; a red rollup never does");
    assert_eq!((merged(&f), red(&f)), (vec![1, 2, 3], vec![4]));
    let s = forge.st.borrow();
    let closed: Vec<u64> = s.closed.iter().map(|c| c.0).filter(|n| *n >= ROLLUP).collect();
    assert_eq!(closed, vec![ROLLUP, 902, 904], "superseded rollups are closed");
    assert!(s.closed.iter().filter(|c| c.0 >= ROLLUP).all(|c| c.1.contains("never merged")), "{:?}", s.closed);
    // The second round was built on the tip the first green half made.
    let rows = Ledger::at(f.home.path()).read();
    let base_of = |n: u64| rows.iter().find(|r| r["kind"] == "rollup_opened" && r["pr"] == n).unwrap()["base_sha"].as_str().unwrap().to_string();
    assert_ne!(base_of(903), base_of(901));
    assert!(rows.iter().any(|r| r["kind"] == "rollup_half" && r["pr"] == 904 && r["round"] == 2 && r["parent"] == 902), "{rows:?}");
    drop(s);
    no_rollup_branches(&f);
    // `merge fix` takes the isolated one.
    let e = Errors::at(f.home.path()).entries().into_iter().find(|e| e.class == "rollup-red").unwrap();
    assert!(super::errors::Class::from_slug(&e.class).unwrap().builder_can_fix() && e.detail.contains("red on its own"), "{e:?}");
}

#[test]
fn two_bad_pull_requests_in_different_halves_are_each_isolated() {
    let (f, forge, _) = world(4, vec![vec![1], vec![4]]);
    run(&f, &forge, 4);
    assert_eq!((merged(&f), red(&f)), (vec![2, 3], vec![1, 4]));
    assert_eq!(merged_rollups(&forge).len(), 2);
    assert!(!merged_rollups(&forge).iter().any(|n| [ROLLUP, 901, 902].contains(n)));
    no_rollup_branches(&f);
}

#[test]
fn a_bad_pull_request_not_isolated_after_two_rounds_goes_back_to_the_serial_path() {
    // Five: {1,2,3} red and {4,5} green; then {1,2} red and {3} green; {1,2} is round 2, so it is not split again.
    let (f, forge, _) = world(5, vec![vec![1]]);
    let line = run(&f, &forge, 5);
    assert!(line.contains("back to the serial path #1, #2"), "{line}");
    assert_eq!((merged(&f), red(&f)), (vec![3, 4, 5], vec![]), "not isolated, so no `rollup-red` entry");
    assert_eq!(opened(&forge).len(), 5, "two rounds, no third");
    let rows = Ledger::at(f.home.path()).read();
    assert!(rows.iter().any(|r| r["kind"] == "rollup_serial" && r["constituents"] == serde_json::json!([1, 2])), "{rows:?}");
    // They stay queued for `--all` without `--batch`.
    let s = forge.st.borrow();
    assert!([1, 2].iter().all(|k| s.prs[k].state == "OPEN" && s.prs[k].labels.iter().any(|l| l == "merge-queue")));
}

#[test]
fn red_together_and_green_apart_sends_every_pull_request_to_the_serial_path() {
    let (f, forge, _) = world(4, vec![vec![1, 3]]);
    let line = run(&f, &forge, 4);
    assert!(merged_rollups(&forge).is_empty(), "nothing merges");
    assert_eq!(red(&f), vec![1, 2, 3, 4]);
    assert!(Errors::at(f.home.path()).entries().iter().all(|e| e.class != "rollup-red" || e.detail.contains("red together, green apart")));
    assert!(line.contains("back to the serial path #1, #2, #3, #4"), "{line}");
    let closed: Vec<u64> = forge.st.borrow().closed.iter().map(|c| c.0).collect();
    assert!([ROLLUP, 901, 902].iter().all(|n| closed.contains(n)), "the red rollup and both green halves are closed: {closed:?}");
    no_rollup_branches(&f);
}

/// Open the red rollup of four (CI pending at first, so nothing is bisected yet) and recognise it.
fn red_rollup(f: &Fixture, forge: &FakeForge) -> super::rollup_pr::Recognised {
    let red_when = std::mem::take(&mut forge.st.borrow_mut().red_when);
    forge.st.borrow_mut().checks_on_open = vec![Check { name: "ci-gate".into(), bucket: "pending".into() }];
    let out = with_ctx(f, forge, Some(&quote(4)), |ctx| batch(ctx, &[1, 2, 3, 4], 4, false, ID)).unwrap();
    assert!(matches!(&out, Outcome::Refused { reasons, .. } if reasons[0].contains("have not finished")), "{out:?}");
    let mut s = forge.st.borrow_mut();
    s.red_when = red_when;
    s.checks.insert(ROLLUP, vec![Check { name: "ci-gate".into(), bucket: "fail".into() }]);
    drop(s);
    with_ctx(f, forge, None, |ctx| recognise(ctx.forge, ctx.rollup.unwrap(), &super::check::evaluate(ctx.forge, ROLLUP).unwrap())).unwrap()
}

fn approval(heads: &[String], prs: &[u64]) -> Approval {
    Approval { att: Attestation::ChatQuote(quote(4)), pairs: prs.iter().map(|k| (*k, heads[*k as usize - 1].clone())).collect() }
}

#[test]
fn a_gate_changing_pull_request_never_enters_a_half() {
    let (f, forge, heads) = world(4, vec![vec![4]]);
    let r = red_rollup(&f, &forge);
    // #2 now changes a gate (as read from the forge): the builder leaves it out of its half, and it goes to the serial path.
    forge.st.borrow_mut().paths.insert(2, vec!["AGENTS.md".into()]);
    let out = with_ctx(&f, &forge, None, |ctx| bisect(ctx, ctx.rollup.unwrap(), &r, ROLLUP, &approval(&heads, &[1, 2, 3, 4])));
    let line = format!("{out:?}");
    assert!(line.contains("back to the serial path #2") && line.contains("merged #1, #3"), "{line}");
    assert!(opened(&forge).iter().skip(1).all(|prs| !prs.contains(&2)), "{:?}", opened(&forge));
    assert_eq!(red(&f), vec![4]);
}

#[test]
fn the_approval_carries_only_to_halves_of_the_approved_set() {
    let (f, forge, heads) = world(4, vec![vec![4]]);
    let r = red_rollup(&f, &forge);
    // An approval that does not name #4 (or names another head of it) cannot bisect a rollup that carries #4: nothing is built.
    for a in [approval(&heads, &[1, 2, 3]), Approval { pairs: vec![(1, heads[0].clone()), (2, heads[1].clone()), (3, heads[2].clone()), (4, "e".repeat(40))], ..approval(&heads, &[]) }] {
        let out = with_ctx(&f, &forge, None, |ctx| bisect(ctx, ctx.rollup.unwrap(), &r, ROLLUP, &a));
        assert!(format!("{out:?}").contains("rollups left open #900"), "{out:?}");
    }
    assert_eq!(opened(&forge).len(), 1, "no half was opened");
    assert!(forge.st.borrow().closed.is_empty() && forge.st.borrow().merged.is_empty());
}

#[test]
fn halves_whose_checks_have_not_finished_stay_open_and_status_shows_them_bisecting() {
    let (f, forge, _) = world(4, vec![]);
    forge.st.borrow_mut().checks_on_open = vec![Check { name: "ci-gate".into(), bucket: "pending".into() }];
    let first = with_ctx(&f, &forge, Some(&quote(4)), |ctx| batch(ctx, &[1, 2, 3, 4], 4, false, ID)).unwrap();
    assert!(matches!(first, Outcome::Refused { .. }));
    forge.st.borrow_mut().checks.insert(ROLLUP, vec![Check { name: "ci-gate".into(), bucket: "fail".into() }]);
    // A later run on the operator's words: red, so bisected; the halves' CI is still running, so both stay open.
    let out = with_ctx(&f, &forge, Some("merge #1, #2, #3 and #4"), |ctx| merge_one(ctx, ROLLUP, None));
    assert!(format!("{out:?}").contains("rollups left open #901, #902"), "{out:?}");
    let lines = status_lines(&forge, &Ledger::at(f.home.path()));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("rollup #901 of #1, #2: bisecting: round 1 of 2, a half of #900"), "{lines:?}");
    assert!(forge.st.borrow().merged.is_empty());
}
