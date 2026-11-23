//! `delphi merge refresh` against real git repositories with a local bare remote (spec 094 §11.10 V2, AC-10). No network.

use super::errors::Errors;
use super::fake::FakeForge;
use super::record::Ledger;
use super::refresh::{refresh, Done, Env, Report, Runner, Stop, TRACKER};
use crate::spec_compile::{self, GROUPS_FILE, TREE_DIR};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null").output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn spec_text(stem: &str) -> String {
    format!("---\nid: spec-{stem}\ntitle: \"{stem}: a spec\"\nstatus: approved\n---\n\n# Spec {stem}\n\n## 1. Scope and objectives\n\nWhat {stem} does.\n")
}

const GROUPS_HEAD: &str = "[[group]]\nid = \"a\"\ntitle = \"A\"\norder = 1\n\n[[group]]\nid = \"b\"\ntitle = \"B\"\norder = 2\n\n[specs]\n";
const BASE_SPECS: [(&str, &str); 5] = [("001-a1", "a"), ("003-a3", "a"), ("005-a5", "a"), ("007-b7", "b"), ("009-b9", "b")];
const TRACKER_BASE: &str = "# Tracker\n\n| Spec | Slice | State |\n| :-- | :-- | :-- |\n| 001 | S1 | merged |\n| 094 | S9 | not built |\n";

pub(super) struct Fixture {
    _t: tempfile::TempDir,
    pub(super) origin: PathBuf,
    pub(super) work: PathBuf,
    pub(super) home: tempfile::TempDir,
}

/// A bare origin and a clone with five specs in two groups, compiled, a tracker and a README, pushed as `main`.
pub(super) fn fixture() -> Fixture {
    let t = tempfile::tempdir().unwrap();
    let (origin, work) = (t.path().join("origin.git"), t.path().join("work"));
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    git(t.path(), &["clone", "-q", origin.to_str().unwrap(), work.to_str().unwrap()]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
        git(&work, &["config", k, v]);
    }
    std::fs::create_dir_all(work.join("specs")).unwrap();
    std::fs::create_dir_all(work.join(TRACKER).parent().unwrap()).unwrap();
    let mut groups = GROUPS_HEAD.to_string();
    for (stem, g) in BASE_SPECS {
        std::fs::write(work.join("specs").join(format!("{stem}.md")), spec_text(stem)).unwrap();
        groups.push_str(&format!("\"{stem}\" = \"{g}\"\n"));
    }
    std::fs::create_dir_all(work.join(GROUPS_FILE).parent().unwrap()).unwrap();
    std::fs::write(work.join(GROUPS_FILE), groups).unwrap();
    std::fs::write(work.join(TRACKER), TRACKER_BASE).unwrap();
    std::fs::write(work.join("README.md"), "readme\n").unwrap();
    compile(&work);
    commit(&work, "base");
    git(&work, &["push", "-q", "origin", "HEAD:main"]);
    Fixture { _t: t, origin, work, home: tempfile::tempdir().unwrap() }
}

pub(super) fn compile(dir: &Path) {
    let groups = spec_compile::parse_groups(&std::fs::read_to_string(dir.join(GROUPS_FILE)).unwrap()).unwrap();
    let out = spec_compile::build(&spec_compile::read_sources(dir).unwrap(), &groups).unwrap();
    spec_compile::write_all(dir, &out).unwrap();
}

