//! Pointers (`specs/102` §4, slice S1): where the words of a history record can be checked, never a copy of the transcript.
//!
//! A pointer is `{tool, session_id, range, hmac, kind}`. The `hmac` is an HMAC-SHA256 over the span's text under a local key
//! (§3.4), because a plain hash of a short prompt can be guessed from the record. The session id is stored and the path is not:
//! the path is searched for at resolve time, under the Claude Code projects root only (§4).
//!
//! * **Resolving** reads the transcript with the versioned reader of `specs/062` §8.3 (`transcript::read_claude_code`) and
//!   compares the span's keyed hash. A match is `Resolved`. A missing file, a format the reader does not know, a range that is no
//!   longer there, a line of the wrong role or a hash that no longer matches is `Unresolvable`, with the reason; it is never
//!   silently valid, and a claim that cites it reads **not validated** (`Resolution::validated`).
//! * **What `Resolved` proves.** That the text exists in that transcript, on a line of the right role. It does not prove who wrote
//!   it: the host chooses which span to report. Only a prompt the daemon saw itself is `observed` (that label is S3's).
//! * **Role check.** An `operator-words` span must be a `user` record that is not a tool result, not a meta line and not a
//!   sidechain (the reader's own `typed_text`); an `agent-words` span must be an `assistant` record's text. A line of the other
//!   role or a tool result never reads resolved, whatever its hash.
//! * **Nothing here reaches the network, calls a model, or follows anything in a transcript.** It only reads local files.
//!   Nothing here is a criterion for trust, promotion, tool access or approval (`specs/102` §11 item 1).

use crate::transcript::{read_claude_code, typed_text};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::path::{Path, PathBuf};

/// The tool name a pointer into a Claude Code transcript carries. Other tools are recorded as `unread` (§4, §9).
pub const CLAUDE_CODE: &str = "claude-code";
/// The key file, in the workspace's daemon-only `local/` area (`specs/102` §3.4).
pub const KEY_FILE: &str = "local/history.key";
/// A pointer names at most this many transcript lines (a prompt is one record; this only bounds a hostile range).
pub const MAX_SPAN_LINES: usize = 20;
const KEY_BYTES: usize = 32;

/// Whose words the span is claimed to be: a label from the daemon's evidence, not a finding (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PointerKind {
    OperatorWords,
    AgentWords,
    /// Codex, Copilot and agy: recorded by tool and session id only until a reader with fixtures exists (§4).
    Unread,
}

impl PointerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PointerKind::OperatorWords => "operator-words",
            PointerKind::AgentWords => "agent-words",
            PointerKind::Unread => "unread",
        }
    }
}

/// A span of transcript lines, 1-based and inclusive (the line numbers the 062 reader's evidence uses).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pointer {
    pub tool: String,
    pub session_id: String,
    /// Absent for an `unread` pointer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
    /// Lowercase hex HMAC-SHA256 of the span text; empty for an `unread` pointer.
    #[serde(default)]
    pub hmac: String,
    pub kind: PointerKind,
}

impl Pointer {
    /// A pointer into another tool's store: tool and session id only, kind `unread` (§4).
    pub fn unread(tool: &str, session_id: &str) -> Pointer {
        Pointer { tool: tool.to_string(), session_id: session_id.to_string(), range: None, hmac: String::new(), kind: PointerKind::Unread }
    }
}

/// Why a pointer is not resolved. Fixed words only: nothing from a transcript is carried into a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The pointer is into a tool Delphi has no reader for.
    Unread,
    /// The pointer is not well formed (no range, a range too long, or a hash that is not hex).
    Malformed,
    /// There is no key to recompute the hash with.
    NoKey,
    /// No transcript for the session id was found under the projects root.
    NoSession,
    /// The file could not be read as text.
    NotReadable,
    /// The reader did not recognize the format (or its version): nothing is guessed.
    FormatNotRecognized,
    /// The range is past the end of the transcript, or names a line that is not a record.
    RangeGone,
    /// The line is there but is not of the role the pointer claims, or is a tool result.
    WrongRole,
    /// The text on those lines hashes to something else now.
    HashMismatch,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Unread => "no reader for this tool",
            Reason::Malformed => "the pointer is malformed",
            Reason::NoKey => "the history key is missing",
            Reason::NoSession => "no transcript for this session id under the projects root",
            Reason::NotReadable => "the transcript could not be read",
            Reason::FormatNotRecognized => "the transcript format is not one the reader knows",
            Reason::RangeGone => "the range is no longer in the transcript",
            Reason::WrongRole => "the line is not of the claimed role",
            Reason::HashMismatch => "the text no longer matches its hash",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Resolved,
    Unresolvable(Reason),
}

