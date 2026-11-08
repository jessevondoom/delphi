//! `delphi merge`: the core-repo merge flow (spec 094).
//!
//! Agents queue a ready pull request with a label; the operator's instruction starts a merge; this command derives the facts
//! from git and `gh`, refuses anything that is not ready, records the approval, and merges only the head commit it derived.
//! Cleanup (S4), the error log and `merge fix` (S5) and `--auto` (S6) plug into the hooks marked below.
//!
//! This module is on the gate list (`merge-gates.toml`): a pull request that changes it needs a human at a terminal.

pub mod approve;
pub mod auto;
pub mod check;
pub mod cleanup;
pub mod errors;
pub mod fix;
pub mod forge;
pub mod gates;
pub mod record;

#[cfg(test)]
mod auto_tests;
#[cfg(test)]
mod cleanup_tests;
#[cfg(test)]
mod fake;
#[cfg(test)]
mod flow_tests;

use approve::{attest, StdTerminal, Target, Terminal};
use check::{evaluate, Verdict, QUEUE_LABEL, REVIEW_LABEL};
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
}

impl Outcome {
    /// Counts as success for the exit code: merged, would merge, or deliberately left for a human.
    pub fn ok(&self) -> bool {
        matches!(self, Outcome::Merged { .. } | Outcome::DryRun { .. } | Outcome::NeedsHuman { .. })
    }
    pub fn line(&self) -> String {
        match self {
            Outcome::Merged { pr, head } => format!("#{pr}: merged (head {})", short(head)),
            Outcome::DryRun { pr, head, gate_changing } => format!("#{pr}: would merge (head {}){}", short(head), if *gate_changing { "; it changes a gate, so a person must confirm at a terminal" } else { "" }),
            Outcome::Refused { pr, reasons, .. } => format!("#{pr}: not merged, because {}", reasons.join("; ")),
            Outcome::NeedsHuman { pr, paths } => format!("#{pr}: left for a human, it changes a gate ({}); labelled `{REVIEW_LABEL}`. Merge it yourself: delphi merge {pr}", paths.join(", ")),
            Outcome::Declined { pr, why } => format!("#{pr}: not merged: {why}"),
            Outcome::Failed { pr, detail, .. } => format!("#{pr}: FAILED: {detail}"),
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

fn queued(v: &Verdict) -> bool {
    v.pr.labels.iter().any(|l| l == QUEUE_LABEL)
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
            for class in reasons.iter().filter_map(|r| classify_refusal(r)) {
                let detail = reasons.iter().filter(|r| classify_refusal(r) == Some(class)).cloned().collect::<Vec<_>>().join("; ");
                put(*pr, head, class, &detail);
            }
        }
        Outcome::Failed { pr, detail, head } => put(*pr, head, classify_failure(detail), detail),
        Outcome::DryRun { .. } | Outcome::NeedsHuman { .. } | Outcome::Declined { .. } => {}
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
    let mut reasons = v.refusals.clone();
    // A gate-changing pull request is never queueable, so only an ordinary one must carry the label.
    if !v.gate_changing() && !queued(&v) {
        reasons.insert(0, format!("it is not queued (no `{QUEUE_LABEL}` label; an agent queues it with `delphi merge queue {n}`)"));
    }
    if !reasons.is_empty() {
        return Outcome::Refused { pr: n, reasons, head: v.pr.head_sha };
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
    let rec = v.record(att, ctx.session.clone(), (ctx.now)());
    finish(ctx, &v, rec)
}

/// Write the record, merge exactly the head that was derived, and leave the evidence.
fn finish(ctx: &mut Ctx, v: &Verdict, rec: Record) -> Outcome {
    let n = v.pr.number;
    if let Err(e) = ctx.ledger.append(&rec.to_json("approved")) {
        return Outcome::Failed { pr: n, detail: format!("cannot write the ledger ({e}); not merging without a record"), head: v.pr.head_sha.clone() };
    }
    match ctx.forge.merge(n, &v.pr.head_sha) {
        Ok(()) => {
            let _ = ctx.ledger.append(&rec.to_json("merged"));
            if let Err(e) = ctx.forge.comment(n, &rec.comment()) {
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
            let mut row = rec.to_json("failed");
            row["detail"] = serde_json::json!(e.to_string());
            let _ = ctx.ledger.append(&row);
            Outcome::Failed { pr: n, detail: e.to_string(), head: v.pr.head_sha.clone() }
        }
    }
}

/// Remove the remote branch, the local branch and the worktrees of the merged pull request. Returns the failures.
fn after_merge(ctx: &mut Ctx, v: &Verdict) -> Vec<String> {
    match ctx.git {
        Some(git) => cleanup::cleanup_pr(git, ctx.forge, v.pr.number, false).problems,
        None => Vec::new(),
    }
}

/// Merge several queued pull requests, each re-evaluated just before it merges (the base moved after the last one).
pub fn merge_many(ctx: &mut Ctx, numbers: &[u64], batch: Option<&Attestation>) -> Vec<Outcome> {
    numbers.iter().map(|n| merge_one(ctx, *n, batch)).collect()
}

/// Add a ready, ordinary pull request to the queue. A gate-changing one is labelled for a human instead.
pub fn queue_one(forge: &dyn Forge, n: u64) -> Result<String, String> {
    let v = evaluate(forge, n).map_err(|e| e.to_string())?;
    if !v.ready() {
        return Err(format!("#{n} is not ready to queue: {}", v.refusals.join("; ")));
    }
    if v.gate_changing() {
        let _ = forge.set_label(n, REVIEW_LABEL, true);
        return Err(format!("#{n} changes a gate ({}), so it is not queueable; labelled `{REVIEW_LABEL}`. A human merges it with `delphi merge {n}` at a terminal", v.gate_hits.join(", ")));
    }
    forge.set_label(n, QUEUE_LABEL, true).map_err(|e| e.to_string())?;
    Ok(format!("#{n} queued (head {})", short(&v.pr.head_sha)))
}

/// One line per queued pull request: what `delphi merge --all` would do with it.
pub fn status_lines(forge: &dyn Forge) -> Result<Vec<String>, String> {
    let list = forge.queued(QUEUE_LABEL).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for n in list {
        let v = evaluate(forge, n).map_err(|e| e.to_string())?;
        out.push(if !v.refusals.is_empty() {
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
    Cleanup(Vec<u64>),
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
            "--merged" => merged = true,
            "--yes" => yes = true,
            f if f.starts_with("--") => return Err(format!("unknown option `{f}` (see `delphi help merge`)")),
            w => words.push(w),
        }
    }
    let sub = match (words.first().copied(), all, auto) {
        (_, true, true) => return Err("--all and --auto are different modes; pick one".into()),
        (Some(w), false, false) if matches!(w, "queue" | "status" | "log" | "fix" | "cleanup") => match w {
            "queue" => Sub::Queue(words.get(1).and_then(|n| pr_number(n)).ok_or("usage: delphi merge queue <PR>")?),
            "status" => Sub::Status,
            "log" => Sub::Log,
            "fix" => Sub::Fix(words.get(1).and_then(|n| pr_number(n)).ok_or("usage: delphi merge fix <PR> [--done | --needs-human \"<why>\"]")?),
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
    if sub == Sub::Auto {
        caps.validate()?;
    }
    Ok(Parsed { sub, home, quote, dry_run, open, json, pr, done, needs_human, merged, yes, caps, caps_given })
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
                0
            }
            Err(e) => {
                eprintln!("delphi merge: {e}");
                4
            }
        },
        Sub::Log => log_cmd(&p),
        Sub::Fix(n) => fix_cmd(&p, &forge, n),
        Sub::Cleanup(ref numbers) => cleanup::run(&forge, numbers, p.merged, p.dry_run, p.yes),
        Sub::Auto => auto_cmd(&p, &forge),
        Sub::Merge(ref numbers) => run_merges(&p, &forge, numbers.clone(), false),
        Sub::All => match forge.queued(QUEUE_LABEL) {
            Ok(list) if list.is_empty() => {
                println!("nothing is queued");
                0
            }
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
        "--auto session: up to {} pull request(s), at most {} minute(s), pausing after {} error(s) in a row. It never merges a pull request that changes a gate.",
        p.caps.max_prs,
        p.caps.run_for.as_secs() / 60,
        p.caps.pause_after_errors
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
    let mut ctx = Ctx { forge, ledger: &ledger, errors: &error_log, term: &mut term, quote: None, dry_run: false, git: Some(&git), session: None, settle: Duration::from_secs(3), now: delphi_core::time::now_rfc3339 };
    let stop = || INTERRUPTED.load(std::sync::atomic::Ordering::SeqCst) || stop_file.exists();
    let summary = auto::run(&mut ctx, &p.caps, &att, &auto::SystemClock, &stop, &mut |l| println!("{l}"));
    summary.lines().iter().for_each(|l| println!("{l}"));
    if summary.errors > 0 {
        1
    } else {
        0
    }
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
    let mut ctx = Ctx { forge, ledger: &ledger, errors: &error_log, term: &mut term, quote: p.quote.clone(), dry_run: p.dry_run, git: Some(&git), session: None, settle: Duration::from_secs(3), now: delphi_core::time::now_rfc3339 };
    let outcomes = if batch { merge_many(&mut ctx, &numbers, batch_att.as_ref()) } else { numbers.iter().map(|n| merge_one(&mut ctx, *n, None)).collect() };
    outcomes.iter().for_each(|o| println!("{}", o.line()));
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
        assert_eq!(parse(&a("--home /x merge 5")).unwrap().home.as_deref(), Some("/x"));
        let q = vec!["5".to_string(), "--operator-said".to_string(), "merge #5 now".to_string()];
        assert_eq!(parse(&q).unwrap().quote.as_deref(), Some("merge #5 now"));
    }

    #[test]
    fn nonsense_is_a_usage_error() {
        for bad in ["merge", "merge --all --auto", "merge --auto --max-prs 0", "merge --auto --for 48h", "merge --auto --poll 1s", "merge --max-prs 3 5", "merge --auto --for soon", "merge --all 5", "merge abc", "merge 0", "merge --bogus 5", "merge queue", "merge queue x"] {
            assert!(parse(&a(bad)).is_err(), "{bad}");
        }
        assert!(parse(&["5".to_string(), "--operator-said".to_string()]).is_err());
    }
}
