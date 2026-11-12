//! The private-read mark (`specs/103`). A task (for a host over MCP, the host session) that was handed the text of a private entry is
//! *marked*. The mark is computed, logged, counted and clearable at the console. It changes nothing that flows IN: no read returns a
//! different result, no prompt differs, and no gate or filter is added to anything a task reads (the operator, 2026-11-11: research must
//! be easier to find, not harder).
//!
//! * Slice M1 built the mark and its counter. **Slice M2** (this file's [`send_record`], [`task_key`], [`note_held`]) makes two of the four
//!   channels act on it: the senders of `Outbound` (a `WebFetch`/`WebSearch` request) and `Connector` (a connector call) hand the task's
//!   record to the one egress check, and when the check refuses, the send is not made silently and not refused outright: it becomes an
//!   operator card that shows the exact text, is Red, never takes "allow similar", and is approved only at the console (`specs/103` §7.1
//!   OD-1, OD-2). Nothing is sent before an answer, and a timeout is a denial. Relay and Mail are `M3`'s.
//! * The mark is not a tier and not a taint level and grants nothing (GOVERNANCE §4). It only ever raises a card or asks; it cannot lower
//!   a tier, approve an action or grant a tool. It records entry ids and a time, never text.
//! * It is set only by the readers listed in [`READS`], through [`mark`]. An inventory test fails when a file that reads entry text is not
//!   listed, so a new reader cannot ship unrecorded.
//! * It is cleared only by [`clear`], whose one caller is the daemon's console-scope method `private_read.clear`. A test enumerates the
//!   callers; no tool, no host method, no relay reply and no handoff reaches it (`specs/103` §4.3, §7.1 OD-4: the delegated form, an agent
//!   holding an operator permission, is a separate decision and is NOT built).
//! * An unreadable mark file reads as [`State::Unreadable`]. On Outbound and Connector, and only there, it fails closed: the send asks
//!   the operator exactly as for a marked task.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};


/// The log event written when a task first reads an entry it had not read before (ids and time only).
pub const MARK_EVENT: &str = "task.private_read";
/// The log event written when the operator clears a mark at the console.
pub const CLEAR_EVENT: &str = "task.private_read_cleared";

/// The approval card tool name of a held Outbound request. `merge_class::console_only` names it, so a relay reply can deny but never approve it.
pub const OUTBOUND_TOOL: &str = "harness_outbound_marked";
/// What every held card says first, in fixed words (it names no entry and carries no text from one).
pub const HELD_BANNER: &str = "PRIVATE-READ MARK: this task read a private entry (or its mark cannot be read), so what is sent below could carry it out of this install. Nothing is sent until you answer at the console; a relay reply can only say no to it.";
/// How much of a held request is written on the card's summary line (the payload carries the whole of it).
const EXACT_CAP: usize = 1500;

/// The exact text of a held request for the card: the request as JSON, cut at [`EXACT_CAP`] characters with a plain marker. The approval
/// is bound to the whole payload's hash, so nothing is hidden by the cut; it only bounds one line.
pub fn exact(v: &Value) -> String {
    let t = v.to_string();
    if t.chars().count() <= EXACT_CAP {
        t
    } else {
        format!("{}… [cut at {} characters; the whole request is in the payload]", t.chars().take(EXACT_CAP).collect::<String>(), EXACT_CAP)
    }
}

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
    /// A record exists and cannot be read. Outbound and Connector (M2) treat the task as marked; no other path reads this.
    Unreadable,
}

