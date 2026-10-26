//! Tests for the verified sync and rewrite recovery. The attacker is the store host (or someone who can push to
//! it): every case is a real git repository built with the test kit and fetched through the real code path,
//! and every roster, record and statement is really signed. Where a rule is guarded twice (the walk and the
//! sticky state), a test says which one it is exercising.

use super::*;
use crate::xenia_testkit::{TestRemote, GOOD_GITATTRIBUTES};
use delphi_xenia::pins::{self, JoinRequest};
use delphi_xenia::testkit::{MemberSpec, TestOrg};
use delphi_xenia::testkit_records::{self as rk, FreshSpec, SignedRecord};
use delphi_xenia::time::parse_utc;

const DAY: i64 = 24 * 3600;
const A: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";
const B: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RB";
const T: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RC";
const T2: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RD";
const E: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RE";

fn now() -> i64 {
    parse_utc("2026-10-26T12:00:00Z").unwrap()
}

fn cfg() -> VerifyConfig {
    VerifyConfig::at(now())
}

struct Fx {
    tmp: tempfile::TempDir,
    remote: TestRemote,
    store: Store,
    org: TestOrg,
}

impl Fx {
    fn new(org: TestOrg) -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let remote = TestRemote::new(&tmp.path().join("host"));
        remote.write(".gitattributes", GOOD_GITATTRIBUTES.as_bytes());
        remote.write("org.toml", b"org = \"acme\"\nstore = \"acme-main\"\n");
        let store = Store::open(&home, "acme", "acme-main", &remote.url(), true).unwrap();
        let fx = Fx { tmp, remote, store, org };
        fx.roster();
        fx
    }

    /// The default: three admins, one epoch, roster written into the working copy.
    fn standard() -> Fx {
        Fx::new(TestOrg::three_admins())
    }

    /// Write every epoch file of `self.org` (unchanged files make no diff).
    fn roster(&self) {
        for (i, f) in self.org.files.iter().enumerate() {
            let n = i + 1;
            self.remote.write(&format!("roster/epochs/{n}.json"), &f.epoch_json);
            self.remote.write(&format!("roster/epochs/{n}.sigs.json"), &f.sigs_json);
        }
    }

    fn id(&self, who: &str) -> delphi_xenia::identity::Identity {
        self.org.key(who)
    }

    fn mem(&self, who: &str, id: &str, rev: u64) -> SignedRecord {
        rk::memory(&self.id(who), id, rev, "active", "v")
    }

    fn put(&self, r: &SignedRecord) {
        for (p, b) in r.files() {
            self.remote.write(p, b);
        }
    }

    fn drop_record(&self, r: &SignedRecord) {
        for (p, _) in r.files() {
            self.remote.remove(p);
        }
    }

    fn commit(&self, m: &str) -> String {
        self.remote.commit(m)
    }

    fn put_commit(&self, r: &SignedRecord, m: &str) -> String {
        self.put(r);
        self.commit(m)
    }

    /// A statement for the head epoch naming `store_head`, issued an hour ago for a week, signed by `signers`.
    fn fresh(&self, store_head: &str, signers: &[&str]) -> String {
        self.fresh_with(&FreshSpec::for_head(&self.org, store_head, now() - 3600, 7 * DAY), signers)
    }

    fn fresh_with(&self, f: &FreshSpec, signers: &[&str]) -> String {
        let keys: Vec<_> = signers.iter().map(|n| self.id(n)).collect();
        let refs: Vec<&_> = keys.iter().collect();
        let (stmt, sigs) = f.sign(&refs);
        self.remote.write("roster/freshness.json", &stmt);
        self.remote.write("roster/freshness.sigs.json", &sigs);
        self.commit("freshness")
    }

    fn join_store(&self, store: &Store) {
        let (epoch, hash) = self.org.head_pair();
        let req = JoinRequest { url: self.remote.url(), admin_fingerprint: self.org.fingerprint("alice"), head_epoch: epoch, head_hash: hash };
        pins::join(&store.pin_storage(), &req, &self.org.files).unwrap();
    }

    fn join(&self) {
        self.join_store(&self.store);
    }

    fn sync(&self) -> VerifiedSync {
        self.store.sync_verified(&cfg()).unwrap()
    }

    /// An install that has never synced, joined at the org's current head.
    fn fresh_install(&self, name: &str) -> Store {
        let home = self.tmp.path().join(name);
        std::fs::create_dir_all(&home).unwrap();
        let s = Store::open(&home, "acme", "acme-main", &self.remote.url(), true).unwrap();
        self.join_store(&s);
        s
    }

    /// The baseline most tests start from: `records` and the roster in one commit, a statement for it in the
    /// next, the install joined and synced to that.
    fn installed(records: &[&SignedRecord]) -> (Fx, String) {
        let fx = Fx::standard();
        for r in records {
            fx.put(r);
        }
        let c1 = fx.commit("c1");
        let c2 = fx.fresh(&c1, &["alice"]);
        fx.join();
        let r = fx.sync();
        assert_eq!(accepted(&r).0, c2, "baseline: {r:?}");
        (fx, c1)
    }

    fn sticky(&self) -> StickyState {
        self.store.load_sticky().unwrap()
    }
}

fn accepted(r: &VerifiedSync) -> (&str, &FreshnessState) {
    match r {
        VerifiedSync::Accepted { head, freshness, .. } | VerifiedSync::UpToDate { head, freshness } => (head, freshness),
        other => panic!("expected an accepted sync, got {other:?}"),
    }
}

fn tamper(r: VerifiedSync) -> Vec<(String, &'static str)> {
    match r {
        VerifiedSync::Tampering { findings, .. } => {
            let mut v: Vec<_> = findings.into_iter().map(|f| (f.path, f.reason)).collect();
            v.sort();
            v
        }
        other => panic!("expected tampering, got {other:?}"),
    }
}

