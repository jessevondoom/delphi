//! The `update` grant (`specs/120` §13.2): one method, `app.update_apply`, which takes no parameter, and the daemon's own knowledge of
//! whether a newer release exists.
//!
//! * **Availability** (item 12). At start and every six hours the daemon starts `<delphi> update --check --home <home>` (clause 2, read
//!   only), parses the one line that command prints for an update, and keeps the version as `update_available`. It, and
//!   `restart_pending`, go into the presence file as optional fields.
//! * **A request** (items 2, 5, 7). A key that holds `update` asks. The daemon, not the app, says what the release is. Without the key's
//!   standing `--auto` the request is **pending**, one at a time, for ten minutes, and nothing changes until the operator, at a
//!   terminal, types the short code the app's window shows (`app.update_approve`): the daemon gives that code to the app connection that
//!   asked and lists it nowhere else. A version no newer than the one installed, a second apply within the hour for the key, an apply
//!   less than ten minutes after a failure or an hour after an install, an apply while one runs: each is refused, by name.
//! * **The apply** (items 3, 4, 8). The daemon starts the program it is running as with `update --no-restart --review --version
//!   v<approved> --workspace <name> --home <home>` (clause 1): detached from any terminal, standard input closed, nothing inherited above
//!   descriptor 2, a private temporary directory, an environment of `HOME`, `PATH`, `DELPHI_HOME` and `TMPDIR` only, output kept to a
//!   bounded buffer, and its whole process group ended when it is done or out of time. Nothing in the command comes from the app. An install
//!   of any release but the approved one is a failure, written loudly. The daemon does not restart itself; it records `restart_pending`.
//!
//! This is the only place in the engine that starts that program, and it is listed in `crates/delphi/tests/spawn_sites.toml`.
//! **No lock is held against a `delphi update` run at a terminal**: `delphi update` takes none (it replaces the file by renaming), so
//! the only guard here is this daemon's own flag and the one-an-hour limit.

use super::{err, EngineService};
use delphi_core::app_keys::{self, KeyStore};
use delphi_core::event::EventEnvelope;
use delphi_core::proto::code;
use delphi_core::server::RpcOutcome;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

/// How long a pending request waits for the operator (item 5).
pub const PENDING_SECS: i64 = 600;
/// One apply an hour for a key (item 7).
pub const APPLY_EVERY_SECS: i64 = 3600;
/// How often the daemon looks for a newer release (item 12).
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Wrong versions typed at one pending request before it is cancelled.
pub const MAX_WRONG: u8 = 3;
/// The least time between one apply ending and the next starting, for any key: after an install, and after a failure (item 7).
pub const AFTER_OK_SECS: i64 = 3600;
pub const AFTER_FAIL_SECS: i64 = 600;
/// How long a child's output is read after the child has gone, before whatever it left behind is ended (a process it started may hold the pipes).
const READERS_GRACE: Duration = Duration::from_secs(2);
/// What `delphi update --only-newer` exits with when the release is not newer than the program installed.
const EXIT_NOT_NEWER: i32 = 3;
/// What is kept of a child's output, per stream. The rest is read and thrown away so the child never blocks on a full pipe.
pub const OUT_MAX: usize = 64 * 1024;
/// How long an apply, and a check, may take before the child is killed.
const APPLY_MAX: Duration = Duration::from_secs(15 * 60);
const CHECK_MAX: Duration = Duration::from_secs(60);

/// The two ways the daemon starts its own program for this grant. Each is a clause of the spawn-site list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clause<'a> {
    /// Clause 1: install exactly this release (the one the daemon found and the operator approved), leaving running daemons on the old code.
    Apply { version: &'a str },
    /// Clause 2: ask whether there is a newer one; changes nothing.
    Check,
}

/// A workspace's name as it may stand in a command: plain, and never an option.
pub fn plain_name(n: &str) -> bool {
    let mut b = n.bytes();
    n.len() <= 64 && b.next().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// The arguments of clause 1. Made from the daemon's own workspace name and home and the release the daemon found and the operator approved,
/// never from anything an app sent. `--version` makes the install exactly that release, whatever the release list says later; `--only-newer`
/// makes the program refuse (exit 3) when that release is not newer than the program installed now, so a program updated by hand since the
/// daemon looked is never put back; `--review` leaves the release's changes to the built-in agents and skills pending for `/ratify`
/// instead of ratifying them unseen.
pub fn apply_argv(workspace: &str, home: &Path, version: &str) -> Vec<OsString> {
    vec!["update".into(), "--no-restart".into(), "--review".into(), "--only-newer".into(), "--version".into(), format!("v{version}").into(), "--workspace".into(), workspace.into(), "--home".into(), home.as_os_str().to_owned()]
}

/// The arguments of clause 2.
pub fn check_argv(home: &Path) -> Vec<OsString> {
    vec!["update".into(), "--check".into(), "--home".into(), home.as_os_str().to_owned()]
}

/// `1.2.3` or `v1.2.3`, three numbers and nothing more: a release, never a pre-release and never text.
pub fn parse_release(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let mut it = v.split('.');
    let (a, b, c) = (it.next()?, it.next()?, it.next()?);
    let num = |s: &str| (!s.is_empty() && s.len() <= 4 && s.bytes().all(|d| d.is_ascii_digit())).then(|| s.parse::<u64>().ok()).flatten();
    it.next().is_none().then(|| Some((num(a)?, num(b)?, num(c)?)))?
}

/// Is `candidate` a release newer than `current`? A version that is not a plain release is not newer.
pub fn newer(candidate: &str, current: &str) -> bool {
    match (parse_release(candidate), parse_release(current)) {
        (Some(c), Some(i)) => c > i,
        _ => false,
    }
}

/// What `delphi update --check` printed, as the version it found: the line `<current> -> <tag>` that comes with `An update is
/// available.`, and only when the tag is a release newer than `installed`. Anything else, including a program that printed more, is
/// no release.
pub fn parse_check(stdout: &str, installed: &str) -> Option<String> {
    if !stdout.lines().any(|l| l.starts_with("An update is available.")) {
        return None;
    }
    let tag = stdout.lines().find_map(|l| l.split_once(" -> ").filter(|(from, _)| parse_release(from).is_some()).map(|(_, to)| to.trim()))?;
    let (a, b, c) = parse_release(tag)?;
    let v = format!("{a}.{b}.{c}");
    newer(&v, installed).then_some(v)
}

/// The tag `delphi update` says it installed (`Installed v0.6.6 at …`), as a release number.
pub fn parse_installed(stdout: &str) -> Option<String> {
    let tag = stdout.lines().find_map(|l| l.strip_prefix("Installed "))?.split_whitespace().next()?;
    let (a, b, c) = parse_release(tag)?;
    Some(format!("{a}.{b}.{c}"))
}

/// A child's words as an event may carry them: one line, no control character, at most 200 characters.
fn one_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().map(|c| if c.is_control() { ' ' } else { c }).take(200).collect::<String>().trim().to_string()
}

/// What a child that ran said and did.
#[derive(Debug)]
pub struct Ran {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

struct Pending {
    key: String,
    version: String,
    request: String,
    /// The code the operator types at the terminal, as shown (`K7Q-4M2`). Made by the daemon, given to the app connection that asked and
    /// to nobody else: [`EngineService::app_update_admin`]'s list never holds it.
    code: String,
    /// When the request was made, on the monotonic clock ([`mono`]) and on the wall clock. It is stale when either says so: the monotonic
    /// clock can pause while a machine sleeps, and the wall clock can be set back.
    issued: i64,
    issued_wall: i64,
    wrong: u8,
}

fn stale(p: &Pending, at: i64, wall: i64) -> bool {
    at - p.issued > PENDING_SECS || wall - p.issued_wall > PENDING_SECS
}

/// Seconds on a clock that only moves forward: the wall clock can be set back, which would stretch a request's ten minutes or an hour's limit.
fn mono() -> i64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs() as i64
}

/// What the daemon knows about updates. In memory: a daemon that restarts forgets the hour's count, as the other limits do.
#[derive(Default)]
pub struct State {
    available: Option<String>,
    pending: Option<Pending>,
    running: bool,
    /// When each key's last apply started ([`mono`]).
    last_apply: HashMap<String, i64>,
    /// When the last apply ended, and whether it installed: the next apply of any key waits [`AFTER_OK_SECS`] or [`AFTER_FAIL_SECS`].
    last_end: Option<(i64, bool)>,
    restart_pending: bool,
    /// Counts each start and end of an apply: a check that began before one and ended after it is out of date and is thrown away.
    epoch: u64,
    made: u64,
    /// The process group of the child that runs now (its pid), so a shutdown can end it, and a process that was running when an earlier
    /// daemon stopped (its pid, from the marker file), which keeps this daemon from starting a second apply on top of it.
    child: Option<u32>,
    orphan: Option<i32>,
    /// Tests: seconds added to the monotonic clock, a shorter limit on a child, and a panic in the apply task.
    skew: i64,
    limit: Option<Duration>,
    #[cfg(test)]
    panic_in_apply: bool,
    /// The program to start instead of this one (tests: a script that records what it was asked and answers as it is told).
    exe: Option<PathBuf>,
    /// Where the presence file is, and what it said when the daemon wrote it: the update fields are added to it and it is rewritten.
    presence: Option<(delphi_core::spawn::Layout, delphi_core::presence::Presence)>,
    /// One writer of the presence file at a time, each reading the facts after it holds this.
    publish_gate: Arc<std::sync::Mutex<()>>,
}

impl State {
    fn now(&self) -> i64 {
        mono() + self.skew
    }
}

/// A private folder for one child: a new directory, mode 0700, that nothing else knows the name of.
fn private_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let mut rnd = [0u8; 8];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut rnd)?;
    let dir = std::env::temp_dir().join(format!("delphi-update-{}", app_keys::hex(&rnd)));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}

/// Read a stream into `buf` as it comes: the first [`OUT_MAX`] bytes kept, the rest read and dropped.
async fn read_into<R: tokio::io::AsyncRead + Unpin>(mut r: R, buf: Arc<std::sync::Mutex<Vec<u8>>>) {
    use tokio::io::AsyncReadExt;
    let mut chunk = [0u8; 4096];
    loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let mut b = buf.lock().unwrap_or_else(|e| e.into_inner());
                let room = OUT_MAX.saturating_sub(b.len());
                b.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    }
}

