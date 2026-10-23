//! End to end: the real `delphi` console and `delphi-core` daemon against a mock Claude server.
//!
//! Covers the Step 2 flow as an operator would live it: initialize a workspace, ratify its endpoints from
//! the console, run a tool-using task, answer an approval card, read usage, and check that an unratified
//! edit to the endpoint config never redirects traffic.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");
const KEY: &str = "sk-e2e-key-0123456789abcdef";

fn ensure_daemon_binary() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let sibling = Path::new(DELPHI).with_file_name("delphi-core");
        if !sibling.exists() {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            assert!(Command::new(cargo).args(["build", "-p", "delphi-engine", "--bin", "delphi-core"]).current_dir(root).status().unwrap().success());
        }
        assert!(sibling.exists(), "delphi-core must sit next to delphi");
    });
}

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
    Env { _t: t, home, ws }
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