fn rejected(r: &VerifiedSync) -> &'static str {
    match r {
        VerifiedSync::Rejected { reason, .. } => reason,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn record_path(kind: &str, id: &str) -> String {
    format!("records/{kind}/{id}.md")
}

fn assert_stuck(fx: &Fx, at: &str) {
    assert_eq!(fx.store.accepted_head().unwrap().as_deref(), Some(at), "the accepted head did not move");
}

// --- the happy path and idempotence --------------------------------------------------------------------

#[test]
fn a_fresh_install_walks_from_the_statements_store_head_and_accepts() {
    let alice_note = |fx: &Fx| fx.mem("alice", A, 1);
    let fx = Fx::standard();
    fx.put(&alice_note(&fx));
    let c1 = fx.commit("c1");
    let c2 = fx.fresh(&c1, &["alice"]);
    fx.join();
    let r = fx.sync();
    match &r {
        VerifiedSync::Accepted { head, from: None, freshness, added, quarantined, .. } => {
            assert_eq!(head, &c2);
            assert!(freshness.endorsements_count(), "{freshness:?}");
            assert!(*added >= 6, "{added}");
            assert!(quarantined.is_empty(), "{quarantined:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fx.sticky().paths[&record_path("memory", A)].tracking, Tracking::Rev(1));
    // Syncing again reads nothing and writes nothing, but the freshness is re-judged against the clock.
    let before = std::fs::read(&fx.store.paths.sticky).unwrap();
    let r2 = fx.sync();
    assert!(matches!(&r2, VerifiedSync::UpToDate { head, freshness } if head == &c2 && freshness.endorsements_count()), "{r2:?}");
    assert_eq!(std::fs::read(&fx.store.paths.sticky).unwrap(), before);
    let later = fx.store.freshness_status(&VerifyConfig::at(now() + 30 * DAY)).unwrap();
    assert_eq!(later, FreshnessState::Stale { reason: "expired" }, "no fetch, no write: the clock alone made it stale");
    assert!(!later.endorsements_count());
}

#[test]
fn a_fresh_install_with_no_statement_has_no_trust_base_and_accepts_nothing() {
    let fx = Fx::standard();
    fx.put(&fx.mem("alice", A, 1));
    fx.commit("c1");
    fx.join();
    assert_eq!(rejected(&fx.sync()), "no-trust-base");
    assert_eq!(fx.store.accepted_head().unwrap(), None);
    assert!(!fx.store.paths.sticky.exists(), "a rejected first sync writes no state");
}

#[test]
fn an_unjoined_store_cannot_sync() {
    let fx = Fx::standard();
    fx.commit("c1");
    assert!(fx.store.sync_verified(&cfg()).is_err());
}

// --- freshness in the sync (011 tests 12 and 15) -------------------------------------------------------

#[test]
fn an_expired_statement_still_syncs_but_no_endorsement_counts() {
    let fx = Fx::standard();
    let c1 = fx.commit("c1");
    let old = FreshSpec::for_head(&fx.org, &c1, now() - 20 * DAY, 7 * DAY);
    fx.fresh_with(&old, &["alice"]);
    fx.join();
    let r = fx.sync();
    assert_eq!(accepted(&r).1, &FreshnessState::Stale { reason: "expired" });
    assert!(!accepted(&r).1.endorsements_count());
}

#[test]
fn a_statement_signed_by_a_non_admin_is_stale_and_cannot_be_a_trust_base() {
    let fx = Fx::standard();
    let c1 = fx.commit("c1");
    fx.fresh(&c1, &["dave"]);
    fx.join();
    assert_eq!(rejected(&fx.sync()), "no-trust-base");
}

#[test]
fn a_statement_for_another_store_or_with_a_far_future_expiry_counts_for_nothing() {
    for (label, edit) in [
        ("another store", Box::new(|f: &mut FreshSpec| f.store = "elsewhere".into()) as Box<dyn Fn(&mut FreshSpec)>),
        ("year 2099", Box::new(|f: &mut FreshSpec| f.expires_at = "2099-01-01T00:00:00Z".into())),
        ("issued ahead", Box::new(|f: &mut FreshSpec| {
            f.issued_at = delphi_xenia::time::format_utc(now() + 600);
            f.expires_at = delphi_xenia::time::format_utc(now() + 600 + DAY);
        })),
    ] {
        let fx = Fx::standard();
        let c1 = fx.commit("c1");
        let mut f = FreshSpec::for_head(&fx.org, &c1, now() - 3600, 7 * DAY);
        edit(&mut f);
        fx.fresh_with(&f, &["alice"]);
        fx.join();
        match fx.sync() {
            VerifiedSync::Accepted { freshness, .. } => assert!(!freshness.endorsements_count(), "{label}: {freshness:?}"),
            VerifiedSync::Rejected { reason, .. } => assert_eq!(reason, "no-trust-base", "{label}"),
            other => panic!("{label}: {other:?}"),
        }
    }
}

#[test]
fn a_store_head_that_is_not_a_descendant_of_the_statements_is_rejected() {
    let fx = Fx::standard();
    fx.commit("c1");
    // The statement names a commit this store has never had.
    fx.fresh(&"7".repeat(40), &["alice"]);
    fx.join();
    assert_eq!(rejected(&fx.sync()), "store-head-not-ancestor");
    assert_eq!(fx.store.accepted_head().unwrap(), None);
}

#[test]
fn a_hidden_newest_epoch_leaves_an_old_statement_stale_and_a_pinned_install_refuses_the_old_view() {
    let mut org = TestOrg::three_admins();
    let frank = org.who("frank");
    let epoch1 = org.clone();
    org.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    // (a) The store has epoch 2 but the statement is still the epoch-1 one: unexpired, validly signed, stale.
    let fx = Fx::new(epoch1.clone());
    let c1 = fx.commit("c1");
    let old_statement = FreshSpec::for_head(&epoch1, &c1, now() - 3600, 7 * DAY);
    fx.fresh_with(&old_statement, &["alice"]);
    fx.join();
    assert!(fx.sync().clone_freshness().endorsements_count());
    let mut fx = fx;
    fx.org = org.clone();
    fx.roster();
    fx.commit("epoch 2, nobody refreshed the statement");
    let r = fx.sync();
    assert_eq!(accepted(&r).1, &FreshnessState::Stale { reason: "older-epoch" }, "{r:?}");
    assert_eq!(pins::load(&fx.store.pin_storage()).unwrap().unwrap().head().0, 2, "the install learned epoch 2 and pinned it");
    // (b) An install that already knows epoch 2 is served the epoch-1-only view: refused outright.
    let fx2 = Fx::new(epoch1.clone());
    let c1 = fx2.commit("c1");
    fx2.fresh_with(&FreshSpec::for_head(&epoch1, &c1, now() - 3600, 7 * DAY), &["alice"]);
    let known = fx2.fresh_install("knows-epoch-2");
    let (e, h) = org.head_pair();
    let req = JoinRequest { url: fx2.remote.url(), admin_fingerprint: org.fingerprint("alice"), head_epoch: e, head_hash: h };
    // Join writes pins only after verifying the chain, so give the install its pins from the full chain directly.
    std::fs::remove_file(known.paths.state.join("pins.toml")).unwrap();
    pins::join(&known.pin_storage(), &req, &org.files).unwrap();
    let r = known.sync_verified(&cfg()).unwrap();
    assert_eq!(rejected(&r), delphi_xenia::roster::RosterError::PinnedEpochMissing { epoch: 2 }.reason());
}

impl VerifiedSync {
    fn clone_freshness(&self) -> FreshnessState {
        accepted(self).1.clone()
    }
}

// --- the history walk (011 test 13, 012 tests 6 and 21) -----------------------------------------------

/// Run `scenario` against the installed baseline and against a fresh install of the final state; both must see
/// the same tampering.
fn both_see(fx: &Fx, expect: &[(&str, &'static str)]) {
    let want: Vec<(String, &'static str)> = expect.iter().map(|(p, r)| (p.to_string(), *r)).collect();
    assert_eq!(tamper(fx.sync()), want, "an install with sticky state");
    let fresh = fx.fresh_install("fresh");
    assert_eq!(tamper(fresh.sync_verified(&cfg()).unwrap()), want, "a fresh install (statement base)");
}

#[test]
fn deleting_a_tombstone_an_endorsement_withdraw_a_dispute_or_a_roster_file_is_tampering() {
    let fx0 = Fx::standard();
    let (alice, dave) = (fx0.id("alice"), fx0.id("dave"));
    let cases: Vec<(&str, SignedRecord)> = vec![
        ("tombstone", rk::tombstone(&alice, T, 1, A, "withdraw")),
        ("endorsement withdraw", rk::endorsement(&dave, E, 1, A, 1, "withdraw")),
        ("endorsement dispute", rk::endorsement(&dave, B, 1, A, 1, "dispute")),
        ("memory", rk::memory(&alice, A, 1, "active", "v")),
    ];
    for (label, rec) in cases {
        let (fx, _) = Fx::installed(&[&rec]);
        fx.drop_record(&rec);
        fx.commit("delete");
        let md = rec.path.clone();
        let car = rec.sidecar_path.clone();
        // The record file is reported; the sidecar is reported as deleted too.
        let r = tamper(fx.sync());
        assert!(r.contains(&(md.clone(), "deleted")) && r.contains(&(car.clone(), "deleted")), "{label}: {r:?}");
        let fresh = fx.fresh_install("fresh");
        let rf = tamper(fresh.sync_verified(&cfg()).unwrap());
        assert_eq!(r, rf, "{label}: the same on a fresh install");
        assert!(fx.store.accepted_head().unwrap().is_some());
    }
}

#[test]
fn deleting_a_roster_signature_file_or_the_freshness_files_is_tampering_and_an_epoch_file_is_refused() {
    let (fx, _) = Fx::installed(&[]);
    fx.remote.remove("roster/freshness.sigs.json");
    fx.commit("drop the freshness signatures");
    assert_eq!(tamper(fx.sync()), vec![("roster/freshness.sigs.json".to_string(), "deleted")]);
    // A missing epoch file breaks the chain itself: refused before any record is looked at.
    let (fx, c1) = Fx::installed(&[]);
    let _ = c1;
    fx.remote.remove("roster/epochs/1.json");
    fx.commit("drop genesis");
    assert_eq!(rejected(&fx.sync()), "no-roster");
}

#[test]
fn a_valid_higher_rev_by_the_owner_is_accepted_and_supersedes() {
    let (fx, _) = Fx::installed(&[&rk::memory(&Fx::standard().id("alice"), A, 1, "active", "v1")]);
    let alice = fx.id("alice");
    let c3 = fx.put_commit(&rk::memory(&alice, A, 2, "active", "v2"), "rev 2");
    let r = fx.sync();
    assert!(matches!(&r, VerifiedSync::Accepted { head, superseded: 2, .. } if head == &c3), "{r:?}");
    assert_eq!(fx.sticky().paths[&record_path("memory", A)].tracking, Tracking::Rev(2));
}

/// `old` is stored, `newer` is a valid higher rev that the install accepts, then `back` is committed on top.
fn revert_case(old: SignedRecord, newer: SignedRecord, back: SignedRecord, reason: &'static str) {
    let (fx, _) = Fx::installed(&[&old]);
    fx.put_commit(&newer, "newer");
    let r = fx.sync();
    assert!(matches!(r, VerifiedSync::Accepted { .. }), "the valid higher rev is accepted first: {r:?}");
    let at = fx.store.accepted_head().unwrap().unwrap();
    fx.put_commit(&back, "revert");
    both_see(&fx, &[(&back.path, reason)]);
    assert_stuck(&fx, &at);
}

#[test]
fn a_revert_to_an_older_valid_rev_is_tampering_on_both_kinds_of_install() {
    let fx0 = Fx::standard();
    let (alice, dave) = (fx0.id("alice"), fx0.id("dave"));
    // An endorsement withdraw (rev 2) replaced by its valid rev 1 endorse.
    revert_case(rk::endorsement(&dave, E, 1, A, 1, "endorse"), rk::endorsement(&dave, E, 2, A, 1, "withdraw"), rk::endorsement(&dave, E, 1, A, 1, "endorse"), "rev-rollback");
    // A restore reverted to the withdraw, and the reverse.
    revert_case(rk::tombstone(&alice, T, 1, A, "withdraw"), rk::tombstone(&alice, T, 2, A, "restore"), rk::tombstone(&alice, T, 1, A, "withdraw"), "rev-rollback");
    revert_case(rk::tombstone(&alice, T, 1, A, "restore"), rk::tombstone(&alice, T, 2, A, "withdraw"), rk::tombstone(&alice, T, 1, A, "restore"), "rev-rollback");
    // A memory rev 3 replaced by rev 2.
    revert_case(rk::memory(&alice, A, 2, "active", "two"), rk::memory(&alice, A, 3, "active", "three"), rk::memory(&alice, A, 2, "active", "two"), "rev-rollback");
}

#[test]
fn the_same_rev_with_a_different_hash_is_tampering_and_the_same_hash_is_idempotent() {
    let fx0 = Fx::standard();
    let alice = fx0.id("alice");
    let (fx, _) = Fx::installed(&[&rk::memory(&alice, A, 3, "active", "original")]);
    let at = fx.store.accepted_head().unwrap().unwrap();
    // Idempotent: the identical bytes committed again change nothing, and an unrelated commit is fine.
    fx.put_commit(&rk::memory(&alice, A, 3, "active", "original"), "same bytes");
    fx.put_commit(&rk::memory(&alice, B, 1, "active", "other"), "another record");
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { unchanged, .. } if unchanged > 0));
    let at2 = fx.store.accepted_head().unwrap().unwrap();
    assert_ne!(at, at2);
    let swap = rk::memory(&alice, A, 3, "active", "swapped");
    fx.put_commit(&swap, "swap");
    both_see(&fx, &[(&swap.path, "same-rev-different-hash")]);
}

#[test]
fn a_takeover_by_another_member_at_a_higher_rev_is_detected_even_on_a_fresh_install() {
    // erin created the record; dave, a validly signed member, overwrites it at rev 2.
    let fx0 = Fx::standard();
    let erin_note = rk::memory(&fx0.id("erin"), A, 1, "active", "erin's");
    let (fx, _) = Fx::installed(&[&erin_note]);
    let takeover = rk::memory(&fx.id("dave"), A, 2, "active", "dave's");
    fx.put_commit(&takeover, "dave overwrites erin's id");
    // The signature is good, the rev is higher, standing is fine: only signer continuity sees it.
    both_see(&fx, &[(&takeover.path, "takeover")]);
}

#[test]
fn a_successor_may_continue_the_predecessors_record_and_nobody_else_may() {
    let mut org = TestOrg::three_admins();
    let fx0 = Fx::new(org.clone());
    let dave_note = rk::memory(&fx0.id("dave"), A, 1, "active", "dave's");
    drop(fx0);
    // Epoch 2 retires dave in favour of dave2 (routine rotation).
    org.advance(&["alice", "bob"], |t, s| t.rotate(s, "dave", "dave2", false));
    let fx = Fx::new(TestOrg::three_admins());
    fx.put(&dave_note);
    let c1 = fx.commit("c1");
    fx.fresh(&c1, &["alice"]);
    fx.join();
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }));
    let mut fx = fx;
    fx.org = org;
    fx.roster();
    let c3 = fx.commit("epoch 2");
    fx.fresh(&c3, &["alice"]);
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }), "retiring dave is not tampering with dave's stored record");
    // dave2, his successor, publishes rev 2 of the same id.
    let dave2_rev2 = rk::memory(&fx.id("dave2"), A, 2, "active", "dave2's");
    fx.put_commit(&dave2_rev2, "the successor continues");
    let r = fx.sync();
    assert!(matches!(r, VerifiedSync::Accepted { .. }), "{r:?}");
    // erin is not in the chain.
    let erin = rk::memory(&fx.id("erin"), A, 3, "active", "erin's");
    fx.put_commit(&erin, "erin tries");
    assert_eq!(tamper(fx.sync()), vec![(erin.path.clone(), "takeover")]);
}

