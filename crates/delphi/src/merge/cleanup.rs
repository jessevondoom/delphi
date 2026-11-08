//! Cleanup after a merge (spec 094 §3.3): the remote branch, the local branch and the worktrees that belonged to a merged pull
//! request, removed with the safest verbs git has and reported one line at a time.
//!
//! Rules, each with a test:
//! - never the base branch of the pull request, the repository's default branch, `main` or `master`;
//! - never a branch whose pull request is not `MERGED` (read from the forge, not assumed);
//! - the local branch only when `git merge-base --is-ancestor` proves it is in the fetched `origin/<base>`, and then with
//!   `git branch -D` (see `local_step` for why `-d` cannot be used); an unproven branch is never touched;
//! - a worktree is removed only without `--force`, and is kept and reported when it is locked, has uncommitted changes, is the
//!   main worktree, or is (or contains) this process's own directory;
//! - the remote branch is kept and reported when another OPEN pull request uses it as its base, or when its tip is not the
//!   head the pull request merged (a different branch that happens to share the name);
//! - a failure here never changes the outcome of a merge: it is printed, and returned in `Report::problems`.
//!
//! This module is on the gate list (`merge-gates.toml`).

use super::forge::Forge;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository cleanup works in. `cwd` is this process's directory, kept apart from `dir` so tests can say where "here" is.
pub struct Git {
    dir: PathBuf,
    cwd: PathBuf,
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

#[derive(Debug, PartialEq)]
struct Worktree {
    path: PathBuf,
    branch: Option<String>,
    locked: bool,
}

impl Git {
    pub fn here() -> Git {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Git { dir: PathBuf::from("."), cwd }
    }

    #[cfg(test)]
    pub(super) fn at(dir: &Path, cwd: &Path) -> Git {
        Git { dir: dir.to_path_buf(), cwd: cwd.to_path_buf() }
    }

    /// The one place `git` is started: arguments are built here, never taken from a pull request except as a branch name
    /// passed after `--` or as a ref that git itself validates.
    fn run_in(&self, dir: &Path, args: &[&str]) -> Result<Out, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        Ok(Out { code: out.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&out.stdout).to_string(), stderr: String::from_utf8_lossy(&out.stderr).trim().to_string() })
    }

    fn ok(&self, args: &[&str]) -> Result<String, String> {
        let o = self.run_in(&self.dir, args)?;
        if o.code == 0 {
            Ok(o.stdout)
        } else {
            Err(format!("git {} failed: {}", args.first().unwrap_or(&""), if o.stderr.is_empty() { format!("exit code {}", o.code) } else { o.stderr }))
        }
    }

    fn default_branch(&self) -> Option<String> {
        let o = self.ok(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).ok()?;
        o.trim().strip_prefix("origin/").map(String::from)
    }

    fn local_exists(&self, branch: &str) -> bool {
        self.run_in(&self.dir, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).map(|o| o.code == 0).unwrap_or(false)
    }

    /// Branch name to tip, for every branch on `origin`.
    fn remote_heads(&self) -> Result<HashMap<String, String>, String> {
        let out = self.ok(&["ls-remote", "--heads", "origin"])?;
        Ok(out.lines().filter_map(|l| l.split_once('\t')).filter_map(|(sha, r)| Some((r.strip_prefix("refs/heads/")?.to_string(), sha.to_string()))).collect())
    }

    fn worktrees(&self) -> Result<Vec<Worktree>, String> {
        let out = self.ok(&["worktree", "list", "--porcelain"])?;
        let mut list = Vec::new();
        for block in out.split("\n\n") {
            let mut w = Worktree { path: PathBuf::new(), branch: None, locked: false };
            for l in block.lines() {
                if let Some(p) = l.strip_prefix("worktree ") {
                    w.path = PathBuf::from(p);
                } else if let Some(b) = l.strip_prefix("branch refs/heads/") {
                    w.branch = Some(b.to_string());
                } else if l == "locked" || l.starts_with("locked ") {
                    w.locked = true;
                }
            }
            if !w.path.as_os_str().is_empty() {
                list.push(w);
            }
        }
        Ok(list)
    }

    /// Why this worktree must stay, if it must.
    fn keep_reason(&self, w: &Worktree, is_main: bool) -> Option<String> {
        if is_main {
            return Some("it is the main worktree".into());
        }
        if w.locked {
            return Some("it is locked".into());
        }
        let here = self.cwd.canonicalize().unwrap_or_else(|_| self.cwd.clone());
        let there = w.path.canonicalize().unwrap_or_else(|_| w.path.clone());
        if here.starts_with(&there) {
            return Some("it is (or contains) the directory this command runs in".into());
        }
        match self.run_in(&w.path, &["status", "--porcelain"]) {
            Ok(o) if o.code == 0 && o.stdout.trim().is_empty() => None,
            Ok(o) if o.code == 0 => Some("it has uncommitted changes".into()),
            Ok(o) => Some(format!("its state could not be read ({})", o.stderr)),
            Err(e) => Some(e),
        }
    }
}

