//! The verified sync of a Xenia store (spec 011 §3.4, §3.5, §4, §8.4; plan milestone X4b): X4a's fetch and sticky
//! state, plus everything that decides whether the fetched head may be believed.
//!
//! One pass, in this order, and the first failure stops it with the previous accepted state intact:
//!
//! 1. the fetch (X4a): caps, tree rules, `.gitattributes`, fast-forward only;
//! 2. the **roster chain** at the fetched head, verified against the install's pins (X3). A chain that does not
//!    contain every pinned hash, or is shorter than the pins, is rejected;
//! 3. the **freshness statement** (spec 011 §4.1). A missing, ignored or expired statement is not a rejection:
//!    the sync is accepted and the answer says `Stale`, which means no endorsement counts (spec 011 §4.3). An
//!    authentic statement also fixes check 2: the fetched head must descend from its `store_head`;
//! 4. the **history walk** (check 3) from the accepted head, or, on a fresh install, from the statement's
//!    `store_head`, which the admin signed. A fresh install with no authentic statement has no trust base and
//!    accepts nothing;
//! 5. the **real classifier** over the tip: a file raises its stored `rev` only after the signature, the
//!    structure and the signer's standing pass (spec 011 §3.4);
//! 6. the **sticky rules** (X4a) on the classified tip.
//!
//! Anything found by 4 or 6 rejects the **whole** sync (`Tampering`): applying the clean paths while refusing
//! the others would let a host smuggle new records past a withheld deletion. The way out of a tampered or
//! rewritten store is [`Store::plan_rewrite`] and [`Store::accept_rewritten_head`] (spec 011 §8.4): an admin
//! **quorum** statement naming the replaced head, and the operator's explicit list of every signed file that was
//! deleted, reverted or taken over. Sticky entries and pinned epoch hashes are reconciled, never reset.
//!
//! Git is only ever run through `xenia_store::git_command` (via `Store::local`). Nothing here reads a clock:
//! the caller passes `now`.

use crate::xenia_store::{io, run, Divergence, Fetch, NonFastForward, Refused, RunFail, Snapshot, Store, REF_ACCEPTED, REF_FETCHED};
use delphi_xenia::freshness::{self, Assessment, FreshnessState, Policy};
use delphi_xenia::layout::{roster_epoch, roster_epoch_sigs, FRESHNESS, FRESHNESS_SIGS};
use delphi_xenia::pins::{self, Pins};
use delphi_xenia::record::Pinned;
use delphi_xenia::roster::{EpochFile, RosterError, VerifiedRoster, MAX_EPOCHS};
use delphi_xenia::sticky::{Observation, StickyState, Tracking};
use delphi_xenia::walk::{self, Blob, Change, CommitInfo, Ctx, Finding, Source};
use delphi_xenia::{hash, Error, Result};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;

/// What the caller supplies: the clock and the local freshness policy. Nothing in a store can change either.
#[derive(Debug, Clone, Copy)]
pub struct VerifyConfig {
    /// Unix seconds.
    pub now: i64,
    pub policy: Policy,
}

impl VerifyConfig {
    pub fn at(now: i64) -> VerifyConfig {
        VerifyConfig { now, policy: Policy::default() }
    }
}

