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

/// The fixed root of throwaway homes: the canonical `/tmp` (short enough for a daemon's Unix sockets, and not under the operator's home).
pub fn fixed_root() -> Result<PathBuf, String> {
    std::fs::canonicalize("/tmp").map_err(|e| format!("/tmp: {e}"))
}

/// Build the throwaway home from the operator's workspace and the snapshot of `project`. Any failure refuses the run.
pub fn prepare(operator_ws_dir: &Path, project: &Path) -> Result<GrantedArea, String> {
    let root = fixed_root()?;
    let home = FreshHome::create(&root)?;
    let name = operator_ws_dir.file_name().ok_or("the workspace has no name")?;
    let ws_dir = home.path().join("workspaces").join(name);
    crate::train_arms::copy_filtered(operator_ws_dir, &ws_dir, Path::new(""))?;
    // A clean operator layer: a valid, empty lock and nothing ratified in it (the daemon here has no store and no ledger).
    let op = home.path().join("operator");
    std::fs::create_dir_all(op.join("ratified")).map_err(|e| e.to_string())?;
    delphi_core::lock::write(&op.join("ratified.lock"), &delphi_core::lock::LockFile::default()).map_err(|e| e.to_string())?;
    // The walk, now that everything is in: no symlink, no mount, no credential-named file.
    home.recheck().map_err(|e| format!("the throwaway home does not pass its walk: {e}"))?;
    crate::train_run::sock_fits(home.path())?;
    let snapshot = Snapshot::build(project, &root).map_err(|e| format!("the project snapshot: {e}"))?;
    Ok(GrantedArea { home, ws_dir, snapshot })
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

/// Why the ancestry shows this launch is not the operator's own terminal, or `None`.
pub fn seat_refusal() -> Option<String> {
    match process_chain() {
        None => Some("refused: the process ancestry of this launch could not be read, so it cannot be shown that no agent seat started it".into()),
        Some(chain) => seat_in_chain(&chain).map(|p| format!("refused: this launch descends from `{p}`, an agent seat or Delphi's daemon; an authorized run is the operator's act from a terminal of their own")),
    }
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
        for (p, body) in [("config.toml", "[workspace]\nprojects = []\n"), ("local/ratified.lock", ""), ("local/missions/m1/log", "secret mission"), ("local/state.db", "x"), ("team/specs/s.md", "x"), ("agents/a.md", "role")] {
            let f = src.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, body).unwrap();
        }
        // a symlink out of the home is not copied
        std::os::unix::fs::symlink("/etc/hosts", src.join("link")).unwrap();
        let project = t.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("/usr/bin/git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(&project).output().unwrap().status.success());
        git(&["init", "-q"]);
        std::fs::write(project.join("a.txt"), "a").unwrap();
        git(&["add", "a.txt"]);
        let area = prepare(&src, &project).unwrap();
        let ws = &area.ws_dir;
        assert!(ws.join("config.toml").is_file() && ws.join("agents/a.md").is_file());
        assert!(!ws.join("local/missions").exists() && !ws.join("local/state.db").exists() && !ws.join("team/specs").exists() && !ws.join("link").exists());
        assert!(area.home.path().join("operator/ratified.lock").is_file());
        assert!(!area.home.path().join(concat!("operator/authoriza", "tions.json")).exists() && !area.home.path().join("operator/uses").exists() && !area.home.path().join("operator/audit").exists());
        assert!(area.snapshot.path().join("a.txt").is_file());
        let (home, snap) = (area.home.path().to_path_buf(), area.snapshot.root().to_path_buf());
        drop(area);
        assert!(!home.exists() && !snap.exists(), "discarded with the run");
    }

    #[test]
    fn a_credential_named_file_in_the_workspace_refuses_the_run() {
        let t = tempfile::tempdir().unwrap();
        let src = std::fs::canonicalize(t.path()).unwrap().join("workspaces/w");
        std::fs::create_dir_all(src.join("local")).unwrap();
        std::fs::write(src.join("config.toml"), "x").unwrap();
        std::fs::write(src.join("local/auth.json"), "token").unwrap();
        let err = prepare(&src, t.path()).err().unwrap();
        assert!(err.contains("credential-named"), "{err}");
    }
}
