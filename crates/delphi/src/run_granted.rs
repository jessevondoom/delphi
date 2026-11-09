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
            t.retain(|k, _| !secret_key(k));
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
