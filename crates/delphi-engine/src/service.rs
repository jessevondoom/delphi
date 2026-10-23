//! The engine's RPC surface, mounted on the daemon as an [`Extension`].
//!
//! Reading is open to both scopes; anything that changes state (`task.submit`, `approval.resolve`,
//! `budget.set`, `permissions.set`, `ratify.*`) needs the `console` scope. The server has already
//! authenticated the connection; this layer decides what each scope may do.

use crate::approval::{Answer, Mode, ResolveError};
use crate::engine::{Engine, RecoveryPlan};
use crate::mission::{EngineError, MissionState};
use delphi_core::event::EventEnvelope;
use delphi_core::ledger::GroupBy;
use delphi_core::proto::{code, RpcError, Scope};
use delphi_core::server::{CallContext, Extension, RpcFuture, RpcOutcome};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Running {
    target: String,
    prompt: String,
    started: Instant,
}

pub struct EngineService {
    engine: Arc<Engine>,
    default_mission: String,
    missions: Mutex<HashMap<String, Arc<MissionState>>>,
    running: Arc<Mutex<HashMap<String, Running>>>,
    notices: Mutex<Vec<String>>,
    /// Serializes opening a mission (and planning its recovery).
    open_gate: tokio::sync::Mutex<()>,
    /// Recovery plans awaiting the operator's decision, by mission.
    recovery: Mutex<HashMap<String, PlanState>>,
}

struct PlanState {
    plan: Arc<RecoveryPlan>,
    resumed: std::collections::HashSet<String>,
}

fn err(code: i64, message: impl Into<String>) -> RpcError {
    RpcError { code, message: message.into(), data: None }
}

fn need_console(ctx: &CallContext) -> Result<(), RpcError> {
    if ctx.scope == Scope::Console {
        Ok(())
    } else {
        Err(err(code::FORBIDDEN, "this method requires the console scope"))
    }
}

fn result_msg(id: &str) -> String {
    format!("`{}` changed after you reviewed it; review it again", id)
}

fn valid_mission_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !id.starts_with('-')
}

fn engine_err(e: EngineError) -> RpcError {
    err(-32000, e.to_string())
}

impl EngineService {
    pub fn new(engine: Arc<Engine>, default_mission: &str) -> Arc<EngineService> {
        Arc::new(EngineService { engine, default_mission: default_mission.to_string(), missions: Mutex::new(HashMap::new()), running: Arc::new(Mutex::new(HashMap::new())), notices: Mutex::new(Vec::new()), open_gate: tokio::sync::Mutex::new(()), recovery: Mutex::new(HashMap::new()) })
    }

    pub fn set_notices(&self, n: Vec<String>) {
        *self.notices.lock().unwrap() = n;
    }

    /// Register a mission that was opened elsewhere (tests, recovery).
    pub fn adopt(&self, m: Arc<MissionState>) {
        self.missions.lock().unwrap().insert(m.id.clone(), m);
    }

    /// Open (or return the already open) mission.
    pub async fn mission(&self, id: Option<&str>) -> Result<Arc<MissionState>, RpcError> {
        let id = id.unwrap_or(&self.default_mission);
        if !valid_mission_id(id) {
            return Err(err(code::INVALID_PARAMS, "mission ids are lowercase letters, digits and `-`"));
        }
        let _gate = self.open_gate.lock().await;
        if let Some(m) = self.missions.lock().unwrap().get(id) {
            return Ok(m.clone());
        }
        let e = &self.engine;
        let m = MissionState::open(&e.ws.dir.join("local/missions").join(id), id, e.hub.clone(), e.ledger.clone(), e.secrets.clone(), e.settings.mission_budget_usd).map_err(engine_err)?;
        // A mission with unfinished work is planned for recovery now; nothing resumes until the operator says so.
        if let Some(plan) = self.engine.recover_mission(&m).await.map_err(engine_err)? {
            self.recovery.lock().unwrap().insert(id.to_string(), PlanState { plan: Arc::new(plan), resumed: Default::default() });
        }
        self.missions.lock().unwrap().insert(id.to_string(), m.clone());
        Ok(m)
    }

