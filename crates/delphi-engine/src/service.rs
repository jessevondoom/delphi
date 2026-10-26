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
use crate::timers::{self, Fired};
use delphi_core::proto::{code, RpcError, Scope};
use delphi_core::provenance::Tier;
use delphi_core::server::{CallContext, Extension, RpcFuture, RpcOutcome};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// The `delphi` binary that `org.action` runs (`bundle add|remove`); this process when unset.
    bundle_exe: Mutex<Option<std::path::PathBuf>>,
}

struct PlanState {
    plan: Arc<RecoveryPlan>,
    resumed: std::collections::HashSet<String>,
}

/// The TOTP secret behind a credential reference: base32, as an authenticator shows it.
pub fn totp_secret(credential: &str) -> Result<Vec<u8>, String> {
    use delphi_adapter::credential::{CredentialRef, Resolved, Resolver};
    let r = CredentialRef::parse(credential)?;
    match Resolver::system(Arc::new(|_: &str| {})).resolve(&r) {
        Ok(Resolved::Token(s)) => delphi_core::totp::base32_decode(s.expose().trim()).filter(|b| b.len() >= 10).ok_or_else(|| "the stored secret is not base32 (or is too short)".to_string()),
        Ok(_) => Err("the credential did not give a secret".into()),
        Err(e) => Err(format!("could not read the secret: {}", e)),
    }
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
        Arc::new(EngineService { engine, aborts: Mutex::new(HashMap::new()), default_mission: default_mission.to_string(), missions: Mutex::new(HashMap::new()), running: Arc::new(Mutex::new(HashMap::new())), notices: Mutex::new(Vec::new()), open_gate: tokio::sync::Mutex::new(()), recovery: Mutex::new(HashMap::new()), bundle_exe: Mutex::new(None) })
    }

    /// Run `org.action` through this binary instead of the current executable (tests, where that is the test binary).
    pub fn set_bundle_exe(&self, exe: std::path::PathBuf) {
        *self.bundle_exe.lock().unwrap() = Some(exe);
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
        // The daemon has restarted since this mission was last open, so what was said before it is a past session (`specs/030`).
        if let Err(e) = self.begin_session(&m, "restart").await {
            self.notices.lock().unwrap().push(format!("could not start a session for {}: {}", id, e));
        }
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
        let task_id = self.spawn_task(&m, &target, &prompt, None);
        Ok(json!({"task_id": task_id, "mission_id": m.id}))
    }

    /// Start a task in the background and track it for `task.cancel` and the snapshot. A `ceiling` starts it
    /// below full trust (a timer's wake carries its setter's).
    fn spawn_task(&self, m: &Arc<MissionState>, target: &str, prompt: &str, ceiling: Option<Tier>) -> String {
        let task_id = m.next_task_id();
        self.running.lock().unwrap().insert(task_id.clone(), Running { target: target.to_string(), prompt: prompt.chars().take(120).collect(), started: Instant::now() });
        let (engine, running, tid, m, target, prompt) = (self.engine.clone(), self.running.clone(), task_id.clone(), m.clone(), target.to_string(), prompt.to_string());
        let h = tokio::spawn(async move {
            let run = match ceiling {
                Some(c) => engine.run_task_woken(&m, tid.clone(), &target, &prompt, c).await,
                None => engine.run_task_with_id(&m, tid.clone(), &target, &prompt).await,
            };
            if let Err(e) = run {
                let mut ev = EventEnvelope::new(&m.id, "task.failed", json!({"error": engine.secrets.redact(&e.to_string())}));
                ev.task_id = Some(tid.clone());
                let _ = m.append(ev).await;
            }
            running.lock().unwrap().remove(&tid);
        });
        self.aborts.lock().unwrap().insert(task_id.clone(), h.abort_handle());
        task_id
    }

    /// Run the timers whose time has come (`specs/017`): start each one's task, log `timer.fired`, then record the
    /// firing, in that order, so a crash in between fires it again (reported late) and never loses it. The daemon calls
    /// this every second; tests call it directly.
    pub async fn timer_tick(&self) {
        let now = (self.engine.clock_unix)();
        for t in self.engine.timers.due(now) {
            let Ok(m) = self.mission(Some(&t.mission_id)).await else { continue };
            if !self.engine.settings.enabled_agents.contains(&t.target) {
                let _ = m.append(EventEnvelope::new(&m.id, "timer.failed", json!({"timer_id": t.id, "reason": format!("`{}` is no longer an enabled role", t.target)}))).await;
                let _ = self.engine.timers.abandon(&t.id, "role not enabled");
                continue;
            }
            let fired = Fired::of(&t, now);
            let task_id = self.spawn_task(&m, &t.target, &timers::wake_prompt(&t, &fired), Some(Tier::parse(&t.ceiling).unwrap_or(Tier::L0)));
            let mut ev = EventEnvelope::new(&m.id, "timer.fired", json!({"timer_id": t.id, "task_id": task_id, "target": t.target, "creator": t.creator, "note": t.note, "due_unix": t.due_unix, "late_by_secs": fired.late_by_secs, "missed": fired.missed, "every_secs": t.every_secs}));
            ev.task_id = Some(task_id);
            let _ = m.append(ev).await;
            if let Err(e) = self.engine.timers.mark_fired(&t.id, now) {
                let _ = m.append(EventEnvelope::new(&m.id, "timer.failed", json!({"timer_id": t.id, "reason": e.0}))).await;
            }
        }
    }

    /// Start the once-a-second timer loop on the current runtime.
    pub fn start_timers(self: &Arc<Self>) {
        let svc = self.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(1));
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                every.tick().await;
                svc.timer_tick().await;
            }
        });
    }

    fn timers_json(&self) -> Value {
        let now = (self.engine.clock_unix)();
        let one = |t: &timers::Timer| {
            let mut v = t.to_json();
            v["relative"] = json!(timers::relative(t.due_unix, now));
            v
        };
        json!({"now": delphi_core::time::format_unix_millis(now, 0).replace(".000Z", "Z"), "timers": self.engine.timers.list().iter().map(one).collect::<Vec<_>>()})
    }

    /// `timer.set`: the operator sets a wake for a role. Console only.
    async fn timer_set(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let target = p["target"].as_str().unwrap_or("orchestrator").to_string();
        if !self.engine.settings.enabled_agents.contains(&target) {
            return Err(err(code::INVALID_PARAMS, format!("`{}` is not an enabled role (enabled: {})", target, self.engine.settings.enabled_agents.join(", "))));
        }
        let at_unix = match p["at"].as_str() {
            Some(s) => Some(delphi_core::time::parse_rfc3339(s).ok_or_else(|| err(code::INVALID_PARAMS, "`at` must be an RFC 3339 time"))?),
            None => None,
        };
        let req = timers::NewTimer { mission_id: m.id.clone(), target, note: p["note"].as_str().unwrap_or("").into(), creator: timers::OPERATOR.into(), ceiling: Tier::L5.as_str().into(), in_secs: p["in_secs"].as_i64(), at_unix, every_secs: p["every_secs"].as_i64() };
        let t = self.engine.timers.set(req, (self.engine.clock_unix)()).map_err(|e| err(code::INVALID_PARAMS, e.0))?;
        let _ = m.append(EventEnvelope::new(&m.id, "timer.set", t.to_json())).await;
        Ok(t.to_json())
    }

    /// `timer.cancel`: the operator cancels any pending timer. Console only.
    async fn timer_cancel(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let t = self.engine.timers.cancel(p["id"].as_str().unwrap_or(""), timers::OPERATOR).map_err(|e| err(code::NOT_FOUND, e.0))?;
        if let Ok(m) = self.mission(Some(&t.mission_id)).await {
            let _ = m.append(EventEnvelope::new(&m.id, "timer.cancelled", t.to_json())).await;
        }
        Ok(t.to_json())
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
        let optimistic = p["optimistic"].as_bool().unwrap_or(false) || self.engine.broker.mode() == Mode::Optimistic;
        if optimistic {
            // Refused before anything starts: the mode runs work ahead of the operator's review and needs the sandbox.
            self.engine.mark_optimistic(&task_id).map_err(|e| err(code::INVALID_PARAMS, e))?;
        }
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
        Ok(json!({"task_id": task_id, "mission_id": mid, "optimistic": optimistic}))
    }

    /// `decisions.list`: the provisional decisions of a mission (or one task), with their reviews.
    async fn decisions_list(&self, p: Value) -> RpcOutcome {
        let m = self.mission(p["mission_id"].as_str()).await?;
        let events = m.replay().map_err(|e| err(-32000, e.to_string()))?;
        let ds: Vec<_> = match p["task_id"].as_str() {
            Some(t) => crate::provisional::ledger(&events, t),
            None => crate::provisional::ledger_all(&events),
        };
        let pending = ds.iter().filter(|d| d.outcome == "allow" && d.review.is_none()).count();
        Ok(json!({"mission_id": m.id, "decisions": ds.iter().map(|d| d.to_json()).collect::<Vec<_>>(), "pending": pending}))
    }

    /// `decisions.review`: the operator ratifies or rejects one provisional decision. The review is bound to the
    /// decision's payload hash, so a review of one thing cannot ratify another, and only an allowed decision (the
    /// kind that enabled work) is reviewed.
    async fn decisions_review(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let (Some(id), Some(hash)) = (p["decision_id"].as_str(), p["payload_blake3"].as_str()) else {
            return Err(err(code::INVALID_PARAMS, "`decision_id` and `payload_blake3` are required"));
        };
        let resolution = match p["resolution"].as_str() {
            Some(r @ ("ratified" | "rejected")) => r,
            _ => return Err(err(code::INVALID_PARAMS, "`resolution` must be ratified or rejected")),
        };
        let events = m.replay().map_err(|e| err(-32000, e.to_string()))?;
        let Some(d) = crate::provisional::ledger_all(&events).into_iter().find(|d| d.id == id) else {
            return Err(err(code::NOT_FOUND, format!("no provisional decision `{}`", id)));
        };
        if d.outcome != "allow" {
            return Err(err(code::INVALID_PARAMS, "that decision refused something; there is nothing to review"));
        }
        if d.payload_blake3 != hash {
            return Err(err(code::INVALID_PARAMS, result_msg(id)));
        }
        let note: String = p["note"].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(500).collect();
        let by = format!("console-{}", ctx.client_id);
        let mut e = EventEnvelope::new(&m.id, "decision.reviewed", json!({"decision_id": id, "resolution": resolution, "note": note, "by": by, "payload_blake3": hash}));
        e.task_id = Some(d.task_id.clone());
        m.append(e).await.map_err(|e| err(-32000, e.to_string()))?;
        self.engine.decisions_changed.notify_one();
        Ok(json!({"decision_id": id, "resolution": resolution, "task_id": d.task_id}))
    }

    /// `build.merge`: run the gates and the merge card again for a build whose branch was kept, for example after
    /// the operator reviewed its provisional decisions. The result is logged as `build.merge_finished`.
    async fn build_merge(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let Some(task_id) = p["task_id"].as_str().filter(|t| t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !t.is_empty()).map(String::from) else {
            return Err(err(code::INVALID_PARAMS, "`task_id` is required (for example t_0003)"));
        };
        if self.running.lock().unwrap().contains_key(&task_id) {
            return Err(err(code::INVALID_PARAMS, format!("{} is still running", task_id)));
        }
        let events = m.replay().map_err(|e| err(-32000, e.to_string()))?;
        if !events.iter().any(|e| e.kind == "worktree.created" && e.task_id.as_deref() == Some(task_id.as_str())) {
            return Err(err(code::NOT_FOUND, format!("{} has no kept branch in this mission", task_id)));
        }
        self.running.lock().unwrap().insert(task_id.clone(), Running { target: "builder".into(), prompt: format!("merge {}", task_id), started: Instant::now() });
        let (engine, running, tid) = (self.engine.clone(), self.running.clone(), task_id.clone());
        let mm = m.clone();
        let h = tokio::spawn(async move {
            let (outcome, detail) = match engine.finalize_build(&mm, &tid).await {
                Ok(o) => {
                    let (a, b) = o.headline();
                    (a.to_string(), b)
                }
                Err(e) => ("failed".to_string(), engine.secrets.redact(&e.to_string())),
            };
            let mut ev = EventEnvelope::new(&mm.id, "build.merge_finished", json!({"outcome": outcome, "detail": detail}));
            ev.task_id = Some(tid.clone());
            let _ = mm.append(ev).await;
            running.lock().unwrap().remove(&tid);
        });
        self.aborts.lock().unwrap().insert(task_id.clone(), h.abort_handle());
        Ok(json!({"task_id": task_id, "mission_id": m.id}))
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
        let opts = crate::council::CouncilOpts { seats, author_family: p["author_family"].as_str().map(String::from), lenses: p["lenses"].as_bool().unwrap_or(false) };
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

    /// `approval.relay_resolve`: the answer a relay brings back from the operator's other channel (`specs/014` §2).
    /// Narrower than `approval.resolve` on purpose: it needs the relay scope, takes only `allow` or `deny` (never
    /// "allow similar"), and **cannot allow a Red card**, whatever the channel, so a stolen mailbox or chat
    /// account can deny anything but approve only what the operator already treats as routine. The answer is
    /// still bound to the payload hash and still loses to an earlier answer or a timeout.
    fn approval_relay_resolve(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        if ctx.scope != Scope::Relay {
            return Err(err(code::FORBIDDEN, "this method is for the relay scope; the console uses approval.resolve"));
        }
        let (Some(id), Some(hash)) = (p["approval_id"].as_str(), p["payload_blake3"].as_str()) else {
            return Err(err(code::INVALID_PARAMS, "`approval_id` and `payload_blake3` are required"));
        };
        let allow = match p["decision"].as_str() {
            Some("allow") => true,
            Some("deny") => false,
            _ => return Err(err(code::INVALID_PARAMS, "`decision` must be allow or deny")),
        };
        // A Red card can be denied by anyone the relay vouches for. Approving one also needs the operator's authenticator
        // code, which the daemon checks itself: a stolen mailbox or chat account does not have it.
        let mut stepped_up = false;
        if allow && self.engine.broker.get(id).is_some_and(|r| r.tier == crate::approval::Tier::Red) {
            let Some(cred) = self.engine.settings.relay_totp_credential.clone() else {
                return Err(err(code::FORBIDDEN, "a Red action is answered at the console, not through a relay (it can still be denied here); step-up is not set up"));
            };
            let Some(code_text) = p["totp"].as_str() else {
                return Err(err(code::FORBIDDEN, "this Red action also needs your authenticator code with the answer"));
            };
            let secret = totp_secret(&cred).map_err(|e| err(code::FORBIDDEN, format!("step-up is not usable: {}", e)))?;
            self.engine.totp.check(&secret, code_text.trim(), (self.engine.clock_unix)().max(0) as u64).map_err(|e| err(code::FORBIDDEN, e.to_string()))?;
            stepped_up = true;
        }
        // Who answered, for the log: the channel and the sender it vouches for, printable and short.
        let clean = |v: &Value, max: usize| -> String { v.as_str().unwrap_or("").chars().take(max).map(|c| if c.is_ascii_alphanumeric() || "@._:+-".contains(c) { c } else { '_' }).collect() };
        let by = format!("relay:{}:{}{}", clean(&p["channel"], 24), clean(&p["sender"], 80), if stepped_up { "+totp" } else { "" });
        let answer = if allow { Answer::Allow } else { Answer::Deny { reason: p["reason"].as_str().unwrap_or("").chars().take(500).collect() } };
        match self.engine.broker.resolve(id, hash, answer, &by) {
            Ok(r) => Ok(r.event_payload(id, &by)),
            Err(ResolveError::Gone { resolution, by: who }) => Err(RpcError { code: code::NOT_FOUND, message: format!("{}", ResolveError::Gone { resolution: resolution.clone(), by: who.clone() }), data: Some(json!({"by": who, "resolution": resolution.map(|r| r.event_payload(id, "")) })) }),
            Err(e) => Err(err(code::INVALID_PARAMS, e.to_string())),
        }
    }

    /// `models.get`: the pool as `delphi models` prints it, from the daemon's live state.
    fn models_get(&self) -> RpcOutcome {
        let cfg = self.engine.cfg.get();
        let now = (self.engine.clock_unix)();
        Ok(json!({"text": crate::models_cli::render(&cfg, &self.engine.pool.snapshot(), now), "configured": !cfg.pool_entries().is_empty()}))
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

    /// `observe.state`: the Observer's folded picture of the mission (agents, DAG, councils, builds, spend), computed
    /// here so every client shows the same thing. Live text is not in it: frames are never persisted.
    async fn observe_state(&self, p: Value) -> RpcOutcome {
        let m = self.mission(p["mission_id"].as_str()).await?;
        let mut events = m.replay().map_err(engine_err)?;
        let tools = crate::observe::tool_stats(&events);
        let adoption = crate::toolreport::report(&events);
        events.retain(|e| crate::observe::KINDS.contains(&e.kind.as_str()));
        let skip = events.len().saturating_sub(4000);
        let mut st = crate::observe::ObserverState::new(&self.engine.settings.enabled_agents);
        for e in events.into_iter().skip(skip) {
            st.apply(&serde_json::to_value(e).expect("events serialize"));
        }
        st.live.clear();
        let mut v = serde_json::to_value(&st).expect("state serializes");
        v["cache_hit_ratio"] = json!(st.cache_hit_ratio());
        v["default_view"] = json!(st.default_view());
        v["tools"] = serde_json::to_value(&tools).expect("tool stats serialize");
        v["token_cap"] = json!(self.engine.settings.harness_token_cap);
        v["adoption"] = serde_json::to_value(&adoption).expect("adoption serializes");
        v["mission_id"] = json!(m.id);
        v["budget"] = Self::budget_json(&m);
        v["pending_proposals"] = json!(crate::ratify::list(&self.engine.ws.home, &self.engine.ws.dir).len());
        Ok(v)
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
        let (ws, home, role) = (self.engine.ws.dir.clone(), self.engine.ws.home.clone(), p["role"].as_str().map(String::from));
        // Reranking reads the best results with a second model and adds about half a second; `"rerank": false` skips it.
        let rerank = p["rerank"].as_bool().unwrap_or(true);
        let hits = tokio::task::spawn_blocking(move || -> Result<Vec<crate::kb::Hit>, crate::kb::KbError> {
            let mut kb = crate::kb::Kb::open(&ws.join("local/state.db"))?;
            // A search never waits for the model to read the corpus, and never answers from a stale index: the words
            // and the release's preloaded vectors are brought up to date first (unchanged files cost a stat), and
            // vectors for anything new are made in the background and used from the next search on.
            let model = crate::kb::attach_default(&home, &mut kb)?;
            kb.refresh(&ws, "")?;
            if model {
                let (vectors, chunks) = kb.vector_counts()?;
                if vectors < chunks {
                    crate::kb::embed_in_background(&home, &ws);
                }
            }
            let mut opts = crate::kb::SearchOpts { role: role.clone(), ..Default::default() };
            if !rerank {
                opts.rerank_weight = 0.0;
            }
            let hits = kb.search(&query, &opts)?;
            crate::kb::log_search(&ws, &query, role.as_deref(), hits.len());
            Ok(hits)
        })
        .await
        .map_err(|e| err(-32000, e.to_string()))?
        .map_err(|e| err(-32000, e.to_string()))?;
        Ok(json!(hits.iter().map(|h| json!({"pointer": h.pointer, "heading": h.heading_path, "preview": h.preview, "blake3": h.blake3})).collect::<Vec<_>>()))
    }

    /// `kb.get`: the read-only knowledge API (`specs/028` §6a) over the daemon's socket, for the narrator's archives
    /// pane and the reader. `target` is a path and query such as `/v1/doc?path=specs/004.md`; the answer is the
    /// HTTP API's `{status, body}`. The index is brought up to date first, as for `kb.search`.
    async fn kb_get(&self, p: Value) -> RpcOutcome {
        let target = p["target"].as_str().unwrap_or("").to_string();
        if !target.starts_with("/v1/") || target.len() > 2048 {
            return Err(err(code::INVALID_PARAMS, "`target` must be a /v1/ path of at most 2048 characters"));
        }
        let (ws, home) = (self.engine.ws.dir.clone(), self.engine.ws.home.clone());
        let r = tokio::task::spawn_blocking(move || -> Result<crate::kbserve::Reply, crate::kb::KbError> {
            let mut kb = crate::kb::Kb::open(&ws.join("local/state.db"))?;
            let model = crate::kb::attach_default(&home, &mut kb)?;
            kb.refresh(&ws, "")?;
            if model {
                let (vectors, chunks) = kb.vector_counts()?;
                if vectors < chunks {
                    crate::kb::embed_in_background(&home, &ws);
                }
            }
            Ok(crate::kbserve::Reader { kb: &kb, ws: &ws, operator: true }.get(&target))
        })
        .await
        .map_err(|e| err(-32000, e.to_string()))?
        .map_err(|e| err(-32000, e.to_string()))?;
        Ok(json!({"status": r.status, "body": r.body}))
    }

    async fn kb_index(&self, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let (ws, home) = (self.engine.ws.dir.clone(), self.engine.ws.home.clone());
        let rep = tokio::task::spawn_blocking(move || -> Result<crate::kb::IndexReport, crate::kb::KbError> {
            let mut kb = crate::kb::Kb::open(&ws.join("local/state.db"))?;
            let model = crate::kb::attach_default(&home, &mut kb)?;
            // The words are searchable now, and so are the built-in documents' preloaded vectors; the rest follow
            // in the background.
            let rep = kb.refresh(&ws, "")?;
            if model {
                crate::kb::embed_in_background(&home, &ws);
            }
            Ok(rep)
        })
            .await
            .map_err(|e| err(-32000, e.to_string()))?
            .map_err(|e| err(-32000, e.to_string()))?;
        Ok(json!({"added": rep.added, "updated": rep.updated, "removed": rep.removed, "unchanged": rep.unchanged}))
    }

    /// `org.view` (`specs/026` §7.1): the added bundles, goals, perspectives and open items. A read, and no model call.
    fn org_view(&self, ctx: &CallContext) -> RpcOutcome {
        need_console(ctx)?;
        Ok(crate::org_context::view(&self.engine.ws, &self.engine.projects, &self.engine.now()[..10]))
    }

    /// `org.action`: the operator's explicit `add <dir>`, `update <name>` or `remove <name>` in the console. It runs the
    /// same `delphi bundle` command the shell does (one implementation of every check), shows what it printed and raises
    /// no approval card (`specs/024` OD-1). `rollback` and `map connector` do not exist yet and are refused by name.
    async fn org_action(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let word = |k: &str| -> Result<String, RpcError> {
            let v = p[k].as_str().map(str::trim).unwrap_or("");
            if v.is_empty() || v.len() > 4096 || v.starts_with('-') || v.contains('\0') {
                return Err(err(code::INVALID_PARAMS, format!("`{}` is required (and must not start with `-`)", k)));
            }
            Ok(v.to_string())
        };
        let ws = &self.engine.ws;
        let args: Vec<String> = match p["action"].as_str().unwrap_or("") {
            "add" => {
                let path = word("path")?;
                if !std::path::Path::new(&path).is_absolute() {
                    return Err(err(code::INVALID_PARAMS, "`path` must be an absolute directory"));
                }
                vec!["add".into(), path]
            }
            "remove" => vec!["remove".into(), word("name")?],
            // An update is adding again from the directory the bundle came from.
            "update" => {
                let name = word("name")?;
                let found: Vec<_> = delphi_core::bundles::active_entries(&ws.dir).into_iter().filter(|e| e.name == name || e.id() == name).collect();
                match found.as_slice() {
                    [] => return Err(err(code::INVALID_PARAMS, format!("no added bundle named `{}`", name))),
                    [e] if e.source.kind == "directory" && !e.source.path.starts_with('-') => vec!["add".into(), e.source.path.clone()],
                    [e] => return Err(err(code::INVALID_PARAMS, format!("{} came from a {} source, and only a directory can be added again", e.id(), e.source.kind))),
                    _ => return Err(err(code::INVALID_PARAMS, format!("`{}` is ambiguous; use org/name", name))),
                }
            }
            a @ ("rollback" | "map_connector") => return Err(err(code::INVALID_PARAMS, format!("`{}` is not available yet", a))),
            other => return Err(err(code::INVALID_PARAMS, format!("unknown org action `{}` (add, update, remove)", other))),
        };
        let exe = match self.bundle_exe.lock().unwrap().clone() {
            Some(e) => e,
            None => std::env::current_exe().map_err(|e| err(-32000, e.to_string()))?,
        };
        let ws_name = ws.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut cmd = tokio::process::Command::new(exe);
        cmd.arg("bundle").args(&args).arg("--workspace").arg(ws_name).arg("--home").arg(&ws.home).stdin(std::process::Stdio::null()).kill_on_drop(true);
        let out = tokio::time::timeout(Duration::from_secs(180), cmd.output()).await.map_err(|_| err(-32000, "the bundle command took too long and was stopped"))?.map_err(|e| err(-32000, format!("could not run the bundle command: {}", e)))?;
        let ok = out.status.success();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let output: String = crate::observe::sanitize(text.trim_end()).chars().take(8000).collect();
        if ok {
            // Tell the mission's log now, so the Narrator shows it before the next task starts.
            if let Ok(m) = self.mission(p["mission_id"].as_str()).await {
                let _ = self.engine.reconcile_bundles(&m).await;
            }
        }
        Ok(json!({"ok": ok, "code": out.status.code(), "output": output}))
    }

    fn ratify_list(&self) -> RpcOutcome {
        let ps = crate::ratify::list(&self.engine.ws.home, &self.engine.ws.dir);
        let today = self.engine.now()[..10].to_string();
        Ok(Value::Array(
            ps.iter()
                .map(|p| {
                    let mut v = json!({"id": p.id, "kind": p.kind.as_str(), "summary": p.summary, "live_blake3": p.live_blake3, "ratified_blake3": p.ratified_blake3, "diff": p.diff.chars().take(20_000).collect::<String>()});
                    // A spec the operator is about to approve is checked against the organization's goals and decisions it
                    // cites (`specs/026` §5, §7.2). Advice only: the approval is the operator's act and nothing here gates it.
                    if p.kind == crate::ratify::Kind::SpecApproval {
                        // An approval carries no diff: the spec being approved is read from its live file.
                        let text = std::fs::read_to_string(self.engine.ws.dir.join("team").join(&p.logical)).unwrap_or_default();
                        let (findings, _) = crate::align::prepass(&self.engine.ws, &text, &today);
                        if !findings.is_empty() {
                            v["alignment"] = json!(findings.iter().map(|f| f.line()).collect::<Vec<_>>());
                        }
                    }
                    v
                })
                .collect(),
        ))
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

    /// End the mission's current session and begin the next (`specs/030`): the one that ends leaves a digest in the central
    /// log, the new one is announced in the mission log so every client starts from the same place. Returns the new id, or the
    /// current one if nothing has been said since it began and the daemon has just restarted.
    async fn begin_session(&self, m: &Arc<MissionState>, reason: &str) -> Result<String, EngineError> {
        let events = m.replay()?;
        let (id, session) = crate::session::current(&events);
        let said = crate::session::has_content(session);
        if reason == "restart" && (events.is_empty() || !said && events.iter().any(|e| e.kind == "session.started")) {
            return Ok(id); // nothing to close, and a session is already open
        }
        if said {
            let workspace = self.engine.ws.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if let Some(d) = crate::session::digest(&m.id, &workspace, &id, reason, session) {
                let secrets = self.engine.secrets.clone();
                if let Err(e) = crate::session::append_central(&self.engine.ws.home, &d, &|s| secrets.redact(s)) {
                    self.notices.lock().unwrap().push(format!("could not write the session digest: {}", e));
                }
            }
        }
        let next = crate::session::next_id(&events);
        m.append(EventEnvelope::new(&m.id, "session.started", json!({"session_id": next, "reason": reason, "previous": id}))).await?;
        Ok(next)
    }

    /// `session.new`: the operator starts afresh. The console clears when it sees `session.started`; the model is told
    /// nothing of what came before, except a few lines about earlier sessions.
    async fn session_new(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let id = self.begin_session(&m, "new").await.map_err(engine_err)?;
        Ok(json!({"session_id": id, "mission_id": m.id}))
    }

    /// `session.context`: what the next task would be told of the conversation (`/context`). The operator's own
    /// conversation, so it is for the console only, and it carries the text itself.
    async fn session_context(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let m = self.mission(p["mission_id"].as_str()).await?;
        let events = m.replay().map_err(engine_err)?;
        let (session, _) = crate::session::current(&events);
        let budget = self.engine.settings.conversation_tokens;
        Ok(match self.engine.conversation_for(&m) {
            None => json!({"session": session, "budget_tokens": budget, "turns": 0, "digested": 0, "omitted": 0, "earlier": 0, "tokens": 0, "trimmed": false, "ceiling": "L5", "text": ""}),
            Some(p) => json!({"session": session, "budget_tokens": budget, "turns": p.turns, "digested": p.digested, "omitted": p.omitted, "earlier": p.earlier, "tokens": p.text.len() / 4, "trimmed": p.digested + p.omitted > 0, "ceiling": p.ceiling.as_str(), "text": p.text}),
        })
    }

    async fn snapshot(&self, p: Value) -> RpcOutcome {
        let m = self.mission(p["mission_id"].as_str()).await?;
        let running: Vec<Value> = self.running.lock().unwrap().iter().map(|(id, r)| json!({"task_id": id, "target": r.target, "prompt": r.prompt, "seconds": r.started.elapsed().as_secs()})).collect();
        Ok(json!({
            "mission_id": m.id,
            "session": m.replay().map(|e| crate::session::current(&e).0).unwrap_or_default(),
            "budget": Self::budget_json(&m),
            "mode": self.engine.broker.mode().as_str(),
            "pending_approvals": self.pending_json(),
            "recovery": self.recovery_json(&m.id),
            "pending_proposals": crate::ratify::list(&self.engine.ws.home, &self.engine.ws.dir).len(),
            "notices": self.notices.lock().unwrap().clone(),
            "running_tasks": running,
            "timers": self.timers_json()["timers"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["state"] == "pending").collect::<Vec<_>>(),
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
                "session.new" => svc.session_new(params, ctx).await,
                "session.context" => svc.session_context(params, ctx).await,
                "task.cancel" => svc.task_cancel(params, ctx).await,
                "timer.list" => Ok(svc.timers_json()),
                "timer.set" => svc.timer_set(params, ctx).await,
                "timer.cancel" => svc.timer_cancel(params, ctx).await,
                "build.start" => svc.build_start(params, ctx).await,
                "build.merge" => svc.build_merge(params, ctx).await,
                "decisions.list" => svc.decisions_list(params).await,
                "decisions.review" => svc.decisions_review(params, ctx).await,
                "council.start" => svc.council_start(params, ctx).await,
                "kb.search" => svc.kb_search(params).await,
                "kb.get" => svc.kb_get(params).await,
                "observe.snapshot" => svc.observe_snapshot(params).await,
                "observe.state" => svc.observe_state(params).await,
                "observe.diff" => svc.observe_diff(params).await,
                "kb.index" => svc.kb_index(ctx).await,
                "approval.list" => Ok(svc.pending_json()),
                "approval.resolve" => svc.approval_resolve(params, ctx),
                "approval.relay_resolve" => svc.approval_relay_resolve(params, ctx),
                "usage.get" => svc.usage_get(params).await,
                "models.get" => svc.models_get(),
                "budget.get" => svc.mission(params["mission_id"].as_str()).await.map(|m| EngineService::budget_json(&m)),
                "budget.set" => svc.budget_set(params, ctx).await,
                "permissions.get" => Ok(json!({"mode": svc.engine.broker.mode().as_str(), "allow_similar": svc.engine.broker.allow_similar().iter().map(|(t, tool, pat)| json!({"task_id": t, "tool": tool, "pattern": pat})).collect::<Vec<_>>()})),
                "permissions.set" => (|| {
                    need_console(&ctx)?;
                    let mode = params["mode"].as_str().and_then(Mode::parse).ok_or_else(|| err(code::INVALID_PARAMS, "`mode` must be default, accept-edits, mission-auto or optimistic"))?;
                    if mode == Mode::Optimistic && svc.engine.exec.sandbox.is_none() {
                        return Err(err(code::INVALID_PARAMS, "optimistic mode needs the OS sandbox, and none is available on this machine"));
                    }
                    svc.engine.broker.set_mode(mode);
                    Ok(json!({"mode": mode.as_str()}))
                })(),
                "session.snapshot" => svc.snapshot(params).await,
                "org.view" => svc.org_view(&ctx),
                "org.action" => svc.org_action(params, ctx).await,
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
        relay_sock: PathBuf,
        console: String,
        narrator: String,
        relay: String,
        _dir: tempfile::TempDir,
    }

    async fn serve(f: &Fx) -> Served {
        serve_with(f, None).await
    }

    async fn serve_with(f: &Fx, queue_cap: Option<usize>) -> Served {
        let dir = tempfile::Builder::new().prefix("dl").tempdir_in("/tmp").unwrap();
        let sock = dir.path().join("d.sock");
        let narrator_sock = dir.path().join("n.sock");
        let relay_sock = dir.path().join("r.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let narrator_listener = tokio::net::UnixListener::bind(&narrator_sock).unwrap();
        let relay_listener = tokio::net::UnixListener::bind(&relay_sock).unwrap();
        let mut cfg = DaemonConfig::new(dir.path().to_path_buf(), dir.path().to_path_buf());
        cfg.tty_policy = TtyPolicy::Disabled;
        if let Some(c) = queue_cap {
            cfg.queue_cap = c;
        }
        let daemon = Daemon::with_hub(cfg, f.engine.hub.clone()).unwrap();
        let svc = EngineService::new(f.engine.clone(), "m1");
        svc.adopt(f.m.clone());
        // `org.action` runs `delphi bundle …`; here `echo` stands in and prints the command line it was given.
        svc.set_bundle_exe(PathBuf::from("/bin/echo"));
        daemon.set_extension(svc.extension());
        let (console, narrator, relay) = (daemon.console_token().to_string(), daemon.narrator_token().to_string(), daemon.relay_token().to_string());
        tokio::spawn(daemon.serve_all(listener, Some(narrator_listener), Some(relay_listener)));
        Served { sock, narrator_sock, relay_sock, console, narrator, relay, _dir: dir }
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

    fn central_lines(f: &Fx) -> Vec<Value> {
        fs::read_to_string(crate::session::log_path(&f.engine.ws.home)).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    // specs/030 AC-2, AC-4: a follow-up is told what was said in the session; after `/new` it is not, except for a line about the one before.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_follow_up_is_told_the_session_so_far_and_a_new_session_starts_clean_with_a_digest_in_the_central_log() {
        let f = setup(vec![Step::text("Option one is Redis, option two is Postgres."), Step::text("Going with Postgres."), Step::text("Nothing earlier here.")], 5.0, |_| {});
        let svc = EngineService::new(f.engine.clone(), "m1");
        svc.adopt(f.m.clone());
        f.engine.run_task(&f.m, "orchestrator", "list two queue options").await.unwrap();
        f.engine.run_task(&f.m, "orchestrator", "do the second one").await.unwrap();
        let reqs = f.model.requests();
        assert!(!reqs[0].body.to_string().contains("Conversation so far"), "the first task has nothing to be told");
        let second = reqs[1].body.to_string();
        assert!(second.contains("Conversation so far") && second.contains("list two queue options") && second.contains("option two is Postgres"), "{}", second);

        let id = svc.begin_session(&f.m, "new").await.unwrap();
        assert_eq!(id, "s_0001");
        f.engine.run_task(&f.m, "orchestrator", "what were we doing?").await.unwrap();
        let third = f.model.requests()[2].body.to_string();
        assert!(!third.contains("option two is Postgres"), "the new session does not carry the old conversation: {}", third);
        assert!(third.contains("Earlier session") && third.contains("list two queue options"), "but it is told in a line what the last one was about: {}", third);

        let lines = central_lines(&f);
        assert_eq!(lines.len(), 1, "{:?}", lines);
        assert_eq!((lines[0]["session_id"].as_str(), lines[0]["ended_by"].as_str(), lines[0]["turns"].as_u64()), (Some("s_0000"), Some("new"), Some(2)));
        assert_eq!(events_of(&f, "session.started").len(), 1);
        assert_eq!(svc.begin_session(&f.m, "new").await.unwrap(), "s_0002", "a session with nothing in it is still closed, and leaves no digest");
        assert_eq!(central_lines(&f).len(), 2, "the third task's session had one ask");
    }

    // specs/030 §6: `/context` says what the next task would be told, and nothing for a session with nothing in it.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_context_report_says_what_the_next_task_would_be_told() {
        let f = setup(vec![Step::text("Redis or Postgres.")], 5.0, |_| {});
        let svc = EngineService::new(f.engine.clone(), "m1");
        svc.adopt(f.m.clone());
        let console = || CallContext { scope: Scope::Console, client_id: 0 };
        let empty = svc.session_context(json!({}), console()).await.unwrap();
        assert_eq!((empty["turns"].as_u64(), empty["text"].as_str(), empty["trimmed"].as_bool()), (Some(0), Some(""), Some(false)));
        f.engine.run_task(&f.m, "orchestrator", "list two queue options").await.unwrap();
        let r = svc.session_context(json!({}), console()).await.unwrap();
        assert_eq!((r["turns"].as_u64(), r["session"].as_str(), r["ceiling"].as_str()), (Some(2), Some("s_0000"), Some("L5")));
        assert!(r["text"].as_str().unwrap().contains("Redis or Postgres.") && r["tokens"].as_u64().unwrap() > 0, "{}", r);
        assert!(svc.session_context(json!({}), CallContext { scope: Scope::Narrator, client_id: 0 }).await.is_err(), "the narrator may not read the conversation");
    }

    // specs/030 D-1: a restart ends the session; a restart with nothing said does not pile up empty sessions.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_restart_ends_the_session_once_and_only_when_something_was_said() {
        let f = setup(vec![Step::text("An answer.")], 5.0, |_| {});
        f.engine.run_task(&f.m, "orchestrator", "a question").await.unwrap();
        let f = f.restart(vec![Step::text("Another.")]);
        let svc = EngineService::new(f.engine.clone(), "m1");
        svc.adopt(f.m.clone());
        assert_eq!(svc.begin_session(&f.m, "restart").await.unwrap(), "s_0001");
        assert_eq!(central_lines(&f)[0]["ended_by"], "restart");
        assert_eq!(svc.begin_session(&f.m, "restart").await.unwrap(), "s_0001", "a second restart with nothing said starts nothing");
        assert_eq!(events_of(&f, "session.started").len(), 1);
        assert_eq!(central_lines(&f).len(), 1);
        f.engine.run_task(&f.m, "orchestrator", "after the restart").await.unwrap();
        let body = f.model.requests()[0].body.to_string();
        assert!(!body.contains("An answer.") && body.contains("Earlier session"), "{}", body);
    }

    // specs/026 AC-7: `/org` reads the view and each action is the operator's explicit act: it runs the bundle command,
    // returns what it printed, raises no card, and the log learns of the change at once.
    #[tokio::test(flavor = "multi_thread")]
    async fn org_view_and_org_action_serve_the_console_run_the_bundle_command_and_raise_no_card() {
        use crate::bundle_load::tests::{add_bundle, FM};
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let goals = format!("{}## [G-acme-01] Cut p99 checkout latency\n- **Horizon:** 2026-Q4 to 2999-Q1\n- **Measure:** p99 weekly\n- **Status:** active\n", FM);
        add_bundle(&f.engine.ws, "acme", "plat", 10, "2026-10-01", &[("behavior/goals.md", &goals), ("bundle.toml", "[bundle]\nschema = 1\n")]);
        let ws_name = f.engine.ws.dir.file_name().unwrap().to_string_lossy().to_string();
        let s = serve(&f).await;
        let (sock, tok, nsock, ntok) = (s.sock.clone(), s.console.clone(), s.narrator_sock.clone(), s.narrator.clone());
        let (view, add, update, remove, refused, narrator, pending) = blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            let view = c.call("org.view", json!({})).unwrap();
            let add = c.call("org.action", json!({"action": "add", "path": "/srv/bundles/acme"})).unwrap();
            let update = c.call("org.action", json!({"action": "update", "name": "plat"})).unwrap();
            let remove = c.call("org.action", json!({"action": "remove", "name": "acme/plat"})).unwrap();
            let mut refused = Vec::new();
            for p in [json!({"action": "add", "path": "relative/dir"}), json!({"action": "add", "path": "--yes"}), json!({"action": "remove", "name": "-x"}), json!({"action": "update", "name": "nope"}), json!({"action": "rollback", "name": "plat"}), json!({"action": "map_connector"}), json!({"action": "publish"})] {
                refused.push(rpc_code(c.call("org.action", p)));
            }
            let pending = c.call("approval.list", json!({})).unwrap();
            let mut n = Client::connect(&nsock).unwrap();
            n.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            n.hello(&ntok).unwrap();
            let narrator = (rpc_code(n.call("org.view", json!({}))), rpc_code(n.call("org.action", json!({"action": "remove", "name": "plat"}))));
            (view, add, update, remove, refused, narrator, pending)
        })
        .await;
        assert_eq!((view["bundles"][0]["id"].as_str(), view["goals"][0]["id"].as_str(), view["unavailable"][0].as_str()), (Some("acme/plat"), Some("G-acme-01"), Some("rollback")));
        let tail = format!("--workspace {} --home {}", ws_name, f.engine.ws.home.display());
        assert_eq!((add["ok"].as_bool(), add["output"].as_str()), (Some(true), Some(format!("bundle add /srv/bundles/acme {}", tail).as_str())));
        assert_eq!(update["output"].as_str(), Some(format!("bundle add /x {}", tail).as_str()), "an update adds again from the recorded source directory");
        assert_eq!(remove["output"].as_str(), Some(format!("bundle remove acme/plat {}", tail).as_str()));
        assert_eq!(refused, vec![Some(code::INVALID_PARAMS); 7], "{:?}", refused);
        assert_eq!(narrator, (Some(code::FORBIDDEN), Some(code::FORBIDDEN)), "the narrator reads nothing of the organization and changes nothing");
        assert_eq!(pending, json!([]), "no action raises an approval card");
        assert_eq!(events_of(&f, "bundle.added").len(), 1, "the log learns of the added bundle without waiting for a task");
        assert_eq!(f.model.requests().len(), 0, "no model call");
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
            for (m, p) in [("task.submit", json!({"prompt": "hi"})), ("task.cancel", json!({})), ("build.start", json!({"prompt": "hi"})), ("council.start", json!({"proposition": "hi"})), ("kb.index", json!({})), ("org.action", json!({"action": "remove", "name": "x"})), ("org.view", json!({})), ("approval.resolve", json!({"approval_id": "a", "payload_blake3": "b", "decision": "allow"})), ("budget.set", json!({"usd": 99})), ("permissions.set", json!({"mode": "mission-auto"})), ("mission.open", json!({})), ("session.new", json!({})), ("session.context", json!({}))] {
                assert_eq!(rpc_code(c.call(m, p)), Some(code::FORBIDDEN), "{}", m);
            }
            for m in ["usage.get", "approval.list", "permissions.get", "budget.get", "session.snapshot", "observe.snapshot", "observe.state", "observe.diff"] {
                assert!(c.call(m, json!({})).is_ok(), "{} should be readable", m);
            }
            assert_eq!(rpc_code(c.call("no.such.method", json!({}))), Some(code::FORBIDDEN), "an unknown method is refused, not looked up");
            assert_eq!(c.call("session.ping", json!({})).unwrap(), json!("pong"));
        })
        .await;
        assert_eq!(f.model.requests().len(), 0, "the narrator started nothing");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_console_sets_lists_and_cancels_timers_and_nothing_else_can() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok, nsock, ntok, rsock, rtok) = (s.sock.clone(), s.console.clone(), s.narrator_sock.clone(), s.narrator.clone(), s.relay_sock.clone(), s.relay.clone());
        blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            let t = c.call("timer.set", json!({"target": "orchestrator", "in_secs": 600, "note": "look at CI"})).unwrap();
            assert_eq!((t["id"].as_str(), t["creator"].as_str(), t["ceiling"].as_str()), (Some("tm_0001"), Some("operator"), Some("L5")));
            assert_eq!(rpc_code(c.call("timer.set", json!({"target": "nobody", "in_secs": 60, "note": "x"}))), Some(code::INVALID_PARAMS), "an unknown role");
            assert_eq!(rpc_code(c.call("timer.set", json!({"target": "orchestrator", "in_secs": 0, "note": "x"}))), Some(code::INVALID_PARAMS));
            assert_eq!(rpc_code(c.call("timer.set", json!({"target": "orchestrator", "at": "later", "note": "x"}))), Some(code::INVALID_PARAMS));
            let l = c.call("timer.list", json!({})).unwrap();
            assert_eq!((l["timers"].as_array().unwrap().len(), l["timers"][0]["state"].as_str()), (1, Some("pending")));
            assert!(l["timers"][0]["relative"].as_str().unwrap().starts_with("in 10m"), "{}", l["timers"][0]);
            assert_eq!(c.call("session.snapshot", json!({})).unwrap()["timers"][0]["id"], "tm_0001", "the snapshot carries pending timers for the console");
            assert_eq!(rpc_code(c.call("timer.cancel", json!({"id": "tm_9999"}))), Some(code::NOT_FOUND));
            assert_eq!(c.call("timer.cancel", json!({"id": "tm_0001"})).unwrap()["state"], "cancelled");
            assert!(c.call("session.snapshot", json!({})).unwrap()["timers"].as_array().unwrap().is_empty());

            for (path, token) in [(nsock, ntok), (rsock, rtok)] {
                let mut other = Client::connect(&path).unwrap();
                other.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                other.hello(&token).unwrap();
                for (m, p) in [("timer.set", json!({"target": "orchestrator", "in_secs": 60, "note": "x"})), ("timer.cancel", json!({"id": "tm_0001"})), ("timer.list", json!({}))] {
                    assert_eq!(rpc_code(other.call(m, p)), Some(code::FORBIDDEN), "{} on {}", m, path.display());
                }
            }
        })
        .await;
        assert_eq!(events_of(&f, "timer.set").len(), 1);
        assert_eq!(events_of(&f, "timer.cancelled").len(), 1);
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
        assert_eq!(listed, vec!["approval.list", "budget.get", "events.subscribe", "kb.get", "kb.search", "observe.diff", "observe.snapshot", "observe.state", "permissions.get", "ratify.list", "session.ping", "session.snapshot", "usage.get"]);
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.narrator_sock.clone(), s.narrator.clone());
        blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            let every_mutating_method_the_daemon_has = ["task.submit", "build.start", "council.start", "kb.index", "org.action", "org.view", "approval.resolve", "budget.set", "permissions.set", "mission.open", "session.new", "session.context", "recovery.resume", "recovery.pause", "ratify.apply", "ratify.reject", "mission.rewind", "daemon.shutdown", "auth.issue", "hello", "events.replay", "shell.run", "anything.else"];
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

    // ── the relay scope (specs/014) ──────────────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn each_socket_accepts_only_its_own_token_and_the_relay_socket_issues_only_relay_tokens() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let (c_sock, n_sock, r_sock) = (s.sock.clone(), s.narrator_sock.clone(), s.relay_sock.clone());
        let (c_tok, n_tok, r_tok) = (s.console.clone(), s.narrator.clone(), s.relay.clone());
        blocking(move || {
            let try_hello = |sock: &PathBuf, tok: &str| {
                let mut c = Client::connect(sock).unwrap();
                c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                c.hello(tok).is_ok()
            };
            assert!(try_hello(&r_sock, &r_tok) && try_hello(&c_sock, &c_tok) && try_hello(&n_sock, &n_tok));
            for (sock, tok, what) in [(&r_sock, &c_tok, "console token on the relay socket"), (&r_sock, &n_tok, "narrator token on the relay socket"), (&c_sock, &r_tok, "relay token on the console socket"), (&n_sock, &r_tok, "relay token on the narrator socket")] {
                assert!(!try_hello(sock, tok), "{} must be refused", what);
            }
            let mut c = Client::connect(&r_sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            assert_eq!(rpc_code(c.call("auth.issue", json!({"scope": "console"}))), Some(code::FORBIDDEN), "the relay socket cannot hand out a console token");
            assert_eq!(rpc_code(c.call("auth.issue", json!({"scope": "narrator"}))), Some(code::FORBIDDEN));
            assert!(c.issue_token(delphi_core::proto::Scope::Relay).is_ok(), "it issues its own kind");
            let mut c = Client::connect(&c_sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            assert_eq!(rpc_code(c.call("auth.issue", json!({"scope": "relay"}))), Some(code::FORBIDDEN), "and the console socket cannot issue one");
            let mut c = Client::connect(&r_sock).unwrap();
            assert_eq!(c.hello(&r_tok).unwrap()["scope"], "relay");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_relay_socket_answers_a_fixed_short_list_and_refuses_everything_else_by_name() {
        let mut listed: Vec<&str> = delphi_core::server::RELAY_METHODS.to_vec();
        listed.sort();
        assert_eq!(listed, vec!["approval.list", "approval.relay_resolve", "budget.get", "events.subscribe", "session.ping"], "the list is the contract; adding to it is a decision");
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.relay_sock.clone(), s.relay.clone());
        blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            let everything_a_relay_must_not_reach = ["task.submit", "build.start", "council.start", "kb.index", "org.action", "org.view", "kb.search", "kb.get", "approval.resolve", "budget.set", "permissions.set", "permissions.get", "mission.open", "session.new", "session.context", "recovery.resume", "ratify.apply", "ratify.reject", "ratify.list", "mission.rewind", "daemon.shutdown", "usage.get", "session.snapshot", "observe.snapshot", "observe.state", "observe.diff", "models.get", "anything.else"];
            for m in everything_a_relay_must_not_reach {
                assert_eq!(rpc_code(c.call(m, json!({"prompt": "x", "mission_id": "m1"}))), Some(code::FORBIDDEN), "{}", m);
            }
            for m in delphi_core::server::RELAY_METHODS {
                assert_ne!(rpc_code(c.call(m, json!({}))), Some(code::FORBIDDEN), "{} is on the list, so it must be answered", m);
            }
        })
        .await;
        assert_eq!(f.model.requests().len(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_relay_is_sent_cards_and_outcomes_but_never_what_an_agent_says() {
        let f = setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-relay.md", "content": "# Relay\nbody"})), Step::text("A very private model reply.")], 5.0, |_| {});
        let s = serve(&f).await;
        let (c_sock, r_sock, c_tok, r_tok) = (s.sock.clone(), s.relay_sock.clone(), s.console.clone(), s.relay.clone());
        let kinds = blocking(move || {
            let mut relay = Client::connect(&r_sock).unwrap();
            relay.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            relay.hello(&r_tok).unwrap();
            relay.call("events.subscribe", json!({})).unwrap();
            let mut console = Client::connect(&c_sock).unwrap();
            console.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            console.hello(&c_tok).unwrap();
            console.call("task.submit", json!({"target": "architect", "prompt": "Draft a spec"})).unwrap();
            let mut kinds: Vec<String> = Vec::new();
            let mut answered = false;
            loop {
                let n = relay.next_notification().unwrap();
                assert_ne!(n["method"], "delta.frame", "live model text must not reach a relay");
                if n["method"] != "event" {
                    continue;
                }
                let e = &n["params"];
                kinds.push(e["type"].as_str().unwrap().to_string());
                assert!(!e.to_string().contains("private model reply"), "model text leaked to the relay: {}", e);
                if e["type"] == "approval.requested" && !answered {
                    answered = true;
                    let p = &e["payload"];
                    let r = relay.call("approval.relay_resolve", json!({"approval_id": p["approval_id"], "payload_blake3": p["payload_blake3"], "decision": "allow", "channel": "email", "sender": "me@example.com"})).unwrap();
                    assert_eq!(r["resolution"], "approved");
                }
                if e["type"] == "task.completed" {
                    break;
                }
            }
            kinds
        })
        .await;
        for k in &kinds {
            assert!(delphi_core::hub::RELAY_EVENTS.contains(&k.as_str()), "a relay was sent `{}`", k);
        }
        assert!(kinds.contains(&"approval.requested".to_string()) && kinds.contains(&"approval.resolved".to_string()) && kinds.contains(&"task.completed".to_string()), "{:?}", kinds);
        assert!(f.root.join("workspaces/w/team/specs/009-relay.md").exists(), "the relay's answer approved the write");
        let resolved = events_of(&f, "approval.resolved");
        assert_eq!(resolved[0].payload["by"], "relay:email:me@example.com", "the log says who answered: {}", resolved[0].payload);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_relay_can_approve_amber_deny_anything_and_never_approve_red() {
        use crate::approval::{Request, Tier};
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let mk = |tier: Tier, tool: &str| Request { approval_id: String::new(), task_id: "t_0001".into(), span_id: "sp1".into(), role: "architect".into(), tier, tool: tool.into(), pattern: tool.into(), worktree: None, summary: "s".into(), payload: json!({"tool": tool}), payload_blake3: String::new(), timeout: Duration::from_secs(60) };
        let amber = f.engine.broker.request(mk(Tier::Amber, "write_file"));
        let red = f.engine.broker.request(mk(Tier::Red, "merge"));
        let red2 = f.engine.broker.request(mk(Tier::Red, "pull_request"));
        let amber2 = f.engine.broker.request(mk(Tier::Amber, "write_file"));
        let hash = |id: &str| f.engine.broker.get(id).unwrap().payload_blake3;
        let ids = [amber.approval_id.clone(), red.approval_id.clone(), red2.approval_id.clone(), amber2.approval_id.clone()];
        let hashes: Vec<String> = ids.iter().map(|i| hash(i)).collect();
        let (r_sock, r_tok, c_sock, c_tok) = (s.relay_sock.clone(), s.relay.clone(), s.sock.clone(), s.console.clone());
        let ids_in = ids.clone();
        blocking(move || {
            let ids = ids_in;
            let mut r = Client::connect(&r_sock).unwrap();
            r.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            r.hello(&r_tok).unwrap();
            let call = |r: &mut Client, i: usize, decision: &str| r.call("approval.relay_resolve", json!({"approval_id": ids[i], "payload_blake3": hashes[i], "decision": decision, "channel": "email", "sender": "me@example.com"}));
            // Red: allow is refused by the daemon, whoever asks; the card is still waiting.
            assert_eq!(rpc_code(call(&mut r, 1, "allow")), Some(code::FORBIDDEN));
            assert_eq!(rpc_code(call(&mut r, 2, "allow")), Some(code::FORBIDDEN));
            // "Allow similar" is not a relay decision at all.
            assert_eq!(rpc_code(call(&mut r, 0, "allow_similar")), Some(code::INVALID_PARAMS));
            // A wrong payload hash approves nothing.
            let bad = r.call("approval.relay_resolve", json!({"approval_id": ids[0], "payload_blake3": "0000", "decision": "allow", "channel": "email", "sender": "x"}));
            assert!(bad.is_err(), "a changed payload is a new approval");
            // Amber: allowed. Red: denied is fine.
            assert_eq!(call(&mut r, 0, "allow").unwrap()["resolution"], "approved");
            assert!(call(&mut r, 1, "deny").is_ok());
            // The first answer wins: a second answer to the same card learns it lost.
            assert_eq!(rpc_code(call(&mut r, 0, "deny")), Some(code::NOT_FOUND));
            // The console's method is not the relay's, and the relay's is not the console's.
            let mut c = Client::connect(&c_sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&c_tok).unwrap();
            assert_eq!(rpc_code(c.call("approval.relay_resolve", json!({"approval_id": ids[3], "payload_blake3": hashes[3], "decision": "allow"}))), Some(code::FORBIDDEN), "a console uses approval.resolve");
        })
        .await;
        assert!(matches!(amber.wait().await, crate::approval::Resolution::Approved { .. }));
        assert!(matches!(red.wait().await, crate::approval::Resolution::Denied { .. }));
        assert!(f.engine.broker.get(&ids[2]).is_some() && f.engine.broker.get(&ids[3]).is_some(), "the refused and untouched cards are still waiting");
        drop((red2, amber2));
    }

    // ── provisional decisions (specs/015) ────────────────────────────────────

    async fn seed_decision(f: &Fx, id: &str, allow: bool) -> String {
        let c = crate::tools::Classified { tool: "write_file".into(), tier: crate::tools::ToolTier::Amber, kind: crate::tools::ActionKind::WorktreeEdit, summary: format!("write src/{}.rs", id), payload: json!({"tool": "write_file", "path": format!("@worktree/src/{}.rs", id), "bytes": 3, "blake3": "ab"}), pattern: "write_file @worktree/src/*".into() };
        let v = crate::provisional::Verdict::by_rule(allow, "ok".into(), crate::provisional::Risk::Routine);
        let ev = crate::provisional::decision_event(id, &c, crate::approval::Tier::Amber, &v, "builder");
        let hash = ev["payload_blake3"].as_str().unwrap().to_string();
        let mut e = EventEnvelope::new(&f.m.id, "decision.provisional", ev);
        e.task_id = Some("t_0001".into());
        e.agent = Some("builder".into());
        f.m.append(e).await.unwrap();
        hash
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn decisions_are_listed_and_reviewed_by_the_console_only_and_a_review_is_bound_to_the_payload_hash() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let hash_a = seed_decision(&f, "dec_a", true).await;
        let _ = seed_decision(&f, "dec_b", false).await;
        let s = serve(&f).await;
        let (sock, tok, n_sock, n_tok, r_sock, r_tok) = (s.sock.clone(), s.console.clone(), s.narrator_sock.clone(), s.narrator.clone(), s.relay_sock.clone(), s.relay.clone());
        blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            let l = c.call("decisions.list", json!({})).unwrap();
            assert_eq!((l["pending"].as_u64(), l["decisions"].as_array().unwrap().len()), (Some(1), 2), "the refusal is listed but needs no review: {}", l);
            let review = |c: &mut Client, id: &str, hash: &str, res: &str| c.call("decisions.review", json!({"decision_id": id, "payload_blake3": hash, "resolution": res, "note": "looked fine"}));
            assert_eq!(rpc_code(review(&mut c, "dec_missing", &hash_a, "ratified")), Some(code::NOT_FOUND));
            assert_eq!(rpc_code(review(&mut c, "dec_a", "0000", "ratified")), Some(code::INVALID_PARAMS), "a changed payload is a new decision");
            assert_eq!(rpc_code(review(&mut c, "dec_a", &hash_a, "maybe")), Some(code::INVALID_PARAMS));
            assert_eq!(rpc_code(review(&mut c, "dec_b", "x", "ratified")), Some(code::INVALID_PARAMS), "there is nothing to review in a refusal");
            let ok = review(&mut c, "dec_a", &hash_a, "ratified").unwrap();
            assert_eq!((ok["decision_id"].as_str(), ok["resolution"].as_str()), (Some("dec_a"), Some("ratified")));
            let l = c.call("decisions.list", json!({"task_id": "t_0001"})).unwrap();
            assert_eq!(l["pending"], 0);
            assert_eq!(l["decisions"][0]["review"]["resolution"], "ratified");
            assert_eq!(l["decisions"][0]["review"]["note"], "looked fine");
            // The narrator and the relay cannot review (the methods are not on their lists), and cannot even list.
            for (sock, tok) in [(&n_sock, &n_tok), (&r_sock, &r_tok)] {
                let mut c = Client::connect(sock).unwrap();
                c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                c.hello(tok).unwrap();
                assert_eq!(rpc_code(c.call("decisions.review", json!({"decision_id": "dec_a", "payload_blake3": "x", "resolution": "ratified"}))), Some(code::FORBIDDEN));
                assert_eq!(rpc_code(c.call("build.merge", json!({"task_id": "t_0001"}))), Some(code::FORBIDDEN));
            }
        })
        .await;
        let reviewed = events_of(&f, "decision.reviewed");
        assert_eq!(reviewed.len(), 1);
        assert!(reviewed[0].payload["by"].as_str().unwrap().starts_with("console-") && reviewed[0].task_id.as_deref() == Some("t_0001"));
    }

    // ── step-up: a Red answer through a relay needs the authenticator code (specs/014 §7) ──

    /// base32 of the RFC 6238 test secret "12345678901234567890".
    const TEST_SECRET_B32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    fn with_stepup(secret_cmd: &str) -> Fx {
        let cred = format!(r#"command:["/bin/echo","{{\"token\":\"{}\"}}"]"#, secret_cmd);
        setup(vec![Step::text("x")], 5.0, move |s| s.relay_totp_credential = Some(cred))
    }

    fn red_card(f: &Fx, tool: &str) -> (crate::approval::Waiting, String, String) {
        use crate::approval::{Request, Tier};
        let w = f.engine.broker.request(Request { approval_id: String::new(), task_id: "t_0001".into(), span_id: "sp1".into(), role: "architect".into(), tier: Tier::Red, tool: tool.into(), pattern: tool.into(), worktree: None, summary: "s".into(), payload: json!({"tool": tool}), payload_blake3: String::new(), timeout: Duration::from_secs(60) });
        let id = w.approval_id.clone();
        let hash = f.engine.broker.get(&id).unwrap().payload_blake3;
        (w, id, hash)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_relay_can_approve_a_red_card_only_with_a_valid_authenticator_code_that_works_once() {
        let f = with_stepup(TEST_SECRET_B32);
        let s = serve(&f).await;
        let (w1, id1, h1) = red_card(&f, "merge");
        let (w2, id2, h2) = red_card(&f, "pull_request");
        let now = (f.engine.clock_unix)() as u64;
        let good = delphi_core::totp::code_at(b"12345678901234567890", now);
        let wrong = if good == "000000" { "000001".to_string() } else { "000000".to_string() };
        let (sock, tok) = (s.relay_sock.clone(), s.relay.clone());
        let good2 = good.clone();
        blocking(move || {
            let mut r = Client::connect(&sock).unwrap();
            r.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            r.hello(&tok).unwrap();
            let ask = |r: &mut Client, id: &str, hash: &str, decision: &str, totp: Option<&str>| {
                let mut p = json!({"approval_id": id, "payload_blake3": hash, "decision": decision, "channel": "email", "sender": "me@example.com"});
                if let Some(t) = totp {
                    p["totp"] = json!(t);
                }
                r.call("approval.relay_resolve", p)
            };
            let msg = |e: Result<Value, ClientError>| match e {
                Err(ClientError::Rpc { code: c, message, .. }) => (c, message),
                other => panic!("expected a refusal, got {:?}", other),
            };
            let (c, m) = msg(ask(&mut r, &id1, &h1, "allow", None));
            assert_eq!(c, code::FORBIDDEN);
            assert!(m.contains("authenticator code"), "{}", m);
            let (c, m) = msg(ask(&mut r, &id1, &h1, "allow", Some(&wrong)));
            assert!(c == code::FORBIDDEN && m.contains("wrong"), "{}", m);
            // The right code approves, and the log says it was stepped up.
            let ok = ask(&mut r, &id1, &h1, "allow", Some(&good2)).unwrap();
            assert_eq!(ok["resolution"], "approved");
            assert_eq!(ok["by"], "relay:email:me@example.com+totp");
            // The same code again, for another card, is a replay.
            let (c, m) = msg(ask(&mut r, &id2, &h2, "allow", Some(&good2)));
            assert!(c == code::FORBIDDEN && m.contains("already used"), "{}", m);
            // Denying never needs a code.
            assert!(ask(&mut r, &id2, &h2, "deny", None).is_ok());
        })
        .await;
        assert!(matches!(w1.wait().await, crate::approval::Resolution::Approved { .. }));
        assert!(matches!(w2.wait().await, crate::approval::Resolution::Denied { .. }));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn wrong_codes_lock_step_up_out_even_for_the_right_one() {
        let f = with_stepup(TEST_SECRET_B32);
        let s = serve(&f).await;
        let (_w, id, hash) = red_card(&f, "merge");
        let now = (f.engine.clock_unix)() as u64;
        let good = delphi_core::totp::code_at(b"12345678901234567890", now);
        let wrong = if good == "000000" { "000001" } else { "000000" };
        let (sock, tok) = (s.relay_sock.clone(), s.relay.clone());
        blocking(move || {
            let mut r = Client::connect(&sock).unwrap();
            r.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            r.hello(&tok).unwrap();
            for _ in 0..delphi_core::totp::MAX_FAILURES {
                let e = r.call("approval.relay_resolve", json!({"approval_id": id, "payload_blake3": hash, "decision": "allow", "channel": "email", "sender": "me@example.com", "totp": wrong}));
                assert_eq!(rpc_code(e), Some(code::FORBIDDEN));
            }
            let e = r.call("approval.relay_resolve", json!({"approval_id": id, "payload_blake3": hash, "decision": "allow", "channel": "email", "sender": "me@example.com", "totp": good}));
            match e {
                Err(ClientError::Rpc { message, .. }) => assert!(message.contains("locked"), "{}", message),
                other => panic!("{:?}", other),
            }
        })
        .await;
        assert!(f.engine.broker.get(&_w.approval_id).is_some(), "the card is still waiting; the console can still answer it");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn step_up_that_is_not_set_up_or_whose_secret_is_unusable_approves_nothing() {
        // Not configured at all: a Red approval is refused whatever code comes with it.
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let (_w, id, hash) = red_card(&f, "merge");
        let good = delphi_core::totp::code_at(b"12345678901234567890", (f.engine.clock_unix)() as u64);
        let (sock, tok) = (s.relay_sock.clone(), s.relay.clone());
        let (i2, h2, g2) = (id.clone(), hash.clone(), good.clone());
        blocking(move || {
            let mut r = Client::connect(&sock).unwrap();
            r.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            r.hello(&tok).unwrap();
            let e = r.call("approval.relay_resolve", json!({"approval_id": i2, "payload_blake3": h2, "decision": "allow", "channel": "email", "sender": "me@example.com", "totp": g2}));
            match e {
                Err(ClientError::Rpc { message, .. }) => assert!(message.contains("step-up is not set up"), "{}", message),
                other => panic!("{:?}", other),
            }
        })
        .await;
        // Configured, but the secret is not base32.
        let f = with_stepup("not base32 at all!");
        let s = serve(&f).await;
        let (_w, id, hash) = red_card(&f, "merge");
        let (sock, tok) = (s.relay_sock.clone(), s.relay.clone());
        blocking(move || {
            let mut r = Client::connect(&sock).unwrap();
            r.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            r.hello(&tok).unwrap();
            let e = r.call("approval.relay_resolve", json!({"approval_id": id, "payload_blake3": hash, "decision": "allow", "channel": "email", "sender": "me@example.com", "totp": "123456"}));
            match e {
                Err(ClientError::Rpc { message, .. }) => assert!(message.contains("not usable"), "{}", message),
                other => panic!("{:?}", other),
            }
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn build_merge_needs_a_task_with_a_kept_branch() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        let s = serve(&f).await;
        let (sock, tok) = (s.sock.clone(), s.console.clone());
        blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            assert_eq!(rpc_code(c.call("build.merge", json!({}))), Some(code::INVALID_PARAMS));
            assert_eq!(rpc_code(c.call("build.merge", json!({"task_id": "../x"}))), Some(code::INVALID_PARAMS));
            assert_eq!(rpc_code(c.call("build.merge", json!({"task_id": "t_0099"}))), Some(code::NOT_FOUND), "no such branch in this mission");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_relay_is_told_that_decisions_await_review_and_the_list_of_relay_events_is_pinned() {
        let mut listed: Vec<&str> = delphi_core::hub::RELAY_EVENTS.to_vec();
        listed.sort();
        assert_eq!(listed, vec!["approval.parked", "approval.requested", "approval.resolved", "decisions.review_requested", "task.completed", "task.failed"], "the list is the contract; adding to it is a decision");
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
            let health = c.call("kb.get", json!({"target": "/v1/health"})).unwrap();
            assert!(health["status"] == 200 && health["body"]["ok"] == true, "{health}");
            assert_eq!(c.call("kb.get", json!({"target": "/v1/doc?path=../../etc/passwd"})).unwrap()["status"], 404, "a path outside the index finds nothing");
            assert_eq!(rpc_code(c.call("kb.get", json!({"target": "/etc/passwd"}))), Some(code::INVALID_PARAMS), "only /v1/ targets");
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
    async fn the_daemon_serves_the_folded_observer_state_to_a_narrator() {
        let f = setup(vec![Step::text("The ladder has three attempts.")], 5.0, |_| {});
        f.engine.run_task(&f.m, "orchestrator", "Explain the retry ladder").await.unwrap();
        let s = serve(&f).await;
        let (sock, tok) = (s.narrator_sock.clone(), s.narrator.clone());
        let st = blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&tok).unwrap();
            c.call("observe.state", json!({})).unwrap()
        })
        .await;
        assert_eq!(st["tasks"]["t_0001"]["status"], "done");
        assert_eq!((st["agents"]["orchestrator"]["calls"].as_u64(), st["agents"]["orchestrator"]["state"].as_str()), (Some(1), Some("idle")));
        assert!(st["spent_usd"].as_f64().unwrap() > 0.0 && st["tokens"]["output"].as_u64().unwrap() > 0);
        assert!(st["live"].as_object().unwrap().is_empty(), "streamed text is never part of the persisted picture");
        assert!(st["cache_hit_ratio"].is_number() && st["default_view"].is_string() && st["budget"]["cap_usd"].as_f64().unwrap() > 0.0);
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
    async fn models_get_shows_the_pool_as_the_daemon_sees_it() {
        let f = setup(vec![], 5.0, |_| {});
        let reset = f.now.load(std::sync::atomic::Ordering::SeqCst) + 3600;
        f.engine.pool.record_exhausted("direct", "opus-x", &delphi_adapter::exhaust::Exhaustion { reason: "quota reached".into(), reset_at: Some(reset), raw: String::new() });
        let s = serve(&f).await;
        let (sock, tok) = (s.sock.clone(), s.console.clone());
        let v = blocking(move || {
            let mut c = Client::connect(&sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            c.hello(&tok).unwrap();
            c.call("models.get", json!({})).unwrap()
        })
        .await;
        let text = v["text"].as_str().unwrap();
        let out = text.lines().find(|l| l.contains("direct") && l.contains("opus-x")).unwrap();
        assert!(out.contains("exhausted: quota reached") && out.contains("in 59m") || out.contains("in 1h"), "{}", out);
        let other = text.lines().find(|l| l.contains("backup") && l.contains("opus-x")).unwrap();
        assert!(other.trim_end().ends_with("available"), "the same model on another endpoint is untouched: {}", other);
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

    // specs/026 §7.2: the spec the operator is about to approve lists what it cites that no longer stands. Advice on the
    // existing approval, which is still the operator's own act.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_spec_awaiting_approval_lists_what_it_cites_that_no_longer_stands() {
        let f = setup(vec![Step::text("x")], 5.0, |_| {});
        crate::align::tests::fixture(&f.engine.ws);
        let dir = f.root.join("workspaces/w/team/specs");
        fs::create_dir_all(&dir).unwrap();
        let approved = |body: &str| format!("---\nstatus: approved\napproved_sections: \"§1\"\n---\n# Spec\n{}\n", body);
        fs::write(dir.join("012-cites.md"), approved("Serves [G-acme-01] and [G-acme-02]; follows decision 0042 and decision 0057.")).unwrap();
        fs::write(dir.join("013-clean.md"), approved("Serves [G-acme-01]; honors [P-acme-05].")).unwrap();
        let s = serve(&f).await;
        let (c_sock, c_tok) = (s.sock.clone(), s.console.clone());
        let listed = blocking(move || {
            let mut c = Client::connect(&c_sock).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            c.hello(&c_tok).unwrap();
            c.call("ratify.list", json!({})).unwrap()
        })
        .await;
        let find = |name: &str| listed.as_array().unwrap().iter().find(|p| p["id"] == format!("ws:specs/{}", name)).unwrap_or_else(|| panic!("{} not listed in {}", name, listed)).clone();
        let cites = find("012-cites.md");
        assert_eq!(cites["kind"], "spec-approval");
        let lines: Vec<&str> = cites["alignment"].as_array().unwrap().iter().filter_map(|l| l.as_str()).collect();
        assert!(lines.iter().any(|l| l.starts_with("ALIGN-REF-01 (P2) G-acme-02")) && lines.iter().any(|l| l.starts_with("ALIGN-REF-01 (P2) decision 0042")), "{:?}", lines);
        assert_eq!(lines.len(), 2, "the live goal and the live decision are not findings: {:?}", lines);
        assert!(find("013-clean.md").get("alignment").is_none(), "a spec that cites only what stands has nothing to add");
        assert_eq!(f.model.requests().len(), 0, "no model read anything");
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
