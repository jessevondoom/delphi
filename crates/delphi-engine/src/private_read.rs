//! The private-read mark, slice M1 (`specs/103`): OBSERVE ONLY. A task (for a host over MCP, the host session) that was handed the text of a
//! private entry is *marked*. In this slice the mark is computed, logged, counted and clearable at the console, and it changes nothing
//! else: no read returns a different result, no prompt differs, no send differs, no gate or filter is added to anything that flows in
//! (the operator, 2026-11-11: research must be easier to find, not harder). `M2` and `M3` use the counter this slice builds to decide
//! what to do on Outbound, Connector, Relay and Mail.
//!
//! * The mark is not a tier and not a taint level and grants nothing (GOVERNANCE §4). It records entry ids and a time, never text.
//! * It is set only by the readers listed in [`READS`], through [`mark`]. An inventory test fails when a file that reads entry text is not
//!   listed, so a new reader cannot ship unrecorded.
//! * It is cleared only by [`clear`], whose one caller is the daemon's console-scope method `private_read.clear`. A test enumerates the
//!   callers; no tool, no host method, no relay reply and no handoff reaches it (`specs/103` §4.3, §7.1 OD-4: the delegated form, an agent
//!   holding an operator permission, is a decision before M2 and is NOT built here).
//! * An unreadable mark file reads as [`State::Unreadable`]. The counter counts such a task as one a send would have changed; in M1 that
//!   changes nothing, and M2 and M3 make it fail closed on the four channels only.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

/// The log event written when a task first reads an entry it had not read before (ids and time only).
pub const MARK_EVENT: &str = "task.private_read";
/// The log event written when the operator clears a mark at the console.
pub const CLEAR_EVENT: &str = "task.private_read_cleared";

/// One writer at a time within the daemon process: the files are small read-modify-write records.
static LOCK: Mutex<()> = Mutex::new(());

/// What a reader does with a private entry's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// The reader calls [`mark`] for the caller when it returns a private entry's text.
    Marks,
    /// The reader refuses a private entry's text by rule; the named test proves it (file relative to the `crates/` directory).
    Withholds { proof_file: &'static str, proof_test: &'static str },
    /// The reader does not mark yet. Said plainly: the first table is not validated complete, and `M4` attacks it. `why` is what keeps the
    /// exposure small today; M2 and M3 must not flip a channel to `Yes` for a task that such a reader can feed.
    NotYetMarked { why: &'static str },
    /// The file writes or defines the thing but returns no entry text to a model or host.
    NoReturn { why: &'static str },
}

/// One file that touches the text of a private entry and what it does about it. The file is relative to this crate's `src`.
pub struct Read {
    pub file: &'static str,
    pub what: &'static str,
    pub how: How,
}

/// Every file of this crate that returns, or can return, the text of a private entry. The inventory test below checks the table against
/// the accessors it scans for; a reader of private text that is not here fails it.
pub const READS: [Read; 8] = [
    Read { file: "service.rs", what: "`host.search_kb`: the `noted` layer returns entries a host left, with the preview; one the daemon stored private is private text handed to the host", how: How::Marks },
    Read { file: "library_claims.rs", what: "`claim_slice`: opening `@library/claim/<id>`", how: How::Withholds { proof_file: "delphi-engine/src/private_read.rs", proof_test: "a_claim_that_rests_on_a_private_source_is_withheld_from_the_seat_that_opens_it" } },
    Read { file: "engine.rs", what: "block 5 of a span: opens a claim pointer through `claim_slice` (above)", how: How::Withholds { proof_file: "delphi-engine/src/private_read.rs", proof_test: "a_claim_that_rests_on_a_private_source_is_withheld_from_the_seat_that_opens_it" } },
    Read { file: "library_page_search.rs", what: "a page search in the internal scope reads private sources into the page writer", how: How::NotYetMarked { why: "the writer is a model with no tools and no path to the four channels; the page is a library record, not a send. An external page leaves private sources out" } },
    Read { file: "library_page_check.rs", what: "the page check reads source text to verify quotes (internal scope includes private sources)", how: How::NotYetMarked { why: "the checker reads text to compare it and returns verdicts, not text; an external page refuses a private source" } },
    Read { file: "library_page_validate.rs", what: "the page validator reads source text", how: How::NotYetMarked { why: "a validator seat with no tools; it returns a verdict" } },
    Read { file: "library_pages.rs", what: "writes the `Noted, not validated` section of a page", how: How::NoReturn { why: "it composes a library page from entries that are already stored; it is not a reader of private text for a caller" } },
    Read { file: "host_noted.rs", what: "stores a host's entry, deciding `private`", how: How::NoReturn { why: "it stores and labels; it returns no entry text" } },
];

/// The accessors whose callers are the readers of entry text. Matched against the production text of every file of this crate.
/// This is a token scan of three accessors, not a proof of completeness (`specs/103` §2: NOT VALIDATED; the red-team slice M4 attacks it).
pub const ACCESSORS: [&str; 3] = ["stored_text", "claim_slice(", "NOTED_HEADING"];

/// A key is a task or host-session identifier: a short run of characters that cannot name a path or hold prose.
fn key_ok(key: &str) -> bool {
    // letters, digits, `-`, `_`, `:` and `/` between non-empty parts (`<mission>/<task>`); no `.`, so no `..`, and no leading or doubled `/`
    (1..=96).contains(&key.len()) && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '/')) && key.split('/').all(|part| !part.is_empty())
}

