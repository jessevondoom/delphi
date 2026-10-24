//! What the Narrator shows (`specs/005` §4, `specs/005b` §4), derived from the event stream.
//!
//! The view model is a pure fold over event envelopes. The daemon serves recent history as events
//! (`observe.snapshot`), the Narrator applies live events on top with the same [`ObserverState::apply`],
//! so a window that is killed and reopened rebuilds exactly what it had. Nothing here starts work or
//! approves anything: it only reads.
//!
//! Text that comes from models or tools is shown, never obeyed; the renderer strips control sequences
//! with [`sanitize`] before it reaches a terminal.

use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};

/// Event kinds the Narrator consumes. The snapshot is filtered to these so it stays small.
pub const KINDS: [&str; 26] = [
    "task.created", "task.output", "task.completed", "task.failed", "dag.node.added", "dag.node.state", "agent.usage", "approval.requested", "approval.resolved", "span.taint", "build.attempt", "gates.evaluated", "audit.recorded", "merge.approved", "worktree.created", "action.result", "gate.flaky",
    "council.started", "council.barrier.released", "council.stance", "council.critique", "council.verdict", "endpoint.throttled", "harness.fallback", "model.exhausted", "model.recovered",
];

const NARRATIVE_CAP: usize = 300;
const TEXT_CAP: usize = 6000;

