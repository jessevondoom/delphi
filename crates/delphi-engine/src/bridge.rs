//! The Delphi tool bridge (`specs/018` §2): Delphi's own tools, offered to a subscription CLI as one MCP server.
//!
//! A harness span used to be text-only: the CLI ran its own loop and could not reach `timer`, `dispatch_agent`,
//! the scoped file tools or the connectors. For each span the daemon now listens on a unix socket in a private
//! directory, and the CLI starts `delphi __mcp-bridge` (a relay, [`run_relay`]) as its MCP server. The relay only
//! forwards lines: it holds no policy. Every `tools/call` becomes a [`BridgeCall`] handed to the span's own loop
//! (`harness_run.rs`), which runs it through the same `exec_tool` an API span uses, so classification, taint gates,
//! approvals and the log are one code path. The reply goes back to the CLI as the tool result.
//!
//! Why the call is run when it arrives, not when the CLI asks permission: in the CLI's auto permission mode a
//! tool call raises no permission request at all (verified 2026-10-25, CLI 2.1.310), so a design that waited for
//! the request would never run. Delphi's own approvals happen inside `exec_tool`, whatever the CLI's mode is.
//!
//! The socket directory is mode 0700 and the first line on every connection must carry the span's 256-bit token
//! (passed to the relay in its environment, never on a command line). A call for a tool the span was not offered
//! is refused here as well as in `exec_tool`.

use crate::tools::ToolResult;
use delphi_adapter::claude::ToolDef;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, Mutex};

/// The server name the CLI shows, and so the prefix of every tool name it uses: `mcp__delphi__<tool>`.
pub const SERVER_NAME: &str = "delphi";
pub const TOOL_PREFIX: &str = "mcp__delphi__";
/// The protocol revision offered when the client names none.
const DEFAULT_PROTOCOL: &str = "2025-11-25";
/// The longest request line the bridge reads.
const MAX_LINE: usize = 4 << 20;
/// A reply longer than this is cut, with a note, so one tool result cannot fill the CLI's context.
const MAX_REPLY_CHARS: usize = 400_000;

/// One tool call from the CLI, waiting for the span's loop to run it.
#[derive(Debug)]
pub struct BridgeCall {
    pub name: String,
    pub args: Value,
    /// The CLI's own id for the call (`_meta["claudecode/toolUseId"]`), for the log.
    pub call_id: String,
    pub reply: oneshot::Sender<ToolResult>,
}

/// A running bridge. Dropping it stops the server and removes its directory.
pub struct Bridge {
    dir: PathBuf,
    config_path: PathBuf,
    token: String,
    accept: tokio::task::JoinHandle<()>,
}

pub fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut b = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{:02x}", x)).collect())
}

/// Map a Delphi tool to MCP's shape.
fn mcp_tool(d: &ToolDef) -> Value {
    json!({"name": d.name, "description": d.description, "inputSchema": d.input_schema})
}

