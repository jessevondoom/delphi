//! The preconditions (spec 094 §3.2): what a pull request must be before `delphi merge` will touch it, decided from facts read
//! at this moment, never from a label or a caller's say-so. `evaluate` never merges and never writes; it returns a verdict that
//! lists every reason the PR is not ready, so a refusal is specific.

use super::forge::{Check, Forge, ForgeError, PrInfo};
use super::gates::{self, Source, GATES_FILE};
use super::record::{blake3_hex, paths_hash, spec_approval, spec_links, Attestation, Record, SpecState};
use std::collections::{BTreeMap, BTreeSet};

pub const QUEUE_LABEL: &str = "merge-queue";
pub const REVIEW_LABEL: &str = "needs-human-review";

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub pr: PrInfo,
    pub paths: Vec<String>,
    /// The changed paths that are in the gate class; empty for an ordinary pull request.
    pub gate_hits: Vec<String>,
    pub gates_source: Source,
    /// The checks judged: only those of the newest run of each workflow on the head (spec 094 §18).
    pub checks: Vec<Check>,
    /// What was left out of `checks` and why ("ignored 1 check entry from superseded run 45430975730 ..."); empty when nothing was.
    pub check_notes: Vec<String>,
    /// The first spec state (kept for callers and ledger readers that expect one).
    pub spec: SpecState,
    /// Every distinct spec the trailers name, each judged on its own; one entry (`Exempt`, `MarkdownOnly` or `Missing`) when none is named.
    pub specs: Vec<SpecState>,
    /// Every reason this pull request cannot merge now; empty when it can.
    pub refusals: Vec<String>,
    /// How many commits of the base tip the head does not contain (0 when current, or when the pull request is not open).
    pub behind_by: u64,
}

/// How every "no checks yet" refusal starts (spec 094 §16.2); `wait::not_yet` and `errors::classify_refusal` find it by this.
pub const NO_CHECKS_MARK: &str = "no checks have reported";

/// No check has reported on `head`: a distinct state, "no checks yet". While a command waits it is pending; for a merge it is
/// never green. Worded without "conflicts with", which is the conflict class's mark.
pub fn no_checks_reason(head: &str) -> String {
    format!("{NO_CHECKS_MARK} on head `{}` yet: CI may not have started (or the pull request has a conflict with its base, and such a pull request runs no CI)", super::short(head))
}

/// The words every freshness refusal carries; `errors::classify_refusal` finds the class `behind` by them.
pub const BEHIND_MARK: &str = "commit(s) behind";

/// The freshness refusal (spec 094 §11.3.1): what is wrong and what to do about it.
pub fn behind_reason(n: u64, behind_by: u64, base_ref: &str) -> String {
    format!("it is {behind_by} {BEHIND_MARK} `{base_ref}`, so its checks ran against an older base: refresh it (`delphi merge refresh {n}`) and wait for CI on the new head")
}

impl Verdict {
    pub fn ready(&self) -> bool {
        self.refusals.is_empty()
    }
    /// Ready except for being behind the base. Queueing asks this (the queue is advice, §11.3.1): the merge checks freshness.
    pub fn ready_but_maybe_behind(&self) -> bool {
        self.refusals.iter().all(|r| r.contains(BEHIND_MARK))
    }
    pub fn gate_changing(&self) -> bool {
        !self.gate_hits.is_empty()
    }
    /// The record for this verdict, stamped with how it was approved.
    pub fn record(&self, attestation: Attestation, session: Option<String>, time: String) -> Record {
        Record {
            pr: self.pr.number,
            head_sha: self.pr.head_sha.clone(),
            base_ref: self.pr.base_ref.clone(),
            base_sha: self.pr.base_sha.clone(),
            paths_count: self.paths.len(),
            paths_hash: paths_hash(&self.paths),
            spec: self.spec.clone(),
            specs: self.specs.clone(),
            checks: self.checks.iter().map(|c| (c.name.clone(), c.bucket.clone())).collect(),
            gate_changing: self.gate_changing(),
            attestation,
            session,
            time,
        }
    }
}

/// Is the change inside the bounded exemption of `specs/003` §3.1.2: at most 2 files and 100 changed lines, no host-executed file,
/// dependency manifest or lockfile, nothing under the charter, the rubric, `agents/` or `skills/`, and nothing in the gate class?
fn within_exemption(paths: &[String], additions: u64, deletions: u64, gate_hits: &[String]) -> bool {
    const BLOCKED: &[&str] = &[
        "**/Cargo.toml",
        "**/Cargo.lock",
        "**/package.json",
        "**/package-lock.json",
        "**/build.rs",
        ".cargo/**",
        ".envrc",
        ".gitattributes",
        ".gitmodules",
        ".github/**",
        ".vscode/**",
        "GOVERNANCE.md",
        "REVIEW.md",
        "agents/**",
        "skills/**",
    ];
    paths.len() <= 2 && additions + deletions <= 100 && gate_hits.is_empty() && !paths.iter().any(|p| BLOCKED.iter().any(|b| gates::matches(b, p)))
}

