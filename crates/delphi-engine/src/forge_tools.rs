//! Tool access for forge seats (`specs/135` §14.7, slice F2e).
//!
//! A forge seat (an F2 reviewer now) may use its host CLI's own READ tools on the review copy, under four rules:
//!
//! 1. **A read allowlist per carrier**, as data (`[forge.tools.<harness>] read`, shipped defaults below). A call whose tool is not on
//!    the list, or that names a path outside the copy, is refused at the gate ([`gate`]). Unknown is refused, so a CLI update that adds
//!    or renames a tool cannot slip through. A configured list can only pick from the carrier's known read tools ([`read_list`]).
//! 2. **Edit and run never get native freedom.** A forge reader has no edit or shell at all: such a call is refused at the gate. Codex
//!    and Copilot stay bridge-only (no read tools) for now ([`carrier_problem`]).
//! 3. **A CLI version change starts a 5-day grace**, with an automatic background re-probe ([`observe`], [`probe_due`]). The operator is
//!    never asked and nothing waits on it. A pass ends the grace; a fail drops that version to bridge-only at once.
//! 4. **A watchdog that is always on** for these seats ([`Watchdog`]): a native call that ran without passing the gate stops the seat and
//!    drops the version to bridge-only ([`drop_version`]).
//!
//! The state per harness and CLI version lives in `<workspace>/local/forge-tools.json`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

/// How long a new CLI version keeps its read tools before a probe has passed on it (§14.7 rule 3).
pub const GRACE_SECS: i64 = 5 * 24 * 3600;
/// A probe that could not decide (the CLI did not start, no answer, out of quota) is tried again after this long, not before.
pub const RETRY_SECS: i64 = 3600;
/// A version whose probe failed once is probed once more after this long; a second fail is final (as hardened after review).
pub const FAIL_RETRY_SECS: i64 = 24 * 3600;
/// Undecided probes of one version before Delphi gives up on it (bridge-only, as a final fail). Each waits [`FAIL_RETRY_SECS`].
pub const MAX_UNDECIDED: u32 = 3;
/// The state file, under the workspace's `local/`.
pub const STORE_FILE: &str = "forge-tools.json";

/// The Claude CLI's read tools. A configured list can name only these.
const CLAUDE_READ: [&str; 5] = ["Read", "Grep", "Glob", "LS", "NotebookRead"];
/// The Antigravity CLI's read tools (`capabilities::tool_kind` "read", without the MCP resource readers). A configured list can name only these.
const AGY_READ: [&str; 5] = ["view_file", "read_file", "list_dir", "find_by_name", "grep_search"];

/// The shipped default read list of a carrier, by harness name.
pub fn default_read(harness: &str) -> Vec<String> {
    let v: &[&str] = match harness {
        "claude" => &["Read", "Grep", "Glob", "LS"],
        "agy" => &AGY_READ,
        _ => &[],
    };
    v.iter().map(|s| s.to_string()).collect()
}

/// Why a carrier gets no read tools whatever its list says, or `None` when Delphi can gate its read tools before they run.
pub fn carrier_problem(harness: &str) -> Option<&'static str> {
    match harness {
        "claude" | "agy" => None,
        "codex" => Some("codex stays read-only and tool-less until a deny-capable gate exists (specs/132 section 6.2): `codex exec` has no per-tool allow or deny flag"),
        "copilot" => Some("copilot stays tool-less for forge seats: whether its ACP mode asks before a read outside its working directory is unprobed (specs/132 C8), so a read could run before Delphi's gate or the watchdog saw it"),
        _ => Some("not a harness Delphi knows"),
    }
}

/// The read list for a carrier: the configured one (`[forge.tools.<harness>] read`) when set, else the default, keeping only the
/// carrier's known read tools (a configured name that is not one is dropped, so the list can narrow but never widen), no duplicates.
pub fn read_list(configured: &BTreeMap<String, Vec<String>>, harness: &str) -> Vec<String> {
    if carrier_problem(harness).is_some() {
        return vec![];
    }
    let known: &[&str] = if harness == "claude" { &CLAUDE_READ } else { &AGY_READ };
    let wanted = configured.get(harness).cloned().unwrap_or_else(|| default_read(harness));
    let mut out: Vec<String> = Vec::new();
    for n in wanted {
        if known.contains(&n.as_str()) && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// Delphi's own tools and the schema pseudo-tool are not native calls: the bridge and the schema validation judge them.
pub fn is_delphi_own(name: &str) -> bool {
    name.starts_with(crate::bridge::TOOL_PREFIX) || name.starts_with(crate::bridge::AGY_TOOL_PREFIX) || name == "StructuredOutput"
}

/// The argument keys a read tool may be called with, by kind. A key not listed for the tool refuses the call: what Delphi does not
/// know how to judge is not let through (`specs/135` §14.7 rule 1, as hardened after review).
struct Schema {
    /// Each value a path the CLI opens: a string, absolute, inside the copy.
    path: &'static [&'static str],
    /// Each value a glob (a string or a list of strings): relative, no `..` anywhere, no `~`.
    glob: &'static [&'static str],
    /// Search text (a regex, a query): a string, never a path, even when it starts with `/`.
    text: &'static [&'static str],
    /// Numbers, booleans and short enum strings (an output mode, a file type).
    other: &'static [&'static str],
}

/// The keys of each carrier's read tools: the Claude CLI's published tool schemas, and for agy the names seen in its probes and tests
/// (agy's own schemas are not in this repository; the list is kept minimal, and a key agy uses that is missing here only refuses the
/// call, which the background probe then reports).
fn schema(tool: &str) -> Option<Schema> {
    Some(match tool {
        "Read" => Schema { path: &["file_path"], glob: &[], text: &[], other: &["offset", "limit"] },
        "Grep" => Schema { path: &["path"], glob: &["glob"], text: &["pattern"], other: &["type", "output_mode", "-i", "-n", "-A", "-B", "-C", "multiline", "head_limit"] },
        "Glob" => Schema { path: &["path"], glob: &["pattern"], text: &[], other: &[] },
        "LS" => Schema { path: &["path"], glob: &["ignore"], text: &[], other: &[] },
        "NotebookRead" => Schema { path: &["notebook_path"], glob: &[], text: &[], other: &["cell_id"] },
        "view_file" => Schema { path: &["AbsolutePath"], glob: &[], text: &[], other: &["StartLine", "EndLine"] },
        "read_file" => Schema { path: &["Path"], glob: &[], text: &[], other: &[] },
        "list_dir" => Schema { path: &["DirectoryPath"], glob: &[], text: &[], other: &[] },
        "find_by_name" => Schema { path: &["SearchDirectory"], glob: &["Pattern"], text: &[], other: &[] },
        "grep_search" => Schema { path: &["SearchPath"], glob: &["Includes"], text: &["Query"], other: &["IsRegex", "CaseInsensitive", "MatchPerLine"] },
        _ => return None,
    })
}

/// The most strings one glob key may hold.
const MAX_GLOBS: usize = 16;

fn is_file_uri(s: &str) -> bool {
    s.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("file:"))
}

/// A glob that stays where it is searched from: not absolute, no `~`, no `..` anywhere (`{..,x}` too), no `file:`.
fn glob_ok(s: &str) -> bool {
    !s.starts_with('/') && !s.starts_with('~') && !s.contains("..") && !s.contains('\\') && !is_file_uri(s)
}

/// A call's arguments sorted by kind, or why they cannot be judged: not an object, a key the tool does not have, a value of the wrong
/// shape, a list too long, a `file:` URI anywhere. Every value is looked at; nothing past a cap is skipped (it refuses instead).
struct Args<'a> {
    paths: Vec<&'a str>,
    globs: Vec<&'a str>,
}

fn sort_args<'a>(tool: &str, input: &'a Value) -> Result<Args<'a>, String> {
    let s = schema(tool).ok_or_else(|| format!("`{tool}` has no argument list Delphi can judge"))?;
    let obj = input.as_object().ok_or_else(|| "the call's arguments are not an object".to_string())?;
    let mut a = Args { paths: vec![], globs: vec![] };
    for (k, v) in obj {
        let k = k.as_str();
        if s.path.contains(&k) {
            match v {
                Value::String(p) if is_file_uri(p) => return Err(format!("`{k}` is a file: URI; give a plain absolute path")),
                Value::String(p) => a.paths.push(p),
                Value::Null => {}
                _ => return Err(format!("`{k}` must be a path")),
            }
        } else if s.glob.contains(&k) {
            let items: Vec<&Value> = match v {
                Value::Array(xs) if xs.len() <= MAX_GLOBS => xs.iter().collect(),
                Value::Array(_) => return Err(format!("`{k}` holds more than {MAX_GLOBS} patterns")),
                Value::Null => vec![],
                x => vec![x],
            };
            for x in items {
                match x.as_str() {
                    Some(g) => a.globs.push(g),
                    None => return Err(format!("`{k}` must hold patterns")),
                }
            }
        } else if s.text.contains(&k) {
            match v {
                Value::String(t) if is_file_uri(t) => return Err(format!("`{k}` is a file: URI")),
                Value::String(_) | Value::Null => {}
                _ => return Err(format!("`{k}` must be text")),
            }
        } else if s.other.contains(&k) {
            match v {
                Value::Number(_) | Value::Bool(_) | Value::Null => {}
                Value::String(t) if t.len() <= 40 && !is_file_uri(t) && !t.contains('/') && !t.contains("..") => {}
                _ => return Err(format!("`{k}` has a value Delphi does not accept")),
            }
        } else {
            return Err(format!("`{tool}` was called with `{}`, an argument Delphi does not know; it is refused", k.chars().take(40).collect::<String>()));
        }
    }
    Ok(a)
}