/// What a cleanup did: every line it printed, and the failures among them.
#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    /// Real failures (a command that errored), for the error log of slice S5. A deliberate "kept" is only a line.
    pub problems: Vec<String>,
}

impl Report {
    fn say(&mut self, s: String) {
        println!("{s}");
        self.lines.push(s);
    }
    fn fail(&mut self, s: String) {
        eprintln!("{s}");
        self.lines.push(s.clone());
        self.problems.push(s);
    }
}

/// Branches that are never deleted whatever a pull request says.
fn protected(git: &Git, branch: &str, base: &str) -> bool {
    branch == base || matches!(branch, "main" | "master") || git.default_branch().as_deref() == Some(branch)
}

/// Clean up after pull request `pr`. With `dry` nothing is changed (only remote-tracking refs are fetched, to judge "merged").
pub fn cleanup_pr(git: &Git, forge: &dyn Forge, pr: u64, dry: bool) -> Report {
    let mut r = Report::default();
    let info = match forge.pr(pr) {
        Ok(i) => i,
        Err(e) => {
            r.fail(format!("#{pr}: cleanup skipped, cannot read the pull request: {e}"));
            return r;
        }
    };
    if info.state != "MERGED" {
        r.say(format!("#{pr}: nothing removed, the pull request is {}, not merged", info.state));
        return r;
    }
    let (branch, base) = (info.head_ref.clone(), info.base_ref.clone());
    if branch.is_empty() || branch.starts_with('-') || protected(git, &branch, &base) {
        r.say(format!("#{pr}: nothing removed, `{branch}` is the base or default branch (or not a branch name)"));
        return r;
    }
    if let Err(e) = git.ok(&["fetch", "--quiet", "origin"]) {
        r.fail(format!("#{pr}: cleanup skipped, cannot update from origin: {e}"));
        return r;
    }
    remote_step(git, forge, &info.head_sha, &branch, pr, dry, &mut r);
    let held = worktree_step(git, &branch, pr, dry, &mut r);
    local_step(git, &branch, &base, held, pr, dry, &mut r);
    r
}

fn remote_step(git: &Git, forge: &dyn Forge, head: &str, branch: &str, pr: u64, dry: bool, r: &mut Report) {
    let heads = match git.remote_heads() {
        Ok(h) => h,
        Err(e) => return r.fail(format!("#{pr}: remote branch `{branch}` not touched: {e}")),
    };
    let Some(tip) = heads.get(branch) else { return };
    if tip != head {
        return r.say(format!("#{pr}: kept remote branch `{branch}`: its tip is not the commit that was merged"));
    }
    match forge.open_bases() {
        Err(e) => return r.fail(format!("#{pr}: kept remote branch `{branch}`: cannot tell whether an open pull request is based on it: {e}")),
        Ok(bases) if bases.iter().any(|b| b == branch) => return r.say(format!("#{pr}: kept remote branch `{branch}`: an open pull request uses it as its base")),
        Ok(_) => {}
    }
    if dry {
        return r.say(format!("#{pr}: would remove remote branch `{branch}`"));
    }
    match git.ok(&["push", "origin", "--delete", branch]) {
        Ok(_) => r.say(format!("#{pr}: removed remote branch `{branch}`")),
        Err(e) => r.fail(format!("#{pr}: remote branch `{branch}` not removed: {e}")),
    }
}

