//! The history walk (spec 011 §4.2 check 3) and the real file classifier (spec 011 §3.4, §3.5).
//!
//! Sticky state (`sticky.rs`) compares the **tip** with what this install stored. It cannot see a commit that
//! deleted a tombstone and a later commit that put it back, nor a `rev` that went 2, 5, 3 when the install last
//! stored 2. The walk compares **every commit with each of its parents**, so those are caught too, and on a
//! fresh install (nothing stored) it is the only thing that can tell who first created an `id`.
//!
//! For each commit `C` and each parent `P` (a merge has several, and each is checked: a deletion hidden in a
//! merge shows against the parent that still had the file), for every classified path `C` changed:
//!
//! * deleted: tampering, unless the old file never verified (a junk file nobody could rely on);
//! * a roster epoch file changed: tampering (epochs are immutable);
//! * a record version that no longer verifies where the old one did: tampering;
//! * the signer of the new version is not `active` in the roster epoch in effect at `C` (apart from the
//!   fail-hidden exception for withdrawals, spec 011 §3.4.3): tampering. "In effect" is the highest epoch file in
//!   `C`'s tree, because `signed_at` and `roster_epoch` are the author's and decide nothing;
//! * old and new both verify: [`check_edge`]: the signer is the same or in the old signer's `successor` chain
//!   (a different member overwriting an `id` is a takeover even at a higher `rev`), `rev` is not lower, an equal
//!   `rev` has an identical hash, and the author's own state only moves along `tier::author_transition_allowed`
//!   (nothing hidden by its author becomes `active` again).
//!
//! The signature is verified before anything is read from a file (`read_version` goes through X2's
//! `validate_record`, whose first three steps are the verify-before-parse ones).
//!
//! Git is behind the [`Source`] trait so these rules run over the real `git` in `delphi-core` and over a
//! plain in-memory history in unit tests.

use crate::layout::RecordPath;
use crate::record::{validate_record, AuthorStatus, KindFields, Pinned, TombstoneAction, Verdict};
use crate::roster::VerifiedRoster;
use crate::sticky::Tracking;
use crate::store::{classify_path, PathClass};
use crate::{hash, Result};
use std::collections::{BTreeSet, HashMap};

/// What the walk is verifying against: the install's pinned scope and the roster it verified from the head.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub pinned: Pinned<'a>,
    pub roster: &'a VerifiedRoster,
}

/// One verified version of a record file: who signed it and what it claims, after signature, structure and
/// cross-checks, but with no standing decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub signer: String,
    pub rev: u64,
    /// `blake3:` of the `.md` bytes.
    pub md_hash: String,
    pub status: AuthorStatus,
    /// The record can only hide or demote (a `withdraw` tombstone, an endorsement `withdraw` or `dispute`).
    pub hide_only: bool,
}

/// Verify a record file and its sidecar and say what they are. `Err` is the quarantine slug.
pub fn read_version(ctx: &Ctx, path: &str, md: &[u8], sidecar: &[u8]) -> std::result::Result<Version, &'static str> {
    let v = validate_record(ctx.pinned, path, md, sidecar, &|_| Ok(())).map_err(|r| r.reason())?;
    let (rec, car) = (v.record(), v.sidecar());
    let hide_only = match &rec.kind_fields {
        KindFields::Tombstone(t) => t.action == TombstoneAction::Withdraw,
        KindFields::Endorsement(e) => matches!(e.verdict, Verdict::Withdraw | Verdict::Dispute),
        KindFields::Memory(_) => false,
    };
    if rec.rev == 0 {
        return Err("rev-is-zero");
    }
    Ok(Version { signer: car.kid.clone(), rev: rec.rev, md_hash: hash::of(md), status: rec.status, hide_only })
}

