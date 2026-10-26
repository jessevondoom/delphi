//! What the learning loop needs to know about a finished task, read from the event log (`specs/031` §3.4, slice L0).
//!
//! Nothing here writes anything: [`task_records`] folds a mission's events into one [`TaskRecord`] per task, so a rule's
//! or a skill's effect can later be compared across the tasks that carried it and the tasks that did not. The record is
//! derived, like the usage ledger, and can be rebuilt from `events.jsonl` at any time.

use std::collections::{BTreeMap, BTreeSet};

use delphi_core::event::EventEnvelope;

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
}
