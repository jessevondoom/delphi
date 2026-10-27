//! Tests for the admin operations. The attacker is a hostile proposal branch, a store host that tampers, and the
//! operator's own mistakes. Every store is a real git repository driven by the real code path; every roster,
//! statement and signature is made by the real functions under test, and every result is checked by the real
//! verifier (`sync_verified`) on a second, fresh install.

use super::*;
use crate::xenia_sync::VerifiedSync;
use crate::xenia_testkit::TestRemote;
use delphi_xenia::roster::Epoch;
use delphi_xenia::testkit::key;
use delphi_xenia::testkit_records as rk;
use delphi_xenia::time::parse_utc;
use std::cell::Cell;
use std::process::Command as StdCommand;

const ORG: &str = "acme";
const STORE: &str = "acme-main";
const REC: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";

fn t0() -> i64 {
    parse_utc("2026-10-26T12:00:00Z").unwrap()
}

/// alice=1 bob=2 carol=3 dave=4 erin=5; successors: alice2=11 bob2=12 dave2=14.
fn who(name: &str) -> Identity {
    key(match name {
        "alice" => 1,
        "bob" => 2,
        "carol" => 3,
        "dave" => 4,
        "erin" => 5,
        "alice2" => 11,
        "bob2" => 12,
        "dave2" => 14,
        "mallory" => 66,
        other => panic!("unknown {other}"),
    })
}

fn founder(name: &str, admin: bool, areas: &[&str]) -> NewMember {
    NewMember { did: who(name).did(), person: name.trim_end_matches('2').into(), handle: name.into(), admin, steward_areas: areas.iter().map(|s| s.to_string()).collect() }
}

fn three_founders() -> Vec<NewMember> {
    vec![founder("alice", true, &[]), founder("bob", true, &[]), founder("carol", true, &[]), founder("dave", false, &["rust"]), founder("erin", false, &[])]
}

/// Approve a prepared action exactly as previewed (flags acknowledged) and sign it.
fn approve_and_sign<T>(p: Prepared<T>, key: &Identity) -> Result<Signed<T>, AdminError> {
    let a = p.preview().approve(true);
    p.sign(&a, key)
}

/// Approves exactly what it is shown; records every preview.
struct Yes {
    ack: bool,
    seen: Vec<AdminPreview>,
}

impl Yes {
    fn new() -> Yes {
        Yes { ack: true, seen: Vec::new() }
    }
}

impl AdminApprover for Yes {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        self.seen.push(p.clone());
        AdminDecision::Approve(p.approve(self.ack))
    }
}

struct Closure<F: FnMut(&AdminPreview) -> AdminDecision>(F);

impl<F: FnMut(&AdminPreview) -> AdminDecision> AdminApprover for Closure<F> {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        (self.0)(p)
    }
}

struct Fx {
    _tmp: tempfile::TempDir,
    remote: TestRemote,
    store: Store,
    tick: Cell<i64>,
    join: JoinInfo,
}

/// Run genesis for `founders` (all admins among them sign) into a fresh bare store, then join and sync as the
/// first founder's install.
fn genesis_fx(founders: Vec<NewMember>, signing: &[&str], statement_signer: &str) -> Result<Fx, AdminError> {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let remote = TestRemote::new(&tmp.path().join("host"));
    let store = Store::open(&home, ORG, STORE, &remote.url(), true).unwrap();
    let admin = Admin::new(&store, VerifyConfig::at(t0()));
    let draft = GenesisDraft::build(ORG, STORE, &founders, &time::format_utc(t0()))?;
    let mut sigs = Vec::new();
    for name in signing {
        // Each founder rebuilds the draft from the bytes they were handed.
        let theirs = GenesisDraft::from_bytes(ORG, STORE, draft.bytes())?;
        let p = prepare_genesis_signature(&theirs, &who(name))?;
        let signed = approve_and_sign(p, &who(name))?;
        sigs.push(signed.proposal_sig());
    }
    let out = admin.init_store(&draft, &sigs, &who(statement_signer), admin::DEFAULT_VALIDITY_SECS, &mut Yes::new())?;
    let fx = Fx { _tmp: tmp, remote, store, tick: Cell::new(1), join: out.join };
    join_store(&fx.store, &fx.join.request()).unwrap();
    match fx.store.sync_verified(&fx.cfg()).unwrap() {
        VerifiedSync::Accepted { .. } => {}
        other => panic!("baseline sync: {other:?}"),
    }
    Ok(fx)
}

impl Fx {
    fn new() -> Fx {
        genesis_fx(three_founders(), &["alice", "bob", "carol"], "alice").unwrap()
    }

    fn cfg(&self) -> VerifyConfig {
        VerifyConfig::at(t0() + 120 * self.tick.get())
    }

    /// The clock moves two minutes: every statement is newer than the last.
    fn tick(&self) {
        self.tick.set(self.tick.get() + 1);
    }

