//! Is a project file still what the base commit has? (`specs/001` §5.1 origin defaults.)
//!
//! Tracked files unchanged since the base commit are `L4`; edits since are `L3`. This shells out to
//! `git`, hardened per `specs/001` §2.2: no hooks, no fsmonitor, no optional locks, no system config,
//! and no prompts. Only read-only commands are used.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;

pub trait ProjectProbe: Send + Sync {
    /// True if `rel` (relative to `root`) is tracked and unmodified relative to HEAD.
    fn clean_at_base(&self, root: &Path, rel: &str) -> bool;
}

/// Every file counts as clean (tests, or projects that aren't git repositories).
pub struct AssumeClean;

impl ProjectProbe for AssumeClean {
    fn clean_at_base(&self, _root: &Path, _rel: &str) -> bool {
        true
    }
}

#[derive(Default)]
pub struct GitProbe {
    cache: Mutex<HashMap<(String, String), bool>>,
}

impl GitProbe {
    pub fn new() -> GitProbe {
        GitProbe::default()
    }

    fn git(root: &Path, args: &[&str]) -> Option<(bool, String)> {
        let out = Command::new("git")
            .arg("--no-optional-locks")
            .args(["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "core.untrackedCache=false"])
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        Some((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
    }
}

impl ProjectProbe for GitProbe {
    fn clean_at_base(&self, root: &Path, rel: &str) -> bool {
        // A file that changed since it was probed is probed again.
        let stamp = std::fs::metadata(root.join(rel)).ok().map(|m| format!("{}:{:?}", m.len(), m.modified().ok())).unwrap_or_default();
        let key = (root.display().to_string(), format!("{}\0{}", rel, stamp));
        if let Some(v) = self.cache.lock().unwrap().get(&key) {
            return *v;
        }
        let tracked = GitProbe::git(root, &["ls-files", "--error-unmatch", "--", rel]).map(|(ok, _)| ok).unwrap_or(false);
        let clean = tracked && GitProbe::git(root, &["status", "--porcelain", "--", rel]).map(|(ok, out)| ok && out.trim().is_empty()).unwrap_or(false);
        self.cache.lock().unwrap().insert(key, clean);
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(root: &Path, args: &[&str]) {
        assert!(Command::new("git").arg("-C").arg(root).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).stdout(Stdio::null()).status().unwrap().success());
    }

    #[test]
    fn tracked_unmodified_files_are_clean_and_everything_else_is_not() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        git(r, &["init", "-q"]);
        fs::write(r.join("a.rs"), "a").unwrap();
        fs::write(r.join("b.rs"), "b").unwrap();
        git(r, &["add", "."]);
        git(r, &["commit", "-qm", "base"]);
        fs::write(r.join("b.rs"), "b edited").unwrap();
        fs::write(r.join("new.rs"), "untracked").unwrap();
        let p = GitProbe::new();
        assert!(p.clean_at_base(r, "a.rs"));
        assert!(!p.clean_at_base(r, "b.rs"), "modified since the base commit");
        assert!(!p.clean_at_base(r, "new.rs"), "untracked");
        assert!(!p.clean_at_base(r, "missing.rs"));
    }

    #[test]
    fn a_hostile_repo_config_cannot_make_git_run_commands() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        git(r, &["init", "-q"]);
        fs::write(r.join("a.rs"), "a").unwrap();
        git(r, &["add", "."]);
        git(r, &["commit", "-qm", "base"]);
        let marker = r.join("pwned");
        let cfg = format!("\n[core]\n\tfsmonitor = touch {}\n", marker.display());
        let mut c = fs::read_to_string(r.join(".git/config")).unwrap();
        c.push_str(&cfg);
        fs::write(r.join(".git/config"), c).unwrap();
        fs::write(r.join("a.rs"), "changed").unwrap();
        let _ = GitProbe::new().clean_at_base(r, "a.rs");
        assert!(!marker.exists(), "core.fsmonitor from the repo config must not run");
    }

    #[test]
    fn a_directory_that_is_not_a_repo_is_never_clean() {
        let t = tempfile::tempdir().unwrap();
        assert!(!GitProbe::new().clean_at_base(t.path(), "x"));
    }
}
