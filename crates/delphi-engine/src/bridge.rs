//! The Delphi tool bridge (`specs/020` §2): Delphi's own tools, offered to a subscription CLI as one MCP server.
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
//!
//! The Antigravity CLI (`agy`) gets the same bridge through a workspace-local `.agents/` directory instead of a flag
//! ([`Bridge::write_agy_workspace`], `specs/020` §8): a plugin that starts the relay, and a `PreToolUse` hook
//! (`delphi __agy-hook`, [`run_agy_hook`]) that asks this server, over the same socket, whether each tool call may
//! run. The answer comes from a [`HookPolicy`]: Delphi's own tools and the few CLI tools a profile names are allowed,
//! everything else is denied. A hook that cannot reach the server denies, and so does one that fails, which `agy`
//! also treats as a denial.

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

/// A tool call of the CLI's own that the span's loop must judge (a Builder's file and shell tools), waiting for its answer.
/// The gate holds the CLI's hook open until the loop replies, which may be after an approval card has been answered.
#[derive(Debug)]
pub struct HookAsk {
    pub tool: String,
    pub args: Value,
    /// `(allowed, reason shown to the model when it is not)`
    pub reply: oneshot::Sender<(bool, String)>,
}

/// The CLI's own tools a Builder's gate hands to the span's loop instead of denying. Its shell is not among them: the CLI
/// cannot ask anyone to approve a command when it runs headless, so a Builder runs commands through Delphi's own
/// `run_command` tool (sandboxed, classified, approved by the operator), and the gate says so when the shell is tried.
pub const AGY_BUILDER_TOOLS: [&str; 3] = ["read_file", "write_file", "edit_file"];

/// The tool-name prefix of Delphi's tools as the Antigravity CLI shows them: `mcp_<plugin>_<server>_<tool>`.
pub const AGY_TOOL_PREFIX: &str = "mcp_delphi_delphi_";

/// What a `PreToolUse` hook may let through (`specs/020` §8.3).
#[derive(Debug, Clone, Default)]
pub struct HookPolicy {
    /// A tool whose name starts with one of these is Delphi's own.
    pub allow_prefixes: Vec<String>,
    /// Exact names of the CLI's own tools a profile allows (`search_web`, `read_url_content`).
    pub allow_names: Vec<String>,
    /// Set the moment the gate lets one of `allow_names` through. What such a tool brings back is text nobody vetted, so
    /// the span's loop checks this before it runs any Delphi call and marks the span untrusted (L0). The gate sets it
    /// before the CLI runs the tool, so a later call cannot beat it.
    pub tainted: Arc<std::sync::atomic::AtomicBool>,
    /// A Builder's gate: the file and shell tools in [`AGY_BUILDER_TOOLS`] are asked of the span's loop, which applies the
    /// same rules as for a Claude Builder. Without it they are denied.
    pub ask: Option<mpsc::Sender<HookAsk>>,
}

impl HookPolicy {
    pub fn new(allow_prefixes: Vec<String>, allow_names: Vec<String>) -> HookPolicy {
        HookPolicy { allow_prefixes, allow_names, tainted: Arc::default(), ask: None }
    }

    /// This policy for a Builder, with the channel its asks go to.
    pub fn for_builder(mut self, ask: mpsc::Sender<HookAsk>) -> HookPolicy {
        self.ask = Some(ask);
        self
    }

    /// Has the gate let a tool through that reads from outside Delphi?
    pub fn has_read_outside(&self) -> bool {
        self.tainted.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Allow or deny a tool by name, with the reason the model is told when it is denied.
    pub fn decide(&self, tool: &str) -> (bool, String) {
        if !tool.is_empty() && self.allow_prefixes.iter().any(|p| tool.starts_with(p)) {
            return (true, String::new());
        }
        if !tool.is_empty() && self.allow_names.iter().any(|n| n == tool) {
            self.tainted.store(true, std::sync::atomic::Ordering::SeqCst);
            return (true, String::new());
        }
        let own = self.allow_prefixes.first().map(String::as_str).unwrap_or(AGY_TOOL_PREFIX);
        let web = if self.allow_names.is_empty() { String::new() } else { format!(" and {}", self.allow_names.join(", ")) };
        let why = match tool {
            "call_mcp_tool" => format!("call the Delphi tools directly by their names (they start with `{}`)", own),
            "run_command" if self.ask.is_some() => format!("your own shell is switched off in this task; run commands with the Delphi tool `{}shell` (a shell line, when it is offered) or `{}run_command` (a program and its arguments, no shell)", own, own),
            "" => "the tool call named no tool".to_string(),
            t => format!("`{}` is not available in this task; the Delphi tools (names starting `{}`){} are", t, own, web),
        };
        (false, format!("denied by Delphi: {}", why))
    }
}

/// How long the hook waits for the gate when the span is not a Builder: a decision is instant. (The CLI's own limit on the hook is five seconds more.)
pub const DEFAULT_HOOK_WAIT_SECS: u64 = 10;

/// The Antigravity CLI's names for the CLI tools a `[harness.agy]` profile lists. The profile uses the Claude CLI's names
/// for the web tools (`WebSearch`, `WebFetch`); an entry that is already an `agy` tool name (lower case, underscores) is
/// kept; any other Claude-only name (`Read`, `Bash`, ...) means nothing to `agy` and is dropped, so it cannot widen the
/// gate by accident. No duplicates.
pub fn agy_native_tools(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let mapped = match n.as_str() {
            "WebSearch" => Some("search_web".to_string()),
            "WebFetch" => Some("read_url_content".to_string()),
            x if !x.is_empty() && x.chars().all(|c| c.is_ascii_lowercase() || c == '_') => Some(x.to_string()),
            _ => None,
        };
        if let Some(m) = mapped.filter(|m| !out.contains(m)) {
            out.push(m);
        }
    }
    out
}

