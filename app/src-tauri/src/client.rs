//! The app's own client of the daemon (spec 111 §2, §3.2): newline-delimited JSON-RPC 2.0 over a Unix socket. It can send
//! only the methods of [`Method`]: twenty reads and, since spec 120 K5, the four requests of the daemon's `request` grant
//! ([`Method::REQUESTS`]). No other method that acts can be named from here, let alone from the window.
//!
//! It attaches in one of two ways, and each fixes what it attached as:
//! * on the **narrator** socket, with the token that socket issues ([`Client::attach`]): scope `narrator` and nothing else;
//! * on the **app** socket (spec 120 §3), with no token: the daemon sends a challenge and the app answers with a signature by
//!   its paired key ([`Client::attach_app`]); or, once, with the ticket a terminal handed it ([`Client::pair`]); or, with no
//!   key and no ticket, by asking to be approved with a code its operator types at a terminal ([`Client::pair_request`], spec 120
//!   §13.1). The app can show the code and wait; it cannot approve.
//!
//! `auth.issue`, `hello`, `app.pair`, `app.pair_request` and `app.hello` are sent by those functions only. They are how a connection begins, not
//! methods the app can call, and [`Method`] does not grow by them.

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

/// Every daemon method the app may call. Three kinds; the first two are reads:
/// * the nine of spec 111 §2.2 ([`Method::NARRATOR`]), a subset of the daemon's narrator allowlist, sent on either socket;
/// * the encyclopedia's eleven of spec 117 §3 ([`Method::PAIRED_ONLY`]): rows of the daemon's `read` grant that are **not**
///   narrator methods, because `library.source` returns a private source's text. They are sent only on a connection that
///   proved a paired key: [`Client::call`] refuses them on any other, before a byte is written;
/// * the four requests of spec 120 §4.2 ([`Method::REQUESTS`]): the daemon's `request` grant, each asking the daemon to
///   start something the operator could start at a terminal. Sent only on a connection that proved a paired key, as the
///   encyclopedia's reads are.
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
    PagesList,
    PagesGet,
    PagesVersions,
    PagesLinks,
    TopicsList,
    TopicsGet,
    LibrarySource,
    LibraryClaim,
    MemoryRoles,
    MemoryRole,
    ChangesSince,
    ResearchContinue,
    ResearchAsk,
    PageRebuild,
    PageDispute,
    /// The one method of the daemon's `update` grant (spec 120 §13.2): it takes no parameters.
    UpdateApply,
}

impl Method {
    /// The requests of spec 120 §4.2 (`delphi_core::app_keys::REQUEST_METHODS`), in the daemon's order.
    pub const REQUESTS: [Method; 4] = [Method::ResearchContinue, Method::ResearchAsk, Method::PageRebuild, Method::PageDispute];
    /// The `update` grant's methods (spec 120 §13.2): one, and it carries no parameter.
    pub const UPDATES: [Method; 1] = [Method::UpdateApply];
    /// The reads of spec 111 §2.2: narrator methods.
    pub const NARRATOR: [Method; 9] = [Method::Ping, Method::Subscribe, Method::SessionSnapshot, Method::ObserveState, Method::ObserveSnapshot, Method::KbSearch, Method::KbGet, Method::UsageGet, Method::ApprovalList];
    /// The encyclopedia's reads (spec 117 §3 item 1; `delphi_core::app_keys::ENCYCLOPEDIA_READS`), in the daemon's order.
    pub const PAIRED_ONLY: [Method; 11] = [
        Method::PagesList,
        Method::PagesGet,
        Method::PagesVersions,
        Method::PagesLinks,
        Method::TopicsList,
        Method::TopicsGet,
        Method::LibrarySource,
        Method::LibraryClaim,
        Method::MemoryRoles,
        Method::MemoryRole,
        Method::ChangesSince,
    ];
    pub const ALL: [Method; 25] = [
        Method::Ping,
        Method::Subscribe,
        Method::SessionSnapshot,
        Method::ObserveState,
        Method::ObserveSnapshot,
        Method::KbSearch,
        Method::KbGet,
        Method::UsageGet,
        Method::ApprovalList,
        Method::PagesList,
        Method::PagesGet,
        Method::PagesVersions,
        Method::PagesLinks,
        Method::TopicsList,
        Method::TopicsGet,
        Method::LibrarySource,
        Method::LibraryClaim,
        Method::MemoryRoles,
        Method::MemoryRole,
        Method::ChangesSince,
        Method::ResearchContinue,
        Method::ResearchAsk,
        Method::PageRebuild,
        Method::PageDispute,
        Method::UpdateApply,
    ];

    /// Whether this read is sent only on a connection that proved a paired key.
    pub fn paired_only(self) -> bool {
        Method::PAIRED_ONLY.contains(&self)
    }

    /// Whether this is one of the four requests or the update: a method that asks the daemon to start something.
    pub fn asks(self) -> bool {
        Method::REQUESTS.contains(&self) || Method::UPDATES.contains(&self)
    }

