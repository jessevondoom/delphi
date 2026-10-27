//! Standing, tiers, endorsements and tombstones, played by the attacker (spec 011 §10 tests 7-11, 16-19; spec 012
//! tests 5, 7-9, 22, 30). Every record here is really signed and goes through `validate_record` before it
//! reaches `compute`; only the roster and the freshness state are built directly.

use super::*;
use crate::identity::Identity;
use crate::record::{validate_record, Pinned};
use crate::roster::VerifiedRoster;
use crate::testkit::{EpochSpec, MemberSpec, TestOrg, ORG, STORE};
use crate::testkit_records::{self as rk, SignedRecord};

/// A ULID that sorts by `n` (smaller `n` sorts first).
fn u(n: usize) -> String {
    let alphabet = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    format!("01JBX3Q8T2M9Z4K7WYH5N6D0R{}", alphabet[n] as char)
}

fn fresh() -> FreshnessState {
    FreshnessState::Fresh { issued_at: "2026-10-26T11:00:00Z".into(), expires_at: "2026-11-02T11:00:00Z".into(), age_secs: 3600, life_used_pct: 1 }
}

fn stale() -> FreshnessState {
    FreshnessState::Stale { reason: "expired" }
}

/// alice, bob, carol admins; sam and sam2 two DIDs of the person `sam` (steward of payments); sue (payments, rust);
/// wes (`*`); rus (rust only); mia and mia_b two DIDs of the person `mia` (members, the authors); erin a member.
fn cast() -> TestOrg {
    let mut t = TestOrg::empty();
    let k = |t: &mut TestOrg, n: &str| t.who(n);
    let (alice, bob, carol) = (k(&mut t, "alice"), k(&mut t, "bob"), k(&mut t, "carol"));
    let (sam, sam2, sue, wes, rus) = (k(&mut t, "sam"), k(&mut t, "sam2"), k(&mut t, "sue"), k(&mut t, "wes"), k(&mut t, "rus"));
    let (mia, mia_b, erin) = (k(&mut t, "mia"), k(&mut t, "mia_b"), k(&mut t, "erin"));
    let genesis = EpochSpec {
        org: ORG.into(),
        store: STORE.into(),
        epoch: 1,
        prev_hash: None,
        members: vec![
            MemberSpec::new(&alice, "alice").admin(),
            MemberSpec::new(&bob, "bob").admin(),
            MemberSpec::new(&carol, "carol").admin(),
            MemberSpec::new(&sam, "sam").steward(&["payments"]),
            MemberSpec::new(&sam2, "sam").steward(&["payments"]),
            MemberSpec::new(&sue, "sue").steward(&["payments", "rust"]),
            MemberSpec::new(&wes, "wes").steward(&["*"]),
            MemberSpec::new(&rus, "rus").steward(&["rust"]),
            MemberSpec::new(&mia, "mia"),
            MemberSpec::new(&mia_b, "mia"),
            MemberSpec::new(&erin, "erin"),
        ],
    };
    t.push_signed(genesis, &["alice", "bob", "carol"]);
    t
}

struct W {
    org: TestOrg,
    roster: VerifiedRoster,
}

impl W {
    fn new() -> W {
        W::with(cast())
    }

    fn with(org: TestOrg) -> W {
        let roster = org.verify().expect("the test roster verifies");
        W { org, roster }
    }

    /// The cast after one more epoch (signed by alice and bob).
    fn after(edit: impl FnOnce(&mut TestOrg, &mut EpochSpec)) -> W {
        let mut t = cast();
        t.advance(&["alice", "bob"], edit);
        W::with(t)
    }

    fn key(&self, n: &str) -> Identity {
        self.org.key(n)
    }

    fn vr(&self, r: &SignedRecord) -> VerifiedRecord {
        let pinned = Pinned { org: &self.org.org, store: &self.org.store };
        let v = validate_record(pinned, &r.path, &r.md, &r.sidecar, &|_| Ok(())).unwrap_or_else(|e| panic!("test record {} does not validate: {e:?}", r.path));
        VerifiedRecord::from_validated(&v)
    }

    fn run(&self, rs: &[&SignedRecord], f: &FreshnessState) -> Computed {
        self.run_prior(rs, f, &BTreeMap::new())
    }

    fn run_prior(&self, rs: &[&SignedRecord], f: &FreshnessState, prior: &BTreeMap<String, BTreeSet<String>>) -> Computed {
        let records: Vec<VerifiedRecord> = rs.iter().map(|r| self.vr(r)).collect();
        compute(&Inputs { roster: &self.roster, records: &records, freshness: f, prior_tags: prior })
    }

    fn mem(&self, who: &str, id: usize, rev: u64) -> SignedRecord {
        rk::memory(&self.key(who), &u(id), rev, "active", "v")
    }

    /// An endorsement record `eid` by `who` with `verdict` for `target`'s exact current bytes.
    fn end(&self, who: &str, eid: usize, target: &SignedRecord, rev: u64, verdict: &str) -> SignedRecord {
        rk::endorsement_with(&self.key(who), &u(eid), 1, &target.id, rev, &target.hash(), verdict, "payments")
    }

    fn tomb(&self, who: &str, tid: usize, target: usize, action: &str, reason: &str) -> SignedRecord {
        rk::tombstone_with(&self.key(who), &u(tid), 1, &u(target), action, reason)
    }
}

fn outcome<'a>(c: &'a Computed, r: &SignedRecord) -> &'a Outcome {
    c.outcomes.get(&r.path).unwrap_or_else(|| panic!("no outcome for {}", r.path))
}

fn quarantined(c: &Computed, r: &SignedRecord) -> &'static str {
    match outcome(c, r) {
        Outcome::Quarantined(w) => w,
        other => panic!("expected quarantined, got {other:?} for {}", r.path),
    }
}

fn no_effect(c: &Computed, r: &SignedRecord) -> &'static str {
    match outcome(c, r) {
        Outcome::NoEffect(w) => w,
        other => panic!("expected no effect, got {other:?} for {}", r.path),
    }
}

