//! Admin operations, the pure half (spec 011 §3.2, §4.1, §8; plan milestone X8): build the next roster epoch for
//! `add`, `remove` and `rotate`, judge **any** candidate epoch (ours or a hostile proposal branch's), show what
//! changes, and write the small signed files an admin produces. No I/O, no clock (`now` is passed), no git.
//!
//! The one rule this module exists to keep: **a candidate epoch is judged by the verifier's own code, never by
//! a copy of it.** [`check_candidate`] parses with [`Epoch::parse`] and applies [`roster::check_transition`]; the
//! quorum is never counted here (the caller assembles signatures and hands the chain to
//! [`roster::verify_chain`]). What this module adds is only what the verifier cannot know: policy about admins,
//! and a description of the change for the human who is about to sign it.
//!
//! * [`build_next`] produces a candidate and runs it through [`check_candidate`] before returning it, so an
//!   operation we build is held to exactly the standard of a proposal we read from a branch.
//! * [`diff`] describes a candidate against its predecessor: every added, retired, removed or altered entry,
//!   plus [`Flag`]s for the changes that deserve a second look (a new admin, a widened area, a second key for
//!   an existing person). Nothing in a diff is store text except DIDs, slugs and handles, which the parser
//!   already restricted to a terminal-safe alphabet.
//! * A signature on an epoch is made by [`sign_epoch`], over the roster [`Domain`] only; [`assemble_sigs`]
//!   keeps the signatures that verify over *these* bytes from an active admin of the previous epoch and drops
//!   everything else, so a hostile branch cannot bloat the signatures file or smuggle junk onto `main`.

use crate::freshness::{self, Statement};
use crate::identity::Identity;
use crate::roster::{self, Areas, Epoch, Member, Role, RosterError, Status};
use crate::sig::{self, Domain};
use crate::{did, hash, is_utc_timestamp, layout, time};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The number of admin **people** `delphi doctor` recommends (spec 011 §3.2).
pub const RECOMMENDED_ADMINS: usize = 3;

/// How long a statement we write lives unless the caller asks for less. Half of the verifier's default cap, so a
/// statement is current for a week and `status` starts warning at 3.5 days (spec 011 §4.1).
pub const DEFAULT_VALIDITY_SECS: i64 = 7 * 24 * 3600;

/// The largest signature file a proposal branch may carry per key.
pub const MAX_SIG_FILE_BYTES: usize = 1024;

/// Local policy about the roster, stricter than the verifier and never read from a store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// A change that **reduces** the number of admin people must leave at least this many. Default 2: the
    /// verifier only insists on one (an epoch without any admin deadlocks), but one admin is a single point of
    /// failure and `n_admin = 2` is the smallest roster where losing a key is a recoverable event instead of a
    /// total one only if nobody removes the second. An org that really wants to go down to one lowers this
    /// explicitly; the floor of one admin is not configurable.
    pub min_admins: usize,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy { min_admins: 2 }
    }
}

/// Why a candidate epoch or an operation was refused. A fixed slug per variant; no store text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The verifier's own rules (parse, [`roster::check_transition`], genesis shape).
    Roster(RosterError),
    /// The file names another org or store than this one.
    WrongScope,
    NotNextEpoch { expected: u64, found: u64 },
    /// `prev_hash` is not the hash of the epoch this install holds as head: a proposal built on an old head.
    StaleBase,
    /// The change would leave the roster with no admin.
    LastAdmin,
    /// The change reduces the admins below the local minimum.
    BelowMinimumAdmins { after: usize, min: usize },
    UnknownMember,
    /// The entry is not in a state the operation applies to (removing a removed entry, rotating a retired key).
    NotActive,
    /// The DID is already in the roster (entries are never dropped; a returning human gets a new key).
    AlreadyListed,
    /// `add` for a person who is already in the roster. A second key for a person is a rotation.
    PersonExists,
    SameKey,
    BadInput(&'static str),
    Validity(&'static str),
}

impl Refusal {
    pub fn reason(&self) -> &'static str {
        match self {
            Refusal::Roster(e) => e.reason(),
            Refusal::WrongScope => "wrong-scope",
            Refusal::NotNextEpoch { .. } => "not-next-epoch",
            Refusal::StaleBase => "stale-proposal",
            Refusal::LastAdmin => "last-admin",
            Refusal::BelowMinimumAdmins { .. } => "below-minimum-admins",
            Refusal::UnknownMember => "unknown-member",
            Refusal::NotActive => "not-active",
            Refusal::AlreadyListed => "already-listed",
            Refusal::PersonExists => "person-exists",
            Refusal::SameKey => "same-key",
            Refusal::BadInput(_) => "bad-input",
            Refusal::Validity(_) => "bad-validity",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Roster(e) => write!(f, "{e}"),
            Refusal::NotNextEpoch { expected, found } => write!(f, "not-next-epoch: the next epoch is {expected}, the file says {found}"),
            Refusal::BelowMinimumAdmins { after, min } => write!(f, "below-minimum-admins: this would leave {after} admin(s); the minimum is {min}"),
            Refusal::BadInput(w) | Refusal::Validity(w) => write!(f, "{}: {w}", self.reason()),
            other => write!(f, "{}", other.reason()),
        }
    }
}