    /// The grant a key must hold for this method to be sent at all (the daemon holds the same line): `update` for the update, else `request`.
    pub fn grant(self) -> &'static str {
        if Method::UPDATES.contains(&self) { "update" } else { "request" }
    }

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
            Method::PagesList => "pages.list",
            Method::PagesGet => "pages.get",
            Method::PagesVersions => "pages.versions",
            Method::PagesLinks => "pages.links",
            Method::TopicsList => "topics.list",
            Method::TopicsGet => "topics.get",
            Method::LibrarySource => "library.source",
            Method::LibraryClaim => "library.claim",
            Method::MemoryRoles => "memory.roles",
            Method::MemoryRole => "memory.role",
            Method::ChangesSince => "changes.since",
            Method::ResearchContinue => "app.research_continue",
            Method::ResearchAsk => "app.research_ask",
            Method::PageRebuild => "app.page_rebuild",
            Method::PageDispute => "app.page_dispute",
            Method::UpdateApply => "app.update_apply",
        }
    }
}

/// What the window is told when it asks for an encyclopedia read without a paired key.
pub const NOT_PAIRED: &str = "the encyclopedia is read with a paired key: run `delphi app` in a terminal once";

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The daemon answered with an error: its code and its words.
    Rpc { code: i64, message: String },
    /// The daemon closed the connection (a restart, a refused token).
    Closed,
    /// Something on the wire was not the protocol.
    Protocol(String),
    /// A paired-only read was asked for on a connection that proved no key. Nothing was sent.
    NotPaired,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Rpc { code, message } => write!(f, "the daemon refused ({code}): {message}"),
            Error::Closed => write!(f, "the daemon closed the connection"),
            Error::Protocol(m) => write!(f, "{m}"),
            Error::NotPaired => write!(f, "{NOT_PAIRED}"),
        }
    }
}

impl Error {
    /// Whether the connection is gone, as opposed to one call having been refused.
    pub fn is_disconnect(&self) -> bool {
        matches!(self, Error::Io(_) | Error::Closed | Error::Protocol(_))
    }
}

/// What the daemon sends first on its app socket: a nonce made for this connection, and the id of this daemon process.
#[derive(Debug, Clone)]
pub struct Challenge {
    pub nonce: String,
    pub instance: String,
}

/// What the daemon answered to `app.pair_request`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairAsked {
    pub code: String,
    pub name: String,
    pub expires_in: u64,
}

/// A pairing code as the daemon shows it, `K7Q-4M2`: what the window is willing to print, whatever it was sent.
pub fn is_code(s: &str) -> bool {
    let ok = |c: char| c.is_ascii_uppercase() && !matches!(c, 'I' | 'L' | 'O') || ('2'..='9').contains(&c);
    s.len() == 7 && s.as_bytes()[3] == b'-' && s.chars().enumerate().all(|(i, c)| i == 3 || ok(c))
}

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    /// A line being read when a timeout struck: `read_until` keeps what arrived, and the next read goes on from it.
    partial: Vec<u8>,
    /// Notifications that arrived while a call waited for its reply.
    pending: VecDeque<Value>,
    /// Whether this connection proved a paired key (`app.hello` answered with scope `app` and the key's own name). Set only
    /// by [`Client::prove`]; a narrator connection never has it.
    proved: bool,
}

impl Client {
    pub fn connect(path: &Path) -> io::Result<Client> {
        let stream = UnixStream::connect(path)?;
        Ok(Client { reader: BufReader::new(stream.try_clone()?), writer: stream, next_id: 1, partial: Vec::new(), pending: VecDeque::new(), proved: false })
    }

    /// Connect and attach as a read-only client: ask the narrator socket for its token, then say hello with it.
    pub fn attach(path: &Path, version: &str) -> Result<Client, Error> {
        Client::attach_hello(path, version, Duration::from_secs(10)).map(|(c, _)| c)
    }

    /// [`Client::attach`] with its own timeout for every read and write, which also returns what the daemon answered to
    /// `hello` (its pid, its version, and its instance when it names one). Discovery (spec 130 §3) uses it to tell a
    /// daemon that is there from a stale file, and to read a summary, over a connection it closes afterwards.
    pub fn attach_hello(path: &Path, version: &str, timeout: Duration) -> Result<(Client, Value), Error> {
        let mut c = Client::connect(path).map_err(Error::Io)?;
        c.set_read_timeout(Some(timeout));
        let _ = c.writer.set_write_timeout(Some(timeout));
        let issued = c.raw("auth.issue", json!({ "scope": "narrator" }))?;
        let token = issued["token"].as_str().ok_or_else(|| Error::Protocol("the daemon issued no token".into()))?.to_string();
        if issued["scope"].as_str() != Some("narrator") {
            return Err(Error::Protocol("the daemon issued a token for another scope; refusing it".into()));
        }
        let hello = c.raw("hello", json!({ "protocol_version": PROTOCOL_MAJOR, "binary_version": format!("delphi-app {version}"), "token": token }))?;
        if hello["scope"].as_str() != Some("narrator") {
            return Err(Error::Protocol("attached with a scope other than narrator; closing".into()));
        }
        Ok((c, hello))
    }

