//! Tests for `xenia_store`. The attacker is the store host: every hostile case is a real git repository built
//! with the test kit and fetched through the real code path. Where a test claims "nothing ran", a control
//! run first proves the planted program does run when git is used without the store's hardening, so the
//! assertion can fail.

use super::*;
use crate::xenia_testkit::{TestRemote, GOOD_GITATTRIBUTES};
use std::os::unix::fs::symlink;

const A: &str = "records/memory/01JBX3Q8T2M9Z4K7WYH5N6D0RA.md";
const B: &str = "records/memory/01JBX3Q8T2M9Z4K7WYH5N6D0RB.md";
const A_SIG: &str = "records/memory/01JBX3Q8T2M9Z4K7WYH5N6D0RA.sig.json";
const EPOCH1: &str = "roster/epochs/1.json";

struct Fx {
    tmp: tempfile::TempDir,
    home: PathBuf,
    remote: TestRemote,
    store: Store,
}

/// A remote whose working copy already has a correct `.gitattributes`, and an install that opened a store on it.
fn fx() -> Fx {
    fx_with(Caps::default())
}

fn fx_with(caps: Caps) -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let remote = TestRemote::new(&tmp.path().join("host"));
    remote.write(".gitattributes", GOOD_GITATTRIBUTES.as_bytes());
    remote.write("org.toml", b"org = \"acme\"\n");
    let store = Store::open(&home, "acme", "main", &remote.url(), true).unwrap().with_caps(caps);
    Fx { tmp, home, remote, store }
}

fn rec(rev: u64, body: &str) -> Vec<u8> {
    format!("rev: {rev}\n{body}\n").into_bytes()
}

fn rev_of(bytes: &[u8]) -> std::result::Result<u64, &'static str> {
    let first = std::str::from_utf8(bytes).map_err(|_| "not-utf8")?.lines().next().unwrap_or("");
    first.strip_prefix("rev: ").and_then(|n| n.parse().ok()).ok_or("no-rev")
}

fn sync(f: &Fx) -> SyncReport {
    f.store.sync(&mut |p, b| classify_by_layout(p, b, &rev_of)).unwrap()
}

fn head_of(r: &SyncReport) -> &str {
    match r {
        SyncReport::Accepted { head, .. } | SyncReport::UpToDate { head } => head,
        other => panic!("expected an accepted sync, got {other:?}"),
    }
}