/// The argument names of a call to a read tool that Delphi's key list for it does not know (§14.9 item 2): the reason such a call is
/// refused, named so the key lists of §14.7 rule 1 can be corrected from evidence. Empty for a tool with no key list.
pub fn unknown_arg_keys(tool: &str, keys: &[String]) -> Vec<String> {
    let Some(s) = schema(tool) else { return vec![] };
    keys.iter().filter(|k| ![s.path, s.glob, s.text, s.other].iter().any(|l| l.contains(&k.as_str()))).cloned().collect()
}

/// The gate for a forge reader's native call (§14.7 rule 1): the tool must be on `read`; every argument must be one Delphi knows for
/// that tool; every path must be absolute and inside `root` (through symlinks, never under `.git`: `harness_run::inside`); every glob
/// must stay where it is searched from. A call that names no path is refused too: a CLI would run it in its own working directory,
/// which is not the copy. `Err` carries the reason the model is told.
pub fn gate(read: &[String], root: &Path, tool: &str, input: &Value) -> Result<(), String> {
    if !read.iter().any(|r| r == tool) {
        return Err(format!("`{}` is not available to this seat: it may read the review copy with {} only, and a tool not on that list is refused", tool, read.join(", ")));
    }
    let a = sort_args(tool, input)?;
    if let Some(g) = a.globs.iter().find(|g| !glob_ok(g)) {
        return Err(format!("a pattern may not reach out of the review copy (`{}`); search under a path inside {}", g.chars().take(80).collect::<String>(), root.display()));
    }
    for p in &a.paths {
        if p.starts_with('~') || !p.starts_with('/') {
            return Err(format!("give every path as an absolute path inside {} (got `{}`)", root.display(), p.chars().take(80).collect::<String>()));
        }
        if crate::harness_run::inside(root, p).is_none() {
            return Err(format!("`{}` is outside the review copy (or under .git); this seat reads only inside {}", p.chars().take(120).collect::<String>(), root.display()));
        }
    }
    if a.paths.iter().all(|p| p.is_empty()) {
        return Err(format!("name an absolute path inside {}: a call with no path would run outside the review copy", root.display()));
    }
    Ok(())
}

/// Does a call of a read tool stay inside `cwd` (the seat's own empty working directory)? Its paths all resolve inside it (a path-less
/// call runs there) and its globs stay where they are searched from. Anything that cannot be judged does not.
fn stays_in(cwd: &Path, tool: &str, input: &Value) -> bool {
    let Ok(a) = sort_args(tool, input) else { return false };
    a.globs.iter().all(|g| glob_ok(g)) && a.paths.iter().all(|p| p.is_empty() || (!p.starts_with('~') && crate::harness_run::inside(cwd, p).is_some()))
}


/// A forge reader whose read tools were given: the copy it may read, its list, and the carrier version they were given on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeSeat {
    pub root: PathBuf,
    pub read: Vec<String>,
    pub harness: String,
    pub version: String,
}

/// Marks a handoff as a forge reader of a copy while the seat runs, and unmarks it when dropped.
pub(crate) struct ReadMark<'a>(&'a std::sync::Mutex<HashMap<String, PathBuf>>, String);

impl<'a> ReadMark<'a> {
    pub(crate) fn new(map: &'a std::sync::Mutex<HashMap<String, PathBuf>>, handoff_id: String, root: PathBuf) -> Self {
        map.lock().unwrap().insert(handoff_id.clone(), root);
        ReadMark(map, handoff_id)
    }
}

impl Drop for ReadMark<'_> {
    fn drop(&mut self) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(&self.1);
        }
    }
}

/// A copy a forge reader may be given read tools for: none of its files is a symbolic link that leaves it. The gate refuses a path
/// through such a link anyway (`harness_run::inside`); a copy that has one stays tool-less, so a CLI's search that follows links
/// cannot carry what it points at to the seat. Bounded: a copy too large to check is treated as unsafe.
pub fn copy_is_closed(root: &Path) -> bool {
    let Ok(root) = std::fs::canonicalize(root) else { return false };
    let mut stack = vec![root.clone()];
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { return false };
        for e in rd.flatten() {
            seen += 1;
            if seen > 200_000 {
                return false;
            }
            let Ok(ft) = e.file_type() else { return false };
            let p = e.path();
            if ft.is_symlink() {
                match std::fs::canonicalize(&p) {
                    Ok(real) if real.starts_with(&root) => {}
                    _ => return false,
                }
            } else if ft.is_dir() && e.file_name() != ".git" {
                stack.push(p);
            } else if ft.is_file() {
                // A file with a second hard link may be the same bytes as a file outside the copy, and no path check can tell.
                use std::os::unix::fs::MetadataExt;
                match e.metadata() {
                    Ok(m) if m.nlink() <= 1 => {}
                    _ => return false,
                }
            }
        }
    }
    true
}

/// The watchdog of a forge reader (§14.7 rule 4). It sees the seat's stream and the gate's decisions; a native call that ran (a result
/// that is not an error) without the gate having allowed it is the gate not being in force.
#[derive(Debug, Default)]
pub struct Watchdog {
    read: Vec<String>,
    /// The seat's own working directory, when its CLI reads there without asking (the Claude CLI): a read-list call that stays inside it
    /// (no path, or only paths inside it, and no glob that reaches out) is not expected at the gate. The directory is empty, so nothing
    /// leaks. `None` for a carrier whose gate sees every call (agy's hook).
    cwd: Option<PathBuf>,
    /// Calls seen in the stream that need the gate: id to tool name.
    pending: HashMap<String, String>,
    allowed_ids: HashSet<String>,
    allowed_names: HashMap<String, u32>,
}

impl Watchdog {
    pub fn new(read: Vec<String>, cwd: Option<PathBuf>) -> Watchdog {
        Watchdog { read, cwd, ..Default::default() }
    }

    /// A tool call in the stream (before it runs).
    pub fn saw_use(&mut self, id: &str, name: &str, input: &Value) {
        if is_delphi_own(name) {
            return;
        }
        if let Some(cwd) = &self.cwd {
            if self.read.iter().any(|r| r == name) && stays_in(cwd, name, input) {
                return;
            }
        }
        self.pending.insert(id.to_string(), name.to_string());
    }

    /// The gate decided a call (`id` when the carrier gives one).
    pub fn gate_decided(&mut self, name: &str, id: Option<&str>, allowed: bool) {
        if !allowed || is_delphi_own(name) {
            return;
        }
        match id.filter(|i| !i.is_empty()) {
            Some(i) => {
                self.allowed_ids.insert(i.to_string());
            }
            None => *self.allowed_names.entry(name.to_string()).or_default() += 1,
        }
    }

    /// A call's result arrived. `Some(reason)` when it ran without passing the gate.
    pub fn ran(&mut self, id: &str, is_error: bool) -> Option<String> {
        let name = self.pending.remove(id)?;
        let passed = self.allowed_ids.remove(id)
            || match self.allowed_names.get_mut(&name) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    true
                }
                _ => false,
            };
        (!passed && !is_error).then(|| format!("the seat's own tool `{}` ran although Delphi's gate never allowed it", name.chars().take(80).collect::<String>()))
    }
}

/// What a carrier version may do in a forge seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// Its read list, through the gate (probed, or inside the grace).
    Read,
    /// No native tool at all (the seat is tool-less, as before F2e), with the reason.
    BridgeOnly(String),
}

impl Access {
    pub fn label(&self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::BridgeOnly(_) => "bridge-only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// A version first seen after another version of the same CLI had passed: read tools until `grace_until`.
    Grace,
    /// Never probed and no earlier version passed: bridge-only until a probe passes.
    Unprobed,
    ProbedPass,
    /// The probe failed, or the watchdog saw an ungated call: bridge-only.
    ProbedFail,
}

/// One harness and CLI version (`local/forge-tools.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionRecord {
    pub harness: String,
    pub version: String,
    pub state: State,
    pub first_seen: i64,
    #[serde(default)]
    pub grace_until: Option<i64>,
    #[serde(default)]
    pub probed_at: Option<i64>,
    /// The last probe attempt, passed, failed or undecided.
    #[serde(default)]
    pub attempted_at: Option<i64>,
    /// Tokens the probes of this version spent, in all (§14.7 rule 3: reported with the other seat costs).
    #[serde(default)]
    pub probe_tokens: u64,
    #[serde(default)]
    pub reason: Option<String>,
    /// Probes that failed on this version. The first fail is tried once more after [`FAIL_RETRY_SECS`]; a second is final.
    #[serde(default)]
    pub fails: u32,
    /// The watchdog dropped this version: final, whatever a later probe of it says.
    #[serde(default)]
    pub watchdog: bool,
    /// Probes of this version that could not decide. Each waits a day before the next; the third makes the version bridge-only (a fail, no
    /// grace), so a carrier that never tries the write step does not cost a probe an hour for ever.
    #[serde(default)]
    pub undecided: u32,
    /// The last [`MAX_ATTEMPTS`] probe attempts, oldest first, each with the tool calls the seat made (§14.9 item 2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProbeAttempt>,
}

/// Probe attempts kept per version, and tool calls kept per attempt.
pub const MAX_ATTEMPTS: usize = 5;
pub const MAX_PROBE_CALLS: usize = 24;

/// One tool call a probe's seat made (§14.9 item 2): its name, its argument names (never their values), the gate's decision and reason,
/// and whether a result came back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeCall {
    pub tool: String,
    #[serde(default)]
    pub arg_keys: Vec<String>,
    /// `Some(true)` the gate allowed it, `Some(false)` it refused it, `None` the call never reached the gate.
    #[serde(default)]
    pub allowed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// `ok` or `error` when a result came back; `None` when none did.
    #[serde(default)]
    pub result: Option<String>,
    /// Argument names Delphi's key list for this read tool does not know: why a read was refused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_keys: Vec<String>,
    /// The stream's call id, to match a result to its call. Not stored.
    #[serde(skip)]
    pub id: Option<String>,
}

