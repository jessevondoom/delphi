//! The bridge between the window and the daemon (spec 111 §3.2): it keeps two narrator connections (one for calls, one for
//! `events.subscribe`), forwards the daemon's notifications to the window in batches, says when it is not connected, and
//! attaches again when the daemon comes back. It never starts a daemon.
//!
//! With a paired key (spec 120 §2 item 7, §3) the two connections are made on the daemon's app socket instead, each proved
//! by a signature; the first time, a ticket a terminal handed over pairs the key. With no key, a key the daemon refuses, or
//! a daemon with no app socket, it attaches on the narrator socket and reads, as before. Either way the window can ask for
//! the same nine reads; a paired window can also ask for the encyclopedia's eleven (spec 117 §3; `crate::ency`), which the
//! client never sends on the narrator socket. Nothing else can be named.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::client::{Client, Error, Method};
use crate::key::{self, AppKey, KeyStore};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conn {
    pub connected: bool,
    pub detail: String,
    /// The short name of the key this window proved, when it is attached on the app socket (spec 120); `None` when it reads
    /// on the narrator socket.
    pub paired: Option<String>,
    /// The grants that key held when the session opened.
    pub grants: Vec<String>,
    /// Why a window that has a key (or was handed a ticket) is reading without one: the key was refused, could not be
    /// read, the pairing failed. Empty when it is paired or never tried. The title row shows it (spec 120 §6 item 2).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unpaired_because: String,
}

impl Conn {
    fn off(detail: &str) -> Conn {
        Conn { connected: false, detail: detail.to_string(), paired: None, grants: Vec::new(), unpaired_because: String::new() }
    }
}

/// What the bridge found out about the key, once: the key, none, or why it could not be read.
type Loaded = Result<Option<AppKey>, String>;

/// Where the bridge sends what the window should know. The app's is the window; a test's is a channel.
pub trait Sink: Send + Sync + 'static {
    fn conn(&self, c: &Conn);
    /// Persisted events (`event` notifications), in order.
    fn events(&self, batch: Vec<Value>);
    /// A live text frame (`delta.frame`): never persisted, only for text that is still being written.
    fn frame(&self, frame: Value);
    /// The daemon dropped notifications for this client: ask for the state again.
    fn resync(&self);
}

/// What the window is told when it asks for something to be started without a paired key.
pub const NOT_PAIRED_TO_ASK: &str = "asking from this window needs a paired key: run `delphi app` in a terminal once";

/// The knowledge base reads the window may ask for through `kb.get` (spec 111 §2.2): search, listings, one document, and a
/// picture beside a document (the daemon decodes it and sends pixels, never the file). Everything else under `/v1/` is left out.
pub const KB_READS: [&str; 6] = ["/v1/search", "/v1/documents", "/v1/doc", "/v1/image", "/v1/kinds", "/v1/collections"];

/// Whether `target` is one of [`KB_READS`], with or without a query, and short enough for the daemon to take.
pub fn kb_target_ok(target: &str) -> bool {
    let path = target.split('?').next().unwrap_or("");
    target.len() <= 2048 && !target.contains(['\n', '\r', '#']) && KB_READS.contains(&path)
}

/// The most pixels a side the window may ask a picture for (spec 118 §4 item 3). The daemon holds the same ceiling; the
/// bridge holds it too, so a window that asks for more never makes the daemon decode and send it.
pub const IMAGE_SIDE_MAX: u64 = 1600;

/// The `kb.get` target the bridge sends for a target the window asked for, or `None` when the window may not make that
/// read. Every target must pass [`kb_target_ok`]. A picture's size, `max`, is clamped to 1 to [`IMAGE_SIDE_MAX`] here,
/// read the way the daemon reads it (percent-decoded name and value, the last one given counts); a `max` that is not a
/// number is dropped, so the daemon's default applies. A library picture is one more form of the image read (spec 118 §4
/// item 4), and only that form: `src=lib:<source>/<file>` given once, with `max` and nothing else, a plain source id and a
/// plain picture name. It is sent rebuilt from those two parts; the daemon serves it only with its licence record, and
/// refuses one without (`no licence record`, spec 118 §2 item 2).
pub fn kb_target(target: &str) -> Option<String> {
    if !kb_target_ok(target) {
        return None;
    }
    let Some(("/v1/image", query)) = target.split_once('?') else { return Some(target.to_string()) };
    let mut kept: Vec<String> = Vec::new();
    let mut max = None;
    let (mut lib, mut srcs, mut others) = (None, 0, 0);
    for part in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        let key = query_decode(k);
        match key.as_str() {
            "max" => max = Some(query_decode(v)),
            _ => {
                if key == "src" {
                    srcs += 1;
                    if let Some(r) = query_decode(v).strip_prefix("lib:") {
                        lib = Some(r.to_string());
                    }
                } else {
                    others += 1;
                }
                kept.push(part.to_string());
            }
        }
    }
    let max = max.and_then(|v| v.parse::<u64>().ok()).map(|n| format!("max={}", n.clamp(1, IMAGE_SIDE_MAX)));
    if let Some(r) = lib {
        // The library form: one `src`, no other name, a plain `<source>/<file>`.
        let (sid, file) = r.split_once('/')?;
        if srcs != 1 || others != 0 || !lib_part(sid) || !lib_part(file) || !is_picture(file) {
            return None;
        }
        return Some(format!("/v1/image?src=lib:{}/{}{}", sid, file, max.map(|m| format!("&{m}")).unwrap_or_default()));
    }
    kept.extend(max);
    Some(format!("/v1/image?{}", kept.join("&")))
}

