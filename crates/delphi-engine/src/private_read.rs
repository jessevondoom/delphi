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
    /// The reader returns text only to the operator's own console (a method that needs the console scope, or a command the operator
    /// runs), never to a model seat or an MCP caller. `why` names the check that keeps it so.
    OperatorOnly { why: &'static str },
    /// SKIPS THE MARK: a real gap. The reader can hand a model or a host words that came from a private entry and does not mark. `exposure`
    /// says what leaks and what the operator should know. Found by the red-team pass (`specs/103` M4); it is not fixed because the fix is a
    /// decision (a child or another session inheriting a mark, OD-5), not a bug.
    Skips { exposure: &'static str },
}

/// One file that touches the text of a private entry and what it does about it. The file is relative to this crate's `src`.
pub struct Read {
    pub file: &'static str,
    pub what: &'static str,
    pub how: How,
}

/// Every file of this crate that returns, or can return, the text of a private entry. The inventory test below checks the table against
/// the accessors it scans for; a reader of private text that is not here fails it.
pub const READS: [Read; 21] = [
    Read { file: "service.rs", what: "`host.search_kb`: the `noted` layer returns entries a host left, with the preview; one the daemon stored private is private text handed to the host. (The same file's `kb.search` and `kb.get` serve the console and the narrator pane with `include_staged`: the operator's own view, not a seat; the per-method tables `HOST_SURFACE` and `MCP_SURFACE` name every method a host can call)", how: How::Marks },
    Read { file: "library_claims.rs", what: "`claim_slice`: opening `@library/claim/<id>`; also reads `source.md` and `excerpts` to check an anchor or a support count, which return counts and verdicts and no text", how: How::Withholds { proof_file: "delphi-engine/src/private_read.rs", proof_test: "a_claim_that_rests_on_a_private_source_is_withheld_from_the_seat_that_opens_it" } },
    Read { file: "engine.rs", what: "block 5 of a span: opens a claim pointer through `claim_slice` (above)", how: How::Withholds { proof_file: "delphi-engine/src/private_read.rs", proof_test: "a_claim_that_rests_on_a_private_source_is_withheld_from_the_seat_that_opens_it" } },
    Read { file: "library_page_search.rs", what: "a page search in the internal scope reads private sources into the page writer", how: How::NotYetMarked { why: "the writer is a model with no tools and no path to the four channels; the page is a library record, not a send. An external page leaves private sources out" } },
    Read { file: "library_page_check.rs", what: "the page check reads source text and excerpts to verify quotes (internal scope includes private sources)", how: How::NotYetMarked { why: "the checker reads text to compare it and returns verdicts, not text; an external page refuses a private source" } },
    Read { file: "library_page_validate.rs", what: "the page validator reads source text", how: How::NotYetMarked { why: "a validator seat with no tools; it returns a verdict" } },
    Read { file: "library_pages.rs", what: "writes the `Noted, not validated` section of a page", how: How::NoReturn { why: "it composes a library page from entries that are already stored; it is not a reader of private text for a caller" } },
    Read { file: "host_noted.rs", what: "stores a host's entry, deciding `private`", how: How::NoReturn { why: "it stores and labels; it returns no entry text" } },
    // ---- slice M4 (the red-team pass): the files the wider scan found ----
    Read { file: "library.rs", what: "captures and recaptures a source: reads the previous `source.md` and `excerpts.json` to compute a change note (sizes and heading names) and to carry excerpts over", how: How::NoReturn { why: "it writes the library; its only text-derived output is a change note of sizes and heading names that `library history` shows the operator" } },
    Read { file: "library_archive.rs", what: "asks whether a source's `source.md` exists", how: How::NoReturn { why: "an `is_file` check; it reads no text" } },
    Read { file: "library_excerpts.rs", what: "`Library::excerpts` returns the kept excerpt text of any source, private included; `add_excerpt` reads `source.md` to locate a quote", how: How::NotYetMarked { why: "a helper: its callers are `library_claims.rs` (withholds), `library_page_check.rs` (above) and `library_export.rs` (egress-checked); `add_excerpt` is the operator's own act. No seat or host method calls it directly" } },
    Read { file: "library_export.rs", what: "`export_one` reads `source.md` and the excerpts of a source to write a bundle", how: How::Withholds { proof_file: "delphi-engine/src/library_tests.rs", proof_test: "a_private_source_never_leaves_and_is_never_indexed" } },
    Read { file: "library_extract.rs", what: "reads source text to check that a summary page's quote is a verbatim substring of a source the library holds, for the operator's accept and attest flow", how: How::OperatorOnly { why: "reached by `digest.act` and the attest commands, which need the console scope or the operator's terminal; the candidates carry the page's own quote" } },
    Read { file: "library_history.rs", what: "`Library::history` lists a source's kept captures", how: How::NoReturn { why: "it measures `source.md` (bytes) and reads the record; the entry carries a version, a hash, a size, a change note and an anchor count, no text" } },
    Read { file: "kb.rs", what: "the index: `chunk_content` returns indexed text; `include_staged` opts in to staged notes (host-noted entries among them)", how: How::NoReturn { why: "the index lists only library sources that are not private (`Library::indexable`); staged notes are off unless a caller sets `include_staged`, and the callers are the `service.rs` row (host search, marks) and the console's `kb.search`" } },
    Read { file: "mcp_project.rs", what: "the MCP server's own readers (`search_project`, `search_memory`, `query_memory`, `kb_retrieve`): they run in the MCP server process, which has no daemon session to mark", how: How::NoReturn { why: "their file sets exclude the `staged` zone (`scope.rs` drops it) and their KB zone list names no `staged`; `local/staged` is never readable by a role (`pathguard`). The test `the_mcp_servers_own_readers_never_return_a_noted_entry` proves it with a canary" } },
    Read { file: "host_rpc.rs", what: "`read_handoff`: `host.handoff_get` returns the handoff another session of the same project put", how: How::Skips { exposure: "the text came from another session whose mark (if any) is its own: the reading session is unmarked, so a handoff that carries words from a private note reaches a fresh session with no mark and nothing held on its sends. Accepted for now by OD-5 (no inheritance); a decision, not a bug" } },
    Read { file: concat!("history_", "record.rs"), what: "`for_prompt` returns a spec 102 record's prompt text; `read` returns the store", how: How::Withholds { proof_file: "delphi-engine/tests/history_egress.rs", proof_test: "nothing_but_the_history_module_calls_for_prompt_and_by_default_no_record_is_served" } },
    Read { file: concat!("history_", "view.rs"), what: "`history.view` shows the stored records' prompt text", how: How::OperatorOnly { why: "the `history.view` method needs the console scope (`history_view_is_for_the_console_only_reads_only_and_says_history_is_off_when_it_is`)" } },
    Read { file: concat!("history_", "manage.rs"), what: "`overview` counts the verified records", how: How::NoReturn { why: "counts and states only (`delphi history status`, the operator's command)" } },
    Read { file: "council.rs", what: "fences a noted entry in a seat's reading (`host_noted::fence`) and withdraws the seat's tools", how: How::NoReturn { why: "no path delivers a noted entry to a seat (`engine.rs` refuses with `READ_REFUSAL`); this is the tripwire for one added later" } },
];

// ---- slice M4 (`specs/103` §6, the red-team pass): every way a host reaches the daemon ---------------------------------------------------
//
// `READS` is a table of FILES. The host surface is a table of NAMES: every method the host socket answers and every tool the MCP server
// offers, each with what it does about a private entry. A method or tool added later fails `every_host_method_and_mcp_tool_is_classified`
// until someone classifies it, so a new way to hand a host private text cannot ship unrecorded.

/// One host method or MCP tool and what it does about a private entry's text.
pub struct Surface {
    pub name: &'static str,
    pub how: How,
}

const fn no_return(name: &'static str, why: &'static str) -> Surface {
    Surface { name, how: How::NoReturn { why } }
}

const WRITES: &str = "a write: it returns an id, a status or fixed words, never an entry's text";
const PROJECT_FILES: &str = "reads the project's own files, the role packs or the specs, which hold no library or host-noted entry";
const SEATS_CANNOT_READ: &str = "an answer written by seats that cannot read a noted entry (`engine.rs` refuses it) and that are handed a claim resting on a private source withheld (`claim_slice`); NOT VALIDATED beyond those two refusals";
const MCP_OWN: &str = "runs in the MCP server's own process over files and the index; their file sets and zones exclude `staged` (`the_mcp_servers_own_readers_never_return_a_noted_entry`) and the index lists no private source";

/// The methods of the host socket (`delphi_core::server::HOST_METHODS`).
pub const HOST_SURFACE: [Surface; 24] = [
    no_return("session.ping", "answers `pong`"),
    Surface { name: "host.search_kb", how: How::Marks },
    no_return("host.propose_note", WRITES),
    no_return("host.propose_kb_entry", "a write: it returns a status, a ref, a `private` flag and a reason slug per claim, never the claim text"),
    no_return("host.request_test_authorization", WRITES),
    no_return("host.record_session_summary", WRITES),
    no_return("host.handoff_put", WRITES),
    Surface { name: "host.handoff_get", how: How::Skips { exposure: "returns the handoff another session put; that session's mark is its own and does not follow the text (OD-5)" } },
    no_return("host.request_help", WRITES),
    no_return("host.help_result", SEATS_CANNOT_READ),
    no_return("host.request_council", WRITES),
    no_return("host.council_status", "fixed-form progress (role, state, counts); no model text"),
    no_return("host.council_result", SEATS_CANNOT_READ),
    no_return("host.council_open", WRITES),
    no_return("host.council_frame", "the static frame of a council: no entry"),
    no_return("host.council_brief", "returns the brief the host's own session wrote"),
    no_return("host.council_submit", WRITES),
    no_return("host.council_choose", WRITES),
    no_return("host.request_research", WRITES),
    no_return("host.research_status", "fixed-form progress; no entry text"),
    no_return("host.research_result", "the answer of a run over the indexed KB (no private source, no staged note) and pages the run fetched; a claim resting on a private source is counted and kept out of outbound queries; NOT VALIDATED beyond reading the run's callers"),
    no_return("host.cancel_job", WRITES),
    no_return("host.route_decided", WRITES),
    no_return("host.ask_family", "the answer of another model family to the host's own question; it is handed no entry by the daemon"),
];

/// The tools of the MCP server (`mcp_host::tools()`).
pub const MCP_SURFACE: [Surface; 39] = [
    Surface { name: "search_kb", how: How::Marks },
    no_return("list_roles", PROJECT_FILES),
    no_return("delphi_roster", PROJECT_FILES),
    no_return("get_agent", PROJECT_FILES),
    no_return("list_skills", PROJECT_FILES),
    no_return("get_skill", PROJECT_FILES),
    no_return("specs_status", PROJECT_FILES),
    no_return("specs_show", PROJECT_FILES),
    no_return("specs_list", PROJECT_FILES),
    no_return("scaffold_status", PROJECT_FILES),
    no_return("search_project", MCP_OWN),
    no_return("search_memory", MCP_OWN),
    no_return("query_memory", MCP_OWN),
    no_return("kb_retrieve", MCP_OWN),
    no_return("consult_orchestrator", "advice from the role packs; no entry"),
    no_return("spawn_agent_pack", PROJECT_FILES),
    no_return("route", "ranks roles by words only"),
    no_return("propose_note", WRITES),
    no_return("request_test_authorization", WRITES),
    no_return("propose_kb_entry", WRITES),
    no_return("propose_council_record", WRITES),
    no_return("record_session_summary", WRITES),
    no_return("handoff_put", WRITES),
    Surface { name: "handoff_get", how: How::Skips { exposure: "see `host.handoff_get`" } },
    no_return("request_help", WRITES),
    no_return("help_result", SEATS_CANNOT_READ),
    no_return("council_frame", "the static frame of a council: no entry"),
    no_return("council_open", WRITES),
    no_return("council_brief", "returns the brief the host's own session wrote"),
    no_return("council_submit", WRITES),
    no_return("council_choose", WRITES),
    no_return("request_council", WRITES),
    no_return("council_status", "fixed-form progress; no model text"),
    no_return("council_result", SEATS_CANNOT_READ),
    no_return("request_research", WRITES),
    no_return("research_status", "fixed-form progress; no entry text"),
    no_return("research_result", "see `host.research_result`"),
    no_return("cancel_job", WRITES),
    no_return("ask_family", "see `host.ask_family`"),
];

/// The relay stream's event kinds that carry no text an agent wrote, and why. The kinds that do are `AGENT_TEXT_KINDS` (masked above). A
/// kind added to `delphi_core::hub::RELAY_EVENTS` fails `every_relay_event_kind_is_masked_or_shown_to_carry_no_agent_text` until it is in
/// one of the two lists.
pub const RELAY_KINDS_WITHOUT_AGENT_TEXT: [(&str, &str); 3] = [
    ("approval.resolved", "ids, the resolution word, `by` (a console client, `relay:<channel>:<sender>`, `timeout`, `recovery`) and a reason that is the operator's typed words or a fixed phrase of the daemon's; a hook's output becomes a denial, not an event"),
    ("approval.parked", "ids and two counts"),
    ("xenia.alert", "fixed slugs and counts; the relay words it from the help catalog and never from the payload"),
];

/// The methods the relay socket answers and what each returns (`delphi_core::server::RELAY_METHODS`).
pub const RELAY_SURFACE: [(&str, &str); 5] = [
    ("session.ping", "`pong`"),
    ("events.subscribe", "the stream, through the relay filter"),
    ("approval.list", "the pending cards, through `mask_pending_for_relay`"),
    ("approval.relay_resolve", "the resolution of the card it answers (`Resolution::event_payload`: ids, the word, `by`, the operator's reason); refused for a withheld card that it would allow"),
    ("budget.get", "numbers"),
];


/// The accessors whose callers are the readers of entry text. Matched against the production text of every file of this crate.
/// This is a token scan of three accessors, not a proof of completeness (`specs/103` §2: NOT VALIDATED; the red-team slice M4 attacks it).
///
/// Slice M4 widened it from three accessors to the ones below: the raw files a source is kept in (`"source.md"`, `excerpts.json`), the
/// excerpt reader, the staged-note opt-in, the handoff reader, the indexed chunk reader and the history record's prompt text. A file that
/// contains one in production code and is not a row of [`READS`] fails the test. It is still a token scan: a reader that reaches the text
/// through a function that is not named here is found only if one of its callees is (the scan cannot see a rename).
pub const ACCESSORS: [&str; 11] = ["stored_text", "claim_slice(", "NOTED_HEADING", "\"source.md\"", "excerpts.json", ".excerpts(", "include_staged", "read_handoff(", "chunk_content", concat!("for_", "prompt("), ".prompt.as_ref()"];

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
    /// Of the sends held on Outbound and Connector, how many were held ONLY because of the task's scope (an ancestor's mark or the session's)
    /// and not its own: what the OD-5 rule costs, so the operator can see it (`specs/103` §7.1).
    pub held_by_inheritance: u64,
}