impl Bridge {
    /// Listen for one span. `tools` are the only names `tools/call` will accept; `instructions` is shown to the model
    /// by the CLI as the server's own usage notes. `program` is what the CLI runs as the relay (the `delphi` executable).
    pub async fn start(tools: Vec<ToolDef>, instructions: String, program: &Path) -> std::io::Result<(Bridge, mpsc::Receiver<BridgeCall>)> {
        // A unix socket path must fit in about 100 bytes, so the directory is short and sits in the temp dir.
        let mut base = std::env::temp_dir();
        if base.as_os_str().len() > 60 {
            base = PathBuf::from("/tmp");
        }
        let dir = base.join(format!("dlb-{}", random_hex(6)?));
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        }
        let socket = dir.join("b.sock");
        let listener = UnixListener::bind(&socket)?;
        let token = random_hex(32)?;
        let config_path = dir.join("mcp.json");
        let config = json!({"mcpServers": {SERVER_NAME: {"command": program.display().to_string(), "args": ["__mcp-bridge", "--socket", socket.display().to_string()], "env": {"DELPHI_BRIDGE_TOKEN": token}}}});
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&config_path)?;
            f.write_all(config.to_string().as_bytes())?;
        }
        let (tx, rx) = mpsc::channel::<BridgeCall>(16);
        let shared = Arc::new(Shared { tools: tools.iter().map(mcp_tool).collect(), names: tools.iter().map(|t| t.name.clone()).collect(), instructions, token: token.clone(), tx });
        let accept = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let shared = shared.clone();
                tokio::spawn(async move { serve(stream, shared).await });
            }
        });
        Ok((Bridge { dir, config_path, token, accept }, rx))
    }

    /// The MCP config file to hand the CLI (`--mcp-config`).
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// The span's token (tests connect with it).
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn socket(&self) -> PathBuf {
        self.dir.join("b.sock")
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.accept.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Shared {
    tools: Vec<Value>,
    names: Vec<String>,
    instructions: String,
    token: String,
    tx: mpsc::Sender<BridgeCall>,
}

fn same_token(a: &str, b: &str) -> bool {
    // Hash both so the comparison does not depend on where the strings first differ.
    blake3::hash(a.as_bytes()) == blake3::hash(b.as_bytes())
}

async fn serve(stream: UnixStream, shared: Arc<Shared>) {
    let (r, w) = stream.into_split();
    let writer = Arc::new(Mutex::new(w));
    let mut lines = BufReader::new(r);
    let mut buf = String::new();
    // The first line is the token and nothing else.
    match lines.read_line(&mut buf).await {
        Ok(n) if n > 0 && n < 256 && serde_json::from_str::<Value>(&buf).ok().and_then(|v| v["token"].as_str().map(|t| same_token(t, &shared.token))).unwrap_or(false) => {}
        _ => return,
    }
    loop {
        buf.clear();
        match lines.read_line(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) if n > MAX_LINE => return,
            Ok(_) => {}
        }
        let Ok(req) = serde_json::from_str::<Value>(&buf) else { continue };
        let (shared, writer) = (shared.clone(), writer.clone());
        // Each request on its own task: a slow tool call must not hold up a `ping` behind it.
        tokio::spawn(async move {
            if let Some(resp) = handle(&shared, &req).await {
                let mut line = resp.to_string();
                line.push('\n');
                let mut w = writer.lock().await;
                let _ = w.write_all(line.as_bytes()).await;
                let _ = w.flush().await;
            }
        });
    }
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// One JSON-RPC request. `None` for a notification, which has no answer.
async fn handle(shared: &Shared, req: &Value) -> Option<Value> {
    let id = req.get("id").filter(|v| !v.is_null())?.clone();
    let method = req["method"].as_str().unwrap_or("");
    Some(match method {
        "initialize" => {
            let version = req["params"]["protocolVersion"].as_str().unwrap_or(DEFAULT_PROTOCOL);
            json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": version, "capabilities": {"tools": {}}, "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")}, "instructions": shared.instructions}})
        }
        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        "tools/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"tools": shared.tools}}),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("").to_string();
            let call_id = req["params"]["_meta"]["claudecode/toolUseId"].as_str().unwrap_or("").to_string();
            let content = if !shared.names.contains(&name) {
                ToolResult { content: format!("`{}` is not one of the tools Delphi offered this task", name), is_error: true }
            } else {
                let (reply, rx) = oneshot::channel();
                let args = req["params"].get("arguments").cloned().unwrap_or_else(|| json!({}));
                if shared.tx.send(BridgeCall { name, args, call_id, reply }).await.is_err() {
                    ToolResult { content: "this task has ended; the tool was not run".into(), is_error: true }
                } else {
                    rx.await.unwrap_or(ToolResult { content: "this task ended before the tool finished".into(), is_error: true })
                }
            };
            let mut text = content.content;
            if text.chars().count() > MAX_REPLY_CHARS {
                text = text.chars().take(MAX_REPLY_CHARS).collect::<String>() + "\n[the result was cut at the bridge's size limit]";
            }
            json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": text}], "isError": content.is_error}})
        }
        // The CLI probes with methods of newer protocol revisions (`server/discover`); the right answer is "no such method".
        _ => error(&id, -32601, "method not found"),
    })
}

