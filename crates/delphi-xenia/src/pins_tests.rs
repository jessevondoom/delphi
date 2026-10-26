//! `pins.toml` and `join` (spec 011 §10 tests 5, 6, 26, 31; threats T17, T24).

use super::*;
use crate::roster;
use crate::roster::RosterError;
use crate::testkit::{self, MemberSpec, TestOrg, ORG, STORE};

const URL: &str = "https://git.example.com/acme/xenia-acme.git";

/// Four epochs: 1 genesis; 2 adds frank; 3 retires alice (compromised, to alice2); 4 removes erin.
fn org4() -> TestOrg {
    let mut t = TestOrg::three_admins();
    let frank = t.who("frank");
    t.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    t.advance(&["bob", "carol"], |t, s| t.rotate(s, "alice", "alice2", true));
    t.advance(&["bob", "carol"], |t, s| {
        let erin = t.key("erin");
        let m = s.member_mut(&erin);
        m.status = "removed";
        m.removed_at_epoch = Some(4);
    });
    t
}

fn request(t: &TestOrg, admin: &str) -> JoinRequest {
    let (epoch, hash) = t.head_pair();
    JoinRequest { url: URL.into(), admin_fingerprint: t.fingerprint(admin), head_epoch: epoch, head_hash: hash }
}

fn joined(t: &TestOrg) -> Pins {
    join_verify(&request(t, "bob"), &t.files).unwrap().0
}

// ---- pins.toml --------------------------------------------------------------------------------------------

#[test]
fn pins_round_trip_exactly() {
    let t = org4();
    let pins = joined(&t);
    let text = pins.to_toml();
    assert_eq!(Pins::parse(&text).unwrap(), pins);
    assert_eq!(Pins::parse(&text).unwrap().to_toml(), text);
    assert_eq!(pins.head().0, 4);
    assert_eq!(pins.head().1, t.head_pair().1);
    assert_eq!((pins.org(), pins.store(), pins.url()), (ORG, STORE, URL));
    assert_eq!(pins.admin(), t.fingerprint("bob"));
    assert_eq!(pins.epoch_hash(2), Some(t.specs[1].hash().as_str()));
    assert_eq!(pins.anchor(), t.anchor_all());
}

#[test]
fn malformed_pins_are_refused() {
    let t = org4();
    let good = joined(&t).to_toml();
    let h2 = t.specs[1].hash();
    let bad: Vec<(&str, String)> = vec![
        ("unknown key", good.replacen("format = 1\n", "format = 1\nextra = 1\n", 1)),
        ("duplicate key", good.replacen("format = 1\n", "format = 1\nformat = 1\n", 1)),
        ("future format", good.replacen("format = 1", "format = 2", 1)),
        ("format as string", good.replacen("format = 1", "format = \"1\"", 1)),
        ("missing admin", good.lines().filter(|l| !l.starts_with("admin")).collect::<Vec<_>>().join("\n")),
        ("org in capitals", good.replacen("org = \"acme\"", "org = \"Acme\"", 1)),
        ("short fingerprint", good.replacen(&t.fingerprint("bob"), &t.fingerprint("bob")[..63], 1)),
        ("upper-case fingerprint", good.replacen(&t.fingerprint("bob"), &t.fingerprint("bob").to_uppercase(), 1)),
        ("a hole in the epochs", good.replacen(&format!("2 = \"{h2}\"\n"), "", 1)),
        ("epochs from 2", good.replacen(&format!("1 = \"{}\"\n", t.specs[0].hash()), "", 1)),
        ("non-canonical epoch key", good.replacen("2 = ", "\"02\" = ", 1)),
        ("signed epoch key", good.replacen("2 = ", "\"+2\" = ", 1)),
        ("epoch zero", good.replacen("1 = ", "0 = ", 1)),
        ("duplicate epoch key", good.replacen("2 = ", "1 = ", 1)),
        ("upper-case hash", good.replacen(&h2, &h2.to_uppercase(), 1)),
        ("hash as a number", good.replacen(&format!("2 = \"{h2}\""), "2 = 5", 1)),
        ("no epochs table", good.split("[epochs]").next().unwrap().to_string()),
        ("empty epochs table", good.split("[epochs]").next().unwrap().to_string() + "[epochs]\n"),
        ("url as option", good.replacen(URL, "--upload-pack=evil", 1)),
        ("url with a password", good.replacen("https://", "https://u:pw@", 1)),
        ("url with a space", good.replacen(".git", " .git", 1)),
        ("empty url", good.replacen(URL, "", 1)),
        ("not toml", "format = ".into()),
        ("empty", String::new()),
    ];
    for (name, text) in bad {
        assert_ne!(text, good, "{name}: the edit applied");
        assert!(Pins::parse(&text).is_err(), "should be refused: {name}");
    }
    assert!(matches!(Pins::parse(&" ".repeat(MAX_PINS_BYTES + 1)), Err(RosterError::Oversize(_))));
}

