//! Tests for the admin operations. Every store is a real git repository (a bare "host" and an author's working copy)
//! driven through the real code path; every roster is really signed by generated test identities (alice, bob, carol:
//! 2-of-3). The attackers are a host that can write any branch, a proposer who smuggles changes, an admin who lost
//! standing, a clock that runs backwards, and a store that was tampered with before a statement was asked for.

use super::*;
use crate::xenia_fixture::{cfg, now, records as rk, Fx, DAY};
use crate::xenia_publish::push_command_for;
use delphi_xenia::admin::{NewMember, Rotation};
use delphi_xenia::roster::Epoch;
use delphi_xenia::testkit::key;
use std::process::Command as StdCommand;

// ---------------------------------------------------------------------------------------------------------
// Harness

struct Yes {
    ack: bool,
    seen: Vec<AdminPreview>,
}

impl Yes {
    fn new() -> Yes {
        Yes { ack: false, seen: Vec::new() }
    }
    fn acking() -> Yes {
        Yes { ack: true, seen: Vec::new() }
    }
}

impl AdminApprover for Yes {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        self.seen.push(p.clone());
        AdminDecision::Approve(p.approve(self.ack))
    }
}

struct No;

impl AdminApprover for No {
    fn decide(&mut self, _: &AdminPreview) -> AdminDecision {
        AdminDecision::Deny
    }
}

/// Runs a closure at the moment of the decision, then approves what it was shown.
struct Hook<F: FnMut()> {
    f: F,
    ack: bool,
    calls: usize,
}

impl<F: FnMut()> AdminApprover for Hook<F> {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        self.calls += 1;
        (self.f)();
        AdminDecision::Approve(p.approve(self.ack))
    }
}

fn setup(tmp: &tempfile::TempDir) -> Fx {
    Fx::installed(tmp.path(), &[]).0
}

fn admin(fx: &Fx) -> Admin<'_> {
    Admin::new(&fx.store, cfg())
}

fn admin_at(fx: &Fx, now: i64) -> Admin<'_> {
    Admin::new(&fx.store, VerifyConfig::at(now))
}

fn newcomer(seed: u8, person: &str) -> NewMember {
    NewMember { did: key(seed).did(), person: person.into(), handle: person.into(), admin: false, steward_areas: vec![] }
}

/// A git command against the host repository; `None` when it fails.
fn host(fx: &Fx, args: &[&str]) -> Option<String> {
    let out = StdCommand::new("git").env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).arg("--git-dir").arg(fx.remote.host_path()).args(args).output().unwrap();
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn host_main(fx: &Fx) -> String {
    host(fx, &["rev-parse", "refs/heads/main"]).unwrap()
}

fn proposal_tip(fx: &Fx, n: u64) -> Option<String> {
    host(fx, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/roster-proposal/{n}")])
}

fn host_file(fx: &Fx, rev: &str, path: &str) -> Option<Vec<u8>> {
    let out = StdCommand::new("git").env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).arg("--git-dir").arg(fx.remote.host_path()).args(["cat-file", "blob", &format!("{rev}:{path}")]).output().unwrap();
    out.status.success().then_some(out.stdout)
}

fn is_ancestor(fx: &Fx, a: &str, b: &str) -> bool {
    host(fx, &["merge-base", "--is-ancestor", a, b]).is_some()
}

/// The attacker writes `roster-proposal/<n>` directly on the host: a commit on top of main's head carrying `files`
/// (and nothing from main's working tree changes). Overwrites any existing branch.
fn plant(fx: &Fx, n: u64, files: &[(&str, &[u8])]) {
    let back = fx.remote.head();
    for (p, b) in files {
        fx.remote.write(p, b);
    }
    fx.remote.git(&["add", "-A", "--force"]);
    fx.remote.git(&["commit", "-q", "--allow-empty", "-m", "planted"]);
    let url = fx.remote.host_path().to_str().unwrap().to_string();
    fx.remote.git(&["push", "-q", "--force", &url, &format!("HEAD:refs/heads/roster-proposal/{n}")]);
    fx.remote.git(&["reset", "-q", "--hard", &back]);
    fx.remote.git(&["clean", "-fdq"]);
}

fn head_epoch(fx: &Fx, rev: &str) -> Epoch {
    let mut best = 0;
    for n in 1..20u64 {
        if host_file(fx, rev, &format!("roster/epochs/{n}.json")).is_some() {
            best = n;
        }
    }
    Epoch::parse(&host_file(fx, rev, &format!("roster/epochs/{best}.json")).unwrap()).unwrap()
}

/// propose (alice) + sign (bob) + publish (carol) for one op, on the standard fixture.
fn full(fx: &Fx, op: Op) -> EpochPublished {
    let a = admin(fx);
    let p = a.propose(&fx.id("alice"), op, &mut Yes::acking()).unwrap();
    a.sign(&fx.id("bob"), p.epoch, &mut Yes::acking()).unwrap();
    a.publish(&fx.id("carol"), p.epoch, &mut Yes::acking()).unwrap()
}

// ---------------------------------------------------------------------------------------------------------
// The flow