/// An entry id is an identifier too, never text.
fn id_ok(id: &str) -> bool {
    (1..=160).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '#'))
}

pub fn dir(ws_dir: &Path) -> PathBuf {
    ws_dir.join("local/private_read")
}

fn file_of(ws_dir: &Path, key: &str) -> PathBuf {
    dir(ws_dir).join(format!("{}.json", key.replace('/', "__").replace(':', "_")))
}

fn counter_path(ws_dir: &Path) -> PathBuf {
    dir(ws_dir).join("counter.json")
}

/// What is known about a key's mark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Nothing was recorded, or every entry read has been cleared.
    Unmarked,
    /// At least one entry read is not cleared. Ids and the first time only.
    Marked { entries: Vec<String>, first_at: i64 },
    /// A record exists and cannot be read. M2 and M3 treat the task as marked on the four channels only; M1 only counts it.
    Unreadable,
}

impl State {
    /// Would a send on one of the four channels be affected by this state once M2 and M3 act on it?
    pub fn would_affect_a_send(&self) -> bool {
        !matches!(self, State::Unmarked)
    }
}

fn read_record(path: &Path) -> Result<Option<Value>, ()> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
        Ok(t) => match serde_json::from_str::<Value>(&t) {
            Ok(v) if v.is_object() && v["entries"].is_array() => Ok(Some(v)),
            _ => Err(()),
        },
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The state of one key's mark. A key that is not a valid identifier reads as unreadable (deny by default).
pub fn state(ws_dir: &Path, key: &str) -> State {
    if !key_ok(key) {
        return State::Unreadable;
    }
    match read_record(&file_of(ws_dir, key)) {
        Err(()) => State::Unreadable,
        Ok(None) => State::Unmarked,
        Ok(Some(v)) => {
            let cleared = strings(&v["cleared"]);
            let live: Vec<String> = strings(&v["entries"]).into_iter().filter(|e| !cleared.contains(e)).collect();
            if live.is_empty() {
                State::Unmarked
            } else {
                State::Marked { entries: live, first_at: v["first_at"].as_i64().unwrap_or(0) }
            }
        }
    }
}

/// The running totals M2 and M3 decide from. Counts only; nothing here names text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counter {
    /// Keys that have ever been marked.
    pub keys_marked: u64,
    /// Entries marked in all (an entry read again by the same key is not counted twice).
    pub entries_marked: u64,
    /// Times the operator cleared a mark.
    pub cleared: u64,
    /// Sends on Outbound that the mark would have changed, had M2 acted on it.
    pub outbound_would_change: u64,
    /// Sends on Connector that the mark would have changed.
    pub connector_would_change: u64,
    /// Of those, how many were because the mark could not be read.
    pub unreadable: u64,
}

