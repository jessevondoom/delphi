//! The authoring side of the rewrite recovery (spec 011 §8.4; the verifying side is `Store::plan_rewrite` and
//! `Store::accept_rewritten_head`). A child module of `xenia_admin`, so it uses the same plumbing, the same
//! [`Prepared`]/[`Signed`] guarantees and the same approver, and has no other way to sign.
//!
//! ```text
//! propose : an admin; the host serves a head that is not a fast-forward of the one this install holds. The roster
//!           chain at that head must verify against this install's pins. Builds a freshness statement with
//!           `rewrite_of` = the head this install holds and `store_head` = the head the host serves, and pushes it to
//!           the branch rewrite-proposal/<id> on top of the served head. Nothing is signed.
//! sign    : an admin of the head epoch; fetches the branch (hostile input), re-judges the host and the statement,
//!           ONE approval at the card, then signs the freshness domain over those exact bytes and pushes the signature.
//! publish : an admin of the head epoch; the X3-era freshness verifier (`freshness::authenticate`) must say that a
//!           MAJORITY of the admin people signed these exact bytes. ONE approval, no new signature, then
//!           freshness.json + freshness.sigs.json go on top of the served head, fast-forward only.
//! ```
//!
//! An install then runs `sync --accept-rewritten-head`: it reads the same statement, lists every affected path and
//! needs its operator to type the commit and the path count. Nothing here accepts anything for an install.

use super::*;
use crate::xenia_sync::RewriteCandidate;
use delphi_xenia::freshness::Statement;
use std::collections::BTreeMap;

/// The id of a rewrite proposal: 12 characters of the rewritten head, a dash, and 8 of the statement's hash. A new
/// statement (another time, another validity) is a new id, so an expired proposal never blocks a fresh one.
pub fn rewrite_id(store_head: &str, statement: &[u8]) -> String {
    let h = hash::of(statement);
    format!("{}-{}", &store_head[..12.min(store_head.len())], h.trim_start_matches("blake3:").chars().take(8).collect::<String>())
}

fn valid_id(id: &str) -> bool {
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    matches!(id.split_once('-'), Some((a, b)) if hex(a, 12) && hex(b, 8))
}

pub fn rewrite_branch(id: &str) -> String {
    format!("rewrite-proposal/{id}")
}

fn statement_path(id: &str) -> String {
    format!("roster/rewrite/{id}/statement.json")
}

fn sig_dir(id: &str) -> String {
    format!("roster/rewrite/{id}/")
}

fn sig_path(id: &str, kid: &str) -> Option<String> {
    let fp = did::fingerprint(kid).ok()?;
    Some(format!("{}{}.json", sig_dir(id), fp.trim_start_matches("blake3:")))
}

/// `freshness.sigs.json` with several signatures.
fn sigs_file(sigs: &[ProposalSig]) -> Vec<u8> {
    let list: Vec<serde_json::Value> = sigs.iter().map(|s| serde_json::json!({ "kid": s.kid, "sig": s.sig })).collect();
    let mut t = serde_json::to_string_pretty(&serde_json::json!({ "signatures": list })).expect("serializes");
    t.push('\n');
    t.into_bytes()
}

/// What `rewrite show` and the proposal commands report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteReport {
    pub id: String,
    pub branch: String,
    pub bytes: Vec<u8>,
    pub quorum: QuorumView,
    pub view: RewriteView,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteProposal {
    pub report: RewriteReport,
    pub commit: String,
}

struct RewriteLoaded {
    cand: RewriteCandidate,
    id: String,
    bytes: Vec<u8>,
    st: Statement,
    sigs: Vec<ProposalSig>,
}

pub struct RewriteSignPayload {
    id: String,
    bytes: Vec<u8>,
}

pub struct RewritePublishPayload {
    bytes: Vec<u8>,
    sigs_file: Vec<u8>,
    epoch: u64,
    epoch_hash: String,
}

