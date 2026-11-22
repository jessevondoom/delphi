//! A red rollup (spec 094 §13.5, slice S21): bisection in halves for at most two rounds, then the serial path.
//!
//! CI already retries a failing test once (spec 125), so a rollup whose `ci-gate` failed is bisected at once (OD-19, as the
//! operator decided). The red rollup is closed and its branch removed; each half is built from the base tip by the builder of
//! S18, carrying the operator's approval of exactly those `(pr, head)` pairs (§13.4 invariant 4), and opened as in S19. Both
//! halves' checks are awaited; a green half merges the S20 way, a red one is split again on the new tip, and both green is "red
//! together, green apart". A constituent red on its own is `rollup-red`, for `merge fix`; what two rounds do not isolate goes back
//! to the serial path. A red rollup is never merged. This module is on the gate list.

use super::check::evaluate;
use super::cleanup::cleanup_closed_rollup;
use super::errors::Class;
use super::forge::Forge;
use super::record::Ledger;
use super::rollup::{self, BRANCH_PREFIX};
use super::rollup_pr::{judge, open, recognise, record_and_merge, Approval, Env, Judged, Recognised, RED, STALE};
use super::{put_error, short, Ctx, Outcome};
use serde_json::json;

/// Rounds of halving before the serial path (OD-18).
pub const MAX_ROUNDS: u64 = 2;

/// What a bisection did with each constituent.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Tally {
    pub merged: Vec<u64>,
    /// Red on its own: `rollup-red`, handed to `merge fix`.
    pub isolated: Vec<u64>,
    /// Back to the serial path: not isolated after two rounds, red together and green apart, or left out of a half.
    pub serial: Vec<u64>,
    /// Halves left open because their checks did not finish (or something else stopped them) within the wait.
    pub open: Vec<u64>,
    /// Red together and green apart: a `rollup-red` entry each (they are in `serial` too).
    pub red_apart: Vec<u64>,
}

/// Bisect the red rollup `n` (recognised as `r`, approved as `approval`). The outcome stays a refusal of `n`: it was not merged.
pub(super) fn bisect(ctx: &mut Ctx, env: Env, r: &Recognised, n: u64, approval: &Approval) -> Outcome {
    let mut t = Tally::default();
    split(ctx, env, r, n, approval, &mut t);
    let list = |v: &[u64]| if v.is_empty() { "none".to_string() } else { v.iter().map(|k| format!("#{k}")).collect::<Vec<_>>().join(", ") };
    let line = format!(
        "its `ci-gate` failed, so it was bisected (spec 094 §13.5): merged {}; red on its own ({}, for `merge fix`) {}; back to the serial path {}; rollups left open {}",
        list(&t.merged),
        Class::RollupRed.slug(),
        list(&t.isolated),
        list(&t.serial),
        list(&t.open)
    );
    Outcome::Refused { pr: n, reasons: vec![line], head: r.verdict.pr.head_sha.clone() }
}

/// The round a rollup belongs to: 0 for one a batch opened, the recorded round for a half (read from the ledger, not the body).
pub fn round_of(ledger: &Ledger, n: u64) -> u64 {
    ledger.read().iter().rev().find(|r| r["kind"] == "rollup_half" && r["pr"] == n).and_then(|r| r["round"].as_u64()).unwrap_or(0)
}

/// Close the red rollup `n`, then build, open, await and settle its two halves.
pub(super) fn split(ctx: &mut Ctx, env: Env, r: &Recognised, n: u64, approval: &Approval, t: &mut Tally) {
    let parts: Vec<(u64, String)> = r.manifest.prs.iter().map(|(k, h, _)| (*k, h.clone())).collect();
    // Invariant 4: only halves of the approved set. A pair the operator did not approve in this run stops the bisection.
    if let Some((k, h)) = parts.iter().find(|(k, h)| !approval.covers(*k, h)) {
        ctx.term.show(&format!("rollup #{n}: not bisected: {STALE}#{k} at `{}` is not a head the operator approved in this run; a new batch needs the operator", short(h)));
        t.open.push(n);
        return;
    }
    let round = round_of(ctx.ledger, n);
    let head = r.verdict.pr.head_sha.clone();
    if parts.len() == 1 {
        supersede(ctx, n, "its checks failed (`ci-gate`), and it carries one pull request, which is therefore red on its own");
        let (k, h) = &parts[0];
        put_error(ctx.errors, &(ctx.now)(), *k, h, Class::RollupRed, &format!("{RED}#{k} is red on its own in rollup #{n} (head `{}`); `delphi merge fix {k}` takes it", short(&head)));
        ctx.term.show(&format!("#{k}: red on its own in rollup #{n}; an entry of class `rollup-red` was written for `merge fix`"));
        t.isolated.push(*k);
        return;
    }
    if round >= MAX_ROUNDS {
        supersede(ctx, n, &format!("its checks failed (`ci-gate`) after {MAX_ROUNDS} rounds of halving; its pull requests go back to the serial path"));
        serial(ctx, n, &parts, &format!("rollup #{n} was still red after {MAX_ROUNDS} rounds of halving"), t);
        return;
    }
    let mid = parts.len().div_ceil(2);
    let id = r.verdict.pr.head_ref.strip_prefix(BRANCH_PREFIX).unwrap_or("rollup").to_string();
    let halves: Vec<Option<u64>> = [(&parts[..mid], "a"), (&parts[mid..], "b")].into_iter().map(|(half, s)| open_half(ctx, env, approval, n, round + 1, &format!("{id}-{s}"), half, t)).collect();
    let opened: Vec<u64> = halves.iter().flatten().copied().collect();
    let _ = ctx.ledger.append(&json!({"kind": "rollup_bisected", "pr": n, "head_sha": head, "round": round + 1, "halves": opened, "time": (ctx.now)()}));
    let halves: Vec<String> = opened.iter().map(|h| format!("#{h}")).collect();
    supersede(ctx, n, &format!("its checks failed (`ci-gate`), so its pull requests were split into {}", if halves.is_empty() { "no new rollup".to_string() } else { halves.join(" and ") }));
    // Wait for both halves before deciding anything: both green means red together, green apart.
    let judged: Vec<(u64, Recognised, Judged)> = opened.iter().filter_map(|h| judge_half(ctx, env, *h, approval, t)).collect();
    settle(ctx, env, n, &parts, judged, approval, t);
}