#[test]
fn add_propose_sign_publish_moves_main_by_a_fast_forward_and_another_install_accepts_it() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let before = host_main(&fx);
    let a = admin(&fx);
    let mut yes = Yes::acking();
    let p = a.add(&fx.id("alice"), newcomer(40, "frank"), &mut yes).unwrap();
    assert_eq!((p.epoch, p.branch.as_str()), (2, "roster-proposal/2"));
    // Only the proposal branch moved. Main is untouched until a quorum signs and someone publishes.
    assert_eq!(host_main(&fx), before);
    assert!(proposal_tip(&fx, 2).is_some());
    let shown = &yes.seen[0];
    assert_eq!((shown.have, shown.need), (1, 2));
    assert!(shown.changes.iter().any(|c| c.contains("frank")), "{:?}", shown.changes);

    // One signature is not a quorum.
    let err = a.publish(&fx.id("carol"), 2, &mut Yes::acking()).unwrap_err();
    assert_eq!(err, AdminError::QuorumNotMet { have: 1, need: 2 });
    assert_eq!(host_main(&fx), before);

    let s = a.sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap();
    assert_eq!((s.have, s.need), (2, 2));
    assert_eq!(host_main(&fx), before);

    let out = a.publish(&fx.id("carol"), 2, &mut Yes::acking()).unwrap();
    assert!(out.adopted && out.attempts == 1);
    let after = host_main(&fx);
    assert_eq!(after, out.commit);
    assert!(is_ancestor(&fx, &before, &after), "publish must be a fast-forward of main");
    // The epoch, its signatures and the statement arrived in that one commit.
    let diff = host(&fx, &["diff", "--name-status", &before, &after]).unwrap();
    for f in ["roster/epochs/2.json", "roster/epochs/2.sigs.json", "roster/freshness.json", "roster/freshness.sigs.json"] {
        assert!(diff.contains(f), "{diff}");
    }
    assert_eq!(diff.lines().count(), 4, "{diff}");
    assert!(head_epoch(&fx, "main").member(&key(40).did()).is_some());

    // This install adopted it; a second, independent install that only knew epoch 1 follows the chain forward.
    assert!(matches!(fx.sync(), VerifiedSync::UpToDate { .. }));
    assert_eq!(pins::load(&fx.store.pin_storage()).unwrap().unwrap().head().0, 2);
    let other = fx.fresh_install("second");
    assert!(matches!(other.sync_verified(&cfg()).unwrap(), VerifiedSync::Accepted { .. }));
    assert_eq!(pins::load(&other.pin_storage()).unwrap().unwrap().head().0, 2);
}

#[test]
fn the_next_epoch_needs_the_quorum_of_the_epoch_before_it() {
    // After adding a fourth admin there are four admin people: three signatures are needed for the next change.
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let admin4 = NewMember { admin: true, ..newcomer(40, "frank") };
    full(&fx, Op::Add(admin4));
    let a = admin(&fx);
    let p = a.propose(&fx.id("alice"), Op::Add(newcomer(41, "gina")), &mut Yes::acking()).unwrap();
    assert_eq!(p.epoch, 3);
    a.sign(&fx.id("bob"), 3, &mut Yes::acking()).unwrap();
    assert_eq!(a.publish(&fx.id("carol"), 3, &mut Yes::acking()).unwrap_err(), AdminError::QuorumNotMet { have: 2, need: 3 });
    a.sign(&fx.id("carol"), 3, &mut Yes::acking()).unwrap();
    assert!(a.publish(&fx.id("bob"), 3, &mut Yes::acking()).is_ok());
}

#[test]
fn the_same_key_signing_twice_adds_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    a.add(&fx.id("alice"), newcomer(40, "frank"), &mut Yes::acking()).unwrap();
    let again = a.sign(&fx.id("alice"), 2, &mut No).unwrap();
    assert_eq!((again.have, again.commit), (1, None));
    assert_eq!(a.publish(&fx.id("bob"), 2, &mut Yes::acking()).unwrap_err(), AdminError::QuorumNotMet { have: 1, need: 2 });
}

#[test]
fn a_proposal_for_an_open_epoch_cannot_be_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    a.add(&fx.id("alice"), newcomer(40, "frank"), &mut Yes::acking()).unwrap();
    let tip = proposal_tip(&fx, 2).unwrap();
    let err = a.add(&fx.id("bob"), newcomer(41, "gina"), &mut Yes::acking()).unwrap_err();
    assert_eq!(err, AdminError::ProposalExists(2));
    assert_eq!(proposal_tip(&fx, 2).unwrap(), tip, "the open proposal is untouched");
}

#[test]
fn a_concurrent_signature_is_merged_not_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    a.add(&fx.id("alice"), newcomer(40, "frank"), &mut Yes::acking()).unwrap();
    // While bob is reading the preview, carol signs and pushes first: bob's push is rejected and redone on top.
    let mut hook = Hook {
        f: || {
            admin(&fx).sign(&fx.id("carol"), 2, &mut Yes::acking()).unwrap();
        },
        ack: true,
        calls: 0,
    };
    let s = a.sign(&fx.id("bob"), 2, &mut hook).unwrap();
    assert_eq!(hook.calls, 1, "the operator is asked once, not again after a retry");
    assert_eq!((s.have, s.need), (3, 2));
    assert!(a.publish(&fx.id("alice"), 2, &mut Yes::acking()).is_ok());
}

#[test]
fn a_record_landing_while_the_admin_decides_is_built_on_and_never_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    a.add(&fx.id("alice"), newcomer(40, "frank"), &mut Yes::acking()).unwrap();
    a.sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap();
    // A member publishes a record (as its own commit on main) while carol is reading the publish preview.
    let rec = rk::memory(&fx.id("erin"), &crate::xenia_fixture::u(1), 1, "active", "late");
    let mut hook = Hook {
        f: || {
            fx.put_commit(&rec, "a record lands");
        },
        ack: true,
        calls: 0,
    };
    let out = a.publish(&fx.id("carol"), 2, &mut hook).unwrap();
    assert_eq!(hook.calls, 1);
    // The record is on main under our commit, the epoch too: a plain rebuild on the new head.
    let main = host_main(&fx);
    assert_eq!(main, out.commit);
    assert!(host_file(&fx, &main, rec.files()[0].0).is_some(), "the member's record must survive");
    assert!(host_file(&fx, &main, "roster/epochs/2.json").is_some());
}

