//! The publish gate (spec 011 §7.1, §8.1; spec 012 §3, §6; plan milestone X7): the one way a record leaves this
//! install for a Xenia store.
//!
//! ```text
//! prepare  : verified sync of the store, build + seal + serialize each record, scan, similar-records list
//! preview  : the exact bytes that will be signed, the flags, the digest of all of it
//! decide   : ONE call to the operator's Approver with a shared reference to the preview. Nothing else runs.
//! sign     : approval digest == digest of the held bytes, flags acknowledged, sources unchanged, then sign
//! deliver  : commit on the verified head, re-verify the candidate head, push (never force), on a rejected push
//!            fetch + verify + rebuild on the new head, re-verify, push again
//! ```
//!
//! # What is guaranteed, and by what
//!
//! * **Promoted scope only.** A `memory` is built only from an [`Origin::Content`] whose scope is workspace or team
//!   *and* that the operator promoted; operator scope and unpromoted content are refused before anything is
//!   built ([`PublishError::NotPromoted`]).
//! * **Scan before bytes are shown.** A [`xenia_scan`] block (a secret, token, key block, registered credential,
//!   local path, session trace) refuses the publish; nothing is previewed, signed or pushed. Flags are shown and the
//!   approval must acknowledge them.
//! * **The bytes shown are the bytes signed, and no model runs between.** [`Prepared`] holds the serialized
//!   bytes; [`Preview::digest`] covers every byte of every file and the sidecar fields that go into the signature.
//!   [`Prepared::sign`] recomputes the digest from the bytes it holds and refuses an [`Approval`] for any other. The
//!   only code that runs between preview and sign is [`Approver::decide`], which is given `&Preview` (immutable data:
//!   no key, no store, no `Prepared`, no tool or model handle) and returns a [`Decision`]; this module imports
//!   nothing that can start a model turn (a test reads this file's own source to prove it). [`Prepared`] is neither
//!   `Clone` nor `Copy` and `sign` consumes it, so a prepared batch is signed once, after exactly one decision.
//! * **A change to the source file after the preview aborts.** A promoted file the content came from is hashed when
//!   read ([`WatchedFile`]) and re-read after the approval and before the signature; any difference is
//!   [`PublishError::SourceChanged`], even though the bytes in memory are unchanged.
//! * **The key never leaves the caller.** The [`Identity`] is passed to `sign` by reference and is used for exactly
//!   one thing: [`Identity::sign`] over a [`delphi_xenia::sig::Domain`]. It is not stored in any struct here, not
//!   formatted into any error, and `Identity`'s `Debug` prints the DID only.
//! * **Never force, never rewrite.** Delivery creates a new commit whose parent is the verified head and pushes it
//!   with an explicit `<commit>:refs/heads/<branch>` refspec and no `--force`, no `+`, no lease ([`push_command`]
//!   is tested for that). A remote that is not a fast-forward of what this install accepted is refused
//!   ([`PublishError::RemoteRewrote`]), not repaired.
//! * **Re-verify before push.** The candidate head (the verified head plus our commit) is read back from the clone
//!   and checked with the verifier's own code (every new file through `validate_record` with the signer's standing,
//!   then the X4b history walk from the verified head). A candidate that fails is not pushed.
//! * **A failed publish leaves local state untouched.** Delivery adds no ref, no sticky entry and no pin: the
//!   commit is an unreferenced object. (The verified sync that precedes each attempt is an ordinary sync and may
//!   advance the accepted head, exactly as `delphi xenia sync` would; a refused or rejected publish adds nothing
//!   to it.) The next sync accepts our commit like anyone else's, through the same walk.
//!
//! # Not here
//!
//! The approval *surface* (the console card, the Red-tier broker) is the caller's: this module defines what the
//! operator is shown ([`Preview`]) and what comes back ([`Decision`]). CLI wiring is X9.

use crate::xenia_scan::{self, Block, Flag, ScanContext};
use crate::xenia_store::{git_command, run, short, RunFail, Store};
use crate::xenia_sync::{GitSource, VerifiedSync, VerifyConfig};
use delphi_xenia::freshness::FreshnessState;
use delphi_xenia::identity::Identity;
use delphi_xenia::layout::Kind;
use delphi_xenia::pins;
use delphi_xenia::pointer::Pointer;
use delphi_xenia::record::{validate_record, AppliesTo, AuthorStatus, BodyKind, Confidence, KindFields, MemoryFields, MemoryType, Pinned, Record, Redaction, Role, SourceCeiling, Validated, VerifiedBy};
use delphi_xenia::roster::{EpochFile, VerifiedRoster};
use delphi_xenia::serialize::{self, SerializeError};
use delphi_xenia::sidecar::Sidecar;
use delphi_xenia::sig::Domain;
use delphi_xenia::store::{classify_path, PathClass};
use delphi_xenia::tier::{self, Computed, Inputs, VerifiedRecord};
use delphi_xenia::walk::{self, Ctx};
use delphi_xenia::{hash, time, ulid};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Rebuilds after a rejected push before giving up (each is a verified sync, a rebuild, a re-verify and a push).
pub const MAX_REBASES: usize = 3;

