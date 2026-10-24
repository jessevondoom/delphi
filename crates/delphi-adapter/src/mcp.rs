//! A minimal MCP client over Streamable HTTP (`specs/010` §2).
//!
//! Only what Delphi uses: `initialize`, `tools/list`, `tools/call`. No roots, sampling, prompts or resources.
//! The transport is a small trait so an adopted library can replace it later; the policy code (taint,
//! approvals, pinning, host allowlist) lives outside this file and does not change.
//!
//! The HTTP transport refuses any host that is not on the connector's list, follows a redirect only to
//! another listed host over https, caps response size and time, and never puts a token in an error.

use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

/// The newest protocol revision Delphi speaks, and the oldest it accepts (`specs/010` §2.5).
pub const PROTOCOL_VERSION: &str = "2025-11-25";
pub const MIN_PROTOCOL_VERSION: &str = "2025-11-25";

const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOOL_PAGES: usize = 10;

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

/// What a transport hands back for one POST: the HTTP status, the session id if one was issued, and the JSON-RPC
/// response message for the request (`None` for a notification).
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub session_id: Option<String>,
    pub message: Option<Value>,
    pub retry_after: Option<Duration>,
    pub body_hint: String,
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One POST of a JSON-RPC message, with the headers MCP needs.
pub trait McpTransport: Send + Sync {
    fn post<'a>(&'a self, body: &'a Value, headers: &'a [(String, String)], expect_id: Option<i64>) -> BoxFuture<'a, Result<Reply, McpError>>;
}

/// Streamable HTTP over `reqwest`, bound to one URL and its host list.
pub struct HttpTransport {
    http: reqwest::Client,
    url: String,
    hosts: Vec<String>,
    timeout: Duration,
}

fn host_allowed(url: &reqwest::Url, hosts: &[String]) -> bool {
    let ok_scheme = url.scheme() == "https" || (url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost") | Some("[::1]")));
    ok_scheme && url.host_str().is_some_and(|h| hosts.iter().any(|x| x.eq_ignore_ascii_case(h)))
}

impl HttpTransport {
    pub fn new(url: &str, hosts: &[String]) -> Result<HttpTransport, McpError> {
        let parsed = reqwest::Url::parse(url).map_err(|e| McpError::Transport(format!("bad connector url: {}", e)))?;
        if !host_allowed(&parsed, hosts) {
            return Err(McpError::HostRefused(parsed.host_str().unwrap_or("").to_string()));
        }
        // Redirects are handled by hand, so each hop is checked against the host list.
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(15)).user_agent(concat!("delphi/", env!("CARGO_PKG_VERSION"))).build().map_err(|e| McpError::Transport(e.to_string()))?;
        Ok(HttpTransport { http, url: url.to_string(), hosts: hosts.to_vec(), timeout: Duration::from_secs(60) })
    }
}

/// The JSON-RPC response with `id` from an SSE body (other messages, such as notifications, are ignored).
fn from_sse(text: &str, id: i64) -> Option<Value> {
    let mut data = String::new();
    let mut found = None;
    let flush = |data: &mut String, found: &mut Option<Value>| {
        if !data.is_empty() {
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                if v["id"].as_i64() == Some(id) && (v.get("result").is_some() || v.get("error").is_some()) {
                    *found = Some(v);
                }
            }
            data.clear();
        }
    };
    for line in text.lines() {
        if let Some(d) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(d.strip_prefix(' ').unwrap_or(d));
        } else if line.is_empty() {
            flush(&mut data, &mut found);
        }
    }
    flush(&mut data, &mut found);
    found
}

