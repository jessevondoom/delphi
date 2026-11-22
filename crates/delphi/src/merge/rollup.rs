//! Batched merges through a rollup pull request (spec 094 §13): the candidate builder (slice S18).
//!
//! Given pull requests the operator would approve as a batch, it chooses those that may be batched (ready except perhaps for
//! freshness, not gate-changing, §13.2 step 1), builds in a temporary worktree of its own under the Delphi home a branch from the
//! base tip with each constituent's head merged in turn (one merge commit each, in order; never a rebase), settles conflicts only
//! by `refresh`'s closed list with the earlier side first, runs the spec compile after each merge, and checks the invariants of
//! §13.4: the candidate changes exactly what its constituents change (outside the generated files), and it touches no gate path.
//!
//! This module pushes nothing, opens nothing and merges nothing; `rollup_pr` opens a candidate as a pull request (S19), recognises
//! it again by `rebuild` (§13.3) and merges it (S20). This module is on the gate list.

use super::check::{evaluate, BEHIND_MARK};
use super::forge::{Forge, ForgeError};
use super::gates::{self, GATES_FILE};
use super::record::SpecState;
use super::refresh::{changes, recompile, settle_plan, union_ours_first, Rule, Runner, GENERATED_ROOT};
use super::{short, wait};
use std::collections::BTreeMap;
use std::path::Path;

/// Every rollup branch starts with this (§13.3).
pub const BRANCH_PREFIX: &str = "delphi-rollup/";
/// The first line of a manifest, and the fence it lives in.
pub const MANIFEST_TAG: &str = "delphi-rollup v1";
pub const FENCE: &str = "```delphi-rollup";
/// `--batch` alone (OD-14).
pub const DEFAULT_BATCH: usize = 4;
pub const MAX_BATCH: usize = 8;

/// One pull request in a batch, bound to the head the operator is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constituent {
    pub pr: u64,
    pub head_sha: String,
    pub head_ref: String,
    pub title: String,
    /// The spec its trailers name; `None` for a Markdown-only change or the bounded exemption.
    pub spec: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Choice {
    pub base_ref: String,
    pub chosen: Vec<Constituent>,
    /// Pull requests left for the serial path, with the reason.
    pub skipped: Vec<(u64, String)>,
}

/// Choose up to `max` pull requests from `numbers`, in order (§13.2 step 1). Only the facts are read; nothing is written.
pub fn choose(forge: &dyn Forge, numbers: &[u64], max: usize) -> Choice {
    let mut c = Choice::default();
    for &n in numbers {
        if c.chosen.len() >= max {
            c.skipped.push((n, format!("the batch is full ({max}); it waits for the next one")));
            continue;
        }
        let v = match evaluate(forge, n) {
            Ok(v) => v,
            Err(e) => {
                c.skipped.push((n, e.to_string()));
                continue;
            }
        };
        if v.gate_changing() {
            c.skipped.push((n, format!("it changes a gate ({}); a gate-changing pull request is never batched", v.gate_hits.join(", "))));
            continue;
        }
        let blocking: Vec<String> = wait::reasons(&v).into_iter().filter(|r| !r.contains(BEHIND_MARK)).collect();
        if !blocking.is_empty() {
            c.skipped.push((n, blocking.join("; ")));
            continue;
        }
        if c.base_ref.is_empty() {
            c.base_ref = v.pr.base_ref.clone();
        } else if v.pr.base_ref != c.base_ref {
            c.skipped.push((n, format!("its base is `{}`, not the batch's `{}`", v.pr.base_ref, c.base_ref)));
            continue;
        }
        let spec = spec_of(&v);
        c.chosen.push(Constituent { pr: n, head_sha: v.pr.head_sha, head_ref: v.pr.head_ref, title: v.pr.title, spec });
    }
    c
}

/// The spec a constituent names, as its manifest line carries it (`None`: Markdown only or the bounded exemption).
pub fn spec_of(v: &super::check::Verdict) -> Option<String> {
    match &v.spec {
        SpecState::Approved { path, .. } | SpecState::SpecPr { path, .. } => Some(path.clone()),
        _ => None,
    }
}

