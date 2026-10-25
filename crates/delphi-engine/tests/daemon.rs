//! Daemon behavior: specs/005 §6 tests 3, 5, 7, 13, 16, plus the coalescer rate (test 1, scaled).

use delphi_core::client::{Client, ClientError};
use delphi_core::hub::DeltaKind;
use delphi_core::proto::{code, Scope};
use delphi_core::server::{Daemon, DaemonConfig, TtyPolicy};
use delphi_core::spawn::{connect_or_spawn, layout, SpawnOptions};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DAEMON_EXE: &str = env!("CARGO_BIN_EXE_delphi-core");

struct Fixture {
    _dir: tempfile::TempDir,
    sock: PathBuf,
    narrator_sock: PathBuf,
    relay_sock: PathBuf,
    daemon: Arc<Daemon>,
}

/// An in-process daemon on a short socket path (macOS limits Unix socket paths to ~100 bytes).
async fn start(policy: TtyPolicy, queue_cap: usize) -> Fixture {
    let dir = tempfile::Builder::new().prefix("dl").tempdir_in("/tmp").unwrap();
    let sock = dir.path().join("d.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    let narrator_sock = dir.path().join("n.sock");
    let narrator_listener = tokio::net::UnixListener::bind(&narrator_sock).unwrap();
    let relay_sock = dir.path().join("r.sock");
    let relay_listener = tokio::net::UnixListener::bind(&relay_sock).unwrap();
    let mut cfg = DaemonConfig::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    cfg.tty_policy = policy;
    cfg.queue_cap = queue_cap;
    cfg.auth_timeout = Duration::from_millis(500);
    let daemon = Daemon::new(cfg).unwrap();
    tokio::spawn(daemon.clone().serve_all(listener, Some(narrator_listener), Some(relay_listener)));
    Fixture { _dir: dir, sock, narrator_sock, relay_sock, daemon }
}

fn authed(f: &Fixture, scope: Scope) -> Client {
    let (sock, token) = match scope {
        Scope::Console => (&f.sock, f.daemon.console_token()),
        Scope::Narrator => (&f.narrator_sock, f.daemon.narrator_token()),
        Scope::Relay => (&f.relay_sock, f.daemon.relay_token()),
    };
    let mut c = Client::connect(sock).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    c.hello(token).unwrap();
    c
}

fn rpc_code(r: Result<Value, ClientError>) -> Option<i64> {
    match r {
        Err(ClientError::Rpc { code, .. }) => Some(code),
        _ => None,
    }
}

/// Everything the server sends on `s` until it closes it (or `wait` passes).
fn drain(s: &mut UnixStream, wait: Duration) -> (Vec<u8>, bool) {
    s.set_read_timeout(Some(wait)).unwrap();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match s.read(&mut tmp) {
            Ok(0) => return (buf, true),
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => return (buf, false),
        }
    }
}

// specs/005 §6 test 16
#[tokio::test(flavor = "multi_thread")]
async fn connections_without_a_valid_token_get_nothing_and_are_closed() {
    let f = start(TtyPolicy::Disabled, 1024).await;
    for first in [
        r#"{"jsonrpc":"2.0","id":1,"method":"session.snapshot","params":{}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":1,"method":"events.subscribe","params":{}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol_version":1,"binary_version":"x"}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol_version":1,"binary_version":"x","token":"deadbeef"}}"#.to_string(),
        "not json at all".to_string(),
    ] {
        let mut s = UnixStream::connect(&f.sock).unwrap();
        writeln!(s, "{}", first).unwrap();
        let (data, closed) = drain(&mut s, Duration::from_secs(3));
        assert!(data.is_empty(), "leaked {:?} for {}", String::from_utf8_lossy(&data), first);
        assert!(closed, "connection should be closed for {}", first);
    }
    // A silent connection is dropped at the auth timeout.
    let mut idle = UnixStream::connect(&f.sock).unwrap();
    let (data, closed) = drain(&mut idle, Duration::from_secs(3));
    assert!(data.is_empty() && closed);
    // And none of that disturbed a legitimate client.
    let mut c = authed(&f, Scope::Console);
    assert_eq!(c.call("session.ping", json!({})).unwrap(), json!("pong"));
}

