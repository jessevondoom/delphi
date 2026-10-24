//! Sandboxed code execution for the Builder (`specs/008` §4, `specs/001` §5.1): the `[sandbox]` policy,
//! the task worktrees, and `run_command`.
//!
//! The `[sandbox]` table widens what a sandboxed process may do (toolchain paths it can read, the
//! environment it sees), so it is one of the blocks `local/ratified.lock` pins: a local edit of
//! `config.toml` cannot change it until the operator ratifies.
//!
//! ```toml
//! [sandbox]
//! toolchain = ["~/.cargo", "~/.rustup"]          # read-only and executable, nothing else of $HOME
//! env = { PATH = "~/.cargo/bin:/usr/bin:/bin", CARGO_TARGET_DIR = "{scratch}/target" }
//! timeout_secs = 600
//! max_output_bytes = 262144
//! host_exec_paths = ["tools/deploy"]
//! test_command = ["cargo", "test", "--offline"]   # merge gate 1
//! check_command = ["cargo", "clippy", "--offline"] # merge gate 2
//! ```
//!
//! The environment is built from `env` alone (plus `HOME`, `TMPDIR` and the like under the scratch
//! directory); nothing is inherited. Network is always off here. Placeholders `{scratch}` and
//! `{worktree}` expand to absolute paths.

use crate::worktree::{GitError, Worktree, Worktrees};
use delphi_sandbox::{run_bounded, Network, RunOptions, RunOutcome, Sandbox, SandboxSpec};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct ExecConfig {
    pub toolchain: Vec<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub cpu_secs: u64,
    pub scratch_quota_bytes: u64,
    pub host_exec_paths: Vec<String>,
    pub test_command: Option<Vec<String>>,
    pub check_command: Option<Vec<String>>,
}

impl Default for ExecConfig {
    fn default() -> ExecConfig {
        ExecConfig {
            toolchain: vec![],
            env: BTreeMap::from([("PATH".to_string(), "/usr/bin:/bin:/usr/sbin:/sbin".to_string())]),
            timeout: Duration::from_secs(600),
            max_output_bytes: 256 * 1024,
            cpu_secs: 900,
            scratch_quota_bytes: 2 << 30,
            host_exec_paths: vec![],
            test_command: None,
            check_command: None,
        }
    }
}

fn expand_home(s: &str, home: &Path) -> String {
    if s == "~" {
        home.display().to_string()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest).display().to_string()
    } else {
        s.replace(":~/", &format!(":{}/", home.display()))
    }
}

fn string_list(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

impl ExecConfig {
    /// From the effective config's `[sandbox]` table; anything missing keeps its default. `home` is the
    /// operator's home directory (for `~` in toolchain paths).
    pub fn from_root(root: &toml::Value, home: &Path) -> ExecConfig {
        let mut c = ExecConfig::default();
        let Some(t) = root.get("sandbox") else { return c };
        c.toolchain = string_list(t.get("toolchain")).iter().map(|p| PathBuf::from(expand_home(p, home))).collect();
        if let Some(env) = t.get("env").and_then(|e| e.as_table()) {
            c.env = env.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), expand_home(v, home)))).collect();
        }
        if let Some(n) = t.get("timeout_secs").and_then(|v| v.as_integer()).filter(|n| *n > 0) {
            c.timeout = Duration::from_secs(n as u64);
        }
        if let Some(n) = t.get("max_output_bytes").and_then(|v| v.as_integer()).filter(|n| *n > 0) {
            c.max_output_bytes = n as usize;
        }
        if let Some(n) = t.get("cpu_secs").and_then(|v| v.as_integer()).filter(|n| *n > 0) {
            c.cpu_secs = n as u64;
        }
        c.host_exec_paths = string_list(t.get("host_exec_paths"));
        let cmd = |k: &str| Some(string_list(t.get(k))).filter(|v| !v.is_empty());
        c.test_command = cmd("test_command");
        c.check_command = cmd("check_command");
        c
    }
}