fn sticky_bytes(f: &Fx) -> Vec<u8> {
    fs::read(&f.store.paths.sticky).unwrap()
}

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// Incompressible bytes, so a "huge" blob stays huge in the pack.
fn noise(n: usize) -> Vec<u8> {
    let mut x = 0x9E3779B97F4A7C15u64;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

/// An executable that records that it ran.
fn trap(dir: &Path, name: &str, sentinel: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\ntouch '{}'\nexit 0\n", sentinel.display())).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Git as an unhardened tool would run it against the clone: reads the clone's own configuration and hooks.
fn plain_git(clone: &Path, home: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", home)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .arg("--git-dir")
        .arg(clone)
        .args(args)
        .output()
        .unwrap()
}

fn refused(r: SyncReport) -> Refused {
    match r {
        SyncReport::Refused(x) => x,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn nothing_accepted(f: &Fx) {
    assert_eq!(f.store.accepted_head().unwrap(), None);
    assert!(!f.store.paths.sticky.exists(), "a refused first sync leaves no state");
    assert_ne!(f.store.exit_code(&["rev-parse", "--verify", "--quiet", REF_FETCHED]).unwrap(), 0, "the staging ref is dropped");
}

// --- location and privacy (011 test 24) -----------------------------------------------------------------

#[test]
fn the_clone_is_a_bare_private_repository_under_xenia_and_outside_every_zone() {
    let f = fx();
    let p = &f.store.paths;
    assert!(p.clone.starts_with(f.home.join("xenia").join("acme").join("main")));
    assert!(p.is_outside_zones(&f.home, &[f.home.join("team"), f.home.join("workspaces")]));
    for d in [&f.home.join("xenia"), &p.root, &p.clone, &p.state, &p.empty_hooks, &p.empty_home] {
        assert_eq!(mode(d), 0o700, "{}", d.display());
    }
    assert!(p.clone.join("HEAD").is_file() && !p.clone.join(".git").exists(), "a bare repository: nothing to check out into");
    assert!(!f.home.join("team").exists());
    f.remote.commit_files(&[(A, &rec(1, "x"))], "a");
    sync(&f);
    assert!(!p.clone.join("records").exists(), "records are read from objects, never checked out");
}

#[test]
fn a_symlinked_store_directory_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join("xenia")).unwrap();
    fs::create_dir_all(tmp.path().join("elsewhere")).unwrap();
    symlink(tmp.path().join("elsewhere"), home.join("xenia").join("acme")).unwrap();
    assert!(Store::open(&home, "acme", "main", "https://example.com/x.git", false).is_err());
    assert!(fs::read_dir(tmp.path().join("elsewhere")).unwrap().next().is_none(), "nothing was created through the link");
}

#[test]
fn a_planted_symlink_in_place_of_the_clone_is_refused() {
    let f = fx();
    let target = f.tmp.path().join("attacker-repo");
    fs::create_dir_all(&target).unwrap();
    fs::remove_dir_all(&f.store.paths.clone).unwrap();
    symlink(&target, &f.store.paths.clone).unwrap();
    assert!(Store::open(&f.home, "acme", "main", &f.remote.url(), true).is_err());
}

// --- sticky state (011 tests 27 and 28, 012 test 21) ----------------------------------------------------

#[test]
fn syncing_twice_changes_nothing() {
    let f = fx();
    let head = f.remote.commit_files(&[(A, &rec(1, "one")), (A_SIG, b"{}"), (EPOCH1, b"{}")], "first");
    let r1 = sync(&f);
    assert!(matches!(&r1, SyncReport::Accepted { from: None, added: 3, .. }), "{r1:?}");
    assert_eq!(head_of(&r1), head);
    let (bytes, stamp) = (sticky_bytes(&f), fs::metadata(&f.store.paths.sticky).unwrap().modified().unwrap());
    assert_eq!(sync(&f), SyncReport::UpToDate { head });
    assert_eq!(sticky_bytes(&f), bytes);
    assert_eq!(fs::metadata(&f.store.paths.sticky).unwrap().modified().unwrap(), stamp, "the file was not even rewritten");
    assert_eq!(mode(&f.store.paths.sticky), 0o600);
}

#[test]
fn an_unchanged_record_in_a_later_commit_is_idempotent() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(1, "one"))], "a");
    sync(&f);
    let entry = f.store.load_sticky().unwrap().paths[A].clone();
    f.remote.commit_files(&[(B, &rec(1, "other"))], "b");
    assert!(matches!(sync(&f), SyncReport::Accepted { added: 1, unchanged: 1, superseded: 0, .. }));
    assert_eq!(f.store.load_sticky().unwrap().paths[A], entry);
}

