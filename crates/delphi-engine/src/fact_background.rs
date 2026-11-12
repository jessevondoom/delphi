//! Background validation of library claims (`specs/040` §18.3, §20.2; `specs/100` §4, §5): a low-priority worker that runs the validator in
//! **advisory** mode on queued claims while the daemon is idle, so a claim with the origins and trusted sources can read `vetted` without the
//! operator typing the claim's exact words into a manual run.
//!
//! What this module is: the *choice* of the next claim and the *accounting*, as plain functions of their inputs (a clock, the library, the
//! journal), so a test needs no network and no real time. The check itself is the validator's existing loop (`fact_serve`), called by
//! `Engine::run_background_claim` with `background` set by the daemon, never by a request a span wrote.
//!
//! What it never does: ratify (the worker runs at most `advisory`, whatever the mode is), write a status, change a claim's rank, take a
//! decision for the operator, or send a private claim's text outside. Whether a claim is chosen is never a reason to trust it: the choice only
//! decides which claim is checked next, and every check is the same check.
//!
//! Switch: `[validator] background` (default **off**, since spec 040 §18.3 says "switched on only with `advisory` mode first" and it spends
//! tokens). It also needs `[validator] mode` not `off`.

use crate::fact_audit::{self, Cap, Journal, BACKGROUND};
use crate::library::Library;
use delphi_adapter::config::ValidatorMode;
use serde_json::{json, Value};
use std::path::Path;

/// Spec 040 §18.3 item 5: a candidate is never retried more than twice.
pub const MAX_ATTEMPTS: usize = 2;
/// Spec 040 §18.3 item 5: the worker pauses while the audit backlog exceeds this many cards.
pub const BACKLOG_PAUSE: usize = 10;
/// **Hypothesis**, not in the spec: the most tokens the worker spends in one UTC day, on top of the spec's weekly 10% (§20.2). About eight
/// per-claim reserves, so a quiet install cannot be surprised; to be tuned from the status line's numbers.
pub const DAILY_TOKENS: u64 = 100_000;
/// **Hypothesis**: how long the operator must have been quiet before the worker starts a check.
pub const IDLE_SECS: i64 = 300;
/// **Hypothesis**: how often the daemon looks for work. One claim per look.
pub const TICK_SECS: i64 = 60;

const DAY: i64 = 86_400;

/// Why the worker is not running a check now. `Display` is the one line the status shows; it never carries claim text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wait {
    /// `[validator] background` is false.
    Disabled,
    /// `[validator] mode = off` (or the latch took it there).
    ModeOff,
    /// The vetted tier is rolled back, so a dry run would feed a tier that is closed.
    Latched,
    /// The audit backlog exceeds the pause threshold (§18.3 item 5).
    Backlog(usize),
    /// Today's cap is reached.
    Daily { spent: u64, cap: u64 },
    /// The weekly 10% share (§20.2) is reached.
    Weekly { spent: u64, cap: u64 },
    /// The operator used the console within the idle window.
    OperatorActive,
    /// A task or a foreground validator job is running.
    Busy,
    /// No claim is waiting, or each waiting one is skipped.
    Nothing,
}

impl std::fmt::Display for Wait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wait::Disabled => write!(f, "off ([validator] background = false)"),
            Wait::ModeOff => write!(f, "paused: [validator] mode is off"),
            Wait::Latched => write!(f, "paused: the vetted tier is rolled back"),
            Wait::Backlog(n) => write!(f, "paused: {n} audit card(s) wait for you (it resumes at {BACKLOG_PAUSE} or fewer)"),
            Wait::Daily { spent, cap } => write!(f, "stopped at today's cap ({spent} of {cap} tokens); it resumes tomorrow"),
            Wait::Weekly { spent, cap } => write!(f, "stopped at the weekly share ({spent} of {cap} tokens, 10% of the install's week); it resumes as the week rolls on"),
            Wait::OperatorActive => write!(f, "waiting: you are active"),
            Wait::Busy => write!(f, "waiting: a task or a validator job is running"),
            Wait::Nothing => write!(f, "idle: no claim is waiting"),
        }
    }
}

