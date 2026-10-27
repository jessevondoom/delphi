//! Admin operations on a Xenia store (spec 011 §3.2, §4.1, §8; plan milestone X8): `add`, `remove`, `rotate`,
//! `propose`, `sign`, `publish` and `refresh`. The pure rules (drafting an epoch, describing a change, counting
//! signatures) are in `delphi_xenia::admin`; this module is git, the verified view and the operator's approval.
//!
//! **The proposal branch is untrusted input.** `roster-proposal/<n>` lives on the same host as everything else, so
//! anyone who can push can write to it. What that means here:
//!
//! * It is fetched like a sync is (hardened git, the same timeout and object caps, nothing checked out) and only two
//!   blobs are read from it, by object id: `roster/epochs/<n>.json` and `<n>.sigs.json`. Nothing else in its tree is
//!   ever read, copied or pushed onward.
//! * The epoch file is checked as a link in the verified chain (right scope, number and `prev_hash`; only the
//!   transitions the roster allows) and then **described change by change** to the signer. Changes none of
//!   add/remove/rotate makes, and anything that grants admin, are warnings the approval must acknowledge.
//! * A signature is made over the exact bytes of the epoch file, and counts only if it verifies over those bytes and
//!   comes from an `active` admin of the previous epoch, once per person. A proposal edited after signatures were
//!   collected keeps none of them.
//! * `sign` writes a **fresh tree**: the verified main head's tree plus the two epoch files. The proposal's own tree
//!   is never the parent of anything on main. (Its commit is the git parent of the signature commit, so the branch
//!   stays a fast-forward for the next signer.)
//! * `publish` does not merge or fast-forward main to the proposal tip. It builds a new commit on the verified main
//!   head that adds the epoch file, the signatures that count, and a freshness statement for the new epoch (spec 011
//!   §4.1: "a roster change never leaves the org stale"), verifies that candidate with the verifier's own code (the
//!   chain at the candidate, the statement, the X4b history walk) and pushes it as a fast-forward with no force.
//!
//! **A statement is never signed blind.** `refresh` runs the whole verified sync first (chain, statement, history
//! walk from the accepted head, record verification, sticky rules) and then, from the previous authentic statement's
//! `store_head`, the walk again; any finding refuses. Time moves forward only: a statement is never issued at or
//! before the previous one.
//!
//! **No model between preview and signature, and no key in a struct.** The only code between the two is
//! [`AdminApprover::decide`], which gets immutable plain data. Approvals are bound to a digest of what is signed, which
//! is recomputed from the bytes held. A test reads this file's source for network, model and async constructs.
//!
//! **Genesis** ([`Admin::sign_genesis`], [`Admin::publish_genesis`]) creates a store: every founder signs the exact
//! epoch-1 bytes, and one founder writes the epoch and a statement for it as the first two commits of `main`
//! (a fresh install with no authentic statement accepts nothing). It refuses a remote that already has a `main`.
//!
//! Known limits: a store lock is not held across fetch, commit and push; the CLI and the console card are X9.

use crate::xenia_publish::{epoch_files, push_to, run_git, PublishError, PushOutcome};
use crate::xenia_store::{AuxFetch, Store};
use crate::xenia_sync::{GitSource, VerifiedSync, VerifyConfig};
use delphi_xenia::admin::{self, AdminError as RuleError, Draft, Op};
use delphi_xenia::freshness::{self, Authentic};
use delphi_xenia::identity::Identity;
use delphi_xenia::layout::{roster_epoch, roster_epoch_sigs, FRESHNESS, FRESHNESS_SIGS};
use delphi_xenia::pins;
use delphi_xenia::record::Pinned;
use delphi_xenia::roster::{VerifiedEpoch, VerifiedRoster, MAX_EPOCH_BYTES, MAX_SIGS_BYTES};
use delphi_xenia::walk::{self, Ctx};
use delphi_xenia::{hash, time};
use std::collections::{BTreeMap, BTreeSet};

/// The root attributes file a store must carry (spec 012 §7): exact bytes, no conversion.
pub const ATTRIBUTES: &str = "* -text\n";

/// Rebuilds after a rejected push before giving up.
pub const MAX_ATTEMPTS: usize = 3;

/// How long a statement written by `publish` or `refresh` lives unless the caller says otherwise. The verifier's own
/// cap (default 14 days) still applies; this is deliberately half of it.
pub const DEFAULT_LIFE_SECS: i64 = 7 * 24 * 3600;

