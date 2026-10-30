//! The digest's library items (`specs/052` §3.4, slice KS4): contested pairs and claims due for review, as typed `items`
//! of the same `learn` card the captured candidates use (`digest.rs`, KS3).
//!
//! A pair shows both claims side by side, what each says, where it came from and how old it is; the daemon picks no
//! winner. A review-due claim shows why. Both are offered before the captured candidates, share the day's quota, and
//! ask for nothing: **keep** and **check** change nothing, **dismiss** hides the item for this mission, and a due claim
//! can be **confirmed**, which only moves its `last_confirmed` date (`Library::confirm_claim`). Nothing here resolves a
//! card, satisfies a gate, retires, edits or promotes a claim, or raises a status; the label of every claim is derived
//! by `claims::status`. Text of a claim that rests on a private source is withheld.

use crate::decision_model::{clean, TITLE_MAX};
use crate::digest::{acted, used_today, DAILY};
use crate::library::claims::status;
use crate::library::review::due_reason;
use crate::library::Library;
use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};

pub const PAIR_ACTIONS: [&str; 3] = ["keep", "check", "dismiss"];
pub const DUE_ACTIONS: [&str; 4] = ["keep", "check", "confirm", "dismiss"];

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Pair,
    Due(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewItem {
    pub id: String,
    pub kind: Kind,
    pub claims: Vec<Value>,
    pub age_days: i64,
}

fn age_days(c: &Value, now_days: i64) -> i64 {
    c["last_confirmed"].as_str().and_then(delphi_core::time::parse_rfc3339).map_or(0, |t| (now_days - t.div_euclid(86_400)).max(0))
}

fn view(lib: &Library, c: &Value, now_days: i64, contested: bool) -> Value {
    let st = if contested { delphi_core::knowledge_status::Status::Contested } else { status(c) };
    let private = lib.rests_on_private(c);
    json!({
        "type": "claim", "id": c["id"],
        "text": if private { "(rests on a private source: the text stays in the library)".to_string() } else { clean(c["text"].as_str().unwrap_or(""), 400) },
        "from": lib.origin_of(c), "status": st.as_str(), "label": st.label(), "age_days": age_days(c, now_days),
    })
}

/// Pairs first, then claims due for review, those the operator has not acted on in this mission. `now` is RFC 3339.
pub fn items(lib: &Library, events: &[EventEnvelope], now: &str) -> Vec<ReviewItem> {
    let done = acted(events);
    let Some(now_days) = delphi_core::time::parse_rfc3339(now).map(|t| t.div_euclid(86_400)) else { return Vec::new() };
    let mut out = Vec::new();
    for p in lib.contested_pairs() {
        let id = p.id();
        if done.contains(&id.as_str()) {
            continue;
        }
        let claims: Vec<Value> = [&p.a, &p.b].iter().filter_map(|i| lib.claim(i)).map(|c| view(lib, &c, now_days, true)).collect();
        if claims.len() == 2 {
            let age = claims.iter().map(|c| c["age_days"].as_i64().unwrap_or(0)).max().unwrap_or(0);
            out.push(ReviewItem { id, kind: Kind::Pair, claims, age_days: age });
        }
    }
    for d in lib.review_due(now) {
        let Some(c) = lib.claim(&d.id) else { continue };
        let id = format!("due:{}:{}", d.id, c["last_confirmed"].as_str().unwrap_or("").get(..10).unwrap_or(""));
        if done.contains(&id.as_str()) || due_reason(&c, now_days).is_none() {
            continue;
        }
        out.push(ReviewItem { id, kind: Kind::Due(d.reason.clone()), claims: vec![view(lib, &c, now_days, false)], age_days: d.age_days });
    }
    out
}

/// What the operator is asked to judge, hashed: it changes with the claims' text, so an answer to a stale card is refused.
pub fn payload_hash(i: &ReviewItem) -> String {
    let texts: Vec<&str> = i.claims.iter().filter_map(|c| c["text"].as_str()).collect();
    blake3::hash(format!("{}\n{}", i.id, texts.join("\n")).as_bytes()).to_hex().to_string()
}

pub fn to_decision(i: &ReviewItem, left: usize, now: &str) -> Value {
    let (actions, summary, changes, title) = match &i.kind {
        Kind::Pair => (
            &PAIR_ACTIONS[..],
            "Two claims in the library say nearly the same thing in different words and may disagree. Both are shown; neither is picked for you.".to_string(),
            {
                let mut v: Vec<String> = i.claims.iter().enumerate().map(|(n, c)| clean(&format!("{}: {} ({} days since confirmed, from {})", if n == 0 { "one" } else { "other" }, c["text"].as_str().unwrap_or(""), c["age_days"], c["from"].as_str().unwrap_or("?")), 200)).collect();
                v.push("both stay as they are; a possible conflict found by word overlap, it may be a false alarm".to_string());
                v
            },
            format!("Possible conflict: {}", i.claims.first().and_then(|c| c["text"].as_str()).unwrap_or("")),
        ),
        Kind::Due(why) => (&DUE_ACTIONS[..], format!("This claim is due for review: {}. It is still shown and still pulled; confirm it if it holds.", why), vec![why.clone(), "confirming moves only its date; it does not validate it".to_string()], format!("Review due: {}", i.claims.first().and_then(|c| c["text"].as_str()).unwrap_or(""))),
    };
    let label = |a: &str| match a {
        "keep" => "Leave it as it is",
        "check" => "Ask the validator to check it",
        "confirm" => "It still holds: confirm it",
        _ => "Dismiss",
    };
    let choices: Vec<Value> = actions.iter().map(|a| json!({"id": a, "label": label(a), "effect": "digest", "reversible": *a != "confirm"})).collect();
    let more = if left == 0 { String::new() } else { format!(" {} more wait.", left) };
    json!({
        "id": crate::digest::card_id(now), "source": "digest", "source_id": i.id, "label": "DIGEST",
        "kind": "learn", "tier": "open", "title": clean(&title, TITLE_MAX),
        "requester": "daemon", "task": "", "raised_at": now, "timeout_secs": Value::Null, "payload_blake3": payload_hash(i),
        "brief": {
            "summary": clean(&format!("{} Nothing waits on this.{}", summary, more), 400),
            "changes": changes, "state": "plain",
            "recommendation": {"choice_id": "keep", "why": "nothing changes; the daemon decides nothing here"},
        },
        "items": i.claims,
        "choices": choices,
        "tickable": true, "tickable_reason": "keeping changes nothing",
    })
}

/// The events one action writes: the `learn.digested` mark that takes the item off the list. `Err` for an action the
/// item does not offer. `confirm` has a side effect the caller performs with `Library::confirm_claim`.
pub fn act(mission_id: &str, i: &ReviewItem, action: &str) -> Result<Vec<EventEnvelope>, String> {
    let allowed: &[&str] = if i.kind == Kind::Pair { &PAIR_ACTIONS } else { &DUE_ACTIONS };
    if !allowed.contains(&action) {
        return Err(format!("`{}` is not an action of this item ({})", action, allowed.join(", ")));
    }
    Ok(vec![EventEnvelope::new(mission_id, "learn.digested", json!({"id": i.id, "action": action}))])
}

pub const ACQUIRE_ACTIONS: [&str; 4] = ["accept", "skip", "rule", "keep"];

/// The acquisition batch the job proposed (`specs/055` §8): one card, the sources as typed items. `accept` takes them all, `skip`
/// declines them all and they are not proposed again, `rule` takes them and records a standing rule from the batch's topics and
/// archives, `keep` leaves them for later. The daemon captures what is accepted; nothing here sets a licence, level or status.
#[derive(Debug, Clone, PartialEq)]
pub struct AcquireItem {
    pub id: String,
    pub batch: String,
    pub sources: Vec<Value>,
}

pub fn acquire_item(lib: &Library, events: &[EventEnvelope]) -> Option<AcquireItem> {
    let b = lib.open_batch()?;
    let batch = b["id"].as_str()?.to_string();
    let id = format!("acq:{}", batch);
    if acted(events).contains(&id.as_str()) {
        return None;
    }
    let sources: Vec<Value> = b["items"]
        .as_array()?
        .iter()
        .filter(|i| i["status"] == "proposed")
        .map(|i| {
            json!({
                "type": "source", "id": i["id"], "title": clean(i["title"].as_str().unwrap_or(""), 160), "author": clean(i["author"].as_str().unwrap_or(""), 120),
                "from": i["archive"], "year": i["year"], "size": i["size"], "topic": i["topic"],
                "licence": i["licence"], "basis": clean(i["basis"].as_str().unwrap_or(""), 200),
                "status": "noted", "label": delphi_core::knowledge_status::Status::Noted.label(),
            })
        })
        .collect();
    (!sources.is_empty()).then_some(AcquireItem { id, batch, sources })
}

pub fn acquire_hash(i: &AcquireItem) -> String {
    let ids: Vec<&str> = i.sources.iter().filter_map(|s| s["id"].as_str()).collect();
    blake3::hash(format!("{}\n{}", i.id, ids.join("\n")).as_bytes()).to_hex().to_string()
}

pub fn acquire_decision(i: &AcquireItem, left: usize, now: &str) -> Value {
    let label = |a: &str| match a {
        "accept" => "Add these to the library",
        "skip" => "Skip these: do not propose them again",
        "rule" => "Add them, and take anything like them from these archives",
        _ => "Not now",
    };
    let choices: Vec<Value> = ACQUIRE_ACTIONS.iter().map(|a| json!({"id": a, "label": label(a), "effect": "digest", "reversible": *a == "keep"})).collect();
    let more = if left == 0 { String::new() } else { format!(" {} more wait.", left) };
    json!({
        "id": crate::digest::card_id(now), "source": "digest", "source_id": i.id, "label": "DIGEST",
        "kind": "learn", "tier": "open", "title": clean(&format!("{} sources found for the library", i.sources.len()), TITLE_MAX),
        "requester": "daemon", "task": "", "raised_at": now, "timeout_secs": Value::Null, "payload_blake3": acquire_hash(i),
        "brief": {
            "summary": clean(&format!("The library scout found public-domain and research sources that match your projects and interests. They are not fetched until you accept. Nothing waits on this.{}", more), 400),
            "changes": ["accepted sources are captured in full under the archive's own terms and kept with its notice", "they arrive not validated; their claims start as noted"],
            "state": "plain",
            "recommendation": {"choice_id": "keep", "why": "nothing is fetched until you accept"},
        },
        "items": i.sources,
        "choices": choices,
        "tickable": true, "tickable_reason": "keeping fetches nothing",
    })
}

pub fn acquire_act(mission_id: &str, i: &AcquireItem, action: &str) -> Result<Vec<EventEnvelope>, String> {
    if !ACQUIRE_ACTIONS.contains(&action) {
        return Err(format!("`{}` is not an action of this item ({})", action, ACQUIRE_ACTIONS.join(", ")));
    }
    Ok(vec![EventEnvelope::new(mission_id, "learn.digested", json!({"id": i.id, "action": action}))])
}

/// What the next card shows, and how many more wait today: a library item if there is one, else a captured candidate.
pub enum Pick {
    Review(ReviewItem),
    Acquire(AcquireItem),
    Note(crate::learn::LiveNote),
}

pub fn pick(mission: &str, lib: &Library, events: &[EventEnvelope], now: &str) -> Option<(Pick, usize)> {
    let used = used_today(events, now);
    if used >= DAILY {
        return None;
    }
    let mut reviews = items(lib, events, now);
    let note = crate::digest::next(mission, events, now);
    let acq = acquire_item(lib, events);
    let total = reviews.len() + usize::from(acq.is_some()) + note.as_ref().map_or(0, |(_, l)| l + 1);
    if total == 0 {
        return None;
    }
    let left = total.min(DAILY - used) - 1;
    if !reviews.is_empty() {
        return Some((Pick::Review(reviews.remove(0)), left));
    }
    if let Some(a) = acq {
        return Some((Pick::Acquire(a), left));
    }
    note.map(|(n, _)| (Pick::Note(n), left))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::claims::ClaimOpts;

    const NOW: &str = "2026-10-29T09:00:00.000Z";

    fn setup() -> (tempfile::TempDir, Library, String, String) {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let a = l.add_claim("the retry limit for the billing service is three attempts", &ClaimOpts::default(), &[]).unwrap()["id"].as_str().unwrap().to_string();
        let b = l.add_claim("the retry limit for the billing service is five attempts", &ClaimOpts::default(), &[]).unwrap()["id"].as_str().unwrap().to_string();
        (d, l, a, b)
    }

    fn digested(id: &str) -> EventEnvelope {
        EventEnvelope::new("m1", "learn.digested", json!({"id": id, "action": "keep"}))
    }

    #[test]
    fn a_contested_pair_is_one_card_with_both_claims_typed_contested_and_no_winner() {
        let (_d, l, _a, _b) = setup();
        let (p, left) = pick("m1", &l, &[], NOW).unwrap();
        let Pick::Review(i) = p else { panic!("a pair comes first") };
        assert_eq!(left, 0);
        let card = to_decision(&i, left, NOW);
        assert_eq!((card["kind"].as_str(), card["tier"].as_str(), card["source"].as_str()), (Some("learn"), Some("open"), Some("digest")));
        let items = card["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|c| c["type"] == "claim" && c["status"] == "contested" && c["label"] == delphi_core::knowledge_status::Status::Contested.label() && c["age_days"].is_i64() && c["from"].is_string()));
        let ids: Vec<&str> = card["choices"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["keep", "check", "dismiss"], "no choice picks a side");
        assert_eq!(card["brief"]["recommendation"]["choice_id"], "keep");
        assert!(!card.to_string().contains("untrusted"));
    }

    #[test]
    fn acting_on_a_pair_hides_it_and_writes_only_the_digested_mark_and_an_unknown_action_is_refused() {
        let (_d, l, _a, _b) = setup();
        let Some((Pick::Review(i), _)) = pick("m1", &l, &[], NOW) else { panic!() };
        let evs = act("m1", &i, "dismiss").unwrap();
        assert_eq!(evs.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["learn.digested"]);
        assert!(act("m1", &i, "confirm").is_err() && act("m1", &i, "approve").is_err() && act("m1", &i, "promote").is_err());
        assert!(pick("m1", &l, &[digested(&i.id)], NOW).is_none());
        assert_eq!(l.claims().len(), 2, "dismissing deletes nothing");
    }

    #[test]
    fn a_stale_claim_is_offered_for_review_with_confirm_and_its_status_is_not_raised() {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let id = l.add_claim("the staging cluster runs on three nodes", &ClaimOpts::default(), &[]).unwrap()["id"].as_str().unwrap().to_string();
        let mut c = l.claim(&id).unwrap();
        c["last_confirmed"] = json!("2026-06-01T00:00:00.000Z");
        std::fs::write(l.claims_dir().join(format!("{}.json", id)), serde_json::to_vec(&c).unwrap()).unwrap();
        let Some((Pick::Review(i), _)) = pick("m1", &l, &[], NOW) else { panic!("due claim") };
        let card = to_decision(&i, 0, NOW);
        assert!(card["title"].as_str().unwrap().starts_with("Review due"));
        assert_eq!(card["items"][0]["status"], "noted");
        assert_eq!(card["choices"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect::<Vec<_>>(), ["keep", "check", "confirm", "dismiss"]);
        assert_ne!(payload_hash(&i), "");
        assert!(act("m1", &i, "confirm").is_ok());
        l.confirm_claim(&id).unwrap();
        assert!(items(&l, &[], &delphi_core::time::now_rfc3339()).is_empty(), "a confirmed claim is not due");
    }

    #[test]
    fn the_quota_counts_library_items_and_notes_together() {
        let (_d, l, _a, _b) = setup();
        let evs: Vec<EventEnvelope> = (0..DAILY).map(|i| {
            let mut e = digested(&format!("x{}", i));
            e.timestamp = NOW.into();
            e
        }).collect();
        assert!(pick("m1", &l, &evs, NOW).is_none(), "today's quota is used");
    }
}
