//! Spec 092 S2b: the machinery an operator-authorized test would run in, and the proof that it holds.
//!
//! Nothing in this module is called from a run. It builds (1) the **authorized profile**, a [`SandboxSpec`] with a read-only
//! snapshot, a no-exec scratch directory, no network, one executable interpreter by literal path and a fixed environment;
//! (2) [`probe_authorized`], a canary battery run under that very profile, whose answer is a struct (never a bare yes) and
//! which fails closed; (3) the [`Snapshot`] builder (a disposable read-only copy of the tracked files); and (4) the
//! [`FreshHome`] builder and its walk (no symlinks, no other-device mount, no credential-named file).
//!
//! macOS is the primary platform: `sandbox-exec` can grant `process-exec` as a literal and omit it on scratch. Bubblewrap has
//! neither, so on Linux the profile is **refused** (`SandboxError::Unsupported`) and the probe says so; it never falls back.

use crate::{Network, Sandbox, SandboxError, SandboxSpec};
use crate::run::{run_bounded, RunOptions};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::io::{self, Write as _};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// The first admitted interpreter (spec 092, confirmed by the operator 2026-11-08): the sealed-volume system bash.
pub const INTERPRETER: &str = "/bin/bash";

/// The fixed environment of an authorized command: no `PATH` search (the directory does not exist), no `LD_PRELOAD`, no
/// `DYLD_*`, no `BASH_ENV` or `ENV`, no `HOME`, no token. The engine's `scrubbed_env()` is this map.
pub fn authorized_env() -> BTreeMap<String, String> {
    [("LC_ALL", "C"), ("PATH", "/nonexistent")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// The files that may be executed when `interpreter` is the runner: itself, and bash too when it is `/bin/sh` (which re-executes bash).
pub fn exec_literals(interpreter: &str) -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from(interpreter)];
    if interpreter == "/bin/sh" {
        v.push(PathBuf::from(INTERPRETER));
    }
    v
}

/// The authorized profile. `snapshot` is read-only and not executable; `scratch` is writable and not executable; the network
/// is off; only the interpreter (by literal path) can be executed; the environment is exactly [`authorized_env`].
pub fn authorized_spec(snapshot: &Path, scratch: &Path, interpreter: &str) -> SandboxSpec {
    let mut s = SandboxSpec::new();
    s.snapshot_ro = Some(snapshot.to_path_buf());
    s.scratch = vec![scratch.to_path_buf()];
    s.scratch_noexec = true;
    s.exec_only = Some(exec_literals(interpreter));
    s.network = Network::Off;
    s.env = authorized_env();
    s.limits.cpu_secs = Some(60);
    s.limits.max_file_bytes = Some(8 << 20);
    s.limits.max_open_files = Some(256);
    s
}

// ------------------------------------------------------------------------------------------------ small helpers

/// A fresh directory (`mkdtemp`, mode 0700) under `parent`.
pub fn mkdtemp(parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    let template = parent.join(format!("{prefix}XXXXXX"));
    let c = CString::new(template.as_os_str().as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))?;
    let raw = c.into_raw();
    // SAFETY: `raw` is a NUL-terminated writable buffer owned here; mkdtemp rewrites the XXXXXX in place.
    let made = unsafe { libc::mkdtemp(raw) };
    // SAFETY: taking back the pointer from `into_raw` above.
    let owned = unsafe { CString::from_raw(raw) };
    if made.is_null() {
        return Err(io::Error::last_os_error());
    }
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(owned.to_bytes()));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

fn make_writable_tree(root: &Path) {
    // Directories first (so they can be read), links never followed.
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&p) else { continue };
        if meta.is_symlink() {
            continue;
        }
        let mode = if meta.is_dir() { 0o700 } else { 0o600 };
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(mode));
        if meta.is_dir() {
            if let Ok(rd) = fs::read_dir(&p) {
                stack.extend(rd.flatten().map(|e| e.path()));
            }
        }
    }
}

