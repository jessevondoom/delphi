//! `delphi merge --auto`: one bounded session (spec 094 §3.5).
//!
//! A session lives exactly as long as the command. It is never a setting, it does not survive a restart, and a second invocation
//! is a second session. Its grant (the caps and how the operator approved) is written to the ledger first, and every merge it makes
//! carries the session id. It merges queued pull requests oldest first, serially, and each one is re-derived by `merge_one` just
//! before it merges (the base moved after the last). It **never merges a gate-changing pull request** (those are labelled for a
//! human and left), and it never changes its own caps.
//!
//! It stops on the first of: the cap on merged pull requests, the time cap, the `STOP` file under `$DELPHI_HOME/merge/`, Ctrl-C,
//! too many errors in a row, or a long enough stretch with nothing to do. Waiting (running checks, a pull request that is not yet
//! green) is not an error and is retried each poll; a pull request that errored is parked until its head commit changes. A pull
//! request that is behind and otherwise ready is refreshed and left (§11.3.3): its new head comes back through the queue once CI
//! is green; only a refresh that fails counts as an error.
//!
//! With `--batch[=N]` (§13, S22) it merges through rollups under the same approval and rules (`auto_batch`); a constituent
//! counts against `--max-prs`, and a `rollup-red` entry counts as an error.

use super::auto_batch::Batching;
use super::check::QUEUE_LABEL;
use super::errors::classify_refusal;
use super::record::Attestation;
use super::base::{check_and_note, BaseCheck, BaseState};
use super::{base_of, merge_one, Ctx, Outcome};
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

/// The limits of a session. Fixed when it starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    pub max_prs: u32,
    pub run_for: Duration,
    pub pause_after_errors: u32,
    pub idle: Duration,
    pub poll: Duration,
    /// `--batch[=N]` (spec 094 §13, S22): merge through rollups of up to N; `None` merges one at a time.
    pub batch: Option<usize>,
}

impl Default for Caps {
    /// The defaults of spec 094 OD-2: 10 pull requests, 4 hours, pause after 2 errors in a row, stop after 30 idle minutes.
    fn default() -> Self {
        Caps { max_prs: 10, run_for: Duration::from_secs(4 * 3600), pause_after_errors: 2, idle: Duration::from_secs(30 * 60), poll: Duration::from_secs(60), batch: None }
    }
}

impl Caps {
    /// A session cannot be made unbounded: every limit has a ceiling.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_prs == 0 || self.max_prs > 100 {
            return Err("--max-prs must be between 1 and 100".into());
        }
        if self.run_for < Duration::from_secs(60) || self.run_for > Duration::from_secs(24 * 3600) {
            return Err("--for must be between 1 minute and 24 hours".into());
        }
        if self.pause_after_errors == 0 || self.pause_after_errors > 10 {
            return Err("--pause-after-errors must be between 1 and 10".into());
        }
        if self.idle < Duration::from_secs(60) || self.idle > Duration::from_secs(24 * 3600) {
            return Err("--idle must be between 1 minute and 24 hours".into());
        }
        if self.poll < Duration::from_secs(5) || self.poll > Duration::from_secs(3600) {
            return Err("--poll must be between 5 seconds and 1 hour".into());
        }
        if self.batch.is_some_and(|n| !(2..=super::rollup::MAX_BATCH).contains(&n)) {
            return Err(format!("--batch takes 2 to {} pull requests", super::rollup::MAX_BATCH));
        }
        Ok(())
    }
}

/// `90s`, `45m`, `4h`; a bare number is seconds.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1),
        Some('m') => (&s[..s.len() - 1], 60),
        Some('h') => (&s[..s.len() - 1], 3600),
        Some(c) if c.is_ascii_digit() => (s, 1),
        _ => return Err(format!("`{s}` is not a duration (use 90s, 45m or 4h)")),
    };
    num.parse::<u64>().ok().filter(|n| *n > 0).map(|n| Duration::from_secs(n * mult)).ok_or_else(|| format!("`{s}` is not a duration (use 90s, 45m or 4h)"))
}

/// Time and waiting, so a test can drive a four-hour session in no time.
pub trait Clock {
    fn now_secs(&self) -> u64;
    fn sleep(&self, d: Duration);
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_secs(&self) -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub session: String,
    pub reason: String,
    pub merged: Vec<u64>,
    pub left_for_a_human: Vec<u64>,
    pub errors: u32,
    /// `--batch`: the rollups the session opened, and those still open when it ended.
    pub rollups: Vec<u64>,
    pub rollups_open: Vec<u64>,
}