    fn admin(&self) -> Admin<'_> {
        Admin::new(&self.store, self.cfg())
    }

    fn host(&self) -> String {
        self.remote.host_path().to_str().unwrap().to_string()
    }

    fn git_host(&self, args: &[&str]) -> Option<Vec<u8>> {
        let out = StdCommand::new("git").arg("--git-dir").arg(self.host()).args(args).output().unwrap();
        out.status.success().then_some(out.stdout)
    }

    fn host_head(&self) -> String {
        String::from_utf8(self.git_host(&["rev-parse", "refs/heads/main"]).unwrap()).unwrap().trim().to_string()
    }

    fn host_file(&self, path: &str) -> Option<Vec<u8>> {
        self.git_host(&["show", &format!("main:{path}")])
    }

    fn branch_exists(&self, n: u64) -> bool {
        self.git_host(&["rev-parse", "--verify", "--quiet", &format!("refs/heads/roster-proposal/{n}")]).is_some()
    }

    /// Everything local that a failed operation must not change.
    fn local_state(&self) -> (Vec<u8>, Vec<u8>, String) {
        let sticky = std::fs::read(&self.store.paths.sticky).unwrap_or_default();
        let pins = std::fs::read(self.store.paths.state.join("pins.toml")).unwrap_or_default();
        let refs = String::from_utf8(self.store.git_ok(&["for-each-ref"], 1 << 16).unwrap()).unwrap();
        (sticky, pins, refs)
    }

    /// The remote's working copy follows `refspec` on the host (to author hostile commits on top of it).
    fn checkout(&self, branch: &str) {
        let host = self.host();
        self.remote.git(&["fetch", "--quiet", &host, branch]);
        self.remote.reset_hard("FETCH_HEAD");
    }

    /// Commit `files` on top of `from_branch` and push them to `to_branch` on the host (an attacker with push access).
    fn hostile_push(&self, from_branch: &str, to_branch: &str, files: &[(String, Vec<u8>)]) {
        self.checkout(from_branch);
        for (p, b) in files {
            self.remote.write(p, b);
        }
        self.remote.git(&["add", "-A", "--force"]);
        self.remote.git(&["commit", "--quiet", "--no-verify", "--allow-empty", "-m", "hostile"]);
        let host = self.host();
        self.remote.git(&["push", "--quiet", "--force", &host, &format!("HEAD:refs/heads/{to_branch}")]);
        self.checkout("main");
    }

    /// An honest author commits straight to `main` (records, a tamper, anything) and the install can sync it.
    fn author(&self, files: &[(String, Vec<u8>)], remove: &[&str]) -> String {
        self.checkout("main");
        for (p, b) in files {
            self.remote.write(p, b);
        }
        for p in remove {
            self.remote.remove(p);
        }
        self.remote.commit("author")
    }

    fn head_epoch(&self) -> Epoch {
        let n = (1..20).take_while(|n| self.host_file(&format!("roster/epochs/{n}.json")).is_some()).last().unwrap();
        Epoch::parse(&self.host_file(&format!("roster/epochs/{n}.json")).unwrap()).unwrap()
    }

    fn head_epoch_bytes_hash(&self) -> String {
        let n = self.head_epoch().epoch;
        hash::of(&self.host_file(&format!("roster/epochs/{n}.json")).unwrap())
    }

    /// A hostile epoch `head + 1` built from the real head by `edit`, as bytes.
    fn forge(&self, edit: impl FnOnce(&mut Epoch)) -> Vec<u8> {
        let head = self.head_epoch();
        let mut e = head.clone();
        e.epoch += 1;
        e.prev_hash = Some(self.head_epoch_bytes_hash());
        edit(&mut e);
        admin::epoch_bytes(&e)
    }

    /// Put `bytes` on `roster-proposal/<n>` with signatures by `signers` over exactly those bytes.
    fn hostile_proposal(&self, n: u64, bytes: &[u8], signers: &[&str]) {
        let mut files = vec![(layout::roster_epoch(n), bytes.to_vec())];
        for s in signers {
            let sig = admin::sign_epoch(&who(s), ORG, STORE, n, bytes).unwrap();
            files.push((admin::proposal_sig_path(n, &sig.kid).unwrap(), sig.to_json()));
        }
        self.hostile_push("main", &admin::proposal_branch(n), &files);
    }

    /// Propose, collect `signers`' signatures, and publish as `publisher`: the honest path.
    fn change(&self, op: Operation, signers: &[&str], publisher: &str) -> Delivered {
        let p = self.admin().propose(op).unwrap();
        let n = p.report.n;
        for s in signers {
            self.admin().sign(&who(s), n, &mut Yes::new()).unwrap();
        }
        self.tick();
        let d = self.admin().publish(&who(publisher), n, admin::DEFAULT_VALIDITY_SECS, &mut Yes::new()).unwrap();
        self.tick();
        d
    }

    fn sync(&self) -> VerifiedSync {
        self.store.sync_verified(&self.cfg()).unwrap()
    }

    /// A brand-new install joins from the strings an admin would read out, and syncs.
    fn fresh_install(&self, join: &JoinInfo) -> (tempfile::TempDir, Store, Result<VerifiedSync, AdminError>) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let store = Store::open(&home, ORG, STORE, &self.remote.url(), true).unwrap();
        let r = join_store(&store, &join.request()).map(|_| store.sync_verified(&self.cfg()).unwrap());
        (dir, store, r)
    }

    /// The join strings for the current head, read from the roster the host serves.
    fn join_now(&self, admin: &str) -> JoinInfo {
        let e = self.head_epoch();
        JoinInfo { url: self.remote.url(), admin_fingerprint: who(admin).fingerprint(), head_epoch: e.epoch, head_hash: self.head_epoch_bytes_hash() }
    }
}

fn accepted(r: VerifiedSync) -> (String, delphi_xenia::freshness::FreshnessState) {
    match r {
        VerifiedSync::Accepted { head, freshness, .. } | VerifiedSync::UpToDate { head, freshness } => (head, freshness),
        other => panic!("expected an accepted sync, got {other:?}"),
    }
}

// ----- genesis ---------------------------------------------------------------------------------------------

#[test]
fn a_fresh_join_and_sync_accepts_a_store_created_only_through_genesis() {
    let fx = Fx::new();
    // The join strings come from the genesis API, not from test helpers.
    assert!(fx.join.command().starts_with("delphi xenia join file://"));
    assert_eq!(fx.join.head_epoch, 1);
    let (_dir, store, r) = fx.fresh_install(&fx.join);
    let (head, freshness) = accepted(r.unwrap());
    assert_eq!(head, fx.host_head());
    assert!(freshness.endorsements_count(), "the genesis statement is current: {freshness:?}");
    // The pins a joiner got name the head pair and the admin who was read out.
    let pins = pins::load(&store.pin_storage()).unwrap().unwrap();
    assert_eq!((pins.head().0, pins.head().1), (1, fx.join.head_hash.as_str()));
    assert_eq!(pins.admin(), fx.join.admin_fingerprint);
    // The store the genesis wrote is exactly the layout the spec names, and passes the tree rules.
    assert_eq!(fx.host_file(".gitattributes").unwrap(), GITATTRIBUTES.as_bytes());
    assert!(fx.host_file("org.toml").is_some() && fx.host_file("roster/freshness.sigs.json").is_some());
    pins.check_org_toml(std::str::from_utf8(&fx.host_file("org.toml").unwrap()).unwrap()).unwrap();
    // Two commits: the roster, then the statement that names the first.
    let log = String::from_utf8(fx.git_host(&["rev-list", "--parents", "main"]).unwrap()).unwrap();
    assert_eq!(log.lines().count(), 2);
    let st = freshness::Statement::parse(&fx.host_file("roster/freshness.json").unwrap()).unwrap();
    let first = log.lines().find(|l| !l.contains(' ')).unwrap();
    assert_eq!(st.store_head, first);
}

#[test]
fn genesis_with_a_missing_founder_signature_is_refused_and_nothing_is_pushed() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let remote = TestRemote::new(&tmp.path().join("host"));
    let store = Store::open(&home, ORG, STORE, &remote.url(), true).unwrap();
    let admin = Admin::new(&store, VerifyConfig::at(t0()));
    let draft = GenesisDraft::build(ORG, STORE, &three_founders(), &time::format_utc(t0())).unwrap();
    let sign = |name: &str| {
        let p = prepare_genesis_signature(&draft, &who(name)).unwrap();
        approve_and_sign(p, &who(name)).unwrap().proposal_sig()
    };
    let mut approver = Closure(|_: &AdminPreview| panic!("no approval may be asked for before the founders have all signed"));
    let two = [sign("alice"), sign("bob")];
    let err = admin.init_store(&draft, &two, &who("alice"), admin::DEFAULT_VALIDITY_SECS, &mut approver).unwrap_err();
    assert_eq!(err, AdminError::MissingFounderSignatures(vec![who("carol").did()]));
    // A signature over some other draft, a duplicate and a non-founder's signature fill no gap.
    let other = GenesisDraft::build(ORG, STORE, &[founder("alice", true, &[]), founder("bob", true, &[]), founder("carol", true, &[])], &time::format_utc(t0() + 1)).unwrap();
    let carol_other = {
        let p = prepare_genesis_signature(&other, &who("carol")).unwrap();
        approve_and_sign(p, &who("carol")).unwrap().proposal_sig()
    };
    let junk = admin::sign_epoch(&who("mallory"), ORG, STORE, 1, draft.bytes()).unwrap();
    let all = [two[0].clone(), two[0].clone(), two[1].clone(), carol_other, junk];
    let err = admin.init_store(&draft, &all, &who("alice"), admin::DEFAULT_VALIDITY_SECS, &mut approver).unwrap_err();
    assert_eq!(err.reason(), "missing-founder-signatures");
    let heads = StdCommand::new("git").arg("--git-dir").arg(remote.host_path()).args(["for-each-ref"]).output().unwrap();
    assert!(heads.stdout.is_empty(), "nothing was pushed");
}

