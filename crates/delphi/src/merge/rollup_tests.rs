//! The rollup candidate builder (spec 094 §13, S18) against real git repositories with a local bare remote (AC-18). No network,
//! and nothing is pushed: every test also checks that the origin gained no branch and the forge saw no write.

use super::fake::{FakeForge, APPROVED_SPEC, DRAFT_SPEC};
use super::forge::Check;
use super::refresh::{Runner, TRACKER};
use super::refresh_tests::{add_spec, commit, compile, fixture, git, Fixture};
use super::rollup::{build, choose, parse_manifest, Candidate, Choice};
use std::path::Path;

const OTHER_SPEC: &str = "specs/093-other.md";

/// Branch `name` from `main`, change it with `edit`, commit and push it; back on `main`. Returns the head.
fn branch(f: &Fixture, name: &str, edit: impl FnOnce(&Path)) -> String {
    git(&f.work, &["switch", "-q", "-c", name, "main"]);
    edit(&f.work);
    commit(&f.work, name);
    git(&f.work, &["push", "-q", "origin", name]);
    let head = git(&f.work, &["rev-parse", "HEAD"]);
    git(&f.work, &["switch", "-q", "main"]);
    head
}

/// Move `main` on, so every constituent is behind (a batch accepts that: the rollup tests it against the tip).
fn advance_main(f: &Fixture) {
    std::fs::write(f.work.join("NOTES.md"), "another pull request merged\n").unwrap();
    commit(&f.work, "another pull request merged");
    git(&f.work, &["push", "-q", "origin", "main"]);
}

/// A ready pull request `n` on branch `name` at `head`; `spec` names its trailer (`None`: no trailer).
fn ready(forge: &FakeForge, n: u64, name: &str, head: &str, paths: &[&str], spec: Option<&str>) {
    forge.with_ready_pr(n, head, paths);
    forge.edit(n, |p| {
        p.head_ref = name.into();
        p.title = format!("PR {name}");
        p.commit_messages = vec![match spec {
            Some(s) => format!("{name}\n\nImplements-Spec: {s}\n"),
            None => name.to_string(),
        }];
    });
    if let Some(s) = spec {
        let mut st = forge.st.borrow_mut();
        st.files.insert(("main".into(), s.into()), APPROVED_SPEC.into());
        st.files.insert((head.into(), s.into()), APPROVED_SPEC.into());
    }
}

fn run(f: &Fixture, forge: &FakeForge, numbers: &[u64], max: usize) -> (Choice, Result<Candidate, String>) {
    let choice = choose(forge, numbers, max);
    let out = build(forge, &Runner::new(&f.work), f.home.path(), &choice, "20261122000000");
    (choice, out)
}

/// Nothing left the machine: no branch on the origin but `main` and the pull requests', and no forge write.
fn assert_nothing_pushed(f: &Fixture, forge: &FakeForge) {
    assert_eq!(git(&f.origin, &["branch", "--list", "delphi-rollup/*"]), "", "a rollup branch was pushed");
    let s = forge.st.borrow();
    assert!(s.merged.is_empty() && s.comments.is_empty() && s.labels.is_empty() && s.opened.is_empty(), "the forge was written to");
    assert!(!f.home.path().join("merge").join("rollup").join("20261122000000").exists(), "the worktree was left behind");
}

fn at(f: &Fixture, rev: &str, path: &str) -> String {
    git(&f.work, &["show", &format!("{rev}:{path}")])
}

