//! `delphi run --on-card authorization:<id>` (spec 092 S2d1): the places a granted run lives in, and a second look at who is launching it.
//!
//! **A granted run does not run in the operator's home.** The launcher makes a fresh throwaway home (a `mkdtemp` directory, mode 0700,
//! under the canonical `/tmp`), copies into it the workspace the way `delphi train` seals a template home (`train_arms::copy_filtered`:
//! the release and team layers, the configuration and its ratified access blocks; no missions, no state, no memory, no worktrees, no
//! symlinks; an EMPTY operator layer with a valid empty lock, so the daemon there has no operator store, no ledger, no credentials), walks it
//! (no symlink, no other-device mount, no credential-named file) and starts the granted daemon in it. A harness-login endpoint carries
//! no secret in the configuration: the CLI's own login lives outside the Delphi home.
//!
//! The operator's store stays in the operator's home. The launcher reads it; the daemon reaches it only through the grant (its path
//! travels in the payload), to re-verify the record and to take a use from the ledger, as in S2c. The daemon's own `operator/` is the
//! empty one it was given, and the sandboxed command can read neither home.
//!
//! **What survives the run:** the exit code, the summary `delphi run` prints, the `authorization.*` events the launcher echoes to
//! stderr as they happen, the use ledger and the audit events in the operator's home. Everything else (the mission log, the throwaway
//! home, the snapshot) is discarded when this value drops.

use delphi_sandbox::authorized::{FreshHome, Snapshot};
use std::path::{Path, PathBuf};

/// The throwaway home and the disposable snapshot of one granted run. Dropping it discards both.
pub struct GrantedArea {
    pub home: FreshHome,
    /// The workspace directory inside the throwaway home (what the daemon is started with).
    pub ws_dir: PathBuf,
    pub snapshot: Snapshot,
}

/// Why a granted run may not start on this machine, from a probe of the authorized sandbox profile, or `None` (spec 092 TA-2,
/// GOVERNANCE §5.12: "where no such sandbox exists the run does not start"). The launcher probes once before it makes anything; the
/// daemon probes again before every authorized action, the first included. Pure over the probe so each case is tested.
pub fn sandbox_refusal(p: &delphi_sandbox::authorized::AuthorizedProbe) -> Option<String> {
    if p.is_ok() {
        return None;
    }
    Some(format!(
        "refused: no verified OS sandbox with the network off is available here ({}); an authorized run starts only where one exists (macOS today; on Linux and elsewhere the run refuses)",
        p.summary()
    ))
}

/// The fixed root of throwaway homes: the canonical `/tmp` (short enough for a daemon's Unix sockets, and not under the operator's home).
pub fn fixed_root() -> Result<PathBuf, String> {
    std::fs::canonicalize("/tmp").map_err(|e| format!("/tmp: {e}"))
}

/// Build the throwaway home from the operator's workspace and the snapshot of `project`. Any failure refuses the run.
pub fn prepare(operator_ws_dir: &Path, project: &Path) -> Result<GrantedArea, String> {
    let root = fixed_root()?;
    let home = FreshHome::create(&root)?;
    let snapshot = Snapshot::build(project, &root).map_err(|e| format!("the project snapshot: {e}"))?;
    let name = operator_ws_dir.file_name().ok_or("the workspace has no name")?;
    let ws_dir = home.path().join("workspaces").join(name);
    crate::train_arms::copy_filtered(operator_ws_dir, &ws_dir, Path::new(""))?;
    // The configuration of a granted run is REWRITTEN, not copied (see `granted_config`): the project is the snapshot, the tables that reach
    // outside or hold secrets are dropped, and the pinned access blocks are sealed again over what is left, as `delphi train` does.
    let cfg_path = ws_dir.join("config.toml");
    let cfg = std::fs::read_to_string(&cfg_path).map_err(|e| format!("config.toml: {e}"))?;
    let cfg = granted_config(&cfg, snapshot.path())?;
    std::fs::write(&cfg_path, &cfg).map_err(|e| e.to_string())?;
    let blocks = delphi_core::lock::access_blocks(&cfg);
    let ratified = ws_dir.join("local/ratified");
    std::fs::create_dir_all(&ratified).map_err(|e| e.to_string())?;
    delphi_core::lock::write_atomic(&ratified.join("config.access.toml"), blocks.as_bytes()).map_err(|e| e.to_string())?;
    let lock_path = ws_dir.join("local/ratified.lock");
    let mut lk = delphi_core::lock::read(&lock_path).map_err(|e| format!("the workspace has no usable ratified.lock: {e}"))?;
    lk.files.insert("config.toml#access".into(), delphi_core::lock::hash(blocks.as_bytes()));
    delphi_core::lock::write(&lock_path, &lk).map_err(|e| e.to_string())?;
    // The knowledge base is not carried into a granted run.
    let _ = std::fs::remove_dir_all(ws_dir.join("local/kb"));
    // The workspace's project registry (`specs/058`, `local/projects.json`) maps project ids to the ROOT PATHS of every project the operator
    // registered, and `local/projects/<id>/` holds each one's own layer of memory and rules. The daemon reads the registry to decide which
    // project layer applies to a path (`delphi_engine::scope`) and its console views list the names; none of that is wanted in a run whose
    // only project is the disposable snapshot, and the paths are the operator's. The copy is replaced by an EMPTY, valid registry
    // (schema 1: the snapshot is not registered, so the persistent `default` project applies) and the per-project layers are not carried.
    let local = ws_dir.join("local");
    std::fs::create_dir_all(&local).map_err(|e| e.to_string())?;
    std::fs::write(local.join("projects.json"), "{\"schema\":1,\"projects\":{}}\n").map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(local.join("projects"));
    // A clean operator layer: a valid, empty lock and nothing ratified in it (the daemon here has no store and no ledger).
    let op = home.path().join("operator");
    std::fs::create_dir_all(op.join("ratified")).map_err(|e| e.to_string())?;
    delphi_core::lock::write(&op.join("ratified.lock"), &delphi_core::lock::LockFile::default()).map_err(|e| e.to_string())?;
    // The walk, now that everything is in: no symlink, no mount, no credential-named file.
    home.recheck().map_err(|e| format!("the throwaway home does not pass its walk: {e}"))?;
    crate::train_run::sock_fits(home.path())?;
    Ok(GrantedArea { home, ws_dir, snapshot })
}

