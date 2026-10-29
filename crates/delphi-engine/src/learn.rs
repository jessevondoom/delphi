//! What the learning loop needs to know about a finished task, read from the event log (`specs/031` §3.4, slice L0).
//!
//! Nothing here writes anything: [`task_records`] folds a mission's events into one [`TaskRecord`] per task, so a rule's
//! or a skill's effect can later be compared across the tasks that carried it and the tasks that did not. The record is
//! derived, like the usage ledger, and can be rebuilt from `events.jsonl` at any time.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use delphi_core::event::EventEnvelope;
use delphi_core::provenance::Tier;

/// How a task ended, as far as the log says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `task.completed` with status `done`.
    Done,
    /// `task.completed` with status `escalated`: the operator was handed the decision.
    Escalated,
    /// `task.failed`.
    Failed,
    /// `task.cancelled`.
    Cancelled,
    /// No terminal event yet.
    Open,
}

/// One task, summarized.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRecord {
    pub task_id: String,
    pub outcome: Outcome,
    /// `(scope, id)` of every rule and lesson any span of the task carried, once each.
    pub rules: BTreeSet<(String, String)>,
    /// `build.attempt` events: 1 means the first try was the only one; 0 for a task that never builds.
    pub attempts: u32,
    /// Cards the operator denied during the task.
    pub denied_cards: u32,
    /// Provisional decisions the operator rejected at review (`decision.reverted`).
    pub reverted_decisions: u32,
    /// Money spent by the task's spans, from `agent.usage`.
    pub cost_usd: f64,
}

impl TaskRecord {
    /// Done on the first build attempt with nothing denied: the signal `specs/004` §5.6 calls first-pass success.
    /// Only meaningful for a task that builds (`attempts >= 1`).
    pub fn first_pass(&self) -> bool {
        self.outcome == Outcome::Done && self.attempts == 1 && self.denied_cards == 0 && self.reverted_decisions == 0
    }
}

/// Fold `events` into one record per task, in order of first appearance. Events without a `task_id` are skipped.
pub fn task_records(events: &[EventEnvelope]) -> Vec<TaskRecord> {
    let mut order: Vec<String> = Vec::new();
    let mut by: BTreeMap<String, TaskRecord> = BTreeMap::new();
    for e in events {
        let Some(task) = e.task_id.as_deref() else { continue };
        let rec = by.entry(task.to_string()).or_insert_with(|| {
            order.push(task.to_string());
            TaskRecord { task_id: task.to_string(), outcome: Outcome::Open, rules: BTreeSet::new(), attempts: 0, denied_cards: 0, reverted_decisions: 0, cost_usd: 0.0 }
        });
        match e.kind.as_str() {
            "snapshot.loaded" => {
                for r in e.payload["rules"].as_array().into_iter().flatten() {
                    if let (Some(scope), Some(id)) = (r["scope"].as_str(), r["id"].as_str()) {
                        rec.rules.insert((scope.to_string(), id.to_string()));
                    }
                }
            }
            "task.completed" => rec.outcome = if e.payload["status"] == "escalated" { Outcome::Escalated } else { Outcome::Done },
            "task.failed" => rec.outcome = Outcome::Failed,
            "task.cancelled" => rec.outcome = Outcome::Cancelled,
            "build.attempt" => rec.attempts += 1,
            "approval.resolved" if e.payload["resolution"] == "denied" => rec.denied_cards += 1,
            "decision.reverted" => rec.reverted_decisions += 1,
            "agent.usage" => rec.cost_usd += e.payload["cost_usd"].as_f64().unwrap_or(0.0),
            _ => {}
        }
    }
    order.into_iter().filter_map(|t| by.remove(&t)).collect()
}

// ── L1: the staging tier (`specs/031` §3.1, §3.2) ───────────────────────────────────────────────────────────────

/// A staged note that has not been dropped, read back from a mission's log. Derived: delete nothing, rebuild any time.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveNote {
    pub mission: String,
    pub id: String,
    pub trigger: String,
    pub summary: String,
    pub tier: String,
    pub task_id: Option<String>,
    pub role: Option<String>,
    pub timestamp: String,
    pub event_seqs: Vec<u64>,
}

impl LiveNote {
    /// `fact` or `episode`, from the capture trigger (`specs/050` ML-1).
    pub fn kind(&self) -> &'static str {
        crate::frontmatter::kind_for_trigger(&self.trigger)
    }

    /// `<mission>/<id>`: what `delphi learn show` and `drop` take.
    pub fn reference(&self) -> String {
        format!("{}/{}", self.mission, self.id)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({"ref": self.reference(), "mission": self.mission, "id": self.id, "trigger": self.trigger, "summary": self.summary, "tier": self.tier,
                           "task_id": self.task_id, "role": self.role, "at": self.timestamp, "event_seqs": self.event_seqs})
    }
}

