//! Telling a mission what happened to the joined Xenia stores (`specs/011` §4.1, §5.3; plan milestone X9).
//!
//! `delphi xenia sync` and the commands that publish run in the CLI, where the daemon is not in the loop, so what they
//! did is written to each store's journal (`delphi_core::xenia_status`). The log learns of it the way it learns of an
//! added bundle (`bundle_events.rs`): when a task starts, or when the console finishes an attestation. A mission that
//! has never been told about a store hears only the newest entry; after that it hears every newer one, at most
//! [`MAX_PER_STORE`] at a time.
//!
//! * `xenia.synced`: a sync accepted a new head (counts, freshness).
//! * `xenia.alert`: the store needs a human: a head was refused (tampering, rejected, non-fast-forward, refused) or the
//!   freshness statement is stale or past half its life.
//! * `xenia.published`: a publish, endorsement, withdrawal ... went out, or was refused with a fixed slug.
//!
//! An accepted sync also writes the `provenance.stamp` events X6 built (one per record in force, zone `xenia`, L1 or
//! L2 by the computed tier, reason `xenia:<head>:rev<n>`), so the span ledger of the mission can tell what it read.
//! Events carry slugs, counts and ids only: never record text. No model turn is spent.

use crate::engine::Engine;
use crate::mission::{EngineError, MissionState};
use delphi_core::event::EventEnvelope;
use delphi_core::xenia_status::{self, Entry, AGING_PCT};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Store id (`org/store`) to the last journal sequence number the mission was told.
pub(crate) type Seen = BTreeMap<String, u64>;

const MAX_PER_STORE: usize = 20;
/// Provenance stamps appended for one accepted head. A store with more records in force is stamped for the first
/// ones only; the cap keeps one sync from flooding a mission log.
const MAX_STAMPS: usize = 500;

fn seen_from_log(m: &MissionState) -> Seen {
    let mut seen = Seen::new();
    for e in m.replay().unwrap_or_default() {
        if !e.kind.starts_with("xenia.") {
            continue;
        }
        if let (Some(org), Some(store), Some(seq)) = (e.payload["org"].as_str(), e.payload["store"].as_str(), e.payload["seq"].as_u64()) {
            let slot = seen.entry(format!("{org}/{store}")).or_insert(0);
            *slot = (*slot).max(seq);
        }
    }
    seen
}

/// The events one journal entry stands for. Pure.
pub(crate) fn events_for(mission: &str, org: &str, store: &str, e: &Entry) -> Vec<EventEnvelope> {
    let base = |extra: serde_json::Value| {
        let mut p = json!({"org": org, "store": store, "seq": e.seq, "at": e.at});
        if let (Some(p), Some(x)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(x.clone());
        }
        p
    };
    let ev = |kind: &str, p: serde_json::Value| EventEnvelope::new(mission, kind, p);
    let mut out = Vec::new();
    match (e.kind.as_str(), e.outcome.as_str()) {
        ("sync", "accepted" | "up-to-date") => {
            if e.outcome == "accepted" {
                let d = &e.detail;
                out.push(ev("xenia.synced", base(json!({"head": e.head, "added": d["added"], "superseded": d["superseded"], "quarantined": d["quarantined"], "freshness": d["freshness"], "life_used_pct": d["life_used_pct"]}))));
            }
            match e.detail["freshness"].as_str() {
                Some("fresh") | None => {
                    if e.detail["life_used_pct"].as_u64().is_some_and(|p| p >= AGING_PCT as u64) {
                        out.push(ev("xenia.alert", base(json!({"reason": "statement-aging", "life_used_pct": e.detail["life_used_pct"]}))));
                    }
                }
                Some(why) => out.push(ev("xenia.alert", base(json!({"reason": "stale", "freshness": why})))),
            }
        }
        ("sync", "tampering" | "rejected" | "non-fast-forward" | "refused") => {
            out.push(ev("xenia.alert", base(json!({"reason": e.outcome, "count": e.detail["count"], "detail": e.detail["reason"]}))));
        }
        ("publish", _) => out.push(ev("xenia.published", base(json!({"action": e.detail["action"], "outcome": e.outcome, "commit": e.head})))),
        ("admin", _) => out.push(ev("xenia.admin", base(json!({"outcome": e.outcome, "proposal": e.detail["proposal"], "via": e.detail["via"], "commit": e.head})))),
        _ => {}
    }
    out
}