fn mem_state<'a>(c: &'a Computed, m: &SignedRecord) -> &'a MemoryState {
    c.memories.get(&m.id).unwrap_or_else(|| panic!("{} is not in force", m.id))
}

fn persons(v: &[Endorser]) -> Vec<&str> {
    v.iter().map(|e| e.person.as_str()).collect()
}

// --- tiers (spec 011 §5.1; test 7) ---------------------------------------------------------------------

#[test]
fn a_member_signed_memory_is_l1_and_a_steward_endorsement_makes_it_l2_and_never_more() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let c = w.run(&[&m], &fresh());
    assert_eq!(mem_state(&c, &m).tier, Tier::L1);
    assert_eq!(mem_state(&c, &m).tier.source_type(), "federated_peer");
    let e = w.end("sam", 1, &m, 1, "endorse");
    let c = w.run(&[&m, &e], &fresh());
    let s = mem_state(&c, &m);
    assert_eq!((s.tier, persons(&s.endorsed_by)), (Tier::L2, vec!["sam"]));
    assert_eq!(s.tier.source_type(), "federated_verified");
    assert_eq!(outcome(&c, &e), &Outcome::Effective);
    // A wildcard steward and a second steward add nothing above L2.
    let e2 = w.end("wes", 2, &m, 1, "endorse");
    let c = w.run(&[&m, &e, &e2], &fresh());
    assert_eq!(mem_state(&c, &m).tier, Tier::L2);
    assert_eq!(persons(&mem_state(&c, &m).endorsed_by), vec!["sam", "wes"]);
}

#[test]
fn the_source_ceiling_clamps_even_an_endorsed_record() {
    let w = W::new();
    let k = w.key("mia");
    for (ceiling, expect) in [("external_untrusted", Tier::L0), ("federated_peer", Tier::L1), ("internal_generated", Tier::L2), ("user_ratified", Tier::L2)] {
        let m = rk::memory_with(&k, &u(0), 1, "active", "v", "payments", ceiling);
        let e = w.end("sam", 1, &m, 1, "endorse");
        let c = w.run(&[&m, &e], &fresh());
        let s = mem_state(&c, &m);
        assert_eq!(s.class_tier, Tier::L2, "{ceiling}");
        assert_eq!(s.tier, expect, "{ceiling}");
        assert_eq!(s.clamped_by_ceiling, expect < Tier::L2, "{ceiling}");
    }
}

#[test]
fn the_tier_is_a_pure_function_of_the_inputs_whatever_their_order() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let m2 = w.mem("mia", 5, 1);
    let e1 = w.end("sam", 1, &m, 1, "endorse");
    let e2 = w.end("sam2", 2, &m, 1, "endorse");
    let e3 = w.end("sue", 3, &m, 1, "endorse");
    let d = w.end("erin", 4, &m, 1, "dispute");
    let t1 = w.tomb("sam", 6, 5, "withdraw", "mistake");
    let t2 = w.tomb("sue", 7, 5, "restore", "mistake");
    let all = [&m, &m2, &e1, &e2, &e3, &d, &t1, &t2];
    let base = w.run(&all, &fresh());
    assert!(matches!(mem_state(&base, &m2).visibility, Visibility::Hidden(HiddenBy::Contested(_))));
    let mut order: Vec<&SignedRecord> = all.to_vec();
    for round in 0..12 {
        order.rotate_left(round % 7 + 1);
        if round % 3 == 0 {
            order.reverse();
        }
        assert_eq!(w.run(&order, &fresh()), base, "order {round}");
    }
}

// --- endorsement rules (spec 011 §7.3; tests 8, 9, 10, 11) --------------------------------------------------

#[test]
fn self_endorsement_is_ignored_even_from_another_install_or_after_a_rotation() {
    // A steward authors a record; the same person's second DID endorses it.
    let w = W::new();
    let m = w.mem("sam", 0, 1);
    let own = w.end("sam", 1, &m, 1, "endorse");
    let other_install = w.end("sam2", 2, &m, 1, "endorse");
    let c = w.run(&[&m, &own, &other_install], &fresh());
    assert_eq!(mem_state(&c, &m).tier, Tier::L1);
    assert_eq!(no_effect(&c, &own), "self-endorsement");
    assert_eq!(no_effect(&c, &other_install), "self-endorsement");
    // After a routine rotation the successor is the same person.
    let w = W::after(|t, spec| t.rotate(spec, "sam", "sam_new", false));
    let m = rk::memory(&w.key("sam_new"), &u(0), 2, "active", "v");
    let e = w.end("sam_new", 1, &m, 2, "endorse");
    let c = w.run(&[&m, &e], &fresh());
    assert_eq!(no_effect(&c, &e), "self-endorsement");
    assert_eq!(mem_state(&c, &m).tier, Tier::L1);
    // Control: the same endorsement from a different person counts.
    let ok = w.end("sue", 2, &m, 2, "endorse");
    assert_eq!(mem_state(&w.run(&[&m, &ok], &fresh()), &m).tier, Tier::L2);
}

#[test]
fn a_member_an_admin_or_a_steward_outside_the_area_cannot_endorse() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    for (who, eid, why) in [("erin", 1, "endorsement-not-steward"), ("alice", 2, "endorsement-not-steward"), ("rus", 3, "outside-areas")] {
        let e = w.end(who, eid, &m, 1, "endorse");
        let c = w.run(&[&m, &e], &fresh());
        assert_eq!(quarantined(&c, &e), why, "{who}");
        assert_eq!(mem_state(&c, &m).tier, Tier::L1, "{who}");
        assert!(mem_state(&c, &m).endorsed_by.is_empty());
    }
    // A withdraw is a steward act too.
    let wd = w.end("erin", 4, &m, 1, "withdraw");
    assert_eq!(quarantined(&w.run(&[&m, &wd], &fresh()), &wd), "endorsement-not-steward");
}