pub(crate) fn rerr(e: RosterError) -> Error {
    Error::BadField(e.reason())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedSync {
    /// The host serves the head already accepted. `freshness` is re-evaluated against the clock anyway: a
    /// statement can expire while the store sits still.
    UpToDate { head: String, freshness: FreshnessState },
    Accepted {
        head: String,
        from: Option<String>,
        freshness: FreshnessState,
        snapshot: Snapshot,
        /// Tracked but not in force (for example the signer lost standing) or not tracked at all (failed to verify).
        quarantined: Vec<(String, &'static str)>,
        added: usize,
        superseded: usize,
        unchanged: usize,
    },
    /// The walk or the sticky rules found tampering. Nothing was applied.
    Tampering { head: String, findings: Vec<Finding> },
    /// A check other than tampering failed (roster, statement head, no trust base). Nothing was applied.
    Rejected { head: String, reason: &'static str },
    NonFastForward(NonFastForward),
    Refused(Refused),
    Unavailable(String),
}

// ---------------------------------------------------------------------------------------------------------
// Reading history through the hardened git runner

/// [`Source`] over the store's bare clone. Every call goes through `Store::local`, which is `git_command`.
pub struct GitSource<'a> {
    store: &'a Store,
}

impl<'a> GitSource<'a> {
    pub fn new(store: &'a Store) -> GitSource<'a> {
        GitSource { store }
    }

    /// Every commit reachable from `head`, parents before children (the whole history a fetch brought in, which the
    /// fetch caps already bound). The index uses it to learn the tags of earlier revisions.
    pub(crate) fn all_commits(&self, head: &str) -> Result<Vec<CommitInfo>> {
        check_commit(head)?;
        self.rev_list(head)
    }

    fn rev_list(&self, range: &str) -> Result<Vec<CommitInfo>> {
        let cap = (self.store.caps.max_new_commits as usize + 1) * (65 * 8 + 4);
        let out = self.sh(&["rev-list", "--reverse", "--topo-order", "--parents", range], None, cap)?;
        let mut v = Vec::new();
        for line in String::from_utf8_lossy(&out).lines() {
            let mut ids = line.split(' ');
            let id = ids.next().unwrap_or("").to_string();
            let parents: Vec<String> = ids.map(str::to_string).collect();
            if check_commit(&id).is_err() || parents.iter().any(|p| check_commit(p).is_err()) {
                return Err(Error::BadField("rev-list printed something unexpected"));
            }
            v.push(CommitInfo { id, parents });
            if v.len() as u64 > self.store.caps.max_new_commits {
                return Err(Error::Oversize("too many commits"));
            }
        }
        Ok(v)
    }

    pub(crate) fn sh(&self, args: &[&str], stdin: Option<Vec<u8>>, cap: usize) -> Result<Vec<u8>> {
        let mut c = self.store.local();
        c.args(args);
        let out = run(c, stdin, cap, Duration::from_secs(120), None).map_err(|f| match f {
            RunFail::TooMuchOutput => Error::Oversize("git output over the cap"),
            _ => Error::Io(format!("git {} failed", args.first().unwrap_or(&""))),
        })?;
        if out.code != 0 {
            return Err(Error::Io(format!("git {} failed: {}", args.first().unwrap_or(&""), crate::xenia_store::short(&out.stderr))));
        }
        Ok(out.stdout)
    }
}

fn check_commit(id: &str) -> Result<()> {
    if delphi_xenia::sticky::is_commit_id(id) { Ok(()) } else { Err(Error::BadField("not a commit id")) }
}

impl Source for GitSource<'_> {
    fn commits(&self, base: &str, head: &str) -> Result<Vec<CommitInfo>> {
        check_commit(base)?;
        check_commit(head)?;
        self.rev_list(&format!("{base}..{head}"))
    }

    fn changes(&self, parent: Option<&str>, commit: &str) -> Result<Vec<Change>> {
        check_commit(commit)?;
        let mut args = vec!["diff-tree", "-r", "-z", "--raw", "--no-renames", "--no-abbrev", "--no-commit-id"];
        match parent {
            Some(p) => {
                check_commit(p)?;
                args.push(p);
            }
            None => args.push("--root"),
        }
        args.push(commit);
        args.extend(["--", "records", "roster"]);
        let cap = (self.store.caps.max_tree_entries + 1) * (self.store.caps.max_path_bytes + 200) * 2;
        let out = self.sh(&args, None, cap)?;
        let mut toks = out.split(|b| *b == 0).filter(|t| !t.is_empty());
        let mut v = Vec::new();
        while let Some(meta) = toks.next() {
            let path = toks.next().ok_or(Error::BadField("diff-tree output is truncated"))?;
            let meta = std::str::from_utf8(meta).map_err(|_| Error::BadField("diff-tree metadata"))?;
            let f: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
            let [old_mode, new_mode, _, _, status] = f[..] else {
                return Err(Error::BadField("diff-tree line has the wrong shape"));
            };
            let (old_mode, new_mode) = (u32::from_str_radix(old_mode, 8).map_err(|_| Error::BadField("mode"))?, u32::from_str_radix(new_mode, 8).map_err(|_| Error::BadField("mode"))?);
            let Ok(path) = std::str::from_utf8(path) else { continue };
            // A path the layout does not know is never reported, so its bytes need no further care.
            if delphi_xenia::store::classify_path(path).is_none() {
                continue;
            }
            v.push(Change { path: path.to_string(), existed_before: old_mode != 0 && !status.starts_with('A'), exists_now: new_mode != 0 && !status.starts_with('D'), new_mode });
        }
        Ok(v)
    }

    fn read(&self, commit: &str, paths: &[String], max_bytes: usize) -> Result<Vec<Blob>> {
        check_commit(commit)?;
        if paths.iter().any(|p| p.contains(['\n', '\0']) || delphi_xenia::store::classify_path(p).is_none()) {
            return Err(Error::BadField("a path the layout does not know"));
        }
        let names: String = paths.iter().map(|p| format!("{commit}:{p}\n")).collect();
        let checked = self.sh(&["cat-file", "--batch-check"], Some(names.into_bytes()), paths.len() * 400 + 1024)?;
        let mut plan: Vec<std::result::Result<(String, u64), Blob>> = Vec::new();
        for line in String::from_utf8_lossy(&checked).lines() {
            let f: Vec<&str> = line.split(' ').collect();
            match f[..] {
                [_, "missing"] => plan.push(Err(Blob::Missing)),
                [sha, "blob", size] => match size.parse::<u64>() {
                    Ok(n) if n as usize <= max_bytes => plan.push(Ok((sha.to_string(), n))),
                    Ok(_) => plan.push(Err(Blob::TooBig)),
                    Err(_) => return Err(Error::BadField("cat-file size")),
                },
                // A tree or commit at a file path is not a file.
                [_, _, _] => plan.push(Err(Blob::Missing)),
                _ => return Err(Error::BadField("cat-file printed something unexpected")),
            }
        }
        if plan.len() != paths.len() {
            return Err(Error::BadField("cat-file answered a different number of objects"));
        }
        let want: Vec<&(String, u64)> = plan.iter().filter_map(|p| p.as_ref().ok()).collect();
        let mut bodies = Vec::new();
        if !want.is_empty() {
            let stdin: String = want.iter().map(|(s, _)| format!("{s}\n")).collect();
            let cap = want.iter().map(|(_, n)| *n as usize + 160).sum::<usize>() + 1024;
            let out = self.sh(&["cat-file", "--batch"], Some(stdin.into_bytes()), cap)?;
            let mut pos = 0usize;
            for (sha, size) in &want {
                let nl = out[pos..].iter().position(|b| *b == b'\n').ok_or(Error::Io("truncated cat-file output".into()))?;
                let header = String::from_utf8_lossy(&out[pos..pos + nl]).into_owned();
                pos += nl + 1;
                let f: Vec<&str> = header.split(' ').collect();
                let n = *size as usize;
                if f.len() != 3 || f[0] != sha.as_str() || f[1] != "blob" || f[2].parse::<usize>().ok() != Some(n) || out.len() < pos + n + 1 {
                    return Err(Error::Io("cat-file disagrees with itself".into()));
                }
                bodies.push(out[pos..pos + n].to_vec());
                pos += n + 1;
            }
        }
        let mut bodies = bodies.into_iter();
        Ok(plan.into_iter().map(|p| p.map(|_| Blob::Bytes(bodies.next().expect("one body per object"))).unwrap_or_else(|b| b)).collect())
    }

    fn epoch_in_tree(&self, commit: &str) -> Result<u64> {
        check_commit(commit)?;
        let out = self.sh(&["ls-tree", "--name-only", "-z", commit, "roster/epochs/"], None, 1 << 20)?;
        let mut best = 0;
        for name in out.split(|b| *b == 0) {
            if let Some(delphi_xenia::store::PathClass::EpochFile(n)) = std::str::from_utf8(name).ok().and_then(delphi_xenia::store::classify_path) {
                best = best.max(n);
            }
        }
        Ok(best)
    }
}

// ---------------------------------------------------------------------------------------------------------
// Inspection of one fetched head

struct Inspected {
    roster: VerifiedRoster,
    assessment: Assessment,
    /// Violations of the history walk, from the accepted head (or the statement's `store_head`).
    walk: Vec<Finding>,
    observed: Vec<Observation>,
    invalid: Vec<String>,
    quarantined: Vec<(String, &'static str)>,
}

pub(crate) fn epoch_files(snap: &Snapshot) -> Vec<EpochFile> {
    let mut v = Vec::new();
    for n in 1..=MAX_EPOCHS as u64 {
        let (Some(j), Some(s)) = (snap.files.get(&roster_epoch(n)), snap.files.get(&roster_epoch_sigs(n))) else { break };
        v.push(EpochFile { epoch_json: j.clone(), sigs_json: s.clone() });
    }
    v
}

/// Record the newest genuine statement's `issued_at`; a replay leaves the mark where it was.
fn advance_mark(next: &mut StickyState, assessment: &Assessment) {
    if let Some(a) = &assessment.authentic {
        next.freshness_mark = Some(a.advance(next.freshness_mark.as_ref()));
    }
}

pub(crate) fn statement_files(snap: &Snapshot) -> Option<(&[u8], &[u8])> {
    Some((snap.files.get(FRESHNESS)?.as_slice(), snap.files.get(FRESHNESS_SIGS)?.as_slice()))
}

impl Store {
    pub(crate) fn load_pins(&self) -> Result<Pins> {
        let p = pins::load(&self.pin_storage()).map_err(rerr)?.ok_or(Error::BadField("this store has not been joined (no pins)"))?;
        if p.org() != self.org || p.store() != self.store {
            return Err(Error::BadField("the pins belong to another store"));
        }
        Ok(p)
    }

    /// Is `a` an ancestor of `b`, or `b` itself? An object the clone does not have (git says 128) is "no": a
    /// statement may name any commit, and one we cannot find cannot be a base.
    pub(crate) fn is_ancestor_or_equal(&self, a: &str, b: &str) -> Result<bool> {
        if a == b {
            return Ok(true);
        }
        match self.exit_code(&["merge-base", "--is-ancestor", a, b])? {
            0 => Ok(true),
            1 | 128 => Ok(false),
            _ => Err(Error::Io("git merge-base failed".into())),
        }
    }

    pub(crate) fn pinned(&self) -> Pinned<'_> {
        Pinned { org: &self.org, store: &self.store }
    }

    /// Steps 2 to 5 for one fetched head. `Ok(Err(reason))` is a rejection; the walk's and the classifier's
    /// findings are returned for the caller to act on.
    fn inspect(&self, pins: &Pins, cfg: &VerifyConfig, head: &str, snap: &Snapshot, sticky: &StickyState, statement_base: bool) -> Result<std::result::Result<Inspected, &'static str>> {
        let (accepted, mark) = (sticky.accepted_head.as_deref(), sticky.freshness_mark.as_ref());
        let files = epoch_files(snap);
        if files.is_empty() {
            return Ok(Err("no-roster"));
        }
        let roster = match pins.verify(&files) {
            Ok(r) => r,
            Err(e) => return Ok(Err(e.reason())),
        };
        let assessment = freshness::assess_after(&self.org, &self.store, statement_files(snap), &roster, cfg.now, &cfg.policy, mark);
        let auth_head = assessment.authentic.as_ref().map(|a| a.statement.store_head.clone());
        // Check 2: the fetched head descends from the head the admin verified.
        if let Some(sh) = &auth_head {
            if !self.is_ancestor_or_equal(sh, head)? {
                return Ok(Err("store-head-not-ancestor"));
            }
        }
        let base = match (statement_base, accepted, &auth_head) {
            (false, Some(a), _) => a.to_string(),
            (_, _, Some(sh)) => sh.clone(),
            (true, _, None) => return Ok(Err("no-valid-statement")),
            (false, None, None) => return Ok(Err("no-trust-base")),
        };
        let ctx = Ctx { pinned: self.pinned(), roster: &roster };
        let findings = if base == head { Vec::new() } else { walk::walk(&GitSource::new(self), &ctx, &base, head)? };
        let lookup = |p: &str| snap.files.get(p).map(Vec::as_slice);
        let (mut observed, mut invalid, mut quarantined) = (Vec::new(), Vec::new(), Vec::new());
        for (path, bytes) in &snap.files {
            match walk::classify(&ctx, path, bytes, &lookup) {
                Ok((tracking, note)) => {
                    observed.push(Observation { path: path.clone(), tracking, hash: hash::of(bytes) });
                    if let Some(n) = note {
                        quarantined.push((path.clone(), n));
                    }
                }
                Err(reason) => {
                    invalid.push(path.clone());
                    quarantined.push((path.clone(), reason));
                }
            }
        }
        Ok(Ok(Inspected { roster, assessment, walk: findings, observed, invalid, quarantined }))
    }

    /// Only the roster and the freshness files of `head`: all the freshness judgement needs, so judging an
    /// unchanged store against the clock does not read a single record.
    fn roster_snapshot(&self, head: &str) -> Result<Snapshot> {
        let src = GitSource::new(self);
        let n = src.epoch_in_tree(head)?.min(MAX_EPOCHS as u64);
        let mut paths = vec![FRESHNESS.to_string(), FRESHNESS_SIGS.to_string()];
        for i in 1..=n {
            paths.push(roster_epoch(i));
            paths.push(roster_epoch_sigs(i));
        }
        let blobs = src.read(head, &paths, delphi_xenia::roster::MAX_EPOCH_BYTES)?;
        let files = paths.into_iter().zip(blobs).filter_map(|(p, b)| if let Blob::Bytes(b) = b { Some((p, b)) } else { None }).collect();
        Ok(Snapshot { head: head.to_string(), files })
    }

    /// The freshness of the accepted head, by `cfg.now`. This is what `status` shows, and it changes with the
    /// clock alone: no fetch, no write.
    pub fn freshness_status(&self, cfg: &VerifyConfig) -> Result<FreshnessState> {
        let pins = self.load_pins()?;
        let sticky = self.load_sticky()?;
        let Some(head) = sticky.accepted_head.clone() else {
            return Ok(FreshnessState::Stale { reason: "nothing-accepted" });
        };
        let snap = self.roster_snapshot(&head)?;
        let files = epoch_files(&snap);
        let roster = match (!files.is_empty()).then(|| pins.verify(&files)) {
            Some(Ok(r)) => r,
            _ => return Ok(FreshnessState::Stale { reason: "roster-invalid" }),
        };
        Ok(freshness::assess_after(&self.org, &self.store, statement_files(&snap), &roster, cfg.now, &cfg.policy, sticky.freshness_mark.as_ref()).state)
    }

    /// The roster chain at the accepted head, verified against the pins. `Ok(None)` when nothing has been accepted.
    pub fn accepted_roster(&self) -> Result<Option<VerifiedRoster>> {
        let pins = self.load_pins()?;
        let Some(head) = self.load_sticky()?.accepted_head else { return Ok(None) };
        let files = epoch_files(&self.roster_snapshot(&head)?);
        if files.is_empty() {
            return Err(Error::BadField("the accepted head has no roster"));
        }
        pins.verify(&files).map(Some).map_err(rerr)
    }

    /// `delphi xenia join` (spec 011 §3.3): fetch the store, verify the roster chain it serves against what the
    /// operator typed (the admin's fingerprint and the epoch/hash pair), and write the pins. Refuses a store that
    /// is already joined. The fetched objects stay in the clone, but nothing is accepted: the first verified sync
    /// does that.
    pub fn join(&self, req: &pins::JoinRequest) -> Result<Pins> {
        if pins::load(&self.pin_storage()).map_err(rerr)?.is_some() {
            return Err(rerr(RosterError::AlreadyPinned));
        }
        let head = match self.fetch()? {
            Fetch::Advanced { head, .. } | Fetch::UpToDate { head } => head,
            Fetch::NonFastForward(_) => return Err(Error::BadField("the store's history is not a fast-forward of what was fetched")),
            Fetch::Refused(r) => return Err(Error::BadField(r.reason())),
            Fetch::Unavailable(m) => return Err(Error::Io(format!("cannot reach the store: {m}"))),
        };
        let files = epoch_files(&self.roster_snapshot(&head)?);
        let joined = pins::join(&self.pin_storage(), req, &files).map(|(p, _)| p).map_err(rerr);
        self.discard_fetched(false);
        joined
    }

    /// Fetch and verify. See the module docs for the order of checks and what each failure leaves behind.
    pub fn sync_verified(&self, cfg: &VerifyConfig) -> Result<VerifiedSync> {
        let lock = self.paths.root.join(".lock");
        crate::lock::with_lock(&lock, || self.sync_verified_locked(cfg)).map_err(io)?
    }

    fn sync_verified_locked(&self, cfg: &VerifyConfig) -> Result<VerifiedSync> {
        let pins = self.load_pins()?;
        let sticky = self.load_sticky()?;
        let (head, from, entries) = match self.fetch_checked(sticky.accepted_head.as_deref())? {
            Fetch::UpToDate { head } => {
                let freshness = self.freshness_status(cfg)?;
                return Ok(VerifiedSync::UpToDate { head, freshness });
            }
            Fetch::NonFastForward(n) => return Ok(VerifiedSync::NonFastForward(n)),
            Fetch::Refused(r) => return Ok(VerifiedSync::Refused(r)),
            Fetch::Unavailable(m) => return Ok(VerifiedSync::Unavailable(m)),
            Fetch::Advanced { head, from, entries } => (head, from, entries),
        };
        let snapshot = self.read_snapshot(&head, &entries)?;
        let ins = match self.inspect(&pins, cfg, &head, &snapshot, &sticky, false)? {
            Ok(i) => i,
            Err(reason) => return Ok(VerifiedSync::Rejected { head, reason }),
        };
        let mut findings = ins.walk.clone();
        let ev = sticky.evaluate(&ins.observed, &ins.invalid);
        findings.extend(ev.refusals.iter().map(|r| Finding { path: r.path().to_string(), commit: head.clone(), reason: r.reason() }));
        // The walk and the sticky rules often see the same tampering; report each (path, reason) once.
        let mut seen = BTreeSet::new();
        findings.retain(|f| seen.insert((f.path.clone(), f.reason)));
        if !findings.is_empty() {
            return Ok(VerifiedSync::Tampering { head, findings });
        }
        let mut next = sticky;
        next.apply(&ev)?;
        next.accepted_head = Some(head.clone());
        advance_mark(&mut next, &ins.assessment);
        self.commit_accepted(&pins, &ins.roster, &next, &head)?;
        Ok(VerifiedSync::Accepted {
            freshness: ins.assessment.state,
            head,
            from,
            snapshot,
            quarantined: ins.quarantined,
            added: ev.added.len(),
            superseded: ev.superseded.len(),
            unchanged: ev.unchanged,
        })
    }

    /// Persist an accepted head. The pins first (they only ever grow, so a crash after this leaves a valid
    /// state), then the anchor ref, then the sticky file that names the head.
    fn commit_accepted(&self, pins: &Pins, roster: &VerifiedRoster, next: &StickyState, head: &str) -> Result<()> {
        use delphi_xenia::pins::PinStorage;
        let extended = pins.extended(roster).map_err(rerr)?;
        if extended != *pins {
            self.pin_storage().write(&extended.to_toml()).map_err(rerr)?;
        }
        self.git_ok(&["update-ref", REF_ACCEPTED, head], 4096)?;
        self.save_sticky(next)
    }

    // -----------------------------------------------------------------------------------------------------
    // Rewrite recovery (spec 011 §4.3, §8.4)

    /// What accepting the fetched head would do, for the operator to read. Changes nothing but the staging
    /// ref and the objects git already fetched.
    pub fn plan_rewrite(&self, cfg: &VerifyConfig) -> Result<Recovery> {
        let lock = self.paths.root.join(".lock");
        crate::lock::with_lock(&lock, || self.plan_locked(cfg).map(|p| match p {
            Planned::Plan(b) => Recovery::Plan(b.plan),
            Planned::Refused(r, f) => Recovery::Refused { reason: r, findings: f },
        }))
        .map_err(io)?
    }

    /// Accept the rewritten head: only with an admin-quorum statement that names the replaced head, and only if
    /// `approval` names exactly the paths [`Store::plan_rewrite`] lists for this very `fetched` commit.
    pub fn accept_rewritten_head(&self, cfg: &VerifyConfig, approval: &RewriteApproval) -> Result<RewriteOutcome> {
        let lock = self.paths.root.join(".lock");
        crate::lock::with_lock(&lock, || self.accept_locked(cfg, approval)).map_err(io)?
    }

    fn plan_locked(&self, cfg: &VerifyConfig) -> Result<Planned> {
        let pins = self.load_pins()?;
        let sticky = self.load_sticky()?;
        let Some(accepted) = sticky.accepted_head.clone() else {
            return Ok(Planned::refuse("nothing-accepted"));
        };
        let (head, entries, kind) = match self.fetch_checked(Some(&accepted))? {
            Fetch::UpToDate { .. } => return Ok(Planned::refuse("nothing-to-recover")),
            Fetch::Unavailable(_) => return Ok(Planned::refuse("host-unavailable")),
            Fetch::Refused(_) => return Ok(Planned::refuse("fetch-refused")),
            Fetch::Advanced { head, entries, .. } => (head, entries, None),
            Fetch::NonFastForward(n) => match self.check_tip(&n.fetched)? {
                Ok(entries) => (n.fetched, entries, Some(n.kind)),
                Err(_) => return Ok(Planned::refuse("tip-refused")),
            },
        };
        let snapshot = self.read_snapshot(&head, &entries)?;
        let ins = match self.inspect(&pins, cfg, &head, &snapshot, &sticky, true)? {
            Ok(i) => i,
            Err(reason) => return Ok(Planned::refuse(reason)),
        };
        // The statement: genuine, naming a rewrite, signed by a quorum, current, and about *this* install's head.
        let Some(auth) = &ins.assessment.authentic else {
            return Ok(Planned::refuse("no-valid-statement"));
        };
        let Some(replaced) = &auth.statement.rewrite_of else {
            return Ok(Planned::refuse("statement-names-no-rewrite"));
        };
        if !ins.assessment.state.endorsements_count() {
            return Ok(Planned::refuse("statement-not-current"));
        }
        if !self.is_ancestor_or_equal(&accepted, replaced)? {
            return Ok(Planned::refuse("rewrite-of-is-not-our-head"));
        }
        if !ins.walk.is_empty() {
            return Ok(Planned::Refused("walk-violations", ins.walk));
        }
        let ev = sticky.evaluate(&ins.observed, &ins.invalid);
        let mut changes: Vec<PlannedChange> = ev
            .refusals
            .iter()
            .map(|r| PlannedChange {
                path: r.path().to_string(),
                reason: r.reason(),
                was: sticky.paths.get(r.path()).map(|e| e.hash.clone()),
                now: ins.observed.iter().find(|o| o.path == r.path()).map(|o| o.hash.clone()),
            })
            .collect();
        // A rewrite that changes who created an `id` lists that path too (spec 011 §8.4), even at a higher rev.
        let ctx = Ctx { pinned: self.pinned(), roster: &ins.roster };
        let src = GitSource::new(self);
        for (path, e) in &sticky.paths {
            if !matches!(e.tracking, Tracking::Rev(_)) || !path.ends_with(".md") || changes.iter().any(|c| &c.path == path) {
                continue;
            }
            let Some(new_md) = snapshot.files.get(path) else { continue };
            let sidecar_path = path.trim_end_matches(".md").to_string() + ".sig.json";
            let (Some(new_car), Ok(old)) = (snapshot.files.get(&sidecar_path), src.read(&accepted, &[path.clone(), sidecar_path.clone()], 16 * 1024)) else { continue };
            let (Blob::Bytes(old_md), Blob::Bytes(old_car)) = (&old[0], &old[1]) else { continue };
            if let (Ok(ov), Ok(nv)) = (walk::read_version(&ctx, path, old_md, old_car), walk::read_version(&ctx, path, new_md, new_car)) {
                if ov.signer != nv.signer && !ins.roster.successor_chain(&ov.signer).contains(&nv.signer) {
                    changes.push(PlannedChange { path: path.clone(), reason: "creator-changed", was: Some(e.hash.clone()), now: Some(hash::of(new_md)) });
                }
            }
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        changes.dedup_by(|a, b| a.path == b.path);
        let plan = RewritePlan {
            accepted,
            fetched: head.clone(),
            kind,
            replaced: replaced.clone(),
            signing_people: auth.signing_people,
            statement_expires_at: auth.statement.expires_at.clone(),
            changes,
        };
        Ok(Planned::Plan(Box::new(PlanBundle { plan, ins, sticky, snapshot, pins })))
    }

    fn accept_locked(&self, cfg: &VerifyConfig, approval: &RewriteApproval) -> Result<RewriteOutcome> {
        let bundle = match self.plan_locked(cfg)? {
            Planned::Plan(b) => *b,
            Planned::Refused(r, f) => return Ok(RewriteOutcome::Refused { reason: r, findings: f }),
        };
        let PlanBundle { plan, ins, sticky, snapshot, pins } = bundle;
        if plan.fetched != approval.fetched {
            return Ok(RewriteOutcome::Refused { reason: "host-head-moved-since-approval", findings: Vec::new() });
        }
        let listed: BTreeSet<String> = plan.changes.iter().map(|c| c.path.clone()).collect();
        let missing: Vec<String> = listed.difference(&approval.paths).cloned().collect();
        if !missing.is_empty() {
            return Ok(RewriteOutcome::NotApproved { missing });
        }
        if approval.paths.difference(&listed).next().is_some() {
            return Ok(RewriteOutcome::Refused { reason: "approval-names-unlisted-path", findings: Vec::new() });
        }
        // Reconcile: the approved paths start over from the new head; every other entry is kept and must hold.
        let mut next = sticky.without(&approval.paths);
        let ev = next.evaluate(&ins.observed, &ins.invalid);
        if !ev.is_clean() {
            let findings = ev.refusals.iter().map(|r| Finding { path: r.path().to_string(), commit: plan.fetched.clone(), reason: r.reason() }).collect();
            return Ok(RewriteOutcome::Refused { reason: "unapproved-sticky-violation", findings });
        }
        next.apply(&ev)?;
        next.accepted_head = Some(plan.fetched.clone());
        advance_mark(&mut next, &ins.assessment);
        self.append_rewrite_log(cfg, &plan)?;
        self.commit_accepted(&pins, &ins.roster, &next, &plan.fetched)?;
        let _ = self.git_ok(&["update-ref", "-d", REF_FETCHED], 4096);
        let _ = snapshot;
        Ok(RewriteOutcome::Accepted { head: plan.fetched.clone(), replaced: plan.changes })
    }

    /// One JSON line per reconciled path, appended and synced (`0600`), so the operator's decision is on record.
    fn append_rewrite_log(&self, cfg: &VerifyConfig, plan: &RewritePlan) -> Result<()> {
        let path = self.paths.state.join("rewrite-log.jsonl");
        if let Ok(m) = fs::symlink_metadata(&path) {
            if !m.file_type().is_file() {
                return Err(Error::Key("the rewrite log is not a regular file"));
            }
        }
        let mut f = fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&path).map_err(io)?;
        let mut text = String::new();
        for c in &plan.changes {
            let line = serde_json::json!({ "at": delphi_xenia::time::format_utc(cfg.now), "from": plan.accepted, "to": plan.fetched, "path": c.path, "why": c.reason, "was": c.was, "now": c.now });
            text.push_str(&line.to_string());
            text.push('\n');
        }
        f.write_all(text.as_bytes()).and_then(|_| f.sync_all()).map_err(io)
    }
}

// ---------------------------------------------------------------------------------------------------------
// Recovery types

/// One signed file the rewrite deletes, reverts, swaps, un-verifies or hands to another creator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedChange {
    pub path: String,
    /// A fixed slug: `deleted`, `rev-rollback`, `same-rev-different-hash`, `no-longer-verifies`,
    /// `tracking-changed`, `creator-changed`.
    pub reason: &'static str,
    /// The hash this install stored, and the new head's (`None` when the file is gone).
    pub was: Option<String>,
    pub now: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewritePlan {
    pub accepted: String,
    pub fetched: String,
    /// `None` when the head is a fast-forward that tampered (recovery from a poisoned history).
    pub kind: Option<Divergence>,
    /// The head the quorum statement says it replaces.
    pub replaced: String,
    pub signing_people: usize,
    pub statement_expires_at: String,
    pub changes: Vec<PlannedChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    Plan(RewritePlan),
    Refused { reason: &'static str, findings: Vec<Finding> },
}

/// What the operator confirmed: this fetched commit, and exactly this list of paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteApproval {
    pub fetched: String,
    pub paths: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteOutcome {
    Accepted { head: String, replaced: Vec<PlannedChange> },
    /// The approval left out paths the rewrite changes. Nothing was applied.
    NotApproved { missing: Vec<String> },
    Refused { reason: &'static str, findings: Vec<Finding> },
}

struct PlanBundle {
    plan: RewritePlan,
    ins: Inspected,
    sticky: StickyState,
    snapshot: Snapshot,
    pins: Pins,
}

enum Planned {
    Plan(Box<PlanBundle>),
    Refused(&'static str, Vec<Finding>),
}

impl Planned {
    fn refuse(reason: &'static str) -> Planned {
        Planned::Refused(reason, Vec::new())
    }
}

#[cfg(test)]
#[path = "xenia_sync_tests.rs"]
mod tests;
