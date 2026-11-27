//! One admission function over one ledger (`specs/143` §2.3 items 3, 6 and 7).
//!
//! * **The ledger** ([`Ledger`]): one append-only file in the daemon's home (`bridge/ledger.jsonl`; the home, because a credential
//!   serves every workspace). One line when a bridged call starts and one when it ends: when, the job, the origin, the credential
//!   (a hash of its reference, never the token), the endpoint, the tokens reported or estimated, and how it ended. Limits are
//!   computed from it, so they survive a restart, and a call that timed out, failed or was stopped counts: as one call, with its
//!   reported tokens or the estimate made from its prompt when it started.
//! * **[`job_may_run`]**: whether a call may start. `Admit`, `Wait(until)` or `Refuse(cause)`. Keyed by credential, so two
//!   endpoints that sign in with one subscription share one count. A rolling window, a daily cap, an absolute ceiling that nothing
//!   passes, a fixed share for jobs nobody started, the weekly budgets by job, the switches, and the pause while the operator is
//!   working or after spent quota. Pure: the same inputs give the same answer, and one table test runs every job against every
//!   origin and limit state.
//! * **[`Gate`]**: bridged calls yield. A Council or Panel seat or an agent's span never checks the bridge and never waits for it;
//!   a bridged call waits while one is starting or running on its endpoint, while its endpoint already runs its cap of bridged
//!   calls, and, when nobody asked for it, while another such job runs anywhere. Waiting calls go in order.
//!
//! The typed confirmation that makes an origin `confirmed`, the commands, and the `[bridge.jobs]` modes are slice F: this
//! function already answers for `confirmed` and takes a job's mode, and nothing here can produce either yet.

use crate::daemon_notice::{Cause, Job};
use delphi_adapter::config::BridgeLimits;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Who started the job (§2.3 item 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// Nobody: the daemon, while idle. Held to its share, and paused while the operator is working.
    Idle,
    /// A command, a console command or an app button. The whole window and the whole daily cap; no pause.
    Command,
    /// A `command` whose operator typed the confirmation at a terminal (slice F). Passes the window and the daily cap; nothing
    /// passes the ceiling.
    Confirmed,
}

impl Origin {
    pub const ALL: [Origin; 3] = [Origin::Idle, Origin::Command, Origin::Confirmed];

    pub fn code(self) -> &'static str {
        match self {
            Origin::Idle => "idle",
            Origin::Command => "command",
            Origin::Confirmed => "confirmed",
        }
    }
}

/// The answer of [`job_may_run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Admit,
    /// Not now: the job stays due and raises nothing. The time is when it is worth asking again, in Unix seconds.
    Wait(i64),
    /// No: the job stays due, and the cause is its notice (`budget`, `quota` or `no_route`).
    Refuse(Cause),
}

/// How long the operator's last sign of work pauses idle jobs (§2.3 item 7).
pub const PAUSE_SECS: i64 = 15 * 60;
const DAY: i64 = 86_400;
const WEEK: i64 = 7 * DAY;

/// The weekly budget of a job, in tokens (§2.3 item 6), or `None` for a job bound only by the limits. Pages: `[library.pages]
/// week_tokens` (`specs/139` §2.3 item 8). Summaries: `[library] summary_week_tokens` (`specs/142` §2.6 item 3). Consolidation: `[memory] consolidate_tokens`.
pub fn weekly_budget(job: Job, i: &Inputs<'_>) -> Option<u64> {
    match job {
        Job::Pages => Some(i.pages_week_tokens),
        Job::Summaries => Some(i.summary_week_tokens),
        Job::Consolidation => Some(i.consolidate_tokens),
        Job::Validation | Job::SessionSummaries => None,
    }
}

/// When a job whose weekly budget is spent may next spend (`specs/142` §2.6 item 3: `delphi kb health` says when the budget next
/// allows more): the moment enough of the week's oldest calls of `job` on `credential` have left the seven days for the rest to be
/// under `budget`. `None` while it is under the budget now.
pub fn weekly_free_at(calls: &[Call], job: Job, credential: &str, budget: u64, now: i64) -> Option<i64> {
    let mut week: Vec<&Call> = calls.iter().filter(|c| c.job == Some(job) && c.credential == credential && c.at > now - WEEK).collect();
    let mut spent: u64 = week.iter().map(|c| c.tokens).sum();
    if spent < budget {
        return None;
    }
    week.sort_by_key(|c| c.at);
    for c in week {
        spent = spent.saturating_sub(c.tokens);
        if spent < budget {
            return Some(c.at + WEEK);
        }
    }
    Some(now + WEEK)
}

/// One call, as the ledger holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub id: String,
    pub at: i64,
    /// `None`: a call made outside the closed list of jobs (a decision brief, the scout's research for a session).
    pub job: Option<Job>,
    pub origin: Origin,
    pub credential: String,
    pub endpoint: String,
    /// The tokens it counts for: what the CLI reported, or the estimate from its prompt while it has not ended or when it failed.
    pub tokens: u64,
    /// How it ended; `None` while it runs, or when the daemon died under it.
    pub outcome: Option<String>,
}