impl Summary {
    pub fn lines(&self) -> Vec<String> {
        let list = |v: &[u64]| if v.is_empty() { "none".to_string() } else { v.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ") };
        let mut out = vec![
            format!("session {} ended: {}", self.session, self.reason),
            format!("  merged: {}", list(&self.merged)),
            format!("  left for a human (they change a gate): {}", list(&self.left_for_a_human)),
            format!("  errors: {} (see `delphi merge log --open`)", self.errors),
        ];
        if !self.rollups.is_empty() {
            out.push(format!("  rollups: opened {}; still open {} (`delphi merge <rollup>` merges one on the operator's approval of its pull requests)", list(&self.rollups), list(&self.rollups_open)));
        }
        out
    }
}

fn session_id(now: u64) -> String {
    format!("a{now}-{}", std::process::id())
}

/// Run a session with no check of the base (tests of the session itself). `stop` is true when the operator asked to stop (Ctrl-C
/// or the STOP file); `say` receives one line at a time.
#[cfg(test)]
pub fn run(ctx: &mut Ctx, caps: &Caps, att: &Attestation, clock: &dyn Clock, stop: &dyn Fn() -> bool, say: &mut dyn FnMut(&str)) -> Summary {
    run_checked(ctx, caps, att, clock, stop, say, None)
}

/// Check the base `n` lands on (§11.3.4). A drift is an error and ends the session; a check that could not run is said.
fn base_drifted(ctx: &Ctx, check: &dyn BaseCheck, n: u64, say: &mut dyn FnMut(&str)) -> bool {
    let (state, line) = check_and_note(ctx.errors, check, &base_of(ctx.forge, n), &(ctx.now)());
    if !matches!(state, BaseState::Current { .. }) {
        say(&line);
    }
    matches!(state, BaseState::Drift { .. })
}

/// `run`, with the check of the base at each poll (before anything merges) and after each merge (§11.3.4): a drift ends the session.
pub fn run_checked(ctx: &mut Ctx, caps: &Caps, att: &Attestation, clock: &dyn Clock, stop: &dyn Fn() -> bool, say: &mut dyn FnMut(&str), base: Option<&dyn BaseCheck>) -> Summary {
    let drift_reason = "the base drifted (its compiled spec views are stale): nothing more is merged until it is repaired; run `delphi merge repair-base`, merge its pull request, then start a new session";
    let start = clock.now_secs();
    let deadline = start + caps.run_for.as_secs();
    let id = session_id(start);
    ctx.session = Some(id.clone());
    let grant = json!({
        "kind": "session_start", "time": (ctx.now)(), "session": id,
        "max_prs": caps.max_prs, "run_for_secs": caps.run_for.as_secs(), "pause_after_errors": caps.pause_after_errors,
        "idle_secs": caps.idle.as_secs(), "poll_secs": caps.poll.as_secs(), "batch": caps.batch, "gate_changing": "never merged by a session",
        "attestation": att.label(), "self_attested": matches!(att, Attestation::ChatQuote(_)), "quote": att.quote(),
    });
    let _ = ctx.ledger.append(&grant);
    say(&format!("session {id}: up to {} pull request(s), for {} minute(s), pausing after {} error(s) in a row, idle limit {} minute(s){}. Stop it with Ctrl-C or by creating the STOP file.", caps.max_prs, caps.run_for.as_secs() / 60, caps.pause_after_errors, caps.idle.as_secs() / 60, caps.batch.map(|n| format!(", in rollups of up to {n}")).unwrap_or_default()));

    let (mut merged, mut human, mut errors, mut consecutive) = (Vec::<u64>::new(), Vec::<u64>::new(), 0u32, 0u32);
    let mut parked: HashMap<u64, String> = HashMap::new();
    let mut last_activity = start;
    let mut batching = caps.batch.map(|n| Batching::new(n, att));

    let reason: String = 'session: loop {
        if stop() {
            break 'session "stopped by the operator (Ctrl-C or the STOP file)".into();
        }
        if merged.len() as u32 >= caps.max_prs {
            break 'session format!("reached the cap of {} merged pull request(s)", caps.max_prs);
        }
        if clock.now_secs() >= deadline {
            break 'session "reached the time cap".into();
        }
        let queue = match ctx.forge.queued(QUEUE_LABEL) {
            Ok(q) => q,
            Err(e) => {
                say(&format!("could not read the queue: {e}"));
                errors += 1;
                consecutive += 1;
                if consecutive >= caps.pause_after_errors {
                    break 'session format!("paused after {consecutive} error(s) in a row (the last: {e})");
                }
                Vec::new()
            }
        };
        if let (Some(check), Some(first)) = (base, queue.first()) {
            if base_drifted(ctx, check, *first, say) {
                errors += 1;
                break 'session drift_reason.into();
            }
        }
        // `--batch` (S22): follow the rollups in flight and open a new one; the serial path below takes what it holds back.
        let (mut hold, mut carried) = (std::collections::BTreeSet::new(), 0);
        if let Some(b) = batching.as_mut() {
            let p = b.poll(ctx, &queue, &parked, caps.max_prs.saturating_sub(merged.len() as u32), say);
            if p.active {
                last_activity = clock.now_secs();
            }
            if !p.merged.is_empty() {
                consecutive = 0;
            }
            let landed = p.merged.first().copied();
            merged.extend(&p.merged);
            (errors, consecutive) = (errors + p.errors, consecutive + p.errors);
            parked.extend(p.parked);
            (hold, carried) = (p.hold, p.carried);
            if let (Some(check), Some(n)) = (base, landed) {
                if base_drifted(ctx, check, n, say) {
                    errors += 1;
                    break 'session drift_reason.into();
                }
            }
            if consecutive >= caps.pause_after_errors {
                break 'session format!("paused after {consecutive} error(s) in a row; fix them (`delphi merge log --open`) and start a new session");
            }
        }
        for n in queue {
            if stop() {
                break 'session "stopped by the operator (Ctrl-C or the STOP file)".into();
            }
            if merged.len() as u32 >= caps.max_prs {
                break 'session format!("reached the cap of {} merged pull request(s)", caps.max_prs);
            }
            if clock.now_secs() >= deadline {
                break 'session "reached the time cap".into();
            }
            // A constituent in flight counts against the cap before it merges; one that may join the next batch waits for it.
            if hold.contains(&n) {
                continue;
            }
            if (merged.len() + carried) as u32 >= caps.max_prs {
                break;
            }
            // A pull request that errored, or that needs a human, is left alone until its head commit changes.
            if let Some(h) = parked.get(&n) {
                if ctx.forge.pr(n).is_ok_and(|p| &p.head_sha == h) {
                    continue;
                }
            }
            let out = merge_one(ctx, n, Some(att));
            say(&out.line());
            match out {
                Outcome::Merged { .. } => {
                    merged.push(n);
                    consecutive = 0;
                    last_activity = clock.now_secs();
                    parked.remove(&n);
                    if base.is_some_and(|check| base_drifted(ctx, check, n, say)) {
                        errors += 1;
                        break 'session drift_reason.into();
                    }
                }
                Outcome::NeedsHuman { .. } => {
                    human.push(n);
                    last_activity = clock.now_secs();
                    if let Ok(p) = ctx.forge.pr(n) {
                        parked.insert(n, p.head_sha);
                    }
                }
                // "No checks yet" (§16.2) is waiting, as checks still running are, not an error that parks the head.
                Outcome::Refused { ref reasons, ref head, .. } if reasons.iter().any(|r| classify_refusal(r).is_some() && !super::wait::not_yet(r)) => {
                    parked.insert(n, head.clone());
                    errors += 1;
                    consecutive += 1;
                    last_activity = clock.now_secs();
                }
                Outcome::Failed { ref head, .. } => {
                    parked.insert(n, head.clone());
                    errors += 1;
                    consecutive += 1;
                    last_activity = clock.now_secs();
                }
                // Refreshed (§11.3.3): CI runs on the new head and the queue brings it back; not an error, and not parked.
                Outcome::Refreshed { .. } => last_activity = clock.now_secs(),
                // Waiting (checks running, not green yet): not an error, tried again at the next poll.
                Outcome::Refused { .. } | Outcome::Declined { .. } | Outcome::DryRun { .. } => {}
            }
            if consecutive >= caps.pause_after_errors {
                break 'session format!("paused after {consecutive} error(s) in a row; fix them (`delphi merge log --open`) and start a new session");
            }
        }
        if clock.now_secs().saturating_sub(last_activity) >= caps.idle.as_secs() {
            break 'session format!("nothing to do for {} minute(s)", caps.idle.as_secs() / 60);
        }
        // Wait for the next poll, in one-second slices so a stop is noticed at once.
        let mut waited = 0;
        while waited < caps.poll.as_secs() && !stop() {
            clock.sleep(Duration::from_secs(1));
            waited += 1;
        }
    };

    let (rollups, rollups_open) = batching.map(|b| (b.opened, b.inflight)).unwrap_or_default();
    let summary = Summary { session: id.clone(), reason, merged, left_for_a_human: human, errors, rollups, rollups_open };
    let _ = ctx.ledger.append(&json!({"kind": "session_end", "time": (ctx.now)(), "session": id, "reason": summary.reason, "merged": summary.merged, "left_for_a_human": summary.left_for_a_human, "errors": summary.errors, "rollups": summary.rollups, "rollups_open": summary.rollups_open}));
    summary
}
