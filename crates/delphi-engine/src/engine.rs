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
use crate::pool::Pool;
use crate::route::{resolve_route, Route, RouteRequest};
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

pub const PROVIDER: &str = "anthropic";

#[derive(Debug, Clone)]
pub struct Settings {
    pub approval_timeout: Duration,
    /// `[away] approval_park_max_sec`: how long a card may stay outstanding across re-raises (`specs/014` §3).
    /// A card that times out is still denied and its id is dead, but the task is not failed: a fresh card with a
    /// new id is raised, and nothing is spent while it waits. Zero (the default) means a timeout ends the wait.
    pub approval_park_max: Duration,
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
    /// CLI features from `[harness.claude]` and `[harness.agy]` (`specs/021`).
    pub harness_claude: crate::harness::Opts,
    pub harness_agy: crate::harness::Opts,
    /// How long a Claude span waits, before its first turn, for the account's connectors to finish loading (`specs/020` §6).
    pub connector_settle: Duration,
    /// `council_budget_usd` (D-10): a separate ceiling for one Council.
    pub council_budget_usd: f64,
    /// A Council seat that has not answered by then abstains.
    pub council_seat_timeout: Duration,
    /// While no model is usable, how often a parked task looks again (`specs/002` §4.1).
    pub park_poll: Duration,
    /// The longest a task waits for a model before escalating. `None` waits as long as it takes; `Some(0)` never waits.
    pub park_max: Option<Duration>,
    /// How long a "run on a weaker model?" card waits for the operator. No answer is not a no: the task
    /// looks again (the target may be back) and asks again if not.
    pub degrade_wait: Duration,
    /// `[optimistic] review_wait_sec`: how long a finished optimistic build waits for the operator to review its
    /// provisional decisions before it stops and keeps its branch (`specs/015`).
    pub decision_review_wait: Duration,
    /// `[optimistic] approver`: who decides a provisional card (`specs/015` §3). The Strategist by default.
    pub approver: crate::provisional::ApproverKind,
    /// `[optimistic] second_seat`: a second reviewer for elevated edits. Never required: with one provider there
    /// is none to give.
    pub second_seat: crate::provisional::SecondSeat,
    /// `[optimistic] unseconded`: what happens to an elevated edit no second reviewer looked at.
    pub unseconded: crate::provisional::Unseconded,
    /// `[optimistic] max_decisions`: provisional decisions one task may make. Past it, cards go to the operator.
    pub max_decisions: u32,
    /// `[optimistic] max_approver_calls`: model calls (Strategist and second seat) one task may spend on decisions.
    /// Past it, cards go to the operator.
    pub max_approver_calls: u32,
    /// `[hooks]` (ratified): deny hooks the daemon runs before an approval (`hooks.rs`). Observe hooks are run by
    /// `delphi hooks`, not here.
    pub hooks: crate::hooks::Hooks,
    /// `[relay] step_up = "totp"` with `totp_credential`: the reference to the secret behind the operator's
    /// authenticator (`keychain:…`, `env:…`, `command:[…]`). With it, a relay may approve a Red card when the answer
    /// carries a valid code; without it a relay can only deny Red (`specs/014` §3).
    pub relay_totp_credential: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            approval_timeout: crate::approval::DEFAULT_TIMEOUT,
            approval_park_max: Duration::ZERO,
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
            harness_claude: Default::default(),
            harness_agy: Default::default(),
            connector_settle: Duration::from_secs(10),
            council_budget_usd: 20.0,
            council_seat_timeout: Duration::from_secs(600),
            park_poll: Duration::from_secs(5),
            park_max: None,
            degrade_wait: Duration::from_secs(300),
            decision_review_wait: Duration::from_secs(24 * 3600),
            approver: crate::provisional::ApproverKind::Strategist,
            second_seat: crate::provisional::SecondSeat::CrossFamily,
            unseconded: crate::provisional::Unseconded::Park,
            max_decisions: 200,
            max_approver_calls: 100,
            hooks: Default::default(),
            relay_totp_credential: None,
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
    /// Which `(endpoint, model)` pairs are usable now (`specs/002` §4.1). Outcomes are recorded here; routing reads it from M3.
    pub pool: Arc<Pool>,
    /// Unix seconds. Real time in production; tests may fix it.
    pub clock_unix: Arc<dyn Fn() -> i64 + Send + Sync>,
    /// Per task, the weaker substitutes the operator confirmed or declined (`specs/002` §4.1). A confirmation covers that task only.
    pub(crate) degraded: Mutex<HashMap<String, DegradedTask>>,
    /// Tasks started with `--optimistic` (`specs/015`): their eligible cards are decided provisionally and the
    /// merge waits for the operator's review of every decision.
    pub(crate) optimistic: Mutex<HashSet<String>>,
    /// Woken when the operator reviews a decision, so a task waiting for review looks again at once.
    pub(crate) decisions_changed: tokio::sync::Notify,
    /// Durable timers (`specs/017`): wakes the daemon keeps on behalf of roles and the operator.
    pub timers: Arc<crate::timers::TimerStore>,
    /// What each subscription CLI last reported it could do (`specs/020` §4), shown to roles that dispatch work.
    pub inventory: Arc<crate::inventory::Inventory>,
    /// Handoffs whose spans get no tools at all (an approver answers in text), by handoff id.
    pub(crate) toolless: Mutex<HashSet<String>>,
    /// Model calls spent on provisional decisions, by task (`[optimistic] max_approver_calls`).
    pub(crate) approver_calls: Mutex<HashMap<String, u32>>,
    /// Single-use and rate-limit state for step-up codes.
    pub(crate) totp: delphi_core::totp::Verifier,
}

/// What the operator decided about weaker substitutes for one task, as `(endpoint, model)` pairs.
#[derive(Default)]
pub(crate) struct DegradedTask {
    pub(crate) confirmed: HashSet<(String, String)>,
    pub(crate) declined: HashSet<(String, String)>,
}

