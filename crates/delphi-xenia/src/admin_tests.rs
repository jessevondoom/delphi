//! Tests for the pure admin rules. Every epoch here is judged by the real verifier (`roster::verify_chain`,
//! `check_transition`, `freshness::authenticate`); a test that builds an attack builds it as bytes, the way a
//! hostile proposal branch would carry it.

use super::*;
use crate::roster::{Anchor, EpochFile, RosterError};
use crate::testkit::{key, EpochSpec, MemberSpec, TestOrg, ORG, STORE};

const NOW: &str = "2026-10-26T12:00:00Z";

fn head_of(t: &TestOrg) -> (Epoch, String) {
    let f = t.files.last().unwrap();
    (Epoch::parse(&f.epoch_json).unwrap(), hash::of(&f.epoch_json))
}

fn add(t: &mut TestOrg, name: &str, person: &str, admin: bool, areas: &[&str]) -> Operation {
    let id = t.who(name);
    Operation::Add(NewMember { did: id.did(), person: person.into(), handle: person.into(), admin, steward_areas: areas.iter().map(|s| s.to_string()).collect() })
}

/// Chain verification the way a join or a sync does it: against the head pair of the newest epoch.
fn verify(files: &[EpochFile], head: &Proposed) -> Result<roster::VerifiedRoster, RosterError> {
    roster::verify_chain(ORG, STORE, files, &Anchor::new([(head.epoch.epoch, head.hash.clone())]).unwrap())
}

fn signed_file(t: &TestOrg, p: &Proposed, signers: &[&str]) -> EpochFile {
    let (prev, _) = head_of(t);
    let sigs: Vec<ProposalSig> = signers.iter().map(|s| sign_epoch(&t.key(s), ORG, STORE, p.epoch.epoch, &p.bytes).unwrap()).collect();
    EpochFile { epoch_json: p.bytes.clone(), sigs_json: assemble_sigs(ORG, STORE, p.epoch.epoch, &p.bytes, &prev, &sigs) }
}

#[test]
fn add_builds_an_epoch_the_chain_verifier_accepts_when_a_quorum_signed_it() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let op = add(&mut t, "gail", "gail", false, &["payments"]);
    let p = build_next(&head, &hh, &op, NOW, &Policy::default()).unwrap();
    assert_eq!(p.epoch.epoch, 2);
    assert_eq!(p.epoch.prev_hash.as_deref(), Some(hh.as_str()));
    let gail = p.epoch.member(&t.did("gail")).unwrap();
    assert!(gail.is_active() && !gail.is_admin());
    assert_eq!(gail.added_at, NOW);
    let mut files = t.files.clone();
    files.push(signed_file(&t, &p, &["alice", "bob"]));
    let r = verify(&files, &p).unwrap();
    assert_eq!(r.head_epoch(), 2);
}

#[test]
fn one_of_three_is_not_a_quorum() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let op = add(&mut t, "gail", "gail", false, &[]);
    let p = build_next(&head, &hh, &op, NOW, &Policy::default()).unwrap();
    let mut files = t.files.clone();
    files.push(signed_file(&t, &p, &["alice"]));
    assert_eq!(verify(&files, &p), Err(RosterError::QuorumNotMet { epoch: 2, have: 1, need: 2 }));
}

