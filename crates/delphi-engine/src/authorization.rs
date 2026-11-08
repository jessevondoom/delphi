//! Operator-authorized tests, slice S1 (`specs/092` TA-0, TA-0r, TA-1): the request, its staging rules, and the protected store.
//!
//! **This module grants no capability.** Nothing here runs an action, adds `--on-card authorization:<id>`, touches the
//! sandbox, or turns a record into an approval; no code path leads from a record to a decision on any card (S2 and S3 do
//! that, each reviewed on its own, and `specs/016` invariant 1, "Red is never allowed", stands unamended).
//!
//! What it does:
//!
//! * **The request** (TA-0). An agent (`request_test_authorization`) or a host (`host.request_test_authorization`) proposes
//!   a manifest of exact actions. The request is *staged*: an `authorization.request.staged` event in a mission log, tier
//!   `L0`, inert, labelled with who asked and the span's taint, which the daemon computes from the span (agent) or fixes at
//!   `federated_peer` (host, which has no span). Every limit of §3 is enforced here, in [`admit`].
//! * **The store** (TA-1). A record is written only by the operator's `delphi test authorize`, into one file,
//!   `$DELPHI_HOME/operator/authorizations.json`, whose blake3 hash is pinned in the operator-scope `ratified.lock` under
//!   the key [`LOCK_KEY`], the same mechanism that pins ratified files (`delphi_core::lock`, [`crate::ratify`]). A store
//!   whose hash does not match its pin, or with no pin, is **ignored and logged**, never read as records ([`load`]).
//!   Every read verifies the pin; there is no other path from a file to a record.
//! * **The deny-list** (TA-5), applied when a record is made and again at staging.

use delphi_core::event::EventEnvelope;
use delphi_core::lock::{self, hash, LockFile};
use delphi_core::provenance::Tier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_ENTRIES: usize = 5;
pub const MAX_PENDING_PER_RUN: usize = 3;
pub const MAX_PENDING_PER_HOST_SESSION: usize = 3;
pub const MAX_PENDING_PER_MISSION: usize = 5;
/// Workspace-wide limits, whatever the session or task ids: all pending requests, host-originated ones, and the pace of host ones.
pub const MAX_PENDING_WORKSPACE: usize = 20;
pub const MAX_PENDING_HOST_WORKSPACE: usize = 6;
pub const HOST_GLOBAL_RATE_SECS: i64 = 60;
/// A refused request is not staged again for 24 hours (`[choice]`, §3 TA-0).
pub const RESTAGE_SECS: i64 = 24 * 3600;
/// A run may stage one request per five minutes (`[choice]`).
pub const RATE_SECS: i64 = 300;
/// A pending request expires after seven days (OD-5).
pub const PENDING_TTL_SECS: i64 = 7 * 24 * 3600;
/// First-slice defaults (OD-2, confirmed 2026-11-07) and the maximum.
pub const DEFAULT_EXPIRY_SECS: i64 = 2 * 3600;
pub const DEFAULT_USES: u32 = 10;
pub const MAX_EXPIRY_SECS: i64 = 24 * 3600;
pub const MAX_USES: u32 = 50;
pub const MIN_EXPIRY_SECS: i64 = 60;

pub const MAX_INTENT: usize = 200;
pub const MAX_PURPOSE: usize = 300;
pub const MAX_CONTEXT: usize = 1000;
pub const MAX_RISK: usize = 1000;
pub const MAX_RUN_PLAN: usize = 600;
pub const MAX_ARGV: usize = 32;
pub const MAX_ARG_CHARS: usize = 256;
pub const MAX_FILES: usize = 16;

/// What the agent's tool call carries; the engine turns it into [`STAGED`] or [`REFUSED`].
pub const ASK: &str = "authorization.request";
pub const STAGED: &str = "authorization.request.staged";
pub const REFUSED: &str = "authorization.request.refused";
pub const CREATED: &str = "authorization.created";
pub const REVOKED: &str = "authorization.revoked";
/// Written when an audit event was appended but the store change it announced failed: the event answers nothing.
pub const CREATE_ROLLED_BACK: &str = "authorization.create.rolled_back";
pub const REVOKE_ROLLED_BACK: &str = "authorization.revoke.rolled_back";

/// The key under which the store's hash is pinned in `operator/ratified.lock`.
pub const LOCK_KEY: &str = "authorizations.json";
pub const SCRATCH_MODE: &str = "noexec";
pub const CWD: &str = "@snapshot";
pub const PROVENANCES: [&str; 2] = ["project_snapshot", "daemon_installed"];

/// The fixed, scrubbed environment every authorized command would get: no `PATH` search, no `LD_PRELOAD`, no `DYLD_*`.
pub fn scrubbed_env() -> BTreeMap<String, String> {
    [("LC_ALL", "C"), ("PATH", ""), ("PYTHONDONTWRITEBYTECODE", "1"), ("PYTHONNOUSERSITE", "1")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

// ------------------------------------------------------------------------------------------------ text hygiene

fn is_ctl(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200b}'..='\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{206a}'..='\u{206f}' | '\u{061c}' | '\u{00ad}' | '\u{034f}' | '\u{3164}' | '\u{ffa0}' | '\u{feff}' | '\u{e0000}'..='\u{e007f}'
        )
}

/// Control, escape, bidirectional-override and zero-width characters removed (line breaks and tabs first become spaces).
pub fn strip_control(s: &str) -> String {
    s.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).filter(|c| !is_ctl(*c)).collect()
}

/// Text for a terminal: every line stripped of control and escape characters, the line breaks kept.
pub fn safe_lines(s: &str) -> String {
    s.split('\n').map(strip_control).collect::<Vec<_>>().join("\n")
}

/// Free text from a requester: stripped, whitespace collapsed to single spaces, cut to `max` characters.
pub fn clean_text(s: &str, max: usize) -> String {
    strip_control(s).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

// ------------------------------------------------------------------------------------------------ the manifest

/// One exact action. Everything the agent may say about it; the binary's path and hash, the environment and the working
/// directory are computed by Delphi when the operator authorizes (see [`RecordEntry`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub tool: String,
    /// The canonical argument vector. `argv[0]` is the absolute path of the binary. No shell string, no glob, no wrapper.
    pub argv: Vec<String>,
    /// Every script or file the command reads and executes, absolute paths (pinned and hashed at authorization time).
    #[serde(default)]
    pub files: Vec<String>,
    /// `project_snapshot` or `daemon_installed`.
    pub runner_provenance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub intent: String,
    pub purpose: String,
    pub manifest: Vec<Entry>,
    pub context: String,
    pub risk: String,
    pub run_plan: String,
}

#[derive(Debug, PartialEq)]
pub enum EntryError {
    /// Malformed or outside the first slice: the caller is told and nothing is logged.
    Invalid(String),
    /// The deny-list (TA-5): a merge, pull request, credential, network tool, dispatch, or a write to `.git`, hooks, configuration or the store.
    Denied(String),
}

