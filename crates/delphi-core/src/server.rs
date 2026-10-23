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
use crate::proto::{code, Request, Response, Scope, PROTOCOL_MAJOR, RESTART_HINT};
use serde_json::{json, Value};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Notify};

/// A request line larger than this closes the connection.
pub const MAX_LINE: usize = 1024 * 1024;

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
    /// True while an unsandboxed (Amber) command runs: no new tokens without console confirmation.
    pub amber_active: AtomicBool,
    uid: u32,
    shutdown: Notify,
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
        Ok(Arc::new(Daemon {
            cfg,
            hub: Hub::new(),
            console_token: random_token()?,
            narrator_token: random_token()?,
            amber_active: AtomicBool::new(false),
            uid: unsafe { libc::geteuid() },
            shutdown: Notify::new(),
        }))
    }

    /// Delivered to the spawning client over the readiness pipe; never written to disk or the environment.
    pub fn console_token(&self) -> &str {
        &self.console_token
    }

    pub fn narrator_token(&self) -> &str {
        &self.narrator_token
    }

    pub fn request_shutdown(&self) {
        self.shutdown.notify_one();
    }

    fn scope_for(&self, token: &str) -> Option<Scope> {
        // Compare against both so timing doesn't reveal which scope a near-miss was for.
        let c = ct_eq(token, &self.console_token);
        let n = ct_eq(token, &self.narrator_token);
        if c {
            Some(Scope::Console)
        } else if n {
            Some(Scope::Narrator)
        } else {
            None
        }
    }

    /// Accept connections until shutdown is requested.
    pub async fn serve(self: Arc<Self>, listener: UnixListener) {
        tokio::spawn(self.hub.clone().run_coalescer());
        loop {
            tokio::select! {
                _ = self.shutdown.notified() => return,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let me = self.clone();
                    tokio::spawn(async move { let _ = me.handle(stream).await; });
                }
            }
        }
    }

    async fn handle(self: Arc<Self>, stream: UnixStream) -> io::Result<()> {
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
                    let resp = self.auth_issue(&req, peer_pid);
                    write_line(&mut wr, &resp).await?;
                }
                "hello" => {
                    let token = req.params["token"].as_str().unwrap_or("");
                    let Some(scope) = self.scope_for(token) else { return Ok(()) };
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
                Ok(req) => self.dispatch(req, scope, client_id),
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

    fn auth_issue(&self, req: &Request, peer_pid: Option<i32>) -> Response {
        let scope = match req.params["scope"].as_str() {
            Some("console") => Scope::Console,
            Some("narrator") => Scope::Narrator,
            _ => return Response::err(req.id.clone(), code::INVALID_PARAMS, "scope must be `console` or `narrator`"),
        };
        match self.cfg.tty_policy {
            TtyPolicy::Required => {
                let tty = peer_pid.and_then(controlling_tty);
                if tty.is_none() {
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
        };
        Response::ok(req.id.clone(), json!({"token": token, "scope": scope.as_str()}))
    }

    fn dispatch(&self, req: Request, scope: Scope, client_id: u64) -> Response {
        let id = req.id.clone();
        match req.method.as_str() {
            "session.ping" => Response::ok(id, json!("pong")),
            "events.subscribe" => {
                self.hub.subscribe(client_id);
                Response::ok(id, json!({"subscribed": true}))
            }
            "session.snapshot" => Response::ok(
                id,
                json!({"clients": {"console": self.hub.client_count(Some(Scope::Console)), "narrator": self.hub.client_count(Some(Scope::Narrator))}, "missions": []}),
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
