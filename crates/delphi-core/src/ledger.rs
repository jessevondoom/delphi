//! The usage ledger (`specs/005` §2.4): a SQLite table derived from `agent.usage` events.
//!
//! The log is the truth; `state.db` is an index that can always be deleted and rebuilt. Each
//! mission has its own `seq` space, so the ledger's key is `(mission_id, seq)` and the highest
//! applied `seq` is tracked per mission (`applied`). Applying is idempotent and strictly in order.

use crate::event::EventEnvelope;
use crate::log::{replay, LogError};
use crate::usage::UsagePayload;
use rusqlite::{params, Connection};
use std::path::Path;

#[derive(Debug)]
pub enum LedgerError {
    Sql(rusqlite::Error),
    Log(LogError),
    /// An event arrived out of order for its mission.
    Gap { mission_id: String, expected: u64, found: u64 },
    Invalid(String),
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerError::Sql(e) => write!(f, "ledger database error: {}", e),
            LedgerError::Log(e) => write!(f, "{}", e),
            LedgerError::Gap { mission_id, expected, found } => write!(f, "ledger gap in mission {}: expected seq {}, found {}", mission_id, expected, found),
            LedgerError::Invalid(m) => write!(f, "invalid usage event: {}", m),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<rusqlite::Error> for LedgerError {
    fn from(e: rusqlite::Error) -> Self {
        LedgerError::Sql(e)
    }
}

impl From<LogError> for LedgerError {
    fn from(e: LogError) -> Self {
        LedgerError::Log(e)
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS usage_ledger (
    mission_id         TEXT NOT NULL,
    seq                INTEGER NOT NULL,       -- seq of the agent.usage event in that mission's log
    ts                 TEXT NOT NULL,
    task_id            TEXT NOT NULL,
    span_id            TEXT NOT NULL,
    role               TEXT NOT NULL,          -- canonical role ID, never a theme name
    mode               TEXT NOT NULL,          -- direct | dag | council
    provider           TEXT NOT NULL,
    model_tier         TEXT NOT NULL,
    model_id           TEXT NOT NULL,
    attempt            INTEGER NOT NULL,
    input_tokens       INTEGER NOT NULL,
    cache_read_tokens  INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens      INTEGER NOT NULL,
    thinking_tokens    INTEGER NOT NULL DEFAULT 0,
    cost_usd           REAL NOT NULL,
    reserved_usd       REAL NOT NULL,
    endpoint_id        TEXT NOT NULL,
    access_path        TEXT NOT NULL,
    usage_source       TEXT NOT NULL,
    cost_model         TEXT NOT NULL,
    outcome            TEXT NOT NULL,
    PRIMARY KEY (mission_id, seq)
);
CREATE INDEX IF NOT EXISTS usage_by_mission ON usage_ledger(mission_id, role, task_id);
CREATE TABLE IF NOT EXISTS chain_head (
    mission_id TEXT PRIMARY KEY,
    seq        INTEGER NOT NULL,
    h          TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS applied (
    mission_id TEXT PRIMARY KEY,
    last_seq   INTEGER NOT NULL
);";

pub struct Ledger {
    conn: Connection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    Mission,
    Role,
    Task,
    Model,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Totals {
    pub key: String,
    pub calls: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub thinking_tokens: u64,
    pub cost_usd: f64,
}

impl Ledger {
    pub fn open(path: &Path) -> Result<Ledger, LedgerError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Ledger::init(conn)
    }

    pub fn open_in_memory() -> Result<Ledger, LedgerError> {
        Ledger::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Ledger, LedgerError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Ledger { conn })
    }

    /// The highest `seq` applied for `mission_id` (0 if none).
    pub fn last_applied(&self, mission_id: &str) -> Result<u64, LedgerError> {
        let n: Option<i64> = self.conn.query_row("SELECT last_seq FROM applied WHERE mission_id = ?1", [mission_id], |r| r.get(0)).ok();
        Ok(n.unwrap_or(0) as u64)
    }

    /// The head of a mission's chain as of the last applied event, kept here so a cut tail is caught (`specs/040` §21.6).
    pub fn chain_head(&self, mission_id: &str) -> Option<(u64, String)> {
        self.conn.query_row("SELECT seq, h FROM chain_head WHERE mission_id = ?1", [mission_id], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?))).ok()
    }

