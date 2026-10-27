//! From an accepted Xenia head to the rows the knowledge base will hold (spec 011 §5.1, §7.2; spec 012 §8; milestone
//! X6). Everything here is a derivation: [`derive`] reads the **accepted** state of one store (the head in the sticky
//! file, the roster against the pins, the freshness statement against the clock) and returns what the index zone
//! should contain. It writes nothing but the tag history, and that only through [`TagHistory::save`], which the caller
//! calls after the zone has been updated.
//!
//! What it trusts, and what it does not:
//!
//! * **Nothing from the sync result is taken on faith.** The caller says "this sync was accepted"; the bytes come
//!   from git objects of the accepted head, every record is verified again (`validate_record`: hash, signature,
//!   structure, limits), every record must be the one the sticky state tracks (same hash), and the tier is computed
//!   by X5's pure [`tier::compute`] from the verified roster and the freshness state judged against the caller's
//!   clock. A record can never raise its own tier: the six trust keys are refused by X2 and no field of a record is
//!   read as a tier here.
//! * **`prior_tags` is install-side.** X5's tombstone area check needs the union of a record's tags over all its
//!   revisions, so an author cannot retag to escape a steward. [`TagHistory`] keeps that union, per record id and per
//!   signing DID, in `state/prior-tags.json` outside the clone. It is filled only from versions that verify
//!   (`validate_record`) and only from the git history of this store; it is never read from a store file, it only
//!   grows, and a version signed by a DID that is not the same person as the current signer is not counted (so a
//!   member cannot widen a victim's tags by committing a record of their own at the victim's id). Lose the file and
//!   the whole history is walked again; a damaged one is an error, never an empty history.
//! * **Text is hostile.** Titles, summaries, bodies, tags and pointer labels go through the 007 §3.2 sanitizer
//!   ([`xenia_ingress`]) on the way in. Only title, summary and tags are searchable; the body is kept apart for the
//!   quarantine read.

