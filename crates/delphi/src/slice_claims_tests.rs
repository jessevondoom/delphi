//! Spec 135 AC-1 to AC-5. The repositories are files only: a `.git` directory and a worktree's `.git` file, no git process.

use super::*;
use std::fs;

struct Fixture {
    _dir: tempfile::TempDir,
    main: PathBuf,
    wt: PathBuf,
    home: PathBuf,
}

const T0: i64 = 1_800_000_000;

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("repo");
    fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
    fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::create_dir_all(main.join("specs")).unwrap();
    fs::write(main.join("specs/135-the-forge.md"), "---\nid: x\nstatus: approved   # by the operator\n---\n# 135\n").unwrap();
    fs::write(main.join("specs/136-a-draft.md"), "---\nid: y\nstatus: draft\n---\n# 136\n").unwrap();
    let tr = main.join(tracker::PATH);
    fs::create_dir_all(tr.parent().unwrap()).unwrap();
    fs::write(&tr, "| Spec | Slice | Content | State |\n| :-- | :-- | :-- | :-- |\n| 135 | F0 | claims | not built |\n| 135 | F1 | finisher | built and merged (#1) |\n").unwrap();
    // A worktree of the same repository, on its own branch, with its own copy of the files.
    let wt = dir.path().join("wt");
    fs::create_dir_all(wt.join("specs")).unwrap();
    fs::write(wt.join(".git"), format!("gitdir: {}\n", main.join(".git/worktrees/wt").display())).unwrap();
    fs::write(main.join(".git/worktrees/wt/HEAD"), "ref: refs/heads/feature-x\n").unwrap();
    fs::write(main.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
    for f in ["specs/135-the-forge.md", "specs/136-a-draft.md"] {
        fs::copy(main.join(f), wt.join(f)).unwrap();
    }
    fs::create_dir_all(wt.join(tr.parent().unwrap().strip_prefix(&main).unwrap())).unwrap();
    fs::copy(&tr, wt.join(tracker::PATH)).unwrap();
    let home = dir.path().join("home");
    Fixture { _dir: dir, main, wt, home }
}

fn req<'a>(repo: &'a Path, home: &'a Path, slice: &'a str, by: Option<&'a str>, take: bool, now: i64) -> ClaimReq<'a> {
    ClaimReq { repo, home, spec: "135", slice, by, ttl_secs: 3600, take, now }
}

#[test]
fn ac4_worktrees_of_one_checkout_name_one_repository_and_default_to_their_branch() {
    let f = fixture();
    let a = git_facts(&f.main).unwrap();
    let b = git_facts(&f.wt.join("specs")).unwrap();
    assert_eq!(a.common, b.common);
    assert_eq!(a.branch.as_deref(), Some("main"));
    assert_eq!(b.branch.as_deref(), Some("feature-x"));
    assert_eq!(b.top, fs::canonicalize(&f.wt).unwrap());
    let o = claim(&req(&f.wt, &f.home, "F0", None, false, T0));
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("for feature-x"), "{}", o.out);
}

#[test]
fn ac1_a_live_claim_refuses_another_holder_until_taken_or_expired() {
    let f = fixture();
    assert_eq!(claim(&req(&f.wt, &f.home, "F0", Some("A"), false, T0)).code, 0);
    let o = claim(&req(&f.main, &f.home, "F0", Some("B"), false, T0 + 60));
    assert_eq!(o.code, 1);
    assert!(o.err.contains("taken: 135 F0 by A"), "{}", o.err);
    // The same holder renews.
    let o = claim(&req(&f.wt, &f.home, "F0", Some("A"), false, T0 + 120));
    assert_eq!(o.code, 0);
    assert!(o.out.contains("renewed"), "{}", o.out);
    // --take records whom it took from.
    let o = claim(&req(&f.main, &f.home, "F0", Some("B"), true, T0 + 180));
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("taken over from A"));
    let file = load(&f.home).unwrap();
    assert_eq!(file.claims.len(), 1);
    assert_eq!(file.claims[0].took_from.as_deref(), Some("A"));
    // After expiry anyone may claim without --take.
    let o = claim(&req(&f.wt, &f.home, "F0", Some("C"), false, T0 + 180 + 3601));
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("had expired"), "{}", o.out);
}

