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
//! * privileged RPCs require the `console` scope.

use crate::hub::{ClientQueue, Hub, DEFAULT_QUEUE_CAP};
use crate::peer::controlling_tty;
use crate::proto::{code, Request, Response, RpcError, Scope, PROTOCOL_MAJOR, RESTART_HINT};
use serde_json::{json, Value};
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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
pub const HOST_METHODS: [&str; 7] = ["session.ping", "host.propose_note", "host.record_session_summary", "host.handoff_put", "host.handoff_get", "host.request_help", "host.help_result"];

/// A request line larger than this closes the connection.
pub const MAX_LINE: usize = 1024 * 1024;

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
}

pub fn random_token() -> io::Result<String> {
    use std::io::Read;
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{:02x}", x)).collect())
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
        tokio::spawn(self.hub.clone().run_coalescer());
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
        loop {
            if self.stopped.load(Ordering::SeqCst) {
                return;
            }
            tokio::select! {
                _ = self.shutdown.notified() => return,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let me = self.clone();
                    tokio::spawn(async move {
                        // A connection that ended in an I/O error was silent; now it leaves a record (kind only, no peer data).
                        if let Err(e) = me.handle(stream, scope).await {
                            if !matches!(e.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof) {
                                crate::elog::warn("daemon", "connection_error", "a client connection ended in an error", json!({"kind": format!("{:?}", e.kind()), "view": scope.as_str()}));
                            }
                        }
                    });
                }
            }
        }
    }

    async fn handle(self: Arc<Self>, stream: UnixStream, socket_scope: Scope) -> io::Result<()> {
        let cred = stream.peer_cred()?;
        if cred.uid() != self.uid {
            return Ok(()); // another OS user: drop without a word
        }
        let peer_pid = cred.pid();
        let (rd, mut wr) = stream.into_split();
        let mut rd = BufReader::new(rd);

        // ── Unauthenticated phase: only `hello` and `auth.issue`. Anything else closes the connection.
        let scope = loop {
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

        while let Ok(Some(line)) = read_line_limited(&mut rd).await {
            let resp = match serde_json::from_str::<Request>(&line) {
                Ok(req) => self.dispatch(req, scope, client_id).await,
                Err(e) => Response::err(Value::Null, code::PARSE, &format!("parse error: {}", e)),
            };
            if resp_tx.send(resp).is_err() {
                break;
            }
        }
        self.hub.detach(client_id);
        drop(resp_tx);
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
        };
        Response::ok(req.id.clone(), json!({"token": token, "scope": scope.as_str()}))
    }

    async fn dispatch(&self, req: Request, scope: Scope, client_id: u64) -> Response {
        let id = req.id.clone();
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
                json!({"clients": {"console": self.hub.client_count(Some(Scope::Console)), "narrator": self.hub.client_count(Some(Scope::Narrator)), "relay": self.hub.client_count(Some(Scope::Relay)), "host": self.hub.client_count(Some(Scope::Host))}, "missions": []}),
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
