//! `delphi merge`: the core-repo merge flow (spec 094).
//!
//! Agents queue a ready pull request with a label; the operator's instruction starts a merge; this command derives the facts
//! from git and `gh`, refuses anything that is not ready, records the approval, and merges only the head commit it derived.
//! Cleanup (S4), the error log and `merge fix` (S5) and `--auto` (S6) plug into the hooks marked below.
//!
//! This module is on the gate list (`merge-gates.toml`): a pull request that changes it needs a human at a terminal.

pub mod approve;
pub mod auto;
pub mod auto_batch;
pub mod base;
pub mod bisect;
pub mod check;
pub mod cleanup;
pub mod errors;
pub mod fix;
pub mod forge;
pub mod gates;
pub mod record;
pub mod refresh;
pub mod rollup;
pub mod rollup_pr;
pub mod wait;

#[cfg(test)]
mod auto_batch_tests;
#[cfg(test)]
mod auto_tests;
#[cfg(test)]
mod base_tests;
#[cfg(test)]
mod bisect_tests;
#[cfg(test)]
mod cleanup_tests;
#[cfg(test)]
mod fake;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod refresh_tests;
#[cfg(test)]
mod rollup_pr_tests;
#[cfg(test)]
mod rollup_tests;
#[cfg(test)]
mod wait_tests;

use approve::{attest, StdTerminal, Target, Terminal};
use check::{evaluate, Verdict, BEHIND_MARK, QUEUE_LABEL, REVIEW_LABEL};
use errors::{classify_failure, classify_refusal, log_central, Errors};
use forge::{Forge, ForgeError, Gh};
use record::{Attestation, Ledger, Record};
use std::time::Duration;

/// What happened to one pull request.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Merged { pr: u64, head: String },
    /// `--dry-run`: it would merge; nothing was written.
    DryRun { pr: u64, head: String, gate_changing: bool },
    Refused { pr: u64, reasons: Vec<String>, head: String },
    /// Gate-changing, in a batch: left for a human.
    NeedsHuman { pr: u64, paths: Vec<String> },
    /// The operator did not approve (or the approval was not acceptable).
    Declined { pr: u64, why: String },
    Failed { pr: u64, detail: String, head: String },
    /// Behind, so this run refreshed it (§11.3.3); CI runs on the new head. `waited`: its checks did not finish in the wait.
    Refreshed { pr: u64, head: String, waited: bool },
}

impl Outcome {
    /// Counts as success for the exit code: merged, would merge, or deliberately left for a human.
    pub fn ok(&self) -> bool {
        matches!(self, Outcome::Merged { .. } | Outcome::DryRun { .. } | Outcome::NeedsHuman { .. } | Outcome::Refreshed { .. })
    }
    pub fn line(&self) -> String {
        match self {
            Outcome::Merged { pr, head } => format!("#{pr}: merged (head {})", short(head)),
            Outcome::DryRun { pr, head, gate_changing } => format!("#{pr}: would merge (head {}){}", short(head), if *gate_changing { "; it changes a gate, so a person must confirm at a terminal" } else { "" }),
            Outcome::Refused { pr, reasons, .. } => format!("#{pr}: not merged, because {}", reasons.join("; ")),
            Outcome::NeedsHuman { pr, paths } => format!("#{pr}: left for a human, it changes a gate ({}); labelled `{REVIEW_LABEL}`. Merge it yourself: delphi merge {pr}", paths.join(", ")),
            Outcome::Declined { pr, why } => format!("#{pr}: not merged: {why}"),
            Outcome::Failed { pr, detail, .. } => format!("#{pr}: FAILED: {detail}"),
            Outcome::Refreshed { pr, head, waited: false } => format!("#{pr}: it was behind, so it was refreshed (new head {}); CI runs on it, and it merges at the next run once green", short(head)),
            Outcome::Refreshed { pr, head, waited: true } => format!("#{pr}: it was behind, so it was refreshed (new head {}), but its checks did not finish within the wait; run `delphi merge {pr}` again once they are green", short(head)),
        }
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}

/// Everything a merge needs besides the pull request number.
pub struct Ctx<'a> {
    pub forge: &'a dyn Forge,
    pub ledger: &'a Ledger,
    pub errors: &'a Errors,
    pub term: &'a mut dyn Terminal,
    pub quote: Option<String>,
    pub dry_run: bool,
    /// The repository to clean up after a merge; `None` leaves cleanup out (tests that have no repository).
    pub git: Option<&'a cleanup::Git>,
    pub session: Option<String>,
    /// How long to wait before asking again when GitHub has not finished working out mergeability.
    pub settle: Duration,
    pub now: fn() -> String,
    /// Refresh a pull request that is behind and otherwise ready (§11.3.3); `None` refuses it instead (tests of the refusal).
    pub refresh: Option<wait::Refreshing<'a>>,
    /// Where slice claims live and which repository this is (spec 135 F0b); `None` leaves them alone.
    pub claims: Option<Claims<'a>>,
    /// Where a rollup is rebuilt to recognise it (spec 094 §13.3); `None` refuses every rollup.
    pub rollup: Option<rollup_pr::Env<'a>>,
}

/// The slice claims a merge reads and releases (spec 135 §4, F0b): `home` holds `forge/claims.json`, `repo` is this
/// repository's id there (its common git directory).
#[derive(Clone, Copy)]
pub struct Claims<'a> {
    pub home: &'a std::path::Path,
    pub repo: &'a str,
    pub now: fn() -> i64,
}

impl Claims<'_> {
    /// Live claims on the pull request's linked spec held by another branch: a note, never a refusal (§14.9 item 4).
    fn notes(&self, v: &Verdict) -> Vec<String> {
        let mut out = Vec::new();
        for s in &v.specs {
            let (record::SpecState::Approved { path, .. } | record::SpecState::SpecPr { path, .. }) = s else { continue };
            match crate::slice_claims::others_on_spec(self.home, self.repo, path, &v.pr.head_ref, (self.now)()) {
                Ok(list) => out.extend(list.into_iter().map(|c| format!("#{}: note: another session holds a live slice claim on this spec: {c}", v.pr.number))),
                Err(e) => out.push(format!("#{}: note: the slice claims could not be read ({e})", v.pr.number)),
            }
        }
        out
    }
}