impl McpTransport for HttpTransport {
    fn post<'a>(&'a self, body: &'a Value, headers: &'a [(String, String)], expect_id: Option<i64>) -> BoxFuture<'a, Result<Reply, McpError>> {
        Box::pin(async move {
            let mut url = reqwest::Url::parse(&self.url).map_err(|e| McpError::Transport(e.to_string()))?;
            for _hop in 0..3 {
                if !host_allowed(&url, &self.hosts) {
                    return Err(McpError::HostRefused(url.host_str().unwrap_or("").to_string()));
                }
                let mut req = self.http.post(url.clone()).timeout(self.timeout).header("content-type", "application/json").header("accept", "application/json, text/event-stream").body(serde_json::to_vec(body).map_err(|e| McpError::Transport(e.to_string()))?);
                for (k, v) in headers {
                    req = req.header(k.as_str(), v.as_str());
                }
                let resp = req.send().await.map_err(|e| McpError::Transport(e.without_url().to_string()))?;
                let status = resp.status().as_u16();
                if matches!(status, 307 | 308) {
                    // Only a redirect that keeps the method, only within the host list.
                    let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).ok_or_else(|| McpError::Transport("a redirect with no location".into()))?;
                    url = url.join(loc).map_err(|e| McpError::Transport(e.to_string()))?;
                    continue;
                }
                if (300..400).contains(&status) {
                    return Err(McpError::HostRefused("a redirect that would change the request".into()));
                }
                let session_id = resp.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(String::from);
                let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).map(Duration::from_secs);
                let is_sse = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("text/event-stream"));
                // Read at most MAX_RESPONSE_BYTES.
                let mut bytes: Vec<u8> = Vec::new();
                let mut resp = resp;
                while let Some(chunk) = resp.chunk().await.map_err(|e| McpError::Transport(e.without_url().to_string()))? {
                    if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                        return Err(McpError::Transport(format!("the response is larger than {} bytes", MAX_RESPONSE_BYTES)));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let hint: String = text.chars().take(300).collect();
                if !(200..300).contains(&status) {
                    return Ok(Reply { status, session_id, message: None, retry_after, body_hint: hint });
                }
                let message = match expect_id {
                    None => None, // a notification: 202 with no body
                    Some(id) if is_sse => from_sse(&text, id),
                    Some(_) => serde_json::from_str::<Value>(&text).ok().filter(|v| v.is_object()),
                };
                return Ok(Reply { status, session_id, message, retry_after, body_hint: hint });
            }
            Err(McpError::HostRefused("too many redirects".into()))
        })
    }
}

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

/// An initialized MCP session.
pub struct McpClient {
    transport: Box<dyn McpTransport>,
    token: Option<String>,
    session: Mutex<Option<String>>,
    version: Mutex<String>,
    next_id: std::sync::atomic::AtomicI64,
}

impl McpClient {
    /// `token` is the OAuth access token, sent as a Bearer header on every request.
    pub fn new(transport: Box<dyn McpTransport>, token: Option<String>) -> McpClient {
        McpClient { transport, token, session: Mutex::new(None), version: Mutex::new(PROTOCOL_VERSION.into()), next_id: std::sync::atomic::AtomicI64::new(1) }
    }

    fn headers(&self) -> Vec<(String, String)> {
        let mut h = vec![("mcp-protocol-version".to_string(), self.version.lock().unwrap().clone())];
        if let Some(t) = &self.token {
            h.push(("authorization".into(), format!("Bearer {}", t)));
        }
        if let Some(s) = self.session.lock().unwrap().clone() {
            h.push(("mcp-session-id".into(), s));
        }
        h
    }

    fn map_status(r: &Reply) -> Result<(), McpError> {
        match r.status {
            200..=299 => Ok(()),
            401 => Err(McpError::NeedsReauth),
            403 => Err(McpError::Forbidden(r.body_hint.chars().take(200).collect())),
            429 | 503 => Err(McpError::Throttled(r.retry_after)),
            other => Err(McpError::Transport(format!("HTTP {}: {}", other, r.body_hint.chars().take(200).collect::<String>()))),
        }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let headers = self.headers();
        let reply = self.transport.post(&body, &headers, Some(id)).await?;
        Self::map_status(&reply)?;
        if let Some(s) = &reply.session_id {
            *self.session.lock().unwrap() = Some(s.clone());
        }
        let msg = reply.message.ok_or_else(|| McpError::Transport(format!("no response to `{}`", method)))?;
        if let Some(e) = msg.get("error") {
            return Err(McpError::Rpc { code: e["code"].as_i64().unwrap_or(0), message: e["message"].as_str().unwrap_or("").chars().take(300).collect() });
        }
        Ok(msg["result"].clone())
    }

