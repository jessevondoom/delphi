//! The Delphi tool bridge's relay as a real process (`delphi-core __mcp-bridge`, the same code `delphi __mcp-bridge` runs):
//! what a subscription CLI starts as its MCP server. The server side is the library's own [`Bridge`]; the span loop is played
//! by the test.

use delphi_adapter::claude::ToolDef;
use delphi_engine::bridge::Bridge;
use delphi_engine::tools::ToolResult;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn defs() -> Vec<ToolDef> {
    vec![ToolDef { name: "timer".into(), description: "Wake me later.".into(), input_schema: json!({"type": "object"}) }]
}

/// Started the way a CLI starts it from the generated config: no arguments, the socket and token in the environment.
fn relay(socket: &std::path::Path, token: &str) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_delphi-core")).args(["__mcp-bridge"]).env("DELPHI_BRIDGE_SOCKET", socket).env("DELPHI_BRIDGE_TOKEN", token).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_relay_carries_a_cli_session_to_the_span_and_back() {
    let (bridge, mut calls) = Bridge::start(defs(), "notes".into(), std::path::Path::new(env!("CARGO_BIN_EXE_delphi-core"))).await.unwrap();
    // The span's side: answer the one call that comes.
    let span = tokio::spawn(async move {
        let call = calls.recv().await.expect("a call");
        assert_eq!((call.name.as_str(), call.args["note"].as_str(), call.call_id.as_str()), ("timer", Some("check CI"), "toolu_9"));
        call.reply.send(ToolResult { content: "timer tm_0001 set".into(), is_error: false }).unwrap();
    });
    let mut child = relay(&bridge.socket(), bridge.token());
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let (init, list, call) = tokio::task::spawn_blocking(move || {
        let mut ask = |req: Value| -> Value {
            writeln!(stdin, "{}", req).unwrap();
            stdin.flush().unwrap();
            let mut line = String::new();
            out.read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        };
        let init = ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}}));
        let list = ask(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
        let call = ask(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "timer", "arguments": {"note": "check CI"}, "_meta": {"claudecode/toolUseId": "toolu_9"}}}));
        (init, list, call) // `stdin` drops here: the CLI closing its end must end the relay

    })
    .await
    .unwrap();
    span.await.unwrap();
    assert_eq!(init["result"]["instructions"], "notes");
    assert_eq!(list["result"]["tools"][0]["name"], "timer");
    assert_eq!((call["result"]["content"][0]["text"].as_str(), call["result"]["isError"].as_bool()), (Some("timer tm_0001 set"), Some(false)));
    let status = tokio::task::spawn_blocking(move || child.wait().unwrap()).await.unwrap();
    assert!(status.success(), "the relay ends cleanly when its input closes");
}

