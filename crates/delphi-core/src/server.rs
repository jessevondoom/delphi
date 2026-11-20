//! The daemon: a token-authenticated JSON-RPC server on a Unix socket (`specs/005` §2.1).
//!
//! Security invariants enforced here:
//! * the socket is created `0600` inside a `0700` directory, and the peer's UID must equal ours;
//! * **every connection needs a capability token** (`console` or `narrator` scope). A connection
//!   that presents none, or a wrong one, is closed before it receives any event, snapshot or
//!   stream. Only `hello` and `auth.issue` may be called before authentication;
//! * tokens are issued over the spawn pipe, or to a peer that is attached to an interactive
//!   terminal (`auth.issue`), and never while an unsandboxed command is running (that needs a
//!   console confirmation);
//! * privileged RPCs require the `console` scope;
//! * **the app socket issues no token** (`specs/120`). A connection there is sent a fresh nonce and must answer with a
//!   signature by a key a terminal paired, over that nonce, this daemon's instance id, the protocol major and the socket's
//!   path; nothing else opens a session. A pairing ticket is given only to a console client attached to a terminal, is good
//!   once and for a minute. A refusal is recorded, and five in a minute close the socket to new connections for a minute.
//!   A session calls only the methods of the grants its key holds *now* ([`crate::app_keys::GRANTS`]), and ends the moment
//!   its key is revoked.

use crate::app_keys::{self, AppKey, CallRefused, KeyStore};
use crate::hub::{ClientQueue, Hub, DEFAULT_QUEUE_CAP};
use crate::peer::controlling_tty;
use crate::proto::{code, Notification, Request, Response, RpcError, Scope, PROTOCOL_MAJOR, RESTART_HINT};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Notify};

/// The only methods the narrator socket answers (besides the `hello` handshake). Read-only, by name.
pub const NARRATOR_METHODS: [&str; 12] = ["session.ping", "events.subscribe", "session.snapshot", "observe.snapshot", "observe.state", "kb.search", "kb.get", "usage.get", "budget.get", "approval.list", "permissions.get", "ratify.list"];

/// The only methods the relay socket answers (besides the `hello` handshake): read the outstanding cards and the
/// budget, answer a card through the relay's own narrower method, and listen. Everything else is refused by name.
pub const RELAY_METHODS: [&str; 5] = ["session.ping", "events.subscribe", "approval.list", "approval.relay_resolve", "budget.get"];

/// The only methods the host socket answers (besides the `hello` handshake): stage a lesson or a summary as untrusted
/// content, and keep or read one handoff (`specs/062` §8). A host cannot start work, read events or approve anything.
pub const HOST_METHODS: [&str; 32] = ["session.ping", "host.search_kb", "host.propose_note", "host.propose_kb_entry", "host.record_finding", "host.flag_wrong","host.request_test_authorization", "host.record_session_summary", "host.handoff_put", "host.handoff_get", "host.recall_sessions", "host.ingest_transcript", "host.request_help", "host.help_result", "host.request_council", "host.council_status", "host.council_result", "host.council_open", "host.council_frame", "host.council_brief", "host.council_submit", "host.council_choose", "host.request_research", "host.research_status", "host.research_result", "host.record_research", "host.research_feed", "host.cancel_job", "host.route_decided", "host.reads_seen", "host.ask_family", "host.ask_result"];

/// The console methods that pair, list, revoke and grant the desktop app's keys (`specs/120` §2, §5, §13.1) and that list and approve
/// a pending update (§13.2 item 5). All need the console scope; the ones that change what a key may do or approve something (all
/// but the three lists, `app.keys`, `app.pair_pending` and `app.update_pending`) also need the caller to be attached to a terminal,
/// as `auth.issue` does.
pub const APP_ADMIN_METHODS: [&str; 8] = ["auth.pair_ticket", "app.keys", "app.revoke", "app.grant", "app.pair_pending", "app.pair_approve", "app.update_pending", "app.update_approve"];
/// The two of [`APP_ADMIN_METHODS`] that belong to the extension, which holds the pending update (`Extension::app_update_admin`).
pub const APP_UPDATE_ADMIN_METHODS: [&str; 2] = ["app.update_pending", "app.update_approve"];

/// How many refusals on the app socket, within [`DaemonConfig::app_refusal_window`], close it to new connections.
pub const APP_REFUSALS: usize = 5;
/// Pairing tickets that may be outstanding at once.
pub const MAX_TICKETS: usize = 4;
/// Pairing requests by code that may be pending at once, how many may be made in an hour, and how many wrong codes cancel one
/// (`specs/120` §13.1 item 4).
pub const MAX_PAIR_PENDING: usize = 3;
pub const MAX_PAIR_PER_HOUR: usize = 6;
pub const MAX_WRONG_CODES: u8 = 3;
/// Unauthenticated connections on the app socket at once (a local program that opens many holds one place each, and no more).
pub const MAX_UNAUTH: usize = 8;
/// How long past its request's time an answered pairing request is kept for its app to collect.
const ANSWERED_KEPT: Duration = Duration::from_secs(60);

/// A request line larger than this closes the connection.
pub const MAX_LINE: usize = 1024 * 1024;

/// Where the daemon records what happened on the app socket (`app.paired`, `app.refused`, `app.revoked`, `app.granted`,
/// `app.expired`): the engine writes each as an event. The payload holds a key's short name, a grant's name and a fixed reason,
/// never anything a peer sent as free text.
pub type AppAudit = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// Is a pairing request still worth keeping? A pending one while it is good; an answered one (approved or cancelled) until its app has
/// collected the answer, or a minute past the request's time.
fn pair_kept(p: &PairReq, now: Instant, ttl: Duration) -> bool {
    now.duration_since(p.issued) <= if matches!(p.outcome, PairOutcome::Pending) { ttl } else { ttl + ANSWERED_KEPT }
}

/// A place among the unauthenticated connections, given back when the connection's handshake ends.
struct UnauthSlot<'a>(&'a Daemon);

impl Drop for UnauthSlot<'_> {
    fn drop(&mut self) {
        let mut st = self.0.app.lock().unwrap();
        st.unauth = st.unauth.saturating_sub(1);
    }
}

/// What an app is told when its pairing request by code is refused: the limit, by name. The reason word is also what the log keeps.
fn pair_refusal_text(reason: &str) -> &'static str {
    match reason {
        "too_many_pending" => "3 pairing requests are waiting for an operator already; try again in two minutes",
        "too_many_requests_this_hour" => "6 pairing requests were made in the last hour; try again later",
        "already_paired" => "this key is paired already",
        "key_record_full" => "the daemon holds as many keys as it keeps; revoke one first",
        "bad_public_key" => "that is not a public key",
        _ => "pairing was refused",
    }
}

/// A pairing ticket (`specs/120` §2 item 2): 32 random bytes, one use, good until `issued + ticket_ttl`. Kept in memory only.
struct Ticket {
    secret: String,
    /// The short name the key will have, decided when the ticket is made so the terminal can print it.
    name: String,
    issued: Instant,
}

/// What became of a pairing request by code, until the connection that filed it is told.
enum PairOutcome {
    Pending,
    /// The operator typed the code: the key is recorded with these grants.
    Approved(Vec<String>),
    /// Too many wrong codes.
    Cancelled,
}

/// A pairing request by code (`specs/120` §13.1): kept in memory only, bound to the public key and to this daemon's instance.
struct PairReq {
    /// The name the key will have, also the request's id on the console.
    name: String,
    /// Six characters of the code alphabet, no dash.
    code: String,
    public_key: String,
    app_version: String,
    issued: Instant,
    wrong: u8,
    outcome: PairOutcome,
}