/// The ledger file. Reads are tolerant: a line that does not parse is skipped, and a missing file is empty.
#[derive(Debug, Clone)]
pub struct Ledger {
    path: PathBuf,
}

/// The name a credential has in the ledger: a hash of its reference. The reference itself is a name, not a secret; the hash keeps
/// even that out of the file, and the token is never near it.
pub fn credential_id(reference: &str) -> String {
    blake3::hash(reference.as_bytes()).to_hex().chars().take(16).collect()
}

impl Ledger {
    pub fn in_home(daemon_home: &Path) -> Ledger {
        Ledger { path: daemon_home.join("bridge/ledger.jsonl") }
    }

    fn append(&self, line: Value) {
        use std::io::Write;
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{line}");
        }
    }

    /// A call starts. `estimate` is its tokens as guessed from its prompt: what it counts for until it ends.
    pub fn start(&self, call: &Call) {
        self.append(json!({"ev": "start", "id": call.id, "t": call.at, "job": call.job.map(Job::code), "origin": call.origin.code(), "cred": call.credential, "endpoint": call.endpoint, "est": call.tokens}));
    }

    /// A call ends, however it ends. `tokens` is what the CLI reported, or `None` when it reported none: the estimate stands.
    pub fn end(&self, id: &str, now: i64, tokens: Option<u64>, estimated: bool, outcome: &str) {
        self.append(json!({"ev": "end", "id": id, "t": now, "tokens": tokens, "estimated": estimated, "outcome": outcome}));
    }

    /// Every call the file holds, oldest first.
    pub fn calls(&self) -> Vec<Call> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let mut calls: Vec<Call> = Vec::new();
        let mut at: HashMap<String, usize> = HashMap::new();
        for v in text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
            let Some(id) = v["id"].as_str() else { continue };
            match v["ev"].as_str() {
                Some("start") => {
                    let origin = Origin::ALL.into_iter().find(|o| Some(o.code()) == v["origin"].as_str()).unwrap_or(Origin::Command);
                    let job = Job::ALL.into_iter().find(|j| Some(j.code()) == v["job"].as_str());
                    at.insert(id.to_string(), calls.len());
                    calls.push(Call { id: id.into(), at: v["t"].as_i64().unwrap_or(0), job, origin, credential: v["cred"].as_str().unwrap_or("").into(), endpoint: v["endpoint"].as_str().unwrap_or("").into(), tokens: v["est"].as_u64().unwrap_or(0), outcome: None });
                }
                Some("end") => {
                    if let Some(c) = at.get(id).and_then(|i| calls.get_mut(*i)) {
                        if let Some(t) = v["tokens"].as_u64() {
                            c.tokens = t;
                        }
                        c.outcome = Some(v["outcome"].as_str().unwrap_or("").to_string());
                    }
                }
                _ => {}
            }
        }
        calls
    }
}

/// What [`job_may_run`] reads besides its three arguments. Everything is passed in, so the function is a table.
#[derive(Debug, Clone)]
pub struct Inputs<'a> {
    pub now: i64,
    /// The ledger's calls ([`Ledger::calls`]).
    pub calls: &'a [Call],
    pub limits: &'a BridgeLimits,
    /// The job's own switch (`[library.pages] enabled` and `auto_rebuild`, `[memory] consolidate`, `[session] summary`, the
    /// validator's background switch) and, from slice F, its `[bridge.jobs]` mode. A "no" is a refusal nothing here turns into a yes.
    pub switched_on: bool,
    /// `[memory] consolidate_tokens`, the weekly budget of consolidation.
    pub consolidate_tokens: u64,
    /// `[library.pages] week_tokens`, the weekly budget of pages (`specs/139` §2.3 item 8).
    pub pages_week_tokens: u64,
    /// The tokens this start sets aside against the job's weekly budget: 0 for one call, the worst case of the whole build before
    /// a page starts (`specs/139` §2.3 item 8: in proportion to the page's depth). The job is refused when what it spent in the
    /// week and the reserve together pass the budget.
    pub reserve: u64,
    /// The call goes through the bridge, so the subscription's limits apply (the ceiling, the window, the daily cap, the idle
    /// share, the pauses). `false` for a job served by an API endpoint: only its switch and its weekly budget apply, since the
    /// limits are the subscription's.
    pub bridged: bool,
    /// `[library] summary_week_tokens`, the weekly budget of source summaries (`specs/142` §2.6 item 3).
    pub summary_week_tokens: u64,
    /// The last sign of the operator at work: a console call, a host session's hook, or a host tool call over MCP. Unix seconds.
    pub last_active: i64,
}