#[test]
fn an_admin_who_loses_standing_cannot_propose_sign_publish_or_refresh() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    // carol's key is retired in favour of a new one (routine).
    let a = admin(&fx);
    let new_carol = key(60);
    let p = a.rotate(&fx.id("alice"), &fx.id("carol").did(), &new_carol.did(), Rotation::Routine, &mut Yes::acking()).unwrap();
    a.sign(&fx.id("bob"), p.epoch, &mut Yes::acking()).unwrap();
    a.publish(&fx.id("alice"), p.epoch, &mut Yes::acking()).unwrap();
    let old = fx.id("carol");
    assert_eq!(a.propose(&old, Op::Add(newcomer(41, "gina")), &mut Yes::new()).unwrap_err(), AdminError::NotAnAdmin);
    assert_eq!(a.sign(&old, 3, &mut Yes::new()).unwrap_err(), AdminError::NotAnAdmin);
    assert_eq!(a.publish(&old, 3, &mut Yes::new()).unwrap_err(), AdminError::NotAnAdmin);
    assert_eq!(admin_at(&fx, now() + DAY).refresh(&old, &mut Yes::new()).unwrap_err(), AdminError::NotAnAdmin);
    // The successor has the standing.
    assert!(a.propose(&new_carol, Op::Add(newcomer(41, "gina")), &mut Yes::acking()).is_ok());
}

#[test]
fn members_and_stewards_are_not_admins() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let before = host(&fx, &["for-each-ref"]).unwrap();
    let a = admin(&fx);
    for who in ["dave", "erin"] {
        assert_eq!(a.propose(&fx.id(who), Op::Add(newcomer(40, "frank")), &mut Yes::acking()).unwrap_err(), AdminError::NotAnAdmin);
    }
    // A stranger's key as well.
    assert_eq!(a.propose(&key(99), Op::Add(newcomer(40, "frank")), &mut Yes::acking()).unwrap_err(), AdminError::NotAnAdmin);
    assert_eq!(host(&fx, &["for-each-ref"]).unwrap(), before, "nothing was pushed");
}

#[test]
fn a_compromised_rotation_voids_tombstones_and_the_old_key_loses_everything() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let dave = fx.id("dave");
    let new_dave = key(61);
    let a = admin(&fx);
    let p = a.rotate(&fx.id("alice"), &dave.did(), &new_dave.did(), Rotation::Compromised, &mut Yes::acking()).unwrap();
    // The proposal says so, on the branch, before anyone signs.
    let e = Epoch::parse(&host_file(&fx, "refs/heads/roster-proposal/2", "roster/epochs/2.json").unwrap()).unwrap();
    let old = e.member(&dave.did()).unwrap();
    assert!(old.voids_tombstones && old.successor.as_deref() == Some(new_dave.did().as_str()));
    a.sign(&fx.id("bob"), p.epoch, &mut Yes::acking()).unwrap();
    a.publish(&fx.id("carol"), p.epoch, &mut Yes::acking()).unwrap();
    let main = head_epoch(&fx, "main");
    assert!(main.member(&dave.did()).unwrap().voids_tombstones);
    let routine = full(&fx, Op::Rotate { old: fx.id("erin").did(), new: key(62).did(), kind: Rotation::Routine });
    assert_eq!(routine.epoch, 3);
    assert!(!head_epoch(&fx, "main").member(&fx.id("erin").did()).unwrap().voids_tombstones);
}

#[test]
fn removing_oneself_is_allowed_but_only_another_admin_can_publish_it() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    a.remove(&fx.id("alice"), &fx.id("alice").did(), false, &mut Yes::acking()).unwrap();
    a.sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap();
    // alice could not sign the new epoch's statement: she is not active in it.
    assert_eq!(a.publish(&fx.id("alice"), 2, &mut Yes::acking()).unwrap_err(), AdminError::NotActiveInNewEpoch);
    assert!(a.publish(&fx.id("bob"), 2, &mut Yes::acking()).is_ok());
    assert_eq!(a.propose(&fx.id("alice"), Op::Add(newcomer(40, "frank")), &mut Yes::new()).unwrap_err(), AdminError::NotAnAdmin);
}

// ---------------------------------------------------------------------------------------------------------
// The operator's approval

#[test]
fn a_denied_proposal_pushes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let before = host(&fx, &["for-each-ref"]).unwrap();
    assert_eq!(admin(&fx).add(&fx.id("alice"), newcomer(40, "frank"), &mut No).unwrap_err(), AdminError::Denied);
    assert_eq!(host(&fx, &["for-each-ref"]).unwrap(), before);
}

