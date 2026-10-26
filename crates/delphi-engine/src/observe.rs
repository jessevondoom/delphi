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
pub const KINDS: [&str; 41] = [
    "task.created", "task.output", "task.completed", "task.failed", "dag.node.added", "dag.node.state", "agent.usage", "approval.requested", "approval.resolved", "span.taint", "build.attempt", "gates.evaluated", "audit.recorded", "merge.approved", "worktree.created", "action.result", "gate.flaky",
    "council.started", "council.barrier.released", "council.stance", "council.critique", "council.verdict", "endpoint.throttled", "harness.fallback", "model.exhausted", "model.recovered", "task.blocked", "task.unblocked", "model.degraded_confirm", "model.homogeneous", "budget.reserve", "harness.started", "thread.named", "session.started", "change.recorded", "bundle.added", "bundle.updated", "bundle.removed", "bundle.vectors_progress", "bundle.vectors_rejected", "org_context.built",
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

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
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

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
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

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Node {
    pub id: String,
    pub role: String,
    pub parent: Option<String>,
    pub depth: u64,
    pub task_id: String,
    pub state: String,
    pub tokens: u64,
    pub cost_usd: f64,
    /// The endpoint and model this agent's latest call went to (empty until its first call), and the tier it asked for.
    pub endpoint: String,
    pub model: String,
    pub tier: String,
    /// What it is waiting for, if anything: `waiting on you`, or `waiting for a model: <why>`.
    pub waiting: String,
    /// When the node was added and when it finished (`done` or `escalated`), as Unix milliseconds from the event
    /// timestamps. `ended_ms` is absent while it runs. 0 when the event carried no readable timestamp.
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    /// The nodes this one comes after. Taken from the event's `after` list when the plan recorded one; otherwise
    /// **derived**, because the event log does not record plan edges: the first child follows its parent and every
    /// later child follows the sibling created just before it. Roots have none.
    pub after: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Task {
    pub id: String,
    pub flow: String,
    pub prompt: String,
    /// `running`, `done`, `escalated`, `failed`.
    pub status: String,
    pub output: String,
    /// The session it was asked in (`specs/030`); empty before the first `session.started`.
    pub session: String,
}

/// A console session as the narrator groups work by it: how it began, when, and how many requests it has held.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct SessionRow {
    pub id: String,
    pub reason: String,
    pub started_ms: u64,
    pub turns: u32,
}

const MAX_SESSIONS: usize = 20;

/// One line of the change log (`change.recorded`): what an agent says it finished, in its own plain words, with the
/// session it happened in and when. Results only; nothing of the reasoning behind them is ever recorded.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Change {
    pub session: String,
    pub at_ms: u64,
    pub role: String,
    pub task: String,
    pub summary: String,
    pub files: Vec<String>,
}

