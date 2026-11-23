//! `delphi merge --auto --batch[=N]`: rollups inside the bounded session (spec 094 §13, slice S22).
//!
//! The session's approval and rules do not change. Only a pull request the session would merge on its own today (queued, not
//! gate-changing, green on its own head) is batched, and the approval a rollup carries is bound to the `(pr, head)` pairs the
//! session chose itself (§13.4 invariant 4). At each poll the session first looks, without waiting, at the rollups it has in
//! flight: a green one merges (S20), a red one is bisected (S21) and its halves are followed at later polls (two halves are decided
//! together once both have finished), one whose checks still run is left for the next poll. A rollup made stale by a move of the
//! base is closed and built again: a half at its round, a first rollup by the next batch. Then, with nothing in flight and two or
//! more pull requests that may be batched, it opens one rollup of up to N of them, never more than the merges left under
//! `--max-prs`. With one, or with room for one, the serial path takes them. While a rollup is in flight, a pull request that could
//! join a batch waits for the next one, so a serial merge does not make the rollup behind. Rollups still open when the session ends
//! stay open; `delphi merge <rollup>` merges one on the operator's approval of its pull requests. This module is on the gate list.

use super::bisect::{open_half, round_of, settle, split, supersede, Tally};
use super::check::{evaluate, Verdict};
use super::record::{Attestation, Ledger};
use super::rollup::{build, choose, parse_manifest, Choice, Constituent, BRANCH_PREFIX};
use super::rollup_pr::{judge, open, recognise, record_and_merge, Approval, Env, Judged, Recognised, NOT_FINISHED, STALE};
use super::{note, Ctx, Outcome};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};

/// The session's batching: its own approval of the pairs it chose, and the rollups it has in flight.
pub(super) struct Batching {
    max: usize,
    approval: Approval,
    /// Rollups this session opened that are still followed, oldest first.
    pub inflight: Vec<u64>,
    /// Pull requests a bisection sent back to the serial path: never batched again in this session.
    serial_only: BTreeSet<u64>,
    /// Every rollup this session opened.
    pub opened: Vec<u64>,
    seq: u32,
}

/// What one poll's batching did, for the session's books.
#[derive(Default)]
pub(super) struct Polled {
    pub merged: Vec<u64>,
    /// Errors, a `rollup-red` entry among them (each counts).
    pub errors: u32,
    pub active: bool,
    /// Parked like any pull request that errored, until its head changes: `(pr, head)`.
    pub parked: Vec<(u64, String)>,
    /// Left alone by the serial path this poll: constituents in flight, and pull requests waiting for the next batch.
    pub hold: BTreeSet<u64>,
    /// How many constituents are in flight (they count against `--max-prs` before they merge).
    pub carried: usize,
}

enum Look {
    /// Checks still running (with an error to count when the rollup could not be read).
    Wait(Option<String>),
    /// Closed, merged, or rebuilt: no longer followed.
    Done,
    /// Refused for a reason a person must look at: no longer followed.
    Error(String),
    Judged(Box<Recognised>, Judged),
}

impl Batching {
    pub fn new(max: usize, att: &Attestation) -> Self {
        Batching { max, approval: Approval { att: att.clone(), pairs: Vec::new() }, inflight: Vec::new(), serial_only: BTreeSet::new(), opened: Vec::new(), seq: 0 }
    }

    /// Constituents of the rollups in flight, as their opening rows name them.
    fn carried(&self, ledger: &Ledger) -> BTreeSet<u64> {
        let rows = ledger.read();
        let opening = rows.iter().filter(|r| r["kind"] == "rollup_opened" && r["pr"].as_u64().is_some_and(|n| self.inflight.contains(&n)));
        opening.flat_map(|r| r["constituents"].as_array().cloned().unwrap_or_default()).filter_map(|c| c["pr"].as_u64()).collect()
    }