use crate::provenance::{Index, Tier as LocalTier, Zone};
use crate::xenia_ingress::{one_line, sanitize_counted, Stripped};
use crate::xenia_store::{io, Store};
use crate::xenia_sync::{epoch_files, rerr, statement_files, GitSource, VerifyConfig};
use delphi_xenia::freshness::{self, FreshnessState};
use delphi_xenia::layout::Kind;
use delphi_xenia::record::{validate_record, Confidence, KindFields, TombstoneAction, TombstoneReason};
use delphi_xenia::roster::VerifiedRoster;
use delphi_xenia::store::{classify_path, PathClass};
pub use delphi_xenia::tier::Tier;
use delphi_xenia::tier::{self, HiddenBy, Inputs, Outcome, Visibility, VerifiedRecord};
use delphi_xenia::time::parse_utc;
use delphi_xenia::walk::{Blob, Source};
use delphi_xenia::{hash, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;

const TAG_FILE_VERSION: u32 = 1;
const TAG_FILE_NAME: &str = "prior-tags.json";
/// Bounds on the tag history (a hostile author can publish many revisions): past them nothing more is recorded for
/// that id, which can only make the union smaller for an attacker's own record.
const MAX_IDS: usize = 20_000;
const MAX_DIDS_PER_ID: usize = 16;
const MAX_TAGS_PER_DID: usize = 512;
const MAX_TAG_FILE: u64 = 16 << 20;

// ---------------------------------------------------------------------------------------------------------
// The tag history (the persisted `prior_tags`)

type Tags = BTreeMap<String, BTreeMap<String, BTreeSet<String>>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagHistory {
    v: u32,
    org: String,
    store: String,
    /// The accepted head this history is complete through: every version in the history of this commit is in `ids`.
    through: String,
    /// Record id, then signing DID, then every tag that DID's verified revisions carried.
    ids: Tags,
}

impl TagHistory {
    fn empty(store: &Store, through: &str) -> TagHistory {
        TagHistory { v: TAG_FILE_VERSION, org: store.org.clone(), store: store.store.clone(), through: through.to_string(), ids: Tags::new() }
    }

    fn path(store: &Store) -> std::path::PathBuf {
        store.paths.state.join(TAG_FILE_NAME)
    }

    /// `None` when there is no file (a fresh install, or a lost file: the caller walks the whole history). A file
    /// that is damaged, of another store, readable by others or not a plain file is an error and never "empty".
    pub fn load(store: &Store) -> Result<Option<TagHistory>> {
        let p = Self::path(store);
        let m = match fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(e)),
        };
        if !m.file_type().is_file() {
            return Err(Error::Key("the tag history path is not a regular file"));
        }
        if m.permissions().mode() & 0o077 != 0 {
            return Err(Error::Key("the tag history file is accessible to others (expected mode 0600)"));
        }
        if m.len() > MAX_TAG_FILE {
            return Err(Error::Oversize("the tag history file is too large"));
        }
        let h: TagHistory = serde_json::from_slice(&fs::read(&p).map_err(io)?).map_err(|_| Error::BadField("the tag history file is damaged"))?;
        if h.v != TAG_FILE_VERSION || h.org != store.org || h.store != store.store || !delphi_xenia::sticky::is_commit_id(&h.through) {
            return Err(Error::BadField("the tag history file is not this store's"));
        }
        if h.ids.len() > MAX_IDS || h.ids.values().any(|d| d.len() > MAX_DIDS_PER_ID || d.values().any(|t| t.len() > MAX_TAGS_PER_DID)) {
            return Err(Error::BadField("the tag history file is over its bounds"));
        }
        Ok(Some(h))
    }

    /// Atomic, `0600`, flushed (X3's `write_atomic`, which also refuses a symlink at the target).
    pub fn save(&self, store: &Store) -> Result<()> {
        crate::xenia_store::private_dir(&store.paths.state)?;
        let bytes = serde_json::to_vec(self).map_err(|_| Error::BadField("tag history does not serialize"))?;
        delphi_xenia::pins::write_atomic(&Self::path(store), &bytes).map_err(rerr)
    }

    pub fn through(&self) -> &str {
        &self.through
    }

    /// Add what was seen. Never removes anything.
    fn absorb(&mut self, seen: Tags) {
        for (id, by_did) in seen {
            if !self.ids.contains_key(&id) && self.ids.len() >= MAX_IDS {
                continue;
            }
            let mine = self.ids.entry(id).or_default();
            for (did, tags) in by_did {
                if !mine.contains_key(&did) && mine.len() >= MAX_DIDS_PER_ID {
                    continue;
                }
                let t = mine.entry(did).or_default();
                for tag in tags {
                    if t.len() < MAX_TAGS_PER_DID {
                        t.insert(tag);
                    }
                }
            }
        }
    }

    /// The union of tags over every revision signed by the same person as `owner`, for [`tier::Inputs::prior_tags`].
    fn owner_union(&self, roster: &VerifiedRoster, id: &str, owner: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for (did, tags) in self.ids.get(id).into_iter().flatten() {
            if same_person(roster, did, owner) {
                out.extend(tags.iter().cloned());
            }
        }
        out
    }
}

/// Same human (spec 011 §2.1): equal `person`, or one DID in the other's `successor` chain.
fn same_person(roster: &VerifiedRoster, a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (roster.person_of(a), roster.person_of(b)) {
        (Some(x), Some(y)) => x == y,
        _ => roster.successor_chain(a).iter().any(|d| d == b) || roster.successor_chain(b).iter().any(|d| d == a),
    }
}