/// A count of calls and tokens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Used {
    calls: u64,
    tokens: u64,
}

fn used<'a>(calls: impl Iterator<Item = &'a Call>) -> Used {
    calls.fold(Used::default(), |u, c| Used { calls: u.calls + 1, tokens: u.tokens + c.tokens })
}

fn over(u: Used, calls: u64, tokens: u64) -> bool {
    u.calls >= calls || u.tokens >= tokens
}

fn day_start(now: i64) -> i64 {
    now - now.rem_euclid(DAY)
}

/// The tokens `job` spent in the seven days before `now`, over every credential and endpoint: a weekly budget is the job's, however
/// its calls were served (§2.3 item 6; `specs/139` §2.3 item 8).
pub fn week_spent(calls: &[Call], job: Job, now: i64) -> u64 {
    calls.iter().filter(|c| c.job == Some(job) && c.at > now - WEEK).map(|c| c.tokens).sum()
}

/// Whether `job`'s weekly budget leaves room for `reserve` more (`true` for a job with no weekly budget).
fn week_has_room(job: Option<Job>, i: &Inputs<'_>) -> bool {
    match job.and_then(|j| weekly_budget(j, i).map(|b| (j, b))) {
        Some((j, budget)) => {
            let week = week_spent(i.calls, j, i.now);
            week < budget && week.saturating_add(i.reserve) <= budget
        }
        None => true,
    }
}

/// May a call of `job`, started by `origin`, signed in by `credential` ([`credential_id`]), start now (§2.3 item 6)?
///
/// In order: the job's switch; the ceiling, which nothing passes; `confirmed`, which passes what follows; the rolling window and
/// the daily cap, of which jobs nobody started have a fixed share; the job's weekly budget, with the start's reserve; and, for a
/// job nobody started, the pause while the operator is working and after spent quota until the window has passed. A job served by
/// an API endpoint (`bridged` false) meets only its switch and its weekly budget.
pub fn job_may_run(job: Option<Job>, origin: Origin, credential: &str, i: &Inputs<'_>) -> Admission {
    if !i.switched_on {
        return Admission::Refuse(Cause::NoRoute);
    }
    if !i.bridged {
        return if origin == Origin::Confirmed || week_has_room(job, i) { Admission::Admit } else { Admission::Refuse(Cause::Budget) };
    }
    let l = i.limits;
    let window = (l.window_hours as i64) * 3600;
    let mine: Vec<&Call> = i.calls.iter().filter(|c| c.credential == credential).collect();
    let today: Vec<&Call> = mine.iter().copied().filter(|c| c.at >= day_start(i.now)).collect();
    let recent: Vec<&Call> = mine.iter().copied().filter(|c| c.at > i.now - window).collect();
    let (day, win) = (used(today.iter().copied()), used(recent.iter().copied()));

    // The ceiling: nothing passes it, whoever asks.
    if over(day, l.daily_calls.saturating_mul(l.ceiling_factor), l.daily_tokens.saturating_mul(l.ceiling_factor)) {
        return Admission::Refuse(Cause::Budget);
    }
    if origin == Origin::Confirmed {
        return Admission::Admit;
    }
    if over(win, l.window_calls, l.window_tokens) || over(day, l.daily_calls, l.daily_tokens) {
        return Admission::Refuse(Cause::Budget);
    }
    if !week_has_room(job, i) {
        return Admission::Refuse(Cause::Budget);
    }
    if origin == Origin::Idle {
        // Half is always there for what the operator starts.
        let share = |n: u64| ((n as f64) * l.idle_share).floor() as u64;
        let idle = |calls: &[&Call]| used(calls.iter().copied().filter(|c| c.origin == Origin::Idle));
        if over(idle(&recent), share(l.window_calls), share(l.window_tokens)) || over(idle(&today), share(l.daily_calls), share(l.daily_tokens)) {
            return Admission::Refuse(Cause::Budget);
        }
        // After spent quota, idle jobs wait until the rolling window has passed.
        if let Some(spent) = recent.iter().filter(|c| c.outcome.as_deref() == Some("quota")).map(|c| c.at).max() {
            return Admission::Wait(spent + window);
        }
        if i.now - i.last_active < PAUSE_SECS {
            return Admission::Wait(i.last_active + PAUSE_SECS);
        }
    }
    Admission::Admit
}

// ── the gate: bridged calls yield ─────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct GateState {
    /// Seats and spans starting or running, by endpoint.
    seats: HashMap<String, usize>,
    /// Bridged calls running, by endpoint.
    bridged: HashMap<String, usize>,
    /// A job nobody asked for is running a bridged call, on any endpoint.
    idle_running: bool,
    /// The calls waiting on each endpoint, in the order they came.
    waiting: HashMap<String, VecDeque<u64>>,
    next: u64,
}

/// Who runs on a harness endpoint now (§2.3 item 3).
#[derive(Default)]
pub struct Gate {
    state: Mutex<GateState>,
    changed: tokio::sync::Notify,
}

