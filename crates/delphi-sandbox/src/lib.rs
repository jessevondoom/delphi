//! OS-level sandbox (`specs/008` §4, `specs/009` §7, D-04).
//!
//! A [`SandboxSpec`] says what a process may see and touch; [`Sandbox::command`] turns it into a
//! `std::process::Command` that runs the program under `sandbox-exec` (macOS) or `bwrap` (Linux).
//! Everything is **deny-by-default**, reads included: a path that is not listed cannot be opened, and on
//! macOS cannot even be `stat`ed. The one thing a spec cannot grant is a write to any path segment named
//! `.git`, in the worktree or anywhere else it is granted.
//!
//! The profile is passed inline (`sandbox-exec -p`), never through a temp file, so there is nothing to
//! swap between writing it and applying it. The environment is cleared and rebuilt from an allowlist.

pub mod egress;
pub mod run;

pub use run::{run_bounded, RunOptions, RunOutcome};

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What the sandboxed process may do on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Network {
    /// Nothing, not even loopback.
    Off,
    /// Connect only to this TCP port on loopback (the daemon's CONNECT proxy, `specs/009` §7.4).
    ProxyOnly { port: u16 },
}

#[derive(Debug, Clone)]
pub struct SandboxSpec {
    /// Read-write, except that no `.git` segment is writable (the task worktree).
    pub worktree: Option<PathBuf>,
    /// Read-write scratch space: a temp dir, the ephemeral per-task state dir. Also never `.git`-writable.
    pub scratch: Vec<PathBuf>,
    /// Read-only, and executable: the toolchain and anything else the program needs to run.
    pub read_only: Vec<PathBuf>,
    pub network: Network,
    /// macOS Mach services the process may look up (for example the keychain daemon). Names only; each is
    /// validated, and nothing is allowed by default. Ignored by `bwrap`, which has no such concept.
    pub mach_services: Vec<String>,
    /// The entire environment of the process. Nothing is inherited.
    pub env: BTreeMap<String, String>,
    pub limits: Limits,
}

/// Resource limits applied with `setrlimit` before the program starts. They bound what a runaway or
/// hostile program can consume; they are not a substitute for the wall-clock timeout in [`run_bounded`].
///
/// Deliberately absent: a process-count limit (`RLIMIT_NPROC` counts every process of the user, so a low
/// value breaks the sandbox's own `fork` and a high one stops nothing) and an address-space limit (not
/// enforced on macOS). A fork bomb is stopped by the timeout killing the whole process group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// CPU seconds (the kernel signals `SIGXCPU` at the limit and kills five seconds later).
    pub cpu_secs: Option<u64>,
    /// The largest single file the program may write.
    pub max_file_bytes: Option<u64>,
    pub max_open_files: Option<u64>,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { cpu_secs: Some(900), max_file_bytes: Some(512 << 20), max_open_files: Some(1024) }
    }
}

impl SandboxSpec {
    pub fn new() -> SandboxSpec {
        SandboxSpec { worktree: None, scratch: vec![], read_only: system_read_only(), network: Network::Off, mach_services: vec![], env: BTreeMap::new(), limits: Limits::default() }
    }
}

impl Default for SandboxSpec {
    fn default() -> SandboxSpec {
        SandboxSpec::new()
    }
}

/// The minimum a dynamically linked program needs to start. Callers add their toolchain.
pub fn system_read_only() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        ["/usr/lib", "/usr/bin", "/bin", "/System", "/Library/Preferences/Logging", "/private/var/db/dyld", "/usr/share"].iter().map(PathBuf::from).collect()
    } else {
        ["/usr", "/bin", "/lib", "/lib64", "/etc/ld.so.cache"].iter().map(PathBuf::from).collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SandboxError {
    /// No sandbox is available on this machine: execution falls back to Amber (D-04).
    Unavailable,
    /// A path is missing, not absolute, or cannot be put in a profile safely.
    BadPath(String),
    Unsupported(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxError::Unavailable => write!(f, "no OS sandbox is available"),
            SandboxError::BadPath(p) => write!(f, "bad sandbox path: {}", p),
            SandboxError::Unsupported(m) => write!(f, "unsupported: {}", m),
        }
    }
}

