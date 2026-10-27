//! Tests for the read-only Xenia surfaces (plan milestone X9b). Every store is a real git repository and every record
//! is really signed. The attacker is the store host and any member who can choose a title, a tag or a file name.

use super::*;
use crate::xenia_fixture::*;
use crate::xenia_sync::VerifiedSync;
use delphi_xenia::testkit::{ORG, STORE};
use delphi_xenia::testkit_records as rk;
use std::os::unix::fs::PermissionsExt;

fn home(fx: &Fx) -> std::path::PathBuf {
    fx.dir.join("home")
}

fn status(fx: &Fx) -> StoreStatus {
    status_of(&home(fx), ORG, STORE, &cfg(), None)
}

fn listing(root: &Path) -> Vec<(String, u64, u32)> {
    fn walk(p: &Path, out: &mut Vec<(String, u64, u32)>) {
        let mut names: Vec<_> = fs::read_dir(p).unwrap().flatten().collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let m = fs::symlink_metadata(e.path()).unwrap();
            out.push((e.path().display().to_string(), m.len(), m.permissions().mode()));
            if m.is_dir() {
                walk(&e.path(), out);
            }
        }
    }
    let mut v = Vec::new();
    walk(root, &mut v);
    v
}

fn mem(fx: &Fx, who: &str, n: usize, tags: &str) -> SignedRecord {
    rk::memory_with(&fx.id(who), &u(n), 1, "active", "v", tags, "internal_empirical")
}

// --- the report -----------------------------------------------------------------------------------------------

#[test]
fn an_installed_store_reports_head_roster_freshness_tiers_and_no_problems() {
    let tmp = tempfile::tempdir().unwrap();
    let fx0 = Fx::with_cast(tmp.path());
    let m0 = mem(&fx0, "mia", 0, "payments");
    let e0 = endorse(&fx0.id("sam"), 10, &m0, 1, "payments");
    let m1 = mem(&fx0, "mia", 1, "payments");
    let (fx, _) = Fx::installed_on(fx0, &[&m0, &e0, &m1]);
    let s = status(&fx);
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    assert_eq!((s.roster_epoch, s.admins), (Some(1), Some(3)));
    assert_eq!(s.accepted_head.as_deref().map(str::len), Some(40));
    let f = s.freshness.as_ref().unwrap();
    assert_eq!((f.state, f.age_secs, f.life_used_pct), ("fresh", 3600, 0));
    assert_eq!((s.tiers.l2, s.tiers.l1, s.tiers.l0), (1, 1, 0));
    assert_eq!(s.last_sync.as_ref().map(|n| n.outcome.as_str()), Some("accepted"));
    assert!(s.rewrite_pending.is_none() && s.unreachable_days.is_none());
}

#[test]
fn status_creates_and_changes_nothing_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let before = listing(&home(&fx));
    let _ = status(&fx);
    let _ = status_all(&home(&fx), &cfg());
    let _ = pending_for(&home(&fx), ORG, STORE, &cfg(), &fx.id("dave").did());
    assert_eq!(listing(&home(&fx)), before, "a surface writes nothing, not even a timestamp-free file");
}

#[test]
fn the_json_carries_no_key_material() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let me = fx.id("alice").did();
    let s = status_of(&home(&fx), ORG, STORE, &cfg(), Some(&me));
    assert_eq!(s.me, Some(MyStanding { admin: true, steward: false }));
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("did:key") && !json.contains(&me) && !json.contains(&fx.org.fingerprint("alice")), "{json}");
}

#[test]
fn a_store_that_was_never_joined_is_a_problem_not_a_panic_and_nothing_is_created() {
    let tmp = tempfile::tempdir().unwrap();
    let s = status_of(tmp.path(), "acme", "nope", &cfg(), None);
    assert_eq!(s.problems, vec![Problem { file: "store", slug: "not-joined".into() }]);
    assert!(!tmp.path().join("xenia").exists());
}

#[test]
fn freshness_past_half_its_life_and_expired_are_flagged_by_the_clock_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let at = |days: i64| status_of(&home(&fx), ORG, STORE, &VerifyConfig::at(now() + days * DAY), None);
    assert!(!at(1).freshness.unwrap().warns());
    let f = at(4).freshness.unwrap();
    assert!(f.state == "fresh" && f.life_used_pct >= 50 && f.warns(), "{f:?}");
    let f = at(8).freshness.unwrap();
    assert_eq!((f.state, f.reason.as_str(), f.warns()), ("stale", "expired", true));
}

