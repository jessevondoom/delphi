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
//! [`DAILY`] items a day unless `[memory] digest_daily` says otherwise, and it asks for nothing. Pure functions of the mission's
//! events; no model is called.
//!
//! A provisional note (`specs/116` §3) is shown the same way, the most used first: the operator leaves it, marks it wrong, or
//! proposes it as a workspace heuristic for `/retro`. No action here writes a lesson, a heuristic or any rule.

use crate::decision_model::{clean, TITLE_MAX};
use crate::learn::{is_contact, live_notes, LiveNote, MAX_PROPOSAL_TEXT};
use crate::provisional_notes::Note;
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

/// The ids of everything the operator has already acted on in this mission.
pub fn acted(events: &[EventEnvelope]) -> Vec<&str> {
    events.iter().filter(|e| e.kind == "learn.digested").filter_map(|e| e.payload["id"].as_str()).collect()
}

/// How many items were acted on today; the quota of [`DAILY`] counts all kinds.
pub fn used_today(events: &[EventEnvelope], now: &str) -> usize {
    events.iter().filter(|e| e.kind == "learn.digested" && day_of(&e.timestamp) == day_of(now)).count()
}

/// The candidate to show now, and how many more wait today: the oldest live captured note nobody has acted on, unless
/// today's quota is used. `now` is an RFC 3339 timestamp.
pub fn next(mission: &str, events: &[EventEnvelope], now: &str) -> Option<(LiveNote, usize)> {
    next_within(mission, events, now, DAILY)
}

/// [`next`] under the operator's own daily limit (`[memory] digest_daily`, `specs/116` §3 item 4).
pub fn next_within(mission: &str, events: &[EventEnvelope], now: &str, daily: usize) -> Option<(LiveNote, usize)> {
    let acted = acted(events);
    let today = used_today(events, now);
    if today >= daily {
        return None;
    }
    let mut pending: Vec<LiveNote> = live_notes(mission, events).into_iter().filter(|n| is_contact(&n.trigger) && !acted.contains(&n.id.as_str())).collect();
    if pending.is_empty() {
        return None;
    }
    let left = pending.len().min(daily - today) - 1;
    Some((pending.remove(0), left))
}

// ── provisional notes in the digest (`specs/116` §3) ────────────────────────────────────────────────────────────

/// What the operator may do with a provisional note from the console, in the order the pane lists them. Keeping one as a lesson
/// is not among them: that is `delphi learn keep` at a terminal, where the operator words the trigger and types yes.
pub const NOTE_ACTIONS: [&str; 3] = ["leave", "heuristic", "wrong"];

/// The id a provisional note is acted on by: its staged reference, marked as the tier's.
pub fn note_id(n: &Note) -> String {
    format!("pv:{}", n.reference())
}

/// A note written by a span that had read nothing untrusted (and by no host) may be proposed as a heuristic; any other may only
/// be left, marked wrong, or kept at the terminal.
fn proposable(n: &Note) -> bool {
    !n.tainted()
}

/// What the operator is asked to judge, hashed: the note, its words and how often it was used, so an answer to a stale card is refused.
pub fn note_hash(n: &Note) -> String {
    blake3::hash(format!("{}\n{}\n{}\n{}", note_id(n), n.rel_path(), n.uses, n.text).as_bytes()).to_hex().to_string()
}

/// The `delphi learn keep` command for a provisional note, as a lesson for its role (a rule for one no single role wrote).
pub fn note_keep_command(n: &Note) -> String {
    if n.role == crate::provisional_notes::SHARED {
        format!("delphi learn keep {} --as rule", n.reference())
    } else {
        format!("delphi learn keep {} --as lesson --role {} --text \"T: <when it applies> D: <what to do>\"", n.reference(), n.role)
    }
}