/// Everything `pick` reads. All of it is a value, so a test passes a fake clock and a temporary library.
pub struct Inputs<'a> {
    pub enabled: bool,
    /// The effective mode (`fact_mode::effective`), so a latch that dropped it is already in.
    pub mode: ValidatorMode,
    pub state: &'a Path,
    pub lib: &'a Library,
    pub now: i64,
    /// When the operator last used the console, unix seconds (0 = never).
    pub last_operator: i64,
    pub busy: bool,
    /// The install's tokens over the trailing seven days (the ledger), for the weekly share.
    pub install_week: u64,
    pub lane_on: bool,
}

/// The claim to check next: its id and its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub fact_id: String,
    pub claim_id: String,
    pub text: String,
}

fn day_start(now: i64) -> i64 {
    now - now.rem_euclid(DAY)
}

fn background_events(state: &Path) -> Vec<Value> {
    Journal::new(state).all()
}

/// Tokens the worker spent since `since`: the journal's verdicts for the `background` pseudo-mission.
fn spent_since(events: &[Value], since: i64) -> u64 {
    events.iter().filter(|e| e["kind"] == "verdict" && e["mission"] == BACKGROUND && e["at"].as_i64().unwrap_or(0) >= since).map(|e| e["tokens"].as_u64().unwrap_or(0)).sum()
}

fn attempts(events: &[Value], fact_id: &str) -> usize {
    events.iter().filter(|e| e["kind"] == "background_attempt" && e["fact_id"] == fact_id).count()
}

/// Record that a check was started, before it runs, so a crash mid-check still counts toward the retry limit.
pub fn record_attempt(state: &Path, fact_id: &str, now: i64) {
    Journal::new(state).append("background_attempt", json!({"fact_id": fact_id}), now);
}

/// May a check start? The gates come in an order that puts the cheapest and the most permanent first. Pure.
pub fn gate(i: &Inputs) -> Result<(), Wait> {
    if !i.enabled {
        return Err(Wait::Disabled);
    }
    if i.mode == ValidatorMode::Off {
        return Err(Wait::ModeOff);
    }
    if crate::tier_audit::latched(i.state).contains("vetted") {
        return Err(Wait::Latched);
    }
    if i.busy {
        return Err(Wait::Busy);
    }
    if i.last_operator > 0 && i.now - i.last_operator < IDLE_SECS {
        return Err(Wait::OperatorActive);
    }
    let (cards, _) = fact_audit::backlog(i.state, i.now);
    if cards > BACKLOG_PAUSE {
        return Err(Wait::Backlog(cards));
    }
    let events = background_events(i.state);
    let reserve = crate::fact_intake::RESERVE_PER_CLAIM;
    let today = spent_since(&events, day_start(i.now));
    if today + reserve > DAILY_TOKENS {
        return Err(Wait::Daily { spent: today, cap: DAILY_TOKENS });
    }
    if let Err(Cap::Background { spent, cap }) = fact_audit::background_allows(i.state, i.install_week, i.now, reserve) {
        return Err(Wait::Weekly { spent, cap });
    }
    Ok(())
}