impl State {
    /// Is a send on Outbound or Connector held for the operator's answer in this state? (Relay and Mail are M3's.)
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
    /// Sends on Outbound that the mark held for the operator's answer (M1 counted what it would have changed; since M2 the send really asks).
    pub outbound_would_change: u64,
    /// Sends on Connector that the mark held for the operator's answer.
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

/// The key a Delphi task's mark is kept under: `<mission>/<task>`, because task ids (`t_0001`) repeat across missions and a mark in one must
/// never hold a send in another. An id with a character a key cannot hold is replaced by a hash of the pair, so a task is never made
/// unreadable (and so never asked about) just for how it was named. The readers that mark a Delphi task use the same function.
pub fn task_key(mission: &str, task: &str) -> String {
    let k = format!("{mission}/{task}");
    if key_ok(&k) {
        k
    } else {
        format!("x:{}", &blake3::hash(format!("{mission}\u{0}{task}").as_bytes()).to_hex()[..32])
    }
}

/// The record a send of the task `key` is checked against (`specs/103` §4, `egress::check`): `{"private": false}` when nothing is marked
/// (the check passes and the send is exactly as before), `{"private": true}` when the task read a private entry, and a value that is not
/// an object when the mark cannot be read, which the check refuses (deny by default). Reads the mark; writes nothing.
pub fn send_record(ws_dir: &Path, key: &str) -> Value {
    match state(ws_dir, key) {
        State::Unmarked => json!({"private": false}),
        State::Marked { .. } => json!({"private": true}),
        State::Unreadable => Value::Null,
    }
}

/// The send on `channel` (`"outbound"` or `"connector"`) of the task `key` was refused by the egress check and is being put to the
/// operator instead. Count it. Called only by the two senders, only after the check refused; it decides nothing.
pub fn note_held(ws_dir: &Path, channel: &str, key: &str) {
    let st = state(ws_dir, key);
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
        "private-read mark: {} task(s) marked, {} entr(y/ies) read, {} cleared; sends held for your answer: outbound {}, connector {} ({} unreadable); {} reader(s) do not mark yet",
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

// ---- slice M3 (`specs/103` §4.2): what a relay and a mailbox are sent --------------------------------------------------------------------
//
// The daemon, before an event reaches a relay client, replaces the text an agent wrote when the event's task read a private entry. A relay
// client (the Telegram bot, the mailbox) is another process: it never receives what the daemon withheld, so it cannot leak it. Mail is the
// relay's SMTP transport, fed by the same stream, so one seam covers both rows. The decision goes through the one egress check
// ([`crate::egress::check`]) with a record shaped from the mark, so "unreadable" is private by the check's own deny-by-default rule. Nothing
// here changes what flows in, grants anything, or is a tier; a mark only degrades a message.

/// The one phrase a relay or mailbox is sent in place of agent-written text.
pub const RELAY_PHRASE: &str = "[withheld: this task read a private entry; see it at the console]";

/// The relay event kinds whose payload holds text an agent wrote. Every other kind carries ids, words the daemon chose, or fixed slugs.
const AGENT_TEXT_KINDS: [&str; 4] = ["approval.requested", "decisions.review_requested", "task.completed", "task.failed"];

/// The cards whose details the stream withheld, newest last. In memory only: a restarted daemon parks its cards and raises them again, and
/// each passes the filter again. It lets the card list and the relay's answer agree with the stream when the mark was on the mission and
/// not the task (a card record names its task, not its mission).
static MASKED_CARDS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
const MASKED_CARDS_CAP: usize = 2000;

fn note_masked_card(approval_id: &str) {
    let mut q = MASKED_CARDS.lock().unwrap_or_else(|p| p.into_inner());
    if !q.iter().any(|c| c == approval_id) {
        if q.len() >= MASKED_CARDS_CAP {
            q.pop_front();
        }
        q.push_back(approval_id.to_string());
    }
}

fn card_is_masked(approval_id: &str) -> bool {
    MASKED_CARDS.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|c| c == approval_id)
}

/// The record the egress check reads for a mark: public when nothing is marked, private when something is, and not a record at all (so
/// private by the check's deny-by-default rule) when the mark cannot be read.
fn record_for(st: &State) -> Value {
    match st {
        State::Unmarked => json!({}),
        State::Marked { .. } => json!({"private": true}),
        State::Unreadable => Value::Null,
    }
}

/// May text an agent wrote leave through `channel` for a task in this state? One question put to the one check.
fn text_can_go(st: &State, channel: crate::library::Channel) -> bool {
    crate::egress::check(&record_for(st), channel).is_ok()
}

/// May agent-written text go on the relay stream, which feeds the Telegram bot (`Relay`) and the mailbox (`Mail`)?
pub fn stream_text_can_go(st: &State) -> bool {
    text_can_go(st, crate::library::Channel::Relay) && text_can_go(st, crate::library::Channel::Mail)
}

/// Put the relay filter on `hub` for the workspace at `ws_dir`. Called once when an engine is built (the daemon's boot, and the test
/// fixture that stands in for it), so every event a relay is sent passes [`mask_event_for_relay`].
pub fn install_relay_filter(hub: &delphi_core::hub::Hub, ws_dir: &Path) {
    let ws = ws_dir.to_path_buf();
    hub.set_relay_filter(std::sync::Arc::new(move |e: &Value| mask_event_for_relay(&ws, e)));
}

/// The state of the keys an event names: its task and its mission. A key that is not a valid identifier cannot have been marked and is
/// skipped; a record that exists and cannot be read makes the whole answer `Unreadable`.
pub fn event_state(ws_dir: &Path, event: &Value) -> State {
    let task = event["task_id"].as_str().or_else(|| event["payload"]["task_id"].as_str());
    let mission = event["mission_id"].as_str();
    let mut found = State::Unmarked;
    // A Delphi task's mark is kept under `<mission>/<task>` (`task_key`, `specs/103` M2); the bare task id and the bare mission id are the
    // keys the host session and an older mark use. All of them count, so a mark under any key withholds the event.
    let mut keys: Vec<String> = [task, mission].into_iter().flatten().filter(|k| key_ok(k)).map(str::to_string).collect();
    if let (Some(m), Some(t)) = (mission, task) {
        keys.push(task_key(m, t));
    }
    for key in &keys {
        match state(ws_dir, key) {
            State::Unreadable => return State::Unreadable,
            m @ State::Marked { .. } => found = m,
            State::Unmarked => {}
        }
    }
    if mission.is_none() {
        // No mission named: any mission's mark for this task id counts (it can only withhold more, never less).
        if let Some(t) = task {
            match task_state_in_any_mission(ws_dir, t) {
                State::Unreadable => return State::Unreadable,
                m @ State::Marked { .. } => found = m,
                State::Unmarked => {}
            }
        }
    }
    found
}

/// The state of a task named without its mission (a card on the relay's list carries only the task id): every live mark whose key is
/// `<mission>/<task>`, in any mission. Task ids repeat across missions, so this can withhold a card of an unmarked task that shares an id
/// with a marked one; it can never let a marked task's text go. A record that exists and cannot be read makes the answer `Unreadable`.
fn task_state_in_any_mission(ws_dir: &Path, task: &str) -> State {
    if !key_ok(task) {
        return State::Unmarked;
    }
    let suffix = format!("/{task}");
    let mut found = State::Unmarked;
    for (key, _, _) in live(ws_dir) {
        if !key.ends_with(&suffix) {
            continue;
        }
        match state(ws_dir, &key) {
            State::Unreadable => return State::Unreadable,
            m @ State::Marked { .. } => found = m,
            State::Unmarked => {}
        }
    }
    found
}

/// Is a card's text withheld from a relay? True when the stream withheld it, or when its task is marked or its mark cannot be read.
pub fn card_withheld(ws_dir: &Path, approval_id: &str, task_id: &str) -> bool {
    if card_is_masked(approval_id) {
        return true;
    }
    let st = if key_ok(task_id) {
        // The bare key (host session, older marks) and any mission's `<mission>/<task>` mark: the card names no mission.
        match state(ws_dir, task_id) {
            State::Unmarked => task_state_in_any_mission(ws_dir, task_id),
            other => other,
        }
    } else {
        State::Unmarked
    };
    !stream_text_can_go(&st)
}

fn replace_text(v: &mut Value, field: &str) {
    if v.get(field).is_some_and(|x| !x.is_null()) {
        v[field] = json!(RELAY_PHRASE);
    }
}

/// The event a relay is sent in place of `event`, or `None` when it goes out as written. Only the agent-written fields of the four kinds in
/// `AGENT_TEXT_KINDS` change; ids, tier, tool name, payload hash, task id and status stay, so a card still reaches the operator.
pub fn mask_event_for_relay(ws_dir: &Path, event: &Value) -> Option<Value> {
    let kind = event["type"].as_str()?;
    if !AGENT_TEXT_KINDS.contains(&kind) {
        return None;
    }
    if stream_text_can_go(&event_state(ws_dir, event)) {
        return None;
    }
    let mut e = event.clone();
    let p = &mut e["payload"];
    if !p.is_object() {
        return Some(e);
    }
    match kind {
        "approval.requested" => {
            for f in ["summary", "payload", "pattern"] {
                replace_text(p, f);
            }
            p["withheld"] = json!(true);
            if let Some(id) = p["approval_id"].as_str().map(str::to_string) {
                note_masked_card(&id);
            }
        }
        "decisions.review_requested" => {
            for d in p["decisions"].as_array_mut().into_iter().flatten() {
                for f in ["summary", "rationale"] {
                    replace_text(d, f);
                }
            }
        }
        "task.completed" => {
            if p["reason"].as_str().is_some_and(|r| !r.is_empty()) {
                p["reason"] = json!(RELAY_PHRASE);
            }
        }
        "task.failed" => replace_text(p, "error"),
        _ => {}
    }
    Some(e)
}

/// The card list a relay is sent (`approval.list`): the same replacement, for each pending card whose details the stream withheld or whose
/// task is marked or unreadable.
pub fn mask_pending_for_relay(ws_dir: &Path, mut list: Value) -> Value {
    for card in list.as_array_mut().into_iter().flatten() {
        let (id, task) = (card["approval_id"].as_str().unwrap_or("").to_string(), card["task_id"].as_str().unwrap_or("").to_string());
        if card.is_object() && card_withheld(ws_dir, &id, &task) {
            for f in ["summary", "payload", "pattern"] {
                replace_text(card, f);
            }
            card["withheld"] = json!(true);
            note_masked_card(&id);
        }
    }
    list
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

    /// `specs/103` §6 `a_task_that_read_nothing_private_is_unchanged_on_all_four_channels`: with no mark the record a send is checked against is
    /// public, the one egress check passes on every channel, and no file, counter or event is written. The two senders that act on a mark (M2)
    /// do so only when that check refuses; Relay and Mail do not read the mark at all until M3.
    #[test]
    fn a_task_that_read_nothing_private_is_unchanged_on_all_four_channels() {
        use crate::library::Channel;
        let d = ws();
        let rec = send_record(d.path(), &task_key("m-1", "t_0001"));
        assert_eq!(rec, json!({"private": false}));
        for ch in [Channel::Outbound, Channel::Connector, Channel::Relay, Channel::Mail] {
            assert!(crate::egress::check(&rec, ch).is_ok(), "{ch:?}");
        }
        assert!(!dir(d.path()).exists(), "looking at an unmarked task creates nothing");
        // each acting sender passes the task's record to the one check, for its own channel, and only a refusal reaches the operator
        for (name, channel) in [("harness_run.rs", "Channel::Outbound"), ("connector_run.rs", "Channel::Connector")] {
            let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name)).unwrap();
            let prod = production(&src);
            let call = format!("crate::egress::check(&crate::private_read::send_record(&self.ws.dir, &mark_key), crate::library::{channel})");
            assert_eq!(prod.matches(&call).count(), 1, "{name} checks the task's mark once, through the one egress check");
            assert_eq!(prod.matches("crate::private_read::note_held(").count(), 1, "{name} counts a held send in one place");
            assert!(!prod.contains("observe_send"), "{name}: the M1 observe call is gone");
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
        assert!(state(d.path(), "host-abcd1234").would_affect_a_send(), "an unreadable mark holds a send on Outbound and Connector");
        assert_eq!(state(d.path(), "bad key with spaces"), State::Unreadable, "a key that is not an identifier is never read as unmarked");
        assert!(send_record(d.path(), "host-abcd1234").is_null(), "an unreadable mark is a record the egress check refuses (deny by default)");
        note_held(d.path(), "outbound", "host-abcd1234");
        let c = counter(d.path());
        assert_eq!((c.outbound_would_change, c.unreadable), (1, 1));
    }