/// Spec 011 §3.4 step 3 against roster epoch `epoch`: the signer is `active` there. The one exception is
/// fail-hidden: a version that can only hide or demote still counts from a `retired` or `removed` DID, unless the
/// entry sets `voids_tombstones` (mandatory after a compromise, spec 011 §8.2).
pub fn standing(roster: &VerifiedRoster, epoch: u64, v: &Version) -> std::result::Result<(), &'static str> {
    let Some(ve) = roster.epoch(epoch) else {
        return Err("no-roster");
    };
    let Some(m) = ve.epoch.member(&v.signer) else {
        return Err("not-a-member");
    };
    if m.is_active() || (v.hide_only && !m.voids_tombstones) {
        Ok(())
    } else {
        Err("not-active")
    }
}

/// Why a new version may not follow an old one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeBreak {
    /// A member who is not the owner (or in the owner's `successor` chain) took over the `id`.
    Takeover,
    Rollback,
    Swapped,
    /// An author-hidden record (`withdrawn` or `superseded`) came back to `active`, or a `withdrawn` one changed
    /// state at all (spec 012 test 7 and `tier::author_transition_allowed`).
    StateRegression,
}

impl EdgeBreak {
    pub fn reason(self) -> &'static str {
        match self {
            EdgeBreak::Takeover => "takeover",
            EdgeBreak::Rollback => "rev-rollback",
            EdgeBreak::Swapped => "same-rev-different-hash",
            EdgeBreak::StateRegression => "author-state-regression",
        }
    }
}

/// The rule for one step of one path's history. Signer first: a takeover is reported as a takeover even when
/// its `rev` is also wrong.
pub fn check_edge(old: &Version, new: &Version, roster: &VerifiedRoster) -> std::result::Result<(), EdgeBreak> {
    if new.signer != old.signer && !roster.successor_chain(&old.signer).contains(&new.signer) {
        return Err(EdgeBreak::Takeover);
    }
    if new.rev < old.rev {
        return Err(EdgeBreak::Rollback);
    }
    if new.rev == old.rev && new.md_hash != old.md_hash {
        return Err(EdgeBreak::Swapped);
    }
    if !crate::tier::author_transition_allowed(old.status, new.status) {
        return Err(EdgeBreak::StateRegression);
    }
    Ok(())
}

/// The real classifier for `Store::sync`: sticky ordering from the path, and nothing is tracked that did not
/// verify (signature first, then structure). `lookup` finds other files of the same snapshot (a record's
/// sidecar). A file that fails returns its quarantine slug and so can never raise a stored `rev` and lock the
/// real record out.
///
/// Standing in the **head** epoch (spec 011 §3.4 step 3) decides whether a verified record is *in force*, not
/// whether it is tracked: a record whose signer is a roster member that is no longer `active` is still ordered
/// by sticky state (so a rollback of it is still tampering, and retiring a member does not read as tampering)
/// but comes back with a quarantine note. A signer who is not in the roster at all is not tracked.
pub fn classify<'f>(ctx: &Ctx, path: &str, bytes: &[u8], lookup: &dyn Fn(&str) -> Option<&'f [u8]>) -> std::result::Result<(Tracking, Option<&'static str>), &'static str> {
    match classify_path(path).ok_or("unknown-path")? {
        PathClass::Record(rp) => {
            let sidecar = lookup(&rp.sidecar()).ok_or("missing-sidecar")?;
            let v = read_version(ctx, path, bytes, sidecar)?;
            match standing(ctx.roster, ctx.roster.head_epoch(), &v) {
                Ok(()) => Ok((Tracking::Rev(v.rev), None)),
                Err("not-active") => Ok((Tracking::Rev(v.rev), Some("not-active"))),
                Err(other) => Err(other),
            }
        }
        // An epoch file is tracked only if it is exactly the one the verified chain holds under that number.
        PathClass::EpochFile(n) => match ctx.roster.epoch(n) {
            Some(ve) if ve.hash == hash::of(bytes) => Ok((Tracking::Immutable, None)),
            _ => Err("not-in-verified-chain"),
        },
        PathClass::Sidecar | PathClass::EpochSigs | PathClass::Freshness | PathClass::FreshnessSigs => Ok((Tracking::Present, None)),
    }
}

// ---------------------------------------------------------------------------------------------------------
// The walk

/// A commit and its parents, as the history source reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub id: String,
    pub parents: Vec<String>,
}

