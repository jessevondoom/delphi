//! Telling a mission what the added bundles are (`specs/024` §4.2 step 6, §3.4).
//!
//! `delphi bundle add` runs in the CLI and the daemon is not in that loop, and events live in missions, not in the
//! workspace. So the log learns of a bundle when a task starts: the first time a mission sees an added bundle it gets
//! `bundle.added` and one `provenance.stamp` for each of the bundle's files, with reason `bundle-add:<hash>`; a new hash
//! of a bundle it knew is `bundle.updated`, or `bundle.rolled_back` when the lock says a
//! rollback put the earlier hash back (`specs/024` §4.4); one that is no longer added is `bundle.removed`. Nothing here changes what
//! loads (that is read from the lock and the cache, `bundle_load`); it is the record, and it costs no model turn.

use crate::engine::Engine;
use crate::mission::{EngineError, MissionState};
use delphi_core::bundles::{self, Entry};
use delphi_core::event::EventEnvelope;
use delphi_core::provenance::{Index, Tier, Zone};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

/// What a mission already knows: bundle id to the hash it last saw. The vector events it was told live in the same map
/// under a prefix (`vectors:` the last progress as `done/total`, `rejected:` the cache file last reported), so that a
/// restarted engine does not say them twice.
pub(crate) type Seen = BTreeMap<String, String>;
const PROGRESS: &str = "vectors:";
const REJECTED: &str = "rejected:";
/// The tier the mission was last told for a bundle (`trust:<org/name>` to the tier name): a raise is an event of its own.
const TRUST: &str = "trust:";

fn seen_from_log(m: &MissionState) -> Seen {
    let mut seen = Seen::new();
    for e in m.replay().unwrap_or_default() {
        let id = || Some(format!("{}/{}", e.payload["org"].as_str()?, e.payload["name"].as_str()?));
        match (e.kind.as_str(), id()) {
            ("bundle.added" | "bundle.updated" | "bundle.rolled_back", Some(id)) => {
                seen.insert(format!("{}{}", TRUST, id), e.payload["tier"].as_str().unwrap_or(bundles::DEFAULT_TIER).to_string());
                seen.insert(id, e.payload["hash"].as_str().unwrap_or_default().to_string());
            }
            ("bundle.trust_changed", Some(id)) => {
                seen.insert(format!("{}{}", TRUST, id), e.payload["tier"].as_str().unwrap_or(bundles::DEFAULT_TIER).to_string());
            }
            ("bundle.removed", Some(id)) => {
                seen.remove(&format!("{}{}", TRUST, id));
                seen.remove(&id);
            }
            ("bundle.vectors_progress", Some(id)) => {
                seen.insert(format!("{}{}", PROGRESS, id), format!("{}/{}", e.payload["done"].as_u64().unwrap_or(0), e.payload["total"].as_u64().unwrap_or(0)));
            }
            ("bundle.vectors_rejected", Some(id)) => {
                seen.insert(format!("{}{}", REJECTED, id), e.payload["file"].as_str().unwrap_or_default().to_string());
            }
            _ => {}
        }
    }
    seen
}

/// The stamps for one bundle's files, from its `files.json` (checked against the locked hash first). Behavior, skills and
/// agents are what the operator approved by adding (L5, `bundle-add:<hash>`); knowledge is federated content at L2, or
/// L5 with reason `bundle-trust:<hash>` while the operator's raise is in force (`specs/024` §3.7).
fn stamps(m: &MissionState, ws: &crate::workspace::WorkspacePaths, e: &Entry) -> Vec<EventEnvelope> {
    let cache = bundles::cache_dir(&ws.home, &e.org, &e.name, &e.hash);
    let Ok(manifest) = std::fs::read(cache.join(".delphi-bundle/files.json")) else { return Vec::new() };
    if delphi_bundle::bundle_hash(&manifest) != e.hash {
        return Vec::new();
    }
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&manifest) else { return Vec::new() };
    let reason = format!("bundle-add:{}", e.hash);
    let knowledge = if e.raised() { Tier::L5 } else { Tier::L2 };
    let knowledge_reason = if e.raised() { format!("bundle-trust:{}", e.hash) } else { reason.clone() };
    let mut out = Vec::new();
    for (path, entry) in json["files"].as_object().into_iter().flatten() {
        let tier = if path.starts_with("knowledge/") {
            knowledge
        } else if path.starts_with("behavior/") || path.starts_with("skills/") || path.starts_with("agents/") {
            Tier::L5
        } else {
            continue;
        };
        let Some(hex) = entry["blake3"].as_str().map(|h| h.trim_start_matches("blake3:")) else { continue };
        out.push(EventEnvelope::new(&m.id, "provenance.stamp", Index::stamp_payload_with_reason(Zone::Bundle, &format!("{}/{}/{}", e.org, e.name, path), hex, tier, if path.starts_with("knowledge/") { &knowledge_reason } else { &reason })));
    }
    out
}

