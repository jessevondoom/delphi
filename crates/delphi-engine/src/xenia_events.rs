//! Telling a mission what happened to the org's Xenia stores (plan milestone X9b; `specs/011` §4.3, §5).
//!
//! Like `bundle_events`, this runs when a task starts: the daemon is not in the loop when `delphi xenia sync` runs in a
//! shell, so the log learns of a change when a task next begins. [`plan`] compares what the read-only status
//! (`delphi_core::xenia_status`) says now with what the mission's log already said and appends the difference:
//!
//! | event | when |
//! | :--- | :--- |
//! | `xenia.sync_accepted` | the last sync was accepted and the log has not heard of it |
//! | `xenia.sync_refused` | the last sync found tampering or was rejected, refused or not a fast-forward |
//! | `xenia.freshness_stale` | the freshness statement is stale (expired, missing, replayed ...), and the log was not told |
//! | `xenia.tier_changed` | the count of records at L2, L1, L0 or hidden moved (the first sighting logs it with `from: null`) |
//! | `xenia.quarantine_changed` | the number of quarantined files moved |
//!
//! **Payloads carry fixed slugs and counts, never store text**: an org and store name (slugs by construction), outcome
//! and reason slugs, a 12-character head prefix, numbers. A hostile record title cannot reach the log through here.
//! No model is called and nothing is fetched; the engine reads local files only.
//!
//! The mission's memory of what it was told sits in the engine's `bundle_seen` map under the outer key `xenia:<mission>`
//! (a separate entry, so the bundle reconciler never mistakes these keys for bundles).

use crate::engine::Engine;
use crate::mission::{EngineError, MissionState};
use delphi_core::event::EventEnvelope;
use delphi_core::xenia_status::{self as xs, StoreStatus};
use delphi_core::xenia_sync::VerifyConfig;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(crate) type Seen = BTreeMap<String, String>;

pub const KINDS: [&str; 5] = ["xenia.sync_accepted", "xenia.sync_refused", "xenia.freshness_stale", "xenia.tier_changed", "xenia.quarantine_changed"];

fn name(p: &serde_json::Value) -> Option<String> {
    Some(format!("{}/{}", p["org"].as_str()?, p["store"].as_str()?))
}

fn tiers_key(l2: u64, l1: u64, l0: u64, hidden: u64) -> String {
    format!("{l2}/{l1}/{l0}/{hidden}")
}

/// What the log already said, rebuilt from the events (after a restart).
pub(crate) fn seen_from_log(events: &[EventEnvelope]) -> Seen {
    let mut seen = Seen::new();
    for e in events {
        let Some(n) = name(&e.payload) else { continue };
        let n64 = |k: &str| e.payload[k].as_u64().unwrap_or(0);
        match e.kind.as_str() {
            "xenia.sync_accepted" | "xenia.sync_refused" => {
                seen.insert(format!("{n}#sync"), n64("at").to_string());
            }
            "xenia.freshness_stale" => {
                seen.insert(format!("{n}#fresh"), format!("stale:{}", e.payload["reason"].as_str().unwrap_or("")));
            }
            "xenia.tier_changed" => {
                seen.insert(format!("{n}#tiers"), tiers_key(n64("l2"), n64("l1"), n64("l0"), n64("hidden")));
            }
            "xenia.quarantine_changed" => {
                seen.insert(format!("{n}#quar"), n64("count").to_string());
            }
            _ => {}
        }
    }
    seen
}

