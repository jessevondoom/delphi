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
use crate::handoff::{pointers_from_prompt, ContextPointer, HandoffEnvelope};
use crate::mission::{EngineError, MissionState, SecretRegistry};
use crate::model::{ModelClient, ModelError};
use crate::probe::ProjectProbe;
use crate::tools::{self, classify, ToolCtx};
use crate::workspace::{AgentManifest, WorkspacePaths};
use delphi_adapter::blocks::{ContentBlock, Turn};
use delphi_adapter::claude::{compile, Compiled, CompileOptions, Completed, StreamEvent, Target, ToolDef};
use delphi_adapter::config::{Config, Endpoint};
use delphi_adapter::ipr::{ContextSlice, Ipr};
use delphi_core::event::EventEnvelope;
use delphi_core::hub::{DeltaKind, Hub};
use delphi_core::ledger::Ledger;
use delphi_core::pathguard::{Guard, Zones};
use delphi_core::provenance::{gate, Action, Gate, OriginRules, Span};
use serde_json::{json, Value};
use std::collections::HashSet;
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
}

#[derive(Debug, Clone)]
pub struct TaskOutcome {
    pub task_id: String,
    pub status: Status,
    pub text: String,
}

pub struct Engine {
    pub ws: WorkspacePaths,
    pub cfg: Arc<Config>,
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
}

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why a span stopped without finishing; becomes `Status::Escalated`.
struct Escalate(String);

/// Everything one span needs while it runs.
struct SpanCtx {
    m: Arc<MissionState>,
    task_id: String,
    span_id: String,
    role: String,
    manifest: AgentManifest,
    guard: Guard,
    span: Span,
    depth: u32,
    tier: String,
    thinking: u32,
}

impl SpanCtx {
    fn ev(&self, kind: &str, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new(&self.m.id, kind, payload);
        e.task_id = Some(self.task_id.clone());
        e.span_id = Some(self.span_id.clone());
        e.agent = Some(self.role.clone());
        e
    }