// ---------------------------------------------------------------------------------------------------------
// Errors

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    /// The content did not come from promoted team or workspace scope.
    NotPromoted(&'static str),
    /// A memory without content provenance, or a command-made record with some.
    WrongOrigin(&'static str),
    /// The `[REV-SEC-05]` scan found something. Nothing was previewed. The findings never carry the values.
    ScanBlocked(Vec<Block>),
    /// The preview has flags and the approval did not acknowledge them.
    FlagsNotAcknowledged,
    /// The operator said no.
    Denied,
    /// The approval is for other bytes than the ones held.
    ApprovalMismatch,
    /// The file the content came from changed after the preview.
    SourceChanged(String),
    /// The key given is not the one the batch was prepared for.
    WrongKey,
    SignerNotActive,
    /// A new revision must be higher than the one in the store.
    RevRegression { id: String, have: u64, want: u64 },
    /// An id that exists is not ours (or is of another kind).
    IdTaken(String),
    NotOwner(String),
    /// An author-hidden record cannot come back, and a withdrawn one cannot change.
    StateRegression(String),
    /// Someone else published the same id between the verified head and our push.
    Raced(String),
    Serialize(SerializeError),
    /// The store could not be used: tampering, a refused fetch, an unjoined store, no trust base.
    StoreNotUsable(String),
    /// The remote's history is not a fast-forward of what this install accepted (rewritten or older).
    RemoteRewrote(&'static str),
    /// The candidate head failed the verifier; nothing was pushed.
    VerifyFailed(Vec<String>),
    /// The remote refused the push (hook, permissions, size) and said so.
    PushRejected(String),
    /// Rejected as non-fast-forward more times than [`MAX_REBASES`].
    TooManyRaces,
    Unavailable(String),
    Io(String),
    // Member commands (xenia_member).
    /// A rule the command checks locally so the operator hears about it before a verifier ignores it.
    Rule(String),
}

impl PublishError {
    /// A stable slug.
    pub fn reason(&self) -> &'static str {
        match self {
            PublishError::NotPromoted(_) => "not-promoted",
            PublishError::WrongOrigin(_) => "wrong-origin",
            PublishError::ScanBlocked(_) => "scan-blocked",
            PublishError::FlagsNotAcknowledged => "flags-not-acknowledged",
            PublishError::Denied => "denied",
            PublishError::ApprovalMismatch => "approval-mismatch",
            PublishError::SourceChanged(_) => "source-changed",
            PublishError::WrongKey => "wrong-key",
            PublishError::SignerNotActive => "signer-not-active",
            PublishError::RevRegression { .. } => "rev-regression",
            PublishError::IdTaken(_) => "id-taken",
            PublishError::NotOwner(_) => "not-owner",
            PublishError::StateRegression(_) => "state-regression",
            PublishError::Raced(_) => "raced",
            PublishError::Serialize(e) => e.reason(),
            PublishError::StoreNotUsable(_) => "store-not-usable",
            PublishError::RemoteRewrote(_) => "remote-rewrote-history",
            PublishError::VerifyFailed(_) => "verify-failed",
            PublishError::PushRejected(_) => "push-rejected",
            PublishError::TooManyRaces => "too-many-races",
            PublishError::Unavailable(_) => "unavailable",
            PublishError::Io(_) => "io",
            PublishError::Rule(_) => "rule",
        }
    }
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PublishError::NotPromoted(w) => write!(f, "refused: {w}; only content the operator promoted from team or workspace scope can be published"),
            PublishError::WrongOrigin(w) => write!(f, "refused: {w}"),
            PublishError::ScanBlocked(b) => {
                write!(f, "refused: the secret and path scan found {} problem(s); nothing was shown or signed:", b.len())?;
                for x in b.iter().take(12) {
                    write!(f, " [{} in {}, line {}]", x.class, x.field, x.line)?;
                }
                Ok(())
            }
            PublishError::FlagsNotAcknowledged => write!(f, "refused: the preview has flags (links, long random strings, identifiers) and the approval did not acknowledge them"),
            PublishError::Denied => write!(f, "not published: the operator declined"),
            PublishError::ApprovalMismatch => write!(f, "refused: the approval is not for the bytes held; nothing was signed"),
            PublishError::SourceChanged(p) => write!(f, "refused: {p} changed after the preview; nothing was signed (preview again)"),
            PublishError::WrongKey => write!(f, "refused: this is not the key the batch was prepared for"),
            PublishError::SignerNotActive => write!(f, "refused: this install's key is not an active member of the roster, so nothing it signs would count"),
            PublishError::RevRegression { id, have, want } => write!(f, "refused: {id} is at revision {have} in the store; revision {want} would not advance it"),
            PublishError::IdTaken(id) => write!(f, "refused: {id} already exists in the store as something else"),
            PublishError::NotOwner(id) => write!(f, "refused: {id} belongs to another member; only its author (or their successor) can publish a new revision"),
            PublishError::StateRegression(w) => write!(f, "refused: {w}"),
            PublishError::Raced(p) => write!(f, "stopped: someone else published {p} while this was being sent; look at the store and decide"),
            PublishError::Serialize(e) => write!(f, "refused: the record cannot be written ({e})"),
            PublishError::StoreNotUsable(w) => write!(f, "refused: the store cannot be trusted right now ({w}); nothing was sent"),
            PublishError::RemoteRewrote(w) => write!(f, "refused: the store's history is not a fast-forward of what this install accepted ({w}); nothing was sent and nothing was changed"),
            PublishError::VerifyFailed(w) => write!(f, "refused: the head to push failed verification ({}); nothing was sent", w.join(", ")),
            PublishError::PushRejected(w) => write!(f, "the store refused the push: {w}; nothing local was changed"),
            PublishError::TooManyRaces => write!(f, "stopped: the store kept moving; try again"),
            PublishError::Unavailable(w) => write!(f, "the store could not be reached: {w}"),
            PublishError::Io(w) => write!(f, "io: {w}"),
            PublishError::Rule(w) => write!(f, "refused: {w}"),
        }
    }
}

impl std::error::Error for PublishError {}

impl From<SerializeError> for PublishError {
    fn from(e: SerializeError) -> PublishError {
        PublishError::Serialize(e)
    }
}

