//! The exchange record (`specs/102` §3), slice S3: one operator prompt, redacted, with a capped summary, a typed outcome and the
//! facts the daemon saw. **Dormant**: `[history] enabled` is `false` until slice S8, and while it is, nothing here writes a file, makes
//! the key, or holds a prompt.
//!
//! * **The daemon writes what it saw.** A record is written only at a **trigger the daemon itself observed** (§3.2), by the [`Tap`]
//!   that the mission's event path calls. A host cannot create, edit or delete one, and cannot set `kind`, `observed`, `private`,
//!   `chain` or `hash` ([`parse_host_exchange`] refuses a request that names them). `kind` is computed here: `observed` only for a
//!   prompt the daemon saw, `resolved`/`reported` for a host's report (the S1 check), never `observed` for relayed text (§11 item 13).
//! * **Fixed fields, no room for more** (§3.1). There is no field for a tool result, thinking, assistant dialogue, file contents or
//!   pasted text: a pasted block is a keyed hash and a length.
//! * **Append-only and hash-chained.** Every hash is an HMAC under the local key (`history_pointer`). A record's `chain` is the
//!   previous record's hash; [`read`] verifies the chain on every read and marks a record that fails, and every record after it,
//!   `ChainBroken` (§11 item 14): it is left out of [`Read::counted`], never shown as valid, and nothing is repaired or deleted.
//! * **Private, and out of every model prompt by default** (§7 item 2): [`release`] and [`for_prompt`] go through the one egress check
//!   (`Channel::History`). `[history] to_models` defaults to `deny`; even when it is `allow`, a seat of another family, or of a family
//!   the daemon does not know, gets nothing.
//! * **Nothing here is a criterion for trust, promotion, tool access or approval**, and the outcome is a label that is never counted
//!   (§11 items 1, 2). Nothing here reaches the network or calls a model.
//!
//! Not in this slice: the settings commands, purge, delete, the install question (S4), the console view (S5), the citation check (S6).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use delphi_core::event::EventEnvelope;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::history_pointer::{span_hmac, Pointer, PointerKind};
use crate::history_redact::{scrub, scrub_field, Input, PastedBlock, Typing};
use crate::history_report::safe_id;
use crate::library::Channel;

pub const STORE_DIR: &str = "local/history";
pub const FILE: &str = "records.jsonl";
const GENESIS: &str = "genesis";
const MB: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------------------------------------------------------
// Settings

/// `[history] to_models`: whether record text may enter a model prompt at all (§7 item 2). Not 055's `private_to_models`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToModels {
    Deny,
    Allow,
}

/// `[history]`. The default is **off** (§7 item 10, OD-8): only slice S8 changes it, and a test fails if it is on while the egress
/// check is not registered for the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    /// The paste rule's length threshold (§3.4, a hypothesis).
    pub paste_chars: usize,
    pub to_models: ToModels,
    /// The store's cap per project, in megabytes (§7 item 9, a hypothesis). At the cap the daemon stops adding and says so.
    pub budget_mb: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: false, paste_chars: crate::history_redact::DEFAULT_PASTE_CHARS, to_models: ToModels::Deny, budget_mb: 50 }
    }
}

impl Settings {
    pub const PASTE_CHARS_RANGE: (usize, usize) = (50, 5000);
    pub const BUDGET_MB_RANGE: (u64, u64) = (1, 1024);