/// The real `claude` against the real bridge and relay (about a tenth of a cent on haiku). `cargo test -p delphi-engine
/// --test bridge_relay -- --ignored --nocapture`. Both permission modes must reach the tool: in `manual` the CLI asks first
/// and Delphi allows; in `auto` it asks nothing, which is why the bridge runs the call when it arrives, not when it is approved.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "uses the operator's Claude login"]
async fn live_claude_calls_a_delphi_tool_through_the_bridge_in_both_permission_modes() {
    use delphi_engine::harness::{cli_env, Cli, Decision, Event, Launch, LaunchProfile, Session, Tools};
    // Strict and manual; strict and auto; and the shipped default: auto, the web tools, the account's own connectors loaded.
    for (auto, defaults) in [(false, false), (true, false), (true, true)] {
        let (bridge, mut calls) = Bridge::start(defs(), "Delphi's timer tool wakes you later.".into(), std::path::Path::new(env!("CARGO_BIN_EXE_delphi-core"))).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let l = Launch {
            cli: Cli::Claude,
            program: "claude".into(),
            model: "haiku".into(),
            system_prompt: "You are a test fixture. Be brief.".into(),
            user_prompt: "Call the timer tool once with note 'check CI' and then reply with the single word done.".into(),
            cwd: dir.path().to_path_buf(),
            tools: Tools::None,
            env: cli_env(),
            profile: LaunchProfile { mcp_config: Some(bridge.config_path().to_path_buf()), auto_approve: auto, claudeai_connectors: defaults, native_tools: if defaults { vec!["WebSearch".into(), "WebFetch".into()] } else { vec![] }, settle: if defaults { std::time::Duration::from_secs(10) } else { std::time::Duration::ZERO }, expect_servers: vec![] },
            opts: Default::default(),
        };
        let mut s = Session::start(&l).await.expect("claude starts");
        let (mut asked, mut served, mut connected, mut done) = (false, false, false, false);
        loop {
            // Serve a bridge call as the span loop would, between CLI events.
            let ev = tokio::select! {
                Some(call) = calls.recv() => {
                    assert_eq!((call.name.as_str(), call.args["note"].as_str()), ("timer", Some("check CI")));
                    served = true;
                    call.reply.send(ToolResult { content: "timer tm_0001 set. It is now 12:00.".into(), is_error: false }).unwrap();
                    continue;
                }
                ev = s.next(std::time::Duration::from_secs(120)) => match ev { Some(ev) => ev, None => break },
            };
            match ev {
                Event::Init { mcp_servers, tools, .. } => {
                    connected = mcp_servers.as_ref().is_some_and(|m| m.iter().any(|(n, st)| n == "delphi" && st == "connected"));
                    if defaults {
                        assert!(tools.contains(&"mcp__delphi__timer".to_string()) && tools.contains(&"WebSearch".to_string()), "{:?}", tools);
                    } else {
                        assert_eq!(tools, vec!["mcp__delphi__timer".to_string()], "only the bridge's tool is offered");
                    }
                    println!("  servers: {:?}", mcp_servers);
                }
                Event::Permission(p) => {
                    asked = true;
                    assert_eq!(p.tool_name, "mcp__delphi__timer");
                    s.decide(&p, &Decision::Allow).await.unwrap();
                }
                Event::Result { is_error, .. } => {
                    assert!(!is_error);
                    done = true;
                    s.close_input();
                }
                Event::Exit { .. } => break,
                _ => {}
            }
        }
        println!("auto={} defaults={} connected={} asked_permission={} served={} done={}", auto, defaults, connected, asked, served, done);
        assert!(connected && served && done, "auto={}", auto);
        assert_eq!(asked, !auto, "manual asks first; auto asks nothing");
    }
}

