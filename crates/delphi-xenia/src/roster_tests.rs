//! The roster chain, played by the attacker (spec 011 §10 tests 3, 4, 29; spec 012 tests 13, 14).
//! Tests that a lenient reading would pass say so with an assertion on the premise.

use super::*;
use crate::roster;
use crate::testkit::{self, sign_bytes, EpochSpec, MemberSpec, TestOrg, ORG, STORE, TS};
use serde_json::Value;

/// A four-epoch chain: 1 genesis (alice, bob, carol admins; dave steward; erin member), 2 adds frank,
/// 3 rotates dave to dave2, 4 removes erin.
fn org4() -> TestOrg {
    let mut t = TestOrg::three_admins();
    let frank = t.who("frank");
    t.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    t.advance(&["bob", "carol"], |t, s| t.rotate(s, "dave", "dave2", false));
    t.advance(&["alice", "carol"], |t, s| {
        let erin = t.key("erin");
        let m = s.member_mut(&erin);
        m.status = "removed";
        m.removed_at_epoch = Some(4);
    });
    t
}

fn verified(t: &TestOrg) -> RosterError {
    t.verify().expect_err("expected the chain to be refused")
}

#[test]
fn a_valid_four_epoch_chain_verifies_and_answers_standing_questions() {
    let t = org4();
    let r = t.verify().unwrap();
    assert_eq!(r.head_epoch(), 4);
    assert_eq!(r.head_hash(), t.head_spec().hash());
    assert!(r.is_active(&t.did("alice")));
    assert!(r.is_active(&t.did("dave2")));
    assert!(!r.is_active(&t.did("dave")), "retired has no standing");
    assert!(!r.is_active(&t.did("erin")), "removed has no standing");
    assert!(!r.is_active(&testkit::key(99).did()), "a stranger has none either");
    assert_eq!(r.person_of(&t.did("dave2")), Some("dave"));
    assert_eq!(r.successor_chain(&t.did("dave")), vec![t.did("dave"), t.did("dave2")]);
    assert_eq!(r.epoch(2).unwrap().epoch.epoch, 2);
    assert!(r.epoch(0).is_none() && r.epoch(5).is_none());
    // Verifying twice, and with the whole chain pinned, agrees.
    assert_eq!(roster::verify_chain(ORG, STORE, &t.files, &t.anchor_all()).unwrap(), r);
}

#[test]
fn an_install_pinned_at_epoch_one_verifies_the_store_at_epoch_four() {
    // Spec 011 test 4: pinned at 1, store at 4: 1 -> 2 -> 3 -> 4.
    let t = org4();
    let pinned_at_1 = Anchor::new([(1, t.specs[0].hash())]).unwrap();
    assert_eq!(roster::verify_chain(ORG, STORE, &t.files, &pinned_at_1).unwrap().head_epoch(), 4);
    // A missing link, or short signatures, refuse the target (the caller keeps its last good epoch).
    let mut missing = t.files.clone();
    missing.remove(1);
    assert!(matches!(roster::verify_chain(ORG, STORE, &missing, &pinned_at_1), Err(RosterError::Gap { expected: 2, found: 3 })));
    let mut short = t.clone();
    short.files[2] = t.specs[2].sign(&[&t.key("bob")]);
    assert!(matches!(short.verify(), Err(RosterError::QuorumNotMet { epoch: 3, have: 1, need: 2 })));
    // The prefix that is still good verifies on its own.
    assert_eq!(short.upto(2).verify().unwrap().head_epoch(), 2);
}

// ---- strict parsing ---------------------------------------------------------------------------------------

fn genesis_json() -> String {
    TestOrg::three_admins().specs[0].to_json()
}

fn edited(f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(&genesis_json()).unwrap();
    f(&mut v);
    serde_json::to_string_pretty(&v).unwrap()
}

#[test]
fn the_untouched_genesis_parses() {
    let e = Epoch::parse(genesis_json().as_bytes()).unwrap();
    assert_eq!(e.epoch, 1);
    assert!(e.prev_hash.is_none());
    assert_eq!(e.members.len(), 5);
    assert_eq!(e.admin_people().len(), 3);
}

