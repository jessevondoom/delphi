//! Tests for the member commands. Same attacker model as the gate's tests: every case is a real store, a real
//! roster and real signatures, and "refused" means the remote did not move.

use super::*;
use crate::xenia_publish::tests::{draft, Closure, Fx, Yes};
use crate::xenia_publish::{Decision, NoIndex, Preview};
use delphi_xenia::testkit::TestOrg;
use delphi_xenia::tier::Tier;

impl Fx {
    /// Publish the next epoch of `org` (built from this fixture's chain) and a fresh statement for it.
    fn push_epoch(&self, org: &TestOrg) {
        self.catch_up();
        let n = org.files.len();
        self.remote.write(&format!("roster/epochs/{n}.json"), &org.files[n - 1].epoch_json);
        self.remote.write(&format!("roster/epochs/{n}.sigs.json"), &org.files[n - 1].sigs_json);
        let c = self.remote.commit(&format!("epoch {n}"));
        self.fresh_for(org, &c);
    }

    fn spec(&self, target: &str) -> EndorseSpec {
        EndorseSpec { target: target.into(), scope: vec![], checked: vec![Check::TargetOpen, Check::SummaryMatches], note: None }
    }

    fn endorse(&self, who: &str, target: &str) -> Result<Published, PublishError> {
        self.publisher(&NoIndex).endorse(&self.key(who), self.spec(target), &mut Yes::new())
    }

    /// The command was refused locally: nothing was previewed, asked or sent.
    fn refused(&self, r: Result<Published, PublishError>, head: &str, says: &str) {
        let err = r.unwrap_err();
        assert_eq!(err.reason(), "rule", "{err}");
        assert!(err.to_string().contains(says), "wanted {says:?} in {err}");
        assert_eq!(self.host_head(), head, "a refused command still sent something");
    }
}

fn rust_memory(fx: &Fx, who: &str) -> String {
    fx.memory_id(who, draft("Stripe retries", &["rust"]))
}

// ---------------------------------------------------------------------------------------------------------
// endorse / dispute / retract

#[test]
fn a_steward_endorses_the_exact_revision_and_a_new_revision_needs_a_new_endorsement() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    assert_eq!(fx.tier_of(&id), Tier::L1);
    fx.endorse("dave", &id).unwrap();
    assert_eq!(fx.tier_of(&id), Tier::L2);
    // The endorsement carries the BLAKE3 of the bytes the steward reviewed.
    let v = fx.view();
    let md = v.files.get(&format!("records/memory/{id}.md")).unwrap();
    let e = v.records.iter().find(|r| matches!(&r.record().kind_fields, KindFields::Endorsement(_))).unwrap();
    let KindFields::Endorsement(f) = &e.record().kind_fields else { unreachable!() };
    assert_eq!((f.target_id.as_str(), f.target_rev, f.verdict), (id.as_str(), 1, Verdict::Endorse));
    assert_eq!(f.target_hash, hash::of(md));
    assert_eq!(f.scope, vec!["rust".to_string()]);
    // A new revision of the memory voids it (hash and rev mismatch): back to L1 until endorsed again.
    let mut d = draft("Stripe retries, updated", &["rust"]);
    d.id = Some(id.clone());
    fx.publish_memory("erin", d).unwrap();
    assert_eq!(fx.tier_of(&id), Tier::L1);
    fx.endorse("dave", &id).unwrap();
    assert_eq!(fx.tier_of(&id), Tier::L2);
}

#[test]
fn endorsing_outside_your_areas_is_refused_here_with_a_clear_message_even_though_verifiers_would_ignore_it() {
    let fx = Fx::new();
    let id = fx.memory_id("erin", draft("Payments gotcha", &["payments"]));
    let head = fx.host_head();
    let mut yes = Yes::new();
    let err = fx.publisher(&NoIndex).endorse(&fx.key("dave"), fx.spec(&id), &mut yes).unwrap_err();
    assert_eq!(yes.calls, 0, "the operator was asked to approve something every verifier would ignore");
    let text = err.to_string();
    assert!(text.contains("outside your areas") && text.contains("payments") && text.contains("rust"), "{text}");
    assert_eq!(fx.host_head(), head);
    // The right steward, and a wildcard steward, can.
    fx.endorse("frank", &id).unwrap();
    assert_eq!(fx.tier_of(&id), Tier::L2);
    let other = fx.memory_id("erin", draft("Ops gotcha", &["ops"]));
    fx.endorse("gail", &other).unwrap();
}

