//! The Claude CLI as a model path (`specs/009` §7, §7B): the operator's own `claude` process, driven over
//! stream-json, with its tool-permission requests routed back to Delphi.
//!
//! This module is the process and protocol layer only: start the CLI, turn its stdout into [`Event`]s,
//! answer permission requests, meter tokens, and make sure nothing outlives the session. The decisions
//! (what to allow, taint, budgets, events) belong to the engine (`harness_run.rs`).
//!
//! Wire format verified by the 2026-10-24 spike (`tools/claude_bridge_spike.py`, spec 009 §7B): the
//! bridge exists only with `--permission-prompt-tool stdio`; a `control_request` of subtype
//! `can_use_tool` is answered with a `control_response` carrying `behavior` allow or deny.
//! `permission_suggestions` are never echoed back: they would edit the operator's settings.

use delphi_sandbox::{plain_command, Limits};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::mpsc;

/// Which built-in tools the CLI is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tools {
    /// None: the role gets its context in the prompt and returns text (non-builder roles, Council seats).
    None,
    /// The CLI's usual set; every permission request still goes through Delphi.
    Standard,
}

#[derive(Debug, Clone)]
pub struct Launch {
    /// `claude`, or a path (tests use a stand-in script).
    pub program: String,
    pub model: String,
    pub system_prompt: String,
    pub user_prompt: String,
    pub cwd: PathBuf,
    pub tools: Tools,
    pub env: BTreeMap<String, String>,
}

/// A tool call the CLI wants to make.
#[derive(Debug, Clone, PartialEq)]
pub struct Permission {
    pub request_id: String,
    pub tool_name: String,
    pub input: Value,
    pub tool_use_id: Option<String>,
    pub blocked_path: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}

impl Tokens {
    /// What the subscription cap counts: everything except cache reads, which are nearly free and repeat
    /// every turn, so counting them would make any cap meaningless.
    pub fn metered(&self) -> u64 {
        self.input + self.cache_write + self.output
    }

