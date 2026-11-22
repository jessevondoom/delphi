//! The ratchet and the grade suggestions (`specs/056` §3.2, §4; slice M4b), from retrospective results.
//!
//! A finished build task is a retrospective row: the class its first attempt ran at, whether it passed first time (gates green, no
//! retry, nothing denied or reverted: [`crate::learn::TaskRecord::first_pass`]) and the tokens it took. The row goes to the evidence
//! file ([`crate::assign::record_evidence`]) and, for a plan-following role under discovered assignment, moves the ratchet: after `k`
//! first-pass successes in a row the next build tries the class below the preferred one, never below the role's floor; one failure
//! puts it back; two failures in a row on a class stop trying that class for the kind until the operator clears it.
//!
//! The state is local (`local/model-ratchet.json`), read by routing only and never put in a prompt (`specs/056` §10.4). It can only
//! lower a request between the role's preferred class and its floor, so even a forged file cannot serve a role below its floor.
//! A suggestion is text for the operator; nothing here changes a grade.

use crate::assign::{level, EvidenceRow, Tokens};
use crate::learn::Outcome;
use crate::models::Class;
use delphi_core::event::EventEnvelope;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE: &str = "local/model-ratchet.json";
/// The only task kind a dispatch carries today: a build under the retry ladder, which has a plan and deterministic gates.
pub const BUILD: &str = "build";

/// The role's bounds, in class levels ([`level`]): the floor, the preferred class, and the ratchet's `k`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub floor: i32,
    pub preferred: i32,
    pub k: u32,
}

/// One `(role, kind)`'s ratchet.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Entry {
    pub role: String,
    pub kind: String,
    /// Classes below the preferred one the next dispatch tries: 0 or less.
    pub step: i32,
    /// First-pass successes in a row since the last move or failure.
    pub streak: u32,
    /// Failures in a row at a class level.
    #[serde(default)]
    pub fails: BTreeMap<i32, u32>,
    /// Class levels not tried again for this kind until the operator clears them.
    #[serde(default)]
    pub stopped: Vec<i32>,
    /// `[first-pass, runs]` at each class level, for the suggestions.
    #[serde(default)]
    pub tally: BTreeMap<i32, [u32; 2]>,
}

/// What a result did to the ratchet.
#[derive(Debug, Clone, PartialEq)]
pub enum Move {
    Down { from: i32, to: i32 },
    Up { from: i32, to: i32 },
    Stopped { at: i32 },
}

impl Move {
    /// The `model.ratchet` payload for this move.
    pub fn payload(&self, role: &str, kind: &str, task_id: &str) -> serde_json::Value {
        let (mv, from, to, reason) = match self {
            Move::Down { from, to } => ("down", *from, *to, "first-pass successes in a row: the next build tries the class below"),
            Move::Up { from, to } => ("up", *from, *to, "a build at the lowered class failed: back to the class above"),
            Move::Stopped { at } => ("stopped", *at, *at, "two failures in a row at this class: not tried again for this kind until the operator clears it"),
        };
        serde_json::json!({"role": role, "kind": kind, "task_id": task_id, "move": mv, "from": name(from), "to": name(to), "reason": reason})
    }
}

/// A level's class name, for people.
pub fn name(l: i32) -> &'static str {
    match l {
        i32::MIN..=0 => "quick",
        1 => "steady",
        _ => "deep",
    }
}

impl Entry {
    /// Apply one result at class level `at`.
    pub fn apply(&mut self, at: i32, first_pass: bool, b: Bounds) -> Vec<Move> {
        let t = self.tally.entry(at).or_insert([0, 0]);
        t[1] += 1;
        t[0] += first_pass as u32;
        let mut moves = Vec::new();
        let here = b.preferred + self.step;
        if first_pass {
            self.fails.remove(&at);
            self.streak = self.streak.saturating_add(1);
            let below = here - 1;
            if self.streak >= b.k && below >= b.floor && !self.stopped.contains(&below) {
                self.step -= 1;
                self.streak = 0;
                moves.push(Move::Down { from: here, to: below });
            }
        } else {
            self.streak = 0;
            let n = self.fails.entry(at).or_insert(0);
            *n += 1;
            // Only a class below the preferred one is a trial; the preferred class itself is never stopped.
            if *n >= 2 && at < b.preferred && !self.stopped.contains(&at) {
                self.stopped.push(at);
                moves.push(Move::Stopped { at });
            }
            if self.step < 0 {
                self.step += 1;
                moves.push(Move::Up { from: here, to: here + 1 });
            }
        }
        moves
    }