#[test]
fn a_valid_higher_rev_supersedes_and_the_new_head_is_accepted() {
    let f = fx();
    let h1 = f.remote.commit_files(&[(A, &rec(1, "one"))], "a1");
    sync(&f);
    let h2 = f.remote.commit_files(&[(A, &rec(2, "two")), (B, &rec(1, "other"))], "a2");
    match sync(&f) {
        SyncReport::Accepted { head, from, added, superseded, snapshot, .. } => {
            assert_eq!((head, from, added, superseded), (h2.clone(), Some(h1), 1, 1));
            assert_eq!(snapshot.files[A], rec(2, "two"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(f.store.load_sticky().unwrap().paths[A].tracking, Tracking::Rev(2));
    assert_eq!(f.store.accepted_head().unwrap(), Some(h2));
}

#[test]
fn a_lower_rev_is_a_rollback_nothing_is_applied_and_the_report_repeats() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(1, "one"))], "a1");
    let h2 = f.remote.commit_files(&[(A, &rec(2, "two"))], "a2");
    sync(&f);
    let before = sticky_bytes(&f);
    f.remote.commit_files(&[(A, &rec(1, "one"))], "revert to rev 1");
    for _ in 0..2 {
        match sync(&f) {
            SyncReport::Tampering { refusals, .. } => assert_eq!(refusals, vec![Refusal::Rollback { path: A.into(), stored: 2, seen: 1 }]),
            other => panic!("{other:?}"),
        }
        assert_eq!(sticky_bytes(&f), before, "the stored entry keeps its effect");
        assert_eq!(f.store.accepted_head().unwrap(), Some(h2.clone()));
    }
}

#[test]
fn the_same_rev_with_different_bytes_is_a_swap() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(3, "honest"))], "a");
    sync(&f);
    f.remote.commit_files(&[(A, &rec(3, "swapped"))], "swap");
    match sync(&f) {
        SyncReport::Tampering { refusals, .. } => assert_eq!(refusals, vec![Refusal::Swapped { path: A.into() }]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn deleted_files_are_reported_and_the_last_seen_versions_keep_their_effect() {
    let f = fx();
    let h1 = f.remote.commit_files(&[(A, &rec(1, "one")), (A_SIG, b"{}"), (EPOCH1, b"{}"), (B, &rec(1, "keep"))], "all");
    sync(&f);
    let before = sticky_bytes(&f);
    for gone in [A, A_SIG, EPOCH1] {
        f.remote.remove(gone);
    }
    f.remote.commit("delete three signed files");
    match sync(&f) {
        SyncReport::Tampering { mut refusals, .. } => {
            refusals.sort_by(|a, b| a.path().cmp(b.path()));
            assert_eq!(refusals, vec![Refusal::Deleted { path: A.into() }, Refusal::Deleted { path: A_SIG.into() }, Refusal::Deleted { path: EPOCH1.into() }]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(sticky_bytes(&f), before);
    assert_eq!(f.store.accepted_head().unwrap(), Some(h1.clone()));
    assert_eq!(f.store.snapshot_of(&h1).unwrap().files[A], rec(1, "one"), "the accepted snapshot is still readable");
}

#[test]
fn an_immutable_roster_epoch_replaced_under_the_same_number_is_equivocation() {
    let f = fx();
    f.remote.commit_files(&[(EPOCH1, b"{\"epoch\":1}")], "epoch 1");
    sync(&f);
    f.remote.commit_files(&[(EPOCH1, b"{\"epoch\":1,\"forked\":true}")], "fork");
    assert!(matches!(sync(&f), SyncReport::Tampering { refusals, .. } if refusals == vec![Refusal::Swapped { path: EPOCH1.into() }]));
}

#[test]
fn a_file_that_fails_verification_never_raises_a_stored_rev() {
    let f = fx();
    let classify = |p: &str, b: &[u8]| {
        if b.starts_with(b"FORGED") { Err("bad-signature") } else { classify_by_layout(p, b, &rev_of) }
    };
    f.remote.commit_files(&[(B, b"FORGED\nrev: 99999\n")], "hostile file");
    match f.store.sync(&mut { classify }).unwrap() {
        SyncReport::Accepted { quarantined, added, .. } => {
            assert_eq!(quarantined, vec![(B.to_string(), "bad-signature")]);
            assert_eq!(added, 0);
        }
        other => panic!("{other:?}"),
    }
    assert!(!f.store.load_sticky().unwrap().paths.contains_key(B));
    f.remote.commit_files(&[(B, &rec(1, "legit"))], "the real record");
    assert!(matches!(f.store.sync(&mut { classify }).unwrap(), SyncReport::Accepted { added: 1, .. }));
}

#[test]
fn a_stored_record_replaced_by_a_file_that_no_longer_verifies_is_refused() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(1, "one"))], "a");
    sync(&f);
    f.remote.commit_files(&[(A, b"FORGED\nrev: 2\n")], "forged overwrite");
    let mut classify = |p: &str, b: &[u8]| if b.starts_with(b"FORGED") { Err("bad-signature") } else { classify_by_layout(p, b, &rev_of) };
    match f.store.sync(&mut classify).unwrap() {
        SyncReport::Tampering { refusals, .. } => assert_eq!(refusals, vec![Refusal::Invalidated { path: A.into() }]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_damaged_or_exposed_sticky_file_stops_the_sync_instead_of_resetting_it() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(1, "one"))], "a");
    sync(&f);
    let good = sticky_bytes(&f);
    let run = || f.store.sync(&mut |p, b| classify_by_layout(p, b, &rev_of));
    fs::write(&f.store.paths.sticky, b"not json").unwrap();
    fs::set_permissions(&f.store.paths.sticky, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(run().is_err(), "damage is an error, not an empty state");
    fs::write(&f.store.paths.sticky, &good).unwrap();
    fs::set_permissions(&f.store.paths.sticky, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(run().is_err(), "a group- or world-readable state file is refused");
    fs::remove_file(&f.store.paths.sticky).unwrap();
    let elsewhere = f.tmp.path().join("other-sticky.json");
    fs::write(&elsewhere, &good).unwrap();
    symlink(&elsewhere, &f.store.paths.sticky).unwrap();
    assert!(run().is_err(), "a symlink is never followed");
}

#[test]
fn state_lives_only_under_the_store_directory_and_the_team_registry_cannot_touch_it() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(1, "one"))], "a");
    sync(&f);
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(p).unwrap().flatten() {
            let path = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&f.home, &mut files);
    assert!(!files.is_empty() && files.iter().all(|p| p.starts_with(f.home.join("xenia").join("acme").join("main"))), "{files:?}");
    assert!(f.store.paths.sticky.starts_with(&f.store.paths.state) && !f.store.paths.sticky.starts_with(&f.store.paths.clone));
    let before = sticky_bytes(&f);
    fs::create_dir_all(f.home.join("team")).unwrap();
    fs::write(f.home.join("team").join("stores.toml"), "[[store]]\norg = \"evil\"\nurl = \"https://evil.example/x.git\"\n").unwrap();
    assert!(matches!(sync(&f), SyncReport::UpToDate { .. }));
    assert_eq!(sticky_bytes(&f), before);
}

