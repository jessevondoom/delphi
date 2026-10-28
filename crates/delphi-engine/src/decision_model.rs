//! The read model of the decisions pane (`specs/044` §4, §5): one list of everything the operator owes, built from
//! the daemon's own records and nothing else. The pane never reads the sources itself.
//!
//! * An approval card becomes `kind: action` (Red or Amber by the card's tier).
//! * A provisional decision (`specs/015`) that still awaits review becomes `kind: build`, Amber.
//!
//! Everything here is a pure function of the records, so it needs no model: the brief of a card is the plain text
//! of the facts the daemon classified (`state: plain`) and a provisional decision's recommendation is the daemon's
//! own ledger (`risk`, `second`). Whether a decision is **tickable** is decided here, in code, from the rule of
//! `specs/044` §5.4, and never from anything an agent says.

use crate::provisional::Decision;
use serde_json::{json, Value};

/// The longest title the pane shows (`specs/044` §4.1).
pub const TITLE_MAX: usize = 100;
const SUMMARY_MAX: usize = 400;
const CHANGE_MAX: usize = 120;
const CHANGES_MAX: usize = 5;

/// Plain text only: control characters become spaces, runs of space collapse, and the text is cut at `max`
/// characters with an ellipsis.
pub fn clean(s: &str, max: usize) -> String {
    let t: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > max {
        let mut o: String = t.chars().take(max.saturating_sub(1)).collect();
        o.push('…');
        o
    } else {
        t
    }
}

fn rank(tier: &str) -> u8 {
    match tier {
        "red" => 0,
        "amber" => 1,
        _ => 2,
    }
}

/// An approval card (the payload of `approval.requested`) as a decision.
pub fn from_card(card: &Value) -> Option<Value> {
    let source_id = card["approval_id"].as_str()?;
    let tier = if card["tier"].as_str() == Some("red") { "red" } else { "amber" };
    let role = card["role"].as_str().unwrap_or("");
    let tool = card["tool"].as_str().unwrap_or("");
    let summary = clean(card["summary"].as_str().unwrap_or(""), TITLE_MAX);
    let title = if summary.is_empty() { clean(tool, TITLE_MAX) } else { summary.clone() };
    let mut changes: Vec<String> = Vec::new();
    let p = &card["payload"];
    if let Some(path) = p["path"].as_str() {
        changes.push(clean(&format!("touches {}", path), CHANGE_MAX));
    }
    if let Some(n) = p["bytes"].as_u64() {
        changes.push(format!("{} bytes", n));
    }
    if let Some(cmd) = p["command"].as_str() {
        changes.push(clean(&format!("runs {}", cmd), CHANGE_MAX));
    }
    changes.truncate(CHANGES_MAX);
    let word = if tier == "red" { "Red" } else { "Amber" };
    let brief = clean(&format!("{} asks to use {}: {}. It is a {}-tier action, so it waits for you.", role, tool, title, word), SUMMARY_MAX);
    let mut choices = vec![json!({"id": "allow", "label": "Approve once", "effect": "approval.resolve", "reversible": false})];
    if tier != "red" {
        choices.push(json!({"id": "allow_similar", "label": "Allow similar actions this task", "effect": "approval.resolve", "reversible": false}));
    }
    choices.push(json!({"id": "deny", "label": "Reject", "effect": "approval.resolve", "reversible": false}));
    Some(json!({
        "id": format!("card:{}", source_id), "source": "card", "source_id": source_id, "label": source_id,
        "kind": "action", "tier": tier, "title": title,
        "requester": role, "task": card["task_id"].as_str().unwrap_or(""), "raised_at": Value::Null,
        "timeout_secs": card["timeout_secs"], "payload_blake3": card["payload_blake3"],
        "brief": {"summary": brief, "changes": changes, "recommendation": Value::Null, "state": "plain"},
        "choices": choices,
        "tickable": false,
        "tickable_reason": if tier == "red" { "a Red action is decided one at a time" } else { "an action card is not batch-accepted" },
    }))
}

