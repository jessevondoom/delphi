//! Endpoint capability probing (`specs/009` §6): a minimal, token-capped suite sent to allowed endpoints
//! only, recording what each endpoint × model really does instead of what a vendor page says.
//!
//! Probes, in order, stopping at the first failure that makes the rest meaningless:
//! 1. **plain**: authentication, a reply, and whether usage is reported (≤ 64 output tokens);
//! 2. **tools (+ reasoning)**: a forced tool call, with the model's minimum thinking budget where it has
//!    one (≤ budget + 512 output tokens); records tool calling, reasoning and whether signatures exist;
//! 3. **signature round trip**: the tool result sent back with the signed reasoning replayed, which is
//!    what proves a native endpoint (or a gateway claiming passthrough) preserves signatures.
//!
//! Not probed in v1, and recorded as such: prompt caching (it needs a prefix over 1K tokens, which
//! costs real money), effective context limits, and cross-endpoint `E1 → E2` replay.

use crate::harness::{Cli, Event, Launch, Session, Tools};
use crate::model::{ModelClient, ModelError};
use std::time::Duration;
use delphi_adapter::blocks::{ContentBlock, Turn};
use delphi_adapter::claude::{CompileOptions, Target, ToolDef};
use delphi_adapter::config::{AccessPath, Config, Endpoint};
use delphi_adapter::ipr::Ipr;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

/// Cached results are trusted for a day.
pub const TTL_SECS: i64 = 24 * 3600;
const PLAIN_MAX_TOKENS: u64 = 64;
const TOOL_MAX_TOKENS: u64 = 512;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capability {
    pub endpoint_id: String,
    pub model_id: String,
    pub path: String,
    /// A compat gateway: tools work, reasoning signatures and cache breakpoints do not carry.
    pub degraded: bool,
    pub probed_at: i64,
    /// `ok`, or why not (`auth failed`, `re-authenticate`, `throttled`, `failed: …`).
    pub auth: String,
    pub usage_reported: Option<bool>,
    pub tool_calling: Option<bool>,
    pub reasoning: Option<bool>,
    pub signature_round_trip: Option<bool>,
    pub cache: String,
    pub tokens_used: u64,
    pub cost_usd: Option<f64>,
    pub notes: Vec<String>,
}

impl Capability {
    pub fn is_fresh(&self, now: i64) -> bool {
        now - self.probed_at < TTL_SECS
    }
}

/// What the last conformance probe of an `agy` seat found (`specs/020` §8.5). One per endpoint, in `local/agy-gate.json`, apart
/// from the capabilities cache because it outlives that cache's day and is keyed to the CLI's version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateRecord {
    pub endpoint_id: String,
    /// `agy --version` when it was probed. A different version has not been probed.
    pub agy_version: String,
    pub at: i64,
    /// Every tool the gate saw the model reach for, by the CLI's own name (all of them denied in the probe).
    pub attempted: Vec<String>,
    /// The kinds of tool among them (`shell`, `edit`, `browser`, ...).
    pub kinds: Vec<String>,
    /// Delphi's own tool reached the span through the plugin.
    pub served: bool,
    /// Anything that happened although the gate should have stopped it.
    pub side_effects: Vec<String>,
    pub verified: bool,
}

/// How long a conformance record is trusted. It is tied to the CLI's version, so what ages it out is time, not an update.
pub const GATE_TTL_SECS: i64 = 14 * 24 * 3600;
/// Fewest kinds of tool the probe must have seen denied before it calls the gate verified.
pub const GATE_MIN_KINDS: usize = 5;

impl GateRecord {
    pub fn is_fresh(&self, now: i64) -> bool {
        now - self.at < GATE_TTL_SECS
    }
}

/// The kind of tool, from the name the hook receives.
pub fn tool_kind(name: &str) -> &'static str {
    match name {
        "run_command" | "send_command_input" | "command_status" => "shell",
        "read_file" | "view_file" | "list_dir" | "find_by_name" | "grep_search" | "sed_file" | "read_resource" | "list_resources" => "read",
        "write_file" | "write_to_file" => "write",
        "replace_file_content" | "multi_replace_file_content" | "notebook_edit" | "edit_file" => "edit",
        "search_web" => "web search",
        "read_url_content" => "web read",
        "invoke_subagent" | "define_subagent" | "manage_subagents" => "subagent",
        "schedule" | "manage_task" | "run_workflow" => "schedule",
        "generate_image" => "image",
        "call_mcp_tool" => "mcp",
        n if n.starts_with("browser_") || n.contains("browser") => "browser",
        _ => "other",
    }
}

/// `agy --version` as installed now, asked at most once every five minutes by a running daemon.
pub fn installed_agy_version(program: &str) -> Option<String> {
    type Cache = std::collections::HashMap<String, (std::time::Instant, Option<String>)>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    if let Some((at, v)) = CACHE.lock().unwrap().get_or_insert_with(Default::default).get(program) {
        if at.elapsed() < Duration::from_secs(300) {
            return v.clone();
        }
    }
    let v = agy_version(program);
    CACHE.lock().unwrap().get_or_insert_with(Default::default).insert(program.to_string(), (std::time::Instant::now(), v.clone()));
    v
}

pub fn load_gates(path: &Path) -> Vec<GateRecord> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Replace the record for this endpoint; keep the others.
pub fn save_gate(path: &Path, rec: &GateRecord) -> std::io::Result<()> {
    let mut all: Vec<GateRecord> = load_gates(path).into_iter().filter(|g| g.endpoint_id != rec.endpoint_id).collect();
    all.push(rec.clone());
    all.sort_by(|a, b| a.endpoint_id.cmp(&b.endpoint_id));
    delphi_core::lock::write_atomic(path, serde_json::to_string_pretty(&all).expect("serializes").as_bytes())
}

/// `<program> --version`, first line: what a conformance record is tied to. `None` if it will not run.
pub fn agy_version(program: &str) -> Option<String> {
    let out = std::process::Command::new(program).arg("--version").stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    let line = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    (out.status.success() && !line.is_empty()).then_some(line)
}

