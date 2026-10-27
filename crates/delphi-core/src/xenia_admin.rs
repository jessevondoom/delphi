//! Admin operations (spec 011 §3.2, §4.1, §8; plan milestone X8): genesis, `add`/`remove`/`rotate` as roster
//! proposals, `sign`, `publish` and `refresh`. Library API only; the CLI and the console card are X9.
//!
//! ```text
//! propose  : verified sync, build the next epoch from an Operation, judge it (admin::check_candidate), push it to
//!            roster-proposal/<n> (never force; an existing branch is refused)
//! sign     : verified sync, fetch the branch (hostile input), judge the epoch, preview diff + exact bytes,
//!            ONE call to the AdminApprover, then sign the roster Domain over those bytes, push the signature
//! publish  : same judgement, assemble signatures, ask the X3 chain verifier whether a quorum signed, preview,
//!            ONE approval, sign the new freshness statement, push epoch + signatures + statement fast-forward only
//! refresh  : verified sync, an explicit history walk from every base we can name, refuse on any finding,
//!            preview, ONE approval, sign a statement naming the verified head, push
//! genesis  : every founder signs the same draft; the first commit holds the roster, the second the first statement
//! ```
//!
//! # What is guaranteed, and by what
//!
//! * **No private key in a struct, a log or an error.** An [`Identity`] is a parameter of `sign` only.
//! * **The bytes shown are the bytes signed, and no model runs between.** [`Prepared`] is not `Clone` and `sign`
//!   consumes it; it recomputes the digest from the bytes it holds and the domains it will sign, and refuses an
//!   approval for any other digest. The only code between preview and signature is [`AdminApprover::decide`], which
//!   is given `&AdminPreview` (plain data). A test reads this file's source to pin that.
//! * **A proposal branch is hostile.** Only the epoch file and `roster/proposals/<n>/*.json` are read from it, by
//!   object id; nothing is checked out. The epoch is judged by `admin::check_candidate` (the verifier's parser and
//!   `check_transition`, plus admin policy) before anyone is asked to sign.
//! * **Quorum is the verifier's.** `publish` hands the chain plus the assembled signatures to
//!   `roster::verify_chain`; the number of people needed is read from that verifier too, never recomputed here.
//! * **Never force.** Every push has an explicit `<commit>:refs/heads/<branch>` refspec and no `--force`, `+` or
//!   lease ([`push_command`] is tested). A failed push leaves sticky state and pins byte-identical.
//! * **A refresh never signs over unverified history.** See [`Admin::prepare_refresh`].

use crate::xenia_publish::{NoIndex, Publisher};
use crate::xenia_store::{git_command, run, short, Fetch, RunFail, Store};
use crate::xenia_sync::{epoch_files, statement_files, GitSource, VerifyConfig};
use delphi_xenia::admin::{self, Change, Flag, NewMember, Operation, Policy, ProposalSig, Proposed, Refusal, Reminder};
use delphi_xenia::freshness;
use delphi_xenia::identity::Identity;
use delphi_xenia::pins::{self, JoinRequest, Pins};
use delphi_xenia::record::Pinned;
use delphi_xenia::roster::{self, Anchor, Epoch, EpochFile, RosterError, VerifiedRoster};
use delphi_xenia::sig::Domain;
use delphi_xenia::walk::{self, Ctx, Source};
use delphi_xenia::{did, hash, layout, time};
use std::collections::BTreeSet;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Rebuilds after a rejected push before giving up.
pub const MAX_REBASES: usize = 3;
/// What a store's root `.gitattributes` must say (spec 012 §7).
pub const GITATTRIBUTES: &str = "* -text\n";
const MAX_SIG_FILES: usize = 64;

// ---------------------------------------------------------------------------------------------------------
// Errors

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminError {
    Refused(Refusal),
    StoreNotUsable(String),
    /// The key is not an `active` admin of the epoch whose admins must approve (or sign the statement).
    NotAnApprover,
    ProposalNotFound(u64),
    ProposalExists(u64),
    /// The proposal's base is no longer the head (or its number is already taken).
    StaleProposal { head_epoch: u64 },
    /// The proposal branch changed under us between preview and delivery.
    ProposalChanged,
    QuorumNotMet { have: usize, need: usize },
    FlagsNotAcknowledged,
    Denied,
    ApprovalMismatch,
    WrongKey,
    /// The history walk found something; nothing was signed.
    HistoryFailed(Vec<String>),
    /// The head to push failed the verifier; nothing was pushed.
    VerifyFailed(Vec<String>),
    /// The local clock is not ahead of the last statement this install accepted: it would be ignored as a replay.
    ClockBehind,
    StoreNotEmpty,
    MissingFounderSignatures(Vec<String>),
    PushRejected(String),
    TooManyRaces,
    Unavailable(String),
    Io(String),
}

impl AdminError {
    pub fn reason(&self) -> &'static str {
        match self {
            AdminError::Refused(r) => r.reason(),
            AdminError::StoreNotUsable(_) => "store-not-usable",
            AdminError::NotAnApprover => "not-an-approver",
            AdminError::ProposalNotFound(_) => "proposal-not-found",
            AdminError::ProposalExists(_) => "proposal-exists",
            AdminError::StaleProposal { .. } => "stale-proposal",
            AdminError::ProposalChanged => "proposal-changed",
            AdminError::QuorumNotMet { .. } => "quorum-not-met",
            AdminError::FlagsNotAcknowledged => "flags-not-acknowledged",
            AdminError::Denied => "denied",
            AdminError::ApprovalMismatch => "approval-mismatch",
            AdminError::WrongKey => "wrong-key",
            AdminError::HistoryFailed(_) => "history-failed",
            AdminError::VerifyFailed(_) => "verify-failed",
            AdminError::ClockBehind => "clock-behind",
            AdminError::StoreNotEmpty => "store-not-empty",
            AdminError::MissingFounderSignatures(_) => "missing-founder-signatures",
            AdminError::PushRejected(_) => "push-rejected",
            AdminError::TooManyRaces => "too-many-races",
            AdminError::Unavailable(_) => "unavailable",
            AdminError::Io(_) => "io",
        }
    }
}

impl std::fmt::Display for AdminError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminError::Refused(r) => write!(f, "refused: {r}"),
            AdminError::StoreNotUsable(w) => write!(f, "refused: the store cannot be trusted right now ({w}); nothing was signed or sent"),
            AdminError::StaleProposal { head_epoch } => write!(f, "refused: the roster is already at epoch {head_epoch}; this proposal is stale"),
            AdminError::QuorumNotMet { have, need } => write!(f, "refused: {have} of {need} required admins have signed"),
            AdminError::HistoryFailed(w) | AdminError::VerifyFailed(w) => write!(f, "refused: {} ({}); nothing was signed or sent", self.reason(), w.join(", ")),
            AdminError::MissingFounderSignatures(w) => write!(f, "refused: {} founder(s) have not signed", w.len()),
            AdminError::PushRejected(w) => write!(f, "the store refused the push: {w}"),
            AdminError::Unavailable(w) | AdminError::Io(w) => write!(f, "{}: {w}", self.reason()),
            other => write!(f, "{}", other.reason()),
        }
    }
}

impl std::error::Error for AdminError {}

impl From<Refusal> for AdminError {
    fn from(r: Refusal) -> AdminError {
        AdminError::Refused(r)
    }
}

fn xe(e: delphi_xenia::Error) -> AdminError {
    AdminError::Io(e.to_string())
}

