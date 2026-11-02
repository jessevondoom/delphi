//! Research jobs for host agents (`specs/066` §3, §6): the lifecycle, the queue, the egress scopes and the audit record.
//!
//! This module runs no model and opens no connection. A *runner* (the planner of S3 and the validator of S4) is handed a
//! [`JobCtx`] and does the work through it; the context counts steps against the depth budget, refuses any egress the
//! role's scope does not allow, and records every query and URL for `research_status`.
//!
//! * **At most [`MAX_CONCURRENT`] jobs run at once; the rest wait in order.** There is no limit on how many may wait (D-5).
//! * **Two scopes, never one.** The scout may search and fetch; the validator may only fetch. Neither has a connector,
//!   a write or a command, and the validator's scope is a subset of the scout's, so it cannot do what the scout may not.
//! * **A job is read by the session that asked**, and by nobody else.
//! * **A job that reaches its step budget returns `partial`**; there are no hidden retries.

use crate::host_rpc::RESEARCH_EVENT;
use delphi_core::research::Depth;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, Semaphore};

pub const MAX_CONCURRENT: usize = 2;
/// A running job is stopped after this long and reports `failed`.
pub const JOB_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Finished jobs are kept this long so a slow answer can be fetched, then dropped.
const KEEP: Duration = Duration::from_secs(3600);
/// What the status keeps of the queries and URLs a job touched.
const LIST_CAP: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Egress {
    Search,
    Fetch,
}

/// What a role's research span may reach out with. Anything not listed is refused, whatever the model asks for.
pub fn egress_scope(role: &str) -> &'static [Egress] {
    match role {
        "scout" => &[Egress::Search, Egress::Fetch],
        "validator" => &[Egress::Fetch],
        _ => &[],
    }
}