impl std::fmt::Display for EntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryError::Invalid(m) => write!(f, "{m}"),
            EntryError::Denied(m) => write!(f, "denied by the deny-list (spec 092 TA-5): {m}"),
        }
    }
}

fn inv<T>(m: impl Into<String>) -> Result<T, EntryError> {
    Err(EntryError::Invalid(m.into()))
}

/// Tool names are canonicalised before matching and before display (alias ambiguity, §5). The first slice admits one kind.
pub fn canonical_tool(name: &str) -> Option<&'static str> {
    match name.trim().to_ascii_lowercase().as_str() {
        "shell" | "sandboxed_shell" | "sandbox_shell" | "run_command" | "run_shell" | "bash" => Some("shell"),
        _ => None,
    }
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase()
}

/// An absolute path with no `.`, `..`, empty or trailing component: already in the form `canonicalize` gives (symlinks aside).
fn normal_abs(p: &str) -> bool {
    p.starts_with('/') && p.len() > 1 && !p.ends_with('/') && p[1..].split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

const WRAPPERS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "csh", "tcsh", "fish", "env", "xargs", "sudo", "su", "doas", "nohup", "time", "timeout", "nice", "exec", "eval", "command", "busybox", "script", "expect", "setsid", "strace", "find", "osascript", "open", "xcrun", "caffeinate"];
const CODE_FLAGS: &[&str] = &["-c", "-e", "--command", "--eval", "-exec", "--exec", "-execdir", "-ec"];
const DENY_BINARIES: &[&str] = &[
    // network
    "curl", "wget", "nc", "ncat", "netcat", "socat", "ssh", "scp", "sftp", "rsync", "ftp", "tftp", "telnet", "nmap", "ping", "traceroute", "dig", "nslookup", "openssl", "docker", "podman", "kubectl", "helm", "terraform", "aws", "gcloud", "az",
    // merge, pull request, publish, install
    "git", "gh", "hub", "glab", "npm", "npx", "yarn", "pnpm", "pip", "pip3", "brew", "apt", "apt-get", "dnf", "yum", "cargo", "make",
    // credentials
    "security", "gpg", "gpg2", "ssh-keygen", "ssh-add", "ssh-agent", "op", "pass", "vault", "kinit",
    // dispatch: another agent, another carrier
    "delphi", "claude", "agy", "codex", "gemini", "copilot", "ollama", "cursor", "aider",
    // writers (the snapshot is read-only to the command; a Red entry never writes outside scratch)
    "rm", "mv", "cp", "dd", "tee", "ln", "chmod", "chown", "install", "truncate", "shred",
];
const DENY_SEGMENTS: &[&str] = &[
    ".git", ".githooks", "hooks", ".husky", ".delphi", "operator", "authorizations", "authorizations.json", "ratified", "ratified.lock", "config.toml", ".gitconfig", ".gitattributes", ".gitmodules", ".ssh", ".aws", ".gnupg", ".netrc", ".npmrc", ".pypirc", ".env", "credentials", "id_rsa", "id_ed25519", "keychains", "login.keychain-db", ".claude", ".config", ".kube", ".docker", "audit",
];

/// The deny-list, TA-5: why an entry may never be authorized, or `None`.
pub fn deny_reason(e: &Entry) -> Option<String> {
    let bin = basename(e.argv.first().map(String::as_str).unwrap_or(""));
    if DENY_BINARIES.contains(&bin.as_str()) {
        return Some(format!("`{}` could merge, publish, fetch, use a credential, dispatch another agent or write", bin));
    }
    let paths = e.argv.iter().map(|a| a.as_str()).chain(e.files.iter().map(String::as_str));
    for a in paths {
        let lower = a.to_ascii_lowercase();
        if lower.contains("://") || lower.starts_with("git@") {
            return Some(format!("`{}` names a network location", clean_text(a, 60)));
        }
        // A `--flag=value` carries a path after the equals sign.
        let value = lower.split_once('=').map(|(_, v)| v).unwrap_or(&lower).to_string();
        for part in [lower.as_str(), value.as_str()] {
            for seg in part.split('/') {
                if DENY_SEGMENTS.contains(&seg) || seg.ends_with(".pem") || seg.ends_with(".key") || seg.ends_with("ratified.lock") {
                    return Some(format!("`{}` touches `{}` (git internals, hooks, configuration, credentials or the authorization store)", clean_text(a, 60), seg));
                }
            }
        }
    }
    None
}

/// What the daemon knows: the interpreters it will pin. A runner is admitted only if `argv[0]` is one of these exact paths, so
/// `/<project>/tools/python3` (a "runner built in the run") is never one, whatever `runner_provenance` says.
#[derive(Debug, Clone)]
pub struct Policy {
    pub interpreters: Vec<String>,
}

impl Policy {
    /// The fixed set of system locations.
    pub fn system() -> Policy {
        Policy { interpreters: ["/usr/bin/python3", "/usr/local/bin/python3", "/opt/homebrew/bin/python3"].iter().map(|s| s.to_string()).collect() }
    }
}

/// Why a runner's isolation cannot be asserted, or `None` (TA-5 "Scratch and interpreters"). The first slice admits only a
/// known system `python3` run as `python3 -I /abs/script.py [args]`: `-I` is isolated mode (no user site, no current directory,
/// no `PYTHON*` variables), the script is pinned in `files`, and there is no `-m` (a module name is code nobody pinned, and
/// `timeit`, `pdb`, `code`, `trace`, `unittest` and others run arbitrary code).
pub fn isolation_problem(e: &Entry, policy: &Policy) -> Option<String> {
    let bin = e.argv.first().map(String::as_str).unwrap_or("");
    if !policy.interpreters.iter().any(|i| i == bin) {
        return Some(format!("`{}` is not an interpreter the daemon knows (only a system python3 at a fixed path is admitted in the first slice); a project-local or built runner is never admitted", clean_text(bin, 80)));
    }
    if e.argv.get(1).map(String::as_str) != Some("-I") {
        return Some("a Python runner must put `-I` (isolated mode) first, so no user site or working-directory code is loaded".into());
    }
    match e.argv.get(2).map(String::as_str) {
        Some(script) if script.starts_with('/') => {
            if e.files.iter().any(|f| f == script) {
                None
            } else {
                Some(format!("the script `{}` must be listed in `files` so it is pinned by hash", clean_text(script, 80)))
            }
        }
        _ => Some("after `-I` give the absolute path of a script (`-m` is not admitted: a module is code nobody pinned)".into()),
    }
}