/// One decision the gate made, for the span's log.
#[derive(Debug, Clone, PartialEq)]
pub struct HookNote {
    pub tool: String,
    pub allowed: bool,
    /// The first useful argument (a path, a command, a query), shortened, for the log. Never a secret the model was not already given.
    pub summary: String,
}

/// A short, printable stand-in for a tool call's arguments.
fn summarize_args(args: &Value) -> String {
    let pick = ["CommandLine", "Path", "path", "query", "Query", "Url", "url", "TargetFile", "Prompt"].iter().find_map(|k| args[*k].as_str());
    pick.unwrap_or("").chars().take(200).collect()
}

/// What the bridge turned away on its own, before anything reached the span: calls for a tool the span was not offered
/// (a model that guessed a name, or a forged call) and connections that did not open with the span's token. Counts and
/// tool names only, never arguments; the span logs it once at its end (`harness.bridge_refused`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Refusals {
    pub not_offered: u32,
    /// The first few distinct tool names asked for, cut to 60 characters each.
    pub tools: Vec<String>,
    pub bad_connections: u32,
}

const MAX_REFUSED_NAMES: usize = 10;

impl Refusals {
    pub fn is_empty(&self) -> bool {
        self.not_offered == 0 && self.bad_connections == 0
    }
}

/// A running bridge. Dropping it stops the server and removes its directory.
pub struct Bridge {
    refusals: Arc<std::sync::Mutex<Refusals>>,
    dir: PathBuf,
    config_path: PathBuf,
    token: String,
    accept: tokio::task::JoinHandle<()>,
    program: PathBuf,
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(bytes)
}

/// `s` as one word for `sh -c`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut b = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{:02x}", x)).collect())
}

/// Gemini's function-calling validation rejects the whole request, and so the whole span, if any array in any tool's
/// schema has no `items` key; `items: {}` is accepted and claims nothing about the elements. Claude does not mind either way, so
/// this is applied only to the server an `agy` span gets (a remote connector's schema is not Delphi's to correct at the source).
fn with_array_items(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            if map.get("type").is_some_and(|t| t == "array" || t.as_array().is_some_and(|a| a.iter().any(|x| x == "array"))) && !map.contains_key("items") {
                map.insert("items".into(), json!({}));
            }
            for v in map.values_mut() {
                with_array_items(v);
            }
        }
        Value::Array(list) => list.iter_mut().for_each(with_array_items),
        _ => {}
    }
}

/// Map a Delphi tool to MCP's shape.
fn mcp_tool(d: &ToolDef, gemini: bool) -> Value {
    let mut schema = d.input_schema.clone();
    if gemini {
        with_array_items(&mut schema);
    }
    // `antigravity/alwaysLoad`: the Antigravity CLI loads a server's tools on demand, behind its generic `call_mcp_tool`,
    // unless the server marks them. Delphi's tools are the point of the seat, so they are offered directly by their own
    // names (which also lets a gate see which tool is being called). Other clients ignore the key.
    json!({"name": d.name, "description": d.description, "inputSchema": schema, "_meta": {"antigravity/alwaysLoad": true}})
}

impl Bridge {
    /// Listen for one span. `tools` are the only names `tools/call` will accept; `instructions` is shown to the model
    /// by the CLI as the server's own usage notes. `program` is what the CLI runs as the relay (the `delphi` executable).
    pub async fn start(tools: Vec<ToolDef>, instructions: String, program: &Path) -> std::io::Result<(Bridge, mpsc::Receiver<BridgeCall>)> {
        let (b, rx, _) = Bridge::start_gated(tools, instructions, program, None).await?;
        Ok((b, rx))
    }

