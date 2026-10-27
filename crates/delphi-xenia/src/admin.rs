//! Admin operations on the roster, as pure functions (spec 011 §3.2, §8; plan milestone X8): drafting the next
//! epoch file from a verified one, describing what a draft changes, and counting signatures.
//!
//! Nothing here reads a file, a clock or the network, and nothing holds a key: [`sign_epoch`] takes an
//! [`Identity`] by reference for one call and returns the signature. Git, branches and the operator's approval are
//! the caller's (`delphi-core/src/xenia_admin.rs`).
//!
//! * **A draft is checked, not trusted.** Every draft is serialized in one canonical spelling, then parsed back with
//!   the verifier's own [`Epoch::parse`], compared with what was meant and put through [`check_transition`]
//!   against the epoch it follows. A draft that does not survive is an error, never a file.
//! * **A rotation names its reason.** [`Rotation`] has no default and no `Option` form: routine or compromised.
//!   A compromised rotation sets `voids_tombstones` on the old key (spec 011 §8.2, mandatory), and no other path
//!   can set it on a rotation.
//! * **Every change is described to the signer.** [`describe`] compares a proposal with the epoch it follows and
//!   reports *every* difference, including ones the draft functions here never make (a role granted to an active
//!   member, a changed handle): a proposal is attacker-controlled bytes until an admin has read this list, so
//!   anything outside the three operations is a [`Warning`] the signer must acknowledge.
//! * **Signatures count only over the exact bytes** and only from active admins of the previous epoch, once per
//!   person (the same rule [`crate::roster::verify_chain`] applies); [`merge_sigs`] drops everything else.

use crate::hash;
use crate::identity::Identity;
use crate::roster::{check_transition, Areas, Epoch, EpochSigs, Member, RResult, Role, RosterError, Status, VerifiedEpoch};
use crate::sig::{self, Domain};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Why a rotation is happening. No default: the caller must say (spec 011 §8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// The key is fine; the person is moving to a new one. The member re-signs their own records afterwards.
    Routine,
    /// The key may be in someone else's hands. Forces `voids_tombstones` on the old entry.
    Compromised,
}