/// Claims of this repository in the Delphi home, for the command line: `None` outside a git checkout.
fn claims_here(home: &std::path::Path) -> Option<(std::path::PathBuf, String)> {
    let repo = crate::slice_claims::git_facts(&std::env::current_dir().ok()?).ok()?;
    Some((home.to_path_buf(), repo.common.display().to_string()))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Evaluate, and ask again a few times while the only objection is that GitHub is still computing mergeability (it is, right after
/// another pull request merged).
fn verdict(ctx: &Ctx, n: u64) -> Result<Verdict, ForgeError> {
    let mut tries = 0;
    loop {
        let v = evaluate(ctx.forge, n)?;
        let only_unknown = !v.refusals.is_empty() && v.refusals.iter().all(|r| r.starts_with("GitHub has not finished"));
        if only_unknown && tries < 8 {
            tries += 1;
            std::thread::sleep(ctx.settle);
            continue;
        }
        return Ok(v);
    }
}

/// Merge pull request `n`. `batch` is the approval already obtained for a whole `--all` run; without it, this asks. Whatever
/// the outcome, a problem somebody must act on is written to the error log, and a merge resolves the entries open for it.
pub fn merge_one(ctx: &mut Ctx, n: u64, batch: Option<&Attestation>) -> Outcome {
    let out = decide_one(ctx, n, batch);
    note(ctx, &out);
    out
}

/// Keep the books on an outcome (spec 094 §3.6). A dry run changes nothing, so it logs nothing.
fn note(ctx: &Ctx, out: &Outcome) {
    if ctx.dry_run {
        return;
    }
    let now = (ctx.now)();
    let put = |pr: u64, head: &str, class: errors::Class, detail: &str| put_error(ctx.errors, &now, pr, head, class, detail);
    match out {
        // A merge resolves what was open for it; that happens in `finish`, before cleanup, so a cleanup problem stays open.
        Outcome::Merged { .. } => {}
        Outcome::Refused { pr, reasons, head } => {
            // A refresh that was handed over has already written its `refresh-refused` entry.
            for class in reasons.iter().filter_map(|r| classify_refusal(r)).filter(|c| *c != errors::Class::RefreshRefused) {
                let detail = reasons.iter().filter(|r| classify_refusal(r) == Some(class)).cloned().collect::<Vec<_>>().join("; ");
                put(*pr, head, class, &detail);
            }
        }
        Outcome::Failed { pr, detail, head } => put(*pr, head, classify_failure(detail), detail),
        Outcome::DryRun { .. } | Outcome::NeedsHuman { .. } | Outcome::Declined { .. } | Outcome::Refreshed { .. } => {}
    }
}

/// Write one problem to the error log, and to the central log the first time it is seen.
fn put_error(errors: &Errors, now: &str, pr: u64, head: &str, class: errors::Class, detail: &str) {
    match errors.record(pr, head, class, detail, now) {
        Ok(e) if e.attempts == 1 => log_central(pr, class, e.attempts),
        Ok(_) => {}
        Err(e) => eprintln!("delphi merge: could not write the error log: {e}"),
    }
}

fn decide_one(ctx: &mut Ctx, n: u64, batch: Option<&Attestation>) -> Outcome {
    let v = match verdict(ctx, n) {
        Ok(v) => v,
        Err(e) => return Outcome::Failed { pr: n, detail: e.to_string(), head: String::new() },
    };
    // A rollup is its own kind (§13.3): recognised by a rebuild, or refused as before.
    if v.pr.head_ref.starts_with(rollup::BRANCH_PREFIX) {
        return rollup_pr::decide(ctx, v, batch.is_some());
    }
    // A gate-changing pull request is never queueable, so only an ordinary one must carry the label.
    let reasons = wait::reasons(&v);
    // Behind and otherwise ready: refreshed below, once the approval is in hand. Never on a dry run.
    let refresh = ctx.refresh.filter(|_| !ctx.dry_run && wait::refreshable(&v));
    if !reasons.is_empty() && refresh.is_none() {
        return Outcome::Refused { pr: n, reasons, head: v.pr.head_sha };
    }
    if let Some(c) = ctx.claims {
        for line in c.notes(&v) {
            ctx.term.show(&line);
        }
    }
    if v.gate_changing() && batch.is_some() {
        let _ = ctx.forge.set_label(n, REVIEW_LABEL, true);
        return Outcome::NeedsHuman { pr: n, paths: v.gate_hits };
    }
    if ctx.dry_run {
        let gate_changing = v.gate_changing();
        return Outcome::DryRun { pr: n, head: v.pr.head_sha, gate_changing };
    }
    if v.gate_changing() && ctx.term.is_interactive() {
        ctx.term.show(&format!("#{n} changes a gate:\n  {}\n", v.gate_hits.join("\n  ")));
        match ctx.forge.diff(n) {
            Ok(d) => ctx.term.show(&d),
            Err(e) => return Outcome::Failed { pr: n, detail: format!("could not read the diff to show you: {e}"), head: v.pr.head_sha },
        }
    }
    let att = match batch {
        Some(a) => a.clone(),
        None => match attest(ctx.term, ctx.quote.as_deref(), Target::Pr(n), v.gate_changing()) {
            Ok(a) => a,
            Err(why) => return Outcome::Declined { pr: n, why },
        },
    };
    if let Some(r) = refresh {
        return wait::refresh_and_merge(ctx, r, v, att);
    }
    let rec = v.record(att, ctx.session.clone(), (ctx.now)());
    finish(ctx, &v, rec)
}

/// Write the record, merge exactly the head that was derived, and leave the evidence.
fn finish(ctx: &mut Ctx, v: &Verdict, rec: Record) -> Outcome {
    let n = v.pr.number;
    // A head this run made by a refresh carries the approval given for the head it replaced (OD-8); the ledger says which.
    let carried_from = ctx.refresh.and_then(|r| r.made.borrow().get(&v.pr.head_sha).cloned());
    let row = |kind: &str| {
        let mut row = rec.to_json(kind);
        if let Some(old) = &carried_from {
            row["carried_from"] = serde_json::json!(old);
        }
        row
    };
    if let Err(e) = ctx.ledger.append(&row("approved")) {
        return Outcome::Failed { pr: n, detail: format!("cannot write the ledger ({e}); not merging without a record"), head: v.pr.head_sha.clone() };
    }
    match ctx.forge.merge(n, &v.pr.head_sha) {
        Ok(()) => {
            let _ = ctx.ledger.append(&row("merged"));
            let carried = carried_from.as_ref().map(|old| format!("\n\nThis head was made by `delphi merge refresh` in the same run, from `{old}`; the approval given for that head carried over (spec 094 §11.3.3).")).unwrap_or_default();
            if let Err(e) = ctx.forge.comment(n, &format!("{}{carried}", rec.comment())) {
                eprintln!("delphi merge: #{n} merged, but the comment could not be posted: {e}");
            }
            // The merge resolves what was open for this pull request. Cleanup then runs and never changes the outcome of the
            // merge; a step that could not finish (a locked worktree, a branch another pull request is based on) is printed and
            // stays open in the error log, because only a person can settle it.
            let now = (ctx.now)();
            ctx.errors.resolve_pr(n, &now);
            for problem in after_merge(ctx, v) {
                put_error(ctx.errors, &now, n, &v.pr.head_sha, errors::Class::Cleanup, &problem);
            }
            Outcome::Merged { pr: n, head: v.pr.head_sha.clone() }
        }
        Err(e) => {
            let mut row = row("failed");
            row["detail"] = serde_json::json!(e.to_string());
            let _ = ctx.ledger.append(&row);
            Outcome::Failed { pr: n, detail: e.to_string(), head: v.pr.head_sha.clone() }
        }
    }
}

/// Remove the remote branch, the local branch and the worktrees of the merged pull request, and release its branch's slice
/// claims. Returns the failures.
fn after_merge(ctx: &mut Ctx, v: &Verdict) -> Vec<String> {
    let mut problems = match ctx.git {
        Some(git) => cleanup::cleanup_pr(git, ctx.forge, v.pr.number, false).problems,
        None => Vec::new(),
    };
    // Spec 135 F0b: the merged branch's slice claims are done. A failure is a cleanup problem, never a change to the merge. A
    // fork's branch is not a branch of this repository, so a same-named local branch keeps its claims.
    if let Some(c) = ctx.claims.filter(|_| !v.pr.cross_repo) {
        match crate::slice_claims::release_branch(c.home, c.repo, &v.pr.head_ref) {
            Ok(lines) => lines.iter().for_each(|l| ctx.term.show(&format!("#{}: {l}", v.pr.number))),
            Err(e) => problems.push(format!("the slice claims of branch {} could not be released: {e}", v.pr.head_ref)),
        }
    }
    problems
}

/// Merge several queued pull requests, each re-evaluated just before it merges (the base moved after the last one).
pub fn merge_many(ctx: &mut Ctx, numbers: &[u64], batch: Option<&Attestation>) -> Vec<Outcome> {
    numbers.iter().map(|n| merge_one(ctx, *n, batch)).collect()
}

/// What `merge_checked` did: the outcomes, the lines about the base, and whether a drift stopped the run.
pub struct Checked {
    pub outcomes: Vec<Outcome>,
    pub lines: Vec<String>,
    pub drifted: bool,
}

/// The base a pull request lands on (`main` when it cannot be read: the check then says what it could not do).
pub fn base_of(forge: &dyn Forge, n: u64) -> String {
    forge.pr(n).map(|p| p.base_ref).unwrap_or_else(|_| "main".into())
}

/// `merge_many` with the check of the base (§11.3.4) after every merge. With `stops` (`--all`) the base is also checked before the
/// first pull request, and a drift merges nothing more: the run stops and names what it did not reach. Without (`<PR>`), a drift
/// is recorded and said, and the pull requests the operator named one by one still go ahead. A dry run checks nothing.
pub fn merge_checked(ctx: &mut Ctx, numbers: &[u64], batch: Option<&Attestation>, check: &dyn base::BaseCheck, stops: bool) -> Checked {
    let mut c = Checked { outcomes: Vec::new(), lines: Vec::new(), drifted: false };
    if ctx.dry_run {
        c.outcomes = merge_many(ctx, numbers, batch);
        return c;
    }
    let look = |ctx: &Ctx, n: u64, c: &mut Checked| {
        let (state, line) = base::check_and_note(ctx.errors, check, &base_of(ctx.forge, n), &(ctx.now)());
        c.lines.push(line);
        let drift = matches!(state, base::BaseState::Drift { .. });
        c.drifted |= drift;
        drift && stops
    };
    let not_reached = |rest: &[u64]| format!("stopped because the base drifted; not merged: {}", rest.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", "));
    if stops && !numbers.is_empty() && look(ctx, numbers[0], &mut c) {
        c.lines.push(not_reached(numbers));
        return c;
    }
    for (i, n) in numbers.iter().enumerate() {
        let out = merge_one(ctx, *n, batch);
        let merged = matches!(out, Outcome::Merged { .. });
        c.outcomes.push(out);
        if merged && look(ctx, *n, &mut c) {
            if i + 1 < numbers.len() {
                c.lines.push(not_reached(&numbers[i + 1..]));
            }
            break;
        }
    }
    c
}

/// Add a ready, ordinary pull request to the queue. A gate-changing one is labelled for a human instead.
pub fn queue_one(forge: &dyn Forge, n: u64) -> Result<String, String> {
    let v = evaluate(forge, n).map_err(|e| e.to_string())?;
    // The queue is advice and does not require freshness (§11.3.1): the merge itself refuses a head that is behind.
    if !v.ready_but_maybe_behind() {
        return Err(format!("#{n} is not ready to queue: {}", v.refusals.join("; ")));
    }
    if v.gate_changing() {
        let _ = forge.set_label(n, REVIEW_LABEL, true);
        return Err(format!("#{n} changes a gate ({}), so it is not queueable; labelled `{REVIEW_LABEL}`. A human merges it with `delphi merge {n}` at a terminal", v.gate_hits.join(", ")));
    }
    forge.set_label(n, QUEUE_LABEL, true).map_err(|e| e.to_string())?;
    let behind = if v.behind_by > 0 { format!("; it is {} commit(s) behind `{}`, so refresh it (`delphi merge refresh {n}`) before it can merge", v.behind_by, v.pr.base_ref) } else { String::new() };
    Ok(format!("#{n} queued (head {}){behind}", short(&v.pr.head_sha)))
}

/// One line per queued pull request: what `delphi merge --all` would do with it.
pub fn status_lines(forge: &dyn Forge) -> Result<Vec<String>, String> {
    let list = forge.queued(QUEUE_LABEL).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for n in list {
        let v = evaluate(forge, n).map_err(|e| e.to_string())?;
        out.push(if v.behind_by > 0 && wait::refreshable(&v) {
            format!("#{n}: {} {BEHIND_MARK} `{}`; will be refreshed at merge, then merged once CI is green on the new head", v.behind_by, v.pr.base_ref)
        } else if !v.ready() {
            format!("#{n}: not ready: {}", v.refusals.join("; "))
        } else if v.gate_changing() {
            format!("#{n}: ready, but it changes a gate ({}): needs a human", v.gate_hits.join(", "))
        } else {
            format!("#{n}: ready (head {})", short(&v.pr.head_sha))
        });
    }
    if out.is_empty() {
        out.push(format!("nothing is queued (no open pull request has the `{QUEUE_LABEL}` label)"));
    }
    Ok(out)
}

// ---- the command line ------------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Sub {
    Merge(Vec<u64>),
    All,
    Auto,
    Queue(u64),
    Status,
    Log,
    Fix(u64),
    Refresh(u64),
    Cleanup(Vec<u64>),
    RepairBase,
}

