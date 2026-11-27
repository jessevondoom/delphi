//! Task worktrees and the hardened git calls Delphi makes on them (`specs/008` §4, `specs/006` §3.1).
//!
//! * A worktree lives under the workspace home, never in the project checkout, on a branch
//!   `delphi/<task-id>` cut from the project's `HEAD`.
//! * **Every git call is made by the daemon, outside the sandbox**, with the git directory and work tree
//!   given explicitly (never discovered from content a sandboxed process could have written), the system
//!   and user config disabled, hooks and external commands switched off, and a minimal environment.
//! * A project whose own config carries `include` directives or content filters is refused: those run
//!   programs on files a build can influence.
//! * [`Worktree::diff`] reports what changed against the base commit and flags files that execute on the
//!   operator's machine once merged (the Red **host-execution** badge at merge gate 5).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, PartialEq, Eq)]
pub enum GitError {
    Io(String),
    /// A git command failed; stderr is kept short and is never shown to a model.
    Failed { args: String, stderr: String },
    BadTaskId(String),
    NotAGitRepo(PathBuf),
    UnsafeConfig(Vec<String>),
    Exists(PathBuf),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Io(m) => write!(f, "{}", m),
            GitError::Failed { args, stderr } => write!(f, "git {} failed: {}", args, stderr),
            GitError::BadTaskId(id) => write!(f, "`{}` is not a valid task id (letters, digits, `_`, `-`)", id),
            GitError::NotAGitRepo(p) => write!(f, "{} is not a git repository with at least one commit", p.display()),
            GitError::UnsafeConfig(keys) => write!(f, "the project's git config sets {}, which would run programs on files a build can write; remove them (or use a clone without them) to allow worktrees", keys.join(", ")),
            GitError::Exists(p) => write!(f, "{} already exists", p.display()),
        }
    }
}

impl std::error::Error for GitError {}

/// Settings every daemon git call carries.
const HARDENING: &[&str] = &[
    "core.fsmonitor=false",
    "core.hooksPath=/dev/null",
    "core.pager=cat",
    "core.editor=true",
    "core.askpass=true",
    "core.sshCommand=false",
    "core.untrackedCache=false",
    "protocol.file.allow=never",
    "protocol.ext.allow=never",
    "diff.external=",
    "commit.gpgSign=false",
    "tag.gpgSign=false",
    "gc.auto=0",
    "maintenance.auto=false",
    "advice.detachedHead=false",
];

fn git_command(git_dir: Option<&Path>, work_tree: Option<&Path>, home: &Path) -> Command {
    let mut c = Command::new("git");
    c.env_clear();
    // PATH only so `git` can find its helpers; HOME points at an empty directory so no user config is read.
    c.env("PATH", std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()));
    c.env("HOME", home).env("XDG_CONFIG_HOME", home).env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0").env("LC_ALL", "C");
    for h in HARDENING {
        c.arg("-c").arg(h);
    }
    c.arg("-c").arg("user.name=Delphi").arg("-c").arg("user.email=delphi@localhost.invalid");
    if let Some(g) = git_dir {
        c.arg("--git-dir").arg(g);
    }
    if let Some(w) = work_tree {
        c.arg("--work-tree").arg(w);
    }
    c.stdin(Stdio::null());
    c
}

fn run(mut c: Command, label: &str) -> Result<String, GitError> {
    let out = c.output().map_err(|e| GitError::Io(format!("cannot run git: {}", e)))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(GitError::Failed { args: label.into(), stderr: String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect() })
    }
}