#[test]
fn a_member_who_is_not_a_steward_cannot_endorse_and_a_wildcard_steward_must_name_an_area_on_an_untagged_record() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    let head = fx.host_head();
    fx.refused(fx.endorse("hank", &id), &head, "not a steward");
    let untagged = fx.memory_id("erin", draft("No tags", &[]));
    let head = fx.host_head();
    fx.refused(fx.endorse("gail", &untagged), &head, "name the area");
    // Explicit scope on an untagged record is taken as written (tier.rs decision for the 011/012 gap).
    let mut spec = fx.spec(&untagged);
    spec.scope = vec!["ops".into()];
    fx.publisher(&NoIndex).endorse(&fx.key("gail"), spec, &mut Yes::new()).unwrap();
    // A scope outside the record's tags is refused.
    let mut spec = fx.spec(&id);
    spec.scope = vec!["payments".into()];
    let head = fx.host_head();
    fx.refused(fx.publisher(&NoIndex).endorse(&fx.key("gail"), spec, &mut Yes::new()), &head, "scope must be inside");
    // An endorsement attests something: say what you checked.
    let mut spec = fx.spec(&id);
    spec.checked.clear();
    fx.refused(fx.publisher(&NoIndex).endorse(&fx.key("dave"), spec, &mut Yes::new()), &head, "what you checked");
}

#[test]
fn nobody_endorses_their_own_record_from_any_of_their_keys() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "dave");
    let head = fx.host_head();
    fx.refused(fx.endorse("dave", &id), &head, "your own record");
    // After a routine rotation the successor key is the same person.
    let mut org2 = fx.org.clone();
    org2.advance(&["alice", "bob"], |t, spec| t.rotate(spec, "dave", "dave2", false));
    fx.push_epoch(&org2);
    let head = fx.host_head();
    let r = fx.publisher(&NoIndex).endorse(&org2.key("dave2"), fx.spec(&id), &mut Yes::new());
    fx.refused(r, &head, "your own record");
}

#[test]
fn one_cannot_endorse_a_hidden_or_missing_or_non_memory_target() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    fx.publisher(&NoIndex).withdraw(&fx.key("erin"), &id, TombstoneReason::Mistake, None, &mut Yes::new()).unwrap();
    let head = fx.host_head();
    fx.refused(fx.endorse("dave", &id), &head, "hidden");
    fx.refused(fx.endorse("dave", "01JBX3Q8T2M9Z4K7WYH5N6D999"), &head, "not in the store");
}

#[test]
fn a_changed_mind_is_a_higher_rev_of_the_same_record_and_a_retraction_lowers_the_tier() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    let first = fx.endorse("dave", &id).unwrap();
    let eid = first.paths[0].clone();
    assert_eq!(fx.tier_of(&id), Tier::L2);
    // The same exact endorsement again is refused as a no-op.
    let head = fx.host_head();
    fx.refused(fx.endorse("dave", &id), &head, "already endorsed");
    // Retract: rev 2 of the same file, verdict withdraw.
    let out = fx.publisher(&NoIndex).retract_endorsement(&fx.key("dave"), &id, &mut Yes::new()).unwrap();
    assert_eq!(out.paths[0], eid, "same record, higher rev");
    let v = fx.view();
    let rec = v.records.iter().find(|r| r.path() == eid).unwrap();
    assert_eq!(rec.record().rev, 2);
    assert_eq!(fx.tier_of(&id), Tier::L1);
    // Nothing left to retract.
    let head = fx.host_head();
    fx.refused(fx.publisher(&NoIndex).retract_endorsement(&fx.key("dave"), &id, &mut Yes::new()), &head, "nothing to withdraw");
    // And endorsing again is rev 3 of the same record.
    let out = fx.endorse("dave", &id).unwrap();
    assert_eq!(out.paths[0], eid);
    assert_eq!(fx.view().records.iter().find(|r| r.path() == eid).unwrap().record().rev, 3);
    assert_eq!(fx.tier_of(&id), Tier::L2);
}

#[test]
fn any_member_can_dispute_with_a_note_and_it_changes_no_tier() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    fx.endorse("dave", &id).unwrap();
    let head = fx.host_head();
    let no_note = fx.publisher(&NoIndex).dispute(&fx.key("hank"), &id, String::new(), &mut Yes::new());
    fx.refused(no_note, &head, "note is required");
    fx.publisher(&NoIndex).dispute(&fx.key("hank"), &id, "The retry window is wrong for the v2 API.".into(), &mut Yes::new()).unwrap();
    let v = fx.view();
    assert!(v.computed.memories[&id].disputed());
    assert_eq!(v.computed.tier_of(&id), Some(Tier::L2), "a dispute flags; it does not demote");
    // A stranger key is refused.
    let stranger = delphi_xenia::testkit::key(77);
    let err = fx.publisher(&NoIndex).dispute(&stranger, &id, "x".into(), &mut Yes::new()).unwrap_err();
    assert_eq!(err, PublishError::SignerNotActive);
}