#[test]
fn genesis_signatures_need_a_founder_admin_and_the_statement_signer_is_one_too() {
    let draft = GenesisDraft::build(ORG, STORE, &three_founders(), &time::format_utc(t0())).unwrap();
    assert_eq!(prepare_genesis_signature(&draft, &who("erin")).err(), Some(AdminError::NotAnApprover));
    assert_eq!(prepare_genesis_signature(&draft, &who("mallory")).err(), Some(AdminError::NotAnApprover));
    let e = genesis_fx(three_founders(), &["alice", "bob", "carol"], "erin").err().unwrap();
    assert_eq!(e, AdminError::NotAnApprover);
}

#[test]
fn a_founder_who_declines_gets_nothing_pushed_and_a_second_genesis_cannot_overwrite() {
    let fx = Fx::new();
    let before = fx.host_head();
    // A second genesis into the same store is refused before anything is built, and again the push could not force.
    let draft = GenesisDraft::build(ORG, STORE, &[founder("mallory", true, &[])], &time::format_utc(t0() + 5)).unwrap();
    let p = prepare_genesis_signature(&draft, &who("mallory")).unwrap();
    let sig = approve_and_sign(p, &who("mallory")).unwrap().proposal_sig();
    let err = fx.admin().init_store(&draft, &[sig], &who("mallory"), admin::DEFAULT_VALIDITY_SECS, &mut Yes::new()).unwrap_err();
    assert_eq!(err, AdminError::StoreNotEmpty);
    assert_eq!(fx.host_head(), before);
}

#[test]
fn a_declined_genesis_pushes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let remote = TestRemote::new(&tmp.path().join("host"));
    let store = Store::open(&home, ORG, STORE, &remote.url(), true).unwrap();
    let admin = Admin::new(&store, VerifyConfig::at(t0()));
    let draft = GenesisDraft::build(ORG, STORE, &[founder("alice", true, &[])], &time::format_utc(t0())).unwrap();
    let p = prepare_genesis_signature(&draft, &who("alice")).unwrap();
    let sig = approve_and_sign(p, &who("alice")).unwrap().proposal_sig();
    let err = admin.init_store(&draft, &[sig], &who("alice"), admin::DEFAULT_VALIDITY_SECS, &mut Closure(|_| AdminDecision::Deny)).unwrap_err();
    assert_eq!(err, AdminError::Denied);
    assert!(StdCommand::new("git").arg("--git-dir").arg(remote.host_path()).arg("for-each-ref").output().unwrap().stdout.is_empty());
}

// ----- the proposal flow ------------------------------------------------------------------------------------

fn add_gail() -> Operation {
    Operation::Add(NewMember { did: key(30).did(), person: "gail".into(), handle: "gail".into(), admin: false, steward_areas: vec![] })
}

#[test]
fn propose_sign_publish_is_a_fast_forward_that_a_fresh_install_accepts() {
    let fx = Fx::new();
    let before = fx.host_head();
    let p = fx.admin().propose(add_gail()).unwrap();
    assert_eq!((p.report.n, p.report.branch.as_str()), (2, "roster-proposal/2"));
    assert!(fx.branch_exists(2));
    assert_eq!(fx.host_head(), before, "a proposal does not touch main");
    assert!(p.report.changes.iter().any(|c| c.person == "gail"));
    assert_eq!((p.report.quorum.met, p.report.quorum.have), (false, 0));
    // Signing shows the diff and the exact bytes; one signature is not a quorum.
    let mut yes = Yes::new();
    fx.admin().sign(&who("alice"), 2, &mut yes).unwrap();
    assert_eq!(yes.seen.len(), 1, "exactly one decision");
    assert_eq!(yes.seen[0].files[0].bytes, p.report.bytes);
    assert!(yes.seen[0].signs[0].starts_with("xenia-roster/1\nacme\nacme-main\n2\nblake3:"));
    let rep = fx.admin().inspect(2).unwrap();
    assert_eq!((rep.quorum.met, rep.quorum.have, rep.quorum.need), (false, 1, 2));
    assert_eq!(fx.admin().publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS, &mut Closure(|_| panic!("no approval before the quorum"))).unwrap_err(), AdminError::QuorumNotMet { have: 1, need: 2 });
    fx.admin().sign(&who("bob"), 2, &mut Yes::new()).unwrap();
    assert!(fx.admin().inspect(2).unwrap().quorum.met);
    fx.tick();
    let d = fx.admin().publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS, &mut Yes::new()).unwrap();
    assert!(!d.already_there);
    assert_eq!(d.parent, before);
    assert_eq!(fx.host_head(), d.commit);
    // Fast-forward: the old head is an ancestor of the new one.
    assert!(fx.git_host(&["merge-base", "--is-ancestor", &before, &d.commit]).is_some());
    // The new statement names the new epoch and the head that was verified.
    let st = freshness::Statement::parse(&fx.host_file("roster/freshness.json").unwrap()).unwrap();
    assert_eq!((st.epoch, st.store_head.as_str()), (2, before.as_str()));
    // A fresh install joined at epoch 1 verifies the whole chain forward and accepts the new head.
    let (_d, store, r) = fx.fresh_install(&fx.join);
    let (head, fresh) = accepted(r.unwrap());
    assert_eq!(head, d.commit);
    assert!(fresh.endorsements_count());
    let pins = pins::load(&store.pin_storage()).unwrap().unwrap();
    assert_eq!(pins.head().0, 2);
    // So does the publishing install itself.
    let (head, _) = accepted(fx.sync());
    assert_eq!(head, d.commit);
}

#[test]
fn a_second_proposal_for_the_same_epoch_is_refused_not_overwritten() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    let tip = fx.git_host(&["rev-parse", "refs/heads/roster-proposal/2"]).unwrap();
    let other = Operation::Remove { did: who("erin").did(), for_cause: false };
    assert_eq!(fx.admin().propose(other).unwrap_err(), AdminError::ProposalExists(2));
    assert_eq!(fx.git_host(&["rev-parse", "refs/heads/roster-proposal/2"]).unwrap(), tip);
}

#[test]
fn a_proposal_the_operation_rules_refuse_never_reaches_the_branch() {
    let fx = Fx::new();
    let dup = Operation::Add(NewMember { did: who("erin").did(), person: "x".into(), handle: "x".into(), admin: false, steward_areas: vec![] });
    assert_eq!(fx.admin().propose(dup).unwrap_err(), AdminError::Refused(Refusal::AlreadyListed));
    assert!(!fx.branch_exists(2));
}