/// The events to append for `all`, given what the mission has been told. Updates `seen`. Pure.
pub(crate) fn plan(mission: &str, seen: &mut Seen, all: &[StoreStatus]) -> Vec<EventEnvelope> {
    let mut out = Vec::new();
    for s in all {
        let n = format!("{}/{}", s.org, s.store);
        let base = json!({"org": s.org, "store": s.store});
        let with = |extra: serde_json::Value| {
            let mut p = base.clone();
            if let (Some(p), Some(x)) = (p.as_object_mut(), extra.as_object()) {
                p.extend(x.clone());
            }
            p
        };
        if let Some(note) = &s.last_sync {
            let key = format!("{n}#sync");
            let unseen = seen.get(&key).and_then(|v| v.parse::<i64>().ok()).is_none_or(|at| note.at > at);
            if unseen && note.outcome == "accepted" {
                let head = s.accepted_head.as_deref().map(|h| &h[..12.min(h.len())]).unwrap_or("");
                out.push(EventEnvelope::new(mission, "xenia.sync_accepted", with(json!({"head": head, "at": note.at}))));
                seen.insert(key, note.at.to_string());
            } else if unseen && note.alarming() {
                out.push(EventEnvelope::new(mission, "xenia.sync_refused", with(json!({"outcome": note.outcome, "reason": note.reason, "findings": note.findings, "at": note.at}))));
                seen.insert(key, note.at.to_string());
            }
        }
        if s.accepted_head.is_none() {
            continue;
        }
        let key = format!("{n}#fresh");
        let now_fresh = match &s.freshness {
            Some(f) if f.state == "stale" => format!("stale:{}", f.reason),
            _ => "fresh".to_string(),
        };
        if now_fresh == "fresh" {
            seen.remove(&key);
        } else if seen.get(&key) != Some(&now_fresh) {
            out.push(EventEnvelope::new(mission, "xenia.freshness_stale", with(json!({"reason": now_fresh.trim_start_matches("stale:")}))));
            seen.insert(key, now_fresh);
        }
        let key = format!("{n}#tiers");
        let t = &s.tiers;
        let now = tiers_key(t.l2 as u64, t.l1 as u64, t.l0 as u64, t.hidden as u64);
        if seen.get(&key) != Some(&now) {
            let from = seen.get(&key).map(|f| {
                let v: Vec<u64> = f.split('/').filter_map(|x| x.parse().ok()).collect();
                json!({"l2": v.first(), "l1": v.get(1), "l0": v.get(2), "hidden": v.get(3)})
            });
            out.push(EventEnvelope::new(mission, "xenia.tier_changed", with(json!({"l2": t.l2, "l1": t.l1, "l0": t.l0, "hidden": t.hidden, "from": from}))));
            seen.insert(key, now);
        }
        let key = format!("{n}#quar");
        let was = seen.get(&key).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        if s.quarantined.total != was {
            out.push(EventEnvelope::new(mission, "xenia.quarantine_changed", with(json!({"count": s.quarantined.total, "from": was}))));
            seen.insert(key, s.quarantined.total.to_string());
        }
    }
    out
}

impl Engine {
    /// Bring `m`'s log up to date with the org's Xenia stores. Cheap when no store is joined (one directory listing).
    pub(crate) async fn reconcile_xenia(self: &Arc<Self>, m: &Arc<MissionState>) -> Result<(), EngineError> {
        let home = self.ws.home.clone();
        if xs::discover(&home).is_empty() {
            return Ok(());
        }
        let now = delphi_core::xenia_index::unix_of(&self.now()).unwrap_or(0);
        let all = tokio::task::spawn_blocking(move || xs::status_all(&home, &VerifyConfig::at(now))).await.map_err(|e| EngineError::Invalid(e.to_string()))?;
        let outer = format!("xenia:{}", m.id);
        let cached = self.bundle_seen.lock().unwrap().remove(&outer);
        let mut seen = match cached {
            Some(s) => s,
            None => seen_from_log(&m.replay().unwrap_or_default()),
        };
        let events = plan(&m.id, &mut seen, &all);
        if !events.is_empty() {
            m.append_batch(events).await?;
        }
        self.bundle_seen.lock().unwrap().insert(outer, seen);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::xenia_fixture::*;
    use delphi_core::xenia_status::status_all;

    fn kinds(e: &[EventEnvelope]) -> Vec<&str> {
        e.iter().map(|e| e.kind.as_str()).collect()
    }

    #[test]
    fn the_first_sighting_logs_a_baseline_and_a_repeat_logs_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (fx, _) = Fx::installed(tmp.path(), &[]);
        let all = status_all(&fx.dir.join("home"), &cfg());
        let mut seen = Seen::new();
        let first = plan("m1", &mut seen, &all);
        assert_eq!(kinds(&first), vec!["xenia.sync_accepted", "xenia.tier_changed"], "{first:?}");
        assert_eq!(first[1].payload["from"], serde_json::Value::Null);
        assert!(plan("m1", &mut seen, &all).is_empty(), "nothing moved");
        // A restart rebuilds the same memory from the log.
        let mut again = seen_from_log(&first);
        assert_eq!(again, seen);
        assert!(plan("m1", &mut again, &all).is_empty());
    }

