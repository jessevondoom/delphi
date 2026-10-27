//! Tests for the console-admin rules. The classification is judged over real epochs built by the real admin code; a
//! diff that looks routine but is not (a second key for an admin person) is built by hand, the way a hostile
//! proposal branch would carry it.

use super::*;
use delphi_xenia::admin::{self, NewMember, Operation, Policy, Rotation};
use delphi_xenia::roster::Role;
use delphi_xenia::testkit::TestOrg;

const NOW: &str = "2026-10-26T12:00:00Z";

fn head_of(t: &TestOrg) -> (Epoch, String) {
    let f = t.files.last().unwrap();
    (Epoch::parse(&f.epoch_json).unwrap(), delphi_xenia::hash::of(&f.epoch_json))
}

fn org() -> TestOrg {
    TestOrg::three_admins()
}

fn next(t: &TestOrg, op: &Operation) -> (Epoch, Epoch) {
    let (head, hh) = head_of(t);
    let p = admin::build_next(&head, &hh, op, NOW, &Policy::default()).unwrap();
    (head, p.epoch)
}

fn add(t: &mut TestOrg, name: &str, person: &str, admin: bool) -> Operation {
    let id = t.who(name);
    Operation::Add(NewMember { did: id.did(), person: person.into(), handle: person.into(), admin, steward_areas: vec![] })
}

#[test]
fn a_plain_member_may_be_added_or_removed_at_the_console() {
    let mut t = org();
    let op = add(&mut t, "gail", "gail", false);
    let (b, c) = next(&t, &op);
    assert_eq!(classify(&b, &c, 2), Ok(()));
    let erin = t.did("erin");
    let (b, c) = next(&t, &Operation::Remove { did: erin, for_cause: false });
    assert_eq!(classify(&b, &c, 2), Ok(()));
}

#[test]
fn anything_that_touches_an_admin_is_terminal_only() {
    let mut t = org();
    let op = add(&mut t, "gail", "gail", true);
    let (b, c) = next(&t, &op);
    assert_eq!(classify(&b, &c, 2), Err(TerminalOnly::AdminTouched), "a new admin");
    let (b, c) = next(&t, &Operation::Remove { did: t.did("carol"), for_cause: false });
    assert_eq!(classify(&b, &c, 2), Err(TerminalOnly::AdminTouched), "removing an admin");
    let new = t.who("alice2");
    let (b, c) = next(&t, &Operation::Rotate { old_did: t.did("alice"), new_did: new.did(), kind: Rotation::Routine });
    assert_eq!(classify(&b, &c, 2), Err(TerminalOnly::AdminTouched), "rotating an admin key changes the admin set");
}

#[test]
fn a_rotation_of_a_plain_member_and_a_compromise_are_terminal_only() {
    let mut t = org();
    let new = t.who("dave2");
    let (b, c) = next(&t, &Operation::Rotate { old_did: t.did("dave"), new_did: new.did(), kind: Rotation::Routine });
    assert_eq!(classify(&b, &c, 2), Err(TerminalOnly::Rotation));
    let new = t.who("dave3");
    let (b, c) = next(&t, &Operation::Rotate { old_did: t.did("dave"), new_did: new.did(), kind: Rotation::Compromised });
    assert!(classify(&b, &c, 2).is_err());
    let (b, c) = next(&t, &Operation::Remove { did: t.did("erin"), for_cause: true });
    assert_eq!(classify(&b, &c, 2), Err(TerminalOnly::Compromise), "a removal for cause voids the entry's withdrawals");
}

#[test]
fn a_diff_that_looks_routine_but_adds_a_second_key_for_an_admin_person_is_terminal_only() {
    let mut t = org();
    let (head, _) = head_of(&t);
    let id = t.who("mallory");
    let mut cand = head.clone();
    cand.epoch += 1;
    let mut m = head.members.iter().find(|m| m.did == t.did("alice")).unwrap().clone();
    // The new entry is a plain member: nothing in the admin set changes, only a second key appears under alice's person.
    m.did = id.did();
    m.roles = vec![Role::Member];
    cand.members.push(m);
    assert_eq!(classify(&head, &cand, 2), Err(TerminalOnly::KeyForAdminPerson));
    // The same entry under a new person is an ordinary add.
    let mut cand2 = head.clone();
    cand2.epoch += 1;
    let mut m2 = cand.members.last().unwrap().clone();
    m2.person = "someone-else".into();
    cand2.members.push(m2);
    assert_eq!(classify(&head, &cand2, 2), Ok(()));
}

#[test]
fn the_switch_is_off_unless_the_operator_file_says_true() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    assert!(!console_admin_enabled(home), "no file");
    let write = |text: &str| {
        std::fs::create_dir_all(home.join("operator")).unwrap();
        let _ = std::fs::remove_file(home.join("operator/xenia.toml"));
        std::fs::write(home.join("operator/xenia.toml"), text).unwrap();
    };
    for off in ["", "[xenia]\nconsole_admin = false\n", "[xenia]\nconsole_admin = \"true\"\n", "[other]\nconsole_admin = true\n", "console_admin = true\n", "not toml {{"] {
        write(off);
        assert!(!console_admin_enabled(home), "{off:?}");
    }
    write("[xenia]\nconsole_admin = true\n");
    assert!(console_admin_enabled(home));
    // A symlink is never followed.
    std::fs::remove_file(home.join("operator/xenia.toml")).unwrap();
    std::fs::write(home.join("real.toml"), "[xenia]\nconsole_admin = true\n").unwrap();
    std::os::unix::fs::symlink(home.join("real.toml"), home.join("operator/xenia.toml")).unwrap();
    assert!(!console_admin_enabled(home));
}

#[test]
fn every_terminal_only_reason_has_its_own_message_that_names_a_next_step() {
    for t in [TerminalOnly::AdminTouched, TerminalOnly::KeyForAdminPerson, TerminalOnly::Rotation, TerminalOnly::Compromise, TerminalOnly::FewerAdmins] {
        let m = message(t.slug());
        assert!(m.contains("{command}") && m.contains("terminal"), "{}: {m}", t.slug());
        assert_ne!(m, message("no-such-slug"));
    }
    for s in ["switch-off", "pending-card", "rate-limit", "stale", "no-card", "wrong-confirm", "flags-unacknowledged", "no-key", "terminal-genesis", "terminal-rewrite", "not-admin"] {
        assert_ne!(message(s), message("no-such-slug"), "{s}");
    }
    assert!(fill("stale", "", "7").contains("admin show 7"));
}

#[test]
fn the_rate_limit_counts_only_console_signatures_in_the_last_hour() {
    let now = 1_000_000;
    let e = vec![
        ("admin", "signed", "console", now - 10),
        ("admin", "published", "console", now - 3599),
        ("admin", "signed", "console", now - 3601),
        ("admin", "proposed", "console", now - 10),
        ("admin", "signed", "terminal", now - 10),
        ("sync", "signed", "console", now - 10),
        ("admin", "refreshed", "console", now - 100),
    ];
    assert_eq!(recent_console_signatures(e, now), 3);
}
