//! Tests for `delphi xenia admin ...`. Every store is a real git repository (a bare "host" in a temp dir), every
//! identity is generated, and every roster, statement and signature is made by the real library behind the real
//! command path; only the human at the card is a test double. The attacker is a hostile proposal branch, a non-admin,
//! a script that tries to answer for the operator, and the operator's own omissions.

use super::*;
use crate::xenia_cmd::{run_with_both, TEST_NOW};
use delphi_core::xenia_publish::{Approver, Decision, Preview};
use delphi_core::xenia_testkit::TestRemote;
use delphi_xenia::testkit::key;
use std::path::PathBuf;
use std::process::Command;

const ORG: &str = "acme";
const STORE: &str = "acme-main";

fn a(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The member approver is never reached by an admin command.
struct NoMember;

impl Approver for NoMember {
    fn decide(&mut self, _: &Preview) -> Decision {
        panic!("an admin command asked the member approver");
    }
}

/// The human at the card. `ack` is whether they also acknowledge the warnings.
struct Say {
    yes: bool,
    ack: bool,
    seen: Vec<AdminPreview>,
}

impl Say {
    fn yes() -> Say {
        Say { yes: true, ack: true, seen: Vec::new() }
    }
    fn no() -> Say {
        Say { yes: false, ack: true, seen: Vec::new() }
    }
}

impl AdminApprover for Say {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        self.seen.push(p.clone());
        if self.yes { AdminDecision::Approve(p.approve(self.ack)) } else { AdminDecision::Deny }
    }
}

fn go(args: &[&str], ap: &mut Say) -> i32 {
    run_with_both(&a(&[&["admin"], args].concat()), &mut NoMember, ap)
}

struct World {
    _tmp: tempfile::TempDir,
    remote: TestRemote,
    root: PathBuf,
    clock: i64,
}