    /// [`Ledger::chain_head`] read straight from a `state.db` without opening it for writing; `None` when there is no such file or head.
    pub fn stored_chain_head(db: &Path, mission_id: &str) -> Option<(u64, String)> {
        let conn = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
        conn.query_row("SELECT seq, h FROM chain_head WHERE mission_id = ?1", [mission_id], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?))).ok()
    }

    /// Apply one event. Events at or below the mission's `last_seq` are ignored (idempotent); a
    /// later event that skips ahead is an error. Only `agent.usage` events add rows, but every
    /// event advances `last_seq`, so the position always matches the log.
    pub fn apply(&mut self, e: &EventEnvelope) -> Result<(), LedgerError> {
        let last = self.last_applied(&e.mission_id)?;
        if e.seq <= last {
            return Ok(());
        }
        if e.seq != last + 1 {
            return Err(LedgerError::Gap { mission_id: e.mission_id.clone(), expected: last + 1, found: e.seq });
        }
        let tx = self.conn.transaction()?;
        if e.kind == "agent.usage" {
            let u = UsagePayload::from_value(&e.payload).map_err(LedgerError::Invalid)?;
            let need = |name: &str, v: &Option<String>| v.clone().ok_or_else(|| LedgerError::Invalid(format!("agent.usage is missing {}", name)));
            tx.execute(
                "INSERT INTO usage_ledger VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
                params![
                    e.mission_id,
                    e.seq as i64,
                    e.timestamp,
                    need("task_id", &e.task_id)?,
                    need("span_id", &e.span_id)?,
                    need("agent", &e.agent)?,
                    e.mode.clone().unwrap_or_else(|| "direct".into()),
                    u.provider,
                    need("model_tier", &e.model_tier)?,
                    need("model_id", &e.model_id)?,
                    e.attempt.ok_or_else(|| LedgerError::Invalid("agent.usage is missing attempt".into()))?,
                    u.input_tokens as i64,
                    u.cache_read_tokens as i64,
                    u.cache_write_tokens as i64,
                    u.output_tokens as i64,
                    u.thinking_tokens as i64,
                    u.cost_usd,
                    u.reserved_usd,
                    need("endpoint_id", &e.endpoint_id)?,
                    need("access_path", &e.access_path)?,
                    u.usage_source,
                    u.cost_model,
                    u.outcome,
                ],
            )?;
        }
        if let Some(h) = &e.h {
            tx.execute("INSERT INTO chain_head(mission_id, seq, h) VALUES (?1, ?2, ?3) ON CONFLICT(mission_id) DO UPDATE SET seq = excluded.seq, h = excluded.h", params![e.mission_id, e.seq as i64, h])?;
        }
        tx.execute("INSERT INTO applied(mission_id, last_seq) VALUES (?1, ?2) ON CONFLICT(mission_id) DO UPDATE SET last_seq = excluded.last_seq", params![e.mission_id, e.seq as i64])?;
        tx.commit()?;
        Ok(())
    }

    /// Bring the ledger up to date with a mission's log (also the full rebuild after `state.db`
    /// was deleted). Returns the number of events newly applied.
    pub fn catch_up(&mut self, mission_dir: &Path) -> Result<u64, LedgerError> {
        let mut applied = 0;
        for e in replay(mission_dir)? {
            let before = self.last_applied(&e.mission_id)?;
            self.apply(&e)?;
            if e.seq > before {
                applied += 1;
            }
        }
        Ok(applied)
    }

    /// Every token the install used since `ts` (an RFC 3339 time), all roles and missions.
    pub fn tokens_since(&self, ts: &str) -> Result<u64, LedgerError> {
        let n: i64 = self.conn.query_row("SELECT COALESCE(SUM(input_tokens + output_tokens + thinking_tokens), 0) FROM usage_ledger WHERE ts >= ?1", [ts], |r| r.get(0))?;
        Ok(n as u64)
    }

    /// Totals grouped by mission, role, task or model, optionally limited to one mission.
    pub fn totals(&self, group: GroupBy, mission: Option<&str>) -> Result<Vec<Totals>, LedgerError> {
        let col = match group {
            GroupBy::Mission => "mission_id",
            GroupBy::Role => "role",
            GroupBy::Task => "task_id",
            GroupBy::Model => "model_id",
        };
        let sql = format!(
            "SELECT {col}, COUNT(*), SUM(input_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens), SUM(output_tokens), SUM(thinking_tokens), SUM(cost_usd) \
             FROM usage_ledger WHERE (?1 IS NULL OR mission_id = ?1) GROUP BY {col} ORDER BY {col}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([mission], |r| {
            Ok(Totals {
                key: r.get(0)?,
                calls: r.get::<_, i64>(1)? as u64,
                input_tokens: r.get::<_, i64>(2)? as u64,
                cache_read_tokens: r.get::<_, i64>(3)? as u64,
                cache_write_tokens: r.get::<_, i64>(4)? as u64,
                output_tokens: r.get::<_, i64>(5)? as u64,
                thinking_tokens: r.get::<_, i64>(6)? as u64,
                cost_usd: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Dollar spend recorded for a mission.
    pub fn mission_cost_usd(&self, mission: &str) -> Result<f64, LedgerError> {
        Ok(self.totals(GroupBy::Mission, Some(mission))?.first().map(|t| t.cost_usd).unwrap_or(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{EventLog, LogOptions};
    use serde_json::json;

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn usage_event(mission: &str, role: &str, task: &str, model: &str, attempt: u32, input: u64, output: u64, cost: f64) -> EventEnvelope {
        let mut e = EventEnvelope::new(
            mission,
            "agent.usage",
            json!({"provider": "anthropic", "input_tokens": input, "cache_read_tokens": 10, "cache_write_tokens": 5, "output_tokens": output,
                   "thinking_tokens": 3, "cost_usd": cost, "reserved_usd": cost * 2.0, "usage_source": "reported", "cost_model": "metered", "outcome": "ok"}),
        );
        e.agent = Some(role.into());
        e.span_id = Some(format!("sp_{}_{}", role, task));
        e.task_id = Some(task.into());
        e.endpoint_id = Some("direct".into());
        e.access_path = Some("api".into());
        e.model_tier = Some("workhorse".into());
        e.model_id = Some(model.into());
        e.attempt = Some(attempt);
        e
    }

    fn fill(dir: &Path, mission: &str) {
        let mut log = EventLog::open(dir, LogOptions { max_segment_bytes: 1500 }).unwrap();
        log.append(EventEnvelope::new(mission, "mission.created", json!({"goal": "g"}))).unwrap();
        for i in 0..12u64 {
            let role = ["orchestrator", "architect", "red-team"][(i % 3) as usize];
            log.append(usage_event(mission, role, &format!("t{}", i % 4), if i % 5 == 0 { "model-b" } else { "model-a" }, 1 + (i % 2) as u32, 100 + i * 7, 50 + i * 3, 0.01 * (i + 1) as f64)).unwrap();
            log.append(EventEnvelope::new(mission, "dag.node.state", json!({"state": "running"}))).unwrap();
        }
    }

    fn all_totals(l: &Ledger) -> Vec<Vec<Totals>> {
        [GroupBy::Mission, GroupBy::Role, GroupBy::Task, GroupBy::Model].iter().map(|g| l.totals(*g, None).unwrap()).collect()
    }

    // specs/005 §6 test 8
    #[test]
    fn a_cut_tail_is_caught_by_the_head_the_ledger_kept() {
        let t = tempfile::tempdir().unwrap();
        fill(t.path(), "m1");
        let mut l = Ledger::open_in_memory().unwrap();
        l.catch_up(t.path()).unwrap();
        let stored = l.chain_head("m1");
        assert!(stored.is_some());
        let whole = crate::log::verify_chain(t.path()).unwrap().against_stored_head(stored.clone());
        assert_eq!(whole.broken, None);
        let last = std::fs::read_dir(t.path().join("events")).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).max().unwrap();
        std::fs::write(&last, "").unwrap();
        let cut = crate::log::verify_chain(t.path()).unwrap();
        assert_eq!(cut.broken, None, "the chain itself is intact");
        assert!(cut.against_stored_head(stored).broken.unwrap().contains("cut"));
    }

    #[test]
    fn deleting_state_db_and_replaying_gives_identical_totals() {
        let t = tempfile::tempdir().unwrap();
        let (m1, m2) = (t.path().join("m1"), t.path().join("m2"));
        fill(&m1, "m1");
        fill(&m2, "m2");
        let db = t.path().join("state.db");
        let mut ledger = Ledger::open(&db).unwrap();
        assert!(ledger.catch_up(&m1).unwrap() > 0);
        ledger.catch_up(&m2).unwrap();
        let before = all_totals(&ledger);
        assert_eq!(before[0].len(), 2, "two missions share one database without colliding seqs");
        drop(ledger);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", db.display(), suffix));
        }
        let mut rebuilt = Ledger::open(&db).unwrap();
        assert_eq!(rebuilt.last_applied("m1").unwrap(), 0);
        rebuilt.catch_up(&m1).unwrap();
        rebuilt.catch_up(&m2).unwrap();
        assert_eq!(all_totals(&rebuilt), before);
    }

    #[test]
    fn apply_is_idempotent_and_catch_up_is_incremental() {
        let t = tempfile::tempdir().unwrap();
        fill(t.path(), "m1");
        let mut ledger = Ledger::open_in_memory().unwrap();
        let first = ledger.catch_up(t.path()).unwrap();
        assert_eq!(ledger.catch_up(t.path()).unwrap(), 0, "nothing new the second time");
        assert_eq!(ledger.last_applied("m1").unwrap(), first);
        let totals = ledger.totals(GroupBy::Mission, Some("m1")).unwrap();
        assert_eq!(totals[0].calls, 12);
        let mut log = EventLog::open(t.path(), LogOptions::default()).unwrap();
        log.append(usage_event("m1", "builder", "t9", "model-a", 1, 1, 1, 1.0)).unwrap();
        drop(log);
        assert_eq!(ledger.catch_up(t.path()).unwrap(), 1);
        assert_eq!(ledger.totals(GroupBy::Mission, Some("m1")).unwrap()[0].calls, 13);
    }

    #[test]
    fn out_of_order_events_are_rejected() {
        let mut ledger = Ledger::open_in_memory().unwrap();
        let mut e = EventEnvelope::new("m1", "mission.created", json!({}));
        e.seq = 2;
        assert!(matches!(ledger.apply(&e), Err(LedgerError::Gap { expected: 1, found: 2, .. })));
    }

    #[test]
    fn totals_group_correctly() {
        let mut ledger = Ledger::open_in_memory().unwrap();
        let mut seq = 0;
        for (role, task, cost) in [("builder", "t1", 0.5), ("builder", "t2", 0.25), ("red-team", "t1", 1.0)] {
            seq += 1;
            let mut e = usage_event("m1", role, task, "model-a", 1, 100, 50, cost);
            e.seq = seq;
            e.timestamp = "2026-01-01T00:00:00.000Z".into();
            ledger.apply(&e).unwrap();
        }
        let by_role = ledger.totals(GroupBy::Role, None).unwrap();
        assert_eq!(by_role.iter().map(|t| (t.key.as_str(), t.calls, t.cost_usd)).collect::<Vec<_>>(), vec![("builder", 2, 0.75), ("red-team", 1, 1.0)]);
        assert_eq!(ledger.totals(GroupBy::Task, None).unwrap().len(), 2);
        assert!((ledger.mission_cost_usd("m1").unwrap() - 1.75).abs() < 1e-12);
    }
}