/// The notes of one mission that are still live, oldest first: every `learn.staged` minus every `learn.dropped`.
pub fn live_notes(mission: &str, events: &[EventEnvelope]) -> Vec<LiveNote> {
    let mut notes: Vec<LiveNote> = Vec::new();
    for e in events {
        match e.kind.as_str() {
            "learn.staged" => {
                let Some(id) = e.payload["id"].as_str() else { continue };
                notes.push(LiveNote {
                    mission: mission.to_string(),
                    id: id.to_string(),
                    trigger: e.payload["trigger"].as_str().unwrap_or("").to_string(),
                    summary: e.payload["summary"].as_str().unwrap_or("").to_string(),
                    tier: e.payload["tier"].as_str().unwrap_or("L0").to_string(),
                    task_id: e.task_id.clone(),
                    role: e.agent.clone(),
                    timestamp: e.timestamp.clone(),
                    event_seqs: e.payload["event_seqs"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).collect(),
                });
            }
            "learn.dropped" => {
                if let Some(id) = e.payload["id"].as_str() {
                    notes.retain(|n| n.id != id);
                }
            }
            _ => {}
        }
    }
    notes
}

/// What `suggest_rule` may do and where: the vocabulary of `learn.proposed` (`specs/004` §4.2, `specs/031` §3.3).
pub const PROPOSAL_OPS: [&str; 3] = ["add", "merge", "demote"];
pub const PROPOSAL_SCOPES: [&str; 2] = ["workspace", "operator"];

/// A proposal's rule text is at most this many characters (the 350-character cap of a rule, `specs/004` §5.2).
pub const MAX_PROPOSAL_TEXT: usize = 350;

/// A proposal that has not been resolved, read back from a mission's log. Derived, like [`LiveNote`]; its id is
/// `p<seq>` of the `learn.proposed` event, so no counter is kept.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProposal {
    pub mission: String,
    pub id: String,
    pub op: String,
    pub scope: String,
    pub text: String,
    pub rationale: String,
    pub target: String,
    pub tier: String,
    pub task_id: Option<String>,
    pub timestamp: String,
}

impl LiveProposal {
    /// `<mission>/<id>`: what `delphi retro show` takes.
    pub fn reference(&self) -> String {
        format!("{}/{}", self.mission, self.id)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({"ref": self.reference(), "mission": self.mission, "id": self.id, "op": self.op, "scope": self.scope, "text": self.text,
                           "rationale": self.rationale, "target": self.target, "tier": self.tier, "task_id": self.task_id, "at": self.timestamp})
    }
}

/// The proposals of one mission still waiting for the operator: every `learn.proposed` minus every
/// `learn.proposal_resolved` (which names the `id`; the apply slice writes it).
pub fn live_proposals(mission: &str, events: &[EventEnvelope]) -> Vec<LiveProposal> {
    let mut out: Vec<LiveProposal> = Vec::new();
    for e in events {
        match e.kind.as_str() {
            "learn.proposed" => {
                let s = |k: &str| e.payload[k].as_str().unwrap_or("").to_string();
                // A malformed or out-of-vocabulary proposal is ignored, whatever wrote it.
                if !PROPOSAL_OPS.contains(&s("op").as_str()) || !PROPOSAL_SCOPES.contains(&s("scope").as_str()) || s("text").is_empty() {
                    continue;
                }
                let tier = e.payload["tier"].as_str().and_then(Tier::parse).unwrap_or(Tier::L0).min(Tier::L3);
                out.push(LiveProposal { mission: mission.to_string(), id: format!("p{}", e.seq), op: s("op"), scope: s("scope"), text: s("text"), rationale: s("rationale"), target: s("target"), tier: tier.as_str().to_string(), task_id: e.task_id.clone(), timestamp: e.timestamp.clone() });
            }
            "learn.proposal_resolved" => {
                if let Some(id) = e.payload["id"].as_str() {
                    out.retain(|p| p.id != id);
                }
            }
            _ => {}
        }
    }
    // A chatty mission cannot bury the operator's review: only the newest few stay listed.
    let excess = out.len().saturating_sub(MAX_LIVE_PROPOSALS);
    out.drain(..excess);
    out
}

/// Unresolved proposals listed per mission; older ones fall off the list (their events stay in the log).
pub const MAX_LIVE_PROPOSALS: usize = 20;

/// The five capture triggers the daemon can see without a model (`specs/031` §3.2 items 1-5), by the name a note and
/// `[learning] triggers` use.
pub const TRIGGERS: [&str; 6] = ["denied_card", "correction", "recovery", "escalation", "rejected_decision", "agent_note"];

/// Agent notes kept per task; the rest are ignored so one chatty task cannot push the operator's own notes out.
const AGENT_NOTES_PER_TASK: u32 = 3;

/// A staged note's summary is at most this many characters.
pub const MAX_SUMMARY: usize = 280;

/// `[learning]` in `config.toml`. Capture can be turned off or narrowed; nothing here can make staged text load.
#[derive(Debug, Clone, PartialEq)]
pub struct LearningSettings {
    pub capture: bool,
    /// Notes kept per mission; the oldest is dropped (`learn.dropped`, reason `evicted`) when a new one would exceed it.
    pub max_notes: usize,
    pub triggers: BTreeSet<String>,
}