    /// [`Bridge::start`], and with a [`HookPolicy`] the server also answers the `agy` gate hook and sends a [`HookNote`]
    /// for every decision.
    pub async fn start_gated(tools: Vec<ToolDef>, instructions: String, program: &Path, hook: Option<HookPolicy>) -> std::io::Result<(Bridge, mpsc::Receiver<BridgeCall>, mpsc::UnboundedReceiver<HookNote>)> {
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
        let config = json!({"mcpServers": {SERVER_NAME: {"command": program.display().to_string(), "args": ["__mcp-bridge"], "env": {"DELPHI_BRIDGE_SOCKET": socket.display().to_string(), "DELPHI_BRIDGE_TOKEN": token}}}});
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&config_path)?;
            f.write_all(config.to_string().as_bytes())?;
        }
        let (tx, rx) = mpsc::channel::<BridgeCall>(16);
        let (notes, notes_rx) = mpsc::unbounded_channel::<HookNote>();
        let refusals = Arc::new(std::sync::Mutex::new(Refusals::default()));
        let shared = Arc::new(Shared { tools: tools.iter().map(|t| mcp_tool(t, hook.is_some())).collect(), names: tools.iter().map(|t| t.name.clone()).collect(), instructions, token: token.clone(), tx, hook, notes, refusals: refusals.clone() });
        let accept = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let shared = shared.clone();
                tokio::spawn(async move { serve(stream, shared).await });
            }
        });
        Ok((Bridge { refusals, dir, config_path, token, accept, program: program.to_path_buf() }, rx, notes_rx))
    }

    /// What the bridge turned away since the last call (see [`Refusals`]); the count starts again from nothing.
    pub fn take_refusals(&self) -> Refusals {
        std::mem::take(&mut *self.refusals.lock().unwrap())
    }

    /// For the Antigravity CLI: write this span's `.agents/` into `dir`, a directory Delphi made for the span and the CLI
    /// will use as its working directory. `.agents/plugins/delphi/` is a plugin whose MCP server is this bridge's relay
    /// (the same config the Claude CLI is given), and `.agents/hooks.json` has one `PreToolUse` hook, for every tool, that
    /// runs `delphi __agy-hook`. Nothing outside `dir` is touched, and an earlier span's `.agents/` (a dead socket and
    /// token) is replaced. Files are mode 0600 in directories of mode 0700, because they hold the span's token.
    pub fn write_agy_workspace(&self, dir: &Path) -> std::io::Result<()> {
        self.write_agy_workspace_waiting(dir, DEFAULT_HOOK_WAIT_SECS)
    }

    /// [`Self::write_agy_workspace`], with the hook allowed to wait `wait_secs` for the gate's answer. A Builder's hook can
    /// be held open by an approval card, so it waits as long as the card may stay up.
    pub fn write_agy_workspace_waiting(&self, dir: &Path, wait_secs: u64) -> std::io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        let agents = dir.join(".agents");
        let _ = std::fs::remove_dir_all(&agents);
        let plugin = agents.join("plugins").join("delphi");
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&plugin)?;
        write_private(&plugin.join("plugin.json"), br#"{"name":"delphi"}"#)?;
        write_private(&plugin.join("mcp_config.json"), std::fs::read(&self.config_path)?.as_slice())?;
        let command = format!("DELPHI_BRIDGE_SOCKET={} DELPHI_BRIDGE_TOKEN={} DELPHI_HOOK_WAIT={} {} __agy-hook", sh_quote(&self.socket().display().to_string()), sh_quote(&self.token), wait_secs, sh_quote(&self.program.display().to_string()));
        let hooks = json!({"delphi-gate": {"PreToolUse": [{"matcher": ".*", "hooks": [{"type": "command", "command": command, "timeout": wait_secs + 5}]}]}});
        write_private(&agents.join("hooks.json"), hooks.to_string().as_bytes())
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
    hook: Option<HookPolicy>,
    notes: mpsc::UnboundedSender<HookNote>,
    refusals: Arc<std::sync::Mutex<Refusals>>,
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
        // Nothing legitimate opens a connection and then fails to say who it is; a connection that closes before saying anything is not counted.
        Ok(0) | Err(_) => return,
        _ => {
            let mut r = shared.refusals.lock().unwrap();
            r.bad_connections = r.bad_connections.saturating_add(1);
            return;
        }
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
                let mut r = shared.refusals.lock().unwrap();
                r.not_offered = r.not_offered.saturating_add(1);
                let shown: String = name.chars().take(60).collect();
                if r.tools.len() < MAX_REFUSED_NAMES && !r.tools.contains(&shown) {
                    r.tools.push(shown);
                }
                drop(r);
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
        // The Antigravity CLI's `PreToolUse` gate asks whether a tool call may run (`run_agy_hook`).
        "delphi/hook" => match &shared.hook {
            Some(policy) => {
                let call = &req["params"]["toolCall"];
                let tool = call["name"].as_str().unwrap_or("");
                let (mut allowed, mut reason) = policy.decide(tool);
                if let (false, Some(ask), true) = (allowed, &policy.ask, AGY_BUILDER_TOOLS.contains(&tool)) {
                    let (reply, answer) = oneshot::channel();
                    (allowed, reason) = if ask.send(HookAsk { tool: tool.to_string(), args: call["args"].clone(), reply }).await.is_err() {
                        (false, "denied by Delphi: this task has ended".to_string())
                    } else {
                        answer.await.unwrap_or((false, "denied by Delphi: this task ended before the call was judged".to_string()))
                    };
                }
                let _ = shared.notes.send(HookNote { tool: tool.to_string(), allowed, summary: summarize_args(&call["args"]) });
                json!({"jsonrpc": "2.0", "id": id, "result": {"decision": if allowed { "allow" } else { "deny" }, "reason": reason}})
            }
            None => error(&id, -32601, "this task has no gate"),
        },
        // The CLI probes with methods of newer protocol revisions (`server/discover`); the right answer is "no such method".
        _ => error(&id, -32601, "method not found"),
    })
}

