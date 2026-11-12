//! The operator's controls over the project history (`specs/102` §7 item 6, slice S4): `off`, `purge [--since]`, `delete <id>` and `status`.
//!
//! * **Local files only.** Nothing here reaches the network, a process or a model, and it adds no daemon method: there is nothing for a
//!   host or a seat to call. The CLI (`delphi history`, the operator's, refused inside an agent seat) is the only caller.
//! * **`off` deletes nothing.** It writes a marker file the daemon checks at every append, so it takes effect at once and a running
//!   daemon needs no restart. Removing the marker is the way back.
//! * **`purge` and `delete` leave a tombstone** in place of each erased record: its id, its time, the hash of the record before it, its
//!   own keyed hash and a keyed seal, and **no text** ([`Tombstone`]). The line count does not change, so ids stay unique, and
//!   [`history_record::read`] verifies the chain through every tombstone. A record that had already failed the chain check leaves a
//!   tombstone that is itself a break, so erasing text never launders a tampered record. A line that cannot be read at all is replaced
//!   by a void tombstone (text gone, break kept).
//! * **The rewrite is atomic and locked.** It takes the in-process gate and the cross-process store lock the daemon appends under, writes
//!   a sibling temporary file with owner-only permission and renames it over the store.
//! * **`status` leaks no record text.** It prints fixed templates, counts, sizes, a verified/broken chain state and the timestamp of the
//!   last record (and only if it parses as a time).

use std::path::{Path, PathBuf};

use crate::history_pointer::{load_key, load_or_create_key};
use crate::history_record::{self as rec, lock_store, make_tombstone, parse_tombstone, tomb_seal, Record, Settings, Status, Tombstone, APPEND_GATE};
use crate::history_report::safe_id;

// ---------------------------------------------------------------------------------------------------------------------------
// off

/// `delphi history off [--project <id>]`: stop new records at once. Nothing is deleted. Returns the marker that was written.
pub fn off(ws_dir: &Path, project: Option<&str>) -> Result<PathBuf, String> {
    let path = match project {
        None => rec::global_off_path(ws_dir),
        Some(p) => {
            let id = safe_id(p).ok_or_else(|| "that is not a project id".to_string())?;
            ws_dir.join("local/projects").join(id).join("history.off")
        }
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|_| "the switch could not be written".to_string())?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(&path).map_err(|_| "the switch could not be written".to_string())?;
    Ok(path)
}

// ---------------------------------------------------------------------------------------------------------------------------
// purge and delete

/// Which records to erase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which<'a> {
    /// Every record.
    All,
    /// The records written at or after this Unix time (`purge --since`). A record whose time cannot be read is erased too: the safe side.
    Since(i64),
    /// One record, by id (`delete <id>`).
    Id(&'a str),
}

/// What an erase did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Erased {
    /// Records that became tombstones.
    pub records: usize,
    /// Lines that could not be read at all and became void tombstones (their text is gone; the break stays).
    pub voided: usize,
}

fn erased_record(which: Which, r: &Record) -> bool {
    match which {
        Which::All => true,
        Which::Since(t) => delphi_core::time::parse_rfc3339(&r.at).map(|at| at >= t).unwrap_or(true),
        Which::Id(id) => r.id == id,
    }
}