pub(super) fn commit(dir: &Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// Add a spec: its file, its group line (after `after`, or at the end), optionally a tracker row; then compile.
pub(super) fn add_spec(dir: &Path, stem: &str, group: &str, after: Option<&str>, tracker_row: Option<&str>) {
    std::fs::write(dir.join("specs").join(format!("{stem}.md")), spec_text(stem)).unwrap();
    let line = format!("\"{stem}\" = \"{group}\"\n");
    let text = std::fs::read_to_string(dir.join(GROUPS_FILE)).unwrap();
    let text = match after {
        Some(a) => text.lines().map(|l| if l.starts_with(&format!("\"{a}\" = ")) { format!("{l}\n{line}") } else { format!("{l}\n") }).collect(),
        None => text + &line,
    };
    std::fs::write(dir.join(GROUPS_FILE), text).unwrap();
    if let Some(row) = tracker_row {
        let t = std::fs::read_to_string(dir.join(TRACKER)).unwrap();
        std::fs::write(dir.join(TRACKER), t + row + "\n").unwrap();
    }
    compile(dir);
}

/// Branch `pr` from main with `on_pr`, push it; then advance main with `on_main`. Returns the PR's head commit.
fn diverge(f: &Fixture, on_pr: impl FnOnce(&Path), on_main: impl FnOnce(&Path)) -> String {
    git(&f.work, &["switch", "-q", "-c", "pr"]);
    on_pr(&f.work);
    commit(&f.work, "the pull request");
    git(&f.work, &["push", "-q", "origin", "pr"]);
    let head = git(&f.work, &["rev-parse", "HEAD"]);
    git(&f.work, &["switch", "-q", "main"]);
    on_main(&f.work);
    commit(&f.work, "another pull request merged");
    git(&f.work, &["push", "-q", "origin", "main"]);
    head
}

/// A fake forge that knows pull request 7 on branch `pr` at `head`, one commit behind.
fn forge_for(head: &str, paths: &[&str]) -> FakeForge {
    let forge = FakeForge::default();
    forge.with_ready_pr(7, head, paths);
    forge.edit(7, |p| p.head_ref = "pr".into());
    forge.st.borrow_mut().behind.insert(head.to_string(), 1);
    forge
}

fn run(f: &Fixture, forge: &FakeForge, before_push: Option<&dyn Fn()>) -> (Result<Done, Stop>, Vec<String>) {
    let runner = Runner::new(&f.work);
    let (errors, ledger) = (Errors::at(f.home.path()), Ledger::at(f.home.path()));
    let env = Env { forge, errors: &errors, ledger: &ledger, home: f.home.path(), git: &runner, now: "2026-11-20T00:00:00Z".into(), before_push };
    let out = refresh(&env, 7);
    let log = runner.log.borrow().clone();
    (out, log)
}

fn origin_pr(f: &Fixture) -> String {
    git(&f.origin, &["rev-parse", "refs/heads/pr"])
}

/// The file at the tip of the origin's `pr` branch.
fn at_pr(f: &Fixture, path: &str) -> String {
    git(&f.origin, &["show", &format!("refs/heads/pr:{path}")])
}

fn refreshed(out: Result<Done, Stop>) -> Report {
    match out {
        Ok(Done::Refreshed(r)) => r,
        other => panic!("{other:?}"),
    }
}

fn assert_no_force_or_rebase(log: &[String]) {
    for l in log {
        assert!(!l.contains("--force") && !l.contains("rebase") && !l.split(' ').any(|w| w == "-f"), "a forbidden verb: {l}");
    }
}

#[test]
fn both_sides_added_a_spec_with_no_conflict_and_the_refresh_recompiles_and_checks_the_result() {
    // The shape of 2026-11-20 (§11.1 item 3): two branches each add a spec and the merge has no conflict at all. The count lines
    // that made that merge wrong are gone from the views (spec 088 amendment), but the compile still runs on every refresh, so
    // any view both sides changed the same way is regenerated and the result must pass `--check`.
    let f = fixture();
    let head = diverge(&f, |d| add_spec(d, "002-a2", "a", Some("001-a1"), None), |d| add_spec(d, "010-b10", "b", None, None));
    let forge = forge_for(&head, &["specs/002-a2.md"]);
    let (out, log) = run(&f, &forge, None);
    let r = refreshed(out);
    assert!(r.settled.is_empty(), "nothing conflicted: {r:?}");
    let index = at_pr(&f, &format!("{TREE_DIR}/index.md"));
    assert!(index.contains("| [002-a2]") && index.contains("| [010-b10]"), "{index}");
    // The pushed commit is a merge commit whose first parent is the old head and second the base tip.
    assert_eq!(origin_pr(&f), r.new_head);
    assert_eq!(git(&f.origin, &["rev-parse", "refs/heads/pr^1"]), head);
    assert_eq!(git(&f.origin, &["rev-parse", "refs/heads/pr^2"]), git(&f.origin, &["rev-parse", "refs/heads/main"]));
    assert_eq!(git(&f.origin, &["log", "-1", "--format=%s", "refs/heads/pr"]), "Merge main into pr (delphi merge refresh)");
    // The result is what a fresh compile writes.
    let check = f.work.join("..").join("check");
    git(&f.work, &["worktree", "add", "-q", "--detach", check.to_str().unwrap(), &r.new_head]);
    let groups = spec_compile::parse_groups(&std::fs::read_to_string(check.join(GROUPS_FILE)).unwrap()).unwrap();
    let built = spec_compile::build(&spec_compile::read_sources(&check).unwrap(), &groups).unwrap();
    assert!(spec_compile::check(&check, &groups, &built).is_empty());
    // AC-10 g, V4: no force, no rebase, a push was made, and nothing was merged on the forge; the worktree is gone.
    assert_no_force_or_rebase(&log);
    assert!(log.iter().any(|l| l.starts_with("push ")), "{log:?}");
    assert!(forge.st.borrow().merged.is_empty());
    assert!(!f.home.path().join("merge/refresh/pr-7").exists());
    // The books: a ledger row and a comment.
    let rows = Ledger::at(f.home.path()).read();
    assert_eq!((rows[0]["kind"].as_str(), rows[0]["old_head"].as_str(), rows[0]["new_head"].as_str()), (Some("refreshed"), Some(head.as_str()), Some(r.new_head.as_str())));
    assert!(forge.st.borrow().comments[0].1.contains("delphi merge refresh"));
}

#[test]
fn the_groups_file_and_the_tracker_are_settled_by_union_with_the_base_lines_first() {
    let f = fixture();
    let head = diverge(&f, |d| add_spec(d, "002-a2", "a", None, Some("| 002 | S1 | built |")), |d| add_spec(d, "010-b10", "b", None, Some("| 010 | S1 | merged |")));
    let forge = forge_for(&head, &["specs/002-a2.md"]);
    // An open `behind` entry becomes fixed-pending-ci.
    let errors = Errors::at(f.home.path());
    errors.record(7, &head, super::errors::Class::Behind, "it is 1 commit(s) behind `main`", "t0").unwrap();
    let (out, log) = run(&f, &forge, None);
    let r = refreshed(out);
    assert!(r.settled.contains(&GROUPS_FILE.to_string()) && r.settled.contains(&TRACKER.to_string()), "{r:?}");
    let groups = at_pr(&f, GROUPS_FILE);
    let (b10, a2) = (groups.find("\"010-b10\"").unwrap(), groups.find("\"002-a2\"").unwrap());
    assert!(b10 < a2, "the base's line first: {groups}");
    let tracker = at_pr(&f, TRACKER);
    assert!(tracker.ends_with("| 010 | S1 | merged |\n| 002 | S1 | built |"), "{tracker}");
    assert!(!tracker.contains("<<<<<<<") && !groups.contains("======="));
    let index = at_pr(&f, &format!("{TREE_DIR}/index.md"));
    assert!(index.contains("| [002-a2]") && index.contains("| [010-b10]") && !index.contains("<<<<<<<"), "{index}");
    assert_no_force_or_rebase(&log);
    assert_eq!(errors.entries()[0].state, "fixed-pending-ci");
}

#[test]
fn a_conflict_in_any_other_file_is_handed_to_merge_fix_and_nothing_is_pushed() {
    let f = fixture();
    let head = diverge(
        &f,
        |d| std::fs::write(d.join("README.md"), "the pull request's readme\n").unwrap(),
        |d| std::fs::write(d.join("README.md"), "the base's readme\n").unwrap(),
    );
    let forge = forge_for(&head, &["README.md"]);
    let (out, log) = run(&f, &forge, None);
    let msg = match &out {
        Err(s @ Stop::HandOver(why)) => {
            assert!(why.contains("README.md"), "{why}");
            s.line(7)
        }
        other => panic!("{other:?}"),
    };
    assert!(msg.contains("delphi merge fix 7") && msg.contains("nothing was pushed"), "{msg}");
    assert_eq!(origin_pr(&f), head, "nothing was pushed");
    assert!(log.iter().any(|l| l == "merge --abort"), "{log:?}");
    assert!(!log.iter().any(|l| l.starts_with("push") || l.starts_with("commit")), "{log:?}");
    let e = &Errors::at(f.home.path()).entries()[0];
    assert_eq!((e.class.as_str(), e.state.as_str()), ("refresh-refused", "open"));
    assert!(e.detail.contains("README.md"));
    assert!(!f.home.path().join("merge/refresh/pr-7").exists(), "the worktree is removed");
    assert!(Ledger::at(f.home.path()).read().is_empty());
}

#[test]
fn a_union_that_would_change_a_line_the_pull_request_did_not_change_is_refused_by_the_widening_check() {
    // Both sides edited the same tracker row: the union would keep both versions, so the result widens the change.
    let f = fixture();
    let edit = |to: &'static str| move |d: &Path| {
        let t = std::fs::read_to_string(d.join(TRACKER)).unwrap();
        std::fs::write(d.join(TRACKER), t.replace("| 094 | S9 | not built |", to)).unwrap();
    };
    let head = diverge(&f, edit("| 094 | S9 | built (PR open) |"), edit("| 094 | S9 | merged |"));
    let forge = forge_for(&head, &[TRACKER]);
    let (out, _) = run(&f, &forge, None);
    assert!(matches!(&out, Err(Stop::HandOver(why)) if why.contains("outside the generated files") && why.contains(TRACKER)), "{out:?}");
    assert_eq!(origin_pr(&f), head);
}

