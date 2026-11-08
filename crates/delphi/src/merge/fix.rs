//! `delphi merge fix <PR>` (spec 094 §3.6): hand an open error entry to the builder.
//!
//! This prepares the ground and keeps the books; it does not resolve anything itself. It puts a worktree on the pull request's own
//! branch (or points at the one where that branch is already checked out), writes a brief, and moves the entry to `fixing`.
//! The builder then works with the `git-merge-and-conflict-resolution` skill and reports with `--done` or `--needs-human`.
//! A new head commit voids the earlier approval, so a fixed pull request goes back through the queue.

use super::errors::{Class, Entry, Errors};
use super::forge::{Forge, PrInfo};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository's own git, run in one place (one `Command::new`, listed in `spawn_sites.toml` as class `git`).
pub struct Git {
    pub repo: PathBuf,
}

impl Git {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new("git").arg("-C").arg(&self.repo).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| format!("cannot run git: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }

    /// The path of a worktree that has `branch` checked out, from `git worktree list --porcelain`.
    pub fn worktree_of(&self, branch: &str) -> Result<Option<PathBuf>, String> {
        Ok(worktree_for_branch(&self.run(&["worktree", "list", "--porcelain"])?, branch))
    }
}

/// Parse `git worktree list --porcelain` and find the worktree whose branch is `branch`.
pub fn worktree_for_branch(porcelain: &str, branch: &str) -> Option<PathBuf> {
    let want = format!("refs/heads/{branch}");
    let mut path: Option<PathBuf> = None;
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if line.strip_prefix("branch ") == Some(want.as_str()) {
            return path;
        }
    }
    None
}

#[derive(Debug, PartialEq)]
pub struct Plan {
    pub entry: Entry,
    pub worktree: PathBuf,
    /// The branch was already checked out here; nothing was created.
    pub reused: bool,
    pub brief_path: PathBuf,
    pub brief: String,
}

/// The newest entry for `pr` that is still open and not already handed to a person.
fn open_entry(errors: &Errors, pr: u64) -> Option<Entry> {
    errors.entries().into_iter().rev().find(|e| e.pr == pr && e.open() && e.state != "needs-human")
}

pub fn brief(entry: &Entry, pr: &PrInfo, worktree: &Path) -> String {
    format!(
        "# Fix pull request #{n}: {class}\n\n\
         Pull request: #{n} \"{title}\" (branch `{branch}`, base `{base}`)\n\
         Error entry: {id}, class `{class}`, seen {attempts} time(s), recorded on head `{head}`\n\
         What was seen: {detail}\n\
         Worktree: {wt}\n\n\
         ## Do\n\
         1. Work only in that worktree, on the branch `{branch}`. Use the `git-merge-and-conflict-resolution` skill.\n\
         2. Bring the base in by **merging** `origin/{base}` into the branch (this repository merges with merge commits). Never rebase or force-push this branch.\n\
         3. Resolve what conflicts; fix what fails. Run the gates before you push: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo run -q --locked -p delphi -- lint --repo .`.\n\
         4. Push an ordinary commit to `{branch}`, then run `delphi merge fix {n} --done`.\n\n\
         ## Stop and hand to a person (`delphi merge fix {n} --needs-human \"<why>\"`) when\n\
         - the conflict touches a governed or gate file: GOVERNANCE.md, REVIEW.md, AGENTS.md, context/heuristics.md, .github/**, deny.toml, merge-gates.toml, or an approved spec;\n\
         - more than 10 files conflict;\n\
         - tests fail after a resolution you believe is correct;\n\
         - you cannot tell what either side meant.\n\n\
         A new head commit voids the earlier approval: after `--done` the pull request goes back through the queue.\n",
        n = pr.number,
        title = pr.title.replace('"', "'"),
        branch = pr.head_ref,
        base = pr.base_ref,
        id = entry.id,
        class = entry.class,
        attempts = entry.attempts,
        head = entry.head_sha,
        detail = entry.detail,
        wt = worktree.display(),
    )
}