/// Why `run_command` could not even start.
#[derive(Debug, PartialEq, Eq)]
pub enum ExecError {
    NoSandbox,
    BadArgs(String),
    Io(String),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::NoSandbox => write!(f, "no OS sandbox is available on this machine, so code is not executed (see `delphi doctor`)"),
            ExecError::BadArgs(m) | ExecError::Io(m) => write!(f, "{}", m),
        }
    }
}

/// Validate `run_command` arguments: a program and its arguments, never a shell string.
pub fn parse_argv(args: &serde_json::Value) -> Result<Vec<String>, ExecError> {
    let list = args["argv"].as_array().ok_or_else(|| ExecError::BadArgs("`argv` must be an array of strings: the program, then its arguments (there is no shell)".into()))?;
    let argv: Vec<String> = list.iter().map(|x| x.as_str().map(String::from)).collect::<Option<_>>().ok_or_else(|| ExecError::BadArgs("`argv` must contain only strings".into()))?;
    if argv.is_empty() || argv.len() > 64 || argv[0].is_empty() {
        return Err(ExecError::BadArgs("`argv` needs 1 to 64 entries, the first being the program".into()));
    }
    if argv.iter().any(|a| a.len() > 4096 || a.contains('\0')) {
        return Err(ExecError::BadArgs("an argument is too long or contains a NUL".into()));
    }
    Ok(argv)
}

/// Modified-file detection without trusting the command: (length, mtime) before and after.
type Snapshot = HashMap<String, (u64, i128)>;

fn snapshot(root: &Path) -> Snapshot {
    fn walk(dir: &Path, root: &Path, out: &mut Snapshot, budget: &mut usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            if *budget == 0 {
                return;
            }
            let name = e.file_name();
            if name.to_string_lossy().eq_ignore_ascii_case(".git") {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                walk(&e.path(), root, out, budget);
            } else if ft.is_file() {
                *budget -= 1;
                if let Ok(md) = e.metadata() {
                    let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i128).unwrap_or(0);
                    out.insert(e.path().strip_prefix(root).unwrap_or(&e.path()).display().to_string(), (md.len(), mtime));
                }
            }
        }
    }
    let mut out = Snapshot::new();
    let mut budget = 200_000usize;
    walk(root, root, &mut out, &mut budget);
    out
}

/// Files in `after` that are new or differ from `before`, in path order.
fn changed(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut v: Vec<String> = after.iter().filter(|(p, meta)| before.get(*p) != Some(*meta)).map(|(p, _)| p.clone()).collect();
    v.sort();
    v
}

#[derive(Debug)]
pub struct CommandReport {
    pub outcome: RunOutcome,
    /// Paths (relative to the worktree) the command created or modified.
    pub changed: Vec<String>,
    pub elapsed: Duration,
}

pub struct ExecEnv {
    pub sandbox: Option<Sandbox>,
    pub cfg: ExecConfig,
    pub worktrees: Worktrees,
    scratch_root: PathBuf,
    live: Mutex<HashMap<String, Worktree>>,
}

impl ExecEnv {
    /// `ws_dir` is the workspace home; worktrees live in `<ws>/worktrees`, scratch space in `<ws>/scratch`.
    pub fn new(ws_dir: &Path, cfg: ExecConfig, sandbox: Option<Sandbox>) -> ExecEnv {
        ExecEnv { sandbox, cfg, worktrees: Worktrees::new(&ws_dir.join("worktrees")), scratch_root: ws_dir.join("scratch"), live: Mutex::new(HashMap::new()) }
    }