#[test]
fn add_refuses_a_listed_key_a_listed_person_and_a_bad_key() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let policy = Policy::default();
    let dave = Operation::Add(NewMember { did: t.did("dave"), person: "someone".into(), handle: "x".into(), admin: false, steward_areas: vec![] });
    assert_eq!(build_next(&head, &hh, &dave, NOW, &policy).unwrap_err(), Refusal::AlreadyListed);
    let new_key_old_person = add(&mut t, "mallory", "alice", false, &[]);
    assert_eq!(build_next(&head, &hh, &new_key_old_person, NOW, &policy).unwrap_err(), Refusal::PersonExists);
    let bad = Operation::Add(NewMember { did: "did:key:nope".into(), person: "z".into(), handle: "z".into(), admin: false, steward_areas: vec![] });
    assert_eq!(build_next(&head, &hh, &bad, NOW, &policy).unwrap_err().reason(), "bad-input");
    // The parser's own alphabet rules apply to what we build, too.
    let loud = Operation::Add(NewMember { did: key(99).did(), person: "Z Z".into(), handle: "z".into(), admin: false, steward_areas: vec![] });
    assert_eq!(build_next(&head, &hh, &loud, NOW, &policy).unwrap_err().reason(), "bad-epoch");
    assert_eq!(build_next(&head, &hh, &add(&mut t, "z", "z", false, &[]), "yesterday", &policy).unwrap_err().reason(), "bad-input");
}

#[test]
fn remove_marks_the_entry_removed_in_this_epoch_and_for_cause_voids_its_withdrawals() {
    let t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let plain = build_next(&head, &hh, &Operation::Remove { did: t.did("erin"), for_cause: false }, NOW, &Policy::default()).unwrap();
    let erin = plain.epoch.member(&t.did("erin")).unwrap();
    assert_eq!((erin.status, erin.removed_at_epoch, erin.voids_tombstones), (Status::Removed, Some(2), false));
    let cause = build_next(&head, &hh, &Operation::Remove { did: t.did("erin"), for_cause: true }, NOW, &Policy::default()).unwrap();
    assert!(cause.epoch.member(&t.did("erin")).unwrap().voids_tombstones);
    let d = diff(Some(&head), &cause.epoch);
    assert!(d.flags.contains(&Flag::VoidsTombstones));
    assert_eq!(build_next(&head, &hh, &Operation::Remove { did: key(77).did(), for_cause: false }, NOW, &Policy::default()).unwrap_err(), Refusal::UnknownMember);
}

#[test]
fn a_removed_entry_cannot_be_removed_twice() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let p = build_next(&head, &hh, &Operation::Remove { did: t.did("erin"), for_cause: false }, NOW, &Policy::default()).unwrap();
    t.files.push(signed_file(&t, &p, &["alice", "bob"]));
    let (head2, hh2) = head_of(&t);
    assert_eq!(build_next(&head2, &hh2, &Operation::Remove { did: t.did("erin"), for_cause: false }, NOW, &Policy::default()).unwrap_err(), Refusal::NotActive);
}

/// One admin (alice), one member (erin): removing alice would leave the roster with no admin.
fn lone_admin() -> TestOrg {
    let mut t = TestOrg::empty();
    let (a, e) = (t.who("alice"), t.who("erin"));
    let g = EpochSpec { org: ORG.into(), store: STORE.into(), epoch: 1, prev_hash: None, members: vec![MemberSpec::new(&a, "alice").admin(), MemberSpec::new(&e, "erin")] };
    t.push_signed(g, &["alice"]);
    t
}

#[test]
fn the_last_admin_cannot_be_removed_even_with_the_policy_lowered() {
    let t = lone_admin();
    let (head, hh) = head_of(&t);
    for min in [0, 1, 2] {
        let e = build_next(&head, &hh, &Operation::Remove { did: t.did("alice"), for_cause: false }, NOW, &Policy { min_admins: min }).unwrap_err();
        assert_eq!(e, Refusal::LastAdmin, "min_admins = {min}");
    }
}

#[test]
fn removing_an_admin_below_the_minimum_is_refused_but_a_removal_that_keeps_it_is_not() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let p = build_next(&head, &hh, &Operation::Remove { did: t.did("carol"), for_cause: false }, NOW, &Policy::default()).unwrap();
    assert_eq!(p.epoch.admin_people().len(), 2);
    t.files.push(signed_file(&t, &p, &["alice", "bob"]));
    let (head2, hh2) = head_of(&t);
    let e = build_next(&head2, &hh2, &Operation::Remove { did: t.did("bob"), for_cause: false }, NOW, &Policy::default()).unwrap_err();
    assert_eq!(e, Refusal::BelowMinimumAdmins { after: 1, min: 2 });
    // An org that decides to go down to one says so, explicitly.
    assert!(build_next(&head2, &hh2, &Operation::Remove { did: t.did("bob"), for_cause: false }, NOW, &Policy { min_admins: 1 }).is_ok());
}

