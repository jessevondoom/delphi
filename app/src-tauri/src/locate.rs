//! Finding the daemon (spec 111 §3.2 item 1). The app asks the operator's own `delphi` where its sockets are and how roles
//! look (`delphi daemon where`, the command the CLI keeps for clients in other languages). A window started from the Finder has
//! no shell `PATH`, so the program is looked for in the places an install puts it. If it cannot be asked, the narrator socket of
//! the default workspace is computed the way the daemon computes it, and the window uses its built-in avatar table.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

pub struct Located {
    /// What `delphi daemon where` printed, or the little the app can say by itself.
    pub info: Value,
    pub narrator: PathBuf,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// The Delphi home: `DELPHI_HOME`, else `~/.delphi` (as `crates/delphi/src/home.rs` resolves it).
pub fn delphi_home() -> Option<PathBuf> {
    env_path("DELPHI_HOME").or_else(|| env_path("HOME").map(|h| h.join(".delphi")))
}

/// Where `delphi` may be: `DELPHI_EXE` first, then the standard install locations.
/// The program inside the app's own bundle (spec 122 §10.1 item 3): `<exe>/../../Resources/delphi` for an app executable at
/// `Delphi.app/Contents/MacOS/<name>`. A path only; nothing is read or run here.
pub fn bundled_path(app_exe: &Path) -> Option<PathBuf> {
    let contents = app_exe.parent()?.parent()?;
    (contents.file_name()? == "Contents").then(|| contents.join("Resources/delphi"))
}

fn own_bundled() -> Option<PathBuf> {
    bundled_path(&std::env::current_exe().ok()?)
}

/// The places an install puts `delphi`, without the bundled copy.
pub fn installed_candidates(user_home: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = env_path("DELPHI_EXE").into_iter().collect();
    if let Some(h) = user_home {
        v.push(h.join(".local/bin/delphi"));
    }
    v.push(PathBuf::from("/opt/homebrew/bin/delphi"));
    v.push(PathBuf::from("/usr/local/bin/delphi"));
    v
}

/// Where `delphi` may be: `DELPHI_EXE` first, then the standard install locations, then the copy in this app's bundle (last,
/// so an installed program is always preferred).
pub fn candidates(user_home: Option<&Path>) -> Vec<PathBuf> {
    let mut v = installed_candidates(user_home);
    v.extend(own_bundled());
    v
}

/// What the start screen needs to offer "Install the delphi command" (spec 122 §10.1 item 3): whether an installed `delphi`
/// exists, and the bundled one's path when this app has one. Reads file metadata only; nothing is run.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Program {
    pub installed: bool,
    pub bundled: Option<String>,
}

pub fn program_in(user_home: Option<&Path>, bundled: Option<PathBuf>) -> Program {
    let installed = installed_candidates(user_home).iter().any(|p| p.is_file());
    let bundled = bundled.filter(|p| p.is_file()).and_then(|p| p.to_str().map(String::from));
    Program { installed, bundled }
}

pub fn program() -> Program {
    program_in(env_path("HOME").as_deref(), own_bundled())
}

/// The narrator socket of a workspace, as `delphi_core::spawn::layout` names it: `<home>/run/<h>.narrator.sock`, `h` the first
/// twelve hex characters of the blake3 hash of the workspace's canonical path.
pub fn narrator_socket(home: &Path, workspace: &Path) -> Option<PathBuf> {
    let canon = std::fs::canonicalize(workspace).ok()?;
    let hash = blake3::hash(canon.as_os_str().as_encoded_bytes()).to_hex();
    Some(home.join("run").join(format!("{}.narrator.sock", &hash[..12])))
}

/// The app socket of a workspace (spec 120 §3 item 1), named as the daemon names it: `<home>/run/<h>.app.sock`, beside the
/// narrator's and with the same `h`.
pub fn app_socket(home: &Path, workspace: &Path) -> Option<PathBuf> {
    let narrator = narrator_socket(home, workspace)?;
    let name = narrator.file_name()?.to_str()?.strip_suffix(".narrator.sock")?.to_string();
    Some(narrator.with_file_name(format!("{name}.app.sock")))
}

/// The app socket `delphi daemon where` named (`app_socket`), when it is an absolute path. A `delphi` from before spec 120
/// names none, and the app then stays on the narrator socket.
pub fn app_socket_in(info: &Value) -> Option<PathBuf> {
    info["app_socket"].as_str().map(PathBuf::from).filter(|p| p.is_absolute())
}

/// What `delphi daemon where` printed, when it is a JSON object naming a narrator socket. The output is data: nothing in it is run.
pub fn parse_where(stdout: &[u8]) -> Option<(Value, PathBuf)> {
    let v: Value = serde_json::from_slice(stdout).ok()?;
    let sock = PathBuf::from(v.get("narrator_socket")?.as_str()?);
    (v.is_object() && sock.is_absolute()).then_some((v, sock))
}

fn ask(exe: &Path, home: Option<&Path>, workspace: &str) -> Option<(Value, PathBuf)> {
    let mut cmd = Command::new(exe);
    cmd.args(["daemon", "where", "--workspace", workspace]);
    if let Some(h) = home {
        cmd.arg("--home").arg(h);
    }
    // No terminal, no input, and a directory that exists whatever started the app.
    let out = cmd.stdin(Stdio::null()).stderr(Stdio::null()).current_dir("/").output().ok()?;
    out.status.success().then(|| parse_where(&out.stdout)).flatten()
}

