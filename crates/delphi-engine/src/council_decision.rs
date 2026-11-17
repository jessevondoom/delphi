//! An escalated host Council or Panel reaches the decisions pane (`specs/123` §8, as amended by §15 item 1). Pure functions of the
//! mission's events; the engine and the host service write what they return.
//!
//! * When a host Council or Panel escalates, one raised decision of kind `council` is admitted for it, as the `orchestrator`, through
//!   `decision_raised::admit` (so the ten-open cap holds). When the cap is full nothing is raised, and `council.decision_not_raised`
//!   says so, so the card can (the card is a pure function of the log).
//! * A choice the host relays (`council_choose`) resolves that decision only when Delphi orchestrated the Council (a daemon Council,
//!   or a Panel in which Delphi ran a seat on a family other than the host's, by the receipts). After a same-family Panel the choice
//!   becomes the decision's recommendation, and the decision stays open for the operator. No other raised decision can be resolved
//!   from a host: only the decision this module raised for this Council, marked `origin` by the daemon, is ever found.

use crate::decision_raised::{self, Rec};
use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};

/// The marker the daemon puts on the payload of the decision it raises; an agent's `raise_decision` payload is built by `admit`
/// from parsed fields and can never carry it.
pub const ORIGIN: &str = "council-escalation";
/// Written instead of a raise when the mission's open raised decisions are at the cap.
pub const NOT_RAISED: &str = "council.decision_not_raised";
/// Who raises it: the role that rules a Council.
pub const RAISER: &str = "orchestrator";
const HOLD: &str = "Hold: decide nothing yet";

fn host_event<'a>(events: &'a [EventEnvelope], task_id: &str) -> Option<&'a EventEnvelope> {
    events.iter().find(|e| e.kind == crate::host_rpc::COUNCIL_EVENT && e.payload["task_id"] == task_id)
}

fn verdict<'a>(events: &'a [EventEnvelope], task_id: &str) -> Option<&'a EventEnvelope> {
    events.iter().find(|e| e.kind == "council.verdict" && e.task_id.as_deref() == Some(task_id))
}

/// The labels the ruling offered, in order. The choice ids are their positions: `opt_1` to `opt_5`.
fn offered(v: &Value) -> Vec<String> {
    v["options"].as_array().into_iter().flatten().filter_map(|o| o["label"].as_str().map(String::from)).take(5).collect()
}

/// The `decision.raised` payload for an escalated host Council, or `None` when there is nothing to raise (not a host Council, not
/// escalated, or already raised). `Some(Err)` is the cap.
pub fn raise_on_verdict(events: &[EventEnvelope], task_id: &str) -> Option<Result<(String, Value), String>> {
    host_event(events, task_id)?;
    let v = &verdict(events, task_id)?.payload;
    let why = v["escalated"].as_str()?;
    if raised_for(events, task_id).is_some() {
        return None;
    }
    let mut labels = offered(v);
    // A decision has 2 to 5 choices. A ruling that offered fewer gets a fixed hold choice (and a fixed proceed one when it offered none).
    if labels.is_empty() {
        labels.push("Proceed as proposed".into());
    }
    if labels.len() < 2 {
        labels.push(HOLD.into());
    }
    let n = v["n"].as_u64().unwrap_or(0);
    let path = v["path"].as_str().unwrap_or("");
    let args = json!({
        "kind": "council", "title": format!("Council {n:03}: {why}"),
        "summary": format!("A host {} escalated to you: {why}. Choose one of the options it offered; the record is {path}.", if v["venue"] == "panel" { "Panel" } else { "Council" }),
        "choices": labels.iter().enumerate().map(|(i, l)| json!({"id": format!("opt_{}", i + 1), "label": l, "reversible": false})).collect::<Vec<_>>(),
        "recommendation": Value::Null, "related": task_id,
    });
    let raise = match decision_raised::parse(&args) {
        Ok(r) => r,
        Err(e) => return Some(Err(e)),
    };
    Some(decision_raised::admit(events, &raise).map(|(id, mut p)| {
        p["origin"] = json!(ORIGIN);
        (id, p)
    }))
}

/// After `council.verdict`: raise the decision for an escalated host Council, or record that the cap stopped it.
pub async fn raise_for_host(m: &std::sync::Arc<crate::mission::MissionState>, task_id: &str) -> Result<(), crate::mission::EngineError> {
    let events = m.replay()?;
    let mut e = match raise_on_verdict(&events, task_id) {
        None => return Ok(()),
        Some(Ok((_, p))) => {
            let mut e = EventEnvelope::new(&m.id, "decision.raised", p);
            e.agent = Some(RAISER.into());
            e
        }
        Some(Err(why)) => EventEnvelope::new(&m.id, NOT_RAISED, json!({"reason": "cap", "detail": why, "path": verdict(&events, task_id).map(|v| v.payload["path"].clone()).unwrap_or(Value::Null)})),
    };
    e.task_id = Some(task_id.to_string());
    m.append(e).await?;
    Ok(())
}

