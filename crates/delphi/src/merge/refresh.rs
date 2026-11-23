//! `delphi merge refresh <PR>` (spec 094 §11.3.2): bring a pull request that is behind its base up to date, by a merge of the
//! base into its branch, settling only the conflicts this repository has seen and knows how to settle by rule.
//!
//! - It refuses at once a pull request that is not open, a draft, a fork's, gate-changing, or already current.
//! - It works in a temporary worktree of its own under the Delphi home, on the pull request's head commit, and removes it at
//!   the end whatever happened.
//! - It merges `origin/<base>` in with `git merge --no-ff --no-commit` (a merge commit; never a rebase).
//! - It settles only the closed list in `rule_for`: the groups file and the spec tracker by a union (the base's lines first),
//!   and the compiled spec tree by taking the base's side. Any other conflict, a conflict on a gate path, or more than ten
//!   conflicted files aborts the merge, pushes nothing, writes a `refresh-refused` entry and hands over to `delphi merge fix`.
//! - It always recompiles the specs in process (the code path of `delphi specs compile`, spec 088) and checks the result, so a
//!   view that merged with no conflict and is still wrong (the count line of 2026-11-20 was one) is regenerated.
//! - It checks that it widened nothing (§11.6, invariant 3), commits with a fixed message, and pushes plainly: never a force of
//!   any kind, so a branch that moved meanwhile rejects the push and nothing is overwritten.
//!
//! It never merges a pull request (it has no path to `Forge::merge`) and needs no approval of its own: it pushes one ordinary
//! commit that changes nothing the pull request did not already change, apart from generated files. This module is on the gate
//! list, so the closed list of settlements cannot change without a person at a terminal.

use super::check::evaluate;
use super::errors::{Class, Errors};
use super::fix::Git;
use super::forge::Forge;
use super::gates::{self, GATES_FILE};
use super::record::Ledger;
use crate::spec_compile;
use serde_json::json;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The append-only spec tracker, settled by a union like the groups file.
pub const TRACKER: &str = "missions/2026-10-23-foundational-design/spec-tracker.md";
/// The words every hand-over line carries; `errors::classify_refusal` finds the class `refresh-refused` by them.
pub const HANDED_OVER: &str = "not refreshed, and nothing was pushed";
/// More conflicted files than this is not a mechanical update; it goes to `merge fix`.
pub const MAX_CONFLICTS: usize = 10;
/// The only directory whose changed paths may differ after a refresh (the compile writes there; the groups file lives there).
pub(super) const GENERATED_ROOT: &str = "compiled/";

/// How one conflicted path is settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Both sides' lines of each conflicted hunk, the base's first, each line kept once.
    UnionBaseFirst,
    /// The base's side; the compile regenerates it.
    TakeBase,
}

/// The closed list (§11.3.2 step 4). Anything not named here is not settled by this command.
pub fn rule_for(path: &str) -> Option<Rule> {
    if path == spec_compile::GROUPS_FILE || path == TRACKER {
        Some(Rule::UnionBaseFirst)
    } else if path.strip_prefix(spec_compile::TREE_DIR).is_some_and(|rest| rest.starts_with('/')) {
        Some(Rule::TakeBase)
    } else {
        None
    }
}

