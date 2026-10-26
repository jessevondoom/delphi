//! The member commands (spec 011 §2.2, §7.3, §7.4, §8.1; spec 012 §5.2-§5.3; plan milestone X7): `endorse`,
//! `dispute`, `retract` (an endorsement `withdraw`), `withdraw`, `restore` and `republish --resign`.
//!
//! Each command reads the **verified** store, checks locally the rules a verifier would apply later, builds the
//! record or records, and hands them to the publish gate ([`crate::xenia_publish`]) which scans, previews the exact
//! bytes, asks once, signs and delivers. The local checks exist so that the operator hears "you are not a steward
//! for these tags" now, instead of publishing a record every verifier will ignore. They are **advisory for others
//! and binding for us**: a verifier never trusts them (it recomputes everything in `tier::compute`), and a hostile
//! client that skips them gains nothing.
//!
//! What is checked, per command:
//!
//! | command | local rules (a verifier applies the same, from the roster and the store) |
//! | :--- | :--- |
//! | `endorse` | signer is an active steward; the target is a visible `memory`; the target's tags are in the signer's areas; the scope is inside areas and tags; not the author's person (or successor chain); a changed mind is a higher `rev` of the signer's own record for that revision |
//! | `dispute` | any active member; a visible `memory`; a note (required) |
//! | `retract` | the signer has an `endorse` of that revision to withdraw |
//! | `withdraw` | the author's person (through the `successor` chain), or a steward whose areas cover the **union** of the target's tags over all revisions; one tombstone per (signer, target), changes are a higher `rev` |
//! | `restore` | the same signers; **refused** when the author themself withdrew or superseded the record (author-set `withdrawn` is terminal, spec 012 §2.1) or when a `sensitive` steward withdrawal holds and the signer is not that steward |
//! | `resign` | the old key is `retired` with this install's key as `successor` by a **routine** rotation (`voids_tombstones` unset: a compromise does no automatic re-signing, spec 011 §8.2); records with an effective `withdraw` are skipped, never re-published |

use crate::xenia_publish::{Approver, Item, Origin, Published, PublishError, Publisher, StoreView};
use delphi_xenia::hash;
use delphi_xenia::identity::Identity;
use delphi_xenia::layout::Kind;
use delphi_xenia::record::{
    AuthorStatus, BodyKind, Check, Confidence, EndorsementFields, KindFields, Record, Redaction, Role, SourceCeiling, TombstoneAction, TombstoneFields, TombstoneReason, Verdict,
};
use delphi_xenia::roster::{Areas, Status};
use delphi_xenia::tier::{self, HiddenBy, Inputs, Outcome, Visibility, VerifiedRecord};
use std::collections::BTreeSet;

fn rule(s: impl Into<String>) -> PublishError {
    PublishError::Rule(s.into())
}

/// A record with only the kind-specific part filled in; the gate sets identity, scope, dates, rev and the redaction
/// block. Endorsements and tombstones carry no body.
fn shell(kind_fields: KindFields, title: String, summary: &str, tags: Vec<String>) -> Record {
    Record {
        id: String::new(),
        rev: 0,
        org: String::new(),
        store: String::new(),
        author: String::new(),
        author_handle: String::new(),
        author_role: Role::Operator,
        title,
        summary: summary.to_string(),
        tags,
        created: String::new(),
        updated: String::new(),
        // A human typed this command: the lattice's `user_ratified`.
        source_ceiling: SourceCeiling::UserRatified,
        confidence: Confidence::Medium,
        status: AuthorStatus::Active,
        supersedes: None,
        review_by: None,
        expires: None,
        pointers: Vec::new(),
        evidence: Vec::new(),
        related: Vec::new(),
        redaction: Redaction { scanner: String::new(), scanner_version: String::new(), content_hash: hash::of(b"unsealed") },
        body_kind: BodyKind::None,
        kind_fields,
        body: String::new(),
    }
}

/// The memory a command is about: it must be a verified `memory` record in force.
fn target_memory<'v>(view: &'v StoreView, id: &str) -> Result<&'v VerifiedRecord, PublishError> {
    let v = view.record(id).ok_or_else(|| rule(format!("{id} is not in the store (sync first, or check the id)")))?;
    if v.record().kind() != Kind::Memory {
        return Err(rule(format!("{id} is not a memory record")));
    }
    Ok(v)
}