fn io_err(e: impl std::fmt::Display) -> PublishError {
    PublishError::Io(e.to_string())
}

fn xe(e: delphi_xenia::Error) -> PublishError {
    PublishError::Io(e.to_string())
}

// ---------------------------------------------------------------------------------------------------------
// Where content came from

/// The operator's zones as far as the gate cares (spec 004 §5.7: operator scope never leaves the machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Operator,
    Workspace,
    Team,
}

/// A file the content was read from, with its hash when it was read. The gate re-reads it after the approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedFile {
    path: PathBuf,
    hash: String,
}

impl WatchedFile {
    /// Read `path` and remember what it hashed to. The text is returned for the caller to build a draft from.
    pub fn read(path: &Path) -> Result<(WatchedFile, String), PublishError> {
        let bytes = std::fs::read(path).map_err(io_err)?;
        let text = String::from_utf8(bytes.clone()).map_err(|_| PublishError::Io("the source file is not UTF-8".into()))?;
        Ok((WatchedFile { path: path.to_path_buf(), hash: hash::of(&bytes) }, text))
    }

    fn unchanged(&self) -> bool {
        std::fs::read(&self.path).map(|b| hash::of(&b) == self.hash).unwrap_or(false)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentOrigin {
    pub scope: Scope,
    /// The operator promoted this content out of the working area (spec 004 §5.5).
    pub promoted: bool,
    pub file: Option<WatchedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A memory: content from a file or a span, with the scope it lives in.
    Content(ContentOrigin),
    /// An endorsement, dispute, withdrawal or tombstone: typed by the operator at the command, no content source.
    Command,
    /// A record the store already holds, signed again by the successor key (`republish --resign`): the content was
    /// promoted and published once; the gate still scans and previews it.
    Resign,
}

/// One unit of work. `record.id` empty means "a new id"; `record.rev` 0 means "the next revision of that id". The
/// gate sets `org`, `store`, `author`, `author_handle`, `created`, `updated`, the redaction block and the id/rev;
/// nothing the caller put in those fields is believed.
#[derive(Debug, Clone)]
pub struct Item {
    pub origin: Origin,
    pub record: Record,
}

/// What a person writes to publish a `memory` (the rest is the gate's).
#[derive(Debug, Clone)]
pub struct MemoryDraft {
    pub origin: ContentOrigin,
    /// Set to publish a new revision of an existing record.
    pub id: Option<String>,
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub memory_type: MemoryType,
    pub applies_to: AppliesTo,
    pub verified_by: VerifiedBy,
    pub confidence: Confidence,
    pub body: String,
    pub pointers: Vec<Pointer>,
    pub evidence: Vec<Pointer>,
    pub related: Vec<String>,
    pub review_by: String,
    pub expires: Option<String>,
    pub supersedes: Option<(String, u64)>,
    /// Stamped by the publishing daemon from the span ceilings, never chosen by an agent (spec 012 §2).
    pub source_ceiling: SourceCeiling,
    pub author_role: Role,
    pub status: AuthorStatus,
}

impl MemoryDraft {
    pub fn into_item(self) -> Item {
        // Line endings are LF, normalized by the publisher before hashing (spec 012 §3).
        let body = self.body.replace("\r\n", "\n");
        let record = Record {
            id: self.id.unwrap_or_default(),
            rev: 0,
            org: String::new(),
            store: String::new(),
            author: String::new(),
            author_handle: String::new(),
            author_role: self.author_role,
            title: self.title,
            summary: self.summary,
            tags: self.tags,
            created: String::new(),
            updated: String::new(),
            source_ceiling: self.source_ceiling,
            confidence: self.confidence,
            status: self.status,
            supersedes: self.supersedes,
            review_by: Some(self.review_by),
            expires: self.expires,
            pointers: self.pointers,
            evidence: self.evidence,
            related: self.related,
            redaction: Redaction { scanner: String::new(), scanner_version: String::new(), content_hash: hash::of(b"unsealed") },
            body_kind: if body.is_empty() { BodyKind::None } else { BodyKind::Markdown },
            kind_fields: KindFields::Memory(MemoryFields { memory_type: self.memory_type, applies_to: self.applies_to, verified_by: self.verified_by }),
            body,
        };
        Item { origin: Origin::Content(self.origin), record }
    }
}

// ---------------------------------------------------------------------------------------------------------
// The verified view of a store

/// The store at one verified head, as the gate reads it: the roster, every record that passes signature and
/// structure, and the tiers computed from them. Built from the clone only (never from the network).
pub struct StoreView {
    pub head: String,
    pub roster: VerifiedRoster,
    pub files: BTreeMap<String, Vec<u8>>,
    pub records: Vec<VerifiedRecord>,
    pub quarantined: Vec<(String, &'static str)>,
    pub freshness: FreshnessState,
    pub computed: Computed,
}

pub(crate) fn epoch_files(files: &BTreeMap<String, Vec<u8>>) -> Vec<EpochFile> {
    let mut v = Vec::new();
    for n in 1..=delphi_xenia::roster::MAX_EPOCHS as u64 {
        let (Some(j), Some(s)) = (files.get(&delphi_xenia::layout::roster_epoch(n)), files.get(&delphi_xenia::layout::roster_epoch_sigs(n))) else { break };
        v.push(EpochFile { epoch_json: j.clone(), sigs_json: s.clone() });
    }
    v
}

impl StoreView {
    /// Read `head` (a commit already in the clone) and verify it against the install's pins. `prior_tags` is the
    /// per-record tag history the caller keeps (see [`delphi_xenia::tier::Inputs`]); empty is safe but weaker.
    pub fn load(store: &Store, cfg: &VerifyConfig, head: &str, prior_tags: &BTreeMap<String, BTreeSet<String>>) -> Result<StoreView, PublishError> {
        let snap = store.snapshot_of(head).map_err(xe)?;
        let pins = pins::load(&store.pin_storage()).map_err(|e| PublishError::StoreNotUsable(e.reason().into()))?.ok_or_else(|| PublishError::StoreNotUsable("this store has not been joined".into()))?;
        let roster = pins.verify(&epoch_files(&snap.files)).map_err(|e| PublishError::StoreNotUsable(e.reason().into()))?;
        let pinned = Pinned { org: &store.org, store: &store.store };
        let (mut records, mut quarantined) = (Vec::new(), Vec::new());
        for (path, md) in &snap.files {
            let Some(PathClass::Record(rp)) = classify_path(path) else { continue };
            let Some(car) = snap.files.get(&rp.sidecar()) else {
                quarantined.push((path.clone(), "no-sidecar"));
                continue;
            };
            match validate_record(pinned, path, md, car, &|_| Ok(())) {
                Ok(v) => records.push(VerifiedRecord::from_validated(&v)),
                Err(r) => quarantined.push((path.clone(), r.reason())),
            }
        }
        let freshness = store.freshness_status(cfg).map_err(xe)?;
        let computed = tier::compute(&Inputs { roster: &roster, records: &records, freshness: &freshness, prior_tags });
        Ok(StoreView { head: head.to_string(), roster, files: snap.files, records, quarantined, freshness, computed })
    }

    /// The verified record with this id, whatever its kind.
    pub fn record(&self, id: &str) -> Option<&VerifiedRecord> {
        self.records.iter().find(|r| r.record().id == id)
    }

    pub fn bytes_of(&self, v: &VerifiedRecord) -> Option<&Vec<u8>> {
        self.files.get(&v.path())
    }
}

// ---------------------------------------------------------------------------------------------------------
// Similar records (spec 011 §5.1, plan §5.1 item 6)

#[derive(Debug, Clone, PartialEq)]
pub struct SimilarHit {
    pub id: String,
    pub title: String,
    pub tier: String,
    pub score: f32,
}

pub struct SimilarQuery<'a> {
    pub title: &'a str,
    pub summary: &'a str,
    pub tags: &'a [String],
}

/// "Similar records already in the store": implemented by the hybrid search over the store's index (X6). The gate
/// asks and shows; it never builds an index. Hits are record titles written by other members, so they are bounded
/// and stripped of control characters before they are shown.
pub trait SimilarRecords {
    fn similar(&self, q: &SimilarQuery<'_>, limit: usize) -> Vec<SimilarHit>;
}

/// No index yet: the list is empty.
pub struct NoIndex;

impl SimilarRecords for NoIndex {
    fn similar(&self, _: &SimilarQuery<'_>, _: usize) -> Vec<SimilarHit> {
        Vec::new()
    }
}

const SIMILAR_LIMIT: usize = 5;

fn tame(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

// ---------------------------------------------------------------------------------------------------------
// Preview, decision, approval

/// One file as it will be signed: exactly these bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewItem {
    pub path: String,
    pub kind: Kind,
    pub id: String,
    pub rev: u64,
    /// The exact bytes of the record file. The signature will cover `hash` of these.
    pub bytes: Vec<u8>,
    pub hash: String,
    pub signer: String,
    /// Fixed now so they are part of the digest: the sidecar fields inside the signature.
    pub roster_epoch: u64,
    pub signed_at: String,
}

/// Everything the operator sees before saying yes. Plain data; changing a copy of it changes nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub items: Vec<PreviewItem>,
    pub flags: Vec<Flag>,
    pub similar: Vec<SimilarHit>,
    /// Reminders the spec attaches to the action, as slugs (for example `rotate-secrets` after a `sensitive` withdraw).
    pub reminders: Vec<&'static str>,
    /// The head the batch was built on.
    pub head: String,
    /// BLAKE3 over every byte above that is signed ([`preview_digest`]).
    pub digest: String,
}

impl Preview {
    /// An approval of exactly this preview. The only constructor of [`Approval`] outside this module.
    pub fn approve(&self, flags_acknowledged: bool) -> Approval {
        Approval { digest: self.digest.clone(), flags_acknowledged }
    }
}

/// "Yes, to exactly this."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    digest: String,
    flags_acknowledged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Approve(Approval),
    Deny,
}

/// The operator's side: shows the preview and answers. This is the **only** call that happens between the
/// preview and the signature, and its argument is immutable data.
pub trait Approver {
    fn decide(&mut self, preview: &Preview) -> Decision;
}

/// The domain-separated digest of everything that will be signed: each file's path and exact bytes and the sidecar
/// fields that are inside the signature, in order.
pub fn preview_digest(items: &[PreviewItem]) -> String {
    let mut buf = b"xenia-publish-preview/1\n".to_vec();
    for i in items {
        for part in [i.path.as_bytes(), i.signer.as_bytes(), i.signed_at.as_bytes(), i.roster_epoch.to_string().as_bytes(), &i.bytes] {
            buf.extend_from_slice(part.len().to_string().as_bytes());
            buf.push(b'\n');
            buf.extend_from_slice(part);
            buf.push(b'\n');
        }
    }
    hash::of(&buf)
}

// ---------------------------------------------------------------------------------------------------------
// Prepared, Signed

struct PreparedFile {
    record: Record,
    item: PreviewItem,
    watch: Option<WatchedFile>,
}

/// A batch that passed the gate's checks and is waiting for one decision. Not `Clone`; [`Prepared::sign`] consumes it.
pub struct Prepared {
    files: Vec<PreparedFile>,
    preview: Preview,
    signer_did: String,
}

impl Prepared {
    pub fn preview(&self) -> &Preview {
        &self.preview
    }

