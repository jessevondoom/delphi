//! Raised decisions (`specs/044` §4.4): the decisions an agent puts in front of the operator with `raise_decision`, and what
//! happens to them. Pure functions of the mission's events; nothing here calls a model.
//!
//! * `decision.raised` records one (or **updates** the open one with the same kind, title and `related`).
//! * `decision.resolved` records the operator's answer: the choice, a note, conditions, the time. That is the whole effect
//!   of every raised kind (`record`): no choice can carry another.
//! * `decision.delivered` marks an answer as told to its requester, so it is told once.
//!
//! What the agent wrote is untrusted content for display: plain text, escapes removed, capped. It is the brief; no model is
//! asked to write one. The raiser cannot resolve what it raised: only `decision.resolve` (console) writes `decision.resolved`.

use crate::decision_model::{clean, TITLE_MAX};
use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};

pub const KINDS: [&str; 5] = ["spec", "question", "council", "release", "budget"];
/// Roles that may raise one: not a builder, a scout or a bundle agent, so a span that read untrusted content cannot.
pub const RAISERS: [&str; 3] = ["orchestrator", "architect", "strategist"];
/// Open raised decisions allowed in one mission.
pub const OPEN_MAX: usize = 10;
const SUMMARY_MAX: usize = 400;
const CHANGE_MAX: usize = 120;
const CHANGES_MAX: usize = 5;
const LABEL_MAX: usize = 80;
const WHY_MAX: usize = 200;
const RELATED_MAX: usize = 120;
const NOTE_MAX: usize = 400;
const CONDITION_MAX: usize = 120;
const CONDITIONS_MAX: usize = 3;
/// How many answers one slice tells, and how long it is.
const TELL_MAX: usize = 6;

/// A validated, cleaned call of `raise_decision`.
#[derive(Debug, Clone, PartialEq)]
pub struct Raise {
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub changes: Vec<String>,
    pub choices: Vec<(String, String, bool)>,
    pub recommendation: Option<(String, String)>,
    pub related: String,
}

fn text(args: &Value, k: &str, max: usize) -> String {
    clean(args.get(k).and_then(Value::as_str).unwrap_or(""), max)
}

/// The arguments of a `raise_decision` call, checked against the table of §4.4. Text is cleaned, not rejected, when it is too long;
/// a missing or malformed field is an error the agent can read and fix.
pub fn parse(args: &Value) -> Result<Raise, String> {
    let kind = text(args, "kind", 20).to_lowercase();
    if !KINDS.contains(&kind.as_str()) {
        return Err(format!("`kind` must be one of {}", KINDS.join(", ")));
    }
    let title = text(args, "title", TITLE_MAX);
    let summary = text(args, "summary", SUMMARY_MAX);
    if title.is_empty() || summary.is_empty() {
        return Err("`title` and `summary` are required: say what the decision is and what it adds or changes".into());
    }
    let changes: Vec<String> = args.get("changes").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(|c| clean(c, CHANGE_MAX)).filter(|c| !c.is_empty()).take(CHANGES_MAX).collect();
    let raw = args.get("choices").and_then(Value::as_array).ok_or("`choices` must be a list of 2 to 5 of {id, label, reversible}")?;
    if !(2..=5).contains(&raw.len()) {
        return Err("`choices` must have 2 to 5 entries".into());
    }
    let mut choices: Vec<(String, String, bool)> = Vec::new();
    for c in raw {
        let id = clean(c.get("id").and_then(Value::as_str).unwrap_or(""), 40).to_lowercase().replace(' ', "_");
        let label = clean(c.get("label").and_then(Value::as_str).unwrap_or(""), LABEL_MAX);
        if id.is_empty() || label.is_empty() || !id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
            return Err("each choice needs an `id` (letters, digits, - and _) and a `label`".into());
        }
        if choices.iter().any(|(i, _, _)| *i == id) {
            return Err(format!("choice id `{}` is used twice", id));
        }
        choices.push((id, label, c.get("reversible").and_then(Value::as_bool).unwrap_or(false)));
    }
    let recommendation = match args.get("recommendation").filter(|r| !r.is_null()) {
        None => None,
        Some(r) => {
            let id = clean(r.get("choice_id").and_then(Value::as_str).unwrap_or(""), 40).to_lowercase();
            if !choices.iter().any(|(i, _, _)| *i == id) {
                return Err("`recommendation.choice_id` must name one of the choices".into());
            }
            Some((id, clean(r.get("why").and_then(Value::as_str).unwrap_or(""), WHY_MAX)))
        }
    };
    Ok(Raise { kind, title, summary, changes, choices, recommendation, related: text(args, "related", RELATED_MAX) })
}

