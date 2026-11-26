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
use std::collections::{HashMap, VecDeque};
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

/// How a job ended, as [`Jobs::submit_with`] tells its `on_end`: the state, the runner's value when it produced one, the error when
/// it failed, and how long it ran.
pub struct Ended<'a> {
    pub state: State,
    pub result: Option<&'a Value>,
    pub error: Option<&'a str>,
    pub ms: u64,
}

/// A step awaited after a job ends and before it reads as finished ([`Jobs::submit_then`]), given the state it ended in.
pub type Then = Box<dyn FnOnce(State) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

/// The budget ran out; the runner should return what it has as [`Finish::Partial`].
#[derive(Debug, PartialEq, Eq)]
pub struct BudgetSpent;

#[derive(Default)]
struct Trail {
    queries: Vec<String>,
    fetched: Vec<String>,
    refused: Vec<String>,
    /// For each page fetched: where its URL came from (`trusted`, or the search that returned it; `specs/098` §7.5 item 7a).
    fetched_via: Vec<(String, String)>,
}

/// When a job last did something, and its last few calls by kind (`search`, `fetch`, `refused`): what a status call reports so a
/// host can tell a slow job from a stuck one (`specs/098` §19). Never a query or a URL.
struct Activity {
    last: Instant,
    recent: VecDeque<(i64, &'static str)>,
}

/// The most queries a run asks a host to search, and the most results a host may hand back for one (`specs/119` §2.1 item 4).
pub const FEED_QUERIES: usize = 12;
pub const FEED_RESULTS: usize = 10;

/// What a host handed back for one query the run asked it to search (`specs/119` §2.1 item 4): the query, as the run asked it, and
/// the results. `leads_only` is a feed from a session under the private-read mark: the daemon fetches none of its addresses.
#[derive(Debug, Clone)]
pub struct Fed {
    pub query: String,
    pub rows: Vec<delphi_adapter::research::search::Row>,
    pub leads_only: bool,
}

/// What a feed was refused for, in fixed words. None of them carries the host's text.
pub const FEED_NOT_ASKED: &str = "this run asked the host for no search, or is not waiting for one yet: read research_status";
pub const FEED_CLOSED: &str = "this run is no longer taking search results";
pub const FEED_UNKNOWN_QUERY: &str = "that query is not one this run asked for: hand back each query exactly as it was given";
pub const FEED_TWICE: &str = "that query was already handed back";

/// The feed board of a run that asked its host to search (`specs/119` §2.1 items 3 and 4). The run posts the queries it planned,
/// already cleaned and guarded; the host hands back results for those queries and no other, once each, while the board is open.
#[derive(Default)]
struct Board {
    asked: Vec<String>,
    fed: Vec<Fed>,
    open: bool,
    closed: bool,
}

/// What a runner holds while it works.
pub struct JobCtx {
    max_steps: u32,
    steps: AtomicU32,
    /// The check's own budget (`specs/112` §4.4): the validator's steps are counted here and never against the search.
    check_max: u32,
    check_steps: AtomicU32,
    trail: Mutex<Trail>,
    activity: Mutex<Activity>,
    /// The workspace's `[privacy] identifiers` (`specs/119` §2 item 7), set once by whoever starts the runner; unset means none.
    identifiers: std::sync::OnceLock<Vec<String>>,
    /// The queries asked of the host and what it handed back (`specs/119` §2.1).
    board: Mutex<Board>,
    fed_signal: Notify,
}

impl JobCtx {
    pub fn with_budget(max_steps: u32) -> JobCtx {
        JobCtx { max_steps, steps: AtomicU32::new(0), check_max: check_budget(max_steps), check_steps: AtomicU32::new(0), trail: Mutex::new(Trail::default()), activity: Mutex::new(Activity { last: Instant::now(), recent: VecDeque::new() }), identifiers: Default::default(), board: Default::default(), fed_signal: Notify::new() }
    }

    /// Give the job the workspace's privacy identifiers. The first call wins.
    pub fn set_identifiers(&self, identifiers: Vec<String>) {
        let _ = self.identifiers.set(identifiers);
    }

