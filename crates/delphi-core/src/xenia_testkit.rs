//! A test "remote" for Xenia: a real git repository, driven with the real `git`, that stands in for the org's
//! store host. Built for X4a and meant to be reused by X4b (history walk, rewrite recovery) and X7/X8
//! (publish, admin operations). Compiled for tests and under the `test-support` feature only.
//!
//! The model is one bare repository (the "host", whose `file://` URL an install fetches from) and one
//! working copy (the author's side). Authoring happens in the working copy; every [`commit`] force-pushes the
//! working copy's `HEAD` to the host's `main`, so the host always shows what the author last committed. That
//! one rule gives every scenario the spec needs:
//!
//! * fast-forward: [`commit`] on top of the previous head;
//! * deletion or revert of a file: [`remove`] or [`write`] the old bytes, then [`commit`];
//! * history rewrite or force-push: [`reset_hard`] to an earlier commit, [`commit`] something else (the host
//!   now diverges from what an install accepted), or [`amend`] the last commit;
//! * a stale host: [`reset_hard`] to an earlier commit and [`force_push`] (the host goes backwards);
//! * hostile content: [`symlink`], [`gitlink`], and plain [`write`] of `.gitmodules` or `.gitattributes`.
//!
//! The test side runs git with a clean environment and no user configuration, and commits with fixed
//! identities and a strictly increasing fake clock, so commit ids are stable across runs.
//!
//! [`commit`]: TestRemote::commit
//! [`remove`]: TestRemote::remove
//! [`write`]: TestRemote::write
//! [`reset_hard`]: TestRemote::reset_hard
//! [`amend`]: TestRemote::amend
//! [`force_push`]: TestRemote::force_push
//! [`symlink`]: TestRemote::symlink
//! [`gitlink`]: TestRemote::gitlink

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What a correct store's root `.gitattributes` says (spec 012 §7).
pub const GOOD_GITATTRIBUTES: &str = "* -text\n";

pub struct TestRemote {
    bare: PathBuf,
    work: PathBuf,
    clock: Cell<u64>,
    /// Gitlinks to re-stage after `add -A`, which would drop them (no such path on disk).
    gitlinks: RefCell<Vec<(String, String)>>,
}

impl TestRemote {
    /// Create `<dir>/remote.git` (bare, branch `main`) and `<dir>/work` (the author's working copy). `dir`
    /// should be a fresh temporary directory owned by the test.
    pub fn new(dir: &Path) -> TestRemote {
        let (bare, work) = (dir.join("remote.git"), dir.join("work"));
        fs::create_dir_all(&bare).unwrap();
        fs::create_dir_all(&work).unwrap();
        let t = TestRemote { bare, work, clock: Cell::new(1_700_000_000), gitlinks: RefCell::new(Vec::new()) };
        t.run(&t.bare, &["init", "--bare", "--quiet", "--initial-branch=main"]);
        t.run(&t.work, &["init", "--quiet", "--initial-branch=main"]);
        t.run(&t.work, &["config", "core.symlinks", "true"]);
        t
    }

    /// The `file://` URL an install fetches from.
    pub fn url(&self) -> String {
        format!("file://{}", self.bare.display())
    }

    /// The host's bare repository (for example to read what it serves).
    pub fn host_path(&self) -> &Path {
        &self.bare
    }

    pub fn work_path(&self) -> &Path {
        &self.work
    }