#[test]
fn malformed_epoch_files_are_refused() {
    type Mutation = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Mutation)> = vec![
        ("unknown top-level key", Box::new(|v| v["extra"] = Value::from(1))),
        ("unknown member key", Box::new(|v| v["members"][0]["extra"] = Value::from(1))),
        ("epoch as a string", Box::new(|v| v["epoch"] = Value::from("1"))),
        ("epoch as a float", Box::new(|v| v["epoch"] = Value::from(1.5))),
        ("negative epoch", Box::new(|v| v["epoch"] = Value::from(-1))),
        ("epoch zero", Box::new(|v| v["epoch"] = Value::from(0))),
        ("roles as a string", Box::new(|v| v["members"][0]["roles"] = Value::from("admin"))),
        ("no roles", Box::new(|v| v["members"][0]["roles"] = Value::from(Vec::<String>::new()))),
        ("unknown role", Box::new(|v| v["members"][0]["roles"] = Value::from(vec!["root"]))),
        ("role twice", Box::new(|v| v["members"][0]["roles"] = Value::from(vec!["admin", "admin"]))),
        ("null successor", Box::new(|v| v["members"][0]["successor"] = Value::Null)),
        ("null removed_at_epoch", Box::new(|v| v["members"][0]["removed_at_epoch"] = Value::Null)),
        ("missing prev_hash", Box::new(|v| {
            v.as_object_mut().unwrap().remove("prev_hash");
        })),
        ("genesis with a prev_hash", Box::new(|v| v["prev_hash"] = Value::from(crate::hash::of(b"x")))),
        ("missing person", Box::new(|v| {
            v["members"][0].as_object_mut().unwrap().remove("person");
        })),
        ("person with capitals", Box::new(|v| v["members"][0]["person"] = Value::from("Alice"))),
        ("person with a newline", Box::new(|v| v["members"][0]["person"] = Value::from("al\nice"))),
        ("handle with a space", Box::new(|v| v["members"][0]["handle"] = Value::from("a b"))),
        ("handle with a terminal escape", Box::new(|v| v["members"][0]["handle"] = Value::from("a\u{1b}[2J"))),
        ("empty handle", Box::new(|v| v["members"][0]["handle"] = Value::from(""))),
        ("wildcard mixed with a tag", Box::new(|v| v["members"][3]["steward_areas"] = Value::from(vec!["*", "rust"]))),
        ("area with capitals", Box::new(|v| v["members"][3]["steward_areas"] = Value::from(vec!["Rust"]))),
        ("area twice", Box::new(|v| v["members"][3]["steward_areas"] = Value::from(vec!["rust", "rust"]))),
        ("status in capitals", Box::new(|v| v["members"][0]["status"] = Value::from("Active"))),
        ("did:web", Box::new(|v| v["members"][0]["did"] = Value::from("did:web:example.com"))),
        ("timestamp with an offset", Box::new(|v| v["members"][0]["added_at"] = Value::from("2026-10-24T09:00:00+00:00"))),
        ("successor on an active entry", Box::new(|v| v["members"][0]["successor"] = Value::from(testkit::key(1).did()))),
        ("removed_at_epoch on an active entry", Box::new(|v| v["members"][0]["removed_at_epoch"] = Value::from(1))),
        ("voids_tombstones on an active entry", Box::new(|v| v["members"][0]["voids_tombstones"] = Value::from(true))),
        ("removed without removed_at_epoch", Box::new(|v| v["members"][4]["status"] = Value::from("removed"))),
        ("removed in the future", Box::new(|v| {
            v["members"][4]["status"] = Value::from("removed");
            v["members"][4]["removed_at_epoch"] = Value::from(2);
        })),
        ("no members", Box::new(|v| v["members"] = Value::from(Vec::<Value>::new()))),
        ("a DID listed twice", Box::new(|v| {
            let first = v["members"][0].clone();
            v["members"].as_array_mut().unwrap().push(first);
        })),
        ("org with a newline", Box::new(|v| v["org"] = Value::from("acme\nevil"))),
        ("store in capitals", Box::new(|v| v["store"] = Value::from("Acme"))),
    ];
    for (name, mutate) in cases {
        let text = edited(|v| mutate(v));
        assert!(Epoch::parse(text.as_bytes()).is_err(), "should be refused: {name}");
    }
}

