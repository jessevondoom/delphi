//! `delphi history off | purge | delete | status` from the outside (`specs/102` §7 item 6, §11 items 7, 10 and 15; slice S4).
//! The records here are made by the daemon's own tap in a temporary home with obviously fake text.

use delphi_core::event::EventEnvelope;
use delphi_engine::history_record::{Settings, Tap};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

fn delphi(home: &Path, args: &[&str], seat: bool) -> Output {
    let mut c = Command::new(DELPHI);
    c.args(args).arg("--workspace").arg("w").arg("--home").arg(home).env_remove("DELPHI_HOME").env_remove("DELPHI_BRIDGE_SOCKET").env_remove("DELPHI_BRIDGE_TOKEN").stdin(Stdio::null());
    if seat {
        c.env("DELPHI_BRIDGE_TOKEN", "x");
    }
    c.output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// A home with one workspace, and `n` records in its `home` project written by the daemon's tap.
fn fixture(n: usize) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let proj = t.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    let init = Command::new(DELPHI).args(["init", "--non-interactive", "--workspace", "w", "--project"]).arg(&proj).arg("--home").arg(&home).env_remove("DELPHI_HOME").stdin(Stdio::null()).output().unwrap();
    assert!(init.status.success(), "{}", text(&init));
    let ws = home.join("workspaces/w");
    assert!(ws.is_dir(), "the workspace is at {}", ws.display());
    let tap = Tap::new(ws.clone(), Settings { enabled: true, ..Settings::default() }, "home");
    for i in 0..n {
        tap.note_operator_prompt("t_1", &format!("FAKE-CANARY prompt number {i}"));
        let wrote = tap.on_events("m1", &[EventEnvelope::new("m1", "research.finished", json!({"topic": "queues", "new": 1, "updated": 0, "held": 0, "outcome": "done"}))]);
        assert_eq!(wrote.len(), 1);
    }
    (t, home, ws)
}

fn store(ws: &Path) -> String {
    std::fs::read_to_string(ws.join("local/history/home/records.jsonl")).unwrap_or_default()
}

#[test]
fn status_shows_counts_and_the_chain_and_none_of_the_text() {
    let (_t, home, _ws) = fixture(3);
    let o = delphi(&home, &["history", "status"], false);
    assert!(o.status.success(), "{}", text(&o));
    let out = text(&o);
    assert!(out.contains("History: off") || out.contains("History: on"), "{out}");
    assert!(out.contains("3 record(s)") && out.contains("chain intact") && out.contains("Key: present"), "{out}");
    assert!(!out.contains("FAKE-CANARY"), "status printed record text: {out}");
}

#[test]
fn off_stops_new_records_and_deletes_nothing() {
    let (_t, home, ws) = fixture(2);
    let before = store(&ws);
    let o = delphi(&home, &["history", "off"], false);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("nothing was deleted"), "{}", text(&o));
    assert!(ws.join("local/history.off").exists());
    assert_eq!(store(&ws), before, "off deletes nothing");
    let tap = Tap::new(ws.clone(), Settings { enabled: true, ..Settings::default() }, "home");
    tap.note_operator_prompt("t_2", "FAKE-CANARY after off");
    let wrote = tap.on_events("m1", &[EventEnvelope::new("m1", "research.finished", json!({"topic": "q", "new": 0, "updated": 0, "held": 0, "outcome": "done"}))]);
    assert_eq!(wrote, vec![delphi_engine::history_record::Appended::SwitchedOff]);
    assert_eq!(store(&ws), before, "a running daemon honours the marker at once");
    let status = text(&delphi(&home, &["history", "status"], false));
    assert!(status.contains("switched off with `delphi history off`"), "{status}");
}