#[test]
fn the_scope_must_sit_inside_the_stewards_areas_and_the_revisions_tags() {
    let w = W::new();
    let m = w.mem("mia", 0, 1); // tags: payments
    // sue covers payments and rust, but the record is not about rust.
    let wide = rk::endorsement_with(&w.key("sue"), &u(1), 1, &m.id, 1, &m.hash(), "endorse", "payments, rust");
    assert_eq!(quarantined(&w.run(&[&m, &wide], &fresh()), &wide), "scope-outside-areas");
    let right = rk::endorsement_with(&w.key("sue"), &u(2), 1, &m.id, 1, &m.hash(), "endorse", "payments");
    assert_eq!(mem_state(&w.run(&[&m, &right], &fresh()), &m).tier, Tier::L2);
    // A scope the steward does not hold.
    let foreign = rk::endorsement_with(&w.key("sam"), &u(3), 1, &m.id, 1, &m.hash(), "endorse", "rust");
    assert_eq!(quarantined(&w.run(&[&m, &foreign], &fresh()), &foreign), "scope-outside-areas");
}

#[test]
fn an_untagged_record_is_endorsed_only_by_a_wildcard_steward() {
    let w = W::new();
    let m = rk::memory_with(&w.key("mia"), &u(0), 1, "active", "v", "", "internal_empirical");
    let by_sam = w.end("sam", 1, &m, 1, "endorse");
    let by_wes = w.end("wes", 2, &m, 1, "endorse");
    let c = w.run(&[&m, &by_sam, &by_wes], &fresh());
    assert_eq!(quarantined(&c, &by_sam), "outside-areas");
    assert_eq!(outcome(&c, &by_wes), &Outcome::Effective);
    assert_eq!(persons(&mem_state(&c, &m).endorsed_by), vec!["wes"]);
}

#[test]
fn an_endorsement_binds_to_the_exact_revision_and_bytes() {
    let w = W::new();
    let m1 = w.mem("mia", 0, 1);
    let e = w.end("sam", 1, &m1, 1, "endorse");
    assert_eq!(mem_state(&w.run(&[&m1, &e], &fresh()), &m1).tier, Tier::L2);
    // A new rev needs a new endorsement.
    let m2 = w.mem("mia", 0, 2);
    let c = w.run(&[&m2, &e], &fresh());
    assert_eq!(mem_state(&c, &m2).tier, Tier::L1);
    assert_eq!(no_effect(&c, &e), "target-rev-mismatch");
    // Same rev, replaced bytes: the hash no longer matches.
    let swapped = rk::memory(&w.key("mia"), &u(0), 1, "active", "other bytes");
    let c = w.run(&[&swapped, &e], &fresh());
    assert_eq!(mem_state(&c, &swapped).tier, Tier::L1);
    assert_eq!(no_effect(&c, &e), "hash-mismatch");
    // An endorsement of id@2 does not apply to id@1 either (the id@2 endorsement is for bytes that are not there).
    let e2 = w.end("sam", 2, &m2, 2, "endorse");
    let c = w.run(&[&m1, &e2], &fresh());
    assert_eq!(no_effect(&c, &e2), "target-rev-mismatch");
}

#[test]
fn duplicates_count_once_per_person_and_per_signer_the_smallest_ulid_survives() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    // Two keys of one person.
    let a = w.end("sam", 1, &m, 1, "endorse");
    let b = w.end("sam2", 2, &m, 1, "endorse");
    let c = w.run(&[&m, &a, &b], &fresh());
    assert_eq!(persons(&mem_state(&c, &m).endorsed_by), vec!["sam"], "one person, counted once");
    assert_eq!(outcome(&c, &a), &Outcome::Effective);
    assert_eq!(outcome(&c, &b), &Outcome::Effective, "both are valid records; the person is one endorser");
    // One signer, two files for one target: the smallest ULID survives and the other is a duplicate.
    let first = w.end("sam", 1, &m, 1, "endorse");
    let later = w.end("sam", 9, &m, 1, "withdraw");
    let c = w.run(&[&m, &later, &first], &fresh());
    assert_eq!(quarantined(&c, &later), "duplicate");
    assert_eq!(mem_state(&c, &m).tier, Tier::L2, "the surviving endorse stands; ULIDs are a tie-break only");
    // And the other way round: the surviving file is the withdraw.
    let early_withdraw = w.end("sam", 1, &m, 1, "withdraw");
    let late_endorse = w.end("sam", 9, &m, 1, "endorse");
    let c = w.run(&[&m, &late_endorse, &early_withdraw], &fresh());
    assert_eq!(quarantined(&c, &late_endorse), "duplicate");
    assert_eq!(mem_state(&c, &m).tier, Tier::L1);
}

#[test]
fn a_dispute_flags_the_record_and_never_by_itself_changes_the_tier() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let endorse = w.end("sue", 1, &m, 1, "endorse");
    let dispute = w.end("erin", 2, &m, 1, "dispute"); // any member may dispute
    // The dispute races the endorsement: both orders give the same answer.
    for order in [[&m, &endorse, &dispute], [&dispute, &m, &endorse]] {
        let c = w.run(&order, &fresh());
        let s = mem_state(&c, &m);
        assert_eq!(s.tier, Tier::L2, "a dispute alone does not demote");
        assert!(s.disputed());
        assert_eq!(s.disputes[0].person, "erin");
    }
    // A dispute alone on an unendorsed record: still L1, flagged.
    let c = w.run(&[&m, &dispute], &fresh());
    assert_eq!((mem_state(&c, &m).tier, mem_state(&c, &m).disputed()), (Tier::L1, true));
    // A dispute of an earlier revision is history, not a warning on the new one.
    let m2 = w.mem("mia", 0, 2);
    let c = w.run(&[&m2, &dispute], &fresh());
    assert!(!mem_state(&c, &m2).disputed());
    assert_eq!(no_effect(&c, &dispute), "dispute-of-another-revision");
}

#[test]
fn the_same_person_disputing_or_withdrawing_through_another_key_cancels_their_endorsement() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let endorse = w.end("sam", 1, &m, 1, "endorse");
    for (verdict, eid) in [("dispute", 2), ("withdraw", 3)] {
        let other_key = w.end("sam2", eid, &m, 1, verdict);
        let c = w.run(&[&m, &endorse, &other_key], &fresh());
        assert_eq!(mem_state(&c, &m).tier, Tier::L1, "{verdict}");
        assert_eq!(no_effect(&c, &endorse), "cancelled-by-same-person", "{verdict}");
    }
    // Another steward's withdraw retracts only their own.
    let sue_withdraw = w.end("sue", 4, &m, 1, "withdraw");
    assert_eq!(mem_state(&w.run(&[&m, &endorse, &sue_withdraw], &fresh()), &m).tier, Tier::L2);
}