/// The manifest block a rollup pull request's body carries (§13.3).
pub fn manifest(base_ref: &str, base_sha: &str, merged: &[Constituent]) -> String {
    let mut out = format!("{FENCE}\n{MANIFEST_TAG}\nbase {base_ref} {base_sha}\n");
    for c in merged {
        out.push_str(&format!("pr {} {} {}\n", c.pr, c.head_sha, c.spec.as_deref().unwrap_or("-")));
    }
    out.push_str("```\n");
    out
}

/// What a manifest says. Trusted for nothing: every fact in it is derived again before a rollup merges (§13.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub base_ref: String,
    pub base_sha: String,
    /// `(pr, head, spec)` in merge order.
    pub prs: Vec<(u64, String, Option<String>)>,
}

fn is_sha(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Read the one manifest block of a pull request body; anything else (none, two, a malformed line) is an error.
pub fn parse_manifest(body: &str) -> Result<Manifest, String> {
    let blocks: Vec<&str> = body.split(FENCE).skip(1).collect();
    if blocks.len() != 1 {
        return Err(format!("a rollup body carries exactly one `{FENCE}` block; this one has {}", blocks.len()));
    }
    let block = blocks[0].split("```").next().unwrap_or("");
    let mut lines = block.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some(MANIFEST_TAG) {
        return Err(format!("the manifest does not start with `{MANIFEST_TAG}`"));
    }
    let base: Vec<&str> = lines.next().unwrap_or("").split_whitespace().collect();
    let (base_ref, base_sha) = match base.as_slice() {
        ["base", r, s] if is_sha(s) && !r.is_empty() && !r.contains("..") => (r.to_string(), s.to_string()),
        _ => return Err("the manifest's second line is not `base <branch> <sha>`".into()),
    };
    let mut prs: Vec<(u64, String, Option<String>)> = Vec::new();
    for l in lines {
        let w: Vec<&str> = l.split_whitespace().collect();
        let row = match w.as_slice() {
            ["pr", n, h, s] if is_sha(h) => n.parse::<u64>().ok().filter(|n| *n > 0).map(|n| (n, h.to_string(), if *s == "-" { None } else { Some(s.to_string()) })),
            _ => None,
        };
        let row = row.ok_or_else(|| format!("a manifest line is not `pr <number> <head sha> <spec or ->`: `{l}`"))?;
        if prs.iter().any(|p| p.0 == row.0) {
            return Err(format!("the manifest names #{} twice", row.0));
        }
        prs.push(row);
    }
    if prs.is_empty() {
        return Err("the manifest names no pull request".into());
    }
    Ok(Manifest { base_ref, base_sha, prs })
}

/// Invariant 1 (§13.4): per path outside the generated directory, the candidate's added and removed lines against the base equal
/// the sum of the constituents' own. Returns what differs, if anything.
pub fn union_differs(parts: &[String], whole: &str) -> Vec<String> {
    let outside = |m: BTreeMap<String, (Vec<String>, Vec<String>)>| m.into_iter().filter(|(p, _)| !p.starts_with(GENERATED_ROOT)).collect::<BTreeMap<_, _>>();
    let mut sum: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for part in parts {
        for (p, (a, r)) in outside(changes(part)) {
            let e = sum.entry(p).or_default();
            e.0.extend(a);
            e.1.extend(r);
        }
    }
    for (a, r) in sum.values_mut() {
        a.sort();
        r.sort();
    }
    let whole = outside(changes(whole));
    let mut out = Vec::new();
    for p in sum.keys().chain(whole.keys()).collect::<std::collections::BTreeSet<_>>() {
        match (sum.get(p), whole.get(p)) {
            (Some(x), Some(y)) if x == y => {}
            (Some(_), Some(_)) => out.push(format!("{p} (its added or removed lines are not the constituents' own)")),
            (Some(_), None) => out.push(format!("{p} (a constituent changes it; the rollup does not)")),
            (None, _) => out.push(format!("{p} (only the rollup changes it)")),
        }
    }
    out
}

/// A built candidate: what S19 would push as `branch` and open with `manifest` in the body.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub branch: String,
    pub base_ref: String,
    pub base_sha: String,
    pub head: String,
    pub tree: String,
    /// Each merged constituent with its merge commit in the candidate (one can be reverted alone with `git revert -m 1`).
    pub merged: Vec<(Constituent, String)>,
    /// Constituents left out of the build, with the reason (class `rollup-refused` or `rollup-stale` once S19 logs them).
    pub dropped: Vec<(u64, String)>,
    pub settled: Vec<String>,
    pub regenerated: Vec<String>,
    /// Every path the candidate changes against the base.
    pub paths: Vec<String>,
}

