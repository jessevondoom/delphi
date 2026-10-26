//! End to end: the real `delphi` console and `delphi-core` daemon against a mock Claude server.
//!
//! Covers the Step 2 flow as an operator would live it: initialize a workspace, ratify its endpoints from
//! the console, run a tool-using task, answer an approval card, read usage, and check that an unratified
//! edit to the endpoint config never redirects traffic.

mod common;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");
const KEY: &str = "sk-e2e-key-0123456789abcdef";

/// The daemon is `delphi __core`: there is no second executable to build or find. (This used to build and check
/// for a sibling `delphi-core`. Verified once with the sibling deleted; it is not deleted here because the engine's
/// own daemon tests use that binary and run at the same time.)
fn ensure_daemon_binary() {}

// ── a mock Claude ───────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Seen {
    path: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

fn sse(events: &[(&str, Value)]) -> String {
    events.iter().map(|(n, d)| format!("event: {}\ndata: {}\n\n", n, d)).collect()
}

fn reply_text(text: &str) -> String {
    sse(&[
        ("message_start", json!({"type": "message_start", "message": {"usage": {"input_tokens": 1000, "output_tokens": 1}}})),
        ("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})),
        ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}})),
        ("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
        ("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 200}})),
        ("message_stop", json!({"type": "message_stop"})),
    ])
}

fn reply_tool(id: &str, name: &str, args: Value) -> String {
    sse(&[
        ("message_start", json!({"type": "message_start", "message": {"usage": {"input_tokens": 1000, "output_tokens": 1}}})),
        ("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}})),
        ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": args.to_string()}})),
        ("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
        ("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 100}})),
        ("message_stop", json!({"type": "message_stop"})),
    ])
}

struct Mock {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn mock(script: Vec<String>) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (seen2, script) = (seen.clone(), Arc::new(Mutex::new(script)));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let (seen, script) = (seen2.clone(), script.clone());
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                if r.read_line(&mut first).unwrap_or(0) == 0 {
                    return;
                }
                let mut headers = BTreeMap::new();
                loop {
                    let mut l = String::new();
                    r.read_line(&mut l).unwrap();
                    if l.trim().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = l.split_once(':') {
                        headers.insert(k.trim().to_lowercase(), v.trim().to_string());
                    }
                }
                let n: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
                let mut body = vec![0u8; n];
                r.read_exact(&mut body).unwrap();
                seen.lock().unwrap().push(Seen { path: first.split_whitespace().nth(1).unwrap_or("").into(), headers, body: serde_json::from_slice(&body).unwrap_or(Value::Null) });
                let reply = {
                    let mut sc = script.lock().unwrap();
                    if sc.len() > 1 { sc.remove(0) } else { sc[0].clone() }
                };
                let _ = write!(s, "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n");
                if reply == "HANG" {
                    // Accept the request and never finish: the daemon is killed while waiting.
                    std::thread::sleep(Duration::from_secs(3600));
                    return;
                }
                for chunk in reply.as_bytes().chunks(300) {
                    if s.write_all(chunk).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            });
        }
    });
    Mock { port, seen }
}

// ── a driven console session ────────────────────────────────────────────────

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    out: String,
}

impl Session {
    fn start(home: &Path, args: &[&str]) -> Session {
        let mut child = Command::new(DELPHI).args(args).arg("--home").arg(home).env("DELPHI_E2E_KEY", KEY).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let (tx, rx) = channel();
        let err = child.stderr.take().unwrap();
        for mut r in [Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>, Box::new(err) as Box<dyn Read + Send>] {
            let tx = tx.clone();
            std::thread::spawn(move || {
                // Read raw bytes: live text has no trailing newline.
                let mut buf = [0u8; 4096];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 || tx.send(String::from_utf8_lossy(&buf[..n]).into_owned()).is_err() {
                        break;
                    }
                }
            });
        }
        let stdin = child.stdin.take();
        Session { child, stdin, lines: rx, out: String::new() }
    }

    fn send(&mut self, line: &str) {
        let s = self.stdin.as_mut().unwrap();
        writeln!(s, "{}", line).unwrap();
        s.flush().unwrap();
    }

    /// Everything printed so far must eventually contain `needle`.
    fn wait_for(&mut self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(40);
        while !self.out.contains(needle) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left.max(Duration::from_millis(1))) {
                Ok(chunk) => self.out.push_str(&chunk),
                Err(_) => panic!("timed out waiting for `{}`; output so far:\n{}", needle, self.out),
            }
        }
    }

    /// Close stdin, let the console finish, return (exit code, full output).
    fn finish(mut self) -> (i32, String) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            while let Ok(c) = self.lines.try_recv() {
                self.out.push_str(&c);
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                std::thread::sleep(Duration::from_millis(100));
                while let Ok(c) = self.lines.try_recv() {
                    self.out.push_str(&c);
                }
                return (status.code().unwrap_or(-1), self.out);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        panic!("the console did not exit; output:\n{}", self.out);
    }
}

// ── the workspace ───────────────────────────────────────────────────────────

struct Env {
    /// First, so it drops first: it stops the daemons while the directory is still there.
    _reap: common::DaemonReaper,
    _t: tempfile::TempDir,
    home: PathBuf,
    ws: PathBuf,
}

fn run(args: &[&str]) {
    assert!(Command::new(DELPHI).args(args).stdout(Stdio::null()).status().unwrap().success(), "{:?}", args);
}

fn config_for(project: &Path, port: u16) -> String {
    format!(
        r#"
[workspace]
name = "e2e"
projects = ["{proj}"]
[theme]
name = "greek"

[models.claude]
fast = "haiku-t"
workhorse = "sonnet-t"
frontier = "opus-t"
[models.catalog."haiku-t"]
max_output_tokens = 8000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 5.0
[models.catalog."sonnet-t"]
max_output_tokens = 64000
min_thinking_budget = 1024
max_thinking_budget = 32000
input_usd_per_mtok = 3.0
output_usd_per_mtok = 15.0
[models.catalog."opus-t"]
max_output_tokens = 64000
min_thinking_budget = 1024
max_thinking_budget = 32000
input_usd_per_mtok = 15.0
output_usd_per_mtok = 75.0

[[endpoints]]
id = "mock"
family = "claude"
path = "api"
base_url = "http://127.0.0.1:{port}"
credential = "env:DELPHI_E2E_KEY"

[access]
order = {{ claude = ["mock"] }}
allowed_endpoints = ["mock"]

[budget]
mission_budget_usd = 5.0
[hitl]
approval_timeout_sec = 60
"#,
        proj = project.display(),
        port = port
    )
}

fn setup(port: u16) -> Env {
    ensure_daemon_binary();
    let t = tempfile::Builder::new().prefix("dl").tempdir_in("/tmp").unwrap();
    let home = t.path().join("h");
    let proj = t.path().join("proj");
    std::fs::create_dir_all(proj.join("src")).unwrap();
    std::fs::write(proj.join("src/lib.rs"), "pub fn needle() -> u32 { 42 }\n").unwrap();
    for args in [vec!["init", "-q"], vec!["add", "."]] {
        assert!(Command::new("git").arg("-C").arg(&proj).args(&args).stdout(Stdio::null()).status().unwrap().success());
    }
    assert!(Command::new("git").arg("-C").arg(&proj).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-qm", "base"]).stdout(Stdio::null()).status().unwrap().success());
    run(&["init", "--non-interactive", "--home", home.to_str().unwrap(), "--workspace", "e2e", "--project", proj.to_str().unwrap()]);
    let ws = home.join("workspaces/e2e");
    std::fs::write(ws.join("config.toml"), config_for(&proj, port)).unwrap();
    Env { _reap: common::DaemonReaper::new(&home), _t: t, home, ws }
}

/// The daemon's pid, from the console banner (`daemon pid 1234`).
fn pid_in(out: &str) -> i32 {
    let rest = out.split("daemon pid ").nth(1).unwrap_or_else(|| panic!("no daemon pid in:\n{}", out));
    rest.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap()
}

/// `delphi daemon stop` needs an interactive terminal for its token, so tests stop daemons by pid.
fn kill_daemon(pid: i32) {
    unsafe { libc::kill(pid, libc::SIGKILL) };
    std::thread::sleep(Duration::from_millis(100));
}

#[test]
fn an_operator_ratifies_endpoints_runs_a_task_and_reads_the_cost() {
    let m = mock(vec![reply_tool("toolu_1", "read_file", json!({"path": "@project/proj/src/lib.rs"})), reply_text("The file defines `needle`, which returns 42.")]);
    let e = setup(m.port);

    let mut s = Session::start(&e.home, &["chat", "--workspace", "e2e"]);
    s.wait_for("/help for commands");
    s.wait_for("unratified endpoint/access changes");
    s.send("/ratify");
    s.wait_for("1 pending proposal(s)");
    s.wait_for("[config-access] [RED]");
    s.send("/ratify show 1");
    s.wait_for("127.0.0.1");
    s.send("/ratify apply 1");
    s.wait_for("ratified ws:config.toml#access");
    s.send("What does @project/proj/src/lib.rs define?");
    s.wait_for("✓ t_0001 done");
    s.send("/usage");
    s.wait_for("total $");
    s.send("/budget");
    s.wait_for("ceiling $5.00");
    // A second window attaches read-only and draws what just happened, without a console token.
    // (`--once` is drawn by the full-screen window's own renderer, so it needs the UI program; skipped without it.)
    if let Some(launcher) = ui_launcher(e._t.path()) {
        let (ncode, screen, nerr) = narrator_once(&e.home, "e2e", "agents", Some(&launcher));
        assert_eq!(ncode, 0, "{}\n{}", screen, nerr);
        for want in ["delphi v", "· agents", "Athena", "opus-t", "nothing needs you", "running  ·"] {
            assert!(screen.contains(want), "missing `{}` in\n{}", want, screen);
        }
        assert!(!screen.contains('$') && !screen.contains("mission 2"), "the narrator shows no money and no mission id:\n{}", screen);
    }
    let (code, out) = s.finish();
    assert_eq!(code, 0, "{}", out);
    let pid = pid_in(&out);
    assert!(out.contains("→ read_file(@project/proj/src/lib.rs)") && out.contains("needle"), "{}", out);
    assert!(out.contains("Athena:") || out.contains("orchestrator:"), "the active theme names the role: {}", out);

    // The mock saw two calls with the operator's key, the frontier model, and the file read as the tool result.
    let seen = m.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "{:?}", seen.iter().map(|s| &s.path).collect::<Vec<_>>());
    assert_eq!((seen[0].path.as_str(), seen[0].headers["x-api-key"].as_str()), ("/v1/messages", KEY));
    assert_eq!(seen[0].body["model"], "opus-t");
    assert!(seen[1].body["messages"].to_string().contains("pub fn needle()"));

    // On disk: a fsync'd event log, the cost in the ledger — and the API key nowhere.
    let mission_dir = std::fs::read_dir(e.ws.join("local/missions")).unwrap().next().unwrap().unwrap().path();
    let log = std::fs::read_to_string(mission_dir.join("events/000001.jsonl")).unwrap();
    assert!(log.contains("\"type\":\"agent.usage\"") && log.contains("\"type\":\"task.completed\""));
    assert!(!log.contains(KEY), "the credential never reaches the event log");
    for entry in walkdir_files(&e.ws) {
        assert!(!std::fs::read(&entry).unwrap().windows(KEY.len()).any(|w| w == KEY.as_bytes()), "{} contains the key", entry.display());
    }
    kill_daemon(pid);
}