/// One path that differs between a parent and a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub existed_before: bool,
    pub exists_now: bool,
    /// The git mode of the new entry (`0` when deleted); only `0o100644` is acceptable.
    pub new_mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blob {
    Missing,
    /// Present but over the size the caller allows to be read.
    TooBig,
    Bytes(Vec<u8>),
}

impl Blob {
    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Blob::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

/// Where history comes from. `delphi-core` implements it over the clone with the hardened git runner.
pub trait Source {
    /// Every commit reachable from `head` and not from `base`, parents before children.
    fn commits(&self, base: &str, head: &str) -> Result<Vec<CommitInfo>>;
    /// Paths under `records/` and `roster/` that differ between `parent` (`None` for a root commit) and `commit`.
    fn changes(&self, parent: Option<&str>, commit: &str) -> Result<Vec<Change>>;
    /// The files at `commit`, in the order asked, each read at most `max_bytes`.
    fn read(&self, commit: &str, paths: &[String], max_bytes: usize) -> Result<Vec<Blob>>;
    /// The highest `roster/epochs/<n>.json` present in `commit`'s tree (0 when none).
    fn epoch_in_tree(&self, commit: &str) -> Result<u64>;
}

/// Something the walk found. `reason` is a fixed slug; `path` is a store path already validated by the layout
/// (an unclassified path is never reported), `commit` is a commit id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub path: String,
    pub commit: String,
    pub reason: &'static str,
}

/// Most findings reported: a hostile history can make millions, and the first few are enough to refuse.
pub const MAX_FINDINGS: usize = 200;

const RECORD_READ_MAX: usize = crate::sidecar::MAX_RECORD_BYTES;
const EPOCH_READ_MAX: usize = crate::roster::MAX_EPOCH_BYTES;