// ---------------------------------------------------------------------------------------------------------
// withdraw / restore

fn visibility(fx: &Fx, id: &str) -> Visibility {
    fx.view().computed.memories[id].visibility.clone()
}

#[test]
fn the_author_withdraws_and_restores_with_one_tombstone_record_that_gains_a_rev() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    let w = fx.publisher(&NoIndex).withdraw(&fx.key("erin"), &id, TombstoneReason::Mistake, Some("Wrong API version.".into()), &mut Yes::new()).unwrap();
    assert_eq!(visibility(&fx, &id), Visibility::Hidden(HiddenBy::AuthorTombstone));
    let head = fx.host_head();
    fx.refused(fx.publisher(&NoIndex).withdraw(&fx.key("erin"), &id, TombstoneReason::Mistake, None, &mut Yes::new()), &head, "already withdrew");
    let r = fx.publisher(&NoIndex).restore(&fx.key("erin"), &id, None, &mut Yes::new()).unwrap();
    assert_eq!(r.paths[0], w.paths[0], "one tombstone per (signer, target); a changed mind is a higher rev");
    assert_eq!(visibility(&fx, &id), Visibility::Visible);
    assert_eq!(fx.view().records.iter().find(|x| x.path() == w.paths[0]).unwrap().record().rev, 2);
}

#[test]
fn who_may_withdraw_a_covering_steward_the_author_and_nobody_else() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    let head = fx.host_head();
    let who = "only the record's author, or a steward whose areas cover its tags";
    fx.refused(fx.publisher(&NoIndex).withdraw(&fx.key("hank"), &id, TombstoneReason::Mistake, None, &mut Yes::new()), &head, who);
    fx.refused(fx.publisher(&NoIndex).withdraw(&fx.key("frank"), &id, TombstoneReason::Mistake, None, &mut Yes::new()), &head, who);
    fx.publisher(&NoIndex).withdraw(&fx.key("dave"), &id, TombstoneReason::Other, None, &mut Yes::new()).unwrap();
    assert_eq!(visibility(&fx, &id), Visibility::Hidden(HiddenBy::StewardTombstone));
}

#[test]
fn the_tags_of_earlier_revisions_count_for_a_stewards_area() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    // erin retags the record away from `rust` in revision 2 to escape dave.
    let mut d = draft("Stripe retries", &["ops"]);
    d.id = Some(id.clone());
    fx.publish_memory("erin", d).unwrap();
    let head = fx.host_head();
    let r = fx.publisher(&NoIndex).withdraw(&fx.key("dave"), &id, TombstoneReason::Mistake, None, &mut Yes::new());
    fx.refused(r, &head, "cover its tags");
    // With the tag history the caller keeps (the union over revisions), dave covers it.
    let mut p = fx.publisher(&NoIndex);
    p.prior_tags.insert(id.clone(), ["rust".to_string()].into_iter().collect());
    p.withdraw(&fx.key("dave"), &id, TombstoneReason::Mistake, None, &mut Yes::new()).unwrap();
}

#[test]
fn a_restore_of_a_record_the_author_withdrew_is_refused() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    let mut d = draft("Stripe retries", &["rust"]);
    d.id = Some(id.clone());
    d.status = AuthorStatus::Withdrawn;
    fx.publish_memory("erin", d.clone()).unwrap();
    let head = fx.host_head();
    for who in ["erin", "dave", "gail"] {
        fx.refused(fx.publisher(&NoIndex).restore(&fx.key(who), &id, None, &mut Yes::new()), &head, "final");
    }
    // Superseded by the author is the same: nothing the author hid comes back through a tombstone.
    let id2 = rust_memory(&fx, "erin");
    let mut d = draft("Stripe retries v2", &["rust"]);
    d.id = Some(id2.clone());
    d.status = AuthorStatus::Superseded;
    fx.publish_memory("erin", d).unwrap();
    let head = fx.host_head();
    fx.refused(fx.publisher(&NoIndex).restore(&fx.key("erin"), &id2, None, &mut Yes::new()), &head, "superseded");
}