#[test]
fn an_approval_for_other_bytes_signs_nothing() {
    // The approver answers with the approval of a different (earlier) preview: its digest cannot match.
    struct Stale(AdminApproval);
    impl AdminApprover for Stale {
        fn decide(&mut self, _: &AdminPreview) -> AdminDecision {
            AdminDecision::Approve(self.0.clone())
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let mut first = Yes::acking();
    admin(&fx).add(&fx.id("alice"), newcomer(40, "frank"), &mut first).unwrap();
    // Open proposals aside, a second operation with a different payload on a different epoch number.
    let old = first.seen[0].approve(true);
    let other = Admin::new(&fx.store, cfg());
    let err = other.sign(&fx.id("bob"), 2, &mut Stale(old.clone())).unwrap_err();
    // Sign's digest has the `sign` action in it; the propose-approval is for another action and other bytes.
    assert_eq!(err, AdminError::ApprovalMismatch);
    // A made-up digest.
    let forged = AdminApproval { digest: hash::of(b"anything"), warnings_acknowledged: true };
    assert_eq!(other.sign(&fx.id("bob"), 2, &mut Stale(forged)).unwrap_err(), AdminError::ApprovalMismatch);
    let (have, _) = {
        let q = host_file(&fx, "refs/heads/roster-proposal/2", "roster/epochs/2.sigs.json").unwrap();
        let s = delphi_xenia::roster::EpochSigs::parse(&q).unwrap();
        (s.signatures.len(), 0)
    };
    assert_eq!(have, 1, "bob's signature was not added");
}

#[test]
fn warnings_must_be_acknowledged() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let before = host(&fx, &["for-each-ref"]).unwrap();
    let admin_add = NewMember { admin: true, ..newcomer(40, "frank") };
    let a = admin(&fx);
    let mut no_ack = Yes::new();
    assert_eq!(a.add(&fx.id("alice"), admin_add.clone(), &mut no_ack).unwrap_err(), AdminError::WarningsNotAcknowledged);
    assert!(no_ack.seen[0].warnings.iter().any(|w| w.contains("GRANTS ADMIN")), "{:?}", no_ack.seen[0].warnings);
    assert_eq!(host(&fx, &["for-each-ref"]).unwrap(), before);
    assert!(a.add(&fx.id("alice"), admin_add, &mut Yes::acking()).is_ok());
}

// ---------------------------------------------------------------------------------------------------------
// A hostile proposal branch

fn proposal_files(fx: &Fx, edit: impl FnOnce(&mut Epoch)) -> (Vec<u8>, Vec<u8>) {
    let a = admin(fx);
    let base = a.base().unwrap();
    let head = base.roster.head();
    let mut e = delphi_xenia::admin::draft(head, &Op::Add(newcomer(40, "frank")), "2026-10-26T12:00:00Z").unwrap().epoch;
    edit(&mut e);
    let bytes = delphi_xenia::admin::serialize_epoch(&e);
    let sig = delphi_xenia::admin::sign_epoch(&fx.id("alice"), &fx.store.org, &fx.store.store, e.epoch, &bytes);
    let sigs = delphi_xenia::admin::merge_sigs(&head.epoch, e.epoch, &bytes, None, Some(sig)).unwrap();
    (bytes, sigs)
}

#[test]
fn a_proposal_edited_after_it_was_signed_keeps_no_signatures_and_shows_the_smuggled_change() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    a.add(&fx.id("alice"), newcomer(40, "frank"), &mut Yes::acking()).unwrap();
    // The host (or any pusher) edits the epoch file on the branch: erin becomes an admin. alice's signature stays.
    let signed_sigs = host_file(&fx, "refs/heads/roster-proposal/2", "roster/epochs/2.sigs.json").unwrap();
    let (evil, _) = proposal_files(&fx, |e| {
        let erin = fx.id("erin").did();
        e.members.iter_mut().find(|m| m.did == erin).unwrap().roles = vec![delphi_xenia::roster::Role::Member, delphi_xenia::roster::Role::Admin];
    });
    plant(&fx, 2, &[("roster/epochs/2.json", &evil), ("roster/epochs/2.sigs.json", &signed_sigs)]);

    // bob is shown what changed. The smuggled grant is a warning he must acknowledge; alice's signature is gone.
    let mut careless = Yes::new();
    assert_eq!(a.sign(&fx.id("bob"), 2, &mut careless).unwrap_err(), AdminError::WarningsNotAcknowledged);
    let shown = &careless.seen[0];
    assert!(shown.changes.iter().any(|c| c.contains("MODIFIES existing entry") && c.contains("roles")), "{:?}", shown.changes);
    assert!(shown.warnings.iter().any(|w| w.contains("GRANTS ADMIN")));
    assert_eq!(shown.signed, vec!["bob".to_string()], "alice's signature is over other bytes and counts for nothing");
    assert!(shown.notes.iter().any(|n| n.contains("count for nothing")), "{:?}", shown.notes);
    // Even if bob signs anyway, the quorum is 1 of 2: the edited file never reaches main.
    let before = host_main(&fx);
    a.sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap();
    assert_eq!(a.publish(&fx.id("carol"), 2, &mut Yes::acking()).unwrap_err(), AdminError::QuorumNotMet { have: 1, need: 2 });
    assert_eq!(host_main(&fx), before);
}

#[test]
fn a_hostile_tree_on_the_proposal_branch_never_reaches_main() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    let p = a.add(&fx.id("alice"), newcomer(40, "frank"), &mut Yes::acking()).unwrap();
    assert_eq!(p.epoch, 2);
    // Re-plant the branch with the same signed files plus junk: a forged record, a symlink, a submodule file, a hook-ish
    // path and a deletion of a roster file. alice's real files are kept byte for byte.
    let epoch = host_file(&fx, "refs/heads/roster-proposal/2", "roster/epochs/2.json").unwrap();
    let sigs = host_file(&fx, "refs/heads/roster-proposal/2", "roster/epochs/2.sigs.json").unwrap();
    fx.remote.symlink("evil-link", "/etc/passwd");
    plant(
        &fx,
        2,
        &[
            ("roster/epochs/2.json", &epoch),
            ("roster/epochs/2.sigs.json", &sigs),
            ("records/memory/01JBX3Q8T2M9Z4K7WYH5N6D0RZ.md", b"forged"),
            (".gitmodules", b"[submodule \"x\"]\n\tpath = x\n\turl = https://example.invalid/x\n"),
            (".githooks/post-merge", b"#!/bin/sh\ntouch /tmp/xenia-pwned\n"),
        ],
    );
    fx.remote.git(&["rm", "-q", "-f", "--ignore-unmatch", "evil-link"]);
    a.sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap();
    a.publish(&fx.id("carol"), 2, &mut Yes::acking()).unwrap();
    let main = host_main(&fx);
    let tree = host(&fx, &["ls-tree", "-r", "--name-only", &main]).unwrap();
    for bad in ["forged", "gitmodules", "githooks", "evil", "01JBX3Q8T2M9Z4K7WYH5N6D0RZ"] {
        assert!(!tree.contains(bad), "{bad} reached main:\n{tree}");
    }
    assert!(!std::path::Path::new("/tmp/xenia-pwned").exists());
}