    /// `send_record` is what the egress check reads: public when unmarked or fully cleared, private when marked, unreadable when damaged.
    #[test]
    fn the_record_a_send_is_checked_against_follows_the_mark() {
        use crate::library::Channel;
        let d = ws();
        let key = task_key("m-1", "t_0001");
        assert_eq!(send_record(d.path(), &key), json!({"private": false}));
        mark(d.path(), &key, "s-abc", 5).unwrap();
        let rec = send_record(d.path(), &key);
        assert_eq!(rec, json!({"private": true}));
        for ch in [Channel::Outbound, Channel::Connector] {
            assert!(crate::egress::check(&rec, ch).is_err(), "{ch:?}");
        }
        clear(d.path(), &key, 6).unwrap();
        assert_eq!(send_record(d.path(), &key), json!({"private": false}), "cleared at the console, the next send is as before");
        std::fs::write(file_of(d.path(), &key), b"{not json").unwrap();
        assert!(send_record(d.path(), &key).is_null());
        assert!(crate::egress::check(&send_record(d.path(), &key), Channel::Outbound).is_err());
    }

    /// Task ids repeat across missions, so the key carries the mission; an id a key cannot hold is hashed, not made unreadable.
    #[test]
    fn a_tasks_key_names_its_mission_and_a_name_a_key_cannot_hold_is_hashed_not_unreadable() {
        let d = ws();
        assert_ne!(task_key("m-1", "t_0001"), task_key("m-2", "t_0001"));
        assert_eq!(task_key("m-1", "t_0001"), "m-1/t_0001");
        mark(d.path(), &task_key("m-1", "t_0001"), "s-abc", 5).unwrap();
        assert_eq!(send_record(d.path(), &task_key("m-2", "t_0001")), json!({"private": false}), "another mission's task of the same id is not held");
        let long = "m".repeat(120);
        for odd in ["a mission with spaces", "\u{fc}ber", "x.y", long.as_str()] {
            let k = task_key(odd, "t_0001");
            assert!(key_ok(&k), "{odd}: {k}");
            assert!(k.starts_with("x:") && k.len() == 34, "{k}");
            assert_eq!(state(d.path(), &k), State::Unmarked, "an oddly named task is unmarked, not unreadable");
            assert_ne!(k, task_key(odd, "t_0002"));
        }
    }