#[derive(Debug, PartialEq)]
struct Parsed {
    sub: Sub,
    home: Option<String>,
    quote: Option<String>,
    dry_run: bool,
    /// `log`: only entries still open.
    open: bool,
    json: bool,
    /// `log`: only this pull request.
    pr: Option<u64>,
    /// `fix`: the builder pushed its fix.
    done: bool,
    /// `fix`: the builder stopped; a person must look, and why.
    needs_human: Option<String>,
    /// `cleanup`: sweep every merged pull request whose branch is still around.
    merged: bool,
    /// `cleanup --merged`: the one explicit yes a mass delete needs.
    yes: bool,
    /// `--auto`: the session's limits (the defaults of spec 094 unless a flag changes them, within the ceilings).
    caps: auto::Caps,
    /// Any of the `--auto` limit flags was given.
    caps_given: bool,
    /// `<PR>` and `--all`: how long to wait for CI on a refreshed head (`--wait`); `None` is `--no-wait`.
    wait: Option<Duration>,
    /// `--wait` or `--no-wait` was given.
    wait_given: bool,
    /// `repair-base --base <branch>`: the base to repair (default `main`).
    base: Option<String>,
    /// `--batch[=N]` (spec 094 §13): build a rollup of up to N pull requests and open it (with `--dry-run`, only print it).
    batch: Option<usize>,
}

