//! Sticky state (spec 011 §3.5): per store, per path under `records/` and `roster/`, the highest `rev`
//! seen and its hash, and the last head the install accepted. Pure: this module decides, the daemon
//! persists (`delphi-core::xenia_store`).
//!
//! The update rule never overwrites silently. For every observed path:
//! * unseen path: recorded;
//! * higher `rev`: supersedes (this is how a `restore` undoes a `withdraw`);
//! * same `rev`, same hash: unchanged (an idempotent re-sync);
//! * same `rev`, different hash: [`Refusal::Swapped`];
//! * lower `rev`: [`Refusal::Rollback`];
//! * a stored path that is not observed: [`Refusal::Deleted`].
//!
//! Callers feed only observations whose file **verified** (signature first, then `rev`): a hostile file that
//! failed verification must never be able to raise a path's stored `rev` and lock the legitimate one out.
//! A stored path whose file is present but no longer verifies is [`Refusal::Invalidated`]: the stored
//! version keeps its effect.

use crate::{hash, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How a path's versions are ordered. Records carry a `rev`; the files around them do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "rev", rename_all = "lowercase")]
pub enum Tracking {
    /// A record with a monotonic `rev` (>= 1).
    Rev(u64),
    /// Never changes once seen (a roster epoch file): any other hash is equivocation.
    Immutable,
    /// Must stay present, content may change (sidecars, signature files, the freshness statement, whose
    /// own validity is X4b's job).
    Present,
}

impl Tracking {
    fn same_family(self, other: Tracking) -> bool {
        matches!((self, other), (Tracking::Rev(_), Tracking::Rev(_)) | (Tracking::Immutable, Tracking::Immutable) | (Tracking::Present, Tracking::Present))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(flatten)]
    pub tracking: Tracking,
    /// `blake3:<64 hex>` of the file's raw bytes.
    pub hash: String,
}

/// One verified file seen at the fetched head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub path: String,
    pub tracking: Tracking,
    pub hash: String,
}

/// Tampering found by the update rule. None of these is ever applied: the stored version keeps its effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A path seen before is gone (spec 011 §3.5, T4).
    Deleted { path: String },
    /// The path is there but its file no longer verifies.
    Invalidated { path: String },
    /// A lower `rev` than stored (a revert, T21).
    Rollback { path: String, stored: u64, seen: u64 },
    /// The same `rev` with a different hash (a swap), or an immutable file changed.
    Swapped { path: String },
    /// The path is now ordered differently than before (for example a record file became a signature file).
    TrackingChanged { path: String },
}

impl Refusal {
    pub fn path(&self) -> &str {
        match self {
            Refusal::Deleted { path } | Refusal::Invalidated { path } | Refusal::Rollback { path, .. } | Refusal::Swapped { path } | Refusal::TrackingChanged { path } => path,
        }
    }

    /// The slug `delphi xenia status` shows; contains no attacker text.
    pub fn reason(&self) -> &'static str {
        match self {
            Refusal::Deleted { .. } => "deleted",
            Refusal::Invalidated { .. } => "no-longer-verifies",
            Refusal::Rollback { .. } => "rev-rollback",
            Refusal::Swapped { .. } => "same-rev-different-hash",
            Refusal::TrackingChanged { .. } => "tracking-changed",
        }
    }
}

/// What an update would do, computed without changing anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Evaluation {
    pub refusals: Vec<Refusal>,
    pub added: Vec<(String, Entry)>,
    pub superseded: Vec<(String, Entry)>,
    pub unchanged: usize,
}

impl Evaluation {
    pub fn is_clean(&self) -> bool {
        self.refusals.is_empty()
    }

