//! The approval broker (`specs/005` §3.1 approval cards, `specs/009` §7.6, `specs/008` HITL tiers).
//!
//! * **Green** actions never ask. **Amber** actions ask unless a permission mode or an in-task
//!   "allow similar" covers them. **Red** actions always ask: no mode, no "allow similar".
//! * **Bound answers:** an answer carries the blake3 of the exact payload the card showed. A changed
//!   payload is a new approval, so a stale or forged answer can't approve something else.
//! * **First answer wins** by `approval_id`; later answers learn who answered and how.
//! * **Timeout always denies,** whether or not any console is attached; the agent gets a structured
//!   denial and does not hang.
//! * `[a]` "allow similar" covers only `(task_id, tool, the normalized pattern shown on the card,
//!   worktree)` for the rest of that task, and never a Red action.

use crate::tools::{ActionKind, Classified, ToolTier};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// `/permissions` modes. None of them ever approves a Red action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Every Amber action asks.
    Default,
    /// Auto-approves Amber worktree file and branch edits only.
    AcceptEdits,
    /// Additionally auto-approves dependency installs pinned in a committed lockfile.
    MissionAuto,
    /// As `MissionAuto`, and a build started in it decides its eligible cards provisionally (`specs/015`), as if
    /// started with `--optimistic`. Needs the OS sandbox.
    Optimistic,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "default" => Mode::Default,
            "accept-edits" => Mode::AcceptEdits,
            "mission-auto" => Mode::MissionAuto,
            "optimistic" => Mode::Optimistic,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Default => "default",
            Mode::AcceptEdits => "accept-edits",
            Mode::MissionAuto => "mission-auto",
            Mode::Optimistic => "optimistic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Amber,
    Red,
}

impl Tier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Amber => "amber",
            Tier::Red => "red",
        }
    }
}

/// Compact JSON with every object's keys sorted, whatever order the map held them in.
fn canonical_json(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).expect("string serializes"));
                out.push(':');
                canonical_json(&m[k.as_str()], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical_json(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).expect("scalar serializes")),
    }
}

/// `blake3` of the canonical JSON of the payload shown on the card. Canonical means sorted keys and no
/// whitespace, so the same payload hashes the same wherever and however it was built or reloaded.
pub fn payload_blake3(payload: &Value) -> String {
    let mut s = String::new();
    canonical_json(payload, &mut s);
    blake3::hash(s.as_bytes()).to_hex().to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub approval_id: String,
    pub task_id: String,
    pub span_id: String,
    /// Canonical role ID.
    pub role: String,
    pub tier: Tier,
    pub tool: String,
    pub pattern: String,
    pub worktree: Option<String>,
    pub summary: String,
    pub payload: Value,
    pub payload_blake3: String,
    pub timeout: Duration,
}

impl Request {
    /// Red cards and connector draft cards never take "allow similar" (spec 010-A2).
    pub fn similar_allowed(&self) -> bool {
        self.tier != Tier::Red && !(self.tool == "connector" && self.pattern.ends_with(" draft"))
    }