    fn digest(&self) -> String {
        let items: Vec<PreviewItem> = self.files.iter().map(|f| f.item.clone()).collect();
        preview_digest(&items)
    }

    /// Sign every file in the batch, if and only if the approval is for exactly these bytes. Checks, in order: the
    /// key is the one the batch is for; the approval digest equals the digest of the bytes held (not of the
    /// preview struct, which a caller could have edited); flags are acknowledged; every source file is unchanged.
    /// Then it signs. Nothing runs between those checks and the signatures.
    pub fn sign(self, approval: &Approval, key: &Identity) -> Result<Signed, PublishError> {
        if key.did() != self.signer_did {
            return Err(PublishError::WrongKey);
        }
        if approval.digest != self.digest() {
            return Err(PublishError::ApprovalMismatch);
        }
        if !self.preview.flags.is_empty() && !approval.flags_acknowledged {
            return Err(PublishError::FlagsNotAcknowledged);
        }
        for f in &self.files {
            if let Some(w) = &f.watch {
                if !w.unchanged() {
                    return Err(PublishError::SourceChanged(w.path.display().to_string()));
                }
            }
        }
        let org_store: Vec<(String, String)> = self.files.iter().map(|f| (f.record.org.clone(), f.record.store.clone())).collect();
        let mut out = Vec::new();
        for (f, (org, store)) in self.files.iter().zip(org_store) {
            let d = Domain::record(&org, &store, f.item.kind, &f.item.id, &f.item.signer, &f.item.hash, f.item.roster_epoch, &f.item.signed_at).map_err(xe)?;
            let car = Sidecar { kid: f.item.signer.clone(), signed_at: f.item.signed_at.clone(), roster_epoch: f.item.roster_epoch, hash: f.item.hash.clone(), sig: key.sign(&d) };
            out.push(SignedFile { record: f.record.clone(), path: f.item.path.clone(), md: f.item.bytes.clone(), sidecar_path: sidecar_path(&f.item.path), sidecar: car.to_json().into_bytes() });
        }
        Ok(Signed { files: out, head: self.preview.head.clone() })
    }
}

fn sidecar_path(md: &str) -> String {
    format!("{}.sig.json", md.trim_end_matches(".md"))
}

#[derive(Debug, Clone)]
pub struct SignedFile {
    pub record: Record,
    pub path: String,
    pub md: Vec<u8>,
    pub sidecar_path: String,
    pub sidecar: Vec<u8>,
}

/// Signed bytes, ready to deliver. Constructible only by [`Prepared::sign`].
#[derive(Debug)]
pub struct Signed {
    files: Vec<SignedFile>,
    head: String,
}

impl Signed {
    pub fn files(&self) -> &[SignedFile] {
        &self.files
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    /// The commit now at the store's branch (ours, or the head that already held our bytes).
    pub commit: String,
    /// The verified head it was built on.
    pub parent: String,
    pub paths: Vec<String>,
    /// 1 when the first push went through; more when the store moved and the batch was rebuilt.
    pub attempts: usize,
    /// Nothing was sent: the store already held exactly these bytes.
    pub already_there: bool,
}

// ---------------------------------------------------------------------------------------------------------
// The publisher

pub struct Publisher<'a> {
    pub store: &'a Store,
    pub cfg: VerifyConfig,
    pub scan: ScanContext,
    pub similar: &'a dyn SimilarRecords,
    /// Tag history for the tombstone area check; see [`StoreView::load`].
    pub prior_tags: BTreeMap<String, BTreeSet<String>>,
    /// A source of new ids (milliseconds in, ULID out); `None` uses the system random source.
    pub ids: Option<&'a dyn Fn(u64) -> String>,
}

impl<'a> Publisher<'a> {
    pub fn new(store: &'a Store, cfg: VerifyConfig, similar: &'a dyn SimilarRecords) -> Publisher<'a> {
        Publisher { store, cfg, scan: ScanContext::default(), similar, prior_tags: BTreeMap::new(), ids: None }
    }