#[test]
fn near_miss_edits_that_are_legal_still_parse() {
    // Controls for the table above: the same kinds of edit, made legally, are accepted, so the refusals are
    // about the bad value and not about the edit itself.
    for (name, mutate) in [
        ("another handle", Box::new(|v: &mut Value| v["members"][0]["handle"] = Value::from("a.b_c-D")) as Box<dyn Fn(&mut Value)>),
        ("wildcard alone", Box::new(|v| v["members"][3]["steward_areas"] = Value::from(vec!["*"]))),
        ("two tags", Box::new(|v| v["members"][3]["steward_areas"] = Value::from(vec!["rust", "ops"]))),
        ("explicit false voids flag", Box::new(|v| v["members"][0]["voids_tombstones"] = Value::from(false))),
        ("one role", Box::new(|v| v["members"][0]["roles"] = Value::from(vec!["admin"]))),
    ] {
        let text = edited(|v| mutate(v));
        Epoch::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn duplicate_keys_are_refused_where_a_lenient_parser_takes_the_last() {
    let base = genesis_json();
    let top = base.replacen("\"epoch\": 1,", "\"epoch\": 2,\n  \"epoch\": 1,", 1);
    let member = base.replacen("\"person\": \"alice\",", "\"person\": \"mallory\",\n      \"person\": \"alice\",", 1);
    let prev = base.replacen("\"prev_hash\": null,", "\"prev_hash\": null,\n  \"prev_hash\": null,", 1);
    for (name, text) in [("top", top), ("member", member), ("prev_hash", prev)] {
        assert_ne!(text, base, "{name}: the edit applied");
        assert!(serde_json::from_str::<Value>(&text).is_ok(), "{name}: premise: a lenient JSON reader accepts duplicate keys");
        assert!(Epoch::parse(text.as_bytes()).is_err(), "{name}");
    }
}

#[test]
fn bytes_that_are_not_clean_json_are_refused() {
    let base = genesis_json();
    assert!(Epoch::parse(base.as_bytes()).is_ok());
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend_from_slice(base.as_bytes());
    assert!(Epoch::parse(&bom).is_err(), "byte order mark");
    assert!(Epoch::parse(format!("{base} garbage").as_bytes()).is_err(), "trailing text");
    assert!(Epoch::parse(format!("{base}{base}").as_bytes()).is_err(), "two documents");
    assert!(Epoch::parse(&base.as_bytes()[..base.len() / 2]).is_err(), "truncated");
    assert!(Epoch::parse(b"\xff\xfe{}").is_err(), "not UTF-8");
    assert!(Epoch::parse(b"").is_err());
    assert!(Epoch::parse(b"[]").is_err());
    let big = vec![b' '; MAX_EPOCH_BYTES + 1];
    assert!(matches!(Epoch::parse(&big), Err(RosterError::Oversize(_))));
    let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    assert!(Epoch::parse(deep.as_bytes()).is_err(), "deep nesting does not overflow the stack");
}

#[test]
fn signature_files_are_strict_too() {
    let t = TestOrg::three_admins();
    let good = String::from_utf8(t.files[0].sigs_json.clone()).unwrap();
    assert!(EpochSigs::parse(good.as_bytes()).is_ok());
    let kid = t.did("alice");
    let sig = &EpochSigs::parse(good.as_bytes()).unwrap().signatures[0].1.clone();
    let bad = [
        good.replacen("\"epoch\": 1,", "\"epoch\": 1, \"extra\": 1,", 1),
        good.replacen("\"epoch\": 1,", "\"epoch\": 1, \"epoch\": 1,", 1),
        good.replacen(sig.as_str(), &format!("{sig}="), 1),
        good.replacen(sig.as_str(), &sig[..sig.len() - 1], 1),
        good.replacen(&kid, "did:key:zNope", 1),
        r#"{"epoch":1}"#.to_string(),
        r#"{"epoch":"1","signatures":[]}"#.to_string(),
        r#"{"signatures":[]}"#.to_string(),
        r#"{"epoch":1,"signatures":[{"kid":"x"}]}"#.to_string(),
    ];
    for b in bad {
        assert!(EpochSigs::parse(b.as_bytes()).is_err(), "{b}");
    }
    let many: Vec<(String, String)> = (0..300).map(|_| (kid.clone(), sig.clone())).collect();
    assert!(EpochSigs::parse(testkit::sigs_json(1, &many).as_bytes()).is_err(), "too many signatures");
    assert!(matches!(EpochSigs::parse(&vec![b' '; MAX_SIGS_BYTES + 1]), Err(RosterError::Oversize(_))));
}

// ---- genesis ----------------------------------------------------------------------------------------------

#[test]
fn genesis_must_be_signed_by_every_founding_admin() {
    let mut t = TestOrg::three_admins();
    t.files[0] = t.specs[0].sign(&[&t.key("alice"), &t.key("bob")]);
    assert!(matches!(verified(&t), RosterError::GenesisNotFullySigned { epoch: 1 }));
    // A non-admin founder signing does not stand in for the missing admin.
    t.files[0] = t.specs[0].sign(&[&t.key("alice"), &t.key("bob"), &t.key("erin")]);
    assert!(matches!(verified(&t), RosterError::GenesisNotFullySigned { epoch: 1 }));
    // Nobody signing is no better.
    t.files[0] = t.specs[0].sign(&[]);
    assert!(matches!(verified(&t), RosterError::GenesisNotFullySigned { .. }));
    // All three, with a stranger's extra signature ignored: fine.
    let stranger = testkit::key(77);
    t.files[0] = t.specs[0].sign(&[&t.key("alice"), &t.key("bob"), &t.key("carol"), &stranger]);
    t.verify().unwrap();
}

#[test]
fn genesis_with_no_admin_or_a_non_active_founder_is_refused() {
    let mut t = TestOrg::empty();
    let a = t.who("alice");
    let no_admin = EpochSpec { org: ORG.into(), store: STORE.into(), epoch: 1, prev_hash: None, members: vec![MemberSpec::new(&a, "alice")] };
    t.push_signed(no_admin, &["alice"]);
    assert!(matches!(verified(&t), RosterError::NoActiveAdmin { epoch: 1 }));

    let mut t = TestOrg::empty();
    let a = t.who("alice");
    let b2 = t.who("bob");
    let mut retired_founder = MemberSpec::new(&b2, "bob");
    retired_founder.status = "retired";
    let spec = EpochSpec { org: ORG.into(), store: STORE.into(), epoch: 1, prev_hash: None, members: vec![MemberSpec::new(&a, "alice").admin(), retired_founder] };
    t.push_signed(spec, &["alice"]);
    assert!(matches!(verified(&t), RosterError::BadNewEntry { epoch: 1 }));
}

#[test]
fn a_genesis_that_names_a_previous_epoch_is_refused() {
    let mut t = TestOrg::three_admins();
    t.specs[0].prev_hash = Some(crate::hash::of(b"before"));
    t.files[0] = t.specs[0].sign(&[&t.key("alice"), &t.key("bob"), &t.key("carol")]);
    assert_eq!(verified(&t), RosterError::GenesisPrevHash);
}

// ---- quorum -----------------------------------------------------------------------------------------------

/// Epoch 2 that only adds frank, signed by `signers`.
fn add_frank_signed_by(signers: &[&str]) -> TestOrg {
    let mut t = TestOrg::three_admins();
    let frank = t.who("frank");
    t.advance(signers, |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    t
}

#[test]
fn two_of_three_admins_suffice_and_one_does_not() {
    for ok in [&["alice", "bob"][..], &["bob", "carol"], &["alice", "carol"], &["alice", "bob", "carol"]] {
        add_frank_signed_by(ok).verify().unwrap_or_else(|e| panic!("{ok:?}: {e}"));
    }
    for few in [&["alice"][..], &["bob"], &["carol"], &[]] {
        assert!(matches!(verified(&add_frank_signed_by(few)), RosterError::QuorumNotMet { epoch: 2, need: 2, .. }), "{few:?}");
    }
}

#[test]
fn non_admins_do_not_count_toward_quorum() {
    // dave is a steward and erin a member: neither approves a roster change.
    let t = add_frank_signed_by(&["alice", "dave", "erin"]);
    assert!(matches!(verified(&t), RosterError::QuorumNotMet { epoch: 2, have: 1, need: 2 }));
}

#[test]
fn two_dids_of_one_person_count_once() {
    let mut t = TestOrg::three_admins();
    let alice2 = t.who("alice2");
    // Epoch 2: alice gets a second admin DID (signed legitimately).
    t.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&alice2, "alice").admin()));
    assert_eq!(t.verify().unwrap().head().epoch.admin_people().len(), 3, "three people, four admin keys");
    // Epoch 3 signed by both of alice's keys only.
    let frank = t.who("frank");
    t.advance(&["alice", "alice2"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    // Premise: two valid signatures by active admin keys are present, so a per-key count of 2 would pass.
    let sigs = EpochSigs::parse(&t.files[2].sigs_json).unwrap();
    assert_eq!(sigs.signatures.len(), 2);
    assert!(matches!(verified(&t), RosterError::QuorumNotMet { epoch: 3, have: 1, need: 2 }));
    // One DID of alice plus bob is two people.
    t.files[2] = t.specs[2].sign(&[&t.key("alice2"), &t.key("bob")]);
    t.verify().unwrap();
}

#[test]
fn quorum_is_counted_over_people_when_a_person_holds_two_admin_keys() {
    // n_admin counts people: alice (2 keys), bob, carol = 3 people, need 2, whatever the key count.
    let mut t = TestOrg::three_admins();
    let alice2 = t.who("alice2");
    t.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&alice2, "alice").admin()));
    let frank = t.who("frank");
    t.advance(&["alice", "alice2", "bob"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    t.verify().unwrap();
}

#[test]
fn signatures_by_retired_and_removed_keys_are_ignored() {
    // Epoch 2 retires carol (to carol2); epoch 3 removes bob.
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| t.rotate(s, "carol", "carol2", false));
    t.advance(&["alice", "carol2"], |t, s| {
        let bob = t.key("bob");
        let m = s.member_mut(&bob);
        m.status = "removed";
        m.removed_at_epoch = Some(3);
    });
    t.verify().unwrap();
    let frank = t.who("frank");
    // Epoch 4: alice plus the retired carol and the removed bob. Only alice counts (n_admin = alice, carol2 -> need 2).
    let spec = {
        let mut s = t.draft();
        s.members.push(MemberSpec::new(&frank, "frank"));
        s
    };
    let stale_signers = ["alice", "carol", "bob"];
    t.push_signed(spec.clone(), &stale_signers);
    let sigs = EpochSigs::parse(&t.files[3].sigs_json).unwrap();
    assert_eq!(sigs.signatures.len(), 3, "premise: three cryptographically valid signatures are present");
    assert!(matches!(verified(&t), RosterError::QuorumNotMet { epoch: 4, have: 1, need: 2 }));
    // With carol2 instead of the dead keys it passes.
    t.files[3] = spec.sign(&[&t.key("alice"), &t.key("carol2")]);
    t.verify().unwrap();
}