    /// The identifier guard for a query this job is about to send to an engine or hand to a host (`specs/119` §2 item 7).
    pub fn guard(&self, query: &str) -> crate::query_guard::Guarded {
        crate::query_guard::guard(query, self.identifiers.get().map(Vec::as_slice).unwrap_or(&[]))
    }

    /// Ask the host to search (`specs/119` §2.1 item 3): post the run's queries, at most [`FEED_QUERIES`], and open the board. The
    /// caller has cleaned and guarded each. Asking twice is the first ask.
    pub fn ask_host(&self, queries: &[String]) {
        let mut b = self.board.lock().unwrap();
        if b.open || b.closed {
            return;
        }
        b.asked = queries.iter().take(FEED_QUERIES).cloned().collect();
        b.open = true;
    }

    /// What the board holds, for a status call: the queries asked, how many were handed back, and whether the run still waits.
    /// `None` when the run asked the host for nothing.
    pub fn host_asked(&self) -> Option<(Vec<String>, usize, bool)> {
        let b = self.board.lock().unwrap();
        (b.open || b.closed).then(|| (b.asked.clone(), b.fed.len(), b.open))
    }

    /// A host hands back the results of one query (`specs/119` §2.1 item 4). Taken only while the board is open, only for a query
    /// the run asked, exactly as it was asked, and once for each; at most [`FEED_RESULTS`] results are kept. Returns how many
    /// results were taken and how many queries still wait.
    pub fn feed(&self, query: &str, mut rows: Vec<delphi_adapter::research::search::Row>, leads_only: bool) -> Result<(usize, usize), &'static str> {
        let mut b = self.board.lock().unwrap();
        if b.closed {
            return Err(FEED_CLOSED);
        }
        if !b.open {
            return Err(FEED_NOT_ASKED);
        }
        let query = query.trim();
        if !b.asked.iter().any(|q| q == query) {
            return Err(FEED_UNKNOWN_QUERY);
        }
        if b.fed.iter().any(|f| f.query == query) {
            return Err(FEED_TWICE);
        }
        rows.truncate(FEED_RESULTS);
        let taken = rows.len();
        b.fed.push(Fed { query: query.to_string(), rows, leads_only });
        let left = b.asked.len() - b.fed.len();
        drop(b);
        self.note("feed");
        self.fed_signal.notify_one();
        Ok((taken, left))
    }

