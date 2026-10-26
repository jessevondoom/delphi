//! What the learning loop needs to know about a finished task, read from the event log (`specs/031` §3.4, slice L0).
//!
//! Nothing here writes anything: [`task_records`] folds a mission's events into one [`TaskRecord`] per task, so a rule's
//! or a skill's effect can later be compared across the tasks that carried it and the tasks that did not. The record is
//! derived, like the usage ledger, and can be rebuilt from `events.jsonl` at any time.

use std::collections::{BTreeMap, BTreeSet};

use delphi_core::event::EventEnvelope;
use delphi_core::provenance::Tier;

/// How a task ended, as far as the log says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `task.completed` with status `done`.
    Done,
    /// `task.completed` with status `escalated`: the operator was handed the decision.
    Escalated,
    /// `task.failed`.
    Failed,
    /// `task.cancelled`.
    Cancelled,
    /// No terminal event yet.
    Open,
}

/// One task, summarized.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRecord {
    pub task_id: String,
    pub outcome: Outcome,
    /// `(scope, id)` of every rule and lesson any span of the task carried, once each.
    pub rules: BTreeSet<(String, String)>,
    /// `build.attempt` events: 1 means the first try was the only one; 0 for a task that never builds.
    pub attempts: u32,
    /// Cards the operator denied during the task.
    pub denied_cards: u32,
    /// Provisional decisions the operator rejected at review (`decision.reverted`).
    pub reverted_decisions: u32,
    /// Money spent by the task's spans, from `agent.usage`.
    pub cost_usd: f64,
}

impl TaskRecord {
    /// Done on the first build attempt with nothing denied: the signal `specs/004` §5.6 calls first-pass success.
    /// Only meaningful for a task that builds (`attempts >= 1`).
    pub fn first_pass(&self) -> bool {
        self.outcome == Outcome::Done && self.attempts == 1 && self.denied_cards == 0 && self.reverted_decisions == 0
    }
}

/// Fold `events` into one record per task, in order of first appearance. Events without a `task_id` are skipped.
pub fn task_records(events: &[EventEnvelope]) -> Vec<TaskRecord> {
    let mut order: Vec<String> = Vec::new();
    let mut by: BTreeMap<String, TaskRecord> = BTreeMap::new();
    for e in events {
        let Some(task) = e.task_id.as_deref() else { continue };
        let rec = by.entry(task.to_string()).or_insert_with(|| {
            order.push(task.to_string());
            TaskRecord { task_id: task.to_string(), outcome: Outcome::Open, rules: BTreeSet::new(), attempts: 0, denied_cards: 0, reverted_decisions: 0, cost_usd: 0.0 }
        });
        match e.kind.as_str() {
            "snapshot.loaded" => {
                for r in e.payload["rules"].as_array().into_iter().flatten() {
                    if let (Some(scope), Some(id)) = (r["scope"].as_str(), r["id"].as_str()) {
                        rec.rules.insert((scope.to_string(), id.to_string()));
                    }
                }
            }
            "task.completed" => rec.outcome = if e.payload["status"] == "escalated" { Outcome::Escalated } else { Outcome::Done },
            "task.failed" => rec.outcome = Outcome::Failed,
            "task.cancelled" => rec.outcome = Outcome::Cancelled,
            "build.attempt" => rec.attempts += 1,
            "approval.resolved" if e.payload["resolution"] == "denied" => rec.denied_cards += 1,
            "decision.reverted" => rec.reverted_decisions += 1,
            "agent.usage" => rec.cost_usd += e.payload["cost_usd"].as_f64().unwrap_or(0.0),
            _ => {}
        }
    }
    order.into_iter().filter_map(|t| by.remove(&t)).collect()
}

// ── L1: the staging tier (`specs/031` §3.1, §3.2) ───────────────────────────────────────────────────────────────

/// The five capture triggers the daemon can see without a model (`specs/031` §3.2 items 1-5), by the name a note and
/// `[learning] triggers` use.
pub const TRIGGERS: [&str; 5] = ["denied_card", "correction", "recovery", "escalation", "rejected_decision"];

/// A staged note's summary is at most this many characters.
pub const MAX_SUMMARY: usize = 280;

/// `[learning]` in `config.toml`. Capture can be turned off or narrowed; nothing here can make staged text load.
#[derive(Debug, Clone, PartialEq)]
pub struct LearningSettings {
    pub capture: bool,
    /// Notes kept per mission; the oldest is dropped (`learn.dropped`, reason `evicted`) when a new one would exceed it.
    pub max_notes: usize,
    pub triggers: BTreeSet<String>,
}

