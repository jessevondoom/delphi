//! Spec 092 S2d1 tests: the anchor of the real use ledger. Each test names an attack on the ledger and fails against a build without the
//! anchor. Nothing here runs a command: the fixture "interpreter" is a file that is only ever hashed.

use super::*;
use crate::authorization_grant::{launch_payload, take_use, take_use_anchored};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

struct Fx {
    _t: tempfile::TempDir,
    home: PathBuf,
    id: String,
    policy: a::Policy,
}

fn now() -> i64 {
    delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap()
}

fn fixture(max_uses: u32) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(t.path()).unwrap();
    let home = root.join("home");
    fs::create_dir_all(home.join("operator")).unwrap();
    delphi_core::lock::write(&home.join("operator/ratified.lock"), &Default::default()).unwrap();
    let (bin, script) = (root.join("bash"), root.join("t.sh"));
    fs::write(&bin, b"#!fake interpreter\n").unwrap();
    fs::write(&script, b"#!/bin/bash\nexit 0\n").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();
    let (b, s) = (bin.to_string_lossy().to_string(), script.to_string_lossy().to_string());
    let policy = a::Policy { interpreters: vec![b.clone()], sealed_roots: vec![] };
    let entry = a::Entry { tool: "shell".into(), argv: vec![b, s.clone()], files: vec![s], runner_provenance: "project_snapshot".into() };
    let n = a::NewRecord { purpose: "p".into(), intent: "i".into(), run: "run-1".into(), expires_in_secs: a::DEFAULT_EXPIRY_SECS, max_uses, entries: vec![entry], request: None, policy: policy.clone() };
    let rec = a::build_record(&home, &n, now(), &delphi_core::time::now_rfc3339()).unwrap();
    let id = rec.id.clone();
    a::create_audited(&home, rec.clone(), json!({"id": rec.id, "manifest_hash": rec.manifest_hash, "run": rec.run, "by": "operator", "source": "operator"})).unwrap();
    Fx { _t: t, home, id, policy }
}

impl Fx {
    fn launch(&self) -> Result<(Vec<u8>, RunAnchor), String> {
        launch_granted(&self.home, &self.id, "run-1", now(), &self.policy)
    }

    /// One whole run: launch, `uses` real uses (anchored, as a daemon takes them), end.
    fn run(&self, uses: u32) -> LedgerState {
        let (_bytes, anchor) = self.launch().unwrap();
        let mut st = ledger_state(&self.home, &self.id).unwrap();
        for _ in 0..uses {
            st = take_use_anchored(&self.home, &self.id, "h", "run-1", 50, &st).unwrap().1;
        }
        anchor.end().unwrap()
    }

    fn segment(&self) -> PathBuf {
        fs::read_dir(ledger_dir(&self.home, &self.id).join("events")).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "jsonl")).unwrap()
    }

    /// Cut the last `n` lines off the ledger (a truncated tail: the chain stays valid).
    fn cut_tail(&self, n: usize) {
        let seg = self.segment();
        let text = fs::read_to_string(&seg).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        fs::write(&seg, format!("{}\n", lines[..lines.len() - n].join("\n"))).unwrap();
    }
}

#[test]
fn a_clean_run_launches_ends_and_the_next_launch_finds_the_ledger_where_it_was_left() {
    let f = fixture(10);
    let end = f.run(2);
    assert_eq!(end.count, 2);
    let (_b, anchor) = f.launch().unwrap();
    // the launch and end events are in the pinned audit chain, and the store's recorded head moved with them
    let events = a::read_audit(&f.home).unwrap();
    assert_eq!(events.iter().filter(|e| e.kind == LAUNCH).count(), 2);
    assert_eq!(events.iter().filter(|e| e.kind == RUN_END).count(), 1);
    assert_eq!(a::load(&f.home).unwrap().audit_head.unwrap().seq, events.last().unwrap().seq);
    drop(anchor);
    assert_eq!(a::read_audit(&f.home).unwrap().iter().filter(|e| e.kind == RUN_END).count(), 2, "dropping the anchor records the end");
}

#[test]
fn a_cut_tail_of_the_ledger_is_refused_at_the_next_launch_by_name() {
    let f = fixture(10);
    f.run(3);
    f.cut_tail(1);
    let err = f.launch().err().unwrap();
    assert!(err.contains("not where the last run left it") && err.contains("cut"), "{err}");
    f.cut_tail(1);
    assert!(f.launch().is_err());
}

#[test]
fn a_deleted_ledger_directory_is_refused_at_the_next_launch() {
    let f = fixture(10);
    f.run(2);
    fs::remove_dir_all(ledger_dir(&f.home, &f.id)).unwrap();
    let err = f.launch().err().unwrap();
    assert!(err.contains("not where the last run left it"), "{err}");
}

#[test]
fn an_older_copy_of_the_ledger_put_back_is_refused() {
    let f = fixture(10);
    f.run(1);
    let copy = f.home.join("operator/uses-copy");
    fs::create_dir_all(&copy).unwrap();
    fs::copy(f.segment(), copy.join("000001.jsonl")).unwrap();
    f.run(2);
    fs::remove_dir_all(ledger_dir(&f.home, &f.id)).unwrap();
    fs::create_dir_all(ledger_dir(&f.home, &f.id).join("events")).unwrap();
    fs::copy(copy.join("000001.jsonl"), ledger_dir(&f.home, &f.id).join("events/000001.jsonl")).unwrap();
    assert_eq!(ledger_state(&f.home, &f.id).unwrap().count, 1, "the old copy verifies on its own");
    let err = f.launch().err().unwrap();
    assert!(err.contains("not where the last run left it"), "{err}");
}