/// What to do with each conflicted path, or why this is not a mechanical update (the paths are named).
pub fn settle_plan(conflicts: &[String], gate_patterns: &[String]) -> Result<Vec<(String, Rule)>, String> {
    if conflicts.len() > MAX_CONFLICTS {
        return Err(format!("{} files conflict, more than {MAX_CONFLICTS}: {}", conflicts.len(), conflicts.join(", ")));
    }
    let gate = gates::hits(gate_patterns, conflicts);
    if !gate.is_empty() {
        return Err(format!("a gate path conflicts: {}", gate.join(", ")));
    }
    let other: Vec<&String> = conflicts.iter().filter(|p| rule_for(p).is_none()).collect();
    if !other.is_empty() {
        return Err(format!("a conflict outside the files it settles by rule: {}", other.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
    }
    Ok(conflicts.iter().map(|p| (p.clone(), rule_for(p).expect("checked"))).collect())
}

/// Settle every conflicted hunk of `text` (git's `merge` conflict style, the pull request's side first, the base's second) by
/// keeping the base's lines, then the pull request's lines that are not among them. Refuses a hunk it cannot read.
pub fn union_base_first(text: &str) -> Result<String, String> {
    union(text, true)
}

/// The same union with the checked-out side ("ours") first: a rollup (spec 094 §13.2) has the base and the earlier constituents
/// checked out and merges the next constituent in, so the earlier lines come first.
pub fn union_ours_first(text: &str) -> Result<String, String> {
    union(text, false)
}

fn union(text: &str, theirs_first: bool) -> Result<String, String> {
    enum At {
        Outside,
        Ours,
        Theirs,
    }
    let key = |l: &str| l.trim_end_matches(['\n', '\r']).to_string();
    let mut out = String::new();
    let (mut at, mut ours, mut theirs) = (At::Outside, Vec::<String>::new(), Vec::<String>::new());
    for line in text.split_inclusive('\n') {
        let line = if line.ends_with('\n') { line.to_string() } else { format!("{line}\n") };
        match at {
            At::Outside if line.starts_with("<<<<<<<") => at = At::Ours,
            At::Outside if line.starts_with("=======") || line.starts_with(">>>>>>>") || line.starts_with("|||||||") => return Err("a conflict marker outside a conflict".into()),
            At::Outside => out.push_str(&line),
            At::Ours if line.starts_with("=======") => at = At::Theirs,
            At::Ours if line.starts_with("|||||||") || line.starts_with("<<<<<<<") || line.starts_with(">>>>>>>") => return Err("a conflict in a style this command does not read".into()),
            At::Ours => ours.push(line),
            At::Theirs if line.starts_with(">>>>>>>") => {
                let mut seen: Vec<String> = Vec::new();
                let (first, second) = if theirs_first { (&mut theirs, &mut ours) } else { (&mut ours, &mut theirs) };
                for l in first.drain(..).chain(second.drain(..)) {
                    if !seen.contains(&key(&l)) {
                        seen.push(key(&l));
                        out.push_str(&l);
                    }
                }
                at = At::Outside;
            }
            At::Theirs if line.starts_with("<<<<<<<") || line.starts_with("=======") || line.starts_with("|||||||") => return Err("a conflict in a style this command does not read".into()),
            At::Theirs => theirs.push(line),
        }
    }
    if !matches!(at, At::Outside) {
        return Err("a conflict that does not end".into());
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    Ok(out)
}

/// Per changed path, the added and the removed lines of a `git diff -U0`, each sorted (compared as multisets).
pub fn changes(diff: &str) -> BTreeMap<String, (Vec<String>, Vec<String>)> {
    let mut out: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
    let (mut path, mut header) = (String::new(), false);
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            path = rest.rsplit_once(" b/").map(|(_, b)| b.to_string()).unwrap_or_else(|| rest.to_string());
            header = true;
            out.entry(path.clone()).or_default();
        } else if header && line.starts_with("@@") {
            header = false;
        } else if header {
            if let Some(p) = line.strip_prefix("+++ b/").or_else(|| line.strip_prefix("--- a/")) {
                if p != path {
                    let v = out.remove(&path).unwrap_or_default();
                    path = p.to_string();
                    out.insert(path.clone(), v);
                }
            }
        } else if let Some(l) = line.strip_prefix('+') {
            out.entry(path.clone()).or_default().0.push(l.to_string());
        } else if let Some(l) = line.strip_prefix('-') {
            out.entry(path.clone()).or_default().1.push(l.to_string());
        }
    }
    for (a, r) in out.values_mut() {
        a.sort();
        r.sort();
    }
    out
}

/// Invariant 3 (§11.6): the pull request's change against the new base equals its change against the old one, except under the
/// generated directory. Returns what widened, if anything.
pub fn widened(before: &str, after: &str) -> Vec<String> {
    let (b, a) = (changes(before), changes(after));
    let outside = |m: &BTreeMap<String, (Vec<String>, Vec<String>)>| m.iter().filter(|(p, _)| !p.starts_with(GENERATED_ROOT)).map(|(p, v)| (p.clone(), v.clone())).collect::<BTreeMap<_, _>>();
    let (b, a) = (outside(&b), outside(&a));
    let mut out = Vec::new();
    for p in b.keys().chain(a.keys()).collect::<std::collections::BTreeSet<_>>() {
        match (b.get(p), a.get(p)) {
            (Some(x), Some(y)) if x == y => {}
            (Some(_), Some(_)) => out.push(format!("{p} (its added or removed lines changed)")),
            (Some(_), None) => out.push(format!("{p} (no longer changed)")),
            (None, _) => out.push(format!("{p} (newly changed)")),
        }
    }
    out
}

/// The spec compile, in process, then its check. Skipped (`Ok(false)`) in a repository with no groups file.
pub(super) fn recompile(wt: &Path) -> Result<bool, String> {
    let text = match std::fs::read_to_string(wt.join(spec_compile::GROUPS_FILE)) {
        Ok(t) => t,
        Err(_) => return Ok(false),
    };
    let groups = spec_compile::parse_groups(&text)?;
    let sources = spec_compile::read_sources(wt)?;
    let out = spec_compile::build(&sources, &groups).map_err(|e| e.join("; "))?;
    spec_compile::write_all(wt, &out)?;
    let diffs = spec_compile::check(wt, &groups, &out);
    if diffs.is_empty() {
        Ok(true)
    } else {
        Err(format!("the compiled files are not current after the compile: {}", diffs.join("; ")))
    }
}

/// Runs git and keeps a log of every argument list, so a test can show which verbs were used (§11.10 AC-10 g).
pub struct Runner {
    pub repo: PathBuf,
    pub log: RefCell<Vec<String>>,
}

impl Runner {
    pub fn new(repo: &Path) -> Runner {
        Runner { repo: repo.to_path_buf(), log: RefCell::new(Vec::new()) }
    }
    pub(super) fn git(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        self.log.borrow_mut().push(args.join(" "));
        Git { repo: dir.to_path_buf() }.run(args)
    }
}

/// Everything a refresh needs besides the pull request number.
pub struct Env<'a> {
    pub forge: &'a dyn Forge,
    pub errors: &'a Errors,
    pub ledger: &'a Ledger,
    pub home: &'a Path,
    pub git: &'a Runner,
    pub now: String,
    /// Called just before the push (tests move the branch here).
    pub before_push: Option<&'a dyn Fn()>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub pr: u64,
    pub old_head: String,
    pub new_head: String,
    pub base_sha: String,
    pub settled: Vec<String>,
    pub regenerated: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Done {
    /// Not behind: nothing to do.
    Current,
    Refreshed(Report),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stop {
    /// Refused before any work (not open, a draft, a fork, gate-changing, or the forge could not be read).
    Refused(String),
    /// Work started and was undone: not a mechanical update. A `refresh-refused` entry was written; `merge fix` takes it.
    HandOver(String),
}

impl Stop {
    pub fn line(&self, n: u64) -> String {
        match self {
            Stop::Refused(why) => format!("#{n}: not refreshed: {why}"),
            Stop::HandOver(why) => format!("#{n}: {HANDED_OVER}: {why}. This is not a mechanical update; hand it to the builder with `delphi merge fix {n}`"),
        }
    }
}

/// Refresh pull request `n`. Never merges anything.
pub fn refresh(env: &Env, n: u64) -> Result<Done, Stop> {
    let v = evaluate(env.forge, n).map_err(|e| Stop::Refused(e.to_string()))?;
    let pr = &v.pr;
    if pr.state != "OPEN" {
        return Err(Stop::Refused(format!("it is {}, not open", pr.state.to_lowercase())));
    }
    if pr.is_draft {
        return Err(Stop::Refused("it is a draft".into()));
    }
    if pr.cross_repo {
        return Err(Stop::Refused("it comes from a fork, and this command pushes only to this repository's branches".into()));
    }
    if v.gate_changing() {
        return Err(Stop::Refused(format!("it changes a gate ({}); a person updates it (`gh pr update-branch {n}`) or hands it to `delphi merge fix {n}`", v.gate_hits.join(", "))));
    }
    if v.behind_by == 0 {
        return Ok(Done::Current);
    }
    let dir = env.home.join("merge").join("refresh").join(format!("pr-{n}"));
    let result = work(env, &v, &dir);
    // Whatever happened, the worktree goes: the directory, then git's record of it (no `--force` needed for either).
    let _ = std::fs::remove_dir_all(&dir);
    let _ = env.git.git(&env.git.repo, &["worktree", "prune"]);
    if let Err(Stop::HandOver(detail)) = &result {
        match env.errors.record(n, &pr.head_sha, Class::RefreshRefused, detail, &env.now) {
            Ok(e) if e.attempts == 1 => super::errors::log_central(n, Class::RefreshRefused, e.attempts),
            Ok(_) => {}
            Err(e) => eprintln!("delphi merge refresh: could not write the error log: {e}"),
        }
    }
    let report = result?;
    let row = json!({
        "kind": "refreshed", "pr": n, "old_head": report.old_head, "new_head": report.new_head, "base_sha": report.base_sha,
        "settled": report.settled, "regenerated": report.regenerated, "delphi_version": env!("CARGO_PKG_VERSION"), "time": env.now,
    });
    if let Err(e) = env.ledger.append(&row) {
        eprintln!("delphi merge refresh: #{n} was pushed, but the ledger row could not be written: {e}");
    }
    if let Err(e) = env.forge.comment(n, &comment(&report, &pr.base_ref)) {
        eprintln!("delphi merge refresh: #{n} was pushed, but the comment could not be posted: {e}");
    }
    for e in env.errors.entries().into_iter().filter(|e| e.pr == n && e.open() && e.class == Class::Behind.slug()) {
        let _ = env.errors.set_state(&e.id, "fixed-pending-ci", &format!("refreshed to {}", report.new_head), &env.now);
    }
    Ok(Done::Refreshed(report))
}

fn comment(r: &Report, base: &str) -> String {
    let list = |v: &[String]| if v.is_empty() { "none".to_string() } else { v.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ") };
    format!(
        "Refreshed by `delphi merge refresh`: merged `{base}` (`{}`) into this branch. New head `{}` (was `{}`).\n\nSettled by rule: {}. Regenerated by the spec compile: {}.\n\nNothing outside those files changed. CI runs again on the new head; an approval given for the old head does not carry over.",
        r.base_sha,
        r.new_head,
        r.old_head,
        list(&r.settled),
        list(&r.regenerated)
    )
}

/// The refresh proper, in `dir`. A `HandOver` here has already aborted the merge; the caller removes the worktree.
fn work(env: &Env, v: &super::check::Verdict, dir: &Path) -> Result<Report, Stop> {
    let pr = &v.pr;
    let (repo, g) = (env.git.repo.as_path(), env.git);
    let refused = |e: String| Stop::Refused(e);
    g.git(repo, &["fetch", "-q", "origin", &pr.base_ref, &pr.head_ref]).map_err(|e| refused(format!("could not fetch: {e}")))?;
    let tip = g.git(repo, &["rev-parse", "--verify", "-q", &format!("refs/remotes/origin/{}", pr.head_ref)]).map_err(refused)?;
    if tip.trim() != pr.head_sha {
        return Err(Stop::Refused(format!("the branch `{}` is at `{}`, not the head GitHub reported (`{}`); run this again once it settles", pr.head_ref, tip.trim(), pr.head_sha)));
    }
    let base = format!("origin/{}", pr.base_ref);
    let base_sha = g.git(repo, &["rev-parse", "--verify", "-q", &base]).map_err(refused)?.trim().to_string();
    // A directory left by an interrupted run is this command's own; clear it.
    let _ = std::fs::remove_dir_all(dir);
    let _ = g.git(repo, &["worktree", "prune"]);
    std::fs::create_dir_all(dir.parent().expect("has a parent")).map_err(|e| refused(e.to_string()))?;
    let path = dir.to_string_lossy().to_string();
    g.git(repo, &["worktree", "add", "-q", "--detach", &path, &pr.head_sha]).map_err(|e| refused(format!("could not make a worktree: {e}")))?;

    let hand_over = |why: String| {
        let _ = g.git(dir, &["merge", "--abort"]);
        Stop::HandOver(why)
    };
    let merged_clean = g.git(dir, &["-c", "merge.conflictStyle=merge", "merge", "-q", "--no-ff", "--no-commit", &base]).is_ok();
    let conflicts: Vec<String> = g.git(dir, &["-c", "core.quotePath=false", "diff", "--name-only", "--diff-filter=U"]).map_err(hand_over)?.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    if !merged_clean && conflicts.is_empty() {
        return Err(hand_over(format!("`git merge` of `{base}` failed with no conflicted file to settle")));
    }
    let patterns = match env.forge.file_at(&pr.base_ref, GATES_FILE) {
        Ok(Some(text)) => gates::parse(&text).map_err(|e| hand_over(format!("the gate list on `{}` cannot be used: {e}", pr.base_ref)))?,
        Ok(None) => gates::DEFAULT_GATES.iter().map(|s| s.to_string()).collect(),
        Err(e) => return Err(hand_over(e.to_string())),
    };
    let plan = settle_plan(&conflicts, &patterns).map_err(hand_over)?;
    for (p, rule) in &plan {
        let file = dir.join(p);
        match rule {
            Rule::UnionBaseFirst => {
                let text = std::fs::read_to_string(&file).map_err(|e| hand_over(format!("{p}: {e}")))?;
                let settled = union_base_first(&text).map_err(|e| hand_over(format!("{p}: {e}")))?;
                std::fs::write(&file, settled).map_err(|e| hand_over(format!("{p}: {e}")))?;
            }
            Rule::TakeBase => {
                if g.git(dir, &["checkout", "--theirs", "--", p]).is_err() {
                    // The base removed it: so does the settlement (the compile writes it again if it should exist).
                    let _ = std::fs::remove_file(&file);
                }
            }
        }
    }
    g.git(dir, &["add", "-A"]).map_err(hand_over)?;
    let before_compile = g.git(dir, &["write-tree"]).map_err(hand_over)?.trim().to_string();
    recompile(dir).map_err(|e| hand_over(format!("the spec compile failed: {e}")))?;
    g.git(dir, &["add", "-A"]).map_err(hand_over)?;
    let after_compile = g.git(dir, &["write-tree"]).map_err(hand_over)?.trim().to_string();
    let regenerated: Vec<String> = g.git(dir, &["-c", "core.quotePath=false", "diff", "--name-only", &before_compile, &after_compile]).map_err(hand_over)?.lines().map(String::from).collect();

    let range = format!("{base}...{}", pr.head_sha);
    let diff = |tail: &[&str]| -> Result<String, Stop> {
        let mut args = vec!["-c", "core.quotePath=false", "diff", "-U0", "--no-color", "--no-ext-diff", "--no-renames"];
        args.extend_from_slice(tail);
        g.git(dir, &args).map_err(hand_over)
    };
    let before = diff(&[&range])?;
    let after = diff(&["--cached", &base])?;
    let wide = widened(&before, &after);
    if !wide.is_empty() {
        return Err(hand_over(format!("the result would change what the pull request changes outside the generated files: {}", wide.join(", "))));
    }

    let message = format!("Merge {} into {} (delphi merge refresh)", pr.base_ref, pr.head_ref);
    g.git(dir, &["commit", "-q", "-m", &message]).map_err(|e| hand_over(format!("could not commit: {e}")))?;
    let new_head = g.git(dir, &["rev-parse", "HEAD"]).map_err(Stop::HandOver)?.trim().to_string();
    let first_parent = g.git(dir, &["rev-parse", "HEAD^1"]).map_err(Stop::HandOver)?.trim().to_string();
    if first_parent != pr.head_sha {
        return Err(Stop::HandOver(format!("the new commit's first parent is `{first_parent}`, not the head `{}`", pr.head_sha)));
    }
    if let Some(f) = env.before_push {
        f();
    }
    // A plain push: if the branch moved meanwhile, git refuses it and nothing is overwritten. Never retried.
    g.git(dir, &["push", "-q", "origin", &format!("HEAD:refs/heads/{}", pr.head_ref)]).map_err(|e| Stop::HandOver(format!("the push was rejected (did the branch move meanwhile?): {e}")))?;
    let settled = plan.into_iter().map(|(p, _)| p).collect();
    Ok(Report { pr: pr.number, old_head: pr.head_sha.clone(), new_head, base_sha, settled, regenerated })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_closed_list_names_the_groups_file_the_tracker_and_the_compiled_tree_and_nothing_else() {
        assert_eq!(rule_for(spec_compile::GROUPS_FILE), Some(Rule::UnionBaseFirst));
        assert_eq!(rule_for(TRACKER), Some(Rule::UnionBaseFirst));
        assert_eq!(rule_for(&format!("{}/index.md", spec_compile::TREE_DIR)), Some(Rule::TakeBase));
        assert_eq!(rule_for(&format!("{}/x.json", spec_compile::DATA_DIR)), Some(Rule::TakeBase));
        for other in ["README.md", "Cargo.lock", "specs/094-core-repo-merge-flow.md", "compiled/other.toml", &format!("{}x/index.md", spec_compile::TREE_DIR)] {
            assert_eq!(rule_for(other), None, "{other}");
        }
    }

    #[test]
    fn a_conflict_outside_the_list_on_a_gate_path_or_in_more_than_ten_files_is_refused_with_the_paths() {
        let gates: Vec<String> = gates::DEFAULT_GATES.iter().map(|s| s.to_string()).collect();
        let ok = settle_plan(&[TRACKER.to_string(), spec_compile::GROUPS_FILE.to_string()], &gates).unwrap();
        assert_eq!(ok.len(), 2);
        let e = settle_plan(&[TRACKER.to_string(), "README.md".to_string()], &gates).unwrap_err();
        assert!(e.contains("README.md") && !e.contains(TRACKER), "{e}");
        let e = settle_plan(&["AGENTS.md".to_string()], &gates).unwrap_err();
        assert!(e.contains("gate path") && e.contains("AGENTS.md"), "{e}");
        let many: Vec<String> = (0..11).map(|i| format!("{}/data/{i}.json", spec_compile::TREE_DIR)).collect();
        assert!(settle_plan(&many, &gates).unwrap_err().contains("more than 10"));
        assert_eq!(settle_plan(&many[..10], &gates).unwrap().len(), 10);
    }

    #[test]
    fn a_union_keeps_the_base_lines_first_then_the_branch_lines_each_once() {
        let text = "head\n<<<<<<< HEAD\nmine\nshared\n=======\ntheirs\nshared\n>>>>>>> origin/main\ntail\n";
        assert_eq!(union_base_first(text).unwrap(), "head\ntheirs\nshared\nmine\ntail\n");
        assert_eq!(union_base_first("no conflict\n").unwrap(), "no conflict\n");
        assert!(union_base_first("<<<<<<< HEAD\na\n||||||| base\nb\n=======\nc\n>>>>>>> x\n").is_err(), "diff3 style is not read");
        assert!(union_base_first("<<<<<<< HEAD\na\n=======\nc\n").is_err(), "an unterminated conflict");
        assert!(union_base_first("=======\n").is_err());
        assert_eq!(union_ours_first(text).unwrap(), "head\nmine\nshared\ntheirs\ntail\n");
    }

    #[test]
    fn the_widening_check_compares_added_and_removed_lines_outside_the_generated_directory() {
        let d = |path: &str, body: &str| format!("diff --git a/{path} b/{path}\nindex 1..2 100644\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n{body}");
        let before = format!("{}{}", d("a.md", "-x\n+y\n"), d("compiled/z.md", "+1\n"));
        // The same change at another place, and a different generated change: not wider.
        let same = format!("{}{}", d("a.md", "+y\n-x\n"), d("compiled/z.md", "+2\n"));
        assert!(widened(&before, &same).is_empty());
        // One more line kept, a line no longer removed, a new path: each is wider.
        assert!(!widened(&before, &d("a.md", "-x\n+y\n+y2\n")).is_empty());
        assert!(!widened(&before, &d("a.md", "+y\n")).is_empty());
        assert!(widened(&before, &format!("{}{}", d("a.md", "-x\n+y\n"), d("b.md", "+q\n")))[0].contains("b.md"));
        // A new file and a deleted one are read by their real path.
        let new_file = "diff --git a/n.md b/n.md\nnew file mode 100644\n--- /dev/null\n+++ b/n.md\n@@ -0,0 +1 @@\n+hello\n";
        assert_eq!(changes(new_file).get("n.md"), Some(&(vec!["hello".to_string()], vec![])));
        let gone = "diff --git a/g.md b/g.md\ndeleted file mode 100644\n--- a/g.md\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\n";
        assert_eq!(changes(gone).get("g.md"), Some(&(vec![], vec!["bye".to_string()])));
    }
}
