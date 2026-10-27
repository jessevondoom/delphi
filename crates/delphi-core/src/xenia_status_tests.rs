//! Tests for what `status`, `doctor` and the events read (spec 011 §4.1, §5.3). Real stores, real signatures.

use super::*;
use crate::xenia_fixture::*;
use delphi_xenia::pins;
use delphi_xenia::testkit::{EpochSpec, MemberSpec, TestOrg};
use delphi_xenia::testkit_records as rk;

fn home(fx: &Fx) -> std::path::PathBuf {
    fx.dir.join("home")
}

fn mem(fx: &Fx, who: &str, n: usize, tags: &str) -> SignedRecord {
    rk::memory_with(&fx.id(who), &u(n), 1, "active", "v", tags, "internal_empirical")
}

fn read(fx: &Fx, cfg: &VerifyConfig) -> Status {
    let r = list_stores(&home(fx)).remove(0);
    status(&fx.store, &r, cfg).unwrap()
}

/// An org of two admins (below the three the roster quorum wants) and a steward.
fn small_org() -> TestOrg {
    let mut t = TestOrg::empty();
    let (a, b, d, e) = (t.who("alice"), t.who("bob"), t.who("dave"), t.who("erin"));
    let genesis = EpochSpec {
        org: delphi_xenia::testkit::ORG.into(),
        store: delphi_xenia::testkit::STORE.into(),
        epoch: 1,
        prev_hash: None,
        members: vec![MemberSpec::new(&a, "alice").admin(), MemberSpec::new(&b, "bob").admin(), MemberSpec::new(&d, "dave").steward(&["rust"]), MemberSpec::new(&e, "erin")],
    };
    t.push_signed(genesis, &["alice", "bob"]);
    t
}

#[test]
fn a_joined_store_is_found_by_its_pins_and_picked_by_name_or_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let found = list_stores(&home(&fx));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id(), "acme/acme-main");
    assert_eq!(pick_store(&home(&fx), None).unwrap(), found[0]);
    assert_eq!(pick_store(&home(&fx), Some("acme/acme-main")).unwrap(), found[0]);
    assert!(pick_store(&home(&fx), Some("acme/other")).unwrap_err().contains("no joined store"));
    let empty = tempfile::tempdir().unwrap();
    assert!(list_stores(empty.path()).is_empty());
    assert!(pick_store(empty.path(), None).unwrap_err().contains("no store is joined"));
    // Something that is not a store's own pins is not listed.
    std::fs::create_dir_all(home(&fx).join("xenia/evil/x/state")).unwrap();
    std::fs::write(home(&fx).join("xenia/evil/x/state/pins.toml"), "not toml at all").unwrap();
    assert_eq!(list_stores(&home(&fx)).len(), 1);
}

#[test]
fn status_counts_tiers_lists_contested_and_hidden_and_the_roster() {
    let tmp = tempfile::tempdir().unwrap();
    let fx0 = Fx::with_cast(tmp.path());
    let m0 = mem(&fx0, "mia", 0, "payments");
    let e0 = endorse(&fx0.id("sam"), 10, &m0, 1, "payments");
    let m1 = mem(&fx0, "mia", 1, "payments");
    let contested = mem(&fx0, "mia", 2, "payments");
    let t_sam = rk::tombstone_with(&fx0.id("sam"), &u(22), 1, &u(2), "withdraw", "mistake");
    let t_wes = rk::tombstone_with(&fx0.id("wes"), &u(23), 1, &u(2), "restore", "mistake");
    let (fx, _) = Fx::installed_on(fx0, &[&m0, &e0, &m1, &contested, &t_sam, &t_wes]);
    let st = read(&fx, &cfg());
    assert!(st.head.is_some());
    assert_eq!((st.l2, st.l1), (1, 1), "{st:?}");
    assert_eq!(st.contested, vec![u(2)]);
    assert!(st.hidden.contains(&(u(2), "contested")));
    let r = st.roster.clone().unwrap();
    assert_eq!((r.epoch, r.admins, r.stewards, r.members), (1, 3, 3, 8));
    let f = st.freshness.clone().unwrap();
    assert_eq!((f.state.as_str(), f.age_secs), ("fresh", Some(3600)));
    assert!(st.warnings.iter().any(|w| w.id == "contested"), "{:?}", st.warnings);
    assert!(!st.warnings.iter().any(|w| w.id == "few-admins" || w.id == "stale"));
    let text = render(&st, now());
    assert!(text.contains("1 at L2") && text.contains("Contested") && text.contains(&u(2)), "{text}");
    assert_eq!(to_json(&st)["contested"][0], u(2));
}

#[test]
fn fewer_than_three_admins_is_a_warning_and_three_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed_on(Fx::new(tmp.path(), small_org()), &[]);
    let st = read(&fx, &cfg());
    assert_eq!(st.roster.as_ref().unwrap().admins, 2);
    let w = st.warnings.iter().find(|w| w.id == "few-admins").expect("warned");
    assert!(w.text.contains("only 2 admin"), "{}", w.text);
    assert!(render(&st, now()).contains("what to do: Add admins: `delphi xenia help being-an-admin`."));
    let tmp3 = tempfile::tempdir().unwrap();
    let (fx3, _) = Fx::installed(tmp3.path(), &[]);
    assert!(read(&fx3, &cfg()).warnings.iter().all(|w| w.id != "few-admins"));
}