fn read_counter(ws_dir: &Path) -> Counter {
    let v = std::fs::read_to_string(counter_path(ws_dir)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    let n = |k: &str| v[k].as_u64().unwrap_or(0);
    Counter { keys_marked: n("keys_marked"), entries_marked: n("entries_marked"), cleared: n("cleared"), outbound_would_change: n("outbound_would_change"), connector_would_change: n("connector_would_change"), unreadable: n("unreadable"), held_by_inheritance: n("held_by_inheritance") }
}

fn write_counter(ws_dir: &Path, c: &Counter) -> Result<(), String> {
    let v = json!({"keys_marked": c.keys_marked, "entries_marked": c.entries_marked, "cleared": c.cleared, "outbound_would_change": c.outbound_would_change, "connector_would_change": c.connector_would_change, "unreadable": c.unreadable, "held_by_inheritance": c.held_by_inheritance});
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

// ---- OD-5 (`specs/103` §7.1): a child holds what its parent holds, within a session ----------------------------------------------------
//
// The operator, 2026-11-12: "the child should have the same scope as the parent in a given session". Read in the smallest faithful way, a
// task is held when ANY key in its scope is marked:
//
//   * its own key, `<mission>/<task>`;
//   * the keys of its recorded ancestors, followed through [`link_task`] (at most [`MAX_CHAIN`] steps; a cycle ends the walk);
//   * the session scope: the mission's own key. A host session's mark is kept under its mission id (`host-<session>`), and every task the
//     host started in that session (`host.request_help`, a Council, a research run) runs in that mission, so it shares the host's scope.
//     Another session of the same host is another mission and does not share it;
//   * the bare task id, which older marks and the relay tests used (it can only hold more).
//
// A task inside one mission already shares its own key with every span of its DAG (a scout the orchestrator dispatched is a span of the same
// task), so the only parent links that need recording are the ones between separate TASKS. The one such link Delphi creates is a timer: the
// wake is "a new task" set by an earlier one, so the setter's task key is noted when the timer is set ([`note_timer_setter`]) and linked to
// the woken task before it starts ([`link_woken_task`]). Both are written by the daemon from its own context, never from a tool argument.
// A task with no recorded link has no ancestors and behaves as before the decision (tasks that existed before it was built are such).
//
// Inheritance only ever ADDS a hold: it reads marks, writes none, and clearing is still the console clearing a KEY (`clear`). A task stays
// held while any key in its scope is still marked, so clearing the parent's mark does not lift a hold that the session's key still gives.

/// How many ancestors are followed. A chain this long is not made by a person; the cap ends a loop a damaged link could make.
pub const MAX_CHAIN: usize = 8;

fn links_dir(ws_dir: &Path) -> PathBuf {
    dir(ws_dir).join("links")
}

fn link_file(ws_dir: &Path, name: &str) -> PathBuf {
    links_dir(ws_dir).join(format!("{}.json", name.replace('/', "__").replace(':', "_")))
}

/// Record that the task `child_key` is a child of the task `parent_key` (both from [`task_key`]). Called by the daemon when it starts a task
/// on a parent's behalf; never from a tool argument. A link is written once and not changed.
pub fn link_task(ws_dir: &Path, child_key: &str, parent_key: &str) -> Result<(), String> {
    if !key_ok(child_key) || !key_ok(parent_key) || child_key == parent_key {
        return Err("a link names two different keys by identifier only".into());
    }
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = link_file(ws_dir, child_key);
    if path.exists() {
        return Ok(());
    }
    write_atomic(&path, &json!({"key": child_key, "parent": parent_key}))
}

/// Note which task set the timer `timer_id`, so the task its wake starts can be linked to it. Called by the daemon when a role's timer is
/// set from inside a task. An operator-set timer has no setter task and no note.
pub fn note_timer_setter(ws_dir: &Path, timer_id: &str, setter_key: &str) -> Result<(), String> {
    if !id_ok(timer_id) || !key_ok(setter_key) {
        return Err("a timer note names a timer and a task by identifier only".into());
    }
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    write_atomic(&link_file(ws_dir, &format!("timer:{timer_id}")), &json!({"timer": timer_id, "parent": setter_key}))
}

/// Link the task `child_key` (a timer's wake, about to start) to the task that set the timer, when one is noted. Nothing noted is nothing to
/// link: the wake is then an ordinary task. Returns whether a link was made.
pub fn link_woken_task(ws_dir: &Path, timer_id: &str, child_key: &str) -> bool {
    let parent = match read_link(&link_file(ws_dir, &format!("timer:{timer_id}"))) {
        Ok(Some(p)) => p,
        _ => return false,
    };
    link_task(ws_dir, child_key, &parent).is_ok()
}

/// The parent a link file names: `Ok(None)` when there is no file, `Err(())` when a file exists and cannot be read.
fn read_link(path: &Path) -> Result<Option<String>, ()> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
        Ok(t) => match serde_json::from_str::<Value>(&t) {
            Ok(v) => match v["parent"].as_str() {
                Some(p) if key_ok(p) => Ok(Some(p.to_string())),
                _ => Err(()),
            },
            Err(_) => Err(()),
        },
    }
}