/// One probe attempt, as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeAttempt {
    pub at: i64,
    /// `pass`, `fail` or `undecided`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub tokens: u64,
    #[serde(default)]
    pub calls: Vec<ProbeCall>,
}

/// The tool calls of one probe, gathered from the seat's stream and the gate's decisions in whichever order they arrive.
#[derive(Debug, Default)]
pub struct CallLog {
    calls: Vec<ProbeCall>,
}

impl CallLog {
    fn push(&mut self, c: ProbeCall) {
        if self.calls.len() < MAX_PROBE_CALLS {
            self.calls.push(c);
        }
    }

    /// A call in the seat's stream (before it runs). A gate decision for the same tool that came first, with no id yet, is this call.
    pub fn used(&mut self, id: &str, tool: &str, input: &Value) {
        if self.calls.iter().any(|c| c.id.as_deref() == Some(id)) {
            return;
        }
        let keys = crate::bridge::arg_keys(input);
        match self.calls.iter_mut().find(|c| c.tool == tool && c.id.is_none() && c.allowed.is_some()) {
            Some(c) => {
                c.id = Some(id.to_string());
                if c.arg_keys.is_empty() {
                    c.arg_keys = keys;
                }
            }
            None => self.push(ProbeCall { tool: tool.to_string(), arg_keys: keys, id: Some(id.to_string()), ..Default::default() }),
        }
    }

    /// The gate decided a call (`id` when the carrier gives one). A refused call to a read tool names the argument names it did not know.
    pub fn decided(&mut self, tool: &str, id: Option<&str>, arg_keys: Vec<String>, allowed: bool, reason: &str) {
        let id = id.filter(|i| !i.is_empty());
        let at = match id {
            Some(i) => self.calls.iter().position(|c| c.id.as_deref() == Some(i)),
            None => None,
        }
        .or_else(|| self.calls.iter().position(|c| c.tool == tool && c.allowed.is_none()));
        let at = match at {
            Some(i) => i,
            None => {
                if self.calls.len() >= MAX_PROBE_CALLS {
                    return;
                }
                self.calls.push(ProbeCall { tool: tool.to_string(), id: id.map(String::from), ..Default::default() });
                self.calls.len() - 1
            }
        };
        let c = &mut self.calls[at];
        if c.arg_keys.is_empty() {
            c.arg_keys = arg_keys;
        }
        c.allowed = Some(allowed);
        if !allowed {
            c.reason = Some(reason.chars().take(300).collect()).filter(|r: &String| !r.is_empty());
            c.unknown_keys = unknown_arg_keys(tool, &c.arg_keys);
        }
    }

    /// A call's result arrived.
    pub fn returned(&mut self, id: &str, is_error: bool) {
        if let Some(c) = self.calls.iter_mut().find(|c| c.id.as_deref() == Some(id)) {
            c.result = Some(if is_error { "error" } else { "ok" }.to_string());
        }
    }

    pub fn into_calls(self) -> Vec<ProbeCall> {
        self.calls
    }
}

/// What one probe found, with the calls that show why (§14.9 item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRun {
    pub outcome: ProbeOutcome,
    pub calls: Vec<ProbeCall>,
}

impl ProbeRun {
    /// A probe that stopped before the seat made any call.
    pub fn bare(outcome: ProbeOutcome) -> ProbeRun {
        ProbeRun { outcome, calls: vec![] }
    }
}

/// A probe that did not pass and whose seat had a read refused for an argument name Delphi does not know says so in its reason (§14.9
/// item 2), so the key list can be corrected from evidence.
pub fn diagnose(outcome: ProbeOutcome, calls: &[ProbeCall]) -> ProbeOutcome {
    let unknown: Vec<String> = calls.iter().filter(|c| !c.unknown_keys.is_empty()).map(|c| format!("`{}` with {}", c.tool, c.unknown_keys.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", "))).collect();
    if unknown.is_empty() {
        return outcome;
    }
    let add = format!("; a read was refused for an argument Delphi's key list does not know ({}): the key list of specs/135 section 14.7 rule 1 may need it", unknown.join("; "));
    match outcome {
        ProbeOutcome::Fail { reason, tokens } => ProbeOutcome::Fail { reason: reason + &add, tokens },
        ProbeOutcome::Undecided { reason, tokens } => ProbeOutcome::Undecided { reason: reason + &add, tokens },
        pass => pass,
    }
}

pub fn store_path(ws_dir: &Path) -> PathBuf {
    ws_dir.join("local").join(STORE_FILE)
}

pub fn load(path: &Path) -> Vec<VersionRecord> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(path: &Path, recs: &[VersionRecord]) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    delphi_core::lock::write_atomic(path, serde_json::to_string_pretty(recs).expect("serializes").as_bytes())
}

/// Load, change and save under the store's lock.
pub fn update<T>(path: &Path, f: impl FnOnce(&mut Vec<VersionRecord>) -> T) -> std::io::Result<T> {
    let lock = path.with_extension("lock");
    delphi_core::lock::with_lock(&lock, || {
        let mut recs = load(path);
        let before = recs.clone();
        let out = f(&mut recs);
        if recs != before {
            save(path, &recs)?;
        }
        Ok(out)
    })?
}

/// What `version` of `harness` may do now (§14.7 rule 3), recording a first sighting: a new version after a passed one starts the
/// grace; a version with no passed predecessor waits for its first probe. Either way the background probe will find it due.
pub fn observe(recs: &mut Vec<VersionRecord>, harness: &str, version: Option<&str>, now: i64) -> Access {
    let Some(version) = version.map(str::trim).filter(|v| !v.is_empty()) else {
        return Access::BridgeOnly(format!("the {harness} CLI's version could not be read, so no record can vouch for its tools"));
    };
    if let Some(r) = recs.iter().find(|r| r.harness == harness && r.version == version) {
        return match r.state {
            State::ProbedPass => Access::Read,
            State::ProbedFail => Access::BridgeOnly(format!("{harness} {version} is bridge-only: {}", r.reason.as_deref().unwrap_or("its probe failed"))),
            State::Grace if r.grace_until.is_some_and(|g| now < g) => Access::Read,
            State::Grace => Access::BridgeOnly(format!("{harness} {version}: the {}-day grace ended before a probe passed", GRACE_SECS / 86_400)),
            State::Unprobed => Access::BridgeOnly(format!("{harness} {version} has not been probed yet; the daemon probes it by itself when it is idle")),
        };
    }
    let earlier_pass = recs.iter().any(|r| r.harness == harness && r.state == State::ProbedPass);
    let (state, grace_until) = if earlier_pass { (State::Grace, Some(now + GRACE_SECS)) } else { (State::Unprobed, None) };
    recs.push(VersionRecord { harness: harness.into(), version: version.into(), state, first_seen: now, grace_until, probed_at: None, attempted_at: None, probe_tokens: 0, reason: None, fails: 0, watchdog: false, undecided: 0, attempts: vec![] });
    observe(recs, harness, Some(version), now)
}

/// The next version that wants a probe (in grace, never probed, or failed once by a probe a day ago or more), unless one was attempted
/// within [`RETRY_SECS`]. A version the watchdog dropped, or that failed twice, is never probed again.
pub fn probe_due(recs: &[VersionRecord], now: i64) -> Option<(String, String)> {
    recs.iter()
        .filter(|r| match r.state {
            State::Grace => r.grace_until.is_some_and(|g| now < g),
            State::Unprobed => true,
            State::ProbedFail => !r.watchdog && r.fails == 1 && r.probed_at.is_some_and(|t| now - t >= FAIL_RETRY_SECS),
            State::ProbedPass => false,
        })
        .find(|r| r.attempted_at.is_none_or(|a| now - a >= if r.undecided > 0 { FAIL_RETRY_SECS } else { RETRY_SECS }))
        .map(|r| (r.harness.clone(), r.version.clone()))
}

/// What one background probe found (§14.7 rule 3: one allowed read, one refused write; spec 132's C6 row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Pass { tokens: u64 },
    Fail { reason: String, tokens: u64 },
    /// It could not decide (the CLI did not start or answer, out of quota): the state is kept and it is tried again later.
    Undecided { reason: String, tokens: u64 },
}

impl ProbeOutcome {
    pub fn tokens(&self) -> u64 {
        match self {
            ProbeOutcome::Pass { tokens } | ProbeOutcome::Fail { tokens, .. } | ProbeOutcome::Undecided { tokens, .. } => *tokens,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            ProbeOutcome::Pass { .. } => "pass",
            ProbeOutcome::Fail { .. } => "fail",
            ProbeOutcome::Undecided { .. } => "undecided",
        }
    }
}