impl Rotation {
    /// The two command-line flags, exactly one of which must be given.
    pub fn from_flags(routine: bool, compromised: bool) -> Result<Rotation, AdminError> {
        match (routine, compromised) {
            (true, false) => Ok(Rotation::Routine),
            (false, true) => Ok(Rotation::Compromised),
            (false, false) => Err(AdminError::RotationReasonMissing),
            (true, true) => Err(AdminError::RotationReasonConflict),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMember {
    pub did: String,
    pub person: String,
    pub handle: String,
    pub admin: bool,
    /// Steward areas: empty (not a steward), tags, or exactly `["*"]`.
    pub steward_areas: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Add(NewMember),
    /// Mark an entry `removed`. `voids_tombstones` is for cause (spec 011 §8.3).
    Remove { did: String, voids_tombstones: bool },
    Rotate { old: String, new: String, kind: Rotation },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminError {
    RotationReasonMissing,
    RotationReasonConflict,
    UnknownMember,
    AlreadyListed,
    NotActive,
    AlreadyRemoved,
    SameKey,
    /// The draft broke a roster rule; the slug is [`RosterError::reason`].
    Roster(&'static str),
    /// The canonical file did not parse back to the draft: a bug, refused rather than written.
    NotCanonical,
    /// A proposal for the wrong place in the chain.
    WrongEpoch { expected: u64, found: u64 },
    WrongPrevHash,
    WrongScope,
}

impl AdminError {
    pub fn reason(&self) -> &'static str {
        match self {
            AdminError::RotationReasonMissing => "rotation-reason-missing",
            AdminError::RotationReasonConflict => "rotation-reason-conflict",
            AdminError::UnknownMember => "unknown-member",
            AdminError::AlreadyListed => "already-listed",
            AdminError::NotActive => "not-active",
            AdminError::AlreadyRemoved => "already-removed",
            AdminError::SameKey => "same-key",
            AdminError::Roster(r) => r,
            AdminError::NotCanonical => "not-canonical",
            AdminError::WrongEpoch { .. } => "wrong-epoch",
            AdminError::WrongPrevHash => "wrong-prev-hash",
            AdminError::WrongScope => "wrong-scope",
        }
    }
}

impl std::fmt::Display for AdminError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminError::RotationReasonMissing => f.write_str("rotate needs --routine or --compromised; there is no default"),
            AdminError::RotationReasonConflict => f.write_str("rotate takes --routine or --compromised, not both"),
            AdminError::UnknownMember => f.write_str("no such member in the roster"),
            AdminError::AlreadyListed => f.write_str("that DID is already listed in the roster (entries are never dropped or reused)"),
            AdminError::NotActive => f.write_str("that member is not active"),
            AdminError::AlreadyRemoved => f.write_str("that member is already removed"),
            AdminError::SameKey => f.write_str("the new key must differ from the old one"),
            AdminError::Roster(r) => write!(f, "the draft breaks a roster rule ({r})"),
            AdminError::NotCanonical => f.write_str("the draft did not read back as written; refusing to write it"),
            AdminError::WrongEpoch { expected, found } => write!(f, "the proposal is epoch {found}, expected {expected}"),
            AdminError::WrongPrevHash => f.write_str("the proposal does not follow the current head epoch (prev_hash differs)"),
            AdminError::WrongScope => f.write_str("the proposal is for another org or store"),
        }
    }
}

impl std::error::Error for AdminError {}

fn roster_err(e: RosterError) -> AdminError {
    AdminError::Roster(e.reason())
}

// ---------------------------------------------------------------------------------------------------------
// Canonical spelling

fn member_json(m: &Member) -> Value {
    let mut v = json!({
        "did": m.did,
        "person": m.person,
        "handle": m.handle,
        "roles": m.roles.iter().map(|r| match r { Role::Member => "member", Role::Admin => "admin" }).collect::<Vec<_>>(),
        "steward_areas": match &m.steward_areas { Areas::All => vec!["*".to_string()], Areas::Tags(t) => t.clone() },
        "added_at": m.added_at,
        "status": m.status.as_str(),
    });
    let o = v.as_object_mut().expect("an object");
    if let Some(s) = &m.successor {
        o.insert("successor".into(), json!(s));
    }
    if let Some(n) = m.removed_at_epoch {
        o.insert("removed_at_epoch".into(), json!(n));
    }
    if m.voids_tombstones {
        o.insert("voids_tombstones".into(), json!(true));
    }
    v
}

/// The one spelling of an epoch file this crate writes: pretty JSON with sorted keys and a final newline.
pub fn serialize_epoch(e: &Epoch) -> Vec<u8> {
    let v = json!({
        "org": e.org, "store": e.store, "epoch": e.epoch, "prev_hash": e.prev_hash,
        "members": e.members.iter().map(member_json).collect::<Vec<_>>(),
    });
    let mut s = serde_json::to_string_pretty(&v).expect("a JSON value serializes");
    s.push('\n');
    s.into_bytes()
}

/// A draft epoch: the parsed form, its exact bytes and their hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub epoch: Epoch,
    pub bytes: Vec<u8>,
    pub hash: String,
}

fn seal(prev: &VerifiedEpoch, next: Epoch) -> Result<Draft, AdminError> {
    let bytes = serialize_epoch(&next);
    let back = Epoch::parse(&bytes).map_err(roster_err)?;
    if back != next {
        return Err(AdminError::NotCanonical);
    }
    check_transition(&prev.epoch, &back).map_err(roster_err)?;
    let hash = hash::of(&bytes);
    Ok(Draft { epoch: back, bytes, hash })
}