fn remove_tree(root: &Path) {
    make_writable_tree(root);
    let _ = fs::remove_dir_all(root);
}

// ------------------------------------------------------------------------------------------------ the snapshot

/// A disposable, read-only copy of a project's **tracked** files (what `git ls-files` lists: no `.git`, nothing untracked).
/// Files are 0444, directories 0555. It is discarded when dropped; there is no path from it back to the project.
#[derive(Debug)]
pub struct Snapshot {
    root: PathBuf,
    dir: PathBuf,
    tree_hash: String,
    files: usize,
}

impl Snapshot {
    /// Copy the tracked files of the git project at `project` into a fresh directory under `parent`.
    pub fn build(project: &Path, parent: &Path) -> Result<Snapshot, String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(project)
            .args(["ls-files", "-z", "--cached"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
            .output()
            .map_err(|e| format!("git ls-files: {e}"))?;
        if !out.status.success() {
            return Err(format!("git ls-files failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let names: Vec<PathBuf> = out.stdout.split(|b| *b == 0).filter(|n| !n.is_empty()).map(|n| PathBuf::from(std::ffi::OsStr::from_bytes(n))).collect();
        let project = fs::canonicalize(project).map_err(|e| format!("project: {e}"))?;
        Snapshot::from_files(&project, &names, parent)
    }

    /// The copy itself, from an explicit list of project-relative paths. Refuses a path that leaves the tree, a `.git` segment,
    /// and any symlink whose target is absolute or leaves the tree. A submodule (a directory) is left out.
    pub fn from_files(project: &Path, names: &[PathBuf], parent: &Path) -> Result<Snapshot, String> {
        let root = mkdtemp(parent, "delphi-snapshot-").map_err(|e| format!("snapshot directory: {e}"))?;
        let dir = root.join("snapshot");
        let result = (|| -> Result<usize, String> {
            fs::create_dir(&dir).map_err(|e| e.to_string())?;
            let mut count = 0;
            for rel in names {
                let comps: Vec<_> = rel.components().collect();
                if comps.is_empty() || comps.iter().any(|c| !matches!(c, Component::Normal(_))) {
                    return Err(format!("`{}` is not a plain relative path", rel.display()));
                }
                if comps.iter().any(|c| c.as_os_str() == ".git") {
                    return Err(format!("`{}` has a `.git` segment; a snapshot never carries git internals", rel.display()));
                }
                let src = project.join(rel);
                let meta = fs::symlink_metadata(&src).map_err(|e| format!("`{}`: {e}", rel.display()))?;
                let dst = dir.join(rel);
                if let Some(p) = dst.parent() {
                    fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                if meta.is_symlink() {
                    let target = fs::read_link(&src).map_err(|e| e.to_string())?;
                    check_link_inside(rel, &target)?;
                    std::os::unix::fs::symlink(&target, &dst).map_err(|e| e.to_string())?;
                } else if meta.is_file() {
                    let bytes = fs::read(&src).map_err(|e| format!("`{}`: {e}", rel.display()))?;
                    let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&dst).map_err(|e| e.to_string())?;
                    f.write_all(&bytes).map_err(|e| e.to_string())?;
                    drop(f);
                    fs::set_permissions(&dst, fs::Permissions::from_mode(0o444)).map_err(|e| e.to_string())?;
                    count += 1;
                } // a directory here is a submodule: left out
            }
            seal(&dir);
            Ok(count)
        })();
        match result {
            Ok(files) => {
                let tree_hash = tree_hash_of(&dir)?;
                Ok(Snapshot { root, dir, tree_hash, files })
            }
            Err(e) => {
                remove_tree(&root);
                Err(e)
            }
        }
    }

    /// The read-only directory a sandboxed command sees.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// The hash of the tree (paths, kinds, content) as it was built.
    pub fn tree_hash(&self) -> &str {
        &self.tree_hash
    }

    pub fn file_count(&self) -> usize {
        self.files
    }

    /// The directory that holds the snapshot and is removed with it (a sibling place for scratch and canaries).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Re-hash the tree on disk: `Ok` only if it is exactly what was built (nothing written, added, removed or re-linked).
    pub fn verify(&self) -> Result<(), String> {
        let now = tree_hash_of(&self.dir)?;
        if now == self.tree_hash {
            Ok(())
        } else {
            Err("the snapshot changed after it was built".into())
        }
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        remove_tree(&self.root);
    }
}

/// A symlink is kept only if its relative target stays inside the tree (checked lexically, from the link's own directory).
fn check_link_inside(rel: &Path, target: &Path) -> Result<(), String> {
    if target.is_absolute() {
        return Err(format!("`{}` is a symlink with an absolute target; a snapshot refuses it", rel.display()));
    }
    let mut depth: i64 = rel.components().count() as i64 - 1; // directories between the root and the link
    for c in target.components() {
        match c {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(format!("`{}` is a symlink that leaves the tree; a snapshot refuses it", rel.display()));
                }
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            _ => return Err(format!("`{}` is a symlink with an unusual target", rel.display())),
        }
    }
    Ok(())
}

/// Files 0444 are already set; directories become 0555, deepest first.
fn seal(root: &Path) {
    let mut dirs = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        if let Ok(rd) = fs::read_dir(&p) {
            for e in rd.flatten() {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push(e.path());
                }
            }
        }
        dirs.push(p);
    }
    for d in dirs.iter().rev() {
        let _ = fs::set_permissions(d, fs::Permissions::from_mode(0o555));
    }
}

/// A deterministic hash of a tree: directories, files (mode and content) and symlinks (target), sorted by path, links never followed.
pub fn tree_hash_of(root: &Path) -> Result<String, String> {
    let mut lines: Vec<(PathBuf, String)> = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let rd = fs::read_dir(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        for e in rd.flatten() {
            let path = e.path();
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            let mode = meta.permissions().mode() & 0o7777;
            if meta.is_symlink() {
                let t = fs::read_link(&path).map_err(|e| e.to_string())?;
                lines.push((rel, format!("l {}", t.display())));
            } else if meta.is_dir() {
                lines.push((rel, format!("d {mode:o}")));
                stack.push(path);
            } else if meta.is_file() {
                let h = blake3::hash(&fs::read(&path).map_err(|e| e.to_string())?);
                lines.push((rel, format!("f {mode:o} {}", h.to_hex())));
            } else {
                lines.push((rel, "other".into()));
            }
        }
    }
    lines.sort();
    let mut h = blake3::Hasher::new();
    for (p, l) in &lines {
        h.update(p.as_os_str().as_bytes());
        h.update(b"\0");
        h.update(l.as_bytes());
        h.update(b"\n");
    }
    Ok(h.finalize().to_hex().to_string())
}

// ------------------------------------------------------------------------------------------------ the fresh home

/// A throwaway home the daemon made: `mkdtemp`, mode 0700, owned by this user. [`FreshHome::recheck`] walks it with `lstat` and
/// refuses any symlink, any entry on another device (a mount) and any credential-named file; call it at each use.
#[derive(Debug)]
pub struct FreshHome {
    path: PathBuf,
    dev: u64,
}

impl FreshHome {
    pub fn create(root: &Path) -> Result<FreshHome, String> {
        let rmeta = fs::symlink_metadata(root).map_err(|e| format!("home root: {e}"))?;
        if rmeta.is_symlink() || !rmeta.is_dir() {
            return Err("the home root is a symlink or not a directory".into());
        }
        let path = mkdtemp(root, "delphi-home-").map_err(|e| format!("home: {e}"))?;
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let h = FreshHome { path, dev: meta.dev() };
        h.recheck()?;
        Ok(h)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Walk the home now. `Err` names the first problem.
    pub fn recheck(&self) -> Result<(), String> {
        let meta = fs::symlink_metadata(&self.path).map_err(|e| format!("the home is gone: {e}"))?;
        // SAFETY: geteuid has no preconditions.
        let me = unsafe { libc::geteuid() };
        if meta.is_symlink() || !meta.is_dir() || meta.uid() != me || meta.permissions().mode() & 0o077 != 0 {
            return Err("the home is not a private directory of this user".into());
        }
        if meta.dev() != self.dev {
            return Err("the home moved to another device".into());
        }
        let mut stack = vec![self.path.clone()];
        while let Some(dir) = stack.pop() {
            let rd = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for e in rd {
                let e = e.map_err(|e| e.to_string())?;
                let path = e.path();
                let m = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(why) = judge_home_entry(&name, m.is_symlink(), m.is_dir() || m.is_file(), m.dev(), self.dev) {
                    return Err(format!("{}: {why}", path.display()));
                }
                if m.is_dir() {
                    stack.push(path);
                }
            }
        }
        Ok(())
    }
}

impl Drop for FreshHome {
    fn drop(&mut self) {
        remove_tree(&self.path);
    }
}

/// What is wrong with one directory entry of a throwaway home, or `None`. Pure, so every rule is tested without a mount.
pub fn judge_home_entry(name: &str, is_symlink: bool, regular_or_dir: bool, dev: u64, home_dev: u64) -> Option<String> {
    if is_symlink {
        return Some("a symlink in the throwaway home".into());
    }
    if dev != home_dev {
        return Some("another device (a mount) inside the throwaway home".into());
    }
    if !regular_or_dir {
        return Some("a socket, device or pipe in the throwaway home".into());
    }
    let n = name.to_ascii_lowercase();
    if n == "auth.json" || n == ".netrc" || n == "_netrc" || n.starts_with("credentials") || n.ends_with(".pem") || n.starts_with("id_") {
        return Some("a credential-named file in the throwaway home".into());
    }
    None
}

// ------------------------------------------------------------------------------------------------ the probe

/// The answer of [`probe_authorized`]: one field per canary, all false until proven. A struct, never a bare yes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorizedProbe {
    /// Which sandbox ran the canaries, or `None` when there is none.
    pub mode: Option<&'static str>,
    /// Why the authorized profile cannot be had on this machine at all (no sandbox, or the platform cannot express it).
    pub unavailable: Option<String>,
    /// (a) a network connect from inside the profile never reached a listener the probe was holding.
    pub network_blocked: bool,
    /// (b) a canary file outside the grant could not be read.
    pub outside_unreadable: bool,
    /// (c) the snapshot could be read and could not be written (the file and the directory are unchanged).
    pub snapshot_readonly: bool,
    /// (d) scratch could be written and an executable file in it could not be run.
    pub scratch_noexec: bool,
    /// (e) the interpreter started and ran a trivial pinned script.
    pub interpreter_runs: bool,
    /// What the probe saw, for a human (never trusted for a decision).
    pub detail: String,
}

impl AuthorizedProbe {
    pub fn is_ok(&self) -> bool {
        self.unavailable.is_none() && self.network_blocked && self.outside_unreadable && self.snapshot_readonly && self.scratch_noexec && self.interpreter_runs
    }

