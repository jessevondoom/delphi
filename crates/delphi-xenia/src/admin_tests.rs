use super::*;
use crate::freshness;
use crate::roster::{verify_chain, EpochFile};
use crate::testkit::{key, sign_bytes, TestOrg, ORG, STORE};

const NOW: &str = "2026-10-26T12:00:00Z";

fn head(t: &TestOrg) -> VerifiedEpoch {
    t.verify().unwrap().head().clone()
}

fn newcomer(seed: u8, person: &str) -> NewMember {
    NewMember { did: key(seed).did(), person: person.into(), handle: person.into(), admin: false, steward_areas: vec![] }
}

fn chain_with(t: &TestOrg, d: &Draft, signers: &[&str]) -> Vec<EpochFile> {
    let keys: Vec<_> = signers.iter().map(|n| t.key(n)).collect();
    let refs: Vec<&Identity> = keys.iter().collect();
    let mut files = t.files.clone();
    files.push(sign_bytes(ORG, STORE, d.epoch.epoch, &d.bytes, &refs));
    files
}

#[test]
fn add_drafts_a_chain_link_that_verifies_with_two_of_three() {
    let t = TestOrg::three_admins();
    let d = draft(&head(&t), &Op::Add(newcomer(40, "frank")), NOW).unwrap();
    assert_eq!(d.epoch.epoch, 2);
    let files = chain_with(&t, &d, &["alice", "bob"]);
    let r = verify_chain(ORG, STORE, &files, &t.pin_head()).unwrap();
    assert_eq!(r.head_epoch(), 2);
    assert!(r.is_active(&key(40).did()));
    // One signature is not enough.
    let files = chain_with(&t, &d, &["alice"]);
    assert!(verify_chain(ORG, STORE, &files, &t.pin_head()).is_err());
}

#[test]
fn draft_bytes_are_canonical_and_stable() {
    let t = TestOrg::three_admins();
    let d = draft(&head(&t), &Op::Add(newcomer(40, "frank")), NOW).unwrap();
    assert_eq!(serialize_epoch(&d.epoch), d.bytes);
    let again = draft(&head(&t), &Op::Add(newcomer(40, "frank")), NOW).unwrap();
    assert_eq!(d, again);
}

#[test]
fn rotation_needs_exactly_one_reason() {
    assert_eq!(Rotation::from_flags(true, false), Ok(Rotation::Routine));
    assert_eq!(Rotation::from_flags(false, true), Ok(Rotation::Compromised));
    assert_eq!(Rotation::from_flags(false, false), Err(AdminError::RotationReasonMissing));
    assert_eq!(Rotation::from_flags(true, true), Err(AdminError::RotationReasonConflict));
}

#[test]
fn a_compromised_rotation_forces_voids_tombstones_and_a_routine_one_does_not() {
    let t = TestOrg::three_admins();
    let (old, new) = (t.did("dave"), key(41).did());
    let comp = draft(&head(&t), &Op::Rotate { old: old.clone(), new: new.clone(), kind: Rotation::Compromised }, NOW).unwrap();
    let m = comp.epoch.member(&old).unwrap();
    assert!(m.voids_tombstones && m.status == Status::Retired && m.successor.as_deref() == Some(new.as_str()));
    let routine = draft(&head(&t), &Op::Rotate { old: old.clone(), new: new.clone(), kind: Rotation::Routine }, NOW).unwrap();
    assert!(!routine.epoch.member(&old).unwrap().voids_tombstones);
    // The successor inherits person, roles and areas exactly and starts clean.
    let s = routine.epoch.member(&new).unwrap();
    let o = routine.epoch.member(&old).unwrap();
    assert!(s.person == o.person && s.roles == o.roles && s.steward_areas == o.steward_areas);
    assert!(s.is_active() && s.successor.is_none() && !s.voids_tombstones);
    // Both are valid chain links.
    for d in [&comp, &routine] {
        verify_chain(ORG, STORE, &chain_with(&t, d, &["alice", "carol"]), &t.pin_head()).unwrap();
    }
}