/// The next claim to check, in the fast lane's order (`Library::validation_candidates`). A claim is skipped, never checked, when it is in a
/// protected class, rests on a private source (the check would send its words to a search engine), has been tried twice, or its sources changed
/// since its counts were taken (a fresh derivation moved it out of the waiting set). A contested, validated, disproved or retired claim is not
/// a candidate to begin with. Reads the library and may refresh a claim's derived counts, as the accept-all plan does; writes nothing else.
pub fn pick(i: &Inputs) -> Result<Pick, Wait> {
    gate(i)?;
    let events = background_events(i.state);
    let params = delphi_core::knowledge_status::tier_params_configured();
    for c in i.lib.validation_candidates(i.lane_on) {
        let Some(claim) = i.lib.claim(&c.id) else { continue };
        let text = claim["text"].as_str().unwrap_or("").to_string();
        if text.trim().is_empty() || crate::fact_check::protected_class(&text).is_some() || i.lib.rests_on_private(&claim) {
            continue;
        }
        let fact_id = crate::fact_store::fact_id(&text);
        if attempts(&events, &fact_id) >= MAX_ATTEMPTS {
            continue;
        }
        let fresh = i.lib.refresh_support(&c.id).unwrap_or(claim);
        if !matches!(crate::library::claims::status_with(&fresh, &params), delphi_core::knowledge_status::Status::Noted | delphi_core::knowledge_status::Status::Corroborated | delphi_core::knowledge_status::Status::Substantiated) {
            continue;
        }
        return Ok(Pick { fact_id, claim_id: c.id, text });
    }
    Err(Wait::Nothing)
}

/// What the status line and `delphi validation queue` show. Ids and numbers, never a claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub enabled: bool,
    pub tokens_today: u64,
    pub cap_today: u64,
    pub tokens_week: u64,
    pub claims_total: usize,
    pub claims_today: usize,
    pub last_run: Option<i64>,
    /// Why it is not running now, when it is not (`None` = it would run if a claim waits).
    pub waiting: Option<Wait>,
}

pub fn status(i: &Inputs) -> Status {
    let events = background_events(i.state);
    let verdicts: Vec<&Value> = events.iter().filter(|e| e["kind"] == "verdict" && e["mission"] == BACKGROUND).collect();
    let since = day_start(i.now);
    Status {
        enabled: i.enabled,
        tokens_today: spent_since(&events, since),
        cap_today: DAILY_TOKENS,
        tokens_week: spent_since(&events, i.now - fact_audit::WEEK_SECS),
        claims_total: verdicts.len(),
        claims_today: verdicts.iter().filter(|e| e["at"].as_i64().unwrap_or(0) >= since).count(),
        last_run: events.iter().filter(|e| e["kind"] == "background_attempt").filter_map(|e| e["at"].as_i64()).max(),
        waiting: pick_reason(i),
    }
}

/// The gate's refusal, or `Nothing` when no candidate passes, or `None` when a check would start.
fn pick_reason(i: &Inputs) -> Option<Wait> {
    pick(i).err()
}

impl Status {
    /// One or two lines for the operator. The cap line is always present, so "stopped at the cap" is said where it is read.
    pub fn lines(&self) -> String {
        let last = self.last_run.map(|t| delphi_core::time::format_unix_millis(t * 1000, 0).replace(".000Z", "Z")).unwrap_or_else(|| "never".into());
        let mut s = format!(
            "background validation: {} · {} of {} tokens today ({} this week) · {} claim(s) checked, {} today · last run {}\n",
            if self.enabled { "on" } else { "off" },
            self.tokens_today,
            self.cap_today,
            self.tokens_week,
            self.claims_total,
            self.claims_today,
            last
        );
        if let Some(w) = &self.waiting {
            s.push_str(&format!("  now: {w}\n"));
        }
        s
    }