#[test]
fn a_roster_already_below_the_minimum_can_still_grow() {
    let mut t = lone_admin();
    let (head, hh) = head_of(&t);
    let op = add(&mut t, "bob", "bob", true, &[]);
    assert!(build_next(&head, &hh, &op, NOW, &Policy::default()).is_ok());
}

#[test]
fn routine_rotation_retires_the_old_key_with_a_successor_of_the_same_person_and_authority() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let new = t.who("dave2");
    let op = Operation::Rotate { old_did: t.did("dave"), new_did: new.did(), kind: Rotation::Routine };
    let p = build_next(&head, &hh, &op, NOW, &Policy::default()).unwrap();
    let (old, fresh) = (p.epoch.member(&t.did("dave")).unwrap(), p.epoch.member(&new.did()).unwrap());
    assert_eq!((old.status, old.successor.as_deref(), old.voids_tombstones), (Status::Retired, Some(new.did().as_str()), false));
    assert_eq!((fresh.person.as_str(), fresh.status, &fresh.roles, &fresh.steward_areas), (old.person.as_str(), Status::Active, &old.roles, &old.steward_areas));
    let mut files = t.files.clone();
    files.push(signed_file(&t, &p, &["alice", "carol"]));
    let r = verify(&files, &p).unwrap();
    // X5 and X7 rely on exactly this chain.
    assert_eq!(r.successor_chain(&t.did("dave")), vec![t.did("dave"), new.did()]);
    assert!(!r.is_active(&t.did("dave")) && r.is_active(&new.did()));
    let d = diff(Some(&head), &p.epoch);
    assert_eq!(d.reminders, vec![Reminder::RepublishResign]);
    assert!(!d.flags.contains(&Flag::KeyForExistingPerson), "a rotation's successor is not a second key: {:?}", d.flags);
}

#[test]
fn compromise_rotation_forces_voids_tombstones_and_says_what_happens_to_the_old_keys_records() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let new = t.who("dave2");
    let op = Operation::Rotate { old_did: t.did("dave"), new_did: new.did(), kind: Rotation::Compromised };
    let p = build_next(&head, &hh, &op, NOW, &Policy::default()).unwrap();
    assert!(p.epoch.member(&t.did("dave")).unwrap().voids_tombstones);
    assert!(!p.epoch.member(&new.did()).unwrap().voids_tombstones);
    let d = diff(Some(&head), &p.epoch);
    assert_eq!(d.reminders, vec![Reminder::CompromisedKeyRecords]);
    assert!(d.flags.contains(&Flag::VoidsTombstones));
    assert!(Reminder::CompromisedKeyRecords.text().contains("republish --resign` refuses a compromise rotation"));
}

#[test]
fn a_rotation_of_an_admin_keeps_the_admin_count_and_the_person_counts_once() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let new = t.who("alice2");
    let op = Operation::Rotate { old_did: t.did("alice"), new_did: new.did(), kind: Rotation::Compromised };
    let p = build_next(&head, &hh, &op, NOW, &Policy::default()).unwrap();
    assert_eq!(p.epoch.admin_people().len(), 3);
    assert!(p.epoch.member(&new.did()).unwrap().is_admin());
}