/// Is a process with this id alive (this user's, or another's that we may not signal)?
fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only asks whether the process exists.
    pid > 0 && (unsafe { libc::kill(pid, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

/// Given back when the apply task ends, however it ends: a panic in the task must not leave the daemon thinking an apply still runs.
struct RunGuard {
    svc: Arc<EngineService>,
    armed: bool,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let marker = {
            let mut st = self.svc.update.lock().unwrap_or_else(|e| e.into_inner());
            let at = st.now();
            st.running = false;
            st.epoch += 1;
            st.last_end = Some((at, false));
            st.child = None;
            st.presence.as_ref().map(|(l, _)| l.presence.with_extension("update.lock"))
        };
        if let Some(m) = marker {
            let _ = std::fs::remove_file(m);
        }
    }
}

/// Is `exe` something the daemon may start as the installed program? It is made canonical once, and that path is the one returned and the
/// one started. It must be a regular file named `delphi`, owned by this user, that no group or other can write, and every directory above it
/// must be owned by this user or by root and closed to group and other writers (a sticky directory owned by root, such as `/tmp`, is the
/// one exception), so nobody else can swap the file or a folder on the way to it. (The daemon runs as `<delphi> __core`, so this is the
/// file it was started from.)
pub fn is_installed_program(exe: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt;
    if !exe.is_absolute() {
        return Err("this daemon is not running as an installed `delphi`".into());
    }
    let canon = std::fs::canonicalize(exe).map_err(|e| format!("the installed program cannot be found: {e}"))?;
    if canon.file_name().and_then(|n| n.to_str()) != Some("delphi") {
        return Err("this daemon is not running as an installed `delphi`".into());
    }
    let m = std::fs::metadata(&canon).map_err(|e| format!("the installed program cannot be read: {e}"))?;
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    if !m.is_file() || m.uid() != me || m.mode() & 0o022 != 0 {
        return Err("the installed program is not a file only this user can change".into());
    }
    for dir in canon.ancestors().skip(1) {
        let d = std::fs::metadata(dir).map_err(|e| format!("a folder above the installed program cannot be read: {e}"))?;
        let sticky_root = d.uid() == 0 && d.mode() & 0o1000 != 0;
        if d.uid() != me && d.uid() != 0 {
            return Err(format!("{} is owned by another user, so the installed program could be swapped", dir.display()));
        }
        if d.mode() & 0o022 != 0 && !sticky_root {
            return Err(format!("{} can be written by others, so the installed program could be swapped", dir.display()));
        }
    }
    Ok(canon)
}

/// Kill every process of the child's group (the child made its own session, so its group is its pid) and wait until none is left.
async fn end_group(pid: u32) {
    let group = -(pid as i32);
    // SAFETY: kill has no preconditions; a group that is gone answers ESRCH.
    unsafe { libc::kill(group, libc::SIGKILL) };
    for _ in 0..100 {
        // SAFETY: as above; signal 0 only asks whether the group exists.
        if unsafe { libc::kill(group, 0) } != 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A fresh pairing-style code (`K7Q-4M2`) from the system's random source.
fn fresh_code() -> Result<String, String> {
    let mut rnd = [0u8; 64];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom").map_err(|e| e.to_string())?, &mut rnd).map_err(|e| e.to_string())?;
    app_keys::code_from(&rnd).ok_or_else(|| "no randomness for a code".to_string())
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// What one look at the state decided for a request, made and recorded under one lock.
enum Verdict {
    Refuse(&'static str, String),
    /// The same key asking again while its request waits: the same request and code.
    Waiting { request: String, code: String, left: i64, version: String },
    /// A new pending request, already recorded.
    Pending { code: String, version: String },
    /// A standing authorization: go on.
    Go { version: String },
}

impl EngineService {
    /// Start `Clause`'s command: the program this daemon is running as, with the clause's own arguments. The one process start of this
    /// grant (`crates/delphi/tests/spawn_sites.toml`, class `self`).
    pub async fn update_run(&self, clause: Clause<'_>) -> Result<Ran, String> {
        let exe = self.update_exe()?;
        let home = self.engine.ws.home.clone();
        let limit_override = self.update.lock().unwrap().limit;
        let (argv, limit) = match clause {
            Clause::Apply { version } => {
                let name = self.engine.ws.dir.file_name().and_then(|n| n.to_str()).filter(|n| plain_name(n)).ok_or("this workspace's name is not a plain one")?.to_string();
                if parse_release(version).is_none() {
                    return Err("the release to install is not a release number".into());
                }
                (apply_argv(&name, &home, version), APPLY_MAX)
            }
            Clause::Check => (check_argv(&home), CHECK_MAX),
        };
        let limit = limit_override.unwrap_or(limit);
        let tmp = private_dir().map_err(|e| format!("no private folder for the update: {e}"))?;
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.args(&argv).env_clear().env("PATH", "/usr/bin:/bin").env("DELPHI_HOME", &home).env("TMPDIR", &tmp).current_dir(&tmp).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        if let Some(h) = std::env::var_os("HOME") {
            cmd.env("HOME", h);
        }
        // SAFETY: setsid and fcntl run between fork and exec and touch nothing of this process. The child gets no controlling terminal and
        // every descriptor above 2 that this daemon holds (the readiness pipe, the lock, sockets) is closed for it when it starts the program.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                let max = libc::sysconf(libc::_SC_OPEN_MAX).clamp(256, 8192) as i32;
                for fd in 3..max {
                    libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                }
                Ok(())
            });
        }
        let out = match cmd.spawn() {
            Err(e) => Err(format!("the program could not be started: {e}")),
            Ok(mut child) => {
                let pid = child.id().unwrap_or(0);
                if pid != 0 {
                    self.update_child_started(pid);
                }
                let (out_buf, err_buf) = (Arc::new(std::sync::Mutex::new(Vec::new())), Arc::new(std::sync::Mutex::new(Vec::new())));
                let mut readers: Vec<tokio::task::JoinHandle<()>> = Vec::new();
                if let Some(o) = child.stdout.take() {
                    readers.push(tokio::spawn(read_into(o, out_buf.clone())));
                }
                if let Some(e) = child.stderr.take() {
                    readers.push(tokio::spawn(read_into(e, err_buf.clone())));
                }
                let waited = tokio::time::timeout(limit, child.wait()).await;
                if waited.is_ok() {
                    // The program is done. What it printed is read for a moment: a process it started and left behind may hold the pipes
                    // open for ever, and that must not make a finished install look like a failure.
                    let _ = tokio::time::timeout(READERS_GRACE, async {
                        for h in readers.iter_mut() {
                            if !h.is_finished() {
                                let _ = h.await;
                            }
                        }
                    })
                    .await;
                }
                // Whatever happened, nothing the child started stays behind, and this does not return until the group is gone: the next
                // apply is not allowed while the last one's processes might still run.
                if pid != 0 {
                    end_group(pid).await;
                }
                for h in readers.iter_mut() {
                    if !h.is_finished() {
                        let _ = tokio::time::timeout(Duration::from_secs(1), &mut *h).await;
                    }
                    h.abort();
                }
                self.update_child_ended();
                let text = |b: &Arc<std::sync::Mutex<Vec<u8>>>| String::from_utf8_lossy(&b.lock().unwrap_or_else(|e| e.into_inner())).into_owned();
                match waited {
                    Ok(Ok(status)) => Ok(Ran { code: status.code(), stdout: text(&out_buf), stderr: text(&err_buf) }),
                    Ok(Err(e)) => Err(format!("the program could not be waited for: {e}")),
                    Err(_) => {
                        let _ = child.wait().await;
                        Err("the program did not finish in time and was stopped".into())
                    }
                }
            }
        };
        let _ = std::fs::remove_dir_all(&tmp);
        out
    }

    /// The marker of a running apply, beside the presence file: the child's process id and when it started, readable by this user only.
    fn update_marker(&self) -> Option<PathBuf> {
        self.update.lock().unwrap_or_else(|e| e.into_inner()).presence.as_ref().map(|(l, _)| l.presence.with_extension("update.lock"))
    }

    fn update_child_started(&self, pid: u32) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        self.update.lock().unwrap_or_else(|e| e.into_inner()).child = Some(pid);
        if let Some(m) = self.update_marker() {
            let _ = std::fs::remove_file(&m);
            if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&m) {
                let _ = writeln!(f, "{} {}", pid, (self.engine.clock_unix)());
            }
        }
    }

    fn update_child_ended(&self) {
        self.update.lock().unwrap_or_else(|e| e.into_inner()).child = None;
        if let Some(m) = self.update_marker() {
            let _ = std::fs::remove_file(m);
        }
    }

    /// The daemon is stopping: end the child that is running, with everything it started, so no orphan goes on installing after this
    /// daemon is gone (a new daemon would not know of it). Called by the daemon's boot after the server has stopped.
    pub async fn update_shutdown(&self) {
        let pid = self.update.lock().unwrap_or_else(|e| e.into_inner()).child;
        if let Some(pid) = pid {
            end_group(pid).await;
        }
    }

    /// The program to start: the one this daemon runs as, checked to be the installed `delphi` (tests give their own). The path returned is
    /// the canonical one that was checked, and the one that is started.
    fn update_exe(&self) -> Result<PathBuf, String> {
        let exe = match self.update.lock().unwrap().exe.clone() {
            Some(e) => e,
            None => std::env::current_exe().map_err(|e| format!("this daemon's program cannot be found: {e}"))?,
        };
        is_installed_program(&exe)
    }

    /// Tests: start this program instead of the current executable.
    pub fn set_update_exe(&self, exe: PathBuf) {
        self.update.lock().unwrap().exe = Some(exe);
    }

    /// Tests: move the monotonic clock the limits use forward, and shorten the time a child may run.
    pub fn update_advance(&self, secs: i64) {
        self.update.lock().unwrap().skew += secs;
    }

    pub fn set_update_limit(&self, limit: Duration) {
        self.update.lock().unwrap().limit = Some(limit);
    }

    /// Tests: whether an apply is running now.
    pub fn update_running(&self) -> bool {
        self.update.lock().unwrap_or_else(|e| e.into_inner()).running
    }

    /// Where the presence file is and what it held, so that a change to `update_available` or `restart_pending` rewrites it.
    ///
    /// A marker beside the presence file names the child of an apply that an earlier daemon was running when it stopped. If that process is
    /// still alive, this daemon starts no apply while it is, and for the window after any failure once it has ended; a marker whose process
    /// is gone is removed.
    pub fn set_update_presence(&self, layout: delphi_core::spawn::Layout, base: delphi_core::presence::Presence) {
        let marker = layout.presence.with_extension("update.lock");
        let pid = std::fs::read_to_string(&marker).ok().and_then(|t| t.split_whitespace().next().and_then(|p| p.parse::<i32>().ok()));
        let mut st = self.update.lock().unwrap();
        st.presence = Some((layout, base));
        match pid {
            Some(p) if alive(p) => {
                st.orphan = Some(p);
                st.last_end = Some((st.now(), false));
            }
            _ => {
                let _ = std::fs::remove_file(&marker);
            }
        }
    }

    /// The daemon's own version: what an update must be newer than.
    fn installed_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// What the daemon holds now, for the presence file and for tests: (`update_available`, `restart_pending`).
    pub fn update_facts(&self) -> (Option<String>, bool) {
        let st = self.update.lock().unwrap();
        (st.available.clone(), st.restart_pending)
    }

    /// Rewrite the presence file with the two facts, when the daemon has one to rewrite. One writer at a time, and each reads the facts
    /// only once it holds the gate, so a writer that waited writes what is true now and never an older snapshot over a newer one.
    fn update_publish(&self) {
        let gate = self.update.lock().unwrap().publish_gate.clone();
        let _one_at_a_time = gate.lock().unwrap();
        let (facts, layout, mut p) = {
            let st = self.update.lock().unwrap();
            let Some((layout, base)) = st.presence.clone() else { return };
            ((st.available.clone(), st.restart_pending), layout, base)
        };
        (p.update_available, p.restart_pending) = facts;
        if let Err(e) = delphi_core::presence::write(&layout, &p) {
            delphi_core::elog::warn("daemon", "presence_update", "the presence file could not be rewritten", json!({"error": e.to_string()}));
        }
    }

    /// Look for a newer release now (clause 2) and keep what was found. A check that fails, or prints something that is not an update,
    /// leaves the daemon knowing no release. A check that began before an apply started or ended, or that finds one running, is out of
    /// date when it is done and changes nothing.
    pub async fn update_check_now(&self) {
        let epoch = {
            let st = self.update.lock().unwrap();
            if st.running {
                return;
            }
            st.epoch
        };
        let found = match self.update_run(Clause::Check).await {
            Ok(r) if r.code == Some(0) => parse_check(&r.stdout, Self::installed_version()),
            _ => None,
        };
        let changed = {
            let mut st = self.update.lock().unwrap();
            if st.running || st.epoch != epoch {
                return;
            }
            let changed = st.available != found;
            st.available = found;
            changed
        };
        if changed {
            self.update_publish();
        }
    }

    /// The daemon's own look for a release: at start, then every six hours. Called once, by the daemon's boot after it is ready, and by no test.
    pub fn start_update_checks(self: &Arc<Self>) {
        let svc = self.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(CHECK_EVERY);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                every.tick().await;
                svc.update_check_now().await;
            }
        });
    }

    /// Write an event of the default mission. The payload is the key's name, a version, a fixed reason: never anything the app sent.
    async fn update_event(&self, kind: &str, payload: Value) {
        if let Ok(m) = self.mission_at(None, None).await {
            let _ = m.append(EventEnvelope::new(&m.id, kind, payload)).await;
        }
    }

    /// The `app.request` event of an update request, in the states of [`crate::app_requests::STATES`]. Never the code.
    async fn update_request_event(&self, key: &str, request: &str, state: &str, because: Option<&str>) {
        let mut p = json!({"name": key, "request": request, "method": "app.update_apply", "ids": {}, "state": state});
        if let Some(b) = because {
            p["why"] = json!(one_line(b));
        }
        self.update_event(crate::app_audit::REQUEST_EVENT, p).await;
    }

    /// A request that was refused: both records, and the sentence the app is told.
    async fn update_refuse(&self, key: &str, request: &str, reason: &str, text: String) -> RpcOutcome {
        self.update_request_event(key, request, "refused", Some(&text)).await;
        self.update_event(crate::app_audit::UPDATE_REFUSED_EVENT, json!({"name": key, "reason": reason, "why": one_line(&text)})).await;
        Err(err(code::FORBIDDEN, text))
    }

    /// Does the key's record, read now, hold `update`, and is it standing authorized? `None` when the key is gone or the record cannot be read.
    fn update_key_state(&self, key: &str) -> Option<(bool, bool)> {
        let keys = KeyStore::at(&self.engine.ws.home).load().ok()?;
        keys.iter().find(|k| k.name == key).map(|k| (k.grants.iter().any(|g| g == "update"), k.update_auto))
    }

    /// `app.update_apply` (item 2): no parameter. The caller is a key the server found to hold `update` at this moment.
    pub async fn app_update_apply(self: &Arc<Self>, key: &str, params: Value) -> RpcOutcome {
        let now = (self.engine.clock_unix)();
        let request = {
            let mut st = self.update.lock().unwrap();
            st.made += 1;
            format!("au-{:x}-{}", crate::host_rpc::now_ms(), st.made)
        };
        let empty = match &params {
            Value::Null => true,
            Value::Object(o) => o.is_empty(),
            _ => false,
        };
        if !empty {
            // Nothing the app sent is repeated, and nothing is counted: a malformed call costs nothing.
            return Err(err(code::INVALID_PARAMS, "`app.update_apply` takes no parameters: not a version, a channel, a URL or a path"));
        }
        {
            let mut tally = self.app_asked.lock().unwrap();
            tally.admit(key, now, &|_| false).map_err(|m| err(code::FORBIDDEN, m))?;
        }
        let Some((holds, auto)) = self.update_key_state(key) else { return self.update_refuse(key, &request, "no_key", "this key is no longer paired".into()).await };
        if !holds {
            return self.update_refuse(key, &request, "no_grant", "this key does not hold `update`".into()).await;
        }
        let code = fresh_code().map_err(|e| err(code::INTERNAL, e))?;
        // One look at the state decides and records: whether this request is refused, is the one already waiting, becomes the waiting one, or
        // goes on. Nothing is awaited while the lock is held, and two keys asking at once cannot both become the pending request.
        let verdict = {
            let mut st = self.update.lock().unwrap();
            let at = st.now();
            if st.pending.as_ref().is_some_and(|p| stale(p, at, now)) {
                st.pending = None;
            }
            if st.running {
                Verdict::Refuse("update_running", "an update is already running".to_string())
            } else if let Some(pid) = st.orphan.filter(|p| alive(*p)) {
                Verdict::Refuse("orphan_update", format!("an update that an earlier run of this daemon started (process {pid}) may still be running; wait for it to end"))
            } else if let Some(since) = st.last_apply.get(key).filter(|t| at - **t < APPLY_EVERY_SECS) {
                Verdict::Refuse("one_an_hour", format!("one update an hour for one key; this key's last was started {} minutes ago", (at - *since) / 60))
            } else if let Some((ended, ok)) = st.last_end.filter(|(t, ok)| at - *t < if *ok { AFTER_OK_SECS } else { AFTER_FAIL_SECS }) {
                Verdict::Refuse("too_soon", format!("the last update ended {} minutes ago{}; the next waits {} minutes after one", (at - ended) / 60, if ok { "" } else { " in a failure" }, (if ok { AFTER_OK_SECS } else { AFTER_FAIL_SECS }) / 60))
            } else if st.restart_pending {
                Verdict::Refuse("restart_pending", "an update was applied and this daemon has not been restarted since; restart it first".to_string())
            } else if let Some(v) = st.available.clone() {
                if !newer(&v, Self::installed_version()) {
                    Verdict::Refuse("not_newer", "the release found is not newer than the one installed".to_string())
                } else if let Some(p) = st.pending.as_ref() {
                    if p.key == key {
                        Verdict::Waiting { request: p.request.clone(), code: p.code.clone(), left: (PENDING_SECS - (at - p.issued)).max(0), version: v }
                    } else {
                        Verdict::Refuse("another_pending", "another update request is waiting for approval at the terminal".to_string())
                    }
                } else if auto {
                    Verdict::Go { version: v }
                } else {
                    st.pending = Some(Pending { key: key.to_string(), version: v.clone(), request: request.clone(), code: code.clone(), issued: at, issued_wall: now, wrong: 0 });
                    Verdict::Pending { code: code.clone(), version: v }
                }
            } else {
                Verdict::Refuse("none_available", "no newer stable release is known; the daemon looks at start and every 6 hours".to_string())
            }
        };
        match verdict {
            Verdict::Refuse(reason, text) => self.update_refuse(key, &request, reason, text).await,
            // The same key again: the same answer, and the clock is not extended. The code goes to this connection, which is the key's own.
            Verdict::Waiting { request, code, left, version } => Ok(json!({"request": request, "state": "waiting", "version": version, "code": code, "expires_in": left})),
            Verdict::Go { version } => self.update_begin(key, &version, &request).await.map(|_| json!({"request": request, "state": "running", "version": version})),
            Verdict::Pending { code, version } => {
                self.update_request_event(key, &request, "waiting", None).await;
                Ok(json!({"request": request, "state": "waiting", "version": version, "code": code, "expires_in": PENDING_SECS}))
            }
        }
    }

    /// Start the apply for `key`: the checks that must hold at the moment it starts, the child, and its outcome in a task of its own.
    /// `version` is the release the daemon found and the operator approved (or the standing authorization covered); it is the one the
    /// command is told to install.
    async fn update_begin(self: &Arc<Self>, key: &str, version: &str, request: &str) -> Result<(), delphi_core::proto::RpcError> {
        // The key must still hold the grant, and the program must still be the installed one, now that it is time to go.
        let still = self.update_key_state(key).is_some_and(|(holds, _)| holds);
        let ready = if still { self.update_exe().map(|_| ()) } else { Err("this key no longer holds `update`".to_string()) };
        if let Err(text) = ready {
            return self.update_refuse(key, request, if still { "not_installed" } else { "no_grant" }, text).await.map(|_| ());
        }
        let verdict = {
            let mut st = self.update.lock().unwrap();
            let at = st.now();
            if st.running {
                Some(("update_running", "an update is already running"))
            } else if st.orphan.is_some_and(alive) {
                Some(("orphan_update", "an update that an earlier run of this daemon started may still be running"))
            } else if st.last_apply.get(key).is_some_and(|t| at - *t < APPLY_EVERY_SECS) {
                Some(("one_an_hour", "one update an hour for one key"))
            } else if st.last_end.is_some_and(|(t, ok)| at - t < if ok { AFTER_OK_SECS } else { AFTER_FAIL_SECS }) {
                Some(("too_soon", "the last update ended too recently for another to start"))
            } else if !st.available.as_deref().is_some_and(|a| a == version && newer(a, Self::installed_version())) {
                Some(("not_newer", "the release found is not newer than the one installed"))
            } else {
                st.running = true;
                st.epoch += 1;
                st.last_apply.insert(key.to_string(), at);
                None
            }
        };
        if let Some((reason, text)) = verdict {
            return self.update_refuse(key, request, reason, text.to_string()).await.map(|_| ());
        }
        self.update_request_event(key, request, "running", None).await;
        let (svc, key, version, request) = (self.clone(), key.to_string(), version.to_string(), request.to_string());
        tokio::spawn(async move {
            let mut guard = RunGuard { svc: svc.clone(), armed: true };
            #[cfg(test)]
            if svc.update.lock().unwrap().panic_in_apply {
                panic!("a panic in the apply task (a test's)");
            }
            let ran = svc.update_run(Clause::Apply { version: &version }).await;
            svc.update_finish(&key, &version, &request, ran, &mut guard).await;
        });
        Ok(())
    }

    /// The end of the apply, as events: `app.updated` for the install that was approved; `app.update_refused` for anything else, and
    /// loudly for an install of a different release than the one approved, which still leaves the daemon needing a restart.
    async fn update_finish(&self, key: &str, version: &str, request: &str, ran: Result<Ran, String>, guard: &mut RunGuard) {
        let from = Self::installed_version();
        enum End {
            Installed(String),
            Different(String),
            NotNewer,
            Failed(String),
        }
        let end = match ran {
            Err(e) => End::Failed(e),
            Ok(r) if r.code == Some(EXIT_NOT_NEWER) => End::NotNewer,
            Ok(r) if r.code == Some(0) => match parse_installed(&r.stdout) {
                Some(to) if to == version => End::Installed(to),
                Some(to) => End::Different(to),
                None => End::Failed("the program finished without installing: it says it is already the newest".to_string()),
            },
            Ok(r) => End::Failed(format!("the update did not complete (exit {}): {}", r.code.map_or("signal".to_string(), |c| c.to_string()), one_line(&r.stderr))),
        };
        let installed = matches!(end, End::Installed(_) | End::Different(_));
        {
            let mut st = self.update.lock().unwrap();
            let at = st.now();
            st.running = false;
            st.epoch += 1;
            st.last_end = Some((at, matches!(end, End::Installed(_))));
            if installed {
                st.restart_pending = true;
                st.available = None;
            }
        }
        guard.armed = false;
        if installed {
            self.update_publish();
        }
        match end {
            End::Installed(to) => {
                self.update_event(crate::app_audit::UPDATED_EVENT, json!({"name": key, "from": from, "to": to, "restart_pending": true})).await;
                self.update_request_event(key, request, "done", None).await;
            }
            End::Different(to) => {
                let text = format!("installed a different release than the one approved (approved {version}, installed {to}); restart the daemon, and check what is installed");
                self.update_event(crate::app_audit::UPDATE_REFUSED_EVENT, json!({"name": key, "reason": "installed_different_release", "approved": version, "installed": to, "restart_pending": true, "why": text})).await;
                self.update_request_event(key, request, "failed", Some(&text)).await;
            }
            End::NotNewer => {
                let text = format!("the program refused to install {version}: it is not newer than the program installed now (it was updated since the daemon looked); nothing was installed");
                self.update_event(crate::app_audit::UPDATE_REFUSED_EVENT, json!({"name": key, "reason": "not_newer", "approved": version, "why": text})).await;
                self.update_request_event(key, request, "refused", Some(&text)).await;
            }
            End::Failed(text) => {
                self.update_event(crate::app_audit::UPDATE_REFUSED_EVENT, json!({"name": key, "reason": "update_failed", "why": one_line(&text)})).await;
                self.update_request_event(key, request, "failed", Some(&text)).await;
            }
        }
    }

    /// `app.update_pending` and `app.update_approve` (item 5), for the server, which has checked the caller's scope and terminal.
    pub async fn app_update_admin(self: &Arc<Self>, method: &str, params: Value) -> Option<RpcOutcome> {
        match method {
            // What the console sees: the key and the release the DAEMON found, never anything the app said, and never the code.
            "app.update_pending" => {
                let wall = (self.engine.clock_unix)();
                let mut st = self.update.lock().unwrap();
                let at = st.now();
                if st.pending.as_ref().is_some_and(|p| stale(p, at, wall)) {
                    st.pending = None;
                }
                let rows: Vec<Value> = st.pending.iter().map(|p| json!({"request_id": p.request, "key": p.key, "version": p.version, "installed": Self::installed_version(), "age_secs": at - p.issued, "expires_in": (PENDING_SECS - (at - p.issued)).max(0)})).collect();
                Some(Ok(json!({"pending": rows, "update_available": st.available, "restart_pending": st.restart_pending})))
            }
            "app.update_approve" => Some(self.update_approve(params).await),
            _ => None,
        }
    }

    /// The operator typed the code shown in the app's window. Only that code approves the request that waits; Enter alone, or any other
    /// string, approves nothing, and three wrong ones cancel the request (so a local process can cancel one by guessing: a nuisance, and no more).
    async fn update_approve(self: &Arc<Self>, params: Value) -> RpcOutcome {
        enum Outcome {
            Nothing,
            Expired(String, String),
            Wrong { key: String, request: String, cancelled: bool },
            Go(Pending),
        }
        let typed = app_keys::normalize_code(params["code"].as_str().unwrap_or(""));
        let wall = (self.engine.clock_unix)();
        // Decided under the lock, with nothing awaited there.
        let outcome = {
            let mut st = self.update.lock().unwrap();
            let at = st.now();
            match st.pending.as_mut() {
                None => Outcome::Nothing,
                Some(p) if stale(p, at, wall) => {
                    let v = Outcome::Expired(p.key.clone(), p.request.clone());
                    st.pending = None;
                    v
                }
                Some(p) if !typed.as_deref().is_some_and(|t| ct_eq(t, &p.code.replace('-', ""))) => {
                    p.wrong += 1;
                    let v = Outcome::Wrong { key: p.key.clone(), request: p.request.clone(), cancelled: p.wrong >= MAX_WRONG };
                    if p.wrong >= MAX_WRONG {
                        st.pending = None;
                    }
                    v
                }
                Some(_) => Outcome::Go(st.pending.take().unwrap()),
            }
        };
        match outcome {
            Outcome::Nothing => Err(err(code::NOT_FOUND, "no update is waiting for approval")),
            Outcome::Expired(key, request) => {
                self.update_request_event(&key, &request, "refused", Some("not approved in 10 minutes")).await;
                self.update_event(crate::app_audit::UPDATE_REFUSED_EVENT, json!({"name": key, "reason": "expired_request"})).await;
                Err(err(code::FORBIDDEN, "that update request has expired; the app asks again"))
            }
            Outcome::Wrong { key, request, cancelled } => {
                self.update_event(crate::app_audit::UPDATE_REFUSED_EVENT, json!({"name": key, "reason": if cancelled { "wrong_code_limit" } else { "wrong_code" }})).await;
                if cancelled {
                    self.update_request_event(&key, &request, "refused", Some("the wrong code was typed 3 times")).await;
                }
                Err(err(code::FORBIDDEN, if cancelled { format!("that is not the code, {MAX_WRONG} times: the request is cancelled and the app asks again") } else { "that is not the code shown in the app's window".to_string() }))
            }
            Outcome::Go(taken) => {
                self.update_begin(&taken.key, &taken.version, &taken.request).await?;
                Ok(json!({"approved": true, "version": taken.version, "name": taken.key}))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_clauses_are_made_from_the_daemons_own_words() {
        let home = Path::new("/h/.delphi");
        assert_eq!(apply_argv("default", home, "0.6.6"), ["update", "--no-restart", "--review", "--only-newer", "--version", "v0.6.6", "--workspace", "default", "--home", "/h/.delphi"].map(OsString::from));
        assert_eq!(check_argv(home), ["update", "--check", "--home", "/h/.delphi"].map(OsString::from));
        for ok in ["default", "my-ws", "a.b_c", "7"] {
            assert!(plain_name(ok), "{ok}");
        }
        for bad in ["", "-rf", "--home", "a b", "a/b", "é", &"x".repeat(65), ".hidden"] {
            assert!(!plain_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_release_is_three_numbers_and_newer_means_greater() {
        assert_eq!(parse_release("v0.6.6"), Some((0, 6, 6)));
        assert_eq!(parse_release("1.2.3"), Some((1, 2, 3)));
        for bad in ["", "1.2", "1.2.3.4", "1.2.3-rc1", "a.b.c", "1.2.x", "1..3", "<b>", "99999.0.0"] {
            assert_eq!(parse_release(bad), None, "{bad:?}");
        }
        assert!(newer("0.6.6", "0.6.5") && newer("0.10.0", "0.9.9") && newer("1.0.0", "0.99.99"));
        assert!(!newer("0.6.5", "0.6.5") && !newer("0.6.4", "0.6.5") && !newer("0.6.6-rc1", "0.6.5") && !newer("x", "0.6.5"));
    }

    #[test]
    fn only_the_line_delphi_update_check_prints_for_an_update_is_a_release() {
        let found = "0.6.5 -> v0.6.6\nAn update is available. Run `delphi update` to install it.\n";
        assert_eq!(parse_check(found, "0.6.5").as_deref(), Some("0.6.6"));
        assert_eq!(parse_check(found, "0.6.6"), None, "not newer than the daemon");
        assert_eq!(parse_check(found, "0.7.0"), None, "never a downgrade");
        assert_eq!(parse_check("delphi 0.6.5 is the latest release.\n", "0.6.5"), None);
        assert_eq!(parse_check("0.6.5 -> v0.6.6\n", "0.6.5"), None, "the line alone is not enough");
        assert_eq!(parse_check("0.6.5 -> v0.6.6-rc1\nAn update is available. x\n", "0.6.5"), None, "a pre-release is not a release");
        assert_eq!(parse_check("x -> v0.6.6\nAn update is available. x\n", "0.6.5"), None);
        assert_eq!(parse_check("0.6.5 -> $(id)\nAn update is available. x\n", "0.6.5"), None);
        assert_eq!(parse_installed("Installed v0.6.6 at /x/delphi (the old version is kept as /x/delphi.prev)\n").as_deref(), Some("0.6.6"));
        assert_eq!(parse_installed("delphi 0.6.5 is the latest release.\n"), None);
        assert_eq!(parse_installed("Installed whatever at /x\n"), None);
    }

    #[test]
    fn the_program_started_is_a_delphi_that_only_this_user_can_change() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let ok = t.path().join("delphi");
        std::fs::write(&ok, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&ok, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(is_installed_program(&ok).unwrap(), std::fs::canonicalize(&ok).unwrap(), "the canonical path is what is returned, and started");
        std::fs::set_permissions(&ok, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(is_installed_program(&ok).is_err(), "group-writable");
        let other = t.path().join("not-delphi");
        std::fs::write(&other, "#!/bin/sh\n").unwrap();
        assert!(is_installed_program(&other).is_err(), "another name");
        assert!(is_installed_program(Path::new("delphi")).is_err() && is_installed_program(&t.path().join("nope/delphi")).is_err());
    }

    /// Item 6: a folder above the program that others can write could be used to swap the program; a link is followed and what it leads to
    /// is judged.
    #[test]
    fn every_folder_above_the_program_must_be_closed_to_others() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let open = t.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o775)).unwrap();
        let inner = open.join("bin");
        std::fs::create_dir(&inner).unwrap();
        std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o755)).unwrap();
        let exe = inner.join("delphi");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = is_installed_program(&exe).unwrap_err();
        assert!(e.contains("can be written by others") && e.contains("open"), "{e}");
        // A link named `delphi` that leads into the open folder is judged by where it leads.
        let safe = t.path().join("safe");
        std::fs::create_dir(&safe).unwrap();
        std::os::unix::fs::symlink(&exe, safe.join("delphi")).unwrap();
        assert!(is_installed_program(&safe.join("delphi")).is_err(), "a link into a folder others can write");
        // A closed folder above it is fine again, and a link under another name to a program of the right name is judged by the target.
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_installed_program(&exe).is_ok());
        std::os::unix::fs::symlink(&exe, t.path().join("other-name")).unwrap();
        assert_eq!(is_installed_program(&t.path().join("other-name")).unwrap(), std::fs::canonicalize(&exe).unwrap());
    }

    // ── The engine's half, with a script standing in for the installed program ─────────────────────────────────────────────
    //
    // No test here runs a real `delphi update`: the program the daemon starts is a script named `delphi` in a private folder,
    // which records how it was started and prints what it is told to print.

    use crate::service::EngineService;
    use crate::testkit::{events_of, setup, Fx};
    use delphi_core::app_keys::AppKey;
    use delphi_core::proto::code as rpc;
    use delphi_core::proto::RpcError;
    use std::os::unix::fs::PermissionsExt;

    const KEY: &str = "app-0a1b2c";
    const OTHER: &str = "app-0d0e0f";
    const FOUND: &str = "0.0.1 -> v99.0.0\nAn update is available. Run `delphi update` to install it.\n";
    const INSTALLED: &str = "0.0.1 -> v99.0.0\nInstalled v99.0.0 at /x/delphi (the old version is kept as /x/delphi.prev; `delphi update --rollback` restores it).\n";

    struct Rig {
        f: Fx,
        svc: Arc<EngineService>,
        rec: PathBuf,
        bin: tempfile::TempDir,
    }

    fn key(name: &str, grants: &[&str], auto: bool) -> AppKey {
        AppKey { name: name.into(), public_key: "ab".repeat(32), paired_at: "2026-11-20T00:00:00.000Z".into(), last_used: "2026-11-20T00:00:00.000Z".into(), app_version: "0.6.5".into(), grants: grants.iter().map(|g| g.to_string()).collect(), update_auto: auto }
    }

    fn rig(keys: Vec<AppKey>) -> Rig {
        let f = setup(vec![], 5.0, |_| {});
        let svc = EngineService::new(f.engine.clone(), "m1");
        svc.adopt(f.m.clone());
        KeyStore::at(&f.engine.ws.home).save(&keys).unwrap();
        let bin = tempfile::tempdir().unwrap();
        let rec = bin.path().join("rec");
        std::fs::create_dir(&rec).unwrap();
        let script = format!(
            "#!/bin/sh\nrec='{}'\n{{ echo \"ARGV $*\"; echo \"PWD $(pwd)\"; echo \"TMP $TMPDIR\"; echo \"ENV $(env | cut -d= -f1 | sort | tr '\\n' ' ')\"; echo \"PATH $PATH\"; if [ -t 0 ]; then echo 'STDIN tty'; else echo 'STDIN none'; fi; }} >> \"$rec/calls\"\nif [ -e \"$rec/leakfd\" ]; then if [ -e \"/dev/fd/$(cat \"$rec/leakfd\")\" ]; then echo LEAK >> \"$rec/leak\"; else echo CLEAN >> \"$rec/leak\"; fi; fi\nif [ -e \"$rec/hang\" ]; then sleep 300 & echo $! > \"$rec/bgpid\"; sleep 300; fi\nif [ -e \"$rec/linger\" ]; then sleep 300 & echo $! > \"$rec/bgpid\"; fi\ncase \"$*\" in\n  *--check*) if [ -e \"$rec/slow\" ]; then while [ ! -e \"$rec/go\" ]; do sleep 0.05; done; fi; cat \"$rec/check.out\"; exit 0 ;;\n  *) cat \"$rec/apply.out\"; exit \"$(cat \"$rec/apply.code\")\" ;;\nesac\n",
            rec.display()
        );
        let exe = bin.path().join("delphi");
        std::fs::write(&exe, script).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(rec.join("check.out"), FOUND).unwrap();
        std::fs::write(rec.join("apply.out"), INSTALLED).unwrap();
        std::fs::write(rec.join("apply.code"), "0").unwrap();
        svc.set_update_exe(exe);
        Rig { f, svc, rec, bin }
    }

    impl Rig {
        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(self.rec.join("calls")).unwrap_or_default().lines().map(String::from).collect()
        }

        fn applies(&self) -> usize {
            self.calls().iter().filter(|l| l.starts_with("ARGV update --no-restart")).count()
        }

        fn kinds(&self, kind: &str) -> Vec<Value> {
            events_of(&self.f, kind).into_iter().map(|e| e.payload).collect()
        }

        async fn wait(&self, kind: &str, n: usize) -> Vec<Value> {
            let end = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                let got = self.kinds(kind);
                if got.len() >= n {
                    return got;
                }
                assert!(std::time::Instant::now() < end, "no {kind} event: {:?}", self.f.m.replay().unwrap().iter().map(|e| e.kind.clone()).collect::<Vec<_>>());
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }

        fn states(&self) -> Vec<String> {
            self.kinds("app.request").iter().filter(|p| p["method"] == "app.update_apply").map(|p| p["state"].as_str().unwrap().to_string()).collect()
        }

        fn advance(&self, secs: i64) {
            self.svc.update_advance(secs);
        }
    }

    fn refused(r: Result<Value, RpcError>) -> (i64, String) {
        let e = r.unwrap_err();
        (e.code, e.message)
    }

    /// AC-16: a call carrying any parameter is refused, counts nothing, starts nothing and writes nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_call_with_any_parameter_is_refused_before_anything_happens() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        let before = r.calls().len();
        for bad in [json!({"version": "99.0.0"}), json!({"url": "http://x"}), json!({"channel": "dev"}), json!({"path": "/tmp/x"}), json!({"": ""}), json!("99.0.0"), json!(["x"]), json!(7), json!(true)] {
            let (c, m) = refused(r.svc.app_update_apply(KEY, bad.clone()).await);
            assert_eq!(c, rpc::INVALID_PARAMS, "{bad}");
            assert!(m.contains("takes no parameters") && !m.contains("99.0.0") && !m.contains("http"), "{m}");
        }
        assert_eq!(r.calls().len(), before, "nothing was started");
        assert!(r.kinds("app.request").is_empty() && r.kinds("app.update_refused").is_empty() && r.kinds("app.updated").is_empty());
        assert_eq!(r.svc.update_facts(), (Some("99.0.0".to_string()), false));
    }

    /// AC-17: without `--auto`, the call makes a pending request naming the release the daemon found, and changes nothing. Only the code
    /// the app was given approves it: Enter, a typed version, a wrong code and the third wrong one do not, and the console's list never holds the code.
    #[tokio::test(flavor = "multi_thread")]
    async fn without_auto_the_call_waits_for_the_code_typed_at_a_terminal() {
        let r = rig(vec![key(KEY, &["read", "update"], false), key(OTHER, &["read", "update"], false)]);
        r.svc.update_check_now().await;
        assert_eq!(r.calls().iter().filter(|l| l.starts_with("ARGV")).count(), 1, "one check, no apply");
        let asked = r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        assert_eq!((asked["state"].as_str(), asked["version"].as_str(), asked["expires_in"].as_i64()), (Some("waiting"), Some("99.0.0"), Some(PENDING_SECS)));
        let code = asked["code"].as_str().unwrap().to_string();
        assert!(app_keys::normalize_code(&code).is_some(), "{code}");
        assert_eq!(r.states(), ["waiting"]);
        assert!(r.kinds("app.updated").is_empty());
        // The same key again: the same request and the same code. Another key: refused, one at a time.
        let again = r.svc.app_update_apply(KEY, json!(null)).await.unwrap();
        assert_eq!((again["request"].clone(), again["state"].as_str(), again["code"].clone()), (asked["request"].clone(), Some("waiting"), asked["code"].clone()));
        let (c, m) = refused(r.svc.app_update_apply(OTHER, json!({})).await);
        assert_eq!(c, rpc::FORBIDDEN);
        assert!(m.contains("another update request is waiting"), "{m}");
        // What the console sees: the key and the release the DAEMON found, never the code, and no event holds it either.
        let listed = r.svc.app_update_admin("app.update_pending", json!({})).await.unwrap().unwrap();
        assert_eq!((listed["pending"][0]["version"].as_str(), listed["pending"][0]["key"].as_str(), listed["pending"][0]["installed"].as_str()), (Some("99.0.0"), Some(KEY), Some(env!("CARGO_PKG_VERSION"))));
        let seen = format!("{listed}{:?}", r.f.m.replay().unwrap().iter().map(|e| e.payload.to_string()).collect::<Vec<_>>());
        assert!(!seen.contains(&code) && !seen.contains(&code.replace('-', "")), "the code is in what the console or the log can see");
        // Enter, a typed version and the third wrong string: refused, nothing installed, and the third cancels the request.
        let wrong = if code == "AAA-AAA" { "BBB-BBB" } else { "AAA-AAA" };
        for (i, bad) in ["", "99.0.0", wrong].into_iter().enumerate() {
            let (c, m) = refused(r.svc.app_update_admin("app.update_approve", json!({"code": bad})).await.unwrap());
            assert_eq!(c, rpc::FORBIDDEN, "{bad:?}");
            assert_eq!(m.contains("cancelled"), i == 2, "{m}");
        }
        assert_eq!(r.applies(), 0);
        assert_eq!(refused(r.svc.app_update_admin("app.update_approve", json!({"code": code.clone()})).await.unwrap()).0, rpc::NOT_FOUND, "the right code finds nothing waiting now");
        assert!(r.kinds("app.update_refused").iter().any(|p| p["reason"] == "wrong_code_limit"));
        assert_eq!(r.applies(), 0);

        // A new request, then the code as typed from the window (in lower case, with or without the dash).
        let asked = r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let code = asked["code"].as_str().unwrap().to_string();
        let approved = r.svc.app_update_admin("app.update_approve", json!({"code": code.to_lowercase()})).await.unwrap().unwrap();
        assert_eq!((approved["approved"].as_bool(), approved["version"].as_str(), approved["name"].as_str()), (Some(true), Some("99.0.0"), Some(KEY)));
        let done = r.wait("app.updated", 1).await;
        assert_eq!((done[0]["name"].as_str(), done[0]["from"].as_str(), done[0]["to"].as_str(), done[0]["restart_pending"].as_bool()), (Some(KEY), Some(env!("CARGO_PKG_VERSION")), Some("99.0.0"), Some(true)));
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while r.states().last().map(String::as_str) != Some("done") {
            assert!(std::time::Instant::now() < end, "{:?}", r.states());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(r.applies(), 1);
        let apply = r.calls().into_iter().find(|l| l.starts_with("ARGV update --no-restart")).unwrap();
        assert!(apply.contains("--version v99.0.0") && apply.contains("--review") && apply.contains("--only-newer"), "{apply}");
    }

    /// A request that is not approved in ten minutes is dropped, and the version typed afterwards approves nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_request_that_is_not_approved_in_ten_minutes_changes_nothing() {
        let r = rig(vec![key(KEY, &["read", "update"], false)]);
        r.svc.update_check_now().await;
        let code = r.svc.app_update_apply(KEY, json!({})).await.unwrap()["code"].as_str().unwrap().to_string();
        r.advance(PENDING_SECS + 1);
        let (c, m) = refused(r.svc.app_update_admin("app.update_approve", json!({"code": code})).await.unwrap());
        assert_eq!(c, rpc::FORBIDDEN);
        assert!(m.contains("expired"), "{m}");
        assert_eq!(r.applies(), 0);
        assert!(r.kinds("app.update_refused").iter().any(|p| p["reason"] == "expired_request"));
        assert_eq!(refused(r.svc.app_update_admin("app.update_approve", json!({"code": "ABC-DEF"})).await.unwrap()).0, rpc::NOT_FOUND);
        // A request that waited past the time is not listed either.
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        r.advance(PENDING_SECS + 1);
        assert_eq!(r.svc.app_update_admin("app.update_pending", json!({})).await.unwrap().unwrap()["pending"], json!([]));
    }

    /// AC-18 and the spawn's own rules: the program started is the one the daemon runs as, with the argument list of clause 1 (the
    /// daemon's own workspace name and home), no terminal, a private folder, and an environment of four variables and what the shell adds.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_apply_starts_the_installed_program_with_clause_ones_arguments_and_nothing_from_the_app() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        let asked = r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        assert_eq!((asked["state"].as_str(), asked["version"].as_str()), (Some("running"), Some("99.0.0")), "a standing authorization goes on");
        r.wait("app.updated", 1).await;
        let calls = r.calls();
        let apply: Vec<&String> = calls.iter().skip_while(|l| !l.starts_with("ARGV update --no-restart")).take(6).collect();
        assert_eq!(apply.len(), 6, "{calls:?}");
        let name = r.f.engine.ws.dir.file_name().unwrap().to_string_lossy().into_owned();
        let home = r.f.engine.ws.home.display().to_string();
        assert_eq!(apply[0], &format!("ARGV update --no-restart --review --only-newer --version v99.0.0 --workspace {name} --home {home}"));
        let words = |argv: Vec<OsString>| argv.iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        assert_eq!(apply[0], &format!("ARGV {}", words(apply_argv(&name, &r.f.engine.ws.home, "99.0.0"))));
        assert!(apply[1].starts_with("PWD ") && apply[1].contains("delphi-update-"), "a private folder of its own: {}", apply[1]);
        assert_eq!(apply[2].trim_start_matches("TMP ").trim_start_matches("/private"), apply[1].trim_start_matches("PWD ").trim_start_matches("/private"), "and it is the temporary folder");
        let env = apply[3].trim_start_matches("ENV ");
        for v in env.split_whitespace() {
            assert!(["DELPHI_HOME", "HOME", "PATH", "PWD", "SHLVL", "TMPDIR", "_", "OLDPWD"].contains(&v), "{v} is not in the child's environment: {env}");
        }
        assert!(env.contains("DELPHI_HOME") && env.contains("PATH") && env.contains("TMPDIR"), "{env}");
        assert_eq!(apply[4], "PATH /usr/bin:/bin");
        assert_eq!(apply[5], "STDIN none");
        assert!(!std::path::Path::new(apply[1].trim_start_matches("PWD ")).exists(), "the private folder is removed");
        // The check (clause 2) is its own list.
        assert!(calls.contains(&format!("ARGV update --check --home {home}")), "{calls:?}");
        assert_eq!(r.svc.update_facts(), (None, true));
    }

    /// AC-19: none known, a release no newer than the one installed, a second call within the hour, and a daemon that was updated and not
    /// restarted each refuse by name, and start nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_release_no_newer_or_a_second_call_within_the_hour_is_refused_with_the_reason() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        let (c, m) = refused(r.svc.app_update_apply(KEY, json!({})).await);
        assert_eq!(c, rpc::FORBIDDEN);
        assert!(m.contains("no newer stable release is known"), "{m}");
        // A release the program says is older, or the same, or not a release: it is not an update.
        let same = format!("{0} -> v{0}\nAn update is available. x\n", env!("CARGO_PKG_VERSION"));
        for out in ["0.0.1 -> v0.0.0\nAn update is available. x\n", same.as_str(), "0.0.1 -> v99.0.0-rc1\nAn update is available. x\n", "delphi 0.0.1 is the latest release.\n", "garbage"] {
            std::fs::write(r.rec.join("check.out"), out).unwrap();
            r.svc.update_check_now().await;
            assert_eq!(r.svc.update_facts().0, None, "{out:?}");
            assert!(r.svc.app_update_apply(KEY, json!({})).await.is_err());
        }
        assert_eq!(r.applies(), 0);
        // A release newer than this daemon: it goes on (standing authorization), and a second call in the hour is refused.
        std::fs::write(r.rec.join("check.out"), FOUND).unwrap();
        r.svc.update_check_now().await;
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let (c, m) = refused(r.svc.app_update_apply(KEY, json!({})).await);
        assert_eq!(c, rpc::FORBIDDEN);
        assert!(m.contains("already running") || m.contains("one update an hour"), "{m}");
        r.wait("app.updated", 1).await;
        let (_, m) = refused(r.svc.app_update_apply(KEY, json!({})).await);
        assert!(m.contains("one update an hour"), "{m}");
        assert_eq!(r.applies(), 1, "one apply in all");
        // The hour passes: still no second apply, since the daemon has not been restarted since the first.
        r.advance(APPLY_EVERY_SECS + 1);
        r.svc.update_check_now().await;
        let (_, m) = refused(r.svc.app_update_apply(KEY, json!({})).await);
        assert!(m.contains("has not been restarted"), "{m}");
        let reasons: Vec<String> = r.kinds("app.update_refused").iter().map(|p| p["reason"].as_str().unwrap().to_string()).collect();
        assert!(reasons.iter().any(|x| x == "one_an_hour") && reasons.iter().any(|x| x == "restart_pending") && reasons.iter().any(|x| x == "none_available"), "{reasons:?}");
        assert_eq!(r.applies(), 1);
    }

    /// A key without the grant, a key that is gone, and a key whose grant was taken between the request and the approval each change
    /// nothing; so does a program that is not the installed `delphi`.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_grant_the_key_and_the_program_are_checked_when_it_is_time_to_go() {
        let r = rig(vec![key(KEY, &["read", "request"], true), key(OTHER, &["read", "update"], false)]);
        r.svc.update_check_now().await;
        let (_, m) = refused(r.svc.app_update_apply(KEY, json!({})).await);
        assert!(m.contains("does not hold `update`"), "{m}");
        let (_, m) = refused(r.svc.app_update_apply("app-ffffff", json!({})).await);
        assert!(m.contains("no longer paired"), "{m}");
        // Pending, then the operator takes the grant away: approval finds nothing to go on with.
        let code = r.svc.app_update_apply(OTHER, json!({})).await.unwrap()["code"].as_str().unwrap().to_string();
        KeyStore::at(&r.f.engine.ws.home).save(&[key(OTHER, &["read"], false)]).unwrap();
        let (_, m) = refused(r.svc.app_update_admin("app.update_approve", json!({"code": code})).await.unwrap());
        assert!(m.contains("no longer holds `update`"), "{m}");
        assert_eq!(r.applies(), 0);
        assert!(!r.svc.update_facts().1);
        // Not the installed program: another name.
        let other = r.bin.path().join("not-delphi");
        std::fs::copy(r.bin.path().join("delphi"), &other).unwrap();
        r.svc.set_update_exe(other);
        KeyStore::at(&r.f.engine.ws.home).save(&[key(OTHER, &["read", "update"], true)]).unwrap();
        let (_, m) = refused(r.svc.app_update_apply(OTHER, json!({})).await);
        assert!(m.contains("not running as an installed `delphi`"), "{m}");
        assert_eq!(r.applies(), 0);
    }

    /// An apply that fails, or that finds nothing to install, is recorded as a refusal; the daemon is not marked for a restart.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_apply_that_fails_or_installs_nothing_is_recorded_as_such() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        std::fs::write(r.rec.join("apply.out"), "delphi update: the checksum did not match\n").unwrap();
        std::fs::write(r.rec.join("apply.code"), "1").unwrap();
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let refusals = r.wait("app.update_refused", 1).await;
        assert_eq!(refusals[0]["reason"], "update_failed");
        assert!(r.kinds("app.updated").is_empty());
        assert_eq!(r.svc.update_facts(), (Some("99.0.0".to_string()), false));
        let (_, m) = refused(r.svc.app_update_apply(KEY, json!({})).await);
        assert!(m.contains("one update an hour"), "a failed apply counts: {m}");
        // Exit 0 without an install: the program said it is already the newest.
        r.advance(APPLY_EVERY_SECS + 1);
        std::fs::write(r.rec.join("apply.out"), "delphi 99.0.0 is the latest release.\n").unwrap();
        std::fs::write(r.rec.join("apply.code"), "0").unwrap();
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while r.kinds("app.update_refused").iter().filter(|p| p["reason"] == "update_failed").count() < 2 {
            assert!(std::time::Instant::now() < end, "{:?}", r.kinds("app.update_refused"));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(r.kinds("app.updated").is_empty());
        assert!(!r.svc.update_facts().1);
        assert_eq!(r.applies(), 2);
    }

    /// Item 12: the daemon's own look for a release, and the optional fields of its presence file. The file keeps `version` 1; a restart
    /// writes a new one without them.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_presence_file_says_what_the_daemon_found_and_that_a_restart_is_due() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        let (home, ws) = (r.f.engine.ws.home.clone(), r.f.engine.ws.dir.clone());
        let l = delphi_core::spawn::layout(&home, &ws).unwrap();
        delphi_core::spawn::ensure_run_dir(&l.run_dir).unwrap();
        let base = delphi_core::presence::Presence::new(&l, &"0123456789abcdef".repeat(2), "0.6.5", &ws, &home);
        delphi_core::presence::write(&l, &base).unwrap();
        r.svc.set_update_presence(l.clone(), base.clone());
        let read = || delphi_core::presence::read(&l.presence).unwrap();
        assert_eq!((read().update_available, read().restart_pending), (None, false));
        r.svc.update_check_now().await;
        let p = read();
        assert_eq!((p.version, p.update_available.as_deref(), p.restart_pending, p.instance.as_str()), (1, Some("99.0.0"), false, base.instance.as_str()), "the rest of the file is as the daemon wrote it");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&l.presence).unwrap()).unwrap();
        for k in v.as_object().unwrap().keys() {
            for bad in ["token", "secret", "ticket", "key"] {
                assert!(!k.contains(bad), "{k}");
            }
        }
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        r.wait("app.updated", 1).await;
        let p = read();
        assert_eq!((p.update_available, p.restart_pending), (None, true));
        // The daemon's restart: a new file from the start, without either.
        delphi_core::presence::write(&l, &base).unwrap();
        assert_eq!((read().update_available, read().restart_pending), (None, false));
        let text = std::fs::read_to_string(&l.presence).unwrap();
        assert!(!text.contains("update_available") && !text.contains("restart_pending"));
    }

    /// Item 1: the command is told which release to install, and an install of any other is a failure written loudly, `--auto` or not.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_install_of_another_release_than_the_approved_one_is_a_loud_failure() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        std::fs::write(r.rec.join("apply.out"), "0.0.1 -> v99.0.1\nInstalled v99.0.1 at /x/delphi (the old version is kept as /x/delphi.prev)\n").unwrap();
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let refusals = r.wait("app.update_refused", 1).await;
        assert_eq!((refusals[0]["reason"].as_str(), refusals[0]["approved"].as_str(), refusals[0]["installed"].as_str()), (Some("installed_different_release"), Some("99.0.0"), Some("99.0.1")));
        assert!(refusals[0]["why"].as_str().unwrap().contains("installed a different release"), "{}", refusals[0]);
        assert!(r.kinds("app.updated").is_empty(), "it is not reported as the update that was approved");
        // Something was installed, so the daemon still needs its restart, and the log says it failed.
        assert_eq!(r.svc.update_facts(), (None, true));
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while r.states().last().map(String::as_str) != Some("failed") {
            assert!(std::time::Instant::now() < end, "{:?}", r.states());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(r.calls().iter().any(|l| l == &format!("ARGV {}", [
            "update", "--no-restart", "--review", "--only-newer", "--version", "v99.0.0", "--workspace", r.f.engine.ws.dir.file_name().unwrap().to_str().unwrap(), "--home", r.f.engine.ws.home.to_str().unwrap()
        ].join(" "))));
    }

    /// Item 3: two keys asking at the same moment cannot both become the pending request, whichever gets the lock first.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_keys_asking_at_once_leave_one_pending_request_and_one_refusal() {
        for round in 0..8 {
            let r = rig(vec![key(KEY, &["read", "update"], false), key(OTHER, &["read", "update"], false)]);
            r.svc.update_check_now().await;
            let (a, b) = (r.svc.clone(), r.svc.clone());
            let (x, y) = tokio::join!(tokio::spawn(async move { a.app_update_apply(KEY, json!({})).await }), tokio::spawn(async move { b.app_update_apply(OTHER, json!({})).await }));
            let (x, y) = (x.unwrap(), y.unwrap());
            assert_eq!(x.is_ok() as u8 + y.is_ok() as u8, 1, "round {round}: exactly one waits: {x:?} {y:?}");
            let winner = if x.is_ok() { KEY } else { OTHER };
            let (_, m) = refused(if x.is_ok() { y } else { x });
            assert!(m.contains("another update request is waiting"), "{m}");
            let listed = r.svc.app_update_admin("app.update_pending", json!({})).await.unwrap().unwrap();
            assert_eq!((listed["pending"].as_array().unwrap().len(), listed["pending"][0]["key"].as_str()), (1, Some(winner)), "round {round}: the request that waits is the one that was told it waits");
            assert!(r.kinds("app.update_refused").iter().any(|p| p["reason"] == "another_pending"));
        }
    }

    /// Item 4: out of time, the child's whole group is ended, and the daemon is not free to start another until it is gone.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_child_that_runs_out_of_time_is_ended_with_everything_it_started() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        std::fs::write(r.rec.join("hang"), "").unwrap();
        r.svc.set_update_limit(Duration::from_millis(1500));
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let refusals = r.wait("app.update_refused", 1).await;
        assert_eq!(refusals[0]["reason"], "update_failed");
        assert!(refusals[0]["why"].as_str().unwrap().contains("did not finish in time"), "{}", refusals[0]);
        let bg: i32 = std::fs::read_to_string(r.rec.join("bgpid")).unwrap().trim().parse().unwrap();
        // The event is written after the group is gone and `running` is cleared: nothing the child started is left.
        assert!(unsafe { libc::kill(bg, 0) } != 0, "the process the child started is still running");
        let st = r.svc.update.lock().unwrap();
        assert!(!st.running);
    }

    /// Item 5: the child holds no descriptor above 2 that the daemon held (the readiness pipe, the lock, a socket).
    #[tokio::test(flavor = "multi_thread")]
    async fn the_child_inherits_no_descriptor_above_two() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        let f = std::fs::File::open("/dev/null").unwrap();
        use std::os::fd::AsRawFd;
        // A descriptor that is not close-on-exec, as the daemon's lock and readiness descriptors are.
        let leaked = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_DUPFD, 200) };
        assert!(leaked >= 200);
        std::fs::write(r.rec.join("leakfd"), leaked.to_string()).unwrap();
        r.svc.update_check_now().await;
        let seen = std::fs::read_to_string(r.rec.join("leak")).unwrap();
        unsafe { libc::close(leaked) };
        assert_eq!(seen.trim(), "CLEAN", "the descriptor was open in the child");
    }

    /// Item 7: any key waits after an apply, longer after an install than after a failure, and the wall clock is no part of it.
    #[tokio::test(flavor = "multi_thread")]
    async fn another_key_waits_after_any_apply_and_the_wall_clock_does_not_count() {
        let r = rig(vec![key(KEY, &["read", "update"], true), key(OTHER, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        std::fs::write(r.rec.join("apply.out"), "delphi update: nope\n").unwrap();
        std::fs::write(r.rec.join("apply.code"), "1").unwrap();
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        r.wait("app.update_refused", 1).await;
        // The wall clock moves a day; the daemon does not take it for time passed.
        r.f.now.fetch_add(86_400, std::sync::atomic::Ordering::SeqCst);
        let (_, m) = refused(r.svc.app_update_apply(OTHER, json!({})).await);
        assert!(m.contains("the next waits 10 minutes"), "{m}");
        r.advance(AFTER_FAIL_SECS + 1);
        std::fs::write(r.rec.join("apply.out"), INSTALLED).unwrap();
        std::fs::write(r.rec.join("apply.code"), "0").unwrap();
        r.svc.app_update_apply(OTHER, json!({})).await.unwrap();
        r.wait("app.updated", 1).await;
        // After an install: an hour, for any key. (The restart is the other reason the next is refused; the wait is named first.)
        r.advance(AFTER_OK_SECS - 100);
        let (_, m) = refused(r.svc.app_update_apply(KEY, json!({})).await);
        assert!(m.contains("the next waits 60 minutes"), "{m}");
    }

    /// Item 8: a check that began before an apply and ends after it is out of date, and changes nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_check_that_straddles_an_apply_is_thrown_away() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        assert_eq!(r.svc.update_facts().0.as_deref(), Some("99.0.0"));
        std::fs::write(r.rec.join("slow"), "").unwrap();
        let svc = r.svc.clone();
        let slow = tokio::spawn(async move { svc.update_check_now().await });
        // Wait until the slow check is running, then apply and finish the apply.
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while r.calls().iter().filter(|l| l.contains("--check")).count() < 2 {
            assert!(std::time::Instant::now() < end);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        r.wait("app.updated", 1).await;
        assert_eq!(r.svc.update_facts(), (None, true));
        // Now the slow check answers, with the release the apply just installed: it must not bring `update_available` back.
        std::fs::write(r.rec.join("go"), "").unwrap();
        slow.await.unwrap();
        assert_eq!(r.svc.update_facts(), (None, true));
    }

    /// Item 8: writers of the presence file go one at a time and each writes what is true when it gets its turn.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_presence_file_is_never_left_older_than_the_state() {
        let r = rig(vec![]);
        let (home, ws) = (r.f.engine.ws.home.clone(), r.f.engine.ws.dir.clone());
        let l = delphi_core::spawn::layout(&home, &ws).unwrap();
        delphi_core::spawn::ensure_run_dir(&l.run_dir).unwrap();
        let base = delphi_core::presence::Presence::new(&l, &"0123456789abcdef".repeat(2), "0.6.5", &ws, &home);
        r.svc.set_update_presence(l.clone(), base);
        let mut tasks = Vec::new();
        for i in 0..24 {
            let svc = r.svc.clone();
            tasks.push(tokio::task::spawn_blocking(move || {
                if i == 12 {
                    let mut st = svc.update.lock().unwrap();
                    st.available = None;
                    st.restart_pending = true;
                }
                svc.update_publish();
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        let p = delphi_core::presence::read(&l.presence).unwrap();
        assert_eq!((p.update_available, p.restart_pending), (None, true));
    }

    /// Round 2, item 1: a program updated by hand to something newer than the approved release refuses to be put back (the child exits 3),
    /// and the daemon reports it by name and installs nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_release_that_is_no_longer_newer_is_refused_by_the_program_and_reported() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        std::fs::write(r.rec.join("apply.out"), "delphi update: v99.0.0 is not newer than the installed 99.0.1; nothing was installed (--only-newer)\n").unwrap();
        std::fs::write(r.rec.join("apply.code"), "3").unwrap();
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let refusals = r.wait("app.update_refused", 1).await;
        assert_eq!((refusals[0]["reason"].as_str(), refusals[0]["approved"].as_str()), (Some("not_newer"), Some("99.0.0")), "{}", refusals[0]);
        assert!(r.kinds("app.updated").is_empty(), "no clean app.updated for an install that did not happen");
        assert_eq!(r.svc.update_facts(), (Some("99.0.0".to_string()), false), "nothing was installed, so no restart is due");
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while r.states().last().map(String::as_str) != Some("refused") {
            assert!(std::time::Instant::now() < end, "{:?}", r.states());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(r.calls().iter().any(|l| l.contains("--only-newer")));
    }

    /// Item 3: a daemon that stops while an apply runs ends the child and what it started.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_daemon_that_stops_ends_the_apply_it_is_running() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        std::fs::write(r.rec.join("hang"), "").unwrap();
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while !r.rec.join("bgpid").exists() || r.svc.update.lock().unwrap().child.is_none() {
            assert!(std::time::Instant::now() < end, "the child never started");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let bg: i32 = std::fs::read_to_string(r.rec.join("bgpid")).unwrap().trim().parse().unwrap();
        assert!(unsafe { libc::kill(bg, 0) } == 0, "the child's process runs");
        r.svc.update_shutdown().await;
        let end = std::time::Instant::now() + Duration::from_secs(10);
        while unsafe { libc::kill(bg, 0) } == 0 {
            assert!(std::time::Instant::now() < end, "the process the child started survived the daemon's shutdown");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Item 3: the marker beside the presence file names a running apply's child and is gone when it ends; a new daemon that finds one
    /// naming a live process starts no apply, and one naming a dead process removes it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_marker_names_a_running_apply_and_a_new_daemon_waits_for_an_orphan() {
        use std::os::unix::fs::PermissionsExt;
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        let (home, ws) = (r.f.engine.ws.home.clone(), r.f.engine.ws.dir.clone());
        let l = delphi_core::spawn::layout(&home, &ws).unwrap();
        delphi_core::spawn::ensure_run_dir(&l.run_dir).unwrap();
        let base = delphi_core::presence::Presence::new(&l, &"0123456789abcdef".repeat(2), "0.6.5", &ws, &home);
        let marker = l.presence.with_extension("update.lock");
        assert!(marker.to_string_lossy().ends_with(".update.lock"), "{}", marker.display());

        // A dead process: the marker is removed and nothing is held back.
        let mut gone = std::process::Command::new("/bin/sleep").arg("0").spawn().unwrap();
        let dead = gone.id();
        gone.wait().unwrap();
        std::fs::write(&marker, format!("{dead} 1\n")).unwrap();
        r.svc.set_update_presence(l.clone(), base.clone());
        assert!(!marker.exists(), "a marker whose process is gone is removed");

        // A running apply writes its marker (0600, pid and time) and removes it at the end.
        r.svc.update_check_now().await;
        std::fs::write(r.rec.join("hang"), "").unwrap();
        r.svc.set_update_limit(Duration::from_secs(3));
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while !marker.exists() {
            assert!(std::time::Instant::now() < end, "no marker while the child runs");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let text = std::fs::read_to_string(&marker).unwrap();
        let mut words = text.split_whitespace();
        let (pid, at): (i32, i64) = (words.next().unwrap().parse().unwrap(), words.next().unwrap().parse().unwrap());
        assert!(pid > 0 && at > 0, "{text}");
        assert_eq!(std::fs::metadata(&marker).unwrap().permissions().mode() & 0o777, 0o600);
        r.wait("app.update_refused", 1).await;
        assert!(!marker.exists(), "the marker is removed when the apply ends");

        // A live process named by the marker: this daemon starts no apply until it ends.
        let mut orphan = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
        std::fs::write(&marker, format!("{} 1\n", orphan.id())).unwrap();
        let r2 = rig(vec![key(KEY, &["read", "update"], true)]);
        let (home2, ws2) = (r2.f.engine.ws.home.clone(), r2.f.engine.ws.dir.clone());
        let l2 = delphi_core::spawn::layout(&home2, &ws2).unwrap();
        delphi_core::spawn::ensure_run_dir(&l2.run_dir).unwrap();
        std::fs::write(l2.presence.with_extension("update.lock"), format!("{} 1\n", orphan.id())).unwrap();
        r2.svc.update_check_now().await;
        r2.svc.set_update_presence(l2.clone(), delphi_core::presence::Presence::new(&l2, &"fedcba9876543210".repeat(2), "0.6.5", &ws2, &home2));
        let (_, m) = refused(r2.svc.app_update_apply(KEY, json!({})).await);
        assert!(m.contains("may still be running"), "{m}");
        assert_eq!(r2.applies(), 0);
        let _ = orphan.kill();
        let _ = orphan.wait();
    }

    /// Item 4: a program that has finished its install and left a process holding its output open is not reported as a failure.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_install_that_finished_is_not_a_failure_because_something_it_left_holds_the_pipe() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        std::fs::write(r.rec.join("linger"), "").unwrap();
        let started = std::time::Instant::now();
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let done = r.wait("app.updated", 1).await;
        assert_eq!(done[0]["to"], "99.0.0");
        assert!(started.elapsed() < Duration::from_secs(20), "it did not wait for the leftover process for ever");
        assert!(r.kinds("app.update_refused").is_empty());
        assert_eq!(r.svc.update_facts(), (None, true), "the install happened, so a restart is due");
        let bg: i32 = std::fs::read_to_string(r.rec.join("bgpid")).unwrap().trim().parse().unwrap();
        assert!(unsafe { libc::kill(bg, 0) } != 0, "and the leftover process was ended");
    }

    /// Item 6: a code does not outlive the wall clock either: a request older than ten minutes by the wall clock is stale though the
    /// monotonic clock (which can pause across a sleep) says otherwise.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_request_older_than_ten_minutes_by_the_wall_clock_is_stale() {
        let r = rig(vec![key(KEY, &["read", "update"], false)]);
        r.svc.update_check_now().await;
        let code = r.svc.app_update_apply(KEY, json!({})).await.unwrap()["code"].as_str().unwrap().to_string();
        r.f.now.fetch_add(PENDING_SECS + 1, std::sync::atomic::Ordering::SeqCst);
        let (c, m) = refused(r.svc.app_update_admin("app.update_approve", json!({"code": code})).await.unwrap());
        assert_eq!(c, rpc::FORBIDDEN);
        assert!(m.contains("expired"), "{m}");
        assert_eq!(r.applies(), 0);
    }

    /// Item 9: a panic in the apply task gives the daemon its freedom back, and it counts as a failure.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_panic_in_the_apply_task_does_not_leave_the_daemon_thinking_one_runs() {
        let r = rig(vec![key(KEY, &["read", "update"], true)]);
        r.svc.update_check_now().await;
        r.svc.update.lock().unwrap().panic_in_apply = true;
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while r.svc.update_running() {
            assert!(std::time::Instant::now() < end, "running was never cleared");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(r.applies(), 0);
        r.svc.update.lock().unwrap_or_else(|e| e.into_inner()).panic_in_apply = false;
        let (_, m) = refused(r.svc.app_update_apply(OTHER_KEY_FOR_PANIC, json!({})).await);
        assert!(m.contains("no longer paired"), "{m}");
        let (_, m) = refused(r.svc.app_update_apply(KEY, json!({})).await);
        assert!(m.contains("one update an hour") || m.contains("the next waits"), "it counts as a failed apply: {m}");
    }
    const OTHER_KEY_FOR_PANIC: &str = "app-ffffff";
}