    /// The one-sentence doctor fact. Present only when the worker is on and has a reason to say something the operator should know.
    pub fn doctor_fact(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        match &self.waiting {
            Some(w @ (Wait::Daily { .. } | Wait::Weekly { .. } | Wait::Backlog(_) | Wait::Latched | Wait::ModeOff)) => Some(format!("background validation {w}")),
            _ => None,
        }
    }
}
/// The status read from the files alone, for `delphi validation queue` and `delphi doctor` (no daemon needed). `cfg_text` is the workspace's
/// `config.toml`; `ws_dir` is `$DELPHI_HOME/workspaces/<name>` and the library is the install's. The weekly share needs the ledger's week, which
/// a command line reads from the same database the daemon does.
pub fn status_from_files(ws_dir: &Path, home: &Path, cfg_text: &str, now: i64) -> Status {
    let root: toml::Value = toml::from_str(cfg_text).unwrap_or_else(|_| toml::Value::Table(Default::default()));
    let v = root.get("validator");
    let enabled = v.and_then(|v| v.get("background")).and_then(|b| b.as_bool()).unwrap_or(false);
    let config_mode = v.and_then(|v| v.get("mode")).and_then(|m| m.as_str()).and_then(ValidatorMode::parse).unwrap_or_default();
    let state = crate::fact_mode::state_dir(ws_dir);
    let lib = Library::in_home(home);
    let install_week = {
        use delphi_core::ledger::Ledger;
        Ledger::open(&ws_dir.join("local/state.db")).ok().and_then(|l| l.tokens_since(&delphi_core::time::format_unix_millis(now - 7 * 86_400, 0)).ok()).unwrap_or(0)
    };
    let i = Inputs { enabled, mode: crate::fact_mode::effective(config_mode, &state).mode, state: &state, lib: &lib, now, last_operator: 0, busy: false, install_week, lane_on: delphi_core::knowledge_status::tier_params_configured().fast_lane_on };
    status(&i)
}