/// `delphi __mcp-bridge --socket <path>`: the relay the CLI starts as its MCP server. It connects to the span's
/// socket, sends the token from `DELPHI_BRIDGE_TOKEN`, and copies lines both ways. Exit code 0 when either side closes.
pub fn run_relay(args: &[String]) -> i32 {
    let socket = args.iter().position(|a| a == "--socket").and_then(|i| args.get(i + 1));
    let token = std::env::var("DELPHI_BRIDGE_TOKEN").unwrap_or_default();
    let (Some(socket), false) = (socket, token.is_empty()) else {
        eprintln!("usage: delphi __mcp-bridge --socket <path> (with DELPHI_BRIDGE_TOKEN set)");
        return 2;
    };
    // SAFETY: single-threaded here; the child tools the CLI runs never inherit the token.
    unsafe { std::env::remove_var("DELPHI_BRIDGE_TOKEN") };
    let mut stream = match std::os::unix::net::UnixStream::connect(socket) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot reach the Delphi bridge: {}", e);
            return 1;
        }
    };
    if stream.write_all(format!("{}\n", json!({"token": token})).as_bytes()).is_err() {
        return 1;
    }
    let mut up = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut up);
        // The CLI closed its end: let the server see EOF.
        let _ = up.shutdown(std::net::Shutdown::Write);
    });
    let mut out = std::io::stdout().lock();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return 0,
            Ok(n) => {
                if out.write_all(&chunk[..n]).is_err() || out.flush().is_err() {
                    return 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    fn defs() -> Vec<ToolDef> {
        vec![ToolDef { name: "timer".into(), description: "Wake me later.".into(), input_schema: json!({"type": "object", "properties": {"note": {"type": "string"}}}) }]
    }

    async fn client(b: &Bridge, token: &str) -> (BufReader<tokio::net::unix::OwnedReadHalf>, tokio::net::unix::OwnedWriteHalf) {
        let (r, mut w) = UnixStream::connect(b.socket()).await.unwrap().into_split();
        w.write_all(format!("{}\n", json!({"token": token})).as_bytes()).await.unwrap();
        (BufReader::new(r), w)
    }

    async fn ask(r: &mut BufReader<tokio::net::unix::OwnedReadHalf>, w: &mut tokio::net::unix::OwnedWriteHalf, req: Value) -> Value {
        w.write_all(format!("{}\n", req).as_bytes()).await.unwrap();
        let mut line = String::new();
        r.read_line(&mut line).await.unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[tokio::test]
    async fn the_handshake_lists_the_offered_tools_and_refuses_unknown_methods_politely() {
        let (b, _rx) = Bridge::start(defs(), "Use `timer` to wait.".into(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let init = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}}})).await;
        assert_eq!((init["result"]["protocolVersion"].as_str(), init["result"]["serverInfo"]["name"].as_str(), init["result"]["instructions"].as_str()), (Some("2025-11-25"), Some("delphi"), Some("Use `timer` to wait.")));
        w.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n").await.unwrap();
        let probe = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": "server-discover-probe-1", "method": "server/discover"})).await;
        assert_eq!((probe["id"].as_str(), probe["error"]["code"].as_i64()), (Some("server-discover-probe-1"), Some(-32601)));
        let list = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
        assert_eq!(list["result"]["tools"][0]["name"], "timer");
        assert_eq!(list["result"]["tools"][0]["inputSchema"]["type"], "object");
        assert!(ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 3, "method": "ping"})).await["result"].is_object());
    }

    #[tokio::test]
    async fn a_tool_call_reaches_the_span_loop_and_its_result_goes_back_to_the_cli() {
        let (b, mut rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let span = tokio::spawn(async move {
            let call = rx.recv().await.expect("a call");
            assert_eq!((call.name.as_str(), call.args["note"].as_str(), call.call_id.as_str()), ("timer", Some("check CI"), "toolu_1"));
            call.reply.send(ToolResult { content: "set for 12:00".into(), is_error: false }).unwrap();
        });
        let resp = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": "timer", "arguments": {"note": "check CI"}, "_meta": {"claudecode/toolUseId": "toolu_1"}}})).await;
        span.await.unwrap();
        assert_eq!((resp["id"].as_i64(), resp["result"]["content"][0]["text"].as_str(), resp["result"]["isError"].as_bool()), (Some(9), Some("set for 12:00"), Some(false)));
    }

    #[tokio::test]
    async fn a_tool_that_was_not_offered_is_refused_without_reaching_the_span() {
        let (b, mut rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let resp = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "write_file", "arguments": {}}})).await;
        assert_eq!(resp["result"]["isError"], true);
        assert!(resp["result"]["content"][0]["text"].as_str().unwrap().contains("not one of the tools"));
        assert!(rx.try_recv().is_err(), "nothing was queued for the span");
    }

    #[tokio::test]
    async fn a_connection_without_the_span_token_gets_nothing_and_is_closed() {
        let (b, _rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (r, mut w) = UnixStream::connect(b.socket()).await.unwrap().into_split();
        w.write_all(b"{\"token\":\"wrong\"}\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n").await.unwrap();
        let mut rest = Vec::new();
        BufReader::new(r).read_to_end(&mut rest).await.unwrap();
        assert!(rest.is_empty(), "a wrong token is answered with silence, not with the tool list");
    }

    #[tokio::test]
    async fn the_socket_directory_is_private_and_removed_with_the_bridge() {
        use std::os::unix::fs::PermissionsExt;
        let (b, _rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let dir = b.dir.clone();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(b.config_path()).unwrap().permissions().mode() & 0o777, 0o600);
        let cfg: Value = serde_json::from_str(&std::fs::read_to_string(b.config_path()).unwrap()).unwrap();
        assert_eq!(cfg["mcpServers"]["delphi"]["args"][0], "__mcp-bridge");
        assert!(!cfg["mcpServers"]["delphi"]["args"].to_string().contains(b.token()), "the token is in the environment, not on a command line");
        assert_eq!(cfg["mcpServers"]["delphi"]["env"]["DELPHI_BRIDGE_TOKEN"], b.token());
        drop(b);
        assert!(!dir.exists());
    }

    #[tokio::test]
    async fn a_call_after_the_span_ended_is_answered_with_an_error_not_a_hang() {
        let (b, rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        drop(rx);
        let (mut r, mut w) = client(&b, b.token()).await;
        let resp = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "timer", "arguments": {}}})).await;
        assert_eq!(resp["result"]["isError"], true);
    }
}