impl Engine {
    /// Bring `m`'s log up to date with what the joined stores' journals say. Cheap when there is no store or nothing new.
    pub(crate) async fn reconcile_xenia(self: &Arc<Self>, m: &Arc<MissionState>) -> Result<(), EngineError> {
        let home = self.ws.home.clone();
        let stores = xenia_status::list_stores(&home);
        if stores.is_empty() {
            return Ok(());
        }
        let mut seen = self.xenia_seen.lock().unwrap().remove(&m.id).unwrap_or_else(|| seen_from_log(m));
        let mut events = Vec::new();
        let mut stamp_for: Option<(delphi_core::xenia_status::StoreRef, i64)> = None;
        for r in stores {
            let Ok(store) = xenia_status::open(&home, &r) else { continue };
            let journal = xenia_status::read_journal(&store);
            let told = seen.get(&r.id()).copied();
            let fresh: Vec<&Entry> = match told {
                Some(t) => journal.iter().filter(|e| e.seq > t).collect(),
                None => journal.last().into_iter().collect(),
            };
            let skip = fresh.len().saturating_sub(MAX_PER_STORE);
            for e in fresh.iter().skip(skip) {
                events.extend(events_for(&m.id, &r.org, &r.store, e));
                if e.kind == "sync" && e.outcome == "accepted" {
                    stamp_for = Some((r.clone(), e.at));
                }
            }
            if let Some(last) = journal.last() {
                seen.insert(r.id(), last.seq);
            }
        }
        if let Some((r, at)) = stamp_for {
            let home = home.clone();
            let derived = tokio::task::spawn_blocking(move || {
                let store = xenia_status::open(&home, &r).ok()?;
                delphi_core::xenia_index::derive(&store, &delphi_core::xenia_sync::VerifyConfig::at(delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(at))).ok()
            })
            .await
            .ok()
            .flatten();
            if let Some(d) = derived {
                let stamps = d.stamps();
                events.extend(crate::xenia_read::stamp_events(&m.id, &stamps[..stamps.len().min(MAX_STAMPS)]));
            }
        }
        if !events.is_empty() {
            m.append_batch(events).await?;
        }
        self.xenia_seen.lock().unwrap().insert(m.id.clone(), seen);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::xenia_status::Entry;

    fn entry(seq: u64, kind: &str, outcome: &str, detail: serde_json::Value) -> Entry {
        Entry { seq, at: 1000, kind: kind.into(), outcome: outcome.into(), head: Some("a".repeat(40)), detail }
    }

    fn kinds(v: &[EventEnvelope]) -> Vec<String> {
        v.iter().map(|e| e.kind.clone()).collect()
    }

    #[test]
    fn an_accepted_sync_is_one_event_and_a_stale_or_aging_statement_is_an_alert() {
        let ev = events_for("m", "acme", "main", &entry(3, "sync", "accepted", json!({"added": 2, "superseded": 0, "quarantined": 1, "freshness": "fresh", "life_used_pct": 10})));
        assert_eq!(kinds(&ev), vec!["xenia.synced"]);
        assert_eq!((ev[0].payload["seq"].as_u64(), ev[0].payload["added"].as_u64()), (Some(3), Some(2)));
        let aging = events_for("m", "acme", "main", &entry(4, "sync", "up-to-date", json!({"freshness": "fresh", "life_used_pct": 57})));
        assert_eq!(kinds(&aging), vec!["xenia.alert"], "an unchanged head is not news, but a statement past half its life is");
        assert_eq!(aging[0].payload["reason"], "statement-aging");
        let stale = events_for("m", "acme", "main", &entry(5, "sync", "accepted", json!({"freshness": "expired"})));
        assert_eq!(kinds(&stale), vec!["xenia.synced", "xenia.alert"]);
        assert_eq!(stale[1].payload["reason"], "stale");
        assert!(events_for("m", "acme", "main", &entry(6, "sync", "up-to-date", json!({"freshness": "fresh", "life_used_pct": 3}))).is_empty());
    }

    #[test]
    fn a_refused_head_is_an_alert_with_a_count_and_never_a_path_or_text() {
        let ev = events_for("m", "acme", "main", &entry(1, "sync", "tampering", json!({"count": 2, "findings": [["records/memory/X.md", "deleted"]]})));
        assert_eq!(kinds(&ev), vec!["xenia.alert"]);
        assert_eq!((ev[0].payload["reason"].as_str(), ev[0].payload["count"].as_u64()), (Some("tampering"), Some(2)));
        assert!(!ev[0].payload.to_string().contains("records/memory"), "paths stay in `status`, not in the log");
        assert!(events_for("m", "acme", "main", &entry(2, "sync", "unavailable", json!({}))).is_empty());
        for o in ["rejected", "non-fast-forward", "refused"] {
            assert_eq!(kinds(&events_for("m", "acme", "main", &entry(2, "sync", o, json!({"reason": "x"})))), vec!["xenia.alert"], "{o}");
        }
    }

    #[test]
    fn a_publish_is_one_event_with_the_action_and_the_outcome() {
        let ev = events_for("m", "acme", "main", &entry(7, "publish", "denied", json!({"action": "endorse"})));
        assert_eq!(kinds(&ev), vec!["xenia.published"]);
        assert_eq!((ev[0].payload["action"].as_str(), ev[0].payload["outcome"].as_str()), (Some("endorse"), Some("denied")));
    }
}