impl Default for LearningSettings {
    fn default() -> Self {
        LearningSettings { capture: true, max_notes: 200, triggers: TRIGGERS.iter().map(|t| t.to_string()).collect() }
    }
}

impl LearningSettings {
    pub const MAX_NOTES_FLOOR: usize = 10;
    pub const MAX_NOTES_CEILING: usize = 1000;

    /// Read `[learning]` from a parsed `config.toml`. A value outside the floor and ceiling is clamped; an unknown key or
    /// trigger is a returned problem, for the boot notices.
    pub fn parse(root: &toml::Value) -> (LearningSettings, Vec<String>) {
        let mut s = LearningSettings::default();
        let mut problems = Vec::new();
        let Some(t) = root.get("learning").and_then(|v| v.as_table()) else { return (s, problems) };
        for k in t.keys() {
            if !["capture", "max_notes", "triggers"].contains(&k.as_str()) {
                problems.push(format!("[learning] {}: not a setting (capture, max_notes, triggers)", k));
            }
        }
        if let Some(v) = t.get("capture") {
            match v.as_bool() {
                Some(b) => s.capture = b,
                None => problems.push("[learning] capture: expected true or false".into()),
            }
        }
        if let Some(v) = t.get("max_notes") {
            match v.as_integer() {
                Some(n) => {
                    let c = (n.max(0) as usize).clamp(Self::MAX_NOTES_FLOOR, Self::MAX_NOTES_CEILING);
                    if c as i64 != n {
                        problems.push(format!("[learning] max_notes: {} is outside {}..={}; using {}", n, Self::MAX_NOTES_FLOOR, Self::MAX_NOTES_CEILING, c));
                    }
                    s.max_notes = c;
                }
                None => problems.push("[learning] max_notes: expected a whole number".into()),
            }
        }
        if let Some(v) = t.get("triggers") {
            match v.as_array() {
                Some(a) => {
                    let mut on = BTreeSet::new();
                    for name in a.iter().filter_map(|x| x.as_str()) {
                        if TRIGGERS.contains(&name) {
                            on.insert(name.to_string());
                        } else {
                            problems.push(format!("[learning] triggers: `{}` is not a trigger ({})", name, TRIGGERS.join(", ")));
                        }
                    }
                    s.triggers = on;
                }
                None => problems.push("[learning] triggers: expected a list of names".into()),
            }
        }
        (s, problems)
    }
}

/// One candidate lesson, before it is written as a `learn.staged` event.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedNote {
    pub trigger: &'static str,
    pub task_id: Option<String>,
    pub role: Option<String>,
    /// The `seq` of every event the note was built from.
    pub event_seqs: Vec<u64>,
    pub summary: String,
    /// The lowest tier among the sources: operator-typed text is `L5`; anything the daemon composed is `L3`; a task whose
    /// span read untrusted content (`span.taint`) caps it at that span's ceiling. Promotion refuses a note below `L4`.
    pub tier: Tier,
}

impl StagedNote {
    /// The note's id: the `seq` of the event that triggered it, so a replay names it the same way.
    pub fn id(&self) -> String {
        format!("n{}", self.event_seqs.last().copied().unwrap_or(0))
    }

    pub fn to_event(&self, mission_id: &str) -> EventEnvelope {
        let mut e = EventEnvelope::new(mission_id, "learn.staged", serde_json::json!({"id": self.id(), "trigger": self.trigger, "summary": self.summary, "event_seqs": self.event_seqs, "tier": self.tier.as_str()}));
        e.task_id = self.task_id.clone();
        e.agent = self.role.clone();
        e
    }
}

/// What observing one event asks the log to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Capture {
    Note(StagedNote),
    /// A live note pushed out by the cap: write `learn.dropped` with reason `evicted`.
    Evict(String),
}

fn clip(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= MAX_SUMMARY {
        return one;
    }
    let mut t: String = one.chars().take(MAX_SUMMARY - 1).collect();
    t.push('…');
    t
}

/// A card reason the daemon or a headless run wrote, not a person: never a lesson.
fn machine_reason(r: &str) -> bool {
    ["headless run:", "blocked by the hook", "the task was cancelled", "the daemon restarted"].iter().any(|p| r.starts_with(p))
}

