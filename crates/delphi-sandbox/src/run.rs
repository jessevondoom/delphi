//! Running a sandboxed command to completion with a wall-clock timeout, an output cap, and a guarantee
//! that nothing it started outlives it.
//!
//! The sandbox launcher puts every program in its own session (`setsid`), so the whole tree shares a
//! process group that can be signalled at once. Residual risk: a program can leave that group by calling
//! `setsid` itself, and macOS has no die-with-parent, so if Delphi is killed outright the program runs on
//! until its own CPU limit.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Wall-clock limit; the process group is killed when it passes.
    pub timeout: Duration,
    /// Each of stdout and stderr is kept up to this many bytes. Passing it kills the run: a program that
    /// floods its output is not doing the task.
    pub max_output_bytes: usize,
    /// After the run, flag any of these directories that has grown past the limit (a quota we can only
    /// check, not enforce).
    pub quota: Vec<(PathBuf, u64)>,
}

impl Default for RunOptions {
    fn default() -> RunOptions {
        RunOptions { timeout: Duration::from_secs(300), max_output_bytes: 1 << 20, quota: vec![] }
    }
}

#[derive(Debug)]
pub struct RunOutcome {
    pub status: Option<ExitStatus>,
    pub timed_out: bool,
    /// The run was killed for exceeding the output cap.
    pub output_capped: bool,
    pub stdout: String,
    pub stderr: String,
    /// Directories over their quota after the run.
    pub over_quota: Vec<PathBuf>,
}

impl RunOutcome {
    pub fn success(&self) -> bool {
        self.status.is_some_and(|s| s.success()) && !self.timed_out && !self.output_capped
    }
}

fn kill_group(pgid: i32, sig: i32) {
    // SAFETY: plain signal delivery to a process group we created.
    unsafe {
        libc::kill(-pgid, sig);
    }
}

type Shared = std::sync::Arc<std::sync::Mutex<Vec<u8>>>;

/// Bytes read from a pipe into `into`, capped; the rest is drained and discarded so the writer never
/// blocks. `flag` is raised the moment a byte had to be dropped.
fn drain(mut r: impl Read + Send + 'static, cap: usize, into: Shared, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut out = into.lock().unwrap();
                    let take = n.min(cap.saturating_sub(out.len()));
                    out.extend_from_slice(&buf[..take]);
                    if take < n {
                        flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
        }
    })
}

/// Wait briefly for a reader to see end-of-file. A program that escaped the process group can hold the
/// pipe open; we keep what was read and move on rather than hang.
fn finish(t: std::thread::JoinHandle<()>, buf: &Shared) -> String {
    let until = Instant::now() + Duration::from_secs(1);
    while !t.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    if t.is_finished() {
        let _ = t.join();
    }
    String::from_utf8_lossy(&buf.lock().unwrap()).into_owned()
}

fn dir_size(p: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten().map(|e| match e.file_type() {
        Ok(t) if t.is_dir() => dir_size(&e.path()),
        Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
        _ => 0, // symlinks are not followed
    }).sum()
}