/// `--batch` (the default size) or `--batch=N`, N from 2 to the ceiling (§13.9).
fn batch_size(flag: &str) -> Result<Option<usize>, String> {
    let n = match flag.strip_prefix("--batch") {
        Some("") => rollup::DEFAULT_BATCH,
        Some(rest) => rest.strip_prefix('=').and_then(|n| n.parse::<usize>().ok()).ok_or(format!("`{flag}`: write --batch or --batch=N"))?,
        None => return Ok(None),
    };
    if !(2..=rollup::MAX_BATCH).contains(&n) {
        return Err(format!("--batch takes 2 to {} pull requests", rollup::MAX_BATCH));
    }
    Ok(Some(n))
}

fn pr_number(s: &str) -> Option<u64> {
    s.trim_start_matches('#').parse::<u64>().ok().filter(|n| *n > 0)
}

/// `args` is what `main` hands every command: the command line with the command word itself already removed.
fn parse(args: &[String]) -> Result<Parsed, String> {
    let (mut home, mut quote, mut dry_run, mut all, mut auto, mut merged, mut yes) = (None, None, false, false, false, false, false);
    let (mut open, mut json, mut pr, mut done, mut needs_human) = (false, false, None, false, None);
    let mut caps = auto::Caps::default();
    let mut caps_given = false;
    let (mut wait, mut wait_given) = (Some(wait::DEFAULT_WAIT), false);
    let mut base = None;
    let mut batch = None;
    let mut words: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--home" => home = Some(it.next().cloned().ok_or("--home needs a directory")?),
            "--operator-said" => quote = Some(it.next().cloned().ok_or("--operator-said needs the operator's words")?),
            "--dry-run" => dry_run = true,
            "--open" => open = true,
            "--json" => json = true,
            "--done" => done = true,
            "--pr" => pr = Some(it.next().and_then(|n| pr_number(n)).ok_or("--pr needs a pull request number")?),
            "--needs-human" => needs_human = Some(it.next().cloned().ok_or("--needs-human needs the reason, in words")?),
            "--all" => all = true,
            "--auto" => auto = true,
            "--max-prs" => {
                caps.max_prs = it.next().and_then(|n| n.parse().ok()).ok_or("--max-prs needs a number")?;
                caps_given = true;
            }
            "--for" => {
                caps.run_for = auto::parse_duration(it.next().ok_or("--for needs a duration (90s, 45m, 4h)")?)?;
                caps_given = true;
            }
            "--pause-after-errors" => {
                caps.pause_after_errors = it.next().and_then(|n| n.parse().ok()).ok_or("--pause-after-errors needs a number")?;
                caps_given = true;
            }
            "--idle" => {
                caps.idle = auto::parse_duration(it.next().ok_or("--idle needs a duration (90s, 45m, 4h)")?)?;
                caps_given = true;
            }
            "--poll" => {
                caps.poll = auto::parse_duration(it.next().ok_or("--poll needs a duration (90s, 45m, 4h)")?)?;
                caps_given = true;
            }
            "--wait" => {
                let d = auto::parse_duration(it.next().ok_or("--wait needs a duration (90s, 45m, 4h)")?)?;
                if d > Duration::from_secs(4 * 3600) {
                    return Err("--wait must be at most 4 hours".into());
                }
                (wait, wait_given) = (Some(d), true);
            }
            "--no-wait" => (wait, wait_given) = (None, true),
            "--base" => base = Some(it.next().cloned().ok_or("--base needs a branch name")?),
            "--merged" => merged = true,
            "--yes" => yes = true,
            f if f == "--batch" || f.starts_with("--batch=") => batch = batch_size(f)?,
            f if f.starts_with("--") => return Err(format!("unknown option `{f}` (see `delphi help merge`)")),
            w => words.push(w),
        }
    }
    let sub = match (words.first().copied(), all, auto) {
        (_, true, true) => return Err("--all and --auto are different modes; pick one".into()),
        (Some(w), false, false) if matches!(w, "queue" | "status" | "log" | "fix" | "refresh" | "cleanup" | "repair-base") => match w {
            "queue" => Sub::Queue(words.get(1).and_then(|n| pr_number(n)).ok_or("usage: delphi merge queue <PR>")?),
            "status" => Sub::Status,
            "repair-base" if words.len() == 1 => Sub::RepairBase,
            "repair-base" => return Err("usage: delphi merge repair-base [--base <branch>] [--dry-run] [--operator-said \"<words>\"]".into()),
            "log" => Sub::Log,
            "fix" => Sub::Fix(words.get(1).and_then(|n| pr_number(n)).ok_or("usage: delphi merge fix <PR> [--done | --needs-human \"<why>\"]")?),
            "refresh" => Sub::Refresh(words.get(1).and_then(|n| pr_number(n)).ok_or("usage: delphi merge refresh <PR>")?),
            _ => Sub::Cleanup(words[1..].iter().map(|w| pr_number(w).ok_or(format!("`{w}` is not a pull request number"))).collect::<Result<Vec<_>, _>>()?),
        },
        (_, true, false) if words.is_empty() => Sub::All,
        (_, false, true) if words.is_empty() => Sub::Auto,
        (_, true, false) | (_, false, true) => return Err("--all and --auto take no pull request numbers".into()),
        (Some(_), false, false) => Sub::Merge(words.iter().map(|w| pr_number(w).ok_or(format!("`{w}` is not a pull request number"))).collect::<Result<Vec<_>, _>>()?),
        (None, false, false) => return Err("usage: delphi merge <PR>... | --all | --auto | queue <PR> | status (see `delphi help merge`)".into()),
    };
    if (merged || yes) && !matches!(sub, Sub::Cleanup(_)) {
        return Err("--merged and --yes belong to `delphi merge cleanup`".into());
    }
    if caps_given && sub != Sub::Auto {
        return Err("--max-prs, --for, --pause-after-errors, --idle and --poll belong to `delphi merge --auto`".into());
    }
    if wait_given && !matches!(sub, Sub::Merge(_) | Sub::All) {
        return Err("--wait and --no-wait belong to `delphi merge <PR>` and `--all` (a session never waits; it comes back at its next poll)".into());
    }
    if base.is_some() && sub != Sub::RepairBase {
        return Err("--base belongs to `delphi merge repair-base`".into());
    }
    if batch.is_some() && !matches!(sub, Sub::Merge(_) | Sub::All | Sub::Auto) {
        return Err("--batch belongs to `delphi merge <PR>...`, `--all` and `--auto` (spec 094 §13.9)".into());
    }
    if sub == Sub::Auto && batch.is_some() && dry_run {
        return Err("a session has no dry run; see a batch with `delphi merge --all --batch --dry-run`".into());
    }
    if sub == Sub::Auto {
        caps.batch = batch;
        caps.validate()?;
    }
    Ok(Parsed { sub, home, quote, dry_run, open, json, pr, done, needs_human, merged, yes, caps, caps_given, wait, wait_given, base, batch })
}