/// The decision the daemon raised for this Council, if any: kind `council`, `related` this task, first raised by the daemon.
pub fn raised_for(events: &[EventEnvelope], task_id: &str) -> Option<Rec> {
    let first = events.iter().find(|e| e.kind == "decision.raised" && e.payload["origin"] == ORIGIN && e.payload["related"] == task_id && e.payload["kind"] == "council" && e.agent.as_deref() == Some(RAISER))?;
    let id = first.payload["id"].as_str()?;
    // An agent that re-raised it under the same kind, title and `related` changed what it says: then it is no longer the daemon's.
    let last = events.iter().rfind(|e| e.kind == "decision.raised" && e.payload["id"] == id)?;
    if last.payload["origin"] != ORIGIN || last.agent.as_deref() != Some(RAISER) {
        return None;
    }
    decision_raised::fold(events).into_iter().find(|r| r.id == id && r.raise.kind == "council" && r.raise.related == task_id)
}

/// The label a decision shows, `RD-001` for `rd_001`.
pub fn label(id: &str) -> String {
    id.to_uppercase().replace('_', "-")
}

/// The sentence that ends an escalated host Council's card, from the log.
pub fn card_line(events: &[EventEnvelope], task_id: &str) -> Option<String> {
    if let Some(e) = events.iter().find(|e| e.kind == "decision.raised" && e.payload["origin"] == ORIGIN && e.payload["related"] == task_id) {
        return Some(format!("Raised for your decisions pane as {} in mission `{}`.", label(e.payload["id"].as_str().unwrap_or("")), e.mission_id));
    }
    events.iter().find(|e| e.kind == NOT_RAISED && e.task_id.as_deref() == Some(task_id)).map(|e| {
        format!("Not raised for your decisions pane: {} raised decisions are already open in mission `{}`; answer some there, then this Council's record is {}.", decision_raised::OPEN_MAX, e.mission_id, e.payload["path"].as_str().unwrap_or("in its mission"))
    })
}

/// Did Delphi orchestrate this Council (`specs/123` §15 item 1)? A daemon Council, or a Panel whose receipts show Delphi ran a seat on a
/// known family other than the host's. A Panel whose host family is not known is not: the foreign seat cannot be shown to differ.
pub fn delphi_orchestrated(events: &[EventEnvelope], task_id: &str) -> bool {
    let Some(h) = host_event(events, task_id) else { return false };
    if h.payload["venue"] != "panel" {
        return true;
    }
    let host = events.iter().find(|e| e.kind == "council.started" && e.task_id.as_deref() == Some(task_id)).and_then(|e| e.payload["host_family"].as_str());
    let (served, _) = crate::seat::served_for_task(events, task_id);
    host.is_some_and(|host| served.iter().any(|f| f != crate::seat::UNKNOWN && !f.is_empty() && f != host))
}

/// What a host's relayed choice does to the Council's raised decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// No raised decision for this Council (it was not raised, or the cap stopped it).
    None,
    /// The decision was already answered; nothing changes.
    Answered { id: String },
    /// Delphi-orchestrated: write this `decision.resolved` payload.
    Resolve { id: String, payload: Value },
    /// Same-family Panel: write this `decision.raised` payload (an update of the open decision, with the choice as its recommendation).
    Recommend { id: String, payload: Value },
}

