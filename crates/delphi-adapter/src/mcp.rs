//! The MCP types Delphi's connector host works with (`specs/010` §2): the session trait, the typed errors,
//! and the definition hash that pins a tool.
//!
//! The client itself is the official SDK, wrapped in [`crate::mcp_rmcp`]. The policy code (taint, approvals,
//! pinning, host allowlist) lives outside that file and only sees these types.

use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// The oldest protocol revision Delphi accepts (`specs/010` §2.5).
pub const MIN_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpError {
    /// The token is missing, expired or revoked (HTTP 401). Never retried with another account.
    NeedsReauth,
    /// The provider refused the app or the account (HTTP 403), with its own words.
    Forbidden(String),
    /// HTTP 429 or 503.
    Throttled(Option<Duration>),
    /// The URL or a redirect named a host that is not on the connector's list (or was not https).
    HostRefused(String),
    /// The server's protocol revision is older than the minimum.
    OldProtocol(String),
    /// A JSON-RPC error object.
    Rpc { code: i64, message: String },
    /// Anything about the HTTP exchange or the body.
    Transport(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::NeedsReauth => write!(f, "the connector needs you to sign in again"),
            McpError::Forbidden(m) => write!(f, "the provider refused access: {}", m),
            McpError::Throttled(_) => write!(f, "the connector is rate limited"),
            McpError::HostRefused(h) => write!(f, "refused to contact `{}`: not on this connector's host list", h),
            McpError::OldProtocol(v) => write!(f, "the server speaks MCP revision {}, older than the minimum {}", v, MIN_PROTOCOL_VERSION),
            McpError::Rpc { code, message } => write!(f, "the server returned an error ({}): {}", code, message),
            McpError::Transport(m) => write!(f, "{}", m),
        }
    }
}

impl std::error::Error for McpError {}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One tool as the server describes it. The description is untrusted text.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// A tool result with its content flattened to text.
#[derive(Debug, Clone, PartialEq)]
pub struct CallResult {
    pub text: String,
    pub is_error: bool,
}

/// What the connector host needs from an initialized MCP session: list the server's tools and call one. The real
/// implementation is [`crate::mcp_rmcp::RmcpClient`]; tests supply a scripted one.
pub trait McpSession: Send + Sync {
    fn list_tools(&self) -> BoxFuture<'_, Result<Vec<RemoteTool>, McpError>>;
    fn call_tool<'a>(&'a self, name: &'a str, arguments: &'a Value) -> BoxFuture<'a, Result<CallResult, McpError>>;
}

/// A stable hash of a tool's name, input schema and description (`specs/010` §3.2): a change to any of the three
/// changes it. The schema is hashed in canonical (sorted-key) form.
pub fn definition_hash(t: &RemoteTool) -> String {
    fn canon(v: &Value) -> String {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                format!("{{{}}}", keys.iter().map(|k| format!("{}:{}", serde_json::to_string(k).unwrap_or_default(), canon(&m[*k]))).collect::<Vec<_>>().join(","))
            }
            Value::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(",")),
            other => other.to_string(),
        }
    }
    blake3::hash(format!("{}\n{}\n{}", t.name, canon(&t.input_schema), t.description).as_bytes()).to_hex().to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex as StdMutex};

    /// A one-thread MCP server on loopback. `handler(method, params, headers) -> (status, content-type, body)`.
    pub(crate) struct Server {
        pub(crate) port: u16,
        pub(crate) seen: Arc<StdMutex<Vec<Seen>>>,
    }

    pub(crate) type Seen = (String, std::collections::BTreeMap<String, String>, Value);

    pub(crate) type Handler = dyn Fn(&str, &Value, &std::collections::BTreeMap<String, String>) -> (u16, String, String, Vec<(String, String)>) + Send + Sync;

    pub(crate) fn server(handler: Arc<Handler>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let (handler, seen) = (handler.clone(), seen2.clone());
                std::thread::spawn(move || {
                    let mut r = BufReader::new(s.try_clone().unwrap());
                    let mut first = String::new();
                    if r.read_line(&mut first).unwrap_or(0) == 0 {
                        return;
                    }
                    let mut headers = std::collections::BTreeMap::new();
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
                    let msg: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let method = msg["method"].as_str().unwrap_or("").to_string();
                    seen.lock().unwrap().push((method.clone(), headers.clone(), msg.clone()));
                    let (status, ctype, out, extra) = handler(&method, &msg, &headers);
                    let mut head = format!("HTTP/1.1 {} X\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n", status, ctype, out.len());
                    for (k, v) in extra {
                        head.push_str(&format!("{}: {}\r\n", k, v));
                    }
                    let _ = write!(s, "{}\r\n{}", head, out);
                });
            }
        });
        Server { port, seen }
    }

    pub(crate) fn rpc(id: &Value, result: Value) -> String {
        json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
    }

    pub(crate) fn standard(tools: Value) -> Arc<Handler> {
        Arc::new(move |method, msg, _h| match method {
            "initialize" => (200, "application/json".into(), rpc(&msg["id"], json!({"protocolVersion": "2025-11-25", "capabilities": {}, "serverInfo": {"name": "mock", "version": "0"}})), vec![("mcp-session-id".into(), "sess-1".into())]),
            "notifications/initialized" => (202, "text/plain".into(), String::new(), vec![]),
            "tools/list" => (200, "application/json".into(), rpc(&msg["id"], json!({"tools": tools})), vec![]),
            "tools/call" => (200, "text/event-stream".into(), format!("event: message\ndata: {}\n\n", rpc(&msg["id"], json!({"content": [{"type": "text", "text": format!("called {}", msg["params"]["name"].as_str().unwrap_or(""))}, {"type": "image", "data": "xx", "mimeType": "image/png"}], "isError": false}))), vec![]),
            _ => (404, "text/plain".into(), "no".into(), vec![]),
        })
    }

    #[test]
    fn definition_hashes_track_every_part() {
        let t = RemoteTool { name: "a".into(), description: "A".into(), input_schema: json!({"b": 1, "a": {"y": 2, "x": 1}}) };
        let same_reordered = RemoteTool { input_schema: json!({"a": {"x": 1, "y": 2}, "b": 1}), ..t.clone() };
        assert_eq!(definition_hash(&t), definition_hash(&same_reordered), "key order does not matter");
        for changed in [RemoteTool { name: "a2".into(), ..t.clone() }, RemoteTool { description: "A, now with instructions".into(), ..t.clone() }, RemoteTool { input_schema: json!({"b": 1}), ..t.clone() }] {
            assert_ne!(definition_hash(&t), definition_hash(&changed));
        }
    }
}