/// A provisional note as an entry of `decision.list` (`specs/116` §3 item 1): kind `learn`, tier `open`, tickable for `leave` only.
pub fn note_decision(n: &Note, left: usize, now: &str) -> Value {
    let mut choices = vec![json!({"id": "leave", "label": "Leave it", "effect": "digest", "reversible": true})];
    if proposable(n) {
        choices.push(json!({"id": "heuristic", "label": "Keep as a workspace heuristic: propose it in /retro", "effect": "digest", "reversible": false}));
    }
    choices.push(json!({"id": "wrong", "label": "Mark it wrong", "effect": "digest", "reversible": false}));
    let more = if left == 0 { String::new() } else { format!(" {} more wait.", left) };
    let used = format!("used {} time(s) in tasks that ended well", n.uses);
    let mut changes = vec![n.label(), used.clone(), format!("valid until {}", n.valid_until), format!("to keep it as a lesson, at a terminal: {}", note_keep_command(n))];
    if !proposable(n) {
        changes.push("cannot be proposed here: it was written after reading untrusted content, or by a host".to_string());
    }
    json!({
        "id": card_id(now), "source": "digest", "source_id": note_id(n), "label": "DIGEST",
        "kind": "learn", "tier": "open", "title": clean(&n.text, TITLE_MAX),
        "requester": "daemon", "task": "", "raised_at": n.created, "timeout_secs": Value::Null, "payload_blake3": note_hash(n),
        "brief": {
            "summary": clean(&format!("An unreviewed note by {} ({}), {}. No one has reviewed it; a later run of the same role may be given it as data until {}. Leave it, mark it wrong, or keep it: as a workspace heuristic through /retro, or as a lesson with `delphi learn keep` at a terminal. Nothing waits on this.{}", n.role, n.kind, used, n.valid_until, more), 400),
            "changes": changes, "state": "plain",
            "recommendation": {"choice_id": "leave", "why": "it stays an unreviewed note and expires on its own; it comes back here only if it is used again"},
        },
        "items": [{"type": "note", "id": note_id(n), "text": clean(&n.text, 400), "from": n.trigger, "role": n.role, "kind": n.kind, "uses": n.uses, "tier": n.tier.as_str(), "status": "provisional", "label": n.label()}],
        "choices": choices,
        "tickable": true, "tickable_reason": "leaving changes nothing",
    })
}

/// The events one action on a provisional note writes: the `learn.digested` mark that counts toward the day's limit, and for
/// `heuristic` a proposal the operator still has to accept in `/retro` (never a rule). The note's own file is the caller's to
/// change, with [`crate::provisional_notes::leave`] or [`crate::provisional_notes::mark_wrong`]. `Err` for an unknown action or a
/// proposal of a note written after reading untrusted content.
pub fn note_act(mission_id: &str, n: &Note, action: &str) -> Result<Vec<EventEnvelope>, String> {
    let mut out = Vec::new();
    match action {
        "leave" | "wrong" => {}
        "heuristic" => {
            if !proposable(n) {
                return Err("this note was written after reading untrusted content, or by a host; it can be left, marked wrong, or kept with `delphi learn keep` at a terminal, not proposed here".into());
            }
            let text: String = n.claim().chars().take(MAX_PROPOSAL_TEXT).collect();
            // A proposal, never a rule: the operator still accepts it in `/retro`, and nothing loads before that.
            out.push(EventEnvelope::new(mission_id, "learn.proposed", json!({"op": "add", "scope": "workspace", "text": text, "rationale": format!("promoted from the digest: the provisional note {} by {} ({} use(s))", n.reference(), n.role, n.uses), "target": "", "tier": n.tier.min(Tier::L3).as_str()})));
        }
        _ => return Err(format!("`{}` is not an action on a provisional note ({})", action, NOTE_ACTIONS.join(", "))),
    }
    out.push(EventEnvelope::new(mission_id, "learn.digested", json!({"id": note_id(n), "action": action})));
    Ok(out)
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

/// The learning queue at a glance (`specs/083` §4): how many notes sit staged, how many proposals and promotions wait for
/// the operator, and when the oldest of them arrived. Counts and an age, never a score; derived from the logs on every call.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Health {
    pub staged: usize,
    pub review: usize,
    /// Of the staged notes, those a host wrote that `delphi learn keep` can keep (`specs/108` M2).
    pub host_notes: usize,
    /// The daemon's own timestamp of the oldest waiting item; `None` when nothing waits or no timestamp parses.
    pub oldest: Option<String>,
}