/// Explicit cues only (`specs/031` §11 Q1): a message that opens by saying no, stop or don't. Its precision is measured in
/// training mode before this widens.
pub fn is_correction(prompt: &str) -> bool {
    let p = prompt.trim_start().to_lowercase();
    p.chars().count() >= 8 && ["no,", "no.", "no ", "don't ", "do not ", "stop ", "that's wrong", "that is wrong", "that's not", "that is not", "wrong,", "actually,", "not like that", "undo "].iter().any(|c| p.starts_with(c))
}

/// Cheap per-mission capture state. Feed it every event of the mission in order (the log, then each new append); it
/// answers with the notes to stage. No I/O, no model call, and a `learn.*` event never triggers another.
#[derive(Debug, Default)]
pub struct Stager {
    settings: LearningSettings,
    /// Highest `build.attempt` per task.
    attempts: BTreeMap<String, u32>,
    /// The lowest tier ceiling a task's spans reached (`span.taint`).
    ceiling: BTreeMap<String, Tier>,
    /// `seq` of the attempt-2-or-3 event per task still waiting to see whether the task finishes.
    retried: BTreeMap<String, u64>,
    tasks_seen: u32,
    /// Ids of live notes, oldest first.
    live: std::collections::VecDeque<String>,
}

impl Stager {
    pub fn new(settings: LearningSettings) -> Stager {
        Stager { settings, ..Default::default() }
    }

    pub fn set_settings(&mut self, settings: LearningSettings) {
        self.settings = settings;
    }

    /// Rebuild the state from a mission's events. Notes already in the log are not repeated; the returned state only
    /// remembers them, so the cap holds across a restart.
    pub fn replay(settings: LearningSettings, events: &[EventEnvelope]) -> Stager {
        let mut s = Stager::new(LearningSettings { capture: false, ..settings.clone() });
        for e in events {
            s.observe(e);
        }
        s.settings = settings;
        s
    }

    fn on(&self, trigger: &str) -> bool {
        self.settings.capture && self.settings.triggers.contains(trigger)
    }

    /// The tier a note from `task` may claim: `base`, lowered to the task's span ceiling.
    fn tier(&self, task: Option<&str>, base: Tier) -> Tier {
        task.and_then(|t| self.ceiling.get(t)).map_or(base, |c| base.min(*c))
    }

