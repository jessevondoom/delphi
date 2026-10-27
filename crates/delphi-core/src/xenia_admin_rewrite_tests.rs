//! Tests for the authoring side of the rewrite recovery. Every store is a real git repository (a bare host in a temp
//! dir), every identity is generated, every roster, statement and signature is made by the real code. The attacker is a
//! host that force-pushes, a branch anyone can write, a single admin, a non-admin, and old or foreign statements.

use super::*;
use crate::xenia_fixture::{self as fxt, Fx, Org, SignedRecord};
use crate::xenia_sync::{Recovery, RewriteApproval, RewriteOutcome, VerifiedSync, VerifyConfig};
use delphi_xenia::testkit_records as rk;

fn cfg() -> VerifyConfig {
    fxt::cfg()
}

/// The human at the card: answers yes (with the warnings acknowledged unless `ack` is false) and keeps what it saw.
struct Say {
    yes: bool,
    ack: bool,
    seen: Vec<AdminPreview>,
}

impl Say {
    fn yes() -> Say {
        Say { yes: true, ack: true, seen: Vec::new() }
    }
}

impl AdminApprover for Say {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        self.seen.push(p.clone());
        if self.yes { AdminDecision::Approve(p.approve(self.ack)) } else { AdminDecision::Deny }
    }
}

struct World {
    _tmp: tempfile::TempDir,
    fx: Fx,
    bob: Store,
    carol: Store,
    /// An ordinary install holding the history: it only ever syncs and recovers.
    dana: Store,
    c1: String,
    c2: String,
    /// The rewritten head the host serves (the tombstone is gone).
    r2: String,
    tomb: SignedRecord,
}

impl World {
    fn admin<'a>(&self, s: &'a Store) -> Admin<'a> {
        Admin::new(s, cfg())
    }

    fn id(&self, who: &str) -> Identity {
        self.fx.id(who)
    }
}

/// Four installs hold a note and a tombstone at `c2`; the host then force-pushes a history without the tombstone.
fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let org = Org::three_admins();
    let alice = org.key("alice");
    let note = rk::memory(&alice, &fxt::u(0), 1, "active", "note");
    let tomb = rk::tombstone(&alice, &fxt::u(1), 1, &fxt::u(0), "withdraw");
    let (fx, c1) = Fx::installed_on(Fx::new(tmp.path(), org), &[&note, &tomb]);
    let c2 = fx.store.accepted_head().unwrap().unwrap();
    let mut others = Vec::new();
    for name in ["bob", "carol", "dana"] {
        let s = fx.fresh_install(name);
        assert!(matches!(s.sync_verified(&cfg()).unwrap(), VerifiedSync::Accepted { .. }), "{name} baseline");
        others.push(s);
    }
    fx.remote.reset_hard(&c1);
    fx.remote.remove(&tomb.path);
    fx.remote.remove(&tomb.sidecar_path);
    let r2 = fx.commit("rewrite");
    let dana = others.pop().unwrap();
    let carol = others.pop().unwrap();
    let bob = others.pop().unwrap();
    World { _tmp: tmp, fx, bob, carol, dana, c1, c2, r2, tomb }
}

fn plan(s: &Store, at: VerifyConfig) -> crate::xenia_sync::RewritePlan {
    match s.plan_rewrite(&at).unwrap() {
        Recovery::Plan(p) => p,
        other => panic!("expected a plan, got {other:?}"),
    }
}