    /// The `approval.requested` event payload (and what a console renders).
    pub fn event_payload(&self) -> Value {
        json!({"approval_id": self.approval_id, "task_id": self.task_id, "span_id": self.span_id, "role": self.role, "tier": self.tier.as_str(),
               "tool": self.tool, "pattern": self.pattern, "summary": self.summary, "payload": self.payload, "payload_blake3": self.payload_blake3,
               "timeout_secs": self.timeout.as_secs(), "similar_allowed": self.similar_allowed()})
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// `[y]` approve once.
    Allow,
    /// `[a]` allow similar for the rest of the task (Amber only).
    AllowSimilar,
    /// `[n]` reject and redirect.
    Deny { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Approved { similar: bool },
    Denied { reason: String },
    TimedOut,
}

impl Resolution {
    pub fn is_approved(&self) -> bool {
        matches!(self, Resolution::Approved { .. })
    }

    /// The `approval.resolved` event payload.
    pub fn event_payload(&self, approval_id: &str, by: &str) -> Value {
        match self {
            Resolution::Approved { similar } => json!({"approval_id": approval_id, "resolution": "approved", "similar": similar, "by": by}),
            Resolution::Denied { reason } => json!({"approval_id": approval_id, "resolution": "denied", "reason": reason, "by": by}),
            Resolution::TimedOut => json!({"approval_id": approval_id, "resolution": "timed_out", "by": by}),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// Already resolved (or never existed): the first answer won.
    Gone { resolution: Option<Resolution>, by: Option<String> },
    /// The answer was for a different payload than the card shows.
    PayloadMismatch,
    /// `[a]` on a Red action.
    SimilarNotAllowedForRed,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Gone { resolution: Some(r), by: Some(b) } => write!(f, "already answered by {}: {:?}", b, r),
            ResolveError::Gone { .. } => write!(f, "no such pending approval"),
            ResolveError::PayloadMismatch => write!(f, "the answer's payload_blake3 does not match the payload shown"),
            ResolveError::SimilarNotAllowedForRed => write!(f, "`allow similar` never covers a Red action or a connector draft"),
        }
    }
}

impl std::error::Error for ResolveError {}

#[derive(Debug, PartialEq, Eq)]
pub enum Need {
    /// Run it; the string says why no approval was needed (for the narration).
    No(&'static str),
    Yes(Tier),
}

struct Pending {
    req: Request,
    tx: oneshot::Sender<Resolution>,
}

#[derive(Default)]
struct State {
    mode_idx: u8,
    pending: BTreeMap<String, Pending>,
    /// Recently resolved, so a late second answer can learn who won.
    resolved: HashMap<String, (Resolution, String)>,
    similar: HashSet<(String, String, String, Option<String>)>,
    /// Tasks running on a weaker model than the one that ran out (`specs/035` D-13): the permission mode's automatic approvals are off for them.
    degraded: HashSet<String>,
    next_id: u64,
}

pub struct Broker {
    state: Mutex<State>,
    prefix: String,
}

/// Waits for the operator's answer (or the timeout).
pub struct Waiting {
    pub approval_id: String,
    rx: oneshot::Receiver<Resolution>,
}

impl Waiting {
    pub async fn wait(self) -> Resolution {
        // A dropped sender means the broker went away: deny.
        self.rx.await.unwrap_or(Resolution::Denied { reason: "the approval was abandoned".into() })
    }
}

fn mode_from(i: u8) -> Mode {
    match i {
        1 => Mode::AcceptEdits,
        2 => Mode::MissionAuto,
        3 => Mode::Optimistic,
        _ => Mode::Default,
    }
}

impl Broker {
    /// A broker whose ids are unique across daemon restarts (`ap_<4 hex>_<n>`), so an id in the log never
    /// refers to two different cards.
    pub fn new_unique() -> Arc<Broker> {
        let tag = delphi_core::server::random_token().map(|t| t[..4].to_string()).unwrap_or_else(|_| "0000".into());
        Broker::new(&format!("ap_{}_", tag))
    }

    pub fn new(id_prefix: &str) -> Arc<Broker> {
        Arc::new(Broker { state: Mutex::new(State::default()), prefix: id_prefix.to_string() })
    }

    pub fn mode(&self) -> Mode {
        mode_from(self.state.lock().unwrap().mode_idx)
    }

    pub fn set_mode(&self, m: Mode) {
        self.state.lock().unwrap().mode_idx = match m {
            Mode::Default => 0,
            Mode::AcceptEdits => 1,
            Mode::MissionAuto => 2,
            Mode::Optimistic => 3,
        };
    }

    /// Does this classified action need the operator?
    pub fn need(&self, task_id: &str, worktree: Option<&str>, c: &Classified) -> Need {
        if c.tier == ToolTier::Green {
            return Need::No("green");
        }
        let st = self.state.lock().unwrap();
        let mode = mode_from(st.mode_idx);
        // A task on a weaker model gets no automatic approval from the mode: each write asks. (Red is never automatic anyway.)
        let auto = !st.degraded.contains(task_id) && matches!(
            (mode, c.kind),
            (Mode::AcceptEdits | Mode::MissionAuto | Mode::Optimistic, ActionKind::WorktreeEdit) | (Mode::MissionAuto | Mode::Optimistic, ActionKind::PinnedDependencyInstall)
        );
        if auto {
            return Need::No("permission mode");
        }
        if st.similar.contains(&(task_id.to_string(), c.tool.clone(), c.pattern.clone(), worktree.map(String::from))) {
            return Need::No("allowed similar earlier in this task");
        }
        Need::Yes(Tier::Amber)
    }

    /// Raise an approval and start its timeout. The returned [`Waiting`] resolves with the answer.
    pub fn request(self: &Arc<Self>, mut req: Request) -> Waiting {
        let (tx, rx) = oneshot::channel();
        {
            let mut st = self.state.lock().unwrap();
            if req.approval_id.is_empty() {
                st.next_id += 1;
                req.approval_id = format!("{}{:04}", self.prefix, st.next_id);
            }
            req.payload_blake3 = payload_blake3(&req.payload);
            st.pending.insert(req.approval_id.clone(), Pending { req: req.clone(), tx });
        }
        let (me, id, timeout) = (self.clone(), req.approval_id.clone(), req.timeout);
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            me.expire(&id);
        });
        Waiting { approval_id: req.approval_id, rx }
    }

    fn expire(&self, approval_id: &str) {
        let mut st = self.state.lock().unwrap();
        if let Some(p) = st.pending.remove(approval_id) {
            st.resolved.insert(approval_id.to_string(), (Resolution::TimedOut, "timeout".into()));
            let _ = p.tx.send(Resolution::TimedOut);
        }
    }

    /// Answer a card. `by` identifies the answering console for the others to see.
    pub fn resolve(&self, approval_id: &str, payload_blake3: &str, answer: Answer, by: &str) -> Result<Resolution, ResolveError> {
        let mut st = self.state.lock().unwrap();
        let Some(p) = st.pending.get(approval_id) else {
            let prior = st.resolved.get(approval_id).cloned();
            return Err(ResolveError::Gone { resolution: prior.as_ref().map(|(r, _)| r.clone()), by: prior.map(|(_, b)| b) });
        };
        if p.req.payload_blake3 != payload_blake3 {
            return Err(ResolveError::PayloadMismatch);
        }
        if answer == Answer::AllowSimilar && !p.req.similar_allowed() {
            return Err(ResolveError::SimilarNotAllowedForRed);
        }
        let p = st.pending.remove(approval_id).expect("checked above");
        let resolution = match answer {
            Answer::Allow => Resolution::Approved { similar: false },
            Answer::AllowSimilar => {
                st.similar.insert((p.req.task_id.clone(), p.req.tool.clone(), p.req.pattern.clone(), p.req.worktree.clone()));
                Resolution::Approved { similar: true }
            }
            Answer::Deny { reason } => Resolution::Denied { reason },
        };
        st.resolved.insert(approval_id.to_string(), (resolution.clone(), by.to_string()));
        let _ = p.tx.send(resolution.clone());
        Ok(resolution)
    }

    /// A pending request by id.
    /// Who ended a card, as the broker recorded it (`console-<id>`, `relay:<channel>:<sender>`, `timeout`, `daemon`).
    pub fn resolved_by(&self, approval_id: &str) -> Option<String> {
        self.state.lock().unwrap().resolved.get(approval_id).map(|(_, by)| by.clone())
    }

    pub fn get(&self, approval_id: &str) -> Option<Request> {
        self.state.lock().unwrap().pending.get(approval_id).map(|p| p.req.clone())
    }

    /// Cards awaiting an answer, oldest first (shown on attach).
    pub fn pending(&self) -> Vec<Request> {
        self.state.lock().unwrap().pending.values().map(|p| p.req.clone()).collect()
    }

    /// `/permissions` lists these.
    pub fn allow_similar(&self) -> Vec<(String, String, String)> {
        let mut v: Vec<_> = self.state.lock().unwrap().similar.iter().map(|(t, tool, pat, _)| (t.clone(), tool.clone(), pat.clone())).collect();
        v.sort();
        v
    }

    /// A task ended: its "allow similar" grants end with it, and its open cards are denied.
    /// From now until the task ends, the permission mode no longer approves anything for it by itself (`specs/035` D-13).
    pub fn mark_degraded(&self, task_id: &str) {
        self.state.lock().unwrap().degraded.insert(task_id.to_string());
    }

    pub fn is_degraded(&self, task_id: &str) -> bool {
        self.state.lock().unwrap().degraded.contains(task_id)
    }

    pub fn end_task(&self, task_id: &str) {
        let mut st = self.state.lock().unwrap();
        st.similar.retain(|(t, _, _, _)| t != task_id);
        st.degraded.remove(task_id);
        let ids: Vec<String> = st.pending.iter().filter(|(_, p)| p.req.task_id == task_id).map(|(k, _)| k.clone()).collect();
        for id in ids {
            if let Some(p) = st.pending.remove(&id) {
                let r = Resolution::Denied { reason: "the task ended".into() };
                st.resolved.insert(id, (r.clone(), "daemon".into()));
                let _ = p.tx.send(r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::classify;

    fn spec_write(content: &str) -> Classified {
        classify("write_file", &json!({"path": "specs/009-x.md", "content": content})).unwrap()
    }

    fn req(c: &Classified, task: &str, timeout: Duration) -> Request {
        Request { approval_id: String::new(), task_id: task.into(), span_id: "sp1".into(), role: "architect".into(), tier: Tier::Amber, tool: c.tool.clone(), pattern: c.pattern.clone(), worktree: None, summary: c.summary.clone(), payload: c.payload.clone(), payload_blake3: String::new(), timeout }
    }

    fn red(task: &str) -> Request {
        Request { approval_id: String::new(), task_id: task.into(), span_id: "sp1".into(), role: "integrator".into(), tier: Tier::Red, tool: "gh_pr_create".into(), pattern: "gh_pr_create".into(), worktree: None, summary: "open a PR".into(), payload: json!({"title": "x"}), payload_blake3: String::new(), timeout: Duration::from_secs(60) }
    }

    #[test]
    fn green_never_asks_and_amber_asks_in_every_mode_that_does_not_cover_it() {
        let b = Broker::new("ap_");
        let note = classify("write_file", &json!({"path": "missions/m/n.md", "content": "x"})).unwrap();
        assert_eq!(b.need("t1", None, &note), Need::No("green"));
        let spec = spec_write("x");
        for m in [Mode::Default, Mode::AcceptEdits, Mode::MissionAuto, Mode::Optimistic] {
            b.set_mode(m);
            assert_eq!(b.need("t1", None, &spec), Need::Yes(Tier::Amber), "a draft spec outside the worktree asks in {:?}", m);
        }
        // Worktree edits and pinned installs (Step 3) follow the modes.
        let mut edit = spec.clone();
        edit.kind = ActionKind::WorktreeEdit;
        let mut install = spec.clone();
        install.kind = ActionKind::PinnedDependencyInstall;
        b.set_mode(Mode::Default);
        assert_eq!((b.need("t", None, &edit), b.need("t", None, &install)), (Need::Yes(Tier::Amber), Need::Yes(Tier::Amber)));
        b.set_mode(Mode::AcceptEdits);
        assert_eq!((b.need("t", None, &edit), b.need("t", None, &install)), (Need::No("permission mode"), Need::Yes(Tier::Amber)));
        b.set_mode(Mode::MissionAuto);
        assert_eq!((b.need("t", None, &edit), b.need("t", None, &install)), (Need::No("permission mode"), Need::No("permission mode")));
        b.set_mode(Mode::Optimistic);
        assert_eq!((b.need("t", None, &edit), b.need("t", None, &install)), (Need::No("permission mode"), Need::No("permission mode")));
        // specs/035 D-13: a task on a weaker model is approved by nothing but the operator, in every mode, until it ends.
        b.set_mode(Mode::Optimistic);
        b.mark_degraded("t");
        assert!(b.is_degraded("t") && !b.is_degraded("other"));
        assert_eq!((b.need("t", None, &edit), b.need("t", None, &install)), (Need::Yes(Tier::Amber), Need::Yes(Tier::Amber)));
        assert_eq!(b.need("other", None, &edit), Need::No("permission mode"), "another task is unaffected");
        b.end_task("t");
        assert_eq!(b.need("t", None, &edit), Need::No("permission mode"), "the end of the task ends it");
        assert_eq!(Mode::parse("mission-auto"), Some(Mode::MissionAuto));
        assert_eq!(Mode::parse("optimistic"), Some(Mode::Optimistic));
        assert_eq!(Mode::parse("yolo"), None);
    }

    #[tokio::test]
    async fn an_approval_resolves_once_and_the_event_payloads_carry_the_binding() {
        let b = Broker::new("ap_");
        let c = spec_write("hello");
        let w = b.request(req(&c, "t1", Duration::from_secs(60)));
        let pending = b.pending();
        assert_eq!(pending.len(), 1);
        let e = pending[0].event_payload();
        assert_eq!((e["tier"].as_str(), e["tool"].as_str()), (Some("amber"), Some("write_file")));
        assert_eq!(e["payload_blake3"], payload_blake3(&c.payload));
        let r = b.resolve(&w.approval_id, &payload_blake3(&c.payload), Answer::Allow, "console-1").unwrap();
        assert_eq!(r, Resolution::Approved { similar: false });
        assert_eq!(w.wait().await, r);
        assert!(b.pending().is_empty());
    }

    // specs/005 §6 test 18: bound answers
    #[tokio::test]
    async fn an_answer_for_a_different_payload_is_rejected_and_the_card_stays_open() {
        let b = Broker::new("ap_");
        let shown = spec_write("the diff the operator saw");
        let w = b.request(req(&shown, "t1", Duration::from_secs(60)));
        let swapped = payload_blake3(&spec_write("something else entirely").payload);
        assert_eq!(b.resolve(&w.approval_id, &swapped, Answer::Allow, "c1"), Err(ResolveError::PayloadMismatch));
        assert_eq!(b.pending().len(), 1, "still waiting for a correct answer");
        assert!(b.resolve(&w.approval_id, &payload_blake3(&shown.payload), Answer::Deny { reason: "no".into() }, "c1").is_ok());
        assert_eq!(w.wait().await, Resolution::Denied { reason: "no".into() });
    }

    // specs/005 §6 test 4: first answer wins
    #[tokio::test]
    async fn two_consoles_answering_at_once_apply_exactly_one_answer() {
        let b = Broker::new("ap_");
        let c = spec_write("x");
        let hash = payload_blake3(&c.payload);
        for _ in 0..50 {
            let w = b.request(req(&c, "t1", Duration::from_secs(60)));
            let (id, h1, h2) = (w.approval_id.clone(), hash.clone(), hash.clone());
            let (b1, b2, id1, id2) = (b.clone(), b.clone(), id.clone(), id.clone());
            let a = tokio::spawn(async move { b1.resolve(&id1, &h1, Answer::Allow, "console-A") });
            let n = tokio::spawn(async move { b2.resolve(&id2, &h2, Answer::Deny { reason: "n".into() }, "console-B") });
            let (ra, rn) = (a.await.unwrap(), n.await.unwrap());
            assert!(ra.is_ok() ^ rn.is_ok(), "exactly one answer applies: {:?} / {:?}", ra, rn);
            let winner = w.wait().await;
            let loser = if ra.is_ok() { rn } else { ra };
            match loser {
                Err(ResolveError::Gone { resolution: Some(r), by: Some(who) }) => {
                    assert_eq!(r, winner);
                    assert!(who == "console-A" || who == "console-B");
                }
                other => panic!("the loser should learn who won: {:?}", other),
            }
        }
    }

    // specs/005 §6 test 4 (orphan half) and §3.1
    #[tokio::test]
    async fn an_unanswered_approval_denies_at_the_timeout_with_no_console_attached() {
        let b = Broker::new("ap_");
        let c = spec_write("x");
        let w = b.request(req(&c, "t1", Duration::from_millis(60)));
        let t0 = std::time::Instant::now();
        assert_eq!(w.wait().await, Resolution::TimedOut);
        assert!(t0.elapsed() < Duration::from_secs(2));
        assert!(b.pending().is_empty());
        let late = b.resolve("ap_0001", &payload_blake3(&c.payload), Answer::Allow, "c1");
        assert_eq!(late, Err(ResolveError::Gone { resolution: Some(Resolution::TimedOut), by: Some("timeout".into()) }), "a late approval cannot resurrect a timed-out request");
    }

    #[tokio::test]
    async fn allow_similar_is_narrow_task_scoped_and_listed() {
        let b = Broker::new("ap_");
        let first = spec_write("a");
        let w = b.request(req(&first, "t1", Duration::from_secs(60)));
        assert_eq!(b.resolve(&w.approval_id, &payload_blake3(&first.payload), Answer::AllowSimilar, "c").unwrap(), Resolution::Approved { similar: true });
        // Same tool and pattern, different content, same task: covered.
        assert_eq!(b.need("t1", None, &spec_write("b")), Need::No("allowed similar earlier in this task"));
        // A different task, a different directory, or a different tool: asks.
        assert_eq!(b.need("t2", None, &spec_write("b")), Need::Yes(Tier::Amber));
        let other_dir = classify("write_file", &json!({"path": "context/domains/x.md", "content": "b"})).unwrap();
        assert_eq!(b.need("t1", None, &other_dir), Need::Yes(Tier::Amber));
        assert_eq!(b.allow_similar(), vec![("t1".to_string(), "write_file".to_string(), "write_file specs/*".to_string())]);
        b.end_task("t1");
        assert_eq!(b.need("t1", None, &spec_write("b")), Need::Yes(Tier::Amber), "the grant ends with the task");
        assert!(b.allow_similar().is_empty());
    }

    #[tokio::test]
    async fn allow_similar_never_covers_a_red_action() {
        let b = Broker::new("ap_");
        let w = b.request(red("t1"));
        let hash = b.pending()[0].payload_blake3.clone();
        assert_eq!(b.resolve(&w.approval_id, &hash, Answer::AllowSimilar, "c"), Err(ResolveError::SimilarNotAllowedForRed));
        assert!(b.resolve(&w.approval_id, &hash, Answer::Allow, "c").is_ok());
        assert!(b.allow_similar().is_empty());
    }

    #[tokio::test]
    async fn ending_a_task_denies_its_open_cards() {
        let b = Broker::new("ap_");
        let c = spec_write("x");
        let w1 = b.request(req(&c, "t1", Duration::from_secs(60)));
        let w2 = b.request(req(&c, "t2", Duration::from_secs(60)));
        b.end_task("t1");
        assert_eq!(w1.wait().await, Resolution::Denied { reason: "the task ended".into() });
        assert_eq!(b.pending().len(), 1);
        drop(w2);
    }

    #[test]
    fn the_binding_hash_ignores_key_order_and_whitespace_but_not_content() {
        let a: Value = serde_json::from_str(r#"{"b": [1, {"y": 2, "x": 1}], "a": "s"}"#).unwrap();
        let b: Value = serde_json::from_str("{\"a\":\"s\",\"b\":[1,{\"x\":1,\"y\":2}]}").unwrap();
        assert_eq!(payload_blake3(&a), payload_blake3(&b));
        let c: Value = serde_json::from_str(r#"{"a": "s", "b": [1, {"x": 1, "y": 3}]}"#).unwrap();
        assert_ne!(payload_blake3(&a), payload_blake3(&c));
        assert_ne!(payload_blake3(&json!([1, 2])), payload_blake3(&json!([2, 1])), "array order is content");
    }

    #[test]
    fn resolution_events_say_who_decided_and_why() {
        let e = Resolution::Denied { reason: "too risky".into() }.event_payload("ap_1", "console-A");
        assert_eq!((e["resolution"].as_str(), e["reason"].as_str(), e["by"].as_str()), (Some("denied"), Some("too risky"), Some("console-A")));
        assert_eq!(Resolution::TimedOut.event_payload("ap_1", "timeout")["resolution"], "timed_out");
    }
}
