//! Provisional decisions (`specs/015`): the optimistic workflow.
//!
//! When the operator asks for it (`/build --optimistic`), an approval card whose every effect stays inside the
//! task's branch is decided by an approver instead of waiting for a person, the decision is written to the log,
//! and **the merge is blocked until the operator has reviewed and ratified every decision** (gate 6).
//!
//! A provisional decision is not an approval. It lets work *proceed* on a branch that can be thrown away; the
//! operator's later review is the approval, and nothing reaches their checkout, an outside service or the host
//! without it. So:
//!
//! * [`eligible`] is an **allow-list**. A new kind of action is ineligible until someone decides otherwise here.
//! * The ledger is written by the daemon from what it saw, never by an agent, so an agent cannot leave an entry out.
//! * Reviews are bound to the decision's payload hash: a review of one thing cannot ratify another.

use crate::approval::Tier;
use crate::gates::{self, DecisionCounts};
use crate::mission::{EngineError, MissionState};
use crate::tools::{ActionKind, Classified};
use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};

/// Raised once when a task is waiting for the operator to review its decisions.
pub const REVIEW_REQUESTED: &str = "decisions.review_requested";

/// May this action be decided provisionally? Only a worktree edit the operator would otherwise be asked about.
/// Everything else (a command on the host, a write outside the worktree, a spec, an outbound action, a merge, a
/// Red card of any kind) waits for a person, as it always has.
pub fn eligible(c: &Classified, tier: Tier) -> bool {
    tier == Tier::Amber && c.kind == ActionKind::WorktreeEdit
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// An ordinary edit. May be ratified in bulk.
    Routine,
    /// An edit to something that deserves a look of its own (a dependency manifest, CI configuration). Never
    /// ratified in bulk.
    Elevated,
}

impl Risk {
    pub fn as_str(&self) -> &'static str {
        match self {
            Risk::Routine => "routine",
            Risk::Elevated => "elevated",
        }
    }
}

/// What the approver decided.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub allow: bool,
    pub rationale: String,
    pub risk: Risk,
}

/// The first approver: rules, no model. It allows an ordinary edit inside the worktree and refuses the ones that
/// would run on the operator's machine or change the rules agents work under. A model-backed approver (the
/// Strategist) replaces it behind the same call.
pub struct RuleApprover<'a> {
    /// `host_exec_paths` from `[exec]`.
    pub host_exec: &'a [String],
}

impl RuleApprover<'_> {
    pub fn decide(&self, c: &Classified) -> Verdict {
        let path = c.payload["path"].as_str().unwrap_or("");
        let inner = path.strip_prefix("@worktree/").unwrap_or(path);
        if inner.is_empty() || inner.split('/').any(|s| s == "..") || inner.starts_with('/') {
            return Verdict { allow: false, rationale: format!("`{}` is not a plain path inside the worktree", path), risk: Risk::Routine };
        }
        if gates::is_protected_path(inner) {
            return Verdict { allow: false, rationale: format!("`{}` is protected governance: it changes only through a ratified proposal", inner), risk: Risk::Elevated };
        }
        if crate::worktree::is_host_exec(inner, self.host_exec) {
            return Verdict { allow: false, rationale: format!("`{}` would run on the operator's machine once merged; that needs a person", inner), risk: Risk::Elevated };
        }
        if gates::is_dependency_file(inner) {
            return Verdict { allow: true, rationale: format!("`{}` is a dependency manifest or lockfile inside the worktree; allowed, flagged for a closer look", inner), risk: Risk::Elevated };
        }
        Verdict { allow: true, rationale: format!("an ordinary edit to `{}` inside the task's worktree", inner), risk: Risk::Routine }
    }
}

/// A reviewed decision's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    /// `ratified` or `rejected`.
    pub resolution: String,
    pub note: String,
    pub by: String,
}

/// One entry of the ledger, with its review if it has one.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub id: String,
    pub seq: u64,
    pub task_id: String,
    pub role: String,
    pub tool: String,
    pub summary: String,
    pub payload_blake3: String,
    /// `allow` or `deny`.
    pub outcome: String,
    pub rationale: String,
    pub risk: String,
    pub effects: Vec<Value>,
    pub review: Option<Review>,
}

impl Decision {
    pub fn to_json(&self) -> Value {
        json!({
            "decision_id": self.id, "task_id": self.task_id, "role": self.role, "tool": self.tool, "summary": self.summary,
            "payload_blake3": self.payload_blake3, "outcome": self.outcome, "rationale": self.rationale, "risk": self.risk, "effects": self.effects,
            "review": self.review.as_ref().map(|r| json!({"resolution": r.resolution, "note": r.note, "by": r.by})),
        })
    }
}

