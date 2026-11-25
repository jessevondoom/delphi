//! Failures of background jobs that the operator can see (`specs/143` §2.7).
//!
//! A job nobody asked for at that moment (an idle topic rebuild, background validation, a session summary, consolidation, a source
//! summary) that could not run raises a **notice**: a fixed cause code and fixed words, and never any text of a page, a claim or a
//! prompt. A notice is shown once for each cause in a day, across all jobs, with a count of the jobs it stopped: as one
//! `daemon.notice` event, one line in the error log at its default level, one line among the console's notices, one line in the next
//! session-start brief, and, for pages, one line of `delphi kb health`.
//!
//! The record is one small file in the workspace ([`FILE`]), because the brief and `delphi kb health` are other processes than the
//! daemon. Everything here takes a closed enum and writes only its code: there is no parameter a caller could pass entry text in.

use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::Mutex;

/// The record, in the workspace.
pub const FILE: &str = "local/daemon-notices.json";

/// Why a background job could not run (`specs/143` §2.7 item 1). Closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cause {
    /// No model can serve the call.
    NoRoute,
    /// A share, a page budget or a limit is reached.
    Budget,
    /// The endpoint's quota is spent.
    Quota,
    Timeout,
    /// The page did not pass its check.
    CheckFailed,
    /// The harness asked for a tool in a single call.
    HarnessTool,
}

impl Cause {
    pub const ALL: [Cause; 6] = [Cause::NoRoute, Cause::Budget, Cause::Quota, Cause::Timeout, Cause::CheckFailed, Cause::HarnessTool];

    /// The fixed code, as the event, the log and the record carry it.
    pub fn code(self) -> &'static str {
        match self {
            Cause::NoRoute => "no_route",
            Cause::Budget => "budget",
            Cause::Quota => "quota",
            Cause::Timeout => "timeout",
            Cause::CheckFailed => "check_failed",
            Cause::HarnessTool => "harness_tool",
        }
    }

    pub fn parse(code: &str) -> Option<Cause> {
        Cause::ALL.into_iter().find(|c| c.code() == code)
    }

    /// The fixed words for a line a person reads.
    pub fn words(self) -> &'static str {
        match self {
            Cause::NoRoute => "no model route",
            Cause::Budget => "a budget or a limit is reached",
            Cause::Quota => "the endpoint's quota is spent",
            Cause::Timeout => "the call timed out",
            Cause::CheckFailed => "the page did not pass its check",
            Cause::HarnessTool => "the harness asked for a tool in a single call",
        }
    }
}

/// The background jobs a notice covers (`specs/143` §2.7 item 2). Closed: a job added later joins this list and the test that
/// enumerates it (`service::tests::every_idle_job_failure_writes_a_notice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Job {
    /// The idle topic rebuild (`specs/117` §2 item 3).
    Pages,
    /// Source summaries (`specs/142` §2.6).
    Summaries,
    /// Background validation (`specs/040`).
    Validation,
    /// Session summaries (`specs/030` §4.3).
    SessionSummaries,
    /// Consolidation (`specs/116` §6).
    Consolidation,
}

impl Job {
    pub const ALL: [Job; 5] = [Job::Pages, Job::Summaries, Job::Validation, Job::SessionSummaries, Job::Consolidation];

    pub fn code(self) -> &'static str {
        match self {
            Job::Pages => "pages",
            Job::Summaries => "summaries",
            Job::Validation => "validation",
            Job::SessionSummaries => "session_summaries",
            Job::Consolidation => "consolidation",
        }
    }
}

/// One job that could not run. `key` is the configuration key the notice names, when a setting is what stopped the job (the
/// bridge switched off on an install where a harness could have served: §2.7 item 1); a fixed name, never a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failure {
    pub job: Job,
    pub cause: Cause,
    pub key: Option<&'static str>,
}

/// Where the engine leaves failures for the daemon to raise: the place that learns a job could not run is often deep in the engine,
/// with no mission and no console at hand. The daemon drains it on its timer (`EngineService::notices_tick`).
#[derive(Default)]
pub struct Sink(Mutex<Vec<Failure>>);

impl Sink {
    pub fn push(&self, f: Failure) {
        self.0.lock().unwrap().push(f);
    }

    pub fn drain(&self) -> Vec<Failure> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// What [`record`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The first failure with this cause today: the one that is an event and a log line.
    pub first: bool,
    /// How many jobs this cause stopped today, this one included.
    pub count: u64,
}

fn day_of(unix: i64) -> String {
    delphi_core::time::format_unix_millis(unix, 0).chars().take(10).collect()
}