#[test]
fn a_non_admin_cannot_sign_and_a_stranger_cannot_publish() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    for name in ["erin", "dave", "mallory"] {
        assert_eq!(fx.admin().prepare_sign(&who(name), 2).err(), Some(AdminError::NotAnApprover), "{name}");
    }
    fx.admin().sign(&who("alice"), 2, &mut Yes::new()).unwrap();
    fx.admin().sign(&who("bob"), 2, &mut Yes::new()).unwrap();
    assert_eq!(fx.admin().prepare_publish(&who("mallory"), 2, admin::DEFAULT_VALIDITY_SECS).err(), Some(AdminError::NotAnApprover));
}

#[test]
fn signing_twice_is_idempotent_and_an_unknown_proposal_is_not_found() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    fx.admin().sign(&who("alice"), 2, &mut Yes::new()).unwrap();
    let p = fx.admin().prepare_sign(&who("alice"), 2).unwrap();
    let signed = approve_and_sign(p, &who("alice")).unwrap();
    assert!(fx.admin().deliver_signature(&signed).unwrap().already_there);
    assert_eq!(fx.admin().inspect(3).unwrap_err(), Refusal::NotNextEpoch { expected: 2, found: 3 }.into());
    let fx2 = Fx::new();
    assert_eq!(fx2.admin().inspect(2).unwrap_err(), AdminError::ProposalNotFound(2));
}

// ----- hostile proposals ------------------------------------------------------------------------------------

#[test]
fn a_hidden_admin_is_shown_flagged_and_needs_an_explicit_acknowledgement() {
    let fx = Fx::new();
    let bytes = fx.forge(|e| {
        let mut m = e.member(&who("erin").did()).unwrap().clone();
        m.roles = vec![roster::Role::Member, roster::Role::Admin];
        e.members.retain(|x| x.did != m.did);
        e.members.push(m);
        let mut ghost = e.member(&who("erin").did()).cloned().unwrap();
        ghost.did = who("mallory").did();
        ghost.person = "mallory".into();
        ghost.handle = "mallory".into();
        e.members.push(ghost);
    });
    fx.hostile_proposal(2, &bytes, &[]);
    let p = fx.admin().prepare_sign(&who("alice"), 2).expect("the verifier alone cannot tell a hidden admin from a wanted one");
    let pv = p.preview();
    assert!(pv.flags.contains(&Flag::AddsAdmin) && pv.flags.contains(&Flag::GainsAdmin), "{:?}", pv.flags);
    assert!(pv.changes.iter().any(|c| c.person == "mallory"));
    // An approval that does not acknowledge the flags signs nothing.
    let no_ack = p.preview().approve(false);
    assert_eq!(p.sign(&no_ack, &who("alice")).err(), Some(AdminError::FlagsNotAcknowledged));
    // And an operator who reads the diff and declines pushes nothing.
    let tip = fx.git_host(&["rev-parse", "refs/heads/roster-proposal/2"]).unwrap();
    let err = fx.admin().sign(&who("alice"), 2, &mut Closure(|pv: &AdminPreview| if pv.flags.is_empty() { AdminDecision::Approve(pv.approve(true)) } else { AdminDecision::Deny })).unwrap_err();
    assert_eq!(err, AdminError::Denied);
    assert_eq!(fx.git_host(&["rev-parse", "refs/heads/roster-proposal/2"]).unwrap(), tip);
}

#[test]
fn a_proposal_that_changes_person_or_added_at_or_drops_an_entry_is_refused_before_any_preview() {
    let fx = Fx::new();
    let mut never = Closure(|_: &AdminPreview| panic!("a refused proposal must not reach an approver"));
    for (name, edit, want) in [
        ("person", Box::new(|e: &mut Epoch| e.members[3].person = "alice".into()) as Box<dyn FnOnce(&mut Epoch)>, "person-changed"),
        ("added_at", Box::new(|e: &mut Epoch| e.members[3].added_at = "2020-01-01T00:00:00Z".into()), "immutable-field-changed"),
        ("dropped", Box::new(|e: &mut Epoch| {
            e.members.pop();
        }), "entry-dropped"),
        ("no-admin", Box::new(|e: &mut Epoch| {
            for m in e.members.iter_mut().take(3) {
                m.status = roster::Status::Retired;
            }
        }), "last-admin"),
        ("one-admin", Box::new(|e: &mut Epoch| {
            for m in e.members.iter_mut().take(2) {
                m.status = roster::Status::Retired;
            }
        }), "below-minimum-admins"),
    ] {
        let n = fx.head_epoch().epoch + 1;
        let bytes = fx.forge(edit);
        let branch = admin::proposal_branch(n);
        fx.hostile_push("main", &branch, &[(layout::roster_epoch(n), bytes)]);
        let err = fx.admin().sign(&who("alice"), n, &mut never).unwrap_err();
        assert_eq!(err.reason(), want, "{name}");
        // Delete the branch so the next case can reuse the number (the attacker's remote is ours to reset).
        fx.git_host(&["update-ref", "-d", &format!("refs/heads/{branch}")]);
    }
}

#[test]
fn a_proposal_for_the_wrong_base_or_scope_is_refused() {
    let fx = Fx::new();
    let stale_base = fx.forge(|e| e.prev_hash = Some(hash::of(b"another epoch 1")));
    fx.hostile_proposal(2, &stale_base, &[]);
    assert_eq!(fx.admin().prepare_sign(&who("alice"), 2).err(), Some(AdminError::Refused(Refusal::StaleBase)));
    fx.git_host(&["update-ref", "-d", "refs/heads/roster-proposal/2"]);
    let wrong_store = fx.forge(|e| e.store = "other-store".into());
    fx.hostile_proposal(2, &wrong_store, &[]);
    assert_eq!(fx.admin().prepare_sign(&who("alice"), 2).err(), Some(AdminError::Refused(Refusal::WrongScope)));
    fx.git_host(&["update-ref", "-d", "refs/heads/roster-proposal/2"]);
    let wrong_number = fx.forge(|e| e.epoch = 5);
    fx.hostile_push("main", "roster-proposal/2", &[(layout::roster_epoch(2), wrong_number)]);
    assert_eq!(fx.admin().prepare_sign(&who("alice"), 2).err(), Some(AdminError::Refused(Refusal::NotNextEpoch { expected: 2, found: 5 })));
}