impl Admin<'_> {
    fn candidate(&self) -> Result<RewriteCandidate, AdminError> {
        match self.store.rewrite_candidate(&self.cfg) {
            Ok(Ok(c)) => Ok(c),
            Ok(Err(reason)) => Err(AdminError::RewriteNotApplicable(reason)),
            Err(e) => Err(AdminError::StoreNotUsable(e.to_string())),
        }
    }

    fn view_of(c: &RewriteCandidate) -> RewriteView {
        RewriteView { replaced: c.accepted.clone(), rewritten: c.fetched.clone(), affected: c.changes.iter().map(|x| (x.reason.to_string(), x.path.clone())).collect(), rule_findings: c.rule_findings }
    }

    fn require_head_admin(c: &RewriteCandidate, signer: &Identity) -> Result<(), AdminError> {
        if c.roster.head().epoch.active_admins().contains_key(signer.did().as_str()) { Ok(()) } else { Err(AdminError::NotAnApprover) }
    }

    /// Who signed these exact bytes, among the `active` admins of the head epoch at the served head, and whether the
    /// freshness verifier counts a quorum. The assembled file holds only the signatures worth publishing.
    fn rewrite_quorum(&self, c: &RewriteCandidate, bytes: &[u8], sigs: &[ProposalSig]) -> (Vec<u8>, QuorumView, bool) {
        let epoch = &c.roster.head().epoch;
        let admins = epoch.active_admins();
        let need_default = epoch.admin_people().len() / 2 + 1;
        let Ok(domain) = admin::statement_domain(&self.store.org, &self.store.store, bytes) else {
            return (sigs_file(&[]), QuorumView { met: false, have: 0, need: need_default, people: Vec::new() }, false);
        };
        let mut kept: BTreeMap<String, ProposalSig> = BTreeMap::new();
        for s in sigs {
            if admins.contains_key(s.kid.as_str()) && delphi_xenia::sig::verify(&s.kid, &domain, &s.sig).is_ok() {
                kept.entry(s.kid.clone()).or_insert_with(|| s.clone());
            }
        }
        let kept: Vec<ProposalSig> = kept.into_values().collect();
        let people: Vec<String> = kept.iter().filter_map(|s| admins.get(s.kid.as_str()).map(|p| p.to_string())).collect::<BTreeSet<_>>().into_iter().collect();
        let file = sigs_file(&kept);
        // The verifier decides; the count is only for display.
        match freshness::authenticate(&self.store.org, &self.store.store, bytes, &file, &c.roster) {
            Ok(a) => (file, QuorumView { met: true, have: a.signing_people, need: a.needed, people }, true),
            Err(_) => (file, QuorumView { met: false, have: people.len(), need: need_default, people }, false),
        }
    }

    /// Everything about proposal `id` against what the host serves now, judged. A refusal here means nobody should
    /// be asked to sign or publish.
    fn load_rewrite(&self, id: &str) -> Result<RewriteLoaded, AdminError> {
        if !valid_id(id) {
            return Err(AdminError::Refused(Refusal::BadInput("a rewrite proposal id looks like 0123456789ab-01234567")));
        }
        let cand = self.candidate()?;
        let branch = rewrite_branch(id);
        let (staged, tip) = self.fetch_branch(&branch, &format!("refs/xenia/rewrite-proposal/{id}"))?.ok_or_else(|| AdminError::RewriteProposalNotFound(id.to_string()))?;
        let bytes = self.store.git_ok(&["cat-file", "blob", &format!("{tip}:{}", statement_path(id))], freshness::MAX_STATEMENT_BYTES + 1).map_err(|_| AdminError::Refused(Refusal::BadInput("the proposal branch has no statement of the right size")))?;
        let sigs = self.read_sig_files(&tip, &sig_dir(id), &|kid| sig_path(id, kid))?;
        drop(staged);
        let st = Statement::parse(&bytes).map_err(|_| AdminError::RewriteStatementRefused("malformed"))?;
        if st.org != self.store.org || st.store != self.store.store {
            return Err(AdminError::RewriteStatementRefused("wrong-scope"));
        }
        if rewrite_id(&st.store_head, &bytes) != id {
            return Err(AdminError::RewriteStatementRefused("id-does-not-match-the-bytes"));
        }
        let Some(replaced) = st.rewrite_of.clone() else { return Err(AdminError::RewriteStatementRefused("names-no-rewrite")) };
        if st.store_head != cand.fetched {
            return Err(AdminError::RewriteHeadMoved);
        }
        if st.epoch != cand.roster.head_epoch() || st.epoch_hash != cand.roster.head_hash() {
            return Err(AdminError::RewriteStatementRefused("not-the-head-epoch"));
        }
        if !self.store.is_ancestor_or_equal(&cand.accepted, &replaced).map_err(xe)? {
            return Err(AdminError::RewriteOfNotOurHead);
        }
        freshness::currency(&st, self.cfg.now, &self.cfg.policy).map_err(AdminError::RewriteStatementRefused)?;
        Ok(RewriteLoaded { cand, id: id.to_string(), bytes, st, sigs })
    }

    fn rewrite_report(&self, l: &RewriteLoaded) -> RewriteReport {
        let (_, quorum, _) = self.rewrite_quorum(&l.cand, &l.bytes, &l.sigs);
        RewriteReport { id: l.id.clone(), branch: rewrite_branch(&l.id), bytes: l.bytes.clone(), quorum, view: Self::view_of(&l.cand), expires_at: l.st.expires_at.clone() }
    }

    /// `rewrite propose`: an admin of the roster the host serves writes the statement draft to
    /// `rewrite-proposal/<id>`. Nothing is signed.
    pub fn rewrite_propose(&self, proposer: &Identity, validity_secs: i64) -> Result<RewriteProposal, AdminError> {
        let cand = self.candidate()?;
        Self::require_head_admin(&cand, proposer)?;
        let bytes = admin::build_rewrite_statement(&self.store.org, &self.store.store, cand.roster.head_epoch(), cand.roster.head_hash(), &cand.fetched, &cand.accepted, self.cfg.now, validity_secs, &self.cfg.policy)?;
        self.check_issue_time(&bytes)?;
        let id = rewrite_id(&cand.fetched, &bytes);
        let commit = self.commit_on(Some(&cand.fetched), &[(statement_path(&id), bytes)], &format!("xenia: rewrite proposal {id}"))?;
        match self.push(&commit, &rewrite_branch(&id))? {
            Pushed::Yes => {}
            Pushed::NotFastForward => return Err(AdminError::RewriteProposalExists(id)),
        }
        let l = self.load_rewrite(&id)?;
        Ok(RewriteProposal { report: self.rewrite_report(&l), commit })
    }

    /// Read-only: what proposal `id` is, who signed it, and whether a majority has.
    pub fn rewrite_inspect(&self, id: &str) -> Result<RewriteReport, AdminError> {
        let l = self.load_rewrite(id)?;
        Ok(self.rewrite_report(&l))
    }

    pub fn prepare_rewrite_sign(&self, signer: &Identity, id: &str) -> Result<Prepared<RewriteSignPayload>, AdminError> {
        let l = self.load_rewrite(id)?;
        Self::require_head_admin(&l.cand, signer)?;
        self.check_issue_time(&l.bytes)?;
        let (_, quorum, _) = self.rewrite_quorum(&l.cand, &l.bytes, &l.sigs);
        let held = Held {
            action: Action::SignRewrite,
            org: self.store.org.clone(),
            store: self.store.store.clone(),
            epoch: l.st.epoch,
            signer: signer.did(),
            files: vec![PreviewFile { path: layout::FRESHNESS.to_string(), bytes: l.bytes.clone() }],
            domains: vec![admin::statement_domain(&self.store.org, &self.store.store, &l.bytes)?],
            store_head: Some(l.st.store_head.clone()),
            expires_at: Some(l.st.expires_at.clone()),
            flags: vec![Flag::RewritesHistory],
            rewrite: Some(Self::view_of(&l.cand)),
        };
        let parts = PreviewParts { quorum: Some(quorum), ..Default::default() };
        Ok(make_prepared(held, parts, RewriteSignPayload { id: l.id, bytes: l.bytes }))
    }

    /// Push the signature to the proposal branch as a fast-forward of whatever tip it has now, provided the statement
    /// there is still exactly the one that was signed.
    pub fn deliver_rewrite_signature(&self, signed: &Signed<RewriteSignPayload>) -> Result<SigDelivered, AdminError> {
        let p = &signed.payload;
        let sig = ProposalSig { kid: signed.signer.clone(), sig: signed.sigs[0].clone() };
        let path = sig_path(&p.id, &sig.kid).ok_or(AdminError::Refused(Refusal::BadInput("kid")))?;
        let branch = rewrite_branch(&p.id);
        for _ in 0..=MAX_REBASES {
            let (staged, tip) = self.fetch_branch(&branch, &format!("refs/xenia/rewrite-proposal/{}", p.id))?.ok_or_else(|| AdminError::RewriteProposalNotFound(p.id.clone()))?;
            let bytes = self.store.git_ok(&["cat-file", "blob", &format!("{tip}:{}", statement_path(&p.id))], freshness::MAX_STATEMENT_BYTES + 1).map_err(|_| AdminError::ProposalChanged)?;
            let sigs = self.read_sig_files(&tip, &sig_dir(&p.id), &|kid| sig_path(&p.id, kid))?;
            drop(staged);
            if bytes != p.bytes {
                return Err(AdminError::ProposalChanged);
            }
            if sigs.contains(&sig) {
                return Ok(SigDelivered { commit: tip, already_there: true });
            }
            let commit = self.commit_on(Some(&tip), &[(path.clone(), sig.to_json())], &format!("xenia: signature for rewrite proposal {}", p.id))?;
            match self.push(&commit, &branch)? {
                Pushed::Yes => return Ok(SigDelivered { commit, already_there: false }),
                Pushed::NotFastForward => continue,
            }
        }
        Err(AdminError::TooManyRaces)
    }

    /// The whole of `rewrite sign`: prepare, one decision, sign, deliver.
    pub fn rewrite_sign(&self, signer: &Identity, id: &str, approver: &mut dyn AdminApprover) -> Result<SigDelivered, AdminError> {
        let prepared = self.prepare_rewrite_sign(signer, id)?;
        let decision = approver.decide(prepared.preview());
        let signed = match decision {
            AdminDecision::Approve(a) => prepared.sign(&a, signer)?,
            AdminDecision::Deny => return Err(AdminError::Denied),
        };
        self.deliver_rewrite_signature(&signed)
    }

    /// `rewrite publish`: refuses unless the freshness verifier counts a majority of the admin **people** over these
    /// exact bytes, and unless the publisher is an `active` admin. Adds no signature of the publisher's own.
    pub fn prepare_rewrite_publish(&self, publisher: &Identity, id: &str) -> Result<Prepared<RewritePublishPayload>, AdminError> {
        let l = self.load_rewrite(id)?;
        Self::require_head_admin(&l.cand, publisher)?;
        self.check_issue_time(&l.bytes)?;
        let (file, quorum, met) = self.rewrite_quorum(&l.cand, &l.bytes, &l.sigs);
        if !met {
            return Err(AdminError::QuorumNotMet { have: quorum.have, need: quorum.need });
        }
        let held = Held {
            action: Action::PublishRewrite,
            org: self.store.org.clone(),
            store: self.store.store.clone(),
            epoch: l.st.epoch,
            signer: publisher.did(),
            files: vec![PreviewFile { path: layout::FRESHNESS.to_string(), bytes: l.bytes.clone() }, PreviewFile { path: layout::FRESHNESS_SIGS.to_string(), bytes: file.clone() }],
            domains: Vec::new(),
            store_head: Some(l.st.store_head.clone()),
            expires_at: Some(l.st.expires_at.clone()),
            flags: vec![Flag::RewritesHistory],
            rewrite: Some(Self::view_of(&l.cand)),
        };
        let parts = PreviewParts { quorum: Some(quorum), ..Default::default() };
        Ok(make_prepared(held, parts, RewritePublishPayload { bytes: l.bytes, sigs_file: file, epoch: l.st.epoch, epoch_hash: l.st.epoch_hash }))
    }

    /// Put the two statement files on top of the head the host serves, re-verified with the verifier's own code,
    /// fast-forward only.
    pub fn deliver_rewrite_publish(&self, signed: &Signed<RewritePublishPayload>) -> Result<Delivered, AdminError> {
        let p = &signed.payload;
        let st = Statement::parse(&p.bytes).map_err(|_| AdminError::RewriteStatementRefused("malformed"))?;
        let cand = self.candidate()?;
        let files = vec![(layout::FRESHNESS.to_string(), p.bytes.clone()), (layout::FRESHNESS_SIGS.to_string(), p.sigs_file.clone())];
        let paths: Vec<String> = files.iter().map(|f| f.0.clone()).collect();
        if cand.fetched != st.store_head {
            // The very bytes may already be on top of the head we meant: then there is nothing left to do.
            // Both files, byte for byte: the statement alone without its signatures verifies for nobody.
            let both = cand.files.get(layout::FRESHNESS) == Some(&p.bytes) && cand.files.get(layout::FRESHNESS_SIGS) == Some(&p.sigs_file);
            if both && self.store.is_ancestor(&st.store_head, &cand.fetched).map_err(xe)? {
                return Ok(Delivered { commit: cand.fetched.clone(), parent: st.store_head, paths, attempts: 1, already_there: true });
            }
            return Err(AdminError::RewriteHeadMoved);
        }
        let candidate = self.commit_on(Some(&cand.fetched), &files, &format!("xenia: rewrite statement for {}", &st.store_head[..12]))?;
        self.verify_candidate(&cand.fetched, &cand.files, &candidate, &files, &Goal::Refresh { epoch: p.epoch, epoch_hash: &p.epoch_hash })?;
        match self.push(&candidate, &self.store.branch)? {
            Pushed::Yes => Ok(Delivered { commit: candidate, parent: cand.fetched, paths, attempts: 1, already_there: false }),
            Pushed::NotFastForward => Err(AdminError::RewriteHeadMoved),
        }
    }

    pub fn rewrite_publish(&self, publisher: &Identity, id: &str, approver: &mut dyn AdminApprover) -> Result<Delivered, AdminError> {
        let prepared = self.prepare_rewrite_publish(publisher, id)?;
        let decision = approver.decide(prepared.preview());
        let signed = match decision {
            AdminDecision::Approve(a) => prepared.sign(&a, publisher)?,
            AdminDecision::Deny => return Err(AdminError::Denied),
        };
        self.deliver_rewrite_publish(&signed)
    }
}

#[cfg(test)]
#[path = "xenia_admin_rewrite_tests.rs"]
mod tests;