/// Find the daemon's narrator socket and the roles' look. `None` when there is no Delphi home to look in.
pub fn locate() -> Option<Located> {
    let user_home = env_path("HOME");
    let home = delphi_home()?;
    let workspace = std::env::var("DELPHI_WORKSPACE").ok().filter(|w| !w.is_empty()).unwrap_or_else(|| "default".into());
    let explicit_home = env_path("DELPHI_HOME");
    for exe in candidates(user_home.as_deref()) {
        if exe.is_file() {
            if let Some((info, narrator)) = ask(&exe, explicit_home.as_deref(), &workspace) {
                return Some(Located { info, narrator });
            }
        }
    }
    let ws_dir = home.join("workspaces").join(&workspace);
    let narrator = narrator_socket(&home, &ws_dir)?;
    let app = app_socket(&home, &ws_dir);
    Some(Located { info: json!({ "narrator_socket": narrator, "app_socket": app, "workspace": ws_dir, "workspace_name": workspace, "home": home, "roles": {} }), narrator })
}

// ── Discovery (spec 130 §2, §3): every daemon on this machine, from the presence files in `<home>/run`. ────────────────────
//
// A presence file is data a daemon wrote, and nothing it says is trusted: it is read only when it is a plain file of the
// operator's own that nobody else can read or write, it is bounded, parsed and shown as text, and the sockets that are
// probed are the ones this program names from the file's own name (`<hash>.json` -> `<hash>.narrator.sock`), never a path
// the file gives. A daemon is "running" only when its socket answers and what answers is the daemon that wrote the file.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::client::{Client, Method};

/// At most this many files of the run folder are looked at, and this many sockets listed.
pub const RUN_ENTRIES_MAX: usize = 32;
/// A presence file is a few hundred bytes; one larger than this is not one.
pub const PRESENCE_MAX_BYTES: u64 = 4096;
/// At most this many daemons are asked for a summary at a time, each for at most [`SUMMARY_TIMEOUT`], and no daemon is asked
/// again within [`SUMMARY_EVERY`].
pub const SUMMARIES_MAX: usize = 8;
pub const SUMMARY_TIMEOUT: Duration = Duration::from_secs(2);
pub const SUMMARY_EVERY: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_millis(1000);

/// What a daemon's presence file says (spec 130 §2). Only what a card shows; the file's socket paths are not kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Presence {
    pub instance: String,
    pub pid: u64,
    pub binary_version: String,
    pub workspace: String,
    pub workspace_name: String,
    pub home: String,
    pub started: String,
    /// The newest stable release the daemon says it found (`1.2.3`), and whether it says an update was applied and it awaits a restart
    /// (spec 120 §13.2 item 12). Optional fields of the file: absent means nothing is known.
    pub update_available: Option<String>,
    pub restart_pending: bool,
}

/// A release as the daemon writes it: three numbers and nothing else. A value of any other shape is not shown.
fn release(v: &Value, key: &str) -> Option<String> {
    let s = v.get(key)?.as_str()?;
    let parts: Vec<&str> = s.split('.').collect();
    (parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.len() <= 4 && p.bytes().all(|b| b.is_ascii_digit()))).then(|| s.to_string())
}

/// Twelve lower-case hex characters: the workspace hash that names a daemon's files.
pub fn is_hash(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn text(v: &Value, key: &str, max: usize) -> Option<String> {
    v.get(key)?.as_str().filter(|s| s.len() <= max && !s.chars().any(char::is_control)).map(String::from)
}

/// A presence file's bytes as data: a JSON object with `version` 1 and the fields of spec 130 §2, whose `narrator_socket`
/// is named `<hash>.narrator.sock` (the hash being the file's own name). Anything else is `None`.
pub fn parse_presence(bytes: &[u8], hash: &str) -> Option<Presence> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    if !v.is_object() || v["version"].as_u64() != Some(1) || !is_hash(hash) {
        return None;
    }
    let socket = text(&v, "narrator_socket", 1024)?;
    if Path::new(&socket).file_name()?.to_str()? != format!("{hash}.narrator.sock") {
        return None;
    }
    let instance = text(&v, "instance", 64).filter(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_hexdigit()))?;
    Some(Presence {
        instance,
        pid: v["pid"].as_u64()?,
        binary_version: text(&v, "binary_version", 64)?,
        workspace: text(&v, "workspace", 1024)?,
        workspace_name: text(&v, "workspace_name", 128)?,
        home: text(&v, "home", 1024)?,
        started: text(&v, "started", 64).unwrap_or_default(),
        update_available: release(&v, "update_available"),
        restart_pending: v.get("restart_pending").and_then(|b| b.as_bool()).unwrap_or(false),
    })
}

/// The bytes of a presence file, only when it is a regular file (not a link) of `uid`'s that no group or other can touch and
/// that is at most [`PRESENCE_MAX_BYTES`]. The handle is checked again after it is opened.
fn read_presence_file(path: &Path, uid: u32) -> Option<Vec<u8>> {
    let ok = |m: &std::fs::Metadata| m.file_type().is_file() && m.uid() == uid && m.mode() & 0o077 == 0 && m.len() <= PRESENCE_MAX_BYTES;
    if !ok(&std::fs::symlink_metadata(path).ok()?) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    if !ok(&file.metadata().ok()?) {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(PRESENCE_MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= PRESENCE_MAX_BYTES).then_some(bytes)
}

/// The user id the operator's own files have: the Delphi home's owner (the daemon makes it, and nobody else's files belong in it).
pub fn operator_uid(home: &Path) -> Option<u32> {
    std::fs::metadata(home).ok().map(|m| m.uid())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// The socket answered, and what answered is the daemon that wrote the file.
    Running,
    /// A file, and nothing behind it that answers as that daemon: a crash left it.
    Stopped,
    /// A narrator socket with no presence file: a daemon from before spec 130.
    Older,
}

/// What a running daemon says about itself through `observe.state` (spec 130 §3.3). Tokens, not money: `spent_usd` is never read.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Summary {
    pub running: u32,
    pub waiting: u32,
    pub escalated: u32,
    pub approvals: u32,
    /// The oldest waiting approval's `since_ms` (when it was raised), if any.
    pub oldest_approval_ms: Option<u64>,
    pub mission: Option<String>,
    pub session: Option<String>,
    pub tokens: u64,
}