impl crate::engine::Engine {
    /// The worker's inputs, read now.
    pub(crate) fn background_inputs<'a>(&'a self, lib: &'a Library, state: &'a Path, last_operator: i64, tasks_running: usize) -> Inputs<'a> {
        Inputs {
            enabled: self.cfg.get().validator.background,
            mode: crate::fact_mode::effective(self.cfg.get().validator.mode, state).mode,
            state,
            lib,
            now: (self.clock_unix)(),
            last_operator,
            busy: tasks_running > 0 || self.validator.busy(),
            install_week: self.install_week_tokens(),
            lane_on: delphi_core::knowledge_status::tier_params_configured().fast_lane_on,
        }
    }

    /// One look for work: if every gate passes, check the next claim (one per look). Returns the claim's fact id when a check ran. Where the
    /// install has no library there is nothing to validate.
    pub async fn background_validate_tick(self: &std::sync::Arc<Self>, m: &std::sync::Arc<crate::mission::MissionState>, last_operator: i64, tasks_running: usize) -> Option<String> {
        if !self.ws.home.join("library").is_dir() {
            return None;
        }
        let lib = Library::in_home(&self.ws.home);
        let state = crate::fact_mode::state_dir(&self.ws.dir);
        let chosen = {
            let i = self.background_inputs(&lib, &state, last_operator, tasks_running);
            pick(&i).ok()?
        };
        record_attempt(&state, &chosen.fact_id, (self.clock_unix)());
        let _ = self.run_background_claim(m, &chosen.text).await;
        Some(chosen.fact_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000;

    struct Fx {
        _d: tempfile::TempDir,
        lib: Library,
        state: std::path::PathBuf,
    }

    fn fx() -> Fx {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::in_home(d.path());
        let state = d.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        Fx { _d: d, lib, state }
    }

    fn inputs<'a>(f: &'a Fx, now: i64) -> Inputs<'a> {
        Inputs { enabled: true, mode: ValidatorMode::Advisory, state: &f.state, lib: &f.lib, now, last_operator: 0, busy: false, install_week: 10_000_000, lane_on: true }
    }

    fn verdict(f: &Fx, fact: &str, tokens: u64, at: i64) {
        Journal::new(&f.state).append("verdict", json!({"fact_id": fact, "mission": BACKGROUND, "verdict": "would_ratify", "tokens": tokens}), at);
    }

    #[test]
    fn it_is_off_unless_switched_on_and_never_runs_with_the_validator_off() {
        let f = fx();
        let mut i = inputs(&f, T0);
        i.enabled = false;
        assert_eq!(gate(&i), Err(Wait::Disabled));
        let mut i = inputs(&f, T0);
        i.mode = ValidatorMode::Off;
        assert_eq!(gate(&i), Err(Wait::ModeOff));
        assert_eq!(gate(&inputs(&f, T0)), Ok(()));
    }

    #[test]
    fn it_waits_while_the_operator_is_active_or_something_runs_and_resumes_on_the_fake_clock() {
        let f = fx();
        let mut i = inputs(&f, T0);
        i.last_operator = T0 - 10;
        assert_eq!(gate(&i), Err(Wait::OperatorActive));
        i.now = T0 - 10 + IDLE_SECS;
        assert_eq!(gate(&i), Ok(()), "quiet for the idle window");
        i.busy = true;
        assert_eq!(gate(&i), Err(Wait::Busy));
    }

    #[test]
    fn it_stops_at_the_daily_cap_says_so_and_resumes_the_next_day() {
        let f = fx();
        verdict(&f, "fact-aaaaaaaaaaaa", DAILY_TOKENS - 5_000, T0);
        let w = gate(&inputs(&f, T0 + 60)).unwrap_err();
        assert_eq!(w, Wait::Daily { spent: DAILY_TOKENS - 5_000, cap: DAILY_TOKENS });
        let s = status(&inputs(&f, T0 + 60));
        assert!(s.lines().contains("stopped at today's cap"), "{}", s.lines());
        assert!(s.doctor_fact().unwrap().contains("stopped at today's cap"));
        assert_eq!(gate(&inputs(&f, day_start(T0) + DAY + 10)), Ok(()), "a new UTC day");
    }

    #[test]
    fn it_honours_the_weekly_share_of_the_install() {
        let f = fx();
        verdict(&f, "fact-aaaaaaaaaaaa", 1_000, T0);
        let mut i = inputs(&f, T0 + 60);
        i.install_week = 20_000;
        assert_eq!(gate(&i), Err(Wait::Weekly { spent: 1_000, cap: 2_000 }));
    }

    #[test]
    fn it_pauses_while_more_than_ten_audit_cards_wait() {
        let f = fx();
        for n in 0..11 {
            Journal::new(&f.state).append("card", json!({"card": format!("fact-{n:012}"), "fact_id": format!("fact-{n:012}"), "mission": "m", "batch": "b"}), T0 - 100);
        }
        assert_eq!(gate(&inputs(&f, T0)), Err(Wait::Backlog(11)));
    }

    #[test]
    fn a_claim_is_tried_at_most_twice() {
        let f = fx();
        record_attempt(&f.state, "fact-x", T0);
        assert_eq!(attempts(&background_events(&f.state), "fact-x"), 1);
        record_attempt(&f.state, "fact-x", T0 + 1);
        assert_eq!(attempts(&background_events(&f.state), "fact-x"), MAX_ATTEMPTS);
    }

    #[test]
    fn the_status_counts_runs_tokens_and_the_last_attempt_without_a_claim() {
        let f = fx();
        verdict(&f, "fact-aaaaaaaaaaaa", 700, T0 - 2 * DAY);
        verdict(&f, "fact-bbbbbbbbbbbb", 300, T0);
        record_attempt(&f.state, "fact-bbbbbbbbbbbb", T0 - 5);
        let s = status(&inputs(&f, T0 + 30));
        assert_eq!((s.tokens_today, s.tokens_week, s.claims_total, s.claims_today, s.last_run), (300, 1_000, 2, 1, Some(T0 - 5)));
        let text = s.lines();
        assert!(text.contains("background validation: on") && text.contains("300 of 100000 tokens today"), "{text}");
    }

    #[test]
    fn nothing_waiting_is_idle_not_an_error() {
        let f = fx();
        assert_eq!(pick(&inputs(&f, T0)), Err(Wait::Nothing));
    }
}