// --- fast-forward only ----------------------------------------------------------------------------------

#[test]
fn a_rewritten_history_is_detected_refused_and_never_applied() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(1, "one"))], "a");
    let h2 = f.remote.commit_files(&[(B, &rec(1, "two"))], "b");
    sync(&f);
    let before = sticky_bytes(&f);
    let rewritten = f.remote.amend("b, rewritten");
    assert_ne!(rewritten, h2);
    for _ in 0..2 {
        assert_eq!(sync(&f), SyncReport::NonFastForward(NonFastForward { accepted: h2.clone(), fetched: rewritten.clone(), kind: Divergence::Diverged }));
        assert_eq!(sticky_bytes(&f), before);
        assert_eq!(f.store.accepted_head().unwrap(), Some(h2.clone()));
    }
    assert_eq!(f.store.exit_code(&["rev-parse", "--verify", "--quiet", REF_FETCHED]).unwrap(), 0, "the staging ref stays for the recovery path");
    // A rewrite followed by honest commits on top is still not a descendant of what was accepted.
    f.remote.commit_files(&[(B, &rec(2, "later"))], "on top of the rewrite");
    assert!(matches!(sync(&f), SyncReport::NonFastForward(n) if n.kind == Divergence::Diverged));
}

#[test]
fn a_host_that_goes_back_in_time_is_reported_as_behind() {
    let f = fx();
    let h1 = f.remote.commit_files(&[(A, &rec(1, "one"))], "a");
    let h2 = f.remote.commit_files(&[(A, &rec(2, "two"))], "a2");
    sync(&f);
    f.remote.reset_hard(&h1);
    f.remote.force_push();
    assert_eq!(sync(&f), SyncReport::NonFastForward(NonFastForward { accepted: h2.clone(), fetched: h1, kind: Divergence::Behind }));
    assert_eq!(f.store.accepted_head().unwrap(), Some(h2));
}