#[test]
fn a_quorum_computed_from_the_new_epoch_would_let_one_admin_take_over() {
    // The takeover: alice alone writes epoch 2 demoting bob and carol, so that in *her* epoch she is the whole
    // admin set and her one signature is a unanimous quorum. The spec counts the PREVIOUS epoch's admins.
    let mut t = TestOrg::three_admins();
    t.advance(&["alice"], |t, s| {
        for n in ["bob", "carol"] {
            let k = t.key(n);
            s.member_mut(&k).roles = vec!["member"];
        }
    });
    assert_eq!(t.head_spec().members.iter().filter(|m| m.roles.contains(&"admin")).count(), 1);
    assert!(matches!(verified(&t), RosterError::QuorumNotMet { epoch: 2, have: 1, need: 2 }));
}

#[test]
fn a_new_admin_cannot_approve_their_own_appointment() {
    let mut t = TestOrg::three_admins();
    let mallory = t.who("mallory");
    t.advance(&["alice", "mallory"], |_, s| s.members.push(MemberSpec::new(&mallory, "mallory").admin()));
    assert!(matches!(verified(&t), RosterError::QuorumNotMet { epoch: 2, have: 1, need: 2 }));
}

#[test]
fn a_signature_by_a_key_outside_the_previous_epoch_counts_for_nothing() {
    let mut t = TestOrg::three_admins();
    let frank = t.who("frank");
    t.who("outsider");
    t.advance(&["alice", "outsider"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    assert!(matches!(verified(&t), RosterError::QuorumNotMet { epoch: 2, have: 1, need: 2 }));
}

#[test]
fn a_signature_over_different_bytes_epoch_or_store_counts_for_nothing() {
    let t = add_frank_signed_by(&["alice", "bob"]);
    t.verify().unwrap();
    let good = &t.files[1];
    let (alice, bob) = (t.key("alice"), t.key("bob"));

    // Different bytes: genuine signatures of another version of epoch 2 attached to this one.
    let mut other = t.specs[1].clone();
    other.members[4].handle = "erin2".into();
    let other_sigs = other.sign(&[&alice, &bob]).sigs_json;
    let mut swapped = t.clone();
    swapped.files[1] = EpochFile { epoch_json: good.epoch_json.clone(), sigs_json: other_sigs };
    assert!(matches!(verified(&swapped), RosterError::QuorumNotMet { epoch: 2, have: 0, need: 2 }));

    // One byte of whitespace changed after signing.
    let mut spaced = good.epoch_json.clone();
    spaced.push(b'\n');
    let mut t2 = t.clone();
    t2.files[1] = EpochFile { epoch_json: spaced, sigs_json: good.sigs_json.clone() };
    let anchor_at_1 = Anchor::new([(1, t.specs[0].hash())]).unwrap();
    assert!(matches!(roster::verify_chain(ORG, STORE, &t2.files, &anchor_at_1), Err(RosterError::QuorumNotMet { epoch: 2, have: 0, .. })));
    // (With the head pinned, the changed bytes are caught earlier, by the pin.)
    assert!(matches!(t2.verify(), Err(RosterError::PinMismatch { epoch: 2 })));

    // Signed for another epoch number or another store.
    let domain_other_epoch = Domain::roster(ORG, STORE, 3, &hash::of(&good.epoch_json)).unwrap();
    let domain_other_store = Domain::roster(ORG, "other-store", 2, &hash::of(&good.epoch_json)).unwrap();
    let domain_other_org = Domain::roster("other", STORE, 2, &hash::of(&good.epoch_json)).unwrap();
    for d in [domain_other_epoch, domain_other_store, domain_other_org] {
        let sigs: Vec<(String, String)> = [&alice, &bob].iter().map(|k| (k.did(), k.sign(&d))).collect();
        let mut t3 = t.clone();
        t3.files[1] = EpochFile { epoch_json: good.epoch_json.clone(), sigs_json: testkit::sigs_json(2, &sigs).into_bytes() };
        assert!(matches!(t3.verify(), Err(RosterError::QuorumNotMet { epoch: 2, have: 0, .. })), "{:?}", t3.verify());
    }

    // The signatures file naming another epoch is refused outright.
    let mut t4 = t.clone();
    let sigs = EpochSigs::parse(&good.sigs_json).unwrap();
    t4.files[1].sigs_json = testkit::sigs_json(3, &sigs.signatures).into_bytes();
    assert!(matches!(t4.verify(), Err(RosterError::SigsEpochMismatch { epoch: 2 })));
}

#[test]
fn a_garbage_signature_from_an_admin_is_ignored_but_the_real_ones_still_count() {
    let t = add_frank_signed_by(&["alice", "bob"]);
    let sigs = EpochSigs::parse(&t.files[1].sigs_json).unwrap();
    let mut junk = sigs.signatures.clone();
    junk[0].1 = junk[1].1.clone(); // alice presented with bob's signature
    let mut t2 = t.clone();
    t2.files[1].sigs_json = testkit::sigs_json(2, &junk).into_bytes();
    assert!(matches!(t2.verify(), Err(RosterError::QuorumNotMet { epoch: 2, have: 1, need: 2 })));
    // Adding carol's real signature back restores a quorum of (bob, carol).
    let carol = t.key("carol");
    let d = Domain::roster(ORG, STORE, 2, &hash::of(&t.files[1].epoch_json)).unwrap();
    junk.push((carol.did(), carol.sign(&d)));
    t2.files[1].sigs_json = testkit::sigs_json(2, &junk).into_bytes();
    t2.verify().unwrap();
}

#[test]
fn repeating_one_signature_does_not_multiply_it() {
    let t = add_frank_signed_by(&["alice"]);
    let one = EpochSigs::parse(&t.files[1].sigs_json).unwrap().signatures;
    let many: Vec<_> = std::iter::repeat_n(one[0].clone(), 5).collect();
    let mut t2 = t.clone();
    t2.files[1].sigs_json = testkit::sigs_json(2, &many).into_bytes();
    assert!(matches!(t2.verify(), Err(RosterError::QuorumNotMet { epoch: 2, have: 1, need: 2 })));
}

fn two_admin_org() -> TestOrg {
    let mut t = TestOrg::empty();
    let (a, b) = (t.who("alice"), t.who("bob"));
    let g = EpochSpec { org: ORG.into(), store: STORE.into(), epoch: 1, prev_hash: None, members: vec![MemberSpec::new(&a, "alice").admin(), MemberSpec::new(&b, "bob").admin()] };
    t.push_signed(g, &["alice", "bob"]);
    t
}

#[test]
fn with_two_admins_both_must_sign_and_with_one_the_one_signs() {
    let mut t = two_admin_org();
    let frank = t.who("frank");
    t.advance(&["alice"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    assert!(matches!(verified(&t), RosterError::QuorumNotMet { epoch: 2, have: 1, need: 2 }));
    t.files[1] = t.specs[1].sign(&[&t.key("alice"), &t.key("bob")]);
    t.verify().unwrap();

    // One admin: that admin alone.
    let mut solo = TestOrg::empty();
    let a = solo.who("alice");
    let g = EpochSpec { org: ORG.into(), store: STORE.into(), epoch: 1, prev_hash: None, members: vec![MemberSpec::new(&a, "alice").admin()] };
    solo.push_signed(g, &["alice"]);
    let frank = solo.who("frank");
    solo.advance(&["alice"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    solo.verify().unwrap();
    solo.files[1] = solo.specs[1].sign(&[]);
    assert!(matches!(verified(&solo), RosterError::QuorumNotMet { epoch: 2, have: 0, need: 1 }));
}

#[test]
fn quorum_follows_the_roster_as_it_changes() {
    // After bob and carol are demoted by a proper 2-of-3 epoch, alice alone is the whole quorum.
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        for n in ["bob", "carol"] {
            let k = t.key(n);
            s.member_mut(&k).roles = vec!["member"];
        }
    });
    let frank = t.who("frank");
    t.advance(&["alice"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    t.verify().unwrap();
    // And bob, now only a member, can no longer approve.
    t.files[2] = t.specs[2].sign(&[&t.key("bob")]);
    assert!(matches!(verified(&t), RosterError::QuorumNotMet { epoch: 3, have: 0, need: 1 }));
}

// ---- chain structure --------------------------------------------------------------------------------------

#[test]
fn a_fork_two_files_for_one_epoch_number_is_refused() {
    let t = org4();
    // Honest epoch 3 and a different, validly signed epoch 3'.
    let forked = t.fork_at(3, &["alice", "bob"], |t, s| {
        let g = t.who("gina");
        s.members.push(MemberSpec::new(&g, "gina"));
    });
    assert_ne!(forked.files[2], t.files[2]);
    forked.verify().unwrap();
    // Offered together as one list, the second 3 is a fork.
    let mut both = t.files[..3].to_vec();
    both.push(forked.files[2].clone());
    assert_eq!(roster::verify_chain(ORG, STORE, &both, &t.pin_head()).unwrap_err(), RosterError::Fork { epoch: 3 });
    // An install that already accepted the honest epoch 3 refuses the forked chain by its pin.
    assert_eq!(roster::verify_chain(ORG, STORE, &forked.files, &t.anchor_all()).unwrap_err(), RosterError::PinMismatch { epoch: 3 });
}

#[test]
fn a_gap_a_reorder_and_a_replay_are_refused() {
    let t = org4();
    let f = &t.files;
    let anchor = t.pin_head();
    let go = |list: Vec<EpochFile>| roster::verify_chain(ORG, STORE, &list, &anchor);
    assert_eq!(go(vec![f[0].clone(), f[1].clone(), f[3].clone()]).unwrap_err(), RosterError::Gap { expected: 3, found: 4 });
    assert_eq!(go(vec![f[0].clone(), f[2].clone()]).unwrap_err(), RosterError::Gap { expected: 2, found: 3 });
    assert_eq!(go(vec![f[0].clone(), f[2].clone(), f[1].clone()]).unwrap_err(), RosterError::Gap { expected: 2, found: 3 });
    // A chain that does not start at genesis.
    assert_eq!(go(vec![f[1].clone(), f[2].clone()]).unwrap_err(), RosterError::Gap { expected: 1, found: 2 });
    // Replay: an old epoch shown again at the end, or twice in a row.
    assert_eq!(go(vec![f[0].clone(), f[1].clone(), f[0].clone()]).unwrap_err(), RosterError::Replay { epoch: 1 });
    assert_eq!(go(vec![f[0].clone(), f[1].clone(), f[1].clone()]).unwrap_err(), RosterError::Replay { epoch: 2 });
    assert_eq!(go(vec![]).unwrap_err(), RosterError::EmptyChain);
}

#[test]
fn a_swapped_prev_hash_is_refused_even_when_validly_signed() {
    let mut t = org4();
    // Epoch 3 claims to follow epoch 1 (skipping 2's changes), signed by a real quorum.
    let mut s3 = t.specs[2].clone();
    s3.prev_hash = Some(t.specs[0].hash());
    t.files[2] = s3.sign(&[&t.key("bob"), &t.key("carol")]);
    assert_eq!(t.verify().unwrap_err(), RosterError::PrevHashMismatch { epoch: 3 });
    // A prev_hash that is not even a hash is a parse failure, not a mismatch.
    let mut s3 = t.specs[2].clone();
    s3.prev_hash = Some("blake3:xyz".into());
    t.files[2] = s3.sign(&[&t.key("bob"), &t.key("carol")]);
    assert!(matches!(t.verify().unwrap_err(), RosterError::BadEpoch(_)));
}

#[test]
fn epoch_files_from_another_store_or_org_are_refused() {
    let t = org4();
    assert_eq!(roster::verify_chain(ORG, "other-store", &t.files, &t.pin_head()).unwrap_err(), RosterError::WrongScope { epoch: 1 });
    assert_eq!(roster::verify_chain("other", STORE, &t.files, &t.pin_head()).unwrap_err(), RosterError::WrongScope { epoch: 1 });
    assert_eq!(roster::verify_chain("Bad Org", STORE, &t.files, &t.pin_head()).unwrap_err(), RosterError::BadScope);
    // A single epoch from another store spliced into the chain.
    let mut other = TestOrg::three_admins();
    other.store = "elsewhere".into();
    let mut s = other.specs[0].clone();
    s.store = "elsewhere".into();
    let spliced = s.sign(&[&t.key("alice"), &t.key("bob"), &t.key("carol")]);
    let mut list = t.files.clone();
    list[0] = spliced;
    assert_eq!(roster::verify_chain(ORG, STORE, &list, &t.pin_head()).unwrap_err(), RosterError::WrongScope { epoch: 1 });
}

#[test]
fn an_epoch_whose_own_number_disagrees_with_its_position_is_refused() {
    let mut t = org4();
    let mut s2 = t.specs[1].clone();
    s2.epoch = 3;
    t.files[1] = s2.sign(&[&t.key("alice"), &t.key("bob")]);
    assert!(matches!(t.verify().unwrap_err(), RosterError::Gap { expected: 2, found: 3 }));
}

// ---- the anchor -------------------------------------------------------------------------------------------

#[test]
fn a_chain_with_nothing_pinned_is_refused() {
    assert_eq!(Anchor::new([]).unwrap_err(), RosterError::NoAnchor);
    assert!(Anchor::new([(0, crate::hash::of(b"x"))]).is_err());
    assert!(Anchor::new([(1, "blake3:short".into())]).is_err());
}

#[test]
fn a_self_consistent_forged_chain_fails_only_by_its_hash_pin() {
    // The attacker holds keys that were once founders. Real chain: epoch 2 retires alice and bob.
    let mut real = TestOrg::three_admins();
    real.advance(&["alice", "bob"], |t, s| {
        t.rotate(s, "alice", "alice2", true);
        t.rotate(s, "bob", "bob2", true);
    });
    real.verify().unwrap();
    let pinned = real.pin_head();
    // Forged chain: same genesis, epoch 2' appoints mallory, signed by the old founders (active in the forged
    // view). It is internally valid: anchored at its own head it verifies.
    let forged = real.takeover_with(2, &["alice", "bob"], |t, s| {
        let m = t.who("mallory");
        s.members.push(MemberSpec::new(&m, "mallory").admin());
    });
    let own_head = forged.pin_head();
    roster::verify_chain(ORG, STORE, &forged.files, &own_head).expect("premise: the forgery is internally consistent");
    assert_eq!(roster::verify_chain(ORG, STORE, &forged.files, &pinned).unwrap_err(), RosterError::PinMismatch { epoch: 2 });
}

#[test]
fn a_chain_shorter_than_the_pins_is_a_stale_view_and_is_refused() {
    let t = org4();
    let short = t.upto(3);
    assert_eq!(roster::verify_chain(ORG, STORE, &short.files, &t.anchor_all()).unwrap_err(), RosterError::PinnedEpochMissing { epoch: 4 });
    assert_eq!(roster::verify_chain(ORG, STORE, &short.files, &t.pin_head()).unwrap_err(), RosterError::PinnedEpochMissing { epoch: 4 });
    // The same short chain is fine for an install that only knows epoch 3.
    roster::verify_chain(ORG, STORE, &short.files, &short.pin_head()).unwrap();
}

#[test]
fn a_chain_may_run_past_the_pins() {
    let t = org4();
    let pinned_at_2 = Anchor::new([(2, t.specs[1].hash())]).unwrap();
    assert_eq!(roster::verify_chain(ORG, STORE, &t.files, &pinned_at_2).unwrap().head_epoch(), 4);
}

#[test]
fn a_pin_in_the_middle_holds_even_if_later_epochs_would_be_fine() {
    let t = org4();
    let forked = t.fork_at(2, &["alice", "carol"], |t, s| {
        let g = t.who("gina");
        s.members.push(MemberSpec::new(&g, "gina"));
    });
    let pinned = Anchor::new([(2, t.specs[1].hash())]).unwrap();
    assert_eq!(roster::verify_chain(ORG, STORE, &forked.files, &pinned).unwrap_err(), RosterError::PinMismatch { epoch: 2 });
}

// ---- transitions: person, never dropped, status ----------------------------------------------------------

#[test]
fn a_dropped_entry_is_refused_even_with_a_full_quorum() {
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob", "carol"], |t, s| {
        let erin = t.did("erin");
        s.members.retain(|m| m.did != erin);
    });
    assert_eq!(verified(&t), RosterError::Dropped { epoch: 2 });
}

#[test]
fn changing_a_persons_identifier_is_refused() {
    // Spec 011 test 29.
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        s.member_mut(&erin).person = "frank".into();
    });
    assert_eq!(verified(&t), RosterError::PersonChanged { epoch: 2 });
}

#[test]
fn a_successor_with_another_person_is_refused() {
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        t.rotate(s, "dave", "dave2", false);
        let d2 = t.key("dave2");
        s.member_mut(&d2).person = "erin".into();
    });
    assert_eq!(verified(&t), RosterError::PersonChanged { epoch: 2 });
}