/// Decide on the judged halves of the red rollup `n`, which carried `parts`: one not yet decided leaves both open; both green is
/// red together, green apart; otherwise a green half merges and a red one is split again. `--auto --batch` calls it too, at the
/// poll where both halves' checks have finished (S22).
pub(super) fn settle(ctx: &mut Ctx, env: Env, n: u64, parts: &[(u64, String)], judged: Vec<(u64, Recognised, Judged)>, approval: &Approval, t: &mut Tally) {
    let list: Vec<String> = judged.iter().map(|(h, _, _)| format!("#{h}")).collect();
    if judged.iter().any(|(_, _, j)| matches!(j, Judged::Refused(_))) {
        for (h, _, j) in &judged {
            let why = if let Judged::Refused(why) = j { why.join("; ") } else { "its sibling has not been judged".into() };
            ctx.term.show(&format!("#{h}: rollup left open: {why}; `delphi merge {h}` judges it again on the operator's approval of its pull requests"));
            t.open.push(*h);
        }
        return;
    }
    let green = |j: &Judged| matches!(j, Judged::Green);
    if judged.len() == 2 && judged.iter().all(|(_, _, j)| green(j)) {
        for (h, _, _) in &judged {
            supersede(ctx, *h, &format!("it is green, and so is its sibling, while rollup #{n} of both was red: red together, green apart; its pull requests go back to the serial path"));
        }
        let now = (ctx.now)();
        for (k, h) in parts {
            put_error(ctx.errors, &now, *k, h, Class::RollupRed, &format!("{RED}red together, green apart: rollup #{n} was red, its halves {} were green", list.join(" and ")));
            t.red_apart.push(*k);
        }
        serial(ctx, n, parts,&format!("red together, green apart (rollup #{n})"), t);
        return;
    }
    // A green half merges first (nothing has landed since both were built on the same tip); a red one is split on the new tip.
    for (h, rr, _) in judged.iter().filter(|(_, _, j)| green(j)) {
        let head = rr.verdict.pr.head_sha.clone();
        match record_and_merge(ctx, env, rr, *h, &head, approval) {
            Outcome::Merged { .. } => t.merged.extend(rr.parts.iter().map(|p| p.pr.number)),
            other => {
                ctx.term.show(&other.line());
                t.open.push(*h);
            }
        }
    }
    for (h, rr, _) in judged.iter().filter(|(_, _, j)| matches!(j, Judged::Red(_))) {
        split(ctx, env, rr, *h, approval, t);
    }
}

/// Build and open one half from the current base tip; its constituents must still be at their approved heads and still batchable.
/// What cannot go into the half goes back to the serial path. Returns the half's number.
#[allow(clippy::too_many_arguments)]
pub(super) fn open_half(ctx: &mut Ctx, env: Env, approval: &Approval, parent: u64, round: u64, id: &str, half: &[(u64, String)], t: &mut Tally) -> Option<u64> {
    let numbers: Vec<u64> = half.iter().map(|p| p.0).collect();
    let mut choice = rollup::choose(ctx.forge, &numbers, rollup::MAX_BATCH);
    let (keep, moved): (Vec<_>, Vec<_>) = choice.chosen.into_iter().partition(|c| approval.covers(c.pr, &c.head_sha));
    choice.chosen = keep;
    choice.skipped.extend(moved.into_iter().map(|c| (c.pr, format!("{STALE}its head is `{}` now, not the one the operator approved", short(&c.head_sha)))));
    for (k, why) in &choice.skipped {
        ctx.term.show(&format!("#{k}: left out of the half of rollup #{parent}, back to the serial path: {why}"));
        t.serial.push(*k);
    }
    if choice.chosen.is_empty() {
        return None;
    }
    let to_serial = |ctx: &mut Ctx, t: &mut Tally, why: String| {
        ctx.term.show(&format!("a half of rollup #{parent} was not opened ({why}); {} go back to the serial path", numbers.iter().map(|k| format!("#{k}")).collect::<Vec<_>>().join(", ")));
        t.serial.extend(choice.chosen.iter().map(|c| c.pr));
    };
    let c = match rollup::build(ctx.forge, env.git, env.home, &choice, id) {
        Ok(c) => c,
        Err(e) => {
            to_serial(ctx, t, e);
            return None;
        }
    };
    t.serial.extend(c.dropped.iter().map(|d| d.0));
    match open(ctx.forge, env, ctx.ledger, ctx.errors, &c, approval, &(ctx.now)()) {
        Ok(h) => {
            let prs: Vec<u64> = c.merged.iter().map(|(k, _)| k.pr).collect();
            let _ = ctx.ledger.append(&json!({"kind": "rollup_half", "pr": h, "parent": parent, "round": round, "constituents": prs, "time": (ctx.now)()}));
            ctx.term.show(&format!("opened rollup #{h}, a half of #{parent} (round {round} of {MAX_ROUNDS}): {}", prs.iter().map(|k| format!("#{k}")).collect::<Vec<_>>().join(", ")));
            Some(h)
        }
        Err(e) => {
            to_serial(ctx, t, e);
            None
        }
    }
}

