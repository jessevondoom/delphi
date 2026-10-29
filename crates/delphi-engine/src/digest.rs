//! The daily digest (`specs/052` §3.3, slice KS3): the captured candidates of a mission, shown to the operator one at a
//! time as a `learn` decision in the decisions pane (`specs/044`), with four actions.
//!
//! * **keep**: the claim stays `noted` and searchable. The default; it changes nothing.
//! * **check**: records a request for the validator (`specs/040` §5). Nothing consumes it yet, so the label stays "not validated".
//! * **promote**: only a candidate that is not below `internal_empirical` (L4) can be promoted, and only into the existing
//!   ratify path: it becomes a proposal the operator still has to accept in `/retro`. Nothing is loaded by promoting.
//! * **dismiss**: the claim leaves the `staged` zone (`learn.dropped`); the log keeps the event, so nothing is deleted.
//!
//! The digest never resolves a card, satisfies a gate or blocks work: it is a list the operator may leave alone, at most
//! [`DAILY`] items a day, and it asks for nothing. Pure functions of the mission's events; no model is called.

use crate::decision_model::{clean, TITLE_MAX};
use crate::learn::{is_contact, live_notes, LiveNote, MAX_PROPOSAL_TEXT};
use delphi_core::event::EventEnvelope;
use delphi_core::provenance::Tier;
use serde_json::{json, Value};

/// Items shown per day, however many candidates wait.
pub const DAILY: usize = 10;

/// The actions, in the order the pane lists them.
pub const ACTIONS: [&str; 4] = ["keep", "check", "promote", "dismiss"];

fn day_of(ts: &str) -> &str {
    ts.get(..10).unwrap_or(ts)
}

fn tier_of(n: &LiveNote) -> Tier {
    Tier::parse(&n.tier).unwrap_or(Tier::L0)
}

fn promotable(n: &LiveNote) -> bool {
    tier_of(n) >= Tier::L4
}

fn source_words(trigger: &str) -> &'static str {
    match trigger {
        "council_verdict" => "the Council's verdict",
        "council_minority" => "the Council's Minority Report",
        "council_claim" => "a Council seat's claim",
        "decision_resolved" => "a decision you resolved",
        "accepted_answer" => "an answer you kept",
        "research_run" => "a research run",
        _ => "an event",
    }
}

/// The candidate to show now, and how many more wait today: the oldest live captured note nobody has acted on, unless
/// today's quota is used. `now` is an RFC 3339 timestamp.
pub fn next(mission: &str, events: &[EventEnvelope], now: &str) -> Option<(LiveNote, usize)> {
    let acted: Vec<&str> = events.iter().filter(|e| e.kind == "learn.digested").filter_map(|e| e.payload["id"].as_str()).collect();
    let today = events.iter().filter(|e| e.kind == "learn.digested" && day_of(&e.timestamp) == day_of(now)).count();
    if today >= DAILY {
        return None;
    }
    let mut pending: Vec<LiveNote> = live_notes(mission, events).into_iter().filter(|n| is_contact(&n.trigger) && !acted.contains(&n.id.as_str())).collect();
    if pending.is_empty() {
        return None;
    }
    let left = pending.len().min(DAILY - today) - 1;
    Some((pending.remove(0), left))
}

/// The card's id: one per day, whichever candidate it shows.
pub fn card_id(now: &str) -> String {
    format!("digest:{}", day_of(now))
}

/// What the operator is asked to judge, hashed: it changes with the candidate, so an answer to a stale card is refused.
pub fn payload_hash(n: &LiveNote) -> String {
    blake3::hash(format!("{}\n{}\n{}", n.id, n.trigger, n.summary).as_bytes()).to_hex().to_string()
}