    /// Wait until the host has handed back every query asked, or for `wait`, then close the board and return what was handed
    /// back. A feed that arrives after this is refused.
    pub async fn fed(&self, wait: Duration) -> Vec<Fed> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            {
                let mut b = self.board.lock().unwrap();
                if b.fed.len() >= b.asked.len() || tokio::time::Instant::now() >= deadline {
                    b.open = false;
                    b.closed = true;
                    return b.fed.clone();
                }
            }
            let _ = tokio::time::timeout_at(deadline, self.fed_signal.notified()).await;
        }
    }

    /// Note that the job just made a call of this kind.
    fn note(&self, kind: &'static str) {
        let mut a = self.activity.lock().unwrap();
        a.last = Instant::now();
        if a.recent.len() >= 20 {
            a.recent.pop_front();
        }
        a.recent.push_back((crate::host_rpc::now_ms(), kind));
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

    /// Count one step of the check. It has a budget of its own ([`check_budget`]) and takes nothing from the search (`specs/112` §4.4).
    pub fn check_step(&self) -> Result<(), BudgetSpent> {
        let n = self.check_steps.fetch_add(1, Ordering::SeqCst);
        if n >= self.check_max {
            self.check_steps.store(self.check_max, Ordering::SeqCst);
            Err(BudgetSpent)
        } else {
            Ok(())
        }
    }

    pub fn check_steps(&self) -> u32 {
        self.check_steps.load(Ordering::SeqCst)
    }

    pub fn max_check_steps(&self) -> u32 {
        self.check_max
    }

    /// Check `role`'s scope for `tool` and count the step; the query or URL is recorded, or the refusal is.
    pub fn egress(&self, role: &str, tool: Egress, what: &str) -> Result<(), String> {
        let mut t = self.trail.lock().unwrap();
        if let Err(e) = authorize(role, tool) {
            push_capped(&mut t.refused, what);
            drop(t);
            self.note("refused");
            return Err(e);
        }
        drop(t);
        // The validator's steps are the check's (`specs/112` §4.4); every other role spends the run's.
        if role == "validator" {
            self.check_step().map_err(|_| "the step budget for the check is spent".to_string())?;
        } else {
            self.step().map_err(|_| "the step budget for this depth is spent".to_string())?;
        }
        let mut t = self.trail.lock().unwrap();
        push_capped(if tool == Egress::Search { &mut t.queries } else { &mut t.fetched }, what);
        drop(t);
        self.note(if tool == Egress::Search { "search" } else { "fetch" });
        Ok(())
    }

    /// A URL the fetcher refused (not trusted, not https, an internal address): listed in the status, never fetched.
    pub fn refused(&self, url: &str) {
        push_capped(&mut self.trail.lock().unwrap().refused, url);
        self.note("refused");
    }

    /// What the trail holds of fetched pages' provenance, for tests.
    #[cfg(test)]
    pub(crate) fn fetched_via_list(&self) -> Vec<(String, String)> {
        self.trail.lock().unwrap().fetched_via.clone()
    }

    /// Where a page the fetcher read came from. A label, never page text; the list is capped like the others.
    pub fn fetched_via(&self, url: &str, via: &str) {
        let mut t = self.trail.lock().unwrap();
        if t.fetched_via.len() < LIST_CAP {
            t.fetched_via.push((url.chars().take(300).collect(), via.chars().take(80).collect()));
        }
    }
}

/// The step budget of the check (`specs/112` §4.4): a quarter of the run's, and at least two.
pub fn check_budget(max_steps: u32) -> u32 {
    (max_steps / 4).max(2)
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
    /// The step budget at each depth (`[research.limits]`, `specs/112` §4 item 6).
    limits: Arc<crate::research_run::Limits>,
}

impl Default for Jobs {
    fn default() -> Self {
        Jobs::new(MAX_CONCURRENT, JOB_TIMEOUT)
    }
}

impl Jobs {
    pub fn new(max_concurrent: usize, timeout: Duration) -> Jobs {
        Jobs { jobs: Default::default(), permits: Arc::new(Semaphore::new(max_concurrent)), timeout, limits: Default::default() }
    }

    /// The queue under the one limits table (`specs/112` §4 item 6): how many jobs run at once, how long one may run, and the
    /// step budget of each depth. One queue serves every caller: a host, the console and an inside agent.
    pub fn with_limits(limits: &crate::research_run::Limits) -> Jobs {
        Jobs { jobs: Default::default(), permits: Arc::new(Semaphore::new(limits.concurrent.max(1))), timeout: Duration::from_secs(limits.job_secs), limits: Arc::new(limits.clone()) }
    }

