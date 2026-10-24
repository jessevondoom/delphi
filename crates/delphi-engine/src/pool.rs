//! The model pool's runtime state (`specs/002` §4.1, D-26): which `(endpoint, model)` pairs can be used now.
//!
//! * `available`: usable.
//! * `cooling_down`: throttled; usable again when the cooldown ends (the provider's `retry-after`, or an
//!   exponential backoff).
//! * `exhausted`: quota, spend limit or subscription window used up. Leaves the state when a known reset
//!   time passes, or when a capped probe call succeeds (first probe `reprobe_first_secs` after the
//!   failure, doubling to `reprobe_max_secs`).
//!
//! Rule 3 of the classifier lives here because it needs history: `throttle_attempts` consecutive
//! throttles of one pair spread over at least `throttle_window_secs` count as exhaustion, for providers
//! that report a spent quota as an ordinary rate limit (Bedrock, the Gemini API).
//!
//! State is kept in memory and mirrored to `local/pool-state.json` after every change so a restart does
//! not rediscover a dead model. The event log is the record; this file is a cache of it, and a lost file
//! simply starts everything `available`.
//!
//! Nothing here routes yet: the engine records outcomes into the pool, and `resolve_route` (M3) will read it.

use delphi_adapter::config::PoolTuning;
use delphi_adapter::exhaust::Exhaustion;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// First cooldown after a throttle with no `retry-after`; doubles with each consecutive throttle.
const COOLDOWN_BASE_SECS: i64 = 120;
const COOLDOWN_MAX_SECS: i64 = 900;
/// Throttle times kept per pair (only the last few matter to rule 3).
const KEEP_THROTTLES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Available,
    CoolingDown,
    Exhausted,
}

impl State {
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Available => "available",
            State::CoolingDown => "cooling_down",
            State::Exhausted => "exhausted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub endpoint_id: String,
    pub model_id: String,
    pub state: State,
    /// Unix seconds of the last change of state.
    pub since: i64,
    /// Cooling down: when the cooldown ends. Exhausted: the reset time, when the provider gave one.
    pub until: Option<i64>,
    pub reason: String,
    /// Times of the consecutive throttles since the last success.
    pub throttles: Vec<i64>,
    /// Exhausted with no known reset: the wait before the next probe, doubled after each failed probe.
    pub probe_interval_secs: i64,
    pub next_probe_at: Option<i64>,
}

impl Entry {
    fn fresh(endpoint_id: &str, model_id: &str, now: i64) -> Entry {
        Entry { endpoint_id: endpoint_id.into(), model_id: model_id.into(), state: State::Available, since: now, until: None, reason: String::new(), throttles: vec![], probe_interval_secs: 0, next_probe_at: None }
    }
}

#[derive(Serialize, Deserialize)]
struct File {
    entries: Vec<Entry>,
}

type Key = (String, String);

pub struct Pool {
    inner: Mutex<BTreeMap<Key, Entry>>,
    path: Option<PathBuf>,
    tuning: PoolTuning,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

pub fn system_clock() -> Arc<dyn Fn() -> i64 + Send + Sync> {
    Arc::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
}

/// Read a state file. A missing or unreadable file is an empty pool: everything starts available.
pub fn load(path: &Path) -> Vec<Entry> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<File>(&t).ok()).map(|f| f.entries).unwrap_or_default()
}

/// The saved entries with expiries applied as of `now`, without writing anything (for `doctor` and `delphi models`).
pub fn view(path: &Path, now: i64) -> Vec<Entry> {
    load(path)
        .into_iter()
        .filter_map(|e| {
            match (e.state, e.until) {
                (State::CoolingDown, Some(t)) if t <= now => return None,
                (State::Exhausted, Some(t)) if t <= now => return None,
                _ => {}
            }
            Some(e)
        })
        .collect()
}

/// `5d 16h`, `14m`, `45s`.
pub fn fmt_secs(secs: i64) -> String {
    let s = secs.max(0);
    match s {
        0..=59 => format!("{}s", s),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d {}h", s / 86_400, s % 86_400 / 3600),
    }
}