impl std::error::Error for Refusal {}

impl From<RosterError> for Refusal {
    fn from(e: RosterError) -> Refusal {
        Refusal::Roster(e)
    }
}

// ---------------------------------------------------------------------------------------------------------
// Operations

/// Whether a rotation answers a routine need or a compromise. There is no default (spec 011 §8.1): the caller
/// must say which, and the choice decides `voids_tombstones` and what the operator is told to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// The old key is retired with the new one as `successor`; the member then runs `republish --resign`.
    Routine,
    /// As routine, **and** the old entry gets `voids_tombstones`: nothing the old key signed has standing, not
    /// even a withdrawal, so a thief cannot leave `sensitive` withdrawals behind (spec 011 §8.2).
    Compromised,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMember {
    pub did: String,
    pub person: String,
    pub handle: String,
    pub admin: bool,
    /// Tags, or `["*"]` alone for every area; empty for a plain member.
    pub steward_areas: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Add(NewMember),
    /// Mark the entry `removed`. `for_cause` also sets `voids_tombstones` (spec 011 §8.3, §7.4).
    Remove { did: String, for_cause: bool },
    Rotate { old_did: String, new_did: String, kind: Rotation },
}

/// A candidate epoch that passed [`check_candidate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposed {
    pub epoch: Epoch,
    /// The exact bytes that will be signed and published.
    pub bytes: Vec<u8>,
    pub hash: String,
}

fn areas_of(raw: &[String]) -> Areas {
    if raw.len() == 1 && raw[0] == "*" { Areas::All } else { Areas::Tags(raw.to_vec()) }
}

/// The next epoch after `head` (whose file hashes to `head_hash`) with `op` applied. `now` is the `added_at` of
/// a new entry. The result has been through [`check_candidate`].
pub fn build_next(head: &Epoch, head_hash: &str, op: &Operation, now: &str, policy: &Policy) -> Result<Proposed, Refusal> {
    if !is_utc_timestamp(now) {
        return Err(Refusal::BadInput("added_at is not a UTC timestamp"));
    }
    let n = head.epoch + 1;
    let mut members = head.members.clone();
    match op {
        Operation::Add(m) => {
            if !did::is_valid(&m.did) {
                return Err(Refusal::BadInput("the new member's DID is not a did:key"));
            }
            if members.iter().any(|x| x.did == m.did) {
                return Err(Refusal::AlreadyListed);
            }
            if members.iter().any(|x| x.person == m.person) {
                return Err(Refusal::PersonExists);
            }
            members.push(Member {
                did: m.did.clone(),
                person: m.person.clone(),
                handle: m.handle.clone(),
                roles: if m.admin { vec![Role::Member, Role::Admin] } else { vec![Role::Member] },
                steward_areas: areas_of(&m.steward_areas),
                added_at: now.to_string(),
                status: Status::Active,
                successor: None,
                removed_at_epoch: None,
                voids_tombstones: false,
            });
        }
        Operation::Remove { did: target, for_cause } => {
            let m = members.iter_mut().find(|x| &x.did == target).ok_or(Refusal::UnknownMember)?;
            if m.status == Status::Removed {
                return Err(Refusal::NotActive);
            }
            m.status = Status::Removed;
            m.removed_at_epoch = Some(n);
            if *for_cause {
                m.voids_tombstones = true;
            }
        }
        Operation::Rotate { old_did, new_did, kind } => {
            if old_did == new_did {
                return Err(Refusal::SameKey);
            }
            if !did::is_valid(new_did) {
                return Err(Refusal::BadInput("the new key is not a did:key"));
            }
            if members.iter().any(|x| &x.did == new_did) {
                return Err(Refusal::AlreadyListed);
            }
            let old = members.iter_mut().find(|x| &x.did == old_did).ok_or(Refusal::UnknownMember)?;
            if old.status != Status::Active {
                return Err(Refusal::NotActive);
            }
            old.status = Status::Retired;
            old.successor = Some(new_did.clone());
            if *kind == Rotation::Compromised {
                old.voids_tombstones = true;
            }
            // The successor inherits the person exactly, and the authority exactly: a rotation changes a key, not
            // what the key may do.
            let fresh = Member {
                did: new_did.clone(),
                person: old.person.clone(),
                handle: old.handle.clone(),
                roles: old.roles.clone(),
                steward_areas: old.steward_areas.clone(),
                added_at: now.to_string(),
                status: Status::Active,
                successor: None,
                removed_at_epoch: None,
                voids_tombstones: false,
            };
            members.push(fresh);
        }
    }
    let epoch = Epoch { org: head.org.clone(), store: head.store.clone(), epoch: n, prev_hash: Some(head_hash.to_string()), members };
    let bytes = epoch_bytes(&epoch);
    let epoch = check_candidate(head, head_hash, &head.org, &head.store, &bytes, policy)?;
    Ok(Proposed { epoch, hash: hash::of(&bytes), bytes })
}