#[test]
fn an_unreachable_host_is_unavailable_and_changes_nothing() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(1, "one"))], "a");
    sync(&f);
    let before = sticky_bytes(&f);
    let gone = Store::open(&f.home, "acme", "main", "/nonexistent/path/store.git", true).unwrap();
    assert!(matches!(gone.sync(&mut |p, b| classify_by_layout(p, b, &rev_of)).unwrap(), SyncReport::Unavailable(_)));
    assert_eq!(sticky_bytes(&f), before);
    assert_eq!(gone.exit_code(&["rev-parse", "--verify", "--quiet", REF_FETCHED]).unwrap(), 1, "no stale staging ref survives");
}

// --- a hostile store (R2) -------------------------------------------------------------------------------

#[test]
fn a_symlink_in_the_tree_is_refused() {
    let f = fx();
    f.remote.symlink("records/memory/link", "/etc/passwd");
    f.remote.commit("link");
    let r = refused(sync(&f));
    assert!(matches!(&r, Refused::Tree(h) if h.len() == 1 && h[0].reason == "symlink" && h[0].path == "records/memory/link"), "{r:?}");
    nothing_accepted(&f);
}

#[test]
fn a_submodule_and_a_gitmodules_file_are_refused() {
    let f = fx();
    f.remote.gitlink("vendor/sub", &"1".repeat(40));
    f.remote.write(".gitmodules", b"[submodule \"x\"]\n\tpath = vendor/sub\n\turl = https://evil.example/x.git\n");
    f.remote.commit("submodule");
    let r = refused(sync(&f));
    let Refused::Tree(h) = &r else { panic!("{r:?}") };
    let mut reasons: Vec<_> = h.iter().map(|x| x.reason).collect();
    reasons.sort();
    assert_eq!(reasons, vec!["git-metadata-name", "submodule"]);
    nothing_accepted(&f);
}

#[test]
fn an_executable_file_is_refused() {
    let f = fx();
    f.remote.write("indexes/run.sh", b"#!/bin/sh\n");
    f.remote.make_executable("indexes/run.sh");
    f.remote.commit("exec");
    assert!(matches!(refused(sync(&f)), Refused::Tree(h) if h[0].reason == "executable-file"));
}

#[test]
fn a_gitattributes_with_a_filter_driver_is_refused() {
    let f = fx();
    f.remote.write(".gitattributes", b"* -text\n*.md filter=evil\n");
    f.remote.commit("filter");
    assert_eq!(refused(sync(&f)), Refused::Gitattributes(AttrsRefusal::ForbiddenAttribute));
    nothing_accepted(&f);
}

#[test]
fn a_store_without_gitattributes_is_refused_with_a_specific_reason() {
    let f = fx();
    f.remote.remove(".gitattributes");
    f.remote.commit_files(&[(A, &rec(1, "one"))], "no attributes");
    let r = refused(sync(&f));
    assert_eq!(r, Refused::Gitattributes(AttrsRefusal::Missing));
    assert_eq!(r.reason(), "gitattributes-missing");
    nothing_accepted(&f);
}