/// The summary out of an `observe.state` reply.
pub fn summarize_state(s: &Value) -> Summary {
    let count = |state: &str| s["agents"].as_object().map(|a| a.values().filter(|r| r["state"].as_str() == Some(state)).count() as u32).unwrap_or(0);
    let approvals = s["approvals"].as_object();
    let tokens = ["input", "cache_read", "cache_write", "output", "thinking"].iter().map(|k| s["tokens"][*k].as_u64().unwrap_or(0)).sum();
    let short = |v: &Value| v.as_str().filter(|t| !t.is_empty()).map(|t| t.chars().filter(|c| !c.is_control()).take(80).collect::<String>());
    Summary {
        running: count("running"),
        waiting: count("waiting on you"),
        escalated: count("escalated"),
        approvals: approvals.map(|a| a.len() as u32).unwrap_or(0),
        oldest_approval_ms: approvals.and_then(|a| a.values().filter_map(|w| w["since_ms"].as_u64()).min()),
        mission: short(&s["mission_id"]),
        session: s["sessions"].as_array().and_then(|a| a.last()).and_then(|r| short(&r["id"])),
        tokens,
    }
}

/// One daemon the app found. Shown as text; nothing in it is run, opened or sent to a daemon but the hash an `attach` names.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Found {
    pub hash: String,
    pub state: State,
    /// Whether the narrator socket answered (always so for `running`; for `older`, the only thing known).
    pub answers: bool,
    pub workspace_name: String,
    pub workspace: String,
    pub home: String,
    pub version: String,
    pub pid: u64,
    pub started: String,
    pub narrator_socket: PathBuf,
    pub app_socket: PathBuf,
    pub summary: Option<Summary>,
    /// From the presence file (spec 120 §13.2 item 12): the release the daemon found, and whether a restart is due after an update.
    pub update_available: Option<String>,
    pub restart_pending: bool,
}

/// Whether the daemon that answered `hello` is the one that wrote `p`. A daemon names its instance in the reply; one that
/// does not is matched by the pid the reply always carries.
pub fn hello_matches(hello: &Value, p: &Presence) -> bool {
    match hello["instance"].as_str() {
        Some(i) => i == p.instance,
        None => hello["pid"].as_u64() == Some(p.pid),
    }
}

/// Connect to a narrator socket and say hello; the reply, or `None` when nothing answers in time.
fn probe(socket: &Path) -> Option<Value> {
    Client::attach_hello(socket, env!("CARGO_PKG_VERSION"), PROBE_TIMEOUT).ok().map(|(_, hello)| hello)
}

fn socket_pair(home: &Path, hash: &str) -> (PathBuf, PathBuf) {
    (home.join("run").join(format!("{hash}.narrator.sock")), home.join("run").join(format!("{hash}.app.sock")))
}

/// What is in the run folder, before any socket is asked: presence files that parse (by hash), and narrator sockets (by hash).
/// At most [`RUN_ENTRIES_MAX`] of each, read in name order.
pub fn scan(home: &Path, uid: u32) -> (BTreeMap<String, Presence>, Vec<String>) {
    let run = home.join("run");
    let mut names: Vec<String> = std::fs::read_dir(&run).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    names.sort();
    let (mut files, mut sockets) = (BTreeMap::new(), Vec::new());
    let mut seen = 0;
    for name in names {
        if let Some(hash) = name.strip_suffix(".json").filter(|h| is_hash(h)) {
            seen += 1;
            if seen <= RUN_ENTRIES_MAX {
                if let Some(p) = read_presence_file(&run.join(&name), uid).and_then(|b| parse_presence(&b, hash)) {
                    files.insert(hash.to_string(), p);
                }
            }
        } else if let Some(hash) = name.strip_suffix(".narrator.sock").filter(|h| is_hash(h)) {
            if sockets.len() < RUN_ENTRIES_MAX && std::fs::symlink_metadata(run.join(&name)).is_ok_and(|m| m.file_type().is_socket()) {
                sockets.push(hash.to_string());
            }
        }
    }
    (files, sockets)
}