/// The identity of a raised decision: a second call with the same kind, title and `related` updates it.
pub fn key(r: &Raise) -> String {
    blake3::hash(format!("{}\n{}\n{}", r.kind, r.title.to_lowercase(), r.related.to_lowercase()).as_bytes()).to_hex()[..16].to_string()
}

/// What the operator is asked to judge, hashed: it changes when the raiser changes anything shown.
pub fn payload_hash(r: &Raise) -> String {
    blake3::hash(serde_json::to_vec(&json!([r.kind, r.title, r.summary, r.changes, r.choices, r.recommendation, r.related])).unwrap_or_default().as_slice()).to_hex().to_string()
}

/// Tier by kind (`specs/044` §4.3).
pub fn tier_of(kind: &str) -> &'static str {
    match kind {
        "spec" | "release" => "red",
        "council" | "budget" => "amber",
        _ => "open",
    }
}

/// One raised decision as the log holds it now.
#[derive(Debug, Clone, PartialEq)]
pub struct Rec {
    pub id: String,
    pub key: String,
    pub raise: Raise,
    pub payload_blake3: String,
    pub requester: String,
    pub task: String,
    pub at: String,
    pub resolved: Option<Resolved>,
    /// The roles that have been told the answer.
    pub delivered: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub choice_id: String,
    pub note: String,
    pub conditions: Vec<String>,
    pub at: String,
    pub by: String,
}

fn raise_of(p: &Value) -> Option<Raise> {
    let mut r = parse(p).ok()?;
    // The log holds what `parse` accepted; read it back through the same rules, with the recommendation in its stored shape.
    r.recommendation = p["recommendation"]["choice_id"].as_str().filter(|id| r.choices.iter().any(|(i, _, _)| i == id)).map(|id| (id.to_string(), clean(p["recommendation"]["why"].as_str().unwrap_or(""), WHY_MAX)));
    Some(r)
}