/// The parent of the task `key`: `Ok(None)` when none is recorded.
fn parent_of(ws_dir: &Path, key: &str) -> Result<Option<String>, ()> {
    read_link(&link_file(ws_dir, key))
}

/// What holds a task, and whether the hold is the task's own or came from its scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub state: State,
    /// True when the task's own key is not marked and the hold came from an ancestor or the session (what the rule costs research).
    pub inherited: bool,
}

/// Fold the states of several keys: any unreadable key makes the answer unreadable (deny by default on the four channels); otherwise any
/// marked key marks it, with the entries of the first one found.
fn fold(states: impl IntoIterator<Item = State>) -> State {
    let mut found = State::Unmarked;
    for s in states {
        match s {
            State::Unreadable => return State::Unreadable,
            m @ State::Marked { .. } => {
                if found == State::Unmarked {
                    found = m;
                }
            }
            State::Unmarked => {}
        }
    }
    found
}

/// The keys that hold the task `task` of `mission`, as `(own, inherited)`. `Err(())` when a link on the chain cannot be read. The own keys
/// are the task's key and the bare task id; the inherited ones are its ancestors' keys and the mission's own key.
fn scope_keys(ws_dir: &Path, mission: &str, task: &str) -> Result<(Vec<String>, Vec<String>), ()> {
    let own_key = task_key(mission, task);
    let mut own = vec![own_key.clone()];
    if key_ok(task) {
        own.push(task.to_string());
    }
    let mut inherited = Vec::new();
    let mut seen = vec![own_key.clone()];
    let mut at = own_key;
    for _ in 0..MAX_CHAIN {
        match parent_of(ws_dir, &at)? {
            Some(p) if !seen.contains(&p) => {
                inherited.push(p.clone());
                seen.push(p.clone());
                at = p;
            }
            _ => break,
        }
    }
    if key_ok(mission) {
        inherited.push(mission.to_string());
    }
    Ok((own, inherited))
}

/// The one question every send and every relay text asks (`specs/103` §7.1 OD-5): is this task held, by its own mark or by its scope's?
/// With a mission it resolves the keys above. Without one (a relay card that names no mission and has none noted) it asks of every mission
/// that has a mark or a link for a task of that id, which can hold a card of an unmarked task that shares an id and can never let a
/// marked task's text go. Reads marks and links; writes nothing.
pub fn task_scope(ws_dir: &Path, mission: Option<&str>, task: &str) -> Scope {
    match mission {
        Some(m) => scope_in_mission(ws_dir, m, task),
        None => {
            // Marks under `<mission>/<task>` in any mission, found by FILE NAME so a damaged record fails closed (M4), and the bare id.
            let own = fold([task_state_in_any_mission(ws_dir, task), if key_ok(task) { state(ws_dir, task) } else { State::Unmarked }]);
            // Links: a task of that id in any mission may be the child of a marked task or of a marked session. A link file that cannot be
            // read has no key inside it to find, so it is found by file name too, and a damaged one fails closed.
            let mut missions: Vec<String> = Vec::new();
            let mut damaged = false;
            if key_ok(task) {
                let name_suffix = format!("__{}.json", task.replace(':', "_"));
                let key_suffix = format!("/{task}");
                for e in std::fs::read_dir(links_dir(ws_dir)).into_iter().flatten().flatten() {
                    let p = e.path();
                    if !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&name_suffix)) {
                        continue;
                    }
                    let key = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["key"].as_str().map(str::to_string));
                    match key.as_deref().and_then(|k| k.strip_suffix(&key_suffix)) {
                        Some(m) if !m.is_empty() => {
                            if !missions.iter().any(|x| x == m) {
                                missions.push(m.to_string());
                            }
                        }
                        _ => damaged = true,
                    }
                }
            }
            let linked: Vec<Scope> = missions.iter().map(|m| scope_in_mission(ws_dir, m, task)).collect();
            let own_marked = matches!(own, State::Marked { .. }) || linked.iter().any(|s| matches!(s.state, State::Marked { .. }) && !s.inherited);
            let mut all = vec![own];
            all.extend(linked.into_iter().map(|s| s.state));
            if damaged {
                all.push(State::Unreadable);
            }
            let state = fold(all);
            Scope { inherited: matches!(state, State::Marked { .. }) && !own_marked, state }
        }
    }
}

fn scope_in_mission(ws_dir: &Path, mission: &str, task: &str) -> Scope {
    let Ok((own, inherited)) = scope_keys(ws_dir, mission, task) else {
        return Scope { state: State::Unreadable, inherited: false };
    };
    let own_state = fold(own.iter().map(|k| state(ws_dir, k)));
    if own_state != State::Unmarked {
        // an unreadable ancestor still makes the whole answer unreadable
        let up = fold(inherited.iter().map(|k| state(ws_dir, k)));
        let state = if up == State::Unreadable { State::Unreadable } else { own_state };
        return Scope { state, inherited: false };
    }
    let up = fold(inherited.iter().map(|k| state(ws_dir, k)));
    Scope { inherited: matches!(up, State::Marked { .. }), state: up }
}