    /// Tasks of a recovery plan not yet resumed, as JSON for the console.
    fn recovery_json(&self, mission: &str) -> Value {
        let g = self.recovery.lock().unwrap();
        match g.get(mission) {
            Some(st) => {
                let pending: Vec<Value> = st.plan.items.iter().filter(|i| !st.resumed.contains(&i.task_id)).map(|i| json!({"task_id": i.task_id, "role": i.role, "action": i.action})).collect();
                json!({"tasks": pending, "charged_usd": st.plan.charged_usd, "expired_approvals": st.plan.expired_approvals})
            }
            None => json!({"tasks": [], "charged_usd": 0.0, "expired_approvals": []}),
        }
    }

    /// `recovery.resume`: continue the chosen tasks (default: all) in the background.
    async fn recovery_resume(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let wanted: Option<Vec<String>> = p["tasks"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect());
        let (rec, ids) = {
            let mut g = self.recovery.lock().unwrap();
            let Some(st) = g.get_mut(&m.id) else { return Ok(json!({"resumed": []})) };
            let ids: Vec<String> = st.plan.items.iter().map(|i| i.task_id.clone()).filter(|t| !st.resumed.contains(t) && wanted.as_ref().map(|w| w.contains(t)).unwrap_or(true)).collect();
            st.resumed.extend(ids.iter().cloned());
            (st.plan.rec.clone(), ids)
        };
        for tid in &ids {
            let (engine, running, m2, rec2, tid2) = (self.engine.clone(), self.running.clone(), m.clone(), rec.clone(), tid.clone());
            let target = rec.tasks.get(tid).map(|t| t.target.clone()).unwrap_or_default();
            let prompt = rec.tasks.get(tid).map(|t| t.prompt.chars().take(120).collect::<String>()).unwrap_or_default();
            running.lock().unwrap().insert(tid.clone(), Running { target, prompt, started: Instant::now() });
            tokio::spawn(async move {
                if let Err(e) = engine.resume_task(&m2, &rec2, &tid2).await {
                    let mut ev = EventEnvelope::new(&m2.id, "task.failed", json!({"error": engine.secrets.redact(&e.to_string())}));
                    ev.task_id = Some(tid2.clone());
                    let _ = m2.append(ev).await;
                }
                running.lock().unwrap().remove(&tid2);
            });
        }
        Ok(json!({"resumed": ids}))
    }

    fn budget_json(m: &MissionState) -> Value {
        let b = m.budget.lock().unwrap();
        json!({"cap_usd": b.cap_micro() as f64 / 1e6, "spent_usd": b.spent() as f64 / 1e6, "reserved_usd": b.reserved() as f64 / 1e6, "available_usd": b.available() as f64 / 1e6})
    }

    fn pending_json(&self) -> Value {
        Value::Array(self.engine.broker.pending().iter().map(|r| r.event_payload()).collect())
    }

    async fn task_submit(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let target = p["target"].as_str().unwrap_or("orchestrator").to_string();
        let prompt = p["prompt"].as_str().unwrap_or("").trim().to_string();
        if prompt.is_empty() || prompt.chars().count() > 20_000 {
            return Err(err(code::INVALID_PARAMS, "`prompt` must be 1 to 20,000 characters"));
        }
        if !self.engine.settings.enabled_agents.contains(&target) {
            return Err(err(code::INVALID_PARAMS, format!("`{}` is not an enabled role (enabled: {})", target, self.engine.settings.enabled_agents.join(", "))));
        }
        let task_id = m.next_task_id();
        self.running.lock().unwrap().insert(task_id.clone(), Running { target: target.clone(), prompt: prompt.chars().take(120).collect(), started: Instant::now() });
        let (engine, running, tid, mid) = (self.engine.clone(), self.running.clone(), task_id.clone(), m.id.clone());
        tokio::spawn(async move {
            if let Err(e) = engine.run_task_with_id(&m, tid.clone(), &target, &prompt).await {
                let mut ev = EventEnvelope::new(&m.id, "task.failed", json!({"error": engine.secrets.redact(&e.to_string())}));
                ev.task_id = Some(tid.clone());
                let _ = m.append(ev).await;
            }
            running.lock().unwrap().remove(&tid);
        });
        Ok(json!({"task_id": task_id, "mission_id": mid}))
    }