fn re(e: RosterError) -> AdminError {
    AdminError::Refused(Refusal::Roster(e))
}

// ---------------------------------------------------------------------------------------------------------
// Preview, decision, approval

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Genesis,
    SignEpoch,
    Publish,
    Refresh,
}

impl Action {
    fn slug(self) -> &'static str {
        match self {
            Action::Genesis => "genesis",
            Action::SignEpoch => "sign-epoch",
            Action::Publish => "publish",
            Action::Refresh => "refresh",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewFile {
    pub path: String,
    /// The exact bytes.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuorumView {
    pub met: bool,
    pub have: usize,
    pub need: usize,
    /// Persons who signed (for display).
    pub people: Vec<String>,
}

/// Everything the admin sees before saying yes. Plain data; changing a copy changes nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct AdminPreview {
    pub action: Action,
    pub org: String,
    pub store: String,
    pub epoch: u64,
    pub signer: String,
    pub files: Vec<PreviewFile>,
    /// The signing domains, as text: exactly what the key will sign.
    pub signs: Vec<String>,
    pub changes: Vec<Change>,
    pub flags: Vec<Flag>,
    pub reminders: Vec<Reminder>,
    pub quorum: Option<QuorumView>,
    pub store_head: Option<String>,
    pub expires_at: Option<String>,
    /// For a refresh: each base the history was walked from, and how many commits that covered.
    pub walked: Vec<(String, usize)>,
    pub digest: String,
}

impl AdminPreview {
    pub fn approve(&self, flags_acknowledged: bool) -> AdminApproval {
        AdminApproval { digest: self.digest.clone(), flags_acknowledged }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminApproval {
    digest: String,
    flags_acknowledged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminDecision {
    Approve(AdminApproval),
    Deny,
}

/// The admin's side. The **only** call between the preview and the signature; its argument is immutable data.
pub trait AdminApprover {
    fn decide(&mut self, preview: &AdminPreview) -> AdminDecision;
}

struct Held {
    action: Action,
    org: String,
    store: String,
    epoch: u64,
    signer: String,
    files: Vec<PreviewFile>,
    domains: Vec<Domain>,
    store_head: Option<String>,
    expires_at: Option<String>,
    flags: Vec<Flag>,
}

fn digest_of(h: &Held) -> String {
    let epoch = h.epoch.to_string();
    let mut parts: Vec<(String, Vec<u8>)> = vec![
        ("action".into(), h.action.slug().as_bytes().to_vec()),
        ("org".into(), h.org.clone().into_bytes()),
        ("store".into(), h.store.clone().into_bytes()),
        ("epoch".into(), epoch.into_bytes()),
        ("signer".into(), h.signer.clone().into_bytes()),
        ("store_head".into(), h.store_head.clone().unwrap_or_default().into_bytes()),
        ("expires_at".into(), h.expires_at.clone().unwrap_or_default().into_bytes()),
    ];
    for f in &h.files {
        parts.push((format!("file:{}", f.path), f.bytes.clone()));
    }
    for d in &h.domains {
        parts.push(("signs".into(), d.as_str().as_bytes().to_vec()));
    }
    let refs: Vec<(&str, &[u8])> = parts.iter().map(|(a, b)| (a.as_str(), b.as_slice())).collect();
    admin::digest(&refs)
}

/// A batch waiting for one decision. Not `Clone`; [`Prepared::sign`] consumes it.
pub struct Prepared<T> {
    held: Held,
    preview: AdminPreview,
    payload: T,
}

/// Signatures made over a [`Prepared`] after an approval. Constructible only by [`Prepared::sign`].
pub struct Signed<T> {
    payload: T,
    sigs: Vec<String>,
    signer: String,
}

impl<T> Prepared<T> {
    pub fn preview(&self) -> &AdminPreview {
        &self.preview
    }

    /// Sign every domain if and only if the approval is for exactly the bytes and domains held. Checks, in order:
    /// the key is the one prepared for; the digest recomputed from what is held equals the approval's (not the
    /// preview struct, which a caller could have edited); flags are acknowledged. Then it signs.
    pub fn sign(self, approval: &AdminApproval, key: &Identity) -> Result<Signed<T>, AdminError> {
        if key.did() != self.held.signer {
            return Err(AdminError::WrongKey);
        }
        if approval.digest != digest_of(&self.held) {
            return Err(AdminError::ApprovalMismatch);
        }
        if !self.held.flags.is_empty() && !approval.flags_acknowledged {
            return Err(AdminError::FlagsNotAcknowledged);
        }
        let sigs = self.held.domains.iter().map(|d| key.sign(d)).collect();
        Ok(Signed { payload: self.payload, sigs, signer: self.held.signer })
    }
}

fn make_prepared<T>(held: Held, preview_parts: PreviewParts, payload: T) -> Prepared<T> {
    let digest = digest_of(&held);
    let preview = AdminPreview {
        action: held.action,
        org: held.org.clone(),
        store: held.store.clone(),
        epoch: held.epoch,
        signer: held.signer.clone(),
        files: held.files.clone(),
        signs: held.domains.iter().map(|d| d.as_str().to_string()).collect(),
        changes: preview_parts.changes,
        flags: held.flags.clone(),
        reminders: preview_parts.reminders,
        quorum: preview_parts.quorum,
        store_head: held.store_head.clone(),
        expires_at: held.expires_at.clone(),
        walked: preview_parts.walked,
        digest,
    };
    Prepared { held, preview, payload }
}

#[derive(Default)]
struct PreviewParts {
    changes: Vec<Change>,
    reminders: Vec<Reminder>,
    quorum: Option<QuorumView>,
    walked: Vec<(String, usize)>,
}

/// Payloads: what a signature will be used for.
pub struct EpochSigPayload {
    n: u64,
    bytes: Vec<u8>,
}

pub struct PublishPayload {
    n: u64,
    epoch_hash: String,
    files: Vec<(String, Vec<u8>)>,
    statement: Vec<u8>,
}

pub struct RefreshPayload {
    epoch: u64,
    epoch_hash: String,
    statement: Vec<u8>,
}

pub struct GenesisPayload {
    first_commit: String,
    statement: Vec<u8>,
    epoch_hash: String,
}

impl Signed<EpochSigPayload> {
    /// The detached signature, as it goes on the proposal branch (or to the other founders).
    pub fn proposal_sig(&self) -> ProposalSig {
        ProposalSig { kid: self.signer.clone(), sig: self.sigs[0].clone() }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Reports

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalReport {
    pub n: u64,
    pub branch: String,
    pub epoch_hash: String,
    pub bytes: Vec<u8>,
    pub changes: Vec<Change>,
    pub flags: Vec<Flag>,
    pub reminders: Vec<Reminder>,
    pub quorum: QuorumView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub report: ProposalReport,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigDelivered {
    pub commit: String,
    pub already_there: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    /// The commit now at the store's branch (ours, or the head that already held our bytes).
    pub commit: String,
    pub parent: String,
    pub paths: Vec<String>,
    pub attempts: usize,
    pub already_there: bool,
}

/// What a joining member needs, out of band (spec 011 §3.3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinInfo {
    pub url: String,
    pub admin_fingerprint: String,
    pub head_epoch: u64,
    pub head_hash: String,
}

impl JoinInfo {
    pub fn request(&self) -> JoinRequest {
        JoinRequest { url: self.url.clone(), admin_fingerprint: self.admin_fingerprint.clone(), head_epoch: self.head_epoch, head_hash: self.head_hash.clone() }
    }

    pub fn command(&self) -> String {
        format!("delphi xenia join {} --admin {} --head {}:{}", self.url, self.admin_fingerprint, self.head_epoch, self.head_hash)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisOutcome {
    pub head: String,
    pub join: JoinInfo,
}

// ---------------------------------------------------------------------------------------------------------
// Genesis drafts

/// A genesis epoch and its bytes. Each founder rebuilds one from the bytes they are handed ([`GenesisDraft::from_bytes`])
/// and judges it before signing; nobody signs a draft they did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisDraft {
    org: String,
    store: String,
    proposed: Proposed,
}

impl GenesisDraft {
    pub fn build(org: &str, store: &str, founders: &[NewMember], now_ts: &str) -> Result<GenesisDraft, AdminError> {
        Ok(GenesisDraft { org: org.into(), store: store.into(), proposed: admin::build_genesis(org, store, founders, now_ts)? })
    }

    pub fn from_bytes(org: &str, store: &str, bytes: &[u8]) -> Result<GenesisDraft, AdminError> {
        let epoch = admin::check_genesis_draft(org, store, bytes)?;
        Ok(GenesisDraft { org: org.into(), store: store.into(), proposed: Proposed { epoch, bytes: bytes.to_vec(), hash: hash::of(bytes) } })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.proposed.bytes
    }

    pub fn hash(&self) -> &str {
        &self.proposed.hash
    }
}

/// A founder's signature on a genesis draft: preview of every founder and role, one decision, one signature.
pub fn prepare_genesis_signature(draft: &GenesisDraft, signer: &Identity) -> Result<Prepared<EpochSigPayload>, AdminError> {
    let me = signer.did();
    if !draft.proposed.epoch.active_admins().contains_key(me.as_str()) {
        return Err(AdminError::NotAnApprover);
    }
    let d = admin::diff(None, &draft.proposed.epoch);
    let domain = admin::epoch_domain(&draft.org, &draft.store, 1, &draft.proposed.bytes)?;
    let held = Held {
        action: Action::Genesis,
        org: draft.org.clone(),
        store: draft.store.clone(),
        epoch: 1,
        signer: me,
        files: vec![PreviewFile { path: layout::roster_epoch(1), bytes: draft.proposed.bytes.clone() }],
        domains: vec![domain],
        store_head: None,
        expires_at: None,
        flags: d.flags,
    };
    let parts = PreviewParts { changes: d.changes, reminders: d.reminders, ..Default::default() };
    Ok(make_prepared(held, parts, EpochSigPayload { n: 1, bytes: draft.proposed.bytes.clone() }))
}

// ---------------------------------------------------------------------------------------------------------
// Git plumbing (all through `git_command`)

static INDEX_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The push command. An explicit `<commit>:refs/heads/<branch>` refspec, no `+`, no `--force`, no lease: the
/// remote accepts it only as a fast-forward. Built in one place so a test can pin that.
pub fn push_command(store: &Store, commit: &str, branch: &str) -> Command {
    let mut c = git_command(&store.paths, Some(store.url.scheme), Some(&store.paths.clone));
    c.args(["push", "--quiet", "--no-verify", "--no-recurse-submodules"]).arg(&store.url.url).arg(format!("{commit}:refs/heads/{branch}"));
    c
}

enum Pushed {
    Yes,
    NotFastForward,
}

/// Deletes a staging ref when dropped, so a fetched proposal leaves no ref behind.
struct Staged<'a> {
    store: &'a Store,
    name: String,
}

impl Drop for Staged<'_> {
    fn drop(&mut self) {
        let _ = self.store.git_ok(&["update-ref", "-d", &self.name], 4096);
    }
}

/// The admin operations for one store. `cfg.now` is the clock; nothing here reads one.
pub struct Admin<'a> {
    pub store: &'a Store,
    pub cfg: VerifyConfig,
    pub policy: Policy,
}

impl<'a> Admin<'a> {
    pub fn new(store: &'a Store, cfg: VerifyConfig) -> Admin<'a> {
        Admin { store, cfg, policy: Policy::default() }
    }

    fn now_ts(&self) -> String {
        time::format_utc(self.cfg.now)
    }

    fn git(&self, args: &[&str], env: &[(&str, &str)], stdin: Option<Vec<u8>>, cap: usize) -> Result<Vec<u8>, AdminError> {
        let mut c: Command = git_command(&self.store.paths, None, Some(&self.store.paths.clone));
        c.args(args);
        for (k, v) in env {
            c.env(k, v);
        }
        let out = run(c, stdin, cap, Duration::from_secs(60), None).map_err(|f| match f {
            RunFail::Timeout => AdminError::Io(format!("git {} timed out", args.first().unwrap_or(&""))),
            RunFail::TooMuchOutput => AdminError::Io(format!("git {} printed too much", args.first().unwrap_or(&""))),
            _ => AdminError::Io(format!("git {} failed", args.first().unwrap_or(&""))),
        })?;
        if out.code != 0 {
            return Err(AdminError::Io(format!("git {} failed: {}", args.first().unwrap_or(&""), short(&out.stderr))));
        }
        Ok(out.stdout)
    }

    /// A commit on `parent` (or a root commit) that sets `files`, built with plumbing through a private temporary
    /// index. No ref is touched: the commit is an unreferenced object until (and unless) it is pushed.
    fn commit_on(&self, parent: Option<&str>, files: &[(String, Vec<u8>)], msg: &str) -> Result<String, AdminError> {
        let index = self.store.paths.state.join(format!(".admin-{}-{}.index", std::process::id(), INDEX_COUNTER.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_file(&index);
        let idx = index.to_string_lossy().into_owned();
        let env = [("GIT_INDEX_FILE", idx.as_str())];
        let result = (|| {
            match parent {
                Some(p) => self.git(&["read-tree", p], &env, None, 1 << 16)?,
                None => self.git(&["read-tree", "--empty"], &env, None, 1 << 16)?,
            };
            for (path, bytes) in files {
                let blob = String::from_utf8_lossy(&self.git(&["hash-object", "-w", "--no-filters", "--stdin"], &[], Some(bytes.clone()), 4096)?).trim().to_string();
                self.git(&["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{path}")], &env, None, 4096)?;
            }
            let tree = String::from_utf8_lossy(&self.git(&["write-tree"], &env, None, 4096)?).trim().to_string();
            let date = format!("{} +0000", self.cfg.now.max(0));
            let env2 = [
                ("GIT_AUTHOR_NAME", "delphi-xenia"),
                ("GIT_AUTHOR_EMAIL", "xenia@delphi.invalid"),
                ("GIT_COMMITTER_NAME", "delphi-xenia"),
                ("GIT_COMMITTER_EMAIL", "xenia@delphi.invalid"),
                ("GIT_AUTHOR_DATE", date.as_str()),
                ("GIT_COMMITTER_DATE", date.as_str()),
            ];
            let mut args = vec!["commit-tree", tree.as_str()];
            if let Some(p) = parent {
                args.extend(["-p", p]);
            }
            args.extend(["-m", msg]);
            let commit = String::from_utf8_lossy(&self.git(&args, &env2, None, 4096)?).trim().to_string();
            if !delphi_xenia::sticky::is_commit_id(&commit) {
                return Err(AdminError::Io("git commit-tree printed something unexpected".into()));
            }
            Ok(commit)
        })();
        let _ = std::fs::remove_file(&index);
        result
    }

    fn push(&self, commit: &str, branch: &str) -> Result<Pushed, AdminError> {
        let cmd = push_command(self.store, commit, branch);
        let out = run(cmd, None, 64 * 1024, Duration::from_secs(self.store.caps.fetch_timeout_secs.max(10)), None).map_err(|f| match f {
            RunFail::Timeout => AdminError::Unavailable("the push timed out".into()),
            _ => AdminError::Io("git push failed to run".into()),
        })?;
        if out.code == 0 {
            return Ok(Pushed::Yes);
        }
        let lower = out.stderr.to_lowercase();
        if lower.contains("non-fast-forward") || lower.contains("fetch first") || lower.contains("[rejected]") && lower.contains("stale") {
            return Ok(Pushed::NotFastForward);
        }
        Err(AdminError::PushRejected(short(&out.stderr)))
    }

    /// A verified sync (X4b), then the head it accepted.
    pub fn verified_head(&self) -> Result<String, AdminError> {
        Publisher::new(self.store, self.cfg, &NoIndex).verified_head().map_err(|e| AdminError::StoreNotUsable(e.reason().to_string()))
    }

    fn load_pins(&self) -> Result<Pins, AdminError> {
        self.store.load_pins().map_err(|e| AdminError::StoreNotUsable(e.to_string()))
    }

    /// The roster at `head`, verified against the install's pins, and the epoch files it came from.
    fn head_roster(&self, head: &str) -> Result<(VerifiedRoster, Vec<EpochFile>, crate::xenia_store::Snapshot, Pins), AdminError> {
        let pins = self.load_pins()?;
        let snap = self.store.roster_snapshot(head).map_err(xe)?;
        let files = epoch_files(&snap);
        let roster = pins.verify(&files).map_err(|e| AdminError::StoreNotUsable(e.reason().to_string()))?;
        Ok((roster, files, snap, pins))
    }

    fn pinned(&self) -> Pinned<'_> {
        Pinned { org: &self.store.org, store: &self.store.store }
    }

    // -----------------------------------------------------------------------------------------------------
    // Proposal branches (hostile input)

    /// Fetch `roster-proposal/<n>` into a staging ref. `Ok(None)`: the host has no such branch.
    fn fetch_proposal(&self, n: u64) -> Result<Option<(Staged<'_>, String)>, AdminError> {
        let name = format!("refs/xenia/proposal/{n}");
        let objects = self.store.paths.clone.join("objects");
        let mut cmd = git_command(&self.store.paths, Some(self.store.url.scheme), Some(&self.store.paths.clone));
        cmd.args(["fetch", "--quiet", "--force", "--no-tags", "--no-recurse-submodules", "--no-write-fetch-head", "--no-auto-gc", "--no-auto-maintenance"]);
        cmd.arg(&self.store.url.url).arg(format!("+refs/heads/{}:{name}", admin::proposal_branch(n)));
        let staged = Staged { store: self.store, name: name.clone() };
        match run(cmd, None, 64 * 1024, Duration::from_secs(self.store.caps.fetch_timeout_secs), Some((&objects, self.store.caps.max_total_bytes))) {
            Err(RunFail::DiskCap) | Err(RunFail::TooMuchOutput) => return Err(AdminError::StoreNotUsable("the proposal branch is too large".into())),
            Err(RunFail::Timeout) => return Err(AdminError::Unavailable("the fetch timed out".into())),
            Err(RunFail::Spawn(e)) => return Err(AdminError::Io(format!("cannot run git: {e}"))),
            Ok(out) if out.code != 0 => {
                if out.stderr.contains("couldn't find remote ref") {
                    return Ok(None);
                }
                return Err(AdminError::Unavailable(short(&out.stderr)));
            }
            Ok(_) => {}
        }
        let tip = String::from_utf8_lossy(&self.store.git_ok(&["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")], 256).map_err(xe)?).trim().to_string();
        if !delphi_xenia::sticky::is_commit_id(&tip) {
            return Err(AdminError::StoreNotUsable("the proposal tip is not a commit".into()));
        }
        let accepted = self.store.accepted_head().map_err(xe)?;
        if let Some(r) = self.store.check_new_objects(&tip, accepted.as_deref()).map_err(xe)? {
            return Err(AdminError::StoreNotUsable(format!("the proposal branch was refused ({})", r.reason())));
        }
        Ok(Some((staged, tip)))
    }

    /// The epoch file and the signature files on a proposal tip, read by object id. Nothing else is read.
    fn read_proposal(&self, tip: &str, n: u64) -> Result<(Vec<u8>, Vec<ProposalSig>), AdminError> {
        let epoch_path = layout::roster_epoch(n);
        let bytes = self.store.git_ok(&["cat-file", "blob", &format!("{tip}:{epoch_path}")], roster::MAX_EPOCH_BYTES + 1).map_err(|_| AdminError::Refused(Refusal::BadInput("the proposal branch has no epoch file of the right size")))?;
        if bytes.len() > roster::MAX_EPOCH_BYTES {
            return Err(AdminError::Refused(Refusal::Roster(RosterError::Oversize("epoch file over 128 KiB"))));
        }
        let dir = admin::proposal_sig_dir(n);
        let listing = self.store.git_ok(&["ls-tree", "-z", "--name-only", tip, "--", &dir], 64 * 1024).map_err(xe)?;
        let mut paths = Vec::new();
        for name in listing.split(|b| *b == 0).filter(|t| !t.is_empty()) {
            let Ok(name) = std::str::from_utf8(name) else { continue };
            let Some(file) = name.strip_prefix(dir.as_str()) else { continue };
            let stem = file.strip_suffix(".json").unwrap_or("");
            if stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) && paths.len() < MAX_SIG_FILES {
                paths.push(name.to_string());
            }
        }
        let mut sigs = Vec::new();
        if !paths.is_empty() {
            let stdin: String = paths.iter().map(|p| format!("{tip}:{p}\n")).collect();
            let out = self.git(&["cat-file", "--batch"], &[], Some(stdin.into_bytes()), paths.len() * (admin::MAX_SIG_FILE_BYTES + 200) + 1024)?;
            let mut pos = 0usize;
            for p in &paths {
                let Some(nl) = out[pos..].iter().position(|b| *b == b'\n') else { break };
                let header = String::from_utf8_lossy(&out[pos..pos + nl]).into_owned();
                pos += nl + 1;
                let f: Vec<&str> = header.split(' ').collect();
                let size = match f[..] {
                    [_, "blob", s] => s.parse::<usize>().ok(),
                    _ => None,
                };
                let Some(size) = size else { continue };
                if out.len() < pos + size + 1 {
                    break;
                }
                let body = &out[pos..pos + size];
                pos += size + 1;
                // A file counts only if it parses and is named for its own key.
                if let Ok(s) = ProposalSig::parse(body) {
                    if admin::proposal_sig_path(n, &s.kid).as_deref() == Ok(p.as_str()) {
                        sigs.push(s);
                    }
                }
            }
        }
        Ok((bytes, sigs))
    }

    /// Everything about proposal `n` against the verified `head`, judged. Any refusal here means no one should be
    /// asked to sign.
    fn load_proposal(&self, head: &str, n: u64) -> Result<Loaded, AdminError> {
        let (roster, chain, _snap, pins) = self.head_roster(head)?;
        let head_epoch = roster.head_epoch();
        if n <= head_epoch {
            return Err(AdminError::StaleProposal { head_epoch });
        }
        if n != head_epoch + 1 {
            return Err(Refusal::NotNextEpoch { expected: head_epoch + 1, found: n }.into());
        }
        let (staged, tip) = self.fetch_proposal(n)?.ok_or(AdminError::ProposalNotFound(n))?;
        let (bytes, sigs) = self.read_proposal(&tip, n)?;
        drop(staged);
        let prev = &roster.head().epoch;
        let epoch = admin::check_candidate(prev, roster.head_hash(), &self.store.org, &self.store.store, &bytes, &self.policy)?;
        let hash = hash::of(&bytes);
        Ok(Loaded { head: head.to_string(), roster, chain, pins, n, bytes, epoch, hash, sigs })
    }

    /// Whether a quorum signed, as the X3 verifier says it. Returns the assembled signatures file and the view.
    fn quorum_of(&self, l: &Loaded) -> Result<(Vec<u8>, QuorumView, Option<VerifiedRoster>), AdminError> {
        let prev = &l.roster.head().epoch;
        let sigs_file = admin::assemble_sigs(&self.store.org, &self.store.store, l.n, &l.bytes, prev, &l.sigs);
        let anchor = l.pins.anchor();
        let try_with = |sigs: &[u8]| {
            let mut files = l.chain.clone();
            files.push(EpochFile { epoch_json: l.bytes.clone(), sigs_json: sigs.to_vec() });
            roster::verify_chain(&self.store.org, &self.store.store, &files, &anchor)
        };
        // The number of people needed comes from the verifier: an empty signatures file reports it.
        let need = match try_with(&admin::epoch_sigs_file(l.n, &[])) {
            Err(RosterError::QuorumNotMet { need, .. }) => need,
            Err(e) => return Err(re(e)),
            Ok(_) => 0,
        };
        let parsed = roster::EpochSigs::parse(&sigs_file).map_err(re)?;
        let approvers = prev.active_admins();
        let people: Vec<String> = parsed.signatures.iter().filter_map(|(k, _)| approvers.get(k.as_str()).map(|p| p.to_string())).collect::<BTreeSet<_>>().into_iter().collect();
        match try_with(&sigs_file) {
            Ok(r) => Ok((sigs_file, QuorumView { met: true, have: people.len(), need, people }, Some(r))),
            Err(RosterError::QuorumNotMet { have, need, .. }) => Ok((sigs_file, QuorumView { met: false, have, need, people }, None)),
            Err(e) => Err(re(e)),
        }
    }

    fn report(&self, l: &Loaded, q: QuorumView) -> ProposalReport {
        let d = admin::diff(Some(&l.roster.head().epoch), &l.epoch);
        ProposalReport { n: l.n, branch: admin::proposal_branch(l.n), epoch_hash: l.hash.clone(), bytes: l.bytes.clone(), changes: d.changes, flags: d.flags, reminders: d.reminders, quorum: q }
    }

    // -----------------------------------------------------------------------------------------------------
    // propose / inspect / sign

    /// `admin add|remove|rotate`: write the draft epoch to `roster-proposal/<n>`. Nothing is signed. An existing
    /// branch of that name is refused (we never force; abandoning a proposal is not supported yet).
    pub fn propose(&self, op: Operation) -> Result<Proposal, AdminError> {
        let head = self.verified_head()?;
        let (roster, chain, _snap, pins) = self.head_roster(&head)?;
        let built = admin::build_next(&roster.head().epoch, roster.head_hash(), &op, &self.now_ts(), &self.policy)?;
        let n = built.epoch.epoch;
        let commit = self.commit_on(Some(&head), &[(layout::roster_epoch(n), built.bytes.clone())], &format!("xenia: roster proposal {n}"))?;
        match self.push(&commit, &admin::proposal_branch(n))? {
            Pushed::Yes => {}
            Pushed::NotFastForward => return Err(AdminError::ProposalExists(n)),
        }
        let l = Loaded { head, roster, chain, pins, n, bytes: built.bytes, epoch: built.epoch, hash: built.hash, sigs: Vec::new() };
        let (_, q, _) = self.quorum_of(&l)?;
        Ok(Proposal { report: self.report(&l, q), commit })
    }

    /// Read-only: what proposal `n` is, who signed it, and whether a quorum has.
    pub fn inspect(&self, n: u64) -> Result<ProposalReport, AdminError> {
        let head = self.verified_head()?;
        let l = self.load_proposal(&head, n)?;
        let (_, q, _) = self.quorum_of(&l)?;
        Ok(self.report(&l, q))
    }

    /// The verified head epoch and the epoch proposal `n` would make, parsed and judged by the verifier's rules. Typed
    /// input for the console's classification (`xenia_admin_console::classify`).
    pub fn proposal_epochs(&self, n: u64) -> Result<(Epoch, Epoch), AdminError> {
        let head = self.verified_head()?;
        let l = self.load_proposal(&head, n)?;
        Ok((l.roster.head().epoch.clone(), l.epoch))
    }

    /// The verified head epoch and the epoch `op` would make. Nothing is pushed.
    pub fn operation_epochs(&self, op: &Operation) -> Result<(Epoch, Epoch), AdminError> {
        let head = self.verified_head()?;
        let (roster, _chain, _snap, _pins) = self.head_roster(&head)?;
        let built = admin::build_next(&roster.head().epoch, roster.head_hash(), op, &self.now_ts(), &self.policy)?;
        Ok((roster.head().epoch.clone(), built.epoch))
    }

    /// `admin sign <n>`: validate the proposal, then preview the diff and the exact bytes.
    pub fn prepare_sign(&self, signer: &Identity, n: u64) -> Result<Prepared<EpochSigPayload>, AdminError> {
        let head = self.verified_head()?;
        let l = self.load_proposal(&head, n)?;
        let me = signer.did();
        if !l.roster.head().epoch.active_admins().contains_key(me.as_str()) {
            return Err(AdminError::NotAnApprover);
        }
        let (_, q, _) = self.quorum_of(&l)?;
        let rep = self.report(&l, q);
        let domain = admin::epoch_domain(&self.store.org, &self.store.store, n, &l.bytes)?;
        let held = Held {
            action: Action::SignEpoch,
            org: self.store.org.clone(),
            store: self.store.store.clone(),
            epoch: n,
            signer: me,
            files: vec![PreviewFile { path: layout::roster_epoch(n), bytes: l.bytes.clone() }],
            domains: vec![domain],
            store_head: Some(l.head.clone()),
            expires_at: None,
            flags: rep.flags.clone(),
        };
        let parts = PreviewParts { changes: rep.changes, reminders: rep.reminders, quorum: Some(rep.quorum), ..Default::default() };
        Ok(make_prepared(held, parts, EpochSigPayload { n, bytes: l.bytes }))
    }

    /// Push the signature to the proposal branch, as a fast-forward of whatever tip it has now, provided the epoch
    /// file there is still exactly the one that was signed.
    pub fn deliver_signature(&self, signed: &Signed<EpochSigPayload>) -> Result<SigDelivered, AdminError> {
        let p = &signed.payload;
        let sig = signed.proposal_sig();
        let path = admin::proposal_sig_path(p.n, &sig.kid)?;
        let body = sig.to_json();
        for _ in 0..=MAX_REBASES {
            let head = self.verified_head()?;
            let (roster, ..) = self.head_roster(&head)?;
            if roster.head_epoch() >= p.n {
                return Err(AdminError::StaleProposal { head_epoch: roster.head_epoch() });
            }
            let (staged, tip) = self.fetch_proposal(p.n)?.ok_or(AdminError::ProposalNotFound(p.n))?;
            let (bytes, sigs) = self.read_proposal(&tip, p.n)?;
            drop(staged);
            if bytes != p.bytes {
                return Err(AdminError::ProposalChanged);
            }
            if sigs.contains(&sig) {
                return Ok(SigDelivered { commit: tip, already_there: true });
            }
            let commit = self.commit_on(Some(&tip), &[(path.clone(), body.clone())], &format!("xenia: signature for roster proposal {}", p.n))?;
            match self.push(&commit, &admin::proposal_branch(p.n))? {
                Pushed::Yes => return Ok(SigDelivered { commit, already_there: false }),
                Pushed::NotFastForward => continue,
            }
        }
        Err(AdminError::TooManyRaces)
    }

    /// The whole of `admin sign`: prepare, one decision, sign, deliver.
    pub fn sign(&self, signer: &Identity, n: u64, approver: &mut dyn AdminApprover) -> Result<SigDelivered, AdminError> {
        let prepared = self.prepare_sign(signer, n)?;
        let decision = approver.decide(prepared.preview());
        let signed = match decision {
            AdminDecision::Approve(a) => prepared.sign(&a, signer)?,
            AdminDecision::Deny => return Err(AdminError::Denied),
        };
        self.deliver_signature(&signed)
    }

    // -----------------------------------------------------------------------------------------------------
    // publish

    /// A statement issued now must be newer than the newest one this install accepted, or every install that has
    /// seen the older one ignores it as a replay.
    fn check_issue_time(&self, statement: &[u8]) -> Result<(), AdminError> {
        let st = freshness::Statement::parse(statement).map_err(|e| AdminError::Io(e.to_string()))?;
        let sticky = self.store.load_sticky().map_err(xe)?;
        match &sticky.freshness_mark {
            Some(m) if st.issued < m.issued => Err(AdminError::ClockBehind),
            Some(m) if st.issued == m.issued && hash::of(statement) != m.statement_hash => Err(AdminError::ClockBehind),
            _ => Ok(()),
        }
    }

    /// `admin publish <n>`: refuse unless the X3 verifier says a quorum signed exactly these bytes; then preview the
    /// change, the signatures and the new statement. The signer must be an admin of the **new** epoch.
    pub fn prepare_publish(&self, signer: &Identity, n: u64, validity_secs: i64) -> Result<Prepared<PublishPayload>, AdminError> {
        let head = self.verified_head()?;
        let l = self.load_proposal(&head, n)?;
        let (sigs_file, q, new_roster) = self.quorum_of(&l)?;
        let Some(new_roster) = new_roster else {
            return Err(AdminError::QuorumNotMet { have: q.have, need: q.need });
        };
        let me = signer.did();
        if !new_roster.head().epoch.active_admins().contains_key(me.as_str()) {
            return Err(AdminError::NotAnApprover);
        }
        let statement = admin::build_statement(&self.store.org, &self.store.store, n, &l.hash, &head, self.cfg.now, validity_secs, &self.cfg.policy)?;
        self.check_issue_time(&statement)?;
        let stmt_path = layout::FRESHNESS.to_string();
        let (epoch_path, sigs_path) = admin::epoch_paths(n);
        let rep = self.report(&l, q);
        let st = freshness::Statement::parse(&statement).map_err(|e| AdminError::Io(e.to_string()))?;
        let held = Held {
            action: Action::Publish,
            org: self.store.org.clone(),
            store: self.store.store.clone(),
            epoch: n,
            signer: me,
            files: vec![
                PreviewFile { path: epoch_path.clone(), bytes: l.bytes.clone() },
                PreviewFile { path: sigs_path.clone(), bytes: sigs_file.clone() },
                PreviewFile { path: stmt_path.clone(), bytes: statement.clone() },
            ],
            domains: vec![admin::statement_domain(&self.store.org, &self.store.store, &statement)?],
            store_head: Some(head.clone()),
            expires_at: Some(st.expires_at),
            flags: rep.flags.clone(),
        };
        let parts = PreviewParts { changes: rep.changes, reminders: rep.reminders, quorum: Some(rep.quorum), ..Default::default() };
        let payload = PublishPayload { n, epoch_hash: l.hash, files: vec![(epoch_path, l.bytes), (sigs_path, sigs_file)], statement };
        Ok(make_prepared(held, parts, payload))
    }

    pub fn publish(&self, signer: &Identity, n: u64, validity_secs: i64, approver: &mut dyn AdminApprover) -> Result<Delivered, AdminError> {
        let prepared = self.prepare_publish(signer, n, validity_secs)?;
        let decision = approver.decide(prepared.preview());
        let signed = match decision {
            AdminDecision::Approve(a) => prepared.sign(&a, signer)?,
            AdminDecision::Deny => return Err(AdminError::Denied),
        };
        self.deliver_publish(&signed)
    }

    pub fn deliver_publish(&self, signed: &Signed<PublishPayload>) -> Result<Delivered, AdminError> {
        let p = &signed.payload;
        let mut files = p.files.clone();
        files.push((layout::FRESHNESS.to_string(), p.statement.clone()));
        files.push((layout::FRESHNESS_SIGS.to_string(), admin::statement_sigs_file(&signed.signer, &signed.sigs[0])));
        self.deliver_main(&files, Goal::Publish { n: p.n, epoch_hash: &p.epoch_hash }, &format!("xenia: roster epoch {} and freshness statement", p.n))
    }

    // -----------------------------------------------------------------------------------------------------
    // refresh

    /// `admin refresh`: run the history walk **first**, from every base we can name, and refuse on any failure.
    ///
    /// 1. A verified sync (which itself walks from the install's accepted head and rejects tampering).
    /// 2. The signer must be an `active` admin of the head epoch.
    /// 3. An explicit walk ([`walk::walk`]) from the install's accepted head *before* step 1 and from the previous
    ///    authentic statement's `store_head`, to the verified head. This does not trust the install's own accepted
    ///    state: a head that state says is accepted but the walk finds tampered is refused.
    /// 4. Only then is a statement built, naming the chain head epoch and hash and `store_head`, and previewed.
    pub fn prepare_refresh(&self, signer: &Identity, validity_secs: i64) -> Result<Prepared<RefreshPayload>, AdminError> {
        let accepted_before = self.store.accepted_head().map_err(xe)?;
        let head = self.verified_head()?;
        let (roster, _chain, snap, _pins) = self.head_roster(&head)?;
        let me = signer.did();
        if !roster.head().epoch.active_admins().contains_key(me.as_str()) {
            return Err(AdminError::NotAnApprover);
        }
        let sticky = self.store.load_sticky().map_err(xe)?;
        let prior = freshness::assess_after(&self.store.org, &self.store.store, statement_files(&snap), &roster, self.cfg.now, &self.cfg.policy, sticky.freshness_mark.as_ref());
        let mut bases: BTreeSet<String> = BTreeSet::new();
        if let Some(a) = accepted_before {
            bases.insert(a);
        }
        if let Some(a) = &prior.authentic {
            bases.insert(a.statement.store_head.clone());
        }
        bases.remove(&head);
        let src = GitSource::new(self.store);
        let ctx = Ctx { pinned: self.pinned(), roster: &roster };
        let mut walked = Vec::new();
        for base in &bases {
            if !self.store.is_ancestor(base, &head).map_err(xe)? {
                return Err(AdminError::HistoryFailed(vec![format!("{}: base-not-an-ancestor", &base[..12])]));
            }
            let findings = walk::walk(&src, &ctx, base, &head).map_err(xe)?;
            if !findings.is_empty() {
                return Err(AdminError::HistoryFailed(findings.iter().take(8).map(|f| format!("{}: {}", f.path, f.reason)).collect()));
            }
            walked.push((base.clone(), src.commits(base, &head).map_err(xe)?.len()));
        }
        let statement = admin::build_statement(&self.store.org, &self.store.store, roster.head_epoch(), roster.head_hash(), &head, self.cfg.now, validity_secs, &self.cfg.policy)?;
        self.check_issue_time(&statement)?;
        let st = freshness::Statement::parse(&statement).map_err(|e| AdminError::Io(e.to_string()))?;
        let held = Held {
            action: Action::Refresh,
            org: self.store.org.clone(),
            store: self.store.store.clone(),
            epoch: roster.head_epoch(),
            signer: me,
            files: vec![PreviewFile { path: layout::FRESHNESS.to_string(), bytes: statement.clone() }],
            domains: vec![admin::statement_domain(&self.store.org, &self.store.store, &statement)?],
            store_head: Some(head.clone()),
            expires_at: Some(st.expires_at),
            flags: Vec::new(),
        };
        let parts = PreviewParts { walked, ..Default::default() };
        Ok(make_prepared(held, parts, RefreshPayload { epoch: roster.head_epoch(), epoch_hash: roster.head_hash().to_string(), statement }))
    }

    pub fn refresh(&self, signer: &Identity, validity_secs: i64, approver: &mut dyn AdminApprover) -> Result<Delivered, AdminError> {
        let prepared = self.prepare_refresh(signer, validity_secs)?;
        let decision = approver.decide(prepared.preview());
        let signed = match decision {
            AdminDecision::Approve(a) => prepared.sign(&a, signer)?,
            AdminDecision::Deny => return Err(AdminError::Denied),
        };
        self.deliver_refresh(&signed)
    }

    pub fn deliver_refresh(&self, signed: &Signed<RefreshPayload>) -> Result<Delivered, AdminError> {
        let p = &signed.payload;
        let files = vec![
            (layout::FRESHNESS.to_string(), p.statement.clone()),
            (layout::FRESHNESS_SIGS.to_string(), admin::statement_sigs_file(&signed.signer, &signed.sigs[0])),
        ];
        self.deliver_main(&files, Goal::Refresh { epoch: p.epoch, epoch_hash: &p.epoch_hash }, "xenia: freshness statement")
    }

    // -----------------------------------------------------------------------------------------------------
    // Delivery to the main branch

    fn deliver_main(&self, files: &[(String, Vec<u8>)], goal: Goal<'_>, msg: &str) -> Result<Delivered, AdminError> {
        for attempt in 1..=MAX_REBASES + 1 {
            // Always against a freshly verified head: a store that moved (or already holds our bytes) is seen here.
            let head = self.verified_head()?;
            let (roster, ..) = self.head_roster(&head)?;
            let snap = self.store.snapshot_of(&head).map_err(xe)?;
            if files.iter().all(|(p, b)| snap.files.get(p) == Some(b)) {
                return Ok(Delivered { commit: head.clone(), parent: head, paths: files.iter().map(|f| f.0.clone()).collect(), attempts: attempt, already_there: true });
            }
            match goal {
                Goal::Publish { n, .. } => {
                    if roster.head_epoch() >= n {
                        return Err(AdminError::StaleProposal { head_epoch: roster.head_epoch() });
                    }
                }
                Goal::Refresh { epoch, epoch_hash } => {
                    if roster.head_epoch() != epoch || roster.head_hash() != epoch_hash {
                        return Err(AdminError::StaleProposal { head_epoch: roster.head_epoch() });
                    }
                }
            }
            let candidate = self.commit_on(Some(&head), files, msg)?;
            self.verify_candidate(&head, &snap.files, &candidate, files, &goal)?;
            match self.push(&candidate, &self.store.branch)? {
                Pushed::Yes => return Ok(Delivered { commit: candidate, parent: head, paths: files.iter().map(|f| f.0.clone()).collect(), attempts: attempt, already_there: false }),
                Pushed::NotFastForward => continue,
            }
        }
        Err(AdminError::TooManyRaces)
    }

    /// Re-verify the head we are about to push with the verifier's own code: only our files changed, the roster
    /// chain verifies against the pins and ends where we meant, the statement is authentic for it and names an
    /// ancestor, and the history walk from the verified head finds nothing.
    fn verify_candidate(&self, head: &str, head_files: &std::collections::BTreeMap<String, Vec<u8>>, candidate: &str, files: &[(String, Vec<u8>)], goal: &Goal<'_>) -> Result<(), AdminError> {
        let mut problems: Vec<String> = Vec::new();
        let snap = self.store.snapshot_of(candidate).map_err(xe)?;
        let ours: BTreeSet<&str> = files.iter().map(|f| f.0.as_str()).collect();
        for (path, bytes) in &snap.files {
            if !ours.contains(path.as_str()) && head_files.get(path) != Some(bytes) {
                problems.push("the commit changed a file that is not ours".into());
            }
        }
        if head_files.keys().any(|p| !snap.files.contains_key(p)) {
            problems.push("the commit deleted a file".into());
        }
        for (p, b) in files {
            if snap.files.get(p) != Some(b) {
                problems.push(format!("{p}: not the signed bytes"));
            }
        }
        if let Ok(Err(r)) = self.store.check_tip(candidate) {
            problems.push(format!("tree: {}", r.reason()));
        }
        let pins = self.load_pins()?;
        match pins.verify(&epoch_files(&snap)) {
            Err(e) => problems.push(format!("roster: {}", e.reason())),
            Ok(roster) => {
                let (want_epoch, want_hash) = match goal {
                    Goal::Publish { n, epoch_hash } => (*n, *epoch_hash),
                    Goal::Refresh { epoch, epoch_hash } => (*epoch, *epoch_hash),
                };
                if roster.head_epoch() != want_epoch || roster.head_hash() != want_hash {
                    problems.push("roster: the head is not the epoch we meant".into());
                }
                match statement_files(&snap) {
                    None => problems.push("statement: missing".into()),
                    Some((s, g)) => match freshness::authenticate(&self.store.org, &self.store.store, s, g, &roster) {
                        Err(e) => problems.push(format!("statement: {e}")),
                        Ok(a) => {
                            if !self.store.is_ancestor(&a.statement.store_head, candidate).unwrap_or(false) {
                                problems.push("statement: store_head is not an ancestor".into());
                            }
                        }
                    },
                }
                if problems.is_empty() {
                    let ctx = Ctx { pinned: self.pinned(), roster: &roster };
                    for f in walk::walk(&GitSource::new(self.store), &ctx, head, candidate).map_err(xe)?.iter().take(8) {
                        problems.push(format!("{}: {}", f.path, f.reason));
                    }
                }
            }
        }
        if problems.is_empty() { Ok(()) } else { Err(AdminError::VerifyFailed(problems)) }
    }

    // -----------------------------------------------------------------------------------------------------
    // Genesis

    /// Create the store: the roster (epoch 1, signed by **every** founding admin key), the advisory `org.toml` and
    /// `.gitattributes` in the first commit, and the first freshness statement in the second (it names the first
    /// commit as `store_head`). Pushed to an empty branch only. `founder_sigs` come from the founders
    /// ([`prepare_genesis_signature`]); `signer` (a founder admin) signs the statement.
    pub fn init_store(&self, draft: &GenesisDraft, founder_sigs: &[ProposalSig], signer: &Identity, validity_secs: i64, approver: &mut dyn AdminApprover) -> Result<GenesisOutcome, AdminError> {
        if draft.org != self.store.org || draft.store != self.store.store {
            return Err(Refusal::WrongScope.into());
        }
        let me = signer.did();
        if !draft.proposed.epoch.active_admins().contains_key(me.as_str()) {
            return Err(AdminError::NotAnApprover);
        }
        let sigs_file = admin::assemble_sigs(&draft.org, &draft.store, 1, &draft.proposed.bytes, &draft.proposed.epoch, founder_sigs);
        let epoch_file = EpochFile { epoch_json: draft.proposed.bytes.clone(), sigs_json: sigs_file.clone() };
        let anchor = Anchor::new([(1, draft.proposed.hash.clone())]).map_err(re)?;
        // X3 decides whether every founder signed.
        if let Err(e) = roster::verify_chain(&draft.org, &draft.store, std::slice::from_ref(&epoch_file), &anchor) {
            if matches!(e, RosterError::GenesisNotFullySigned { .. }) {
                let have: BTreeSet<String> = roster::EpochSigs::parse(&sigs_file).map(|s| s.signatures.into_iter().map(|(k, _)| k).collect()).unwrap_or_default();
                let missing = draft.proposed.epoch.active_admins().keys().filter(|d| !have.contains(**d)).map(|d| d.to_string()).collect();
                return Err(AdminError::MissingFounderSignatures(missing));
            }
            return Err(re(e));
        }
        match self.store.fetch().map_err(xe)? {
            Fetch::Unavailable(_) => {}
            _ => return Err(AdminError::StoreNotEmpty),
        }
        let org_toml = format!("org = \"{}\"\nstore = \"{}\"\n", draft.org, draft.store).into_bytes();
        let first_files = vec![
            (".gitattributes".to_string(), GITATTRIBUTES.as_bytes().to_vec()),
            ("org.toml".to_string(), org_toml),
            (layout::roster_epoch(1), epoch_file.epoch_json.clone()),
            (layout::roster_epoch_sigs(1), epoch_file.sigs_json.clone()),
        ];
        let first = self.commit_on(None, &first_files, "xenia: genesis roster")?;
        let statement = admin::build_statement(&draft.org, &draft.store, 1, &draft.proposed.hash, &first, self.cfg.now, validity_secs, &self.cfg.policy)?;
        let st = freshness::Statement::parse(&statement).map_err(|e| AdminError::Io(e.to_string()))?;
        let d = admin::diff(None, &draft.proposed.epoch);
        let held = Held {
            action: Action::Genesis,
            org: draft.org.clone(),
            store: draft.store.clone(),
            epoch: 1,
            signer: me,
            files: vec![PreviewFile { path: layout::roster_epoch(1), bytes: draft.proposed.bytes.clone() }, PreviewFile { path: layout::FRESHNESS.to_string(), bytes: statement.clone() }],
            domains: vec![admin::statement_domain(&draft.org, &draft.store, &statement)?],
            store_head: Some(first.clone()),
            expires_at: Some(st.expires_at),
            flags: d.flags,
        };
        let parts = PreviewParts { changes: d.changes, reminders: d.reminders, ..Default::default() };
        let prepared = make_prepared(held, parts, GenesisPayload { first_commit: first.clone(), statement, epoch_hash: draft.proposed.hash.clone() });
        let decision = approver.decide(prepared.preview());
        let signed = match decision {
            AdminDecision::Approve(a) => prepared.sign(&a, signer)?,
            AdminDecision::Deny => return Err(AdminError::Denied),
        };
        let p = &signed.payload;
        let second_files = vec![(layout::FRESHNESS.to_string(), p.statement.clone()), (layout::FRESHNESS_SIGS.to_string(), admin::statement_sigs_file(&signed.signer, &signed.sigs[0]))];
        let second = self.commit_on(Some(&p.first_commit), &second_files, "xenia: first freshness statement")?;
        self.verify_genesis(&p.first_commit, &second, &anchor)?;
        match self.push(&second, &self.store.branch)? {
            Pushed::Yes => {}
            Pushed::NotFastForward => return Err(AdminError::StoreNotEmpty),
        }
        let fp = did::fingerprint(&signed.signer).map_err(xe)?;
        Ok(GenesisOutcome { head: second, join: JoinInfo { url: self.store.url.url.clone(), admin_fingerprint: fp, head_epoch: 1, head_hash: p.epoch_hash.clone() } })
    }

    fn verify_genesis(&self, first: &str, second: &str, anchor: &Anchor) -> Result<(), AdminError> {
        let mut problems: Vec<String> = Vec::new();
        let snap = self.store.snapshot_of(second).map_err(xe)?;
        if let Ok(Err(r)) = self.store.check_tip(second) {
            problems.push(format!("tree: {}", r.reason()));
        }
        match roster::verify_chain(&self.store.org, &self.store.store, &epoch_files(&snap), anchor) {
            Err(e) => problems.push(format!("roster: {}", e.reason())),
            Ok(roster) => {
                match statement_files(&snap) {
                    None => problems.push("statement: missing".into()),
                    Some((s, g)) => match freshness::authenticate(&self.store.org, &self.store.store, s, g, &roster) {
                        Err(e) => problems.push(format!("statement: {e}")),
                        Ok(a) if a.statement.store_head != first => problems.push("statement: store_head is not the first commit".into()),
                        Ok(_) => {}
                    },
                }
                if problems.is_empty() {
                    let ctx = Ctx { pinned: self.pinned(), roster: &roster };
                    for f in walk::walk(&GitSource::new(self.store), &ctx, first, second).map_err(xe)?.iter().take(8) {
                        problems.push(format!("{}: {}", f.path, f.reason));
                    }
                }
            }
        }
        if problems.is_empty() { Ok(()) } else { Err(AdminError::VerifyFailed(problems)) }
    }
}

enum Goal<'a> {
    Publish { n: u64, epoch_hash: &'a str },
    Refresh { epoch: u64, epoch_hash: &'a str },
}

struct Loaded {
    head: String,
    roster: VerifiedRoster,
    chain: Vec<EpochFile>,
    pins: Pins,
    n: u64,
    bytes: Vec<u8>,
    epoch: Epoch,
    hash: String,
    sigs: Vec<ProposalSig>,
}

/// A joining member: fetch the chain, verify it forward from genesis against the head pair the operator was given,
/// and write the pins (spec 011 §3.3). A refused join writes nothing.
pub fn join_store(store: &Store, req: &JoinRequest) -> Result<Pins, AdminError> {
    match store.fetch_checked(None).map_err(xe)? {
        Fetch::Advanced { head, entries, .. } => {
            let snap = store.read_snapshot(&head, &entries).map_err(xe)?;
            let (pins, _roster) = pins::join(&store.pin_storage(), req, &epoch_files(&snap)).map_err(re)?;
            Ok(pins)
        }
        Fetch::Unavailable(m) => Err(AdminError::Unavailable(m)),
        other => Err(AdminError::StoreNotUsable(format!("{other:?}").chars().take(60).collect())),
    }
}

#[cfg(test)]
#[path = "xenia_admin_tests.rs"]
pub(crate) mod tests;