const MAX_CHANGES: usize = 200;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Seat {
    pub role: String,
    pub family: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Stance {
    pub role: String,
    pub answered: bool,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Critique {
    pub role: String,
    pub recommendation: String,
    pub concur: Option<bool>,
    pub findings: Vec<(String, String)>,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
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

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct GateRow {
    pub name: String,
    pub state: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
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

/// A thread of work the orchestrator named (`thread.named`): a label and the tasks that belong to it. The narrator's
/// epic pane groups by these; tasks no thread lists are grouped by what their requests say.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Thread {
    pub id: String,
    pub label: String,
    pub tasks: Vec<String>,
}

const MAX_THREADS: usize = 40;
const MAX_THREAD_TASKS: usize = 50;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ObserverState {
    pub last_seq: u64,
    pub agents: BTreeMap<String, AgentRow>,
    pub nodes: BTreeMap<String, Node>,
    pub tasks: BTreeMap<String, Task>,
    pub councils: Vec<Council>,
    pub builds: Vec<Build>,
    pub threads: Vec<Thread>,
    /// The change log, oldest first (the newest 200).
    pub changes: Vec<Change>,
    /// The console sessions of the mission, oldest first (the newest 20); the last is the current one.
    pub sessions: Vec<SessionRow>,
    pub pending_approvals: BTreeMap<String, String>,
    pub spent_usd: f64,
    pub tokens: Tokens,
    pub retries: u64,
    pub narrative: VecDeque<String>,
    /// Cost of each model call, oldest first (the spend sparkline); the newest 240.
    pub series: VecDeque<f64>,
    /// Live text per span while it streams (cleared when the node finishes).
    pub live: BTreeMap<String, String>,
    /// Tokens per minute for the last twelve minutes, oldest first, the last one being the minute of the newest
    /// `agent.usage` event (the pace row of the narrator).
    pub pace: VecDeque<u64>,
    /// The minute (Unix minutes) the last `pace` bucket is for.
    #[serde(skip)]
    pace_minute: u64,
    /// The newest child of each parent node, to derive `Node::after`.
    #[serde(skip)]
    last_child: BTreeMap<String, String>,
}

const PACE_MINUTES: usize = 12;

/// Calls and failures of one tool.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct ToolCount {
    pub calls: u32,
    pub errors: u32,
}

/// What one role's agents have called in the mission: totals, the tool used last, and the count of each tool (the
/// ten most used). Names and counts only: never arguments, results or the text around them.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct RoleTools {
    pub calls: u32,
    pub errors: u32,
    pub last: String,
    pub by_tool: BTreeMap<String, ToolCount>,
}

const MAX_TOOLS_PER_ROLE: usize = 10;

/// A tool name as the operator knows it: Delphi's own tools lose the prefix a CLI gives them.
pub fn tool_name(raw: &str) -> String {
    let n = raw.strip_prefix(crate::bridge::TOOL_PREFIX).or_else(|| raw.strip_prefix(crate::bridge::AGY_TOOL_PREFIX)).unwrap_or(raw);
    sanitize(n).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(60).collect()
}

/// Is this a tool call, as `(tool, failed)`? Three kinds of event record one, depending on who ran it: `tool.result`
/// (an API seat's call, with its outcome), `harness.bridge_call` (a Delphi tool called from a subscription CLI, with its
/// outcome) and `harness.tool` (the CLI's own tool, no outcome). A Delphi tool called from a CLI is also logged as
/// `harness.tool`, as the CLI saw it; that line is skipped so each call counts once.
pub fn tool_call_of(kind: &str, p: &Value) -> Option<(String, bool)> {
    match kind {
        "tool.result" | "harness.bridge_call" => {
            let n = tool_name(p["name"].as_str()?);
            (!n.is_empty()).then(|| (n, p["is_error"].as_bool().unwrap_or(false)))
        }
        "harness.tool" => {
            let raw = p["tool"].as_str()?;
            if raw.starts_with(crate::bridge::TOOL_PREFIX) || raw.starts_with(crate::bridge::AGY_TOOL_PREFIX) {
                return None;
            }
            let n = tool_name(raw);
            (!n.is_empty()).then_some((n, false))
        }
        _ => None,
    }
}

/// Tool use per role over a whole mission log. It is a pass of its own, not part of [`ObserverState::apply`], because
/// calls are the most frequent events in a log and would push everything else out of the window the fold keeps.
pub fn tool_stats(events: &[delphi_core::event::EventEnvelope]) -> BTreeMap<String, RoleTools> {
    let mut out: BTreeMap<String, RoleTools> = BTreeMap::new();
    for e in events {
        let Some((tool, failed)) = tool_call_of(&e.kind, &e.payload) else { continue };
        let role = e.agent.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "unknown".into());
        let r = out.entry(role).or_default();
        r.calls += 1;
        r.errors += failed as u32;
        r.last = tool.clone();
        let c = r.by_tool.entry(tool).or_default();
        c.calls += 1;
        c.errors += failed as u32;
    }
    for r in out.values_mut() {
        if r.by_tool.len() > MAX_TOOLS_PER_ROLE {
            let mut v: Vec<(String, ToolCount)> = std::mem::take(&mut r.by_tool).into_iter().collect();
            // `record_change` is always kept: the window shows it beside the totals.
            v.sort_by(|a, b| (b.0 == "record_change").cmp(&(a.0 == "record_change")).then_with(|| b.1.calls.cmp(&a.1.calls)).then_with(|| a.0.cmp(&b.0)));
            v.truncate(MAX_TOOLS_PER_ROLE);
            r.by_tool = v.into_iter().collect();
        }
    }
    out
}

