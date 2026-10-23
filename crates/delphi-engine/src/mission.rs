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
        }
    }

    /// Mask secrets in free text (error messages, provider bodies).
    pub fn redact(&self, text: &str) -> String {
        self.redactor.lock().unwrap().redact_str(text).0
    }

    fn snapshot(&self) -> Vec<String> {
        self.literals.lock().unwrap().clone()
    }
}

pub struct MissionState {
    pub id: String,
    pub dir: PathBuf,
    log: Arc<Mutex<EventLog>>,
    pub budget: Mutex<Budget>,
    pub index: Mutex<Index>,
    hub: Hub,
    ledger: Arc<Mutex<Ledger>>,
    secrets: Arc<SecretRegistry>,
    registered: AtomicU64,
    /// Serializes reserve decisions so parallel spans can't jointly overshoot the cap.
    pub reserve_gate: tokio::sync::Mutex<()>,
    next_task: AtomicU64,
    next_span: AtomicU64,
    next_res: AtomicU64,
}

/// Highest trailing number among `prefix`-ed ids seen in `events` (so ids never repeat after a restart).
fn max_suffix(events: &[EventEnvelope], pick: impl Fn(&EventEnvelope) -> Option<String>, prefix: &str) -> u64 {
    events.iter().filter_map(&pick).filter_map(|id| id.strip_prefix(prefix).and_then(|n| n.rsplit('_').next().map(String::from)).and_then(|n| n.parse::<u64>().ok())).max().unwrap_or(0)
}

impl MissionState {
    /// Open (creating if new) the mission under `dir`, replaying its log into the derived state.
    pub fn open(dir: &Path, id: &str, hub: Hub, ledger: Arc<Mutex<Ledger>>, secrets: Arc<SecretRegistry>, budget_cap_usd: f64) -> Result<Arc<MissionState>, EngineError> {
        std::fs::create_dir_all(dir).map_err(|e| EngineError::Invalid(format!("{}: {}", dir.display(), e)))?;
        let events = replay(dir)?;
        let log = EventLog::open(dir, LogOptions::default())?;
        let mut budget = Budget::usd(budget_cap_usd);
        let mut index = Index::new();
        for e in &events {
            budget.apply_event(e, &|_| None);
            index.apply_event(e);
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
            hub,
            ledger,
            secrets,
            registered: AtomicU64::new(0),
            reserve_gate: tokio::sync::Mutex::new(()),
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
    pub async fn append(&self, e: EventEnvelope) -> Result<EventEnvelope, EngineError> {
        Ok(self.append_batch(vec![e]).await?.remove(0))
    }

    /// Persist several events together (one fsync), then apply and publish them in order.
    pub async fn append_batch(&self, events: Vec<EventEnvelope>) -> Result<Vec<EventEnvelope>, EngineError> {
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
        for e in &stored {
            self.budget.lock().unwrap().apply_event(e, &|_| None);
            self.index.lock().unwrap().apply_event(e);
            self.ledger.lock().unwrap().apply(e)?;
            self.hub.publish_event(&serde_json::to_value(e).expect("events serialize"));
        }
        Ok(stored)
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
        (MissionState::open(dir, "m1", Hub::new(), ledger.clone(), secrets.clone(), 5.0).unwrap(), ledger, secrets)
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