    /// Read `[history]` from a parsed `config.toml`. A number outside its range is clamped and reported; an unknown key or a wrong
    /// type is a returned problem, for the boot notices. A wrong value never turns the feature on.
    pub fn parse(root: &toml::Value) -> (Settings, Vec<String>) {
        let mut s = Settings::default();
        let mut problems = Vec::new();
        let Some(t) = root.get("history").and_then(|v| v.as_table()) else { return (s, problems) };
        for k in t.keys() {
            if !["enabled", "paste_chars", "to_models", "budget_mb"].contains(&k.as_str()) {
                problems.push(format!("[history] {k}: not a setting (enabled, paste_chars, to_models, budget_mb)"));
            }
        }
        if let Some(v) = t.get("enabled") {
            match v.as_bool() {
                Some(b) => s.enabled = b,
                None => problems.push("[history] enabled: expected true or false".into()),
            }
        }
        if let Some(v) = t.get("paste_chars") {
            match v.as_integer() {
                Some(n) => {
                    let (lo, hi) = Self::PASTE_CHARS_RANGE;
                    let c = (n.max(0) as usize).clamp(lo, hi);
                    if c as i64 != n {
                        problems.push(format!("[history] paste_chars: {n} is outside {lo}..={hi}; using {c}"));
                    }
                    s.paste_chars = c;
                }
                None => problems.push("[history] paste_chars: expected a whole number".into()),
            }
        }
        if let Some(v) = t.get("budget_mb") {
            match v.as_integer() {
                Some(n) => {
                    let (lo, hi) = Self::BUDGET_MB_RANGE;
                    let c = (n.max(0) as u64).clamp(lo, hi);
                    if c as i64 != n {
                        problems.push(format!("[history] budget_mb: {n} is outside {lo}..={hi}; using {c}"));
                    }
                    s.budget_mb = c;
                }
                None => problems.push("[history] budget_mb: expected a whole number".into()),
            }
        }
        if let Some(v) = t.get("to_models") {
            match v.as_str() {
                Some("deny") => s.to_models = ToModels::Deny,
                Some("allow") => s.to_models = ToModels::Allow,
                _ => problems.push("[history] to_models: expected \"deny\" or \"allow\"; using \"deny\"".into()),
            }
        }
        (s, problems)
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// The record

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Merge,
    ResearchRun,
    Panel,
    Council,
    Artifact,
}

/// A label from the daemon's evidence (§3.1), never a finding: `Resolved` proves only that the text exists in the transcript (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Observed,
    Resolved,
    Reported,
}

/// The closed enum of `specs/084` §3. It is a label: no count, ratio or trend of it is computed anywhere (§11 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeValue {
    Done,
    Partly,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// The daemon saw the result.
    Observed,
    /// A host said so (`untrusted/self_reported`).
    SelfReported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub value: OutcomeValue,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptField {
    pub text: String,
    pub cut: bool,
    /// The keyed hash of the full text, present only when `cut`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_hash: Option<String>,
}

/// An entry of the artifact prompt log (`specs/070` §3) that holds the prompt: the exchange record references it and holds no prompt text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// The log's path relative to the workspace.
    pub log: String,
    /// The entry's own hash (its id).
    pub entry: String,
    pub artifact: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_hash: Option<String>,
}

/// The host's account of its own reply (§3.3): plain text, each field at most 200 characters, labelled `self_reported`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub asked: String,
    pub did: String,
    pub open: String,
}

/// Counts and ids the daemon saw: PR numbers, event ids, council numbers. Ids pass `safe_id`; nothing here is event text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    pub ids: Vec<String>,
    pub counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub at: String,
    pub project: String,
    pub mission: String,
    pub trigger: Trigger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    pub pasted: Vec<PastedBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<Reply>,
    pub outcome: Outcome,
    pub observed: Observed,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<Pointer>,
    pub private: bool,
    pub chain: String,
    pub hash: String,
}

fn canonical(r: &Record) -> String {
    let mut c = r.clone();
    c.hash = String::new();
    serde_json::to_string(&c).expect("a record serializes")
}

/// The record's own keyed hash (§3.4): HMAC over its canonical form with `hash` empty.
pub fn record_hash(key: &[u8], r: &Record) -> String {
    span_hmac(key, &canonical(r))
}

// ---------------------------------------------------------------------------------------------------------------------------
// What a trigger is: a function from the daemon's events to "this prompt is concluded" (§3.2), with no model in it

/// The five triggers and the event each listens to (S3 names them from the code it read):
/// * research run: `research.finished`, written by the daemon's own research job (`service.rs`);
/// * panel: `council.verdict` with `venue: panel`; Council: `council.verdict` with any other venue (`council.rs`);
/// * work artifact: an entry the daemon appended to the artifact prompt log (`engine.rs` `log_artifact_prompts`; not an event kind, a call);
/// * merge: a `merged` row of the merge ledger (`delphi merge`, spec 094). The ledger is written by the `delphi merge` process, which
///   has no daemon connection and must not write the daemon-only store (§11 item 5), so the daemon reads the ledger itself on its
///   own timer (`history_merge`, `EngineService::merge_ledger_tick`) and calls [`Tap::on_merge_row`].
pub fn trigger_of(e: &EventEnvelope) -> Option<Trigger> {
    match e.kind.as_str() {
        "research.finished" => Some(Trigger::ResearchRun),
        "council.verdict" => Some(if e.payload["venue"].as_str() == Some("panel") { Trigger::Panel } else { Trigger::Council }),
        _ => None,
    }
}