#[test]
fn rotation_refusals() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let new = t.who("x");
    let pol = Policy::default();
    let same = Operation::Rotate { old_did: t.did("dave"), new_did: t.did("dave"), kind: Rotation::Routine };
    assert_eq!(build_next(&head, &hh, &same, NOW, &pol).unwrap_err(), Refusal::SameKey);
    let to_listed = Operation::Rotate { old_did: t.did("dave"), new_did: t.did("erin"), kind: Rotation::Routine };
    assert_eq!(build_next(&head, &hh, &to_listed, NOW, &pol).unwrap_err(), Refusal::AlreadyListed);
    let unknown = Operation::Rotate { old_did: key(88).did(), new_did: new.did(), kind: Rotation::Routine };
    assert_eq!(build_next(&head, &hh, &unknown, NOW, &pol).unwrap_err(), Refusal::UnknownMember);
    // A retired key cannot be rotated again.
    let p = build_next(&head, &hh, &Operation::Rotate { old_did: t.did("dave"), new_did: new.did(), kind: Rotation::Routine }, NOW, &pol).unwrap();
    t.files.push(signed_file(&t, &p, &["alice", "bob"]));
    let (head2, hh2) = head_of(&t);
    let again = Operation::Rotate { old_did: t.did("dave"), new_did: key(66).did(), kind: Rotation::Routine };
    assert_eq!(build_next(&head2, &hh2, &again, NOW, &pol).unwrap_err(), Refusal::NotActive);
}

// ----- hostile candidates --------------------------------------------------------------------------------

fn hostile(t: &TestOrg, edit: impl FnOnce(&mut EpochSpec)) -> Vec<u8> {
    let mut spec = t.draft();
    edit(&mut spec);
    spec.to_json().into_bytes()
}

fn judge(t: &TestOrg, bytes: &[u8]) -> Result<Epoch, Refusal> {
    let (head, hh) = head_of(t);
    check_candidate(&head, &hh, ORG, STORE, bytes, &Policy::default())
}

#[test]
fn a_proposal_that_changes_person_or_added_at_is_refused() {
    let t = TestOrg::three_admins();
    let person = hostile(&t, |s| s.members[3].person = "alice".into());
    assert_eq!(judge(&t, &person).unwrap_err(), Refusal::Roster(RosterError::PersonChanged { epoch: 2 }));
    let when = hostile(&t, |s| s.members[3].added_at = "2020-01-01T00:00:00Z".into());
    assert_eq!(judge(&t, &when).unwrap_err(), Refusal::Roster(RosterError::ImmutableChanged { epoch: 2, field: "added_at" }));
    let dropped = hostile(&t, |s| {
        s.members.pop();
    });
    assert_eq!(judge(&t, &dropped).unwrap_err(), Refusal::Roster(RosterError::Dropped { epoch: 2 }));
}

#[test]
fn a_proposal_for_the_wrong_epoch_scope_or_base_is_refused() {
    let t = TestOrg::three_admins();
    assert_eq!(judge(&t, &hostile(&t, |s| s.epoch = 3)).unwrap_err(), Refusal::NotNextEpoch { expected: 2, found: 3 });
    assert_eq!(
        judge(&t, &hostile(&t, |s| {
            s.epoch = 1;
            s.prev_hash = None;
        }))
        .unwrap_err(),
        Refusal::NotNextEpoch { expected: 2, found: 1 }
    );
    assert_eq!(judge(&t, &hostile(&t, |s| s.org = "other".into())).unwrap_err(), Refusal::WrongScope);
    assert_eq!(judge(&t, &hostile(&t, |s| s.prev_hash = Some(hash::of(b"some other epoch")))).unwrap_err(), Refusal::StaleBase);
    assert_eq!(judge(&t, &hostile(&t, |s| s.prev_hash = None)).unwrap_err().reason(), "bad-epoch");
    assert_eq!(judge(&t, b"{\"not\":\"an epoch\"}").unwrap_err().reason(), "bad-epoch");
}