#[test]
fn a_sensitive_withdrawal_is_reversed_only_by_the_steward_who_made_it_and_asks_for_the_secret_to_be_rotated() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    let mut yes = Yes::new();
    fx.publisher(&NoIndex).withdraw(&fx.key("dave"), &id, TombstoneReason::Sensitive, Some("Contains a token.".into()), &mut yes).unwrap();
    assert!(yes.seen[0].reminders.contains(&"rotate-secrets"), "{:?}", yes.seen[0].reminders);
    assert_eq!(visibility(&fx, &id), Visibility::Hidden(HiddenBy::SensitiveHold));
    let head = fx.host_head();
    for who in ["erin", "gail"] {
        fx.refused(fx.publisher(&NoIndex).restore(&fx.key(who), &id, None, &mut Yes::new()), &head, "only that steward");
    }
    fx.publisher(&NoIndex).restore(&fx.key("dave"), &id, None, &mut Yes::new()).unwrap();
    assert_eq!(visibility(&fx, &id), Visibility::Visible);
}

#[test]
fn restoring_something_that_is_not_hidden_is_refused() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    let head = fx.host_head();
    fx.refused(fx.publisher(&NoIndex).restore(&fx.key("erin"), &id, None, &mut Yes::new()), &head, "not hidden");
}

#[test]
fn the_gate_scans_a_note_typed_into_a_command() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    let head = fx.host_head();
    let err = fx
        .publisher(&NoIndex)
        .withdraw(&fx.key("erin"), &id, TombstoneReason::Sensitive, Some("the token was ghp_abcdefghijklmnopqrstuvwxyz0123456789".into()), &mut Yes::new())
        .unwrap_err();
    assert_eq!(err.reason(), "scan-blocked");
    assert_eq!(fx.host_head(), head);
}

#[test]
fn the_approver_sees_a_command_as_exact_bytes_too() {
    let fx = Fx::new();
    let id = rust_memory(&fx, "erin");
    let mut seen: Option<Preview> = None;
    let mut approver = Closure(|p: &Preview| {
        seen = Some(p.clone());
        Decision::Approve(p.approve(true))
    });
    fx.publisher(&NoIndex).endorse(&fx.key("dave"), fx.spec(&id), &mut approver).unwrap();
    let p = seen.unwrap();
    assert_eq!(p.items.len(), 1);
    assert_eq!(fx.host_file(&p.items[0].path).unwrap(), p.items[0].bytes);
}

// ---------------------------------------------------------------------------------------------------------
// republish --resign (spec 011 §8.1, spec 012 test 18)

#[test]
fn a_routine_rotation_then_resign_keeps_endorsements_and_skips_what_is_hidden() {
    let fx = Fx::new();
    // erin's note, endorsed by dave: L2.
    let m = rust_memory(&fx, "erin");
    fx.endorse("dave", &m).unwrap();
    // dave's own visible note, and a second one he withdrew himself.
    let visible = fx.memory_id("dave", draft("Dave's visible note", &["rust"]));
    let hidden = fx.memory_id("dave", draft("Dave's withdrawn note", &["rust"]));
    fx.publisher(&NoIndex).withdraw(&fx.key("dave"), &hidden, TombstoneReason::Mistake, None, &mut Yes::new()).unwrap();
    // dave disputes someone else's record.
    let k = fx.memory_id("erin", draft("Another note", &["rust"]));
    fx.publisher(&NoIndex).dispute(&fx.key("dave"), &k, "Check the date.".into(), &mut Yes::new()).unwrap();
    assert_eq!(fx.tier_of(&m), Tier::L2);

    // Admins rotate dave's key, routine. Nothing the old key signed has standing (except fail-hidden withdrawals).
    let mut org2 = fx.org.clone();
    org2.advance(&["alice", "bob"], |t, spec| t.rotate(spec, "dave", "dave2", false));
    fx.push_epoch(&org2);
    assert_eq!(fx.tier_of(&m), Tier::L1, "the old key's endorsement lost standing");
    assert!(!fx.view().computed.memories.contains_key(&visible), "the old key's memory lost standing");

    // `republish --resign` with the successor key: one preview, one approval, one commit.
    let dave2 = org2.key("dave2");
    let mut yes = Yes::new();
    let out = fx.publisher(&NoIndex).resign(&dave2, &fx.key("dave").did(), &mut yes).unwrap();
    assert_eq!(yes.calls, 1);
    let kinds: Vec<(Kind, String)> = yes.seen[0].items.iter().map(|i| (i.kind, i.id.clone())).collect();
    assert!(kinds.iter().any(|(k, id)| *k == Kind::Memory && id == &visible), "{kinds:?}");
    assert!(!kinds.iter().any(|(_, id)| id == &hidden), "a record with an effective withdraw was re-published: {kinds:?}");
    assert!(kinds.iter().any(|(k, _)| *k == Kind::Endorsement), "{kinds:?}");
    assert!(kinds.iter().any(|(k, _)| *k == Kind::Tombstone), "the withdraw is re-signed so its effect continues");
    assert_eq!(out.paths.len(), yes.seen[0].items.len());
    assert!(yes.seen[0].items.iter().all(|i| i.signer == dave2.did() && i.rev == 2), "{:?}", yes.seen[0].items.iter().map(|i| i.rev).collect::<Vec<_>>());

    // The covered record keeps its tier; the member's own record is back at L1; the hidden record stays hidden.
    assert_eq!(fx.tier_of(&m), Tier::L2, "the re-issued endorsement keeps the record at L2");
    assert_eq!(fx.tier_of(&visible), Tier::L1);
    let v = fx.view();
    assert!(!v.computed.memories.contains_key(&hidden), "the withdrawn note was resurfaced");
    let old = v.files.get(&format!("records/memory/{hidden}.md")).unwrap();
    assert!(String::from_utf8_lossy(old).contains("rev: 1"), "the withdrawn note was re-published");
    // The re-issued endorsement binds the same bytes as before.
    let e = v.records.iter().find(|r| r.signer() == dave2.did() && matches!(&r.record().kind_fields, KindFields::Endorsement(e) if e.verdict == Verdict::Endorse)).unwrap();
    let KindFields::Endorsement(f) = &e.record().kind_fields else { unreachable!() };
    assert_eq!(f.target_hash, hash::of(v.files.get(&format!("records/memory/{m}.md")).unwrap()));
    assert!(v.quarantined.is_empty(), "{:?}", v.quarantined);
}