fn same_person(view: &StoreView, a: &str, b: &str) -> bool {
    a == b
        || match (view.roster.person_of(a), view.roster.person_of(b)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        }
        || view.roster.successor_chain(a).iter().any(|d| d == b)
        || view.roster.successor_chain(b).iter().any(|d| d == a)
}

/// An active member of the roster, or the reason this key has no standing.
fn active_member<'v>(view: &'v StoreView, me: &str) -> Result<&'v delphi_xenia::roster::Member, PublishError> {
    view.roster.member(me).filter(|m| m.is_active()).ok_or(PublishError::SignerNotActive)
}

/// This signer's own record of `kind` aimed at `(target_id, rev)` (an endorsement) or `target_id` (a tombstone).
fn own_endorsement<'v>(view: &'v StoreView, me: &str, target_id: &str, target_rev: u64) -> Option<&'v VerifiedRecord> {
    view.records.iter().find(|r| {
        r.signer() == me && matches!(&r.record().kind_fields, KindFields::Endorsement(e) if e.target_id == target_id && e.target_rev == target_rev)
    })
}

fn own_tombstone<'v>(view: &'v StoreView, me: &str, target_id: &str) -> Option<&'v VerifiedRecord> {
    view.records.iter().find(|r| r.signer() == me && matches!(&r.record().kind_fields, KindFields::Tombstone(t) if t.target == target_id))
}

fn areas_text(a: &Areas) -> String {
    match a {
        Areas::All => "*".into(),
        Areas::Tags(t) => t.join(", "),
    }
}

// ---------------------------------------------------------------------------------------------------------
// Endorsements

/// What a steward attests when they `endorse`: that they opened the target and its pointers and that the summary
/// matches (spec 011 §7.3). `checked` is what the console showed ticked.
#[derive(Debug, Clone)]
pub struct EndorseSpec {
    pub target: String,
    /// Topic areas covered; empty means "all of the signer's areas that the record's tags touch".
    pub scope: Vec<String>,
    pub checked: Vec<Check>,
    pub note: Option<String>,
}

fn endorsement_item(view: &StoreView, me: &str, target: &VerifiedRecord, verdict: Verdict, scope: Vec<String>, checked: Vec<Check>, note: Option<String>) -> Result<Item, PublishError> {
    let rec = target.record();
    let bytes = view.bytes_of(target).ok_or_else(|| rule("the target's bytes are not in the store"))?;
    let target_hash = hash::of(bytes);
    let mut record = shell(
        KindFields::Endorsement(EndorsementFields { target_id: rec.id.clone(), target_rev: rec.rev, target_hash, verdict, scope: scope.clone(), note, checked }),
        format!("{} of {} revision {}", verdict_word(verdict), rec.id, rec.rev),
        "A review of one revision of a memory record, bound to its exact bytes.",
        scope,
    );
    // A changed verdict is a higher rev of the signer's own record for this revision (spec 012 §5.2).
    if let Some(own) = own_endorsement(view, me, &rec.id, rec.rev) {
        record.id = own.record().id.clone();
    }
    Ok(Item { origin: Origin::Command, record })
}

fn verdict_word(v: Verdict) -> &'static str {
    match v {
        Verdict::Endorse => "Endorsement",
        Verdict::Dispute => "Dispute",
        Verdict::Withdraw => "Withdrawal of endorsement",
    }
}