/// A provisional decision that awaits the operator's review, as a decision. `None` for one that needs nothing from
/// the operator: it refused something, was already reviewed, or was reverted.
pub fn from_provisional(d: &Decision, raised_at: &str) -> Option<Value> {
    if d.outcome != "allow" || d.review.is_some() || d.reverted {
        return None;
    }
    let seconded = d.second.starts_with("concurred:");
    let routine = d.risk == "routine";
    let mut changes: Vec<String> = Vec::new();
    for e in &d.effects {
        if e["kind"].as_str() == Some("worktree_edit") {
            let path = e["path"].as_str().unwrap_or("");
            let bytes = e["bytes"].as_u64().map(|n| format!(" ({} bytes)", n)).unwrap_or_default();
            changes.push(clean(&format!("edits {}{}", path, bytes), CHANGE_MAX));
        }
    }
    changes.truncate(CHANGES_MAX);
    let recommendation = if routine && seconded {
        let by = d.second.trim_start_matches("concurred:");
        json!({"choice_id": "ratified", "why": clean(&format!("routine risk, second seat ({}) concurred", by), 200)})
    } else {
        Value::Null
    };
    let why = clean(&d.rationale, 200);
    let summary = clean(&format!("Provisional change from an optimistic build, task {}: {}. {}", d.task_id, clean(&d.summary, TITLE_MAX), why), SUMMARY_MAX);
    let tickable = !recommendation.is_null();
    Some(json!({
        "id": format!("provisional:{}", d.id), "source": "provisional", "source_id": d.id, "label": d.id,
        "kind": "build", "tier": "amber", "title": clean(&d.summary, TITLE_MAX),
        "requester": d.role, "task": d.task_id, "raised_at": raised_at, "timeout_secs": Value::Null, "payload_blake3": d.payload_blake3,
        "brief": {"summary": summary, "changes": changes, "recommendation": recommendation, "state": "authored"},
        "choices": [
            {"id": "ratified", "label": "Ratify", "effect": "decisions.review", "reversible": true},
            {"id": "rejected", "label": "Reject", "effect": "decisions.review", "reversible": true},
        ],
        "tickable": tickable,
        "tickable_reason": if tickable { "routine risk and a second seat concurred" } else if !routine { "the risk is not routine" } else { "no second seat concurred" },
    }))
}