/// Why a span cannot take its next turn and when something may change: it parks instead of failing.
pub(crate) struct ParkInfo {
    pub(crate) back_at: Option<i64>,
    pub(crate) why: String,
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

/// The reason when the workspace has no model at all (an empty pool, no harness to fall back to). Distinct from
/// `EXHAUSTED`: waiting would never help, and the consoles show "run delphi setup", not "throttled".
pub(crate) const NO_MODEL: &str = "no_model_configured: no model is configured for this workspace";

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
    /// The `(endpoint, model)` that served the last turn, so a switch is reported once (`model.promoted`).
    pub(crate) last_key: Mutex<Option<(String, String)>>,
    /// Set by `model_turn` when it stops because no usable model exists; the span loop parks on it.
    pub(crate) parked: Mutex<Option<ParkInfo>>,
    /// When this span first had to wait for a model; cleared by the next turn that runs. `park_max` counts from here.
    pub(crate) blocked_since: Mutex<Option<std::time::Instant>>,
    /// Families this span should stay out of if it can (from the handoff), and whether it already said it could not.
    pub(crate) avoid_families: Vec<String>,
    pub(crate) said_homogeneous: Mutex<bool>,
    /// This span is offered no tools, and refuses any it is not offered (`provisional_run.rs`).
    pub(crate) toolless: bool,
    /// A harness was already tried for this span (and failed for a reason that is not a spent quota, so the pool does not know).
    pub(crate) harness_tried: std::sync::atomic::AtomicBool,
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

fn timer_def() -> ToolDef {
    ToolDef {
        name: "timer".into(),
        description: "Ask Delphi to wake you later. You cannot wait, sleep or watch a clock yourself: if you say you will check back and have not set a timer, nothing will happen. `set` takes `note` (why you are waking: you will read it in a new task, with nothing else carried over), and either `in_secs` (seconds from now) or `at` (an RFC 3339 time, which must still be in the future), and optionally `every_secs` (at least 60) to repeat. The result gives the real current time and the exact wake time. `list` shows your pending timers; `cancel` takes the `id` of one you set. A timer wakes you, and only you.".into(),
        input_schema: json!({"type": "object", "required": ["action"], "properties": {
            "action": {"type": "string", "enum": ["set", "list", "cancel"]},
            "note": {"type": "string"}, "in_secs": {"type": "integer", "minimum": 1}, "at": {"type": "string"},
            "every_secs": {"type": "integer", "minimum": 60}, "id": {"type": "string"}}}),
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
        self.start_task(m, task_id, target, prompt, true, None).await
    }

    /// A task started by a timer (`specs/017`): it begins at the trust ceiling of whoever set the timer, so waiting
    /// cannot turn a note written under untrusted input into a clean instruction.
    pub async fn run_task_woken(self: &Arc<Self>, m: &Arc<MissionState>, task_id: String, target: &str, prompt: &str, ceiling: delphi_core::provenance::Tier) -> Result<TaskOutcome, EngineError> {
        self.start_task(m, task_id, target, prompt, true, Some(Span::with_ceiling("timer", ceiling))).await
    }

    async fn start_task(self: &Arc<Self>, m: &Arc<MissionState>, task_id: String, target: &str, prompt: &str, announce: bool, parent: Option<Span>) -> Result<TaskOutcome, EngineError> {
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
        self.reprobe_due(m).await?;
        let outcome = self.run_span(m, &task_id, handoff, parent, 0).await;
        self.broker.end_task(&task_id);
        self.degraded.lock().unwrap().remove(&task_id);
        self.finish_task(m, task_id, outcome?).await
    }

    /// Re-probe exhausted pairs whose next probe is due (`specs/002` §4.1): a capped one-word call for an
    /// API endpoint, the CLI's own probe for a harness. A pair with a known reset time is never probed;
    /// it comes back when the time passes. Runs when a task starts, so an idle daemon spends nothing.
    pub(crate) async fn reprobe_due(&self, m: &Arc<MissionState>) -> Result<(), EngineError> {
        let due = self.pool.due_probes();
        if due.is_empty() {
            return Ok(());
        }
        let cfg = self.cfg.get();
        let now = (self.clock_unix)();
        for (endpoint_id, model_id) in due {
            let Some(ep) = cfg.endpoint(&endpoint_id).filter(|e| cfg.is_allowed(&e.id)) else { continue };
            let alive = if ep.path == delphi_adapter::config::AccessPath::Harness {
                let program = if ep.harness.as_deref() == Some("agy") { &self.settings.agy_program } else { &self.settings.harness_program };
                crate::capabilities::probe_harness(&cfg, ep, program, now).await.auth == "ok"
            } else {
                crate::capabilities::probe_alive(&*self.model, &cfg, ep, &model_id).await.is_ok()
            };
            if alive {
                self.pool.record_success(&endpoint_id, &model_id);
                m.append(EventEnvelope::new(&m.id, "model.recovered", json!({"endpoint_id": endpoint_id, "model_id": model_id, "by": "probe"}))).await?;
            } else {
                self.pool.probe_failed(&endpoint_id, &model_id);
            }
        }
        Ok(())
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
            let mut sc = SpanCtx { m: m.clone(), task_id: task_id.to_string(), span_id: span_id.clone(), role: role.clone(), guard: self.guard_for(&manifest), manifest, span, depth, tier, family, thinking, child_escalation: None, content_ref: None, last_key: Mutex::new(None), parked: Mutex::new(None), blocked_since: Mutex::new(None), avoid_families: handoff.routing_directive.avoid_families.clone(), said_homogeneous: Mutex::new(false), harness_tried: std::sync::atomic::AtomicBool::new(false), toolless: self.toolless.lock().unwrap().contains(&handoff.handoff_id) };

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
            // A role that dispatches work picks a family and tier; what the chosen path can do is its business to know.
            if sc.manifest.execute.iter().any(|e| e == "agent-dispatch") && !sc.toolless {
                slices.push(ContextSlice { label: "@runtime/tool-inventory".into(), tier: "L3".into(), text: crate::inventory::render(&self.cfg.get(), &self.inventory.seats()) });
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
            let defs = self.span_tool_defs(&sc).await?;

            let mut turns = history.iter().filter(|t| t.role == delphi_adapter::blocks::Role::Assistant).count() as u32;
            // A family whose first allowed endpoint is a harness runs the whole span in the subscription CLI
            // (`harness_run.rs`). If that cannot be done without repeating a side effect, the same span is
            // tried by the family's API endpoints; and the other way round when every API endpoint is down.
            let mut harness_tried = false;
            let mut harness_result: Option<Result<String, Escalate>> = None;
            if let Some(hep) = if setup_error.is_none() { self.route_harness(&sc) } else { None } {
                harness_tried = true;
                sc.harness_tried.store(true, std::sync::atomic::Ordering::SeqCst);
                match self.run_harness(&mut sc, &ipr, &hep).await? {
                    Ok(text) => harness_result = Some(Ok(text)),
                    Err(f) if f.retryable && self.has_api_route(&sc) => {
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
                        // A harness cannot resume a span: it starts from the first prompt. So the API -> harness fallback is only
                        // for a span whose first turn failed; after any turn (and its tool calls) it would repeat them.
                        if esc.0.starts_with(EXHAUSTED) && !harness_tried && turns == 0 {
                            if let Some(hep) = self.route_harness(&sc) {
                                let tools = if !sc.manifest.write.is_empty() || sc.manifest.execute.iter().any(|e| e == "agent-dispatch") { " (a harness answers in text only: this role's Delphi tools are not available there)" } else { "" };
                                m.append(sc.ev("harness.fallback", json!({"from": "api", "to": hep.id, "reason": format!("{}{}", esc.0, tools)}))).await?;
                                break match self.run_harness(&mut sc, &ipr, &hep).await? {
                                    Ok(text) => Ok(text),
                                    Err(f) => Err(Escalate(format!("{}; then the {} harness: {}", esc.0, hep.id, f.reason))),
                                };
                            }
                        }
                        // No usable model, but one will return: wait for it, then take the turn again (`specs/002` §4.1).
                        let parked = sc.parked.lock().unwrap().take();
                        if let Some(info) = parked {
                            if self.park(&sc, info).await? {
                                continue;
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

    /// The operator stopped a task (`task.cancel`): close what it left open, then record the end. The in-flight
    /// model call is charged its full reservation (its real usage can't be known), approvals waiting on the task
    /// are closed, and `task.cancelled` ends it so recovery never offers it again.
    pub async fn cancel_task(self: &Arc<Self>, m: &Arc<MissionState>, task_id: &str) -> Result<(), EngineError> {
        let events = m.replay()?;
        let open = m.budget.lock().unwrap().open_reservations();
        let mut batch = Vec::new();
        for (rid, amount) in open {
            let Some(res) = events.iter().find(|e| e.kind == "budget.reserve" && e.payload["reservation_id"] == rid.as_str() && e.task_id.as_deref() == Some(task_id)) else { continue };
            let usage = delphi_core::usage::UsagePayload {
                provider: res.endpoint_id.as_deref().and_then(|id| self.cfg.get().endpoint(id).map(delphi_adapter::provider_for)).unwrap_or(PROVIDER).into(), input_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 0, thinking_tokens: 0,
                cost_usd: amount as f64 / 1e6, reserved_usd: amount as f64 / 1e6, usage_source: "estimated".into(), cost_model: "metered".into(), outcome: "cancelled".into(), reservation_id: Some(rid.clone()),
            };
            let mut u = EventEnvelope::new(&m.id, "agent.usage", usage.to_value());
            (u.task_id, u.span_id, u.agent, u.endpoint_id, u.access_path, u.model_tier, u.model_id, u.attempt, u.mode) =
                (res.task_id.clone(), res.span_id.clone(), res.agent.clone(), res.endpoint_id.clone(), res.access_path.clone(), res.model_tier.clone(), res.model_id.clone(), res.attempt, res.mode.clone());
            batch.push(u);
        }
        // Approvals still waiting for this task will never be acted on.
        let before = recovery::rebuild_with(&events, &|h| self.connectors.read_payload(h));
        for id in before.open_approvals.iter().filter(|id| events.iter().any(|e| e.kind == "approval.requested" && e.payload["approval_id"] == id.as_str() && e.task_id.as_deref() == Some(task_id))) {
            batch.push(EventEnvelope::new(&m.id, "approval.resolved", json!({"approval_id": id, "resolution": "expired", "by": "operator", "reason": "the task was cancelled"})));
        }
        self.broker.end_task(task_id);
        let mut c = EventEnvelope::new(&m.id, "task.cancelled", json!({"reason": "the operator stopped it"}));
        c.task_id = Some(task_id.to_string());
        batch.push(c);
        m.append_batch(batch).await?;
        Ok(())
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
        self.reprobe_due(m).await?;
        let Some(root) = t.root_span.clone() else {
            // It never started a span: run it from the saved request.
            return self.start_task(m, task_id.to_string(), &t.target, &t.prompt, false, None).await;
        };
        let st = rec.spans.get(&root).ok_or_else(|| EngineError::Invalid(format!("no recorded span `{}`", root)))?;
        let outcome = match &st.status {
            NodeStatus::Running => self.drive(m, task_id, Start::Resume { rec: rec.clone(), span_id: root }).await?,
            NodeStatus::Done => SpanOutcome { status: Status::Done, text: st.last_text(), turns: 0, ceiling: st.ceiling },
            NodeStatus::Escalated(r) => SpanOutcome { status: Status::Escalated(r.clone()), text: format!("[escalated to the operator: {}]", r), turns: 0, ceiling: st.ceiling },
        };
        self.broker.end_task(task_id);
        self.degraded.lock().unwrap().remove(task_id);
        self.finish_task(m, task_id.to_string(), outcome).await
    }

    /// One model turn: pick an endpoint from the ratified order, reserve, call, persist. `Err(Escalate)`
    /// means stop and hand to the operator; `Err(EngineError)` (outer) is a fatal storage failure.
    async fn model_turn(&self, sc: &SpanCtx, ipr: &Ipr, history: &[Turn], defs: &[ToolDef], turn_index: u32, prior_interruptions: u32) -> Result<Result<(Completed, Endpoint, String), Escalate>, EngineError> {
        let cfg = self.cfg.get();
        let m = &sc.m;
        let mut interruptions = prior_interruptions;
        if interruptions >= self.settings.max_interruptions {
            return Ok(Err(Escalate(format!("the same turn was interrupted {} times in a row", interruptions))));
        }
        let mut exhausted_note: Option<String> = None;
        // Each pass asks the resolver for the best pair that is usable now. A pair that throttles or runs out
        // of quota is recorded in the pool, so the next pass moves on to the next one, across families
        // (`specs/002` §4.1). The cap only guards against a pool that never settles.
        let mut passes = 0usize;
        'endpoints: loop {
            passes += 1;
            if passes > cfg.pool_entries().len() + 2 {
                return Ok(Err(Escalate(format!("{}: no candidate settled after {} tries this turn", EXHAUSTED, passes - 1))));
            }
            let now = (self.clock_unix)();
            let route = resolve_route(&cfg, &self.pool.snapshot(), now, &self.route_request(sc, true));
            let note = exhausted_note.as_ref().map(|n| format!(": {}", n)).unwrap_or_default();
            let (key, passed_over) = match &route {
                Route::Target { key, .. } => (key.clone(), None),
                Route::Similar { key, target, why, .. } => (key.clone(), Some((target.clone(), why.clone()))),
                // The API has nothing as capable as the target, but a subscription harness does: hand the span to it
                // (the span loop does that on this signal) before asking about a weaker API model or waiting. Only for a span's first turn:
                // a harness cannot resume, so later turns ask or wait instead.
                Route::Weaker { .. } | Route::NoModel { .. } if turn_index == 1 && !sc.harness_tried.load(std::sync::atomic::Ordering::SeqCst) && self.route_harness(sc).is_some() => {
                    return Ok(Err(Escalate(format!("{}{}: a subscription harness can serve this span instead", EXHAUSTED, note))));
                }
                Route::Weaker { key, target, why, back_at, lost, .. } => {
                    // Only less capable models are usable. Never silently: the operator confirms for this task, or the task waits.
                    let (from, to) = (format!("{}/{}", target.endpoint_id, target.model_id), format!("{}/{}", key.endpoint_id, key.model_id));
                    // What the operator answered covers this target being replaced by this substitute, with its particular losses:
                    // consent to run sonnet instead of opus is not consent to run sonnet instead of a 1M-token model.
                    let pair = (from.clone(), to.clone());
                    let (confirmed, declined) = self.degraded_decisions(m, &sc.task_id, &pair);
                    let mut ok = confirmed;
                    if !confirmed && !declined {
                        let summary = self.secrets.redact(&format!("{} is unavailable ({}). Run this task on {} instead? It has {}.", from, why, to, lost.join(" and ")));
                        let detail = json!({"target": {"endpoint_id": target.endpoint_id, "model_id": target.model_id}, "substitute": {"endpoint_id": key.endpoint_id, "model_id": key.model_id}, "unavailable": self.secrets.redact(why), "lost": lost});
                        m.append(sc.ev("model.degraded_confirm", json!({"state": "asked", "detail": detail}))).await?;
                        let who = Asker { m: &sc.m, task_id: &sc.task_id, span_id: &sc.span_id, role: &sc.role };
                        let answer = self.ask_resolution(&who, ApprovalTier::Amber, "model", &format!("{}->{}", target.model_id, key.model_id), &summary, &detail, Some(self.settings.degrade_wait)).await?;
                        // Silence is not a no: after the wait the task looks again (the target may be back) and asks again if not.
                        let state = match &answer {
                            Resolution::Approved { .. } => "confirmed",
                            Resolution::Denied { .. } => "declined",
                            Resolution::TimedOut => "timed_out",
                        };
                        ok = answer.is_approved();
                        if state != "timed_out" {
                            let mut d = self.degraded.lock().unwrap();
                            let t = d.entry(sc.task_id.clone()).or_default();
                            if ok { t.confirmed.insert(pair.clone()); } else { t.declined.insert(pair.clone()); }
                        }
                        m.append(sc.ev("model.degraded_confirm", json!({"state": state, "target": {"endpoint_id": target.endpoint_id, "model_id": target.model_id}, "substitute": {"endpoint_id": key.endpoint_id, "model_id": key.model_id}}))).await?;
                        if state == "timed_out" {
                            *sc.parked.lock().unwrap() = Some(ParkInfo { back_at: Some(now), why: format!("no answer about running on {} instead of {}; looking again", to, from) });
                            return Ok(Err(Escalate(format!("{}{}: nobody answered whether to run on the weaker {} ({} is unavailable: {})", EXHAUSTED, note, to, from, why))));
                        }
                    }
                    if !ok {
                        *sc.parked.lock().unwrap() = Some(ParkInfo { back_at: *back_at, why: format!("waiting for {} ({}); the weaker {} was declined", from, why, to) });
                        return Ok(Err(Escalate(format!("{}{}: the operator declined the weaker model {} (it has {}); {} is unavailable ({})", EXHAUSTED, note, to, lost.join(" and "), from, why))));
                    }
                    (key.clone(), Some((target.clone(), format!("{}; confirmed by the operator, with {}", why, lost.join(" and ")))))
                }
                Route::NoModel { .. } if cfg.pool_entries().is_empty() => return Ok(Err(Escalate(NO_MODEL.to_string()))),
                Route::NoModel { back_at, .. } => {
                    *sc.parked.lock().unwrap() = Some(ParkInfo { back_at: *back_at, why: route.explain(now) });
                    return Ok(Err(Escalate(format!("{}{}: {}; waiting would not help right now", EXHAUSTED, note, route.explain(now)))));
                }
            };
            let Some(ep) = cfg.endpoint(&key.endpoint_id) else { continue 'endpoints };
            let model_id = key.model_id.clone();
            if route.homogeneous() && !std::mem::replace(&mut *sc.said_homogeneous.lock().unwrap(), true) {
                m.append(sc.ev("model.homogeneous", json!({"avoided": sc.avoid_families, "used": {"endpoint_id": key.endpoint_id, "model_id": key.model_id, "family": key.family}}))).await?;
            }
            let this_key = (key.endpoint_id.clone(), key.model_id.clone());
            loop {
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
                        if self.pool.record_success(&ep.id, &model_id) {
                            m.append(sc.ev("model.recovered", json!({"endpoint_id": ep.id, "model_id": model_id}))).await?;
                        }
                        // Said once the substitute has actually served a turn, so a candidate that fails at once is not reported as promoted.
                        if let Some((target, why)) = &passed_over {
                            if sc.last_key.lock().unwrap().as_ref() != Some(&this_key) {
                                m.append(sc.ev("model.promoted", json!({"from": {"endpoint_id": target.endpoint_id, "model_id": target.model_id}, "to": {"endpoint_id": key.endpoint_id, "model_id": key.model_id}, "reason": self.secrets.redact(why)}))).await?;
                            }
                        }
                        *sc.last_key.lock().unwrap() = Some(this_key);
                        *sc.blocked_since.lock().unwrap() = None;
                        return Ok(Ok((done, ep.clone(), model_id)));
                    }
                    Err(ModelError::Throttled(msg)) => {
                        m.append(sc.agent_ev("budget.release", json!({"reservation_id": rid, "reason": "throttled"}), ep, &model_id)).await?;
                        m.append(sc.ev("endpoint.throttled", json!({"endpoint_id": ep.id, "message": self.secrets.redact(&msg)}))).await?;
                        // Rule 3 (`specs/002` §4.1): throttled over and over for ten minutes is a spent quota, not a rate limit.
                        if let Some(x) = self.pool.record_throttle(&ep.id, &model_id, None) {
                            m.append(sc.ev("model.exhausted", json!({"endpoint_id": ep.id, "model_id": model_id, "reason": x.reason, "reset_at": x.reset_at, "raw": ""}))).await?;
                        }
                        continue 'endpoints; // the next allowed endpoint, in ratified order
                    }
                    Err(ModelError::Exhausted(x)) => {
                        // Out of quota, not slow: no retry here (`specs/002` §4.1). Say why and when it returns, then try the next endpoint.
                        m.append(sc.agent_ev("budget.release", json!({"reservation_id": rid, "reason": "quota exhausted"}), ep, &model_id)).await?;
                        m.append(sc.ev("model.exhausted", json!({"endpoint_id": ep.id, "model_id": model_id, "reason": x.reason, "reset_at": x.reset_at, "raw": self.secrets.redact(&x.raw)}))).await?;
                        self.pool.record_exhausted(&ep.id, &model_id, &x);
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
    }

    /// What the operator already decided about this target being replaced by this substitute, for this task: `(confirmed, declined)`. The
    /// decisions live in the mission's log (`model.degraded_confirm`), so after a daemon restart in the same
    /// session (the same mission) a resumed task is not asked again. A new mission starts with no decisions.
    fn degraded_decisions(&self, m: &MissionState, task_id: &str, pair: &(String, String)) -> (bool, bool) {
        let mut all = self.degraded.lock().unwrap();
        if !all.contains_key(task_id) {
            let mut restored = DegradedTask::default();
            for e in m.replay().unwrap_or_default() {
                if e.kind != "model.degraded_confirm" || e.task_id.as_deref() != Some(task_id) {
                    continue;
                }
                let (sub, tgt) = (&e.payload["substitute"], &e.payload["target"]);
                // An answer recorded without its target (an older log) cannot be matched to a decision: ask again.
                let (Some(ep), Some(model), Some(tep), Some(tmodel)) = (sub["endpoint_id"].as_str(), sub["model_id"].as_str(), tgt["endpoint_id"].as_str(), tgt["model_id"].as_str()) else { continue };
                let key = (format!("{}/{}", tep, tmodel), format!("{}/{}", ep, model));
                match e.payload["state"].as_str() {
                    Some("confirmed") => {
                        restored.declined.remove(&key);
                        restored.confirmed.insert(key);
                    }
                    Some("declined") => {
                        restored.confirmed.remove(&key);
                        restored.declined.insert(key);
                    }
                    _ => {}
                }
            }
            all.insert(task_id.to_string(), restored);
        }
        all.get(task_id).map(|d| (d.confirmed.contains(pair), d.declined.contains(pair))).unwrap_or((false, false))
    }

    /// Wait, durably visible, until a model can serve this span again (`specs/002` §4.1 `blocked: no_model`).
    /// Returns true when it can take its turn, false when waiting is disabled, there is nothing to wait for, or
    /// `park_max` ran out (the caller escalates). A daemon restart in the middle is ordinary recovery.
    async fn park(&self, sc: &SpanCtx, info: ParkInfo) -> Result<bool, EngineError> {
        let Some(back_at) = info.back_at else { return Ok(false) };
        if self.settings.park_max == Some(Duration::ZERO) {
            return Ok(false);
        }
        sc.m.append(sc.ev("task.blocked", json!({"reason": "no_model", "back_at": back_at, "why": self.secrets.redact(&info.why)}))).await?;
        let started = *sc.blocked_since.lock().unwrap().get_or_insert_with(std::time::Instant::now);
        loop {
            tokio::time::sleep(self.settings.park_poll).await;
            if self.settings.park_max.is_some_and(|max| started.elapsed() >= max) {
                sc.m.append(sc.ev("task.unblocked", json!({"resumed": false, "reason": "waited as long as allowed"}))).await?;
                return Ok(false);
            }
            self.reprobe_due(&sc.m).await?;
            if self.ready_for_turn(sc) {
                sc.m.append(sc.ev("task.unblocked", json!({"resumed": true, "waited_secs": started.elapsed().as_secs()}))).await?;
                return Ok(true);
            }
        }
    }

    /// Would the next turn find a model it can use (or one the operator has not yet been asked about)?
    fn ready_for_turn(&self, sc: &SpanCtx) -> bool {
        let cfg = self.cfg.get();
        match resolve_route(&cfg, &self.pool.snapshot(), (self.clock_unix)(), &self.route_request(sc, true)) {
            Route::Target { .. } | Route::Similar { .. } => true,
            Route::Weaker { key, target, .. } => !self.degraded.lock().unwrap().get(&sc.task_id).is_some_and(|d| d.declined.contains(&(format!("{}/{}", target.endpoint_id, target.model_id), format!("{}/{}", key.endpoint_id, key.model_id)))),
            Route::NoModel { .. } => false,
        }
    }

    /// Does `family` have a pair that could serve right now (allowed, able to run the role, not in cooldown
    /// or out of quota)? Used to plan Councils and the retry ladder from what is usable, not what is configured.
    pub(crate) fn family_usable(&self, family: &str, mutating: bool) -> bool {
        let cfg = self.cfg.get();
        let now = (self.clock_unix)();
        let state = self.pool.snapshot();
        cfg.pool_entries().into_iter().filter(|k| k.family == family).any(|k| {
            let Some(ep) = cfg.endpoint(&k.endpoint_id) else { return false };
            if mutating && !ep.can_mutate() {
                return false;
            }
            state.iter().find(|e| e.endpoint_id == k.endpoint_id && e.model_id == k.model_id).is_none_or(|e| match (e.state, e.until) {
                (crate::pool::State::Available, _) => true,
                (crate::pool::State::CoolingDown | crate::pool::State::Exhausted, Some(t)) => t <= now,
                (crate::pool::State::CoolingDown, None) => true,
                (crate::pool::State::Exhausted, None) => false,
            })
        })
    }

    /// What this span asks of a model (`route.rs`). The usual family and the role's target are preferences;
    /// a span that can run shell commands (a worktree and the shell tool, as for a Builder) needs an endpoint
    /// with an approval bridge.
    fn route_request(&self, sc: &SpanCtx, turn_by_turn: bool) -> RouteRequest {
        RouteRequest {
            role: sc.role.clone(),
            tier: sc.tier.clone(),
            prefer_family: Some(sc.family.clone()),
            avoid_families: sc.avoid_families.clone(),
            mutating: sc.guard.zones.worktree.is_some() && sc.manifest.execute.iter().any(|e| e == "shell-worktree-scoped"),
            min_context_tokens: 0,
            target: self.cfg.get().routing.targets.get(&sc.role).cloned(),
            turn_by_turn,
        }
    }

    /// The harness endpoint the resolver would use for this span now, if its pick is one (and needs no confirmation).
    pub(crate) fn route_harness(&self, sc: &SpanCtx) -> Option<Endpoint> {
        let cfg = self.cfg.get();
        let route = resolve_route(&cfg, &self.pool.snapshot(), (self.clock_unix)(), &self.route_request(sc, false));
        if route.needs_confirmation() {
            return None;
        }
        route.key().and_then(|k| cfg.endpoint(&k.endpoint_id)).filter(|e| e.path == delphi_adapter::config::AccessPath::Harness).cloned()
    }

    /// Is there an API pair that can serve this span turn by turn right now?
    pub(crate) fn has_api_route(&self, sc: &SpanCtx) -> bool {
        let cfg = self.cfg.get();
        // A weaker model counts: `model_turn` asks the operator before using it, so the fallback is not a dead end.
        matches!(resolve_route(&cfg, &self.pool.snapshot(), (self.clock_unix)(), &self.route_request(sc, true)), Route::Target { .. } | Route::Similar { .. } | Route::Weaker { .. })
    }

    /// The tools this span is offered: the built-ins its scope allows, then the ones its manifest's `execute` list
    /// adds, then connector tools. An API span sends them with every turn; a harness span gets them over the bridge
    /// (`bridge.rs`), so both seats have the same Delphi tools for the same role.
    pub(crate) async fn span_tool_defs(&self, sc: &SpanCtx) -> Result<Vec<ToolDef>, EngineError> {
        let mut defs = self.tools.defs(!sc.manifest.write.is_empty());
        if sc.manifest.execute.iter().any(|e| e == "agent-dispatch") {
            defs.push(dispatch_def());
        }
        if sc.manifest.execute.iter().any(|e| e == "timers") {
            defs.push(timer_def());
        }
        if sc.guard.zones.worktree.is_some() && sc.manifest.execute.iter().any(|e| e == "shell-worktree-scoped") {
            defs.push(run_command_def());
        }
        defs.extend(self.connector_defs(sc).await?);
        if sc.toolless {
            defs.clear();
        }
        Ok(defs)
    }

    /// Run one tool call: classify → taint gate → approval if needed → execute → persist stamps.
    pub(crate) async fn exec_tool(self: &Arc<Self>, sc: &mut SpanCtx, call_id: &str, name: &str, args: &Value, rec: &Option<Arc<Recovered>>) -> Result<tools::ToolResult, EngineError> {
        if sc.toolless {
            return Ok(tool_result_error("this span answers in text only; it has no tools"));
        }
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
        if name == "timer" {
            // As with dispatch: the schema is only offered to roles that may set timers; a forged call is refused here.
            if !sc.manifest.execute.iter().any(|e| e == "timers") {
                return Ok(tool_result_error("this role's manifest does not authorize timers"));
            }
            return self.exec_timer(sc, args).await;
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
        } else if crate::artifacts::is_artifact_tool(name) {
            if let Err(refused) = crate::artifacts::precheck(&sc.guard, sc.span.ceiling(), &sc.m.id, name, args) {
                return Ok(refused);
            }
        }
        if let Need::Yes(tier) = self.broker.need(&sc.task_id, None, &c) {
            if let Some(denied) = self.ask_classified(sc, tier, &c).await? {
                return Ok(denied);
            }
        } else if c.tier == tools::ToolTier::Amber && !self.settings.hooks.deny.is_empty() {
            // Allowed without a card (a permission mode, "allow similar"): a deny hook can still say no.
            let who = Asker { m: &sc.m, task_id: &sc.task_id, span_id: &sc.span_id, role: &sc.role };
            if let Some((hook, why)) = self.hook_denial(&who, &c.tool, &c.pattern, &c.summary, "amber", &c.payload).await? {
                return Ok(tool_result_error(format!("This action was blocked by the hook `{}`: {}. Do not retry it; propose an alternative or ask.", hook, why)));
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

    /// As [`Engine::ask`] for a classified tool call. In an optimistic task an eligible call is decided provisionally
    /// (`provisional.rs`) instead of raising a card; anything else, and every other task, asks as before.
    pub(crate) async fn ask_classified(self: &Arc<Self>, sc: &SpanCtx, tier: ApprovalTier, c: &tools::Classified) -> Result<Option<tools::ToolResult>, EngineError> {
        if self.optimistic.lock().unwrap().contains(sc.task_id.as_str()) && crate::provisional::eligible(c, tier) {
            if !self.settings.hooks.deny.is_empty() {
                let who = Asker { m: &sc.m, task_id: &sc.task_id, span_id: &sc.span_id, role: &sc.role };
                if let Some((hook, why)) = self.hook_denial(&who, &c.tool, &c.pattern, &c.summary, tier.as_str(), &c.payload).await? {
                    return Ok(Some(tool_result_error(format!("This action was blocked by the hook `{}`: {}. Do not retry it; propose an alternative or ask.", hook, why))));
                }
            }
            let id = crate::provisional::new_decision_id();
            if let crate::provisional_run::Provisional::Decided(verdict) = self.decide_provisionally(sc, c, &id).await? {
                sc.m.append(sc.ev("decision.provisional", crate::provisional::decision_event(&id, c, tier, &verdict, &sc.role))).await?;
                return Ok(if verdict.allow {
                    None
                } else {
                    Some(tool_result_error(format!("This action was refused ({}). Do not retry it; propose an alternative or ask.", verdict.rationale)))
                });
            }
            // Not decided (no usable answer, or the reviewers disagreed): the operator is asked, as without the mode.
        }
        self.ask(sc, tier, &c.tool, &c.pattern, &c.summary, &c.payload).await
    }

    /// Mark a task optimistic. Refused unless the OS sandbox is on: everything an optimistic task does happens
    /// before the operator's review, and a worktree is not a sandbox (GOVERNANCE).
    pub fn mark_optimistic(&self, task_id: &str) -> Result<(), String> {
        if self.exec.sandbox.is_none() {
            return Err("optimistic mode needs the OS sandbox, and none is available on this machine (everything it does runs before you review it)".into());
        }
        self.optimistic.lock().unwrap().insert(task_id.to_string());
        Ok(())
    }

    /// Raise an approval card and wait for the answer. `None` means approved; `Some(result)` is the denial
    /// the model sees.
    pub(crate) async fn ask(&self, sc: &SpanCtx, tier: ApprovalTier, tool: &str, pattern: &str, summary: &str, payload: &Value) -> Result<Option<tools::ToolResult>, EngineError> {
        let who = Asker { m: &sc.m, task_id: &sc.task_id, span_id: &sc.span_id, role: &sc.role };
        self.ask_as(&who, tier, tool, pattern, summary, payload).await
    }

    /// As [`Engine::ask`] for a decision that belongs to no running span (merge approval after a build).
    pub(crate) async fn ask_as(&self, who: &Asker<'_>, tier: ApprovalTier, tool: &str, pattern: &str, summary: &str, payload: &Value) -> Result<Option<tools::ToolResult>, EngineError> {
        Ok(match self.ask_resolution(who, tier, tool, pattern, summary, payload, None).await? {
            Resolution::Approved { .. } => None,
            Resolution::Denied { reason } if reason.starts_with("blocked by the hook") => Some(tool_result_error(format!("This action was {}. Do not retry it; propose an alternative or ask.", reason))),
            Resolution::Denied { reason } => Some(tool_result_error(format!("The operator denied this action{}. Do not retry it; propose an alternative or ask.", if reason.is_empty() { String::new() } else { format!(": {}", reason) }))),
            Resolution::TimedOut => Some(tool_result_error("The approval timed out and was denied. Do not retry it; propose an alternative.")),
        })
    }

    /// Raise a card and return how it ended. `timeout` overrides the usual approval timeout for this card.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn ask_resolution(&self, who: &Asker<'_>, tier: ApprovalTier, tool: &str, pattern: &str, summary: &str, payload: &Value, timeout: Option<Duration>) -> Result<Resolution, EngineError> {
        let ev = |kind: &str, payload: Value| {
            let mut e = EventEnvelope::new(&who.m.id, kind, payload);
            e.task_id = Some(who.task_id.to_string());
            e.span_id = Some(who.span_id.to_string());
            e.agent = Some(who.role.to_string());
            e
        };
        if !self.settings.hooks.deny.is_empty() {
            if let Some((hook, why)) = self.hook_denial(who, tool, pattern, summary, tier.as_str(), payload).await? {
                return Ok(Resolution::Denied { reason: format!("blocked by the hook `{}`: {}", hook, why) });
            }
        }
        let base_timeout = timeout.unwrap_or(self.settings.approval_timeout);
        let started = std::time::Instant::now();
        let mut reraised_from: Option<String> = None;
        loop {
            let req = Request { approval_id: String::new(), task_id: who.task_id.to_string(), span_id: who.span_id.to_string(), role: who.role.to_string(), tier, tool: tool.into(), pattern: pattern.into(), worktree: None, summary: summary.into(), payload: payload.clone(), payload_blake3: payload_blake3(payload), timeout: base_timeout };
            let waiting = self.broker.request(req);
            let id = waiting.approval_id.clone();
            if let Some(raised) = self.broker.get(&id) {
                let mut p = raised.event_payload();
                if let Some(prev) = &reraised_from {
                    p["reraised_from"] = json!(prev);
                }
                who.m.append(ev("approval.requested", p)).await?;
            }
            let resolution = waiting.wait().await;
            // The log says who actually answered: a console client, or a relay and the sender it vouched for.
            let by = match &resolution {
                Resolution::TimedOut => "timeout".to_string(),
                _ => self.broker.resolved_by(&id).unwrap_or_else(|| "console".to_string()),
            };
            who.m.append(ev("approval.resolved", resolution.event_payload(&id, &by))).await?;
            // Away mode: the timed-out card stays denied (its id is dead, so a late answer cannot revive it), but
            // the task waits on a fresh card instead of being told no, until the park window is used up.
            if matches!(resolution, Resolution::TimedOut) && !self.settings.approval_park_max.is_zero() {
                let parked = started.elapsed();
                if parked + base_timeout <= self.settings.approval_park_max {
                    who.m.append(ev("approval.parked", json!({"approval_id": id, "parked_secs": parked.as_secs(), "park_max_secs": self.settings.approval_park_max.as_secs()}))).await?;
                    reraised_from = Some(id);
                    continue;
                }
            }
            return Ok(resolution);
        }
    }

    /// A dispatched child's report as the parent's tool result.
    fn dispatch_result(role: &str, status: &NodeStatus, text: &str) -> tools::ToolResult {
        let mut text: String = text.chars().take(20_000).collect();
        if let NodeStatus::Escalated(r) = status {
            text = format!("{}\n(the {} did not finish: {})", text, role, r);
        }
        tools::ToolResult { content: text, is_error: matches!(status, NodeStatus::Escalated(_)) }
    }

    /// The `timer` tool: set, list or cancel this role's wakes (`specs/017`). Every change is logged in the span, so the
    /// operator can see what each role has promised to come back to.
    async fn exec_timer(self: &Arc<Self>, sc: &mut SpanCtx, args: &Value) -> Result<tools::ToolResult, EngineError> {
        use crate::timers::{relative, NewTimer};
        let now = (self.clock_unix)();
        let at = |t: i64| delphi_core::time::format_unix_millis(t, 0).replace(".000Z", "Z");
        match args["action"].as_str() {
            Some("set") => {
                let at_unix = match args["at"].as_str() {
                    Some(s) => match delphi_core::time::parse_rfc3339(s) {
                        Some(t) => Some(t),
                        None => return Ok(tool_result_error(format!("`at` must be an RFC 3339 time such as {}; it is now {}", at(now + 600), at(now)))),
                    },
                    None => None,
                };
                let req = NewTimer { mission_id: sc.m.id.clone(), target: sc.role.clone(), note: args["note"].as_str().unwrap_or("").into(), creator: sc.role.clone(), ceiling: sc.span.ceiling().as_str().into(), in_secs: args["in_secs"].as_i64(), at_unix, every_secs: args["every_secs"].as_i64() };
                match self.timers.set(req, now) {
                    Ok(t) => {
                        sc.m.append(sc.ev("timer.set", t.to_json())).await?;
                        let every = t.every_secs.map(|e| format!(", then every {}s", e)).unwrap_or_default();
                        Ok(tools::ToolResult { content: format!("timer {} set. It is now {}. You will be woken as `{}` at {} ({}){}. That wake is a new task: you will not be running in between, and you will see only your note: \"{}\"", t.id, at(now), t.target, at(t.due_unix), relative(t.due_unix, now), every, t.note), is_error: false })
                    }
                    Err(e) => Ok(tool_result_error(e.0)),
                }
            }
            Some("list") => {
                let mine: Vec<_> = self.timers.pending().into_iter().filter(|t| t.creator == sc.role).collect();
                let mut out = format!("It is now {}. ", at(now));
                if mine.is_empty() {
                    out.push_str("You have no pending timers.");
                }
                for t in mine {
                    out.push_str(&format!("\n{}  {} ({}){}  \"{}\"", t.id, at(t.due_unix), relative(t.due_unix, now), t.every_secs.map(|e| format!(" every {}s", e)).unwrap_or_default(), t.note));
                }
                Ok(tools::ToolResult { content: out, is_error: false })
            }
            Some("cancel") => match self.timers.cancel(args["id"].as_str().unwrap_or(""), &sc.role) {
                Ok(t) => {
                    sc.m.append(sc.ev("timer.cancelled", t.to_json())).await?;
                    Ok(tools::ToolResult { content: format!("timer {} cancelled", t.id), is_error: false })
                }
                Err(e) => Ok(tool_result_error(e.0)),
            },
            _ => Ok(tool_result_error("`action` must be set, list or cancel")),
        }
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
        let f = setup(vec![Step::Throttle, Step::text("ok")], 5.0, |s| s.degrade_wait = Duration::from_millis(50));
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
        let g = setup(vec![Step::Throttle, Step::Throttle], 5.0, |s| s.degrade_wait = Duration::from_millis(50));
        let out = g.engine.run_task(&g.m, "orchestrator", "hi").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("throttled")), "{:?}", out.status);
        assert_eq!(g.m.budget.lock().unwrap().reserved(), 0, "released, not leaked");
    }

    #[tokio::test]
    async fn an_exhausted_model_is_recorded_with_its_reset_and_the_next_endpoint_takes_over() {
        let f = setup(vec![Step::Exhaust, Step::text("ok")], 5.0, |s| s.degrade_wait = Duration::from_millis(50));
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
        let g = setup(vec![Step::Exhaust, Step::Exhaust], 5.0, |s| s.degrade_wait = Duration::from_millis(50));
        let out = g.engine.run_task(&g.m, "orchestrator", "hi").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("direct is out of quota") && r.contains("resets at")), "{:?}", out.status);
        assert_eq!(g.m.budget.lock().unwrap().reserved(), 0, "released, not leaked");
    }

    /// Claude and Gemini, each with one endpoint; one Gemini model serves every tier.
    fn two_families(f: &Fx) {
        let text = crate::testkit::CONFIG
            .replace("[access]", "[models.gemini]\nfast = \"gem-pro\"\nworkhorse = \"gem-pro\"\nfrontier = \"gem-pro\"\n[models.catalog.\"gem-pro\"]\nmax_output_tokens = 64000\ninput_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0\n[[endpoints]]\nid = \"gem\"\nfamily = \"gemini\"\npath = \"api\"\ncredential = \"env:K\"\n[access]")
            .replace("order = { claude = [\"direct\", \"backup\"] }", "order = { claude = [\"direct\"] }")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"direct\", \"gem\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
    }

    // specs/002 §4.1 tests 1 and 2: promotion at a turn boundary, with no orphaned tool call, and foreign signatures not carried.
    #[tokio::test]
    async fn a_span_whose_model_runs_out_between_turns_finishes_on_another_family_with_its_history() {
        let f = setup(vec![signed_tool_turn("toolu_1", "read_file", json!({"path": "@project/app/src/lib.rs"})), Step::Exhaust, Step::text("Finished on Gemini."), Step::text("Second task.")], 5.0, |_| {});
        two_families(&f);
        let out = f.engine.run_task(&f.m, "architect", "What does @project/app/src/lib.rs define?").await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Finished on Gemini."), "{}", out.text);
        let reqs = f.model.requests();
        let served: Vec<(&str, &str)> = reqs.iter().map(|r| (r.endpoint_id.as_str(), r.model_id.as_str())).collect();
        assert_eq!(served, vec![("direct", "opus-x"), ("direct", "opus-x"), ("gem", "gem-pro")], "turn 2 ran out on Claude and was served by Gemini");
        // The Gemini request carries the whole history: the tool call and its result, but not Claude's signed reasoning.
        let body = reqs[2].body.to_string();
        assert!(body.contains("read_file") && body.contains("pub fn needle"), "the tool call and its result are in the history: {}", body);
        assert!(!body.contains("SIG-EXACT"), "a foreign signature is never sent to another provider");
        // Events: the exhaustion with its reset, then the promotion naming both pairs.
        let ex = events_of(&f, "model.exhausted");
        assert_eq!((ex.len(), ex[0].payload["endpoint_id"].as_str()), (1, Some("direct")));
        let pr = events_of(&f, "model.promoted");
        assert_eq!(pr.len(), 1);
        assert_eq!((pr[0].payload["from"]["endpoint_id"].as_str(), pr[0].payload["to"]["endpoint_id"].as_str(), pr[0].payload["to"]["model_id"].as_str()), (Some("direct"), Some("gem"), Some("gem-pro")));
        assert!(pr[0].payload["reason"].as_str().unwrap().contains("exhausted"), "{}", pr[0].payload);
        // Promotion is not a retry: the exhausted call's reservation was released and nothing is left reserved.
        assert_eq!(f.m.budget.lock().unwrap().reserved(), 0);
        assert!(!kinds(&f).contains(&"turn.interrupted".to_string()));
        // The next task goes straight to Gemini: the pool remembers Claude is out, so it is not tried again.
        let again = f.engine.run_task(&f.m, "architect", "One more").await.unwrap();
        assert_eq!(again.text, "Second task.");
        let reqs = f.model.requests();
        assert_eq!(reqs.len(), 4);
        assert_eq!(reqs[3].endpoint_id, "gem", "no request was spent on the exhausted pair");
        assert_eq!(events_of(&f, "model.promoted").len(), 2, "a new span reports its own substitution");
    }

    /// Every `opus-x` pair is out of quota for `secs` more seconds (the architect's tier is frontier, so only weaker models are left).
    fn opus_out(f: &Fx, secs: i64) {
        let reset = f.now.load(std::sync::atomic::Ordering::SeqCst) + secs;
        for ep in ["direct", "backup"] {
            f.engine.pool.record_exhausted(ep, "opus-x", &delphi_adapter::exhaust::Exhaustion { reason: "quota reached".into(), reset_at: Some(reset), raw: String::new() });
        }
    }

    async fn wait_for_event(f: &Fx, kind: &str) {
        for _ in 0..400 {
            if !events_of(f, kind).is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no `{}` event appeared", kind);
    }

    fn spawn_task(f: &Fx, role: &'static str, prompt: &'static str) -> tokio::task::JoinHandle<Result<TaskOutcome, EngineError>> {
        let (e, m) = (f.engine.clone(), f.m.clone());
        tokio::spawn(async move { e.run_task(&m, role, prompt).await })
    }

    // specs/002 §4.1 test 6a: a weaker substitute does not run until the operator confirms, and it is asked about once per task.
    #[tokio::test]
    async fn a_weaker_substitute_runs_only_after_the_operator_confirms_and_is_asked_once_for_the_task() {
        let f = setup(vec![Step::tool("c1", "read_file", json!({"path": "@project/app/src/lib.rs"})), Step::text("Done on the weaker model.")], 5.0, |_| {});
        opus_out(&f, 3600);
        let task = spawn_task(&f, "architect", "What does @project/app/src/lib.rs define?");
        let card = wait_for_card(&f).await;
        assert_eq!((card.tool.as_str(), card.tier), ("model", ApprovalTier::Amber));
        assert!(card.summary.contains("direct/opus-x is unavailable") && card.summary.contains("direct/sonnet-x") && card.summary.contains("tier workhorse instead of frontier"), "{}", card.summary);
        assert!(f.model.requests().is_empty(), "nothing runs while the question is open");
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Allow, "console-1").unwrap();
        let out = task.await.unwrap().unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Done on the weaker model."), "{}", out.text);
        let reqs = f.model.requests();
        assert_eq!(reqs.len(), 2, "both turns ran on the confirmed substitute");
        assert!(reqs.iter().all(|r| (r.endpoint_id.as_str(), r.model_id.as_str()) == ("direct", "sonnet-x")));
        assert_eq!(events_of(&f, "approval.requested").iter().filter(|e| e.payload["tool"] == "model").count(), 1, "the second turn did not ask again");
        let states: Vec<String> = events_of(&f, "model.degraded_confirm").iter().map(|e| e.payload["state"].as_str().unwrap().to_string()).collect();
        assert_eq!(states, vec!["asked", "confirmed"]);
        assert!(events_of(&f, "model.promoted")[0].payload["reason"].as_str().unwrap().contains("confirmed by the operator"));
        assert_eq!(f.m.budget.lock().unwrap().reserved(), 0);
    }

    // specs/002 §4.1 test 6a, the other half: declining does not stop the task, it waits for the target.
    #[tokio::test]
    async fn declining_a_weaker_substitute_parks_the_task_until_the_target_returns() {
        let f = setup(vec![Step::text("Back on the target.")], 5.0, |s| {
            s.park_max = None;
        });
        opus_out(&f, 3600);
        let task = spawn_task(&f, "architect", "Plan the change");
        let card = wait_for_card(&f).await;
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "wait for opus".into() }, "console-1").unwrap();
        wait_for_event(&f, "task.blocked").await;
        let blocked = &events_of(&f, "task.blocked")[0].payload;
        assert_eq!(blocked["reason"], "no_model");
        assert!(blocked["back_at"].as_i64().unwrap() > f.now.load(std::sync::atomic::Ordering::SeqCst), "it says when the target returns");
        assert!(f.model.requests().is_empty() && !task.is_finished(), "the weaker model never ran, and the task is waiting, not failed");
        f.advance(3601);
        let out = task.await.unwrap().unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Back on the target."), "{}", out.text);
        assert_eq!(f.model.requests().iter().map(|r| r.model_id.clone()).collect::<Vec<_>>(), vec!["opus-x"]);
        let u = &events_of(&f, "task.unblocked")[0].payload;
        assert_eq!(u["resumed"], true);
        let states: Vec<String> = events_of(&f, "model.degraded_confirm").iter().map(|e| e.payload["state"].as_str().unwrap().to_string()).collect();
        assert_eq!(states, vec!["asked", "declined"], "one question; once declined it is not asked again");
    }

    // specs/002 §4.1 test 6 with nobody to answer: silence is retried, never taken as a yes or a no.
    #[tokio::test]
    async fn with_every_model_throttled_and_nobody_to_ask_the_task_keeps_looking_and_resumes() {
        let f = setup(vec![Step::Throttle, Step::Throttle, Step::text("Resumed.")], 5.0, |s| {
            s.park_max = None;
            s.degrade_wait = Duration::from_millis(40);
        });
        let task = spawn_task(&f, "architect", "Plan the change");
        for _ in 0..400 {
            if events_of(&f, "model.degraded_confirm").iter().filter(|e| e.payload["state"] == "asked").count() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!task.is_finished(), "still looking, not failed");
        let states: Vec<String> = events_of(&f, "model.degraded_confirm").iter().map(|e| e.payload["state"].as_str().unwrap().to_string()).collect();
        assert!(states.iter().filter(|s| *s == "asked").count() >= 2 && states.contains(&"timed_out".to_string()), "it asked again after the first card went unanswered: {:?}", states);
        assert!(!states.contains(&"confirmed".to_string()) && !states.contains(&"declined".to_string()), "silence is neither: {:?}", states);
        f.advance(130); // the cooldown after a throttle with no retry-after is two minutes
        let out = task.await.unwrap().unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Resumed."), "{}", out.text);
        assert_eq!(f.model.requests().len(), 3, "it never ran on a weaker model");
        assert_eq!(f.m.budget.lock().unwrap().reserved(), 0);
    }

    // Your decision (2026-10-24): an unanswered downgrade card is asked again, and a later answer is honoured.
    #[tokio::test]
    async fn an_unanswered_downgrade_card_is_asked_again_and_a_later_yes_runs_it() {
        let f = setup(vec![Step::text("Done on the weaker model.")], 5.0, |s| {
            s.park_max = None;
            s.degrade_wait = Duration::from_millis(250);
        });
        opus_out(&f, 3600);
        let task = spawn_task(&f, "architect", "Plan the change");
        let first = wait_for_card(&f).await;
        wait_for_event_state(&f, "timed_out").await;
        let second = loop {
            let c = wait_for_card(&f).await;
            if c.approval_id != first.approval_id {
                break c;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        f.engine.broker.resolve(&second.approval_id, &second.payload_blake3, Answer::Allow, "console-1").unwrap();
        let out = task.await.unwrap().unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Done on the weaker model."), "{}", out.text);
        let states: Vec<String> = events_of(&f, "model.degraded_confirm").iter().map(|e| e.payload["state"].as_str().unwrap().to_string()).collect();
        assert_eq!(states, vec!["asked", "timed_out", "asked", "confirmed"]);
    }

    // Your decision (2026-10-24): a confirmation survives a daemon restart within the session (the mission log).
    #[tokio::test]
    async fn what_the_operator_decided_about_a_weaker_model_is_rebuilt_from_the_mission_log() {
        let f = setup(vec![], 5.0, |_| {});
        // `note(state, task, target, substitute)`; each side is `endpoint/model`.
        let note = |state: &str, task: &str, target: (&str, &str), sub: (&str, &str)| {
            let mut e = EventEnvelope::new(&f.m.id, "model.degraded_confirm", json!({"state": state, "target": {"endpoint_id": target.0, "model_id": target.1}, "substitute": {"endpoint_id": sub.0, "model_id": sub.1}}));
            e.task_id = Some(task.to_string());
            e
        };
        let (opus, gem) = (("direct", "opus-x"), ("gem", "gem-pro"));
        f.m.append(note("asked", "t_0042", opus, ("direct", "sonnet-x"))).await.unwrap();
        f.m.append(note("confirmed", "t_0042", opus, ("direct", "sonnet-x"))).await.unwrap();
        f.m.append(note("declined", "t_0042", opus, ("backup", "haiku-x"))).await.unwrap();
        f.m.append(note("confirmed", "t_0043", opus, ("direct", "haiku-x"))).await.unwrap();
        // A restarted daemon has nothing in memory.
        f.engine.degraded.lock().unwrap().clear();
        let pair = |t: (&str, &str), s: (&str, &str)| (format!("{}/{}", t.0, t.1), format!("{}/{}", s.0, s.1));
        assert_eq!(f.engine.degraded_decisions(&f.m, "t_0042", &pair(opus, ("direct", "sonnet-x"))), (true, false), "confirmed before the restart");
        assert_eq!(f.engine.degraded_decisions(&f.m, "t_0042", &pair(opus, ("backup", "haiku-x"))), (false, true), "declined before the restart");
        assert_eq!(f.engine.degraded_decisions(&f.m, "t_0042", &pair(opus, ("direct", "haiku-x"))), (false, false), "another substitute was never decided");
        assert_eq!(f.engine.degraded_decisions(&f.m, "t_0099", &pair(opus, ("direct", "sonnet-x"))), (false, false), "a confirmation covers one task only");
        // Consent to sonnet instead of opus is not consent to sonnet instead of a model with a far larger window.
        assert_eq!(f.engine.degraded_decisions(&f.m, "t_0042", &pair(gem, ("direct", "sonnet-x"))), (false, false), "another target, same substitute: ask again");
        // A later answer wins: confirming what was declined.
        f.m.append(note("confirmed", "t_0042", opus, ("backup", "haiku-x"))).await.unwrap();
        f.engine.degraded.lock().unwrap().clear();
        assert_eq!(f.engine.degraded_decisions(&f.m, "t_0042", &pair(opus, ("backup", "haiku-x"))), (true, false));
        // An answer logged without its target (an older log) cannot be matched, so it is never trusted.
        let mut old = EventEnvelope::new(&f.m.id, "model.degraded_confirm", json!({"state": "confirmed", "substitute": {"endpoint_id": "direct", "model_id": "fast-x"}}));
        old.task_id = Some("t_0042".into());
        f.m.append(old).await.unwrap();
        f.engine.degraded.lock().unwrap().clear();
        assert_eq!(f.engine.degraded_decisions(&f.m, "t_0042", &pair(opus, ("direct", "fast-x"))), (false, false));
        // A different mission (a new session) starts with no decisions at all.
        let other = tempfile::tempdir().unwrap();
        let fresh = MissionState::open(other.path(), "m2", Hub::new(), f.engine.ledger.clone(), SecretRegistry::new(), 5.0).unwrap();
        f.engine.degraded.lock().unwrap().clear();
        assert_eq!(f.engine.degraded_decisions(&fresh, "t_0042", &pair(opus, ("direct", "sonnet-x"))), (false, false));
    }

    // Independent review (Gemini, 2026-10-25): the card's text is redacted like the rest of what it carries.
    #[tokio::test]
    async fn the_downgrade_card_never_shows_a_registered_secret() {
        let f = setup(vec![Step::text("done")], 5.0, |s| s.degrade_wait = Duration::from_millis(300));
        f.engine.secrets.register("sk-live-REGISTERED-SECRET");
        let reset = f.now.load(std::sync::atomic::Ordering::SeqCst) + 3600;
        for ep in ["direct", "backup"] {
            f.engine.pool.record_exhausted(ep, "opus-x", &delphi_adapter::exhaust::Exhaustion { reason: "quota reached for key sk-live-REGISTERED-SECRET".into(), reset_at: Some(reset), raw: String::new() });
        }
        let task = spawn_task(&f, "architect", "Plan the change");
        let card = wait_for_card(&f).await;
        assert!(!card.summary.contains("REGISTERED-SECRET") && !card.payload.to_string().contains("REGISTERED-SECRET"), "{} {}", card.summary, card.payload);
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Allow, "console-1").unwrap();
        task.await.unwrap().unwrap();
        let logged = serde_json::to_string(&f.m.replay().unwrap()).unwrap();
        assert!(logged.contains("approval.requested") && logged.contains("model.degraded_confirm"), "the events are there to be checked");
        assert!(!logged.contains("REGISTERED-SECRET"), "not in the log either");
    }

    // Independent review (Gemini, 2026-10-25): a substitute that fails at once was reported as promoted into service.
    #[tokio::test]
    async fn a_substitute_that_fails_at_once_is_not_reported_as_promoted() {
        let f = setup(vec![Step::Exhaust, Step::Throttle, Step::text("Served by the third.")], 5.0, |_| {});
        let text = crate::testkit::CONFIG
            .replace("[access]", "[models.gemini]\nfast = \"gem-pro\"\nworkhorse = \"gem-pro\"\nfrontier = \"gem-pro\"\n[models.catalog.\"gem-pro\"]\nmax_output_tokens = 64000\ninput_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0\n[[endpoints]]\nid = \"gem\"\nfamily = \"gemini\"\npath = \"api\"\ncredential = \"env:K\"\n[access]")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"direct\", \"backup\", \"gem\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
        let out = f.engine.run_task(&f.m, "architect", "Plan").await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Served by the third."), "{}", out.text);
        let served: Vec<&str> = f.model.requests().iter().map(|r| r.endpoint_id.as_str()).collect::<Vec<_>>().into_iter().map(|s| if s == "direct" { "direct" } else if s == "backup" { "backup" } else { "gem" }).collect();
        assert_eq!(served, vec!["direct", "backup", "gem"], "direct ran out, backup throttled at once, gem served");
        let promoted = events_of(&f, "model.promoted");
        assert_eq!(promoted.len(), 1, "only the substitute that served is reported: {:?}", promoted.iter().map(|e| e.payload.to_string()).collect::<Vec<_>>());
        assert_eq!(promoted[0].payload["to"]["endpoint_id"], "gem");
    }

    async fn wait_for_event_state(f: &Fx, state: &str) {
        for _ in 0..600 {
            if events_of(f, "model.degraded_confirm").iter().any(|e| e.payload["state"] == state) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no model.degraded_confirm with state `{}`", state);
    }

    #[tokio::test]
    async fn a_task_stops_waiting_for_a_model_after_park_max_and_escalates_with_the_reason() {
        let f = setup(vec![Step::Throttle, Step::Throttle], 5.0, |s| {
            s.park_max = Some(Duration::from_millis(120));
            s.degrade_wait = Duration::from_millis(20);
        });
        let out = f.engine.run_task(&f.m, "architect", "Plan the change").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.contains("throttled")), "{:?}", out.status);
        assert!(!events_of(&f, "task.blocked").is_empty());
        assert_eq!(events_of(&f, "task.unblocked").last().unwrap().payload["resumed"], false, "it gave up after park_max, however many times it asked");
    }

    // specs/005e §6.2: an empty pool is "no model is configured", not "throttled", and waiting would never help.
    #[tokio::test]
    async fn an_empty_pool_escalates_as_no_model_configured_without_waiting() {
        let f = setup(vec![Step::text("never asked")], 5.0, |s| s.park_max = Some(Duration::from_secs(60)));
        let text = crate::testkit::CONFIG.split("[[endpoints]]").next().unwrap().to_string();
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
        assert!(f.engine.cfg.get().pool_entries().is_empty());
        let out = f.engine.run_task(&f.m, "orchestrator", "hello").await.unwrap();
        assert!(matches!(&out.status, Status::Escalated(r) if r.starts_with("no_model_configured") && !r.contains("throttled")), "{:?}", out.status);
        assert!(f.model.requests().is_empty(), "no call was made");
        assert!(events_of(&f, "task.blocked").is_empty(), "it did not park waiting for a model that cannot arrive");
    }

    // specs/002 §4.1, [H-007] and [H-008]: a critic stays out of the author's family, and says so when it cannot.
    #[tokio::test]
    async fn a_critic_span_stays_out_of_the_authors_family_and_says_so_when_it_cannot() {
        let f = setup(vec![Step::text("Reviewed by the other family."), Step::text("Reviewed by the same family.")], 5.0, |_| {});
        two_families(&f);
        let critic = |f: &Fx, id: &str| {
            let mut h = HandoffEnvelope::root(&f.m.id, id, "red-team", "Review the change", vec![]);
            h.routing_directive.model_family = Some("gemini".into());
            h.routing_directive.avoid_families = vec!["claude".into()];
            h
        };
        let out = f.engine.run_span(&f.m, "t_9001", critic(&f, "t_9001"), None, 0).await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Reviewed by the other family."), "{}", out.text);
        assert_eq!(f.model.requests()[0].endpoint_id, "gem");
        assert!(events_of(&f, "model.homogeneous").is_empty(), "another family was usable");
        // Gemini runs out: the critic still runs (never blocked), on the author's family, and the review is labeled.
        let reset = f.now.load(std::sync::atomic::Ordering::SeqCst) + 3600;
        f.engine.pool.record_exhausted("gem", "gem-pro", &delphi_adapter::exhaust::Exhaustion { reason: "quota reached".into(), reset_at: Some(reset), raw: String::new() });
        let out = f.engine.run_span(&f.m, "t_9002", critic(&f, "t_9002"), None, 0).await.unwrap();
        assert_eq!((out.status.clone(), out.text.as_str()), (Status::Done, "Reviewed by the same family."), "{}", out.text);
        assert_eq!(f.model.requests()[1].endpoint_id, "direct");
        let h = events_of(&f, "model.homogeneous");
        assert_eq!(h.len(), 1);
        assert_eq!((h[0].payload["avoided"][0].as_str(), h[0].payload["used"]["family"].as_str()), (Some("claude"), Some("claude")));
    }

    // M8 sweep: the endpoint policy is a wall. An endpoint outside `allowed_endpoints` is never in the pool and never called,
    // however many models fail.
    #[tokio::test]
    async fn an_endpoint_outside_the_pin_is_never_called_however_many_models_fail() {
        let f = setup(vec![Step::Exhaust, Step::Exhaust, Step::Throttle, Step::Throttle, Step::Exhaust, Step::Exhaust], 5.0, |s| s.degrade_wait = Duration::from_millis(30));
        let text = crate::testkit::CONFIG.replace("[access]", "[models.gemini]\nfast = \"gem-pro\"\nworkhorse = \"gem-pro\"\nfrontier = \"gem-pro\"\n[models.catalog.\"gem-pro\"]\nmax_output_tokens = 64000\ninput_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0\n[[endpoints]]\nid = \"gem\"\nfamily = \"gemini\"\npath = \"api\"\ncredential = \"env:K\"\n[access]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
        assert!(f.engine.cfg.get().pool_entries().iter().all(|k| k.endpoint_id != "gem"), "not allowed, so not in the pool");
        for prompt in ["one", "two", "three"] {
            let out = f.engine.run_task(&f.m, "architect", prompt).await.unwrap();
            assert!(matches!(out.status, Status::Escalated(_)), "{:?}", out.status);
        }
        assert!(f.model.requests().iter().all(|r| r.endpoint_id == "direct" || r.endpoint_id == "backup"), "{:?}", f.model.requests().iter().map(|r| r.endpoint_id.clone()).collect::<Vec<_>>());
        assert_eq!(f.m.budget.lock().unwrap().reserved(), 0);
    }

    // M8 sweep: a promotion is charged at the substitute's prices, and the exhausted call is released, not charged.
    #[tokio::test]
    async fn a_promotion_is_charged_at_the_substitutes_prices_and_the_exhausted_call_costs_nothing() {
        let f = setup(vec![Step::Exhaust, Step::text("ok")], 5.0, |_| {});
        two_families(&f);
        let out = f.engine.run_task(&f.m, "architect", "Plan the change").await.unwrap();
        assert_eq!(out.status, Status::Done);
        // One reply of 1000 input and 200 output tokens on gem-pro at $2 and $12 per million: $0.0044.
        let b = f.m.budget.lock().unwrap();
        assert_eq!((b.spent(), b.reserved()), (4400, 0));
        drop(b);
        let usage = events_of(&f, "agent.usage");
        assert_eq!(usage.len(), 1, "the exhausted call recorded no usage");
        assert_eq!((usage[0].endpoint_id.as_deref(), usage[0].model_id.as_deref()), (Some("gem"), Some("gem-pro")));
        assert!((f.engine.ledger.lock().unwrap().mission_cost_usd(&f.m.id).unwrap() - 0.0044).abs() < 1e-9, "the ledger agrees with the budget");
    }

    // M8 sweep: cancelling a task that is waiting for a model ends it cleanly, with nothing reserved and nothing left open.
    #[tokio::test]
    async fn cancelling_a_task_that_is_waiting_for_a_model_leaves_nothing_open() {
        let f = setup(vec![Step::text("never reached")], 5.0, |s| {
            s.park_max = None;
        });
        opus_out(&f, 3600);
        let task = spawn_task(&f, "architect", "Plan the change");
        let card = wait_for_card(&f).await;
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Deny { reason: "wait".into() }, "console-1").unwrap();
        wait_for_event(&f, "task.blocked").await;
        let tid = events_of(&f, "task.created")[0].task_id.clone().unwrap();
        task.abort();
        f.engine.cancel_task(&f.m, &tid).await.unwrap();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(events_of(&f, "task.cancelled").len(), 1);
        assert!(f.engine.broker.pending().is_empty() && f.m.budget.lock().unwrap().reserved() == 0 && f.model.requests().is_empty());
        // The mission is still usable afterwards: a later task runs normally once the target is back.
        f.advance(3601);
        let out = f.engine.run_task(&f.m, "architect", "Again").await.unwrap();
        assert_eq!((out.status, out.text.as_str()), (Status::Done, "never reached"));
    }

    // The operator's request: the running agent and its model, from the events the daemon really writes.
    #[tokio::test]
    async fn the_observer_shows_a_running_agent_and_its_model_from_the_real_event_stream() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "# New"})), Step::text("Drafted.")], 5.0, |_| {});
        let fold = |f: &Fx| {
            let mut st = crate::observe::ObserverState::new(&f.engine.settings.enabled_agents);
            for e in f.m.replay().unwrap().into_iter().filter(|e| crate::observe::KINDS.contains(&e.kind.as_str())) {
                st.apply(&serde_json::to_value(e).unwrap());
            }
            st
        };
        let task = spawn_task(&f, "architect", "Draft a spec");
        let card = wait_for_card(&f).await;
        let st = fold(&f);
        let rows = st.running_agents();
        assert_eq!(rows.len(), 1, "{:?}", rows);
        assert_eq!((rows[0].role.as_str(), crate::observe::ObserverState::model_label(rows[0]).as_str(), rows[0].waiting.as_str()), ("architect", "opus-x via direct (frontier)", "waiting on you"));
        f.engine.broker.resolve(&card.approval_id, &card.payload_blake3, Answer::Allow, "console-1").unwrap();
        task.await.unwrap().unwrap();
        let st = fold(&f);
        assert!(st.running_agents().is_empty(), "a finished agent is not listed: {:?}", st.running_agents());
        assert_eq!(st.describe_running(&|r| r.to_string()), "Nothing is running.\n");
        assert_eq!(st.agents["architect"].model, "opus-x", "the roster still remembers its last model");
    }