impl Health {
    pub fn to_json(&self) -> Value {
        json!({"staged": self.staged, "review": self.review, "host_notes": self.host_notes, "keep": "delphi learn keep <mission>/<id> --as lesson --role <role> | --as rule", "oldest_at": self.oldest})
    }
}

/// [`Health`] of a set of missions' events, each as `(mission id, events)`. Only the daemon's event timestamps feed the age.
pub fn health_of(missions: &[(String, Vec<EventEnvelope>)]) -> Health {
    let mut h = Health::default();
    let mut oldest: Option<(i64, String)> = None;
    let mut see = |ts: &str| {
        if let Some(s) = delphi_core::time::parse_rfc3339(ts) {
            if oldest.as_ref().is_none_or(|(o, _)| s < *o) {
                oldest = Some((s, ts.to_string()));
            }
        }
    };
    for (id, events) in missions {
        let notes = live_notes(id, events);
        h.staged += notes.len();
        h.host_notes += notes.iter().filter(|n| matches!(n.trigger.as_str(), "host_note" | "host_transcript")).count();
        notes.iter().for_each(|n| see(&n.timestamp));
        let proposals = crate::learn::live_proposals(id, events);
        h.review += proposals.len();
        proposals.iter().for_each(|p| see(&p.timestamp));
        let promotions = crate::promote::live_promotions(id, events);
        h.review += promotions.len();
        promotions.iter().for_each(|p| see(&p.timestamp));
    }
    h.oldest = oldest.map(|(_, t)| t);
    h
}

