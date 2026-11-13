//! Fan-out to attached clients: 60Hz delta coalescing and slow-client backpressure
//! (`specs/005` §2.1 items 4–5).
//!
//! * Token deltas (`agent.text.delta`, `agent.thinking.delta`) are buffered per span and flushed to
//!   each subscribed client as one frame every [`FRAME_INTERVAL`]. Frames carry the client's own
//!   `frame_seq` (`frame_from..frame_to`), never persisted and unrelated to the durable `seq`.
//! * Each client has a bounded outbound queue. On overflow the queue is dropped and replaced by a
//!   single `resync_required`, so a stalled client never grows daemon memory or slows the others.

use crate::proto::{Notification, Scope};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

pub const FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub const DEFAULT_QUEUE_CAP: usize = 1024;

/// The only event kinds a relay client is sent (`specs/014` §2). The list is the contract: adding a kind is a
/// decision. None of these carries model text; an approval card's payload is what the operator is asked to judge.
/// `xenia.alert` (X9c) carries only fixed slugs and counts; the relay words it from the Xenia help catalog and never
/// from the payload, and a relay holds no Xenia method (it cannot approve or sign anything).
pub const RELAY_EVENTS: [&str; 7] = ["approval.requested", "approval.resolved", "approval.parked", "task.completed", "task.failed", "decisions.review_requested", "xenia.alert"];

pub struct ClientQueue {
    inner: Mutex<QueueInner>,
    notify: Notify,
    cap: usize,
    pub dropped_frames: AtomicU64,
}

struct QueueInner {
    items: VecDeque<Notification>,
    /// A `resync_required` is queued; further overflow just discards.
    resync_pending: bool,
    closed: bool,
}

impl ClientQueue {
    pub fn new(cap: usize) -> Arc<ClientQueue> {
        Arc::new(ClientQueue {
            inner: Mutex::new(QueueInner { items: VecDeque::new(), resync_pending: false, closed: false }),
            notify: Notify::new(),
            cap,
            dropped_frames: AtomicU64::new(0),
        })
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Queue a notification. On overflow everything queued is dropped and one `resync_required`
    /// takes its place.
    pub fn push(&self, n: Notification) {
        let mut q = self.inner.lock().unwrap();
        if q.closed {
            return;
        }
        if q.items.len() >= self.cap {
            self.dropped_frames.fetch_add(q.items.len() as u64, Ordering::Relaxed);
            q.items.clear();
            q.items.push_back(Notification::new("resync_required", json!({})));
            q.resync_pending = true;
        } else if q.resync_pending && q.items.is_empty() {
            q.resync_pending = false;
            q.items.push_back(n);
        } else if !q.resync_pending {
            q.items.push_back(n);
        } else {
            // A resync is still queued: the client will refetch the snapshot, so newer frames are noise.
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
            return;
        }
        drop(q);
        self.notify.notify_one();
    }

    /// Next notification, waiting if the queue is empty. `None` once closed and drained.
    pub async fn pop(&self) -> Option<Notification> {
        loop {
            {
                let mut q = self.inner.lock().unwrap();
                if let Some(n) = q.items.pop_front() {
                    if q.items.is_empty() {
                        q.resync_pending = false;
                    }
                    return Some(n);
                }
                if q.closed {
                    return None;
                }
            }
            self.notify.notified().await;
        }
    }

    pub fn close(&self) {
        self.inner.lock().unwrap().closed = true;
        self.notify.notify_one();
    }
}

struct Client {
    scope: Scope,
    queue: Arc<ClientQueue>,
    subscribed: bool,
    frame_seq: u64,
}

#[derive(Default)]
struct SpanBuf {
    text: String,
    thinking: String,
    /// The span's running token total, when it reported one since the last frame (a value, not a delta).
    tokens: Option<u64>,
    /// An estimate of the tokens the call in flight has produced so far (an API seat: the turn is logged when it ends).
    est: Option<u64>,
}

/// What the engine registers to stand between a durable event and a relay client (`specs/103` §4.2, slice M3). Given the event as it
/// was written, it returns `Some(replacement)` when a relay must be sent something else (agent-written text withheld), or `None` when
/// the event goes out unchanged. It sees every event of a kind in [`RELAY_EVENTS`] and never runs for any other client.
pub type RelayFilter = Arc<dyn Fn(&Value) -> Option<Value> + Send + Sync>;

#[derive(Default)]
struct HubInner {
    clients: BTreeMap<u64, Client>,
    next_id: u64,
    /// Pending deltas by span, flushed at 60Hz.
    deltas: BTreeMap<String, SpanBuf>,
    /// The relay filter, when the engine set one. A hub without one (a test, a bare daemon) sends a relay the event as written.
    relay_filter: Option<RelayFilter>,
}

#[derive(Clone, Default)]
pub struct Hub {
    inner: Arc<Mutex<HubInner>>,
}

impl Hub {
    pub fn new() -> Hub {
        Hub::default()
    }

