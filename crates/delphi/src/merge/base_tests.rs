//! The check of the base and `repair-base` (spec 094 §11.3.4, AC-11): the check and the repair against real git repositories with a
//! local bare remote, and `--all` and `--auto` against a fake forge and a fake check. No network.

use super::approve::FakeTerminal;
use super::auto::{run_checked, Caps, Clock};
use super::base::{check_base, repair_base, BaseCheck, BaseState, RepairEnv, Repaired};
use super::errors::Errors;
use super::fake::FakeForge;
use super::record::{Attestation, Ledger};
use super::refresh::Runner;
use super::{merge_checked, Ctx, Outcome};
use crate::spec_compile::{self, GROUPS_FILE, TREE_DIR};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null").output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn spec(dir: &Path, stem: &str) {
    let text = format!("---\nid: spec-{stem}\ntitle: \"{stem}: a spec\"\nstatus: approved\n---\n\n# Spec {stem}\n\n## 1. Scope and objectives\n\nWhat {stem} does.\n");
    std::fs::write(dir.join("specs").join(format!("{stem}.md")), text).unwrap();
    let groups = std::fs::read_to_string(dir.join(GROUPS_FILE)).unwrap_or_else(|_| "[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 1\n\n[specs]\n".into());
    std::fs::write(dir.join(GROUPS_FILE), format!("{groups}\"{stem}\" = \"a\"\n")).unwrap();
}

fn push_main(work: &Path, msg: &str) {
    git(work, &["add", "-A"]);
    git(work, &["commit", "-q", "-m", msg]);
    git(work, &["push", "-q", "origin", "HEAD:main"]);
}

/// A bare origin and a clone whose `main` has two specs, compiled and current.
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let (origin, work) = (t.path().join("origin.git"), t.path().join("work"));
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    git(t.path(), &["clone", "-q", origin.to_str().unwrap(), work.to_str().unwrap()]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
        git(&work, &["config", k, v]);
    }
    std::fs::create_dir_all(work.join("specs")).unwrap();
    std::fs::create_dir_all(work.join(GROUPS_FILE).parent().unwrap()).unwrap();
    spec(&work, "001-a");
    spec(&work, "002-b");
    spec_compile::compile_in(&work, false).unwrap();
    push_main(&work, "base");
    (t, origin, work)
}