/// Spec 011 §7.3 as local checks. Returns the scope to write.
fn check_endorse(view: &StoreView, me: &str, spec: &EndorseSpec) -> Result<Vec<String>, PublishError> {
    let member = active_member(view, me)?;
    if !member.steward_areas.is_steward() {
        return Err(rule("you are not a steward, so you cannot endorse (stewards are named in the roster; a member can dispute)"));
    }
    let target = target_memory(view, &spec.target)?;
    let rec = target.record();
    if view.computed.memories.get(&rec.id).is_some_and(|m| m.visibility.is_hidden()) {
        return Err(rule("the record is hidden (withdrawn, superseded or contested); endorsing it would change nothing"));
    }
    if !member.steward_areas.covers(&rec.tags) {
        return Err(rule(format!(
            "outside your areas: the record is tagged [{}] and your areas are [{}]; other stewards' verifiers would ignore this endorsement",
            rec.tags.join(", "),
            areas_text(&member.steward_areas)
        )));
    }
    if same_person(view, me, target.signer()) {
        return Err(rule("you cannot endorse your own record, from this key or any other key of yours (self-endorsement is ignored)"));
    }
    if spec.checked.is_empty() {
        return Err(rule("an endorsement attests that you opened the record and its pointers: say what you checked"));
    }
    let allowed: Vec<String> = match &member.steward_areas {
        Areas::All => rec.tags.clone(),
        Areas::Tags(a) => rec.tags.iter().filter(|t| a.contains(t)).cloned().collect(),
    };
    let scope = if spec.scope.is_empty() { allowed.clone() } else { spec.scope.clone() };
    if scope.is_empty() {
        return Err(rule("name the area your endorsement covers (the record has no tags for your areas to match)"));
    }
    let outside = match &member.steward_areas {
        Areas::All => !rec.tags.is_empty() && scope.iter().any(|s| !rec.tags.contains(s)),
        Areas::Tags(_) => scope.iter().any(|s| !allowed.contains(s)),
    };
    if outside {
        return Err(rule("the scope must be inside your areas and the record's tags"));
    }
    if let Some(own) = own_endorsement(view, me, &rec.id, rec.rev) {
        if let KindFields::Endorsement(e) = &own.record().kind_fields {
            if e.verdict == Verdict::Endorse && e.target_hash == hash::of(view.bytes_of(target).map(|b| b.as_slice()).unwrap_or(&[])) && e.scope == scope {
                return Err(rule("you already endorsed this exact revision"));
            }
        }
    }
    Ok(scope)
}

fn check_note(note: &Option<String>, required: bool) -> Result<(), PublishError> {
    match note {
        Some(n) if !n.is_empty() => Ok(()),
        _ if required => Err(rule("a note is required for a dispute: say what is wrong")),
        _ => Ok(()),
    }
}

impl<'a> Publisher<'a> {
    /// `delphi xenia endorse <id>`.
    pub fn endorse(&self, signer: &Identity, spec: EndorseSpec, approver: &mut dyn Approver) -> Result<Published, PublishError> {
        let me = signer.did();
        self.publish_with(signer, approver, move |view| {
            let scope = check_endorse(view, &me, &spec)?;
            check_note(&spec.note, false)?;
            let target = target_memory(view, &spec.target)?;
            Ok(vec![endorsement_item(view, &me, target, Verdict::Endorse, scope, spec.checked.clone(), spec.note.clone())?])
        })
    }

    /// `delphi xenia dispute <id> --note ...`: any member flags a record to the stewards. It changes no tier.
    pub fn dispute(&self, signer: &Identity, target: &str, note: String, approver: &mut dyn Approver) -> Result<Published, PublishError> {
        let me = signer.did();
        let target = target.to_string();
        self.publish_with(signer, approver, move |view| {
            active_member(view, &me)?;
            check_note(&Some(note.clone()), true)?;
            let t = target_memory(view, &target)?;
            Ok(vec![endorsement_item(view, &me, t, Verdict::Dispute, Vec::new(), vec![Check::TargetOpen], Some(note))?])
        })
    }

    /// `delphi xenia retract <id>`: withdraw one's own endorsement of the current revision.
    pub fn retract_endorsement(&self, signer: &Identity, target: &str, approver: &mut dyn Approver) -> Result<Published, PublishError> {
        let me = signer.did();
        let target = target.to_string();
        self.publish_with(signer, approver, move |view| {
            active_member(view, &me)?;
            let t = target_memory(view, &target)?;
            let own = own_endorsement(view, &me, &t.record().id, t.record().rev).ok_or_else(|| rule("you have not endorsed this revision, so there is nothing to withdraw"))?;
            if !matches!(&own.record().kind_fields, KindFields::Endorsement(e) if e.verdict == Verdict::Endorse) {
                return Err(rule("your latest word on this revision is not an endorsement, so there is nothing to withdraw"));
            }
            Ok(vec![endorsement_item(view, &me, t, Verdict::Withdraw, Vec::new(), Vec::new(), None)?])
        })
    }