#[derive(Default)]
struct AppState {
    tickets: Vec<Ticket>,
    pairs: Vec<PairReq>,
    /// When the requests of the last hour were made.
    pair_times: VecDeque<Instant>,
    refusals: VecDeque<Instant>,
    locked_until: Option<Instant>,
    /// Open sessions by client id: the key's name, and what ends the session at once.
    sessions: HashMap<u64, (String, Arc<Notify>)>,
    /// Connections on the app socket that have not proved a key.
    unauth: usize,
    /// The last few tickets that were used, with what became of each: (ticket name, outcome, key name).
    spent: VecDeque<(String, String, String)>,
}

/// What `dispatch` knows about the connection besides its scope.
struct Peer {
    pid: Option<i32>,
    /// The key's short name, on the app socket.
    app: Option<String>,
    /// Set when an app session's key is found gone from the record: the refusal just sent is the session's last word.
    gone: AtomicBool,
}

/// Who is calling an extension method.
#[derive(Debug, Clone, Copy)]
pub struct CallContext {
    pub scope: Scope,
    pub client_id: u64,
}

pub type RpcOutcome = Result<Value, RpcError>;
pub type RpcFuture = Pin<Box<dyn Future<Output = Option<RpcOutcome>> + Send>>;

/// Where the engine plugs in. `call` returns `None` for a method it does not handle. Scope checks for
/// privileged methods are the extension's job; the server has already authenticated the connection.
pub trait Extension: Send + Sync {
    fn call(&self, method: &str, params: Value, ctx: CallContext) -> RpcFuture;

    /// One of the `request` grant's methods ([`crate::app_keys::REQUEST_METHODS`], `specs/120` §4.2), asked on the app socket by the
    /// key named `key`. The server calls this only for an app session whose key holds the grant now, and calls nothing else for
    /// these methods: [`Extension::call`] is never offered them, so no console, narrator, relay or host client reaches them.
    /// An extension that does not answer them leaves this as it is.
    fn app_request(&self, key: &str, method: &str, params: Value) -> RpcFuture {
        let _ = (key, method, params);
        Box::pin(async { None })
    }

    /// `app.update_pending` and `app.update_approve` ([`APP_UPDATE_ADMIN_METHODS`], `specs/120` §13.2 item 5). The server has already
    /// checked that the caller is a console client and, for the approval, attached to a terminal. An extension that holds no pending
    /// update leaves this as it is.
    fn app_update_admin(&self, method: &str, params: Value) -> RpcFuture {
        let _ = (method, params);
        Box::pin(async { None })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TtyPolicy {
    /// Production: `auth.issue` requires the peer to have a controlling terminal.
    Required,
    /// Tests only.
    #[cfg(any(test, feature = "test-support"))]
    Disabled,
}

pub struct DaemonConfig {
    pub home: PathBuf,
    pub workspace: PathBuf,
    pub tty_policy: TtyPolicy,
    pub queue_cap: usize,
    /// How long an unauthenticated connection may stay silent.
    pub auth_timeout: Duration,
    pub binary_version: String,
    /// How long a pairing ticket is good for (`specs/120` §2 item 2: 60 seconds).
    pub ticket_ttl: Duration,
    /// The window in which [`APP_REFUSALS`] refusals close the app socket, and for how long (`specs/120` §3 item 4: a minute each).
    pub app_refusal_window: Duration,
    pub app_lockout: Duration,
    /// How long a pairing request by code is good for (`specs/120` §13.1 item 4: two minutes).
    pub pair_request_ttl: Duration,
    /// How often a connection that waits for its pairing request to be answered looks (`specs/120` §13.1).
    pub pair_poll: Duration,
    /// Connections on the app socket that have not proved a key (waiting in the handshake, or holding a pairing request) that may be open
    /// at once; one more is closed without a word.
    pub max_unauth: usize,
}

impl DaemonConfig {
    pub fn new(home: PathBuf, workspace: PathBuf) -> DaemonConfig {
        DaemonConfig {
            home,
            workspace,
            tty_policy: TtyPolicy::Required,
            queue_cap: DEFAULT_QUEUE_CAP,
            auth_timeout: Duration::from_secs(5),
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
            ticket_ttl: Duration::from_secs(60),
            app_refusal_window: Duration::from_secs(60),
            app_lockout: Duration::from_secs(60),
            pair_request_ttl: Duration::from_secs(120),
            pair_poll: Duration::from_millis(150),
            max_unauth: MAX_UNAUTH,
        }
    }
}

pub struct Daemon {
    cfg: DaemonConfig,
    pub hub: Hub,
    console_token: String,
    narrator_token: String,
    relay_token: String,
    host_token: String,
    /// True while an unsandboxed (Amber) command runs: no new tokens without console confirmation.
    pub amber_active: AtomicBool,
    uid: u32,
    shutdown: Notify,
    stopped: AtomicBool,
    extension: Mutex<Option<Arc<dyn Extension>>>,
    /// This daemon process, as 32 hex digits: part of what the app signs, so a signature made for one daemon is no use to another.
    instance: String,
    keys: KeyStore,
    /// Tickets, refusals and open app sessions. Also the lock under which the key record is read and written.
    app: Mutex<AppState>,
    app_audit: Mutex<Option<AppAudit>>,
}

pub fn random_token() -> io::Result<String> {
    use std::io::Read;
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{:02x}", x)).collect())
}

/// Random bytes from the same source as [`random_token`].
fn random_bytes<const N: usize>() -> io::Result<[u8; N]> {
    use std::io::Read;
    let mut b = [0u8; N];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b)
}

fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn read_line_limited<R: AsyncBufRead + Unpin>(r: &mut R) -> io::Result<Option<String>> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let avail = r.fill_buf().await?;
        if avail.is_empty() {
            return Ok(None); // EOF; a final unterminated line is dropped
        }
        match avail.iter().position(|b| *b == b'\n') {
            Some(i) => {
                buf.extend_from_slice(&avail[..i]);
                r.consume(i + 1);
                return Ok(Some(String::from_utf8_lossy(&buf).into_owned()));
            }
            None => {
                if buf.len() + avail.len() > MAX_LINE {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "request line too long"));
                }
                buf.extend_from_slice(avail);
                let n = avail.len();
                r.consume(n);
            }
        }
    }
}

impl Daemon {
    pub fn new(cfg: DaemonConfig) -> io::Result<Arc<Daemon>> {
        Daemon::with_hub(cfg, Hub::new())
    }

    /// A daemon that fans out through `hub`: the engine publishes to the same hub the clients attach to.
    pub fn with_hub(cfg: DaemonConfig, hub: Hub) -> io::Result<Arc<Daemon>> {
        Ok(Arc::new(Daemon {
            instance: random_token()?[..32].to_string(),
            keys: KeyStore::at(&cfg.home),
            app: Mutex::new(AppState::default()),
            app_audit: Mutex::new(None),
            cfg,
            hub,
            console_token: random_token()?,
            narrator_token: random_token()?,
            relay_token: random_token()?,
            host_token: random_token()?,
            amber_active: AtomicBool::new(false),
            uid: unsafe { libc::geteuid() },
            shutdown: Notify::new(),
            stopped: AtomicBool::new(false),
            extension: Mutex::new(None),
        }))
    }

    /// Install the engine. Methods the server does not implement itself are offered to it.
    pub fn set_extension(&self, ext: Arc<dyn Extension>) {
        *self.extension.lock().unwrap() = Some(ext);
    }

    /// Delivered to the spawning client over the readiness pipe; never written to disk or the environment.
    pub fn console_token(&self) -> &str {
        &self.console_token
    }

    pub fn narrator_token(&self) -> &str {
        &self.narrator_token
    }

    pub fn host_token(&self) -> &str {
        &self.host_token
    }

    pub fn relay_token(&self) -> &str {
        &self.relay_token
    }

    pub fn request_shutdown(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.shutdown.notify_waiters();
        self.shutdown.notify_one();
    }