/// Every daemon of `home`, without summaries (spec 130 §3.1), for the operator whose user id is `uid`. The order is only
/// stable (name, then hash); the order the window shows is its own (`daemons.ts`).
pub fn discover_as(home: &Path, uid: u32) -> Vec<Found> {
    let (files, sockets) = scan(home, uid);
    let mut wanted: Vec<(String, Option<Presence>)> = files.into_iter().map(|(h, p)| (h, Some(p))).collect();
    let bare: Vec<String> = sockets.into_iter().filter(|h| !wanted.iter().any(|(w, _)| w == h)).collect();
    wanted.extend(bare.into_iter().map(|h| (h, None)));
    let mut out: Vec<Found> = std::thread::scope(|s| {
        let jobs: Vec<_> = wanted
            .into_iter()
            .map(|(hash, p)| {
                s.spawn(move || {
                    let (narrator_socket, app_socket) = socket_pair(home, &hash);
                    let hello = probe(&narrator_socket);
                    let answers = hello.is_some();
                    match p {
                        Some(p) => {
                            let live = hello.as_ref().is_some_and(|h| hello_matches(h, &p));
                            Found { hash, state: if live { State::Running } else { State::Stopped }, answers: live, workspace_name: p.workspace_name, workspace: p.workspace, home: p.home, version: p.binary_version, pid: p.pid, started: p.started, narrator_socket, app_socket, summary: None, update_available: p.update_available, restart_pending: p.restart_pending }
                        }
                        None => {
                            let version = hello.as_ref().and_then(|h| h["binary_version"].as_str()).unwrap_or("").chars().filter(|c| !c.is_control()).take(64).collect();
                            let pid = hello.as_ref().and_then(|h| h["pid"].as_u64()).unwrap_or(0);
                            Found { hash, state: State::Older, answers, workspace_name: String::new(), workspace: String::new(), home: String::new(), version, pid, started: String::new(), narrator_socket, app_socket, summary: None, update_available: None, restart_pending: false }
                        }
                    }
                })
            })
            .collect();
        jobs.into_iter().filter_map(|j| j.join().ok()).collect()
    });
    out.sort_by(|a, b| (&a.workspace_name, &a.hash).cmp(&(&b.workspace_name, &b.hash)));
    out
}

/// Every daemon of `home` (spec 130 §3.1); empty when the home's owner cannot be told.
pub fn discover(home: &Path) -> Vec<Found> {
    operator_uid(home).map(|uid| discover_as(home, uid)).unwrap_or_default()
}

/// `observe.state` over a short-lived narrator connection: the one read a summary makes (spec 130 §3.3), within
/// [`SUMMARY_TIMEOUT`]; `None` when the daemon does not answer in that time or answers with anything else.
pub fn read_summary(narrator: &Path) -> Option<Summary> {
    let started = Instant::now();
    let (mut c, _) = Client::attach_hello(narrator, env!("CARGO_PKG_VERSION"), SUMMARY_TIMEOUT).ok()?;
    c.set_read_timeout(Some(SUMMARY_TIMEOUT.saturating_sub(started.elapsed()).max(Duration::from_millis(50))));
    let state = c.call(Method::ObserveState, json!({})).ok()?;
    state.is_object().then(|| summarize_state(&state))
}

/// What was last read for each daemon, and when: no daemon is asked again within [`SUMMARY_EVERY`].
pub type SummaryCache = BTreeMap<String, (Instant, Option<Summary>)>;

/// Fill in the summary of the first [`SUMMARIES_MAX`] running daemons, from `cache` when it is fresh, otherwise by a read.
pub fn add_summaries(found: &mut [Found], cache: &mut SummaryCache) {
    cache.retain(|h, _| found.iter().any(|f| &f.hash == h));
    let now = Instant::now();
    let running: Vec<usize> = found.iter().enumerate().filter(|(_, f)| f.state == State::Running).map(|(i, _)| i).take(SUMMARIES_MAX).collect();
    let mut stale: Vec<usize> = Vec::new();
    for &i in &running {
        match cache.get(&found[i].hash) {
            Some((at, s)) if now.duration_since(*at) < SUMMARY_EVERY => found[i].summary = s.clone(),
            _ => stale.push(i),
        }
    }
    let read: Vec<(usize, Option<Summary>)> = std::thread::scope(|s| {
        let jobs: Vec<_> = stale
            .iter()
            .map(|&i| {
                let sock = found[i].narrator_socket.clone();
                (i, s.spawn(move || read_summary(&sock)))
            })
            .collect();
        jobs.into_iter().map(|(i, j)| (i, j.join().ok().flatten())).collect()
    });
    for (i, summary) in read {
        cache.insert(found[i].hash.clone(), (now, summary.clone()));
        found[i].summary = summary;
    }
}

/// What the app does at start (spec 130 §3.2), given what it found and `DELPHI_WORKSPACE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// The workspace `DELPHI_WORKSPACE` names, or `default`, asked of `delphi daemon where`, as before this spec.
    Locate,
    /// Exactly one daemon answers: attach to it.
    Attach(String),
    /// Several answer and none was named: the window shows the start screen and nothing is attached.
    Ask,
}

pub fn start_choice(found: &[Found], named: Option<&str>) -> Start {
    if named.is_some_and(|n| !n.is_empty()) {
        return Start::Locate;
    }
    let live: Vec<&Found> = found.iter().filter(|f| f.answers).collect();
    match live.as_slice() {
        [one] => Start::Attach(one.hash.clone()),
        [] => Start::Locate,
        _ => Start::Ask,
    }
}

/// A workspace name as `delphi daemon where --workspace` may be given one from a file: plain, and never an option.
fn plain_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && !n.starts_with(['-', '.']) && n.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// What the bridge is told about a daemon it is pointed at: `delphi daemon where`'s answer for its workspace when that names
/// the same narrator socket (the roles' look is in it), otherwise the little discovery knows.
pub fn info_for(found: &Found) -> Value {
    let minimal = || json!({ "narrator_socket": found.narrator_socket, "app_socket": found.app_socket, "workspace": found.workspace, "workspace_name": found.workspace_name, "home": found.home, "roles": {} });
    if !plain_name(&found.workspace_name) {
        return minimal();
    }
    let explicit_home = env_path("DELPHI_HOME");
    for exe in candidates(env_path("HOME").as_deref()) {
        if exe.is_file() {
            if let Some((info, narrator)) = ask(&exe, explicit_home.as_deref(), &found.workspace_name) {
                if narrator == found.narrator_socket {
                    return info;
                }
            }
        }
    }
    minimal()
}