/// Documents and Stage 1 spec pull requests: every changed file is Markdown, and none is under a governed directory that needs a
/// spec of its own (`agents/`, `skills/`, `themes/`, `memory/`). `specs/` is allowed: a spec pull request has no spec to link.
fn markdown_only(paths: &[String]) -> bool {
    const NEEDS_A_SPEC: &[&str] = &["agents/**", "skills/**", "themes/**", "memory/**"];
    !paths.is_empty() && paths.iter().all(|p| p.to_lowercase().ends_with(".md") && !NEEDS_A_SPEC.iter().any(|g| gates::matches(g, p)))
}

/// Judge every distinct spec the pull request's trailers name (any commit, or the body). Several slices may share one pull request
/// only if they are slices of the SAME spec: trailers naming more than one distinct spec are refused outright (one `Missing`
/// naming them all). With one spec, that spec must be ratified.
fn spec_states(forge: &dyn Forge, pr: &PrInfo, paths: &[String], gate_hits: &[String]) -> Result<Vec<SpecState>, ForgeError> {
    let links = spec_links(&pr.commit_messages, &pr.body);
    if links.is_empty() {
        return Ok(vec![if markdown_only(paths) {
            SpecState::MarkdownOnly
        } else if within_exemption(paths, pr.additions, pr.deletions, gate_hits) {
            SpecState::Exempt
        } else {
            SpecState::Missing("no `Implements-Spec:` trailer, and the change is neither only Markdown nor inside the bounded exemption (more than 2 files or 100 lines, or it touches code, a manifest, a lockfile, or `agents/`, `skills/`, `themes/` or `memory/`)".into())
        }]);
    }
    if links.len() > 1 {
        return Ok(vec![SpecState::Missing(format!(
            "the pull request names {} different specs ({}); a bundled pull request may carry slices of one spec only: split it into one pull request per spec",
            links.len(),
            links.join(", ")
        ))]);
    }
    let mut states: Vec<SpecState> = Vec::new();
    for link in links {
        let touched = paths.iter().any(|p| p == &link);
        let head_text = forge.file_at(&pr.head_sha, &link)?;
        let state = match forge.file_at(&pr.base_ref, &link)? {
            Some(base) if spec_approval(&base).0 => match &head_text {
                // An amendment must leave the spec approved.
                Some(h) if !touched || spec_approval(h).0 => SpecState::Approved { path: link.clone(), blake3: blake3_hex(h) },
                Some(_) => SpecState::Missing(format!("{link} is approved on {} but this pull request changes it to a state that is not approved", pr.base_ref)),
                None => SpecState::Missing(format!("{link} does not exist on the pull request's head")),
            },
            _ => match head_text {
                // A Stage 1 spec pull request: it brings the approval itself, with the operator's name on it.
                Some(h) if touched && spec_approval(&h).0 && spec_approval(&h).1 => SpecState::SpecPr { path: link.clone(), blake3: blake3_hex(&h) },
                _ => SpecState::Missing(format!("{link} is not approved on {} (it needs `status: approved` and `approved_by:` in its frontmatter)", pr.base_ref)),
            },
        };
        states.push(state);
    }
    Ok(states)
}

/// Decide whether pull request `n` could merge now, from facts only. Whether it is *queued* is the caller's to judge: an ordinary
/// pull request must carry the label, while a gate-changing one is never queueable and a human merges it directly.
pub fn evaluate(forge: &dyn Forge, n: u64) -> Result<Verdict, ForgeError> {
    let pr = forge.pr(n)?;
    let mut refusals: Vec<String> = Vec::new();

    if pr.state != "OPEN" {
        refusals.push(format!("it is {}, not open", pr.state.to_lowercase()));
    }
    if pr.is_draft {
        refusals.push("it is a draft".into());
    }
    match pr.mergeable.as_str() {
        "MERGEABLE" => {}
        "CONFLICTING" => refusals.push(format!("it conflicts with `{}`: merge the base into the branch and resolve (the builder's git skill covers this)", pr.base_ref)),
        other => refusals.push(format!("GitHub has not finished working out whether it merges (`{}`): try again in a moment", if other.is_empty() { "unknown" } else { other })),
    }
    // Freshness (§11.3.1): CI tested the head merged with the base as it was at the last push; a head that does not contain the
    // base tip was never tested against what it would land on. A failed compare is an error (`?`), never a pass.
    let behind_by = if pr.state == "OPEN" { forge.behind_by(&pr.base_ref, &pr.head_sha)? } else { 0 };
    if behind_by > 0 {
        refusals.push(behind_reason(n, behind_by, &pr.base_ref));
    }

    // Only the newest run of each workflow on this head is judged (§18); what was left out is said, never hidden.
    let (checks, check_notes) = current_checks(forge, &pr.head_sha, forge.checks(n)?)?;
    if checks.is_empty() {
        refusals.push(no_checks_reason(&pr.head_sha));
    } else {
        let names = |wanted: &dyn Fn(&str) -> bool| checks.iter().filter(|c| wanted(&c.bucket)).map(|c| c.name.clone()).collect::<Vec<_>>().join(", ");
        let failed = names(&|b| b == "fail" || b == "cancel");
        let pending = names(&|b| !matches!(b, "pass" | "fail" | "cancel" | "skipping"));
        let also = if check_notes.is_empty() { String::new() } else { format!(" ({})", check_notes.join("; ")) };
        if !failed.is_empty() {
            refusals.push(format!("checks failed: {failed}{also}"));
        }
        if !pending.is_empty() {
            refusals.push(format!("checks are still running: {pending}{also}"));
        }
    }

    let paths = forge.changed_paths(n)?;
    // The gate list is read from the base branch, never from the pull request.
    let (patterns, gates_source) = match forge.file_at(&pr.base_ref, GATES_FILE)? {
        Some(text) => match gates::parse(&text) {
            Ok(p) => (p, Source::BaseBranch),
            Err(e) => {
                refusals.push(format!("the gate list on `{}` cannot be used, so nothing can be judged: {e}", pr.base_ref));
                (gates::DEFAULT_GATES.iter().map(|s| s.to_string()).collect(), Source::BuiltIn)
            }
        },
        None => (gates::DEFAULT_GATES.iter().map(|s| s.to_string()).collect(), Source::BuiltIn),
    };
    let gate_hits = gates::hits(&patterns, &paths);

    let specs = spec_states(forge, &pr, &paths, &gate_hits)?;
    for s in &specs {
        if let SpecState::Missing(why) = s {
            refusals.push(why.clone());
        }
    }
    // The first Missing if any, else the first state: a single-spec reader still sees a blocked bundle as blocked.
    let spec = specs.iter().find(|s| matches!(s, SpecState::Missing(_))).or(specs.first()).cloned().expect("at least one spec state");

    Ok(Verdict { pr, paths, gate_hits, gates_source, checks, check_notes, spec, specs, refusals, behind_by })
}

