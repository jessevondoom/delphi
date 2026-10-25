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
pub const RELAY_EVENTS: [&str; 6] = ["approval.requested", "approval.resolved", "approval.parked", "task.completed", "task.failed", "decisions.review_requested"];

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
}

#[derive(Default)]
struct HubInner {
    clients: BTreeMap<u64, Client>,
    next_id: u64,
    /// Pending deltas by span, flushed at 60Hz.
    deltas: BTreeMap<String, SpanBuf>,
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

    /// Send a durable event (already appended and fsync'd) to every subscribed client. A relay client gets
    /// only the kinds in [`RELAY_EVENTS`]: it carries cards to a person and has no need to see what agents say.
    pub fn publish_event(&self, event: &Value) {
        let h = self.inner.lock().unwrap();
        let kind = event["type"].as_str().unwrap_or("");
        for c in h.clients.values().filter(|c| c.subscribed && (c.scope != Scope::Relay || RELAY_EVENTS.contains(&kind))) {
            c.queue.push(Notification::new("event", event.clone()));
        }
    }

    /// Flush pending deltas as one frame per subscribed client. Called every [`FRAME_INTERVAL`].
    pub fn flush_frames(&self) {
        let mut h = self.inner.lock().unwrap();
        if h.deltas.is_empty() {
            return;
        }
        let spans: serde_json::Map<String, Value> = std::mem::take(&mut h.deltas)
            .into_iter()
            .map(|(span, b)| (span, json!({"text": b.text, "thinking": b.thinking})))
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
