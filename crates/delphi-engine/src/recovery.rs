//! Rebuilding in-flight work from the event log (`specs/005` §2.5).
//!
//! After any interruption (a closed terminal, a daemon crash, `SIGKILL`, a reboot) everything needed to
//! resume is already an event. This module reads a mission's log and reconstructs, for each span:
//! its completed turns, the tool calls still owed a result, the taint ceiling it had reached, and how
//! many times in a row its current turn has been interrupted. It is pure: no I/O, no model calls.
//!
//! Rules it implements:
//! * a turn counts only if its `agent.turn.complete` marker was persisted: a model call that was cut off
//!   leaves no partial turn behind, however many of its blocks reached the disk;
//! * tool results are owed per call; those already persisted are kept, the rest must be produced again;
//! * `done` and `escalated` spans are never touched.

use crate::handoff::HandoffEnvelope;
use delphi_adapter::blocks::{ContentBlock, Turn};
use delphi_core::event::EventEnvelope;
use delphi_core::provenance::Tier;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub enum NodeStatus {
    Running,
    Done,
    Escalated(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone)]
pub struct SpanState {
    pub span_id: String,
    pub task_id: String,
    pub role: String,
    pub depth: u32,
    pub parent_span: String,
    pub handoff: Option<HandoffEnvelope>,
    pub status: NodeStatus,
    pub ceiling: Tier,
    /// The span's section-tag nonce: restored so a resumed request is byte-identical to the one in flight.
    pub nonce: Option<String>,
    /// Complete turns only (assistant turns with their marker; user turns once every owed result exists).
    pub history: Vec<Turn>,
    /// Tool calls of the last assistant turn that have no result yet.
    pub pending: Vec<Call>,
    /// Results already persisted for the last turn's calls.
    pub known_results: BTreeMap<String, (String, bool)>,
    /// Consecutive interruptions of the turn in progress.
    pub interruptions: u32,
    /// Tool-call id → the child span it dispatched.
    pub dispatches: BTreeMap<String, String>,
}

impl SpanState {
    fn new(span_id: &str, task_id: &str, role: &str, depth: u32, parent: &str, ceiling: Tier) -> SpanState {
        SpanState { span_id: span_id.into(), task_id: task_id.into(), role: role.into(), depth, parent_span: parent.into(), handoff: None, status: NodeStatus::Running, ceiling, nonce: None, history: vec![], pending: vec![], known_results: BTreeMap::new(), interruptions: 0, dispatches: BTreeMap::new() }
    }

    pub fn is_open(&self) -> bool {
        self.status == NodeStatus::Running
    }

