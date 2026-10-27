//! The daemon's side of a Xenia store (spec 011 §3.3.5, §3.5; plan milestone X4a): the clone under
//! `$DELPHI_HOME/xenia/<org>/<store>/`, the fast-forward-only fetch, and the sticky state file. This is the
//! place where a hostile git repository meets the install, so it is built to refuse rather than to cope.
//!
//! * **One way to run git.** [`git_command`] builds every git invocation: a cleared environment, no user
//!   or system configuration, hooks pointed at an empty directory, fsmonitor, symlinks, replace refs,
//!   credential helpers, pagers and prompts off, every protocol denied except the one the store URL needs,
//!   LFS filters neutralised. Nothing else in this module spawns git.
//! * **Nothing is checked out.** The clone is bare and files are read with `git cat-file` by object id, so
//!   hooks like `post-checkout`, `.gitattributes` filters, line-ending conversion, symlink creation and
//!   submodule handling have nothing to act on. `--recurse-submodules` is never passed.
//! * **Caps before trust.** The fetch is cut off if the clone grows past the byte cap; every new object is
//!   measured (count, per-object size, total) before the tip tree is even listed; the tree and
//!   `.gitattributes` are then checked by the pure rules in `delphi_xenia::store`.
//! * **Fast-forward only.** The fetched head goes to a staging ref; the accepted head is advanced only after
//!   every check passed, and a head that is not a descendant of the accepted one is reported
//!   ([`NonFastForward`]) and never applied. Recovery from a rewrite is X4b's job.
//! * **Sticky state outside the clone.** `state/sticky.json` is written atomically, `0600` in a `0700`
//!   directory, and only after a whole sync was clean. A sync with any sticky violation applies nothing.

use delphi_xenia::sticky::{Evaluation, Observation, Refusal, StickyState, Tracking};
use delphi_xenia::store::{self, AttrsRefusal, Caps, Hostile, Scheme, StorePaths, StoreUrl, TreeEntry};
use delphi_xenia::{hash, Error, Result};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where a fetch lands before anything is checked. Never trusted, never read by anything but this module.
pub(crate) const REF_FETCHED: &str = "refs/xenia/fetched";
/// Keeps the accepted history reachable so it is not garbage collected. The authority for "accepted" is the
/// sticky file, not this ref.
pub(crate) const REF_ACCEPTED: &str = "refs/xenia/accepted";

pub(crate) fn io(e: impl std::fmt::Display) -> Error {
    Error::Io(e.to_string())
}

/// Text from git's stderr is partly attacker-controlled (a remote's `remote:` lines): one short line,
/// control characters replaced.
pub(crate) fn short(stderr: &str) -> String {
    let line = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("git failed");
    line.chars().take(160).map(|c| if c.is_control() { '?' } else { c }).collect()
}

/// The single place a git command is built (spec 011 §3.3.5, plan risk R2). `scheme` is the transport the
/// call may use (`None` for local-only calls, which then deny every protocol); `git_dir` is given explicitly
/// so git never discovers a repository from the working directory.
pub fn git_command(paths: &StorePaths, scheme: Option<Scheme>, git_dir: Option<&Path>) -> Command {
    let mut c = Command::new("git");
    c.env_clear();
    c.env("PATH", std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()));
    c.env("HOME", &paths.empty_home).env("XDG_CONFIG_HOME", &paths.empty_home);
    c.env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null");
    c.env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0").env("GIT_NO_REPLACE_OBJECTS", "1").env("GIT_NO_LAZY_FETCH", "1");
    c.env("GIT_ATTR_NOSYSTEM", "1").env("GIT_PAGER", "cat").env("GIT_EDITOR", "true").env("GIT_ASKPASS", "true").env("GIT_LFS_SKIP_SMUDGE", "1");
    c.env("LC_ALL", "C").env("GIT_PROTOCOL_FROM_USER", "0");
    c.env("GIT_ALLOW_PROTOCOL", scheme.map(|s| s.git_name()).unwrap_or("none"));
    if scheme == Some(Scheme::Ssh) {
        // The one transport that needs the operator's ambient identity: their ssh agent and known hosts.
        // git's own configuration stays disabled; ssh may not run a proxy or local command from its config.
        if let Some(h) = std::env::var_os("HOME") {
            c.env("HOME", h);
        }
        if let Some(s) = std::env::var_os("SSH_AUTH_SOCK") {
            c.env("SSH_AUTH_SOCK", s);
        }
        c.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ProxyCommand=none -o PermitLocalCommand=no");
    }
    let hooks = format!("core.hooksPath={}", paths.empty_hooks.display());
    let mut cfg: Vec<String> = vec![
        hooks,
        "core.fsmonitor=false".into(),
        "core.symlinks=false".into(),
        "core.autocrlf=false".into(),
        "core.safecrlf=false".into(),
        "core.attributesFile=/dev/null".into(),
        "core.pager=cat".into(),
        "core.editor=true".into(),
        "core.askpass=true".into(),
        "core.sshCommand=false".into(),
        "core.protectNTFS=true".into(),
        "core.protectHFS=true".into(),
        "core.untrackedCache=false".into(),
        "credential.helper=".into(),
        "transfer.fsckObjects=true".into(),
        "fetch.fsckObjects=true".into(),
        "fetch.recurseSubmodules=false".into(),
        "submodule.recurse=false".into(),
        "uploadpack.allowFilter=false".into(),
        "http.followRedirects=false".into(),
        "protocol.version=2".into(),
        "protocol.allow=never".into(),
        "gc.auto=0".into(),
        "maintenance.auto=false".into(),
        "receive.autogc=false".into(),
        "diff.external=".into(),
        "commit.gpgSign=false".into(),
        "tag.gpgSign=false".into(),
        "advice.detachedHead=false".into(),
        // LFS is never used: a store is small signed text. Even if an LFS filter were configured somewhere
        // git still reads, these keep it from being invoked.
        "filter.lfs.clean=".into(),
        "filter.lfs.smudge=".into(),
        "filter.lfs.process=".into(),
        "filter.lfs.required=false".into(),
    ];
    if let Some(s) = scheme {
        cfg.push(format!("protocol.{}.allow=always", s.git_name()));
    }
    for item in cfg {
        c.arg("-c").arg(item);
    }
    if let Some(g) = git_dir {
        c.arg("--git-dir").arg(g);
    }
    // Without an explicit git dir (`init`) git would otherwise discover whatever repository the daemon's
    // working directory sits in. An empty directory with a ceiling above it finds none.
    c.current_dir(&paths.empty_home).env("GIT_CEILING_DIRECTORIES", &paths.root);
    c.stdin(Stdio::null());
    c
}