#[test]
fn a_gitattributes_that_converts_line_endings_is_refused() {
    let f = fx();
    f.remote.write(".gitattributes", b"* text eol=crlf\n");
    f.remote.commit("crlf");
    assert!(matches!(refused(sync(&f)), Refused::Gitattributes(_)));
}

#[test]
fn records_with_crlf_bytes_read_back_exactly_even_when_the_clone_says_autocrlf() {
    let f = fx();
    let crlf = b"rev: 1\r\nsummary: windows line endings\r\n".to_vec();
    f.remote.commit_files(&[(A, &crlf)], "crlf record");
    // Poison the clone's own configuration the way a careless setup might have it.
    let mut cfg = fs::OpenOptions::new().append(true).open(f.store.paths.clone.join("config")).unwrap();
    cfg.write_all(b"[core]\n\tautocrlf = true\n\teol = crlf\n").unwrap();
    drop(cfg);
    match sync(&f) {
        SyncReport::Accepted { snapshot, .. } => assert_eq!(snapshot.files[A], crlf),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_huge_blob_is_refused_and_removed() {
    let f = fx();
    f.remote.commit_files(&[("indexes/big.bin", &noise(600 * 1024))], "huge");
    match refused(sync(&f)) {
        Refused::ObjectTooLarge { size, .. } => assert_eq!(size, 600 * 1024),
        other => panic!("{other:?}"),
    }
    nothing_accepted(&f);
}

#[test]
fn a_huge_blob_hidden_in_history_and_deleted_at_the_tip_is_still_refused() {
    let f = fx();
    f.remote.commit_files(&[("indexes/big.bin", &noise(600 * 1024))], "add");
    f.remote.remove("indexes/big.bin");
    f.remote.commit_files(&[(A, &rec(1, "one"))], "remove");
    assert!(matches!(refused(sync(&f)), Refused::ObjectTooLarge { .. }));
    nothing_accepted(&f);
}

#[test]
fn a_fetch_that_outgrows_the_byte_cap_is_cut_off() {
    let f = fx_with(Caps { max_total_bytes: 40 * 1024, max_object_bytes: 10 * 1024 * 1024, ..Caps::default() });
    f.remote.commit_files(&[("indexes/a.bin", &noise(200 * 1024)), ("indexes/b.bin", &noise(200 * 1024))], "big");
    assert_eq!(refused(sync(&f)), Refused::TooLarge);
    nothing_accepted(&f);
    let objects = dir_size(&f.store.paths.clone.join("objects"));
    assert!(objects < 100 * 1024, "the refused objects were removed, {objects} bytes remain");
}

#[test]
fn too_many_files_and_too_many_commits_are_refused() {
    let f = fx_with(Caps { max_tree_entries: 3, ..Caps::default() });
    f.remote.commit_files(&[(A, &rec(1, "1")), (B, &rec(1, "2"))], "many files");
    assert_eq!(refused(sync(&f)), Refused::TreeTooLarge);
    let g = fx_with(Caps { max_new_commits: 2, ..Caps::default() });
    for n in 0..3 {
        g.remote.commit_files(&[(A, &rec(n + 1, "x"))], "c");
    }
    assert_eq!(refused(sync(&g)), Refused::TooManyCommits);
}

#[test]
fn a_hostile_store_with_a_hook_a_filter_a_submodule_a_symlink_and_a_huge_blob_runs_nothing() {
    let f = fx();
    let sentinel = f.tmp.path().join("SENTINEL");
    let evil = trap(&f.tmp.path().join("evil"), "run", &sentinel);
    // Everything an attacker can commit that some git workflow would execute or follow.
    let script = fs::read(&evil).unwrap();
    f.remote.write(".gitattributes", b"* filter=evil\n* -text\n");
    f.remote.write(".gitmodules", b"[submodule \"x\"]\n\tpath = vendor/x\n\turl = ext::sh -c touch% SENTINEL\n");
    f.remote.write("hooks/post-checkout", &script);
    f.remote.write(".githooks/post-checkout", &script);
    f.remote.make_executable("hooks/post-checkout");
    f.remote.symlink("records/memory/link", "../../../../etc/passwd");
    f.remote.gitlink("vendor/x", &"2".repeat(40));
    f.remote.write("indexes/big.bin", &noise(700 * 1024));
    f.remote.commit("everything");
    // The attacker also controls the host repository's own hooks.
    f.remote.host_hook("reference-transaction", &format!("#!/bin/sh\ntouch '{}'\n", sentinel.display()));
    let r = sync(&f);
    assert!(matches!(r, SyncReport::Refused(_)), "{r:?}");
    assert!(!sentinel.exists(), "something executed");
    nothing_accepted(&f);
}

#[test]
fn hooks_filters_and_fsmonitor_in_the_clone_configuration_do_not_run() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(1, "one"))], "a");
    assert!(matches!(sync(&f), SyncReport::Accepted { .. }));
    let sentinel = f.tmp.path().join("SENTINEL");
    let hooks = f.tmp.path().join("evil-hooks");
    for h in ["reference-transaction", "post-update", "post-checkout", "pre-auto-gc", "post-merge"] {
        trap(&hooks, h, &sentinel);
    }
    let fsmonitor = trap(&f.tmp.path().join("evil"), "fsmonitor", &sentinel);
    let mut cfg = fs::OpenOptions::new().append(true).open(f.store.paths.clone.join("config")).unwrap();
    write!(cfg, "[core]\n\thooksPath = {}\n\tfsmonitor = {}\n[gc]\n\tauto = 1\n[filter \"evil\"]\n\tclean = touch {}\n\tsmudge = touch {}\n\tprocess = touch {}\n", hooks.display(), fsmonitor.display(), sentinel.display(), sentinel.display(), sentinel.display()).unwrap();
    drop(cfg);
    // Control: git used without the store's hardening does run the planted hook. Without this the
    // assertion below could pass for the wrong reason.
    let head = f.store.accepted_head().unwrap().unwrap();
    let out = plain_git(&f.store.paths.clone, &f.tmp.path().join("h"), &["update-ref", "refs/probe", &head]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(sentinel.exists(), "control failed: the poisoned hook did not fire under plain git");
    fs::remove_file(&sentinel).unwrap();
    // Now every kind of work the store does: fetch, ref updates, listing, reading, gc on refusal.
    f.remote.commit_files(&[(A, &rec(2, "two")), ("indexes/big.bin", &noise(600 * 1024))], "refused");
    assert!(matches!(sync(&f), SyncReport::Refused(_)));
    f.remote.reset_hard(&head);
    f.remote.commit_files(&[(A, &rec(2, "two"))], "ok again");
    assert!(matches!(f.store.fetch().unwrap(), Fetch::Advanced { .. }));
    f.store.snapshot_of(&head).unwrap();
    assert!(!sentinel.exists(), "a poisoned clone configuration ran a program");
}