/// At most this many distinct workflow runs are read to learn their head commit; more is an error, never a pass.
const MAX_RUNS: usize = 16;

/// Keep the checks that speak for `head` now, as GitHub's own required check does (spec 094 §18). `gh pr checks` lists, beside
/// the newest run's jobs, any job of an older run of the same workflow whose name the newest run does not reuse: on #909 a
/// draft-time run that GitHub cancelled when the ready run started left a `macos tests` entry, CANCELLED, next to a green run.
///
/// 1. Every Actions run the checks name is read for its head commit (one read-only call per distinct run, at most `MAX_RUNS`); an
///    entry from a run on another commit is left out. A run that cannot be read is an error.
/// 2. Of the rest, per workflow and event, only the newest run (the highest run id: GitHub numbers runs in the order they are
///    created) is kept. An entry from an older run of the same workflow on the same head was superseded, and is left out.
/// 3. A check with no Actions run (a status from another service) or no workflow name is always kept and judged as it is.
///
/// Whatever is kept is judged as before: a failed, cancelled or timed-out entry of the newest run fails, a running one is pending,
/// none at all is "no checks yet". Returns the kept checks and one note per run left out.
pub fn current_checks(forge: &dyn Forge, head: &str, all: Vec<Check>) -> Result<(Vec<Check>, Vec<String>), ForgeError> {
    let runs: BTreeSet<u64> = all.iter().filter_map(|c| c.run).collect();
    if runs.len() > MAX_RUNS {
        return Err(ForgeError::Failed { what: "judge the checks".into(), detail: format!("they come from {} workflow runs, more than the {MAX_RUNS} this command reads", runs.len()) });
    }
    let mut heads: BTreeMap<u64, String> = BTreeMap::new();
    for r in runs {
        heads.insert(r, forge.run_head(r)?);
    }
    let on_head = |c: &Check| c.run.is_none_or(|r| heads.get(&r).is_some_and(|h| h == head));
    let group = |c: &Check| c.run.filter(|_| !c.workflow.is_empty()).map(|r| ((c.workflow.clone(), c.event.clone()), r));
    let mut newest: BTreeMap<(String, String), u64> = BTreeMap::new();
    for (key, r) in all.iter().filter(|c| on_head(c)).filter_map(group) {
        let n = newest.entry(key).or_insert(r);
        *n = (*n).max(r);
    }
    let (mut kept, mut other_head, mut superseded) = (Vec::new(), BTreeMap::<u64, usize>::new(), BTreeMap::<u64, usize>::new());
    for c in all {
        if !on_head(&c) {
            *other_head.entry(c.run.unwrap_or(0)).or_default() += 1;
        } else if let Some((_, r)) = group(&c).filter(|(key, r)| newest.get(key) != Some(r)) {
            *superseded.entry(r).or_default() += 1;
        } else {
            kept.push(c);
        }
    }
    let entries = |n: usize| if n == 1 { "1 check entry".to_string() } else { format!("{n} check entries") };
    let mut notes: Vec<String> = superseded.iter().map(|(r, n)| format!("ignored {} from superseded run {r} (a newer run of the same workflow on this head replaced it)", entries(*n))).collect();
    notes.extend(other_head.iter().map(|(r, n)| format!("ignored {} from run {r} on head `{}`, not the current head `{}`", entries(*n), super::short(heads.get(r).map(String::as_str).unwrap_or("?")), super::short(head))));
    Ok((kept, notes))
}