#[test]
fn a_lower_rev_smuggled_into_a_later_commit_is_caught_by_the_walk_though_the_tip_looks_higher() {
    // The install stored rev 2. The host's history goes 2, 5, 3. The tip (rev 3) is above what the install
    // stored, so the sticky rule alone sees an ordinary supersede; only the walk sees 5 -> 3.
    let fx0 = Fx::standard();
    let alice = fx0.id("alice");
    let (fx, _) = Fx::installed(&[&rk::memory(&alice, A, 2, "active", "two")]);
    fx.put_commit(&rk::memory(&alice, A, 5, "active", "five"), "five");
    let low = rk::memory(&alice, A, 3, "active", "three");
    fx.put_commit(&low, "three");
    let sticky_before = fx.sticky();
    // Premise: the sticky rule alone is satisfied by the tip.
    let tip = fx.store.fetch().unwrap();
    assert!(matches!(tip, Fetch::Advanced { .. }));
    let mut obs = vec![];
    for (p, e) in &sticky_before.paths {
        obs.push(Observation { path: p.clone(), tracking: e.tracking, hash: e.hash.clone() });
    }
    obs.retain(|o| o.path != record_path("memory", A));
    obs.push(Observation { path: record_path("memory", A), tracking: Tracking::Rev(3), hash: hash::of(&low.md) });
    assert!(sticky_before.evaluate(&obs, &[]).is_clean(), "premise: the tip alone looks fine to the sticky rule");
    let got = tamper(fx.sync());
    assert_eq!(got, vec![(low.path.clone(), "rev-rollback")]);
    assert_eq!(fx.sticky(), sticky_before, "nothing was applied");
}