#[test]
fn a_stale_freshness_statement_counts_no_endorsement_but_remembers_who_would() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let e = w.end("sam", 1, &m, 1, "endorse");
    let c = w.run(&[&m, &e], &stale());
    let s = mem_state(&c, &m);
    assert_eq!(s.tier, Tier::L1);
    assert!(s.endorsed_by.is_empty());
    assert_eq!(persons(&s.withheld_by_staleness), vec!["sam"]);
    assert_eq!(no_effect(&c, &e), "freshness-stale");
    // The same inputs under a fresh statement: L2.
    assert_eq!(mem_state(&w.run(&[&m, &e], &fresh()), &m).tier, Tier::L2);
}

#[test]
fn a_retired_removed_or_stranger_signer_endorses_nothing_but_a_retired_dispute_still_flags() {
    let m_author = "mia";
    let w = W::after(|t, spec| {
        spec.member_mut(&t.key("sam")).status = "retired";
        let rus = t.key("rus");
        let r = spec.member_mut(&rus);
        r.status = "removed";
        r.removed_at_epoch = Some(2);
    });
    let m = w.mem(m_author, 0, 1);
    let retired = w.end("sam", 1, &m, 1, "endorse");
    let removed = rk::endorsement_with(&w.key("rus"), &u(2), 1, &m.id, 1, &m.hash(), "endorse", "payments");
    let c = w.run(&[&m, &retired, &removed], &fresh());
    assert_eq!(quarantined(&c, &retired), "not-active");
    assert_eq!(quarantined(&c, &removed), "not-active");
    assert_eq!(mem_state(&c, &m).tier, Tier::L1);
    // Fail hidden: the retired steward's dispute still takes effect.
    let dispute = w.end("sam", 3, &m, 1, "dispute");
    let c = w.run(&[&m, &dispute], &fresh());
    assert!(mem_state(&c, &m).disputed());
    // A signature-valid stranger (not in the roster at all).
    let mut org = cast();
    let stranger = org.who("mallory");
    let w2 = W::with(org);
    let m2 = w2.mem("mia", 0, 1);
    let e = rk::endorsement_with(&stranger, &u(1), 1, &m2.id, 1, &m2.hash(), "endorse", "payments");
    let c = w2.run(&[&m2, &e], &fresh());
    assert_eq!(quarantined(&c, &e), "not-a-member");
}

#[test]
fn the_endorsement_of_a_hidden_or_withdrawn_record_counts_for_nothing() {
    let w = W::new();
    // The author withdrew it in a new revision.
    let m = rk::memory(&w.key("mia"), &u(0), 2, "withdrawn", "v");
    let e = w.end("sam", 1, &m, 2, "endorse");
    let c = w.run(&[&m, &e], &fresh());
    let s = mem_state(&c, &m);
    assert_eq!((s.visibility.clone(), s.tier), (Visibility::Hidden(HiddenBy::AuthorWithdrawn), Tier::L0));
    assert_eq!(no_effect(&c, &e), "target-hidden");
    assert!(s.endorsed_by.is_empty());
    // A tombstone does the same to a live record.
    let live = w.mem("mia", 3, 1);
    let e2 = w.end("sam", 4, &live, 1, "endorse");
    let t = w.tomb("mia", 5, 3, "withdraw", "mistake");
    let c = w.run(&[&live, &e2, &t], &fresh());
    assert_eq!(mem_state(&c, &live).tier, Tier::L0);
    assert_eq!(no_effect(&c, &e2), "target-hidden");
    // Superseded by its author is hidden too.
    let sup = rk::memory(&w.key("mia"), &u(6), 2, "superseded", "v");
    assert!(mem_state(&w.run(&[&sup], &fresh()), &sup).visibility.is_hidden());
}

// --- target resolution and ids (spec 012 test 5) -------------------------------------------------------------

#[test]
fn an_id_under_two_kinds_is_refused_in_every_kind() {
    let w = W::new();
    let victim = w.mem("mia", 0, 1);
    // An attacker member copies the victim's id into a tombstone and into an endorsement.
    let collide_t = rk::tombstone_with(&w.key("erin"), &victim.id, 1, &u(9), "withdraw", "mistake");
    let c = w.run(&[&victim, &collide_t], &fresh());
    assert_eq!(quarantined(&c, &victim), "id-collision");
    assert_eq!(quarantined(&c, &collide_t), "id-collision");
    assert!(c.memories.is_empty(), "no kind wins; nothing is loaded");
    // Control: without the collision both are fine.
    let ok = w.run(&[&victim], &fresh());
    assert_eq!(outcome(&ok, &victim), &Outcome::Effective);
    // memory vs endorsement
    let collide_e = rk::endorsement_with(&w.key("sam"), &victim.id, 1, &u(9), 1, &victim.hash(), "dispute", "payments");
    let c = w.run(&[&victim, &collide_e], &fresh());
    assert_eq!((quarantined(&c, &victim), quarantined(&c, &collide_e)), ("id-collision", "id-collision"));
}