    /// The head the session approved last for `pr`.
    fn head_of(&self, pr: u64) -> String {
        self.approval.pairs.iter().rev().find(|p| p.0 == pr).map(|p| p.1.clone()).unwrap_or_default()
    }

    /// One poll: follow the rollups in flight, then open a new one if there is room and work. `room` is the merges left.
    pub fn poll(&mut self, ctx: &mut Ctx, queue: &[u64], parked: &HashMap<u64, String>, room: u32, say: &mut dyn FnMut(&str)) -> Polled {
        let mut out = Polled::default();
        let Some(env) = ctx.rollup else { return out };
        let before = ctx.ledger.read().len();
        let mut t = Tally::default();
        self.advance(ctx, env, &mut out, &mut t, say);
        self.absorb(t, &mut out);
        let room = (room as usize).saturating_sub(out.merged.len());
        self.start(ctx, env, queue, parked, room, &mut out, say);
        // Every rollup this poll opened (a batch, or halves) is tagged with the session, for `merge status`.
        for r in ctx.ledger.read().iter().skip(before).filter(|r| r["kind"] == "rollup_opened") {
            if let Some(n) = r["pr"].as_u64() {
                self.opened.push(n);
                let _ = ctx.ledger.append(&json!({"kind": "rollup_session", "pr": n, "session": ctx.session, "time": (ctx.now)()}));
            }
        }
        let carried = self.carried(ctx.ledger);
        out.carried = carried.len();
        out.hold.extend(carried);
        out
    }

    fn absorb(&mut self, t: Tally, out: &mut Polled) {
        out.merged.extend(&t.merged);
        for k in &t.isolated {
            out.parked.push((*k, self.head_of(*k)));
        }
        out.errors += (t.isolated.len() + t.red_apart.len()) as u32;
        self.serial_only.extend(&t.serial);
        for h in t.open {
            if !self.inflight.contains(&h) {
                self.inflight.push(h);
            }
        }
    }

    /// Look at each rollup in flight; merge the green, bisect the red, decide two halves together once both have finished.
    fn advance(&mut self, ctx: &mut Ctx, env: Env, out: &mut Polled, t: &mut Tally, say: &mut dyn FnMut(&str)) {
        let rows = ctx.ledger.read();
        let parent = |n: u64| rows.iter().rev().find(|r| r["kind"] == "rollup_half" && r["pr"] == n).and_then(|r| r["parent"].as_u64());
        let (mut waiting, mut judged) = (Vec::new(), Vec::new());
        for n in std::mem::take(&mut self.inflight) {
            match self.look(ctx, env, n, t, say) {
                Look::Wait(e) => {
                    if let Some(e) = e {
                        say(&e);
                        out.errors += 1;
                    }
                    waiting.push(n);
                }
                Look::Done => out.active = true,
                Look::Error(line) => {
                    say(&line);
                    out.errors += 1;
                    out.active = true;
                }
                Look::Judged(r, j) => judged.push((n, *r, j)),
            }
        }
        while !judged.is_empty() {
            let (n, r, j) = judged.remove(0);
            let p = parent(n);
            if let Some(i) = p.and_then(|p| judged.iter().position(|(m, _, _)| parent(*m) == Some(p))) {
                let other = judged.remove(i);
                let parts: Vec<(u64, String)> = r.manifest.prs.iter().chain(&other.1.manifest.prs).map(|(k, h, _)| (*k, h.clone())).collect();
                out.active = true;
                settle(ctx, env, p.unwrap_or(n), &parts, vec![(n, r, j), other], &self.approval, t);
            } else if p.is_some_and(|p| waiting.iter().any(|m| parent(*m) == Some(p))) {
                // Its sibling's checks still run: decided together at a later poll (the "red together" test of §13.5).
                waiting.push(n);
            } else {
                out.active = true;
                let head = r.verdict.pr.head_sha.clone();
                match j {
                    Judged::Green => match record_and_merge(ctx, env, &r, n, &head, &self.approval) {
                        Outcome::Merged { .. } => t.merged.extend(r.parts.iter().map(|p| p.pr.number)),
                        other => {
                            say(&other.line());
                            note(ctx, &other);
                            out.errors += 1;
                        }
                    },
                    _ => split(ctx, env, &r, n, &self.approval, t),
                }
            }
        }
        self.inflight = waiting;
    }

