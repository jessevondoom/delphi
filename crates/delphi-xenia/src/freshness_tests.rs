//! Freshness statements, played by the attacker (spec 011 §10 tests 12 and 15; spec 012 test 15).

use super::*;
use crate::testkit::{TestOrg, MemberSpec};
use crate::testkit_records::FreshSpec;

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const DAY: i64 = 24 * 3600;

fn now() -> i64 {
    parse_utc("2026-10-26T12:00:00Z").unwrap()
}

fn org() -> TestOrg {
    TestOrg::three_admins()
}

fn spec(t: &TestOrg) -> FreshSpec {
    FreshSpec::for_head(t, HEAD, now() - 3600, 7 * DAY)
}

fn assess_with(t: &TestOrg, f: &FreshSpec, signers: &[&str]) -> Assessment {
    let keys: Vec<_> = signers.iter().map(|n| t.key(n)).collect();
    let refs: Vec<&_> = keys.iter().collect();
    let (stmt, sigs) = f.sign(&refs);
    assess(&t.org, &t.store, Some((&stmt, &sigs)), &t.verify().unwrap(), now(), &Policy::default())
}

fn stale(a: &Assessment) -> &'static str {
    match &a.state {
        FreshnessState::Stale { reason } => reason,
        other => panic!("expected stale, got {other:?}"),
    }
}

#[test]
fn a_statement_signed_by_one_active_admin_is_fresh_and_endorsements_count() {
    let t = org();
    let a = assess_with(&t, &spec(&t), &["alice"]);
    assert!(a.state.endorsements_count(), "{a:?}");
    assert!(a.authentic.is_some());
    match a.state {
        FreshnessState::Fresh { age_secs, life_used_pct, .. } => {
            assert_eq!(age_secs, 3600);
            assert!(life_used_pct < 50);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn missing_statement_is_stale_and_counts_no_endorsements() {
    let t = org();
    let a = assess(&t.org, &t.store, None, &t.verify().unwrap(), now(), &Policy::default());
    assert_eq!(stale(&a), "missing");
    assert!(!a.state.endorsements_count());
    assert!(a.state.warns());
}

#[test]
fn a_statement_signed_by_a_non_admin_is_ignored() {
    let t = org();
    // dave is a steward, erin a member, a stranger holds no key in the roster: none of them is an admin.
    for who in ["dave", "erin"] {
        let a = assess_with(&t, &spec(&t), &[who]);
        assert_eq!(stale(&a), "no-admin-signature", "{who}");
        assert!(a.authentic.is_none());
    }
    let mut t2 = org();
    let stranger = t2.who("mallory");
    let (stmt, sigs) = spec(&t2).sign(&[&stranger]);
    let a = assess(&t2.org, &t2.store, Some((&stmt, &sigs)), &t2.verify().unwrap(), now(), &Policy::default());
    assert_eq!(stale(&a), "no-admin-signature");
}

#[test]
fn a_signature_over_other_bytes_counts_for_nothing() {
    let t = org();
    let (stmt, sigs) = spec(&t).sign(&[&t.key("alice")]);
    let mut other = spec(&t);
    other.store_head = "f".repeat(40);
    let (other_stmt, _) = other.sign(&[&t.key("alice")]);
    // The signatures file of the first statement beside the bytes of the second.
    let a = assess(&t.org, &t.store, Some((&other_stmt, &sigs)), &t.verify().unwrap(), now(), &Policy::default());
    assert_eq!(stale(&a), "no-admin-signature");
    // Control: the matching pair is fine.
    assert!(assess(&t.org, &t.store, Some((&stmt, &sigs)), &t.verify().unwrap(), now(), &Policy::default()).state.endorsements_count());
}

#[test]
fn another_store_org_epoch_or_epoch_hash_is_ignored() {
    let t = org();
    let mut f = spec(&t);
    f.store = "other-store".into();
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "wrong-scope");
    let mut f = spec(&t);
    f.org = "other-org".into();
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "wrong-scope");
    let mut f = spec(&t);
    f.epoch_hash = crate::hash::of(b"another epoch");
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "epoch-hash-mismatch");
    let mut f = spec(&t);
    f.epoch = 2;
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "newer-than-chain");
}

