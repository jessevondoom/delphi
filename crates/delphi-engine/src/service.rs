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
    /// Handles to stop the tasks in `running` (`task.cancel`).
    aborts: Mutex<HashMap<String, tokio::task::AbortHandle>>,
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
        Arc::new(EngineService { engine, aborts: Mutex::new(HashMap::new()), default_mission: default_mission.to_string(), missions: Mutex::new(HashMap::new()), running: Arc::new(Mutex::new(HashMap::new())), notices: Mutex::new(Vec::new()), open_gate: tokio::sync::Mutex::new(()), recovery: Mutex::new(HashMap::new()) })
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
            let h = tokio::spawn(async move {
                if let Err(e) = engine.resume_task(&m2, &rec2, &tid2).await {
                    let mut ev = EventEnvelope::new(&m2.id, "task.failed", json!({"error": engine.secrets.redact(&e.to_string())}));
                    ev.task_id = Some(tid2.clone());
                    let _ = m2.append(ev).await;
                }
                running.lock().unwrap().remove(&tid2);
            });
            self.aborts.lock().unwrap().insert(tid.clone(), h.abort_handle());
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
        let h = tokio::spawn(async move {
            if let Err(e) = engine.run_task_with_id(&m, tid.clone(), &target, &prompt).await {
                let mut ev = EventEnvelope::new(&m.id, "task.failed", json!({"error": engine.secrets.redact(&e.to_string())}));
                ev.task_id = Some(tid.clone());
                let _ = m.append(ev).await;
            }
            running.lock().unwrap().remove(&tid);
        });
        self.aborts.lock().unwrap().insert(task_id.clone(), h.abort_handle());
        Ok(json!({"task_id": task_id, "mission_id": mid}))
    }

    /// `task.cancel`: stop one running task (or every one) at the operator's word. What was started is left in a
    /// defined state: the task ends as `task.cancelled`, an in-flight model call is charged its full reservation (the
    /// same rule as a crash), approvals waiting on it are closed, a build's worktree is kept and nothing is merged.
    /// A command already running in a sandbox finishes or times out on its own; its result is ignored.
    async fn task_cancel(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let wanted: Vec<String> = match p["task_id"].as_str() {
            Some(t) => vec![t.to_string()],
            None => self.running.lock().unwrap().keys().cloned().collect(),
        };
        let mut done = Vec::new();
        for tid in wanted {
            if self.running.lock().unwrap().remove(&tid).is_none() {
                continue;
            }
            if let Some(h) = self.aborts.lock().unwrap().remove(&tid) {
                h.abort();
            }
            self.engine.cancel_task(&m, &tid).await.map_err(engine_err)?;
            done.push(tid);
        }
        self.aborts.lock().unwrap().retain(|k, _| self.running.lock().unwrap().contains_key(k));
        Ok(json!({"cancelled": done}))
    }

    /// `build.start`: a Builder task under the retry ladder (`build.rs`), in the background.
    async fn build_start(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let prompt = p["prompt"].as_str().unwrap_or("").trim().to_string();
        if prompt.is_empty() || prompt.chars().count() > 20_000 {
            return Err(err(code::INVALID_PARAMS, "`prompt` must be 1 to 20,000 characters"));
        }
        if !self.engine.settings.enabled_agents.iter().any(|a| a == "builder") {
            return Err(err(code::INVALID_PARAMS, "the builder role is not enabled"));
        }
        let task_id = m.next_task_id();
        self.running.lock().unwrap().insert(task_id.clone(), Running { target: "builder".into(), prompt: prompt.chars().take(120).collect(), started: Instant::now() });
        let (engine, running, tid, mid) = (self.engine.clone(), self.running.clone(), task_id.clone(), m.id.clone());
        let h = tokio::spawn(async move {
            if let Err(e) = engine.run_build(&m, tid.clone(), &prompt).await {
                let mut ev = EventEnvelope::new(&m.id, "task.failed", json!({"error": engine.secrets.redact(&e.to_string())}));
                ev.task_id = Some(tid.clone());
                let _ = m.append(ev).await;
            }
            running.lock().unwrap().remove(&tid);
        });
        self.aborts.lock().unwrap().insert(task_id.clone(), h.abort_handle());
        Ok(json!({"task_id": task_id, "mission_id": mid}))
    }

    /// `council.start`: a three-round Council on a proposition (`council.rs`), in the background.
    async fn council_start(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let proposition = p["proposition"].as_str().unwrap_or("").trim().to_string();
        if proposition.is_empty() || proposition.chars().count() > 20_000 {
            return Err(err(code::INVALID_PARAMS, "`proposition` must be 1 to 20,000 characters"));
        }
        let seats: Vec<String> = p["seats"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
        let opts = crate::council::CouncilOpts { seats, author_family: p["author_family"].as_str().map(String::from) };
        let task_id = m.next_task_id();
        self.running.lock().unwrap().insert(task_id.clone(), Running { target: "council".into(), prompt: proposition.chars().take(120).collect(), started: Instant::now() });
        let (engine, running, tid, mid) = (self.engine.clone(), self.running.clone(), task_id.clone(), m.id.clone());
        let h = tokio::spawn(async move {
            if let Err(e) = engine.run_council(&m, tid.clone(), &proposition, opts).await {
                let mut ev = EventEnvelope::new(&m.id, "task.failed", json!({"error": engine.secrets.redact(&e.to_string())}));
                ev.task_id = Some(tid.clone());
                let _ = m.append(ev).await;
            }
            running.lock().unwrap().remove(&tid);
        });
        self.aborts.lock().unwrap().insert(task_id.clone(), h.abort_handle());
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

    /// `observe.snapshot`: the recent history the Narrator needs, as the events it would have seen live.
    /// Read-only; only the kinds the Narrator folds, newest 4,000.
    async fn observe_snapshot(&self, p: Value) -> RpcOutcome {
        let m = self.mission(p["mission_id"].as_str()).await?;
        let mut events = m.replay().map_err(engine_err)?;
        events.retain(|e| crate::observe::KINDS.contains(&e.kind.as_str()));
        let skip = events.len().saturating_sub(4000);
        let events: Vec<Value> = events.into_iter().skip(skip).map(|e| serde_json::to_value(e).expect("events serialize")).collect();
        Ok(json!({"mission_id": m.id, "events": events, "enabled_agents": self.engine.settings.enabled_agents, "budget": Self::budget_json(&m), "pending_proposals": crate::ratify::list(&self.engine.ws.home, &self.engine.ws.dir).len()}))
    }

    /// `observe.diff`: the live diff of a build's worktree for the Forge view (read-only).
    async fn observe_diff(&self, p: Value) -> RpcOutcome {
        let task = p["task_id"].as_str().unwrap_or("").to_string();
        let Some(wt) = self.engine.exec.known_worktree(&task) else { return Ok(json!({"available": false})) };
        let r = tokio::task::spawn_blocking(move || wt.live_diff(120_000)).await.map_err(|e| err(-32000, e.to_string()))?;
        match r {
            Ok((stat, diff, untracked)) => Ok(json!({"available": true, "stat": stat, "diff": diff, "untracked": untracked})),
            Err(e) => Ok(json!({"available": false, "error": self.engine.secrets.redact(&e.to_string())})),
        }
    }

    /// `kb.search`: pointers and previews from the workspace's knowledge base (read-only). `kb.index` refreshes it.
    async fn kb_search(&self, p: Value) -> RpcOutcome {
        let query = p["query"].as_str().unwrap_or("").to_string();
        if query.trim().is_empty() || query.chars().count() > 500 {
            return Err(err(code::INVALID_PARAMS, "`query` must be 1 to 500 characters"));
        }
        let (ws, role) = (self.engine.ws.dir.clone(), p["role"].as_str().map(String::from));
        let hits = tokio::task::spawn_blocking(move || -> Result<Vec<crate::kb::Hit>, crate::kb::KbError> {
            let mut kb = crate::kb::Kb::open(&ws.join("local/state.db"))?;
            if kb.counts()?.0 == 0 {
                kb.sync(&crate::kb::sources(&ws), "")?;
            }
            let hits = kb.search(&query, &crate::kb::SearchOpts { role: role.clone(), limit: 10, include_untrusted: false })?;
            crate::kb::log_search(&ws, &query, role.as_deref(), hits.len());
            Ok(hits)
        })
        .await
        .map_err(|e| err(-32000, e.to_string()))?
        .map_err(|e| err(-32000, e.to_string()))?;
        Ok(json!(hits.iter().map(|h| json!({"pointer": h.pointer, "heading": h.heading_path, "preview": h.preview, "blake3": h.blake3})).collect::<Vec<_>>()))
    }

    async fn kb_index(&self, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let ws = self.engine.ws.dir.clone();
        let rep = tokio::task::spawn_blocking(move || -> Result<crate::kb::IndexReport, crate::kb::KbError> { crate::kb::Kb::open(&ws.join("local/state.db"))?.sync(&crate::kb::sources(&ws), "") })
            .await
            .map_err(|e| err(-32000, e.to_string()))?
            .map_err(|e| err(-32000, e.to_string()))?;
        Ok(json!({"added": rep.added, "updated": rep.updated, "removed": rep.removed, "unchanged": rep.unchanged}))
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
                    // Stamp under the zone a read of that file will look it up in, or the stamp never applies.
                    let bare = id.split_once(':').map(|x| x.1).unwrap_or(id);
                    let (zone, path) = if id.starts_with("op:") { (delphi_core::provenance::Zone::Local, bare.to_string()) } else { crate::tools::zone_of(bare) };
                    m.append(EventEnvelope::new(&m.id, "provenance.stamp", delphi_core::provenance::Index::stamp_payload(zone, &path, &pin, delphi_core::provenance::Tier::L5))).await.map_err(engine_err)?;
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
                "task.cancel" => svc.task_cancel(params, ctx).await,
                "build.start" => svc.build_start(params, ctx).await,
                "council.start" => svc.council_start(params, ctx).await,
                "kb.search" => svc.kb_search(params).await,
                "observe.snapshot" => svc.observe_snapshot(params).await,
                "observe.diff" => svc.observe_diff(params).await,
                "kb.index" => svc.kb_index(ctx).await,
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
        narrator_sock: PathBuf,
        console: String,
        narrator: String,
        _dir: tempfile::TempDir,
    }

    async fn serve(f: &Fx) -> Served {
        serve_with(f, None).await
    }

    async fn serve_with(f: &Fx, queue_cap: Option<usize>) -> Served {
        let dir = tempfile::Builder::new().prefix("dl").tempdir_in("/tmp").unwrap();
        let sock = dir.path().join("d.sock");
        let narrator_sock = dir.path().join("n.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let narrator_listener = tokio::net::UnixListener::bind(&narrator_sock).unwrap();
        let mut cfg = DaemonConfig::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        cfg.tty_policy = TtyPolicy::Disabled;
        if let Some(c) = queue_cap {
            cfg.queue_cap = c;
        }
        let daemon = Daemon::with_hub(cfg, f.engine.hub.clone()).unwrap();
        let svc = EngineService::new(f.engine.clone(), "m1");
        svc.adopt(f.m.clone());
        daemon.set_extension(svc.extension());
        let (console, narrator) = (daemon.console_token().to_string(), daemon.narrator_token().to_string());
        tokio::spawn(daemon.serve_sockets(listener, Some(narrator_listener)));
        Served { sock, narrator_sock, console, narrator, _dir: dir }
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
        let (sock, tok) = (s.narrator_sock.clone(), s.narrator.clone());
        blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            for (m, p) in [("task.submit", json!({"prompt": "hi"})), ("task.cancel", json!({})), ("build.start", json!({"prompt": "hi"})), ("council.start", json!({"proposition": "hi"})), ("kb.index", json!({})), ("approval.resolve", json!({"approval_id": "a", "payload_blake3": "b", "decision": "allow"})), ("budget.set", json!({"usd": 99})), ("permissions.set", json!({"mode": "mission-auto"})), ("mission.open", json!({}))] {
                assert_eq!(rpc_code(c.call(m, p)), Some(code::FORBIDDEN), "{}", m);
            }
            for m in ["usage.get", "approval.list", "permissions.get", "budget.get", "session.snapshot", "observe.snapshot", "observe.diff"] {
                assert!(c.call(m, json!({})).is_ok(), "{} should be readable", m);
            }
            assert_eq!(rpc_code(c.call("no.such.method", json!({}))), Some(code::FORBIDDEN), "an unknown method is refused, not looked up");
            assert_eq!(c.call("session.ping", json!({})).unwrap(), json!("pong"));
        })
        .await;
        assert_eq!(f.model.requests().len(), 0, "the narrator started nothing");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_socket_decides_the_scope_and_a_token_for_the_other_socket_gets_nothing() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let (c_sock, n_sock, c_tok, n_tok) = (s.sock.clone(), s.narrator_sock.clone(), s.console.clone(), s.narrator.clone());
        blocking(move || {
            // The console token on the narrator socket, and the narrator token on the console socket: closed, no answer.
            let mut c = Client::connect(&n_sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            assert!(c.hello(&c_tok).is_err(), "a console token is no use on the narrator socket");
            let mut c = Client::connect(&c_sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            assert!(c.hello(&n_tok).is_err(), "a narrator token is no use on the console socket");
            // A client cannot ask the narrator socket for a console token, whatever it is attached to.
            let mut c = Client::connect(&n_sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            assert_eq!(rpc_code(c.call("auth.issue", json!({"scope": "console"}))), Some(code::FORBIDDEN));
            assert!(c.issue_token(delphi_core::proto::Scope::Narrator).is_ok(), "it will issue its own kind");
            let mut c = Client::connect(&c_sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            assert_eq!(rpc_code(c.call("auth.issue", json!({"scope": "narrator"}))), Some(code::FORBIDDEN));
            // The granted scope is the socket's: the handshake reports it.
            let mut c = Client::connect(&n_sock).unwrap();
            assert_eq!(c.hello(&n_tok).unwrap()["scope"], "narrator");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_narrator_socket_answers_a_fixed_list_and_everything_else_is_refused_by_name() {
        // The list is part of the contract: adding a method here is a decision, so this test names it.
        let mut listed: Vec<&str> = delphi_core::server::NARRATOR_METHODS.to_vec();
        listed.sort();
        assert_eq!(listed, vec!["approval.list", "budget.get", "events.subscribe", "kb.search", "observe.diff", "observe.snapshot", "permissions.get", "ratify.list", "session.ping", "session.snapshot", "usage.get"]);
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.narrator_sock.clone(), s.narrator.clone());
        blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            let every_mutating_method_the_daemon_has = ["task.submit", "build.start", "council.start", "kb.index", "approval.resolve", "budget.set", "permissions.set", "mission.open", "recovery.resume", "recovery.pause", "ratify.apply", "ratify.reject", "mission.rewind", "daemon.shutdown", "auth.issue", "hello", "events.replay", "shell.run", "anything.else"];
            for m in every_mutating_method_the_daemon_has {
                assert_eq!(rpc_code(c.call(m, json!({"prompt": "x", "mission_id": "m1"}))), Some(code::FORBIDDEN), "{}", m);
            }
            for m in delphi_core::server::NARRATOR_METHODS {
                assert_ne!(rpc_code(c.call(m, json!({}))), Some(code::FORBIDDEN), "{} is on the list, so it must be answered", m);
            }
        })
        .await;
        assert_eq!(f.model.requests().len(), 0);
        assert!(f.m.replay().unwrap().iter().all(|e| e.kind != "task.created"), "nothing was started");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_narrator_that_stops_reading_is_bounded_and_told_to_resync_without_slowing_the_console() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve_with(&f, Some(16)).await;
        let (c_sock, n_sock, c_tok, n_tok) = (s.sock.clone(), s.narrator_sock.clone(), s.console.clone(), s.narrator.clone());
        let hub = f.engine.hub.clone();
        blocking(move || {
            let mut stalled = Client::connect(&n_sock).unwrap();
            stalled.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            stalled.hello(&n_tok).unwrap();
            stalled.call("events.subscribe", json!({})).unwrap();
            let mut console = Client::connect(&c_sock).unwrap();
            console.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            console.hello(&c_tok).unwrap();
            console.call("events.subscribe", json!({})).unwrap();
            // Far more events than the queue holds; the narrator reads none of them.
            let filler = "x".repeat(16 * 1024); // enough bytes that the socket's own buffer fills and the queue behind it must hold
            for i in 0..400 {
                hub.publish_event(&json!({"type": "mission.note", "payload": {"i": i, "filler": filler}}));
                // The console keeps up, one event at a time.
                let n = console.next_notification().unwrap();
                assert_eq!(n["params"]["payload"]["i"], i);
            }
            // The stalled narrator did not hold the daemon up; when it reads again it is told to start over from a snapshot.
            let mut saw_resync = false;
            for _ in 0..600 {
                match stalled.next_notification() {
                    Ok(n) if n["method"] == "resync_required" => {
                        saw_resync = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            assert!(saw_resync, "an overflowing narrator gets resync_required");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_narrator_that_attaches_late_rebuilds_the_same_picture_from_the_snapshot() {
        let f = setup(vec![Step::text("The ladder has three attempts.")], 5.0, |_| {});
        f.engine.run_task(&f.m, "orchestrator", "Explain the retry ladder").await.unwrap();
        let s = serve(&f).await;
        let (sock, tok) = (s.narrator_sock.clone(), s.narrator.clone());
        let snap = blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            assert_eq!(c.call("observe.diff", json!({"task_id": "t_0001"})).unwrap()["available"], false, "no live worktree for a plain task");
            c.call("observe.snapshot", json!({})).unwrap()
        })
        .await;
        let roles: Vec<String> = snap["enabled_agents"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(String::from)).collect();
        let mut late = crate::observe::ObserverState::new(&roles);
        for e in snap["events"].as_array().unwrap() {
            late.apply(e);
        }
        // The same fold over the log directly gives the same state.
        let mut direct = crate::observe::ObserverState::new(&roles);
        for e in f.m.replay().unwrap() {
            direct.apply(&serde_json::to_value(e).unwrap());
        }
        let (a, b) = (&late.agents["orchestrator"], &direct.agents["orchestrator"]);
        assert_eq!((a.calls, a.tokens.clone(), a.model.clone(), a.state.clone()), (b.calls, b.tokens.clone(), b.model.clone(), b.state.clone()));
        assert_eq!((a.calls, a.state.as_str()), (1, "idle"));
        assert!(a.tokens.total() > 0 && late.spent_usd > 0.0 && (late.spent_usd - direct.spent_usd).abs() < 1e-12);
        assert_eq!(late.tasks["t_0001"].status, "done");
        assert!(snap["budget"]["cap_usd"].as_f64().unwrap() > 0.0);
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
    async fn cancelling_a_task_waiting_on_an_approval_ends_it_cleanly_and_it_is_never_offered_again() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-new.md", "content": "# New"})), Step::text("Drafted.")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.sock.clone(), s.console.clone());
        let (cancelled, again, snap) = blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            c.hello(&tok).unwrap();
            let t = c.call("task.submit", json!({"target": "architect", "prompt": "Draft a spec"})).unwrap();
            loop {
                if c.call("approval.list", json!({})).unwrap().as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let cancelled = c.call("task.cancel", json!({"task_id": t["task_id"]})).unwrap_or_else(|e| panic!("task.cancel failed: {:?}", e));
            let again = c.call("task.cancel", json!({"task_id": t["task_id"]})).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            (cancelled, again, c.call("session.snapshot", json!({})).unwrap())
        })
        .await;
        assert_eq!(cancelled["cancelled"].as_array().unwrap().len(), 1);
        assert_eq!(again["cancelled"].as_array().unwrap().len(), 0, "cancelling twice does nothing the second time");
        assert_eq!(snap["running_tasks"].as_array().unwrap().len(), 0);
        assert_eq!(snap["pending_approvals"].as_array().unwrap().len(), 0, "the waiting card is closed");
        assert_eq!(events_of(&f, "task.cancelled").len(), 1);
        let resolved = events_of(&f, "approval.resolved");
        assert_eq!((resolved.len(), resolved[0].payload["resolution"].as_str()), (1, Some("expired")));
        assert!(events_of(&f, "task.output").is_empty(), "no reply is produced after a cancel");
        assert!(!f.root.join("workspaces/w/team/specs/009-new.md").exists(), "the action that was waiting never ran");
        let rec = crate::recovery::rebuild_with(&f.m.replay().unwrap(), &|_| None);
        assert!(rec.tasks.values().all(|t| t.completed), "recovery never offers a cancelled task");
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
        let (c_sock, c_tok, n_sock, n_tok) = (s.sock.clone(), s.console.clone(), s.narrator_sock.clone(), s.narrator.clone());
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
        let nsock = dir.path().join("n.sock");
        let nlistener = tokio::net::UnixListener::bind(&nsock).unwrap();
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
        tokio::spawn(daemon.serve_sockets(listener, Some(nlistener)));
        let (sock2, sock3) = (nsock.clone(), sock.clone());
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