    /// A verified sync, then the head it accepted. Anything but an accepted or unchanged head is a refusal.
    pub fn verified_head(&self) -> Result<String, PublishError> {
        if pins::load(&self.store.pin_storage()).map_err(|e| PublishError::StoreNotUsable(e.reason().into()))?.is_none() {
            return Err(PublishError::StoreNotUsable("this store has not been joined".into()));
        }
        match self.store.sync_verified(&self.cfg).map_err(xe)? {
            VerifiedSync::Accepted { head, .. } | VerifiedSync::UpToDate { head, .. } => Ok(head),
            VerifiedSync::Tampering { findings, .. } => Err(PublishError::StoreNotUsable(format!("tampering found in {} path(s)", findings.len()))),
            VerifiedSync::Rejected { reason, .. } => Err(PublishError::StoreNotUsable(reason.into())),
            VerifiedSync::NonFastForward(n) => Err(PublishError::RemoteRewrote(match n.kind {
                crate::xenia_store::Divergence::Diverged => "the history diverged",
                crate::xenia_store::Divergence::Behind => "the store is older than what was accepted",
            })),
            VerifiedSync::Refused(r) => Err(PublishError::StoreNotUsable(r.reason().into())),
            VerifiedSync::Unavailable(m) => Err(PublishError::Unavailable(m)),
        }
    }

    /// Sync, check and build `items` against the verified head, and produce the preview.
    pub fn prepare(&self, signer: &Identity, items: Vec<Item>) -> Result<Prepared, PublishError> {
        let head = self.verified_head()?;
        let view = StoreView::load(self.store, &self.cfg, &head, &self.prior_tags)?;
        self.prepare_on(&view, signer, items)
    }