/// One line saying where an entry stands, for `delphi models` and `doctor`.
pub fn describe(e: &Entry, now: i64) -> String {
    match (e.state, e.until, e.next_probe_at) {
        (State::Available, ..) => "available".into(),
        (State::CoolingDown, Some(t), _) => format!("cooling down, back in {} ({})", fmt_secs(t - now), e.reason),
        (State::CoolingDown, None, _) => format!("cooling down ({})", e.reason),
        (State::Exhausted, Some(t), _) => format!("exhausted: {}; resets {} (in {})", e.reason, delphi_core::time::format_unix_millis(t, 0).trim_end_matches(".000Z").replace('T', " "), fmt_secs(t - now)),
        (State::Exhausted, None, Some(p)) => format!("exhausted: {}; no reset time known, next probe in {}", e.reason, fmt_secs(p - now)),
        (State::Exhausted, None, None) => format!("exhausted: {}", e.reason),
    }
}

impl Pool {
    /// A pool mirrored to `path` (when given), restoring what an earlier run saved.
    pub fn open(path: Option<PathBuf>, tuning: PoolTuning, clock: Arc<dyn Fn() -> i64 + Send + Sync>) -> Pool {
        let entries = path.as_deref().map(load).unwrap_or_default();
        Pool { inner: Mutex::new(entries.into_iter().map(|e| ((e.endpoint_id.clone(), e.model_id.clone()), e)).collect()), path, tuning, clock }
    }