    fn scope_for(&self, token: &str) -> Option<Scope> {
        // Compare against both so timing doesn't reveal which scope a near-miss was for.
        let c = ct_eq(token, &self.console_token);
        let n = ct_eq(token, &self.narrator_token);
        let r = ct_eq(token, &self.relay_token);
        let h = ct_eq(token, &self.host_token);
        if c {
            Some(Scope::Console)
        } else if n {
            Some(Scope::Narrator)
        } else if r {
            Some(Scope::Relay)
        } else if h {
            Some(Scope::Host)
        } else {
            None
        }
    }

    /// Accept console connections until shutdown is requested.
    pub async fn serve(self: Arc<Self>, listener: UnixListener) {
        self.serve_sockets(listener, None).await
    }

    /// Serve the console socket and, if given, the narrator socket. The socket a client connects to fixes its
    /// scope, whatever it says in `hello`: a narrator token is refused on the console socket and the other way
    /// round, and the narrator socket's dispatcher offers read-only methods and nothing else.
    pub async fn serve_sockets(self: Arc<Self>, console: UnixListener, narrator: Option<UnixListener>) {
        self.serve_all(console, narrator, None).await
    }

    /// As [`Daemon::serve_sockets`], with the relay socket (`specs/014`): a relay token is refused on the other
    /// sockets and theirs on this one, and its dispatcher offers [`RELAY_METHODS`] and nothing else.
    pub async fn serve_all(self: Arc<Self>, console: UnixListener, narrator: Option<UnixListener>, relay: Option<UnixListener>) {
        self.serve_everything(console, narrator, relay, None).await
    }

    /// As [`Daemon::serve_all`], with the host socket (`specs/062`): a host token is refused on the other sockets and
    /// theirs on this one, and its dispatcher offers [`HOST_METHODS`] and nothing else.
    pub async fn serve_everything(self: Arc<Self>, console: UnixListener, narrator: Option<UnixListener>, relay: Option<UnixListener>, host: Option<UnixListener>) {
        self.serve_with_app(console, narrator, relay, host, None).await
    }

    /// As [`Daemon::serve_everything`], with the app socket (`specs/120` §3): no token is issued or taken there. A connection
    /// proves a paired key by signing the daemon's nonce, and its dispatcher offers the methods of the key's grants and nothing else.
    pub async fn serve_with_app(self: Arc<Self>, console: UnixListener, narrator: Option<UnixListener>, relay: Option<UnixListener>, host: Option<UnixListener>, app: Option<UnixListener>) {
        tokio::spawn(self.hub.clone().run_coalescer());
        if let Some(l) = app {
            let me = self.clone();
            tokio::spawn(async move { me.accept_loop(l, Scope::App).await });
        }
        if let Some(l) = host {
            let me = self.clone();
            tokio::spawn(async move { me.accept_loop(l, Scope::Host).await });
        }
        if let Some(l) = narrator {
            let me = self.clone();
            tokio::spawn(async move { me.accept_loop(l, Scope::Narrator).await });
        }
        if let Some(l) = relay {
            let me = self.clone();
            tokio::spawn(async move { me.accept_loop(l, Scope::Relay).await });
        }
        self.accept_loop(console, Scope::Console).await
    }

