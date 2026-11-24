//! Spec 135 §14.11: claims on the remote. The remotes are local bare repositories; nothing is pushed anywhere else.

use super::*;

#[test]
fn a_ref_path_is_the_spec_number_and_the_slice_in_upper_case() {
    assert_eq!(ref_path("135", "g1").as_deref(), Some("135/G1"));
    assert_eq!(ref_path("135", "S1.b").as_deref(), Some("135/S1.B"));
    for bad in ["a..b", "x.", "x.lock", "§1", ".x", "-x"] {
        assert_eq!(ref_path("135", bad), None, "{}", bad);
    }
}

#[test]
fn the_machine_id_is_random_kept_and_names_no_host() {
    let t = tempfile::tempdir().unwrap();
    let a = machine_id(t.path());
    assert!(a.starts_with("m-") && a.len() == 14, "{}", a);
    assert_eq!(machine_id(t.path()), a);
    let u = tempfile::tempdir().unwrap();
    assert_ne!(machine_id(u.path()), a);
}

// AC-30 to AC-34: two checkouts of one local bare remote, each with its own home (two machines).

use std::path::PathBuf;
use std::process::Command as Cmd;

const T0: i64 = 1_800_000_000;

fn g(dir: &Path, args: &[&str]) -> String {
    let o = Cmd::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t.invalid", "-c", "commit.gpgSign=false", "-c", "core.hooksPath=/dev/null"]).args(args).output().unwrap();
    assert!(o.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

struct World {
    _t: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
    ha: PathBuf,
    hb: PathBuf,
}

fn world() -> World {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().to_path_buf();
    g(&root, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    g(&root, &["init", "-q", "-b", "main", "a"]);
    let a = root.join("a");
    std::fs::create_dir_all(a.join("specs")).unwrap();
    std::fs::write(a.join("specs/135-the-forge.md"), "---\nid: x\nstatus: approved\n---\n# 135\n").unwrap();
    g(&a, &["add", "-A"]);
    g(&a, &["commit", "-q", "-m", "init"]);
    g(&a, &["remote", "add", "origin", root.join("remote.git").to_str().unwrap()]);
    g(&a, &["push", "-q", "origin", "main"]);
    g(&a, &["checkout", "-q", "-b", "feat-a"]);
    g(&root, &["clone", "-q", root.join("remote.git").to_str().unwrap(), "b"]);
    let b = root.join("b");
    g(&b, &["checkout", "-q", "-b", "feat-b"]);
    World { a, b, ha: root.join("ha"), hb: root.join("hb"), _t: t }
}

fn req<'a>(repo: &'a Path, home: &'a Path, slice: &'a str, take: bool, now: i64) -> ClaimReq<'a> {
    ClaimReq { repo, home, spec: "135", slice, by: None, ttl_secs: 3600, take, now }
}

fn remote_claim(repo: &Path, path: &str) -> Option<Seen> {
    Remote::of(repo).unwrap().fetch(false).unwrap().into_iter().find(|s| s.path == path)
}

#[test]
fn ac30_a_claim_on_another_machine_is_refused_until_taken() {
    let w = world();
    let o = claim(&req(&w.a, &w.ha, "g1", false, T0), false);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("on the remote: refs/delphi/claims/135/G1 for feat-a"), "{}", o.out);
    let ma = machine_id(&w.ha);
    let o = claim(&req(&w.b, &w.hb, "G1", false, T0 + 60), false);
    assert_eq!(o.code, 1, "{}", o.out);
    assert!(o.err.contains("taken on the remote") && o.err.contains(&ma) && o.err.contains("feat-a"), "{}", o.err);
    assert!(slice_claims::claim_of_branch(&w.b, &w.hb, T0 + 60).is_none(), "the refused local claim is released again");
    let o = claim(&req(&w.b, &w.hb, "G1", true, T0 + 120), false);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("taken over from feat-a"), "{}", o.out);
    let c = remote_claim(&w.a, "135/G1").unwrap().claim.unwrap();
    assert_eq!((c.by.as_str(), c.took_from.as_deref(), c.machine.as_str()), ("feat-b", Some("feat-a"), machine_id(&w.hb).as_str()));
    // The local-only switch never touches the remote.
    let o = claim(&req(&w.a, &w.ha, "G2", false, T0), true);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(remote_claim(&w.a, "135/G2").is_none());
}