pub fn cli(args: &[String]) -> i32 {
    let p = match parse(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi merge: {e}");
            return 2;
        }
    };
    let forge = Gh;
    match p.sub {
        Sub::Queue(n) => match queue_one(&forge, n) {
            Ok(m) => {
                println!("{m}");
                0
            }
            Err(e) => {
                eprintln!("delphi merge: {e}");
                1
            }
        },
        Sub::Status => match status_lines(&forge) {
            Ok(lines) => {
                lines.iter().for_each(|l| println!("{l}"));
                let home = crate::home::delphi_home(p.home.as_deref());
                // Open rollups, a bisecting half among them (spec 094 §13.5).
                bisect::status_lines(&forge, &Ledger::at(&home)).iter().for_each(|l| println!("{l}"));
                // The batches of the latest `--auto --batch` session (S22).
                auto_batch::status_lines(&Ledger::at(&home)).iter().for_each(|l| println!("{l}"));
                // The check of the base (§11.3.4): the base of the oldest queued pull request, or `main`.
                let base_ref = forge.queued(QUEUE_LABEL).ok().and_then(|q| q.first().copied()).map(|n| base_of(&forge, n)).unwrap_or_else(|| "main".into());
                match std::env::current_dir() {
                    Ok(repo) => println!("{}", base::check_and_note(&Errors::at(&home), &base::InRepo { git: refresh::Runner::new(&repo), home: &home }, &base_ref, &delphi_core::time::now_rfc3339()).1),
                    Err(e) => println!("base `{base_ref}`: could not check it ({e})"),
                }
                0
            }
            Err(e) => {
                eprintln!("delphi merge: {e}");
                4
            }
        },
        Sub::Log => log_cmd(&p),
        Sub::Fix(n) => fix_cmd(&p, &forge, n),
        Sub::Refresh(n) => refresh_cmd(&p, &forge, n),
        Sub::RepairBase => repair_cmd(&p, &forge),
        Sub::Cleanup(ref numbers) => cleanup::run(&forge, numbers, p.merged, p.dry_run, p.yes),
        Sub::Auto => auto_cmd(&p, &forge),
        Sub::Merge(ref numbers) if p.batch.is_some() => batch_cmd(&p, &forge, numbers, false),
        Sub::Merge(ref numbers) => run_merges(&p, &forge, numbers.clone(), false),
        Sub::All => match forge.queued(QUEUE_LABEL) {
            Ok(list) if list.is_empty() => {
                println!("nothing is queued");
                0
            }
            Ok(list) if p.batch.is_some() => batch_cmd(&p, &forge, &list, true),
            Ok(list) => run_merges(&p, &forge, list, true),
            Err(e) => {
                eprintln!("delphi merge: {e}");
                4
            }
        },
    }
}

/// `log`: the error entries (all, or only the open ones) and the most recent ledger rows.
pub fn log_view(home: &std::path::Path, open_only: bool, pr: Option<u64>) -> (Vec<errors::Entry>, Vec<serde_json::Value>) {
    let entries: Vec<errors::Entry> = Errors::at(home).entries().into_iter().filter(|e| (!open_only || e.open()) && pr.is_none_or(|p| e.pr == p)).collect();
    let rows: Vec<serde_json::Value> = Ledger::at(home).read().into_iter().filter(|r| pr.is_none_or(|p| r["pr"].as_u64() == Some(p))).collect();
    let skip = rows.len().saturating_sub(20);
    (entries, rows.into_iter().skip(skip).collect())
}