    /// The step the next dispatch takes: between the floor and the preferred class, and never at a stopped class.
    pub fn wanted(&self, b: Bounds) -> i32 {
        let mut s = self.step.clamp((b.floor - b.preferred).min(0), 0);
        while s < 0 && self.stopped.contains(&(b.preferred + s)) {
            s += 1;
        }
        s
    }
}

pub fn load(ws_dir: &Path) -> Vec<Entry> {
    let text = std::fs::read_to_string(ws_dir.join(FILE)).unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| serde_json::from_value(v["entries"].clone()).ok()).unwrap_or_default()
}

fn save(ws_dir: &Path, entries: &[Entry]) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(&serde_json::json!({"version": 1, "entries": entries})).unwrap_or_default();
    delphi_core::lock::write_atomic(&ws_dir.join(FILE), text.as_bytes())
}

/// The ratchet's step for the next `(role, kind)` dispatch; 0 when nothing is recorded.
pub fn step_for(ws_dir: &Path, role: &str, kind: &str, b: Bounds) -> i32 {
    load(ws_dir).iter().find(|e| e.role == role && e.kind == kind).map(|e| e.wanted(b)).unwrap_or(0)
}

/// Record one result and save; returns the moves it made.
pub fn record(ws_dir: &Path, role: &str, kind: &str, at: i32, first_pass: bool, b: Bounds) -> std::io::Result<Vec<Move>> {
    let mut all = load(ws_dir);
    let i = all.iter().position(|e| e.role == role && e.kind == kind).unwrap_or_else(|| {
        all.push(Entry { role: role.into(), kind: kind.into(), ..Default::default() });
        all.len() - 1
    });
    let moves = all[i].apply(at, first_pass, b);
    save(ws_dir, &all)?;
    Ok(moves)
}

/// The operator clears the stopped classes (and the failure counts) of `(role, kind)`; returns the classes cleared.
pub fn clear(ws_dir: &Path, role: &str, kind: &str) -> std::io::Result<Vec<i32>> {
    let mut all = load(ws_dir);
    let Some(e) = all.iter_mut().find(|e| e.role == role && e.kind == kind) else { return Ok(vec![]) };
    let cleared = std::mem::take(&mut e.stopped);
    e.fails.clear();
    save(ws_dir, &all)?;
    Ok(cleared)
}

/// One finished task as a retrospective row for `role`.
#[derive(Debug, Clone, PartialEq)]
pub struct Retro {
    pub first_pass: bool,
    /// The pair that served the first attempt; the row is counted for it.
    pub endpoint_id: String,
    pub model_id: String,
    /// The class its `model.assigned` named; `None` under static assignment.
    pub class: Option<Class>,
    /// Every pair the role used in the task, with its tokens: a retry is part of the cost of the first pick (`specs/056` §1 item 4).
    pub spent: Vec<(String, String, Tokens)>,
}

/// Fold a task's events into its retrospective row for `role`; `None` for a task that is not finished, was cancelled, never built,
/// or names no model for the role.
pub fn retro(events: &[EventEnvelope], task_id: &str, role: &str) -> Option<Retro> {
    let rec = crate::learn::task_records(events).into_iter().find(|t| t.task_id == task_id)?;
    if rec.attempts == 0 || matches!(rec.outcome, Outcome::Open | Outcome::Cancelled) {
        return None;
    }
    let mine = || events.iter().filter(|e| e.task_id.as_deref() == Some(task_id) && e.agent.as_deref() == Some(role));
    let mut spent: Vec<(String, String, Tokens)> = Vec::new();
    for e in mine().filter(|e| e.kind == "agent.usage" && e.payload["outcome"].as_str().is_none_or(|o| o == "ok")) {
        let (Some(ep), Some(m)) = (e.endpoint_id.clone(), e.model_id.clone()) else { continue };
        let n = |k: &str| e.payload[k].as_u64().unwrap_or(0);
        let t = Tokens { input: n("input_tokens"), cache_read: n("cache_read_tokens"), cache_write: n("cache_write_tokens"), output: n("output_tokens") };
        match spent.iter_mut().find(|(x, y, _)| x == &ep && y == &m) {
            Some((_, _, s)) => *s = Tokens { input: s.input + t.input, cache_read: s.cache_read + t.cache_read, cache_write: s.cache_write + t.cache_write, output: s.output + t.output },
            None => spent.push((ep, m, t)),
        }
    }
    let assigned = mine().find(|e| e.kind == "model.assigned");
    let (endpoint_id, model_id) = match assigned {
        Some(a) => (a.payload["endpoint_id"].as_str()?.to_string(), a.payload["model_id"].as_str()?.to_string()),
        None => spent.first().map(|(e, m, _)| (e.clone(), m.clone()))?,
    };
    let class = assigned.and_then(|a| a.payload["class"].as_str()).and_then(Class::parse);
    Some(Retro { first_pass: rec.first_pass(), endpoint_id, model_id, class, spent })
}