#[test]
fn ac31_the_holder_renews_and_a_stale_lease_changes_nothing() {
    let w = world();
    assert_eq!(claim(&req(&w.a, &w.ha, "G1", false, T0), false).code, 0);
    let o = claim(&req(&w.a, &w.ha, "G1", false, T0 + 600), false);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("(renewed)"), "{}", o.out);
    let before = remote_claim(&w.a, "135/G1").unwrap();
    let r = Remote::of(&w.a).unwrap();
    let other = g(&w.a, &["rev-parse", "HEAD"]);
    assert_eq!(r.push("135/G1", Some(other.trim()), Some(other.trim())), Push::Lost);
    assert_eq!(r.push("135/G1", None, Some(other.trim())), Push::Lost, "a create over an existing ref loses");
    assert_eq!(remote_claim(&w.a, "135/G1").unwrap().oid, before.oid);
    let o = release(&w.a, &w.ha, "135", "G1", None, false, T0 + 700);
    assert!(o.out.contains("released on the remote"), "{} {}", o.out, o.err);
    assert!(remote_claim(&w.a, "135/G1").is_none());
}

#[test]
fn ac32_an_unreachable_remote_falls_back_to_this_machine() {
    let w = world();
    g(&w.a, &["remote", "set-url", "origin", w.a.join("no-such-remote.git").to_str().unwrap()]);
    let o = claim(&req(&w.a, &w.ha, "G1", false, T0), false);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("claimed 135 G1") && o.err.contains("could not be reached") && o.err.contains("this machine only"), "{} / {}", o.out, o.err);
    let o = list(&w.a, &w.ha, false, false, false, false, T0);
    assert_eq!(o.code, 0);
    assert!(o.out.contains("135 G1") && o.err.contains("could not be reached"), "{} / {}", o.out, o.err);
    let o = reserve_spec_number(&w.a, &w.ha, None, 3600, false, T0);
    assert_eq!((o.code, o.out.as_str()), (0, "136\n"));
    assert!(o.err.contains("NOT reserved"), "{}", o.err);
}

#[test]
fn ac33_expired_claims_are_replaced_listed_and_swept_with_merged_branches() {
    let w = world();
    assert_eq!(claim(&req(&w.a, &w.ha, "G1", false, T0), false).code, 0);
    assert_eq!(claim(&req(&w.a, &w.ha, "G2", false, T0), false).code, 0);
    let later = T0 + 7200;
    let o = claim(&req(&w.b, &w.hb, "G1", false, later), false);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.out.contains("had expired"), "{}", o.out);
    let o = list(&w.b, &w.hb, true, false, false, false, later);
    assert!(o.out.contains("135 G2 by feat-a") && o.out.contains("expired"), "{}", o.out);
    assert!(!list(&w.b, &w.hb, false, false, false, false, later).out.contains("135 G2 by"), "expired claims are listed with --all only");
    // A live claim whose branch then merges into main by a pull request's merge commit.
    g(&w.a, &["checkout", "-q", "-b", "feat-m"]);
    std::fs::write(w.a.join("m.txt"), "m").unwrap();
    g(&w.a, &["add", "-A"]);
    g(&w.a, &["commit", "-q", "-m", "m"]);
    assert_eq!(claim(&req(&w.a, &w.ha, "G3", false, later - 60), false).code, 0);
    g(&w.a, &["checkout", "-q", "main"]);
    g(&w.a, &["merge", "-q", "--no-ff", "feat-m", "-m", "Merge pull request #1 from someone/feat-m"]);
    g(&w.a, &["push", "-q", "origin", "main"]);
    let o = list(&w.b, &w.hb, false, true, true, false, later);
    let v: serde_json::Value = serde_json::from_str(&o.out).unwrap();
    let swept = v["swept"].to_string();
    assert!(swept.contains("135/G2 (expired)") && swept.contains("135/G3 (its branch merged"), "{}", swept);
    let refs: Vec<String> = v["remote"].as_array().unwrap().iter().map(|r| r["ref"].as_str().unwrap().to_string()).collect();
    assert_eq!(refs, vec!["refs/delphi/claims/135/G1".to_string()]);
    assert!(remote_claim(&w.a, "135/G2").is_none() && remote_claim(&w.a, "135/G3").is_none());
}