    /// `initialize` and `notifications/initialized`. Fails if the server speaks an older revision than the minimum.
    pub async fn initialize(&self) -> Result<Value, McpError> {
        *self.session.lock().unwrap() = None;
        let r = self.request("initialize", json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "delphi", "version": env!("CARGO_PKG_VERSION")}})).await?;
        let v = r["protocolVersion"].as_str().unwrap_or("").to_string();
        if v.as_str() < MIN_PROTOCOL_VERSION {
            return Err(McpError::OldProtocol(if v.is_empty() { "unknown".into() } else { v }));
        }
        *self.version.lock().unwrap() = v;
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let reply = self.transport.post(&note, &self.headers(), None).await?;
        Self::map_status(&reply)?;
        Ok(r)
    }

    pub async fn list_tools(&self) -> Result<Vec<RemoteTool>, McpError> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_TOOL_PAGES {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let r = self.request("tools/list", params).await?;
            for t in r["tools"].as_array().into_iter().flatten() {
                if let Some(name) = t["name"].as_str() {
                    out.push(RemoteTool { name: name.to_string(), description: t["description"].as_str().unwrap_or("").to_string(), input_schema: t["inputSchema"].clone() });
                }
            }
            match r["nextCursor"].as_str() {
                Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
                _ => return Ok(out),
            }
        }
        Ok(out)
    }

    pub async fn call_tool(&self, name: &str, arguments: &Value) -> Result<CallResult, McpError> {
        let r = self.request("tools/call", json!({"name": name, "arguments": arguments})).await?;
        let mut parts: Vec<String> = Vec::new();
        for c in r["content"].as_array().into_iter().flatten() {
            match c["type"].as_str() {
                Some("text") => parts.push(c["text"].as_str().unwrap_or("").to_string()),
                Some(other) => parts.push(format!("[{} content omitted]", other)),
                None => {}
            }
        }
        if parts.is_empty() {
            if let Some(sc) = r.get("structuredContent") {
                parts.push(sc.to_string());
            }
        }
        Ok(CallResult { text: parts.join("\n"), is_error: r["isError"] == true })
    }
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
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex as StdMutex};

    /// A one-thread MCP server on loopback. `handler(method, params, headers) -> (status, content-type, body)`.
    struct Server {
        port: u16,
        seen: Arc<StdMutex<Vec<Seen>>>,
    }

    type Seen = (String, std::collections::BTreeMap<String, String>, Value);

    type Handler = dyn Fn(&str, &Value, &std::collections::BTreeMap<String, String>) -> (u16, String, String, Vec<(String, String)>) + Send + Sync;

    fn server(handler: Arc<Handler>) -> Server {
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

    fn rpc(id: &Value, result: Value) -> String {
        json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
    }

    fn standard(tools: Value) -> Arc<Handler> {
        Arc::new(move |method, msg, _h| match method {
            "initialize" => (200, "application/json".into(), rpc(&msg["id"], json!({"protocolVersion": "2025-11-25", "capabilities": {}, "serverInfo": {"name": "mock"}})), vec![("mcp-session-id".into(), "sess-1".into())]),
            "notifications/initialized" => (202, "text/plain".into(), String::new(), vec![]),
            "tools/list" => (200, "application/json".into(), rpc(&msg["id"], json!({"tools": tools})), vec![]),
            "tools/call" => (200, "text/event-stream".into(), format!("event: message\ndata: {}\n\n", rpc(&msg["id"], json!({"content": [{"type": "text", "text": format!("called {}", msg["params"]["name"].as_str().unwrap_or(""))}, {"type": "image", "data": "xx"}], "isError": false}))), vec![]),
            _ => (404, "text/plain".into(), "no".into(), vec![]),
        })
    }

    fn client(port: u16, hosts: &[&str], token: Option<&str>) -> McpClient {
        let hosts: Vec<String> = hosts.iter().map(|s| s.to_string()).collect();
        McpClient::new(Box::new(HttpTransport::new(&format!("http://127.0.0.1:{}/mcp", port), &hosts).unwrap()), token.map(String::from))
    }

    #[tokio::test]
    async fn a_session_initializes_lists_tools_and_calls_one_over_json_and_sse() {
        let tools = json!([{"name": "search_files", "description": "Search", "inputSchema": {"type": "object"}}, {"name": "read_file", "description": "Read", "inputSchema": {"type": "object"}}]);
        let s = server(standard(tools));
        let c = client(s.port, &["127.0.0.1"], Some("tok-123"));
        let init = c.initialize().await.unwrap();
        assert_eq!(init["serverInfo"]["name"], "mock");
        let listed = c.list_tools().await.unwrap();
        assert_eq!(listed.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["search_files", "read_file"]);
        let r = c.call_tool("search_files", &json!({"q": "x"})).await.unwrap();
        assert_eq!((r.text.as_str(), r.is_error), ("called search_files\n[image content omitted]", false));
        let seen = s.seen.lock().unwrap().clone();
        let list = seen.iter().find(|(m, _, _)| m == "tools/list").unwrap();
        assert_eq!(list.1["authorization"], "Bearer tok-123");
        assert_eq!(list.1["mcp-session-id"], "sess-1", "the session id from initialize is sent afterwards");
        assert_eq!(list.1["mcp-protocol-version"], "2025-11-25");
        assert!(list.1["accept"].contains("text/event-stream"));
    }

    #[tokio::test]
    async fn status_codes_map_to_typed_errors_and_tokens_never_appear_in_them() {
        for (status, want) in [(401u16, McpError::NeedsReauth), (429, McpError::Throttled(None))] {
            let s = server(Arc::new(move |_m, _msg, _h| (status, "text/plain".into(), "tok-secret leaked in a body".into(), vec![])));
            let err = client(s.port, &["127.0.0.1"], Some("tok-secret")).initialize().await.unwrap_err();
            assert_eq!(err, want);
            assert!(!err.to_string().contains("tok-secret"));
        }
        let s = server(Arc::new(|_m, _msg, _h| (403, "text/plain".into(), "admin approval required".into(), vec![])));
        assert!(matches!(client(s.port, &["127.0.0.1"], None).initialize().await.unwrap_err(), McpError::Forbidden(m) if m.contains("admin approval")));
        let s = server(Arc::new(|_m, msg, _h| (200, "application/json".into(), json!({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32602, "message": "bad params"}}).to_string(), vec![])));
        assert_eq!(client(s.port, &["127.0.0.1"], None).initialize().await.unwrap_err(), McpError::Rpc { code: -32602, message: "bad params".into() });
    }

    #[tokio::test]
    async fn an_old_protocol_revision_is_refused() {
        let s = server(Arc::new(|_m, msg, _h| (200, "application/json".into(), rpc(&msg["id"], json!({"protocolVersion": "2024-11-05"})), vec![])));
        assert_eq!(client(s.port, &["127.0.0.1"], None).initialize().await.unwrap_err(), McpError::OldProtocol("2024-11-05".into()));
    }

    #[tokio::test]
    async fn hosts_outside_the_list_are_refused_including_through_a_redirect() {
        // The configured URL itself must be on the list.
        assert!(matches!(HttpTransport::new("http://127.0.0.1:1/mcp", &["mcp.example.com".to_string()]), Err(McpError::HostRefused(_))));
        assert!(matches!(HttpTransport::new("http://mcp.example.com/mcp", &["mcp.example.com".to_string()]), Err(McpError::HostRefused(_))), "plain http only to loopback");
        // A server that redirects somewhere else.
        let s = server(Arc::new(|_m, _msg, _h| (307, "text/plain".into(), String::new(), vec![("location".into(), "http://localhost:9/evil".into())])));
        let err = client(s.port, &["127.0.0.1"], Some("tok")).initialize().await.unwrap_err();
        assert!(matches!(err, McpError::HostRefused(ref h) if h == "localhost"), "{:?}", err);
        // A redirect that would turn the POST into a GET is refused outright.
        let s = server(Arc::new(|_m, _msg, _h| (302, "text/plain".into(), String::new(), vec![("location".into(), "/elsewhere".into())])));
        assert!(matches!(client(s.port, &["127.0.0.1"], None).initialize().await.unwrap_err(), McpError::HostRefused(_)));
    }

    #[tokio::test]
    async fn tool_listing_follows_pages_and_definition_hashes_track_every_part() {
        let pages: Arc<StdMutex<usize>> = Arc::new(StdMutex::new(0));
        let p2 = pages.clone();
        let s = server(Arc::new(move |method, msg, _h| match method {
            "initialize" => (200, "application/json".into(), rpc(&msg["id"], json!({"protocolVersion": "2025-11-25"})), vec![]),
            "notifications/initialized" => (202, "text/plain".into(), String::new(), vec![]),
            _ => {
                let mut n = p2.lock().unwrap();
                *n += 1;
                let (tools, next) = if *n == 1 { (json!([{"name": "a", "description": "A", "inputSchema": {}}]), json!("page2")) } else { (json!([{"name": "b", "description": "B", "inputSchema": {}}]), Value::Null) };
                (200, "application/json".into(), rpc(&msg["id"], json!({"tools": tools, "nextCursor": next})), vec![])
            }
        }));
        let c = client(s.port, &["127.0.0.1"], None);
        c.initialize().await.unwrap();
        assert_eq!(c.list_tools().await.unwrap().len(), 2);
        let t = RemoteTool { name: "a".into(), description: "A".into(), input_schema: json!({"b": 1, "a": {"y": 2, "x": 1}}) };
        let same_reordered = RemoteTool { input_schema: json!({"a": {"x": 1, "y": 2}, "b": 1}), ..t.clone() };
        assert_eq!(definition_hash(&t), definition_hash(&same_reordered), "key order does not matter");
        for changed in [RemoteTool { name: "a2".into(), ..t.clone() }, RemoteTool { description: "A, now with instructions".into(), ..t.clone() }, RemoteTool { input_schema: json!({"b": 1}), ..t.clone() }] {
            assert_ne!(definition_hash(&t), definition_hash(&changed));
        }
    }

    #[test]
    fn sse_bodies_yield_the_response_with_the_matching_id() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(from_sse(body, 7).unwrap()["result"]["ok"], true);
        assert!(from_sse(body, 8).is_none());
        assert_eq!(from_sse("data: {\"id\":1,\"result\":{}}", 1).map(|v| v["id"].clone()), Some(json!(1)), "a final event with no blank line after it");
    }
}