#[test]
fn a_hidden_admin_passes_the_verifier_but_not_unnoticed() {
    let mut t = TestOrg::three_admins();
    let (head, _) = head_of(&t);
    let mallory = t.who("mallory");
    // "Add a member" plus a second, admin entry nobody asked for, and an existing member quietly promoted.
    let bytes = hostile(&t, |s| {
        s.members.push(MemberSpec::new(&key(70), "newbie"));
        s.members.push(MemberSpec::new(&mallory, "mallory").admin());
        s.members[4].roles = vec!["member", "admin"]; // erin
    });
    let e = judge(&t, &bytes).expect("the verifier alone cannot tell a hidden admin from a wanted one");
    let d = diff(Some(&head), &e);
    assert!(d.flags.contains(&Flag::AddsAdmin) && d.flags.contains(&Flag::GainsAdmin), "{:?}", d.flags);
    assert!(d.changes.iter().any(|c| c.kind == ChangeKind::Added && c.person == "mallory" && c.detail.contains("admin")));
    assert!(d.changes.iter().any(|c| c.kind == ChangeKind::RolesChanged && c.person == "erin"));
}

#[test]
fn a_second_key_for_an_existing_person_and_widened_areas_are_flagged() {
    let t = TestOrg::three_admins();
    let (head, _) = head_of(&t);
    let bytes = hostile(&t, |s| {
        s.members.push(MemberSpec::new(&key(71), "alice"));
        s.members[3].steward_areas = vec!["*".into()]; // dave: rust -> everything
    });
    let d = diff(Some(&head), &judge(&t, &bytes).unwrap());
    assert!(d.flags.contains(&Flag::KeyForExistingPerson) && d.flags.contains(&Flag::WidensAreas), "{:?}", d.flags);
}

#[test]
fn a_proposal_that_empties_the_admins_or_drops_below_the_minimum_is_refused_with_its_own_reason() {
    let t = TestOrg::three_admins();
    let none = hostile(&t, |s| {
        for m in s.members.iter_mut().take(3) {
            m.status = "retired";
        }
    });
    assert_eq!(judge(&t, &none).unwrap_err(), Refusal::LastAdmin);
    let one = hostile(&t, |s| {
        for m in s.members.iter_mut().take(2) {
            m.status = "retired";
        }
    });
    assert_eq!(judge(&t, &one).unwrap_err(), Refusal::BelowMinimumAdmins { after: 1, min: 2 });
}

#[test]
fn genesis_drafts_are_judged_before_a_founder_signs() {
    let founders = |n: usize| -> Vec<NewMember> { (0..n).map(|i| NewMember { did: key(i as u8 + 1).did(), person: format!("p{i}"), handle: format!("p{i}"), admin: true, steward_areas: vec![] }).collect() };
    let g = build_genesis(ORG, STORE, &founders(3), NOW).unwrap();
    assert_eq!(g.epoch.epoch, 1);
    assert_eq!(check_genesis_draft(ORG, STORE, &g.bytes).unwrap(), g.epoch);
    assert_eq!(check_genesis_draft("other", STORE, &g.bytes).unwrap_err(), Refusal::WrongScope);
    let no_admin: Vec<NewMember> = founders(2).into_iter().map(|m| NewMember { admin: false, ..m }).collect();
    assert_eq!(build_genesis(ORG, STORE, &no_admin, NOW).unwrap_err(), Refusal::LastAdmin);
    // A genesis with a retired entry, or one that is not epoch 1, is not a genesis.
    let t = TestOrg::three_admins();
    let second = hostile(&t, |_| {});
    assert_eq!(check_genesis_draft(ORG, STORE, &second).unwrap_err(), Refusal::NotNextEpoch { expected: 1, found: 2 });
    let mut spec = t.specs[0].clone();
    spec.members[4].status = "retired";
    assert_eq!(check_genesis_draft(ORG, STORE, spec.to_json().as_bytes()).unwrap_err().reason(), "bad-new-entry");
}

// ----- signatures ----------------------------------------------------------------------------------------