    // -----------------------------------------------------------------------------------------------------
    // Tombstones

    /// `delphi xenia withdraw <id> --reason ...`
    pub fn withdraw(&self, signer: &Identity, target: &str, reason: TombstoneReason, note: Option<String>, approver: &mut dyn Approver) -> Result<Published, PublishError> {
        self.tombstone(signer, target, TombstoneAction::Withdraw, reason, note, approver)
    }

    /// `delphi xenia restore <id>`
    pub fn restore(&self, signer: &Identity, target: &str, note: Option<String>, approver: &mut dyn Approver) -> Result<Published, PublishError> {
        self.tombstone(signer, target, TombstoneAction::Restore, TombstoneReason::Other, note, approver)
    }

    fn tombstone(&self, signer: &Identity, target: &str, action: TombstoneAction, reason: TombstoneReason, note: Option<String>, approver: &mut dyn Approver) -> Result<Published, PublishError> {
        let me = signer.did();
        let target = target.to_string();
        let prior = self.prior_tags.clone();
        self.publish_with(signer, approver, move |view| {
            let member = active_member(view, &me)?;
            let t = target_memory(view, &target)?;
            let rec = t.record();
            // Who may sign: the author's person, or a covering steward over the union of the tags of all revisions.
            let author = same_person(view, &me, t.signer());
            if !author {
                let mut tags: BTreeSet<String> = rec.tags.iter().cloned().collect();
                if let Some(p) = prior.get(&rec.id) {
                    tags.extend(p.iter().cloned());
                }
                let tags: Vec<String> = tags.into_iter().collect();
                if !member.steward_areas.is_steward() || !member.steward_areas.covers(&tags) {
                    return Err(rule("only the record's author, or a steward whose areas cover its tags, can withdraw or restore it"));
                }
            }
            let state = view.computed.memories.get(&rec.id);
            let own = own_tombstone(view, &me, &rec.id);
            match action {
                TombstoneAction::Withdraw => {
                    if rec.status != AuthorStatus::Active {
                        return Err(rule("the author already hid this record; a tombstone adds nothing"));
                    }
                    if let Some(o) = own {
                        if matches!(&o.record().kind_fields, KindFields::Tombstone(x) if x.action == TombstoneAction::Withdraw && x.reason == reason) {
                            return Err(rule("you already withdrew this record"));
                        }
                    }
                }
                TombstoneAction::Restore => {
                    match rec.status {
                        AuthorStatus::Withdrawn => return Err(rule("the author withdrew this record themselves; an author's withdrawal is final and a restore cannot undo it (publish a new record instead)")),
                        AuthorStatus::Superseded => return Err(rule("the author marked this record superseded; a restore cannot bring it back (publish a new record instead)")),
                        AuthorStatus::Active => {}
                    }
                    match state.map(|s| &s.visibility) {
                        Some(Visibility::Hidden(HiddenBy::SensitiveHold)) if !own.is_some_and(|o| matches!(&o.record().kind_fields, KindFields::Tombstone(x) if x.reason == TombstoneReason::Sensitive)) => {
                            return Err(rule("a steward withdrew this record as sensitive; only that steward can restore it"));
                        }
                        Some(Visibility::Visible) if own.is_none() => return Err(rule("the record is not hidden, so there is nothing to restore")),
                        _ => {}
                    }
                    if let Some(o) = own {
                        if matches!(&o.record().kind_fields, KindFields::Tombstone(x) if x.action == TombstoneAction::Restore) {
                            return Err(rule("you already restored this record"));
                        }
                    }
                }
            }
            let mut record = shell(
                KindFields::Tombstone(TombstoneFields { target: rec.id.clone(), action, reason, note: note.clone() }),
                format!("{} {}", if action == TombstoneAction::Withdraw { "Withdraw" } else { "Restore" }, rec.id),
                if action == TombstoneAction::Withdraw { "Hides one memory record from agents until it is restored." } else { "Brings one hidden memory record back." },
                rec.tags.clone(),
            );
            // One tombstone per (signer, target); a changed mind is a higher rev of that record.
            if let Some(o) = own {
                record.id = o.record().id.clone();
            }
            Ok(vec![Item { origin: Origin::Command, record }])
        })
    }

