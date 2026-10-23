//! `missions/<id>/status.md`: a human-readable projection of where a mission stands (`specs/005` §2.5).
//!
//! It is regenerated from the event log after every node state change, so an operator or a teammate can
//! see the goal, progress, what is waiting on them, the spend and the last recovery without a running
//! daemon. It is a projection: Delphi never reads it back as truth.

use delphi_core::event::EventEnvelope;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Status {
    mission_id: String,
    goal: String,
    tasks: BTreeMap<String, (String, String)>,
    spans: BTreeMap<String, SpanRow>,
    approvals: BTreeMap<String, (String, String)>,
    last_recovery: Option<String>,
    updated: String,
}

struct SpanRow {
    task: String,
    role: String,
    depth: u64,
    state: String,
}

/// Event kinds that change what the projection shows.
pub fn affects_status(kind: &str) -> bool {
    matches!(kind, "task.created" | "task.completed" | "task.failed" | "dag.node.added" | "dag.node.state" | "approval.requested" | "approval.resolved" | "mission.recovered" | "budget.cap" | "agent.usage")
}

impl Status {
    pub fn new(mission_id: &str) -> Status {
        Status { mission_id: mission_id.into(), ..Status::default() }
    }

    pub fn apply(&mut self, e: &EventEnvelope) {
        let p = &e.payload;
        self.updated = e.timestamp.clone();
        let task = e.task_id.clone().unwrap_or_default();
        match e.kind.as_str() {
            "task.created" => {
                if self.goal.is_empty() {
                    self.goal = p["prompt"].as_str().unwrap_or("").chars().take(300).collect();
                }
                self.tasks.insert(task, (p["target"].as_str().unwrap_or("?").to_string(), "running".into()));
            }
            "task.completed" => {
                let state = match p["status"].as_str() {
                    Some("escalated") => format!("needs you: {}", p["reason"].as_str().unwrap_or("")),
                    _ => "done".to_string(),
                };
                if let Some(t) = self.tasks.get_mut(&task) {
                    t.1 = state;
                }
            }
            "task.failed" => {
                if let Some(t) = self.tasks.get_mut(&task) {
                    t.1 = "failed".into();
                }
            }
            "dag.node.added" => {
                let id = p["node_id"].as_str().unwrap_or("").to_string();
                self.spans.insert(id, SpanRow { task, role: p["role"].as_str().unwrap_or("?").into(), depth: p["depth"].as_u64().unwrap_or(0), state: "running".into() });
            }
            "dag.node.state" => {
                if let Some(s) = self.spans.get_mut(p["node_id"].as_str().unwrap_or("")) {
                    s.state = match p["state"].as_str() {
                        Some("escalated") => format!("escalated: {}", p["reason"].as_str().unwrap_or("")),
                        Some(other) => other.to_string(),
                        None => "?".into(),
                    };
                }
            }
            "approval.requested" => {
                self.approvals.insert(p["approval_id"].as_str().unwrap_or("").to_string(), (p["tier"].as_str().unwrap_or("").to_string(), p["summary"].as_str().unwrap_or("").to_string()));
            }
            "approval.resolved" => {
                self.approvals.remove(p["approval_id"].as_str().unwrap_or(""));
            }
            "mission.recovered" => {
                let n = p["tasks"].as_array().map(|a| a.len()).unwrap_or(0);
                self.last_recovery = Some(format!("{} — {} task(s) to resume; ${:.4} charged for calls interrupted by the restart; {} approval card(s) expired and will be asked again.", e.timestamp, n, p["charged_usd"].as_f64().unwrap_or(0.0), p["expired_approvals"].as_array().map(|a| a.len()).unwrap_or(0)));
            }
            _ => {}
        }
    }