/// The epoch file for `e`: strict JSON, fixed fields, one trailing newline.
pub fn epoch_bytes(e: &Epoch) -> Vec<u8> {
    let members: Vec<Value> = e.members.iter().map(member_json).collect();
    let v = json!({ "org": e.org, "store": e.store, "epoch": e.epoch, "prev_hash": e.prev_hash, "members": members });
    let mut s = serde_json::to_string_pretty(&v).expect("a json value serializes");
    s.push('\n');
    s.into_bytes()
}

fn role_name(r: Role) -> &'static str {
    match r {
        Role::Member => "member",
        Role::Admin => "admin",
    }
}

fn areas_list(a: &Areas) -> Vec<String> {
    match a {
        Areas::All => vec!["*".to_string()],
        Areas::Tags(t) => t.clone(),
    }
}

fn member_json(m: &Member) -> Value {
    let mut v = json!({
        "did": m.did, "person": m.person, "handle": m.handle,
        "roles": m.roles.iter().map(|r| role_name(*r)).collect::<Vec<_>>(),
        "steward_areas": areas_list(&m.steward_areas), "added_at": m.added_at, "status": m.status.as_str(),
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

// ---------------------------------------------------------------------------------------------------------
// Judging a candidate

/// Judge `bytes` as epoch `prev.epoch + 1` on top of `prev` (whose file hashes to `prev_hash`). The file may come
/// from anywhere, including a hostile branch: it is parsed by the verifier's parser, must name this org and store,
/// the next number and the right `prev_hash`, must pass [`roster::check_transition`], and must satisfy the admin
/// policy. **A refusal here means no one should be asked to sign.**
///
/// Order matters for the message, not the verdict: the admin-count rules run before the verifier's transition
/// rules so that "this removes the last admin" is said as that, not as a generic `no-active-admin`.
pub fn check_candidate(prev: &Epoch, prev_hash: &str, org: &str, store: &str, bytes: &[u8], policy: &Policy) -> Result<Epoch, Refusal> {
    let e = Epoch::parse(bytes)?;
    if e.org != org || e.store != store || prev.org != org || prev.store != store {
        return Err(Refusal::WrongScope);
    }
    if e.epoch != prev.epoch + 1 {
        return Err(Refusal::NotNextEpoch { expected: prev.epoch + 1, found: e.epoch });
    }
    if e.prev_hash.as_deref() != Some(prev_hash) {
        return Err(Refusal::StaleBase);
    }
    let (before, after) = (prev.admin_people().len(), e.admin_people().len());
    if after == 0 {
        return Err(Refusal::LastAdmin);
    }
    if after < before && after < policy.min_admins {
        return Err(Refusal::BelowMinimumAdmins { after, min: policy.min_admins });
    }
    roster::check_transition(prev, &e)?;
    Ok(e)
}

/// Judge `bytes` as a genesis draft for `org`/`store`: epoch 1, no `prev_hash`, every entry a fresh `active`
/// one, and at least one admin. A founder runs this before signing; the chain verifier runs the same shape check
/// again when the signatures are assembled.
pub fn check_genesis_draft(org: &str, store: &str, bytes: &[u8]) -> Result<Epoch, Refusal> {
    let e = Epoch::parse(bytes)?;
    if e.org != org || e.store != store {
        return Err(Refusal::WrongScope);
    }
    if e.epoch != 1 {
        return Err(Refusal::NotNextEpoch { expected: 1, found: e.epoch });
    }
    roster::check_genesis(&e)?;
    if e.admin_people().is_empty() {
        return Err(Refusal::LastAdmin);
    }
    Ok(e)
}

/// Build a genesis epoch. Every founder is `active`; the ones with `admin` set sign it.
pub fn build_genesis(org: &str, store: &str, founders: &[NewMember], now: &str) -> Result<Proposed, Refusal> {
    if !is_utc_timestamp(now) {
        return Err(Refusal::BadInput("added_at is not a UTC timestamp"));
    }
    let members = founders
        .iter()
        .map(|m| Member {
            did: m.did.clone(),
            person: m.person.clone(),
            handle: m.handle.clone(),
            roles: if m.admin { vec![Role::Member, Role::Admin] } else { vec![Role::Member] },
            steward_areas: areas_of(&m.steward_areas),
            added_at: now.to_string(),
            status: Status::Active,
            successor: None,
            removed_at_epoch: None,
            voids_tombstones: false,
        })
        .collect();
    let epoch = Epoch { org: org.to_string(), store: store.to_string(), epoch: 1, prev_hash: None, members };
    let bytes = epoch_bytes(&epoch);
    let epoch = check_genesis_draft(org, store, &bytes)?;
    Ok(Proposed { epoch, hash: hash::of(&bytes), bytes })
}

// ---------------------------------------------------------------------------------------------------------
// Describing a change

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Retired,
    Removed,
    VoidsTombstonesOn,
    RolesChanged,
    AreasChanged,
    HandleChanged,
}

impl ChangeKind {
    pub fn slug(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Retired => "retired",
            ChangeKind::Removed => "removed",
            ChangeKind::VoidsTombstonesOn => "voids-tombstones",
            ChangeKind::RolesChanged => "roles-changed",
            ChangeKind::AreasChanged => "areas-changed",
            ChangeKind::HandleChanged => "handle-changed",
        }
    }
}

/// One entry-level difference. `detail` is built from DIDs, slugs, handles and role names only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub kind: ChangeKind,
    pub did: String,
    pub person: String,
    pub handle: String,
    pub detail: String,
}