/// The record a send of a task is checked against: [`send_record`] for a whole scope.
pub fn scope_record(st: &State) -> Value {
    record_for(st)
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

/// The send on `channel` (`"outbound"` or `"connector"`) of a task whose scope is `scope` was refused by the egress check and is being put
/// to the operator instead. Count it, and count it again when only the scope held it. Called only by the two senders, only after the check
/// refused; it decides nothing.
pub fn note_held(ws_dir: &Path, channel: &str, scope: &Scope) {
    let st = &scope.state;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut c = read_counter(ws_dir);
    match channel {
        "outbound" => c.outbound_would_change += 1,
        "connector" => c.connector_would_change += 1,
        _ => return,
    }
    if *st == State::Unreadable {
        c.unreadable += 1;
    }
    if scope.inherited {
        c.held_by_inheritance += 1;
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

/// How many readers in [`READS`] skip the mark and are a real gap (`How::Skips`): the red-team pass's findings, reported and not hidden.
pub fn readers_skipping_the_mark() -> usize {
    READS.iter().filter(|r| matches!(r.how, How::Skips { .. })).count()
}

/// One line for `delphi doctor` and the console: what observe mode has seen. Counts only.
pub fn status_line(ws_dir: &Path) -> String {
    let c = counter(ws_dir);
    format!(
        "private-read mark: {} task(s) marked, {} entr(y/ies) read, {} cleared; sends held for your answer: outbound {}, connector {} ({} unreadable, {} only because a parent task or the session was marked); {} reader(s) do not mark yet",
        c.keys_marked,
        c.entries_marked,
        c.cleared,
        c.outbound_would_change,
        c.connector_would_change,
        c.unreadable,
        c.held_by_inheritance,
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

/// The mission each raised card belongs to, noted when the engine raises the card (`note_card_raised`), newest last. In memory only. A card
/// record names its task and not its mission, so without this a mark kept on the MISSION (a host session's mark is kept under its mission
/// id) was found by the stream (the event names its mission) and not by the card list or a relay's answer when the relay was offline while
/// the card was raised and asked for the list on reconnecting (the red-team pass, M4).
static CARD_MISSIONS: Mutex<VecDeque<(String, String)>> = Mutex::new(VecDeque::new());

/// Called by the engine when it raises a card: remember which mission it belongs to. Records nothing about its text.
pub fn note_card_raised(approval_id: &str, mission: &str) {
    let mut q = CARD_MISSIONS.lock().unwrap_or_else(|p| p.into_inner());
    if q.len() >= MASKED_CARDS_CAP {
        q.pop_front();
    }
    q.push_back((approval_id.to_string(), mission.to_string()));
}

fn card_mission(approval_id: &str) -> Option<String> {
    CARD_MISSIONS.lock().unwrap_or_else(|p| p.into_inner()).iter().rev().find(|(id, _)| id == approval_id).map(|(_, m)| m.clone())
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
    let mission = event["mission_id"].as_str().filter(|m| key_ok(m));
    // The one lookup (`task_scope`): the task's own key, its recorded ancestors', the mission's (the session's) and the bare task id.
    let mut st = match task {
        Some(t) => task_scope(ws_dir, mission, t).state,
        None => State::Unmarked,
    };
    // An event that names a mission and no task (a mission-level event) is held by the mission's own mark.
    if task.is_none() {
        if let Some(m) = mission {
            st = state(ws_dir, m);
        }
    }
    st
}

/// The state of a task named without its mission (a card on the relay's list carries only the task id): every live mark whose key is
/// `<mission>/<task>`, in any mission. Task ids repeat across missions, so this can withhold a card of an unmarked task that shares an id
/// with a marked one; it can never let a marked task's text go. A record that exists and cannot be read makes the answer `Unreadable`.
fn task_state_in_any_mission(ws_dir: &Path, task: &str) -> State {
    if !key_ok(task) {
        return State::Unmarked;
    }
    // Found by FILE NAME, not by `live()`: a record that cannot be read has no key inside it for `live()` to find, and a damaged mark must
    // fail closed here as it does on a send (`specs/103` §4.3; the red-team pass, M4). `file_of` writes `<mission>__<task>.json`.
    let suffix = format!("__{}.json", task.replace(':', "_"));
    let mut found = State::Unmarked;
    for e in std::fs::read_dir(dir(ws_dir)).into_iter().flatten().flatten() {
        let p = e.path();
        if !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&suffix)) {
            continue;
        }
        match read_record(&p) {
            Err(()) => return State::Unreadable,
            Ok(None) => {}
            Ok(Some(v)) => match v["key"].as_str().map(|k| state(ws_dir, k)) {
                Some(State::Unreadable) | None => return State::Unreadable,
                Some(m @ State::Marked { .. }) => found = m,
                Some(State::Unmarked) => {}
            },
        }
    }
    found
}

/// Is a card's text withheld from a relay? True when the stream withheld it, or when its task is marked or its mark cannot be read.
pub fn card_withheld(ws_dir: &Path, approval_id: &str, task_id: &str) -> bool {
    if card_is_masked(approval_id) {
        return true;
    }
    // The one lookup, for a card that names no mission: any mission's mark or link for a task of that id, and the bare id.
    let st = if key_ok(task_id) { task_scope(ws_dir, None, task_id).state } else { State::Unmarked };
    if !stream_text_can_go(&st) {
        return true;
    }
    // The mission the engine noted when it raised the card: a mark kept under the mission (a host session's) or under `<mission>/<task>`.
    match card_mission(approval_id) {
        Some(m) => !stream_text_can_go(&event_state(ws_dir, &json!({"mission_id": m, "task_id": task_id}))),
        None => false,
    }
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
            let call = format!("crate::egress::check(&crate::private_read::scope_record(&scope.state), crate::library::{channel})");
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
        // the last test module, however it is declared (`mod tests` or `pub(crate) mod tests`)
        let cut = ["\n#[cfg(test)]\nmod tests", "\nmod tests", "\n#[cfg(test)]\npub(crate) mod tests", "\npub(crate) mod tests"].iter().filter_map(|p| src.rfind(p)).max().unwrap_or(src.len());
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
        note_held(d.path(), "outbound", &Scope { state: State::Unreadable, inherited: false });
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
        let own = Scope { state: state(d.path(), "t-1"), inherited: false };
        note_held(d.path(), "outbound", &own);
        note_held(d.path(), "outbound", &own);
        note_held(d.path(), "connector", &own);
        note_held(d.path(), "relay", &own);
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
    /// The files among `sources` that name an accessor of entry text in production code (comment lines do not count) and are not a row of
    /// `READS`. The module's own file is skipped: it defines the accessors and the table.
    fn unlisted_readers(sources: &[(String, String)]) -> Vec<String> {
        let mut out = Vec::new();
        for (file, text) in sources {
            if file == "private_read.rs" {
                continue;
            }
            let code: String = production(text).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            if ACCESSORS.iter().any(|a| code.contains(a)) && !READS.iter().any(|r| r.file == file) {
                out.push(file.clone());
            }
        }
        out
    }

    /// The scan is only worth its name if it fails for a NEW reader. A fixture adds three unclassified files, each reaching entry text a
    /// different way (the raw source file, the excerpt reader, the staged-note opt-in); one classified file and one that reads nothing are
    /// not reported. A comment that merely names an accessor is not a reader.
    #[test]
    fn the_scan_catches_a_new_unclassified_reader_and_not_a_classified_or_an_innocent_file() {
        let src = |body: &str| format!("pub fn f() {{\n{body}\n}}\n\n#[cfg(test)]\nmod tests {{\n    fn t() {{ let _ = \"source.md\"; }}\n}}\n");
        let fixture: Vec<(String, String)> = vec![
            ("new_raw.rs".into(), src("let t = std::fs::read_to_string(dir.join(\"source.md\"));")),
            ("new_excerpts.rs".into(), src("let e = lib.excerpts(&id);")),
            ("new_staged.rs".into(), src("let o = SearchOpts { include_staged: true, ..Default::default() };")),
            ("library_export.rs".into(), src("let t = std::fs::read_to_string(dir.join(\"source.md\"));")),
            ("innocent.rs".into(), src("// this comment names \"source.md\" and .excerpts( only to explain\nlet x = 1;")),
            ("tests_only.rs".into(), src("let x = 1;")),
        ];
        assert_eq!(unlisted_readers(&fixture), vec!["new_raw.rs".to_string(), "new_excerpts.rs".to_string(), "new_staged.rs".to_string()]);
        // and a row for a new file makes the same fixture pass: the failure message is "add it with what it does about a private entry"
        let accessors_cover_the_known_paths = ["\"source.md\"", "excerpts.json", ".excerpts(", "include_staged", "read_handoff(", "stored_text", "claim_slice(", "NOTED_HEADING"];
        assert!(accessors_cover_the_known_paths.iter().all(|a| ACCESSORS.contains(a)));
    }

    #[test]
    fn every_reader_of_private_text_is_in_the_reads_table() {
        let sources = engine_sources();
        let missing = unlisted_readers(&sources);
        assert!(missing.is_empty(), "{missing:?} read the text of an entry (one of {ACCESSORS:?}) and are not in READS (specs/103 §2): add each with what it does about a private entry");
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

    // ---- slice M4 (`specs/103`): the red-team pass --------------------------------------------------------------------------------------
    //
    // Each test below is an attack written against fake fixtures only. Where an attack SUCCEEDS the test pins what happens today and says so
    // in its name or its comment, so the gap is a recorded fact the operator can read and a change that closes it must update the test on
    // purpose. Where it fails the test is the proof.

    fn surface_names(s: &[Surface]) -> Vec<String> {
        let mut v: Vec<String> = s.iter().map(|x| x.name.to_string()).collect();
        v.sort();
        v
    }

    /// Every method the host socket answers and every tool the MCP server offers is classified; a stale row is an error too. Only
    /// `search_kb` marks, and the MCP tool of that name is the host method of that name.
    #[test]
    fn every_host_method_and_mcp_tool_is_classified() {
        let mut host: Vec<String> = delphi_core::server::HOST_METHODS.iter().map(|s| s.to_string()).collect();
        host.sort();
        assert_eq!(host, surface_names(&HOST_SURFACE), "a host method is missing from HOST_SURFACE, or a row names a method the socket does not answer");
        let tools = crate::mcp_host::tools();
        let mut names: Vec<String> = tools.as_array().expect("tools() is a list").iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        names.sort();
        assert_eq!(names, surface_names(&MCP_SURFACE), "an MCP tool is missing from MCP_SURFACE, or a row names a tool the server does not offer");
        for table in [&HOST_SURFACE[..], &MCP_SURFACE[..]] {
            let marking: Vec<&str> = table.iter().filter(|s| s.how == How::Marks).map(|s| s.name).collect();
            assert!(marking == ["host.search_kb"] || marking == ["search_kb"], "only the host's search marks today: {marking:?}");
            for s in table {
                match s.how {
                    How::NoReturn { why } | How::OperatorOnly { why } | How::NotYetMarked { why } => assert!(why.len() > 8, "{}: say why", s.name),
                    How::Skips { exposure } => assert!(exposure.len() > 8, "{}: say what leaks", s.name),
                    How::Marks | How::Withholds { .. } => {}
                }
            }
        }
        let mcp = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/mcp_host.rs")).unwrap();
        assert!(production(&mcp).contains("\"search_kb\" => query().and_then(|q| self.daemon(\"host.search_kb\""), "the MCP search_kb is the host method that marks");
        // the findings, counted: the two names of the one handoff reader
        assert_eq!(HOST_SURFACE.iter().chain(MCP_SURFACE.iter()).filter(|s| matches!(s.how, How::Skips { .. })).count(), 2);
        assert_eq!(readers_skipping_the_mark(), 1, "READS has the one file-level finding (the handoff reader)");
    }

    /// The MCP server's own readers run in a process with no daemon session, so they cannot mark. The table says they return no private
    /// text; this is the canary that proves it for the one kind of private text that sits where they look: a staged host-noted entry,
    /// present both as a file in the project layer and as an indexed document. (A private library source is never indexed: `library_tests`.)
    #[test]
    fn the_mcp_servers_own_readers_never_return_a_noted_entry() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("workspaces/w");
        let doc = format!("---\nkind: fact\n---\n# Noted entry\n\nTopic: quokkafixture\n\n- the quokkafixture holds {CANARY} (source: https://example.com/q; quote verified by the daemon)\n");
        for rel in ["local/staged/host-abcd1234/n1.md", "local/staged/n2.md"] {
            let p = ws.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, &doc).unwrap();
        }
        let mut kb = crate::kb::Kb::open(&ws.join("local/state.db")).unwrap();
        kb.upsert_from("staged/host-abcd1234/n1.md", "staged", "external_untrusted", &doc, "2026-11-12T00:00:00Z").unwrap();
        // the control: the daemon's host search (which names the staged zone) finds it, so the fixture is a real, searchable noted entry
        let opts = crate::kb::SearchOpts { role: None, limit: 5, zones: Some(vec!["staged"]), include_staged: true, include_untrusted: true, ..Default::default() };
        let hits = kb.search("quokkafixture", &opts).unwrap();
        assert!(hits.iter().any(|h| h.rel_path.starts_with("staged/host-")), "the fixture is searchable by the host's search");
        drop(kb);
        let p = crate::mcp_project::Project::new(&ws, t.path());
        let answers = [
            p.search_project("quokkafixture", 10),
            p.search_memory("quokkafixture", None, 10),
            p.query_memory("quokkafixture", None, None, 10),
            p.kb_retrieve("quokkafixture", 10, "all", "all"),
            p.kb_retrieve("quokkafixture", 10, "propositions_included", "core_only"),
        ];
        for (i, a) in answers.iter().enumerate() {
            let text = a.as_ref().map(|s| s.as_str()).unwrap_or("(error)");
            assert!(!text.contains(CANARY) && !text.contains("staged/"), "reader {i} returned a noted entry: {text}");
        }
    }

    /// A kind added to the relay stream must be masked or shown to carry no agent-written text, and the relay socket answers a closed list.
    #[test]
    fn every_relay_event_kind_is_masked_or_shown_to_carry_no_agent_text() {
        for k in delphi_core::hub::RELAY_EVENTS {
            assert!(AGENT_TEXT_KINDS.contains(&k) || RELAY_KINDS_WITHOUT_AGENT_TEXT.iter().any(|(n, _)| *n == k), "the relay is sent `{k}` and it is neither masked nor classified as carrying no agent text");
        }
        for k in AGENT_TEXT_KINDS {
            assert!(delphi_core::hub::RELAY_EVENTS.contains(&k), "{k} is masked but is not a relay event");
        }
        for (k, why) in RELAY_KINDS_WITHOUT_AGENT_TEXT {
            assert!(delphi_core::hub::RELAY_EVENTS.contains(&k) && !AGENT_TEXT_KINDS.contains(&k) && why.len() > 8, "{k}");
        }
        let mut methods: Vec<&str> = delphi_core::server::RELAY_METHODS.to_vec();
        methods.sort();
        let mut table: Vec<&str> = RELAY_SURFACE.iter().map(|(n, _)| *n).collect();
        table.sort();
        assert_eq!(methods, table, "a relay method is not classified");
        // and the unmasked kinds really do carry nothing the filter would have replaced: their producers' payload keys are fixed words and ids
        for (kind, payload) in [
            ("approval.resolved", json!({"approval_id": "ap_1", "resolution": "denied", "reason": "the task ended", "by": "console-1"})),
            ("approval.parked", json!({"approval_id": "ap_1", "parked_secs": 5, "park_max_secs": 60})),
        ] {
            let d = ws();
            mark(d.path(), "t_0001", "entry-1", 1).unwrap();
            let ev = json!({"type": kind, "mission_id": "m_1", "task_id": "t_0001", "payload": payload});
            assert!(mask_event_for_relay(d.path(), &ev).is_none(), "{kind} is sent as written, and is shown above to hold no agent-written text");
        }
    }

    /// ATTACK: a paraphrase. The mark is not content-based: it does not look at the words. It replaces ALL agent-written text of a marked task,
    /// so a paraphrase, a translation or a summary of a private entry is withheld exactly as a copy is. It does NOT cover text that did not pass
    /// through a marked task: the same words in an unmarked task's card go out untouched (nothing in the daemon knows they are private).
    #[test]
    fn attack_a_paraphrase_is_withheld_with_everything_else_a_marked_task_wrote_and_the_same_words_from_an_unmarked_task_are_not() {
        let d = ws();
        mark(d.path(), "m_1/t_0001", "entry-1", 1).unwrap();
        let said = "the vault password rotates quarterly (in other words)";
        let card = |task: &str, id: &str| {
            let mut e = card_event(task, id);
            e["payload"]["summary"] = json!(said);
            e
        };
        let marked = mask_event_for_relay(d.path(), &card("t_0001", "ap_m4_par_1")).expect("a marked task's card is masked, whatever it says");
        assert!(!marked.to_string().contains("vault") && !marked.to_string().contains("quarterly"));
        assert!(mask_event_for_relay(d.path(), &card("t_0002", "ap_m4_par_2")).is_none(), "NOT COVERED: the same words from a task that never read the entry");
    }

    /// `specs/103` §7.1 OD-5 (the operator, 2026-11-12: "the child should have the same scope as the parent in a given session"). Inheritance follows
    /// RECORDED links and the session's key, never a guess from the mission alone: two tasks of one mission with no link between them are
    /// siblings, and a mark on one does not hold the other. (M4 pinned the opposite for a child with a link; that test is now
    /// `a_child_is_held_by_its_parents_mark...` below, with the same fixtures.)
    #[test]
    fn a_task_with_no_recorded_parent_link_is_not_held_by_a_neighbours_mark() {
        let d = ws();
        let parent = task_key("m_1", "t_0001");
        mark(d.path(), &parent, "entry-1", 1).unwrap();
        let child = task_key("m_1", "t_0002");
        assert_eq!(send_record(d.path(), &child), json!({"private": false}), "no link, no inheritance: the neighbour's sends are not held");
        assert_eq!(task_scope(d.path(), Some("m_1"), "t_0002"), Scope { state: State::Unmarked, inherited: false });
        let mut ev = card_event("t_0002", "ap_m4_child");
        ev["mission_id"] = json!("m_1");
        assert!(mask_event_for_relay(d.path(), &ev).is_none(), "the neighbour's card text goes to the relay as before");
        let mut ev = card_event("t_0001", "ap_m4_parent");
        ev["mission_id"] = json!("m_1");
        assert!(mask_event_for_relay(d.path(), &ev).is_some(), "the marked task itself is masked");
    }

    /// The decision itself, with M4's fixtures: a parent that read a private entry and started a child (a timer's wake, linked by the daemon) holds
    /// the child's Outbound and Connector sends and its relay text, on the first send, and the hold is counted as inherited.
    #[test]
    fn a_child_is_held_by_its_parents_mark_on_all_four_channels_through_one_lookup() {
        let d = ws();
        let (parent, child) = (task_key("m_1", "t_0001"), task_key("m_1", "t_0002"));
        link_task(d.path(), &child, &parent).unwrap();
        // a mark placed AFTER the child exists still holds it: the lookup is made at send time
        assert_eq!(task_scope(d.path(), Some("m_1"), "t_0002").state, State::Unmarked);
        mark(d.path(), &parent, "entry-1", 1).unwrap();
        let s = task_scope(d.path(), Some("m_1"), "t_0002");
        assert!(matches!(s.state, State::Marked { .. }) && s.inherited, "held, and by inheritance: {s:?}");
        assert!(crate::egress::check(&scope_record(&s.state), crate::library::Channel::Outbound).is_err());
        assert!(crate::egress::check(&scope_record(&s.state), crate::library::Channel::Connector).is_err());
        assert!(!stream_text_can_go(&s.state), "Relay and Mail text is withheld");
        let mut ev = card_event("t_0002", "ap_od5_child");
        ev["mission_id"] = json!("m_1");
        assert!(mask_event_for_relay(d.path(), &ev).is_some(), "the child's card text is replaced");
        // the parent's own hold is not "inherited"
        assert!(!task_scope(d.path(), Some("m_1"), "t_0001").inherited);
        // and the cost is counted
        note_held(d.path(), "outbound", &s);
        note_held(d.path(), "connector", &task_scope(d.path(), Some("m_1"), "t_0001"));
        let c = counter(d.path());
        assert_eq!((c.outbound_would_change, c.connector_would_change, c.held_by_inheritance), (1, 1, 1));
    }

    /// A grandchild follows the chain; a sibling, an unrelated task and the child of an unmarked parent are not held.
    #[test]
    fn a_grandchild_is_held_and_a_sibling_an_unrelated_task_and_an_unmarked_parents_child_are_not() {
        let d = ws();
        let k = |t: &str| task_key("m_1", t);
        link_task(d.path(), &k("t_0002"), &k("t_0001")).unwrap(); // child of the marked parent
        link_task(d.path(), &k("t_0003"), &k("t_0002")).unwrap(); // grandchild
        link_task(d.path(), &k("t_0004"), &k("t_0001")).unwrap(); // sibling of t_0002
        link_task(d.path(), &k("t_0006"), &k("t_0005")).unwrap(); // child of an unmarked parent
        mark(d.path(), &k("t_0002"), "entry-1", 1).unwrap(); // the middle task read the entry
        assert!(matches!(task_scope(d.path(), Some("m_1"), "t_0003").state, State::Marked { .. }), "the grandchild is held by its parent's mark");
        assert_eq!(task_scope(d.path(), Some("m_1"), "t_0001").state, State::Unmarked, "a mark never goes UP to the parent");
        assert_eq!(task_scope(d.path(), Some("m_1"), "t_0004").state, State::Unmarked, "a sibling is not held");
        assert_eq!(task_scope(d.path(), Some("m_1"), "t_0009").state, State::Unmarked, "an unrelated task is not held");
        assert_eq!(task_scope(d.path(), Some("m_1"), "t_0006").state, State::Unmarked, "the child of an unmarked parent is not held");
        assert_eq!(task_scope(d.path(), Some("m_2"), "t_0003").state, State::Unmarked, "the same task id in another mission is not held");
    }

    /// The session scope: the host's mark is kept under its mission (`host-<session>`), so every task that session started is held; another
    /// session's tasks are not (the same fixtures as M4's second-session attack, which stays true).
    #[test]
    fn a_mark_on_the_session_holds_every_task_it_started_and_another_session_is_untouched() {
        let d = ws();
        mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 1).unwrap();
        let s = task_scope(d.path(), Some("host-abcd1234"), "t_0007");
        assert!(matches!(s.state, State::Marked { .. }) && s.inherited, "a help task of the marked session is held by the session: {s:?}");
        assert_eq!(task_scope(d.path(), Some("host-bbbbbbbb"), "t_0007").state, State::Unmarked, "another session is not");
    }

    /// Clearing is the console clearing a KEY. A task stays held while ANY key in its scope is still marked.
    #[test]
    fn clearing_one_key_lifts_the_hold_only_when_no_other_key_in_the_scope_is_marked() {
        let d = ws();
        let (parent, child) = (task_key("host-abcd1234", "t_0001"), task_key("host-abcd1234", "t_0002"));
        link_task(d.path(), &child, &parent).unwrap();
        mark(d.path(), &parent, "entry-1", 1).unwrap();
        mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 1).unwrap();
        clear(d.path(), &parent, 2).unwrap();
        assert!(matches!(task_scope(d.path(), Some("host-abcd1234"), "t_0002").state, State::Marked { .. }), "the session's key still holds the child");
        clear(d.path(), "host-abcd1234", 3).unwrap();
        assert_eq!(task_scope(d.path(), Some("host-abcd1234"), "t_0002").state, State::Unmarked, "every key in the scope is clear, so the hold is lifted");
    }

    /// An unreadable mark or link anywhere on the chain fails closed on the four channels (and only there: nothing inbound reads it); a cycle
    /// ends; and the walk is bounded.
    #[test]
    fn an_unreadable_ancestor_or_link_fails_closed_and_a_cycle_or_a_long_chain_ends() {
        let d = ws();
        let k = |t: &str| task_key("m_1", t);
        link_task(d.path(), &k("t_0002"), &k("t_0001")).unwrap();
        mark(d.path(), &k("t_0001"), "entry-1", 1).unwrap();
        std::fs::write(file_of(d.path(), &k("t_0001")), b"{not json").unwrap();
        assert_eq!(task_scope(d.path(), Some("m_1"), "t_0002").state, State::Unreadable, "an unreadable ancestor mark");
        assert!(scope_record(&State::Unreadable).is_null() && crate::egress::check(&scope_record(&State::Unreadable), crate::library::Channel::Outbound).is_err());
        // a damaged LINK file: the child's chain cannot be read
        link_task(d.path(), &k("t_0004"), &k("t_0003")).unwrap();
        std::fs::write(link_file(d.path(), &k("t_0004")), b"{not json").unwrap();
        assert_eq!(task_scope(d.path(), Some("m_1"), "t_0004").state, State::Unreadable, "an unreadable link");
        // found by FILE NAME when the card names no mission, so the damaged link fails closed there too
        assert_eq!(task_scope(d.path(), None, "t_0004").state, State::Unreadable);
        assert!(card_withheld(d.path(), "ap_od5_dmg", "t_0004"));
        // a cycle
        link_task(d.path(), &k("t_0011"), &k("t_0012")).unwrap();
        link_task(d.path(), &k("t_0012"), &k("t_0011")).unwrap();
        assert_eq!(task_scope(d.path(), Some("m_1"), "t_0011").state, State::Unmarked, "a cycle ends the walk");
        // a long chain is followed MAX_CHAIN steps and no further
        for i in 0..(MAX_CHAIN + 2) {
            link_task(d.path(), &k(&format!("t_1{:03}", i + 1)), &k(&format!("t_1{:03}", i))).unwrap();
        }
        mark(d.path(), &k(&format!("t_1{:03}", 0)), "entry-9", 1).unwrap();
        assert!(matches!(task_scope(d.path(), Some("m_1"), &format!("t_1{:03}", MAX_CHAIN)).state, State::Marked { .. }), "within the cap");
        assert_eq!(task_scope(d.path(), Some("m_1"), &format!("t_1{:03}", MAX_CHAIN + 2)).state, State::Unmarked, "beyond the cap the walk has ended");
    }

    /// A card that names no mission (a relay's list after a restart) is found through the links too, and a child's relay events are masked.
    #[test]
    fn a_card_with_no_mission_is_withheld_for_a_child_of_a_marked_task_and_not_for_an_unlinked_one() {
        let d = ws();
        let (parent, child) = (task_key("m_3", "t_0001"), task_key("m_3", "t_0002"));
        link_task(d.path(), &child, &parent).unwrap();
        mark(d.path(), &parent, "entry-1", 1).unwrap();
        assert!(card_withheld(d.path(), "ap_od5_nm", "t_0002"), "the child's card, with no mission known");
        assert!(!card_withheld(d.path(), "ap_od5_nm2", "t_0003"), "an unlinked task of another id");
        let mut no_mission = card_event("t_0002", "ap_od5_nm3");
        no_mission.as_object_mut().unwrap().remove("mission_id");
        assert!(mask_event_for_relay(d.path(), &no_mission).is_some(), "an event that names no mission, for the child");
        for ev in [done_event("t_0002", &format!("done with {CANARY}")), failed_event("t_0002"), review_event("t_0002")] {
            let mut ev = ev;
            ev["mission_id"] = json!("m_3");
            let m = mask_event_for_relay(d.path(), &ev).unwrap_or_else(|| panic!("{} was not masked for the child", ev["type"]));
            assert!(!m.to_string().contains(CANARY));
        }
    }

    /// The link is made by the daemon from its own context: a timer's setter is noted when the timer is set, the wake is linked to it before it
    /// starts, an operator-set timer (nothing noted) makes no link, a link is written once, and a link names two valid keys.
    #[test]
    fn a_woken_task_is_linked_to_the_task_that_set_its_timer_and_an_operator_timer_makes_no_link() {
        let d = ws();
        let (setter, woken) = (task_key("m_1", "t_0001"), task_key("m_1", "t_0005"));
        note_timer_setter(d.path(), "tm_0001", &setter).unwrap();
        assert!(link_woken_task(d.path(), "tm_0001", &woken));
        assert_eq!(parent_of(d.path(), &woken), Ok(Some(setter.clone())));
        // an every-N timer wakes again: another task, the same setter
        assert!(link_woken_task(d.path(), "tm_0001", &task_key("m_1", "t_0006")));
        assert_eq!(parent_of(d.path(), &task_key("m_1", "t_0006")), Ok(Some(setter.clone())));
        // an operator-set timer has no note and so no link
        assert!(!link_woken_task(d.path(), "tm_0002", &task_key("m_1", "t_0007")));
        assert_eq!(parent_of(d.path(), &task_key("m_1", "t_0007")), Ok(None));
        // write-once: a second link for the same child does not change the first
        link_task(d.path(), &woken, &task_key("m_1", "t_0099")).unwrap();
        assert_eq!(parent_of(d.path(), &woken), Ok(Some(setter)));
        // names, not text
        assert!(link_task(d.path(), "../x", &woken).is_err() && link_task(d.path(), &woken, &woken).is_err() && note_timer_setter(d.path(), "tm 1", &woken).is_err());
        // the links directory is not a mark: nothing in it is listed as one, and the console lists no link
        assert!(live(d.path()).is_empty());
    }

    /// ATTACK: another MCP session of the same host. SUCCEEDS the same way: a mark is a session's own (`host-<session>`), a second session of
    /// the same tool in the same folder reads the first session's handoff or note unmarked.
    #[test]
    fn attack_a_second_session_of_the_same_host_is_not_marked_by_the_first() {
        let d = ws();
        mark(d.path(), "host-aaaaaaaa", "host-aaaaaaaa/n1", 1).unwrap();
        let mut ev = card_event("t_0001", "ap_m4_s2");
        ev["mission_id"] = json!("host-bbbbbbbb");
        assert!(mask_event_for_relay(d.path(), &ev).is_none(), "SUCCEEDS: session b is a different unit");
        ev["mission_id"] = json!("host-aaaaaaaa");
        assert!(mask_event_for_relay(d.path(), &ev).is_some());
    }

    /// M4 found, and pinned, that a mark on the host SESSION's mission (`host-<session>`, set by the host's search) withheld the relay text of the
    /// tasks that session started but did not hold their Outbound and Connector sends, because those read `<mission>/<task>` only. OD-5 (the
    /// operator, 2026-11-12) makes the session one scope: the same fixtures now hold on all four channels. Exposure that remains: a
    /// session's tasks run read-only at taint ceiling L0, where the taint gate already refuses an Outbound request.
    #[test]
    fn a_mission_level_mark_withholds_relay_text_and_holds_a_helper_tasks_outbound_and_connector_send() {
        let d = ws();
        mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 1).unwrap();
        let helper = task_scope(d.path(), Some("host-abcd1234"), "t_0001");
        assert!(crate::egress::check(&scope_record(&helper.state), crate::library::Channel::Outbound).is_err(), "the helper task's Outbound send is held");
        assert!(crate::egress::check(&scope_record(&helper.state), crate::library::Channel::Connector).is_err(), "and its Connector send");
        let mut ev = card_event("t_0001", "ap_m4_helper");
        ev["mission_id"] = json!("host-abcd1234");
        assert!(mask_event_for_relay(d.path(), &ev).is_some(), "the same task's relay text is withheld, as before");
    }

    /// ATTACK: key collisions and odd keys. `t_0001` repeats across missions (an event that names its mission is held only for its own);
    /// a mission or task name a key cannot hold is hashed (`x:`), not made unreadable, and the hashed mark withholds that mission's event;
    /// `file_of` maps `/` to `__` and `:` to `_`, so two keys can share a file, which can only over-hold, and a clear of one lifts both.
    #[test]
    fn attack_colliding_and_odd_keys_over_hold_and_never_under_hold() {
        let d = ws();
        mark(d.path(), &task_key("m_9", "t_0001"), "entry-1", 1).unwrap();
        let mut other = card_event("t_0001", "ap_m4_k1");
        other["mission_id"] = json!("m_2");
        assert!(mask_event_for_relay(d.path(), &other).is_none(), "another mission's t_0001 is untouched");
        let mut no_mission = card_event("t_0001", "ap_m4_k2");
        no_mission.as_object_mut().unwrap().remove("mission_id");
        assert!(mask_event_for_relay(d.path(), &no_mission).is_some(), "an event that names no mission is held when any mission's task of that id is marked");
        // a hashed key
        let odd = task_key("a mission.with odd chars", "t_0001");
        assert!(odd.starts_with("x:"));
        mark(d.path(), &odd, "entry-2", 1).unwrap();
        let mut ev = card_event("t_0001", "ap_m4_k3");
        ev["mission_id"] = json!("a mission.with odd chars");
        assert!(mask_event_for_relay(d.path(), &ev).is_some(), "the hashed key is found from the mission and task the event names");
        // a key that cannot be a key can never be marked, and an event naming one is just unmarked
        assert!(mark(d.path(), "../etc", "entry-3", 1).is_err() && mark(d.path(), "a/../b", "entry-3", 1).is_err() && mark(d.path(), "a//b", "entry-3", 1).is_err());
        // two keys, one file: a mark under `a/b` is seen under `a__b` (over-hold), and the clear of either lifts both
        mark(d.path(), "m_5/t_7", "entry-4", 1).unwrap();
        assert!(matches!(state(d.path(), "m_5__t_7"), State::Marked { .. }));
        clear(d.path(), "m_5__t_7", 2).unwrap();
        assert_eq!(state(d.path(), "m_5/t_7"), State::Unmarked);
    }

    /// ATTACK: a task that renames itself. A mark is keyed by the mission and task ids the daemon chose; a name, a thread or any text field of an
    /// event plays no part in the lookup.
    #[test]
    fn attack_a_task_that_renames_itself_keeps_its_mark() {
        let d = ws();
        mark(d.path(), &task_key("m_1", "t_0001"), "entry-1", 1).unwrap();
        let mut ev = card_event("t_0001", "ap_m4_rn");
        ev["mission_id"] = json!("m_1");
        ev["payload"]["name"] = json!("t_0099");
        ev["payload"]["thread"] = json!("a harmless thread");
        ev["payload"]["role"] = json!("scout");
        assert!(mask_event_for_relay(d.path(), &ev).is_some());
        assert_eq!(task_key("m_1", "t_0001"), "m_1/t_0001", "a pure function of the two ids");
    }

    /// ATTACK: a relay that was offline when the card was raised, with the mark kept on the MISSION (a host session's). The stream never ran for
    /// the card, so nothing noted it as withheld, and a card record names its task and not its mission: before the fix the relay's card list on
    /// reconnecting and its `allow` both missed the mark. The engine now notes the card's mission when it raises it.
    #[test]
    fn attack_a_relay_that_reconnects_after_the_card_was_raised_still_finds_a_mission_level_mark() {
        let d = ws();
        mark(d.path(), "host-abcd1234", "host-abcd1234/n1", 1).unwrap();
        let rec = |id: &str| json!({"approval_id": id, "task_id": "t_0003", "tier": "amber", "tool": "write_file", "pattern": "p", "summary": CANARY, "payload": {"content": CANARY}, "payload_blake3": "ab"});
        assert!(!card_withheld(d.path(), "ap_m4_off_a", "t_0003"), "a card the engine never noted is not linked to any mission (the control)");
        note_card_raised("ap_m4_off_a", "host-abcd1234");
        note_card_raised("ap_m4_off_b", "m_other");
        assert!(card_withheld(d.path(), "ap_m4_off_a", "t_0003"), "the engine noted the card's mission when it raised it");
        assert!(!card_withheld(d.path(), "ap_m4_off_b", "t_0003"), "a card of an unmarked mission is untouched");
        let out = mask_pending_for_relay(d.path(), json!([rec("ap_m4_off_a"), rec("ap_m4_off_b")]));
        assert_eq!(out[0]["summary"], RELAY_PHRASE);
        assert_eq!(out[1]["summary"], CANARY);
        // and a mark kept under `<mission>/<task>` is found the same way
        mark(d.path(), &task_key("m_7", "t_0003"), "entry-1", 1).unwrap();
        note_card_raised("ap_m4_off_c", "m_7");
        assert!(card_withheld(d.path(), "ap_m4_off_c", "t_0003"));
    }

    /// ATTACK: the card list for a card that carries no mission, with the mark UNREADABLE. Before the fix the any-mission lookup listed only marks
    /// it could read, so a damaged `<mission>/<task>` record was invisible to the list and to `allow`: fail-open on the one place the spec says
    /// fail closed. The lookup now goes by file name.
    #[test]
    fn attack_an_unreadable_mark_under_a_mission_and_task_key_fails_closed_for_a_card_that_names_no_mission() {
        let d = ws();
        mark(d.path(), &task_key("m_9", "t_0001"), "entry-1", 1).unwrap();
        std::fs::write(file_of(d.path(), "m_9/t_0001"), b"{not json").unwrap();
        assert_eq!(task_state_in_any_mission(d.path(), "t_0001"), State::Unreadable);
        assert!(card_withheld(d.path(), "ap_m4_unr", "t_0001"), "a damaged mark withholds the card's text from the list");
        assert!(!card_withheld(d.path(), "ap_m4_unr2", "t_0002"), "another task is not affected");
        let ev = json!({"type": "task.failed", "task_id": "t_0001", "payload": {"error": CANARY}});
        assert!(!mask_event_for_relay(d.path(), &ev).unwrap().to_string().contains(CANARY), "and an event that names no mission");
    }

    /// ATTACK: a mark cleared while a card is in flight. A card the stream withheld stays withheld (and cannot be allowed by a relay reply)
    /// after the clear, because the relay may already have been sent the phrase and the operator never saw the text there; a card raised
    /// AFTER the clear is sent as it always was. Both are conservative.
    #[test]
    fn attack_a_clear_mid_flight_keeps_the_withheld_card_withheld_and_frees_the_next_one() {
        let d = ws();
        mark(d.path(), "t_0001", "entry-1", 1).unwrap();
        assert!(mask_event_for_relay(d.path(), &card_event("t_0001", "ap_m4_mid_a")).is_some());
        clear(d.path(), "t_0001", 2).unwrap();
        assert!(card_withheld(d.path(), "ap_m4_mid_a", "t_0001"), "the card the stream withheld stays withheld");
        assert!(mask_event_for_relay(d.path(), &card_event("t_0001", "ap_m4_mid_b")).is_none(), "the next card is not");
        assert!(!card_withheld(d.path(), "ap_m4_mid_b", "t_0001"));
        // and a mark written AFTER a card was raised withholds it from the list, because the list looks at the mark when it is asked
        mark(d.path(), "t_0002", "entry-2", 3).unwrap();
        assert!(card_withheld(d.path(), "ap_m4_mid_c", "t_0002"));
    }

    /// ATTACK: reach the mark from outside the console. Nothing a seat, a host, a relay or the narrator can call clears or writes a mark; the
    /// files sit under `local/`, which no role may read or write through the file tools.
    #[test]
    fn attack_nothing_but_the_consoles_clear_reaches_a_mark_and_no_role_can_touch_its_file() {
        // the methods each lesser scope answers do not include the two console methods
        for list in [&delphi_core::server::HOST_METHODS[..], &delphi_core::server::RELAY_METHODS[..], &delphi_core::server::NARRATOR_METHODS[..]] {
            assert!(!list.iter().any(|m| m.starts_with("private_read.")), "{list:?}");
        }
        // and the daemon's method table gates both on the console scope
        let svc = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/service.rs")).unwrap();
        for f in ["async fn private_read_status", "async fn private_read_clear"] {
            let at = svc.find(f).expect(f);
            assert!(svc[at..at + 400].contains("need_console(&ctx)"), "{f} is console scope only");
        }
        // an agent's own file tools: the pathguard names `local/` as never readable, and the mark's directory is under it
        assert!(dir(Path::new("/ws")).starts_with("/ws/local/"), "the marks live under local/");
        let guard = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-core/src/pathguard.rs")).unwrap();
        assert!(guard.contains("for p in [\"run/\", \"state.db\", \"local/\""), "local/ is on the never-readable list of the path guard");
    }

    /// ATTACK: a card text that reaches a relay client by some path that is not one of the four masked kinds. The relay socket answers five
    /// methods (classified above); of them only `approval.list` and `approval.relay_resolve` return card data, and the first is masked. The
    /// hub sends a relay client only `RELAY_EVENTS`, and a relay client is not sent `agent.*` deltas, `task.output` or any other kind.
    #[test]
    fn attack_a_relay_client_is_sent_only_the_relay_kinds_and_the_card_list_is_masked() {
        let hub = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-core/src/hub.rs")).unwrap();
        let prod = &hub[..hub.find("#[cfg(test)]").unwrap_or(hub.len())];
        let sends = prod.matches("c.queue.push(").count();
        assert!(sends >= 2, "the publish step pushes to a queue");
        assert!(prod.contains("c.scope != Scope::Relay || RELAY_EVENTS.contains(&kind)"), "a relay client is sent only the relay kinds");
        let svc = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/service.rs")).unwrap();
        assert!(svc.contains("\"approval.list\" => Ok(svc.pending_json_for(ctx.scope))"), "the card list goes through the scope-aware masker");
    }

    /// `specs/103` §4.3: the unit of the mark's lookup for a Delphi task is `<mission>/<task>` and the relay filter counts the bare task, the
    /// bare mission and the pair; a mark under any of the three withholds, and a clear lifts only what was read.
    #[test]
    fn attack_the_filter_finds_a_mark_under_each_of_the_three_keys() {
        for key in ["t_0001", "m_1", "m_1/t_0001"] {
            let d = ws();
            mark(d.path(), key, "entry-1", 1).unwrap();
            let mut ev = card_event("t_0001", "ap_m4_3k");
            ev["mission_id"] = json!("m_1");
            assert!(mask_event_for_relay(d.path(), &ev).is_some(), "{key}");
            clear(d.path(), key, 2).unwrap();
            assert!(mask_event_for_relay(d.path(), &ev).is_none(), "{key} cleared");
        }
    }
}