#[test]
fn a_delete_then_restore_in_two_commits_is_tampering_though_the_tip_matches_what_was_stored() {
    let fx0 = Fx::standard();
    let t = rk::tombstone(&fx0.id("alice"), T, 1, A, "withdraw");
    let (fx, _) = Fx::installed(&[&t]);
    fx.drop_record(&t);
    fx.commit("delete the tombstone");
    fx.put_commit(&t, "put it back");
    let got = tamper(fx.sync());
    assert!(got.contains(&(t.path.clone(), "deleted")), "{got:?}");
}

#[test]
fn a_deletion_hidden_in_a_merge_commit_is_seen_against_each_parent() {
    let fx0 = Fx::standard();
    let t = rk::tombstone(&fx0.id("alice"), T, 1, A, "withdraw");
    let (fx, _) = Fx::installed(&[&t]);
    let alice = fx.id("alice");
    // Two innocent branches; the merge commit is the only commit that drops the tombstone.
    fx.remote.git(&["checkout", "-q", "-b", "side"]);
    fx.put(&rk::memory(&alice, B, 1, "active", "side"));
    fx.remote.git(&["add", "-A"]);
    fx.remote.git(&["commit", "-q", "-m", "side work"]);
    fx.remote.git(&["checkout", "-q", "main"]);
    fx.put_commit(&rk::memory(&alice, A, 1, "active", "main work"), "main work");
    fx.remote.git(&["merge", "--no-ff", "--no-commit", "-q", "side"]);
    fx.drop_record(&t);
    let merge = fx.commit("merge");
    let parents = fx.remote.git(&["rev-list", "--parents", "-n", "1", &merge]);
    assert_eq!(parents.split_whitespace().count(), 3, "premise: a merge commit");
    let got = tamper(fx.sync());
    assert!(got.contains(&(t.path.clone(), "deleted")), "{got:?}");
}

#[test]
fn a_signer_retired_before_the_commit_is_tampering_but_one_retired_after_is_not() {
    let fx0 = Fx::standard();
    let dave_note = rk::memory(&fx0.id("dave"), A, 1, "active", "v1");
    let (mut fx, _) = Fx::installed(&[&dave_note]);
    // dave publishes rev 2 while still active; then epoch 2 retires him. Later he is just a retired member.
    let dave = fx.id("dave");
    fx.put_commit(&rk::memory(&dave, A, 2, "active", "v2"), "dave, while active");
    fx.org.advance(&["alice", "bob"], |t, s| t.rotate(s, "dave", "dave2", false));
    fx.roster();
    let c = fx.commit("epoch 2 retires dave");
    fx.fresh(&c, &["alice"]);
    let r = fx.sync();
    match &r {
        VerifiedSync::Accepted { quarantined, .. } => {
            let q: Vec<_> = quarantined.iter().map(|(p, why)| (p.as_str(), *why)).collect();
            assert!(q.contains(&(record_path("memory", A).as_str(), "not-active")), "tracked but not in force: {q:?}");
        }
        other => panic!("retiring a member must not read as tampering: {other:?}"),
    }
    assert_eq!(fx.sticky().paths[&record_path("memory", A)].tracking, Tracking::Rev(2), "still ordered by sticky state");
    // Now the retired key signs a new revision: no standing at the epoch in effect.
    let late = rk::memory(&dave, A, 3, "active", "after retirement");
    fx.put_commit(&late, "retired key publishes");
    assert_eq!(tamper(fx.sync()), vec![(late.path.clone(), "signer-not-active")]);
}

#[test]
fn a_retired_signers_withdraw_still_takes_effect_unless_the_entry_voids_tombstones() {
    for compromised in [false, true] {
        let mut org = TestOrg::three_admins();
        org.advance(&["alice", "bob"], |t, s| t.rotate(s, "dave", "dave2", compromised));
        // (1) The records are inside the history the admin signed: they are tracked, and quarantined unless
        // they can only hide.
        let fx = Fx::new(org.clone());
        let dave = fx.id("dave");
        let w = rk::tombstone(&dave, T, 1, A, "withdraw");
        let restore = rk::tombstone(&dave, T2, 1, A, "restore");
        fx.put(&w);
        fx.put(&restore);
        let c1 = fx.commit("records");
        fx.fresh(&c1, &["alice"]);
        fx.join();
        let r = fx.sync();
        let VerifiedSync::Accepted { quarantined, .. } = &r else { panic!("{r:?}") };
        let has = |p: &str| quarantined.iter().any(|(q, _)| q == p);
        assert_eq!(has(&w.path), compromised, "a withdraw from a retired DID stays in force (fail hidden) unless voids_tombstones is set");
        assert!(has(&restore.path), "a restore from a retired DID never counts");
        assert!(fx.sticky().paths.contains_key(&w.path) && fx.sticky().paths.contains_key(&restore.path), "tracked either way, so they cannot be rolled back");
        // (2) The same records pushed after the statement are refused by the walk, with the same exception.
        let fx = Fx::new(org);
        let c1 = fx.commit("roster");
        fx.fresh(&c1, &["alice"]);
        fx.join();
        assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }));
        fx.put(&w);
        fx.put(&restore);
        fx.commit("late records");
        let mut want = vec![(restore.path.clone(), "signer-not-active")];
        if compromised {
            want.push((w.path.clone(), "signer-not-active"));
        }
        want.sort();
        assert_eq!(tamper(fx.sync()), want);
    }
}


#[test]
fn an_authors_withdrawn_cannot_become_active_again() {
    let fx0 = Fx::standard();
    let alice = fx0.id("alice");
    let (fx, _) = Fx::installed(&[&rk::memory(&alice, A, 1, "active", "v")]);
    fx.put_commit(&rk::memory(&alice, A, 2, "withdrawn", "gone"), "withdraw");
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }));
    let back = rk::memory(&alice, A, 3, "active", "back");
    fx.put_commit(&back, "resurrect");
    both_see(&fx, &[(&back.path, "author-state-regression")]);
}

#[test]
fn a_record_that_stops_verifying_is_tampering_whether_its_bytes_or_its_sidecar_changed() {
    let fx0 = Fx::standard();
    let note = rk::memory(&fx0.id("alice"), A, 1, "active", "v");
    // One flipped byte of the record, sidecar untouched.
    let (fx, _) = Fx::installed(&[&note]);
    let mut flipped = note.md.clone();
    *flipped.last_mut().unwrap() ^= 1;
    fx.remote.write(&note.path, &flipped);
    fx.commit("flip a byte");
    both_see(&fx, &[(&note.path, "no-longer-verifies")]);
    // The sidecar replaced by one that belongs to other bytes.
    let (fx, _) = Fx::installed(&[&note]);
    let other = rk::memory(&fx.id("alice"), A, 1, "active", "other");
    fx.remote.write(&note.sidecar_path, &other.sidecar);
    fx.commit("swap the sidecar");
    both_see(&fx, &[(&note.path, "no-longer-verifies")]);
}