fn read_counter(ws_dir: &Path) -> Counter {
    let v = std::fs::read_to_string(counter_path(ws_dir)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    let n = |k: &str| v[k].as_u64().unwrap_or(0);
    Counter { keys_marked: n("keys_marked"), entries_marked: n("entries_marked"), cleared: n("cleared"), outbound_would_change: n("outbound_would_change"), connector_would_change: n("connector_would_change"), unreadable: n("unreadable") }
}

fn write_counter(ws_dir: &Path, c: &Counter) -> Result<(), String> {
    let v = json!({"keys_marked": c.keys_marked, "entries_marked": c.entries_marked, "cleared": c.cleared, "outbound_would_change": c.outbound_would_change, "connector_would_change": c.connector_would_change, "unreadable": c.unreadable});
    write_atomic(&counter_path(ws_dir), &v)
}

fn write_atomic(path: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn counter(ws_dir: &Path) -> Counter {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    read_counter(ws_dir)
}

/// Record that `key` was handed the text of the entry `entry_id` at `now` (unix seconds). Called only by the readers in [`READS`].
/// Returns whether this was an entry the key had not read before (the caller then writes the [`MARK_EVENT`] event). It changes nothing
/// about what the reader returns.
pub fn mark(ws_dir: &Path, key: &str, entry_id: &str, now: i64) -> Result<bool, String> {
    if !key_ok(key) || !id_ok(entry_id) {
        return Err("a mark names a key and an entry by identifier only".into());
    }
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = file_of(ws_dir, key);
    let (mut rec, first_key) = match read_record(&path) {
        Ok(Some(v)) => (v, false),
        Ok(None) => (json!({"key": key, "entries": [], "cleared": [], "first_at": now}), true),
        // an unreadable record is replaced by a fresh one that names the entry: the mark cannot be lost by damage, only re-made
        Err(()) => (json!({"key": key, "entries": [], "cleared": [], "first_at": now}), true),
    };
    let mut entries = strings(&rec["entries"]);
    if entries.iter().any(|e| e == entry_id) {
        return Ok(false);
    }
    if entries.len() >= 200 {
        // the list is capped; the mark stays (a key with 200 entries read is marked either way)
        return Ok(false);
    }
    entries.push(entry_id.to_string());
    rec["entries"] = json!(entries);
    write_atomic(&path, &rec)?;
    let mut c = read_counter(ws_dir);
    c.entries_marked += 1;
    if first_key {
        c.keys_marked += 1;
    }
    write_counter(ws_dir, &c)?;
    Ok(true)
}

/// Lift the mark on `key`: every entry read so far is recorded as cleared. A later read of a different entry marks again. Returns how many
/// entries were cleared. The one caller is the daemon's console-scope `private_read.clear`.
pub fn clear(ws_dir: &Path, key: &str, now: i64) -> Result<usize, String> {
    if !key_ok(key) {
        return Err("not a valid key".into());
    }
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = file_of(ws_dir, key);
    let mut rec = match read_record(&path) {
        Ok(Some(v)) => v,
        Ok(None) => return Ok(0),
        Err(()) => json!({"key": key, "entries": [], "cleared": [], "first_at": now}), // clearing an unreadable record replaces it
    };
    let entries = strings(&rec["entries"]);
    let mut cleared = strings(&rec["cleared"]);
    let newly: Vec<String> = entries.iter().filter(|e| !cleared.contains(e)).cloned().collect();
    cleared.extend(newly.iter().cloned());
    rec["cleared"] = json!(cleared);
    rec["cleared_at"] = json!(now);
    write_atomic(&path, &rec)?;
    let mut c = read_counter(ws_dir);
    c.cleared += 1;
    write_counter(ws_dir, &c)?;
    Ok(newly.len())
}

/// OBSERVE: the send on `channel` (`"outbound"` or `"connector"`) of the task `key` is about to go ahead exactly as before. If the mark
/// would have changed it, count that. Returns nothing a caller may act on: in M1 no send depends on it.
pub fn observe_send(ws_dir: &Path, channel: &str, key: &str) {
    let st = state(ws_dir, key);
    if !st.would_affect_a_send() {
        return;
    }
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut c = read_counter(ws_dir);
    match channel {
        "outbound" => c.outbound_would_change += 1,
        "connector" => c.connector_would_change += 1,
        _ => return,
    }
    if st == State::Unreadable {
        c.unreadable += 1;
    }
    let _ = write_counter(ws_dir, &c);
}

/// The marks that are live, as `(key, entries, first_at)`, for the console and `delphi doctor`. Ids and times only.
pub fn live(ws_dir: &Path) -> Vec<(String, Vec<String>, i64)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir(ws_dir)).into_iter().flatten().flatten() {
        let p = e.path();
        if p.file_name().and_then(|n| n.to_str()) == Some("counter.json") || p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        if let Ok(Some(v)) = read_record(&p) {
            let key = v["key"].as_str().unwrap_or("").to_string();
            if let State::Marked { entries, first_at } = state(ws_dir, &key) {
                out.push((key, entries, first_at));
            }
        }
    }
    out.sort();
    out
}