pub fn authorize(role: &str, tool: Egress) -> Result<(), String> {
    if egress_scope(role).contains(&tool) {
        Ok(())
    } else {
        Err(format!("`{}` has no {} access in a research job", role, if tool == Egress::Search { "search" } else { "fetch" }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Queued,
    Running,
    Done,
    Partial,
    Failed,
    Cancelled,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Done => "done",
            State::Partial => "partial",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
        }
    }

    pub fn finished(self) -> bool {
        !matches!(self, State::Queued | State::Running)
    }
}

/// How a runner ends: its result, or the result so far because the budget ran out.
pub enum Finish {
    Done(Value),
    Partial(Value),
}

/// The budget ran out; the runner should return what it has as [`Finish::Partial`].
#[derive(Debug, PartialEq, Eq)]
pub struct BudgetSpent;

#[derive(Default)]
struct Trail {
    queries: Vec<String>,
    fetched: Vec<String>,
    refused: Vec<String>,
}

/// What a runner holds while it works.
pub struct JobCtx {
    max_steps: u32,
    steps: AtomicU32,
    trail: Mutex<Trail>,
}

impl JobCtx {
    pub fn with_budget(max_steps: u32) -> JobCtx {
        JobCtx { max_steps, steps: AtomicU32::new(0), trail: Mutex::new(Trail::default()) }
    }

    /// Count one step. The step that would pass the depth's budget is refused.
    pub fn step(&self) -> Result<(), BudgetSpent> {
        let n = self.steps.fetch_add(1, Ordering::SeqCst);
        if n >= self.max_steps {
            self.steps.store(self.max_steps, Ordering::SeqCst);
            Err(BudgetSpent)
        } else {
            Ok(())
        }
    }

    pub fn steps(&self) -> u32 {
        self.steps.load(Ordering::SeqCst)
    }

    /// Check `role`'s scope for `tool` and count the step; the query or URL is recorded, or the refusal is.
    pub fn egress(&self, role: &str, tool: Egress, what: &str) -> Result<(), String> {
        let mut t = self.trail.lock().unwrap();
        if let Err(e) = authorize(role, tool) {
            push_capped(&mut t.refused, what);
            return Err(e);
        }
        drop(t);
        self.step().map_err(|_| "the step budget for this depth is spent".to_string())?;
        let mut t = self.trail.lock().unwrap();
        push_capped(if tool == Egress::Search { &mut t.queries } else { &mut t.fetched }, what);
        Ok(())
    }

    /// A URL the fetcher refused (not trusted, not https, an internal address): listed in the status, never fetched.
    pub fn refused(&self, url: &str) {
        push_capped(&mut self.trail.lock().unwrap().refused, url);
    }
}

fn push_capped(list: &mut Vec<String>, s: &str) {
    if list.len() < LIST_CAP {
        list.push(s.chars().take(300).collect());
    }
}

struct Job {
    session: String,
    depth: Depth,
    state: State,
    queued: Instant,
    ended: Option<Instant>,
    result: Option<Value>,
    error: Option<String>,
    ctx: Arc<JobCtx>,
    cancel: Arc<Notify>,
}

/// The daemon's research jobs.
#[derive(Clone)]
pub struct Jobs {
    jobs: Arc<Mutex<HashMap<String, Job>>>,
    permits: Arc<Semaphore>,
    timeout: Duration,
}

impl Default for Jobs {
    fn default() -> Self {
        Jobs::new(MAX_CONCURRENT, JOB_TIMEOUT)
    }
}

impl Jobs {
    pub fn new(max_concurrent: usize, timeout: Duration) -> Jobs {
        Jobs { jobs: Default::default(), permits: Arc::new(Semaphore::new(max_concurrent)), timeout }
    }

    /// Queue a job and return at once. `run` starts when one of the running slots is free.
    pub fn submit<F, Fut>(&self, id: &str, session: &str, depth: Depth, run: F)
    where
        F: FnOnce(Arc<JobCtx>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Finish, String>> + Send + 'static,
    {
        let ctx = Arc::new(JobCtx { max_steps: depth.max_steps(), steps: AtomicU32::new(0), trail: Mutex::new(Trail::default()) });
        let cancel = Arc::new(Notify::new());
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.retain(|_, j| j.ended.is_none_or(|t| t.elapsed() < KEEP));
            jobs.insert(id.to_string(), Job { session: session.to_string(), depth, state: State::Queued, queued: Instant::now(), ended: None, result: None, error: None, ctx: ctx.clone(), cancel: cancel.clone() });
        }
        let (me, id, timeout) = (self.clone(), id.to_string(), self.timeout);
        tokio::spawn(async move {
            let permit = tokio::select! {
                p = me.permits.clone().acquire_owned() => p.ok(),
                _ = cancel.notified() => None,
            };
            let Some(permit) = permit else {
                me.end(&id, State::Cancelled, None, None);
                return;
            };
            me.set_state(&id, State::Running);
            let outcome = tokio::select! {
                r = tokio::time::timeout(timeout, run(ctx)) => Some(r),
                _ = cancel.notified() => None,
            };
            drop(permit);
            match outcome {
                None => me.end(&id, State::Cancelled, None, None),
                Some(Err(_)) => me.end(&id, State::Failed, None, Some("the job ran out of time".into())),
                Some(Ok(Ok(Finish::Done(v)))) => me.end(&id, State::Done, Some(v), None),
                Some(Ok(Ok(Finish::Partial(v)))) => me.end(&id, State::Partial, Some(v), None),
                Some(Ok(Err(e))) => me.end(&id, State::Failed, None, Some(e)),
            }
        });
    }

    fn set_state(&self, id: &str, s: State) {
        if let Some(j) = self.jobs.lock().unwrap().get_mut(id) {
            j.state = s;
        }
    }

    fn end(&self, id: &str, s: State, result: Option<Value>, error: Option<String>) {
        if let Some(j) = self.jobs.lock().unwrap().get_mut(id) {
            if !j.state.finished() {
                j.state = s;
                j.result = result;
                j.error = error;
                j.ended = Some(Instant::now());
            }
        }
    }

    /// Stop a queued or running job. `false` when it is not this session's, or already finished.
    pub fn cancel(&self, id: &str, session: &str) -> bool {
        let jobs = self.jobs.lock().unwrap();
        match jobs.get(id) {
            Some(j) if j.session == session && !j.state.finished() => {
                j.cancel.notify_one();
                true
            }
            _ => false,
        }
    }

    /// The state of a job for the session that asked, or `None` (no such job for this session; the two are not told apart).
    pub fn status(&self, id: &str, session: &str) -> Option<Value> {
        let jobs = self.jobs.lock().unwrap();
        let j = jobs.get(id).filter(|j| j.session == session)?;
        let t = j.ctx.trail.lock().unwrap();
        let ahead = if j.state == State::Queued { jobs.values().filter(|o| o.state == State::Queued && o.queued < j.queued).count() } else { 0 };
        Some(json!({
            "research_id": id, "status": j.state.as_str(), "depth": j.depth, "queued_ahead": ahead,
            "steps": j.ctx.steps(), "max_steps": j.depth.max_steps(),
            "queries": t.queries, "fetched": t.fetched, "refused": t.refused,
            "error": j.error,
        }))
    }

    /// The finished result for the session that asked: the runner's value with the job's `status`, or `None` while it is
    /// still queued or running (and for a job that is not this session's).
    pub fn result(&self, id: &str, session: &str) -> Option<Value> {
        let jobs = self.jobs.lock().unwrap();
        let j = jobs.get(id).filter(|j| j.session == session && j.state.finished())?;
        let mut v = j.result.clone().unwrap_or_else(|| json!({}));
        if let Some(o) = v.as_object_mut() {
            o.insert("status".into(), json!(j.state.as_str()));
            if let Some(e) = &j.error {
                o.insert("error".into(), json!(e));
            }
        }
        Some(v)
    }

    /// Wait for a job to finish, for at most `wait`; `None` if it is still going or not this session's.
    pub async fn wait(&self, id: &str, session: &str, wait: Duration) -> Option<Value> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(v) = self.result(id, session) {
                return Some(v);
            }
            if Instant::now() >= deadline || self.status(id, session).is_none() {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// The payload of the `host.research.requested` event: who asked and how, never the question's text.
pub fn audit_payload(research_id: &str, depth: Depth, source: &str, question_chars: usize) -> Value {
    json!({"research_id": research_id, "depth": depth, "source": source, "question_chars": question_chars, "event": RESEARCH_EVENT})
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    fn done(v: Value) -> Result<Finish, String> {
        Ok(Finish::Done(v))
    }

    #[test]
    fn the_validators_scope_is_a_subset_of_the_scouts_and_no_other_role_has_one() {
        assert!(egress_scope("validator").iter().all(|t| egress_scope("scout").contains(t)));
        assert_eq!(egress_scope("validator"), &[Egress::Fetch]);
        assert!(authorize("validator", Egress::Search).is_err());
        assert!(authorize("scout", Egress::Search).is_ok());
        for role in ["orchestrator", "builder", "architect", "investigator", "writer", ""] {
            assert!(authorize(role, Egress::Fetch).is_err() && authorize(role, Egress::Search).is_err(), "{role}");
        }
    }

    #[test]
    fn the_step_budget_is_hard_and_a_refused_tool_is_listed_not_counted() {
        let ctx = JobCtx { max_steps: 2, steps: AtomicU32::new(0), trail: Mutex::new(Trail::default()) };
        assert!(ctx.egress("validator", Egress::Search, "q").is_err());
        assert_eq!(ctx.steps(), 0);
        assert!(ctx.egress("scout", Egress::Search, "q1").is_ok());
        assert!(ctx.egress("scout", Egress::Fetch, "https://a.example/").is_ok());
        assert!(ctx.egress("scout", Egress::Fetch, "https://b.example/").is_err());
        assert_eq!(ctx.steps(), 2);
        let t = ctx.trail.lock().unwrap();
        assert_eq!((t.queries.len(), t.fetched.len(), t.refused.len()), (1, 1, 1));
    }

    #[tokio::test]
    async fn only_two_jobs_run_at_once_and_the_rest_queue_in_order_and_all_finish() {
        let jobs = Jobs::default();
        let (running, peak) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
        let mut gates = Vec::new();
        for i in 0..5 {
            let (tx, rx) = oneshot::channel::<()>();
            gates.push(tx);
            let (r, p) = (running.clone(), peak.clone());
            jobs.submit(&format!("r{i}"), "s", Depth::Quick, move |_| async move {
                let now = r.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(now, Ordering::SeqCst);
                let _ = rx.await;
                r.fetch_sub(1, Ordering::SeqCst);
                done(json!({"n": 1}))
            });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(jobs.status("r0", "s").unwrap()["status"], "running");
        assert_eq!(jobs.status("r1", "s").unwrap()["status"], "running");
        let third = jobs.status("r4", "s").unwrap();
        assert_eq!((third["status"].as_str(), third["queued_ahead"].as_u64()), (Some("queued"), Some(2)));
        for g in gates {
            let _ = g.send(());
        }
        for i in 0..5 {
            let r = jobs.wait(&format!("r{i}"), "s", Duration::from_secs(5)).await.expect("finished");
            assert_eq!(r["status"], "done");
        }
        assert_eq!(peak.load(Ordering::SeqCst), 2, "never more than two at once");
    }

    #[tokio::test]
    async fn a_job_belongs_to_its_session_and_a_spent_budget_returns_partial() {
        let jobs = Jobs::default();
        jobs.submit("r1", "mine", Depth::Quick, |ctx| async move {
            let mut n = 0;
            while ctx.step().is_ok() {
                n += 1;
            }
            Ok(Finish::Partial(json!({"steps_done": n})))
        });
        let r = jobs.wait("r1", "mine", Duration::from_secs(5)).await.unwrap();
        assert_eq!((r["status"].as_str(), r["steps_done"].as_u64()), (Some("partial"), Some(5)));
        assert!(jobs.status("r1", "other").is_none() && jobs.result("r1", "other").is_none());
        assert!(!jobs.cancel("r1", "other"));
        assert!(jobs.status("nope", "mine").is_none());
    }

    #[tokio::test]
    async fn cancel_stops_a_running_job_and_a_queued_one() {
        let jobs = Jobs::new(1, JOB_TIMEOUT);
        jobs.submit("run", "s", Depth::Deep, |_| async { std::future::pending::<Result<Finish, String>>().await });
        jobs.submit("wait", "s", Depth::Deep, |_| async { done(json!({})) });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(jobs.status("wait", "s").unwrap()["status"], "queued");
        assert!(jobs.cancel("wait", "s") && jobs.cancel("run", "s"));
        for id in ["run", "wait"] {
            assert_eq!(jobs.wait(id, "s", Duration::from_secs(5)).await.unwrap()["status"], "cancelled", "{id}");
        }
        assert!(!jobs.cancel("run", "s"), "already finished");
    }

    #[tokio::test]
    async fn a_slow_job_times_out_and_a_failing_one_says_why() {
        let jobs = Jobs::new(2, Duration::from_millis(50));
        jobs.submit("slow", "s", Depth::Quick, |_| async { std::future::pending::<Result<Finish, String>>().await });
        jobs.submit("bad", "s", Depth::Quick, |_| async { Err("no provider".to_string()) });
        let slow = jobs.wait("slow", "s", Duration::from_secs(5)).await.unwrap();
        assert_eq!(slow["status"], "failed");
        assert!(slow["error"].as_str().unwrap().contains("out of time"));
        let bad = jobs.wait("bad", "s", Duration::from_secs(5)).await.unwrap();
        assert_eq!((bad["status"].as_str(), bad["error"].as_str()), (Some("failed"), Some("no provider")));
    }

    #[test]
    fn the_audit_record_has_the_asker_and_the_length_but_not_the_question() {
        let a = audit_payload("t-1", Depth::Survey, "host:claude-code/x", 42);
        assert_eq!((a["research_id"].as_str(), a["question_chars"].as_u64(), a["depth"].as_str()), (Some("t-1"), Some(42), Some("survey")));
        assert!(a.get("question").is_none());
    }
}