impl std::error::Error for SandboxError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    SandboxExec,
    Bwrap,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::SandboxExec => "sandbox-exec",
            Mode::Bwrap => "bubblewrap",
        }
    }
}

/// Which sandbox this machine has, proven by running a trivial program under it.
pub fn detect() -> Option<Mode> {
    let probe = |mode: Mode| -> bool {
        let spec = SandboxSpec::new();
        match Sandbox::with_mode(mode).command(&spec, "/usr/bin/true", std::iter::empty::<&str>()) {
            Ok(mut c) => c.status().map(|s| s.success()).unwrap_or(false),
            Err(_) => false,
        }
    };
    if cfg!(target_os = "macos") && probe(Mode::SandboxExec) {
        return Some(Mode::SandboxExec);
    }
    if cfg!(target_os = "linux") && probe(Mode::Bwrap) {
        return Some(Mode::Bwrap);
    }
    None
}

pub struct Sandbox {
    mode: Mode,
}

impl Sandbox {
    /// The sandbox for this machine, or `Unavailable`.
    pub fn detect() -> Result<Sandbox, SandboxError> {
        detect().map(|mode| Sandbox { mode }).ok_or(SandboxError::Unavailable)
    }

    pub fn with_mode(mode: Mode) -> Sandbox {
        Sandbox { mode }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// A command that runs `program args…` under the spec. The caller sets the working directory, stdio and
    /// anything else that is not about confinement.
    pub fn command<I, S>(&self, spec: &SandboxSpec, program: impl AsRef<OsStr>, args: I) -> Result<Command, SandboxError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let resolved = Resolved::new(spec)?;
        let mut cmd = match self.mode {
            Mode::SandboxExec => {
                let mut c = Command::new("/usr/bin/sandbox-exec");
                c.arg("-p").arg(sbpl(&resolved));
                c.arg(program.as_ref());
                c
            }
            Mode::Bwrap => {
                let mut c = Command::new("bwrap");
                c.args(bwrap_args(&resolved)?);
                c.arg("--").arg(program.as_ref());
                c
            }
        };
        cmd.args(args);
        cmd.env_clear();
        cmd.envs(&spec.env);
        harden_process(&mut cmd, spec.limits);
        Ok(cmd)
    }
}

/// Applied to every program Delphi starts for a task, sandboxed or not: its own session and process group
/// (so the tree can be signalled at once, with no controlling terminal), resource limits, and no inherited
/// descriptors (the daemon passes its lock and readiness descriptors across its own `exec`).
fn harden_process(cmd: &mut Command, limits: Limits) {
    // SAFETY: only async-signal-safe calls (`setsid`, `setrlimit`, `fcntl`, `close`, `prctl`) run between fork and exec.
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(move || {
            libc::setsid();
            #[cfg(target_os = "linux")]
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            let set = |resource, soft: u64, hard: u64| {
                let rl = libc::rlimit { rlim_cur: soft as libc::rlim_t, rlim_max: hard as libc::rlim_t };
                libc::setrlimit(resource, &rl);
            };
            set(libc::RLIMIT_CORE, 0, 0);
            if let Some(n) = limits.cpu_secs {
                set(libc::RLIMIT_CPU, n, n + 5);
            }
            if let Some(n) = limits.max_file_bytes {
                set(libc::RLIMIT_FSIZE, n, n);
            }
            if let Some(n) = limits.max_open_files {
                set(libc::RLIMIT_NOFILE, n, n);
            }
            let max = libc::getdtablesize().clamp(0, 65_536);
            for fd in 3..max {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                // Close-on-exec descriptors (including std's own exec-error pipe) vanish at exec anyway.
                if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                    libc::close(fd);
                }
            }
            Ok(())
        });
    }
}

/// A program run directly, **without** the OS sandbox, but with the same process hardening (own process
/// group, limits, no inherited descriptors) and an explicit environment. Used when the operator has not
/// enabled the sandbox; what stands between the program and the machine is then the approval card.
pub fn plain_command<I, S>(limits: Limits, env: &BTreeMap<String, String>, program: impl AsRef<OsStr>, args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.env_clear();
    cmd.envs(env);
    harden_process(&mut cmd, limits);
    cmd
}