    fn approval_resolve(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let (Some(id), Some(hash)) = (p["approval_id"].as_str(), p["payload_blake3"].as_str()) else {
            return Err(err(code::INVALID_PARAMS, "`approval_id` and `payload_blake3` are required"));
        };
        let answer = match p["decision"].as_str() {
            Some("allow") => Answer::Allow,
            Some("allow_similar") => Answer::AllowSimilar,
            Some("deny") => Answer::Deny { reason: p["reason"].as_str().unwrap_or("").chars().take(500).collect() },
            _ => return Err(err(code::INVALID_PARAMS, "`decision` must be allow, allow_similar or deny")),
        };
        match self.engine.broker.resolve(id, hash, answer, &format!("console-{}", ctx.client_id)) {
            Ok(r) => Ok(r.event_payload(id, &format!("console-{}", ctx.client_id))),
            Err(ResolveError::Gone { resolution, by }) => Err(RpcError { code: code::NOT_FOUND, message: format!("{}", ResolveError::Gone { resolution: resolution.clone(), by: by.clone() }), data: Some(json!({"by": by, "resolution": resolution.map(|r| r.event_payload(id, "")) })) }),
            Err(e) => Err(err(code::INVALID_PARAMS, e.to_string())),
        }
    }

    async fn usage_get(&self, p: Value) -> RpcOutcome {
        let group = match p["group"].as_str().unwrap_or("role") {
            "mission" => GroupBy::Mission,
            "role" => GroupBy::Role,
            "task" => GroupBy::Task,
            "model" => GroupBy::Model,
            other => return Err(err(code::INVALID_PARAMS, format!("unknown group `{}` (mission, role, task or model)", other))),
        };
        let m = self.mission(p["mission_id"].as_str()).await?;
        let rows = self.engine.ledger.lock().unwrap().totals(group, Some(&m.id)).map_err(|e| err(-32000, e.to_string()))?;
        Ok(json!({
            "mission_id": m.id,
            "rows": rows.iter().map(|t| json!({"key": t.key, "calls": t.calls, "input_tokens": t.input_tokens, "cache_read_tokens": t.cache_read_tokens, "cache_write_tokens": t.cache_write_tokens, "output_tokens": t.output_tokens, "thinking_tokens": t.thinking_tokens, "cost_usd": t.cost_usd})).collect::<Vec<_>>(),
            "budget": Self::budget_json(&m),
        }))
    }

    async fn budget_set(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let usd = p["usd"].as_f64().filter(|u| u.is_finite() && *u > 0.0 && *u <= 10_000.0).ok_or_else(|| err(code::INVALID_PARAMS, "`usd` must be a number above 0 and at most 10000"))?;
        let cap = delphi_core::budget::usd_to_micro(usd);
        m.append(EventEnvelope::new(&m.id, "budget.cap", json!({"cap": cap, "usd": usd, "by": format!("console-{}", ctx.client_id)}))).await.map_err(engine_err)?;
        Ok(Self::budget_json(&m))
    }

    fn ratify_list(&self) -> RpcOutcome {
        let ps = crate::ratify::list(&self.engine.ws.home, &self.engine.ws.dir);
        Ok(Value::Array(ps.iter().map(|p| json!({"id": p.id, "kind": p.kind.as_str(), "summary": p.summary, "live_blake3": p.live_blake3, "ratified_blake3": p.ratified_blake3, "diff": p.diff.chars().take(20_000).collect::<String>()})).collect()))
    }