fn probe_ipr(contract: &str) -> Ipr {
    Ipr { governance: "This is a capability probe. Follow the contract exactly.".into(), role: "You are a connectivity probe.".into(), persona: None, guidance: None, bundle_skills: None, heuristics: "None.".into(), lessons: "None.".into(), context_slices: vec![], contract: contract.into(), reminder: None, nonce: "probe".into() }
}

fn echo_tool() -> ToolDef {
    ToolDef { name: "echo".into(), description: "Echo the given text.".into(), input_schema: json!({"type": "object", "required": ["text"], "properties": {"text": {"type": "string"}}}) }
}

fn describe(e: &ModelError) -> String {
    match e {
        ModelError::NeedsReauth { .. } => "re-authenticate".into(),
        ModelError::Throttled(m) => format!("throttled: {}", m.chars().take(120).collect::<String>()),
        ModelError::Exhausted(e) => format!("exhausted: {}", e),
        ModelError::Refused(m) => format!("refused: {}", m.chars().take(120).collect::<String>()),
        ModelError::Interrupted(m) => format!("interrupted: {}", m.chars().take(120).collect::<String>()),
        ModelError::Failed(m) => format!("failed: {}", m.chars().take(160).collect::<String>()),
    }
}

/// The cheapest possible "is it back?" check: one capped one-word call (`specs/002` §4.1 re-probe).
pub async fn probe_alive(model: &dyn ModelClient, cfg: &Config, ep: &Endpoint, model_id: &str) -> Result<(), ModelError> {
    let target = Target { provider: delphi_adapter::provider_for(ep).into(), endpoint_id: ep.id.clone(), model_id: model_id.to_string() };
    let plain = CompileOptions { max_tokens: PLAIN_MAX_TOKENS, thinking_budget: None, temperature: None, tools: vec![], betas: ep.betas.clone() };
    let compiled = delphi_adapter::compile_for(ep, &probe_ipr("Reply with exactly the word OK."), &[], &target, &plain).map_err(|e| ModelError::Refused(e.to_string()))?;
    let _ = cfg;
    let mut sink = |_e: delphi_adapter::claude::StreamEvent| {};
    model.call(ep, model_id, &compiled, &mut sink).await.map(|_| ())
}

/// Probe one endpoint × model. Never panics and never leaves the endpoint it was given.
pub async fn probe(model: &dyn ModelClient, cfg: &Config, ep: &Endpoint, model_id: &str, now: i64) -> Capability {
    let mut cap = Capability {
        endpoint_id: ep.id.clone(),
        model_id: model_id.to_string(),
        path: ep.path.as_str().to_string(),
        degraded: ep.path == AccessPath::GatewayCompat,
        probed_at: now,
        auth: "not reached".into(),
        usage_reported: None,
        tool_calling: None,
        reasoning: None,
        signature_round_trip: None,
        cache: "not probed (needs a >1K-token prefix; see specs/009 §6)".into(),
        tokens_used: 0,
        cost_usd: None,
        notes: Vec::new(),
    };
    let target = Target { provider: delphi_adapter::provider_for(ep).into(), endpoint_id: ep.id.clone(), model_id: model_id.to_string() };
    let price = cfg.catalog_entry(model_id).filter(|e| e.has_prices()).map(|e| e.price());
    let mut cost = 0.0;
    let mut note_usage = |cap: &mut Capability, u: &delphi_adapter::claude::Usage| {
        cap.tokens_used += u.input_tokens + u.cache_read_tokens + u.cache_write_tokens + u.output_tokens + u.thinking_tokens;
        if let Some(p) = &price {
            cost += u.to_payload_for("probe", Some(p), 0.0, None, "ok", "reported").cost_usd;
        }
    };
    let mut sink = |_e: delphi_adapter::claude::StreamEvent| {};

    // 1. plain
    let plain = CompileOptions { max_tokens: PLAIN_MAX_TOKENS, thinking_budget: None, temperature: None, tools: vec![], betas: ep.betas.clone() };
    let compiled = match delphi_adapter::compile_for(ep, &probe_ipr("Reply with exactly the word OK."), &[], &target, &plain) {
        Ok(c) => c,
        Err(e) => {
            cap.auth = format!("not probed: {}", e);
            return cap;
        }
    };
    match model.call(ep, model_id, &compiled, &mut sink).await {
        Ok(done) => {
            cap.auth = "ok".into();
            cap.usage_reported = Some(done.usage.input_tokens + done.usage.cache_read_tokens + done.usage.output_tokens > 0);
            note_usage(&mut cap, &done.usage);
        }
        Err(e) => {
            cap.auth = describe(&e);
            return cap;
        }
    }

    // 2. tools + reasoning
    let thinking = if cap.degraded { None } else { cfg.catalog_entry(model_id).filter(|e| e.max_thinking_budget > 0).and_then(|_| cfg.clamp_thinking(model_id, 1)) };
    let max_tokens = TOOL_MAX_TOKENS + thinking.unwrap_or(0) as u64;
    let opts = CompileOptions { max_tokens, thinking_budget: thinking, temperature: None, tools: vec![echo_tool()], betas: ep.betas.clone() };
    let ipr = probe_ipr("Call the `echo` tool exactly once with text \"ping\". Do not answer in prose.");
    let compiled = match delphi_adapter::compile_for(ep, &ipr, &[], &target, &opts) {
        Ok(c) => c,
        Err(e) => {
            cap.notes.push(format!("tool probe not built: {}", e));
            cap.cost_usd = price.map(|_| cost);
            return cap;
        }
    };
    let first = match model.call(ep, model_id, &compiled, &mut sink).await {
        Ok(d) => d,
        Err(e) => {
            cap.tool_calling = Some(false);
            cap.notes.push(format!("tool probe: {}", describe(&e)));
            cap.cost_usd = price.map(|_| cost);
            return cap;
        }
    };
    note_usage(&mut cap, &first.usage);
    let call_id = first.blocks.iter().find_map(|b| if let ContentBlock::ToolCall { id, .. } = b { Some(id.clone()) } else { None });
    cap.tool_calling = Some(call_id.is_some());
    if thinking.is_some() || cap.degraded {
        cap.reasoning = Some(first.blocks.iter().any(|b| b.is_reasoning()));
    }
    let signed = first.blocks.iter().any(|b| b.is_signed_reasoning());

    // 3. signature round trip
    if let (Some(id), true) = (call_id, signed && !cap.degraded) {
        let history = vec![
            Turn::assistant(1, &target.provider, &ep.id, model_id, first.blocks.clone()),
            Turn::user(2, vec![ContentBlock::ToolResult { tool_call_id: id, content: "ping".into(), is_error: false }]),
        ];
        match delphi_adapter::compile_for(ep, &ipr, &history, &target, &opts) {
            Ok(c) => match model.call(ep, model_id, &c, &mut sink).await {
                Ok(done) => {
                    note_usage(&mut cap, &done.usage);
                    cap.signature_round_trip = Some(true);
                }
                Err(e) => {
                    cap.signature_round_trip = Some(false);
                    cap.notes.push(format!("signature round trip: {}", describe(&e)));
                }
            },
            Err(e) => cap.notes.push(format!("round trip not built: {}", e)),
        }
    } else if cap.degraded {
        cap.notes.push("degraded compat endpoint: signatures never cross it".into());
    } else if thinking.is_some() {
        cap.notes.push("no signed reasoning came back, so a round trip could not be tested".into());
    }
    cap.cost_usd = price.map(|_| cost);
    cap
}