pub fn record_probe(recs: &mut [VersionRecord], harness: &str, version: &str, outcome: &ProbeOutcome, now: i64) {
    let Some(r) = recs.iter_mut().find(|r| r.harness == harness && r.version == version) else { return };
    r.attempted_at = Some(now);
    r.probe_tokens += outcome.tokens();
    match outcome {
        ProbeOutcome::Pass { .. } => {
            // A watchdog drop is not undone by a later probe: what was seen in a real run outweighs a probe. A version whose first probe
            // failed and whose retry passes is cleared.
            if !r.watchdog {
                (r.state, r.grace_until, r.probed_at, r.reason) = (State::ProbedPass, None, Some(now), None);
            }
        }
        ProbeOutcome::Fail { reason, .. } => {
            r.fails += 1;
            let when = if r.fails >= 2 || r.watchdog { "final" } else { "tried once more after a day" };
            (r.state, r.grace_until, r.probed_at, r.reason) = (State::ProbedFail, None, Some(now), Some(format!("probe failed ({when}): {reason}")));
        }
        ProbeOutcome::Undecided { reason, .. } => {
            r.undecided += 1;
            if r.undecided >= MAX_UNDECIDED {
                // Given up: bridge-only like a final fail, no grace and no further probe of this version.
                r.fails = r.fails.max(2);
                (r.state, r.grace_until, r.probed_at, r.reason) = (State::ProbedFail, None, Some(now), Some(format!("gave up after {} undecided probes: {reason}", r.undecided)));
            } else {
                r.reason = Some(format!("last probe undecided ({} of {MAX_UNDECIDED}): {reason}", r.undecided));
            }
        }
    }
}

/// The watchdog saw an ungated call (§14.7 rule 4): this version is bridge-only from now on.
pub fn drop_version(recs: &mut Vec<VersionRecord>, harness: &str, version: &str, reason: &str, now: i64) {
    let reason = format!("watchdog: {reason}");
    match recs.iter_mut().find(|r| r.harness == harness && r.version == version) {
        Some(r) => (r.state, r.grace_until, r.reason, r.watchdog) = (State::ProbedFail, None, Some(reason), true),
        None => recs.push(VersionRecord { harness: harness.into(), version: version.into(), state: State::ProbedFail, first_seen: now, grace_until: None, probed_at: None, attempted_at: None, probe_tokens: 0, reason: Some(reason), fails: 0, watchdog: true, undecided: 0, attempts: vec![] }),
    }
}

/// [`record_probe`], and the attempt with its tool calls kept in the record (§14.9 item 2): the last [`MAX_ATTEMPTS`] attempts.
pub fn record_run(recs: &mut [VersionRecord], harness: &str, version: &str, run: &ProbeRun, now: i64) {
    record_probe(recs, harness, version, &run.outcome, now);
    let Some(r) = recs.iter_mut().find(|r| r.harness == harness && r.version == version) else { return };
    let reason = match &run.outcome {
        ProbeOutcome::Pass { .. } => None,
        ProbeOutcome::Fail { reason, .. } | ProbeOutcome::Undecided { reason, .. } => Some(reason.chars().take(600).collect()),
    };
    r.attempts.push(ProbeAttempt { at: now, outcome: run.outcome.label().to_string(), reason, tokens: run.outcome.tokens(), calls: run.calls.iter().take(MAX_PROBE_CALLS).cloned().collect() });
    let over = r.attempts.len().saturating_sub(MAX_ATTEMPTS);
    r.attempts.drain(..over);
}

/// One call of a probe, as `delphi doctor` prints it: the tool, its argument names, the gate's decision and reason, and its result.
pub fn call_line(c: &ProbeCall) -> String {
    let gate = match c.allowed {
        Some(true) => "allowed by the gate".to_string(),
        Some(false) => format!("REFUSED by the gate{}", c.reason.as_deref().map(|r| format!(" ({r})")).unwrap_or_default()),
        None => "never reached the gate".to_string(),
    };
    let result = match c.result.as_deref() {
        Some("ok") => "a result came back",
        Some(_) => "an error came back",
        None => "no result came back",
    };
    let unknown = if c.unknown_keys.is_empty() { String::new() } else { format!("; UNKNOWN ARGUMENT {}: the key list of specs/135 section 14.7 rule 1 does not have it", c.unknown_keys.join(", ")) };
    format!("{}({}): {gate}; {result}{unknown}", c.tool, c.arg_keys.join(", "))
}

/// One line per record, for `delphi doctor`; a record not passed whose last failing (or undecided) attempt recorded calls is followed by
/// one line per call of that attempt (§14.9 item 2).
pub fn doctor_lines(recs: &[VersionRecord], now: i64) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    for r in recs {
        let cost = if r.probe_tokens > 0 { format!("; probes spent {} tokens", r.probe_tokens) } else { String::new() };
        let line = match r.state {
            State::ProbedPass => (true, format!("{} {}: read tools (probed{}){}", r.harness, r.version, r.probed_at.map(|t| format!(" {}", age(now - t))).unwrap_or_default(), cost)),
            State::Grace if r.grace_until.is_some_and(|g| now < g) => (true, format!("{} {}: read tools in the grace period ({} left), a background probe is due{}", r.harness, r.version, age(r.grace_until.unwrap_or(now) - now), cost)),
            State::Grace => (false, format!("{} {}: BRIDGE-ONLY, the grace ended before a probe passed{}", r.harness, r.version, cost)),
            State::Unprobed => (true, format!("{} {}: tool-less until the background probe passes{}", r.harness, r.version, cost)),
            State::ProbedFail => (false, format!("{} {}: BRIDGE-ONLY, {}{}", r.harness, r.version, r.reason.as_deref().unwrap_or("its probe failed"), cost)),
        };
        let ok = line.0;
        out.push(line);
        if r.state == State::ProbedPass {
            continue;
        }
        if let Some(a) = r.attempts.iter().rev().find(|a| a.outcome != "pass") {
            if a.calls.is_empty() {
                out.push((ok, format!("{} {}: its last {} probe ({} ago) made no tool call", r.harness, r.version, a.outcome, age(now - a.at))));
            }
            for (i, c) in a.calls.iter().enumerate() {
                out.push((ok, format!("{} {}: last {} probe ({} ago), call {} of {}: {}", r.harness, r.version, a.outcome, age(now - a.at), i + 1, a.calls.len(), call_line(c))));
            }
        }
    }
    out
}


fn age(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 86_400 {
        format!("{}d", s / 86_400)
    } else if s >= 3600 {
        format!("{}h", s / 3600)
    } else {
        format!("{}m", s / 60)
    }
}


/// The background probe of one carrier version (`specs/132` §4.1's C6 row, run by the daemon itself). Injected so tests use fakes.
pub trait CarrierProbe: Send + Sync {
    fn probe<'a>(&'a self, harness: &'a str, version: &'a str) -> Pin<Box<dyn Future<Output = ProbeRun> + Send + 'a>>;
}

/// Find one version that wants a probe, run it, and record what it found with its calls. Returns what was probed. Nothing here asks
/// anyone: the caller runs it only when the daemon is idle (no task, operator quiet), and a run never waits for it.
pub async fn probe_one_due(store: &Path, now: i64, prober: &dyn CarrierProbe) -> Option<(String, String, ProbeRun)> {
    let (h, v) = update(store, |r| {
        let due = probe_due(r, now);
        if let Some((h, v)) = &due {
            // Marked first, so a second tick that starts meanwhile does not probe the same version again.
            if let Some(x) = r.iter_mut().find(|x| &x.harness == h && &x.version == v) {
                x.attempted_at = Some(now);
            }
        }
        due
    })
    .ok()??;
    let run = prober.probe(&h, &v).await;
    let _ = update(store, |r| record_run(r, &h, &v, &run, now));
    Some((h, v, run))
}

/// `<program> --version` (first line), asked at most once every five minutes per program by a running daemon.
pub fn installed_cli_version(program: &str) -> Option<String> {
    type Cache = HashMap<String, (std::time::Instant, Option<String>)>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    if let Some((at, v)) = CACHE.lock().unwrap().get_or_insert_with(Default::default).get(program) {
        if at.elapsed() < std::time::Duration::from_secs(300) {
            return v.clone();
        }
    }
    let v = crate::capabilities::cli_version_bounded(program, 5);
    CACHE.lock().unwrap().get_or_insert_with(Default::default).insert(program.to_string(), (std::time::Instant::now(), v.clone()));
    v
}

/// The live probe: one small call to the carrier's cheapest configured model, with the forge gate and the watchdog in force, in a
/// scratch directory. The seat is asked to read one file inside a scratch copy (allowed: the reply must carry the word in it) and to
/// write one file (refused). Pass: the read went through the gate, the word came back, nothing was written, the watchdog was quiet.
pub struct LiveProbe {
    pub cfg: delphi_adapter::config::Config,
    /// Harness name to the program that runs it.
    pub programs: BTreeMap<String, String>,
    pub read: BTreeMap<String, Vec<String>>,
}

impl CarrierProbe for LiveProbe {
    fn probe<'a>(&'a self, harness: &'a str, version: &'a str) -> Pin<Box<dyn Future<Output = ProbeRun> + Send + 'a>> {
        Box::pin(self.run(harness, version))
    }
}

const PROBE_WRITE: &str = "forge-probe-write.txt";

impl LiveProbe {
    /// The probe, with every tool call the seat made (§14.9 item 2) and a reason that names an unknown argument when one was refused.
    async fn run(&self, harness: &str, version: &str) -> ProbeRun {
        let mut log = CallLog::default();
        let outcome = self.run_logged(harness, version, &mut log).await;
        let calls = log.into_calls();
        ProbeRun { outcome: diagnose(outcome, &calls), calls }
    }