#[test]
fn ac34_spec_numbers_are_reserved_once_and_swept_when_on_main() {
    let w = world();
    let a = reserve_spec_number(&w.a, &w.ha, None, SPEC_NUMBER_TTL_SECS, false, T0);
    assert_eq!((a.code, a.out.as_str()), (0, "136\n"), "{}", a.err);
    assert!(a.err.contains("reserved spec number 136 for feat-a"), "{}", a.err);
    let b = reserve_spec_number(&w.b, &w.hb, None, SPEC_NUMBER_TTL_SECS, false, T0 + 1);
    assert_eq!((b.code, b.out.as_str()), (0, "137\n"), "{}", b.err);
    assert!(list(&w.b, &w.hb, false, false, false, false, T0 + 2).out.contains("spec number 136 by feat-a"));
    g(&w.a, &["checkout", "-q", "main"]);
    std::fs::write(w.a.join("specs/136-new.md"), "---\nstatus: draft\n---\n").unwrap();
    g(&w.a, &["add", "-A"]);
    g(&w.a, &["commit", "-q", "-m", "136"]);
    g(&w.a, &["push", "-q", "origin", "main"]);
    let o = list(&w.b, &w.hb, false, false, true, false, T0 + 3);
    assert!(o.out.contains("swept refs/delphi/claims/spec-number/136 (its spec is on origin/main)"), "{}", o.out);
    assert!(o.out.contains("spec number 137 by feat-b"), "{}", o.out);
    let o = release_spec_number(&w.b, "137", None, T0 + 4);
    assert!(o.out.contains("released on the remote"), "{} {}", o.out, o.err);
    let c = reserve_spec_number(&w.b, &w.hb, None, SPEC_NUMBER_TTL_SECS, false, T0 + 5);
    assert_eq!(c.out, "137\n", "{}", c.err);
}

#[test]
fn ac31_of_two_checkouts_that_claim_at_once_exactly_one_wins() {
    let w = world();
    let (a, b, ha, hb) = (w.a.clone(), w.b.clone(), w.ha.clone(), w.hb.clone());
    let t = std::thread::spawn(move || claim(&req(&a, &ha, "G9", false, T0), false));
    let mine = claim(&req(&b, &hb, "G9", false, T0), false);
    let theirs = t.join().unwrap();
    let mut codes = vec![mine.code, theirs.code];
    codes.sort();
    assert_eq!(codes, vec![0, 1], "{} {} / {} {}", mine.out, mine.err, theirs.out, theirs.err);
}

// §14.13 I2: the finisher renews the ref; a sweep sees a merged pull request (a squash merge leaves no merge commit).

#[test]
fn ac46_finish_body_renews_the_remote_claim_and_is_quiet_offline() {
    let w = world();
    assert_eq!(claim(&req(&w.a, &w.ha, "I2", false, T0), false).code, 0);
    let before = remote_claim(&w.a, "135/I2").unwrap().claim.unwrap();
    let later = T0 + 600;
    let finish = |now: i64| crate::finisher::finish_with(&crate::finisher::FinishReq { repo: &w.a, home: &w.ha, check: false, spec: None, slice: None, body: true, gates: false, fast_only: false, now }, &mut crate::finisher_gates::SystemRunner);
    let o = finish(later);
    assert!(o.err.contains("on the remote: refs/delphi/claims/135/I2") && o.err.contains("(renewed)"), "{}", o.err);
    let after = remote_claim(&w.a, "135/I2").unwrap().claim.unwrap();
    assert_eq!(after.expires, fmt_time(later + slice_claims::DEFAULT_TTL_SECS));
    assert_ne!(after.expires, before.expires);
    g(&w.a, &["remote", "set-url", "origin", w.a.join("no-such-remote.git").to_str().unwrap()]);
    let o = finish(later + 60);
    assert!(!o.err.contains("could not be reached") && !o.err.contains("not renewed"), "{}", o.err);
    assert!(slice_claims::claim_of_branch(&w.a, &w.ha, later + 60).is_some_and(|c| c.slice == "I2"), "the local claim stands");
}

#[test]
fn ac47_a_sweep_deletes_the_claim_of_a_branch_with_a_merged_pull_request() {
    let w = world();
    assert_eq!(claim(&req(&w.a, &w.ha, "I1", false, T0), false).code, 0);
    assert_eq!(claim(&req(&w.b, &w.hb, "I2", false, T0), false).code, 0);
    let mut asked = Vec::new();
    let mut fake = |b: &str| {
        asked.push(b.to_string());
        b == "feat-a"
    };
    let o = list_with(&w.b, &w.hb, false, false, true, false, T0 + 60, Some(&mut fake));
    assert!(o.out.contains("swept refs/delphi/claims/135/I1 (its branch has a merged pull request)"), "{} / {}", o.out, o.err);
    assert!(o.out.contains("135 I2 by feat-b"), "{}", o.out);
    asked.sort();
    assert_eq!(asked, vec!["feat-a".to_string(), "feat-b".to_string()], "once per branch");
    assert!(remote_claim(&w.a, "135/I1").is_none() && remote_claim(&w.a, "135/I2").is_some());
}
