//! The app's own client of the daemon's narrator socket (spec 111 §2, §3.2): newline-delimited JSON-RPC 2.0 over a Unix socket.
//! It attaches with scope `narrator` and nothing else, and can send only the methods of [`Method`], so a method that acts cannot
//! be named from here, let alone from the window.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

/// The protocol major the daemon speaks (`delphi_core::proto::PROTOCOL_MAJOR`).
pub const PROTOCOL_MAJOR: u64 = 1;
/// A reply or notification longer than this is refused instead of read into memory without end.
const MAX_LINE: usize = 64 * 1024 * 1024;

/// Every daemon method the app may call (spec 111 §2.2): a subset of the daemon's narrator allowlist, all reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Ping,
    Subscribe,
    SessionSnapshot,
    ObserveState,
    ObserveSnapshot,
    KbSearch,
    KbGet,
    UsageGet,
    ApprovalList,
}

impl Method {
    pub const ALL: [Method; 9] = [Method::Ping, Method::Subscribe, Method::SessionSnapshot, Method::ObserveState, Method::ObserveSnapshot, Method::KbSearch, Method::KbGet, Method::UsageGet, Method::ApprovalList];

    pub fn name(self) -> &'static str {
        match self {
            Method::Ping => "session.ping",
            Method::Subscribe => "events.subscribe",
            Method::SessionSnapshot => "session.snapshot",
            Method::ObserveState => "observe.state",
            Method::ObserveSnapshot => "observe.snapshot",
            Method::KbSearch => "kb.search",
            Method::KbGet => "kb.get",
            Method::UsageGet => "usage.get",
            Method::ApprovalList => "approval.list",
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The daemon answered with an error: its code and its words.
    Rpc { code: i64, message: String },
    /// The daemon closed the connection (a restart, a refused token).
    Closed,
    /// Something on the wire was not the protocol.
    Protocol(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Rpc { code, message } => write!(f, "the daemon refused ({code}): {message}"),
            Error::Closed => write!(f, "the daemon closed the connection"),
            Error::Protocol(m) => write!(f, "{m}"),
        }
    }
}

impl Error {
    /// Whether the connection is gone, as opposed to one call having been refused.
    pub fn is_disconnect(&self) -> bool {
        matches!(self, Error::Io(_) | Error::Closed | Error::Protocol(_))
    }
}

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    /// A line being read when a timeout struck: `read_until` keeps what arrived, and the next read goes on from it.
    partial: Vec<u8>,
    /// Notifications that arrived while a call waited for its reply.
    pending: VecDeque<Value>,
}

impl Client {
    pub fn connect(path: &Path) -> io::Result<Client> {
        let stream = UnixStream::connect(path)?;
        Ok(Client { reader: BufReader::new(stream.try_clone()?), writer: stream, next_id: 1, partial: Vec::new(), pending: VecDeque::new() })
    }

    /// Connect and attach as a read-only client: ask the narrator socket for its token, then say hello with it.
    pub fn attach(path: &Path, version: &str) -> Result<Client, Error> {
        let mut c = Client::connect(path).map_err(Error::Io)?;
        c.set_read_timeout(Some(Duration::from_secs(10)));
        let issued = c.raw("auth.issue", json!({ "scope": "narrator" }))?;
        let token = issued["token"].as_str().ok_or_else(|| Error::Protocol("the daemon issued no token".into()))?.to_string();
        if issued["scope"].as_str() != Some("narrator") {
            return Err(Error::Protocol("the daemon issued a token for another scope; refusing it".into()));
        }
        let hello = c.raw("hello", json!({ "protocol_version": PROTOCOL_MAJOR, "binary_version": format!("delphi-app {version}"), "token": token }))?;
        if hello["scope"].as_str() != Some("narrator") {
            return Err(Error::Protocol("attached with a scope other than narrator; closing".into()));
        }
        Ok(c)
    }

    pub fn set_read_timeout(&self, t: Option<Duration>) {
        let _ = self.reader.get_ref().set_read_timeout(t);
    }

    /// One of the reads the app is allowed.
    pub fn call(&mut self, method: Method, params: Value) -> Result<Value, Error> {
        self.raw(method.name(), params)
    }

    fn raw(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).map_err(|e| Error::Protocol(e.to_string()))?;
        line.push(b'\n');
        self.writer.write_all(&line).map_err(Error::Io)?;
        loop {
            let msg = self.read_message()?;
            if msg.get("id").is_none() || msg["id"].is_null() {
                self.pending.push_back(msg);
                continue;
            }
            if msg["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(err) = msg.get("error").filter(|e| !e.is_null()) {
                return Err(Error::Rpc { code: err["code"].as_i64().unwrap_or(0), message: err["message"].as_str().unwrap_or("").to_string() });
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// The next notification. With a read timeout set, `Ok(None)` means nothing arrived in that time.
    pub fn next_notification(&mut self) -> Result<Option<Value>, Error> {
        if let Some(n) = self.pending.pop_front() {
            return Ok(Some(n));
        }
        loop {
            match self.read_message() {
                Ok(msg) if msg.get("id").is_none() || msg["id"].is_null() => return Ok(Some(msg)),
                Ok(_) => continue,
                Err(Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    fn read_message(&mut self) -> Result<Value, Error> {
        loop {
            match self.reader.read_until(b'\n', &mut self.partial) {
                Ok(0) => return Err(Error::Closed),
                Ok(_) if self.partial.ends_with(b"\n") => {
                    let line = std::mem::take(&mut self.partial);
                    if line.iter().all(|b| b.is_ascii_whitespace()) {
                        continue;
                    }
                    return serde_json::from_slice(&line).map_err(|_| Error::Protocol("the daemon sent a line that is not JSON".into()));
                }
                // The stream ended in the middle of a line.
                Ok(_) => return Err(Error::Closed),
                Err(e) => {
                    if self.partial.len() > MAX_LINE {
                        return Err(Error::Protocol("the daemon sent a line too long to read".into()));
                    }
                    return Err(Error::Io(e));
                }
            }
        }
    }
}
