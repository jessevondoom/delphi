//! `delphi mcp` serves requests concurrently (`specs/104` §4, R3): one thread reads, each `tools/call`, `resources/read` and `prompts/get`
//! runs on a worker (at most [`MAX_WORKERS`] at once), and every reply or notification is written whole, one line at a time, under one
//! lock. `delphi bundle serve` keeps the sequential loop of `mcp_wire`.
//!
//! * **Cancel.** `notifications/cancelled` for a request still running stops this server waiting for it: no reply is sent for it (MCP's
//!   rule). It does not cancel a job in the daemon, with one exception (`specs/104` §12 A1): a job whose id the host has not yet been
//!   given ([`Job::started`]) is abandoned through [`Handler::abandon`], so a host can always stop a seat it started.
//! * **Close.** When the input closes, a job whose id the host never received is abandoned, and the server waits at most
//!   [`CLOSE_WAIT`] for its workers.
//!
//! Like the rest of the MCP server it writes no file and opens no network socket; it only orders the work on its two streams.

use crate::mcp_wire::{err, read_line, write_line, Line, MAX_LINE};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// How many requests run at once; the next waits its turn (`specs/104` §4 item 1).
pub const MAX_WORKERS: usize = 8;
/// How long the server waits for its workers once the input has closed (`specs/104` §4 item 6).
pub const CLOSE_WAIT: Duration = Duration::from_secs(2);
/// The reason given to the daemon when a job is stopped because the host went away before it received the job's id (`specs/104` §12 A1).
pub const ABANDONED: &str = "the host cancelled the request before it received the id";

/// What a concurrent MCP server does with one request.
pub trait Handler: Send + Sync + 'static {
    /// The reply to one request (or `None`), as the sequential server would give it. `job` lets a long call say which daemon job it
    /// started and learn whether the host still waits for it.
    fn respond(&self, v: Value, job: &Job) -> Option<Value>;
    /// Stop the daemon job `id`: the host went away before it was given the id.
    fn abandon(&self, id: &str);
}

/// The methods that run on a worker; everything else is answered on the reading thread, in order.
fn on_worker(method: &str) -> bool {
    matches!(method, "tools/call" | "resources/read" | "prompts/get")
}

#[derive(Default)]
struct Slot {
    cancelled: bool,
    /// The daemon job this request started, while its id has not been handed to the host.
    job: Option<String>,
}

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;
/// How a job whose id the host never received is stopped.
type Abandon = Arc<dyn Fn(&str) + Send + Sync>;

/// One request in flight, as its handler sees it.
pub struct Job {
    slot: Option<Arc<Mutex<Slot>>>,
    abandon: Option<Abandon>,
    out: Option<Writer>,
}

impl Job {
    /// A request that is not tracked (the sequential server, a test): never cancelled, and no notification goes anywhere.
    pub fn none() -> Job {
        Job { slot: None, abandon: None, out: None }
    }

    /// The request started the daemon job `id`, whose id the host does not have yet. `false` when the host has already cancelled the
    /// request: the job is then abandoned at once and the handler should stop waiting.
    pub fn started(&self, id: &str) -> bool {
        let Some(slot) = &self.slot else { return true };
        let cancelled = {
            let mut s = slot.lock().unwrap();
            if !s.cancelled {
                s.job = Some(id.to_string());
            }
            s.cancelled
        };
        if cancelled {
            if let Some(a) = &self.abandon {
                a(id);
            }
        }
        !cancelled
    }

    /// The daemon job this request started, while the host does not have its id yet.
    pub fn job_id(&self) -> Option<String> {
        self.slot.as_ref().and_then(|s| s.lock().unwrap().job.clone())
    }

    /// Whether the host has cancelled this request.
    pub fn cancelled(&self) -> bool {
        self.slot.as_ref().is_some_and(|s| s.lock().unwrap().cancelled)
    }

    /// Write one notification for this request, whole, unless it has been cancelled or answered.
    pub fn notify(&self, v: &Value) -> bool {
        let (Some(slot), Some(out)) = (&self.slot, &self.out) else { return false };
        let s = slot.lock().unwrap();
        if s.cancelled {
            return false;
        }
        let mut w = out.lock().unwrap();
        write_line(&mut *w, v).is_ok()
    }
}