/// Prepare the fix for pull request `n`: a worktree on its branch, a brief, and the entry in state `fixing`.
pub fn prepare(git: &Git, forge: &dyn Forge, errors: &Errors, home: &Path, n: u64, now: &str) -> Result<Plan, String> {
    let entry = open_entry(errors, n).ok_or_else(|| format!("there is no open error entry for #{n} (see `delphi merge log --open`)"))?;
    if !Class::from_slug(&entry.class).is_some_and(|c| c.builder_can_fix()) {
        return Err(format!("{} is a `{}` problem, which needs a person and not a branch fix; resolve it and the entry closes when the pull request merges", entry.id, entry.class));
    }
    let pr = forge.pr(n).map_err(|e| e.to_string())?;
    if pr.state != "OPEN" {
        return Err(format!("#{n} is {}, not open: there is nothing to fix", pr.state.to_lowercase()));
    }
    git.run(&["fetch", "-q", "origin", &pr.head_ref])?;
    let (worktree, reused) = match git.worktree_of(&pr.head_ref)? {
        Some(p) => (p, true),
        None => {
            let dir = home.join("merge").join("fix").join(format!("pr-{n}"));
            if dir.exists() {
                return Err(format!("{} already exists but has no worktree on `{}`; remove it (`git worktree prune`, then delete the directory) and run this again", dir.display(), pr.head_ref));
            }
            std::fs::create_dir_all(dir.parent().expect("has a parent")).map_err(|e| e.to_string())?;
            let path = dir.to_string_lossy().to_string();
            let remote = format!("origin/{}", pr.head_ref);
            // A local branch of that name that is not checked out anywhere is reused; otherwise one is made from the remote.
            if git.run(&["rev-parse", "--verify", "-q", &format!("refs/heads/{}", pr.head_ref)]).is_ok() {
                git.run(&["worktree", "add", &path, &pr.head_ref])?;
                git.run(&["-C", &path, "merge", "--ff-only", &remote]).map_err(|e| format!("the local branch `{}` is not a fast-forward of {remote} ({e}); settle that first", pr.head_ref))?;
            } else {
                git.run(&["worktree", "add", "-b", &pr.head_ref, &path, &remote])?;
            }
            (dir, false)
        }
    };
    let text = brief(&entry, &pr, &worktree);
    let brief_path = home.join("merge").join("fix").join(format!("pr-{n}.brief.md"));
    std::fs::create_dir_all(brief_path.parent().expect("has a parent")).map_err(|e| e.to_string())?;
    std::fs::write(&brief_path, &text).map_err(|e| e.to_string())?;
    errors.set_state(&entry.id, "fixing", &format!("worktree {}", worktree.display()), now)?;
    let entry = errors.entries().into_iter().find(|e| e.id == entry.id).ok_or("entry vanished")?;
    Ok(Plan { entry, worktree, reused, brief_path, brief: text })
}

/// The builder pushed a fix: the head commit must have moved since the error was recorded.
pub fn done(forge: &dyn Forge, errors: &Errors, n: u64, now: &str) -> Result<String, String> {
    let entry = errors.entries().into_iter().rev().find(|e| e.pr == n && e.state == "fixing").ok_or_else(|| format!("#{n} has no entry in state `fixing` (run `delphi merge fix {n}` first)"))?;
    let pr = forge.pr(n).map_err(|e| e.to_string())?;
    if pr.head_sha == entry.head_sha {
        return Err(format!("the head commit of #{n} is still `{}`, the one the error was recorded on: push your fix first", entry.head_sha));
    }
    errors.set_state(&entry.id, "fixed-pending-ci", &format!("pushed {}", pr.head_sha), now)?;
    Ok(format!("{}: fixed-pending-ci. #{n} has a new head (`{}`), so the earlier approval no longer applies; once its checks pass it can be queued again (`delphi merge queue {n}`)", entry.id, pr.head_sha))
}

/// The builder stopped: a person must look.
pub fn needs_human(errors: &Errors, n: u64, why: &str, now: &str) -> Result<String, String> {
    if why.trim().is_empty() {
        return Err("--needs-human needs the reason, in words".into());
    }
    let entry = errors.entries().into_iter().rev().find(|e| e.pr == n && e.open()).ok_or_else(|| format!("#{n} has no open error entry"))?;
    errors.set_state(&entry.id, "needs-human", why, now)?;
    Ok(format!("{}: needs-human ({}). It stays on `delphi merge log --open` until a person resolves it", entry.id, why.trim()))
}

#[cfg(test)]
mod tests {
    use super::super::fake::FakeForge;
    use super::*;
    use std::process::Command as C;