#[test]
fn only_valid_signatures_by_active_admins_over_these_bytes_are_assembled() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let op = add(&mut t, "gail", "gail", false, &[]);
    let p = build_next(&head, &hh, &op, NOW, &Policy::default()).unwrap();
    let other = build_next(&head, &hh, &add(&mut t, "hank", "hank", false, &[]), NOW, &Policy::default()).unwrap();
    let good = sign_epoch(&t.key("alice"), ORG, STORE, 2, &p.bytes).unwrap();
    let candidates = vec![
        good.clone(),
        good.clone(),                                                       // a duplicate
        sign_epoch(&t.key("bob"), ORG, STORE, 2, &other.bytes).unwrap(),   // over different bytes
        sign_epoch(&t.key("erin"), ORG, STORE, 2, &p.bytes).unwrap(),      // a member, not an admin
        sign_epoch(&key(90), ORG, STORE, 2, &p.bytes).unwrap(),            // a stranger
        sign_epoch(&t.key("carol"), ORG, STORE, 3, &p.bytes).unwrap(),     // wrong epoch number in the domain
        sign_epoch(&t.key("carol"), "other", STORE, 2, &p.bytes).unwrap(), // wrong org
        ProposalSig { kid: t.did("carol"), sig: good.sig.clone() },         // someone else's signature under carol's name
    ];
    let file = assemble_sigs(ORG, STORE, 2, &p.bytes, &head, &candidates);
    let parsed = roster::EpochSigs::parse(&file).unwrap();
    assert_eq!(parsed.epoch, 2);
    assert_eq!(parsed.signatures.len(), 1);
    assert_eq!(parsed.signatures[0].0, t.did("alice"));
}

#[test]
fn two_keys_of_one_person_count_once() {
    // alice holds two active admin keys; she and bob are the whole of a 3-person roster with carol silent.
    let mut t = TestOrg::empty();
    let (a1, a2, b, c) = (t.who("alice"), t.who("alice2"), t.who("bob"), t.who("carol"));
    let g = EpochSpec {
        org: ORG.into(),
        store: STORE.into(),
        epoch: 1,
        prev_hash: None,
        members: vec![MemberSpec::new(&a1, "alice").admin(), MemberSpec::new(&a2, "alice").admin(), MemberSpec::new(&b, "bob").admin(), MemberSpec::new(&c, "carol").admin()],
    };
    t.push_signed(g, &["alice", "alice2", "bob", "carol"]);
    let (head, hh) = head_of(&t);
    assert_eq!(head.admin_people().len(), 3);
    let p = build_next(&head, &hh, &add(&mut t, "erin", "erin", false, &[]), NOW, &Policy::default()).unwrap();
    let mut files = t.files.clone();
    files.push(signed_file(&t, &p, &["alice", "alice2"]));
    assert_eq!(verify(&files, &p), Err(RosterError::QuorumNotMet { epoch: 2, have: 1, need: 2 }));
    files.pop();
    files.push(signed_file(&t, &p, &["alice", "alice2", "bob"]));
    assert!(verify(&files, &p).is_ok());
}

#[test]
fn after_a_rotation_the_old_key_signs_nothing_that_counts() {
    let mut t = TestOrg::three_admins();
    let (head, hh) = head_of(&t);
    let new = t.who("alice2");
    let p = build_next(&head, &hh, &Operation::Rotate { old_did: t.did("alice"), new_did: new.did(), kind: Rotation::Compromised }, NOW, &Policy::default()).unwrap();
    t.files.push(signed_file(&t, &p, &["bob", "carol"]));
    let (head2, hh2) = head_of(&t);
    let next = build_next(&head2, &hh2, &add(&mut t, "gail", "gail", false, &[]), NOW, &Policy::default()).unwrap();
    // The thief has the old alice key and one more signature: the old key is ignored, so one person signed.
    let mut thief = t.files.clone();
    thief.push(signed_file(&t, &next, &["alice", "bob"]));
    assert_eq!(verify(&thief, &next), Err(RosterError::QuorumNotMet { epoch: 3, have: 1, need: 2 }));
    let mut honest = t.files.clone();
    honest.push(signed_file(&t, &next, &["alice2", "bob"]));
    assert!(verify(&honest, &next).is_ok());
}