    /// Names of the canaries that failed (or `unavailable`).
    pub fn failed(&self) -> Vec<&'static str> {
        if self.unavailable.is_some() {
            return vec!["unavailable"];
        }
        let mut v = vec![];
        for (ok, name) in [(self.network_blocked, "network_blocked"), (self.outside_unreadable, "outside_unreadable"), (self.snapshot_readonly, "snapshot_readonly"), (self.scratch_noexec, "scratch_noexec"), (self.interpreter_runs, "interpreter_runs")] {
            if !ok {
                v.push(name);
            }
        }
        v
    }

    /// One line for `delphi doctor`.
    pub fn summary(&self) -> String {
        if let Some(u) = &self.unavailable {
            return format!("unavailable on this machine: {u}");
        }
        if self.is_ok() {
            return format!("{} holds all five canaries (no network, no read outside the grant, read-only snapshot, no-exec scratch, interpreter runs)", self.mode.unwrap_or("sandbox"));
        }
        format!("{} fails: {}; an authorized test would refuse to start", self.mode.unwrap_or("sandbox"), self.failed().join(", "))
    }
}

/// What a test hook may need to weaken the probe's profile.
#[allow(dead_code)]
pub(crate) struct ProbeCtx {
    pub port: u16,
    pub snapshot: PathBuf,
}