#[test]
fn a_person_cannot_be_swapped_by_moving_a_key_to_a_different_person() {
    // Key reuse across persons: erin's DID listed again under mallory is a duplicate DID; relabelling the one
    // entry is a person change. Either way a key never belongs to two people.
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        s.members.push(MemberSpec::new(&erin, "mallory"));
    });
    assert!(matches!(verified(&t), RosterError::BadEpoch("a DID is listed twice")));
}

#[test]
fn a_removed_or_retired_identity_cannot_come_back() {
    // Spec 012 test 13: retired/removed DID returned to active.
    for (status, extra) in [("removed", Some(2u64)), ("retired", None)] {
        let mut t = TestOrg::three_admins();
        t.advance(&["alice", "bob"], |t, s| {
            let erin = t.key("erin");
            let m = s.member_mut(&erin);
            m.status = status;
            m.removed_at_epoch = extra;
        });
        t.verify().unwrap();
        t.advance(&["alice", "bob"], |t, s| {
            let erin = t.key("erin");
            let m = s.member_mut(&erin);
            m.status = "active";
            m.removed_at_epoch = None;
        });
        assert_eq!(verified(&t), RosterError::StatusRegression { epoch: 3 }, "{status}");
    }
}

#[test]
fn removed_cannot_soften_to_retired_and_retired_may_be_removed() {
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        s.member_mut(&erin).status = "retired";
    });
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        let m = s.member_mut(&erin);
        m.status = "removed";
        m.removed_at_epoch = Some(3);
    });
    t.verify().unwrap();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        let m = s.member_mut(&erin);
        m.status = "retired";
        m.removed_at_epoch = None;
    });
    assert_eq!(verified(&t), RosterError::StatusRegression { epoch: 4 });
}