    /// Stage `bytes` at `rel` in the working copy (parent directories are created).
    pub fn write(&self, rel: &str, bytes: &[u8]) {
        let p = self.work.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&p);
        fs::write(p, bytes).unwrap();
    }

    pub fn remove(&self, rel: &str) {
        fs::remove_file(self.work.join(rel)).unwrap();
    }

    /// A symlink at `rel` pointing to `target`.
    pub fn symlink(&self, rel: &str, target: &str) {
        let p = self.work.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&p);
        std::os::unix::fs::symlink(target, p).unwrap();
    }

    /// A submodule entry (mode 160000) at `rel` naming `commit`; the commit need not exist anywhere.
    pub fn gitlink(&self, rel: &str, commit: &str) {
        self.gitlinks.borrow_mut().push((rel.to_string(), commit.to_string()));
        self.stage_all();
    }

    fn stage_all(&self) {
        self.run(&self.work, &["add", "-A", "--force"]);
        for (rel, commit) in self.gitlinks.borrow().iter() {
            self.run(&self.work, &["update-index", "--add", "--cacheinfo", &format!("160000,{commit},{rel}")]);
        }
    }

    /// Mark `rel` executable in the working copy.
    pub fn make_executable(&self, rel: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(self.work.join(rel), fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Stage everything (including removals), commit, and make the host's `main` equal to the new commit,
    /// forcing if needed. Returns the commit id.
    pub fn commit(&self, message: &str) -> String {
        self.stage_all();
        self.run(&self.work, &["commit", "--quiet", "--allow-empty", "--no-verify", "-m", message]);
        self.force_push();
        self.head()
    }

    /// [`write`](TestRemote::write) each file, then [`commit`](TestRemote::commit).
    pub fn commit_files(&self, files: &[(&str, &[u8])], message: &str) -> String {
        for (p, b) in files {
            self.write(p, b);
        }
        self.commit(message)
    }

    /// Rewrite the last commit (new message, so a new id) and force the host to it: a history rewrite.
    pub fn amend(&self, message: &str) -> String {
        self.stage_all();
        self.run(&self.work, &["commit", "--quiet", "--amend", "--allow-empty", "--no-verify", "-m", message]);
        self.force_push();
        self.head()
    }

    /// Move the working copy (files and `HEAD`) to `rev`. The host is unchanged until the next
    /// [`commit`](TestRemote::commit) or [`force_push`](TestRemote::force_push).
    pub fn reset_hard(&self, rev: &str) {
        self.run(&self.work, &["reset", "--quiet", "--hard", rev]);
    }

    /// Make the host's `main` equal to the working copy's `HEAD`, whatever that does to its history.
    pub fn force_push(&self) {
        let url = self.bare.to_str().unwrap();
        self.run(&self.work, &["push", "--quiet", "--force", url, "HEAD:refs/heads/main"]);
    }

    /// The working copy's `HEAD`.
    pub fn head(&self) -> String {
        self.run(&self.work, &["rev-parse", "HEAD"]).trim().to_string()
    }

    /// The commit the host currently serves on `main`.
    pub fn host_head(&self) -> String {
        self.run(&self.bare, &["rev-parse", "refs/heads/main"]).trim().to_string()
    }

    /// Any git command in the working copy; returns stdout. Panics on failure.
    pub fn git(&self, args: &[&str]) -> String {
        self.run(&self.work, args)
    }

    /// Plant `bytes` at `hooks/<name>` of the host repository and make it executable: a hook that would
    /// run if anything ever executed git hooks of the repository it fetches from.
    pub fn host_hook(&self, name: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        let p = self.bare.join("hooks").join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, script).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn run(&self, dir: &Path, args: &[&str]) -> String {
        let tick = self.clock.get() + 60;
        self.clock.set(tick);
        let date = format!("{tick} +0000");
        let out = Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Test Author")
            .env("GIT_AUTHOR_EMAIL", "author@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test Author")
            .env("GIT_COMMITTER_EMAIL", "author@example.invalid")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .args(["-c", "protocol.file.allow=always", "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false", "-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commits_reach_the_host_and_a_rewrite_changes_the_served_head() {
        let d = tempfile::tempdir().unwrap();
        let r = TestRemote::new(d.path());
        let a = r.commit_files(&[("a.txt", b"one")], "a");
        assert_eq!(r.host_head(), a);
        let b = r.commit_files(&[("a.txt", b"two")], "b");
        assert_eq!(r.host_head(), b);
        let b2 = r.amend("b again");
        assert_ne!(b, b2);
        assert_eq!(r.host_head(), b2);
        r.reset_hard(&a);
        r.force_push();
        assert_eq!(r.host_head(), a);
    }

    #[test]
    fn hostile_shapes_can_be_built() {
        let d = tempfile::tempdir().unwrap();
        let r = TestRemote::new(d.path());
        r.write("x", b"x");
        r.symlink("link", "/etc/passwd");
        r.gitlink("sub", &"1".repeat(40));
        r.commit("hostile");
        let tree = r.git(&["ls-tree", "-r", "HEAD"]);
        assert!(tree.contains("120000") && tree.contains("160000"), "{tree}");
    }
}