    /// The worktree for a task: already known, or re-adopted after a restart, or created from `project`.
    /// The flag says a new one was cut just now.
    pub fn ensure(&self, task_id: &str, project: &Path) -> Result<(Worktree, bool), GitError> {
        if let Some(w) = self.live.lock().unwrap().get(task_id) {
            return Ok((w.clone(), false));
        }
        let (wt, created) = match self.worktrees.adopt(task_id) {
            Ok(w) => (w, false),
            Err(_) => (self.worktrees.create(project, task_id)?, true),
        };
        self.live.lock().unwrap().insert(task_id.to_string(), wt.clone());
        Ok((wt, created))
    }

    pub fn worktree_for(&self, task_id: &str, project: &Path) -> Result<Worktree, GitError> {
        self.ensure(task_id, project).map(|(w, _)| w)
    }

    pub fn known_worktree(&self, task_id: &str) -> Option<Worktree> {
        self.live.lock().unwrap().get(task_id).cloned()
    }

    pub fn forget(&self, task_id: &str) {
        self.live.lock().unwrap().remove(task_id);
    }

    pub fn scratch_for(&self, task_id: &str) -> PathBuf {
        self.scratch_root.join(task_id)
    }

    fn spec(&self, wt: &Path, scratch: &Path) -> Result<SandboxSpec, ExecError> {
        for d in ["home", "tmp", "target"] {
            std::fs::create_dir_all(scratch.join(d)).map_err(|e| ExecError::Io(e.to_string()))?;
        }
        let scratch = std::fs::canonicalize(scratch).map_err(|e| ExecError::Io(e.to_string()))?;
        let mut spec = SandboxSpec::new();
        spec.worktree = Some(wt.to_path_buf());
        spec.scratch = vec![scratch.clone()];
        spec.read_only.extend(self.cfg.toolchain.iter().filter(|p| p.exists()).cloned());
        spec.network = Network::Off;
        spec.limits.cpu_secs = Some(self.cfg.cpu_secs);
        let sub = |s: &str| s.replace("{scratch}", &scratch.display().to_string()).replace("{worktree}", &wt.display().to_string());
        spec.env = self.cfg.env.iter().map(|(k, v)| (k.clone(), sub(v))).collect();
        for (k, v) in [("HOME", scratch.join("home")), ("TMPDIR", scratch.join("tmp"))] {
            spec.env.entry(k.to_string()).or_insert_with(|| v.display().to_string());
        }
        spec.env.entry("CARGO_TARGET_DIR".into()).or_insert_with(|| scratch.join("target").display().to_string());
        Ok(spec)
    }

    /// Run `argv` in the worktree under the sandbox. Blocking; call from `spawn_blocking`.
    pub fn run(&self, wt: &Worktree, argv: &[String], timeout: Option<Duration>) -> Result<CommandReport, ExecError> {
        let sb = self.sandbox.as_ref().ok_or(ExecError::NoSandbox)?;
        let scratch = self.scratch_for(&wt.task_id);
        let spec = self.spec(&wt.path, &scratch)?;
        let mut cmd = sb.command(&spec, &argv[0], &argv[1..]).map_err(|e| ExecError::Io(e.to_string()))?;
        cmd.current_dir(&wt.path);
        let before = snapshot(&wt.path);
        let started = std::time::Instant::now();
        let opts = RunOptions { timeout: timeout.unwrap_or(self.cfg.timeout).min(self.cfg.timeout), max_output_bytes: self.cfg.max_output_bytes, quota: vec![(scratch.clone(), self.cfg.scratch_quota_bytes)] };
        let outcome = run_bounded(cmd, &opts).map_err(|e| ExecError::Io(format!("could not start `{}`: {}", argv[0], e)))?;
        let after = snapshot(&wt.path);
        Ok(CommandReport { outcome, changed: changed(&before, &after), elapsed: started.elapsed() })
    }
}