/// Recognise an opened half and wait for its checks.
fn judge_half(ctx: &mut Ctx, env: Env, h: u64, approval: &Approval, t: &mut Tally) -> Option<(u64, Recognised, Judged)> {
    let recognised = evaluate(ctx.forge, h).map_err(|e| e.to_string()).and_then(|v| recognise(ctx.forge, env, &v));
    match recognised {
        Ok(r) => {
            let head = r.verdict.pr.head_sha.clone();
            let j = judge(ctx, &r, h, &head, approval);
            Some((h, r, j))
        }
        Err(why) => {
            ctx.term.show(&format!("#{h}: the half was opened but is not recognised as a rollup ({why}); it is left open for a person"));
            t.open.push(h);
            None
        }
    }
}

/// Close a rollup that will not merge, with a comment saying why, and remove its `delphi-rollup/*` branch (§13.5).
pub(super) fn supersede(ctx: &mut Ctx, n: u64, why: &str) {
    let now = (ctx.now)();
    let comment = format!("Closed by `delphi merge` (spec 094 §13.5): {why}. A red rollup is never merged; each pull request in it keeps its own approval and is not changed.");
    if let Err(e) = ctx.forge.close(n, &comment) {
        put_error(ctx.errors, &now, n, "", Class::Cleanup, &format!("rollup #{n} could not be closed: {e}"));
        return;
    }
    let _ = ctx.ledger.append(&json!({"kind": "rollup_closed", "pr": n, "reason": why, "time": now}));
    if let Some(git) = ctx.git {
        for problem in cleanup_closed_rollup(git, ctx.forge, n).problems {
            put_error(ctx.errors, &now, n, "", Class::Cleanup, &problem);
        }
    }
}

/// Send `parts` back to the serial path: they stay queued, and `delphi merge --all` without `--batch` takes them one by one.
fn serial(ctx: &mut Ctx, n: u64, parts: &[(u64, String)], why: &str, t: &mut Tally) {
    let _ = ctx.ledger.append(&json!({"kind": "rollup_serial", "pr": n, "constituents": parts.iter().map(|p| p.0).collect::<Vec<_>>(), "reason": why, "time": (ctx.now)()}));
    for (k, _) in parts {
        ctx.term.show(&format!("#{k}: back to the serial path ({why}); it stays queued, and `delphi merge --all` without `--batch` takes it"));
        t.serial.push(*k);
    }
}

/// `merge status`: every rollup the ledger knows that is still open, with its round and parent when it is a half.
pub fn status_lines(forge: &dyn Forge, ledger: &Ledger) -> Vec<String> {
    let rows = ledger.read();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for r in rows.iter().filter(|r| r["kind"] == "rollup_opened") {
        let Some(n) = r["pr"].as_u64() else { continue };
        if seen.contains(&n) || !forge.pr(n).is_ok_and(|p| p.state == "OPEN") {
            continue;
        }
        seen.push(n);
        let prs: Vec<String> = r["constituents"].as_array().into_iter().flatten().filter_map(|c| c["pr"].as_u64()).map(|k| format!("#{k}")).collect();
        let half = rows.iter().rev().find(|x| x["kind"] == "rollup_half" && x["pr"] == n);
        let what = match half {
            Some(x) => format!("bisecting: round {} of {MAX_ROUNDS}, a half of #{}", x["round"], x["parent"]),
            None => "open".into(),
        };
        let what = match rows.iter().find(|x| x["kind"] == "rollup_session" && x["pr"] == n).and_then(|x| x["session"].as_str()) {
            Some(s) => format!("{what}, opened by `--auto` session {s}"),
            None => what,
        };
        out.push(format!("rollup #{n} of {}: {what}; `delphi merge {n}` merges it once green, on the operator's approval of its pull requests", prs.join(", ")));
    }
    out
}