fn log_cmd(p: &Parsed) -> i32 {
    let home = crate::home::delphi_home(p.home.as_deref());
    let (entries, rows) = log_view(&home, p.open, p.pr);
    if p.json {
        for e in &entries {
            println!("{}", serde_json::json!({"source": "errors", "id": e.id, "pr": e.pr, "class": e.class, "state": e.state, "attempts": e.attempts, "head_sha": e.head_sha, "detail": e.detail, "note": e.note, "first": e.first, "last": e.last}));
        }
        for r in &rows {
            println!("{}", serde_json::json!({"source": "ledger", "row": r}));
        }
        return 0;
    }
    if entries.is_empty() {
        println!("errors: none{}", if p.open { " open" } else { "" });
    } else {
        println!("errors:");
        entries.iter().for_each(|e| println!("  {}", e.line()));
    }
    if !p.open {
        if rows.is_empty() {
            println!("ledger: nothing recorded yet ({})", Ledger::at(&home).path().display());
        } else {
            println!("ledger (latest {}):", rows.len());
            for r in &rows {
                println!("  {} #{} {} head {} {}", r["time"].as_str().unwrap_or("?"), r["pr"], r["kind"].as_str().unwrap_or("?"), short(r["head_sha"].as_str().unwrap_or("?")), r["attestation"].as_str().unwrap_or(""));
            }
        }
    }
    0
}

fn fix_cmd(p: &Parsed, forge: &Gh, n: u64) -> i32 {
    let home = crate::home::delphi_home(p.home.as_deref());
    let errors = Errors::at(&home);
    let now = delphi_core::time::now_rfc3339();
    let result = if p.done {
        fix::done(forge, &errors, n, &now)
    } else if let Some(why) = &p.needs_human {
        fix::needs_human(&errors, n, why, &now)
    } else {
        // The repository is the one the command is run in.
        match std::env::current_dir().map_err(|e| e.to_string()) {
            Ok(repo) => fix::prepare(&fix::Git { repo }, forge, &errors, &home, n, &now).map(|plan| {
                format!(
                    "{}\nworktree: {}{}\nbrief: {}\nGive the builder the brief (it names the git-merge-and-conflict-resolution skill). When it has pushed: delphi merge fix {n} --done",
                    plan.entry.line(),
                    plan.worktree.display(),
                    if plan.reused { " (the branch was already checked out here)" } else { "" },
                    plan.brief_path.display()
                )
            }),
            Err(e) => Err(e),
        }
    };
    match result {
        Ok(m) => {
            println!("{m}");
            0
        }
        Err(e) => {
            eprintln!("delphi merge fix: {e}");
            1
        }
    }
}

/// `refresh <PR>`: update a behind pull request's branch by a merge of its base (§11.3.2). It never merges the pull request and
/// needs no approval of its own; an agent may run it, as it may run `fix`.
fn refresh_cmd(p: &Parsed, forge: &Gh, n: u64) -> i32 {
    let home = crate::home::delphi_home(p.home.as_deref());
    let repo = match std::env::current_dir() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi merge refresh: {e}");
            return 2;
        }
    };
    let (errors, ledger, runner) = (Errors::at(&home), Ledger::at(&home), refresh::Runner::new(&repo));
    let env = refresh::Env { forge, errors: &errors, ledger: &ledger, home: &home, git: &runner, now: delphi_core::time::now_rfc3339(), before_push: None };
    match refresh::refresh(&env, n) {
        Ok(refresh::Done::Current) => {
            println!("#{n}: already contains the tip of its base; nothing to refresh");
            0
        }
        Ok(refresh::Done::Refreshed(r)) => {
            let list = |v: &[String]| if v.is_empty() { "none".to_string() } else { v.join(", ") };
            println!("#{n}: refreshed: head {} (was {}), base {}", short(&r.new_head), short(&r.old_head), short(&r.base_sha));
            println!("  settled by rule: {}", list(&r.settled));
            println!("  regenerated by the spec compile: {}", list(&r.regenerated));
            println!("  CI runs again on the new head; merge it once it is green (`delphi merge {n}`, on the operator's word)");
            0
        }
        Err(stop) => {
            eprintln!("delphi merge refresh: {}", stop.line(n));
            1
        }
    }
}

/// `--batch` (spec 094 §13): with `--dry-run` (S18), choose, build the candidate in a temporary worktree, check its invariants and
/// print it, writing nothing; without, the operator approves the batch, the rollup is opened (S19) and merged once green (S20).
fn batch_cmd(p: &Parsed, forge: &Gh, numbers: &[u64], from_all: bool) -> i32 {
    if crate::xenia_terminal::inside_an_agent_seat() && !p.dry_run {
        eprintln!("delphi merge: refused; merging is the operator's, and this process was started by an agent seat");
        return 2;
    }
    let home = crate::home::delphi_home(p.home.as_deref());
    let repo = match std::env::current_dir() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi merge --batch: {e}");
            return 2;
        }
    };
    let id = delphi_core::time::now_rfc3339().chars().filter(|c| c.is_ascii_digit()).take(14).collect::<String>();
    let max = p.batch.unwrap_or(rollup::DEFAULT_BATCH);
    let runner = refresh::Runner::new(&repo);
    if !p.dry_run {
        let (ledger, errors, mut term, git) = (Ledger::at(&home), Errors::at(&home), StdTerminal, cleanup::Git::here());
        let made = std::cell::RefCell::new(std::collections::HashMap::new());
        let refresher = wait::InRepo { forge, errors: &errors, ledger: &ledger, home: &home, git: refresh::Runner::new(&repo) };
        // A rollup is never refreshed; `Refreshing` here only carries the wait for its checks (§13.2 step 6).
        let refresh = wait::Refreshing { refresher: &refresher, clock: &auto::SystemClock, wait: p.wait, poll: Duration::from_secs(30), made: &made };
        let here = claims_here(&home);
        let claims = here.as_ref().map(|(h, r)| Claims { home: h, repo: r, now: unix_now });
        let mut ctx = Ctx { forge, ledger: &ledger, errors: &errors, term: &mut term, quote: p.quote.clone(), dry_run: false, git: Some(&git), session: None, settle: Duration::from_secs(3), now: delphi_core::time::now_rfc3339, refresh: Some(refresh), claims, rollup: Some(rollup_pr::Env { git: &runner, home: &home }) };
        return match rollup_pr::batch(&mut ctx, numbers, max, from_all, &id) {
            Ok(out) => {
                println!("{}", out.line());
                i32::from(!out.ok())
            }
            Err(e) => {
                eprintln!("delphi merge --batch: no rollup: {e}");
                1
            }
        };
    }
    let choice = rollup::choose(forge, numbers, max);
    for (n, why) in &choice.skipped {
        println!("#{n}: not in the batch: {why}");
    }
    match rollup::build(forge, &runner, &home, &choice, &id) {
        Ok(c) => {
            c.lines().iter().for_each(|l| println!("{l}"));
            println!("dry run: nothing was pushed, opened or merged");
            0
        }
        Err(e) => {
            eprintln!("delphi merge --batch: no rollup: {e}");
            1
        }
    }
}