    pub fn in_memory(tuning: PoolTuning, clock: Arc<dyn Fn() -> i64 + Send + Sync>) -> Pool {
        Pool::open(None, tuning, clock)
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    fn save(&self, map: &BTreeMap<Key, Entry>) {
        let Some(path) = &self.path else { return };
        let body = serde_json::to_string_pretty(&File { entries: map.values().cloned().collect() }).expect("entries serialize");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Write beside the file and rename, so a crash never leaves half a file. A failure only loses the cache.
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, body).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Apply expiries that have passed: a cooldown that ended, or an exhaustion whose reset time passed.
    fn settle(&self, map: &mut BTreeMap<Key, Entry>) -> bool {
        let now = self.now();
        let mut changed = false;
        for e in map.values_mut() {
            match (e.state, e.until) {
                (State::CoolingDown, Some(t)) if t <= now => {
                    e.state = State::Available;
                    e.since = now;
                    e.until = None;
                    changed = true;
                }
                (State::Exhausted, Some(t)) if t <= now => {
                    *e = Entry::fresh(&e.endpoint_id.clone(), &e.model_id.clone(), now);
                    changed = true;
                }
                _ => {}
            }
        }
        changed
    }

    fn with<R>(&self, f: impl FnOnce(&mut BTreeMap<Key, Entry>, i64) -> (R, bool)) -> R {
        let mut map = self.inner.lock().unwrap();
        let settled = self.settle(&mut map);
        let now = self.now();
        let (out, changed) = f(&mut map, now);
        if settled || changed {
            self.save(&map);
        }
        out
    }

    pub fn state(&self, endpoint_id: &str, model_id: &str) -> State {
        self.with(|m, _| (m.get(&(endpoint_id.into(), model_id.into())).map(|e| e.state).unwrap_or(State::Available), false))
    }

    pub fn entry(&self, endpoint_id: &str, model_id: &str) -> Option<Entry> {
        self.with(|m, _| (m.get(&(endpoint_id.into(), model_id.into())).cloned(), false))
    }

    /// A call succeeded. Returns true if the pair was exhausted before (it has recovered).
    pub fn record_success(&self, endpoint_id: &str, model_id: &str) -> bool {
        self.with(|m, now| {
            let key = (endpoint_id.to_string(), model_id.to_string());
            match m.get(&key) {
                None => (false, false),
                Some(e) if e.state == State::Available && e.throttles.is_empty() => (false, false),
                Some(e) => {
                    let recovered = e.state == State::Exhausted;
                    m.insert(key, Entry::fresh(endpoint_id, model_id, now));
                    (recovered, true)
                }
            }
        })
    }

    /// A call was throttled. Returns `Some` when this tips the pair into exhaustion (rule 3).
    pub fn record_throttle(&self, endpoint_id: &str, model_id: &str, retry_after: Option<Duration>) -> Option<Exhaustion> {
        let t = self.tuning.clone();
        self.with(|m, now| {
            let e = m.entry((endpoint_id.to_string(), model_id.to_string())).or_insert_with(|| Entry::fresh(endpoint_id, model_id, now));
            if e.state == State::Exhausted {
                return (None, false); // already known to be out; a throttle adds nothing
            }
            e.throttles.push(now);
            if e.throttles.len() > KEEP_THROTTLES {
                e.throttles.remove(0);
            }
            let n = t.throttle_attempts as usize;
            // A run of at least `n` consecutive throttles that has lasted `throttle_window_secs`.
            if e.throttles.len() >= n && now - e.throttles[0] >= t.throttle_window_secs {
                let reason = format!("throttled {} times in a row over {} minutes", e.throttles.len(), (now - e.throttles[0]) / 60);
                e.state = State::Exhausted;
                e.since = now;
                e.until = None;
                e.reason = reason.clone();
                e.throttles.clear();
                e.probe_interval_secs = t.reprobe_first_secs;
                e.next_probe_at = Some(now + t.reprobe_first_secs);
                return (Some(Exhaustion { reason, reset_at: None, raw: String::new() }), true);
            }
            let cooldown = retry_after.map(|d| d.as_secs() as i64).unwrap_or_else(|| (COOLDOWN_BASE_SECS << (e.throttles.len() - 1).min(8)).min(COOLDOWN_MAX_SECS));
            e.state = State::CoolingDown;
            e.since = now;
            e.until = Some(now + cooldown.max(1));
            e.reason = "rate limited".into();
            (None, true)
        })
    }

    /// A call (or the classifier) says the quota is used up.
    pub fn record_exhausted(&self, endpoint_id: &str, model_id: &str, x: &Exhaustion) {
        let t = self.tuning.clone();
        self.with(|m, now| {
            let e = m.entry((endpoint_id.to_string(), model_id.to_string())).or_insert_with(|| Entry::fresh(endpoint_id, model_id, now));
            let known = x.reset_at.filter(|r| *r > now);
            e.state = State::Exhausted;
            e.since = now;
            e.until = known;
            e.reason = x.reason.clone();
            e.throttles.clear();
            e.probe_interval_secs = t.reprobe_first_secs;
            // With a known reset time the pair simply comes back then; otherwise probe on a doubling schedule.
            e.next_probe_at = if known.is_some() { None } else { Some(now + t.reprobe_first_secs) };
            ((), true)
        })
    }

    /// A re-probe of an exhausted pair failed: wait twice as long, up to the cap.
    pub fn probe_failed(&self, endpoint_id: &str, model_id: &str) {
        let t = self.tuning.clone();
        self.with(|m, now| {
            let Some(e) = m.get_mut(&(endpoint_id.to_string(), model_id.to_string())) else { return ((), false) };
            if e.state != State::Exhausted {
                return ((), false);
            }
            e.probe_interval_secs = (e.probe_interval_secs.max(t.reprobe_first_secs) * 2).min(t.reprobe_max_secs);
            e.next_probe_at = Some(now + e.probe_interval_secs);
            ((), true)
        })
    }

    /// Exhausted pairs with no known reset time whose next probe is due.
    pub fn due_probes(&self) -> Vec<(String, String)> {
        self.with(|m, now| (m.values().filter(|e| e.state == State::Exhausted && e.until.is_none() && e.next_probe_at.is_some_and(|t| t <= now)).map(|e| (e.endpoint_id.clone(), e.model_id.clone())).collect(), false))
    }

    /// Every pair that has ever left `available`, in key order, with expiries applied.
    pub fn snapshot(&self) -> Vec<Entry> {
        self.with(|m, _| (m.values().cloned().collect(), false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    const T0: i64 = 1_790_000_000;

    fn pool_at(path: Option<PathBuf>) -> (Pool, Arc<AtomicI64>) {
        let now = Arc::new(AtomicI64::new(T0));
        let n = now.clone();
        (Pool::open(path, PoolTuning::default(), Arc::new(move || n.load(Ordering::SeqCst))), now)
    }

    fn pool() -> (Pool, Arc<AtomicI64>) {
        pool_at(None)
    }

    fn advance(now: &AtomicI64, secs: i64) {
        now.fetch_add(secs, Ordering::SeqCst);
    }

    fn quota(reset_at: Option<i64>) -> Exhaustion {
        Exhaustion { reason: "quota reached".into(), reset_at, raw: "Individual quota reached.".into() }
    }

    #[test]
    fn an_unknown_pair_is_available_and_a_success_changes_nothing() {
        let (p, _) = pool();
        assert_eq!(p.state("a", "m"), State::Available);
        assert!(!p.record_success("a", "m"));
        assert!(p.snapshot().is_empty(), "nothing is recorded for a pair that never failed");
    }

    #[test]
    fn a_throttle_cools_the_pair_down_for_the_providers_wait_or_a_doubling_backoff() {
        let (p, now) = pool();
        assert!(p.record_throttle("a", "m", Some(Duration::from_secs(30))).is_none());
        assert_eq!(p.state("a", "m"), State::CoolingDown);
        advance(&now, 29);
        assert_eq!(p.state("a", "m"), State::CoolingDown);
        advance(&now, 1);
        assert_eq!(p.state("a", "m"), State::Available, "the cooldown ended");
        // Without retry-after: 2 minutes, then 4.
        p.record_throttle("b", "m", None);
        assert_eq!(p.entry("b", "m").unwrap().until, Some(T0 + 30 + 120));
        advance(&now, 120);
        p.record_throttle("b", "m", None);
        assert_eq!(p.entry("b", "m").unwrap().until, Some(T0 + 30 + 120 + 240));
    }

    #[test]
    fn rule_3_three_throttles_in_a_row_over_ten_minutes_count_as_exhaustion() {
        let (p, now) = pool();
        assert!(p.record_throttle("a", "m", None).is_none());
        advance(&now, 240);
        assert!(p.record_throttle("a", "m", None).is_none());
        advance(&now, 240);
        assert!(p.record_throttle("a", "m", None).is_none(), "three throttles, but only 8 minutes apart");
        advance(&now, 240);
        let x = p.record_throttle("a", "m", None).expect("the fourth, 12 minutes after the second, tips it");
        assert!(x.reason.contains("4 times in a row over 12 minutes") && x.reset_at.is_none(), "{}", x.reason);
        let e = p.entry("a", "m").unwrap();
        assert_eq!((e.state, e.next_probe_at, e.probe_interval_secs), (State::Exhausted, Some(now.load(Ordering::SeqCst) + 900), 900));
    }

    #[test]
    fn three_quick_throttles_and_a_success_in_between_do_not_count() {
        let (p, now) = pool();
        for _ in 0..3 {
            assert!(p.record_throttle("a", "m", Some(Duration::from_secs(1))).is_none());
            advance(&now, 2);
        }
        assert_eq!(p.state("a", "m"), State::Available, "three throttles within seconds are a burst, not exhaustion");
        p.record_success("a", "m");
        advance(&now, 700);
        assert!(p.record_throttle("a", "m", None).is_none(), "the run restarted after the success");
        assert_eq!(p.entry("a", "m").unwrap().throttles.len(), 1);
    }

    #[test]
    fn an_exhaustion_with_a_known_reset_comes_back_then_and_is_never_probed() {
        let (p, now) = pool();
        p.record_exhausted("a", "m", &quota(Some(T0 + 3600)));
        assert_eq!(p.state("a", "m"), State::Exhausted);
        advance(&now, 3599);
        assert_eq!(p.state("a", "m"), State::Exhausted);
        assert!(p.due_probes().is_empty(), "a known reset needs no probe");
        advance(&now, 1);
        assert_eq!(p.state("a", "m"), State::Available);
        assert!(!p.record_success("a", "m"), "it was already settled back to available by its reset time");
    }

    #[test]
    fn an_exhaustion_with_no_reset_is_probed_at_15_minutes_doubling_to_4_hours() {
        let (p, now) = pool();
        p.record_exhausted("a", "m", &quota(None));
        assert!(p.due_probes().is_empty());
        advance(&now, 900);
        assert_eq!(p.due_probes(), vec![("a".to_string(), "m".to_string())]);
        let mut waits = vec![];
        for _ in 0..6 {
            p.probe_failed("a", "m");
            let e = p.entry("a", "m").unwrap();
            waits.push(e.probe_interval_secs);
            advance(&now, e.probe_interval_secs);
            assert_eq!(p.due_probes().len(), 1, "due again after the new wait");
        }
        assert_eq!(waits, vec![1800, 3600, 7200, 14_400, 14_400, 14_400]);
        assert!(p.record_success("a", "m"), "a successful probe recovers it");
        assert_eq!(p.state("a", "m"), State::Available);
        assert!(p.due_probes().is_empty());
    }

    #[test]
    fn a_throttle_never_downgrades_an_exhausted_pair() {
        let (p, _) = pool();
        p.record_exhausted("a", "m", &quota(None));
        assert!(p.record_throttle("a", "m", None).is_none());
        assert_eq!(p.state("a", "m"), State::Exhausted);
    }

    #[test]
    fn state_is_per_endpoint_and_model_not_per_model() {
        let (p, _) = pool();
        p.record_exhausted("direct", "sonnet", &quota(None));
        assert_eq!(p.state("direct", "sonnet"), State::Exhausted);
        assert_eq!(p.state("bedrock", "sonnet"), State::Available, "the same model on another endpoint has its own quota");
        assert_eq!(p.state("direct", "haiku"), State::Available);
    }

    #[test]
    fn view_drops_what_has_already_expired_and_describe_says_where_each_entry_stands() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pool-state.json");
        let (p, _) = pool_at(Some(path.clone()));
        let reset = T0 + 3 * 86_400 + 5 * 3600;
        p.record_exhausted("a", "m", &quota(Some(reset)));
        p.record_exhausted("b", "m", &quota(None));
        p.record_throttle("c", "m", Some(Duration::from_secs(90)));
        p.record_exhausted("d", "m", &quota(Some(T0 + 60)));
        let at = |secs: i64| view(&path, T0 + secs).iter().map(|e| format!("{}:{}", e.endpoint_id, e.state.as_str())).collect::<Vec<_>>();
        assert_eq!(at(0), vec!["a:exhausted", "b:exhausted", "c:cooling_down", "d:exhausted"]);
        assert_eq!(at(120), vec!["a:exhausted", "b:exhausted"], "the cooldown and the one-minute reset have passed; a read-only view writes nothing");
        let all = p.snapshot();
        let when = delphi_core::time::format_unix_millis(reset, 0).trim_end_matches(".000Z").replace('T', " ");
        assert_eq!(describe(&all[0], T0), format!("exhausted: quota reached; resets {} (in 3d 5h)", when));
        assert_eq!(describe(&all[1], T0), "exhausted: quota reached; no reset time known, next probe in 15m");
        assert_eq!(describe(&all[2], T0), "cooling down, back in 1m (rate limited)");
        assert_eq!((fmt_secs(45), fmt_secs(3 * 3600 + 7 * 60)), ("45s".to_string(), "3h 07m".to_string()));
    }

    #[test]
    fn state_survives_a_restart_and_a_corrupt_file_starts_clean() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local/pool-state.json");
        let (p, now) = pool_at(Some(path.clone()));
        p.record_exhausted("a", "m", &quota(Some(T0 + 3600)));
        p.record_throttle("b", "m", Some(Duration::from_secs(60)));
        drop(p);
        let (q, _) = pool_at(Some(path.clone()));
        assert_eq!((q.state("a", "m"), q.state("b", "m")), (State::Exhausted, State::CoolingDown));
        advance(&now, 0);
        std::fs::write(&path, "{ not json").unwrap();
        let (r, _) = pool_at(Some(path));
        assert_eq!(r.state("a", "m"), State::Available);
    }
}