    /// Ratifying and rejecting are the operator's decisions: console scope, bound to the version reviewed,
    /// and recorded in the mission log. Ratification is the only path to L5, and it resets the artifact's ceiling.
    async fn ratify_change(&self, p: Value, ctx: CallContext, reject: bool) -> RpcOutcome {
        need_console(&ctx)?;
        let (Some(id), Some(expected)) = (p["id"].as_str(), p["expected_blake3"].as_str()) else {
            return Err(err(code::INVALID_PARAMS, "`id` and `expected_blake3` are required"));
        };
        let (home, ws) = (self.engine.ws.home.clone(), self.engine.ws.dir.clone());
        let (idc, exc) = (id.to_string(), expected.to_string());
        let m = self.mission(p["mission_id"].as_str()).await?;
        let result = tokio::task::spawn_blocking(move || if reject { crate::ratify::reject(&home, &ws, &idc, &exc).map(|_| String::new()) } else { crate::ratify::apply(&home, &ws, &idc, &exc) }).await.map_err(|e| err(-32000, e.to_string()))?;
        match result {
            Ok(pin) => {
                let by = format!("console-{}", ctx.client_id);
                let kind = if reject { "ratify.rejected" } else { "ratify.applied" };
                m.append(EventEnvelope::new(&m.id, kind, json!({"id": id, "blake3": pin, "by": by}))).await.map_err(engine_err)?;
                if !reject && !pin.is_empty() && !id.ends_with("#access") {
                    let zone = if id.starts_with("op:") { delphi_core::provenance::Zone::Local } else { delphi_core::provenance::Zone::Team };
                    let path = id.split_once(':').map(|x| x.1).unwrap_or(id);
                    m.append(EventEnvelope::new(&m.id, "provenance.stamp", delphi_core::provenance::Index::stamp_payload(zone, path, &pin, delphi_core::provenance::Tier::L5))).await.map_err(engine_err)?;
                }
                // Newly ratified endpoints are live immediately; no restart.
                if !reject && id.ends_with("#access") {
                    let notices = crate::boot::reload_config(&self.engine).map_err(|e| err(-32000, e))?;
                    self.set_notices(notices);
                }
                Ok(json!({"id": id, "blake3": pin}))
            }
            Err(crate::ratify::RatifyError::Stale { .. }) => Err(err(code::CONFIRMATION_REQUIRED, "stale: ".to_string() + &result_msg(id))),
            Err(e) => Err(err(code::INVALID_PARAMS, e.to_string())),
        }
    }

    async fn snapshot(&self, p: Value) -> RpcOutcome {
        let m = self.mission(p["mission_id"].as_str()).await?;
        let running: Vec<Value> = self.running.lock().unwrap().iter().map(|(id, r)| json!({"task_id": id, "target": r.target, "prompt": r.prompt, "seconds": r.started.elapsed().as_secs()})).collect();
        Ok(json!({
            "mission_id": m.id,
            "budget": Self::budget_json(&m),
            "mode": self.engine.broker.mode().as_str(),
            "pending_approvals": self.pending_json(),
            "recovery": self.recovery_json(&m.id),
            "pending_proposals": crate::ratify::list(&self.engine.ws.home, &self.engine.ws.dir).len(),
            "notices": self.notices.lock().unwrap().clone(),
            "running_tasks": running,
            "enabled_agents": self.engine.settings.enabled_agents,
        }))
    }
}

/// The handle the daemon holds (a newtype, since `Extension` and `Arc` are both foreign to this crate).
pub struct ServiceExt(pub Arc<EngineService>);

impl EngineService {
    pub fn extension(self: &Arc<Self>) -> Arc<dyn Extension> {
        Arc::new(ServiceExt(self.clone()))
    }
}