    #[tokio::test]
    async fn outcomes_are_recorded_in_the_pool_per_endpoint_and_model() {
        use crate::pool::State;
        let f = setup(vec![Step::Exhaust, Step::text("ok")], 5.0, |_| {});
        f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
        let pool = f.engine.pool.snapshot();
        assert_eq!(pool.len(), 1, "only the failed pair is recorded: {:?}", pool);
        assert_eq!((pool[0].endpoint_id.as_str(), pool[0].state), ("direct", State::Exhausted));
        assert!(pool[0].until.is_some(), "the scripted exhaustion carries a reset time");
        assert_eq!(f.engine.pool.state("backup", &pool[0].model_id), State::Available, "the same model on another endpoint is untouched");
        assert!(f.engine.ws.dir.join("local/pool-state.json").exists(), "state is mirrored to the workspace");
    }

    #[tokio::test]
    async fn throttling_task_after_task_for_ten_minutes_becomes_exhaustion() {
        use crate::pool::State;
        let f = setup(vec![Step::Throttle, Step::Throttle, Step::Throttle, Step::Throttle, Step::Throttle, Step::Throttle], 5.0, |s| s.degrade_wait = Duration::from_millis(50));
        for gap in [0, 300, 360] {
            f.advance(gap);
            let out = f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
            assert!(matches!(out.status, Status::Escalated(_)), "both endpoints throttle every time");
        }
        let ex = events_of(&f, "model.exhausted");
        assert_eq!(ex.len(), 2, "direct and backup each tip over on the third throttle, 11 minutes after the first: {:?}", ex.iter().map(|e| e.payload.to_string()).collect::<Vec<_>>());
        assert!(ex[0].payload["reason"].as_str().unwrap().contains("3 times in a row over 11 minutes"));
        assert!(ex[0].payload["reset_at"].is_null());
        let snap = f.engine.pool.snapshot();
        assert!(snap.iter().all(|e| e.state == State::Exhausted && e.next_probe_at.is_some()), "{:?}", snap);
    }

