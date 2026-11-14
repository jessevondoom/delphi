//! The operator's standing consent for foreign seats (`specs/104` §5, as amended by §12 A2 and A4).
//!
//! `[access] standing_foreign_tokens` (default 0: off) is read from the ratified copy of `config.toml` only, so a host agent that can edit
//! the live file cannot raise it. Above 0, a read-only foreign seat asked for without `operator_confirmed` runs if the tokens of standing-
//! consent seats in the last 24 hours, the ceilings of those still running included, plus this seat's ceiling, stay at or under it.
//!
//! The check and the debit are one step (A2): [`reserve`] reads the ledger, sums it, compares and records the reservation under one
//! process-wide lock before the seat starts; [`settle`] replaces a reservation by the tokens the seat really used. The ledger is a small
//! JSON file in the workspace's `local/`, so a restart keeps the day's sum; a reservation a crashed daemon left behind counts at its
//! ceiling until it is 24 hours old, which can only refuse more.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How far back the standing sum reaches.
pub const WINDOW_SECS: i64 = 24 * 3600;

/// One process at a time reads and writes the ledger: the reservation is atomic.
static GATE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Entry {
    id: String,
    at: i64,
    tokens: u64,
    /// Still running: `tokens` is its ceiling.
    reserved: bool,
}

fn path(ws_dir: &Path) -> PathBuf {
    ws_dir.join("local/standing_seats.json")
}

fn load(ws_dir: &Path, now: i64) -> Vec<Entry> {
    let list: Vec<Entry> = std::fs::read_to_string(path(ws_dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    list.into_iter().filter(|e| now - e.at < WINDOW_SECS).collect()
}

fn store(ws_dir: &Path, list: &[Entry]) -> Result<(), String> {
    let p = path(ws_dir);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// The tokens standing-consent seats have used in the last 24 hours, the ceilings of those still running included.
pub fn used(ws_dir: &Path, now: i64) -> u64 {
    let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
    load(ws_dir, now).iter().map(|e| e.tokens).sum()
}

/// The same sum in two parts for the console: the tokens seats have really used (settled) in the last 24 hours, and the ceilings
/// still reserved by seats that are running. `used` is their total.
pub fn split(ws_dir: &Path, now: i64) -> (u64, u64) {
    let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
    let list = load(ws_dir, now);
    let reserved: u64 = list.iter().filter(|e| e.reserved).map(|e| e.tokens).sum();
    let settled: u64 = list.iter().filter(|e| !e.reserved).map(|e| e.tokens).sum();
    (settled, reserved)
}

/// Reserve `ceiling` tokens for the seat `id` under the standing amount `cap`, in one step. `Ok(used)` with the sum now (this seat
/// included); `Err(used)` with the sum before it when the seat does not fit, and then nothing is recorded. A ledger that cannot be
/// written refuses (`Err`), so a seat never runs on a reservation that was not kept.
pub fn reserve(ws_dir: &Path, id: &str, ceiling: u64, cap: u64, now: i64) -> Result<u64, u64> {
    let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = load(ws_dir, now);
    let before: u64 = list.iter().map(|e| e.tokens).sum();
    if cap == 0 || before.saturating_add(ceiling) > cap {
        return Err(before);
    }
    list.push(Entry { id: id.to_string(), at: now, tokens: ceiling, reserved: true });
    store(ws_dir, &list).map_err(|_| before)?;
    Ok(before + ceiling)
}

/// The seat `id` has ended: its reservation becomes the tokens it really used. An id with no reservation changes nothing.
pub fn settle(ws_dir: &Path, id: &str, actual: u64, now: i64) {
    let _g = GATE.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = load(ws_dir, now);
    let mut changed = false;
    for e in list.iter_mut().filter(|e| e.id == id && e.reserved) {
        e.tokens = actual;
        e.reserved = false;
        changed = true;
    }
    if changed {
        let _ = store(ws_dir, &list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reservation_fits_or_is_refused_whole_and_settling_replaces_the_ceiling_by_the_real_spend() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(reserve(d.path(), "a", 100, 0, 1_000), Err(0), "0 is off: nothing fits");
        assert_eq!(reserve(d.path(), "a", 100, 250, 1_000), Ok(100));
        assert_eq!(reserve(d.path(), "b", 100, 250, 1_000), Ok(200));
        assert_eq!(reserve(d.path(), "c", 100, 250, 1_000), Err(200), "a seat that would cross the amount is refused and not recorded");
        assert_eq!(used(d.path(), 1_000), 200);
        settle(d.path(), "a", 30, 1_001);
        assert_eq!(used(d.path(), 1_001), 130, "the ceiling is replaced by what the seat used");
        assert_eq!(split(d.path(), 1_001), (30, 100), "what a seat used, and what a running seat has reserved");
        assert_eq!(reserve(d.path(), "c", 100, 250, 1_002), Ok(230));
        assert_eq!(used(d.path(), 1_000 + WINDOW_SECS + 5), 0, "the sum reaches back 24 hours");
    }

    #[test]
    fn parallel_reservations_never_pass_the_amount() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().to_path_buf();
        let ok: usize = (0..16).map(|i| {
            let dir = dir.clone();
            std::thread::spawn(move || reserve(&dir, &format!("s{i}"), 100, 450, 5_000).is_ok())
        }).collect::<Vec<_>>().into_iter().map(|h| h.join().unwrap()).filter(|ok| *ok).count();
        assert_eq!(ok, 4, "exactly the number that fit");
        assert_eq!(used(&dir, 5_000), 400);
    }
}