/// The real `agy` against Delphi's own pieces, with nothing in the operator's configuration touched: the bridge, the relay,
/// the `.agents/` the engine writes for a span ([`Bridge::write_agy_workspace`]) and the real `delphi __agy-hook` gate. The
/// plugin and the gate live in the span's working directory, which `agy` loads them from. Delphi's tool is allowed and
/// served; the shell is denied. `cargo test -p delphi-engine --test bridge_relay -- --ignored --nocapture live_agy`.
/// Flash, a few tenths of a cent.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "uses the operator's Antigravity login"]
async fn live_agy_calls_a_delphi_tool_through_a_workspace_plugin_while_a_hook_denies_its_other_tools() {
    use delphi_engine::bridge::{HookPolicy, AGY_TOOL_PREFIX};
    use delphi_engine::harness::{cli_env, Cli, Event, Launch, LaunchProfile, Session, Tools};
    let policy = HookPolicy { allow_prefixes: vec![AGY_TOOL_PREFIX.into()], allow_names: vec![] };
    let (bridge, mut calls, mut notes) = Bridge::start_gated(defs(), "Delphi's timer tool wakes you later.".into(), std::path::Path::new(env!("CARGO_BIN_EXE_delphi-core")), Some(policy)).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    bridge.write_agy_workspace(&root).unwrap();
    let l = Launch {
        cli: Cli::Agy,
        program: "agy".into(),
        model: "gemini-3.8-flash-low".into(),
        system_prompt: "You are a test fixture. Be brief. Delphi's tools are named mcp_delphi_delphi_<tool>; your own shell, file and scheduling tools are switched off.".into(),
        user_prompt: "Use the MCP tool named timer from the delphi server (not your built-in scheduler) once, with note 'check CI' and in_secs 90. Then run the shell command: echo leaked > proof.txt . Then say what each one returned.".into(),
        cwd: root.clone(),
        tools: Tools::None,
        env: cli_env(),
        // The engine marks a span that has a bridge this way; it is what keeps "do not call any tools" out of the first turn.
        profile: LaunchProfile { mcp_config: Some(bridge.config_path().to_path_buf()), ..Default::default() },
        opts: Default::default(),
    };
    let mut s = Session::start(&l).await.expect("agy starts");
    let (mut served, mut text) = (false, String::new());
    loop {
        let ev = tokio::select! {
            Some(call) = calls.recv() => {
                assert_eq!(call.name, "timer");
                served = true;
                call.reply.send(ToolResult { content: "timer tm_0001 set. It is now 12:00.".into(), is_error: false }).unwrap();
                continue;
            }
            ev = s.next(std::time::Duration::from_secs(120)) => match ev { Some(ev) => ev, None => break },
        };
        match ev {
            Event::Text(t) => text.push_str(&t),
            Event::Result { text: t, .. } => {
                if text.is_empty() {
                    text = t;
                }
                s.close_input();
            }
            Event::Exit { .. } => break,
            _ => {}
        }
    }
    let decisions: Vec<(String, bool)> = std::iter::from_fn(|| notes.try_recv().ok()).map(|n| (n.tool, n.allowed)).collect();
    println!("gate decided: {:?}\nagy said: {}", decisions, text.replace('\n', " "));
    assert!(served, "the Delphi tool reached the span through the plugin");
    assert!(decisions.iter().any(|(t, ok)| t.starts_with("mcp_delphi_") && *ok), "the gate allowed Delphi's tool: {:?}", decisions);
    assert!(decisions.iter().any(|(t, ok)| t == "run_command" && !*ok), "and denied the shell: {:?}", decisions);
    assert!(!root.join("proof.txt").exists(), "the shell command never ran");
}

