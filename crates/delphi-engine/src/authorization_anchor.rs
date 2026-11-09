//! Operator-authorized tests, slice S2d1 (`specs/092` TA-3, the S2c finding): the ANCHOR of the real use ledger.
//!
//! The use ledger (`operator/uses/<id>`) is hash-chained but, alone, has no external anchor: a same-user process that cuts its tail,
//! deletes its directory or puts an older copy back restores spent uses and nothing notices. The smallest anchor that is sound:
//!
//! * **The launcher** (the operator's own process, which already writes the pinned audit chain for `delphi test`) appends an
//!   `authorization.run.launch` event to `operator/audit` recording the ledger's count and head, and an `authorization.run.end` event
//!   (count and head) when the run ends. Both go through the same writer lock and the same pinned-head update as `authorize` and
//!   `revoke`, so the store's recorded audit head vouches for them: cutting them off the audit log is the loud refusal that already
//!   exists for any cut.
//! * **The next launch** compares the ledger with the last of those events and REFUSES, naming the reason, if the ledger is shorter,
//!   its head differs, or (after a clean end) it is anything but exactly what the end event recorded.
//! * **The daemon** keeps the count and head in memory (the launcher's record at boot, then after each use it took) and takes every
//!   use under the ledger's writer lock only if the ledger is exactly there (`take_use_anchored`).
//!
//! **What this does NOT cover.** (1) A same-user attacker who rewrites the store, its pin, the audit log and the ledger consistently
//! (the exposure TA-3 already states). (2) Cutting the ledger between the daemon's check and its append: the window is the length of
//! one locked read-and-append, and an attacker holds no lock (the lock is advisory). (3) A run that crashed (no `run.end`): the next
//! launch can only require the ledger to be at least as long as at that run's launch and to contain its recorded head, so a tail of
//! uses taken in the crashed run and then cut off is not seen; the launch is allowed, says so, and records `after_crash: true`, and the
//! operator should treat the record's remaining uses as an upper bound. (4) A second live run on the same record: its uses move the ledger
//! under the first run's daemon, whose next use then refuses (safe), so one live run per record is the working rule.

use crate::authorization as a;
use crate::authorization_grant::{ledger_dir, ledger_state, LedgerState};
use serde_json::{json, Value};
use std::path::Path;

pub const LAUNCH: &str = "authorization.run.launch";
pub const RUN_END: &str = "authorization.run.end";

fn state_of(e: &delphi_core::event::EventEnvelope) -> LedgerState {
    let p = &e.payload;
    let head = match (p["ledger_seq"].as_u64(), p["ledger_h"].as_str()) {
        (Some(s), Some(h)) => Some((s, h.to_string())),
        _ => None,
    };
    LedgerState { count: p["ledger_count"].as_u64().unwrap_or(0) as u32, head }
}

/// The launch check: is the real ledger of `id` where the pinned audit chain says it must be? `Ok(false)` is a clean start;
/// `Ok(true)` means the last run of this record never recorded its end (a crash), so the weaker rule (3) above applied.
pub fn check_against_audit(home: &Path, id: &str, now: &LedgerState) -> Result<bool, String> {
    let events = a::read_audit(home)?;
    let last = events.iter().rev().find(|e| (e.kind == LAUNCH || e.kind == RUN_END) && e.payload["id"].as_str() == Some(id));
    let Some(last) = last else {
        if *now == LedgerState::default() {
            return Ok(false);
        }
        return Err(format!("the use ledger of {id} holds {} use(s) but the audit log records no run of it: the ledger was written outside a launched run, or the audit events were removed", now.count));
    };
    let want = state_of(last);
    if last.kind == RUN_END {
        if *now != want {
            return Err(format!(
                "the use ledger of {id} is not where the last run left it (that run ended with {} use(s) at event {}, the ledger now has {} at event {}): it was cut, deleted, replaced by an older copy or written since; no run starts until the operator has looked",
                want.count,
                want.head.as_ref().map(|h| h.0.to_string()).unwrap_or_else(|| "none".into()),
                now.count,
                now.head.as_ref().map(|h| h.0.to_string()).unwrap_or_else(|| "none".into())
            ));
        }
        return Ok(false);
    }
    // The last run of this record has a launch and no end: a crash, or a run still alive. The ledger may only have grown, and must still
    // contain the head that launch recorded.
    if now.count < want.count {
        return Err(format!("the use ledger of {id} holds {} use(s) but a run launched with {}: the ledger was cut or deleted; no run starts until the operator has looked", now.count, want.count));
    }
    if let Some((seq, h)) = &want.head {
        let events = delphi_core::log::replay(&ledger_dir(home, id)).map_err(|e| format!("the use ledger of {id} cannot be read: {e}"))?;
        if !events.iter().any(|e| e.seq == *seq && e.h.as_deref() == Some(h.as_str())) {
            return Err(format!("the use ledger of {id} does not contain the event {seq} that a run launched with: it was replaced by another copy; no run starts until the operator has looked"));
        }
    }
    Ok(true)
}