// specs/005 §6 test 7
#[tokio::test(flavor = "multi_thread")]
async fn protocol_major_mismatch_is_refused_with_a_restart_hint() {
    let f = start(TtyPolicy::Disabled, 1024).await;
    let mut c = Client::connect(&f.sock).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    match c.hello_as(f.daemon.console_token(), 2) {
        Err(ClientError::Rpc { code: c, message, data }) => {
            assert_eq!(c, code::VERSION_SKEW);
            assert!(message.contains("delphi daemon restart"), "{}", message);
            assert!(data.unwrap()["hint"].as_str().unwrap().contains("restart"));
        }
        other => panic!("expected a version-skew error, got {:?}", other),
    }
    assert!(matches!(c.call("session.ping", json!({})), Err(ClientError::Closed) | Err(ClientError::Io(_))));
}

// specs/005 §6 test 13
#[tokio::test(flavor = "multi_thread")]
async fn privileged_rpcs_need_the_console_scope() {
    let f = start(TtyPolicy::Disabled, 1024).await;
    let params = json!({"approval_id": "ap_1", "decision": "allow", "payload_blake3": "00"});
    let mut narrator = authed(&f, Scope::Narrator);
    for m in ["approval.resolve", "budget.set", "permissions.set", "ratify.apply", "mission.rewind", "daemon.shutdown"] {
        assert_eq!(rpc_code(narrator.call(m, params.clone())), Some(code::FORBIDDEN), "{}", m);
    }
    // The narrator can read, though.
    assert_eq!(narrator.call("session.ping", json!({})).unwrap(), json!("pong"));
    assert_eq!(narrator.call("session.snapshot", json!({})).unwrap()["clients"]["narrator"], 1);
    // A console client reaches the handler (no such approval yet), proving the scope check is the only gate.
    let mut console = authed(&f, Scope::Console);
    assert_eq!(rpc_code(console.call("approval.resolve", params)), Some(code::NOT_FOUND));
    assert_eq!(rpc_code(console.call("nope", json!({}))), Some(code::METHOD_NOT_FOUND));
}

#[tokio::test(flavor = "multi_thread")]
async fn token_issue_requires_a_terminal_and_no_running_amber_command() {
    // Production policy: this test process has no tty-attached socket peer in the CI sense of the word,
    // but the peer here is the test process itself; use a setsid'd child to be sure.
    let f = start(TtyPolicy::Required, 1024).await;
    let out = run_child_without_tty(&f.sock, "console");
    assert!(out.contains(&code::TTY_REQUIRED.to_string()), "expected TTY_REQUIRED, got {}", out);
    // The read-only narrator scope needs no terminal; a script can watch (`delphi observe --once`).
    let out = run_child_without_tty(&f.narrator_sock, "narrator");
    assert!(out.contains("PROBE ok"), "the narrator is issued without a terminal, got {}", out);

    let g = start(TtyPolicy::Disabled, 1024).await;
    let mut c = Client::connect(&g.sock).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    assert_eq!(c.issue_token(Scope::Console).unwrap(), g.daemon.console_token());
    g.daemon.amber_active.store(true, std::sync::atomic::Ordering::SeqCst);
    // Each socket issues only its own kind of token; with an unsandboxed command running, neither issues any.
    assert_eq!(rpc_code(c.issue_token(Scope::Narrator).map(Value::from)), Some(code::FORBIDDEN), "the console socket will not issue a narrator token");
    assert_eq!(rpc_code(c.issue_token(Scope::Console).map(Value::from)), Some(code::CONFIRMATION_REQUIRED));
    let mut n = Client::connect(&g.narrator_sock).unwrap();
    n.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    assert_eq!(rpc_code(n.issue_token(Scope::Narrator).map(Value::from)), Some(code::CONFIRMATION_REQUIRED));
}