/// The effect of the host reporting that the operator chose `option` (a label the ruling offered, already checked by the caller).
pub fn on_host_choice(events: &[EventEnvelope], task_id: &str, option: &str, session: &str) -> Effect {
    let Some(rec) = raised_for(events, task_id) else { return Effect::None };
    if rec.resolved.is_some() {
        return Effect::Answered { id: rec.id };
    }
    let Some(pos) = verdict(events, task_id).map(|v| offered(&v.payload)).and_then(|l| l.iter().position(|x| x == option)) else { return Effect::None };
    let choice = format!("opt_{}", pos + 1);
    let Some((_, label, _)) = rec.raise.choices.iter().find(|(i, _, _)| *i == choice).cloned() else { return Effect::None };
    if delphi_orchestrated(events, task_id) {
        return Effect::Resolve { id: rec.id.clone(), payload: json!({"id": rec.id, "choice_id": choice, "note": "", "conditions": [], "by": "host-relayed", "surface": "host", "session": session, "payload_blake3": rec.payload_blake3}) };
    }
    let mut raise = rec.raise.clone();
    raise.recommendation = Some((choice, format!("the host reported the operator chose {label} after a same-family Panel")));
    match decision_raised::admit(events, &raise) {
        Ok((id, mut p)) => {
            p["origin"] = json!(ORIGIN);
            Effect::Recommend { id, payload: p }
        }
        Err(_) => Effect::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, payload: Value, agent: Option<&str>) -> EventEnvelope {
        let mut e = EventEnvelope::new("host-abcdef0123456789", kind, payload);
        e.task_id = Some("t_0001".into());
        e.agent = agent.map(String::from);
        e
    }
    fn receipt(agent: &str, family: &str) -> EventEnvelope {
        ev(crate::seat::RECEIPT_KIND, json!({"served": {"family": family}, "tokens_in": 1, "tokens_out": 1, "outcome": "ok"}), Some(agent))
    }
    /// A host Council (`panel` = a Panel whose host is on `claude`) that escalated with two options.
    fn log(panel: bool, escalated: bool) -> Vec<EventEnvelope> {
        let mut host = json!({"task_id": "t_0001"});
        if panel {
            host["venue"] = json!("panel");
        }
        vec![
            EventEnvelope::new("host-abcdef0123456789", crate::host_rpc::COUNCIL_EVENT, host),
            ev("council.started", json!({"venue": if panel { "panel" } else { "daemon" }, "host_family": if panel { json!("claude") } else { Value::Null }}), None),
            ev("council.verdict", json!({"venue": if panel { "panel" } else { "daemon" }, "n": 3, "path": "missions/host-abcdef0123456789/councils/003-ship-it.md", "escalated": if escalated { json!("the Round 2 recommendations conflict") } else { Value::Null }, "options": [{"label": "PROCEED", "seats": ["architect"]}, {"label": "BLOCK", "seats": ["red-team"]}]}), None),
        ]
    }
    fn raise(mut events: Vec<EventEnvelope>) -> Vec<EventEnvelope> {
        let (_, p) = raise_on_verdict(&events, "t_0001").unwrap().unwrap();
        events.push(ev("decision.raised", p, Some(RAISER)));
        events
    }

    #[test]
    fn an_escalated_host_council_raises_one_council_decision_with_the_rulings_options_and_no_recommendation() {
        let events = raise(log(false, true));
        let rec = raised_for(&events, "t_0001").unwrap();
        assert_eq!((rec.id.as_str(), rec.raise.kind.as_str(), rec.requester.as_str(), rec.raise.related.as_str()), ("rd_001", "council", "orchestrator", "t_0001"));
        assert_eq!(rec.raise.title, "Council 003: the Round 2 recommendations conflict");
        assert!(rec.raise.summary.contains("003-ship-it.md"), "{}", rec.raise.summary);
        assert_eq!(rec.raise.choices, vec![("opt_1".into(), "PROCEED".into(), false), ("opt_2".into(), "BLOCK".into(), false)]);
        assert_eq!(rec.raise.recommendation, None);
        assert_eq!(decision_raised::to_decision(&rec)["tier"], "amber");
        assert!(raise_on_verdict(&events, "t_0001").is_none(), "raised once");
        assert_eq!(card_line(&events, "t_0001").unwrap(), "Raised for your decisions pane as RD-001 in mission `host-abcdef0123456789`.");
    }

    #[test]
    fn nothing_is_raised_for_a_council_that_did_not_escalate_or_that_no_host_asked_for() {
        assert!(raise_on_verdict(&log(false, false), "t_0001").is_none());
        assert!(raise_on_verdict(&log(false, true)[1..], "t_0001").is_none(), "a console Council keeps its own escalation flow");
        assert!(card_line(&log(false, false), "t_0001").is_none());
    }

    #[test]
    fn a_ruling_with_one_option_gets_a_hold_choice() {
        let mut events = log(false, true);
        events[2].payload["options"] = json!([{"label": "PROCEED", "seats": ["architect", "red-team"]}]);
        let rec = raised_for(&raise(events), "t_0001").unwrap();
        assert_eq!(rec.raise.choices.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), ["PROCEED", HOLD]);
    }

    #[test]
    fn when_ten_decisions_are_open_nothing_is_raised_and_the_card_says_so() {
        let mut events = log(false, true);
        for i in 0..decision_raised::OPEN_MAX {
            let r = decision_raised::parse(&json!({"kind": "question", "title": format!("q{i}"), "summary": "s", "choices": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]})).unwrap();
            let (_, p) = decision_raised::admit(&events, &r).unwrap();
            events.push(ev("decision.raised", p, Some("architect")));
        }
        assert!(raise_on_verdict(&events, "t_0001").unwrap().unwrap_err().contains("already waiting"));
        events.push(ev(NOT_RAISED, json!({"reason": "cap", "path": "missions/host-abcdef0123456789/councils/003-ship-it.md"}), None));
        let line = card_line(&events, "t_0001").unwrap();
        assert!(line.starts_with("Not raised for your decisions pane: 10 raised decisions are already open"), "{line}");
        assert_eq!(on_host_choice(&events, "t_0001", "BLOCK", "abcdef0123456789"), Effect::None);
    }

    #[test]
    fn a_daemon_councils_relayed_choice_resolves_it_as_host_relayed() {
        let events = raise(log(false, true));
        let Effect::Resolve { id, payload } = on_host_choice(&events, "t_0001", "BLOCK", "abcdef0123456789") else { panic!() };
        assert_eq!(id, "rd_001");
        assert_eq!((payload["choice_id"].as_str(), payload["by"].as_str(), payload["surface"].as_str(), payload["session"].as_str()), (Some("opt_2"), Some("host-relayed"), Some("host"), Some("abcdef0123456789")));
        assert_eq!(payload["payload_blake3"].as_str(), Some(raised_for(&events, "t_0001").unwrap().payload_blake3.as_str()));
        let mut done = events.clone();
        done.push(ev("decision.resolved", payload, None));
        assert!(decision_raised::open_entries(&done).is_empty());
        assert_eq!(raised_for(&done, "t_0001").unwrap().resolved.unwrap().by, "host-relayed");
        assert_eq!(on_host_choice(&done, "t_0001", "PROCEED", "abcdef0123456789"), Effect::Answered { id: "rd_001".into() });
    }

    #[test]
    fn a_panel_with_a_foreign_family_seat_is_delphi_orchestrated_and_a_same_family_panel_only_recommends() {
        let mut foreign = log(true, true);
        foreign.push(receipt("red-team", "gemini"));
        assert!(delphi_orchestrated(&foreign, "t_0001"));
        assert!(matches!(on_host_choice(&raise(foreign), "t_0001", "PROCEED", "abcdef0123456789"), Effect::Resolve { .. }));
        for same in [vec![], vec![receipt("red-team", "claude")], vec![receipt("red-team", crate::seat::UNKNOWN)]] {
            let mut events = log(true, true);
            events.extend(same);
            assert!(!delphi_orchestrated(&events, "t_0001"));
            let events = raise(events);
            let Effect::Recommend { id, payload } = on_host_choice(&events, "t_0001", "BLOCK", "abcdef0123456789") else { panic!() };
            assert_eq!((id.as_str(), payload["updated"].clone()), ("rd_001", json!(true)));
            let mut after = events.clone();
            after.push(ev("decision.raised", payload, Some(RAISER)));
            let open = decision_raised::open_entries(&after);
            assert_eq!(open.len(), 1, "it stays open for the operator");
            assert_eq!(open[0]["brief"]["recommendation"], json!({"choice_id": "opt_2", "why": "the host reported the operator chose BLOCK after a same-family Panel"}));
            assert_ne!(open[0]["payload_blake3"].as_str(), Some(raised_for(&events, "t_0001").unwrap().payload_blake3.as_str()), "what the operator is shown changed");
        }
        // A Panel whose host family is unknown cannot show its foreign seat differed.
        let mut unknown_host = log(true, true);
        unknown_host[1].payload["host_family"] = Value::Null;
        unknown_host.push(receipt("red-team", "gemini"));
        assert!(!delphi_orchestrated(&unknown_host, "t_0001"));
    }

    #[test]
    fn no_other_raised_decision_can_be_resolved_from_a_host() {
        // An agent's own decision about this Council (any kind, even `council`, even by the orchestrator) is not the daemon's.
        let mut events = log(false, true);
        for kind in ["council", "question", "spec"] {
            let r = decision_raised::parse(&json!({"kind": kind, "title": format!("about {kind}"), "summary": "s", "choices": [{"id": "opt_1", "label": "PROCEED"}, {"id": "opt_2", "label": "BLOCK"}], "related": "t_0001"})).unwrap();
            let (_, p) = decision_raised::admit(&events, &r).unwrap();
            assert!(p.get("origin").is_none(), "admit never sets the marker");
            events.push(ev("decision.raised", p, Some("orchestrator")));
        }
        assert!(raised_for(&events, "t_0001").is_none());
        assert_eq!(on_host_choice(&events, "t_0001", "BLOCK", "abcdef0123456789"), Effect::None);
        // With the daemon's decision raised too, only that one is resolved.
        let events = raise(events);
        let Effect::Resolve { id, .. } = on_host_choice(&events, "t_0001", "BLOCK", "abcdef0123456789") else { panic!() };
        assert_eq!(id, "rd_004");
        // Another Council's decision is not this one's.
        assert!(raised_for(&events, "t_0002").is_none());
    }
}