#[test]
fn a_dropped_and_readded_identity_is_refused_at_the_drop() {
    // "Removed then re-added" the other way: drop erin in epoch 2 and add the same key fresh in epoch 3.
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.did("erin");
        s.members.retain(|m| m.did != erin);
    });
    assert_eq!(verified(&t), RosterError::Dropped { epoch: 2 });
}

#[test]
fn removed_at_epoch_must_name_the_epoch_of_removal() {
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        let m = s.member_mut(&erin);
        m.status = "removed";
        m.removed_at_epoch = Some(1); // backdated
    });
    assert_eq!(verified(&t), RosterError::ImmutableChanged { epoch: 2, field: "removed_at_epoch" });
}

#[test]
fn what_is_frozen_once_an_entry_leaves_stays_frozen() {
    let base = || {
        let mut t = TestOrg::three_admins();
        t.advance(&["alice", "bob"], |t, s| {
            let erin = t.key("erin");
            s.member_mut(&erin).status = "retired";
        });
        t
    };
    type Edit = fn(&mut MemberSpec);
    let edits: [(&str, Edit); 4] = [
        ("handle", |m| m.handle = "someone-else".into()),
        ("roles", |m| m.roles = vec!["member", "admin"]),
        ("steward_areas", |m| m.steward_areas = vec!["*".into()]),
        ("added_at", |m| m.added_at = "2020-01-01T00:00:00Z".into()),
    ];
    for (field, edit) in edits {
        let mut t = base();
        t.advance(&["alice", "bob"], |t, s| {
            let erin = t.key("erin");
            edit(s.member_mut(&erin));
        });
        assert_eq!(verified(&t), RosterError::ImmutableChanged { epoch: 3, field }, "{field}");
    }
    // A retiring entry cannot also change roles in the same epoch.
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let dave = t.key("dave");
        let m = s.member_mut(&dave);
        m.status = "retired";
        m.steward_areas = vec!["*".into()];
    });
    assert_eq!(verified(&t), RosterError::ImmutableChanged { epoch: 2, field: "steward_areas" });
}

