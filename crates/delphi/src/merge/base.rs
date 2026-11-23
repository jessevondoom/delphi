//! The check of the base after each merge, and `delphi merge repair-base` (spec 094 §11.3.4).
//!
//! - **The check.** After a merge, at each `merge status` and at each `--auto` poll, the flow fetches the base and runs the spec
//!   compile's check (`delphi specs compile --check`, in process) on its tip, in a temporary worktree of its own under the Delphi
//!   home that is removed at the end. No build. A drift is an error entry of class `base-drift` (pull request 0, the base
//!   commit), and `--all` and `--auto` merge nothing more until a check finds the base current again, which resolves the entry.
//!   A check that could not run (no repository, a fetch that failed) is said and never taken for drift or for current.
//! - **`repair-base`.** From the base tip, in its own temporary worktree: the spec compile, then a commit of the compiled spec
//!   tree only (anything else that changed is refused, not committed), a plain push of a new branch, and a pull request ("Recompile
//!   ... on main"), as #806 was by hand. It pushes and opens a pull request only after the operator confirms (`tty`, or a quote
//!   that asks for a repair). It never merges: it has no path to `Forge::merge`; the operator merges the repair as any other.

use super::approve::{attest, Target, Terminal};
use super::errors::{log_central, Class, Errors};
use super::forge::Forge;
use super::record::Ledger;
use super::refresh::{same_compiler, Runner};
use super::short;
use crate::spec_compile::{self, GROUPS_FILE, TREE_DIR};
use serde_json::json;
use std::path::Path;

/// The pull request number of a `base-drift` entry: it belongs to the base, not to a pull request.
pub const BASE_ENTRY: u64 = 0;
/// The spec whose output a repair regenerates; its trailer goes on the repair commit, as on #806's.
pub const COMPILE_SPEC: &str = "specs/088-spec-compile-and-current-set.md";

#[derive(Debug, Clone, PartialEq)]
pub enum BaseState {
    /// The compile writes exactly what is committed, or the repository has no spec groups file (nothing to compile).
    Current { sha: String },
    Drift { sha: String, detail: String },
    /// The check could not run.
    Unchecked(String),
}

/// One check of one base. The real one is `check_base` in this repository; tests use a fake.
pub trait BaseCheck {
    fn check(&self, base_ref: &str) -> BaseState;
}

/// `check_base`, in the repository the command was started in.
pub struct InRepo<'a> {
    pub git: Runner,
    pub home: &'a Path,
}

impl BaseCheck for InRepo<'_> {
    fn check(&self, base_ref: &str) -> BaseState {
        check_base(&self.git, self.home, base_ref)
    }
}