#[test]
fn publish_alone_scrubs_a_hostile_tree_even_when_no_one_signed_through_sign() {
    // The attacker (or a careless tool) leaves a fully signed branch whose tree carries junk. `publish` must not
    // build on that tree or fast-forward main to it.
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    let base = a.base().unwrap();
    let head = base.roster.head();
    let d = delphi_xenia::admin::draft(head, &Op::Add(newcomer(40, "frank")), "2026-10-26T12:00:00Z").unwrap();
    let mut sigs: Option<Vec<u8>> = None;
    for who in ["alice", "bob"] {
        let s = delphi_xenia::admin::sign_epoch(&fx.id(who), &fx.store.org, &fx.store.store, 2, &d.bytes);
        sigs = Some(delphi_xenia::admin::merge_sigs(&head.epoch, 2, &d.bytes, sigs.as_deref(), Some(s)).unwrap());
    }
    plant(&fx, 2, &[("roster/epochs/2.json", &d.bytes), ("roster/epochs/2.sigs.json", sigs.as_deref().unwrap()), ("records/memory/01JBX3Q8T2M9Z4K7WYH5N6D0RZ.md", b"forged"), (".gitmodules", b"[submodule \"x\"]\n")]);
    let before = host_main(&fx);
    a.publish(&fx.id("carol"), 2, &mut Yes::acking()).unwrap();
    let tree = host(&fx, &["ls-tree", "-r", "--name-only", &host_main(&fx)]).unwrap();
    assert!(!tree.contains("gitmodules") && !tree.contains("01JBX3Q8T2M9Z4K7WYH5N6D0RZ"), "{tree}");
    assert!(is_ancestor(&fx, &before, &host_main(&fx)));
}

#[test]
fn a_proposal_at_the_wrong_place_in_the_chain_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    let (ok, ok_sigs) = proposal_files(&fx, |_| {});
    let case = |label: &str, bytes: &[u8]| {
        plant(&fx, 2, &[("roster/epochs/2.json", bytes), ("roster/epochs/2.sigs.json", &ok_sigs)]);
        a.sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap_err_or(label)
    };
    // A different number inside the file than on the branch name: the sign would be for another epoch.
    let e = delphi_xenia::admin::check_proposal(a.base().unwrap().roster.head(), &ok).unwrap().epoch;
    let mut skip = e.clone();
    skip.epoch = 3;
    assert!(matches!(case("number", &delphi_xenia::admin::serialize_epoch(&skip)), AdminError::StaleProposal { next: 2, proposed: 3 }));
    // A fork: prev_hash names something else.
    let mut fork = e.clone();
    fork.prev_hash = Some(hash::of(b"another chain"));
    assert_eq!(case("fork", &delphi_xenia::admin::serialize_epoch(&fork)), AdminError::Rule(RuleError::WrongPrevHash));
    // Another store.
    let mut other = e.clone();
    other.store = "another-store".into();
    assert_eq!(case("scope", &delphi_xenia::admin::serialize_epoch(&other)), AdminError::Rule(RuleError::WrongScope));
    // An entry dropped (the roster never drops entries).
    let mut dropped = e.clone();
    dropped.members.retain(|m| m.did != fx.id("erin").did());
    assert!(matches!(case("drop", &delphi_xenia::admin::serialize_epoch(&dropped)), AdminError::Rule(RuleError::Roster(_))));
    // Garbage, and a sigs file for the wrong epoch.
    assert!(matches!(case("garbage", b"{\"not\": \"a roster\"}"), AdminError::Rule(RuleError::Roster(_))));
    // No epoch file at all (only a README): the proposal is missing.
    plant(&fx, 2, &[("README", b"nothing here")]);
    assert_eq!(a.sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap_err(), AdminError::ProposalMissing(2));
    // And no such branch.
    assert_eq!(a.sign(&fx.id("bob"), 9, &mut Yes::acking()).unwrap_err(), AdminError::ProposalMissing(9));
}

trait UnwrapErrOr {
    fn unwrap_err_or(self, label: &str) -> AdminError;
}

impl<T: std::fmt::Debug> UnwrapErrOr for Result<T, AdminError> {
    fn unwrap_err_or(self, label: &str) -> AdminError {
        match self {
            Err(e) => e,
            Ok(v) => panic!("{label}: expected a refusal, got {v:?}"),
        }
    }
}

#[test]
fn an_epoch_file_that_is_a_symlink_or_a_directory_is_not_a_proposal() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    fx.remote.symlink("roster/epochs/2.json", "../../../../etc/passwd");
    fx.remote.git(&["add", "-A", "--force"]);
    fx.remote.git(&["commit", "-q", "-m", "link"]);
    let url = fx.remote.host_path().to_str().unwrap().to_string();
    fx.remote.git(&["push", "-q", "--force", &url, "HEAD:refs/heads/roster-proposal/2"]);
    assert_eq!(a.sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap_err(), AdminError::ProposalMissing(2));
}

#[test]
fn an_oversize_object_on_the_proposal_branch_is_refused_before_anything_is_read() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let (ok, sigs) = proposal_files(&fx, |_| {});
    let big = vec![b'x'; 600 * 1024];
    plant(&fx, 2, &[("roster/epochs/2.json", &ok), ("roster/epochs/2.sigs.json", &sigs), ("junk.bin", &big)]);
    let err = admin(&fx).sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap_err();
    assert_eq!(err, AdminError::ProposalRefused("object-too-large"));
}