#[test]
fn urls_that_could_become_git_options_credentials_or_markup_are_refused() {
    for ok in ["https://h.example/a.git", "ssh://git@h.example/a.git", "git@h.example:acme/a.git", "https://token@h.example/a.git", "/srv/git/a.git"] {
        assert!(check_url(ok).is_ok(), "{ok}");
    }
    for bad in ["", "-oProxyCommand=x", "--help", "https://u:p@h/a.git", "ssh://u:p@h/a", "a b", "a\nb", "a\"b", "a\\b", "h\u{e9}.example", &"a".repeat(513)] {
        assert!(check_url(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn org_toml_is_advisory_and_a_disagreement_with_the_pins_is_refused() {
    // Spec 011 test 6.
    let pins = joined(&org4());
    pins.check_org_toml("org = \"acme\"\nstore = \"acme-main\"\nmin_client = \"0.2\"\n").unwrap();
    for bad in [
        "org = \"evil\"\nstore = \"acme-main\"\n",
        "org = \"acme\"\nstore = \"evil-main\"\n",
        "org = \"acme\"\n",
        "store = \"acme-main\"\n",
        "org = 5\nstore = \"acme-main\"\n",
        "org = [\"acme\"]\nstore = \"acme-main\"\n",
        "not toml at all [",
        "",
    ] {
        assert!(pins.check_org_toml(bad).is_err(), "{bad:?}");
    }
    assert!(matches!(pins.check_org_toml(&"#".repeat(17 * 1024)), Err(RosterError::Oversize(_))));
}

// ---- join -------------------------------------------------------------------------------------------------

#[test]
fn join_with_the_right_fingerprint_and_head_pair_pins_every_epoch() {
    // Spec 011 test 26 (the success half), test 31's first half.
    let t = org4();
    let storage = MemoryPinStorage::default();
    let (pins, roster) = join(&storage, &request(&t, "bob"), &t.files).unwrap();
    assert_eq!(roster.head_epoch(), 4);
    assert_eq!(pins.head(), (4, t.head_pair().1.as_str()));
    assert_eq!(load(&storage).unwrap().unwrap(), pins);
    for n in 1..=4u64 {
        assert_eq!(pins.epoch_hash(n), Some(t.specs[n as usize - 1].hash().as_str()));
    }
    // The pins then verify the same files and extend to nothing new.
    assert_eq!(pins.extended(&pins.verify(&t.files).unwrap()).unwrap(), pins);
}

#[test]
fn a_wrong_fingerprint_or_head_pair_is_refused_and_writes_nothing() {
    let t = org4();
    let (e, h) = t.head_pair();
    let ok = request(&t, "bob");
    let other_hash = t.specs[1].hash();
    type Expect = fn(&RosterError) -> bool;
    let refused: Vec<(&str, JoinRequest, Expect)> = vec![
        ("fingerprint of a member who is not an admin", JoinRequest { admin_fingerprint: t.fingerprint("dave"), ..ok.clone() }, |e| matches!(e, RosterError::AdminNotActive)),
        ("fingerprint of a stranger", JoinRequest { admin_fingerprint: crate::did::fingerprint(&testkit::key(88).did()).unwrap(), ..ok.clone() }, |e| matches!(e, RosterError::AdminNotActive)),
        ("fingerprint of a retired admin", JoinRequest { admin_fingerprint: t.fingerprint("alice"), ..ok.clone() }, |e| matches!(e, RosterError::AdminNotActive)),
        ("fingerprint of a DID's own text, not its hash", JoinRequest { admin_fingerprint: t.did("bob"), ..ok.clone() }, |e| matches!(e, RosterError::BadRequest(_))),
        ("upper-case fingerprint", JoinRequest { admin_fingerprint: t.fingerprint("bob").to_uppercase(), ..ok.clone() }, |e| matches!(e, RosterError::BadRequest(_))),
        ("pinned hash of another epoch", JoinRequest { head_hash: other_hash, ..ok.clone() }, |e| matches!(e, RosterError::PinMismatch { epoch: 4 })),
        ("pinned hash of nothing", JoinRequest { head_hash: crate::hash::of(b"nope"), ..ok.clone() }, |e| matches!(e, RosterError::PinMismatch { epoch: 4 })),
        ("pinned epoch the chain does not reach", JoinRequest { head_epoch: e + 1, ..ok.clone() }, |e| matches!(e, RosterError::PinnedEpochMissing { epoch: 5 })),
        ("pinned epoch zero", JoinRequest { head_epoch: 0, ..ok.clone() }, |e| matches!(e, RosterError::BadRequest(_))),
        ("malformed hash", JoinRequest { head_hash: h.to_uppercase(), ..ok.clone() }, |e| matches!(e, RosterError::BadRequest(_))),
        ("url as a git option", JoinRequest { url: "--upload-pack=x".into(), ..ok.clone() }, |e| matches!(e, RosterError::BadRequest(_))),
    ];
    for (name, req, expect) in refused {
        let storage = MemoryPinStorage::default();
        let err = join(&storage, &req, &t.files).expect_err(name);
        assert!(expect(&err), "{name}: {err}");
        assert!(storage.read().unwrap().is_none(), "{name}: no state written");
    }
    // The control: the same request, correct, succeeds.
    join(&MemoryPinStorage::default(), &ok, &t.files).unwrap();
}

#[test]
fn the_pinned_admin_must_be_active_in_the_pinned_epoch() {
    // Spec 011 test 5, last clause. Alice is active at epoch 2 and retired at epoch 3.
    let t = org4();
    let at2 = JoinRequest { url: URL.into(), admin_fingerprint: t.fingerprint("alice"), head_epoch: 2, head_hash: t.specs[1].hash() };
    let (pins, _) = join_verify(&at2, &t.files).expect("alice was an active admin at the pinned epoch 2; the chain continues validly past it");
    assert_eq!(pins.head().0, 4, "all four epochs are pinned, not just the handed-out one");
    // Pinning epoch 3 and naming alice, who retired in epoch 3, is refused.
    let at3 = JoinRequest { head_epoch: 3, head_hash: t.specs[2].hash(), ..at2.clone() };
    assert_eq!(join_verify(&at3, &t.files).unwrap_err(), RosterError::AdminNotActive);
    // Her successor, active in 3, is accepted there.
    let at3b = JoinRequest { admin_fingerprint: t.fingerprint("alice2"), ..at3 };
    join_verify(&at3b, &t.files).unwrap();
}

#[test]
fn a_chain_forged_with_retired_founder_keys_is_refused() {
    // Spec 011 test 5, first clause. Real chain: epoch 2 retires alice and bob (both compromised).
    let mut real = TestOrg::three_admins();
    real.advance(&["alice", "bob"], |t, s| {
        t.rotate(s, "alice", "alice2", true);
        t.rotate(s, "bob", "bob2", true);
    });
    real.advance(&["carol", "alice2"], |_, _| {});
    // The attacker, holding the old founder keys, rebuilds from the same genesis to a different head.
    let forged = real.takeover_with(2, &["alice", "bob"], |t, s| {
        let m = t.who("mallory");
        s.members.push(MemberSpec::new(&m, "mallory").admin());
    });
    let mut forged = forged;
    forged.advance(&["alice", "bob", "mallory"], |_, _| {});
    // The joiner was handed the real head and a real admin (carol, who never rotated).
    let req = request(&real, "carol");
    let forged_req = JoinRequest { url: URL.into(), ..req.clone() };
    assert_eq!(join_verify(&forged_req, &forged.files).unwrap_err(), RosterError::PinMismatch { epoch: 3 });
    // Premise: the forged chain is a perfectly consistent roster, and it contains carol as an admin; it is
    // refused only because it does not contain the pinned hash.
    let forged_req2 = request(&forged, "carol");
    join_verify(&forged_req2, &forged.files).expect("premise: the forgery verifies against its own head");
    // The real chain with the same request is fine.
    join_verify(&req, &real.files).unwrap();
}

#[test]
fn a_chain_that_verifies_but_has_a_different_head_hash_is_refused() {
    let t = org4();
    // Same genesis through epoch 3, then a different valid epoch 4'.
    let alt = t.fork_at(4, &["bob", "carol"], |t, s| {
        let g = t.who("gina");
        s.members.push(MemberSpec::new(&g, "gina"));
    });
    alt.verify().unwrap();
    assert_eq!(join_verify(&request(&t, "bob"), &alt.files).unwrap_err(), RosterError::PinMismatch { epoch: 4 });
}

#[test]
fn a_roster_change_after_the_pin_was_handed_out_does_not_break_joining() {
    // Spec 011 test 31. The joiner was told epoch 2; by the time they download, the store is at 4.
    let t = org4();
    let req = JoinRequest { url: URL.into(), admin_fingerprint: t.fingerprint("bob"), head_epoch: 2, head_hash: t.specs[1].hash() };
    let (pins, roster) = join_verify(&req, &t.files).unwrap();
    assert_eq!(roster.head_epoch(), 4);
    assert_eq!(pins.head().0, 4);
    // A chain without the pinned hash is refused, even if it is longer and valid.
    let other = t.fork_at(2, &["alice", "carol"], |t, s| {
        let g = t.who("gina");
        s.members.push(MemberSpec::new(&g, "gina"));
    });
    assert_eq!(join_verify(&req, &other.files).unwrap_err(), RosterError::PinMismatch { epoch: 2 });
    // A chain that stops before the pinned epoch is refused too.
    assert!(matches!(join_verify(&req, &t.files[..1]).unwrap_err(), RosterError::PinnedEpochMissing { epoch: 2 }));
    // And so is one that continues invalidly after the pin (epoch 3 with too few signatures).
    let mut short = t.clone();
    short.files[2] = t.specs[2].sign(&[&t.key("bob")]);
    assert!(matches!(join_verify(&req, &short.files).unwrap_err(), RosterError::QuorumNotMet { epoch: 3, .. }));
}

#[test]
fn join_will_not_overwrite_existing_pins() {
    let t = org4();
    let storage = MemoryPinStorage::default();
    join(&storage, &request(&t, "bob"), &t.files).unwrap();
    let before = storage.read().unwrap();
    assert_eq!(join(&storage, &request(&t, "bob"), &t.files).unwrap_err(), RosterError::AlreadyPinned);
    assert_eq!(storage.read().unwrap(), before);
    // Even a join against some other org does not replace them.
    let mut other = TestOrg::three_admins();
    other.org = "other".into();
    assert_eq!(join(&storage, &request(&t, "bob"), &other.files).unwrap_err(), RosterError::AlreadyPinned);
}

#[test]
fn the_org_and_store_come_from_a_chain_the_hash_pin_covers() {
    // A chain for another store, with its own consistent head: the pair the joiner holds does not match it.
    let t = org4();
    let mut elsewhere = TestOrg::empty();
    elsewhere.store = "elsewhere".into();
    let (a, b, c) = (elsewhere.who("alice"), elsewhere.who("bob"), elsewhere.who("carol"));
    let g = testkit::EpochSpec {
        org: ORG.into(),
        store: "elsewhere".into(),
        epoch: 1,
        prev_hash: None,
        members: vec![MemberSpec::new(&a, "alice").admin(), MemberSpec::new(&b, "bob").admin(), MemberSpec::new(&c, "carol").admin()],
    };
    elsewhere.push_signed(g, &["alice", "bob", "carol"]);
    let req = request(&t, "bob");
    assert!(join_verify(&req, &elsewhere.files).is_err());
}

// ---- keeping the pins current ----------------------------------------------------------------------------

#[test]
fn pins_extend_with_a_valid_longer_chain_and_never_shrink_or_rewrite() {
    let t = org4();
    let at2 = t.upto(2);
    let req = JoinRequest { url: URL.into(), admin_fingerprint: t.fingerprint("bob"), head_epoch: 2, head_hash: t.specs[1].hash() };
    let (pins2, _) = join_verify(&req, &at2.files).unwrap();
    assert_eq!(pins2.head().0, 2);
    // Sync later: the store is at 4.
    let r4 = pins2.verify(&t.files).unwrap();
    let pins4 = pins2.extended(&r4).unwrap();
    assert_eq!(pins4.head().0, 4);
    assert_eq!(pins4.epoch_hash(1), pins2.epoch_hash(1));
    // A store that hides the newest epochs is refused by the pins that already know them.
    assert_eq!(pins4.verify(&t.files[..2]).unwrap_err(), RosterError::PinnedEpochMissing { epoch: 3 });
    // A fork under an accepted epoch number is refused (equivocation).
    let fork = t.fork_at(3, &["alice", "bob"], |t, s| {
        let g = t.who("gina");
        s.members.push(MemberSpec::new(&g, "gina"));
    });
    assert_eq!(pins4.verify(&fork.files).unwrap_err(), RosterError::PinMismatch { epoch: 3 });
    // `extended` itself refuses a roster that disagrees with a pin or is shorter, even if a caller skipped verify.
    let fork_roster = roster::verify_chain(ORG, STORE, &fork.files, &fork.pin_head()).unwrap();
    assert_eq!(pins4.extended(&fork_roster).unwrap_err(), RosterError::PinMismatch { epoch: 3 });
    let short_roster = roster::verify_chain(ORG, STORE, &t.files[..2], &at2.pin_head()).unwrap();
    assert!(matches!(pins4.extended(&short_roster).unwrap_err(), RosterError::PinnedEpochMissing { .. }));
    // A roster of another store is refused.
    let mut other = TestOrg::three_admins();
    other.store = "elsewhere".into();
    other.specs[0].store = "elsewhere".into();
    let f = other.specs[0].sign(&[&other.key("alice"), &other.key("bob"), &other.key("carol")]);
    let other_roster = roster::verify_chain(ORG, "elsewhere", &[f], &Anchor::new([(1, other.specs[0].hash())]).unwrap()).unwrap();
    assert!(matches!(pins4.extended(&other_roster).unwrap_err(), RosterError::WrongScope { .. }));
}

// ---- storage ----------------------------------------------------------------------------------------------

#[test]
fn file_storage_writes_atomically_with_private_modes() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("state/pins.toml");
    let storage = FilePinStorage::new(&path);
    assert!(storage.read().unwrap().is_none());
    let t = org4();
    let (pins, _) = join(&storage, &request(&t, "bob"), &t.files).unwrap();
    assert_eq!(load(&storage).unwrap().unwrap(), pins);
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1, "no temp file left behind");
    assert_eq!(join(&storage, &request(&t, "bob"), &t.files).unwrap_err(), RosterError::AlreadyPinned);
}

#[test]
fn a_refused_join_leaves_no_file_and_no_directory() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("xenia/acme/acme-main/state/pins.toml");
    let storage = FilePinStorage::new(&path);
    let t = org4();
    let bad = JoinRequest { admin_fingerprint: t.fingerprint("erin"), ..request(&t, "bob") };
    assert!(join(&storage, &bad, &t.files).is_err());
    assert!(!d.path().join("xenia").exists());
}

#[test]
fn write_atomic_replaces_whole_and_cleans_up() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("f.toml");
    write_atomic(&p, b"one").unwrap();
    write_atomic(&p, b"two, longer").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"two, longer");
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    // A target that is a directory is refused and leaves no temp file.
    let dir_target = d.path().join("sub");
    std::fs::create_dir(&dir_target).unwrap();
    assert!(write_atomic(&dir_target, b"x").is_err());
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2);
}