impl Default for LearningSettings {
    fn default() -> Self {
        LearningSettings { capture: true, max_notes: 200, triggers: TRIGGERS.iter().map(|t| t.to_string()).collect() }
    }
}

impl LearningSettings {
    pub const MAX_NOTES_FLOOR: usize = 10;
    pub const MAX_NOTES_CEILING: usize = 1000;

    /// Read `[learning]` from a parsed `config.toml`. A value outside the floor and ceiling is clamped; an unknown key or
    /// trigger is a returned problem, for the boot notices.
    pub fn parse(root: &toml::Value) -> (LearningSettings, Vec<String>) {
        let mut s = LearningSettings::default();
        let mut problems = Vec::new();
        let Some(t) = root.get("learning").and_then(|v| v.as_table()) else { return (s, problems) };
        for k in t.keys() {
            if !["capture", "max_notes", "triggers"].contains(&k.as_str()) {
                problems.push(format!("[learning] {}: not a setting (capture, max_notes, triggers)", k));
            }
        }
        if let Some(v) = t.get("capture") {
            match v.as_bool() {
                Some(b) => s.capture = b,
                None => problems.push("[learning] capture: expected true or false".into()),
            }
        }
        if let Some(v) = t.get("max_notes") {
            match v.as_integer() {
                Some(n) => {
                    let c = (n.max(0) as usize).clamp(Self::MAX_NOTES_FLOOR, Self::MAX_NOTES_CEILING);
                    if c as i64 != n {
                        problems.push(format!("[learning] max_notes: {} is outside {}..={}; using {}", n, Self::MAX_NOTES_FLOOR, Self::MAX_NOTES_CEILING, c));
                    }
                    s.max_notes = c;
                }
                None => problems.push("[learning] max_notes: expected a whole number".into()),
            }
        }
        if let Some(v) = t.get("triggers") {
            match v.as_array() {
                Some(a) => {
                    let mut on = BTreeSet::new();
                    for name in a.iter().filter_map(|x| x.as_str()) {
                        if TRIGGERS.contains(&name) {
                            on.insert(name.to_string());
                        } else {
                            problems.push(format!("[learning] triggers: `{}` is not a trigger ({})", name, TRIGGERS.join(", ")));
                        }
                    }
                    s.triggers = on;
                }
                None => problems.push("[learning] triggers: expected a list of names".into()),
            }
        }
        (s, problems)
    }
}

/// One candidate lesson, before it is written as a `learn.staged` event.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedNote {
    pub trigger: &'static str,
    pub task_id: Option<String>,
    pub role: Option<String>,
    /// The `seq` of every event the note was built from.
    pub event_seqs: Vec<u64>,
    pub summary: String,
    /// The lowest tier among the sources: operator-typed text is `L5`; anything the daemon composed is `L3`; a task whose
    /// span read untrusted content (`span.taint`) caps it at that span's ceiling. Promotion refuses a note below `L4`.
    pub tier: Tier,
}

impl StagedNote {
    /// The note's id: the `seq` of the event that triggered it, so a replay names it the same way.
    pub fn id(&self) -> String {
        format!("n{}", self.event_seqs.last().copied().unwrap_or(0))
    }

    pub fn to_event(&self, mission_id: &str) -> EventEnvelope {
        let mut e = EventEnvelope::new(mission_id, "learn.staged", serde_json::json!({"id": self.id(), "trigger": self.trigger, "summary": self.summary, "event_seqs": self.event_seqs, "tier": self.tier.as_str()}));
        e.task_id = self.task_id.clone();
        e.agent = self.role.clone();
        e
    }
}

/// What observing one event asks the log to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Capture {
    Note(StagedNote),
    /// A live note pushed out by the cap: write `learn.dropped` with reason `evicted`.
    Evict(String),
}

fn clip(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= MAX_SUMMARY {
        return one;
    }
    let mut t: String = one.chars().take(MAX_SUMMARY - 1).collect();
    t.push('…');
    t
}

// ── the `staged` knowledge-base zone (`specs/031` §3.1) ─────────────────────────────────────────────────────────

/// The kb `source_type` a note's tier maps to, so the zone's taint rules are the knowledge base's own.
pub fn source_type_for(tier: &str) -> &'static str {
    match tier {
        "L5" => "user_ratified",
        "L4" => "internal_empirical",
        "L3" => "internal_generated",
        "L2" => "federated_verified",
        "L1" => "federated_peer",
        _ => "external_untrusted",
    }
}

/// One note as a Markdown document for the knowledge base: labelled with where it came from and how far to trust it.
pub fn note_document(n: &LiveNote) -> String {
    format!(
        "---\nsource_type: {}\nauthor: daemon\nconfidence: low\nstatus: active\nkind: {}\n{}---\n# Staged note {}\n\nStaged note ({} · tier {} · {} · mission {}{}{}). A candidate lesson only: not ratified, not guidance.\n\n{}\n",
        source_type_for(&n.tier),
        n.kind(),
        if n.timestamp.is_empty() { String::new() } else { format!("created: {0}\nlast_confirmed: {0}\n", n.timestamp) },
        n.id,
        n.trigger,
        n.tier,
        n.timestamp,
        n.mission,
        n.task_id.as_deref().map(|t| format!(" · task {}", t)).unwrap_or_default(),
        n.role.as_deref().map(|r| format!(" · {}", r)).unwrap_or_default(),
        n.summary
    )
}