/// A change that deserves a second look. A preview with flags needs them acknowledged, as in the publish gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Flag {
    /// A new entry holds the `admin` role.
    AddsAdmin,
    /// An existing member gains the `admin` role.
    GainsAdmin,
    /// A steward's areas grow (to `*`, or by a tag).
    WidensAreas,
    /// The number of admin people goes down.
    ReducesAdmins,
    /// A new key under a person who is already in the roster, other than as a rotation's successor.
    KeyForExistingPerson,
    /// An entry's tombstones and withdrawals stop counting.
    VoidsTombstones,
    /// The roster has fewer than [`RECOMMENDED_ADMINS`] admin people afterwards.
    FewAdmins,
    ChangesRoles,
    ChangesHandle,
}

impl Flag {
    pub fn slug(self) -> &'static str {
        match self {
            Flag::AddsAdmin => "adds-admin",
            Flag::GainsAdmin => "gains-admin",
            Flag::WidensAreas => "widens-areas",
            Flag::ReducesAdmins => "reduces-admins",
            Flag::KeyForExistingPerson => "key-for-existing-person",
            Flag::VoidsTombstones => "voids-tombstones",
            Flag::FewAdmins => "few-admins",
            Flag::ChangesRoles => "changes-roles",
            Flag::ChangesHandle => "changes-handle",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Flag::AddsAdmin => "a new entry becomes an admin: it will take part in every later roster quorum",
            Flag::GainsAdmin => "an existing member becomes an admin",
            Flag::WidensAreas => "a steward's areas grow: they may endorse and tombstone more",
            Flag::ReducesAdmins => "the number of admin people goes down",
            Flag::KeyForExistingPerson => "a new key is listed for a person who is already in the roster, and it is not a rotation: whoever holds it speaks as that person",
            Flag::VoidsTombstones => "an entry's withdrawals and tombstones stop having effect",
            Flag::FewAdmins => "fewer than 3 admin people remain; losing one key would be hard to recover from",
            Flag::ChangesRoles => "an active member's roles change",
            Flag::ChangesHandle => "an active member's handle changes",
        }
    }
}