#[test]
fn deleting_the_freshness_files_or_adding_an_epoch_the_chain_does_not_hold_is_tampering_on_both_installs() {
    let (fx, _) = Fx::installed(&[]);
    // Deleted in one commit and put back in the next: the tip matches what is stored, only the walk objects.
    let sigs = std::fs::read(fx.remote.work_path().join("roster/freshness.sigs.json")).unwrap();
    fx.remote.remove("roster/freshness.sigs.json");
    fx.commit("drop the freshness signatures");
    fx.remote.write("roster/freshness.sigs.json", &sigs);
    fx.commit("put them back");
    both_see(&fx, &[("roster/freshness.sigs.json", "deleted")]);
    let (fx, _) = Fx::installed(&[]);
    fx.remote.write("roster/epochs/9.json", b"{}");
    fx.commit("an epoch nobody signed");
    both_see(&fx, &[("roster/epochs/9.json", "epoch-differs-from-chain")]);
}

#[test]
fn the_epoch_in_effect_comes_from_the_commits_own_tree_within_one_sync() {
    // One sync sees both the commit that retires dave and the later commit that he signs. The retirement is
    // inside the range, so only the walk's own epoch tracking can know dave was retired by then.
    let fx0 = Fx::standard();
    let dave_note = rk::memory(&fx0.id("dave"), A, 1, "active", "v1");
    let (mut fx, _) = Fx::installed(&[&dave_note]);
    let dave = fx.id("dave");
    fx.org.advance(&["alice", "bob"], |t, s| t.rotate(s, "dave", "dave2", false));
    fx.roster();
    fx.commit("epoch 2 retires dave");
    let late = rk::memory(&dave, A, 2, "active", "after");
    fx.put_commit(&late, "dave signs after retirement");
    // The statement still names epoch 1, so it is stale, but the roster chain itself is verified.
    assert_eq!(tamper(fx.sync()), vec![(late.path.clone(), "signer-not-active")]);
}

// --- the classifier: a file raises its stored rev only after it verifies --------------------------------

#[test]
fn a_forged_high_rev_is_quarantined_and_cannot_lock_out_the_real_record() {
    let (fx, _) = Fx::installed(&[]);
    let alice = fx.id("alice");
    // A hostile file at a new path claims rev 999 but its sidecar belongs to different bytes.
    let real = rk::memory(&alice, A, 1, "active", "real");
    let mut forged = rk::memory(&alice, A, 999, "active", "forged");
    forged.sidecar = real.sidecar.clone();
    fx.put(&forged);
    fx.commit("forgery");
    match fx.sync() {
        VerifiedSync::Accepted { quarantined, .. } => assert!(quarantined.iter().any(|(p, why)| p == &forged.path && *why == "hash-mismatch"), "{quarantined:?}"),
        other => panic!("{other:?}"),
    }
    assert!(!fx.sticky().paths.contains_key(&forged.path), "an unverified file is never tracked");
    // The real record, at rev 2, is not below a stored 999.
    fx.put_commit(&rk::memory(&alice, A, 2, "active", "real again"), "the owner writes");
    let r = fx.sync();
    assert!(matches!(r, VerifiedSync::Accepted { .. }), "{r:?}");
    assert_eq!(fx.sticky().paths[&forged.path].tracking, Tracking::Rev(2));
}

#[test]
fn a_record_signed_by_a_stranger_is_not_accepted() {
    let (fx, _) = Fx::installed(&[]);
    let mut org = fx.org.clone();
    let mallory = org.who("mallory");
    let r = rk::memory(&mallory, A, 1, "active", "m");
    fx.put_commit(&r, "stranger");
    assert_eq!(tamper(fx.sync()), vec![(r.path.clone(), "signer-not-active")]);
    assert!(!fx.sticky().paths.contains_key(&r.path));
}

// --- the roster and its pins ---------------------------------------------------------------------------

#[test]
fn the_pins_learn_new_epochs_and_a_forked_epoch_is_refused() {
    let (mut fx, _) = Fx::installed(&[]);
    let frank = fx.org.who("frank");
    let real = fx.org.clone();
    fx.org.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    fx.roster();
    let c = fx.commit("epoch 2");
    fx.fresh(&c, &["alice"]);
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }));
    let p = pins::load(&fx.store.pin_storage()).unwrap().unwrap();
    assert_eq!(p.head().0, 2);
    assert_eq!(p.epoch_hash(1), Some(real.specs[0].hash().as_str()), "an old pin is never forgotten");
    // A fork: a different epoch 2 that is itself validly signed by 2 of 3 admins.
    let mallory_member = fx.org.who("mallory");
    let fork = real.fork_at(2, &["alice", "carol"], |_, s| s.members.push(MemberSpec::new(&mallory_member, "mallory")));
    fx.remote.write("roster/epochs/2.json", &fork.files[1].epoch_json);
    fx.remote.write("roster/epochs/2.sigs.json", &fork.files[1].sigs_json);
    fx.commit("fork epoch 2");
    let r = fx.sync();
    assert_eq!(rejected(&r), delphi_xenia::roster::RosterError::PinMismatch { epoch: 2 }.reason());
    // 1 of 3 cannot even form a chain.
    let weak = real.fork_at(2, &["alice"], |_, s| s.members.push(MemberSpec::new(&mallory_member, "mallory")));
    fx.remote.write("roster/epochs/2.json", &weak.files[1].epoch_json);
    fx.remote.write("roster/epochs/2.sigs.json", &weak.files[1].sigs_json);
    fx.commit("weak fork");
    let reason = rejected(&fx.sync());
    assert!(reason == "quorum-not-met" || reason == delphi_xenia::roster::RosterError::PinMismatch { epoch: 2 }.reason(), "{reason}");
}

// --- rewrite recovery (011 test 14) --------------------------------------------------------------------

struct Rw {
    fx: Fx,
    c1: String,
    c2: String,
    tomb: SignedRecord,
    tomb2: SignedRecord,
    note: SignedRecord,
}

/// An install accepted at c2 holding a note and two tombstones; the host then rewrites history.
fn rewritten_baseline() -> Rw {
    let fx0 = Fx::standard();
    let alice = fx0.id("alice");
    let note = rk::memory(&alice, A, 1, "active", "note");
    let tomb = rk::tombstone(&alice, T, 1, A, "withdraw");
    let tomb2 = rk::tombstone(&alice, T2, 1, B, "withdraw");
    let (fx, c1) = Fx::installed(&[&note, &tomb, &tomb2]);
    let c2 = fx.store.accepted_head().unwrap().unwrap();
    Rw { fx, c1, c2, tomb, tomb2, note }
}

impl Rw {
    /// Rewrite: back to c1, `edit` the tree, commit, then a statement naming the replaced head `rewrite_of`.
    fn rewrite(&self, edit: impl FnOnce(&Fx), signers: &[&str], rewrite_of: &str) -> String {
        self.fx.remote.reset_hard(&self.c1);
        edit(&self.fx);
        let r1 = self.fx.commit("rewrite");
        let mut f = FreshSpec::for_head(&self.fx.org, &r1, now() - 600, 7 * DAY);
        f.rewrite_of = Some(rewrite_of.to_string());
        self.fx.fresh_with(&f, signers)
    }

