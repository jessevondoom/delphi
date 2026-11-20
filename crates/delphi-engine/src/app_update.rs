//! The `update` grant (`specs/120` §13.2): one method, `app.update_apply`, which takes no parameter, and the daemon's own knowledge of
//! whether a newer release exists.
//!
//! * **Availability** (item 12). At start and every six hours the daemon starts `<delphi> update --check --home <home>` (clause 2, read
//!   only), parses the one line that command prints for an update, and keeps the version as `update_available`. It, and
//!   `restart_pending`, go into the presence file as optional fields.
//! * **A request** (items 2, 5, 7). A key that holds `update` asks. The daemon, not the app, says what the release is. Without the key's
//!   standing `--auto` the request is **pending**, one at a time, for ten minutes, and nothing changes until the operator, at a
//!   terminal, types the version the daemon found (`app.update_approve`). A version no newer than the one installed, a second apply
//!   within the hour for the key, an apply while one runs: each is refused, by name.
//! * **The apply** (items 3, 4, 8). The daemon starts the program it is running as with `update --no-restart --workspace <name> --home
//!   <home>` (clause 1): detached from any terminal, standard input closed, a private temporary directory, an environment of `HOME`,
//!   `PATH`, `DELPHI_HOME` and `TMPDIR` only, output kept to a bounded buffer. Nothing in the command comes from the app. The daemon
//!   does not restart itself; it records `restart_pending`.
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
/// What is kept of a child's output, per stream. The rest is read and thrown away so the child never blocks on a full pipe.
pub const OUT_MAX: usize = 64 * 1024;
/// How long an apply, and a check, may take before the child is killed.
const APPLY_MAX: Duration = Duration::from_secs(15 * 60);
const CHECK_MAX: Duration = Duration::from_secs(60);

/// The two ways the daemon starts its own program for this grant. Each is a clause of the spawn-site list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clause {
    /// Clause 1: install the release, leaving running daemons on the old code.
    Apply,
    /// Clause 2: ask whether there is a newer one; changes nothing.
    Check,
}