    #[test]
    fn a_lapsed_statement_a_tier_move_and_a_quarantine_each_log_once() {
        let tmp = tempfile::tempdir().unwrap();
        let fx0 = Fx::with_cast(tmp.path());
        let m0 = records::memory_with(&fx0.id("mia"), &u(0), 1, "active", "v", "payments", "internal_empirical");
        let e0 = endorse(&fx0.id("sam"), 10, &m0, 1, "payments");
        let (fx, _) = Fx::installed_on(fx0, &[&m0, &e0]);
        let home = fx.dir.join("home");
        let mut seen = Seen::new();
        let base = plan("m1", &mut seen, &status_all(&home, &cfg()));
        assert_eq!(base.iter().find(|e| e.kind == "xenia.tier_changed").map(|e| e.payload["l2"].clone()), Some(json!(1)));
        // Nine days on, the statement has expired: the L2 reads as L1.
        let later = status_all(&home, &VerifyConfig::at(now() + 9 * DAY));
        let ev = plan("m1", &mut seen, &later);
        assert_eq!(kinds(&ev), vec!["xenia.freshness_stale", "xenia.tier_changed", "xenia.quarantine_changed"], "{ev:?}");
        assert_eq!(ev[0].payload["reason"], "expired");
        assert_eq!((ev[1].payload["l2"].clone(), ev[1].payload["l1"].clone(), ev[1].payload["from"]["l2"].clone()), (json!(0), json!(1), json!(1)));
        assert!(plan("m1", &mut seen, &later).is_empty(), "told once");
        // Junk in the store is a quarantine count, never its text.
        fx.remote.write(&format!("records/memory/{}.md", u(7)), b"\x1b[2Jnot a record\n");
        fx.remote.write(&format!("records/memory/{}.md", u(8)), b"another \x1b[2Jnot a record\n");
        fx.commit("junk");
        let _ = fx.sync();
        let after = status_all(&home, &cfg());
        let ev = plan("m1", &mut seen, &after);
        assert!(kinds(&ev).contains(&"xenia.quarantine_changed"), "{ev:?}");
        let text = serde_json::to_string(&ev.iter().map(|e| &e.payload).collect::<Vec<_>>()).unwrap();
        assert!(!text.contains("not a record") && !text.contains('\u{1b}') && !text.contains("did:key"), "{text}");
    }

    #[test]
    fn a_refused_sync_is_logged_with_its_slug_and_a_hostile_host_adds_no_text() {
        let tmp = tempfile::tempdir().unwrap();
        let fx0 = Fx::with_cast(tmp.path());
        let m0 = records::memory_with(&fx0.id("mia"), &u(0), 1, "active", "v", "payments", "internal_empirical");
        let (fx, _) = Fx::installed_on(fx0, &[&m0]);
        let mut seen = Seen::new();
        let home = fx.dir.join("home");
        let _ = plan("m1", &mut seen, &status_all(&home, &cfg()));
        fx.remote.remove(&m0.path);
        fx.remote.remove(&m0.sidecar_path);
        fx.commit("delete a signed file");
        let _ = fx.sync_at(now() + 60);
        let ev = plan("m1", &mut seen, &status_all(&home, &VerifyConfig::at(now() + 60)));
        let refused = ev.iter().find(|e| e.kind == "xenia.sync_refused").expect("a refusal is logged");
        assert_eq!(refused.payload["outcome"], "tampering");
        assert!(refused.payload["reason"].as_str().is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')));
        assert!(!ev.iter().any(|e| e.kind == "xenia.sync_accepted"));
    }

    /// The wiring: a task starting is what tells the mission, once, with no model call; a restart does not repeat it; an
    /// install with no store costs nothing.
    #[tokio::test]
    async fn a_task_start_tells_the_mission_about_the_stores_once_and_calls_no_model_for_it() {
        use crate::testkit::{events_of, setup};
        let f = setup(vec![crate::model::Step::text("a"), crate::model::Step::text("b"), crate::model::Step::text("c")], 5.0, |_| {});
        f.engine.run_task(&f.m, "architect", "no store joined").await.unwrap();
        assert!(events_of(&f, "xenia.tier_changed").is_empty());
        let tmp = tempfile::tempdir().unwrap();
        let (fx, _) = Fx::installed(tmp.path(), &[]);
        let copy = std::process::Command::new("cp").arg("-R").arg(fx.dir.join("home/xenia")).arg(f.engine.ws.home.join("xenia")).status().unwrap();
        assert!(copy.success());
        f.now.store(now(), std::sync::atomic::Ordering::SeqCst);
        f.engine.run_task(&f.m, "architect", "one").await.unwrap();
        let tiers = events_of(&f, "xenia.tier_changed");
        assert_eq!((tiers.len(), tiers[0].payload["org"].as_str(), tiers[0].payload["store"].as_str()), (1, Some("acme"), Some("acme-main")));
        assert_eq!(events_of(&f, "xenia.sync_accepted").len(), 1);
        f.engine.run_task(&f.m, "architect", "two").await.unwrap();
        assert_eq!(events_of(&f, "xenia.tier_changed").len(), 1, "the second task adds nothing");
        let f2 = f.restart(vec![crate::model::Step::text("d")]);
        f2.engine.run_task(&f2.m, "architect", "three").await.unwrap();
        assert_eq!(events_of(&f2, "xenia.tier_changed").len(), 1, "a restarted engine reads what the mission was told from its log");
        assert_eq!(events_of(&f2, "xenia.sync_accepted").len(), 1);
    }
}