    fn git(dir: &Path, args: &[&str]) {
        let out = C::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A bare origin and a clone of it, with `main` and a pushed branch `feature` that is behind main.
    fn repos() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let (origin, clone) = (t.path().join("origin.git"), t.path().join("clone"));
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        git(t.path(), &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()]);
        std::fs::write(clone.join("a.txt"), "a\n").unwrap();
        git(&clone, &["add", "."]);
        git(&clone, &["commit", "-q", "-m", "base"]);
        git(&clone, &["push", "-q", "origin", "HEAD:main"]);
        git(&clone, &["switch", "-q", "-c", "feature"]);
        std::fs::write(clone.join("b.txt"), "b\n").unwrap();
        git(&clone, &["add", "."]);
        git(&clone, &["commit", "-q", "-m", "feature"]);
        git(&clone, &["push", "-q", "origin", "feature"]);
        git(&clone, &["switch", "-q", "main"]);
        git(&clone, &["branch", "-q", "-D", "feature"]);
        (t, origin, clone)
    }

    fn world() -> (FakeForge, tempfile::TempDir) {
        let f = FakeForge::default();
        f.with_ready_pr(7, "head-old", &["b.txt"]);
        f.edit(7, |p| p.head_ref = "feature".into());
        (f, tempfile::tempdir().unwrap())
    }

    #[test]
    fn porcelain_output_is_read_for_the_branch_checked_out() {
        let p = "worktree /r\nHEAD abc\nbranch refs/heads/main\n\nworktree /r/wt\nHEAD def\nbranch refs/heads/feature\n\nworktree /r/det\nHEAD 123\ndetached\n";
        assert_eq!(worktree_for_branch(p, "feature"), Some(PathBuf::from("/r/wt")));
        assert_eq!(worktree_for_branch(p, "main"), Some(PathBuf::from("/r")));
        assert_eq!(worktree_for_branch(p, "other"), None);
        assert_eq!(worktree_for_branch(p, "feat"), None, "a prefix is not the branch");
    }

    #[test]
    fn a_conflict_entry_gets_a_worktree_on_the_pull_requests_branch_a_brief_and_the_state_fixing() {
        let (_t, _origin, clone) = repos();
        let (forge, home) = world();
        let errors = Errors::at(home.path());
        errors.record(7, "head-old", Class::Conflict, "it conflicts with `main`", "t1").unwrap();
        let plan = prepare(&Git { repo: clone.clone() }, &forge, &errors, home.path(), 7, "t2").unwrap();
        assert!(!plan.reused && plan.worktree.join("b.txt").is_file(), "the worktree is on the feature branch: {plan:?}");
        assert_eq!(plan.entry.state, "fixing");
        assert!(plan.brief.contains("git-merge-and-conflict-resolution") && plan.brief.contains("Never rebase or force-push") && plan.brief.contains("--needs-human") && plan.brief.contains("origin/main"), "{}", plan.brief);
        assert_eq!(std::fs::read_to_string(&plan.brief_path).unwrap(), plan.brief);
        // The worktree really is on the branch.
        let porcelain = C::new("git").arg("-C").arg(&clone).args(["worktree", "list", "--porcelain"]).output().unwrap();
        assert_eq!(worktree_for_branch(&String::from_utf8_lossy(&porcelain.stdout), "feature").map(|p| p.canonicalize().unwrap()), Some(plan.worktree.canonicalize().unwrap()));
    }

    #[test]
    fn a_branch_already_checked_out_somewhere_is_pointed_at_not_created_again() {
        let (_t, _origin, clone) = repos();
        let (forge, home) = world();
        let errors = Errors::at(home.path());
        errors.record(7, "head-old", Class::CiRed, "checks failed: ui", "t1").unwrap();
        let first = prepare(&Git { repo: clone.clone() }, &forge, &errors, home.path(), 7, "t2").unwrap();
        // Preparing again (say the builder lost the brief) reuses the worktree.
        errors.set_state("e1", "open", "", "t3").unwrap();
        let second = prepare(&Git { repo: clone }, &forge, &errors, home.path(), 7, "t4").unwrap();
        // (macOS reports the same directory as /private/var/... and /var/...)
        assert!(second.reused && second.worktree.canonicalize().unwrap() == first.worktree.canonicalize().unwrap(), "{second:?}");
    }

    #[test]
    fn what_a_branch_fix_cannot_solve_is_refused_and_so_is_a_pull_request_that_is_gone() {
        let (_t, _origin, clone) = repos();
        let (forge, home) = world();
        let errors = Errors::at(home.path());
        assert!(prepare(&Git { repo: clone.clone() }, &forge, &errors, home.path(), 7, "t").unwrap_err().contains("no open error entry"));
        errors.record(7, "head-old", Class::Forge, "cannot run `gh`", "t1").unwrap();
        assert!(prepare(&Git { repo: clone.clone() }, &forge, &errors, home.path(), 7, "t").unwrap_err().contains("needs a person"));
        errors.record(7, "head-old", Class::Conflict, "conflict", "t2").unwrap();
        forge.edit(7, |p| p.state = "MERGED".into());
        assert!(prepare(&Git { repo: clone }, &forge, &errors, home.path(), 7, "t").unwrap_err().contains("nothing to fix"));
    }

    #[test]
    fn done_needs_a_new_head_and_needs_human_needs_a_reason() {
        let (forge, home) = world();
        let errors = Errors::at(home.path());
        errors.record(7, "head-old", Class::Conflict, "c", "t1").unwrap();
        assert!(done(&forge, &errors, 7, "t").unwrap_err().contains("fixing"), "not started yet");
        errors.set_state("e1", "fixing", "w", "t2").unwrap();
        assert!(done(&forge, &errors, 7, "t").unwrap_err().contains("still `head-old`"), "nothing was pushed");
        forge.edit(7, |p| p.head_sha = "head-new".into());
        let msg = done(&forge, &errors, 7, "t3").unwrap();
        assert!(msg.contains("fixed-pending-ci") && msg.contains("head-new") && msg.contains("no longer applies"), "{msg}");
        assert_eq!(errors.entries()[0].state, "fixed-pending-ci");
        // The builder stops instead.
        errors.record(8, "h", Class::Conflict, "c", "t4").unwrap();
        assert!(needs_human(&errors, 8, "  ", "t5").unwrap_err().contains("reason"));
        assert!(needs_human(&errors, 8, "touches GOVERNANCE.md", "t5").unwrap().contains("needs-human"));
        let e = errors.entries().into_iter().find(|e| e.pr == 8).unwrap();
        assert_eq!((e.state.as_str(), e.note.as_str()), ("needs-human", "touches GOVERNANCE.md"));
        assert!(open_entry(&errors, 8).is_none(), "a person has it; fix does not offer it again");
    }
}