/// A seat or a span on an endpoint, from before it starts until it has ended.
pub struct SeatGuard<'a> {
    gate: &'a Gate,
    endpoint: String,
}

/// A bridged call that was let in; the slot is given back when it is dropped.
pub struct CallGuard<'a> {
    gate: &'a Gate,
    endpoint: String,
    idle: bool,
}

/// A place in an endpoint's line, given up if the call is dropped while it waits.
struct Ticket<'a> {
    gate: &'a Gate,
    endpoint: &'a str,
    n: u64,
}

impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        let mut s = self.gate.state.lock().unwrap();
        if let Some(q) = s.waiting.get_mut(self.endpoint) {
            q.retain(|t| *t != self.n);
        }
        drop(s);
        self.gate.changed.notify_waiters();
    }
}

impl Gate {
    /// A seat or a span starts on `endpoint`. It starts at once, whatever the bridge is doing: it checks nothing and waits for
    /// nothing.
    pub fn seat(&self, endpoint: &str) -> SeatGuard<'_> {
        *self.state.lock().unwrap().seats.entry(endpoint.to_string()).or_insert(0) += 1;
        SeatGuard { gate: self, endpoint: endpoint.to_string() }
    }

    /// A bridged call asks to start on `endpoint`. It waits, in the order it came, while a seat or a span is starting or running
    /// there, while `parallel` bridged calls already run there, and, when `idle`, while another idle job's call runs anywhere.
    pub async fn enter(&self, endpoint: &str, idle: bool, parallel: usize) -> CallGuard<'_> {
        let n = {
            let mut s = self.state.lock().unwrap();
            s.next += 1;
            let n = s.next;
            s.waiting.entry(endpoint.to_string()).or_default().push_back(n);
            n
        };
        let ticket = Ticket { gate: self, endpoint, n };
        loop {
            // Registered before the state is read, so a change between the read and the wait is not missed.
            let changed = self.changed.notified();
            {
                let mut s = self.state.lock().unwrap();
                let first = s.waiting.get(endpoint).and_then(|q| q.front()) == Some(&n);
                let free = s.seats.get(endpoint).copied().unwrap_or(0) == 0 && s.bridged.get(endpoint).copied().unwrap_or(0) < parallel.max(1) && !(idle && s.idle_running);
                if first && free {
                    *s.bridged.entry(endpoint.to_string()).or_insert(0) += 1;
                    if idle {
                        s.idle_running = true;
                    }
                    drop(s);
                    drop(ticket);
                    return CallGuard { gate: self, endpoint: endpoint.to_string(), idle };
                }
            }
            changed.await;
        }
    }

    /// Bridged calls running and waiting on `endpoint` (tests, and the doctor).
    pub fn bridged(&self, endpoint: &str) -> (usize, usize) {
        let s = self.state.lock().unwrap();
        (s.bridged.get(endpoint).copied().unwrap_or(0), s.waiting.get(endpoint).map(|q| q.len()).unwrap_or(0))
    }
}

impl Drop for SeatGuard<'_> {
    fn drop(&mut self) {
        if let Some(n) = self.gate.state.lock().unwrap().seats.get_mut(&self.endpoint) {
            *n = n.saturating_sub(1);
        }
        self.gate.changed.notify_waiters();
    }
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        {
            let mut s = self.gate.state.lock().unwrap();
            if let Some(n) = s.bridged.get_mut(&self.endpoint) {
                *n = n.saturating_sub(1);
            }
            if self.idle {
                s.idle_running = false;
            }
        }
        self.gate.changed.notify_waiters();
    }
}

/// What the daemon writes about a bridged call once it has ended (§2.2 item 3, §2.3 item 1): the seat receipt and the usage. Left
/// here by the engine and raised by the daemon on its timer, as a job's failure is.
#[derive(Debug, Clone, PartialEq)]
pub struct CallRecord {
    pub endpoint_id: String,
    pub model_id: String,
    /// The tier the pool lists the model under.
    pub model_tier: String,
    pub served: Option<String>,
    pub job: Option<Job>,
    pub tokens: u64,
    /// Of those, the tokens of the answer.
    pub tokens_out: u64,
    pub estimated: bool,
    pub prompt_hash: String,
    pub prompt_bytes: u64,
    /// The call's id in the ledger.
    pub id: String,
    /// `ok`, or the cause code of the failure.
    pub outcome: String,
}

/// The engine's state for the bridge: the gate, the last sign of the operator at work, and what waits to be written.
#[derive(Default)]
pub struct Bridge {
    pub gate: Gate,
    /// Unix seconds of the last console call that was the operator at work, host hook, or host tool call over MCP.
    pub last_active: std::sync::atomic::AtomicI64,
    records: Mutex<Vec<CallRecord>>,
}

