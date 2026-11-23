//! How the merge commands use `refresh` (spec 094 §11.3.3): a pull request that is behind its base and otherwise ready is
//! refreshed inside `delphi merge <PR>` and `--all`, which then wait for the new head's checks and merge it only when they all
//! passed and it is still current; `--auto` refreshes it and moves on, and the session's next poll merges it.
//!
//! - At most `MAX_ROUNDS` refreshes per pull request in one invocation: a base that keeps moving stops it, with a report.
//! - A behind pull request that is red, or whose checks are still running, is not refreshed (that wastes a CI run on a head that
//!   will be replaced); it is refused as before.
//! - The approval carries over to the new head only because this invocation (or session) made the refresh itself (OD-8): the
//!   heads it made are kept in `made`, and the record of a merge of one of them says `carried_from` the old head. A head that
//!   changed any other way (somebody pushed while it waited) is refused, and needs the operator again.
//!
//! Nothing here merges on its own: the merge still goes through `finish`, with the attestation the invocation already holds.

use super::check::{evaluate, Verdict, BEHIND_MARK, QUEUE_LABEL};
use super::errors::Errors;
use super::forge::Forge;
use super::record::{Attestation, Ledger};
use super::refresh::{self, Done, Stop};
use super::{auto::Clock, finish, short, Ctx, Outcome};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Refreshes per pull request in one invocation (OD-9).
pub const MAX_ROUNDS: u32 = 2;
/// How long `<PR>` and `--all` wait for the new head's checks, unless `--wait` says otherwise (OD-9).
pub const DEFAULT_WAIT: Duration = Duration::from_secs(40 * 60);

/// One refresh of one pull request. The real one is `refresh::refresh` in this repository; tests use a fake.
pub trait Refresher {
    fn refresh(&self, n: u64) -> Result<Done, Stop>;
}

/// `refresh::refresh`, run in the repository the command was started in.
pub struct InRepo<'a> {
    pub forge: &'a dyn Forge,
    pub errors: &'a Errors,
    pub ledger: &'a Ledger,
    pub home: &'a Path,
    pub git: refresh::Runner,
}

impl Refresher for InRepo<'_> {
    fn refresh(&self, n: u64) -> Result<Done, Stop> {
        let env = refresh::Env { forge: self.forge, errors: self.errors, ledger: self.ledger, home: self.home, git: &self.git, now: delphi_core::time::now_rfc3339(), before_push: None };
        refresh::refresh(&env, n)
    }
}

/// What a merge command does with a pull request that is behind. Without it (`Ctx::refresh` is `None`) such a pull request is
/// refused, as before §11.3.3.
#[derive(Clone, Copy)]
pub struct Refreshing<'a> {
    pub refresher: &'a dyn Refresher,
    pub clock: &'a dyn Clock,
    /// How long to wait for the new head's checks; `None` refreshes and stops (`--no-wait`, and every `--auto` session).
    pub wait: Option<Duration>,
    /// How often to look while waiting.
    pub poll: Duration,
    /// The heads this invocation or session made by a refresh: new head to old head. Only these carry the approval over.
    pub made: &'a RefCell<HashMap<String, String>>,
}

/// While waiting, these refusals mean "not yet": CI has not started or finished on the new head.
pub(super) fn not_yet(reason: &str) -> bool {
    reason.starts_with("checks are still running") || reason.starts_with(super::check::NO_CHECKS_MARK) || reason.starts_with("GitHub has not finished")
}

/// Every reason `v` cannot merge now, the missing queue label included (a gate-changing pull request is never queued).
pub fn reasons(v: &Verdict) -> Vec<String> {
    let mut out = v.refusals.clone();
    if !v.gate_changing() && !v.pr.labels.iter().any(|l| l == QUEUE_LABEL) {
        let n = v.pr.number;
        out.insert(0, format!("it is not queued (no `{QUEUE_LABEL}` label; an agent queues it with `delphi merge queue {n}`)"));
    }
    out
}

