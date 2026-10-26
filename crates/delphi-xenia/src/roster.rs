//! The roster chain (spec 011 §3.2, spec 012 §3 and §7): epoch files, their detached signatures, and the pure
//! verification of an ordered list of them against a pinned trust anchor.
//!
//! **The store is untrusted; the roster is the root of trust** (spec 011 §1.3), and the roster is only as good as
//! this module. Everything here parses attacker-controlled bytes, so it fails closed:
//!
//! - Epoch and signature files are strict JSON: unknown keys, duplicate keys, wrong types and `null` where a value
//!   is required are refused, and every string is checked against the narrowest alphabet that fits its job.
//! - A chain is verified **forward from genesis**. Epoch `n` must name the BLAKE3 of the bytes of epoch `n-1`, be
//!   signed by a majority (counted per **person**, not per key) of the `active` admins of epoch `n-1`, and only
//!   change the previous roster in the ways [`check_transition`] allows. The quorum is computed from the
//!   *previous* epoch, never from the file being checked, otherwise any epoch could appoint its own approvers.
//! - A chain proves nothing about itself: anyone can build a consistent chain from keys they hold. It is only
//!   believed if it contains every epoch hash in the [`Anchor`] (the install's pins, or the head pair a joiner was
//!   handed out of band), and an empty anchor is refused, not treated as "nothing to check".
//!
//! Nothing here reads a file, a clock or the network: the caller hands in bytes.

use crate::sig::{self, Domain};
use crate::{did, hash, is_slug, is_utc_timestamp};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Upper bounds, so a hostile store cannot make a parser allocate or loop without end.
pub const MAX_EPOCH_BYTES: usize = 128 * 1024;
pub const MAX_SIGS_BYTES: usize = 64 * 1024;
pub const MAX_MEMBERS: usize = 256;
pub const MAX_EPOCHS: usize = 4096;
const MAX_SIGNATURES: usize = 256;
const MAX_AREAS: usize = 32;