// ---------------------------------------------------------------------------------------------------------
// Running a command with limits.

pub(crate) struct Out {
    pub(crate) code: i32,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: String,
}

pub(crate) enum RunFail {
    Timeout,
    TooMuchOutput,
    DiskCap,
    Spawn(String),
}

fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    let Ok(rd) = fs::read_dir(p) else { return 0 };
    for e in rd.flatten() {
        let Ok(m) = fs::symlink_metadata(e.path()) else { continue };
        if m.is_dir() {
            total += dir_size(&e.path());
        } else {
            total += m.len();
        }
    }
    total
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Run `cmd`, capturing at most `out_cap` bytes of stdout, with a wall-clock limit and an optional ceiling on
/// how much a directory may grow while it runs. The child is killed on any breach.
pub(crate) fn run(mut cmd: Command, stdin: Option<Vec<u8>>, out_cap: usize, timeout: Duration, watch: Option<(&Path, u64)>) -> std::result::Result<Out, RunFail> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let base = watch.map(|(p, _)| dir_size(p)).unwrap_or(0);
    let mut child = cmd.spawn().map_err(|e| RunFail::Spawn(e.to_string()))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let writer = stdin.map(|bytes| {
        let mut si = child.stdin.take().expect("piped stdin");
        std::thread::spawn(move || {
            let _ = si.write_all(&bytes);
        })
    });
    let mut so = child.stdout.take().expect("piped stdout");
    let flag = overflow.clone();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            match so.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() + n > out_cap {
                        flag.store(true, Ordering::SeqCst);
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        }
        buf
    });
    let mut se = child.stderr.take().expect("piped stderr");
    let errs = std::thread::spawn(move || {
        let mut keep = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = se.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if keep.len() < 8192 {
                keep.extend_from_slice(&chunk[..n.min(8192 - keep.len())]);
            }
        }
        String::from_utf8_lossy(&keep).into_owned()
    });
    let start = Instant::now();
    let mut last_watch = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                kill(&mut child);
                return Err(RunFail::Spawn(e.to_string()));
            }
        }
        if overflow.load(Ordering::SeqCst) {
            kill(&mut child);
            return Err(RunFail::TooMuchOutput);
        }
        if start.elapsed() > timeout {
            kill(&mut child);
            return Err(RunFail::Timeout);
        }
        if let Some((p, cap)) = watch {
            if last_watch.elapsed() > Duration::from_millis(50) {
                last_watch = Instant::now();
                if dir_size(p).saturating_sub(base) > cap {
                    kill(&mut child);
                    return Err(RunFail::DiskCap);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if let Some(w) = writer {
        let _ = w.join();
    }
    let stdout = reader.join().unwrap_or_default();
    if overflow.load(Ordering::SeqCst) {
        return Err(RunFail::TooMuchOutput);
    }
    let stderr = errs.join().unwrap_or_default();
    Ok(Out { code: status.code().unwrap_or(-1), stdout, stderr })
}

// ---------------------------------------------------------------------------------------------------------
// Reports.

/// A reason the fetched head was refused outright. The previous accepted state stays in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The tip tree has entries a store never legitimately has (symlinks, submodules, `.git*` names, ...).
    Tree(Vec<Hostile>),
    /// The tip tree has more entries than the cap, or `ls-tree` output was unreasonably large.
    TreeTooLarge,
    /// The root `.gitattributes` is missing or not the byte-exact one (spec 012 §7, test 16).
    Gitattributes(AttrsRefusal),
    /// A new git object over the per-object cap.
    ObjectTooLarge { sha: String, size: u64 },
    TooManyObjects,
    TooManyCommits,
    /// The fetch grew the clone past the byte cap, or the new objects sum past it.
    TooLarge,
    /// Objects are missing or unreadable, or git printed something unparseable.
    Malformed(&'static str),
}

impl Refused {
    /// The slug for `delphi xenia status`.
    pub fn reason(&self) -> &'static str {
        match self {
            Refused::Tree(_) => "hostile-tree",
            Refused::TreeTooLarge => "tree-too-large",
            Refused::Gitattributes(a) => a.reason(),
            Refused::ObjectTooLarge { .. } => "object-too-large",
            Refused::TooManyObjects => "too-many-objects",
            Refused::TooManyCommits => "too-many-commits",
            Refused::TooLarge => "fetch-too-large",
            Refused::Malformed(w) => w,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Divergence {
    /// The fetched head is not a descendant and not an ancestor: history was rewritten or force-pushed.
    Diverged,
    /// The fetched head is an ancestor of the accepted one: the host serves an older view (withheld updates
    /// or a reverted branch).
    Behind,
}

/// The fetched head is not a fast-forward of the accepted one. Never applied; the staging ref
/// `refs/xenia/fetched` is left in the clone so X4b can show the operator what was deleted or reverted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonFastForward {
    pub accepted: String,
    pub fetched: String,
    pub kind: Divergence,
}

/// What fetching a side branch (see [`Store::fetch_aux`]) found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuxFetch {
    /// The remote has no such branch.
    Missing,
    Tip(String),
    Refused(Refused),
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    UpToDate { head: String },
    /// A fast-forward (or the first fetch) that passed every cap and tree check. `entries` is the tip tree.
    Advanced { head: String, from: Option<String>, entries: Vec<TreeEntry> },
    NonFastForward(NonFastForward),
    Refused(Refused),
    /// The host could not be reached or has no such branch; offline sync keeps working from what is accepted.
    Unavailable(String),
}

/// Files under `records/` and `roster/` at one commit, by path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub head: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncReport {
    /// The host serves the head already accepted. Nothing was read or written.
    UpToDate { head: String },
    Accepted { head: String, from: Option<String>, snapshot: Snapshot, quarantined: Vec<(String, &'static str)>, added: usize, superseded: usize, unchanged: usize },
    /// The sticky rules found tampering. Nothing was applied: the accepted head and the stored state are
    /// exactly as before, so every stored version keeps its effect (spec 011 §3.5, §4.3).
    Tampering { head: String, refusals: Vec<Refusal> },
    NonFastForward(NonFastForward),
    Refused(Refused),
    Unavailable(String),
}

/// Decides one file's sticky ordering, or why it is quarantined. It is where X2's verification plugs in: a
/// file that does not verify must return `Err`, so it can never raise a stored `rev`.
pub type Classifier<'a> = &'a mut dyn FnMut(&str, &[u8]) -> std::result::Result<Tracking, &'static str>;

/// The layout-only classifier: sticky ordering from the path, a record's `rev` from `rev_of`. `rev_of` must
/// itself verify the signature before reading a `rev` (spec 011 §3.4); X4a tests pass a trivial reader.
pub fn classify_by_layout(path: &str, bytes: &[u8], rev_of: &dyn Fn(&[u8]) -> std::result::Result<u64, &'static str>) -> std::result::Result<Tracking, &'static str> {
    let class = store::classify_path(path).ok_or("unknown-path")?;
    store::tracking_for(&class, || rev_of(bytes))
}

// ---------------------------------------------------------------------------------------------------------
// The store.

pub struct Store {
    pub(crate) org: String,
    pub(crate) store: String,
    pub paths: StorePaths,
    pub(crate) url: StoreUrl,
    pub(crate) branch: String,
    pub(crate) caps: Caps,
}

fn valid_branch(b: &str) -> bool {
    !b.is_empty()
        && b.len() <= 64
        && b.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b'/'))
        && !b.starts_with(['-', '/', '.'])
        && !b.ends_with(['/', '.'])
        && !b.contains("..")
        && !b.contains("//")
        && !b.ends_with(".lock")
}

pub(crate) fn private_dir(p: &Path) -> Result<()> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => return Err(Error::Key("a store directory is a symlink or not a directory")),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(p).map_err(io)?,
        Err(e) => return Err(io(e)),
    }
    fs::set_permissions(p, fs::Permissions::from_mode(0o700)).map_err(io)
}