/// The spec with every path made absolute and canonical (symlinks resolved), which is what both sandboxes
/// match against.
struct Resolved {
    worktree: Option<PathBuf>,
    scratch: Vec<PathBuf>,
    read_only: Vec<PathBuf>,
    network: Network,
    mach_services: Vec<String>,
}

fn canonical(p: &Path) -> Result<PathBuf, SandboxError> {
    let bad = |why: &str| SandboxError::BadPath(format!("{} ({})", p.display(), why));
    if !p.is_absolute() {
        return Err(bad("not absolute"));
    }
    let s = p.to_string_lossy();
    if s.contains('\0') || s.contains('\n') || s.contains('\r') {
        return Err(bad("control character"));
    }
    std::fs::canonicalize(p).map_err(|e: io::Error| bad(&e.to_string()))
}

impl Resolved {
    fn new(spec: &SandboxSpec) -> Result<Resolved, SandboxError> {
        let worktree = spec.worktree.as_deref().map(canonical).transpose()?;
        let scratch = spec.scratch.iter().map(|p| canonical(p)).collect::<Result<Vec<_>, _>>()?;
        // A missing toolchain directory is not an error: it simply is not there to be read.
        let read_only = spec.read_only.iter().filter_map(|p| canonical(p).ok()).collect();
        let ok_service = |n: &str| !n.is_empty() && n.len() <= 128 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
        if let Some(bad) = spec.mach_services.iter().find(|n| !ok_service(n)) {
            return Err(SandboxError::BadPath(format!("mach service `{}`", bad)));
        }
        Ok(Resolved { worktree, scratch, read_only, network: spec.network.clone(), mach_services: spec.mach_services.clone() })
    }

    fn writable(&self) -> impl Iterator<Item = &PathBuf> {
        self.worktree.iter().chain(self.scratch.iter())
    }
}

