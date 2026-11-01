//! Collaboration telemetry and tenacity pushback (`specs/063` §4.1, §4.6, §6).
//!
//! Measures multi-agent collaboration outcomes from mission events (First-Pass Yield, rework ratio,
//! and collaboration overhead), and enforces the tenacity directive when the investigator exhausts avenues.

use delphi_core::event::EventEnvelope;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The canonical directive the orchestrator pushes back with when the investigator hits an impasse.
pub const TENACITY_PUSHBACK_MESSAGE: &str =
    "This problem is solvable. Step back, reconsider assumptions, and pursue an alternate angle.";

/// Event kind emitted when tenacity pushback occurs.
pub const TENACITY_EVENT: &str = "investigator.tenacity_pushback";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskCollaboration {
    pub task_id: String,
    pub agents: Vec<String>,
    pub attempts: u32,
    pub passed_first_attempt: bool,
    pub succeeded: bool,
    pub tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollaborationReport {
    pub total_tasks: u32,
    pub collaborative_tasks: u32,
    pub single_agent_tasks: u32,
    pub first_pass_yield_pct: Option<u32>,
    pub collaborative_fpy_pct: Option<u32>,
    pub single_agent_fpy_pct: Option<u32>,
    pub avg_attempts_collaborative: f64,
    pub avg_attempts_single: f64,
    pub tenacity_pushbacks: u32,
}

/// Analyze collaboration metrics from a slice of mission events.
pub fn report(events: &[EventEnvelope]) -> CollaborationReport {
    let mut tasks: BTreeMap<String, TaskCollaboration> = BTreeMap::new();
    let mut tenacity_pushbacks = 0;

    for ev in events {
        match ev.kind.as_str() {
            "task.created" => {
                let tid = ev.task_id.as_deref().unwrap_or("");
                let target = ev.payload["target"].as_str().unwrap_or("").to_string();
                if !tid.is_empty() {
                    let entry = tasks.entry(tid.to_string()).or_default();
                    entry.task_id = tid.to_string();
                    if !target.is_empty() && !entry.agents.contains(&target) {
                        entry.agents.push(target);
                    }
                    if entry.attempts == 0 {
                        entry.attempts = 1;
                    }
                }
            }
            "dag.node.added" => {
                let tid = ev.task_id.as_deref().unwrap_or("");
                let role = ev.payload["role"].as_str().unwrap_or("").to_string();
                if !tid.is_empty() {
                    let entry = tasks.entry(tid.to_string()).or_default();
                    if !role.is_empty() && !entry.agents.contains(&role) {
                        entry.agents.push(role);
                    }
                }
            }
            "build.attempt" => {
                let tid = ev.task_id.as_deref().unwrap_or("");
                if !tid.is_empty() {
                    let entry = tasks.entry(tid.to_string()).or_default();
                    let attempt = ev.payload["attempt"].as_u64().unwrap_or(1) as u32;
                    if attempt > entry.attempts {
                        entry.attempts = attempt;
                    }
                }
            }
            "gates.evaluated" => {
                let tid = ev.task_id.as_deref().unwrap_or("");
                let passed = ev.payload["passed"].as_bool().unwrap_or(false);
                if !tid.is_empty() {
                    let entry = tasks.entry(tid.to_string()).or_default();
                    if passed && entry.attempts <= 1 {
                        entry.passed_first_attempt = true;
                    }
                }
            }
            "task.completed" => {
                let tid = ev.task_id.as_deref().unwrap_or("");
                if !tid.is_empty() {
                    let entry = tasks.entry(tid.to_string()).or_default();
                    entry.succeeded = true;
                    if entry.attempts <= 1 {
                        entry.passed_first_attempt = true;
                    }
                }
            }
            "agent.usage" => {
                let tid = ev.task_id.as_deref().unwrap_or("");
                let tokens = ev.payload["tokens"]["total"].as_u64().unwrap_or(0);
                if !tid.is_empty() {
                    let entry = tasks.entry(tid.to_string()).or_default();
                    entry.tokens += tokens;
                }
            }
            TENACITY_EVENT => {
                tenacity_pushbacks += 1;
            }
            _ => {}
        }
    }

    let total_tasks = tasks.len() as u32;
    let mut collab_count = 0;
    let mut single_count = 0;
    let mut collab_fpy_count = 0;
    let mut single_fpy_count = 0;
    let mut collab_attempts_total = 0;
    let mut single_attempts_total = 0;
    let mut total_fpy_count = 0;

    for task in tasks.values() {
        if task.passed_first_attempt {
            total_fpy_count += 1;
        }
        if task.agents.len() > 1 {
            collab_count += 1;
            collab_attempts_total += task.attempts;
            if task.passed_first_attempt {
                collab_fpy_count += 1;
            }
        } else {
            single_count += 1;
            single_attempts_total += task.attempts;
            if task.passed_first_attempt {
                single_fpy_count += 1;
            }
        }
    }

    let pct = |n: u32, d: u32| (d > 0).then_some((n * 100 + d / 2) / d);
    let avg = |tot: u32, cnt: u32| if cnt > 0 { tot as f64 / cnt as f64 } else { 0.0 };

    CollaborationReport {
        total_tasks,
        collaborative_tasks: collab_count,
        single_agent_tasks: single_count,
        first_pass_yield_pct: pct(total_fpy_count, total_tasks),
        collaborative_fpy_pct: pct(collab_fpy_count, collab_count),
        single_agent_fpy_pct: pct(single_fpy_count, single_count),
        avg_attempts_collaborative: avg(collab_attempts_total, collab_count),
        avg_attempts_single: avg(single_attempts_total, single_count),
        tenacity_pushbacks,
    }
}

/// Check whether the tenacity pushback should be triggered for an investigator span.
pub fn should_pushback(role: &str, result_exhausted_or_failed: bool, already_pushed_back: bool) -> bool {
    role == "investigator" && result_exhausted_or_failed && !already_pushed_back
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(kind: &str, task_id: &str, payload: serde_json::Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", kind, payload);
        e.task_id = Some(task_id.to_string());
        e
    }

    #[test]
    fn collaboration_report_calculates_fpy_and_attempts() {
        let events = vec![
            // Task 1: Single agent, passes first attempt
            ev("task.created", "t1", json!({"target": "orchestrator"})),
            ev("task.completed", "t1", json!({})),
            // Task 2: Collaborative (builder + architect), passes on attempt 2
            ev("task.created", "t2", json!({"target": "builder"})),
            ev("dag.node.added", "t2", json!({"role": "architect"})),
            ev("build.attempt", "t2", json!({"attempt": 2})),
            ev("task.completed", "t2", json!({})),
            // Tenacity pushback
            EventEnvelope::new("m1", TENACITY_EVENT, json!({"role": "investigator"})),
        ];

        let rep = report(&events);
        assert_eq!(rep.total_tasks, 2);
        assert_eq!(rep.single_agent_tasks, 1);
        assert_eq!(rep.collaborative_tasks, 1);
        assert_eq!(rep.single_agent_fpy_pct, Some(100));
        assert_eq!(rep.collaborative_fpy_pct, Some(0));
        assert_eq!(rep.first_pass_yield_pct, Some(50));
        assert_eq!(rep.tenacity_pushbacks, 1);
    }

    #[test]
    fn tenacity_pushback_triggers_once_for_investigator() {
        assert!(should_pushback("investigator", true, false));
        assert!(!should_pushback("investigator", true, true));
        assert!(!should_pushback("investigator", false, false));
        assert!(!should_pushback("scout", true, false));
    }
}