/// How many readers in [`READS`] do not mark yet.
pub fn readers_not_yet_marking() -> usize {
    READS.iter().filter(|r| matches!(r.how, How::NotYetMarked { .. })).count()
}

/// One line for `delphi doctor` and the console: what observe mode has seen. Counts only.
pub fn status_line(ws_dir: &Path) -> String {
    let c = counter(ws_dir);
    format!(
        "private-read mark (observe only, changes nothing): {} task(s) marked, {} entr(y/ies) read, {} cleared; a send would have changed: outbound {}, connector {} ({} unreadable); {} reader(s) do not mark yet",
        c.keys_marked,
        c.entries_marked,
        c.cleared,
        c.outbound_would_change,
        c.connector_would_change,
        c.unreadable,
        readers_not_yet_marking()
    )
}

/// The `entry_id` a host-noted hit is recorded under, and the host mission that holds its `learn.staged` event, from the hit's relative
/// path (`staged/host-<session>/n<k>.md`). `None` for any other path.
pub fn noted_entry(rel_path: &str) -> Option<(String, String)> {
    let rest = rel_path.strip_prefix("staged/")?;
    let (mission, file) = rest.split_once('/')?;
    let id = file.strip_suffix(".md")?;
    let session_ok = mission.strip_prefix("host-").is_some_and(|s| (8..=32).contains(&s.len()) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    let id_ok_ = id.starts_with('n') && id.len() > 1 && id[1..].chars().all(|c| c.is_ascii_digit());
    (session_ok && id_ok_).then(|| (mission.to_string(), format!("{mission}/{id}")))
}

/// Did the daemon store this host-noted entry as private? Read from the `learn.staged` event of the host mission that holds it (the file
/// does not say). Deny by default: an entry whose record cannot be found or read is treated as private (`specs/103` §2: "host-noted entry
/// the daemon holds as private"; the same reading as `egress::xenia_file_record`).
pub fn noted_is_private(ws_dir: &Path, mission: &str, entry: &str) -> bool {
    let Some(id) = entry.rsplit('/').next() else { return true };
    let Ok(events) = delphi_core::log::replay(&delphi_core::sessions::dir(ws_dir, mission)) else { return true };
    events
        .iter()
        .find(|e| e.kind == "learn.staged" && e.payload["trigger"].as_str() == Some("host_noted") && e.payload["id"].as_str() == Some(id))
        .map(|e| e.payload["private"].as_bool() != Some(false))
        .unwrap_or(true)
}

/// The ids of the private host-noted entries among these search hits (relative paths), in order, once each. Reads the host missions' logs
/// and writes nothing.
pub fn private_noted_in<'a>(ws_dir: &Path, rel_paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for rel in rel_paths {
        if let Some((mission, entry)) = noted_entry(rel) {
            if !out.contains(&entry) && noted_is_private(ws_dir, &mission, &entry) {
                out.push(entry);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(ws: &Path, mission: &str, events: Vec<(&str, Value)>) {
        let mut log = delphi_core::log::EventLog::open(&delphi_core::sessions::dir(ws, mission), Default::default()).unwrap();
        for (kind, payload) in events {
            let mut ev = delphi_core::event::EventEnvelope::new(mission, kind, payload);
            ev.agent = None;
            log.append(ev).unwrap();
        }
    }

    #[test]
    fn a_private_noted_hit_is_found_from_the_log_and_a_public_or_unknown_one_is_read_as_the_log_says() {
        let d = ws();
        seed(d.path(), "host-abcd1234", vec![("learn.staged", json!({"id": "n1", "trigger": "host_noted", "private": true})), ("learn.staged", json!({"id": "n2", "trigger": "host_noted", "private": false}))]);
        let hits = ["staged/host-abcd1234/n1.md", "staged/host-abcd1234/n2.md", "staged/host-abcd1234/n3.md", "staged/host-abcd1234/n1.md", "core/x.md", "staged/host-zzzzzzzz/n1.md"];
        // n1 is private; n2 is public and is not marked; n3 has no record, and a mission with no log is unreadable: both read as private
        assert_eq!(private_noted_in(d.path(), hits), vec!["host-abcd1234/n1".to_string(), "host-abcd1234/n3".to_string(), "host-zzzzzzzz/n1".to_string()]);
    }

    /// `specs/103` §6 `inbound_reads_are_identical_with_and_without_a_mark`: the mark is not a filter. The host search builds its answer from the
    /// same `kept` hits either way; `private_noted_in` only reads their paths and returns ids, and the ids never reach the answer.
    #[test]
    fn inbound_reads_are_identical_with_and_without_a_mark() {
        let svc = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/service.rs")).unwrap();
        let body = production(&svc);
        let at = body.find("async fn host_search_kb").expect("host_search_kb exists");
        let end = body[at..].find("async fn private_read_mark").map(|n| at + n).unwrap();
        let f = &body[at..end];
        assert!(f.contains("kept.iter().map(|h| json!({\"layer\": \"noted\""), "the noted layer is built from every kept hit");
        assert!(f.contains("private_read::private_noted_in(&ws.dir, kept.iter().map(|h| h.rel_path.as_str()))"), "the mark only reads the kept hits' paths");
        assert!(!f.contains("out[\"private") && !f.contains("\"private_seen\""), "the answer carries no mark field");
        // the only uses of the ids: declared, assigned, returned with `out`, unpacked, and passed to the marker after the answer is built
        let after = &f[f.find("let (out, private_seen) = out;").expect("unpacked after the blocking part")..];
        assert!(after.contains("self.private_read_mark(&mark_key, &mark_cwd, &private_seen, \"host.search_kb\").await;"), "the marker runs after the answer is built");
        assert!(after.contains("Ok(out)"), "the answer is returned as built");
    }

    #[test]
    fn a_task_that_read_nothing_private_is_unchanged_on_all_four_channels() {
        // observe mode with no mark: no file, no counter, no event; the senders call only `observe_send`, which returns nothing
        let d = ws();
        for ch in ["outbound", "connector", "relay", "mail"] {
            observe_send(d.path(), ch, "t-9");
        }
        assert!(!dir(d.path()).exists());
        for name in ["harness_run.rs", "connector_run.rs"] {
            let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name)).unwrap();
            let used: Vec<&str> = production(&src).lines().filter(|l| l.contains("private_read::")).collect();
            assert_eq!(used.len(), 1, "{name} uses the mark module in one place only: {used:?}");
            assert!(used[0].trim_start().starts_with("crate::private_read::observe_send("), "{name}: only the observe call, as a statement whose result is not used: {used:?}");
        }
    }

    fn ws() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// The non-test text of a source file (as `egress.rs` does).
    fn production(src: &str) -> &str {
        let cut = src.rfind("\n#[cfg(test)]\nmod tests").or_else(|| src.rfind("\nmod tests")).unwrap_or(src.len());
        &src[..cut]
    }

    fn engine_sources() -> Vec<(String, String)> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            // test-support files (`*_tests.rs`, `tests.rs`) are test code, not readers
            let stem = p.file_stem().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if p.extension().is_some_and(|x| x == "rs") && !(stem == "tests" || stem.ends_with("_tests")) {
                out.push((p.file_name().unwrap().to_string_lossy().to_string(), std::fs::read_to_string(&p).unwrap()));
            }
        }
        out
    }

    #[test]
    fn a_read_marks_the_task_once_per_entry_and_records_ids_only() {
        let d = ws();
        assert_eq!(state(d.path(), "host-abcd1234"), State::Unmarked);
        assert!(mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 100).unwrap());
        assert!(!mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 200).unwrap(), "the same entry again is not a new read");
        assert!(mark(d.path(), "host-abcd1234", "host-abcd1234/n2", 300).unwrap());
        assert_eq!(state(d.path(), "host-abcd1234"), State::Marked { entries: vec!["host-abcd1234/n1".into(), "host-abcd1234/n2".into()], first_at: 100 });
        let c = counter(d.path());
        assert_eq!((c.keys_marked, c.entries_marked), (1, 2));
        let on_disk = std::fs::read_to_string(file_of(d.path(), "host-abcd1234")).unwrap();
        assert!(!on_disk.contains("text") && on_disk.contains("host-abcd1234/n1"), "ids and times only: {on_disk}");
    }

    #[test]
    fn a_mark_takes_identifiers_only_never_prose() {
        let d = ws();
        assert!(mark(d.path(), "host-abcd1234", "the vault key is hunter2", 1).is_err());
        assert!(mark(d.path(), "../escape", "x", 1).is_err());
        assert!(mark(d.path(), "", "x", 1).is_err());
        assert!(mark(d.path(), &"k".repeat(97), "x", 1).is_err());
        assert!(!dir(d.path()).exists(), "a refused mark writes nothing");
    }

    #[test]
    fn clearing_lifts_the_mark_for_what_was_read_and_a_new_entry_marks_again() {
        let d = ws();
        mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 1).unwrap();
        assert_eq!(clear(d.path(), "host-abcd1234", 10).unwrap(), 1);
        assert_eq!(state(d.path(), "host-abcd1234"), State::Unmarked);
        assert_eq!(clear(d.path(), "host-abcd1234", 11).unwrap(), 0, "nothing more to clear");
        mark(d.path(), "host-abcd1234", "host-abcd1234/n2", 20).unwrap();
        assert!(matches!(state(d.path(), "host-abcd1234"), State::Marked { ref entries, .. } if entries == &vec!["host-abcd1234/n2".to_string()]));
        assert_eq!(counter(d.path()).cleared, 2);
    }

    #[test]
    fn an_unreadable_mark_is_not_silently_unmarked() {
        let d = ws();
        mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 1).unwrap();
        std::fs::write(file_of(d.path(), "host-abcd1234"), b"{not json").unwrap();
        assert_eq!(state(d.path(), "host-abcd1234"), State::Unreadable);
        assert!(state(d.path(), "host-abcd1234").would_affect_a_send(), "an unreadable mark is counted as one a send would change");
        assert_eq!(state(d.path(), "bad key with spaces"), State::Unreadable, "a key that is not an identifier is never read as unmarked");
        observe_send(d.path(), "outbound", "host-abcd1234");
        let c = counter(d.path());
        assert_eq!((c.outbound_would_change, c.unreadable), (1, 1));
    }

    #[test]
    fn observing_a_send_counts_a_marked_task_and_never_an_unmarked_one() {
        let d = ws();
        observe_send(d.path(), "outbound", "t-1");
        observe_send(d.path(), "connector", "t-1");
        assert_eq!(counter(d.path()), Counter::default(), "an unmarked task changes nothing and writes nothing");
        assert!(!dir(d.path()).exists(), "observing an unmarked task creates no file");
        mark(d.path(), "t-1", "s-abc", 5).unwrap();
        observe_send(d.path(), "outbound", "t-1");
        observe_send(d.path(), "outbound", "t-1");
        observe_send(d.path(), "connector", "t-1");
        observe_send(d.path(), "relay", "t-1");
        let c = counter(d.path());
        assert_eq!((c.outbound_would_change, c.connector_would_change), (2, 1), "relay and mail are not observed in M1 (their seam is M3's)");
    }

    #[test]
    fn noted_entry_reads_only_the_daemons_own_staged_paths() {
        assert_eq!(noted_entry("staged/host-abcd1234/n7.md"), Some(("host-abcd1234".into(), "host-abcd1234/n7".into())));
        for bad in ["staged/host-abcd1234/../n7.md", "staged/mission1/n7.md", "core/x.md", "staged/host-ABCD1234/n7.md", "staged/host-abcd1234/notes.md", "staged/host-abc/n1.md", "staged/host-abcd1234/n.md"] {
            assert_eq!(noted_entry(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_status_line_is_counts_only() {
        let d = ws();
        mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 1).unwrap();
        let s = status_line(d.path());
        assert!(s.contains("1 task(s) marked") && s.contains("observe only") && !s.contains("host-abcd1234"), "{s}");
    }

    /// `specs/103` §6 `every_reader_of_private_text_is_in_the_reads_table`: a file that calls an accessor of entry text, or that the table
    /// names, must be in the table; every table row names a file that exists and, for `Withholds`, a proof test that exists.
    #[test]
    fn every_reader_of_private_text_is_in_the_reads_table() {
        let sources = engine_sources();
        for (file, text) in &sources {
            let prod = production(text);
            let reads_text = ACCESSORS.iter().any(|a| prod.contains(a));
            let listed = READS.iter().any(|r| r.file == file);
            // the accessors' own definitions and the tests of this module are not readers
            if file == "private_read.rs" {
                continue;
            }
            if reads_text {
                assert!(listed, "{file} reads the text of an entry (one of {ACCESSORS:?}) and is not in READS (specs/103 §2): add it with what it does about a private entry");
            }
        }
        for r in READS.iter() {
            assert!(sources.iter().any(|(f, _)| f == r.file), "READS names {} which is not a file of this crate", r.file);
            if let How::Withholds { proof_file, proof_test } = r.how {
                let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
                let t = std::fs::read_to_string(crates.join(proof_file)).unwrap_or_else(|e| panic!("{proof_file}: {e}"));
                assert!(t.contains(&format!("fn {proof_test}(")), "the proof test `{proof_test}` is missing from {proof_file}");
            }
        }
        assert!(READS.iter().filter(|r| r.how == How::Marks).count() >= 1, "at least the host's search marks");
    }

    #[test]
    fn a_claim_that_rests_on_a_private_source_is_withheld_from_the_seat_that_opens_it() {
        let lib = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/library_claims.rs")).unwrap();
        let body = production(&lib);
        let at = body.find("pub fn claim_slice").expect("claim_slice exists");
        let slice = &body[at..body[at..].find("\n    }\n").map(|n| at + n).unwrap_or(body.len())];
        assert!(slice.contains("rests_on_private") && slice.contains("text withheld"), "claim_slice withholds the text of a claim that rests on a private source");
    }

    /// `specs/103` §4.3 and §7.1 OD-4: only the daemon's console-scope method clears a mark. The delegated form (an operator permission an
    /// agent holds) is a decision before M2 and does not exist here.
    #[test]
    fn only_the_operators_console_can_clear_a_mark() {
        let mut callers: Vec<String> = Vec::new();
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    if !matches!(p.file_name().and_then(|n| n.to_str()), Some("target" | "node_modules" | "tests" | "fixtures")) {
                        walk(&p, out);
                    }
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(&crates, &mut files);
        for f in files {
            let rel = f.strip_prefix(&crates).unwrap().to_string_lossy().replace('\\', "/");
            let text = std::fs::read_to_string(&f).unwrap();
            let prod = production(&text);
            if rel != "delphi-engine/src/private_read.rs" && (prod.contains("private_read::clear(") || prod.contains("private_read::CLEAR_EVENT")) {
                callers.push(rel);
            }
        }
        assert_eq!(callers, vec!["delphi-engine/src/service.rs".to_string()], "the clear function has one caller, the daemon's console method");
        let svc = std::fs::read_to_string(crates.join("delphi-engine/src/service.rs")).unwrap();
        let at = svc.find("async fn private_read_clear").expect("the console method exists");
        let body = &svc[at..svc[at..].find("\n    }\n").map(|n| at + n).unwrap_or(svc.len())];
        assert!(body.contains("need_console(&ctx)"), "the clear method is console scope only");
    }

    /// The mark module reaches no network, no process and no model, and calls no promotion, trust, tool or approval path.
    #[test]
    fn the_mark_module_reaches_nothing_and_grants_nothing() {
        let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/private_read.rs")).unwrap();
        let code = production(&src);
        for needle in ["TcpStream", "reqwest", "Command::new", "std::process", "hyper::", "ureq", "promote", "approve", "grant(", "trust("] {
            let hits = code.lines().filter(|l| !l.trim_start().starts_with("//") && l.contains(needle)).count();
            assert_eq!(hits, 0, "private_read.rs names `{needle}`");
        }
    }
}