impl Candidate {
    pub fn manifest(&self) -> String {
        manifest(&self.base_ref, &self.base_sha, &self.merged.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>())
    }
    /// What `--dry-run` prints.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!("rollup `{}` from `{}` ({}): {} pull request(s), head {} (tree {})", self.branch, self.base_ref, short(&self.base_sha), self.merged.len(), short(&self.head), short(&self.tree))];
        for (c, m) in &self.merged {
            out.push(format!("  #{} {} (head {}, spec {}): merge commit {}", c.pr, c.title, short(&c.head_sha), c.spec.as_deref().unwrap_or("none"), short(m)));
        }
        for (n, why) in &self.dropped {
            out.push(format!("  #{n}: left out: {why}"));
        }
        let list = |v: &[String]| if v.is_empty() { "none".to_string() } else { v.join(", ") };
        out.push(format!("  settled by rule: {}", list(&self.settled)));
        out.push(format!("  regenerated by the spec compile: {}", list(&self.regenerated)));
        out.push(format!("  changes {} path(s); the union-of-diffs and gate checks passed", self.paths.len()));
        out.push("  manifest:".into());
        out.extend(self.manifest().lines().map(|l| format!("    {l}")));
        out
    }
}

/// Build the candidate for `choice` in `dir` (removed at the end, whatever happened). `id` names the branch. Never pushes.
pub fn build(forge: &dyn Forge, git: &Runner, home: &Path, choice: &Choice, id: &str) -> Result<Candidate, String> {
    build_at(forge, git, home, choice, id, None)
}

/// The same build from `base_sha` (a manifest's base) instead of the base tip: how a rollup is recognised at merge time (§13.3).
pub fn rebuild(forge: &dyn Forge, git: &Runner, home: &Path, choice: &Choice, id: &str, base_sha: &str) -> Result<Candidate, String> {
    build_at(forge, git, home, choice, id, Some(base_sha))
}

fn build_at(forge: &dyn Forge, git: &Runner, home: &Path, choice: &Choice, id: &str, at: Option<&str>) -> Result<Candidate, String> {
    if choice.chosen.is_empty() {
        return Err("no pull request can be batched".into());
    }
    let dir = home.join("merge").join("rollup").join(id);
    let result = work(forge, git, &dir, choice, id, at);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = git.git(&git.repo, &["worktree", "prune"]);
    result
}