    #[tokio::test]
    async fn an_exhausted_pair_with_no_known_reset_is_probed_when_due_and_recovers() {
        use crate::pool::State;
        let f = setup(vec![Step::Exhaust, Step::text("ok")], 5.0, |s| s.degrade_wait = Duration::from_millis(50));
        f.engine.run_task(&f.m, "orchestrator", "hi").await.unwrap();
        let key = f.engine.pool.snapshot().remove(0);
        // Forget the reset time so only a probe can bring it back.
        f.engine.pool.record_exhausted(&key.endpoint_id, &key.model_id, &delphi_adapter::exhaust::Exhaustion { reason: "quota reached".into(), reset_at: None, raw: String::new() });
        let before = f.model.requests().len();
        f.model.respond_with(|_| Step::text("OK"));
        assert!(f.engine.pool.due_probes().is_empty(), "not due for 15 minutes");
        f.advance(901);
        f.engine.run_task(&f.m, "orchestrator", "again").await.unwrap();
        let reqs = f.model.requests();
        let probe = &reqs[before];
        assert_eq!((probe.endpoint_id.as_str(), probe.body["max_tokens"].as_u64()), (key.endpoint_id.as_str(), Some(64)), "a capped one-word probe of the exhausted pair first");
        assert_eq!(f.engine.pool.state(&key.endpoint_id, &key.model_id), State::Available);
        assert_eq!(events_of(&f, "model.recovered").len(), 1);
    }