fn valid_task_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A branch name Delphi passes to git (`specs/135` §6.2): letters, digits and `._/-`, never starting with `-`, `/` or `.` (so it is never
/// read as an option), no `..`, `//` or `/.`, not ending in `/`, `.` or `.lock`, at most 200 characters.
pub fn valid_branch(b: &str) -> bool {
    !b.is_empty()
        && b.len() <= 200
        && b.chars().all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
        && !b.starts_with(['-', '/', '.'])
        && !b.ends_with(['/', '.'])
        && !b.ends_with(".lock")
        && !b.contains("..")
        && !b.contains("//")
        && !b.contains("/.")
}

/// Config keys in a project's local config that would run programs on worktree content.
fn unsafe_config(project: &Path, home: &Path) -> Result<Vec<String>, GitError> {
    let mut c = git_command(None, None, home);
    c.arg("-C").arg(project).args(["config", "--local", "--list", "--name-only", "-z"]);
    let out = c.output().map_err(|e| GitError::Io(e.to_string()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut bad: Vec<String> = text
        .split('\0')
        .map(|k| k.to_lowercase())
        .filter(|k| k.starts_with("include.") || k.starts_with("includeif.") || (k.starts_with("filter.") && (k.ends_with(".clean") || k.ends_with(".smudge") || k.ends_with(".process"))))
        .collect();
    bad.sort();
    bad.dedup();
    Ok(bad)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub task_id: String,
    pub path: PathBuf,
    pub branch: String,
    pub project: PathBuf,
    pub base_sha: String,
    /// `<project>/.git/worktrees/<name>`, recorded at creation and never re-read from the worktree.
    git_dir: PathBuf,
    /// An empty directory used as `HOME` for git, so no user config is read.
    scratch_home: PathBuf,
    /// The project had uncommitted changes the worktree does not contain.
    pub project_was_dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    Merged { commit: String },
    /// Nothing was changed in the project.
    Refused(String),
    /// The merge conflicted and was aborted; the project is as it was.
    Conflict(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrOutcome {
    Opened { url: String },
    /// Nothing was pushed.
    Refused(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub status: char,
    pub added: u64,
    pub removed: u64,
    /// Runs on, or reaches into, the operator's machine once merged (Red badge at gate 5).
    pub host_exec: bool,
    /// Why, when it is not just the path: a symlink (it can point anywhere on the operator's disk) or a
    /// submodule.
    pub special: Option<&'static str>,
    /// The git file mode after the change (`100644`, `100755`, ...); empty when git did not report one.
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffSummary {
    pub head_sha: String,
    pub files: Vec<FileChange>,
}

impl DiffSummary {
    pub fn added(&self) -> u64 {
        self.files.iter().map(|f| f.added).sum()
    }
    pub fn removed(&self) -> u64 {
        self.files.iter().map(|f| f.removed).sum()
    }
    pub fn host_exec(&self) -> Vec<&FileChange> {
        self.files.iter().filter(|f| f.host_exec).collect()
    }
}

/// Files that run on the operator's machine, outside any sandbox, once merged and opened by an IDE, a
/// shell or git (`specs/008` §4). `extra` is the configurable `host_exec_paths` list (path prefixes or
/// `*.ext` globs).
pub fn is_host_exec(path: &str, extra: &[String]) -> bool {
    let p = path.trim_start_matches("./");
    let name = p.rsplit('/').next().unwrap_or(p);
    let lower = name.to_lowercase();
    if name == "build.rs" || lower == "makefile" || lower == "gnumakefile" || lower == ".envrc" || lower == ".gitattributes" || lower == ".gitmodules" || lower.starts_with("rust-toolchain") {
        return true;
    }
    if p.starts_with(".github/") || p.starts_with(".vscode/") || p.starts_with(".idea/") || p.starts_with(".cargo/config") || p.contains("/.cargo/config") {
        return true;
    }
    extra.iter().any(|e| match e.strip_prefix("*.") {
        Some(ext) => p.ends_with(&format!(".{}", ext)),
        None => p == e || p.starts_with(&format!("{}/", e.trim_end_matches('/'))),
    })
}

/// A `package.json` runs code through its `scripts` (and lifecycle hooks), so a change to that object is
/// host execution; a change to dependencies alone is a dependency change, not this.
fn package_scripts_changed(base: Option<&str>, head: Option<&str>) -> bool {
    let scripts = |t: Option<&str>| t.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()).map(|v| v["scripts"].clone()).unwrap_or(serde_json::Value::Null);
    scripts(base) != scripts(head)
}

pub struct Worktrees {
    root: PathBuf,
    scratch_home: PathBuf,
}

impl Worktrees {
    /// `root` is `<workspace>/worktrees`.
    pub fn new(root: &Path) -> Worktrees {
        Worktrees { root: root.to_path_buf(), scratch_home: root.join(".git-home") }
    }

    fn home(&self) -> Result<&Path, GitError> {
        std::fs::create_dir_all(&self.scratch_home).map_err(|e| GitError::Io(e.to_string()))?;
        Ok(&self.scratch_home)
    }

    pub fn path_for(&self, task_id: &str) -> PathBuf {
        self.root.join(task_id)
    }

    /// Cut `delphi/<task-id>` from the project's `HEAD` into a new worktree.
    pub fn create(&self, project: &Path, task_id: &str) -> Result<Worktree, GitError> {
        if !valid_task_id(task_id) {
            return Err(GitError::BadTaskId(task_id.into()));
        }
        let home = self.home()?.to_path_buf();
        let project = std::fs::canonicalize(project).map_err(|e| GitError::Io(format!("{}: {}", project.display(), e)))?;
        let mut c = git_command(None, None, &home);
        c.arg("-C").arg(&project).args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
        let base_sha = run(c, "rev-parse HEAD").map_err(|_| GitError::NotAGitRepo(project.clone()))?.trim().to_string();
        let bad = unsafe_config(&project, &home)?;
        if !bad.is_empty() {
            return Err(GitError::UnsafeConfig(bad));
        }
        let path = self.path_for(task_id);
        if path.exists() {
            return Err(GitError::Exists(path));
        }
        std::fs::create_dir_all(&self.root).map_err(|e| GitError::Io(e.to_string()))?;
        let branch = format!("delphi/{}", task_id);
        let mut c = git_command(None, None, &home);
        c.arg("-C").arg(&project).args(["worktree", "add", "--quiet", "-b", &branch]).arg(&path).arg(&base_sha);
        run(c, "worktree add")?;
        let path = std::fs::canonicalize(&path).map_err(|e| GitError::Io(e.to_string()))?;
        // Read the linked git directory once, now, before any sandboxed process has touched the worktree.
        let gitfile = std::fs::read_to_string(path.join(".git")).map_err(|e| GitError::Io(e.to_string()))?;
        let git_dir = gitfile.trim().strip_prefix("gitdir:").map(|g| PathBuf::from(g.trim())).ok_or_else(|| GitError::Io("the new worktree has no gitdir pointer".into()))?;
        let mut c = git_command(None, None, &home);
        c.arg("-C").arg(&project).args(["status", "--porcelain", "--untracked-files=no"]);
        let dirty = !run(c, "status")?.trim().is_empty();
        let wt = Worktree { task_id: task_id.into(), path, branch, project, base_sha, git_dir, scratch_home: home, project_was_dirty: dirty };
        // Kept beside the worktrees, where no sandboxed process can write, so a restarted daemon trusts it
        // instead of re-reading anything from the worktree itself.
        let meta = serde_json::json!({"project": wt.project, "branch": wt.branch, "base_sha": wt.base_sha, "git_dir": wt.git_dir, "project_was_dirty": dirty});
        let meta_dir = self.root.join(".meta");
        std::fs::create_dir_all(&meta_dir).map_err(|e| GitError::Io(e.to_string()))?;
        std::fs::write(meta_dir.join(format!("{}.json", task_id)), meta.to_string()).map_err(|e| GitError::Io(e.to_string()))?;
        Ok(wt)
    }

    /// `git status --porcelain` of a project checkout (empty when clean), or an error marker; used to prove a
    /// harness left a project as it found it.
    pub fn project_status(&self, project: &Path) -> String {
        let Ok(home) = self.home() else { return "<no git home>".into() };
        let mut c = git_command(None, None, home);
        c.arg("-C").arg(project).args(["status", "--porcelain", "-z", "--untracked-files=all"]);
        run(c, "status").unwrap_or_else(|e| format!("<{}>", e))
    }

    /// Re-attach to a worktree this daemon created earlier (after a restart).
    pub fn adopt(&self, task_id: &str) -> Result<Worktree, GitError> {
        if !valid_task_id(task_id) {
            return Err(GitError::BadTaskId(task_id.into()));
        }
        let text = std::fs::read_to_string(self.root.join(".meta").join(format!("{}.json", task_id))).map_err(|e| GitError::Io(format!("no record of a worktree for {}: {}", task_id, e)))?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| GitError::Io(e.to_string()))?;
        let get = |k: &str| v[k].as_str().map(String::from).ok_or_else(|| GitError::Io(format!("worktree record lacks `{}`", k)));
        let path = std::fs::canonicalize(self.path_for(task_id)).map_err(|e| GitError::Io(format!("the worktree for {} is gone: {}", task_id, e)))?;
        Ok(Worktree {
            task_id: task_id.into(),
            path,
            branch: get("branch")?,
            project: PathBuf::from(get("project")?),
            base_sha: get("base_sha")?,
            git_dir: PathBuf::from(get("git_dir")?),
            scratch_home: self.home()?.to_path_buf(),
            project_was_dirty: v["project_was_dirty"].as_bool().unwrap_or(false),
        })
    }
}

impl Worktree {
    fn git(&self) -> Command {
        git_command(Some(&self.git_dir), Some(&self.path), &self.scratch_home)
    }

    /// Files and directories whose contents decide what git runs on the operator's machine: config, attribute
    /// overrides and hooks, for the project and for this worktree. `exec.rs` watches them around any command
    /// that is not sandboxed.
    pub fn git_internals(&self) -> Vec<PathBuf> {
        let common = self.project.join(".git");
        vec![common.join("config"), common.join("info/attributes"), common.join("hooks"), self.git_dir.join("config.worktree"), self.git_dir.join("info/attributes"), self.git_dir.join("hooks")]
    }

    /// Stage everything the build produced (nothing is ever staged from a `.git` segment) and commit it.
    /// `None` means there was nothing to commit. Hooks never run.
    pub fn commit_all(&self, message: &str, author: &str) -> Result<Option<String>, GitError> {
        let mut c = self.git();
        c.args(["add", "--all", "--", "."]); // .gitignore is respected: build junk stays out of the commit
        run(c, "add")?;
        let mut c = self.git();
        c.args(["diff", "--cached", "--quiet"]);
        if c.status().map_err(|e| GitError::Io(e.to_string()))?.success() {
            return Ok(None);
        }
        let mut c = self.git();
        c.arg("-c").arg(format!("user.name={}", author)).args(["commit", "--quiet", "--no-verify", "--no-gpg-sign", "-m", message]);
        run(c, "commit")?;
        Ok(Some(self.head()?))
    }

    /// Put `path` (relative to the worktree) back as the base commit had it: restored if the base has it,
    /// removed if it does not. Returns `"base"` or `"removed"`. A path that is not plain and inside the worktree,
    /// or that has a `.git` segment, is refused. The change is left uncommitted for the caller to commit.
    pub fn restore_to_base(&self, path: &str) -> Result<&'static str, GitError> {
        let plain = !path.is_empty() && !path.starts_with('/') && !path.split('/').any(|s| s.is_empty() || s == ".." || s == "." || s.eq_ignore_ascii_case(".git"));
        if !plain {
            return Err(GitError::Failed { args: "restore".into(), stderr: format!("`{}` is not a plain path inside the worktree", path) });
        }
        if self.blob(&self.base_sha, path).is_ok() {
            let mut c = self.git();
            c.args(["checkout", &self.base_sha, "--", path]);
            run(c, "checkout")?;
            Ok("base")
        } else {
            let full = self.path.join(path);
            if full.is_file() {
                std::fs::remove_file(&full).map_err(|e| GitError::Io(e.to_string()))?;
            }
            Ok("removed")
        }
    }

    pub fn head(&self) -> Result<String, GitError> {
        let mut c = self.git();
        c.args(["rev-parse", "HEAD"]);
        Ok(run(c, "rev-parse")?.trim().to_string())
    }

    /// The bytes of `path` at `rev`.
    pub fn blob(&self, rev: &str, path: &str) -> Result<Vec<u8>, GitError> {
        let mut c = self.git();
        c.args(["show", &format!("{}:{}", rev, path)]);
        let out = c.output().map_err(|e| GitError::Io(e.to_string()))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(GitError::Failed { args: "show".into(), stderr: String::from_utf8_lossy(&out.stderr).trim().chars().take(200).collect() })
        }
    }

    fn show(&self, rev: &str, path: &str) -> Option<String> {
        let mut c = self.git();
        c.args(["show", &format!("{}:{}", rev, path)]);
        run(c, "show").ok()
    }

    /// What `HEAD` changes against the base commit, with the host-execution flag per file.
    pub fn diff(&self, host_exec_paths: &[String]) -> Result<DiffSummary, GitError> {
        let head_sha = self.head()?;
        let mut c = self.git();
        c.args(["diff", "--no-ext-diff", "--no-textconv", "--no-renames", "-z", "--numstat"]).arg(&self.base_sha).arg("HEAD");
        let numstat = run(c, "diff --numstat")?;
        let mut c = self.git();
        c.args(["diff", "--no-ext-diff", "--no-textconv", "--no-renames", "-z", "--name-status"]).arg(&self.base_sha).arg("HEAD");
        let names = run(c, "diff --name-status")?;
        // `-z` output: `A\0path\0` pairs for name-status; `added\tremoved\tpath\0` for numstat.
        let mut status = std::collections::HashMap::new();
        let mut it = names.split('\0').filter(|s| !s.is_empty());
        while let (Some(s), Some(p)) = (it.next(), it.next()) {
            status.insert(p.to_string(), s.chars().next().unwrap_or('M'));
        }
        // `--raw -z` is `:<src mode> <dst mode> <src sha> <dst sha> <status>\0<path>\0`.
        let mut c = self.git();
        c.args(["diff", "--no-ext-diff", "--no-textconv", "--no-renames", "-z", "--raw"]).arg(&self.base_sha).arg("HEAD");
        let raw = run(c, "diff --raw")?;
        let mut modes = std::collections::HashMap::new();
        let mut it = raw.split('\0').filter(|s| !s.is_empty());
        while let (Some(meta), Some(path)) = (it.next(), it.next()) {
            if let Some(dst) = meta.split_whitespace().nth(1) {
                modes.insert(path.to_string(), dst.to_string());
            }
        }
        let mut files = Vec::new();
        for rec in numstat.split('\0').filter(|s| !s.is_empty()) {
            let mut parts = rec.splitn(3, '\t');
            let (a, r, path) = (parts.next().unwrap_or("0"), parts.next().unwrap_or("0"), parts.next().unwrap_or(""));
            if path.is_empty() {
                continue;
            }
            let mut host = is_host_exec(path, host_exec_paths);
            if !host && path.rsplit('/').next() == Some("package.json") {
                host = package_scripts_changed(self.show(&self.base_sha, path).as_deref(), self.show("HEAD", path).as_deref());
            }
            let special = match modes.get(path).map(String::as_str) {
                Some("120000") => Some("symlink"),
                Some("160000") => Some("submodule"),
                _ => None,
            };
            files.push(FileChange { path: path.to_string(), status: status.get(path).copied().unwrap_or('M'), added: a.parse().unwrap_or(10_000), removed: r.parse().unwrap_or(0), host_exec: host || special.is_some(), special, mode: modes.get(path).cloned().unwrap_or_default() });
        }
        files.sort_by(|a, b| (!a.host_exec, &a.path).cmp(&(!b.host_exec, &b.path)));
        Ok(DiffSummary { head_sha, files })
    }

    /// Did this build add or change a test? Judged from the paths (`tests/`, `*_test.*`, `*.spec.*`, …) and from
    /// the added lines (`#[test]`, `#[cfg(test)]`, `def test_`, `it(`, `@Test`, …). Used to decide whether a
    /// failing test run may be retried once as a possible flake: a failure of anything the task wrote always counts.
    pub fn touches_tests(&self) -> Result<bool, GitError> {
        let d = self.diff(&[])?;
        let path_hint = |p: &str| {
            let l = p.to_lowercase();
            l.contains("/tests/") || l.starts_with("tests/") || l.contains("/test/") || l.starts_with("test/") || l.contains("__tests__") || l.contains("_test.") || l.contains(".test.") || l.contains(".spec.") || l.contains("/spec/") || l.rsplit('/').next().is_some_and(|n| n.starts_with("test_"))
        };
        if d.files.iter().any(|f| path_hint(&f.path)) {
            return Ok(true);
        }
        let mut c = self.git();
        c.args(["diff", "--no-ext-diff", "--no-textconv", "--no-renames", "-U0", &self.base_sha, "HEAD"]);
        let text = run(c, "diff -U0")?;
        const MARKERS: [&str; 10] = ["#[test]", "#[cfg(test)]", "#[tokio::test]", "def test_", "func Test", "@Test", " it(", " test(", "describe(", "fn test_"];
        Ok(text.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).any(|l| MARKERS.iter().any(|m| l.contains(m))))
    }

    /// The unified diff of one file against the base (for the approval card), without any external driver.
    pub fn file_diff(&self, path: &str, max_bytes: usize) -> Result<String, GitError> {
        let mut c = self.git();
        c.args(["diff", "--no-ext-diff", "--no-textconv", "--no-renames", &self.base_sha, "HEAD", "--", path]);
        let mut d = run(c, "diff")?;
        if d.len() > max_bytes {
            let mut end = max_bytes;
            while !d.is_char_boundary(end) {
                end -= 1;
            }
            d.truncate(end);
            d.push_str("\n… (truncated)");
        }
        Ok(d)
    }

    /// Merge the task branch into the project's current branch, in the operator's checkout (local mode,
    /// `specs/003` §3 merge-back). Only ever called after the operator approved `approved_sha`.
    pub fn merge_back(&self, approved_sha: &str, message: &str, host_exec_paths: &[String]) -> Result<MergeOutcome, GitError> {
        let project = |args: &[&str], label: &str| {
            let mut c = git_command(None, None, &self.scratch_home);
            c.arg("-C").arg(&self.project).args(args);
            run(c, label)
        };
        let tip = project(&["rev-parse", &self.branch], "rev-parse branch")?.trim().to_string();
        if tip != approved_sha {
            return Ok(MergeOutcome::Refused(format!("the branch moved after approval ({} is not {})", &tip[..tip.len().min(8)], &approved_sha[..approved_sha.len().min(8)])));
        }
        if project(&["symbolic-ref", "--quiet", "--short", "HEAD"], "symbolic-ref").is_err() {
            return Ok(MergeOutcome::Refused("the project has a detached HEAD; check out a branch to merge into".into()));
        }
        if project(&["rev-parse", "--verify", "--quiet", "MERGE_HEAD"], "rev-parse MERGE_HEAD").is_ok() {
            return Ok(MergeOutcome::Refused("the project is in the middle of a merge".into()));
        }
        // The checkout's own config is re-checked now: a merge applies `.gitattributes` from the merged tree.
        let bad = unsafe_config(&self.project, &self.scratch_home)?;
        if !bad.is_empty() {
            return Ok(MergeOutcome::Refused(GitError::UnsafeConfig(bad).to_string()));
        }
        let touched: Vec<String> = self.diff(host_exec_paths)?.files.into_iter().map(|f| f.path).collect();
        let dirty = project(&["status", "--porcelain", "-z", "--untracked-files=all"], "status")?;
        let dirty_paths: Vec<&str> = dirty.split('\0').filter(|r| r.len() > 3).map(|r| &r[3..]).collect();
        let overlap: Vec<&String> = touched.iter().filter(|t| dirty_paths.iter().any(|d| d == t)).collect();
        if !overlap.is_empty() {
            return Ok(MergeOutcome::Refused(format!("you have uncommitted changes in files this merge touches: {}", overlap.iter().take(5).map(|s| s.as_str()).collect::<Vec<_>>().join(", "))));
        }
        match project(&["merge", "--no-ff", "--no-verify", "--no-gpg-sign", "-m", message, &self.branch], "merge") {
            Ok(_) => Ok(MergeOutcome::Merged { commit: project(&["rev-parse", "HEAD"], "rev-parse HEAD")?.trim().to_string() }),
            Err(_) => {
                let conflicts: Vec<String> = project(&["diff", "--name-only", "--diff-filter=U"], "diff conflicts").unwrap_or_default().lines().map(String::from).collect();
                let _ = project(&["merge", "--abort"], "merge --abort");
                Ok(MergeOutcome::Conflict(conflicts))
            }
        }
    }

    /// Push the task branch to `origin` and open a pull request with `gh` (PR mode). Only ever called after the
    /// operator approved `approved_sha`. These two calls need the operator's own credentials (git's helper, the
    /// SSH agent, `gh`'s login), so they run with the operator's environment; hooks and external protocols stay off.
    ///
    /// With `guard`, the files of the range about to be pushed are checked first at the repository sink ([`Worktree::push_check`],
    /// `specs/141` §2.4 item 6, `specs/142` §2.5 item 4 (g)): a refusal (a labelled file, or more of a held source than may leave this
    /// machine) pushes nothing and names the files and the marks or the source. A label that is sent labelled (`internal` to a remote on
    /// `[handling] internal_remotes`) is pushed only when the card the operator answered showed exactly that label's line.
    pub fn open_pr(&self, approved_sha: &str, title: &str, body: &str, gh: &str, guard: Option<&PushGuard>) -> Result<PrOutcome, GitError> {
        let project = |args: &[&str], label: &str| {
            let mut c = git_command(None, None, &self.scratch_home);
            c.arg("-C").arg(&self.project).args(args);
            run(c, label)
        };
        let tip = project(&["rev-parse", &self.branch], "rev-parse branch")?.trim().to_string();
        if tip != approved_sha {
            return Ok(PrOutcome::Refused(format!("the branch moved after approval ({} is not {})", &tip[..tip.len().min(8)], &approved_sha[..approved_sha.len().min(8)])));
        }
        let base = match project(&["symbolic-ref", "--quiet", "--short", "HEAD"], "symbolic-ref") {
            Ok(b) => b.trim().to_string(),
            Err(_) => return Ok(PrOutcome::Refused("the project has a detached HEAD; check out the branch the pull request should target".into())),
        };
        if project(&["remote", "get-url", "origin"], "remote get-url").is_err() {
            return Ok(PrOutcome::Refused("the project has no `origin` remote to push to".into()));
        }
        if let Some(g) = guard {
            match self.push_check(g)? {
                Err(e) => return Ok(PrOutcome::Refused(format!("the branch {e}; nothing was pushed"))),
                Ok(sent) if sent.decision.kind == crate::label::Kind::SendLabelled && g.card_line.as_deref() != Some(sent.line().as_str()) => {
                    return Ok(PrOutcome::Refused(format!("the branch is labelled ({}) and the card you answered did not show that label; nothing was pushed", sent.line())));
                }
                Ok(_) => {}
            }
        }
        let mut push = Command::new("git");
        push.args(["-c", "core.hooksPath=/dev/null", "-c", "protocol.ext.allow=never", "-C"]).arg(&self.project).args(["push", "--no-verify", "origin", &format!("{}:refs/heads/{}", self.branch, self.branch)]).env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null());
        if let Err(e) = run(push, "push") {
            return Ok(PrOutcome::Refused(format!("the push failed: {}", e)));
        }
        let mut pr = Command::new(gh);
        pr.args(["pr", "create", "--head", &self.branch, "--base", &base, "--title", title, "--body", body]).current_dir(&self.project).stdin(Stdio::null());
        match run(pr, "gh pr create") {
            Ok(out) => match out.lines().rev().map(str::trim).find(|l| l.starts_with("https://")) {
                Some(url) => Ok(PrOutcome::Opened { url: url.to_string() }),
                None => Ok(PrOutcome::Refused("the branch was pushed, but `gh pr create` printed no pull request URL".into())),
            },
            Err(e) => Ok(PrOutcome::Refused(format!("the branch was pushed, but opening the pull request failed: {}", e))),
        }
    }

    /// The repository sink over the range a push would send (`specs/141` §2.4 item 6): each file's label from the daemon's records
    /// and the quotation bound over all of them as one output, at the sink of `origin` (on `[handling] internal_remotes` or not). A
    /// labelled send is confirmed by the card (`Confirm::Card`); the caller shows `Sent::line` on it. The outer `Err` is git's; the
    /// inner one is the refusal.
    pub fn push_check(&self, g: &PushGuard) -> Result<Result<crate::egress::Sent, String>, GitError> {
        let mut c = git_command(None, None, &self.scratch_home);
        c.arg("-C").arg(&self.project).args(["remote", "get-url", "origin"]);
        let remote = run(c, "remote get-url").ok().map(|u| u.trim().to_string());
        let files = self.range_files()?;
        Ok(repository_check(&g.home, g.ws.as_deref(), &self.project, &files, remote.as_deref(), crate::egress::Confirm::Card))
    }

    /// The files the branch changed since its base, as they are at its tip (deleted files are not read): what a push would send.
    pub fn range_files(&self) -> Result<Vec<(String, Vec<u8>)>, GitError> {
        let project = |args: &[&str], label: &str| {
            let mut c = git_command(None, None, &self.scratch_home);
            c.arg("-C").arg(&self.project).args(args);
            run(c, label)
        };
        let range = format!("{}..{}", self.base_sha, self.branch);
        let names = project(&["diff", "--name-only", "-z", "--no-renames", "--diff-filter=d", &range], "diff range")?;
        let mut out = Vec::new();
        for name in names.split('\0').filter(|n| !n.is_empty()) {
            let text = project(&["show", &format!("{}:{}", self.branch, name)], "show")?;
            out.push((name.to_string(), text.into_bytes()));
        }
        Ok(out)
    }

    /// Remove the worktree and its branch. The project's `.git/worktrees/` entry goes with it.
    pub fn remove(&self) -> Result<(), GitError> {
        let mut c = git_command(None, None, &self.scratch_home);
        c.arg("-C").arg(&self.project).args(["worktree", "remove", "--force"]).arg(&self.path);
        let first = run(c, "worktree remove");
        if first.is_err() {
            let _ = std::fs::remove_dir_all(&self.path);
            let mut c = git_command(None, None, &self.scratch_home);
            c.arg("-C").arg(&self.project).args(["worktree", "prune"]);
            run(c, "worktree prune")?;
        }
        let mut c = git_command(None, None, &self.scratch_home);
        c.arg("-C").arg(&self.project).args(["branch", "-D", &self.branch]);
        let _ = run(c, "branch -D");
        Ok(())
    }
}

/// A throwaway copy of a project for an investigation (`specs/116` §8.1).
///
/// It is a repository of its own, not a linked worktree of the project: the project's objects are fetched into it by the daemon,
/// and its files are checked out detached at the project's head. So nothing an investigation does (a checkout, a bisect, a commit,
/// a build) writes into the project's `.git`, its index or its files, and the sandbox it runs in needs no path of the project at
/// all. It has no remote, so nothing in it can be pushed or fetched; it is deleted when the span ends.
///
/// The git directory is `<root>/repo` and the files are in `<root>/tree`, whose `.git` is a pointer file. The sandbox never lets a
/// `.git` path be written, so a sandboxed command can use git here and cannot repoint the tree at another repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Investigation {
    pub key: String,
    pub root: PathBuf,
    pub tree: PathBuf,
    pub git_dir: PathBuf,
    pub project: PathBuf,
    pub base_sha: String,
}