#[test]
fn a_proposal_that_the_chain_has_moved_past_is_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    full(&fx, Op::Add(newcomer(40, "frank")));
    // roster-proposal/2 still exists on the host and now describes an epoch that is already history.
    assert!(proposal_tip(&fx, 2).is_some());
    let a = admin(&fx);
    assert_eq!(a.sign(&fx.id("bob"), 2, &mut Yes::acking()).unwrap_err(), AdminError::StaleProposal { next: 3, proposed: 2 });
    assert_eq!(a.publish(&fx.id("bob"), 2, &mut Yes::acking()).unwrap_err(), AdminError::StaleProposal { next: 3, proposed: 2 });
}

#[test]
fn the_proposal_changing_under_the_signer_aborts() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let a = admin(&fx);
    a.add(&fx.id("alice"), newcomer(40, "frank"), &mut Yes::acking()).unwrap();
    // While bob decides, the branch is replaced by a different proposal; carol's signature on it lands in between so
    // that bob's push is rejected and the retry sees other bytes.
    let (other, other_sigs) = proposal_files(&fx, |e| e.members.last_mut().unwrap().handle = "mallory".into());
    let mut hook = Hook {
        f: || {
            plant(&fx, 2, &[("roster/epochs/2.json", &other), ("roster/epochs/2.sigs.json", &other_sigs)]);
        },
        ack: true,
        calls: 0,
    };
    let err = a.sign(&fx.id("bob"), 2, &mut hook).unwrap_err();
    assert_eq!(err, AdminError::ProposalChanged);
}

// ---------------------------------------------------------------------------------------------------------
// Refresh

#[test]
fn refresh_writes_a_statement_for_the_head_epoch_naming_the_verified_head() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let before = host_main(&fx);
    let later = now() + 2 * DAY;
    let a = admin_at(&fx, later);
    let mut yes = Yes::new();
    let r = a.refresh(&fx.id("bob"), &mut yes).unwrap();
    assert_eq!(r.store_head, before);
    assert!(is_ancestor(&fx, &before, &host_main(&fx)));
    assert_eq!(host_main(&fx), r.commit);
    let diff = host(&fx, &["diff", "--name-only", &before, &r.commit]).unwrap();
    assert_eq!(diff.lines().collect::<Vec<_>>(), ["roster/freshness.json", "roster/freshness.sigs.json"]);
    // The statement is authentic for the roster, current at the later clock, and a sync sees Fresh.
    match fx.store.sync_verified(&VerifyConfig::at(later)).unwrap() {
        VerifiedSync::Accepted { freshness, .. } | VerifiedSync::UpToDate { freshness, .. } => assert!(freshness.endorsements_count(), "{freshness:?}"),
        other => panic!("{other:?}"),
    }
    assert!(yes.seen[0].notes.iter().any(|n| n.contains("previous statement")));
    assert_eq!(yes.seen[0].store_head.as_deref(), Some(before.as_str()));
}

#[test]
fn refresh_refuses_to_sign_when_the_history_walk_finds_tampering() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = Fx::with_cast(tmp.path());
    let rec = rk::memory(&fx.id("alice"), &crate::xenia_fixture::u(1), 1, "active", "v");
    let (fx, _) = Fx::installed_on(fx, &[&rec]);
    let before = host_main(&fx);
    // The host deletes a signed record in a fast-forward commit.
    fx.remote.remove(rec.files()[0].0);
    fx.remote.remove(rec.files()[1].0);
    fx.remote.commit("delete a record");
    let tampered = host_main(&fx);
    assert_ne!(tampered, before);
    let err = admin_at(&fx, now() + DAY).refresh(&fx.id("bob"), &mut Yes::new()).unwrap_err();
    match &err {
        AdminError::Tampering(p) => assert!(p.iter().any(|(path, _)| path.contains("records/memory")), "{p:?}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(host_main(&fx), tampered, "no statement was signed or pushed");
    // A revert (rev 2 back to rev 1) is refused the same way.
    let tmp3 = tempfile::tempdir().unwrap();
    let fx3 = Fx::with_cast(tmp3.path());
    let r1 = rk::memory(&fx3.id("alice"), &crate::xenia_fixture::u(2), 1, "active", "one");
    let r2 = rk::memory(&fx3.id("alice"), &crate::xenia_fixture::u(2), 2, "active", "two");
    let (fx3, _) = Fx::installed_on(fx3, &[&r2]);
    fx3.put_commit(&r1, "revert to rev 1");
    let err = admin_at(&fx3, now() + DAY).refresh(&fx3.id("bob"), &mut Yes::new()).unwrap_err();
    assert!(matches!(err, AdminError::Tampering(_)), "{err:?}");
}

#[test]
fn refresh_by_an_install_that_never_synced_still_walks_from_the_statements_head() {
    // A fresh install of the admin: no accepted head. Its walk starts at the statement's store_head, which the old
    // admin signed. A deletion after that head is tampering there too.
    let tmp = tempfile::tempdir().unwrap();
    let fx = Fx::with_cast(tmp.path());
    let rec = rk::memory(&fx.id("alice"), &crate::xenia_fixture::u(1), 1, "active", "v");
    let (fx, _) = Fx::installed_on(fx, &[&rec]);
    fx.remote.remove(rec.files()[0].0);
    fx.remote.remove(rec.files()[1].0);
    fx.remote.commit("delete a record");
    let fresh = fx.fresh_install("never-synced");
    let err = Admin::new(&fresh, VerifyConfig::at(now() + DAY)).refresh(&fx.id("bob"), &mut Yes::new()).unwrap_err();
    assert!(matches!(err, AdminError::Tampering(_)), "{err:?}");
}

#[test]
fn refresh_refuses_a_clock_that_would_make_a_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    // The fixture's statement was issued at now - 1h. A clock at or before that is a replay at every other install.
    for t in [now() - 3600, now() - 2 * 3600] {
        let err = admin_at(&fx, t).refresh(&fx.id("bob"), &mut Yes::new()).unwrap_err();
        assert_eq!(err, AdminError::ClockBehind, "at {t}");
    }
    assert!(admin_at(&fx, now() - 3600 + 1).refresh(&fx.id("bob"), &mut Yes::new()).is_ok());
}