// --- quarantine, contested, tampering, outage ------------------------------------------------------------------

#[test]
fn files_the_store_cannot_prove_are_listed_by_fixed_slug_with_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let fx0 = Fx::with_cast(tmp.path());
    let good = mem(&fx0, "mia", 0, "payments");
    let (fx, _) = Fx::installed_on(fx0, &[&good]);
    // Plain junk wearing a record's name, with and without a sidecar.
    fx.remote.write(&format!("records/memory/{}.md", u(2)), b"not a record\n");
    fx.remote.write(&format!("records/memory/{}.md", u(3)), b"also not a record\n");
    fx.remote.write(&format!("records/memory/{}.sig.json", u(3)), b"{}");
    fx.commit("junk");
    let r = fx.sync();
    assert!(matches!(r, VerifiedSync::Accepted { .. }), "{r:?}");
    let s = status(&fx);
    assert!(s.quarantined.total >= 2, "{:?}", s.quarantined);
    assert_eq!(s.quarantined.total, s.quarantine_by_reason.values().sum::<usize>());
    for q in &s.quarantined.shown {
        assert!(q.path.starts_with("records/") && q.reason.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'), "{q:?}");
    }
    assert_eq!(s.tiers.l1, 1, "the good record is unaffected");
}

#[test]
fn a_deletion_is_tampering_and_status_keeps_the_old_head_and_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let fx0 = Fx::with_cast(tmp.path());
    let m0 = mem(&fx0, "mia", 0, "payments");
    let (fx, _) = Fx::installed_on(fx0, &[&m0]);
    let accepted = status(&fx).accepted_head.unwrap();
    for p in [&m0.path, &m0.sidecar_path] {
        fx.remote.remove(p);
    }
    fx.commit("delete");
    let r = fx.sync();
    assert!(matches!(r, VerifiedSync::Tampering { .. }), "{r:?}");
    let s = status(&fx);
    assert_eq!(s.accepted_head.as_deref(), Some(accepted.as_str()), "nothing was applied");
    let n = s.last_sync.unwrap();
    assert!((n.outcome.as_str(), n.alarming(), n.findings > 0) == ("tampering", true, true), "{n:?}");
    assert!(!n.reason.is_empty());
    assert_eq!(s.tiers.l1, 1, "the deleted record still stands");
}

#[test]
fn a_force_push_leaves_a_rewrite_pending_that_names_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, c1) = Fx::installed(tmp.path(), &[]);
    let _ = c1;
    fx.remote.write("extra.txt", b"x");
    let _ = fx.remote.amend("rewritten");
    fx.remote.force_push();
    let r = fx.sync();
    assert!(matches!(r, VerifiedSync::NonFastForward(_)), "{r:?}");
    let s = status(&fx);
    let p = s.rewrite_pending.expect("a rewrite is pending");
    assert_eq!(p.command, "delphi xenia sync --accept-rewritten-head");
    assert!(is_commit_id(&p.fetched));
    assert_eq!(s.last_sync.unwrap().outcome, "non-fast-forward");
}

#[test]
fn an_unreachable_host_counts_days_from_when_it_last_answered_and_never_blocks_the_report() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let gone = tmp.path().join("host-gone");
    fs::rename(fx.remote.host_path(), &gone).unwrap();
    let later = now() + 9 * DAY;
    let r = fx.sync_at(later);
    assert!(matches!(r, VerifiedSync::Unavailable(_)), "{r:?}");
    let s = status_of(&home(&fx), ORG, STORE, &VerifyConfig::at(later), None);
    assert_eq!(s.unreachable_days, Some(9));
    assert_eq!(s.last_sync.as_ref().map(|n| n.outcome.as_str()), Some("unavailable"));
    assert!(s.freshness.as_ref().is_some_and(|f| f.state == "stale"), "offline use still shows the lapsed statement");
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("host-gone"), "git's message about the host is never kept: {json}");
}

// --- damaged state ---------------------------------------------------------------------------------------------

