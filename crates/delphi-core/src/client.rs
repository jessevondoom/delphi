//! A small blocking client for the daemon protocol (used by the CLI and the tests).

use crate::proto::{Response, Scope, PROTOCOL_MAJOR};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    /// Notifications that arrived while waiting for a response.
    pending: VecDeque<Value>,
}

#[derive(Debug)]
pub enum ClientError {
    Io(io::Error),
    /// The daemon answered with a JSON-RPC error.
    Rpc { code: i64, message: String, data: Option<Value> },
    /// The daemon closed the connection (no token, wrong token, or refused).
    Closed,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "{}", e),
            ClientError::Rpc { code, message, .. } => write!(f, "daemon error {}: {}", code, message),
            ClientError::Closed => write!(f, "the daemon closed the connection"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(e: io::Error) -> Self {
        ClientError::Io(e)
    }
}

impl Client {
    pub fn from_stream(stream: UnixStream) -> io::Result<Client> {
        Ok(Client { reader: BufReader::new(stream.try_clone()?), writer: stream, next_id: 1, pending: VecDeque::new() })
    }

    pub fn connect(path: &std::path::Path) -> io::Result<Client> {
        Client::from_stream(UnixStream::connect(path)?)
    }

    pub fn set_read_timeout(&self, t: Option<Duration>) -> io::Result<()> {
        self.writer.set_read_timeout(t)
    }

    fn send(&mut self, method: &str, params: Value) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_string(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).map_err(io::Error::other)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        Ok(id)
    }

    fn read_value(&mut self) -> Result<Value, ClientError> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(ClientError::Closed);
        }
        serde_json::from_str(&line).map_err(|e| ClientError::Io(io::Error::new(io::ErrorKind::InvalidData, e)))
    }

    /// Call a method and wait for its response; notifications that arrive meanwhile are kept.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
        let id = self.send(method, params)?;
        loop {
            let v = self.read_value()?;
            if v.get("id").is_none() {
                self.pending.push_back(v);
                continue;
            }
            let resp: Response = serde_json::from_value(v).map_err(|e| ClientError::Io(io::Error::new(io::ErrorKind::InvalidData, e)))?;
            if resp.id != json!(id) {
                continue;
            }
            return match resp.error {
                Some(e) => Err(ClientError::Rpc { code: e.code, message: e.message, data: e.data }),
                None => Ok(resp.result.unwrap_or(Value::Null)),
            };
        }
    }

    /// The next server-pushed notification (`event`, `delta.frame`, `resync_required`).
    pub fn next_notification(&mut self) -> Result<Value, ClientError> {
        if let Some(n) = self.pending.pop_front() {
            return Ok(n);
        }
        loop {
            let v = self.read_value()?;
            if v.get("id").is_none() {
                return Ok(v);
            }
        }
    }

    /// Authenticate with a token. A protocol-major mismatch comes back as an `Rpc` error whose
    /// `data.hint` says how to recover.
    pub fn hello(&mut self, token: &str) -> Result<Value, ClientError> {
        self.hello_as(token, PROTOCOL_MAJOR)
    }

    pub fn hello_as(&mut self, token: &str, protocol_major: u32) -> Result<Value, ClientError> {
        self.call("hello", json!({"protocol_version": protocol_major, "binary_version": env!("CARGO_PKG_VERSION"), "token": token}))
    }

    /// Ask for a token as an interactive client (`specs/005` §2.1 TTY challenge).
    pub fn issue_token(&mut self, scope: Scope) -> Result<String, ClientError> {
        let r = self.call("auth.issue", json!({"scope": scope.as_str()}))?;
        r["token"].as_str().map(String::from).ok_or(ClientError::Closed)
    }
}
