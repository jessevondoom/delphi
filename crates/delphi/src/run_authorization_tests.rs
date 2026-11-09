//! Spec 092 S2a tests of the launch gate with real (fixture) records. They live here, not in `run.rs`, so the source-scan guard
//! can hold `run.rs` itself to calling `authorize_launch` and nothing else of the authorization module.

use crate::run::*;
use serde_json::json;
use std::path::Path;

/// A tempdir home holding one live record for a fake `bash`, built and stored the way `delphi test authorize` does.
fn authorized_home() -> (tempfile::TempDir, std::path::PathBuf, String, delphi_engine::authorization::Policy) {
    use delphi_engine::authorization as a;
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap();
    let home = root.join("home");
    std::fs::create_dir_all(home.join("operator")).unwrap();
    delphi_core::lock::write(&home.join("operator/ratified.lock"), &Default::default()).unwrap();
    let (bin, script) = (root.join("bash"), root.join("t.sh"));
    std::fs::write(&bin, b"#!fake").unwrap();
    std::fs::write(&script, b"print(1)").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (b, s) = (bin.to_string_lossy().to_string(), script.to_string_lossy().to_string());
    let policy = a::Policy { interpreters: vec![b.clone()], sealed_roots: vec![] };
    let entry = a::Entry { tool: "shell".into(), argv: vec![b, s.clone()], files: vec![s], runner_provenance: "project_snapshot".into() };
    let now = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap();
    let n = a::NewRecord { purpose: "p".into(), intent: "i".into(), run: "run-1".into(), expires_in_secs: a::DEFAULT_EXPIRY_SECS, max_uses: 3, entries: vec![entry], request: None, policy: policy.clone() };
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

// The client-side half of the boundary (S2a, kept in S2d2b): a valid record and the flag let the run START; the run's own `decide` still
// denies a Red card with the unchanged text. The approval of a matching `run_command` is the daemon's, tested in
// `delphi_engine::authorization_use` (non-matching Red, merge, network, dispatch, `.git` write, wrong hash, expired, revoked, used up,
// probe failure and a tainted span all denied; a matching card approved).
#[test]
fn the_client_still_denies_a_red_card_with_a_valid_record_and_the_flag() {
    let (_t, home, id, policy) = authorized_home();
    let on = OnCard::Authorization { id: id.clone(), run: "run-1".into() };
    assert_eq!(gate(&home, &id, "run-1", false, false, &policy), None, "the record is valid and the run may start");
    let red_cmd = json!({"tier": "red", "tool": "run_command", "pattern": "run_command /bin/bash /x/t.sh"});
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


// S2c: the launcher builds the pipe payload only for a record the strict launch check passes, and only from a terminal.
#[test]
fn the_launcher_writes_a_payload_for_a_valid_record_and_nothing_for_any_other() {
    let (_t, home, id, policy) = authorized_home();
    let now = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap();
    let bytes = delphi_engine::authorization_grant::launch_payload(&home, &id, "run-1", now, &policy).unwrap();
    assert!(!bytes.is_empty() && bytes.len() <= delphi_engine::authorization_grant::MAX_BOOT_BYTES);
    assert!(delphi_engine::authorization_grant::launch_payload(&home, &id, "run-2", now, &policy).is_err());
    assert!(delphi_engine::authorization_grant::launch_payload(&home, "ta-nope", "run-1", now, &policy).is_err());
    // the use ledger is what counts uses now (S2a note c): a record whose ledger is full does not launch
    for _ in 0..3 {
        delphi_engine::authorization_grant::take_use(&home, &id, "h", "run-1", 3).unwrap();
    }
    assert!(delphi_engine::authorization_grant::launch_payload(&home, &id, "run-1", now, &policy).unwrap_err().contains("no uses left"));
}

#[test]
fn a_launch_with_no_controlling_terminal_is_refused() {
    assert!(controlling_terminal_refusal(false).unwrap().contains("controlling terminal"));
    assert_eq!(controlling_terminal_refusal(true), None);
}

// The safety boundary again: whatever the daemon does with its grant, the run's own answer to a Red card of any kind is a denial.
#[test]
fn the_run_itself_denies_a_red_card_of_every_kind_it_is_asked_about() {
    let on = OnCard::Authorization { id: "ta-x".into(), run: "run-1".into() };
    for tool in ["run_command", "shell", "harness_bash", "harness_edit", "dispatch_agent"] {
        let (d, why) = decide(&on, &json!({"tier": "red", "tool": tool, "pattern": tool})).unwrap();
        assert_eq!(d, "deny", "{tool}");
        assert!(why.contains("never approved without the operator"));
    }
}

// Review P1 (#601): the granted daemon lives and dies with the run, so --keep-daemon is refused with the flag.
#[test]
fn keep_daemon_is_refused_with_an_authorization() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let e = parse_args(&a(&["do it", "--on-card", "authorization:ta-0123456789", "--auth-run", "run-1", "--keep-daemon"])).unwrap_err();
    assert!(e.contains("--keep-daemon") && e.contains("authorization"), "{e}");
    assert!(parse_args(&a(&["do it", "--on-card", "authorization:ta-0123456789", "--auth-run", "run-1"])).is_ok());
    assert!(parse_args(&a(&["do it", "--keep-daemon"])).is_ok(), "other runs may keep their daemon");
}