/// Behind, and nothing else stands in the way: the only kind of pull request a merge command refreshes.
pub fn refreshable(v: &Verdict) -> bool {
    v.behind_by > 0 && !v.gate_changing() && reasons(v).iter().all(|r| r.contains(BEHIND_MARK))
}

enum Waited {
    Ready(Verdict),
    Behind(Verdict),
    Refused(Verdict, Vec<String>),
    TimedOut,
    Moved(String),
    Error(String),
}

/// Wait for the checks on `head`, at most `limit`.
fn wait_for(ctx: &Ctx, r: &Refreshing, n: u64, head: &str, limit: Duration) -> Waited {
    let start = r.clock.now_secs();
    loop {
        let v = match evaluate(ctx.forge, n) {
            Ok(v) => v,
            Err(e) => return Waited::Error(e.to_string()),
        };
        if v.pr.head_sha != head {
            return Waited::Moved(v.pr.head_sha);
        }
        let other: Vec<String> = reasons(&v).into_iter().filter(|r| !r.contains(BEHIND_MARK)).collect();
        if other.is_empty() {
            return if v.behind_by > 0 { Waited::Behind(v) } else { Waited::Ready(v) };
        }
        if !other.iter().all(|r| not_yet(r)) {
            return Waited::Refused(v, other);
        }
        if r.clock.now_secs().saturating_sub(start) >= limit.as_secs() {
            return Waited::TimedOut;
        }
        r.clock.sleep(r.poll);
    }
}

/// Refresh `first` (behind and otherwise ready), wait for CI on the new head, and merge it with `att` once it is green and
/// current; another round if the base moved meanwhile, at most `MAX_ROUNDS` in all.
pub(super) fn refresh_and_merge(ctx: &mut Ctx, r: Refreshing, first: Verdict, att: Attestation) -> Outcome {
    let n = first.pr.number;
    let mut v = first;
    let mut rounds = 0;
    loop {
        if rounds == MAX_ROUNDS {
            let why = format!("the base kept moving: refreshed {MAX_ROUNDS} times in this run and it is {} {BEHIND_MARK} `{}` again; run it again when the base is quieter", v.behind_by, v.pr.base_ref);
            return Outcome::Refused { pr: n, reasons: vec![why], head: v.pr.head_sha };
        }
        rounds += 1;
        let report = match r.refresher.refresh(n) {
            Ok(Done::Refreshed(rep)) => rep,
            // Current after all: something other than this run brought the base in, so its head needs the operator again.
            Ok(Done::Current) => return Outcome::Refused { pr: n, reasons: vec!["it is already current, by a commit this run did not make; its new head needs the operator's approval again".into()], head: v.pr.head_sha },
            Err(stop) => return Outcome::Refused { pr: n, reasons: vec![stop.line(n)], head: v.pr.head_sha },
        };
        r.made.borrow_mut().insert(report.new_head.clone(), report.old_head.clone());
        let Some(limit) = r.wait else {
            return Outcome::Refreshed { pr: n, head: report.new_head, waited: false };
        };
        match wait_for(ctx, &r, n, &report.new_head, limit) {
            Waited::Ready(ready) => {
                let rec = ready.record(att.clone(), ctx.session.clone(), (ctx.now)());
                return finish(ctx, &ready, rec);
            }
            Waited::Behind(behind) => v = behind,
            Waited::Refused(red, why) => return Outcome::Refused { pr: n, reasons: why, head: red.pr.head_sha },
            Waited::TimedOut => return Outcome::Refreshed { pr: n, head: report.new_head, waited: true },
            Waited::Moved(head) => {
                let why = format!("its head moved to `{}` while it waited, a commit this run did not make; it needs the operator's approval again", short(&head));
                return Outcome::Refused { pr: n, reasons: vec![why], head };
            }
            Waited::Error(e) => return Outcome::Failed { pr: n, detail: e, head: report.new_head },
        }
    }
}
