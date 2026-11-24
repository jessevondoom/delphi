//! The diff shown before a gate-changing merge, built locally (spec 094 §17, AC-28), against real git: a bare `origin`, a clone
//! that pushes the pull request (GitHub's `refs/pull/<n>/head` included) and the operator's own clone, which has not fetched it.

use super::approve::FakeTerminal;
use super::cleanup::Git;
use super::errors::Errors;
use super::fake::FakeForge;
use super::record::Ledger;
use super::show_diff::{fold, FOLD_ABOVE};
use super::{merge_one, Ctx, Outcome};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn g(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

const TRACKER: &str = "missions/2026-10-23-foundational-design/tracker";

struct Fx {
    _t: tempfile::TempDir,
    /// The operator's checkout: cloned before the pull request was pushed, so the head must be fetched.
    op: PathBuf,
    home: tempfile::TempDir,
    forge: FakeForge,
}

/// A base with a README, `AGENTS.md` and one compiled file; pull request 7 on `branch-7` makes the changes `on_pr` writes. The
/// fake forge reports it ready, gate-changing, with the derived base and head commits.
fn fixture(on_pr: impl FnOnce(&Path)) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let (origin, dev, op) = (root.join("origin.git"), root.join("dev"), root.join("op"));
    std::fs::create_dir(&origin).unwrap();
    g(&origin, &["init", "-q", "--bare", "-b", "main"]);
    g(&root, &["clone", "-q", origin.to_str().unwrap(), dev.to_str().unwrap()]);
    std::fs::create_dir_all(dev.join("compiled")).unwrap();
    std::fs::write(dev.join("README.md"), "readme\n").unwrap();
    std::fs::write(dev.join("AGENTS.md"), "rules\n").unwrap();
    std::fs::write(dev.join("compiled/index.md"), "index\n").unwrap();
    g(&dev, &["add", "-A"]);
    g(&dev, &["commit", "-q", "-m", "base"]);
    g(&dev, &["push", "-q", "origin", "HEAD:main"]);
    let base = g(&dev, &["rev-parse", "HEAD"]);
    g(&root, &["clone", "-q", origin.to_str().unwrap(), op.to_str().unwrap()]);
    g(&dev, &["switch", "-q", "-c", "branch-7"]);
    on_pr(&dev);
    g(&dev, &["add", "-A"]);
    g(&dev, &["commit", "-q", "-m", "the pull request"]);
    g(&dev, &["push", "-q", "origin", "branch-7", "HEAD:refs/pull/7/head"]);
    let head = g(&dev, &["rev-parse", "HEAD"]);
    let paths: Vec<String> = g(&dev, &["diff", "--name-only", "--no-renames", &format!("{base}...{head}")]).lines().map(String::from).collect();
    let forge = FakeForge::default();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    forge.with_ready_pr(7, &head, &refs);
    forge.edit(7, |p| {
        p.base_sha = base.clone();
        p.head_ref = "branch-7".into();
        p.labels.clear();
    });
    Fx { _t: t, op, home: tempfile::tempdir().unwrap(), forge }
}

fn run(fx: &Fx, term: &mut FakeTerminal, quote: Option<&str>) -> Outcome {
    let ledger = Ledger::at(fx.home.path());
    let errors = Errors::at(fx.home.path());
    let git = Git::at(&fx.op, &fx.op);
    let mut ctx = Ctx { forge: &fx.forge, ledger: &ledger, errors: &errors, term, quote: quote.map(String::from), dry_run: false, session: None, git: Some(&git), settle: Duration::ZERO, now: || "2026-11-24T00:00:00Z".to_string(), refresh: None, claims: None, rollup: None };
    merge_one(&mut ctx, 7, None)
}