#[test]
fn a_statement_for_an_older_epoch_is_stale_whatever_its_expiry() {
    let mut t = org();
    let old = spec(&t);
    t.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&crate::testkit::key(77), "frank")));
    // Epoch 2 is the chain head; the epoch-1 statement is perfectly signed, unexpired, and still stale.
    let a = assess_with(&t, &old, &["alice"]);
    assert_eq!(stale(&a), "older-epoch");
    assert!(a.authentic.is_none(), "an older statement is not a trust base either");
    // The statement for the head epoch is fine.
    assert!(assess_with(&t, &spec(&t), &["alice"]).state.endorsements_count());
}

#[test]
fn a_departed_admin_cannot_sign_for_the_head_epoch() {
    let mut t = org();
    // Epoch 2 removes carol. Her old key signs a statement naming epoch 2: she is not an admin there.
    t.advance(&["alice", "bob"], |t, s| {
        let carol = t.key("carol");
        let m = s.member_mut(&carol);
        m.status = "removed";
        m.removed_at_epoch = Some(2);
    });
    assert_eq!(stale(&assess_with(&t, &spec(&t), &["carol"])), "no-admin-signature");
    assert!(assess_with(&t, &spec(&t), &["alice"]).state.endorsements_count());
}

#[test]
fn expiry_is_judged_by_the_install_clock() {
    let t = org();
    let mut f = spec(&t);
    // Expired one second ago; the boundary itself counts as expired.
    f.issued_at = format_utc(now() - 2 * DAY);
    f.expires_at = format_utc(now() - 1);
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "expired");
    f.expires_at = format_utc(now());
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "expired");
    f.expires_at = format_utc(now() + 1);
    assert!(assess_with(&t, &f, &["alice"]).state.endorsements_count());
    // An expired statement is still authentic (a valid base for a walk), just not current.
    f.expires_at = format_utc(now() - 5);
    assert!(assess_with(&t, &f, &["alice"]).authentic.is_some());
}

fn format_utc(s: i64) -> String {
    crate::time::format_utc(s)
}

#[test]
fn validity_beyond_the_local_cap_is_rejected_not_clamped() {
    let t = org();
    let mut f = spec(&t);
    f.issued_at = format_utc(now() - DAY);
    f.expires_at = format_utc(now() - DAY + 14 * DAY);
    assert!(assess_with(&t, &f, &["alice"]).state.endorsements_count(), "exactly the cap is allowed");
    f.expires_at = format_utc(now() - DAY + 14 * DAY + 1);
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "validity-over-cap");
    // The signer chose 2099; the verifier does not honour it and does not truncate it to something valid.
    f.expires_at = "2099-01-01T00:00:00Z".into();
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "validity-over-cap");
    // A shorter local cap tightens the same statement.
    let strict = Policy { max_validity_secs: 3 * DAY };
    let g = spec(&t);
    let keys = [t.key("alice")];
    let (stmt, sigs) = g.sign(&[&keys[0]]);
    let roster = t.verify().unwrap();
    assert_eq!(stale(&assess(&t.org, &t.store, Some((&stmt, &sigs)), &roster, now(), &strict)), "validity-over-cap");
}

#[test]
fn expiry_beyond_clock_plus_the_cap_is_rejected_even_with_a_short_life() {
    // A signer-chosen short validity far in the future: issued 2098, expires 2098 + 1 day.
    let t = org();
    let mut f = spec(&t);
    f.issued_at = "2098-12-31T00:00:00Z".into();
    f.expires_at = "2099-01-01T00:00:00Z".into();
    let reason = stale(&assess_with(&t, &f, &["alice"]));
    assert!(reason == "issued-in-future", "{reason}");
    // Issued now, valid for the cap, shifted so only `expires` crosses clock + cap: issued 5 minutes ahead (allowed skew).
    let mut f = spec(&t);
    f.issued_at = format_utc(now() + 299);
    f.expires_at = format_utc(now() + 299 + 14 * DAY);
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "expires-too-far");
}

#[test]
fn issued_more_than_five_minutes_ahead_is_rejected() {
    let t = org();
    let mut f = spec(&t);
    f.issued_at = format_utc(now() + 300);
    f.expires_at = format_utc(now() + 300 + DAY);
    assert!(assess_with(&t, &f, &["alice"]).state.endorsements_count(), "exactly five minutes is within the skew");
    f.issued_at = format_utc(now() + 301);
    f.expires_at = format_utc(now() + 301 + DAY);
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "issued-in-future");
}