    /// Queue a job and return at once. `run` starts when one of the running slots is free.
    pub fn submit<F, Fut>(&self, id: &str, session: &str, depth: Depth, run: F)
    where
        F: FnOnce(Arc<JobCtx>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Finish, String>> + Send + 'static,
    {
        self.submit_with(id, session, depth, run, |_| {})
    }

    /// [`submit`](Jobs::submit), and `on_end` is called once when the job ends, however it ends (done, partial, failed, timed out,
    /// cancelled while queued or running), just before the job reads as finished. It is how a run's record on disk learns its last
    /// state (`specs/112` §2.4): a cancelled or timed-out run never gets back to its own code.
    pub fn submit_with<F, Fut, E>(&self, id: &str, session: &str, depth: Depth, run: F, on_end: E)
    where
        F: FnOnce(Arc<JobCtx>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Finish, String>> + Send + 'static,
        E: FnOnce(&Ended<'_>) + Send + 'static,
    {
        self.submit_then(id, session, depth, run, on_end, None)
    }

    /// [`submit_with`](Jobs::submit_with), and `then`, when given, is awaited once the job has ended, however it ended, after `on_end` and
    /// before the job reads as finished, with the state it ended in. It is how a research run writes its topic's seed page (`specs/139`
    /// §2.5 item 1) before its result is read.
    pub fn submit_then<F, Fut, E>(&self, id: &str, session: &str, depth: Depth, run: F, on_end: E, then: Option<Then>)
    where
        F: FnOnce(Arc<JobCtx>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Finish, String>> + Send + 'static,
        E: FnOnce(&Ended<'_>) + Send + 'static,
    {
        let ctx = Arc::new(JobCtx::with_budget(self.limits.steps_at(depth)));
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
                on_end(&Ended { state: State::Cancelled, result: None, error: None, ms: 0 });
                if let Some(then) = then {
                    then(State::Cancelled).await;
                }
                me.end(&id, State::Cancelled, None, None);
                return;
            };
            me.set_state(&id, State::Running);
            let started = Instant::now();
            let outcome = tokio::select! {
                r = tokio::time::timeout(timeout, run(ctx)) => Some(r),
                _ = cancel.notified() => None,
            };
            drop(permit);
            let (state, result, error) = match outcome {
                None => (State::Cancelled, None, None),
                Some(Err(_)) => (State::Failed, None, Some("the job ran out of time".to_string())),
                Some(Ok(Ok(Finish::Done(v)))) => (State::Done, Some(v), None),
                Some(Ok(Ok(Finish::Partial(v)))) => (State::Partial, Some(v), None),
                Some(Ok(Err(e))) => (State::Failed, None, Some(e)),
            };
            let ms = started.elapsed().as_millis() as u64;
            // `budget_used.ms` (`specs/119` §6 item 1): how long the run took, by the job's own clock.
            let result = result.map(|mut v| {
                if v["budget_used"].is_object() {
                    v["budget_used"]["ms"] = serde_json::json!(ms);
                }
                v
            });
            on_end(&Ended { state, result: result.as_ref(), error: error.as_deref(), ms });
            if let Some(then) = then {
                then(state).await;
            }
            me.end(&id, state, result, error);
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
            "steps": j.ctx.steps(), "max_steps": j.ctx.max_steps, "check_steps": j.ctx.check_steps(), "max_check_steps": j.ctx.max_check_steps(),
            "queries": t.queries, "fetched": t.fetched, "refused": t.refused,
            "fetched_via": t.fetched_via.iter().map(|(u, v)| json!({"url": u, "via": v})).collect::<Vec<_>>(),
            "error": j.error,
            "host_search": j.ctx.host_asked().map(|(asked, fed, open)| json!({"queries": asked, "fed": fed, "waiting": open})),
        }))
    }

    /// A host hands back the results of one query its run asked it to search (`specs/119` §2.1 item 4). `None` when the job is not
    /// this session's (no such job and another session's are not told apart).
    pub fn feed(&self, id: &str, session: &str, query: &str, rows: Vec<delphi_adapter::research::search::Row>, leads_only: bool) -> Option<Result<(usize, usize), &'static str>> {
        let ctx = self.jobs.lock().unwrap().get(id).filter(|j| j.session == session).map(|j| j.ctx.clone())?;
        Some(ctx.feed(query, rows, leads_only))
    }