/// How often a waiting request is sent a progress line (`specs/104` §3 item 3).
#[cfg(not(test))]
pub const PROGRESS_EVERY: Duration = Duration::from_secs(5);
#[cfg(test)]
pub const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// One progress line as the host is sent it: one line, no control characters, at most 200 characters (`specs/104` §3 item 2).
pub fn progress_text(line: &str) -> String {
    let flat: String = line.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect()
}

/// Run `work` while a second thread sends `notifications/progress` for `token` every `every`, until `work` returns (`specs/104` §3,
/// §12 A5). `progress` is the whole seconds waited, with no `total`; `message` is `line()`, which reads the daemon's own status line.
/// Nothing is sent for a request that is not tracked, after the host cancelled it, or after `work` has returned: the ticker is joined
/// before the caller writes the reply.
pub fn with_progress<T>(job: &Job, token: &Value, every: Duration, line: &(dyn Fn() -> String + Sync), work: impl FnOnce() -> T) -> T {
    if job.slot.is_none() {
        return work();
    }
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let start = std::time::Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            // Every `every` until `work` is done (the sender is dropped, which ends the wait at once).
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(every) {
                let message = progress_text(&line());
                job.notify(&serde_json::json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": token, "progress": start.elapsed().as_secs(), "message": message}}));
            }
        });
        let out = work();
        drop(tx);
        out
    })
}

/// A counting gate: at most `n` holders at once.
struct Gate {
    held: Mutex<usize>,
    freed: Condvar,
}

impl Gate {
    fn enter(&self, n: usize) {
        let mut h = self.held.lock().unwrap();
        while *h >= n {
            h = self.freed.wait(h).unwrap();
        }
        *h += 1;
    }

    fn leave(&self) {
        *self.held.lock().unwrap() -= 1;
        self.freed.notify_all();
    }
}

/// Requests in flight by their JSON-RPC id (its JSON text), and how many workers are alive.
#[derive(Default)]
struct Flight {
    slots: Mutex<HashMap<String, Arc<Mutex<Slot>>>>,
    alive: Mutex<usize>,
    done: Condvar,
}

/// The id a `notifications/cancelled` names, as the same JSON text a request's id is kept under.
fn cancelled_id(v: &Value) -> Option<String> {
    (v.get("method").and_then(Value::as_str) == Some("notifications/cancelled")).then(|| v.get("params").and_then(|p| p.get("requestId")).map(Value::to_string)).flatten()
}