/// What the operator must be told after the action, as a fixed slug and the words for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reminder {
    /// A routine rotation: the member runs `republish --resign`.
    RepublishResign,
    /// A compromise rotation: nothing is re-signed automatically, and why.
    CompromisedKeyRecords,
}

impl Reminder {
    pub fn slug(self) -> &'static str {
        match self {
            Reminder::RepublishResign => "republish-resign",
            Reminder::CompromisedKeyRecords => "compromised-key-records",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Reminder::RepublishResign => "routine rotation: the member now runs `republish --resign` with the new key, which re-publishes their records and re-issues their endorsements, tombstones and disputes as new revisions signed by the successor; records with an effective withdraw are skipped.",
            Reminder::CompromisedKeyRecords => "compromised key: nothing the old key signed has standing from this epoch on (records, endorsements and tombstones, including withdrawals), and nothing is re-signed automatically; `republish --resign` refuses a compromise rotation. The successor re-publishes only what they have reviewed, as new revisions; stewards re-endorse what is worth keeping; a record a thief hid can be restored afresh by its author or a covering steward. Rotate any secret the old key could have exposed.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diff {
    pub changes: Vec<Change>,
    pub flags: Vec<Flag>,
    pub reminders: Vec<Reminder>,
}

fn role_list(m: &Member) -> String {
    m.roles.iter().map(|r| role_name(*r)).collect::<Vec<_>>().join("+")
}

fn area_text(a: &Areas) -> String {
    let l = areas_list(a);
    if l.is_empty() { "none".into() } else { l.join(",") }
}

fn widens(old: &Areas, new: &Areas) -> bool {
    match (old, new) {
        (Areas::All, _) => false,
        (_, Areas::All) => true,
        (Areas::Tags(o), Areas::Tags(n)) => n.iter().any(|t| !o.contains(t)),
    }
}

/// Describe `next` against `prev` (`None` for genesis).
pub fn diff(prev: Option<&Epoch>, next: &Epoch) -> Diff {
    let mut changes = Vec::new();
    let mut flags = BTreeSet::new();
    let mut reminders = BTreeSet::new();
    let change = |kind, m: &Member, detail: String| Change { kind, did: m.did.clone(), person: m.person.clone(), handle: m.handle.clone(), detail };
    // A new key is a rotation's successor when some entry of the same person, in this epoch, names it.
    let is_successor = |m: &Member| next.members.iter().any(|o| o.successor.as_deref() == Some(m.did.as_str()) && o.person == m.person);
    for m in &next.members {
        let old = prev.and_then(|p| p.member(&m.did));
        match old {
            None => {
                changes.push(change(ChangeKind::Added, m, format!("roles {}; areas {}", role_list(m), area_text(&m.steward_areas))));
                if m.is_admin() {
                    flags.insert(Flag::AddsAdmin);
                }
                if matches!(m.steward_areas, Areas::All) && !is_successor(m) {
                    flags.insert(Flag::WidensAreas);
                }
                if prev.is_some_and(|p| p.members.iter().any(|o| o.person == m.person)) && !is_successor(m) {
                    flags.insert(Flag::KeyForExistingPerson);
                }
            }
            Some(o) => {
                if o.status == Status::Active && m.status == Status::Retired {
                    changes.push(change(ChangeKind::Retired, m, m.successor.as_ref().map(|s| format!("successor {s}")).unwrap_or_else(|| "no successor".into())));
                    if m.successor.is_some() && !m.voids_tombstones {
                        reminders.insert(Reminder::RepublishResign);
                    }
                }
                if o.status != Status::Removed && m.status == Status::Removed {
                    changes.push(change(ChangeKind::Removed, m, if m.voids_tombstones { "for cause: withdrawals void".into() } else { "withdrawals keep their effect".into() }));
                }
                if !o.voids_tombstones && m.voids_tombstones {
                    changes.push(change(ChangeKind::VoidsTombstonesOn, m, "its tombstones and withdrawals stop counting".into()));
                    flags.insert(Flag::VoidsTombstones);
                    if m.status != Status::Removed && m.successor.is_some() {
                        reminders.insert(Reminder::CompromisedKeyRecords);
                        reminders.remove(&Reminder::RepublishResign);
                    }
                }
                if o.roles != m.roles {
                    changes.push(change(ChangeKind::RolesChanged, m, format!("{} -> {}", role_list(o), role_list(m))));
                    flags.insert(Flag::ChangesRoles);
                    if m.is_admin() && !o.is_admin() {
                        flags.insert(Flag::GainsAdmin);
                    }
                }
                if o.steward_areas != m.steward_areas {
                    changes.push(change(ChangeKind::AreasChanged, m, format!("{} -> {}", area_text(&o.steward_areas), area_text(&m.steward_areas))));
                    if widens(&o.steward_areas, &m.steward_areas) {
                        flags.insert(Flag::WidensAreas);
                    }
                }
                if o.handle != m.handle {
                    changes.push(change(ChangeKind::HandleChanged, m, format!("{} -> {}", o.handle, m.handle)));
                    flags.insert(Flag::ChangesHandle);
                }
            }
        }
    }
    let after = next.admin_people().len();
    if prev.is_some_and(|p| after < p.admin_people().len()) {
        flags.insert(Flag::ReducesAdmins);
    }
    if after < RECOMMENDED_ADMINS {
        flags.insert(Flag::FewAdmins);
    }
    Diff { changes, flags: flags.into_iter().collect(), reminders: reminders.into_iter().collect() }
}

// ---------------------------------------------------------------------------------------------------------
// Proposal branch layout and signatures

/// The pending branch for epoch `n` (spec 011 §3.2).
pub fn proposal_branch(n: u64) -> String {
    format!("roster-proposal/{n}")
}

/// Where an admin's signature for proposal `n` lives on the proposal branch: one file per key, named by the
/// key's fingerprint, so two admins signing at once never touch the same file.
pub fn proposal_sig_path(n: u64, kid: &str) -> Result<String, Refusal> {
    let fp = did::fingerprint(kid).map_err(|_| Refusal::BadInput("kid"))?;
    Ok(format!("roster/proposals/{n}/{}.json", fp.trim_start_matches("blake3:")))
}

/// The directory of proposal `n`'s signature files.
pub fn proposal_sig_dir(n: u64) -> String {
    format!("roster/proposals/{n}/")
}

/// One admin's detached signature over proposal `n`'s exact bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalSig {
    pub kid: String,
    pub sig: String,
}