    /// [`Publisher::prepare`] against a view already loaded (the member commands read the view first).
    pub fn prepare_on(&self, view: &StoreView, signer: &Identity, items: Vec<Item>) -> Result<Prepared, PublishError> {
        if items.is_empty() {
            return Err(PublishError::Rule("nothing to publish".into()));
        }
        let me = signer.did();
        let member = view.roster.member(&me).filter(|m| m.is_active()).ok_or(PublishError::SignerNotActive)?;
        let handle = member.handle.clone();
        let now = time::format_utc(self.cfg.now);
        let today = &now[..10];
        let mut files = Vec::new();
        let mut blocks = Vec::new();
        let mut flags = Vec::new();
        let mut similar = Vec::new();
        let mut reminders: Vec<&'static str> = Vec::new();
        let mut seen_ids = BTreeSet::new();
        for item in items {
            check_origin(&item)?;
            let watch = match &item.origin {
                Origin::Content(c) => c.file.clone(),
                Origin::Command | Origin::Resign => None,
            };
            let mut r = item.record;
            if r.id.is_empty() {
                let ms = u64::try_from(self.cfg.now).unwrap_or(0).saturating_mul(1000);
                r.id = match self.ids {
                    Some(f) => f(ms),
                    None => ulid::generate(ms).map_err(xe)?,
                };
            }
            if !ulid::is_valid(&r.id) {
                return Err(PublishError::Serialize(SerializeError::Unrepresentable("the id is not a ULID")));
            }
            if !seen_ids.insert(r.id.clone()) {
                return Err(PublishError::Rule("one id twice in one batch".into()));
            }
            let existing = view.record(&r.id);
            r.org = self.store.org.clone();
            r.store = self.store.store.clone();
            r.author = me.clone();
            r.author_handle = handle.clone();
            r.updated = now.clone();
            match existing {
                None => {
                    r.created = now.clone();
                    r.rev = 1;
                }
                Some(e) => {
                    r.created = e.record().created.clone();
                    if r.rev == 0 {
                        r.rev = e.record().rev + 1;
                    }
                }
            }
            check_against(view, &r, &me)?;
            r.check_future_dates(today).map_err(|e| PublishError::Serialize(SerializeError::Invalid(e)))?;
            let (sealed, bytes) = serialize::seal(&r)?;
            let report = xenia_scan::scan_record(&sealed, std::str::from_utf8(&bytes).ok(), &self.scan);
            blocks.extend(report.blocks);
            flags.extend(report.flags);
            if let KindFields::Tombstone(t) = &sealed.kind_fields {
                if t.reason == delphi_xenia::record::TombstoneReason::Sensitive && t.action == delphi_xenia::record::TombstoneAction::Withdraw && !reminders.contains(&"rotate-secrets") {
                    // Deletion cannot un-leak: a published secret must be rotated, not just hidden (spec 011 §7.1 item 5).
                    reminders.push("rotate-secrets");
                }
            }
            if sealed.kind() == Kind::Memory {
                for h in self.similar.similar(&SimilarQuery { title: &sealed.title, summary: &sealed.summary, tags: &sealed.tags }, SIMILAR_LIMIT + 1) {
                    if h.id != sealed.id && similar.len() < SIMILAR_LIMIT && !similar.iter().any(|x: &SimilarHit| x.id == h.id) {
                        similar.push(SimilarHit { id: h.id, title: tame(&h.title, 120), tier: tame(&h.tier, 8), score: h.score });
                    }
                }
            }
            let kind = sealed.kind();
            let path = serialize::path_of(&sealed);
            let item = PreviewItem {
                path,
                kind,
                id: sealed.id.clone(),
                rev: sealed.rev,
                hash: hash::of(&bytes),
                bytes,
                signer: me.clone(),
                roster_epoch: view.roster.head_epoch(),
                signed_at: now.clone(),
            };
            files.push(PreparedFile { record: sealed, item, watch });
        }
        if !blocks.is_empty() {
            // Nothing is previewed, signed or pushed, and the findings carry no values.
            return Err(PublishError::ScanBlocked(blocks));
        }
        let items: Vec<PreviewItem> = files.iter().map(|f| f.item.clone()).collect();
        let preview = Preview { digest: preview_digest(&items), items, flags, similar, reminders, head: view.head.clone() };
        Ok(Prepared { files, preview, signer_did: me })
    }

    /// The whole gate: prepare, ask the operator once, sign, deliver.
    pub fn publish(&self, signer: &Identity, items: Vec<Item>, approver: &mut dyn Approver) -> Result<Published, PublishError> {
        self.publish_with(signer, approver, |_| Ok(items))
    }

    /// [`Publisher::publish`] where the items are built from the verified view (the member commands read the store
    /// before they know what to write).
    pub fn publish_with(&self, signer: &Identity, approver: &mut dyn Approver, build: impl FnOnce(&StoreView) -> Result<Vec<Item>, PublishError>) -> Result<Published, PublishError> {
        let head = self.verified_head()?;
        let view = StoreView::load(self.store, &self.cfg, &head, &self.prior_tags)?;
        let items = build(&view)?;
        let prepared = self.prepare_on(&view, signer, items)?;
        // The one call between the preview and the signature.
        let decision = approver.decide(prepared.preview());
        let signed = match decision {
            Decision::Approve(a) => prepared.sign(&a, signer)?,
            Decision::Deny => return Err(PublishError::Denied),
        };
        self.deliver(&signed)
    }