    pub fn observe(&mut self, e: &EventEnvelope) -> Vec<Capture> {
        let task = e.task_id.as_deref();
        let mk = |s: &Stager, trigger: &'static str, summary: String, seqs: Vec<u64>, base: Tier| {
            Capture::Note(StagedNote { trigger, task_id: task.map(String::from), role: e.agent.clone(), event_seqs: seqs, summary: clip(&summary), tier: s.tier(task, base) })
        };
        let mut out = Vec::new();
        match e.kind.as_str() {
            "learn.staged" => {
                // A live capture is counted when it is decided; its own event arrives here a moment later.
                if let Some(id) = e.payload["id"].as_str().filter(|id| !self.live.iter().any(|l| l == id)) {
                    self.live.push_back(id.to_string());
                }
                return out;
            }
            "learn.dropped" => {
                if let Some(id) = e.payload["id"].as_str() {
                    self.live.retain(|l| l != id);
                }
                return out;
            }
            k if k.starts_with("learn.") => return out,
            "span.taint" => {
                if let (Some(t), Some(c)) = (task, e.payload["ceiling"].as_str().and_then(Tier::parse)) {
                    let slot = self.ceiling.entry(t.to_string()).or_insert(c);
                    *slot = (*slot).min(c);
                }
            }
            "task.created" => {
                let prompt = e.payload["prompt"].as_str().unwrap_or("");
                // A correction needs something to correct: an earlier task in this mission, and an explicit cue.
                if self.tasks_seen > 0 && self.on("correction") && is_correction(prompt) {
                    out.push(mk(self, "correction", format!("The operator corrected the team: {}", prompt), vec![e.seq], Tier::L5));
                }
                self.tasks_seen += 1;
            }
            "approval.resolved" if e.payload["resolution"] == "denied" && self.on("denied_card") => {
                let reason = e.payload["reason"].as_str().unwrap_or("").trim();
                if !reason.is_empty() && !machine_reason(reason) {
                    out.push(mk(self, "denied_card", format!("A card was denied: {}", reason), vec![e.seq], Tier::L5));
                }
            }
            "build.attempt" => {
                if let Some(t) = task {
                    let n = e.payload["attempt"].as_u64().unwrap_or(0) as u32;
                    let top = self.attempts.entry(t.to_string()).or_insert(0);
                    *top = (*top).max(n);
                    if (2..=3).contains(&n) {
                        self.retried.entry(t.to_string()).or_insert(e.seq);
                    }
                }
            }
            "decision.reverted" if self.on("rejected_decision") => {
                let path = e.payload["path"].as_str().unwrap_or("a file");
                out.push(mk(self, "rejected_decision", format!("A provisional decision about {} was rejected at review.", path), vec![e.seq], Tier::L3));
            }
            "task.completed" => {
                let tried = task.and_then(|t| self.attempts.get(t)).copied().unwrap_or(0);
                if e.payload["status"] == "escalated" {
                    if self.on("escalation") {
                        let reason = e.payload["reason"].as_str().unwrap_or("").trim();
                        let reason = if reason.is_empty() { "no reason recorded" } else { reason };
                        out.push(mk(self, "escalation", format!("A task was escalated to the operator after {} build attempt(s): {}", tried, reason), vec![e.seq], Tier::L3));
                    }
                } else if let Some(first) = task.and_then(|t| self.retried.remove(t)) {
                    if self.on("recovery") {
                        out.push(mk(self, "recovery", format!("A task passed on build attempt {} after earlier attempts failed.", tried), vec![first, e.seq], Tier::L3));
                    }
                }
            }
            _ => {}
        }
        // The cap counts live notes including the ones about to be written; the oldest go first.
        let new = out.len();
        let mut evict = Vec::new();
        for _ in 0..(self.live.len() + new).saturating_sub(self.settings.max_notes) {
            match self.live.pop_front() {
                Some(id) => evict.push(Capture::Evict(id)),
                None => break,
            }
        }
        for c in &out {
            if let Capture::Note(n) = c {
                self.live.push_back(n.id());
            }
        }
        evict.extend(out);
        evict
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn ev(task: Option<&str>, kind: &str, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m_1", kind, payload);
        e.task_id = task.map(String::from);
        e
    }

    #[test]
    fn a_first_pass_build_is_done_on_attempt_one_with_nothing_denied() {
        let evs = vec![
            ev(Some("t1"), "snapshot.loaded", json!({"rules": [{"scope": "release", "id": "H-001"}, {"scope": "workspace-lessons", "id": "L-BLD-001"}]})),
            ev(Some("t1"), "build.attempt", json!({"attempt": 1})),
            ev(Some("t1"), "agent.usage", json!({"cost_usd": 0.25})),
            ev(Some("t1"), "agent.usage", json!({"cost_usd": 0.5})),
            ev(Some("t1"), "task.completed", json!({"status": "done"})),
        ];
        let r = task_records(&evs);
        assert_eq!(r.len(), 1);
        assert!(r[0].first_pass() && r[0].cost_usd == 0.75);
        assert_eq!(r[0].rules.len(), 2);
    }

    #[test]
    fn retries_denials_reverts_and_escalation_are_not_first_pass() {
        let done = json!({"status": "done"});
        let b = || ev(Some("t"), "build.attempt", json!({}));
        let retried = task_records(&[b(), b(), ev(Some("t"), "task.completed", done.clone())]);
        assert!(!retried[0].first_pass() && retried[0].attempts == 2);
        let denied = task_records(&[b(), ev(Some("t"), "approval.resolved", json!({"resolution": "denied"})), ev(Some("t"), "task.completed", done.clone())]);
        assert!(!denied[0].first_pass() && denied[0].denied_cards == 1);
        let approved = task_records(&[b(), ev(Some("t"), "approval.resolved", json!({"resolution": "approved"})), ev(Some("t"), "task.completed", done.clone())]);
        assert!(approved[0].first_pass(), "an approved card is not a rejection");
        let reverted = task_records(&[b(), ev(Some("t"), "decision.reverted", json!({})), ev(Some("t"), "task.completed", done)]);
        assert!(!reverted[0].first_pass() && reverted[0].reverted_decisions == 1);
        let esc = task_records(&[b(), ev(Some("t"), "task.completed", json!({"status": "escalated", "reason": "budget"}))]);
        assert_eq!(esc[0].outcome, Outcome::Escalated);
        assert!(!esc[0].first_pass());
    }