/// `repair-base`: open the pull request that makes the base's compiled spec tree current (§11.3.4). It asks the operator before it
/// pushes or opens anything, and never merges.
fn repair_cmd(p: &Parsed, forge: &Gh) -> i32 {
    let home = crate::home::delphi_home(p.home.as_deref());
    let repo = match std::env::current_dir() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi merge repair-base: {e}");
            return 2;
        }
    };
    let (errors, ledger, runner) = (Errors::at(&home), Ledger::at(&home), refresh::Runner::new(&repo));
    let env = base::RepairEnv { forge, errors: &errors, ledger: &ledger, home: &home, git: &runner, now: delphi_core::time::now_rfc3339() };
    let base_ref = p.base.clone().unwrap_or_else(|| "main".into());
    match base::repair_base(&env, &base_ref, p.dry_run, &mut StdTerminal, p.quote.as_deref()) {
        Ok(base::Repaired::Current { sha }) => println!("`{base_ref}` ({}): the spec compile changes nothing; there is nothing to repair", short(&sha)),
        Ok(base::Repaired::WouldOpen { sha, paths }) => println!("`{base_ref}` ({}): a repair would recompile {} file(s): {}; nothing was pushed", short(&sha), paths.len(), paths.join(", ")),
        Ok(base::Repaired::Opened { pr, branch, paths, .. }) => println!("opened #{pr} from `{branch}` ({} file(s)); it merges nothing: merge it once CI is green", paths.len()),
        Err(e) => {
            eprintln!("delphi merge repair-base: {e}");
            return 1;
        }
    }
    0
}

/// Set by Ctrl-C (SIGINT) or SIGTERM; read by the session loop.
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    INTERRUPTED.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn auto_cmd(p: &Parsed, forge: &Gh) -> i32 {
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi merge --auto: refused; a session is started by the operator, and this process was started by an agent seat");
        return 2;
    }
    let home = crate::home::delphi_home(p.home.as_deref());
    let stop_file = home.join("merge").join("STOP");
    // A leftover STOP file from an earlier session would end this one at once; the operator asked for this session now.
    let _ = std::fs::remove_file(&stop_file);
    let ledger = Ledger::at(&home);
    let error_log = Errors::at(&home);
    let mut term = StdTerminal;
    term.show(&format!(
        "--auto session: up to {} pull request(s), at most {} minute(s), pausing after {} error(s) in a row{}. It never merges a pull request that changes a gate.",
        p.caps.max_prs,
        p.caps.run_for.as_secs() / 60,
        p.caps.pause_after_errors,
        p.caps.batch.map(|n| format!(", through rollups of up to {n} queued pull requests each")).unwrap_or_default()
    ));
    let att = match attest(&mut term, p.quote.as_deref(), Target::Auto, false) {
        Ok(a) => a,
        Err(why) => {
            eprintln!("delphi merge --auto: {why}");
            return 1;
        }
    };
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    let git = cleanup::Git::here();
    let made = std::cell::RefCell::new(std::collections::HashMap::new());
    let refresher = match in_repo(forge, &error_log, &ledger, &home) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi merge --auto: {e}");
            return 2;
        }
    };
    // A session refreshes a behind pull request and moves on; its next poll merges it once CI is green (§11.3.3).
    let refresh = wait::Refreshing { refresher: &refresher, clock: &auto::SystemClock, wait: None, poll: p.caps.poll, made: &made };
    let here = claims_here(&home);
    let claims = here.as_ref().map(|(h, r)| Claims { home: h, repo: r, now: unix_now });
    // `--batch` (S22): rollups are built in this repository, under the same session approval.
    let runner = refresh::Runner::new(&refresher.git.repo);
    let rollup = p.caps.batch.map(|_| rollup_pr::Env { git: &runner, home: &home });
    let mut ctx = Ctx { forge, ledger: &ledger, errors: &error_log, term: &mut term, quote: None, dry_run: false, git: Some(&git), session: None, settle: Duration::from_secs(3), now: delphi_core::time::now_rfc3339, refresh: Some(refresh), claims, rollup };
    let stop = || INTERRUPTED.load(std::sync::atomic::Ordering::SeqCst) || stop_file.exists();
    let check = base::InRepo { git: refresh::Runner::new(&refresher.git.repo), home: &home };
    let summary = auto::run_checked(&mut ctx, &p.caps, &att, &auto::SystemClock, &stop, &mut |l| println!("{l}"), Some(&check));
    summary.lines().iter().for_each(|l| println!("{l}"));
    if summary.errors > 0 {
        1
    } else {
        0
    }
}

/// The refresh a merge command runs: in the repository the command was started in, as `delphi merge refresh` does.
fn in_repo<'a>(forge: &'a Gh, errors: &'a Errors, ledger: &'a Ledger, home: &'a std::path::Path) -> Result<wait::InRepo<'a>, String> {
    let repo = std::env::current_dir().map_err(|e| e.to_string())?;
    Ok(wait::InRepo { forge, errors, ledger, home, git: refresh::Runner::new(&repo) })
}