#[test]
fn the_ext_transport_never_runs_even_if_a_url_got_through() {
    let f = fx();
    let sentinel = f.tmp.path().join("SENTINEL");
    let url = format!("ext::touch {}", sentinel.display());
    // Control: allowed by an unhardened configuration, the helper runs.
    let _ = Command::new("git").env_clear().env("PATH", std::env::var_os("PATH").unwrap()).env("HOME", f.tmp.path()).env("GIT_CONFIG_GLOBAL", "/dev/null").args(["-c", "protocol.ext.allow=always", "ls-remote", &url]).output().unwrap();
    assert!(sentinel.exists(), "control failed: ext:: did not run under permissive configuration");
    fs::remove_file(&sentinel).unwrap();
    for scheme in [Some(Scheme::Https), Some(Scheme::Ssh), Some(Scheme::File), None] {
        let mut c = git_command(&f.store.paths, scheme, None);
        let out = c.args(["ls-remote", &url]).output().unwrap();
        assert!(!out.status.success());
        assert!(!sentinel.exists(), "ext:: ran with scheme {scheme:?}");
    }
    assert!(Store::open(&f.home, "acme", "other", &url, true).is_err(), "and the URL parser rejects it first");
}

#[test]
fn only_the_transport_the_store_url_needs_is_open() {
    let f = fx();
    f.remote.commit_files(&[(A, &rec(1, "one"))], "a");
    let url = f.remote.url();
    let ls = |scheme| git_command(&f.store.paths, scheme, None).args(["ls-remote", &url]).output().unwrap().status.success();
    assert!(ls(Some(Scheme::File)));
    assert!(!ls(Some(Scheme::Https)), "a store on https may not be redirected to a local path");
    assert!(!ls(Some(Scheme::Ssh)));
    assert!(!ls(None), "local-only calls open no transport at all");
}