fn refusal(s: &Store, at: VerifyConfig) -> &'static str {
    match s.plan_rewrite(&at).unwrap() {
        Recovery::Refused { reason, .. } => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A statement a majority really signed, put on top of the host's head by hand: what an attacker with two stolen keys
/// (or a replayed old file) could do, outside the command.
#[allow(clippy::too_many_arguments)]
fn host_statement(w: &World, org: &str, store: &str, store_head: &str, rewrite_of: &str, issued: i64, life: i64, signers: &[&str]) {
    let bytes = admin::build_rewrite_statement(org, store, 1, &w.fx.org.head_pair().1, store_head, rewrite_of, issued, life, &freshness::Policy { max_validity_secs: 3650 * 86400 }).unwrap();
    let domain = admin::statement_domain(org, store, &bytes).unwrap();
    let sigs: Vec<ProposalSig> = signers.iter().map(|n| ProposalSig { kid: w.id(n).did(), sig: w.id(n).sign(&domain) }).collect();
    w.fx.remote.write(layout::FRESHNESS, &bytes);
    w.fx.remote.write(layout::FRESHNESS_SIGS, &sigs_file(&sigs));
    w.fx.commit("hostile statement");
}

fn approval(p: &crate::xenia_sync::RewritePlan) -> RewriteApproval {
    RewriteApproval { fetched: p.fetched.clone(), paths: p.changes.iter().map(|c| c.path.clone()).collect() }
}

/// Branch `rewrite-proposal/<id>` with extra files, pushed by whoever can write to the host.
fn plant(a: &Admin, id: &str, files: Vec<(String, Vec<u8>)>) {
    let (staged, tip) = a.fetch_branch(&rewrite_branch(id), "refs/xenia/test-plant").unwrap().unwrap();
    drop(staged);
    let c = a.commit_on(Some(&tip), &files, "plant").unwrap();
    assert!(matches!(a.push(&c, &rewrite_branch(id)).unwrap(), Pushed::Yes));
}

/// A proposal branch holding exactly `bytes`, made by anyone with write access to the host.
fn plant_new(a: &Admin, w: &World, bytes: Vec<u8>) -> String {
    a.candidate().unwrap(); // fetches the rewritten head's objects into this install's clone
    let id = rewrite_id(&w.r2, &bytes);
    let c = a.commit_on(Some(&w.r2), &[(statement_path(&id), bytes)], "plant").unwrap();
    assert!(matches!(a.push(&c, &rewrite_branch(&id)).unwrap(), Pushed::Yes));
    id
}

fn sticky_bytes(s: &Store) -> Vec<u8> {
    std::fs::read(s.paths.state.join("sticky.json")).unwrap()
}

fn pins_bytes(s: &Store) -> Vec<u8> {
    std::fs::read(s.paths.state.join("pins.toml")).unwrap()
}

#[test]
fn two_of_three_author_a_rewrite_that_an_install_accepts_after_listing_the_paths() {
    let w = world();
    // The host force-pushed: an ordinary sync refuses, and with no statement the recovery has nothing to go on.
    match w.dana.sync_verified(&cfg()).unwrap() {
        VerifiedSync::NonFastForward(n) => assert_eq!((n.accepted.as_str(), n.fetched.as_str()), (w.c2.as_str(), w.r2.as_str())),
        other => panic!("{other:?}"),
    }
    assert_eq!(refusal(&w.dana, cfg()), "no-valid-statement");

    // propose: an admin of the roster, the draft names both heads, nothing is signed.
    let alice = w.id("alice");
    let pr = w.admin(&w.fx.store).rewrite_propose(&alice, 7 * fxt::DAY).unwrap();
    let id = pr.report.id.clone();
    assert_eq!(pr.report.branch, format!("rewrite-proposal/{id}"));
    assert_eq!((pr.report.view.replaced.as_str(), pr.report.view.rewritten.as_str()), (w.c2.as_str(), w.r2.as_str()));
    assert!(pr.report.view.affected.iter().any(|(why, p)| why == "deleted" && p == &w.tomb.path), "{:?}", pr.report.view);
    assert_eq!((pr.report.quorum.have, pr.report.quorum.need), (0, 2));
    let st = Statement::parse(&pr.report.bytes).unwrap();
    assert_eq!((st.rewrite_of.as_deref(), st.store_head.as_str()), (Some(w.c2.as_str()), w.r2.as_str()));

    // alice signs behind the card; one signature is not a quorum, so publish refuses before any card.
    let mut a = Say::yes();
    w.admin(&w.fx.store).rewrite_sign(&alice, &id, &mut a).unwrap();
    assert_eq!(a.seen.len(), 1, "one card");
    let shown = &a.seen[0];
    assert_eq!(shown.action, Action::SignRewrite);
    assert_eq!(shown.flags, vec![Flag::RewritesHistory], "the admin must acknowledge the rewrite");
    assert_eq!(shown.files[0].bytes, pr.report.bytes, "the bytes shown are the bytes signed");
    let text = crate::xenia_admin_card::card(shown);
    for must in [&w.c2, &w.r2, &w.tomb.path] {
        assert!(text.contains(must.as_str()), "the card shows {must}");
    }
    assert!(text.contains("will list every affected path and require its own operator"), "{text}");
    let mut never = Say::yes();
    let err = w.admin(&w.fx.store).rewrite_publish(&alice, &id, &mut never).unwrap_err();
    assert_eq!(err, AdminError::QuorumNotMet { have: 1, need: 2 });
    assert!(never.seen.is_empty(), "no card for a publish that cannot succeed");
    assert_eq!(w.fx.remote.host_head(), w.r2, "nothing was published");
    // Signing twice adds nothing; the quorum is counted per person.
    assert!(w.admin(&w.fx.store).rewrite_sign(&alice, &id, &mut Say::yes()).unwrap().already_there);
    assert_eq!(w.admin(&w.fx.store).rewrite_inspect(&id).unwrap().quorum.have, 1);

    // bob signs; carol, who did not sign, publishes.
    w.admin(&w.bob).rewrite_sign(&w.id("bob"), &id, &mut Say::yes()).unwrap();
    let mut pub_card = Say::yes();
    let d = w.admin(&w.carol).rewrite_publish(&w.id("carol"), &id, &mut pub_card).unwrap();
    assert_eq!((d.parent.as_str(), d.already_there), (w.r2.as_str(), false));
    assert_eq!(pub_card.seen[0].action, Action::PublishRewrite);
    assert!(pub_card.seen[0].signs.is_empty(), "the publisher adds no signature");
    assert_eq!(w.fx.remote.host_head(), d.commit);
    // The host now serves the published head, so the proposal is spent: a second publish is refused, not repeated.
    assert_eq!(w.admin(&w.carol).rewrite_publish(&w.id("carol"), &id, &mut Say::yes()).unwrap_err(), AdminError::RewriteHeadMoved);

    // The install with the history: it lists the path, needs the exact approval, and keeps its sticky state and pins.
    let (pins_before, sticky_before) = (pins_bytes(&w.dana), sticky_bytes(&w.dana));
    let held = w.dana.load_sticky().unwrap();
    let p = plan(&w.dana, cfg());
    assert_eq!((p.accepted.as_str(), p.fetched.as_str(), p.replaced.as_str(), p.signing_people), (w.c2.as_str(), d.commit.as_str(), w.c2.as_str(), 2));
    let listed: Vec<_> = p.changes.iter().map(|c| (c.path.as_str(), c.reason)).collect();
    assert_eq!(listed, vec![(w.tomb.path.as_str(), "deleted"), (w.tomb.sidecar_path.as_str(), "deleted")]);
    let mut short = approval(&p);
    short.paths.remove(&w.tomb.path);
    assert!(matches!(w.dana.accept_rewritten_head(&cfg(), &short).unwrap(), RewriteOutcome::NotApproved { .. }), "the operator must accept every path");
    assert_eq!(sticky_bytes(&w.dana), sticky_before, "a refused approval changes nothing");
    let out = w.dana.accept_rewritten_head(&cfg(), &approval(&p)).unwrap();
    assert!(matches!(&out, RewriteOutcome::Accepted { head, replaced } if head == &d.commit && replaced.len() == 2), "{out:?}");
    assert_eq!(pins_bytes(&w.dana), pins_before, "pinned epoch hashes are never cleared");
    let after = w.dana.load_sticky().unwrap();
    for (path, entry) in &held.paths {
        if path == &w.tomb.path || path == &w.tomb.sidecar_path {
            assert!(!after.paths.contains_key(path), "the approved deletion replaced the entry");
        } else if !path.starts_with("roster/freshness") {
            assert_eq!(after.paths.get(path), Some(entry), "{path} was not approved and must survive");
        }
    }
    assert!(held.paths.len() > 2 && after.paths.len() >= held.paths.len() - 2, "the note and the rest of the state survive");
    assert!(matches!(w.dana.sync_verified(&cfg()).unwrap(), VerifiedSync::UpToDate { .. }), "an ordinary store again");
    // The admins' own installs recover the same way.
    let pb = plan(&w.bob, cfg());
    assert!(matches!(w.bob.accept_rewritten_head(&cfg(), &approval(&pb)).unwrap(), RewriteOutcome::Accepted { .. }));
}

#[test]
fn nobody_but_an_admin_can_take_any_step() {
    let w = world();
    let erin = w.id("erin");
    let dave = w.id("dave"); // a steward, not an admin
    let a = w.admin(&w.fx.store);
    for who in [&erin, &dave] {
        assert_eq!(a.rewrite_propose(who, 7 * fxt::DAY).unwrap_err(), AdminError::NotAnApprover);
    }
    let id = a.rewrite_propose(&w.id("alice"), 7 * fxt::DAY).unwrap().report.id;
    let heads_before = w.fx.remote.host_head();
    let mut never = Say::yes();
    for who in [&erin, &dave] {
        assert_eq!(a.rewrite_sign(who, &id, &mut never).unwrap_err(), AdminError::NotAnApprover);
        assert_eq!(a.rewrite_publish(who, &id, &mut never).unwrap_err(), AdminError::NotAnApprover);
    }
    assert!(never.seen.is_empty(), "a non-admin is never shown a card");
    // Even with the quorum signed, a non-admin cannot publish it.
    a.rewrite_sign(&w.id("alice"), &id, &mut Say::yes()).unwrap();
    w.admin(&w.bob).rewrite_sign(&w.id("bob"), &id, &mut Say::yes()).unwrap();
    assert_eq!(a.rewrite_publish(&erin, &id, &mut never).unwrap_err(), AdminError::NotAnApprover);
    assert_eq!(w.fx.remote.host_head(), heads_before);
    // The refusal to acknowledge the warning, or to approve at all, signs and sends nothing.
    let mut no_ack = Say { yes: true, ack: false, seen: Vec::new() };
    assert_eq!(a.rewrite_publish(&w.id("alice"), &id, &mut no_ack).unwrap_err(), AdminError::FlagsNotAcknowledged);
    let mut no = Say { yes: false, ack: true, seen: Vec::new() };
    assert_eq!(a.rewrite_publish(&w.id("alice"), &id, &mut no).unwrap_err(), AdminError::Denied);
    assert_eq!(w.fx.remote.host_head(), heads_before);
}

#[test]
fn one_admin_cannot_make_an_install_accept_a_rewrite() {
    let w = world();
    // Whatever one admin (even all of one person's keys) signs, the install counts people against a majority.
    host_statement(&w, "acme", "acme-main", &w.r2, &w.c2, cfg().now - 600, 7 * fxt::DAY, &["alice"]);
    assert_eq!(refusal(&w.dana, cfg()), "no-valid-statement");
    assert!(matches!(w.dana.sync_verified(&cfg()).unwrap(), VerifiedSync::NonFastForward(_)));
    // The same bytes with a second admin are a majority.
    host_statement(&w, "acme", "acme-main", &w.r2, &w.c2, cfg().now - 600, 7 * fxt::DAY, &["alice", "bob"]);
    assert_eq!(plan(&w.dana, cfg()).signing_people, 2);
}

#[test]
fn a_tampered_proposal_or_statement_is_refused_everywhere() {
    let w = world();
    let a = w.admin(&w.fx.store);
    let id = a.rewrite_propose(&w.id("alice"), 7 * fxt::DAY).unwrap().report.id;
    let original = a.rewrite_inspect(&id).unwrap().bytes;
    // The branch's statement is replaced (a longer life): it is no longer the bytes the id names.
    let tampered = String::from_utf8(original.clone()).unwrap().replace("\"expires_at\": \"2026-11-02", "\"expires_at\": \"2026-11-09");
    assert_ne!(tampered.as_bytes(), original.as_slice());
    plant(&a, &id, vec![(statement_path(&id), tampered.into_bytes())]);
    assert_eq!(a.rewrite_inspect(&id).unwrap_err(), AdminError::RewriteStatementRefused("id-does-not-match-the-bytes"));
    let mut never = Say::yes();
    assert!(a.rewrite_sign(&w.id("bob"), &id, &mut never).is_err());
    assert!(never.seen.is_empty());

    // A signature over other bytes is dropped, not counted.
    let id2 = a.rewrite_propose(&w.id("alice"), 6 * fxt::DAY).unwrap().report.id;
    a.rewrite_sign(&w.id("alice"), &id2, &mut Say::yes()).unwrap();
    let other = admin::build_rewrite_statement("acme", "acme-main", 1, &w.fx.org.head_pair().1, &w.r2, &w.c2, cfg().now, 5 * fxt::DAY, &freshness::Policy::default()).unwrap();
    let domain = admin::statement_domain("acme", "acme-main", &other).unwrap();
    let bob = w.id("bob");
    let forged = ProposalSig { kid: bob.did(), sig: bob.sign(&domain) };
    plant(&a, &id2, vec![(sig_path(&id2, &bob.did()).unwrap(), forged.to_json())]);
    let q = a.rewrite_inspect(&id2).unwrap().quorum;
    assert_eq!((q.have, q.met), (1, false), "{q:?}");
    assert!(matches!(a.rewrite_publish(&w.id("alice"), &id2, &mut never), Err(AdminError::QuorumNotMet { have: 1, need: 2 })));

    // A published statement edited afterwards no longer verifies: the install does not accept it.
    let id3 = a.rewrite_propose(&w.id("alice"), 4 * fxt::DAY).unwrap().report.id;
    a.rewrite_sign(&w.id("alice"), &id3, &mut Say::yes()).unwrap();
    w.admin(&w.bob).rewrite_sign(&bob, &id3, &mut Say::yes()).unwrap();
    let d = a.rewrite_publish(&w.id("alice"), &id3, &mut Say::yes()).unwrap();
    assert_eq!(plan(&w.dana, cfg()).fetched, d.commit);
    w.fx.remote.git(&["fetch", "--quiet", w.fx.remote.host_path().to_str().unwrap(), "main"]);
    w.fx.remote.reset_hard("FETCH_HEAD");
    let published = String::from_utf8(std::fs::read(w.fx.remote.work_path().join(layout::FRESHNESS)).unwrap()).unwrap();
    w.fx.remote.write(layout::FRESHNESS, published.replace("4", "5").as_bytes());
    w.fx.commit("edit the statement");
    assert_eq!(refusal(&w.dana, cfg()), "no-valid-statement");
}

#[test]
fn a_wrong_replaced_head_is_refused_by_the_admins_and_by_the_install() {
    let w = world();
    let a = w.admin(&w.fx.store);
    let roster_hash = w.fx.org.head_pair().1;
    for wrong in [w.c1.clone(), "ab".repeat(20)] {
        // At the card: the statement replaces a head this admin does not hold.
        let bytes = admin::build_rewrite_statement("acme", "acme-main", 1, &roster_hash, &w.r2, &wrong, cfg().now, 7 * fxt::DAY, &freshness::Policy::default()).unwrap();
        let id = plant_new(&a, &w, bytes);
        let mut never = Say::yes();
        assert_eq!(a.rewrite_sign(&w.id("bob"), &id, &mut never).unwrap_err(), AdminError::RewriteOfNotOurHead);
        assert!(never.seen.is_empty());
        // At the install: even a real majority's signature over it does not cover this install's head.
        host_statement(&w, "acme", "acme-main", &w.r2, &wrong, cfg().now - 600, 7 * fxt::DAY, &["alice", "bob"]);
        assert_eq!(refusal(&w.dana, cfg()), "rewrite-of-is-not-our-head");
        w.fx.remote.reset_hard(&w.r2);
        w.fx.remote.force_push();
    }
}

#[test]
fn a_statement_for_another_store_is_ignored() {
    let w = world();
    let a = w.admin(&w.fx.store);
    let bytes = admin::build_rewrite_statement("acme", "other-store", 1, &w.fx.org.head_pair().1, &w.r2, &w.c2, cfg().now, 7 * fxt::DAY, &freshness::Policy::default()).unwrap();
    let id = plant_new(&a, &w, bytes);
    assert_eq!(a.rewrite_inspect(&id).unwrap_err(), AdminError::RewriteStatementRefused("wrong-scope"));
    host_statement(&w, "acme", "other-store", &w.r2, &w.c2, cfg().now - 600, 7 * fxt::DAY, &["alice", "bob"]);
    assert_eq!(refusal(&w.dana, cfg()), "no-valid-statement");
}

#[test]
fn an_expired_statement_is_refused_to_sign_and_to_accept() {
    let w = world();
    let a = w.admin(&w.fx.store);
    let id = a.rewrite_propose(&w.id("alice"), fxt::DAY).unwrap().report.id;
    a.rewrite_sign(&w.id("alice"), &id, &mut Say::yes()).unwrap();
    w.admin(&w.bob).rewrite_sign(&w.id("bob"), &id, &mut Say::yes()).unwrap();
    let later = VerifyConfig::at(cfg().now + 2 * fxt::DAY);
    let mut never = Say::yes();
    assert_eq!(Admin::new(&w.carol, later).rewrite_sign(&w.id("carol"), &id, &mut never).unwrap_err(), AdminError::RewriteStatementRefused("expired"));
    assert_eq!(Admin::new(&w.carol, later).rewrite_publish(&w.id("carol"), &id, &mut never).unwrap_err(), AdminError::RewriteStatementRefused("expired"));
    assert!(never.seen.is_empty());
    // Published in time, then read too late by an install: it asks for a fresh statement.
    w.admin(&w.carol).rewrite_publish(&w.id("carol"), &id, &mut Say::yes()).unwrap();
    assert_eq!(refusal(&w.dana, later), "statement-not-current");
    assert_eq!(plan(&w.dana, cfg()).signing_people, 2);
}

#[test]
fn a_replayed_old_statement_is_not_current() {
    let w = world();
    // The install has accepted a statement issued an hour ago; a once-valid majority statement from earlier is a replay.
    host_statement(&w, "acme", "acme-main", &w.r2, &w.c2, cfg().now - 2 * 3600, 7 * fxt::DAY, &["alice", "bob"]);
    assert_eq!(refusal(&w.dana, cfg()), "statement-not-current");
    // The authoring side will not issue one that old either: a statement must be newer than the last one accepted.
    let a = Admin::new(&w.fx.store, VerifyConfig::at(cfg().now - 7200));
    assert_eq!(a.rewrite_propose(&w.id("alice"), 7 * fxt::DAY).unwrap_err(), AdminError::ClockBehind);
}

#[test]
fn nothing_to_author_when_the_host_is_a_plain_fast_forward_or_older() {
    let w = world();
    w.fx.remote.reset_hard(&w.c2);
    w.fx.remote.force_push();
    let a = w.admin(&w.fx.store);
    assert_eq!(a.rewrite_propose(&w.id("alice"), 7 * fxt::DAY).unwrap_err(), AdminError::RewriteNotApplicable("nothing-to-recover"));
    w.fx.remote.reset_hard(&w.c1);
    w.fx.remote.force_push();
    assert_eq!(a.rewrite_propose(&w.id("alice"), 7 * fxt::DAY).unwrap_err(), AdminError::RewriteNotApplicable("host-is-behind"));
}

#[test]
fn the_authoring_code_has_one_signing_path_and_never_forces() {
    let src = include_str!("xenia_admin_rewrite.rs");
    let code = src.split("#[cfg(test)]").next().unwrap();
    assert!(!code.contains("--force") && !code.contains("\"+"), "no force push");
    assert_eq!(code.matches(".sign(&a, ").count(), 2, "the only signatures are behind a Prepared and an approval");
    assert!(!code.contains("Identity::load") && !code.contains("std::env"), "no key is read here");
}
