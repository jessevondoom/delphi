//! The agent loop (`specs/003` §1.2, §3.2; `specs/005` §2.3–2.5; `specs/002` §3.1).
//!
//! A *task* is one operator prompt. It becomes a root `HandoffEnvelope` and a *span* for the target
//! role. A span runs turns: compile the prompt for the chosen endpoint, **reserve the call's worst-case
//! cost first**, stream the reply, persist the whole turn (blocks, completion marker, usage) in one
//! fsync'd batch, then run any tool calls — each classified before it runs, asked about if it needs the
//! operator, gated by the span's taint — and loop. Dispatch to a child role is a tool call, so the
//! child inherits the parent's taint ceiling and a tainted parent needs a Red approval to dispatch.
//!
//! Failures never silently degrade: an exhausted budget, throttled endpoints, an expired credential
//! or repeated interruptions escalate to the operator, and failover only ever walks the ratified
//! endpoint order inside `allowed_endpoints`.

use crate::approval::{payload_blake3, Broker, Need, Request, Resolution, Tier as ApprovalTier};
use crate::exec;
use crate::handoff::{pointers_from_prompt, ContextPointer, HandoffEnvelope};
use crate::mission::{LiveRedactor, EngineError, MissionState, SecretRegistry};
use crate::model::{ModelClient, ModelError};
use crate::probe::ProjectProbe;
use crate::recovery::{self, Call, NodeStatus, Recovered};
use crate::tools::{self, ToolCtx};
use crate::workspace::{AgentManifest, WorkspacePaths};
use delphi_adapter::blocks::{ContentBlock, Turn};
use delphi_adapter::claude::{Compiled, CompileOptions, Completed, StreamEvent, Target, ToolDef};
use delphi_adapter::config::{Endpoint, SharedConfig};
use delphi_adapter::ipr::{ContextSlice, Ipr};
use delphi_core::event::EventEnvelope;
use delphi_core::hub::{DeltaKind, Hub};
use delphi_core::ledger::Ledger;
use delphi_core::pathguard::{Guard, Zones};
use delphi_core::provenance::{gate, Action, Gate, OriginRules, OutputCheck, Span};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const FAMILY: &str = "claude";
pub const PROVIDER: &str = "anthropic";

#[derive(Debug, Clone)]
pub struct Settings {
    pub approval_timeout: Duration,
    /// Output budget per call, raised to leave room above the thinking budget.
    pub max_tokens: u64,
    pub max_dispatch_depth: u32,
    /// `mission_budget_usd` (D-10): the ceiling a new mission starts with.
    pub mission_budget_usd: f64,
    /// Escalate when this many consecutive interruptions hit the same turn (`specs/005` §2.5).
    pub max_interruptions: u32,
    pub enabled_agents: Vec<String>,
    /// The Claude CLI to launch for a harness endpoint.
    pub harness_program: String,
    /// The GitHub CLI used to open pull requests in PR mode.
    pub gh_program: String,
    /// Attempts the unified retry ladder makes before halting for the operator (`specs/003` §3.2).
    pub build_attempts: u32,
    /// The Antigravity CLI to launch for an `agy` harness endpoint.
    pub agy_program: String,
    /// Tokens (input, cache writes and output; not cache reads) one harness span may use before it is stopped.
    pub harness_token_cap: u64,
    /// A harness that says nothing for this long is killed.
    pub harness_idle: Duration,
    /// `council_budget_usd` (D-10): a separate ceiling for one Council.
    pub council_budget_usd: f64,
    /// A Council seat that has not answered by then abstains.
    pub council_seat_timeout: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            approval_timeout: crate::approval::DEFAULT_TIMEOUT,
            max_tokens: 8192,
            max_dispatch_depth: 2,
            mission_budget_usd: 5.0,
            max_interruptions: 2,
            enabled_agents: ["orchestrator", "architect", "builder", "red-team", "strategist"].iter().map(|s| s.to_string()).collect(),
            harness_program: "claude".into(),
            gh_program: "gh".into(),
            build_attempts: 3,
            agy_program: "agy".into(),
            harness_token_cap: 500_000,
            harness_idle: Duration::from_secs(300),
            council_budget_usd: 20.0,
            council_seat_timeout: Duration::from_secs(600),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Done,
    /// Stopped and handed to the operator, with the reason.
    Escalated(String),
}

#[derive(Debug, Clone)]
pub struct SpanOutcome {
    pub status: Status,
    pub text: String,
    pub turns: u32,
    /// The span's taint ceiling when it finished; whoever consumes `text` inherits it.
    pub ceiling: delphi_core::provenance::Tier,
}

#[derive(Debug, Clone)]
pub struct RecoveryItem {
    pub task_id: String,
    pub role: String,
    pub span_id: String,
    /// What will happen on resume, in words for the operator.
    pub action: String,
}

pub struct RecoveryPlan {
    pub rec: Arc<Recovered>,
    pub items: Vec<RecoveryItem>,
    /// Spend charged for calls interrupted by the restart (their full reservations).
    pub charged_usd: f64,
    pub expired_approvals: Vec<String>,
}

impl RecoveryPlan {
    pub fn to_json(&self) -> Value {
        json!({"tasks": self.items.iter().map(|i| json!({"task_id": i.task_id, "role": i.role, "span_id": i.span_id, "action": i.action})).collect::<Vec<_>>(),
               "charged_usd": self.charged_usd, "expired_approvals": self.expired_approvals})
    }
}

#[derive(Debug, Clone)]
pub struct TaskOutcome {
    pub task_id: String,
    pub status: Status,
    pub text: String,
}

pub struct Engine {
    pub ws: WorkspacePaths,
    pub cfg: SharedConfig,
    pub hub: Hub,
    pub broker: Arc<Broker>,
    pub model: Arc<dyn ModelClient>,
    pub secrets: Arc<SecretRegistry>,
    pub ledger: Arc<Mutex<Ledger>>,
    pub settings: Settings,
    pub probe: Arc<dyn ProjectProbe>,
    /// Registered projects by name (read-only through `@project/<name>/**`).
    pub projects: Vec<(String, PathBuf)>,
    pub rules: OriginRules,
    pub clock: Arc<dyn Fn() -> String + Send + Sync>,
    /// The tools offered to agents (`tools::Registry`).
    pub tools: Arc<tools::Registry>,
    /// Sandboxed execution and task worktrees (`exec.rs`).
    pub exec: Arc<exec::ExecEnv>,
    /// Context slices a driver hands a span directly, by handoff id (Council Round 2 peers' stances).
    pub(crate) injected: Mutex<HashMap<String, Vec<delphi_adapter::ipr::ContextSlice>>>,
    /// The connector host (`specs/010`): MCP sessions, pins, the payload store.
    pub connectors: Arc<crate::connectors::ConnectorHost>,
}

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How a span begins.
enum Start {
    Fresh { handoff: Box<HandoffEnvelope>, parent: Option<Span>, depth: u32 },
    /// Continue a span rebuilt from the log; `rec` lets it resume the children it had dispatched.
    Resume { rec: Arc<Recovered>, span_id: String },
}

/// Who an approval card is raised for.
pub(crate) struct Asker<'a> {
    pub m: &'a Arc<MissionState>,
    pub task_id: &'a str,
    pub span_id: &'a str,
    pub role: &'a str,
}

/// The reason when every API endpoint of a family has been tried.
pub(crate) const EXHAUSTED: &str = "every allowed endpoint is throttled or unavailable";

/// Why a span stopped without finishing; becomes `Status::Escalated`.
pub(crate) struct Escalate(pub(crate) String);

/// Everything one span needs while it runs.
pub(crate) struct SpanCtx {
    pub(crate) m: Arc<MissionState>,
    pub(crate) task_id: String,
    pub(crate) span_id: String,
    pub(crate) role: String,
    pub(crate) manifest: AgentManifest,
    pub(crate) guard: Guard,
    pub(crate) span: Span,
    pub(crate) depth: u32,
    pub(crate) tier: String,
    /// Which model family serves this span (`claude` unless the handoff says otherwise).
    pub(crate) family: String,
    pub(crate) thinking: u32,
    /// Set when a dispatched child halted: the parent halts too, and the operator decides (specs/003 §3.2).
    pub(crate) child_escalation: Option<String>,
    /// Set by a tool whose result text is kept outside the event log: the log records this reference instead.
    pub(crate) content_ref: Option<String>,
}

impl SpanCtx {
    pub(crate) fn ev(&self, kind: &str, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new(&self.m.id, kind, payload);
        e.task_id = Some(self.task_id.clone());
        e.span_id = Some(self.span_id.clone());
        e.agent = Some(self.role.clone());
        e
    }

    pub(crate) fn agent_ev(&self, kind: &str, payload: Value, ep: &Endpoint, model_id: &str) -> EventEnvelope {
        let mut e = self.ev(kind, payload);
        e.endpoint_id = Some(ep.id.clone());
        e.access_path = Some(ep.path.as_str().to_string());
        e.model_tier = Some(self.tier.clone());
        e.model_id = Some(model_id.to_string());
        e.attempt = Some(1);
        e.mode = Some(if self.depth == 0 { "direct" } else { "dag" }.to_string());
        e
    }
}

fn final_text(blocks: &[ContentBlock]) -> String {
    blocks.iter().filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("\n")
}

fn tool_result_error(msg: impl Into<String>) -> tools::ToolResult {
    tools::ToolResult { content: msg.into(), is_error: true }
}

fn run_command_def() -> ToolDef {
    ToolDef {
        name: "run_command".into(),
        description: "Run a program in your task worktree (killed after a timeout). Give `argv` (the program, then its arguments; there is no shell, so pipes and redirects do not work). Use it to run the tests, the build and linters. Unless the operator has set a rule allowing it, each command is shown to the operator for approval, so prefer a few meaningful commands over many small ones. Output is truncated to its tail.".into(),
        input_schema: json!({"type": "object", "required": ["argv"], "properties": {"argv": {"type": "array", "items": {"type": "string"}}, "timeout_secs": {"type": "integer"}}}),
    }
}

fn dispatch_def() -> ToolDef {
    ToolDef {
        name: "dispatch_agent".into(),
        description: "Hand a self-contained task to another enabled role (for example `architect` to draft a spec, `red-team` to audit one, `strategist` to check scope). Give an objective and pointers to the files it needs; do not paste file contents. Returns that role's final report.".into(),
        input_schema: json!({"type": "object", "required": ["role", "objective"], "properties": {
            "role": {"type": "string"}, "objective": {"type": "string"},
            "pointers": {"type": "array", "items": {"type": "object", "required": ["uri", "reason"], "properties": {"uri": {"type": "string"}, "reason": {"type": "string"}}}}}}),
    }
}

impl Engine {
    pub(crate) fn now(&self) -> String {
        (self.clock)()
    }

    pub(crate) fn guard_for(&self, manifest: &AgentManifest) -> Guard {
        let ws = &self.ws.dir;
        let mut protected = HashSet::new();
        for p in [ws.join("local/ratified.lock"), ws.join("local/state.db"), self.ws.home.join("operator/ratified.lock")] {
            if let Ok(md) = std::fs::metadata(&p) {
                use std::os::unix::fs::MetadataExt;
                protected.insert((md.dev(), md.ino()));
            }
        }
        Guard {
            zones: Zones { local: ws.join("local"), team: ws.join("team"), base: ws.join("base"), ratified: ws.join("local/ratified"), worktree: None, projects: self.projects.clone(), payload: Some(ws.join("payloads")) },
            role: manifest.id.clone(),
            read_globs: manifest.read.clone(),
            write_globs: manifest.write.clone(),
            protected,
        }
    }

    /// Run one operator prompt as a task: root handoff → span for `target` → outcome.
    pub async fn run_task(self: &Arc<Self>, m: &Arc<MissionState>, target: &str, prompt: &str) -> Result<TaskOutcome, EngineError> {
        let task_id = m.next_task_id();
        self.run_task_with_id(m, task_id, target, prompt).await
    }

    /// As [`Engine::run_task`] for a task id the caller already allocated (so it can be reported
    /// to the console before the task finishes).
    pub async fn run_task_with_id(self: &Arc<Self>, m: &Arc<MissionState>, task_id: String, target: &str, prompt: &str) -> Result<TaskOutcome, EngineError> {
        self.start_task(m, task_id, target, prompt, true).await
    }

    async fn start_task(self: &Arc<Self>, m: &Arc<MissionState>, task_id: String, target: &str, prompt: &str, announce: bool) -> Result<TaskOutcome, EngineError> {
        let mut handoff = HandoffEnvelope::root(&m.id, &task_id, target, prompt, pointers_from_prompt(prompt));
        if target == "builder" {
            // `@specs/011-x.md` in the prompt is the spec this build implements (`specs/003` §3.1.2).
            handoff.implements_spec = handoff.context_pointers.iter().find(|p| p.uri.starts_with("specs/") && p.uri.ends_with(".md")).map(|p| p.uri.clone());
        }
        handoff.validate().map_err(EngineError::Invalid)?;
        if !self.settings.enabled_agents.iter().any(|a| a == target) {
            return Err(EngineError::Invalid(format!("`{}` is not an enabled role", target)));
        }
        if announce {
            let mut created = EventEnvelope::new(&m.id, "task.created", json!({"prompt": prompt, "target": target}));
            created.task_id = Some(task_id.clone());
            m.append(created).await?;
        }
        let outcome = self.run_span(m, &task_id, handoff, None, 0).await;
        self.broker.end_task(&task_id);
        self.finish_task(m, task_id, outcome?).await
    }

    /// Record a task's final output and completion.
    pub(crate) async fn finish_task(self: &Arc<Self>, m: &Arc<MissionState>, task_id: String, outcome: SpanOutcome) -> Result<TaskOutcome, EngineError> {
        let mut out = EventEnvelope::new(&m.id, "task.output", json!({"status": match &outcome.status { Status::Done => "done", Status::Escalated(_) => "escalated" }, "text": outcome.text}));
        out.task_id = Some(task_id.clone());
        m.append(out).await?;
        let mut done = EventEnvelope::new(&m.id, "task.completed", json!({"status": match &outcome.status { Status::Done => "done", Status::Escalated(_) => "escalated" }, "reason": match &outcome.status { Status::Escalated(r) => r.as_str(), _ => "" }}));
        done.task_id = Some(task_id.clone());
        m.append(done).await?;
        Ok(TaskOutcome { task_id, status: outcome.status, text: outcome.text })
    }