impl Extension for ServiceExt {
    fn call(&self, method: &str, params: Value, ctx: CallContext) -> RpcFuture {
        let (svc, method) = (self.0.clone(), method.to_string());
        Box::pin(async move {
            Some(match method.as_str() {
                "mission.open" => match need_console(&ctx) {
                    Err(e) => Err(e),
                    Ok(()) => svc.mission(params["mission_id"].as_str()).await.map(|m| json!({"mission_id": m.id, "budget": EngineService::budget_json(&m), "mode": svc.engine.broker.mode().as_str(), "recovery": svc.recovery_json(&m.id)})),
                },
                "recovery.resume" => svc.recovery_resume(params, ctx).await,
                "recovery.pause" => (|| {
                    need_console(&ctx)?;
                    Ok(json!({"paused": true}))
                })(),
                "task.submit" => svc.task_submit(params, ctx).await,
                "approval.list" => Ok(svc.pending_json()),
                "approval.resolve" => svc.approval_resolve(params, ctx),
                "usage.get" => svc.usage_get(params).await,
                "budget.get" => svc.mission(params["mission_id"].as_str()).await.map(|m| EngineService::budget_json(&m)),
                "budget.set" => svc.budget_set(params, ctx).await,
                "permissions.get" => Ok(json!({"mode": svc.engine.broker.mode().as_str(), "allow_similar": svc.engine.broker.allow_similar().iter().map(|(t, tool, pat)| json!({"task_id": t, "tool": tool, "pattern": pat})).collect::<Vec<_>>()})),
                "permissions.set" => (|| {
                    need_console(&ctx)?;
                    let mode = params["mode"].as_str().and_then(Mode::parse).ok_or_else(|| err(code::INVALID_PARAMS, "`mode` must be default, accept-edits or mission-auto"))?;
                    svc.engine.broker.set_mode(mode);
                    Ok(json!({"mode": mode.as_str()}))
                })(),
                "session.snapshot" => svc.snapshot(params).await,
                "ratify.list" => svc.ratify_list(),
                "ratify.apply" => svc.ratify_change(params, ctx, false).await,
                "ratify.reject" => svc.ratify_change(params, ctx, true).await,
                _ => return None,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Step;
    use crate::testkit::{events_of, setup, Fx};
    use delphi_core::client::{Client, ClientError};
    use delphi_core::server::{Daemon, DaemonConfig, TtyPolicy};
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    struct Served {
        sock: PathBuf,
        console: String,
        narrator: String,
        _dir: tempfile::TempDir,
    }

    async fn serve(f: &Fx) -> Served {
        let dir = tempfile::Builder::new().prefix("dl").tempdir_in("/tmp").unwrap();
        let sock = dir.path().join("d.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let mut cfg = DaemonConfig::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        cfg.tty_policy = TtyPolicy::Disabled;
        let daemon = Daemon::with_hub(cfg, f.engine.hub.clone()).unwrap();
        let svc = EngineService::new(f.engine.clone(), "m1");
        svc.adopt(f.m.clone());
        daemon.set_extension(svc.extension());
        let (console, narrator) = (daemon.console_token().to_string(), daemon.narrator_token().to_string());
        tokio::spawn(daemon.serve(listener));
        Served { sock, console, narrator, _dir: dir }
    }

    async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        tokio::task::spawn_blocking(f).await.unwrap()
    }

    fn rpc_code(r: Result<Value, ClientError>) -> Option<i64> {
        match r {
            Err(ClientError::Rpc { code, .. }) => Some(code),
            _ => None,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_console_submits_a_task_streams_its_events_and_reads_usage_and_budget() {
        let f = setup(vec![Step::text("Three attempts, then escalate.")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.sock.clone(), s.console.clone());
        let (text, usage, budget) = blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            c.hello(&tok).unwrap();
            c.call("events.subscribe", json!({})).unwrap();
            let sub = c.call("task.submit", json!({"prompt": "Explain the retry ladder"})).unwrap();
            assert_eq!(sub["task_id"], "t_0001");
            let mut kinds = vec![];
            let text = loop {
                let n = c.next_notification().unwrap();
                if n["method"] == "event" {
                    kinds.push(n["params"]["type"].as_str().unwrap().to_string());
                    if n["params"]["type"] == "task.output" {
                        break n["params"]["payload"]["text"].as_str().unwrap().to_string();
                    }
                }
            };
            assert!(kinds.contains(&"budget.reserve".to_string()) && kinds.contains(&"agent.usage".to_string()), "{:?}", kinds);
            (text, c.call("usage.get", json!({"group": "role"})).unwrap(), c.call("budget.get", json!({})).unwrap())
        })
        .await;
        assert_eq!(text, "Three attempts, then escalate.");
        assert_eq!((usage["rows"][0]["key"].as_str(), usage["rows"][0]["calls"].as_u64()), (Some("orchestrator"), Some(1)));
        assert!((budget["spent_usd"].as_f64().unwrap() - 0.03).abs() < 1e-9, "{}", budget);
        assert_eq!(budget["reserved_usd"].as_f64().unwrap(), 0.0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_narrator_scope_can_read_but_never_change_anything() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.sock.clone(), s.narrator.clone());
        blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            for (m, p) in [("task.submit", json!({"prompt": "hi"})), ("approval.resolve", json!({"approval_id": "a", "payload_blake3": "b", "decision": "allow"})), ("budget.set", json!({"usd": 99})), ("permissions.set", json!({"mode": "mission-auto"})), ("mission.open", json!({}))] {
                assert_eq!(rpc_code(c.call(m, p)), Some(code::FORBIDDEN), "{}", m);
            }
            for m in ["usage.get", "approval.list", "permissions.get", "budget.get", "session.snapshot"] {
                assert!(c.call(m, json!({})).is_ok(), "{} should be readable", m);
            }
            assert_eq!(rpc_code(c.call("no.such.method", json!({}))), Some(code::METHOD_NOT_FOUND));
            assert_eq!(c.call("session.ping", json!({})).unwrap(), json!("pong"));
        })
        .await;
        assert_eq!(f.model.requests().len(), 0, "the narrator started nothing");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approval_cards_are_answered_over_rpc_with_bound_payloads_and_first_answer_wins() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "# New"})), Step::text("Drafted.")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.sock.clone(), s.console.clone());
        let (wrong, a, b, final_text) = blocking(move || {
            let mut c1 = Client::connect(&sock).unwrap();
            c1.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            c1.hello(&tok).unwrap();
            let mut c2 = Client::connect(&sock).unwrap();
            c2.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            c2.hello(&tok).unwrap();
            c1.call("events.subscribe", json!({})).unwrap();
            c1.call("task.submit", json!({"target": "architect", "prompt": "Draft a spec"})).unwrap();
            let card = loop {
                let list = c1.call("approval.list", json!({})).unwrap();
                if let Some(first) = list.as_array().and_then(|a| a.first()) {
                    break first.clone();
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            let (id, hash) = (card["approval_id"].as_str().unwrap().to_string(), card["payload_blake3"].as_str().unwrap().to_string());
            assert_eq!((card["tier"].as_str(), card["tool"].as_str()), (Some("amber"), Some("write_file")));
            let wrong = rpc_code(c1.call("approval.resolve", json!({"approval_id": id, "payload_blake3": "00ff", "decision": "allow"})));
            let a = c1.call("approval.resolve", json!({"approval_id": id, "payload_blake3": hash, "decision": "allow"}));
            let b = c2.call("approval.resolve", json!({"approval_id": id, "payload_blake3": hash, "decision": "deny"}));
            let final_text = loop {
                let n = c1.next_notification().unwrap();
                if n["method"] == "event" && n["params"]["type"] == "task.output" {
                    break n["params"]["payload"]["text"].as_str().unwrap().to_string();
                }
            };
            (wrong, a, b, final_text)
        })
        .await;
        assert_eq!(wrong, Some(code::INVALID_PARAMS), "a payload hash for something else is refused");
        assert_eq!(a.unwrap()["resolution"], "approved");
        assert_eq!(rpc_code(b), Some(code::NOT_FOUND), "the second answer arrives too late");
        assert_eq!(final_text, "Drafted.");
        assert!(f.root.join("workspaces/w/team/specs/009-new.md").exists());
        assert_eq!(events_of(&f, "approval.resolved").len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_operator_can_lower_the_budget_and_it_takes_effect_and_persists() {
        let f = setup(vec![Step::text("never asked")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.sock.clone(), s.console.clone());
        let (set, out) = blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            c.hello(&tok).unwrap();
            c.call("events.subscribe", json!({})).unwrap();
            assert_eq!(rpc_code(c.call("budget.set", json!({"usd": -1}))), Some(code::INVALID_PARAMS));
            let set = c.call("budget.set", json!({"usd": 0.0001})).unwrap();
            c.call("task.submit", json!({"prompt": "hi"})).unwrap();
            let out = loop {
                let n = c.next_notification().unwrap();
                if n["method"] == "event" && n["params"]["type"] == "task.output" {
                    break n["params"]["payload"].clone();
                }
            };
            (set, out)
        })
        .await;
        assert!((set["cap_usd"].as_f64().unwrap() - 0.0001).abs() < 1e-9);
        assert_eq!(out["status"], "escalated");
        assert!(out["text"].as_str().unwrap().contains("mission budget"));
        assert_eq!(f.model.requests().len(), 0);
        assert_eq!(events_of(&f, "budget.cap").len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ratification_over_rpc_is_console_only_bound_to_the_reviewed_version_and_logged() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let live = f.root.join("workspaces/w/team/context/team-heuristics.md");
        fs::write(&live, "---\nid: t\n---\n### [T-001] changed rule\n- be thorough\n").unwrap();
        let (c_sock, c_tok, n_sock, n_tok) = (s.sock.clone(), s.console.clone(), s.sock.clone(), s.narrator.clone());
        let (listed, narrator_denied, stale, applied, after, snap) = blocking(move || {
            let mut nar = Client::connect(&n_sock).unwrap();
            nar.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            nar.hello(&n_tok).unwrap();
            let mut c = Client::connect(&c_sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&c_tok).unwrap();
            let listed = c.call("ratify.list", json!({})).unwrap();
            assert_eq!(nar.call("ratify.list", json!({})).unwrap(), listed, "the narrator can read proposals");
            let id = listed[0]["id"].as_str().unwrap().to_string();
            let live_hash = listed[0]["live_blake3"].as_str().unwrap().to_string();
            let narrator_denied = rpc_code(nar.call("ratify.apply", json!({"id": id, "expected_blake3": live_hash})));
            let stale = rpc_code(c.call("ratify.apply", json!({"id": id, "expected_blake3": "0000"})));
            let snap = c.call("session.snapshot", json!({})).unwrap();
            let applied = c.call("ratify.apply", json!({"id": id, "expected_blake3": live_hash}));
            let after = c.call("ratify.list", json!({})).unwrap();
            (listed, narrator_denied, stale, applied, after, snap)
        })
        .await;
        assert_eq!((listed.as_array().unwrap().len(), listed[0]["kind"].as_str()), (1, Some("changed")));
        assert!(listed[0]["diff"].as_str().unwrap().contains("+ - be thorough"), "{}", listed[0]["diff"]);
        assert_eq!(narrator_denied, Some(code::FORBIDDEN));
        assert_eq!(stale, Some(code::CONFIRMATION_REQUIRED), "a version other than the reviewed one is refused");
        assert_eq!(snap["pending_proposals"], 1);
        assert!(applied.is_ok());
        assert_eq!(after.as_array().unwrap().len(), 0);
        let ratified = fs::read_to_string(f.root.join("workspaces/w/local/ratified/context/team-heuristics.md")).unwrap();
        assert!(ratified.contains("be thorough"));
        assert_eq!(events_of(&f, "ratify.applied").len(), 1);
        assert_eq!(events_of(&f, "provenance.stamp").last().unwrap().payload["tier"], "L5", "ratification is the path to L5");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_restarted_daemon_offers_recovery_and_only_the_console_can_resume() {
        let f = setup(vec![Step::tool("t1", "read_file", json!({"path": "@project/app/src/lib.rs"})), Step::Hang], 5.0, |_| {});
        let task = { let e = f.engine.clone(); let m = f.m.clone(); tokio::spawn(async move { e.run_task(&m, "architect", "What is in @project/app/src/lib.rs?").await }) };
        let model = f.model.clone();
        for _ in 0..300 {
            if model.requests().len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        task.abort();
        let _ = task.await;
        let f2 = f.restart(vec![Step::text("Resumed answer.")]);
        // A service that has not adopted the mission opens it itself, and so plans its recovery.
        let dir = tempfile::Builder::new().prefix("dl").tempdir_in("/tmp").unwrap();
        let sock = dir.path().join("d.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let mut cfg = DaemonConfig::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        cfg.tty_policy = TtyPolicy::Disabled;
        let daemon = Daemon::with_hub(cfg, f2.engine.hub.clone()).unwrap();
        let svc = EngineService::new(f2.engine.clone(), "m1");
        // The test kit already opened the mission in this process; hand it over rather than opening twice.
        let m2 = f2.m.clone();
        let plan = f2.engine.recover_mission(&m2).await.unwrap().unwrap();
        svc.adopt(m2);
        svc.recovery.lock().unwrap().insert("m1".into(), PlanState { plan: Arc::new(plan), resumed: Default::default() });
        daemon.set_extension(svc.extension());
        let (ctok, ntok) = (daemon.console_token().to_string(), daemon.narrator_token().to_string());
        tokio::spawn(daemon.serve(listener));
        let (sock2, sock3) = (sock.clone(), sock.clone());
        let (snap, denied, resumed, again, out) = blocking(move || {
            let mut nar = Client::connect(&sock2).unwrap();
            nar.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            nar.hello(&ntok).unwrap();
            let mut c = Client::connect(&sock3).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            c.hello(&ctok).unwrap();
            c.call("events.subscribe", json!({})).unwrap();
            let snap = c.call("session.snapshot", json!({})).unwrap();
            let denied = rpc_code(nar.call("recovery.resume", json!({})));
            let resumed = c.call("recovery.resume", json!({})).unwrap();
            let again = c.call("recovery.resume", json!({})).unwrap();
            let out = loop {
                let n = c.next_notification().unwrap();
                if n["method"] == "event" && n["params"]["type"] == "task.output" {
                    break n["params"]["payload"]["text"].as_str().unwrap().to_string();
                }
            };
            (snap, denied, resumed, again, out)
        })
        .await;
        assert_eq!(snap["recovery"]["tasks"][0]["task_id"], "t_0001");
        assert!(snap["recovery"]["tasks"][0]["action"].as_str().unwrap().contains("retries its interrupted turn"));
        assert_eq!(denied, Some(code::FORBIDDEN));
        assert_eq!(resumed["resumed"][0], "t_0001");
        assert_eq!(again["resumed"].as_array().unwrap().len(), 0, "a task is resumed once");
        assert_eq!(out, "Resumed answer.");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn permissions_round_trip_and_the_snapshot_reports_pending_work() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "x"})), Step::text("done")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.sock.clone(), s.console.clone());
        let snap = blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            c.hello(&tok).unwrap();
            assert_eq!(c.call("permissions.get", json!({})).unwrap()["mode"], "default");
            assert_eq!(c.call("permissions.set", json!({"mode": "accept-edits"})).unwrap()["mode"], "accept-edits");
            assert_eq!(rpc_code(c.call("permissions.set", json!({"mode": "yolo"}))), Some(code::INVALID_PARAMS));
            c.call("task.submit", json!({"target": "architect", "prompt": "Draft"})).unwrap();
            loop {
                let snap = c.call("session.snapshot", json!({})).unwrap();
                if snap["pending_approvals"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                    break snap;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        })
        .await;
        assert_eq!(snap["mode"], "accept-edits", "a draft spec still asks in every mode");
        assert_eq!(snap["running_tasks"][0]["target"], "architect");
        assert_eq!(snap["budget"]["cap_usd"], 5.0);
        assert!(snap["enabled_agents"].as_array().unwrap().iter().any(|a| a == "red-team"));
    }
}
