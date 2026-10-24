//! The same MCP client surface as [`crate::mcp::McpClient`], backed by the official `rmcp` SDK (spike).
//!
//! The policy stays ours: the host list is enforced by the `reqwest` client's redirect policy, the URL is
//! checked before anything is sent, and rmcp's errors are mapped back onto [`McpError`] so the connector code
//! above (taint, approvals, pinning) would not change.
//!
//! Gaps against `mcp.rs`, found by running the same loopback tests against both (none is wired into the
//! connectors yet; decide each before switching over):
//! - **No cap on a plain JSON response.** rmcp bounds one SSE event (`max_sse_event_size`) but reads a JSON body
//!   with `response.bytes()`. `mcp.rs` stops at 8 MiB. Closing this needs a custom `StreamableHttpClient`.
//! - **`Retry-After` is dropped**, so `Throttled` is always `None`.
//! - **401/403 are typed only through rmcp's own errors or the text `HTTP <status>: <body>`**, which `status_error`
//!   parses; a change to that wording would silently turn them into `Transport`.
//! - **Strict parsing.** rmcp rejects a server that omits `serverInfo.version`, or an image block with no
//!   `mimeType` (the whole call fails). `mcp.rs` tolerates both.
//! - **Tool paging is unbounded** (`list_all_tools`); `mcp.rs` stops after 10 pages.
//! - rmcp also gives session recovery on a 404, stdio and child-process transports, and an OAuth module, none of
//!   which `mcp.rs` has.

use crate::mcp::{CallResult, McpError, RemoteTool, MIN_PROTOCOL_VERSION};
use rmcp::model::{CallToolRequestParams, ClientConfig, ContentBlock, Implementation};
use rmcp::service::{ClientInitializeError, RoleClient, RunningService, ServiceError, ServiceExt};
use rmcp::transport::DynamicTransportError;
use rmcp::transport::streamable_http_client::{StreamableHttpClientTransportConfig, StreamableHttpError};
use rmcp::transport::StreamableHttpClientTransport;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Refused = Arc<Mutex<Option<String>>>;

pub struct RmcpClient {
    service: RunningService<RoleClient, ClientConfig>,
    refused: Refused,
    version: String,
}