/// Every verified `memory` version in the commits `base..head` (or in all of `head`'s history when `base` is `None`),
/// as `id -> signer -> tags`. A version that does not verify contributes nothing. Read straight from git objects.
fn harvest(store: &Store, base: Option<&str>, head: &str) -> Result<Tags> {
    let src = GitSource::new(store);
    let commits = match base {
        Some(b) => src.commits(b, head)?,
        None => src.all_commits(head)?,
    };
    let pinned = store.pinned();
    let mut out = Tags::new();
    let mut done: BTreeSet<(String, String)> = BTreeSet::new();
    for c in &commits {
        let parents: Vec<Option<&str>> = if c.parents.is_empty() { vec![None] } else { c.parents.iter().map(|p| Some(p.as_str())).collect() };
        for parent in parents {
            for ch in src.changes(parent, &c.id)? {
                let Some(PathClass::Record(rp)) = classify_path(&ch.path) else { continue };
                if !rp.kind.is_target() || !ch.exists_now || !done.insert((c.id.clone(), ch.path.clone())) {
                    continue;
                }
                let sidecar = rp.sidecar();
                let blobs = src.read(&c.id, &[ch.path.clone(), sidecar], delphi_xenia::sidecar::MAX_RECORD_BYTES)?;
                let (Some(Blob::Bytes(md)), Some(Blob::Bytes(car))) = (blobs.first(), blobs.get(1)) else { continue };
                if let Ok(v) = validate_record(pinned, &ch.path, md, car, &|_| Ok(())) {
                    out.entry(v.record().id.clone()).or_default().entry(v.sidecar().kid.clone()).or_default().extend(v.record().tags.iter().cloned());
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------
// What the zone holds

/// One pointer as shown to a human: display text only, never fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerView {
    pub scheme: &'static str,
    pub locator: String,
    pub label: String,
}

/// One `memory` record in force at the accepted head, ready for the zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedMemory {
    /// The knowledge-base path: `xenia/<org>/<store>/memory/<id>`.
    pub rel_path: String,
    pub org: String,
    pub store: String,
    pub id: String,
    pub rev: u64,
    /// `blake3:` of the record file (the content key of a provenance stamp).
    pub hash: String,
    pub signer: String,
    pub author_person: String,
    /// The tier consumers use (`L0` while hidden).
    pub tier: Tier,
    /// `federated_verified`, `federated_peer` or `external_untrusted`: from the computed tier, never from the record.
    pub source_type: &'static str,
    /// `active` or `superseded` (spec 012 §8).
    pub status: &'static str,
    pub superseded_by: Option<String>,
    /// Why it is hidden (a fixed slug), if it is.
    pub hidden: Option<&'static str>,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub tags: Vec<String>,
    pub pointers: Vec<PointerView>,
    pub confidence: &'static str,
    pub author_role: &'static str,
    pub updated: String,
    pub ttl_missions: Option<u32>,
    pub endorsed_by: Vec<String>,
    pub withheld_by_staleness: Vec<String>,
    pub disputed: bool,
    pub clamped_by_ceiling: bool,
    /// Unix seconds until which an `L2` rests on a current freshness statement. A reader past it treats the record
    /// as `L1` even if nobody has re-derived the zone since.
    pub fresh_until: Option<i64>,
}

impl IndexedMemory {
    /// The local id of spec 012 §8: `xenia:<org>/<store>/memory/<id>@<rev>`.
    pub fn local_id(&self) -> String {
        format!("xenia:{}/{}/memory/{}@{}", self.org, self.store, self.id, self.rev)
    }

    /// The local cache entry (spec 012 §8): the provenance frontmatter, then title, summary and tags. **No body**: the
    /// body never reaches the chunks, so it can never reach an agent through a search preview. Every string is
    /// JSON-quoted (valid YAML), so no text can end the frontmatter block or add a key.
    pub fn cache_entry(&self) -> String {
        let q = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
        let mut t = String::from("---\n");
        t.push_str(&format!("id: {}\n", q(&self.local_id())));
        t.push_str(&format!("title: {}\n", q(&self.title)));
        t.push_str(&format!("author: {}\n", self.author_role));
        t.push_str(&format!("timestamp: {}\n", q(&self.updated)));
        t.push_str(&format!("source_type: {}\n", self.source_type));
        t.push_str(&format!("confidence: {}\n", self.confidence));
        t.push_str(&format!("status: {}\n", self.status));
        if let Some(by) = &self.superseded_by {
            t.push_str(&format!("superseded_by: {}\n", q(by)));
        }
        t.push_str(&format!("source_mission: {}\n", q(&format!("xenia:{}", self.org))));
        t.push_str(&format!("tags: [{}]\n", self.tags.iter().map(|x| q(x)).collect::<Vec<_>>().join(", ")));
        if let Some(n) = self.ttl_missions {
            t.push_str(&format!("ttl_missions: {n}\n"));
        }
        t.push_str("---\n");
        t.push_str(&format!("# {}\n\n{}\n", self.title, self.summary));
        if !self.tags.is_empty() {
            t.push_str(&format!("\nTags: {}\n", self.tags.join(", ")));
        }
        t
    }

    /// The `provenance.stamp` this record earns: zone, path inside the zone, content hash (hex) and tier.
    pub fn stamp(&self, head: &str) -> Stamp {
        Stamp {
            path: format!("{}/{}/memory/{}", self.org, self.store, self.id),
            blake3: self.hash.trim_start_matches("blake3:").to_string(),
            tier: match self.tier {
                Tier::L0 => LocalTier::L0,
                Tier::L1 => LocalTier::L1,
                Tier::L2 => LocalTier::L2,
            },
            reason: format!("xenia:{head}:rev{}", self.rev),
        }
    }
}

/// A stamp for the daemon to record as a `provenance.stamp` event (`Index::stamp_payload_with_reason`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub path: String,
    pub blake3: String,
    pub tier: LocalTier,
    pub reason: String,
}

impl Stamp {
    pub fn payload(&self) -> serde_json::Value {
        Index::stamp_payload_with_reason(Zone::Xenia, &self.path, &self.blake3, self.tier, &self.reason)
    }
}

/// Everything [`derive`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    pub org: String,
    pub store: String,
    pub head: String,
    pub freshness: FreshnessState,
    /// Sorted by id.
    pub memories: Vec<IndexedMemory>,
    /// The `bundle` records (spec 027 §4.3), sorted by id. Never indexed as notes: `delphi bundle add --from-xenia`
    /// reads them (see [`crate::xenia_bundle`]).
    pub bundles: Vec<crate::xenia_bundle::StoreBundle>,
    /// Records (any kind) that are not in force, with a fixed slug, for `status`. Never indexed.
    pub held_back: Vec<(String, &'static str)>,
    /// What the sanitizer removed across all text, for the report.
    pub stripped: Stripped,
    history: TagHistory,
}

impl Derived {
    /// `fresh`, or the fixed reason endorsements do not count (`expired`, `missing`, `replayed-statement`, ...).
    pub fn freshness_slug(&self) -> String {
        match &self.freshness {
            FreshnessState::Fresh { .. } => "fresh".into(),
            FreshnessState::Stale { reason } => (*reason).to_string(),
        }
    }

    pub fn stamps(&self) -> Vec<Stamp> {
        self.memories.iter().map(|m| m.stamp(&self.head)).collect()
    }

    /// Persist the tag history. Call it after the zone was updated: a crash in between only costs a longer walk next
    /// time (the history is complete through `through`, and is still correct if it is older).
    pub fn save_history(&self, store: &Store) -> Result<()> {
        self.history.save(store)
    }

    pub fn history(&self) -> &TagHistory {
        &self.history
    }
}

// ---------------------------------------------------------------------------------------------------------
// The local mapping (spec 012 §8; 012 test 23)

fn hidden_slug(h: &HiddenBy) -> &'static str {
    match h {
        HiddenBy::AuthorSuperseded => "author-superseded",
        HiddenBy::AuthorWithdrawn => "author-withdrawn",
        HiddenBy::AuthorTombstone => "author-tombstone",
        HiddenBy::StewardTombstone => "steward-tombstone",
        HiddenBy::SensitiveHold => "sensitive-hold",
        HiddenBy::Contested(_) => "contested",
    }
}

fn confidence_cap(c: Confidence) -> &'static str {
    // Spec 012 §8: capped at `medium` unless a local replay makes it `verified`. Nothing replays in Phase 1.
    match c {
        Confidence::Low => "low",
        Confidence::Medium | Confidence::High | Confidence::Verified => "medium",
    }
}