/// `delphi __mcp-bridge`: the relay the CLI starts as its MCP server. It connects to the span's socket
/// (`DELPHI_BRIDGE_SOCKET`, or `--socket <path>`), sends the token from `DELPHI_BRIDGE_TOKEN`, and copies lines both
/// ways. Exit code 0 when either side closes. The command line is the same for every span, so an administrator who
/// allows MCP servers by command (`allowedMcpServers` with `serverCommand`) can pin it once.
pub fn run_relay(args: &[String]) -> i32 {
    let socket = args.iter().position(|a| a == "--socket").and_then(|i| args.get(i + 1)).cloned().or_else(|| std::env::var("DELPHI_BRIDGE_SOCKET").ok()).filter(|s| !s.is_empty());
    let token = std::env::var("DELPHI_BRIDGE_TOKEN").unwrap_or_default();
    let (Some(socket), false) = (socket, token.is_empty()) else {
        eprintln!("usage: delphi __mcp-bridge (with DELPHI_BRIDGE_SOCKET and DELPHI_BRIDGE_TOKEN set)");
        return 2;
    };
    // SAFETY: single-threaded here; the child tools the CLI runs never inherit the token.
    unsafe { std::env::remove_var("DELPHI_BRIDGE_TOKEN") };
    let mut stream = match std::os::unix::net::UnixStream::connect(&socket) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot reach the Delphi bridge: {}", e);
            delphi_core::elog::error("tool", "bridge_unreachable", "the tool bridge relay could not reach the span's server", serde_json::json!({"kind": format!("{:?}", e.kind())}));
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

/// `delphi __agy-hook`: the `PreToolUse` hook the Antigravity CLI runs before every tool call. It reads the call (JSON on
/// stdin), asks the span's server over its socket (`DELPHI_BRIDGE_SOCKET`, `DELPHI_BRIDGE_TOKEN`) and prints the decision
/// as the CLI expects it. It holds no policy of its own, and anything that goes wrong is a denial: the CLI treats a hook that
/// exits non-zero, prints nothing or prints garbage as a denial as well (verified), but a plain "deny" says why.
pub fn run_agy_hook() -> i32 {
    fn say(decision: &str, reason: &str) -> i32 {
        println!("{}", json!({"decision": decision, "reason": reason}));
        0
    }
    let deny = |why: &str| say("deny", &format!("denied by Delphi: {}", why));
    let mut payload = String::new();
    if std::io::stdin().take(1 << 20).read_to_string(&mut payload).is_err() {
        return deny("the tool call could not be read");
    }
    let Ok(params) = serde_json::from_str::<Value>(&payload) else { return deny("the tool call was not valid JSON") };
    let (Some(socket), token) = (std::env::var("DELPHI_BRIDGE_SOCKET").ok().filter(|s| !s.is_empty()), std::env::var("DELPHI_BRIDGE_TOKEN").unwrap_or_default()) else {
        return deny("this task's gate is not configured");
    };
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&socket) else { return deny("the gate could not be reached") };
    let wait_secs = std::env::var("DELPHI_HOOK_WAIT").ok().and_then(|v| v.parse::<u64>().ok()).filter(|s| (1..=86_400).contains(s)).unwrap_or(DEFAULT_HOOK_WAIT_SECS);
    let wait = Some(std::time::Duration::from_secs(wait_secs));
    let _ = stream.set_read_timeout(wait);
    let _ = stream.set_write_timeout(wait);
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "delphi/hook", "params": params});
    if stream.write_all(format!("{}\n{}\n", json!({"token": token}), request).as_bytes()).is_err() {
        return deny("the gate could not be reached");
    }
    let mut line = String::new();
    if std::io::BufRead::read_line(&mut std::io::BufReader::new(&stream), &mut line).is_err() {
        return deny("the gate did not answer");
    }
    match serde_json::from_str::<Value>(&line) {
        Ok(v) if v["result"]["decision"] == "allow" => say("allow", ""),
        Ok(v) => deny(v["result"]["reason"].as_str().map(|r| r.trim_start_matches("denied by Delphi: ")).filter(|r| !r.is_empty()).unwrap_or("the gate refused it")),
        Err(_) => deny("the gate's answer was not understood"),
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
        assert_eq!(list["result"]["tools"][0]["_meta"]["antigravity/alwaysLoad"], true, "agy offers the tool directly, by its own name, instead of behind call_mcp_tool");
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
        assert_eq!(cfg["mcpServers"]["delphi"]["args"], json!(["__mcp-bridge"]), "the same command line for every span, so an administrator can allow it once by command");
        assert_eq!(cfg["mcpServers"]["delphi"]["env"]["DELPHI_BRIDGE_SOCKET"], b.socket().display().to_string());
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

    #[tokio::test]
    async fn a_notification_gets_no_reply_and_a_line_that_is_not_json_is_skipped_without_closing_the_connection() {
        let (b, _rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        w.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\nthis is not json\n{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"ping\"}\n").await.unwrap();
        // None of those three lines is answered (a notification, garbage, and a null id are not requests), so the first
        // line back is the answer to the ping that follows.
        let pong = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 7, "method": "ping"})).await;
        assert_eq!(pong["id"], 7, "{}", pong);
    }

    #[tokio::test]
    async fn a_slow_tool_call_does_not_hold_up_a_ping_and_each_reply_carries_its_own_id() {
        let (b, mut rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        w.write_all(format!("{}\n", json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "timer", "arguments": {}}})).as_bytes()).await.unwrap();
        let held = rx.recv().await.expect("the call reached the span");
        // The call is waiting for an approval card, say. The CLI's ping is answered meanwhile.
        let pong = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 2, "method": "ping"})).await;
        assert_eq!(pong["id"], 2);
        held.reply.send(ToolResult { content: "late".into(), is_error: false }).unwrap();
        let mut line = String::new();
        r.read_line(&mut line).await.unwrap();
        let late: Value = serde_json::from_str(&line).unwrap();
        assert_eq!((late["id"].as_i64(), late["result"]["content"][0]["text"].as_str()), (Some(1), Some("late")));
    }

    #[tokio::test]
    async fn many_clients_at_once_each_get_the_answer_to_their_own_call() {
        let (b, mut rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let span = tokio::spawn(async move {
            // Answer in reverse arrival order, so a bridge that paired replies by position would swap them.
            let mut calls = Vec::new();
            while calls.len() < 8 {
                calls.push(rx.recv().await.expect("a call"));
            }
            for call in calls.into_iter().rev() {
                let note = call.args["note"].as_str().unwrap().to_string();
                call.reply.send(ToolResult { content: format!("echo {}", note), is_error: false }).unwrap();
            }
        });
        let mut clients = Vec::new();
        for i in 0..8 {
            let (mut r, mut w) = client(&b, b.token()).await;
            clients.push(tokio::spawn(async move {
                let resp = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 100 + i, "method": "tools/call", "params": {"name": "timer", "arguments": {"note": format!("n{}", i)}}})).await;
                (i, resp)
            }));
        }
        for c in clients {
            let (i, resp) = c.await.unwrap();
            assert_eq!((resp["id"].as_i64(), resp["result"]["content"][0]["text"].as_str()), (Some(100 + i), Some(format!("echo n{}", i).as_str())));
        }
        span.await.unwrap();
    }

    #[tokio::test]
    async fn a_reply_longer_than_the_limit_is_cut_with_a_note_so_one_result_cannot_fill_the_clis_context() {
        let (b, mut rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let span = tokio::spawn(async move {
            let call = rx.recv().await.unwrap();
            call.reply.send(ToolResult { content: "é".repeat(MAX_REPLY_CHARS + 50), is_error: false }).unwrap();
        });
        let resp = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "timer", "arguments": {}}})).await;
        span.await.unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.ends_with("[the result was cut at the bridge's size limit]"), "{}", &text[text.len() - 80..]);
        assert_eq!(text.chars().filter(|c| *c == 'é').count(), MAX_REPLY_CHARS, "cut on a character, never inside one");
    }

    #[tokio::test]
    async fn a_call_with_no_arguments_reaches_the_span_as_an_empty_object_and_with_no_call_id() {
        let (b, mut rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let span = tokio::spawn(async move {
            let call = rx.recv().await.unwrap();
            let seen = (call.args.clone(), call.call_id.clone());
            call.reply.send(ToolResult { content: "ok".into(), is_error: false }).unwrap();
            seen
        });
        ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "timer"}})).await;
        assert_eq!(span.await.unwrap(), (json!({}), String::new()));
    }

    #[tokio::test]
    async fn a_span_that_drops_the_call_without_answering_gives_the_cli_an_error_not_a_hang() {
        let (b, mut rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let span = tokio::spawn(async move { drop(rx.recv().await.unwrap()) });
        let resp = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "timer", "arguments": {}}})).await;
        span.await.unwrap();
        assert_eq!(resp["result"]["isError"], true);
        assert!(resp["result"]["content"][0]["text"].as_str().unwrap().contains("ended before the tool finished"));
    }

    #[tokio::test]
    async fn a_request_line_over_the_limit_closes_the_connection_and_the_bridge_keeps_serving_others() {
        let (b, _rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let mut huge = vec![b'x'; MAX_LINE + 10];
        huge.push(b'\n');
        let _ = w.write_all(&huge).await; // the server may close while this is still being written
        let mut rest = Vec::new();
        let _ = r.read_to_end(&mut rest).await;
        assert!(rest.is_empty(), "no answer to a line that size");
        let (mut r2, mut w2) = client(&b, b.token()).await;
        assert!(ask(&mut r2, &mut w2, json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})).await["result"].is_object());
    }

    #[tokio::test]
    async fn a_first_line_that_is_not_just_the_token_closes_the_connection_in_silence() {
        let (b, _rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let long = format!("{}\n", json!({"token": b.token(), "padding": "x".repeat(300)}));
        for first in [long, "not json\n".to_string(), "{\"nope\":1}\n".to_string(), "{\"token\":17}\n".to_string()] {
            let (r, mut w) = UnixStream::connect(b.socket()).await.unwrap().into_split();
            w.write_all(first.as_bytes()).await.unwrap();
            w.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n").await.unwrap();
            let mut rest = Vec::new();
            BufReader::new(r).read_to_end(&mut rest).await.unwrap();
            assert!(rest.is_empty(), "answered {:?}: {}", first.chars().take(30).collect::<String>(), String::from_utf8_lossy(&rest));
        }
    }

    #[tokio::test]
    async fn calls_for_tools_that_were_not_offered_are_counted_by_name_once_each_and_bounded_and_taking_them_starts_again() {
        let (b, _rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        assert!(b.take_refusals().is_empty());
        for (i, name) in ["write_file", "write_file", "write_file"].iter().chain(["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"].iter()).enumerate() {
            ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": i, "method": "tools/call", "params": {"name": name, "arguments": {"secret": "never recorded"}}})).await;
        }
        ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 99, "method": "tools/call", "params": {"name": "x".repeat(200), "arguments": {}}})).await;
        let got = b.take_refusals();
        assert_eq!(got.not_offered, 16, "every refused call is counted");
        assert_eq!(got.tools.len(), MAX_REFUSED_NAMES, "but only the first few distinct names are kept");
        assert_eq!(got.tools[0], "write_file");
        assert_eq!(got.tools.iter().filter(|t| *t == "write_file").count(), 1, "a name once, however often it was asked for");
        assert!(!format!("{:?}", got).contains("never recorded"), "arguments are never kept");
        assert!(b.take_refusals().is_empty(), "taking resets the count");
        // The offered tool is not a refusal.
        ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})).await;
        assert!(b.take_refusals().is_empty());
    }

    #[tokio::test]
    async fn a_name_cut_to_sixty_characters_is_what_is_kept_of_a_refused_tool() {
        let (b, _rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "ü".repeat(200), "arguments": {}}})).await;
        assert_eq!(b.take_refusals().tools, vec!["ü".repeat(60)]);
    }

    #[tokio::test]
    async fn connections_that_do_not_open_with_the_token_are_counted_but_one_that_just_closes_is_not() {
        let (b, _rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        // A wrong token, a first line that is not JSON, and one that is far too long.
        for first in ["{\"token\":\"wrong\"}\n".to_string(), "hello\n".to_string(), format!("{}\n", "x".repeat(400))] {
            let (r, mut w) = UnixStream::connect(b.socket()).await.unwrap().into_split();
            w.write_all(first.as_bytes()).await.unwrap();
            let mut rest = Vec::new();
            BufReader::new(r).read_to_end(&mut rest).await.unwrap(); // the server closes, which is how we know it has judged the line
        }
        // A probe that connects and goes away without a word is not an attack on the token.
        drop(UnixStream::connect(b.socket()).await.unwrap());
        let (mut r, mut w) = client(&b, b.token()).await;
        assert!(ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})).await["result"].is_object());
        let got = b.take_refusals();
        assert_eq!((got.bad_connections, got.not_offered), (3, 0));
    }

    #[tokio::test]
    async fn every_offered_tool_is_listed_in_the_order_given_and_only_those_can_be_called() {
        let mut offered = defs();
        offered.push(ToolDef { name: "name_thread".into(), description: "Name a thread.".into(), input_schema: json!({"type": "object", "properties": {"tasks": {"type": "array", "items": {"type": "string"}}}}) });
        let (b, mut rx) = Bridge::start(offered, String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let list = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["timer", "name_thread"]);
        // A name that differs only by case or by the CLI's prefix is not the tool: the span is asked for exact names.
        for wrong in ["Timer", "mcp__delphi__timer", "timer ", ""] {
            let resp = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": wrong, "arguments": {}}})).await;
            assert_eq!(resp["result"]["isError"], true, "{:?}", wrong);
        }
        assert!(rx.try_recv().is_err());
    }

    fn agy_policy() -> HookPolicy {
        HookPolicy::new(vec![AGY_TOOL_PREFIX.into()], vec!["search_web".into(), "read_url_content".into()])
    }

    #[tokio::test]
    async fn a_gated_bridge_gives_every_array_an_items_key_because_gemini_rejects_the_whole_request_without_one() {
        let sloppy = ToolDef { name: "t".into(), description: "d".into(), input_schema: json!({"type": "object", "properties": {
            "bare": {"type": "array"},
            "typed": {"type": "array", "items": {"type": "string"}},
            "nested": {"type": "array", "items": {"type": "object", "properties": {"inner": {"type": "array"}}}},
            "either": {"type": ["array", "null"]},
            "choice": {"anyOf": [{"type": "array"}, {"type": "string"}]}}}) };
        let ask_tools = |hook: Option<HookPolicy>| {
            let tool = sloppy.clone();
            async move {
                let (b, _calls, _notes) = Bridge::start_gated(vec![tool], String::new(), Path::new("/x/delphi"), hook).await.unwrap();
                let (mut r, mut w) = client(&b, b.token()).await;
                ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await["result"]["tools"][0]["inputSchema"]["properties"].clone()
            }
        };
        let g = ask_tools(Some(agy_policy())).await;
        assert_eq!(g["bare"]["items"], json!({}));
        assert_eq!(g["typed"]["items"], json!({"type": "string"}), "an items that is there is left alone");
        assert_eq!(g["nested"]["items"]["properties"]["inner"]["items"], json!({}));
        assert_eq!(g["either"]["items"], json!({}));
        assert_eq!(g["choice"]["anyOf"][0]["items"], json!({}));
        // The Claude CLI's bridge offers the schema exactly as Delphi defined it.
        let c = ask_tools(None).await;
        assert!(c["bare"].get("items").is_none() && c["nested"]["items"]["properties"]["inner"].get("items").is_none());
    }

    #[test]
    fn the_gate_lets_delphis_tools_and_the_listed_web_tools_through_and_tells_the_model_why_the_rest_is_denied() {
        let g = agy_policy();
        for ok in ["mcp_delphi_delphi_timer", "mcp_delphi_delphi_dispatch_agent", "search_web", "read_url_content"] {
            assert_eq!(g.decide(ok), (true, String::new()), "{}", ok);
        }
        for bad in ["run_command", "view_file", "write_to_file", "browser_subagent", "invoke_subagent", "define_subagent", "schedule", "generate_image", "send_command_input"] {
            let (allowed, why) = g.decide(bad);
            assert!(!allowed, "{}", bad);
            assert!(why.starts_with("denied by Delphi:") && why.contains(bad) && why.contains("mcp_delphi_delphi_") && why.contains("search_web"), "the model is told what to use instead: {}", why);
        }
        // The generic MCP caller would reach the same tools by another name, so the model is sent to the direct names.
        assert!(g.decide("call_mcp_tool").1.contains("directly by their names"));
        // A lookalike is not Delphi's, and a call that names no tool is denied.
        assert!(!g.decide("mcp_delphi_other_timer").0 && !g.decide("xmcp_delphi_delphi_timer").0 && !g.decide("").0);
        // No web tools listed: none allowed, and the reason does not mention them.
        let none = HookPolicy::new(vec![AGY_TOOL_PREFIX.into()], vec![]);
        assert!(!none.decide("search_web").0 && !none.decide("run_command").1.contains("search_web"));
    }

    #[test]
    fn a_web_tool_through_the_gate_raises_the_flag_and_delphis_own_tools_and_denials_do_not() {
        let g = agy_policy();
        assert!(!g.has_read_outside());
        assert!(g.decide("mcp_delphi_delphi_timer").0 && !g.decide("run_command").0);
        assert!(!g.has_read_outside(), "neither Delphi's tool nor a denial brings in outside text");
        assert!(g.decide("search_web").0);
        assert!(g.has_read_outside(), "set by the decision, before the CLI runs the tool");
        // A clone shares the flag, which is how the span's loop sees what the server's copy did.
        let h = agy_policy();
        let seen_by_loop = h.clone();
        assert!(h.decide("read_url_content").0 && seen_by_loop.has_read_outside());
    }

    #[tokio::test]
    async fn the_hook_asks_the_span_over_the_socket_and_every_decision_is_noted_for_the_log() {
        let (b, _calls, mut notes) = Bridge::start_gated(defs(), String::new(), Path::new("/x/delphi"), Some(agy_policy())).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let hook = |name: &str, args: Value| json!({"jsonrpc": "2.0", "id": 5, "method": "delphi/hook", "params": {"toolCall": {"name": name, "args": args}, "conversationId": "c"}});
        let allow = ask(&mut r, &mut w, hook("mcp_delphi_delphi_timer", json!({"note": "x"}))).await;
        assert_eq!((allow["result"]["decision"].as_str(), allow["result"]["reason"].as_str()), (Some("allow"), Some("")));
        let deny = ask(&mut r, &mut w, hook("run_command", json!({"CommandLine": "echo leaked > proof.txt"}))).await;
        assert_eq!(deny["result"]["decision"], "deny");
        assert!(deny["result"]["reason"].as_str().unwrap().contains("run_command"));
        let nameless = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 6, "method": "delphi/hook", "params": {}})).await;
        assert_eq!(nameless["result"]["decision"], "deny", "a payload without a tool is refused, not waved through");
        let seen: Vec<HookNote> = std::iter::from_fn(|| notes.try_recv().ok()).collect();
        assert_eq!(seen[0], HookNote { tool: "mcp_delphi_delphi_timer".into(), allowed: true, summary: String::new() });
        assert_eq!((seen[1].tool.as_str(), seen[1].allowed, seen[1].summary.as_str()), ("run_command", false, "echo leaked > proof.txt"));
        assert_eq!(seen.len(), 3);
    }

    #[tokio::test]
    async fn a_bridge_without_a_gate_does_not_answer_the_hook() {
        let (b, _rx) = Bridge::start(defs(), String::new(), Path::new("/x/delphi")).await.unwrap();
        let (mut r, mut w) = client(&b, b.token()).await;
        let resp = ask(&mut r, &mut w, json!({"jsonrpc": "2.0", "id": 1, "method": "delphi/hook", "params": {"toolCall": {"name": "run_command"}}})).await;
        assert_eq!(resp["error"]["code"], -32601, "no policy means no allow");
        assert!(resp.get("result").is_none());
    }

    #[tokio::test]
    async fn the_agy_workspace_holds_the_plugin_and_the_gate_privately_and_replaces_an_earlier_span() {
        use std::os::unix::fs::PermissionsExt;
        let (b, _calls, _notes) = Bridge::start_gated(defs(), "notes".into(), Path::new("/opt/it's here/delphi"), Some(agy_policy())).await.unwrap();
        let ws = tempfile::tempdir().unwrap();
        // An earlier span's leftovers, and a file that is not Delphi's, in the same directory.
        std::fs::create_dir_all(ws.path().join(".agents/plugins/delphi")).unwrap();
        std::fs::write(ws.path().join(".agents/plugins/delphi/stale.json"), "old").unwrap();
        std::fs::write(ws.path().join("keep.txt"), "mine").unwrap();
        b.write_agy_workspace(ws.path()).unwrap();
        let plugin = ws.path().join(".agents/plugins/delphi");
        assert!(!plugin.join("stale.json").exists() && ws.path().join("keep.txt").exists());
        assert_eq!(std::fs::read_to_string(plugin.join("plugin.json")).unwrap(), r#"{"name":"delphi"}"#);
        // The plugin's MCP server is the same relay config the Claude CLI is given.
        assert_eq!(std::fs::read_to_string(plugin.join("mcp_config.json")).unwrap(), std::fs::read_to_string(b.config_path()).unwrap());
        // The gate: one PreToolUse hook for every tool, running `delphi __agy-hook` with this span's socket and token.
        let hooks: Value = serde_json::from_str(&std::fs::read_to_string(ws.path().join(".agents/hooks.json")).unwrap()).unwrap();
        let h = &hooks["delphi-gate"]["PreToolUse"][0];
        assert_eq!(h["matcher"], ".*");
        let cmd = h["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains(&format!("DELPHI_BRIDGE_SOCKET='{}'", b.socket().display())) && cmd.contains(&format!("DELPHI_BRIDGE_TOKEN='{}'", b.token())), "{}", cmd);
        assert!(cmd.ends_with("'/opt/it'\\''s here/delphi' __agy-hook"), "a quote in the path is escaped, not trusted: {}", cmd);
        assert_eq!((h["hooks"][0]["type"].as_str(), h["hooks"][0]["timeout"].as_u64()), (Some("command"), Some(15)));
        // Private: the files hold the span's token.
        for f in [plugin.join("mcp_config.json"), ws.path().join(".agents/hooks.json")] {
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600, "{}", f.display());
        }
        assert_eq!(std::fs::metadata(ws.path().join(".agents")).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