fn plain(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn newest_event_mtime(mission_dir: &Path) -> Option<std::time::SystemTime> {
    std::fs::read_dir(mission_dir.join("events")).ok()?.flatten().filter_map(|e| e.metadata().and_then(|m| m.modified()).ok()).max()
}

/// Bring `<ws>/local/staged/` in line with the mission logs: one Markdown file per live note, `<mission>/<id>.md`.
/// A projection, like `status.md`: the logs are the truth, a failure here is ignored, and the knowledge base indexes
/// the files as zone `staged`. A mission whose log has not changed since it was last projected is not read again.
pub fn project_staged(ws_dir: &Path) {
    let out = ws_dir.join("local/staged");
    let Ok(rd) = std::fs::read_dir(ws_dir.join("local/missions")) else {
        let _ = std::fs::remove_dir_all(&out);
        return;
    };
    let mut present = BTreeSet::new();
    for e in rd.flatten().filter(|e| e.path().is_dir()) {
        let name = e.file_name().to_string_lossy().to_string();
        if !plain(&name) {
            continue;
        }
        present.insert(name.clone());
        let target = out.join(&name);
        let stamp = target.join(".synced");
        let fresh = match (std::fs::metadata(&stamp).and_then(|m| m.modified()).ok(), newest_event_mtime(&e.path())) {
            (Some(s), Some(ev)) => s >= ev,
            (Some(_), None) => true,
            _ => false,
        };
        if fresh {
            continue;
        }
        let Ok(events) = delphi_core::log::replay(&e.path()) else { continue };
        let notes = live_notes(&name, &events);
        if notes.is_empty() {
            let _ = std::fs::remove_dir_all(&target);
            continue;
        }
        if std::fs::create_dir_all(&target).is_err() {
            continue;
        }
        let mut keep = BTreeSet::new();
        for n in &notes {
            if !plain(&n.id) {
                continue;
            }
            let file = target.join(format!("{}.md", n.id));
            let doc = note_document(n);
            if std::fs::read_to_string(&file).ok().as_deref() != Some(doc.as_str()) {
                let _ = std::fs::write(&file, doc);
            }
            keep.insert(format!("{}.md", n.id));
        }
        if let Ok(old) = std::fs::read_dir(&target) {
            for f in old.flatten() {
                let fname = f.file_name().to_string_lossy().to_string();
                if fname.ends_with(".md") && !keep.contains(&fname) {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        let _ = std::fs::write(&stamp, "");
    }
    if let Ok(old) = std::fs::read_dir(&out) {
        for d in old.flatten().filter(|d| d.path().is_dir()) {
            if !present.contains(d.file_name().to_string_lossy().as_ref()) {
                let _ = std::fs::remove_dir_all(d.path());
            }
        }
    }
}

/// A card reason the daemon or a headless run wrote, not a person: never a lesson.
fn machine_reason(r: &str) -> bool {
    ["headless run:", "blocked by the hook", "the task was cancelled", "the daemon restarted"].iter().any(|p| r.starts_with(p))
}

/// Explicit cues only (`specs/031` §11 Q1): a message that opens by saying no, stop or don't. Its precision is measured in
/// training mode before this widens.
pub fn is_correction(prompt: &str) -> bool {
    let p = prompt.trim_start().to_lowercase();
    p.chars().count() >= 8 && ["no,", "no.", "no ", "don't ", "do not ", "stop ", "that's wrong", "that is wrong", "that's not", "that is not", "wrong,", "actually,", "not like that", "undo "].iter().any(|c| p.starts_with(c))
}

/// Cheap per-mission capture state. Feed it every event of the mission in order (the log, then each new append); it
/// answers with the notes to stage. No I/O, no model call, and a `learn.*` event never triggers another.
#[derive(Debug, Default)]
pub struct Stager {
    settings: LearningSettings,
    /// Highest `build.attempt` per task.
    attempts: BTreeMap<String, u32>,
    /// The lowest tier ceiling a task's spans reached (`span.taint`).
    ceiling: BTreeMap<String, Tier>,
    /// `seq` of the attempt-2-or-3 event per task still waiting to see whether the task finishes.
    retried: BTreeMap<String, u64>,
    tasks_seen: u32,
    /// `note` tool calls kept per task.
    agent_notes: BTreeMap<String, u32>,
    /// Ids of live notes, oldest first.
    live: std::collections::VecDeque<String>,
}

impl Stager {
    pub fn new(settings: LearningSettings) -> Stager {
        Stager { settings, ..Default::default() }
    }

    pub fn set_settings(&mut self, settings: LearningSettings) {
        self.settings = settings;
    }

    /// Rebuild the state from a mission's events. Notes already in the log are not repeated; the returned state only
    /// remembers them, so the cap holds across a restart.
    pub fn replay(settings: LearningSettings, events: &[EventEnvelope]) -> Stager {
        let mut s = Stager::new(LearningSettings { capture: false, ..settings.clone() });
        for e in events {
            s.observe(e);
        }
        s.settings = settings;
        s
    }

    fn on(&self, trigger: &str) -> bool {
        self.settings.capture && self.settings.triggers.contains(trigger)
    }

    /// The tier a note from `task` may claim: `base`, lowered to the task's span ceiling.
    fn tier(&self, task: Option<&str>, base: Tier) -> Tier {
        task.and_then(|t| self.ceiling.get(t)).map_or(base, |c| base.min(*c))
    }

    pub fn observe(&mut self, e: &EventEnvelope) -> Vec<Capture> {
        let task = e.task_id.as_deref();
        let mk = |s: &Stager, trigger: &'static str, summary: String, seqs: Vec<u64>, base: Tier| {
            Capture::Note(StagedNote { trigger, task_id: task.map(String::from), role: e.agent.clone(), event_seqs: seqs, summary: clip(&summary), tier: s.tier(task, base) })
        };
        let mut out = Vec::new();
        match e.kind.as_str() {
            "learn.staged" => {
                // A live capture is counted when it is decided; its own event arrives here a moment later.
                if let Some(id) = e.payload["id"].as_str().filter(|id| !self.live.iter().any(|l| l == id)) {
                    self.live.push_back(id.to_string());
                }
                return out;
            }
            "learn.dropped" => {
                if let Some(id) = e.payload["id"].as_str() {
                    self.live.retain(|l| l != id);
                }
                return out;
            }
            // The `note` tool's request. It is the one `learn.*` event that makes a note; the note it makes is `learn.staged`.
            "learn.note" => {
                let kept = task.map(|t| self.agent_notes.entry(t.to_string()).or_insert(0));
                let allowed = kept.is_none_or(|n| {
                    *n += 1;
                    *n <= AGENT_NOTES_PER_TASK
                });
                let text = e.payload["text"].as_str().unwrap_or("").trim();
                if allowed && self.on("agent_note") && !text.is_empty() {
                    // Never above L3, whatever the payload claims.
                    let base = e.payload["tier"].as_str().and_then(Tier::parse).unwrap_or(Tier::L3).min(Tier::L3);
                    out.push(mk(self, "agent_note", text.to_string(), vec![e.seq], base));
                }
            }
            k if k.starts_with("learn.") => return out,
            "span.taint" => {
                if let (Some(t), Some(c)) = (task, e.payload["ceiling"].as_str().and_then(Tier::parse)) {
                    let slot = self.ceiling.entry(t.to_string()).or_insert(c);
                    *slot = (*slot).min(c);
                }
            }
            "task.created" => {
                let prompt = e.payload["prompt"].as_str().unwrap_or("");
                // A correction needs something to correct: an earlier task in this mission, and an explicit cue.
                if self.tasks_seen > 0 && self.on("correction") && is_correction(prompt) {
                    out.push(mk(self, "correction", format!("The operator corrected the team: {}", prompt), vec![e.seq], Tier::L5));
                }
                self.tasks_seen += 1;
            }
            "approval.resolved" if e.payload["resolution"] == "denied" && self.on("denied_card") => {
                let reason = e.payload["reason"].as_str().unwrap_or("").trim();
                if !reason.is_empty() && !machine_reason(reason) {
                    out.push(mk(self, "denied_card", format!("A card was denied: {}", reason), vec![e.seq], Tier::L5));
                }
            }
            "build.attempt" => {
                if let Some(t) = task {
                    let n = e.payload["attempt"].as_u64().unwrap_or(0) as u32;
                    let top = self.attempts.entry(t.to_string()).or_insert(0);
                    *top = (*top).max(n);
                    if (2..=3).contains(&n) {
                        self.retried.entry(t.to_string()).or_insert(e.seq);
                    }
                }
            }
            "decision.reverted" if self.on("rejected_decision") => {
                let path = e.payload["path"].as_str().unwrap_or("a file");
                out.push(mk(self, "rejected_decision", format!("A provisional decision about {} was rejected at review.", path), vec![e.seq], Tier::L3));
            }
            "task.completed" => {
                let tried = task.and_then(|t| self.attempts.get(t)).copied().unwrap_or(0);
                if e.payload["status"] == "escalated" {
                    if self.on("escalation") {
                        let reason = e.payload["reason"].as_str().unwrap_or("").trim();
                        let reason = if reason.is_empty() { "no reason recorded" } else { reason };
                        out.push(mk(self, "escalation", format!("A task was escalated to the operator after {} build attempt(s): {}", tried, reason), vec![e.seq], Tier::L3));
                    }
                } else if let Some(first) = task.and_then(|t| self.retried.remove(t)) {
                    if self.on("recovery") {
                        out.push(mk(self, "recovery", format!("A task passed on build attempt {} after earlier attempts failed.", tried), vec![first, e.seq], Tier::L3));
                    }
                }
            }
            _ => {}
        }
        // The cap counts live notes including the ones about to be written; the oldest go first.
        let new = out.len();
        let mut evict = Vec::new();
        for _ in 0..(self.live.len() + new).saturating_sub(self.settings.max_notes) {
            match self.live.pop_front() {
                Some(id) => evict.push(Capture::Evict(id)),
                None => break,
            }
        }
        for c in &out {
            if let Capture::Note(n) = c {
                self.live.push_back(n.id());
            }
        }
        evict.extend(out);
        evict
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn ev(task: Option<&str>, kind: &str, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m_1", kind, payload);
        e.task_id = task.map(String::from);
        e
    }

    #[test]
    fn a_first_pass_build_is_done_on_attempt_one_with_nothing_denied() {
        let evs = vec![
            ev(Some("t1"), "snapshot.loaded", json!({"rules": [{"scope": "release", "id": "H-001"}, {"scope": "workspace-lessons", "id": "L-BLD-001"}]})),
            ev(Some("t1"), "build.attempt", json!({"attempt": 1})),
            ev(Some("t1"), "agent.usage", json!({"cost_usd": 0.25})),
            ev(Some("t1"), "agent.usage", json!({"cost_usd": 0.5})),
            ev(Some("t1"), "task.completed", json!({"status": "done"})),
        ];
        let r = task_records(&evs);
        assert_eq!(r.len(), 1);
        assert!(r[0].first_pass() && r[0].cost_usd == 0.75);
        assert_eq!(r[0].rules.len(), 2);
    }

    #[test]
    fn retries_denials_reverts_and_escalation_are_not_first_pass() {
        let done = json!({"status": "done"});
        let b = || ev(Some("t"), "build.attempt", json!({}));
        let retried = task_records(&[b(), b(), ev(Some("t"), "task.completed", done.clone())]);
        assert!(!retried[0].first_pass() && retried[0].attempts == 2);
        let denied = task_records(&[b(), ev(Some("t"), "approval.resolved", json!({"resolution": "denied"})), ev(Some("t"), "task.completed", done.clone())]);
        assert!(!denied[0].first_pass() && denied[0].denied_cards == 1);
        let approved = task_records(&[b(), ev(Some("t"), "approval.resolved", json!({"resolution": "approved"})), ev(Some("t"), "task.completed", done.clone())]);
        assert!(approved[0].first_pass(), "an approved card is not a rejection");
        let reverted = task_records(&[b(), ev(Some("t"), "decision.reverted", json!({})), ev(Some("t"), "task.completed", done)]);
        assert!(!reverted[0].first_pass() && reverted[0].reverted_decisions == 1);
        let esc = task_records(&[b(), ev(Some("t"), "task.completed", json!({"status": "escalated", "reason": "budget"}))]);
        assert_eq!(esc[0].outcome, Outcome::Escalated);
        assert!(!esc[0].first_pass());
    }

    #[test]
    fn spans_of_one_task_pool_their_rules_once_and_tasks_stay_apart() {
        let loaded = |t: &str| ev(Some(t), "snapshot.loaded", json!({"rules": [{"scope": "release", "id": "H-001"}]}));
        let evs = vec![loaded("a"), loaded("a"), loaded("b"), ev(None, "mission.note", json!({})), ev(Some("a"), "task.failed", json!({"error": "x"})), ev(Some("b"), "task.cancelled", json!({}))];
        let r = task_records(&evs);
        assert_eq!(r.iter().map(|t| t.task_id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(r[0].rules.len(), 1);
        assert_eq!((r[0].outcome.clone(), r[1].outcome.clone()), (Outcome::Failed, Outcome::Cancelled));
    }

    #[test]
    fn a_task_with_no_terminal_event_is_open() {
        assert_eq!(task_records(&[ev(Some("t"), "snapshot.loaded", json!({"rules": []}))])[0].outcome, Outcome::Open);
    }

    // ── L1 ──

    fn at(seq: u64, task: Option<&str>, kind: &str, payload: Value) -> EventEnvelope {
        let mut e = ev(task, kind, payload);
        e.seq = seq;
        e
    }

    /// Run `events` through a fresh stager and keep only the notes.
    fn stage(events: &[EventEnvelope]) -> Vec<StagedNote> {
        let mut s = Stager::new(LearningSettings::default());
        events.iter().flat_map(|e| s.observe(e)).filter_map(|c| if let Capture::Note(n) = c { Some(n) } else { None }).collect()
    }

    #[test]
    fn a_denied_card_with_a_reason_is_one_operator_note() {
        let n = stage(&[at(5, Some("t1"), "approval.resolved", json!({"resolution": "denied", "reason": "never touch the lockfile", "by": "console"}))]);
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].trigger, n[0].tier, n[0].event_seqs.clone(), n[0].id()), ("denied_card", Tier::L5, vec![5], "n5".into()));
        assert!(n[0].summary.contains("never touch the lockfile"));
    }

    #[test]
    fn denials_nobody_typed_are_not_lessons() {
        for reason in ["", "  ", "headless run: --on-card deny", "blocked by the hook `x`: no", "the daemon restarted; asked again"] {
            assert!(stage(&[at(1, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": reason}))]).is_empty(), "{:?}", reason);
        }
        assert!(stage(&[at(1, Some("t"), "approval.resolved", json!({"resolution": "approved", "reason": "fine"}))]).is_empty());
    }

    #[test]
    fn a_correction_needs_an_earlier_task_and_an_explicit_cue() {
        let first = at(1, Some("t1"), "task.created", json!({"prompt": "no, wait, do it for me"}));
        assert!(stage(std::slice::from_ref(&first)).is_empty(), "nothing before it to correct");
        let n = stage(&[first, at(9, Some("t2"), "task.created", json!({"prompt": "No, use the existing helper instead"}))]);
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].trigger, n[0].tier), ("correction", Tier::L5));
        let plain = stage(&[at(1, Some("t1"), "task.created", json!({"prompt": "add a flag"})), at(2, Some("t2"), "task.created", json!({"prompt": "now add a test too"}))]);
        assert!(plain.is_empty());
    }

    #[test]
    fn recovery_is_noted_once_when_the_task_finishes_after_attempt_two_or_three() {
        let evs = |last: Value| {
            vec![
                at(1, Some("t"), "build.attempt", json!({"attempt": 1})),
                at(2, Some("t"), "build.attempt", json!({"attempt": 2})),
                at(3, Some("t"), "build.attempt", json!({"attempt": 3})),
                at(4, Some("t"), "task.completed", last),
            ]
        };
        let n = stage(&evs(json!({"status": "done"})));
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].trigger, n[0].event_seqs.clone(), n[0].tier), ("recovery", vec![2, 4], Tier::L3));
        let first_try = stage(&[at(1, Some("t"), "build.attempt", json!({"attempt": 1})), at(2, Some("t"), "task.completed", json!({"status": "done"}))]);
        assert!(first_try.is_empty());
        let esc = stage(&evs(json!({"status": "escalated", "reason": "gates failed"})));
        assert_eq!(esc.iter().map(|n| n.trigger).collect::<Vec<_>>(), vec!["escalation"], "an escalated task did not recover");
        assert!(esc[0].summary.contains("3 build attempt") && esc[0].summary.contains("gates failed"));
    }

    #[test]
    fn a_rejected_provisional_decision_is_noted() {
        let n = stage(&[at(7, Some("t"), "decision.reverted", json!({"decision_id": "d", "path": "src/a.rs"}))]);
        assert_eq!((n.len(), n[0].trigger), (1, "rejected_decision"));
        assert!(n[0].summary.contains("src/a.rs"));
    }

    #[test]
    fn a_note_from_a_task_that_read_untrusted_content_stays_below_empirical() {
        let canary = "CANARY-9f3a";
        let n = stage(&[
            at(1, Some("t"), "span.taint", json!({"ceiling": "L0", "cause": "web"})),
            at(2, Some("t"), "tool.result", json!({"name": "web_fetch", "text": canary})),
            at(3, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": "not that way"})),
        ]);
        assert_eq!(n.len(), 1);
        assert!(n[0].tier < Tier::L4, "tainted: {:?}", n[0].tier);
        assert!(!n[0].summary.contains(canary) && !serde_json::to_string(&n[0].to_event("m")).unwrap().contains(canary), "tool output never reaches a note");
        let other = stage(&[at(1, Some("a"), "span.taint", json!({"ceiling": "L0"})), at(2, Some("b"), "approval.resolved", json!({"resolution": "denied", "reason": "no"}))]);
        assert_eq!(other[0].tier, Tier::L5, "another task's taint does not leak");
    }

    #[test]
    fn learn_events_never_trigger_and_summaries_are_clipped() {
        let mut s = Stager::new(LearningSettings::default());
        assert!(s.observe(&at(1, Some("t"), "learn.staged", json!({"id": "n1", "resolution": "denied", "reason": "x"}))).is_empty());
        let long = "word ".repeat(200);
        let n = stage(&[at(1, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": long}))]);
        assert!(n[0].summary.chars().count() <= MAX_SUMMARY && n[0].summary.ends_with('…'));
    }

    #[test]
    fn the_cap_evicts_the_oldest_and_a_replay_remembers_what_is_live() {
        let settings = LearningSettings { max_notes: 10, ..Default::default() };
        let mut s = Stager::new(settings.clone());
        let mut log = Vec::new();
        for i in 0..12u64 {
            let e = at(100 + i, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": format!("reason {}", i)}));
            for c in s.observe(&e) {
                match c {
                    Capture::Note(n) => log.push(at(200 + i, Some("t"), "learn.staged", json!({"id": n.id()}))),
                    Capture::Evict(id) => log.push(at(200 + i, None, "learn.dropped", json!({"id": id, "reason": "evicted"}))),
                }
            }
        }
        let dropped: Vec<_> = log.iter().filter(|e| e.kind == "learn.dropped").map(|e| e.payload["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(dropped, vec!["n100", "n101"]);
        // The restart: replaying the log restores the live set without producing anything, so the next note evicts n102.
        let mut again = Stager::replay(settings, &log);
        let out = again.observe(&at(500, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": "one more"})));
        assert_eq!(out.iter().filter(|c| matches!(c, Capture::Evict(id) if id == "n102")).count(), 1, "{:?}", out);
    }

    #[test]
    fn an_agent_note_is_staged_below_trusted_tiers_and_three_per_task_are_kept() {
        let note = |seq: u64, task: &str, tier: &str| at(seq, Some(task), "learn.note", json!({"text": format!("note {}", seq), "tier": tier}));
        let n = stage(&[note(1, "t", "L3"), note(2, "t", "L3"), note(3, "t", "L3"), note(4, "t", "L3"), note(5, "u", "L3")]);
        assert_eq!(n.iter().map(|n| n.event_seqs[0]).collect::<Vec<_>>(), vec![1, 2, 3, 5], "the fourth note of task t is ignored, another task is unaffected");
        assert!(n.iter().all(|n| n.trigger == "agent_note" && n.tier == Tier::L3));
        let boosted = stage(&[note(1, "t", "L5")]);
        assert_eq!(boosted[0].tier, Tier::L3, "an agent can never claim more than the daemon's own tier");
        let tainted = stage(&[at(1, Some("t"), "span.taint", json!({"ceiling": "L1"})), note(2, "t", "L3")]);
        assert_eq!(tainted[0].tier, Tier::L1);
        let off = Stager::new(LearningSettings { triggers: ["escalation".to_string()].into(), ..Default::default() }).observe(&note(1, "t", "L3"));
        assert!(off.is_empty());
    }

    #[test]
    fn capture_off_and_single_triggers_can_be_switched_off() {
        let denied = at(1, Some("t"), "approval.resolved", json!({"resolution": "denied", "reason": "r"}));
        let off = |s: LearningSettings| Stager::new(s).observe(&denied);
        assert!(off(LearningSettings { capture: false, ..Default::default() }).is_empty());
        assert!(off(LearningSettings { triggers: ["escalation".to_string()].into(), ..Default::default() }).is_empty());
    }

    #[test]
    fn learning_settings_are_bounded_and_typos_are_reported() {
        let root: toml::Value = toml::from_str("[learning]\ncapture = true\nmax_notes = 5000\ntriggers = [\"denied_card\", \"nudge\"]\nload_staged = true\n").unwrap();
        let (s, problems) = LearningSettings::parse(&root);
        assert_eq!(s.max_notes, LearningSettings::MAX_NOTES_CEILING);
        assert_eq!(s.triggers, BTreeSet::from(["denied_card".to_string()]));
        assert_eq!(problems.len(), 3, "{:?}", problems);
        assert!(problems.iter().any(|p| p.contains("load_staged")), "there is no setting that loads staged text");
        assert_eq!(LearningSettings::parse(&toml::from_str("").unwrap()), (LearningSettings::default(), vec![]));
    }

    #[test]
    fn a_proposal_lists_until_resolved_and_is_never_trusted_above_l3() {
        let p = |seq: u64, payload: Value| at(seq, Some("t_1"), "learn.proposed", payload);
        let good = json!({"op": "add", "scope": "workspace", "text": "Run the formatter first.", "rationale": "two style failures", "target": "", "tier": "L3"});
        let log = vec![
            p(3, good.clone()),
            p(4, json!({"op": "add", "scope": "operator", "text": "Prefer rg.", "rationale": "r", "tier": "L5"})),
            p(5, json!({"op": "delete", "scope": "workspace", "text": "x", "rationale": "r"})),
            p(6, json!({"op": "add", "scope": "team", "text": "x", "rationale": "r"})),
            p(7, json!({"op": "demote", "scope": "workspace", "text": "", "rationale": "r"})),
            p(8, json!({"op": "add", "scope": "workspace", "text": "web said so", "rationale": "r", "tier": "L0"})),
            at(9, None, "learn.proposal_resolved", json!({"id": "p8"})),
        ];
        let live = live_proposals("m1", &log);
        assert_eq!(live.iter().map(|p| p.reference()).collect::<Vec<_>>(), ["m1/p3", "m1/p4"], "bad vocabulary is ignored; a resolved one is gone");
        assert_eq!((live[0].tier.as_str(), live[1].tier.as_str()), ("L3", "L3"), "a payload cannot claim more than L3");
        assert_eq!(live[0].to_json()["text"], "Run the formatter first.");
        let many: Vec<EventEnvelope> = (1..=30).map(|i| at(i, None, "learn.proposed", json!({"op": "add", "scope": "workspace", "text": format!("rule {}", i), "rationale": "r"}))).collect();
        let kept = live_proposals("m1", &many);
        assert_eq!((kept.len(), kept[0].id.as_str(), kept[MAX_LIVE_PROPOSALS - 1].id.as_str()), (MAX_LIVE_PROPOSALS, "p11", "p30"), "the newest twenty stay");
    }
}