/// What the model sees: the exit status and the output, tails kept when long.
pub fn render(report: &CommandReport, argv: &[String]) -> (String, bool) {
    let o = &report.outcome;
    let tail = |s: &str, n: usize| -> String {
        if s.len() <= n {
            s.to_string()
        } else {
            let mut start = s.len() - n;
            while !s.is_char_boundary(start) {
                start += 1;
            }
            format!("… ({} bytes omitted)\n{}", start, &s[start..])
        }
    };
    let status = if o.timed_out {
        "timed out and was killed".to_string()
    } else if o.output_capped {
        "produced more output than allowed and was killed".to_string()
    } else {
        match o.status.map(|s| (s.code(), std::os::unix::process::ExitStatusExt::signal(&s))) {
            Some((Some(0), _)) => "exit 0".to_string(),
            Some((Some(c), _)) => format!("exit {}", c),
            Some((None, Some(sig))) => format!("killed by signal {}", sig),
            _ => "ended without a status".to_string(),
        }
    };
    let mut text = format!("$ {}\n[{}, {:.1}s, sandboxed, no network]\n", argv.join(" "), status, report.elapsed.as_secs_f64());
    if !o.stdout.is_empty() {
        text.push_str(&format!("--- stdout ---\n{}\n", tail(&o.stdout, 12_000)));
    }
    if !o.stderr.is_empty() {
        text.push_str(&format!("--- stderr ---\n{}\n", tail(&o.stderr, 6_000)));
    }
    if !report.changed.is_empty() {
        text.push_str(&format!("[{} file(s) in the worktree created or changed]\n", report.changed.len()));
    }
    if !o.over_quota.is_empty() {
        text.push_str("[scratch space is over its quota]\n");
    }
    (text, !o.success())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;

    pub(crate) struct Fx {
        pub _d: tempfile::TempDir,
        pub env: ExecEnv,
        pub project: PathBuf,
        pub ws: PathBuf,
    }

    pub(crate) fn fx() -> Fx {
        let d = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let project = crate::worktree::tests::project(&root);
        let ws = root.join("ws");
        fs::create_dir_all(&ws).unwrap();
        let env = ExecEnv::new(&ws, ExecConfig::default(), Sandbox::detect().ok());
        Fx { _d: d, env, project, ws }
    }

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn config_reads_the_sandbox_table_and_expands_home_and_placeholders() {
        let root: toml::Value = toml::from_str("[sandbox]\ntoolchain = [\"~/.cargo\", \"/opt/tc\"]\nenv = { PATH = \"~/.cargo/bin:/usr/bin\", X = \"{scratch}/x\" }\ntimeout_secs = 90\nhost_exec_paths = [\"tools\"]\ntest_command = [\"cargo\", \"test\"]\n").unwrap();
        let c = ExecConfig::from_root(&root, Path::new("/op-home"));
        assert_eq!(c.toolchain, [PathBuf::from("/op-home/.cargo"), PathBuf::from("/opt/tc")]);
        assert_eq!(c.env["PATH"], "/op-home/.cargo/bin:/usr/bin");
        assert_eq!((c.timeout, c.host_exec_paths.clone(), c.test_command.clone(), c.check_command.clone()), (Duration::from_secs(90), vec!["tools".to_string()], Some(argv(&["cargo", "test"])), None));
        assert_eq!(ExecConfig::from_root(&toml::Value::Table(Default::default()), Path::new("/h")), ExecConfig::default());
    }

    #[test]
    fn argv_is_validated_and_there_is_no_shell() {
        let ok = parse_argv(&serde_json::json!({"argv": ["cargo", "test"]})).unwrap();
        assert_eq!(ok, ["cargo", "test"]);
        for bad in [serde_json::json!({}), serde_json::json!({"argv": "cargo test"}), serde_json::json!({"argv": []}), serde_json::json!({"argv": ["a", 1]}), serde_json::json!({"argv": [""]}), serde_json::json!({"argv": ["a\0b"]})] {
            assert!(parse_argv(&bad).is_err(), "{}", bad);
        }
    }

    #[test]
    fn a_command_runs_in_the_worktree_sandbox_and_reports_what_it_changed() {
        let f = fx();
        if f.env.sandbox.is_none() {
            return;
        }
        let wt = f.env.worktree_for("t_1", &f.project).unwrap();
        let r = f.env.run(&wt, &argv(&["/bin/sh", "-c", "echo built > out.txt; echo hello; pwd"]), None).unwrap();
        assert!(r.outcome.success(), "{:?}", r.outcome);
        assert_eq!(r.changed, ["out.txt"]);
        assert!(r.outcome.stdout.contains("hello") && r.outcome.stdout.trim_end().ends_with(wt.path.file_name().unwrap().to_str().unwrap()));
        assert_eq!(fs::read_to_string(wt.path.join("out.txt")).unwrap(), "built\n");
        let (text, is_error) = render(&r, &argv(&["sh"]));
        assert!(!is_error && text.contains("exit 0") && text.contains("1 file(s)"), "{}", text);
    }

    #[test]
    fn the_command_cannot_reach_the_project_the_workspace_or_the_network() {
        let f = fx();
        if f.env.sandbox.is_none() {
            return;
        }
        fs::write(f.ws.join("secret"), "s3cret").unwrap();
        let wt = f.env.worktree_for("t_2", &f.project).unwrap();
        let script = format!("cat {}/src/lib.rs; cat {}/secret; echo x > {}/planted; echo x > .git; ls {}/.git; echo done", f.project.display(), f.ws.display(), f.project.display(), wt.path.display());
        let r = f.env.run(&wt, &argv(&["/bin/sh", "-c", &script]), None).unwrap();
        assert!(r.outcome.stdout.contains("done"), "{:?}", r.outcome);
        assert!(!r.outcome.stdout.contains("pub fn a") && !r.outcome.stdout.contains("s3cret"), "{:?}", r.outcome);
        assert!(!f.project.join("planted").exists(), "a write outside the worktree succeeded");
        let wt_git = fs::read_to_string(wt.path.join(".git")).unwrap();
        assert!(wt_git.starts_with("gitdir:"), "the gitfile was overwritten: {}", wt_git);
    }

    #[test]
    fn the_environment_is_only_what_the_policy_gives() {
        let f = fx();
        if f.env.sandbox.is_none() {
            return;
        }
        let wt = f.env.worktree_for("t_3", &f.project).unwrap();
        // SAFETY: test-only; the variable must simply exist in this process.
        unsafe { std::env::set_var("DELPHI_TEST_SECRET", "leak") };
        let r = f.env.run(&wt, &argv(&["/usr/bin/env"]), None).unwrap();
        assert!(!r.outcome.stdout.contains("leak") && !r.outcome.stdout.contains("DELPHI_TEST_SECRET"));
        let vars: Vec<&str> = r.outcome.stdout.lines().filter_map(|l| l.split('=').next()).collect();
        assert!(vars.contains(&"PATH") && vars.contains(&"HOME") && vars.contains(&"TMPDIR"), "{:?}", vars);
    }

    #[test]
    fn a_hung_command_is_killed_at_the_configured_timeout() {
        let mut f = fx();
        if f.env.sandbox.is_none() {
            return;
        }
        f.env.cfg.timeout = Duration::from_millis(500);
        let wt = f.env.worktree_for("t_4", &f.project).unwrap();
        let r = f.env.run(&wt, &argv(&["/bin/sh", "-c", "sleep 30"]), Some(Duration::from_secs(900))).unwrap();
        assert!(r.outcome.timed_out && r.elapsed < Duration::from_secs(5));
        let (text, err) = render(&r, &argv(&["sleep"]));
        assert!(err && text.contains("timed out"), "{}", text);
    }

    #[test]
    fn without_a_sandbox_nothing_runs() {
        let mut f = fx();
        f.env.sandbox = None;
        let wt = f.env.worktree_for("t_5", &f.project).unwrap();
        assert_eq!(f.env.run(&wt, &argv(&["/bin/echo", "hi"]), None).unwrap_err(), ExecError::NoSandbox);
    }
}