/// A document's place on disk: `rel` (a path the daemon gave, relative to the workspace) under `root`, only when it names a
/// file that really is inside `root` once links are followed. Anything absolute, with `..`, or leading outside is refused.
pub fn inside(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return None;
    }
    let root = std::fs::canonicalize(root).ok()?;
    let full = std::fs::canonicalize(root.join(rel)).ok()?;
    (full.starts_with(&root) && full.is_file()).then_some(full)
}

/// The kinds of file the app will hand to the system to open (spec 111 §2.6): a page or a deck, a document to read, a picture,
/// a recording. Never a program, a script or an archive.
pub const OPENABLE: [&str; 18] = ["html", "htm", "pdf", "png", "jpg", "jpeg", "gif", "webp", "svg", "mp4", "mov", "m4v", "webm", "mp3", "wav", "m4a", "md", "txt"];

pub fn openable(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).is_some_and(|e| OPENABLE.contains(&e.as_str()))
}

/// The folders a path the daemon or an agent named may be looked for in: the workspace, then each registered project's root
/// (`projects` in what `delphi daemon where` printed). Only absolute folders that exist.
pub fn roots(info: &Value) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = info["workspace"].as_str().map(PathBuf::from).into_iter().collect();
    out.extend(info["projects"].as_array().into_iter().flatten().filter_map(|p| p["root"].as_str().map(PathBuf::from)));
    out.retain(|p| p.is_absolute() && p.is_dir());
    out.dedup();
    out
}