#[test]
fn purge_erases_the_text_leaves_tombstones_and_the_chain_still_verifies() {
    let (_t, home, ws) = fixture(3);
    assert!(store(&ws).contains("FAKE-CANARY"));
    // Without a terminal and without --yes it refuses and erases nothing.
    let refused = delphi(&home, &["history", "purge"], false);
    assert_eq!(refused.status.code(), Some(2), "{}", text(&refused));
    assert!(store(&ws).contains("FAKE-CANARY"), "a refused purge erased nothing");
    let o = delphi(&home, &["history", "purge", "--yes"], false);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("erased 3 record(s)"), "{}", text(&o));
    let after = store(&ws);
    assert!(!after.contains("FAKE-CANARY") && !after.contains("prompt number"), "text survived the purge");
    assert_eq!(after.lines().count(), 3, "a tombstone stays for each");
    let status = text(&delphi(&home, &["history", "status"], false));
    assert!(status.contains("0 record(s)") && status.contains("3 erased") && status.contains("chain intact"), "{status}");
    // Nothing else under the home holds the text: the whole home is scanned, files of any kind.
    for e in walkdir::WalkDir::new(&home).into_iter().flatten().filter(|e| e.file_type().is_file()) {
        let bytes = std::fs::read(e.path()).unwrap_or_default();
        assert!(!String::from_utf8_lossy(&bytes).contains("FAKE-CANARY prompt"), "{} still holds the prompt text", e.path().display());
    }
}

#[test]
fn delete_erases_one_record_and_purge_since_keeps_the_earlier_ones() {
    let (_t, home, ws) = fixture(3);
    let o = delphi(&home, &["history", "delete", "ex_0002", "--yes"], false);
    assert!(o.status.success(), "{}", text(&o));
    let t = store(&ws);
    assert!(t.contains("number 0") && !t.contains("number 1") && t.contains("number 2"), "only the second record is gone");
    let again = delphi(&home, &["history", "delete", "ex_0002", "--yes"], false);
    assert_eq!(again.status.code(), Some(1));
    assert!(text(&again).contains("already erased"), "{}", text(&again));
    // --since far in the past erases what is left; a bad --since is refused before anything is touched.
    let bad = delphi(&home, &["history", "purge", "--since", "yesterday", "--yes"], false);
    assert_eq!(bad.status.code(), Some(2), "{}", text(&bad));
    assert!(store(&ws).contains("number 0"));
    let o = delphi(&home, &["history", "purge", "--since", "3650d", "--yes"], false);
    assert!(o.status.success() && text(&o).contains("erased 2 record(s)"), "{}", text(&o));
    assert!(!store(&ws).contains("FAKE-CANARY"));
}

#[test]
fn every_history_subcommand_is_refused_inside_an_agent_seat_and_changes_nothing() {
    let (_t, home, ws) = fixture(2);
    let before = store(&ws);
    for args in [&["history", "status"][..], &["history", "off"], &["history", "purge", "--yes"], &["history", "delete", "ex_0001", "--yes"]] {
        let o = delphi(&home, args, true);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", text(&o));
        assert!(text(&o).contains("refused") && text(&o).contains("agent seat"), "{args:?}: {}", text(&o));
    }
    assert_eq!(store(&ws), before);
    assert!(!ws.join("local/history.off").exists(), "a refused `off` wrote no marker");
}

#[test]
fn a_fresh_workspace_reports_history_off_with_no_key_and_doctor_says_so() {
    let (_t, home, ws) = fixture(0);
    let status = text(&delphi(&home, &["history", "status"], false));
    assert!(status.contains("History: off (the default until it is turned on") && status.contains("0 record(s)"), "{status}");
    assert!(!ws.join("local/history").exists() && !ws.join("local/history.key").exists(), "reading the status made nothing");
    let doctor = text(&delphi(&home, &["doctor"], false));
    assert!(doctor.contains("20 history") && doctor.contains("history: off (default)"), "{doctor}");
}

#[test]
fn no_install_question_is_printed_by_init_today() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let o = Command::new(DELPHI).args(["init", "--non-interactive", "--workspace", "w"]).arg("--home").arg(&home).env_remove("DELPHI_HOME").stdin(Stdio::null()).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let out = text(&o);
    assert!(!out.contains("Keep a history") && !out.contains("history"), "init printed something about the history before slice S8: {out}");
    assert!(!home.join("workspaces/w/local/history.consent").exists());
}