/// Run `delphi narrator <view> --once` with a controlling terminal (the daemon only hands a Narrator token to
/// a peer that has one) and return the screen it drew.
fn narrator_once(home: &Path, workspace: &str, view: &str, ui: Option<&Path>) -> (i32, String, String) {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    let (mut master, mut slave) = (0, 0);
    unsafe {
        assert_eq!(libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()), 0);
    }
    let mut cmd = Command::new(DELPHI);
    cmd.args(["narrator", view, "--once", "--workspace", workspace, "--home"]).arg(home);
    if let Some(ui) = ui {
        cmd.env("DELPHI_UI", ui);
    }
    cmd.stdin(unsafe { Stdio::from(OwnedFd::from_raw_fd(libc::dup(slave))) }).stdout(Stdio::piped()).stderr(Stdio::piped());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }
    let out = cmd.output().unwrap();
    unsafe {
        libc::close(slave);
        libc::close(master);
    }
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

fn walkdir_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let ft = e.file_type().unwrap();
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn a_draft_spec_waits_for_the_operator_and_the_focus_guard_and_binding_hold() {
    let m = mock(vec![reply_tool("w1", "write_file", json!({"path": "specs/009-e2e.md", "content": "# E2E spec\nbody"})), reply_text("Drafted specs/009-e2e.md.")]);
    let e = setup(m.port);
    // Ratify the endpoints first (as in the previous test) via a short session.
    let mut s = Session::start(&e.home, &["chat", "--workspace", "e2e"]);
    s.wait_for("/help for commands");
    s.send("/ratify");
    s.wait_for("pending proposal");
    s.send("/ratify apply 1");
    s.wait_for("ratified ws:config.toml#access");

    s.send("@architect Draft a spec called 009-e2e");
    s.wait_for("approval needed [amber]");
    s.wait_for("write specs/009-e2e.md");
    // Answering at once is ignored by the focus guard; the file does not exist.
    s.send("/y");
    s.wait_for("ignored: the card appeared");
    assert!(!e.ws.join("team/specs/009-e2e.md").exists());
    std::thread::sleep(Duration::from_millis(400));
    s.send("/d");
    s.wait_for("\"preview\"");
    s.send("/y");
    s.wait_for(": approved");
    s.wait_for("✓ t_0001 done");
    let (code, out) = s.finish();
    assert_eq!(code, 0, "{}", out);
    let spec = std::fs::read_to_string(e.ws.join("team/specs/009-e2e.md")).unwrap();
    assert!(spec.contains("author: architect") && spec.contains("source_type: internal_generated") && spec.contains("# E2E spec"), "{}", spec);
    // The model never saw a reason to think it approved itself: the second request carries the tool result.
    assert_eq!(m.seen.lock().unwrap().len(), 2);
    kill_daemon(pid_in(&out));
}

#[test]
fn an_unratified_edit_to_the_endpoints_never_redirects_traffic() {
    let honest = mock(vec![reply_text("answer from the ratified endpoint")]);
    let attacker = mock(vec![reply_text("answer from the attacker")]);
    let e = setup(honest.port);
    let mut s = Session::start(&e.home, &["chat", "--workspace", "e2e"]);
    s.wait_for("/help for commands");
    s.send("/ratify");
    s.wait_for("pending proposal");
    s.send("/ratify apply 1");
    s.wait_for("ratified ws:config.toml#access");
    let (code, out) = s.finish();
    assert_eq!(code, 0);
    kill_daemon(pid_in(&out));

    // A local process edits the config to point at its own server.
    let live = std::fs::read_to_string(e.ws.join("config.toml")).unwrap();
    std::fs::write(e.ws.join("config.toml"), live.replace(&format!("127.0.0.1:{}", honest.port), &format!("127.0.0.1:{}", attacker.port))).unwrap();

    let mut s = Session::start(&e.home, &["chat", "--workspace", "e2e"]);
    s.wait_for("unratified endpoint/access changes");
    s.send("hello");
    s.wait_for("✓ t_0001 done");
    let (code, out) = s.finish();
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("answer from the ratified endpoint"), "{}", out);
    assert_eq!(attacker.seen.lock().unwrap().len(), 0, "the attacker's server was never contacted");
    assert_eq!(honest.seen.lock().unwrap().len(), 1);
    kill_daemon(pid_in(&out));
}

#[test]
fn killing_the_daemon_mid_request_loses_nothing_and_the_operator_resumes_from_the_summary() {
    // The provider accepts the request and never answers; then the daemon is killed with SIGKILL.
    let m = mock(vec!["HANG".to_string(), reply_text("Resumed: the file defines `needle`.")]);
    let e = setup(m.port);
    let mut s = Session::start(&e.home, &["chat", "--workspace", "e2e"]);
    s.wait_for("/help for commands");
    s.send("/ratify");
    s.wait_for("pending proposal");
    s.send("/ratify apply 1");
    s.wait_for("ratified ws:config.toml#access");
    s.send("Explain @project/proj/src/lib.rs");
    s.wait_for("(t_0001 started)");
    let start = Instant::now();
    while m.seen.lock().unwrap().is_empty() {
        assert!(start.elapsed() < Duration::from_secs(20), "the daemon never called the provider");
        std::thread::sleep(Duration::from_millis(20));
    }
    let pid = pid_in(&s.out);
    kill_daemon(pid); // SIGKILL mid-request
    let (_, _) = {
        let out = s.out.clone();
        drop(s);
        (0, out)
    };

    // A new console finds the work, says what it will do, and waits to be told.
    let mut s = Session::start(&e.home, &["chat", "--workspace", "e2e"]);
    s.wait_for("Recovered after a restart; nothing has resumed yet");
    s.wait_for("t_0001 (orchestrator)");
    s.wait_for("was charged for calls the restart interrupted");
    s.send("/usage");
    s.wait_for("total $");
    assert_eq!(m.seen.lock().unwrap().len(), 1, "nothing resumed before the operator asked");
    s.send("/resume");
    s.wait_for("resuming 1 task(s)");
    s.wait_for("✓ t_0001 done");
    let (code, out) = s.finish();
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("Resumed: the file defines"), "{}", out);
    let seen = m.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].body["messages"], seen[1].body["messages"], "the retry sends exactly the request that was in flight");
    assert_eq!(seen[0].body["system"], seen[1].body["system"], "and the cached prefix is byte-identical");

    // The books: the interrupted call was charged in full, and nothing is left reserved.
    let mission_dir = std::fs::read_dir(e.ws.join("local/missions")).unwrap().next().unwrap().unwrap().path();
    let log = std::fs::read_to_string(mission_dir.join("events/000001.jsonl")).unwrap();
    assert!(log.contains("\"outcome\":\"cancelled\"") && log.contains("\"type\":\"mission.recovered\""), "{}", log.lines().count());
    let status = std::fs::read_to_string(mission_dir.join("status.md")).unwrap();
    assert!(status.contains("t_0001") && status.contains("done") && status.contains("## Last recovery") && !status.contains("\nnone\n"), "{}", status);
    kill_daemon(pid_in(&out));
}

// ── the inline console, driven through a pseudo-terminal and read back through a terminal emulator ──────

fn writer_dup(fd: i32) -> std::fs::File {
    use std::os::fd::FromRawFd;
    unsafe { std::fs::File::from_raw_fd(libc::dup(fd)) }
}

struct PtyConsole {
    child: Child,
    writer: std::fs::File,
    bytes: Arc<Mutex<Vec<u8>>>,
    rows: u16,
    cols: u16,
    /// Window size changes as (byte offset in `bytes`, rows, cols), so the emulator replays them where they happened.
    resizes: Vec<(usize, u16, u16)>,
    start_size: (u16, u16),
}