fn work(forge: &dyn Forge, g: &Runner, dir: &Path, choice: &Choice, id: &str, at: Option<&str>) -> Result<Candidate, String> {
    let repo = g.repo.as_path();
    let base_ref = &choice.base_ref;
    let mut fetch = vec!["fetch", "-q", "origin", base_ref.as_str()];
    fetch.extend(choice.chosen.iter().map(|c| c.head_ref.as_str()));
    g.git(repo, &fetch).map_err(|e| format!("could not fetch: {e}"))?;
    let base_sha = match at {
        Some(sha) => g.git(repo, &["rev-parse", "--verify", "-q", &format!("{sha}^{{commit}}")]).map_err(|e| format!("the base `{sha}` is not a commit here: {e}"))?.trim().to_string(),
        None => g.git(repo, &["rev-parse", "--verify", "-q", &format!("refs/remotes/origin/{base_ref}")])?.trim().to_string(),
    };
    let patterns = match forge.file_at(base_ref, GATES_FILE).map_err(|e: ForgeError| e.to_string())? {
        Some(text) => gates::parse(&text).map_err(|e| format!("the gate list on `{base_ref}` cannot be used: {e}"))?,
        None => gates::DEFAULT_GATES.iter().map(|s| s.to_string()).collect(),
    };
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.parent().expect("has a parent")).map_err(|e| e.to_string())?;
    g.git(repo, &["worktree", "add", "-q", "--detach", &dir.to_string_lossy(), &base_sha]).map_err(|e| format!("could not make a worktree: {e}"))?;

    let (mut merged, mut dropped, mut settled, mut regenerated, mut parts) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for c in &choice.chosen {
        // The head on the remote must still be the one the operator is shown (§13.4 invariant 2).
        let tip = g.git(repo, &["rev-parse", "--verify", "-q", &format!("refs/remotes/origin/{}", c.head_ref)]).unwrap_or_default();
        if tip.trim() != c.head_sha {
            dropped.push((c.pr, format!("its branch is at `{}`, not the head `{}` it was chosen at", short(tip.trim()), short(&c.head_sha))));
            continue;
        }
        match merge_in(g, dir, c, base_ref, &patterns) {
            Ok((commit, s, r)) => {
                let part = g.git(dir, &["-c", "core.quotePath=false", "diff", "-U0", "--no-color", "--no-ext-diff", "--no-renames", &format!("{base_sha}...{}", c.head_sha)])?;
                parts.push(part);
                merged.push((c.clone(), commit));
                settled.extend(s.into_iter().filter(|p| !settled.contains(p)).collect::<Vec<_>>());
                regenerated.extend(r.into_iter().filter(|p| !regenerated.contains(p)).collect::<Vec<_>>());
            }
            Err(why) => {
                let _ = g.git(dir, &["merge", "--abort"]);
                // Back to the last good merge, without what the compile wrote (this worktree is the command's own).
                let _ = g.git(dir, &["reset", "-q", "--hard"]);
                let _ = g.git(dir, &["clean", "-q", "-d", "--force"]);
                dropped.push((c.pr, why));
            }
        }
    }
    if merged.is_empty() {
        return Err(format!("no pull request could be merged into the candidate: {}", dropped.iter().map(|(n, w)| format!("#{n}: {w}")).collect::<Vec<_>>().join("; ")));
    }
    let whole = g.git(dir, &["-c", "core.quotePath=false", "diff", "-U0", "--no-color", "--no-ext-diff", "--no-renames", &base_sha, "HEAD"])?;
    let differs = union_differs(&parts, &whole);
    if !differs.is_empty() {
        return Err(format!("the candidate does not change exactly what its pull requests change: {}", differs.join(", ")));
    }
    let paths: Vec<String> = g.git(dir, &["-c", "core.quotePath=false", "diff", "--name-only", "--no-renames", &base_sha, "HEAD"])?.lines().map(String::from).collect();
    let gate = gates::hits(&patterns, &paths);
    if !gate.is_empty() {
        return Err(format!("the candidate touches a gate path ({}); a rollup never does", gate.join(", ")));
    }
    let head = g.git(dir, &["rev-parse", "HEAD"])?.trim().to_string();
    let tree = g.git(dir, &["rev-parse", "HEAD^{tree}"])?.trim().to_string();
    let c = Candidate { branch: format!("{BRANCH_PREFIX}{id}"), base_ref: base_ref.clone(), base_sha, head, tree, merged, dropped, settled, regenerated, paths };
    // The manifest a rollup would be opened with must read back to exactly what was built (S19 recognises a rollup by it).
    let m = parse_manifest(&c.manifest())?;
    let built: Vec<(u64, String, Option<String>)> = c.merged.iter().map(|(k, _)| (k.pr, k.head_sha.clone(), k.spec.clone())).collect();
    if m.base_sha != c.base_sha || m.base_ref != c.base_ref || m.prs != built {
        return Err("the manifest does not read back to the candidate that was built".into());
    }
    Ok(c)
}

