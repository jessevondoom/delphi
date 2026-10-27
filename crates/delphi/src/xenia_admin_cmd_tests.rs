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
        rewrite: None,
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

// --- X10: the 3-admin, 2-of-3 chain (spec 011 §1.4 gate 3) --------------------------------------------------------

/// One epoch through the commands: `proposer` proposes, each of `signers` signs its card, `publisher` publishes, and
/// every home in `then_sync` syncs. Ticks the clock before the statement is made.
fn epoch(w: &mut World, n: &str, propose: &[&str], proposer: &str, signers: &[&str], publisher: &str, then_sync: &[&str]) {
    w.tick();
    let mut args: Vec<&str> = propose.to_vec();
    args.extend(["--home", proposer]);
    assert_eq!(go(&args, &mut Say::no()), 0, "propose {propose:?}");
    for s in signers {
        assert_eq!(go(&["sign", n, "--home", s], &mut Say::yes()), 0, "sign epoch {n}");
    }
    w.tick();
    assert_eq!(go(&["publish", n, "--home", publisher], &mut Say::yes()), 0, "publish epoch {n}");
    for h in then_sync {
        sync(h);
    }
}

fn host_grep(host_git_dir: &str, needle: &str) -> bool {
    Command::new("git").arg("--git-dir").arg(host_git_dir).args(["grep", "-q", needle, "refs/heads/main", "--", "roster"]).status().unwrap().success()
}

fn copy_dir(from: &str, to: &str) {
    assert!(Command::new("cp").args(["-R", from, to]).status().unwrap().success());
}

