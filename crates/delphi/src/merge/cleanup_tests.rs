//! Cleanup against real git: a bare `origin`, a clone, real branches and real worktrees, all in temp directories.

use super::cleanup::{cleanup_pr, sweep_plan, Git, Report};
use super::fake::FakeForge;
use std::path::{Path, PathBuf};
use std::process::Command;

fn g(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git").arg("-C").arg(dir).args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A bare `origin`, a clone `work` on `main`, and room for worktrees; all under one temp directory.
struct Repo {
    _t: tempfile::TempDir,
    root: PathBuf,
    work: PathBuf,
}

fn repo() -> Repo {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let origin = root.join("origin.git");
    std::fs::create_dir(&origin).unwrap();
    g(&origin, &["init", "--bare", "-b", "main"]);
    let work = root.join("work");
    std::fs::create_dir(&work).unwrap();
    g(&work, &["init", "-b", "main"]);
    g(&work, &["remote", "add", "origin", origin.to_str().unwrap()]);
    std::fs::write(work.join("a.txt"), "a").unwrap();
    g(&work, &["add", "."]);
    g(&work, &["commit", "-m", "first"]);
    g(&work, &["push", "-u", "origin", "main"]);
    g(&work, &["remote", "set-head", "origin", "main"]);
    Repo { _t: t, root, work }
}

impl Repo {
    /// Branch `name` with one commit, pushed, then merged into `main` with a merge commit and pushed (what GitHub does).
    fn merged_branch(&self, name: &str) -> String {
        g(&self.work, &["checkout", "-b", name]);
        std::fs::write(self.work.join(format!("{name}.txt")), name).unwrap();
        g(&self.work, &["add", "."]);
        g(&self.work, &["commit", "-m", name]);
        g(&self.work, &["push", "-u", "origin", name]);
        let tip = g(&self.work, &["rev-parse", "HEAD"]);
        g(&self.work, &["checkout", "main"]);
        g(&self.work, &["merge", "--no-ff", "-m", "merge", name]);
        g(&self.work, &["push", "origin", "main"]);
        tip
    }
    fn worktree(&self, name: &str, branch: &str) -> PathBuf {
        let p = self.root.join(name);
        g(&self.work, &["worktree", "add", p.to_str().unwrap(), branch]);
        p
    }
    fn git(&self) -> Git {
        Git::at(&self.work, &self.work)
    }
    fn local(&self, b: &str) -> bool {
        !g(&self.work, &["branch", "--list", b]).is_empty()
    }
    fn remote(&self, b: &str) -> bool {
        !g(&self.work, &["ls-remote", "--heads", "origin", b]).is_empty()
    }
}

fn forge(pr: u64, branch: &str, tip: &str, state: &str) -> FakeForge {
    let f = FakeForge::default();
    f.with_ready_pr(pr, tip, &[]);
    f.edit(pr, |p| {
        p.head_ref = branch.into();
        p.state = state.into();
    });
    f
}

fn said(r: &Report, needle: &str) -> bool {
    r.lines.iter().any(|l| l.contains(needle))
}

#[test]
fn a_merged_branch_loses_remote_worktree_and_local_branch() {
    let r = repo();
    let tip = r.merged_branch("feat");
    let wt = r.worktree("wt-feat", "feat");
    let rep = cleanup_pr(&r.git(), &forge(7, "feat", &tip, "MERGED"), 7, false);
    assert!(rep.problems.is_empty(), "{:?}", rep.lines);
    assert!(!r.remote("feat") && !r.local("feat") && !wt.exists(), "{:?}", rep.lines);
    assert_eq!(rep.lines.len(), 3, "one line per step: {:?}", rep.lines);
    assert!(r.local("main") && r.remote("main"));
}

#[test]
fn an_unmerged_local_branch_is_kept_and_reported() {
    let r = repo();
    let tip = r.merged_branch("feat");
    g(&r.work, &["checkout", "feat"]);
    std::fs::write(r.work.join("extra.txt"), "x").unwrap();
    g(&r.work, &["add", "."]);
    g(&r.work, &["commit", "-m", "later work"]);
    g(&r.work, &["checkout", "main"]);
    let rep = cleanup_pr(&r.git(), &forge(7, "feat", &tip, "MERGED"), 7, false);
    assert!(r.local("feat"), "a branch with commits that are not in origin/main is never deleted, even though -D would force it");
    assert!(said(&rep, "not in origin/main"), "{:?}", rep.lines);
    assert!(!r.remote("feat"), "the remote branch was merged and is removed");
}

#[test]
fn a_branch_merged_on_the_forge_is_removed_even_though_the_local_main_is_stale() {
    // What happened to the first live `--auto` session: the merge happened on GitHub, so the local `main` has not seen it and
    // `git branch -d` calls the branch unmerged. The ancestry check against the fetched `origin/main` is what proves it.
    let r = repo();
    g(&r.work, &["checkout", "-b", "feat"]);
    std::fs::write(r.work.join("feat.txt"), "f").unwrap();
    g(&r.work, &["add", "."]);
    g(&r.work, &["commit", "-m", "feat"]);
    g(&r.work, &["push", "-u", "origin", "feat"]);
    let tip = g(&r.work, &["rev-parse", "HEAD"]);
    g(&r.work, &["checkout", "main"]);
    let origin = r.root.join("origin.git");
    let other = r.root.join("other");
    g(&r.root, &["clone", origin.to_str().unwrap(), "other"]);
    g(&other, &["merge", "--no-ff", "-m", "merge feat", "origin/feat"]);
    g(&other, &["push", "origin", "main"]);
    // The working clone's `main` is still the first commit; plain `-d` would refuse here.
    let stale = g(&r.work, &["rev-parse", "main"]);
    let rep = cleanup_pr(&r.git(), &forge(7, "feat", &tip, "MERGED"), 7, false);
    assert!(!r.local("feat") && !r.remote("feat"), "{:?}", rep.lines);
    assert_eq!(g(&r.work, &["rev-parse", "main"]), stale, "cleanup does not move the local main");
    assert!(rep.problems.is_empty(), "{:?}", rep.problems);
}

#[test]
fn a_locked_worktree_is_kept() {
    let r = repo();
    let tip = r.merged_branch("feat");
    let wt = r.worktree("wt-feat", "feat");
    g(&r.work, &["worktree", "lock", wt.to_str().unwrap()]);
    let rep = cleanup_pr(&r.git(), &forge(7, "feat", &tip, "MERGED"), 7, false);
    assert!(wt.exists() && r.local("feat"), "{:?}", rep.lines);
    assert!(said(&rep, "it is locked"));
}

#[test]
fn a_dirty_worktree_is_kept() {
    let r = repo();
    let tip = r.merged_branch("feat");
    let wt = r.worktree("wt-feat", "feat");
    std::fs::write(wt.join("scratch.txt"), "unsaved").unwrap();
    let rep = cleanup_pr(&r.git(), &forge(7, "feat", &tip, "MERGED"), 7, false);
    assert!(wt.join("scratch.txt").exists() && r.local("feat"), "{:?}", rep.lines);
    assert!(said(&rep, "uncommitted changes"));
}

#[test]
fn the_worktree_this_command_runs_in_is_kept() {
    let r = repo();
    let tip = r.merged_branch("feat");
    let wt = r.worktree("wt-feat", "feat");
    let inside = wt.join("sub");
    std::fs::create_dir(&inside).unwrap();
    let git = Git::at(&r.work, &inside);
    let rep = cleanup_pr(&git, &forge(7, "feat", &tip, "MERGED"), 7, false);
    assert!(wt.exists() && r.local("feat"), "{:?}", rep.lines);
    assert!(said(&rep, "directory this command runs in"));
}

#[test]
fn the_base_and_default_branches_are_refused() {
    let r = repo();
    let tip = g(&r.work, &["rev-parse", "main"]);
    let rep = cleanup_pr(&r.git(), &forge(7, "main", &tip, "MERGED"), 7, false);
    assert!(r.local("main") && r.remote("main") && said(&rep, "base or default"), "{:?}", rep.lines);
    // The default branch of a repository whose default is not `main`, named by a pull request into some other base.
    g(&r.work, &["branch", "trunk"]);
    g(&r.work, &["push", "origin", "trunk"]);
    g(&r.work, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/trunk"]);
    let f = forge(8, "trunk", &tip, "MERGED");
    f.edit(8, |p| p.base_ref = "release".into());
    cleanup_pr(&r.git(), &f, 8, false);
    assert!(r.local("trunk") && r.remote("trunk"));
}

#[test]
fn a_pull_request_that_is_not_merged_removes_nothing() {
    let r = repo();
    let tip = r.merged_branch("feat");
    for state in ["OPEN", "CLOSED"] {
        let rep = cleanup_pr(&r.git(), &forge(7, "feat", &tip, state), 7, false);
        assert!(r.local("feat") && r.remote("feat"), "{:?}", rep.lines);
        assert!(said(&rep, "not merged"));
    }
}

#[test]
fn a_branch_that_another_open_pull_request_is_based_on_keeps_its_remote() {
    let r = repo();
    let tip = r.merged_branch("feat");
    let f = forge(7, "feat", &tip, "MERGED");
    f.with_ready_pr(9, "head9", &[]);
    f.edit(9, |p| p.base_ref = "feat".into());
    let rep = cleanup_pr(&r.git(), &f, 7, false);
    assert!(r.remote("feat"), "{:?}", rep.lines);
    assert!(said(&rep, "open pull request uses it as its base"));
    assert!(!r.local("feat"), "only the remote branch is protected");
}

#[test]
fn a_remote_branch_with_another_tip_is_not_the_merged_one() {
    let r = repo();
    r.merged_branch("feat");
    let rep = cleanup_pr(&r.git(), &forge(7, "feat", "0000000000000000000000000000000000000000", "MERGED"), 7, false);
    assert!(r.remote("feat") && said(&rep, "not the commit that was merged"), "{:?}", rep.lines);
}

#[test]
fn dry_run_changes_nothing() {
    let r = repo();
    let tip = r.merged_branch("feat");
    let wt = r.worktree("wt-feat", "feat");
    let rep = cleanup_pr(&r.git(), &forge(7, "feat", &tip, "MERGED"), 7, true);
    assert!(r.remote("feat") && r.local("feat") && wt.exists());
    assert_eq!(rep.lines.len(), 3);
    assert!(rep.lines.iter().all(|l| l.contains("would")), "{:?}", rep.lines);
}

#[test]
fn the_sweep_lists_only_merged_branches_that_still_exist() {
    let r = repo();
    let tip = r.merged_branch("feat");
    let f = forge(7, "feat", &tip, "MERGED");
    f.with_ready_pr(8, "h8", &[]);
    f.edit(8, |p| {
        p.head_ref = "long-gone".into();
        p.state = "MERGED".into();
    });
    f.with_ready_pr(9, "h9", &[]);
    f.edit(9, |p| p.head_ref = "feat".into());
    assert_eq!(sweep_plan(&r.git(), &f, 100).unwrap(), vec![7]);
}

#[test]
fn a_cleanup_failure_does_not_change_the_merge_outcome() {
    use super::approve::FakeTerminal;
    use super::record::Ledger;
    use super::{after_merge, check::evaluate, merge_one, Ctx, Outcome};
    let broken = Git::at(Path::new("/nonexistent/delphi-cleanup-test"), Path::new("/"));
    let f = FakeForge::default();
    f.with_ready_pr(7, "head7", &["docs/x.md"]);
    let home = tempfile::tempdir().unwrap();
    let ledger = Ledger::at(home.path());
    let errors = crate::merge::errors::Errors::at(home.path());
    let mut term = FakeTerminal::new(false, &[]);
    // An earlier conflict entry for the same pull request: the merge resolves it, and the cleanup problem is a new, open one.
    errors.record(7, "head6", crate::merge::errors::Class::Conflict, "it conflicts with `main`", "t0").unwrap();
    let mut ctx = Ctx { forge: &f, ledger: &ledger, errors: &errors, term: &mut term, quote: Some("please merge #7".into()), dry_run: false, git: Some(&broken), session: None, settle: std::time::Duration::ZERO, now: || "2026-11-07T00:00:00Z".to_string() };
    assert_eq!(merge_one(&mut ctx, 7, None), Outcome::Merged { pr: 7, head: "head7".into() });
    let by_class = |class: &str| errors.entries().into_iter().find(|e| e.class == class).map(|e| e.state);
    assert_eq!(by_class("conflict").as_deref(), Some("resolved"), "the merge settled the earlier problem");
    assert_eq!(by_class("cleanup").as_deref(), Some("open"), "a cleanup that could not finish stays open for a person");
    let v = evaluate(&f, 7).unwrap();
    assert!(!after_merge(&mut ctx, &v).is_empty(), "the failure is returned, not raised");
}