    /// Read one rollup in flight and judge it without waiting.
    fn look(&mut self, ctx: &mut Ctx, env: Env, n: u64, t: &mut Tally, say: &mut dyn FnMut(&str)) -> Look {
        let v = match evaluate(ctx.forge, n) {
            Ok(v) => v,
            Err(e) => return Look::Wait(Some(format!("rollup #{n} could not be read: {e}"))),
        };
        if v.pr.state != "OPEN" {
            say(&format!("rollup #{n} is {} now, not by this session; it is no longer followed", v.pr.state.to_lowercase()));
            return Look::Done;
        }
        let refused = |ctx: &mut Ctx, reasons: Vec<String>| {
            note(ctx, &Outcome::Refused { pr: n, reasons: reasons.clone(), head: v.pr.head_sha.clone() });
            Look::Error(format!("rollup #{n}: not merged, because {}; it is left open for a person", reasons.join("; ")))
        };
        let r = match recognise(ctx.forge, env, &v) {
            Ok(r) => r,
            Err(why) if why.starts_with(STALE) => return self.rebuild(ctx, env, n, &v, &why, t, say),
            Err(why) => return refused(ctx, vec![why]),
        };
        match judge(ctx, &r, n, &v.pr.head_sha, &self.approval) {
            Judged::Refused(rs) if rs.iter().all(|x| x.starts_with(NOT_FINISHED)) => Look::Wait(None),
            Judged::Refused(rs) if rs.iter().any(|x| x.starts_with(STALE)) => self.rebuild(ctx, env, n, &v, &rs.join("; "), t, say),
            Judged::Refused(rs) => refused(ctx, rs),
            j => Look::Judged(Box::new(r), j),
        }
    }

    /// A stale rollup (its base or a pull request in it moved): closed, then a half is built again at its round from the tip;
    /// the pull requests of a first rollup go back to the queue and into the next batch. Nothing is broken, so no error.
    #[allow(clippy::too_many_arguments)]
    fn rebuild(&mut self, ctx: &mut Ctx, env: Env, n: u64, v: &Verdict, why: &str, t: &mut Tally, say: &mut dyn FnMut(&str)) -> Look {
        supersede(ctx, n, &format!("{why}; the `--auto` session builds it again on the tip"));
        let round = round_of(ctx.ledger, n);
        let parent = ctx.ledger.read().iter().rev().find(|r| r["kind"] == "rollup_half" && r["pr"] == n).and_then(|r| r["parent"].as_u64());
        match (parent, parse_manifest(&v.pr.body)) {
            (Some(p), Ok(m)) if round >= 1 => {
                let half: Vec<(u64, String)> = m.prs.iter().map(|(k, h, _)| (*k, h.clone())).collect();
                let id = format!("{}-r", v.pr.head_ref.strip_prefix(BRANCH_PREFIX).unwrap_or("rollup"));
                if let Some(h) = open_half(ctx, env, &self.approval, p, round, &id, &half, t) {
                    t.open.push(h);
                }
            }
            _ => say(&format!("rollup #{n} was stale ({why}); its pull requests go into the next batch")),
        }
        Look::Done
    }