#[test]
fn a_three_admin_two_of_three_chain_survives_a_fork_and_an_install_that_slept_through_four_epochs() {
    let mut w = World::new();
    let (ha, hb, hc, genesis_hash) = genesis(&mut w);
    assert_eq!(roster_of(&hc).head_epoch(), 1);
    let host = w.host();
    let (dave, erin, frank, carol2) = (key(4).did(), key(6).did(), key(5).did(), key(13).did());

    // Epoch 2: alice proposes dave (steward of rust); alice and bob sign; bob publishes. Carol is never asked.
    epoch(&mut w, "2", &["add", &dave, "--person", "dave", "--handle", "dave", "--steward", "rust"], &ha, &[&ha, &hb], &hb, &[&ha, &hb]);
    assert_eq!(roster_of(&ha).head_epoch(), 2);

    // -- the fork. A second copy of the host stands for the rival history that another admin pair could build.
    let host_b = w.file("host-b.git");
    copy_dir(&host, &host_b);
    let url_b = format!("file://{host_b}");

    // Same host, same epoch number: alice's proposal takes the branch; bob's rival cannot even be written, and the first is untouched.
    w.tick();
    assert_eq!(go(&["add", &erin, "--person", "erin", "--handle", "erin", "--home", &ha], &mut Say::no()), 0);
    let with_alice = w.refs();
    assert_eq!(go(&["add", &frank, "--person", "frank", "--handle", "frank", "--home", &hb], &mut Say::no()), 1, "a second proposal for epoch 3 is refused");
    assert_eq!(w.refs(), with_alice, "the rival wrote nothing");
    for h in [&ha, &hb] {
        assert_eq!(go(&["sign", "3", "--home", h], &mut Say::yes()), 0);
    }
    w.tick();
    assert_eq!(go(&["publish", "3", "--home", &hb], &mut Say::yes()), 0);
    // Publishing the same epoch again, or a signature collected for it, cannot move main: it is stale now.
    let main_a = w.refs();
    w.tick();
    assert_eq!(go(&["publish", "3", "--home", &ha], &mut Say::yes()), 1, "epoch 3 is taken");
    assert_eq!(w.refs(), main_a);
    sync(&ha);
    sync(&hb);
    assert_eq!(roster_of(&hb).head_epoch(), 3);

    // The rival history on the other host: alice and carol (a different pair) publish a different epoch 3, adding frank.
    let (ha2, hc2) = (w.home("alice-b", 1), w.home("carol-b", 3));
    for h in [&ha2, &hc2] {
        let mut no = Say::no();
        let fp = key(1).fingerprint();
        let code = run_with_both(&a(&["join", &url_b, "--org", ORG, "--store", STORE, "--admin", &fp, "--head", &format!("1:{genesis_hash}"), "--allow-local", "--home", h]), &mut NoMember, &mut no);
        assert_eq!(code, 0);
        sync(h);
        assert_eq!(roster_of(h).head_epoch(), 2, "the copy holds epochs 1 and 2 only");
    }
    epoch(&mut w, "3", &["add", &frank, "--person", "frank", "--handle", "frank"], &ha2, &[&ha2, &hc2], &hc2, &[&ha2, &hc2]);
    assert_eq!(roster_of(&hc2).head_epoch(), 3);
    assert!(roster_of(&hc2).head().epoch.member(&frank).is_some());

    // Only one wins. An install that accepted the first epoch 3 is served the rival history: refused, state unchanged.
    let aside = w.file("host-a-aside.git");
    std::fs::rename(&host, &aside).unwrap();
    copy_dir(&host_b, &host);
    w.tick();
    let code = run_with_both(&a(&["sync", "--home", &hb]), &mut NoMember, &mut Say::no());
    assert_ne!(code, 0, "a forked epoch is refused");
    let r = roster_of(&hb);
    assert_eq!(r.head_epoch(), 3);
    assert!(r.head().epoch.member(&erin).is_some() && r.head().epoch.member(&frank).is_none(), "the first epoch 3 still stands");
    std::fs::remove_dir_all(&host).unwrap();
    std::fs::rename(&aside, &host).unwrap();
    // And the rival never reached main of the real host.
    assert!(host_grep(&host, &erin) && !host_grep(&host, &frank));

    // -- three more epochs while carol's install (pinned at epoch 1) is asleep.
    epoch(&mut w, "4", &["rotate", &key(3).did(), "--to", &carol2, "--routine"], &ha, &[&ha, &hb], &ha, &[&ha, &hb]);
    epoch(&mut w, "5", &["remove", &erin], &hb, &[&hb, &ha], &hb, &[&ha, &hb]);
    assert_eq!(roster_of(&ha).head_epoch(), 5);
    assert_eq!(roster_of(&hc).head_epoch(), 1, "the sleeper has not moved");

    // Offline: the host is gone. The sync fails and changes nothing.
    std::fs::rename(&host, &aside).unwrap();
    w.tick();
    // (The command reports "could not be reached" and exits 0 by design: being offline is not an alarm.)
    assert_eq!(run_with_both(&a(&["sync", "--home", &hc]), &mut NoMember, &mut Say::no()), 0);
    assert_eq!(roster_of(&hc).head_epoch(), 1);
    std::fs::rename(&aside, &host).unwrap();

    // Back online: the chain 1 -> 2 -> 3 -> 4 -> 5 verifies from its pin, and the statement for epoch 5 is fresh.
    w.tick();
    sync(&hc);
    let r = roster_of(&hc);
    assert_eq!(r.head_epoch(), 5);
    let head = &r.head().epoch;
    assert!(head.member(&dave).is_some_and(|m| m.is_active()));
    assert!(head.member(&erin).is_some_and(|m| !m.is_active()), "erin was removed in epoch 5");
    assert!(head.member(&frank).is_none(), "the rival's member exists in no accepted epoch");
    assert!(head.member(&key(3).did()).is_some_and(|m| !m.is_active() && m.successor.as_deref() == Some(carol2.as_str())));
    assert_eq!(run_with_both(&a(&["status", "--home", &hc]), &mut NoMember, &mut Say::no()), 0);

    // A brand-new install given the pinned pair from the day of genesis joins and reaches epoch 5 the same way.
    let hd = w.home("dana", 20);
    let fp = key(1).fingerprint();
    assert_eq!(run_with_both(&a(&["join", &w.url(), "--org", ORG, "--store", STORE, "--admin", &fp, "--head", &format!("1:{genesis_hash}"), "--allow-local", "--home", &hd]), &mut NoMember, &mut Say::no()), 0);
    sync(&hd);
    assert_eq!(roster_of(&hd).head_epoch(), 5);
}