// ---------------------------------------------------------------------------------------------------------
// Errors

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminError {
    /// The store cannot be used: not joined, a refused fetch, no trust base, a failed check that is not tampering.
    StoreNotUsable(String),
    /// The history walk or the sticky rules found tampering. Nothing was signed. `(path, reason)`.
    Tampering(Vec<(String, &'static str)>),
    RemoteRewrote(&'static str),
    Unavailable(String),
    /// The key given is not an `active` admin of the head epoch.
    NotAnAdmin,
    /// The publisher would not be an `active` admin of the new epoch, so it could not sign its statement.
    NotActiveInNewEpoch,
    /// A draft or proposal broke a rule of `delphi_xenia::admin`.
    Rule(RuleError),
    ProposalMissing(u64),
    /// `roster-proposal/<n>` already exists (a proposal for this epoch is open).
    ProposalExists(u64),
    /// The proposal branch changed between the preview and the signature.
    ProposalChanged,
    /// The fetch of the proposal branch was refused (caps).
    ProposalRefused(&'static str),
    /// The chain moved on: the proposal is for an epoch that is no longer the next one.
    StaleProposal { next: u64, proposed: u64 },
    QuorumNotMet { have: usize, need: usize },
    Denied,
    ApprovalMismatch,
    WarningsNotAcknowledged,
    /// A statement must be issued after the previous one (replay protection at every install).
    ClockBehind,
    /// The candidate commit failed the verifier; nothing was pushed.
    VerifyFailed(Vec<String>),
    PushRejected(String),
    TooManyRaces,
    /// The remote already has a main branch: genesis only creates a store, it never adds to one.
    StoreNotEmpty,
    Io(String),
}

impl AdminError {
    pub fn reason(&self) -> &'static str {
        match self {
            AdminError::StoreNotUsable(_) => "store-not-usable",
            AdminError::Tampering(_) => "tampering",
            AdminError::RemoteRewrote(_) => "remote-rewrote-history",
            AdminError::Unavailable(_) => "unavailable",
            AdminError::NotAnAdmin => "not-an-admin",
            AdminError::NotActiveInNewEpoch => "not-active-in-new-epoch",
            AdminError::Rule(r) => r.reason(),
            AdminError::ProposalMissing(_) => "proposal-missing",
            AdminError::ProposalExists(_) => "proposal-exists",
            AdminError::ProposalChanged => "proposal-changed",
            AdminError::ProposalRefused(r) => r,
            AdminError::StaleProposal { .. } => "stale-proposal",
            AdminError::QuorumNotMet { .. } => "quorum-not-met",
            AdminError::Denied => "denied",
            AdminError::ApprovalMismatch => "approval-mismatch",
            AdminError::WarningsNotAcknowledged => "warnings-not-acknowledged",
            AdminError::ClockBehind => "clock-behind",
            AdminError::VerifyFailed(_) => "verify-failed",
            AdminError::PushRejected(_) => "push-rejected",
            AdminError::TooManyRaces => "too-many-races",
            AdminError::StoreNotEmpty => "store-not-empty",
            AdminError::Io(_) => "io",
        }
    }
}

impl std::fmt::Display for AdminError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminError::StoreNotUsable(m) => write!(f, "the store cannot be used: {m}"),
            AdminError::Tampering(p) => {
                write!(f, "refusing to sign: possible tampering in {} path(s)", p.len())?;
                for (path, why) in p.iter().take(5) {
                    write!(f, "\n  {path}: {why}")?;
                }
                Ok(())
            }
            AdminError::RemoteRewrote(m) => write!(f, "the store host rewrote history ({m})"),
            AdminError::Unavailable(m) => write!(f, "the store host is unavailable: {m}"),
            AdminError::NotAnAdmin => f.write_str("this key is not an active admin of the current roster epoch"),
            AdminError::NotActiveInNewEpoch => f.write_str("this key would not be an active admin of the new epoch, so it cannot sign the new epoch's statement; another admin must publish"),
            AdminError::Rule(r) => write!(f, "{r}"),
            AdminError::ProposalMissing(n) => write!(f, "there is no proposal branch roster-proposal/{n}"),
            AdminError::ProposalExists(n) => write!(f, "roster-proposal/{n} already exists; sign or publish that proposal, or ask its author to remove the branch"),
            AdminError::ProposalChanged => f.write_str("the proposal changed while you were deciding; look again"),
            AdminError::ProposalRefused(r) => write!(f, "the proposal branch was refused ({r})"),
            AdminError::StaleProposal { next, proposed } => write!(f, "the proposal is for epoch {proposed} but the next epoch is {next}"),
            AdminError::QuorumNotMet { have, need } => write!(f, "{have} of {need} required admin signatures"),
            AdminError::Denied => f.write_str("not approved"),
            AdminError::ApprovalMismatch => f.write_str("the approval is for other bytes than the ones held"),
            AdminError::WarningsNotAcknowledged => f.write_str("the preview has warnings and the approval did not acknowledge them"),
            AdminError::ClockBehind => f.write_str("the previous statement was issued at or after this clock; refusing to issue a statement that other installs would treat as a replay"),
            AdminError::VerifyFailed(p) => write!(f, "the candidate commit failed verification and was not pushed: {}", p.join("; ")),
            AdminError::PushRejected(m) => write!(f, "the host refused the push: {m}"),
            AdminError::TooManyRaces => f.write_str("the store kept moving; gave up after several attempts"),
            AdminError::StoreNotEmpty => f.write_str("the remote already has a main branch; genesis creates a new store and never adds to an existing one"),
            AdminError::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AdminError {}

impl From<RuleError> for AdminError {
    fn from(e: RuleError) -> AdminError {
        AdminError::Rule(e)
    }
}

impl From<PublishError> for AdminError {
    fn from(e: PublishError) -> AdminError {
        match e {
            PublishError::StoreNotUsable(m) => AdminError::StoreNotUsable(m),
            PublishError::RemoteRewrote(m) => AdminError::RemoteRewrote(m),
            PublishError::Unavailable(m) => AdminError::Unavailable(m),
            PublishError::PushRejected(m) => AdminError::PushRejected(m),
            PublishError::VerifyFailed(p) => AdminError::VerifyFailed(p),
            other => AdminError::Io(other.to_string()),
        }
    }
}

fn io(e: impl std::fmt::Display) -> AdminError {
    AdminError::Io(e.to_string())
}

// ---------------------------------------------------------------------------------------------------------
// The approval surface

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Propose,
    Sign,
    Publish,
    Refresh,
    /// A founder signs the genesis epoch.
    GenesisSign,
    /// Writing the first commits of a new store: the genesis epoch, its signatures and its statement.
    GenesisPublish,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Propose => "propose",
            Action::Sign => "sign",
            Action::Publish => "publish",
            Action::Refresh => "refresh",
            Action::GenesisSign => "genesis-sign",
            Action::GenesisPublish => "genesis-publish",
        }
    }
}

/// Everything the admin is shown before the key signs. Plain data: changing a copy changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminPreview {
    pub action: Action,
    /// The epoch signed (propose, sign), published (publish) or attested (refresh).
    pub epoch: u64,
    pub epoch_hash: String,
    /// Every difference from the current roster, one line each (empty for refresh).
    pub changes: Vec<String>,
    pub warnings: Vec<String>,
    /// Handles of the people whose signatures count so far, and how many the epoch needs.
    pub signed: Vec<String>,
    pub have: usize,
    pub need: usize,
    /// For publish and refresh: the verified head the statement will name, and its dates.
    pub store_head: Option<String>,
    pub statement: Option<(String, String)>,
    /// Anything else worth a line (files that do not verify, the previous statement's age).
    pub notes: Vec<String>,
    /// BLAKE3 over what will be signed.
    pub digest: String,
}