impl Worktrees {
    /// Where investigation copies live: beside the task worktrees, under the workspace.
    pub fn investigations_root(&self) -> PathBuf {
        self.root.join(".investigations")
    }

    /// Copy `project` at its `HEAD` into a new investigation named `key`. Every git call is the daemon's, hardened as for a task
    /// worktree, and all of them finish before any sandboxed command runs: the daemon never runs git here again.
    pub fn create_investigation(&self, project: &Path, key: &str) -> Result<Investigation, GitError> {
        self.create_investigation_at(project, key, None)
    }

    /// As [`Worktrees::create_investigation`], checked out at the project's branch `branch` (`refs/heads/<branch>`) instead of its head
    /// when one is named (`specs/135` §6.2 item 2: the review copy of a branch under review).
    pub fn create_investigation_at(&self, project: &Path, key: &str, branch: Option<&str>) -> Result<Investigation, GitError> {
        if !valid_task_id(key) {
            return Err(GitError::BadTaskId(key.into()));
        }
        if let Some(b) = branch.filter(|b| !valid_branch(b)) {
            return Err(GitError::Failed { args: "branch".into(), stderr: format!("`{}` is not a branch name Delphi accepts", b.chars().take(80).collect::<String>()) });
        }
        let home = self.home()?.to_path_buf();
        let project = std::fs::canonicalize(project).map_err(|e| GitError::Io(format!("{}: {}", project.display(), e)))?;
        let mut c = git_command(None, None, &home);
        c.arg("-C").arg(&project).args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
        let base_sha = run(c, "rev-parse HEAD").map_err(|_| GitError::NotAGitRepo(project.clone()))?.trim().to_string();
        let bad = unsafe_config(&project, &home)?;
        if !bad.is_empty() {
            return Err(GitError::UnsafeConfig(bad));
        }
        let io = |e: std::io::Error| GitError::Io(e.to_string());
        let root = self.investigations_root().join(key);
        if root.exists() {
            return Err(GitError::Exists(root));
        }
        std::fs::create_dir_all(root.join("tree")).map_err(io)?;
        let root = std::fs::canonicalize(&root).map_err(io)?;
        let (tree, git_dir) = (root.join("tree"), root.join("repo"));
        let made = (|| {
            // No template: no sample hooks, no description, nothing from the machine's git installation.
            let mut c = git_command(None, None, &home);
            c.args(["init", "--quiet", "--template="]).arg("--separate-git-dir").arg(&git_dir).arg(&tree);
            run(c, "init")?;
            // The project's branches and tags, as they are now, and its head (which may be on no branch). The one place the
            // hardening's ban on the local transport is lifted: the address is the registered project, built here, and
            // submodules are not followed. The project is only read.
            let mut c = git_command(Some(&git_dir), Some(&tree), &home);
            c.args(["-c", "protocol.file.allow=always", "fetch", "--quiet", "--no-recurse-submodules", "--no-write-fetch-head", "--update-head-ok"]).arg(&project).args(["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*", "+HEAD:refs/delphi/base"]);
            run(c, "fetch")?;
            let at = match branch {
                None => base_sha.clone(),
                Some(b) => {
                    let mut c = git_command(Some(&git_dir), Some(&tree), &home);
                    c.args(["rev-parse", "--verify", "--quiet"]).arg(format!("refs/heads/{}^{{commit}}", b));
                    run(c, "rev-parse branch").map_err(|_| GitError::Failed { args: "branch".into(), stderr: format!("the project has no branch `{}`", b) })?.trim().to_string()
                }
            };
            let mut c = git_command(Some(&git_dir), Some(&tree), &home);
            c.args(["checkout", "--quiet", "--force", "--detach", &at]);
            run(c, "checkout").map(|_| at)
        })();
        let base_sha = match &made {
            Ok(at) => at.clone(),
            Err(_) => base_sha,
        };
        let made = made.map(|_| ());
        let inv = Investigation { key: key.into(), root, tree, git_dir, project, base_sha };
        if let Err(e) = made {
            inv.remove();
            return Err(e);
        }
        Ok(inv)
    }