/// Tables a granted run's configuration does not carry: whatever reaches outside (federation, search, the web, MCP servers, connectors,
/// the relay, scheduled library acquisition, self-update), whatever signs in (`auth`), operator hooks (they run commands), and the
/// retrieval store. What stays is what routing, the sandbox and the roles need: `workspace`, `endpoints`, `access`, `harness`, `exec`,
/// `models`, `agents`, `routing`, `budget`, `hitl`, `events`, `loop`, `grades`, `privacy`, `theme`, `tui`.
const DROPPED_TABLES: &[&str] = &["auth", "connectors", "relay", "search", "library", "hooks", "update", "federation", "mcp", "web", "rag", "kb"];

fn secret_key(k: &str) -> bool {
    let l = k.to_ascii_lowercase();
    ["api_key", "apikey", "token", "secret", "password", "passphrase", "private_key", "bearer", "authorization"].iter().any(|n| l.contains(n))
}

fn scrub(v: &mut toml::Value) {
    match v {
        toml::Value::Table(t) => {
            // A key that names a secret is dropped, except a COUNT of tokens (`max_output_tokens`, `max_tokens`): a number under a name
            // that says `tokens` is a model limit, not a credential, and the daemon refuses a model catalog without it.
            t.retain(|k, v| !secret_key(k) || (k.to_ascii_lowercase().contains("tokens") && (v.is_integer() || v.is_float())));
            for (_, x) in t.iter_mut() {
                scrub(x);
            }
        }
        toml::Value::Array(a) => a.iter_mut().for_each(scrub),
        _ => {}
    }
}