    pub(crate) fn run_span<'a>(self: &'a Arc<Self>, m: &'a Arc<MissionState>, task_id: &'a str, handoff: HandoffEnvelope, parent: Option<Span>, depth: u32) -> BoxFut<'a, Result<SpanOutcome, EngineError>> {
        self.drive(m, task_id, Start::Fresh { handoff: Box::new(handoff), parent, depth })
    }

    /// Run a span from its start, or pick it up where the log says it stopped.
    fn drive<'a>(self: &'a Arc<Self>, m: &'a Arc<MissionState>, task_id: &'a str, start: Start) -> BoxFut<'a, Result<SpanOutcome, EngineError>> {
        Box::pin(async move {
            // Who this span is, and (when resuming) what it had already done.
            let new_nonce = || delphi_core::server::random_token().map(|t| t[..12].to_string()).unwrap_or_else(|_| "nonce".into());
            let (handoff, span_id, depth, span, mut history, mut pending, mut known, mut interruptions, rec, fresh, nonce) = match start {
                Start::Fresh { handoff, parent, depth } => {
                    let handoff = *handoff;
                    let span_id = m.next_span_id(&handoff.target_agent);
                    let span = match &parent {
                        Some(p) => p.child(&span_id),
                        None => Span::root(&span_id),
                    };
                    (handoff, span_id, depth, span, Vec::new(), None, BTreeMap::new(), 0u32, None, true, new_nonce())
                }
                Start::Resume { rec, span_id } => {
                    let st = rec.spans.get(&span_id).ok_or_else(|| EngineError::Invalid(format!("no recorded span `{}`", span_id)))?.clone();
                    let handoff = st.handoff.clone().ok_or_else(|| EngineError::Invalid(format!("span `{}` has no recorded handoff", span_id)))?;
                    let pending = if st.pending.is_empty() { None } else { Some(st.pending.clone()) };
                    let nonce = st.nonce.clone().unwrap_or_else(new_nonce);
                    (handoff, span_id.clone(), st.depth, Span::with_ceiling(&span_id, st.ceiling), st.history.clone(), pending, st.known_results.clone(), st.interruptions, Some(rec), false, nonce)
                }
            };
            let role = handoff.target_agent.clone();
            let manifest = self.ws.load_manifest(&role)?;
            let snapshot = self.ws.load_snapshot(&role)?;
            let tier = handoff.routing_directive.model_tier.clone().unwrap_or_else(|| manifest.tier().to_string());
            let family = handoff.routing_directive.model_family.clone().unwrap_or_else(|| self.cfg.get().family_for_role(&role).to_string());
            let thinking = handoff.routing_directive.thinking_budget_tokens.unwrap_or_else(|| manifest.thinking_budget());
            let mut sc = SpanCtx { m: m.clone(), task_id: task_id.to_string(), span_id: span_id.clone(), role: role.clone(), guard: self.guard_for(&manifest), manifest, span, depth, tier, family, thinking, child_escalation: None, content_ref: None };

            if fresh {
                let mut open = vec![
                    sc.ev("dag.node.added", json!({"node_id": span_id, "role": role, "parent": handoff.parent_span_id, "depth": depth, "ceiling": sc.span.ceiling().as_str(), "nonce": nonce})),
                    sc.ev("handoff.sent", json!({"handoff": serde_json::to_value(&handoff).expect("handoff serializes")})),
                    sc.ev("dag.node.state", json!({"node_id": span_id, "state": "running"})),
                ];
                for t in &snapshot.trims {
                    open.push(sc.ev("snapshot.trim", json!({"note": t})));
                }
                m.append_batch(open).await?;
            }

            // Other roles of a task that has a worktree (an auditor) can read it; their write globs keep them out.
            if let Some(w) = self.exec.known_worktree(task_id) {
                sc.guard.zones.worktree = Some(w.path.clone());
            }
            // A role that writes to `@worktree/**` works in its task's worktree, cut from the project's HEAD.
            let mut setup_error: Option<String> = None;
            if sc.manifest.write.iter().any(|g| g.starts_with("@worktree/")) {
                if let Err(e) = self.attach_worktree(&mut sc, &handoff).await {
                    setup_error = Some(e);
                }
            }

            // Block 5: resolve each pointer as this role; the span's ceiling is clamped before turn 1.
            let mut slices = Vec::new();
            let before = sc.span.ceiling();
            let mut stamps_unused = Vec::new();
            for p in &handoff.context_pointers {
                let resolved = {
                    let mut idx = m.index.lock().unwrap();
                    let now = self.now();
                    let mut ctx = ToolCtx { guard: &sc.guard, span: &mut sc.span, index: &mut idx, rules: &self.rules, probe: &*self.probe, role: &sc.role, mission_id: &m.id, now_rfc3339: &now, stamps: vec![] };
                    let r = tools::read_pointer(&mut ctx, &p.uri);
                    stamps_unused.append(&mut ctx.stamps);
                    r
                };
                match resolved {
                    Ok((text, tier)) => slices.push(ContextSlice { label: p.uri.clone(), tier: tier.as_str().into(), text }),
                    Err(e) => slices.push(ContextSlice { label: p.uri.clone(), tier: "unavailable".into(), text: format!("(could not be read: {})", e) }),
                }
            }
            if let Some(extra) = self.injected.lock().unwrap().get(&handoff.handoff_id) {
                slices.extend(extra.iter().cloned());
            }
            if sc.span.ceiling() != before {
                m.append(sc.ev("span.taint", json!({"ceiling": sc.span.ceiling().as_str(), "cause": "context pointers"}))).await?;
            }
            let ipr = Ipr {
                governance: snapshot.governance,
                role: sc.manifest.body.clone(),
                persona: None,
                heuristics: snapshot.heuristics,
                lessons: snapshot.lessons,
                context_slices: slices,
                contract: handoff.contract_text(),
                reminder: Some(handoff.reminder_text()),
                nonce,
            };
            let mut defs = self.tools.defs(!sc.manifest.write.is_empty());
            if sc.manifest.execute.iter().any(|e| e == "agent-dispatch") {
                defs.push(dispatch_def());
            }
            if sc.guard.zones.worktree.is_some() && sc.manifest.execute.iter().any(|e| e == "shell-worktree-scoped") {
                defs.push(run_command_def());
            }
            defs.extend(self.connector_defs(&sc).await?);

            let mut turns = history.iter().filter(|t| t.role == delphi_adapter::blocks::Role::Assistant).count() as u32;
            // A family whose first allowed endpoint is a harness runs the whole span in the subscription CLI
            // (`harness_run.rs`). If that cannot be done without repeating a side effect, the same span is
            // tried by the family's API endpoints; and the other way round when every API endpoint is down.
            let mut harness_tried = false;
            let mut harness_result: Option<Result<String, Escalate>> = None;
            if let Some(hep) = if setup_error.is_none() { self.harness_endpoint(&sc.family) } else { None } {
                harness_tried = true;
                match self.run_harness(&mut sc, &ipr, &hep).await? {
                    Ok(text) => harness_result = Some(Ok(text)),
                    Err(f) if f.retryable && self.has_api_endpoint(&sc.family) => {
                        m.append(sc.ev("harness.fallback", json!({"from": hep.id, "to": "api", "reason": self.secrets.redact(&f.reason)}))).await?;
                    }
                    Err(f) => harness_result = Some(Err(Escalate(f.reason))),
                }
            }
            let outcome = if let Some(r) = harness_result { r } else { loop {
                if let Some(e) = setup_error.take() {
                    break Err(Escalate(e));
                }
                // Tool calls still owed a result (a fresh tool_use turn, or ones interrupted by a crash).
                if let Some(calls) = pending.take() {
                    let result_index = history.len() as u32 + 1;
                    let mut results = Vec::new();
                    for Call { id, name, args } in calls {
                        let r = match known.remove(&id) {
                            Some((content, is_error)) => tools::ToolResult { content, is_error }, // already persisted before the interruption
                            None => {
                                let r = self.exec_tool(&mut sc, &id, &name, &args, &rec).await?;
                                // Durable before the next tool runs (or waits for an approval): a crash must not re-run a finished side effect.
                                let payload = match sc.content_ref.take() {
                                    // A connector result: the log holds a pointer to the payload store, never the text.
                                    Some(h) => json!({"turn_index": result_index, "tool_call_id": id, "name": name, "content": format!("[connector result stored as @payload/{}, {} characters]", h, r.content.chars().count()), "content_ref": h, "is_error": r.is_error}),
                                    None => json!({"turn_index": result_index, "tool_call_id": id, "name": name, "content": r.content, "is_error": r.is_error}),
                                };
                                m.append(sc.ev("tool.result", payload)).await?;
                                r
                            }
                        };
                        results.push(ContentBlock::ToolResult { tool_call_id: id, content: r.content, is_error: r.is_error });
                    }
                    history.push(Turn::user(result_index, results));
                    if let Some(why) = sc.child_escalation.take() {
                        break Err(Escalate(why));
                    }
                }
                if turns >= sc.manifest.max_turn_budget {
                    break Err(Escalate(format!("`{}` used its whole turn budget ({}) without finishing", sc.role, sc.manifest.max_turn_budget)));
                }
                let turn_index = history.len() as u32 + 1;
                let (done, ep, model_id) = match self.model_turn(&sc, &ipr, &history, &defs, turn_index, interruptions).await? {
                    Ok(t) => t,
                    Err(esc) => {
                        if esc.0.starts_with(EXHAUSTED) && !harness_tried {
                            if let Some(hep) = self.any_harness_endpoint(&sc.family) {
                                m.append(sc.ev("harness.fallback", json!({"from": "api", "to": hep.id, "reason": esc.0}))).await?;
                                break match self.run_harness(&mut sc, &ipr, &hep).await? {
                                    Ok(text) => Ok(text),
                                    Err(f) => Err(Escalate(format!("{}; then the {} harness: {}", esc.0, hep.id, f.reason))),
                                };
                            }
                        }
                        break Err(esc);
                    }
                };
                interruptions = 0;
                turns += 1;
                history.push(Turn::assistant(turn_index, delphi_adapter::provider_for(&ep), &ep.id, &model_id, done.blocks.clone()));
                let calls: Vec<Call> = done.blocks.iter().filter_map(|b| if let ContentBlock::ToolCall { id, name, arguments } = b { Some(Call { id: id.clone(), name: name.clone(), args: arguments.clone() }) } else { None }).collect();
                if done.stop_reason.as_deref() != Some("tool_use") || calls.is_empty() {
                    let mut text = final_text(&done.blocks);
                    if done.stop_reason.as_deref() == Some("max_tokens") {
                        text.push_str("\n[the reply was cut off at the output limit]");
                    }
                    break Ok(text);
                }
                pending = Some(calls);
            } };

            let (status, text) = match outcome {
                Ok(text) => (Status::Done, text),
                Err(Escalate(reason)) => (Status::Escalated(reason.clone()), format!("[escalated to the operator: {}]", reason)),
            };
            let state = match &status {
                Status::Done => json!({"node_id": sc.span_id, "state": "done"}),
                Status::Escalated(r) => json!({"node_id": sc.span_id, "state": "escalated", "reason": r}),
            };
            m.append(sc.ev("dag.node.state", state)).await?;
            Ok(SpanOutcome { status, text, turns, ceiling: sc.span.ceiling() })
        })
    }

    // ── recovery (`specs/005` §2.5) ─────────────────────────────────────────

    /// After a restart: charge the calls that died with the daemon their full reservation, close approval
    /// cards that died with it, rebuild every open task from the log, record `mission.recovered`, and
    /// return the plan. Nothing resumes until [`Engine::resume_task`] is called, so the operator sees the
    /// summary first. `None` means there is nothing to recover.
    pub async fn recover_mission(self: &Arc<Self>, m: &Arc<MissionState>) -> Result<Option<RecoveryPlan>, EngineError> {
        let events = m.replay()?;
        let before = recovery::rebuild_with(&events, &|h| self.connectors.read_payload(h));
        let open_reservations = m.budget.lock().unwrap().open_reservations();
        let unfinished = before.tasks.values().any(|t| !t.completed);
        if !unfinished && open_reservations.is_empty() && before.open_approvals.is_empty() {
            return Ok(None);
        }
        let mut batch = Vec::new();
        let mut charged = 0.0;
        for (rid, amount) in open_reservations {
            let Some(res) = events.iter().find(|e| e.kind == "budget.reserve" && e.payload["reservation_id"] == rid.as_str()) else { continue };
            // The usage of an interrupted call can't be known: charge the whole reservation.
            let usage = delphi_core::usage::UsagePayload {
                provider: res.endpoint_id.as_deref().and_then(|id| self.cfg.get().endpoint(id).map(delphi_adapter::provider_for)).unwrap_or(PROVIDER).into(), input_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 0, thinking_tokens: 0,
                cost_usd: amount as f64 / 1e6, reserved_usd: amount as f64 / 1e6, usage_source: "estimated".into(), cost_model: "metered".into(), outcome: "cancelled".into(), reservation_id: Some(rid.clone()),
            };
            let mut u = EventEnvelope::new(&m.id, "agent.usage", usage.to_value());
            (u.task_id, u.span_id, u.agent, u.endpoint_id, u.access_path, u.model_tier, u.model_id, u.attempt, u.mode) =
                (res.task_id.clone(), res.span_id.clone(), res.agent.clone(), res.endpoint_id.clone(), res.access_path.clone(), res.model_tier.clone(), res.model_id.clone(), res.attempt, res.mode.clone());
            batch.push(u);
            let mut t = EventEnvelope::new(&m.id, "turn.interrupted", json!({"turn_index": res.payload["turn_index"], "recovered": true}));
            (t.task_id, t.span_id, t.agent) = (res.task_id.clone(), res.span_id.clone(), res.agent.clone());
            batch.push(t);
            charged += amount as f64 / 1e6;
        }
        // An approval nobody answered is not a grant: the action is asked about again after the restart.
        for id in &before.open_approvals {
            batch.push(EventEnvelope::new(&m.id, "approval.resolved", json!({"approval_id": id, "resolution": "expired", "by": "recovery", "reason": "the daemon restarted; the action will be asked about again"})));
        }
        if !batch.is_empty() {
            m.append_batch(batch).await?;
        }

        let rec = Arc::new(recovery::rebuild_with(&m.replay()?, &|h| self.connectors.read_payload(h)));
        let mut items = Vec::new();
        for t in rec.tasks.values().filter(|t| !t.completed) {
            let Some(root) = t.root_span.as_ref().and_then(|id| rec.spans.get(id)) else {
                items.push(RecoveryItem { task_id: t.task_id.clone(), role: t.target.clone(), span_id: String::new(), action: "start again from your saved request (it had not begun)".into() });
                continue;
            };
            let open: Vec<&recovery::SpanState> = rec.spans.values().filter(|s| s.task_id == t.task_id && s.is_open()).collect();
            let action = if open.is_empty() {
                "close it (it had already finished)".to_string()
            } else if let Some(s) = open.iter().find(|s| s.interruptions >= self.settings.max_interruptions) {
                format!("stop and ask you: {}'s turn was interrupted {} times in a row", s.role, s.interruptions)
            } else {
                open.iter().map(|s| if s.pending.is_empty() { format!("{} retries its interrupted turn", s.role) } else { format!("{} finishes {} tool call(s), then continues", s.role, s.pending.len()) }).collect::<Vec<_>>().join("; ")
            };
            items.push(RecoveryItem { task_id: t.task_id.clone(), role: root.role.clone(), span_id: root.span_id.clone(), action });
        }
        let plan = RecoveryPlan { rec, items, charged_usd: charged, expired_approvals: before.open_approvals };
        m.append(EventEnvelope::new(&m.id, "mission.recovered", plan.to_json())).await?;
        Ok(Some(plan))
    }

    /// Resume one task from the log.
    pub async fn resume_task(self: &Arc<Self>, m: &Arc<MissionState>, rec: &Arc<Recovered>, task_id: &str) -> Result<TaskOutcome, EngineError> {
        let t = rec.tasks.get(task_id).ok_or_else(|| EngineError::Invalid(format!("no recorded task `{}`", task_id)))?;
        if t.completed {
            return Err(EngineError::Invalid(format!("task `{}` already completed", task_id)));
        }
        let Some(root) = t.root_span.clone() else {
            // It never started a span: run it from the saved request.
            return self.start_task(m, task_id.to_string(), &t.target, &t.prompt, false).await;
        };
        let st = rec.spans.get(&root).ok_or_else(|| EngineError::Invalid(format!("no recorded span `{}`", root)))?;
        let outcome = match &st.status {
            NodeStatus::Running => self.drive(m, task_id, Start::Resume { rec: rec.clone(), span_id: root }).await?,
            NodeStatus::Done => SpanOutcome { status: Status::Done, text: st.last_text(), turns: 0, ceiling: st.ceiling },
            NodeStatus::Escalated(r) => SpanOutcome { status: Status::Escalated(r.clone()), text: format!("[escalated to the operator: {}]", r), turns: 0, ceiling: st.ceiling },
        };
        self.broker.end_task(task_id);
        self.finish_task(m, task_id.to_string(), outcome).await
    }

    /// One model turn: pick an endpoint from the ratified order, reserve, call, persist. `Err(Escalate)`
    /// means stop and hand to the operator; `Err(EngineError)` (outer) is a fatal storage failure.
    async fn model_turn(&self, sc: &SpanCtx, ipr: &Ipr, history: &[Turn], defs: &[ToolDef], turn_index: u32, prior_interruptions: u32) -> Result<Result<(Completed, Endpoint, String), Escalate>, EngineError> {
        let cfg = self.cfg.get();
        // Harness endpoints run whole spans (`harness_run.rs`); the turn-by-turn loop uses API endpoints only.
        let endpoints: Vec<Endpoint> = cfg.endpoints_for(&sc.family).into_iter().filter(|e| e.path != delphi_adapter::config::AccessPath::Harness).cloned().collect();
        if endpoints.is_empty() {
            return Ok(Err(Escalate(format!("no allowed {} API endpoint is configured (see `delphi doctor`)", sc.family))));
        }
        let m = &sc.m;
        let mut interruptions = prior_interruptions;
        if interruptions >= self.settings.max_interruptions {
            return Ok(Err(Escalate(format!("the same turn was interrupted {} times in a row", interruptions))));
        }
        let mut exhausted_note: Option<String> = None;
        'endpoints: for ep in &endpoints {
            loop {
                let Some(model_id) = cfg.resolve_model(ep, &sc.tier) else { continue 'endpoints };
                let Some(entry) = cfg.catalog_entry(&model_id) else {
                    return Ok(Err(Escalate(format!("model `{}` has no [models.catalog] entry, so its cost can't be reserved", model_id))));
                };
                if !entry.has_prices() {
                    return Ok(Err(Escalate(format!("model `{}` has no prices, so its cost can't be reserved", model_id))));
                }
                let price = entry.price();
                let thinking = cfg.clamp_thinking(&model_id, sc.thinking);
                let max_tokens = self.settings.max_tokens.max(thinking.unwrap_or(0) as u64 + 2048).min(entry.max_output_tokens.max(thinking.unwrap_or(0) as u64 + 1025));
                let opts = CompileOptions { max_tokens, thinking_budget: thinking, temperature: None, tools: defs.to_vec(), betas: ep.betas.clone() };
                let target = Target { provider: delphi_adapter::provider_for(ep).into(), endpoint_id: ep.id.clone(), model_id: model_id.clone() };
                let compiled: Compiled = match delphi_adapter::compile_for(ep, ipr, history, &target, &opts) {
                    Ok(c) => c,
                    Err(e) => return Ok(Err(Escalate(format!("could not build the request: {}", e)))),
                };
                let body_bytes = serde_json::to_vec(&compiled.body).map(|b| b.len()).unwrap_or(0) as u64;
                let system_bytes = serde_json::to_vec(delphi_adapter::system_of(ep, &compiled.body)).map(|b| b.len()).unwrap_or(0) as u64;
                let amount = price.worst_case_micro(body_bytes, system_bytes, max_tokens);

                // Reserve before dispatch; the gate makes parallel spans see each other's reservations.
                let rid = {
                    let _gate = m.reserve_gate.lock().await;
                    let available = m.budget.lock().unwrap().available();
                    if amount > available {
                        return Ok(Err(Escalate(format!("mission budget: this call could cost up to ${:.4} but only ${:.4} remains (raise it with /budget)", amount as f64 / 1e6, available as f64 / 1e6))));
                    }
                    let rid = m.next_reservation_id();
                    m.append(sc.agent_ev("budget.reserve", json!({"reservation_id": rid, "amount": amount, "unit": "micro_usd", "turn_index": turn_index}), ep, &model_id)).await?;
                    rid
                };

                let hub: Hub = self.hub.clone();
                let span_id = sc.span_id.clone();
                let secrets = sc.m.secrets();
                // Live deltas are redacted too. A secret split across chunks is caught by holding back the tail
                // of each stream that could still be the start of one.
                let mut text_tail = LiveRedactor::new(secrets.clone());
                let mut think_tail = LiveRedactor::new(secrets);
                let mut on_event = move |e: StreamEvent| match e {
                    StreamEvent::TextDelta { text, .. } => hub.push_delta(&span_id, DeltaKind::Text, &text_tail.push(&text)),
                    StreamEvent::ThinkingDelta { text, .. } => hub.push_delta(&span_id, DeltaKind::Thinking, &think_tail.push(&text)),
                    StreamEvent::BlockComplete { .. } => {
                        for (kind, tail) in [(DeltaKind::Text, text_tail.finish()), (DeltaKind::Thinking, think_tail.finish())] {
                            if !tail.is_empty() {
                                hub.push_delta(&span_id, kind, &tail);
                            }
                        }
                    }
                };
                let result = self.model.call(ep, &model_id, &compiled, &mut on_event).await;
                match result {
                    Ok(done) => {
                        let payload = done.usage.to_payload_for(delphi_adapter::provider_for(ep), Some(&price), amount as f64 / 1e6, Some(rid.clone()), "ok", "reported");
                        let mut batch: Vec<EventEnvelope> = done
                            .blocks
                            .iter()
                            .enumerate()
                            .map(|(i, b)| sc.agent_ev("agent.block.complete", json!({"turn_index": turn_index, "role": "assistant", "block_index": i, "block": serde_json::to_value(b).expect("blocks serialize")}), ep, &model_id))
                            .collect();
                        // The completion marker is last: a turn without it is discarded on recovery.
                        batch.push(sc.agent_ev("agent.turn.complete", json!({"turn_index": turn_index, "stop_reason": done.stop_reason, "blocks": done.blocks.len(), "provider": delphi_adapter::provider_for(ep)}), ep, &model_id));
                        batch.push(sc.agent_ev("agent.usage", payload.to_value(), ep, &model_id));
                        m.append_batch(batch).await?;
                        return Ok(Ok((done, ep.clone(), model_id)));
                    }
                    Err(ModelError::Throttled(msg)) => {
                        m.append(sc.agent_ev("budget.release", json!({"reservation_id": rid, "reason": "throttled"}), ep, &model_id)).await?;
                        m.append(sc.ev("endpoint.throttled", json!({"endpoint_id": ep.id, "message": self.secrets.redact(&msg)}))).await?;
                        continue 'endpoints; // the next allowed endpoint, in ratified order
                    }
                    Err(ModelError::Exhausted(x)) => {
                        // Out of quota, not slow: no retry here (`specs/002` §4.1). Say why and when it returns, then try the next endpoint.
                        m.append(sc.agent_ev("budget.release", json!({"reservation_id": rid, "reason": "quota exhausted"}), ep, &model_id)).await?;
                        m.append(sc.ev("model.exhausted", json!({"endpoint_id": ep.id, "model_id": model_id, "reason": x.reason, "reset_at": x.reset_at, "raw": self.secrets.redact(&x.raw)}))).await?;
                        exhausted_note.get_or_insert_with(|| format!("{} is out of quota ({})", ep.id, self.secrets.redact(&x.to_string())));
                        continue 'endpoints;
                    }
                    Err(ModelError::Interrupted(msg)) => {
                        // The usage of an interrupted call is unknowable: charge the full reservation.
                        let mut u = delphi_adapter::claude::Usage::default().to_payload_for(delphi_adapter::provider_for(ep), Some(&price), amount as f64 / 1e6, Some(rid.clone()), "cancelled", "estimated");
                        u.input_tokens = body_bytes.div_ceil(4);
                        u.cost_usd = amount as f64 / 1e6;
                        m.append_batch(vec![sc.agent_ev("agent.usage", u.to_value(), ep, &model_id), sc.ev("turn.interrupted", json!({"turn_index": turn_index, "message": self.secrets.redact(&msg)}))]).await?;
                        interruptions += 1;
                        if interruptions >= self.settings.max_interruptions {
                            return Ok(Err(Escalate(format!("the same turn was interrupted {} times in a row", interruptions))));
                        }
                    }
                    Err(ModelError::NeedsReauth { endpoint }) => {
                        m.append(sc.agent_ev("budget.release", json!({"reservation_id": rid, "reason": "needs re-authentication"}), ep, &model_id)).await?;
                        m.append(sc.ev("endpoint.reauth_required", json!({"endpoint_id": endpoint}))).await?;
                        return Ok(Err(Escalate(format!("re-authenticate {}", endpoint))));
                    }
                    Err(ModelError::Refused(msg)) | Err(ModelError::Failed(msg)) => {
                        m.append(sc.agent_ev("budget.release", json!({"reservation_id": rid, "reason": "rejected"}), ep, &model_id)).await?;
                        return Ok(Err(Escalate(self.secrets.redact(&msg))));
                    }
                }
            }
        }
        let why = exhausted_note.map(|n| format!(": {}", n)).unwrap_or_default();
        Ok(Err(Escalate(format!("{}{}; waiting would not help right now", EXHAUSTED, why))))
    }

    /// Run one tool call: classify → taint gate → approval if needed → execute → persist stamps.
    async fn exec_tool(self: &Arc<Self>, sc: &mut SpanCtx, call_id: &str, name: &str, args: &Value, rec: &Option<Arc<Recovered>>) -> Result<tools::ToolResult, EngineError> {
        if name == "run_command" {
            return self.exec_command(sc, call_id, args).await;
        }
        if name == "dispatch_agent" {
            // The schema is only offered to roles that may dispatch; a call from any other role is refused here too.
            if !sc.manifest.execute.iter().any(|e| e == "agent-dispatch") {
                return Ok(tool_result_error("this role's manifest does not authorize agent-dispatch"));
            }
            return self.exec_dispatch(sc, call_id, args, rec).await;
        }
        if let Some((connector, tool)) = self.connectors.parse_wire(name) {
            return self.exec_connector(sc, call_id, &connector, &tool, args).await;
        }
        let c = match self.tools.classify(name, args) {
            Ok(c) => c,
            Err(e) => return Ok(tool_result_error(e.0)),
        };
        // Refuse what would fail anyway before the operator is asked about it.
        if name == "write_file" {
            let now = self.now();
            if let Err(refused) = tools::precheck_write(&sc.guard, sc.span.ceiling(), &sc.role, &sc.m.id, &now, args) {
                return Ok(refused);
            }
        }
        if let Need::Yes(tier) = self.broker.need(&sc.task_id, None, &c) {
            if let Some(denied) = self.ask(sc, tier, &c.tool, &c.pattern, &c.summary, &c.payload).await? {
                return Ok(denied);
            }
        }
        let before = sc.span.ceiling();
        let (result, stamps) = {
            let mut idx = sc.m.index.lock().unwrap();
            let now = self.now();
            let mut ctx = ToolCtx { guard: &sc.guard, span: &mut sc.span, index: &mut idx, rules: &self.rules, probe: &*self.probe, role: &sc.role, mission_id: &sc.m.id, now_rfc3339: &now, stamps: vec![] };
            let r = self.tools.run(&mut ctx, name, args);
            (r, std::mem::take(&mut ctx.stamps))
        };
        let mut events: Vec<EventEnvelope> = stamps.iter().map(|s| sc.ev("provenance.stamp", delphi_core::provenance::Index::stamp_payload(s.zone, &s.path, &s.blake3, s.tier))).collect();
        if sc.span.ceiling() != before {
            events.push(sc.ev("span.taint", json!({"ceiling": sc.span.ceiling().as_str(), "cause": name})));
        }
        if !events.is_empty() {
            sc.m.append_batch(events).await?;
        }
        Ok(result)
    }

    /// The registered project a Builder task works on: named by an `@project/<name>/…` pointer, else the
    /// only registered one.
    fn project_for(&self, handoff: &HandoffEnvelope) -> Result<(String, PathBuf), String> {
        let named = handoff.context_pointers.iter().find_map(|p| p.uri.trim_start_matches("./").strip_prefix("@project/").and_then(|r| r.split('/').next()).map(|n| n.to_lowercase()));
        match named {
            Some(n) => self.projects.iter().find(|(name, _)| name.to_lowercase() == n).cloned().ok_or_else(|| format!("`@project/{}` is not a registered project", n)),
            None if self.projects.len() == 1 => Ok(self.projects[0].clone()),
            None => Err("this task needs a project: mention one with an @project/<name>/… pointer".into()),
        }
    }

    async fn attach_worktree(&self, sc: &mut SpanCtx, handoff: &HandoffEnvelope) -> Result<(), String> {
        let (name, path) = self.project_for(handoff)?;
        // The spec gate (`specs/003` §3.1.2): no worktree without a ratified spec, or the operator's
        // approval of the bounded exemption. Frontmatter proves nothing; only the lock does.
        match crate::finalize::spec_link(&self.ws.dir, handoff.implements_spec.as_deref()) {
            crate::gates::SpecLink::Ratified(_) => {}
            crate::gates::SpecLink::NotRatified { spec, why } => return Err(format!("`{}` is not a ratified spec ({}); the operator must ratify it before a build starts", spec, why)),
            crate::gates::SpecLink::None => {
                let payload = json!({"tool": "build_without_spec", "task_id": sc.task_id, "objective": handoff.objective.chars().take(500).collect::<String>(), "limits": "at most 2 files and 100 changed lines, no host-executed files, dependency manifests or governance files"});
                let summary = "build without a ratified spec under the bounded exemption (at most 2 files, 100 lines, nothing sensitive)";
                if self.ask(sc, ApprovalTier::Amber, "build_without_spec", "build_without_spec", summary, &payload).await.map_err(|e| e.to_string())?.is_some() {
                    return Err("the operator did not approve building without a ratified spec".into());
                }
                sc.m.append(sc.ev("exemption.approved", json!({"task_id": sc.task_id}))).await.map_err(|e| e.to_string())?;
            }
        }
        let (exec, task) = (self.exec.clone(), sc.task_id.clone());
        let (wt, created) = tokio::task::spawn_blocking(move || exec.ensure(&task, &path)).await.map_err(|e| e.to_string())?.map_err(|e| format!("could not set up the worktree: {}", e))?;
        sc.guard.zones.worktree = Some(wt.path.clone());
        if created {
            let ev = sc.ev("worktree.created", json!({"project": name, "branch": wt.branch, "base_sha": wt.base_sha, "project_was_dirty": wt.project_was_dirty, "implements_spec": handoff.implements_spec}));
            sc.m.append(ev).await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// `run_command`: gate on the span's taint, run in the sandbox, stamp what changed.
    async fn exec_command(self: &Arc<Self>, sc: &mut SpanCtx, call_id: &str, args: &Value) -> Result<tools::ToolResult, EngineError> {
        if !sc.manifest.execute.iter().any(|e| e == "shell-worktree-scoped") {
            return Ok(tool_result_error("this role's manifest does not authorize shell-worktree-scoped"));
        }
        let Some(wt) = self.exec.known_worktree(&sc.task_id) else {
            return Ok(tool_result_error("this task has no worktree"));
        };
        let argv = match exec::parse_argv(args) {
            Ok(a) => a,
            Err(e) => return Ok(tool_result_error(e.to_string())),
        };
        if let Err(e) = self.exec.can_run() {
            return Ok(tool_result_error(e.to_string()));
        }
        match gate(sc.span.ceiling(), Action::WorktreeWrite) {
            Gate::Allow => {}
            Gate::Deny => return Ok(tool_result_error("code execution is not allowed from a span at this taint level")),
            Gate::Red => {
                let payload = json!({"tool": "run_command", "argv": argv, "ceiling": sc.span.ceiling().as_str()});
                let summary = format!("run `{}` from a span tainted at {}", argv.join(" "), sc.span.ceiling());
                if let Some(denied) = self.ask(sc, ApprovalTier::Red, "run_command", "run_command", &summary, &payload).await? {
                    return Ok(denied);
                }
            }
        }
        // The policy: deny rules, the sandbox, allow rules and the read-only list decide what needs a card.
        match self.exec.verdict_at(&argv, Some(&wt.path)) {
            exec::Verdict::Refused(why) => return Ok(tool_result_error(format!("refused by policy: {}", why))),
            exec::Verdict::Allowed(_) => {}
            exec::Verdict::Ask => {
                // "Allow similar" covers the program, and its subcommand for tools that have them (`cargo test`, `git log`).
                let has_sub = ["cargo", "git", "npm", "pnpm", "yarn", "go", "make", "pip", "uv", "bundle", "gradle", "mvn"].contains(&argv[0].rsplit('/').next().unwrap_or(""));
                let pattern = format!("run_command {}", argv.iter().take(if has_sub && argv.len() > 1 && !argv[1].starts_with('-') { 2 } else { 1 }).cloned().collect::<Vec<_>>().join(" "));
                let c = tools::Classified {
                    tool: "run_command".into(),
                    tier: tools::ToolTier::Amber,
                    kind: tools::ActionKind::UnsandboxedExec,
                    summary: format!("run `{}` in the worktree (not sandboxed)", argv.join(" ")),
                    payload: json!({"tool": "run_command", "argv": argv, "cwd": "@worktree", "sandboxed": false, "timeout_secs": args["timeout_secs"]}),
                    pattern,
                };
                if let Need::Yes(tier) = self.broker.need(&sc.task_id, None, &c) {
                    if let Some(denied) = self.ask(sc, tier, &c.tool, &c.pattern, &c.summary, &c.payload).await? {
                        return Ok(denied);
                    }
                }
            }
        }
        // What a read-only command prints may be untrusted text (a vendored file): it taints like `read_file`.
        if exec::is_safe_read_only(&argv, Some(&wt.path)) {
            let hit = argv[1..].iter().filter(|a| !a.starts_with('-')).any(|a| self.rules.untrusted(a.trim_start_matches("./")) || self.rules.untrusted(&format!("{}/x", a.trim_start_matches("./").trim_end_matches('/'))));
            if hit && sc.span.observe_input(delphi_core::provenance::Tier::L0) {
                sc.m.append(sc.ev("span.taint", json!({"ceiling": sc.span.ceiling().as_str(), "cause": "run_command output"}))).await?;
            }
        }
        // Write-ahead: a crash between these two events leaves a command whose outcome is unknown.
        sc.m.append(sc.ev("action.intent", json!({"action_id": call_id, "tool": "run_command", "argv": argv}))).await?;
        let (exec, wt2, argv2) = (self.exec.clone(), wt.clone(), argv.clone());
        let timeout = args["timeout_secs"].as_u64().filter(|n| *n > 0).map(Duration::from_secs);
        let ran = tokio::task::spawn_blocking(move || exec.run(&wt2, &argv2, timeout)).await.map_err(|e| EngineError::Invalid(e.to_string()))?;
        let report = match ran {
            Ok(r) => r,
            Err(e) => {
                sc.m.append(sc.ev("action.result", json!({"action_id": call_id, "ok": false, "error": e.to_string()}))).await?;
                return Ok(tool_result_error(e.to_string()));
            }
        };
        // Files the command created or changed take this span's tier, exactly as if a file tool had written them.
        let tier = sc.span.output_tier(OutputCheck::Unverified);
        let mut events = Vec::new();
        {
            let mut idx = sc.m.index.lock().unwrap();
            for rel in report.changed.iter().take(500) {
                let Ok(bytes) = std::fs::read(wt.path.join(rel)) else { continue };
                if bytes.len() > 8 << 20 {
                    continue;
                }
                let hash = blake3::hash(&bytes).to_hex().to_string();
                idx.stamp(delphi_core::provenance::Zone::Worktree, rel, &hash, tier);
                events.push(sc.ev("provenance.stamp", delphi_core::provenance::Index::stamp_payload(delphi_core::provenance::Zone::Worktree, rel, &hash, tier)));
            }
        }
        let o = &report.outcome;
        events.push(sc.ev("action.result", json!({"action_id": call_id, "ok": o.success(), "exit_code": o.status.and_then(|s| s.code()), "timed_out": o.timed_out, "output_capped": o.output_capped, "changed_files": report.changed.len(), "over_quota": !o.over_quota.is_empty(), "millis": report.elapsed.as_millis() as u64})));
        sc.m.append_batch(events).await?;
        let (text, is_error) = exec::render(&report, &argv);
        Ok(tools::ToolResult { content: text, is_error })
    }

    /// Raise an approval card and wait for the answer. `None` means approved; `Some(result)` is the denial
    /// the model sees.
    pub(crate) async fn ask(&self, sc: &SpanCtx, tier: ApprovalTier, tool: &str, pattern: &str, summary: &str, payload: &Value) -> Result<Option<tools::ToolResult>, EngineError> {
        let who = Asker { m: &sc.m, task_id: &sc.task_id, span_id: &sc.span_id, role: &sc.role };
        self.ask_as(&who, tier, tool, pattern, summary, payload).await
    }

    /// As [`Engine::ask`] for a decision that belongs to no running span (merge approval after a build).
    pub(crate) async fn ask_as(&self, who: &Asker<'_>, tier: ApprovalTier, tool: &str, pattern: &str, summary: &str, payload: &Value) -> Result<Option<tools::ToolResult>, EngineError> {
        let ev = |kind: &str, payload: Value| {
            let mut e = EventEnvelope::new(&who.m.id, kind, payload);
            e.task_id = Some(who.task_id.to_string());
            e.span_id = Some(who.span_id.to_string());
            e.agent = Some(who.role.to_string());
            e
        };
        let req = Request { approval_id: String::new(), task_id: who.task_id.to_string(), span_id: who.span_id.to_string(), role: who.role.to_string(), tier, tool: tool.into(), pattern: pattern.into(), worktree: None, summary: summary.into(), payload: payload.clone(), payload_blake3: payload_blake3(payload), timeout: self.settings.approval_timeout };
        let waiting = self.broker.request(req);
        let id = waiting.approval_id.clone();
        if let Some(raised) = self.broker.get(&id) {
            who.m.append(ev("approval.requested", raised.event_payload())).await?;
        }
        let resolution = waiting.wait().await;
        let by = match &resolution {
            Resolution::TimedOut => "timeout",
            _ => "console",
        };
        who.m.append(ev("approval.resolved", resolution.event_payload(&id, by))).await?;
        Ok(match resolution {
            Resolution::Approved { .. } => None,
            Resolution::Denied { reason } => Some(tool_result_error(format!("The operator denied this action{}. Do not retry it; propose an alternative or ask.", if reason.is_empty() { String::new() } else { format!(": {}", reason) }))),
            Resolution::TimedOut => Some(tool_result_error("The approval timed out and was denied. Do not retry it; propose an alternative.")),
        })
    }

    /// A dispatched child's report as the parent's tool result.
    fn dispatch_result(role: &str, status: &NodeStatus, text: &str) -> tools::ToolResult {
        let mut text: String = text.chars().take(20_000).collect();
        if let NodeStatus::Escalated(r) = status {
            text = format!("{}\n(the {} did not finish: {})", text, role, r);
        }
        tools::ToolResult { content: text, is_error: matches!(status, NodeStatus::Escalated(_)) }
    }

    async fn exec_dispatch(self: &Arc<Self>, sc: &mut SpanCtx, call_id: &str, args: &Value, rec: &Option<Arc<Recovered>>) -> Result<tools::ToolResult, EngineError> {
        let (result, child) = self.dispatch_child(sc, call_id, args, rec).await?;
        if let Some((ceiling, status)) = child {
            // What the child read is now in the parent's context: the parent inherits the child's taint.
            if sc.span.observe_input(ceiling) {
                sc.m.append(sc.ev("span.taint", json!({"ceiling": sc.span.ceiling().as_str(), "cause": "dispatch result"}))).await?;
            }
            if let NodeStatus::Escalated(r) = status {
                sc.child_escalation = Some(format!("the dispatched `{}` halted: {}", args["role"].as_str().unwrap_or("agent"), r));
            }
        }
        Ok(result)
    }

    /// Run (or recover) the child; returns its report and, if a child ran, its final ceiling and status.
    async fn dispatch_child(self: &Arc<Self>, sc: &mut SpanCtx, call_id: &str, args: &Value, rec: &Option<Arc<Recovered>>) -> Result<(tools::ToolResult, Option<(delphi_core::provenance::Tier, NodeStatus)>), EngineError> {
        // Resuming: this call may already have a child (finished, or still running when the daemon stopped).
        if let Some(rec) = rec {
            if let Some(child) = rec.spans.get(&sc.span_id).and_then(|p| p.dispatches.get(call_id)).and_then(|id| rec.spans.get(id)) {
                return match &child.status {
                    NodeStatus::Running => {
                        let out = self.drive(&sc.m.clone(), &sc.task_id.clone(), Start::Resume { rec: rec.clone(), span_id: child.span_id.clone() }).await?;
                        let st = match &out.status {
                            Status::Done => NodeStatus::Done,
                            Status::Escalated(r) => NodeStatus::Escalated(r.clone()),
                        };
                        Ok((Self::dispatch_result(&child.role, &st, &out.text), Some((out.ceiling, st))))
                    }
                    done => Ok((Self::dispatch_result(&child.role, done, &child.last_text()), Some((child.ceiling, done.clone())))),
                };
            }
        }
        let (Some(role), Some(objective)) = (args["role"].as_str(), args["objective"].as_str()) else {
            return Ok((tool_result_error("dispatch_agent needs `role` and `objective`"), None));
        };
        if !self.settings.enabled_agents.iter().any(|a| a == role) {
            return Ok((tool_result_error(format!("`{}` is not an enabled role (enabled: {})", role, self.settings.enabled_agents.join(", "))), None));
        }
        if role == sc.role {
            return Ok((tool_result_error("a role cannot dispatch to itself"), None));
        }
        if sc.depth + 1 > self.settings.max_dispatch_depth {
            return Ok((tool_result_error(format!("dispatch depth limit ({}) reached", self.settings.max_dispatch_depth)), None));
        }
        // The taint-gated action table: a span at L1 or below needs a Red approval to dispatch (the child
        // would inherit its taint); anything else dispatches freely.
        match gate(sc.span.ceiling(), Action::DispatchAgent) {
            Gate::Allow => {}
            Gate::Deny => return Ok((tool_result_error("dispatch is not allowed from a span at this taint level"), None)),
            Gate::Red => {
                let payload = json!({"tool": "dispatch_agent", "role": role, "objective": objective, "ceiling": sc.span.ceiling().as_str()});
                let summary = format!("dispatch `{}` from a span tainted at {} (the child inherits the taint)", role, sc.span.ceiling());
                if let Some(denied) = self.ask(sc, ApprovalTier::Red, "dispatch_agent", "dispatch_agent", &summary, &payload).await? {
                    return Ok((denied, None));
                }
            }
        }
        let pointers: Vec<ContextPointer> = args["pointers"].as_array().map(|a| a.iter().filter_map(|p| Some(ContextPointer { uri: p["uri"].as_str()?.to_string(), heading: String::new(), blake3: String::new(), reason: p["reason"].as_str()?.to_string() })).collect()).unwrap_or_default();
        let mut child = HandoffEnvelope::root(&sc.m.id, &sc.task_id, role, objective, pointers);
        child.handoff_id = format!("ho_{}_{}", sc.span_id, call_id);
        child.parent_span_id = sc.span_id.clone();
        child.caller_agent = sc.role.clone();
        if let Err(e) = child.validate() {
            return Ok((tool_result_error(format!("invalid handoff: {}", e)), None));
        }
        let out = self.run_span(&sc.m.clone(), &sc.task_id.clone(), child, Some(sc.span.clone()), sc.depth + 1).await?;
        let st = match &out.status {
            Status::Done => NodeStatus::Done,
            Status::Escalated(r) => NodeStatus::Escalated(r.clone()),
        };
        Ok((Self::dispatch_result(role, &st, &out.text), Some((out.ceiling, st))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{events_of, kinds, setup, Fx};
    use crate::mission::EngineError;
    use crate::approval::{Answer, Mode};
    use crate::model::Step;
    use delphi_adapter::blocks::ProviderMeta;
    use delphi_adapter::claude::Usage;
    use delphi_core::hub::ClientQueue;
    use delphi_core::ledger::GroupBy;
    use delphi_core::proto::Scope;
    use std::fs;
    use std::path::Path;

    fn signed_tool_turn(id: &str, name: &str, args: Value) -> Step {
        Step::Reply {
            blocks: vec![
                ContentBlock::Thinking { thought: "let me check".into(), replayable: true, signature_dropped_reason: None, provider_meta: ProviderMeta { provider: Some("anthropic".into()), signature: Some("SIG-EXACT==".into()), data: None } },
                ContentBlock::ToolCall { id: id.into(), name: name.into(), arguments: args },
            ],
            usage: Usage { input_tokens: 1000, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 150, thinking_tokens: 0 },
            stop_reason: "tool_use",
        }
    }

    #[tokio::test]
    async fn a_simple_task_runs_end_to_end_and_everything_is_derived_from_the_log() {
        let f = setup(vec![Step::text("The retry ladder has three attempts.")], 5.0, |_| {});
        let q = ClientQueue::new(256);
        let cid = f.engine.hub.attach(Scope::Narrator, q.clone());
        f.engine.hub.subscribe(cid);
        let out = f.engine.run_task(&f.m, "orchestrator", "Explain the retry ladder").await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "The retry ladder has three attempts."));
        let k = kinds(&f);
        for want in ["task.created", "dag.node.added", "handoff.sent", "dag.node.state", "budget.reserve", "agent.block.complete", "agent.turn.complete", "agent.usage", "task.completed"] {
            assert!(k.contains(&want.to_string()), "missing {} in {:?}", want, k);
        }
        // The request: frontier tier for a deep-reasoning role, thinking on, frozen snapshot in the system channel.
        let req = &f.model.requests()[0];
        assert_eq!((req.endpoint_id.as_str(), req.model_id.as_str()), ("direct", "opus-x"));
        assert_eq!(req.body["thinking"]["budget_tokens"], 8192);
        assert!(req.body["max_tokens"].as_u64().unwrap() > 8192);
        let sys = req.body["system"].to_string();
        assert!(sys.contains("Never exceed your role's scope") && sys.contains("[H-001]") && sys.contains("[W-001]"), "{}", sys);
        assert!(!sys.contains("Explain the retry ladder"), "the operator's prompt is user content, not system");
        assert!(req.body["messages"][0]["content"][0]["text"].as_str().unwrap().contains("Explain the retry ladder"));
        let tool_names: Vec<String> = req.body["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert!(tool_names.contains(&"dispatch_agent".to_string()) && tool_names.contains(&"write_file".to_string()));
        // Budget settled; ledger agrees with the priced usage (1000 in @ $15/M + 200 out @ $75/M).
        {
            let b = f.m.budget.lock().unwrap();
            assert_eq!(b.reserved(), 0);
            assert_eq!(b.spent(), 15_000 + 15_000);
        }
        let totals = f.engine.ledger.lock().unwrap().totals(GroupBy::Role, Some("m1")).unwrap();
        assert_eq!((totals[0].key.as_str(), totals[0].calls, totals[0].input_tokens, totals[0].output_tokens), ("orchestrator", 1, 1000, 200));
        assert!((totals[0].cost_usd - 0.03).abs() < 1e-9, "{}", totals[0].cost_usd);
        // Live deltas reached the narrator; nothing about them was persisted.
        f.engine.hub.flush_frames();
        let mut saw_delta = false;
        while let Some(n) = tokio::time::timeout(Duration::from_millis(50), q.pop()).await.ok().flatten() {
            saw_delta |= n.method == "delta.frame";
        }
        assert!(saw_delta);
        assert!(!k.iter().any(|x| x.contains("delta")));
    }

    /// The default roster config plus one Gemini endpoint that serves the `architect` role.
    fn with_gemini(f: &crate::testkit::Fx) {
        let text = crate::testkit::CONFIG
            .replace("[access]", "[models.gemini]\nfast = \"gem-fast\"\nworkhorse = \"gem-work\"\nfrontier = \"gem-pro\"\n[models.catalog.\"gem-pro\"]\nmax_output_tokens = 64000\nmin_thinking_budget = 128\nmax_thinking_budget = 32000\ninput_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0\n[[endpoints]]\nid = \"gem\"\nfamily = \"gemini\"\npath = \"api\"\ncredential = \"env:K\"\n[access]\nrole_family = { architect = \"gemini\" }")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"direct\", \"backup\", \"gem\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
    }

    #[tokio::test]
    async fn a_role_served_by_gemini_speaks_the_gemini_dialect_end_to_end() {
        let sig = ContentBlock::Thinking { thought: String::new(), replayable: true, signature_dropped_reason: None, provider_meta: ProviderMeta { provider: Some("google".into()), signature: Some("GSIG==".into()), data: None } };
        let call = Step::Reply {
            blocks: vec![sig, ContentBlock::ToolCall { id: "dg-1".into(), name: "read_file".into(), arguments: json!({"path": "@project/app/src/lib.rs"}) }],
            usage: Usage { input_tokens: 1000, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 100, thinking_tokens: 50 },
            stop_reason: "tool_use",
        };
        let f = setup(vec![call, Step::text("It defines `needle`.")], 5.0, |_| {});
        with_gemini(&f);
        let out = f.engine.run_task(&f.m, "architect", "What does @project/app/src/lib.rs define?").await.unwrap();
        assert_eq!(out.status, Status::Done);
        let reqs = f.model.requests();
        assert_eq!((reqs[0].endpoint_id.as_str(), reqs[0].model_id.as_str()), ("gem", "gem-pro"), "a Gemini role never lands on a Claude endpoint");
        assert!(reqs[0].body["contents"].is_array() && reqs[0].body["systemInstruction"].is_object() && reqs[0].body.get("messages").is_none());
        let replay = reqs[1].body["contents"][1]["parts"].as_array().unwrap();
        assert_eq!(replay.iter().find(|p| p.get("functionCall").is_some()).unwrap()["thoughtSignature"], "GSIG==", "same endpoint and model: the signature is replayed on its call");
        let usage = events_of(&f, "agent.usage");
        assert!(usage.iter().all(|e| e.payload["provider"] == "google"));
        assert_eq!(usage[0].payload["thinking_tokens"], 50);
        // 1000 in @ $2/M + (100 out + 50 thinking) @ $12/M
        assert!((usage[0].payload["cost_usd"].as_f64().unwrap() - (0.002 + 0.0018)).abs() < 1e-9, "{}", usage[0].payload["cost_usd"]);
        let turns = events_of(&f, "agent.turn.complete");
        assert_eq!(turns[0].payload["provider"], "google");
    }

    #[tokio::test]
    async fn a_compat_gateway_is_degraded_but_works() {
        let f = setup(vec![signed_tool_turn("call_1", "read_file", json!({"path": "@project/app/src/lib.rs"})), Step::text("It defines `needle`.")], 5.0, |_| {});
        let text = crate::testkit::CONFIG
            .replace("[access]", "[[endpoints]]\nid = \"compat\"\nfamily = \"claude\"\npath = \"gateway-compat\"\nbase_url = \"https://gw.example.internal\"\ncredential = \"env:K\"\n[access]")
            .replace("order = { claude = [\"direct\", \"backup\"] }", "order = { claude = [\"compat\"] }")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"compat\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
        let out = f.engine.run_task(&f.m, "architect", "What does @project/app/src/lib.rs define?").await.unwrap();
        assert_eq!(out.status, Status::Done);
        let reqs = f.model.requests();
        assert_eq!(reqs[0].endpoint_id, "compat");
        let msgs = reqs[1].body["messages"].as_array().unwrap();
        assert_eq!(msgs.iter().map(|m| m["role"].as_str().unwrap()).collect::<Vec<_>>(), ["system", "user", "assistant", "tool", "user"]);
        assert!(!reqs[1].body.to_string().contains("SIG-EXACT"), "no signature crosses a compat gateway");
        assert!(events_of(&f, "agent.usage").iter().all(|e| e.payload["provider"] == "openai-compat"));
    }

    /// The testkit project becomes a git repository so a Builder can get a worktree.
    fn git_project(f: &Fx) {
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
    }

    #[tokio::test]
    async fn a_builder_edits_and_runs_commands_in_a_sandboxed_worktree_and_the_project_stays_clean() {
        let f = setup(
            vec![
                Step::tool("c1", "write_file", json!({"path": "@worktree/src/new.rs", "content": "pub fn b() {}\n"})),
                Step::tool("c2", "run_command", json!({"argv": ["/bin/sh", "-c", "echo compiled > built.txt; cat src/new.rs; echo \"cwd=$(pwd)\""]})),
                Step::text("Added `b` and ran the build."),
            ],
            5.0,
            |_| {},
        );
        git_project(&f);
        crate::testkit::ratify_spec(&f);
        f.engine.broker.set_mode(Mode::AcceptEdits);
        let out = f.engine.run_task(&f.m, "builder", "Implement @specs/011-retry.md: add a function b in @project/app/src/lib.rs").await.unwrap();
        if f.engine.exec.sandbox.is_none() {
            assert!(matches!(out.status, Status::Done | Status::Escalated(_)));
            return;
        }
        assert_eq!(out.status, Status::Done, "{}", out.text);
        let reqs = f.model.requests();
        let tools_offered: Vec<String> = reqs[0].body["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert!(tools_offered.contains(&"run_command".to_string()) && tools_offered.contains(&"write_file".to_string()), "{:?}", tools_offered);
        // The command's output reached the model, and it ran in the worktree, not the project.
        let result = reqs[2].body["messages"].to_string();
        assert!(result.contains("pub fn b()") && result.contains("exit 0") && result.contains("sandboxed, no network"), "{}", result);
        let created = events_of(&f, "worktree.created");
        assert_eq!(created.len(), 1);
        let wt = f.root.join("workspaces/w/worktrees").join(&out.task_id);
        assert_eq!(fs::read_to_string(wt.join("src/new.rs")).unwrap(), "pub fn b() {}\n");
        assert_eq!(fs::read_to_string(wt.join("built.txt")).unwrap(), "compiled\n");
        assert!(!f.root.join("proj/src/new.rs").exists() && !f.root.join("proj/built.txt").exists(), "the project checkout must stay untouched");
        let intents = events_of(&f, "action.intent");
        let results = events_of(&f, "action.result");
        assert_eq!((intents.len(), results.len()), (1, 1));
        assert_eq!((results[0].payload["ok"].as_bool(), results[0].payload["changed_files"].as_u64()), (Some(true), Some(1)));
        let stamped: Vec<String> = events_of(&f, "provenance.stamp").iter().map(|e| e.payload["path"].as_str().unwrap_or("").to_string()).collect();
        assert!(stamped.contains(&"built.txt".to_string()) && stamped.contains(&"src/new.rs".to_string()), "{:?}", stamped);
    }

    fn builder_fixture(steps: Vec<Step>, cfg: impl FnOnce(&mut crate::exec::ExecConfig)) -> Fx {
        let f = crate::testkit::setup_with_exec(steps, 5.0, |_| {}, cfg);
        git_project(&f);
        crate::testkit::ratify_spec(&f);
        f
    }

    const BUILD_PROMPT: &str = "Implement @specs/011-retry.md in @project/app/src/lib.rs";

    #[tokio::test]
    async fn without_the_sandbox_every_command_asks_and_no_permission_mode_waves_it_through() {
        let f = builder_fixture(vec![Step::tool("c1", "run_command", json!({"argv": ["/bin/sh", "-c", "echo hi"]})), Step::text("done")], |c| c.sandbox = crate::exec::SandboxMode::Off);
        f.engine.broker.set_mode(Mode::MissionAuto);
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "builder", BUILD_PROMPT).await }) };
        let card = wait_for_card(&f).await;
        assert_eq!((card.tool.as_str(), card.tier), ("run_command", ApprovalTier::Amber), "even in the most permissive mode");
        assert_eq!(card.payload["sandboxed"], false);
        assert!(card.summary.contains("echo hi") && card.summary.contains("not sandboxed"), "{}", card.summary);
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Allow, "console-1").unwrap();
        assert_eq!(task.await.unwrap().unwrap().status, Status::Done);
        let result = f.model.requests()[1].body["messages"].to_string();
        assert!(result.contains("hi") && result.contains("not sandboxed") && result.contains("exit 0"), "{}", result);
    }

    #[tokio::test]
    async fn a_denied_command_does_not_run_and_allow_similar_covers_the_same_command_shape_for_the_task() {
        let f = builder_fixture(
            vec![
                Step::tool("c1", "run_command", json!({"argv": ["/bin/sh", "-c", "touch denied.txt"]})),
                Step::tool("c2", "run_command", json!({"argv": ["/usr/bin/touch", "a.txt"]})),
                Step::tool("c3", "run_command", json!({"argv": ["/usr/bin/touch", "b.txt"]})),
                Step::text("done"),
            ],
            |c| c.sandbox = crate::exec::SandboxMode::Off,
        );
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "builder", BUILD_PROMPT).await }) };
        let card = wait_for_card(&f).await;
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "no".into() }, "console-1").unwrap();
        let card = wait_for_card(&f).await;
        assert!(card.summary.contains("touch a.txt"));
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::AllowSimilar, "console-1").unwrap();
        assert_eq!(task.await.unwrap().unwrap().status, Status::Done);
        assert_eq!(events_of(&f, "approval.requested").iter().filter(|e| e.payload["tool"] == "run_command").count(), 2, "the third command did not ask");
        let wt = f.root.join("workspaces/w/worktrees").join(events_of(&f, "task.created")[0].task_id.clone().unwrap());
        assert!(!wt.join("denied.txt").exists() && wt.join("a.txt").exists() && wt.join("b.txt").exists());
    }

    #[tokio::test]
    async fn ratified_rules_allow_or_refuse_without_a_card_and_the_read_only_list_runs_free() {
        let f = builder_fixture(
            vec![
                Step::tool("c1", "run_command", json!({"argv": ["/usr/bin/touch", "ok.txt"]})),
                Step::tool("c2", "run_command", json!({"argv": ["/usr/bin/git", "push", "origin"]})),
                Step::tool("c3", "run_command", json!({"argv": ["ls", "-a"]})),
                Step::tool("c4", "run_command", json!({"argv": ["cat", "/etc/hosts"]})),
                Step::text("done"),
            ],
            |c| {
                c.sandbox = crate::exec::SandboxMode::Off;
                c.allow = vec!["/usr/bin/touch*".into()];
                c.deny = vec!["/usr/bin/git push*".into()];
            },
        );
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "builder", BUILD_PROMPT).await }) };
        // Only the out-of-tree `cat` asks.
        let card = wait_for_card(&f).await;
        assert!(card.summary.contains("cat /etc/hosts"), "{}", card.summary);
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "no".into() }, "console-1").unwrap();
        assert_eq!(task.await.unwrap().unwrap().status, Status::Done);
        let all = f.model.requests()[4].body["messages"].to_string();
        assert!(all.contains("refused by policy") && all.contains("deny rule"), "{}", all);
        assert_eq!(events_of(&f, "approval.requested").iter().filter(|e| e.payload["tool"] == "run_command").count(), 1);
    }

    #[tokio::test]
    async fn reading_a_vendored_file_through_a_command_taints_the_span_like_read_file() {
        let f = builder_fixture(vec![Step::tool("c1", "run_command", json!({"argv": ["cat", "vendor/x.js"]})), Step::text("done")], |c| c.sandbox = crate::exec::SandboxMode::Off);
        let out = f.engine.run_task(&f.m, "builder", BUILD_PROMPT).await.unwrap();
        assert_eq!(out.status, Status::Done);
        let taints = events_of(&f, "span.taint");
        assert!(taints.iter().any(|e| e.payload["cause"] == "run_command output" && e.payload["ceiling"] == "L0"), "{:?}", taints.iter().map(|e| e.payload.clone()).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn required_sandbox_refuses_when_none_exists_but_not_otherwise() {
        let f = builder_fixture(vec![Step::tool("c1", "run_command", json!({"argv": ["ls"]})), Step::text("done")], |c| c.sandbox = crate::exec::SandboxMode::Required);
        if f.engine.exec.sandbox.is_some() {
            return; // the refusal path needs a machine without one; exec.rs tests it directly
        }
        f.engine.run_task(&f.m, "builder", BUILD_PROMPT).await.unwrap();
        assert!(f.model.requests()[1].body["messages"].to_string().contains("no OS sandbox"));
    }

    #[tokio::test]
    async fn only_roles_that_may_run_code_are_offered_or_obeyed_on_run_command() {
        let f = setup(vec![Step::tool("c1", "run_command", json!({"argv": ["/bin/echo", "x"]})), Step::text("done")], 5.0, |_| {});
        git_project(&f);
        let out = f.engine.run_task(&f.m, "architect", "Look at @project/app/src/lib.rs").await.unwrap();
        assert_eq!(out.status, Status::Done);
        let reqs = f.model.requests();
        let offered = reqs[0].body["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert!(!offered.contains(&"run_command".to_string()), "{:?}", offered);
        let r = reqs[1].body["messages"].to_string();
        assert!(r.contains("does not authorize shell-worktree-scoped"), "a call to a tool that was never offered is refused: {}", r);
        assert!(events_of(&f, "action.intent").is_empty());
    }

    #[tokio::test]
    async fn a_builder_without_a_git_project_escalates_instead_of_running_anywhere_else() {
        let f = setup(vec![Step::text("unreachable")], 5.0, |_| {});
        crate::testkit::ratify_spec(&f);
        let out = f.engine.run_task(&f.m, "builder", "Implement @specs/011-retry.md in @project/app/src/lib.rs").await.unwrap();
        match out.status {
            Status::Escalated(r) => assert!(r.contains("worktree"), "{}", r),
            other => panic!("{:?}", other),
        }
        assert_eq!(f.model.requests().len(), 0, "no model call is made for a task that cannot be set up");
    }

    #[tokio::test]
    async fn a_tool_loop_replays_signed_reasoning_exactly_and_pairs_results_with_calls() {
        let f = setup(vec![signed_tool_turn("toolu_1", "read_file", json!({"path": "@project/app/src/lib.rs"})), Step::text("It defines `needle`.")], 5.0, |_| {});
        let out = f.engine.run_task(&f.m, "architect", "What does @project/app/src/lib.rs define?").await.unwrap();
        assert_eq!(out.status, Status::Done);
        let reqs = f.model.requests();
        assert_eq!(reqs.len(), 2);
        let second = &reqs[1].body["messages"];
        let asst = second[1]["content"].as_array().unwrap();
        assert_eq!(asst[0], json!({"type": "thinking", "thinking": "let me check", "signature": "SIG-EXACT=="}), "same endpoint and model: echoed byte-exact");
        assert_eq!(second[2]["content"][0]["type"], "tool_result");
        assert_eq!(second[2]["content"][0]["tool_use_id"], "toolu_1");
        assert!(second[2]["content"][0]["content"].as_str().unwrap().contains("pub fn needle()"));
        // The pointer from the prompt was resolved into a slice, not pasted by the model.
        assert!(reqs[0].body["messages"][0]["content"][0]["text"].as_str().unwrap().contains("pub fn needle()"));
        let results = events_of(&f, "tool.result");
        assert_eq!((results.len(), results[0].payload["name"].as_str()), (1, Some("read_file")));
        // The persisted copy keeps the signature (nothing secret in it), and the turn has its completion marker.
        let blocks = events_of(&f, "agent.block.complete");
        assert_eq!(blocks[0].payload["block"]["provider_meta"]["signature"], "SIG-EXACT==");
        assert_eq!(events_of(&f, "agent.turn.complete").len(), 2);
    }

    #[tokio::test]
    async fn a_throttled_endpoint_fails_over_in_the_ratified_order_and_re_reserves() {
        let f = setup(vec![Step::Throttle, Step::text("ok")], 5.0, |_| {});
        let out = f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
        assert_eq!(out.status, Status::Done);
        let seen: Vec<String> = f.model.requests().iter().map(|r| r.endpoint_id.clone()).collect();
        assert_eq!(seen, vec!["direct", "backup"]);
        let k = kinds(&f);
        assert_eq!(k.iter().filter(|x| *x == "budget.reserve").count(), 2);
        assert_eq!(k.iter().filter(|x| *x == "budget.release").count(), 1);
        assert!(k.contains(&"endpoint.throttled".to_string()));
        assert_eq!(f.m.budget.lock().unwrap().reserved(), 0);
        // Both throttled: no endpoint left inside the pin, so the task escalates instead of leaving it.
        let g = setup(vec![Step::Throttle, Step::Throttle], 5.0, |_| {});
        let out = g.engine.run_task(&g.m, "orchestrator", "hi").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("throttled")), "{:?}", out.status);
        assert_eq!(g.m.budget.lock().unwrap().reserved(), 0, "released, not leaked");
    }

    #[tokio::test]
    async fn an_exhausted_model_is_recorded_with_its_reset_and_the_next_endpoint_takes_over() {
        let f = setup(vec![Step::Exhaust, Step::text("ok")], 5.0, |_| {});
        let out = f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
        assert_eq!(out.status, Status::Done);
        let seen: Vec<String> = f.model.requests().iter().map(|r| r.endpoint_id.clone()).collect();
        assert_eq!(seen, vec!["direct", "backup"]);
        let k = kinds(&f);
        assert!(k.contains(&"model.exhausted".to_string()) && !k.contains(&"endpoint.throttled".to_string()), "exhaustion is not reported as a throttle");
        assert_eq!(k.iter().filter(|x| *x == "budget.release").count(), 1, "the first reservation is released, not charged");
        let ex = &events_of(&f, "model.exhausted")[0].payload;
        assert_eq!((ex["endpoint_id"].as_str(), ex["reason"].as_str()), (Some("direct"), Some("quota reached")));
        assert!(ex["reset_at"].as_i64().unwrap() > 0 && ex["raw"].as_str().unwrap().contains("quota reached"));
        assert_eq!(f.m.budget.lock().unwrap().reserved(), 0);
        // Every endpoint out of quota: the task escalates and says which and why.
        let g = setup(vec![Step::Exhaust, Step::Exhaust], 5.0, |_| {});
        let out = g.engine.run_task(&g.m, "orchestrator", "hi").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("direct is out of quota") && r.contains("resets at")), "{:?}", out.status);
        assert_eq!(g.m.budget.lock().unwrap().reserved(), 0, "released, not leaked");
    }

    #[tokio::test]
    async fn an_exhausted_budget_escalates_before_any_call_and_never_downscales() {
        let f = setup(vec![Step::text("should never be asked")], 0.0001, |_| {});
        let out = f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("mission budget") && r.contains("/budget")), "{:?}", out.status);
        assert!(f.model.requests().is_empty(), "no call was made");
        assert_eq!(f.model.remaining(), 1);
        assert!(events_of(&f, "budget.reserve").is_empty());
    }

    // specs/005 §6 test 15
    #[tokio::test]
    async fn repeated_interruptions_of_the_same_turn_escalate_and_each_charges_its_reservation() {
        let f = setup(vec![Step::Interrupt, Step::Interrupt, Step::text("never reached")], 5.0, |_| {});
        let out = f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("interrupted 2 times")), "{:?}", out.status);
        let usages = events_of(&f, "agent.usage");
        assert_eq!(usages.len(), 2);
        for u in &usages {
            assert_eq!((u.payload["outcome"].as_str(), u.payload["usage_source"].as_str()), (Some("cancelled"), Some("estimated")));
            assert_eq!(u.payload["cost_usd"], u.payload["reserved_usd"], "charged in full");
        }
        let b = f.m.budget.lock().unwrap();
        assert_eq!(b.reserved(), 0);
        assert!(b.spent() > 0);
        let total: f64 = usages.iter().map(|u| u.payload["cost_usd"].as_f64().unwrap()).sum();
        assert!((b.spent() as f64 / 1e6 - total).abs() < 1e-6);
        assert_eq!(f.model.remaining(), 1, "no third attempt");
    }

    #[tokio::test]
    async fn one_interruption_is_retried_with_the_same_turn_and_no_partial_blocks_survive() {
        let f = setup(vec![Step::Interrupt, Step::text("recovered")], 5.0, |_| {});
        let out = f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
        assert_eq!((out.status, out.text.as_str()), (Status::Done, "recovered"));
        assert_eq!(events_of(&f, "agent.block.complete").len(), 1, "only the completed turn's block is persisted");
        assert_eq!(events_of(&f, "turn.interrupted").len(), 1);
        let bodies: Vec<String> = f.model.requests().iter().map(|r| r.body["messages"].to_string()).collect();
        assert_eq!(bodies[0], bodies[1], "the retry sends the identical request");
        assert!(!bodies[1].contains("partial"));
    }

    #[tokio::test]
    async fn an_expired_credential_pauses_for_reauthentication_without_failing_over() {
        let f = setup(vec![Step::NeedsReauth, Step::text("must not be used")], 5.0, |_| {});
        let out = f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r == "re-authenticate direct"), "{:?}", out.status);
        assert_eq!(f.model.requests().len(), 1, "the backup endpoint was not tried");
        assert_eq!(f.m.budget.lock().unwrap().reserved(), 0);
        assert_eq!(events_of(&f, "endpoint.reauth_required").len(), 1);
    }

    async fn wait_for_card(f: &Fx) -> Request {
        for _ in 0..200 {
            if let Some(r) = f.engine.broker.pending().into_iter().next() {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no approval card appeared");
    }

    #[tokio::test]
    async fn writing_a_draft_spec_asks_the_operator_and_an_approved_write_lands_with_a_provenance_header() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "# New spec\nbody"})), Step::text("Drafted.")], 5.0, |_| {});
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "architect", "Draft a spec").await }) };
        let card = wait_for_card(&f).await;
        assert_eq!((card.tier, card.tool.as_str(), card.pattern.as_str()), (ApprovalTier::Amber, "write_file", "write_file specs/*"));
        assert!(!f.root.join("workspaces/w/team/specs/009-new.md").exists(), "nothing is written before the answer");
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Allow, "console-1").unwrap();
        let out = task.await.unwrap().unwrap();
        assert_eq!(out.status, Status::Done);
        let results: Vec<String> = events_of(&f, "tool.result").iter().map(|e| e.payload["content"].to_string()).collect();
        let written = fs::read_to_string(f.root.join("workspaces/w/team/specs/009-new.md")).unwrap_or_else(|e| panic!("{} / tool results: {:?}", e, results));
        assert!(written.contains("author: architect") && written.contains("source_type: internal_generated") && written.contains("status: draft") && written.contains("# New spec"), "{}", written);
        let k = kinds(&f);
        assert!(k.contains(&"approval.requested".to_string()) && k.contains(&"approval.resolved".to_string()) && k.contains(&"provenance.stamp".to_string()));
        assert_eq!(events_of(&f, "approval.resolved")[0].payload["resolution"], "approved");
    }

    #[tokio::test]
    async fn a_denied_or_timed_out_write_never_happens_and_the_model_is_told_not_to_retry() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "x"})), Step::text("Understood.")], 5.0, |_| {});
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "architect", "Draft a spec").await }) };
        let card = wait_for_card(&f).await;
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "not now".into() }, "console-1").unwrap();
        assert_eq!(task.await.unwrap().unwrap().status, Status::Done);
        assert!(!f.root.join("workspaces/w/team/specs/009-new.md").exists());
        let second = f.model.requests()[1].body["messages"].to_string();
        assert!(second.contains("The operator denied this action: not now") && second.contains("is_error"), "{}", second);

        let g = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "x"})), Step::text("Understood.")], 5.0, |s| s.approval_timeout = Duration::from_millis(60));
        let out = g.engine.run_task(&g.m, "architect", "Draft a spec").await.unwrap();
        assert_eq!(out.status, Status::Done);
        assert!(!g.root.join("workspaces/w/team/specs/009-new.md").exists(), "no console attached: the timeout denied it");
        assert!(g.model.requests()[1].body["messages"].to_string().contains("timed out"));
        assert_eq!(events_of(&g, "approval.resolved")[0].payload["resolution"], "timed_out");
    }

    #[tokio::test]
    async fn allow_similar_covers_the_next_write_in_the_same_task_only() {
        let f = setup(
            vec![Step::tool("w1", "write_file", json!({"path": "specs/009-a.md", "content": "a"})), Step::tool("w2", "write_file", json!({"path": "specs/009-b.md", "content": "b"})), Step::text("Both drafted.")],
            5.0,
            |_| {},
        );
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "architect", "Draft two specs").await }) };
        let card = wait_for_card(&f).await;
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::AllowSimilar, "console-1").unwrap();
        assert_eq!(task.await.unwrap().unwrap().status, Status::Done);
        assert_eq!(events_of(&f, "approval.requested").len(), 1, "the second write didn't ask");
        assert!(f.root.join("workspaces/w/team/specs/009-a.md").exists() && f.root.join("workspaces/w/team/specs/009-b.md").exists());
        assert!(f.engine.broker.allow_similar().is_empty(), "the grant ended with the task");
        let _ = Mode::Default;
    }

    #[tokio::test]
    async fn role_scopes_are_enforced_by_the_guard_not_by_the_model() {
        let f = setup(
            vec![Step::tool("a", "write_file", json!({"path": "@project/app/src/lib.rs", "content": "pwn"})), Step::tool("b", "write_file", json!({"path": "missions/m1/notes.md", "content": "x"})), Step::tool("c", "read_file", json!({"path": "missions/m1/events/000001.jsonl"})), Step::text("done")],
            5.0,
            |_| {},
        );
        // red-team may write only under councils/ and threat-models/, and can never read the event log.
        let out = f.engine.run_task(&f.m, "red-team", "Audit it").await.unwrap();
        assert_eq!(out.status, Status::Done);
        assert_eq!(fs::read_to_string(f.root.join("proj/src/lib.rs")).unwrap(), "pub fn needle() {}\n");
        let results: Vec<String> = events_of(&f, "tool.result").iter().map(|e| e.payload["content"].as_str().unwrap().to_string()).collect();
        assert!(results[0].contains("denied") && results[1].contains("outside your role") && results[2].contains("never readable"), "{:?}", results);
        assert!(events_of(&f, "approval.requested").is_empty(), "denials don't reach the operator");
    }

    #[tokio::test]
    async fn reading_untrusted_content_taints_the_span_and_what_it_writes() {
        let f = setup(
            vec![Step::tool("a", "read_file", json!({"path": "@project/app/vendor/x.js"})), Step::tool("b", "write_file", json!({"path": "missions/m1/notes.md", "content": "# Notes\nsummary"})), Step::text("done")],
            5.0,
            |_| {},
        );
        let out = f.engine.run_task(&f.m, "orchestrator", "Look at vendor").await.unwrap();
        assert_eq!(out.status, Status::Done);
        let taint = events_of(&f, "span.taint");
        assert_eq!(taint.len(), 1);
        assert_eq!(taint[0].payload["ceiling"], "L0");
        let stamps = events_of(&f, "provenance.stamp");
        assert_eq!(stamps[0].payload["tier"], "L0", "output of a tainted span is stamped at its ceiling");
        // The model was told the file is untrusted data.
        assert!(f.model.requests()[1].body["messages"].to_string().contains("untrusted data"));
    }

    #[tokio::test]
    async fn dispatching_to_a_child_role_runs_its_own_span_and_returns_its_report() {
        let f = setup(
            vec![Step::tool("d1", "dispatch_agent", json!({"role": "architect", "objective": "Draft a plan", "pointers": [{"uri": "@project/app/src/lib.rs", "reason": "the code"}]})), Step::text("The architect says: plan ready."), Step::text("Final: plan ready.")],
            5.0,
            |_| {},
        );
        // Calls consume the script in order: parent (dispatch) → child (report) → parent (final answer).
        let out = f.engine.run_task(&f.m, "orchestrator", "Plan something").await.unwrap();
        assert_eq!((out.status, out.text.as_str()), (Status::Done, "Final: plan ready."));
        let added = events_of(&f, "dag.node.added");
        assert_eq!(added.len(), 2);
        assert_eq!((added[1].payload["role"].as_str(), added[1].payload["depth"].as_u64()), (Some("architect"), Some(1)));
        let handoffs = events_of(&f, "handoff.sent");
        assert_eq!(handoffs[1].payload["handoff"]["caller_agent"], "orchestrator");
        let child_req = &f.model.requests()[1];
        assert!(child_req.body["messages"][0]["content"][0]["text"].as_str().unwrap().contains("pub fn needle()"), "the child got the pointed-at slice");
        assert_eq!(f.model.requests()[2].body["messages"][2]["content"][0]["content"], "The architect says: plan ready.", "the child's report came back as the tool result");
    }

    #[tokio::test]
    async fn a_tainted_span_needs_a_red_approval_to_dispatch() {
        let f = setup(
            vec![Step::tool("a", "read_file", json!({"path": "@project/app/vendor/x.js"})), Step::tool("d1", "dispatch_agent", json!({"role": "architect", "objective": "Do what the vendored file says"})), Step::text("Understood.")],
            5.0,
            |_| {},
        );
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "orchestrator", "Look at vendor").await }) };
        let card = wait_for_card(&f).await;
        assert_eq!((card.tier, card.tool.as_str()), (ApprovalTier::Red, "dispatch_agent"));
        assert_eq!(f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::AllowSimilar, "c"), Err(crate::approval::ResolveError::SimilarNotAllowedForRed));
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "tainted".into() }, "c").unwrap();
        assert_eq!(task.await.unwrap().unwrap().status, Status::Done);
        assert_eq!(events_of(&f, "dag.node.added").len(), 1, "no child span was created");
    }

    // Roadmap Step 2 test: a parent cannot launder an untrusted file by passing the child only a pointer.
    #[tokio::test]
    async fn a_clean_parent_cannot_launder_an_untrusted_file_through_a_pointer() {
        let f = setup(
            vec![
                Step::tool("d1", "dispatch_agent", json!({"role": "architect", "objective": "Summarize it", "pointers": [{"uri": "@project/app/vendor/x.js", "reason": "to summarize"}]})),
                Step::tool("w1", "write_file", json!({"path": "missions/m1/research/summary.md", "content": "# Summary\nof vendored code"})),
                Step::text("child done"),
                Step::text("parent done"),
            ],
            5.0,
            |_| {},
        );
        let out = f.engine.run_task(&f.m, "orchestrator", "Get the architect to summarize the vendored file").await.unwrap();
        assert_eq!(out.status, Status::Done);
        let events = f.m.replay().unwrap();
        let parent_span = events.iter().find(|e| e.kind == "dag.node.added" && e.payload["depth"] == 0).unwrap().payload["node_id"].as_str().unwrap().to_string();
        let child_span = events.iter().find(|e| e.kind == "dag.node.added" && e.payload["depth"] == 1).unwrap().payload["node_id"].as_str().unwrap().to_string();
        // The parent never read the file itself: its only taint is the child's report arriving (see the next test).
        assert!(events.iter().filter(|e| e.kind == "span.taint" && e.span_id.as_deref() == Some(parent_span.as_str())).all(|e| e.payload["cause"] == "dispatch result"));
        // ...but the child's ceiling was clamped by the pointer before it took its first turn.
        let taint_seq = events.iter().find(|e| e.kind == "span.taint" && e.span_id.as_deref() == Some(child_span.as_str())).map(|e| e.seq).expect("the child is tainted");
        let first_call = events.iter().find(|e| e.kind == "budget.reserve" && e.span_id.as_deref() == Some(child_span.as_str())).unwrap().seq;
        assert!(taint_seq < first_call, "tainted before turn 1");
        assert_eq!(events.iter().find(|e| e.kind == "span.taint" && e.span_id.as_deref() == Some(child_span.as_str())).unwrap().payload["ceiling"], "L0");
        // And what the child writes is stamped with that ceiling, not the L3 an unverified clean span would get.
        assert_eq!(events_of(&f, "provenance.stamp")[0].payload["tier"], "L0");
        // The child saw the slice labelled as untrusted.
        assert!(f.model.requests()[1].body["messages"][0]["content"][0]["text"].as_str().unwrap().contains("tier=\"L0\""));
    }

    // Red-team step-2 round 1: the child's taint must flow back into the parent that consumes its report.
    #[tokio::test]
    async fn a_child_that_read_untrusted_input_taints_the_parent_that_uses_its_report() {
        let f = setup(
            vec![
                Step::tool("d1", "dispatch_agent", json!({"role": "architect", "objective": "Summarize it", "pointers": [{"uri": "@project/app/vendor/x.js", "reason": "to summarize"}]})),
                Step::text("child report"),
                Step::tool("w1", "write_file", json!({"path": "missions/m1/research/summary.md", "content": "# Summary"})),
                Step::text("parent done"),
            ],
            5.0,
            |_| {},
        );
        f.engine.run_task(&f.m, "orchestrator", "Summarize the vendored file via the architect").await.unwrap();
        let events = f.m.replay().unwrap();
        let parent_span = events.iter().find(|e| e.kind == "dag.node.added" && e.payload["depth"] == 0).unwrap().payload["node_id"].as_str().unwrap().to_string();
        let taint = events.iter().find(|e| e.kind == "span.taint" && e.span_id.as_deref() == Some(parent_span.as_str())).expect("the parent inherits the child's taint");
        assert_eq!((taint.payload["ceiling"].as_str(), taint.payload["cause"].as_str()), (Some("L0"), Some("dispatch result")));
        let stamp = events_of(&f, "provenance.stamp").into_iter().find(|e| e.span_id.as_deref() == Some(parent_span.as_str())).unwrap();
        assert_eq!(stamp.payload["tier"], "L0", "what the parent writes after reading the child's report is stamped untrusted");
    }

    // Red-team step-2 round 1: a child that halts halts the parent too; the operator decides (specs/003 §3.2).
    #[tokio::test]
    async fn a_dispatched_child_that_halts_halts_the_parent_instead_of_being_retried() {
        let f = setup(
            vec![Step::tool("d1", "dispatch_agent", json!({"role": "architect", "objective": "Draft a plan"})), Step::Fail("provider rejected the request")],
            5.0,
            |_| {},
        );
        let out = f.engine.run_task(&f.m, "orchestrator", "Plan something").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("architect") && r.contains("halted")), "{:?}", out.status);
        assert_eq!(f.model.requests().len(), 2, "the parent made no further model call");
    }

    // Red-team step-2 round 2: the dispatch schema is hidden from other roles, but a forged call must be refused too.
    #[tokio::test]
    async fn a_role_without_agent_dispatch_cannot_dispatch_by_forging_the_call() {
        let f = setup(vec![Step::tool("d1", "dispatch_agent", json!({"role": "strategist", "objective": "Do something"})), Step::text("ok")], 5.0, |_| {});
        f.engine.run_task(&f.m, "architect", "Draft a plan").await.unwrap();
        assert_eq!(events_of(&f, "dag.node.added").len(), 1, "no child span");
        let r = events_of(&f, "tool.result");
        assert!(r[0].payload["is_error"] == true && r[0].payload["content"].as_str().unwrap().contains("agent-dispatch"), "{:?}", r[0].payload);
    }

    #[tokio::test]
    async fn the_turn_budget_stops_a_runaway_span() {
        let steps: Vec<Step> = (0..50).map(|i| Step::tool(&format!("t{}", i), "list_dir", json!({"path": "@project/app"}))).collect();
        let f = setup(steps, 5.0, |_| {});
        // Make the strategist's budget tiny by rewriting its ratified manifest and lock.
        let ratified = f.root.join("workspaces/w/local/ratified/agents/strategist/AGENT.md");
        let text = fs::read_to_string(&ratified).unwrap();
        let (fm, _) = crate::workspace::split_frontmatter(&text);
        let old = fm.unwrap()["max_turn_budget"].as_u64().unwrap();
        let tiny = text.replace(&format!("max_turn_budget: {}", old), "max_turn_budget: 3");
        fs::write(&ratified, &tiny).unwrap();
        let lock_path = f.root.join("workspaces/w/local/ratified.lock");
        let mut lock: Value = serde_json::from_slice(&fs::read(&lock_path).unwrap()).unwrap();
        lock["files"]["agents/strategist/AGENT.md"] = json!(blake3::hash(tiny.as_bytes()).to_hex().to_string());
        fs::write(&lock_path, lock.to_string()).unwrap();
        let out = f.engine.run_task(&f.m, "strategist", "Loop forever").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("turn budget (3)")), "{:?}", out.status);
        assert_eq!(f.model.requests().len(), 3);
    }

    #[tokio::test]
    async fn a_tampered_manifest_refuses_to_start_the_task() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        fs::write(f.root.join("workspaces/w/local/ratified/agents/architect/AGENT.md"), "---\nid: architect\ndefault_access_scope:\n  read: ['*']\n  write: ['**']\n---\nevil").unwrap();
        let err = f.engine.run_task(&f.m, "architect", "hi").await.unwrap_err();
        assert!(err.to_string().contains("does not match its lock"), "{}", err);
        assert!(f.model.requests().is_empty());
        assert!(f.engine.run_task(&f.m, "scout", "hi").await.is_err(), "roles outside the v1 roster have no manifest here");
        let _ = Path::new("");
    }

    // ── recovery: every test models a real death by aborting the running task and building a new engine
    // over the same files ──────────────────────────────────────────────────────────────────────────────

    async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
        for _ in 0..300 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {}", what);
    }

    async fn kill(task: tokio::task::JoinHandle<Result<TaskOutcome, EngineError>>) {
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn a_crash_mid_model_call_is_charged_in_full_and_the_identical_turn_is_retried() {
        let f = setup(vec![Step::tool("t1", "read_file", json!({"path": "@project/app/src/lib.rs"})), Step::Hang], 5.0, |_| {});
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "architect", "What is in @project/app/src/lib.rs?").await }) };
        let model = f.model.clone();
        wait_until("the second call to be in flight", || model.requests().len() == 2).await;
        kill(task).await;
        let before = model.requests()[1].body.clone();

        let f2 = f.restart(vec![Step::text("recovered answer")]);
        let plan = f2.engine.recover_mission(&f2.m).await.unwrap().expect("there is work to recover");
        assert_eq!(plan.items.len(), 1);
        assert!(plan.items[0].action.contains("retries its interrupted turn"), "{}", plan.items[0].action);
        assert!(plan.charged_usd > 0.0);
        // The interrupted call is charged its whole reservation, as an estimate, and the books balance.
        let cancelled: Vec<_> = events_of(&f2, "agent.usage").into_iter().filter(|e| e.payload["outcome"] == "cancelled").collect();
        assert_eq!(cancelled.len(), 1);
        assert_eq!((cancelled[0].payload["usage_source"].as_str(), cancelled[0].payload["cost_usd"].as_f64()), (Some("estimated"), cancelled[0].payload["reserved_usd"].as_f64()));
        assert_eq!(f2.m.budget.lock().unwrap().reserved(), 0);
        assert!(events_of(&f2, "mission.recovered").len() == 1);

        let out = f2.engine.resume_task(&f2.m, &plan.rec, "t_0001").await.unwrap();
        assert_eq!((out.status, out.text.as_str()), (Status::Done, "recovered answer"));
        // The retry sends exactly the request that was in flight, with the earlier tool result intact.
        assert_eq!(f2.model.requests()[0].body["messages"], before["messages"]);
        assert_eq!(f2.model.requests()[0].body["system"], before["system"]);
        assert_eq!(events_of(&f2, "tool.result").len(), 1, "the finished tool call was not run again");
        // Ledger and budget agree once everything is settled.
        let ledger_total = f2.engine.ledger.lock().unwrap().mission_cost_usd("m1").unwrap();
        let spent = f2.m.budget.lock().unwrap().spent() as f64 / 1e6;
        assert!((ledger_total - spent).abs() < 1e-6, "{} vs {}", ledger_total, spent);
        assert_eq!(f2.m.budget.lock().unwrap().reserved(), 0);
        // Finished work is not recovered again.
        assert!(f2.engine.recover_mission(&f2.m).await.unwrap().is_none());
    }

    // specs/005 §6 test 15, across daemon restarts
    #[tokio::test]
    async fn a_turn_that_keeps_killing_the_daemon_escalates_instead_of_looping() {
        let f = setup(vec![Step::Hang], 5.0, |_| {});
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "orchestrator", "hi").await }) };
        let model = f.model.clone();
        wait_until("the first call", || model.requests().len() == 1).await;
        kill(task).await;

        let f2 = f.restart(vec![Step::Hang]);
        let plan = f2.engine.recover_mission(&f2.m).await.unwrap().unwrap();
        assert!(plan.items[0].action.contains("retries"), "{}", plan.items[0].action);
        let task = { let e = f2.engine.clone(); let m = f2.m.clone(); let rec = plan.rec.clone(); tokio::spawn(async move { e.resume_task(&m, &rec, "t_0001").await }) };
        let model = f2.model.clone();
        wait_until("the retried call", || model.requests().len() == 1).await;
        kill(task).await;

        let f3 = f2.restart(vec![Step::text("must never be asked")]);
        let plan = f3.engine.recover_mission(&f3.m).await.unwrap().unwrap();
        assert!(plan.items[0].action.contains("stop and ask you") && plan.items[0].action.contains("2 times"), "{}", plan.items[0].action);
        let out = f3.engine.resume_task(&f3.m, &plan.rec, "t_0001").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("interrupted 2 times")), "{:?}", out.status);
        assert!(f3.model.requests().is_empty(), "no third attempt");
        // Each interrupted call was charged its reservation; nothing is left reserved.
        let cancelled = events_of(&f3, "agent.usage").into_iter().filter(|e| e.payload["outcome"] == "cancelled").count();
        assert_eq!(cancelled, 2);
        assert_eq!(f3.m.budget.lock().unwrap().reserved(), 0);
    }

    #[tokio::test]
    async fn an_unanswered_approval_dies_with_the_daemon_and_the_action_is_asked_about_again() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "# New"}))], 5.0, |_| {});
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "architect", "Draft a spec").await }) };
        let card = wait_for_card(&f).await;
        kill(task).await;

        let f2 = f.restart(vec![Step::text("Drafted.")]);
        let plan = f2.engine.recover_mission(&f2.m).await.unwrap().unwrap();
        assert_eq!(plan.expired_approvals, vec![card.approval_id.clone()]);
        let expired = events_of(&f2, "approval.resolved");
        assert_eq!((expired[0].payload["resolution"].as_str(), expired[0].payload["by"].as_str()), (Some("expired"), Some("recovery")));
        assert!(plan.items[0].action.contains("finishes 1 tool call"), "{}", plan.items[0].action);
        assert!(!f2.root.join("workspaces/w/team/specs/009-new.md").exists(), "nothing was granted, so nothing was written");

        let task = { let e = f2.engine.clone(); let m = f2.m.clone(); let rec = plan.rec.clone(); tokio::spawn(async move { e.resume_task(&m, &rec, "t_0001").await }) };
        let again = wait_for_card(&f2).await;
        assert_ne!(again.approval_id, card.approval_id, "a new card, not the dead one");
        assert!(!f2.root.join("workspaces/w/team/specs/009-new.md").exists(), "still not written before an answer");
        f2.engine.broker.resolve(&again.approval_id, &again.payload_blake3, Answer::Allow, "console-1").unwrap();
        let out = task.await.unwrap().unwrap();
        assert_eq!(out.status, Status::Done);
        assert!(f2.root.join("workspaces/w/team/specs/009-new.md").exists());
        // The read-only ledger of what happened: requested, expired, requested, approved.
        let kinds: Vec<String> = f2.m.replay().unwrap().iter().filter(|e| e.kind.starts_with("approval.")).map(|e| format!("{}:{}", e.kind, e.payload["resolution"].as_str().unwrap_or(""))).collect();
        assert_eq!(kinds, vec!["approval.requested:", "approval.resolved:expired", "approval.requested:", "approval.resolved:approved"]);
    }

    #[tokio::test]
    async fn a_dispatched_child_is_resumed_once_and_its_report_reaches_the_parent() {
        let f = setup(vec![Step::tool("d1", "dispatch_agent", json!({"role": "architect", "objective": "Draft a plan"})), Step::Hang], 5.0, |_| {});
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "orchestrator", "Plan something").await }) };
        let model = f.model.clone();
        wait_until("the child's call", || model.requests().len() == 2).await;
        kill(task).await;

        let f2 = f.restart(vec![Step::text("child report"), Step::text("Parent final.")]);
        let plan = f2.engine.recover_mission(&f2.m).await.unwrap().unwrap();
        assert!(plan.items[0].action.contains("architect retries its interrupted turn") && plan.items[0].action.contains("orchestrator finishes 1 tool call"), "{}", plan.items[0].action);
        let out = f2.engine.resume_task(&f2.m, &plan.rec, "t_0001").await.unwrap();
        assert_eq!((out.status, out.text.as_str()), (Status::Done, "Parent final."));
        assert_eq!(events_of(&f2, "dag.node.added").len(), 2, "the child was resumed, not started a second time");
        let parent_second = &f2.model.requests()[1].body["messages"];
        assert_eq!(parent_second[2]["content"][0]["content"], "child report", "the child's report came back as the dispatch result");
        assert_eq!(f2.m.budget.lock().unwrap().reserved(), 0);
    }

    #[tokio::test]
    async fn status_md_follows_the_mission_through_a_crash_and_its_recovery() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "# New"}))], 5.0, |_| {});
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "architect", "Draft a spec for retries").await }) };
        let card = wait_for_card(&f).await;
        let path = f.root.join("workspaces/w/local/missions/m1/status.md");
        // The card exists a moment before its `approval.requested` event is durable; the projection follows the log.
        wait_until("status.md to show the waiting approval", || fs::read_to_string(&path).map(|t| t.contains("approval `")).unwrap_or(false)).await;
        let waiting = fs::read_to_string(&path).unwrap();
        assert!(waiting.contains("Draft a spec for retries") && waiting.contains("running") && waiting.contains(&format!("approval `{}` [amber]", card.approval_id)), "{}", waiting);
        kill(task).await;

        let f2 = f.restart(vec![Step::text("Drafted.")]);
        let plan = f2.engine.recover_mission(&f2.m).await.unwrap().unwrap();
        let recovered = fs::read_to_string(&path).unwrap();
        assert!(recovered.contains("## Last recovery") && recovered.contains("1 task(s) to resume") && recovered.contains("## Waiting on you\nnothing"), "{}", recovered);
        let task = { let e = f2.engine.clone(); let m = f2.m.clone(); let rec = plan.rec.clone(); tokio::spawn(async move { e.resume_task(&m, &rec, "t_0001").await }) };
        let again = wait_for_card(&f2).await;
        f2.engine.broker.resolve(&again.approval_id, &again.payload_blake3, Answer::Allow, "console-1").unwrap();
        task.await.unwrap().unwrap();
        let done = fs::read_to_string(&path).unwrap();
        assert!(done.contains("**t_0001** → architect: done") && done.contains("(architect): done"), "{}", done);
    }

    #[tokio::test]
    async fn a_task_that_never_started_begins_again_from_its_saved_request() {
        let f = setup(vec![Step::text("unused")], 5.0, |_| {});
        let mut created = EventEnvelope::new("m1", "task.created", json!({"prompt": "Summarize the project", "target": "orchestrator"}));
        created.task_id = Some("t_0001".into());
        f.m.append(created).await.unwrap();
        let f2 = f.restart(vec![Step::text("Here is the summary.")]);
        let plan = f2.engine.recover_mission(&f2.m).await.unwrap().unwrap();
        assert!(plan.items[0].action.contains("start again"), "{}", plan.items[0].action);
        let out = f2.engine.resume_task(&f2.m, &plan.rec, "t_0001").await.unwrap();
        assert_eq!((out.status, out.text.as_str()), (Status::Done, "Here is the summary."));
        assert_eq!(events_of(&f2, "task.created").len(), 1, "no second task.created");
        assert!(f2.model.requests()[0].body["messages"][0]["content"][0]["text"].as_str().unwrap().contains("Summarize the project"));
    }

    #[tokio::test]
    async fn finished_and_escalated_work_is_left_alone() {
        let f = setup(vec![Step::text("done already")], 5.0, |_| {});
        f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
        let f2 = f.restart(vec![Step::text("never")]);
        assert!(f2.engine.recover_mission(&f2.m).await.unwrap().is_none());
        let g = setup(vec![Step::text("x")], 0.0001, |_| {});
        assert!(matches!(g.engine.run_task(&g.m, "orchestrator", "hi").await.unwrap().status, Status::Escalated(_)));
        let g2 = g.restart(vec![Step::text("never")]);
        assert!(g2.engine.recover_mission(&g2.m).await.unwrap().is_none(), "an escalated task is the operator's, not ours to retry");
    }
}
