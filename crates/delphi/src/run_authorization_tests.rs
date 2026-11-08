//! Spec 092 S2a tests of the launch gate with real (fixture) records. They live here, not in `run.rs`, so the source-scan guard
//! can hold `run.rs` itself to calling `authorize_launch` and nothing else of the authorization module.

use crate::run::*;
use serde_json::json;
use std::path::Path;

/// A tempdir home holding one live record for a fake `python3`, built and stored the way `delphi test authorize` does.
fn authorized_home() -> (tempfile::TempDir, std::path::PathBuf, String, delphi_engine::authorization::Policy) {
    use delphi_engine::authorization as a;
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap();
    let home = root.join("home");
    std::fs::create_dir_all(home.join("operator")).unwrap();
    delphi_core::lock::write(&home.join("operator/ratified.lock"), &Default::default()).unwrap();
    let (bin, script) = (root.join("python3"), root.join("t.py"));
    std::fs::write(&bin, b"#!fake").unwrap();
    std::fs::write(&script, b"print(1)").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (b, s) = (bin.to_string_lossy().to_string(), script.to_string_lossy().to_string());
    let policy = a::Policy { interpreters: vec![b.clone()] };
    let entry = a::Entry { tool: "shell".into(), argv: vec![b, "-I".into(), s.clone()], files: vec![s], runner_provenance: "project_snapshot".into() };
    let now = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap();
    let n = a::NewRecord { purpose: "p".into(), intent: "i".into(), run: "run-1".into(), expires_in_secs: a::DEFAULT_EXPIRY_SECS, max_uses: 3, entries: vec![entry], request: None, policy: a::Policy { interpreters: policy.interpreters.clone() } };
    let rec = a::build_record(&home, &n, now, &delphi_core::time::now_rfc3339()).unwrap();
    let id = rec.id.clone();
    a::create_audited(&home, rec.clone(), json!({"id": rec.id, "manifest_hash": rec.manifest_hash, "run": rec.run, "by": "operator", "source": "operator"})).unwrap();
    (t, home, id, policy)
}

fn gate(home: &Path, id: &str, run: &str, seat: bool, marker: bool, policy: &delphi_engine::authorization::Policy) -> Option<String> {
    let now = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap();
    launch_refusal(home, &OnCard::Authorization { id: id.into(), run: run.into() }, seat, marker, Some(now), policy)
}

#[test]
fn the_launch_gate_passes_a_valid_record_and_refuses_each_other_case_with_a_reason() {
    let (_t, home, id, policy) = authorized_home();
    assert_eq!(gate(&home, &id, "run-1", false, false, &policy), None);
    assert!(gate(&home, &id, "run-2", false, false, &policy).unwrap().contains("another run"));
    assert!(gate(&home, "ta-unknown", "run-1", false, false, &policy).unwrap().contains("no authorization"));
    let other = tempfile::tempdir().unwrap();
    assert!(gate(other.path(), &id, "run-1", false, false, &policy).is_some(), "another home has no such record");
    assert_eq!(launch_refusal(&home, &OnCard::Deny, true, true, None, &policy), None, "only the authorization flag is gated");
}

#[test]
fn a_run_started_by_an_agent_seat_or_nested_in_an_authorized_run_may_not_carry_the_flag() {
    let (_t, home, id, policy) = authorized_home();
    assert!(gate(&home, &id, "run-1", true, false, &policy).unwrap().contains("agent seat"));
    let nested = gate(&home, &id, "run-1", false, true, &policy).unwrap();
    assert!(nested.contains(AUTHORIZED_RUN_ENV) && nested.contains("nested"), "{nested}");
}

#[test]
fn a_broken_store_refuses_the_run() {
    let (_t, home, id, policy) = authorized_home();
    let path = home.join("operator").join(["authorizations", "json"].join("."));
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0] = b' ';
    std::fs::write(&path, bytes).unwrap();
    assert!(gate(&home, &id, "run-1", false, false, &policy).unwrap().contains("pin"));
}

// The safety boundary of S2a: a valid record and the flag approve nothing.
#[test]
fn a_red_card_is_denied_even_with_a_valid_record_and_the_flag() {
    let (_t, home, id, policy) = authorized_home();
    let on = OnCard::Authorization { id: id.clone(), run: "run-1".into() };
    assert_eq!(gate(&home, &id, "run-1", false, false, &policy), None, "the record is valid and the run may start");
    let red_cmd = json!({"tier": "red", "tool": "run_command", "pattern": "run_command /usr/bin/python3 -I /x/t.py"});
    let (tier, why) = decide(&on, &red_cmd).unwrap();
    assert_eq!(tier, "deny");
    assert!(why.contains("a Red action is never approved without the operator"), "{why}");
}

// An unreadable clock used to become `now = 0`, which makes every expiry look in the future: it must refuse instead.
#[test]
fn a_clock_that_cannot_be_read_refuses_the_launch_instead_of_passing_every_expiry() {
    let (_t, home, id, policy) = authorized_home();
    let on = OnCard::Authorization { id: id.clone(), run: "run-1".into() };
    let why = launch_refusal(&home, &on, false, false, None, &policy).unwrap();
    assert!(why.contains("clock"), "{why}");
}