/// The candidate as an entry of `decision.list`: kind `learn`, tier `open`, tickable for `keep` only (it changes nothing).
pub fn to_decision(n: &LiveNote, left: usize, now: &str) -> Value {
    let mut choices = vec![json!({"id": "keep", "label": "Keep it noted", "effect": "digest", "reversible": true}), json!({"id": "check", "label": "Ask the validator to check it", "effect": "digest", "reversible": true})];
    if promotable(n) {
        choices.push(json!({"id": "promote", "label": "Promote: propose it in /retro", "effect": "digest", "reversible": false}));
    }
    choices.push(json!({"id": "dismiss", "label": "Dismiss", "effect": "digest", "reversible": true}));
    let more = if left == 0 { String::new() } else { format!(" {} more wait.", left) };
    let mut changes = vec![format!("from {}", source_words(&n.trigger)), "not validated: nothing has checked it".to_string()];
    if !promotable(n) {
        changes.push("cannot be promoted: it did not come from you or a checked source".to_string());
    }
    json!({
        "id": card_id(now), "source": "digest", "source_id": n.id, "label": "DIGEST",
        "kind": "learn", "tier": "open", "title": clean(&n.summary, TITLE_MAX),
        "requester": "daemon", "task": n.task_id.clone().unwrap_or_default(), "raised_at": n.timestamp, "timeout_secs": Value::Null, "payload_blake3": payload_hash(n),
        "brief": {
            "summary": clean(&format!("Captured from {}, not validated. Keep it, ask for a check, promote it for review, or dismiss it. Nothing waits on this.{}", source_words(&n.trigger), more), 400),
            "changes": changes, "state": "plain",
            "recommendation": {"choice_id": "keep", "why": "it stays searchable and not validated; nothing else changes"},
        },
        // A list of typed items, so later slices can add sources, contested pairs and acquisition proposals (`specs/055`)
        // beside the claims without changing the entry's shape. Today it holds the one claim on show.
        "items": [{"type": "claim", "id": n.id, "text": clean(&n.summary, 400), "from": n.trigger, "tier": n.tier, "status": "noted", "label": "not validated"}],
        "choices": choices,
        "tickable": true, "tickable_reason": "keeping changes nothing",
    })
}