#[test]
fn rotating_an_admin_keeps_the_admin_count_and_does_not_warn_about_a_second_key() {
    let t = TestOrg::three_admins();
    let d = draft(&head(&t), &Op::Rotate { old: t.did("alice"), new: key(42).did(), kind: Rotation::Routine }, NOW).unwrap();
    let (changes, warnings) = describe(&head(&t).epoch, &d.epoch);
    assert_eq!(changes.len(), 2);
    // A rotation hands admin to the new key by design; it is still listed, but no second-key or count warning.
    assert!(!warnings.iter().any(|w| matches!(w, Warning::SecondKeyForPerson(_) | Warning::AdminCountChanges { .. })), "{warnings:?}");
}

#[test]
fn drafts_that_break_a_rule_are_errors() {
    let t = TestOrg::three_admins();
    let h = head(&t);
    let listed = Op::Add(NewMember { did: t.did("erin"), ..newcomer(1, "erin2") });
    assert_eq!(draft(&h, &listed, NOW), Err(AdminError::AlreadyListed));
    assert_eq!(draft(&h, &Op::Remove { did: key(99).did(), voids_tombstones: false }, NOW), Err(AdminError::UnknownMember));
    assert_eq!(draft(&h, &Op::Rotate { old: t.did("erin"), new: t.did("erin"), kind: Rotation::Routine }, NOW), Err(AdminError::SameKey));
    // The new key of a rotation may not be a key already in the roster (an entry is never reused).
    assert_eq!(draft(&h, &Op::Rotate { old: t.did("erin"), new: t.did("dave"), kind: Rotation::Routine }, NOW), Err(AdminError::AlreadyListed));
    // Bad text is refused by the verifier's own parser, not written.
    let bad = Op::Add(NewMember { handle: "a b\nc".into(), ..newcomer(43, "gina") });
    assert!(matches!(draft(&h, &bad, NOW), Err(AdminError::Roster(_))));
    let bad = Op::Add(NewMember { did: "did:key:nope".into(), ..newcomer(43, "gina") });
    assert!(matches!(draft(&h, &bad, NOW), Err(AdminError::Roster(_))));
    // Removing twice, and rotating someone already retired.
    let once = draft(&h, &Op::Remove { did: t.did("erin"), voids_tombstones: false }, NOW).unwrap();
    let vh = VerifiedEpoch { epoch: once.epoch.clone(), hash: once.hash.clone() };
    assert_eq!(draft(&vh, &Op::Remove { did: t.did("erin"), voids_tombstones: false }, NOW), Err(AdminError::AlreadyRemoved));
    assert_eq!(draft(&vh, &Op::Rotate { old: t.did("erin"), new: key(44).did(), kind: Rotation::Routine }, NOW), Err(AdminError::NotActive));
}

#[test]
fn the_last_admin_cannot_be_removed() {
    let mut t = TestOrg::empty();
    let a = t.who("alice");
    let g = crate::testkit::EpochSpec { org: ORG.into(), store: STORE.into(), epoch: 1, prev_hash: None, members: vec![crate::testkit::MemberSpec::new(&a, "alice").admin()] };
    t.push_signed(g, &["alice"]);
    let r = draft(&head(&t), &Op::Remove { did: a.did(), voids_tombstones: false }, NOW);
    assert_eq!(r, Err(AdminError::Roster("no-active-admin")));
}

#[test]
fn removal_sets_the_epoch_and_never_clears_the_flag() {
    let t = TestOrg::three_admins();
    let d = draft(&head(&t), &Op::Remove { did: t.did("dave"), voids_tombstones: true }, NOW).unwrap();
    let m = d.epoch.member(&t.did("dave")).unwrap();
    assert!(m.status == Status::Removed && m.removed_at_epoch == Some(2) && m.voids_tombstones);
    verify_chain(ORG, STORE, &chain_with(&t, &d, &["alice", "bob"]), &t.pin_head()).unwrap();
}