impl Retro {
    /// The evidence row: one run for the first attempt's pair, its cost every pair's tokens times that pair's multiplier, already
    /// weighted (so the row carries no multiplier of its own).
    pub fn row(&self, role: &str, kind: &str, multiplier: impl Fn(&str, &str) -> Option<f64>) -> EvidenceRow {
        let cost: u64 = self.spent.iter().map(|(e, m, t)| t.cost(multiplier(e, m))).sum();
        EvidenceRow { role: role.into(), kind: kind.into(), endpoint_id: self.endpoint_id.clone(), model_id: self.model_id.clone(), accepted: self.first_pass, tokens: Tokens { input: cost, ..Default::default() }, multiplier: None }
    }
}

/// Suggestions from the tallies (`specs/056` §4): text for the operator, as counts. A class where every one of at least `enough`
/// results passed first time suggests -1 (when a class below meets the floor); one where fewer than half did suggests +1.
pub fn suggestions(entries: &[Entry], enough: u32, floor_of: impl Fn(&str) -> i32) -> Vec<String> {
    let mut out = Vec::new();
    for e in entries {
        for (at, [pass, runs]) in &e.tally {
            if *runs < enough.max(1) {
                continue;
            }
            let try_ = if pass == runs && *at > floor_of(&e.role) {
                "try -1"
            } else if pass * 2 < *runs {
                "try +1"
            } else {
                continue;
            };
            out.push(format!("{}, {} tasks: {} of {} first-pass at {}; {} (`delphi agent {} {}`)", e.role, e.kind, pass, runs, name(*at), try_, if try_ == "try -1" { "nerf" } else { "promote" }, e.role));
        }
    }
    out
}