fn parse_date(d: &str) -> Option<i64> {
    parse_utc(&format!("{d}T00:00:00Z"))
}

/// Compute the zone's content for the accepted head of `store`, judged at `cfg.now`.
pub fn derive(store: &Store, cfg: &VerifyConfig) -> Result<Derived> {
    let pins = store.load_pins()?;
    let sticky = store.load_sticky()?;
    let head = sticky.accepted_head.clone().ok_or(Error::BadField("nothing has been accepted yet"))?;
    let snap = store.snapshot_of(&head)?;
    let files = epoch_files(&snap);
    if files.is_empty() {
        return Err(Error::BadField("the accepted head has no roster"));
    }
    let roster = pins.verify(&files).map_err(rerr)?;
    let assessment = freshness::assess_after(&store.org, &store.store, statement_files(&snap), &roster, cfg.now, &cfg.policy, sticky.freshness_mark.as_ref());

    // The records: verified again from the bytes, and only the ones the sync tracked, byte for byte.
    let pinned = store.pinned();
    let (mut verified, mut held): (Vec<VerifiedRecord>, Vec<(String, &'static str)>) = (Vec::new(), Vec::new());
    for (path, md) in snap.files.iter().filter(|(p, _)| p.starts_with("records/") && p.ends_with(".md")) {
        let Some(PathClass::Record(rp)) = classify_path(path) else {
            held.push((path.clone(), "unknown-path"));
            continue;
        };
        let tracked = sticky.paths.get(path).is_some_and(|e| e.hash == hash::of(md));
        let Some(car) = snap.files.get(&rp.sidecar()).filter(|_| tracked) else {
            held.push((path.clone(), if tracked { "missing-sidecar" } else { "not-tracked" }));
            continue;
        };
        match validate_record(pinned, path, md, car, &|_| Ok(())) {
            Ok(v) => verified.push(VerifiedRecord::from_validated(&v)),
            Err(r) => held.push((path.clone(), r.reason())),
        }
    }

    // prior_tags: from the verified history, kept install-side, never from a store file.
    let saved = TagHistory::load(store)?;
    let mut history = match saved {
        Some(h) if h.through == head => h,
        Some(h) if store.is_ancestor_or_equal(&h.through, &head)? => {
            let seen = harvest(store, Some(&h.through), &head)?;
            let mut n = h;
            n.absorb(seen);
            n.through = head.clone();
            n
        }
        // No file, or one that does not lead to this head (a rewrite the operator approved): walk it all, and keep
        // whatever the old file knew (the union only ever grows).
        other => {
            let mut n = TagHistory::empty(store, &head);
            if let Some(old) = other {
                n.absorb(old.ids);
            }
            n.absorb(harvest(store, None, &head)?);
            n
        }
    };
    // The tip itself always counts, whatever the walk found.
    let mut tip: Tags = Tags::new();
    for r in verified.iter().filter(|r| r.record().kind().is_target()) {
        tip.entry(r.record().id.clone()).or_default().entry(r.signer().to_string()).or_default().extend(r.record().tags.iter().cloned());
    }
    history.absorb(tip);
    history.through = head.clone();
    let mut prior: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for r in verified.iter().filter(|r| r.record().kind().is_target()) {
        prior.insert(r.record().id.clone(), history.owner_union(&roster, &r.record().id, r.signer()));
    }

    let computed = tier::compute(&Inputs { roster: &roster, records: &verified, freshness: &assessment.state, prior_tags: &prior });
    for (path, out) in &computed.outcomes {
        match out {
            Outcome::Effective => {}
            Outcome::NoEffect(why) | Outcome::Quarantined(why) => held.push((path.clone(), why)),
        }
    }
    let fresh_until = match &assessment.state {
        FreshnessState::Fresh { expires_at, .. } => parse_utc(expires_at),
        FreshnessState::Stale { .. } => None,
    };

    let by_id: BTreeMap<&str, &VerifiedRecord> = verified.iter().map(|r| (r.record().id.as_str(), r)).collect();
    let mut stripped = Stripped::default();
    let mut memories = Vec::new();
    let mut bundles = Vec::new();
    for (id, st) in &computed.memories {
        let Some(vr) = by_id.get(id.as_str()) else { continue };
        let rec = vr.record();
        // A `bundle` record is endorsed and hidden like a memory but is not a note: it is read through its own path.
        if rec.kind() != Kind::Memory {
            if let Some(b) = crate::xenia_bundle::store_bundle(&roster, vr, st) {
                bundles.push(b);
            }
            continue;
        }
        let (status, hidden) = match &st.visibility {
            Visibility::Visible => ("active", None),
            Visibility::Hidden(h) => ("superseded", Some(hidden_slug(h))),
        };
        let superseded_by = st.visibility_hidden_by().map(|h| superseded_by(h, &roster, &verified, &computed.memories, &computed.outcomes, rec, vr.signer()));
        let mut clean = |s: &str, multi: bool| {
            let (t, st) = sanitize_counted(s);
            stripped.merge(st);
            if multi { t } else { t.split_whitespace().collect::<Vec<_>>().join(" ") }
        };
        let title = clean(&rec.title, false);
        let summary = clean(&rec.summary, false);
        let body = clean(&rec.body, true);
        let tags: Vec<String> = rec.tags.iter().map(|t| clean(t, false)).collect();
        let pointers = rec.pointers.iter().map(|p| PointerView { scheme: p.scheme.as_str(), locator: clean(&p.locator, false), label: clean(&p.label, false) }).collect();
        let ttl = rec.review_by.as_deref().and_then(parse_date).map(|t| ((t - cfg.now).max(0) / 86_400).min(3650) as u32);
        let tier = st.tier;
        memories.push(IndexedMemory {
            rel_path: format!("xenia/{}/{}/memory/{}", store.org, store.store, id),
            org: store.org.clone(),
            store: store.store.clone(),
            id: id.clone(),
            rev: st.rev,
            hash: st.hash.clone(),
            signer: st.signer.clone(),
            author_person: st.author_person.clone(),
            tier,
            source_type: tier.source_type(),
            status,
            superseded_by: superseded_by.flatten(),
            hidden,
            title,
            summary,
            body,
            tags,
            pointers,
            confidence: confidence_cap(rec.confidence),
            author_role: rec.author_role.as_str(),
            updated: rec.updated.clone(),
            ttl_missions: ttl,
            endorsed_by: st.endorsed_by.iter().map(|e| one_line(&e.person)).collect(),
            withheld_by_staleness: st.withheld_by_staleness.iter().map(|e| one_line(&e.person)).collect(),
            disputed: st.disputed(),
            clamped_by_ceiling: st.clamped_by_ceiling,
            fresh_until: if tier == Tier::L2 { fresh_until } else { None },
        });
    }
    held.sort();
    held.dedup();
    Ok(Derived { org: store.org.clone(), store: store.store.clone(), head, freshness: assessment.state, memories, bundles, held_back: held, stripped, history })
}

trait HiddenView {
    fn visibility_hidden_by(&self) -> Option<&HiddenBy>;
}

impl HiddenView for tier::MemoryState {
    fn visibility_hidden_by(&self) -> Option<&HiddenBy> {
        match &self.visibility {
            Visibility::Hidden(h) => Some(h),
            Visibility::Visible => None,
        }
    }
}

/// `superseded_by` (spec 012 §8): the local id of the superseding record if one by the same person names this
/// record in `supersedes`; otherwise `tombstone:<id>`, `author-withdrawn` or `contested`. A record by someone else
/// that claims to supersede this one is a display link only (X5) and is not used here. For an author-superseded
/// record that nothing in the store claims to replace, the spec gives no spelling; `author-superseded`.
fn superseded_by(
    h: &HiddenBy,
    roster: &VerifiedRoster,
    all: &[VerifiedRecord],
    memories: &BTreeMap<String, tier::MemoryState>,
    outcomes: &BTreeMap<String, Outcome>,
    rec: &delphi_xenia::record::Record,
    signer: &str,
) -> Option<String> {
    let successor = || {
        all.iter()
            .filter(|r| r.record().kind() == Kind::Memory && r.record().supersedes.as_ref().is_some_and(|(sid, _)| *sid == rec.id) && same_person(roster, r.signer(), signer))
            .filter(|r| memories.get(&r.record().id).is_some_and(|m| m.visibility == Visibility::Visible))
            .map(|r| r.record().id.clone())
            .min()
            .and_then(|id| memories.get(&id).map(|m| format!("xenia:{}/{}/memory/{}@{}", rec.org, rec.store, id, m.rev)))
    };
    let tombstone = |sensitive_first: bool| {
        let mut ids: Vec<(bool, String)> = all
            .iter()
            .filter_map(|r| match &r.record().kind_fields {
                KindFields::Tombstone(t) if t.target == rec.id && t.action == TombstoneAction::Withdraw && outcomes.get(&r.path()) == Some(&Outcome::Effective) => {
                    Some((!(sensitive_first && t.reason == TombstoneReason::Sensitive), r.record().id.clone()))
                }
                _ => None,
            })
            .collect();
        ids.sort();
        ids.into_iter().next().map(|(_, id)| format!("tombstone:{id}"))
    };
    Some(match h {
        HiddenBy::AuthorSuperseded => successor().unwrap_or_else(|| "author-superseded".into()),
        HiddenBy::AuthorWithdrawn => successor().unwrap_or_else(|| "author-withdrawn".into()),
        HiddenBy::AuthorTombstone | HiddenBy::StewardTombstone => tombstone(false).unwrap_or_else(|| "tombstone:unknown".into()),
        HiddenBy::SensitiveHold => tombstone(true).unwrap_or_else(|| "tombstone:unknown".into()),
        HiddenBy::Contested(_) => "contested".into(),
    })
}

/// Unix seconds of an RFC 3339 UTC time (`2026-10-26T12:00:00Z`), for callers that hold a string clock.
pub fn unix_of(rfc3339: &str) -> Option<i64> {
    parse_utc(rfc3339.split('.').next().map(|s| if s.ends_with('Z') { s.to_string() } else { format!("{s}Z") }).as_deref()?)
}

#[cfg(test)]
#[path = "xenia_index_tests.rs"]
mod tests;