/// Canonicalise and check one entry: tool name, argument vector, files, provenance, the deny-list and runner isolation.
pub fn check_entry(e: &Entry, policy: &Policy) -> Result<Entry, EntryError> {
    let Some(tool) = canonical_tool(&e.tool) else { return inv(format!("tool `{}` is not admitted: the first slice admits only a sandboxed `shell` command", clean_text(&e.tool, 40))) };
    if e.argv.is_empty() || e.argv.len() > MAX_ARGV {
        return inv(format!("`argv` takes 1 to {} elements", MAX_ARGV));
    }
    for a in &e.argv {
        if a.chars().count() > MAX_ARG_CHARS || a.chars().any(is_ctl) {
            return inv(format!("an `argv` element is over {} characters or holds a control or escape character", MAX_ARG_CHARS));
        }
        // ASCII only in the first slice, so no confusable character can make one command look like another to a quick `y`.
        if !a.is_ascii() {
            return inv("an `argv` element holds a non-ASCII character; the first slice admits ASCII commands only");
        }
        if a.contains('*') || a.contains('?') || a.contains('`') || a.contains("$(") || a.contains("${") {
            return inv(format!("`{}`: no globs, no substitution; the canonical argument vector is matched exactly", clean_text(a, 60)));
        }
        if matches!(a.as_str(), ";" | "&&" | "||" | "|" | "&" | ">" | ">>" | "<" | "2>&1") || CODE_FLAGS.contains(&a.as_str()) {
            return inv(format!("`{}`: no chaining, no redirection and no inline code strings", clean_text(a, 20)));
        }
    }
    let bin = &e.argv[0];
    if !normal_abs(bin) {
        return inv("`argv[0]` must be the absolute canonical path of the binary (no `.`, `..`, `//` or trailing `/`); a name found on `PATH` is never matched");
    }
    if WRAPPERS.contains(&basename(bin).as_str()) {
        return inv(format!("`{}` is a shell or wrapper; name the real binary and its arguments", basename(bin)));
    }
    if e.files.len() > MAX_FILES || e.files.iter().any(|f| !normal_abs(f) || !f.is_ascii() || f.chars().any(is_ctl)) {
        return inv(format!("`files` takes up to {} absolute canonical paths", MAX_FILES));
    }
    let mut seen = HashSet::new();
    if !e.files.iter().all(|f| seen.insert(f)) {
        return inv("`files` lists a path twice");
    }
    if !PROVENANCES.contains(&e.runner_provenance.as_str()) {
        return inv("`runner_provenance` is `project_snapshot` or `daemon_installed`");
    }
    let c = Entry { tool: tool.into(), argv: e.argv.clone(), files: e.files.clone(), runner_provenance: e.runner_provenance.clone() };
    if let Some(why) = deny_reason(&c) {
        return Err(EntryError::Denied(why));
    }
    if let Some(why) = isolation_problem(&c, policy) {
        return inv(why);
    }
    Ok(c)
}

/// The canonical, hashed form of a manifest: what the operator saw is what is hashed.
pub fn manifest_hash(m: &[Entry]) -> String {
    hash(&serde_json::to_vec(m).unwrap_or_default())
}

/// `argv` as one line, each element quoted only when it needs it. The operator retypes this line (TA-0r).
pub fn render_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|a| if !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || "_./:=@%+,-".contains(c)) { a.clone() } else { format!("'{}'", a.replace('\'', "'\\''")) })
        .collect::<Vec<_>>()
        .join(" ")
}

// ------------------------------------------------------------------------------------------------ the request

#[derive(Debug, PartialEq)]
pub enum ParseError {
    Invalid(String),
    /// Hit the deny-list: refused, and logged with the hash of the manifest as asked for.
    Denied { reason: String, manifest_hash: String },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Invalid(m) => write!(f, "{m}"),
            ParseError::Denied { reason, .. } => write!(f, "denied by the deny-list (spec 092 TA-5): {reason}"),
        }
    }
}

/// A request from a tool call's arguments: unknown manifest fields are refused, free text is cleaned and cut, each entry is
/// checked ([`check_entry`]). Used by the agent tool and by the host socket alike (which adds its own fields, ignored here).
pub fn parse_request(v: &Value, policy: &Policy) -> Result<Request, ParseError> {
    let bad = |m: String| ParseError::Invalid(m);
    let o = v.as_object().ok_or_else(|| bad("the request must be an object".into()))?;
    let text = |k: &str, max: usize| -> Result<String, ParseError> {
        let t = clean_text(o.get(k).and_then(Value::as_str).unwrap_or(""), max);
        if t.is_empty() {
            Err(ParseError::Invalid(format!("`{k}` is required: say it in plain words, up to {max} characters")))
        } else {
            Ok(t)
        }
    };
    let (intent, purpose, context, risk, run_plan) = (text("intent", MAX_INTENT)?, text("purpose", MAX_PURPOSE)?, text("context", MAX_CONTEXT)?, text("risk", MAX_RISK)?, text("run_plan", MAX_RUN_PLAN)?);
    let raw = o.get("manifest").and_then(Value::as_array).ok_or_else(|| bad("`manifest` is required: a list of exact actions".into()))?;
    if raw.is_empty() || raw.len() > MAX_ENTRIES {
        return Err(bad(format!("`manifest` takes 1 to {} entries", MAX_ENTRIES)));
    }
    let mut manifest = Vec::new();
    let mut denied: Option<String> = None;
    for (i, r) in raw.iter().enumerate() {
        let e: Entry = serde_json::from_value(r.clone()).map_err(|e| bad(format!("manifest entry {}: {}", i + 1, e)))?;
        match check_entry(&e, policy) {
            Ok(c) => manifest.push(c),
            Err(EntryError::Invalid(m)) => return Err(bad(format!("manifest entry {}: {}", i + 1, m))),
            Err(EntryError::Denied(m)) => {
                denied.get_or_insert(format!("entry {}: {}", i + 1, m));
                manifest.push(Entry { tool: canonical_tool(&e.tool).unwrap_or("shell").into(), ..e });
            }
        }
    }
    if let Some(reason) = denied {
        return Err(ParseError::Denied { reason, manifest_hash: manifest_hash(&manifest) });
    }
    let mut seen = HashSet::new();
    if !manifest.iter().all(|e| seen.insert(e.argv.clone())) {
        return Err(bad("`manifest` lists the same action twice".into()));
    }
    Ok(Request { intent, purpose, manifest, context, risk, run_plan })
}

/// The request as hashed: all its fields.
pub fn request_hash(r: &Request) -> String {
    hash(&serde_json::to_vec(r).unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Agent,
    Host,
}

/// Who asked, as the daemon knows it (never from the request's own text).
#[derive(Debug, Clone)]
pub struct Who {
    pub kind: Kind,
    /// `<role>/<model>` for an agent, `host:<tool>/<model>` for a host.
    pub label: String,
    pub role: Option<String>,
    pub model: String,
    pub span: Option<String>,
    pub task: Option<String>,
    /// Computed from the span for an agent; fixed `federated_peer` for a host.
    pub taint: Tier,
    /// The run the per-run limits count against: the task for an agent, the session for a host.
    pub run: String,
    /// The mission log the request is staged in.
    pub mission: String,
}

/// The fixed taint of a request that came over the host socket: it has no span to compute one from.
pub const HOST_TAINT: Tier = Tier::L1;

pub struct Clock<'a> {
    pub now_unix: i64,
    pub now_rfc: &'a str,
}