fn payload(id: &str, run: &str, s: &LedgerState, extra: Value) -> Value {
    let mut v = json!({"id": id, "run": run, "ledger_count": s.count, "ledger_seq": s.head.as_ref().map(|h| h.0), "ledger_h": s.head.as_ref().map(|h| h.1.clone()), "source": "launcher", "pid": std::process::id()});
    if let (Some(o), Some(x)) = (v.as_object_mut(), extra.as_object()) {
        for (k, val) in x {
            o.insert(k.clone(), val.clone());
        }
    }
    v
}

/// Append one anchor event to the operator's audit log and pin the new head in the store, as `refuse_audited` does: the writer lock,
/// the event, then the store's recorded head, so a later cut is caught by the checks that already exist.
fn append_pinned(home: &Path, kind: &str, body: Value) -> Result<(), String> {
    let mut audit = a::open_audit(home)?;
    let head = a::append_audit(home, &mut audit, kind, body)?;
    a::mutate(home, |s| {
        s.audit_head = Some(head);
        Ok(())
    })
}

/// Record that a run is about to start with the ledger at `state`.
pub fn record_launch(home: &Path, id: &str, run: &str, state: &LedgerState, after_crash: bool) -> Result<(), String> {
    append_pinned(home, LAUNCH, payload(id, run, state, json!({"after_crash": after_crash})))
}

/// Record that the run ended, with the ledger as it now stands. If the ledger cannot be read the event is NOT written (the next launch
/// then treats this run as a crash, which is the safe reading).
pub fn record_run_end(home: &Path, id: &str, run: &str, outcome: &str) -> Result<LedgerState, String> {
    let state = ledger_state(home, id)?;
    append_pinned(home, RUN_END, payload(id, run, &state, json!({"outcome": outcome})))?;
    Ok(state)
}

/// What the launcher holds for the length of a granted run. Dropping it records the run's end (best effort, reported on stderr), so
/// every exit path of the launcher, a failed spawn included, leaves the anchor closed.
pub struct RunAnchor {
    home: std::path::PathBuf,
    id: String,
    run: String,
    outcome: String,
    done: bool,
}

impl RunAnchor {
    pub fn set_outcome(&mut self, outcome: &str) {
        self.outcome = outcome.to_string();
    }

    /// End the run now and return the ledger state recorded.
    pub fn end(mut self) -> Result<LedgerState, String> {
        self.done = true;
        record_run_end(&self.home, &self.id, &self.run, &self.outcome)
    }
}

impl Drop for RunAnchor {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = record_run_end(&self.home, &self.id, &self.run, &self.outcome) {
                eprintln!("delphi run: could not record the end of the authorized run ({e}); the next launch treats this run as one that crashed");
            }
        }
    }
}

/// The launcher's whole start: the strict launch check, the ledger anchor check, the `launch` event, and the payload for the daemon's
/// pipe, which carries the ledger state recorded. Any failure refuses the run; a failure after the `launch` event closes it again.
pub fn launch_granted(home: &Path, id: &str, run: &str, now_unix: i64, policy: &a::Policy) -> Result<(Vec<u8>, RunAnchor), String> {
    launch_granted_for(home, id, run, now_unix, policy, None)
}

/// As [`launch_granted`], for a run whose snapshot is `(directory, tree hash)`: the record's tree pin and every pinned `@snapshot/<rel>`
/// file are checked against it BEFORE anything is written (no launch event, no use), and the snapshot goes to the daemon in the payload.
pub fn launch_granted_for(home: &Path, id: &str, run: &str, now_unix: i64, policy: &a::Policy, snapshot: Option<(&Path, &str)>) -> Result<(Vec<u8>, RunAnchor), String> {
    let (record, seq) = a::authorize_launch_with_head(home, id, run, now_unix, policy)?;
    match snapshot {
        Some((dir, tree)) => a::check_snapshot_pins(&record, dir, tree)?,
        None if record.manifest.iter().any(|e| e.files.iter().any(|f| crate::authorization_tokens::is_snapshot_file(&f.path))) => {
            return Err(format!("authorization {} pins files in the run's snapshot and the launcher gave none", record.id));
        }
        None => {}
    }
    let state = ledger_state(home, &record.id)?;
    let after_crash = check_against_audit(home, &record.id, &state)?;
    record_launch(home, &record.id, run, &state, after_crash)?;
    let anchor = RunAnchor { home: home.to_path_buf(), id: record.id.clone(), run: run.to_string(), outcome: "ended".into(), done: false };
    // The head moved when the launch event was pinned; the grant remembers the new one.
    let seq = a::load(home)?.audit_head.map(|h| h.seq).unwrap_or(seq);
    let bytes = crate::authorization_grant::encode_boot_with(&record, run, home, seq, &state, snapshot)?;
    if after_crash {
        eprintln!("delphi run: the last run of {} never recorded its end (a crash?); it is allowed, and the record's remaining uses should be read as an upper bound", record.id);
    }
    Ok((bytes, anchor))
}


#[cfg(test)]
mod tests;