/// Draft epoch `prev.epoch + 1` from the verified `prev`, applying one operation. `now` is a UTC timestamp for
/// `added_at` (the caller's clock; this module has none).
pub fn draft(prev: &VerifiedEpoch, op: &Op, now: &str) -> Result<Draft, AdminError> {
    let n = prev.epoch.epoch + 1;
    let mut next = Epoch { org: prev.epoch.org.clone(), store: prev.epoch.store.clone(), epoch: n, prev_hash: Some(prev.hash.clone()), members: prev.epoch.members.clone() };
    match op {
        Op::Add(m) => {
            if prev.epoch.member(&m.did).is_some() {
                return Err(AdminError::AlreadyListed);
            }
            let roles = if m.admin { vec![Role::Member, Role::Admin] } else { vec![Role::Member] };
            let areas = if m.steward_areas.iter().any(|a| a == "*") { Areas::All } else { Areas::Tags(m.steward_areas.clone()) };
            next.members.push(Member {
                did: m.did.clone(),
                person: m.person.clone(),
                handle: m.handle.clone(),
                roles,
                steward_areas: areas,
                added_at: now.to_string(),
                status: Status::Active,
                successor: None,
                removed_at_epoch: None,
                voids_tombstones: false,
            });
        }
        Op::Remove { did, voids_tombstones } => {
            let m = next.members.iter_mut().find(|m| &m.did == did).ok_or(AdminError::UnknownMember)?;
            if m.status == Status::Removed {
                return Err(AdminError::AlreadyRemoved);
            }
            m.status = Status::Removed;
            m.removed_at_epoch = Some(n);
            m.voids_tombstones |= *voids_tombstones;
        }
        Op::Rotate { old, new, kind } => {
            if old == new {
                return Err(AdminError::SameKey);
            }
            if prev.epoch.member(new).is_some() {
                return Err(AdminError::AlreadyListed);
            }
            let m = next.members.iter_mut().find(|m| &m.did == old).ok_or(AdminError::UnknownMember)?;
            if m.status != Status::Active {
                return Err(AdminError::NotActive);
            }
            m.status = Status::Retired;
            m.successor = Some(new.clone());
            if *kind == Rotation::Compromised {
                m.voids_tombstones = true;
            }
            let mut fresh = m.clone();
            fresh.did = new.clone();
            fresh.status = Status::Active;
            fresh.successor = None;
            fresh.voids_tombstones = false;
            fresh.added_at = now.to_string();
            next.members.push(fresh);
        }
    }
    seal(prev, next)
}

/// Genesis (`1.json`, spec 011 §3.2): the founding members, all `active`, no previous epoch. It is valid only if
/// **every** founding admin signs it, so this draft is checked by the verifier's own [`Epoch::parse`] and
/// [`verify_genesis`] once the signatures are in. `founders` must list at least one admin.
pub fn genesis(org: &str, store: &str, founders: &[NewMember], now: &str) -> Result<Draft, AdminError> {
    let mut members = Vec::new();
    for m in founders {
        members.push(Member {
            did: m.did.clone(),
            person: m.person.clone(),
            handle: m.handle.clone(),
            roles: if m.admin { vec![Role::Member, Role::Admin] } else { vec![Role::Member] },
            steward_areas: if m.steward_areas.iter().any(|a| a == "*") { Areas::All } else { Areas::Tags(m.steward_areas.clone()) },
            added_at: now.to_string(),
            status: Status::Active,
            successor: None,
            removed_at_epoch: None,
            voids_tombstones: false,
        });
    }
    let e = Epoch { org: org.to_string(), store: store.to_string(), epoch: 1, prev_hash: None, members };
    let bytes = serialize_epoch(&e);
    let back = Epoch::parse(&bytes).map_err(roster_err)?;
    if back != e {
        return Err(AdminError::NotCanonical);
    }
    if back.admin_people().is_empty() {
        return Err(AdminError::Roster("no-active-admin"));
    }
    let hash = hash::of(&bytes);
    Ok(Draft { epoch: back, bytes, hash })
}

/// The chain `[genesis]` verified with every founding admin's signature required, anchored on the genesis hash
/// itself (the founders are the root of trust; nobody can pin anything older).
pub fn verify_genesis(org: &str, store: &str, bytes: &[u8], sigs: &[u8]) -> RResult<crate::roster::VerifiedRoster> {
    let anchor = crate::roster::Anchor::new([(1u64, hash::of(bytes))])?;
    crate::roster::verify_chain(org, store, &[crate::roster::EpochFile { epoch_json: bytes.to_vec(), sigs_json: sigs.to_vec() }], &anchor)
}

/// A genesis signatures file from the founders' `(kid, signature)` pairs, sorted by `kid`, repeats dropped. Whether
/// they count is [`verify_genesis`]'s decision.
pub fn genesis_sigs(sigs: &[(String, String)]) -> Vec<u8> {
    let map: BTreeMap<&str, &str> = sigs.iter().map(|(k, s)| (k.as_str(), s.as_str())).collect();
    let list: Vec<Value> = map.iter().map(|(k, s)| json!({ "kid": k, "sig": s })).collect();
    let mut out = serde_json::to_string_pretty(&json!({ "epoch": 1, "signatures": list })).expect("a JSON value serializes");
    out.push('\n');
    out.into_bytes()
}