#[derive(Debug)]
pub enum Outcome {
    /// The payload of an `authorization.request.staged` event.
    Staged(Value),
    /// Not staged. `log` is the payload of an `authorization.request.refused` event, or `None` when the refusal is quiet
    /// (a limit hit again and again must not fill the log).
    Refused { message: String, log: Option<Value> },
}

fn ts(e: &EventEnvelope) -> i64 {
    delphi_core::time::parse_rfc3339(&e.timestamp).unwrap_or(0)
}

fn refused(message: impl Into<String>, who: &Who, c: &Clock, manifest_hash: &str, why: &str, blocks_restage: bool) -> Outcome {
    Outcome::Refused {
        message: message.into(),
        log: Some(json!({"manifest_hash": manifest_hash, "reason": why, "blocks_restage": blocks_restage, "run": who.run, "mission": who.mission, "kind": if who.kind == Kind::Host { "host" } else { "agent" }, "requester": who.label, "taint": who.taint.name(), "refused_at": c.now_rfc, "source": "daemon"})),
    }
}

/// Requests staged in one mission's log and not yet resolved by the operator or expired.
fn pending<'a>(log: &'a [EventEnvelope], resolved: &HashSet<String>, now: i64) -> Vec<&'a EventEnvelope> {
    log.iter().filter(|e| e.kind == STAGED && !resolved.contains(e.payload["id"].as_str().unwrap_or("")) && now - ts(e) < PENDING_TTL_SECS).collect()
}

/// Ids the operator has resolved (authorized or refused), from the operator's audit log.
pub fn resolved_ids(operator_log: &[EventEnvelope]) -> HashSet<String> {
    // A `created` event whose record could not be made is rolled back by a later event and answers nothing.
    let rolled_back: HashSet<u64> = operator_log.iter().filter(|e| e.kind == CREATE_ROLLED_BACK).filter_map(|e| e.payload["created_seq"].as_u64()).collect();
    operator_log
        .iter()
        .filter(|e| e.kind == REFUSED || (e.kind == CREATED && !rolled_back.contains(&e.seq)))
        .filter_map(|e| e.payload["request_id"].as_str().map(String::from))
        .collect()
}

/// Every rule of §3 TA-0, over the logs alone. `ledger` is the workspace-wide request ledger (every request staged or
/// refused in any mission, see [`record_request`]); `operator_log` is the operator's audit log (their refusals and
/// authorizations). Counting over the whole workspace is what keeps a fresh host session id, or a fresh task, from
/// buying a fresh budget.
pub fn admit(ledger: &[EventEnvelope], operator_log: &[EventEnvelope], who: &Who, parsed: Result<Request, ParseError>, c: &Clock) -> Outcome {
    let quiet = |m: String| Outcome::Refused { message: m, log: None };
    // 1. one request per five minutes per run, counting what was staged and what was refused with a record
    let recent = ledger.iter().filter(|e| (e.kind == STAGED || e.kind == REFUSED) && e.payload["run"].as_str() == Some(&who.run) && c.now_unix - ts(e) < RATE_SECS).map(ts).max();
    if let Some(t) = recent {
        return quiet(format!("rate limit: a run may stage one authorization request per {} minutes; try again in {} seconds, or say in your report what you need", RATE_SECS / 60, (RATE_SECS - (c.now_unix - t)).max(1)));
    }
    let req = match parsed {
        Ok(r) => r,
        Err(ParseError::Invalid(m)) => return quiet(m),
        Err(ParseError::Denied { reason, manifest_hash }) => return refused(format!("refused: denied by the deny-list (spec 092 TA-5): {reason}"), who, c, &manifest_hash, &format!("deny-list: {reason}"), true),
    };
    let mh = manifest_hash(&req.manifest);
    // 1b. whatever a host calls itself, the workspace as a whole takes one host request a minute: a fresh session id per call
    // buys no fresh budget
    if who.kind == Kind::Host && ledger.iter().any(|e| (e.kind == STAGED || e.kind == REFUSED) && e.payload["kind"].as_str() == Some("host") && c.now_unix - ts(e) < HOST_GLOBAL_RATE_SECS) {
        return quiet(format!("rate limit: this workspace takes one host authorization request per {} seconds, whichever session sends it", HOST_GLOBAL_RATE_SECS));
    }
    // 2. a request from an agent span at federated_peer taint or lower cannot exist (a host's is fixed at federated_peer and staged)
    if who.kind == Kind::Agent && who.taint <= Tier::L1 {
        return refused(
            format!("refused: this span's taint is {} (external or federated input is in its context), and a request staged below the gate cannot exist; say in your report what you need", who.taint.name()),
            who,
            c,
            &mh,
            &format!("taint {}", who.taint.name()),
            false,
        );
    }
    // 3. a refused request is not staged again for 24 hours
    let again = ledger.iter().chain(operator_log.iter()).any(|e| e.kind == REFUSED && e.payload["blocks_restage"].as_bool() == Some(true) && e.payload["manifest_hash"].as_str() == Some(&mh) && c.now_unix - ts(e) < RESTAGE_SECS);
    if again {
        return quiet("this manifest was refused less than 24 hours ago and is not staged again".into());
    }
    // 4. de-duplicated by the hash of the manifest, 5. the caps
    let resolved = resolved_ids(operator_log);
    let pend = pending(ledger, &resolved, c.now_unix);
    if let Some(d) = pend.iter().find(|e| e.payload["manifest_hash"].as_str() == Some(&mh)) {
        return quiet(format!("the same manifest is already pending as {}", d.payload["id"].as_str().unwrap_or("?")));
    }
    let cap_run = if who.kind == Kind::Host { MAX_PENDING_PER_HOST_SESSION } else { MAX_PENDING_PER_RUN };
    if pend.iter().filter(|e| e.payload["run"].as_str() == Some(&who.run)).count() >= cap_run {
        return quiet(format!("{} requests from this {} are already pending; the operator has to answer one first", cap_run, if who.kind == Kind::Host { "host session" } else { "run" }));
    }
    if pend.iter().filter(|e| e.payload["mission"].as_str() == Some(&who.mission)).count() >= MAX_PENDING_PER_MISSION {
        return quiet(format!("{} requests are already pending in this mission; the operator has to answer one first", MAX_PENDING_PER_MISSION));
    }
    if who.kind == Kind::Host && pend.iter().filter(|e| e.payload["requester"]["kind"].as_str() == Some("host")).count() >= MAX_PENDING_HOST_WORKSPACE {
        return quiet(format!("{} host requests are already pending in this workspace; the operator has to answer one first", MAX_PENDING_HOST_WORKSPACE));
    }
    if pend.len() >= MAX_PENDING_WORKSPACE {
        return quiet(format!("{} requests are already pending in this workspace; the operator has to answer one first", MAX_PENDING_WORKSPACE));
    }
    let rh = request_hash(&req);
    let id = format!("ar-{}", &hash(format!("{mh}|{rh}|{}|{}", who.run, c.now_rfc).as_bytes())[..10]);
    Outcome::Staged(json!({
        "id": id, "tier": "L0", "state": "pending",
        "request": req, "manifest_hash": mh, "request_hash": rh,
        "requester": {"kind": if who.kind == Kind::Host { "host" } else { "agent" }, "label": who.label, "role": who.role, "model": who.model, "span": who.span, "task": who.task},
        "taint": {"tier": who.taint.as_str(), "name": who.taint.name()},
        "run": who.run, "mission": who.mission, "kind": if who.kind == Kind::Host { "host" } else { "agent" }, "staged_at": c.now_rfc,
    }))
}