pub fn trigger_of_ledger_row(row: &Value) -> Option<Trigger> {
    (row["kind"].as_str() == Some("merged")).then_some(Trigger::Merge)
}

fn count_of(p: &Value, k: &str) -> Option<u64> {
    p[k].as_u64()
}

/// What the daemon saw at the event: counts and a safe id only.
fn observed_of(e: &EventEnvelope) -> Observed {
    let mut o = Observed::default();
    if let Some(id) = safe_id(&format!("ev_{}", e.seq)) {
        o.ids.push(id);
    }
    for k in ["new", "updated", "held", "n"] {
        if let Some(n) = count_of(&e.payload, k) {
            o.counts.insert(k.to_string(), n);
        }
    }
    o
}

fn outcome_of(e: &EventEnvelope) -> Outcome {
    let seen = |value| Outcome { value, provenance: Provenance::Observed };
    match e.kind.as_str() {
        "research.finished" => match e.payload["outcome"].as_str() {
            Some("failed") | Some("cancelled") => seen(OutcomeValue::Stopped),
            Some("done") | None if e.payload.get("new").is_some() || e.payload.get("updated").is_some() => seen(OutcomeValue::Done),
            _ => Outcome { value: OutcomeValue::Unknown, provenance: Provenance::Observed },
        },
        "council.verdict" => seen(if e.payload["escalated"].as_bool() == Some(true) { OutcomeValue::Partly } else { OutcomeValue::Done }),
        _ => Outcome { value: OutcomeValue::Unknown, provenance: Provenance::Observed },
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// Building a record

pub enum PromptIn {
    /// A prompt the daemon saw (`kind: observed`). Only the daemon's own task start makes one.
    Observed { text: String, marked: Vec<String>, typing: Typing },
    /// A host's report of the operator's prompt (`resolved` when the S1 check matches the transcript, else `reported`).
    Reported { text: String, marked: Vec<String>, session_id: String, transcript: Option<String> },
    /// The prompt is held by the artifact prompt log (§3.5): the record references its entries and holds no prompt text.
    Artifacts(Vec<ArtifactRef>),
}

pub struct Draft {
    pub trigger: Trigger,
    pub mission: String,
    pub project: String,
    pub at: String,
    pub prompt: PromptIn,
    pub reply: Option<Reply>,
    pub outcome: Outcome,
    pub observed: Observed,
}

/// Build the record (without `id`, `chain` and `hash`, which the append sets).
pub fn build(key: &[u8], settings: &Settings, d: Draft) -> Record {
    let (prompt, pasted, artifacts, kind, pointer) = match d.prompt {
        PromptIn::Observed { text, marked, typing } => {
            let s = scrub(key, &Input { text: &text, marked: &marked, typing, paste_chars: settings.paste_chars });
            (Some(PromptField { text: s.text, cut: s.cut, full_hash: s.full_hash }), s.pasted, Vec::new(), Kind::Observed, None)
        }
        PromptIn::Reported { text, marked, session_id, transcript } => {
            // A host's report gets the paste rule with nothing known of how it was typed (§3.4), and the S1 check decides the label.
            let s = scrub(key, &Input { text: &text, marked: &marked, typing: Typing::NotSeen, paste_chars: settings.paste_chars });
            let matched = transcript.as_deref().map(|t| crate::history_pointer::match_reported(key, &session_id, t, &text, PointerKind::OperatorWords));
            let (kind, pointer) = match matched {
                Some(crate::history_pointer::Matched::Resolved(p)) => (Kind::Resolved, Some(p)),
                _ => (Kind::Reported, None),
            };
            (Some(PromptField { text: s.text, cut: s.cut, full_hash: s.full_hash }), s.pasted, Vec::new(), kind, pointer)
        }
        PromptIn::Artifacts(refs) => (None, Vec::new(), refs, Kind::Observed, None),
    };
    Record {
        id: String::new(),
        at: d.at,
        project: d.project,
        mission: d.mission,
        trigger: d.trigger,
        prompt,
        artifacts,
        pasted,
        reply: d.reply,
        outcome: d.outcome,
        observed: d.observed,
        kind,
        pointer,
        private: true,
        chain: String::new(),
        hash: String::new(),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// What a host may say (§3.3, §10 item 1): the exchange fields, and nothing the daemon computes

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostExchange {
    pub prompt: Option<String>,
    pub session_id: Option<String>,
    pub reply: Reply,
    pub outcome: Option<OutcomeValue>,
}

/// Read a host's optional exchange fields (`prompt`, `session_id`, `asked`, `did`, `open`, `outcome`). A request that names a field
/// the daemon computes (`kind`, `observed`, `private`, `chain`, `hash`, `pointer`, `id`, `at`, `project`, `mission`, `trigger`) is
/// refused, so a host cannot claim `observed` for a prompt the daemon did not see (§11 item 12); a list of outcomes is refused
/// (one outcome per exchange, §10 item 2). This makes no record: only a trigger the daemon observed does (§11 item 2).
pub fn parse_host_exchange(p: &Value) -> Result<HostExchange, String> {
    const DAEMON_ONLY: [&str; 11] = ["kind", "observed", "private", "chain", "hash", "pointer", "id", "at", "project", "mission", "trigger"];
    let Some(obj) = p.as_object() else { return Err("the exchange fields are an object".into()) };
    for k in DAEMON_ONLY {
        if obj.contains_key(k) {
            return Err(format!("`{k}` is set by the daemon, not by a host"));
        }
    }
    let text = |k: &str| -> Result<Option<String>, String> {
        match obj.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(format!("`{k}` is text")),
        }
    };
    let outcome = match obj.get("outcome") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(match s.as_str() {
            "done" => OutcomeValue::Done,
            "partly" => OutcomeValue::Partly,
            "stopped" => OutcomeValue::Stopped,
            "unknown" => OutcomeValue::Unknown,
            _ => return Err("`outcome` is one of done, partly, stopped, unknown".into()),
        }),
        Some(_) => return Err("one outcome per exchange: `outcome` is one word".into()),
    };
    let field = |k: &str| -> Result<String, String> { Ok(text(k)?.map(|t| scrub_field(&t)).unwrap_or_default()) };
    let session_id = text("session_id")?.filter(|s| crate::history_pointer::valid_session_id(s));
    Ok(HostExchange { prompt: text("prompt")?, session_id, reply: Reply { asked: field("asked")?, did: field("did")?, open: field("open")? }, outcome })
}

// ---------------------------------------------------------------------------------------------------------------------------
// The store: append-only, chained, verified on read

pub fn store_dir(ws_dir: &Path, project: &str) -> PathBuf {
    ws_dir.join(STORE_DIR).join(project)
}

pub fn store_path(ws_dir: &Path, project: &str) -> PathBuf {
    store_dir(ws_dir, project).join(FILE)
}

/// The per-project switch (§7 item 5): `local/projects/<id>/history.off`. Written by a later slice; honoured here.
pub fn project_off(ws_dir: &Path, project: &str) -> bool {
    ws_dir.join("local/projects").join(project).join("history.off").exists()
}

/// What an append did. Everything but `Written` writes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Appended {
    Written(Box<Record>),
    /// `[history] enabled` is off (the default until S8): nothing is written, and no key is made.
    Dormant,
    /// The project's own switch is off.
    ProjectOff,
    /// The store is at its budget: the daemon stops adding and says so (§7 item 9). Nothing is evicted.
    Budget,
    /// Not written, with a fixed reason (never record text).
    Failed(&'static str),
}

static APPEND_GATE: Mutex<()> = Mutex::new(());

fn last_hash(path: &Path) -> Result<(String, usize), &'static str> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((GENESIS.into(), 0)),
        Err(_) => return Err("the history store cannot be read"),
    };
    let n = text.lines().count();
    match text.lines().last() {
        None => Ok((GENESIS.into(), 0)),
        Some(l) => serde_json::from_str::<Record>(l).map(|r| (r.hash, n)).map_err(|_| "the last record cannot be read; nothing is repaired"),
    }
}