impl Resolution {
    /// The word a record or a claim shows: `resolved` or `unresolvable` (§4).
    pub fn word(self) -> &'static str {
        match self {
            Resolution::Resolved => "resolved",
            Resolution::Unresolvable(_) => "unresolvable",
        }
    }

    /// Does a claim that cites this pointer read validated? Only a resolved pointer does; an unresolvable one reads **not
    /// validated** (`specs/102` §4, §11 item 8). A resolved pointer proves the text exists, not who wrote it.
    pub fn validated(self) -> bool {
        matches!(self, Resolution::Resolved)
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// The key (§3.4): a local secret in the daemon-only `local/` area, owner-only, never exported, never read by an agent.

pub fn key_path(ws_dir: &Path) -> PathBuf {
    ws_dir.join(KEY_FILE)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Read the key. `Ok(None)` when there is none. A key file that others can read, or that is not 64 hex characters, is an error:
/// a leaked key makes short secrets guessable again (§13), so it is refused and never repaired silently.
pub fn load_key(ws_dir: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = key_path(ws_dir);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read the history key: {e}")),
    };
    if !meta.is_file() {
        return Err("the history key is not a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("the history key can be read by other users; fix its permissions (owner only) before it is used".into());
        }
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read the history key: {e}"))?;
    match unhex(text.trim()) {
        Some(k) if k.len() == KEY_BYTES => Ok(Some(k)),
        _ => Err("the history key is not 64 hex characters".into()),
    }
}

/// The key, made on first need with owner-only permissions. Making a pointer needs it; resolving one does not create it.
pub fn load_or_create_key(ws_dir: &Path) -> Result<Vec<u8>, String> {
    if let Some(k) = load_key(ws_dir)? {
        return Ok(k);
    }
    let path = key_path(ws_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
    }
    let token = delphi_core::server::random_token().map_err(|e| format!("no randomness for the history key: {e}"))?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    match opts.open(&path) {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(token.as_bytes()).map_err(|e| format!("could not write the history key: {e}"))?;
        }
        // Another writer made it between the read and the create: use theirs.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("could not make the history key: {e}")),
    }
    load_key(ws_dir)?.ok_or_else(|| "the history key vanished after it was made".to_string())
}

/// The keyed hash of §3.4: HMAC-SHA256 of `text` under `key`, as lowercase hex.
pub fn span_hmac(key: &[u8], text: &str) -> String {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(text.as_bytes());
    hex(&m.finalize().into_bytes())
}

fn hmac_matches(key: &[u8], text: &str, want_hex: &str) -> bool {
    let Some(want) = unhex(want_hex) else { return false };
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(text.as_bytes());
    m.verify_slice(&want).is_ok()
}

// ---------------------------------------------------------------------------------------------------------------------------
// The span: the text of a record, by role.

/// The text of an assistant record: its `text` parts (or a string). Tool calls and thinking are not text.
fn assistant_text(rec: &Value) -> Option<String> {
    if rec["isSidechain"].as_bool() == Some(true) {
        return None;
    }
    match &rec["message"]["content"] {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => {
            let t: Vec<&str> = parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect();
            if t.is_empty() { None } else { Some(t.join(" ")) }
        }
        _ => None,
    }
}

/// The text of one record as the pointer's kind claims it: a user's typed words, or an assistant's text.
fn record_text(rec: &Value, kind: PointerKind) -> Option<String> {
    match (kind, rec["type"].as_str()) {
        (PointerKind::OperatorWords, Some("user")) => typed_text(rec),
        (PointerKind::AgentWords, Some("assistant")) => assistant_text(rec),
        _ => None,
    }
}

enum SpanErr {
    RangeGone,
    WrongRole,
}

/// The text of lines `from..=to` (1-based) of a transcript, joined with `\n`, every line of the right role.
fn span_text(transcript: &str, range: Range, kind: PointerKind) -> Result<String, SpanErr> {
    if range.from == 0 || range.to < range.from || range.to - range.from >= MAX_SPAN_LINES {
        return Err(SpanErr::RangeGone);
    }
    let mut out: Vec<String> = Vec::new();
    for (i, line) in transcript.lines().enumerate() {
        let n = i + 1;
        if n < range.from {
            continue;
        }
        if n > range.to {
            break;
        }
        let rec: Value = serde_json::from_str(line).map_err(|_| SpanErr::RangeGone)?;
        if !rec.is_object() {
            return Err(SpanErr::RangeGone);
        }
        out.push(record_text(&rec, kind).ok_or(SpanErr::WrongRole)?);
    }
    if out.len() != range.to - range.from + 1 {
        return Err(SpanErr::RangeGone);
    }
    Ok(out.join("\n"))
}

/// Does this transcript parse with the 062 reader, and is it the session the pointer names?
fn reads_as_session(transcript: &str, session_id: &str) -> bool {
    matches!(read_claude_code(transcript), Ok(m) if m.session == session_id)
}

/// The first line whose text for `kind`, trimmed, equals `claimed`, trimmed. A prompt is one record, so a single line.
pub fn locate(transcript: &str, claimed: &str, kind: PointerKind) -> Option<Range> {
    let want = claimed.trim();
    if want.is_empty() {
        return None;
    }
    for (i, line) in transcript.lines().enumerate() {
        let Ok(rec) = serde_json::from_str::<Value>(line) else { continue };
        if record_text(&rec, kind).is_some_and(|t| t.trim() == want) {
            return Some(Range { from: i + 1, to: i + 1 });
        }
    }
    None
}

/// What the daemon's check of a host-reported span found (§3.1 `kind`): `Resolved` carries the pointer to keep; `Reported` means
/// nothing matched, so there is no pointer. A text that appears only in a tool result, or in the other role, is `Reported`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matched {
    Resolved(Pointer),
    Reported,
}