/// A staged request as the operator reads it back.
#[derive(Debug, Clone)]
pub struct Staged {
    pub mission: String,
    pub id: String,
    pub request: Request,
    pub manifest_hash: String,
    pub request_hash: String,
    pub requester_kind: String,
    pub requester_label: String,
    pub taint_tier: Tier,
    pub span: Option<String>,
    pub task: Option<String>,
    pub staged_at: String,
}

impl Staged {
    pub fn from_event(mission: &str, e: &EventEnvelope) -> Option<Staged> {
        if e.kind != STAGED {
            return None;
        }
        let p = &e.payload;
        let request: Request = serde_json::from_value(p["request"].clone()).ok()?;
        // The hashes are recomputed: a log line that disagrees with its own request is not shown as a request.
        let (mh, rh) = (manifest_hash(&request.manifest), request_hash(&request));
        if p["manifest_hash"].as_str() != Some(&mh) || p["request_hash"].as_str() != Some(&rh) {
            return None;
        }
        // Only the shape the daemon writes is shown: an id and a time of a fixed form, whatever the line says.
        let id = p["id"].as_str().filter(|i| i.len() == 13 && i.starts_with("ar-") && i[3..].bytes().all(|b| b.is_ascii_hexdigit()))?.to_string();
        delphi_core::time::parse_rfc3339(p["staged_at"].as_str()?)?;
        Some(Staged {
            mission: mission.into(),
            id,
            request,
            manifest_hash: mh,
            request_hash: rh,
            requester_kind: p["requester"]["kind"].as_str()?.to_string(),
            requester_label: strip_control(p["requester"]["label"].as_str().unwrap_or("?")),
            taint_tier: Tier::parse(p["taint"]["tier"].as_str()?)?,
            span: p["requester"]["span"].as_str().map(strip_control),
            task: p["requester"]["task"].as_str().map(strip_control),
            staged_at: p["staged_at"].as_str().unwrap_or("").to_string(),
        })
    }

    pub fn is_host(&self) -> bool {
        self.requester_kind == "host"
    }

    /// Whether the operator must retype or confirm each literal argv (§3 TA-0r): any request below `internal_empirical`.
    pub fn needs_retype(&self) -> bool {
        self.taint_tier < Tier::L4
    }
}

// ------------------------------------------------------------------------------------------------ the record (TA-1)

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pinned {
    pub path: String,
    pub blake3: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordEntry {
    pub tool: String,
    pub argv: Vec<String>,
    /// The absolute canonical path of `argv[0]` and a hash of it, taken when the operator authorized.
    pub binary: Pinned,
    pub files: Vec<Pinned>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub runner_provenance: String,
    pub scratch_mode: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    /// The operator's words, shown at every use.
    pub purpose: String,
    pub intent: String,
    pub manifest: Vec<RecordEntry>,
    pub manifest_hash: String,
    pub home: String,
    pub machine: String,
    /// Required: the run this authorization is for, chosen by the operator.
    pub run: String,
    pub created: String,
    pub expires: String,
    pub max_uses: u32,
    /// The use counter S2 decrements against; S1 never changes it.
    pub uses: u32,
    pub request_id: Option<String>,
    pub request_hash: Option<String>,
    pub requester: Option<String>,
    pub revoked: Option<String>,
}

impl Record {
    pub fn is_revoked(&self) -> bool {
        self.revoked.is_some()
    }
}

/// The last event of the operator's audit log as of the last change to the store. The store is pinned, so this is pinned too: a
/// deleted, cut or rewritten audit log (a `created` event removed so a request is pending again) no longer reaches it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditHead {
    pub seq: u64,
    pub h: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default = "one")]
    pub version: u32,
    pub records: Vec<Record>,
    #[serde(default)]
    pub audit_head: Option<AuditHead>,
}

fn one() -> u32 {
    1
}

pub fn store_path(home: &Path) -> PathBuf {
    home.join("operator/authorizations.json")
}

fn lock_path(home: &Path) -> PathBuf {
    home.join("operator/ratified.lock")
}

fn guard_path(home: &Path) -> PathBuf {
    home.join("operator/locks/ratify.lock")
}

/// Where the next version of the store waits between being pinned and being moved into place (see [`mutate`]).
fn next_path(home: &Path) -> PathBuf {
    home.join("operator/authorizations.json.next")
}

fn parse_store(bytes: &[u8]) -> Result<Store, String> {
    let store: Store = serde_json::from_slice(bytes).map_err(|e| format!("the authorization store is pinned but does not parse ({e})"))?;
    if store.version != 1 {
        return Err(format!("the authorization store has version {}, this build reads 1", store.version));
    }
    Ok(store)
}

/// Read the store against `lockfile`: the pin must exist and match the file's hash. Nothing is trusted otherwise. The second
/// value is true when the live file is the previous version and `authorizations.json.next` is the one the pin covers (a crash
/// between pinning and the final rename): the pinned bytes are used, and [`mutate`] finishes the move.
fn load_with(home: &Path, lockfile: &LockFile) -> Result<(Store, bool), String> {
    let pin = lockfile.files.get(LOCK_KEY);
    let live = fs::read(store_path(home));
    match (live, pin) {
        (Err(e), None) if e.kind() == std::io::ErrorKind::NotFound => Ok((Store { version: 1, records: vec![], audit_head: None }, false)),
        (Err(e), None) => Err(format!("the authorization store cannot be read ({e})")),
        (Ok(_), None) => Err("an authorization store exists with no entry in operator/ratified.lock (planted, or its pin was removed); it is ignored".into()),
        (Ok(bytes), Some(p)) if hash(&bytes) == *p => Ok((parse_store(&bytes)?, false)),
        (live, Some(p)) => match fs::read(next_path(home)) {
            Ok(next) if hash(&next) == *p => Ok((parse_store(&next)?, true)),
            _ => Err(match live {
                Ok(_) => "the authorization store does not match its pin in operator/ratified.lock (edited or planted); it is ignored".into(),
                Err(e) => format!("the authorization store is pinned but cannot be read ({e}); no record is trusted"),
            }),
        },
    }
}