/// A model's words made safe to print: control characters and escape sequences removed, newlines and tabs kept.
pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                // CSI (`ESC [ … final`) and OSC (`ESC ] … BEL|ST`) sequences are dropped whole; any other ESC with its next char.
                match chars.peek() {
                    Some('[') => {
                        chars.next();
                        for n in chars.by_ref() {
                            if ('\x40'..='\x7e').contains(&n) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        chars.next();
                        while let Some(n) = chars.next() {
                            if n == '\x07' || (n == '\x1b' && chars.peek() == Some(&'\\')) {
                                if n == '\x1b' {
                                    chars.next();
                                }
                                break;
                            }
                        }
                    }
                    Some(_) => {
                        chars.next();
                    }
                    None => {}
                }
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() || ('\u{202a}'..='\u{202e}').contains(&c) || ('\u{2066}'..='\u{2069}').contains(&c) => {} // controls and bidi overrides
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tokens {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
    pub thinking: u64,
}

impl Tokens {
    fn add(&mut self, p: &Value) {
        let n = |k: &str| p[k].as_u64().unwrap_or(0);
        self.input += n("input_tokens");
        self.cache_read += n("cache_read_tokens");
        self.cache_write += n("cache_write_tokens");
        self.output += n("output_tokens");
        self.thinking += n("thinking_tokens");
    }

    pub fn total(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output + self.thinking
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentRow {
    pub role: String,
    /// `idle`, `running`, `waiting on you`, `escalated`.
    pub state: String,
    pub task_id: String,
    pub endpoint: String,
    pub tier: String,
    pub model: String,
    /// The call before this one was on a lower tier (an upscale on the retry ladder).
    pub upscaled: bool,
    pub tokens: Tokens,
    pub cost_usd: f64,
    pub calls: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    pub id: String,
    pub role: String,
    pub parent: Option<String>,
    pub depth: u64,
    pub task_id: String,
    pub state: String,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Task {
    pub id: String,
    pub flow: String,
    pub prompt: String,
    /// `running`, `done`, `escalated`, `failed`.
    pub status: String,
    pub output: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seat {
    pub role: String,
    pub family: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stance {
    pub role: String,
    pub answered: bool,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Critique {
    pub role: String,
    pub recommendation: String,
    pub concur: Option<bool>,
    pub findings: Vec<(String, String)>,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Council {
    pub task_id: String,
    pub proposition: String,
    pub seats: Vec<Seat>,
    pub homogeneous: bool,
    /// `round 1`, `round 2`, `verdict`, `escalated`.
    pub phase: String,
    pub stances: Vec<Stance>,
    pub critiques: Vec<Critique>,
    pub escalation: Option<String>,
    pub path: String,
    pub unresolved: Vec<(String, String, String)>,
    pub options: Vec<(String, Vec<String>)>,
    pub synthesis: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GateRow {
    pub name: String,
    pub state: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Build {
    pub task_id: String,
    pub branch: String,
    pub attempts: Vec<String>,
    pub sha: String,
    pub gates: Vec<GateRow>,
    pub findings: Vec<(String, String)>,
    pub flaky: bool,
    pub merge: String,
}

#[derive(Debug, Clone, Default)]
pub struct ObserverState {
    pub last_seq: u64,
    pub agents: BTreeMap<String, AgentRow>,
    pub nodes: BTreeMap<String, Node>,
    pub tasks: BTreeMap<String, Task>,
    pub councils: Vec<Council>,
    pub builds: Vec<Build>,
    pub pending_approvals: BTreeMap<String, String>,
    pub spent_usd: f64,
    pub tokens: Tokens,
    pub retries: u64,
    pub narrative: VecDeque<String>,
    /// Cost of each model call, oldest first (the spend sparkline); the newest 240.
    pub series: VecDeque<f64>,
    /// Live text per span while it streams (cleared when the node finishes).
    pub live: BTreeMap<String, String>,
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

impl ObserverState {
    pub fn new(roles: &[String]) -> ObserverState {
        let mut st = ObserverState::default();
        for r in roles {
            st.agents.insert(r.clone(), AgentRow { role: r.clone(), state: "idle".into(), ..Default::default() });
        }
        st
    }

    fn say(&mut self, line: String) {
        self.narrative.push_back(line);
        while self.narrative.len() > NARRATIVE_CAP {
            self.narrative.pop_front();
        }
    }

    fn council_mut(&mut self, task: &str) -> Option<&mut Council> {
        self.councils.iter_mut().rev().find(|c| c.task_id == task)
    }

    fn build_mut(&mut self, task: &str) -> &mut Build {
        if let Some(i) = self.builds.iter().rposition(|b| b.task_id == task) {
            return &mut self.builds[i];
        }
        self.builds.push(Build { task_id: task.into(), ..Default::default() });
        self.builds.last_mut().expect("just pushed")
    }

    /// Text streaming from a span (a `delta.frame`): shown live, bounded.
    pub fn apply_frame(&mut self, params: &Value) {
        let Some(spans) = params["spans"].as_object() else { return };
        for (id, v) in spans {
            let t = v["text"].as_str().unwrap_or("");
            if t.is_empty() {
                continue;
            }
            let buf = self.live.entry(id.clone()).or_default();
            buf.push_str(t);
            if buf.len() > TEXT_CAP {
                let cut = buf.len() - TEXT_CAP;
                let at = (cut..buf.len()).find(|i| buf.is_char_boundary(*i)).unwrap_or(buf.len());
                buf.drain(..at);
            }
        }
    }

    /// Fold one event envelope (as JSON) into the view. Events at or below `last_seq` are ignored, so a
    /// snapshot and the live stream can overlap safely.
    pub fn apply(&mut self, e: &Value) {
        let seq = e["seq"].as_u64().unwrap_or(0);
        if seq != 0 {
            if seq <= self.last_seq {
                return;
            }
            self.last_seq = seq;
        }
        let kind = e["type"].as_str().unwrap_or("");
        let p = &e["payload"];
        let task = s(&e["task_id"]);
        let role = s(&e["agent"]);
        match kind {
            "task.created" => {
                let flow = p["flow"].as_str().or(p["target"].as_str()).unwrap_or("task").to_string();
                let prompt = sanitize(p["prompt"].as_str().unwrap_or("")).chars().take(300).collect::<String>();
                self.say(format!("{} started ({}): {}", task, flow, prompt.chars().take(80).collect::<String>()));
                self.tasks.insert(task.clone(), Task { id: task, flow, prompt, status: "running".into(), output: String::new() });
            }
            "task.output" => {
                if let Some(t) = self.tasks.get_mut(&task) {
                    t.output = sanitize(p["text"].as_str().unwrap_or("")).chars().take(TEXT_CAP).collect();
                }
                if let Some(c) = self.council_mut(&task) {
                    c.synthesis = sanitize(p["text"].as_str().unwrap_or("")).chars().take(TEXT_CAP).collect();
                }
            }
            "task.completed" => {
                let st = s(&p["status"]);
                if let Some(t) = self.tasks.get_mut(&task) {
                    t.status = if st.is_empty() { "done".into() } else { st.clone() };
                }
                self.say(format!("{} {}", task, if st == "escalated" { "needs you".to_string() } else { "finished".to_string() }));
            }
            "task.failed" => {
                if let Some(t) = self.tasks.get_mut(&task) {
                    t.status = "failed".into();
                }
                self.say(format!("{} failed", task));
            }
            "dag.node.added" => {
                let id = s(&p["node_id"]);
                let r = s(&p["role"]);
                self.nodes.insert(id.clone(), Node { id: id.clone(), role: r.clone(), parent: p["parent"].as_str().filter(|x| !x.is_empty() && *x != "operator").map(String::from), depth: p["depth"].as_u64().unwrap_or(0), task_id: task.clone(), state: "pending".into(), ..Default::default() });
                if let Some(a) = self.agents.get_mut(&r) {
                    a.state = "running".into();
                    a.task_id = task;
                }
            }
            "dag.node.state" => {
                let id = s(&p["node_id"]);
                let st = s(&p["state"]);
                let mut r = String::new();
                if let Some(n) = self.nodes.get_mut(&id) {
                    n.state = st.clone();
                    r = n.role.clone();
                }
                if st == "done" || st == "escalated" {
                    self.live.remove(&id);
                    if let Some(a) = self.agents.get_mut(&r) {
                        // Another node of the same role may still be running.
                        let busy = self.nodes.values().any(|n| n.role == r && n.state == "running");
                        a.state = if busy { "running".into() } else if st == "escalated" { "escalated".into() } else { "idle".into() };
                    }
                }
                if !r.is_empty() {
                    self.say(format!("{} is {}", r, st));
                }
            }
            "agent.usage" => {
                let cost = p["cost_usd"].as_f64().unwrap_or(0.0);
                self.spent_usd += cost;
                self.tokens.add(p);
                self.series.push_back(cost);
                while self.series.len() > 240 {
                    self.series.pop_front();
                }
                let (tier, model, ep) = (s(&e["model_tier"]), s(&e["model_id"]), s(&e["endpoint_id"]));
                let span = s(&e["span_id"]);
                if let Some(n) = self.nodes.get_mut(&span) {
                    n.cost_usd += cost;
                    n.tokens += p["input_tokens"].as_u64().unwrap_or(0) + p["output_tokens"].as_u64().unwrap_or(0);
                }
                let r = if role.is_empty() { self.nodes.get(&span).map(|n| n.role.clone()).unwrap_or_default() } else { role };
                if let Some(a) = self.agents.get_mut(&r) {
                    let rank = |t: &str| match t {
                        "fast" => 0,
                        "workhorse" => 1,
                        "frontier" => 2,
                        _ => -1,
                    };
                    a.upscaled = !a.tier.is_empty() && rank(&tier) > rank(&a.tier);
                    if a.upscaled {
                        self.retries += 1;
                    }
                    if !tier.is_empty() {
                        a.tier = tier;
                    }
                    if !model.is_empty() {
                        a.model = model;
                    }
                    if !ep.is_empty() {
                        a.endpoint = ep;
                    }
                    a.tokens.add(p);
                    a.cost_usd += cost;
                    a.calls += 1;
                }
            }
            "approval.requested" => {
                let id = s(&p["approval_id"]);
                self.pending_approvals.insert(id, s(&p["summary"]));
                if let Some(a) = self.agents.get_mut(&s(&p["role"])) {
                    a.state = "waiting on you".into();
                }
                self.say(format!("waiting on you: {}", sanitize(p["summary"].as_str().unwrap_or("")).chars().take(80).collect::<String>()));
            }
            "approval.resolved" => {
                self.pending_approvals.remove(&s(&p["approval_id"]));
                for a in self.agents.values_mut().filter(|a| a.state == "waiting on you") {
                    a.state = "running".into();
                }
            }
            "build.attempt" => {
                let line = format!("attempt {} · {} · {}{}", p["attempt"].as_u64().unwrap_or(0), p["tier"].as_str().unwrap_or("?"), p["family"].as_str().unwrap_or("?"), if p["homogeneous_retry"] == true { " (same family)" } else { "" });
                if p["attempt"].as_u64().unwrap_or(1) > 1 {
                    self.say(format!("{}: retrying ({})", task, line));
                }
                self.build_mut(&task).attempts.push(line);
            }
            "worktree.created" => {
                self.build_mut(&task).branch = s(&p["branch"]);
            }
            "gates.evaluated" => {
                let b = self.build_mut(&task);
                b.sha = s(&p["sha"]).chars().take(12).collect();
                b.gates = p["gates"].as_array().map(|a| a.iter().map(|g| GateRow { name: s(&g["name"]), state: s(&g["state"]), detail: sanitize(g["detail"].as_str().unwrap_or("")).chars().take(300).collect() }).collect()).unwrap_or_default();
            }
            "gate.flaky" => {
                self.build_mut(&task).flaky = true;
            }
            "audit.recorded" => {
                let b = self.build_mut(&task);
                b.findings = p["findings"].as_array().map(|a| a.iter().map(|f| (s(&f["severity"]), sanitize(f["title"].as_str().unwrap_or("")))).collect()).unwrap_or_default();
            }
            "merge.approved" => {
                self.build_mut(&task).merge = "approved".into();
            }
            "action.result" => {
                if s(&p["action_id"]).starts_with("merge-") || s(&p["action_id"]).starts_with("pull_request-") {
                    let ok = p["ok"] == true;
                    self.build_mut(&task).merge = if ok { "merged".into() } else { "refused".into() };
                }
            }
            "council.started" => {
                let seats = p["seats"].as_array().map(|a| a.iter().map(|x| Seat { role: s(&x["role"]), family: s(&x["family"]) }).collect()).unwrap_or_default();
                self.councils.push(Council { task_id: task.clone(), proposition: sanitize(p["proposition"].as_str().unwrap_or("")).chars().take(600).collect(), seats, homogeneous: p["homogeneous"] == true, phase: "round 1".into(), ..Default::default() });
                self.say(format!("{}: council convened", task));
            }
            "council.barrier.released" => {
                if let Some(c) = self.council_mut(&task) {
                    c.phase = "round 2".into();
                }
                self.say(format!("{}: round 1 is in", task));
            }
            "council.stance" => {
                let st = Stance { role: s(&p["role"]), answered: p["status"] == "answered", text: sanitize(p["text"].as_str().unwrap_or("")).chars().take(TEXT_CAP).collect() };
                if let Some(c) = self.council_mut(&task) {
                    c.stances.push(st);
                }
            }
            "council.critique" => {
                let cr = Critique {
                    role: s(&p["role"]),
                    recommendation: s(&p["recommendation"]),
                    concur: p["concur"].as_bool(),
                    findings: p["findings"].as_array().map(|a| a.iter().map(|f| (s(&f["severity"]), sanitize(f["title"].as_str().unwrap_or("")))).collect()).unwrap_or_default(),
                    text: sanitize(p["text"].as_str().unwrap_or("")).chars().take(TEXT_CAP).collect(),
                };
                if let Some(c) = self.council_mut(&task) {
                    c.critiques.push(cr);
                }
            }
            "council.verdict" => {
                let esc = p["escalated"].as_str().map(String::from);
                let line = format!("{}: council {}", task, if esc.is_some() { "needs your decision" } else { "reached a verdict" });
                if let Some(c) = self.council_mut(&task) {
                    c.phase = if esc.is_some() { "escalated".into() } else { "verdict".into() };
                    c.escalation = esc;
                    c.path = s(&p["path"]);
                    c.unresolved = p["unresolved"].as_array().map(|a| a.iter().map(|u| (s(&u["severity"]), s(&u["seat"]), sanitize(u["title"].as_str().unwrap_or("")))).collect()).unwrap_or_default();
                    c.options = p["options"].as_array().map(|a| a.iter().map(|o| (s(&o["label"]), o["seats"].as_array().map(|x| x.iter().map(s).collect()).unwrap_or_default())).collect()).unwrap_or_default();
                }
                self.say(line);
            }
            "endpoint.throttled" => self.say(format!("{} is throttled; trying the next endpoint", s(&p["endpoint_id"]))),
            "model.recovered" => self.say(format!("{} is back ({})", s(&p["endpoint_id"]), s(&p["model_id"]))),
            "model.exhausted" => self.say(format!("{} is out of quota ({}); trying the next endpoint", s(&p["endpoint_id"]), s(&p["reason"]))),
            "harness.fallback" => self.say(format!("{} fell back to the API", s(&p["from"]))),
            _ => {}
        }
    }

    /// The cache-read share of all input-side tokens, 0 to 1.
    pub fn cache_hit_ratio(&self) -> f64 {
        let denom = self.tokens.input + self.tokens.cache_read + self.tokens.cache_write;
        if denom == 0 {
            0.0
        } else {
            self.tokens.cache_read as f64 / denom as f64
        }
    }

    /// Children of a node, in order.
    pub fn children(&self, id: &str) -> Vec<&Node> {
        self.nodes.values().filter(|n| n.parent.as_deref() == Some(id)).collect()
    }

    /// Top-level nodes (no parent).
    pub fn roots(&self) -> Vec<&Node> {
        self.nodes.values().filter(|n| n.parent.is_none()).collect()
    }

    /// The view the Narrator should open on when none is asked for: a Council in progress, a build in
    /// progress, else the Epic.
    pub fn default_view(&self) -> &'static str {
        if self.councils.iter().any(|c| c.phase == "round 1" || c.phase == "round 2") {
            "council"
        } else if self.builds.iter().any(|b| b.merge.is_empty() || b.merge == "approved") && self.tasks.values().any(|t| t.flow == "build" && t.status == "running") {
            "forge"
        } else {
            "epic"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(seq: u64, kind: &str, task: &str, agent: &str, payload: Value) -> Value {
        json!({"seq": seq, "type": kind, "task_id": task, "agent": agent, "payload": payload})
    }

    fn usage(seq: u64, span: &str, role: &str, tier: &str, cost: f64) -> Value {
        let mut e = ev(seq, "agent.usage", "t_1", role, json!({"input_tokens": 1000, "cache_read_tokens": 3000, "cache_write_tokens": 0, "output_tokens": 200, "thinking_tokens": 50, "cost_usd": cost}));
        e["span_id"] = json!(span);
        e["model_tier"] = json!(tier);
        e["model_id"] = json!(format!("{}-model", tier));
        e["endpoint_id"] = json!("direct");
        e
    }

    fn roles() -> Vec<String> {
        ["orchestrator", "architect", "builder", "red-team", "strategist"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn control_sequences_from_models_never_reach_the_terminal() {
        assert_eq!(sanitize("ok\x1b[31mred\x1b[0m\x1b]8;;http://x\x07link\x1b]8;;\x07 done\x07\x08"), "okredlink done");
        assert_eq!(sanitize("a\nb\tc\r\x00\x7f"), "a\nb\tc");
        assert_eq!(sanitize("safe \u{202e}evil\u{2066}"), "safe evil", "bidi overrides are dropped");
        assert_eq!(sanitize("tail\x1b"), "tail");
        assert_eq!(sanitize("日本語 ✓ émoji 🏛️"), "日本語 ✓ émoji 🏛️");
    }

    #[test]
    fn agents_accumulate_tokens_spend_and_show_an_upscale() {
        let mut st = ObserverState::new(&roles());
        st.apply(&ev(1, "task.created", "t_1", "", json!({"prompt": "build it", "target": "builder", "flow": "build"})));
        let mut added = ev(2, "dag.node.added", "t_1", "builder", json!({"node_id": "sp_builder_1", "role": "builder", "depth": 0}));
        added["span_id"] = json!("sp_builder_1");
        st.apply(&added);
        assert_eq!(st.agents["builder"].state, "running");
        st.apply(&usage(3, "sp_builder_1", "builder", "workhorse", 0.10));
        st.apply(&usage(4, "sp_builder_1", "builder", "frontier", 0.50));
        let a = &st.agents["builder"];
        assert_eq!((a.tokens.input, a.tokens.cache_read, a.tokens.output, a.tokens.thinking), (2000, 6000, 400, 100));
        assert!((a.cost_usd - 0.60).abs() < 1e-9 && a.calls == 2);
        assert!(a.upscaled && a.tier == "frontier" && a.model == "frontier-model", "{:?}", a);
        assert_eq!(st.retries, 1);
        assert_eq!(st.series.iter().copied().collect::<Vec<_>>(), vec![0.10, 0.50], "one point per call, for the sparkline");
        assert!((st.spent_usd - 0.60).abs() < 1e-9);
        assert!((st.cache_hit_ratio() - 0.75).abs() < 1e-9);
        assert_eq!(st.nodes["sp_builder_1"].tokens, 2400);
        st.apply(&ev(5, "dag.node.state", "t_1", "builder", json!({"node_id": "sp_builder_1", "state": "done"})));
        assert_eq!(st.agents["builder"].state, "idle");
    }

    #[test]
    fn approvals_flip_an_agent_to_waiting_and_back() {
        let mut st = ObserverState::new(&roles());
        st.apply(&ev(1, "dag.node.added", "t_1", "builder", json!({"node_id": "n1", "role": "builder", "depth": 0})));
        st.apply(&ev(2, "approval.requested", "t_1", "builder", json!({"approval_id": "ap_1", "role": "builder", "summary": "merge abc\x1b[31m"})));
        assert_eq!(st.agents["builder"].state, "waiting on you");
        assert_eq!(st.pending_approvals["ap_1"], "merge abc\x1b[31m", "stored as given; the renderer sanitizes");
        st.apply(&ev(3, "approval.resolved", "t_1", "builder", json!({"approval_id": "ap_1"})));
        assert_eq!((st.agents["builder"].state.as_str(), st.pending_approvals.len()), ("running", 0));
        assert!(!st.narrative.iter().any(|l| l.contains('\x1b')));
    }

    #[test]
    fn a_replayed_snapshot_and_the_live_stream_can_overlap() {
        let events: Vec<Value> = vec![
            ev(1, "task.created", "t_1", "", json!({"prompt": "p", "target": "orchestrator"})),
            ev(2, "dag.node.added", "t_1", "orchestrator", json!({"node_id": "n1", "role": "orchestrator", "depth": 0})),
            usage(3, "n1", "orchestrator", "frontier", 0.2),
        ];
        let mut a = ObserverState::new(&roles());
        for e in &events {
            a.apply(e);
        }
        let mut b = ObserverState::new(&roles());
        for e in events.iter().chain(events.iter()) {
            b.apply(e); // the same events delivered twice: counted once
        }
        assert_eq!((a.spent_usd, a.last_seq, a.agents["orchestrator"].calls), (b.spent_usd, b.last_seq, b.agents["orchestrator"].calls));
        assert_eq!(b.agents["orchestrator"].calls, 1);
    }

    #[test]
    fn the_epic_keeps_parents_children_and_the_live_text_of_a_running_span() {
        let mut st = ObserverState::new(&roles());
        st.apply(&ev(1, "dag.node.added", "t_1", "orchestrator", json!({"node_id": "n1", "role": "orchestrator", "depth": 0})));
        st.apply(&ev(2, "dag.node.added", "t_1", "architect", json!({"node_id": "n2", "role": "architect", "depth": 1, "parent": "n1"})));
        assert_eq!((st.roots().len(), st.children("n1").len()), (1, 1));
        st.apply_frame(&json!({"spans": {"n2": {"text": "Thinking about ", "thinking": "secret"}}}));
        st.apply_frame(&json!({"spans": {"n2": {"text": "the barrier.", "thinking": ""}}}));
        assert_eq!(st.live["n2"], "Thinking about the barrier.");
        st.apply_frame(&json!({"spans": {"n2": {"text": "x".repeat(10_000), "thinking": ""}}}));
        assert!(st.live["n2"].len() <= TEXT_CAP);
        st.apply(&ev(3, "dag.node.state", "t_1", "architect", json!({"node_id": "n2", "state": "done"})));
        assert!(!st.live.contains_key("n2"));
    }

    #[test]
    fn a_council_is_followed_from_convening_to_its_verdict() {
        let mut st = ObserverState::new(&roles());
        st.apply(&ev(1, "council.started", "t_2", "", json!({"proposition": "Queue or cron?", "seats": [{"role": "architect", "family": "claude"}, {"role": "red-team", "family": "gemini"}], "homogeneous": false})));
        assert_eq!(st.councils[0].phase, "round 1");
        assert_eq!(st.default_view(), "council");
        st.apply(&ev(2, "council.barrier.released", "t_2", "", json!({"round": 1})));
        st.apply(&ev(3, "council.stance", "t_2", "", json!({"role": "architect", "status": "answered", "text": "Queue.\x1b[2J"})));
        st.apply(&ev(4, "council.critique", "t_2", "", json!({"role": "red-team", "recommendation": "PROCEED", "concur": true, "findings": [{"severity": "P1", "title": "idempotency"}], "text": "ok"})));
        st.apply(&ev(5, "council.verdict", "t_2", "", json!({"path": "missions/m/councils/001-x.md", "escalated": "2 unresolved P0 finding(s)", "unresolved": [{"severity": "P0", "seat": "red-team", "title": "t"}], "options": [{"label": "PROCEED", "seats": ["architect", "red-team"]}]})));
        let c = &st.councils[0];
        assert_eq!((c.phase.as_str(), c.stances[0].text.as_str(), c.critiques[0].findings.len(), c.unresolved.len(), c.options[0].1.len()), ("escalated", "Queue.", 1, 1, 2));
        assert_eq!(st.default_view(), "epic", "a finished council no longer pulls the view");
    }

    #[test]
    fn a_build_shows_attempts_gates_findings_and_how_it_ended() {
        let mut st = ObserverState::new(&roles());
        st.apply(&ev(1, "task.created", "t_3", "", json!({"prompt": "b", "target": "builder", "flow": "build"})));
        st.apply(&ev(2, "worktree.created", "t_3", "builder", json!({"branch": "delphi/t_3"})));
        st.apply(&ev(3, "build.attempt", "t_3", "", json!({"attempt": 1, "tier": "workhorse", "family": "claude", "homogeneous_retry": false})));
        st.apply(&ev(4, "build.attempt", "t_3", "", json!({"attempt": 2, "tier": "frontier", "family": "claude", "homogeneous_retry": false})));
        st.apply(&ev(5, "gates.evaluated", "t_3", "", json!({"sha": "0123456789abcdef", "gates": [{"name": "tests", "state": "green", "detail": "ok"}, {"name": "operator", "state": "red", "detail": "waiting"}]})));
        st.apply(&ev(6, "audit.recorded", "t_3", "red-team", json!({"findings": [{"severity": "P2", "title": "naming"}]})));
        assert_eq!(st.default_view(), "forge");
        st.apply(&ev(7, "merge.approved", "t_3", "", json!({"sha": "x"})));
        st.apply(&ev(8, "action.result", "t_3", "", json!({"action_id": "merge-abc", "ok": true})));
        let b = &st.builds[0];
        assert_eq!((b.branch.as_str(), b.attempts.len(), b.sha.as_str(), b.gates.len(), b.findings.len(), b.merge.as_str()), ("delphi/t_3", 2, "0123456789ab", 2, 1, "merged"));
        assert!(st.narrative.iter().any(|l| l.contains("retrying")));
    }
}