    /// With nothing in flight and two or more pull requests that may be batched, open one rollup of up to `max` (and `room`).
    #[allow(clippy::too_many_arguments)]
    fn start(&mut self, ctx: &mut Ctx, env: Env, queue: &[u64], parked: &HashMap<u64, String>, room: usize, out: &mut Polled, say: &mut dyn FnMut(&str)) {
        let carried = self.carried(ctx.ledger);
        let parked_now = |n: u64| parked.get(&n).is_some_and(|h| ctx.forge.pr(n).is_ok_and(|p| &p.head_sha == h));
        let candidates: Vec<u64> = queue.iter().copied().filter(|n| !carried.contains(n) && !self.serial_only.contains(n) && !parked_now(*n)).collect();
        if candidates.len() < 2 {
            return;
        }
        // The same rule as `--all --batch` (§13.2 step 1), over the session's own queue; a rollup is never a constituent.
        let all = choose(ctx.forge, &candidates, candidates.len());
        let eligible: Vec<Constituent> = all.chosen.into_iter().filter(|c| !c.head_ref.starts_with(BRANCH_PREFIX)).collect();
        let size = self.max.min(room);
        if !self.inflight.is_empty() {
            out.hold.extend(eligible.iter().map(|c| c.pr));
            return;
        }
        if eligible.len() < 2 || size < 2 {
            return;
        }
        // Those beyond this batch wait for the next one too: merged serially now, they would make it behind at once.
        let waiting: Vec<u64> = eligible.iter().map(|c| c.pr).collect();
        let pick = Choice { base_ref: all.base_ref, chosen: eligible.into_iter().take(size).collect(), skipped: Vec::new() };
        self.seq += 1;
        let id = format!("{}-{}", ctx.session.as_deref().unwrap_or("auto"), self.seq);
        self.approval.pairs.extend(pick.chosen.iter().map(|c| (c.pr, c.head_sha.clone())));
        out.active = true;
        let c = match build(ctx.forge, env.git, env.home, &pick, &id) {
            Ok(c) => c,
            Err(e) => {
                say(&format!("no rollup ({e}); this poll takes them one at a time"));
                out.errors += 1;
                return;
            }
        };
        out.hold.extend(waiting);
        // A constituent the build refused is an error (`open` writes its entry); one that moved comes back in the next batch.
        for (k, why) in c.dropped.iter().filter(|d| !d.1.contains("its branch is at")) {
            say(&format!("#{k}: left out of the rollup: {why}"));
            out.errors += 1;
            out.parked.push((*k, self.head_of(*k)));
        }
        match open(ctx.forge, env, ctx.ledger, ctx.errors, &c, &self.approval, &(ctx.now)()) {
            Ok(n) => {
                let list: Vec<String> = c.merged.iter().map(|(k, _)| format!("#{}", k.pr)).collect();
                say(&format!("opened rollup #{n} of {}; it merges at a later poll once its CI is green", list.join(", ")));
                self.inflight.push(n);
            }
            Err(e) => {
                say(&format!("no rollup: {e}"));
                out.errors += 1;
            }
        }
    }
}

/// `merge status`: the batches of the latest session that opened any, in one line.
pub fn status_lines(ledger: &Ledger) -> Vec<String> {
    let rows = ledger.read();
    let Some(id) = rows.iter().rev().find(|r| r["kind"] == "rollup_session").and_then(|r| r["session"].as_str()) else { return Vec::new() };
    let mine: Vec<u64> = rows.iter().filter(|r| r["kind"] == "rollup_session" && r["session"] == id).filter_map(|r| r["pr"].as_u64()).collect();
    let of = |kind: &str| -> Vec<&serde_json::Value> { rows.iter().filter(|r| r["kind"] == kind && r["pr"].as_u64().is_some_and(|n| mine.contains(&n))).collect() };
    let carried: usize = of("rollup_merged").iter().map(|r| r["constituents"].as_array().map_or(0, Vec::len)).sum();
    let state = match rows.iter().rev().find(|r| r["kind"] == "session_end" && r["session"] == id) {
        Some(r) => format!("ended: {}", r["reason"].as_str().unwrap_or("?")),
        None => "running".into(),
    };
    vec![format!("session {id} ({state}): {} rollup(s) opened, {} merged carrying {carried} pull request(s), {} bisected", mine.len(), of("rollup_merged").len(), of("rollup_bisected").len())]
}