/// Merge one constituent into the candidate checked out in `dir`: settle by the closed list, compile, commit. Returns the merge
/// commit, the settled paths and the regenerated ones. On an error the caller aborts the merge.
fn merge_in(g: &Runner, dir: &Path, c: &Constituent, base_ref: &str, patterns: &[String]) -> Result<(String, Vec<String>, Vec<String>), String> {
    let clean = g.git(dir, &["-c", "merge.conflictStyle=merge", "merge", "-q", "--no-ff", "--no-commit", &c.head_sha]).is_ok();
    let conflicts: Vec<String> = g.git(dir, &["-c", "core.quotePath=false", "diff", "--name-only", "--diff-filter=U"])?.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    if !clean && conflicts.is_empty() {
        return Err("`git merge` failed with no conflicted file to settle".into());
    }
    let plan = settle_plan(&conflicts, patterns)?;
    for (p, rule) in &plan {
        match rule {
            Rule::UnionBaseFirst => {
                let file = dir.join(p);
                let text = std::fs::read_to_string(&file).map_err(|e| format!("{p}: {e}"))?;
                std::fs::write(&file, union_ours_first(&text).map_err(|e| format!("{p}: {e}"))?).map_err(|e| format!("{p}: {e}"))?;
            }
            Rule::TakeBase => {
                if g.git(dir, &["checkout", "--ours", "--", p]).is_err() {
                    let _ = std::fs::remove_file(dir.join(p));
                }
            }
        }
    }
    g.git(dir, &["add", "-A"])?;
    let before = g.git(dir, &["write-tree"])?.trim().to_string();
    recompile(dir).map_err(|e| format!("the spec compile failed: {e}"))?;
    g.git(dir, &["add", "-A"])?;
    let after = g.git(dir, &["write-tree"])?.trim().to_string();
    let regenerated = g.git(dir, &["-c", "core.quotePath=false", "diff", "--name-only", &before, &after])?.lines().map(String::from).collect();
    let message = format!("Rollup: merge #{} (head {}) into {base_ref} (delphi merge --batch)\n\n{}", c.pr, short(&c.head_sha), c.title);
    g.git(dir, &["commit", "-q", "-m", &message]).map_err(|e| format!("could not commit: {e}"))?;
    let commit = g.git(dir, &["rev-parse", "HEAD"])?.trim().to_string();
    Ok((commit, plan.into_iter().map(|(p, _)| p).collect(), regenerated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_round_trips_and_a_malformed_one_is_refused() {
        let cs = vec![
            Constituent { pr: 881, head_sha: "a".repeat(40), head_ref: "x".into(), title: "t".into(), spec: Some("specs/093-x.md".into()) },
            Constituent { pr: 883, head_sha: "b".repeat(40), head_ref: "y".into(), title: "u".into(), spec: None },
        ];
        let body = format!("Rollup of two.\n\n{}", manifest("main", &"c".repeat(40), &cs));
        let m = parse_manifest(&body).unwrap();
        assert_eq!((m.base_ref.as_str(), m.base_sha.len(), m.prs.len()), ("main", 40, 2));
        assert_eq!(m.prs[0], (881, "a".repeat(40), Some("specs/093-x.md".into())));
        assert_eq!(m.prs[1].2, None);
        assert!(parse_manifest("no block").is_err());
        assert!(parse_manifest(&format!("{body}\n{body}")).is_err(), "two blocks");
        assert!(parse_manifest(&body.replace("pr 883", "pr 881")).is_err(), "a number twice");
        assert!(parse_manifest(&body.replace(&"b".repeat(40), "HEAD")).is_err(), "not a sha");
        assert!(parse_manifest(&body.replace(MANIFEST_TAG, "delphi-rollup v9")).is_err());
        assert!(parse_manifest(&format!("{FENCE}\n{MANIFEST_TAG}\nbase main {}\n```\n", "c".repeat(40))).is_err(), "no pull request");
    }

    #[test]
    fn the_union_check_accepts_the_sum_of_the_parts_and_nothing_else() {
        let d = |path: &str, body: &str| format!("diff --git a/{path} b/{path}\nindex 1..2 100644\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n{body}");
        let a = format!("{}{}", d("t.md", "+row a\n"), d("x.rs", "-old\n+new\n"));
        let b = format!("{}{}", d("t.md", "+row b\n"), d("compiled/v.md", "+1\n"));
        let whole = format!("{}{}{}", d("t.md", "+row b\n+row a\n"), d("x.rs", "-old\n+new\n"), d("compiled/v.md", "+2\n"));
        assert!(union_differs(&[a.clone(), b.clone()], &whole).is_empty());
        // A line no constituent added, a line made once where two made it, a path only the rollup changes: each refused.
        assert!(!union_differs(&[a.clone(), b.clone()], &format!("{whole}{}", d("t.md", "+smuggled\n"))).is_empty());
        let twice = d("y.md", "+same\n");
        assert!(!union_differs(&[twice.clone(), twice.clone()], &twice).is_empty());
        assert!(union_differs(&[a.clone(), b.clone()], &format!("{whole}{}", d("evil.sh", "+curl\n")))[0].contains("evil.sh"));
        assert!(union_differs(&[a, b], &d("t.md", "+row b\n+row a\n")).iter().any(|w| w.contains("x.rs")));
    }
}