/// Every raised decision in the log, oldest first. A later `decision.raised` with the same key updates the one that is still open
/// (keeping its id and raise time); one that was already resolved is not reopened by it (a new call then makes a new decision).
pub fn fold(events: &[EventEnvelope]) -> Vec<Rec> {
    let mut out: Vec<Rec> = Vec::new();
    for e in events {
        match e.kind.as_str() {
            "decision.raised" => {
                let (Some(id), Some(raise)) = (e.payload["id"].as_str(), raise_of(&e.payload)) else { continue };
                let k = key(&raise);
                let hash = payload_hash(&raise);
                let requester = e.agent.clone().unwrap_or_default();
                let task = e.task_id.clone().unwrap_or_default();
                if let Some(open) = out.iter_mut().find(|r| r.key == k && r.resolved.is_none()) {
                    open.raise = raise;
                    open.payload_blake3 = hash;
                } else {
                    out.push(Rec { id: id.to_string(), key: k, raise, payload_blake3: hash, requester, task, at: e.timestamp.clone(), resolved: None, delivered: Vec::new() });
                }
            }
            "decision.resolved" => {
                let (Some(id), Some(choice)) = (e.payload["id"].as_str(), e.payload["choice_id"].as_str()) else { continue };
                if let Some(r) = out.iter_mut().find(|r| r.id == id && r.resolved.is_none()) {
                    r.resolved = Some(Resolved {
                        choice_id: choice.to_string(),
                        note: clean(e.payload["note"].as_str().unwrap_or(""), NOTE_MAX),
                        conditions: e.payload["conditions"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|c| clean(c, CONDITION_MAX)).filter(|c| !c.is_empty()).take(CONDITIONS_MAX).collect(),
                        at: e.timestamp.clone(),
                        by: clean(e.payload["by"].as_str().unwrap_or(""), 40),
                    });
                }
            }
            "decision.delivered" => {
                if let (Some(id), Some(to)) = (e.payload["id"].as_str(), e.payload["to"].as_str()) {
                    if let Some(r) = out.iter_mut().find(|r| r.id == id) {
                        r.delivered.push(to.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The record a `raise_decision` call becomes: its id (a new one, or the open decision's own when this updates it) and the payload of
/// `decision.raised`. `Err` is the cap: ten open raised decisions in a mission.
pub fn admit(events: &[EventEnvelope], raise: &Raise) -> Result<(String, Value), String> {
    let recs = fold(events);
    let k = key(raise);
    let existing = recs.iter().find(|r| r.key == k && r.resolved.is_none()).map(|r| r.id.clone());
    if existing.is_none() && recs.iter().filter(|r| r.resolved.is_none()).count() >= OPEN_MAX {
        return Err(format!("{} raised decisions are already waiting for the operator; wait for some to be answered before raising more", OPEN_MAX));
    }
    let id = existing.clone().unwrap_or_else(|| format!("rd_{:03}", recs.len() + 1));
    let payload = json!({
        "id": id, "kind": raise.kind, "title": raise.title, "summary": raise.summary, "changes": raise.changes,
        "choices": raise.choices.iter().map(|(i, l, r)| json!({"id": i, "label": l, "reversible": r})).collect::<Vec<_>>(),
        "recommendation": raise.recommendation.as_ref().map(|(c, w)| json!({"choice_id": c, "why": w})).unwrap_or(Value::Null),
        "related": raise.related, "updated": existing.is_some(),
    });
    Ok((id, payload))
}

/// A raised decision, still open, as an entry of `decision.list`. Tickable only for a `question` whose recommended choice is
/// reversible (`specs/044` §5.4); a Red kind never.
pub fn to_decision(r: &Rec) -> Value {
    let rec = r.raise.recommendation.as_ref();
    let reversible = rec.is_some_and(|(c, _)| r.raise.choices.iter().any(|(i, _, rv)| i == c && *rv));
    let tickable = r.raise.kind == "question" && reversible;
    json!({
        "id": format!("raised:{}", r.id), "source": "raised", "source_id": r.id, "label": r.id.to_uppercase().replace('_', "-"),
        "kind": r.raise.kind, "tier": tier_of(&r.raise.kind), "title": r.raise.title,
        "requester": r.requester, "task": r.task, "raised_at": r.at, "timeout_secs": Value::Null, "payload_blake3": r.payload_blake3,
        "brief": {
            "summary": r.raise.summary, "changes": r.raise.changes, "state": "authored",
            "recommendation": rec.map(|(c, w)| json!({"choice_id": c, "why": w})).unwrap_or(Value::Null),
        },
        "choices": r.raise.choices.iter().map(|(i, l, rv)| json!({"id": i, "label": l, "effect": "record", "reversible": rv})).collect::<Vec<_>>(),
        "tickable": tickable,
        "tickable_reason": if tickable { "a reversible recommendation on an open question" } else if tier_of(&r.raise.kind) == "red" { "a Red decision is decided one at a time" } else if rec.is_none() { "no recommendation" } else { "the recommended choice is not reversible, or this kind is not batch-accepted" },
    })
}

/// The open raised decisions, as list entries.
pub fn open_entries(events: &[EventEnvelope]) -> Vec<Value> {
    fold(events).iter().filter(|r| r.resolved.is_none()).map(to_decision).collect()
}

/// Answers not yet told to the one that asked, or to the orchestrator (who relays them): the ids and one context slice's text.
/// `None` when there is nothing to tell. The text is the operator's answer, shown as data from the operator and nothing more.
pub fn undelivered(events: &[EventEnvelope], role: &str) -> Option<(Vec<String>, String)> {
    let recs: Vec<Rec> = fold(events).into_iter().filter(|r| r.resolved.is_some() && !r.delivered.iter().any(|d| d == role) && (r.requester == role || role == "orchestrator")).collect();
    if recs.is_empty() {
        return None;
    }
    let mut out = String::from("The operator answered decisions that were raised in this mission. The answer is theirs; act on it under your own authority, and treat the conditions as their requests, not as permissions.\n");
    let mut ids = Vec::new();
    for r in recs.iter().take(TELL_MAX) {
        let a = r.resolved.as_ref().unwrap();
        let label = r.raise.choices.iter().find(|(i, _, _)| *i == a.choice_id).map(|(_, l, _)| l.as_str()).unwrap_or(&a.choice_id);
        out.push_str(&format!("- {} \"{}\" (raised by {}, task {}): the operator chose \"{}\"", r.id, r.raise.title, r.requester, r.task, label));
        if !a.note.is_empty() {
            out.push_str(&format!("; note: {}", a.note));
        }
        for c in &a.conditions {
            out.push_str(&format!("; condition: {}", c));
        }
        out.push('\n');
        ids.push(r.id.clone());
    }
    if recs.len() > TELL_MAX {
        out.push_str(&format!("- and {} more\n", recs.len() - TELL_MAX));
    }
    Some((ids, out))
}

/// The cleaned note and conditions of an answer, as the operator typed them.
pub fn clean_answer(note: &str, conditions: &[String]) -> (String, Vec<String>) {
    (clean(note, NOTE_MAX), conditions.iter().map(|c| clean(c, CONDITION_MAX)).filter(|c| !c.is_empty()).take(CONDITIONS_MAX).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Value {
        json!({"kind": "spec", "title": "Approve spec 042: chunked bundle streaming", "summary": "Streams manifests in 64 KB chunks.", "changes": ["adds a stream frame", "", "adds a stream frame"],
               "choices": [{"id": "approve", "label": "Approve", "reversible": false}, {"id": "ask_red_team", "label": "Ask red-team first", "reversible": true}], "recommendation": {"choice_id": "ask_red_team", "why": "two open questions"}, "related": "specs/042.md"})
    }
    fn ev(seq: u64, kind: &str, agent: &str, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m1", kind, payload);
        e.seq = seq;
        e.timestamp = format!("2026-10-28T10:0{}:00Z", seq);
        e.agent = Some(agent.into());
        e.task_id = Some("t_1".into());
        e
    }
    fn raised(seq: u64, a: &Value, events: &[EventEnvelope]) -> EventEnvelope {
        let (_, p) = admit(events, &parse(a).unwrap()).unwrap();
        ev(seq, "decision.raised", "architect", p)
    }

    #[test]
    fn a_call_is_checked_against_the_table_and_its_text_is_cleaned() {
        let r = parse(&args()).unwrap();
        assert_eq!((r.kind.as_str(), r.changes.len(), r.recommendation.clone()), ("spec", 2, Some(("ask_red_team".into(), "two open questions".into()))));
        let mut a = args();
        a["title"] = json!(format!("\u{1b}[31m{}", "x".repeat(300)));
        let t = parse(&a).unwrap().title;
        assert!(t.chars().count() <= TITLE_MAX && !t.contains('\u{1b}'));
        for (field, bad) in [("kind", json!("action")), ("title", json!("")), ("choices", json!([{"id": "a", "label": "A"}])), ("choices", json!([{"id": "a", "label": "A"}, {"id": "a", "label": "B"}])),
            ("choices", json!([{"id": "a b!", "label": "A"}, {"id": "b", "label": "B"}])), ("recommendation", json!({"choice_id": "everything", "why": "x"}))] {
            let mut a = args();
            a[field] = bad.clone();
            assert!(parse(&a).is_err(), "{field}: {bad}");
        }
        let mut six = args();
        six["choices"] = json!((0..6).map(|i| json!({"id": format!("c{i}"), "label": "x"})).collect::<Vec<_>>());
        assert!(parse(&six).is_err(), "at most five choices");
        let mut none = args();
        none["recommendation"] = Value::Null;
        assert_eq!(parse(&none).unwrap().recommendation, None);
    }

    #[test]
    fn the_same_kind_title_and_related_update_the_open_decision_and_change_its_payload_hash() {
        let first = raised(1, &args(), &[]);
        let mut a2 = args();
        a2["summary"] = json!("Now also streams the lockfile.");
        let log = vec![first.clone()];
        let (id2, p2) = admit(&log, &parse(&a2).unwrap()).unwrap();
        assert_eq!(id2, "rd_001", "same id");
        assert_eq!(p2["updated"], true);
        let log = vec![first, ev(2, "decision.raised", "architect", p2)];
        let recs = fold(&log);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].raise.summary, "Now also streams the lockfile.");
        assert_ne!(recs[0].payload_blake3, payload_hash(&parse(&args()).unwrap()), "an answer can only be given to what was shown");
        let mut other = args();
        other["related"] = json!("specs/043.md");
        assert_eq!(admit(&log, &parse(&other).unwrap()).unwrap().0, "rd_002", "another related path is another decision");
    }

    #[test]
    fn at_most_ten_are_open_and_a_resolved_one_makes_room() {
        let mut log: Vec<EventEnvelope> = Vec::new();
        for i in 0..OPEN_MAX {
            let mut a = args();
            a["title"] = json!(format!("decision {i}"));
            log.push(raised(i as u64 + 1, &a, &log));
        }
        let mut more = args();
        more["title"] = json!("one too many");
        assert!(admit(&log, &parse(&more).unwrap()).unwrap_err().contains("already waiting"));
        let mut again = args();
        again["title"] = json!("decision 3");
        assert!(admit(&log, &parse(&again).unwrap()).is_ok(), "updating an open one needs no room");
        log.push(ev(20, "decision.resolved", "console", json!({"id": "rd_001", "choice_id": "approve"})));
        assert!(admit(&log, &parse(&more).unwrap()).is_ok());
        let mut reopened = args();
        reopened["title"] = json!("decision 0");
        assert_eq!(admit(&log, &parse(&reopened).unwrap()).unwrap().0, "rd_011", "a resolved decision is not reopened: the same call makes a new one");
    }

    #[test]
    fn a_raised_decision_is_listed_with_its_tier_choices_and_recommendation_and_leaves_when_resolved() {
        let log = vec![raised(1, &args(), &[])];
        let list = open_entries(&log);
        assert_eq!(list.len(), 1);
        let d = &list[0];
        assert_eq!((d["id"].as_str(), d["source"].as_str(), d["kind"].as_str(), d["tier"].as_str(), d["label"].as_str()), (Some("raised:rd_001"), Some("raised"), Some("spec"), Some("red"), Some("RD-001")));
        assert_eq!((d["tickable"].clone(), d["choices"][1]["effect"].as_str(), d["brief"]["state"].as_str(), d["brief"]["recommendation"]["choice_id"].as_str()), (json!(false), Some("record"), Some("authored"), Some("ask_red_team")));
        let mut q = args();
        q["kind"] = json!("question");
        q["title"] = json!("which search API?");
        let d = to_decision(&fold(&[raised(1, &q, &[])])[0]);
        assert_eq!((d["tier"].as_str(), d["tickable"].clone()), (Some("open"), json!(true)), "a question with a reversible recommendation can take the default");
        q["recommendation"]["choice_id"] = json!("approve");
        assert_eq!(to_decision(&fold(&[raised(1, &q, &[])])[0])["tickable"], false, "an irreversible recommended choice cannot");
        let mut c = args();
        c["kind"] = json!("council");
        assert_eq!(to_decision(&fold(&[raised(1, &c, &[])])[0])["tier"], "amber");
        let done = vec![log[0].clone(), ev(2, "decision.resolved", "console", json!({"id": "rd_001", "choice_id": "ask_red_team"}))];
        assert!(open_entries(&done).is_empty());
    }

    #[test]
    fn an_answer_is_told_once_to_the_requester_and_the_orchestrator_and_the_conditions_are_requests() {
        let log = vec![raised(1, &args(), &[]), ev(2, "decision.resolved", "console", json!({"id": "rd_001", "choice_id": "ask_red_team", "note": "do it\u{7} soon", "conditions": ["keep t_016 paused", "", "tell the architect"], "by": "console-1"}))];
        let (ids, text) = undelivered(&log, "architect").unwrap();
        assert_eq!(ids, ["rd_001"]);
        assert!(text.contains("the operator chose \"Ask red-team first\"") && text.contains("note: do it soon") && text.contains("condition: keep t_016 paused") && text.contains("condition: tell the architect"), "{text}");
        assert!(text.contains("not as permissions"));
        assert!(undelivered(&log, "orchestrator").is_some(), "the orchestrator relays");
        assert!(undelivered(&log, "builder").is_none(), "nobody else is told");
        let told = [log.clone(), vec![ev(3, "decision.delivered", "architect", json!({"id": "rd_001", "to": "architect"}))]].concat();
        assert!(undelivered(&told, "architect").is_none(), "told once");
        assert!(undelivered(&told, "orchestrator").is_some(), "each role is told once: the orchestrator has not been");
        assert!(undelivered(&[log[0].clone()], "architect").is_none(), "an open decision has no answer");
    }

    #[test]
    fn an_answer_is_cleaned_and_capped() {
        let (n, c) = clean_answer(&format!("a\u{1b}[0m{}", "b".repeat(900)), &["x".repeat(500), "".into(), "two".into(), "three".into(), "four".into()]);
        assert!(n.chars().count() <= NOTE_MAX && !n.contains('\u{1b}'));
        assert_eq!((c.len(), c[0].chars().count() <= CONDITION_MAX), (3, true));
    }
}