impl Store {
    /// Open the store's directories, creating them `0700`, and the bare clone if it is not there yet.
    /// `allow_file` admits local-path and `file://` store URLs (tests, shared drives); remote helpers and
    /// cleartext transports are never admitted.
    pub fn open(home: &Path, org: &str, store: &str, url: &str, allow_file: bool) -> Result<Store> {
        let paths = StorePaths::new(home, org, store)?;
        let url = StoreUrl::parse(url, allow_file)?;
        for d in [home.join("xenia"), home.join("xenia").join(org), paths.root.clone(), paths.state.clone(), paths.empty_hooks.clone(), paths.empty_home.clone()] {
            private_dir(&d)?;
        }
        let s = Store { org: org.to_string(), store: store.to_string(), paths, url, branch: "main".into(), caps: Caps::default() };
        s.init_clone()?;
        Ok(s)
    }

    pub fn with_caps(mut self, caps: Caps) -> Store {
        self.caps = caps;
        self
    }

    pub fn with_branch(mut self, branch: &str) -> Result<Store> {
        if !valid_branch(branch) {
            return Err(Error::BadField("branch name"));
        }
        self.branch = branch.to_string();
        Ok(self)
    }

    fn init_clone(&self) -> Result<()> {
        let c = &self.paths.clone;
        match fs::symlink_metadata(c) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => return Err(Error::Key("the clone path is a symlink or not a directory")),
            Ok(_) => {
                if !c.join("HEAD").is_file() || !c.join("objects").is_dir() {
                    return Err(Error::Key("the clone directory is not a git repository"));
                }
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
        let mut cmd = git_command(&self.paths, None, None);
        cmd.args(["init", "--bare", "--quiet", "--initial-branch=main"]).arg(format!("--template={}", self.paths.empty_home.display())).arg(c);
        let out = run(cmd, None, 4096, Duration::from_secs(30), None).map_err(|_| Error::Io("git init failed".into()))?;
        if out.code != 0 {
            return Err(Error::Io(format!("git init failed: {}", short(&out.stderr))));
        }
        fs::set_permissions(c, fs::Permissions::from_mode(0o700)).map_err(io)
    }

    pub(crate) fn local(&self) -> Command {
        git_command(&self.paths, None, Some(&self.paths.clone))
    }

    pub(crate) fn git_ok(&self, args: &[&str], out_cap: usize) -> Result<Vec<u8>> {
        let mut c = self.local();
        c.args(args);
        let out = run(c, None, out_cap, Duration::from_secs(60), None).map_err(|_| Error::Io(format!("git {} failed", args.first().unwrap_or(&""))))?;
        if out.code != 0 {
            return Err(Error::Io(format!("git {} failed: {}", args.first().unwrap_or(&""), short(&out.stderr))));
        }
        Ok(out.stdout)
    }

    pub(crate) fn exit_code(&self, args: &[&str]) -> Result<i32> {
        let mut c = self.local();
        c.args(args);
        let out = run(c, None, 4096, Duration::from_secs(60), None).map_err(|_| Error::Io("git failed".into()))?;
        Ok(out.code)
    }

    pub(crate) fn is_ancestor(&self, a: &str, b: &str) -> Result<bool> {
        match self.exit_code(&["merge-base", "--is-ancestor", a, b])? {
            0 => Ok(true),
            1 => Ok(false),
            _ => Err(Error::Io("git merge-base failed".into())),
        }
    }

    /// Drop the staging ref and, when the fetch brought objects we refuse, the objects too.
    pub(crate) fn discard_fetched(&self, prune: bool) {
        let _ = self.git_ok(&["update-ref", "-d", REF_FETCHED], 4096);
        if prune {
            // A killed fetch leaves `tmp_pack_*`; unreachable objects are removed by an explicit gc.
            if let Ok(rd) = fs::read_dir(self.paths.clone.join("objects").join("pack")) {
                for e in rd.flatten() {
                    if e.file_name().to_string_lossy().starts_with("tmp_") {
                        let _ = fs::remove_file(e.path());
                    }
                }
            }
            let _ = self.git_ok(&["gc", "--prune=now", "--quiet"], 4096);
        }
    }

    /// Where X3's `pins.toml` lives for this store: `state/pins.toml`, next to the sticky file and outside the
    /// clone (spec 011 §3.5).
    pub fn pin_storage(&self) -> delphi_xenia::pins::FilePinStorage {
        delphi_xenia::pins::FilePinStorage::new(self.paths.state.join("pins.toml"))
    }

    /// The last accepted head, from the sticky file.
    pub fn accepted_head(&self) -> Result<Option<String>> {
        Ok(self.load_sticky()?.accepted_head)
    }

    /// Fetch the branch into the staging ref and check it. Does not change the accepted head or the sticky
    /// state. The result is what `sync` acts on.
    pub fn fetch(&self) -> Result<Fetch> {
        let accepted = self.load_sticky()?.accepted_head;
        self.fetch_checked(accepted.as_deref())
    }

    pub(crate) fn fetch_checked(&self, accepted: Option<&str>) -> Result<Fetch> {
        let objects = self.paths.clone.join("objects");
        let mut cmd = git_command(&self.paths, Some(self.url.scheme), Some(&self.paths.clone));
        cmd.args(["fetch", "--quiet", "--force", "--no-tags", "--no-recurse-submodules", "--no-write-fetch-head", "--no-auto-gc", "--no-auto-maintenance", "--no-show-forced-updates"]);
        cmd.arg(&self.url.url).arg(format!("+refs/heads/{}:{}", self.branch, REF_FETCHED));
        match run(cmd, None, 64 * 1024, Duration::from_secs(self.caps.fetch_timeout_secs), Some((&objects, self.caps.max_total_bytes))) {
            Err(RunFail::DiskCap) | Err(RunFail::TooMuchOutput) => {
                self.discard_fetched(true);
                return Ok(Fetch::Refused(Refused::TooLarge));
            }
            Err(RunFail::Timeout) => {
                self.discard_fetched(true);
                return Ok(Fetch::Unavailable("the fetch timed out".into()));
            }
            Err(RunFail::Spawn(e)) => return Err(Error::Io(format!("cannot run git: {e}"))),
            Ok(out) if out.code != 0 => {
                // A failed fetch must not leave a previous staging ref that looks like news.
                self.discard_fetched(false);
                return Ok(Fetch::Unavailable(short(&out.stderr)));
            }
            Ok(_) => {}
        }
        let fetched = match self.git_ok(&["rev-parse", "--verify", "--quiet", &format!("{REF_FETCHED}^{{commit}}")], 256) {
            Ok(o) => String::from_utf8_lossy(&o).trim().to_string(),
            Err(_) => {
                self.discard_fetched(true);
                return Ok(Fetch::Refused(Refused::Malformed("the fetched head is not a commit")));
            }
        };
        if !delphi_xenia::sticky::is_commit_id(&fetched) {
            self.discard_fetched(true);
            return Ok(Fetch::Refused(Refused::Malformed("the fetched head is not a commit id")));
        }
        if accepted == Some(fetched.as_str()) {
            return Ok(Fetch::UpToDate { head: fetched });
        }
        // Objects first: whatever the history looks like, an oversize or enormous fetch is refused and removed.
        if let Some(r) = self.check_new_objects(&fetched, accepted)? {
            self.discard_fetched(true);
            return Ok(Fetch::Refused(r));
        }
        if let Some(acc) = accepted {
            if !self.is_ancestor(acc, &fetched)? {
                let kind = if self.is_ancestor(&fetched, acc)? { Divergence::Behind } else { Divergence::Diverged };
                return Ok(Fetch::NonFastForward(NonFastForward { accepted: acc.to_string(), fetched, kind }));
            }
        }
        match self.check_tip(&fetched)? {
            Ok(entries) => Ok(Fetch::Advanced { head: fetched, from: accepted.map(str::to_string), entries }),
            Err(r) => {
                self.discard_fetched(false);
                Ok(Fetch::Refused(r))
            }
        }
    }

    /// Fetch one other branch of the store (an admin's `roster-proposal/<n>`) into `local_ref`, under the same
    /// hardened git, the same timeout and the same object caps as a sync (everything reachable from the fetched
    /// commit and not from the accepted head is counted). Nothing is checked out and nothing is read from the tree
    /// here; the caller reads named blobs by object id. A refused fetch removes the ref and its objects.
    pub(crate) fn fetch_aux(&self, remote_branch: &str, local_ref: &str) -> Result<AuxFetch> {
        if !valid_branch(remote_branch) || !local_ref.starts_with("refs/xenia/") || local_ref.contains("..") {
            return Err(Error::BadField("branch name"));
        }
        let accepted = self.load_sticky()?.accepted_head;
        let objects = self.paths.clone.join("objects");
        let mut cmd = git_command(&self.paths, Some(self.url.scheme), Some(&self.paths.clone));
        cmd.args(["fetch", "--quiet", "--force", "--no-tags", "--no-recurse-submodules", "--no-write-fetch-head", "--no-auto-gc", "--no-auto-maintenance", "--no-show-forced-updates"]);
        cmd.arg(&self.url.url).arg(format!("+refs/heads/{remote_branch}:{local_ref}"));
        let drop_ref = |prune: bool| {
            let _ = self.git_ok(&["update-ref", "-d", local_ref], 4096);
            if prune {
                let _ = self.git_ok(&["gc", "--prune=now", "--quiet"], 4096);
            }
        };
        match run(cmd, None, 64 * 1024, Duration::from_secs(self.caps.fetch_timeout_secs), Some((&objects, self.caps.max_total_bytes))) {
            Err(RunFail::DiskCap) | Err(RunFail::TooMuchOutput) => {
                drop_ref(true);
                return Ok(AuxFetch::Refused(Refused::TooLarge));
            }
            Err(RunFail::Timeout) => {
                drop_ref(true);
                return Ok(AuxFetch::Unavailable("the fetch timed out".into()));
            }
            Err(RunFail::Spawn(e)) => return Err(Error::Io(format!("cannot run git: {e}"))),
            Ok(out) if out.code != 0 => {
                drop_ref(false);
                if out.stderr.contains("couldn't find remote ref") {
                    return Ok(AuxFetch::Missing);
                }
                return Ok(AuxFetch::Unavailable(short(&out.stderr)));
            }
            Ok(_) => {}
        }
        let tip = match self.git_ok(&["rev-parse", "--verify", "--quiet", &format!("{local_ref}^{{commit}}")], 256) {
            Ok(o) => String::from_utf8_lossy(&o).trim().to_string(),
            Err(_) => {
                drop_ref(true);
                return Ok(AuxFetch::Refused(Refused::Malformed("the fetched head is not a commit")));
            }
        };
        if !delphi_xenia::sticky::is_commit_id(&tip) {
            drop_ref(true);
            return Ok(AuxFetch::Refused(Refused::Malformed("the fetched head is not a commit id")));
        }
        if let Some(r) = self.check_new_objects(&tip, accepted.as_deref())? {
            drop_ref(true);
            return Ok(AuxFetch::Refused(r));
        }
        Ok(AuxFetch::Tip(tip))
    }

    pub(crate) fn drop_ref(&self, name: &str) {
        let _ = self.git_ok(&["update-ref", "-d", name], 4096);
    }

    /// The tip-tree rules for one commit: listable, no hostile entries, and a `.gitattributes` that keeps
    /// bytes exact. Also what rewrite recovery runs on a head that is not a fast-forward.
    pub(crate) fn check_tip(&self, commit: &str) -> Result<std::result::Result<Vec<TreeEntry>, Refused>> {
        let entries = match self.list_tree(commit)? {
            Ok(e) => e,
            Err(r) => return Ok(Err(r)),
        };
        let bad = store::check_tree(&entries, &self.caps);
        if !bad.is_empty() {
            return Ok(Err(Refused::Tree(bad)));
        }
        let attrs = match entries.iter().find(|e| e.path == b".gitattributes") {
            Some(e) => Some(self.read_blobs(std::slice::from_ref(e))?.remove(0)),
            None => None,
        };
        if let Err(a) = store::check_gitattributes(attrs.as_deref()) {
            return Ok(Err(Refused::Gitattributes(a)));
        }
        Ok(Ok(entries))
    }

    /// Count, size and total the objects a fetch added (everything reachable from `fetched` and not from
    /// `accepted`), without reading their content.
    pub(crate) fn check_new_objects(&self, fetched: &str, accepted: Option<&str>) -> Result<Option<Refused>> {
        let mut args = vec!["rev-list", "--objects", "--no-object-names", fetched];
        let not_acc = accepted.map(|a| format!("^{a}"));
        if let Some(n) = &not_acc {
            args.push(n);
        }
        let mut c = self.local();
        c.args(&args);
        let cap = (self.caps.max_new_objects as usize + 1) * 65;
        let out = match run(c, None, cap, Duration::from_secs(self.caps.fetch_timeout_secs), None) {
            Ok(o) => o,
            Err(RunFail::TooMuchOutput) => return Ok(Some(Refused::TooManyObjects)),
            Err(RunFail::Timeout) => return Ok(Some(Refused::Malformed("listing new objects timed out"))),
            Err(_) => return Err(Error::Io("git rev-list failed".into())),
        };
        if out.code != 0 {
            return Ok(Some(Refused::Malformed("objects are missing or corrupt")));
        }
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let shas: Vec<&str> = text.lines().map(|l| l.split(' ').next().unwrap_or("")).filter(|l| !l.is_empty()).collect();
        if shas.len() as u64 > self.caps.max_new_objects {
            return Ok(Some(Refused::TooManyObjects));
        }
        if shas.iter().any(|s| !delphi_xenia::sticky::is_commit_id(s)) {
            return Ok(Some(Refused::Malformed("unexpected object listing")));
        }
        let mut c = self.local();
        c.args(["cat-file", "--batch-check"]);
        let stdin = shas.iter().map(|s| format!("{s}\n")).collect::<String>().into_bytes();
        let out = match run(c, Some(stdin), shas.len() * 160 + 1024, Duration::from_secs(self.caps.fetch_timeout_secs), None) {
            Ok(o) if o.code == 0 => o,
            Ok(_) => return Ok(Some(Refused::Malformed("objects are missing or corrupt"))),
            Err(RunFail::TooMuchOutput) | Err(RunFail::Timeout) => return Ok(Some(Refused::Malformed("measuring objects failed"))),
            Err(_) => return Err(Error::Io("git cat-file failed".into())),
        };
        let (mut total, mut commits) = (0u64, 0u64);
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let f: Vec<&str> = line.split(' ').collect();
            let [sha, otype, size] = f[..] else {
                return Ok(Some(Refused::Malformed("an object is missing or unreadable")));
            };
            let Ok(size) = size.parse::<u64>() else {
                return Ok(Some(Refused::Malformed("object size")));
            };
            if size > self.caps.max_object_bytes {
                return Ok(Some(Refused::ObjectTooLarge { sha: sha.to_string(), size }));
            }
            total += size;
            if otype == "commit" {
                commits += 1;
            }
        }
        if total > self.caps.max_total_bytes {
            return Ok(Some(Refused::TooLarge));
        }
        if commits > self.caps.max_new_commits {
            return Ok(Some(Refused::TooManyCommits));
        }
        Ok(None)
    }