#[test]
fn a_stale_base_is_found_recorded_and_repaired_by_a_pull_request_of_the_compiled_tree_only_that_merges_nothing() {
    let (_t, origin, work) = fixture();
    let home = tempfile::tempdir().unwrap();
    let runner = Runner::new(&work);
    let errors = Errors::at(home.path());
    assert!(matches!(check_base(&runner, home.path(), "main"), BaseState::Current { .. }));
    // A merge that brought a spec in without its compiled views: what a native merge of two spec pull requests can leave.
    spec(&work, "003-c");
    push_main(&work, "a merge that left the views stale");
    let state = check_base(&runner, home.path(), "main");
    assert!(matches!(&state, BaseState::Drift { detail, .. } if detail.contains("003-c")), "{state:?}");
    assert!(!home.path().join("merge/base-check").exists(), "the worktree is removed");
    let in_repo = super::base::InRepo { git: Runner::new(&work), home: home.path() };
    let (_, line) = super::base::check_and_note(&errors, &in_repo, "main", "t1");
    assert!(line.contains("DRIFTED") && line.contains("repair-base"), "{line}");
    let e = &errors.entries()[0];
    assert_eq!((e.pr, e.class.as_str(), e.state.as_str()), (0, "base-drift", "open"));
    assert!(e.line().contains(" base base-drift "), "{}", e.line());

    let forge = FakeForge::default();
    let ledger = Ledger::at(home.path());
    let env = RepairEnv { forge: &forge, errors: &errors, ledger: &ledger, home: home.path(), git: &runner, now: "t2".into() };
    // A dry run names the files and pushes nothing; with no terminal and no quote nothing is pushed either.
    let dry = repair_base(&env, "main", true, &mut FakeTerminal::new(false, &[]), None).unwrap();
    assert!(matches!(&dry, Repaired::WouldOpen { paths, .. } if !paths.is_empty() && paths.iter().all(|p| p.starts_with(TREE_DIR))), "{dry:?}");
    assert!(repair_base(&env, "main", false, &mut FakeTerminal::new(false, &[]), None).unwrap_err().contains("no terminal"));
    assert!(repair_base(&env, "main", false, &mut FakeTerminal::new(false, &[]), Some("merge all")).is_err());
    let branches = || git(&origin, &["branch", "--list"]);
    assert!(!branches().contains("repair-base"), "nothing was pushed: {}", branches());
    assert!(forge.st.borrow().opened.is_empty());

    let done = repair_base(&env, "main", false, &mut FakeTerminal::new(false, &[]), Some("repair main please")).unwrap();
    let Repaired::Opened { pr, branch, head, .. } = done.clone() else { panic!("{done:?}") };
    assert_eq!(pr, 900);
    assert_eq!(git(&origin, &["rev-parse", &format!("refs/heads/{branch}")]), head);
    // Only the compiled tree changed, on top of the base tip; the commit carries the compile spec's trailer.
    let changed = git(&origin, &["diff", "--name-only", "main", &branch]);
    assert!(!changed.is_empty() && changed.lines().all(|p| p.starts_with(&format!("{TREE_DIR}/"))), "{changed}");
    assert_eq!(git(&origin, &["rev-parse", &format!("{branch}^")]), git(&origin, &["rev-parse", "main"]));
    assert!(git(&origin, &["log", "-1", "--format=%B", &branch]).contains("Implements-Spec: specs/088-"));
    let s = forge.st.borrow();
    assert_eq!((s.opened[0].0.as_str(), s.opened[0].1.as_str()), ("main", branch.as_str()));
    assert!(s.merged.is_empty(), "a repair never merges");
    drop(s);
    let log = runner.log.borrow().clone();
    assert!(log.iter().all(|l| !l.contains("--force") && !l.contains("rebase") && !l.split(' ').any(|w| w == "-f")), "{log:?}");
    assert!(!home.path().join("merge/repair-base").exists());
    assert_eq!(Ledger::at(home.path()).read().last().unwrap()["kind"], "repair_base");
    assert_eq!(errors.entries()[0].state, "fixed-pending-ci");

    // The operator merges the repair; the next check finds the base current and resolves the entry.
    git(&work, &["fetch", "-q", "origin"]);
    git(&work, &["push", "-q", "origin", &format!("refs/remotes/origin/{branch}:refs/heads/main")]);
    super::base::check_and_note(&errors, &in_repo, "main", "t3");
    assert_eq!(errors.entries()[0].state, "resolved");
    assert!(matches!(repair_base(&env, "main", false, &mut FakeTerminal::new(true, &[]), None).unwrap(), Repaired::Current { .. }));
}

#[test]
fn a_bad_base_name_and_a_repository_with_no_groups_file_are_not_taken_for_drift() {
    let (_t, _origin, work) = fixture();
    let home = tempfile::tempdir().unwrap();
    let runner = Runner::new(&work);
    assert!(matches!(check_base(&runner, home.path(), "-x"), BaseState::Unchecked(_)));
    assert!(matches!(check_base(&runner, home.path(), "nope"), BaseState::Unchecked(_)));
    std::fs::remove_dir_all(work.join("compiled")).unwrap();
    push_main(&work, "no compiled tree at all");
    assert!(matches!(check_base(&runner, home.path(), "main"), BaseState::Current { .. }));
}

/// A check whose answer a closure gives.
struct Fixed<'a>(Box<dyn Fn(&str) -> BaseState + 'a>);

impl BaseCheck for Fixed<'_> {
    fn check(&self, base_ref: &str) -> BaseState {
        (self.0)(base_ref)
    }
}

/// Drift once the fake forge has merged `after` pull requests.
fn drift_after(forge: &FakeForge, after: usize) -> Fixed<'_> {
    Fixed(Box::new(move |_| if forge.st.borrow().merged.len() >= after { BaseState::Drift { sha: "b1".into(), detail: "stale".into() } } else { BaseState::Current { sha: "b0".into() } }))
}

fn ready(n: &[u64]) -> FakeForge {
    let f = FakeForge::default();
    for k in n {
        f.with_ready_pr(*k, &format!("h{k}"), &["docs/a.md"]);
    }
    f
}

