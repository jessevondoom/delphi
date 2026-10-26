//! Xenia records in the knowledge base: how a verified sync reaches the index, and what an agent or an operator may
//! read from it (`specs/011` §5.3, §7.2; `specs/012` §8; milestone X6).
//!
//! * [`index_sync`] is where `Store::sync_verified`'s answer meets the index. It acts only on an accepted sync
//!   (`Accepted`, `UpToDate`) or an unreachable host (`Unavailable`: the accepted state is intact and the clock still
//!   moves, so freshness must still be able to demote). `Tampering`, `Rejected`, `NonFastForward` and `Refused` change
//!   nothing, not even the tag history. What it indexes is **not** the bytes of the sync result: it derives the zone
//!   from the accepted head again (`delphi_core::xenia_index::derive`), so the index can only ever hold what the
//!   accepted state, the pinned roster and the clock say, and a caller cannot pass it anything else.
//! * [`agent_search`] is the only way an agent reads org records: L2 summaries (title, summary, tags, who endorsed),
//!   each in a nonce-spotlighted `<xenia_record>` envelope, and it tells the caller to lower the span's ceiling to
//!   `L2`. A record that is not `federated_verified`, is hidden, or whose freshness statement has lapsed at the time
//!   of the read is not returned, whatever the index holds.
//! * [`quarantine_search`] and [`quarantine_get`] are the explicit read of everything else (L1, L0, hidden). They
//!   return bodies too, escaped and stripped, and the lowest tier they returned, which the caller must apply to the
//!   span. They are for the operator's console; no agent tool reaches them.

use crate::kb::{Kb, SearchOpts, XeniaApply, XeniaRow};
use delphi_core::event::EventEnvelope;
use delphi_core::provenance::{Span, Tier};
use delphi_core::xenia_index::{self, Derived, Stamp};
use delphi_core::xenia_ingress::{envelope, fresh_nonce, EnvelopeRecord};
use delphi_core::xenia_store::Store;
use delphi_core::xenia_sync::{VerifiedSync, VerifyConfig};

/// What [`index_sync`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum IndexOutcome {
    Indexed {
        head: String,
        /// Fixed slug: `fresh`, or why not (`expired`, `missing`, ...).
        freshness: String,
        apply: XeniaApply,
        /// Records in force / records the derivation put aside (listed by `status`, never indexed).
        in_force: usize,
        held_back: usize,
        /// The stamps to record as `provenance.stamp` events ([`stamp_events`]).
        stamps: Vec<Stamp>,
        /// Whether the sanitizer removed anything from any record's text.
        sanitized: bool,
    },
    /// The sync was refused (or its head is not the accepted one): the zone and the tag history are untouched.
    Skipped(&'static str),
}

/// Bring the zone in line with the accepted state, if `outcome` allows it. See the module docs.
pub fn index_sync(store: &Store, kb: &mut Kb, cfg: &VerifyConfig, outcome: &VerifiedSync, now_rfc3339: &str) -> Result<IndexOutcome, String> {
    match outcome {
        VerifiedSync::Accepted { head, .. } | VerifiedSync::UpToDate { head, .. } => {
            // The head the caller was told about must be the head this install accepted; anything else is a stale
            // or foreign result and indexes nothing.
            if store.accepted_head().map_err(|e| e.to_string())?.as_deref() != Some(head.as_str()) {
                return Ok(IndexOutcome::Skipped("not-the-accepted-head"));
            }
        }
        VerifiedSync::Unavailable(_) => {}
        VerifiedSync::Tampering { .. } => return Ok(IndexOutcome::Skipped("tampering")),
        VerifiedSync::Rejected { .. } => return Ok(IndexOutcome::Skipped("rejected")),
        VerifiedSync::NonFastForward(_) => return Ok(IndexOutcome::Skipped("non-fast-forward")),
        VerifiedSync::Refused(_) => return Ok(IndexOutcome::Skipped("refused")),
    }
    reindex(store, kb, cfg, now_rfc3339)
}