/// A branch name that may go on a git command line and into a branch name of ours.
fn plain_ref(r: &str) -> bool {
    !r.is_empty() && r.len() <= 200 && !r.starts_with('-') && !r.contains("..") && r.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

/// Fetch `base_ref`, make a detached worktree at its tip under `<home>/merge/<name>`, run `f` there, and remove the worktree
/// whatever happened (the directory, then git's record of it; no `--force` needed for either).
fn at_tip<R>(g: &Runner, home: &Path, name: &str, base_ref: &str, f: impl FnOnce(&Path, &str) -> R) -> Result<R, String> {
    if !plain_ref(base_ref) {
        return Err(format!("`{base_ref}` is not a plain branch name"));
    }
    let repo = g.repo.as_path();
    g.git(repo, &["fetch", "-q", "origin", base_ref]).map_err(|e| format!("could not fetch `{base_ref}`: {e}"))?;
    let sha = g.git(repo, &["rev-parse", "--verify", "-q", &format!("refs/remotes/origin/{base_ref}")]).map_err(|e| format!("no `origin/{base_ref}`: {e}"))?.trim().to_string();
    let dir = home.join("merge").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = g.git(repo, &["worktree", "prune"]);
    std::fs::create_dir_all(dir.parent().expect("has a parent")).map_err(|e| e.to_string())?;
    g.git(repo, &["worktree", "add", "-q", "--detach", &dir.to_string_lossy(), &sha]).map_err(|e| format!("could not make a worktree: {e}"))?;
    let out = f(&dir, &sha);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = g.git(repo, &["worktree", "prune"]);
    Ok(out)
}

/// Is the compiled spec tree on the tip of `base_ref` what the compile writes?
pub fn check_base(g: &Runner, home: &Path, base_ref: &str) -> BaseState {
    let checked = at_tip(g, home, "base-check", base_ref, |dir, sha| {
        let sha = sha.to_string();
        if !dir.join(GROUPS_FILE).exists() {
            return BaseState::Current { sha };
        }
        // A binary whose compiler is not the base's would report drift that is not there (§16.3): unchecked, never drift.
        if let Err(why) = same_compiler(dir, &format!("`{base_ref}` at {}", short(&sha))) {
            return BaseState::Unchecked(why);
        }
        match spec_compile::compile_in(dir, true) {
            Ok(_) => BaseState::Current { sha },
            // 1: a stale file, or a spec or the groups that fail to compile; either way the base is not what it should be.
            Err((1, lines)) => BaseState::Drift { sha, detail: lines.join("; ") },
            Err((_, lines)) => BaseState::Unchecked(lines.join("; ")),
        }
    });
    checked.unwrap_or_else(BaseState::Unchecked)
}

/// Check `base_ref` and keep the books: a drift is a `base-drift` entry (the same base commit seen again is one more attempt);
/// a base that is current again resolves the open entries for it. Returns the state and one line to print.
pub fn check_and_note(errors: &Errors, check: &dyn BaseCheck, base_ref: &str, now: &str) -> (BaseState, String) {
    let state = check.check(base_ref);
    let line = match &state {
        BaseState::Current { sha } => {
            set_drift(errors, base_ref, "resolved", &format!("`{base_ref}` is current at {}", short(sha)), now);
            format!("base `{base_ref}` ({}): {TREE_DIR} is current", short(sha))
        }
        BaseState::Drift { sha, detail } => {
            let text = format!("`{base_ref}` at {}: {TREE_DIR} is not what the spec compile writes: {detail}", short(sha));
            match errors.record(BASE_ENTRY, sha, Class::BaseDrift, &text, now) {
                Ok(e) if e.attempts == 1 => log_central(BASE_ENTRY, Class::BaseDrift, 1),
                Ok(_) => {}
                Err(e) => eprintln!("delphi merge: could not write the error log: {e}"),
            }
            format!("base `{base_ref}` ({}) has DRIFTED: {TREE_DIR} is not what the spec compile writes, so --all and --auto merge nothing more until it is repaired: run `delphi merge repair-base`, then merge its pull request once it is green", short(sha))
        }
        BaseState::Unchecked(why) => format!("base `{base_ref}`: could not check it ({why})"),
    };
    (state, line)
}

/// What `repair-base` did.
#[derive(Debug, Clone, PartialEq)]
pub enum Repaired {
    /// The compile changes nothing: there is nothing to repair.
    Current { sha: String },
    /// `--dry-run`: these files would be in the repair; nothing was pushed.
    WouldOpen { sha: String, paths: Vec<String> },
    Opened { pr: u64, branch: String, head: String, sha: String, paths: Vec<String> },
}

/// Everything a repair needs besides the base.
pub struct RepairEnv<'a> {
    pub forge: &'a dyn Forge,
    pub errors: &'a Errors,
    pub ledger: &'a Ledger,
    pub home: &'a Path,
    pub git: &'a Runner,
    pub now: String,
}

/// `delphi merge repair-base`: open a pull request that makes the base's compiled spec tree current. Never merges anything.
pub fn repair_base(env: &RepairEnv, base_ref: &str, dry_run: bool, term: &mut dyn Terminal, quote: Option<&str>) -> Result<Repaired, String> {
    let done = at_tip(env.git, env.home, "repair-base", base_ref, |dir, sha| repair_in(env, base_ref, dir, sha, dry_run, term, quote))??;
    match &done {
        Repaired::Current { sha } => set_drift(env.errors, base_ref, "resolved", &format!("`{base_ref}` is current at {}", short(sha)), &env.now),
        Repaired::Opened { pr, .. } => set_drift(env.errors, base_ref, "fixed-pending-ci", &format!("repair pull request #{pr}"), &env.now),
        Repaired::WouldOpen { .. } => {}
    }
    Ok(done)
}

/// Move the open `base-drift` entries of `base_ref` to `state`.
fn set_drift(errors: &Errors, base_ref: &str, state: &str, note: &str, now: &str) {
    let mark = format!("`{base_ref}` at ");
    for e in errors.entries().into_iter().filter(|e| e.open() && e.class == Class::BaseDrift.slug() && e.detail.starts_with(&mark)) {
        let _ = errors.set_state(&e.id, state, note, now);
    }
}