    fn plan(&self) -> RewritePlan {
        match self.fx.store.plan_rewrite(&cfg()).unwrap() {
            Recovery::Plan(p) => p,
            other => panic!("expected a plan, got {other:?}"),
        }
    }

    fn plan_refusal(&self) -> &'static str {
        match self.fx.store.plan_rewrite(&cfg()).unwrap() {
            Recovery::Refused { reason, .. } => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn approve(&self, p: &RewritePlan) -> RewriteApproval {
        RewriteApproval { fetched: p.fetched.clone(), paths: p.changes.iter().map(|c| c.path.clone()).collect() }
    }
}

#[test]
fn a_force_pushed_store_is_refused_by_a_normal_sync() {
    let rw = rewritten_baseline();
    let r2 = rw.rewrite(|fx| fx.drop_record(&rw.tomb), &["alice", "bob"], &rw.c2);
    match rw.fx.sync() {
        VerifiedSync::NonFastForward(n) => assert_eq!((n.accepted.as_str(), n.fetched.as_str(), n.kind), (rw.c2.as_str(), r2.as_str(), Divergence::Diverged)),
        other => panic!("{other:?}"),
    }
    assert_stuck(&rw.fx, &rw.c2);
}

#[test]
fn a_quorum_statement_and_the_operators_full_list_accept_the_rewrite() {
    let rw = rewritten_baseline();
    let r2 = rw.rewrite(|fx| fx.drop_record(&rw.tomb), &["alice", "bob"], &rw.c2);
    let pins_before = std::fs::read(rw.fx.store.paths.state.join("pins.toml")).unwrap();
    let plan = rw.plan();
    assert_eq!((plan.accepted.as_str(), plan.fetched.as_str(), plan.kind), (rw.c2.as_str(), r2.as_str(), Some(Divergence::Diverged)));
    assert_eq!(plan.signing_people, 2);
    let listed: Vec<_> = plan.changes.iter().map(|c| (c.path.as_str(), c.reason)).collect();
    assert_eq!(listed, vec![(rw.tomb.path.as_str(), "deleted"), (rw.tomb.sidecar_path.as_str(), "deleted")], "every signed file the rewrite deletes is shown");
    let before = rw.fx.sticky();
    let out = rw.fx.store.accept_rewritten_head(&cfg(), &rw.approve(&plan)).unwrap();
    assert!(matches!(&out, RewriteOutcome::Accepted { head, replaced } if head == &r2 && replaced.len() == 2), "{out:?}");
    let after = rw.fx.sticky();
    assert_eq!(after.accepted_head.as_deref(), Some(r2.as_str()));
    // Only the approved paths changed; everything else survived byte for byte.
    for (p, e) in &before.paths {
        if p == &rw.tomb.path || p == &rw.tomb.sidecar_path {
            assert!(!after.paths.contains_key(p), "the approved deletion replaced the entry");
        } else if p.starts_with("roster/freshness") {
            assert!(after.paths.contains_key(p));
        } else {
            assert_eq!(after.paths.get(p), Some(e), "{p} was not approved and must survive");
        }
    }
    assert_eq!(std::fs::read(rw.fx.store.paths.state.join("pins.toml")).unwrap(), pins_before, "pinned epoch hashes are untouched");
    let log = std::fs::read_to_string(rw.fx.store.paths.state.join("rewrite-log.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 2, "{log}");
    assert!(log.contains(&rw.tomb.path) && log.contains("\"why\":\"deleted\""));
    // And now the store is an ordinary one again.
    assert!(matches!(rw.fx.sync(), VerifiedSync::UpToDate { .. }));
}

#[test]
fn a_partial_approval_applies_nothing() {
    let rw = rewritten_baseline();
    rw.rewrite(|fx| {
        fx.drop_record(&rw.tomb);
        fx.drop_record(&rw.tomb2);
    }, &["alice", "bob"], &rw.c2);
    let plan = rw.plan();
    assert_eq!(plan.changes.len(), 4, "two tombstones, each with its sidecar");
    // The operator approves only the first tombstone: the rewrite also deletes the second, so it is refused.
    let partial = RewriteApproval { fetched: plan.fetched.clone(), paths: [rw.tomb.path.clone(), rw.tomb.sidecar_path.clone()].into() };
    let before = rw.fx.sticky();
    match rw.fx.store.accept_rewritten_head(&cfg(), &partial).unwrap() {
        RewriteOutcome::NotApproved { missing } => assert_eq!(missing, vec![rw.tomb2.path.clone(), rw.tomb2.sidecar_path.clone()]),
        other => panic!("{other:?}"),
    }
    assert_eq!(rw.fx.sticky(), before);
    assert_stuck(&rw.fx, &rw.c2);
    // A path the plan does not list is not silently dropped from sticky state either.
    let mut extra = rw.approve(&plan);
    extra.paths.insert(rw.note.path.clone());
    assert!(matches!(rw.fx.store.accept_rewritten_head(&cfg(), &extra).unwrap(), RewriteOutcome::Refused { reason: "approval-names-unlisted-path", .. }));
    assert_eq!(rw.fx.sticky(), before);
}

#[test]
fn a_single_admin_statement_is_refused_and_so_are_two_keys_of_one_person() {
    let rw = rewritten_baseline();
    rw.rewrite(|fx| fx.drop_record(&rw.tomb), &["alice"], &rw.c2);
    assert_eq!(rw.plan_refusal(), "no-valid-statement");
    let approval = RewriteApproval { fetched: rw.fx.remote.host_head(), paths: [rw.tomb.path.clone()].into() };
    assert!(matches!(rw.fx.store.accept_rewritten_head(&cfg(), &approval).unwrap(), RewriteOutcome::Refused { reason: "no-valid-statement", .. }));
    assert_stuck(&rw.fx, &rw.c2);
    // A person with two admin keys is still one person.
    let mut org = TestOrg::three_admins();
    let alice2 = org.who("alice2");
    org.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&alice2, "alice").admin()));
    let fx = Fx::new(org);
    let note = fx.mem("alice", A, 1);
    fx.put(&note);
    let c1 = fx.commit("c1");
    let c2 = fx.fresh(&c1, &["alice"]);
    fx.join();
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }));
    fx.remote.reset_hard(&c1);
    fx.drop_record(&note);
    let r1 = fx.commit("rewrite");
    let mut f = FreshSpec::for_head(&fx.org, &r1, now() - 60, DAY);
    f.rewrite_of = Some(c2);
    fx.fresh_with(&f, &["alice", "alice2"]);
    match fx.store.plan_rewrite(&cfg()).unwrap() {
        Recovery::Refused { reason, .. } => assert_eq!(reason, "no-valid-statement"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_expired_statement_or_one_naming_another_head_is_refused() {
    let rw = rewritten_baseline();
    // Expired a day ago: authentic (a quorum signed it) but not current.
    rw.fx.remote.reset_hard(&rw.c1);
    rw.fx.drop_record(&rw.tomb);
    let r1 = rw.fx.commit("rewrite");
    let mut f = FreshSpec::for_head(&rw.fx.org, &r1, now() - 9 * DAY, 8 * DAY);
    f.rewrite_of = Some(rw.c2.clone());
    rw.fx.fresh_with(&f, &["alice", "bob"]);
    assert_eq!(rw.plan_refusal(), "statement-not-current");
    // Current, but about a different head than the one this install holds.
    let rw = rewritten_baseline();
    rw.rewrite(|fx| fx.drop_record(&rw.tomb), &["alice", "bob"], &"9".repeat(40));
    assert_eq!(rw.plan_refusal(), "rewrite-of-is-not-our-head");
    // A statement without rewrite_of is not a rewrite statement, whoever signed it.
    let rw = rewritten_baseline();
    rw.fx.remote.reset_hard(&rw.c1);
    rw.fx.drop_record(&rw.tomb);
    let r1 = rw.fx.commit("rewrite");
    rw.fx.fresh(&r1, &["alice", "bob", "carol"]);
    assert_eq!(rw.plan_refusal(), "statement-names-no-rewrite");
}

#[test]
fn an_approval_is_bound_to_the_head_the_operator_saw() {
    let rw = rewritten_baseline();
    rw.rewrite(|fx| fx.drop_record(&rw.tomb), &["alice", "bob"], &rw.c2);
    let plan = rw.plan();
    let approval = rw.approve(&plan);
    // The host moves again before the operator confirms.
    rw.fx.put_commit(&rw.fx.mem("alice", E, 1), "one more commit on the rewritten line");
    let before = rw.fx.sticky();
    assert!(matches!(rw.fx.store.accept_rewritten_head(&cfg(), &approval).unwrap(), RewriteOutcome::Refused { reason: "host-head-moved-since-approval", .. }));
    assert_eq!(rw.fx.sticky(), before);
}

#[test]
fn history_after_the_statements_store_head_is_still_walked_during_recovery() {
    let rw = rewritten_baseline();
    rw.rewrite(|fx| fx.drop_record(&rw.tomb), &["alice", "bob"], &rw.c2);
    // After the admins' statement, the host deletes the note in one more commit.
    rw.fx.drop_record(&rw.note);
    rw.fx.commit("one more deletion, after the admins looked");
    match rw.fx.store.plan_rewrite(&cfg()).unwrap() {
        Recovery::Refused { reason, findings } => {
            assert_eq!(reason, "walk-violations");
            assert!(findings.iter().any(|f| f.path == rw.note.path && f.reason == "deleted"), "{findings:?}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_rewrite_that_changes_the_creator_of_an_id_lists_that_path() {
    let rw = rewritten_baseline();
    let hijack = rk::memory(&rw.fx.id("dave"), A, 2, "active", "dave now owns it");
    rw.rewrite(|fx| fx.put(&hijack), &["alice", "bob"], &rw.c2);
    let plan = rw.plan();
    let listed: Vec<_> = plan.changes.iter().map(|c| (c.path.as_str(), c.reason)).collect();
    assert_eq!(listed, vec![(rw.note.path.as_str(), "creator-changed")], "a higher rev, validly signed, still listed because the creator changed");
    let none = RewriteApproval { fetched: plan.fetched.clone(), paths: BTreeSet::new() };
    assert!(matches!(rw.fx.store.accept_rewritten_head(&cfg(), &none).unwrap(), RewriteOutcome::NotApproved { .. }));
    assert!(matches!(rw.fx.store.accept_rewritten_head(&cfg(), &rw.approve(&plan)).unwrap(), RewriteOutcome::Accepted { .. }));
}

#[test]
fn a_host_going_back_in_time_is_refused_and_cannot_be_recovered_without_a_statement() {
    let rw = rewritten_baseline();
    rw.fx.remote.reset_hard(&rw.c1);
    rw.fx.remote.force_push();
    match rw.fx.sync() {
        VerifiedSync::NonFastForward(n) => assert_eq!(n.kind, Divergence::Behind),
        other => panic!("{other:?}"),
    }
    assert_eq!(rw.plan_refusal(), "no-valid-statement");
    assert_stuck(&rw.fx, &rw.c2);
}

#[test]
fn a_rewrite_cannot_change_a_pinned_epoch() {
    let mut org = TestOrg::three_admins();
    let frank = org.who("frank");
    let real = org.clone();
    org.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&frank, "frank")));
    let fx = Fx::new(org.clone());
    let c1 = fx.commit("c1");
    let c2 = fx.fresh(&c1, &["alice"]);
    fx.join();
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }));
    // The host rewrites to a different epoch 2 (valid, 2 of 3) with a quorum statement for it.
    let mut other = real.clone();
    let evil = other.who("evil");
    let fork = real.fork_at(2, &["alice", "carol"], |_, s| s.members.push(MemberSpec::new(&evil, "evil")));
    fx.remote.reset_hard(&c1);
    fx.remote.write("roster/epochs/2.json", &fork.files[1].epoch_json);
    fx.remote.write("roster/epochs/2.sigs.json", &fork.files[1].sigs_json);
    let r1 = fx.commit("rewrite with a forked epoch 2");
    let mut f = FreshSpec::for_head(&fork, &r1, now() - 60, DAY);
    f.rewrite_of = Some(c2.clone());
    let keys = [fork.key("alice"), fork.key("carol")];
    let (stmt, sigs) = f.sign(&[&keys[0], &keys[1]]);
    fx.remote.write("roster/freshness.json", &stmt);
    fx.remote.write("roster/freshness.sigs.json", &sigs);
    fx.commit("statement");
    match fx.store.plan_rewrite(&cfg()).unwrap() {
        Recovery::Refused { reason, .. } => assert_eq!(reason, delphi_xenia::roster::RosterError::PinMismatch { epoch: 2 }.reason()),
        other => panic!("{other:?}"),
    }
    assert_stuck(&fx, &c2);
}

