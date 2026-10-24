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

use crate::harness::{cli_env, Cli, Event, Launch, Session, Tools};
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

fn probe_ipr(contract: &str) -> Ipr {
    Ipr { governance: "This is a capability probe. Follow the contract exactly.".into(), role: "You are a connectivity probe.".into(), persona: None, heuristics: "None.".into(), lessons: "None.".into(), context_slices: vec![], contract: contract.into(), reminder: None, nonce: "probe".into() }
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
        return cap;
    };
    // Any tier's model will do for a connectivity probe; the cheapest configured one.
    let model = ["fast", "workhorse", "frontier"].iter().find_map(|t| cfg.resolve_model(ep, t)).unwrap_or_default();
    cap.model_id = model.clone();
    if cli == Cli::Agy {
        cap.degraded = true;
        cap.notes.push("agy: no approval bridge, tools cannot be turned off, reads any file; usable for read-only roles only, and its output is untrusted (L0)".into());
    }
    let dir = std::env::temp_dir().join(format!("delphi-probe-{}", std::process::id()));
    if std::fs::create_dir_all(&dir).is_err() {
        cap.auth = "failed: no scratch directory".into();
        return cap;
    }
    let l = Launch { cli, program: program.into(), model, system_prompt: "This is a connectivity probe. Follow the request exactly.".into(), user_prompt: "Reply with exactly the word OK.".into(), cwd: dir.clone(), tools: Tools::None, env: cli_env() };
    match Session::start(&l).await {
        Err(e) => cap.auth = format!("failed: {}", e),
        Ok(mut s) => {
            let mut got_result = false;
            while let Some(ev) = s.next(Duration::from_secs(90)).await {
                match ev {
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
    let _ = std::fs::remove_dir_all(&dir);
    cap.notes.dedup();
    cap
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
