//! `specs/100` §7 and §10 items 5 to 7, from the outside: seed an audit with a hard failure, then check the label falls back, nothing is deleted, the
//! `tier.rollback` event is written, the latch outlives the process, only the operator's restore raises it, and an agent seat is refused.
//!
//! This file is its own test binary on purpose. The roll-back latch is a process-wide setting (`set_tier_latch_file`), and one test that sets it
//! must not share a process with tests that read a claim's status.

use delphi_core::knowledge_status::{reload_tier_latch, set_tier_latch_file, tier_params, Status};
use delphi_engine::fact_audit::{Answer, Journal};
use delphi_engine::library::claims::{status, AnchorSpec, ClaimOpts};
use delphi_engine::library::{AddOpts, Budgets, Library};
use delphi_engine::tier_audit as ta;
use std::path::Path;
use std::process::{Command, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

/// A library with one claim on the vetted rung: the top rung's counts and a recorded dry run.
fn vetted_claim(home: &Path) -> (Library, String) {
    let l = Library::in_home(home);
    let page = home.join("page.md");
    std::fs::create_dir_all(home).unwrap();
    std::fs::write(&page, "# Harbour\n\nThe harbour closes at dusk.\n").unwrap();
    let s = l.add_file(&page, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap();
    let a = AnchorSpec { source_id: s.id.clone(), locator: Some("§1".into()), quote: "closes at dusk".into(), ..Default::default() };
    let text = "The harbour closes at dusk.";
    let id = l.add_claim(text, &ClaimOpts::default(), &[a]).unwrap()["id"].as_str().unwrap().to_string();
    let mut c = l.claim(&id).unwrap();
    c["support"]["origins"] = serde_json::json!(5);
    c["support"]["trusted_sources"] = serde_json::json!(2);
    std::fs::write(l.claims_dir().join(format!("{id}.json")), serde_json::to_vec(&c).unwrap()).unwrap();
    assert_eq!(l.record_would_ratify(&delphi_engine::fact_store::fact_id(text)), vec![id.clone()]);
    (l, id)
}

#[test]
fn a_hard_failure_rolls_the_tier_back_the_label_falls_one_rung_nothing_is_deleted_and_only_a_restore_raises_it() {
    let d = tempfile::tempdir().unwrap();
    let (l, id) = vetted_claim(&d.path().join("home"));
    let state = d.path().join("ws/local/validation");
    set_tier_latch_file(ta::latch_file(&state));
    assert!(tier_params().vetted_on && tier_params().substantiated_on);
    assert_eq!(status(&l.claim(&id).unwrap()), Status::Vetted);
    let stored = std::fs::read(l.claims_dir().join(format!("{id}.json"))).unwrap();

    // the audit draws the claim and the operator calls it false: a hard failure
    let members = l.tier_members("vetted");
    assert_eq!(members.len(), 1);
    assert_eq!(ta::draw(&state, "vetted", &members, 5, 1).unwrap(), vec![id.clone()]);
    let got = ta::answer(&state, &id, "vetted", Answer::False, 2).unwrap();
    assert_eq!(got, ta::Answered::RolledBack { tier: "vetted".into(), trigger: "hard_failure".into() });

    // the label falls back one rung, in a process that only had to look at the file
    reload_tier_latch();
    assert!(!tier_params().vetted_on && tier_params().substantiated_on, "vetted is held, substantiated is not");
    assert_eq!(status(&l.claim(&id).unwrap()), Status::Substantiated);
    // nothing is deleted or rewritten: the claim is byte for byte what it was, and its tags are still the top rung's
    assert_eq!(std::fs::read(l.claims_dir().join(format!("{id}.json"))).unwrap(), stored);
    let c = l.claim(&id).unwrap();
    assert_eq!((c["support"]["origins"].as_u64(), c["support"]["trusted_sources"].as_u64(), c["support"]["would_ratify"].as_bool()), (Some(5), Some(2), Some(true)));

    // the event, in the audit journal the report reads
    let roll = Journal::new(&state).all().into_iter().find(|e| e["kind"] == "tier_rollback").expect("a tier.rollback event");
    assert_eq!((roll["tier"].as_str(), roll["trigger"].as_str(), roll["sample"].as_u64(), roll["errors"].as_u64()), (Some("vetted"), Some("hard_failure"), Some(1), Some(1)));
    assert!(roll["audit_batch"].is_string() && roll["bound"].is_f64());

    // a restart is a new process reading the same file: still held
    set_tier_latch_file(ta::latch_file(&state));
    assert_eq!(status(&l.claim(&id).unwrap()), Status::Substantiated, "the latch survives a restart");

    // a config edit alone does not raise it: the ladder's configured numbers and switches never see the latch file
    assert!(delphi_core::knowledge_status::tier_params_configured().vetted_on, "the configuration says vetted is on; the latch still holds it");
    assert!(!tier_params().vetted_on);

    // only the operator's restore raises it, and it needs a reason
    assert!(ta::restore(&state, "vetted", "no", 3).is_err());
    reload_tier_latch();
    assert_eq!(status(&l.claim(&id).unwrap()), Status::Substantiated);
    ta::restore(&state, "vetted", "audited again and the sample was fine", 4).unwrap();
    reload_tier_latch();
    assert_eq!(status(&l.claim(&id).unwrap()), Status::Vetted, "restored");

    // a held substantiated tier takes vetted with it and the claim reads corroborated
    ta::rollback(&state, "substantiated", "operator", "the lane is not trusted today", 5).unwrap();
    reload_tier_latch();
    assert_eq!(status(&l.claim(&id).unwrap()), Status::Corroborated);
    assert_eq!(std::fs::read(l.claims_dir().join(format!("{id}.json"))).unwrap(), stored, "still nothing rewritten");
}

#[test]
fn every_tier_command_is_refused_inside_an_agent_seat_and_creates_nothing() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    for args in [
        &["validation", "tier-audit"][..],
        &["validation", "tier-report"],
        &["validation", "tier", "rollback", "vetted", "--reason", "an agent should not be able to do this"],
        &["validation", "tier", "restore", "vetted", "--reason", "an agent should not be able to do this"],
        &["validation", "tier", "threshold", "vetted", "5"],
    ] {
        let out = Command::new(DELPHI)
            .args(args)
            .arg("--home")
            .arg(&home)
            .env_remove("DELPHI_HOME")
            .env_remove("DELPHI_BRIDGE_SOCKET")
            .env("DELPHI_BRIDGE_TOKEN", "x")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {err}");
        assert!(err.contains("refused") && err.contains("agent seat"), "{args:?}: {err}");
    }
    assert!(!home.exists(), "a refused command does not even create the home: {home:?}");
}