#[test]
fn refresh_needs_a_trust_base_and_an_admin() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = Fx::with_cast(tmp.path());
    // A store with a roster but no statement: a fresh install accepts nothing, so there is nothing to attest.
    fx.commit("roster only");
    fx.join();
    let err = admin_at(&fx, now()).refresh(&fx.id("bob"), &mut Yes::new()).unwrap_err();
    assert!(matches!(err, AdminError::StoreNotUsable(_)), "{err:?}");
    let tmp2 = tempfile::tempdir().unwrap();
    let ok = setup(&tmp2);
    assert_eq!(admin_at(&ok, now() + DAY).refresh(&ok.id("dave"), &mut Yes::new()).unwrap_err(), AdminError::NotAnAdmin);
}

#[test]
fn an_unjoined_store_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = Fx::with_cast(tmp.path());
    fx.commit("roster");
    let err = admin(&fx).propose(&fx.id("alice"), Op::Add(newcomer(40, "frank")), &mut Yes::acking()).unwrap_err();
    assert!(matches!(err, AdminError::StoreNotUsable(m) if m.contains("not been joined")));
}

// ---------------------------------------------------------------------------------------------------------
// By construction

#[test]
fn the_push_for_a_proposal_never_forces() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = setup(&tmp);
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let c = push_command_for(&fx.store, sha, "roster-proposal/2");
    let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    for a in &args {
        assert!(!a.contains("force") && !a.contains("lease") && !a.starts_with('+') && !a.contains(":+") && a != "-f" && !a.contains("--delete") && !a.contains("--mirror"), "{a}");
    }
    assert_eq!(args.last().unwrap(), &format!("{sha}:refs/heads/roster-proposal/2"));
}

#[test]
fn the_admin_module_cannot_start_a_model_turn_hold_a_key_or_force_a_push() {
    let src = include_str!("xenia_admin.rs");
    let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    for forbidden in ["crate::client", "crate::spawn", "crate::hub", "crate::server", "crate::peer", "tokio", "async fn", ".await", "Command::new", "std::net", "reqwest", "--force", "--delete", "push -f"] {
        assert!(!code.contains(forbidden), "xenia_admin.rs mentions {forbidden}");
    }
    let fields = |name: &str| -> String { code.split(&format!("pub struct {name}")).nth(1).map(|t| t.split('}').next().unwrap_or("").to_string()).unwrap_or_default() };
    for s in ["Admin", "AdminPreview", "AdminApproval", "Proposed", "Cosigned", "EpochPublished", "Refreshed"] {
        assert!(!fields(s).contains("Identity"), "{s} holds a key");
    }
    // Rotation is not optional anywhere in the API: the type has no default and `rotate` takes it by value.
    assert!(code.contains("kind: admin::Rotation"));
    let pure = include_str!("../../delphi-xenia/src/admin.rs");
    assert!(!pure.contains("impl Default for Rotation") && !pure.contains("derive(Default"));
    for forbidden in ["std::fs", "std::net", "std::process", "SystemTime", "Instant"] {
        assert!(!pure.lines().filter(|l| !l.trim_start().starts_with("//")).any(|l| l.contains(forbidden)), "the pure admin module mentions {forbidden}");
    }
}

// ---------------------------------------------------------------------------------------------------------
// Genesis

struct Genesis {
    _tmp: tempfile::TempDir,
    remote: crate::xenia_testkit::TestRemote,
    store: Store,
    founders: Vec<Identity>,
}

fn genesis_world() -> Genesis {
    use delphi_xenia::testkit::{ORG, STORE};
    let tmp = tempfile::tempdir().unwrap();
    let remote = crate::xenia_testkit::TestRemote::new(&tmp.path().join("host"));
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let store = Store::open(&home, ORG, STORE, &remote.url(), true).unwrap();
    Genesis { _tmp: tmp, remote, store, founders: vec![key(70), key(71), key(72)] }
}

fn founder_list(g: &Genesis) -> Vec<NewMember> {
    ["ada", "ben", "cy"].iter().zip(&g.founders).map(|(n, k)| NewMember { did: k.did(), person: n.to_string(), handle: n.to_string(), admin: true, steward_areas: vec![] }).collect()
}

fn genesis_bytes(g: &Genesis) -> Vec<u8> {
    use delphi_xenia::testkit::{ORG, STORE};
    delphi_xenia::admin::genesis(ORG, STORE, &founder_list(g), "2026-10-26T12:00:00Z").unwrap().bytes
}

fn host_out(g: &Genesis, args: &[&str]) -> String {
    let out = StdCommand::new("git").env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).arg("--git-dir").arg(g.remote.host_path()).args(args).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn all_sign(g: &Genesis, bytes: &[u8]) -> Vec<u8> {
    let a = Admin::new(&g.store, cfg());
    let sigs: Vec<(String, String)> = g.founders.iter().map(|f| a.sign_genesis(f, bytes, &mut Yes::acking()).unwrap()).collect();
    delphi_xenia::admin::genesis_sigs(&sigs)
}