/// The store, verified against the operator's lock; fails closed (an unreadable lock, a missing or mismatched pin, a store with no pin).
pub fn load(home: &Path) -> Result<Store, String> {
    let lockfile = lock::read(&lock_path(home)).map_err(|e| format!("operator/ratified.lock cannot be read strictly ({e}); no authorization is trusted"))?;
    load_with(home, &lockfile).map(|(s, _)| s)
}

/// As [`load`], but a store that fails verification is reported in the error log and read as empty: **ignored and logged**.
pub fn load_or_ignore(home: &Path) -> Store {
    match load(home) {
        Ok(s) => s,
        Err(why) => {
            delphi_core::elog::error("authorization", "store_ignored", &why, json!({"path": store_path(home).display().to_string()}));
            Store::default()
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".authorizations.tmp-{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    fs::rename(&tmp, path)?;
    fs::File::open(dir)?.sync_all()
}

/// Change the store under the operator scope's advisory lock (`operator/locks/ratify.lock`): the lock is read strictly, the
/// current store must verify against its pin (a tampered store is never written over), `f` edits it, then three steps in an
/// order that no crash can turn into a store nobody can read: (1) the new bytes are written (0600, fsynced) to
/// `authorizations.json.next`, (2) their hash is pinned in the lock, (3) the file is renamed over `authorizations.json`.
/// A crash after (1) leaves the old pin and the old store, which still match; a crash after (2) leaves a live file that no
/// longer matches the pin and a `.next` that does, which [`load`] reads and the next change finishes moving. A planted
/// `.next` matches no pin, so it is ignored.
pub fn mutate<T>(home: &Path, f: impl FnOnce(&mut Store) -> Result<T, String>) -> Result<T, String> {
    lock::with_lock(&guard_path(home), || -> Result<T, String> {
        let mut lockfile = lock::read(&lock_path(home)).map_err(|e| format!("operator/ratified.lock cannot be read strictly ({e}); run `delphi doctor`"))?;
        let (mut store, roll_forward) = load_with(home, &lockfile)?;
        if roll_forward {
            fs::rename(next_path(home), store_path(home)).map_err(|e| format!("could not finish the previous change to the authorization store: {e}"))?;
        }
        let out = f(&mut store)?;
        let bytes = serde_json::to_vec_pretty(&store).map_err(|e| e.to_string())?;
        write_private(&next_path(home), &bytes).map_err(|e| format!("could not write the authorization store: {e}"))?;
        lockfile.files.insert(LOCK_KEY.into(), hash(&bytes));
        lock::write(&lock_path(home), &lockfile).map_err(|e| format!("could not pin the authorization store: {e}"))?;
        fs::rename(next_path(home), store_path(home)).map_err(|e| format!("could not move the authorization store into place (it is pinned and is read from {}): {e}", next_path(home).display()))?;
        Ok(out)
    })
    .map_err(|e| e.to_string())?
}

fn machine() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: gethostname writes at most `buf.len()` bytes.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    if !ok {
        return "unknown".into();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// Resolve, check and hash one file. `allow_link` is for the daemon's own interpreter list only (a system `python3` is often a
/// symlink): the path stays as given and the hash is of what it resolves to now. A script is never allowed to be a link.
fn pin_file(path: &str, allow_link: bool) -> Result<Pinned, String> {
    use std::os::unix::fs::PermissionsExt;
    let canon = fs::canonicalize(path).map_err(|e| format!("`{}`: {e}", clean_text(path, 80)))?;
    if !allow_link && canon.to_string_lossy() != path {
        return Err(format!("`{}` is not canonical (it resolves to `{}`); name the resolved path", clean_text(path, 80), canon.display()));
    }
    let meta = fs::metadata(&canon).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!("`{}` is not a regular file", clean_text(path, 80)));
    }
    if meta.permissions().mode() & 0o022 != 0 {
        return Err(format!("`{}` is group- or world-writable, so an agent could replace it", clean_text(path, 80)));
    }
    if let Some(dir) = canon.parent() {
        if fs::metadata(dir).map(|m| m.permissions().mode() & 0o022 != 0).unwrap_or(true) {
            return Err(format!("the directory of `{}` is group- or world-writable (or unreadable), so a file in it could be replaced", clean_text(path, 80)));
        }
    }
    if canon.components().any(|c| c.as_os_str() == "scratch") {
        return Err(format!("`{}` is in a scratch directory, which an agent can write", clean_text(path, 80)));
    }
    Ok(Pinned { path: path.into(), blake3: hash(&fs::read(&canon).map_err(|e| e.to_string())?) })
}

/// What the operator chose.
pub struct NewRecord {
    pub purpose: String,
    pub intent: String,
    pub run: String,
    pub expires_in_secs: i64,
    pub max_uses: u32,
    pub entries: Vec<Entry>,
    /// The request this authorizes: `(id, hash, requester label)`; `None` for a record the operator wrote at the console.
    pub request: Option<(String, String, String)>,
    pub policy: Policy,
}

/// Make the record: the deny-list and the runner check run again on every entry, each binary and file is resolved and
/// hashed now, and the limits are held to the maximum. Nothing is written.
pub fn build_record(home: &Path, n: &NewRecord, now_unix: i64, now_rfc: &str) -> Result<Record, String> {
    let purpose = clean_text(&n.purpose, MAX_PURPOSE);
    let run = n.run.trim().to_string();
    if purpose.is_empty() {
        return Err("a purpose is required: your words, shown at every use".into());
    }
    if run.is_empty() || run.len() > 64 || !run.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err("`--run <id>` is required: 1 to 64 letters, digits, `-`, `_` or `.`; the launch must pass the same id".into());
    }
    if !(MIN_EXPIRY_SECS..=MAX_EXPIRY_SECS).contains(&n.expires_in_secs) {
        return Err(format!("an authorization lasts {} seconds to {} hours", MIN_EXPIRY_SECS, MAX_EXPIRY_SECS / 3600));
    }
    if n.max_uses == 0 || n.max_uses > MAX_USES {
        return Err(format!("`max_uses` is 1 to {}", MAX_USES));
    }
    if n.entries.is_empty() || n.entries.len() > MAX_ENTRIES {
        return Err(format!("a record holds 1 to {} manifest entries", MAX_ENTRIES));
    }
    let mut manifest = Vec::new();
    let mut checked = Vec::new();
    for (i, e) in n.entries.iter().enumerate() {
        let c = check_entry(e, &n.policy).map_err(|e| format!("manifest entry {}: {}", i + 1, e))?;
        let binary = pin_file(&c.argv[0], true).map_err(|e| format!("manifest entry {}: binary {}", i + 1, e))?;
        let files = c.files.iter().map(|f| pin_file(f, false)).collect::<Result<Vec<_>, _>>().map_err(|e| format!("manifest entry {}: file {}", i + 1, e))?;
        manifest.push(RecordEntry { tool: c.tool.clone(), argv: c.argv.clone(), binary, files, env: scrubbed_env(), cwd: CWD.into(), runner_provenance: c.runner_provenance.clone(), scratch_mode: SCRATCH_MODE.into() });
        checked.push(c);
    }
    let mh = manifest_hash(&checked);
    let home_s = fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf()).to_string_lossy().into_owned();
    let id = format!("ta-{}", &hash(format!("{mh}|{run}|{now_rfc}|{}|{}", std::process::id(), now_unix).as_bytes())[..10]);
    Ok(Record {
        id,
        purpose,
        intent: clean_text(&n.intent, MAX_INTENT),
        manifest,
        manifest_hash: mh,
        home: home_s,
        machine: machine(),
        run,
        created: now_rfc.into(),
        expires: delphi_core::time::format_unix_millis(now_unix + n.expires_in_secs, 0),
        max_uses: n.max_uses,
        uses: 0,
        request_id: n.request.as_ref().map(|r| r.0.clone()),
        request_hash: n.request.as_ref().map(|r| r.1.clone()),
        requester: n.request.as_ref().map(|r| r.2.clone()),
        revoked: None,
    })
}

