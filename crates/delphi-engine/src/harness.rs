//! The Claude CLI as a model path (`specs/009` §7, §7B): the operator's own `claude` process, driven over
//! stream-json, with its tool-permission requests routed back to Delphi.
//!
//! This module is the process and protocol layer only: start the CLI, turn its stdout into [`Event`]s,
//! answer permission requests, meter tokens, and make sure nothing outlives the session. The decisions
//! (what to allow, taint, budgets, events) belong to the engine (`harness_run.rs`).
//!
//! Wire format verified by the 2026-10-24 spike (`dev/claude_bridge_spike.py`, spec 009 §7B): the
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

/// Which subscription CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cli {
    /// `claude` (Claude Code): stream-json with a permission bridge.
    Claude,
    /// `agy` (the Antigravity CLI, the Gemini subscription's CLI now that the Gemini CLI refuses personal
    /// sign-in): stream-json events, no permission bridge, tools cannot be turned off.
    Agy,
}

impl Cli {
    pub fn from_harness(name: &str) -> Option<Cli> {
        match name {
            "claude" => Some(Cli::Claude),
            "agy" => Some(Cli::Agy),
            _ => None,
        }
    }

    /// The program name used when none is configured.
    pub fn program(self) -> &'static str {
        match self {
            Cli::Claude => "claude",
            Cli::Agy => "agy",
        }
    }
}

/// Which built-in tools the CLI is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tools {
    /// None: the role gets its context in the prompt and returns text (non-builder roles, Council seats).
    None,
    /// The CLI's usual set; every permission request still goes through Delphi.
    Standard,
}