#[test]
fn a_tampered_fast_forward_is_recoverable_with_a_quorum_and_the_operators_list() {
    let fx0 = Fx::standard();
    let t = rk::tombstone(&fx0.id("alice"), T, 1, A, "withdraw");
    let (fx, _) = Fx::installed(&[&t]);
    let c2 = fx.store.accepted_head().unwrap().unwrap();
    // The host deletes the tombstone in a plain fast-forward commit.
    fx.drop_record(&t);
    let bad = fx.commit("delete the tombstone");
    assert_eq!(tamper(fx.sync()).len(), 2);
    assert_stuck(&fx, &c2);
    // Admins investigate, decide to keep the deletion, and sign a statement for the commit they looked at.
    let mut f = FreshSpec::for_head(&fx.org, &bad, now() - 60, DAY);
    f.rewrite_of = Some(c2.clone());
    let head = fx.fresh_with(&f, &["alice", "carol"]);
    let rw = match fx.store.plan_rewrite(&cfg()).unwrap() {
        Recovery::Plan(p) => p,
        other => panic!("{other:?}"),
    };
    assert_eq!((rw.kind, rw.fetched.as_str()), (None, head.as_str()));
    let approval = RewriteApproval { fetched: rw.fetched.clone(), paths: rw.changes.iter().map(|c| c.path.clone()).collect() };
    assert!(matches!(fx.store.accept_rewritten_head(&cfg(), &approval).unwrap(), RewriteOutcome::Accepted { .. }));
    assert!(matches!(fx.sync(), VerifiedSync::UpToDate { .. }));
    assert!(!fx.sticky().paths.contains_key(&t.path));
}

#[test]
fn there_is_nothing_to_recover_on_an_ordinary_store() {
    let (fx, _) = Fx::installed(&[]);
    assert!(matches!(fx.store.plan_rewrite(&cfg()).unwrap(), Recovery::Refused { reason: "nothing-to-recover", .. }));
}

// --- replay of an older statement (follow-up to X4b) ---------------------------------------------------

fn stale_reason(r: &VerifiedSync) -> &'static str {
    match accepted(r).1 {
        FreshnessState::Stale { reason } => reason,
        other => panic!("expected stale, got {other:?}"),
    }
}