    #[test]
    fn spans_of_one_task_pool_their_rules_once_and_tasks_stay_apart() {
        let loaded = |t: &str| ev(Some(t), "snapshot.loaded", json!({"rules": [{"scope": "release", "id": "H-001"}]}));
        let evs = vec![loaded("a"), loaded("a"), loaded("b"), ev(None, "mission.note", json!({})), ev(Some("a"), "task.failed", json!({"error": "x"})), ev(Some("b"), "task.cancelled", json!({}))];
        let r = task_records(&evs);
        assert_eq!(r.iter().map(|t| t.task_id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(r[0].rules.len(), 1);
        assert_eq!((r[0].outcome.clone(), r[1].outcome.clone()), (Outcome::Failed, Outcome::Cancelled));
    }

    #[test]
    fn a_task_with_no_terminal_event_is_open() {
        assert_eq!(task_records(&[ev(Some("t"), "snapshot.loaded", json!({"rules": []}))])[0].outcome, Outcome::Open);
    }

    // ── L1 ──

    fn at(seq: u64, task: Option<&str>, kind: &str, payload: Value) -> EventEnvelope {
        let mut e = ev(task, kind, payload);
        e.seq = seq;
        e
    }

    /// Run `events` through a fresh stager and keep only the notes.
    fn stage(events: &[EventEnvelope]) -> Vec<StagedNote> {
        let mut s = Stager::new(LearningSettings::default());
        events.iter().flat_map(|e| s.observe(e)).filter_map(|c| if let Capture::Note(n) = c { Some(n) } else { None }).collect()
    }

    #[test]
    fn a_denied_card_with_a_reason_is_one_operator_note() {
        let n = stage(&[at(5, Some("t1"), "approval.resolved", json!({"resolution": "denied", "reason": "never touch the lockfile", "by": "console"}))]);
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].trigger, n[0].tier, n[0].event_seqs.clone(), n[0].id()), ("denied_card", Tier::L5, vec![5], "n5".into()));
        assert!(n[0].summary.contains("never touch the lockfile"));
    }

    #[test]
    fn denials_nobody_typed_are_not_lessons() {
        for reason in ["", "  ", "headless run: --on-card deny", "blocked by the hook `x`: no", "the daemon restarted; asked again"] {
            assert!(stage(&[at(1, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": reason}))]).is_empty(), "{:?}", reason);
        }
        assert!(stage(&[at(1, Some("t"), "approval.resolved", json!({"resolution": "approved", "reason": "fine"}))]).is_empty());
    }

    #[test]
    fn a_correction_needs_an_earlier_task_and_an_explicit_cue() {
        let first = at(1, Some("t1"), "task.created", json!({"prompt": "no, wait, do it for me"}));
        assert!(stage(&[first.clone()]).is_empty(), "nothing before it to correct");
        let n = stage(&[first, at(9, Some("t2"), "task.created", json!({"prompt": "No, use the existing helper instead"}))]);
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].trigger, n[0].tier), ("correction", Tier::L5));
        let plain = stage(&[at(1, Some("t1"), "task.created", json!({"prompt": "add a flag"})), at(2, Some("t2"), "task.created", json!({"prompt": "now add a test too"}))]);
        assert!(plain.is_empty());
    }

    #[test]
    fn recovery_is_noted_once_when_the_task_finishes_after_attempt_two_or_three() {
        let evs = |last: Value| {
            vec![
                at(1, Some("t"), "build.attempt", json!({"attempt": 1})),
                at(2, Some("t"), "build.attempt", json!({"attempt": 2})),
                at(3, Some("t"), "build.attempt", json!({"attempt": 3})),
                at(4, Some("t"), "task.completed", last),
            ]
        };
        let n = stage(&evs(json!({"status": "done"})));
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].trigger, n[0].event_seqs.clone(), n[0].tier), ("recovery", vec![2, 4], Tier::L3));
        let first_try = stage(&[at(1, Some("t"), "build.attempt", json!({"attempt": 1})), at(2, Some("t"), "task.completed", json!({"status": "done"}))]);
        assert!(first_try.is_empty());
        let esc = stage(&evs(json!({"status": "escalated", "reason": "gates failed"})));
        assert_eq!(esc.iter().map(|n| n.trigger).collect::<Vec<_>>(), vec!["escalation"], "an escalated task did not recover");
        assert!(esc[0].summary.contains("3 build attempt") && esc[0].summary.contains("gates failed"));
    }

    #[test]
    fn a_rejected_provisional_decision_is_noted() {
        let n = stage(&[at(7, Some("t"), "decision.reverted", json!({"decision_id": "d", "path": "src/a.rs"}))]);
        assert_eq!((n.len(), n[0].trigger), (1, "rejected_decision"));
        assert!(n[0].summary.contains("src/a.rs"));
    }

    #[test]
    fn a_note_from_a_task_that_read_untrusted_content_stays_below_empirical() {
        let canary = "CANARY-9f3a";
        let n = stage(&[
            at(1, Some("t"), "span.taint", json!({"ceiling": "L0", "cause": "web"})),
            at(2, Some("t"), "tool.result", json!({"name": "web_fetch", "text": canary})),
            at(3, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": "not that way"})),
        ]);
        assert_eq!(n.len(), 1);
        assert!(n[0].tier < Tier::L4, "tainted: {:?}", n[0].tier);
        assert!(!n[0].summary.contains(canary) && !serde_json::to_string(&n[0].to_event("m")).unwrap().contains(canary), "tool output never reaches a note");
        let other = stage(&[at(1, Some("a"), "span.taint", json!({"ceiling": "L0"})), at(2, Some("b"), "approval.resolved", json!({"resolution": "denied", "reason": "no"}))]);
        assert_eq!(other[0].tier, Tier::L5, "another task's taint does not leak");
    }

    #[test]
    fn learn_events_never_trigger_and_summaries_are_clipped() {
        let mut s = Stager::new(LearningSettings::default());
        assert!(s.observe(&at(1, Some("t"), "learn.staged", json!({"id": "n1", "resolution": "denied", "reason": "x"}))).is_empty());
        let long = "word ".repeat(200);
        let n = stage(&[at(1, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": long}))]);
        assert!(n[0].summary.chars().count() <= MAX_SUMMARY && n[0].summary.ends_with('…'));
    }

    #[test]
    fn the_cap_evicts_the_oldest_and_a_replay_remembers_what_is_live() {
        let settings = LearningSettings { max_notes: 10, ..Default::default() };
        let mut s = Stager::new(settings.clone());
        let mut log = Vec::new();
        for i in 0..12u64 {
            let e = at(100 + i, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": format!("reason {}", i)}));
            for c in s.observe(&e) {
                match c {
                    Capture::Note(n) => log.push(at(200 + i, Some("t"), "learn.staged", json!({"id": n.id()}))),
                    Capture::Evict(id) => log.push(at(200 + i, None, "learn.dropped", json!({"id": id, "reason": "evicted"}))),
                }
            }
        }
        let dropped: Vec<_> = log.iter().filter(|e| e.kind == "learn.dropped").map(|e| e.payload["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(dropped, vec!["n100", "n101"]);
        // The restart: replaying the log restores the live set without producing anything, so the next note evicts n102.
        let mut again = Stager::replay(settings, &log);
        let out = again.observe(&at(500, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": "one more"})));
        assert_eq!(out.iter().filter(|c| matches!(c, Capture::Evict(id) if id == "n102")).count(), 1, "{:?}", out);
    }

    #[test]
    fn capture_off_and_single_triggers_can_be_switched_off() {
        let denied = at(1, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": "r"}));
        let off = |s: LearningSettings| Stager::new(s).observe(&denied);
        assert!(off(LearningSettings { capture: false, ..Default::default() }).is_empty());
        assert!(off(LearningSettings { triggers: ["escalation".to_string()].into(), ..Default::default() }).is_empty());
    }

    #[test]
    fn learning_settings_are_bounded_and_typos_are_reported() {
        let root: toml::Value = toml::from_str("[learning]\ncapture = true\nmax_notes = 5000\ntriggers = [\"denied_card\", \"nudge\"]\nload_staged = true\n").unwrap();
        let (s, problems) = LearningSettings::parse(&root);
        assert_eq!(s.max_notes, LearningSettings::MAX_NOTES_CEILING);
        assert_eq!(s.triggers, BTreeSet::from(["denied_card".to_string()]));
        assert_eq!(problems.len(), 3, "{:?}", problems);
        assert!(problems.iter().any(|p| p.contains("load_staged")), "there is no setting that loads staged text");
        assert_eq!(LearningSettings::parse(&toml::from_str("").unwrap()), (LearningSettings::default(), vec![]));
    }
}