impl ProposalSig {
    pub fn to_json(&self) -> Vec<u8> {
        let mut s = serde_json::to_string_pretty(&json!({ "kid": self.kid, "sig": self.sig })).expect("serializes");
        s.push('\n');
        s.into_bytes()
    }

    /// Strict: exactly `{kid, sig}`, a `did:key` and a canonical signature spelling.
    pub fn parse(bytes: &[u8]) -> Result<ProposalSig, &'static str> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            kid: String,
            sig: String,
        }
        if bytes.len() > MAX_SIG_FILE_BYTES {
            return Err("oversize");
        }
        let raw: Raw = serde_json::from_slice(bytes).map_err(|_| "malformed")?;
        if !did::is_valid(&raw.kid) || sig::decode_signature(&raw.sig).is_err() {
            return Err("malformed");
        }
        Ok(ProposalSig { kid: raw.kid, sig: raw.sig })
    }
}

/// Sign epoch `n`'s exact bytes. The only thing an admin key is ever asked to sign for a roster, and it can only
/// be a roster [`Domain`].
pub fn sign_epoch(key: &Identity, org: &str, store: &str, n: u64, bytes: &[u8]) -> Result<ProposalSig, Refusal> {
    let domain = epoch_domain(org, store, n, bytes)?;
    Ok(ProposalSig { kid: key.did(), sig: key.sign(&domain) })
}

/// The domain a roster signature covers for these bytes.
pub fn epoch_domain(org: &str, store: &str, n: u64, bytes: &[u8]) -> Result<Domain, Refusal> {
    Domain::roster(org, store, n, &hash::of(bytes)).map_err(|_| Refusal::WrongScope)
}

/// The detached-signatures file for epoch `n`.
pub fn epoch_sigs_file(n: u64, sigs: &[(String, String)]) -> Vec<u8> {
    let list: Vec<Value> = sigs.iter().map(|(k, s)| json!({ "kid": k, "sig": s })).collect();
    let mut s = serde_json::to_string_pretty(&json!({ "epoch": n, "signatures": list })).expect("serializes");
    s.push('\n');
    s.into_bytes()
}