/// The configuration of a granted run's throwaway workspace: the operator's, with the dropped tables and every key that names a secret
/// removed, and `[workspace] projects` pointing at the snapshot only. A configuration that does not parse, or has no `[workspace]`, refuses
/// the run. An endpoint's `credential` is a reference (`harness-login`, a variable name), not a secret, and stays; the variable it names
/// is not in the daemon's environment, so an API endpoint cannot serve a granted run (only harness logins do).
pub fn granted_config(text: &str, snapshot: &Path) -> Result<String, String> {
    let mut v: toml::Value = toml::from_str(text).map_err(|e| format!("config.toml does not parse, so a granted run cannot be set up from it: {e}"))?;
    let t = v.as_table_mut().ok_or("config.toml is not a table")?;
    for k in DROPPED_TABLES {
        t.remove(*k);
    }
    scrub(&mut v);
    let t = v.as_table_mut().ok_or("config.toml is not a table")?;
    let ws = t.get_mut("workspace").and_then(|w| w.as_table_mut()).ok_or("config.toml has no [workspace] table, so the project cannot be pointed at the snapshot")?;
    ws.insert("projects".into(), toml::Value::Array(vec![toml::Value::String(snapshot.to_string_lossy().into_owned())]));
    toml::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// Wait (bounded) for the daemon with `pid` to be gone, so the ledger is quiet when the end of the run is recorded.
pub fn wait_gone(pid: u64, max: std::time::Duration) -> bool {
    if pid <= 1 {
        return true;
    }
    let until = std::time::Instant::now() + max;
    loop {
        // SAFETY: signal 0 only tests for existence.
        if unsafe { libc::kill(pid as i32, 0) } != 0 {
            return true;
        }
        if std::time::Instant::now() >= until {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

// ------------------------------------------------------------------------------------------------ who is launching

/// Programs that are agent seats or the daemon that hosts them: a granted run launched from beneath any of these is refused.
const SEAT_PROGRAMS: &[&str] = &["claude", "agy", "codex", "gemini", "copilot", "delphi-core"];

/// Pure: the first process in an ancestry (each entry a full command line) that is an agent seat or Delphi's daemon, or `None`.
pub fn seat_in_chain(chain: &[String]) -> Option<String> {
    for line in chain {
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or("");
        let base = first.rsplit('/').next().unwrap_or(first);
        let is_daemon = (base == "delphi" || base.starts_with("delphi")) && line.split_whitespace().any(|w| w == "__core" || w == "--daemon");
        if SEAT_PROGRAMS.contains(&base) || is_daemon {
            return Some(base.to_string());
        }
    }
    None
}

/// The command lines of this process's ancestors, nearest first, from `ps`. `None` if the ancestry cannot be read (the caller refuses:
/// a launch that cannot be vetted does not start).
pub fn process_chain() -> Option<Vec<String>> {
    let mut chain = vec![];
    // SAFETY: getppid has no preconditions.
    let mut pid = unsafe { libc::getppid() } as i64;
    for _ in 0..48 {
        if pid <= 1 {
            return Some(chain);
        }
        let out = std::process::Command::new("/bin/ps").args(["-o", "ppid=,command=", "-p", &pid.to_string()]).env_clear().output().ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let (ppid, cmd) = text.split_once(char::is_whitespace)?;
        chain.push(cmd.trim().to_string());
        pid = ppid.trim().parse().ok()?;
    }
    None
}

/// Why the ancestry shows this launch is not the operator's own terminal, or `None`. BEST EFFORT, an extra line behind the launch code
/// ([`launch_step_up`]), which is the launch gate (see the stated keychain limit in specs/092 and GOVERNANCE 5.12): a chain that reaches pid 1 at once (the parent is init, as after a double fork) is
/// suspicious and is not a pass.
pub fn seat_refusal() -> Option<String> {
    match process_chain() {
        None => Some("refused: the process ancestry of this launch could not be read, so it cannot be shown that no agent seat started it".into()),
        Some(chain) => chain_refusal(&chain),
    }
}

/// The pure half of [`seat_refusal`].
pub fn chain_refusal(chain: &[String]) -> Option<String> {
    if chain.is_empty() {
        return Some("refused: this launch has no parent process but init, so it is detached from any terminal session or login; an authorized run is the operator's act from a terminal of their own".into());
    }
    seat_in_chain(chain).map(|p| format!("refused: this launch descends from `{p}`, an agent seat or Delphi's daemon; an authorized run is the operator's act from a terminal of their own"))
}

// ------------------------------------------------------------------------------------------------ the launch code

/// The fresh authenticator code that starts a granted run (spec 092 TA-2, GOVERNANCE §5.12): the same second factor as
/// `delphi test authorize` (`[relay] step_up = "totp"` with a `totp_credential`, in the ratified configuration), verified once per code with
/// wrong guesses rationed (`delphi_core::totp`) and its state shared with the daemon and with `authorize`. It is asked BEFORE the launcher makes
/// anything, so a refused code costs no use and starts no daemon. With no second factor configured the launch refuses; there is no fallback.
/// `factor` gives the secret (or why there is none), `ask` reads one line from the operator. Neither the code nor the secret is returned,
/// logged or kept.
pub fn launch_step_up(ws_dir: &Path, now_unix: i64, factor: &dyn Fn(&Path) -> Result<Vec<u8>, String>, ask: &mut dyn FnMut(&str) -> Option<String>) -> Result<(), String> {
    let secret = factor(ws_dir).map_err(|e| format!("refused: an authorized run needs the second factor at launch, and {e}"))?;
    let Some(code) = ask("authenticator code to launch this authorized run: ") else { return Err("refused: no code given; nothing was launched".into()) };
    let verifier = delphi_core::totp::Verifier::persistent(ws_dir.join("local/totp-state"));
    verifier.check(&secret, code.trim(), now_unix.max(0) as u64).map_err(|e| format!("refused: step-up failed: {e}; nothing was launched"))
}

/// The launch step-up, or why the launch is refused. An unreadable clock refuses up front (fail closed): a code is checked against the time,
/// and a launch with no time is not a launch. The caller makes nothing before this returns `Ok`.
pub fn require_step_up(ws_dir: &Path, now_unix: Option<i64>, factor: &dyn Fn(&Path) -> Result<Vec<u8>, String>, ask: &mut dyn FnMut(&str) -> Option<String>) -> Result<(), String> {
    let Some(now) = now_unix else { return Err("refused: the clock could not be read, so the launch code cannot be checked; nothing was launched".into()) };
    launch_step_up(ws_dir, now, factor, ask)
}

/// What a signal handler needs to put the terminal back: the descriptor and the settings it had. Written before the handlers are installed
/// and read only by them; `tcsetattr`, `signal` and `raise` are async-signal-safe.
static SAVED_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
static mut SAVED_TERMIOS: std::mem::MaybeUninit<libc::termios> = std::mem::MaybeUninit::uninit();
/// The echo-off settings, for the stop handler to put back when the process is continued.
static mut QUIET_TERMIOS: std::mem::MaybeUninit<libc::termios> = std::mem::MaybeUninit::uninit();

/// Ctrl-Z while the code is asked: put the terminal back, stop as the default action would, and when continued hide the echo again and
/// install this handler again.
extern "C" fn restore_stop_and_resume(sig: libc::c_int) {
    let fd = SAVED_FD.load(std::sync::atomic::Ordering::SeqCst);
    // SAFETY: only async-signal-safe calls; the termios statics were written before SAVED_FD was set.
    unsafe {
        if fd >= 0 {
            libc::tcsetattr(fd, libc::TCSANOW, std::ptr::addr_of!(SAVED_TERMIOS).cast::<libc::termios>());
        }
        libc::signal(sig, libc::SIG_DFL);
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, sig);
        libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        libc::raise(sig);
        // continued
        libc::signal(sig, restore_stop_and_resume as *const () as usize);
        if fd >= 0 {
            libc::tcsetattr(fd, libc::TCSAFLUSH, std::ptr::addr_of!(QUIET_TERMIOS).cast::<libc::termios>());
        }
    }
}

extern "C" fn restore_and_die(sig: libc::c_int) {
    let fd = SAVED_FD.load(std::sync::atomic::Ordering::SeqCst);
    if fd >= 0 {
        // SAFETY: SAVED_TERMIOS was fully written before SAVED_FD was set; the calls are async-signal-safe.
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, std::ptr::addr_of!(SAVED_TERMIOS).cast::<libc::termios>()) };
    }
    // SAFETY: back to the default action, then the same signal again, so the process ends as it would have.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// Terminal echo off for the length of the value, back on in `Drop` (every return path, a panic unwinding included), and by a signal handler
/// for SIGINT, SIGTERM, SIGHUP and SIGQUIT (the process then dies as it would have, with the terminal restored). SIGKILL cannot be caught.
struct NoEcho {
    fd: i32,
    old: libc::termios,
    handlers: Vec<(libc::c_int, libc::sigaction)>,
}

impl NoEcho {
    fn on(fd: i32) -> Option<NoEcho> {
        // SAFETY: tcgetattr fills a termios; nothing is read from it unless it returned 0.
        let mut old: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut old) } != 0 {
            return None;
        }
        let mut quiet = old;
        quiet.c_lflag &= !(libc::ECHO | libc::ECHONL);
        quiet.c_lflag |= libc::ICANON;
        // SAFETY: published before the descriptor, so the handler never sees a half-written value.
        unsafe {
            std::ptr::addr_of_mut!(SAVED_TERMIOS).write(std::mem::MaybeUninit::new(old));
            std::ptr::addr_of_mut!(QUIET_TERMIOS).write(std::mem::MaybeUninit::new(quiet));
        }
        SAVED_FD.store(fd, std::sync::atomic::Ordering::SeqCst);
        let mut handlers = vec![];
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT, libc::SIGTSTP] {
            // SAFETY: a plain handler that only calls async-signal-safe functions.
            unsafe {
                let mut act: libc::sigaction = std::mem::zeroed();
                act.sa_sigaction = if sig == libc::SIGTSTP { restore_stop_and_resume as *const () as usize } else { restore_and_die as *const () as usize };
                libc::sigemptyset(&mut act.sa_mask);
                let mut prev: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(sig, &act, &mut prev) != 0 {
                    continue;
                }
                if prev.sa_sigaction == libc::SIG_IGN {
                    // Ignored before (SIGHUP under nohup, say): it stays ignored; nothing of ours is left installed for it.
                    libc::sigaction(sig, &prev, std::ptr::null_mut());
                    continue;
                }
                handlers.push((sig, prev));
            }
        }
        // FLUSH: anything typed before the prompt (a paste) is discarded, not taken as the code.
        // SAFETY: a valid descriptor and termios.
        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &quiet) } != 0 {
            let g = NoEcho { fd, old, handlers };
            drop(g);
            return None;
        }
        Some(NoEcho { fd, old, handlers })
    }
}