/// The first of `roots` that holds `rel` as a file inside it.
pub fn find(roots: &[PathBuf], rel: &str) -> Option<PathBuf> {
    roots.iter().find_map(|r| inside(r, rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_file_is_looked_for_in_the_workspace_then_in_registered_projects_and_only_some_kinds_open() {
        let t = tempfile::tempdir().unwrap();
        let (ws, proj) = (t.path().join("ws"), t.path().join("proj"));
        std::fs::create_dir_all(ws.join("missions")).unwrap();
        std::fs::create_dir_all(proj.join("decks")).unwrap();
        std::fs::write(ws.join("missions/report.md"), "x").unwrap();
        std::fs::write(proj.join("decks/pitch.html"), "x").unwrap();
        std::fs::write(proj.join("run.sh"), "x").unwrap();
        let info = json!({"workspace": ws, "projects": [{"name": "default", "root": ws, "home": true}, {"name": "p", "root": proj, "home": false}, {"name": "gone", "root": t.path().join("nope")}, {"name": "rel", "root": "relative"}]});
        let roots = roots(&info);
        assert_eq!(roots.len(), 2, "{roots:?}");
        assert!(find(&roots, "missions/report.md").unwrap().starts_with(std::fs::canonicalize(&ws).unwrap()));
        assert!(find(&roots, "decks/pitch.html").unwrap().ends_with("decks/pitch.html"));
        assert!(find(&roots, "../proj/run.sh").is_none());
        assert!(find(&roots, "decks/none.html").is_none());
        for (name, yes) in [("a.html", true), ("a.HTM", true), ("a.pdf", true), ("a.PNG", true), ("a.mp4", true), ("a.m4a", true), ("a.md", true), ("a.sh", false), ("a.app", false), ("a.command", false), ("a.zip", false), ("a.py", false), ("a", false), ("a.html.sh", false)] {
            assert_eq!(openable(Path::new(name)), yes, "{name}");
        }
    }

    #[test]
    fn only_a_file_inside_the_workspace_can_be_shown() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("ws");
        std::fs::create_dir_all(ws.join("missions/m")).unwrap();
        std::fs::write(ws.join("missions/m/report.md"), "x").unwrap();
        std::fs::write(t.path().join("outside.md"), "x").unwrap();
        std::os::unix::fs::symlink(t.path().join("outside.md"), ws.join("link.md")).unwrap();
        assert!(inside(&ws, "missions/m/report.md").unwrap().ends_with("missions/m/report.md"));
        for bad in ["../outside.md", "missions/../../outside.md", "/etc/hosts", "", "missions/m", "missions/m/none.md", "link.md", "./missions/m/report.md"] {
            assert!(inside(&ws, bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_socket_is_named_as_the_daemon_names_it() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("workspaces/default");
        std::fs::create_dir_all(&ws).unwrap();
        let sock = narrator_socket(t.path(), &ws).unwrap();
        let name = sock.file_name().unwrap().to_str().unwrap();
        let hash = blake3::hash(std::fs::canonicalize(&ws).unwrap().as_os_str().as_encoded_bytes()).to_hex();
        assert_eq!(name, format!("{}.narrator.sock", &hash[..12]));
        assert_eq!(name.len(), 12 + ".narrator.sock".len());
        assert_eq!(sock.parent().unwrap(), t.path().join("run"));
        // A workspace that does not exist has no canonical path, so there is nothing to compute.
        assert!(narrator_socket(t.path(), &t.path().join("nope")).is_none());
        // The app socket is beside it, with the same hash (spec 120 §3 item 1).
        assert_eq!(app_socket(t.path(), &ws).unwrap(), t.path().join("run").join(format!("{}.app.sock", &hash[..12])));
        assert!(app_socket(t.path(), &t.path().join("nope")).is_none());
        // What `delphi daemon where` names is used only when it is an absolute path; an older `delphi` names none.
        assert_eq!(app_socket_in(&json!({"app_socket": "/h/run/abc.app.sock"})), Some(PathBuf::from("/h/run/abc.app.sock")));
        assert_eq!(app_socket_in(&json!({"app_socket": "run/abc.app.sock"})), None);
        assert_eq!(app_socket_in(&json!({"narrator_socket": "/h/run/abc.narrator.sock"})), None);
    }

    #[test]
    fn where_output_is_read_as_data_and_must_name_an_absolute_narrator_socket() {
        let (info, sock) = parse_where(br#"{"narrator_socket":"/h/run/abc.narrator.sock","roles":{"scout":{"name":"scout"}},"version":"0.6.1"}"#).unwrap();
        assert_eq!(sock, PathBuf::from("/h/run/abc.narrator.sock"));
        assert_eq!(info["version"], "0.6.1");
        assert!(parse_where(b"usage: delphi ...").is_none());
        assert!(parse_where(br#"{"console_socket":"/x"}"#).is_none());
        assert!(parse_where(br#"{"narrator_socket":"relative.sock"}"#).is_none());
        assert!(parse_where(br#"["/h/run/abc.narrator.sock"]"#).is_none());
    }

    #[test]
    fn delphi_is_looked_for_where_an_install_puts_it() {
        let c = candidates(Some(Path::new("/h")));
        assert!(c.contains(&PathBuf::from("/h/.local/bin/delphi")));
        assert!(c.contains(&PathBuf::from("/opt/homebrew/bin/delphi")));
        assert!(c.contains(&PathBuf::from("/usr/local/bin/delphi")));
    }

    #[test]
    fn the_bundled_program_is_found_from_the_app_executable() {
        let app = Path::new("/Applications/Delphi.app/Contents/MacOS/delphi-app");
        assert_eq!(bundled_path(app), Some(PathBuf::from("/Applications/Delphi.app/Contents/Resources/delphi")));
        assert_eq!(bundled_path(Path::new("/x/target/debug/delphi-app")), None, "not inside a bundle");
        // Installed places come first; a bundled copy, when there is one, is last.
        let installed = installed_candidates(Some(Path::new("/h")));
        let all = candidates(Some(Path::new("/h")));
        assert_eq!(all[..installed.len()], installed[..]);
    }

    #[test]
    fn program_state_names_the_bundled_copy_only_when_it_exists() {
        let t = tempfile::tempdir().unwrap();
        let bundled = t.path().join("Resources/delphi");
        assert_eq!(program_in(Some(t.path()), Some(bundled.clone())).bundled, None);
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"x").unwrap();
        assert_eq!(program_in(Some(t.path()), Some(bundled.clone())).bundled.as_deref(), bundled.to_str());
        std::fs::create_dir_all(t.path().join(".local/bin")).unwrap();
        std::fs::write(t.path().join(".local/bin/delphi"), b"x").unwrap();
        assert!(program_in(Some(t.path()), None).installed);
    }
}

/// A presence file as a daemon writes it (spec 130 §2), for tests here and in `bridge`.
#[cfg(test)]
pub(crate) fn write_presence(home: &Path, hash: &str, instance: &str, pid: u64, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let run = home.join("run");
    std::fs::create_dir_all(&run).unwrap();
    let body = json!({
        "version": 1, "instance": instance, "pid": pid, "binary_version": "0.6.5", "workspace": format!("/w/{name}"), "workspace_name": name,
        "home": home, "started": "2026-11-19T10:00:00Z", "narrator_socket": run.join(format!("{hash}.narrator.sock")), "app_socket": run.join(format!("{hash}.app.sock")),
    });
    let file = run.join(format!("{hash}.json"));
    std::fs::write(&file, body.to_string()).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    file
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const A: &str = "aaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbb";
    const C: &str = "cccccccccccc";
    const D: &str = "dddddddddddd";
    const E: &str = "eeeeeeeeeeee";
    const F: &str = "ffffffffffff";
    const INSTANCE: &str = "0123456789abcdef0123456789abcdef";

    /// A narrator socket that answers `auth.issue`, `hello` (with `instance`, when given) and `observe.state`; `reads` counts the latter.
    fn fake(path: &Path, instance: Option<&str>, pid: u64, state: Value, reads: Arc<AtomicUsize>) {
        let listener = UnixListener::bind(path).unwrap();
        let instance = instance.map(String::from);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (instance, state, reads) = (instance.clone(), state.clone(), reads.clone());
                std::thread::spawn(move || {
                    let mut out = stream.try_clone().unwrap();
                    for line in BufReader::new(stream).lines() {
                        let Ok(line) = line else { return };
                        let req: Value = serde_json::from_str(&line).unwrap();
                        let id = req["id"].clone();
                        let result = match req["method"].as_str().unwrap() {
                            "auth.issue" => json!({"token": "t", "scope": "narrator"}),
                            "hello" => {
                                let mut h = json!({"protocol_version": 1, "binary_version": "0.6.5", "scope": "narrator", "pid": pid});
                                if let Some(i) = &instance {
                                    h["instance"] = json!(i);
                                }
                                h
                            }
                            "observe.state" => {
                                reads.fetch_add(1, Ordering::SeqCst);
                                state.clone()
                            }
                            m => json!({"echo": m}),
                        };
                        if writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": id, "result": result})).is_err() {
                            return;
                        }
                    }
                });
            }
        });
    }

    fn sock(home: &Path, hash: &str) -> PathBuf {
        home.join("run").join(format!("{hash}.narrator.sock"))
    }

    fn busy() -> Value {
        json!({
            "agents": {"builder": {"state": "running"}, "scout": {"state": "running"}, "red-team": {"state": "waiting on you"}, "architect": {"state": "escalated"}, "writer": {"state": "idle"}},
            "approvals": {"ap_1": {"role": "red-team", "summary": "x", "since_ms": 2000}, "ap_2": {"role": "builder", "summary": "y", "since_ms": 1000}},
            "tokens": {"input": 10, "cache_read": 20, "cache_write": 30, "output": 40, "thinking": 50},
            "spent_usd": 12.5, "mission_id": "m-7", "sessions": [{"id": "s-1"}, {"id": "s-2"}]
        })
    }

    #[test]
    fn a_live_file_is_running_a_stale_one_stopped_and_junk_is_skipped() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let uid = operator_uid(home).unwrap();
        // A: live (its socket answers and names the file's instance). B: stale (a file, nothing behind it).
        write_presence(home, A, INSTANCE, 11, "alpha");
        fake(&sock(home, A), Some(INSTANCE), 11, busy(), Arc::default());
        write_presence(home, B, "fedcba9876543210fedcba9876543210", 12, "beta");
        // C: not JSON. D: a symbolic link to a good file. E: larger than 4 KiB. F: others can read it.
        let run = home.join("run");
        std::fs::write(run.join(format!("{C}.json")), "not json at all").unwrap();
        std::fs::set_permissions(run.join(format!("{C}.json")), std::fs::Permissions::from_mode(0o600)).unwrap();
        let real = write_presence(home, "111111111111", INSTANCE, 13, "gamma");
        std::fs::remove_file(sock(home, "111111111111")).ok();
        std::os::unix::fs::symlink(&real, run.join(format!("{D}.json"))).unwrap();
        let big = write_presence(home, E, INSTANCE, 14, "delta");
        let mut text = std::fs::read_to_string(&big).unwrap();
        text.push_str(&" ".repeat(5000));
        std::fs::write(&big, text).unwrap();
        let open = write_presence(home, F, INSTANCE, 15, "open");
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o644)).unwrap();

        let found = discover_as(home, uid);
        let seen: Vec<(&str, State)> = found.iter().map(|f| (f.hash.as_str(), f.state)).collect();
        // `111111111111` is the one real file the symbolic link pointed at: it is a good file and a stale one.
        assert_eq!(seen, vec![(A, State::Running), (B, State::Stopped), ("111111111111", State::Stopped)], "{seen:?}");
        let live = &found[0];
        assert!(live.answers && live.workspace_name == "alpha" && live.version == "0.6.5" && live.pid == 11 && live.workspace == "/w/alpha");
        assert_eq!(live.narrator_socket, sock(home, A));
        assert!(!found[1].answers);

        // A file that is somebody else's is not read.
        let strangers = discover_as(home, uid.wrapping_add(1));
        assert!(strangers.iter().all(|f| f.state == State::Older), "no file was read: {strangers:?}");
    }

    #[test]
    fn a_daemon_that_is_not_the_one_that_wrote_the_file_is_stopped() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        write_presence(home, A, INSTANCE, 11, "alpha");
        // Another daemon holds the socket now: another instance; and one that names no instance, with another pid.
        fake(&sock(home, A), Some("ffffffffffffffffffffffffffffffff"), 11, busy(), Arc::default());
        write_presence(home, B, INSTANCE, 12, "beta");
        fake(&sock(home, B), None, 99, busy(), Arc::default());
        write_presence(home, C, INSTANCE, 13, "gamma");
        fake(&sock(home, C), None, 13, busy(), Arc::default());
        let found = discover_as(home, operator_uid(home).unwrap());
        let seen: Vec<(&str, State)> = found.iter().map(|f| (f.hash.as_str(), f.state)).collect();
        assert_eq!(seen, vec![(A, State::Stopped), (B, State::Stopped), (C, State::Running)]);
        assert!(!found[0].answers);
    }

    #[test]
    fn a_narrator_socket_with_no_file_is_an_older_daemon_and_a_wrong_version_is_skipped() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        std::fs::create_dir_all(home.join("run")).unwrap();
        fake(&sock(home, A), None, 5, json!({}), Arc::default());
        // A socket nobody listens on any more.
        drop(UnixListener::bind(sock(home, B)).unwrap());
        // A file of version 2 beside nothing, and a plain file that is no socket.
        let v2 = write_presence(home, C, INSTANCE, 1, "v2");
        std::fs::write(&v2, std::fs::read_to_string(&v2).unwrap().replace("\"version\":1", "\"version\":2")).unwrap();
        std::fs::write(sock(home, D), "x").unwrap();
        let found = discover_as(home, operator_uid(home).unwrap());
        let seen: Vec<(&str, State, bool)> = found.iter().map(|f| (f.hash.as_str(), f.state, f.answers)).collect();
        assert_eq!(seen, vec![(A, State::Older, true), (B, State::Older, false)], "{seen:?}");
        assert_eq!(found[0].version, "0.6.5");
        assert_eq!(found[0].workspace_name, "");
    }

    #[test]
    fn a_presence_file_is_parsed_as_data_and_must_name_its_own_socket() {
        let good = json!({"version": 1, "instance": INSTANCE, "pid": 3, "binary_version": "0.6.5", "workspace": "/w", "workspace_name": "default", "home": "/h", "started": "t", "narrator_socket": format!("/h/run/{A}.narrator.sock"), "app_socket": "/h/run/x.app.sock"});
        let p = parse_presence(good.to_string().as_bytes(), A).unwrap();
        assert_eq!((p.pid, p.workspace_name.as_str(), p.binary_version.as_str()), (3, "default", "0.6.5"));
        let with = |k: &str, v: Value| {
            let mut g = good.clone();
            g[k] = v;
            g.to_string()
        };
        assert!(parse_presence(with("version", json!(2)).as_bytes(), A).is_none());
        assert!(parse_presence(with("narrator_socket", json!(format!("/h/run/{B}.narrator.sock"))).as_bytes(), A).is_none(), "another daemon's socket");
        assert!(parse_presence(with("narrator_socket", json!("/etc/passwd")).as_bytes(), A).is_none());
        assert!(parse_presence(with("instance", json!("not hex")).as_bytes(), A).is_none());
        assert!(parse_presence(with("workspace_name", json!("a\u{1b}[31mb")).as_bytes(), A).is_none(), "control characters");
        assert!(parse_presence(with("pid", json!("3")).as_bytes(), A).is_none());
        assert!(parse_presence(good.to_string().as_bytes(), "ABC").is_none());
        // Spec 120 §13.2 item 12: the two update fields are optional, and carried only in the shape the daemon writes them.
        assert_eq!((p.update_available.clone(), p.restart_pending), (None, false), "a file from before them says nothing");
        let mut updated = good.clone();
        updated["update_available"] = json!("0.6.6");
        updated["restart_pending"] = json!(true);
        let q = parse_presence(updated.to_string().as_bytes(), A).unwrap();
        assert_eq!((q.update_available.as_deref(), q.restart_pending), (Some("0.6.6"), true));
        for bad in [json!("v0.6.6"), json!("0.6.6-rc1"), json!("0.6"), json!("<b>0.6.6</b>"), json!(7), json!("0.6.6\n"), json!("")] {
            assert_eq!(parse_presence(with("update_available", bad.clone()).as_bytes(), A).unwrap().update_available, None, "{bad}");
        }
        assert!(!parse_presence(with("restart_pending", json!("yes")).as_bytes(), A).unwrap().restart_pending);
        assert!(parse_presence(b"[1,2]", A).is_none() && parse_presence(b"", A).is_none() && parse_presence(b"{", A).is_none());
        assert!(is_hash(A) && !is_hash("AAAAAAAAAAAA") && !is_hash("aaaaaaaaaaa") && !is_hash("../../../etc/"));
    }

    #[test]
    fn the_summary_is_counts_and_tokens_and_never_money() {
        let s = summarize_state(&busy());
        assert_eq!((s.running, s.waiting, s.escalated, s.approvals), (2, 1, 1, 2));
        assert_eq!((s.oldest_approval_ms, s.tokens), (Some(1000), 150));
        assert_eq!((s.mission.as_deref(), s.session.as_deref()), (Some("m-7"), Some("s-2")));
        assert!(!serde_json::to_string(&s).unwrap().contains("usd") && !serde_json::to_string(&s).unwrap().contains("12.5"));
        assert_eq!(summarize_state(&json!({})), Summary::default());
    }

    #[test]
    fn a_summary_is_one_observe_state_read_and_not_repeated_within_five_seconds() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        write_presence(home, A, INSTANCE, 11, "alpha");
        let reads = Arc::new(AtomicUsize::new(0));
        fake(&sock(home, A), Some(INSTANCE), 11, busy(), reads.clone());
        write_presence(home, B, INSTANCE, 12, "stale");
        let mut cache = SummaryCache::new();
        let mut found = discover_as(home, operator_uid(home).unwrap());
        add_summaries(&mut found, &mut cache);
        assert_eq!(found[0].summary.as_ref().map(|s| s.running), Some(2));
        assert!(found[1].summary.is_none(), "a stopped daemon is not asked");
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        let mut again = discover_as(home, operator_uid(home).unwrap());
        add_summaries(&mut again, &mut cache);
        assert_eq!(again[0].summary, found[0].summary);
        assert_eq!(reads.load(Ordering::SeqCst), 1, "within five seconds the last read is shown");
    }

    #[test]
    fn at_start_one_live_daemon_is_attached_several_ask_and_a_named_workspace_is_looked_up_as_before() {
        let f = |hash: &str, answers: bool| Found {
            hash: hash.into(),
            state: if answers { State::Running } else { State::Stopped },
            answers,
            workspace_name: String::new(),
            workspace: String::new(),
            home: String::new(),
            version: String::new(),
            pid: 0,
            started: String::new(),
            narrator_socket: PathBuf::new(),
            app_socket: PathBuf::new(),
            summary: None,
            update_available: None,
            restart_pending: false,
        };
        assert_eq!(start_choice(&[], None), Start::Locate);
        assert_eq!(start_choice(&[f(A, false)], None), Start::Locate);
        assert_eq!(start_choice(&[f(A, true), f(B, false)], None), Start::Attach(A.into()));
        assert_eq!(start_choice(&[f(A, true), f(B, true)], None), Start::Ask);
        assert_eq!(start_choice(&[f(A, true), f(B, true)], Some("beta")), Start::Locate);
        assert_eq!(start_choice(&[f(A, true), f(B, true)], Some("")), Start::Ask);
        assert!(plain_name("default") && plain_name("my.ws_1-b") && !plain_name("-rf") && !plain_name("..") && !plain_name("a b") && !plain_name("a/b") && !plain_name(""));
    }
}