    fn agent_ev(&self, kind: &str, payload: Value, ep: &Endpoint, model_id: &str) -> EventEnvelope {
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
    fn now(&self) -> String {
        (self.clock)()
    }

    fn guard_for(&self, manifest: &AgentManifest) -> Guard {
        let ws = &self.ws.dir;
        let mut protected = HashSet::new();
        for p in [ws.join("local/ratified.lock"), ws.join("local/state.db"), self.ws.home.join("operator/ratified.lock")] {
            if let Ok(md) = std::fs::metadata(&p) {
                use std::os::unix::fs::MetadataExt;
                protected.insert((md.dev(), md.ino()));
            }
        }
        Guard {
            zones: Zones { local: ws.join("local"), team: ws.join("team"), base: ws.join("base"), ratified: ws.join("local/ratified"), worktree: None, projects: self.projects.clone() },
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
        let handoff = HandoffEnvelope::root(&m.id, &task_id, target, prompt, pointers_from_prompt(prompt));
        handoff.validate().map_err(EngineError::Invalid)?;
        if !self.settings.enabled_agents.iter().any(|a| a == target) {
            return Err(EngineError::Invalid(format!("`{}` is not an enabled role", target)));
        }
        let mut created = EventEnvelope::new(&m.id, "task.created", json!({"prompt_chars": prompt.chars().count(), "target": target}));
        created.task_id = Some(task_id.clone());
        m.append(created).await?;
        let outcome = self.run_span(m, &task_id, handoff, None, 0).await;
        self.broker.end_task(&task_id);
        let outcome = outcome?;
        let mut out = EventEnvelope::new(&m.id, "task.output", json!({"status": match &outcome.status { Status::Done => "done", Status::Escalated(_) => "escalated" }, "text": outcome.text}));
        out.task_id = Some(task_id.clone());
        m.append(out).await?;
        let mut done = EventEnvelope::new(&m.id, "task.completed", json!({"status": match &outcome.status { Status::Done => "done", Status::Escalated(_) => "escalated" }, "reason": match &outcome.status { Status::Escalated(r) => r.as_str(), _ => "" }}));
        done.task_id = Some(task_id.clone());
        m.append(done).await?;
        Ok(TaskOutcome { task_id, status: outcome.status, text: outcome.text })
    }

    fn run_span<'a>(self: &'a Arc<Self>, m: &'a Arc<MissionState>, task_id: &'a str, handoff: HandoffEnvelope, parent: Option<Span>, depth: u32) -> BoxFut<'a, Result<SpanOutcome, EngineError>> {
        Box::pin(async move {
            let role = handoff.target_agent.clone();
            let manifest = self.ws.load_manifest(&role)?;
            let snapshot = self.ws.load_snapshot(&role)?;
            let span_id = m.next_span_id(&role);
            let span = match &parent {
                Some(p) => p.child(&span_id),
                None => Span::root(&span_id),
            };
            let tier = handoff.routing_directive.model_tier.clone().unwrap_or_else(|| manifest.tier().to_string());
            let thinking = handoff.routing_directive.thinking_budget_tokens.unwrap_or_else(|| manifest.thinking_budget());
            let mut sc = SpanCtx { m: m.clone(), task_id: task_id.to_string(), span_id: span_id.clone(), role: role.clone(), guard: self.guard_for(&manifest), manifest, span, depth, tier, thinking };

            let mut open = vec![
                sc.ev("dag.node.added", json!({"node_id": span_id, "role": role, "parent": handoff.parent_span_id, "depth": depth})),
                sc.ev("handoff.sent", json!({"handoff": serde_json::to_value(&handoff).expect("handoff serializes")})),
                sc.ev("dag.node.state", json!({"node_id": span_id, "state": "running"})),
            ];
            for t in &snapshot.trims {
                open.push(sc.ev("snapshot.trim", json!({"note": t})));
            }
            m.append_batch(open).await?;

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
                nonce: delphi_core::server::random_token().map(|t| t[..12].to_string()).unwrap_or_else(|_| "nonce".into()),
            };
            let mut defs = tools::tool_defs(!sc.manifest.write.is_empty());
            if sc.manifest.execute.iter().any(|e| e == "agent-dispatch") {
                defs.push(dispatch_def());
            }

            let mut history: Vec<Turn> = Vec::new();
            let mut turns = 0u32;
            let outcome = loop {
                if turns >= sc.manifest.max_turn_budget {
                    break Err(Escalate(format!("`{}` used its whole turn budget ({}) without finishing", sc.role, sc.manifest.max_turn_budget)));
                }
                let turn_index = history.len() as u32 + 1;
                let (done, ep, model_id) = match self.model_turn(&sc, &ipr, &history, &defs, turn_index).await? {
                    Ok(t) => t,
                    Err(esc) => break Err(esc),
                };
                turns += 1;
                history.push(Turn::assistant(turn_index, PROVIDER, &ep.id, &model_id, done.blocks.clone()));
                let calls: Vec<(String, String, Value)> = done.blocks.iter().filter_map(|b| if let ContentBlock::ToolCall { id, name, arguments } = b { Some((id.clone(), name.clone(), arguments.clone())) } else { None }).collect();
                if done.stop_reason.as_deref() != Some("tool_use") || calls.is_empty() {
                    let mut text = final_text(&done.blocks);
                    if done.stop_reason.as_deref() == Some("max_tokens") {
                        text.push_str("\n[the reply was cut off at the output limit]");
                    }
                    break Ok(text);
                }
                let result_index = history.len() as u32 + 1;
                let mut results = Vec::new();
                let mut events = Vec::new();
                for (id, name, args) in calls {
                    let r = self.exec_tool(&mut sc, &name, &args, &handoff).await?;
                    events.push(sc.ev("tool.result", json!({"turn_index": result_index, "tool_call_id": id, "name": name, "content": r.content, "is_error": r.is_error})));
                    results.push(ContentBlock::ToolResult { tool_call_id: id, content: r.content, is_error: r.is_error });
                }
                m.append_batch(events).await?;
                history.push(Turn::user(result_index, results));
            };

            let (status, text) = match outcome {
                Ok(text) => (Status::Done, text),
                Err(Escalate(reason)) => (Status::Escalated(reason.clone()), format!("[escalated to the operator: {}]", reason)),
            };
            let state = match &status {
                Status::Done => json!({"node_id": sc.span_id, "state": "done"}),
                Status::Escalated(r) => json!({"node_id": sc.span_id, "state": "escalated", "reason": r}),
            };
            m.append(sc.ev("dag.node.state", state)).await?;
            Ok(SpanOutcome { status, text, turns })
        })
    }

    /// One model turn: pick an endpoint from the ratified order, reserve, call, persist. `Err(Escalate)`
    /// means stop and hand to the operator; `Err(EngineError)` (outer) is a fatal storage failure.
    async fn model_turn(&self, sc: &SpanCtx, ipr: &Ipr, history: &[Turn], defs: &[ToolDef], turn_index: u32) -> Result<Result<(Completed, Endpoint, String), Escalate>, EngineError> {
        let endpoints: Vec<Endpoint> = self.cfg.endpoints_for(FAMILY).into_iter().cloned().collect();
        if endpoints.is_empty() {
            return Ok(Err(Escalate("no allowed Claude endpoint is configured (see `delphi doctor`)".into())));
        }
        let m = &sc.m;
        let mut interruptions = 0u32;
        'endpoints: for ep in &endpoints {
            loop {
                let Some(model_id) = self.cfg.resolve_model(ep, &sc.tier) else { continue 'endpoints };
                let Some(entry) = self.cfg.catalog_entry(&model_id) else {
                    return Ok(Err(Escalate(format!("model `{}` has no [models.catalog] entry, so its cost can't be reserved", model_id))));
                };
                if !entry.has_prices() {
                    return Ok(Err(Escalate(format!("model `{}` has no prices, so its cost can't be reserved", model_id))));
                }
                let price = entry.price();
                let thinking = self.cfg.clamp_thinking(&model_id, sc.thinking);
                let max_tokens = self.settings.max_tokens.max(thinking.unwrap_or(0) as u64 + 2048).min(entry.max_output_tokens.max(thinking.unwrap_or(0) as u64 + 1025));
                let opts = CompileOptions { max_tokens, thinking_budget: thinking, temperature: None, tools: defs.to_vec(), betas: ep.betas.clone() };
                let target = Target { provider: PROVIDER.into(), endpoint_id: ep.id.clone(), model_id: model_id.clone() };
                let compiled: Compiled = match compile(ipr, history, &target, &opts) {
                    Ok(c) => c,
                    Err(e) => return Ok(Err(Escalate(format!("could not build the request: {}", e)))),
                };
                let body_bytes = serde_json::to_vec(&compiled.body).map(|b| b.len()).unwrap_or(0) as u64;
                let system_bytes = serde_json::to_vec(&compiled.body["system"]).map(|b| b.len()).unwrap_or(0) as u64;
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
                let mut on_event = move |e: StreamEvent| match e {
                    StreamEvent::TextDelta { text, .. } => hub.push_delta(&span_id, DeltaKind::Text, &text),
                    StreamEvent::ThinkingDelta { text, .. } => hub.push_delta(&span_id, DeltaKind::Thinking, &text),
                    StreamEvent::BlockComplete { .. } => {}
                };
                let result = self.model.call(ep, &model_id, &compiled, &mut on_event).await;
                match result {
                    Ok(done) => {
                        let payload = done.usage.to_payload(Some(&price), amount as f64 / 1e6, Some(rid.clone()), "ok", "reported");
                        let mut batch: Vec<EventEnvelope> = done
                            .blocks
                            .iter()
                            .enumerate()
                            .map(|(i, b)| sc.agent_ev("agent.block.complete", json!({"turn_index": turn_index, "role": "assistant", "block_index": i, "block": serde_json::to_value(b).expect("blocks serialize")}), ep, &model_id))
                            .collect();
                        // The completion marker is last: a turn without it is discarded on recovery.
                        batch.push(sc.agent_ev("agent.turn.complete", json!({"turn_index": turn_index, "stop_reason": done.stop_reason, "blocks": done.blocks.len()}), ep, &model_id));
                        batch.push(sc.agent_ev("agent.usage", payload.to_value(), ep, &model_id));
                        m.append_batch(batch).await?;
                        return Ok(Ok((done, ep.clone(), model_id)));
                    }
                    Err(ModelError::Throttled(msg)) => {
                        m.append(sc.agent_ev("budget.release", json!({"reservation_id": rid, "reason": "throttled"}), ep, &model_id)).await?;
                        m.append(sc.ev("endpoint.throttled", json!({"endpoint_id": ep.id, "message": self.secrets.redact(&msg)}))).await?;
                        continue 'endpoints; // the next allowed endpoint, in ratified order
                    }
                    Err(ModelError::Interrupted(msg)) => {
                        // The usage of an interrupted call is unknowable: charge the full reservation.
                        let mut u = delphi_adapter::claude::Usage::default().to_payload(Some(&price), amount as f64 / 1e6, Some(rid.clone()), "cancelled", "estimated");
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
        Ok(Err(Escalate("every allowed endpoint is throttled or unavailable; waiting would not help right now".into())))
    }

    /// Run one tool call: classify → taint gate → approval if needed → execute → persist stamps.
    async fn exec_tool(self: &Arc<Self>, sc: &mut SpanCtx, name: &str, args: &Value, _handoff: &HandoffEnvelope) -> Result<tools::ToolResult, EngineError> {
        if name == "dispatch_agent" {
            return self.exec_dispatch(sc, args).await;
        }
        let c = match classify(name, args) {
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
            let r = tools::run(&mut ctx, name, args);
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

    /// Raise an approval card and wait for the answer. `None` means approved; `Some(result)` is the denial
    /// the model sees.
    async fn ask(&self, sc: &SpanCtx, tier: ApprovalTier, tool: &str, pattern: &str, summary: &str, payload: &Value) -> Result<Option<tools::ToolResult>, EngineError> {
        let req = Request { approval_id: String::new(), task_id: sc.task_id.clone(), span_id: sc.span_id.clone(), role: sc.role.clone(), tier, tool: tool.into(), pattern: pattern.into(), worktree: None, summary: summary.into(), payload: payload.clone(), payload_blake3: payload_blake3(payload), timeout: self.settings.approval_timeout };
        let waiting = self.broker.request(req);
        let id = waiting.approval_id.clone();
        if let Some(raised) = self.broker.get(&id) {
            sc.m.append(sc.ev("approval.requested", raised.event_payload())).await?;
        }
        let resolution = waiting.wait().await;
        let by = match &resolution {
            Resolution::TimedOut => "timeout",
            _ => "console",
        };
        sc.m.append(sc.ev("approval.resolved", resolution.event_payload(&id, by))).await?;
        Ok(match resolution {
            Resolution::Approved { .. } => None,
            Resolution::Denied { reason } => Some(tool_result_error(format!("The operator denied this action{}. Do not retry it; propose an alternative or ask.", if reason.is_empty() { String::new() } else { format!(": {}", reason) }))),
            Resolution::TimedOut => Some(tool_result_error("The approval timed out and was denied. Do not retry it; propose an alternative.")),
        })
    }

    async fn exec_dispatch(self: &Arc<Self>, sc: &mut SpanCtx, args: &Value) -> Result<tools::ToolResult, EngineError> {
        let (Some(role), Some(objective)) = (args["role"].as_str(), args["objective"].as_str()) else {
            return Ok(tool_result_error("dispatch_agent needs `role` and `objective`"));
        };
        if !self.settings.enabled_agents.iter().any(|a| a == role) {
            return Ok(tool_result_error(format!("`{}` is not an enabled role (enabled: {})", role, self.settings.enabled_agents.join(", "))));
        }
        if role == sc.role {
            return Ok(tool_result_error("a role cannot dispatch to itself"));
        }
        if sc.depth + 1 > self.settings.max_dispatch_depth {
            return Ok(tool_result_error(format!("dispatch depth limit ({}) reached", self.settings.max_dispatch_depth)));
        }
        // The taint-gated action table: a span at L1 or below needs a Red approval to dispatch (the child
        // would inherit its taint); anything else dispatches freely.
        match gate(sc.span.ceiling(), Action::DispatchAgent) {
            Gate::Allow => {}
            Gate::Deny => return Ok(tool_result_error("dispatch is not allowed from a span at this taint level")),
            Gate::Red => {
                let payload = json!({"tool": "dispatch_agent", "role": role, "objective": objective, "ceiling": sc.span.ceiling().as_str()});
                let summary = format!("dispatch `{}` from a span tainted at {} (the child inherits the taint)", role, sc.span.ceiling());
                if let Some(denied) = self.ask(sc, ApprovalTier::Red, "dispatch_agent", "dispatch_agent", &summary, &payload).await? {
                    return Ok(denied);
                }
            }
        }
        let pointers: Vec<ContextPointer> = args["pointers"].as_array().map(|a| a.iter().filter_map(|p| Some(ContextPointer { uri: p["uri"].as_str()?.to_string(), heading: String::new(), blake3: String::new(), reason: p["reason"].as_str()?.to_string() })).collect()).unwrap_or_default();
        let mut child = HandoffEnvelope::root(&sc.m.id, &sc.task_id, role, objective, pointers);
        child.handoff_id = format!("ho_{}_{}", sc.span_id, role);
        child.parent_span_id = sc.span_id.clone();
        child.caller_agent = sc.role.clone();
        if let Err(e) = child.validate() {
            return Ok(tool_result_error(format!("invalid handoff: {}", e)));
        }
        let out = self.run_span(&sc.m.clone(), &sc.task_id.clone(), child, Some(sc.span.clone()), sc.depth + 1).await?;
        let mut text: String = out.text.chars().take(20_000).collect();
        if let Status::Escalated(r) = &out.status {
            text = format!("{}\n(the {} did not finish: {})", text, role, r);
        }
        Ok(tools::ToolResult { content: text, is_error: matches!(out.status, Status::Escalated(_)) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{events_of, kinds, setup, Fx};
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
            usage: Usage { input_tokens: 1000, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 150 },
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
        assert!(f.engine.run_task(&f.m, "builder", "hi").await.is_err(), "roles outside the v1 roster aren't enabled");
        let _ = Path::new("");
    }
}