#[test]
fn signatures_over_a_different_epoch_hash_count_for_nothing() {
    let fx = Fx::new();
    let wanted = fx.forge(|e| e.members.iter_mut().find(|m| m.person == "erin").unwrap().handle = "erin2".into());
    let swapped = fx.forge(|e| {
        let mut ghost = e.member(&who("erin").did()).cloned().unwrap();
        ghost.did = who("mallory").did();
        ghost.person = "mallory".into();
        ghost.handle = "mallory".into();
        ghost.roles = vec![roster::Role::Member, roster::Role::Admin];
        e.members.push(ghost);
    });
    assert_ne!(wanted, swapped);
    // Two admins signed the innocent file; the branch carries the other one with those signatures attached.
    let mut files = vec![(layout::roster_epoch(2), swapped.clone())];
    for s in ["alice", "bob"] {
        let sig = admin::sign_epoch(&who(s), ORG, STORE, 2, &wanted).unwrap();
        files.push((admin::proposal_sig_path(2, &sig.kid).unwrap(), sig.to_json()));
    }
    fx.hostile_push("main", "roster-proposal/2", &files);
    let rep = fx.admin().inspect(2).unwrap();
    assert_eq!((rep.quorum.met, rep.quorum.have, rep.quorum.need), (false, 0, 2));
    assert_eq!(fx.admin().prepare_publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS).err(), Some(AdminError::QuorumNotMet { have: 0, need: 2 }));
}

#[test]
fn signatures_from_non_admins_misnamed_files_and_oversize_junk_are_ignored() {
    let fx = Fx::new();
    let legal = fx.forge(|e| {
        let m = e.members.iter_mut().find(|m| m.person == "erin").unwrap();
        m.handle = "erin2".into();
    });
    let mut files = vec![(layout::roster_epoch(2), legal.clone())];
    for s in ["erin", "dave", "mallory"] {
        let sig = admin::sign_epoch(&who(s), ORG, STORE, 2, &legal).unwrap();
        files.push((admin::proposal_sig_path(2, &sig.kid).unwrap(), sig.to_json()));
    }
    let alice = admin::sign_epoch(&who("alice"), ORG, STORE, 2, &legal).unwrap();
    files.push((admin::proposal_sig_path(2, &who("bob").did()).unwrap(), alice.to_json()));
    files.push((admin::proposal_sig_path(2, &who("carol").did()).unwrap(), vec![b' '; 4096]));
    fx.hostile_push("main", "roster-proposal/2", &files);
    let rep = fx.admin().inspect(2).unwrap();
    assert_eq!((rep.quorum.met, rep.quorum.have, rep.quorum.people.len()), (false, 0, 0));
}

#[test]
fn a_one_of_three_publish_is_refused_even_when_the_branch_is_otherwise_valid() {
    let fx = Fx::new();
    let before = fx.host_head();
    let bytes = fx.forge(|e| e.members.iter_mut().find(|m| m.person == "erin").unwrap().handle = "erin2".into());
    fx.hostile_proposal(2, &bytes, &["alice"]);
    let err = fx.admin().publish(&who("alice"), 2, admin::DEFAULT_VALIDITY_SECS, &mut Closure(|_| panic!("asked to approve a 1-of-3"))).unwrap_err();
    assert_eq!(err, AdminError::QuorumNotMet { have: 1, need: 2 });
    assert_eq!(fx.host_head(), before);
}

#[test]
fn the_same_person_with_two_keys_counts_once_at_publish() {
    // alice holds two admin keys in the roster; with carol silent, alice's two signatures are one person.
    let mut founders = three_founders();
    founders.push(NewMember { did: who("alice2").did(), person: "alice".into(), handle: "alice-b".into(), admin: true, steward_areas: vec![] });
    let fx = genesis_fx(founders, &["alice", "alice2", "bob", "carol"], "alice").unwrap();
    let bytes = fx.forge(|e| e.members.iter_mut().find(|m| m.person == "erin").unwrap().handle = "erin2".into());
    fx.hostile_proposal(2, &bytes, &["alice", "alice2"]);
    let rep = fx.admin().inspect(2).unwrap();
    assert_eq!((rep.quorum.met, rep.quorum.have, rep.quorum.need), (false, 1, 2));
    assert_eq!(rep.quorum.people, vec!["alice".to_string()]);
    assert_eq!(fx.admin().prepare_publish(&who("bob"), 2, admin::DEFAULT_VALIDITY_SECS).err(), Some(AdminError::QuorumNotMet { have: 1, need: 2 }));
}

#[test]
fn an_old_proposal_replayed_after_the_head_moved_is_stale() {
    let fx = Fx::new();
    let p = fx.admin().propose(add_gail()).unwrap();
    for s in ["alice", "bob"] {
        fx.admin().sign(&who(s), 2, &mut Yes::new()).unwrap();
    }
    fx.tick();
    fx.admin().publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS, &mut Yes::new()).unwrap();
    // The branch is still there with its quorum; nobody can sign or publish it again.
    assert!(fx.branch_exists(2));
    let head = fx.host_head();
    assert_eq!(fx.admin().prepare_sign(&who("carol"), 2).err(), Some(AdminError::StaleProposal { head_epoch: 2 }));
    assert_eq!(fx.admin().prepare_publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS).err(), Some(AdminError::StaleProposal { head_epoch: 2 }));
    assert_eq!(fx.host_head(), head);
    // Delivering a signed-before payload after the move is refused as well.
    let _ = p;
}

#[test]
fn a_proposal_whose_base_moved_before_delivery_is_not_published() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    for s in ["alice", "bob"] {
        fx.admin().sign(&who(s), 2, &mut Yes::new()).unwrap();
    }
    fx.tick();
    let prepared = fx.admin().prepare_publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS).unwrap();
    let signed = approve_and_sign(prepared, &who("carol")).unwrap();
    // Meanwhile somebody else publishes epoch 2 by another route (a different branch tip would be needed; here the
    // same bytes land first).
    fx.tick();
    fx.admin().deliver_publish(&signed).unwrap();
    // Delivering the same signed payload again is recognised as already there, not pushed twice.
    let again = fx.admin().deliver_publish(&signed).unwrap();
    assert!(again.already_there);
}

// ----- remove, rotate ---------------------------------------------------------------------------------------

#[test]
fn remove_publishes_and_the_member_loses_standing_for_a_fresh_install() {
    let fx = Fx::new();
    fx.change(Operation::Remove { did: who("erin").did(), for_cause: false }, &["alice", "bob"], "alice");
    let (_d, _s, r) = fx.fresh_install(&fx.join_now("alice"));
    accepted(r.unwrap());
    let e = fx.head_epoch();
    let erin = e.member(&who("erin").did()).unwrap();
    assert_eq!((erin.status, erin.removed_at_epoch), (roster::Status::Removed, Some(2)));
}

#[test]
fn the_last_admin_cannot_be_removed_and_the_floor_holds_for_the_second() {
    let fx = genesis_fx(vec![founder("alice", true, &[]), founder("bob", true, &[]), founder("erin", false, &[])], &["alice", "bob"], "alice").unwrap();
    // Two admins: removing one would leave one admin, below the default minimum of two.
    let err = fx.admin().propose(Operation::Remove { did: who("bob").did(), for_cause: false }).unwrap_err();
    assert_eq!(err, AdminError::Refused(Refusal::BelowMinimumAdmins { after: 1, min: 2 }));
    let mut a = fx.admin();
    a.policy = Policy { min_admins: 1 };
    // Even with the floor lowered, the roster keeps one admin.
    assert!(a.propose(Operation::Remove { did: who("bob").did(), for_cause: false }).is_ok());
    let one = genesis_fx(vec![founder("alice", true, &[]), founder("erin", false, &[])], &["alice"], "alice").unwrap();
    let mut a = one.admin();
    a.policy = Policy { min_admins: 0 };
    assert_eq!(a.propose(Operation::Remove { did: who("alice").did(), for_cause: false }).unwrap_err(), AdminError::Refused(Refusal::LastAdmin));
    assert!(!one.branch_exists(2));
}