impl Drop for NoEcho {
    fn drop(&mut self) {
        // SAFETY: restoring what was read; the descriptor is still open while the guard lives.
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.old);
            for (sig, prev) in &self.handlers {
                libc::sigaction(*sig, prev, std::ptr::null_mut());
            }
        }
        SAVED_FD.store(-1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The prompt and one line read from `tty`, with echo off while it is typed (a code on a shared screen or in a recording is not wanted) and a
/// newline written after it. If echo cannot be turned off the code is NOT asked for: a launch that cannot hide its code is refused.
pub fn ask_on(tty: std::fs::File, prompt: &str) -> Option<String> {
    use std::io::{BufRead, Write};
    use std::os::fd::AsRawFd;
    let guard = NoEcho::on(tty.as_raw_fd())?;
    let mut out = tty.try_clone().ok()?;
    out.write_all(prompt.as_bytes()).ok()?;
    out.flush().ok()?;
    let mut line = String::new();
    let got = std::io::BufReader::new(&tty).read_line(&mut line);
    let _ = out.write_all(b"\n");
    drop(guard);
    match got {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_string()),
    }
}

/// One line typed at the controlling terminal: `/dev/tty` is opened for the prompt and the answer, never stdin, an argument or the environment.
pub fn tty_ask(prompt: &str) -> Option<String> {
    ask_on(std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").ok()?, prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ancestry_with_an_agent_seat_or_the_daemon_is_refused_and_a_plain_shell_is_not() {
        let plain = vec!["-zsh".to_string(), "/Applications/Terminal.app/Contents/MacOS/Terminal".to_string(), "/sbin/launchd".to_string()];
        assert_eq!(seat_in_chain(&plain), None);
        for seat in ["/opt/homebrew/bin/claude --dangerously-skip-permissions", "agy chat", "/usr/local/bin/codex exec", "gemini", "copilot --acp", "/x/delphi-core --daemon --workspace w", "/usr/local/bin/delphi __core --daemon --workspace w"] {
            let chain = vec!["bash -c delphi run".to_string(), seat.to_string(), "-zsh".to_string()];
            assert!(seat_in_chain(&chain).is_some(), "not caught: {seat}");
        }
        // `delphi run` itself and a console are not seats
        assert_eq!(seat_in_chain(&["/usr/local/bin/delphi run --on-card deny".to_string()]), None);
    }

    const SECRET: &[u8] = b"12345678901234567890";

    fn ws() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("local")).unwrap();
        t
    }

    fn now() -> i64 {
        delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap()
    }

    fn good_factor(_: &Path) -> Result<Vec<u8>, String> {
        Ok(SECRET.to_vec())
    }

    // Spec 092 TA-2 / GOVERNANCE §5.12: the launch is the operator's own act because it needs a fresh authenticator code.
    #[test]
    fn a_valid_code_launches_and_the_same_code_a_second_time_is_a_replay() {
        let t = ws();
        let code = delphi_core::totp::code_at(SECRET, now() as u64);
        let mut asked = vec![];
        assert_eq!(launch_step_up(t.path(), now(), &good_factor, &mut |p| { asked.push(p.to_string()); Some(code.clone()) }), Ok(()));
        assert!(asked[0].contains("authenticator code"));
        let again = launch_step_up(t.path(), now(), &good_factor, &mut |_| Some(code.clone())).unwrap_err();
        assert!(again.starts_with("refused:") && again.contains("nothing was launched"), "{again}");
    }

    #[test]
    fn a_wrong_code_no_code_and_a_malformed_code_refuse_the_launch() {
        let t = ws();
        for code in ["000000", "", "12345", "abcdef", "1234567"] {
            let e = launch_step_up(t.path(), now(), &good_factor, &mut |_| Some(code.to_string())).unwrap_err();
            assert!(e.starts_with("refused:"), "{code}: {e}");
        }
        let e = launch_step_up(t.path(), now(), &good_factor, &mut |_| None).unwrap_err();
        assert!(e.contains("no code given"), "{e}");
    }

    #[test]
    fn with_no_second_factor_configured_the_launch_refuses_and_never_asks() {
        let t = ws();
        let mut asked = false;
        let e = launch_step_up(t.path(), now(), &|_| Err("no second factor is configured".to_string()), &mut |_| { asked = true; Some("123456".into()) }).unwrap_err();
        assert!(e.starts_with("refused:") && e.contains("second factor"), "{e}");
        assert!(!asked, "no prompt, no fallback to no step-up");
        // the real reader of the ratified configuration refuses a workspace that has none
        assert!(crate::test_cmd::configured_factor(t.path()).is_err());
    }

    #[test]
    fn wrong_guesses_are_rationed_so_the_right_code_is_refused_after_a_burst_of_wrong_ones() {
        let t = ws();
        for _ in 0..12 {
            let _ = launch_step_up(t.path(), now(), &good_factor, &mut |_| Some("000000".into()));
        }
        let code = delphi_core::totp::code_at(SECRET, now() as u64);
        assert!(launch_step_up(t.path(), now(), &good_factor, &mut |_| Some(code.clone())).is_err(), "locked out after repeated wrong codes");
    }

    #[test]
    fn an_unreadable_clock_refuses_the_launch_before_the_code_is_asked() {
        let t = ws();
        let mut asked = false;
        let e = require_step_up(t.path(), None, &good_factor, &mut |_| { asked = true; Some("123456".into()) }).unwrap_err();
        assert!(e.starts_with("refused:") && e.contains("clock"), "{e}");
        assert!(!asked);
        let code = delphi_core::totp::code_at(SECRET, now() as u64);
        assert_eq!(require_step_up(t.path(), Some(now()), &good_factor, &mut |_| Some(code.clone())), Ok(()));
    }

    // The code is typed without echo and the terminal is put back, on the normal path and when the line cannot be read.
    #[test]
    fn the_code_is_typed_without_echo_and_the_terminal_is_restored_on_every_path() {
        use std::io::{Read, Write};
        use std::os::fd::{AsRawFd, FromRawFd};
        let (mut master, mut slave) = (0, 0);
        // SAFETY: openpty fills two descriptors.
        assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }, 0);
        // SAFETY: both descriptors are ours and each File owns one.
        let (mut master, slave) = unsafe { (std::fs::File::from_raw_fd(master), std::fs::File::from_raw_fd(slave)) };
        let echo = |fd: i32| {
            // SAFETY: tcgetattr on a valid descriptor.
            let mut t: libc::termios = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::tcgetattr(fd, &mut t) }, 0);
            t.c_lflag & libc::ECHO != 0
        };
        let fd = slave.as_raw_fd();
        assert!(echo(fd), "a fresh terminal echoes");
        let keep = slave.try_clone().unwrap();
        let worker = std::thread::spawn(move || ask_on(slave, "code: "));
        // wait for the prompt, which is written after echo is off
        let mut seen = String::new();
        let mut buf = [0u8; 64];
        while !seen.contains("code: ") {
            let n = master.read(&mut buf).unwrap();
            seen.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        assert!(!echo(keep.as_raw_fd()), "echo is off while the code is typed");
        master.write_all(b"424242\n").unwrap();
        assert_eq!(worker.join().unwrap().as_deref(), Some("424242"));
        assert!(echo(keep.as_raw_fd()), "echo is back after the line");
        // what the screen showed: the prompt and a newline, never the digits
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut rest = [0u8; 256];
        // SAFETY: the master is non-blocking for this one read.
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
        let n = master.read(&mut rest).unwrap_or(0);
        // SAFETY: blocking again for the next prompt.
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, 0) };
        let shown = format!("{seen}{}", String::from_utf8_lossy(&rest[..n]));
        assert!(!shown.contains("424242"), "the code was echoed: {shown:?}");
        // an end of input (the other side hung up) is no code, and the terminal is restored all the same
        let again = keep.try_clone().unwrap();
        let w2 = std::thread::spawn(move || ask_on(again, "code: "));
        let mut seen2 = String::new();
        while !seen2.contains("code: ") {
            let n = master.read(&mut buf).unwrap();
            seen2.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        master.write_all(&[0x04]).unwrap();
        assert_eq!(w2.join().unwrap(), None);
        assert!(echo(keep.as_raw_fd()), "echo is back after an end of input");
    }

    // A signal that was ignored before the prompt stays ignored, and every disposition is back as it was afterwards (SIGTSTP included).
    #[test]
    fn the_prompt_leaves_an_ignored_signal_ignored_and_restores_every_disposition() {
        let disposition = |sig: libc::c_int| {
            // SAFETY: reading the current action only.
            let mut cur: libc::sigaction = unsafe { std::mem::zeroed() };
            unsafe { libc::sigaction(sig, std::ptr::null(), &mut cur) };
            cur.sa_sigaction
        };
        let (mut master, mut slave) = (0, 0);
        // SAFETY: openpty fills two descriptors that are closed below.
        assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }, 0);
        // SAFETY: SIGHUP is set to ignore for this test process, as a shell does under nohup.
        let before_hup = unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
        let (int_before, tstp_before) = (disposition(libc::SIGINT), disposition(libc::SIGTSTP));
        let g = NoEcho::on(slave).expect("echo off on a pty");
        assert_eq!(disposition(libc::SIGHUP), libc::SIG_IGN, "an ignored signal stays ignored");
        assert_ne!(disposition(libc::SIGINT), int_before, "SIGINT restores the terminal");
        assert_ne!(disposition(libc::SIGTSTP), tstp_before, "SIGTSTP restores the terminal when the process is stopped");
        assert!(g.handlers.iter().all(|(s, _)| *s != libc::SIGHUP));
        drop(g);
        assert_eq!((disposition(libc::SIGINT), disposition(libc::SIGTSTP), disposition(libc::SIGHUP)), (int_before, tstp_before, libc::SIG_IGN));
        // SAFETY: put the process back as it was.
        unsafe {
            libc::signal(libc::SIGHUP, before_hup);
            libc::close(master);
            libc::close(slave);
        }
    }

    #[test]
    fn a_launch_whose_parent_is_init_is_suspicious_and_not_a_pass() {
        assert!(chain_refusal(&[]).unwrap().contains("detached"));
        assert_eq!(chain_refusal(&["-zsh".to_string(), "login -pf x".to_string()]), None);
        assert!(chain_refusal(&["python3 x.py".to_string(), "claude".to_string()]).unwrap().contains("claude"));
    }

    #[test]
    fn a_run_without_a_verified_sandbox_refuses_to_start_and_linux_always_refuses() {
        use delphi_sandbox::authorized::AuthorizedProbe;
        // nothing proven: refuse, say why, name where a sandbox exists
        let none = sandbox_refusal(&AuthorizedProbe::default()).unwrap();
        assert!(none.starts_with("refused:") && none.contains("macOS today") && none.contains("Linux"), "{none}");
        let off = sandbox_refusal(&AuthorizedProbe { unavailable: Some("forced off (test)".into()), ..AuthorizedProbe::default() }).unwrap();
        assert!(off.contains("forced off (test)"), "{off}");
        // one canary short of ok is still a refusal
        let partly = AuthorizedProbe { mode: Some("test"), network_blocked: true, outside_unreadable: true, snapshot_readonly: true, scratch_noexec: true, interpreter_runs: true, ..AuthorizedProbe::default() };
        assert!(sandbox_refusal(&partly).is_some());
        let ok = AuthorizedProbe { scratch_no_map_exec: true, ..partly };
        assert_eq!(sandbox_refusal(&ok), None);
        // on this machine: a Mac with the sandbox passes the real probe; anything else (Linux included) refuses
        let real = sandbox_refusal(&delphi_sandbox::authorized::probe_authorized());
        if !cfg!(target_os = "macos") {
            assert!(real.is_some(), "Linux and every other platform refuse an authorized run");
        }
    }

    #[test]
    fn the_real_ancestry_of_a_test_process_is_readable() {
        // A test is run by cargo from a shell; the point is that the ancestry can be read and walked to the top.
        assert!(process_chain().is_some());
    }

    #[test]
    fn a_daemon_that_is_gone_is_reported_gone_at_once() {
        assert!(wait_gone(0, std::time::Duration::from_millis(10)));
        let mut c = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = c.id() as u64;
        c.wait().unwrap();
        assert!(wait_gone(pid, std::time::Duration::from_secs(1)));
    }

    #[test]
    fn the_throwaway_home_holds_the_workspace_and_an_empty_operator_layer_and_nothing_else_of_the_operators() {
        let t = tempfile::tempdir().unwrap();
        let src = std::fs::canonicalize(t.path()).unwrap().join("workspaces/w");
        for (p, body) in [("config.toml", RICH_CONFIG), ("local/kb/index.db", "kb"), ("local/projects.json", "{\"schema\":1,\"projects\":{\"p-aaaaaaaa\":{\"project_id\":\"p-aaaaaaaa\",\"root\":\"/srv/operator/secret-project\",\"display_name\":\"secret-project\",\"created_unix\":1}}}"), ("local/projects/p-aaaaaaaa/memory.md", "secret-project lessons"), ("local/missions/m1/log", "secret mission"), ("local/state.db", "x"), ("team/specs/s.md", "x"), ("agents/a.md", "role")] {
            let f = src.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, body).unwrap();
        }
        delphi_core::lock::write(&src.join("local/ratified.lock"), &Default::default()).unwrap();
        // a symlink out of the home is not copied
        std::os::unix::fs::symlink("/etc/hosts", src.join("link")).unwrap();
        let project = t.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("/usr/bin/git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(&project).output().unwrap().status.success());
        git(&["init", "-q"]);
        git(&["remote", "add", "origin", "https://x:ghp_PLANTED_REMOTE_TOKEN@example.com/r.git"]);
        std::fs::write(project.join("a.txt"), "a").unwrap();
        git(&["add", "a.txt"]);
        let area = prepare(&src, &project).unwrap();
        let ws = &area.ws_dir;
        assert!(ws.join("config.toml").is_file() && ws.join("agents/a.md").is_file());
        let cfg = std::fs::read_to_string(ws.join("config.toml")).unwrap();
        assert!(cfg.contains(&format!("projects = [\"{}\"]", area.snapshot.path().display())), "the project is the snapshot, not the real tree: {cfg}");
        assert!(!cfg.contains(project.to_str().unwrap()));
        for gone in ["federation", "connectors", "relay", "[mcp", "search", "auth", "hooks", "library", "SECRET-IN-ENDPOINT", "SECRET-NESTED", "SECRET-WEB"] {
            assert!(!cfg.contains(gone), "{gone} survived in the copied config: {cfg}");
        }
        assert!(cfg.contains("claude-sub") && cfg.contains("harness-login") && cfg.contains("allowed_endpoints"), "what a granted run needs is kept: {cfg}");
        assert!(!ws.join("local/kb").exists(), "the knowledge base is not carried");
        // the operator's project registry and per-project layers are not carried: no operator path is in the throwaway workspace
        let reg = std::fs::read_to_string(ws.join("local/projects.json")).unwrap();
        assert!(!reg.contains("/srv/operator/secret-project") && !reg.contains("secret-project"), "{reg}");
        assert!(!ws.join("local/projects").exists(), "per-project layers are not carried");
        assert_eq!(delphi_engine::scope::registered_ids(ws).unwrap().len(), 0, "an empty registry that still parses");
        assert!(!walkdir_contains(ws, "secret-project"), "an operator project path survived somewhere in the throwaway workspace");
        // the pinned access blocks are sealed over what is left, so the daemon loads the copy without a prompt
        let lock = delphi_core::lock::read(&ws.join("local/ratified.lock")).unwrap();
        assert_eq!(lock.files.get("config.toml#access"), Some(&delphi_core::lock::hash(delphi_core::lock::access_blocks(&cfg).as_bytes())));
        // the snapshot carries no git internals and none of the project's remote credentials
        assert!(!area.snapshot.path().join(".git").exists());
        let leaked = walkdir_contains(area.snapshot.path(), "ghp_PLANTED_REMOTE_TOKEN");
        assert!(!leaked, "a token from .git/config reached the snapshot");
        assert!(!ws.join("local/missions").exists() && !ws.join("local/state.db").exists() && !ws.join("team/specs").exists() && !ws.join("link").exists());
        assert!(area.home.path().join("operator/ratified.lock").is_file());
        assert!(!area.home.path().join(concat!("operator/authoriza", "tions.json")).exists() && !area.home.path().join("operator/uses").exists() && !area.home.path().join("operator/audit").exists());
        assert!(area.snapshot.path().join("a.txt").is_file());
        let (home, snap) = (area.home.path().to_path_buf(), area.snapshot.root().to_path_buf());
        drop(area);
        assert!(!home.exists() && !snap.exists(), "discarded with the run");
    }

    const RICH_CONFIG: &str = "[workspace]\nprojects = [\"/real/project\"]\n\n[[endpoints]]\nid = \"claude-sub\"\nfamily = \"claude\"\npath = \"harness\"\nharness = \"claude\"\ncredential = \"harness-login\"\napi_key = \"SECRET-IN-ENDPOINT\"\n\n[access]\nallowed_endpoints = [\"claude-sub\"]\n\n[federation]\npeers = [\"x\"]\n\n[connectors.slack]\ntoken = \"SECRET-NESTED\"\n\n[relay]\nurl = \"https://r\"\n\n[mcp.srv]\ncommand = \"x\"\n\n[search]\nprovider = \"brave\"\n\n[auth.github]\nclient = \"c\"\n\n[hooks]\nx = \"y\"\n\n[library]\nschedule = \"daily\"\n\n[web]\nbearer = \"SECRET-WEB\"\n\n[exec]\nsandbox = \"required\"\n";

    fn walkdir_contains(root: &Path, needle: &str) -> bool {
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if std::fs::read_to_string(&p).is_ok_and(|t| t.contains(needle)) {
                    return true;
                }
            }
        }
        false
    }

    // Found by the first run of the real binary (CI): a key containing `token` was dropped as a secret, which removed `max_output_tokens`
    // from the model catalog and the granted daemon refused its own configuration.
    #[test]
    fn the_models_token_limits_survive_the_rewrite_and_the_daemon_can_still_read_what_is_left() {
        let cfg = format!(
            "{RICH_CONFIG}\n[models.claude]\nfast = \"haiku-t\"\nworkhorse = \"haiku-t\"\nfrontier = \"haiku-t\"\n[models.catalog.\"haiku-t\"]\nmax_output_tokens = 8000\nmin_thinking_budget = 1024\nmax_thinking_budget = 32000\ninput_usd_per_mtok = 1.0\noutput_usd_per_mtok = 5.0\nbearer_token = \"SECRET-TOKEN-KEY\"\naccess_token = \"SECRET-ACCESS\"\n"
        );
        let out = granted_config(&cfg, Path::new("/snap")).unwrap();
        assert!(out.contains("max_output_tokens = 8000") && out.contains("max_thinking_budget"), "{out}");
        assert!(!out.contains("SECRET-TOKEN-KEY") && !out.contains("SECRET-ACCESS") && !out.contains("bearer_token") && !out.contains("access_token"), "{out}");
        // the same parser the daemon uses reads what is left
        if let Err(problems) = delphi_adapter::config::Config::parse(&out) {
            panic!("the rewritten configuration does not parse: {problems:?}");
        }
        // a token that is a TEXT under a `tokens` name is still dropped
        let out = granted_config("[workspace]\nprojects = []\n[x]\ntokens = \"SECRET-LIST\"\nmax_tokens = 100\n", Path::new("/s")).unwrap();
        assert!(!out.contains("SECRET-LIST") && out.contains("max_tokens = 100"), "{out}");
    }

    #[test]
    fn a_configuration_that_does_not_parse_or_has_no_workspace_refuses_the_run() {
        assert!(granted_config("this is [not toml", Path::new("/s")).unwrap_err().contains("does not parse"));
        assert!(granted_config("[access]\nx = 1\n", Path::new("/s")).unwrap_err().contains("no [workspace]"));
        let ok = granted_config(RICH_CONFIG, Path::new("/snap")).unwrap();
        assert!(ok.contains("/snap") && !ok.contains("/real/project"));
    }

    #[test]
    fn a_credential_named_file_in_the_workspace_refuses_the_run() {
        let t = tempfile::tempdir().unwrap();
        let src = std::fs::canonicalize(t.path()).unwrap().join("workspaces/w");
        std::fs::create_dir_all(src.join("local")).unwrap();
        std::fs::write(src.join("config.toml"), RICH_CONFIG).unwrap();
        delphi_core::lock::write(&src.join("local/ratified.lock"), &Default::default()).unwrap();
        std::fs::write(src.join("local/auth.json"), "token").unwrap();
        let project = t.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let run = |args: &[&str]| assert!(std::process::Command::new("/usr/bin/git").args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(&project).output().unwrap().status.success());
        run(&["init", "-q"]);
        std::fs::write(project.join("a.txt"), "a").unwrap();
        run(&["add", "a.txt"]);
        let err = prepare(&src, &project).err().unwrap();
        assert!(err.contains("credential-named"), "{err}");
    }
}
