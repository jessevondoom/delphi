//! A mission's durable state: its event log plus the in-memory views derived from it.
//!
//! Every state change is an event; the budget, the provenance index and the usage ledger are all
//! updated **from the persisted events** (never from the intent behind them), so a live run and a
//! replay after a crash arrive at the same state. Events are fsync'd before anything depends on them
//! and are redacted before they reach disk or a client.

use delphi_core::budget::Budget;
use delphi_core::event::EventEnvelope;
use delphi_core::hub::Hub;
use delphi_core::ledger::{Ledger, LedgerError};
use delphi_core::log::{replay, EventLog, LogError, LogOptions};
use delphi_core::provenance::Index;
use delphi_core::redact::Redactor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum EngineError {
    Load(crate::workspace::LoadError),
    Log(LogError),
    Ledger(LedgerError),
    Invalid(String),
    Join(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Load(e) => write!(f, "{}", e),
            EngineError::Log(e) => write!(f, "{}", e),
            EngineError::Ledger(e) => write!(f, "{}", e),
            EngineError::Invalid(m) | EngineError::Join(m) => write!(f, "{}", m),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<crate::workspace::LoadError> for EngineError {
    fn from(e: crate::workspace::LoadError) -> Self {
        EngineError::Load(e)
    }
}
impl From<LogError> for EngineError {
    fn from(e: LogError) -> Self {
        EngineError::Log(e)
    }
}
impl From<LedgerError> for EngineError {
    fn from(e: LedgerError) -> Self {
        EngineError::Ledger(e)
    }
}

/// Every resolved credential literal and secret header value, for redaction. Shared by all missions.
pub struct SecretRegistry {
    literals: Mutex<Vec<String>>,
    redactor: Mutex<Redactor>,
}

impl SecretRegistry {
    pub fn new() -> Arc<SecretRegistry> {
        Arc::new(SecretRegistry { literals: Mutex::new(Vec::new()), redactor: Mutex::new(Redactor::new()) })
    }

    pub fn register(&self, secret: &str) {
        if secret.is_empty() {
            return;
        }
        let mut lits = self.literals.lock().unwrap();
        if !lits.iter().any(|l| l == secret) {
            lits.push(secret.to_string());
            self.redactor.lock().unwrap().register_literal(secret);
            // The central error log masks the same runtime credentials (`specs/039`).
            delphi_core::elog::register_secret(secret);
        }
    }

    /// Mask secrets in free text (error messages, provider bodies).
    pub fn redact(&self, text: &str) -> String {
        self.redactor.lock().unwrap().redact_str(text).0
    }

    fn snapshot(&self) -> Vec<String> {
        self.literals.lock().unwrap().clone()
    }

    fn longest(&self) -> usize {
        self.literals.lock().unwrap().iter().map(|l| l.len()).max().unwrap_or(0)
    }
}

/// Redacts a live text stream chunk by chunk. A secret can arrive split across chunks, so the tail that
/// could still be the beginning of one is held back until more text (or `finish`) settles it.
pub struct LiveRedactor {
    secrets: Arc<SecretRegistry>,
    held: String,
}

impl LiveRedactor {
    /// Enough to hold back the start of a credential-shaped token even when no literal is registered.
    const MIN_HOLD: usize = 64;
    /// The longest block held back waiting for its END marker.
    const MAX_BLOCK: usize = 16 * 1024;

    pub fn new(secrets: Arc<SecretRegistry>) -> LiveRedactor {
        LiveRedactor { secrets, held: String::new() }
    }

    /// The part of `chunk` that is safe to show now.
    pub fn push(&mut self, chunk: &str) -> String {
        let all = self.secrets.redact(&format!("{}{}", self.held, chunk));
        let hold = self.secrets.longest().max(Self::MIN_HOLD) - 1;
        // A private-key block can run for kilobytes before its END line: hold everything from an open BEGIN.
        if let Some(begin) = all.rfind("-----BEGIN ") {
            if !all[begin..].contains("-----END ") && all.len() - begin <= Self::MAX_BLOCK {
                self.held = all[begin..].to_string();
                return all[..begin].to_string();
            }
        }
        let mut cut = all.len().saturating_sub(hold);
        while !all.is_char_boundary(cut) {
            cut -= 1;
        }
        self.held = all[cut..].to_string();
        all[..cut].to_string()
    }

    /// Whatever is still held, redacted; the stream has ended.
    pub fn finish(&mut self) -> String {
        let rest = self.secrets.redact(&self.held);
        self.held.clear();
        rest
    }
}

pub struct MissionState {
    pub id: String,
    pub dir: PathBuf,
    log: Arc<Mutex<EventLog>>,
    pub budget: Mutex<Budget>,
    pub index: Mutex<Index>,
    status: Mutex<crate::status::Status>,
    hub: Hub,
    ledger: Arc<Mutex<Ledger>>,
    secrets: Arc<SecretRegistry>,
    registered: AtomicU64,
    /// Serializes reserve decisions so parallel spans can't jointly overshoot the cap.
    pub reserve_gate: tokio::sync::Mutex<()>,
    /// Persist-then-apply is one step: concurrent appenders must not apply their events to the derived state out of `seq` order.
    append_gate: tokio::sync::Mutex<()>,
    next_task: AtomicU64,
    next_span: AtomicU64,
    next_res: AtomicU64,
    /// Content-bearing events of tasks whose output must stay in memory for now (Council Round 1).
    held: Mutex<std::collections::HashMap<String, Vec<EventEnvelope>>>,
    /// Staged-note capture state (`learn.rs`), rebuilt from the log on open.
    stager: Mutex<crate::learn::Stager>,
}

/// Highest trailing number among `prefix`-ed ids seen in `events` (so ids never repeat after a restart).
fn max_suffix(events: &[EventEnvelope], pick: impl Fn(&EventEnvelope) -> Option<String>, prefix: &str) -> u64 {
    events.iter().filter_map(&pick).filter_map(|id| id.strip_prefix(prefix).and_then(|n| n.rsplit('_').next().map(String::from)).and_then(|n| n.parse::<u64>().ok())).max().unwrap_or(0)
}

impl MissionState {
    pub fn secrets(&self) -> Arc<SecretRegistry> {
        self.secrets.clone()
    }

    /// Open (creating if new) the mission under `dir`, replaying its log into the derived state.
    pub fn open(dir: &Path, id: &str, hub: Hub, ledger: Arc<Mutex<Ledger>>, secrets: Arc<SecretRegistry>, budget_cap_usd: f64) -> Result<Arc<MissionState>, EngineError> {
        std::fs::create_dir_all(dir).map_err(|e| EngineError::Invalid(format!("{}: {}", dir.display(), e)))?;
        let events = replay(dir)?;
        let log = EventLog::open(dir, LogOptions::default())?;
        let mut budget = Budget::usd(budget_cap_usd);
        let mut index = Index::new();
        let mut status = crate::status::Status::new(id);
        for e in &events {
            budget.apply_event(e, &|_| None);
            index.apply_event(e);
            status.apply(e);
        }
        ledger.lock().unwrap().catch_up(dir)?;
        Ok(Arc::new(MissionState {
            id: id.to_string(),
            dir: dir.to_path_buf(),
            next_task: AtomicU64::new(max_suffix(&events, |e| e.task_id.clone(), "t_")),
            next_span: AtomicU64::new(max_suffix(&events, |e| e.span_id.clone(), "sp_")),
            next_res: AtomicU64::new(max_suffix(&events, |e| e.payload["reservation_id"].as_str().map(String::from), "r_")),
            log: Arc::new(Mutex::new(log)),
            budget: Mutex::new(budget),
            index: Mutex::new(index),
            status: Mutex::new(status),
            hub,
            ledger,
            secrets,
            registered: AtomicU64::new(0),
            reserve_gate: tokio::sync::Mutex::new(()),
            append_gate: tokio::sync::Mutex::new(()),
            held: Mutex::new(std::collections::HashMap::new()),
            stager: Mutex::new(crate::learn::Stager::replay(crate::learn::LearningSettings::default(), &events)),
        }))
    }

    pub fn next_task_id(&self) -> String {
        format!("t_{:04}", self.next_task.fetch_add(1, Ordering::SeqCst) + 1)
    }

    pub fn next_span_id(&self, role: &str) -> String {
        format!("sp_{}_{}", role.replace('-', "_"), self.next_span.fetch_add(1, Ordering::SeqCst) + 1)
    }

    pub fn next_reservation_id(&self) -> String {
        format!("r_{}", self.next_res.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Persist one event (fsync'd), apply it to the derived state, and publish it to attached clients.
    pub async fn append(self: &Arc<Self>, e: EventEnvelope) -> Result<EventEnvelope, EngineError> {
        let mut stored = self.append_batch(vec![e.clone()]).await?;
        Ok(if stored.is_empty() { e } else { stored.remove(0) }) // empty: held in memory
    }

    /// From now until [`MissionState::release_blocks`], what a task's agents say stays in daemon memory:
    /// its `agent.block.complete` and `agent.turn.complete` events are buffered, not written. Everything
    /// else (budget, usage, state) is written as usual.
    pub fn hold_blocks(&self, task_id: &str) {
        self.held.lock().unwrap().insert(task_id.to_string(), Vec::new());
    }

    /// Write what was held, in order, and stop holding. A no-op if nothing is held for the task.
    pub async fn release_blocks(self: &Arc<Self>, task_id: &str) -> Result<(), EngineError> {
        let buffered = self.held.lock().unwrap().remove(task_id);
        match buffered {
            Some(events) if !events.is_empty() => self.append_batch(events).await.map(|_| ()),
            _ => Ok(()),
        }
    }

    /// Persist several events together (one fsync), then apply and publish them in order.
    pub async fn append_batch(self: &Arc<Self>, events: Vec<EventEnvelope>) -> Result<Vec<EventEnvelope>, EngineError> {
        let events = {
            let mut held = self.held.lock().unwrap();
            if held.is_empty() {
                events
            } else {
                let mut rest = Vec::with_capacity(events.len());
                for e in events {
                    match e.task_id.as_ref().and_then(|t| held.get_mut(t)) {
                        Some(buf) if e.kind == "agent.block.complete" || e.kind == "agent.turn.complete" => buf.push(e),
                        _ => rest.push(e),
                    }
                }
                rest
            }
        };
        if events.is_empty() {
            return Ok(Vec::new());
        }
        // Writing and applying an event must never be cut in half: `task.cancel` aborts a task at any await point, and
        // an abort between the file write and the ledger update leaves a gap in the sequence. The commit therefore
        // runs as its own task; if the caller is dropped, it still finishes.
        let me = self.clone();
        tokio::spawn(async move { me.commit(events).await }).await.map_err(|e| EngineError::Join(e.to_string()))?
    }

    async fn commit(&self, events: Vec<EventEnvelope>) -> Result<Vec<EventEnvelope>, EngineError> {
        let _gate = self.append_gate.lock().await;
        let stored = self.write_and_apply(events).await?;
        // Capture (`specs/031` §3.2): deterministic and cheap, no model call. The notes are events of their own, written
        // under the same gate so their `seq` follows the events they cite; they are not observed again as triggers.
        let learned: Vec<EventEnvelope> = {
            let mut stager = self.stager.lock().unwrap();
            stored.iter().flat_map(|e| stager.observe(e)).map(|c| self.capture_event(c)).collect()
        };
        if !learned.is_empty() {
            self.write_and_apply(learned).await?;
        }
        Ok(stored)
    }

    fn capture_event(&self, c: crate::learn::Capture) -> EventEnvelope {
        match c {
            crate::learn::Capture::Note(n) => n.to_event(&self.id),
            crate::learn::Capture::Evict(id) => EventEnvelope::new(&self.id, "learn.dropped", serde_json::json!({"id": id, "reason": "evicted"})),
        }
    }

    /// Replace the capture settings (`[learning]`); the notes already in the log stay.
    pub fn set_learning(&self, settings: crate::learn::LearningSettings) {
        self.stager.lock().unwrap().set_settings(settings);
    }

    async fn write_and_apply(&self, events: Vec<EventEnvelope>) -> Result<Vec<EventEnvelope>, EngineError> {
        let log = self.log.clone();
        let known = self.registered.load(Ordering::SeqCst) as usize;
        let literals = self.secrets.snapshot();
        let new_literals: Vec<String> = literals.iter().skip(known).cloned().collect();
        self.registered.store(literals.len() as u64, Ordering::SeqCst);
        let stored = tokio::task::spawn_blocking(move || -> Result<Vec<EventEnvelope>, LogError> {
            let mut l = log.lock().unwrap();
            for lit in &new_literals {
                l.redactor_mut().register_literal(lit);
            }
            l.append_batch_stored(events)
        })
        .await
        .map_err(|e| EngineError::Join(e.to_string()))??;
        let mut rewrite = false;
        for e in &stored {
            self.budget.lock().unwrap().apply_event(e, &|_| None);
            self.index.lock().unwrap().apply_event(e);
            self.status.lock().unwrap().apply(e);
            rewrite |= crate::status::affects_status(&e.kind);
            self.ledger.lock().unwrap().apply(e)?;
            self.hub.publish_event(&serde_json::to_value(e).expect("events serialize"));
        }
        if rewrite {
            self.write_status();
        }
        Ok(stored)
    }

    /// Regenerate `status.md`. A projection: failure to write it never fails the mission.
    pub fn write_status(&self) {
        let (cap, spent, reserved) = {
            let b = self.budget.lock().unwrap();
            (b.cap_micro() as f64 / 1e6, b.spent() as f64 / 1e6, b.reserved() as f64 / 1e6)
        };
        let text = self.status.lock().unwrap().render(cap, spent, reserved);
        let _ = crate::status::write_projection(&self.dir.join("status.md"), &text);
    }

    /// The ids of all events, as persisted: used by recovery and tests.
    pub fn replay(&self) -> Result<Vec<EventEnvelope>, EngineError> {
        Ok(replay(&self.dir)?)
    }

    pub fn hub(&self) -> &Hub {
        &self.hub
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn open(dir: &Path) -> (Arc<MissionState>, Arc<Mutex<Ledger>>, Arc<SecretRegistry>) {
        let ledger = Arc::new(Mutex::new(Ledger::open_in_memory().unwrap()));
        let secrets = SecretRegistry::new();
        // A reopen right after a drop can find the lock still held: another test's fork/exec briefly inherits the descriptor
        // (the same race `delphi-core`'s log tests retry through). Wait it out instead of failing.
        let mut tries = 0;
        let state = loop {
            match MissionState::open(dir, "m1", Hub::new(), ledger.clone(), secrets.clone(), 5.0) {
                Err(EngineError::Log(delphi_core::log::LogError::Locked(_))) if tries < 300 => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                other => break other.unwrap(),
            }
        };
        (state, ledger, secrets)
    }

    fn denied(task: &str, reason: &str) -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", "approval.resolved", json!({"approval_id": "a1", "resolution": "denied", "reason": reason, "by": "console"}));
        e.task_id = Some(task.into());
        e
    }

    #[tokio::test]
    async fn a_denied_card_is_staged_next_to_it_and_survives_a_restart_once() {
        let t = tempfile::tempdir().unwrap();
        let (m, _, _) = open(t.path());
        let stored = m.append(denied("t_0001", "use the existing helper")).await.unwrap();
        let log = m.replay().unwrap();
        let notes: Vec<&EventEnvelope> = log.iter().filter(|e| e.kind == "learn.staged").collect();
        assert_eq!(notes.len(), 1, "{:?}", log.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>());
        assert_eq!(notes[0].seq, stored.seq + 1, "the note follows the event it cites");
        assert_eq!((notes[0].payload["id"].clone(), notes[0].payload["tier"].clone(), notes[0].payload["event_seqs"].clone()), (json!(format!("n{}", stored.seq)), json!("L5"), json!([stored.seq])));
        assert_eq!(notes[0].task_id.as_deref(), Some("t_0001"));
        drop(m);
        // Reopening replays the log: the state is rebuilt, nothing is staged twice.
        let (again, _, _) = open(t.path());
        again.append(EventEnvelope::new("m1", "task.note", json!({}))).await.unwrap();
        assert_eq!(again.replay().unwrap().iter().filter(|e| e.kind == "learn.staged").count(), 1);
    }

    #[tokio::test]
    async fn capture_off_stages_nothing_and_a_cap_drops_the_oldest() {
        let t = tempfile::tempdir().unwrap();
        let (m, _, _) = open(t.path());
        m.set_learning(crate::learn::LearningSettings { capture: false, ..Default::default() });
        m.append(denied("t", "x")).await.unwrap();
        assert!(m.replay().unwrap().iter().all(|e| !e.kind.starts_with("learn.")));
        m.set_learning(crate::learn::LearningSettings { max_notes: 10, ..Default::default() });
        for i in 0..11 {
            m.append(denied("t", &format!("reason {}", i))).await.unwrap();
        }
        let log = m.replay().unwrap();
        let dropped: Vec<&EventEnvelope> = log.iter().filter(|e| e.kind == "learn.dropped").collect();
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].payload["reason"], "evicted");
        let staged_first = log.iter().find(|e| e.kind == "learn.staged").unwrap();
        assert_eq!(dropped[0].payload["id"], staged_first.payload["id"], "the oldest note is the one dropped");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborting_tasks_at_any_point_never_leaves_a_gap_in_the_event_sequence() {
        let t = tempfile::tempdir().unwrap();
        let (m, _ledger, _) = open(t.path());
        let mut handles = Vec::new();
        for i in 0..60u64 {
            let m2 = m.clone();
            let h = tokio::spawn(async move {
                for k in 0..20 {
                    let _ = m2.append(EventEnvelope::new("m1", "task.note", json!({"i": i, "k": k}))).await;
                }
            });
            handles.push(h);
        }
        // Abort them at staggered moments, while the rest keep writing.
        for (n, h) in handles.iter().enumerate() {
            tokio::time::sleep(std::time::Duration::from_micros(150 * (n as u64 % 7))).await;
            if n % 2 == 0 {
                h.abort();
            }
        }
        for h in handles {
            let _ = h.await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await; // detached commits finish
        m.append(EventEnvelope::new("m1", "task.note", json!({"after": true}))).await.expect("the mission still accepts events after the aborts");
        let seqs: Vec<u64> = m.replay().unwrap().iter().map(|e| e.seq).collect();
        assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "contiguous sequence numbers");
        assert!(seqs.len() > 100);
    }

    #[tokio::test]
    async fn events_update_the_derived_views_and_survive_a_reopen() {
        let t = tempfile::tempdir().unwrap();
        let (m, ledger, _) = open(t.path());
        m.append(EventEnvelope::new("m1", "budget.reserve", json!({"reservation_id": "r_1", "amount": 400_000}))).await.unwrap();
        assert_eq!(m.budget.lock().unwrap().reserved(), 400_000);
        let mut u = EventEnvelope::new(
            "m1",
            "agent.usage",
            json!({"provider": "anthropic", "input_tokens": 10, "output_tokens": 5, "cost_usd": 0.25, "reserved_usd": 0.4, "usage_source": "reported", "cost_model": "metered", "outcome": "ok", "reservation_id": "r_1"}),
        );
        (u.agent, u.span_id, u.task_id, u.endpoint_id, u.access_path, u.model_tier, u.model_id, u.attempt) = (Some("architect".into()), Some("sp_architect_1".into()), Some("t_0007".into()), Some("direct".into()), Some("api".into()), Some("frontier".into()), Some("m".into()), Some(1));
        m.append(u).await.unwrap();
        let (spent, reserved) = {
            let b = m.budget.lock().unwrap();
            (b.spent(), b.reserved())
        };
        assert_eq!((spent, reserved), (250_000, 0));
        assert!((ledger.lock().unwrap().mission_cost_usd("m1").unwrap() - 0.25).abs() < 1e-9);
        drop(m);
        // Reopen: the budget and the ledger come back from the log alone, and ids do not repeat.
        let (m2, ledger2, _) = open(t.path());
        assert_eq!(m2.budget.lock().unwrap().spent(), 250_000);
        assert!((ledger2.lock().unwrap().mission_cost_usd("m1").unwrap() - 0.25).abs() < 1e-9);
        assert_eq!(m2.next_task_id(), "t_0008");
        assert_eq!(m2.next_span_id("architect"), "sp_architect_2");
        assert_eq!(m2.next_reservation_id(), "r_2");
    }

    #[tokio::test]
    async fn published_events_are_the_redacted_ones_and_secrets_registered_later_apply_immediately() {
        let t = tempfile::tempdir().unwrap();
        let (m, _, secrets) = open(t.path());
        let q = delphi_core::hub::ClientQueue::new(64);
        let id = m.hub().attach(delphi_core::proto::Scope::Console, q.clone());
        m.hub().subscribe(id);
        secrets.register("runtime-secret-token-0123456789");
        let stored = m.append(EventEnvelope::new("m1", "mission.note", json!({"text": "the token is runtime-secret-token-0123456789"}))).await.unwrap();
        assert!(!stored.payload["text"].as_str().unwrap().contains("runtime-secret"));
        let sent = q.pop().await.unwrap();
        assert!(!sent.params.to_string().contains("runtime-secret"), "{}", sent.params);
        let raw = std::fs::read_to_string(t.path().join("events/000001.jsonl")).unwrap();
        assert!(!raw.contains("runtime-secret"));
        assert_eq!(secrets.redact("again runtime-secret-token-0123456789"), "again [REDACTED]");
    }

    #[tokio::test]
    async fn a_batch_is_applied_in_order_and_a_second_writer_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let (m, _, _) = open(t.path());
        let s = m.append_batch(vec![EventEnvelope::new("m1", "mission.note", json!({"n": 1})), EventEnvelope::new("m1", "mission.note", json!({"n": 2}))]).await.unwrap();
        assert_eq!(s.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2]);
        let second = MissionState::open(t.path(), "m1", Hub::new(), Arc::new(Mutex::new(Ledger::open_in_memory().unwrap())), SecretRegistry::new(), 5.0);
        assert!(matches!(second, Err(EngineError::Log(LogError::Locked(_)))));
    }
}

#[cfg(test)]
mod live_redactor_tests {
    use super::*;

    #[test]
    fn a_private_key_block_is_held_until_it_ends_then_masked() {
        let mut r = LiveRedactor::new(SecretRegistry::new());
        let mut shown = r.push("Here: -----BEGIN RSA PRIVATE KEY-----\n");
        for _ in 0..40 {
            shown.push_str(&r.push("MIIEowIBAAKCAQEAxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n"));
        }
        shown.push_str(&r.push("-----END RSA PRIVATE KEY----- done."));
        shown.push_str(&r.finish());
        assert!(!shown.contains("MIIEow") && shown.starts_with("Here: ") && shown.ends_with("done."), "{}", shown);
    }

    #[test]
    fn a_secret_split_across_chunks_never_reaches_the_stream() {
        let secrets = SecretRegistry::new();
        secrets.register("runtime-secret-token-0123456789");
        let mut r = LiveRedactor::new(secrets);
        let mut shown = String::new();
        for chunk in ["The key is runtime-sec", "ret-token-01", "23456789 and then some more text to push it out of the window.", " Done."] {
            shown.push_str(&r.push(chunk));
        }
        shown.push_str(&r.finish());
        assert!(!shown.contains("runtime-sec") && !shown.contains("0123456789"), "{}", shown);
        assert!(shown.starts_with("The key is ") && shown.ends_with("Done."), "{}", shown);
    }
}