#[test]
fn the_git_command_reads_no_configuration_but_its_own() {
    let f = fx();
    fs::write(f.store.paths.empty_home.join(".gitconfig"), "[core]\n\thooksPath = /evil\n[user]\n\tname = evil\n").unwrap();
    fs::create_dir_all(f.store.paths.empty_home.join(".config/git")).unwrap();
    fs::write(f.store.paths.empty_home.join(".config/git/config"), "[core]\n\tfsmonitor = /evil\n").unwrap();
    let out = git_command(&f.store.paths, Some(Scheme::File), None).args(["config", "--list", "--show-origin"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!text.contains("/evil") && !text.contains("name=evil"), "{text}");
    assert!(text.lines().all(|l| l.starts_with("command line:")), "{text}");
    let hooks = f.store.paths.empty_hooks.display().to_string();
    for want in [format!("core.hookspath={hooks}"), "core.fsmonitor=false".into(), "core.symlinks=false".into(), "protocol.allow=never".into(), "protocol.file.allow=always".into(), "credential.helper=".into(), "core.autocrlf=false".into()] {
        assert!(text.contains(&want), "missing {want}\n{text}");
    }
    assert!(!text.contains("protocol.https.allow") && !text.contains("protocol.ext"), "only the store's transport is opened\n{text}");
    let envs: BTreeMap<_, _> = git_command(&f.store.paths, None, None).get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
    assert_eq!(envs["GIT_CONFIG_GLOBAL"].as_deref(), Some("/dev/null"));
    assert_eq!(envs["GIT_TERMINAL_PROMPT"].as_deref(), Some("0"));
    assert_eq!(envs["GIT_ALLOW_PROTOCOL"].as_deref(), Some("none"));
    assert!(!envs.contains_key("GIT_SSH_COMMAND") && !envs.contains_key("SSH_AUTH_SOCK"));
}

#[test]
fn only_the_ssh_transport_gets_the_ssh_environment() {
    let f = fx();
    let envs: BTreeMap<_, _> = git_command(&f.store.paths, Some(Scheme::Ssh), None).get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
    let ssh = envs["GIT_SSH_COMMAND"].clone().unwrap();
    assert!(ssh.contains("BatchMode=yes") && ssh.contains("StrictHostKeyChecking=yes") && ssh.contains("ProxyCommand=none"));
    assert_eq!(envs["GIT_ALLOW_PROTOCOL"].as_deref(), Some("ssh"));
}

#[test]
fn branch_names_cannot_smuggle_options_or_refs() {
    let f = fx();
    for bad in ["", "-x", "--upload-pack=x", "a..b", "a b", "/x", "x/", "refs/../x", "a.lock", "a//b", ".hidden", "a\nb"] {
        assert!(Store::open(&f.home, "acme", "b1", &f.remote.url(), true).unwrap().with_branch(bad).is_err(), "{bad:?}");
    }
    assert!(Store::open(&f.home, "acme", "b2", &f.remote.url(), true).unwrap().with_branch("release/1.x").is_ok());
}