/// A workspace's name as it may stand in a command: plain, and never an option.
pub fn plain_name(n: &str) -> bool {
    let mut b = n.bytes();
    n.len() <= 64 && b.next().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// The arguments of clause 1. Made from the daemon's own workspace name and home, and from nothing an app sent.
pub fn apply_argv(workspace: &str, home: &Path) -> Vec<OsString> {
    vec!["update".into(), "--no-restart".into(), "--workspace".into(), workspace.into(), "--home".into(), home.as_os_str().to_owned()]
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
    issued: i64,
    wrong: u8,
}

/// What the daemon knows about updates. In memory: a daemon that restarts forgets the hour's count, as the other limits do.
#[derive(Default)]
pub struct State {
    available: Option<String>,
    pending: Option<Pending>,
    running: bool,
    last_apply: HashMap<String, i64>,
    restart_pending: bool,
    made: u64,
    /// The program to start instead of this one (tests: a script that records what it was asked and answers as it is told).
    exe: Option<PathBuf>,
    /// Where the presence file is, and what it said when the daemon wrote it: the update fields are added to it and it is rewritten.
    presence: Option<(delphi_core::spawn::Layout, delphi_core::presence::Presence)>,
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

/// Read a stream: the first [`OUT_MAX`] bytes kept, the rest read and dropped.
async fn read_bounded<R: tokio::io::AsyncRead + Unpin>(mut r: R) -> String {
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let _ = (&mut r).take(OUT_MAX as u64).read_to_end(&mut kept).await;
    let _ = tokio::io::copy(&mut r, &mut tokio::io::sink()).await;
    String::from_utf8_lossy(&kept).into_owned()
}

/// Is `exe` something the daemon may start as the installed program: an absolute path to a regular file named `delphi`, owned by this
/// user, that no group or other can write? (The daemon runs as `<delphi> __core`, so this is the file it was started from.)
pub fn is_installed_program(exe: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if !exe.is_absolute() || exe.file_name().and_then(|n| n.to_str()) != Some("delphi") {
        return Err("this daemon is not running as an installed `delphi`".into());
    }
    let m = std::fs::metadata(exe).map_err(|e| format!("the installed program cannot be read: {e}"))?;
    // SAFETY: geteuid has no preconditions.
    if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 {
        return Err("the installed program is not a file only this user can change".into());
    }
    Ok(())
}

impl EngineService {
    /// Start `Clause`'s command: the program this daemon is running as, with the clause's own arguments. The one process start of this
    /// grant (`crates/delphi/tests/spawn_sites.toml`, class `self`).
    pub async fn update_run(&self, clause: Clause) -> Result<Ran, String> {
        let exe = self.update_exe()?;
        let home = self.engine.ws.home.clone();
        let (argv, limit) = match clause {
            Clause::Apply => {
                let name = self.engine.ws.dir.file_name().and_then(|n| n.to_str()).filter(|n| plain_name(n)).ok_or("this workspace's name is not a plain one")?.to_string();
                (apply_argv(&name, &home), APPLY_MAX)
            }
            Clause::Check => (check_argv(&home), CHECK_MAX),
        };
        let tmp = private_dir().map_err(|e| format!("no private folder for the update: {e}"))?;
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.args(&argv).env_clear().env("PATH", "/usr/bin:/bin").env("DELPHI_HOME", &home).env("TMPDIR", &tmp).current_dir(&tmp).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        if let Some(h) = std::env::var_os("HOME") {
            cmd.env("HOME", h);
        }
        // SAFETY: setsid runs between fork and exec and touches nothing of this process: the child has no controlling terminal.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let started = cmd.spawn();
        let out = match started {
            Err(e) => Err(format!("the program could not be started: {e}")),
            Ok(mut child) => {
                let (so, se) = (child.stdout.take(), child.stderr.take());
                let reading = async {
                    let (a, b) = tokio::join!(async { if let Some(s) = so { read_bounded(s).await } else { String::new() } }, async { if let Some(s) = se { read_bounded(s).await } else { String::new() } });
                    (a, b)
                };
                let finished = tokio::time::timeout(limit, async {
                    let (outs, status) = tokio::join!(reading, child.wait());
                    (outs, status)
                })
                .await;
                match finished {
                    Ok(((stdout, stderr), Ok(status))) => Ok(Ran { code: status.code(), stdout, stderr }),
                    Ok((_, Err(e))) => Err(format!("the program could not be waited for: {e}")),
                    Err(_) => {
                        let _ = child.kill().await;
                        Err("the program did not finish in time and was stopped".into())
                    }
                }
            }
        };
        let _ = std::fs::remove_dir_all(&tmp);
        out
    }

    /// The program to start: the one this daemon runs as, checked to be the installed `delphi` (tests give their own).
    fn update_exe(&self) -> Result<PathBuf, String> {
        let exe = match self.update.lock().unwrap().exe.clone() {
            Some(e) => e,
            None => std::env::current_exe().and_then(std::fs::canonicalize).map_err(|e| format!("this daemon's program cannot be found: {e}"))?,
        };
        is_installed_program(&exe)?;
        Ok(exe)
    }

    /// Tests: start this program instead of the current executable.
    pub fn set_update_exe(&self, exe: PathBuf) {
        self.update.lock().unwrap().exe = Some(exe);
    }

    /// Where the presence file is and what it held, so that a change to `update_available` or `restart_pending` rewrites it.
    pub fn set_update_presence(&self, layout: delphi_core::spawn::Layout, base: delphi_core::presence::Presence) {
        self.update.lock().unwrap().presence = Some((layout, base));
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

    /// Rewrite the presence file with the two facts, when the daemon has one to rewrite.
    fn update_publish(&self) {
        let (facts, base) = {
            let st = self.update.lock().unwrap();
            let Some((layout, base)) = st.presence.clone() else { return };
            ((st.available.clone(), st.restart_pending), (layout, base))
        };
        let (layout, mut p) = base;
        (p.update_available, p.restart_pending) = facts;
        if let Err(e) = delphi_core::presence::write(&layout, &p) {
            delphi_core::elog::warn("daemon", "presence_update", "the presence file could not be rewritten", json!({"error": e.to_string()}));
        }
    }

    /// Look for a newer release now (clause 2) and keep what was found. A check that fails, or prints something that is not an update,
    /// leaves the daemon knowing no release.
    pub async fn update_check_now(&self) {
        let found = match self.update_run(Clause::Check).await {
            Ok(r) if r.code == Some(0) => parse_check(&r.stdout, Self::installed_version()),
            _ => None,
        };
        let changed = {
            let mut st = self.update.lock().unwrap();
            let changed = st.available != found;
            st.available = found;
            changed
        };
        if changed {
            self.update_publish();
        }
    }

    /// The daemon's own look for a release: at start, then every six hours. Called once, by the daemon's boot, and by no test.
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

    /// The `app.request` event of an update request, in the states of [`crate::app_requests::STATES`].
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
        let verdict = {
            let mut st = self.update.lock().unwrap();
            if st.pending.as_ref().is_some_and(|p| now - p.issued > PENDING_SECS) {
                st.pending = None;
            }
            if st.running {
                Err(("update_running", "an update is already running".to_string()))
            } else if let Some(at) = st.last_apply.get(key).filter(|at| now - **at < APPLY_EVERY_SECS) {
                Err(("one_an_hour", format!("one update an hour for one key; this key's last was started {} minutes ago", (now - *at) / 60)))
            } else if st.restart_pending {
                Err(("restart_pending", "an update was applied and this daemon has not been restarted since; restart it first".to_string()))
            } else if let Some(v) = st.available.clone() {
                if !newer(&v, Self::installed_version()) {
                    Err(("not_newer", "the release found is not newer than the one installed".to_string()))
                } else if let Some(p) = st.pending.as_ref() {
                    if p.key == key {
                        Ok((v, Some((p.request.clone(), p.issued))))
                    } else {
                        Err(("another_pending", "another update request is waiting for approval at the terminal".to_string()))
                    }
                } else {
                    Ok((v, None))
                }
            } else {
                Err(("none_available", "no newer stable release is known; the daemon looks at start and every 6 hours".to_string()))
            }
        };
        let (version, waiting) = match verdict {
            Ok(v) => v,
            Err((reason, text)) => return self.update_refuse(key, &request, reason, text).await,
        };
        if let Some((request, issued)) = waiting {
            // The same key asking again while its request waits: the same answer, and the clock is not extended.
            return Ok(json!({"request": request, "state": "waiting", "version": version, "expires_in": (PENDING_SECS - (now - issued)).max(0)}));
        }
        if auto {
            return self.update_begin(key, &version, &request).await.map(|_| json!({"request": request, "state": "running", "version": version}));
        }
        self.update.lock().unwrap().pending = Some(Pending { key: key.to_string(), version: version.clone(), request: request.clone(), issued: now, wrong: 0 });
        self.update_request_event(key, &request, "waiting", None).await;
        Ok(json!({"request": request, "state": "waiting", "version": version, "expires_in": PENDING_SECS}))
    }

    /// Start the apply for `key`: the checks that must hold at the moment it starts, the child, and its outcome in a task of its own.
    async fn update_begin(self: &Arc<Self>, key: &str, version: &str, request: &str) -> Result<(), delphi_core::proto::RpcError> {
        let now = (self.engine.clock_unix)();
        // The key must still hold the grant, and the program must still be the installed one, now that it is time to go.
        let still = self.update_key_state(key).is_some_and(|(holds, _)| holds);
        let ready = if still { self.update_exe().map(|_| ()) } else { Err("this key no longer holds `update`".to_string()) };
        if let Err(text) = ready {
            return self.update_refuse(key, request, if still { "not_installed" } else { "no_grant" }, text).await.map(|_| ());
        }
        let verdict = {
            let mut st = self.update.lock().unwrap();
            if st.running {
                Some(("update_running", "an update is already running"))
            } else if st.last_apply.get(key).is_some_and(|at| now - *at < APPLY_EVERY_SECS) {
                Some(("one_an_hour", "one update an hour for one key"))
            } else if !st.available.as_deref().is_some_and(|a| a == version && newer(a, Self::installed_version())) {
                Some(("not_newer", "the release found is not newer than the one installed"))
            } else {
                st.running = true;
                st.last_apply.insert(key.to_string(), now);
                None
            }
        };
        if let Some((reason, text)) = verdict {
            return self.update_refuse(key, request, reason, text.to_string()).await.map(|_| ());
        }
        self.update_request_event(key, request, "running", None).await;
        let (svc, key, version, request) = (self.clone(), key.to_string(), version.to_string(), request.to_string());
        tokio::spawn(async move {
            let ran = svc.update_run(Clause::Apply).await;
            svc.update_finish(&key, &version, &request, ran).await;
        });
        Ok(())
    }

    /// The exit code of the apply, as events: `app.updated` for an install, `app.update_refused` for anything else.
    async fn update_finish(&self, key: &str, version: &str, request: &str, ran: Result<Ran, String>) {
        let from = Self::installed_version();
        let outcome: Result<String, String> = match ran {
            Err(e) => Err(e),
            Ok(r) if r.code == Some(0) => parse_installed(&r.stdout).ok_or_else(|| "the program finished without installing: it says it is already the newest".to_string()),
            Ok(r) => Err(format!("the update did not complete (exit {}): {}", r.code.map_or("signal".to_string(), |c| c.to_string()), one_line(&r.stderr))),
        };
        match outcome {
            Ok(to) => {
                {
                    let mut st = self.update.lock().unwrap();
                    st.running = false;
                    st.restart_pending = true;
                    st.available = None;
                }
                self.update_publish();
                let to = if parse_release(&to).is_some() { to } else { version.to_string() };
                self.update_event(crate::app_audit::UPDATED_EVENT, json!({"name": key, "from": from, "to": to, "restart_pending": true})).await;
                self.update_request_event(key, request, "done", None).await;
            }
            Err(text) => {
                self.update.lock().unwrap().running = false;
                self.update_event(crate::app_audit::UPDATE_REFUSED_EVENT, json!({"name": key, "reason": "update_failed", "why": one_line(&text)})).await;
                self.update_request_event(key, request, "failed", Some(&text)).await;
            }
        }
    }

    /// `app.update_pending` and `app.update_approve` (item 5), for the server, which has checked the caller's scope and terminal.
    pub async fn app_update_admin(self: &Arc<Self>, method: &str, params: Value) -> Option<RpcOutcome> {
        let now = (self.engine.clock_unix)();
        match method {
            // What waits, with the release the daemon found: never anything the app said.
            "app.update_pending" => {
                let mut st = self.update.lock().unwrap();
                if st.pending.as_ref().is_some_and(|p| now - p.issued > PENDING_SECS) {
                    st.pending = None;
                }
                let rows: Vec<Value> = st.pending.iter().map(|p| json!({"request_id": p.request, "key": p.key, "version": p.version, "installed": Self::installed_version(), "age_secs": now - p.issued, "expires_in": (PENDING_SECS - (now - p.issued)).max(0)})).collect();
                Some(Ok(json!({"pending": rows, "update_available": st.available, "restart_pending": st.restart_pending})))
            }
            "app.update_approve" => Some(self.update_approve(params, now).await),
            _ => None,
        }
    }

    /// The operator typed a version. Only the version of the request that waits approves it; Enter alone, or any other string, approves
    /// nothing, and three wrong ones cancel the request.
    async fn update_approve(self: &Arc<Self>, params: Value, now: i64) -> RpcOutcome {
        enum Verdict {
            Nothing,
            Expired(String, String),
            Wrong { key: String, request: String, cancelled: bool },
            Go(Pending),
        }
        let typed = params["version"].as_str().unwrap_or("").trim().to_string();
        // Decided under the lock, with nothing awaited there.
        let verdict = {
            let mut st = self.update.lock().unwrap();
            match st.pending.as_mut() {
                None => Verdict::Nothing,
                Some(p) if now - p.issued > PENDING_SECS => {
                    let v = Verdict::Expired(p.key.clone(), p.request.clone());
                    st.pending = None;
                    v
                }
                Some(p) if typed.strip_prefix('v').unwrap_or(&typed).is_empty() || typed.strip_prefix('v').unwrap_or(&typed) != p.version => {
                    p.wrong += 1;
                    let v = Verdict::Wrong { key: p.key.clone(), request: p.request.clone(), cancelled: p.wrong >= MAX_WRONG };
                    if p.wrong >= MAX_WRONG {
                        st.pending = None;
                    }
                    v
                }
                Some(_) => Verdict::Go(st.pending.take().unwrap()),
            }
        };
        match verdict {
            Verdict::Nothing => Err(err(code::NOT_FOUND, "no update is waiting for approval")),
            Verdict::Expired(key, request) => {
                self.update_request_event(&key, &request, "refused", Some("not approved in 10 minutes")).await;
                self.update_event(crate::app_audit::UPDATE_REFUSED_EVENT, json!({"name": key, "reason": "expired_request"})).await;
                Err(err(code::FORBIDDEN, "that update request has expired; the app asks again"))
            }
            Verdict::Wrong { key, request, cancelled } => {
                self.update_event(crate::app_audit::UPDATE_REFUSED_EVENT, json!({"name": key, "reason": if cancelled { "wrong_version_limit" } else { "wrong_version" }})).await;
                if cancelled {
                    self.update_request_event(&key, &request, "refused", Some("the wrong version was typed 3 times")).await;
                }
                Err(err(code::FORBIDDEN, if cancelled { format!("that is not the version, {MAX_WRONG} times: the request is cancelled and the app asks again") } else { "that is not the version the daemon found".to_string() }))
            }
            Verdict::Go(taken) => {
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
        assert_eq!(apply_argv("default", home), ["update", "--no-restart", "--workspace", "default", "--home", "/h/.delphi"].map(OsString::from));
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
        assert!(is_installed_program(&ok).is_ok());
        std::fs::set_permissions(&ok, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(is_installed_program(&ok).is_err(), "group-writable");
        let other = t.path().join("not-delphi");
        std::fs::write(&other, "#!/bin/sh\n").unwrap();
        assert!(is_installed_program(&other).is_err(), "another name");
        assert!(is_installed_program(Path::new("delphi")).is_err() && is_installed_program(&t.path().join("nope/delphi")).is_err());
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
            "#!/bin/sh\nrec='{}'\n{{ echo \"ARGV $*\"; echo \"PWD $(pwd)\"; echo \"TMP $TMPDIR\"; echo \"ENV $(env | cut -d= -f1 | sort | tr '\\n' ' ')\"; echo \"PATH $PATH\"; if [ -t 0 ]; then echo 'STDIN tty'; else echo 'STDIN none'; fi; }} >> \"$rec/calls\"\ncase \"$*\" in\n  *--check*) cat \"$rec/check.out\"; exit 0 ;;\n  *) cat \"$rec/apply.out\"; exit \"$(cat \"$rec/apply.code\")\" ;;\nesac\n",
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
            self.f.now.fetch_add(secs, std::sync::atomic::Ordering::SeqCst);
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

    /// AC-17: without `--auto`, the call makes a pending request naming the release the daemon found, and changes nothing. Only the
    /// version typed approves it: Enter, a wrong string and the third wrong string do not.
    #[tokio::test(flavor = "multi_thread")]
    async fn without_auto_the_call_waits_for_the_version_typed_at_a_terminal() {
        let r = rig(vec![key(KEY, &["read", "update"], false), key(OTHER, &["read", "update"], false)]);
        r.svc.update_check_now().await;
        assert_eq!(r.calls().iter().filter(|l| l.starts_with("ARGV")).count(), 1, "one check, no apply");
        let asked = r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        assert_eq!((asked["state"].as_str(), asked["version"].as_str(), asked["expires_in"].as_i64()), (Some("waiting"), Some("99.0.0"), Some(PENDING_SECS)));
        assert_eq!(r.states(), ["waiting"]);
        assert!(r.kinds("app.updated").is_empty());
        // The same key again: the same request. Another key: refused, one at a time.
        let again = r.svc.app_update_apply(KEY, json!(null)).await.unwrap();
        assert_eq!((again["request"].clone(), again["state"].as_str()), (asked["request"].clone(), Some("waiting")));
        let (c, m) = refused(r.svc.app_update_apply(OTHER, json!({})).await);
        assert_eq!(c, rpc::FORBIDDEN);
        assert!(m.contains("another update request is waiting"), "{m}");
        // What the console sees: the release the DAEMON found, never anything the app said.
        let listed = r.svc.app_update_admin("app.update_pending", json!({})).await.unwrap().unwrap();
        assert_eq!((listed["pending"][0]["version"].as_str(), listed["pending"][0]["key"].as_str(), listed["pending"][0]["installed"].as_str()), (Some("99.0.0"), Some(KEY), Some(env!("CARGO_PKG_VERSION"))));
        // Enter and a wrong version: refused, nothing installed.
        for wrong in ["", "0.0.1"] {
            let (c, m) = refused(r.svc.app_update_admin("app.update_approve", json!({"version": wrong})).await.unwrap());
            assert_eq!((c, m.as_str()), (rpc::FORBIDDEN, "that is not the version the daemon found"), "{wrong:?}");
        }
        assert_eq!(r.applies(), 0);
        // The third wrong string cancels the request: the right version after it finds nothing waiting.
        let (c, m) = refused(r.svc.app_update_admin("app.update_approve", json!({"version": "latest"})).await.unwrap());
        assert!(c == rpc::FORBIDDEN && m.contains("cancelled"), "{m}");
        assert_eq!(refused(r.svc.app_update_admin("app.update_approve", json!({"version": "99.0.0"})).await.unwrap()).0, rpc::NOT_FOUND);
        assert!(r.kinds("app.update_refused").iter().any(|p| p["reason"] == "wrong_version_limit"));
        assert_eq!(r.applies(), 0);

        // A new request, then the version typed (a leading `v` is the same version).
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        let approved = r.svc.app_update_admin("app.update_approve", json!({"version": "v99.0.0"})).await.unwrap().unwrap();
        assert_eq!((approved["approved"].as_bool(), approved["version"].as_str(), approved["name"].as_str()), (Some(true), Some("99.0.0"), Some(KEY)));
        let done = r.wait("app.updated", 1).await;
        assert_eq!((done[0]["name"].as_str(), done[0]["from"].as_str(), done[0]["to"].as_str(), done[0]["restart_pending"].as_bool()), (Some(KEY), Some(env!("CARGO_PKG_VERSION")), Some("99.0.0"), Some(true)));
        r.wait("app.request", 1).await;
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while r.states().last().map(String::as_str) != Some("done") {
            assert!(std::time::Instant::now() < end, "{:?}", r.states());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(r.states(), ["waiting", "refused", "refused", "waiting", "running", "done"], "waiting; the other key refused; the request cancelled; asked again, approved, installed");
        assert_eq!(r.applies(), 1);
    }

    /// A request that is not approved in ten minutes is dropped, and the version typed afterwards approves nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_request_that_is_not_approved_in_ten_minutes_changes_nothing() {
        let r = rig(vec![key(KEY, &["read", "update"], false)]);
        r.svc.update_check_now().await;
        r.svc.app_update_apply(KEY, json!({})).await.unwrap();
        r.advance(PENDING_SECS + 1);
        let (c, m) = refused(r.svc.app_update_admin("app.update_approve", json!({"version": "99.0.0"})).await.unwrap());
        assert_eq!(c, rpc::FORBIDDEN);
        assert!(m.contains("expired"), "{m}");
        assert_eq!(r.applies(), 0);
        assert!(r.kinds("app.update_refused").iter().any(|p| p["reason"] == "expired_request"));
        assert_eq!(refused(r.svc.app_update_admin("app.update_approve", json!({"version": "99.0.0"})).await.unwrap()).0, rpc::NOT_FOUND);
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
        assert_eq!(apply[0], &format!("ARGV update --no-restart --workspace {name} --home {home}"));
        let words = |argv: Vec<OsString>| argv.iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        assert_eq!(apply[0], &format!("ARGV {}", words(apply_argv(&name, &r.f.engine.ws.home))));
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
        r.svc.app_update_apply(OTHER, json!({})).await.unwrap();
        KeyStore::at(&r.f.engine.ws.home).save(&[key(OTHER, &["read"], false)]).unwrap();
        let (_, m) = refused(r.svc.app_update_admin("app.update_approve", json!({"version": "99.0.0"})).await.unwrap());
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
}