#[test]
fn a_target_of_the_wrong_kind_or_none_resolves_to_nothing() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let tomb = w.tomb("mia", 1, 0, "restore", "mistake");
    // An endorsement aimed at a tombstone's id, and a tombstone aimed at an endorsement's id.
    let end_other = w.end("sam", 2, &m, 1, "endorse");
    let to_tomb = rk::endorsement_with(&w.key("sam"), &u(3), 1, &tomb.id, 1, &tomb.hash(), "endorse", "payments");
    let tomb_of_end = w.tomb("mia", 4, 2, "withdraw", "mistake");
    let missing_e = rk::endorsement_with(&w.key("sam"), &u(5), 1, &u(20), 1, &m.hash(), "endorse", "payments");
    let missing_t = w.tomb("mia", 6, 21, "withdraw", "mistake");
    let c = w.run(&[&m, &tomb, &end_other, &to_tomb, &tomb_of_end, &missing_e, &missing_t], &fresh());
    assert_eq!(no_effect(&c, &to_tomb), "target-wrong-kind");
    assert_eq!(no_effect(&c, &tomb_of_end), "target-wrong-kind");
    assert_eq!(no_effect(&c, &missing_e), "target-missing");
    assert_eq!(no_effect(&c, &missing_t), "target-missing");
    assert_eq!(mem_state(&c, &m).tier, Tier::L2, "the one real endorsement still counts");
    assert!(!mem_state(&c, &m).visibility.is_hidden());
}

#[test]
fn a_target_that_arrives_later_starts_to_count() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let e = w.end("sam", 1, &m, 1, "endorse");
    assert_eq!(no_effect(&w.run(&[&e], &fresh()), &e), "target-missing");
    assert_eq!(mem_state(&w.run(&[&e, &m], &fresh()), &m).tier, Tier::L2);
}

// --- tombstones (spec 012 §5.3; tests 9, 16, 17, 22) -------------------------------------------------------

fn hidden_by(c: &Computed, m: &SignedRecord) -> Option<HiddenBy> {
    match &mem_state(c, m).visibility {
        Visibility::Hidden(h) => Some(h.clone()),
        Visibility::Visible => None,
    }
}

#[test]
fn only_the_author_or_a_covering_steward_may_tombstone() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let by_member = w.tomb("erin", 1, 0, "withdraw", "mistake");
    let by_admin = w.tomb("alice", 2, 0, "withdraw", "mistake");
    let by_outside = w.tomb("rus", 3, 0, "withdraw", "mistake"); // steward of rust, record is payments
    let c = w.run(&[&m, &by_member, &by_admin, &by_outside], &fresh());
    for t in [&by_member, &by_admin, &by_outside] {
        assert_eq!(quarantined(&c, t), "tombstone-not-author-or-covering-steward");
    }
    assert_eq!(hidden_by(&c, &m), None);
    // The author's second key and a covering steward can.
    let by_author_key2 = w.tomb("mia_b", 4, 0, "withdraw", "mistake");
    assert_eq!(hidden_by(&w.run(&[&m, &by_author_key2], &fresh()), &m), Some(HiddenBy::AuthorTombstone));
    let by_steward = w.tomb("sam", 5, 0, "withdraw", "mistake");
    assert_eq!(hidden_by(&w.run(&[&m, &by_steward], &fresh()), &m), Some(HiddenBy::StewardTombstone));
}

#[test]
fn the_authors_tombstone_beats_stewards_except_a_sensitive_hold() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    // Stewards withdraw, the author restores: the author decides.
    let sam = w.tomb("sam", 1, 0, "withdraw", "mistake");
    let restore = w.tomb("mia", 2, 0, "restore", "mistake");
    assert_eq!(hidden_by(&w.run(&[&m, &sam, &restore], &fresh()), &m), None);
    // A steward's sensitive withdraw cannot be reversed by the author.
    let sensitive = w.tomb("sam", 1, 0, "withdraw", "sensitive");
    let c = w.run(&[&m, &sensitive, &restore], &fresh());
    assert_eq!(hidden_by(&c, &m), Some(HiddenBy::SensitiveHold));
    // Nor by another steward.
    let sue_restore = w.tomb("sue", 3, 0, "restore", "mistake");
    let c = w.run(&[&m, &sensitive, &restore, &sue_restore], &fresh());
    assert_eq!(hidden_by(&c, &m), Some(HiddenBy::SensitiveHold));
    // The same steward, through a successor key after a rotation, can: "that steward" is the same person.
    let w2 = W::after(|t, spec| t.rotate(spec, "sam", "sam_new", false));
    let m2 = w2.mem("mia", 0, 1);
    let sens = w2.tomb("sam", 1, 0, "withdraw", "sensitive");
    let author_restore = w2.tomb("mia", 2, 0, "restore", "mistake");
    let steward_restore = w2.tomb("sam_new", 3, 0, "restore", "mistake");
    let c = w2.run(&[&m2, &sens, &author_restore, &steward_restore], &fresh());
    assert_eq!(hidden_by(&c, &m2), None, "the successor's restore replaces the predecessor's sensitive withdraw");
    assert_eq!(no_effect(&c, &sens), "replaced-by-successor");
    // An author withdraw hides regardless of what stewards say.
    let withdraw = w.tomb("mia", 4, 0, "withdraw", "mistake");
    let c = w.run(&[&m, &withdraw, &sue_restore], &fresh());
    assert_eq!(hidden_by(&c, &m), Some(HiddenBy::AuthorTombstone));
}

#[test]
fn stewards_in_disagreement_make_it_contested_and_a_far_future_ulid_gains_nothing() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    for (withdraw_id, restore_id) in [(1, 30), (30, 1)] {
        let a = w.tomb("sam", withdraw_id, 0, "withdraw", "mistake");
        let b = w.tomb("sue", restore_id, 0, "restore", "mistake");
        let c = w.run(&[&m, &a, &b], &fresh());
        assert_eq!(hidden_by(&c, &m), Some(HiddenBy::Contested(vec!["sam".into(), "sue".into()])), "ULID order {withdraw_id}/{restore_id}");
    }
    // Agreement decides: all withdraw hides, all restore leaves it visible.
    let a = w.tomb("sam", 1, 0, "withdraw", "mistake");
    let b = w.tomb("sue", 2, 0, "withdraw", "mistake");
    assert_eq!(hidden_by(&w.run(&[&m, &a, &b], &fresh()), &m), Some(HiddenBy::StewardTombstone));
    let r1 = w.tomb("sam", 1, 0, "restore", "mistake");
    let r2 = w.tomb("sue", 2, 0, "restore", "mistake");
    assert_eq!(hidden_by(&w.run(&[&m, &r1, &r2], &fresh()), &m), None);
    // Two keys of one steward are one steward.
    let a2 = w.tomb("sam2", 3, 0, "withdraw", "mistake");
    assert_eq!(hidden_by(&w.run(&[&m, &a, &a2], &fresh()), &m), Some(HiddenBy::StewardTombstone));
}