/// The payload of a `decision.provisional` event. The effects are what the daemon classified, not what the agent
/// says it will do.
pub fn decision_event(id: &str, c: &Classified, tier: Tier, v: &Verdict, role: &str) -> Value {
    let path = c.payload["path"].as_str().unwrap_or("");
    json!({
        "decision_id": id, "tool": c.tool, "tier": tier.as_str(), "pattern": c.pattern, "summary": c.summary,
        "payload_blake3": crate::approval::payload_blake3(&c.payload),
        "effects": [{"kind": "worktree_edit", "path": path, "bytes": c.payload["bytes"], "blake3": c.payload["blake3"]}],
        "decided_by": {"kind": "rule", "name": "rule-approver", "role": role},
        "outcome": if v.allow { "allow" } else { "deny" },
        "rationale": v.rationale, "risk": v.risk.as_str(),
    })
}

pub fn new_decision_id() -> String {
    let t = delphi_core::server::random_token().unwrap_or_else(|_| "0".repeat(64));
    format!("dec_{}", &t[..10])
}

/// The ledger of one task, in the order decisions were made, reviews folded in. Pure over the log.
pub fn ledger(events: &[EventEnvelope], task_id: &str) -> Vec<Decision> {
    let mut out: Vec<Decision> = Vec::new();
    for e in events.iter().filter(|e| e.task_id.as_deref() == Some(task_id)) {
        let p = &e.payload;
        match e.kind.as_str() {
            "decision.provisional" => out.push(Decision {
                id: p["decision_id"].as_str().unwrap_or("").into(),
                seq: e.seq,
                task_id: task_id.into(),
                role: e.agent.clone().unwrap_or_default(),
                tool: p["tool"].as_str().unwrap_or("").into(),
                summary: p["summary"].as_str().unwrap_or("").into(),
                payload_blake3: p["payload_blake3"].as_str().unwrap_or("").into(),
                outcome: p["outcome"].as_str().unwrap_or("").into(),
                rationale: p["rationale"].as_str().unwrap_or("").into(),
                risk: p["risk"].as_str().unwrap_or("routine").into(),
                effects: p["effects"].as_array().cloned().unwrap_or_default(),
                review: None,
            }),
            "decision.reviewed" => {
                // The latest review of a decision wins, so an operator can change their mind before the merge.
                if let Some(d) = out.iter_mut().find(|d| d.id == p["decision_id"].as_str().unwrap_or("\u{0}")) {
                    d.review = Some(Review { resolution: p["resolution"].as_str().unwrap_or("").into(), note: p["note"].as_str().unwrap_or("").into(), by: p["by"].as_str().unwrap_or("").into() });
                }
            }
            _ => {}
        }
    }
    out
}

/// Every task's ledger in a mission, tasks in the order they first made a decision.
pub fn ledger_all(events: &[EventEnvelope]) -> Vec<Decision> {
    let mut tasks: Vec<String> = Vec::new();
    for e in events.iter().filter(|e| e.kind == "decision.provisional") {
        if let Some(t) = &e.task_id {
            if !tasks.contains(t) {
                tasks.push(t.clone());
            }
        }
    }
    tasks.iter().flat_map(|t| ledger(events, t)).collect()
}

pub fn counts(ds: &[Decision]) -> DecisionCounts {
    let allowed: Vec<&Decision> = ds.iter().filter(|d| d.outcome == "allow").collect();
    DecisionCounts {
        allowed: allowed.len(),
        pending: allowed.iter().filter(|d| d.review.is_none()).map(|d| d.id.clone()).collect(),
        rejected: allowed.iter().filter(|d| d.review.as_ref().is_some_and(|r| r.resolution == "rejected")).map(|d| d.id.clone()).collect(),
    }
}