fn repair_in(env: &RepairEnv, base_ref: &str, dir: &Path, sha: &str, dry_run: bool, term: &mut dyn Terminal, quote: Option<&str>) -> Result<Repaired, String> {
    let g = env.git;
    if !dir.join(GROUPS_FILE).exists() {
        return Err(format!("`{base_ref}` has no {GROUPS_FILE}, so there is nothing to recompile"));
    }
    same_compiler(dir, &format!("`{base_ref}` at {}", short(sha)))?;
    spec_compile::compile_in(dir, false).map_err(|(_, l)| format!("the spec compile fails on `{base_ref}`, which a recompile cannot repair; hand it to a builder: {}", l.join("; ")))?;
    g.git(dir, &["add", "-A", "--", TREE_DIR])?;
    let under_tree = |p: &str| p.strip_prefix(TREE_DIR).is_some_and(|rest| rest.starts_with('/'));
    let status = g.git(dir, &["-c", "core.quotePath=false", "status", "--porcelain", "--untracked-files=all"])?;
    let stray: Vec<&str> = status.lines().filter_map(|l| l.get(3..)).filter(|p| !under_tree(p)).collect();
    if !stray.is_empty() {
        return Err(format!("the compile changed files outside {TREE_DIR} ({}); nothing was committed", stray.join(", ")));
    }
    let paths: Vec<String> = g.git(dir, &["-c", "core.quotePath=false", "diff", "--cached", "--name-only"])?.lines().map(String::from).collect();
    let sha = sha.to_string();
    if paths.is_empty() {
        return Ok(Repaired::Current { sha });
    }
    if dry_run {
        return Ok(Repaired::WouldOpen { sha, paths });
    }
    let branch = format!("repair-base-{}-{}", base_ref.replace('/', "-"), short(&sha));
    let title = format!("Recompile {TREE_DIR} on {base_ref}");
    term.show(&format!("`{base_ref}` ({}) has drifted: the spec compile changes {} file(s):\n  {}\nThis commits only those, pushes the new branch `{branch}` and opens the pull request \"{title}\". It merges nothing: you merge it once CI is green.", short(&sha), paths.len(), paths.join("\n  ")));
    let att = attest(term, quote, Target::Repair, false)?;
    let message = format!("{title}\n\nThe spec compile on `{base_ref}` at {sha} writes other files than were committed (drift that entered by a merge, spec 094 §11.3.4). This commit is only what `delphi specs compile` writes, made by `delphi merge repair-base`.\n\nImplements-Spec: {COMPILE_SPEC}\n");
    g.git(dir, &["commit", "-q", "-m", &message]).map_err(|e| format!("could not commit: {e}"))?;
    let head = g.git(dir, &["rev-parse", "HEAD"])?.trim().to_string();
    // A plain push of a new branch: one that exists already rejects it, and nothing is overwritten.
    g.git(dir, &["push", "-q", "origin", &format!("HEAD:refs/heads/{branch}")]).map_err(|e| format!("the push of `{branch}` was rejected (does a repair branch for this tip exist already?): {e}"))?;
    let body = format!(
        "The compiled spec tree on `{base_ref}` (`{sha}`) is not what `delphi specs compile` writes. This pull request is that compile and nothing else ({} file(s)):\n\n{}\n\nOpened by `delphi merge repair-base` (spec 094 §11.3.4), which never merges: merge it once CI is green, as any other pull request.",
        paths.len(),
        paths.iter().map(|p| format!("- `{p}`")).collect::<Vec<_>>().join("\n")
    );
    let pr = env.forge.open_pr(base_ref, &branch, &title, &body).map_err(|e| format!("pushed `{branch}`, but the pull request could not be opened ({e}); open it by hand"))?;
    let row = json!({"kind": "repair_base", "base": base_ref, "base_sha": sha, "branch": branch, "head_sha": head, "pr": pr, "paths": paths, "attestation": att.label(), "quote": att.quote(), "delphi_version": env!("CARGO_PKG_VERSION"), "time": env.now});
    if let Err(e) = env.ledger.append(&row) {
        eprintln!("delphi merge repair-base: #{pr} was opened, but the ledger row could not be written: {e}");
    }
    Ok(Repaired::Opened { pr, branch, head, sha, paths })
}