#[test]
fn one_signer_has_one_tombstone_per_target_and_the_smallest_ulid_survives() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let first = w.tomb("mia", 1, 0, "restore", "mistake");
    let second = w.tomb("mia", 2, 0, "withdraw", "mistake");
    let c = w.run(&[&m, &second, &first], &fresh());
    assert_eq!(quarantined(&c, &second), "duplicate");
    assert_eq!(hidden_by(&c, &m), None, "the withdraw lost the tie-break; a changed mind must be a higher rev");
}

#[test]
fn the_area_check_for_a_tombstone_uses_the_union_of_tags_over_all_revisions() {
    let w = W::new();
    // The author retagged rev 2 away from payments to escape sam.
    let m = rk::memory_with(&w.key("mia"), &u(0), 2, "active", "v", "rust", "internal_empirical");
    let t = w.tomb("sam", 1, 0, "withdraw", "mistake");
    // With the history of tags, sam still covers the record.
    let prior: BTreeMap<String, BTreeSet<String>> = [(u(0), ["payments".to_string()].into_iter().collect())].into_iter().collect();
    assert_eq!(hidden_by(&w.run_prior(&[&m, &t], &fresh(), &prior), &m), Some(HiddenBy::StewardTombstone));
    // Without it only the tip's tags are visible: sam is outside (the documented weakness of an empty history).
    let c = w.run(&[&m, &t], &fresh());
    assert_eq!(quarantined(&c, &t), "tombstone-not-author-or-covering-steward");
    // But an endorsement uses the exact revision's tags only: sam cannot endorse the retagged rev.
    let e = rk::endorsement_with(&w.key("sam"), &u(2), 1, &m.id, 2, &m.hash(), "endorse", "payments");
    assert_eq!(quarantined(&w.run_prior(&[&m, &e], &fresh(), &prior), &e), "outside-areas");
}

#[test]
fn a_stale_freshness_statement_does_not_unhide_anything() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let t = w.tomb("sam", 1, 0, "withdraw", "mistake");
    let e = w.end("sue", 2, &m, 1, "endorse");
    for f in [fresh(), stale()] {
        let c = w.run(&[&m, &t, &e], &f);
        assert_eq!(hidden_by(&c, &m), Some(HiddenBy::StewardTombstone));
        assert_eq!(mem_state(&c, &m).tier, Tier::L0);
    }
}

#[test]
fn a_stolen_keys_tombstone_hides_until_voids_tombstones_is_set_and_never_restores() {
    // The thief holds mia's old key. mia rotates; the author's live record is signed by the successor.
    let routine = W::after(|t, spec| t.rotate(spec, "mia", "mia_new", false));
    let m = rk::memory(&routine.key("mia_new"), &u(0), 2, "active", "v");
    let thief_withdraw = routine.tomb("mia", 1, 0, "withdraw", "sensitive");
    let thief_restore = routine.tomb("mia", 2, 0, "restore", "mistake");
    // Before voids_tombstones (a routine rotation): the retired key's withdraw still hides (fail hidden) ...
    let c = routine.run(&[&m, &thief_withdraw], &fresh());
    assert_eq!(hidden_by(&c, &m), Some(HiddenBy::AuthorTombstone));
    assert_eq!(outcome(&c, &thief_withdraw), &Outcome::Effective);
    // ... but its restore never does anything.
    assert_eq!(quarantined(&routine.run(&[&m, &thief_restore], &fresh()), &thief_restore), "not-active");
    // The successor (the real human) can override the predecessor's tombstone by their own.
    let real_restore = routine.tomb("mia_new", 3, 0, "restore", "mistake");
    let c = routine.run(&[&m, &thief_withdraw, &real_restore], &fresh());
    assert_eq!(hidden_by(&c, &m), None);
    // After rotate --compromised (voids_tombstones): the stolen key's tombstones are void.
    let compromised = W::after(|t, spec| t.rotate(spec, "mia", "mia_new", true));
    let c = compromised.run(&[&m, &thief_withdraw], &fresh());
    assert_eq!(quarantined(&c, &thief_withdraw), "signer-voided");
    assert_eq!(hidden_by(&c, &m), None, "nothing the old key signed has standing");
    // The same holds for a stolen steward key's dispute and endorsement withdraw.
    let thief_dispute = compromised.end("mia", 4, &m, 2, "dispute");
    let c = compromised.run(&[&m, &thief_dispute], &fresh());
    assert_eq!(quarantined(&c, &thief_dispute), "signer-voided");
    assert!(!mem_state(&c, &m).disputed());
}

#[test]
fn a_removed_stewards_withdraw_still_hides_unless_the_entry_voids_it() {
    // A removed steward's withdraw of another member's record.
    let w = W::after(|t, spec| {
        let sam = t.key("sam");
        let s = spec.member_mut(&sam);
        s.status = "removed";
        s.removed_at_epoch = Some(2);
    });
    let m = w.mem("mia", 0, 1);
    let withdraw = w.tomb("sam", 1, 0, "withdraw", "mistake");
    let restore = w.tomb("sam", 2, 0, "restore", "mistake");
    assert_eq!(hidden_by(&w.run(&[&m, &withdraw], &fresh()), &m), Some(HiddenBy::StewardTombstone));
    assert_eq!(quarantined(&w.run(&[&m, &restore], &fresh()), &restore), "not-active");
    let w = W::after(|t, spec| {
        let sam = t.key("sam");
        let s = spec.member_mut(&sam);
        s.status = "removed";
        s.removed_at_epoch = Some(2);
        s.voids_tombstones = Some(true);
    });
    let c = w.run(&[&m, &withdraw], &fresh());
    assert_eq!(quarantined(&c, &withdraw), "signer-voided");
    assert_eq!(hidden_by(&c, &m), None);
}