    pub fn attach(&self, scope: Scope, queue: Arc<ClientQueue>) -> u64 {
        let mut h = self.inner.lock().unwrap();
        h.next_id += 1;
        let id = h.next_id;
        h.clients.insert(id, Client { scope, queue, subscribed: false, frame_seq: 0 });
        id
    }

    pub fn detach(&self, id: u64) {
        if let Some(c) = self.inner.lock().unwrap().clients.remove(&id) {
            c.queue.close();
        }
    }

    pub fn subscribe(&self, id: u64) {
        if let Some(c) = self.inner.lock().unwrap().clients.get_mut(&id) {
            c.subscribed = true;
        }
    }

    pub fn client_count(&self, scope: Option<Scope>) -> usize {
        self.inner.lock().unwrap().clients.values().filter(|c| scope.map(|s| s == c.scope).unwrap_or(true)).count()
    }

    /// Buffer a live token delta; it reaches clients in the next frame and is never persisted.
    pub fn push_delta(&self, span_id: &str, kind: DeltaKind, text: &str) {
        let mut h = self.inner.lock().unwrap();
        let b = h.deltas.entry(span_id.to_string()).or_default();
        match kind {
            DeltaKind::Text => b.text.push_str(text),
            DeltaKind::Thinking => b.thinking.push_str(text),
        }
    }

    /// Buffer a span's running token total; it reaches clients in the next frame and is never persisted. A subscription CLI
    /// logs its usage once, when the span ends, so this is how a window shows the meter move meanwhile. The last value wins.
    pub fn push_usage(&self, span_id: &str, tokens: u64) {
        let mut h = self.inner.lock().unwrap();
        h.deltas.entry(span_id.to_string()).or_default().tokens = Some(tokens);
    }

    /// Buffer an estimate of a call's tokens so far, apart from [`Hub::push_usage`]'s total: it carries no cap and is replaced by
    /// the turn's real usage when that is logged.
    pub fn push_estimate(&self, span_id: &str, tokens: u64) {
        let mut h = self.inner.lock().unwrap();
        h.deltas.entry(span_id.to_string()).or_default().est = Some(tokens);
    }

    /// Send a durable event (already appended and fsync'd) to every subscribed client. A relay client gets
    /// only the kinds in [`RELAY_EVENTS`]: it carries cards to a person and has no need to see what agents say.
    pub fn publish_event(&self, event: &Value) {
        let h = self.inner.lock().unwrap();
        let kind = event["type"].as_str().unwrap_or("");
        // What a relay is sent, worked out once per event and only if a relay is listening: the engine's filter may replace agent-written
        // text (`specs/103` §4.2). The relay process never receives what the daemon withheld, so it cannot leak it.
        let mut for_relay: Option<Value> = None;
        for c in h.clients.values().filter(|c| c.subscribed && (c.scope != Scope::Relay || RELAY_EVENTS.contains(&kind))) {
            if c.scope == Scope::Relay {
                let v = for_relay.get_or_insert_with(|| h.relay_filter.as_ref().and_then(|f| f(event)).unwrap_or_else(|| event.clone()));
                c.queue.push(Notification::new("event", v.clone()));
            } else {
                c.queue.push(Notification::new("event", event.clone()));
            }
        }
    }

    /// Register the filter every event passes before a relay client is sent it (`specs/103` §4.4). Set once, at boot, by the engine.
    pub fn set_relay_filter(&self, f: RelayFilter) {
        self.inner.lock().unwrap().relay_filter = Some(f);
    }