/// Derive the zone from the accepted state of `store` and apply it. Used after a sync and whenever the clock may have
/// moved a freshness statement (for example when `status` is shown); also how a zone emptied by `kb reindex` or lost
/// with `state.db` comes back (spec 012 test 17).
pub fn reindex(store: &Store, kb: &mut Kb, cfg: &VerifyConfig, now_rfc3339: &str) -> Result<IndexOutcome, String> {
    let d: Derived = xenia_index::derive(store, cfg).map_err(|e| e.to_string())?;
    let apply = kb.apply_xenia(&d.org, &d.store, &d.head, &d.memories, &d.held_back, now_rfc3339).map_err(|e| e.to_string())?;
    // After the zone: a crash between the two only means a longer walk next time.
    d.save_history(store).map_err(|e| e.to_string())?;
    Ok(IndexOutcome::Indexed {
        head: d.head.clone(),
        freshness: d.freshness_slug(),
        apply,
        in_force: d.memories.len(),
        held_back: d.held_back.len(),
        stamps: d.stamps(),
        sanitized: d.stripped.any(),
    })
}

/// The `provenance.stamp` events for a derivation, ready for a mission log.
pub fn stamp_events(mission_id: &str, stamps: &[Stamp]) -> Vec<EventEnvelope> {
    stamps.iter().map(|s| EventEnvelope::new(mission_id, "provenance.stamp", s.payload())).collect()
}

// ---------------------------------------------------------------------------------------------------------
// Reads

/// What an agent read returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRead {
    /// One spotlighted element per record, best first.
    pub envelopes: Vec<String>,
    pub ids: Vec<String>,
}

impl AgentRead {
    /// `L2` when anything was returned: the span's ceiling goes down to it (`specs/001` §5.2), exactly as for any other
    /// federated input. A search that returned nothing taints nothing.
    pub fn ceiling(&self) -> Option<Tier> {
        (!self.envelopes.is_empty()).then_some(Tier::L2)
    }

    pub fn observe(&self, span: &mut Span) {
        if let Some(t) = self.ceiling() {
            span.observe_input(t);
        }
    }

    pub fn render(&self) -> String {
        if self.envelopes.is_empty() {
            "no endorsed org records match".to_string()
        } else {
            self.envelopes.join("\n")
        }
    }
}

fn store_of(r: &XeniaRow) -> String {
    format!("{}/{}", r.org, r.store)
}

/// L2 summaries for `query`. `now_unix` is the caller's clock: an `L2` whose freshness statement has lapsed by then is
/// left out even if no sync has demoted it in the index yet.
pub fn agent_search(kb: &Kb, query: &str, limit: usize, now_unix: i64) -> Result<AgentRead, String> {
    let hits = kb.search(query, &SearchOpts::xenia_agent(limit)).map_err(|e| e.to_string())?;
    let mut paths: Vec<String> = Vec::new();
    for h in &hits {
        if !paths.contains(&h.rel_path) {
            paths.push(h.rel_path.clone());
        }
    }
    let nonce = fresh_nonce();
    let mut out = AgentRead { envelopes: Vec::new(), ids: Vec::new() };
    for r in kb.xenia_rows(&paths).map_err(|e| e.to_string())? {
        // The index filter already said L2 and active; the row and the clock say it again.
        let live = r.tier == "L2" && r.status == "active" && r.source_type == "federated_verified" && r.hidden.is_none() && r.fresh_until.is_some_and(|t| t > now_unix);
        if !live {
            continue;
        }
        out.envelopes.push(envelope(
            &nonce,
            &EnvelopeRecord { store: &store_of(&r), id: &r.id, rev: r.rev, tier: "L2", source_type: "federated_verified", title: &r.title, summary: &r.summary, tags: &r.tags, endorsed_by: &r.endorsed_by, note: "", body: "" },
        ));
        out.ids.push(r.id);
    }
    Ok(out)
}