/// Check bytes somebody else wrote as the epoch after `prev`: right scope, right number, right `prev_hash`, only
/// moves [`check_transition`] allows. Does not look at signatures.
pub fn check_proposal(prev: &VerifiedEpoch, bytes: &[u8]) -> Result<Draft, AdminError> {
    let e = Epoch::parse(bytes).map_err(roster_err)?;
    if e.org != prev.epoch.org || e.store != prev.epoch.store {
        return Err(AdminError::WrongScope);
    }
    let expected = prev.epoch.epoch + 1;
    if e.epoch != expected {
        return Err(AdminError::WrongEpoch { expected, found: e.epoch });
    }
    if e.prev_hash.as_deref() != Some(prev.hash.as_str()) {
        return Err(AdminError::WrongPrevHash);
    }
    check_transition(&prev.epoch, &e).map_err(roster_err)?;
    Ok(Draft { epoch: e, bytes: bytes.to_vec(), hash: hash::of(bytes) })
}

// ---------------------------------------------------------------------------------------------------------
// Describing a change

/// One difference between a proposal and the epoch it follows, in words an operator can check. Handles and
/// people are attacker-chosen text with a small alphabet (the roster parser enforces it), so they are safe to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added { did: String, person: String, handle: String, admin: bool, steward_areas: Vec<String> },
    Retired { did: String, handle: String, successor: Option<String>, voids_tombstones: bool },
    Removed { did: String, handle: String, voids_tombstones: bool },
    /// Something about an existing entry changed that none of add, remove or rotate does.
    Modified { did: String, handle: String, what: &'static str },
}

impl Change {
    pub fn line(&self) -> String {
        match self {
            Change::Added { did, person, handle, admin, steward_areas } => {
                let role = if *admin { "ADMIN" } else { "member" };
                let st = if steward_areas.is_empty() { String::new() } else { format!(", steward of {}", steward_areas.join(",")) };
                format!("add {handle} ({person}) as {role}{st}: {did}")
            }
            Change::Retired { did, handle, successor, voids_tombstones } => {
                let to = successor.as_deref().map(|s| format!(" -> successor {s}")).unwrap_or_else(|| " (no successor)".into());
                let v = if *voids_tombstones { " [VOIDS every tombstone this key signed]" } else { "" };
                format!("retire {handle} ({did}){to}{v}")
            }
            Change::Removed { did, handle, voids_tombstones } => {
                let v = if *voids_tombstones { " [VOIDS every tombstone this key signed]" } else { "" };
                format!("remove {handle} ({did}){v}")
            }
            Change::Modified { did, handle, what } => format!("MODIFIES existing entry {handle} ({did}): {what}"),
        }
    }
}

/// A reason to look twice. A signature over a proposal with warnings needs the signer to say they read them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    GrantsAdmin(String),
    ModifiesExisting(String),
    /// An added DID belongs to a person who already has another DID.
    SecondKeyForPerson(String),
    /// Quorum needed for the *next* epoch changes (the number of admin people changes).
    AdminCountChanges { from: usize, to: usize },
    /// Fewer than three admin people: below the recommended 2-of-3 (spec 011 §3.2).
    BelowThreeAdmins(usize),
    /// A retirement with no successor: nobody inherits the member's records.
    RetiredWithoutSuccessor(String),
}

impl Warning {
    pub fn line(&self) -> String {
        match self {
            Warning::GrantsAdmin(h) => format!("GRANTS ADMIN to {h}: an admin can sign roster changes"),
            Warning::ModifiesExisting(h) => format!("changes an existing entry ({h}) in a way add/remove/rotate never does"),
            Warning::SecondKeyForPerson(p) => format!("{p} already has another key in the roster; this adds a second"),
            Warning::AdminCountChanges { from, to } => format!("admin people go from {from} to {to}; the next epoch needs {} signatures", to / 2 + 1),
            Warning::BelowThreeAdmins(n) => format!("only {n} admin people after this change; 2-of-3 is recommended"),
            Warning::RetiredWithoutSuccessor(h) => format!("{h} is retired with no successor"),
        }
    }
}