/// Every owed decision, in the order of `specs/044` §5.1: Red first, then Amber, then the rest; within a tier the
/// order the sources give (cards by arrival, then provisional decisions by sequence).
pub fn list(cards: &[Value], ledger: &[(Decision, String)]) -> Vec<Value> {
    let mut out: Vec<Value> = cards.iter().filter_map(from_card).collect();
    out.extend(ledger.iter().filter_map(|(d, ts)| from_provisional(d, ts)));
    out.sort_by_key(|d| rank(d["tier"].as_str().unwrap_or("")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisional::Review;

    fn card(id: &str, tier: &str, summary: &str) -> Value {
        json!({"approval_id": id, "task_id": "t_1", "role": "builder", "tier": tier, "tool": "write_file", "summary": summary,
               "payload": {"path": "crates/x/src/a.rs", "bytes": 120}, "payload_blake3": format!("h-{}", id), "timeout_secs": 600})
    }

    fn prov(id: &str, risk: &str, second: &str) -> Decision {
        Decision {
            id: id.into(), seq: 7, task_id: "t_2".into(), role: "builder".into(), tool: "write_file".into(), summary: "raise the rate-limit burst".into(),
            payload_blake3: format!("p-{}", id), outcome: "allow".into(), rationale: "stays in scope".into(), risk: risk.into(), decided_by: "strategist".into(),
            second: second.into(), effects: vec![json!({"kind": "worktree_edit", "path": "crates/core/src/rate.rs", "bytes": 340})], review: None, reverted: false,
        }
    }

    #[test]
    fn a_red_card_has_no_allow_similar_and_is_never_tickable() {
        let d = from_card(&card("a1", "red", "ratify spec 005b")).unwrap();
        assert_eq!(d["kind"], "action");
        assert_eq!(d["tier"], "red");
        let ids: Vec<&str> = d["choices"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["allow", "deny"]);
        assert_eq!(d["tickable"], false);
        assert_eq!(d["brief"]["state"], "plain");
        assert!(d["brief"]["recommendation"].is_null(), "no rule supplies a recommendation for a card");
    }

    #[test]
    fn an_amber_card_may_be_allowed_for_similar_actions_but_is_not_tickable() {
        let d = from_card(&card("a2", "amber", "write deck.rs")).unwrap();
        let ids: Vec<&str> = d["choices"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["allow", "allow_similar", "deny"]);
        assert_eq!(d["tier"], "amber");
        assert_eq!(d["tickable"], false);
        assert_eq!(d["brief"]["changes"][0], "touches crates/x/src/a.rs");
    }

    #[test]
    fn a_routine_seconded_provisional_decision_is_recommended_and_tickable() {
        let d = from_provisional(&prov("d1", "routine", "concurred:red-team"), "2026-10-28T09:00:00Z").unwrap();
        assert_eq!(d["kind"], "build");
        assert_eq!(d["tier"], "amber");
        assert_eq!(d["tickable"], true);
        assert_eq!(d["brief"]["recommendation"]["choice_id"], "ratified");
        assert_eq!(d["brief"]["state"], "authored");
        assert_eq!(d["brief"]["changes"][0], "edits crates/core/src/rate.rs (340 bytes)");
        assert_eq!(d["raised_at"], "2026-10-28T09:00:00Z");
    }

    #[test]
    fn an_elevated_or_unseconded_provisional_decision_has_no_recommendation_and_no_tick() {
        for (risk, second, why) in [("elevated", "concurred:red-team", "the risk is not routine"), ("routine", "unseconded", "no second seat concurred"), ("routine", "", "no second seat concurred")] {
            let d = from_provisional(&prov("d2", risk, second), "t").unwrap();
            assert_eq!(d["tickable"], false, "{risk} {second}");
            assert!(d["brief"]["recommendation"].is_null(), "{risk} {second}");
            assert_eq!(d["tickable_reason"], why);
        }
    }

    #[test]
    fn only_decisions_that_await_the_operator_are_listed() {
        let mut denied = prov("d3", "routine", "concurred:red-team");
        denied.outcome = "deny".into();
        let mut reviewed = prov("d4", "routine", "concurred:red-team");
        reviewed.review = Some(Review { resolution: "ratified".into(), note: String::new(), by: "console-1".into() });
        let mut reverted = prov("d5", "routine", "concurred:red-team");
        reverted.reverted = true;
        for d in [&denied, &reviewed, &reverted] {
            assert!(from_provisional(d, "t").is_none(), "{}", d.id);
        }
    }

    #[test]
    fn the_list_puts_red_first_then_amber_keeping_each_sources_order() {
        let cards = [card("a1", "amber", "one"), card("r1", "red", "two"), card("a2", "amber", "three")];
        let ledger = [(prov("d1", "routine", "concurred:red-team"), "t".to_string())];
        let got: Vec<String> = list(&cards, &ledger).iter().map(|d| d["source_id"].as_str().unwrap().to_string()).collect();
        assert_eq!(got, ["r1", "a1", "a2", "d1"]);
    }

    #[test]
    fn titles_are_plain_text_and_cut_at_the_limit() {
        let evil = format!("write\x1b[31m x\u{7}{}", "y".repeat(300));
        let d = from_card(&card("a3", "amber", &evil)).unwrap();
        let title = d["title"].as_str().unwrap();
        assert!(!title.chars().any(|c| c.is_control()));
        assert_eq!(title.chars().count(), TITLE_MAX);
        assert!(title.ends_with('…'));
    }
}