/// The signatures worth publishing: from a key that is an `active` admin of `prev`, over exactly `bytes`, one per
/// key, in key order. Everything else is dropped. This filters; it does not count. Whether enough **people**
/// signed is decided only by [`roster::verify_chain`] on the assembled file.
pub fn assemble_sigs(org: &str, store: &str, n: u64, bytes: &[u8], prev: &Epoch, candidates: &[ProposalSig]) -> Vec<u8> {
    let Ok(domain) = epoch_domain(org, store, n, bytes) else { return epoch_sigs_file(n, &[]) };
    let approvers = prev.active_admins();
    let mut kept: BTreeMap<String, String> = BTreeMap::new();
    for c in candidates {
        if approvers.contains_key(c.kid.as_str()) && sig::verify(&c.kid, &domain, &c.sig).is_ok() {
            kept.entry(c.kid.clone()).or_insert_with(|| c.sig.clone());
        }
    }
    let list: Vec<(String, String)> = kept.into_iter().collect();
    epoch_sigs_file(n, &list)
}

// ---------------------------------------------------------------------------------------------------------
// The freshness statement

/// The statement for the verified head: `epoch`/`epoch_hash` of the chain head, `store_head` the commit the admin
/// verified. `validity_secs` must be positive and within the verifier's own cap, because a statement longer
/// than that is ignored by every install. The output is read back by the verifier's parser before it is returned.
pub fn build_statement(org: &str, store: &str, epoch: u64, epoch_hash: &str, store_head: &str, now: i64, validity_secs: i64, cap: &freshness::Policy) -> Result<Vec<u8>, Refusal> {
    if validity_secs <= 0 {
        return Err(Refusal::Validity("validity must be positive"));
    }
    if validity_secs > cap.max_validity_secs {
        return Err(Refusal::Validity("validity is over the cap every install applies"));
    }
    let v = json!({
        "org": org, "store": store, "epoch": epoch, "epoch_hash": epoch_hash, "store_head": store_head,
        "issued_at": time::format_utc(now), "expires_at": time::format_utc(now + validity_secs),
    });
    let mut text = serde_json::to_string_pretty(&v).expect("serializes");
    text.push('\n');
    let bytes = text.into_bytes();
    let st = Statement::parse(&bytes).map_err(|_| Refusal::Validity("the statement does not parse"))?;
    if st.org != org || st.store != store || st.epoch != epoch || st.epoch_hash != epoch_hash || st.store_head != store_head || st.issued != now || st.rewrite_of.is_some() {
        return Err(Refusal::Validity("the statement does not read back as written"));
    }
    Ok(bytes)
}

/// The domain a freshness signature covers for these statement bytes.
pub fn statement_domain(org: &str, store: &str, bytes: &[u8]) -> Result<Domain, Refusal> {
    Domain::freshness(org, store, &hash::of(bytes)).map_err(|_| Refusal::WrongScope)
}

/// `freshness.sigs.json` with this key's signature over `statement_bytes`.
pub fn sign_statement(key: &Identity, org: &str, store: &str, statement_bytes: &[u8]) -> Result<Vec<u8>, Refusal> {
    let domain = statement_domain(org, store, statement_bytes)?;
    Ok(statement_sigs_file(&key.did(), &key.sign(&domain)))
}

pub fn statement_sigs_file(kid: &str, sig: &str) -> Vec<u8> {
    let mut s = serde_json::to_string_pretty(&json!({ "signatures": [ { "kid": kid, "sig": sig } ] })).expect("serializes");
    s.push('\n');
    s.into_bytes()
}

/// The paths of epoch `n`'s two files, for callers building a commit.
pub fn epoch_paths(n: u64) -> (String, String) {
    (layout::roster_epoch(n), layout::roster_epoch_sigs(n))
}

/// A length-prefixed digest of labelled parts, domain-separated from every signed string. A preview's digest is
/// this over everything that will be signed or published, so an approval cannot be moved to other bytes.
pub fn digest(parts: &[(&str, &[u8])]) -> String {
    let mut buf = b"xenia-admin-preview/1\n".to_vec();
    for (label, bytes) in parts {
        for part in [label.as_bytes(), bytes] {
            buf.extend_from_slice(part.len().to_string().as_bytes());
            buf.push(b'\n');
            buf.extend_from_slice(part);
            buf.push(b'\n');
        }
    }
    hash::of(&buf)
}

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;