/// Erase records in one project's store, leaving a tombstone for each. `removed_at` is the time to record (RFC 3339).
pub fn erase(ws_dir: &Path, project: &str, which: Which, removed_at: &str) -> Result<Erased, String> {
    let project = safe_id(project).ok_or_else(|| "that is not a project id".to_string())?;
    // A purge also tells a running daemon to drop the operator prompt it holds in memory (S7): no text, only a time.
    if !matches!(which, Which::Id(_)) {
        mark_purged(ws_dir);
    }
    let path = rec::store_path(ws_dir, &project);
    if !path.exists() {
        return match which {
            Which::Id(id) => Err(format!("no record {} in project {project}", safe_id(id).unwrap_or_else(|| "(that id)".into()))),
            _ => Ok(Erased::default()),
        };
    }
    let _gate = APPEND_GATE.lock().unwrap_or_else(|p| p.into_inner());
    let _store = lock_store(&rec::store_dir(ws_dir, &project)).map_err(String::from)?;
    let text = std::fs::read_to_string(&path).map_err(|_| "the history store cannot be read".to_string())?;
    // The chain status of each record as it stands now, in file order (a tombstone and an unreadable line are not in `records`).
    let read = rec::read(ws_dir, &project)?;
    let mut statuses = read.records.iter().map(|c| c.status);
    let key = load_or_create_key(ws_dir)?;
    let mut out = String::with_capacity(text.len());
    let mut done = Erased::default();
    let mut found = false;
    for (i, line) in text.lines().enumerate() {
        if let Some(t) = parse_tombstone(line) {
            if let Which::Id(id) = which {
                if t.id == id {
                    return Err(format!("{} was already erased", safe_id(id).unwrap_or_else(|| "that record".into())));
                }
            }
            out.push_str(line);
            out.push('\n');
            continue;
        }
        match serde_json::from_str::<Record>(line) {
            Ok(r) => {
                let status = statuses.next().unwrap_or(Status::ChainBroken);
                if erased_record(which, &r) {
                    let t = make_tombstone(&key, &r, status == Status::Verified, removed_at);
                    out.push_str(&serde_json::to_string(&t).map_err(|_| "a tombstone could not be written".to_string())?);
                    done.records += 1;
                    found = true;
                } else {
                    out.push_str(line);
                }
            }
            Err(_) => {
                // Nothing can be read from this line, so nothing can be kept of it. `delete <id>` names a line by its position's id.
                let id = format!("ex_{:04}", i + 1);
                let void = !matches!(which, Which::Id(want) if want != id);
                if void {
                    let mut t = Tombstone { id, at: String::new(), chain: String::new(), hash: String::new(), tombstone: true, removed_at: removed_at.to_string(), verified: false, seal: String::new() };
                    t.seal = tomb_seal(&key, &t);
                    out.push_str(&serde_json::to_string(&t).map_err(|_| "a tombstone could not be written".to_string())?);
                    done.voided += 1;
                    found = true;
                } else {
                    out.push_str(line);
                }
            }
        }
        out.push('\n');
    }
    if let Which::Id(id) = which {
        if !found {
            return Err(format!("no record {} in project {project}", safe_id(id).unwrap_or_else(|| "(that id)".into())));
        }
    }
    if done == Erased::default() {
        return Ok(done);
    }
    write_atomically(&path, &out)?;
    Ok(done)
}

/// Best effort: if the marker cannot be written the purge itself still runs, and the CLI's note about a running daemon stands.
fn mark_purged(ws_dir: &Path) {
    let path = rec::purged_marker_path(ws_dir);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let _ = std::fs::write(path, format!("{ms}\n"));
}

fn write_atomically(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| "the history store has no directory".to_string())?;
    let tmp = dir.join(".records.jsonl.tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = opts.open(&tmp).and_then(|mut f| f.write_all(text.as_bytes()).and_then(|_| f.sync_all())).and_then(|_| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err("the history store could not be rewritten; nothing was erased".into());
    }
    Ok(())
}