fn t0() -> i64 {
    delphi_xenia::time::parse_utc("2026-10-26T12:00:00Z").unwrap()
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let remote = TestRemote::new(&tmp.path().join("host"));
        let w = World { root: tmp.path().to_path_buf(), _tmp: tmp, remote, clock: t0() };
        w.set_clock();
        w
    }

    fn set_clock(&self) {
        TEST_NOW.with(|c| c.set(self.clock));
    }

    /// Every statement must be newer than the last one an install accepted.
    fn tick(&mut self) {
        self.clock += 120;
        self.set_clock();
    }

    /// A home whose xenia key is `key(seed)`.
    fn home(&self, name: &str, seed: u8) -> String {
        let h = self.root.join(name);
        std::fs::create_dir_all(h.join("xenia")).unwrap();
        key(seed).store_new(&key_path(&h)).unwrap();
        h.to_str().unwrap().to_string()
    }

    fn host(&self) -> String {
        self.remote.host_path().to_str().unwrap().to_string()
    }

    fn refs(&self) -> String {
        let out = Command::new("git").arg("--git-dir").arg(self.host()).args(["for-each-ref", "--format=%(refname) %(objectname)"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn file(&self, name: &str) -> String {
        self.root.join(name).to_str().unwrap().to_string()
    }

    fn url(&self) -> String {
        self.remote.url()
    }
}

fn founder_arg(seed: u8, person: &str, role: &str) -> String {
    format!("{},{person},{person},{role}", key(seed).did())
}

/// The ceremony through the commands: draft, three signatures, publish, and every founder joins and syncs.
/// Returns the three homes (alice=1, bob=2, carol=3) and the draft's epoch hash.
fn genesis(w: &mut World) -> (String, String, String, String) {
    let (ha, hb, hc) = (w.home("alice", 1), w.home("bob", 2), w.home("carol", 3));
    let draft = w.file("draft.json");
    let (fa, fb, fc) = (founder_arg(1, "alice", "admin"), founder_arg(2, "bob", "admin"), founder_arg(3, "carol", "admin"));
    assert_eq!(go(&["genesis", "draft", "--org", ORG, "--store", STORE, "--founder", &fa, "--founder", &fb, "--founder", &fc, "--out", &draft], &mut Say::no()), 0);
    let mut sigs = Vec::new();
    for (name, h) in [("a", &ha), ("b", &hb), ("c", &hc)] {
        let out = w.file(&format!("sig-{name}.json"));
        let mut yes = Say::yes();
        assert_eq!(go(&["genesis", "sign", &draft, "--org", ORG, "--store", STORE, "--out", &out, "--home", h], &mut yes), 0);
        assert_eq!(yes.seen.len(), 1, "one card per founder");
        sigs.push(out);
    }
    let mut yes = Say::yes();
    let url = w.url();
    assert_eq!(go(&["genesis", "publish", &draft, "--url", &url, "--org", ORG, "--store", STORE, "--sig", &sigs[0], "--sig", &sigs[1], "--sig", &sigs[2], "--allow-local", "--home", &ha], &mut yes), 0);
    assert_eq!(yes.seen.len(), 1);
    let hash = GenesisDraft::from_bytes(ORG, STORE, &std::fs::read(&draft).unwrap()).unwrap().hash().to_string();
    let fp = key(1).fingerprint();
    for h in [&ha, &hb, &hc] {
        let mut no = Say::no();
        let code = run_with_both(&a(&["join", &url, "--org", ORG, "--store", STORE, "--admin", &fp, "--head", &format!("1:{hash}"), "--allow-local", "--home", h]), &mut NoMember, &mut no);
        assert_eq!(code, 0, "join for {h}");
        assert_eq!(run_with_both(&a(&["sync", "--home", h]), &mut NoMember, &mut no), 0);
    }
    (ha, hb, hc, hash)
}

fn sync(h: &str) {
    assert_eq!(run_with_both(&a(&["sync", "--home", h]), &mut NoMember, &mut Say::no()), 0);
}

fn roster_of(h: &str) -> delphi_xenia::roster::VerifiedRoster {
    let home = PathBuf::from(h);
    let r = delphi_core::xenia_status::list_stores(&home).remove(0);
    delphi_core::xenia_status::open(&home, &r).unwrap().accepted_roster().unwrap().unwrap()
}

#[test]
fn two_of_three_end_to_end_through_the_commands() {
    let mut w = World::new();
    let (ha, hb, hc, _) = genesis(&mut w);

    // add: alice proposes dave as a steward of rust; alice and bob sign; carol (who did not) is not needed.
    w.tick();
    let dave = key(4).did();
    assert_eq!(go(&["add", &dave, "--person", "dave", "--handle", "dave", "--steward", "rust", "--home", &ha], &mut Say::no()), 0, "propose needs no card and no key");
    let mut alice = Say::yes();
    assert_eq!(go(&["sign", "2", "--home", &ha], &mut alice), 0);
    // One signature is not a quorum: publish refuses before any card.
    let mut early = Say::yes();
    assert_eq!(go(&["publish", "2", "--home", &ha], &mut early), 1);
    assert!(early.seen.is_empty(), "no card for a publish that cannot succeed");
    let mut bob = Say::yes();
    assert_eq!(go(&["sign", "2", "--home", &hb], &mut bob), 0);
    assert_eq!(bob.seen.len(), 1);
    let card_text = card(&bob.seen[0]);
    assert!(card_text.contains("added") && card_text.contains(&dave), "{card_text}");
    w.tick();
    let mut publish = Say::yes();
    assert_eq!(go(&["publish", "2", "--home", &ha], &mut publish), 0);
    assert_eq!(publish.seen.len(), 1);
    sync(&hb);
    assert_eq!(roster_of(&hb).head_epoch(), 2);

    // rotate --compromised: carol's key is retired, the entry voids tombstones. Alice and bob sign, bob publishes.
    w.tick();
    let carol2 = key(13).did();
    assert_eq!(go(&["rotate", &key(3).did(), "--to", &carol2, "--compromised", "--home", &ha], &mut Say::no()), 0);
    let mut s1 = Say::yes();
    assert_eq!(go(&["sign", "3", "--home", &ha], &mut s1), 0);
    let p = &s1.seen[0];
    assert!(p.flags.iter().any(|f| f.slug() == "voids-tombstones"), "the card warns that the old key's withdrawals stop counting");
    assert!(p.reminders.iter().any(|r| r.slug() == "compromised-key-records"));
    let text = card(p);
    assert!(text.contains("WARNING voids-tombstones") && text.contains("Note (compromised-key-records)"), "{text}");
    assert_eq!(go(&["sign", "3", "--home", &hb], &mut Say::yes()), 0);
    w.tick();
    assert_eq!(go(&["publish", "3", "--home", &hb], &mut Say::yes()), 0);
    sync(&ha);
    let r = roster_of(&ha);
    assert_eq!(r.head_epoch(), 3);
    let old = r.head().epoch.member(&key(3).did()).unwrap();
    assert!(old.voids_tombstones && old.successor.as_deref() == Some(carol2.as_str()), "{old:?}");
    assert!(!old.is_active());

    // refresh: bob's key is an admin of the head. Carol's old key is not, so her refresh is refused with no card.
    w.tick();
    let mut refresh = Say::yes();
    assert_eq!(go(&["refresh", "--home", &hb], &mut refresh), 0);
    assert_eq!(refresh.seen.len(), 1);
    assert!(card(&refresh.seen[0]).contains("history verified before this card"));
    w.tick();
    sync(&hc);
    let mut carol = Say::yes();
    assert_eq!(go(&["refresh", "--home", &hc], &mut carol), 1);
    assert!(carol.seen.is_empty());

    // status sees the new roster on a member's install.
    assert_eq!(run_with_both(&a(&["status", "--home", &hc]), &mut NoMember, &mut Say::no()), 0);
}

#[test]
fn rotate_needs_exactly_one_kind_and_changes_nothing_without_it() {
    let mut w = World::new();
    let (ha, _, _, _) = genesis(&mut w);
    w.tick();
    let before = w.refs();
    let (old, new) = (key(3).did(), key(13).did());
    assert_eq!(go(&["rotate", &old, "--to", &new, "--home", &ha], &mut Say::no()), 2);
    assert_eq!(go(&["rotate", &old, "--to", &new, "--routine", "--compromised", "--home", &ha], &mut Say::no()), 2);
    assert_eq!(go(&["propose", "rotate", &old, "--to", &new, "--home", &ha], &mut Say::no()), 2);
    assert_eq!(w.refs(), before, "a refused command pushes nothing");
    assert!(delphi_xenia::admin::Rotation::from_flags(false, false).is_err());
    assert_eq!(delphi_xenia::admin::Rotation::from_flags(true, false), Ok(Rotation::Routine));
    assert_eq!(delphi_xenia::admin::Rotation::from_flags(false, true), Ok(Rotation::Compromised));
    // The spelled-out form works with a kind.
    assert_eq!(go(&["propose", "rotate", &old, "--to", &new, "--routine", "--home", &ha], &mut Say::no()), 0);
}

#[test]
fn a_no_signs_and_sends_nothing() {
    let mut w = World::new();
    let (ha, hb, _, _) = genesis(&mut w);
    w.tick();
    assert_eq!(go(&["add", &key(4).did(), "--person", "dave", "--handle", "dave", "--home", &ha], &mut Say::no()), 0);
    let before = w.refs();
    let mut no = Say::no();
    assert_eq!(go(&["sign", "2", "--home", &hb], &mut no), 1);
    assert_eq!(no.seen.len(), 1, "the card was shown once and answered no");
    assert_eq!(w.refs(), before);
}

#[test]
fn nothing_but_a_terminal_answers_for_the_operator() {
    let mut w = World::new();
    let (ha, hb, _, _) = genesis(&mut w);
    w.tick();
    assert_eq!(go(&["add", &key(4).did(), "--person", "dave", "--handle", "dave", "--home", &ha], &mut Say::no()), 0);
    let before = w.refs();
    // The real command with the real approver, every "answer" an attacker could think of: flags, environment, a pipe.
    // SAFETY: tests in this process never read these variables.
    unsafe {
        std::env::set_var("DELPHI_YES", "1");
        std::env::set_var("XENIA_APPROVE", "1");
        std::env::set_var("CI", "true");
    }
    if unsafe { libc::isatty(0) == 0 } {
        let args = a(&["admin", "sign", "2", "--yes", "--approve", "--code", "000000000000", "--home", &hb]);
        // `--yes` has no meaning: it is parsed as an option with a value and answers nothing.
        let code = crate::xenia_cmd::run(&args);
        assert_ne!(code, 0);
        let mut ap = TtyAdminApprover;
        let preview = fake_preview();
        assert_eq!(ap.decide(&preview), AdminDecision::Deny, "stdin is not a terminal");
    }
    assert_eq!(w.refs(), before, "no signature reached the store");
}

fn fake_preview() -> AdminPreview {
    AdminPreview {
        action: xenia_admin::Action::SignEpoch,
        org: ORG.into(),
        store: STORE.into(),
        epoch: 2,
        signer: key(2).did(),
        files: vec![],
        signs: vec![],
        changes: vec![],
        flags: vec![],
        reminders: vec![],
        quorum: None,
        store_head: None,
        expires_at: None,
        walked: vec![],
        digest: "blake3:0123456789abcdef".into(),
    }
}

#[test]
fn a_hostile_proposal_is_shown_with_its_warnings_and_cannot_be_signed_unread() {
    let mut w = World::new();
    let (ha, hb, _, _) = genesis(&mut w);
    w.tick();
    // The attacker needs no key to propose: mallory asks to become an admin, and a steward of everything.
    let mallory = key(66).did();
    assert_eq!(go(&["add", &mallory, "--person", "mallory", "--handle", "mallory", "--as-admin", "--steward", "*", "--home", &ha], &mut Say::no()), 0);
    let before = w.refs();
    // Bob is asked. He approves but does not acknowledge the warnings: the library refuses, and nothing is pushed.
    let mut unread = Say { yes: true, ack: false, seen: Vec::new() };
    assert_eq!(go(&["sign", "2", "--home", &hb], &mut unread), 1);
    assert_eq!(w.refs(), before);
    let text = card(&unread.seen[0]);
    assert!(text.contains("WARNING adds-admin") && text.contains("WARNING widens-areas"), "{text}");
    assert!(text.contains(&mallory) && text.contains("added"), "every change to the signer set is listed: {text}");
    assert!(text.contains("Approval code:") && text.contains(&code(&unread.seen[0])));
}

#[test]
fn a_member_who_is_not_an_admin_is_refused_before_any_card() {
    let mut w = World::new();
    let (ha, _, _, hash) = genesis(&mut w);
    w.tick();
    assert_eq!(go(&["add", &key(4).did(), "--person", "dave", "--handle", "dave", "--home", &ha], &mut Say::no()), 0);
    // A stranger's install joins (no admin key of its own) and tries every admin word.
    let hs = w.home("stranger", 99);
    let url = w.url();
    assert_eq!(run_with_both(&a(&["join", &url, "--org", ORG, "--store", STORE, "--admin", &key(1).fingerprint(), "--head", &format!("1:{hash}"), "--allow-local", "--home", &hs]), &mut NoMember, &mut Say::no()), 0);
    let before = w.refs();
    for args in [vec!["sign", "2"], vec!["publish", "2"], vec!["refresh"]] {
        let mut ap = Say::yes();
        let mut full: Vec<&str> = args.clone();
        full.extend(["--home", &hs]);
        assert_eq!(go(&full, &mut ap), 1, "{args:?}");
        assert!(ap.seen.is_empty(), "{args:?}: refused before there was anything to show");
    }
    assert_eq!(w.refs(), before);
}

#[test]
fn genesis_refuses_a_missing_founder_a_stranger_and_an_overwrite() {
    let w = World::new();
    let (ha, hb) = (w.home("alice", 1), w.home("bob", 2));
    let draft = w.file("draft.json");
    let (fa, fb) = (founder_arg(1, "alice", "admin"), founder_arg(2, "bob", "admin"));
    assert_eq!(go(&["genesis", "draft", "--org", ORG, "--store", STORE, "--founder", &fa, "--founder", &fb, "--out", &draft], &mut Say::no()), 0);
    assert_eq!(go(&["genesis", "draft", "--org", ORG, "--store", STORE, "--founder", &fa, "--out", &draft], &mut Say::no()), 2, "an existing draft is never overwritten");
    assert_eq!(go(&["genesis", "draft", "--org", ORG, "--store", STORE, "--founder", "nonsense", "--out", &w.file("x")], &mut Say::no()), 2);
    // A stranger is not a founder.
    let hm = w.home("mallory", 66);
    let mut ap = Say::yes();
    assert_eq!(go(&["genesis", "sign", &draft, "--org", ORG, "--store", STORE, "--home", &hm], &mut ap), 1);
    assert!(ap.seen.is_empty());
    // Only alice signs: bob's signature is missing, so nothing is published.
    let sig = w.file("sig-a.json");
    assert_eq!(go(&["genesis", "sign", &draft, "--org", ORG, "--store", STORE, "--out", &sig, "--home", &ha], &mut Say::yes()), 0);
    let url = w.url();
    let mut ap = Say::yes();
    assert_eq!(go(&["genesis", "publish", &draft, "--url", &url, "--org", ORG, "--store", STORE, "--sig", &sig, "--allow-local", "--home", &ha], &mut ap), 1);
    assert!(ap.seen.is_empty(), "the missing signature is found before the card");
    assert_eq!(w.refs(), "");
    // A draft for another org does not parse under this one's name.
    assert_eq!(go(&["genesis", "sign", &draft, "--org", "other", "--store", STORE, "--home", &hb], &mut Say::yes()), 1);
    // A local path is refused without --allow-local.
    assert_eq!(go(&["genesis", "publish", &draft, "--url", &url, "--org", ORG, "--store", STORE, "--sig", &sig, "--home", &ha], &mut Say::yes()), 2);
}

#[test]
fn removing_an_unknown_key_and_other_mistakes_are_refused() {
    let mut w = World::new();
    let (ha, _, _, _) = genesis(&mut w);
    w.tick();
    let before = w.refs();
    // Three admins: removing one leaves two (the local minimum); removing a second leaves one.
    assert_eq!(go(&["remove", &key(3).did(), "--home", &ha], &mut Say::no()), 0);
    let n_after_first = w.refs();
    assert_ne!(n_after_first, before);
    // Unknown DID.
    assert_eq!(go(&["remove", &key(77).did(), "--home", &ha], &mut Say::no()), 1);
    // Bad number and missing arguments are usage errors.
    assert_eq!(go(&["sign", "x", "--home", &ha], &mut Say::yes()), 2);
    assert_eq!(go(&["sign", "1", "--home", &ha], &mut Say::yes()), 2);
    assert_eq!(go(&["add", "--home", &ha], &mut Say::no()), 2);
    assert_eq!(go(&["bogus"], &mut Say::no()), 2);
    assert_eq!(go(&[], &mut Say::no()), 2);
}