/// The bounds of `role` from the class rules, and whether it follows plans (only those ratchet).
pub fn bounds(rules: &crate::models::ClassRules, role: &str) -> (Bounds, bool) {
    let a = crate::assign::Assign::build(rules, role, &[], 0);
    (Bounds { floor: level(a.floor).unwrap_or(1), preferred: level(a.preferred).unwrap_or(1), k: rules.ratchet.k }, a.plan_following)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const B: Bounds = Bounds { floor: 0, preferred: 1, k: 5 };

    /// `specs/056` §8: the ratchet steps down after `K` successes, up after one failure, and stops after two failures in a row.
    #[test]
    fn the_ratchet_steps_down_after_k_up_after_one_failure_and_stops_after_two() {
        let mut e = Entry::default();
        for i in 0..4 {
            assert!(e.apply(1, true, B).is_empty(), "success {i} of 5 does not move it");
        }
        assert_eq!(e.apply(1, true, B), vec![Move::Down { from: 1, to: 0 }]);
        assert_eq!(e.wanted(B), -1);
        assert!(e.apply(0, true, B).is_empty(), "never below the floor, however many successes");
        assert_eq!(e.apply(0, false, B), vec![Move::Up { from: 0, to: 1 }], "one failure puts it back");
        assert_eq!(e.wanted(B), 0);
        for _ in 0..4 {
            e.apply(1, true, B);
        }
        assert_eq!(e.apply(1, true, B), vec![Move::Down { from: 1, to: 0 }]);
        assert_eq!(e.apply(0, false, B), vec![Move::Stopped { at: 0 }, Move::Up { from: 0, to: 1 }], "the second failure in a row at quick stops it");
        for _ in 0..12 {
            assert!(e.apply(1, true, B).is_empty(), "a stopped class is not tried again");
        }
        assert_eq!(e.wanted(B), 0);
    }

    #[test]
    fn a_success_between_failures_resets_the_count_and_the_preferred_class_is_never_stopped() {
        let mut e = Entry { step: -1, ..Default::default() };
        e.apply(0, false, B);
        e.apply(1, false, B);
        e.apply(1, false, B);
        assert!(e.stopped.is_empty(), "failures at the preferred class stop nothing: {e:?}");
        let mut f = Entry { step: -1, ..Default::default() };
        f.apply(0, false, B);
        f.step = -1;
        f.apply(0, true, B);
        f.apply(0, false, B);
        assert!(f.stopped.is_empty(), "a success at quick broke the run of failures");
    }

    #[test]
    fn a_forged_step_cannot_go_below_the_floor_or_above_the_preferred_class() {
        let e = Entry { step: -7, ..Default::default() };
        assert_eq!(e.wanted(B), -1);
        assert_eq!(Entry { step: 3, ..Default::default() }.wanted(B), 0);
        assert_eq!(e.wanted(Bounds { floor: 1, preferred: 1, k: 5 }), 0, "floor equal to preferred: no ratchet");
        let stopped = Entry { step: -1, stopped: vec![0], ..Default::default() };
        assert_eq!(stopped.wanted(B), 0);
    }

    #[test]
    fn state_is_saved_cleared_by_the_operator_and_read_back() {
        let d = tempfile::tempdir().unwrap();
        for _ in 0..5 {
            record(d.path(), "builder", BUILD, 1, true, B).unwrap();
        }
        assert_eq!(step_for(d.path(), "builder", BUILD, B), -1);
        assert_eq!(step_for(d.path(), "writer", BUILD, B), 0);
        record(d.path(), "builder", BUILD, 0, false, B).unwrap();
        for _ in 0..5 {
            record(d.path(), "builder", BUILD, 1, true, B).unwrap();
        }
        let moves = record(d.path(), "builder", BUILD, 0, false, B).unwrap();
        assert!(moves.contains(&Move::Stopped { at: 0 }), "{moves:?}");
        assert_eq!(clear(d.path(), "builder", BUILD).unwrap(), vec![0]);
        assert!(load(d.path())[0].stopped.is_empty());
        assert_eq!(Move::Stopped { at: 0 }.payload("builder", BUILD, "t")["to"], "quick");
    }

    fn ev(task: &str, agent: Option<&str>, kind: &str, pair: Option<(&str, &str)>, payload: serde_json::Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m", kind, payload);
        e.task_id = Some(task.into());
        e.agent = agent.map(String::from);
        if let Some((ep, m)) = pair {
            e.endpoint_id = Some(ep.into());
            e.model_id = Some(m.into());
        }
        e
    }

    #[test]
    fn a_finished_build_is_one_row_for_its_first_pair_with_every_retry_in_its_cost() {
        let usage = |m: &str, n: u64| ev("t", Some("builder"), "agent.usage", Some(("ep", m)), json!({"input_tokens": n, "output_tokens": n, "cache_read_tokens": 1, "cache_write_tokens": 0}));
        let events = vec![
            ev("t", None, "build.attempt", None, json!({"attempt": 1})),
            ev("t", Some("builder"), "model.assigned", None, json!({"endpoint_id": "ep", "model_id": "m-small", "class": "quick"})),
            usage("m-small", 100),
            ev("t", None, "build.attempt", None, json!({"attempt": 2})),
            usage("m-mid", 50),
            ev("t", Some("red-team"), "agent.usage", Some(("ep", "m-big")), json!({"input_tokens": 9999})),
            ev("t", None, "task.completed", None, json!({"status": "done"})),
        ];
        let r = retro(&events, "t", "builder").unwrap();
        assert_eq!((r.first_pass, r.model_id.as_str(), r.class, r.spent.len()), (false, "m-small", Some(Class::Quick), 2));
        let row = r.row("builder", BUILD, |_, m| (m == "m-mid").then_some(3.0));
        assert_eq!((row.accepted, row.tokens.cost(row.multiplier)), (false, 201 + 101 * 3), "a retry on another model is the first pick's cost");
        assert!(retro(&events[..3], "t", "builder").is_none(), "an open task is no row");
        assert!(retro(&[ev("u", None, "task.completed", None, json!({"status": "done"}))], "u", "builder").is_none(), "a task that never built is no row");
    }

    #[test]
    fn suggestions_are_counts_and_never_below_the_floor() {
        let e = Entry { role: "builder".into(), kind: BUILD.into(), tally: [(1, [14, 14]), (0, [2, 9])].into_iter().collect(), ..Default::default() };
        let s = suggestions(std::slice::from_ref(&e), 8, |_| 0);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(s.iter().any(|l| l.starts_with("builder, build tasks: 14 of 14 first-pass at steady; try -1")), "{s:?}");
        assert!(s.iter().any(|l| l.contains("2 of 9 first-pass at quick; try +1")), "{s:?}");
        assert!(suggestions(std::slice::from_ref(&e), 8, |_| 1).iter().all(|l| !l.contains("try -1")), "no -1 below the floor");
        assert!(suggestions(&[e], 20, |_| 0).is_empty(), "too few results suggest nothing");
    }
}
