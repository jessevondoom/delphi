//! The daemon's wire protocol: newline-delimited JSON-RPC 2.0 over a Unix socket (`specs/005` §2.1).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol major. A client and daemon with different majors refuse to talk (`specs/005` §2.1 item 3).
pub const PROTOCOL_MAJOR: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Privileged RPCs (`approval.resolve`, `/budget`, `/permissions`, ratification, `/rewind`) plus reading.
    Console,
    /// Read-only (pause and inspect arrive in Step 5).
    Narrator,
    /// A relay that carries approval cards to the operator on another channel and brings answers back
    /// (`specs/014`). It sees only approval and task-outcome events, and can answer an Amber card or deny any
    /// card. It cannot change the budget, the permission mode, a ratification or anything else.
    Relay,
    /// A coding tool on this machine that uses Delphi through `delphi mcp` (`specs/061`). It can stage lessons and
    /// summaries as untrusted content and keep one handoff, nothing else: [`crate::server::HOST_METHODS`].
    Host,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Console => "console",
            Scope::Narrator => "narrator",
            Scope::Relay => "relay",
            Scope::Host => "host",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// Server-pushed message (no `id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

pub mod code {
    pub const PARSE: i64 = -32700;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const FORBIDDEN: i64 = -32001;
    pub const VERSION_SKEW: i64 = -32002;
    pub const NOT_FOUND: i64 = -32004;
    pub const TTY_REQUIRED: i64 = -32005;
    pub const CONFIRMATION_REQUIRED: i64 = -32006;
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Response {
        Response { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }

    pub fn err(id: Value, code: i64, message: &str) -> Response {
        Response { jsonrpc: "2.0".into(), id, result: None, error: Some(RpcError { code, message: message.into(), data: None }) }
    }
}

impl Notification {
    pub fn new(method: &str, params: Value) -> Notification {
        Notification { jsonrpc: "2.0".into(), method: method.into(), params }
    }
}

/// Hint shown when the client refuses to attach to a daemon of another major.
pub const RESTART_HINT: &str = "run `delphi daemon restart` to replace the running daemon";