/// Walk `base..head` and report every violation of the rules in the module docs. An empty result means the
/// history is clean. `base` is the last accepted head, or on a fresh install the admin-signed `store_head`
/// (history up to it is covered by that signature, spec 011 §4.2).
pub fn walk(src: &dyn Source, ctx: &Ctx, base: &str, head: &str) -> Result<Vec<Finding>> {
    let commits = src.commits(base, head)?;
    let mut findings: BTreeSet<Finding> = BTreeSet::new();
    let mut epochs: HashMap<String, u64> = HashMap::new();
    // A version read at a commit is read again as the "old" side of every child; reading it once is enough.
    let mut versions: VersionCache = HashMap::new();
    let tree_epoch = |id: &str, epochs: &mut HashMap<String, u64>| -> Result<u64> {
        if let Some(e) = epochs.get(id) {
            return Ok(*e);
        }
        let e = src.epoch_in_tree(id)?;
        epochs.insert(id.to_string(), e);
        Ok(e)
    };
    for c in &commits {
        let parent_ids: Vec<Option<&str>> = if c.parents.is_empty() { vec![None] } else { c.parents.iter().map(|p| Some(p.as_str())).collect() };
        let mut epoch = 0u64;
        for p in c.parents.iter() {
            epoch = epoch.max(tree_epoch(p, &mut epochs)?);
        }
        let diffs: Vec<(Option<&str>, Vec<Change>)> = parent_ids.iter().map(|p| src.changes(*p, &c.id).map(|d| (*p, d))).collect::<Result<_>>()?;
        let mut flag = |path: &str, reason: &'static str| {
            if findings.len() < MAX_FINDINGS {
                findings.insert(Finding { path: path.to_string(), commit: c.id.clone(), reason });
            }
        };
        // Roster epoch files first: they set the epoch in effect for this commit's record changes.
        for (_, changes) in &diffs {
            for ch in changes {
                if let Some(PathClass::EpochFile(n)) = classify_path(&ch.path) {
                    if ch.exists_now && ch.existed_before {
                        flag(&ch.path, "epoch-file-changed");
                    } else if ch.exists_now {
                        let blob = src.read(&c.id, std::slice::from_ref(&ch.path), EPOCH_READ_MAX)?;
                        match (ctx.roster.epoch(n), blob[0].bytes()) {
                            (Some(ve), Some(b)) if ve.hash == hash::of(b) => epoch = epoch.max(n),
                            _ => flag(&ch.path, "epoch-differs-from-chain"),
                        }
                    }
                }
            }
        }
        epochs.insert(c.id.clone(), epoch);
        for (parent, changes) in &diffs {
            // Records touched in this diff: a changed sidecar re-opens its record too.
            let mut touched: BTreeSet<String> = BTreeSet::new();
            for ch in changes {
                let Some(class) = classify_path(&ch.path) else { continue };
                if ch.exists_now && ch.new_mode != 0o100644 {
                    flag(&ch.path, "non-regular-file");
                    continue;
                }
                match class {
                    PathClass::Record(rp) => {
                        touched.insert(rp.record());
                    }
                    PathClass::Sidecar => {
                        if !ch.exists_now && ch.existed_before {
                            flag(&ch.path, "deleted");
                        }
                        if let Some(md) = sidecar_to_record(&ch.path) {
                            touched.insert(md);
                        }
                    }
                    PathClass::EpochFile(_) => {
                        if !ch.exists_now && ch.existed_before {
                            flag(&ch.path, "deleted");
                        }
                    }
                    PathClass::EpochSigs | PathClass::Freshness | PathClass::FreshnessSigs => {
                        if !ch.exists_now && ch.existed_before {
                            flag(&ch.path, "deleted");
                        }
                    }
                }
            }
            for md in touched {
                let rp = RecordPath::parse(&md).expect("a path built from a RecordPath");
                let names = [md.clone(), rp.sidecar()];
                let old_v = match parent {
                    Some(p) => version_at(src, ctx, &mut versions, p, &md, &names)?,
                    None => Err("absent"),
                };
                let new_v = version_at(src, ctx, &mut versions, &c.id, &md, &names)?;
                match (&old_v, &new_v) {
                    (Ok(_), Err("absent")) => flag(&md, "deleted"),
                    (Ok(_), Err(_)) => flag(&md, "no-longer-verifies"),
                    (_, Err(_)) => {}
                    (old, Ok(nv)) => {
                        if standing(ctx.roster, epoch, nv).is_err() {
                            flag(&md, "signer-not-active");
                        }
                        if let Ok(ov) = old {
                            if let Err(b) = check_edge(ov, nv, ctx.roster) {
                                flag(&md, b.reason());
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(findings.into_iter().collect())
}

/// `records/<kind>/<id>.sig.json` to `records/<kind>/<id>.md`.
fn sidecar_to_record(sidecar: &str) -> Option<String> {
    let stem = sidecar.strip_suffix(".sig.json")?;
    let md = format!("{stem}.md");
    RecordPath::parse(&md).ok().map(|_| md)
}

type VersionResult = std::result::Result<Version, &'static str>;
type VersionCache = HashMap<(String, String), VersionResult>;

fn version_at(src: &dyn Source, ctx: &Ctx, cache: &mut VersionCache, commit: &str, md_path: &str, names: &[String; 2]) -> Result<VersionResult> {
    let key = (commit.to_string(), md_path.to_string());
    if let Some(v) = cache.get(&key) {
        return Ok(v.clone());
    }
    let v = version_uncached(src, ctx, commit, md_path, names)?;
    cache.insert(key, v.clone());
    Ok(v)
}

fn version_uncached(src: &dyn Source, ctx: &Ctx, commit: &str, md_path: &str, names: &[String; 2]) -> Result<VersionResult> {
    let blobs = src.read(commit, names, RECORD_READ_MAX)?;
    Ok(match (&blobs[0], &blobs[1]) {
        (Blob::Missing, _) => Err("absent"),
        (Blob::TooBig, _) | (_, Blob::TooBig) => Err("oversize"),
        (_, Blob::Missing) => Err("missing-sidecar"),
        (Blob::Bytes(md), Blob::Bytes(car)) => read_version(ctx, md_path, md, car),
    })
}