/// Run `delphi __agy-hook` the way the Antigravity CLI does: the tool call as JSON on stdin, the gate's address in the
/// environment. Returns its exit code and what it printed, parsed.
fn run_hook(socket: Option<&std::path::Path>, token: Option<&str>, stdin: &str) -> (Option<i32>, Value) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_delphi-core"));
    cmd.arg("__agy-hook").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).env_remove("DELPHI_BRIDGE_SOCKET").env_remove("DELPHI_BRIDGE_TOKEN");
    if let Some(s) = socket {
        cmd.env("DELPHI_BRIDGE_SOCKET", s);
    }
    if let Some(t) = token {
        cmd.env("DELPHI_BRIDGE_TOKEN", t);
    }
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap_or_else(|_| json!({"unparsed": String::from_utf8_lossy(&out.stdout)})))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agy_hook_asks_the_gate_and_every_way_it_can_fail_is_a_denial_that_says_why() {
    use delphi_engine::bridge::{HookPolicy, AGY_TOOL_PREFIX};
    let policy = HookPolicy { allow_prefixes: vec![AGY_TOOL_PREFIX.into()], allow_names: vec!["search_web".into()] };
    let (bridge, _calls, _notes) = Bridge::start_gated(defs(), String::new(), std::path::Path::new(env!("CARGO_BIN_EXE_delphi-core")), Some(policy)).await.unwrap();
    let (socket, token) = (bridge.socket(), bridge.token().to_string());
    let call = |name: &str, args: Value| json!({"toolCall": {"name": name, "args": args}, "conversationId": "c", "stepIdx": 3}).to_string();
    let results = tokio::task::spawn_blocking(move || {
        vec![
            run_hook(Some(&socket), Some(&token), &call("mcp_delphi_delphi_timer", json!({"note": "x"}))),
            run_hook(Some(&socket), Some(&token), &call("search_web", json!({"query": "rust"}))),
            run_hook(Some(&socket), Some(&token), &call("run_command", json!({"CommandLine": "echo leaked > proof.txt"}))),
            run_hook(Some(&socket), Some(&token), "this is not json"),
            run_hook(Some(&socket), Some(&token), ""),
            run_hook(Some(&socket), Some("not-the-token"), &call("mcp_delphi_delphi_timer", json!({}))),
            run_hook(Some(std::path::Path::new("/nonexistent/b.sock")), Some(&token), &call("mcp_delphi_delphi_timer", json!({}))),
            run_hook(None, None, &call("mcp_delphi_delphi_timer", json!({}))),
        ]
    })
    .await
    .unwrap();
    let decisions: Vec<(Option<i32>, &str)> = results.iter().map(|(code, v)| (*code, v["decision"].as_str().unwrap_or("?"))).collect();
    assert_eq!(decisions, [(Some(0), "allow"), (Some(0), "allow"), (Some(0), "deny"), (Some(0), "deny"), (Some(0), "deny"), (Some(0), "deny"), (Some(0), "deny"), (Some(0), "deny")], "{:?}", results);
    // The model is told why, in words that point it at what it can use.
    assert!(results[2].1["reason"].as_str().unwrap().contains("run_command") && results[2].1["reason"].as_str().unwrap().contains("mcp_delphi_delphi_"), "{}", results[2].1);
    for (i, why) in [(3, "valid JSON"), (4, "valid JSON"), (5, "not understood"), (6, "could not be reached"), (7, "not configured")] {
        assert!(results[i].1["reason"].as_str().unwrap().starts_with("denied by Delphi:") && results[i].1["reason"].as_str().unwrap().contains(why), "case {}: {}", i, results[i].1);
    }
    // A bridge with no gate (a Claude span) never answers an allow, whoever asks.
    let (open, _rx) = Bridge::start(defs(), String::new(), std::path::Path::new("/x")).await.unwrap();
    let (socket, token) = (open.socket(), open.token().to_string());
    let (code, v) = tokio::task::spawn_blocking(move || run_hook(Some(&socket), Some(&token), &json!({"toolCall": {"name": "run_command"}}).to_string())).await.unwrap();
    assert_eq!((code, v["decision"].as_str()), (Some(0), Some("deny")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relay_with_the_wrong_token_gets_nothing_and_a_relay_without_one_refuses_to_start() {
    let (bridge, _calls) = Bridge::start(defs(), String::new(), std::path::Path::new("/x")).await.unwrap();
    let mut child = relay(&bridge.socket(), "not-the-token");
    let mut stdin = child.stdin.take().unwrap();
    let mut out = child.stdout.take().unwrap();
    let silent = tokio::task::spawn_blocking(move || {
        let _ = writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
        let mut s = String::new();
        std::io::Read::read_to_string(&mut out, &mut s).unwrap();
        s
    })
    .await
    .unwrap();
    assert!(silent.is_empty(), "no tool list for a stranger: {:?}", silent);
    assert!(tokio::task::spawn_blocking(move || child.wait().unwrap()).await.unwrap().success(), "the relay ends when the server closes on it");
    let no_token = Command::new(env!("CARGO_BIN_EXE_delphi-core")).args(["__mcp-bridge"]).env("DELPHI_BRIDGE_SOCKET", "/nonexistent").env_remove("DELPHI_BRIDGE_TOKEN").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
    assert_eq!(no_token.code(), Some(2));
}