/// Everything that differs between `prev` and `next`, plus the warnings. Entries of `prev` that `next` lacks are
/// reported as `Modified("dropped")` (a valid proposal never has one; the roster check refuses it separately).
pub fn describe(prev: &Epoch, next: &Epoch) -> (Vec<Change>, Vec<Warning>) {
    let old: BTreeMap<&str, &Member> = prev.members.iter().map(|m| (m.did.as_str(), m)).collect();
    let (mut changes, mut warnings) = (Vec::new(), Vec::new());
    let people_before: BTreeSet<&str> = prev.members.iter().map(|m| m.person.as_str()).collect();
    for m in &next.members {
        match old.get(m.did.as_str()) {
            None => {
                changes.push(Change::Added {
                    did: m.did.clone(),
                    person: m.person.clone(),
                    handle: m.handle.clone(),
                    admin: m.is_admin(),
                    steward_areas: match &m.steward_areas {
                        Areas::All => vec!["*".into()],
                        Areas::Tags(t) => t.clone(),
                    },
                });
                // A rotation's new key shares the person of the key it replaces; only flag a person who stays.
                let replaces = next.members.iter().any(|o| o.successor.as_deref() == Some(m.did.as_str()));
                if m.is_admin() {
                    warnings.push(Warning::GrantsAdmin(m.handle.clone()));
                }
                if people_before.contains(m.person.as_str()) && !replaces {
                    warnings.push(Warning::SecondKeyForPerson(m.person.clone()));
                }
            }
            Some(o) => {
                if o.status != m.status {
                    match m.status {
                        Status::Retired => {
                            changes.push(Change::Retired { did: m.did.clone(), handle: m.handle.clone(), successor: m.successor.clone(), voids_tombstones: m.voids_tombstones });
                            if m.successor.is_none() {
                                warnings.push(Warning::RetiredWithoutSuccessor(m.handle.clone()));
                            }
                        }
                        Status::Removed => changes.push(Change::Removed { did: m.did.clone(), handle: m.handle.clone(), voids_tombstones: m.voids_tombstones }),
                        Status::Active => changes.push(Change::Modified { did: m.did.clone(), handle: m.handle.clone(), what: "status back to active" }),
                    }
                } else if o.voids_tombstones != m.voids_tombstones {
                    changes.push(Change::Modified { did: m.did.clone(), handle: m.handle.clone(), what: "voids_tombstones" });
                }
                for (differs, what) in [(o.handle != m.handle, "handle"), (o.roles != m.roles, "roles"), (o.steward_areas != m.steward_areas, "steward_areas"), (o.person != m.person, "person"), (o.successor != m.successor && o.status == m.status, "successor")] {
                    if differs {
                        changes.push(Change::Modified { did: m.did.clone(), handle: m.handle.clone(), what });
                    }
                }
                if !o.is_admin() && m.is_admin() {
                    warnings.push(Warning::GrantsAdmin(m.handle.clone()));
                }
                if m.status == o.status && (o.roles != m.roles || o.steward_areas != m.steward_areas || o.handle != m.handle || o.person != m.person) {
                    warnings.push(Warning::ModifiesExisting(m.handle.clone()));
                }
            }
        }
    }
    let kept: BTreeSet<&str> = next.members.iter().map(|m| m.did.as_str()).collect();
    for o in &prev.members {
        if !kept.contains(o.did.as_str()) {
            changes.push(Change::Modified { did: o.did.clone(), handle: o.handle.clone(), what: "dropped" });
            warnings.push(Warning::ModifiesExisting(o.handle.clone()));
        }
    }
    let (from, to) = (prev.admin_people().len(), next.admin_people().len());
    if from != to {
        warnings.push(Warning::AdminCountChanges { from, to });
    }
    if to < 3 {
        warnings.push(Warning::BelowThreeAdmins(to));
    }
    (changes, warnings)
}

// ---------------------------------------------------------------------------------------------------------
// Signatures

/// How many people have signed these exact bytes, out of how many the next epoch needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quorum {
    pub have: usize,
    pub need: usize,
    /// Handles (from `prev`) of the people whose signatures count.
    pub signed: Vec<String>,
    /// Signatures in the file that count for nothing: wrong bytes, wrong epoch, not an admin of `prev`, repeats.
    pub ignored: usize,
}

impl Quorum {
    pub fn met(&self) -> bool {
        self.have >= self.need
    }
}

fn domain(org: &str, store: &str, n: u64, bytes: &[u8]) -> Domain {
    Domain::roster(org, store, n, &hash::of(bytes)).expect("org and store were validated by the roster parser")
}