impl Engine {
    /// Bring `m`'s log up to date with the workspace's added bundles. Cheap when nothing changed.
    pub(crate) async fn reconcile_bundles(self: &Arc<Self>, m: &Arc<MissionState>) -> Result<(), EngineError> {
        let active = bundles::active_entries(&self.ws.dir);
        let mut seen = self.bundle_seen.lock().unwrap().remove(&m.id).unwrap_or_else(|| seen_from_log(m));
        let mut events = Vec::new();
        for e in &active {
            let payload = |extra: serde_json::Value| {
                let mut p = json!({"org": e.org, "name": e.name, "version": e.version, "hash": e.hash, "provenance": e.provenance, "order": e.order, "source": e.source.kind, "tier": e.tier_name()});
                if let (Some(p), Some(x)) = (p.as_object_mut(), extra.as_object()) {
                    p.extend(x.clone());
                }
                p
            };
            let tkey = format!("{}{}", TRUST, e.id());
            let told = seen.get(&tkey).cloned().unwrap_or_else(|| bundles::DEFAULT_TIER.to_string());
            let known = seen.contains_key(&e.id());
            let tier_changed = known && told != e.tier_name();
            match seen.get(&e.id()) {
                Some(h) if *h == e.hash && !tier_changed => continue,
                Some(h) if *h == e.hash => {}
                Some(h) => {
                    // `rollback` leaves the version it replaced at the head of the history, marked; an update never does.
                    let rolled_back = e.history.first().is_some_and(|p| p.left_by == "rollback" && p.hash == *h);
                    events.push(EventEnvelope::new(&m.id, if rolled_back { "bundle.rolled_back" } else { "bundle.updated" }, payload(json!({"from_hash": h}))));
                }
                None => events.push(EventEnvelope::new(&m.id, "bundle.added", payload(json!({})))),
            }
            if tier_changed {
                // The lock's own log says who changed it and why (`cli`, `console`, `trusted-source`, `update`, `rollback`...).
                let by = e.trust_log.first().map(|c| c.by.as_str()).unwrap_or("lock");
                events.push(EventEnvelope::new(&m.id, "bundle.trust_changed", payload(json!({"from": told, "by": by}))));
            }
            events.extend(stamps(m, &self.ws, e));
            seen.insert(e.id(), e.hash.clone());
            seen.insert(tkey, e.tier_name().to_string());
        }
        // Vector readiness (`specs/027` §5.4): the numbers come from the index, which `delphi bundle add` and the background
        // pass change outside any mission, so they are reported here, when they have moved since the log last said so.
        let status = crate::bundle_vectors::statuses(&self.ws.home, &self.ws.dir, &active);
        for e in &active {
            let Some(s) = status.get(&e.id()) else { continue };
            let payload = |extra: serde_json::Value| {
                let mut p = json!({"org": e.org, "name": e.name, "hash": e.hash});
                if let (Some(p), Some(x)) = (p.as_object_mut(), extra.as_object()) {
                    p.extend(x.clone());
                }
                p
            };
            let now = format!("{}/{}", s.done, s.total);
            if s.total > 0 && seen.get(&format!("{}{}", PROGRESS, e.id())) != Some(&now) {
                events.push(EventEnvelope::new(&m.id, "bundle.vectors_progress", payload(json!({"done": s.done, "total": s.total, "percent": s.percent(), "shipped": s.shipped}))));
                seen.insert(format!("{}{}", PROGRESS, e.id()), now);
            }
            for r in &s.rejected {
                if seen.get(&format!("{}{}", REJECTED, e.id())) != Some(&r.file) {
                    events.push(EventEnvelope::new(&m.id, "bundle.vectors_rejected", payload(json!({"file": r.file, "failing": r.failing.iter().take(20).collect::<Vec<_>>(), "sampled": r.sampled}))));
                    seen.insert(format!("{}{}", REJECTED, e.id()), r.file.clone());
                }
            }
        }
        let gone: Vec<(String, String)> = seen.iter().filter(|(id, _)| !id.starts_with(PROGRESS) && !id.starts_with(REJECTED) && !id.starts_with(TRUST) && !active.iter().any(|e| &e.id() == *id)).map(|(i, h)| (i.clone(), h.clone())).collect();
        for (id, hash) in gone {
            let (org, name) = id.split_once('/').unwrap_or(("", &id));
            events.push(EventEnvelope::new(&m.id, "bundle.removed", json!({"org": org, "name": name, "hash": hash})));
            seen.remove(&format!("{}{}", TRUST, id));
            seen.remove(&id);
        }
        if !events.is_empty() {
            m.append_batch(events).await?;
        }
        self.bundle_seen.lock().unwrap().insert(m.id.clone(), seen);
        Ok(())
    }
}