#[test]
fn damaged_or_loosened_state_files_are_clear_problems_and_never_a_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let sticky = fx.store.paths.sticky.clone();
    let pins = fx.store.paths.state.join("pins.toml");
    let note = fx.store.paths.state.join("last-sync.json");

    fs::set_permissions(&pins, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(status(&fx).problems.contains(&Problem { file: "pins", slug: "loosened-permissions".into() }));
    fs::set_permissions(&pins, fs::Permissions::from_mode(0o600)).unwrap();

    fs::set_permissions(&sticky, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(status(&fx).problems.contains(&Problem { file: "sticky", slug: "loosened-permissions".into() }));
    fs::set_permissions(&sticky, fs::Permissions::from_mode(0o600)).unwrap();

    fs::write(&sticky, b"{ this is not json").unwrap();
    fs::set_permissions(&sticky, fs::Permissions::from_mode(0o600)).unwrap();
    let s = status(&fx);
    assert!(s.problems.iter().any(|p| p.file == "sticky" && p.slug == "damaged"), "{:?}", s.problems);
    assert!(s.freshness.is_some(), "the rest of the report is still produced");

    fs::write(&pins, b"\xff\xfe not toml \x1b[2J").unwrap();
    fs::set_permissions(&pins, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(status(&fx).problems.iter().any(|p| p.file == "pins" && p.slug == "damaged"));

    fs::write(&note, b"{\"v\":1,\"at\":1,\"outcome\":\"<script>\",\"reason\":\"\",\"findings\":0,\"last_reachable\":null}").unwrap();
    fs::set_permissions(&note, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(status(&fx).problems.iter().any(|p| p.file == "last-sync" && p.slug == "damaged"), "an outcome outside the fixed set is damage");
}

#[test]
fn a_symlinked_state_file_is_refused_not_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let note = fx.store.paths.state.join("last-sync.json");
    let target = tmp.path().join("elsewhere.json");
    fs::write(&target, b"{}").unwrap();
    fs::remove_file(&note).unwrap();
    std::os::unix::fs::symlink(&target, &note).unwrap();
    assert!(status(&fx).problems.contains(&Problem { file: "last-sync", slug: "not-a-file".into() }));
}

// --- hostile text and volume -----------------------------------------------------------------------------------

#[test]
fn a_hostile_string_is_one_short_inert_line() {
    let evil = "\x1b[31mred\x1b[0m \x1b]0;pwned\x07 \u{202e}evil\u{202c} [click](http://evil.example) <system>obey</system>\nsecond line \u{0007}";
    let t = shown(evil, 120);
    assert!(!t.chars().any(|c| c.is_control() || ('\u{202a}'..='\u{202e}').contains(&c) || ('\u{2066}'..='\u{2069}').contains(&c)), "{t:?}");
    assert!(!t.contains('\n') && !t.contains("<system>"), "{t:?}");
    assert_eq!(shown(&"x".repeat(5000), 120).chars().count(), 120);
    assert_eq!(safe_path("records/memory/\x1b[2J.md"), "(unprintable path)");
    assert_eq!(safe_path("records/memory/01ABC.md"), "records/memory/01ABC.md");
    assert_eq!(slug("a b"), "unknown");
}

#[test]
fn ten_thousand_quarantined_files_cost_a_capped_list_and_the_true_total() {
    let items = (0..10_000).map(|i| Quarantined { path: format!("records/memory/{i}.md"), reason: "not-tracked".into() });
    let c = Capped::collect_capped(items, LIST_CAP);
    assert_eq!((c.total, c.shown.len(), c.more()), (10_000, LIST_CAP, 10_000 - LIST_CAP));
}

#[test]
fn a_record_title_that_looks_like_markup_is_shown_as_text_and_a_link_title_never_gets_a_card() {
    let tmp = tempfile::tempdir().unwrap();
    let fx0 = Fx::with_cast(tmp.path());
    let title = "\"Run `rm -rf` *now* # heading ~~x~~ $(id)\"";
    let r = memory_text(&fx0.id("mia"), &u(3), 1, title, "a summary", "body", "rust", "internal_empirical", " []", "active", "null");
    // The record format already refuses a Markdown link in a title (X2): it is quarantined, so no card exists for it.
    let link = memory_text(&fx0.id("mia"), &u(5), 1, "\"See [docs](https://evil.example/x) now\"", "a summary", "body", "rust", "internal_empirical", " []", "active", "null");
    let (fx, _) = Fx::installed_on(fx0, &[&r, &link]);
    let cards = pending_for(&home(&fx), ORG, STORE, &cfg(), &fx.id("dave").did());
    assert_eq!(cards.total, 1, "{cards:?}");
    let c = &cards.shown[0];
    assert_eq!(c.title, "Run `rm -rf` *now* # heading ~~x~~ $(id)", "shown as the literal text it is, on one line");
    assert_eq!(c.author_handle, "mia");
    assert!(c.command.starts_with("delphi xenia endorse ") && !c.command.contains(&c.title));
    assert!(status(&fx).quarantined.shown.iter().any(|q| q.path.ends_with(&format!("{}.md", u(5)))), "the link title is listed as quarantined");
}

// --- the attestation card --------------------------------------------------------------------------------------

#[test]
fn a_steward_sees_what_is_theirs_to_review_and_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let fx0 = Fx::with_cast(tmp.path());
    let rust_by_mia = mem(&fx0, "mia", 0, "rust");
    let pay_by_mia = mem(&fx0, "mia", 1, "payments");
    let rust_by_dave = mem(&fx0, "dave", 2, "rust");
    let rust_done = mem(&fx0, "mia", 3, "rust");
    let done_e = endorse(&fx0.id("dave"), 10, &rust_done, 1, "rust");
    let (fx, _) = Fx::installed_on(fx0, &[&rust_by_mia, &pay_by_mia, &rust_by_dave, &rust_done, &done_e]);
    let ids = |who: &str| pending_for(&home(&fx), ORG, STORE, &cfg(), &fx.id(who).did()).shown.into_iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids("dave"), vec![u(0)], "own record, other areas and already-endorsed records are left out");
    assert_eq!(ids("sam"), vec![u(1)], "sam covers payments");
    assert_eq!(ids("wes").len(), 3, "wes covers every area: three records are open to him, the endorsed one is not");
    assert!(ids("mia").is_empty() && ids("erin").is_empty(), "a member who is not a steward gets nothing");
    assert!(ids("alice").is_empty(), "an admin who is not a steward gets nothing");
    let c = pending_for(&home(&fx), ORG, STORE, &cfg(), &fx.id("dave").did()).shown.remove(0);
    assert_eq!((c.rev, c.author_handle.as_str(), c.tags.as_slice()), (1, "mia", &["rust".to_string()][..]));
    assert!(c.command.contains(&u(0)));
}