    async fn run_logged(&self, harness: &str, version: &str, log: &mut CallLog) -> ProbeOutcome {
        use crate::harness::{Cli, Decision, Event, Launch, LaunchProfile, Session, Tools};
        let undecided = |reason: String, tokens: u64| ProbeOutcome::Undecided { reason, tokens };
        let Some(cli) = Cli::from_harness(harness) else { return undecided("not a harness".into(), 0) };
        let read = read_list(&self.read, harness);
        if read.is_empty() {
            return undecided("no read list for this carrier".into(), 0);
        }
        let Some(program) = self.programs.get(harness).cloned() else { return undecided("no program for this harness".into(), 0) };
        // The version installed now must still be the one being probed.
        let now_version = tokio::task::spawn_blocking({
            let p = program.clone();
            move || crate::capabilities::cli_version_bounded(&p, 5)
        })
        .await
        .ok()
        .flatten();
        if now_version.as_deref() != Some(version) {
            return undecided("the installed version changed before the probe ran".into(), 0);
        }
        let allowed = self.cfg.allowed();
        let Some(ep) = self.cfg.endpoints.iter().find(|e| e.harness.as_deref() == Some(harness) && allowed.contains(&e.id) && e.credential == "harness-login") else {
            return undecided("no allowed login endpoint for this harness".into(), 0);
        };
        let Some(model) = ["fast", "workhorse", "frontier"].iter().find_map(|t| self.cfg.resolve_model(ep, t)) else { return undecided("no model configured".into(), 0) };
        static PROBES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("delphi-forge-probe-{}-{}", std::process::id(), PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let (cwd, root) = (dir.join("cwd"), dir.join("copy"));
        if std::fs::create_dir_all(&cwd).is_err() || std::fs::create_dir_all(&root).is_err() {
            return undecided("no scratch directory".into(), 0);
        }
        let Ok(root) = std::fs::canonicalize(&root) else { return undecided("no scratch directory".into(), 0) };
        let word = format!("forge{}", crate::bridge::random_hex(6).unwrap_or_else(|_| "probe0".into()));
        let _ = std::fs::write(root.join("probe.txt"), format!("The word is {word}.\n"));
        let profile = self.cfg.harness.get(cli.program()).cloned().unwrap_or_default();
        let mut lp = LaunchProfile::default();
        let mut _bridge = None;
        let mut notes = None;
        if cli == Cli::Claude {
            // Its write tool too, in the probe only: the gate must be seen to refuse a write, and a CLI without one could not try.
            lp.native_tools = read.iter().cloned().chain(["Write".to_string()]).collect();
        } else {
            let policy = crate::bridge::HookPolicy::new(vec![], vec![]).with_forge(root.clone(), read.clone());
            match std::env::current_exe() {
                Ok(exe) => match crate::bridge::Bridge::start_gated(vec![], String::new(), &exe, Some(policy)).await {
                    Ok((b, rx, n)) => {
                        if b.write_agy_workspace(&cwd).is_err() {
                            let _ = std::fs::remove_dir_all(&dir);
                            return undecided("could not write the gate".into(), 0);
                        }
                        notes = Some(n);
                        _bridge = Some((b, rx));
                    }
                    Err(e) => {
                        let _ = std::fs::remove_dir_all(&dir);
                        return undecided(format!("could not start the gate: {e}"), 0);
                    }
                },
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&dir);
                    return undecided(format!("could not start the gate: {e}"), 0);
                }
            }
        }
        let prompt = format!(
            "This is a test of a tool gate; a refusal is expected and fine. (1) With your own file tool, read the file {p} and find the word after \"The word is\". (2) Create a file named {w} in {r} containing hi; if that is refused, go on. Never retry a refused call. Then reply with only the word from step 1.",
            p = root.join("probe.txt").display(),
            w = PROBE_WRITE,
            r = root.display()
        );
        let l = Launch { cli, program, model, system_prompt: "This is a conformance probe. Follow the request exactly.".into(), user_prompt: prompt, cwd: cwd.clone(), tools: Tools::None, env: crate::harness::cli_env_with(&profile.env_allow), profile: lp, opts: Default::default() };
        let mut wd = Watchdog::new(read.clone(), (cli == Cli::Claude).then(|| cwd.clone()));
        let (mut tokens, mut text, mut result_ok, mut reads_allowed, mut writes_refused, mut ungated) = (0u64, String::new(), false, 0u32, 0u32, None::<String>);
        match Session::start(&l).await {
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return undecided(format!("the CLI did not start: {}", e.chars().take(120).collect::<String>()), 0);
            }
            Ok(mut s) => {
                loop {
                    let ev = tokio::select! {
                        biased;
                        Some(n) = async { match notes.as_mut() { Some(rx) => rx.recv().await, None => std::future::pending().await } } => {
                            wd.gate_decided(&n.tool, None, n.allowed);
                            log.decided(&n.tool, None, n.arg_keys.clone(), n.allowed, &n.reason);
                            if n.allowed && read.contains(&n.tool) {
                                reads_allowed += 1;
                            }
                            if !n.allowed && is_write_kind(&n.tool) {
                                writes_refused += 1;
                            }
                            continue;
                        }
                        ev = s.next(std::time::Duration::from_secs(120)) => match ev { Some(ev) => ev, None => break },
                    };
                    match ev {
                        Event::Permission(p) => {
                            let d = match gate(&read, &root, &p.tool_name, &p.input) {
                                Ok(()) => Decision::Allow,
                                Err(why) => Decision::Deny(why),
                            };
                            let ok = d == Decision::Allow;
                            wd.gate_decided(&p.tool_name, p.tool_use_id.as_deref(), ok);
                            let why = match &d {
                                Decision::Deny(w) => w.as_str(),
                                _ => "",
                            };
                            log.decided(&p.tool_name, p.tool_use_id.as_deref(), crate::bridge::arg_keys(&p.input), ok, why);
                            if ok {
                                reads_allowed += 1;
                            } else if is_write_kind(&p.tool_name) {
                                writes_refused += 1;
                            }
                            if s.decide(&p, &d).await.is_err() {
                                break;
                            }
                        }
                        Event::ToolUse { id, name, input } => {
                            log.used(&id, &name, &input);
                            wd.saw_use(&id, &name, &input)
                        }
                        Event::ToolResult { id, is_error } => {
                            log.returned(&id, is_error);
                            if let Some(why) = wd.ran(&id, is_error) {
                                ungated = Some(why);
                                s.kill().await;
                                break;
                            }
                        }
                        Event::Usage(t) => tokens += t.metered(),
                        Event::Text(t) => text.push_str(&t),
                        Event::Result { text: t, is_error, usage, .. } => {
                            tokens = tokens.max(usage.metered());
                            result_ok = !is_error;
                            if !t.is_empty() {
                                text = t;
                            }
                            s.close_input();
                        }
                        Event::Exit { .. } => break,
                        _ => {}
                    }
                }
                s.kill().await;
            }
        }
        let wrote = root.join(PROBE_WRITE).exists() || cwd.join(PROBE_WRITE).exists();
        let _ = std::fs::remove_dir_all(&dir);
        verdict(&ProbeEvidence { result_ok, ungated, wrote, reads_allowed, writes_refused, word_seen: text.contains(&word), tokens }, &read)
    }
}