/// Append one record. Dormant, project-off and budget are answered before anything is written or made.
pub(crate) fn append(ws_dir: &Path, settings: &Settings, d: Draft) -> Appended {
    if !settings.enabled {
        return Appended::Dormant;
    }
    if project_off(ws_dir, &d.project) {
        return Appended::ProjectOff;
    }
    let _gate = APPEND_GATE.lock().unwrap_or_else(|p| p.into_inner());
    let path = store_path(ws_dir, &d.project);
    if std::fs::metadata(&path).map(|m| m.len() >= settings.budget_mb * MB).unwrap_or(false) {
        return Appended::Budget;
    }
    let Ok(key) = crate::history_pointer::load_or_create_key(ws_dir) else { return Appended::Failed("the history key is not usable") };
    let (prev, n) = match last_hash(&path) {
        Ok(x) => x,
        Err(why) => return Appended::Failed(why),
    };
    let mut r = build(&key, settings, d);
    r.id = format!("ex_{:04}", n + 1);
    r.chain = prev;
    r.hash = record_hash(&key, &r);
    let Some(dir) = path.parent() else { return Appended::Failed("the history store has no directory") };
    if std::fs::create_dir_all(dir).is_err() {
        return Appended::Failed("the history store could not be made");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [ws_dir.join(STORE_DIR), dir.to_path_buf()] {
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
        }
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut line = serde_json::to_string(&r).expect("a record serializes");
    line.push('\n');
    match opts.open(&path).and_then(|mut f| f.write_all(line.as_bytes()).and_then(|_| f.sync_all())) {
        Ok(()) => Appended::Written(Box::new(r)),
        Err(_) => Appended::Failed("the record could not be written"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Verified,
    /// This record, or one before it, failed the chain check. It is shown as `chain broken` and counted nowhere.
    ChainBroken,
    /// There is no key to check with.
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub record: Record,
    pub status: Status,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Read {
    pub records: Vec<Checked>,
    /// The 0-based index of the first record that failed, if any.
    pub broken_at: Option<usize>,
}

impl Read {
    /// The records that verified: the only ones any report or count may use (§11 item 14).
    pub fn counted(&self) -> impl Iterator<Item = &Record> {
        self.records.iter().filter(|c| c.status == Status::Verified).map(|c| &c.record)
    }
}

/// Read the store and verify the chain end to end. A line that does not parse, a record whose hash does not match, a `chain` that
/// is not the previous record's hash, or a record that is not `private` breaks the chain there and for every record after it.
/// Nothing is repaired or deleted.
pub fn read(ws_dir: &Path, project: &str) -> Result<Read, String> {
    let text = match std::fs::read_to_string(store_path(ws_dir, project)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Read::default()),
        Err(_) => return Err("the history store cannot be read".into()),
    };
    let key = crate::history_pointer::load_key(ws_dir)?;
    let mut out = Read::default();
    let mut prev = GENESIS.to_string();
    for (i, line) in text.lines().enumerate() {
        let Ok(record) = serde_json::from_str::<Record>(line) else {
            // A line that cannot be read has no record to show: the chain breaks here, and what follows is flagged below.
            out.broken_at.get_or_insert(i);
            continue;
        };
        let status = match &key {
            None => Status::Unverifiable,
            Some(k) => {
                // After a break nothing verifies: the link that vouched for what follows is gone.
                let ok = out.broken_at.is_none() && record.private && record.chain == prev && record_hash(k, &record) == record.hash;
                if !ok {
                    out.broken_at.get_or_insert(i);
                }
                if ok { Status::Verified } else { Status::ChainBroken }
            }
        };
        prev = record.hash.clone();
        out.records.push(Checked { record, status });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------------------
// The one egress check (§7 item 2, `specs/098` §12 item 16)

/// Which seat would receive record text. Unknown means deny.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// A seat of the host's own provider family.
    SameFamily,
    Foreign,
    Unknown,
}

fn meta_of(r: &Record) -> Value {
    json!({"private": r.private})
}

/// May record text leave the store by any outbound channel (a bundle, an export, a relay message, a debug bundle)? Never: every
/// record is `private`, and the one check refuses a private entry.
pub fn release(r: &Record) -> Result<(), String> {
    crate::egress::check(&meta_of(r), Channel::History)
}

/// The prompt text of a record for a model prompt, or the reason it may not be placed there. `[history] to_models` defaults to
/// `deny`; with `allow`, only a seat of the host's own family gets it, and a seat of another or an unknown family never does.
pub fn for_prompt(settings: &Settings, seat: Seat, r: &Record) -> Result<String, String> {
    crate::egress::check_for_seat(&meta_of(r), seat != Seat::SameFamily, settings.to_models == ToModels::Allow)?;
    Ok(r.prompt.as_ref().map(|p| p.text.clone()).unwrap_or_default())
}

// ---------------------------------------------------------------------------------------------------------------------------
// The tap: how the mission's event path and the engine's artifact log reach the store

struct Pending {
    task_id: String,
    text: String,
    concluded: bool,
}

/// A trigger the daemon saw while it held no operator prompt of its own (the usual case in a host's mission: the operator typed the
/// prompt into the host, not into the daemon). It is the daemon's own observation, kept in memory only, so that the host's summary
/// of that exchange can complete it (§3.3). It is never written without a host report of the prompt, and a host's report alone
/// never makes a record (§10 item 2): both must exist.
struct Awaiting {
    trigger: Trigger,
    outcome: Outcome,
    observed: Observed,
    at: String,
    unix: i64,
}

/// At most this many unattributed triggers are remembered; the oldest is dropped. A hypothesis.
pub const AWAITING_MAX: usize = 8;
/// An unattributed trigger older than this (seconds) is forgotten, so a late summary cannot be bound to a trigger of another
/// exchange. A hypothesis, to calibrate from use (OD-3).
pub const AWAITING_SECS: i64 = 3600;
/// A transcript file larger than this is not read to check a reported prompt (the record is then `reported`).
pub const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;

/// What the host's exchange fields did (the answer to `record_session_summary`, with fixed words only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostReport {
    /// `[history] enabled` is off: the exchange fields were ignored and nothing was held or made.
    Off,
    /// The call carried none of the exchange fields.
    Absent,
    /// The fields were refused, with the reason.
    Refused(String),
    /// The daemon has seen no trigger to complete (a host cannot create a record, §10 item 2): nothing was written.
    NoTrigger,
    /// The report named no prompt: the operator's words are the point of a record (§2 item 1), so nothing was written.
    NoPrompt,
    /// The record was written (or the append said why it was not).
    Appended(Appended),
}

impl HostReport {
    /// A fixed word for the tool's answer; it carries no record text.
    pub fn word(&self) -> &'static str {
        match self {
            HostReport::Off => "off",
            HostReport::Absent => "absent",
            HostReport::Refused(_) => "refused",
            HostReport::NoTrigger => "no trigger seen",
            HostReport::NoPrompt => "no prompt given",
            HostReport::Appended(Appended::Written(_)) => "recorded",
            HostReport::Appended(Appended::Dormant) => "off",
            HostReport::Appended(Appended::ProjectOff) => "off for this project",
            HostReport::Appended(Appended::Budget) => "store at its budget",
            HostReport::Appended(Appended::Failed(_)) => "not written",
        }
    }
}

/// The keys of the exchange fields a host may send with a summary (§10 item 1): the prompt, where its words can be found, and the
/// reply in three short fields. Nothing the daemon computes is in this list.
pub const HOST_EXCHANGE_KEYS: [&str; 5] = ["prompt", "session_id", "asked", "did", "open"];

/// One per mission. It holds the last operator prompt the daemon saw (in memory, and only while history is on) and writes a record
/// when a trigger concludes it. A trigger with no prompt, or for a prompt already concluded, writes nothing: one record per prompt.
pub struct Tap {
    ws_dir: PathBuf,
    settings: Settings,
    project: String,
    pending: Mutex<Option<Pending>>,
    awaiting: Mutex<Vec<Awaiting>>,
    /// Where Claude Code's transcripts are searched for (S1): `~/.claude/projects` unless a test or a setting says otherwise.
    claude_root: Option<PathBuf>,
}

impl Tap {
    pub fn new(ws_dir: PathBuf, settings: Settings, project: &str) -> Tap {
        Tap {
            ws_dir,
            settings,
            project: safe_id(project).unwrap_or_else(|| "home".into()),
            pending: Mutex::new(None),
            awaiting: Mutex::new(Vec::new()),
            claude_root: crate::history_pointer::default_claude_root(),
        }
    }

    /// Search another root for transcripts (a test's temporary folder).
    pub fn with_claude_root(mut self, root: PathBuf) -> Tap {
        self.claude_root = Some(root);
        self
    }

    /// The project a mission belongs to: its `scope` file, else `home`.
    pub fn project_of(mission_dir: &Path) -> String {
        std::fs::read_to_string(mission_dir.join("scope")).ok().and_then(|s| safe_id(s.trim())).unwrap_or_else(|| "home".into())
    }

    /// An operator-started task's prompt, at the daemon's own task start. Nothing is held while history is off.
    pub fn note_operator_prompt(&self, task_id: &str, text: &str) {
        if !self.settings.enabled {
            return;
        }
        *self.pending.lock().unwrap() = Some(Pending { task_id: task_id.to_string(), text: text.to_string(), concluded: false });
    }

    fn conclude(&self, mission: &str, trigger: Trigger, outcome: Outcome, observed: Observed) -> Option<Appended> {
        let text = {
            let mut p = self.pending.lock().unwrap();
            match p.as_mut() {
                Some(x) if !x.concluded => {
                    x.concluded = true;
                    x.text.clone()
                }
                // A prompt the daemon already concluded: one record per prompt.
                Some(_) => return None,
                // No prompt of the daemon's own: the trigger is remembered (not written) so a host's summary can complete it.
                None => {
                    drop(p);
                    self.remember_trigger(trigger, outcome, observed);
                    return None;
                }
            }
        };
        let d = Draft {
            trigger,
            mission: safe_id(mission).unwrap_or_default(),
            project: self.project.clone(),
            at: delphi_core::time::now_rfc3339(),
            prompt: PromptIn::Observed { text, marked: Vec::new(), typing: Typing::NotSeen },
            reply: None,
            outcome,
            observed,
        };
        Some(self.report(append(&self.ws_dir, &self.settings, d)))
    }

    fn remember_trigger(&self, trigger: Trigger, outcome: Outcome, observed: Observed) {
        let mut a = self.awaiting.lock().unwrap();
        if a.len() >= AWAITING_MAX {
            a.remove(0);
        }
        let at = delphi_core::time::now_rfc3339();
        let unix = delphi_core::time::parse_rfc3339(&at).unwrap_or(0);
        a.push(Awaiting { trigger, outcome, observed, at, unix });
    }

    /// The host's exchange fields, sent with `record_session_summary` (§3.3, §10 item 1). The host cannot create a record: this
    /// completes the **latest trigger the daemon itself saw** and could not attribute to a prompt, with the prompt the host reports
    /// (`kind` `resolved` if the S1 check finds those words in the host's transcript as a user message, else `reported`) and the
    /// host's reply, labelled self-reported. The daemon's outcome and observed facts are the daemon's; a host's word never
    /// replaces them. With no such trigger, no prompt, or history off, nothing is written. `now_unix` is the daemon's clock.
    pub fn on_host_exchange(&self, mission: &str, p: &Value, now_unix: i64) -> HostReport {
        let Some(obj) = p.as_object() else { return HostReport::Absent };
        let mut fields = serde_json::Map::new();
        for k in HOST_EXCHANGE_KEYS {
            if let Some(v) = obj.get(k).filter(|v| !v.is_null()) {
                fields.insert(k.to_string(), v.clone());
            }
        }
        if fields.is_empty() {
            return HostReport::Absent;
        }
        // Dormant: the fields are ignored (not validated, held or written) and the host is told so.
        if !self.settings.enabled {
            return HostReport::Off;
        }
        let x = match parse_host_exchange(&Value::Object(fields)) {
            Ok(x) => x,
            Err(why) => return HostReport::Refused(why),
        };
        let Some(prompt) = x.prompt.filter(|t| !t.trim().is_empty()) else { return HostReport::NoPrompt };
        let a = {
            let mut list = self.awaiting.lock().unwrap();
            list.retain(|a| now_unix - a.unix <= AWAITING_SECS);
            match list.pop() {
                Some(a) => a,
                None => return HostReport::NoTrigger,
            }
        };
        let transcript = x.session_id.as_deref().and_then(|id| self.read_transcript(id));
        let reply = (!x.reply.asked.is_empty() || !x.reply.did.is_empty() || !x.reply.open.is_empty()).then_some(x.reply);
        // The daemon's outcome stands; a host's is used only where the daemon saw none, and it is labelled so.
        let outcome = match (a.outcome.value, x.outcome) {
            (OutcomeValue::Unknown, Some(v)) => Outcome { value: v, provenance: Provenance::SelfReported },
            _ => a.outcome,
        };
        let d = Draft {
            trigger: a.trigger,
            mission: safe_id(mission).unwrap_or_default(),
            project: self.project.clone(),
            at: a.at,
            prompt: PromptIn::Reported { text: prompt, marked: Vec::new(), session_id: x.session_id.unwrap_or_default(), transcript },
            reply,
            outcome,
            observed: a.observed,
        };
        HostReport::Appended(self.report(append(&self.ws_dir, &self.settings, d)))
    }

    /// The text of the transcript for `session_id`, if one is found under the root (S1's bounded search) and is a plain file of a
    /// size worth reading. The first readable candidate wins; the hash and the role decide the label, not this.
    fn read_transcript(&self, session_id: &str) -> Option<String> {
        let root = self.claude_root.as_deref()?;
        for path in crate::history_pointer::find_sessions(root, session_id) {
            if std::fs::metadata(&path).map(|m| m.len() > MAX_TRANSCRIPT_BYTES).unwrap_or(true) {
                continue;
            }
            if let Ok(t) = std::fs::read_to_string(&path) {
                return Some(t);
            }
        }
        None
    }

    fn report(&self, a: Appended) -> Appended {
        match &a {
            Appended::Budget => eprintln!("delphi: history: the store is at its budget; no record was added"),
            Appended::Failed(why) => eprintln!("delphi: history: {why}"),
            _ => {}
        }
        a
    }

    /// The mission's events just written. Does nothing unless history is on and an event is a trigger.
    pub fn on_events(&self, mission: &str, events: &[EventEnvelope]) -> Vec<Appended> {
        if !self.settings.enabled {
            return Vec::new();
        }
        events.iter().filter_map(|e| trigger_of(e).and_then(|t| self.conclude(mission, t, outcome_of(e), observed_of(e)))).collect()
    }

    /// Artifacts a tool call wrote, each already in the artifact prompt log: one record that references every entry, with no
    /// prompt text (§3.5). It concludes that task's prompt, so no second copy of its text is kept by another trigger.
    pub fn on_artifacts(&self, mission: &str, task_id: &str, refs: Vec<ArtifactRef>) -> Option<Appended> {
        if !self.settings.enabled || refs.is_empty() {
            return None;
        }
        {
            let mut p = self.pending.lock().unwrap();
            if let Some(x) = p.as_mut().filter(|x| x.task_id == task_id) {
                x.concluded = true;
            }
        }
        let mut observed = Observed::default();
        observed.counts.insert("artifacts".into(), refs.len() as u64);
        let d = Draft {
            trigger: Trigger::Artifact,
            mission: safe_id(mission).unwrap_or_default(),
            project: self.project.clone(),
            at: delphi_core::time::now_rfc3339(),
            prompt: PromptIn::Artifacts(refs),
            reply: None,
            outcome: Outcome { value: OutcomeValue::Done, provenance: Provenance::Observed },
            observed,
        };
        Some(self.report(append(&self.ws_dir, &self.settings, d)))
    }

    /// A `merged` row of the merge ledger (spec 094), read by the daemon's own timer: see [`trigger_of`].
    pub fn on_merge_row(&self, mission: &str, row: &Value) -> Option<Appended> {
        if !self.settings.enabled {
            return None;
        }
        let t = trigger_of_ledger_row(row)?;
        let mut observed = Observed::default();
        if let Some(pr) = row["pr"].as_u64() {
            observed.ids.push(format!("pr_{pr}"));
        }
        self.conclude(mission, t, Outcome { value: OutcomeValue::Done, provenance: Provenance::Observed }, observed)
    }
}

#[cfg(test)]
#[path = "history_record_tests.rs"]
mod tests;