#[test]
fn an_active_member_may_change_handle_roles_and_areas() {
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        let m = s.member_mut(&erin);
        m.handle = "erin-b".into();
        m.steward_areas = vec!["rust".into(), "ops".into()];
        let dave = t.key("dave");
        s.member_mut(&dave).steward_areas = vec!["*".into()];
    });
    let r = t.verify().unwrap();
    let erin = r.member(&t.did("erin")).unwrap();
    assert!(erin.steward_areas.is_steward() && erin.steward_areas.covers(&["ops".to_string()]) && !erin.steward_areas.covers(&[]));
    assert!(r.member(&t.did("dave")).unwrap().steward_areas.covers(&[]), "* covers untagged records");
}

#[test]
fn voids_tombstones_can_be_set_later_but_never_cleared() {
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        let m = s.member_mut(&erin);
        m.status = "removed";
        m.removed_at_epoch = Some(2);
    });
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        s.member_mut(&erin).voids_tombstones = Some(true);
    });
    assert!(t.verify().unwrap().member(&t.did("erin")).unwrap().voids_tombstones);
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        s.member_mut(&erin).voids_tombstones = Some(false);
    });
    assert_eq!(verified(&t), RosterError::ImmutableChanged { epoch: 4, field: "voids_tombstones" });
}