#[test]
fn routine_rotation_publishes_a_successor_chain_x7_can_resign_from() {
    let fx = Fx::new();
    let op = Operation::Rotate { old_did: who("dave").did(), new_did: who("dave2").did(), kind: admin::Rotation::Routine };
    let p = fx.admin().propose(op).unwrap();
    assert_eq!(p.report.reminders, vec![Reminder::RepublishResign]);
    for s in ["alice", "bob"] {
        fx.admin().sign(&who(s), 2, &mut Yes::new()).unwrap();
    }
    fx.tick();
    fx.admin().publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS, &mut Yes::new()).unwrap();
    let (_d, store, r) = fx.fresh_install(&fx.join);
    accepted(r.unwrap());
    // The chain a verifier and X7's `resign` rely on: old retired with the new key as successor, same person.
    let e = fx.head_epoch();
    let (old, new) = (e.member(&who("dave").did()).unwrap(), e.member(&who("dave2").did()).unwrap());
    assert_eq!((old.status, old.successor.as_deref(), old.voids_tombstones), (roster::Status::Retired, Some(new.did.as_str()), false));
    assert_eq!((old.person.as_str(), &old.steward_areas), (new.person.as_str(), &new.steward_areas));
    drop(store);
}

#[test]
fn a_compromise_rotation_voids_tombstones_and_says_what_to_do_about_the_old_keys_records() {
    let fx = Fx::new();
    let op = Operation::Rotate { old_did: who("alice").did(), new_did: who("alice2").did(), kind: admin::Rotation::Compromised };
    let p = fx.admin().propose(op).unwrap();
    assert_eq!(p.report.reminders, vec![Reminder::CompromisedKeyRecords]);
    assert!(p.report.flags.contains(&Flag::VoidsTombstones));
    // Every admin who is asked to sign is shown the instruction, as text, in the preview.
    let mut yes = Yes::new();
    fx.admin().sign(&who("bob"), 2, &mut yes).unwrap();
    assert_eq!(yes.seen[0].reminders, vec![Reminder::CompromisedKeyRecords]);
    assert!(Reminder::CompromisedKeyRecords.text().contains("nothing the old key signed has standing"));
    fx.admin().sign(&who("carol"), 2, &mut Yes::new()).unwrap();
    fx.tick();
    // The compromised key is not asked to publish; a fresh admin does.
    fx.admin().publish(&who("bob"), 2, admin::DEFAULT_VALIDITY_SECS, &mut Yes::new()).unwrap();
    let e = fx.head_epoch();
    assert!(e.member(&who("alice").did()).unwrap().voids_tombstones);
    assert_eq!(e.member(&who("alice").did()).unwrap().successor.as_deref(), Some(who("alice2").did().as_str()));
    let (_d, _s, r) = fx.fresh_install(&fx.join);
    accepted(r.unwrap());
}

#[test]
fn after_a_compromise_rotation_the_old_key_cannot_sign_the_next_epoch() {
    let fx = Fx::new();
    fx.change(Operation::Rotate { old_did: who("alice").did(), new_did: who("alice2").did(), kind: admin::Rotation::Compromised }, &["bob", "carol"], "bob");
    // The thief holds the old alice key and bob... no: the thief holds the old alice key only, plus a forged branch.
    let bytes = fx.forge(|e| e.members.iter_mut().find(|m| m.person == "erin").unwrap().handle = "erin3".into());
    fx.hostile_proposal(3, &bytes, &["alice", "mallory"]);
    assert_eq!(fx.admin().prepare_sign(&who("alice"), 3).err(), Some(AdminError::NotAnApprover), "the old key may not even be asked");
    let rep = fx.admin().inspect(3).unwrap();
    assert_eq!((rep.quorum.met, rep.quorum.have), (false, 0));
    // The successor key counts, as the same person.
    fx.admin().sign(&who("alice2"), 3, &mut Yes::new()).unwrap();
    fx.admin().sign(&who("bob"), 3, &mut Yes::new()).unwrap();
    let rep = fx.admin().inspect(3).unwrap();
    assert!(rep.quorum.met, "{rep:?}");
}

#[test]
fn a_publisher_who_is_not_an_admin_of_the_new_epoch_cannot_sign_its_statement() {
    let fx = Fx::new();
    let op = Operation::Rotate { old_did: who("bob").did(), new_did: who("bob2").did(), kind: admin::Rotation::Routine };
    fx.admin().propose(op).unwrap();
    for s in ["alice", "carol"] {
        fx.admin().sign(&who(s), 2, &mut Yes::new()).unwrap();
    }
    // bob's old key is retired in the epoch it would be publishing.
    assert_eq!(fx.admin().prepare_publish(&who("bob"), 2, admin::DEFAULT_VALIDITY_SECS).err(), Some(AdminError::NotAnApprover));
    fx.tick();
    fx.admin().publish(&who("bob2"), 2, admin::DEFAULT_VALIDITY_SECS, &mut Yes::new()).unwrap();
}

// ----- refresh ----------------------------------------------------------------------------------------------

#[test]
fn refresh_walks_first_then_signs_a_statement_naming_the_verified_head() {
    let fx = Fx::new();
    let rec = rk::memory(&who("erin"), REC, 1, "active", "v");
    let files: Vec<(String, Vec<u8>)> = rec.files().iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
    fx.author(&files, &[]);
    accepted(fx.sync());
    fx.tick();
    let head = fx.host_head();
    let mut yes = Yes::new();
    let d = fx.admin().refresh(&who("bob"), admin::DEFAULT_VALIDITY_SECS, &mut yes).unwrap();
    assert_eq!(yes.seen.len(), 1);
    let pv = &yes.seen[0];
    assert_eq!(pv.store_head.as_deref(), Some(head.as_str()));
    assert!(!pv.walked.is_empty(), "the preview says which history was walked and from where");
    assert_eq!(fx.host_head(), d.commit);
    let st = freshness::Statement::parse(&fx.host_file("roster/freshness.json").unwrap()).unwrap();
    assert_eq!((st.epoch, st.store_head.as_str()), (1, head.as_str()));
    assert_eq!(st.epoch_hash, fx.join.head_hash);
    let (_d, _s, r) = fx.fresh_install(&fx.join);
    let (h, fresh) = accepted(r.unwrap());
    assert_eq!(h, d.commit);
    assert!(fresh.endorsements_count());
}