#[test]
fn a_statement_past_half_its_life_warns_and_a_lapsed_one_says_no_endorsement_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    // Issued an hour before the test clock, good for seven days.
    let young = read(&fx, &VerifyConfig::at(now() + DAY));
    assert!(young.warnings.iter().all(|w| w.id != "statement-aging" && w.id != "stale"), "{:?}", young.warnings);
    let used = young.freshness.as_ref().unwrap().life_used_pct.unwrap();
    assert!(used < 50, "{used}");
    let aging = read(&fx, &VerifyConfig::at(now() + 4 * DAY));
    let used = aging.freshness.as_ref().unwrap().life_used_pct.unwrap();
    assert!((50..100).contains(&used), "{used}");
    assert!(aging.warnings.iter().any(|w| w.id == "statement-aging" && w.text.contains(&format!("{used}%"))), "{:?}", aging.warnings);
    let lapsed = read(&fx, &VerifyConfig::at(now() + 8 * DAY));
    assert_eq!(lapsed.freshness.as_ref().unwrap().state, "expired");
    assert!(lapsed.warnings.iter().any(|w| w.id == "stale" && w.text.contains("no endorsement counts")));
    let shown = render(&lapsed, now() + 8 * DAY);
    assert!(shown.contains("STALE") && shown.contains("what to do: Ask an admin to run `delphi xenia admin refresh`"), "{shown}");
}

#[test]
fn a_sync_is_journalled_and_tampering_stays_visible_in_status() {
    let tmp = tempfile::tempdir().unwrap();
    let fx0 = Fx::with_cast(tmp.path());
    let m0 = mem(&fx0, "mia", 0, "payments");
    let (fx, _) = Fx::installed_on(fx0, &[&m0]);
    let ok = sync_and_record(&fx.store, &cfg()).unwrap();
    assert!(matches!(ok, VerifiedSync::UpToDate { .. }));
    let j = read_journal(&fx.store);
    assert_eq!((j.len(), j[0].seq, j[0].kind.as_str(), j[0].outcome.as_str()), (1, 1, "sync", "up-to-date"));
    // The host deletes a signed record: the sync refuses the whole head and says where.
    fx.remote.remove(&m0.path);
    fx.commit("delete a record");
    let bad = sync_and_record(&fx.store, &cfg()).unwrap();
    assert!(matches!(bad, VerifiedSync::Tampering { .. }), "{bad:?}");
    let st = read(&fx, &cfg());
    assert_eq!(st.tampering.len(), 1, "{:?}", st.tampering);
    assert_eq!(st.tampering[0].0, m0.path);
    assert!(st.warnings.iter().any(|w| w.id == "tampering" && w.severity == Severity::Fail));
    assert_eq!(st.l1, 1, "what the install had stays in force");
    let text = render(&st, now());
    assert!(text.contains("Tampering found") && text.contains(&m0.path), "{text}");
    // Under the failure, what to do; and the reason word is explained in the catalog's own words.
    assert!(text.contains("what to do: Do not retry. Tell an admin what `delphi xenia status` shows"), "{text}");
    assert!(text.contains("What the words above mean") && text.contains("deleted: A file that was in the store is gone."), "{text}");
    assert!(to_json(&st)["warnings"][0]["help"]["next"].is_string());
    // The finding stays on the next sync too (the deletion is in the history now); only a newer clean sync clears it.
    assert!(matches!(sync_and_record(&fx.store, &cfg()).unwrap(), VerifiedSync::Tampering { .. }));
    assert_eq!(read(&fx, &cfg()).tampering.len(), 1);
    append(&fx.store, "sync", "accepted", None, json!({}), now()).unwrap();
    assert!(read(&fx, &cfg()).tampering.is_empty());
    let j = read_journal(&fx.store);
    assert_eq!(j.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
}

#[test]
fn the_journal_is_bounded_private_and_keeps_counting() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    for i in 0..(MAX_ENTRIES + 5) {
        append(&fx.store, "sync", "up-to-date", None, json!({"i": i}), now()).unwrap();
    }
    let j = read_journal(&fx.store);
    assert_eq!(j.len(), MAX_ENTRIES);
    assert_eq!(j.last().unwrap().seq as usize, MAX_ENTRIES + 5, "sequence numbers keep counting");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(journal_path(&fx.store)).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn status_before_the_first_sync_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = Fx::with_cast(tmp.path());
    let c = fx.commit("c1");
    fx.fresh(&c);
    fx.join();
    let st = read(&fx, &cfg());
    assert!(st.head.is_none() && st.roster.is_none());
    assert!(st.warnings.iter().any(|w| w.id == "not-synced"));
}

#[test]
fn join_verifies_the_chain_against_what_the_operator_typed_and_writes_the_pins() {
    let tmp = tempfile::tempdir().unwrap();
    let fx = Fx::with_cast(tmp.path());
    let c = fx.commit("c1");
    fx.fresh(&c);
    let (epoch, h) = fx.org.head_pair();
    let other = tempfile::tempdir().unwrap();
    let store = Store::open(other.path(), "acme", "acme-main", &fx.remote.url(), true).unwrap();
    // A wrong admin fingerprint is refused and writes nothing.
    let bad = pins::JoinRequest { url: fx.remote.url(), admin_fingerprint: "0".repeat(64), head_epoch: epoch, head_hash: h.clone() };
    assert!(store.join(&bad).is_err());
    assert!(list_stores(other.path()).is_empty());
    let req = pins::JoinRequest { url: fx.remote.url(), admin_fingerprint: fx.org.fingerprint("alice"), head_epoch: epoch, head_hash: h };
    let p = store.join(&req).unwrap();
    assert_eq!((p.org(), p.store()), ("acme", "acme-main"));
    assert_eq!(list_stores(other.path()).len(), 1);
    assert!(store.join(&req).is_err(), "joining twice is refused");
    // And the first verified sync accepts the store.
    assert!(matches!(store.sync_verified(&cfg()).unwrap(), VerifiedSync::Accepted { .. }));
}