    /// Wait, for at most `wait`, until the run has posted the queries it asks its host to search; `None` when it has not by then,
    /// has ended, or is not this session's.
    pub async fn host_asked(&self, id: &str, session: &str, wait: Duration) -> Option<Vec<String>> {
        let deadline = Instant::now() + wait;
        loop {
            let (asked, finished) = {
                let jobs = self.jobs.lock().unwrap();
                let j = jobs.get(id).filter(|j| j.session == session)?;
                (j.ctx.host_asked(), j.state.finished())
            };
            if let Some((queries, _, _)) = asked {
                return Some(queries);
            }
            if finished || Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// What a status call adds for a job (`specs/098` §19): one worker, its last calls as a fixed-form tail, and a stall flag.
    /// `None` for a job that is not this session's. Nothing here is a query or a URL.
    pub fn progress(&self, id: &str, session: &str, stall_secs: u64, lines: usize) -> Option<Value> {
        let jobs = self.jobs.lock().unwrap();
        let j = jobs.get(id).filter(|j| j.session == session)?;
        let a = j.ctx.activity.lock().unwrap();
        let idle = a.last.elapsed().as_secs();
        let elapsed = j.ended.map(|e| e.duration_since(j.queued)).unwrap_or_else(|| j.queued.elapsed()).as_secs();
        let last_call = a.recent.iter().rev().map(|(_, k)| *k).find(|k| *k != "refused");
        let state = match j.state {
            State::Queued => "starting",
            State::Running if a.recent.is_empty() => "thinking",
            State::Running => "reading",
            State::Done | State::Partial => "submitted",
            State::Failed | State::Cancelled => "abstained",
        };
        let tail: Vec<String> = a
            .recent
            .iter()
            .rev()
            .take(lines.clamp(1, 20))
            .map(|(ms, k)| if *k == "refused" { format!("{} scout gate deny", crate::progress::clock_of_ms(*ms)) } else { format!("{} scout tool {}", crate::progress::clock_of_ms(*ms), k) })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let stalled = j.state == State::Running && stall_secs > 0 && idle >= stall_secs;
        Some(json!({
            "workers": [{"role": "scout", "state": state, "elapsed_s": elapsed, "idle_s": idle, "calls": j.ctx.steps(), "last_call": last_call}],
            "tail": tail,
            "stalled": stalled,
            "stalled_seat": if stalled { json!("scout") } else { Value::Null },
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

    // `specs/119` §2.1 item 4: what a host may hand back, and when. Only for a query the run asked, exactly as asked, once each, at
    // most ten results, and only while the run waits.
    #[tokio::test]
    async fn a_feed_is_taken_only_for_a_query_the_run_asked_once_each_and_only_while_it_waits() {
        use delphi_adapter::research::search::Row;
        let row = |n: usize| Row { title: String::new(), url: format!("https://example.org/{n}"), snippet: String::new() };
        let c = JobCtx::with_budget(10);
        assert_eq!(c.host_asked(), None);
        assert_eq!(c.feed("aspirin fever", vec![row(1)], false), Err(FEED_NOT_ASKED), "nothing was asked yet");
        let many: Vec<String> = (0..20).map(|n| format!("query {n}")).collect();
        c.ask_host(&many);
        let (asked, fed, waiting) = c.host_asked().unwrap();
        assert_eq!((asked.len(), fed, waiting), (FEED_QUERIES, 0, true), "at most twelve queries a run");
        c.ask_host(&["another".to_string()]);
        assert_eq!(c.host_asked().unwrap().0, asked, "asking twice is the first ask");
        assert_eq!(c.feed("a query of the host's own", vec![row(1)], false), Err(FEED_UNKNOWN_QUERY));
        assert_eq!(c.feed("query 12", vec![row(1)], false), Err(FEED_UNKNOWN_QUERY), "a query past the twelve was not asked");
        assert_eq!(c.feed(" query 0 ", (0..25).map(row).collect(), false), Ok((FEED_RESULTS, FEED_QUERIES - 1)), "at most ten results are taken");
        assert_eq!(c.feed("query 0", vec![row(1)], false), Err(FEED_TWICE));
        // The wait ends at its deadline with what was handed back, and the board closes.
        let got = c.fed(Duration::from_millis(20)).await;
        assert_eq!((got.len(), got[0].query.as_str(), got[0].rows.len(), got[0].leads_only), (1, "query 0", FEED_RESULTS, false));
        assert_eq!(c.feed("query 1", vec![row(1)], false), Err(FEED_CLOSED));
        assert!(!c.host_asked().unwrap().2);
        // The wait ends early once every query is handed back.
        let c = Arc::new(JobCtx::with_budget(10));
        c.ask_host(&["one".to_string(), "two".to_string()]);
        let feeder = c.clone();
        tokio::spawn(async move {
            feeder.feed("one", vec![], true).unwrap();
            feeder.feed("two", vec![], true).unwrap();
        });
        let started = Instant::now();
        let got = c.fed(Duration::from_secs(30)).await;
        assert!(got.len() == 2 && got.iter().all(|f| f.leads_only) && started.elapsed() < Duration::from_secs(10));
    }
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
        let ctx = JobCtx::with_budget(2);
        assert!(ctx.egress("validator", Egress::Search, "q").is_err());
        assert_eq!(ctx.steps(), 0);
        assert!(ctx.egress("scout", Egress::Search, "q1").is_ok());
        assert!(ctx.egress("scout", Egress::Fetch, "https://a.example/").is_ok());
        assert!(ctx.egress("scout", Egress::Fetch, "https://b.example/").is_err());
        assert_eq!(ctx.steps(), 2);
        let t = ctx.trail.lock().unwrap();
        assert_eq!((t.queries.len(), t.fetched.len(), t.refused.len()), (1, 1, 1));
    }

    /// `specs/112` §4.4: the check has a quarter of the run's steps, at least two, and neither budget takes from the other.
    #[test]
    fn the_check_has_a_budget_of_its_own_that_the_search_cannot_spend() {
        assert_eq!([Depth::Quick, Depth::Survey, Depth::Deep].map(|d| check_budget(d.max_steps())), [2, 3, 7]);
        assert_eq!((check_budget(0), check_budget(8), check_budget(9)), (2, 2, 2));
        let ctx = JobCtx::with_budget(4);
        while ctx.egress("scout", Egress::Fetch, "https://a.example/").is_ok() {}
        assert_eq!((ctx.steps(), ctx.check_steps()), (4, 0), "the search spent its own budget and none of the check's");
        assert!(ctx.egress("validator", Egress::Fetch, "https://a.example/").is_ok(), "a spent search budget leaves the check its steps");
        assert!(ctx.check_step().is_ok());
        let over = ctx.egress("validator", Egress::Fetch, "https://a.example/").unwrap_err();
        assert!(over.contains("budget"), "{over}");
        assert!(ctx.check_step().is_err());
        assert_eq!((ctx.steps(), ctx.check_steps(), ctx.max_check_steps()), (4, 2, 2), "the check never adds to the run's steps");
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

    /// Slice S1d: `research_status` lists, for each page read, the URL and where it came from (a label, capped; never page text).
    #[tokio::test]
    async fn the_status_shows_where_each_fetched_url_came_from() {
        let jobs = Jobs::default();
        let (tx, rx) = oneshot::channel::<()>();
        jobs.submit("rp", "s", Depth::Quick, move |ctx| async move {
            ctx.fetched_via("https://who.int/a", "trusted");
            ctx.fetched_via("https://blog.example/z", "search:brave#2");
            ctx.fetched_via(&format!("https://long.example/{}", "x".repeat(400)), &"y".repeat(200));
            let _ = rx.await;
            done(json!({}))
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let s = jobs.status("rp", "s").unwrap();
        let via = s["fetched_via"].as_array().unwrap();
        assert_eq!(via[0], json!({"url": "https://who.int/a", "via": "trusted"}));
        assert_eq!(via[1], json!({"url": "https://blog.example/z", "via": "search:brave#2"}));
        assert!(via[2]["url"].as_str().unwrap().chars().count() <= 300 && via[2]["via"].as_str().unwrap().chars().count() <= 80, "{s}");
        let _ = tx.send(());
        jobs.wait("rp", "s", Duration::from_secs(5)).await.unwrap();
    }

    /// `specs/119` §6 item 1: `budget_used` gains `ms`, the time the run took by the job's clock; a result without `budget_used` is left
    /// as it was.
    #[tokio::test]
    async fn the_result_says_how_long_the_run_took() {
        let jobs = Jobs::default();
        jobs.submit("t1", "s", Depth::Quick, |_| async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            done(json!({"budget_used": {"steps": 1, "check_steps": 0}}))
        });
        jobs.submit("t2", "s", Depth::Quick, |_| async { done(json!({"n": 1})) });
        let r = jobs.wait("t1", "s", Duration::from_secs(5)).await.unwrap();
        assert!(r["budget_used"]["ms"].as_u64().is_some_and(|ms| ms >= 20), "{r}");
        assert_eq!(r["budget_used"]["steps"], 1);
        let r = jobs.wait("t2", "s", Duration::from_secs(5)).await.unwrap();
        assert!(r["budget_used"].is_null(), "{r}");
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

    /// `specs/139` §2.5 item 1 (slice E): the step after a job ends is awaited before the job reads as finished, so a research run's seed
    /// page is written before its result can be read; it is told the state the job ended in.
    #[tokio::test]
    async fn the_step_after_a_job_ends_runs_before_the_job_reads_as_finished() {
        let jobs = Jobs::new(2, Duration::from_secs(5));
        let seen: Arc<std::sync::Mutex<Vec<&'static str>>> = Default::default();
        for (id, partial) in [("done", false), ("partial", true)] {
            let s = seen.clone();
            let then: Then = Box::new(move |state| {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    s.lock().unwrap().push(state.as_str());
                })
            });
            jobs.submit_then(id, "s", Depth::Quick, move |_| async move { if partial { Ok(Finish::Partial(json!({}))) } else { done(json!({})) } }, |_| {}, Some(then));
        }
        for id in ["done", "partial"] {
            let r = jobs.wait(id, "s", Duration::from_secs(5)).await.expect("finished");
            assert!(seen.lock().unwrap().contains(&r["status"].as_str().unwrap()), "the step ran before {id} read as finished");
        }
        let mut got = seen.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, ["done", "partial"]);
    }

    /// `specs/112` §2.4: the record learns how a job ended whichever way it ended, before the job reads as finished.
    #[tokio::test]
    async fn on_end_is_told_every_way_a_job_can_end() {
        let jobs = Jobs::new(1, Duration::from_millis(300));
        /// A job's name, the state it ended in, whether it had a result, and its error.
        type End = (String, &'static str, bool, Option<String>);
        let seen: Arc<Mutex<Vec<End>>> = Default::default();
        let tell = |name: &'static str| {
            let seen = seen.clone();
            move |e: &Ended<'_>| seen.lock().unwrap().push((name.to_string(), e.state.as_str(), e.result.is_some(), e.error.map(String::from)))
        };
        jobs.submit_with("done", "s", Depth::Quick, |_| async { done(json!({"n": 1})) }, tell("done"));
        jobs.submit_with("partial", "s", Depth::Quick, |_| async { Ok(Finish::Partial(json!({}))) }, tell("partial"));
        jobs.submit_with("bad", "s", Depth::Quick, |_| async { Err("no provider".to_string()) }, tell("bad"));
        for id in ["done", "partial", "bad"] {
            jobs.wait(id, "s", Duration::from_secs(5)).await.unwrap();
        }
        jobs.submit_with("slow", "s", Depth::Quick, |_| async { std::future::pending::<Result<Finish, String>>().await }, tell("slow"));
        jobs.wait("slow", "s", Duration::from_secs(5)).await.unwrap();
        jobs.submit_with("run", "s", Depth::Quick, |_| async { std::future::pending::<Result<Finish, String>>().await }, tell("run"));
        jobs.submit_with("queued", "s", Depth::Quick, |_| async { done(json!({})) }, tell("queued"));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(jobs.cancel("queued", "s") && jobs.cancel("run", "s"));
        for id in ["run", "queued"] {
            jobs.wait(id, "s", Duration::from_secs(5)).await.unwrap();
        }
        let mut seen = seen.lock().unwrap().clone();
        seen.sort();
        let want = [("bad", "failed", false, Some("no provider")), ("done", "done", true, None), ("partial", "partial", true, None), ("queued", "cancelled", false, None), ("run", "cancelled", false, None), ("slow", "failed", false, Some("the job ran out of time"))];
        assert_eq!(seen, want.map(|(n, s, r, e)| (n.to_string(), s, r, e.map(String::from))).to_vec());
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