/// The ledger of a task, read from its mission's log.
pub fn ledger_of(m: &MissionState, task_id: &str) -> Result<Vec<Decision>, EngineError> {
    Ok(ledger(&m.replay()?, task_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolTier;

    fn edit(path: &str) -> Classified {
        Classified { tool: "write_file".into(), tier: ToolTier::Amber, kind: ActionKind::WorktreeEdit, summary: format!("write {}", path), payload: json!({"tool": "write_file", "path": path, "bytes": 3, "blake3": "ab"}), pattern: "write_file @worktree/src/*".into() }
    }

    fn with_kind(kind: ActionKind) -> Classified {
        Classified { kind, ..edit("@worktree/src/a.rs") }
    }

    #[test]
    fn only_an_amber_worktree_edit_is_eligible_and_everything_else_waits_for_a_person() {
        assert!(eligible(&edit("@worktree/src/a.rs"), Tier::Amber));
        assert!(!eligible(&edit("@worktree/src/a.rs"), Tier::Red), "a Red card is never decided provisionally");
        for kind in [ActionKind::Read, ActionKind::MissionArtifact, ActionKind::SpecDraft, ActionKind::PinnedDependencyInstall, ActionKind::UnsandboxedExec] {
            assert!(!eligible(&with_kind(kind), Tier::Amber), "{:?} must not be eligible", kind);
        }
    }

    #[test]
    fn the_rule_approver_allows_ordinary_edits_and_refuses_what_runs_on_the_host_or_rewrites_the_rules() {
        let a = RuleApprover { host_exec: &["tools/deploy".to_string()] };
        let v = a.decide(&edit("@worktree/src/new.rs"));
        assert!(v.allow && v.risk == Risk::Routine && v.rationale.contains("src/new.rs"));
        for (path, why) in [("@worktree/build.rs", "operator's machine"), ("@worktree/.github/workflows/ci.yml", "protected"), ("@worktree/tools/deploy/run.sh", "operator's machine"), ("@worktree/GOVERNANCE.md", "protected"), ("@worktree/agents/builder/AGENT.md", "protected"), ("@worktree/../x", "not a plain path"), ("@worktree/", "not a plain path")] {
            let v = a.decide(&edit(path));
            assert!(!v.allow && v.rationale.contains(why), "{}: {:?}", path, v);
        }
        let dep = a.decide(&edit("@worktree/Cargo.toml"));
        assert!(dep.allow && dep.risk == Risk::Elevated, "allowed but flagged: {:?}", dep);
    }

    fn ev(seq: u64, kind: &str, task: &str, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", kind, payload);
        e.seq = seq;
        e.task_id = Some(task.into());
        e.agent = Some("builder".into());
        e
    }

    #[test]
    fn the_ledger_folds_reviews_in_and_counts_only_allowed_decisions() {
        let v = Verdict { allow: true, rationale: "ok".into(), risk: Risk::Routine };
        let denied = Verdict { allow: false, rationale: "no".into(), risk: Risk::Elevated };
        let c = edit("@worktree/src/a.rs");
        let log = vec![
            ev(1, "decision.provisional", "t_1", decision_event("dec_a", &c, Tier::Amber, &v, "builder")),
            ev(2, "decision.provisional", "t_1", decision_event("dec_b", &c, Tier::Amber, &denied, "builder")),
            ev(3, "decision.provisional", "t_2", decision_event("dec_other", &c, Tier::Amber, &v, "builder")),
            ev(4, "decision.provisional", "t_1", decision_event("dec_c", &c, Tier::Amber, &v, "builder")),
            ev(5, "decision.reviewed", "t_1", json!({"decision_id": "dec_a", "resolution": "ratified", "note": "", "by": "console-1"})),
        ];
        let ds = ledger(&log, "t_1");
        assert_eq!(ds.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["dec_a", "dec_b", "dec_c"], "only this task's, in order");
        assert_eq!(ds[0].review.as_ref().unwrap().resolution, "ratified");
        let n = counts(&ds);
        assert_eq!((n.allowed, n.pending.clone(), n.rejected.clone()), (2, vec!["dec_c".to_string()], vec![]), "the denied one is recorded but needs no review");
        assert_eq!(ds[0].effects[0]["path"], "@worktree/src/a.rs");
        // The operator changes their mind: the latest review wins.
        let mut log = log;
        log.push(ev(6, "decision.reviewed", "t_1", json!({"decision_id": "dec_a", "resolution": "rejected", "note": "no", "by": "console-1"})));
        let n = counts(&ledger(&log, "t_1"));
        assert_eq!(n.rejected, vec!["dec_a".to_string()]);
    }

    #[test]
    fn a_decision_event_carries_what_the_daemon_classified_and_a_hash_the_review_must_match() {
        let c = edit("@worktree/src/a.rs");
        let v = Verdict { allow: true, rationale: "ok".into(), risk: Risk::Routine };
        let e = decision_event("dec_a", &c, Tier::Amber, &v, "builder");
        assert_eq!((e["outcome"].as_str(), e["decided_by"]["kind"].as_str(), e["effects"][0]["kind"].as_str()), (Some("allow"), Some("rule"), Some("worktree_edit")));
        assert_eq!(e["payload_blake3"], crate::approval::payload_blake3(&c.payload));
        assert!(new_decision_id().starts_with("dec_") && new_decision_id() != new_decision_id());
    }
}
