//! The freshness statement (spec 011 §4.1, spec 012 §7): `roster/freshness.json` and `freshness.sigs.json`.
//!
//! A hosting attacker can serve an old view of a store. The statement is how an install learns what is
//! *current*: one `active` admin of the named epoch signs `{org, store, epoch, epoch_hash, store_head,
//! issued_at, expires_at}`. It confers no authority, only currency, so one signature is enough, except when it
//! carries `rewrite_of`, which asks installs to accept a rewritten history and therefore needs an admin
//! **quorum**, counted per person, like a roster epoch.
//!
//! Two questions are kept apart on purpose:
//!
//! * [`authenticate`]: is this statement *genuine for the roster this install verified*? Right store, the
//!   epoch is the chain head with the chain's own hash, and enough `active` admins of that epoch signed these
//!   exact bytes. Time plays no part. This is what makes `store_head` usable as a trust base for a history
//!   walk: only an admin of the **head** epoch can attest it, so a departed admin cannot sign a base for a
//!   fresh install.
//! * [`currency`]: is it current *by the install's clock*, under the **verifier's** own cap? "Validity is
//!   capped by the verifier, not chosen by the signer": a signer who writes `expires_at: 2099` gets a
//!   statement that is rejected, never one that lasts.
//!
//! Stale, missing or ignored means: tombstones and withdrawals keep their effect and **no endorsement counts**
//! (spec 011 §4.3); nothing is deleted. This module never reads a clock; `now` is Unix seconds from the caller.

use crate::roster::VerifiedRoster;
use crate::sticky::is_commit_id;
use crate::time::parse_utc;
use crate::{did, hash, is_slug, is_utc_timestamp, sig};
use serde::Deserialize;
use std::collections::BTreeSet;

pub const MAX_STATEMENT_BYTES: usize = 4 * 1024;
pub const MAX_SIGS_BYTES: usize = 64 * 1024;
const MAX_SIGNATURES: usize = 256;

/// Spec 011 §4.1: the default cap on `expires_at - issued_at`.
pub const DEFAULT_MAX_VALIDITY_SECS: i64 = 14 * 24 * 3600;
/// Spec 011 §4.1: `issued_at` more than this far in the future is rejected.
pub const FUTURE_SKEW_SECS: i64 = 5 * 60;

/// The install's local configuration. A store can never relax it (nothing in a store reaches this type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub max_validity_secs: i64,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy { max_validity_secs: DEFAULT_MAX_VALIDITY_SECS }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStatement {
    org: String,
    store: String,
    epoch: u64,
    epoch_hash: String,
    store_head: String,
    issued_at: String,
    expires_at: String,
    #[serde(default, deserialize_with = "present_or_absent")]
    rewrite_of: Option<String>,
}

/// `null` for an optional key is a second spelling of "absent".
fn present_or_absent<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    String::deserialize(d).map(Some)
}

/// A parsed statement. Parsing proves the shape only; see [`authenticate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub org: String,
    pub store: String,
    pub epoch: u64,
    pub epoch_hash: String,
    pub store_head: String,
    pub issued_at: String,
    pub expires_at: String,
    /// The head this statement replaces: present only for rewrite recovery (spec 011 §8.4).
    pub rewrite_of: Option<String>,
    pub issued: i64,
    pub expires: i64,
}