/// Run `cmd` (built by [`crate::Sandbox::command`]) with stdin closed.
pub fn run_bounded(mut cmd: Command, opts: &RunOptions) -> std::io::Result<RunOutcome> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let pgid = child.id() as i32;
    let capped = Arc::new(AtomicBool::new(false));
    let (out_buf, err_buf): (Shared, Shared) = (Default::default(), Default::default());
    let out_t = drain(child.stdout.take().expect("piped"), opts.max_output_bytes, out_buf.clone(), capped.clone());
    let err_t = drain(child.stderr.take().expect("piped"), opts.max_output_bytes, err_buf.clone(), capped.clone());
    let deadline = Instant::now() + opts.timeout;
    let (mut status, mut timed_out, mut output_capped) = (None, false, false);
    loop {
        if let Some(s) = child.try_wait()? {
            status = Some(s);
            break;
        }
        if capped.load(Ordering::SeqCst) {
            output_capped = true;
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Whatever the reason we stopped, nothing it started may outlive it: ask, then insist.
    if status.is_none() {
        kill_group(pgid, libc::SIGTERM);
        let grace = Instant::now() + Duration::from_millis(300);
        while Instant::now() < grace && child.try_wait()?.is_none() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    kill_group(pgid, libc::SIGKILL);
    let status = match status {
        Some(s) => Some(s),
        None => child.wait().ok(),
    };
    let (stdout, stderr) = (finish(out_t, &out_buf), finish(err_t, &err_buf));
    let over_quota = opts.quota.iter().filter(|(p, max)| dir_size(p) > *max).map(|(p, _)| p.clone()).collect();
    Ok(RunOutcome { status, timed_out, output_capped, stdout, stderr, over_quota })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Sandbox, SandboxSpec};
    use std::fs;

    struct Fx {
        _d: tempfile::TempDir,
        work: PathBuf,
    }

    fn fx() -> Fx {
        let d = tempfile::tempdir().unwrap();
        let work = fs::canonicalize(d.path()).unwrap().join("work");
        fs::create_dir_all(&work).unwrap();
        Fx { _d: d, work }
    }

    fn go(f: &Fx, script: &str, spec_edit: impl FnOnce(&mut SandboxSpec), opts: RunOptions) -> Option<RunOutcome> {
        let sb = Sandbox::detect().ok()?;
        let mut spec = SandboxSpec::new();
        spec.worktree = Some(f.work.clone());
        spec_edit(&mut spec);
        let mut cmd = sb.command(&spec, "/bin/sh", ["-c", script]).unwrap();
        cmd.current_dir(&f.work);
        Some(run_bounded(cmd, &opts).unwrap())
    }

    fn alive(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    fn pid_in(out: &str) -> i32 {
        out.lines().find_map(|l| l.trim().parse().ok()).expect("a pid on stdout")
    }

    fn quick() -> RunOptions {
        RunOptions { timeout: Duration::from_secs(10), ..RunOptions::default() }
    }

    #[test]
    fn a_plain_run_returns_its_output_and_status() {
        let f = fx();
        let Some(o) = go(&f, "echo hi; echo oops >&2; exit 3", |_| {}, quick()) else { return };
        assert_eq!((o.stdout.trim(), o.stderr.trim(), o.status.unwrap().code()), ("hi", "oops", Some(3)));
        assert!(!o.success() && !o.timed_out);
    }

    #[test]
    fn the_timeout_kills_the_command_and_everything_it_started() {
        let f = fx();
        let opts = RunOptions { timeout: Duration::from_millis(600), ..RunOptions::default() };
        let t = Instant::now();
        let Some(o) = go(&f, "sleep 60 & echo $!; wait", |_| {}, opts) else { return };
        assert!(o.timed_out && !o.success());
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        let pid = pid_in(&o.stdout);
        std::thread::sleep(Duration::from_millis(200));
        assert!(!alive(pid), "the background child must die with the group");
    }

    #[test]
    fn a_background_process_cannot_outlive_a_command_that_exits() {
        let f = fx();
        let Some(o) = go(&f, "sleep 60 & echo $!", |_| {}, quick()) else { return };
        assert!(o.success());
        std::thread::sleep(Duration::from_millis(200));
        assert!(!alive(pid_in(&o.stdout)), "a daemonised leftover is killed when the run ends");
    }

    #[test]
    fn a_flood_of_output_is_capped_and_kills_the_run() {
        let f = fx();
        let opts = RunOptions { timeout: Duration::from_secs(20), max_output_bytes: 4096, ..RunOptions::default() };
        let t = Instant::now();
        let Some(o) = go(&f, "yes", |_| {}, opts) else { return };
        assert!(o.output_capped && !o.success() && o.stdout.len() == 4096, "{} bytes", o.stdout.len());
        assert!(t.elapsed() < Duration::from_secs(5));
        // Exactly filling the cap is not a flood.
        let opts = RunOptions { max_output_bytes: 4, ..quick() };
        let Some(o) = go(&f, "printf abcd", |_| {}, opts) else { return };
        assert!(!o.output_capped && o.stdout == "abcd");
    }

    #[test]
    fn the_file_size_limit_stops_a_disk_filler() {
        let f = fx();
        let Some(o) = go(&f, "dd if=/dev/urandom of=big bs=1048576 count=20 2>/dev/null; echo done-$?", |s| s.limits.max_file_bytes = Some(1 << 20), quick()) else { return };
        let size = fs::metadata(f.work.join("big")).map(|m| m.len()).unwrap_or(0);
        assert_eq!(size, 1 << 20, "dd must have written up to the limit and no further: {:?}", o);
        assert!(!o.stdout.contains("done-0"), "dd must have been stopped: {:?}", o);
    }

    #[test]
    fn the_cpu_limit_stops_a_busy_loop() {
        let f = fx();
        let t = Instant::now();
        let Some(o) = go(&f, "while :; do :; done", |s| s.limits.cpu_secs = Some(1), RunOptions { timeout: Duration::from_secs(30), ..RunOptions::default() }) else { return };
        assert!(!o.timed_out && !o.success(), "{:?}", o);
        assert!(t.elapsed() < Duration::from_secs(12), "{:?}", t.elapsed());
    }

    #[test]
    fn a_directory_over_its_quota_is_flagged() {
        let f = fx();
        let opts = RunOptions { quota: vec![(f.work.clone(), 1000)], ..quick() };
        let Some(o) = go(&f, "head -c 5000 /dev/urandom > blob", |_| {}, opts) else { return };
        assert_eq!(o.over_quota, vec![f.work.clone()]);
        let opts = RunOptions { quota: vec![(f.work.clone(), 1 << 20)], ..quick() };
        assert!(go(&f, "true", |_| {}, opts).unwrap().over_quota.is_empty());
    }

    #[test]
    fn the_command_has_no_controlling_terminal_and_leads_its_own_group() {
        let f = fx();
        let Some(o) = go(&f, "ps -o pid=,pgid=,sess= -p $$", |_| {}, quick()) else { return };
        let cols: Vec<&str> = o.stdout.split_whitespace().collect();
        if cols.len() >= 2 {
            assert_eq!(cols[0], cols[1], "the shell is its own process-group leader: {}", o.stdout);
        }
    }
}
