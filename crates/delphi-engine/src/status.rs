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
    /// `endpoint/model` -> why it is out and when it returns (cleared when it recovers).
    models_out: BTreeMap<String, String>,
    /// The latest substitutions, confirmations and homogeneous reviews, oldest first (kept short).
    model_notes: Vec<String>,
}

struct SpanRow {
    task: String,
    role: String,
    depth: u64,
    state: String,
}

/// Event kinds that change what the projection shows.
pub fn affects_status(kind: &str) -> bool {
    matches!(kind, "task.created" | "task.completed" | "task.failed" | "task.cancelled" | "dag.node.added" | "dag.node.state" | "approval.requested" | "approval.resolved" | "mission.recovered" | "budget.cap" | "budget.reserve" | "budget.release" | "agent.usage" | "model.exhausted" | "model.recovered" | "model.promoted" | "model.degraded_confirm" | "model.homogeneous" | "task.blocked" | "task.unblocked")
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
            "task.cancelled" => {
                if let Some(t) = self.tasks.get_mut(&task) {
                    t.1 = "cancelled".into();
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
            "model.exhausted" => {
                let when = p["reset_at"].as_i64().map(|t| format!("; back at {}", delphi_core::time::format_unix_millis(t, 0).trim_end_matches(".000Z").replace('T', " "))).unwrap_or_default();
                self.models_out.insert(format!("{}/{}", p["endpoint_id"].as_str().unwrap_or("?"), p["model_id"].as_str().unwrap_or("?")), format!("{}{}", p["reason"].as_str().unwrap_or("out of quota"), when));
            }
            "model.recovered" => {
                self.models_out.remove(&format!("{}/{}", p["endpoint_id"].as_str().unwrap_or("?"), p["model_id"].as_str().unwrap_or("?")));
            }
            "model.promoted" => {
                let side = |k: &str| format!("{}/{}", p[k]["endpoint_id"].as_str().unwrap_or("?"), p[k]["model_id"].as_str().unwrap_or("?"));
                self.note(format!("{}: ran on {} instead of {} ({})", task, side("to"), side("from"), p["reason"].as_str().unwrap_or("").chars().take(120).collect::<String>()));
            }
            "model.degraded_confirm" => {
                let sub = format!("{}/{}", p["substitute"]["endpoint_id"].as_str().unwrap_or("?"), p["substitute"]["model_id"].as_str().unwrap_or("?"));
                match p["state"].as_str() {
                    Some("confirmed") => self.note(format!("{}: you confirmed the weaker model {}", task, sub)),
                    Some("declined") => self.note(format!("{}: you declined the weaker model {}; waiting for the target", task, sub)),
                    _ => {}
                }
            }
            "model.homogeneous" => {
                self.note(format!("{}: no model outside {} was usable, so this review is by the same family as the author", task, p["avoided"]));
            }
            "task.blocked" => {
                if let Some(t) = self.tasks.get_mut(&task) {
                    t.1 = format!("waiting for a model: {}", p["why"].as_str().unwrap_or("").chars().take(160).collect::<String>());
                }
            }
            "task.unblocked" => {
                if let Some(t) = self.tasks.get_mut(&task) {
                    if t.1.starts_with("waiting for a model") {
                        t.1 = "running".into();
                    }
                }
            }
            "mission.recovered" => {
                let n = p["tasks"].as_array().map(|a| a.len()).unwrap_or(0);
                self.last_recovery = Some(format!("{} — {} task(s) to resume; ${:.4} charged for calls interrupted by the restart; {} approval card(s) expired and will be asked again.", e.timestamp, n, p["charged_usd"].as_f64().unwrap_or(0.0), p["expired_approvals"].as_array().map(|a| a.len()).unwrap_or(0)));
            }
            _ => {}
        }
    }

    fn note(&mut self, line: String) {
        self.model_notes.push(line);
        if self.model_notes.len() > 10 {
            self.model_notes.remove(0);
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
        s.push_str("\n## Models\n");
        if self.models_out.is_empty() && self.model_notes.is_empty() {
            s.push_str("all configured models were available\n");
        }
        for (pair, why) in &self.models_out {
            s.push_str(&format!("- out of quota: `{}` ({})\n", pair, why));
        }
        for note in &self.model_notes {
            s.push_str(&format!("- {}\n", note));
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
    fn the_projection_shows_which_models_are_out_what_was_substituted_and_a_task_waiting_for_a_model() {
        let mut st = Status::new("m1");
        st.apply(&ev("task.created", "t_0001", json!({"prompt": "Plan the change", "target": "architect"})));
        let plain = st.render(5.0, 0.0, 0.0);
        assert!(plain.contains("## Models\nall configured models were available"), "{}", plain);
        st.apply(&ev("model.exhausted", "t_0001", json!({"endpoint_id": "direct", "model_id": "opus-x", "reason": "quota reached", "reset_at": 1_792_864_800})));
        st.apply(&ev("model.promoted", "t_0001", json!({"from": {"endpoint_id": "direct", "model_id": "opus-x"}, "to": {"endpoint_id": "backup", "model_id": "opus-x"}, "reason": "exhausted: quota reached"})));
        st.apply(&ev("model.degraded_confirm", "t_0001", json!({"state": "asked", "detail": {}})));
        st.apply(&ev("model.degraded_confirm", "t_0001", json!({"state": "confirmed", "substitute": {"endpoint_id": "direct", "model_id": "sonnet-x"}})));
        st.apply(&ev("task.blocked", "t_0001", json!({"reason": "no_model", "back_at": 1_792_864_800, "why": "everything is throttled"})));
        let md = st.render(5.0, 0.0, 0.0);
        assert!(md.contains("- out of quota: `direct/opus-x` (quota reached; back at 2026-10-24 18:00:00)"), "{}", md);
        assert!(md.contains("t_0001: ran on backup/opus-x instead of direct/opus-x") && md.contains("t_0001: you confirmed the weaker model direct/sonnet-x"), "{}", md);
        assert!(md.contains("**t_0001** → architect: waiting for a model: everything is throttled"), "{}", md);
        // It all clears on its own as events say so.
        st.apply(&ev("model.recovered", "t_0001", json!({"endpoint_id": "direct", "model_id": "opus-x"})));
        st.apply(&ev("task.unblocked", "t_0001", json!({"resumed": true})));
        let after = st.render(5.0, 0.0, 0.0);
        assert!(!after.contains("out of quota:") && after.contains("architect: running") && after.contains("you confirmed the weaker model"), "{}", after);
        for kind in ["model.exhausted", "model.recovered", "model.promoted", "model.degraded_confirm", "model.homogeneous", "task.blocked", "task.unblocked"] {
            assert!(affects_status(kind), "{} should rewrite status.md", kind);
        }
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