impl Drop for PtyConsole {
    /// However the test ends, the console it started does not outlive it.
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl PtyConsole {
    fn start(home: &Path, args: &[&str], rows: u16, cols: u16) -> PtyConsole {
        PtyConsole::start_env(home, args, rows, cols, &[])
    }

    fn start_env(home: &Path, args: &[&str], rows: u16, cols: u16, envs: &[(&str, &str)]) -> PtyConsole {
        PtyConsole::start_in(None, home, args, rows, cols, envs)
    }

    /// `start_env`, run from `cwd` (bare `delphi` picks its workspace from the directory it is in).
    fn start_in(cwd: Option<&Path>, home: &Path, args: &[&str], rows: u16, cols: u16, envs: &[(&str, &str)]) -> PtyConsole {
        use std::os::fd::{FromRawFd, OwnedFd};
        use std::os::unix::process::CommandExt;
        let (mut master, mut slave) = (0, 0);
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        unsafe {
            assert_eq!(libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &ws as *const _ as *mut _), 0);
        }
        // The master must not leak into the console we spawn: a copy held there keeps the pty open for good, so the
        // console never gets its hangup when this test ends or fails, and the machine runs out of ptys.
        unsafe {
            for fd in [master, slave] {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }
        let dup = |fd| unsafe { Stdio::from(OwnedFd::from_raw_fd(libc::dup(fd))) };
        let mut cmd = Command::new(DELPHI);
        if let Some(d) = cwd {
            cmd.current_dir(d);
        }
        // A console on a pty counts as interactive: without DELPHI_NO_WINDOWS a test that reached `/observe` would open a real Terminal window.
        cmd.args(args).arg("--home").arg(home).env("DELPHI_E2E_KEY", KEY).env("TERM", "xterm-256color").env("DELPHI_NO_WINDOWS", "1").env_remove("TMUX").envs(envs.iter().copied()).stdin(dup(slave)).stdout(dup(slave)).stderr(dup(slave));
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                Ok(())
            });
        }
        let child = cmd.spawn().unwrap();
        unsafe { libc::close(slave) };
        let reader = unsafe { std::fs::File::from_raw_fd(libc::dup(master)) };
        let writer = unsafe { std::fs::File::from_raw_fd(master) };
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let b2 = bytes.clone();
        let mut answer = writer_dup(master);
        std::thread::spawn(move || {
            let mut r = reader;
            let mut buf = [0u8; 4096];
            while let Ok(n) = r.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut all = b2.lock().unwrap();
                all.extend_from_slice(&buf[..n]);
                // A real terminal answers the background-colour query (the full-screen console tints its blocks from it).
                if buf[..n].windows(7).any(|w| w == b"\x1b]11;?\x07") {
                    let _ = answer.write_all(b"\x1b]11;rgb:0f0f/1717/2a2a\x07");
                }
                // A real terminal answers "where is the cursor?"; the inline viewport asks at start-up.
                if buf[..n].windows(4).any(|w| w == b"\x1b[6n") {
                    let mut p = vt100::Parser::new(rows, cols, 0);
                    p.process(&all);
                    let (row, col) = p.screen().cursor_position();
                    let _ = answer.write_all(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
                }
            }
        });
        PtyConsole { child, writer, bytes, rows, cols, resizes: Vec::new(), start_size: (rows, cols) }
    }

    /// Change the terminal's size, as dragging a window corner does: the child gets SIGWINCH.
    fn resize(&mut self, rows: u16, cols: u16) {
        use std::os::fd::AsRawFd;
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        assert_eq!(unsafe { libc::ioctl(self.writer.as_raw_fd(), libc::TIOCSWINSZ as _, &ws as *const _) }, 0);
        self.resizes.push((self.bytes.lock().unwrap().len(), rows, cols));
        self.rows = rows;
        self.cols = cols;
    }

    /// The emulator after everything the console wrote, with the size changes applied where they happened.
    fn replay(&self, scrollback: usize) -> vt100::Parser {
        let bytes = self.bytes.lock().unwrap();
        let (r0, c0) = self.resizes.first().map(|_| (self.initial_size().0, self.initial_size().1)).unwrap_or((self.rows, self.cols));
        let mut p = vt100::Parser::new(r0, c0, scrollback);
        let mut at = 0;
        for &(off, r, c) in &self.resizes {
            p.process(&bytes[at..off.min(bytes.len())]);
            at = off.min(bytes.len());
            p.set_size(r, c);
        }
        p.process(&bytes[at..]);
        p
    }

    fn initial_size(&self) -> (u16, u16) {
        // The size at start is the one before the first recorded change; it is kept in the first entry's predecessor.
        self.start_size
    }

    fn type_keys(&mut self, s: &str) {
        self.writer.write_all(s.as_bytes()).unwrap();
        self.writer.flush().unwrap();
    }

    /// What a person sees by scrolling back as far as the emulator allows (one screen of history), then the screen.
    fn everything(&self) -> String {
        let mut p = self.replay(5000);
        let cols = self.cols;
        p.set_scrollback(self.rows as usize);
        let n = p.screen().scrollback();
        let mut lines: Vec<String> = p.screen().rows(0, cols).take(n).map(|l| l.trim_end().to_string()).collect();
        p.set_scrollback(0);
        lines.extend(p.screen().rows(0, cols).map(|l| l.trim_end().to_string()));
        lines.join("\n")
    }

    /// How many lines have scrolled off the top of the screen into the terminal's history (up to one screen).
    fn lines_in_scrollback(&self) -> usize {
        let mut p = self.replay(5000);
        p.set_scrollback(self.rows as usize);
        p.screen().scrollback()
    }

    /// How many times `needle` was written in all, including text the full-screen console's alternate screen has since hidden.
    fn written(&self, needle: &str) -> usize {
        String::from_utf8_lossy(&self.bytes.lock().unwrap()).matches(needle).count()
    }

    fn visible(&self) -> String {
        let p = self.replay(5000);
        p.screen().contents()
    }

    /// From `auto`, one Shift+Tab: optimistic needs the OS sandbox, so it is either `auto · optimistic` (then one more
    /// press wraps to default) or refused and default at once. Either way the note for default must follow.
    fn cycle_back_to_default(&mut self) {
        self.type_keys("\x1b[Z");
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            let seen = self.everything();
            if seen.contains("· approval mode: auto · optimistic") {
                self.wait_for("tokens  ·  auto · optimistic");
                self.type_keys("\x1b[Z");
                break;
            }
            if seen.contains("· approval mode: default") {
                return;
            }
            assert!(Instant::now() < deadline, "no mode note after Shift+Tab from auto:\n{}", seen);
            std::thread::sleep(Duration::from_millis(60));
        }
        self.wait_for("· approval mode: default");
    }

    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(40);
        while !self.everything().contains(needle) {
            assert!(Instant::now() < deadline, "timed out waiting for `{}`; the terminal shows:\n{}\nraw output (escaped, last 1500 bytes):\n{}", needle, self.everything(), {
                let raw = self.bytes.lock().unwrap();
                let tail = &raw[raw.len().saturating_sub(1500)..];
                String::from_utf8_lossy(tail).escape_debug().to_string()
            });
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    /// The text is on the screen as drawn now. A frame arrives in pieces, so one snapshot taken the instant a string was
    /// seen in the byte stream can be a half-drawn screen (the note already there, the prompt row not yet): look until it settles.
    fn expect_visible(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.visible().contains(needle) {
            assert!(Instant::now() < deadline, "`{}` is not on the screen:\n{}", needle, self.visible());
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    /// Wait until no reply is streaming. The reply's own words and its "Athena · " label also appear in the streaming copy, so
    /// seeing them does not mean the task is over; a daemon stopped now takes the unwritten reply with it (an agent's words
    /// stay in daemon memory until its task ends) and the reply is missing when the console reloads the log.
    fn wait_until_not_writing(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.visible().contains("· writing") {
            assert!(Instant::now() < deadline, "the reply never finished:\n{}", self.visible());
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    /// Leave with Ctrl+C the way a person does: when the console says "press Ctrl+C again to quit" (text typed, or a
    /// task still finishing: the text a test waited for can be the streaming copy), press it again. The exit code.
    fn ctrl_c_out(mut self) -> i32 {
        let ask = "press Ctrl+C again to quit";
        let before = self.written(ask);
        self.type_keys("\x03");
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut asked_again = false;
        loop {
            if let Some(st) = self.child.try_wait().unwrap() {
                return st.code().unwrap_or(-1);
            }
            // Only a new ask counts (an older note may still be on screen), and the console may be leaving already.
            if !asked_again && self.written(ask) > before {
                asked_again = true;
                let _ = self.writer.write_all(b"\x03");
                let _ = self.writer.flush();
            }
            assert!(Instant::now() < deadline, "the console did not exit:\n{}", self.everything());
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn wait_exit(mut self) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(st) = self.child.try_wait().unwrap() {
                return st.code().unwrap_or(-1);
            }
            assert!(Instant::now() < deadline, "the console did not exit:\n{}", self.everything());
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
fn the_inline_console_pins_a_block_prints_into_scrollback_and_never_takes_over_the_screen() {
    let m = mock(vec![reply_tool("toolu_1", "read_file", json!({"path": "@project/proj/src/lib.rs"})), reply_text("The file defines `needle`, which returns 42.\nIt takes no arguments.")]);
    let e = setup(m.port);
    // A short terminal, so that what is printed must scroll.
    let mut c = PtyConsole::start(&e.home, &["chat", "--workspace", "e2e"], 24, 100);
    c.wait_for("daemon pid");
    let pid = {
        let txt = c.everything();
        let rest = txt.split("daemon pid ").nth(1).unwrap_or_else(|| panic!("no daemon pid in:\n{}", txt));
        rest.chars().take_while(|ch| ch.is_ascii_digit()).collect::<String>().parse::<i32>().unwrap()
    };
    c.wait_for("/help for commands");
    {
        let raw = c.bytes.lock().unwrap().clone();
        let text = String::from_utf8_lossy(&raw).to_string();
        assert!(!text.contains("\x1b[?1049h") && !text.contains("\x1b[?47h"), "the alternate screen is never used");
        assert_eq!(text.matches("\x1b[2J").count(), 1, "the screen is cleared exactly once, at the start");
        assert!(text.starts_with("\x1b[2J"), "and before anything else is drawn");
    }
    // The pinned block: header, prompt and footer, without boxes or money. It is drawn just after the first text.
    c.wait_for("shortcuts: ?");
    let first = c.visible();
    for want in ["all agents idle", "delphi v", "mission", "›", "0 tokens · approvals · shortcuts: ?"] {
        assert!(first.contains(want), "missing `{}` in\n{}", want, first);
    }
    let block: Vec<&str> = first.lines().filter(|l| !l.trim().is_empty()).collect();
    let block = block[block.len().saturating_sub(6)..].join("\n");
    assert!(!block.contains('$') && block.contains("shortcuts: ?"), "no cost in the pinned block:\n{}", block);

    c.type_keys("/ratify\r");
    c.wait_for("pending proposal");
    c.type_keys("/ratify apply 1\r");
    c.wait_for("ratified ws:config.toml#access");
    c.type_keys("What does @project/proj/src/lib.rs define?\r");
    c.wait_for("It takes no arguments.");
    c.wait_for("✓ t_0001 done");
    // The line sent stays in the scrollback as typed, and the task's output is whole lines above the block.
    let all = c.everything();
    assert!(all.contains("› What does @project/proj/src/lib.rs define?") && all.contains("→ read_file(@project/proj/src/lib.rs)") && all.contains("The file defines `needle`, which returns 42."), "{}", all);
    assert!(all.contains("delphi · mission"), "the opening lines are on screen or in the scrollback:\n{}", all);
    // Print more than a screenful: lines scroll off the top into the terminal's own history, and the block stays put.
    for _ in 0..3 {
        c.type_keys("/help\r");
    }
    // Wait for what this means, not for a line of help text (which scrolls out of the visible history as help grows):
    // lines have scrolled into the terminal's history, and then the screen has stopped changing.
    let deadline = Instant::now() + Duration::from_secs(20);
    while c.lines_in_scrollback() < 10 {
        assert!(Instant::now() < deadline, "{}", c.everything());
        std::thread::sleep(Duration::from_millis(60));
    }
    let (mut last, mut stable) = (c.visible(), 0);
    while stable < 6 {
        assert!(Instant::now() < deadline, "the screen never settled:\n{}", c.visible());
        std::thread::sleep(Duration::from_millis(60));
        let now = c.visible();
        if now == last {
            stable += 1;
        } else {
            stable = 0;
            last = now;
        }
    }
    assert!(c.lines_in_scrollback() >= 10, "output scrolled into the terminal's scrollback ({} lines), it was not redrawn over:\n{}", c.lines_in_scrollback(), c.everything());
    let tail = c.visible();
    let shown: Vec<&str> = tail.lines().collect();
    assert!(shown.last().map(|l| l.contains("shortcuts: ?")).unwrap_or(false), "the footer is still the bottom line after scrolling:\n{}", tail);
    c.type_keys("/budget\r");
    c.wait_for("ceiling $5.00");
    // Typing is never dropped while the screen changes.
    c.type_keys("abcdefghijklmnopqrstuvwxyz");
    c.wait_for("› abcdefghijklmnopqrstuvwxyz");
    c.type_keys("\x15/quit\r"); // Ctrl+U, then quit
    assert_eq!(c.wait_exit(), 0);
    kill_daemon(pid);
}

#[test]
fn the_plain_console_is_still_there_and_gives_the_same_answers() {
    let m = mock(vec![reply_text("unused")]);
    let e = setup(m.port);
    let mut s = Session::start(&e.home, &["chat", "--plain", "--workspace", "e2e"]);
    s.wait_for("/help for commands");
    s.send("/budget");
    s.wait_for("ceiling $5.00");
    let (code, out) = s.finish();
    assert_eq!(code, 0, "{}", out);
    assert!(!out.contains('\x1b'), "plain output carries no escape sequences");
    kill_daemon(pid_in(&out));
}

#[test]
fn a_long_reply_is_cut_to_a_screenful_saved_whole_and_available_with_more() {
    let long: String = (1..=60).map(|i| format!("line {} of the long answer", i)).collect::<Vec<_>>().join("\n");
    let m = mock(vec![reply_text(&long)]);
    let e = setup(m.port);
    let mut c = PtyConsole::start(&e.home, &["chat", "--workspace", "e2e"], 50, 100);
    c.wait_for("daemon pid");
    c.wait_for("/help for commands");
    let pid = {
        let txt = c.everything();
        let rest = txt.split("daemon pid ").nth(1).unwrap();
        rest.chars().take_while(|ch| ch.is_ascii_digit()).collect::<String>().parse::<i32>().unwrap()
    };
    c.type_keys("/ratify\r");
    c.wait_for("pending proposal");
    c.type_keys("/ratify apply 1\r");
    c.wait_for("ratified ws:config.toml#access");
    c.type_keys("Tell me everything\r");
    c.wait_for("more lines");
    c.wait_for("✓ t_0001 done");
    let all = c.everything();
    assert!(all.contains("line 24 of the long answer") && !all.contains("line 25 of the long answer"), "24 lines are shown and no more:\n{}", all);
    assert!(all.contains("… 36 more lines · missions/") && all.contains("/more prints them"), "{}", all);
    assert!(all.contains("⟦") && all.contains(" · ") , "the reply is introduced by its speaker's badge and the time");
    // The whole reply is on disk, as a document with frontmatter, and its path was written as a link.
    let saved = std::fs::read_dir(e.ws.join("local/missions")).unwrap().flatten().flat_map(|m| std::fs::read_dir(m.path().join("replies")).into_iter().flatten().flatten()).next().expect("a saved reply").path();
    let text = std::fs::read_to_string(&saved).unwrap();
    assert!(text.starts_with("---\n") && text.contains("line 60 of the long answer") && text.contains("source_type: internal_generated"), "{}", text);
    let raw = String::from_utf8_lossy(&c.bytes.lock().unwrap()).to_string();
    assert!(raw.contains("\x1b]8;;file://") && raw.contains(".md\x1b]8;;\x1b\\"), "the path is an OSC 8 link");
    // /more prints the rest, and then there is nothing more.
    c.type_keys("/more\r");
    c.wait_for("line 60 of the long answer");
    // Everything the console wrote, not the two screens `everything()` can show: whether line 25 is still within
    // them depends on how much more output (the redrawn block) has arrived, which is a race.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut raw = String::new();
    while Instant::now() < deadline {
        raw = String::from_utf8_lossy(&c.bytes.lock().unwrap()).to_string();
        if raw.contains("line 25 of the long answer") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(raw.contains("line 25 of the long answer"), "/more printed lines 25 to 60:\n{}", raw.escape_debug());
    c.type_keys("/more\r");
    c.wait_for("nothing more");
    c.type_keys("/quit\r");
    assert_eq!(c.wait_exit(), 0);
    kill_daemon(pid);
}

/// The command that starts the full-screen console: the compiled executable when CI built one
/// (`DELPHI_UI_COMPILED`), else the sources run with Node. `None` when neither is available.
fn ui_launcher(dir: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DELPHI_UI_COMPILED") {
        return Some(PathBuf::from(p));
    }
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui");
    if !ui.join("node_modules/ink").exists() || Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: ui/node_modules or node is missing (run `npm ci` in ui/)");
        return None;
    }
    let wrapper = dir.join("delphi-ui");
    std::fs::write(&wrapper, format!("#!/bin/sh\ncd '{}' && exec node --import tsx src/main.tsx \"$@\"\n", ui.canonicalize().unwrap().display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    Some(wrapper)
}

/// The full-screen console is a separate program (`ui/`, D-28). This drives it against the real daemon through the
/// real `delphi chat` launcher; it runs only where `npm ci` has been done, and is skipped otherwise.
#[test]
fn the_full_screen_console_attaches_to_the_real_daemon_sends_a_task_and_replays_history() {
    let m = mock(vec![reply_text("Hello **from** the daemon.\n\n- one\n- two")]);
    let e = setup(m.port);
    let Some(wrapper) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", wrapper.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("delphi v");
    c.wait_for("← / ? →");
    c.type_keys("/ratify");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("pending proposal");
    c.type_keys("/ratify apply 1");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("ratified ws:config.toml#access");
    c.type_keys("Tell me something");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("Hello from the daemon.");
    // The text first shows dim as it streams; the finished reply (badge line, rendered list) follows.
    c.wait_for("Athena · ");
    c.wait_for("• one");
    let screen = c.visible();
    assert!(screen.contains("you · ") && screen.contains("Tell me something") && screen.contains("• one") && screen.contains("Athena · "), "{}", screen);
    assert!(screen.lines().nth(1).unwrap().contains("· chat"), "the title row is directly under the strip: {}", screen);
    assert!(!screen.contains("$"), "no money on the main screen: {}", screen);
    assert_eq!(c.ctrl_c_out(), 0);

    // Reopen: the conversation is replayed from the mission's events.
    let c2 = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c2.wait_for("Hello from the daemon.");
    c2.expect_visible("Tell me something");
    c2.expect_visible("you · ");
    drop(c2);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

/// `specs/005e` §6.7: a first run with no model. Declining the guided setup asks once, the console says plainly that
/// nothing can answer, and a prompt is met by an attention card that names the problem and the fix: never
/// "throttled", never the engine's bracketed wrapper.
#[test]
fn declining_setup_asks_once_and_a_prompt_with_no_model_says_so_plainly() {
    let t = tempfile::Builder::new().prefix("dl").tempdir_in("/tmp").unwrap();
    let (home, proj) = (t.path().join("h"), t.path().join("app"));
    let _reap = common::DaemonReaper::new(&home);
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    let Some(wrapper) = ui_launcher(t.path()) else { return };
    let envs = [("DELPHI_UI", wrapper.to_str().unwrap())];
    let mut c = PtyConsole::start_in(Some(&proj), &home, &[], 30, 100, &envs);
    c.wait_for("Run the guided setup now?");
    c.type_keys("n\r");
    // Whatever else the doctor offers is declined too; the console opens once the questions stop.
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut answered = 1; // the setup question, answered above
    while !c.visible().contains("delphi v") {
        assert!(Instant::now() < deadline, "the console did not open:\n{}", c.everything());
        let asks = c.written("[Y/n]");
        if asks > answered {
            answered = asks;
            c.type_keys("n\r");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(c.written("Run the guided setup now?"), 1, "the setup is offered once:\n{}", c.everything());
    c.wait_for("delphi · no model configured");
    c.wait_for("0 tokens  ·  default  ·  no model");
    c.type_keys("hello");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("Stopped: no model is configured for this workspace.");
    c.wait_for("needs you");
    let screen = c.visible();
    assert!(!screen.contains("throttled") && !screen.contains("[escalated"), "{}", screen);
    assert!(screen.contains("delphi setup") && screen.contains("/models"), "{}", screen);
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "app", "--home"]).arg(&home).output();
}

/// `specs/005e` §10.1: the footer promises `?`, so `?` must do something. On an empty prompt it opens the key list;
/// inside text it is a character; Esc closes the list.
#[test]
fn question_mark_opens_the_key_list_on_an_empty_prompt_and_esc_closes_it() {
    let m = mock(vec![]);
    let e = setup(m.port);
    let Some(wrapper) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", wrapper.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    c.type_keys("?");
    c.wait_for("keys · esc closes");
    // A frame arrives in pieces: look until the list's rows are drawn (see `expect_visible`), not at the instant its title was.
    c.expect_visible("shift+tab  approval mode");
    let open = c.visible();
    assert!(!open.contains("❯ ?"), "the ? opened the list instead of being typed:\n{}", open);
    c.type_keys("\x1b");
    let deadline = Instant::now() + Duration::from_secs(5);
    while c.visible().contains("keys · esc closes") {
        assert!(Instant::now() < deadline, "Esc did not close the key list:\n{}", c.visible());
        std::thread::sleep(Duration::from_millis(60));
    }
    c.type_keys("why");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("?");
    c.wait_for("❯ why?");
    assert!(!c.visible().contains("keys · esc closes"), "a ? inside text is a character:\n{}", c.visible());
    c.type_keys("\x03"); // there is text in the prompt, so the first Ctrl+C asks
    c.wait_for("press Ctrl+C again to quit");
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

/// `specs/005e` §7.2 and finding 6: a long reply starts at its label row instead of scrolling its beginning off the
/// screen; a marker says how much waits below; `Space` pages, `Enter` on an empty prompt jumps to the end. The blocks
/// are tinted from the terminal's own background, which the console asks for and this terminal answers.
#[test]
fn a_long_reply_is_pinned_at_its_label_row_and_space_pages_and_enter_jumps_to_the_end() {
    let long = (1..=40).map(|i| format!("Paragraph {} of the long answer.", i)).collect::<Vec<_>>().join("\n\n");
    let m = mock(vec![reply_text(&long)]);
    let e = setup(m.port);
    let Some(wrapper) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", wrapper.to_str().unwrap()), ("COLORTERM", "")]; // 256 colours: the depth is decided by TERM alone
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    for line in ["/ratify", "/ratify apply 1"] {
        c.type_keys(line);
        std::thread::sleep(Duration::from_millis(150));
        c.type_keys("\r");
        c.wait_for(if line == "/ratify" { "pending proposal" } else { "ratified ws:config.toml#access" });
    }
    c.type_keys("Explain everything.");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("more lines · pgdn or space · enter jumps to the end");
    std::thread::sleep(Duration::from_millis(300));
    let screen = c.visible();
    let rows: Vec<&str> = screen.lines().collect();
    assert!(rows[3].contains("Athena · "), "the first body row is the reply's label row:\n{}", screen);
    assert!(rows[4].trim_start().starts_with('▄') && screen.contains("Paragraph 1 of the long answer."), "the block's top cap, then its beginning:\n{}", screen);
    assert!(String::from_utf8_lossy(&c.bytes.lock().unwrap()).contains("48;5;"), "the tint reached the terminal as a 256-colour background");
    c.type_keys(" ");
    let deadline = Instant::now() + Duration::from_secs(5);
    while c.visible().contains("Paragraph 1 of the long answer.") {
        assert!(Instant::now() < deadline, "Space did not page down:\n{}", c.visible());
        std::thread::sleep(Duration::from_millis(60));
    }
    c.expect_visible("more lines");
    c.type_keys("\r");
    c.wait_for("Paragraph 40 of the long answer.");
    assert!(!c.visible().contains("more lines"), "at the end there is no marker:\n{}", c.visible());
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

/// `specs/005e` §8: the left deck is a ticker. When a span ends it leaves the running zone and one `✔ finished` entry
/// joins the history; the chat never shows that chip.
#[test]
fn a_finished_span_becomes_a_history_entry_in_the_activity_deck_and_never_shows_in_the_chat() {
    let m = mock(vec![reply_text("Done and dusted.")]);
    let e = setup(m.port);
    let Some(wrapper) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", wrapper.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    for line in ["/ratify", "/ratify apply 1"] {
        c.type_keys(line);
        std::thread::sleep(Duration::from_millis(150));
        c.type_keys("\r");
        c.wait_for(if line == "/ratify" { "pending proposal" } else { "ratified ws:config.toml#access" });
    }
    c.type_keys("Say something.");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("Done and dusted.");
    assert!(!c.visible().contains("finished"), "the chat never shows the chip:\n{}", c.visible());
    c.type_keys("\x1b[1;2D"); // Shift+Left: the activity deck
    c.wait_for("✔ finished");
    let deck = c.visible();
    assert!(deck.contains("· activity") && deck.contains(r#"answered "Say something.""#), "one history entry says what was answered:\n{}", deck);
    c.type_keys("\x1b[1;2C"); // back to the chat
    std::thread::sleep(Duration::from_millis(300));
    assert!(!c.visible().contains("✔ finished"), "{}", c.visible());
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

/// `specs/005e` §10.5, inline console: `Esc` clears the prompt first and, with it empty, stops the running task;
/// `Ctrl+C` asks once when there is text or a task running, and leaves at once when nothing would be lost.
#[test]
fn the_inline_console_stops_a_task_with_esc_and_asks_before_ctrl_c_throws_anything_away() {
    let m = mock(vec![reply_tool("w1", "write_file", json!({"path": "specs/009-e2e.md", "content": "# E2E spec"}))]);
    let e = setup(m.port);
    let mut c = PtyConsole::start(&e.home, &["chat", "--workspace", "e2e"], 24, 100);
    c.wait_for("shortcuts: ?");
    c.type_keys("/ratify\r");
    c.wait_for("pending proposal");
    c.type_keys("/ratify apply 1\r");
    c.wait_for("ratified ws:config.toml#access");
    c.type_keys("@architect Draft a spec called 009-e2e\r");
    c.wait_for("approval needed"); // the task now waits for the operator: it is running
    // Typed text first: Esc clears it and does not stop the task.
    c.type_keys("half a thought");
    std::thread::sleep(Duration::from_millis(200));
    c.type_keys("\x1b");
    let deadline = Instant::now() + Duration::from_secs(5);
    while c.visible().contains("half a thought") {
        assert!(Instant::now() < deadline, "Esc did not clear the prompt:\n{}", c.visible());
        std::thread::sleep(Duration::from_millis(60));
    }
    assert!(!c.everything().contains("stopping"), "the first Esc only cleared the prompt");
    // Now the prompt is empty: Esc stops the task.
    c.type_keys("\x1b");
    c.wait_for("stopping");
    c.wait_for("■ stopped");
    assert!(!e.ws.join("team/specs/009-e2e.md").exists(), "the action that was waiting never ran");
    // Idle with text typed: Ctrl+C asks and keeps the text.
    c.type_keys("abc");
    std::thread::sleep(Duration::from_millis(200));
    c.type_keys("\x03");
    c.wait_for("press Ctrl+C again to quit");
    c.expect_visible("abc");
    // Esc clears it; with nothing typed and nothing running, one Ctrl+C leaves at once.
    c.type_keys("\x1b");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

/// The same rule in the full-screen console: text in the prompt means the first `Ctrl+C` asks.
#[test]
fn the_full_screen_console_asks_before_ctrl_c_throws_a_draft_away() {
    let m = mock(vec![]);
    let e = setup(m.port);
    let Some(wrapper) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", wrapper.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    c.type_keys("a draft");
    std::thread::sleep(Duration::from_millis(200));
    c.type_keys("\x03");
    c.wait_for("press Ctrl+C again to quit");
    c.expect_visible("a draft");
    assert_eq!(c.ctrl_c_out(), 0);
    // Empty prompt, nothing running: Ctrl+D leaves too.
    let mut d = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    d.wait_for("← / ? →");
    d.type_keys("\x04");
    assert_eq!(d.wait_exit(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

#[test]
fn esc_stops_a_task_waiting_on_an_approval_in_the_full_screen_console_and_nothing_runs() {
    let m = mock(vec![reply_tool("w1", "write_file", json!({"path": "specs/009-e2e.md", "content": "# never written"})), reply_text("Drafted.")]);
    let e = setup(m.port);
    let Some(wrapper) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", wrapper.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    for line in ["/ratify", "/ratify apply 1"] {
        c.type_keys(line);
        std::thread::sleep(Duration::from_millis(150));
        c.type_keys("\r");
        c.wait_for(if line == "/ratify" { "pending proposal" } else { "ratified ws:config.toml#access" });
    }
    c.type_keys("@architect Draft a spec called 009-e2e");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("1 waiting"); // the badge and the footer count carry it; the card is in the chat
    c.wait_for("approval needed [amber]");
    c.wait_for("esc to stop");
    // A message sent while the task waits is queued, not sent.
    c.type_keys("and then another thing");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("(queued)");
    c.wait_for("1 queued");
    // Esc with an empty prompt stops the task.
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\x1b");
    c.wait_for("■ stopped");
    c.wait_for("1 queued message(s) were dropped");
    std::thread::sleep(Duration::from_millis(500));
    let screen = c.visible();
    assert!(!screen.contains("esc to stop") && screen.contains("delphi v"), "{}", screen);
    assert!(!e.ws.join("team/specs/009-e2e.md").exists(), "the action that was waiting never ran");
    assert_eq!(m.seen.lock().unwrap().len(), 1, "the queued message was never sent");
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

/// Every SGR sequence in `bytes` that sets a colour (foreground, background, bright or extended).
fn colour_codes(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for part in text.split("\x1b[").skip(1) {
        let Some(end) = part.find(|c: char| !(c.is_ascii_digit() || c == ';')) else { continue };
        if part.as_bytes()[end] != b'm' {
            continue;
        }
        let colour = part[..end].split(';').filter_map(|p| p.parse::<u32>().ok()).any(|p| matches!(p, 30..=38 | 40..=48 | 90..=97 | 100..=107));
        if colour {
            out.push(format!("\\x1b[{}m", &part[..end]));
        }
    }
    out
}

/// `specs/005e` §5: with `NO_COLOR` set, no console writes a colour code, and each still reads (structure by glyph).
fn no_colour_session(full_screen: bool) {
    let m = mock(vec![reply_text("Plain words, no colour.")]);
    let e = setup(m.port);
    let wrapper = if full_screen { ui_launcher(e._t.path()) } else { None };
    if full_screen && wrapper.is_none() {
        return;
    }
    let mut envs = vec![("NO_COLOR", "1")];
    if let Some(w) = &wrapper {
        envs.push(("DELPHI_UI", w.to_str().unwrap()));
    }
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    if full_screen {
        c.wait_for("← / ? →");
    } else {
        c.wait_for("/help for commands");
    }
    for line in ["/ratify", "/ratify apply 1"] {
        c.type_keys(line);
        std::thread::sleep(Duration::from_millis(150));
        c.type_keys("\r");
        c.wait_for(if line == "/ratify" { "pending proposal" } else { "ratified ws:config.toml#access" });
    }
    c.type_keys("Say something");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("Plain words, no colour.");
    let screen = c.everything();
    assert!(screen.contains("⟦"), "the badges are still there:\n{}", screen);
    let found = colour_codes(&c.bytes.lock().unwrap());
    assert!(found.is_empty(), "NO_COLOR=1 but the console wrote colour codes: {:?}", &found[..found.len().min(8)]);
    if full_screen {
        assert_eq!(c.ctrl_c_out(), 0);
    } else {
        c.type_keys("/quit\r");
        assert_eq!(c.wait_exit(), 0);
    }
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

#[test]
fn the_colour_detector_sees_colour_and_ignores_attributes() {
    assert_eq!(colour_codes(b"a\x1b[38;2;1;2;3mb\x1b[39m\x1b[48;5;240mc"), vec!["\\x1b[38;2;1;2;3m", "\\x1b[48;5;240m"]);
    assert_eq!(colour_codes(b"\x1b[36mx\x1b[91my\x1b[104mz").len(), 3);
    assert!(colour_codes(b"\x1b[1mbold\x1b[22m\x1b[2mdim\x1b[0m\x1b[7m\x1b[2J\x1b[10;5H").is_empty());
}

#[test]
fn the_full_screen_console_writes_no_colour_code_under_no_color() {
    no_colour_session(true);
}

#[test]
fn the_inline_console_writes_no_colour_code_under_no_color() {
    no_colour_session(false);
}

#[test]
fn a_console_of_another_release_hands_over_to_the_inline_console() {
    let m = mock(vec![]);
    let e = setup(m.port);
    let stale = e._t.path().join("delphi-ui");
    std::fs::write(&stale, "#!/bin/sh\necho 'delphi-ui 0.0.1 does not match' >&2\nexit 3\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &[("DELPHI_UI", stale.to_str().unwrap())]);
    c.wait_for("all agents idle");
    c.wait_for("shortcuts: ?");
    c.type_keys("/quit\r");
    assert_eq!(c.wait_exit(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

#[test]
fn the_decision_inbox_approves_one_card_bound_to_its_payload_and_shift_tab_changes_the_mode() {
    let m = mock(vec![reply_tool("w1", "write_file", json!({"path": "specs/009-e2e.md", "content": "# E2E spec\nbody"})), reply_text("Drafted specs/009-e2e.md.")]);
    let e = setup(m.port);
    let Some(launcher) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", launcher.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    for line in ["/ratify", "/ratify apply 1"] {
        c.type_keys(line);
        std::thread::sleep(Duration::from_millis(150));
        c.type_keys("\r");
        c.wait_for(if line == "/ratify" { "pending proposal" } else { "ratified ws:config.toml#access" });
    }
    // Shift+Tab cycles the approval mode, shown in the footer, and the daemon agrees.
    c.type_keys("\x1b[Z");
    c.wait_for("· approval mode: accept edits — file edits run without asking");
    c.wait_for("tokens  ·  accept edits");
    c.type_keys("\x1b[Z");
    c.wait_for("· approval mode: auto — Amber actions run without asking this mission");
    c.wait_for("tokens  ·  auto");
    c.cycle_back_to_default(); // through `auto · optimistic` where the OS sandbox exists, straight to default where it does not
    c.wait_for("tokens  ·  default"); // default mode is named too
    c.type_keys("@architect Draft a spec called 009-e2e");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("1 waiting"); // the badge and the footer count carry it; the card is in the chat
    // Open the inbox (Shift+Right), wait out the guard, open the card, approve with option 1.
    c.type_keys("\x1b[1;2C");
    c.wait_for("Pending decisions (1)");
    std::thread::sleep(Duration::from_millis(500));
    c.type_keys("\r");
    c.wait_for("Approval required · 1 of 1 pending");
    c.wait_for("1. Approve once");
    assert!(!e.ws.join("team/specs/009-e2e.md").exists(), "nothing is written before the answer");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("1");
    c.wait_for("nothing needs you");
    c.type_keys("\x1b");
    c.wait_for("Drafted specs/009-e2e.md.");
    assert!(std::fs::read_to_string(e.ws.join("team/specs/009-e2e.md")).unwrap().contains("# E2E spec"));
    // The reply can appear a frame before the status line settles, so wait for it rather than assert on one snapshot.
    c.wait_for("delphi v");
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

#[test]
fn the_full_screen_observer_shows_a_running_mission_through_the_narrator_socket() {
    let m = mock(vec![reply_text("Observed answer.")]);
    let e = setup(m.port);
    let Some(launcher) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", launcher.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    for line in ["/ratify", "/ratify apply 1"] {
        c.type_keys(line);
        std::thread::sleep(Duration::from_millis(150));
        c.type_keys("\r");
        c.wait_for(if line == "/ratify" { "pending proposal" } else { "ratified ws:config.toml#access" });
    }
    c.type_keys("Say something");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("Observed answer.");
    c.wait_for("Athena · ");
    // The left deck shows the stream; the console stays attached while the Observer opens in its own window.
    let mut o = PtyConsole::start_env(&e.home, &["observe", "agents", "--workspace", "e2e"], 34, 110, &envs);
    o.wait_for("· agents");
    o.wait_for("opus-t"); // the model the orchestrator is on
    let roster = o.visible();
    assert!(roster.contains("delphi v") && roster.contains("orchestrator") && roster.contains("the system is idle"), "{}", roster);
    assert!(roster.contains("← →  ·  ○ ○ ○ ● ○ ○") && !roster.contains('$'), "{}", roster);
    // Only the arrows change the view: Right to archives (pulse, timeline and tools lie between), Left back to epic. In the
    // other views digits, Tab and q do nothing; in archives, the knowledge window, they are text for the search box.
    for _ in 0..4 {
        o.type_keys("\x1b[C");
        std::thread::sleep(Duration::from_millis(120));
    }
    o.wait_for("search what Delphi knows");
    o.wait_for("collections"); // the shelf: read through `kb.get` on the narrator socket
    o.type_keys("council");
    o.type_keys("\r");
    o.wait_for("result"); // "N results · 0.4 s · lexical": the daemon's own search, with no model installed here
    o.type_keys("\x14"); // ctrl+t: trusted documents only, asked of the daemon again
    o.wait_for("trusted only");
    o.type_keys("\x14"); // and every document again
    o.type_keys("\r"); // the first result opens in the reader
    o.wait_for("[ back"); // the reader's hint row, whether or not the document has links
    o.type_keys("\x7f"); // backspace goes back to the results
    o.wait_for("enter open");
    for _ in 0..5 {
        // archives to epic: tools, timeline, pulse and agents lie between them
        o.type_keys("\x1b[D");
        std::thread::sleep(Duration::from_millis(120));
    }
    o.wait_for("· epic");
    o.wait_for("settled"); // the threads view: the finished task is one settled line
    for _ in 0..9 {
        o.type_keys("\x1b[C"); // it stops at the last view, so more presses than views is fine
        std::thread::sleep(Duration::from_millis(120));
    }
    o.wait_for("· changes");
    o.wait_for("no changes recorded yet");
    assert_eq!(o.ctrl_c_out(), 0);
    // The dashboard (`specs/030`): the operational view, with the sessions, what runs now and the pace, from the same socket.
    let d = PtyConsole::start_env(&e.home, &["observe", "dashboard", "--workspace", "e2e"], 34, 110, &envs);
    d.wait_for("· dashboard");
    d.wait_for("running now");
    d.wait_for("nothing is running: the system is idle");
    d.wait_for("tokens/min");
    assert_eq!(d.ctrl_c_out(), 0);
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

/// `delphi observe --once` from a process with no controlling terminal (a script, cron): the narrator scope is
/// read-only, so the daemon issues it without one; the same process asking for a console token is refused.
#[test]
fn observe_once_prints_a_frame_without_any_terminal() {
    use std::os::unix::process::CommandExt;
    let m = mock(vec![]);
    let e = setup(m.port);
    let Some(launcher) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", launcher.to_str().unwrap())];
    let c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    let mut cmd = Command::new(DELPHI);
    cmd.args(["observe", "council", "--once", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_UI", &launcher).stdin(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid(); // a session of its own: no controlling terminal
            Ok(())
        });
    }
    let out = cmd.output().unwrap();
    let (code, frame) = (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned());
    assert_eq!(code, 0, "{}\n{}", frame, String::from_utf8_lossy(&out.stderr));
    assert!(frame.contains("delphi v") && frame.contains("· council") && frame.contains("needs you"), "{}", frame);
    assert_eq!(frame.lines().count(), 24, "no terminal means 80x24:\n{}", frame);
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

#[test]
fn research_panels_search_the_knowledge_base_cite_into_the_prompt_and_pin_to_the_notebook() {
    let m = mock(vec![reply_text("The ladder is described in specs/003-collaboration-and-council.md#ladder and in REVIEW.md.")]);
    let e = setup(m.port);
    let Some(launcher) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", launcher.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    for line in ["/ratify", "/ratify apply 1"] {
        c.type_keys(line);
        std::thread::sleep(Duration::from_millis(150));
        c.type_keys("\r");
        c.wait_for(if line == "/ratify" { "pending proposal" } else { "ratified ws:config.toml#access" });
    }
    c.type_keys("Where is the retry ladder described?");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("Athena · ");
    // Where the answer came from: the reply cites two documents.
    c.type_keys("/sources");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("Sources (2)");
    c.wait_for("specs/003-collaboration-and-council.md#ladder");
    c.type_keys("\x1b");
    c.wait_for("← / ? →");
    // Search the knowledge base with Ctrl+K; Enter on a hit puts its pointer in the prompt.
    c.type_keys("\x0b");
    c.wait_for("Search the knowledge base");
    c.type_keys("review");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("enter put @path in the prompt");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("p");
    c.wait_for("pinned to the mission notebook");
    c.type_keys("\r");
    c.wait_for("❯ @");
    c.type_keys("\x15"); // clear the prompt
    // The notebook holds the pinned hit; the last reply can be pinned too.
    c.type_keys("/pin");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.type_keys("/notes");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("Mission notebook");
    c.wait_for("search “review”");
    let notes = std::fs::read_dir(e.ws.join("local/missions")).unwrap().flatten().map(|m| m.path().join("notes.md")).find(|p| p.exists()).expect("a notebook was written");
    let text = std::fs::read_to_string(&notes).unwrap();
    assert!(text.starts_with("---\n") && text.contains("source_type: internal_generated") && text.contains("# Notebook"), "{}", text);
    assert!(text.contains("· athena (orchestrator)") || text.contains("· Athena (orchestrator)"), "the last reply was pinned with its speaker:\n{}", text);
    assert!(text.contains("The ladder is described in specs/003-collaboration-and-council.md#ladder"), "{}", text);
    c.type_keys("\x1b");
    // Connector status comes from the CLI's own offline report.
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("/connectors");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("no connectors are configured");
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

/// One long, scripted sitting at the full-screen console against the real daemon: a task that needs approval, the
/// decision inbox, a pin and an export, the approval mode, two window resizes, and a clean exit that gives the
/// terminal back. Each step waits for what it expects, so a regression names the step that broke.
#[test]
fn a_scripted_session_through_the_full_screen_console_ends_with_the_terminal_given_back() {
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui");
    if !ui.join("node_modules/ink").exists() || Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: ui/node_modules or node is missing (run `npm ci` in ui/)");
        return;
    }
    let m = mock(vec![reply_tool("w1", "write_file", json!({"path": "specs/009-e2e.md", "content": "# never written"})), reply_text("Understood, nothing was written.")]);
    let e = setup(m.port);
    let wrapper = e._t.path().join("delphi-ui");
    std::fs::write(&wrapper, format!("#!/bin/sh\ncd '{}' && exec node --import tsx src/main.tsx \"$@\"\n", ui.canonicalize().unwrap().display())).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let envs = [("DELPHI_UI", wrapper.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 36, 110, &envs);
    let raw = c.bytes.clone();
    let line = |c: &mut PtyConsole, text: &str| {
        c.type_keys(text);
        std::thread::sleep(Duration::from_millis(150));
        c.type_keys("\r");
    };
    c.wait_for("← / ? →");
    line(&mut c, "/ratify");
    c.wait_for("pending proposal");
    line(&mut c, "/ratify apply 1");
    c.wait_for("ratified ws:config.toml#access");

    // A task that needs the operator, answered from the inbox.
    line(&mut c, "@architect Draft a spec called 009-e2e");
    c.wait_for("approval needed [amber]");
    c.type_keys("\x1b[C"); // Right, with nothing typed: the next deck
    c.wait_for("Pending decisions (1)");
    std::thread::sleep(Duration::from_millis(400)); // the 300 ms guard
    c.type_keys("n");
    c.wait_for("nothing needs you");
    c.type_keys("\x1b");
    c.wait_for("Understood, nothing was written.");
    c.wait_for("Daedalus · ");
    // The reply streams in a block labelled "writing"; the finished one (labelled with its time) is what /pin keeps.
    let settled = Instant::now() + Duration::from_secs(20);
    while c.visible().contains("Daedalus · writing") {
        assert!(Instant::now() < settled, "the reply never finished:\n{}", c.everything());
        std::thread::sleep(Duration::from_millis(60));
    }
    assert!(!e.ws.join("team/specs/009-e2e.md").exists(), "the denied action never ran");

    // Arrows edit a prompt that has text and move between decks when it is empty; Shift+arrows always move a deck.
    c.type_keys("abc");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\x1b[D"); // Left, with text: the cursor moves, the deck stays
    std::thread::sleep(Duration::from_millis(300));
    let kept = c.visible();
    assert!(kept.contains("❯ abc") && kept.contains("← / ? →  ·  ○ ● ○"), "{}", kept);
    c.type_keys("\x1b[1;2D"); // Shift+Left: the left deck, the draft waits
    c.wait_for("← / ? →  ·  ● ○ ○");
    c.type_keys("\x1b[1;2C");
    c.wait_for("❯ abc");
    c.type_keys("\x05\x15"); // End, then delete to the start: an empty prompt
    std::thread::sleep(Duration::from_millis(300));
    assert!(!c.visible().contains("❯ abc"), "{}", c.visible());
    c.type_keys("\x1b[D"); // Left, now empty: the left deck
    c.wait_for("← / ? →  ·  ● ○ ○");
    c.type_keys("\x1b[C"); // and back to the chat
    std::thread::sleep(Duration::from_millis(300));
    c.expect_visible("← / ? →  ·  ○ ● ○");

    // Keep the reply, save the conversation.
    line(&mut c, "/pin");
    c.wait_for("pinned to the mission notebook");
    let notes = std::fs::read_dir(e.ws.join("local/missions")).unwrap().flatten().map(|m| m.path().join("notes.md")).find(|p| p.exists()).expect("a notebook");
    assert!(std::fs::read_to_string(&notes).unwrap().contains("Understood, nothing was written."));
    line(&mut c, "/export");
    c.wait_for("exported to local/missions/");

    // Shift+Tab goes round the approval modes and ends where it started.
    for footer in ["tokens  ·  accept edits", "tokens  ·  auto"] {
        c.type_keys("\x1b[Z");
        c.wait_for(footer);
    }
    c.cycle_back_to_default();
    c.wait_for("tokens  ·  default");

    // The window shrinks and grows: the screen is always exactly the window, never wider, footer on the last row.
    c.resize(24, 70);
    std::thread::sleep(Duration::from_millis(600));
    let small = c.visible();
    let rows: Vec<&str> = small.lines().collect();
    assert_eq!(rows.len(), 24, "{}", small);
    assert!(rows.iter().all(|l| l.chars().count() <= 70), "{}", small);
    assert!(rows[23].contains("← / ? →"), "the footer is on the last row after shrinking:\n{}", small);
    c.resize(36, 110);
    std::thread::sleep(Duration::from_millis(600));
    let big = c.visible();
    let rows: Vec<&str> = big.lines().collect();
    assert_eq!(rows.len(), 36, "{}", big);
    assert!(rows[35].contains("← / ? →"), "{}", big);
    assert!(!big.contains("Pending decisions"), "no leftovers from the inbox:\n{}", big);

    // Leave, and the terminal comes back: alternate screen off, cursor on, mouse and paste modes off.
    // A task may still be running here, in which case Ctrl+C asks once before leaving (`005e` §10.5).
    c.type_keys("\x03");
    std::thread::sleep(Duration::from_millis(400));
    if c.visible().contains("press Ctrl+C again to quit") {
        c.type_keys("\x03");
    }
    assert_eq!(c.wait_exit(), 0);
    std::thread::sleep(Duration::from_millis(200));
    let out = String::from_utf8_lossy(&raw.lock().unwrap()).to_string();
    let (on, off) = (out.rfind("\x1b[?1049h").expect("took over the screen"), out.rfind("\x1b[?1049l").expect("gave the screen back"));
    assert!(off > on, "the last thing was leaving the alternate screen");
    for seq in ["\x1b[?2004l", "\x1b[?1006l", "\x1b[?1000l", "\x1b[?25h"] {
        assert!(out[on..].contains(seq), "restored {:?}", seq);
    }
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

#[test]
fn the_terminal_is_given_back_and_the_mouse_can_be_left_to_the_terminal() {
    let m = mock(vec![]);
    let e = setup(m.port);
    let Some(launcher) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", launcher.to_str().unwrap())];
    // --no-mouse: the wheel is not captured, so the terminal can select and copy text.
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--no-mouse", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    let raw = |c: &PtyConsole| String::from_utf8_lossy(&c.bytes.lock().unwrap()).to_string();
    assert!(raw(&c).contains("\x1b[?1049h") && raw(&c).contains("\x1b[?2004h"), "alternate screen and bracketed paste are on");
    assert!(!raw(&c).contains("\x1b[?1000h"), "mouse reporting stays off with --no-mouse");
    // /mouse turns it on for this session, and says so.
    c.type_keys("/mouse");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("mouse on");
    assert!(raw(&c).contains("\x1b[?1000h"));
    // Leaving puts everything back: alternate screen left, paste and mouse modes off, cursor shown.
    let bytes = c.bytes.clone();
    assert_eq!(c.ctrl_c_out(), 0);
    let end = String::from_utf8_lossy(&bytes.lock().unwrap()).to_string();
    let tail = &end[end.rfind("\x1b[>4;0m").expect("the terminal reset was written")..];
    for want in ["\x1b[?2004l", "\x1b[?1006l", "\x1b[?1000l", "\x1b[?1049l", "\x1b[?25h"] {
        assert!(tail.contains(want), "missing {:?} in the reset {:?}", want, tail);
    }
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

#[test]
fn the_console_survives_a_daemon_restart_and_picks_up_where_it_was() {
    let m = mock(vec![reply_text("First answer."), reply_text("Second answer, after the restart.")]);
    let e = setup(m.port);
    let Some(launcher) = ui_launcher(e._t.path()) else { return };
    let envs = [("DELPHI_UI", launcher.to_str().unwrap())];
    let mut c = PtyConsole::start_env(&e.home, &["chat", "--workspace", "e2e"], 30, 100, &envs);
    c.wait_for("← / ? →");
    for line in ["/ratify", "/ratify apply 1"] {
        c.type_keys(line);
        std::thread::sleep(Duration::from_millis(150));
        c.type_keys("\r");
        c.wait_for(if line == "/ratify" { "pending proposal" } else { "ratified ws:config.toml#access" });
    }
    c.type_keys("One");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("First answer.");
    c.wait_for("Athena · ");
    c.wait_until_not_writing(); // the finished reply is in the log; the streaming copy is not
    // The daemon is stopped and started again behind the console's back.
    // (the CLI asks for a terminal, as the console does, so it runs in one)
    let restart = PtyConsole::start_env(&e.home, &["daemon", "restart", "--workspace", "e2e"], 10, 80, &[]);
    assert_eq!(restart.wait_exit(), 0);
    c.wait_for("the daemon connection closed");
    c.wait_for("reconnected to the daemon");
    // A restart ends the session (`specs/030`): the console comes back to an empty screen, and the next message works.
    let deadline = Instant::now() + Duration::from_secs(10);
    while c.visible().contains("First answer.") {
        assert!(Instant::now() < deadline, "the old session is still on the screen:\n{}", c.visible());
        std::thread::sleep(Duration::from_millis(60));
    }
    c.type_keys("Two");
    std::thread::sleep(Duration::from_millis(150));
    c.type_keys("\r");
    c.wait_for("Second answer, after the restart.");
    assert_eq!(c.ctrl_c_out(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
}

#[test]
fn the_daemon_reports_the_release_of_the_program_that_started_it() {
    let m = mock(vec![]);
    let e = setup(m.port);
    let start = PtyConsole::start_env(&e.home, &["daemon", "restart", "--workspace", "e2e"], 10, 120, &[]);
    assert_eq!(start.wait_exit(), 0);
    let status = PtyConsole::start_env(&e.home, &["daemon", "status", "--workspace", "e2e"], 10, 160, &[]);
    status.wait_for(&format!("running · delphi {} ·", env!("CARGO_PKG_VERSION")));
    assert!(!status.visible().contains("this program is"), "{}", status.visible());
    assert_eq!(status.wait_exit(), 0);
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", "e2e", "--home"]).arg(&e.home).env("DELPHI_E2E_KEY", KEY).output();
    let stop = PtyConsole::start_env(&e.home, &["daemon", "stop", "--workspace", "e2e"], 10, 80, &[]);
    assert_eq!(stop.wait_exit(), 0);
}

// ── delphi run: no terminal, a summary and an exit code ─────────────────────

/// Ratify the endpoints through a short console session, then stop that daemon so `delphi run` starts its own
/// (a script has no terminal, so it can only use a daemon it spawned).
fn ratify_and_stop(e: &Env) {
    let mut s = Session::start(&e.home, &["chat", "--workspace", "e2e"]);
    s.wait_for("/help for commands");
    s.send("/ratify");
    s.wait_for("pending proposal");
    s.send("/ratify apply 1");
    s.wait_for("ratified ws:config.toml#access");
    let (code, out) = s.finish();
    assert_eq!(code, 0, "{}", out);
    kill_daemon(pid_in(&out));
}

/// Run `delphi run` with no stdin and no terminal; returns (exit code, stdout, stderr).
fn delphi_run(e: &Env, args: &[&str]) -> (i32, String, String) {
    let o = Command::new(DELPHI).arg("run").args(args).args(["--workspace", "e2e", "--home", e.home.to_str().unwrap()]).env("DELPHI_E2E_KEY", KEY).stdin(Stdio::null()).output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string())
}

fn summary_of(stdout: &str) -> Value {
    let line = stdout.lines().rev().find(|l| l.contains("\"run.summary\"")).unwrap_or_else(|| panic!("no summary in:\n{}", stdout));
    serde_json::from_str(line).unwrap()
}

#[test]
fn delphi_run_answers_a_task_with_no_terminal_and_reports_a_summary() {
    let m = mock(vec![reply_tool("toolu_1", "read_file", json!({"path": "@project/proj/src/lib.rs"})), reply_text("The file defines `needle`, which returns 42.")]);
    let e = setup(m.port);
    ratify_and_stop(&e);
    let (code, out, err) = delphi_run(&e, &["What does @project/proj/src/lib.rs define?", "--json", "--timeout", "90s"]);
    assert_eq!(code, 0, "stdout:\n{}\nstderr:\n{}", out, err);
    let s = summary_of(&out);
    assert_eq!((s["outcome"].as_str(), s["exit_code"].as_i64()), (Some("completed"), Some(0)));
    assert!(s["text"].as_str().unwrap().contains("needle"), "{}", s);
    assert_eq!(s["cards"]["total"], 0);
    assert!(s["usage"]["rows"].as_array().map(|r| !r.is_empty()).unwrap_or(false), "the cost is in the summary: {}", s["usage"]);
    assert!(!out.contains(KEY) && !err.contains(KEY), "the credential never reaches the output");
    assert_eq!(m.seen.lock().unwrap().len(), 2);
    let (code2, ..) = delphi_run(&e, &["--task", "", "--json"]);
    assert_eq!(code2, 4, "an empty task is refused before any daemon is touched");
}

#[test]
fn delphi_run_denies_a_card_by_default_and_reports_it_and_a_policy_can_allow_amber() {
    let write = || reply_tool("w1", "write_file", json!({"path": "specs/009-e2e.md", "content": "# E2E spec\nbody"}));
    // Run 1: --on-card deny (the default). The model is told no; nothing is written.
    let m = mock(vec![write(), reply_text("I could not write the spec.")]);
    let e = setup(m.port);
    ratify_and_stop(&e);
    let (code, out, err) = delphi_run(&e, &["@architect Draft a spec called 009-e2e", "--json", "--timeout", "90s"]);
    assert_eq!(code, 0, "stdout:\n{}\nstderr:\n{}", out, err);
    let s = summary_of(&out);
    assert_eq!((s["cards"]["total"].as_u64(), s["cards"]["denied"].as_u64()), (Some(1), Some(1)), "{}", s);
    assert_eq!(s["cards"]["list"][0]["tool"], "write_file");
    assert!(!e.ws.join("team/specs/009-e2e.md").exists(), "a denied card writes nothing");
}

#[test]
fn delphi_run_with_a_policy_allows_exactly_the_amber_tool_it_names() {
    let write = || reply_tool("w1", "write_file", json!({"path": "specs/009-e2e.md", "content": "# E2E spec\nbody"}));
    let m = mock(vec![write(), reply_text("Drafted specs/009-e2e.md.")]);
    let e = setup(m.port);
    ratify_and_stop(&e);
    let policy = e._t.path().join("policy.toml");
    std::fs::write(&policy, "[[allow]]\ntool = \"write_file\"\npattern = \"write_file specs/*\"\n").unwrap();
    let (code, out, err) = delphi_run(&e, &["@architect Draft a spec called 009-e2e", "--json", "--timeout", "90s", "--on-card", &format!("policy:{}", policy.display())]);
    assert_eq!(code, 0, "stdout:\n{}\nstderr:\n{}", out, err);
    let s = summary_of(&out);
    assert_eq!((s["cards"]["allowed"].as_u64(), s["cards"]["denied"].as_u64()), (Some(1), Some(0)), "{}", s);
    assert!(e.ws.join("team/specs/009-e2e.md").exists(), "the allowed card ran");
}

#[test]
fn delphi_run_refuses_a_policy_that_tries_to_allow_a_merge_before_touching_anything() {
    let m = mock(vec![]);
    let e = setup(m.port);
    let policy = e._t.path().join("bad.toml");
    std::fs::write(&policy, "[[allow]]\ntool = \"merge\"\n").unwrap();
    let (code, out, err) = delphi_run(&e, &["anything", "--on-card", &format!("policy:{}", policy.display())]);
    assert_eq!(code, 4, "{}{}", out, err);
    assert!(err.contains("no policy can allow it"), "{}", err);
    assert_eq!(m.seen.lock().unwrap().len(), 0);
}

// ── delphi relay: a card by email and an approval by reply ──────────────────

/// A just-enough SMTP server that records every message it is given.
fn fake_smtp(received: Arc<Mutex<Vec<String>>>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let received = received.clone();
            std::thread::spawn(move || {
                let mut w = s.try_clone().unwrap();
                let mut r = BufReader::new(s);
                let _ = w.write_all(b"220 fake ESMTP\r\n");
                let (mut line, mut data, mut in_data) = (String::new(), String::new(), false);
                while {
                    line.clear();
                    r.read_line(&mut line).unwrap_or(0) > 0
                } {
                    if in_data {
                        if line == ".\r\n" {
                            in_data = false;
                            received.lock().unwrap().push(std::mem::take(&mut data));
                            let _ = w.write_all(b"250 queued\r\n");
                        } else {
                            data.push_str(&line);
                        }
                        continue;
                    }
                    let up = line.to_uppercase();
                    let reply: &[u8] = if up.starts_with("EHLO") || up.starts_with("HELO") {
                        b"250-fake\r\n250 AUTH PLAIN LOGIN\r\n"
                    } else if up.starts_with("AUTH") {
                        b"235 ok\r\n"
                    } else if up.starts_with("DATA") {
                        in_data = true;
                        b"354 go\r\n"
                    } else if up.starts_with("QUIT") {
                        let _ = w.write_all(b"221 bye\r\n");
                        return;
                    } else {
                        b"250 ok\r\n"
                    };
                    let _ = w.write_all(reply);
                }
            });
        }
    });
    port
}

/// A just-enough IMAP server over a shared inbox of `(raw message, seen)`.
type Inbox = Arc<Mutex<Vec<(Vec<u8>, bool)>>>;

fn fake_imap(inbox: Inbox) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let inbox = inbox.clone();
            std::thread::spawn(move || {
                let mut w = s.try_clone().unwrap();
                let mut r = BufReader::new(s);
                let _ = w.write_all(b"* OK fake IMAP ready\r\n");
                let mut line = String::new();
                while {
                    line.clear();
                    r.read_line(&mut line).unwrap_or(0) > 0
                } {
                    let mut parts = line.trim_end().splitn(3, ' ');
                    let (tag, cmd, rest) = (parts.next().unwrap_or(""), parts.next().unwrap_or("").to_uppercase(), parts.next().unwrap_or(""));
                    let ids = |set: &str| -> Vec<usize> { set.split(',').filter_map(|x| x.trim().parse().ok()).collect() };
                    match cmd.as_str() {
                        "LOGIN" => {
                            let _ = w.write_all(format!("{} OK logged in\r\n", tag).as_bytes());
                        }
                        "SELECT" => {
                            let n = inbox.lock().unwrap().len();
                            let _ = w.write_all(format!("* {} EXISTS\r\n* 0 RECENT\r\n* FLAGS (\\Seen)\r\n{} OK [READ-WRITE] done\r\n", n, tag).as_bytes());
                        }
                        "SEARCH" => {
                            let list = inbox.lock().unwrap().iter().enumerate().filter(|(_, (_, seen))| !seen).map(|(i, _)| (i + 1).to_string()).collect::<Vec<_>>().join(" ");
                            let _ = w.write_all(format!("* SEARCH {}\r\n{} OK done\r\n", list, tag).as_bytes());
                        }
                        "FETCH" => {
                            let (set, what) = rest.split_once(' ').unwrap_or((rest, ""));
                            for i in ids(set) {
                                let m = inbox.lock().unwrap()[i - 1].0.clone();
                                if what.contains("RFC822.SIZE") {
                                    let _ = w.write_all(format!("* {} FETCH (RFC822.SIZE {})\r\n", i, m.len()).as_bytes());
                                } else {
                                    let _ = w.write_all(format!("* {} FETCH (BODY[] {{{}}}\r\n", i, m.len()).as_bytes());
                                    let _ = w.write_all(&m);
                                    let _ = w.write_all(b")\r\n");
                                }
                            }
                            let _ = w.write_all(format!("{} OK done\r\n", tag).as_bytes());
                        }
                        "STORE" => {
                            let (set, _) = rest.split_once(' ').unwrap_or((rest, ""));
                            for i in ids(set) {
                                inbox.lock().unwrap()[i - 1].1 = true;
                            }
                            let _ = w.write_all(format!("{} OK stored\r\n", tag).as_bytes());
                        }
                        "LOGOUT" => {
                            let _ = w.write_all(format!("* BYE\r\n{} OK bye\r\n", tag).as_bytes());
                            return;
                        }
                        _ => {
                            let _ = w.write_all(format!("{} BAD unknown\r\n", tag).as_bytes());
                        }
                    }
                }
            });
        }
    });
    port
}

fn relay_block(smtp: u16, imap: u16) -> String {
    format!(
        r#"
[relay]
channel = "email"
poll_secs = 5
[relay.email]
mailbox = "delphi.relay@example.net"
operator = "me@example.com"
imap_host = "127.0.0.1"
imap_port = {imap}
smtp_host = "127.0.0.1"
smtp_port = {smtp}
tls = "none"
credential = "env:DELPHI_E2E_MAIL_PW"
authserv_id = "mx.example.net"
"#
    )
}

fn reply_mail(token: &str, id: &str, auth: &str) -> Vec<u8> {
    format!("From: Me <me@example.com>\r\nTo: delphi.relay@example.net\r\nSubject: Re: card\r\nMessage-ID: <{}@example.com>\r\nAuthentication-Results: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nAPPROVE {}\r\n\r\nOn Mon, Delphi wrote:\r\n> stuff\r\n", id, auth, token).into_bytes()
}

#[test]
fn a_card_goes_out_by_email_a_forged_reply_is_ignored_and_the_real_one_approves_it() {
    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));
    let (smtp, imap) = (fake_smtp(sent.clone()), fake_imap(inbox.clone()));
    let m = mock(vec![reply_tool("w1", "write_file", json!({"path": "specs/009-relay.md", "content": "# Relay\nbody"})), reply_text("Drafted specs/009-relay.md.")]);
    let e = setup(m.port);
    let cfg = std::fs::read_to_string(e.ws.join("config.toml")).unwrap();
    std::fs::write(e.ws.join("config.toml"), format!("{}{}", cfg, relay_block(smtp, imap))).unwrap();
    ratify_and_stop(&e);

    // A headless run raises the card and leaves it for the operator.
    let run = Command::new(DELPHI)
        .args(["run", "@architect Draft a spec called 009-relay", "--on-card", "relay", "--json", "--timeout", "120s", "--workspace", "e2e", "--home", e.home.to_str().unwrap()])
        .env("DELPHI_E2E_KEY", KEY)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // The relay runs in a terminal, as it must (the daemon gives a relay token only to a peer with one).
    let mut relay = PtyConsole::start_env(&e.home, &["relay", "--workspace", "e2e"], 24, 120, &[("DELPHI_E2E_MAIL_PW", "app-password")]);
    relay.wait_for("delphi relay: ready");

    // The card arrives by email, with a token and no secret payload.
    let deadline = Instant::now() + Duration::from_secs(40);
    let card_mail = loop {
        if let Some(m) = sent.lock().unwrap().iter().find(|m| m.contains("[delphi:ap_")).cloned() {
            break m;
        }
        assert!(Instant::now() < deadline, "no card email arrived; relay shows:\n{}", relay.everything());
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(card_mail.contains("To: me@example.com") && card_mail.contains("write specs/009-relay.md") && card_mail.contains("Auto-Submitted: auto-generated"), "{}", card_mail);
    assert!(!card_mail.contains("# Relay"), "the file's content stays home by default");
    let token = card_mail.split("APPROVE ").nth(1).unwrap().chars().take(32).collect::<String>();
    assert_eq!(token.len(), 32);

    // A forgery: the right address, the right token, but the provider says the sender did not authenticate.
    inbox.lock().unwrap().push((reply_mail(&token, "forged", "mx.example.net; dmarc=fail; spf=fail"), false));
    // The operator's real reply.
    inbox.lock().unwrap().push((reply_mail(&token, "real", "mx.example.net; dkim=pass header.d=example.com; dmarc=pass"), false));

    let out = run.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(0), "stdout:\n{}\nstderr:\n{}\nrelay:\n{}", stdout, String::from_utf8_lossy(&out.stderr), relay.everything());
    let s = summary_of(&stdout);
    assert_eq!((s["cards"]["allowed"].as_u64(), s["cards"]["left"].as_u64()), (Some(1), Some(0)), "{}", s);
    assert!(s["cards"]["list"][0]["reason"].as_str().unwrap().contains("relay:email:me@example.com"), "the log names the channel and sender: {}", s);
    assert!(e.ws.join("team/specs/009-relay.md").exists(), "the emailed approval ran the write");

    // The forgery was ignored (the operator was told once that a message claiming to be them failed authentication),
    // and the real reply used the code; both were marked read. Had the forgery been accepted, the real reply would
    // have found a used code and the notice would not exist.
    assert!(inbox.lock().unwrap().iter().all(|(_, seen)| *seen));
    let sent_now = sent.lock().unwrap().clone();
    assert_eq!(sent_now.iter().filter(|m| m.contains("failed authentication")).count(), 1, "{:#?}", sent_now);
    assert!(!sent_now.iter().any(|m| m.contains("that code is not valid")), "the real reply found its code unused: the forgery did not take it");
    let _ = relay.child.kill();
}

#[test]
fn an_unratified_relay_block_is_ignored_so_a_local_edit_cannot_add_an_approver() {
    let m = mock(vec![]);
    let e = setup(m.port);
    let cfg = std::fs::read_to_string(e.ws.join("config.toml")).unwrap();
    std::fs::write(e.ws.join("config.toml"), format!("{}{}", cfg, relay_block(1, 1))).unwrap();
    // Nothing is ratified: the relay must refuse to start rather than trust the edited file.
    let o = Command::new(DELPHI).args(["relay", "check", "--workspace", "e2e", "--home", e.home.to_str().unwrap()]).env("DELPHI_E2E_MAIL_PW", "x").stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", String::from_utf8_lossy(&o.stderr));
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(err.contains("no [relay] table in the ratified config"), "{}", err);
}

// ── delphi hooks: an observe hook runs when a task completes ────────────────

fn hooks_block(log: &Path) -> String {
    format!(
        r#"
[[hooks.observe]]
name = "log-done"
events = ["task.completed"]
command = ["/bin/sh", "-c", "cat >> {log}; echo >> {log}"]
"#,
        log = log.display()
    )
}

#[test]
fn an_observe_hook_runs_when_a_task_completes_and_only_for_the_events_it_asked_for() {
    let m = mock(vec![reply_text("Hello there.")]);
    let e = setup(m.port);
    let log = e._t.path().join("hook.log");
    let cfg = std::fs::read_to_string(e.ws.join("config.toml")).unwrap();
    std::fs::write(e.ws.join("config.toml"), format!("{}{}", cfg, hooks_block(&log))).unwrap();

    // A console keeps the daemon alive; the hooks process attaches to it from a terminal of its own.
    let mut s = Session::start(&e.home, &["chat", "--workspace", "e2e"]);
    s.wait_for("/help for commands");
    s.send("/ratify");
    s.wait_for("pending proposal");
    s.send("/ratify apply 1");
    s.wait_for("ratified ws:config.toml#access");
    let hooks = PtyConsole::start(&e.home, &["hooks", "--workspace", "e2e"], 24, 120);
    hooks.wait_for("delphi hooks: ready (1 hook(s): log-done)");
    std::thread::sleep(Duration::from_millis(500));

    s.send("Say hello");
    s.wait_for("✓ t_0001 done");
    let deadline = Instant::now() + Duration::from_secs(20);
    let seen = loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.contains("task.completed") {
            break text;
        }
        assert!(Instant::now() < deadline, "the hook never ran; hooks shows:\n{}", hooks.everything());
        std::thread::sleep(Duration::from_millis(100));
    };
    let line = seen.lines().find(|l| l.contains("task.completed")).unwrap();
    let ev: Value = serde_json::from_str(line).unwrap();
    assert_eq!((ev["type"].as_str(), ev["task_id"].as_str(), ev["payload"]["status"].as_str()), (Some("task.completed"), Some("t_0001"), Some("done")));
    assert_eq!(seen.matches("task.completed").count(), 1, "one completion, one run; other events did not trigger it:\n{}", seen);
    let (code, out) = s.finish();
    assert_eq!(code, 0, "{}", out);
    kill_daemon(pid_in(&out));
}

#[test]
fn hooks_that_are_not_ratified_do_not_run_and_the_command_says_so() {
    let m = mock(vec![]);
    let e = setup(m.port);
    let log = e._t.path().join("hook.log");
    let cfg = std::fs::read_to_string(e.ws.join("config.toml")).unwrap();
    std::fs::write(e.ws.join("config.toml"), format!("{}{}", cfg, hooks_block(&log))).unwrap();
    let o = Command::new(DELPHI).args(["hooks", "--workspace", "e2e", "--home", e.home.to_str().unwrap()]).stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no observe hooks in the ratified [hooks] config"), "an edit to the file alone adds no hook: {}", String::from_utf8_lossy(&o.stderr));
    assert!(!log.exists());
}