/// The probe script, written into the probe's own snapshot. Arguments: nonce, snapshot, scratch, canary, port.
const PROBE_SCRIPT: &str = r#"#!/bin/bash
nonce="$1"; snap="$2"; scratch="$3"; canary="$4"; port="$5"
echo "interp-ok $nonce"
if ( IFS= read -r x < "$snap/data.txt" ) 2>/dev/null; then echo "snapshot-read=ok"; else echo "snapshot-read=denied"; fi
( echo overwritten > "$snap/data.txt" ) 2>/dev/null
( echo created > "$snap/new-file" ) 2>/dev/null
if ( echo w > "$scratch/w.txt" ) 2>/dev/null; then echo "scratch-write=ok"; else echo "scratch-write=denied"; fi
if ( "$scratch/pre-exec" ) >/dev/null 2>&1; then echo "scratch-exec=RAN"; else echo "scratch-exec=blocked"; fi
if ( IFS= read -r x < "$canary" && echo "read-outside=$x" ) 2>/dev/null; then :; else echo "read-outside=denied"; fi
if ( exec 3<> "/dev/tcp/127.0.0.1/$port" ) 2>/dev/null; then echo "net=OPEN"; else echo "net=denied"; fi
"#;

const CANARY_TEXT: &str = "CANARY-SECRET-delphi-092";
const SNAPSHOT_DATA: &str = "snapshot data\n";