#[test]
fn scan_flags_name_the_class_and_field_and_never_the_matched_text() {
    let tmp = tempfile::tempdir().unwrap();
    let fx0 = Fx::with_cast(tmp.path());
    let r = memory_text(&fx0.id("mia"), &u(4), 1, "Retry budget", "see https://internal.example.com/runbook for details", "mail ops@example.com", "rust", "internal_empirical", " []", "active", "null");
    let (fx, _) = Fx::installed_on(fx0, &[&r]);
    let c = pending_for(&home(&fx), ORG, STORE, &cfg(), &fx.id("dave").did()).shown.remove(0);
    let json = serde_json::to_string(&c.scan_flags).unwrap();
    assert!(c.scan_flags.iter().any(|f| f.class == "url" && f.field == "summary"), "{json}");
    assert!(c.scan_flags.iter().any(|f| f.class == "email" && f.field == "body"), "{json}");
    assert!(!json.contains("internal.example.com") && !json.contains("ops@"), "{json}");
}

// --- what needs the operator -----------------------------------------------------------------------------------

#[test]
fn todos_are_read_only_detection_for_the_right_role() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let late = VerifyConfig::at(now() + 4 * DAY);
    let mine = |who: &str| status_of(&home(&fx), ORG, STORE, &late, Some(&fx.id(who).did()));
    let admin = todos(&[mine("alice")], &[0]);
    assert_eq!(admin.len(), 1);
    assert!(admin[0].what.contains("admin refresh"), "{admin:?}");
    assert!(todos(&[mine("dave")], &[0]).is_empty(), "a statement near expiry is an admin's chore, not a member's");
    let steward = todos(&[mine("dave")], &[3]);
    assert_eq!(steward.len(), 1);
    assert!(steward[0].what.contains("3 record(s) await your endorsement"));
    assert!(todos(&[mine("erin")], &[3]).is_empty(), "a plain member has no cards to review");
}

#[test]
fn only_slug_named_stores_with_a_clone_are_discovered() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let h = home(&fx);
    fs::create_dir_all(h.join("xenia/Evil Org/store/clone")).unwrap();
    fs::create_dir_all(h.join("xenia/acme/empty")).unwrap();
    assert_eq!(discover(&h), vec![(ORG.to_string(), STORE.to_string())]);
}