#[test]
fn describe_lists_a_smuggled_role_grant_that_no_operation_makes() {
    let t = TestOrg::three_admins();
    let h = head(&t);
    // An attacker's proposal: a legitimate-looking add that also makes erin an admin, and renames dave.
    let mut e = draft(&h, &Op::Add(newcomer(40, "frank")), NOW).unwrap().epoch;
    let erin = t.did("erin");
    e.members.iter_mut().find(|m| m.did == erin).unwrap().roles = vec![Role::Member, Role::Admin];
    e.members.iter_mut().find(|m| m.did == t.did("dave")).unwrap().handle = "carol2".into();
    let (changes, warnings) = describe(&h.epoch, &e);
    assert!(changes.iter().any(|c| matches!(c, Change::Modified { what: "roles", .. })), "{changes:?}");
    assert!(changes.iter().any(|c| matches!(c, Change::Modified { what: "handle", .. })));
    assert!(warnings.iter().any(|w| matches!(w, Warning::GrantsAdmin(_))));
    assert!(warnings.iter().any(|w| matches!(w, Warning::ModifiesExisting(_))));
    assert!(warnings.iter().any(|w| matches!(w, Warning::AdminCountChanges { from: 3, to: 4 })));
}

#[test]
fn describe_flags_adding_an_admin_and_a_second_key_for_a_person() {
    let t = TestOrg::three_admins();
    let h = head(&t);
    let admin = NewMember { admin: true, ..newcomer(40, "frank") };
    let d = draft(&h, &Op::Add(admin), NOW).unwrap();
    let (_, w) = describe(&h.epoch, &d.epoch);
    assert!(w.iter().any(|x| matches!(x, Warning::GrantsAdmin(_))));
    let twin = NewMember { admin: true, ..newcomer(41, "alice") };
    let d = draft(&h, &Op::Add(twin), NOW).unwrap();
    let (_, w) = describe(&h.epoch, &d.epoch);
    assert!(w.iter().any(|x| matches!(x, Warning::SecondKeyForPerson(p) if p == "alice")));
    // Warnings print without the raw DID ambiguity: every line is single-line text.
    assert!(w.iter().all(|x| !x.line().contains('\n')));
}

#[test]
fn check_proposal_refuses_the_wrong_place_in_the_chain() {
    let t = TestOrg::three_admins();
    let h = head(&t);
    let good = draft(&h, &Op::Add(newcomer(40, "frank")), NOW).unwrap();
    assert!(check_proposal(&h, &good.bytes).is_ok());
    // The right bytes after the chain has moved on: epoch number is wrong.
    let h2 = VerifiedEpoch { epoch: good.epoch.clone(), hash: good.hash.clone() };
    assert!(matches!(check_proposal(&h2, &good.bytes), Err(AdminError::WrongEpoch { expected: 3, found: 2 })));
    // Another prev_hash (a fork of the same number).
    let mut forked = good.epoch.clone();
    forked.prev_hash = Some(crate::hash::of(b"elsewhere"));
    assert_eq!(check_proposal(&h, &serialize_epoch(&forked)).unwrap_err(), AdminError::WrongPrevHash);
    // Another store.
    let mut other = good.epoch.clone();
    other.store = "other-store".into();
    assert_eq!(check_proposal(&h, &serialize_epoch(&other)).unwrap_err(), AdminError::WrongScope);
    // Dropping an entry is a roster error, not a proposal.
    let mut dropped = good.epoch.clone();
    dropped.members.retain(|m| m.did != t.did("erin"));
    assert!(matches!(check_proposal(&h, &serialize_epoch(&dropped)), Err(AdminError::Roster(_))));
    // Not JSON at all.
    assert!(check_proposal(&h, b"{}").is_err());
}

