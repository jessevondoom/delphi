//! Race-free daemon auto-spawn (`specs/005` §2.1 item 1).
//!
//! `connect_or_spawn`: connect; on failure take an exclusive `flock` on the workspace lock (which
//! serializes concurrent launches; polled, because a live daemon holds it), retry the connect, and only then — holding the lock, with the
//! connect failing, which proves no live daemon exists — unlink any stale socket and spawn the
//! daemon, **passing the locked file descriptor across `exec`** so the daemon holds the lock for its
//! lifetime and never re-locks by path. PID files are not used.

use crate::client::{Client, ClientError};
use crate::proto::Scope;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a launcher waits for a peer launcher or a starting daemon, beyond the readiness timeout.
pub const LAUNCH_WAIT: Duration = Duration::from_secs(5);

pub struct Layout {
    pub run_dir: PathBuf,
    pub sock: PathBuf,
    pub lock: PathBuf,
    pub log: PathBuf,
}

/// 12 hex characters of the blake3 hash of the *canonical* workspace path (short enough for the
/// platform's Unix-socket path limit).
pub fn workspace_hash(workspace: &Path) -> io::Result<String> {
    let canon = fs::canonicalize(workspace)?;
    Ok(blake3::hash(canon.as_os_str().as_encoded_bytes()).to_hex()[..12].to_string())
}

pub fn layout(home: &Path, workspace: &Path) -> io::Result<Layout> {
    let h = workspace_hash(workspace)?;
    let run_dir = home.join("run");
    Ok(Layout { sock: run_dir.join(format!("{}.sock", h)), lock: run_dir.join(format!("{}.lock", h)), log: run_dir.join(format!("{}.log", h)), run_dir })
}

/// Create `run/` with mode 0700 (and tighten it if it already exists).
pub fn ensure_run_dir(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

pub struct Attached {
    pub client: Client,
    pub scope: Scope,
    /// Set when this call started the daemon.
    pub spawned: bool,
    pub daemon_pid: u64,
    /// The capability token, for opening further connections as the same console.
    pub token: String,
    pub sock: PathBuf,
}

pub struct SpawnOptions<'a> {
    pub home: &'a Path,
    pub workspace: &'a Path,
    /// Path of the `delphi-core` binary to exec.
    pub daemon_exe: &'a Path,
    /// Extra arguments for the daemon (tests only).
    pub extra_args: Vec<String>,
}

fn flock(file: &File, op: libc::c_int) -> io::Result<()> {
    if unsafe { libc::flock(file.as_raw_fd(), op) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn clear_cloexec(fd: RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
    }
}

/// Read from the readiness pipe until `\n`, or fail at the deadline.
fn read_ready(pipe: &mut File, deadline: Instant) -> io::Result<String> {
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "daemon did not become ready"));
        }
        let mut pfd = libc::pollfd { fd: pipe.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let n = unsafe { libc::poll(&mut pfd, 1, left.as_millis().min(i32::MAX as u128) as i32) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if n == 0 {
            continue;
        }
        match pipe.read(&mut byte)? {
            0 => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "daemon exited before it was ready")),
            _ if byte[0] == b'\n' => return Ok(String::from_utf8_lossy(&out).into_owned()),
            _ => out.push(byte[0]),
        }
    }
}

fn tail(path: &Path) -> String {
    let t = fs::read_to_string(path).unwrap_or_default();
    t.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
}

/// Attach as the console, starting the daemon if none is running.
pub fn connect_or_spawn(opts: &SpawnOptions) -> Result<Attached, ClientError> {
    let l = layout(opts.home, opts.workspace)?;
    ensure_run_dir(&l.run_dir)?;

    if let Some(a) = try_attach(&l)? {
        return Ok(a);
    }
    // No daemon answered. Serialize with every other launcher. A live daemon holds this lock for its
    // whole lifetime, so a blocking `flock` would wait forever: poll instead, attaching whenever a
    // daemon (possibly one another launcher just started) answers, and spawning only once we own the
    // lock, which proves that no live daemon and no other launcher exists.
    let lock = OpenOptions::new().create(true).read(true).write(true).truncate(false).open(&l.lock)?;
    let deadline = Instant::now() + READY_TIMEOUT + LAUNCH_WAIT;
    loop {
        match flock(&lock, libc::LOCK_EX | libc::LOCK_NB) {
            Ok(()) => break,
            Err(e) if e.raw_os_error() == Some(libc::EWOULDBLOCK) => {
                if let Some(a) = try_attach(&l)? {
                    return Ok(a);
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "another launcher holds the workspace lock but no daemon answered").into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.into()),
        }
    }
    // A daemon may have started between our first attempt and taking the lock.
    if let Some(a) = try_attach(&l)? {
        return Ok(a);
    }
    // We hold the lock and the connect failed: no live daemon exists. Clear any stale socket.
    let _ = fs::remove_file(&l.sock);

    let mut fds = [0 as RawFd; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let (mut ready_r, ready_w) = unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) };
    let (lock_fd, ready_fd) = (lock.as_raw_fd(), ready_w.as_raw_fd());

    let log = OpenOptions::new().create(true).append(true).open(&l.log)?;
    fs::set_permissions(&l.log, fs::Permissions::from_mode(0o600))?;
    let mut cmd = Command::new(opts.daemon_exe);
    cmd.arg("--daemon")
        .arg("--workspace")
        .arg(opts.workspace)
        .arg("--home")
        .arg(opts.home)
        .arg("--ready-fd")
        .arg(ready_fd.to_string())
        .arg("--lock-fd")
        .arg(lock_fd.to_string())
        .args(&opts.extra_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    unsafe {
        cmd.pre_exec(move || {
            // Pass the locked descriptor and the readiness pipe across exec; detach from our session.
            clear_cloexec(lock_fd);
            clear_cloexec(ready_fd);
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    // Close our copies: the daemon's inherited lock fd is now the only holder, and EOF on the pipe
    // means the daemon died.
    drop(ready_w);
    drop(lock);

    let reply = match read_ready(&mut ready_r, Instant::now() + READY_TIMEOUT) {
        Ok(r) => r,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(e.kind(), format!("{} — daemon log:\n{}", e, tail(&l.log))).into());
        }
    };
    // The daemon is now an independent process; don't leave a zombie handle behind.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    let token = reply.trim().to_string();
    let mut client = Client::connect(&l.sock)?;
    let hello = client.hello(&token)?;
    Ok(Attached { client, scope: Scope::Console, spawned: true, daemon_pid: hello["pid"].as_u64().unwrap_or(0), token, sock: l.sock.clone() })
}

/// Connect to a running daemon and obtain a console token through the TTY challenge.
fn try_attach(l: &Layout) -> Result<Option<Attached>, ClientError> {
    let mut client = match Client::connect(&l.sock) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let token = client.issue_token(Scope::Console)?;
    let hello = client.hello(&token)?;
    Ok(Some(Attached { client, scope: Scope::Console, spawned: false, daemon_pid: hello["pid"].as_u64().unwrap_or(0), token, sock: l.sock.clone() }))
}