fn run_merges(p: &Parsed, forge: &Gh, numbers: Vec<u64>, batch: bool) -> i32 {
    // An agent seat started by a Delphi daemon never merges (spec 094 §3.8). The check is an environment variable, so it is a
    // courtesy message, not the control; the control is deferred to the hardening pass (§5).
    if crate::xenia_terminal::inside_an_agent_seat() && !p.dry_run {
        eprintln!("delphi merge: refused; merging is the operator's, and this process was started by an agent seat");
        return 2;
    }
    let home = crate::home::delphi_home(p.home.as_deref());
    let ledger = Ledger::at(&home);
    let error_log = Errors::at(&home);
    let mut term = StdTerminal;
    let batch_att = if batch && !p.dry_run {
        term.show(&format!("{} queued pull request(s): {}", numbers.len(), numbers.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ")));
        match attest(&mut term, p.quote.as_deref(), Target::Batch, false) {
            Ok(a) => Some(a),
            Err(why) => {
                eprintln!("delphi merge: {why}");
                return 1;
            }
        }
    } else {
        None
    };
    let git = cleanup::Git::here();
    let made = std::cell::RefCell::new(std::collections::HashMap::new());
    let refresher = match in_repo(forge, &error_log, &ledger, &home) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi merge: {e}");
            return 2;
        }
    };
    let refresh = wait::Refreshing { refresher: &refresher, clock: &auto::SystemClock, wait: p.wait, poll: Duration::from_secs(30), made: &made };
    let here = claims_here(&home);
    let claims = here.as_ref().map(|(h, r)| Claims { home: h, repo: r, now: unix_now });
    let mut ctx = Ctx { forge, ledger: &ledger, errors: &error_log, term: &mut term, quote: p.quote.clone(), dry_run: p.dry_run, git: Some(&git), session: None, settle: Duration::from_secs(3), now: delphi_core::time::now_rfc3339, refresh: Some(refresh), claims, rollup: None };
    // `<PR>` asks for each pull request itself; --all holds one approval for the batch, and a drifted base stops it (§11.3.4).
    let check = base::InRepo { git: refresh::Runner::new(&refresher.git.repo), home: &home };
    let checked = merge_checked(&mut ctx, &numbers, batch_att.as_ref(), &check, batch);
    let outcomes = checked.outcomes;
    outcomes.iter().for_each(|o| println!("{}", o.line()));
    checked.lines.iter().for_each(|l| println!("{l}"));
    if checked.drifted {
        return 1;
    }
    if outcomes.iter().any(|o| matches!(o, Outcome::Failed { detail, .. } if detail.contains("cannot run `gh`"))) {
        return 4;
    }
    if outcomes.iter().all(|o| o.ok()) {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    /// The arguments as `main` passes them: the words after `delphi`, without the command word `merge`.
    fn a(s: &str) -> Vec<String> {
        s.split_whitespace().filter(|w| *w != "merge").map(String::from).collect()
    }

    #[test]
    fn the_command_line_forms_parse() {
        assert_eq!(parse(&a("merge 552 #553")).unwrap().sub, Sub::Merge(vec![552, 553]));
        assert!(parse(&a("merge --all --dry-run")).unwrap().dry_run);
        assert_eq!(parse(&a("merge --all")).unwrap().sub, Sub::All);
        assert_eq!(parse(&a("merge --auto")).unwrap().sub, Sub::Auto);
        let p = parse(&a("merge --auto --max-prs 3 --for 2h --idle 10m --poll 30s --pause-after-errors 1")).unwrap();
        assert_eq!((p.caps.max_prs, p.caps.run_for.as_secs(), p.caps.idle.as_secs(), p.caps.poll.as_secs(), p.caps.pause_after_errors), (3, 7200, 600, 30, 1));
        assert_eq!(parse(&a("merge queue 7")).unwrap().sub, Sub::Queue(7));
        assert_eq!(parse(&a("merge status")).unwrap().sub, Sub::Status);
        assert_eq!(parse(&a("merge refresh #806")).unwrap().sub, Sub::Refresh(806));
        assert!(parse(&a("merge refresh")).is_err() && parse(&a("merge refresh x")).is_err());
        let r = parse(&a("merge repair-base --base main --dry-run")).unwrap();
        assert_eq!((r.sub, r.base.as_deref(), r.dry_run), (Sub::RepairBase, Some("main"), true));
        assert!(parse(&a("merge repair-base 5")).is_err() && parse(&a("merge 5 --base main")).is_err());
        assert_eq!(parse(&a("--home /x merge 5")).unwrap().home.as_deref(), Some("/x"));
        assert_eq!(parse(&a("merge 5")).unwrap().wait, Some(wait::DEFAULT_WAIT));
        assert_eq!(parse(&a("merge 5 --wait 10m")).unwrap().wait, Some(Duration::from_secs(600)));
        assert_eq!(parse(&a("merge --all --no-wait")).unwrap().wait, None);
        let q = vec!["5".to_string(), "--operator-said".to_string(), "merge #5 now".to_string()];
        assert_eq!(parse(&q).unwrap().quote.as_deref(), Some("merge #5 now"));
        assert_eq!(parse(&a("merge --all --batch --dry-run")).unwrap().batch, Some(rollup::DEFAULT_BATCH));
        let b = parse(&a("merge 5 6 --batch=8 --dry-run")).unwrap();
        assert_eq!((b.sub, b.batch), (Sub::Merge(vec![5, 6]), Some(8)));
        assert_eq!(parse(&a("merge --all")).unwrap().batch, None);
        assert_eq!(parse(&a("merge --auto --batch=3 --max-prs 6")).unwrap().caps.batch, Some(3));
        assert_eq!(parse(&a("merge --auto")).unwrap().caps.batch, None);
    }

    #[test]
    fn batch_size_is_bounded_and_belongs_to_pr_numbers_and_all() {
        assert_eq!(parse(&a("merge --all --batch")).unwrap().batch, Some(rollup::DEFAULT_BATCH));
        for bad in ["merge --all --batch=1 --dry-run", "merge --all --batch=9 --dry-run", "merge --all --batch=x --dry-run", "merge --all --batchy --dry-run", "merge --auto --batch --dry-run", "merge status --batch --dry-run"] {
            assert!(parse(&a(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn nonsense_is_a_usage_error() {
        for bad in ["merge", "merge --all --auto", "merge --auto --max-prs 0", "merge --auto --for 48h", "merge --auto --poll 1s", "merge --max-prs 3 5", "merge --auto --for soon", "merge --all 5", "merge abc", "merge 0", "merge --bogus 5", "merge queue", "merge queue x", "merge --auto --no-wait", "merge status --wait 5m", "merge 5 --wait 5h", "merge 5 --wait"] {
            assert!(parse(&a(bad)).is_err(), "{bad}");
        }
        assert!(parse(&["5".to_string(), "--operator-said".to_string()]).is_err());
    }
}