// ---------------------------------------------------------------------------------------------------------
// Rewrite recovery, end to end through the commands (spec 011 section 8.4)

/// The person at an install's terminal: types what the prompt asks for (or `Some(text)` instead).
struct Typist {
    reply: Option<&'static str>,
    shown: String,
    asked: usize,
}

impl Typist {
    fn honest() -> Typist {
        Typist { reply: None, shown: String::new(), asked: 0 }
    }
}

impl crate::xenia_terminal::Terminal for Typist {
    fn interactive(&self) -> Result<(), String> {
        Ok(())
    }
    fn show(&mut self, text: &str) {
        self.shown.push_str(text);
    }
    fn ask(&mut self, prompt: &str) -> Option<String> {
        self.asked += 1;
        if let Some(r) = self.reply {
            return Some(r.to_string());
        }
        let i = prompt.rfind("type ")? + 5;
        let rest = &prompt[i..];
        Some(rest[..rest.find("; anything")?].to_string())
    }
}

fn proposal_ids(w: &World) -> Vec<String> {
    w.refs().lines().filter_map(|l| l.split(' ').next()?.strip_prefix("refs/heads/rewrite-proposal/")).map(str::to_string).collect()
}

fn open_home(home: &str) -> delphi_core::xenia_store::Store {
    let home = PathBuf::from(home);
    let r = delphi_core::xenia_status::list_stores(&home).remove(0);
    delphi_core::xenia_status::open(&home, &r).unwrap()
}