/// Count the signatures in `sigs_bytes` over `bytes` (epoch `n` of `org`/`store`) that `prev`'s admins made.
pub fn quorum(prev: &Epoch, n: u64, bytes: &[u8], sigs_bytes: &[u8]) -> RResult<Quorum> {
    let sigs = EpochSigs::parse(sigs_bytes)?;
    let dom = domain(&prev.org, &prev.store, n, bytes);
    let approvers = prev.active_admins();
    let (mut people, mut dids) = (BTreeSet::new(), BTreeSet::new());
    let mut ignored = 0;
    if sigs.epoch != n {
        return Ok(Quorum { have: 0, need: prev.admin_people().len() / 2 + 1, signed: Vec::new(), ignored: sigs.signatures.len() });
    }
    for (kid, signature) in &sigs.signatures {
        let counts = approvers.get(kid.as_str()).is_some_and(|person| sig::verify(kid, &dom, signature).is_ok() && dids.insert(kid.as_str()) && people.insert(*person));
        if !counts {
            ignored += 1;
        }
    }
    let signed = people.iter().map(|p| prev.members.iter().find(|m| m.person == **p && m.is_admin() && m.is_active()).map(|m| m.handle.clone()).unwrap_or_default()).collect();
    Ok(Quorum { have: people.len(), need: prev.admin_people().len() / 2 + 1, signed, ignored })
}

/// `(kid, signature)` over the roster domain of exactly `bytes`. The only thing the key is asked to sign.
pub fn sign_epoch(who: &Identity, org: &str, store: &str, n: u64, bytes: &[u8]) -> (String, String) {
    (who.did(), who.sign(&domain(org, store, n, bytes)))
}

/// The signatures file for `bytes`: every signature in `existing` that counts (active admin of `prev`, verifies
/// over exactly these bytes, once per DID) plus `add`, which must itself count. Sorted by `kid`, so two admins who
/// merge the same signatures write the same bytes. Unparseable `existing` is an error, not an empty file.
pub fn merge_sigs(prev: &Epoch, n: u64, bytes: &[u8], existing: Option<&[u8]>, add: Option<(String, String)>) -> RResult<Vec<u8>> {
    let dom = domain(&prev.org, &prev.store, n, bytes);
    let approvers = prev.active_admins();
    let mut keep: BTreeMap<String, String> = BTreeMap::new();
    let mut all: Vec<(String, String)> = Vec::new();
    if let Some(b) = existing {
        let s = EpochSigs::parse(b)?;
        if s.epoch == n {
            all.extend(s.signatures);
        }
    }
    all.extend(add);
    for (kid, signature) in all {
        if approvers.contains_key(kid.as_str()) && sig::verify(&kid, &dom, &signature).is_ok() {
            keep.entry(kid).or_insert(signature);
        }
    }
    let list: Vec<Value> = keep.iter().map(|(k, s)| json!({ "kid": k, "sig": s })).collect();
    let mut out = serde_json::to_string_pretty(&json!({ "epoch": n, "signatures": list })).expect("a JSON value serializes");
    out.push('\n');
    Ok(out.into_bytes())
}

/// The signatures file of a brand-new statement or epoch has no entries before anyone signs; used for tests and
/// for the first signer's file.
pub fn empty_sigs(n: u64) -> Vec<u8> {
    let mut out = serde_json::to_string_pretty(&json!({ "epoch": n, "signatures": Vec::<Value>::new() })).expect("a JSON value serializes");
    out.push('\n');
    out.into_bytes()
}

// ---------------------------------------------------------------------------------------------------------
// Freshness statement (written by publish and refresh)

/// The statement `{org, store, epoch, epoch_hash, store_head, issued_at, expires_at}` in canonical spelling
/// (no `rewrite_of`: rewrite recovery is a quorum act, not an everyday admin operation), and its detached
/// signature file signed by `who`.
#[allow(clippy::too_many_arguments)]
pub fn sign_statement(who: &Identity, org: &str, store: &str, epoch: u64, epoch_hash: &str, store_head: &str, issued: i64, life_secs: i64) -> (Vec<u8>, Vec<u8>) {
    let v = json!({
        "org": org, "store": store, "epoch": epoch, "epoch_hash": epoch_hash, "store_head": store_head,
        "issued_at": crate::time::format_utc(issued), "expires_at": crate::time::format_utc(issued + life_secs),
    });
    let mut body = serde_json::to_string_pretty(&v).expect("a JSON value serializes");
    body.push('\n');
    let body = body.into_bytes();
    let dom = Domain::freshness(org, store, &hash::of(&body)).expect("org and store were validated by the roster parser");
    let sigs = json!({ "signatures": [ { "kid": who.did(), "sig": who.sign(&dom) } ] });
    let mut s = serde_json::to_string_pretty(&sigs).expect("a JSON value serializes");
    s.push('\n');
    (body, s.into_bytes())
}

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;