/// Probe a subscription harness endpoint: one capped call with no tools, which proves the CLI is installed,
/// signed in and reachable, and records what it reports. Tool calling is not probed here (the Claude
/// approval bridge was verified separately, and agy has none).
pub async fn probe_harness(cfg: &Config, ep: &Endpoint, program: &str, now: i64) -> Capability {
    probe_harness_seat(cfg, ep, program, now, Duration::ZERO).await.0
}

/// [`probe_harness`], and what the CLI reported at start-up (its tools and MCP servers), for `local/harness-tools.json`.
/// A Claude seat whose profile has the Delphi bridge on is started with it, so the probe shows whether an organization's
/// MCP policy lets the bridge load, and which of the account's own connectors came with it.
pub async fn probe_harness_seat(cfg: &Config, ep: &Endpoint, program: &str, now: i64, settle: Duration) -> (Capability, Option<crate::inventory::SeatRecord>) {
    let (cap, seat, _) = probe_harness_full(cfg, ep, program, now, settle, None).await;
    (cap, seat)
}

/// What the conformance probe asks an `agy` model to attempt: one tool call each, so the gate is seen to meet every kind.
const GATE_PROBE_PROMPT: &str = "This is a test of a tool gate; refusals are expected and fine. Do each of the following with one tool call each, in this order. If a call is refused, say so and go straight on to the next one; never retry a refused call. (1) Use the MCP tool named probe from the delphi server. (2) Run the shell command: echo x > conf-shell.txt . (3) Create a file named conf-write.txt containing hi. (4) Change the text 'original' to 'changed' in the file conf-edit.txt. (5) View the file /etc/hosts. (6) List the files in the current directory. (7) Search the web for 'rust language'. (8) Read the page at https://example.com . (9) Open https://example.com in the browser. (10) Start a subagent that says hello. (11) Schedule a reminder for tomorrow. Then reply with exactly the word OK.";