    /// A held send is counted per channel; the counter changes nothing else, and an unknown channel is ignored.
    #[test]
    fn a_held_send_is_counted_per_channel() {
        let d = ws();
        mark(d.path(), "t-1", "s-abc", 5).unwrap();
        note_held(d.path(), "outbound", "t-1");
        note_held(d.path(), "outbound", "t-1");
        note_held(d.path(), "connector", "t-1");
        note_held(d.path(), "relay", "t-1");
        let c = counter(d.path());
        assert_eq!((c.outbound_would_change, c.connector_would_change, c.unreadable), (2, 1, 0), "relay and mail are not held in M2 (their seam is M3's)");
    }

    /// The card's one line of exact text is bounded and says so when it is cut.
    #[test]
    fn the_exact_text_on_a_held_card_is_bounded_and_says_when_it_is_cut() {
        let short = json!({"query": "a b c"});
        assert_eq!(exact(&short), short.to_string());
        let long = json!({"query": "x".repeat(4000)});
        let t = exact(&long);
        assert!(t.chars().count() < 1700 && t.contains("cut at 1500 characters"), "{}", t.chars().count());
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
        assert!(s.contains("1 task(s) marked") && s.contains("sends held for your answer") && !s.contains("observe only") && !s.contains("host-abcd1234"), "{s}");
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

    // ---- slice M3: what a relay and a mailbox are sent ---------------------------------------------------------------------------------

    const CANARY: &str = "CANARY-private-words-4f1c";

    fn card_event(task: &str, id: &str) -> Value {
        json!({"type": "approval.requested", "mission_id": "m_1", "task_id": task, "payload": {"approval_id": id, "task_id": task, "role": "architect", "tier": "amber", "tool": "write_file",
               "pattern": format!("write_file {CANARY}"), "summary": format!("write {CANARY}.md"), "payload": {"path": format!("{CANARY}.md"), "content": CANARY},
               "payload_blake3": "abcdef0123456789abcdef", "timeout_secs": 600}})
    }

    fn review_event(task: &str) -> Value {
        json!({"type": "decisions.review_requested", "mission_id": "m_1", "task_id": task, "payload": {"count": 1, "decisions": [{"decision_id": "dec_a", "risk": "routine", "summary": format!("edit {CANARY}"), "rationale": format!("because {CANARY}")}]}})
    }

    fn done_event(task: &str, reason: &str) -> Value {
        json!({"type": "task.completed", "mission_id": "m_1", "task_id": task, "payload": {"status": "done", "reason": reason}})
    }

    fn failed_event(task: &str) -> Value {
        json!({"type": "task.failed", "mission_id": "m_1", "task_id": task, "payload": {"error": format!("could not read {CANARY}")}})
    }

    /// `specs/103` §4.2: a task that read a private entry has the text an agent wrote replaced by one fixed phrase, in all four kinds; the ids,
    /// tier, tool name, payload hash, status and counts stay, so a card still reaches the operator.
    #[test]
    fn an_event_of_a_marked_task_loses_its_agent_written_fields_and_keeps_its_ids() {
        let d = ws();
        mark(d.path(), "t_0001", "entry-1", 100).unwrap();
        for ev in [card_event("t_0001", "ap_m3_a"), review_event("t_0001"), done_event("t_0001", &format!("done with {CANARY}")), failed_event("t_0001")] {
            let m = mask_event_for_relay(d.path(), &ev).unwrap_or_else(|| panic!("{} was not masked", ev["type"]));
            let text = m.to_string();
            assert!(!text.contains(CANARY), "{}: agent-written text reached the relay: {text}", ev["type"]);
            assert!(text.contains(RELAY_PHRASE), "{}: the fixed phrase is missing: {text}", ev["type"]);
            assert_eq!(m["type"], ev["type"]);
            assert_eq!(m["task_id"], ev["task_id"]);
        }
        let card = mask_event_for_relay(d.path(), &card_event("t_0001", "ap_m3_b")).unwrap();
        let p = &card["payload"];
        assert_eq!((p["approval_id"].as_str(), p["payload_blake3"].as_str(), p["tier"].as_str(), p["tool"].as_str(), p["role"].as_str(), p["timeout_secs"].as_u64()), (Some("ap_m3_b"), Some("abcdef0123456789abcdef"), Some("amber"), Some("write_file"), Some("architect"), Some(600)));
        assert_eq!((p["summary"].as_str(), p["payload"].as_str(), p["pattern"].as_str(), p["withheld"].as_bool()), (Some(RELAY_PHRASE), Some(RELAY_PHRASE), Some(RELAY_PHRASE), Some(true)));
        let rev = mask_event_for_relay(d.path(), &review_event("t_0001")).unwrap();
        let dec = &rev["payload"]["decisions"][0];
        assert_eq!((rev["payload"]["count"].as_u64(), dec["decision_id"].as_str(), dec["risk"].as_str(), dec["summary"].as_str(), dec["rationale"].as_str()), (Some(1), Some("dec_a"), Some("routine"), Some(RELAY_PHRASE), Some(RELAY_PHRASE)));
        let done = mask_event_for_relay(d.path(), &done_event("t_0001", "some reason")).unwrap();
        assert_eq!((done["payload"]["status"].as_str(), done["payload"]["reason"].as_str()), (Some("done"), Some(RELAY_PHRASE)));
        // an empty reason has nothing to withhold and is not turned into the phrase
        assert_eq!(mask_event_for_relay(d.path(), &done_event("t_0001", "")).unwrap()["payload"]["reason"], "");
    }

    /// `specs/103` M2 keys a Delphi task's mark `<mission>/<task>` (`task_key`), because task ids repeat across missions. The relay filter must
    /// find a mark under that key, or a task a reader marked would have its text sent to the relay: a mark in one mission holds that
    /// mission's task, a card that names no mission is held when any mission's task of that id is marked, and another mission's task of the
    /// same id (an event that names its mission) is not touched.
    #[test]
    fn a_mark_under_the_mission_and_task_key_withholds_the_relay_text_and_another_missions_task_is_untouched() {
        let d = ws();
        mark(d.path(), &task_key("m_9", "t_0001"), "entry-1", 100).unwrap();
        let mut in_m9 = card_event("t_0001", "ap_key_a");
        in_m9["mission_id"] = json!("m_9");
        let masked = mask_event_for_relay(d.path(), &in_m9).expect("a mark under <mission>/<task> must withhold that task's card");
        assert!(masked.to_string().contains(RELAY_PHRASE) && !masked.to_string().contains(CANARY), "{masked}");
        let mut no_mission = done_event("t_0001", &format!("done with {CANARY}"));
        no_mission.as_object_mut().unwrap().remove("mission_id");
        let masked = mask_event_for_relay(d.path(), &no_mission).expect("an event naming no mission is withheld when any mission's task of that id is marked");
        assert!(!masked.to_string().contains(CANARY), "{masked}");
        assert!(card_withheld(d.path(), "ap_none", "t_0001"), "a relay card list entry names only the task id and is withheld");
        let mut in_m2 = card_event("t_0001", "ap_key_b");
        in_m2["mission_id"] = json!("m_2");
        assert!(mask_event_for_relay(d.path(), &in_m2).is_none(), "the same task id in another mission is not marked");
        assert!(!card_withheld(d.path(), "ap_other", "t_0002"), "another task is not withheld");
    }

    /// `specs/103` §6 `a_task_that_read_nothing_private_is_unchanged_on_all_four_channels`, the Relay and Mail rows: with no mark the filter
    /// returns nothing to substitute for any event, and a marked task's events of kinds that carry no agent text are not touched either.
    #[test]
    fn a_task_that_read_nothing_private_is_sent_exactly_what_it_always_was_and_so_are_the_kinds_with_no_agent_text() {
        let d = ws();
        for ev in [card_event("t_0001", "ap_m3_c"), review_event("t_0001"), done_event("t_0001", "fine"), failed_event("t_0001")] {
            assert!(mask_event_for_relay(d.path(), &ev).is_none(), "{} changed with no mark", ev["type"]);
        }
        assert!(!dir(d.path()).exists(), "looking at an event writes nothing");
        mark(d.path(), "t_0001", "entry-1", 100).unwrap();
        for kind in ["approval.resolved", "approval.parked", "xenia.alert", "task.output", "anything.else"] {
            let ev = json!({"type": kind, "mission_id": "m_1", "task_id": "t_0001", "payload": {"reason": "stale", "by": "console-1"}});
            assert!(mask_event_for_relay(d.path(), &ev).is_none(), "{kind} carries no agent-written text and is sent as written");
        }
        // a different task is not the marked one
        assert!(mask_event_for_relay(d.path(), &card_event("t_0002", "ap_m3_d")).is_none());
        // and the same event twice gives the same answer
        assert_eq!(mask_event_for_relay(d.path(), &card_event("t_0001", "ap_m3_e")), mask_event_for_relay(d.path(), &card_event("t_0001", "ap_m3_e")));
    }

    #[test]
    fn a_mark_on_the_host_mission_withholds_its_events_too_and_clearing_lifts_it() {
        let d = ws();
        mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 100).unwrap();
        let mut ev = card_event("t_0009", "ap_m3_f");
        ev["mission_id"] = json!("host-abcd1234");
        assert!(mask_event_for_relay(d.path(), &ev).is_some(), "the unit for a host over MCP is its session's mission");
        clear(d.path(), "host-abcd1234", 200).unwrap();
        assert!(mask_event_for_relay(d.path(), &ev).is_none(), "the operator's clear lifts it");
    }