#[test]
fn proposal_signature_files_are_strict() {
    let t = TestOrg::three_admins();
    let s = sign_epoch(&t.key("alice"), ORG, STORE, 2, b"x").unwrap();
    assert_eq!(ProposalSig::parse(&s.to_json()), Ok(s.clone()));
    assert!(ProposalSig::parse(b"{\"kid\":\"x\",\"sig\":\"y\"}").is_err());
    assert!(ProposalSig::parse(format!("{{\"kid\":\"{}\",\"sig\":\"{}\",\"extra\":1}}", s.kid, s.sig).as_bytes()).is_err());
    assert!(ProposalSig::parse(&vec![b' '; MAX_SIG_FILE_BYTES + 1]).is_err());
    let path = proposal_sig_path(2, &s.kid).unwrap();
    assert!(path.starts_with("roster/proposals/2/") && path.ends_with(".json") && path.len() == "roster/proposals/2/".len() + 64 + 5);
    assert_eq!(proposal_branch(7), "roster-proposal/7");
}

// ----- statements ----------------------------------------------------------------------------------------

#[test]
fn a_statement_reads_back_as_written_and_is_authentic_for_its_roster_only() {
    let t = TestOrg::three_admins();
    let roster = t.verify().unwrap();
    let head = "a".repeat(40);
    let cap = freshness::Policy::default();
    let now = crate::time::parse_utc(NOW).unwrap();
    let bytes = build_statement(ORG, STORE, 1, roster.head_hash(), &head, now, DEFAULT_VALIDITY_SECS, &cap).unwrap();
    let sigs = sign_statement(&t.key("alice"), ORG, STORE, &bytes).unwrap();
    let a = freshness::authenticate(ORG, STORE, &bytes, &sigs, &roster).unwrap();
    assert_eq!((a.statement.store_head.as_str(), a.needed, a.signing_people), (head.as_str(), 1, 1));
    // A member who is not an admin signs nothing that counts.
    let by_member = sign_statement(&t.key("erin"), ORG, STORE, &bytes).unwrap();
    assert_eq!(freshness::authenticate(ORG, STORE, &bytes, &by_member, &roster).unwrap_err(), "no-admin-signature");
    // A statement signed for other bytes does not carry over.
    let other = build_statement(ORG, STORE, 1, roster.head_hash(), &"b".repeat(40), now, DEFAULT_VALIDITY_SECS, &cap).unwrap();
    assert!(freshness::authenticate(ORG, STORE, &other, &sigs, &roster).is_err());
}

#[test]
fn a_statement_longer_than_the_verifiers_cap_is_never_written() {
    let t = TestOrg::three_admins();
    let roster = t.verify().unwrap();
    let cap = freshness::Policy::default();
    let go = |v: i64| build_statement(ORG, STORE, 1, roster.head_hash(), &"a".repeat(40), 1_800_000_000, v, &cap);
    assert!(go(cap.max_validity_secs).is_ok());
    assert_eq!(go(cap.max_validity_secs + 1).unwrap_err().reason(), "bad-validity");
    assert_eq!(go(0).unwrap_err().reason(), "bad-validity");
    assert_eq!(go(-5).unwrap_err().reason(), "bad-validity");
    assert!(build_statement(ORG, STORE, 1, roster.head_hash(), "not a commit", 1_800_000_000, 60, &cap).is_err());
    assert!(build_statement(ORG, STORE, 1, "not a hash", &"a".repeat(40), 1_800_000_000, 60, &cap).is_err());
}

#[test]
fn the_digest_moves_with_every_part() {
    let a = digest(&[("epoch", b"one"), ("sigs", b"two")]);
    assert_ne!(a, digest(&[("epoch", b"one"), ("sigs", b"twO")]));
    assert_ne!(a, digest(&[("epoch", b"one"), ("sig", b"stwo")]), "the split between label and bytes is part of the digest");
    assert_ne!(a, digest(&[("epoch", b"onetwo")]));
    assert_ne!(digest(&[("a", b"b")]), digest(&[("ab", b"")]));
}