/// [`health_of`] over every mission of the workspace. Reads the logs and nothing else.
pub fn health(ws_dir: &std::path::Path) -> Health {
    let missions: Vec<(String, Vec<EventEnvelope>)> = delphi_core::sessions::dirs(ws_dir).into_iter().filter_map(|(id, path)| Some((id, delphi_core::log::replay(&path).ok()?))).collect();
    health_of(&missions)
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
        let low = to_decision(&LiveNote { mission: "m1".into(), id: "n2".into(), trigger: "council_verdict".into(), summary: "build the table".into(), tier: "L3".into(), task_id: None, role: None, timestamp: NOW.into(), event_seqs: vec![2], valid_until: None, shown: None }, 0, NOW);
        let ids = |d: &Value| d["choices"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!((low["kind"].as_str(), low["tier"].as_str(), low["source"].as_str(), low["id"].as_str()), (Some("learn"), Some("open"), Some("digest"), Some("digest:2026-10-29")));
        assert_eq!(ids(&low), ["keep", "check", "dismiss"]);
        assert!(low["brief"]["summary"].as_str().unwrap().contains("not validated") && !low.to_string().contains("untrusted"));
        assert_eq!(low["brief"]["recommendation"]["choice_id"], "keep");
        assert_eq!(low["items"][0]["type"], "claim", "the card carries typed items");
        assert_eq!(low["items"][0]["label"], "not validated");
        let high = to_decision(&LiveNote { mission: "m1".into(), id: "n3".into(), trigger: "decision_resolved".into(), summary: "ship it".into(), tier: "L5".into(), task_id: None, role: None, timestamp: NOW.into(), event_seqs: vec![3], valid_until: None, shown: None }, 2, NOW);
        assert_eq!(ids(&high), ["keep", "check", "promote", "dismiss"]);
        assert_ne!(payload_hash(&LiveNote { mission: "m1".into(), id: "n3".into(), trigger: "x".into(), summary: "a".into(), tier: "L5".into(), task_id: None, role: None, timestamp: NOW.into(), event_seqs: vec![], valid_until: None, shown: None }), high["payload_blake3"].as_str().unwrap());
    }

    #[test]
    fn promote_only_proposes_for_the_retro_review_and_never_a_card_or_a_rule_and_a_low_tier_cannot_promote() {
        let mut n = LiveNote { mission: "m1".into(), id: "n3".into(), trigger: "decision_resolved".into(), summary: "ship it".into(), tier: "L5".into(), task_id: None, role: None, timestamp: NOW.into(), event_seqs: vec![3], valid_until: None, shown: None };
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

    fn pnote(tier: Tier, role: &str, uses: u32) -> Note {
        Note { id: "2026-10-29-m1-n7".into(), role: role.into(), mission: "m1".into(), note: "n7".into(), project: String::new(), tier, kind: "pitfall".into(), created: NOW.into(), valid_until: "2026-11-28".into(), uses, trigger: if tier == Tier::L0 { "host_note".into() } else { "agent_note".into() }, source: None, left_uses: None, text: if tier == Tier::L0 { "[host:x/y] the CI image has no python3 (evidence: s7)".into() } else { "the CI image has no python3".into() }, path: std::path::PathBuf::new() }
    }

    /// `specs/116` §3 item 1: the operator's choices on a provisional note. The console leaves it, marks it wrong, or proposes it for
    /// `/retro`; a lesson is the terminal's; a tainted note cannot be proposed; nothing here writes a rule.
    #[test]
    fn a_provisional_note_is_left_marked_wrong_or_proposed_for_retro_and_never_written_as_a_rule_here() {
        let n = pnote(Tier::L3, "builder", 4);
        let d = note_decision(&n, 1, NOW);
        let ids = |d: &Value| d["choices"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(ids(&d), ["leave", "heuristic", "wrong"]);
        assert_eq!((d["kind"].as_str(), d["tier"].as_str(), d["source_id"].as_str(), d["brief"]["recommendation"]["choice_id"].as_str()), (Some("learn"), Some("open"), Some("pv:m1/n7"), Some("leave")));
        let text = d.to_string();
        assert!(text.contains("unreviewed note by builder, 2026-10-29") && text.contains("used 4 time(s)") && text.contains("delphi learn keep m1/n7 --as lesson --role builder"), "{text}");
        assert_eq!(d["items"][0]["status"], "provisional");
        let evs = note_act("m1", &n, "heuristic").unwrap();
        assert_eq!(evs.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["learn.proposed", "learn.digested"], "a proposal waits for /retro; no rule is written here");
        assert_eq!((evs[0].payload["op"].as_str(), evs[0].payload["scope"].as_str(), evs[0].payload["text"].as_str(), evs[0].payload["tier"].as_str()), (Some("add"), Some("workspace"), Some("the CI image has no python3"), Some("L3")));
        assert_eq!(evs[1].payload, json!({"id": "pv:m1/n7", "action": "heuristic"}));
        for a in ["leave", "wrong"] {
            let evs = note_act("m1", &n, a).unwrap();
            assert_eq!(evs.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["learn.digested"], "{a} writes the mark and nothing else");
        }
        for a in ["lesson", "keep", "promote", "approve", ""] {
            assert!(note_act("m1", &n, a).is_err(), "{a}");
        }
        // A host's note, or one written after reading untrusted content, cannot be proposed from the console.
        let host = pnote(Tier::L0, "builder", 0);
        assert_eq!(ids(&note_decision(&host, 0, NOW)), ["leave", "wrong"]);
        assert!(note_act("m1", &host, "heuristic").is_err() && note_decision(&host, 0, NOW).to_string().contains("from untrusted content"));
        // The hash moves with the note's uses and words, so a stale answer is refused.
        assert_ne!(note_hash(&n), note_hash(&pnote(Tier::L3, "builder", 5)));
        assert_eq!(note_keep_command(&pnote(Tier::L3, "shared", 0)), "delphi learn keep m1/n7 --as rule");
    }

    /// `specs/116` §3 item 4: the daily limit is the operator's number.
    #[test]
    fn the_daily_limit_is_a_number_the_operator_sets() {
        let mut evs: Vec<_> = (1..=12).map(|i| staged(i, &format!("n{}", i), "council_verdict", "L3", NOW)).collect();
        assert_eq!(next_within("m1", &evs, NOW, 3).unwrap().1, 2, "three today: this one and two more");
        assert_eq!(next_within("m1", &evs, NOW, 50).unwrap().1, 11);
        assert!(next_within("m1", &evs, NOW, 0).is_none(), "zero shows none");
        for i in 1..=3u64 {
            evs.push(digested(100 + i, &format!("n{}", i), NOW));
        }
        assert!(next_within("m1", &evs, NOW, 3).is_none() && next_within("m1", &evs, NOW, 4).is_some());
        assert_eq!(next("m1", &evs, NOW).map(|(n, _)| n.id), next_within("m1", &evs, NOW, DAILY).map(|(n, _)| n.id), "the default is ten");
    }

    #[test]
    fn health_counts_staged_and_waiting_and_dates_the_oldest_from_daemon_timestamps_only() {
        assert_eq!(health_of(&[]), Health::default());
        let mut dropped = EventEnvelope::new("m1", "learn.dropped", json!({"id": "n1"}));
        dropped.seq = 9;
        let mut proposed = EventEnvelope::new("m2", "learn.proposed", json!({"op": "add", "scope": "workspace", "text": "Run the formatter first.", "rationale": "r", "target": "", "tier": "L3"}));
        proposed.seq = 2;
        proposed.timestamp = "2026-10-30T08:00:00Z".into();
        let m1 = vec![staged(1, "n1", "agent_note", "L2", "2026-10-01T00:00:00Z"), staged(2, "n2", "agent_note", "L2", "2026-11-01T00:00:00Z"), staged(3, "n3", "agent_note", "L2", "2026-11-02T00:00:00Z"), dropped];
        let h = health_of(&[("m1".into(), m1), ("m2".into(), vec![proposed.clone()])]);
        assert_eq!((h.staged, h.review), (2, 1), "a dropped note no longer counts");
        assert_eq!(h.oldest.as_deref(), Some("2026-10-30T08:00:00Z"), "the dropped note's date does not count either");
        assert_eq!(h.to_json()["oldest_at"], "2026-10-30T08:00:00Z");
        let mut resolved = EventEnvelope::new("m2", "learn.proposal_resolved", json!({"id": "p2", "resolution": "rejected"}));
        resolved.seq = 3;
        assert_eq!(health_of(&[("m2".into(), vec![proposed.clone(), resolved])]), Health::default(), "a resolved proposal leaves the queue");
        let mut undated = proposed;
        undated.timestamp = "not a time".into();
        let h = health_of(&[("m2".into(), vec![undated])]);
        assert_eq!((h.review, h.oldest), (1, None), "no usable timestamp: counted, but no age");
    }

    #[test]
    fn health_counts_the_host_notes_waiting_and_a_kept_one_leaves_the_count() {
        let host = vec![staged(1, "h1", "host_note", "L0", NOW), staged(2, "h2", "host_note", "L0", NOW), staged(3, "n3", "agent_note", "L2", NOW)];
        let h = health_of(&[("host-a".into(), host.clone())]);
        assert_eq!((h.staged, h.host_notes), (3, 2));
        assert_eq!(h.to_json()["host_notes"], 2);
        let mut kept = EventEnvelope::new("host-a", "learn.proposal_resolved", json!({"id": "h1", "resolution": "accepted"}));
        kept.seq = 4;
        let mut after = host;
        after.push(kept);
        assert_eq!(health_of(&[("host-a".into(), after)]).host_notes, 1);
    }
}