/// What a quarantine read returns: entries with bodies, and the lowest tier among them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineRead {
    pub entries: Vec<String>,
    pub ids: Vec<String>,
    pub lowest: Option<Tier>,
}

impl QuarantineRead {
    pub fn observe(&self, span: &mut Span) {
        if let Some(t) = self.lowest {
            span.observe_input(t);
        }
    }
}

fn tier_of(r: &XeniaRow) -> Tier {
    match r.tier.as_str() {
        "L2" => Tier::L2,
        "L1" => Tier::L1,
        _ => Tier::L0,
    }
}

/// Why a record is in the quarantine read, as a fixed slug.
fn note_of(r: &XeniaRow, now_unix: i64) -> String {
    if let Some(h) = &r.hidden {
        return format!("hidden:{h}");
    }
    if r.tier == "L2" {
        // An L2 that lapsed since the index was derived.
        return if r.fresh_until.is_some_and(|t| t > now_unix) { "endorsed".into() } else { "stale-freshness".into() };
    }
    if !r.withheld_by_staleness.is_empty() {
        "stale-freshness".into()
    } else if r.clamped_by_ceiling {
        "ceiling-clamp".into()
    } else if r.tier == "L0" {
        "untrusted-source".into()
    } else {
        "unendorsed".into()
    }
}

fn entry(r: &XeniaRow, nonce: &str, now_unix: i64) -> String {
    // A record whose L2 lapsed is shown as the L1 it now is.
    let tier = if r.tier == "L2" && !r.fresh_until.is_some_and(|t| t > now_unix) { "L1" } else { r.tier.as_str() };
    let source = match tier {
        "L2" => "federated_verified",
        "L1" => "federated_peer",
        _ => "external_untrusted",
    };
    envelope(nonce, &EnvelopeRecord { store: &store_of(r), id: &r.id, rev: r.rev, tier, source_type: source, title: &r.title, summary: &r.summary, tags: &r.tags, endorsed_by: &r.endorsed_by, note: &note_of(r, now_unix), body: &r.body })
}

/// The explicit quarantine read by search: L1, L0 and hidden records matching `query`.
pub fn quarantine_search(kb: &Kb, query: &str, limit: usize, now_unix: i64) -> Result<QuarantineRead, String> {
    let hits = kb.search(query, &SearchOpts::xenia_quarantine(limit)).map_err(|e| e.to_string())?;
    let mut paths: Vec<String> = Vec::new();
    for h in &hits {
        if !paths.contains(&h.rel_path) {
            paths.push(h.rel_path.clone());
        }
    }
    let nonce = fresh_nonce();
    let rows = kb.xenia_rows(&paths).map_err(|e| e.to_string())?;
    Ok(read_of(&rows, &nonce, now_unix))
}

/// The explicit quarantine read of one record by id, whatever its tier or visibility (the operator asked for it).
pub fn quarantine_get(kb: &Kb, org: &str, store: &str, id: &str, now_unix: i64) -> Result<QuarantineRead, String> {
    let rows: Vec<XeniaRow> = kb.xenia_get(org, store, id).map_err(|e| e.to_string())?.into_iter().collect();
    Ok(read_of(&rows, &fresh_nonce(), now_unix))
}

fn read_of(rows: &[XeniaRow], nonce: &str, now_unix: i64) -> QuarantineRead {
    let mut lowest: Option<Tier> = None;
    let mut out = QuarantineRead { entries: Vec::new(), ids: Vec::new(), lowest: None };
    for r in rows {
        let t = if r.tier == "L2" && !r.fresh_until.is_some_and(|t| t > now_unix) { Tier::L1 } else { tier_of(r) };
        lowest = Some(lowest.map_or(t, |l| l.min(t)));
        out.entries.push(entry(r, nonce, now_unix));
        out.ids.push(r.id.clone());
    }
    out.lowest = lowest;
    out
}

#[cfg(test)]
#[path = "xenia_read_tests.rs"]
pub(crate) mod tests;