    /// The Markdown, with the spend figures supplied by the caller (they come from the budget, itself
    /// derived from the same log).
    pub fn render(&self, cap_usd: f64, spent_usd: f64, reserved_usd: f64) -> String {
        let mut s = format!(
            "---\nid: status-{id}\ntitle: \"Status: {id}\"\nauthor: orchestrator\ntimestamp: {ts}\nsource_type: internal_generated\nconfidence: high\nstatus: active\nsource_mission: {id}\ntags: [status, generated]\n---\n\n# Mission `{id}`\n\n> Generated from the event log after every change. Do not edit: Delphi never reads it back, and the next change overwrites it.\n\n## Goal\n{goal}\n\n## Progress\n",
            id = self.mission_id,
            ts = if self.updated.is_empty() { "1970-01-01T00:00:00.000Z" } else { &self.updated },
            goal = if self.goal.is_empty() { "(no task yet)".to_string() } else { self.goal.replace('\n', " ") }
        );
        if self.tasks.is_empty() {
            s.push_str("(nothing yet)\n");
        }
        for (tid, (target, state)) in &self.tasks {
            s.push_str(&format!("- **{}** → {}: {}\n", tid, target, state));
            for (sid, row) in self.spans.iter().filter(|(_, r)| &r.task == tid) {
                s.push_str(&format!("{}- `{}` ({}): {}\n", "  ".repeat(row.depth as usize + 1), sid, row.role, row.state));
            }
        }
        s.push_str("\n## Waiting on you\n");
        if self.approvals.is_empty() {
            s.push_str("nothing\n");
        }
        for (id, (tier, summary)) in &self.approvals {
            s.push_str(&format!("- approval `{}` [{}]: {}\n", id, tier, summary));
        }
        s.push_str(&format!("\n## Spend\n${:.4} of a ${:.2} ceiling (${:.4} reserved)\n\n## Last recovery\n{}\n", spent_usd, cap_usd, reserved_usd, self.last_recovery.as_deref().unwrap_or("none")));
        s
    }
}

/// Write the projection without fsync (it can always be regenerated): temp file then rename.
pub fn write_projection(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(kind: &str, task: &str, payload: serde_json::Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", kind, payload);
        e.task_id = Some(task.into());
        e.timestamp = "2026-10-24T00:00:00.000Z".into();
        e
    }

    #[test]
    fn the_projection_shows_goal_progress_what_waits_on_you_and_spend() {
        let mut s = Status::new("m1");
        s.apply(&ev("task.created", "t_0001", json!({"prompt": "Plan the retry ladder", "target": "orchestrator"})));
        s.apply(&ev("dag.node.added", "t_0001", json!({"node_id": "sp_o_1", "role": "orchestrator", "depth": 0})));
        s.apply(&ev("dag.node.added", "t_0001", json!({"node_id": "sp_a_2", "role": "architect", "depth": 1})));
        s.apply(&ev("approval.requested", "t_0001", json!({"approval_id": "ap_1", "tier": "amber", "summary": "write specs/009.md (12 bytes)"})));
        let text = s.render(5.0, 0.0312, 0.01);
        for want in ["# Mission `m1`", "Plan the retry ladder", "**t_0001** → orchestrator: running", "`sp_a_2` (architect): running", "approval `ap_1` [amber]: write specs/009.md", "$0.0312 of a $5.00 ceiling ($0.0100 reserved)", "author: orchestrator"] {
            assert!(text.contains(want), "missing `{}` in:\n{}", want, text);
        }
        s.apply(&ev("approval.resolved", "t_0001", json!({"approval_id": "ap_1", "resolution": "approved"})));
        s.apply(&ev("dag.node.state", "t_0001", json!({"node_id": "sp_a_2", "state": "done"})));
        s.apply(&ev("task.completed", "t_0001", json!({"status": "done", "reason": ""})));
        let text = s.render(5.0, 0.05, 0.0);
        assert!(text.contains("**t_0001** → orchestrator: done") && text.contains("`sp_a_2` (architect): done") && text.contains("## Waiting on you\nnothing"), "{}", text);
    }

    #[test]
    fn escalations_and_recoveries_are_visible() {
        let mut s = Status::new("m1");
        s.apply(&ev("task.created", "t_0001", json!({"prompt": "x", "target": "architect"})));
        s.apply(&ev("task.completed", "t_0001", json!({"status": "escalated", "reason": "mission budget"})));
        s.apply(&ev("mission.recovered", "", json!({"tasks": [{}, {}], "charged_usd": 0.25, "expired_approvals": ["ap_9"]})));
        let text = s.render(5.0, 0.25, 0.0);
        assert!(text.contains("needs you: mission budget") && text.contains("2 task(s) to resume") && text.contains("$0.2500 charged") && text.contains("1 approval card(s) expired"), "{}", text);
    }

    #[test]
    fn only_relevant_events_trigger_a_rewrite_and_the_frontmatter_satisfies_the_linter() {
        assert!(affects_status("dag.node.state") && affects_status("approval.requested") && !affects_status("tool.result") && !affects_status("agent.block.complete"));
        let text = Status::new("m1").render(5.0, 0.0, 0.0);
        assert!(text.starts_with("---\nid: status-m1\n") && text.contains("source_type: internal_generated") && text.contains("(nothing yet)"));
    }

    #[test]
    fn the_projection_is_written_atomically_without_leaving_temp_files() {
        let t = tempfile::tempdir().unwrap();
        write_projection(&t.path().join("status.md"), "one").unwrap();
        write_projection(&t.path().join("status.md"), "two").unwrap();
        assert_eq!(std::fs::read_to_string(t.path().join("status.md")).unwrap(), "two");
        assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 1);
    }
}