    pub fn changes_anything(&self) -> bool {
        !self.added.is_empty() || !self.superseded.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StickyState {
    /// Format version; a state file from a newer format is refused rather than half-understood.
    pub v: u32,
    /// The store this state belongs to, checked on load so one store's file cannot stand in for another's.
    pub org: String,
    pub store: String,
    /// The last git commit the install accepted (hex). Install-owned state, kept here and not in the clone.
    pub accepted_head: Option<String>,
    pub paths: BTreeMap<String, Entry>,
}

const VERSION: u32 = 1;

impl StickyState {
    pub fn new(org: &str, store: &str) -> StickyState {
        StickyState { v: VERSION, org: org.to_string(), store: store.to_string(), accepted_head: None, paths: BTreeMap::new() }
    }

    /// Strict parse, bound to the expected store. Any damage is an error: a damaged sticky file is never
    /// treated as "no state", which would let an attacker reset it by corrupting it.
    pub fn from_json(bytes: &[u8], org: &str, store: &str) -> Result<StickyState> {
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(Error::Oversize("sticky state over 16 MiB"));
        }
        let s: StickyState = serde_json::from_slice(bytes).map_err(|_| Error::BadField("sticky state is damaged"))?;
        if s.v != VERSION {
            return Err(Error::BadField("sticky state has an unknown version"));
        }
        if s.org != org || s.store != store {
            return Err(Error::BadField("sticky state belongs to another store"));
        }
        if let Some(h) = &s.accepted_head {
            if !is_commit_id(h) {
                return Err(Error::BadField("sticky accepted_head is not a commit id"));
            }
        }
        for (p, e) in &s.paths {
            if p.is_empty() || !hash::is_valid(&e.hash) {
                return Err(Error::BadField("sticky entry is damaged"));
            }
            if matches!(e.tracking, Tracking::Rev(0)) {
                return Err(Error::BadField("sticky rev is 0"));
            }
        }
        Ok(s)
    }

    /// Deterministic bytes (sorted paths), so an unchanged state serialises identically.
    pub fn to_json(&self) -> Vec<u8> {
        let mut v = serde_json::to_vec_pretty(self).expect("sticky state serialises");
        v.push(b'\n');
        v
    }

    /// Apply the update rule to everything seen at a fetched head. `invalid` lists paths that are present
    /// but did not verify. Nothing is changed; see [`StickyState::apply`].
    pub fn evaluate(&self, observed: &[Observation], invalid: &[String]) -> Evaluation {
        let mut ev = Evaluation::default();
        let mut seen: BTreeMap<&str, &Observation> = BTreeMap::new();
        for o in observed {
            // A path listed twice would let the later entry win silently.
            if seen.insert(o.path.as_str(), o).is_some() {
                ev.refusals.push(Refusal::Swapped { path: o.path.clone() });
            }
        }
        for o in observed {
            let new = Entry { tracking: o.tracking, hash: o.hash.clone() };
            let Some(old) = self.paths.get(&o.path) else {
                ev.added.push((o.path.clone(), new));
                continue;
            };
            if !old.tracking.same_family(o.tracking) {
                ev.refusals.push(Refusal::TrackingChanged { path: o.path.clone() });
                continue;
            }
            match (old.tracking, o.tracking) {
                (Tracking::Rev(stored), Tracking::Rev(seen)) => {
                    if seen > stored {
                        ev.superseded.push((o.path.clone(), new));
                    } else if seen < stored {
                        ev.refusals.push(Refusal::Rollback { path: o.path.clone(), stored, seen });
                    } else if old.hash == o.hash {
                        ev.unchanged += 1;
                    } else {
                        ev.refusals.push(Refusal::Swapped { path: o.path.clone() });
                    }
                }
                (Tracking::Immutable, _) => {
                    if old.hash == o.hash {
                        ev.unchanged += 1;
                    } else {
                        ev.refusals.push(Refusal::Swapped { path: o.path.clone() });
                    }
                }
                _ => {
                    if old.hash == o.hash {
                        ev.unchanged += 1;
                    } else {
                        ev.superseded.push((o.path.clone(), new));
                    }
                }
            }
        }
        for p in invalid {
            if self.paths.contains_key(p) && !seen.contains_key(p.as_str()) {
                ev.refusals.push(Refusal::Invalidated { path: p.clone() });
            }
        }
        for p in self.paths.keys() {
            if !seen.contains_key(p.as_str()) && !invalid.contains(p) {
                ev.refusals.push(Refusal::Deleted { path: p.clone() });
            }
        }
        ev
    }

    /// Apply a clean evaluation. A dirty one is never applied, so tampering cannot half-update the state.
    pub fn apply(&mut self, ev: &Evaluation) -> Result<()> {
        if !ev.is_clean() {
            return Err(Error::BadField("refusing to apply an evaluation that found tampering"));
        }
        for (p, e) in ev.added.iter().chain(ev.superseded.iter()) {
            self.paths.insert(p.clone(), e.clone());
        }
        Ok(())
    }
}

/// A git commit id: 40 hex (SHA-1) or 64 hex (SHA-256), lowercase.
pub fn is_commit_id(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(n: u8) -> String {
        hash::of(&[n])
    }

    fn obs(path: &str, rev: u64, n: u8) -> Observation {
        Observation { path: path.into(), tracking: Tracking::Rev(rev), hash: h(n) }
    }

    fn state_with(path: &str, rev: u64, n: u8) -> StickyState {
        let mut s = StickyState::new("acme", "main");
        let ev = s.evaluate(&[obs(path, rev, n)], &[]);
        s.apply(&ev).unwrap();
        s
    }

    #[test]
    fn a_new_path_is_recorded_and_a_second_look_changes_nothing() {
        let mut s = StickyState::new("acme", "main");
        let ev = s.evaluate(&[obs("records/memory/A.md", 1, 1)], &[]);
        assert!(ev.is_clean() && ev.changes_anything());
        s.apply(&ev).unwrap();
        let before = s.to_json();
        let again = s.evaluate(&[obs("records/memory/A.md", 1, 1)], &[]);
        assert!(again.is_clean() && !again.changes_anything());
        assert_eq!(again.unchanged, 1);
        s.apply(&again).unwrap();
        assert_eq!(s.to_json(), before);
    }

    #[test]
    fn a_higher_rev_supersedes() {
        let mut s = state_with("records/tombstone/T.md", 1, 1);
        let ev = s.evaluate(&[obs("records/tombstone/T.md", 2, 2)], &[]);
        assert!(ev.is_clean());
        s.apply(&ev).unwrap();
        assert_eq!(s.paths["records/tombstone/T.md"].tracking, Tracking::Rev(2));
    }

    #[test]
    fn a_lower_rev_is_a_rollback_and_the_stored_entry_survives() {
        let mut s = state_with("records/endorsement/E.md", 2, 2);
        let ev = s.evaluate(&[obs("records/endorsement/E.md", 1, 1)], &[]);
        assert_eq!(ev.refusals, vec![Refusal::Rollback { path: "records/endorsement/E.md".into(), stored: 2, seen: 1 }]);
        let before = s.clone();
        assert!(s.apply(&ev).is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn the_same_rev_with_another_hash_is_a_swap() {
        let s = state_with("records/memory/A.md", 3, 1);
        let ev = s.evaluate(&[obs("records/memory/A.md", 3, 9)], &[]);
        assert_eq!(ev.refusals[0].reason(), "same-rev-different-hash");
    }

    #[test]
    fn a_missing_path_is_deleted_tampering_unless_it_is_merely_invalid() {
        let s = state_with("records/memory/A.md", 1, 1);
        let ev = s.evaluate(&[], &[]);
        assert_eq!(ev.refusals, vec![Refusal::Deleted { path: "records/memory/A.md".into() }]);
        let ev = s.evaluate(&[], &["records/memory/A.md".to_string()]);
        assert_eq!(ev.refusals, vec![Refusal::Invalidated { path: "records/memory/A.md".into() }]);
    }

    #[test]
    fn an_invalid_file_at_an_unseen_path_is_not_tampering_and_records_nothing() {
        let s = StickyState::new("acme", "main");
        let ev = s.evaluate(&[], &["records/memory/Z.md".to_string()]);
        assert!(ev.is_clean() && !ev.changes_anything());
    }

    #[test]
    fn immutable_files_never_change_and_present_files_may() {
        let mut s = StickyState::new("acme", "main");
        let imm = |n| Observation { path: "roster/epochs/1.json".into(), tracking: Tracking::Immutable, hash: h(n) };
        let pre = |n| Observation { path: "roster/freshness.json".into(), tracking: Tracking::Present, hash: h(n) };
        let ev = s.evaluate(&[imm(1), pre(1)], &[]);
        s.apply(&ev).unwrap();
        let ev = s.evaluate(&[imm(1), pre(2)], &[]);
        assert!(ev.is_clean());
        assert_eq!(ev.superseded.len(), 1);
        let ev = s.evaluate(&[imm(2), pre(1)], &[]);
        assert_eq!(ev.refusals, vec![Refusal::Swapped { path: "roster/epochs/1.json".into() }]);
    }

    #[test]
    fn a_path_changing_family_is_refused() {
        let s = state_with("records/memory/A.md", 1, 1);
        let o = Observation { path: "records/memory/A.md".into(), tracking: Tracking::Present, hash: h(1) };
        assert_eq!(s.evaluate(&[o], &[]).refusals[0].reason(), "tracking-changed");
    }

    #[test]
    fn a_duplicated_observation_is_refused() {
        let s = StickyState::new("acme", "main");
        let ev = s.evaluate(&[obs("records/memory/A.md", 1, 1), obs("records/memory/A.md", 9, 2)], &[]);
        assert!(!ev.is_clean());
    }

    #[test]
    fn the_file_round_trips_and_every_kind_of_damage_is_an_error_not_an_empty_state() {
        let mut s = state_with("records/memory/A.md", 4, 1);
        s.accepted_head = Some("a".repeat(40));
        let bytes = s.to_json();
        assert_eq!(StickyState::from_json(&bytes, "acme", "main").unwrap(), s);
        assert!(StickyState::from_json(&bytes, "acme", "other").is_err());
        assert!(StickyState::from_json(&bytes, "evil", "main").is_err());
        assert!(StickyState::from_json(b"", "acme", "main").is_err());
        assert!(StickyState::from_json(b"{}", "acme", "main").is_err());
        let text = String::from_utf8(bytes).unwrap();
        for bad in [text.replace("\"v\": 1", "\"v\": 2"), text.replace(&"a".repeat(40), "xyz"), text.replace("\"rev\": 4", "\"rev\": 0"), text.replace("\"paths\"", "\"extra\": 1,\n  \"paths\"")] {
            assert!(StickyState::from_json(bad.as_bytes(), "acme", "main").is_err(), "{bad}");
        }
    }
}