/// Check a host-reported text against a Claude Code transcript and, on a match, make the pointer with the keyed hash of the
/// transcript's own text (not the host's). The key is made on first need by the caller (`load_or_create_key`).
pub fn match_reported(key: &[u8], session_id: &str, transcript: &str, claimed: &str, kind: PointerKind) -> Matched {
    if kind == PointerKind::Unread || !reads_as_session(transcript, session_id) {
        return Matched::Reported;
    }
    let Some(range) = locate(transcript, claimed, kind) else { return Matched::Reported };
    match span_text(transcript, range, kind) {
        Ok(text) => Matched::Resolved(Pointer { tool: CLAUDE_CODE.into(), session_id: session_id.into(), range: Some(range), hmac: span_hmac(key, &text), kind }),
        Err(_) => Matched::Reported,
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// Finding the transcript: by session id, under the projects root, and nowhere else.

/// `~/.claude/projects`, if `HOME` is set. The caller may pass a configured root instead.
pub fn default_claude_root() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude").join("projects"))
}

/// A session id is letters, digits, `-` and `_` (Claude Code's are UUIDs): nothing that could name a path.
pub fn valid_session_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Every `<root>/<folder>/<session_id>.jsonl`, sorted. Only the root's direct sub-folders are looked in; a folder or a file that
/// is a link is skipped, and so is anything whose real path is not under the real root. Nothing above the root is read.
pub fn find_sessions(root: &Path, session_id: &str) -> Vec<PathBuf> {
    if !valid_session_id(session_id) {
        return Vec::new();
    }
    let Ok(real_root) = std::fs::canonicalize(root) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&real_root) else { return Vec::new() };
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let folder = entry.path();
        let Ok(fm) = std::fs::symlink_metadata(&folder) else { continue };
        if fm.file_type().is_symlink() || !fm.is_dir() {
            continue;
        }
        let file = folder.join(format!("{session_id}.jsonl"));
        let Ok(m) = std::fs::symlink_metadata(&file) else { continue };
        if m.file_type().is_symlink() || !m.is_file() {
            continue;
        }
        if std::fs::canonicalize(&file).is_ok_and(|real| real.starts_with(&real_root)) {
            out.push(file);
        }
    }
    out.sort();
    out
}