impl AdminPreview {
    /// An approval of exactly this preview. The only constructor of [`AdminApproval`] outside this module.
    pub fn approve(&self, warnings_acknowledged: bool) -> AdminApproval {
        AdminApproval { digest: self.digest.clone(), warnings_acknowledged }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminApproval {
    digest: String,
    warnings_acknowledged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminDecision {
    Approve(AdminApproval),
    Deny,
}

/// The operator's side. The **only** call between the preview and the signature; its argument is immutable data.
pub trait AdminApprover {
    fn decide(&mut self, preview: &AdminPreview) -> AdminDecision;
}

/// The digest of what a signature will cover: the action, the epoch, the hash of the epoch file's exact bytes and,
/// for a statement, when it is issued and for how long.
pub fn digest(action: Action, epoch: u64, epoch_hash: &str, issued: i64, life: i64) -> String {
    hash::of(format!("xenia-admin-preview/1\n{}\n{epoch}\n{epoch_hash}\n{issued}\n{life}\n", action.as_str()).as_bytes())
}

fn check_approval(decision: AdminDecision, want: &str, warnings: bool) -> Result<(), AdminError> {
    match decision {
        AdminDecision::Deny => Err(AdminError::Denied),
        AdminDecision::Approve(a) => {
            if a.digest != want {
                return Err(AdminError::ApprovalMismatch);
            }
            if warnings && !a.warnings_acknowledged {
                return Err(AdminError::WarningsNotAcknowledged);
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Results

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposed {
    pub epoch: u64,
    pub epoch_hash: String,
    pub branch: String,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cosigned {
    pub epoch: u64,
    pub epoch_hash: String,
    pub have: usize,
    pub need: usize,
    /// The commit pushed to the proposal branch; `None` when this key had already signed (nothing to do).
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochPublished {
    pub epoch: u64,
    pub epoch_hash: String,
    pub commit: String,
    pub attempts: usize,
    /// Whether this install's own verified sync then accepted the new head.
    pub adopted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisPublished {
    /// The head pushed: the commit that holds the first statement.
    pub commit: String,
    /// The commit that holds the epoch itself (the statement's `store_head`).
    pub epoch_commit: String,
    pub epoch_hash: String,
    /// Whether this install is now joined to the store (pinned at epoch 1 by the publisher's own fingerprint) and
    /// its first verified sync accepted the head.
    pub adopted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refreshed {
    pub commit: String,
    pub store_head: String,
    pub expires_at: String,
    pub adopted: bool,
}

// ---------------------------------------------------------------------------------------------------------
// The operations

pub struct Admin<'a> {
    pub store: &'a Store,
    pub cfg: VerifyConfig,
    /// Life of the statements `publish` and `refresh` write.
    pub statement_life_secs: i64,
}

/// The verified head and what was read from it.
struct Base {
    head: String,
    roster: VerifiedRoster,
    files: BTreeMap<String, Vec<u8>>,
}

/// A proposal branch as fetched: its tip and the two blobs read from it.
struct Fetched {
    tip: String,
    epoch_json: Vec<u8>,
    sigs: Option<Vec<u8>>,
}

/// Removes the staging ref when the operation ends, however it ends.
struct RefGuard<'a>(&'a Store, String);

impl Drop for RefGuard<'_> {
    fn drop(&mut self) {
        self.0.drop_ref(&self.1);
    }
}

fn proposal_branch(n: u64) -> String {
    format!("roster-proposal/{n}")
}

impl<'a> Admin<'a> {
    pub fn new(store: &'a Store, cfg: VerifyConfig) -> Admin<'a> {
        let life = DEFAULT_LIFE_SECS.min(cfg.policy.max_validity_secs);
        Admin { store, cfg, statement_life_secs: life }
    }

    // -- Commands that create a proposal -------------------------------------------------------------------

    /// `admin add`: propose a roster epoch that adds a member.
    pub fn add(&self, me: &Identity, member: admin::NewMember, approver: &mut dyn AdminApprover) -> Result<Proposed, AdminError> {
        self.propose(me, Op::Add(member), approver)
    }

    /// `admin remove`: propose marking a member `removed` (`voids_tombstones` is for cause).
    pub fn remove(&self, me: &Identity, did: &str, voids_tombstones: bool, approver: &mut dyn AdminApprover) -> Result<Proposed, AdminError> {
        self.propose(me, Op::Remove { did: did.to_string(), voids_tombstones }, approver)
    }

    /// `admin rotate`: propose retiring `old` in favour of `new`. The reason is a required argument of type
    /// [`admin::Rotation`], which has no default; `Compromised` forces `voids_tombstones`.
    pub fn rotate(&self, me: &Identity, old: &str, new: &str, kind: admin::Rotation, approver: &mut dyn AdminApprover) -> Result<Proposed, AdminError> {
        self.propose(me, Op::Rotate { old: old.to_string(), new: new.to_string(), kind }, approver)
    }

    /// `admin propose`: draft the next epoch, show it, sign it as the first signer and push it to
    /// `roster-proposal/<n>`. Refuses if that branch already exists.
    pub fn propose(&self, me: &Identity, op: Op, approver: &mut dyn AdminApprover) -> Result<Proposed, AdminError> {
        let base = self.base()?;
        let head = base.roster.head();
        require_admin(head, &me.did())?;
        let d = admin::draft(head, &op, &time::format_utc(self.cfg.now))?;
        let n = d.epoch.epoch;
        let (changes, warnings) = admin::describe(&head.epoch, &d.epoch);
        let need = head.epoch.admin_people().len() / 2 + 1;
        let me_handle = head.epoch.member(&me.did()).map(|m| m.handle.clone()).unwrap_or_default();
        let preview = AdminPreview {
            action: Action::Propose,
            epoch: n,
            epoch_hash: d.hash.clone(),
            changes: changes.iter().map(|c| c.line()).collect(),
            warnings: warnings.iter().map(|w| w.line()).collect(),
            signed: vec![me_handle],
            have: 1,
            need,
            store_head: None,
            statement: None,
            notes: vec![format!("this writes roster-proposal/{n}; nothing reaches the main branch until {need} admins have signed and one runs publish")],
            digest: digest(Action::Propose, n, &d.hash, 0, 0),
        };
        let want = digest(Action::Propose, n, &hash::of(&d.bytes), 0, 0);
        check_approval(approver.decide(&preview), &want, !preview.warnings.is_empty())?;
        let sig = admin::sign_epoch(me, &self.store.org, &self.store.store, n, &d.bytes);
        let sigs = admin::merge_sigs(&head.epoch, n, &d.bytes, None, Some(sig)).map_err(|e| AdminError::Rule(RuleError::Roster(e.reason())))?;
        let files = vec![(roster_epoch(n), d.bytes.clone()), (roster_epoch_sigs(n), sigs)];
        let commit = self.build_commit(Some(&base.head), &[&base.head], &files, &format!("xenia: propose roster epoch {n}"))?;
        match push_to(self.store, &commit, &proposal_branch(n)).map_err(AdminError::from)? {
            PushOutcome::Pushed => Ok(Proposed { epoch: n, epoch_hash: d.hash, branch: proposal_branch(n), commit }),
            PushOutcome::NotFastForward => Err(AdminError::ProposalExists(n)),
        }
    }

    // -- Signing -------------------------------------------------------------------------------------------

    /// `admin sign <n>`: fetch the proposal, check that it is the next link of the verified chain, show every
    /// change, and add this key's signature over the epoch file's exact bytes.
    pub fn sign(&self, me: &Identity, n: u64, approver: &mut dyn AdminApprover) -> Result<Cosigned, AdminError> {
        let mut approved: Option<String> = None;
        for _attempt in 0..=MAX_ATTEMPTS {
            let base = self.base()?;
            let head = base.roster.head();
            require_admin(head, &me.did())?;
            let _guard = RefGuard(self.store, format!("refs/xenia/proposal/{n}"));
            let fetched = self.fetch_proposal(n)?;
            let d = self.check_proposal(head, n, &fetched)?;
            if approved.as_deref().is_some_and(|h| h != d.hash) {
                return Err(AdminError::ProposalChanged);
            }
            let q = admin::quorum(&head.epoch, n, &d.bytes, fetched.sigs.as_deref().unwrap_or(&admin::empty_sigs(n))).map_err(|e| AdminError::Rule(RuleError::Roster(e.reason())))?;
            let me_person = head.epoch.member(&me.did()).map(|m| m.person.clone()).unwrap_or_default();
            let already = q.signed.iter().any(|h| head.epoch.members.iter().any(|m| &m.handle == h && m.person == me_person));
            if already {
                return Ok(Cosigned { epoch: n, epoch_hash: d.hash, have: q.have, need: q.need, commit: None });
            }
            let (changes, warnings) = admin::describe(&head.epoch, &d.epoch);
            if approved.is_none() {
                let mut signed = q.signed.clone();
                signed.push(head.epoch.member(&me.did()).map(|m| m.handle.clone()).unwrap_or_default());
                let preview = AdminPreview {
                    action: Action::Sign,
                    epoch: n,
                    epoch_hash: d.hash.clone(),
                    changes: changes.iter().map(|c| c.line()).collect(),
                    warnings: warnings.iter().map(|w| w.line()).collect(),
                    signed,
                    have: q.have + 1,
                    need: q.need,
                    store_head: None,
                    statement: None,
                    notes: if q.ignored > 0 { vec![format!("{} signature(s) in the file count for nothing (other bytes, not an admin, repeats)", q.ignored)] } else { Vec::new() },
                    digest: digest(Action::Sign, n, &d.hash, 0, 0),
                };
                let want = digest(Action::Sign, n, &hash::of(&d.bytes), 0, 0);
                check_approval(approver.decide(&preview), &want, !preview.warnings.is_empty())?;
                approved = Some(d.hash.clone());
            }
            let sig = admin::sign_epoch(me, &self.store.org, &self.store.store, n, &d.bytes);
            let merged = admin::merge_sigs(&head.epoch, n, &d.bytes, fetched.sigs.as_deref(), Some(sig)).map_err(|e| AdminError::Rule(RuleError::Roster(e.reason())))?;
            let files = vec![(roster_epoch(n), d.bytes.clone()), (roster_epoch_sigs(n), merged.clone())];
            let commit = self.build_commit(Some(&base.head), &[&fetched.tip], &files, &format!("xenia: sign roster epoch {n}"))?;
            match push_to(self.store, &commit, &proposal_branch(n)).map_err(AdminError::from)? {
                PushOutcome::Pushed => {
                    let after = admin::quorum(&head.epoch, n, &d.bytes, &merged).map_err(|e| AdminError::Rule(RuleError::Roster(e.reason())))?;
                    return Ok(Cosigned { epoch: n, epoch_hash: d.hash, have: after.have, need: after.need, commit: Some(commit) });
                }
                // Another admin signed first: fetch again and add ours on top of theirs.
                PushOutcome::NotFastForward => continue,
            }
        }
        Err(AdminError::TooManyRaces)
    }

    // -- Publishing ----------------------------------------------------------------------------------------

    /// `admin publish <n>`: with a quorum of signatures in hand, write epoch `n`, the signatures that count and a
    /// fresh statement for the new epoch to main, as one fast-forward commit built on the verified head.
    pub fn publish(&self, me: &Identity, n: u64, approver: &mut dyn AdminApprover) -> Result<EpochPublished, AdminError> {
        let mut approved: Option<String> = None;
        let issued = self.cfg.now;
        for attempt in 1..=MAX_ATTEMPTS + 1 {
            let base = self.base()?;
            let head = base.roster.head();
            require_admin(head, &me.did())?;
            let _guard = RefGuard(self.store, format!("refs/xenia/proposal/{n}"));
            let fetched = self.fetch_proposal(n)?;
            let d = self.check_proposal(head, n, &fetched)?;
            if approved.as_deref().is_some_and(|h| h != d.hash) {
                return Err(AdminError::ProposalChanged);
            }
            // Only signatures that count are carried onto main; everything else in the file is dropped.
            let merged = admin::merge_sigs(&head.epoch, n, &d.bytes, fetched.sigs.as_deref(), None).map_err(|e| AdminError::Rule(RuleError::Roster(e.reason())))?;
            let q = admin::quorum(&head.epoch, n, &d.bytes, &merged).map_err(|e| AdminError::Rule(RuleError::Roster(e.reason())))?;
            if !q.met() {
                return Err(AdminError::QuorumNotMet { have: q.have, need: q.need });
            }
            if !d.epoch.active_admins().contains_key(me.did().as_str()) {
                return Err(AdminError::NotActiveInNewEpoch);
            }
            let (changes, warnings) = admin::describe(&head.epoch, &d.epoch);
            if approved.is_none() {
                let preview = AdminPreview {
                    action: Action::Publish,
                    epoch: n,
                    epoch_hash: d.hash.clone(),
                    changes: changes.iter().map(|c| c.line()).collect(),
                    warnings: warnings.iter().map(|w| w.line()).collect(),
                    signed: q.signed.clone(),
                    have: q.have,
                    need: q.need,
                    store_head: Some(base.head.clone()),
                    statement: Some((time::format_utc(issued), time::format_utc(issued + self.statement_life_secs))),
                    notes: vec!["this fast-forwards the main branch with the epoch, its signatures and a statement for the new epoch".into()],
                    digest: digest(Action::Publish, n, &d.hash, issued, self.statement_life_secs),
                };
                let want = digest(Action::Publish, n, &hash::of(&d.bytes), issued, self.statement_life_secs);
                check_approval(approver.decide(&preview), &want, !preview.warnings.is_empty())?;
                approved = Some(d.hash.clone());
            }
            let (stmt, stmt_sigs) = admin::sign_statement(me, &self.store.org, &self.store.store, n, &d.hash, &base.head, issued, self.statement_life_secs);
            let files = vec![(roster_epoch(n), d.bytes.clone()), (roster_epoch_sigs(n), merged), (FRESHNESS.to_string(), stmt), (FRESHNESS_SIGS.to_string(), stmt_sigs)];
            let commit = self.build_commit(Some(&base.head), &[&base.head], &files, &format!("xenia: roster epoch {n}"))?;
            self.verify_candidate(&base, &commit, &files, n, &d.hash)?;
            match push_to(self.store, &commit, &self.store.branch).map_err(AdminError::from)? {
                PushOutcome::Pushed => {
                    let adopted = matches!(self.store.sync_verified(&self.cfg), Ok(VerifiedSync::Accepted { .. } | VerifiedSync::UpToDate { .. }));
                    return Ok(EpochPublished { epoch: n, epoch_hash: d.hash, commit, attempts: attempt, adopted });
                }
                PushOutcome::NotFastForward => continue,
            }
        }
        Err(AdminError::TooManyRaces)
    }

    // -- Refreshing ----------------------------------------------------------------------------------------

    /// `admin refresh`: verify everything, then (and only then) sign a new statement for the head epoch naming the
    /// head that was verified.
    ///
    /// Order: the full verified sync (chain against the pins, previous statement, history walk from this install's
    /// accepted head, record verification, sticky rules), then the history walk again from the previous authentic
    /// statement's `store_head`. Any tampering finding refuses and nothing is signed.
    pub fn refresh(&self, me: &Identity, approver: &mut dyn AdminApprover) -> Result<Refreshed, AdminError> {
        let issued = self.cfg.now;
        let mut approved = false;
        for _attempt in 0..=MAX_ATTEMPTS {
            let base = self.base()?;
            let head = base.roster.head();
            require_admin(head, &me.did())?;
            let n = head.epoch.epoch;
            let previous = self.previous_statement(&base);
            if let Some(p) = &previous {
                // Check 3 from the statement's own head, in addition to the accepted-head walk the sync did.
                let pinned = Pinned { org: &self.store.org, store: &self.store.store };
                let ctx = Ctx { pinned, roster: &base.roster };
                let findings = walk::walk(&GitSource::new(self.store), &ctx, &p.statement.store_head, &base.head).map_err(|e| AdminError::StoreNotUsable(format!("cannot walk from the previous statement: {e}")))?;
                if !findings.is_empty() {
                    return Err(AdminError::Tampering(findings.iter().map(|f| (f.path.clone(), f.reason)).collect()));
                }
                if p.statement.issued >= issued {
                    return Err(AdminError::ClockBehind);
                }
            }
            if !approved {
                let mut notes = Vec::new();
                match &previous {
                    Some(p) => notes.push(format!("previous statement issued {}, expires {}", p.statement.issued_at, p.statement.expires_at)),
                    None => notes.push("there is no authentic previous statement".into()),
                }
                let bad = self.unverifiable(&base);
                if !bad.is_empty() {
                    notes.push(format!("{} file(s) do not verify and are not in force: {}", bad.len(), bad.iter().take(5).map(|(p, r)| format!("{p} ({r})")).collect::<Vec<_>>().join(", ")));
                }
                let preview = AdminPreview {
                    action: Action::Refresh,
                    epoch: n,
                    epoch_hash: head.hash.clone(),
                    changes: Vec::new(),
                    warnings: Vec::new(),
                    signed: Vec::new(),
                    have: 0,
                    need: 0,
                    store_head: Some(base.head.clone()),
                    statement: Some((time::format_utc(issued), time::format_utc(issued + self.statement_life_secs))),
                    notes,
                    digest: digest(Action::Refresh, n, &head.hash, issued, self.statement_life_secs),
                };
                let want = digest(Action::Refresh, n, &head.hash, issued, self.statement_life_secs);
                check_approval(approver.decide(&preview), &want, false)?;
                approved = true;
            }
            let (stmt, stmt_sigs) = admin::sign_statement(me, &self.store.org, &self.store.store, n, &head.hash, &base.head, issued, self.statement_life_secs);
            let files = vec![(FRESHNESS.to_string(), stmt), (FRESHNESS_SIGS.to_string(), stmt_sigs)];
            let commit = self.build_commit(Some(&base.head), &[&base.head], &files, &format!("xenia: refresh statement for epoch {n}"))?;
            self.verify_candidate(&base, &commit, &files, n, &head.hash)?;
            match push_to(self.store, &commit, &self.store.branch).map_err(AdminError::from)? {
                PushOutcome::Pushed => {
                    let adopted = matches!(self.store.sync_verified(&self.cfg), Ok(VerifiedSync::Accepted { .. } | VerifiedSync::UpToDate { .. }));
                    return Ok(Refreshed { commit, store_head: base.head, expires_at: time::format_utc(issued + self.statement_life_secs), adopted });
                }
                PushOutcome::NotFastForward => continue,
            }
        }
        Err(AdminError::TooManyRaces)
    }

    // -- Genesis -------------------------------------------------------------------------------------------

    /// A founder's half of creating a store: check that `bytes` is a genesis epoch for this store that lists this key
    /// as an `active` admin, show the founders, and sign the exact bytes. Every founding admin must do this
    /// (spec 011 §3.2); the signatures are then handed to [`Admin::publish_genesis`].
    pub fn sign_genesis(&self, me: &Identity, bytes: &[u8], approver: &mut dyn AdminApprover) -> Result<(String, String), AdminError> {
        let e = self.check_genesis_bytes(bytes)?;
        if !e.active_admins().contains_key(me.did().as_str()) {
            return Err(AdminError::NotAnAdmin);
        }
        let hash = hash::of(bytes);
        let nobody = delphi_xenia::roster::Epoch { org: e.org.clone(), store: e.store.clone(), epoch: 0, prev_hash: None, members: Vec::new() };
        let (changes, warnings) = admin::describe(&nobody, &e);
        let preview = AdminPreview {
            action: Action::GenesisSign,
            epoch: 1,
            epoch_hash: hash.clone(),
            changes: changes.iter().map(|c| c.line()).collect(),
            warnings: warnings.iter().map(|w| w.line()).collect(),
            signed: Vec::new(),
            have: 0,
            need: e.active_admins().len(),
            store_head: None,
            statement: None,
            notes: vec!["every founding admin must sign these exact bytes; this key is one of them".into()],
            digest: digest(Action::GenesisSign, 1, &hash, 0, 0),
        };
        let want = digest(Action::GenesisSign, 1, &hash::of(bytes), 0, 0);
        check_approval(approver.decide(&preview), &want, !preview.warnings.is_empty())?;
        Ok(admin::sign_epoch(me, &self.store.org, &self.store.store, 1, bytes))
    }

    /// Create the store: with every founder's signature in `sigs`, write the genesis epoch, the root attributes file,
    /// `org.toml` and a statement for epoch 1 (spec 011 §4.1; a fresh install with no authentic statement accepts
    /// nothing, so a store without one is unusable) as two commits, push them as the first commits of `main`, and join
    /// this install at epoch 1 with the publisher's own fingerprint.
    ///
    /// The statement's `store_head` is the epoch commit (a commit cannot name itself). The remote must have no
    /// `main`: a rejected push is [`AdminError::StoreNotEmpty`], and nothing is joined or written locally in that case.
    pub fn publish_genesis(&self, me: &Identity, bytes: &[u8], sigs: &[u8], approver: &mut dyn AdminApprover) -> Result<GenesisPublished, AdminError> {
        if pins::load(&self.store.pin_storage()).map_err(|e| AdminError::StoreNotUsable(e.reason().into()))?.is_some() {
            return Err(AdminError::StoreNotUsable("this install is already joined to a store".into()));
        }
        let e = self.check_genesis_bytes(bytes)?;
        if !e.active_admins().contains_key(me.did().as_str()) {
            return Err(AdminError::NotAnAdmin);
        }
        let roster = admin::verify_genesis(&self.store.org, &self.store.store, bytes, sigs).map_err(|r| AdminError::Rule(RuleError::Roster(r.reason())))?;
        let epoch_hash = roster.head_hash().to_string();
        let issued = self.cfg.now;
        let nobody = delphi_xenia::roster::Epoch { org: e.org.clone(), store: e.store.clone(), epoch: 0, prev_hash: None, members: Vec::new() };
        let (changes, warnings) = admin::describe(&nobody, &e);
        let handles: Vec<String> = e.members.iter().filter(|m| m.is_admin()).map(|m| m.handle.clone()).collect();
        let preview = AdminPreview {
            action: Action::GenesisPublish,
            epoch: 1,
            epoch_hash: epoch_hash.clone(),
            changes: changes.iter().map(|c| c.line()).collect(),
            warnings: warnings.iter().map(|w| w.line()).collect(),
            signed: handles.clone(),
            have: handles.len(),
            need: handles.len(),
            store_head: None,
            statement: Some((time::format_utc(issued), time::format_utc(issued + self.statement_life_secs))),
            notes: vec!["this creates the first commits of the main branch of an empty store".into()],
            digest: digest(Action::GenesisPublish, 1, &epoch_hash, issued, self.statement_life_secs),
        };
        let want = digest(Action::GenesisPublish, 1, &hash::of(bytes), issued, self.statement_life_secs);
        check_approval(approver.decide(&preview), &want, !preview.warnings.is_empty())?;
        let toml = format!("org = \"{}\"\nstore = \"{}\"\n", self.store.org, self.store.store);
        let first = vec![
            (".gitattributes".to_string(), ATTRIBUTES.as_bytes().to_vec()),
            ("org.toml".to_string(), toml.into_bytes()),
            (roster_epoch(1), bytes.to_vec()),
            (roster_epoch_sigs(1), sigs.to_vec()),
        ];
        let epoch_commit = self.build_commit(None, &[], &first, "xenia: genesis")?;
        let (stmt, stmt_sigs) = admin::sign_statement(me, &self.store.org, &self.store.store, 1, &epoch_hash, &epoch_commit, issued, self.statement_life_secs);
        let second = vec![(FRESHNESS.to_string(), stmt.clone()), (FRESHNESS_SIGS.to_string(), stmt_sigs.clone())];
        let commit = self.build_commit(Some(&epoch_commit), &[&epoch_commit], &second, "xenia: statement for epoch 1")?;
        // The same checks an installing member will make, before anything is pushed: the tree rules, the chain with
        // every founder's signature, and a statement that is authentic for it and names the epoch commit.
        let mut problems = Vec::new();
        match self.store.check_tip(&commit).map_err(io)? {
            Ok(_) => {}
            Err(r) => problems.push(format!("the new tree is refused by the sync rules ({})", r.reason())),
        }
        match freshness::authenticate(&self.store.org, &self.store.store, &stmt, &stmt_sigs, &roster) {
            Ok(a) if a.statement.store_head == epoch_commit => {}
            Ok(_) => problems.push("the statement does not name the epoch commit".into()),
            Err(why) => problems.push(format!("the statement is not authentic ({why})")),
        }
        if !problems.is_empty() {
            return Err(AdminError::VerifyFailed(problems));
        }
        match push_to(self.store, &commit, &self.store.branch).map_err(AdminError::from)? {
            PushOutcome::Pushed => {}
            PushOutcome::NotFastForward => return Err(AdminError::StoreNotEmpty),
        }
        let req = pins::JoinRequest { url: self.store.url.url.clone(), admin_fingerprint: me.fingerprint(), head_epoch: 1, head_hash: epoch_hash.clone() };
        let files = [delphi_xenia::roster::EpochFile { epoch_json: bytes.to_vec(), sigs_json: sigs.to_vec() }];
        pins::join(&self.store.pin_storage(), &req, &files).map_err(|e| AdminError::StoreNotUsable(e.reason().into()))?;
        let adopted = matches!(self.store.sync_verified(&self.cfg), Ok(VerifiedSync::Accepted { .. } | VerifiedSync::UpToDate { .. }));
        Ok(GenesisPublished { commit, epoch_commit, epoch_hash, adopted })
    }

    fn check_genesis_bytes(&self, bytes: &[u8]) -> Result<delphi_xenia::roster::Epoch, AdminError> {
        let e = delphi_xenia::roster::Epoch::parse(bytes).map_err(|r| AdminError::Rule(RuleError::Roster(r.reason())))?;
        if e.org != self.store.org || e.store != self.store.store {
            return Err(AdminError::Rule(RuleError::WrongScope));
        }
        if e.epoch != 1 || e.prev_hash.is_some() {
            return Err(AdminError::Rule(RuleError::WrongEpoch { expected: 1, found: e.epoch }));
        }
        Ok(e)
    }

    // -- Internals -----------------------------------------------------------------------------------------

    /// A verified sync, then the accepted head's roster and files. Anything but an accepted or unchanged head is a
    /// refusal; tampering carries its findings.
    fn base(&self) -> Result<Base, AdminError> {
        if pins::load(&self.store.pin_storage()).map_err(|e| AdminError::StoreNotUsable(e.reason().into()))?.is_none() {
            return Err(AdminError::StoreNotUsable("this store has not been joined".into()));
        }
        let head = match self.store.sync_verified(&self.cfg).map_err(io)? {
            VerifiedSync::Accepted { head, .. } | VerifiedSync::UpToDate { head, .. } => head,
            VerifiedSync::Tampering { findings, .. } => return Err(AdminError::Tampering(findings.iter().take(50).map(|f| (f.path.clone(), f.reason)).collect())),
            VerifiedSync::Rejected { reason, .. } => return Err(AdminError::StoreNotUsable(reason.into())),
            VerifiedSync::NonFastForward(nf) => {
                return Err(AdminError::RemoteRewrote(match nf.kind {
                    crate::xenia_store::Divergence::Diverged => "the history diverged",
                    crate::xenia_store::Divergence::Behind => "the store is older than what was accepted",
                }))
            }
            VerifiedSync::Refused(r) => return Err(AdminError::StoreNotUsable(r.reason().into())),
            VerifiedSync::Unavailable(m) => return Err(AdminError::Unavailable(m)),
        };
        let snap = self.store.snapshot_of(&head).map_err(io)?;
        let p = pins::load(&self.store.pin_storage()).map_err(|e| AdminError::StoreNotUsable(e.reason().into()))?.ok_or_else(|| AdminError::StoreNotUsable("this store has not been joined".into()))?;
        let roster = p.verify(&epoch_files(&snap.files)).map_err(|e| AdminError::StoreNotUsable(e.reason().into()))?;
        Ok(Base { head, roster, files: snap.files })
    }

    fn previous_statement(&self, base: &Base) -> Option<Authentic> {
        let st = base.files.get(FRESHNESS)?;
        let sg = base.files.get(FRESHNESS_SIGS)?;
        freshness::authenticate(&self.store.org, &self.store.store, st, sg, &base.roster).ok()
    }

    /// Record files at the head that fail signature or structure checks (not a tampering finding by themselves:
    /// a retired member's old records are legitimately out of force).
    fn unverifiable(&self, base: &Base) -> Vec<(String, &'static str)> {
        let pinned = Pinned { org: &self.store.org, store: &self.store.store };
        let mut out = Vec::new();
        for (path, md) in &base.files {
            let Some(delphi_xenia::store::PathClass::Record(rp)) = delphi_xenia::store::classify_path(path) else { continue };
            match base.files.get(&rp.sidecar()) {
                None => out.push((path.clone(), "no-sidecar")),
                Some(car) => {
                    if let Err(r) = delphi_xenia::record::validate_record(pinned, path, md, car, &|_| Ok(())) {
                        out.push((path.clone(), r.reason()));
                    }
                }
            }
        }
        out
    }

    fn check_proposal(&self, head: &VerifiedEpoch, n: u64, f: &Fetched) -> Result<Draft, AdminError> {
        let d = admin::check_proposal(head, &f.epoch_json).map_err(|e| match e {
            RuleError::WrongEpoch { expected, found } => AdminError::StaleProposal { next: expected, proposed: found },
            other => AdminError::Rule(other),
        })?;
        // The branch name and the file must agree, or a proposal for one epoch could be signed as another.
        if d.epoch.epoch != n {
            return Err(AdminError::StaleProposal { next: head.epoch.epoch + 1, proposed: d.epoch.epoch });
        }
        Ok(d)
    }

    /// Fetch `roster-proposal/<n>` into a staging ref (the caller's guard removes it) and read the two blobs.
    fn fetch_proposal(&self, n: u64) -> Result<Fetched, AdminError> {
        let tip = match self.store.fetch_aux(&proposal_branch(n), &format!("refs/xenia/proposal/{n}")).map_err(io)? {
            AuxFetch::Tip(t) => t,
            AuxFetch::Missing => return Err(AdminError::ProposalMissing(n)),
            AuxFetch::Refused(r) => return Err(AdminError::ProposalRefused(r.reason())),
            AuxFetch::Unavailable(m) => return Err(AdminError::Unavailable(m)),
        };
        let epoch_json = self.blob_at(&tip, &roster_epoch(n), MAX_EPOCH_BYTES)?.ok_or(AdminError::ProposalMissing(n))?;
        let sigs = self.blob_at(&tip, &roster_epoch_sigs(n), MAX_SIGS_BYTES)?;
        Ok(Fetched { tip, epoch_json, sigs })
    }

    /// One file of a commit's tree, by name, read by object id. Only a regular blob counts: a symlink, a gitlink or a
    /// directory under that name is "absent" (and an absent epoch file ends the operation).
    fn blob_at(&self, commit: &str, path: &str, cap: usize) -> Result<Option<Vec<u8>>, AdminError> {
        let listing = run_git(self.store, &["ls-tree", "-z", commit, "--", path], &[], None)?;
        let text = String::from_utf8_lossy(&listing).into_owned();
        let Some(entry) = text.split('\0').find(|e| !e.is_empty()) else { return Ok(None) };
        let Some((meta, name)) = entry.split_once('\t') else { return Ok(None) };
        let f: Vec<&str> = meta.split(' ').collect();
        let [mode, kind, sha] = f[..] else { return Ok(None) };
        if name != path || mode != "100644" || kind != "blob" || !delphi_xenia::sticky::is_commit_id(sha) {
            return Ok(None);
        }
        let bytes = run_git(self.store, &["cat-file", "blob", sha], &[], None)?;
        if bytes.len() > cap {
            return Err(AdminError::ProposalRefused("file-too-large"));
        }
        Ok(Some(bytes))
    }

    /// A new commit whose tree is `tree_of`'s tree plus `files` and whose parents are `parents`, built with git
    /// plumbing through a private temporary index. No ref is touched: the commit is an unreferenced object until
    /// (and unless) it is pushed.
    fn build_commit(&self, tree_of: Option<&str>, parents: &[&str], files: &[(String, Vec<u8>)], msg: &str) -> Result<String, AdminError> {
        let index = self.store.paths.state.join(format!(".admin-{}.index", std::process::id()));
        let _ = std::fs::remove_file(&index);
        let idx = index.to_string_lossy().into_owned();
        let env = [("GIT_INDEX_FILE", idx.as_str())];
        let result = (|| -> Result<String, AdminError> {
            if let Some(t) = tree_of {
                run_git(self.store, &["read-tree", t], &env, None)?;
            }
            for (path, bytes) in files {
                let blob = String::from_utf8_lossy(&run_git(self.store, &["hash-object", "-w", "--no-filters", "--stdin"], &[], Some(bytes.clone()))?).trim().to_string();
                run_git(self.store, &["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{path}")], &env, None)?;
            }
            let tree = String::from_utf8_lossy(&run_git(self.store, &["write-tree"], &env, None)?).trim().to_string();
            let date = format!("{} +0000", self.cfg.now.max(0));
            let env2 = [
                ("GIT_AUTHOR_NAME", "delphi-xenia"),
                ("GIT_AUTHOR_EMAIL", "xenia@delphi.invalid"),
                ("GIT_COMMITTER_NAME", "delphi-xenia"),
                ("GIT_COMMITTER_EMAIL", "xenia@delphi.invalid"),
                ("GIT_AUTHOR_DATE", date.as_str()),
                ("GIT_COMMITTER_DATE", date.as_str()),
            ];
            let mut args: Vec<&str> = vec!["commit-tree", &tree];
            for p in parents {
                args.push("-p");
                args.push(p);
            }
            args.push("-m");
            args.push(msg);
            let commit = String::from_utf8_lossy(&run_git(self.store, &args, &env2, None)?).trim().to_string();
            if !delphi_xenia::sticky::is_commit_id(&commit) {
                return Err(AdminError::Io("git commit-tree printed something unexpected".into()));
            }
            Ok(commit)
        })();
        let _ = std::fs::remove_file(&index);
        result
    }

    /// Re-verify the head we are about to push with the verifier's own code: only our files changed, nothing was
    /// deleted, the chain at the candidate verifies against the pins and ends at the epoch we mean, the statement is
    /// authentic for it and names the head we verified, and the history walk from that head finds nothing.
    fn verify_candidate(&self, base: &Base, candidate: &str, ours: &[(String, Vec<u8>)], epoch: u64, epoch_hash: &str) -> Result<(), AdminError> {
        let snap = self.store.snapshot_of(candidate).map_err(io)?;
        let mut problems: Vec<String> = Vec::new();
        let names: BTreeSet<&str> = ours.iter().map(|(p, _)| p.as_str()).collect();
        for (path, bytes) in &snap.files {
            if !names.contains(path.as_str()) && base.files.get(path) != Some(bytes) {
                problems.push("the commit changed a file that is not ours".into());
            }
        }
        for path in base.files.keys() {
            if !snap.files.contains_key(path) {
                problems.push("the commit deleted a file".into());
            }
        }
        for (path, bytes) in ours {
            if snap.files.get(path) != Some(bytes) {
                problems.push(format!("{path} is not the signed bytes"));
            }
        }
        let loaded = pins::load(&self.store.pin_storage()).ok().flatten();
        let roster = loaded.as_ref().map(|p| p.verify(&epoch_files(&snap.files)));
        match roster {
            Some(Ok(r)) => {
                if r.head_epoch() != epoch || r.head_hash() != epoch_hash {
                    problems.push("the chain at the candidate does not end at the intended epoch".into());
                }
                match (snap.files.get(FRESHNESS), snap.files.get(FRESHNESS_SIGS)) {
                    (Some(st), Some(sg)) => match freshness::authenticate(&self.store.org, &self.store.store, st, sg, &r) {
                        Ok(a) => {
                            if a.statement.store_head != base.head {
                                problems.push("the statement does not name the head that was verified".into());
                            }
                        }
                        Err(why) => problems.push(format!("the statement is not authentic ({why})")),
                    },
                    _ => problems.push("the candidate has no statement".into()),
                }
                if problems.is_empty() {
                    let pinned = Pinned { org: &self.store.org, store: &self.store.store };
                    let ctx = Ctx { pinned, roster: &r };
                    let findings = walk::walk(&GitSource::new(self.store), &ctx, &base.head, candidate).map_err(io)?;
                    for f in findings.iter().take(8) {
                        problems.push(format!("{}: {}", f.path, f.reason));
                    }
                }
            }
            Some(Err(e)) => problems.push(format!("the chain at the candidate does not verify ({})", e.reason())),
            None => problems.push("this store has not been joined".into()),
        }
        if problems.is_empty() { Ok(()) } else { Err(AdminError::VerifyFailed(problems)) }
    }
}

fn require_admin(head: &VerifiedEpoch, did: &str) -> Result<(), AdminError> {
    match head.epoch.member(did) {
        Some(m) if m.is_active() && m.is_admin() => Ok(()),
        _ => Err(AdminError::NotAnAdmin),
    }
}

#[cfg(test)]
#[path = "xenia_admin_tests.rs"]
mod tests;