#[test]
fn ac2_drafts_unknown_specs_and_bad_slice_ids_are_refused() {
    let f = fixture();
    let mut r = req(&f.main, &f.home, "F0", Some("A"), false, T0);
    r.spec = "136";
    let o = claim(&r);
    assert_eq!(o.code, 1);
    assert!(o.err.contains("not approved"), "{}", o.err);
    r.spec = "999";
    assert_eq!(claim(&r).code, 2);
    r.spec = "135";
    for bad in ["", "-x", "a b", "waytoolongsliceid17", "x;y"] {
        r.slice = bad;
        assert_eq!(claim(&r).code, 2, "{:?}", bad);
    }
    assert!(!f.home.join("forge/claims.json").exists());
}

#[test]
fn ac5_the_tracker_turns_into_warnings_never_refusals() {
    let f = fixture();
    let o = claim(&req(&f.main, &f.home, "Z9", Some("A"), false, T0));
    assert_eq!(o.code, 0);
    assert!(o.err.contains("lists no slice Z9 for spec 135; it lists F0, F1"), "{}", o.err);
    let o = claim(&req(&f.main, &f.home, "F1", Some("A"), false, T0));
    assert_eq!(o.code, 0);
    assert!(o.err.contains("already merged"), "{}", o.err);
    let o = claim(&req(&f.main, &f.home, "F0", Some("A"), false, T0));
    assert!(o.err.is_empty(), "{}", o.err);
}

#[test]
fn ac3_concurrent_claims_have_exactly_one_winner() {
    let f = fixture();
    let (main, home) = (f.main.clone(), f.home.clone());
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let (main, home) = (main.clone(), home.clone());
            std::thread::spawn(move || {
                let by = format!("holder-{}", i);
                claim(&ClaimReq { repo: &main, home: &home, spec: "135", slice: "F0", by: Some(&by), ttl_secs: 3600, take: false, now: T0 }).code
            })
        })
        .collect();
    let codes: Vec<i32> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(codes.iter().filter(|c| **c == 0).count(), 1, "{:?}", codes);
    assert_eq!(codes.iter().filter(|c| **c == 1).count(), 7, "{:?}", codes);
    assert_eq!(load(&f.home).unwrap().claims.len(), 1);
}

#[test]
fn release_is_the_holders_and_list_shows_live_claims_of_this_repository() {
    let f = fixture();
    assert_eq!(claim(&req(&f.wt, &f.home, "F0", None, false, T0)).code, 0);
    let o = release(&f.main, &f.home, "135", "F0", Some("someone-else"), T0 + 1);
    assert_eq!(o.code, 1);
    assert!(o.err.contains("not yours"));
    let l = list(&f.main, &f.home, false, false, T0 + 1);
    assert!(l.out.contains("135 F0 by feature-x"), "{}", l.out);
    assert!(claim_of_branch(&f.wt, &f.home, T0 + 1).is_some());
    assert!(claim_of_branch(&f.main, &f.home, T0 + 1).is_none());
    let o = release(&f.wt, &f.home, "135", "F0", None, T0 + 2);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(list(&f.main, &f.home, false, false, T0 + 3).out.contains("no live claims"));
    assert_eq!(release(&f.wt, &f.home, "135", "F0", None, T0 + 4).code, 0);
}

#[test]
fn a_malformed_file_is_reported_and_never_overwritten() {
    let f = fixture();
    fs::create_dir_all(f.home.join("forge")).unwrap();
    fs::write(f.home.join("forge/claims.json"), "{not json").unwrap();
    let o = claim(&req(&f.main, &f.home, "F0", Some("A"), false, T0));
    assert_eq!(o.code, 4);
    assert!(o.err.contains("not a claims file"), "{}", o.err);
    assert_eq!(fs::read_to_string(f.home.join("forge/claims.json")).unwrap(), "{not json");
}