/// Resolve a pointer against the transcripts under `root`. `key` is the history key (`load_key`), `None` if there is none.
/// The same session id may exist in more than one folder (a worktree wrote to another one); the hash decides, so the pointer is
/// resolved if any candidate matches, and otherwise reports the first candidate's reason.
pub fn resolve(key: Option<&[u8]>, root: &Path, p: &Pointer) -> Resolution {
    if p.kind == PointerKind::Unread || p.tool != CLAUDE_CODE {
        return Resolution::Unresolvable(Reason::Unread);
    }
    let well_formed = |r: &Range| r.from >= 1 && r.to >= r.from && r.to - r.from < MAX_SPAN_LINES;
    let (Some(range), true) = (p.range.filter(well_formed), unhex(&p.hmac).is_some_and(|h| h.len() == 32)) else {
        return Resolution::Unresolvable(Reason::Malformed);
    };
    let Some(key) = key else { return Resolution::Unresolvable(Reason::NoKey) };
    let candidates = find_sessions(root, &p.session_id);
    if candidates.is_empty() {
        return Resolution::Unresolvable(Reason::NoSession);
    }
    let mut first: Option<Reason> = None;
    for path in candidates {
        let verdict = match std::fs::read_to_string(&path) {
            Err(_) => Err(Reason::NotReadable),
            Ok(text) if !reads_as_session(&text, &p.session_id) => Err(Reason::FormatNotRecognized),
            Ok(text) => match span_text(&text, range, p.kind) {
                Err(SpanErr::RangeGone) => Err(Reason::RangeGone),
                Err(SpanErr::WrongRole) => Err(Reason::WrongRole),
                Ok(t) if hmac_matches(key, &t, &p.hmac) => Ok(()),
                Ok(_) => Err(Reason::HashMismatch),
            },
        };
        match verdict {
            Ok(()) => return Resolution::Resolved,
            Err(r) => {
                first.get_or_insert(r);
            }
        }
    }
    Resolution::Unresolvable(first.unwrap_or(Reason::NoSession))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SID: &str = "6f2c1a90-aaaa-4bbb-8ccc-1234567890ab";
    const KEY: [u8; 32] = [7u8; 32];

    fn user(text: &str) -> String {
        json!({"type": "user", "version": "2.0.1", "sessionId": SID, "message": {"role": "user", "content": text}}).to_string()
    }
    fn assistant(text: &str) -> String {
        json!({"type": "assistant", "version": "2.0.1", "sessionId": SID, "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}}).to_string()
    }
    fn tool_result(text: &str) -> String {
        json!({"type": "user", "version": "2.0.1", "sessionId": SID, "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": text}]}}).to_string()
    }
    fn transcript(lines: &[String]) -> String {
        lines.join("\n") + "\n"
    }
    fn ws_with(root: &Path, folder: &str, text: &str) -> PathBuf {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(format!("{SID}.jsonl"));
        std::fs::write(&f, text).unwrap();
        f
    }
    fn pointer_for(text: &str, claimed: &str, kind: PointerKind) -> Pointer {
        match match_reported(&KEY, SID, text, claimed, kind) {
            Matched::Resolved(p) => p,
            Matched::Reported => panic!("expected a match for {claimed:?}"),
        }
    }

    #[test]
    fn the_hash_is_a_keyed_hmac_not_a_plain_hash() {
        let a = span_hmac(&KEY, "fix the build");
        assert_eq!(a.len(), 64);
        assert_eq!(a, span_hmac(&KEY, "fix the build"));
        assert_ne!(a, span_hmac(&[8u8; 32], "fix the build"), "another key gives another hash");
        assert_ne!(a, span_hmac(&KEY, "fix the build."));
        // Not the unkeyed SHA-256 of the text: a candidate guess against the record needs the key.
        use sha2::Digest;
        assert_ne!(a, hex(&Sha256::digest(b"fix the build")));
        assert!(hmac_matches(&KEY, "fix the build", &a));
        assert!(!hmac_matches(&KEY, "fix the build", "zz"), "a hash that is not hex never matches");
    }

    #[test]
    fn the_key_is_made_owner_only_reused_and_a_loose_or_garbled_one_is_refused() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(load_key(t.path()).unwrap(), None, "reading never creates it");
        let k = load_or_create_key(t.path()).unwrap();
        assert_eq!(k.len(), 32);
        assert_eq!(load_or_create_key(t.path()).unwrap(), k, "the same key every time");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(key_path(t.path())).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
            std::fs::set_permissions(key_path(t.path()), std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(load_key(t.path()).unwrap_err().contains("other users"), "a readable key is refused, not used");
            std::fs::set_permissions(key_path(t.path()), std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        std::fs::write(key_path(t.path()), "not hex").unwrap();
        assert!(load_key(t.path()).unwrap_err().contains("64 hex"));
    }

    #[test]
    fn a_reported_prompt_that_is_in_the_transcript_resolves_and_one_that_is_not_stays_reported() {
        let text = transcript(&[user("fix the build"), assistant("done"), tool_result("secret tool output")]);
        let p = pointer_for(&text, "  fix the build \n", PointerKind::OperatorWords);
        assert_eq!(p.range, Some(Range { from: 1, to: 1 }));
        assert_eq!(p.kind, PointerKind::OperatorWords);
        assert_eq!(p.hmac, span_hmac(&KEY, "fix the build"), "the hash is of the transcript's own text");
        assert_eq!(match_reported(&KEY, SID, &text, "something nobody typed", PointerKind::OperatorWords), Matched::Reported);
        assert_eq!(match_reported(&KEY, SID, &text, "", PointerKind::OperatorWords), Matched::Reported);
        // The assistant's text is not the operator's words, and a tool result is not typed by anyone.
        assert_eq!(match_reported(&KEY, SID, &text, "done", PointerKind::OperatorWords), Matched::Reported);
        assert_eq!(match_reported(&KEY, SID, &text, "secret tool output", PointerKind::OperatorWords), Matched::Reported);
        // The same text is an agent-words span for the other kind.
        assert_eq!(pointer_for(&text, "done", PointerKind::AgentWords).range, Some(Range { from: 2, to: 2 }));
        // A transcript of another session, or of a format the reader refuses, matches nothing.
        assert_eq!(match_reported(&KEY, "another-session", &text, "fix the build", PointerKind::OperatorWords), Matched::Reported);
        assert_eq!(match_reported(&KEY, SID, "not json at all\n", "fix the build", PointerKind::OperatorWords), Matched::Reported);
        assert_eq!(match_reported(&KEY, SID, &text, "fix the build", PointerKind::Unread), Matched::Reported);
    }

    #[test]
    fn a_pointer_resolves_when_the_span_is_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let text = transcript(&[user("fix the build"), assistant("done")]);
        ws_with(root.path(), "-Users-me-proj", &text);
        let op = pointer_for(&text, "fix the build", PointerKind::OperatorWords);
        let ag = pointer_for(&text, "done", PointerKind::AgentWords);
        assert_eq!(resolve(Some(&KEY), root.path(), &op), Resolution::Resolved);
        assert_eq!(resolve(Some(&KEY), root.path(), &ag), Resolution::Resolved);
        assert!(Resolution::Resolved.validated());
        assert_eq!(Resolution::Resolved.word(), "resolved");
    }

    #[test]
    fn every_way_a_pointer_can_fail_reads_unresolvable_and_never_validated() {
        let root = tempfile::tempdir().unwrap();
        let text = transcript(&[user("fix the build"), assistant("done")]);
        let file = ws_with(root.path(), "-Users-me-proj", &text);
        let p = pointer_for(&text, "fix the build", PointerKind::OperatorWords);
        let un = |r: Resolution| match r {
            Resolution::Unresolvable(why) => {
                assert!(!r.validated(), "an unresolvable pointer is never validated");
                assert_eq!(r.word(), "unresolvable");
                why
            }
            Resolution::Resolved => panic!("expected unresolvable"),
        };
        // The wrong key: the hash no longer matches.
        assert_eq!(un(resolve(Some(&[9u8; 32]), root.path(), &p)), Reason::HashMismatch);
        assert_eq!(un(resolve(None, root.path(), &p)), Reason::NoKey);
        // The span changed after the pointer was made.
        std::fs::write(&file, transcript(&[user("fix the build NOW"), assistant("done")])).unwrap();
        assert_eq!(un(resolve(Some(&KEY), root.path(), &p)), Reason::HashMismatch);
        // The line is now a tool result, whatever it says: the role check holds even if the text were the same.
        std::fs::write(&file, transcript(&[tool_result("fix the build"), assistant("done")])).unwrap();
        assert_eq!(un(resolve(Some(&KEY), root.path(), &p)), Reason::WrongRole);
        // An operator-words pointer onto an assistant line.
        let mut onto_assistant = p.clone();
        onto_assistant.range = Some(Range { from: 2, to: 2 });
        std::fs::write(&file, &text).unwrap();
        assert_eq!(un(resolve(Some(&KEY), root.path(), &onto_assistant)), Reason::WrongRole);
        // The range is gone (the file was truncated).
        std::fs::write(&file, transcript(&[assistant("hello")])).unwrap();
        let mut far = p.clone();
        far.range = Some(Range { from: 5, to: 5 });
        assert_eq!(un(resolve(Some(&KEY), root.path(), &far)), Reason::RangeGone);
        // The format changed: a log from a Claude Code major the reader was not tested against.
        let newer = json!({"type": "user", "version": "9.0.0", "sessionId": SID, "message": {"content": "fix the build"}}).to_string();
        std::fs::write(&file, newer + "\n").unwrap();
        assert_eq!(un(resolve(Some(&KEY), root.path(), &p)), Reason::FormatNotRecognized);
        // The file is not a transcript at all, and then it is gone.
        std::fs::write(&file, "plain text, not JSON lines\n").unwrap();
        assert_eq!(un(resolve(Some(&KEY), root.path(), &p)), Reason::FormatNotRecognized);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(un(resolve(Some(&KEY), root.path(), &p)), Reason::NoSession);
        // A pointer into another tool's store is `unread`; a malformed one never resolves.
        assert_eq!(un(resolve(Some(&KEY), root.path(), &Pointer::unread("codex", "abc"))), Reason::Unread);
        let mut bad = p.clone();
        bad.hmac = "xyz".into();
        assert_eq!(un(resolve(Some(&KEY), root.path(), &bad)), Reason::Malformed);
        let mut long = p.clone();
        long.range = Some(Range { from: 1, to: 1 + MAX_SPAN_LINES });
        assert_eq!(un(resolve(Some(&KEY), root.path(), &long)), Reason::Malformed);
    }

    #[test]
    fn the_session_id_is_found_in_a_renamed_folder_and_the_hash_decides_between_copies() {
        let root = tempfile::tempdir().unwrap();
        let text = transcript(&[user("fix the build")]);
        let p = pointer_for(&text, "fix the build", PointerKind::OperatorWords);
        // The same session written to two folders (a worktree wrote to another one); one copy was edited.
        ws_with(root.path(), "-Users-me-proj", &transcript(&[user("something else")]));
        ws_with(root.path(), "-Users-me-proj--claude-worktrees-x", &text);
        assert_eq!(find_sessions(root.path(), SID).len(), 2);
        assert_eq!(resolve(Some(&KEY), root.path(), &p), Resolution::Resolved, "any copy that matches resolves it");
    }

    #[test]
    fn the_search_stays_inside_the_root_and_follows_no_link_out_of_it() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("projects");
        std::fs::create_dir_all(&root).unwrap();
        let text = transcript(&[user("fix the build")]);
        // A transcript above the root, one nested too deep, and one in a folder that is a link out of the root.
        let outside = t.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(t.path().join(format!("{SID}.jsonl")), &text).unwrap();
        std::fs::write(outside.join(format!("{SID}.jsonl")), &text).unwrap();
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b").join(format!("{SID}.jsonl")), &text).unwrap();
        assert!(find_sessions(&root, SID).is_empty(), "nothing above the root, nothing two levels down");
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&outside, root.join("linked-folder")).unwrap();
            assert!(find_sessions(&root, SID).is_empty(), "a folder that is a link out of the root is not followed");
            std::fs::create_dir_all(root.join("real")).unwrap();
            symlink(outside.join(format!("{SID}.jsonl")), root.join("real").join(format!("{SID}.jsonl"))).unwrap();
            assert!(find_sessions(&root, SID).is_empty(), "a file that is a link is not read");
            let p = pointer_for(&text, "fix the build", PointerKind::OperatorWords);
            assert_eq!(resolve(Some(&KEY), &root, &p), Resolution::Unresolvable(Reason::NoSession));
        }
        // An id that could name a path is refused before any search.
        for bad in ["", "..", "../x", "a/b", "a\\b", "a.b", &"x".repeat(65)] {
            assert!(!valid_session_id(bad), "{bad:?}");
            assert!(find_sessions(&root, bad).is_empty());
        }
        assert!(valid_session_id(SID));
        // A root that does not exist finds nothing.
        assert!(find_sessions(&t.path().join("missing"), SID).is_empty());
    }

    #[test]
    fn a_pointer_round_trips_as_json_and_an_unread_one_carries_no_hash() {
        let p = Pointer { tool: CLAUDE_CODE.into(), session_id: SID.into(), range: Some(Range { from: 3, to: 3 }), hmac: span_hmac(&KEY, "x"), kind: PointerKind::OperatorWords };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"kind\":\"operator-words\""), "{j}");
        assert_eq!(serde_json::from_str::<Pointer>(&j).unwrap(), p);
        let u = serde_json::to_value(Pointer::unread("agy", "conv-1")).unwrap();
        assert_eq!(u["kind"], "unread");
        assert!(u.get("range").is_none() && u["hmac"] == "");
        assert_eq!(PointerKind::AgentWords.as_str(), "agent-words");
    }
}