/// [`probe_harness_seat`], and for an `agy` seat the conformance record (`agy_version` is what `agy --version` said, `None` in tests).
pub async fn probe_harness_full(cfg: &Config, ep: &Endpoint, program: &str, now: i64, settle: Duration, agy_version: Option<String>) -> (Capability, Option<crate::inventory::SeatRecord>, Option<GateRecord>) {
    let mut gate_record: Option<GateRecord> = None;
    let mut seat: Option<crate::inventory::SeatRecord> = None;
    let mut cap = Capability {
        endpoint_id: ep.id.clone(),
        model_id: String::new(),
        path: "harness".into(),
        degraded: false,
        probed_at: now,
        auth: "not reached".into(),
        usage_reported: None,
        tool_calling: None,
        reasoning: None,
        signature_round_trip: None,
        cache: "not applicable (subscription)".into(),
        tokens_used: 0,
        cost_usd: None,
        notes: Vec::new(),
    };
    let Some(cli) = ep.harness.as_deref().and_then(Cli::from_harness) else {
        cap.auth = format!("failed: the `{}` harness is not implemented", ep.harness.as_deref().unwrap_or("?"));
        return (cap, None, None);
    };
    // Any tier's model will do for a connectivity probe; the cheapest configured one.
    let model = ["fast", "workhorse", "frontier"].iter().find_map(|t| cfg.resolve_model(ep, t)).unwrap_or_default();
    cap.model_id = model.clone();
    let profile = cfg.harness.get(if cli == Cli::Agy { "agy" } else { "claude" }).cloned().unwrap_or_default();
    let gate_wanted = cli == Cli::Agy && profile.bridge;
    if cli == Cli::Agy && !gate_wanted {
        cap.degraded = true;
        cap.notes.push("agy: the Delphi bridge and gate are off in [harness.agy]; nothing denies its tools, it reads any file, and it is usable for read-only roles only with its output untrusted (L0)".into());
    }
    // One scratch directory per probe: probes can overlap (tests, and anything that probes several endpoints at once), and each
    // removes its directory when it ends.
    static PROBES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("delphi-probe-{}-{}", std::process::id(), PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    if std::fs::create_dir_all(&dir).is_err() {
        cap.auth = "failed: no scratch directory".into();
        return (cap, None, None);
    }
    let connectors = cli == Cli::Claude && profile.claudeai_connectors;
    // `settle`: the probe is the place to learn what loads, so a real probe waits for the account's connectors as a span does.
    let mut lp = crate::harness::LaunchProfile { claudeai_connectors: connectors, settle: if connectors { settle } else { Duration::ZERO }, ..Default::default() };
    let bridge_wanted = cli == Cli::Claude && profile.bridge;
    // Kept alive for the probe; no call is expected, so the receiver is only held.
    let mut _bridge = None;
    if bridge_wanted {
        let stub = vec![ToolDef { name: "probe".into(), description: "Connectivity probe; never call it.".into(), input_schema: json!({"type": "object"}) }];
        match std::env::current_exe() {
            Ok(exe) => match crate::bridge::Bridge::start(stub, String::new(), &exe).await {
                Ok((b, rx)) => {
                    lp.mcp_config = Some(b.config_path().to_path_buf());
                    _bridge = Some((b, rx));
                }
                Err(e) => cap.notes.push(format!("could not start the Delphi tool bridge for the probe: {}", e)),
            },
            Err(e) => cap.notes.push(format!("could not start the Delphi tool bridge for the probe: {}", e)),
        }
    }
    // `agy`: the same stub bridge, behind the gate a span would get (`specs/020` §8). The probe asks the model to call the stub
    // tool and then to try the shell; what happened is the verdict on the gate.
    let (mut agy_calls, mut agy_notes) = (None, None);
    if gate_wanted {
        let stub = vec![ToolDef { name: "probe".into(), description: "Gate probe: call this once.".into(), input_schema: json!({"type": "object"}) }];
        // Nothing of the CLI's own is allowed here, so that every attempt is a denial that can be told from a success. (The
        // profile's web tools are tried too: they are denied in the probe and allowed in a span.)
        let policy = crate::bridge::HookPolicy::new(vec![crate::bridge::AGY_TOOL_PREFIX.to_string()], vec![]);
        let _ = std::fs::write(dir.join("conf-edit.txt"), "original");
        match std::env::current_exe() {
            Ok(exe) => match crate::bridge::Bridge::start_gated(stub, String::new(), &exe, Some(policy)).await {
                Ok((b, rx, notes)) => match b.write_agy_workspace(&dir) {
                    Ok(()) => {
                        lp.mcp_config = Some(b.config_path().to_path_buf());
                        (agy_calls, agy_notes, _bridge) = (Some(rx), Some(notes), Some((b, tokio::sync::mpsc::channel(1).1)));
                    }
                    Err(e) => cap.notes.push(format!("could not prepare the Antigravity workspace for the probe: {}", e)),
                },
                Err(e) => cap.notes.push(format!("could not start the gate for the probe: {}", e)),
            },
            Err(e) => cap.notes.push(format!("could not start the gate for the probe: {}", e)),
        }
    }
    let gate_probe = agy_calls.is_some();
    let user_prompt = if gate_probe { GATE_PROBE_PROMPT } else { "Reply with exactly the word OK." };
    let l = Launch { cli, program: program.into(), model: model.clone(), system_prompt: "This is a connectivity probe. Follow the request exactly.".into(), user_prompt: user_prompt.into(), cwd: dir.clone(), tools: Tools::None, env: crate::harness::cli_env_with(&profile.env_allow), profile: lp, opts: Default::default() };
    let mut served = false;
    match Session::start(&l).await {
        Err(e) => cap.auth = format!("failed: {}", e),
        Ok(mut s) => {
            let mut got_result = false;
            loop {
                let ev = tokio::select! {
                    Some(call) = async { match agy_calls.as_mut() { Some(rx) => rx.recv().await, None => std::future::pending().await } } => {
                        served = true;
                        let _ = call.reply.send(crate::tools::ToolResult { content: "probe ok".into(), is_error: false });
                        continue;
                    }
                    ev = s.next(Duration::from_secs(90)) => match ev { Some(ev) => ev, None => break },
                };
                match ev {
                    Event::Init { tools, mcp_servers, .. } => {
                        if let (true, Some(servers)) = (_bridge.is_some(), &mcp_servers) {
                            let state = servers.iter().find(|(n, _)| n == crate::bridge::SERVER_NAME).map(|(_, st)| st.as_str());
                            if matches!(state, Some("connected" | "pending")) {
                                cap.notes.push("Delphi tool bridge: loaded, so roles with tools can run on this seat".into());
                            } else {
                                cap.degraded = true;
                                cap.notes.push(format!("Delphi tool bridge: NOT loaded (status: {}). An organization policy that allows only managed MCP servers would cause this; roles that need tools cannot run on this seat until the `delphi` MCP server is allowed", state.unwrap_or("absent")));
                            }
                        }
                        if let Some(servers) = &mcp_servers {
                            let others: Vec<String> = servers.iter().filter(|(n, _)| n != crate::bridge::SERVER_NAME).map(|(n, st)| format!("{} ({})", n, st)).collect();
                            if !others.is_empty() {
                                cap.notes.push(format!("account connectors loaded: {}", others.join(", ")));
                            }
                        }
                        seat = Some(crate::inventory::SeatRecord { endpoint_id: ep.id.clone(), harness: if cli == Cli::Agy { "agy" } else { "claude" }.into(), model: model.clone(), tools, mcp_servers: mcp_servers.unwrap_or_default(), bridge: _bridge.is_some(), seen_unix: now });
                    }
                    Event::Usage(t) => cap.tokens_used += t.metered(),
                    Event::RateLimit { utilization, window, .. } => cap.notes.push(format!("{} window {:.0}% used", window.replace('_', "-"), utilization * 100.0)),
                    Event::Result { is_error, text, usage, .. } => {
                        got_result = true;
                        cap.usage_reported = Some(usage.metered() + usage.cache_read > 0);
                        cap.tokens_used = cap.tokens_used.max(usage.metered());
                        cap.auth = if is_error { format!("failed: {}", text.chars().take(120).collect::<String>()) } else { "ok".into() };
                        s.close_input();
                    }
                    Event::Exit { code } => {
                        if !got_result {
                            cap.auth = format!("failed: {} exited ({:?}) without a result; is it signed in?", program, code);
                        }
                        break;
                    }
                    _ => {}
                }
            }
            s.kill().await;
        }
    }
    if gate_probe {
        let decisions: Vec<crate::bridge::HookNote> = agy_notes.as_mut().map(|rx| std::iter::from_fn(|| rx.try_recv().ok()).collect()).unwrap_or_default();
        let mut attempted: Vec<String> = decisions.iter().map(|n| n.tool.clone()).filter(|t| !t.starts_with(crate::bridge::AGY_TOOL_PREFIX)).collect();
        attempted.sort();
        attempted.dedup();
        let mut kinds: Vec<String> = attempted.iter().map(|t| tool_kind(t).to_string()).collect();
        kinds.sort();
        kinds.dedup();
        // Anything that happened although the gate should have stopped it: a file written or changed, or one nobody asked for.
        let mut side_effects: Vec<String> = Vec::new();
        for f in ["conf-shell.txt", "conf-write.txt", "gate-probe.txt"] {
            if dir.join(f).exists() {
                side_effects.push(format!("{} was created", f));
            }
        }
        if std::fs::read_to_string(dir.join("conf-edit.txt")).ok().as_deref() != Some("original") {
            side_effects.push("conf-edit.txt was changed".into());
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if !matches!(n.as_str(), ".agents" | "conf-edit.txt" | "conf-shell.txt" | "conf-write.txt" | "gate-probe.txt") {
                    side_effects.push(format!("{} appeared", n));
                }
            }
        }
        let shell_seen = kinds.iter().any(|k| k == "shell");
        let verified = served && side_effects.is_empty() && shell_seen && kinds.len() >= GATE_MIN_KINDS;
        let version = agy_version.clone().unwrap_or_default();
        gate_record = Some(GateRecord { endpoint_id: ep.id.clone(), agy_version: version.clone(), at: now, attempted: attempted.clone(), kinds: kinds.clone(), served, side_effects: side_effects.clone(), verified });
        let seen = if kinds.is_empty() { "nothing".to_string() } else { format!("{} kind(s) of tool ({})", kinds.len(), kinds.join(", ")) };
        if !side_effects.is_empty() {
            cap.degraded = true;
            cap.auth = "failed: the agy gate did not stop a tool".into();
            cap.notes.push(format!("agy gate: FAILED. Something ran although the gate should have denied it ({}). Do not use this seat until it is understood; its spans stay untrusted, but nothing else is checked", side_effects.join("; ")));
        } else if verified {
            cap.notes.push(format!("agy gate: verified{}. Delphi's tool was served and the gate met and denied {}; none ran, so a span on this agy version starts untrusted only if it reads from the web", if version.is_empty() { String::new() } else { format!(" on {}", version) }, seen));
        } else if served {
            cap.notes.push(format!("agy gate: unproven. Delphi's tool was served and nothing got through, but the model reached for only {}; the gate needs a shell and {} kinds of tool denied to count as verified, so the span stays untrusted. Run the probe again", seen, GATE_MIN_KINDS));
        } else {
            cap.degraded = true;
            cap.notes.push("agy gate: Delphi's tool was not served. The workspace plugin did not load, or this agy does not honor a workspace-local .agents/ any more, or the model did not call it; roles that need tools cannot run on this seat".into());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    cap.notes.dedup();
    (cap, seat, gate_record)
}

pub fn load(path: &Path) -> Vec<Capability> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Replace the entries for the probed endpoint × model pairs; keep the rest.
pub fn save(path: &Path, fresh: &[Capability]) -> std::io::Result<()> {
    let mut all: Vec<Capability> = load(path).into_iter().filter(|old| !fresh.iter().any(|n| n.endpoint_id == old.endpoint_id && n.model_id == old.model_id)).collect();
    all.extend(fresh.iter().cloned());
    all.sort_by(|a, b| (&a.endpoint_id, &a.model_id).cmp(&(&b.endpoint_id, &b.model_id)));
    delphi_core::lock::write_atomic(path, serde_json::to_string_pretty(&all).expect("serializes").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ScriptedModel, Step};
    use delphi_adapter::blocks::ProviderMeta;
    use delphi_adapter::claude::Usage;

    const CFG: &str = r#"
[models.claude]
workhorse = "sonnet-x"
[models.catalog."sonnet-x"]
max_output_tokens = 64000
min_thinking_budget = 1024
max_thinking_budget = 32000
input_usd_per_mtok = 3.0
output_usd_per_mtok = 15.0
[[endpoints]]
id = "direct"
family = "claude"
path = "api"
credential = "env:K"
[[endpoints]]
id = "gw"
family = "claude"
path = "gateway-compat"
base_url = "https://gw.example.internal"
credential = "env:K"
"#;

    fn signed_call() -> Step {
        Step::Reply {
            blocks: vec![
                ContentBlock::Thinking { thought: "plan".into(), replayable: true, signature_dropped_reason: None, provider_meta: ProviderMeta { provider: Some("anthropic".into()), signature: Some("SIG".into()), data: None } },
                ContentBlock::ToolCall { id: "toolu_1".into(), name: "echo".into(), arguments: json!({"text": "ping"}) },
            ],
            usage: Usage { input_tokens: 300, cache_read_tokens: 0, cache_write_tokens: 0, output_tokens: 100, thinking_tokens: 0 },
            stop_reason: "tool_use",
        }
    }

    #[tokio::test]
    async fn a_native_endpoint_is_probed_end_to_end_within_its_token_caps() {
        let cfg = Config::parse(CFG).unwrap();
        let ep = cfg.endpoint("direct").unwrap().clone();
        let model = ScriptedModel::new(vec![Step::text("OK"), signed_call(), Step::text("done")]);
        let cap = probe(&*model, &cfg, &ep, "sonnet-x", 1000).await;
        assert_eq!(cap.auth, "ok");
        assert_eq!((cap.usage_reported, cap.tool_calling, cap.reasoning, cap.signature_round_trip), (Some(true), Some(true), Some(true), Some(true)));
        assert!(!cap.degraded && cap.cost_usd.unwrap() > 0.0 && cap.tokens_used > 1000);
        let seen = model.requests();
        assert_eq!(seen.len(), 3);
        assert!(seen[0].body["max_tokens"].as_u64().unwrap() <= 64, "the plain probe is capped");
        assert_eq!(seen[1].body["thinking"]["budget_tokens"], 1024, "the model's minimum budget, nothing more");
        assert!(seen[1].body["max_tokens"].as_u64().unwrap() <= 1024 + 512);
        let replay = seen[2].body["messages"][1]["content"].to_string();
        assert!(replay.contains("\"signature\":\"SIG\""), "the round trip really replays the signature: {}", replay);
    }

    #[tokio::test]
    async fn a_failed_signature_round_trip_is_recorded_not_hidden() {
        let cfg = Config::parse(CFG).unwrap();
        let ep = cfg.endpoint("direct").unwrap().clone();
        let model = ScriptedModel::new(vec![Step::text("OK"), signed_call(), Step::Fail("invalid signature")]);
        let cap = probe(&*model, &cfg, &ep, "sonnet-x", 0).await;
        assert_eq!((cap.tool_calling, cap.signature_round_trip), (Some(true), Some(false)));
        assert!(cap.notes.iter().any(|n| n.contains("invalid signature")));
    }

    #[tokio::test]
    async fn authentication_failures_stop_the_suite_after_one_request() {
        let cfg = Config::parse(CFG).unwrap();
        let ep = cfg.endpoint("direct").unwrap().clone();
        for (step, want) in [(Step::NeedsReauth, "re-authenticate"), (Step::Throttle, "throttled"), (Step::Fail("401"), "failed")] {
            let model = ScriptedModel::new(vec![step]);
            let cap = probe(&*model, &cfg, &ep, "sonnet-x", 0).await;
            assert!(cap.auth.starts_with(want), "{}", cap.auth);
            assert_eq!((model.requests().len(), cap.tool_calling), (1, None));
        }
    }

    #[tokio::test]
    async fn a_compat_gateway_is_marked_degraded_and_skips_the_round_trip() {
        let cfg = Config::parse(CFG).unwrap();
        let ep = cfg.endpoint("gw").unwrap().clone();
        let call = Step::tool("call_1", "echo", json!({"text": "ping"}));
        let model = ScriptedModel::new(vec![Step::text("OK"), call]);
        let cap = probe(&*model, &cfg, &ep, "sonnet-x", 0).await;
        assert!(cap.degraded);
        assert_eq!((cap.tool_calling, cap.signature_round_trip), (Some(true), None));
        assert!(cap.notes.iter().any(|n| n.contains("degraded")));
        assert_eq!(model.requests().len(), 2);
        assert!(model.requests()[1].body.get("thinking").is_none());
    }

    const HARNESS_CFG: &str = r#"
[models.claude]
fast = "haiku-x"
[[endpoints]]
id = "sub"
family = "claude"
path = "harness"
harness = "claude"
credential = "harness-login"
cost_model = "subscription"
"#;

    #[tokio::test]
    async fn a_harness_probe_proves_the_cli_is_signed_in_and_reports_the_window() {
        let cfg = Config::parse(HARNESS_CFG).unwrap();
        let ep = cfg.endpoint("sub").unwrap().clone();
        let d = tempfile::tempdir().unwrap();
        let good = crate::harness::tests::fake_cli(d.path(), r#"echo '{"type":"system","subtype":"init","session_id":"s"}'
echo '{"type":"rate_limit_event","rate_limit_info":{"utilization":0.32,"rateLimitType":"seven_day"}}'
echo '{"type":"result","subtype":"success","result":"OK","usage":{"input_tokens":5,"output_tokens":3}}'"#);
        let cap = probe_harness(&cfg, &ep, &good, 7).await;
        assert_eq!((cap.auth.as_str(), cap.usage_reported, cap.model_id.as_str()), ("ok", Some(true), "haiku-x"));
        assert!(cap.notes.iter().any(|n| n.contains("seven-day window 32% used")), "{:?}", cap.notes);
        let d2 = tempfile::tempdir().unwrap();
        let bad = crate::harness::tests::fake_cli(d2.path(), "exit 1");
        let cap = probe_harness(&cfg, &ep, &bad, 7).await;
        assert!(cap.auth.starts_with("failed") && cap.auth.contains("signed in"), "{}", cap.auth);
        let cap = probe_harness(&cfg, &ep, "/nonexistent/claude", 7).await;
        assert!(cap.auth.starts_with("failed: cannot start"), "{}", cap.auth);
    }

    #[tokio::test]
    async fn the_probe_says_whether_the_delphi_bridge_loaded_and_which_account_connectors_came_with_it() {
        let cfg = Config::parse(HARNESS_CFG).unwrap();
        let ep = cfg.endpoint("sub").unwrap().clone();
        let run = |servers: &str| {
            let d = tempfile::tempdir().unwrap();
            let body = format!("echo '{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s\",\"tools\":[\"mcp__delphi__probe\"],\"mcp_servers\":[{}]}}'\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"OK\",\"usage\":{{\"input_tokens\":5,\"output_tokens\":3}}}}'", servers);
            let prog = crate::harness::tests::fake_cli(d.path(), &body);
            (d, prog)
        };
        let (_d, prog) = run(r#"{"name":"delphi","status":"connected"},{"name":"claude.ai Slack","status":"pending"}"#);
        let (cap, seat) = probe_harness_seat(&cfg, &ep, &prog, 7, Duration::ZERO).await;
        assert!(!cap.degraded && cap.notes.iter().any(|n| n.contains("Delphi tool bridge: loaded")) && cap.notes.iter().any(|n| n.contains("account connectors loaded: claude.ai Slack (pending)")), "{:?}", cap.notes);
        let seat = seat.expect("the seat's start-up report is returned");
        assert_eq!((seat.endpoint_id.as_str(), seat.bridge, seat.tools.as_slice()), ("sub", true, &["mcp__delphi__probe".to_string()][..]));
        // Policy keeps the bridge out: the seat is marked degraded and the note says what to do.
        let (_d2, prog) = run(r#"{"name":"delphi","status":"failed"}"#);
        let (cap, _) = probe_harness_seat(&cfg, &ep, &prog, 7, Duration::ZERO).await;
        assert!(cap.degraded && cap.notes.iter().any(|n| n.contains("NOT loaded (status: failed)") && n.contains("organization policy")), "{:?}", cap.notes);
        // A profile with the bridge off does not look for it.
        let off = Config::parse(&format!("{}\n[harness.claude]\nbridge = false\n", HARNESS_CFG)).unwrap();
        let (_d3, prog) = run(r#"{"name":"delphi","status":"failed"}"#);
        let (cap, seat) = probe_harness_seat(&off, &ep, &prog, 7, Duration::ZERO).await;
        assert!(!cap.degraded && !cap.notes.iter().any(|n| n.contains("bridge")), "{:?}", cap.notes);
        assert!(!seat.unwrap().bridge);
    }

    const AGY_CFG: &str = r#"
[models.gemini]
fast = "gemini-fast"
[[endpoints]]
id = "ag"
family = "gemini"
path = "harness"
harness = "agy"
credential = "harness-login"
cost_model = "subscription"
"#;

    /// A stand-in `agy` that does what the real one does inside the probe: finds the plugin and the gate in `.agents/`, asks the
    /// gate about each tool in `attempts` the way the hook does, calls the stub tool through the plugin's server, and obeys any
    /// tool the gate did not deny (as a CLI whose gate failed would). `leak` names a file to write regardless, `edit` changes
    /// the seeded file regardless.
    fn agy_probe_cli(dir: &std::path::Path, attempts: &[&str], call_tool: bool, leak: Option<&str>, edit: bool) -> String {
        let client = format!(
            r#"import json, socket
cfg = json.load(open(".agents/plugins/delphi/mcp_config.json"))["mcpServers"]["delphi"]["env"]
s = socket.socket(socket.AF_UNIX); s.connect(cfg["DELPHI_BRIDGE_SOCKET"])
f = s.makefile("rw"); f.write(json.dumps({{"token": cfg["DELPHI_BRIDGE_TOKEN"]}}) + "\n"); f.flush()
def rpc(o):
    f.write(json.dumps(o) + "\n"); f.flush(); return json.loads(f.readline())
for name in {attempts}:
    d = rpc({{"jsonrpc": "2.0", "id": 1, "method": "delphi/hook", "params": {{"toolCall": {{"name": name, "args": {{"CommandLine": "echo x"}}}}}}}})["result"]["decision"]
    if d == "allow":
        open("conf-shell.txt", "w").write("x")
if {leak}:
    open({leak_name}, "w").write("x")
if {edit}:
    open("conf-edit.txt", "w").write("changed")
if {call_tool}:
    rpc({{"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {{"protocolVersion": "2025-11-25"}}}})
    rpc({{"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {{"name": "probe", "arguments": {{}}}}}})
"#,
            attempts = serde_json::to_string(attempts).unwrap(),
            call_tool = if call_tool { "True" } else { "False" },
            leak = if leak.is_some() { "True" } else { "False" },
            leak_name = serde_json::to_string(leak.unwrap_or("none")).unwrap(),
            edit = if edit { "True" } else { "False" }
        );
        std::fs::write(dir.join("client.py"), client).unwrap();
        crate::harness::tests::fake_cli(dir, &format!("python3 -I '{}'\necho '{{\"event\":\"init\",\"init\":{{\"model\":\"m\",\"tools\":[\"run_command\"]}}}}'\necho '{{\"event\":\"result\",\"result\":{{\"status\":\"SUCCESS\",\"response\":\"OK\",\"usage\":{{\"input_tokens\":5,\"output_tokens\":3}}}}}}'", dir.join("client.py").display()))
    }

    const EVERY_KIND: [&str; 8] = ["run_command", "read_file", "write_file", "replace_file_content", "search_web", "read_url_content", "browser_subagent", "invoke_subagent"];

    #[test]
    fn a_tool_is_classed_by_what_it_can_do_not_by_what_it_is_called_here() {
        for (tool, kind) in [("run_command", "shell"), ("send_command_input", "shell"), ("read_file", "read"), ("list_dir", "read"), ("write_file", "write"), ("replace_file_content", "edit"), ("search_web", "web search"), ("read_url_content", "web read"), ("browser_click_element", "browser"), ("open_browser_url", "browser"), ("invoke_subagent", "subagent"), ("schedule", "schedule"), ("generate_image", "image"), ("call_mcp_tool", "mcp"), ("something_new", "other")] {
            assert_eq!(tool_kind(tool), kind, "{}", tool);
        }
    }

    #[tokio::test]
    async fn the_probe_judges_the_agy_gate_by_what_happened_not_by_what_was_promised() {
        if !std::process::Command::new("python3").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
            return;
        }
        let cfg = Config::parse(AGY_CFG).unwrap();
        let ep = cfg.endpoint("ag").unwrap().clone();
        let run = |attempts: &'static [&'static str], call_tool: bool, leak: Option<&'static str>, edit: bool| {
            let (cfg, ep) = (cfg.clone(), ep.clone());
            async move {
                let d = tempfile::tempdir().unwrap();
                let prog = agy_probe_cli(d.path(), attempts, call_tool, leak, edit);
                probe_harness_full(&cfg, &ep, &prog, 7, Duration::ZERO, Some("agy 1.4.7".into())).await
            }
        };
        // Works: the tool is served and the gate meets and denies every kind of tool; nothing runs.
        let (cap, seat, gate) = run(&EVERY_KIND, true, None, false).await;
        assert!(!cap.degraded && cap.auth == "ok" && cap.notes.iter().any(|n| n.contains("agy gate: verified on agy 1.4.7") && n.contains("8 kind(s)")), "{:?}", cap.notes);
        assert!(seat.unwrap().bridge, "the seat is recorded as carrying the bridge");
        let g = gate.unwrap();
        assert!(g.verified && g.served && g.side_effects.is_empty() && g.agy_version == "agy 1.4.7" && g.at == 7 && g.endpoint_id == "ag", "{:?}", g);
        assert_eq!(g.kinds, ["browser", "edit", "read", "shell", "subagent", "web read", "web search", "write"]);
        // A file written although the gate was asked: the worst case, and it fails the seat.
        let (cap, _, gate) = run(&EVERY_KIND, true, Some("conf-write.txt"), false).await;
        assert!(cap.degraded && cap.auth.starts_with("failed: the agy gate did not stop") && cap.notes.iter().any(|n| n.contains("agy gate: FAILED") && n.contains("conf-write.txt was created")), "{} {:?}", cap.auth, cap.notes);
        assert!(!gate.unwrap().verified);
        // A file changed although the gate was asked.
        let (cap, _, gate) = run(&EVERY_KIND, true, None, true).await;
        assert!(cap.degraded && cap.notes.iter().any(|n| n.contains("conf-edit.txt was changed")), "{:?}", cap.notes);
        assert!(!gate.unwrap().verified);
        // Something nobody asked for appeared: also a failure.
        let (cap, _, _) = run(&EVERY_KIND, true, Some("surprise.sh"), false).await;
        assert!(cap.degraded && cap.notes.iter().any(|n| n.contains("surprise.sh appeared")), "{:?}", cap.notes);
        // The model reached for too few kinds of tool: nothing got through, but that is not proof, and the note says so.
        let (cap, _, gate) = run(&["run_command", "read_file"], true, None, false).await;
        assert!(!cap.degraded && cap.notes.iter().any(|n| n.contains("agy gate: unproven") && n.contains("2 kind(s)")), "{:?}", cap.notes);
        assert!(!gate.unwrap().verified, "two kinds are not enough");
        // Without a shell among them it is not verified either, however many other kinds were tried.
        let (_, _, gate) = run(&["read_file", "write_file", "replace_file_content", "search_web", "browser_subagent", "invoke_subagent"], true, None, false).await;
        assert!(!gate.unwrap().verified, "the shell is the one that must have been met");
        // Delphi's tool was never served: the plugin did not load.
        let (cap, _, gate) = run(&EVERY_KIND, false, None, false).await;
        assert!(cap.degraded && cap.notes.iter().any(|n| n.contains("agy gate: Delphi's tool was not served")), "{:?}", cap.notes);
        assert!(!gate.unwrap().verified);
        // With the bridge off nothing is gated, and the note says that instead; there is no record to keep.
        let off = Config::parse(&format!("{}\n[harness.agy]\nbridge = false\n", AGY_CFG)).unwrap();
        let d = tempfile::tempdir().unwrap();
        let prog = crate::harness::tests::fake_cli(d.path(), "echo '{\"event\":\"result\",\"result\":{\"status\":\"SUCCESS\",\"response\":\"OK\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}'");
        let (cap, _, gate) = probe_harness_full(&off, &ep, &prog, 7, Duration::ZERO, None).await;
        assert!(cap.degraded && cap.notes.iter().any(|n| n.contains("the Delphi bridge and gate are off")) && !cap.notes.iter().any(|n| n.contains("agy gate")), "{:?}", cap.notes);
        assert!(gate.is_none());
    }

    #[test]
    fn the_gate_record_is_kept_per_endpoint_tied_to_a_version_and_goes_stale() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("local/agy-gate.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let rec = |ep: &str, v: &str, at: i64, ok: bool| GateRecord { endpoint_id: ep.into(), agy_version: v.into(), at, attempted: vec!["run_command".into()], kinds: vec!["shell".into()], served: true, side_effects: vec![], verified: ok };
        save_gate(&path, &rec("a", "1.4.7", 100, true)).unwrap();
        save_gate(&path, &rec("b", "1.4.7", 100, false)).unwrap();
        save_gate(&path, &rec("a", "1.4.8", 200, true)).unwrap();
        let all = load_gates(&path);
        assert_eq!(all.iter().map(|g| (g.endpoint_id.as_str(), g.agy_version.as_str(), g.at)).collect::<Vec<_>>(), [("a", "1.4.8", 200), ("b", "1.4.7", 100)], "a new probe replaces the endpoint's old record");
        assert!(all[0].is_fresh(200 + GATE_TTL_SECS - 1) && !all[0].is_fresh(200 + GATE_TTL_SECS));
        assert!(load_gates(&d.path().join("missing.json")).is_empty());
        std::fs::write(&path, "not json").unwrap();
        assert!(load_gates(&path).is_empty(), "a damaged file is no evidence");
    }

    #[test]
    fn results_are_merged_by_endpoint_and_model_and_expire_after_a_day() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("local/endpoint-capabilities.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mk = |ep: &str, model: &str, at: i64| Capability { endpoint_id: ep.into(), model_id: model.into(), path: "api".into(), degraded: false, probed_at: at, auth: "ok".into(), usage_reported: Some(true), tool_calling: Some(true), reasoning: None, signature_round_trip: None, cache: "not probed".into(), tokens_used: 1, cost_usd: None, notes: vec![] };
        save(&p, &[mk("a", "m1", 0), mk("b", "m1", 0)]).unwrap();
        save(&p, &[mk("a", "m1", 500)]).unwrap();
        let all = load(&p);
        assert_eq!(all.iter().map(|c| (c.endpoint_id.as_str(), c.probed_at)).collect::<Vec<_>>(), [("a", 500), ("b", 0)]);
        assert!(all[0].is_fresh(500 + TTL_SECS - 1) && !all[0].is_fresh(500 + TTL_SECS));
    }
}