    #[tokio::test]
    async fn an_exhausted_budget_escalates_before_any_call_and_never_downscales() {
        let f = setup(vec![Step::text("should never be asked")], 0.0001, |s| s.degrade_wait = Duration::from_millis(50));
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

    /// The next pending card whose id is not `not`.
    async fn next_card_after(f: &Fx, not: &str) -> Request {
        for _ in 0..400 {
            if let Some(r) = f.engine.broker.pending().into_iter().find(|r| r.approval_id != not) {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no second approval card appeared");
    }

    #[tokio::test]
    async fn away_mode_re_raises_a_timed_out_card_under_a_new_id_and_the_old_id_stays_dead() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-park.md", "content": "# Parked\nbody"})), Step::text("Drafted.")], 5.0, |s| {
            s.approval_timeout = Duration::from_millis(120);
            s.approval_park_max = Duration::from_secs(30);
        });
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "architect", "Draft a spec").await }) };
        let first = wait_for_card(&f).await;
        // Nobody answers. The first card times out; the task is not failed, and a new card appears.
        let second = next_card_after(&f, &first.approval_id).await;
        assert_ne!(second.approval_id, first.approval_id);
        assert_eq!(second.payload_blake3, first.payload_blake3, "it is the same action");
        // A late answer to the dead card is refused, whoever sends it and whatever it says.
        let late = f.engine.broker.resolve(&first.approval_id, &first.payload_blake3, Answer::Allow, "relay-email");
        assert!(matches!(late, Err(crate::approval::ResolveError::Gone { resolution: Some(Resolution::TimedOut), .. })), "{:?}", late);
        assert!(!f.root.join("workspaces/w/team/specs/009-park.md").exists(), "nothing ran while parked");
        // Only the fresh card can approve it.
        f.engine.broker.resolve(&second.approval_id, &second.payload_blake3, Answer::Allow, "relay-email").unwrap();
        assert_eq!(task.await.unwrap().unwrap().status, Status::Done);
        assert!(f.root.join("workspaces/w/team/specs/009-park.md").exists());
        let req = events_of(&f, "approval.requested");
        assert!(req.len() >= 2 && req.last().unwrap().payload["reraised_from"].is_string(), "the re-raise says what it replaces");
        assert!(!events_of(&f, "approval.parked").is_empty());
        assert_eq!(f.model.requests().len(), 2, "no model call was made while the card was parked");
    }

    #[tokio::test]
    async fn away_mode_still_denies_once_the_park_window_is_used_up() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-park.md", "content": "x"})), Step::text("Understood.")], 5.0, |s| {
            s.approval_timeout = Duration::from_millis(60);
            s.approval_park_max = Duration::from_millis(200);
        });
        let out = f.engine.run_task(&f.m, "architect", "Draft a spec").await.unwrap();
        assert_eq!(out.status, Status::Done);
        assert!(!f.root.join("workspaces/w/team/specs/009-park.md").exists());
        let asked = events_of(&f, "approval.requested").len();
        assert!((2..=4).contains(&asked), "re-raised a few times and then stopped: {}", asked);
        assert!(f.model.requests()[1].body["messages"].to_string().contains("timed out"), "the model is finally told it was denied");
        assert_eq!(events_of(&f, "approval.resolved").last().unwrap().payload["resolution"], "timed_out");
    }

    #[tokio::test]
    async fn without_a_park_window_a_timeout_ends_the_wait_as_before() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-park.md", "content": "x"})), Step::text("Understood.")], 5.0, |s| s.approval_timeout = Duration::from_millis(60));
        f.engine.run_task(&f.m, "architect", "Draft a spec").await.unwrap();
        assert_eq!(events_of(&f, "approval.requested").len(), 1);
        assert!(events_of(&f, "approval.parked").is_empty());
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

    // Multi-threaded: after the simulated death a detached commit may still be finishing, and `restart` blocks this thread.
    #[tokio::test(flavor = "multi_thread")]
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