/// `delphi xenia sync --accept-rewritten-head` for the home, with the person at `term`.
fn recover(w: &World, home: &str, term: &mut Typist) -> (i32, String, String) {
    let hp = PathBuf::from(home);
    let r = delphi_core::xenia_status::list_stores(&hp).remove(0);
    let store = delphi_core::xenia_status::open(&hp, &r).unwrap();
    let cfg = VerifyConfig::at(w.clock);
    let outcome = store.sync_verified(&cfg).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = crate::xenia_recover::recover_with(&r, &store, &cfg, &outcome, term, &mut out, &mut err);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

#[test]
fn a_force_pushed_store_is_recovered_through_propose_sign_publish_and_the_installs_accept() {
    let mut w = World::new();
    let (ha, hb, hc, hash) = genesis(&mut w);
    // A member who is not an admin: joined, never allowed to author.
    let he = w.home("erin", 9);
    let fp = key(1).fingerprint();
    let url = w.url();
    assert_eq!(run_with_both(&a(&["join", &url, "--org", ORG, "--store", STORE, "--admin", &fp, "--head", &format!("1:{hash}"), "--allow-local", "--home", &he]), &mut NoMember, &mut Say::no()), 0);

    // The store holds a signed note; everyone accepts it.
    w.remote.git(&["fetch", "--quiet", &w.host(), "main"]);
    w.remote.reset_hard("FETCH_HEAD");
    let genesis_head = w.remote.head();
    let note = delphi_core::xenia_fixture::records::memory(&key(1), &delphi_core::xenia_fixture::u(0), 1, "active", "note");
    for (p, b) in note.files() {
        w.remote.write(p, b);
    }
    w.remote.commit("a signed note");
    w.tick();
    for h in [&ha, &hb, &hc, &he] {
        sync(h);
    }
    let pins_before = std::fs::read(open_home(&hc).paths.state.join("pins.toml")).unwrap();
    assert!(open_home(&hc).load_sticky().unwrap().paths.contains_key(&note.path), "the note is tracked");

    // The host force-pushes a history without the note: a normal sync refuses and applies nothing.
    w.remote.reset_hard(&genesis_head);
    let rewritten = w.remote.commit("history rewritten");
    w.tick();
    assert_ne!(run_with_both(&a(&["sync", "--home", &hc]), &mut NoMember, &mut Say::no()), 0, "a force-push is refused");
    let mut t = Typist::honest();
    let (code, _, err) = recover(&w, &hc, &mut t);
    assert_eq!(code, 1, "no statement yet: {err}");
    assert!(err.contains("rewrite-quorum-missing") && t.asked == 0, "{err}");

    // propose: an admin; a non-admin is refused with no card and nothing pushed.
    let before = w.refs();
    let mut none = Say::yes();
    assert_eq!(go(&["rewrite", "propose", "--home", &he], &mut none), 1);
    assert!(none.seen.is_empty());
    assert_eq!(w.refs(), before);
    assert_eq!(go(&["rewrite", "propose", "--home", &ha], &mut Say::no()), 0, "propose needs a key but no card: nothing is signed");
    let ids = proposal_ids(&w);
    assert_eq!(ids.len(), 1, "{:?}", w.refs());
    let id = ids[0].clone();
    assert!(id.starts_with(&rewritten[..12]));

    // One signature is not a quorum; a non-admin cannot sign; the card says what is being asked.
    let mut alice = Say::yes();
    assert_eq!(go(&["rewrite", "sign", &id, "--home", &ha], &mut alice), 0);
    let card_text = card(&alice.seen[0]);
    assert!(card_text.contains(&rewritten) && card_text.contains(&note.path), "{card_text}");
    assert!(card_text.contains("replaced head") && card_text.contains("rewritten head"), "{card_text}");
    assert!(card_text.contains("will list every affected path and require its own operator"), "{card_text}");
    assert!(card_text.contains("WARNING rewrites-history"), "{card_text}");
    assert_eq!(go(&["rewrite", "sign", &id, "--home", &he], &mut none), 1);
    let mut early = Say::yes();
    assert_eq!(go(&["rewrite", "publish", &id, "--home", &hb], &mut early), 1);
    assert!(early.seen.is_empty(), "no card for a publish that cannot succeed");
    assert_eq!(go(&["rewrite", "show", &id, "--home", &hb], &mut Say::no()), 0);
    // A card whose warning is not acknowledged signs nothing.
    let mut cold = Say { yes: true, ack: false, seen: Vec::new() };
    assert_eq!(go(&["rewrite", "sign", &id, "--home", &hb], &mut cold), 1);
    assert_eq!(go(&["rewrite", "sign", &id, "--home", &hb], &mut Say::yes()), 0);
    let mut publish = Say::yes();
    assert_eq!(go(&["rewrite", "publish", &id, "--home", &hb], &mut publish), 0);
    assert_eq!(publish.seen.len(), 1);

    // The install with the history: it lists the path, wants the commit and the count typed, and accepts exactly that.
    let published = w.remote.host_head();
    assert_ne!(published, rewritten);
    let mut wrong = Typist { reply: Some("accept 000000000000 99"), shown: String::new(), asked: 0 };
    let (code, _, _) = recover(&w, &hc, &mut wrong);
    assert_eq!(code, 1, "a wrong answer changes nothing");
    assert!(wrong.shown.contains(&note.path), "the path is listed before anything is asked: {}", wrong.shown);
    assert_eq!(open_home(&hc).accepted_head().unwrap().as_deref(), Some(open_home(&ha).accepted_head().unwrap().unwrap().as_str()), "still on the old history");
    let mut person = Typist::honest();
    let (code, out, err) = recover(&w, &hc, &mut person);
    assert_eq!(code, 0, "{err}");
    assert!(person.shown.contains(&note.path), "{}", person.shown);
    assert!(out.contains("accepted the rewritten head"), "{out}");
    let store = open_home(&hc);
    assert_eq!(store.accepted_head().unwrap().as_deref(), Some(published.as_str()));
    assert_eq!(std::fs::read(store.paths.state.join("pins.toml")).unwrap(), pins_before, "pinned roster hashes are untouched");
    assert!(!store.load_sticky().unwrap().paths.contains_key(&note.path), "the approved path started over");
    assert_eq!(run_with_both(&a(&["sync", "--home", &hc]), &mut NoMember, &mut Say::no()), 0, "an ordinary store again");
}