/// A source id or a picture's file name as the library writes them: letters, digits, `.`, `_` and `-`, never `.` or `..`.
fn lib_part(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A picture the daemon decodes: PNG, JPEG or GIF.
fn is_picture(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif"].iter().any(|e| l.ends_with(e))
}

/// A query name or value decoded as the daemon decodes it (`kbserve::parse_query`): `+` is a space, `%XX` a byte.
fn query_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'+', _, _) => out.push(b' '),
            (b'%', Some(h), Some(l)) => {
                out.push(h * 16 + l);
                i += 2;
            }
            (c, _, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// How the bridge finds the socket: the app's asks `delphi`; a test's names its own.
pub type Locator = Box<dyn Fn() -> Option<(Value, PathBuf)> + Send + Sync>;

const BATCH_MAX: usize = 256;
const BATCH_WAIT: Duration = Duration::from_millis(40);
const CALL_TIMEOUT: Duration = Duration::from_secs(20);

pub struct Bridge {
    version: String,
    locate: Locator,
    conn: Mutex<Conn>,
    info: Mutex<Value>,
    calls: Mutex<Option<Client>>,
    stop: AtomicBool,
    /// Where the key is kept; `None` for a bridge that only ever reads on the narrator socket.
    keys: Option<Box<dyn KeyStore>>,
    /// The key, asked of the store once in the app's life (a keychain may ask the operator; it is not asked again and again).
    key: Mutex<Option<Loaded>>,
    /// A pairing ticket a terminal handed over, until it is used. Held in memory only.
    ticket: Mutex<Option<String>>,
}

impl Bridge {
    /// A bridge with no key store: it attaches on the narrator socket only.
    pub fn new(version: &str, locate: Locator) -> Arc<Bridge> {
        Bridge::build(version, locate, None)
    }

    /// A bridge that proves a paired key when `keys` holds one (spec 120 §2 item 7).
    pub fn with_keys(version: &str, locate: Locator, keys: Box<dyn KeyStore>) -> Arc<Bridge> {
        Bridge::build(version, locate, Some(keys))
    }

    fn build(version: &str, locate: Locator, keys: Option<Box<dyn KeyStore>>) -> Arc<Bridge> {
        Arc::new(Bridge {
            version: version.to_string(),
            locate,
            conn: Mutex::new(Conn::off("Looking for the daemon")),
            info: Mutex::new(json!({ "roles": {} })),
            calls: Mutex::new(None),
            stop: AtomicBool::new(false),
            keys,
            key: Mutex::new(None),
            ticket: Mutex::new(None),
        })
    }

    /// Give the bridge the ticket a terminal handed this app (spec 120 §2 item 3). It is used once, at the next attach.
    pub fn offer_ticket(&self, ticket: Option<String>) {
        if let Some(t) = ticket.filter(|t| key::is_ticket(t)) {
            *self.ticket.lock().unwrap() = Some(t);
        }
    }

    pub fn conn(&self) -> Conn {
        self.conn.lock().unwrap().clone()
    }

    pub fn info(&self) -> Value {
        self.info.lock().unwrap().clone()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// One allowed read. A refusal from the daemon is an error the window shows; a lost connection also drops the call
    /// connection, and the event loop notices the same loss and re-attaches.
    pub fn call(&self, method: Method, params: Value) -> Result<Value, String> {
        let mut guard = self.calls.lock().unwrap();
        let client = guard.as_mut().ok_or_else(|| "not connected to the daemon".to_string())?;
        match client.call(method, params) {
            Ok(v) => Ok(v),
            Err(e) => {
                let text = e.to_string();
                if e.is_disconnect() {
                    *guard = None;
                }
                Err(text)
            }
        }
    }

    /// One of the four requests (spec 120 §4.2). Asked only when this window's key held `request` when its session
    /// opened; otherwise the answer is the sentence that says how to get there, and the daemon is not asked. The daemon
    /// reads the key's record again for every call, so a grant taken away since is refused there too.
    pub fn request(&self, method: Method, params: Value) -> Result<Value, String> {
        if !method.asks() {
            return Err("that is not a request this app makes".into());
        }
        let conn = self.conn();
        match conn.paired {
            None => return Err(NOT_PAIRED_TO_ASK.into()),
            Some(name) if !conn.grants.iter().any(|g| g == "request") => return Err(format!("this app's key does not hold `request`: `delphi app grant {name} request` in a terminal gives it")),
            Some(_) => {}
        }
        self.call(method, params)
    }

    fn set_conn(&self, sink: &dyn Sink, connected: bool, detail: &str) {
        self.say(sink, Conn { connected, detail: detail.to_string(), paired: None, grants: Vec::new(), unpaired_because: String::new() });
    }

    fn say(&self, sink: &dyn Sink, c: Conn) {
        let changed = {
            let mut cur = self.conn.lock().unwrap();
            let changed = *cur != c;
            *cur = c.clone();
            changed
        };
        if changed {
            sink.conn(&c);
        }
    }

    /// Attach: on the app socket with the paired key when there is one (or a ticket to pair with), otherwise, and whenever
    /// that does not open a session, on the narrator socket to read. Returns the event connection and what to tell the window.
    fn attach(&self, narrator: &Path, app: Option<&Path>) -> Result<(Client, Conn), Error> {
        let mut why_not_paired = String::new();
        if let (Some(app), Some(keys)) = (app, self.keys.as_deref()) {
            match self.attach_paired(app, keys) {
                Ok(Some((events, name, grants))) => {
                    let detail = format!("Connected to the daemon, paired as {name}");
                    return Ok((events, Conn { connected: true, detail, paired: Some(name), grants, unpaired_because: String::new() }));
                }
                Ok(None) => {}
                Err(why) => why_not_paired = why,
            }
        }
        let calls = Client::attach(narrator, &self.version)?;
        calls.set_read_timeout(Some(CALL_TIMEOUT));
        let mut events = Client::attach(narrator, &self.version)?;
        events.call(Method::Subscribe, json!({}))?;
        *self.calls.lock().unwrap() = Some(calls);
        let detail = if why_not_paired.is_empty() { "Connected to the daemon, read-only".to_string() } else { format!("Connected to the daemon, read-only ({why_not_paired})") };
        Ok((events, Conn { connected: true, detail, paired: None, grants: Vec::new(), unpaired_because: why_not_paired }))
    }

    /// The paired way in (spec 120 §2 items 4, 5 and 7; §3 item 2). `Ok(None)`: there is no key and no ticket, or no app
    /// socket to try, so there is nothing to say. `Err`: why a key or a ticket did not open a session; the caller reads on.
    fn attach_paired(&self, app: &Path, keys: &dyn KeyStore) -> Result<Option<(Client, String, Vec<String>)>, String> {
        // No socket file: no daemon, or one from before the app socket. The key is not asked for on its account.
        if !app.exists() {
            return Ok(None);
        }
        let refused = |e: Error| match e {
            Error::Rpc { .. } => "this app's key was refused; `delphi app` in a terminal pairs it again".to_string(),
            Error::Closed => "the daemon closed the app socket; it does for a minute after five refusals".to_string(),
            Error::Io(_) => "the daemon's app socket did not answer".to_string(),
            Error::Protocol(m) => m,
            Error::NotPaired => crate::client::NOT_PAIRED.to_string(),
        };
        let stored = self.key.lock().unwrap().get_or_insert_with(|| keys.load()).clone();
        let ticket = self.ticket.lock().unwrap().take();
        let (key, proved) = match ticket {
            // First run from a terminal: pair, keep the key, and prove it on the same connection.
            Some(ticket) => {
                // A key that may exist but could not be read is not replaced by a new one.
                let stored = stored.map_err(|why| format!("not paired: {why}"))?;
                let seed = match &stored {
                    Some(k) => *k.seed(),
                    None => key::new_seed().map_err(|e| format!("not paired: no key could be made ({e})"))?,
                };
                let (mut c, challenge) = Client::knock(app).map_err(|e| format!("not paired: {}", refused(e)))?;
                let name = c.pair(&ticket, &key::public_hex(&seed), &self.version).map_err(|e| match e {
                    Error::Rpc { .. } => "the pairing was refused; run `delphi app` in a terminal again".to_string(),
                    other => format!("not paired: {}", refused(other)),
                })?;
                let key = AppKey::new(&name, seed).ok_or("not paired: the daemon did not name the key")?;
                if stored.as_ref().is_none_or(|k| k.name != key.name) {
                    // A key that cannot be kept is not used: the daemon's record of it is left for the operator to remove.
                    keys.save(&key).map_err(|why| format!("paired as {name}, but the key could not be kept ({why}); `delphi app revoke {name}` removes it"))?;
                    *self.key.lock().unwrap() = Some(Ok(Some(key.clone())));
                }
                let grants = c.prove(&key, &challenge, app, &self.version).map_err(refused)?;
                (key, Some((c, grants)))
            }
            None => match stored? {
                Some(key) => (key, None),
                None => return Ok(None),
            },
        };
        let (calls, _) = match proved {
            Some(p) => p,
            None => Client::attach_app(app, &self.version, &key).map_err(|e| {
                let why = refused(e);
                // A key the daemon refused is not offered again while this window is open: every refusal is recorded
                // there, and five in a minute close the socket to everyone.
                if why.starts_with("this app's key was refused") {
                    *self.key.lock().unwrap() = Some(Err(why.clone()));
                }
                why
            })?,
        };
        calls.set_read_timeout(Some(CALL_TIMEOUT));
        let (mut events, grants) = Client::attach_app(app, &self.version, &key).map_err(refused)?;
        events.call(Method::Subscribe, json!({})).map_err(|e| match e {
            Error::Rpc { .. } => "this app's key may not read; `delphi app grant <name> read` in a terminal gives it back".to_string(),
            other => refused(other),
        })?;
        *self.calls.lock().unwrap() = Some(calls);
        Ok(Some((events, key.name, grants)))
    }

    /// Read notifications until the connection is lost. Events that arrive close together go to the window as one batch.
    fn pump(&self, events: &mut Client, sink: &dyn Sink) -> Error {
        let mut batch: Vec<Value> = Vec::new();
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return Error::Closed;
            }
            // With nothing waiting to be sent, wait for the next notification; with a batch open, wait only briefly for more.
            events.set_read_timeout(Some(if batch.is_empty() { Duration::from_millis(500) } else { BATCH_WAIT }));
            let next = match events.next_notification() {
                Ok(n) => n,
                Err(e) => {
                    if !batch.is_empty() {
                        sink.events(std::mem::take(&mut batch));
                    }
                    return e;
                }
            };
            match next {
                None => {
                    if !batch.is_empty() {
                        sink.events(std::mem::take(&mut batch));
                    }
                }
                Some(mut n) => match n["method"].as_str() {
                    Some("event") => {
                        batch.push(n["params"].take());
                        if batch.len() >= BATCH_MAX {
                            sink.events(std::mem::take(&mut batch));
                        }
                    }
                    // Anything that is not an event goes out after the events that came before it, so the window sees the
                    // daemon's order.
                    other => {
                        if !batch.is_empty() {
                            sink.events(std::mem::take(&mut batch));
                        }
                        match other {
                            Some("delta.frame") => sink.frame(n["params"].take()),
                            Some("resync_required") => sink.resync(),
                            _ => {}
                        }
                    }
                },
            }
        }
    }

    /// Find the daemon, attach, forward, and when the connection is lost say so and try again, waiting a little longer each
    /// time up to five seconds. Returns only after [`Bridge::stop`].
    pub fn run(self: &Arc<Self>, sink: Arc<dyn Sink>) {
        let mut wait = Duration::from_millis(500);
        while !self.stop.load(Ordering::SeqCst) {
            match (self.locate)() {
                None => self.set_conn(sink.as_ref(), false, "No Delphi home was found on this machine"),
                Some((info, socket)) => {
                    // The app socket is the one `delphi daemon where` named, or the one the app worked out beside the narrator's.
                    let app = crate::locate::app_socket_in(&info);
                    *self.info.lock().unwrap() = info;
                    match self.attach(&socket, app.as_deref()) {
                        Ok((mut events, conn)) => {
                            wait = Duration::from_millis(500);
                            self.say(sink.as_ref(), conn);
                            let lost = self.pump(&mut events, sink.as_ref());
                            *self.calls.lock().unwrap() = None;
                            self.set_conn(sink.as_ref(), false, &format!("The daemon went away ({lost}); trying again"));
                        }
                        Err(Error::Io(_)) => self.set_conn(sink.as_ref(), false, "No daemon is running"),
                        Err(e) => self.set_conn(sink.as_ref(), false, &format!("Could not attach to the daemon: {e}")),
                    }
                }
            }
            let mut slept = Duration::ZERO;
            while slept < wait && !self.stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
                slept += Duration::from_millis(50);
            }
            wait = (wait * 2).min(Duration::from_secs(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::mpsc;

    /// What the fake daemon saw: every request line, in order.
    type Seen = Arc<Mutex<Vec<Value>>>;

    /// A daemon for tests: answers `auth.issue` and `hello`, `events.subscribe` (then pushes `after_subscribe`), refuses
    /// `kb.get` the way the real one refuses a method, and answers everything else with `{"echo": method}`.
    fn serve(stream: UnixStream, seen: Seen, after_subscribe: Vec<Value>, close_after_push: bool) {
        let mut out = stream.try_clone().unwrap();
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { return };
            let req: Value = serde_json::from_str(&line).unwrap();
            seen.lock().unwrap().push(req.clone());
            let id = req["id"].clone();
            let reply = match req["method"].as_str().unwrap() {
                "auth.issue" => json!({"jsonrpc": "2.0", "id": id, "result": {"token": "t0k", "scope": req["params"]["scope"]}}),
                "hello" => json!({"jsonrpc": "2.0", "id": id, "result": {"protocol_version": 1, "binary_version": "test", "scope": "narrator", "pid": 1}}),
                "kb.get" => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32001, "message": "refused for the test"}}),
                m => json!({"jsonrpc": "2.0", "id": id, "result": {"echo": m}}),
            };
            writeln!(out, "{reply}").unwrap();
            if req["method"] == "events.subscribe" {
                for n in &after_subscribe {
                    writeln!(out, "{n}").unwrap();
                }
                if close_after_push {
                    return;
                }
            }
        }
    }

    fn daemon(path: &Path, seen: Seen, after_subscribe: Vec<Value>, close_after_push: bool) {
        let listener = UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (seen, push) = (seen.clone(), after_subscribe.clone());
                std::thread::spawn(move || serve(stream, seen, push, close_after_push));
            }
        });
    }

    #[derive(Debug, PartialEq)]
    enum Got {
        Conn(bool, String),
        Events(Vec<Value>),
        Frame(Value),
        Resync,
    }
    struct Chan(Mutex<mpsc::Sender<Got>>);
    impl Sink for Chan {
        fn conn(&self, c: &Conn) {
            let _ = self.0.lock().unwrap().send(Got::Conn(c.connected, c.detail.clone()));
        }
        fn events(&self, batch: Vec<Value>) {
            let _ = self.0.lock().unwrap().send(Got::Events(batch));
        }
        fn frame(&self, frame: Value) {
            let _ = self.0.lock().unwrap().send(Got::Frame(frame));
        }
        fn resync(&self) {
            let _ = self.0.lock().unwrap().send(Got::Resync);
        }
    }

    fn start(socket: PathBuf) -> (Arc<Bridge>, mpsc::Receiver<Got>) {
        let (tx, rx) = mpsc::channel();
        let bridge = Bridge::new("9.9.9", Box::new(move || Some((json!({"roles": {}, "version": "test"}), socket.clone()))));
        let (b, sink): (Arc<Bridge>, Arc<dyn Sink>) = (bridge.clone(), Arc::new(Chan(Mutex::new(tx))));
        std::thread::spawn(move || b.run(sink));
        (bridge, rx)
    }

    fn wait_for(rx: &mpsc::Receiver<Got>, what: impl Fn(&Got) -> bool) -> Got {
        loop {
            let g = rx.recv_timeout(Duration::from_secs(10)).expect("the bridge said nothing in ten seconds");
            if what(&g) {
                return g;
            }
        }
    }

    #[test]
    fn it_attaches_as_narrator_with_protocol_one_and_forwards_what_the_daemon_sends() {
        let t = tempfile::tempdir().unwrap();
        let sock = t.path().join("n.sock");
        let seen: Seen = Arc::default();
        let push = vec![
            json!({"jsonrpc": "2.0", "method": "event", "params": {"type": "change.recorded", "seq": 1}}),
            json!({"jsonrpc": "2.0", "method": "event", "params": {"type": "task.output", "seq": 2}}),
            json!({"jsonrpc": "2.0", "method": "delta.frame", "params": {"spans": {}}}),
            json!({"jsonrpc": "2.0", "method": "resync_required", "params": {}}),
        ];
        daemon(&sock, seen.clone(), push, false);
        let (bridge, rx) = start(sock);
        assert_eq!(wait_for(&rx, |g| matches!(g, Got::Conn(true, _))), Got::Conn(true, "Connected to the daemon, read-only".into()));

        // Both events arrive, in order (in one batch or two), then the frame and the resync.
        let mut seqs = Vec::new();
        while seqs.len() < 2 {
            if let Got::Events(b) = wait_for(&rx, |g| matches!(g, Got::Events(_))) {
                seqs.extend(b.iter().map(|e| e["seq"].as_u64().unwrap()));
            }
        }
        assert_eq!(seqs, vec![1, 2]);
        wait_for(&rx, |g| matches!(g, Got::Frame(_)));
        wait_for(&rx, |g| *g == Got::Resync);

        // Every connection asked for the narrator scope and said hello with protocol 1 and the issued token; nothing else was asked for.
        let seen = seen.lock().unwrap().clone();
        let issues: Vec<&Value> = seen.iter().filter(|r| r["method"] == "auth.issue").collect();
        assert_eq!(issues.len(), 2, "one connection for calls, one for events");
        assert!(issues.iter().all(|r| r["params"] == json!({"scope": "narrator"})));
        let hellos: Vec<&Value> = seen.iter().filter(|r| r["method"] == "hello").collect();
        assert!(hellos.iter().all(|r| r["params"]["protocol_version"] == 1 && r["params"]["token"] == "t0k" && r["params"]["binary_version"] == "delphi-app 9.9.9"));

        // A read is answered; a refusal reaches the caller as an error, and the connection is kept.
        assert_eq!(bridge.call(Method::ObserveState, json!({})).unwrap(), json!({"echo": "observe.state"}));
        let refused = bridge.call(Method::KbGet, json!({"target": "/v1/doc"})).unwrap_err();
        assert!(refused.contains("-32001") && refused.contains("refused for the test"), "{refused}");
        assert_eq!(bridge.call(Method::Ping, json!({})).unwrap(), json!({"echo": "session.ping"}));
        bridge.stop();
    }

    #[test]
    fn with_no_daemon_it_says_so_and_attaches_when_one_appears_and_again_after_it_goes() {
        let t = tempfile::tempdir().unwrap();
        let sock = t.path().join("n.sock");
        let (bridge, rx) = start(sock.clone());
        assert_eq!(wait_for(&rx, |g| matches!(g, Got::Conn(false, _))), Got::Conn(false, "No daemon is running".into()));
        assert!(bridge.call(Method::ObserveState, json!({})).unwrap_err().contains("not connected"));

        // A daemon that closes the event connection after one event: the bridge reports the loss, then attaches again.
        let seen: Seen = Arc::default();
        daemon(&sock, seen.clone(), vec![json!({"jsonrpc": "2.0", "method": "event", "params": {"seq": 7}})], true);
        wait_for(&rx, |g| matches!(g, Got::Conn(true, _)));
        assert_eq!(wait_for(&rx, |g| matches!(g, Got::Events(_))), Got::Events(vec![json!({"seq": 7})]));
        let lost = wait_for(&rx, |g| matches!(g, Got::Conn(false, _)));
        assert!(matches!(&lost, Got::Conn(false, d) if d.contains("went away")), "{lost:?}");
        wait_for(&rx, |g| matches!(g, Got::Conn(true, _)));
        bridge.stop();
    }

    #[test]
    fn a_token_for_another_scope_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let sock = t.path().join("n.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut out = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let req: Value = serde_json::from_str(&line).unwrap();
                    writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": req["id"], "result": {"token": "t", "scope": "console"}})).unwrap();
                }
            }
        });
        let err = Client::attach(&sock, "1").err().expect("a console token must not be accepted");
        assert!(err.to_string().contains("another scope"), "{err}");
    }

    #[test]
    fn only_the_listed_knowledge_base_reads_pass() {
        for ok in ["/v1/search?q=narrator+socket&limit=20&facets=1", "/v1/documents?limit=50", "/v1/doc?path=SPEC.md", "/v1/image?path=a.md&src=img/x.png", "/v1/kinds", "/v1/collections"] {
            assert!(kb_target_ok(ok), "{ok}");
        }
        for bad in ["/v1/images?path=x.png", "/v1/health", "/v2/search?q=x", "/v1/searching", "/v1/doc/../x", "v1/doc?path=x", "/v1/doc?path=x\n/v1/health", "/v1/doc?path=x#frag", ""] {
            assert!(!kb_target_ok(bad), "{bad}");
        }
        assert!(!kb_target_ok(&format!("/v1/search?q={}", "x".repeat(2100))));
    }

    #[test]
    fn a_pictures_size_is_clamped_by_the_bridge_and_a_library_picture_is_one_plain_form() {
        let t = |s: &str| kb_target(s);
        // Not a picture: sent as written; not a listed read: refused as before.
        assert_eq!(t("/v1/doc?path=a.md&max=99999").as_deref(), Some("/v1/doc?path=a.md&max=99999"));
        assert_eq!(t("/v1/health?max=1"), None);
        assert_eq!(t("/v1/image?path=a.md&src=x.png").as_deref(), Some("/v1/image?path=a.md&src=x.png"), "no `max`: the daemon's default");
        assert_eq!(t("/v1/image?path=a.md&src=x.png&max=1200").as_deref(), Some("/v1/image?path=a.md&src=x.png&max=1200"));
        assert_eq!(t("/v1/image?max=99999&path=a.md&src=x.png").as_deref(), Some("/v1/image?path=a.md&src=x.png&max=1600"), "past the ceiling is the ceiling");
        assert_eq!(t("/v1/image?path=a.md&src=x.png&max=0").as_deref(), Some("/v1/image?path=a.md&src=x.png&max=1"));
        // Read as the daemon reads it: a percent-encoded name or value, and the last of several, are found and clamped.
        assert_eq!(t("/v1/image?path=a.md&src=x.png&m%61x=%39%39%39%39%39").as_deref(), Some("/v1/image?path=a.md&src=x.png&max=1600"));
        assert_eq!(t("/v1/image?path=a.md&max=10&src=x.png&max=7000").as_deref(), Some("/v1/image?path=a.md&src=x.png&max=1600"));
        assert_eq!(t("/v1/image?path=a.md&src=x.png&max=18446744073709551616").as_deref(), Some("/v1/image?path=a.md&src=x.png"), "too big to be a number: the default");
        for not_a_number in ["-5", "1e9", "+5", "12px", ""] {
            assert_eq!(t(&format!("/v1/image?path=a.md&src=x.png&max={not_a_number}")).as_deref(), Some("/v1/image?path=a.md&src=x.png"), "{not_a_number}");
        }
        // A library picture (spec 118 §4 item 4): the one form, rebuilt from its parts, with `max` clamped; the daemon serves it
        // only with its licence record.
        assert_eq!(t("/v1/image?src=lib:src1/a.png").as_deref(), Some("/v1/image?src=lib:src1/a.png"));
        assert_eq!(t("/v1/image?src=lib%3Asrc1/a.png&max=400").as_deref(), Some("/v1/image?src=lib:src1/a.png&max=400"));
        assert_eq!(t("/v1/image?max=99999&src=lib:s-1_x.y/0f.JPG").as_deref(), Some("/v1/image?src=lib:s-1_x.y/0f.JPG&max=1600"));
        // Anything else that names the library is refused: a second `src`, a `path`, another name, a path in the name, a file
        // that is not a picture, a part that is not plain.
        for lib in [
            "/v1/image?path=a.md&src=x.png&src=lib:s/a.png",
            "/v1/image?src=lib:s/a.png&src=lib:s/b.png",
            "/v1/image?src=lib:s/a.png&path=a.md",
            "/v1/image?src=lib:s/a.png&trusted_only=0",
            "/v1/image?src=lib:s/../meta.json",
            "/v1/image?src=lib:s/sub/a.png",
            "/v1/image?src=lib:../s/a.png",
            "/v1/image?src=lib:s/meta.json",
            "/v1/image?src=lib:s/source.md",
            "/v1/image?src=lib:s/a%20b.png",
            "/v1/image?src=lib:s",
            "/v1/image?src=lib:/a.png",
            "/v1/image?src=lib:s/.png%00.png",
        ] {
            assert_eq!(t(lib), None, "{lib}");
        }
    }

    #[test]
    fn every_method_the_app_can_send_is_on_the_daemons_narrator_allowlist() {
        // The daemon's own list, read from its source, so a method removed there fails here.
        let server = include_str!("../../../crates/delphi-core/src/server.rs");
        let line = server.lines().find(|l| l.contains("pub const NARRATOR_METHODS")).expect("the allowlist is where it was");
        let narrator = |m: Method| line.contains(&format!("\"{}\"", m.name()));
        // Every method is exactly one of three: a narrator method; one of the encyclopedia's reads (spec 117 §3), which the
        // client sends only on a connection that proved a paired key (spec 120); or one of the four requests (spec 120 §4.2).
        for m in Method::ALL {
            assert_eq!([narrator(m), m.paired_only(), m.asks()].iter().filter(|x| **x).count(), 1, "{} must be a narrator method, a paired-only read or a request, and only one of them", m.name());
        }
        for m in Method::NARRATOR {
            assert!(narrator(m) && !m.paired_only() && !m.asks(), "{} is not a narrator method", m.name());
        }
        let names: std::collections::BTreeSet<&str> = Method::ALL.iter().map(|m| m.name()).collect();
        assert_eq!(names.len(), Method::ALL.len());
        // No read the app can name starts work, answers an approval or changes a setting.
        let reads: Vec<Method> = Method::ALL.iter().copied().filter(|m| !m.asks()).collect();
        for m in &reads {
            let n = m.name();
            assert!(!["task.", "approval.resolve", "permissions.set", "settings.", "decision.", "ratify.", "budget.set", "shell.", "app.", "auth.", "pages.promote", "pages.dispute", "pages.save", "pages.refresh", "pages.open", "pages.search", "research."].iter().any(|p| n.starts_with(p)), "{n}");
        }
        // Spec 120 K3: pairing and the signed handshake are how a connection begins, and the list of what the app can call
        // did not grow by them. It is the nine reads of spec 111 §2.2, the encyclopedia's eleven (spec 117 E5), each a row
        // of the daemon's `read` grant; and, since spec 120 K5, the four requests of the daemon's `request` grant, which
        // are the only methods here that are not reads.
        assert_eq!((Method::NARRATOR.len(), Method::PAIRED_ONLY.len(), Method::REQUESTS.len(), Method::ALL.len()), (9, 11, 4, 24));
        assert_eq!(reads.len(), 20);
        let grants = include_str!("../../../crates/delphi-core/src/app_keys.rs");
        let read = grants.split("name: \"read\"").nth(1).and_then(|s| s.split("Grant {").next()).expect("the grant table is where it was");
        for m in &reads {
            assert!(read.contains(&format!("\"{}\"", m.name())), "{} is not under the daemon's `read` grant", m.name());
        }
        // The requests are exactly the daemon's `REQUEST_METHODS`, in its order, and that list is the `request` grant's row.
        let asked = grants.lines().find(|l| l.contains("pub const REQUEST_METHODS")).and_then(|l| l.split_once('=')).map(|(_, r)| r.trim().trim_end_matches(';').to_string()).expect("the request list is where it was");
        assert_eq!(asked, format!("[{}]", Method::REQUESTS.iter().map(|m| format!("\"{}\"", m.name())).collect::<Vec<_>>().join(", ")));
        assert!(grants.contains("Grant { name: \"request\", methods: &REQUEST_METHODS }"), "the `request` grant is that list");
        for m in Method::REQUESTS {
            assert!(!read.contains(&format!("\"{}\"", m.name())), "{} must not be a read", m.name());
        }
        // The paired-only reads are exactly the daemon's `ENCYCLOPEDIA_READS`, in its order: one added there, or one here
        // that the daemon does not list, fails.
        let listed = grants.lines().find(|l| l.contains("pub const ENCYCLOPEDIA_READS")).and_then(|l| l.split_once('=')).map(|(_, r)| r.trim().trim_end_matches(';').to_string()).expect("the encyclopedia's list is where it was");
        let ours = format!("[{}]", Method::PAIRED_ONLY.iter().map(|m| format!("\"{}\"", m.name())).collect::<Vec<_>>().join(", "));
        assert_eq!(listed, ours);
    }

    /// Spec 117 §3 with spec 120: an encyclopedia read is sent on a connection that proved a paired key and on no other.
    /// On the narrator socket the bridge answers for itself, and the daemon is not asked.
    #[test]
    fn an_encyclopedia_read_is_sent_only_on_a_paired_connection() {
        let t = tempfile::tempdir().unwrap();
        // No key: the narrator socket. Every paired-only read is refused by the client, and none reaches the daemon.
        let sock = t.path().join("n.sock");
        let seen: Seen = Arc::default();
        daemon(&sock, seen.clone(), vec![], false);
        let (bridge, rx) = start(sock);
        wait_for(&rx, |g| matches!(g, Got::Conn(true, _)));
        for m in Method::PAIRED_ONLY {
            let said = bridge.call(m, json!({})).unwrap_err();
            assert_eq!(said, crate::client::NOT_PAIRED, "{}", m.name());
        }
        // The connection is kept, and a narrator read still goes through.
        assert_eq!(bridge.call(Method::Ping, json!({})).unwrap(), json!({"echo": "session.ping"}));
        let sent = methods(&seen);
        assert!(Method::PAIRED_ONLY.iter().all(|m| !sent.iter().any(|s| s == m.name())), "{sent:?}");
        bridge.stop();

        // A paired key: the same reads go over the app socket, and the narrator socket hears nothing.
        let (home, nsock, asock) = (t.path().join("home"), t.path().join("n2.sock"), t.path().join("a.sock"));
        let seed = [5u8; 32];
        key::FileStore::under(&home).save(&AppKey::new(PAIRED_NAME, seed).unwrap()).unwrap();
        let narrator_seen: Seen = Arc::default();
        daemon(&nsock, narrator_seen.clone(), vec![], false);
        let app = app_socket(&asock, None, vec![(PAIRED_NAME.to_string(), key::public_hex(&seed))]);
        let (bridge, rx) = start_with_keys(&home, nsock, &asock, None);
        wait_for(&rx, |g| matches!(g, Got::Conn(true, _)));
        for m in Method::PAIRED_ONLY {
            assert_eq!(bridge.call(m, json!({})).unwrap(), json!({"echo": m.name()}));
        }
        assert!(methods(&narrator_seen).is_empty());
        assert_eq!(methods(&app.seen)[3..], Method::PAIRED_ONLY.map(|m| m.name().to_string()));
        bridge.stop();
    }

    // ── Spec 120 K3: the app socket. ──────────────────────────────────────────────────────────────────────────────────

    const INSTANCE: &str = "5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e";
    const PAIRED_NAME: &str = "app-0a1b2c";

    /// The daemon's app socket for tests, as `delphi-core` serves it (spec 120 §3): a challenge first; `app.pair` with the
    /// one ticket it was given records the public key; `app.hello` opens a session only with a signature by a recorded key
    /// over this connection's nonce, this instance and this socket's path. Anything else ends the connection.
    struct AppSocket {
        seen: Seen,
        keys: Recorded,
    }

    /// (name, public key as hex) of the keys the test daemon has recorded.
    type Recorded = Arc<Mutex<Vec<(String, String)>>>;

    fn signed_by(public_hex: &str, message: &str, signature_hex: &str) -> bool {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let bytes = |s: &str| -> Option<Vec<u8>> { (0..s.len() / 2).map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()).collect() };
        let (Some(public), Some(sig)) = (bytes(public_hex).and_then(|b| <[u8; 32]>::try_from(b).ok()), bytes(signature_hex).and_then(|b| Signature::from_slice(&b).ok())) else { return false };
        VerifyingKey::from_bytes(&public).is_ok_and(|k| k.verify(message.as_bytes(), &sig).is_ok())
    }

    fn app_socket(path: &Path, ticket: Option<&str>, keys: Vec<(String, String)>) -> AppSocket {
        let listener = UnixListener::bind(path).unwrap();
        let (seen, keys): (Seen, Recorded) = (Arc::default(), Arc::new(Mutex::new(keys)));
        let ticket = Arc::new(Mutex::new(ticket.map(String::from)));
        let socket = path.to_str().unwrap().to_string();
        let out = AppSocket { seen: seen.clone(), keys: keys.clone() };
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().flatten().enumerate() {
                let (seen, keys, ticket, socket) = (seen.clone(), keys.clone(), ticket.clone(), socket.clone());
                std::thread::spawn(move || {
                    let mut out = stream.try_clone().unwrap();
                    let nonce = format!("{:064x}", n + 1);
                    writeln!(out, "{}", json!({"jsonrpc": "2.0", "method": "app.challenge", "params": {"nonce": nonce, "instance": INSTANCE, "protocol_version": 1}})).unwrap();
                    let mut proved = false;
                    for line in BufReader::new(stream).lines().map_while(Result::ok) {
                        let req: Value = serde_json::from_str(&line).unwrap();
                        seen.lock().unwrap().push(req.clone());
                        let (id, p) = (req["id"].clone(), &req["params"]);
                        let refuse = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32001, "message": "the app key was refused"}});
                        let reply = match req["method"].as_str().unwrap() {
                            "app.pair" if !proved => {
                                let good = ticket.lock().unwrap().take_if(|t| p["ticket"] == t.as_str()).is_some();
                                if !good {
                                    let _ = writeln!(out, "{refuse}");
                                    return;
                                }
                                keys.lock().unwrap().push((PAIRED_NAME.to_string(), p["public_key"].as_str().unwrap().to_string()));
                                json!({"jsonrpc": "2.0", "id": id, "result": {"name": PAIRED_NAME, "grants": ["read", "request"]}})
                            }
                            "app.hello" if !proved => {
                                let name = p["name"].as_str().unwrap_or("");
                                let public = keys.lock().unwrap().iter().find(|(n, _)| n == name).map(|(_, k)| k.clone());
                                let message = format!("delphi-app/1\n{nonce}\n{INSTANCE}\n1\n{socket}");
                                if !public.is_some_and(|k| signed_by(&k, &message, p["signature"].as_str().unwrap_or(""))) {
                                    let _ = writeln!(out, "{refuse}");
                                    return;
                                }
                                proved = true;
                                json!({"jsonrpc": "2.0", "id": id, "result": {"protocol_version": 1, "binary_version": "test", "scope": "app", "pid": 1, "name": name, "grants": ["read", "request"]}})
                            }
                            // No session, no answer: a token is no use here.
                            _ if !proved => return,
                            m => json!({"jsonrpc": "2.0", "id": id, "result": {"echo": m}}),
                        };
                        writeln!(out, "{reply}").unwrap();
                    }
                });
            }
        });
        out
    }

    fn methods(seen: &Seen) -> Vec<String> {
        seen.lock().unwrap().iter().map(|r| r["method"].as_str().unwrap().to_string()).collect()
    }

    /// A bridge with a key file under `home`, told of both sockets, as `delphi daemon where` tells the app.
    fn start_with_keys(home: &Path, narrator: PathBuf, app: &Path, ticket: Option<&str>) -> (Arc<Bridge>, mpsc::Receiver<Got>) {
        let (tx, rx) = mpsc::channel();
        let info = json!({"roles": {}, "version": "test", "app_socket": app});
        let bridge = Bridge::with_keys("9.9.9", Box::new(move || Some((info.clone(), narrator.clone()))), Box::new(key::FileStore::under(home)));
        bridge.offer_ticket(ticket.map(String::from));
        let (b, sink): (Arc<Bridge>, Arc<dyn Sink>) = (bridge.clone(), Arc::new(Chan(Mutex::new(tx))));
        std::thread::spawn(move || b.run(sink));
        (bridge, rx)
    }

    /// Spec 120 §2 items 3 to 5: started with a ticket and no key, the app makes a key, pairs it, keeps it where only the
    /// operator reads, and opens its session with a signature. The ticket is used once and is written nowhere.
    #[test]
    fn with_a_ticket_and_no_key_it_pairs_keeps_the_key_and_opens_a_signed_session() {
        let t = tempfile::tempdir().unwrap();
        let (home, nsock, asock) = (t.path().join("home"), t.path().join("n.sock"), t.path().join("a.sock"));
        let ticket = "7a".repeat(32);
        let narrator_seen: Seen = Arc::default();
        daemon(&nsock, narrator_seen.clone(), vec![], false);
        let app = app_socket(&asock, Some(&ticket), vec![]);
        let (bridge, rx) = start_with_keys(&home, nsock, &asock, Some(&ticket));
        assert_eq!(wait_for(&rx, |g| matches!(g, Got::Conn(true, _))), Got::Conn(true, format!("Connected to the daemon, paired as {PAIRED_NAME}")));
        let conn = bridge.conn();
        assert_eq!((conn.paired.as_deref(), conn.grants), (Some(PAIRED_NAME), vec!["read".to_string(), "request".to_string()]));

        // On the wire: one pairing, with the ticket, the public key and the app's version; a signed hello on each of the two
        // connections; the subscription. No token was asked for anywhere, and the narrator socket was not touched.
        assert_eq!(methods(&app.seen), ["app.pair", "app.hello", "app.hello", "events.subscribe"]);
        let pair = app.seen.lock().unwrap()[0]["params"].clone();
        assert_eq!((pair["ticket"].as_str(), pair["app_version"].as_str(), pair["public_key"].as_str().map(str::len)), (Some(ticket.as_str()), Some("9.9.9"), Some(64)));
        assert!(pair.as_object().unwrap().len() == 3, "the private key is never sent: {pair}");
        assert!(methods(&narrator_seen).is_empty(), "{:?}", methods(&narrator_seen));

        // The key is kept under the home's `app/` folder, for the operator alone, under the name the daemon gave it; its
        // public half is the one the daemon recorded; the ticket is in no file.
        use std::os::unix::fs::PermissionsExt;
        let file = home.join("app/key.json");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        let kept = key::FileStore::under(&home).load().unwrap().unwrap();
        assert_eq!((kept.name.as_str(), key::public_hex(kept.seed())), (PAIRED_NAME, app.keys.lock().unwrap()[0].1.clone()));
        assert!(!std::fs::read_to_string(&file).unwrap().contains(&ticket));

        // The window's reads go over the session.
        assert_eq!(bridge.call(Method::ObserveState, json!({})).unwrap(), json!({"echo": "observe.state"}));
        bridge.stop();
    }

    /// AC-5: started from outside a terminal (no ticket) with a paired key, the app opens a session on the app socket; with
    /// no key it attaches on the narrator socket and reads, as it did before, and does not knock on the app socket at all.
    #[test]
    fn with_a_paired_key_it_opens_a_session_and_with_no_key_it_reads_on_the_narrator_socket() {
        let t = tempfile::tempdir().unwrap();
        let (home, nsock, asock) = (t.path().join("home"), t.path().join("n.sock"), t.path().join("a.sock"));
        let seed = [3u8; 32];
        key::FileStore::under(&home).save(&AppKey::new(PAIRED_NAME, seed).unwrap()).unwrap();
        let narrator_seen: Seen = Arc::default();
        daemon(&nsock, narrator_seen.clone(), vec![], false);
        let app = app_socket(&asock, None, vec![(PAIRED_NAME.to_string(), key::public_hex(&seed))]);
        let (bridge, rx) = start_with_keys(&home, nsock.clone(), &asock, None);
        assert_eq!(wait_for(&rx, |g| matches!(g, Got::Conn(true, _))), Got::Conn(true, format!("Connected to the daemon, paired as {PAIRED_NAME}")));
        assert_eq!(methods(&app.seen), ["app.hello", "app.hello", "events.subscribe"], "the key is enough: no ticket, no pairing");
        let hello = app.seen.lock().unwrap()[0]["params"].clone();
        assert_eq!((hello["name"].as_str(), hello["protocol_version"].as_u64(), hello["signature"].as_str().map(str::len)), (Some(PAIRED_NAME), Some(1), Some(128)));
        assert!(hello.get("token").is_none() && methods(&narrator_seen).is_empty());
        assert_eq!(bridge.call(Method::Ping, json!({})).unwrap(), json!({"echo": "session.ping"}));
        bridge.stop();

        // No key: read-only on the narrator socket.
        let empty = t.path().join("home2");
        let before = methods(&app.seen).len();
        let (bridge, rx) = start_with_keys(&empty, nsock, &asock, None);
        assert_eq!(wait_for(&rx, |g| matches!(g, Got::Conn(true, _))), Got::Conn(true, "Connected to the daemon, read-only".into()));
        assert_eq!((bridge.conn().paired, bridge.conn().grants.len()), (None, 0));
        assert_eq!(methods(&narrator_seen), ["auth.issue", "hello", "auth.issue", "hello", "events.subscribe"]);
        assert_eq!(methods(&app.seen).len(), before, "an app with no key does not knock on the app socket");
        assert!(!empty.exists(), "and it writes nothing under the home");
        bridge.stop();
    }

    /// A key the daemon no longer holds (revoked), or a ticket it does not know, opens no session: the app reads on the
    /// narrator socket and says why. A refused key is offered once, not on every re-attach, and is not thrown away.
    #[test]
    fn a_refused_key_or_ticket_falls_back_to_reading_and_a_refused_key_is_offered_once() {
        let t = tempfile::tempdir().unwrap();
        let (home, nsock, asock) = (t.path().join("home"), t.path().join("n.sock"), t.path().join("a.sock"));
        key::FileStore::under(&home).save(&AppKey::new(PAIRED_NAME, [4u8; 32]).unwrap()).unwrap();
        // The narrator daemon drops the event connection after one event, so the bridge attaches again and again.
        daemon(&nsock, Arc::default(), vec![json!({"jsonrpc": "2.0", "method": "event", "params": {"seq": 1}})], true);
        let app = app_socket(&asock, None, vec![(PAIRED_NAME.to_string(), key::public_hex(&[9u8; 32]))]);
        let (bridge, rx) = start_with_keys(&home, nsock.clone(), &asock, None);
        let first = wait_for(&rx, |g| matches!(g, Got::Conn(true, _)));
        assert!(matches!(&first, Got::Conn(true, d) if d.starts_with("Connected to the daemon, read-only (this app's key was refused") && d.contains("delphi app")), "{first:?}");
        assert_eq!(bridge.conn().paired, None);
        wait_for(&rx, |g| matches!(g, Got::Conn(false, _)));
        wait_for(&rx, |g| matches!(g, Got::Conn(true, _)));
        assert_eq!(methods(&app.seen), ["app.hello"], "one refusal, however often the bridge attaches again");
        assert!(key::FileStore::under(&home).load().unwrap().is_some(), "the key is kept: the operator may pair it again");
        bridge.stop();

        // A ticket the daemon does not know: nothing is kept, and the app reads.
        let home2 = t.path().join("home2");
        let (n2, a2) = (t.path().join("n2.sock"), t.path().join("a2.sock"));
        daemon(&n2, Arc::default(), vec![], false);
        let app2 = app_socket(&a2, Some(&"11".repeat(32)), vec![]);
        let (bridge, rx) = start_with_keys(&home2, n2, &a2, Some(&"22".repeat(32)));
        let said = wait_for(&rx, |g| matches!(g, Got::Conn(true, _)));
        assert!(matches!(&said, Got::Conn(true, d) if d.contains("read-only") && d.contains("the pairing was refused")), "{said:?}");
        assert_eq!(methods(&app2.seen), ["app.pair"]);
        // The reason is the window's to show while it is connected (spec 120 §6 item 2), and a window with no key asks for nothing.
        assert!(bridge.conn().unpaired_because.contains("the pairing was refused"), "{:?}", bridge.conn());
        assert_eq!(bridge.request(Method::PageRebuild, json!({"slug": "tides"})).unwrap_err(), NOT_PAIRED_TO_ASK);
        assert_eq!(methods(&app2.seen), ["app.pair"]);
        assert!(!home2.join("app/key.json").exists(), "a key that was not paired is not kept");
        bridge.stop();
    }

    /// Spec 120 §4.2, §6 item 1 (K5): a request is sent only by a window that proved a key holding `request`, and then
    /// on the app socket with the parameters `ask::request` built. A window reading on the narrator socket, or one whose
    /// key holds only `read`, is told how to get there and the daemon is not asked. Nothing but the four is a request.
    #[test]
    fn a_request_is_sent_only_on_a_paired_connection_whose_key_holds_request() {
        let t = tempfile::tempdir().unwrap();
        // No key: the narrator socket hears no request, by the bridge's rule and again by the client's.
        let sock = t.path().join("n.sock");
        let seen: Seen = Arc::default();
        daemon(&sock, seen.clone(), vec![], false);
        let (bridge, rx) = start(sock);
        wait_for(&rx, |g| matches!(g, Got::Conn(true, _)));
        for m in Method::REQUESTS {
            assert_eq!(bridge.request(m, json!({"slug": "tides"})).unwrap_err(), NOT_PAIRED_TO_ASK, "{}", m.name());
            assert_eq!(bridge.call(m, json!({"slug": "tides"})).unwrap_err(), crate::client::NOT_PAIRED, "{}", m.name());
        }
        assert!(methods(&seen).iter().all(|m| !m.starts_with("app.")), "{:?}", methods(&seen));
        assert_eq!(bridge.conn().unpaired_because, "", "a window that never had a key has no reason to show");
        bridge.stop();

        // A paired key holding `request`: each of the four goes over the app socket as built, and the narrator hears nothing.
        let (home, nsock, asock) = (t.path().join("home"), t.path().join("n2.sock"), t.path().join("a.sock"));
        let seed = [6u8; 32];
        key::FileStore::under(&home).save(&AppKey::new(PAIRED_NAME, seed).unwrap()).unwrap();
        let narrator_seen: Seen = Arc::default();
        daemon(&nsock, narrator_seen.clone(), vec![], false);
        let app = app_socket(&asock, None, vec![(PAIRED_NAME.to_string(), key::public_hex(&seed))]);
        let (bridge, rx) = start_with_keys(&home, nsock, &asock, None);
        wait_for(&rx, |g| matches!(g, Got::Conn(true, _)));
        let window = json!({"topic_id": "tides-0123abcd", "section": "why-twice-a-day", "note": "in winter", "question": "why twice?", "follows": "tides-0123abcd", "slug": "tides", "claim": "c-1", "query": "free text", "text": "a page's words"});
        let mut sent = Vec::new();
        for what in ["research.continue", "research.ask", "page.rebuild", "page.dispute"] {
            let (method, params) = crate::ask::request(what, &window).unwrap();
            assert_eq!(bridge.request(method, params.clone()).unwrap(), json!({"echo": method.name()}));
            sent.push((method.name().to_string(), params));
        }
        let on_wire: Vec<(String, Value)> = app.seen.lock().unwrap()[3..].iter().map(|r| (r["method"].as_str().unwrap().to_string(), r["params"].clone())).collect();
        assert_eq!(on_wire, sent);
        assert_eq!(on_wire.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(), ["app.research_continue", "app.research_ask", "app.page_rebuild", "app.page_dispute"]);
        assert!(on_wire.iter().all(|(_, p)| p.get("query").is_none() && p.get("text").is_none()), "{on_wire:?}");
        assert!(methods(&narrator_seen).is_empty());
        // A read is not a request, whatever it is called with.
        for m in [Method::Ping, Method::KbGet, Method::PagesGet, Method::Subscribe] {
            assert_eq!(bridge.request(m, json!({})).unwrap_err(), "that is not a request this app makes");
        }
        // The same window once its key holds `read` alone: told which command gives the grant, and nothing more is sent.
        let before = app.seen.lock().unwrap().len();
        bridge.conn.lock().unwrap().grants = vec!["read".to_string()];
        let said = bridge.request(Method::PageRebuild, json!({"slug": "tides"})).unwrap_err();
        assert_eq!(said, format!("this app's key does not hold `request`: `delphi app grant {PAIRED_NAME} request` in a terminal gives it"));
        assert_eq!(app.seen.lock().unwrap().len(), before);
        bridge.stop();
    }
}