    pub(crate) fn list_tree(&self, commit: &str) -> Result<std::result::Result<Vec<TreeEntry>, Refused>> {
        let mut c = self.local();
        c.args(["ls-tree", "-r", "-z", "--long", commit]);
        let cap = self.caps.max_tree_entries.saturating_add(1) * (self.caps.max_path_bytes * 4 + 128);
        let out = match run(c, None, cap, Duration::from_secs(60), None) {
            Ok(o) if o.code == 0 => o,
            Ok(_) => return Ok(Err(Refused::Malformed("the tree cannot be listed"))),
            Err(RunFail::TooMuchOutput) => return Ok(Err(Refused::TreeTooLarge)),
            Err(RunFail::Timeout) => return Ok(Err(Refused::Malformed("listing the tree timed out"))),
            Err(_) => return Err(Error::Io("git ls-tree failed".into())),
        };
        match store::parse_ls_tree(&out.stdout, &self.caps) {
            Ok(e) => Ok(Ok(e)),
            Err(Error::Oversize(_)) => Ok(Err(Refused::TreeTooLarge)),
            Err(_) => Ok(Err(Refused::Malformed("the tree listing is malformed"))),
        }
    }

    /// Read blobs by object id in one `cat-file --batch`, which applies no filters, attributes or line-ending
    /// conversion: the bytes are exactly the committed ones, whatever `core.autocrlf` anyone has set.
    pub(crate) fn read_blobs(&self, entries: &[TreeEntry]) -> Result<Vec<Vec<u8>>> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let mut want = 0usize;
        for e in entries {
            want += e.size.ok_or(Error::BadPath("a gitlink has no content"))? as usize + 160;
        }
        let mut c = self.local();
        c.args(["cat-file", "--batch"]);
        let stdin = entries.iter().map(|e| format!("{}\n", e.sha)).collect::<String>().into_bytes();
        let out = run(c, Some(stdin), want + 1024, Duration::from_secs(60), None).map_err(|_| Error::Io("git cat-file failed".into()))?;
        if out.code != 0 {
            return Err(Error::Io("git cat-file failed".into()));
        }
        let (buf, mut pos) = (&out.stdout, 0usize);
        let mut blobs = Vec::new();
        for e in entries {
            let nl = buf[pos..].iter().position(|b| *b == b'\n').ok_or(Error::Io("truncated cat-file output".into()))?;
            let header = String::from_utf8_lossy(&buf[pos..pos + nl]).into_owned();
            pos += nl + 1;
            let f: Vec<&str> = header.split(' ').collect();
            let [sha, otype, size] = f[..] else {
                return Err(Error::Io("an object is missing".into()));
            };
            let size: usize = size.parse().map_err(|_| Error::Io("object size".into()))?;
            if sha != e.sha || otype != "blob" || Some(size as u64) != e.size || buf.len() < pos + size + 1 {
                return Err(Error::Io("cat-file disagrees with ls-tree".into()));
            }
            blobs.push(buf[pos..pos + size].to_vec());
            pos += size + 1;
        }
        Ok(blobs)
    }

    /// The files under `records/` and `roster/` at `head`'s tip. Only `Fetch::Advanced` hands out entries.
    pub(crate) fn read_snapshot(&self, head: &str, entries: &[TreeEntry]) -> Result<Snapshot> {
        let tracked: Vec<TreeEntry> = entries.iter().filter(|e| std::str::from_utf8(&e.path).map(store::is_tracked_area).unwrap_or(false)).cloned().collect();
        let blobs = self.read_blobs(&tracked)?;
        let files = tracked.iter().zip(blobs).map(|(e, b)| (String::from_utf8_lossy(&e.path).into_owned(), b)).collect();
        Ok(Snapshot { head: head.to_string(), files })
    }

    // -----------------------------------------------------------------------------------------------------
    // Sticky state on disk.

    pub fn load_sticky(&self) -> Result<StickyState> {
        let p = &self.paths.sticky;
        let m = match fs::symlink_metadata(p) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(StickyState::new(&self.org, &self.store)),
            Err(e) => return Err(io(e)),
        };
        if !m.file_type().is_file() {
            return Err(Error::Key("the sticky state path is not a regular file"));
        }
        if m.permissions().mode() & 0o077 != 0 {
            return Err(Error::Key("the sticky state file is accessible to others (expected mode 0600)"));
        }
        let bytes = fs::read(p).map_err(io)?;
        StickyState::from_json(&bytes, &self.org, &self.store)
    }

    pub(crate) fn save_sticky(&self, s: &StickyState) -> Result<()> {
        let dir = &self.paths.state;
        private_dir(dir)?;
        let tmp = dir.join(format!(".sticky.{}.tmp", std::process::id()));
        let _ = fs::remove_file(&tmp);
        let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(io)?;
        f.write_all(&s.to_json()).and_then(|_| f.sync_all()).map_err(io)?;
        drop(f);
        fs::rename(&tmp, &self.paths.sticky).map_err(io)?;
        fs::File::open(dir).and_then(|d| d.sync_all()).map_err(io)
    }

    /// Fetch, check, classify and, only if every sticky rule holds, accept. Serialised by a lock so two
    /// syncs of one store cannot interleave. Idempotent: a second sync of an unchanged host reads and writes
    /// nothing.
    pub fn sync(&self, classify: Classifier<'_>) -> Result<SyncReport> {
        let lock = self.paths.root.join(".lock");
        crate::lock::with_lock(&lock, || self.sync_locked(classify)).map_err(io)?
    }

    fn sync_locked(&self, classify: Classifier<'_>) -> Result<SyncReport> {
        let mut sticky = self.load_sticky()?;
        let (head, from, entries) = match self.fetch_checked(sticky.accepted_head.as_deref())? {
            Fetch::UpToDate { head } => return Ok(SyncReport::UpToDate { head }),
            Fetch::NonFastForward(n) => return Ok(SyncReport::NonFastForward(n)),
            Fetch::Refused(r) => return Ok(SyncReport::Refused(r)),
            Fetch::Unavailable(m) => return Ok(SyncReport::Unavailable(m)),
            Fetch::Advanced { head, from, entries } => (head, from, entries),
        };
        let snapshot = self.read_snapshot(&head, &entries)?;
        let (mut observed, mut invalid, mut quarantined) = (Vec::new(), Vec::new(), Vec::new());
        for (path, bytes) in &snapshot.files {
            match classify(path, bytes) {
                Ok(tracking) => observed.push(Observation { path: path.clone(), tracking, hash: hash::of(bytes) }),
                Err(reason) => {
                    invalid.push(path.clone());
                    quarantined.push((path.clone(), reason));
                }
            }
        }
        let ev: Evaluation = sticky.evaluate(&observed, &invalid);
        if !ev.is_clean() {
            return Ok(SyncReport::Tampering { head, refusals: ev.refusals });
        }
        sticky.apply(&ev)?;
        sticky.accepted_head = Some(head.clone());
        // The anchor first: if we crash between the two, the sticky file still names the old head and the
        // next sync redoes this one.
        self.git_ok(&["update-ref", REF_ACCEPTED, &head], 4096)?;
        self.save_sticky(&sticky)?;
        Ok(SyncReport::Accepted { head, from, snapshot, quarantined, added: ev.added.len(), superseded: ev.superseded.len(), unchanged: ev.unchanged })
    }

    /// The files at a commit already in the clone (for example the accepted head), re-read from git.
    pub fn snapshot_of(&self, head: &str) -> Result<Snapshot> {
        if !delphi_xenia::sticky::is_commit_id(head) {
            return Err(Error::BadField("not a commit id"));
        }
        match self.list_tree(head)? {
            Ok(entries) => self.read_snapshot(head, &entries),
            Err(_) => Err(Error::Io("the tree cannot be listed".into())),
        }
    }

    pub fn clone_dir(&self) -> PathBuf {
        self.paths.clone.clone()
    }
}

#[cfg(test)]
#[path = "xenia_store_tests.rs"]
mod tests;