/// Remove the worktrees that have `branch` checked out. Returns true when one stays and so still holds the branch.
fn worktree_step(git: &Git, branch: &str, pr: u64, dry: bool, r: &mut Report) -> bool {
    let list = match git.worktrees() {
        Ok(l) => l,
        Err(e) => {
            r.fail(format!("#{pr}: worktrees not touched: {e}"));
            return true;
        }
    };
    let (mut held, mut removed_any) = (false, false);
    for (i, w) in list.iter().enumerate() {
        if w.branch.as_deref() != Some(branch) {
            continue;
        }
        if let Some(why) = git.keep_reason(w, i == 0) {
            held = true;
            r.say(format!("#{pr}: kept worktree {}: {why}", w.path.display()));
            continue;
        }
        if dry {
            r.say(format!("#{pr}: would remove worktree {}", w.path.display()));
            continue;
        }
        let p = w.path.to_string_lossy().to_string();
        match git.ok(&["worktree", "remove", &p]) {
            Ok(_) => {
                removed_any = true;
                r.say(format!("#{pr}: removed worktree {p}"));
            }
            Err(e) => {
                held = true;
                r.fail(format!("#{pr}: worktree {p} not removed: {e}"));
            }
        }
    }
    if removed_any {
        if let Err(e) = git.ok(&["worktree", "prune"]) {
            r.fail(format!("#{pr}: worktree prune failed: {e}"));
        }
    }
    held
}

fn local_step(git: &Git, branch: &str, base: &str, held: bool, pr: u64, dry: bool, r: &mut Report) {
    if !git.local_exists(branch) {
        return;
    }
    if held {
        return r.say(format!("#{pr}: kept local branch `{branch}`: a worktree that stays has it checked out"));
    }
    let target = format!("origin/{base}");
    match git.run_in(&git.dir, &["merge-base", "--is-ancestor", &format!("refs/heads/{branch}"), &format!("refs/remotes/{target}")]) {
        Ok(o) if o.code == 0 => {}
        Ok(o) if o.code == 1 => return r.say(format!("#{pr}: kept local branch `{branch}`: it has commits that are not in {target}")),
        Ok(o) => return r.fail(format!("#{pr}: kept local branch `{branch}`: cannot compare it with {target}: {}", o.stderr)),
        Err(e) => return r.fail(format!("#{pr}: kept local branch `{branch}`: {e}")),
    }
    if dry {
        return r.say(format!("#{pr}: would remove local branch `{branch}`"));
    }
    // `-D`, but only here, after the ancestry check above has proved every commit of the branch is in the fetched `origin/<base>`.
    // `-d` is not the safer verb: it judges against the local base or the branch's upstream, and a pull request merged on the
    // forge leaves both behind (the local base is stale until pulled, the upstream was just deleted), so it refuses a branch
    // whose commits are all safe. Nothing can be lost: they are reachable from `origin/<base>`.
    match git.ok(&["branch", "-D", "--", branch]) {
        Ok(_) => r.say(format!("#{pr}: removed local branch `{branch}`")),
        Err(e) => r.fail(format!("#{pr}: kept local branch `{branch}`: {e}")),
    }
}

/// The backlog: recent merged pull requests whose branch still exists locally or on `origin`.
pub fn sweep_plan(git: &Git, forge: &dyn Forge, limit: usize) -> Result<Vec<u64>, String> {
    let merged = forge.merged_branches(limit).map_err(|e| e.to_string())?;
    let remote = git.remote_heads()?;
    Ok(merged.into_iter().filter(|(_, b)| !b.is_empty() && (git.local_exists(b) || remote.contains_key(b))).map(|(n, _)| n).collect())
}

/// `delphi merge cleanup`. Returns the exit code.
pub fn run(forge: &dyn Forge, prs: &[u64], merged: bool, dry: bool, yes: bool) -> i32 {
    if prs.is_empty() && !merged {
        eprintln!("delphi merge cleanup: usage: delphi merge cleanup <PR>... | --merged [--dry-run] [--yes]");
        return 2;
    }
    if !dry && crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi merge cleanup: refused; this process was started by an agent seat");
        return 2;
    }
    let git = Git::here();
    let mut numbers: Vec<u64> = prs.to_vec();
    // A sweep changes nothing until one explicit --yes: without it, this prints the plan and refuses.
    let refuse = merged && !dry && !yes;
    if merged {
        match sweep_plan(&git, forge, 100) {
            Ok(list) => numbers.extend(list.into_iter().filter(|n| !prs.contains(n))),
            Err(e) => {
                eprintln!("delphi merge cleanup: {e}");
                return 4;
            }
        }
    }
    if numbers.is_empty() {
        println!("nothing to clean up");
    }
    let mut problems = 0;
    for n in &numbers {
        problems += cleanup_pr(&git, forge, *n, dry || refuse).problems.len();
    }
    if refuse {
        eprintln!("delphi merge cleanup: --merged only prints the plan until you add --yes; nothing was changed");
        return 2;
    }
    if problems > 0 {
        1
    } else {
        0
    }
}