#[test]
fn quorum_counts_people_over_exact_bytes_only() {
    let t = TestOrg::three_admins();
    let h = head(&t);
    let d = draft(&h, &Op::Add(newcomer(40, "frank")), NOW).unwrap();
    let sign = |who: &str, bytes: &[u8]| sign_epoch(&t.key(who), ORG, STORE, 2, bytes);
    // Nothing signed.
    let q = quorum(&h.epoch, 2, &d.bytes, &empty_sigs(2)).unwrap();
    assert_eq!((q.have, q.need, q.met()), (0, 2, false));
    // One signature, repeated: counts once. A non-admin's valid signature counts for nothing.
    let a = sign("alice", &d.bytes);
    let dave = sign("dave", &d.bytes);
    let sigs = merge_sigs(&h.epoch, 2, &d.bytes, None, Some(a.clone())).unwrap();
    let q = quorum(&h.epoch, 2, &d.bytes, &sigs).unwrap();
    assert_eq!((q.have, q.met()), (1, false));
    assert_eq!(q.signed, vec!["alice".to_string()]);
    let raw = crate::testkit::sigs_json(2, &[a.clone(), a.clone(), dave.clone()]);
    let q = quorum(&h.epoch, 2, &d.bytes, raw.as_bytes()).unwrap();
    assert_eq!((q.have, q.ignored), (1, 2));
    // A signature over other bytes (the attacker edited the proposal after alice signed).
    let mut edited = d.epoch.clone();
    edited.members.iter_mut().find(|m| m.did == t.did("erin")).unwrap().roles = vec![Role::Member, Role::Admin];
    let eb = serialize_epoch(&edited);
    let q = quorum(&h.epoch, 2, &eb, raw.as_bytes()).unwrap();
    assert_eq!(q.have, 0);
    // Bob's signature completes the real bytes.
    let both = crate::testkit::sigs_json(2, &[a, sign("bob", &d.bytes)]);
    assert!(quorum(&h.epoch, 2, &d.bytes, both.as_bytes()).unwrap().met());
    // A signature made for epoch 3 does not count for epoch 2.
    let wrong_n = sign_epoch(&t.key("bob"), ORG, STORE, 3, &d.bytes);
    let q = quorum(&h.epoch, 2, &d.bytes, crate::testkit::sigs_json(2, &[wrong_n]).as_bytes()).unwrap();
    assert_eq!(q.have, 0);
    // A sigs file that names another epoch counts for nothing at all.
    let q = quorum(&h.epoch, 2, &d.bytes, crate::testkit::sigs_json(7, &[sign("bob", &d.bytes)]).as_bytes()).unwrap();
    assert_eq!(q.have, 0);
}

#[test]
fn two_keys_of_one_person_count_once() {
    let mut t = TestOrg::three_admins();
    // alice gets a second admin key (same person), as an attacker holding both might.
    t.advance(&["alice", "bob"], |t, s| {
        let k = t.who("alice2");
        s.members.push(crate::testkit::MemberSpec::new(&k, "alice").admin());
    });
    let h = head(&t);
    let d = draft(&h, &Op::Add(newcomer(40, "frank")), NOW).unwrap();
    let s1 = sign_epoch(&t.key("alice"), ORG, STORE, 3, &d.bytes);
    let s2 = sign_epoch(&t.key("alice2"), ORG, STORE, 3, &d.bytes);
    let q = quorum(&h.epoch, 3, &d.bytes, crate::testkit::sigs_json(3, &[s1, s2]).as_bytes()).unwrap();
    assert_eq!((q.have, q.need), (1, 2));
}