/// Unix milliseconds of an event envelope's RFC 3339 `timestamp`, or 0.
fn ts_ms(e: &Value) -> u64 {
    let t = e["timestamp"].as_str().unwrap_or("");
    let secs = delphi_core::time::parse_rfc3339(t).unwrap_or(0).max(0) as u64;
    let millis = t
        .get(19..)
        .and_then(|r| r.strip_prefix('.'))
        .map(|r| r.chars().take_while(|c| c.is_ascii_digit()).take(3).collect::<String>())
        .filter(|d| !d.is_empty())
        .map(|d| format!("{:0<3}", d).parse::<u64>().unwrap_or(0))
        .unwrap_or(0);
    secs * 1000 + millis
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
                let session = self.sessions.last().map(|s| s.id.clone()).unwrap_or_default();
                if let Some(s) = self.sessions.last_mut() {
                    s.turns += 1;
                }
                self.tasks.insert(task.clone(), Task { id: task, flow, prompt, status: "running".into(), output: String::new(), session });
            }
            "session.started" => {
                let clip = |v: &Value, n: usize| -> String { sanitize(v.as_str().unwrap_or("")).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect() };
                let id = clip(&p["session_id"], 40);
                if !id.is_empty() {
                    self.sessions.push(SessionRow { id: id.clone(), reason: clip(&p["reason"], 20), started_ms: ts_ms(e), turns: 0 });
                    if self.sessions.len() > MAX_SESSIONS {
                        self.sessions.remove(0);
                    }
                    self.say(format!("session {} began ({})", id, clip(&p["reason"], 20)));
                }
            }
            "change.recorded" => {
                let clip = |v: &Value, n: usize| -> String { sanitize(v.as_str().unwrap_or("")).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect() };
                let summary = clip(&p["summary"], 300);
                if !summary.is_empty() {
                    let files = p["files"].as_array().map(|a| a.iter().map(|x| clip(x, 120)).filter(|x| !x.is_empty()).take(20).collect()).unwrap_or_default();
                    let session = self.sessions.last().map(|s| s.id.clone()).unwrap_or_default();
                    self.changes.push(Change { session, at_ms: ts_ms(e), role: if role.is_empty() { clip(&p["role"], 40) } else { role.clone() }, task: task.clone(), summary, files });
                    if self.changes.len() > MAX_CHANGES {
                        self.changes.remove(0);
                    }
                }
            }
            "bundle.added" | "bundle.updated" | "bundle.removed" => {
                let clip = |v: &Value, n: usize| -> String { sanitize(v.as_str().unwrap_or("")).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect() };
                let id = format!("{}/{}", clip(&p["org"], 60), clip(&p["name"], 60));
                let version = clip(&p["version"], 30);
                let what = match kind {
                    "bundle.added" => "added",
                    "bundle.updated" => "updated",
                    _ => "removed",
                };
                self.say(format!("bundle {}{} {}", id, if version.is_empty() { String::new() } else { format!("@{}", version) }, what));
            }
            "bundle.vectors_progress" | "bundle.vectors_rejected" => {
                let clip = |v: &Value, n: usize| -> String { sanitize(v.as_str().unwrap_or("")).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect() };
                let id = format!("{}/{}", clip(&p["org"], 60), clip(&p["name"], 60));
                if kind == "bundle.vectors_rejected" {
                    self.say(format!("bundle {} shipped vectors failed verification; embedding locally", id));
                } else {
                    self.say(format!("bundle {} vectors {}% ready", id, p["percent"].as_u64().unwrap_or(0).min(100)));
                }
            }
            "org_context.built" => {
                let n = |k: &str| p[k].as_u64().unwrap_or(0);
                let trimmed = p["trimmed"].as_array().map(|a| a.len()).unwrap_or(0);
                self.say(format!("org context built: {} bundle(s), {} bytes{}", n("bundles"), n("bytes"), if trimmed > 0 { format!(", {} trimmed", trimmed) } else { String::new() }));
            }
            "thread.named" => {
                let clip = |v: &Value, n: usize| -> String { sanitize(v.as_str().unwrap_or("")).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect() };
                let (id, label) = (clip(&p["thread"], 60), clip(&p["label"], 80));
                let tasks: Vec<String> = p["tasks"].as_array().map(|a| a.iter().map(|x| clip(x, 60)).filter(|x| !x.is_empty()).take(MAX_THREAD_TASKS).collect()).unwrap_or_default();
                if !id.is_empty() && !label.is_empty() {
                    let t = Thread { id: id.clone(), label, tasks };
                    match self.threads.iter().position(|x| x.id == id) {
                        Some(i) => self.threads[i] = t,
                        None if self.threads.len() < MAX_THREADS => self.threads.push(t),
                        None => {}
                    }
                }
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
            "task.cancelled" => {
                if let Some(t) = self.tasks.get_mut(&task) {
                    t.status = "cancelled".into();
                }
                self.say(format!("{} stopped by the operator", task));
            }
            "dag.node.added" => {
                let id = s(&p["node_id"]);
                let r = s(&p["role"]);
                let parent = p["parent"].as_str().filter(|x| !x.is_empty() && *x != "operator").map(String::from);
                let after: Vec<String> = match p["after"].as_array() {
                    Some(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                    None => parent.as_ref().map(|par| vec![self.last_child.get(par).cloned().unwrap_or_else(|| par.clone())]).unwrap_or_default(),
                };
                if let Some(par) = &parent {
                    self.last_child.insert(par.clone(), id.clone());
                }
                self.nodes.insert(id.clone(), Node { id: id.clone(), role: r.clone(), parent, depth: p["depth"].as_u64().unwrap_or(0), task_id: task.clone(), state: "pending".into(), started_ms: ts_ms(e), after, ..Default::default() });
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
                    if st == "done" || st == "escalated" {
                        n.waiting.clear();
                        n.ended_ms = Some(ts_ms(e));
                    }
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
            "budget.reserve" | "harness.started" => {
                self.note_model(&s(&e["span_id"]), &s(&e["endpoint_id"]), &s(&e["model_id"]), &s(&e["model_tier"]));
                if let Some(n) = self.nodes.get_mut(&s(&e["span_id"])) {
                    if n.waiting.starts_with("waiting for a model") {
                        n.waiting.clear(); // a call is going out: a model was found
                    }
                }
            }
            "task.blocked" => {
                if let Some(n) = self.nodes.get_mut(&s(&e["span_id"])) {
                    n.waiting = format!("waiting for a model: {}", sanitize(p["why"].as_str().unwrap_or("")).chars().take(100).collect::<String>());
                }
                self.say(format!("waiting for a model: {}", s(&p["why"])));
            }
            "task.unblocked" => {
                if let Some(n) = self.nodes.get_mut(&s(&e["span_id"])) {
                    if n.waiting.starts_with("waiting for a model") {
                        n.waiting.clear();
                    }
                }
                self.say(if p["resumed"] == true { "a model is back; resuming".to_string() } else { "stopped waiting for a model".to_string() });
            }
            "agent.usage" => {
                self.note_model(&s(&e["span_id"]), &s(&e["endpoint_id"]), &s(&e["model_id"]), &s(&e["model_tier"]));
                let cost = p["cost_usd"].as_f64().unwrap_or(0.0);
                self.spent_usd += cost;
                self.tokens.add(p);
                self.add_pace(ts_ms(e) / 60_000, p["input_tokens"].as_u64().unwrap_or(0) + p["output_tokens"].as_u64().unwrap_or(0));
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
                if let Some(n) = self.nodes.get_mut(&s(&e["span_id"])) {
                    n.waiting = "waiting on you".into();
                }
                self.pending_approvals.insert(id, s(&p["summary"]));
                if let Some(a) = self.agents.get_mut(&s(&p["role"])) {
                    a.state = "waiting on you".into();
                }
                self.say(format!("waiting on you: {}", sanitize(p["summary"].as_str().unwrap_or("")).chars().take(80).collect::<String>()));
            }
            "approval.resolved" => {
                if let Some(n) = self.nodes.get_mut(&s(&e["span_id"])) {
                    if n.waiting == "waiting on you" {
                        n.waiting.clear();
                    }
                }
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
            "model.homogeneous" => self.say(format!("no model outside {} was usable; this pass is homogeneous", s(&p["avoided"]))),
            "model.degraded_confirm" if p["state"] == "asked" => self.say("asking whether a weaker model may run this task".to_string()),
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

    /// Remember the endpoint, model and tier an agent's latest call went to. Empty values leave what is known alone.
    /// Count `tokens` into the bucket for Unix minute `minute`, opening new (and empty, for quiet minutes) buckets as
    /// the minutes advance. An event from an earlier minute than the newest is counted in the newest.
    fn add_pace(&mut self, minute: u64, tokens: u64) {
        if self.pace.is_empty() {
            self.pace.push_back(0);
            self.pace_minute = minute;
        }
        if minute > self.pace_minute {
            for _ in 0..(minute - self.pace_minute).min(PACE_MINUTES as u64) {
                self.pace.push_back(0);
            }
            self.pace_minute = minute;
            while self.pace.len() > PACE_MINUTES {
                self.pace.pop_front();
            }
        }
        if let Some(last) = self.pace.back_mut() {
            *last += tokens;
        }
    }

    fn note_model(&mut self, span: &str, endpoint: &str, model: &str, tier: &str) {
        if let Some(n) = self.nodes.get_mut(span) {
            for (field, value) in [(&mut n.endpoint, endpoint), (&mut n.model, model), (&mut n.tier, tier)] {
                if !value.is_empty() {
                    *field = value.to_string();
                }
            }
        }
    }

    /// Every agent running now (one per live span, so two of the same role, a Council's seats and dispatched
    /// children are each their own row), in the order they started within their task.
    pub fn running_agents(&self) -> Vec<&Node> {
        let mut v: Vec<&Node> = self.nodes.values().filter(|n| n.state == "running" || n.state == "pending").collect();
        v.sort_by(|a, b| (&a.task_id, a.depth, &a.id).cmp(&(&b.task_id, b.depth, &b.id)));
        v
    }

    /// What a running agent is on, in words: `claude-sonnet-5-5 via direct (workhorse)`, or `no call yet`.
    pub fn model_label(n: &Node) -> String {
        if n.model.is_empty() {
            return "no call yet".into();
        }
        let via = if n.endpoint.is_empty() { String::new() } else { format!(" via {}", n.endpoint) };
        let tier = if n.tier.is_empty() { String::new() } else { format!(" ({})", n.tier) };
        format!("{}{}{}", n.model, via, tier)
    }

    /// The running agents as plain text, one per line, for the console and logs. `name` maps a role to its display name.
    pub fn describe_running(&self, name: &dyn Fn(&str) -> String) -> String {
        let rows = self.running_agents();
        if rows.is_empty() {
            return "Nothing is running.\n".into();
        }
        let mut out = String::new();
        for n in rows {
            let what = if n.waiting.is_empty() { "working" } else { n.waiting.as_str() };
            out.push_str(&format!("{}{} · {} · {} · {}\n", "  ".repeat(n.depth as usize), name(&n.role), n.task_id, Self::model_label(n), what));
        }
        out
    }

    /// The view the Narrator should open on when none is asked for: a Council in progress, else the Epic.
    pub fn default_view(&self) -> &'static str {
        if self.councils.iter().any(|c| c.phase == "round 1" || c.phase == "round 2") {
            "council"
        } else {
            "dashboard"
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

    /// An envelope for one agent's span, with the fields the daemon stamps on agent events.
    fn on_span(mut e: Value, span: &str, endpoint: &str, model: &str, tier: &str) -> Value {
        e["span_id"] = json!(span);
        e["endpoint_id"] = json!(endpoint);
        e["model_id"] = json!(model);
        e["model_tier"] = json!(tier);
        e
    }

    fn running(st: &mut ObserverState, seq: u64, span: &str, role: &str, parent: Option<&str>, depth: u64) {
        let mut added = json!({"node_id": span, "role": role, "depth": depth});
        if let Some(p) = parent {
            added["parent"] = json!(p);
        }
        st.apply(&ev(seq, "dag.node.added", "t_1", role, added));
        st.apply(&ev(seq + 1, "dag.node.state", "t_1", role, json!({"node_id": span, "state": "running"})));
    }

    // The operator's request: every running agent and the model each is on right now.
    #[test]
    fn every_running_agent_is_listed_with_the_model_it_is_on_and_what_it_waits_for() {
        let mut st = ObserverState::new(&roles());
        assert_eq!(st.describe_running(&|r| r.to_string()), "Nothing is running.\n");
        running(&mut st, 1, "n1", "orchestrator", None, 0);
        running(&mut st, 3, "n2", "architect", Some("n1"), 1);
        running(&mut st, 5, "n3", "red-team", Some("n1"), 1);
        // The first call of the orchestrator and of the architect is about to go out; the red-team has not called yet.
        st.apply(&on_span(ev(7, "budget.reserve", "t_1", "orchestrator", json!({"reservation_id": "r_1", "amount": 1})), "n1", "direct", "opus-x", "frontier"));
        st.apply(&on_span(ev(8, "budget.reserve", "t_1", "architect", json!({"reservation_id": "r_2", "amount": 1})), "n2", "gem", "gem-pro", "frontier"));
        let rows = st.running_agents();
        assert_eq!(rows.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), vec!["n1", "n2", "n3"], "parent before children, one row per live span");
        assert_eq!(ObserverState::model_label(rows[0]), "opus-x via direct (frontier)");
        assert_eq!(ObserverState::model_label(rows[1]), "gem-pro via gem (frontier)");
        assert_eq!(ObserverState::model_label(rows[2]), "no call yet");
        let text = st.describe_running(&|r| r.to_uppercase());
        assert!(text.contains("ORCHESTRATOR · t_1 · opus-x via direct (frontier) · working\n"), "{}", text);
        assert!(text.contains("  ARCHITECT · t_1 · gem-pro via gem (frontier) · working\n"), "children are indented: {}", text);
        assert!(text.contains("  RED-TEAM · t_1 · no call yet · working\n"), "{}", text);

        // The architect's model runs out and it moves to another: the row follows its latest call.
        st.apply(&on_span(ev(9, "budget.reserve", "t_1", "architect", json!({"reservation_id": "r_3", "amount": 1})), "n2", "backup", "sonnet-x", "workhorse"));
        assert_eq!(ObserverState::model_label(st.running_agents()[1]), "sonnet-x via backup (workhorse)");
        st.apply(&on_span(ev(10, "agent.usage", "t_1", "architect", json!({"input_tokens": 10, "output_tokens": 5, "cost_usd": 0.01})), "n2", "backup", "sonnet-x", "workhorse"));
        assert_eq!(st.nodes["n2"].model, "sonnet-x");

        // Waiting on you, and waiting for a model, are per agent, and clear on their own.
        let mut req = ev(11, "approval.requested", "t_1", "architect", json!({"approval_id": "ap_1", "role": "architect", "summary": "write a spec"}));
        req["span_id"] = json!("n2");
        st.apply(&req);
        let mut blocked = ev(12, "task.blocked", "t_1", "red-team", json!({"reason": "no_model", "back_at": 1_792_864_800, "why": "everything is throttled"}));
        blocked["span_id"] = json!("n3");
        st.apply(&blocked);
        let text = st.describe_running(&|r| r.to_string());
        assert!(text.contains("architect · t_1 · sonnet-x via backup (workhorse) · waiting on you\n"), "{}", text);
        assert!(text.contains("red-team · t_1 · no call yet · waiting for a model: everything is throttled\n"), "{}", text);
        assert!(text.contains("orchestrator · t_1 · opus-x via direct (frontier) · working\n"), "the others are unaffected: {}", text);
        let mut resolved = ev(13, "approval.resolved", "t_1", "architect", json!({"approval_id": "ap_1"}));
        resolved["span_id"] = json!("n2");
        st.apply(&resolved);
        let mut back = ev(14, "task.unblocked", "t_1", "red-team", json!({"resumed": true}));
        back["span_id"] = json!("n3");
        st.apply(&back);
        assert!(st.running_agents().iter().all(|n| n.waiting.is_empty()), "{:?}", st.running_agents().iter().map(|n| &n.waiting).collect::<Vec<_>>());

        // A finished agent leaves the list.
        st.apply(&ev(15, "dag.node.state", "t_1", "architect", json!({"node_id": "n2", "state": "done"})));
        assert_eq!(st.running_agents().iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), vec!["n1", "n3"]);
        // A harness run names its model when it starts.
        running(&mut st, 20, "n4", "scout", None, 0);
        st.apply(&on_span(ev(22, "harness.started", "t_1", "scout", json!({"endpoint_id": "agy-sub", "tools": "none"})), "n4", "agy-sub", "gem-agy", "frontier"));
        assert_eq!(ObserverState::model_label(st.running_agents().into_iter().find(|n| n.id == "n4").unwrap()), "gem-agy via agy-sub (frontier)");
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

    fn at(mut e: Value, timestamp: &str) -> Value {
        e["timestamp"] = json!(timestamp);
        e
    }

    // specs/005e §9.3: the rail and the pace row need node times, edges and tokens per minute.
    #[test]
    fn nodes_carry_start_and_end_times_and_derived_edges() {
        let mut st = ObserverState::new(&roles());
        let add = |seq, id: &str, parent: Option<&str>, t: &str| {
            let mut p = json!({"node_id": id, "role": "builder", "depth": 1});
            if let Some(par) = parent {
                p["parent"] = json!(par);
            }
            at(ev(seq, "dag.node.added", "t_1", "builder", p), t)
        };
        st.apply(&add(1, "n1", None, "2026-10-25T10:00:00.250Z"));
        st.apply(&add(2, "n2", Some("n1"), "2026-10-25T10:00:05.000Z"));
        st.apply(&add(3, "n3", Some("n1"), "2026-10-25T10:00:06.000Z"));
        st.apply(&add(4, "n4", Some("n3"), "2026-10-25T10:00:07.000Z"));
        let done = at(ev(5, "dag.node.state", "t_1", "builder", json!({"node_id": "n2", "state": "done"})), "2026-10-25T10:00:09.500Z");
        st.apply(&done);
        let (n1, n2, n3, n4) = (&st.nodes["n1"], &st.nodes["n2"], &st.nodes["n3"], &st.nodes["n4"]);
        assert_eq!(n1.started_ms % 1000, 250, "milliseconds are kept");
        assert_eq!(n2.started_ms - n1.started_ms, 4750);
        assert_eq!(n2.ended_ms, Some(n2.started_ms + 4500));
        assert_eq!(n1.ended_ms, None, "still running");
        assert!(n1.after.is_empty(), "a root follows nothing");
        assert_eq!(n2.after, vec!["n1"], "the first child follows its parent");
        assert_eq!(n3.after, vec!["n2"], "a later child follows the sibling before it");
        assert_eq!(n4.after, vec!["n3"]);
        // A plan that records its edges wins over the derived ones.
        st.apply(&at(ev(6, "dag.node.added", "t_1", "builder", json!({"node_id": "n5", "role": "builder", "depth": 1, "parent": "n1", "after": ["n2", "n3"]})), "2026-10-25T10:00:10.000Z"));
        assert_eq!(st.nodes["n5"].after, vec!["n2", "n3"]);
        let j = serde_json::to_value(&st).unwrap();
        assert!(j["nodes"]["n1"]["started_ms"].is_u64() && j["nodes"]["n2"]["ended_ms"].is_u64() && j["nodes"]["n1"]["ended_ms"].is_null());
        assert!(j.get("pace_minute").is_none() && j.get("last_child").is_none(), "bookkeeping is not sent");
    }

    #[test]
    fn pace_is_tokens_per_minute_for_the_last_twelve_minutes() {
        let mut st = ObserverState::new(&roles());
        assert!(st.pace.is_empty());
        // 1200 tokens (1000 in + 200 out) each; two in minute 0, one in minute 1, then a quiet gap to minute 4.
        for (seq, t) in [(1, "2026-10-25T10:00:10Z"), (2, "2026-10-25T10:00:50Z"), (3, "2026-10-25T10:01:05Z"), (4, "2026-10-25T10:04:00Z")] {
            st.apply(&at(usage(seq, "n1", "builder", "fast", 0.01), t));
        }
        assert_eq!(st.pace.iter().copied().collect::<Vec<_>>(), vec![2400, 1200, 0, 0, 1200]);
        // More than twelve minutes later, only the last twelve remain and the old ones are gone.
        st.apply(&at(usage(5, "n1", "builder", "fast", 0.01), "2026-10-25T10:40:00Z"));
        assert_eq!(st.pace.len(), 12);
        assert_eq!(st.pace.back(), Some(&1200));
        assert_eq!(st.pace.iter().sum::<u64>(), 1200, "everything older fell out");
        // Events without a timestamp count into the newest bucket rather than being lost.
        st.apply(&usage(6, "n1", "builder", "fast", 0.01));
        assert_eq!(st.pace.back(), Some(&2400));
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

    fn tev(kind: &str, role: &str, payload: Value) -> delphi_core::event::EventEnvelope {
        let mut e = delphi_core::event::EventEnvelope::new("m", kind, payload);
        e.agent = Some(role.into());
        e
    }

    #[test]
    fn tool_calls_are_counted_once_per_call_per_role_from_all_three_kinds_of_event() {
        let log = vec![
            tev("tool.result", "builder", json!({"name": "read_file", "is_error": false, "content": "SECRET TEXT"})),
            tev("tool.result", "builder", json!({"name": "read_file", "is_error": true, "content": "denied"})),
            tev("harness.bridge_call", "builder", json!({"name": "mcp__delphi__record_change", "is_error": false, "arguments": {"summary": "x"}})),
            // The same call as the CLI saw it: not counted a second time. The CLI's own tools are counted.
            tev("harness.tool", "builder", json!({"tool": "mcp__delphi__record_change", "summary": "record_change"})),
            tev("harness.tool", "builder", json!({"tool": "mcp_delphi_delphi_read_file", "summary": ""})),
            tev("harness.tool", "builder", json!({"tool": "Bash", "summary": "cargo test"})),
            tev("harness.tool", "writer", json!({"tool": "WebSearch\u{1b}[31m", "summary": "q"})),
            tev("task.output", "builder", json!({"text": "not a call"})),
        ];
        let s = tool_stats(&log);
        let b = &s["builder"];
        assert_eq!((b.calls, b.errors, b.last.as_str()), (4, 1, "Bash"));
        assert_eq!(b.by_tool["read_file"], ToolCount { calls: 2, errors: 1 });
        assert_eq!(b.by_tool["record_change"], ToolCount { calls: 1, errors: 0 });
        assert_eq!(s["writer"].by_tool.keys().collect::<Vec<_>>(), ["WebSearch"], "control sequences are removed");
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.contains("SECRET") && !text.contains("cargo test") && !text.contains("summary"), "names and counts only: {}", text);
    }

    #[test]
    fn a_role_keeps_its_ten_most_used_tools_and_always_record_change() {
        let mut log: Vec<_> = (0..14).flat_map(|i| (0..=i).map(move |_| tev("tool.result", "builder", json!({"name": format!("tool_{:02}", i)})))).collect();
        log.push(tev("tool.result", "builder", json!({"name": "record_change"})));
        let b = &tool_stats(&log)["builder"];
        assert_eq!(b.by_tool.len(), MAX_TOOLS_PER_ROLE);
        assert!(b.by_tool.contains_key("record_change") && b.by_tool.contains_key("tool_13") && !b.by_tool.contains_key("tool_00"));
        assert_eq!(b.calls, 106, "the totals count every call, kept or not");
    }

    #[test]
    fn recorded_changes_are_kept_by_session_cleaned_and_bounded() {
        let mut st = ObserverState::new(&roles());
        st.apply(&ev(1, "change.recorded", "t_1", "builder", json!({"summary": "reworked the \x1b[31mpermission\x1b[0m model\n to be less intrusive", "role": "builder", "files": ["a.rs", ""]})));
        st.apply(&ev(2, "session.started", "", "", json!({"session_id": "s_0002", "reason": "new"})));
        st.apply(&ev(3, "change.recorded", "t_2", "", json!({"summary": "removed ratify requirements", "role": "writer"})));
        st.apply(&ev(4, "change.recorded", "t_2", "writer", json!({"summary": "   "})));
        assert_eq!(st.changes.len(), 2, "an empty summary is not a change");
        assert_eq!((st.changes[0].summary.as_str(), st.changes[0].role.as_str(), st.changes[0].files.as_slice()), ("reworked the permission model to be less intrusive", "builder", &["a.rs".to_string()][..]));
        assert_eq!((st.changes[0].session.as_str(), st.changes[1].session.as_str(), st.changes[1].role.as_str()), ("", "s_0002", "writer"), "the session is the newest one begun, and the payload names the role when the envelope does not");
        for i in 0..250 {
            st.apply(&ev(10 + i, "change.recorded", "t_1", "builder", json!({"summary": format!("c{}", i)})));
        }
        assert_eq!((st.changes.len(), st.changes.last().unwrap().summary.as_str()), (MAX_CHANGES, "c249"), "bounded, newest kept");
        assert!(KINDS.contains(&"change.recorded"));
    }

    #[test]
    fn bundle_and_org_context_events_become_clean_narrator_rows() {
        let mut st = ObserverState::new(&roles());
        st.apply(&ev(1, "bundle.added", "", "", json!({"org": "acme", "name": "platform\x1b[31m", "version": "1.4.0", "hash": "blake3:x"})));
        st.apply(&ev(2, "bundle.updated", "", "", json!({"org": "acme", "name": "platform", "version": "1.5.0"})));
        st.apply(&ev(3, "bundle.removed", "", "", json!({"org": "acme", "name": "platform"})));
        st.apply(&ev(4, "org_context.built", "t_1", "orchestrator", json!({"bundles": 2, "bytes": 3100, "trimmed": ["acme/platform"], "notes": []})));
        let rows: Vec<&str> = st.narrative.iter().map(|s| s.as_str()).collect();
        assert_eq!(rows, vec!["bundle acme/platform@1.4.0 added", "bundle acme/platform@1.5.0 updated", "bundle acme/platform removed", "org context built: 2 bundle(s), 3100 bytes, 1 trimmed"]);
        for k in ["bundle.added", "bundle.updated", "bundle.removed", "org_context.built"] {
            assert!(KINDS.contains(&k));
        }
    }

    #[test]
    fn named_threads_are_kept_cleaned_and_replaced_by_id() {
        let mut st = ObserverState::new(&roles());
        st.apply(&ev(1, "thread.named", "t_1", "orchestrator", json!({"thread": "pricing", "label": "Pricing \x1b[31mtiers\x1b[0m\n for the kb", "tasks": ["t_1", "t_2", ""]})));
        assert_eq!(st.threads, vec![Thread { id: "pricing".into(), label: "Pricing tiers for the kb".into(), tasks: vec!["t_1".into(), "t_2".into()] }]);
        st.apply(&ev(2, "thread.named", "t_1", "orchestrator", json!({"thread": "pricing", "label": "Pricing", "tasks": ["t_1"]})));
        assert_eq!((st.threads.len(), st.threads[0].label.as_str(), st.threads[0].tasks.len()), (1, "Pricing", 1), "the same id replaces, it does not add");
        st.apply(&ev(3, "thread.named", "t_1", "orchestrator", json!({"thread": "", "label": "no id"})));
        st.apply(&ev(4, "thread.named", "t_1", "orchestrator", json!({"thread": "x", "label": "  "})));
        assert_eq!(st.threads.len(), 1, "a thread needs an id and a label");
        for i in 0..60 {
            st.apply(&ev(10 + i, "thread.named", "t_1", "orchestrator", json!({"thread": format!("th{}", i), "label": "l", "tasks": []})));
        }
        assert_eq!(st.threads.len(), MAX_THREADS, "bounded");
        assert!(KINDS.contains(&"thread.named"));
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
        assert_eq!(st.default_view(), "dashboard", "a finished council no longer pulls the view");
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
        assert_eq!(st.default_view(), "dashboard", "a build in progress is on the dashboard, and in the epic pane");
        st.apply(&ev(7, "merge.approved", "t_3", "", json!({"sha": "x"})));
        st.apply(&ev(8, "action.result", "t_3", "", json!({"action_id": "merge-abc", "ok": true})));
        let b = &st.builds[0];
        assert_eq!((b.branch.as_str(), b.attempts.len(), b.sha.as_str(), b.gates.len(), b.findings.len(), b.merge.as_str()), ("delphi/t_3", 2, "0123456789ab", 2, 1, "merged"));
        assert!(st.narrative.iter().any(|l| l.contains("retrying")));
    }
}