fn with_ctx<R>(forge: &FakeForge, home: &Path, quote: Option<&str>, f: impl FnOnce(&mut Ctx) -> R) -> R {
    let (ledger, errors) = (Ledger::at(home), Errors::at(home));
    let mut term = FakeTerminal::new(false, &[]);
    let mut ctx = Ctx { forge, ledger: &ledger, errors: &errors, term: &mut term, quote: quote.map(String::from), dry_run: false, git: None, session: None, settle: Duration::ZERO, now: || "2026-11-22T00:00:00Z".to_string(), refresh: None, claims: None, rollup: None };
    f(&mut ctx)
}

#[test]
fn all_stops_after_a_merge_that_leaves_the_base_stale_and_a_drifted_base_stops_it_before_it_starts() {
    let att = Attestation::ChatQuote("merge all".into());
    let (forge, home) = (ready(&[1, 2, 3]), tempfile::tempdir().unwrap());
    let c = with_ctx(&forge, home.path(), None, |ctx| merge_checked(ctx, &[1, 2, 3], Some(&att), &drift_after(&forge, 1), true));
    assert_eq!(c.outcomes.len(), 1);
    assert!(matches!(c.outcomes[0], Outcome::Merged { pr: 1, .. }));
    assert!(c.drifted && c.lines.last().unwrap().ends_with("not merged: #2, #3"), "{:?}", c.lines);
    assert_eq!(forge.st.borrow().merged.len(), 1);
    let e = &Errors::at(home.path()).entries()[0];
    assert_eq!((e.pr, e.class.as_str()), (0, "base-drift"));
    // The base is still stale at the next run: nothing merges at all.
    let c = with_ctx(&forge, home.path(), None, |ctx| merge_checked(ctx, &[2, 3], Some(&att), &drift_after(&forge, 1), true));
    assert!(c.outcomes.is_empty() && c.drifted, "{:?}", c.lines);
    assert_eq!(Errors::at(home.path()).entries()[0].attempts, 2, "the same base commit is one more attempt");
    // `<PR>` (no stop): the drift is recorded and said, and the next pull request the operator named still merges.
    let (forge, home) = (ready(&[1, 2]), tempfile::tempdir().unwrap());
    let c = with_ctx(&forge, home.path(), Some("merge #1 and #2"), |ctx| merge_checked(ctx, &[1, 2], None, &drift_after(&forge, 1), false));
    assert_eq!(forge.st.borrow().merged.len(), 2, "{:?}", c.outcomes);
    assert!(c.drifted && c.lines.iter().all(|l| l.contains("DRIFTED")), "{:?}", c.lines);
    // A dry run checks nothing.
    let (forge, home) = (ready(&[1]), tempfile::tempdir().unwrap());
    let c = with_ctx(&forge, home.path(), None, |ctx| {
        ctx.dry_run = true;
        merge_checked(ctx, &[1], Some(&att), &drift_after(&forge, 0), true)
    });
    assert!(!c.drifted && c.lines.is_empty() && Errors::at(home.path()).entries().is_empty());
}

#[test]
fn an_auto_session_ends_on_a_drifted_base_at_a_poll_or_after_its_own_merge() {
    struct Clk(Cell<u64>);
    impl Clock for Clk {
        fn now_secs(&self) -> u64 {
            self.0.get()
        }
        fn sleep(&self, d: Duration) {
            self.0.set(self.0.get() + d.as_secs());
        }
    }
    let caps = Caps { max_prs: 10, run_for: Duration::from_secs(3600), pause_after_errors: 5, idle: Duration::from_secs(600), poll: Duration::from_secs(60), batch: None };
    let att = Attestation::ChatQuote("auto".into());
    for (after, merged) in [(1, 1), (0, 0)] {
        let (forge, home) = (ready(&[1, 2]), tempfile::tempdir().unwrap());
        let check = drift_after(&forge, after);
        let s = with_ctx(&forge, home.path(), None, |ctx| run_checked(ctx, &caps, &att, &Clk(Cell::new(0)), &|| false, &mut |_| {}, Some(&check)));
        assert_eq!(s.merged.len(), merged, "{s:?}");
        assert!(s.reason.contains("base drifted") && s.errors == 1, "{s:?}");
        assert_eq!(Errors::at(home.path()).entries()[0].class, "base-drift");
    }
}