#[test]
fn genesis_creates_a_store_a_second_install_can_join_and_the_admins_can_then_work() {
    use delphi_xenia::testkit::{ORG, STORE};
    let g = genesis_world();
    let bytes = genesis_bytes(&g);
    let sigs = all_sign(&g, &bytes);
    let a = Admin::new(&g.store, cfg());
    let mut yes = Yes::acking();
    let out = a.publish_genesis(&g.founders[0], &bytes, &sigs, &mut yes).unwrap();
    assert!(out.adopted);
    assert!(yes.seen[0].warnings.iter().any(|w| w.contains("GRANTS ADMIN")), "the founders are shown as admins");
    // Two commits: the epoch, then the statement that names it.
    let log = host_out(&g, &["rev-list", "--parents", "refs/heads/main"]);
    assert_eq!(log.lines().count(), 2, "{log}");
    assert!(log.contains(&format!("{} {}", out.commit, out.epoch_commit)));
    // A second install joins with a founder's fingerprint and the head pair, and its first sync accepts the store.
    let home2 = g._tmp.path().join("second");
    std::fs::create_dir_all(&home2).unwrap();
    let second = Store::open(&home2, ORG, STORE, &g.remote.url(), true).unwrap();
    let req = pins::JoinRequest { url: g.remote.url(), admin_fingerprint: g.founders[1].fingerprint(), head_epoch: 1, head_hash: out.epoch_hash.clone() };
    let files = [delphi_xenia::roster::EpochFile { epoch_json: bytes.clone(), sigs_json: sigs.clone() }];
    pins::join(&second.pin_storage(), &req, &files).unwrap();
    match second.sync_verified(&cfg()).unwrap() {
        VerifiedSync::Accepted { freshness, .. } => assert!(freshness.endorsements_count(), "{freshness:?}"),
        other => panic!("{other:?}"),
    }
    // And the ordinary flow works on top of it: add a member with 2 of 3 founders.
    let p = a.add(&g.founders[0], newcomer(80, "dee"), &mut Yes::acking()).unwrap();
    a.sign(&g.founders[1], p.epoch, &mut Yes::acking()).unwrap();
    a.publish(&g.founders[2], p.epoch, &mut Yes::acking()).unwrap();
}

#[test]
fn genesis_needs_every_founder_and_exactly_these_bytes() {
    let g = genesis_world();
    let bytes = genesis_bytes(&g);
    let a = Admin::new(&g.store, cfg());
    let sign = |i: usize| a.sign_genesis(&g.founders[i], &bytes, &mut Yes::acking()).unwrap();
    // Two of three founders: not a genesis (it is the one epoch that needs everyone, spec 011 §3.2).
    let two = delphi_xenia::admin::genesis_sigs(&[sign(0), sign(1)]);
    let err = a.publish_genesis(&g.founders[0], &bytes, &two, &mut Yes::acking()).unwrap_err();
    assert!(matches!(err, AdminError::Rule(RuleError::Roster(r)) if r.contains("genesis")), "{err:?}");
    assert_eq!(host_out(&g, &["for-each-ref"]), "", "nothing was pushed");
    assert!(pins::load(&g.store.pin_storage()).unwrap().is_none(), "nothing was joined");
    // The third founder signed a different genesis (one more admin): their signature counts for nothing here.
    let mut founders = founder_list(&g);
    founders.push(NewMember { did: key(90).did(), person: "mallory".into(), handle: "mallory".into(), admin: true, steward_areas: vec![] });
    let evil = delphi_xenia::admin::genesis(delphi_xenia::testkit::ORG, delphi_xenia::testkit::STORE, &founders, "2026-10-26T12:00:00Z").unwrap().bytes;
    let c_on_evil = a.sign_genesis(&g.founders[2], &evil, &mut Yes::acking()).unwrap();
    let mixed = delphi_xenia::admin::genesis_sigs(&[sign(0), sign(1), c_on_evil]);
    assert!(a.publish_genesis(&g.founders[0], &bytes, &mixed, &mut Yes::acking()).is_err());
    assert_eq!(host_out(&g, &["for-each-ref"]), "");
    // Someone who is not a founder can neither sign nor publish it.
    assert_eq!(a.sign_genesis(&key(99), &bytes, &mut Yes::acking()).unwrap_err(), AdminError::NotAnAdmin);
    let full = all_sign(&g, &bytes);
    assert_eq!(a.publish_genesis(&key(99), &bytes, &full, &mut Yes::acking()).unwrap_err(), AdminError::NotAnAdmin);
    // Another store's genesis is not this store's genesis.
    let other = delphi_xenia::admin::genesis(delphi_xenia::testkit::ORG, "other-store", &founder_list(&g), "2026-10-26T12:00:00Z").unwrap().bytes;
    assert_eq!(a.sign_genesis(&g.founders[0], &other, &mut Yes::acking()).unwrap_err(), AdminError::Rule(RuleError::WrongScope));
}

#[test]
fn genesis_never_adds_to_a_store_that_already_has_a_main() {
    let g = genesis_world();
    g.remote.write("README", b"someone got here first");
    g.remote.commit("first");
    let before = host_out(&g, &["for-each-ref"]);
    let bytes = genesis_bytes(&g);
    let sigs = all_sign(&g, &bytes);
    let err = Admin::new(&g.store, cfg()).publish_genesis(&g.founders[0], &bytes, &sigs, &mut Yes::acking()).unwrap_err();
    assert_eq!(err, AdminError::StoreNotEmpty);
    assert_eq!(host_out(&g, &["for-each-ref"]), before);
    assert!(pins::load(&g.store.pin_storage()).unwrap().is_none(), "a refused genesis joins nothing");
}