#[test]
fn a_statement_that_expires_before_it_was_issued_is_rejected() {
    let t = org();
    let mut f = spec(&t);
    f.issued_at = format_utc(now() - 10);
    f.expires_at = format_utc(now() - 20);
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "expires-before-issued");
}

#[test]
fn rewrite_of_needs_a_quorum_of_people_not_keys() {
    let mut t = org();
    let alice2 = t.who("alice2");
    t.advance(&["alice", "bob"], |_, s| s.members.push(MemberSpec::new(&alice2, "alice").admin()));
    let mut f = spec(&t);
    f.rewrite_of = Some("a".repeat(40));
    // 3 admin people (alice with two keys, bob, carol): a majority is 2.
    assert_eq!(stale(&assess_with(&t, &f, &["alice"])), "rewrite-quorum-missing");
    assert_eq!(stale(&assess_with(&t, &f, &["alice", "alice2"])), "rewrite-quorum-missing", "two keys of one person are one");
    let a = assess_with(&t, &f, &["alice", "bob"]);
    assert!(a.state.endorsements_count(), "{a:?}");
    let auth = a.authentic.unwrap();
    assert_eq!((auth.signing_people, auth.needed), (2, 2));
    assert!(assess_with(&t, &f, &["alice2", "carol"]).authentic.is_some());
    // Without `rewrite_of`, one admin is enough.
    assert!(assess_with(&t, &spec(&t), &["alice2"]).authentic.is_some());
}

#[test]
fn the_statement_and_signature_files_are_strict() {
    let t = org();
    let good = spec(&t).to_json();
    let parse = |s: &str| Statement::parse(s.as_bytes());
    assert!(parse(&good).is_ok());
    for bad in [
        good.replace("\"epoch\": 1", "\"epoch\": 0"),
        good.replace("\"epoch\": 1", "\"epoch\": \"1\""),
        good.replacen("{", "{\n  \"extra\": 1,", 1),
        good.replace("\"store_head\": \"0123456789abcdef0123456789abcdef01234567\"", "\"store_head\": \"main\""),
        good.replace("Z\"", "+00:00\""),
        good.replacen("\"org\"", "\"org\": \"a\", \"org\"", 1),
        good.replace("\n}", ",\n  \"rewrite_of\": null\n}"),
        good.replace("\n}", ",\n  \"rewrite_of\": \"nothex\"\n}"),
        String::new(),
        "[]".to_string(),
    ] {
        assert!(parse(&bad).is_err(), "{bad}");
    }
    assert_eq!(Statement::parse(&vec![b' '; MAX_STATEMENT_BYTES + 1]), Err("oversize"));
    let issued = spec(&t).issued_at;
    assert!(parse(&good.replace(&issued, "2026-02-31T11:00:00Z")).is_err(), "an impossible day");
    // Garbage in the signatures file is "ignored", never a panic or a pass.
    let (stmt, sigs) = spec(&t).sign(&[&t.key("alice")]);
    let roster = t.verify().unwrap();
    for bad in [&b"{}"[..], b"{\"signatures\": [{\"kid\": \"x\", \"sig\": \"y\"}]}", b"not json", b"{\"signatures\": [], \"x\": 1}"] {
        let a = assess(&t.org, &t.store, Some((&stmt, bad)), &roster, now(), &Policy::default());
        assert!(!a.state.endorsements_count(), "{}", String::from_utf8_lossy(bad));
    }
    assert!(assess(&t.org, &t.store, Some((&stmt, &sigs)), &roster, now(), &Policy::default()).state.endorsements_count());
}

#[test]
fn half_of_the_life_used_warns() {
    let t = org();
    let mut f = spec(&t);
    f.issued_at = format_utc(now() - 4 * DAY);
    f.expires_at = format_utc(now() + 4 * DAY);
    let a = assess_with(&t, &f, &["alice"]);
    assert!(a.state.endorsements_count() && a.state.warns(), "{a:?}");
    let b = assess_with(&t, &spec(&t), &["alice"]);
    assert!(!b.state.warns());
}