    fn of(u: &Value) -> Tokens {
        let n = |k: &str| u[k].as_u64().unwrap_or(0);
        Tokens { input: n("input_tokens"), cache_read: n("cache_read_input_tokens"), cache_write: n("cache_creation_input_tokens"), output: n("output_tokens") }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Init { session_id: String },
    Text(String),
    /// The CLI's own tool use, for the log.
    ToolUse { id: String, name: String, input: Value },
    ToolResult { id: String, is_error: bool },
    Permission(Permission),
    /// One assistant message's usage (the sum of these is the live meter).
    Usage(Tokens),
    /// Share of the subscription's window used, 0 to 1.
    RateLimit { utilization: f64, window: String },
    Result { text: String, is_error: bool, stop_reason: Option<String>, total_cost_usd: f64, usage: Tokens },
    /// The process ended; no more events follow.
    Exit { code: Option<i32> },
}

/// Turn one line of CLI output into events. Unknown or malformed lines yield nothing.
pub fn parse_line(line: &str) -> Vec<Event> {
    let Ok(m) = serde_json::from_str::<Value>(line) else { return vec![] };
    let mut out = Vec::new();
    match m["type"].as_str() {
        Some("system") if m["subtype"] == "init" => out.push(Event::Init { session_id: m["session_id"].as_str().unwrap_or("").into() }),
        Some("assistant") => {
            for b in m["message"]["content"].as_array().into_iter().flatten() {
                match b["type"].as_str() {
                    Some("text") => out.push(Event::Text(b["text"].as_str().unwrap_or("").into())),
                    Some("tool_use") => out.push(Event::ToolUse { id: b["id"].as_str().unwrap_or("").into(), name: b["name"].as_str().unwrap_or("").into(), input: b["input"].clone() }),
                    _ => {}
                }
            }
            if m["message"]["usage"].is_object() {
                out.push(Event::Usage(Tokens::of(&m["message"]["usage"])));
            }
        }
        Some("user") => {
            for b in m["message"]["content"].as_array().into_iter().flatten() {
                if b["type"] == "tool_result" {
                    out.push(Event::ToolResult { id: b["tool_use_id"].as_str().unwrap_or("").into(), is_error: b["is_error"].as_bool().unwrap_or(false) });
                }
            }
        }
        Some("rate_limit_event") => {
            let i = &m["rate_limit_info"];
            if let Some(u) = i["utilization"].as_f64() {
                out.push(Event::RateLimit { utilization: u, window: i["rateLimitType"].as_str().unwrap_or("").into() });
            }
        }
        Some("control_request") if m["request"]["subtype"] == "can_use_tool" => {
            let r = &m["request"];
            out.push(Event::Permission(Permission {
                request_id: m["request_id"].as_str().unwrap_or("").into(),
                tool_name: r["tool_name"].as_str().unwrap_or("").into(),
                input: r["input"].clone(),
                tool_use_id: r["tool_use_id"].as_str().map(String::from),
                blocked_path: r["blocked_path"].as_str().map(String::from),
                description: r["description"].as_str().map(String::from),
            }));
        }
        Some("result") => out.push(Event::Result {
            text: m["result"].as_str().unwrap_or("").into(),
            is_error: m["is_error"].as_bool().unwrap_or(false) || m["subtype"].as_str().is_some_and(|s| s != "success"),
            stop_reason: m["stop_reason"].as_str().map(String::from),
            total_cost_usd: m["total_cost_usd"].as_f64().unwrap_or(0.0),
            usage: Tokens::of(&m["usage"]),
        }),
        _ => {}
    }
    out
}

/// The `control_response` line for a decision. Never carries `updatedPermissions`.
pub fn response_line(p: &Permission, d: &Decision) -> String {
    let body = match d {
        Decision::Allow => json!({"behavior": "allow", "updatedInput": p.input}),
        Decision::Deny(why) => json!({"behavior": "deny", "message": why}),
    };
    json!({"type": "control_response", "response": {"subtype": "success", "request_id": p.request_id, "response": body}}).to_string()
}

fn args(l: &Launch) -> Vec<String> {
    let mut a: Vec<String> = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--permission-prompt-tool", "stdio", "--permission-mode", "manual", "--no-session-persistence", "--setting-sources", "", "--strict-mcp-config"].iter().map(|s| s.to_string()).collect();
    a.extend(["--model".into(), l.model.clone(), "--append-system-prompt".into(), l.system_prompt.clone()]);
    if l.tools == Tools::None {
        a.extend(["--tools".into(), String::new()]);
    }
    a
}

/// A running CLI session.
pub struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: mpsc::Receiver<Event>,
    pgid: i32,
    exited: bool,
}