/// The events one action writes: the `learn.digested` mark that takes the candidate off the list, and what the action does.
/// `Err` for an unknown action or a promotion of a candidate that is below L4.
pub fn act(mission_id: &str, n: &LiveNote, action: &str) -> Result<Vec<EventEnvelope>, String> {
    let mut out = Vec::new();
    match action {
        "keep" | "check" => {}
        "dismiss" => out.push(EventEnvelope::new(mission_id, "learn.dropped", json!({"id": n.id, "reason": "dismissed"}))),
        "promote" => {
            if !promotable(n) {
                return Err("this candidate is below the promotion floor (L4); it can be kept or checked, not promoted".into());
            }
            let text: String = n.summary.chars().take(MAX_PROPOSAL_TEXT).collect();
            // A proposal, never a rule: the operator still accepts it in `/retro`, and nothing loads before that.
            out.push(EventEnvelope::new(mission_id, "learn.proposed", json!({"op": "add", "scope": "workspace", "text": text, "rationale": format!("promoted from the digest: {}", n.reference()), "target": "", "tier": tier_of(n).min(Tier::L3).as_str()})));
        }
        _ => return Err(format!("`{}` is not a digest action ({})", action, ACTIONS.join(", "))),
    }
    out.push(EventEnvelope::new(mission_id, "learn.digested", json!({"id": n.id, "action": action})));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged(seq: u64, id: &str, trigger: &str, tier: &str, ts: &str) -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", "learn.staged", json!({"id": id, "trigger": trigger, "summary": format!("claim {}", id), "event_seqs": [seq], "tier": tier}));
        e.seq = seq;
        e.timestamp = ts.into();
        e
    }

    fn digested(seq: u64, id: &str, ts: &str) -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", "learn.digested", json!({"id": id, "action": "keep"}));
        e.seq = seq;
        e.timestamp = ts.into();
        e
    }

    const NOW: &str = "2026-10-29T09:00:00.000Z";

    #[test]
    fn the_digest_shows_one_captured_candidate_at_a_time_and_only_captured_ones() {
        let evs = vec![staged(1, "n1", "denied_card", "L5", NOW), staged(2, "n2", "council_verdict", "L3", NOW), staged(3, "n3", "decision_resolved", "L5", NOW)];
        let (n, left) = next("m1", &evs, NOW).unwrap();
        assert_eq!((n.id.as_str(), left), ("n2", 1), "a denied card is not a capture-on-contact candidate");
        let mut more = evs.clone();
        more.push(digested(4, "n2", NOW));
        assert_eq!(next("m1", &more, NOW).unwrap().0.id, "n3", "an acted-on candidate is not shown again");
    }

    #[test]
    fn at_most_ten_a_day_and_a_dismissed_or_empty_digest_shows_nothing() {
        let mut evs: Vec<_> = (1..=12).map(|i| staged(i, &format!("n{}", i), "council_verdict", "L3", NOW)).collect();
        assert!(next("m1", &evs, NOW).is_some());
        for i in 1..=DAILY as u64 {
            evs.push(digested(100 + i, &format!("n{}", i), NOW));
        }
        assert!(next("m1", &evs, NOW).is_none(), "today's quota is used");
        assert!(next("m1", &evs, "2026-10-30T09:00:00.000Z").is_some(), "tomorrow another batch is offered");
        assert!(next("m1", &[], NOW).is_none());
    }

    #[test]
    fn the_card_is_a_learn_decision_that_is_not_validated_recommends_keep_and_offers_promote_only_at_l4_or_above() {
        let low = to_decision(&LiveNote { mission: "m1".into(), id: "n2".into(), trigger: "council_verdict".into(), summary: "build the table".into(), tier: "L3".into(), task_id: None, role: None, timestamp: NOW.into(), event_seqs: vec![2] }, 0, NOW);
        let ids = |d: &Value| d["choices"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!((low["kind"].as_str(), low["tier"].as_str(), low["source"].as_str(), low["id"].as_str()), (Some("learn"), Some("open"), Some("digest"), Some("digest:2026-10-29")));
        assert_eq!(ids(&low), ["keep", "check", "dismiss"]);
        assert!(low["brief"]["summary"].as_str().unwrap().contains("not validated") && !low.to_string().contains("untrusted"));
        assert_eq!(low["brief"]["recommendation"]["choice_id"], "keep");
        assert_eq!(low["items"][0]["type"], "claim", "the card carries typed items");
        assert_eq!(low["items"][0]["label"], "not validated");
        let high = to_decision(&LiveNote { mission: "m1".into(), id: "n3".into(), trigger: "decision_resolved".into(), summary: "ship it".into(), tier: "L5".into(), task_id: None, role: None, timestamp: NOW.into(), event_seqs: vec![3] }, 2, NOW);
        assert_eq!(ids(&high), ["keep", "check", "promote", "dismiss"]);
        assert_ne!(payload_hash(&LiveNote { mission: "m1".into(), id: "n3".into(), trigger: "x".into(), summary: "a".into(), tier: "L5".into(), task_id: None, role: None, timestamp: NOW.into(), event_seqs: vec![] }), high["payload_blake3"].as_str().unwrap());
    }

    #[test]
    fn promote_only_proposes_for_the_retro_review_and_never_a_card_or_a_rule_and_a_low_tier_cannot_promote() {
        let mut n = LiveNote { mission: "m1".into(), id: "n3".into(), trigger: "decision_resolved".into(), summary: "ship it".into(), tier: "L5".into(), task_id: None, role: None, timestamp: NOW.into(), event_seqs: vec![3] };
        let evs = act("m1", &n, "promote").unwrap();
        let kinds: Vec<&str> = evs.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["learn.proposed", "learn.digested"], "a proposal waits for /retro; no rule is written here");
        assert!(evs.iter().all(|e| !e.kind.starts_with("approval.") && !e.kind.starts_with("decision.") && !e.kind.contains("gate")), "the digest never approves");
        assert_eq!(evs[0].payload["tier"], "L3");
        n.tier = "L3".into();
        assert!(act("m1", &n, "promote").is_err());
        let dismissed = act("m1", &n, "dismiss").unwrap();
        assert_eq!(dismissed[0].kind, "learn.dropped");
        assert_eq!(act("m1", &n, "check").unwrap().len(), 1);
        assert!(act("m1", &n, "approve").is_err());
    }
}