/// Run the canary battery under the REAL authorized profile and say which canaries held. Never falls back to an unsandboxed run:
/// with no sandbox, or on a platform that cannot express the profile, every canary is false and `unavailable` says why.
pub fn probe_authorized() -> AuthorizedProbe {
    #[cfg(test)]
    if std::env::var_os("DELPHI_TEST_FORCE_NO_SANDBOX").is_some() {
        return AuthorizedProbe { unavailable: Some("forced off by DELPHI_TEST_FORCE_NO_SANDBOX (test hook)".into()), ..AuthorizedProbe::default() };
    }
    probe_with(INTERPRETER, |_, _| {})
}

/// The battery with a hook to change the profile before it runs. The hook exists so tests can weaken the profile and see the
/// canaries notice; production calls [`probe_authorized`].
pub(crate) fn probe_with(interpreter: &str, weaken: impl FnOnce(&mut SandboxSpec, &ProbeCtx)) -> AuthorizedProbe {
    let mut out = AuthorizedProbe::default();
    let sb = match Sandbox::detect() {
        Ok(sb) => sb,
        Err(e) => {
            out.unavailable = Some(e.to_string());
            return out;
        }
    };
    out.mode = Some(sb.mode().name());
    let tmp = match fs::canonicalize(std::env::temp_dir()).map_err(|e| e.to_string()).and_then(|t| mkdtemp(&t, "delphi-probe-").map_err(|e| e.to_string())) {
        Ok(p) => p,
        Err(e) => {
            out.unavailable = Some(format!("no temporary directory: {e}"));
            return out;
        }
    };
    let result = probe_in(&sb, &tmp, interpreter, weaken, &mut out);
    remove_tree(&tmp);
    if let Err(e) = result {
        // A profile the platform cannot express, or any setup error: nothing was proven.
        out.unavailable = Some(e);
        out.network_blocked = false;
        out.outside_unreadable = false;
        out.snapshot_readonly = false;
        out.scratch_noexec = false;
        out.interpreter_runs = false;
    }
    out
}