/// The projects that have a store, in name order. A directory whose name is not a safe id is not listed.
pub fn projects(ws_dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(ws_dir.join(rec::STORE_DIR))
        .map(|d| d.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().to_str().and_then(safe_id)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

// ---------------------------------------------------------------------------------------------------------------------------
// status

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chain {
    /// Nothing is stored.
    Empty,
    Intact,
    /// The chain broke at this line (1-based): that record and every later one is left out of every count.
    Broken(usize),
    /// There is no key to check with.
    Unverifiable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Absent,
    Present,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub project: String,
    /// Records still holding text.
    pub records: usize,
    /// Of those, the ones that verified.
    pub verified: usize,
    /// Records erased by `purge` or `delete`.
    pub tombstones: usize,
    pub bytes: u64,
    pub chain: Chain,
    /// The time of the last verified record, if it reads as a time.
    pub last_at: Option<String>,
    /// Verified records older than the age budget. Nothing is deleted at that age.
    pub past_age: usize,
    pub off: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    pub enabled: bool,
    pub switched_off: bool,
    pub key: KeyState,
    pub budget_mb: u64,
    pub retention_days: u32,
    pub timeline: bool,
    pub rows: Vec<ProjectRow>,
    /// Fixed reasons a store could not be read (never record text).
    pub problems: Vec<String>,
}

/// What is stored, without reading any record's text into the answer. `now` is Unix seconds.
pub fn overview(ws_dir: &Path, settings: &Settings, now: i64) -> Overview {
    let key = match load_key(ws_dir) {
        Ok(Some(_)) => KeyState::Present,
        Ok(None) => KeyState::Absent,
        Err(_) => KeyState::Unreadable,
    };
    let mut o = Overview {
        enabled: settings.enabled,
        switched_off: rec::global_off(ws_dir),
        key,
        budget_mb: settings.budget_mb,
        retention_days: settings.retention_days,
        timeline: settings.timeline,
        rows: Vec::new(),
        problems: Vec::new(),
    };
    let cutoff = now - i64::from(settings.retention_days) * 86_400;
    for project in projects(ws_dir) {
        let bytes = std::fs::metadata(rec::store_path(ws_dir, &project)).map(|m| m.len()).unwrap_or(0);
        let read = match rec::read(ws_dir, &project) {
            Ok(r) => r,
            Err(_) => {
                o.problems.push(format!("project {project}: the store cannot be read"));
                continue;
            }
        };
        let verified: Vec<&Record> = read.counted().collect();
        let chain = if read.records.is_empty() && read.tombstones.is_empty() && read.broken_at.is_none() {
            Chain::Empty
        } else if let Some(i) = read.broken_at {
            Chain::Broken(i + 1)
        } else if o.key != KeyState::Present {
            Chain::Unverifiable
        } else {
            Chain::Intact
        };
        let last_at = verified.last().map(|r| r.at.clone()).filter(|at| delphi_core::time::parse_rfc3339(at).is_some());
        let past_age = verified.iter().filter(|r| delphi_core::time::parse_rfc3339(&r.at).map(|t| t < cutoff).unwrap_or(false)).count();
        o.rows.push(ProjectRow {
            off: rec::project_off(ws_dir, &project),
            project,
            records: read.records.len(),
            verified: verified.len(),
            tombstones: read.tombstones.len(),
            bytes,
            chain,
            last_at,
            past_age,
        });
    }
    o
}

fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

fn chain_word(c: Chain) -> String {
    match c {
        Chain::Empty => "nothing stored".into(),
        Chain::Intact => "chain intact".into(),
        Chain::Broken(n) => format!("CHAIN BROKEN at line {n} (that record and every later one is left out)"),
        Chain::Unverifiable => "chain cannot be checked (no key)".into(),
    }
}

/// The `delphi history status` lines. Fixed templates, counts, sizes, project ids and a timestamp that parsed: no record text.
pub fn status_lines(o: &Overview) -> Vec<String> {
    let mut v = Vec::new();
    v.push(
        match (o.enabled, o.switched_off) {
            (_, true) => "History: off (switched off with `delphi history off`; nothing new is recorded)",
            (true, false) => "History: on",
            (false, false) => "History: off (the default until it is turned on; nothing is recorded)",
        }
        .to_string(),
    );
    v.push(
        match o.key {
            KeyState::Present => "Key: present",
            KeyState::Absent => "Key: not made yet (it is made when the first record is written)",
            KeyState::Unreadable => "Key: UNREADABLE (records cannot be checked; nothing is repaired)",
        }
        .to_string(),
    );
    let records: usize = o.rows.iter().map(|r| r.records).sum();
    let tombs: usize = o.rows.iter().map(|r| r.tombstones).sum();
    let bytes: u64 = o.rows.iter().map(|r| r.bytes).sum();
    v.push(format!("Stored: {records} record(s) and {tombs} erased (tombstone) in {} project(s), {} (budget {} MB per project)", o.rows.len(), size(bytes), o.budget_mb));
    for r in &o.rows {
        let mut line = format!("  {}: {} record(s) ({} verified), {} erased, {}, {}", r.project, r.records, r.verified, r.tombstones, size(r.bytes), chain_word(r.chain));
        if let Some(at) = &r.last_at {
            line.push_str(&format!(", last {at}"));
        }
        if r.off {
            line.push_str(" [off for this project]");
        }
        v.push(line);
    }
    let past: usize = o.rows.iter().map(|r| r.past_age).sum();
    if past > 0 {
        v.push(format!("Past the age budget ({} days): {past} record(s). Nothing is deleted at that age; `delphi history purge` erases.", o.retention_days));
    }
    v.push(format!("Timeline: {}", if o.timeline { "on (ids and counts only, no operator text)" } else { "off" }));
    for p in &o.problems {
        v.push(format!("Problem: {p}"));
    }
    v
}

/// The one `delphi doctor` line, and whether it is a problem.
pub fn doctor_line(o: &Overview) -> (bool, String) {
    let broken = o.rows.iter().find_map(|r| if let Chain::Broken(n) = r.chain { Some((r.project.clone(), n)) } else { None });
    if let Some((p, n)) = broken {
        return (true, format!("history: the chain of project {p} is broken at line {n}; the records after it are left out of every count (nothing is repaired)"));
    }
    if o.key == KeyState::Unreadable {
        return (true, "history: the key is unreadable, so records cannot be checked".into());
    }
    if let Some(p) = o.problems.first() {
        return (true, format!("history: {p}"));
    }
    let records: usize = o.rows.iter().map(|r| r.records).sum();
    let tombs: usize = o.rows.iter().map(|r| r.tombstones).sum();
    let state = if o.switched_off { "off (switched off)" } else if o.enabled { "on" } else { "off (default)" };
    let key = if o.key == KeyState::Present { "key present" } else { "no key yet" };
    (false, format!("history: {state}; {records} record(s), {tombs} erased; chain intact; {key}"))
}

#[cfg(test)]
#[path = "history_manage_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "history_residue_tests.rs"]
mod residue_tests;