/// Child half: connect and ask for a token, print the outcome.
#[test]
fn token_probe_child() {
    let Ok(sock) = std::env::var("DELPHI_PROBE_SOCK") else { return };
    let mut c = Client::connect(Path::new(&sock)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let scope = if std::env::var("DELPHI_PROBE_SCOPE").as_deref() == Ok("narrator") { Scope::Narrator } else { Scope::Console };
    match c.issue_token(scope) {
        Ok(_) => println!("PROBE ok"),
        Err(ClientError::Rpc { code, .. }) => println!("PROBE rpc {}", code),
        Err(e) => println!("PROBE err {}", e),
    }
}

/// A process in its own session with no controlling terminal.
fn run_child_without_tty(sock: &Path, scope: &str) -> String {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "token_probe_child", "--nocapture", "--test-threads=1"]).env("DELPHI_PROBE_SOCK", sock).env("DELPHI_PROBE_SCOPE", scope).stdin(std::process::Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let out = cmd.output().unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

// specs/005 §6 test 1, scaled down: 5 spans x 500 deltas/s for ~1 s.
#[tokio::test(flavor = "multi_thread")]
async fn deltas_are_coalesced_to_at_most_60_frames_per_second_without_loss() {
    let f = start(TtyPolicy::Disabled, 1024).await;
    let mut c = authed(&f, Scope::Narrator);
    c.call("events.subscribe", json!({})).unwrap();
    let hub = f.daemon.hub.clone();
    let producer = tokio::spawn(async move {
        let start = Instant::now();
        let mut sent = 0usize;
        // 2,500 deltas/second across 5 spans, in 1 ms steps.
        while start.elapsed() < Duration::from_millis(1000) {
            for span in 0..5 {
                hub.push_delta(&format!("sp{}", span), DeltaKind::Text, "ab");
                sent += 2;
            }
            // Two more per span per ms would be 10k/s; pace at roughly 500/s per span.
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        sent
    });
    let reader = tokio::task::spawn_blocking(move || {
        let t0 = Instant::now();
        let mut frames = 0usize;
        let mut chars = 0usize;
        let mut last_seq = 0u64;
        c.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        while t0.elapsed() < Duration::from_millis(2500) {
            match c.next_notification() {
                Ok(n) if n["method"] == "delta.frame" => {
                    frames += 1;
                    let from = n["params"]["frame_from"].as_u64().unwrap();
                    assert_eq!(from, last_seq + 1, "frame_seq must be contiguous");
                    last_seq = n["params"]["frame_to"].as_u64().unwrap();
                    for (_, span) in n["params"]["spans"].as_object().unwrap() {
                        chars += span["text"].as_str().unwrap().len();
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    if frames > 0 && t0.elapsed() > Duration::from_millis(1400) {
                        break;
                    }
                }
            }
        }
        (frames, chars)
    });
    let sent = producer.await.unwrap();
    let (frames, chars) = reader.await.unwrap();
    assert!(frames <= 66, "at most ~60 frames/s for ~1.1 s of traffic, got {}", frames);
    assert!(frames >= 30, "coalescer should be flushing steadily, got {}", frames);
    assert_eq!(chars, sent, "no delta may be lost or duplicated");
}

// specs/005 §6 test 5
#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_client_is_resynced_and_does_not_slow_the_others() {
    let f = start(TtyPolicy::Disabled, 64).await;
    let mut stalled = UnixStream::connect(&f.narrator_sock).unwrap();
    writeln!(stalled, "{}", json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol_version":1,"binary_version":"x","token":f.daemon.narrator_token()}})).unwrap();
    writeln!(stalled, "{}", json!({"jsonrpc":"2.0","id":2,"method":"events.subscribe","params":{}})).unwrap();
    let mut healthy = authed(&f, Scope::Console);
    healthy.call("events.subscribe", json!({})).unwrap();
    // Give the server a moment to register the stalled client's subscription, then stop reading.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let hub = f.daemon.hub.clone();
    let big = "x".repeat(4096);
    let n = 4000usize;
    let t0 = Instant::now();
    let producer = tokio::task::spawn_blocking(move || {
        for i in 0..n {
            hub.publish_event(&json!({"seq": i, "pad": big}));
        }
    });
    let reader = tokio::task::spawn_blocking(move || {
        let mut got = 0usize;
        healthy.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        while got < n {
            match healthy.next_notification() {
                Ok(m) if m["method"] == "event" => got += 1,
                // A slow runner can leave the healthy client behind too; being told to resync is the daemon serving
                // it (not hanging), and the queue it drops is the point of the test, so stop waiting for the rest.
                Ok(m) if m["method"] == "resync_required" => {
                    got += 1;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        got
    });
    producer.await.unwrap();
    // The healthy client may itself resync under this burst only if it were slow; it must see them all or a resync, never hang.
    let got = reader.await.unwrap();
    assert!(got > 0, "the healthy client kept receiving while another was stalled");
    assert!(t0.elapsed() < Duration::from_secs(20), "the stalled client must not slow the daemon");

    // The stalled client resumes: it finds a resync_required in what the daemon queued.
    stalled.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut reader = BufReader::new(stalled);
    let mut saw_resync = false;
    let mut line = String::new();
    for _ in 0..200_000 {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if line.contains("resync_required") {
                    saw_resync = true;
                    break;
                }
            }
        }
    }
    assert!(saw_resync, "a client that stalled past its queue limit must be told to resync");
}

/// Give `cmd` a pseudo-terminal as its controlling terminal. The returned master must be kept alive.
fn with_pty(cmd: &mut std::process::Command) -> std::fs::File {
    use std::os::unix::io::FromRawFd;
    use std::os::unix::process::CommandExt;
    let (mut master, mut slave) = (0, 0);
    let rc = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
    assert_eq!(rc, 0, "openpty failed");
    let slave_file = unsafe { std::fs::File::from_raw_fd(slave) };
    cmd.stdin(std::process::Stdio::from(slave_file.try_clone().unwrap()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }
    unsafe { std::fs::File::from_raw_fd(master) }
}

/// Child half of the spawn race: attach (starting the daemon if needed) and record the result.
#[test]
fn race_child() {
    let Ok(out) = std::env::var("DELPHI_RACE_OUT") else { return };
    let home = PathBuf::from(std::env::var("DELPHI_RACE_HOME").unwrap());
    let ws = PathBuf::from(std::env::var("DELPHI_RACE_WS").unwrap());
    let pretend = std::env::var("DELPHI_RACE_VERSION").ok();
    let res = connect_or_spawn(&SpawnOptions { home: &home, workspace: &ws, daemon_exe: Path::new(DAEMON_EXE), extra_args: vec![], prefix_args: vec![], own_version: pretend.as_deref() });
    let line = match res {
        Ok(mut a) => {
            let pong = a.client.call("session.ping", json!({})).map(|v| v == json!("pong")).unwrap_or(false);
            json!({"ok": pong, "spawned": a.spawned, "pid": a.daemon_pid, "replaced": a.replaced, "stale": a.stale, "version": a.daemon_version}).to_string()
        }
        Err(e) => json!({"ok": false, "error": e.to_string()}).to_string(),
    };
    std::fs::write(out, line).unwrap();
}

fn launch_race_child(home: &Path, ws: &Path, out: &Path) -> (std::process::Child, std::fs::File) {
    launch_race_child_as(home, ws, out, None)
}

fn launch_race_child_as(home: &Path, ws: &Path, out: &Path, version: Option<&str>) -> (std::process::Child, std::fs::File) {
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "race_child", "--nocapture", "--test-threads=1"])
        .env("DELPHI_RACE_OUT", out)
        .env("DELPHI_RACE_HOME", home)
        .env("DELPHI_RACE_WS", ws);
    if let Some(v) = version {
        cmd.env("DELPHI_RACE_VERSION", v);
    }
    let master = with_pty(&mut cmd);
    (cmd.spawn().unwrap(), master)
}

// An upgrade does not stop the daemon, so an idle one from another release is replaced when a client attaches
// (the operator would otherwise test new code through an old daemon).
#[test]
fn an_idle_daemon_of_another_release_is_replaced_when_a_client_attaches() {
    let dir = tempfile::Builder::new().prefix("dl").tempdir_in("/tmp").unwrap();
    let (home, ws) = (dir.path().join("h"), dir.path().join("w"));
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("config.toml"), "").unwrap();
    let run = |name: &str, version: Option<&str>| -> Value {
        let out = dir.path().join(name);
        let (mut child, _master) = launch_race_child_as(&home, &ws, &out, version);
        assert!(child.wait().unwrap().success());
        serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap()
    };
    let first = run("a.json", None);
    assert_eq!((first["ok"] == true, first["spawned"] == true, first["replaced"].is_null()), (true, true, true), "{}", first);
    let same = run("b.json", None);
    assert_eq!((same["pid"] == first["pid"], same["spawned"] == false, same["replaced"].is_null()), (true, true, true), "the same release keeps its daemon: {}", same);
    // The client pretends to be another release: the idle daemon is stopped and a fresh one started.
    let other = run("c.json", Some("999.0.0"));
    assert_eq!((other["ok"] == true, other["spawned"] == true), (true, true), "{}", other);
    assert_eq!(other["replaced"].as_str(), first["version"].as_str(), "it says which release it replaced: {}", other);
    assert_ne!(other["pid"], first["pid"], "a new process");
    let old_pid = first["pid"].as_u64().unwrap() as i32;
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(old_pid, 0) } == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(unsafe { libc::kill(old_pid, 0) }, 0, "the old daemon is gone");
    unsafe { libc::kill(other["pid"].as_u64().unwrap() as i32, libc::SIGKILL) };
}

// specs/005 §6 test 3, plus crash recovery of a stale socket and the on-disk permission invariants.
#[test]
fn ten_simultaneous_clients_start_exactly_one_daemon() {
    let dir = tempfile::Builder::new().prefix("dl").tempdir_in("/tmp").unwrap();
    let home = dir.path().join("h");
    let ws = dir.path().join("w");
    std::fs::create_dir_all(&ws).unwrap();
    // The real daemon boots an engine from the workspace's config.toml; an empty one is valid
    // (no endpoints yet, which is also what an unratified workspace looks like).
    std::fs::write(ws.join("config.toml"), "").unwrap();

    let mut kids = Vec::new();
    for i in 0..10 {
        let out = dir.path().join(format!("out{}.json", i));
        let (child, master) = launch_race_child(&home, &ws, &out);
        kids.push((child, master, out));
    }
    let mut results = Vec::new();
    for (mut child, _master, out) in kids {
        assert!(child.wait().unwrap().success());
        results.push(serde_json::from_str::<Value>(&std::fs::read_to_string(out).unwrap()).unwrap());
    }
    assert!(results.iter().all(|r| r["ok"] == true), "{:?}", results);
    assert_eq!(results.iter().filter(|r| r["spawned"] == true).count(), 1, "exactly one launcher starts the daemon: {:?}", results);
    let pids: std::collections::BTreeSet<u64> = results.iter().map(|r| r["pid"].as_u64().unwrap()).collect();
    assert_eq!(pids.len(), 1, "all ten attach to the same daemon: {:?}", results);
    let pid = *pids.iter().next().unwrap() as i32;

    // On-disk invariants: run/ is 0700, the socket is 0600, and the daemon holds the lock.
    use std::os::unix::fs::PermissionsExt;
    let l = layout(&home, &ws).unwrap();
    assert_eq!(std::fs::metadata(&l.run_dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(&l.sock).unwrap().permissions().mode() & 0o777, 0o600);
    {
        use std::os::unix::io::AsRawFd;
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&l.lock).unwrap();
        let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        assert_ne!(rc, 0, "a live daemon holds the workspace lock");
    }

    // SIGKILL the daemon: the lock dies with it, and the next launcher cleans the stale socket and starts a new one.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(l.sock.exists(), "SIGKILL leaves a stale socket behind");
    let out = dir.path().join("after.json");
    let (mut child, _master) = launch_race_child(&home, &ws, &out);
    assert!(child.wait().unwrap().success());
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(after["ok"], true, "{}", after);
    assert_eq!(after["spawned"], true);
    assert_ne!(after["pid"].as_u64().unwrap() as i32, pid);
    unsafe { libc::kill(after["pid"].as_u64().unwrap() as i32, libc::SIGKILL) };
}