/// Serve until the input closes. Returns the exit code.
pub fn serve<H: Handler, R: BufRead, W: Write + Send + 'static>(handler: Arc<H>, mut input: R, output: W) -> i32 {
    let out: Writer = Arc::new(Mutex::new(Box::new(output)));
    let broken = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flight = Arc::new(Flight::default());
    let gate = Arc::new(Gate { held: Mutex::new(0), freed: Condvar::new() });
    let abandon: Abandon = {
        let h = handler.clone();
        Arc::new(move |id: &str| h.abandon(id))
    };
    let write = |v: &Value| {
        let mut w = out.lock().unwrap();
        if write_line(&mut *w, v).is_err() {
            broken.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    };
    loop {
        if broken.load(std::sync::atomic::Ordering::SeqCst) {
            return 1; // the client went away
        }
        let bytes = match read_line(&mut input) {
            Ok(Line::Eof) => break,
            Ok(Line::Ok(b)) if b.iter().all(u8::is_ascii_whitespace) => continue,
            Ok(Line::Ok(b)) => b,
            Ok(Line::TooLong) => {
                write(&err(Value::Null, -32600, &format!("invalid request: the request is over the {} KiB limit", MAX_LINE / 1024)));
                continue;
            }
            Err(_) => return 1,
        };
        let parsed = std::str::from_utf8(&bytes).ok().and_then(|t| serde_json::from_str::<Value>(t).ok());
        let Some(v) = parsed else {
            // Not UTF-8 or not JSON: the sequential path answers it with its own parse error.
            if let Some(r) = crate::mcp_wire::handle_bytes(&bytes, &mut |v| handler.respond(v, &Job::none())) {
                write(&r);
            }
            continue;
        };
        if let Some(id) = cancelled_id(&v) {
            let slot = flight.slots.lock().unwrap().get(&id).cloned();
            if let Some(slot) = slot {
                let job = {
                    let mut s = slot.lock().unwrap();
                    s.cancelled = true;
                    s.job.take()
                };
                if let Some(job) = job {
                    let a = abandon.clone();
                    std::thread::spawn(move || a(&job));
                }
            }
            continue;
        }
        let method = v.get("method").and_then(Value::as_str).unwrap_or("");
        let id = v.get("id").filter(|i| i.is_string() || i.is_number()).map(Value::to_string);
        let Some(id) = id.filter(|_| on_worker(method)) else {
            if let Some(r) = handler.respond(v, &Job::none()) {
                write(&r);
            }
            continue;
        };
        let slot = Arc::new(Mutex::new(Slot::default()));
        flight.slots.lock().unwrap().insert(id.clone(), slot.clone());
        *flight.alive.lock().unwrap() += 1;
        let (h, out, gate, flight, abandon, broken) = (handler.clone(), out.clone(), gate.clone(), flight.clone(), abandon.clone(), broken.clone());
        std::thread::spawn(move || {
            gate.enter(MAX_WORKERS);
            let job = Job { slot: Some(slot.clone()), abandon: Some(abandon), out: Some(out.clone()) };
            let skip = slot.lock().unwrap().cancelled;
            let reply = if skip { None } else { h.respond(v, &job) };
            gate.leave();
            {
                // The reply and the end of the slot are one step: a cancel that comes after it finds nothing to stop.
                let mut s = slot.lock().unwrap();
                if let Some(r) = reply.filter(|_| !s.cancelled) {
                    let mut w = out.lock().unwrap();
                    if write_line(&mut *w, &r).is_err() {
                        broken.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                s.job = None;
                s.cancelled = true;
            }
            flight.slots.lock().unwrap().remove(&id);
            *flight.alive.lock().unwrap() -= 1;
            flight.done.notify_all();
        });
    }
    // The input closed: a job whose id the host never received is stopped, then the workers get a short while to finish.
    let jobs: Vec<String> = flight
        .slots
        .lock()
        .unwrap()
        .values()
        .filter_map(|s| {
            let mut s = s.lock().unwrap();
            let job = s.job.take();
            if job.is_some() {
                s.cancelled = true;
            }
            job
        })
        .collect();
    for j in jobs {
        abandon(&j);
    }
    let deadline = std::time::Instant::now() + CLOSE_WAIT;
    let mut alive = flight.alive.lock().unwrap();
    while *alive > 0 {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        alive = flight.done.wait_timeout(alive, deadline - now).unwrap().0;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::mpsc;

    /// Input that arrives when a test sends it, so a test can send a cancel after a request.
    struct ChanReader {
        rx: mpsc::Receiver<Vec<u8>>,
        buf: Vec<u8>,
        pos: usize,
    }

    impl std::io::Read for ChanReader {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.pos >= self.buf.len() {
                match self.rx.recv() {
                    Ok(b) => {
                        self.buf = b;
                        self.pos = 0;
                    }
                    Err(_) => return Ok(0),
                }
            }
            let n = out.len().min(self.buf.len() - self.pos);
            out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Sink {
        /// Every line written, each parsed on its own: a line that is not one whole JSON value fails the test.
        fn lines(&self) -> Vec<Value> {
            let text = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
            text.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not a whole line ({e}): {l}"))).collect()
        }
        fn ids(&self) -> Vec<Value> {
            self.lines().iter().map(|l| l["id"].clone()).collect()
        }
    }

    #[derive(Default)]
    struct Fake {
        running: Mutex<usize>,
        peak: Mutex<usize>,
        abandoned: Mutex<Vec<String>>,
    }

    impl Handler for Fake {
        fn respond(&self, v: Value, job: &Job) -> Option<Value> {
            let id = v.get("id").cloned()?;
            let name = v["params"]["name"].as_str().unwrap_or("").to_string();
            let token = v["params"]["_meta"]["progressToken"].clone();
            match name.as_str() {
                "progress" if !token.is_null() => with_progress(job, &token, Duration::from_millis(50), &|| "architect thinking,\n1 s".to_string(), || std::thread::sleep(Duration::from_millis(450))),
                "progress" => std::thread::sleep(Duration::from_millis(450)),
                "slow" => std::thread::sleep(Duration::from_millis(400)),
                "brief" => std::thread::sleep(Duration::from_millis(5)),
                "hold" => {
                    {
                        let mut r = self.running.lock().unwrap();
                        *r += 1;
                        let mut p = self.peak.lock().unwrap();
                        *p = (*p).max(*r);
                    }
                    std::thread::sleep(Duration::from_millis(300));
                    *self.running.lock().unwrap() -= 1;
                }
                "ask" => {
                    if job.started("t_1") {
                        let t = std::time::Instant::now();
                        while !job.cancelled() && t.elapsed() < Duration::from_secs(3) {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                "ask_fast" => {
                    job.started("t_2");
                }
                _ => {}
            }
            Some(json!({"jsonrpc": "2.0", "id": id, "result": {"name": name}}))
        }
        fn abandon(&self, id: &str) {
            self.abandoned.lock().unwrap().push(id.to_string());
        }
    }

    fn req(id: u64, name: &str) -> Vec<u8> {
        format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name}})).into_bytes()
    }

    fn cancel(id: u64) -> Vec<u8> {
        format!("{}\n", json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": id}})).into_bytes()
    }

    fn wait_reply(sink: &Sink, id: u64) {
        let t = std::time::Instant::now();
        while !sink.lines().iter().any(|l| l["id"] == json!(id)) && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Spec 104 §3: a waiting call that carries a progress token is sent progress lines before its reply and none after; a call
    /// without a token gets none. The line is flattened to one line of at most 200 characters.
    #[test]
    fn a_waiting_call_with_a_token_gets_progress_before_its_reply_and_none_after() {
        let (tx, sink, t) = start(Arc::new(Fake::default()));
        tx.send(format!("{}\n", json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "progress", "_meta": {"progressToken": "tok-1"}}})).into_bytes()).unwrap();
        wait_reply(&sink, 1);
        std::thread::sleep(Duration::from_millis(400));
        tx.send(req(2, "progress")).unwrap();
        wait_reply(&sink, 2);
        std::thread::sleep(Duration::from_millis(300));
        drop(tx);
        t.join().unwrap();
        let lines = sink.lines();
        let reply = lines.iter().position(|l| l["id"] == json!(1)).unwrap();
        let notes: Vec<(usize, &Value)> = lines.iter().enumerate().filter(|(_, l)| l["method"] == "notifications/progress").collect();
        assert!(!notes.is_empty(), "{lines:?}");
        for (i, n) in &notes {
            assert!(*i < reply, "no progress after the reply: {lines:?}");
            assert_eq!(n["params"]["progressToken"], "tok-1");
            assert!(n["params"]["progress"].is_u64() && n["params"].get("total").is_none(), "{n}");
            assert_eq!(n["params"]["message"], "architect thinking, 1 s", "{n}");
        }
        assert_eq!(lines.iter().filter(|l| l["method"] == "notifications/progress").count(), notes.len(), "the call without a token got none");
        assert_eq!(progress_text(&"x".repeat(300)).chars().count(), 200);
    }

    fn start(h: Arc<Fake>) -> (mpsc::Sender<Vec<u8>>, Sink, std::thread::JoinHandle<i32>) {
        let (tx, rx) = mpsc::channel();
        let sink = Sink::default();
        let out = sink.clone();
        let t = std::thread::spawn(move || serve(h, std::io::BufReader::new(ChanReader { rx, buf: Vec::new(), pos: 0 }), out));
        (tx, sink, t)
    }

    fn wait_for(sink: &Sink, n: usize) {
        let t = std::time::Instant::now();
        while sink.lines().len() < n && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_quick_call_sent_after_a_waiting_call_is_answered_first() {
        let (tx, sink, t) = start(Arc::new(Fake::default()));
        tx.send(req(1, "slow")).unwrap();
        tx.send(req(2, "quick")).unwrap();
        wait_for(&sink, 2);
        drop(tx);
        assert_eq!(t.join().unwrap(), 0);
        assert_eq!(sink.ids(), [json!(2), json!(1)], "reply order is completion order");
    }

    #[test]
    fn eight_waiting_calls_run_at_once_and_the_ninth_starts_when_one_ends() {
        let h = Arc::new(Fake::default());
        let (tx, sink, t) = start(h.clone());
        for i in 0..9 {
            tx.send(req(i, "hold")).unwrap();
        }
        wait_for(&sink, 9);
        drop(tx);
        t.join().unwrap();
        assert_eq!(*h.peak.lock().unwrap(), MAX_WORKERS, "never more than eight at once");
        assert_eq!(sink.lines().len(), 9, "the ninth ran once a worker was free");
    }

    #[test]
    fn a_cancelled_request_gets_no_reply_and_the_server_keeps_serving() {
        let h = Arc::new(Fake::default());
        let (tx, sink, t) = start(h.clone());
        tx.send(req(1, "slow")).unwrap();
        tx.send(cancel(1)).unwrap();
        tx.send(req(2, "quick")).unwrap();
        wait_for(&sink, 1);
        std::thread::sleep(Duration::from_millis(600));
        drop(tx);
        t.join().unwrap();
        assert_eq!(sink.ids(), [json!(2)], "no reply for the cancelled request, and the next one is served");
        assert!(h.abandoned.lock().unwrap().is_empty(), "a request that started no job stops nothing in the daemon");
    }

    /// Spec 104 §12 A1: a request cancelled before the host received its job's id stops that job; one whose id was returned does not.
    #[test]
    fn a_job_whose_id_the_host_never_received_is_stopped_on_cancel_and_one_already_returned_is_not() {
        let h = Arc::new(Fake::default());
        let (tx, sink, t) = start(h.clone());
        tx.send(req(1, "ask")).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        tx.send(cancel(1)).unwrap();
        tx.send(req(2, "ask_fast")).unwrap();
        wait_for(&sink, 1);
        tx.send(cancel(2)).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        drop(tx);
        t.join().unwrap();
        assert_eq!(sink.ids(), [json!(2)], "{:?}", sink.lines());
        assert_eq!(h.abandoned.lock().unwrap().as_slice(), ["t_1"], "only the job whose id never reached the host");
    }

    /// Spec 104 §4 item 6 and §12 A1: when the input closes, an in-flight job whose id the host never received is stopped.
    #[test]
    fn a_closed_input_stops_an_in_flight_job_and_the_server_exits_within_the_close_wait() {
        let h = Arc::new(Fake::default());
        let (tx, _sink, t) = start(h.clone());
        tx.send(req(1, "ask")).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let at = std::time::Instant::now();
        drop(tx);
        assert_eq!(t.join().unwrap(), 0);
        assert!(at.elapsed() < CLOSE_WAIT + Duration::from_millis(500), "{:?}", at.elapsed());
        assert_eq!(h.abandoned.lock().unwrap().as_slice(), ["t_1"]);
    }

    #[test]
    fn replies_are_whole_lines_under_a_load_of_two_hundred_mixed_requests() {
        let (tx, sink, t) = start(Arc::new(Fake::default()));
        let mut expect = 0;
        for i in 0..200u64 {
            let line = match i % 5 {
                0 => req(i, "brief"),
                1 => req(i, "quick"),
                2 => format!("{}\n", json!({"jsonrpc": "2.0", "id": i, "method": "ping"})).into_bytes(),
                3 => b"{not json\n".to_vec(),
                _ => format!("{}\n", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).into_bytes(),
            };
            if i % 5 != 4 {
                expect += 1;
            }
            tx.send(line).unwrap();
        }
        wait_for(&sink, expect);
        drop(tx);
        t.join().unwrap();
        assert_eq!(sink.lines().len(), expect, "one whole line per request that is answered, none for a notification");
    }
}