impl Bridge {
    pub fn active(&self, now: i64) {
        self.last_active.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn record(&self, r: CallRecord) {
        self.records.lock().unwrap().push(r);
    }

    pub fn drain(&self) -> Vec<CallRecord> {
        std::mem::take(&mut *self.records.lock().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const NOW: i64 = 1_795_000_000 - 1_795_000_000 % DAY + 20 * 3600; // 20:00 UTC, so five hours back is still today
    const CRED: &str = "cred-a";

    fn call(n: usize, at: i64, origin: Origin, tokens: u64) -> Call {
        Call { id: format!("c{n}"), at, job: Some(Job::Pages), origin, credential: CRED.into(), endpoint: "claude-sub".into(), tokens, outcome: Some("ok".into()) }
    }

    /// `n` calls of `tokens` each, by `origin`, spread over the last `within` seconds.
    fn calls(n: usize, origin: Origin, tokens: u64, within: i64) -> Vec<Call> {
        (0..n).map(|k| call(k, NOW - 1 - (k as i64 * within) / (n.max(1) as i64), origin, tokens)).collect()
    }

    fn ask(job: Option<Job>, origin: Origin, calls: &[Call], last_active: i64, on: bool) -> Admission {
        let limits = BridgeLimits::default();
        job_may_run(job, origin, CRED, &Inputs { now: NOW, calls, limits: &limits, switched_on: on, consolidate_tokens: 30_000, summary_week_tokens: 300_000, pages_week_tokens: 600_000, reserve: 0, bridged: true, last_active })
    }

    const QUIET: i64 = NOW - 3600;

    /// `specs/142` §2.6 item 3 and AC-21: the summaries' row reads `[library] summary_week_tokens`, a command is refused past it and
    /// passes only `confirmed`, and the week's oldest call leaving the seven days is when it allows more.
    #[test]
    fn the_summaries_row_reads_summary_week_tokens_and_says_when_it_allows_more() {
        let limits = BridgeLimits::default();
        let spent: Vec<Call> = (0..2).map(|k| Call { job: Some(Job::Summaries), at: NOW - 86_400 * (3 - k as i64), ..call(k, 0, Origin::Idle, 6_000) }).collect();
        let at = |budget: u64, origin: Origin| job_may_run(Some(Job::Summaries), origin, CRED, &Inputs { now: NOW, calls: &spent, limits: &limits, switched_on: true, consolidate_tokens: 30_000, summary_week_tokens: budget, pages_week_tokens: 600_000, reserve: 0, bridged: true, last_active: QUIET });
        assert_eq!(at(300_000, Origin::Command), Admission::Admit);
        assert_eq!(at(12_000, Origin::Command), Admission::Refuse(Cause::Budget));
        assert_eq!(at(12_000, Origin::Confirmed), Admission::Admit);
        assert_eq!(weekly_free_at(&spent, Job::Summaries, CRED, 300_000, NOW), None);
        assert_eq!(weekly_free_at(&spent, Job::Summaries, CRED, 12_000, NOW), Some(NOW - 3 * 86_400 + WEEK));
    }

    // AC-16 and the fitness function "no limit is passed without a typed confirmation, and none past the ceiling": every job,
    // against every origin, every limit state and the switch, gives the answer written here.
    #[test]
    fn the_admission_table() {
        use Admission::{Admit, Refuse, Wait};
        let d = BridgeLimits::default();
        assert_eq!((d.window_hours, d.window_calls, d.window_tokens, d.daily_calls, d.daily_tokens, d.ceiling_factor, d.idle_share), (5, 30, 300_000, 100, 1_000_000, 3, 0.5));
        let budget = Refuse(Cause::Budget);
        // The limit states, each a ledger: (name, calls, [idle, command, confirmed] for a job with no weekly budget).
        let states: Vec<(&str, Vec<Call>, [Admission; 3])> = vec![
            ("nothing spent", vec![], [Admit, Admit, Admit]),
            ("14 idle calls in the window", calls(14, Origin::Idle, 100, 4 * 3600), [Admit, Admit, Admit]),
            ("15 idle calls in the window: idle is at half", calls(15, Origin::Idle, 100, 4 * 3600), [budget, Admit, Admit]),
            ("15 command calls in the window: idle has its own half", calls(15, Origin::Command, 100, 4 * 3600), [Admit, Admit, Admit]),
            ("29 calls in the window", calls(29, Origin::Command, 100, 4 * 3600), [Admit, Admit, Admit]),
            ("30 calls in five hours: the thirty-first", calls(30, Origin::Command, 100, 4 * 3600), [budget, budget, Admit]),
            ("150,000 idle tokens in the window: idle is at half", calls(3, Origin::Idle, 50_000, 3600), [budget, Admit, Admit]),
            ("300,000 tokens in the window", calls(3, Origin::Command, 100_000, 3600), [budget, budget, Admit]),
            ("50 idle calls today, outside the window: idle is at half the day", calls(50, Origin::Idle, 100, 13 * 3600).into_iter().map(|mut c| { c.at -= 6 * 3600; c }).collect(), [budget, Admit, Admit]),
            ("100 calls today, outside the window: the hundred-and-first", calls(100, Origin::Command, 100, 13 * 3600).into_iter().map(|mut c| { c.at -= 6 * 3600; c }).collect(), [budget, budget, Admit]),
            ("1,000,000 tokens today, outside the window", calls(4, Origin::Command, 250_000, 3600).into_iter().map(|mut c| { c.at -= 6 * 3600; c }).collect(), [budget, budget, Admit]),
            ("300 calls today: the ceiling", calls(300, Origin::Confirmed, 100, 19 * 3600), [budget, budget, budget]),
            ("3,000,000 tokens today: the ceiling", calls(3, Origin::Confirmed, 1_000_000, 3600), [budget, budget, budget]),
            ("100 calls yesterday", calls(100, Origin::Command, 100, 3600).into_iter().map(|mut c| { c.at -= DAY; c }).collect(), [Admit, Admit, Admit]),
            ("a call that timed out counts", (0..30).map(|k| Call { outcome: Some("timeout".into()), ..call(k, NOW - 60 - k as i64, Origin::Command, 100) }).collect(), [budget, budget, Admit]),
            ("a call the daemon died under counts", (0..30).map(|k| Call { outcome: None, ..call(k, NOW - 60 - k as i64, Origin::Command, 100) }).collect(), [budget, budget, Admit]),
            ("quota was spent an hour ago", vec![Call { outcome: Some("quota".into()), ..call(0, NOW - 3600, Origin::Command, 100) }], [Wait(NOW - 3600 + 5 * 3600), Admit, Admit]),
            ("quota was spent six hours ago", vec![Call { outcome: Some("quota".into()), ..call(0, NOW - 6 * 3600, Origin::Command, 100) }], [Admit, Admit, Admit]),
            ("another credential spent everything", calls(300, Origin::Command, 100_000, 3600).into_iter().map(|mut c| { c.credential = "cred-b".into(); c }).collect(), [Admit, Admit, Admit]),
        ];
        for (name, ledger, want) in &states {
            for job in [Some(Job::Validation), Some(Job::SessionSummaries), None] {
                for (origin, want) in Origin::ALL.iter().zip(want) {
                    assert_eq!(ask(job, *origin, ledger, QUIET, true), *want, "{name}: {job:?} {origin:?}");
                    // The switch: a "no" is a refusal whatever the ledger and whoever asks.
                    assert_eq!(ask(job, *origin, ledger, QUIET, false), Refuse(Cause::NoRoute), "{name}: {job:?} {origin:?}, switched off");
                }
            }
        }
        // Every job of the closed list has a row for its weekly budget: (the job, tokens of that job this week, [idle, command, confirmed]).
        for job in Job::ALL {
            let (spent, want) = match job {
                Job::Pages => (600_000, [budget, budget, Admit]),
                Job::Summaries => (300_000, [budget, budget, Admit]),
                Job::Consolidation => (30_000, [budget, budget, Admit]),
                // No weekly budget here: bound by the limits and, for validation, by the share `specs/040` keeps.
                Job::Validation | Job::SessionSummaries => (600_000, [Admit, Admit, Admit]),
            };
            // Spent over the week, three days back, so neither the window nor the day is touched.
            let week: Vec<Call> = (0..6).map(|k| Call { job: Some(job), ..call(k, NOW - 3 * DAY - k as i64, Origin::Command, spent / 6) }).collect();
            for (origin, want) in Origin::ALL.iter().zip(want) {
                assert_eq!(ask(Some(job), *origin, &week, QUIET, true), want, "{job:?} {origin:?} at its weekly budget");
            }
            let under: Vec<Call> = week.iter().take(5).cloned().collect();
            assert_eq!(ask(Some(job), Origin::Idle, &under, QUIET, true), Admit, "{job:?} under its weekly budget");
            // Another job's spend is not this job's budget.
            let other = Job::ALL.into_iter().find(|j| *j != job && !matches!(j, Job::Pages | Job::Summaries | Job::Consolidation)).unwrap();
            let theirs: Vec<Call> = week.iter().cloned().map(|c| Call { job: Some(other), ..c }).collect();
            assert_eq!(ask(Some(job), Origin::Idle, &theirs, QUIET, true), Admit, "{job:?}: {other:?} spent, not it");
        }
        // The pause (§2.3 item 7): an idle job does not start within 15 minutes of the operator's last sign of work; a command does.
        for (ago, want) in [(0, Wait(NOW + PAUSE_SECS)), (PAUSE_SECS - 1, Wait(NOW + 1)), (PAUSE_SECS, Admit), (3600, Admit)] {
            assert_eq!(ask(Some(Job::Pages), Origin::Idle, &[], NOW - ago, true), want, "idle, {ago}s after the operator");
            assert_eq!(ask(Some(Job::Pages), Origin::Command, &[], NOW - ago, true), Admit, "a command does not pause");
        }
    }

    // AC-16: the counts are the same after the daemon restarts (they are the file's), a call that never ended counts with the
    // estimate made when it started, two endpoints with one credential share one count, and nothing of a token is in the file.
    #[test]
    fn the_ledger_is_the_count_and_survives_a_restart() {
        let home = tempfile::tempdir().unwrap();
        let cred = credential_id("keychain:bridge-claude");
        assert_eq!((cred.len(), credential_id("keychain:bridge-claude") == cred, credential_id("keychain:other") == cred), (16, true, false));
        let ledger = Ledger::in_home(home.path());
        assert!(ledger.calls().is_empty(), "no file is no calls");
        for k in 0..30usize {
            let endpoint = if k % 2 == 0 { "claude-sub" } else { "claude-alt" };
            let c = Call { id: format!("c{k}"), at: NOW - 100 + k as i64, job: Some(Job::Validation), origin: Origin::Command, credential: cred.clone(), endpoint: endpoint.into(), tokens: 400, outcome: None };
            ledger.start(&c);
            match k % 3 {
                0 => ledger.end(&c.id, c.at + 2, Some(1000), false, "ok"),
                1 => ledger.end(&c.id, c.at + 2, None, true, "timeout"),
                _ => {} // the daemon died under it
            }
        }
        // A new daemon: another handle on the same home.
        let again = Ledger::in_home(home.path()).calls();
        assert_eq!(again.len(), 30);
        assert_eq!(again.iter().map(|c| c.tokens).sum::<u64>(), 10 * 1000 + 20 * 400, "reported where it was, the estimate where it was not");
        assert_eq!(again.iter().filter(|c| c.outcome.is_none()).count(), 10);
        let limits = BridgeLimits::default();
        let inputs = Inputs { now: NOW, calls: &again, limits: &limits, switched_on: true, consolidate_tokens: 30_000, summary_week_tokens: 300_000, pages_week_tokens: 600_000, reserve: 0, bridged: true, last_active: QUIET };
        assert_eq!(job_may_run(Some(Job::Validation), Origin::Command, &cred, &inputs), Admission::Refuse(Cause::Budget), "30 calls over two endpoints of one credential");
        assert_eq!(job_may_run(Some(Job::Validation), Origin::Command, &credential_id("keychain:other"), &inputs), Admission::Admit);
        let text = std::fs::read_to_string(home.path().join("bridge/ledger.jsonl")).unwrap();
        assert!(!text.contains("keychain") && !text.contains("bridge-claude") && text.lines().count() == 50, "a hash of the reference, never the reference or a token");
        // A torn line is skipped, not fatal.
        std::fs::write(home.path().join("bridge/ledger.jsonl"), format!("{text}{{\"ev\":\"start\",\"id\"")).unwrap();
        assert_eq!(Ledger::in_home(home.path()).calls().len(), 30);
    }

    // AC-15 and the fitness function "a seat or a span never waits for a bridged call": with two bridged calls running and three
    // queued on an endpoint, a seat that starts there starts at once; a bridged call that would start while a seat is starting or
    // running waits until it has ended. AC-6: with the cap at 2 a third call waits; a second idle job waits for the first on any
    // endpoint; waiting calls go in order.
    #[tokio::test]
    async fn a_seat_starts_at_once_whatever_the_bridge_is_doing() {
        let gate = std::sync::Arc::new(Gate::default());
        let a = gate.enter("ep", false, 2).await;
        let b = gate.enter("ep", false, 2).await;
        let order = std::sync::Arc::new(Mutex::new(Vec::new()));
        let mut queued = Vec::new();
        for k in 0..3 {
            let (g, o) = (gate.clone(), order.clone());
            queued.push(tokio::spawn(async move {
                let _in = g.enter("ep", false, 2).await;
                o.lock().unwrap().push(k);
                tokio::time::sleep(Duration::from_millis(30)).await;
            }));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(gate.bridged("ep"), (2, 3), "the cap is two: the third, fourth and fifth wait");
        // A seat starts at once: `seat` is not async and takes no part in the line.
        let started = std::time::Instant::now();
        let seat = gate.seat("ep");
        assert!(started.elapsed() < Duration::from_millis(50));
        // Both running calls end; the queued ones still wait, because a seat is running.
        drop(a);
        drop(b);
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!((gate.bridged("ep"), order.lock().unwrap().len()), ((0, 3), 0), "a bridged call waits for the seat");
        // Another endpoint is not held up by this one's seat.
        let elsewhere = tokio::time::timeout(Duration::from_millis(200), gate.enter("other", false, 2)).await.expect("its own endpoint is free");
        drop(elsewhere);
        drop(seat);
        for q in queued {
            tokio::time::timeout(Duration::from_secs(5), q).await.unwrap().unwrap();
        }
        assert_eq!(*order.lock().unwrap(), vec![0, 1, 2], "in the order they came");
        assert_eq!(gate.bridged("ep"), (0, 0));

        // One idle job at a time, across endpoints; a call somebody asked for is not held by it.
        let idle = gate.enter("ep", true, 2).await;
        assert!(tokio::time::timeout(Duration::from_millis(100), gate.enter("other", true, 2)).await.is_err(), "a second idle job waits, on any endpoint");
        assert_eq!(gate.bridged("other"), (0, 0), "a call that gave up waiting leaves the line");
        let asked = tokio::time::timeout(Duration::from_millis(200), gate.enter("other", false, 2)).await.expect("a command is not an idle job");
        drop(idle);
        let next = tokio::time::timeout(Duration::from_millis(500), gate.enter("other", true, 2)).await.expect("the first idle job ended");
        drop((asked, next));
    }

    // `specs/139` AC-11 (slice F): the page budget is one row of the admission table. It is `[library.pages] week_tokens`, counted
    // over every credential and endpoint the job's calls went through; the reserve a build asks is added before it starts; a page
    // served by an API endpoint meets only its switch and that budget (none of the subscription's limits); confirmed passes it.
    #[test]
    fn the_page_budget_is_week_tokens_counted_over_every_endpoint_with_the_reserve() {
        use Admission::{Admit, Refuse};
        let budget = Refuse(Cause::Budget);
        let limits = BridgeLimits::default();
        let ask = |calls: &[Call], week_tokens: u64, reserve: u64, bridged: bool, origin: Origin, on: bool| {
            job_may_run(Some(Job::Pages), origin, CRED, &Inputs { now: NOW, calls, limits: &limits, switched_on: on, consolidate_tokens: 30_000, summary_week_tokens: 300_000, pages_week_tokens: week_tokens, reserve, bridged, last_active: QUIET })
        };
        // 100,000 tokens of pages three days back: half through the bridge's credential, half through an API endpoint.
        let week: Vec<Call> = (0..2).map(|k| Call { credential: if k == 0 { CRED.into() } else { "endpoint-api".into() }, ..call(k, NOW - 3 * DAY - k as i64, Origin::Command, 50_000) }).collect();
        assert_eq!(week_spent(&week, Job::Pages, NOW), 100_000);
        for bridged in [true, false] {
            for (week_tokens, reserve, want) in [(200_000, 0, Admit), (200_000, 100_000, Admit), (200_000, 100_001, budget), (100_000, 0, budget), (90_000, 0, budget), (600_000, 80_000, Admit)] {
                assert_eq!(ask(&week, week_tokens, reserve, bridged, Origin::Command, true), want, "bridged {bridged}: {week_tokens} with a reserve of {reserve}");
                assert_eq!(ask(&week, week_tokens, reserve, bridged, Origin::Idle, true), want, "bridged {bridged}, idle");
                assert_eq!(ask(&week, week_tokens, reserve, bridged, Origin::Confirmed, true), Admit, "confirmed passes the page budget");
            }
            assert_eq!(ask(&week, 600_000, 0, bridged, Origin::Command, false), Refuse(Cause::NoRoute), "a switch that says no");
        }
        // A week ago is outside the week.
        let old: Vec<Call> = week.iter().cloned().map(|c| Call { at: NOW - WEEK - 1, ..c }).collect();
        assert_eq!(ask(&old, 100_000, 80_000, true, Origin::Idle, true), Admit);
        // Through an API endpoint the subscription's limits do not apply: 30 calls in the window, the operator at work.
        let busy = calls(30, Origin::Idle, 100, 3600);
        assert_eq!(ask(&busy, 600_000, 80_000, true, Origin::Command, true), budget, "the bridge's window");
        assert_eq!(job_may_run(Some(Job::Pages), Origin::Idle, CRED, &Inputs { now: NOW, calls: &busy, limits: &limits, switched_on: true, consolidate_tokens: 30_000, summary_week_tokens: 300_000, pages_week_tokens: 600_000, reserve: 80_000, bridged: false, last_active: NOW }), Admit, "an API page is not held by the bridge's limits or its pause");
    }

    #[test]
    fn the_last_sign_of_the_operator_only_moves_forward() {
        let b = Bridge::default();
        b.active(100);
        b.active(50);
        assert_eq!(b.last_active.load(std::sync::atomic::Ordering::SeqCst), 100);
        b.record(CallRecord { endpoint_id: "e".into(), model_id: "m".into(), model_tier: "fast".into(), served: None, job: None, tokens: 1, tokens_out: 0, estimated: true, prompt_hash: "h".into(), prompt_bytes: 2, id: "c".into(), outcome: "ok".into() });
        assert_eq!((b.drain().len(), b.drain().len()), (1, 0));
    }
}