impl Session {
    /// Start the CLI and send the first user turn.
    pub async fn start(l: &Launch) -> Result<Session, String> {
        let std_cmd = plain_command(Limits { cpu_secs: None, max_file_bytes: None, max_open_files: Some(4096) }, &l.env, &l.program, args(l));
        let mut cmd = tokio::process::Command::from(std_cmd);
        cmd.current_dir(&l.cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| format!("cannot start `{}`: {}", l.program, e))?;
        let pgid = child.id().unwrap_or(0) as i32;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let first = json!({"type": "user", "message": {"role": "user", "content": l.user_prompt}}).to_string() + "\n";
        stdin.write_all(first.as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            // A single line is capped; a longer one is dropped rather than buffered without bound.
            while let Ok(Some(line)) = lines.next_line().await {
                if line.len() > 8 << 20 {
                    continue;
                }
                for ev in parse_line(&line) {
                    if tx.send(ev).await.is_err() {
                        return;
                    }
                }
            }
        });
        Ok(Session { child, stdin: Some(stdin), rx, pgid, exited: false })
    }

    /// The next event; after the process ends, one [`Event::Exit`] and then `None`.
    pub async fn next(&mut self, idle: Duration) -> Option<Event> {
        if self.exited {
            return None;
        }
        match tokio::time::timeout(idle, self.rx.recv()).await {
            Ok(Some(ev)) => Some(ev),
            Ok(None) => {
                self.exited = true;
                let code = self.child.wait().await.ok().and_then(|s| s.code());
                Some(Event::Exit { code })
            }
            Err(_) => {
                self.kill().await;
                self.exited = true;
                Some(Event::Exit { code: None })
            }
        }
    }

    pub async fn decide(&mut self, p: &Permission, d: &Decision) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("the session's input is closed")?;
        stdin.write_all((response_line(p, d) + "\n").as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())
    }

    /// Let the CLI finish (it exits once its input closes after the result).
    pub fn close_input(&mut self) {
        self.stdin = None;
    }

    /// Kill the CLI and everything it started.
    pub async fn kill(&mut self) {
        self.stdin = None;
        if self.pgid > 0 {
            // SAFETY: plain signal delivery to the process group the launcher created for this child.
            unsafe {
                libc::kill(-self.pgid, libc::SIGKILL);
            }
        }
        let _ = self.child.kill().await;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.pgid > 0 {
            // SAFETY: as in `kill`; nothing the session started may outlive it.
            unsafe {
                libc::kill(-self.pgid, libc::SIGKILL);
            }
        }
    }
}