#[test]
fn a_branch_that_moves_before_the_push_rejects_it_and_nothing_is_overwritten() {
    let f = fixture();
    let head = diverge(&f, |d| add_spec(d, "002-a2", "a", Some("001-a1"), None), |d| add_spec(d, "010-b10", "b", None, None));
    // Somebody else's commit on top of the pull request, pushed while the refresh works.
    git(&f.work, &["switch", "-q", "-c", "elsewhere", &head]);
    std::fs::write(f.work.join("other.txt"), "x\n").unwrap();
    commit(&f.work, "someone else");
    let moved = git(&f.work, &["rev-parse", "HEAD"]);
    git(&f.work, &["switch", "-q", "main"]);
    let work = f.work.clone();
    let push_theirs = move || {
        git(&work, &["push", "-q", "origin", "elsewhere:pr"]);
    };
    let forge = forge_for(&head, &["specs/002-a2.md"]);
    let (out, log) = run(&f, &forge, Some(&push_theirs));
    assert!(matches!(&out, Err(Stop::HandOver(why)) if why.contains("push was rejected")), "{out:?}");
    assert_eq!(origin_pr(&f), moved, "their commit is still there");
    assert_no_force_or_rebase(&log);
    assert_eq!(Errors::at(f.home.path()).entries()[0].class, "refresh-refused");
}