#[test]
fn replaying_an_older_authentic_statement_of_the_same_epoch_counts_no_endorsement_and_cannot_move_the_mark() {
    let (fx, c1) = Fx::installed(&[]);
    let mark = fx.sticky().freshness_mark.clone().expect("the accepted statement is recorded");
    assert_eq!(mark.issued, now() - 3600);
    // The host keeps an old, once-valid statement and serves it again in a later commit.
    let old = FreshSpec::for_head(&fx.org, &c1, now() - 7200, 7 * DAY);
    let c3 = fx.fresh_with(&old, &["alice"]);
    let r = fx.sync();
    assert_eq!(accepted(&r).0, c3, "the sync is accepted (nothing was deleted), but ...");
    assert_eq!(stale_reason(&r), "replayed-statement");
    assert_eq!(fx.sticky().freshness_mark, Some(mark.clone()), "a replay never moves the mark back");
    // The same store read again (no fetch): still stale.
    assert!(!fx.store.freshness_status(&cfg()).unwrap().endorsements_count());
    // A genuinely newer statement from an admin restores freshness and advances the mark.
    let newer = FreshSpec::for_head(&fx.org, &c1, now() - 60, 7 * DAY);
    let c4 = fx.fresh_with(&newer, &["alice"]);
    let r = fx.sync();
    assert_eq!(accepted(&r).0, c4);
    assert!(accepted(&r).1.endorsements_count(), "{:?}", accepted(&r).1);
    assert_eq!(fx.sticky().freshness_mark.unwrap().issued, now() - 60);
}

#[test]
fn an_equal_issued_at_with_different_bytes_is_a_replay_and_identical_bytes_are_not() {
    let (fx, c1) = Fx::installed(&[]);
    // Same issued_at as the stored statement, different life: different bytes, different signature.
    let twin = FreshSpec::for_head(&fx.org, &c1, now() - 3600, 6 * DAY);
    let c3 = fx.fresh_with(&twin, &["alice"]);
    let r = fx.sync();
    assert_eq!(accepted(&r).0, c3);
    assert_eq!(stale_reason(&r), "replayed-statement");
    assert_eq!(fx.sticky().freshness_mark.unwrap().issued, now() - 3600);
    // Back to the exact bytes already accepted (another admin's signature over them is fine): not a replay.
    let same = FreshSpec::for_head(&fx.org, &c1, now() - 3600, 7 * DAY);
    let c4 = fx.fresh_with(&same, &["alice", "bob"]);
    let r = fx.sync();
    assert_eq!(accepted(&r).0, c4);
    assert!(accepted(&r).1.endorsements_count(), "{:?}", accepted(&r).1);
}

#[test]
fn a_fresh_install_has_no_mark_and_takes_the_latest_statement_it_is_given() {
    let (fx, c1) = Fx::installed(&[]);
    let old = FreshSpec::for_head(&fx.org, &c1, now() - 7200, 7 * DAY);
    fx.fresh_with(&old, &["alice"]);
    let second = fx.fresh_install("second");
    let r = second.sync_verified(&cfg()).unwrap();
    assert!(accepted(&r).1.endorsements_count(), "no stored mark to replay against: {r:?}");
}

#[test]
fn a_sticky_file_written_before_the_mark_existed_still_loads() {
    let (fx, _) = Fx::installed(&[]);
    let mut v: serde_json::Value = serde_json::from_slice(&fx.sticky().to_json()).unwrap();
    v.as_object_mut().unwrap().remove("freshness_mark");
    let back = StickyState::from_json(&serde_json::to_vec(&v).unwrap(), "acme", "acme-main").unwrap();
    assert_eq!(back.freshness_mark, None);
}

// --- author state: nothing hidden by its author returns to active (X5 decision) ---------------------------------

#[test]
fn a_superseded_record_cannot_become_active_again_but_may_stay_hidden_or_be_withdrawn() {
    let fx0 = Fx::standard();
    let alice = fx0.id("alice");
    let (fx, _) = Fx::installed(&[&rk::memory(&alice, A, 1, "active", "v")]);
    fx.put_commit(&rk::memory(&alice, A, 2, "superseded", "replaced"), "supersede");
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }));
    // Staying hidden, or hiding harder, is fine.
    fx.put_commit(&rk::memory(&alice, A, 3, "superseded", "still"), "still superseded");
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }));
    fx.put_commit(&rk::memory(&alice, A, 4, "withdrawn", "gone"), "withdraw");
    assert!(matches!(fx.sync(), VerifiedSync::Accepted { .. }));
    // A withdrawn record is terminal: not active, not superseded.
    for (rev, status) in [(5, "active"), (5, "superseded")] {
        let (fx, _) = Fx::installed(&[&rk::memory(&alice, A, 1, "withdrawn", "gone")]);
        let back = rk::memory(&alice, A, rev, status, "back");
        fx.put_commit(&back, "regress");
        both_see(&fx, &[(&back.path, "author-state-regression")]);
    }
    // And superseded -> active is refused on both kinds of install.
    let (fx, _) = Fx::installed(&[&rk::memory(&alice, A, 1, "superseded", "replaced")]);
    let back = rk::memory(&alice, A, 2, "active", "back");
    fx.put_commit(&back, "un-supersede");
    both_see(&fx, &[(&back.path, "author-state-regression")]);
}

// --- X5 seam: a real synced snapshot feeds the pure tier computation ----------------------------------------

#[test]
fn a_synced_snapshot_feeds_the_tier_computation_and_a_stale_statement_withdraws_the_endorsement() {
    use delphi_xenia::layout::RecordPath;
    use delphi_xenia::record::validate_record;
    use delphi_xenia::tier::{compute, Inputs, Tier, VerifiedRecord};
    use std::collections::BTreeMap;
    let fx0 = Fx::standard();
    let m = rk::memory_with(&fx0.id("erin"), A, 1, "active", "v", "rust", "internal_empirical");
    let e = rk::endorsement_with(&fx0.id("dave"), E, 1, A, 1, &m.hash(), "endorse", "rust");
    let (fx, _) = Fx::installed(&[&m, &e]);
    let snap = fx.store.snapshot_of(&fx.sticky().accepted_head.unwrap()).unwrap();
    let roster = fx.org.verify().unwrap();
    let pinned = delphi_xenia::record::Pinned { org: "acme", store: "acme-main" };
    let records: Vec<VerifiedRecord> = snap
        .files
        .iter()
        .filter(|(p, _)| RecordPath::parse(p).is_ok())
        .map(|(p, md)| {
            let car = snap.files.get(&p.replace(".md", ".sig.json")).expect("sidecar");
            VerifiedRecord::from_validated(&validate_record(pinned, p, md, car, &|_| Ok(())).unwrap())
        })
        .collect();
    assert_eq!(records.len(), 2);
    let prior = BTreeMap::new();
    let fresh = fx.store.freshness_status(&cfg()).unwrap();
    let c = compute(&Inputs { roster: &roster, records: &records, freshness: &fresh, prior_tags: &prior });
    assert_eq!(c.tier_of(A), Some(Tier::L2), "{c:?}");
    // The same store read a month later: the statement has expired, no endorsement counts, nothing is deleted.
    let later = VerifyConfig::at(now() + 30 * DAY);
    let stale = fx.store.freshness_status(&later).unwrap();
    assert!(!stale.endorsements_count());
    let c = compute(&Inputs { roster: &roster, records: &records, freshness: &stale, prior_tags: &prior });
    assert_eq!(c.tier_of(A), Some(Tier::L1));
}