#[test]
fn three_pull_requests_of_three_specs_build_one_candidate_in_order_with_the_shared_files_settled_by_rule() {
    // AC-18 (a) and (b): two add a spec and a tracker row each (the groups file, the tracker and the compiled tree conflict), one
    // changes a Markdown file; one merge commit per constituent, in order, the earlier side's lines first, the compile current.
    let f = fixture();
    let h1 = branch(&f, "b1", |d| add_spec(d, "002-a2", "a", None, Some("| 002 | S1 | built |")));
    let h2 = branch(&f, "b2", |d| add_spec(d, "010-b10", "b", None, Some("| 010 | S1 | built |")));
    let h3 = branch(&f, "b3", |d| std::fs::write(d.join("README.md"), "readme\nmore\n").unwrap());
    advance_main(&f);
    let forge = FakeForge::default();
    ready(&forge, 1, "b1", &h1, &["specs/002-a2.md", TRACKER], Some(super::fake::SPEC_PATH));
    ready(&forge, 2, "b2", &h2, &["specs/010-b10.md", TRACKER], Some(OTHER_SPEC));
    ready(&forge, 3, "b3", &h3, &["README.md"], None);

    let (choice, out) = run(&f, &forge, &[1, 2, 3], 4);
    assert!(choice.skipped.is_empty(), "{:?}", choice.skipped);
    let c = out.unwrap();
    assert!(c.dropped.is_empty(), "{:?}", c.dropped);
    assert_eq!(c.merged.iter().map(|(k, _)| k.pr).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(c.base_sha, git(&f.origin, &["rev-parse", "refs/heads/main"]));
    // Each merge commit's first parent is the one before it (the first: the base tip); its second is the approved head.
    let mut prev = c.base_sha.clone();
    for ((k, m), head) in c.merged.iter().zip([&h1, &h2, &h3]) {
        assert_eq!(git(&f.work, &["rev-parse", &format!("{m}^1")]), prev);
        assert_eq!(git(&f.work, &["rev-parse", &format!("{m}^2")]), *head, "#{}", k.pr);
        prev = m.clone();
    }
    assert_eq!(prev, c.head);
    let tracker = at(&f, &c.head, TRACKER);
    let (r2, r10) = (tracker.find("| 002 |").unwrap(), tracker.find("| 010 |").unwrap());
    assert!(r2 < r10, "the earlier constituent's row first:\n{tracker}");
    let groups = at(&f, &c.head, crate::spec_compile::GROUPS_FILE);
    assert!(groups.contains("\"002-a2\" = \"a\"") && groups.contains("\"010-b10\" = \"b\""), "{groups}");
    assert!(c.settled.contains(&TRACKER.to_string()), "{:?}", c.settled);
    assert_eq!(at(&f, &c.head, "README.md"), "readme\nmore");
    // The compiled tree at the head is what the compile writes.
    git(&f.work, &["switch", "-q", "--detach", &c.head]);
    compile(&f.work);
    assert_eq!(git(&f.work, &["status", "--porcelain"]), "", "the compiled tree is not current");
    git(&f.work, &["switch", "-q", "main"]);
    // The manifest names each head and spec, and reads back.
    let m = parse_manifest(&c.manifest()).unwrap();
    assert_eq!(m.prs, vec![(1, h1, Some(super::fake::SPEC_PATH.to_string())), (2, h2, Some(OTHER_SPEC.to_string())), (3, h3, None)]);
    assert!(c.branch.starts_with("delphi-rollup/"));
    assert!(c.lines().iter().any(|l| l.contains("union-of-diffs")));
    assert_nothing_pushed(&f, &forge);
}

#[test]
fn a_conflict_outside_the_closed_list_leaves_that_pull_request_out_and_the_build_carries_on() {
    // AC-18 (c).
    let f = fixture();
    let h3 = branch(&f, "b3", |d| std::fs::write(d.join("README.md"), "readme, one way\n").unwrap());
    let h4 = branch(&f, "b4", |d| std::fs::write(d.join("README.md"), "readme, another way\n").unwrap());
    let h5 = branch(&f, "b5", |d| std::fs::write(d.join("OTHER.md"), "other\n").unwrap());
    let forge = FakeForge::default();
    ready(&forge, 3, "b3", &h3, &["README.md"], None);
    ready(&forge, 4, "b4", &h4, &["README.md"], None);
    ready(&forge, 5, "b5", &h5, &["OTHER.md"], None);
    let (_, out) = run(&f, &forge, &[3, 4, 5], 4);
    let c = out.unwrap();
    assert_eq!(c.merged.iter().map(|(k, _)| k.pr).collect::<Vec<_>>(), vec![3, 5]);
    assert_eq!(c.dropped.len(), 1);
    assert!(c.dropped[0].0 == 4 && c.dropped[0].1.contains("README.md"), "{:?}", c.dropped);
    assert_eq!(at(&f, &c.head, "README.md"), "readme, one way");
    assert_nothing_pushed(&f, &forge);
}

#[test]
fn a_gate_changing_draft_red_or_unapproved_pull_request_is_never_chosen_and_the_batch_stops_at_its_size() {
    // AC-18 (d): decided from the forge's facts before any build.
    let forge = FakeForge::default();
    for n in 1..=7 {
        ready(&forge, n, &format!("b{n}"), &format!("{n:040x}"), &["README.md"], None);
    }
    forge.st.borrow_mut().paths.insert(1, vec!["AGENTS.md".into()]);
    forge.edit(2, |p| p.is_draft = true);
    forge.st.borrow_mut().checks.insert(3, vec![Check { name: "test".into(), bucket: "fail".into() }]);
    ready(&forge, 4, "b4", &format!("{:040x}", 4), &["src/x.rs"], Some(OTHER_SPEC));
    forge.st.borrow_mut().files.insert(("main".into(), OTHER_SPEC.into()), DRAFT_SPEC.into());
    forge.edit(5, |p| p.labels.clear());
    forge.st.borrow_mut().behind.insert(format!("{:040x}", 6), 3);
    let c = choose(&forge, &[1, 2, 3, 4, 5, 6, 7], 1);
    let why = |n: u64| c.skipped.iter().find(|s| s.0 == n).map(|s| s.1.clone()).unwrap_or_default();
    assert!(why(1).contains("gate") && why(1).contains("AGENTS.md"), "{}", why(1));
    assert!(why(2).contains("draft"));
    assert!(why(3).contains("checks failed"));
    assert!(why(4).contains("not approved"), "{}", why(4));
    assert!(why(5).contains("not queued"));
    // Behind is no reason to leave a pull request out; the size is.
    assert_eq!(c.chosen.iter().map(|k| k.pr).collect::<Vec<_>>(), vec![6]);
    assert!(why(7).contains("full"));
    assert!(forge.st.borrow().labels.is_empty(), "choosing labels nothing");
}

#[test]
fn a_head_that_moved_after_it_was_chosen_is_left_out() {
    // §13.4 invariant 2: the remote branch must still be at the head the operator is shown.
    let f = fixture();
    let h3 = branch(&f, "b3", |d| std::fs::write(d.join("README.md"), "readme\nmore\n").unwrap());
    let h5 = branch(&f, "b5", |d| std::fs::write(d.join("OTHER.md"), "other\n").unwrap());
    let forge = FakeForge::default();
    ready(&forge, 3, "b3", &"e".repeat(40), &["README.md"], None);
    ready(&forge, 5, "b5", &h5, &["OTHER.md"], None);
    let (_, out) = run(&f, &forge, &[3, 5], 4);
    let c = out.unwrap();
    assert_eq!(c.merged.len(), 1);
    assert!(c.dropped[0].0 == 3 && c.dropped[0].1.contains(&h3[..10]), "{:?}", c.dropped);
    assert_nothing_pushed(&f, &forge);
}

#[test]
fn two_pull_requests_that_make_the_same_change_fail_the_union_check_and_nothing_is_built() {
    // AC-18 (e) on a real merge: git takes the second identical change silently, so the rollup changes the line once where the
    // constituents change it twice. Refused rather than guessed at.
    let f = fixture();
    let h3 = branch(&f, "b3", |d| std::fs::write(d.join("README.md"), "readme\nsame\n").unwrap());
    let h4 = branch(&f, "b4", |d| std::fs::write(d.join("README.md"), "readme\nsame\n").unwrap());
    let forge = FakeForge::default();
    ready(&forge, 3, "b3", &h3, &["README.md"], None);
    ready(&forge, 4, "b4", &h4, &["README.md"], None);
    let (_, out) = run(&f, &forge, &[3, 4], 4);
    let e = out.unwrap_err();
    assert!(e.contains("README.md") && e.contains("not the constituents' own"), "{e}");
    assert_nothing_pushed(&f, &forge);
}