#[test]
fn two_unchained_keys_of_the_author_that_disagree_fail_hidden() {
    let w = W::new();
    let m = w.mem("mia", 0, 1);
    let withdraw = w.tomb("mia", 1, 0, "withdraw", "mistake");
    let restore = w.tomb("mia_b", 2, 0, "restore", "mistake");
    // Commit order is not authenticated and neither key replaces the other: withdraw wins.
    for order in [[&m, &withdraw, &restore], [&restore, &withdraw, &m]] {
        assert_eq!(hidden_by(&w.run(&order, &fresh()), &m), Some(HiddenBy::AuthorTombstone));
    }
}

// --- standing -----------------------------------------------------------------------------------------------

#[test]
fn a_retired_authors_memory_has_no_standing_until_the_successor_republishes() {
    let w = W::after(|t, spec| t.rotate(spec, "mia", "mia_new", false));
    let old = w.mem("mia", 0, 1);
    let c = w.run(&[&old], &fresh());
    assert_eq!(quarantined(&c, &old), "not-active");
    assert!(c.memories.is_empty());
    let republished = rk::memory(&w.key("mia_new"), &u(0), 2, "active", "v");
    assert_eq!(mem_state(&w.run(&[&republished], &fresh()), &republished).tier, Tier::L1);
}

#[test]
fn a_memory_by_a_stranger_is_not_loaded() {
    let mut org = cast();
    let stranger = org.who("mallory");
    let w = W::with(org);
    let m = rk::memory(&stranger, &u(0), 1, "active", "v");
    let c = w.run(&[&m], &fresh());
    assert_eq!(quarantined(&c, &m), "not-a-member");
    assert!(c.memories.is_empty());
}

// --- author state -------------------------------------------------------------------------------------------

#[test]
fn nothing_hidden_by_its_author_returns_to_active() {
    use AuthorStatus::{Active, Superseded, Withdrawn};
    let allowed = [(Active, Active), (Active, Superseded), (Active, Withdrawn), (Superseded, Superseded), (Superseded, Withdrawn), (Withdrawn, Withdrawn)];
    for old in [Active, Superseded, Withdrawn] {
        for new in [Active, Superseded, Withdrawn] {
            assert_eq!(author_transition_allowed(old, new), allowed.contains(&(old, new)), "{old:?} -> {new:?}");
        }
    }
}

#[test]
fn a_wildcard_steward_is_still_bound_by_the_revisions_tags_when_it_has_any() {
    let w = W::new();
    let m = w.mem("mia", 0, 1); // tags: payments
    let off_topic = rk::endorsement_with(&w.key("wes"), &u(1), 1, &m.id, 1, &m.hash(), "endorse", "rust");
    assert_eq!(quarantined(&w.run(&[&m, &off_topic], &fresh()), &off_topic), "scope-outside-areas");
}

#[test]
fn two_different_files_for_one_path_are_refused_not_picked_between() {
    let w = W::new();
    let a = w.mem("mia", 0, 1);
    let b = rk::memory(&w.key("mia"), &u(0), 2, "active", "v");
    let c = w.run(&[&a, &b], &fresh());
    assert_eq!(quarantined(&c, &a), "duplicate-path");
    assert!(c.memories.is_empty());
    // The same file listed twice is one file.
    assert!(w.run(&[&a, &a], &fresh()).memories.contains_key(&a.id));
}

// --- the `bundle` kind is endorsed and hidden by the same rules (spec 027 §4.3) ---------------------------------

fn bundle_of(w: &W, who: &str, id: usize, rev: u64, version: &str, tags: &str) -> SignedRecord {
    let mut spec = rk::BundleSpec::new("acme-platform", version, &crate::hash::of(version.as_bytes()));
    spec.tags = tags.into();
    rk::bundle(&w.key(who), &u(id), rev, &spec)
}

#[test]
fn a_bundle_is_member_signed_until_a_steward_of_another_person_endorses_exactly_those_bytes() {
    let w = W::new();
    let b = bundle_of(&w, "mia", 0, 1, "1.4.0", "payments");
    let c = w.run(&[&b], &fresh());
    assert_eq!(c.bundle_level(&b.id), Some(BundleLevel::MemberSigned));
    assert_eq!(BundleLevel::MemberSigned.as_str(), "member-signed");
    let e = w.end("sam", 1, &b, 1, "endorse");
    let c = w.run(&[&b, &e], &fresh());
    assert_eq!(c.bundle_level(&b.id), Some(BundleLevel::Endorsed));
    assert_eq!(persons(&mem_state(&c, &b).endorsed_by), vec!["sam"]);
    assert_eq!(outcome(&c, &e), &Outcome::Effective);
    // An unknown id has no level.
    assert_eq!(c.bundle_level(&u(9)), None);
}

#[test]
fn an_endorsement_of_an_older_revision_does_not_carry_to_a_replayed_or_updated_one() {
    let w = W::new();
    let v1 = bundle_of(&w, "mia", 0, 1, "1.4.0", "payments");
    let e = w.end("sam", 1, &v1, 1, "endorse");
    assert_eq!(w.run(&[&v1, &e], &fresh()).bundle_level(&v1.id), Some(BundleLevel::Endorsed));
    // The publisher moves on to rev 2 (new bytes): the old endorsement names rev 1 and the old hash.
    let v2 = bundle_of(&w, "mia", 0, 2, "1.5.0", "payments");
    let c = w.run(&[&v2, &e], &fresh());
    assert_eq!(c.bundle_level(&v2.id), Some(BundleLevel::MemberSigned));
    assert_eq!(no_effect(&c, &e), "target-rev-mismatch");
    // Same rev, other bytes (a swapped tree under an endorsed number): the hash binds.
    let swapped = bundle_of(&w, "mia", 0, 1, "1.4.1", "payments");
    let c = w.run(&[&swapped, &e], &fresh());
    assert_eq!(c.bundle_level(&swapped.id), Some(BundleLevel::MemberSigned));
    assert_eq!(no_effect(&c, &e), "hash-mismatch");
}