/// What a profile (`[harness.<cli>]`, `specs/020`) changes about how the Claude CLI is started.
#[derive(Debug, Clone, Default)]
pub struct LaunchProfile {
    /// The MCP config file that starts the Delphi tool bridge (`bridge.rs`).
    pub mcp_config: Option<PathBuf>,
    /// The CLI's built-in tools given to a span with `Tools::None`. Empty means none.
    pub native_tools: Vec<String>,
    /// Start in the CLI's auto permission mode, so its own policy and classifier answer first.
    pub auto_approve: bool,
    /// Do not pass `--strict-mcp-config`: the signed-in account's own connectors load too.
    pub claudeai_connectors: bool,
    /// With account connectors on: how long to wait, before the first turn, for them to finish loading. They load in the
    /// background and can arrive after the first model request, so a model asked at once may not be offered them. Zero
    /// does not wait (tests, and a CLI that does not answer).
    pub settle: Duration,
    /// MCP servers a previous session of this CLI had connected, which the wait holds out for until its cap.
    pub expect_servers: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Launch {
    pub cli: Cli,
    /// The program name or a path (tests use a stand-in script).
    pub program: String,
    pub model: String,
    pub system_prompt: String,
    pub user_prompt: String,
    pub cwd: PathBuf,
    pub tools: Tools,
    pub env: BTreeMap<String, String>,
    pub profile: LaunchProfile,
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
    /// `tools` are the names the CLI will offer the model; `mcp_servers` are `(name, status)`.
    Init { session_id: String, tools: Vec<String>, mcp_servers: Option<Vec<(String, String)>> },
    Text(String),
    /// The CLI's own tool use, for the log.
    ToolUse { id: String, name: String, input: Value },
    ToolResult { id: String, is_error: bool },
    Permission(Permission),
    /// One assistant message's usage (the sum of these is the live meter).
    Usage(Tokens),
    /// Share of the subscription's window used, 0 to 1.
    /// `status` is `allowed`, `allowed_warning` or `rejected` (empty when the CLI does not say); `resets_at` is Unix seconds or milliseconds, as the CLI sent it.
    RateLimit { utilization: f64, window: String, status: String, resets_at: Option<i64> },
    /// The answer to Delphi's own `mcp_status` request: `(server, status)`. Only the connector wait asks.
    McpStatus(Vec<(String, String)>),
    Result { text: String, is_error: bool, stop_reason: Option<String>, total_cost_usd: f64, usage: Tokens },
    /// The process ended; no more events follow.
    Exit { code: Option<i32> },
}

/// Turn one line of CLI output into events. Unknown or malformed lines yield nothing.
pub fn parse_line(line: &str) -> Vec<Event> {
    let Ok(m) = serde_json::from_str::<Value>(line) else { return vec![] };
    let mut out = Vec::new();
    match m["type"].as_str() {
        Some("system") if m["subtype"] == "init" => out.push(Event::Init {
            session_id: m["session_id"].as_str().unwrap_or("").into(),
            tools: names_of(&m["tools"]),
            mcp_servers: m["mcp_servers"].as_array().map(|a| a.iter().map(|s| (s["name"].as_str().unwrap_or("").to_string(), s["status"].as_str().unwrap_or("").to_string())).collect()),
        }),
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
        Some("control_response") if m["response"]["request_id"].as_str().is_some_and(|id| id.starts_with(MCP_STATUS_ID)) => {
            let servers = m["response"]["response"]["mcpServers"].as_array().into_iter().flatten().map(|s| (s["name"].as_str().unwrap_or("").to_string(), s["status"].as_str().unwrap_or("").to_string())).collect();
            out.push(Event::McpStatus(servers));
        }
        Some("rate_limit_event") => {
            let i = &m["rate_limit_info"];
            let status = i["status"].as_str().unwrap_or("");
            if i["utilization"].is_number() || !status.is_empty() {
                let resets_at = ["resetsAt", "resets_at"].iter().find_map(|k| i[*k].as_i64().or_else(|| i[*k].as_f64().map(|f| f as i64)));
                out.push(Event::RateLimit { utilization: i["utilization"].as_f64().unwrap_or(0.0), window: i["rateLimitType"].as_str().unwrap_or("").into(), status: status.into(), resets_at });
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

/// The id prefix of Delphi's own `mcp_status` requests, so their answers are told from the CLI's other replies.
const MCP_STATUS_ID: &str = "delphi-mcp-";

/// Tool names from a list that holds either strings or objects with a `name`.
fn names_of(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|t| t.as_str().or_else(|| t["name"].as_str()).map(String::from)).collect()
}

/// `agy` output. Events: `init`, `step_update` (user input, agent response deltas, tool steps) and `result`.
pub fn parse_agy_line(line: &str) -> Vec<Event> {
    let Ok(m) = serde_json::from_str::<Value>(line) else { return vec![] };
    let usage_of = |u: &Value| Tokens { input: u["input_tokens"].as_u64().unwrap_or(0), cache_read: u["cache_read_tokens"].as_u64().unwrap_or(0), cache_write: 0, output: u["output_tokens"].as_u64().unwrap_or(0) + u["thinking_tokens"].as_u64().unwrap_or(0) };
    let mut out = Vec::new();
    match m["event"].as_str() {
        Some("init") => out.push(Event::Init { session_id: String::new(), tools: names_of(&m["init"]["tools"]), mcp_servers: None }),
        Some("step_update") => {
            let s = &m["step_update"];
            let id = s["step_index"].as_u64().unwrap_or(0).to_string();
            match (s["step_type"].as_str(), s["state"].as_str()) {
                (Some("agent_response"), _) => {
                    if let Some(t) = s["text_delta"].as_str().filter(|t| !t.is_empty()) {
                        out.push(Event::Text(t.into()));
                    }
                    if s["state"] == "DONE" && s["usage"].is_object() {
                        out.push(Event::Usage(usage_of(&s["usage"])));
                    }
                }
                (Some("tool"), Some("ACTIVE")) => out.push(Event::ToolUse { id, name: s["tool_name"].as_str().unwrap_or("").into(), input: s["tool_info"]["parameters"].clone() }),
                // Only a finished step is a result. A step that is still pending or running says nothing yet, and a
                // refused or cancelled one is a failure, not a completion.
                (Some("tool"), Some("DONE" | "COMPLETED" | "SUCCESS")) => out.push(Event::ToolResult { id, is_error: false }),
                (Some("tool"), Some("ERROR" | "FAILED" | "CANCELLED" | "CANCELED" | "DENIED" | "REJECTED")) => out.push(Event::ToolResult { id, is_error: true }),
                _ => {}
            }
        }
        Some("result") => {
            let r = &m["result"];
            let denied = r["denied_actions"].as_array().map(|a| a.len()).unwrap_or(0);
            let is_error = r["status"].as_str().is_some_and(|s| s != "SUCCESS");
            // A failed run has an empty `response`; the reason (a spent quota, its reset time) is in `error`.
            let response = r["response"].as_str().unwrap_or("");
            let text = if is_error && response.is_empty() { r["error"].as_str().unwrap_or("") } else { response };
            out.push(Event::Result {
                text: text.into(),
                is_error,
                stop_reason: (denied > 0).then(|| format!("{} action(s) denied by the CLI", denied)),
                total_cost_usd: 0.0,
                usage: usage_of(&r["usage"]),
            });
        }
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

/// The longest line of CLI output that is kept.
const MAX_LINE: usize = 8 << 20;

/// The first user turn, as the CLI's input format wants it.
fn first_turn(l: &Launch) -> String {
    match l.cli {
        Cli::Claude => json!({"type": "user", "message": {"role": "user", "content": l.user_prompt}}),
        // `agy` has no system-prompt flag: the governance and role come first in the user turn. It cannot turn
        // its tools off either, so a role that should have none is told so (a request, not an enforcement: the
        // engine treats every agy span as untrusted and checks the projects afterwards).
        Cli::Agy => {
            let no_tools = if l.tools == Tools::None { "Do not call any tools. Answer in text only.\n\n" } else { "" };
            json!({"event": "user", "message": {"content": format!("{}\n\n{}{}", l.system_prompt, no_tools, l.user_prompt)}})
        }
    }
    .to_string()
        + "\n"
}

/// Read one line into `buf` (without its newline), keeping at most `cap` bytes. `Some(true)` is a line within the cap;
/// `Some(false)` is a longer one, which has been read to its end and discarded; `None` is the end of the stream.
async fn read_capped_line<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R, cap: usize, buf: &mut Vec<u8>) -> std::io::Result<Option<bool>> {
    buf.clear();
    let (mut over, mut any) = (false, false);
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(any.then_some(!over));
        }
        any = true;
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i, true),
            None => (chunk.len(), false),
        };
        if !over {
            if buf.len() + take > cap {
                over = true;
                buf.clear();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        r.consume(take + usize::from(done));
        if done {
            return Ok(Some(!over));
        }
    }
}

fn args(l: &Launch) -> Vec<String> {
    if l.cli == Cli::Agy {
        // `--print=` takes the prompt as its value; with stream input the prompt arrives on stdin instead.
        return ["--input-format", "stream-json", "--output-format", "stream-json", "--print=", "--model", l.model.as_str()].iter().map(|s| s.to_string()).collect();
    }
    let p = &l.profile;
    let mode = if p.auto_approve { "auto" } else { "manual" };
    let mut a: Vec<String> = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--permission-prompt-tool", "stdio", "--permission-mode", mode, "--no-session-persistence", "--setting-sources", ""].iter().map(|s| s.to_string()).collect();
    // By default every MCP server but ours stays out (condition 5 of `specs/009` §7). A profile that lets the account's
    // own connectors in drops the flag; their calls then reach Delphi's permission bridge like any other tool.
    if !p.claudeai_connectors {
        a.push("--strict-mcp-config".into());
    }
    if let Some(cfg) = &p.mcp_config {
        a.extend(["--mcp-config".into(), cfg.display().to_string()]);
    }
    a.extend(["--model".into(), l.model.clone(), "--append-system-prompt".into(), l.system_prompt.clone()]);
    if l.tools == Tools::None {
        a.extend(["--tools".into(), p.native_tools.join(",")]);
    }
    a
}

/// Wait for the account's connectors to finish loading: ask for the MCP status until nothing is pending, every server
/// a previous session had is connected, and three answers in a row agree; or until `cap`. A CLI that does not answer a
/// status request (a stand-in, or an older release) is not waited on.
async fn settle_connectors(stdin: &mut ChildStdin, rx: &mut mpsc::Receiver<Event>, cap: Duration, expect: &[String]) {
    let deadline = tokio::time::Instant::now() + cap;
    let (mut last, mut agree, mut n): (Option<Vec<(String, String)>>, u32, u32) = (None, 0, 0);
    while tokio::time::Instant::now() < deadline {
        n += 1;
        let req = json!({"type": "control_request", "request_id": format!("{}{}", MCP_STATUS_ID, n), "request": {"subtype": "mcp_status"}}).to_string() + "\n";
        if stdin.write_all(req.as_bytes()).await.is_err() || stdin.flush().await.is_err() {
            return;
        }
        let answer = loop {
            match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
                Ok(Some(Event::McpStatus(s))) => break Some(s),
                Ok(Some(_)) => continue,
                _ => break None,
            }
        };
        let Some(mut servers) = answer else { return };
        servers.sort();
        let ready = servers.iter().all(|(_, st)| st != "pending") && expect.iter().all(|e| servers.iter().any(|(n, st)| n == e && st == "connected"));
        agree = if ready && last.as_ref() == Some(&servers) { agree + 1 } else { 0 };
        last = Some(servers);
        if ready && agree >= 2 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    }
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
        let first = first_turn(l);
        let cli = l.cli;
        let (tx, mut rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            // A single line is capped while it is read; a longer one is dropped rather than buffered. A line that
            // is not UTF-8 is decoded lossily (it then fails to parse and yields nothing) and never ends the reader.
            while let Ok(Some(complete)) = read_capped_line(&mut reader, MAX_LINE, &mut buf).await {
                if !complete {
                    continue;
                }
                let line = String::from_utf8_lossy(&buf);
                for ev in if cli == Cli::Agy { parse_agy_line(&line) } else { parse_line(&line) } {
                    if tx.send(ev).await.is_err() {
                        return;
                    }
                }
            }
        });
        // The CLI says nothing until it has a first turn, but it answers control requests: settle the connectors first.
        if cli == Cli::Claude && l.profile.claudeai_connectors && !l.profile.settle.is_zero() {
            settle_connectors(&mut stdin, &mut rx, l.profile.settle, &l.profile.expect_servers).await;
        }
        stdin.write_all(first.as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;
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
                // The output is closed; a process that stays alive anyway is killed rather than waited on forever.
                let code = match tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
                    Ok(r) => r.ok().and_then(|s| s.code()),
                    Err(_) => {
                        self.kill().await;
                        None
                    }
                };
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
        // Only the Claude stream carries permission requests, so only it has answers to send.
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
    cli_env_with(&[])
}

/// [`cli_env`] plus the variable names a ratified `[harness.<cli>] env_allow` lists (`specs/020` §3).
pub fn cli_env_with(extra: &[String]) -> BTreeMap<String, String> {
    const KEEP: [&str; 9] = ["PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "TERM", "TMPDIR", "XDG_CONFIG_HOME"];
    std::env::vars().filter(|(k, _)| KEEP.contains(&k.as_str()) || k.starts_with("LC_") || k == "CLAUDE_CONFIG_DIR" || extra.iter().any(|e| e == k)).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn real_cli_lines_become_events() {
        let init = parse_line(r#"{"type":"system","subtype":"init","session_id":"s1","cwd":"/x","tools":["Bash"]}"#);
        assert_eq!(init, vec![Event::Init { session_id: "s1".into(), tools: vec!["Bash".into()], mcp_servers: None }]);
        let asst = parse_line(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"},{"type":"tool_use","id":"tu1","name":"Bash","input":{"command":"touch a"}}],"usage":{"input_tokens":4,"cache_creation_input_tokens":100,"cache_read_input_tokens":9000,"output_tokens":20}}}"#);
        assert_eq!(asst.len(), 3);
        assert_eq!(asst[0], Event::Text("hi".into()));
        assert!(matches!(&asst[1], Event::ToolUse { name, .. } if name == "Bash"));
        let Event::Usage(t) = &asst[2] else { panic!() };
        assert_eq!((t.metered(), t.cache_read), (124, 9000), "cache reads are not metered");
        assert_eq!(parse_line(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu1","is_error":true,"content":"x"}]}}"#), vec![Event::ToolResult { id: "tu1".into(), is_error: true }]);
        assert_eq!(parse_line(r#"{"type":"rate_limit_event","rate_limit_info":{"utilization":0.32,"rateLimitType":"seven_day"}}"#), vec![Event::RateLimit { utilization: 0.32, window: "seven_day".into(), status: String::new(), resets_at: None }]);
        assert_eq!(parse_line(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour","resetsAt":1790003600}}"#), vec![Event::RateLimit { utilization: 0.0, window: "five_hour".into(), status: "rejected".into(), resets_at: Some(1_790_003_600) }]);
        let res = parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"done","stop_reason":"end_turn","total_cost_usd":0.02,"usage":{"input_tokens":4,"output_tokens":80}}"#);
        assert!(matches!(&res[0], Event::Result { text, is_error: false, total_cost_usd, .. } if text == "done" && *total_cost_usd == 0.02));
        // `claude` 2.1.310 with the session limit reached (a captured sample, `tests/fixtures/quota/`): the event has no
        // top-level `utilization`, only `status` and the reset time, and the result is an error with a 429.
        assert_eq!(
            parse_line(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1792864800,"rateLimitType":"five_hour","overageStatus":"rejected","overageDisabledReason":"org_level_disabled","isUsingOverage":false,"unifiedWindows":{"five_hour":{"utilization":1,"resetsAt":1792864800,"status":"rejected","surpassedThreshold":1},"seven_day":{"utilization":0.62,"resetsAt":1793365200,"status":"allowed"}}},"uuid":"u","session_id":"s"}"#),
            vec![Event::RateLimit { utilization: 0.0, window: "five_hour".into(), status: "rejected".into(), resets_at: Some(1_792_864_800) }]
        );
        let res = parse_line(r#"{"type":"result","subtype":"success","is_error":true,"api_error_status":429,"api_error":"usage_limit_reached","result":"You've hit your session limit · resets 2pm (America/New_York)","stop_reason":"stop_sequence","total_cost_usd":0,"usage":{"input_tokens":0,"output_tokens":0},"duration_ms":268}"#);
        assert!(matches!(&res[0], Event::Result { text, is_error: true, .. } if text.starts_with("You've hit your session limit")), "{:?}", res);
        // `agy` with the quota used up: the reason is in `error`, not `response` (a captured sample).
        let res = parse_agy_line(r#"{"event":"result","result":{"status":"ERROR","response":"","error":"Individual quota reached. Resets in 136h12m54s.","usage":{"input_tokens":0,"output_tokens":0}}}"#);
        assert!(matches!(&res[0], Event::Result { text, is_error: true, .. } if text == "Individual quota reached. Resets in 136h12m54s."), "{:?}", res);
        let err = parse_line(r#"{"type":"result","subtype":"error_max_budget_usd","is_error":false,"result":""}"#);
        assert!(matches!(&err[0], Event::Result { is_error: true, .. }), "a non-success subtype is an error");
        assert!(parse_line("not json").is_empty() && parse_line(r#"{"type":"stream_event"}"#).is_empty());
    }

    #[test]
    fn agy_lines_become_the_same_events_and_its_args_use_stdin_for_the_prompt() {
        assert_eq!(parse_agy_line(r#"{"event":"init","init":{"model":"m","tools":[]}}"#), vec![Event::Init { session_id: String::new(), tools: vec![], mcp_servers: None }]);
        let delta = parse_agy_line(r#"{"event":"step_update","step_update":{"step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"po"}}"#);
        assert_eq!(delta, vec![Event::Text("po".into())]);
        let done = parse_agy_line(r#"{"event":"step_update","step_update":{"step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"\n","usage":{"input_tokens":6801,"output_tokens":1,"thinking_tokens":3,"cache_read_tokens":10}}}"#);
        assert!(matches!(&done[1], Event::Usage(t) if t.metered() == 6805 && t.cache_read == 10), "{:?}", done);
        let tool = parse_agy_line(r#"{"event":"step_update","step_update":{"step_index":2,"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"parameters":{"CommandLine":"ls"}}}}"#);
        assert!(matches!(&tool[0], Event::ToolUse { name, input, .. } if name == "run_command" && input["CommandLine"] == "ls"));
        let err = parse_agy_line(r#"{"event":"step_update","step_update":{"step_index":2,"state":"ERROR","step_type":"tool","tool_name":"run_command"}}"#);
        assert_eq!(err, vec![Event::ToolResult { id: "2".into(), is_error: true }]);
        let res = parse_agy_line(r#"{"event":"result","result":{"status":"SUCCESS","response":"pong\n","usage":{"input_tokens":6,"output_tokens":1},"denied_actions":[{"action":"command"}]}}"#);
        assert!(matches!(&res[0], Event::Result { text, is_error: false, stop_reason: Some(r), .. } if text == "pong\n" && r.contains("denied")), "{:?}", res);
        let bad = parse_agy_line(r#"{"event":"result","result":{"status":"ERROR","error":"x"}}"#);
        assert!(matches!(&bad[0], Event::Result { is_error: true, .. }));
        let l = Launch { cli: Cli::Agy, program: "agy".into(), model: "gemini-3.8-flash-low".into(), system_prompt: "SYS".into(), user_prompt: "hi".into(), cwd: PathBuf::from("."), tools: Tools::None, env: BTreeMap::new(), profile: Default::default() };
        let a = args(&l);
        assert_eq!(a, ["--input-format", "stream-json", "--output-format", "stream-json", "--print=", "--model", "gemini-3.8-flash-low"]);
        assert!(!a.iter().any(|x| x.contains("dangerously")), "never the bypass flag");
    }

    #[test]
    fn agy_tool_steps_that_are_not_finished_are_not_reported_as_completed() {
        let step = |state: &str| parse_agy_line(&format!(r#"{{"event":"step_update","step_update":{{"step_index":3,"state":"{}","step_type":"tool","tool_name":"run_command"}}}}"#, state));
        assert_eq!(step("DONE"), vec![Event::ToolResult { id: "3".into(), is_error: false }]);
        for failed in ["ERROR", "CANCELLED", "DENIED"] {
            assert_eq!(step(failed), vec![Event::ToolResult { id: "3".into(), is_error: true }], "{}", failed);
        }
        for pending in ["PENDING", "WAITING", "SOMETHING_NEW"] {
            assert!(step(pending).is_empty(), "{} is not a result", pending);
        }
    }

    #[test]
    fn agy_with_no_tools_is_told_so_in_its_first_turn() {
        let mk = |tools| Launch { cli: Cli::Agy, program: "agy".into(), model: "m".into(), system_prompt: "SYS".into(), user_prompt: "hi".into(), cwd: PathBuf::from("."), tools, env: BTreeMap::new(), profile: Default::default() };
        assert!(first_turn(&mk(Tools::None)).contains("Do not call any tools"));
        assert!(!first_turn(&mk(Tools::Standard)).contains("Do not call any tools"));
    }

    #[tokio::test]
    async fn long_lines_are_dropped_while_read_and_the_next_line_still_arrives() {
        let mut data = vec![b'x'; 100];
        data.extend_from_slice(b"\nshort\n\nlast");
        let mut r = tokio::io::BufReader::with_capacity(16, &data[..]);
        let mut buf = Vec::new();
        assert_eq!(read_capped_line(&mut r, 10, &mut buf).await.unwrap(), Some(false));
        assert!(buf.is_empty(), "nothing over the cap is kept");
        assert_eq!(read_capped_line(&mut r, 10, &mut buf).await.unwrap(), Some(true));
        assert_eq!(buf, b"short");
        assert_eq!(read_capped_line(&mut r, 10, &mut buf).await.unwrap(), Some(true));
        assert!(buf.is_empty());
        assert_eq!(read_capped_line(&mut r, 10, &mut buf).await.unwrap(), Some(true));
        assert_eq!(buf, b"last");
        assert_eq!(read_capped_line(&mut r, 10, &mut buf).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_line_that_is_not_utf8_does_not_end_the_session() {
        let d = tempfile::tempdir().unwrap();
        let prog = fake_cli(d.path(), "printf '\\377\\376 bad\\n'\necho '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"after\",\"usage\":{}}'");
        let mut s = Session::start(&launch(prog, d.path())).await.unwrap();
        let mut got = false;
        while let Some(ev) = s.next(Duration::from_secs(10)).await {
            got |= matches!(&ev, Event::Result { text, .. } if text == "after");
            if matches!(ev, Event::Exit { .. }) {
                break;
            }
        }
        assert!(got, "the line after the bad one was read");
    }

    #[tokio::test]
    async fn a_cli_that_closes_its_output_but_stays_alive_is_killed_not_waited_for() {
        let d = tempfile::tempdir().unwrap();
        let prog = fake_cli(d.path(), "exec 1>&-\nsleep 60");
        let mut s = Session::start(&launch(prog, d.path())).await.unwrap();
        let t = std::time::Instant::now();
        assert!(matches!(s.next(Duration::from_secs(30)).await, Some(Event::Exit { .. })));
        assert!(t.elapsed() < Duration::from_secs(15));
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
        let l = Launch { cli: Cli::Claude, program: "claude".into(), model: "sonnet".into(), system_prompt: "SYS".into(), user_prompt: "hi".into(), cwd: PathBuf::from("."), tools: Tools::None, env: BTreeMap::new(), profile: Default::default() };
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
        Launch { cli: Cli::Claude, program, model: "sonnet".into(), system_prompt: "s".into(), user_prompt: "do it".into(), cwd: dir.to_path_buf(), tools: Tools::Standard, env: cli_env(), profile: Default::default() }
    }

    #[tokio::test]
    async fn the_first_turn_waits_until_the_accounts_connectors_have_stopped_loading() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let log = d.path().join("log.txt");
        // A stand-in that answers `mcp_status` (two pending answers, then connected) and notes what arrived when.
        let prog = d.path().join("fake-claude-slow");
        std::fs::write(
            &prog,
            format!(
                r#"#!/bin/sh
n=0
while read line; do
  case "$line" in
    *mcp_status*)
      n=$((n+1))
      id=$(echo "$line" | sed 's/.*"request_id":"\([^"]*\)".*/\1/')
      if [ $n -lt 3 ]; then st=pending; else st=connected; fi
      echo "status $n $st" >> '{log}'
      echo "{{\"type\":\"control_response\",\"response\":{{\"subtype\":\"success\",\"request_id\":\"$id\",\"response\":{{\"mcpServers\":[{{\"name\":\"claude.ai Docs\",\"status\":\"$st\"}}]}}}}}}";;
    *'"type":"user"'*)
      echo "first turn after $n status answers" >> '{log}'
      echo '{{"type":"system","subtype":"init","session_id":"s1","tools":[],"mcp_servers":[{{"name":"claude.ai Docs","status":"connected"}}]}}'
      echo '{{"type":"result","subtype":"success","result":"fin","usage":{{}}}}'
      exit 0;;
  esac
done
"#,
                log = log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut l = launch(prog.display().to_string(), d.path());
        l.tools = Tools::None;
        l.profile = LaunchProfile { claudeai_connectors: true, settle: Duration::from_secs(20), ..Default::default() };
        let mut s = Session::start(&l).await.unwrap();
        let mut saw_init = false;
        while let Some(ev) = s.next(Duration::from_secs(10)).await {
            match ev {
                Event::Init { mcp_servers, .. } => saw_init = mcp_servers.is_some_and(|m| m == vec![("claude.ai Docs".to_string(), "connected".to_string())]),
                Event::Exit { .. } => break,
                _ => {}
            }
        }
        assert!(saw_init);
        let log = std::fs::read_to_string(&log).unwrap();
        // pending, pending, connected, then two more equal answers; only then the first turn.
        assert!(log.contains("status 5 connected") && log.trim_end().ends_with("first turn after 5 status answers"), "{}", log);

        // Without account connectors, or with no wait configured, the first turn goes out at once.
        let d2 = tempfile::tempdir().unwrap();
        let quick = fake_cli(d2.path(), r#"echo '{"type":"system","subtype":"init","session_id":"s2"}'
echo '{"type":"result","subtype":"success","result":"fin","usage":{}}'"#);
        for profile in [LaunchProfile { claudeai_connectors: false, settle: Duration::from_secs(20), ..Default::default() }, LaunchProfile { claudeai_connectors: true, settle: Duration::ZERO, ..Default::default() }] {
            let mut l = launch(quick.clone(), d2.path());
            l.profile = profile;
            let started = std::time::Instant::now();
            let mut s = Session::start(&l).await.unwrap();
            while let Some(ev) = s.next(Duration::from_secs(10)).await {
                if matches!(ev, Event::Exit { .. }) {
                    break;
                }
            }
            assert!(started.elapsed() < Duration::from_secs(5), "no wait: {:?}", started.elapsed());
        }
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
            assert!(seen.iter().any(|e| matches!(e, Event::Init { session_id, .. } if session_id == "s9")));
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
        let ev = s.next(Duration::from_secs(3)).await;
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
        let l = Launch { cli: Cli::Claude, program: "claude".into(), model: "sonnet".into(), system_prompt: "You are a test fixture. Be brief.".into(), user_prompt: "Use the Bash tool to run exactly: touch live.txt   Then reply with the single word done.".into(), cwd: dir.clone(), tools: Tools::Standard, env: cli_env(), profile: Default::default() };
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

    /// Against the real `agy` (a few thousand tokens of the Gemini subscription; run with `--ignored`).
    #[tokio::test]
    #[ignore]
    async fn live_agy_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let l = Launch { cli: Cli::Agy, program: "agy".into(), model: "gemini-3.8-flash-low".into(), system_prompt: "You are a test fixture. Be brief.".into(), user_prompt: "Reply with the single word pong.".into(), cwd: std::fs::canonicalize(d.path()).unwrap(), tools: Tools::None, env: cli_env(), profile: Default::default() };
        let mut s = Session::start(&l).await.unwrap();
        let (mut text, mut result, mut metered) = (String::new(), false, 0);
        while let Some(ev) = s.next(Duration::from_secs(60)).await {
            match ev {
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
        assert!(result && text.to_lowercase().contains("pong") && metered > 0, "text={:?} metered={}", text, metered);
    }
}