fn probe_in(sb: &Sandbox, tmp: &Path, interpreter: &str, weaken: impl FnOnce(&mut SandboxSpec, &ProbeCtx), out: &mut AuthorizedProbe) -> Result<(), String> {
    let (snap, scratch, canary) = (tmp.join("snapshot"), tmp.join("scratch"), tmp.join("canary.txt"));
    let io = |e: io::Error| e.to_string();
    fs::create_dir(&snap).map_err(io)?;
    fs::create_dir(&scratch).map_err(io)?;
    fs::write(snap.join("probe.sh"), PROBE_SCRIPT).map_err(io)?;
    fs::write(snap.join("data.txt"), SNAPSHOT_DATA).map_err(io)?;
    for f in ["probe.sh", "data.txt"] {
        fs::set_permissions(snap.join(f), fs::Permissions::from_mode(0o444)).map_err(io)?;
    }
    fs::set_permissions(&snap, fs::Permissions::from_mode(0o555)).map_err(io)?;
    fs::write(&canary, format!("{CANARY_TEXT}\n")).map_err(io)?;
    let marker = scratch.join("exec-marker");
    let pre = scratch.join("pre-exec");
    fs::write(&pre, format!("#!/bin/bash\necho ran > '{}'\nexit 0\n", marker.display())).map_err(io)?;
    fs::set_permissions(&pre, fs::Permissions::from_mode(0o755)).map_err(io)?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(io)?;
    listener.set_nonblocking(true).map_err(io)?;
    let port = listener.local_addr().map_err(io)?.port();
    let nonce = format!("{:x}", std::process::id() as u64 ^ (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)));

    let mut spec = authorized_spec(&snap, &scratch, interpreter);
    weaken(&mut spec, &ProbeCtx { port, snapshot: snap.clone() });
    let mut cmd = sb.command(&spec, interpreter, [snap.join("probe.sh").as_os_str().to_owned(), nonce.clone().into(), snap.as_os_str().to_owned(), scratch.as_os_str().to_owned(), canary.as_os_str().to_owned(), port.to_string().into()]).map_err(|e| match e {
        SandboxError::Unsupported(m) => format!("this platform cannot express the authorized profile: {m}"),
        other => other.to_string(),
    })?;
    cmd.current_dir(&snap);
    let run = run_bounded(cmd, &RunOptions { timeout: Duration::from_secs(30), max_output_bytes: 64 << 10, quota: vec![] }).map_err(io)?;
    let text = run.stdout.clone();
    let has = |s: &str| text.lines().any(|l| l == s);
    out.interpreter_runs = run.success() && has(&format!("interp-ok {nonce}"));
    // Each verdict is taken from what happened on disk or at the listener, not from what the script says, where it can be.
    let connected = listener.accept().is_ok();
    out.network_blocked = out.interpreter_runs && !connected && !has("net=OPEN");
    out.outside_unreadable = out.interpreter_runs && !text.contains(CANARY_TEXT);
    let snap_unchanged = fs::read_to_string(snap.join("data.txt")).map(|d| d == SNAPSHOT_DATA).unwrap_or(false) && !snap.join("new-file").exists();
    out.snapshot_readonly = out.interpreter_runs && has("snapshot-read=ok") && snap_unchanged;
    out.scratch_noexec = out.interpreter_runs && has("scratch-write=ok") && !marker.exists() && !has("scratch-exec=RAN");
    out.detail = format!("exit {:?}; stdout {:?}; stderr {:?}", run.status.and_then(|s| s.code()), text.trim(), run.stderr.trim());
    Ok(())
}

#[cfg(test)]
mod tests;