    /// What this span reported: its last assistant turn's text.
    pub fn last_text(&self) -> String {
        self.history
            .iter()
            .rev()
            .find(|t| t.role == delphi_adapter::blocks::Role::Assistant)
            .map(|t| t.blocks.iter().filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct TaskState {
    pub task_id: String,
    pub target: String,
    pub prompt: String,
    pub completed: bool,
    pub root_span: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Recovered {
    pub spans: BTreeMap<String, SpanState>,
    pub tasks: BTreeMap<String, TaskState>,
    /// Approvals requested but never resolved: their cards died with the daemon.
    pub open_approvals: Vec<String>,
}

fn tier(v: &Value) -> Option<Tier> {
    v.as_str().and_then(Tier::parse)
}

pub fn rebuild(events: &[EventEnvelope]) -> Recovered {
    let mut rec = Recovered::default();
    // (span, turn_index) → block_index → (block, endpoint, model)
    type Parts = BTreeMap<u64, (ContentBlock, String, String)>;
    let mut blocks: BTreeMap<(String, u32), Parts> = BTreeMap::new();
    let mut open_approvals: BTreeSet<String> = BTreeSet::new();
    for e in events {
        let p = &e.payload;
        let span = e.span_id.clone().unwrap_or_default();
        match e.kind.as_str() {
            "task.created" => {
                let tid = e.task_id.clone().unwrap_or_default();
                rec.tasks.insert(tid.clone(), TaskState { task_id: tid, target: p["target"].as_str().unwrap_or("").into(), prompt: p["prompt"].as_str().unwrap_or("").into(), completed: false, root_span: None });
            }
            // A recorded failure is as final as a completion: recovery never re-runs it (specs/005 §2.5).
            "task.completed" | "task.failed" => {
                if let Some(t) = e.task_id.as_ref().and_then(|id| rec.tasks.get_mut(id)) {
                    t.completed = true;
                }
            }
            "dag.node.added" => {
                let id = p["node_id"].as_str().unwrap_or("").to_string();
                let mut st = SpanState::new(&id, e.task_id.as_deref().unwrap_or(""), p["role"].as_str().unwrap_or(""), p["depth"].as_u64().unwrap_or(0) as u32, p["parent"].as_str().unwrap_or(""), tier(&p["ceiling"]).unwrap_or(Tier::L5));
                st.nonce = p["nonce"].as_str().map(String::from);
                if st.depth == 0 {
                    if let Some(t) = rec.tasks.get_mut(&st.task_id) {
                        t.root_span = Some(id.clone());
                    }
                }
                rec.spans.insert(id, st);
            }
            "handoff.sent" => {
                let handoff: Option<HandoffEnvelope> = serde_json::from_value(p["handoff"].clone()).ok();
                if let Some(h) = handoff {
                    // A dispatched child's handoff id is `ho_<parent span>_<tool call id>`.
                    if let Some(call_id) = h.handoff_id.strip_prefix(&format!("ho_{}_", h.parent_span_id)) {
                        if let Some(parent) = rec.spans.get_mut(&h.parent_span_id) {
                            parent.dispatches.insert(call_id.to_string(), span.clone());
                        }
                    }
                    if let Some(s) = rec.spans.get_mut(&span) {
                        s.handoff = Some(h);
                    }
                }
            }
            "dag.node.state" => {
                if let Some(s) = rec.spans.get_mut(p["node_id"].as_str().unwrap_or("")) {
                    s.status = match p["state"].as_str() {
                        Some("done") => NodeStatus::Done,
                        Some("escalated") => NodeStatus::Escalated(p["reason"].as_str().unwrap_or("").into()),
                        _ => NodeStatus::Running,
                    };
                }
            }
            "span.taint" => {
                if let (Some(s), Some(t)) = (rec.spans.get_mut(&span), tier(&p["ceiling"])) {
                    s.ceiling = s.ceiling.min(t);
                }
            }
            "agent.block.complete" => {
                if let Ok(b) = serde_json::from_value::<ContentBlock>(p["block"].clone()) {
                    blocks.entry((span, p["turn_index"].as_u64().unwrap_or(0) as u32)).or_default().insert(p["block_index"].as_u64().unwrap_or(0), (b, e.endpoint_id.clone().unwrap_or_default(), e.model_id.clone().unwrap_or_default()));
                }
            }
            "agent.turn.complete" => {
                let turn_index = p["turn_index"].as_u64().unwrap_or(0) as u32;
                let Some(s) = rec.spans.get_mut(&span) else { continue };
                let Some(parts) = blocks.remove(&(span.clone(), turn_index)) else { continue };
                let expected = p["blocks"].as_u64().unwrap_or(0) as usize;
                if parts.len() != expected {
                    continue; // a marker without all its blocks is not a complete turn
                }
                let (ep, model) = parts.values().next().map(|(_, e, m)| (e.clone(), m.clone())).unwrap_or_default();
                let list: Vec<ContentBlock> = parts.into_values().map(|(b, _, _)| b).collect();
                s.pending = if p["stop_reason"] == "tool_use" { list.iter().filter_map(|b| if let ContentBlock::ToolCall { id, name, arguments } = b { Some(Call { id: id.clone(), name: name.clone(), args: arguments.clone() }) } else { None }).collect() } else { vec![] };
                s.known_results.clear();
                s.interruptions = 0;
                s.history.push(Turn::assistant(turn_index, "anthropic", &ep, &model, list));
            }
            "tool.result" => {
                let Some(s) = rec.spans.get_mut(&span) else { continue };
                let id = p["tool_call_id"].as_str().unwrap_or("").to_string();
                if s.pending.iter().any(|c| c.id == id) {
                    s.known_results.insert(id, (p["content"].as_str().unwrap_or("").to_string(), p["is_error"].as_bool().unwrap_or(false)));
                }
                if !s.pending.is_empty() && s.pending.iter().all(|c| s.known_results.contains_key(&c.id)) {
                    let results: Vec<ContentBlock> = s.pending.iter().map(|c| {
                        let (content, is_error) = s.known_results[&c.id].clone();
                        ContentBlock::ToolResult { tool_call_id: c.id.clone(), content, is_error }
                    }).collect();
                    s.history.push(Turn::user(p["turn_index"].as_u64().unwrap_or(0) as u32, results));
                    s.pending.clear();
                    s.known_results.clear();
                }
            }
            "turn.interrupted" => {
                if let Some(s) = rec.spans.get_mut(&span) {
                    s.interruptions += 1;
                }
            }
            "approval.requested" => {
                open_approvals.insert(p["approval_id"].as_str().unwrap_or("").to_string());
            }
            "approval.resolved" => {
                open_approvals.remove(p["approval_id"].as_str().unwrap_or(""));
            }
            _ => {}
        }
    }
    rec.open_approvals = open_approvals.into_iter().collect();
    rec
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Log {
        events: Vec<EventEnvelope>,
    }

    impl Log {
        fn new() -> Log {
            Log { events: vec![] }
        }

        fn ev(&mut self, kind: &str, span: &str, payload: Value) {
            let mut e = EventEnvelope::new("m1", kind, payload);
            e.task_id = Some("t_0001".into());
            e.span_id = Some(span.into());
            e.agent = Some("architect".into());
            e.endpoint_id = Some("direct".into());
            e.model_id = Some("opus-x".into());
            e.seq = self.events.len() as u64 + 1;
            self.events.push(e);
        }

        fn start(&mut self, span: &str, depth: u32, parent: &str, handoff_id: &str) {
            self.ev("dag.node.added", span, json!({"node_id": span, "role": "architect", "parent": parent, "depth": depth, "ceiling": "L5"}));
            let mut h = HandoffEnvelope::root("m1", "t_0001", "architect", "do it", vec![]);
            h.handoff_id = handoff_id.into();
            h.parent_span_id = parent.into();
            self.ev("handoff.sent", span, json!({"handoff": h}));
            self.ev("dag.node.state", span, json!({"node_id": span, "state": "running"}));
        }

        fn turn(&mut self, span: &str, idx: u32, blocks: Vec<ContentBlock>, stop: &str, complete: bool) {
            for (i, b) in blocks.iter().enumerate() {
                self.ev("agent.block.complete", span, json!({"turn_index": idx, "block_index": i, "block": b}));
            }
            if complete {
                self.ev("agent.turn.complete", span, json!({"turn_index": idx, "stop_reason": stop, "blocks": blocks.len()}));
            }
        }
    }

    fn call(id: &str) -> ContentBlock {
        ContentBlock::ToolCall { id: id.into(), name: "read_file".into(), arguments: json!({"path": "x"}) }
    }

    fn text(t: &str) -> ContentBlock {
        ContentBlock::Text { text: t.into() }
    }

    #[test]
    fn complete_turns_and_their_results_rebuild_the_history() {
        let mut l = Log::new();
        l.ev("task.created", "", json!({"target": "architect"}));
        l.start("sp1", 0, "operator", "ho_t_0001");
        l.turn("sp1", 1, vec![call("c1"), call("c2")], "tool_use", true);
        l.ev("tool.result", "sp1", json!({"turn_index": 2, "tool_call_id": "c1", "name": "read_file", "content": "one", "is_error": false}));
        l.ev("tool.result", "sp1", json!({"turn_index": 2, "tool_call_id": "c2", "name": "read_file", "content": "two", "is_error": true}));
        l.turn("sp1", 3, vec![text("answer")], "end_turn", true);
        let r = rebuild(&l.events);
        let s = &r.spans["sp1"];
        assert_eq!(s.history.len(), 3);
        assert!(s.pending.is_empty() && s.is_open());
        match &s.history[1].blocks[..] {
            [ContentBlock::ToolResult { tool_call_id: a, content: ac, is_error: false }, ContentBlock::ToolResult { tool_call_id: b, is_error: true, .. }] => assert_eq!((a.as_str(), ac.as_str(), b.as_str()), ("c1", "one", "c2")),
            other => panic!("{:?}", other),
        }
        assert_eq!(s.last_text(), "answer");
        assert_eq!(r.tasks["t_0001"].root_span.as_deref(), Some("sp1"));
        assert_eq!(s.history[0].endpoint_id.as_deref(), Some("direct"), "assistant turns remember where they were produced");
    }

    #[test]
    fn a_turn_without_its_marker_is_discarded_however_many_blocks_were_written() {
        let mut l = Log::new();
        l.start("sp1", 0, "operator", "ho_t_0001");
        l.turn("sp1", 1, vec![text("first"), text("second")], "end_turn", false);
        let s = &rebuild(&l.events).spans["sp1"].clone();
        assert!(s.history.is_empty() && s.pending.is_empty());
        // A marker that promises more blocks than were persisted is no better.
        let mut l = Log::new();
        l.start("sp1", 0, "operator", "ho_t_0001");
        l.ev("agent.block.complete", "sp1", json!({"turn_index": 1, "block_index": 0, "block": text("a")}));
        l.ev("agent.turn.complete", "sp1", json!({"turn_index": 1, "stop_reason": "end_turn", "blocks": 2}));
        assert!(rebuild(&l.events).spans["sp1"].history.is_empty());
    }

    #[test]
    fn calls_without_results_stay_pending_and_persisted_results_are_kept() {
        let mut l = Log::new();
        l.start("sp1", 0, "operator", "ho_t_0001");
        l.turn("sp1", 1, vec![call("c1"), call("c2")], "tool_use", true);
        l.ev("tool.result", "sp1", json!({"turn_index": 2, "tool_call_id": "c1", "name": "read_file", "content": "one", "is_error": false}));
        let s = rebuild(&l.events).spans["sp1"].clone();
        assert_eq!(s.history.len(), 1, "no user turn until every owed result exists");
        assert_eq!(s.pending.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["c1", "c2"]);
        assert_eq!(s.known_results.keys().collect::<Vec<_>>(), vec!["c1"]);
    }

    #[test]
    fn interruptions_count_until_a_turn_completes() {
        let mut l = Log::new();
        l.start("sp1", 0, "operator", "ho_t_0001");
        l.ev("turn.interrupted", "sp1", json!({"turn_index": 1}));
        l.ev("turn.interrupted", "sp1", json!({"turn_index": 1, "recovered": true}));
        assert_eq!(rebuild(&l.events).spans["sp1"].interruptions, 2);
        l.turn("sp1", 1, vec![text("ok")], "end_turn", true);
        assert_eq!(rebuild(&l.events).spans["sp1"].interruptions, 0);
    }

    #[test]
    fn taint_is_restored_and_only_ever_goes_down() {
        let mut l = Log::new();
        l.start("sp1", 0, "operator", "ho_t_0001");
        l.ev("span.taint", "sp1", json!({"ceiling": "L2"}));
        l.ev("span.taint", "sp1", json!({"ceiling": "L0"}));
        l.ev("span.taint", "sp1", json!({"ceiling": "L3"}));
        assert_eq!(rebuild(&l.events).spans["sp1"].ceiling, Tier::L0);
    }

    #[test]
    fn dispatch_links_and_final_states_are_found() {
        let mut l = Log::new();
        l.ev("task.created", "", json!({"target": "orchestrator"}));
        l.start("sp_orch_1", 0, "operator", "ho_t_0001");
        l.turn("sp_orch_1", 1, vec![call("d1")], "tool_use", true);
        l.start("sp_arch_2", 1, "sp_orch_1", "ho_sp_orch_1_d1");
        l.turn("sp_arch_2", 1, vec![text("child report")], "end_turn", true);
        l.ev("dag.node.state", "sp_arch_2", json!({"node_id": "sp_arch_2", "state": "done"}));
        let r = rebuild(&l.events);
        assert_eq!(r.spans["sp_orch_1"].dispatches["d1"], "sp_arch_2");
        assert_eq!(r.spans["sp_arch_2"].status, NodeStatus::Done);
        assert_eq!(r.spans["sp_arch_2"].last_text(), "child report");
        assert!(r.spans["sp_orch_1"].is_open() && r.spans["sp_orch_1"].pending.len() == 1);
    }

    #[test]
    fn escalated_spans_and_open_approvals_are_reported() {
        let mut l = Log::new();
        l.start("sp1", 0, "operator", "ho_t_0001");
        l.ev("approval.requested", "sp1", json!({"approval_id": "ap_1"}));
        l.ev("approval.requested", "sp1", json!({"approval_id": "ap_2"}));
        l.ev("approval.resolved", "sp1", json!({"approval_id": "ap_1", "resolution": "approved"}));
        l.ev("dag.node.state", "sp1", json!({"node_id": "sp1", "state": "escalated", "reason": "budget"}));
        let r = rebuild(&l.events);
        assert_eq!(r.open_approvals, vec!["ap_2"]);
        assert_eq!(r.spans["sp1"].status, NodeStatus::Escalated("budget".into()));
        assert!(!r.spans["sp1"].is_open());
    }
}