    /// The stat and the diff of `branch` against `base` (`base...branch`, from their merge base) in an investigation copy, by the
    /// daemon's git (`specs/135` §6.2 item 2). Called before any sandboxed command runs in the copy, so no command can change what
    /// it reads. No external diff driver and no textconv run.
    pub fn investigation_diff(&self, inv: &Investigation, base: &str, branch: &str) -> Result<(String, String), GitError> {
        for b in [base, branch] {
            if !valid_branch(b) {
                return Err(GitError::Failed { args: "branch".into(), stderr: format!("`{}` is not a branch name Delphi accepts", b.chars().take(80).collect::<String>()) });
            }
        }
        let home = self.home()?.to_path_buf();
        let range = format!("refs/heads/{}...refs/heads/{}", base, branch);
        let diff = |stat: bool| -> Result<String, GitError> {
            let mut c = git_command(Some(&inv.git_dir), Some(&inv.tree), &home);
            c.args(["diff", "--no-color", "--no-ext-diff", "--no-textconv"]);
            if stat {
                c.arg("--stat=200");
            }
            c.arg(&range).arg("--");
            run(c, if stat { "diff --stat" } else { "diff" })
        };
        Ok((diff(true)?, diff(false)?))
    }

    /// Delete every investigation copy `live` does not name: what a daemon that died left behind.
    pub fn sweep_investigations(&self, live: &dyn Fn(&str) -> bool) {
        for e in std::fs::read_dir(self.investigations_root()).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !live(&name) {
                force_remove(&e.path());
            }
        }
    }
}

impl Investigation {
    /// Delete the copy. No git runs: the directory is removed as files, whatever a command left in it.
    pub fn remove(&self) {
        force_remove(&self.root);
    }
}