fn read(ws_dir: &Path) -> Value {
    std::fs::read(ws_dir.join(FILE)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn write(ws_dir: &Path, v: &Value) -> bool {
    let path = ws_dir.join(FILE);
    if !path.parent().is_some_and(Path::is_dir) {
        return false;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, v.to_string()).and_then(|()| std::fs::rename(&tmp, &path)).is_ok()
}

/// Count one failure. A new day starts the counts again and forgets what the brief has said; the newest failure of each job is kept
/// across days, with the day its cause was first seen, until the job runs again ([`cleared`]). Writes nothing in a workspace that
/// has no `local/` directory, and then every failure reads as the first, so the log line is still written.
pub fn record(ws_dir: &Path, f: Failure, now: i64) -> Recorded {
    let today = day_of(now);
    let mut v = read(ws_dir);
    if v["day"].as_str() != Some(today.as_str()) {
        v["day"] = json!(today);
        v["causes"] = json!({});
        v["mentioned"] = json!([]);
    }
    let code = f.cause.code();
    let count = v["causes"][code]["count"].as_u64().unwrap_or(0) + 1;
    let mut jobs: Vec<String> = v["causes"][code]["jobs"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    if !jobs.iter().any(|j| j == f.job.code()) {
        jobs.push(f.job.code().to_string());
    }
    let key = f.key.map(String::from).or_else(|| v["causes"][code]["key"].as_str().map(String::from));
    v["causes"][code] = json!({"count": count, "jobs": jobs, "key": key});
    let since = match &v["last"][f.job.code()] {
        last if last["cause"].as_str() == Some(code) => last["since"].as_str().unwrap_or(&today).to_string(),
        _ => today.clone(),
    };
    v["last"][f.job.code()] = json!({"cause": code, "since": since});
    write(ws_dir, &v);
    Recorded { first: count == 1, count }
}

/// A job ran: its last failure is forgotten, so `delphi kb health` stops showing it. Today's counts stay.
pub fn cleared(ws_dir: &Path, job: Job) {
    let mut v = read(ws_dir);
    if v["last"].as_object_mut().and_then(|m| m.remove(job.code())).is_some() {
        write(ws_dir, &v);
    }
}

/// The newest failure of `job` that no later run has cleared: its cause and the day that cause was first seen.
pub fn last_failure(ws_dir: &Path, job: Job) -> Option<(Cause, String)> {
    let v = read(ws_dir);
    let last = &v["last"][job.code()];
    Some((Cause::parse(last["cause"].as_str()?)?, last["since"].as_str()?.to_string()))
}

fn causes_of(v: &Value) -> Vec<(Cause, u64, Option<String>)> {
    let empty = Map::new();
    let m = v["causes"].as_object().unwrap_or(&empty);
    Cause::ALL.into_iter().filter_map(|c| m.get(c.code()).map(|e| (c, e["count"].as_u64().unwrap_or(0), e["key"].as_str().map(String::from)))).filter(|(_, n, _)| *n > 0).collect()
}

fn times(n: u64) -> String {
    format!("{} time{}", n, if n == 1 { "" } else { "s" })
}

/// One notice as the console shows it: fixed words, the count, the cause code, and the key when a setting is the cause.
pub fn line(cause: Cause, count: u64, key: Option<&str>) -> String {
    let key = key.map(|k| format!("; `{}` decides it", k)).unwrap_or_default();
    format!("background jobs could not run {} today: {} ({}){}. `delphi doctor` says what serves them", times(count), cause.words(), cause.code(), key)
}

/// Today's notices, one line for each cause, for the console's notices. Reads only.
pub fn lines(ws_dir: &Path, now: i64) -> Vec<String> {
    let v = read(ws_dir);
    if v["day"].as_str() != Some(day_of(now).as_str()) {
        return Vec::new();
    }
    causes_of(&v).into_iter().map(|(c, n, key)| line(c, n, key.as_deref())).collect()
}

/// The brief's one line (§2.7 item 3): the causes of the recorded day that no brief has said yet, each with its count, and remember
/// that they have now been said. `None` when there are none, or when the record cannot be written (a line that would be repeated
/// in every brief is not said at all).
pub fn take_brief_line(ws_dir: &Path) -> Option<String> {
    let mut v = read(ws_dir);
    let said: Vec<String> = v["mentioned"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    let fresh: Vec<(Cause, u64, Option<String>)> = causes_of(&v).into_iter().filter(|(c, _, _)| !said.iter().any(|s| s == c.code())).collect();
    if fresh.is_empty() {
        return None;
    }
    v["mentioned"] = json!(said.into_iter().chain(fresh.iter().map(|(c, _, _)| c.code().to_string())).collect::<Vec<_>>());
    if !write(ws_dir, &v) {
        return None;
    }
    let parts: Vec<String> = fresh.iter().map(|(c, n, _)| format!("{} ({}, {})", c.words(), c.code(), times(*n))).collect();
    Some(format!("Delphi's background jobs could not run on {}: {}. The operator can run `delphi doctor` at a terminal.", v["day"].as_str().unwrap_or("an unknown day"), parts.join("; ")))
}

/// The line `delphi kb health` adds for pages (§2.7 item 3), when the newest idle rebuild failed.
pub fn pages_health_line(due: usize, cause: Cause, since: &str) -> String {
    format!("pages: {} due, last idle rebuild failed: {}, since {}", due, cause.words(), since)
}

/// The words of the error-log line (§2.7 item 4): fixed, with the cause code as its only context.
pub const LOG_SLUG: &str = "background_job_failed";
pub const LOG_WORDS: &str = "a background job could not run";

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_795_000_000;
    const DAY: i64 = 86_400;

    fn ws() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("local")).unwrap();
        t
    }

    fn fail(job: Job, cause: Cause) -> Failure {
        Failure { job, cause, key: None }
    }

    // `specs/143` §2.7 item 3 and AC-18: one notice for each cause in a day across all jobs, with a count; a new day starts again.
    #[test]
    fn a_cause_is_first_once_a_day_across_jobs_and_counts_the_jobs_it_stopped() {
        let t = ws();
        assert_eq!(record(t.path(), fail(Job::Pages, Cause::NoRoute), T0), Recorded { first: true, count: 1 });
        assert_eq!(record(t.path(), fail(Job::Validation, Cause::NoRoute), T0 + 3600), Recorded { first: false, count: 2 }, "another job, the same cause: no second notice");
        assert_eq!(record(t.path(), fail(Job::Pages, Cause::Quota), T0 + 3700), Recorded { first: true, count: 1 }, "another cause is its own notice");
        assert_eq!(lines(t.path(), T0 + 4000), vec![line(Cause::NoRoute, 2, None), line(Cause::Quota, 1, None)]);
        assert!(lines(t.path(), T0 + DAY).is_empty(), "yesterday's notices are not today's");
        assert_eq!(record(t.path(), fail(Job::Pages, Cause::NoRoute), T0 + DAY), Recorded { first: true, count: 1 });
    }

    // The brief says each cause once, in one line; a later cause is said once more.
    #[test]
    fn the_brief_line_is_said_once_for_each_cause() {
        let t = ws();
        assert_eq!(take_brief_line(t.path()), None);
        record(t.path(), fail(Job::Pages, Cause::NoRoute), T0);
        record(t.path(), fail(Job::Consolidation, Cause::NoRoute), T0);
        let said = take_brief_line(t.path()).expect("a line");
        assert!(said.contains("no model route (no_route, 2 times)") && said.contains("`delphi doctor`") && said.lines().count() == 1, "{said}");
        assert_eq!(take_brief_line(t.path()), None, "said once");
        record(t.path(), fail(Job::Pages, Cause::NoRoute), T0 + 60);
        assert_eq!(take_brief_line(t.path()), None, "the same cause, the same day: not said again");
        record(t.path(), fail(Job::Pages, Cause::Budget), T0 + 120);
        assert!(take_brief_line(t.path()).is_some_and(|l| l.contains("(budget, 1 time)") && !l.contains("no_route")));
    }

    // `delphi kb health` shows the newest failure of the idle rebuild with the day its cause was first seen, until a rebuild runs.
    #[test]
    fn the_last_failure_of_a_job_keeps_its_first_day_until_the_job_runs() {
        let t = ws();
        assert_eq!(last_failure(t.path(), Job::Pages), None);
        record(t.path(), fail(Job::Pages, Cause::NoRoute), T0);
        record(t.path(), fail(Job::Pages, Cause::NoRoute), T0 + 2 * DAY);
        let (cause, since) = last_failure(t.path(), Job::Pages).unwrap();
        assert_eq!((cause, since.as_str()), (Cause::NoRoute, day_of(T0).as_str()));
        assert_eq!(pages_health_line(9, cause, "2026-11-25"), "pages: 9 due, last idle rebuild failed: no model route, since 2026-11-25");
        record(t.path(), fail(Job::Pages, Cause::Budget), T0 + 3 * DAY);
        assert_eq!(last_failure(t.path(), Job::Pages), Some((Cause::Budget, day_of(T0 + 3 * DAY))), "a new cause has its own first day");
        cleared(t.path(), Job::Pages);
        assert_eq!(last_failure(t.path(), Job::Pages), None);
    }

    // The key is a fixed name, kept with the cause, and the line names it.
    #[test]
    fn a_notice_names_the_key_when_a_setting_is_the_cause() {
        let t = ws();
        record(t.path(), Failure { job: Job::Pages, cause: Cause::NoRoute, key: Some("[routing] single_call_harness") }, T0);
        let l = lines(t.path(), T0);
        assert!(l.len() == 1 && l[0].contains("`[routing] single_call_harness` decides it"), "{l:?}");
    }

    // A workspace with no `local/` is never created; every failure then reads as the first, so the log still gets its line.
    #[test]
    fn nothing_is_created_in_a_workspace_that_has_no_local_directory() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(record(t.path(), fail(Job::Pages, Cause::NoRoute), T0), Recorded { first: true, count: 1 });
        assert!(!t.path().join("local").exists());
        assert_eq!((take_brief_line(t.path()), lines(t.path(), T0)), (None, Vec::new()));
    }

    #[test]
    fn the_codes_are_fixed_and_distinct() {
        let codes: Vec<&str> = Cause::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(codes, ["no_route", "budget", "quota", "timeout", "check_failed", "harness_tool"]);
        assert!(Cause::ALL.iter().all(|c| Cause::parse(c.code()) == Some(*c)));
        assert_eq!(Job::ALL.iter().map(|j| j.code()).collect::<Vec<_>>(), ["pages", "summaries", "validation", "session_summaries", "consolidation"]);
    }
}