impl Statement {
    pub fn parse(bytes: &[u8]) -> Result<Statement, &'static str> {
        if bytes.len() > MAX_STATEMENT_BYTES {
            return Err("oversize");
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "not-utf8")?;
        let raw: RawStatement = serde_json::from_str(text).map_err(|_| "malformed")?;
        if !is_slug(&raw.org, 32) || !is_slug(&raw.store, 32) {
            return Err("malformed");
        }
        if raw.epoch == 0 || !hash::is_valid(&raw.epoch_hash) || !is_commit_id(&raw.store_head) {
            return Err("malformed");
        }
        if raw.rewrite_of.as_deref().is_some_and(|r| !is_commit_id(r)) {
            return Err("malformed");
        }
        if !is_utc_timestamp(&raw.issued_at) || !is_utc_timestamp(&raw.expires_at) {
            return Err("malformed");
        }
        let (Some(issued), Some(expires)) = (parse_utc(&raw.issued_at), parse_utc(&raw.expires_at)) else {
            return Err("malformed");
        };
        Ok(Statement {
            org: raw.org,
            store: raw.store,
            epoch: raw.epoch,
            epoch_hash: raw.epoch_hash,
            store_head: raw.store_head,
            issued_at: raw.issued_at,
            expires_at: raw.expires_at,
            rewrite_of: raw.rewrite_of,
            issued,
            expires,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSigs {
    signatures: Vec<RawSig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSig {
    kid: String,
    sig: String,
}

fn parse_sigs(bytes: &[u8]) -> Result<Vec<(String, String)>, &'static str> {
    if bytes.len() > MAX_SIGS_BYTES {
        return Err("sigs-oversize");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "sigs-malformed")?;
    let raw: RawSigs = serde_json::from_str(text).map_err(|_| "sigs-malformed")?;
    if raw.signatures.len() > MAX_SIGNATURES {
        return Err("sigs-malformed");
    }
    let mut out = Vec::new();
    for s in raw.signatures {
        if !did::is_valid(&s.kid) || sig::decode_signature(&s.sig).is_err() {
            return Err("sigs-malformed");
        }
        out.push((s.kid, s.sig));
    }
    Ok(out)
}

/// A statement that is genuine for the verified roster. Holding one says nothing about the clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authentic {
    pub statement: Statement,
    /// How many distinct people (admins of the named epoch) signed these exact bytes.
    pub signing_people: usize,
    /// The people needed: 1, or a majority of the epoch's admin people when `rewrite_of` is present.
    pub needed: usize,
}

/// Is this statement genuine for `roster`? `Err` carries the reason it is **ignored**; an ignored statement
/// is treated exactly like a missing one.
pub fn authenticate(org: &str, store: &str, statement_bytes: &[u8], sigs_bytes: &[u8], roster: &VerifiedRoster) -> Result<Authentic, &'static str> {
    let st = Statement::parse(statement_bytes)?;
    if st.org != org || st.store != store {
        return Err("wrong-scope");
    }
    let head = roster.head_epoch();
    // "A statement for an older epoch than the newest chain head the install knows is stale whatever its
    // expiry" (spec 011 §4.1). The chain verified against the pins, so `head` is at least the newest pin.
    if st.epoch < head {
        return Err("older-epoch");
    }
    if st.epoch > head {
        return Err("newer-than-chain");
    }
    let named = roster.epoch(st.epoch).ok_or("newer-than-chain")?;
    if named.hash != st.epoch_hash {
        return Err("epoch-hash-mismatch");
    }
    let sigs = parse_sigs(sigs_bytes)?;
    let domain = sig::Domain::freshness(org, store, &hash::of(statement_bytes)).map_err(|_| "wrong-scope")?;
    let admins = named.epoch.active_admins();
    let mut people = BTreeSet::new();
    for (kid, signature) in &sigs {
        // Only an `active` admin of the named epoch counts; a member, a retired or removed key, or a stranger
        // is ignored, as is a signature over any other bytes.
        let Some(person) = admins.get(kid.as_str()) else { continue };
        if sig::verify(kid, &domain, signature).is_ok() {
            people.insert(*person);
        }
    }
    let needed = if st.rewrite_of.is_some() { named.epoch.admin_people().len() / 2 + 1 } else { 1 };
    if people.is_empty() {
        return Err("no-admin-signature");
    }
    if people.len() < needed {
        return Err("rewrite-quorum-missing");
    }
    Ok(Authentic { signing_people: people.len(), needed, statement: st })
}

/// Is an authentic statement current at `now` under `policy`? `Err` is the reason it is not.
pub fn currency(st: &Statement, now: i64, policy: &Policy) -> Result<(), &'static str> {
    if st.issued > now.saturating_add(FUTURE_SKEW_SECS) {
        return Err("issued-in-future");
    }
    if st.expires <= st.issued {
        return Err("expires-before-issued");
    }
    if st.expires - st.issued > policy.max_validity_secs {
        return Err("validity-over-cap");
    }
    if st.expires > now.saturating_add(policy.max_validity_secs) {
        return Err("expires-too-far");
    }
    if st.expires <= now {
        return Err("expired");
    }
    Ok(())
}

/// What `status` shows and what the tier code reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreshnessState {
    Fresh { issued_at: String, expires_at: String, age_secs: i64, life_used_pct: u8 },
    /// No endorsement counts (spec 011 §4.3). `reason` is a fixed slug, never store text.
    Stale { reason: &'static str },
}

impl FreshnessState {
    /// Spec 011 §4.3: only a fresh statement lets endorsements count. Withdrawals and tombstones keep their
    /// effect either way.
    pub fn endorsements_count(&self) -> bool {
        matches!(self, FreshnessState::Fresh { .. })
    }

    /// `status` and `doctor` warn when 50% of the statement's life is used (spec 011 §4.1).
    pub fn warns(&self) -> bool {
        match self {
            FreshnessState::Fresh { life_used_pct, .. } => *life_used_pct >= 50,
            FreshnessState::Stale { .. } => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub state: FreshnessState,
    /// Present when the statement is genuine for the roster, current or not.
    pub authentic: Option<Authentic>,
}

/// The whole judgement for one statement, or its absence (`files` is `None` when either file is missing).
pub fn assess(org: &str, store: &str, files: Option<(&[u8], &[u8])>, roster: &VerifiedRoster, now: i64, policy: &Policy) -> Assessment {
    let Some((stmt, sigs)) = files else {
        return Assessment { state: FreshnessState::Stale { reason: "missing" }, authentic: None };
    };
    let auth = match authenticate(org, store, stmt, sigs, roster) {
        Ok(a) => a,
        Err(reason) => return Assessment { state: FreshnessState::Stale { reason }, authentic: None },
    };
    let st = &auth.statement;
    let state = match currency(st, now, policy) {
        Ok(()) => {
            let life = (st.expires - st.issued).max(1);
            let used = ((now - st.issued).clamp(0, life) * 100 / life) as u8;
            FreshnessState::Fresh { issued_at: st.issued_at.clone(), expires_at: st.expires_at.clone(), age_secs: (now - st.issued).max(0), life_used_pct: used }
        }
        Err(reason) => FreshnessState::Stale { reason },
    };
    Assessment { state, authentic: Some(auth) }
}

#[cfg(test)]
#[path = "freshness_tests.rs"]
mod tests;