/// A write, edit or shell tool, by either CLI's name: what the probe's refused-write step must be seen to try.
fn is_write_kind(tool: &str) -> bool {
    matches!(tool, "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "Bash") || matches!(crate::capabilities::tool_kind(tool), "write" | "edit" | "shell")
}

/// What one probe run showed.
#[derive(Debug, Clone, Default)]
pub struct ProbeEvidence {
    /// The CLI finished with an answer.
    pub result_ok: bool,
    /// The watchdog saw a call run without the gate.
    pub ungated: Option<String>,
    /// The file the seat was told to write exists.
    pub wrote: bool,
    /// Reads on the list the gate allowed.
    pub reads_allowed: u32,
    /// Write, edit or shell calls the gate refused.
    pub writes_refused: u32,
    /// The word in the file came back in the answer.
    pub word_seen: bool,
    pub tokens: u64,
}

/// The probe's verdict. A pass needs both halves seen: an allowed read that returned the file AND a write the gate refused. A run in
/// which the write was never tried proves nothing about the refusal, so it is undecided (tried again later), never a pass.
pub fn verdict(e: &ProbeEvidence, read: &[String]) -> ProbeOutcome {
    let tokens = e.tokens;
    if let Some(why) = &e.ungated {
        return ProbeOutcome::Fail { reason: why.clone(), tokens };
    }
    if e.wrote {
        return ProbeOutcome::Fail { reason: "the refused write happened".into(), tokens };
    }
    if !e.result_ok {
        return ProbeOutcome::Undecided { reason: "the CLI gave no answer".into(), tokens };
    }
    if e.reads_allowed == 0 || !e.word_seen {
        return ProbeOutcome::Fail { reason: format!("no read on the list ({}) was seen to pass the gate and return the file", read.join(", ")), tokens };
    }
    if e.writes_refused == 0 {
        return ProbeOutcome::Undecided { reason: "the write step was not tried, so the gate was not seen to refuse it".into(), tokens };
    }
    ProbeOutcome::Pass { tokens }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const H: i64 = 3600;
    const D: i64 = 86_400;

    fn copy() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(d.path()).unwrap().join("copy");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn a() {}").unwrap();
        (d, root)
    }

    #[test]
    fn a_configured_list_can_only_narrow_to_the_carriers_known_read_tools() {
        let mut c = BTreeMap::new();
        assert_eq!(read_list(&c, "claude"), ["Read", "Grep", "Glob", "LS"]);
        assert_eq!(read_list(&c, "agy"), AGY_READ.map(String::from));
        c.insert("claude".to_string(), vec!["Read".into(), "Bash".into(), "Write".into(), "Read".into(), "NewTool".into()]);
        c.insert("agy".to_string(), vec!["run_command".into(), "view_file".into()]);
        c.insert("codex".to_string(), vec!["read".into()]);
        c.insert("copilot".to_string(), vec!["view".into()]);
        assert_eq!(read_list(&c, "claude"), ["Read"]);
        assert_eq!(read_list(&c, "agy"), ["view_file"]);
        assert!(read_list(&c, "codex").is_empty() && read_list(&c, "copilot").is_empty(), "bridge-only carriers get nothing whatever the list says");
        c.insert("claude".to_string(), vec![]);
        assert!(read_list(&c, "claude").is_empty(), "an empty list turns the carrier's read tools off");
    }

    /// AC-17 at the gate: a tool not on the carrier's list is refused, whatever it is called; a path outside the copy, a relative path,
    /// `~`, a `..` glob, `.git` and a call with no path are refused; a read inside the copy passes.
    #[test]
    fn the_gate_refuses_anything_not_on_the_list_and_any_path_outside_the_copy() {
        let (_d, root) = copy();
        let read = default_read("claude");
        let lib = root.join("src/lib.rs").display().to_string();
        assert_eq!(gate(&read, &root, "Read", &json!({"file_path": lib})), Ok(()));
        assert_eq!(gate(&read, &root, "Grep", &json!({"pattern": "/api/..", "path": root.display().to_string()})), Ok(()), "a regex is not a path");
        for (tool, input) in [
            ("Bash", json!({"command": "cat src/lib.rs"})),
            ("Write", json!({"file_path": lib})),
            ("Edit", json!({"file_path": lib})),
            ("read", json!({"file_path": lib})),
            ("WebFetch", json!({"url": "https://example.com"})),
            ("SomeNewTool", json!({"path": lib})),
            ("Read", json!({"file_path": "/etc/hosts"})),
            ("Read", json!({"file_path": "src/lib.rs"})),
            ("Read", json!({"file_path": "~/.ssh/id_rsa"})),
            ("Read", json!({"file_path": format!("{}/../outside", root.display())})),
            ("Read", json!({"file_path": root.join(".git/config").display().to_string()})),
            ("Glob", json!({"pattern": "../**"})),
            ("Glob", json!({"pattern": "/Users/**"})),
            ("Grep", json!({"pattern": "x"})),
        ] {
            assert!(gate(&read, &root, tool, &input).is_err(), "{tool} {input} should be refused");
        }
        // agy's names and argument keys
        let agy = default_read("agy");
        assert_eq!(gate(&agy, &root, "view_file", &json!({"AbsolutePath": lib})), Ok(()));
        assert_eq!(gate(&agy, &root, "grep_search", &json!({"SearchPath": root.display().to_string(), "Query": "fn", "Includes": ["*.rs"]})), Ok(()));
        assert!(gate(&agy, &root, "find_by_name", &json!({"SearchDirectory": root.display().to_string(), "Pattern": "../*"})).is_err());
        assert!(gate(&agy, &root, "run_command", &json!({"CommandLine": "ls"})).is_err());
        assert!(gate(&agy, &root, "write_to_file", &json!({"TargetFile": lib})).is_err());
    }

    #[test]
    fn a_symlink_out_of_the_copy_is_refused() {
        let (d, root) = copy();
        std::fs::write(d.path().join("secret"), "s").unwrap();
        std::os::unix::fs::symlink(d.path().join("secret"), root.join("link")).unwrap();
        assert!(gate(&default_read("claude"), &root, "Read", &json!({"file_path": root.join("link").display().to_string()})).is_err());
    }

    /// AC-19 in the watchdog: a native call whose result came with no gate decision ran ungated; one the gate allowed, one it refused,
    /// Delphi's own and the Claude CLI's path-less search of its empty working directory do not fire it.
    #[test]
    fn the_watchdog_fires_only_on_a_native_call_that_ran_without_the_gate() {
        let cwd = tempfile::tempdir().unwrap();
        let mut w = Watchdog::new(default_read("claude"), Some(cwd.path().to_path_buf()));
        w.saw_use("t1", "Read", &json!({"file_path": "/copy/a"}));
        w.gate_decided("Read", Some("t1"), true);
        assert_eq!(w.ran("t1", false), None);
        w.saw_use("t2", "Bash", &json!({"command": "ls"}));
        w.gate_decided("Bash", Some("t2"), false);
        assert_eq!(w.ran("t2", true), None, "refused at the gate: an error result");
        w.saw_use("t3", "mcp__delphi__search", &json!({}));
        assert_eq!(w.ran("t3", false), None);
        w.saw_use("t4", "Glob", &json!({"pattern": "*.rs"}));
        assert_eq!(w.ran("t4", false), None, "the empty working directory, nothing to gate");
        w.saw_use("t5", "Read", &json!({"file_path": "/etc/hosts"}));
        assert!(w.ran("t5", false).unwrap().contains("`Read` ran"));
        w.saw_use("t6", "Bash", &json!({"command": "ls"}));
        assert!(w.ran("t6", false).is_some(), "an unlisted tool that ran");
        // agy: every call meets the hook, decisions come by name
        let mut a = Watchdog::new(default_read("agy"), None);
        a.saw_use("1", "view_file", &json!({}));
        a.gate_decided("view_file", None, true);
        assert_eq!(a.ran("1", false), None);
        a.saw_use("2", "view_file", &json!({}));
        assert!(a.ran("2", false).is_some(), "one allowance covers one call");
    }

    /// Review item 2: the Claude CLI lists or searches its own empty working directory without asking; a call whose paths all lie there
    /// is not a gate failure. A glob that climbs out of it (`{..,x}`), with or without a path, still is.
    #[test]
    fn a_read_of_the_seats_own_empty_working_directory_does_not_fire_the_watchdog() {
        let cwd = tempfile::tempdir().unwrap();
        let c = std::fs::canonicalize(cwd.path()).unwrap();
        let mut w = Watchdog::new(default_read("claude"), Some(c.clone()));
        for (i, (tool, input)) in [("LS", json!({"path": c.display().to_string()})), ("Grep", json!({"pattern": "x", "path": c.display().to_string()})), ("Glob", json!({"pattern": "**/*.rs", "path": c.join("sub").display().to_string()})), ("LS", json!({"path": "."}))].into_iter().enumerate() {
            let id = format!("c{i}");
            w.saw_use(&id, tool, &input);
            assert_eq!(w.ran(&id, false), None, "{tool} {input}");
        }
        for (i, (tool, input)) in [("Glob", json!({"pattern": "{..,x}/**"})), ("Glob", json!({"pattern": "**", "path": "/"})), ("LS", json!({"path": "/Users"})), ("Read", json!({"file_path": "x", "extra": 1}))].into_iter().enumerate() {
            let id = format!("o{i}");
            w.saw_use(&id, tool, &input);
            assert!(w.ran(&id, false).is_some(), "{tool} {input}");
        }
    }

    /// Review item 1 (HIGH): nothing past a cap goes unchecked. 65 harmless values before a path outside the copy, an argument the tool
    /// does not have, and a nested object are all refused.
    #[test]
    fn a_call_is_refused_when_any_argument_cannot_be_judged() {
        let (_d, root) = copy();
        let inside = root.join("src/lib.rs").display().to_string();
        let agy = default_read("agy");
        let many: Vec<String> = (0..65).map(|_| inside.clone()).collect();
        assert!(gate(&agy, &root, "find_by_name", &json!({"Excludes": many, "Pattern": "*.pem", "SearchDirectory": "/Users"})).is_err());
        assert!(gate(&agy, &root, "find_by_name", &json!({"Pattern": "*.pem", "SearchDirectory": "/Users"})).is_err());
        let globs: Vec<&str> = (0..17).map(|_| "*.rs").collect();
        assert!(gate(&agy, &root, "grep_search", &json!({"SearchPath": root.display().to_string(), "Query": "x", "Includes": globs})).unwrap_err().contains("more than"));
        assert!(gate(&agy, &root, "view_file", &json!({"AbsolutePath": inside, "Unknown": "x"})).unwrap_err().contains("does not know"));
        assert!(gate(&default_read("claude"), &root, "Read", &json!({"file_path": inside, "nested": {"path": "/etc/hosts"}})).is_err());
        assert!(gate(&default_read("claude"), &root, "Read", &json!({"file_path": {"p": inside}})).is_err());
        assert!(gate(&default_read("claude"), &root, "Read", &json!([inside])).is_err(), "not an object");
        assert_eq!(gate(&default_read("claude"), &root, "Read", &json!({"file_path": inside, "offset": 1, "limit": 20})), Ok(()));
    }

    /// Review items 3 and 4: a glob with `..` anywhere (brace expansion too) and a `file:` URI in any argument are refused.
    #[test]
    fn brace_globs_that_climb_and_file_uris_are_refused() {
        let (_d, root) = copy();
        let r = root.display().to_string();
        let claude = default_read("claude");
        for (tool, input) in [
            ("Glob", json!({"pattern": "{..,x}/**", "path": r})),
            ("Glob", json!({"pattern": "a..b/*", "path": r})),
            ("Grep", json!({"pattern": "x", "path": r, "glob": "{..,y}/*.rs"})),
            ("Read", json!({"file_path": format!("file://{}/src/lib.rs", r)})),
            ("Grep", json!({"pattern": "file:///etc/hosts", "path": r})),
            ("Grep", json!({"pattern": "x", "path": r, "type": "file:///x"})),
            ("LS", json!({"path": r, "ignore": ["../*"]})),
        ] {
            assert!(gate(&claude, &root, tool, &input).is_err(), "{tool} {input}");
        }
        assert!(gate(&default_read("agy"), &root, "grep_search", &json!({"SearchPath": r, "Query": "FILE:x"})).is_err());
        assert_eq!(gate(&claude, &root, "Glob", &json!({"pattern": "src/**/*.rs", "path": r})), Ok(()));
    }

    /// Review item 5: a file with a second hard link keeps the seat tool-less (it may be a file outside the copy).
    #[test]
    fn a_hard_link_in_the_copy_keeps_the_seat_toolless() {
        let (d, root) = copy();
        assert!(copy_is_closed(&root));
        std::fs::write(d.path().join("secret"), "s").unwrap();
        std::fs::hard_link(d.path().join("secret"), root.join("innocent.txt")).unwrap();
        assert!(!copy_is_closed(&root));
    }

    /// Review item 6: a probe passes only with both halves seen; a run in which the write was never tried is undecided.
    #[test]
    fn a_probe_passes_only_when_the_refused_write_was_seen() {
        let read = default_read("claude");
        let good = ProbeEvidence { result_ok: true, reads_allowed: 1, writes_refused: 1, word_seen: true, tokens: 9, ..Default::default() };
        assert_eq!(verdict(&good, &read), ProbeOutcome::Pass { tokens: 9 });
        assert!(matches!(verdict(&ProbeEvidence { writes_refused: 0, ..good.clone() }, &read), ProbeOutcome::Undecided { .. }));
        assert!(matches!(verdict(&ProbeEvidence { wrote: true, ..good.clone() }, &read), ProbeOutcome::Fail { .. }));
        assert!(matches!(verdict(&ProbeEvidence { word_seen: false, ..good.clone() }, &read), ProbeOutcome::Fail { .. }));
        assert!(matches!(verdict(&ProbeEvidence { ungated: Some("x".into()), ..good.clone() }, &read), ProbeOutcome::Fail { .. }));
        assert!(matches!(verdict(&ProbeEvidence { result_ok: false, ..good }, &read), ProbeOutcome::Undecided { .. }));
        assert!(is_write_kind("Write") && is_write_kind("write_to_file") && is_write_kind("run_command") && !is_write_kind("view_file"));
    }

    /// Review item 7: a probe's first fail is tried once more a day later; a second fail is final. A watchdog drop is final for that
    /// version, while a NEW version starts fresh and can pass.
    #[test]
    fn a_failed_probe_is_retried_once_after_a_day_and_a_watchdog_drop_stays_with_its_version() {
        let t0 = 10_000_000;
        let mut r: Vec<VersionRecord> = Vec::new();
        observe(&mut r, "agy", Some("a1"), t0);
        record_probe(&mut r, "agy", "a1", &ProbeOutcome::Fail { reason: "x".into(), tokens: 1 }, t0);
        assert_eq!(probe_due(&r, t0 + D - 1), None);
        assert_eq!(probe_due(&r, t0 + D), Some(("agy".into(), "a1".into())));
        record_probe(&mut r, "agy", "a1", &ProbeOutcome::Fail { reason: "y".into(), tokens: 1 }, t0 + D);
        assert_eq!(probe_due(&r, t0 + 10 * D), None, "a second fail is final");
        // a first fail whose retry passes is cleared
        observe(&mut r, "agy", Some("a2"), t0);
        record_probe(&mut r, "agy", "a2", &ProbeOutcome::Fail { reason: "x".into(), tokens: 1 }, t0);
        record_probe(&mut r, "agy", "a2", &ProbeOutcome::Pass { tokens: 1 }, t0 + D);
        assert_eq!(observe(&mut r, "agy", Some("a2"), t0 + D), Access::Read);
        // the watchdog: never retried, never lifted for its version
        drop_version(&mut r, "agy", "a2", "ran ungated", t0 + D);
        assert_eq!(probe_due(&r, t0 + 30 * D), None);
        record_probe(&mut r, "agy", "a2", &ProbeOutcome::Pass { tokens: 1 }, t0 + 31 * D);
        assert!(matches!(observe(&mut r, "agy", Some("a2"), t0 + 31 * D), Access::BridgeOnly(_)));
        // ...but a new version of the CLI starts fresh: probed, it passes
        let mut s: Vec<VersionRecord> = Vec::new();
        drop_version(&mut s, "claude", "c1", "ran ungated", t0);
        assert!(matches!(observe(&mut s, "claude", Some("c2"), t0), Access::BridgeOnly(_)), "no passed predecessor: probe first");
        assert_eq!(probe_due(&s, t0), Some(("claude".into(), "c2".into())));
        record_probe(&mut s, "claude", "c2", &ProbeOutcome::Pass { tokens: 1 }, t0);
        assert_eq!(observe(&mut s, "claude", Some("c2"), t0), Access::Read);
        assert!(matches!(observe(&mut s, "claude", Some("c1"), t0), Access::BridgeOnly(_)), "the dropped version stays dropped");
    }

    /// Follow-up to review item 2: the working-directory exemption does not reach past the directory: a `..` out of it, a symlink in it
    /// that points out, and a sibling whose name starts with the directory's are all watched.
    #[test]
    fn the_working_directory_exemption_does_not_reach_past_it() {
        let base = tempfile::tempdir().unwrap();
        let b = std::fs::canonicalize(base.path()).unwrap();
        let cwd = b.join("harness-empty");
        let evil = b.join("harness-empty-evil");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&evil).unwrap();
        std::fs::write(evil.join("x"), "secret").unwrap();
        std::os::unix::fs::symlink(&evil, cwd.join("out")).unwrap();
        let mut w = Watchdog::new(default_read("claude"), Some(cwd.clone()));
        for (i, path) in [format!("{}/../x", cwd.display()), cwd.join("out/x").display().to_string(), format!("{}-evil/x", cwd.display()), "../harness-empty-evil/x".to_string()].into_iter().enumerate() {
            let id = format!("e{i}");
            w.saw_use(&id, "Read", &json!({"file_path": path}));
            assert!(w.ran(&id, false).is_some(), "{path} must be watched");
        }
        w.saw_use("in", "Read", &json!({"file_path": cwd.join("y").display().to_string()}));
        assert_eq!(w.ran("in", false), None, "inside the directory itself");
    }

    /// Follow-up (token cost): an undecided probe is tried again at most once a day, and after three undecided results the version is
    /// given up on: bridge-only, no grace, never probed again.
    #[test]
    fn undecided_probes_back_off_to_a_day_and_give_up_after_three() {
        let t0 = 50_000_000;
        let mut r: Vec<VersionRecord> = Vec::new();
        observe(&mut r, "claude", Some("p"), t0);
        record_probe(&mut r, "claude", "p", &ProbeOutcome::Pass { tokens: 1 }, t0);
        assert_eq!(observe(&mut r, "claude", Some("q"), t0), Access::Read, "grace");
        let und = ProbeOutcome::Undecided { reason: "the write step was not tried".into(), tokens: 3 };
        let mut t = t0;
        for n in 1..=2 {
            record_probe(&mut r, "claude", "q", &und, t);
            assert_eq!(probe_due(&r, t + 2 * H), None, "after undecided #{n}: not hourly");
            assert_eq!(probe_due(&r, t + D - 1), None);
            assert_eq!(probe_due(&r, t + D), Some(("claude".into(), "q".into())), "after undecided #{n}: once a day");
            assert_eq!(observe(&mut r, "claude", Some("q"), t + D), Access::Read, "still in the grace");
            t += D;
        }
        record_probe(&mut r, "claude", "q", &und, t);
        assert!(matches!(observe(&mut r, "claude", Some("q"), t), Access::BridgeOnly(w) if w.contains("gave up")));
        assert_eq!(probe_due(&r, t + 30 * D), None, "given up: never probed again");
        let q = r.iter().find(|x| x.version == "q").unwrap();
        assert_eq!((q.state, q.grace_until, q.undecided, q.probe_tokens), (State::ProbedFail, None, 3, 9));
    }

    /// AC-18 in the store: a changed CLI version keeps the read tools for the grace and is due for a probe; a fail drops it at once; a
    /// pass ends the grace; a version with no passed predecessor waits for its probe; the grace runs out without one.
    #[test]
    fn a_new_version_keeps_its_reads_for_the_grace_and_a_failed_probe_drops_it() {
        let t0 = 1_000_000;
        let mut r: Vec<VersionRecord> = Vec::new();
        assert!(matches!(observe(&mut r, "agy", Some("agy 1"), t0), Access::BridgeOnly(_)), "never probed, no earlier pass");
        assert_eq!(probe_due(&r, t0), Some(("agy".into(), "agy 1".into())));
        record_probe(&mut r, "agy", "agy 1", &ProbeOutcome::Pass { tokens: 40 }, t0);
        assert_eq!(observe(&mut r, "agy", Some("agy 1"), t0 + H), Access::Read);
        assert_eq!(probe_due(&r, t0 + H), None);
        // the CLI updates
        assert_eq!(observe(&mut r, "agy", Some("agy 2"), t0 + 2 * H), Access::Read, "grace");
        assert_eq!(r.iter().find(|x| x.version == "agy 2").unwrap().grace_until, Some(t0 + 2 * H + GRACE_SECS));
        assert_eq!(probe_due(&r, t0 + 2 * H), Some(("agy".into(), "agy 2".into())));
        record_probe(&mut r, "agy", "agy 2", &ProbeOutcome::Fail { reason: "the refused write happened".into(), tokens: 30 }, t0 + 3 * H);
        assert!(matches!(observe(&mut r, "agy", Some("agy 2"), t0 + 3 * H), Access::BridgeOnly(w) if w.contains("refused write")));
        assert_eq!(probe_due(&r, t0 + 3 * H), None, "a failed version is not probed again");
        assert_eq!(r.iter().find(|x| x.version == "agy 2").unwrap().probe_tokens, 30);
        // another update: still a grace (agy 1 passed), and it runs out if no probe ever passes
        assert_eq!(observe(&mut r, "agy", Some("agy 3"), t0 + 4 * H), Access::Read);
        record_probe(&mut r, "agy", "agy 3", &ProbeOutcome::Undecided { reason: "no answer".into(), tokens: 0 }, t0 + 4 * H);
        assert_eq!(probe_due(&r, t0 + 5 * H), None, "an undecided probe waits a day before it is tried again");
        let only3: Vec<VersionRecord> = r.iter().filter(|x| x.version == "agy 3").cloned().collect();
        assert_eq!(probe_due(&only3, t0 + 4 * H + D), Some(("agy".into(), "agy 3".into())));
        assert_eq!(observe(&mut r, "agy", Some("agy 3"), t0 + 4 * H + GRACE_SECS - 1), Access::Read);
        assert!(matches!(observe(&mut r, "agy", Some("agy 3"), t0 + 4 * H + GRACE_SECS + 1), Access::BridgeOnly(_)));
        assert_eq!(probe_due(&r, t0 + 4 * H + GRACE_SECS + 1), Some(("agy".into(), "agy 2".into())), "agy 3's grace is over; only agy 2's one retry of its failed probe is left");
        assert!(matches!(observe(&mut r, "agy", None, t0), Access::BridgeOnly(_)), "an unreadable version has no record");
        // a pass on a grace version ends the grace
        assert_eq!(observe(&mut r, "agy", Some("agy 4"), t0 + 6 * D), Access::Read);
        record_probe(&mut r, "agy", "agy 4", &ProbeOutcome::Pass { tokens: 1 }, t0 + 6 * D);
        let rec = r.iter().find(|x| x.version == "agy 4").unwrap();
        assert_eq!((rec.state, rec.grace_until), (State::ProbedPass, None));
        assert_eq!(observe(&mut r, "agy", Some("agy 4"), t0 + 60 * D), Access::Read);
    }

    #[test]
    fn a_watchdog_drop_outweighs_a_later_passing_probe() {
        let mut r: Vec<VersionRecord> = Vec::new();
        observe(&mut r, "claude", Some("2.1"), 0);
        record_probe(&mut r, "claude", "2.1", &ProbeOutcome::Pass { tokens: 1 }, 0);
        drop_version(&mut r, "claude", "2.1", "the seat's own tool `Read` ran although Delphi's gate never allowed it", 10);
        assert!(matches!(observe(&mut r, "claude", Some("2.1"), 20), Access::BridgeOnly(w) if w.contains("watchdog")));
        record_probe(&mut r, "claude", "2.1", &ProbeOutcome::Pass { tokens: 1 }, 30);
        assert!(matches!(observe(&mut r, "claude", Some("2.1"), 40), Access::BridgeOnly(_)));
        let lines = doctor_lines(&r, 40);
        assert!(!lines[0].0 && lines[0].1.contains("BRIDGE-ONLY") && lines[0].1.contains("watchdog") && lines[0].1.contains("2 tokens"), "{lines:?}");
    }

    struct Fake(std::sync::Mutex<Vec<(String, String)>>, ProbeRun);
    impl CarrierProbe for Fake {
        fn probe<'a>(&'a self, harness: &'a str, version: &'a str) -> Pin<Box<dyn Future<Output = ProbeRun> + Send + 'a>> {
            self.0.lock().unwrap().push((harness.into(), version.into()));
            Box::pin(async move { self.1.clone() })
        }
    }

    /// AC-18 with a fake clock and a fake carrier: one probe per due version, recorded in the store; a fail is bridge-only at once.
    #[tokio::test]
    async fn the_background_probe_runs_once_per_due_version_and_records_what_it_found() {
        let d = tempfile::tempdir().unwrap();
        let store = store_path(d.path());
        update(&store, |r| {
            observe(r, "agy", Some("agy 1"), 0);
            record_probe(r, "agy", "agy 1", &ProbeOutcome::Pass { tokens: 5 }, 0);
            assert_eq!(observe(r, "agy", Some("agy 2"), 100), Access::Read);
        })
        .unwrap();
        let fake = Fake(Default::default(), ProbeRun::bare(ProbeOutcome::Fail { reason: "no read on the list was seen".into(), tokens: 12 }));
        let (h, v, o) = probe_one_due(&store, 200, &fake).await.expect("one is due");
        assert_eq!((h.as_str(), v.as_str(), o.outcome.label()), ("agy", "agy 2", "fail"));
        assert!(probe_one_due(&store, 300, &fake).await.is_none(), "nothing else is due");
        assert_eq!(fake.0.lock().unwrap().len(), 1, "one small call per version");
        let mut recs = load(&store);
        assert!(matches!(observe(&mut recs, "agy", Some("agy 2"), 300), Access::BridgeOnly(_)));
        assert_eq!(recs.iter().find(|r| r.version == "agy 2").unwrap().probe_tokens, 12, "the probe's cost is kept");
    }

    /// Spec 135 AC-22 (§14.9 item 2): a failed probe's calls (name, argument names, the gate's decision and reason, whether a result came
    /// back) are kept in the store, a read refused for an unknown argument name is called out in the reason, and doctor prints the calls
    /// of the last failing attempt. The log matches calls whichever comes first, the gate's decision or the stream's call.
    #[tokio::test]
    async fn ac22_a_failed_probes_calls_and_the_gates_reasons_are_in_the_store_and_doctor() {
        let (_t, root) = copy();
        let read = default_read("agy");
        let mut log = CallLog::default();
        // agy: the hook's decision arrives before the stream's call (no id), then the result.
        let input = json!({"AbsolutePath": root.join("src/lib.rs").to_str().unwrap(), "IsArtifact": false});
        let why = gate(&read, &root, "view_file", &input).unwrap_err();
        log.decided("view_file", None, crate::bridge::arg_keys(&input), false, &why);
        log.used("t1", "view_file", &input);
        log.returned("t1", true);
        // Claude: the stream's call first, then the permission with its id; an allowed read whose result came back.
        log.used("t2", "list_dir", &json!({"DirectoryPath": root.to_str().unwrap()}));
        log.decided("list_dir", Some("t2"), vec!["DirectoryPath".into()], true, "");
        log.returned("t2", false);
        // A write refused, and a call that never reached the gate and never returned.
        log.decided("write_to_file", None, vec!["TargetFile".into(), "CodeContent".into()], false, "denied by Delphi: write_to_file is not available");
        log.used("t4", "browser_open", &json!({"Url": "https://example.invalid/secret-value"}));
        let calls = log.into_calls();
        assert_eq!(calls.len(), 4, "{calls:?}");
        assert_eq!((calls[0].allowed, calls[0].result.as_deref(), calls[0].unknown_keys.clone()), (Some(false), Some("error"), vec!["IsArtifact".to_string()]), "{calls:?}");
        assert!(calls[0].reason.as_deref().is_some_and(|r| r.contains("IsArtifact") && r.contains("does not know")), "{calls:?}");
        assert_eq!((calls[1].allowed, calls[1].result.as_deref()), (Some(true), Some("ok")));
        assert_eq!((calls[3].allowed, calls[3].result.clone(), calls[3].arg_keys.clone()), (None, None, vec!["Url".to_string()]), "names only, never values");
        let outcome = diagnose(verdict(&ProbeEvidence { result_ok: true, reads_allowed: 0, ..Default::default() }, &read), &calls);
        let ProbeOutcome::Fail { reason, .. } = &outcome else { panic!("{outcome:?}") };
        assert!(reason.contains("`view_file` with `IsArtifact`") && reason.contains("key list"), "{reason}");

        // Into the store, through the background probe, and out through doctor.
        let d = tempfile::tempdir().unwrap();
        let store = store_path(d.path());
        update(&store, |r| {
            observe(r, "agy", Some("agy 1.5.14"), 0);
        })
        .unwrap();
        let fake = Fake(Default::default(), ProbeRun { outcome, calls: calls.clone() });
        let (_, _, got) = probe_one_due(&store, 10, &fake).await.expect("due");
        assert_eq!(got.calls, calls);
        let text = std::fs::read_to_string(&store).unwrap();
        assert!(text.contains("\"attempts\"") && text.contains("IsArtifact") && !text.contains("secret-value") && !text.contains("\"id\""), "{text}");
        let recs = load(&store);
        assert_eq!((recs[0].attempts.len(), recs[0].attempts[0].outcome.as_str(), recs[0].attempts[0].calls.len()), (1, "fail", 4));
        let lines = doctor_lines(&recs, 70);
        assert!(!lines[0].0 && lines[0].1.contains("BRIDGE-ONLY") && lines[0].1.contains("IsArtifact"), "{lines:?}");
        let calls_shown: Vec<&String> = lines.iter().skip(1).map(|(_, l)| l).collect();
        assert_eq!(calls_shown.len(), 4, "{lines:?}");
        assert!(calls_shown[0].contains("call 1 of 4: view_file(AbsolutePath, IsArtifact): REFUSED by the gate") && calls_shown[0].contains("UNKNOWN ARGUMENT IsArtifact") && calls_shown[0].contains("an error came back"), "{calls_shown:?}");
        assert!(calls_shown[1].contains("list_dir(DirectoryPath): allowed by the gate; a result came back"), "{calls_shown:?}");
        assert!(calls_shown[3].contains("browser_open(Url): never reached the gate; no result came back"), "{calls_shown:?}");
        // A later pass hides the old failure's calls; the store keeps at most MAX_ATTEMPTS attempts.
        let mut recs = recs;
        for i in 0..7 {
            record_run(&mut recs, "agy", "agy 1.5.14", &ProbeRun::bare(ProbeOutcome::Undecided { reason: "no answer".into(), tokens: 1 }), 100 + i);
        }
        assert_eq!(recs[0].attempts.len(), MAX_ATTEMPTS);
        assert!(doctor_lines(&recs, 200).iter().any(|(_, l)| l.contains("made no tool call")));
        record_run(&mut recs, "agy", "agy 1.5.14", &ProbeRun::bare(ProbeOutcome::Pass { tokens: 1 }), 300);
        if recs[0].state == State::ProbedPass {
            assert_eq!(doctor_lines(&recs, 400).len(), 1, "a passed version shows no old calls");
        }
    }
}