    /// Commit, re-verify and push `signed`. See the module docs.
    pub fn deliver(&self, signed: &Signed) -> Result<Published, PublishError> {
        let mut head = signed.head.clone();
        for attempt in 1..=MAX_REBASES + 1 {
            if attempt > 1 {
                head = self.verified_head()?;
            }
            let view = StoreView::load(self.store, &self.cfg, &head, &self.prior_tags)?;
            // The store may have moved since the bytes were signed: the same rules, against the new head.
            if signed.files.iter().all(|f| view.files.get(&f.path) == Some(&f.md) && view.files.get(&f.sidecar_path) == Some(&f.sidecar)) {
                return Ok(Published { commit: head.clone(), parent: head, paths: signed.files.iter().map(|f| f.path.clone()).collect(), attempts: attempt, already_there: true });
            }
            for f in &signed.files {
                let signer = f.record.author.as_str();
                if let Err(e) = check_against(&view, &f.record, signer) {
                    return Err(match e {
                        PublishError::RevRegression { .. } | PublishError::IdTaken(_) if attempt > 1 => PublishError::Raced(f.path.clone()),
                        other => other,
                    });
                }
            }
            let candidate = self.commit_on(&head, signed)?;
            self.verify_candidate(&view, &head, &candidate, signed)?;
            match self.push(&candidate)? {
                PushOutcome::Pushed => {
                    return Ok(Published { commit: candidate, parent: head, paths: signed.files.iter().map(|f| f.path.clone()).collect(), attempts: attempt, already_there: false });
                }
                PushOutcome::NotFastForward => continue,
            }
        }
        Err(PublishError::TooManyRaces)
    }

    fn git(&self, args: &[&str], env: &[(&str, &str)], stdin: Option<Vec<u8>>) -> Result<Vec<u8>, PublishError> {
        run_git(self.store, args, env, stdin)
    }

    /// A new commit on `head` that adds the signed files, built with git plumbing in the bare clone through a
    /// private temporary index. No ref is touched; the commit is an unreferenced object until (and unless) it is
    /// pushed. A record and its sidecar go in the same commit, because the history walk requires every commit to be
    /// valid on its own.
    fn commit_on(&self, head: &str, signed: &Signed) -> Result<String, PublishError> {
        let index = self.store.paths.state.join(format!(".publish-{}.index", std::process::id()));
        let _ = std::fs::remove_file(&index);
        let idx = index.to_string_lossy().into_owned();
        let env = [("GIT_INDEX_FILE", idx.as_str())];
        let result = (|| {
            self.git(&["read-tree", head], &env, None)?;
            for f in &signed.files {
                for (path, bytes) in [(&f.path, &f.md), (&f.sidecar_path, &f.sidecar)] {
                    let blob = String::from_utf8_lossy(&self.git(&["hash-object", "-w", "--no-filters", "--stdin"], &[], Some(bytes.clone()))?).trim().to_string();
                    self.git(&["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{path}")], &env, None)?;
                }
            }
            let tree = String::from_utf8_lossy(&self.git(&["write-tree"], &env, None)?).trim().to_string();
            let date = format!("{} +0000", self.cfg.now.max(0));
            let what = signed.files.iter().map(|f| format!("{} {} rev {}", f.record.kind().as_str(), f.record.id, f.record.rev)).collect::<Vec<_>>().join("; ");
            let msg = format!("xenia: publish {what}");
            let env2 = [
                ("GIT_AUTHOR_NAME", "delphi-xenia"),
                ("GIT_AUTHOR_EMAIL", "xenia@delphi.invalid"),
                ("GIT_COMMITTER_NAME", "delphi-xenia"),
                ("GIT_COMMITTER_EMAIL", "xenia@delphi.invalid"),
                ("GIT_AUTHOR_DATE", date.as_str()),
                ("GIT_COMMITTER_DATE", date.as_str()),
            ];
            let commit = String::from_utf8_lossy(&self.git(&["commit-tree", &tree, "-p", head, "-m", &msg], &env2, None)?).trim().to_string();
            if !delphi_xenia::sticky::is_commit_id(&commit) {
                return Err(PublishError::Io("git commit-tree printed something unexpected".into()));
            }
            Ok(commit)
        })();
        let _ = std::fs::remove_file(&index);
        result
    }

    /// Re-verify the head we are about to push: the new commit changes exactly our files, each validates with the
    /// signer's standing, and the X4b history walk from the verified head finds nothing.
    fn verify_candidate(&self, view: &StoreView, base: &str, candidate: &str, signed: &Signed) -> Result<(), PublishError> {
        let snap = self.store.snapshot_of(candidate).map_err(xe)?;
        let mut problems: Vec<String> = Vec::new();
        let ours: BTreeSet<&str> = signed.files.iter().flat_map(|f| [f.path.as_str(), f.sidecar_path.as_str()]).collect();
        for (path, bytes) in &snap.files {
            if !ours.contains(path.as_str()) && view.files.get(path) != Some(bytes) {
                problems.push("the commit changed a file that is not ours".into());
            }
        }
        for path in view.files.keys() {
            if !snap.files.contains_key(path) {
                problems.push("the commit deleted a file".into());
            }
        }
        let pinned = Pinned { org: &self.store.org, store: &self.store.store };
        for f in &signed.files {
            let (Some(md), Some(car)) = (snap.files.get(&f.path), snap.files.get(&f.sidecar_path)) else {
                problems.push("a signed file is missing from the commit".into());
                continue;
            };
            if md != &f.md || car != &f.sidecar {
                problems.push("a file in the commit is not the signed bytes".into());
                continue;
            }
            let active = |c: &Sidecar| if view.roster.is_active(&c.kid) { Ok(()) } else { Err("signer-not-active") };
            let v: Result<Validated, _> = validate_record(pinned, &f.path, md, car, &active);
            if let Err(e) = v {
                problems.push(format!("{}: {}", f.path, e.reason()));
            }
        }
        if problems.is_empty() {
            let ctx = Ctx { pinned, roster: &view.roster };
            let findings = walk::walk(&GitSource::new(self.store), &ctx, base, candidate).map_err(xe)?;
            for f in findings.iter().take(8) {
                problems.push(format!("{}: {}", f.path, f.reason));
            }
        }
        if problems.is_empty() { Ok(()) } else { Err(PublishError::VerifyFailed(problems)) }
    }

    fn push(&self, commit: &str) -> Result<PushOutcome, PublishError> {
        push_to(self.store, commit, &self.store.branch)
    }
}