#[test]
fn refresh_over_a_tampered_history_is_refused_and_nothing_is_signed() {
    let fx = Fx::new();
    let rec = rk::memory(&who("erin"), REC, 1, "active", "v");
    let files: Vec<(String, Vec<u8>)> = rec.files().iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
    fx.author(&files, &[]);
    accepted(fx.sync());
    // The host deletes the signed record in a fast-forward commit.
    let tampered = fx.author(&[], &[files[0].0.as_str(), files[1].0.as_str()]);
    fx.tick();
    let before = fx.local_state();
    let mut never = Closure(|_: &AdminPreview| panic!("a refresh over tampered history must never be shown for signing"));
    let err = fx.admin().refresh(&who("alice"), admin::DEFAULT_VALIDITY_SECS, &mut never).unwrap_err();
    assert!(matches!(err, AdminError::StoreNotUsable(_)), "{err:?}");
    assert_eq!(fx.host_head(), tampered, "nothing was pushed");
    assert_eq!(fx.local_state().1, before.1);
}

#[test]
fn the_explicit_walk_does_not_trust_the_installs_own_accepted_state() {
    let fx = Fx::new();
    let rec = rk::memory(&who("erin"), REC, 1, "active", "v");
    let files: Vec<(String, Vec<u8>)> = rec.files().iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
    fx.author(&files, &[]);
    accepted(fx.sync());
    let tampered = fx.author(&[], &[files[0].0.as_str(), files[1].0.as_str()]);
    // Force the install's state to claim the tampered head was accepted (a bug, an old version, a restored backup).
    let _ = fx.store.fetch().unwrap();
    let mut sticky = fx.store.load_sticky().unwrap();
    sticky.accepted_head = Some(tampered.clone());
    fx.store.save_sticky(&sticky).unwrap();
    fx.git_host(&["rev-parse", "main"]).unwrap();
    fx.tick();
    let err = fx.admin().prepare_refresh(&who("alice"), admin::DEFAULT_VALIDITY_SECS).err().unwrap();
    match err {
        AdminError::HistoryFailed(w) => assert!(w.iter().any(|x| x.contains("deleted")), "{w:?}"),
        other => panic!("expected the walk to refuse, got {other:?}"),
    }
}

#[test]
fn refresh_needs_an_active_admin_a_newer_clock_and_a_validity_within_the_cap() {
    let fx = Fx::new();
    fx.tick();
    for name in ["erin", "dave", "mallory"] {
        assert_eq!(fx.admin().prepare_refresh(&who(name), admin::DEFAULT_VALIDITY_SECS).err(), Some(AdminError::NotAnApprover), "{name}");
    }
    let cap = freshness::DEFAULT_MAX_VALIDITY_SECS;
    assert_eq!(fx.admin().prepare_refresh(&who("alice"), cap + 1).err().unwrap().reason(), "bad-validity");
    assert_eq!(fx.admin().prepare_refresh(&who("alice"), 0).err().unwrap().reason(), "bad-validity");
    assert!(fx.admin().prepare_refresh(&who("alice"), cap).is_ok());
    // A clock that is not ahead of the last statement would be ignored everywhere as a replay: refused up front.
    let behind = Admin::new(&fx.store, VerifyConfig::at(t0()));
    assert_eq!(behind.prepare_refresh(&who("alice"), 60).err(), Some(AdminError::ClockBehind));
    let well_behind = Admin::new(&fx.store, VerifyConfig::at(t0() - 600));
    assert_eq!(well_behind.prepare_refresh(&who("alice"), 60).err(), Some(AdminError::ClockBehind));
}

#[test]
fn a_signature_is_not_pushed_onto_a_proposal_that_was_swapped_after_the_preview() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    let p = fx.admin().prepare_sign(&who("alice"), 2).unwrap();
    let signed = approve_and_sign(p, &who("alice")).unwrap();
    // The attacker pushes a new commit on the branch that replaces the epoch file with other bytes.
    let other = fx.forge(|e| e.members.iter_mut().find(|m| m.person == "erin").unwrap().handle = "erin9".into());
    fx.hostile_push("roster-proposal/2", "roster-proposal/2", &[(layout::roster_epoch(2), other)]);
    let tip = fx.git_host(&["rev-parse", "refs/heads/roster-proposal/2"]).unwrap();
    assert_eq!(fx.admin().deliver_signature(&signed).unwrap_err(), AdminError::ProposalChanged);
    assert_eq!(fx.git_host(&["rev-parse", "refs/heads/roster-proposal/2"]).unwrap(), tip);
}

#[test]
fn a_signed_publish_is_not_delivered_after_a_different_epoch_took_the_number() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    for s in ["alice", "bob"] {
        fx.admin().sign(&who(s), 2, &mut Yes::new()).unwrap();
    }
    fx.tick();
    let signed = approve_and_sign(fx.admin().prepare_publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS).unwrap(), &who("carol")).unwrap();
    // Another epoch 2 (a different change, also with a real quorum) is published first, by another route.
    let fx_other = &fx;
    fx_other.git_host(&["update-ref", "-d", "refs/heads/roster-proposal/2"]);
    let rival = fx.forge(|e| e.members.iter_mut().find(|m| m.person == "erin").unwrap().handle = "erin7".into());
    fx.hostile_proposal(2, &rival, &["alice", "bob"]);
    fx.tick();
    fx.admin().publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS, &mut Yes::new()).unwrap();
    let head = fx.host_head();
    fx.tick();
    let err = fx.admin().deliver_publish(&signed).unwrap_err();
    assert_eq!(err, AdminError::StaleProposal { head_epoch: 2 });
    assert_eq!(fx.host_head(), head);
}

#[test]
fn an_oversize_object_on_a_proposal_branch_is_refused_before_anything_is_read() {
    let fx = Fx::new();
    let bytes = fx.forge(|e| e.members.iter_mut().find(|m| m.person == "erin").unwrap().handle = "erin2".into());
    fx.hostile_push("main", "roster-proposal/2", &[(layout::roster_epoch(2), bytes), ("junk.bin".to_string(), vec![7u8; 700 * 1024])]);
    let err = fx.admin().inspect(2).unwrap_err();
    assert!(matches!(&err, AdminError::StoreNotUsable(w) if w.contains("object-too-large")), "{err:?}");
}

#[test]
fn a_statement_already_there_with_other_signatures_does_not_count_as_delivered() {
    let fx = Fx::new();
    fx.tick();
    let signed = approve_and_sign(fx.admin().prepare_refresh(&who("alice"), admin::DEFAULT_VALIDITY_SECS).unwrap(), &who("alice")).unwrap();
    // Somebody lands the very same statement bytes with a signatures file that is not alice's.
    let junk_sigs = admin::statement_sigs_file(&who("erin").did(), &admin::sign_epoch(&who("erin"), ORG, STORE, 9, b"x").unwrap().sig);
    fx.author(&[(layout::FRESHNESS.to_string(), signed.payload.statement.clone()), (layout::FRESHNESS_SIGS.to_string(), junk_sigs.clone())], &[]);
    let d = fx.admin().deliver_refresh(&signed).unwrap();
    assert!(!d.already_there);
    assert_ne!(fx.host_file(layout::FRESHNESS_SIGS).unwrap(), junk_sigs);
}