fn write(dir: &Path, path: &str, body: &str) {
    let p = dir.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn a_small_gate_changing_pull_request_shows_its_stat_and_its_whole_diff_and_still_needs_the_number_typed() {
    let fx = fixture(|d| {
        write(d, "AGENTS.md", "rules\nA NEW RULE\n");
        write(d, "docs/x.md", "a doc\n");
        write(d, "compiled/index.md", "index\nA GENERATED LINE\n");
    });
    // A chat quote is still refused, and so is the wrong word; nothing merges.
    let q = run(&fx, &mut FakeTerminal::new(true, &["7"]), Some("merge #7"));
    assert!(matches!(&q, Outcome::Declined { why, .. } if why.contains("not accepted")), "{q:?}");
    let wrong = run(&fx, &mut FakeTerminal::new(true, &["yes"]), None);
    assert!(matches!(wrong, Outcome::Declined { .. }), "{wrong:?}");
    assert!(fx.forge.st.borrow().merged.is_empty());
    let mut term = FakeTerminal::new(true, &["7"]);
    let ok = run(&fx, &mut term, None);
    assert!(matches!(ok, Outcome::Merged { .. }), "{ok:?}");
    let shown = term.shown.join("\n");
    // The stat comes first, then the whole patch, generated path included: a small diff folds nothing.
    let stat_at = shown.find("3 files changed").expect(&shown);
    assert!(stat_at < shown.find("diff --git a/AGENTS.md").expect(&shown), "{shown}");
    assert!(shown.contains("+A NEW RULE") && shown.contains("+a doc") && shown.contains("+A GENERATED LINE"), "{shown}");
    assert!(!shown.contains("folded"), "{shown}");
}

#[test]
fn a_large_pull_request_folds_only_its_generated_paths_and_never_a_gate_path() {
    let fx = fixture(|d| {
        let body: String = (0..30).map(|i| format!("generated line {i}\n")).collect();
        for i in 0..200 {
            write(d, &format!("compiled/views/gen-{i:03}.md"), &body);
        }
        for i in 0..20 {
            write(d, &format!("{TRACKER}/094/S{i}.md"), &body);
        }
        write(d, "AGENTS.md", "rules\nA NEW RULE\n");
        write(d, "merge-gates.toml", "paths = [\"AGENTS.md\"]\n");
        write(d, "crates/delphi/src/merge/new.rs", "// a gate source\n");
        write(d, "src/lib.rs", "// ordinary code\n");
    });
    let mut term = FakeTerminal::new(true, &["7"]);
    let ok = run(&fx, &mut term, None);
    assert!(matches!(ok, Outcome::Merged { .. }), "{ok:?}");
    let shown = term.shown.join("\n");
    // The stat names every file, generated ones too, and the counts.
    assert!(shown.contains("224 files changed") && shown.contains("compiled/views/gen-199.md") && shown.contains("tracker/094/S19.md"), "{shown}");
    // Gate paths and ordinary code are in full; the generated patches are folded and counted, with the command to see them.
    for want in ["diff --git a/AGENTS.md", "+A NEW RULE", "diff --git a/merge-gates.toml", "diff --git a/crates/delphi/src/merge/new.rs", "diff --git a/src/lib.rs"] {
        assert!(shown.contains(want), "{want}: {shown}");
    }
    assert!(!shown.contains("diff --git a/compiled/") && !shown.contains(&format!("diff --git a/{TRACKER}")), "{shown}");
    assert!(shown.contains("220 generated file(s)") && shown.contains("git diff ") && shown.contains("-- compiled/ missions/"), "{shown}");
    assert!(shown.lines().count() < 1000, "the folded view is short: {} lines", shown.lines().count());
}

#[test]
fn a_gate_path_inside_a_generated_directory_is_never_folded() {
    let chunk = |p: &str| format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n{}", "+x\n".repeat(FOLD_ABOVE));
    let patch = format!("{}{}", chunk("compiled/gated.md"), chunk("compiled/other.md"));
    let shown = fold(" 2 files changed\n", &patch, "a...b", &["compiled/gated.md".to_string()]);
    assert_eq!((shown.folded_files, shown.text.contains("diff --git a/compiled/gated.md")), (1, true));
    assert!(!shown.text.contains("diff --git a/compiled/other.md"));
    // A header this cannot read is never folded either.
    let odd = format!("diff --git \"a/compiled/q\\\"x\" \"b/compiled/q\\\"x\"\n{}", "+y\n".repeat(FOLD_ABOVE + 1));
    assert_eq!(fold("", &odd, "a...b", &[]).folded_files, 0);
}

#[test]
fn a_head_that_cannot_be_fetched_fails_the_merge_and_merges_nothing() {
    let fx = fixture(|d| write(d, "AGENTS.md", "rules\nA NEW RULE\n"));
    let gone = "0123456789abcdef0123456789abcdef01234567";
    fx.forge.edit(7, |p| p.head_sha = gone.into());
    fx.forge.st.borrow_mut().files.insert((gone.into(), super::fake::SPEC_PATH.into()), super::fake::APPROVED_SPEC.into());
    let out = run(&fx, &mut FakeTerminal::new(true, &["7"]), None);
    assert!(matches!(&out, Outcome::Failed { detail, .. } if detail.contains("could not build the diff") && detail.contains("could not be fetched")), "{out:?}");
    assert!(fx.forge.st.borrow().merged.is_empty() && Ledger::at(fx.home.path()).read().is_empty());
}

#[test]
fn the_forge_is_never_asked_for_a_diff() {
    // GitHub refuses a large diff; the trait has no such call, and the `gh` forge starts no `pr diff`.
    let src = include_str!("forge.rs");
    assert!(!src.contains("\"pr\", \"diff\"") && !src.contains("fn diff("), "forge.rs must not read a diff from GitHub");
}