#[test]
fn nobody_endorses_their_own_bundle_and_a_stale_store_endorses_nothing() {
    let w = W::new();
    // sam is a steward of payments and the publisher: neither sam's other DID nor sam counts.
    let b = bundle_of(&w, "sam", 0, 1, "1.4.0", "payments");
    let own = w.end("sam2", 1, &b, 1, "endorse");
    let c = w.run(&[&b, &own], &fresh());
    assert_eq!(c.bundle_level(&b.id), Some(BundleLevel::MemberSigned));
    assert_eq!(no_effect(&c, &own), "self-endorsement");
    // A real endorsement under a stale statement is withheld: the level falls back, the record is still visible.
    let b = bundle_of(&w, "mia", 0, 1, "1.4.0", "payments");
    let e = w.end("sam", 1, &b, 1, "endorse");
    let c = w.run(&[&b, &e], &stale());
    assert_eq!(c.bundle_level(&b.id), Some(BundleLevel::MemberSigned));
    assert_eq!(persons(&mem_state(&c, &b).withheld_by_staleness), vec!["sam"]);
}

#[test]
fn a_steward_outside_the_bundles_tags_and_a_plain_member_cannot_endorse_it() {
    let w = W::new();
    let b = bundle_of(&w, "mia", 0, 1, "1.4.0", "payments");
    for (who, eid, why) in [("erin", 1, "endorsement-not-steward"), ("rus", 2, "outside-areas")] {
        let e = w.end(who, eid, &b, 1, "endorse");
        let c = w.run(&[&b, &e], &fresh());
        assert_eq!(quarantined(&c, &e), why, "{who}");
        assert_eq!(c.bundle_level(&b.id), Some(BundleLevel::MemberSigned));
    }
}

#[test]
fn a_covering_steward_or_the_author_can_hide_a_bundle_and_nobody_else_can() {
    let w = W::new();
    let b = bundle_of(&w, "mia", 0, 1, "1.4.0", "payments");
    let e = w.end("sam", 9, &b, 1, "endorse");
    // The author withdraws.
    let own = w.tomb("mia", 2, 0, "withdraw", "mistake");
    assert_eq!(w.run(&[&b, &e, &own], &fresh()).bundle_level(&b.id), Some(BundleLevel::Hidden));
    // A steward whose areas cover the tags withdraws.
    let steward = w.tomb("sue", 3, 0, "withdraw", "sensitive");
    assert_eq!(w.run(&[&b, &steward], &fresh()).bundle_level(&b.id), Some(BundleLevel::Hidden));
    // A steward of another area and a plain member have no say.
    for (who, tid) in [("rus", 4), ("erin", 5)] {
        let t = w.tomb(who, tid, 0, "withdraw", "mistake");
        let c = w.run(&[&b, &t], &fresh());
        assert_eq!(quarantined(&c, &t), "tombstone-not-author-or-covering-steward", "{who}");
        assert_eq!(c.bundle_level(&b.id), Some(BundleLevel::MemberSigned));
    }
    // An author-set status hides it as well, and an endorsement does not undo that.
    let mut spec = rk::BundleSpec::new("acme-platform", "1.4.0", &crate::hash::of(b"x"));
    spec.status = "withdrawn".into();
    let gone = rk::bundle(&w.key("mia"), &u(0), 1, &spec);
    let e = w.end("sam", 1, &gone, 1, "endorse");
    let c = w.run(&[&gone, &e], &fresh());
    assert_eq!(c.bundle_level(&gone.id), Some(BundleLevel::Hidden));
    assert_eq!(no_effect(&c, &e), "target-hidden");
}

#[test]
fn a_bundle_and_a_memory_cannot_share_an_id_and_a_non_member_publishes_nothing() {
    let w = W::new();
    let b = bundle_of(&w, "mia", 0, 1, "1.4.0", "payments");
    let m = w.mem("mia", 0, 1);
    let c = w.run(&[&b, &m], &fresh());
    assert_eq!((quarantined(&c, &b), quarantined(&c, &m)), ("id-collision", "id-collision"));
    assert_eq!(c.bundle_level(&b.id), None);
    // A key the roster has never heard of.
    let stranger = Identity::from_seed([99; 32]);
    let theirs = rk::bundle(&stranger, &u(3), 1, &rk::BundleSpec::new("acme-platform", "1.4.0", &crate::hash::of(b"x")));
    let c = w.run(&[&theirs], &fresh());
    assert_eq!(quarantined(&c, &theirs), "not-a-member");
    assert_eq!(c.bundle_level(&theirs.id), None);
}

// --- X10: the clause of spec 011 test 7 that had no test -------------------------------------------------------

#[test]
fn evidence_pointers_alone_change_no_tier() {
    // A member's memory that carries `evidence` pointers is exactly as trusted as one that carries none: L1
    // unendorsed, L2 only through a steward's endorsement of the exact bytes, never L2 on pointers alone.
    let w = W::new();
    let plain = w.mem("mia", 0, 1);
    let text = String::from_utf8(plain.md.clone()).unwrap();
    let evidence = "evidence:\n  - scheme: https\n    locator: \"https://docs.acme.example/proof\"\n    pin:\n      kind: none\n    label: Proof\n    content_type: text/plain\n";
    assert!(text.contains("evidence: []\n"));
    let with = rk::sign_record(&w.key("mia"), plain.kind, &plain.id, text.replace("evidence: []\n", evidence).into_bytes());
    let c = w.run(&[&with], &fresh());
    assert_eq!(outcome(&c, &with), &Outcome::Effective);
    assert_eq!(mem_state(&c, &with).tier, Tier::L1, "evidence pointers do not lift a record");
    assert!(mem_state(&c, &with).endorsed_by.is_empty());
    let boasting = rk::sign_record(&w.key("mia"), plain.kind, &plain.id, text.replace("evidence: []\n", &evidence.replace("label: Proof", "label: Verified by the steward")).into_bytes());
    assert_eq!(mem_state(&w.run(&[&boasting], &fresh()), &boasting).tier, Tier::L1);
    // The endorsement is what lifts it, evidence or not.
    let e = w.end("sam", 1, &with, 1, "endorse");
    assert_eq!(mem_state(&w.run(&[&with, &e], &fresh()), &with).tier, Tier::L2);
}