/// Remove a directory tree even when something in it was made unreadable or unwritable.
pub(crate) fn force_remove(dir: &Path) {
    if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        if let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) {
            open_up_at(libc::AT_FDCWD, &c, 0);
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Make the directory `name` (relative to the open directory `parent`) and everything under it searchable and writable by its
/// owner, so it can be removed. A process that outlived its command may still be swapping entries for symlinks while this runs,
/// so nothing here follows one or acts on a path twice: the mode is set with `AT_SYMLINK_NOFOLLOW` (on a symlink it touches the
/// link, or fails), the directory is then opened `O_NOFOLLOW | O_DIRECTORY` and set again through that descriptor, and its entries
/// are read from the descriptor and opened relative to it. A swap can only make an entry fail to open, never redirect a change
/// outside the tree.
#[cfg(unix)]
fn open_up_at(parent: libc::c_int, name: &std::ffi::CStr, depth: usize) {
    if depth > 256 {
        return;
    }
    // SAFETY: plain syscalls on a NUL-terminated name and descriptors this function owns; each descriptor is closed once.
    unsafe {
        let mut st: libc::stat = std::mem::zeroed();
        if libc::fstatat(parent, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) != 0 || st.st_mode & libc::S_IFMT != libc::S_IFDIR {
            return;
        }
        // A directory a moment ago. Swapped since for a symlink, this touches the link; for a hard link to a file, the command
        // could have changed that file through the link itself, since its sandbox allows writes by path inside the copy.
        libc::fchmodat(parent, name.as_ptr(), 0o700, libc::AT_SYMLINK_NOFOLLOW);
        let fd = libc::openat(parent, name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        if fd < 0 {
            return;
        }
        libc::fchmod(fd, 0o700);
        let mut names = Vec::new();
        let dup = libc::dup(fd);
        if dup >= 0 {
            let d = libc::fdopendir(dup);
            if d.is_null() {
                libc::close(dup);
            } else {
                loop {
                    let ent = libc::readdir(d);
                    if ent.is_null() {
                        break;
                    }
                    let n = std::ffi::CStr::from_ptr((*ent).d_name.as_ptr());
                    if n.to_bytes() != b"." && n.to_bytes() != b".." {
                        names.push(n.to_owned());
                    }
                }
                libc::closedir(d);
            }
        }
        for n in names {
            open_up_at(fd, &n, depth + 1);
        }
        libc::close(fd);
    }
}


/// What the daemon's push is checked against (`specs/141` §2.4 item 6): the Delphi home (its library), the workspace (its handling
/// records and `[handling] internal_remotes`), and the label line the operator's card showed, if any.
#[derive(Debug, Clone, Default)]
pub struct PushGuard {
    pub home: PathBuf,
    pub ws: Option<PathBuf>,
    pub card_line: Option<String>,
}

// ---- the repository sink (`specs/141` §2.4, the repository row; slice D) ----------------------------------------------------------

/// Which files of a repository `delphi handling check` reads.
pub enum Pick<'a> {
    /// The index: what the next commit would hold.
    Staged,
    /// `<a>..<b>`: the files changed between the two, as they are at `<b>` (deleted files are not read).
    Range(&'a str),
    /// Files or directories, as they are on disk, relative to the directory the check runs in.
    Paths(&'a [String]),
}

/// Files by name, with their bytes.
pub type Files = Vec<(String, Vec<u8>)>;

/// An empty `HOME` for the git calls of the check: no user config is read.
const NO_GIT_HOME: &str = "/nonexistent-delphi-git-home";

/// The files `pick` names in the repository at (or above) `dir`, each by its path relative to the returned root, with its bytes.
/// Git runs with the daemon's hardening (no hooks, no user or system config, no external diff); nothing is written.
pub fn repo_files(dir: &Path, pick: &Pick<'_>) -> Result<(PathBuf, Files), String> {
    let git = |args: &[&str], label: &str| -> Result<String, String> {
        let mut c = git_command(None, None, Path::new(NO_GIT_HOME));
        c.arg("-C").arg(dir).args(args);
        run(c, label).map_err(|e| e.to_string())
    };
    let (names, rev) = match pick {
        Pick::Paths(paths) => {
            let mut out = Vec::new();
            for p in paths.iter() {
                let path = dir.join(p);
                if path.is_dir() {
                    let base = p.trim_end_matches('/');
                    out.extend(crate::held_text::files_under(&path).into_iter().map(|(n, b)| (format!("{base}/{n}"), b)));
                } else {
                    out.push((p.clone(), std::fs::read(&path).map_err(|e| format!("{p}: {e}"))?));
                }
            }
            return Ok((dir.to_path_buf(), out));
        }
        Pick::Staged => (git(&["diff", "--cached", "--name-only", "-z", "--no-renames", "--diff-filter=d"], "diff --cached")?, String::new()),
        Pick::Range(r) => {
            let (a, b) = r.split_once("..").filter(|(a, b)| !a.is_empty() && !b.is_empty() && !b.starts_with('.')).ok_or_else(|| format!("`{r}` is not a range (write <a>..<b>)"))?;
            if [a, b].iter().any(|x| x.starts_with('-')) {
                return Err(format!("`{r}` is not a range"));
            }
            (git(&["diff", "--name-only", "-z", "--no-renames", "--diff-filter=d", a, b], "diff range")?, b.to_string())
        }
    };
    let root = PathBuf::from(git(&["rev-parse", "--show-toplevel"], "rev-parse")?.trim());
    let mut out = Vec::new();
    for name in names.split('\0').filter(|n| !n.is_empty()) {
        let mut c = git_command(None, None, Path::new(NO_GIT_HOME));
        c.arg("-C").arg(&root).args(["show", &format!("{rev}:{name}")]);
        let o = c.output().map_err(|e| format!("cannot run git: {e}"))?;
        if !o.status.success() {
            return Err(format!("git show {rev}:{name} failed"));
        }
        out.push((name.to_string(), o.stdout));
    }
    Ok((root, out))
}

/// Where git runs the pre-push hook of the repository at (or above) `dir` (`specs/141` §2.4 item 10): `core.hooksPath` from the
/// repository's own config when it is set (relative to the top of the work tree), else `hooks/` in its common git directory. `None`
/// outside a repository. A `core.hooksPath` set in the operator's global config is not read here (no user config is), so a hook
/// written for a repository that relies on one is not where git looks; `delphi handling hook` says where it wrote.
pub fn pre_push_hook_path(dir: &Path) -> Option<PathBuf> {
    let git = |args: &[&str]| -> Option<String> {
        let mut c = git_command(None, None, Path::new(NO_GIT_HOME));
        c.arg("-C").arg(dir).args(args);
        run(c, "rev-parse").ok().map(|o| o.trim().to_string()).filter(|o| !o.is_empty())
    };
    let common = PathBuf::from(git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?);
    let config = common.join("config");
    let hooks = match git(&["config", "--file", &config.to_string_lossy(), "--get", "core.hooksPath"]) {
        Some(h) if Path::new(&h).is_absolute() => PathBuf::from(h),
        Some(h) => PathBuf::from(git(&["rev-parse", "--show-toplevel"])?).join(h),
        None => common.join("hooks"),
    };
    Some(hooks.join("pre-push"))
}

/// The repository sink of `specs/141` §2.4 (item 6): `files` (named relative to `root`) as one output, each labelled from the daemon's
/// records and scanned for held text, decided at the repository row for `remote`. An `internal` file goes only to a remote on the
/// workspace's `[handling] internal_remotes`, labelled and confirmed; every other mark but `text_held` is refused at every remote; held
/// text is bounded by `body_may_leave`. The refusal names the files and the marks, never any text.
pub fn repository_check(home: &Path, ws: Option<&Path>, root: &Path, files: &[(String, Vec<u8>)], remote: Option<&str>, confirm: crate::egress::Confirm<'_>) -> Result<crate::egress::Sent, String> {
    let internal = remote.is_some_and(|r| ws.map(crate::handling::config_of).unwrap_or_default().is_internal_remote(r));
    let sink = if internal { crate::egress::Sink::Repository { internal_remote: true } } else { crate::egress::Sink::Repository { internal_remote: false } };
    crate::egress::check_output(home, ws, Some(root), files, sink, confirm)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;

    /// Turn an existing directory of files into a repository with one commit, using the same hardened calls.
    pub(crate) fn init_repo(p: &Path, scratch_home: &Path) {
        fs::create_dir_all(scratch_home).unwrap();
        for args in [vec!["init", "--quiet", "-b", "main"], vec!["add", "--all"], vec!["commit", "--quiet", "-m", "init"]] {
            let mut c = git_command(None, None, scratch_home);
            c.arg("-C").arg(p).args(&args);
            run(c, "setup").unwrap();
        }
    }

    /// A project repository with one commit.
    pub(crate) fn project(dir: &Path) -> PathBuf {
        let p = dir.join("proj");
        fs::create_dir_all(p.join("src")).unwrap();
        fs::write(p.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        fs::write(p.join("package.json"), "{\"name\":\"x\",\"scripts\":{\"test\":\"true\"},\"dependencies\":{}}\n").unwrap();
        init_repo(&p, &dir.join("gh"));
        p
    }

    fn fx() -> (tempfile::TempDir, PathBuf, Worktrees) {
        let d = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let p = project(&root);
        let w = Worktrees::new(&root.join("ws/worktrees"));
        (d, p, w)
    }

    #[test]
    fn a_worktree_is_cut_from_head_outside_the_project_and_leaves_it_clean() {
        let (_d, p, w) = fx();
        let wt = w.create(&p, "t_0001").unwrap();
        assert!(wt.path.starts_with(fs::canonicalize(&w.root).unwrap()) && !wt.path.starts_with(&p));
        assert_eq!((wt.branch.as_str(), wt.project_was_dirty), ("delphi/t_0001", false));
        assert!(wt.path.join("src/lib.rs").exists() && wt.path.join(".git").is_file());
        // The project checkout itself is untouched: no new files, no new branch checked out.
        let mut c = git_command(None, None, &wt.scratch_home);
        c.arg("-C").arg(&p).args(["status", "--porcelain"]);
        assert_eq!(run(c, "status").unwrap(), "");
        assert!(matches!(w.create(&p, "t_0001"), Err(GitError::Exists(_))));
        // After a daemon restart the same worktree is re-adopted from the record kept beside it.
        let again = Worktrees::new(&w.root).adopt("t_0001").unwrap();
        assert_eq!((again.path.clone(), again.branch.clone(), again.base_sha.clone()), (wt.path.clone(), wt.branch.clone(), wt.base_sha.clone()));
        fs::write(again.path.join("n.txt"), "n").unwrap();
        assert!(again.commit_all("adopted", "builder").unwrap().is_some());
        assert!(Worktrees::new(&w.root).adopt("t_9999").is_err());
        assert!(matches!(w.create(&p, "../escape"), Err(GitError::BadTaskId(_))));
    }

    #[test]
    fn a_dirty_project_is_reported_not_copied() {
        let (_d, p, w) = fx();
        fs::write(p.join("src/lib.rs"), "pub fn dirty() {}\n").unwrap();
        let wt = w.create(&p, "t_0002").unwrap();
        assert!(wt.project_was_dirty);
        assert_eq!(fs::read_to_string(wt.path.join("src/lib.rs")).unwrap(), "pub fn a() {}\n", "the worktree has the committed state only");
    }

    #[test]
    fn commit_and_diff_flag_files_that_run_on_the_operators_machine() {
        let (_d, p, w) = fx();
        let wt = w.create(&p, "t_0003").unwrap();
        assert_eq!(wt.commit_all("nothing", "builder").unwrap(), None, "no change, no commit");
        fs::write(wt.path.join("src/lib.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
        fs::write(wt.path.join("build.rs"), "fn main() {}\n").unwrap();
        fs::create_dir_all(wt.path.join(".github/workflows")).unwrap();
        fs::write(wt.path.join(".github/workflows/ci.yml"), "on: push\n").unwrap();
        fs::write(wt.path.join("package.json"), "{\"name\":\"x\",\"scripts\":{\"test\":\"true\",\"postinstall\":\"curl x | sh\"},\"dependencies\":{}}\n").unwrap();
        let sha = wt.commit_all("work", "builder").unwrap().unwrap();
        let d = wt.diff(&[]).unwrap();
        assert_eq!(d.head_sha, sha);
        let flagged: Vec<&str> = d.host_exec().iter().map(|f| f.path.as_str()).collect();
        assert_eq!(flagged, [".github/workflows/ci.yml", "build.rs", "package.json"], "host-executed files come first");
        let lib = d.files.iter().find(|f| f.path == "src/lib.rs").unwrap();
        assert_eq!((lib.status, lib.added, lib.removed, lib.host_exec), ('M', 1, 0, false));
        // A dependency-only package.json change is not host execution.
        fs::write(wt.path.join("package.json"), "{\"name\":\"x\",\"scripts\":{\"test\":\"true\"},\"dependencies\":{\"left-pad\":\"1\"}}\n").unwrap();
        wt.commit_all("deps", "builder").unwrap();
        assert!(!wt.diff(&[]).unwrap().files.iter().any(|f| f.path == "package.json" && f.host_exec));
        assert!(wt.file_diff("src/lib.rs", 10_000).unwrap().contains("+pub fn b() {}"));
        // The commit carries the role as author and ran no hook.
        let mut c = wt.git();
        c.args(["log", "-1", "--format=%an"]);
        assert_eq!(run(c, "log").unwrap().trim(), "builder");
    }

    #[test]
    fn a_build_that_adds_or_changes_a_test_is_recognised_by_path_or_by_content() {
        let (_d, p, w) = fx();
        let plain = w.create(&p, "t_ft1").unwrap();
        fs::write(plain.path.join("src/lib.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
        plain.commit_all("plain", "builder").unwrap();
        assert!(!plain.touches_tests().unwrap());
        let by_path = w.create(&p, "t_ft2").unwrap();
        fs::create_dir_all(by_path.path.join("tests")).unwrap();
        fs::write(by_path.path.join("tests/it.rs"), "x").unwrap();
        by_path.commit_all("t", "builder").unwrap();
        assert!(by_path.touches_tests().unwrap());
        let by_content = w.create(&p, "t_ft3").unwrap();
        fs::write(by_content.path.join("src/lib.rs"), "pub fn a() {}\n#[cfg(test)]\nmod t { #[test] fn ok() {} }\n").unwrap();
        by_content.commit_all("t", "builder").unwrap();
        assert!(by_content.touches_tests().unwrap());
    }

    #[test]
    fn symlinks_and_submodule_entries_get_the_host_execution_badge() {
        let (_d, p, w) = fx();
        let wt = w.create(&p, "t_sl").unwrap();
        std::os::unix::fs::symlink("/etc/hosts", wt.path.join("innocent.txt")).unwrap();
        fs::write(wt.path.join("plain.txt"), "ok").unwrap();
        wt.commit_all("links", "builder").unwrap().unwrap();
        let d = wt.diff(&[]).unwrap();
        let link = d.files.iter().find(|f| f.path == "innocent.txt").unwrap();
        assert_eq!((link.host_exec, link.special), (true, Some("symlink")));
        let plain = d.files.iter().find(|f| f.path == "plain.txt").unwrap();
        assert_eq!((plain.host_exec, plain.special), (false, None));
        assert_eq!(d.files[0].path, "innocent.txt", "flagged entries are listed first");
    }

    #[test]
    fn the_configurable_host_exec_list_extends_the_defaults() {
        let extra = vec!["tools/deploy".to_string(), "*.sh".to_string()];
        assert!(is_host_exec("tools/deploy/run.py", &extra) && is_host_exec("scripts/x.sh", &extra));
        assert!(!is_host_exec("tools/deployment/x", &extra) && !is_host_exec("src/main.rs", &extra));
        for p in ["Makefile", "sub/Makefile", ".envrc", ".gitattributes", ".gitmodules", "rust-toolchain.toml", ".cargo/config.toml", "crates/a/build.rs", ".vscode/tasks.json", ".idea/x.xml"] {
            assert!(is_host_exec(p, &[]), "{}", p);
        }
    }

    #[test]
    fn hooks_in_the_project_never_run_for_daemon_commits() {
        let (_d, p, w) = fx();
        let hook = p.join(".git/hooks/pre-commit");
        let marker = p.parent().unwrap().join("HOOK-RAN");
        fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let wt = w.create(&p, "t_0004").unwrap();
        fs::write(wt.path.join("x.txt"), "x").unwrap();
        wt.commit_all("c", "builder").unwrap().unwrap();
        assert!(!marker.exists(), "a pre-commit hook ran");
    }

    #[test]
    fn a_project_with_content_filters_or_includes_is_refused() {
        let (_d, p, w) = fx();
        let home = w.scratch_home.clone();
        fs::create_dir_all(&home).unwrap();
        for (k, v) in [("filter.evil.clean", "touch /tmp/x"), ("include.path", "/tmp/evil.cfg")] {
            let mut c = git_command(None, None, &home);
            c.arg("-C").arg(&p).args(["config", "--local", k, v]);
            run(c, "config").unwrap();
        }
        match w.create(&p, "t_0005") {
            Err(GitError::UnsafeConfig(keys)) => assert_eq!(keys, ["filter.evil.clean", "include.path"]),
            other => panic!("{:?}", other),
        }
    }

    fn built(w: &Worktrees, p: &Path, id: &str, file: &str, content: &str) -> (Worktree, String) {
        let wt = w.create(p, id).unwrap();
        fs::write(wt.path.join(file), content).unwrap();
        let sha = wt.commit_all("work", "builder").unwrap().unwrap();
        (wt, sha)
    }

    #[test]
    fn restore_to_base_puts_a_file_back_removes_a_new_one_and_refuses_odd_paths() {
        let (_d, p, w) = fx();
        let wt = w.create(&p, "t_r1").unwrap();
        fs::write(wt.path.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
        fs::write(wt.path.join("src/new.rs"), "pub fn n() {}\n").unwrap();
        wt.commit_all("work", "builder").unwrap().unwrap();
        assert_eq!(wt.restore_to_base("src/lib.rs").unwrap(), "base");
        assert_eq!(wt.restore_to_base("src/new.rs").unwrap(), "removed");
        assert_eq!(fs::read_to_string(wt.path.join("src/lib.rs")).unwrap(), "pub fn a() {}\n");
        assert!(!wt.path.join("src/new.rs").exists());
        assert!(wt.commit_all("revert", "delphi").unwrap().is_some(), "the restore is a change to commit");
        assert!(wt.diff(&[]).unwrap().files.is_empty(), "nothing differs from the base any more");
        assert_eq!(wt.restore_to_base("src/missing.rs").unwrap(), "removed", "a file that was never there is simply absent");
        for bad in ["", "/etc/passwd", "../x", "src/../../x", ".git/config", "src/.GIT/x", "a//b", "./x"] {
            assert!(wt.restore_to_base(bad).is_err(), "{:?}", bad);
        }
    }

    /// `specs/142` AC-9, the daemon's push: a branch that adds a file quoting 400 characters of a held source is refused before the
    /// push and the remote gains no branch; with no held source in the home the same branch is pushed (the pull request then fails, as
    /// there is no `gh`, which is after the push).
    #[test]
    fn a_push_whose_range_quotes_a_held_source_is_refused_and_nothing_is_pushed() {
        let (d, p, w) = fx();
        let remote = d.path().join("remote.git");
        let mut c = git_command(None, None, &w.scratch_home);
        c.args(["init", "--quiet", "--bare"]).arg(&remote);
        run(c, "init bare").unwrap();
        let mut c = git_command(None, None, &w.scratch_home);
        c.arg("-C").arg(&p).args(["remote", "add", "origin"]).arg(&remote);
        run(c, "remote add").unwrap();
        let text: String = (0..300).map(|i| format!("Held sentence {i:03} runs on and on. ")).collect();
        let home = d.path().join("home");
        let held = home.join("library/sources/held-a");
        fs::create_dir_all(&held).unwrap();
        fs::write(held.join("meta.json"), serde_json::json!({"type": "webpage", "title": "Held", "licence": "unknown", "capture_level": "full", "text_held": true}).to_string()).unwrap();
        fs::write(held.join("source.md"), &text).unwrap();
        let quote: String = text.chars().skip(20).take(400).collect();
        let (wt, sha) = built(&w, &p, "t_push", "notes.md", &format!("# Notes\n\n{quote}\n"));
        let branches = || {
            let mut c = git_command(None, None, &w.scratch_home);
            c.arg("--git-dir").arg(&remote).args(["branch", "--list"]);
            run(c, "branch list").unwrap()
        };
        match wt.open_pr(&sha, "t", "b", "/nonexistent-gh", Some(&PushGuard { home: home.clone(), ..Default::default() })).unwrap() {
            PrOutcome::Refused(why) => assert!(why.contains("s-held-a") && why.contains("notes.md") && why.contains("nothing was pushed"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(branches().trim().is_empty(), "nothing reached the remote: {}", branches());
        let empty = d.path().join("empty-home");
        let _ = wt.open_pr(&sha, "t", "b", "/nonexistent-gh", Some(&PushGuard { home: empty.clone(), ..Default::default() })).unwrap();
        assert!(branches().contains("delphi/t_push"), "with nothing held the branch is pushed");
    }

    /// `specs/141` AC-8, the daemon's push (the repository row, slice D). A branch that adds a copy of a `personal` page is refused by
    /// the check the card runs first (`push_check`) and again by the push itself, naming the file and the mark; the remote gains no
    /// branch. A copy of an `internal` page is refused while `origin` is not on `[handling] internal_remotes`; once it is, it is pushed
    /// only when the card the operator answered showed the label's line.
    #[test]
    fn a_push_whose_range_holds_a_labelled_page_is_refused_before_the_card_and_after_it() {
        use crate::egress::sink_d_tests::{Fx, INTERNAL_REMOTE};
        use crate::label::Mark;
        let fx = Fx::new();
        let (d, p, w) = fx_with_remote();
        let remote = d.path().join("remote.git");
        let branches = || {
            let mut c = git_command(None, None, &w.scratch_home);
            c.arg("--git-dir").arg(&remote).args(["branch", "--list"]);
            run(c, "branch list").unwrap()
        };
        let guard = |card_line: Option<String>| PushGuard { home: fx.home.clone(), ws: Some(fx.ws.clone()), card_line };
        // personal: refused before the card and by the push
        let id = fx.source("p.md", "A personal note.\n", Mark::Personal);
        let page = fx.page("harbour-p", &[&id]);
        let (wt, sha) = built(&w, &p, "t_lab1", "harbour.md", &page);
        let e = wt.push_check(&guard(None)).unwrap().unwrap_err();
        assert!(e.contains("harbour.md") && e.contains("private") && !e.contains("Line 03"), "{e}");
        match wt.open_pr(&sha, "t", "b", "/nonexistent-gh", Some(&guard(None))).unwrap() {
            PrOutcome::Refused(why) => assert!(why.contains("harbour.md") && why.contains("nothing was pushed"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(branches().trim().is_empty(), "nothing reached the remote");
        // internal: refused while origin is not on the list
        let id = fx.source("i.md", "An internal note.\n", Mark::Internal);
        let page = fx.page("harbour-i", &[&id]);
        let (wt, sha) = built(&w, &p, "t_lab2", "harbour.md", &page);
        assert!(wt.push_check(&guard(None)).unwrap().is_err(), "{INTERNAL_REMOTE} is listed, origin is not");
        // listed: the card shows the line, and the push goes ahead only on a card that showed it
        fs::write(fx.ws.join("config.toml"), format!("[handling]\ninternal_remotes = [\"{}\"]\n", remote.display())).unwrap();
        let sent = wt.push_check(&guard(None)).unwrap().unwrap();
        assert_eq!(sent.decision.kind, crate::label::Kind::SendLabelled);
        let line = sent.line();
        assert!(line.starts_with("> Handling: INTERNAL"), "{line}");
        match wt.open_pr(&sha, "t", "b", "/nonexistent-gh", Some(&guard(None))).unwrap() {
            PrOutcome::Refused(why) => assert!(why.contains("did not show that label"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(branches().trim().is_empty(), "nothing reached the remote");
        let _ = wt.open_pr(&sha, "t", "b", "/nonexistent-gh", Some(&guard(Some(line)))).unwrap();
        assert!(branches().contains("delphi/t_lab2"), "the card showed the line: pushed");
    }

    /// A worktree fixture whose project has a bare `origin`.
    fn fx_with_remote() -> (tempfile::TempDir, PathBuf, Worktrees) {
        let (d, p, w) = fx();
        let remote = d.path().join("remote.git");
        let mut c = git_command(None, None, &w.scratch_home);
        c.args(["init", "--quiet", "--bare"]).arg(&remote);
        run(c, "init bare").unwrap();
        let mut c = git_command(None, None, &w.scratch_home);
        c.arg("-C").arg(&p).args(["remote", "add", "origin"]).arg(&remote);
        run(c, "remote add").unwrap();
        (d, p, w)
    }

    #[test]
    fn merge_back_merges_an_approved_commit_into_the_operators_branch() {
        let (_d, p, w) = fx();
        let (wt, sha) = built(&w, &p, "t_m1", "src/new.rs", "pub fn n() {}\n");
        match wt.merge_back(&sha, "Merge delphi/t_m1", &[]).unwrap() {
            MergeOutcome::Merged { commit } => assert_ne!(commit, sha, "a --no-ff merge commit"),
            other => panic!("{:?}", other),
        }
        assert_eq!(fs::read_to_string(p.join("src/new.rs")).unwrap(), "pub fn n() {}\n");
    }

    #[test]
    fn merge_back_refuses_a_moved_branch_dirty_overlap_and_detached_heads_without_touching_the_project() {
        let (_d, p, w) = fx();
        let (wt, sha) = built(&w, &p, "t_m2", "src/lib.rs", "pub fn changed() {}\n");
        // The branch moves after approval.
        fs::write(wt.path.join("extra.txt"), "x").unwrap();
        wt.commit_all("more", "builder").unwrap();
        assert!(matches!(wt.merge_back(&sha, "m", &[]).unwrap(), MergeOutcome::Refused(m) if m.contains("moved")));
        let tip = wt.head().unwrap();
        // The operator has uncommitted edits to a file the merge touches.
        fs::write(p.join("src/lib.rs"), "pub fn mine() {}\n").unwrap();
        match wt.merge_back(&tip, "m", &[]).unwrap() {
            MergeOutcome::Refused(m) => assert!(m.contains("uncommitted changes") && m.contains("src/lib.rs"), "{}", m),
            other => panic!("{:?}", other),
        }
        assert_eq!(fs::read_to_string(p.join("src/lib.rs")).unwrap(), "pub fn mine() {}\n", "the operator's edit is untouched");
        // Detached HEAD.
        let mut c = git_command(None, None, &wt.scratch_home);
        c.arg("-C").arg(&p).args(["checkout", "--quiet", "--detach"]).env("GIT_COMMITTER_NAME", "x");
        // (checkout needs a clean tree for the touched file; restore it first)
        let mut r = git_command(None, None, &wt.scratch_home);
        r.arg("-C").arg(&p).args(["checkout", "--quiet", "--", "src/lib.rs"]);
        run(r, "restore").unwrap();
        run(c, "detach").unwrap();
        assert!(matches!(wt.merge_back(&tip, "m", &[]).unwrap(), MergeOutcome::Refused(m) if m.contains("detached")));
    }

    #[test]
    fn a_conflicting_merge_is_aborted_and_leaves_the_project_as_it_was() {
        let (_d, p, w) = fx();
        let (wt, sha) = built(&w, &p, "t_m3", "src/lib.rs", "pub fn theirs() {}\n");
        fs::write(p.join("src/lib.rs"), "pub fn ours() {}\n").unwrap();
        for args in [vec!["add", "--all"], vec!["commit", "--quiet", "-m", "ours"]] {
            let mut c = git_command(None, None, &wt.scratch_home);
            c.arg("-C").arg(&p).args(&args);
            run(c, "ours").unwrap();
        }
        match wt.merge_back(&sha, "m", &[]).unwrap() {
            MergeOutcome::Conflict(files) => assert_eq!(files, ["src/lib.rs"]),
            other => panic!("{:?}", other),
        }
        assert_eq!(fs::read_to_string(p.join("src/lib.rs")).unwrap(), "pub fn ours() {}\n");
        assert!(!p.join(".git/MERGE_HEAD").exists());
    }

    #[test]
    fn removal_leaves_no_branch_or_metadata_in_the_project() {
        let (_d, p, w) = fx();
        let wt = w.create(&p, "t_0006").unwrap();
        wt.remove().unwrap();
        assert!(!wt.path.exists());
        let mut c = git_command(None, None, &wt.scratch_home);
        c.arg("-C").arg(&p).args(["branch", "--list", "delphi/*"]);
        assert_eq!(run(c, "branch").unwrap().trim(), "");
        assert!(!p.join(".git/worktrees/t_0006").exists());
    }

    #[test]
    fn a_non_repository_or_an_empty_one_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let w = Worktrees::new(&d.path().join("wt"));
        assert!(matches!(w.create(d.path(), "t_1"), Err(GitError::NotAGitRepo(_))));
    }

    /// Every file under `dir` (the `.git` directory included) with its bytes, and every directory and symlink by name: what
    /// "byte-identical" is checked against.
    pub(crate) fn tree_bytes(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        fn walk(d: &Path, root: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
            for e in fs::read_dir(d).unwrap().flatten() {
                let (p, ft) = (e.path(), e.file_type().unwrap());
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                if ft.is_dir() {
                    out.insert(format!("{rel}/"), vec![]);
                    walk(&p, root, out);
                } else if ft.is_symlink() {
                    out.insert(format!("{rel}@"), fs::read_link(&p).unwrap().to_string_lossy().into_owned().into_bytes());
                } else {
                    out.insert(rel, fs::read(&p).unwrap());
                }
            }
        }
        let mut out = Default::default();
        walk(dir, dir, &mut out);
        out
    }

    /// The parts of [`tree_bytes`] the spec names: the index and the refs (loose, packed and `HEAD`).
    pub(crate) fn index_and_refs(all: &std::collections::BTreeMap<String, Vec<u8>>) -> Vec<(&String, &Vec<u8>)> {
        all.iter().filter(|(k, _)| *k == ".git/index" || *k == ".git/HEAD" || *k == ".git/packed-refs" || k.starts_with(".git/refs/") || k.starts_with(".git/logs/") || k.starts_with(".git/worktrees")).collect()
    }

    /// Commit everything in `p` with `message`, by the same hardened calls; returns the commit.
    pub(crate) fn commit(p: &Path, scratch_home: &Path, message: &str) -> String {
        for args in [vec!["add", "--all"], vec!["commit", "--quiet", "-m", message]] {
            let mut c = git_command(None, None, scratch_home);
            c.arg("-C").arg(p).args(&args);
            run(c, "setup").unwrap();
        }
        let mut c = git_command(None, None, scratch_home);
        c.arg("-C").arg(p).args(["rev-parse", "HEAD"]);
        run(c, "rev-parse").unwrap().trim().to_string()
    }

    /// `specs/116` §8.1 items 2 and 4: an investigation copy is a repository of its own, detached at the project's head, with no
    /// remote; making it, using it and deleting it leave the project's working tree, index and refs byte-identical.
    #[test]
    fn an_investigation_copy_is_its_own_repository_and_the_project_is_byte_identical_before_and_after() {
        let (_d, p, w) = fx();
        let home = p.parent().unwrap().join("gh");
        fs::write(p.join("src/lib.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
        let head = commit(&p, &home, "second");
        let mut c = git_command(None, None, &home);
        c.arg("-C").arg(&p).args(["tag", "v1"]);
        run(c, "tag").unwrap();
        // The operator's own uncommitted work and an untracked file: neither is copied, neither is touched.
        fs::write(p.join("src/lib.rs"), "pub fn dirty() {}\n").unwrap();
        fs::write(p.join("notes.txt"), "mine").unwrap();
        let before = tree_bytes(&p);
        assert!(before.contains_key(".git/index") && before.contains_key(".git/HEAD"));

        let inv = w.create_investigation(&p, "i_0001").unwrap();
        assert!(inv.root.starts_with(fs::canonicalize(w.investigations_root()).unwrap()) && !inv.root.starts_with(&p), "under the workspace's worktree folder, not in the project");
        assert_eq!(inv.base_sha, head);
        assert_eq!(fs::read_to_string(inv.tree.join("src/lib.rs")).unwrap(), "pub fn a() {}\npub fn b() {}\n", "the committed state at the head");
        assert!(!inv.tree.join("notes.txt").exists());
        let gitfile = fs::read_to_string(inv.tree.join(".git")).unwrap();
        assert_eq!(gitfile.trim(), format!("gitdir: {}", inv.git_dir.display()), "the tree points at its own repository, not the project's");
        assert!(!inv.git_dir.components().any(|c| c.as_os_str() == ".git"), "the copy's git directory has no `.git` segment, so a sandboxed git can write it");
        let git = |args: &[&str]| {
            let mut c = git_command(Some(&inv.git_dir), Some(&inv.tree), &home);
            c.args(args);
            run(c, "check")
        };
        assert_eq!(git(&["rev-parse", "HEAD"]).unwrap().trim(), head);
        assert!(git(&["symbolic-ref", "-q", "HEAD"]).is_err(), "detached: it is on no branch");
        assert_eq!(git(&["remote"]).unwrap(), "", "no remote: nothing to push to or fetch from");
        let cfg = fs::read_to_string(inv.git_dir.join("config")).unwrap();
        assert!(!cfg.contains("url") && !cfg.contains(&p.display().to_string()), "the project's address is not kept: {cfg}");
        assert!(!inv.git_dir.join("objects/info/alternates").exists(), "its objects are its own, not borrowed from the project");
        assert!(!inv.git_dir.join("hooks").exists(), "no hooks came with it");
        assert_eq!(git(&["rev-parse", "v1"]).unwrap().trim(), head, "tags and branches came along, so any commit can be checked out");
        assert_eq!(git(&["rev-parse", "main~1"]).unwrap().trim().len(), 40);
        assert_eq!(tree_bytes(&p), before, "making the copy changed nothing in the project, its .git included");

        // What an investigation does: another commit checked out, a commit of its own, a bisect ref, a scratch file.
        git(&["checkout", "--quiet", "--detach", "main~1"]).unwrap();
        fs::write(inv.tree.join("scratch.txt"), "x").unwrap();
        git(&["add", "--all"]).unwrap();
        git(&["commit", "--quiet", "-m", "scratch"]).unwrap();
        git(&["update-ref", "refs/bisect/bad", "HEAD"]).unwrap();
        assert_eq!(tree_bytes(&p), before, "nothing done in the copy reaches the project");

        assert!(matches!(w.create_investigation(&p, "i_0001"), Err(GitError::Exists(_))));
        assert!(matches!(w.create_investigation(&p, "../escape"), Err(GitError::BadTaskId(_))));
        inv.remove();
        assert!(!inv.root.exists(), "the copy is gone");
        let after = tree_bytes(&p);
        assert_eq!(index_and_refs(&after), index_and_refs(&before), "index and refs");
        assert_eq!(after, before, "and every other byte of the project");
    }

    #[test]
    fn a_leftover_investigation_copy_is_swept_and_a_locked_down_one_is_still_removed() {
        let (_d, p, w) = fx();
        let (stale, live) = (w.create_investigation(&p, "i_stale").unwrap(), w.create_investigation(&p, "i_live").unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::create_dir_all(stale.tree.join("locked/inner")).unwrap();
            fs::write(stale.tree.join("locked/inner/f"), "x").unwrap();
            fs::set_permissions(stale.tree.join("locked/inner"), fs::Permissions::from_mode(0o000)).unwrap();
            fs::set_permissions(stale.tree.join("locked"), fs::Permissions::from_mode(0o500)).unwrap();
        }
        w.sweep_investigations(&|k| k == "i_live");
        assert!(!stale.root.exists() && live.root.exists());
        // An unsafe project config is refused, as for a task worktree.
        let mut c = git_command(None, None, &p.parent().unwrap().join("gh"));
        c.arg("-C").arg(&p).args(["config", "filter.x.clean", "cat"]);
        run(c, "config").unwrap();
        assert!(matches!(w.create_investigation(&p, "i_unsafe"), Err(GitError::UnsafeConfig(_))));
        assert!(!w.investigations_root().join("i_unsafe").exists());
    }

    /// Red-team P1-2: removing a locked-down copy opens its directories up to delete them. A symlink in the copy to a directory
    /// outside, there from the start or swapped in for a directory while the removal runs (a process that outlived its command),
    /// never has the outside directory's mode changed.
    #[cfg(unix)]
    #[test]
    fn removing_a_locked_copy_never_changes_the_mode_of_a_directory_a_symlink_points_at() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let d = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let outside = root.join("outside");
        fs::create_dir_all(outside.join("inner")).unwrap();
        fs::set_permissions(outside.join("inner"), fs::Permissions::from_mode(0o500)).unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o555)).unwrap();
        let mode = |p: &Path| fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777;
        let copy = root.join("copy");
        let locked = |with_links: bool| {
            fs::create_dir_all(copy.join("tree/locked/deep")).unwrap();
            fs::write(copy.join("tree/locked/deep/f"), "x").unwrap();
            if with_links {
                symlink(&outside, copy.join("tree/link")).unwrap();
                symlink(&outside, copy.join("tree/locked/link")).unwrap();
            }
            fs::set_permissions(copy.join("tree/locked/deep"), fs::Permissions::from_mode(0o000)).unwrap();
            fs::set_permissions(copy.join("tree/locked"), fs::Permissions::from_mode(0o000)).unwrap();
        };
        // There from the start.
        locked(true);
        force_remove(&copy);
        assert!(!copy.exists(), "the locked copy is removed");
        assert_eq!((mode(&outside), mode(&outside.join("inner"))), (0o555, 0o500));
        // Swapped in while the removal runs: a directory and a symlink to the outside exchanged atomically at one name, over and
        // over, so the name is always there and is a directory one moment and the symlink the next.
        fn exchange(a: &Path, b: &Path) {
            use std::os::unix::ffi::OsStrExt;
            let (a, b) = (std::ffi::CString::new(a.as_os_str().as_bytes()).unwrap(), std::ffi::CString::new(b.as_os_str().as_bytes()).unwrap());
            // SAFETY: two NUL-terminated paths.
            #[cfg(target_os = "macos")]
            unsafe {
                libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP);
            }
            #[cfg(target_os = "linux")]
            unsafe {
                libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), 2 /* RENAME_EXCHANGE */);
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let swapper = {
            let (stop, tree, outside) = (stop.clone(), copy.join("tree"), outside.clone());
            std::thread::spawn(move || {
                let (swap, other) = (tree.join("swap"), tree.join("other"));
                while !stop.load(Ordering::Relaxed) {
                    if fs::symlink_metadata(&swap).is_err() || fs::symlink_metadata(&other).is_err() {
                        let _ = fs::remove_dir(&swap).or_else(|_| fs::remove_file(&swap));
                        let _ = fs::remove_dir(&other).or_else(|_| fs::remove_file(&other));
                        let _ = fs::create_dir(&swap);
                        let _ = symlink(&outside, &other);
                    }
                    exchange(&swap, &other);
                }
            })
        };
        for round in 0..1000 {
            locked(false);
            force_remove(&copy);
            assert_eq!(mode(&outside), 0o555, "round {round}: the outside directory's mode was changed through a swapped-in symlink");
        }
        stop.store(true, Ordering::Relaxed);
        swapper.join().unwrap();
        force_remove(&copy);
        assert!(!copy.exists());
        assert_eq!((mode(&outside), mode(&outside.join("inner"))), (0o555, 0o500));
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(outside.join("inner"), fs::Permissions::from_mode(0o755)).unwrap();
    }
}