#[test]
fn a_gate_changing_a_draft_a_forks_and_a_current_pull_request_are_refused_before_any_work() {
    let f = fixture();
    let head = git(&f.work, &["rev-parse", "HEAD"]);
    type Setup = Box<dyn Fn(&FakeForge)>;
    let cases: Vec<(&str, Setup)> = vec![
        (
            "changes a gate",
            Box::new(|fg| {
                fg.st.borrow_mut().paths.insert(7, vec!["AGENTS.md".into()]);
            }),
        ),
        ("draft", Box::new(|fg| fg.edit(7, |p| p.is_draft = true))),
        ("fork", Box::new(|fg| fg.edit(7, |p| p.cross_repo = true))),
        ("not open", Box::new(|fg| fg.edit(7, |p| p.state = "CLOSED".into()))),
    ];
    for (want, setup) in cases {
        let forge = forge_for(&head, &["a.rs"]);
        setup(&forge);
        let (out, log) = run(&f, &forge, None);
        assert!(matches!(&out, Err(Stop::Refused(why)) if why.contains(want)), "{want}: {out:?}");
        assert!(log.is_empty(), "{want}: no git ran: {log:?}");
    }
    let forge = forge_for(&head, &["a.rs"]);
    forge.st.borrow_mut().behind.clear();
    let (out, log) = run(&f, &forge, None);
    assert_eq!(out, Ok(Done::Current));
    assert!(log.is_empty() && Errors::at(f.home.path()).entries().is_empty());
}

#[test]
fn a_conflicting_pull_request_with_no_checks_is_refreshed_by_rule_or_handed_over_never_failed_on_its_checks() {
    // #890 on 2026-11-23: a conflicting pull request runs no CI, so it has no checks. Refresh does not need any (§16.2): it
    // settles a conflict on the closed list, and hands over one outside it, naming the file.
    let f = fixture();
    let head = diverge(&f, |d| add_spec(d, "002-a2", "a", None, Some("| 002 | S1 | built |")), |d| add_spec(d, "010-b10", "b", None, Some("| 010 | S1 | merged |")));
    let forge = forge_for(&head, &["specs/002-a2.md"]);
    forge.edit(7, |p| p.mergeable = "CONFLICTING".into());
    forge.st.borrow_mut().checks.insert(7, vec![]);
    let r = refreshed(run(&f, &forge, None).0);
    assert!(r.settled.contains(&TRACKER.to_string()), "{r:?}");

    let f = fixture();
    let head = diverge(&f, |d| std::fs::write(d.join("README.md"), "mine\n").unwrap(), |d| std::fs::write(d.join("README.md"), "theirs\n").unwrap());
    let forge = forge_for(&head, &["README.md"]);
    forge.edit(7, |p| p.mergeable = "CONFLICTING".into());
    forge.st.borrow_mut().checks.insert(7, vec![]);
    let (out, _) = run(&f, &forge, None);
    assert!(matches!(&out, Err(Stop::HandOver(why)) if why.contains("README.md")), "{out:?}");
    assert_eq!(origin_pr(&f), head);
}