    async fn accept_loop(self: Arc<Self>, listener: UnixListener, scope: Scope) {
        // The app signs the path of the socket it connected to. That is the path this listener was bound at, or the same file
        // with links resolved (`/tmp` and `/private/tmp` on macOS); a signature naming any other socket does not verify.
        let mut names: Vec<String> = Vec::new();
        if scope == Scope::App {
            if let Some(p) = listener.local_addr().ok().and_then(|a| a.as_pathname().map(|p| p.to_path_buf())) {
                names.push(p.to_string_lossy().into_owned());
                if let Ok(c) = std::fs::canonicalize(&p) {
                    let c = c.to_string_lossy().into_owned();
                    if !names.contains(&c) {
                        names.push(c);
                    }
                }
            }
        }
        let names = Arc::new(names);
        loop {
            if self.stopped.load(Ordering::SeqCst) {
                return;
            }
            tokio::select! {
                _ = self.shutdown.notified() => return,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let me = self.clone();
                    let names = names.clone();
                    tokio::spawn(async move {
                        // A connection that ended in an I/O error was silent; now it leaves a record (kind only, no peer data).
                        if let Err(e) = me.handle(stream, scope, &names).await {
                            if !matches!(e.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof) {
                                crate::elog::warn("daemon", "connection_error", "a client connection ended in an error", json!({"kind": format!("{:?}", e.kind()), "view": scope.as_str()}));
                            }
                        }
                    });
                }
            }
        }
    }

    async fn handle(self: Arc<Self>, stream: UnixStream, socket_scope: Scope, app_socket_names: &[String]) -> io::Result<()> {
        let cred = stream.peer_cred()?;
        if cred.uid() != self.uid {
            return Ok(()); // another OS user: drop without a word
        }
        let peer_pid = cred.pid();
        let (rd, mut wr) = stream.into_split();
        let mut rd = BufReader::new(rd);

        // ── The app socket: no token. A key proves itself, or the connection ends (`specs/120` §3).
        let mut app_key: Option<String> = None;
        if socket_scope == Scope::App {
            match self.app_handshake(&mut rd, &mut wr, app_socket_names).await? {
                Some(name) => app_key = Some(name),
                None => return Ok(()),
            }
        }

        // ── Unauthenticated phase: only `hello` and `auth.issue`. Anything else closes the connection.
        let scope = loop {
            if app_key.is_some() {
                break Scope::App;
            }
            let line = match tokio::time::timeout(self.cfg.auth_timeout, read_line_limited(&mut rd)).await {
                Ok(Ok(Some(l))) => l,
                _ => return Ok(()),
            };
            let Ok(req) = serde_json::from_str::<Request>(&line) else { return Ok(()) };
            match req.method.as_str() {
                "auth.issue" => {
                    let resp = self.auth_issue(&req, peer_pid, socket_scope);
                    write_line(&mut wr, &resp).await?;
                }
                "hello" => {
                    let token = req.params["token"].as_str().unwrap_or("");
                    // The token must be the one for the socket it came in on.
                    let Some(scope) = self.scope_for(token).filter(|s| *s == socket_scope) else { return Ok(()) };
                    let major = req.params["protocol_version"].as_u64();
                    if major != Some(PROTOCOL_MAJOR as u64) {
                        let mut r = Response::err(req.id.clone(), code::VERSION_SKEW, &format!("protocol major mismatch (daemon {}, client {:?}); {}", PROTOCOL_MAJOR, major, RESTART_HINT));
                        r.error.as_mut().unwrap().data = Some(json!({"daemon_protocol": PROTOCOL_MAJOR, "hint": RESTART_HINT}));
                        write_line(&mut wr, &r).await?;
                        return Ok(());
                    }
                    let r = Response::ok(req.id.clone(), json!({"protocol_version": PROTOCOL_MAJOR, "binary_version": self.cfg.binary_version, "scope": scope.as_str(), "pid": std::process::id()}));
                    write_line(&mut wr, &r).await?;
                    break scope;
                }
                _ => return Ok(()),
            }
        };

        // ── Authenticated phase.
        let queue = ClientQueue::new(self.cfg.queue_cap);
        let client_id = self.hub.attach(scope, queue.clone());
        let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<Response>();
        let writer = tokio::spawn({
            let queue = queue.clone();
            async move {
                loop {
                    tokio::select! {
                        biased;
                        r = resp_rx.recv() => match r {
                            Some(r) => if write_line(&mut wr, &r).await.is_err() { break },
                            None => break,
                        },
                        n = queue.pop() => match n {
                            Some(n) => if write_line(&mut wr, &n).await.is_err() { break },
                            None => break,
                        },
                    }
                }
            }
        });

        // An app session is registered under its key's name, with the one thing that ends it at once: revoking the key
        // (`specs/120` §2 item 8), or taking a grant from it.
        let closed = app_key.as_ref().map(|name| {
            let n = Arc::new(Notify::new());
            self.app.lock().unwrap().sessions.insert(client_id, (name.clone(), n.clone()));
            n
        });
        let peer = Peer { pid: peer_pid, app: app_key, gone: AtomicBool::new(false) };
        let session = async {
            while let Ok(Some(line)) = read_line_limited(&mut rd).await {
                let resp = match serde_json::from_str::<Request>(&line) {
                    Ok(req) => self.dispatch(req, scope, client_id, &peer).await,
                    Err(e) => Response::err(Value::Null, code::PARSE, &format!("parse error: {}", e)),
                };
                if resp_tx.send(resp).is_err() || peer.gone.load(Ordering::SeqCst) {
                    break;
                }
            }
        };
        let cut = match &closed {
            // Whatever the session is doing, a call in flight included, stops here.
            Some(c) => tokio::select! {
                biased;
                _ = c.notified() => true,
                _ = session => false,
            },
            None => {
                session.await;
                false
            }
        };
        if closed.is_some() {
            self.app.lock().unwrap().sessions.remove(&client_id);
        }
        self.hub.detach(client_id);
        drop(resp_tx);
        // An app session that ended on its own is let to send what it was answered (the refusal that says its key is gone);
        // one that was cut (a revoked key) is sent nothing more.
        let mut writer = writer;
        if closed.is_some() && !cut {
            let _ = tokio::time::timeout(Duration::from_millis(500), &mut writer).await;
        }
        writer.abort();
        Ok(())
    }

    fn auth_issue(&self, req: &Request, peer_pid: Option<i32>, socket_scope: Scope) -> Response {
        let scope = match req.params["scope"].as_str() {
            Some("console") => Scope::Console,
            Some("narrator") => Scope::Narrator,
            Some("relay") => Scope::Relay,
            Some("host") => Scope::Host,
            _ => return Response::err(req.id.clone(), code::INVALID_PARAMS, "scope must be `console`, `narrator`, `relay` or `host`"),
        };
        if scope != socket_scope {
            return Response::err(req.id.clone(), code::FORBIDDEN, &format!("this socket issues {} tokens only", socket_scope.as_str()));
        }
        match self.cfg.tty_policy {
            TtyPolicy::Required => {
                // The narrator scope is read-only (the daemon refuses anything that starts work or approves
                // anything for it), so it needs no terminal: `delphi observe --once` works from a script. The
                // scopes that can act, console and relay, keep the rule (operator decision, 2026-10-25). The host scope
                // acts on nothing either: it stages untrusted notes, and a coding tool's MCP process has no terminal.
                let tty = peer_pid.and_then(controlling_tty);
                if tty.is_none() && !matches!(scope, Scope::Narrator | Scope::Host) {
                    return Response::err(req.id.clone(), code::TTY_REQUIRED, "tokens are issued only to a client attached to an interactive terminal");
                }
            }
            #[cfg(any(test, feature = "test-support"))]
            TtyPolicy::Disabled => {}
        }
        if self.amber_active.load(Ordering::SeqCst) {
            return Response::err(req.id.clone(), code::CONFIRMATION_REQUIRED, "an unsandboxed command is running; a new attachment needs confirmation from an attached console");
        }
        let token = match scope {
            Scope::Console => &self.console_token,
            Scope::Narrator => &self.narrator_token,
            Scope::Relay => &self.relay_token,
            Scope::Host => &self.host_token,
            // Unreachable (no socket's scope is `app` here, and the name is not accepted above); refused all the same.
            Scope::App => return Response::err(req.id.clone(), code::FORBIDDEN, "the app socket issues no token"),
        };
        Response::ok(req.id.clone(), json!({"token": token, "scope": scope.as_str()}))
    }

    /// Install the recorder for what happens on the app socket. Without one (a bare daemon, a test) it reaches the error log only.
    pub fn set_app_audit(&self, audit: AppAudit) {
        *self.app_audit.lock().unwrap() = Some(audit);
    }

    /// This daemon process's id, as the app is sent it in the challenge.
    pub fn instance(&self) -> &str {
        &self.instance
    }

    fn audit(&self, kind: &str, payload: Value) {
        crate::elog::warn("daemon", "app_socket", kind, json!({"kind": kind, "reason": payload["reason"], "view": "app"}));
        let sink = self.app_audit.lock().unwrap().clone();
        if let Some(sink) = sink {
            sink(kind, payload);
        }
    }

    /// Is the peer attached to an interactive terminal (the test `auth.issue` uses)?
    fn at_terminal(&self, peer_pid: Option<i32>) -> bool {
        match self.cfg.tty_policy {
            TtyPolicy::Required => peer_pid.and_then(controlling_tty).is_some(),
            #[cfg(any(test, feature = "test-support"))]
            TtyPolicy::Disabled => true,
        }
    }

    fn now_unix() -> i64 {
        crate::time::parse_rfc3339(&crate::time::now_rfc3339()).unwrap_or(0)
    }

    /// The recorded keys, with those unused for 180 days removed (and the removal written). Called with the app lock held.
    fn live_keys(&self) -> Result<Vec<AppKey>, String> {
        let mut keys = self.keys.load()?;
        let gone = app_keys::prune(&mut keys, Daemon::now_unix());
        if !gone.is_empty() {
            self.keys.save(&keys)?;
            for name in gone {
                self.audit("app.expired", json!({"name": name, "reason": "unused for 180 days"}));
            }
        }
        Ok(keys)
    }

    /// Record a refusal on the app socket (`specs/120` §3 item 4). `name` is written only when it has a key name's shape, so
    /// nothing a peer made up reaches the log. The fifth refusal in the window closes the socket to new connections.
    fn app_refused(&self, reason: &str, name: Option<&str>) {
        let now = Instant::now();
        let locked = {
            let mut st = self.app.lock().unwrap();
            st.refusals.push_back(now);
            while st.refusals.front().is_some_and(|t| now.duration_since(*t) > self.cfg.app_refusal_window) {
                st.refusals.pop_front();
            }
            let locked = st.refusals.len() >= APP_REFUSALS;
            if locked {
                st.locked_until = Some(now + self.cfg.app_lockout);
                st.refusals.clear();
            }
            locked
        };
        self.audit("app.refused", json!({"reason": reason, "name": name.filter(|n| app_keys::is_name(n)), "locked_out": locked}));
    }

    /// The whole of what an unproven connection on the app socket may do: `app.pair` once, with a ticket, and `app.hello` once,
    /// with a signature over the nonce it was just sent. `Some(name)` is a proven key; `None` means the connection is over.
    async fn app_handshake<R: AsyncBufRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(&self, rd: &mut R, wr: &mut W, socket_names: &[String]) -> io::Result<Option<String>> {
        let _slot = {
            let mut st = self.app.lock().unwrap();
            if st.locked_until.is_some_and(|t| Instant::now() < t) {
                return Ok(None); // closed to new connections: not a word, and no new record for each knock
            }
            if st.unauth >= self.cfg.max_unauth {
                crate::elog::warn("daemon", "app_socket", "too many connections that have not proved a key; one was closed", json!({"limit": self.cfg.max_unauth, "view": "app"}));
                return Ok(None);
            }
            st.unauth += 1;
            UnauthSlot(self)
        };
        let nonce = random_token()?;
        write_line(wr, &Notification::new("app.challenge", json!({"nonce": nonce, "instance": self.instance, "protocol_version": PROTOCOL_MAJOR}))).await?;
        let mut paired = false;
        loop {
            let line = match tokio::time::timeout(self.cfg.auth_timeout, read_line_limited(rd)).await {
                Ok(Ok(Some(l))) => l,
                _ => return Ok(None),
            };
            let Ok(req) = serde_json::from_str::<Request>(&line) else { return Ok(None) };
            match req.method.as_str() {
                "app.pair" if !paired => match self.app_pair(&req.params) {
                    Ok(v) => {
                        paired = true;
                        write_line(wr, &Response::ok(req.id.clone(), v)).await?;
                    }
                    Err(reason) => {
                        self.app_refused(reason, None);
                        write_line(wr, &Response::err(req.id.clone(), code::FORBIDDEN, "pairing was refused")).await?;
                        return Ok(None);
                    }
                },
                // A key that has none asks to be approved by a code its operator types at a terminal (`specs/120` §13.1). The
                // connection waits here for the answer and then goes on to `app.hello` as a connection that paired does.
                "app.pair_request" if !paired => match self.app_pair_request(&req.params) {
                    Ok((name, code)) => {
                        paired = true;
                        let ttl = self.cfg.pair_request_ttl.as_secs();
                        write_line(wr, &Response::ok(req.id.clone(), json!({"code": code, "name": name, "expires_in": ttl}))).await?;
                        let result = loop {
                            tokio::time::sleep(self.cfg.pair_poll).await;
                            if let Some(r) = self.pair_poll(&name) {
                                break r;
                            }
                        };
                        match result {
                            Ok(grants) => write_line(wr, &Notification::new("app.pair_result", json!({"outcome": "approved", "name": name, "grants": grants}))).await?,
                            Err(outcome) => {
                                write_line(wr, &Notification::new("app.pair_result", json!({"outcome": outcome}))).await?;
                                return Ok(None);
                            }
                        }
                    }
                    Err(reason) => {
                        self.app_refused(reason, None);
                        write_line(wr, &Response::err(req.id.clone(), code::FORBIDDEN, pair_refusal_text(reason))).await?;
                        return Ok(None);
                    }
                },
                "app.hello" => {
                    let major = req.params["protocol_version"].as_u64();
                    if major != Some(PROTOCOL_MAJOR as u64) {
                        let mut r = Response::err(req.id.clone(), code::VERSION_SKEW, &format!("protocol major mismatch (daemon {}, client {:?}); {}", PROTOCOL_MAJOR, major, RESTART_HINT));
                        r.error.as_mut().unwrap().data = Some(json!({"daemon_protocol": PROTOCOL_MAJOR, "hint": RESTART_HINT}));
                        write_line(wr, &r).await?;
                        return Ok(None);
                    }
                    let name = req.params["name"].as_str().unwrap_or("");
                    let signature = req.params["signature"].as_str().unwrap_or("");
                    return match self.app_prove(name, signature, &nonce, socket_names) {
                        Ok(key) => {
                            write_line(wr, &Response::ok(req.id.clone(), json!({"protocol_version": PROTOCOL_MAJOR, "binary_version": self.cfg.binary_version, "scope": Scope::App.as_str(), "pid": std::process::id(), "name": key.name, "grants": key.grants}))).await?;
                            Ok(Some(key.name))
                        }
                        Err(reason) => {
                            self.app_refused(reason, Some(name));
                            // One answer for every reason: which of them it was is in the operator's log, not on the wire.
                            write_line(wr, &Response::err(req.id.clone(), code::FORBIDDEN, "the app key was refused")).await?;
                            Ok(None)
                        }
                    };
                }
                _ => return Ok(None),
            }
        }
    }

    /// `app.pair_request {public_key, app_version}` (`specs/120` §13.1 items 1 and 4): file a request, and return its key name and
    /// the code to show. A second request from the same public key replaces its first.
    fn app_pair_request(&self, params: &Value) -> Result<(String, String), &'static str> {
        let public_key = params["public_key"].as_str().unwrap_or("");
        let Some(fingerprint) = app_keys::fingerprint(public_key) else { return Err("bad_public_key") };
        let app_version = app_keys::clean_version(params["app_version"].as_str().unwrap_or(""));
        let now = Instant::now();
        let (name, code) = {
            let mut st = self.app.lock().unwrap();
            let ttl = self.cfg.pair_request_ttl;
            st.pairs.retain(|p| pair_kept(p, now, ttl));
            while st.pair_times.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(3600)) {
                st.pair_times.pop_front();
            }
            if st.pair_times.len() >= MAX_PAIR_PER_HOUR {
                return Err("too_many_requests_this_hour");
            }
            // A second request from the same key replaces its first, unless the first was already answered and its app has not yet
            // collected the answer: that one stays until it is delivered or runs out.
            st.pairs.retain(|p| p.public_key != public_key || !matches!(p.outcome, PairOutcome::Pending));
            if st.pairs.iter().filter(|p| matches!(p.outcome, PairOutcome::Pending)).count() >= MAX_PAIR_PENDING {
                return Err("too_many_pending");
            }
            let keys = self.live_keys().map_err(|_| "key_record_unreadable")?;
            if keys.iter().any(|k| k.public_key == public_key) {
                return Err("already_paired");
            }
            if keys.len() >= app_keys::MAX_KEYS {
                return Err("key_record_full");
            }
            let tag = random_bytes::<3>().map_err(|_| "no_randomness")?;
            let name = format!("app-{}", app_keys::hex(&tag));
            if keys.iter().any(|k| k.name == name) || st.pairs.iter().any(|p| p.name == name) {
                return Err("name_taken");
            }
            let code = app_keys::code_from(&random_bytes::<32>().map_err(|_| "no_randomness")?).ok_or("no_randomness")?;
            st.pair_times.push_back(now);
            st.pairs.push(PairReq { name: name.clone(), code: code.replace('-', ""), public_key: public_key.to_string(), app_version: app_version.clone(), issued: now, wrong: 0, outcome: PairOutcome::Pending });
            (name, code)
        };
        self.audit("app.pair_requested", json!({"name": name, "fingerprint": fingerprint, "app_version": app_version}));
        Ok((name, code))
    }

    /// What became of the request `name`, for the connection that filed it: `None` while it waits; `Ok(grants)` once approved;
    /// `Err` with the word for a request that ran out or was cancelled. The request is removed when it is reported.
    fn pair_poll(&self, name: &str) -> Option<Result<Vec<String>, &'static str>> {
        {
            let mut st = self.app.lock().unwrap();
            let Some(i) = st.pairs.iter().position(|p| p.name == name) else { return Some(Err("expired")) };
            match &st.pairs[i].outcome {
                PairOutcome::Approved(g) => {
                    let g = g.clone();
                    st.pairs.remove(i);
                    return Some(Ok(g));
                }
                PairOutcome::Cancelled => {
                    st.pairs.remove(i);
                    return Some(Err("cancelled"));
                }
                PairOutcome::Pending => {}
            }
            if Instant::now().duration_since(st.pairs[i].issued) <= self.cfg.pair_request_ttl {
                return None;
            }
            st.pairs.remove(i);
        }
        self.audit("app.refused", json!({"reason": "expired_request", "name": name, "locked_out": false}));
        Some(Err("expired"))
    }

    /// `app.pair {ticket, public_key, app_version}` (`specs/120` §2 item 5). The ticket is spent whatever follows.
    fn app_pair(&self, params: &Value) -> Result<Value, &'static str> {
        let ticket = params["ticket"].as_str().unwrap_or("");
        let public_key = params["public_key"].as_str().unwrap_or("");
        let now = Instant::now();
        let mut st = self.app.lock().unwrap();
        // Compare with every outstanding ticket, so timing does not say which one a near-miss was for.
        let hit = st.tickets.iter().enumerate().fold(None, |hit, (i, t)| if ct_eq(ticket, &t.secret) { Some(i) } else { hit });
        let Some(i) = hit else { return Err("bad_ticket") };
        let t = st.tickets.remove(i);
        let outcome = self.app_pair_with(&t, public_key, params["app_version"].as_str().unwrap_or(""), now);
        // What became of the ticket, for the terminal that asked for it (`app.keys`): its name, the outcome and the key's name.
        let (how, key_name) = match &outcome {
            Ok(v) => (if v["already_paired"] == true { "already_paired" } else { "paired" }, v["name"].as_str().unwrap_or("").to_string()),
            Err(reason) => (*reason, String::new()),
        };
        st.spent.push_back((t.name.clone(), how.to_string(), key_name));
        while st.spent.len() > MAX_TICKETS * 2 {
            st.spent.pop_front();
        }
        drop(st);
        if let Ok(v) = &outcome {
            if v["already_paired"] != true {
                self.audit("app.paired", json!({"name": v["name"], "grants": v["grants"], "app_version": v["app_version"]}));
            }
        }
        outcome
    }

    /// The rest of `app.pair`, for a ticket that was found (and is already spent). Called with the app lock held.
    fn app_pair_with(&self, t: &Ticket, public_key: &str, app_version: &str, now: Instant) -> Result<Value, &'static str> {
        if now.duration_since(t.issued) > self.cfg.ticket_ttl {
            return Err("expired_ticket");
        }
        if app_keys::unhex::<32>(public_key).is_none() {
            return Err("bad_public_key");
        }
        let mut keys = self.live_keys().map_err(|_| "key_record_unreadable")?;
        // The same key paired again keeps its record, grants included: a ticket vouches for the app, it does not widen a key
        // the operator narrowed.
        if let Some(k) = keys.iter().find(|k| k.public_key == public_key) {
            return Ok(json!({"name": k.name, "grants": k.grants, "already_paired": true}));
        }
        if keys.len() >= app_keys::MAX_KEYS || keys.iter().any(|k| k.name == t.name) {
            return Err("key_record_full");
        }
        let at = crate::time::now_rfc3339();
        let key = AppKey { name: t.name.clone(), public_key: public_key.to_string(), paired_at: at.clone(), last_used: at, app_version: app_keys::clean_version(app_version), grants: app_keys::PAIRING_GRANTS.iter().map(|g| g.to_string()).collect(), update_auto: false };
        keys.push(key.clone());
        self.keys.save(&keys).map_err(|_| "key_record_unwritable")?;
        Ok(json!({"name": key.name, "grants": key.grants, "app_version": key.app_version}))
    }

    /// Does `signature` by the key named `name` cover this connection? On success the key's last use is written.
    fn app_prove(&self, name: &str, signature: &str, nonce: &str, socket_names: &[String]) -> Result<AppKey, &'static str> {
        if !app_keys::is_name(name) {
            return Err("unknown_key");
        }
        let _st = self.app.lock().unwrap();
        let mut keys = self.live_keys().map_err(|_| "key_record_unreadable")?;
        // A revoked key is no longer in the record: it is unknown, as a name that never existed is.
        let Some(key) = keys.iter_mut().find(|k| k.name == name) else { return Err("unknown_key") };
        if !socket_names.iter().any(|socket| key.signed(nonce, &self.instance, PROTOCOL_MAJOR, socket, signature)) {
            return Err("bad_signature");
        }
        key.last_used = crate::time::now_rfc3339();
        let key = key.clone();
        self.keys.save(&keys).map_err(|_| "key_record_unwritable")?;
        Ok(key)
    }

    /// End every open session of the key `name`, at once. Returns how many there were.
    fn close_sessions(st: &AppState, name: &str) -> usize {
        let open: Vec<&Arc<Notify>> = st.sessions.values().filter(|(n, _)| n == name).map(|(_, c)| c).collect();
        for c in &open {
            c.notify_one();
        }
        open.len()
    }

    /// May this app session call `method` now? The key's record is read for every call (`specs/120` §3 item 5).
    fn app_may_call(&self, peer: &Peer, method: &str) -> Result<(), String> {
        let name = peer.app.as_deref().unwrap_or("");
        let _writers = self.app.lock().unwrap();
        let held = self.keys.load().ok().and_then(|keys| keys.into_iter().find(|k| k.name == name)).map(|k| k.grants);
        let Some(held) = held else {
            // The key is gone (or the record cannot be read): the session has nothing to stand on.
            peer.gone.store(true, Ordering::SeqCst);
            return Err("this key is no longer paired".into());
        };
        match app_keys::check_call(&app_keys::GRANTS, &held, method) {
            Ok(()) => Ok(()),
            Err(CallRefused::NoGrant) => Err(format!("`{}` is listed under no grant of the app socket", method)),
            Err(CallRefused::Lacks(g)) => Err(format!("`{}` needs the `{}` grant, which this key does not hold", method, g)),
        }
    }

    /// The console's four methods for the app's keys ([`APP_ADMIN_METHODS`]).
    fn app_admin(&self, req: &Request, scope: Scope, peer: &Peer) -> Response {
        let id = req.id.clone();
        if scope != Scope::Console {
            return Response::err(id, code::FORBIDDEN, "this method requires the console scope");
        }
        // Pairing, revoking and granting are the operator's, at a terminal (`specs/120` §8): a console token alone (a script's,
        // a headless run's) is not enough.
        if !matches!(req.method.as_str(), "app.keys" | "app.pair_pending") && !self.at_terminal(peer.pid) {
            return Response::err(id, code::TTY_REQUIRED, "the app's keys are paired, revoked and granted only by a client attached to an interactive terminal");
        }
        let name = req.params["name"].as_str().unwrap_or("");
        let mut st = self.app.lock().unwrap();
        match req.method.as_str() {
            "auth.pair_ticket" => {
                if self.amber_active.load(Ordering::SeqCst) {
                    return Response::err(id, code::CONFIRMATION_REQUIRED, "an unsandboxed command is running; pairing waits until it ends");
                }
                let now = Instant::now();
                st.tickets.retain(|t| now.duration_since(t.issued) <= self.cfg.ticket_ttl);
                if st.tickets.len() >= MAX_TICKETS {
                    return Response::err(id, code::FORBIDDEN, "too many pairing tickets are outstanding; try again in a minute");
                }
                let keys = match self.live_keys() {
                    Ok(k) => k,
                    Err(e) => return Response::err(id, code::INTERNAL, &e),
                };
                if keys.len() >= app_keys::MAX_KEYS {
                    return Response::err(id, code::FORBIDDEN, &format!("{} keys are paired already; revoke one first (`delphi app revoke <name>`)", keys.len()));
                }
                let (Ok(secret), Ok(tag)) = (random_token(), random_token()) else { return Response::err(id, code::INTERNAL, "no randomness for a ticket") };
                let key_name = format!("app-{}", &tag[..6]);
                if keys.iter().any(|k| k.name == key_name) || st.tickets.iter().any(|t| t.name == key_name) {
                    return Response::err(id, code::INTERNAL, "the new key's name is taken; ask again");
                }
                st.tickets.push(Ticket { secret: secret.clone(), name: key_name.clone(), issued: now });
                Response::ok(id, json!({"ticket": secret, "name": key_name, "expires_in": self.cfg.ticket_ttl.as_secs(), "grants": app_keys::PAIRING_GRANTS}))
            }
            // The requests waiting for a code (`specs/120` §13.1 item 3). Never the codes: a terminal is told what to ask for, not given it.
            "app.pair_pending" => {
                let now = Instant::now();
                let ttl = self.cfg.pair_request_ttl;
                st.pairs.retain(|p| pair_kept(p, now, ttl));
                let rows: Vec<Value> = st
                    .pairs
                    .iter()
                    .filter(|p| matches!(p.outcome, PairOutcome::Pending))
                    .map(|p| {
                        let age = now.duration_since(p.issued).as_secs();
                        json!({"request_id": p.name, "fingerprint": app_keys::fingerprint(&p.public_key), "app_version": p.app_version, "age_secs": age, "expires_in": ttl.as_secs().saturating_sub(age)})
                    })
                    .collect();
                Response::ok(id, json!({"pending": rows}))
            }
            "app.pair_approve" => {
                let request_id = req.params["request_id"].as_str().unwrap_or("");
                let mut grants: Vec<String> = app_keys::CODE_GRANTS.iter().map(|g| g.to_string()).collect();
                if let Some(extra) = req.params["grants"].as_array() {
                    for g in extra {
                        // Only `request` may be added at this prompt: a key paired by code never starts with more.
                        match g.as_str() {
                            Some("request") => {
                                if !grants.iter().any(|x| x == "request") {
                                    grants.push("request".into());
                                }
                            }
                            Some("read") => {}
                            _ => return Response::err(id, code::INVALID_PARAMS, "a key approved by code may be given `request` at this prompt and nothing else"),
                        }
                    }
                }
                let now = Instant::now();
                let Some(i) = st.pairs.iter().position(|p| p.name == request_id && matches!(p.outcome, PairOutcome::Pending)) else { return Response::err(id, code::NOT_FOUND, "no such pending request") };
                if now.duration_since(st.pairs[i].issued) > self.cfg.pair_request_ttl {
                    st.pairs.remove(i);
                    drop(st);
                    self.audit("app.refused", json!({"reason": "expired_request", "name": request_id, "locked_out": false}));
                    return Response::err(id, code::FORBIDDEN, "that request has expired; the app asks again");
                }
                let typed = app_keys::normalize_code(req.params["code"].as_str().unwrap_or(""));
                let right = typed.as_deref().is_some_and(|t| ct_eq(t, &st.pairs[i].code));
                if !right {
                    st.pairs[i].wrong += 1;
                    let cancelled = st.pairs[i].wrong >= MAX_WRONG_CODES;
                    if cancelled {
                        st.pairs[i].outcome = PairOutcome::Cancelled;
                    }
                    drop(st);
                    self.audit("app.refused", json!({"reason": if cancelled { "wrong_code_limit" } else { "wrong_code" }, "name": request_id, "locked_out": false}));
                    let why = if cancelled { format!("that is not the code, {MAX_WRONG_CODES} times: the request is cancelled and the app asks again") } else { "that is not the code shown in the app's window".to_string() };
                    return Response::err(id, code::FORBIDDEN, &why);
                }
                let mut keys = match self.live_keys() {
                    Ok(k) => k,
                    Err(e) => return Response::err(id, code::INTERNAL, &e),
                };
                if keys.len() >= app_keys::MAX_KEYS || keys.iter().any(|k| k.name == st.pairs[i].name || k.public_key == st.pairs[i].public_key) {
                    return Response::err(id, code::FORBIDDEN, "the key record is full or already holds this key; `delphi app list`");
                }
                let at = crate::time::now_rfc3339();
                let key = AppKey { name: st.pairs[i].name.clone(), public_key: st.pairs[i].public_key.clone(), paired_at: at.clone(), last_used: at, app_version: st.pairs[i].app_version.clone(), grants: grants.clone(), update_auto: false };
                keys.push(key.clone());
                if let Err(e) = self.keys.save(&keys) {
                    return Response::err(id, code::INTERNAL, &e);
                }
                st.pairs[i].outcome = PairOutcome::Approved(grants.clone());
                drop(st);
                self.audit("app.pair_approved", json!({"name": key.name, "grants": grants, "app_version": key.app_version}));
                Response::ok(id, json!({"name": key.name, "grants": grants}))
            }
            "app.keys" => match self.live_keys() {
                Ok(keys) => {
                    let rows: Vec<Value> = keys.iter().map(|k| json!({"name": k.name, "grants": k.grants, "update_auto": k.update_auto, "paired_at": k.paired_at, "last_used": k.last_used, "app_version": k.app_version, "sessions": st.sessions.values().filter(|(n, _)| *n == k.name).count()})).collect();
                    // The names of tickets not yet used or run out: the terminal that asked for one watches for its name to leave this list.
                    let now = Instant::now();
                    let pending: Vec<&str> = st.tickets.iter().filter(|t| now.duration_since(t.issued) <= self.cfg.ticket_ttl).map(|t| t.name.as_str()).collect();
                    let spent: Vec<Value> = st.spent.iter().map(|(t, how, key)| json!({"ticket": t, "outcome": how, "key": key})).collect();
                    Response::ok(id, json!({"keys": rows, "pending": pending, "spent": spent, "grants": app_keys::GRANTS.iter().map(|g| json!({"name": g.name, "methods": g.methods})).collect::<Vec<_>>()}))
                }
                Err(e) => Response::err(id, code::INTERNAL, &e),
            },
            "app.revoke" => {
                let mut keys = match self.live_keys() {
                    Ok(k) => k,
                    Err(e) => return Response::err(id, code::INTERNAL, &e),
                };
                if !keys.iter().any(|k| k.name == name) {
                    return Response::err(id, code::NOT_FOUND, "no such key");
                }
                keys.retain(|k| k.name != name);
                if let Err(e) = self.keys.save(&keys) {
                    return Response::err(id, code::INTERNAL, &e);
                }
                let closed = Daemon::close_sessions(&st, name);
                drop(st);
                self.audit("app.revoked", json!({"name": name, "sessions_closed": closed}));
                Response::ok(id, json!({"revoked": name, "sessions_closed": closed}))
            }
            "app.grant" => {
                let grant = req.params["grant"].as_str().unwrap_or("");
                let remove = req.params["remove"].as_bool().unwrap_or(false);
                // `auto` is the operator's standing authorization for `app.update_apply` (`specs/120` §13.2 item 6) and nothing else's.
                let auto = req.params["auto"].as_bool().unwrap_or(false);
                if auto && (grant != "update" || remove) {
                    return Response::err(id, code::INVALID_PARAMS, "`auto` goes with giving `update`: a standing authorization covers `app.update_apply` and nothing else, and `--remove` ends it");
                }
                // Only a grant the table defines (`specs/120` §5 item 1): a name from anywhere else is refused, not stored.
                if !app_keys::is_grant(grant) {
                    return Response::err(id, code::INVALID_PARAMS, &format!("no such grant; the grants are: {}", app_keys::GRANTS.iter().map(|g| g.name).collect::<Vec<_>>().join(", ")));
                }
                let mut keys = match self.live_keys() {
                    Ok(k) => k,
                    Err(e) => return Response::err(id, code::INTERNAL, &e),
                };
                let Some(key) = keys.iter_mut().find(|k| k.name == name) else { return Response::err(id, code::NOT_FOUND, "no such key") };
                let held = key.grants.iter().any(|g| g == grant);
                let was_auto = key.update_auto;
                if remove {
                    key.grants.retain(|g| g != grant);
                    if grant == "update" {
                        key.update_auto = false;
                    }
                } else if !held {
                    key.grants.push(grant.to_string());
                }
                if auto {
                    key.update_auto = true;
                }
                let (grants, now_auto) = (key.grants.clone(), key.update_auto);
                if let Err(e) = self.keys.save(&keys) {
                    return Response::err(id, code::INTERNAL, &e);
                }
                // A grant taken away ends the key's open sessions: what a session set going under it (an event subscription)
                // does not outlive it. The app connects again and holds what the record now says.
                let closed = if remove && held { Daemon::close_sessions(&st, name) } else { 0 };
                drop(st);
                let changed = held == remove || was_auto != now_auto;
                if changed {
                    self.audit("app.granted", json!({"name": name, "grant": grant, "removed": remove, "auto": now_auto, "sessions_closed": closed}));
                }
                Response::ok(id, json!({"name": name, "grants": grants, "update_auto": now_auto, "changed": changed, "sessions_closed": closed}))
            }
            _ => Response::err(id, code::METHOD_NOT_FOUND, "unknown method"),
        }
    }

    /// Hand one of the `request` grant's methods to the extension, **as a task of its own**. Revoking a key, or taking a grant
    /// from it, ends its session by dropping the session's future, a call in flight included (`specs/120` §2 item 8); a request
    /// that had begun to start work must not be cut halfway through a write, so the work is not part of that future. A request
    /// that was already being answered when its key was revoked runs to its own end and its answer is thrown away.
    async fn app_request(&self, req: Request, key: String) -> Response {
        let id = req.id.clone();
        let Some(ext) = self.extension.lock().unwrap().clone() else { return Response::err(id, code::METHOD_NOT_FOUND, "unknown method") };
        let (method, params) = (req.method, req.params);
        let work = tokio::spawn(async move { ext.app_request(&key, &method, params).await });
        match work.await {
            Ok(Some(Ok(v))) => Response::ok(id, v),
            Ok(Some(Err(e))) => Response { jsonrpc: "2.0".into(), id, result: None, error: Some(e) },
            Ok(None) => Response::err(id, code::METHOD_NOT_FOUND, "unknown method"),
            Err(_) => Response::err(id, code::INTERNAL, "the request could not be answered"),
        }
    }

    /// `app.update_pending` (a list, console scope) and `app.update_approve` (console scope and a terminal, as a key's grant is):
    /// checked here, answered by the extension that holds the pending request.
    async fn app_update_admin(&self, req: Request, scope: Scope, peer: &Peer) -> Response {
        let id = req.id.clone();
        if scope != Scope::Console {
            return Response::err(id, code::FORBIDDEN, "this method requires the console scope");
        }
        if req.method == "app.update_approve" && !self.at_terminal(peer.pid) {
            return Response::err(id, code::TTY_REQUIRED, "an update is approved only by a client attached to an interactive terminal");
        }
        let Some(ext) = self.extension.lock().unwrap().clone() else { return Response::err(id, code::METHOD_NOT_FOUND, "unknown method") };
        match ext.app_update_admin(&req.method, req.params).await {
            Some(Ok(v)) => Response::ok(id, v),
            Some(Err(e)) => Response { jsonrpc: "2.0".into(), id, result: None, error: Some(e) },
            None => Response::err(id, code::METHOD_NOT_FOUND, "unknown method"),
        }
    }

    async fn dispatch(&self, req: Request, scope: Scope, client_id: u64, peer: &Peer) -> Response {
        let id = req.id.clone();
        // The app socket offers the methods of the grants the key holds at this moment, by name, and nothing else
        // (`specs/120` §4): a method is unreachable from it until a row is added to the grant table on purpose.
        if scope == Scope::App {
            if let Err(why) = self.app_may_call(peer, &req.method) {
                crate::elog::warn("daemon", "rpc_forbidden", "a method outside the key's grants was refused", json!({"method": req.method, "kind": "app"}));
                return Response::err(id, code::FORBIDDEN, &why);
            }
        }
        // The `request` grant's methods are the app socket's alone (`specs/120` §4.2): a session whose key holds the grant has them
        // answered by the extension's own entry for them; from any other socket they do not exist.
        if app_keys::REQUEST_METHODS.contains(&req.method.as_str()) || app_keys::UPDATE_METHODS.contains(&req.method.as_str()) {
            return match (scope, peer.app.as_deref()) {
                (Scope::App, Some(key)) => self.app_request(req, key.to_string()).await,
                _ => Response::err(id, code::METHOD_NOT_FOUND, "unknown method"),
            };
        }
        // The narrator socket offers a fixed list of read methods. A new method is unreachable from it until
        // someone adds it here on purpose, so nothing mutating can get through by being left off a deny list.
        if scope == Scope::Narrator && !NARRATOR_METHODS.contains(&req.method.as_str()) {
            crate::elog::warn("daemon", "rpc_forbidden", "a method outside the narrator list was refused", json!({"method": req.method, "kind": "narrator"}));
            return Response::err(id, code::FORBIDDEN, &format!("`{}` is not available on the read-only narrator socket", req.method));
        }
        if scope == Scope::Relay && !RELAY_METHODS.contains(&req.method.as_str()) {
            crate::elog::warn("daemon", "rpc_forbidden", "a method outside the relay list was refused", json!({"method": req.method, "kind": "relay"}));
            return Response::err(id, code::FORBIDDEN, &format!("`{}` is not available on the relay socket", req.method));
        }
        if scope == Scope::Host && !HOST_METHODS.contains(&req.method.as_str()) {
            crate::elog::warn("daemon", "rpc_forbidden", "a method outside the host list was refused", json!({"method": req.method, "kind": "host"}));
            return Response::err(id, code::FORBIDDEN, &format!("`{}` is not available on the host socket", req.method));
        }
        // The pending update is the extension's to hold, behind the same scope and terminal checks as the keys.
        if APP_UPDATE_ADMIN_METHODS.contains(&req.method.as_str()) {
            return self.app_update_admin(req, scope, peer).await;
        }
        // The app's keys are the server's own business, and the console's only: never offered to the engine.
        if APP_ADMIN_METHODS.contains(&req.method.as_str()) {
            return self.app_admin(&req, scope, peer);
        }
        // The server's own methods first; everything else may belong to the engine.
        if matches!(req.method.as_str(), "session.ping" | "events.subscribe" | "daemon.shutdown") {
            return self.dispatch_builtin(req, scope, client_id);
        }
        let ext = self.extension.lock().unwrap().clone();
        if let Some(ext) = ext {
            if let Some(outcome) = ext.call(&req.method, req.params.clone(), CallContext { scope, client_id }).await {
                return match outcome {
                    Ok(v) => Response::ok(id, v),
                    Err(e) => {
                        // The method and the code only: never the params, and the message only at debug level.
                        if crate::elog::enabled(crate::elog::Level::Error) {
                            crate::elog::error("daemon", "rpc_error", &format!("{} failed with code {}", req.method, e.code), json!({"method": req.method, "kind": e.code}));
                            crate::elog::debug_with("daemon", "rpc_error_message", || (e.message.clone(), json!({"method": req.method})));
                        }
                        Response { jsonrpc: "2.0".into(), id, result: None, error: Some(e) }
                    }
                };
            }
        }
        self.dispatch_builtin(req, scope, client_id)
    }

    fn dispatch_builtin(&self, req: Request, scope: Scope, client_id: u64) -> Response {
        let id = req.id.clone();
        match req.method.as_str() {
            "session.ping" => Response::ok(id, json!("pong")),
            "events.subscribe" => {
                self.hub.subscribe(client_id);
                Response::ok(id, json!({"subscribed": true}))
            }
            "session.snapshot" => Response::ok(
                id,
                json!({"clients": {"console": self.hub.client_count(Some(Scope::Console)), "narrator": self.hub.client_count(Some(Scope::Narrator)), "relay": self.hub.client_count(Some(Scope::Relay)), "host": self.hub.client_count(Some(Scope::Host)), "app": self.hub.client_count(Some(Scope::App))}, "missions": []}),
            ),
            // Privileged: console scope only.
            "approval.resolve" | "budget.set" | "permissions.set" | "ratify.apply" | "mission.rewind" | "daemon.shutdown" => {
                if scope != Scope::Console {
                    return Response::err(id, code::FORBIDDEN, "this method requires the console scope");
                }
                if req.method == "daemon.shutdown" {
                    self.request_shutdown();
                    return Response::ok(id, json!({"shutting_down": true}));
                }
                // The approval broker, budget and ratification land with the agent loop (Step 2, M5).
                Response::err(id, code::NOT_FOUND, "no such approval or target")
            }
            _ => Response::err(id, code::METHOD_NOT_FOUND, "unknown method"),
        }
    }
}

async fn write_line<W: tokio::io::AsyncWrite + Unpin, T: serde::Serialize>(w: &mut W, v: &T) -> io::Result<()> {
    let mut s = serde_json::to_string(v).map_err(io::Error::other)?;
    s.push('\n');
    w.write_all(s.as_bytes()).await?;
    w.flush().await
}