fn host_allowed(url: &reqwest::Url, hosts: &[String]) -> bool {
    let ok_scheme = url.scheme() == "https" || (url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost") | Some("[::1]")));
    ok_scheme && url.host_str().is_some_and(|h| hosts.iter().any(|x| x.eq_ignore_ascii_case(h)))
}

/// Maps whatever rmcp produced onto Delphi's typed errors. `refused` is set by the redirect policy, because
/// reqwest reports a policy error as an opaque transport failure.
fn map_error(e: ServiceError, refused: &Refused) -> McpError {
    if let Some(h) = refused.lock().unwrap().clone() {
        return McpError::HostRefused(h);
    }
    match e {
        ServiceError::McpError(d) => McpError::Rpc { code: d.code.0 as i64, message: d.message.chars().take(300).collect() },
        ServiceError::TransportSend(t) => map_transport(&t),
        other => McpError::Transport(other.to_string()),
    }
}

fn map_init_error(e: ClientInitializeError, refused: &Refused) -> McpError {
    if let Some(h) = refused.lock().unwrap().clone() {
        return McpError::HostRefused(h);
    }
    match e {
        ClientInitializeError::TransportError { error, .. } => map_transport(&error),
        ClientInitializeError::JsonRpcError(d) => McpError::Rpc { code: d.code.0 as i64, message: d.message.chars().take(300).collect() },
        ClientInitializeError::NoCompatibleProtocolVersion { server_supported, .. } => McpError::OldProtocol(server_supported.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")),
        other => McpError::Transport(other.to_string()),
    }
}

fn map_transport(t: &DynamicTransportError) -> McpError {
    match t.error.downcast_ref::<StreamableHttpError<reqwest::Error>>() {
        Some(StreamableHttpError::AuthRequired(_)) => McpError::NeedsReauth,
        Some(StreamableHttpError::InsufficientScope(_)) => McpError::Forbidden("the token does not carry the scope this tool needs".into()),
        Some(StreamableHttpError::UnexpectedServerResponse(m)) => status_error(m),
        // Described by kind, never by `Display`, which includes the URL.
        Some(StreamableHttpError::Client(c)) => McpError::Transport(if c.is_timeout() { "the connector timed out" } else if c.is_connect() { "could not connect to the connector" } else { "the request to the connector failed" }.into()),
        Some(other) => McpError::Transport(other.to_string()),
        None => McpError::Transport(t.to_string()),
    }
}

/// rmcp reports an unexpected status as the text `HTTP <status>: <body>`; recover the status.
fn status_error(m: &str) -> McpError {
    let status: u16 = m.strip_prefix("HTTP ").and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|s| s.parse().ok()).unwrap_or(0);
    let body: String = m.split_once(": ").map(|x| x.1).unwrap_or("").chars().take(200).collect();
    match status {
        401 => McpError::NeedsReauth,
        403 => McpError::Forbidden(body),
        // rmcp does not hand back the Retry-After header, so the delay is lost.
        429 | 503 => McpError::Throttled(None),
        _ => McpError::Transport(format!("HTTP {}: {}", status, body)),
    }
}

impl RmcpClient {
    /// Connects and runs `initialize`. `token` is sent as a Bearer header.
    pub async fn connect(url: &str, hosts: &[String], token: Option<String>) -> Result<RmcpClient, McpError> {
        let parsed = reqwest::Url::parse(url).map_err(|e| McpError::Transport(format!("bad connector url: {}", e)))?;
        if !host_allowed(&parsed, hosts) {
            return Err(McpError::HostRefused(parsed.host_str().unwrap_or("").to_string()));
        }
        let refused: Refused = Arc::new(Mutex::new(None));
        let (hosts_for_policy, refused_for_policy) = (hosts.to_vec(), refused.clone());
        // Follow only 307/308 (they keep the method and body), only to a listed host, at most 3 hops.
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            let keeps_body = matches!(attempt.status(), reqwest::StatusCode::TEMPORARY_REDIRECT | reqwest::StatusCode::PERMANENT_REDIRECT);
            let host = attempt.url().host_str().unwrap_or("").to_string();
            if !keeps_body || !host_allowed(attempt.url(), &hosts_for_policy) {
                *refused_for_policy.lock().unwrap() = Some(if keeps_body { host } else { "a redirect that would change the request".into() });
                attempt.error("redirect refused")
            } else if attempt.previous().len() > 3 {
                *refused_for_policy.lock().unwrap() = Some("too many redirects".into());
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        });
        let http = reqwest::Client::builder().redirect(policy).connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(60)).user_agent(concat!("delphi/", env!("CARGO_PKG_VERSION"))).build().map_err(|e| McpError::Transport(e.to_string()))?;
        let mut config = StreamableHttpClientTransportConfig::with_uri(url.to_string());
        if let Some(t) = token {
            config = config.auth_header(t);
        }
        let transport = StreamableHttpClientTransport::with_client(http, config);
        let info = ClientConfig::new(Default::default(), Implementation::new("delphi", env!("CARGO_PKG_VERSION")));
        let service = info.serve(transport).await.map_err(|e| map_init_error(e, &refused))?;
        let version = service.peer_info().map(|p| p.protocol_version.to_string()).unwrap_or_default();
        if version.as_str() < MIN_PROTOCOL_VERSION {
            return Err(McpError::OldProtocol(if version.is_empty() { "unknown".into() } else { version }));
        }
        Ok(RmcpClient { service, refused, version })
    }

    pub fn protocol_version(&self) -> &str {
        &self.version
    }

    pub async fn list_tools(&self) -> Result<Vec<RemoteTool>, McpError> {
        let tools = self.service.list_all_tools().await.map_err(|e| map_error(e, &self.refused))?;
        Ok(tools.into_iter().map(|t| RemoteTool { name: t.name.to_string(), description: t.description.map(|d| d.to_string()).unwrap_or_default(), input_schema: Value::Object((*t.input_schema).clone()) }).collect())
    }

    pub async fn call_tool(&self, name: &str, arguments: &Value) -> Result<CallResult, McpError> {
        let mut params = CallToolRequestParams::new(name.to_string());
        params.arguments = arguments.as_object().cloned();
        let r = self.service.call_tool(params).await.map_err(|e| map_error(e, &self.refused))?;
        let mut parts: Vec<String> = r.content.iter().map(|c| match c {
            ContentBlock::Text(t) => t.text.clone(),
            ContentBlock::Image(_) => "[image content omitted]".to_string(),
            _ => "[content omitted]".to_string(),
        }).collect();
        if parts.is_empty() {
            if let Some(sc) = &r.structured_content {
                parts.push(sc.to_string());
            }
        }
        Ok(CallResult { text: parts.join("\n"), is_error: r.is_error == Some(true) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tests::{rpc, server, standard};
    use serde_json::json;
    use std::sync::Arc;

    async fn connect(port: u16, hosts: &[&str], token: Option<&str>) -> Result<RmcpClient, McpError> {
        let hosts: Vec<String> = hosts.iter().map(|s| s.to_string()).collect();
        RmcpClient::connect(&format!("http://127.0.0.1:{}/mcp", port), &hosts, token.map(String::from)).await
    }

    #[tokio::test]
    async fn a_session_initializes_lists_tools_and_calls_one_over_json_and_sse() {
        let tools = json!([{"name": "search_files", "description": "Search", "inputSchema": {"type": "object"}}, {"name": "read_file", "description": "Read", "inputSchema": {"type": "object"}}]);
        let s = server(standard(tools));
        let c = connect(s.port, &["127.0.0.1"], Some("tok-123")).await.unwrap();
        assert_eq!(c.protocol_version(), "2025-11-25");
        let listed = c.list_tools().await.unwrap();
        assert_eq!(listed.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["search_files", "read_file"]);
        let r = c.call_tool("search_files", &json!({"q": "x"})).await.unwrap();
        assert_eq!((r.text.as_str(), r.is_error), ("called search_files\n[image content omitted]", false));
        let seen = s.seen.lock().unwrap().clone();
        let list = seen.iter().find(|(m, _, _)| m == "tools/list").unwrap();
        assert_eq!(list.1["authorization"], "Bearer tok-123");
        assert_eq!(list.1["mcp-session-id"], "sess-1", "the session id from initialize is sent afterwards");
        assert!(list.1["accept"].contains("text/event-stream"));
    }

    #[tokio::test]
    async fn status_codes_map_to_typed_errors_and_tokens_never_appear_in_them() {
        for (status, want) in [(401u16, McpError::NeedsReauth), (429, McpError::Throttled(None))] {
            let s = server(Arc::new(move |_m, _msg, _h| (status, "text/plain".into(), "tok-secret leaked in a body".into(), vec![])));
            let err = connect(s.port, &["127.0.0.1"], Some("tok-secret")).await.err().unwrap();
            assert_eq!(err, want);
            assert!(!err.to_string().contains("tok-secret"));
        }
        let s = server(Arc::new(|_m, _msg, _h| (403, "text/plain".into(), "admin approval required".into(), vec![])));
        assert!(matches!(connect(s.port, &["127.0.0.1"], None).await.err().unwrap(), McpError::Forbidden(m) if m.contains("admin approval")));
        let s = server(Arc::new(|_m, msg, _h| (200, "application/json".into(), json!({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32602, "message": "bad params"}}).to_string(), vec![])));
        assert_eq!(connect(s.port, &["127.0.0.1"], None).await.err().unwrap(), McpError::Rpc { code: -32602, message: "bad params".into() });
    }

    #[tokio::test]
    async fn an_old_protocol_revision_is_refused() {
        let s = server(Arc::new(|_m, msg, _h| (200, "application/json".into(), rpc(&msg["id"], json!({"protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {"name": "old", "version": "0"}})), vec![])));
        assert!(matches!(connect(s.port, &["127.0.0.1"], None).await.err().unwrap(), McpError::OldProtocol(_)));
    }

    #[tokio::test]
    async fn hosts_outside_the_list_are_refused_including_through_a_redirect() {
        assert!(matches!(connect(1, &["mcp.example.com"], None).await.err().unwrap(), McpError::HostRefused(_)));
        let s = server(Arc::new(|_m, _msg, _h| (307, "text/plain".into(), String::new(), vec![("location".into(), "http://localhost:9/evil".into())])));
        let err = connect(s.port, &["127.0.0.1"], Some("tok")).await.err().unwrap();
        assert!(matches!(err, McpError::HostRefused(ref h) if h == "localhost"), "{:?}", err);
        let s = server(Arc::new(|_m, _msg, _h| (302, "text/plain".into(), String::new(), vec![("location".into(), "/elsewhere".into())])));
        assert!(matches!(connect(s.port, &["127.0.0.1"], None).await.err().unwrap(), McpError::HostRefused(_)));
    }
}