#[test]
fn new_entries_start_active_and_clean() {
    type Edit = fn(&mut MemberSpec);
    let edits: [(&str, Edit); 3] = [
        ("born retired", |m| m.status = "retired"),
        ("born removed", |m| {
            m.status = "removed";
            m.removed_at_epoch = Some(2);
        }),
        ("born voiding", |m| {
            m.status = "retired";
            m.voids_tombstones = Some(true);
        }),
    ];
    for (name, edit) in edits {
        let mut t = TestOrg::three_admins();
        let frank = t.who("frank");
        t.advance(&["alice", "bob"], |_, s| {
            let mut m = MemberSpec::new(&frank, "frank");
            edit(&mut m);
            s.members.push(m);
        });
        assert_eq!(verified(&t), RosterError::BadNewEntry { epoch: 2 }, "{name}");
    }
}

#[test]
fn successors_must_exist_share_the_person_and_not_loop() {
    // Points at a DID that is not in the epoch.
    let mut t = TestOrg::three_admins();
    let ghost = testkit::key(90).did();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        let m = s.member_mut(&erin);
        m.status = "retired";
        m.successor = Some(ghost.clone());
    });
    assert_eq!(verified(&t), RosterError::BadSuccessor { epoch: 2 });

    // A cycle: erin -> dave -> erin, same person throughout (both retired).
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let dave = t.key("dave");
        s.member_mut(&dave).person = "erin".into();
    });
    // That edit itself changes dave's person; build the cycle in genesis instead, where nothing is compared.
    assert_eq!(verified(&t), RosterError::PersonChanged { epoch: 2 });
    let mut c = TestOrg::empty();
    let (a, e1, e2) = (c.who("alice"), c.who("e1"), c.who("e2"));
    let mut m1 = MemberSpec::new(&e1, "erin");
    let mut m2 = MemberSpec::new(&e2, "erin");
    m1.status = "retired";
    m2.status = "retired";
    m1.successor = Some(e2.did());
    m2.successor = Some(e1.did());
    let g = EpochSpec { org: ORG.into(), store: STORE.into(), epoch: 1, prev_hash: None, members: vec![MemberSpec::new(&a, "alice").admin(), m1, m2] };
    // Parsing refuses the cycle (and a non-active founder) before any signature matters.
    assert!(Epoch::parse(g.to_json().as_bytes()).is_err());

    // A successor that is the entry itself.
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        let erin = t.key("erin");
        let m = s.member_mut(&erin);
        m.status = "retired";
        m.successor = Some(erin.did());
    });
    assert_eq!(verified(&t), RosterError::BadSuccessor { epoch: 2 });
}

#[test]
fn a_successor_cannot_be_rewritten_after_the_fact() {
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| t.rotate(s, "dave", "dave2", false));
    t.advance(&["alice", "bob"], |t, s| {
        let dave = t.key("dave");
        s.member_mut(&dave).successor = None;
    });
    assert_eq!(verified(&t), RosterError::ImmutableChanged { epoch: 3, field: "successor" });
}

#[test]
fn the_roster_always_keeps_an_active_admin() {
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| {
        for n in ["alice", "bob", "carol"] {
            let k = t.key(n);
            s.member_mut(&k).roles = vec!["member"];
        }
    });
    assert_eq!(verified(&t), RosterError::NoActiveAdmin { epoch: 2 });
}

#[test]
fn routine_and_compromise_rotations_chain_through_successors() {
    let mut t = TestOrg::three_admins();
    t.advance(&["alice", "bob"], |t, s| t.rotate(s, "dave", "dave2", false));
    t.advance(&["alice", "bob"], |t, s| t.rotate(s, "dave2", "dave3", true));
    let r = t.verify().unwrap();
    assert_eq!(r.successor_chain(&t.did("dave")), vec![t.did("dave"), t.did("dave2"), t.did("dave3")]);
    assert!(r.member(&t.did("dave2")).unwrap().voids_tombstones);
    assert!(!r.member(&t.did("dave")).unwrap().voids_tombstones);
    assert_eq!(r.person_of(&t.did("dave3")), Some("dave"));
}

#[test]
fn oversize_chains_and_rosters_are_refused() {
    let t = TestOrg::three_admins();
    let mut big = t.files[0].clone();
    big.epoch_json = vec![b' '; MAX_EPOCH_BYTES + 1];
    assert!(matches!(roster::verify_chain(ORG, STORE, &[big], &t.pin_head()), Err(RosterError::Oversize(_))));
    let many = vec![t.files[0].clone(); MAX_EPOCHS + 1];
    assert_eq!(roster::verify_chain(ORG, STORE, &many, &t.pin_head()).unwrap_err(), RosterError::TooManyEpochs);
}

#[test]
fn errors_never_print_attacker_text_and_have_slugs() {
    let t = org4();
    let e = roster::verify_chain(ORG, STORE, &t.files[..2], &t.pin_head()).unwrap_err();
    assert_eq!(e.reason(), "pinned-epoch-missing");
    assert!(e.to_string().starts_with("pinned-epoch-missing"));
    let hostile = sign_bytes(ORG, STORE, 1, br#"{"org":"acme","store":"acme-main","epoch":1,"prev_hash":null,"members":[{"did":"\u001b[31mBOOM","x":1}]}"#, &[]);
    let e = roster::verify_chain(ORG, STORE, &[hostile], &t.pin_head()).unwrap_err();
    assert!(!e.to_string().contains("BOOM") && !e.to_string().contains('\u{1b}'));
}

#[test]
fn timestamps_in_the_fixture_are_the_strict_spelling() {
    assert!(crate::is_utc_timestamp(TS));
}