    /// Connect to the app socket and read what the daemon sends first: its challenge (spec 120 §3 item 2). A daemon that
    /// closes the connection instead (it does for a minute after five refusals) is `Error::Closed`.
    pub fn knock(path: &Path) -> Result<(Client, Challenge), Error> {
        let mut c = Client::connect(path).map_err(Error::Io)?;
        c.set_read_timeout(Some(Duration::from_secs(10)));
        let first = c.next_notification()?.ok_or_else(|| Error::Protocol("the daemon sent no challenge".into()))?;
        if first["method"].as_str() != Some("app.challenge") {
            return Err(Error::Protocol("the daemon's first word was not a challenge".into()));
        }
        let p = &first["params"];
        if p["protocol_version"].as_u64() != Some(PROTOCOL_MAJOR) {
            return Err(Error::Protocol("the daemon speaks another protocol version; update the app or Delphi".into()));
        }
        let text = |k: &str| p[k].as_str().map(String::from).ok_or_else(|| Error::Protocol("the daemon's challenge is incomplete".into()));
        Ok((c, Challenge { nonce: text("nonce")?, instance: text("instance")? }))
    }

    /// `app.pair` (spec 120 §2 item 5): hand the daemon the ticket a terminal gave this app, and the public half of its key.
    /// Returns the key's short name, which the terminal printed. The ticket is spent whatever the answer.
    pub fn pair(&mut self, ticket: &str, public_key: &str, version: &str) -> Result<String, Error> {
        let r = self.raw("app.pair", json!({ "ticket": ticket, "public_key": public_key, "app_version": version }))?;
        let name = r["name"].as_str().unwrap_or("");
        if !crate::key::is_name(name) {
            return Err(Error::Protocol("the daemon did not name the key".into()));
        }
        Ok(name.to_string())
    }

    /// `app.pair_request` (spec 120 §13.1 item 1): ask the daemon to approve this public key by a code its operator types at a
    /// terminal. The daemon answers with the code to show, the name the key will have and how long the request is good for.
    pub fn pair_request(&mut self, public_key: &str, version: &str) -> Result<PairAsked, Error> {
        let r = self.raw("app.pair_request", json!({ "public_key": public_key, "app_version": version }))?;
        let (code, name) = (r["code"].as_str().unwrap_or(""), r["name"].as_str().unwrap_or(""));
        if !crate::key::is_name(name) || !is_code(code) {
            return Err(Error::Protocol("the daemon did not give a code and a name".into()));
        }
        Ok(PairAsked { code: code.to_string(), name: name.to_string(), expires_in: r["expires_in"].as_u64().unwrap_or(120).min(600) })
    }

    /// `app.hello` (spec 120 §3 item 2): answer the challenge with the key's name and its signature over this connection.
    /// Returns the grants the key holds now. There is no token: the session lasts as long as the connection.
    pub fn prove(&mut self, key: &crate::key::AppKey, challenge: &Challenge, socket: &Path, version: &str) -> Result<Vec<String>, Error> {
        let signature = socket.to_str().and_then(|s| crate::key::sign_challenge(key.seed(), &challenge.nonce, &challenge.instance, PROTOCOL_MAJOR, s)).ok_or_else(|| Error::Protocol("what the daemon sent is not a challenge this app signs".into()))?;
        let hello = self.raw("app.hello", json!({ "name": key.name, "signature": signature, "protocol_version": PROTOCOL_MAJOR, "binary_version": format!("delphi-app {version}") }))?;
        if hello["scope"].as_str() != Some("app") || hello["name"].as_str() != Some(key.name.as_str()) {
            return Err(Error::Protocol("attached as something other than this app's key; closing".into()));
        }
        self.proved = true;
        Ok(hello["grants"].as_array().map(|g| g.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default())
    }

    /// Connect to the app socket and open a session with a paired key: the challenge, then the signature.
    pub fn attach_app(path: &Path, version: &str, key: &crate::key::AppKey) -> Result<(Client, Vec<String>), Error> {
        let (mut c, challenge) = Client::knock(path)?;
        let grants = c.prove(key, &challenge, path, version)?;
        Ok((c, grants))
    }

    pub fn set_read_timeout(&self, t: Option<Duration>) {
        let _ = self.reader.get_ref().set_read_timeout(t);
    }

    /// Whether this connection proved a paired key.
    pub fn proved(&self) -> bool {
        self.proved
    }

    /// One of the methods the app is allowed. A paired-only read (spec 117 §3) or a request (spec 120 §4.2) on a connection
    /// that proved no key is refused here, with nothing written to the socket: the narrator socket is never asked for the
    /// encyclopedia, and never asked to start anything.
    pub fn call(&mut self, method: Method, params: Value) -> Result<Value, Error> {
        if (method.paired_only() || method.asks()) && !self.proved {
            return Err(Error::NotPaired);
        }
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