    // -----------------------------------------------------------------------------------------------------
    // Rotation

    /// `delphi xenia republish --resign` after a **routine** rotation (spec 011 §8.1): `old_did` is `retired` with
    /// `new_key` as its `successor`. In one batch (one preview, one approval, one commit): the old key's visible
    /// memories become new revisions signed by the successor; its endorsements are re-issued with the same
    /// `target_hash`; its tombstones, disputes and endorsement withdrawals are re-signed. Anything that already has
    /// an effective `withdraw`, or that the author hid, is skipped.
    pub fn resign(&self, new_key: &Identity, old_did: &str, approver: &mut dyn Approver) -> Result<Published, PublishError> {
        let me = new_key.did();
        let old = old_did.to_string();
        self.publish_with(new_key, approver, move |view| plan_resign(view, &old, &me))
    }
}

#[cfg(test)]
#[path = "xenia_member_tests.rs"]
mod tests;

/// What a re-sign would write, from the verified view. Pure.
pub fn plan_resign(view: &StoreView, old_did: &str, new_did: &str) -> Result<Vec<Item>, PublishError> {
    let old_entry = view.roster.member(old_did).ok_or_else(|| rule("the old key is not in the roster"))?;
    let new_entry = view.roster.member(new_did).filter(|m| m.is_active()).ok_or(PublishError::SignerNotActive)?;
    if old_entry.status != Status::Retired || old_entry.successor.as_deref() != Some(new_did) {
        return Err(rule("the old key is not retired with this key as its successor; rotation is an admin operation (rotate --routine)"));
    }
    if old_entry.voids_tombstones {
        return Err(rule("this rotation was for a compromise: nothing the old key signed has standing and nothing is re-signed automatically; publish and re-endorse what you reviewed"));
    }
    if old_entry.person != new_entry.person {
        return Err(rule("the successor is a different person"));
    }
    // The world as it stood before the rotation: the old key active again. That is how an effective withdraw on a
    // record the old key authored can be seen at all (after the rotation the record is quarantined and a
    // tombstone aimed at it finds no target).
    let mut pre = view.roster.clone();
    if let Some(e) = pre.epochs.last_mut() {
        if let Some(m) = e.epoch.members.iter_mut().find(|m| m.did == old_did) {
            m.status = Status::Active;
        }
    }
    let prior = std::collections::BTreeMap::new();
    let computed = tier::compute(&Inputs { roster: &pre, records: &view.records, freshness: &view.freshness, prior_tags: &prior });
    let mut items = Vec::new();
    let mut mine: Vec<&VerifiedRecord> = view.records.iter().filter(|r| r.signer() == old_did).collect();
    mine.sort_by_key(|r| r.path());
    for r in mine {
        let rec = r.record();
        let keep = match &rec.kind_fields {
            KindFields::Memory(_) => rec.status == AuthorStatus::Active && computed.memories.get(&rec.id).is_some_and(|m| !m.visibility.is_hidden()),
            KindFields::Endorsement(e) => match e.verdict {
                Verdict::Endorse => matches!(computed.outcome(Kind::Endorsement, &rec.id), Some(Outcome::Effective) | Some(Outcome::NoEffect("freshness-stale"))),
                _ => matches!(computed.outcome(Kind::Endorsement, &rec.id), Some(Outcome::Effective)),
            },
            KindFields::Tombstone(_) => matches!(computed.outcome(Kind::Tombstone, &rec.id), Some(Outcome::Effective)),
        };
        if !keep {
            continue;
        }
        let mut record = rec.clone();
        record.rev = 0;
        items.push(Item { origin: Origin::Resign, record });
    }
    if items.is_empty() {
        return Err(rule("nothing to re-sign: the old key has no records in force"));
    }
    Ok(items)
}