/// Quote a path for an SBPL string literal.
fn sbpl_str(p: &Path) -> String {
    let mut out = String::from("\"");
    for c in p.to_string_lossy().chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The macOS profile. Rules are evaluated in order and the last match wins, so the `.git` denial comes last.
fn sbpl(r: &Resolved) -> String {
    let mut s = String::from("(version 1)\n(deny default)\n(allow process-fork)\n(allow sysctl-read)\n(allow signal (target self))\n");
    // Executing and reading the toolchain.
    for p in &r.read_only {
        let _ = writeln!(s, "(allow process-exec* (subpath {q}))\n(allow file-read* (subpath {q}))", q = sbpl_str(p));
    }
    // dyld reads the root directory itself when a program starts (the names of top-level directories, nothing below them).
    s.push_str("(allow file-read-data (literal \"/\"))\n");
    let _ = writeln!(s, "(allow file-read* (literal \"/dev/null\") (literal \"/dev/urandom\") (literal \"/dev/random\") (literal \"/private/var/select/sh\"))\n(allow file-write* (literal \"/dev/null\"))");
    // Read-write areas (and the programs inside them, so a build can run what it built).
    for p in r.writable() {
        let _ = writeln!(s, "(allow file-read* file-write* (subpath {q}))\n(allow process-exec* (subpath {q}))", q = sbpl_str(p));
    }
    // Only the ancestors of granted paths can be `stat`ed, so a path outside them cannot even be probed.
    let mut ancestors = std::collections::BTreeSet::new();
    for p in r.read_only.iter().chain(r.writable()) {
        let mut cur = p.parent();
        while let Some(a) = cur {
            ancestors.insert(a.to_path_buf());
            cur = a.parent();
        }
    }
    // The system symlinks (`/etc`, `/var`, `/tmp` point into `/private`), which most programs stat on startup.
    for l in ["/etc", "/var", "/tmp", "/private", "/private/etc", "/private/var", "/private/tmp"] {
        let _ = writeln!(s, "(allow file-read-metadata (literal \"{}\"))", l);
    }
    for a in &ancestors {
        let _ = writeln!(s, "(allow file-read-metadata (literal {}))", sbpl_str(a));
    }
    for p in r.read_only.iter().chain(r.writable()) {
        let _ = writeln!(s, "(allow file-read-metadata (subpath {}))", sbpl_str(p));
    }
    for n in &r.mach_services {
        let _ = writeln!(s, "(allow mach-lookup (global-name {}))", sbpl_str(Path::new(n)));
    }
    match r.network {
        Network::Off => {}
        Network::ProxyOnly { port } => {
            let _ = writeln!(s, "(allow network-outbound (remote ip \"localhost:{}\"))", port);
        }
    }
    // Last, so it wins: no `.git` segment is ever writable, file or directory, at any depth.
    s.push_str("(deny file-write* (regex #\"(^|/)\\.git(/|$)\"))\n");
    s
}

/// The Linux invocation. Existing `.git` entries are re-bound read-only. Note the limit of the mechanism:
/// `bwrap` cannot forbid *creating* a new `.git` inside the writable tree, so the daemon never discovers a
/// repository from a worktree (it passes `--git-dir` explicitly, `specs/008` §4) and the merge gate flags any
/// diff containing one.
fn bwrap_args(r: &Resolved) -> Result<Vec<String>, SandboxError> {
    let mut a: Vec<String> = ["--unshare-all", "--die-with-parent", "--new-session", "--proc", "/proc", "--dev", "/dev"].iter().map(|s| s.to_string()).collect();
    match r.network {
        Network::Off => {}
        Network::ProxyOnly { .. } => return Err(SandboxError::Unsupported("proxy-only networking on Linux needs a socket bridge (roadmap Step 3, harness mode)".into())),
    }
    let s = |p: &Path| p.to_string_lossy().into_owned();
    for p in &r.read_only {
        a.extend(["--ro-bind".to_string(), s(p), s(p)]);
    }
    for p in r.writable() {
        a.extend(["--bind".to_string(), s(p), s(p)]);
    }
    for root in r.writable() {
        for git in find_git(root) {
            a.extend(["--ro-bind".to_string(), s(&git), s(&git)]);
        }
    }
    Ok(a)
}

/// Every existing path segment named `.git` under `root` (file or directory), without descending into one.
fn find_git(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if e.file_name() == ".git" {
                out.push(path);
            } else if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(path);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn links_cannot_be_used_to_reach_files_outside_the_grant() {
        let Some(sb) = sandbox() else { return };
        let f = fx();
        let s = spec(&f);
        let secret = f.outside.join("secret.txt");
        // A hard link to an outside file, created inside the writable tree, must not make it readable.
        let hard = sh(&sb, &s, &format!("ln {} {} && cat {}", secret.display(), f.work.join("hard").display(), f.work.join("hard").display()));
        assert!(!String::from_utf8_lossy(&hard.stdout).contains("hidden"), "hard link read the secret: {:?}", hard);
        // A symlink to it is followed to the real path, which is not granted.
        let soft = sh(&sb, &s, &format!("ln -s {} {} && cat {}", secret.display(), f.work.join("soft").display(), f.work.join("soft").display()));
        assert!(!String::from_utf8_lossy(&soft.stdout).contains("hidden"), "symlink read the secret: {:?}", soft);
    }

    #[test]
    fn file_descriptors_inherited_from_the_parent_do_not_reach_the_child() {
        let Some(sb) = sandbox() else { return };
        use std::os::unix::io::AsRawFd;
        let f = fx();
        let secret = fs::File::open(f.outside.join("secret.txt")).unwrap();
        // Simulate a descriptor that was passed across exec (as the daemon's lock fd is): clear CLOEXEC.
        let fd = secret.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
        }
        let out = sh(&sb, &spec(&f), &format!("cat <&{} 2>&1", fd));
        assert!(!String::from_utf8_lossy(&out.stdout).contains("hidden"), "an inherited descriptor leaked into the sandbox: {:?}", out);
        drop(secret);
    }


    use super::*;
    use std::fs;
    use std::process::Output;

    fn sandbox() -> Option<Sandbox> {
        Sandbox::detect().ok()
    }

    fn run(sb: &Sandbox, spec: &SandboxSpec, program: &str, args: &[&str]) -> Output {
        sb.command(spec, program, args).unwrap().output().unwrap()
    }

    fn sh(sb: &Sandbox, spec: &SandboxSpec, script: &str) -> Output {
        run(sb, spec, "/bin/sh", &["-c", script])
    }

    struct Fx {
        _dir: tempfile::TempDir,
        work: PathBuf,
        scratch: PathBuf,
        outside: PathBuf,
    }

    fn fx() -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let (work, scratch, outside) = (root.join("work"), root.join("scratch"), root.join("outside"));
        for d in [&work, &scratch, &outside, &work.join("src"), &work.join("sub/.git")] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(work.join("a.txt"), "inside").unwrap();
        fs::write(outside.join("secret.txt"), "hidden").unwrap();
        fs::write(work.join("sub/.git/config"), "[core]").unwrap();
        Fx { _dir: dir, work, scratch, outside }
    }

    fn spec(f: &Fx) -> SandboxSpec {
        let mut s = SandboxSpec::new();
        s.worktree = Some(f.work.clone());
        s.scratch = vec![f.scratch.clone()];
        s
    }

    // The other tests return early when no sandbox exists; on the platforms we develop on that must not be silent.
    #[test]
    fn the_sandbox_is_actually_available_on_macos() {
        if cfg!(target_os = "macos") {
            assert_eq!(detect(), Some(Mode::SandboxExec), "sandbox-exec must work here, or the other tests prove nothing");
        }
    }

    #[test]
    fn reads_are_denied_by_default_and_outside_files_cannot_even_be_probed() {
        let Some(sb) = sandbox() else { return };
        let f = fx();
        let s = spec(&f);
        let ok = sh(&sb, &s, &format!("cat {}", f.work.join("a.txt").display()));
        assert_eq!(String::from_utf8_lossy(&ok.stdout), "inside", "{:?}", ok);
        let no = sh(&sb, &s, &format!("cat {}", f.outside.join("secret.txt").display()));
        assert!(!no.status.success() && no.stdout.is_empty());
        // Not even existence is visible outside the granted paths (macOS; bwrap simply does not mount them).
        let probe = sh(&sb, &s, &format!("test -e {}", f.outside.join("secret.txt").display()));
        assert!(!probe.status.success(), "an unlisted path must look absent or be denied");
    }

    #[test]
    fn writes_are_confined_to_the_worktree_and_scratch() {
        let Some(sb) = sandbox() else { return };
        let f = fx();
        let s = spec(&f);
        assert!(sh(&sb, &s, &format!("echo x > {}", f.work.join("new.txt").display())).status.success());
        assert!(sh(&sb, &s, &format!("echo x > {}", f.scratch.join("tmp.txt").display())).status.success());
        assert!(!sh(&sb, &s, &format!("echo x > {}", f.outside.join("new.txt").display())).status.success());
        assert!(!f.outside.join("new.txt").exists());
    }

    #[test]
    fn no_git_segment_is_ever_writable_at_any_depth() {
        let Some(sb) = sandbox() else { return };
        let f = fx();
        let s = spec(&f);
        // Existing .git directory content, a new file in it, a .git created later, and a .git *file* (a linked worktree's pointer).
        assert!(!sh(&sb, &s, &format!("echo x >> {}", f.work.join("sub/.git/config").display())).status.success());
        assert!(!sh(&sb, &s, &format!("echo x > {}", f.work.join("sub/.git/hooks-new").display())).status.success());
        fs::write(f.work.join(".git"), "gitdir: /elsewhere").unwrap();
        assert!(!sh(&sb, &s, &format!("echo 'gitdir: /evil' > {}", f.work.join(".git").display())).status.success());
        assert_eq!(fs::read_to_string(f.work.join(".git")).unwrap(), "gitdir: /elsewhere");
        if sb.mode() == Mode::SandboxExec {
            // On macOS the rule is by name, so a brand-new .git is refused too (bwrap cannot do this; see bwrap_args).
            assert!(!sh(&sb, &s, &format!("mkdir {}", f.work.join("src/.git").display())).status.success());
            assert!(!sh(&sb, &s, &format!("echo x > {}", f.scratch.join(".git").display())).status.success());
        }
        // A name that merely contains ".git" is an ordinary file.
        assert!(sh(&sb, &s, &format!("echo x > {}", f.work.join(".gitignore").display())).status.success());
    }

    #[test]
    fn there_is_no_network_unless_it_is_the_one_proxy_port() {
        let Some(sb) = sandbox() else { return };
        if sb.mode() != Mode::SandboxExec {
            return; // proxy-only networking on Linux is Step 3 harness work
        }
        let f = fx();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let other = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let other_port = other.local_addr().unwrap().port();
        let connect = |s: &SandboxSpec, p: u16| sb.command(s, "/usr/bin/nc", ["-z", "-w", "2", "127.0.0.1", &p.to_string()]).unwrap().output().unwrap().status.success();
        let mut s = spec(&f);
        assert!(!connect(&s, port), "network is off by default");
        s.network = Network::ProxyOnly { port };
        assert!(connect(&s, port), "the proxy port is reachable");
        assert!(!connect(&s, other_port), "any other port is not");
    }

    #[test]
    fn the_environment_is_exactly_the_allowlist() {
        let Some(sb) = sandbox() else { return };
        let f = fx();
        let mut s = spec(&f);
        s.env.insert("KEEP".into(), "yes".into());
        // SAFETY of the test: the parent's variable must not leak into the child.
        unsafe { std::env::set_var("DELPHI_SANDBOX_TEST_SECRET", "leak") };
        let out = run(&sb, &s, "/usr/bin/env", &[]);
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("KEEP=yes") && !text.contains("DELPHI_SANDBOX_TEST_SECRET") && !text.contains("HOME="), "{}", text);
    }

    #[test]
    fn programs_outside_the_granted_paths_cannot_run() {
        let Some(sb) = sandbox() else { return };
        let f = fx();
        let evil = f.outside.join("evil.sh");
        fs::write(&evil, "#!/bin/sh\necho pwned\n").unwrap();
        let mut perms = fs::metadata(&evil).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        fs::set_permissions(&evil, perms).unwrap();
        let out = sh(&sb, &spec(&f), &format!("{}", evil.display()));
        assert!(!String::from_utf8_lossy(&out.stdout).contains("pwned"));
    }

    #[test]
    fn paths_are_validated_and_quoted_safely() {
        assert!(matches!(Resolved::new(&SandboxSpec { worktree: Some(PathBuf::from("relative")), ..SandboxSpec::new() }), Err(SandboxError::BadPath(_))));
        assert!(matches!(Resolved::new(&SandboxSpec { worktree: Some(PathBuf::from("/nonexistent/delphi-x")), ..SandboxSpec::new() }), Err(SandboxError::BadPath(_))));
        assert_eq!(sbpl_str(Path::new("/a\"b)(allow default)\\c")), "\"/a\\\"b)(allow default)\\\\c\"");
        // A directory whose name tries to close the string and add a rule stays one literal.
        let dir = tempfile::tempdir().unwrap();
        let evil = dir.path().join("x\"))(allow default)(\"");
        fs::create_dir(&evil).unwrap();
        let r = Resolved::new(&SandboxSpec { worktree: Some(evil), ..SandboxSpec::new() }).unwrap();
        let profile = sbpl(&r);
        assert!(!profile.contains("\n(allow default)") && profile.contains("\\\"))(allow default)("), "{}", profile);
    }

    #[test]
    fn the_profile_denies_git_last_and_bwrap_rebinds_existing_git_read_only() {
        let f = fx();
        let r = Resolved::new(&spec(&f)).unwrap();
        let p = sbpl(&r);
        assert!(p.starts_with("(version 1)\n(deny default)"));
        assert!(p.trim_end().ends_with("(deny file-write* (regex #\"(^|/)\\.git(/|$)\"))"));
        let args = bwrap_args(&r).unwrap();
        let git = f.work.join("sub/.git").to_string_lossy().into_owned();
        assert!(args.windows(3).any(|w| w[0] == "--ro-bind" && w[1] == git && w[2] == git), "{:?}", args);
        assert!(args.contains(&"--unshare-all".to_string()));
        let proxied = Resolved::new(&SandboxSpec { network: Network::ProxyOnly { port: 9 }, ..spec(&f) }).unwrap();
        assert!(matches!(bwrap_args(&proxied), Err(SandboxError::Unsupported(_))));
    }
}