    /// `specs/103` §4.3: a mark that cannot be read is treated as marked, on the stream only.
    #[test]
    fn an_unreadable_mark_withholds_on_the_stream_and_a_stranger_task_is_not_affected() {
        let d = ws();
        std::fs::create_dir_all(dir(d.path())).unwrap();
        std::fs::write(file_of(d.path(), "t_0001"), "{ not json").unwrap();
        let m = mask_event_for_relay(d.path(), &card_event("t_0001", "ap_m3_g")).expect("an unreadable mark fails closed");
        assert!(!m.to_string().contains(CANARY));
        assert!(mask_event_for_relay(d.path(), &card_event("t_0002", "ap_m3_h")).is_none(), "another task has no record and is unmarked");
        assert!(!stream_text_can_go(&State::Unreadable) && !stream_text_can_go(&State::Marked { entries: vec!["e".into()], first_at: 1 }) && stream_text_can_go(&State::Unmarked));
    }

    /// The card list a relay asks for (`approval.list`) agrees with the stream: a marked task's cards, and any card the stream withheld
    /// (the mark may have been on the mission, which a card record does not name), come back with the phrase and no details.
    #[test]
    fn the_card_list_a_relay_is_sent_agrees_with_the_stream() {
        let d = ws();
        mark(d.path(), "t_0001", "entry-1", 100).unwrap();
        mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 100).unwrap();
        let rec = |id: &str, task: &str| json!({"approval_id": id, "task_id": task, "tier": "amber", "tool": "write_file", "pattern": "p", "summary": CANARY, "payload": {"content": CANARY}, "payload_blake3": "ab"});
        // the stream withheld this one because its mission was marked; its task key is not
        let mut ev = card_event("t_0005", "ap_m3_list_3");
        ev["mission_id"] = json!("host-abcd1234");
        assert!(mask_event_for_relay(d.path(), &ev).is_some());
        let out = mask_pending_for_relay(d.path(), json!([rec("ap_m3_list_1", "t_0001"), rec("ap_m3_list_2", "t_0002"), rec("ap_m3_list_3", "t_0005")]));
        let l = out.as_array().unwrap();
        assert_eq!((l[0]["summary"].as_str(), l[0]["payload"].as_str(), l[0]["withheld"].as_bool()), (Some(RELAY_PHRASE), Some(RELAY_PHRASE), Some(true)));
        assert_eq!((l[1]["summary"].as_str(), l[1]["payload"]["content"].as_str(), l[1].get("withheld")), (Some(CANARY), Some(CANARY), None), "an unmarked task's card is the list as it always was");
        assert_eq!(l[2]["summary"].as_str(), Some(RELAY_PHRASE), "the list agrees with what the stream withheld");
        assert!(card_withheld(d.path(), "ap_m3_list_3", "t_0005") && !card_withheld(d.path(), "ap_m3_list_2", "t_0002"));
        assert_eq!(l.iter().map(|c| c["approval_id"].as_str().unwrap()).collect::<Vec<_>>(), ["ap_m3_list_1", "ap_m3_list_2", "ap_m3_list_3"], "ids and order stay");
    }

    /// The decision is put to the one egress check with a record shaped from the mark, and the relay filter is installed in one place, by the
    /// boot (and the test fixture that stands in for it).
    #[test]
    fn the_stream_decision_goes_through_the_one_egress_check_and_the_filter_is_installed_once_at_boot() {
        let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/private_read.rs")).unwrap();
        let code = production(&src);
        assert!(code.contains("egress::check(") && code.contains("crate::library::Channel::Relay") && code.contains("crate::library::Channel::Mail"), "Relay and Mail are decided by egress::check");
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let (mut setters, mut installers): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        for (rel, text) in engine_sources().into_iter().map(|(f, t)| (format!("delphi-engine/src/{f}"), t)).chain(["delphi-core/src/server.rs", "delphi/src/main.rs"].map(|f| (f.to_string(), std::fs::read_to_string(crates.join(f)).unwrap_or_default()))) {
            let prod = production(&text);
            if prod.contains(".set_relay_filter(") {
                setters.push(rel.clone());
            }
            if prod.contains("private_read::install_relay_filter(") {
                installers.push(rel);
            }
        }
        setters.sort();
        installers.sort();
        assert_eq!(setters, vec!["delphi-engine/src/private_read.rs".to_string()], "the hub's filter is set by the one install function and nothing else");
        assert_eq!(installers, vec!["delphi-engine/src/boot.rs".to_string(), "delphi-engine/src/testkit.rs".to_string()], "the filter is installed when an engine is built: by the boot, and by the test fixture that stands in for it");
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