/// Add a record. The store is verified first and the new record (and the audit head that announced it) are the only changes.
pub fn add_record(home: &Path, r: Record, head: Option<AuditHead>) -> Result<(), String> {
    mutate(home, |s| {
        if s.records.iter().any(|x| x.id == r.id) {
            return Err("a record with this id exists".into());
        }
        s.records.push(r);
        s.audit_head = head;
        Ok(())
    })
}

/// Revoke a record. Irreversible: there is no code that clears `revoked`, and a revoked record is only ever replaced by a new one.
pub fn revoke(home: &Path, id: &str, now_rfc: &str, head: Option<AuditHead>) -> Result<Record, String> {
    mutate(home, |s| {
        let r = s.records.iter_mut().find(|r| r.id == id).ok_or_else(|| format!("no authorization `{}`", clean_text(id, 40)))?;
        if r.revoked.is_some() {
            return Err(format!("{} is already revoked", r.id));
        }
        r.revoked = Some(now_rfc.to_string());
        let out = r.clone();
        s.audit_head = head;
        Ok(out)
    })
}

// ------------------------------------------------------------------------------------------------ the operator's audit log

pub fn audit_dir(home: &Path) -> PathBuf {
    home.join("operator/audit")
}

/// Open the audit log for writing. The log's own writer lock is held until it is dropped, which also serializes two `authorize`
/// calls: the second cannot open it. Only the operator-side code writes it; no agent has a write path (`operator/` is denied to
/// the path guard).
pub fn open_audit(home: &Path) -> Result<delphi_core::log::EventLog, String> {
    delphi_core::log::EventLog::open(&audit_dir(home), Default::default()).map_err(|e| match e {
        delphi_core::log::LogError::Locked(_) => "another `delphi test` command is writing the authorization audit log; try again in a moment".to_string(),
        e => format!("could not open the authorization audit log: {e}"),
    })
}

/// Append one event and return the head it leaves (verified from the chain, not assumed).
pub fn append_audit(home: &Path, log: &mut delphi_core::log::EventLog, kind: &str, payload: Value) -> Result<AuditHead, String> {
    log.append(EventEnvelope::new("operator-audit", kind, payload)).map_err(|e| format!("could not write the audit event `{kind}`: {e}"))?;
    let rep = delphi_core::log::verify_chain(&audit_dir(home)).map_err(|e| e.to_string())?;
    match (rep.broken, rep.head) {
        (None, Some((seq, h))) => Ok(AuditHead { seq, h }),
        (Some(b), _) => Err(format!("the audit log does not verify after the write: {b}")),
        (None, None) => Err("the audit log has no chained event after the write".into()),
    }
}

/// The audit log, **verified**: its hash chain must be intact (no event edited, deleted, reordered or inserted), every event must
/// be chained, and it must reach the head the pinned store recorded (a cut tail is caught). A log that fails is an error, loudly:
/// what it says decides which requests are still pending and which manifests are barred for a day, so it is never read half-trusted.
pub fn read_audit(home: &Path) -> Result<Vec<EventEnvelope>, String> {
    let head = load(home)?.audit_head.map(|h| (h.seq, h.h));
    if !audit_dir(home).join("events").is_dir() {
        return match head {
            None => Ok(vec![]),
            Some(_) => Err("the authorization audit log is missing but the store says events were written: it was deleted; run `delphi doctor`".into()),
        };
    }
    let rep = delphi_core::log::verify_chain(&audit_dir(home)).map_err(|e| format!("the authorization audit log cannot be read: {e}"))?;
    let rep = if rep.unchained > 0 && rep.broken.is_none() { delphi_core::log::ChainReport { broken: Some("an event has no chain fields".into()), ..rep } } else { rep };
    if let Some(b) = rep.against_stored_head(head).broken {
        return Err(format!("the authorization audit log fails verification: {b}; no request or authorization is trusted until the operator has looked (`delphi doctor`)"));
    }
    delphi_core::log::replay(&audit_dir(home)).map_err(|e| format!("the authorization audit log cannot be read: {e}"))
}

/// Make a record live **after** its audit event: the `created` event is appended first (so a record is never live unaudited),
/// then the store changes; if the store change fails, a `rolled_back` event says the first one answers nothing. Holding the
/// audit log's writer lock for the whole of it, and checking again under it that the request is still unanswered, is what stops
/// two `authorize` calls for one request from both passing.
pub fn create_audited(home: &Path, rec: Record, payload: Value) -> Result<(), String> {
    let mut audit = open_audit(home)?;
    let ops = read_audit(home)?;
    if let Some(rid) = &rec.request_id {
        if resolved_ids(&ops).contains(rid) {
            return Err(format!("{rid} was already answered; a changed or repeated request is a new request"));
        }
    }
    let head = append_audit(home, &mut audit, CREATED, payload)?;
    let created_seq = head.seq;
    if let Err(e) = add_record(home, rec.clone(), Some(head)) {
        let _ = append_audit(home, &mut audit, CREATE_ROLLED_BACK, json!({"id": rec.id, "created_seq": created_seq, "reason": e}));
        return Err(e);
    }
    Ok(())
}

