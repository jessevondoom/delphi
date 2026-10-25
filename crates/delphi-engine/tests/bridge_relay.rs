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