#[test]
fn a_compromise_rotation_is_never_re_signed_automatically() {
    let fx = Fx::new();
    let _ = rust_memory(&fx, "dave");
    let mut org2 = fx.org.clone();
    org2.advance(&["alice", "bob"], |t, spec| t.rotate(spec, "dave", "dave2", true));
    fx.push_epoch(&org2);
    let head = fx.host_head();
    let r = fx.publisher(&NoIndex).resign(&org2.key("dave2"), &fx.key("dave").did(), &mut Yes::new());
    fx.refused(r, &head, "compromise");
}

#[test]
fn resign_needs_a_real_successor_and_something_to_do() {
    let fx = Fx::new();
    let _ = rust_memory(&fx, "dave");
    let head = fx.host_head();
    // Not rotated at all.
    let r = fx.publisher(&NoIndex).resign(&fx.key("erin"), &fx.key("dave").did(), &mut Yes::new());
    fx.refused(r, &head, "not retired with this key as its successor");
    // Rotated, but the caller holds some other member's key.
    let mut org2 = fx.org.clone();
    org2.advance(&["alice", "bob"], |t, spec| t.rotate(spec, "dave", "dave2", false));
    fx.push_epoch(&org2);
    let head = fx.host_head();
    let r = fx.publisher(&NoIndex).resign(&fx.key("hank"), &fx.key("dave").did(), &mut Yes::new());
    fx.refused(r, &head, "not retired with this key as its successor");
    // Erin has no records: nothing to do, even after a rotation.
    let mut org3 = org2.clone();
    org3.advance(&["alice", "bob"], |t, spec| t.rotate(spec, "erin", "erin2", false));
    fx.push_epoch(&org3);
    let head = fx.host_head();
    let r = fx.publisher(&NoIndex).resign(&org3.key("erin2"), &fx.key("erin").did(), &mut Yes::new());
    fx.refused(r, &head, "nothing to re-sign");
}

#[test]
fn a_memory_a_steward_withdrew_is_not_resurfaced_by_its_authors_rotation() {
    let fx = Fx::new();
    let hidden = rust_memory(&fx, "erin");
    let kept = fx.memory_id("erin", draft("Kept", &["rust"]));
    fx.publisher(&NoIndex).withdraw(&fx.key("dave"), &hidden, TombstoneReason::Other, None, &mut Yes::new()).unwrap();
    let mut org2 = fx.org.clone();
    org2.advance(&["alice", "bob"], |t, spec| t.rotate(spec, "erin", "erin2", false));
    fx.push_epoch(&org2);
    let mut yes = Yes::new();
    fx.publisher(&NoIndex).resign(&org2.key("erin2"), &fx.key("erin").did(), &mut yes).unwrap();
    let ids: Vec<&str> = yes.seen[0].items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, vec![kept.as_str()], "only the record in force is re-published");
}