/// Revoke after the audit event, as [`create_audited`].
pub fn revoke_audited(home: &Path, id: &str, now_rfc: &str) -> Result<Record, String> {
    let mut audit = open_audit(home)?;
    read_audit(home)?;
    let current = load(home)?.records.into_iter().find(|r| r.id == id).ok_or_else(|| format!("no authorization `{}`", clean_text(id, 40)))?;
    if current.is_revoked() {
        return Err(format!("{} is already revoked", current.id));
    }
    let head = append_audit(home, &mut audit, REVOKED, json!({"id": current.id, "manifest_hash": current.manifest_hash, "run": current.run, "revoked_at": now_rfc, "by": "operator", "source": "operator"}))?;
    match revoke(home, id, now_rfc, Some(head)) {
        Ok(r) => Ok(r),
        Err(e) => {
            let _ = append_audit(home, &mut audit, REVOKE_ROLLED_BACK, json!({"id": id, "reason": e}));
            Err(e)
        }
    }
}

/// The operator closes a request: audited, and the manifest is barred for 24 hours. Under the writer lock, like [`create_audited`].
pub fn refuse_audited(home: &Path, request_id: &str, manifest_hash: &str, now_rfc: &str) -> Result<(), String> {
    let mut audit = open_audit(home)?;
    if resolved_ids(&read_audit(home)?).contains(request_id) {
        return Err(format!("{request_id} was already answered"));
    }
    append_audit(home, &mut audit, REFUSED, json!({"request_id": request_id, "manifest_hash": manifest_hash, "reason": "refused by the operator", "blocks_restage": true, "refused_at": now_rfc, "source": "operator"})).map(|_| ())
}

// ------------------------------------------------------------------------------------------------ the request ledger

/// The workspace-wide ledger of requests: every `authorization.request.staged` and `.refused` payload, in one hash-chained log, so
/// the limits count across all missions and host sessions. Written only by the daemon.
pub fn ledger_dir(ws_dir: &Path) -> PathBuf {
    ws_dir.join("local/authorization-requests")
}

static LEDGER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The ledger, chain-verified; a break is an error.
pub fn read_ledger(ws_dir: &Path) -> Result<Vec<EventEnvelope>, String> {
    let dir = ledger_dir(ws_dir);
    if !dir.join("events").is_dir() {
        return Ok(vec![]);
    }
    let rep = delphi_core::log::verify_chain(&dir).map_err(|e| format!("the authorization request ledger cannot be read: {e}"))?;
    if rep.unchained > 0 || rep.broken.is_some() {
        return Err(format!("the authorization request ledger fails verification ({}); no request is staged or shown until the operator has looked", rep.broken.unwrap_or_else(|| "an event has no chain fields".into())));
    }
    delphi_core::log::replay(&dir).map_err(|e| format!("the authorization request ledger cannot be read: {e}"))
}

/// Append to the ledger (daemon only; tests plant lines with it). A caller outside `record_request` must hold no other writer.
pub fn append_ledger(ws_dir: &Path, kind: &str, payload: Value) -> Result<(), String> {
    let mut log = delphi_core::log::EventLog::open(&ledger_dir(ws_dir), Default::default()).map_err(|e| format!("could not open the authorization request ledger: {e}"))?;
    log.append(EventEnvelope::new("authorization-requests", kind, payload)).map(|_| ()).map_err(|e| format!("could not write the authorization request ledger: {e}"))
}

/// Decide a request against the whole workspace and record the decision in the ledger, as one step under a lock. The caller then
/// writes the same payload to the mission log. An unreadable audit log or ledger refuses the request, loudly.
pub fn record_request(ws_dir: &Path, home: &Path, who: &Who, parsed: Result<Request, ParseError>, c: &Clock) -> Outcome {
    let _g = LEDGER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let loud = |m: String| Outcome::Refused { message: m, log: None };
    let operator = match read_audit(home) {
        Ok(o) => o,
        Err(e) => return loud(e),
    };
    let ledger = match read_ledger(ws_dir) {
        Ok(l) => l,
        Err(e) => return loud(e),
    };
    let out = admit(&ledger, &operator, who, parsed, c);
    let written = match &out {
        Outcome::Staged(p) => append_ledger(ws_dir, STAGED, p.clone()),
        Outcome::Refused { log: Some(p), .. } => append_ledger(ws_dir, REFUSED, p.clone()),
        Outcome::Refused { log: None, .. } => Ok(()),
    };
    match written {
        Ok(()) => out,
        Err(e) => loud(e),
    }
}

/// Add a payload to the ledger as the daemon would (for tests and for the operator-side checks that read it back).
pub fn ledger_staged(ledger: &[EventEnvelope]) -> std::collections::HashMap<String, Value> {
    ledger.iter().filter(|e| e.kind == STAGED).filter_map(|e| e.payload["id"].as_str().map(|i| (i.to_string(), e.payload.clone()))).collect()
}

// ------------------------------------------------------------------------------------------------ review (TA-0r)

/// The dry run: recent recorded shell cards, replayed against the manifest. The cards are data; control and escape
/// characters are stripped before anything is shown.
pub fn dry_run(cards: &[Value], manifest: &[Entry]) -> Vec<String> {
    let shell: Vec<String> = cards
        .iter()
        .filter(|p| matches!(p["tool"].as_str(), Some("shell" | "run_command")))
        .filter_map(|p| p["payload"]["command"].as_str().or_else(|| p["summary"].as_str()).map(|s| strip_control(s).trim().to_string()))
        .collect();
    if shell.is_empty() {
        return vec!["no recorded shell cards to replay against; nothing shows what this would have matched".into()];
    }
    let mut out = vec![format!("{} recent recorded shell card(s) examined (as data)", shell.len())];
    for (i, e) in manifest.iter().enumerate() {
        let line = render_argv(&e.argv);
        let hits = shell.iter().filter(|c| **c == line).count();
        out.push(format!("entry {}: would have matched {} card(s)", i + 1, hits));
    }
    let unmatched = shell.iter().filter(|c| !manifest.iter().any(|e| **c == render_argv(&e.argv))).count();
    out.push(format!("{} card(s) match no entry and would be denied as before", unmatched));
    out
}

/// A structured diff against the operator's earlier grants: per entry, whether the same action, or the same binary, was granted before.
pub fn grant_diff(store: &Store, manifest: &[Entry]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, e) in manifest.iter().enumerate() {
        let same = store.records.iter().find(|r| r.manifest.iter().any(|x| x.argv == e.argv));
        let bin = store.records.iter().find(|r| r.manifest.iter().any(|x| x.argv.first() == e.argv.first()));
        out.push(match (same, bin) {
            (Some(r), _) => format!("entry {}: the same action was granted before in {}{}", i + 1, r.id, if r.is_revoked() { " (revoked)" } else { "" }),
            (None, Some(r)) => format!("entry {}: new arguments for a binary granted before in {}", i + 1, r.id),
            (None, None) => format!("entry {}: NEW; neither this action nor this binary has been granted before", i + 1),
        });
    }
    if store.records.is_empty() {
        out.push("no earlier grants".into());
    }
    out
}

#[cfg(test)]
mod tests;