#[test]
fn write_atomic_never_follows_a_planted_symlink() {
    let d = tempfile::tempdir().unwrap();
    let victim = d.path().join("victim");
    std::fs::write(&victim, b"keep me").unwrap();
    let link = d.path().join("pins.toml");
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    assert!(write_atomic(&link, b"overwritten").is_err());
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
    // Reading through a symlink is refused as well, so a planted link cannot feed the daemon other pins.
    let storage = FilePinStorage::new(&link);
    assert!(storage.read().is_err());
}

#[test]
fn oversize_or_non_file_pins_are_refused_on_read() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("pins.toml");
    std::fs::write(&p, vec![b'#'; MAX_PINS_BYTES + 1]).unwrap();
    assert!(matches!(FilePinStorage::new(&p).read(), Err(RosterError::Oversize(_))));
    let dir = d.path().join("adir");
    std::fs::create_dir(&dir).unwrap();
    assert!(FilePinStorage::new(&dir).read().is_err());
    std::fs::write(&p, "not valid pins").unwrap();
    assert!(load(&FilePinStorage::new(&p)).is_err(), "a damaged pins file is an error, never 'not joined'");
}

#[test]
fn parse_head_takes_epoch_colon_hash_only() {
    let h = crate::hash::of(b"x");
    assert_eq!(JoinRequest::parse_head(&format!("7:{h}")).unwrap(), (7, h.clone()));
    for bad in [format!("07:{h}"), format!("0:{h}"), format!("+7:{h}"), format!("7 :{h}"), format!("7:{}", h.to_uppercase()), "7".into(), h.to_string(), format!("7:{h}:1"), String::new(), format!("-1:{h}")] {
        assert!(JoinRequest::parse_head(&bad).is_err(), "{bad}");
    }
}