#[test]
fn merge_sigs_drops_what_does_not_count_and_is_deterministic() {
    let t = TestOrg::three_admins();
    let h = head(&t);
    let d = draft(&h, &Op::Add(newcomer(40, "frank")), NOW).unwrap();
    let a = sign_epoch(&t.key("alice"), ORG, STORE, 2, &d.bytes);
    let b = sign_epoch(&t.key("bob"), ORG, STORE, 2, &d.bytes);
    let dave = sign_epoch(&t.key("dave"), ORG, STORE, 2, &d.bytes);
    let forged = (t.did("carol"), a.1.clone()); // carol's name on alice's signature
    let existing = crate::testkit::sigs_json(2, &[forged, dave, b.clone()]);
    let m1 = merge_sigs(&h.epoch, 2, &d.bytes, Some(existing.as_bytes()), Some(a.clone())).unwrap();
    let existing2 = crate::testkit::sigs_json(2, std::slice::from_ref(&a));
    let m2 = merge_sigs(&h.epoch, 2, &d.bytes, Some(existing2.as_bytes()), Some(b)).unwrap();
    assert_eq!(m1, m2, "same signatures, same bytes");
    let parsed = EpochSigs::parse(&m1).unwrap();
    assert_eq!(parsed.signatures.len(), 2);
    assert!(merge_sigs(&h.epoch, 2, &d.bytes, Some(b"not json"), None).is_err());
}

#[test]
fn a_signed_statement_is_authentic_for_the_epoch_it_names() {
    let mut t = TestOrg::three_admins();
    let d = draft(&head(&t), &Op::Add(newcomer(40, "frank")), NOW).unwrap();
    let anchor = t.pin_head();
    t.files.push(sign_bytes(ORG, STORE, 2, &d.bytes, &[&t.key("alice"), &t.key("bob")]));
    let r = verify_chain(ORG, STORE, &t.files, &anchor).unwrap();
    let head = "1".repeat(40);
    let (st, sg) = sign_statement(&t.key("carol"), ORG, STORE, 2, &d.hash, &head, 1_700_000_000, 7 * 86400);
    let a = freshness::authenticate(ORG, STORE, &st, &sg, &r).unwrap();
    assert_eq!(a.statement.store_head, head);
    // A signature over other statement bytes does not authenticate.
    let (st2, _) = sign_statement(&t.key("carol"), ORG, STORE, 2, &d.hash, &"2".repeat(40), 1_700_000_000, 7 * 86400);
    assert!(freshness::authenticate(ORG, STORE, &st2, &sg, &r).is_err());
}

#[test]
fn genesis_is_valid_only_with_every_founder_signature() {
    let mut t = TestOrg::empty();
    let ks: Vec<_> = ["a", "b", "c"].iter().map(|n| t.who(n)).collect();
    let founders: Vec<NewMember> = ks.iter().zip(["ada", "ben", "cy"]).map(|(k, n)| NewMember { did: k.did(), person: n.into(), handle: n.into(), admin: true, steward_areas: vec![] }).collect();
    let g = genesis(ORG, STORE, &founders, NOW).unwrap();
    assert_eq!((g.epoch.epoch, g.epoch.prev_hash.clone()), (1, None));
    assert_eq!(serialize_epoch(&g.epoch), g.bytes);
    let sigs = |n: usize| genesis_sigs(&ks[..n].iter().map(|k| sign_epoch(k, ORG, STORE, 1, &g.bytes)).collect::<Vec<_>>());
    assert!(verify_genesis(ORG, STORE, &g.bytes, &sigs(3)).is_ok());
    assert_eq!(verify_genesis(ORG, STORE, &g.bytes, &sigs(2)).unwrap_err().reason(), "genesis-not-fully-signed");
    // Signatures over other bytes do not count.
    let mut other = g.epoch.clone();
    other.members[0].handle = "adaa".into();
    let ob = serialize_epoch(&other);
    assert!(verify_genesis(ORG, STORE, &ob, &sigs(3)).is_err());
    // No admin among the founders is not a store.
    let none = vec![NewMember { admin: false, ..founders[0].clone() }];
    assert_eq!(genesis(ORG, STORE, &none, NOW).unwrap_err(), AdminError::Roster("no-active-admin"));
    // The same founder twice is a bad roster.
    let dup = vec![founders[0].clone(), founders[0].clone()];
    assert!(matches!(genesis(ORG, STORE, &dup, NOW), Err(AdminError::Roster(_))));
}