    /// Flush pending deltas as one frame per subscribed client. Called every [`FRAME_INTERVAL`].
    pub fn flush_frames(&self) {
        let mut h = self.inner.lock().unwrap();
        if h.deltas.is_empty() {
            return;
        }
        let spans: serde_json::Map<String, Value> = std::mem::take(&mut h.deltas)
            .into_iter()
            .map(|(span, b)| {
                let mut v = json!({"text": b.text, "thinking": b.thinking});
                if let Some(t) = b.tokens {
                    v["tokens"] = json!(t);
                }
                if let Some(t) = b.est {
                    v["est"] = json!(t);
                }
                (span, v)
            })
            .collect();
        let spans = Value::Object(spans);
        // Live text is model output; a relay never receives it.
        for c in h.clients.values_mut().filter(|c| c.subscribed && c.scope != Scope::Relay) {
            c.frame_seq += 1;
            c.queue.push(Notification::new("delta.frame", json!({"frame_from": c.frame_seq, "frame_to": c.frame_seq, "spans": spans})));
        }
    }

    /// Run the coalescer until the hub is dropped.
    pub async fn run_coalescer(self) {
        let mut tick = tokio::time::interval(FRAME_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            self.flush_frames();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaKind {
    Text,
    Thinking,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deltas_coalesce_into_one_frame_per_flush() {
        let hub = Hub::new();
        let q = ClientQueue::new(DEFAULT_QUEUE_CAP);
        let id = hub.attach(Scope::Narrator, q.clone());
        hub.subscribe(id);
        for _ in 0..500 {
            hub.push_delta("sp1", DeltaKind::Text, "ab");
            hub.push_delta("sp2", DeltaKind::Thinking, "z");
        }
        hub.flush_frames();
        assert_eq!(q.len(), 1);
        let n = q.pop().await.unwrap();
        assert_eq!(n.method, "delta.frame");
        assert_eq!(n.params["spans"]["sp1"]["text"].as_str().unwrap().len(), 1000);
        assert_eq!(n.params["spans"]["sp2"]["thinking"].as_str().unwrap().len(), 500);
        assert_eq!(n.params["frame_from"], 1);
        hub.flush_frames(); // nothing pending: no empty frame
        assert!(q.is_empty());
    }

    #[tokio::test]
    async fn a_span_running_total_rides_the_frame_as_a_value_and_the_last_one_wins() {
        let hub = Hub::new();
        let q = ClientQueue::new(DEFAULT_QUEUE_CAP);
        let id = hub.attach(Scope::Narrator, q.clone());
        hub.subscribe(id);
        hub.push_usage("sp1", 1200);
        hub.push_usage("sp1", 3400);
        hub.push_estimate("sp2", 700);
        hub.push_estimate("sp2", 900);
        hub.push_delta("sp2", DeltaKind::Text, "x");
        hub.flush_frames();
        let n = q.pop().await.unwrap();
        assert_eq!(n.params["spans"]["sp1"]["tokens"], 3400);
        assert!(n.params["spans"]["sp2"].get("tokens").is_none(), "a span that reported no total carries none");
        assert_eq!((n.params["spans"]["sp2"]["est"].as_u64(), n.params["spans"]["sp1"].get("est")), (Some(900), None), "an estimate is a separate value; the last one wins");
        // A relay never sees frames at all.
        let r = ClientQueue::new(DEFAULT_QUEUE_CAP);
        let rid = hub.attach(Scope::Relay, r.clone());
        hub.subscribe(rid);
        hub.push_usage("sp1", 5);
        hub.flush_frames();
        assert!(r.is_empty());
    }

    /// `specs/103` §4.4: the engine's filter stands between a durable event and a relay client, and only there. Console and narrator clients get
    /// the event as written; a relay gets the replacement when the filter returns one and the event as written when it returns `None`; a hub with
    /// no filter changes nothing; the filter never sees an event kind a relay is not sent.
    #[tokio::test]
    async fn the_relay_filter_replaces_what_a_relay_is_sent_and_nothing_else() {
        let hub = Hub::new();
        let (con, nar, rel) = (ClientQueue::new(8), ClientQueue::new(8), ClientQueue::new(8));
        for (scope, q) in [(Scope::Console, &con), (Scope::Narrator, &nar), (Scope::Relay, &rel)] {
            let id = hub.attach(scope, q.clone());
            hub.subscribe(id);
        }
        let ev = json!({"type": "task.failed", "payload": {"error": "words an agent wrote"}});
        // no filter: all three are sent the event as written
        hub.publish_event(&ev);
        for q in [&con, &nar, &rel] {
            assert_eq!(q.pop().await.unwrap().params, ev);
        }
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let s = seen.clone();
        hub.set_relay_filter(Arc::new(move |e: &Value| {
            s.lock().unwrap().push(e["type"].as_str().unwrap_or("").to_string());
            (e["type"] == "task.failed").then(|| json!({"type": "task.failed", "payload": {"error": "[withheld]"}}))
        }));
        hub.publish_event(&ev);
        assert_eq!(con.pop().await.unwrap().params, ev, "a console is sent the event as written");
        assert_eq!(nar.pop().await.unwrap().params, ev, "a narrator is sent the event as written");
        assert_eq!(rel.pop().await.unwrap().params["payload"]["error"], "[withheld]", "a relay is sent the replacement");
        // `None` means: as written
        hub.publish_event(&json!({"type": "approval.resolved", "payload": {"by": "console-1"}}));
        assert_eq!(rel.pop().await.unwrap().params["payload"]["by"], "console-1");
        // an event a relay is never sent does not reach the filter at all, and the filter runs once per event, not once per client
        let (con2, nar2) = (con.clone(), nar.clone());
        while !con2.is_empty() {
            con2.pop().await;
        }
        while !nar2.is_empty() {
            nar2.pop().await;
        }
        let before = seen.lock().unwrap().len();
        hub.publish_event(&json!({"type": "task.output", "payload": {"text": "live"}}));
        assert!(rel.is_empty() && seen.lock().unwrap().len() == before, "{:?}", seen.lock().unwrap());
        assert_eq!(seen.lock().unwrap().iter().filter(|k| *k == "task.failed").count(), 1, "the filter ran once for the one event it was set for");
    }

    #[tokio::test]
    async fn unsubscribed_clients_receive_nothing() {
        let hub = Hub::new();
        let q = ClientQueue::new(8);
        hub.attach(Scope::Narrator, q.clone());
        hub.push_delta("sp1", DeltaKind::Text, "x");
        hub.flush_frames();
        hub.publish_event(&json!({"seq": 1}));
        assert!(q.is_empty());
    }

    #[tokio::test]
    async fn overflow_drops_the_queue_and_requests_a_resync() {
        let hub = Hub::new();
        let slow = ClientQueue::new(4);
        let fast = ClientQueue::new(4);
        let s = hub.attach(Scope::Narrator, slow.clone());
        let f = hub.attach(Scope::Console, fast.clone());
        hub.subscribe(s);
        hub.subscribe(f);
        for i in 0..50 {
            hub.publish_event(&json!({"seq": i}));
            // The fast client keeps up.
            while !fast.is_empty() {
                fast.pop().await;
            }
        }
        assert!(slow.len() <= 4, "queue stayed bounded: {}", slow.len());
        assert_eq!(slow.pop().await.unwrap().method, "resync_required");
        assert!(slow.dropped_frames.load(Ordering::Relaxed) > 0);
        assert!(fast.is_empty() && fast.dropped_frames.load(Ordering::Relaxed) == 0);
    }

    #[tokio::test]
    async fn frame_seq_is_per_client() {
        let hub = Hub::new();
        let a = ClientQueue::new(8);
        let b = ClientQueue::new(8);
        let ia = hub.attach(Scope::Narrator, a.clone());
        hub.subscribe(ia);
        hub.push_delta("s", DeltaKind::Text, "1");
        hub.flush_frames();
        let ib = hub.attach(Scope::Narrator, b.clone());
        hub.subscribe(ib);
        hub.push_delta("s", DeltaKind::Text, "2");
        hub.flush_frames();
        a.pop().await;
        assert_eq!(a.pop().await.unwrap().params["frame_from"], 2);
        assert_eq!(b.pop().await.unwrap().params["frame_from"], 1);
    }
}
