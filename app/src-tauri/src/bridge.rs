//! The bridge between the window and the daemon (spec 111 §3.2): it keeps two narrator connections (one for calls, one for
//! `events.subscribe`), forwards the daemon's notifications to the window in batches, says when it is not connected, and
//! attaches again when the daemon comes back. It never starts a daemon.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::client::{Client, Error, Method};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conn {
    pub connected: bool,
    pub detail: String,
}

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
}

impl Bridge {
    pub fn new(version: &str, locate: Locator) -> Arc<Bridge> {
        Arc::new(Bridge {
            version: version.to_string(),
            locate,
            conn: Mutex::new(Conn { connected: false, detail: "Looking for the daemon".into() }),
            info: Mutex::new(json!({ "roles": {} })),
            calls: Mutex::new(None),
            stop: AtomicBool::new(false),
        })
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

    fn set_conn(&self, sink: &dyn Sink, connected: bool, detail: &str) {
        let c = Conn { connected, detail: detail.to_string() };
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

    fn attach(&self, socket: &Path) -> Result<Client, Error> {
        let calls = Client::attach(socket, &self.version)?;
        calls.set_read_timeout(Some(CALL_TIMEOUT));
        let mut events = Client::attach(socket, &self.version)?;
        events.call(Method::Subscribe, json!({}))?;
        *self.calls.lock().unwrap() = Some(calls);
        Ok(events)
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
                    *self.info.lock().unwrap() = info;
                    match self.attach(&socket) {
                        Ok(mut events) => {
                            wait = Duration::from_millis(500);
                            self.set_conn(sink.as_ref(), true, "Connected to the daemon, read-only");
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
    fn every_method_the_app_can_send_is_on_the_daemons_narrator_allowlist() {
        // The daemon's own list, read from its source, so a method removed there fails here.
        let server = include_str!("../../../crates/delphi-core/src/server.rs");
        let line = server.lines().find(|l| l.contains("pub const NARRATOR_METHODS")).expect("the allowlist is where it was");
        for m in Method::ALL {
            assert!(line.contains(&format!("\"{}\"", m.name())), "{} is not a narrator method", m.name());
        }
        let names: std::collections::BTreeSet<&str> = Method::ALL.iter().map(|m| m.name()).collect();
        assert_eq!(names.len(), Method::ALL.len());
        // Nothing the app can name starts work, answers an approval or changes a setting.
        for m in Method::ALL {
            let n = m.name();
            assert!(!["task.", "approval.resolve", "permissions.set", "settings.", "decision.", "ratify.", "budget.set", "shell."].iter().any(|p| n.starts_with(p)), "{n}");
        }
    }
}