/// An allowlisted environment for the CLI: enough to find its login and configuration, nothing else.
/// Notably absent: `ANTHROPIC_API_KEY` (it would switch the CLI off the subscription), `AWS_*`, tokens.
pub fn cli_env() -> BTreeMap<String, String> {
    const KEEP: [&str; 9] = ["PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "TERM", "TMPDIR", "XDG_CONFIG_HOME"];
    std::env::vars().filter(|(k, _)| KEEP.contains(&k.as_str()) || k.starts_with("LC_") || k == "CLAUDE_CONFIG_DIR").collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn real_cli_lines_become_events() {
        let init = parse_line(r#"{"type":"system","subtype":"init","session_id":"s1","cwd":"/x","tools":["Bash"]}"#);
        assert_eq!(init, vec![Event::Init { session_id: "s1".into() }]);
        let asst = parse_line(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"},{"type":"tool_use","id":"tu1","name":"Bash","input":{"command":"touch a"}}],"usage":{"input_tokens":4,"cache_creation_input_tokens":100,"cache_read_input_tokens":9000,"output_tokens":20}}}"#);
        assert_eq!(asst.len(), 3);
        assert_eq!(asst[0], Event::Text("hi".into()));
        assert!(matches!(&asst[1], Event::ToolUse { name, .. } if name == "Bash"));
        let Event::Usage(t) = &asst[2] else { panic!() };
        assert_eq!((t.metered(), t.cache_read), (124, 9000), "cache reads are not metered");
        assert_eq!(parse_line(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu1","is_error":true,"content":"x"}]}}"#), vec![Event::ToolResult { id: "tu1".into(), is_error: true }]);
        assert_eq!(parse_line(r#"{"type":"rate_limit_event","rate_limit_info":{"utilization":0.32,"rateLimitType":"seven_day"}}"#), vec![Event::RateLimit { utilization: 0.32, window: "seven_day".into() }]);
        let res = parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"done","stop_reason":"end_turn","total_cost_usd":0.02,"usage":{"input_tokens":4,"output_tokens":80}}"#);
        assert!(matches!(&res[0], Event::Result { text, is_error: false, total_cost_usd, .. } if text == "done" && *total_cost_usd == 0.02));
        let err = parse_line(r#"{"type":"result","subtype":"error_max_budget_usd","is_error":false,"result":""}"#);
        assert!(matches!(&err[0], Event::Result { is_error: true, .. }), "a non-success subtype is an error");
        assert!(parse_line("not json").is_empty() && parse_line(r#"{"type":"stream_event"}"#).is_empty());
    }

    #[test]
    fn permission_requests_parse_and_answers_never_carry_suggestions() {
        let line = r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"touch a"},"tool_use_id":"tu1","blocked_path":"/w/a","description":"d","permission_suggestions":[{"type":"addRules","rules":[]}]}}"#;
        let ev = parse_line(line);
        let Event::Permission(p) = &ev[0] else { panic!("{:?}", ev) };
        assert_eq!((p.request_id.as_str(), p.tool_name.as_str(), p.blocked_path.as_deref()), ("r1", "Bash", Some("/w/a")));
        let allow: Value = serde_json::from_str(&response_line(p, &Decision::Allow)).unwrap();
        assert_eq!(allow["response"]["request_id"], "r1");
        assert_eq!((allow["response"]["response"]["behavior"].as_str(), allow["response"]["response"]["updatedInput"]["command"].as_str()), (Some("allow"), Some("touch a")));
        let deny: Value = serde_json::from_str(&response_line(p, &Decision::Deny("no".into()))).unwrap();
        assert_eq!((deny["response"]["response"]["behavior"].as_str(), deny["response"]["response"]["message"].as_str()), (Some("deny"), Some("no")));
        for v in [&allow, &deny] {
            assert!(!v.to_string().contains("updatedPermissions") && !v.to_string().contains("addRules"));
        }
    }

    #[test]
    fn the_launch_arguments_isolate_the_cli_and_turn_the_bridge_on() {
        let l = Launch { program: "claude".into(), model: "sonnet".into(), system_prompt: "SYS".into(), user_prompt: "hi".into(), cwd: PathBuf::from("."), tools: Tools::None, env: BTreeMap::new() };
        let a = args(&l);
        let pos = |f: &str| a.iter().position(|x| x == f).unwrap();
        assert_eq!(a[pos("--permission-prompt-tool") + 1], "stdio");
        assert_eq!(a[pos("--setting-sources") + 1], "", "no user, project or local settings, so no user hooks");
        assert!(a.contains(&"--strict-mcp-config".to_string()) && a.contains(&"--no-session-persistence".to_string()));
        assert_eq!(a[pos("--tools") + 1], "");
        assert_eq!(a[pos("--model") + 1], "sonnet");
        assert!(!a.contains(&"--dangerously-skip-permissions".to_string()) && !a.contains(&"--bare".to_string()));
        let std = Launch { tools: Tools::Standard, ..l };
        assert!(!args(&std).contains(&"--tools".to_string()));
    }

    #[test]
    fn the_cli_environment_is_an_allowlist_without_api_keys() {
        // SAFETY: test-only; the variables must exist in this process for the assertion to mean anything.
        unsafe {
            std::env::set_var("ANTHROPIC_API_KEY", "sk-leak");
            std::env::set_var("AWS_ACCESS_KEY_ID", "AKIAleak");
        }
        let e = cli_env();
        assert!(e.contains_key("PATH") && e.contains_key("HOME"));
        assert!(!e.contains_key("ANTHROPIC_API_KEY") && !e.keys().any(|k| k.starts_with("AWS_")));
    }

    /// A stand-in `claude`: prints scripted lines, and for each `control_request` waits for the answer.
    pub(crate) fn fake_cli(dir: &std::path::Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("fake-claude");
        std::fs::write(&p, format!("#!/bin/sh\nread first\n{}\n", body)).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    }

    fn launch(program: String, dir: &std::path::Path) -> Launch {
        Launch { program, model: "sonnet".into(), system_prompt: "s".into(), user_prompt: "do it".into(), cwd: dir.to_path_buf(), tools: Tools::Standard, env: cli_env() }
    }

    #[tokio::test]
    async fn a_session_streams_events_answers_permissions_and_ends() {
        let d = tempfile::tempdir().unwrap();
        let prog = fake_cli(
            d.path(),
            r#"echo '{"type":"system","subtype":"init","session_id":"s9"}'
echo '{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"touch a"},"tool_use_id":"tu1"}}'
read answer
case "$answer" in *'"behavior":"allow"'*) echo '{"type":"assistant","message":{"content":[{"type":"text","text":"allowed"}]}}';; *) echo '{"type":"assistant","message":{"content":[{"type":"text","text":"denied"}]}}';; esac
echo '{"type":"result","subtype":"success","result":"fin","total_cost_usd":0.01,"usage":{"input_tokens":1,"output_tokens":2}}'"#,
        );
        for (decision, want) in [(Decision::Allow, "allowed"), (Decision::Deny("no".into()), "denied")] {
            let mut s = Session::start(&launch(prog.clone(), d.path())).await.unwrap();
            let mut seen = Vec::new();
            while let Some(ev) = s.next(Duration::from_secs(10)).await {
                match &ev {
                    Event::Permission(p) => s.decide(p, &decision).await.unwrap(),
                    Event::Exit { .. } => {
                        seen.push(ev);
                        break;
                    }
                    _ => {}
                }
                if matches!(ev, Event::Result { .. }) {
                    s.close_input();
                }
                seen.push(ev);
            }
            assert!(seen.iter().any(|e| matches!(e, Event::Init { session_id } if session_id == "s9")));
            assert!(seen.iter().any(|e| matches!(e, Event::Text(t) if t == want)), "{:?}", seen);
            assert!(seen.iter().any(|e| matches!(e, Event::Result { text, .. } if text == "fin")));
            assert!(matches!(seen.last(), Some(Event::Exit { code: Some(0) })));
        }
    }

    #[tokio::test]
    async fn an_idle_cli_is_killed_with_everything_it_started() {
        let d = tempfile::tempdir().unwrap();
        let pidfile = d.path().join("pid");
        let prog = fake_cli(d.path(), &format!("sleep 60 &\necho $! > {}\nsleep 60", pidfile.display()));
        let mut s = Session::start(&launch(prog, d.path())).await.unwrap();
        let ev = s.next(Duration::from_millis(500)).await;
        assert!(matches!(ev, Some(Event::Exit { code: None })), "{:?}", ev);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let pid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the background process must be gone");
        assert!(s.next(Duration::from_millis(50)).await.is_none());
    }

    /// Against the real CLI (costs a few cents of subscription quota; run with `--ignored` after
    /// `claude` is signed in): the driver, the isolation flags and the bridge, end to end.
    #[tokio::test]
    #[ignore]
    async fn live_claude_round_trip_with_the_approval_bridge() {
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let l = Launch { program: "claude".into(), model: "sonnet".into(), system_prompt: "You are a test fixture. Be brief.".into(), user_prompt: "Use the Bash tool to run exactly: touch live.txt   Then reply with the single word done.".into(), cwd: dir.clone(), tools: Tools::Standard, env: cli_env() };
        let mut s = Session::start(&l).await.unwrap();
        let (mut asked, mut text, mut result, mut metered) = (0, String::new(), false, 0);
        while let Some(ev) = s.next(Duration::from_secs(90)).await {
            match ev {
                Event::Permission(p) => {
                    asked += 1;
                    assert_eq!(p.tool_name, "Bash");
                    s.decide(&p, &Decision::Allow).await.unwrap();
                }
                Event::Text(t) => text.push_str(&t),
                Event::Usage(t) => metered += t.metered(),
                Event::Result { is_error, .. } => {
                    assert!(!is_error);
                    result = true;
                    s.close_input();
                }
                Event::Exit { .. } => break,
                _ => {}
            }
        }
        assert!(result && asked == 1 && text.to_lowercase().contains("done") && metered > 0, "asked={} text={} metered={}", asked, text, metered);
        assert!(dir.join("live.txt").exists(), "the approved command ran");
    }
}