#[test]
fn a_ledger_with_uses_and_no_recorded_run_is_refused() {
    let f = fixture(10);
    take_use(&f.home, &f.id, "h", "run-1", 10).unwrap();
    let err = f.launch().err().unwrap();
    assert!(err.contains("records no run of it"), "{err}");
}

#[test]
fn the_ledger_changing_under_a_running_daemon_refuses_its_next_use() {
    let f = fixture(10);
    let (_b, anchor) = f.launch().unwrap();
    let st0 = ledger_state(&f.home, &f.id).unwrap();
    let (_, st1) = take_use_anchored(&f.home, &f.id, "h", "run-1", 10, &st0).unwrap();
    let (_, st2) = take_use_anchored(&f.home, &f.id, "h", "run-1", 10, &st1).unwrap();
    // mid-run, a same-user process cuts the last use off: the daemon's memory says 2, the disk says 1
    f.cut_tail(1);
    let err = take_use_anchored(&f.home, &f.id, "h", "run-1", 10, &st2).unwrap_err();
    assert!(err.contains("not where this run left it") && err.contains("no use is taken"), "{err}");
    // a whole-directory deletion is the same
    fs::remove_dir_all(ledger_dir(&f.home, &f.id)).unwrap();
    assert!(take_use_anchored(&f.home, &f.id, "h", "run-1", 10, &st2).unwrap_err().contains("not where this run left it"));
    drop(anchor);
}

#[test]
fn a_crash_with_no_run_end_is_allowed_only_while_the_ledger_has_not_shrunk_and_says_so() {
    let f = fixture(10);
    f.run(1);
    // run 2 launches, takes a use, and dies without recording its end
    let (_b, anchor) = f.launch().unwrap();
    let st = ledger_state(&f.home, &f.id).unwrap();
    take_use_anchored(&f.home, &f.id, "h", "run-1", 10, &st).unwrap();
    std::mem::forget(anchor); // no Drop: the process "died"
    let (_b3, anchor3) = f.launch().unwrap();
    let launches: Vec<_> = a::read_audit(&f.home).unwrap().into_iter().filter(|e| e.kind == LAUNCH).collect();
    assert_eq!(launches.last().unwrap().payload["after_crash"], json!(true), "the launch after a crash says so");
    assert_eq!(launches.last().unwrap().payload["ledger_count"], json!(2));
    drop(anchor3);
    // the same crash, then the ledger is cut below what that run launched with: refused even without a run_end
    let (_b, anchor) = f.launch().unwrap();
    std::mem::forget(anchor);
    f.cut_tail(1); // the ledger now holds fewer uses than the crashed run's launch recorded
    let err = f.launch().err().unwrap();
    assert!(err.contains("cut or deleted"), "{err}");
}

#[test]
fn a_crashed_run_whose_tail_is_cut_is_not_seen_the_documented_residual() {
    // The one thing the anchor cannot do (module comment, point 3): uses taken AFTER the last recorded launch, in a run that crashed,
    // and then cut off. The ledger is still at least as long as at the launch, so the launch is allowed (and says so).
    let f = fixture(10);
    f.run(1);
    let (_b, anchor) = f.launch().unwrap();
    let st = ledger_state(&f.home, &f.id).unwrap();
    take_use_anchored(&f.home, &f.id, "h", "run-1", 10, &st).unwrap();
    std::mem::forget(anchor);
    f.cut_tail(1);
    let (_b, anchor) = f.launch().expect("allowed: the ledger is still where that run LAUNCHED");
    drop(anchor);
}

#[test]
fn cutting_the_anchor_events_off_the_audit_log_is_the_existing_loud_refusal() {
    let f = fixture(10);
    f.run(1);
    let dir = a::audit_dir(&f.home).join("events");
    let seg = fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "jsonl")).unwrap();
    let text = fs::read_to_string(&seg).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    fs::write(&seg, format!("{}\n", lines[..lines.len() - 1].join("\n"))).unwrap(); // the run.end event
    let err = f.launch().err().unwrap();
    assert!(err.contains("fails verification") && err.contains("cut"), "{err}");
}

#[test]
fn the_payload_carries_the_ledger_state_the_launcher_recorded() {
    let f = fixture(10);
    f.run(2);
    let (bytes, _a) = f.launch().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["ledger"]["count"], json!(2));
    let plain = launch_payload(&f.home, &f.id, "run-1", now(), &f.policy).unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&plain).unwrap()["ledger"]["count"], json!(2));
}

#[test]
fn a_crashed_runs_ledger_replaced_by_a_longer_forged_one_is_refused_by_its_head() {
    // Crash containment: the launch event recorded the ledger's head. A different ledger that is at least as long (so the count test
    // passes) but does not contain that head event is not the ledger that run launched with.
    let f = fixture(10);
    f.run(1);
    let (_b, anchor) = f.launch().unwrap();
    std::mem::forget(anchor); // the run "crashed": no run.end
    fs::remove_dir_all(ledger_dir(&f.home, &f.id)).unwrap();
    take_use(&f.home, &f.id, "a-different-record-hash", "another-run", 10).unwrap();
    take_use(&f.home, &f.id, "a-different-record-hash", "another-run", 10).unwrap();
    assert!(ledger_state(&f.home, &f.id).unwrap().count >= 1, "long enough for the count check");
    let err = f.launch().err().unwrap();
    assert!(err.contains("does not contain the event") && err.contains("replaced by another copy"), "{err}");
}