/// One git command in the store's bare clone through the hardened builder; `env` adds to the cleared environment.
pub(crate) fn run_git(store: &Store, args: &[&str], env: &[(&str, &str)], stdin: Option<Vec<u8>>) -> Result<Vec<u8>, PublishError> {
    let mut c: Command = git_command(&store.paths, None, Some(&store.paths.clone));
    c.args(args);
    for (k, v) in env {
        c.env(k, v);
    }
    let out = run(c, stdin, 1 << 20, Duration::from_secs(60), None).map_err(|f| match f {
        RunFail::Timeout => PublishError::Io(format!("git {} timed out", args.first().unwrap_or(&""))),
        _ => PublishError::Io(format!("git {} failed", args.first().unwrap_or(&""))),
    })?;
    if out.code != 0 {
        return Err(PublishError::Io(format!("git {} failed: {}", args.first().unwrap_or(&""), short(&out.stderr))));
    }
    Ok(out.stdout)
}

/// Push `commit` to `refs/heads/<branch>` of the store's remote, as a fast-forward only.
pub(crate) fn push_to(store: &Store, commit: &str, branch: &str) -> Result<PushOutcome, PublishError> {
    let cmd = push_command_for(store, commit, branch);
    let out = run(cmd, None, 64 * 1024, Duration::from_secs(store.caps.fetch_timeout_secs.max(10)), None).map_err(|f| match f {
        RunFail::Timeout => PublishError::Unavailable("the push timed out".into()),
        _ => PublishError::Io("git push failed to run".into()),
    })?;
    if out.code == 0 {
        return Ok(PushOutcome::Pushed);
    }
    let lower = out.stderr.to_lowercase();
    if lower.contains("non-fast-forward") || lower.contains("fetch first") || lower.contains("[rejected]") && lower.contains("stale") {
        return Ok(PushOutcome::NotFastForward);
    }
    Err(PublishError::PushRejected(short(&out.stderr)))
}

pub(crate) enum PushOutcome {
    Pushed,
    NotFastForward,
}

/// The push command. An explicit `<commit>:refs/heads/<branch>` refspec, no `+`, no `--force`, no lease: the remote
/// accepts it only as a fast-forward. Built in one place so a test can pin that.
pub fn push_command(store: &Store, commit: &str) -> Command {
    push_command_for(store, commit, &store.branch)
}

/// [`push_command`] for another branch (an admin's `roster-proposal/<n>`). The same refspec shape, so the same
/// guarantees.
pub fn push_command_for(store: &Store, commit: &str, branch: &str) -> Command {
    let mut c = git_command(&store.paths, Some(store.url.scheme), Some(&store.paths.clone));
    c.args(["push", "--quiet", "--no-verify", "--no-recurse-submodules"]).arg(&store.url.url).arg(format!("{commit}:refs/heads/{branch}"));
    c
}

/// Spec 011 §7.1 step 1: where the content may come from, per kind.
fn check_origin(item: &Item) -> Result<(), PublishError> {
    match (&item.origin, item.record.kind()) {
        (Origin::Content(c), Kind::Memory) => {
            if c.scope == Scope::Operator {
                return Err(PublishError::NotPromoted("operator scope never leaves the machine"));
            }
            if !c.promoted {
                return Err(PublishError::NotPromoted("the content has not been promoted"));
            }
            Ok(())
        }
        (Origin::Command, Kind::Memory) => Err(PublishError::WrongOrigin("a memory is published from promoted content, not typed into a command")),
        (Origin::Content(_), _) => Err(PublishError::WrongOrigin("an endorsement or tombstone has no content source")),
        (Origin::Command, _) | (Origin::Resign, _) => Ok(()),
    }
}

/// The rules a new file must satisfy against what the store already holds at this head: the signer is active, a
/// new id starts at revision 1, an existing id is ours (or our predecessor's, through the `successor` chain) and
/// of the same kind, the revision goes up, and the author's own state only moves forward.
pub fn check_against(view: &StoreView, r: &Record, me: &str) -> Result<(), PublishError> {
    if !view.roster.is_active(me) {
        return Err(PublishError::SignerNotActive);
    }
    match view.record(&r.id) {
        None => {
            if view.quarantined.iter().any(|(p, _)| p.contains(&r.id)) {
                return Err(PublishError::IdTaken(r.id.clone()));
            }
            if r.rev != 1 {
                return Err(PublishError::RevRegression { id: r.id.clone(), have: 0, want: r.rev });
            }
            Ok(())
        }
        Some(e) => {
            let old = e.record();
            if old.kind() != r.kind() {
                return Err(PublishError::IdTaken(r.id.clone()));
            }
            let owner = e.signer() == me || view.roster.successor_chain(e.signer()).iter().any(|d| d == me);
            if !owner {
                return Err(PublishError::NotOwner(r.id.clone()));
            }
            if r.rev <= old.rev {
                return Err(PublishError::RevRegression { id: r.id.clone(), have: old.rev, want: r.rev });
            }
            if !tier::author_transition_allowed(old.status, r.status) {
                return Err(PublishError::StateRegression(format!("{} was hidden by its author and cannot come back", r.id)));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "xenia_publish_tests.rs"]
pub(crate) mod tests;