/// Why a roster file, chain or join was refused. The slug is what `delphi xenia status` shows; no variant carries
/// attacker-controlled text (only numbers and fixed strings), so it is safe to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterError {
    /// The scope (org, store) the caller asked about is not a pair of slugs.
    BadScope,
    /// A chain with nothing pinned: it would certify itself.
    NoAnchor,
    EmptyChain,
    TooManyEpochs,
    Oversize(&'static str),
    /// An epoch file that is not the strict JSON shape; `0` is the position in the list when the number is unknown.
    BadEpoch(&'static str),
    BadSigs(&'static str),
    BadPins(&'static str),
    BadRequest(&'static str),
    /// The file names another org or store than the one this chain is for.
    WrongScope { epoch: u64 },
    /// The list skips a number: `expected` was next, `found` was claimed.
    Gap { expected: u64, found: u64 },
    /// A second file for an epoch number already seen, with different bytes (equivocation).
    Fork { epoch: u64 },
    /// A second file for an epoch number already seen, with the same bytes (a replay of an old epoch).
    Replay { epoch: u64 },
    PrevHashMismatch { epoch: u64 },
    GenesisPrevHash,
    SigsEpochMismatch { epoch: u64 },
    /// Genesis must be signed by every founding admin.
    GenesisNotFullySigned { epoch: u64 },
    QuorumNotMet { epoch: u64, have: usize, need: usize },
    /// An entry present in the previous epoch is missing (entries are never dropped).
    Dropped { epoch: u64 },
    /// A DID's `person` changed, or a successor does not carry its predecessor's person.
    PersonChanged { epoch: u64 },
    /// A retired or removed DID came back, or `removed` became `retired`.
    StatusRegression { epoch: u64 },
    /// A field that must not change between epochs did (`added_at`, or anything of a non-active entry).
    ImmutableChanged { epoch: u64, field: &'static str },
    /// A new entry that is not `active`, or carries a successor, removal or voiding flag.
    BadNewEntry { epoch: u64 },
    BadSuccessor { epoch: u64 },
    /// An epoch with no active admin would deadlock the roster.
    NoActiveAdmin { epoch: u64 },
    /// A file under a pinned epoch number has other bytes than the pin (equivocation, spec 011 §3.2).
    PinMismatch { epoch: u64 },
    /// The chain ends before an epoch the anchor pins: a stale or truncated view.
    PinnedEpochMissing { epoch: u64 },
    /// `join`: the admin the operator named is not an active admin of the pinned epoch.
    AdminNotActive,
    AlreadyPinned,
    Io(String),
}

impl RosterError {
    pub fn reason(&self) -> &'static str {
        match self {
            RosterError::BadScope => "bad-scope",
            RosterError::NoAnchor => "no-anchor",
            RosterError::EmptyChain => "empty-chain",
            RosterError::TooManyEpochs => "too-many-epochs",
            RosterError::Oversize(_) => "oversize",
            RosterError::BadEpoch(_) => "bad-epoch",
            RosterError::BadSigs(_) => "bad-signatures-file",
            RosterError::BadPins(_) => "bad-pins",
            RosterError::BadRequest(_) => "bad-request",
            RosterError::WrongScope { .. } => "wrong-scope",
            RosterError::Gap { .. } => "epoch-gap",
            RosterError::Fork { .. } => "epoch-fork",
            RosterError::Replay { .. } => "epoch-replay",
            RosterError::PrevHashMismatch { .. } => "prev-hash-mismatch",
            RosterError::GenesisPrevHash => "genesis-prev-hash",
            RosterError::SigsEpochMismatch { .. } => "sigs-epoch-mismatch",
            RosterError::GenesisNotFullySigned { .. } => "genesis-not-fully-signed",
            RosterError::QuorumNotMet { .. } => "quorum-not-met",
            RosterError::Dropped { .. } => "entry-dropped",
            RosterError::PersonChanged { .. } => "person-changed",
            RosterError::StatusRegression { .. } => "status-regression",
            RosterError::ImmutableChanged { .. } => "immutable-field-changed",
            RosterError::BadNewEntry { .. } => "bad-new-entry",
            RosterError::BadSuccessor { .. } => "bad-successor",
            RosterError::NoActiveAdmin { .. } => "no-active-admin",
            RosterError::PinMismatch { .. } => "pin-mismatch",
            RosterError::PinnedEpochMissing { .. } => "pinned-epoch-missing",
            RosterError::AdminNotActive => "admin-not-active",
            RosterError::AlreadyPinned => "already-pinned",
            RosterError::Io(_) => "io",
        }
    }
}

impl fmt::Display for RosterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slug = self.reason();
        match self {
            RosterError::Oversize(w) | RosterError::BadEpoch(w) | RosterError::BadSigs(w) | RosterError::BadPins(w) | RosterError::BadRequest(w) => write!(f, "{slug}: {w}"),
            RosterError::Io(w) => write!(f, "{slug}: {w}"),
            RosterError::Gap { expected, found } => write!(f, "{slug}: expected epoch {expected}, found {found}"),
            RosterError::QuorumNotMet { epoch, have, need } => write!(f, "{slug}: epoch {epoch} has {have} of {need} required people"),
            RosterError::ImmutableChanged { epoch, field } => write!(f, "{slug}: epoch {epoch} changed {field}"),
            RosterError::WrongScope { epoch }
            | RosterError::Fork { epoch }
            | RosterError::Replay { epoch }
            | RosterError::PrevHashMismatch { epoch }
            | RosterError::SigsEpochMismatch { epoch }
            | RosterError::GenesisNotFullySigned { epoch }
            | RosterError::Dropped { epoch }
            | RosterError::PersonChanged { epoch }
            | RosterError::StatusRegression { epoch }
            | RosterError::BadNewEntry { epoch }
            | RosterError::BadSuccessor { epoch }
            | RosterError::NoActiveAdmin { epoch }
            | RosterError::PinMismatch { epoch }
            | RosterError::PinnedEpochMissing { epoch } => write!(f, "{slug}: epoch {epoch}"),
            _ => write!(f, "{slug}"),
        }
    }
}

impl std::error::Error for RosterError {}

pub type RResult<T> = std::result::Result<T, RosterError>;

// ---------------------------------------------------------------------------------------------------------
// Types

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Active,
    Retired,
    Removed,
}

impl Status {
    fn parse(s: &str) -> Option<Status> {
        match s {
            "active" => Some(Status::Active),
            "retired" => Some(Status::Retired),
            "removed" => Some(Status::Removed),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Retired => "retired",
            Status::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Member,
    Admin,
}

/// A steward's areas (spec 011 §2.3): none (not a steward), named tags, or the wildcard `*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Areas {
    Tags(Vec<String>),
    All,
}

impl Areas {
    pub fn is_steward(&self) -> bool {
        !matches!(self, Areas::Tags(t) if t.is_empty())
    }

    /// Whether a record with these tags is within the areas. A record with no tags is covered only by `*`.
    pub fn covers(&self, tags: &[String]) -> bool {
        match self {
            Areas::All => true,
            Areas::Tags(areas) => tags.iter().any(|t| areas.contains(t)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub did: String,
    pub person: String,
    pub handle: String,
    pub roles: Vec<Role>,
    pub steward_areas: Areas,
    pub added_at: String,
    pub status: Status,
    pub successor: Option<String>,
    pub removed_at_epoch: Option<u64>,
    pub voids_tombstones: bool,
}

impl Member {
    pub fn is_admin(&self) -> bool {
        self.roles.contains(&Role::Admin)
    }
    pub fn is_active(&self) -> bool {
        self.status == Status::Active
    }
}

/// One parsed epoch file. Parsing proves the file is well formed and internally consistent; only
/// [`verify_chain`] proves it belongs to a valid chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Epoch {
    pub org: String,
    pub store: String,
    pub epoch: u64,
    /// `None` only for genesis (epoch 1).
    pub prev_hash: Option<String>,
    pub members: Vec<Member>,
}

impl Epoch {
    pub fn member(&self, did: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.did == did)
    }

    /// DID to person for every `active` admin: the set that signs the next epoch.
    pub fn active_admins(&self) -> BTreeMap<&str, &str> {
        self.members.iter().filter(|m| m.is_active() && m.is_admin()).map(|m| (m.did.as_str(), m.person.as_str())).collect()
    }

    /// The people holding an active admin DID: `n_admin` of spec 011 §3.2 is this set's size.
    pub fn admin_people(&self) -> BTreeSet<&str> {
        self.active_admins().values().copied().collect()
    }

    /// Parse one epoch file. Strict; see the module docs.
    pub fn parse(bytes: &[u8]) -> RResult<Epoch> {
        if bytes.len() > MAX_EPOCH_BYTES {
            return Err(RosterError::Oversize("epoch file over 128 KiB"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| RosterError::BadEpoch("not UTF-8"))?;
        let raw: RawEpoch = serde_json::from_str(text).map_err(|_| RosterError::BadEpoch("not the fixed JSON shape"))?;
        raw.validate()
    }
}

/// The two files of one epoch as the store holds them: `<n>.json` and `<n>.sigs.json`, raw bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochFile {
    pub epoch_json: Vec<u8>,
    pub sigs_json: Vec<u8>,
}

// ---------------------------------------------------------------------------------------------------------
// Strict parsing

/// A key that must be present but may be `null` (genesis has `prev_hash: null`).
fn required_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

/// An optional key that, when present, must carry a real value: `null` is a second spelling of "absent".
fn present_or_absent<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEpoch {
    org: String,
    store: String,
    epoch: u64,
    #[serde(deserialize_with = "required_nullable")]
    prev_hash: Option<String>,
    members: Vec<RawMember>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMember {
    did: String,
    person: String,
    handle: String,
    roles: Vec<String>,
    steward_areas: Vec<String>,
    added_at: String,
    status: String,
    #[serde(default, deserialize_with = "present_or_absent")]
    successor: Option<String>,
    #[serde(default, deserialize_with = "present_or_absent")]
    removed_at_epoch: Option<u64>,
    #[serde(default, deserialize_with = "present_or_absent")]
    voids_tombstones: Option<bool>,
}

/// A handle is display text, but it reaches terminals and prompts, so it gets a small alphabet.
fn is_handle(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

impl RawEpoch {
    fn validate(self) -> RResult<Epoch> {
        if !is_slug(&self.org, 32) || !is_slug(&self.store, 32) {
            return Err(RosterError::BadEpoch("org or store is not a slug"));
        }
        if self.epoch == 0 {
            return Err(RosterError::BadEpoch("epochs are numbered from 1"));
        }
        match (&self.prev_hash, self.epoch) {
            (None, 1) => {}
            (Some(_), 1) => return Err(RosterError::GenesisPrevHash),
            (None, _) => return Err(RosterError::BadEpoch("prev_hash is required after genesis")),
            (Some(h), _) if !hash::is_valid(h) => return Err(RosterError::BadEpoch("prev_hash is not a blake3 hash")),
            _ => {}
        }
        if self.members.is_empty() || self.members.len() > MAX_MEMBERS {
            return Err(RosterError::BadEpoch("member count"));
        }
        let epoch_no = self.epoch;
        let mut members = Vec::with_capacity(self.members.len());
        let mut seen = BTreeSet::new();
        for m in self.members {
            let m = m.validate(epoch_no)?;
            if !seen.insert(m.did.clone()) {
                return Err(RosterError::BadEpoch("a DID is listed twice"));
            }
            members.push(m);
        }
        let epoch = Epoch { org: self.org, store: self.store, epoch: epoch_no, prev_hash: self.prev_hash, members };
        check_successors(&epoch)?;
        Ok(epoch)
    }
}

impl RawMember {
    fn validate(self, epoch: u64) -> RResult<Member> {
        if !did::is_valid(&self.did) {
            return Err(RosterError::BadEpoch("member did is not a did:key"));
        }
        if !is_slug(&self.person, 32) {
            return Err(RosterError::BadEpoch("person is not a slug"));
        }
        if !is_handle(&self.handle) {
            return Err(RosterError::BadEpoch("handle"));
        }
        let mut roles = Vec::new();
        for r in &self.roles {
            let role = match r.as_str() {
                "member" => Role::Member,
                "admin" => Role::Admin,
                _ => return Err(RosterError::BadEpoch("unknown role")),
            };
            if roles.contains(&role) {
                return Err(RosterError::BadEpoch("role listed twice"));
            }
            roles.push(role);
        }
        if roles.is_empty() {
            return Err(RosterError::BadEpoch("a member needs at least one role"));
        }
        let steward_areas = parse_areas(&self.steward_areas)?;
        if !is_utc_timestamp(&self.added_at) {
            return Err(RosterError::BadEpoch("added_at"));
        }
        let status = Status::parse(&self.status).ok_or(RosterError::BadEpoch("unknown status"))?;
        if let Some(s) = &self.successor {
            if !did::is_valid(s) {
                return Err(RosterError::BadEpoch("successor is not a did:key"));
            }
            if s == &self.did {
                return Err(RosterError::BadSuccessor { epoch });
            }
            if status == Status::Active {
                return Err(RosterError::BadSuccessor { epoch });
            }
        }
        match (status, self.removed_at_epoch) {
            (Status::Removed, Some(n)) if (1..=epoch).contains(&n) => {}
            (Status::Removed, _) => return Err(RosterError::BadEpoch("a removed entry needs removed_at_epoch within the chain so far")),
            (_, Some(_)) => return Err(RosterError::BadEpoch("removed_at_epoch on an entry that is not removed")),
            (_, None) => {}
        }
        let voids = self.voids_tombstones.unwrap_or(false);
        if voids && status == Status::Active {
            return Err(RosterError::BadEpoch("voids_tombstones on an active entry"));
        }
        // `voids_tombstones: false` is the same fact as absent; accept it but keep one in-memory form.
        Ok(Member {
            did: self.did,
            person: self.person,
            handle: self.handle,
            roles,
            steward_areas,
            added_at: self.added_at,
            status,
            successor: self.successor,
            removed_at_epoch: self.removed_at_epoch,
            voids_tombstones: voids,
        })
    }
}

fn parse_areas(raw: &[String]) -> RResult<Areas> {
    if raw.len() > MAX_AREAS {
        return Err(RosterError::BadEpoch("too many steward areas"));
    }
    if raw.iter().any(|a| a == "*") {
        return if raw.len() == 1 { Ok(Areas::All) } else { Err(RosterError::BadEpoch("the wildcard area must stand alone")) };
    }
    let mut seen = BTreeSet::new();
    for a in raw {
        if !is_slug(a, 32) {
            return Err(RosterError::BadEpoch("steward area is not a tag"));
        }
        if !seen.insert(a.as_str()) {
            return Err(RosterError::BadEpoch("steward area listed twice"));
        }
    }
    Ok(Areas::Tags(raw.to_vec()))
}

/// Every successor names an entry of the same epoch with the same person, and following successors always ends
/// (a cycle would make "who inherits whose records" ambiguous).
fn check_successors(e: &Epoch) -> RResult<()> {
    let by_did: BTreeMap<&str, &Member> = e.members.iter().map(|m| (m.did.as_str(), m)).collect();
    for m in &e.members {
        let Some(s) = &m.successor else { continue };
        let target = by_did.get(s.as_str()).ok_or(RosterError::BadSuccessor { epoch: e.epoch })?;
        if target.person != m.person {
            return Err(RosterError::PersonChanged { epoch: e.epoch });
        }
        let mut cur = m;
        for _ in 0..=e.members.len() {
            match &cur.successor {
                None => break,
                Some(next) if next == &m.did => return Err(RosterError::BadSuccessor { epoch: e.epoch }),
                Some(next) => cur = by_did.get(next.as_str()).ok_or(RosterError::BadSuccessor { epoch: e.epoch })?,
            }
        }
    }
    Ok(())
}

/// The detached signatures of one epoch, `{ "epoch": n, "signatures": [ { "kid", "sig" } ] }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochSigs {
    pub epoch: u64,
    pub signatures: Vec<(String, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSigs {
    epoch: u64,
    signatures: Vec<RawSig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSig {
    kid: String,
    sig: String,
}

impl EpochSigs {
    pub fn parse(bytes: &[u8]) -> RResult<EpochSigs> {
        if bytes.len() > MAX_SIGS_BYTES {
            return Err(RosterError::Oversize("signatures file over 64 KiB"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| RosterError::BadSigs("not UTF-8"))?;
        let raw: RawSigs = serde_json::from_str(text).map_err(|_| RosterError::BadSigs("not the fixed JSON shape"))?;
        if raw.signatures.len() > MAX_SIGNATURES {
            return Err(RosterError::BadSigs("too many signatures"));
        }
        let mut out = Vec::with_capacity(raw.signatures.len());
        for s in raw.signatures {
            if !did::is_valid(&s.kid) {
                return Err(RosterError::BadSigs("kid is not a did:key"));
            }
            // Spelling is checked here; whether it verifies is decided against the epoch's bytes.
            sig::decode_signature(&s.sig).map_err(|_| RosterError::BadSigs("signature spelling"))?;
            out.push((s.kid, s.sig));
        }
        Ok(EpochSigs { epoch: raw.epoch, signatures: out })
    }
}

// ---------------------------------------------------------------------------------------------------------
// Chain rules

/// The epoch hashes an install holds as its trust anchor: its pins, or the one head pair a joiner was handed.
/// Never empty; see [`verify_chain`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor(BTreeMap<u64, String>);

impl Anchor {
    pub fn new(pairs: impl IntoIterator<Item = (u64, String)>) -> RResult<Anchor> {
        let mut map = BTreeMap::new();
        for (n, h) in pairs {
            if n == 0 || !hash::is_valid(&h) {
                return Err(RosterError::BadPins("anchor epoch or hash"));
            }
            if map.insert(n, h).is_some() {
                return Err(RosterError::BadPins("anchor epoch listed twice"));
            }
        }
        if map.is_empty() {
            return Err(RosterError::NoAnchor);
        }
        Ok(Anchor(map))
    }

    pub fn get(&self, epoch: u64) -> Option<&str> {
        self.0.get(&epoch).map(String::as_str)
    }

    pub fn highest(&self) -> u64 {
        *self.0.keys().next_back().expect("an anchor is never empty")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedEpoch {
    pub epoch: Epoch,
    /// `blake3:` of the epoch file's raw bytes: what the next epoch names and what pins record.
    pub hash: String,
}

/// A chain that verified. `epochs[i]` is epoch `i + 1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRoster {
    pub org: String,
    pub store: String,
    pub epochs: Vec<VerifiedEpoch>,
}

impl VerifiedRoster {
    pub fn head(&self) -> &VerifiedEpoch {
        self.epochs.last().expect("a verified chain has at least genesis")
    }
    pub fn head_epoch(&self) -> u64 {
        self.head().epoch.epoch
    }
    pub fn head_hash(&self) -> &str {
        &self.head().hash
    }
    pub fn epoch(&self, n: u64) -> Option<&VerifiedEpoch> {
        usize::try_from(n).ok().and_then(|n| n.checked_sub(1)).and_then(|i| self.epochs.get(i))
    }
    /// The entry for `did` in the head epoch.
    pub fn member(&self, did: &str) -> Option<&Member> {
        self.head().epoch.member(did)
    }
    /// Standing (spec 011 §3.4.3): `active` in the head epoch. Never depends on a timestamp.
    pub fn is_active(&self, did: &str) -> bool {
        self.member(did).is_some_and(Member::is_active)
    }
    pub fn person_of(&self, did: &str) -> Option<&str> {
        self.member(did).map(|m| m.person.as_str())
    }
    /// `did` followed by each successor in turn (spec 011 §3.4.5): the DIDs that may continue `did`'s records.
    pub fn successor_chain(&self, did: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = self.member(did);
        while let Some(m) = cur {
            if out.len() > self.head().epoch.members.len() {
                break;
            }
            out.push(m.did.clone());
            cur = m.successor.as_deref().and_then(|s| self.member(s));
        }
        out
    }
}

/// Verify an ordered list of epoch files and return the roster, or the first rule broken.
///
/// `files[i]` must be epoch `i + 1`. The result is believed only because it contains every epoch hash in
/// `anchor`; the chain's own signatures prove consistency, not authenticity.
pub fn verify_chain(org: &str, store: &str, files: &[EpochFile], anchor: &Anchor) -> RResult<VerifiedRoster> {
    if !is_slug(org, 32) || !is_slug(store, 32) {
        return Err(RosterError::BadScope);
    }
    if files.is_empty() {
        return Err(RosterError::EmptyChain);
    }
    if files.len() > MAX_EPOCHS {
        return Err(RosterError::TooManyEpochs);
    }
    let mut out: Vec<VerifiedEpoch> = Vec::with_capacity(files.len());
    for (i, f) in files.iter().enumerate() {
        let want = i as u64 + 1;
        let epoch = Epoch::parse(&f.epoch_json)?;
        let file_hash = hash::of(&f.epoch_json);
        if epoch.org != org || epoch.store != store {
            return Err(RosterError::WrongScope { epoch: want });
        }
        // Numbering: the list position is the only thing that counts; the number inside the file must agree.
        if epoch.epoch < want {
            let earlier = &out[(epoch.epoch - 1) as usize];
            return Err(if earlier.hash == file_hash { RosterError::Replay { epoch: epoch.epoch } } else { RosterError::Fork { epoch: epoch.epoch } });
        }
        if epoch.epoch > want {
            return Err(RosterError::Gap { expected: want, found: epoch.epoch });
        }
        if let Some(pinned) = anchor.get(want) {
            if pinned != file_hash {
                return Err(RosterError::PinMismatch { epoch: want });
            }
        }
        match out.last() {
            None => {}
            Some(prev) => {
                if epoch.prev_hash.as_deref() != Some(prev.hash.as_str()) {
                    return Err(RosterError::PrevHashMismatch { epoch: want });
                }
                check_transition(&prev.epoch, &epoch)?;
            }
        }
        let sigs = EpochSigs::parse(&f.sigs_json)?;
        if sigs.epoch != want {
            return Err(RosterError::SigsEpochMismatch { epoch: want });
        }
        check_signatures(org, store, &file_hash, &epoch, out.last().map(|p| &p.epoch), &sigs)?;
        out.push(VerifiedEpoch { epoch, hash: file_hash });
    }
    if anchor.highest() > out.len() as u64 {
        // The pins named an epoch this chain does not reach: a stale view, or a chain built to skip it.
        let missing = (out.len() as u64 + 1..=anchor.highest()).find(|n| anchor.get(*n).is_some()).unwrap_or(anchor.highest());
        return Err(RosterError::PinnedEpochMissing { epoch: missing });
    }
    Ok(VerifiedRoster { org: org.to_string(), store: store.to_string(), epochs: out })
}

/// Who may approve `epoch`, whose signatures count, and whether enough people signed.
fn check_signatures(org: &str, store: &str, file_hash: &str, epoch: &Epoch, prev: Option<&Epoch>, sigs: &EpochSigs) -> RResult<()> {
    let domain = Domain::roster(org, store, epoch.epoch, file_hash).map_err(|_| RosterError::BadScope)?;
    // Genesis is approved by its own founders; every later epoch by the previous epoch's `active` admins.
    let approvers = match prev {
        None => epoch.active_admins(),
        Some(p) => p.active_admins(),
    };
    let mut signed_dids = BTreeSet::new();
    let mut signed_people = BTreeSet::new();
    for (kid, signature) in &sigs.signatures {
        // A signature by anyone who is not an approver (a member, a retired or removed key, a stranger) is
        // ignored; so is one that does not verify over exactly these bytes. Duplicates count once.
        let Some(person) = approvers.get(kid.as_str()) else { continue };
        if sig::verify(kid, &domain, signature).is_ok() {
            signed_dids.insert(kid.as_str());
            signed_people.insert(*person);
        }
    }
    match prev {
        None => {
            // Genesis: every founding admin key signs. (Founding admins are all `active`; see check_genesis.)
            if approvers.is_empty() {
                return Err(RosterError::NoActiveAdmin { epoch: epoch.epoch });
            }
            if approvers.keys().any(|d| !signed_dids.contains(d)) {
                return Err(RosterError::GenesisNotFullySigned { epoch: epoch.epoch });
            }
            check_genesis(epoch)
        }
        Some(p) => {
            let n_admin = p.admin_people().len();
            let need = n_admin / 2 + 1;
            if signed_people.len() < need {
                return Err(RosterError::QuorumNotMet { epoch: epoch.epoch, have: signed_people.len(), need });
            }
            Ok(())
        }
    }
}

fn check_genesis(e: &Epoch) -> RResult<()> {
    for m in &e.members {
        if m.status != Status::Active || m.successor.is_some() || m.removed_at_epoch.is_some() || m.voids_tombstones {
            return Err(RosterError::BadNewEntry { epoch: e.epoch });
        }
    }
    Ok(())
}

/// What may change between consecutive epochs (spec 011 §3.2, spec 012 test 13).
///
/// Entries are never dropped; `did`, `person` and `added_at` never change; status only moves
/// `active -> retired -> removed` (a returning human gets a new DID); once an entry is not `active` it is frozen
/// except for those moves, a successor set at the moment it left, and `voids_tombstones` turning on (never off:
/// un-voiding would bring a thief's withdrawals back). New entries start `active` with nothing else set. The
/// epoch must keep at least one active admin.
pub fn check_transition(prev: &Epoch, next: &Epoch) -> RResult<()> {
    let n = next.epoch;
    let next_by: BTreeMap<&str, &Member> = next.members.iter().map(|m| (m.did.as_str(), m)).collect();
    for old in &prev.members {
        let new = next_by.get(old.did.as_str()).ok_or(RosterError::Dropped { epoch: n })?;
        if new.person != old.person {
            return Err(RosterError::PersonChanged { epoch: n });
        }
        if new.added_at != old.added_at {
            return Err(RosterError::ImmutableChanged { epoch: n, field: "added_at" });
        }
        if new.status < old.status {
            return Err(RosterError::StatusRegression { epoch: n });
        }
        if new.status != Status::Active {
            // Leaving (or already left): the entry is frozen as it was while active.
            if new.handle != old.handle {
                return Err(RosterError::ImmutableChanged { epoch: n, field: "handle" });
            }
            if new.roles != old.roles {
                return Err(RosterError::ImmutableChanged { epoch: n, field: "roles" });
            }
            if new.steward_areas != old.steward_areas {
                return Err(RosterError::ImmutableChanged { epoch: n, field: "steward_areas" });
            }
        }
        if old.status != Status::Active && new.successor != old.successor {
            return Err(RosterError::ImmutableChanged { epoch: n, field: "successor" });
        }
        if old.status == Status::Removed && new.removed_at_epoch != old.removed_at_epoch {
            return Err(RosterError::ImmutableChanged { epoch: n, field: "removed_at_epoch" });
        }
        if new.status == Status::Removed && old.status != Status::Removed && new.removed_at_epoch != Some(n) {
            return Err(RosterError::ImmutableChanged { epoch: n, field: "removed_at_epoch" });
        }
        if old.voids_tombstones && !new.voids_tombstones {
            return Err(RosterError::ImmutableChanged { epoch: n, field: "voids_tombstones" });
        }
    }
    let known: BTreeSet<&str> = prev.members.iter().map(|m| m.did.as_str()).collect();
    for m in &next.members {
        if !known.contains(m.did.as_str()) && (m.status != Status::Active || m.successor.is_some() || m.removed_at_epoch.is_some() || m.voids_tombstones) {
            return Err(RosterError::BadNewEntry { epoch: n });
        }
    }
    if next.admin_people().is_empty() {
        return Err(RosterError::NoActiveAdmin { epoch: n });
    }
    Ok(())
}

/// Read the org and store out of the first epoch file, so a joiner who was given only a URL, a fingerprint and a
/// head pair knows which scope to verify. Safe because [`verify_chain`] then checks every file against it and
/// the whole chain against the operator's pin.
pub fn scope_of(files: &[EpochFile]) -> RResult<(String, String)> {
    let first = files.first().ok_or(RosterError::EmptyChain)?;
    let e = Epoch::parse(&first.epoch_json)?;
    Ok((e.org, e.store))
}

/// A sanity check callers use on a hex fingerprint typed by an operator: 64 lowercase hex characters.
pub fn is_fingerprint(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
#[path = "roster_tests.rs"]
mod tests;