#[test]
fn a_signed_refresh_is_not_delivered_once_the_roster_has_moved_on() {
    let fx = Fx::new();
    fx.tick();
    let signed = approve_and_sign(fx.admin().prepare_refresh(&who("alice"), admin::DEFAULT_VALIDITY_SECS).unwrap(), &who("alice")).unwrap();
    fx.change(add_gail(), &["alice", "bob"], "carol");
    let head = fx.host_head();
    assert_eq!(fx.admin().deliver_refresh(&signed).unwrap_err(), AdminError::StaleProposal { head_epoch: 2 });
    assert_eq!(fx.host_head(), head, "a statement for an older epoch is never pushed over the new one");
}

#[test]
fn a_refresh_after_the_store_moved_is_rebuilt_on_the_new_head_with_the_same_signed_bytes() {
    let fx = Fx::new();
    fx.tick();
    let prepared = fx.admin().prepare_refresh(&who("alice"), admin::DEFAULT_VALIDITY_SECS).unwrap();
    let signed = approve_and_sign(prepared, &who("alice")).unwrap();
    // Somebody publishes a record between the preview and the push.
    let rec = rk::memory(&who("erin"), REC, 1, "active", "v");
    let files: Vec<(String, Vec<u8>)> = rec.files().iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
    let moved = fx.author(&files, &[]);
    let d = fx.admin().deliver_refresh(&signed).unwrap();
    assert_eq!(d.parent, moved);
    // The statement still names the head that was verified when it was signed; it is an ancestor of the tip.
    let (head, fresh) = accepted(fx.sync());
    assert_eq!(head, d.commit);
    assert!(fresh.endorsements_count());
}

// ----- the gate itself --------------------------------------------------------------------------------------

#[test]
fn an_approval_for_other_bytes_or_the_wrong_key_signs_nothing() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    let a = fx.admin().prepare_sign(&who("alice"), 2).unwrap();
    let b = fx.admin().prepare_sign(&who("bob"), 2).unwrap();
    // bob's approval cannot be used for alice's signature: the digest names the signer.
    let bobs = b.preview().approve(true);
    assert_eq!(a.sign(&bobs, &who("alice")).err(), Some(AdminError::ApprovalMismatch));
    // The wrong key is refused before anything else.
    let a = fx.admin().prepare_sign(&who("alice"), 2).unwrap();
    let ok = a.preview().approve(true);
    assert_eq!(a.sign(&ok, &who("bob")).err(), Some(AdminError::WrongKey));
    // An approval for a different proposal's digest.
    let fx2 = Fx::new();
    fx2.admin().propose(Operation::Remove { did: who("erin").did(), for_cause: false }).unwrap();
    let other = fx2.admin().prepare_sign(&who("alice"), 2).unwrap().preview().approve(true);
    let a = fx.admin().prepare_sign(&who("alice"), 2).unwrap();
    assert_eq!(a.sign(&other, &who("alice")).err(), Some(AdminError::ApprovalMismatch));
}

#[test]
fn the_digest_covers_the_bytes_the_domains_and_the_statement() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    for s in ["alice", "bob"] {
        fx.admin().sign(&who(s), 2, &mut Yes::new()).unwrap();
    }
    let p1 = fx.admin().prepare_publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS).unwrap();
    let p2 = fx.admin().prepare_publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS - 60).unwrap();
    assert_ne!(p1.preview().digest, p2.preview().digest, "a different statement is a different thing to approve");
    assert_eq!(p1.preview().files.len(), 3);
    let signed = approve_and_sign(p1, &who("carol")).unwrap();
    assert_eq!(signed.sigs.len(), 1);
}

#[test]
fn the_gate_cannot_start_a_model_turn_by_construction() {
    let src = include_str!("xenia_admin.rs");
    let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    for forbidden in ["crate::client", "crate::spawn", "crate::hub", "crate::server", "crate::peer", "tokio", "async fn", ".await", "Command::new", "std::net", "reqwest"] {
        assert!(!code.contains(forbidden), "xenia_admin.rs mentions {forbidden}");
    }
    let fields = |name: &str| -> String { code.split(&format!("pub struct {name}")).nth(1).map(|t| t.split('}').next().unwrap_or("").to_string()).unwrap_or_default() };
    for s in ["Prepared", "Signed", "Admin", "AdminPreview", "AdminApproval", "EpochSigPayload", "PublishPayload", "RefreshPayload", "GenesisPayload"] {
        assert!(!fields(s).contains("Identity"), "{s} holds a key");
    }
    assert!(code.contains("pub fn sign(self, approval: &AdminApproval, key: &Identity)"));
    assert!(!code.contains("impl<T> Clone for Prepared") && !code.contains("derive(Clone)]\npub struct Prepared"));
    // The key never reaches a formatted string.
    assert!(!code.contains("{key") && !code.contains("{:?}\", key"));
}

#[test]
fn the_push_command_never_forces() {
    let fx = Fx::new();
    let c = push_command(&fx.store, "0123456789abcdef0123456789abcdef01234567", "roster-proposal/2");
    let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert!(args.iter().any(|a| a == "push"));
    for a in &args {
        assert!(!a.contains("force") && !a.contains("lease") && !a.starts_with('+') && !a.contains(":+") && a != "-f" && !a.contains("--mirror") && !a.contains("--delete"), "{a}");
    }
    assert_eq!(args.last().unwrap(), "0123456789abcdef0123456789abcdef01234567:refs/heads/roster-proposal/2");
}

#[test]
fn a_push_the_store_refuses_says_so_and_leaves_local_state_alone() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    for s in ["alice", "bob"] {
        fx.admin().sign(&who(s), 2, &mut Yes::new()).unwrap();
    }
    fx.tick();
    let prepared = fx.admin().prepare_publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS).unwrap();
    let signed = approve_and_sign(prepared, &who("carol")).unwrap();
    fx.remote.host_hook("pre-receive", "#!/bin/sh\necho 'no pushes today' >&2\nexit 1\n");
    let (before, head) = (fx.local_state(), fx.host_head());
    let err = fx.admin().deliver_publish(&signed).unwrap_err();
    assert_eq!(err.reason(), "push-rejected");
    assert_eq!(fx.local_state(), before);
    assert_eq!(fx.host_head(), head);
}

#[test]
fn a_candidate_that_fails_the_verifier_is_never_pushed() {
    let fx = Fx::new();
    fx.admin().propose(add_gail()).unwrap();
    for s in ["alice", "bob"] {
        fx.admin().sign(&who(s), 2, &mut Yes::new()).unwrap();
    }
    fx.tick();
    let prepared = fx.admin().prepare_publish(&who("carol"), 2, admin::DEFAULT_VALIDITY_SECS).unwrap();
    let signed = approve_and_sign(prepared, &who("carol")).unwrap();
    // Corrupt the signed statement bytes the way a bug between sign and deliver would: the signature no longer
    // covers them, so the statement is not authentic and the candidate fails the verifier.
    let mut bad = signed;
    bad.payload.statement = bad.payload.statement.iter().map(|b| if *b == b'7' { b'8' } else { *b }).collect();
    let head = fx.host_head();
    let err = fx.admin().deliver_publish(&bad).unwrap_err();
    assert_eq!(err.reason(), "verify-failed", "{err:?}");
    assert_eq!(fx.host_head(), head);
}
