use super::*;
use crate::history_record::{append, Appended, Draft, Observed, Outcome, OutcomeValue, PromptIn, Provenance, Reply, Trigger};
use crate::history_redact::Typing;

const PROJECT: &str = "home";
const NOW: &str = "2026-11-20T00:00:00Z";

fn on() -> Settings {
    Settings { enabled: true, ..Settings::default() }
}

fn draft(at: &str, prompt: &str) -> Draft {
    Draft {
        trigger: Trigger::ResearchRun,
        mission: "m1".into(),
        project: PROJECT.into(),
        at: at.into(),
        prompt: PromptIn::Observed { text: prompt.into(), marked: vec![], typing: Typing::Typed },
        reply: Some(Reply { asked: format!("asked: {prompt}"), did: format!("did: {prompt}"), open: String::new() }),
        outcome: Outcome { value: OutcomeValue::Done, provenance: Provenance::Observed },
        observed: Observed::default(),
    }
}

fn write(ws: &Path, at: &str, prompt: &str) -> rec::Record {
    match append(ws, &on(), draft(at, prompt)) {
        Appended::Written(r) => *r,
        other => panic!("expected a record, got {other:?}"),
    }
}

fn store(ws: &Path) -> String {
    std::fs::read_to_string(rec::store_path(ws, PROJECT)).unwrap_or_default()
}

fn three(ws: &Path) {
    write(ws, "2026-11-10T00:00:00Z", "CANARY-ONE alpha");
    write(ws, "2026-11-11T00:00:00Z", "CANARY-TWO beta");
    write(ws, "2026-11-12T00:00:00Z", "CANARY-THREE gamma");
}

fn unix(s: &str) -> i64 {
    delphi_core::time::parse_rfc3339(s).unwrap()
}

// ---------------------------------------------------------------------------------------------------------------------------
// off

#[test]
fn off_stops_new_records_at_once_and_deletes_nothing() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    let before = store(ws.path());
    let marker = off(ws.path(), None).unwrap();
    assert_eq!(marker, rec::global_off_path(ws.path()));
    assert_eq!(append(ws.path(), &on(), draft("2026-11-13T00:00:00Z", "CANARY-FOUR")), Appended::SwitchedOff);
    assert_eq!(store(ws.path()), before, "off deletes nothing and writes nothing");
    assert_eq!(rec::read(ws.path(), PROJECT).unwrap().counted().count(), 3, "the records are still there and still verify");
    std::fs::remove_file(&marker).unwrap();
    assert!(matches!(append(ws.path(), &on(), draft("2026-11-13T00:00:00Z", "CANARY-FOUR")), Appended::Written(_)), "removing the marker is the way back");
}

#[test]
fn off_for_one_project_leaves_the_others_writing() {
    let ws = tempfile::tempdir().unwrap();
    let marker = off(ws.path(), Some("alpha")).unwrap();
    assert!(marker.ends_with("local/projects/alpha/history.off"));
    let mut d = draft("2026-11-13T00:00:00Z", "x");
    d.project = "alpha".into();
    assert_eq!(append(ws.path(), &on(), d), Appended::ProjectOff);
    assert!(matches!(append(ws.path(), &on(), draft("2026-11-13T00:00:00Z", "x")), Appended::Written(_)));
    assert!(off(ws.path(), Some("../escape")).is_err(), "a project id is checked, so the marker cannot be written elsewhere");
}

// ---------------------------------------------------------------------------------------------------------------------------
// purge and delete: tombstones, the chain, zero residue

#[test]
fn purge_all_leaves_tombstones_the_chain_verifies_through_them_and_no_text_remains() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    let originals: Vec<String> = rec::read(ws.path(), PROJECT).unwrap().records.iter().map(|c| c.record.hash.clone()).collect();
    let done = erase(ws.path(), PROJECT, Which::All, NOW).unwrap();
    assert_eq!(done, Erased { records: 3, voided: 0 });
    let text = store(ws.path());
    assert_eq!(text.lines().count(), 3, "a tombstone takes the place of each record: the line count and the ids are unchanged");
    for needle in ["CANARY", "alpha", "beta", "gamma", "asked:", "did:"] {
        assert!(!text.contains(needle), "{needle} survived the purge");
    }
    let r = rec::read(ws.path(), PROJECT).unwrap();
    assert!(r.records.is_empty() && r.broken_at.is_none(), "the chain verifies through the tombstones: {r:?}");
    let kept: Vec<(&str, &str)> = r.tombstones.iter().map(|t| (t.id.as_str(), t.hash.as_str())).collect();
    assert_eq!(kept, vec![("ex_0001", originals[0].as_str()), ("ex_0002", originals[1].as_str()), ("ex_0003", originals[2].as_str())], "each keeps its id and its keyed hash");
    assert!(r.tombstones.iter().all(|t| t.verified && !t.at.is_empty() && t.removed_at == NOW));
    // The next record links to the last tombstone's hash and takes the next id.
    let next = write(ws.path(), "2026-11-14T00:00:00Z", "CANARY-FOUR delta");
    assert_eq!((next.id.as_str(), next.chain.as_str()), ("ex_0004", originals[2].as_str()));
    let r = rec::read(ws.path(), PROJECT).unwrap();
    assert_eq!((r.broken_at, r.counted().count(), r.tombstones.len()), (None, 1, 3));
}

#[test]
fn purge_since_erases_from_the_time_on_and_keeps_the_earlier_records() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    // The boundary is inclusive: the second record is at exactly this time.
    let done = erase(ws.path(), PROJECT, Which::Since(unix("2026-11-11T00:00:00Z")), NOW).unwrap();
    assert_eq!(done.records, 2);
    let text = store(ws.path());
    assert!(text.contains("CANARY-ONE") && !text.contains("CANARY-TWO") && !text.contains("CANARY-THREE"));
    let r = rec::read(ws.path(), PROJECT).unwrap();
    assert_eq!((r.broken_at, r.records.len(), r.tombstones.len()), (None, 1, 2));
    // One second later erases only the last (here nothing is left to erase at that time).
    let again = erase(ws.path(), PROJECT, Which::Since(unix("2026-11-12T00:00:01Z")), NOW).unwrap();
    assert_eq!(again, Erased::default(), "an erase that finds nothing changes nothing");
}

#[test]
fn delete_erases_one_record_and_says_when_it_is_missing_or_already_gone() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    assert_eq!(erase(ws.path(), PROJECT, Which::Id("ex_0002"), NOW).unwrap().records, 1);
    let text = store(ws.path());
    assert!(text.contains("CANARY-ONE") && !text.contains("CANARY-TWO") && text.contains("CANARY-THREE"));
    let r = rec::read(ws.path(), PROJECT).unwrap();
    assert_eq!((r.broken_at, r.records.len(), r.tombstones.len()), (None, 2, 1), "the records on both sides still verify");
    assert!(erase(ws.path(), PROJECT, Which::Id("ex_0002"), NOW).unwrap_err().contains("already erased"));
    assert!(erase(ws.path(), PROJECT, Which::Id("ex_0009"), NOW).unwrap_err().contains("no record ex_0009"));
}

#[test]
fn erasing_a_tampered_record_does_not_launder_the_chain() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    let path = rec::store_path(ws.path(), PROJECT);
    std::fs::write(&path, store(ws.path()).replacen("CANARY-TWO beta", "CANARY-TWO forged", 1)).unwrap();
    assert_eq!(rec::read(ws.path(), PROJECT).unwrap().broken_at, Some(1), "the edit is caught before the purge");
    erase(ws.path(), PROJECT, Which::All, NOW).unwrap();
    let text = store(ws.path());
    assert!(!text.contains("forged") && !text.contains("CANARY"));
    let r = rec::read(ws.path(), PROJECT).unwrap();
    assert_eq!(r.broken_at, Some(1), "the tombstone of a record that had failed is itself a break: erasing text does not launder it");
    assert_eq!(r.counted().count(), 0);
}

#[test]
fn a_line_that_cannot_be_read_is_voided_so_no_text_hides_in_it() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    let path = rec::store_path(ws.path(), PROJECT);
    let mut t = store(ws.path());
    t.push_str("this is not json CANARY-HIDDEN\n");
    std::fs::write(&path, t).unwrap();
    let done = erase(ws.path(), PROJECT, Which::All, NOW).unwrap();
    assert_eq!(done, Erased { records: 3, voided: 1 });
    assert!(!store(ws.path()).contains("CANARY"));
    assert_eq!(rec::read(ws.path(), PROJECT).unwrap().broken_at, Some(3), "the void tombstone keeps the break at the same line");
}

#[test]
fn a_tombstone_that_was_edited_breaks_the_chain() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    erase(ws.path(), PROJECT, Which::Id("ex_0002"), NOW).unwrap();
    let path = rec::store_path(ws.path(), PROJECT);
    std::fs::write(&path, store(ws.path()).replacen(NOW, "2026-01-01T00:00:00Z", 1)).unwrap();
    assert_eq!(rec::read(ws.path(), PROJECT).unwrap().broken_at, Some(1), "the seal covers the removal time");
}

#[test]
fn an_erase_with_no_store_changes_nothing_and_makes_no_key() {
    let ws = tempfile::tempdir().unwrap();
    assert_eq!(erase(ws.path(), PROJECT, Which::All, NOW).unwrap(), Erased::default());
    assert!(!ws.path().join("local").exists(), "nothing is made under the home");
    assert!(erase(ws.path(), PROJECT, Which::Id("ex_0001"), NOW).is_err());
    assert!(erase(ws.path(), "../x", Which::All, NOW).is_err());
}

#[test]
fn the_store_file_stays_owner_only_after_a_rewrite() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    erase(ws.path(), PROJECT, Which::Id("ex_0001"), NOW).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(rec::store_path(ws.path(), PROJECT)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    assert!(!rec::store_dir(ws.path(), PROJECT).join(".records.jsonl.tmp").exists(), "the temporary file is gone");
}

// ---------------------------------------------------------------------------------------------------------------------------
// status and doctor leak no text

#[test]
fn status_shows_counts_and_chain_state_and_none_of_the_text() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    erase(ws.path(), PROJECT, Which::Id("ex_0002"), NOW).unwrap();
    let o = overview(ws.path(), &on(), unix("2026-11-13T00:00:00Z"));
    assert_eq!(o.rows.len(), 1);
    let r = &o.rows[0];
    assert_eq!((r.project.as_str(), r.records, r.verified, r.tombstones, r.chain, r.past_age), ("home", 2, 2, 1, Chain::Intact, 0));
    assert_eq!(r.last_at.as_deref(), Some("2026-11-12T00:00:00Z"));
    let text = status_lines(&o).join("\n");
    for needle in ["CANARY", "alpha", "gamma", "asked", "did:"] {
        assert!(!text.contains(needle), "status printed record text: {needle}");
    }
    assert!(text.contains("History: on") && text.contains("2 record(s)") && text.contains("1 erased") && text.contains("chain intact") && text.contains("Key: present"), "{text}");
    let (problem, line) = doctor_line(&o);
    assert!(!problem && line.contains("2 record(s), 1 erased"), "{line}");
}

#[test]
fn status_counts_records_past_the_age_budget_and_deletes_none() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    let s = Settings { retention_days: 1, ..on() };
    let o = overview(ws.path(), &s, unix("2026-11-20T00:00:00Z"));
    assert_eq!(o.rows[0].past_age, 3);
    let text = status_lines(&o).join("\n");
    assert!(text.contains("Past the age budget (1 days): 3 record(s). Nothing is deleted at that age"), "{text}");
    assert_eq!(rec::read(ws.path(), PROJECT).unwrap().counted().count(), 3);
}

#[test]
fn status_and_doctor_warn_on_a_broken_chain_and_an_unreadable_key_and_repair_nothing() {
    let ws = tempfile::tempdir().unwrap();
    three(ws.path());
    let path = rec::store_path(ws.path(), PROJECT);
    let tampered = store(ws.path()).replacen("CANARY-TWO beta", "CANARY-TWO forged", 1);
    std::fs::write(&path, &tampered).unwrap();
    let o = overview(ws.path(), &on(), unix(NOW));
    assert_eq!(o.rows[0].chain, Chain::Broken(2));
    assert_eq!((o.rows[0].records, o.rows[0].verified), (3, 1), "the broken record and every later one are left out of the count");
    let (problem, line) = doctor_line(&o);
    assert!(problem && line.contains("broken at line 2"), "{line}");
    assert!(status_lines(&o).join("\n").contains("CHAIN BROKEN at line 2"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), tampered, "status repairs nothing");
    std::fs::write(crate::history_pointer::key_path(ws.path()), "not hex").unwrap();
    let o = overview(ws.path(), &on(), unix(NOW));
    assert_eq!(o.key, KeyState::Unreadable);
    assert!(doctor_line(&o).0);
}

#[test]
fn a_fresh_install_reads_off_with_no_key_and_no_files() {
    let ws = tempfile::tempdir().unwrap();
    let o = overview(ws.path(), &Settings::default(), unix(NOW));
    assert!(o.rows.is_empty() && !o.enabled && o.key == KeyState::Absent);
    let text = status_lines(&o).join("\n");
    assert!(text.contains("History: off (the default until it is turned on; nothing is recorded)") && text.contains("0 record(s)") && text.contains("Timeline: on"), "{text}");
    assert_eq!(doctor_line(&o), (false, "history: off (default); 0 record(s), 0 erased; chain intact; no key yet".to_string()));
    assert!(!ws.path().join("local").exists(), "reading the status makes nothing");
}

#[test]
fn a_switch_off_reads_as_off_in_status_even_when_the_setting_is_on() {
    let ws = tempfile::tempdir().unwrap();
    off(ws.path(), None).unwrap();
    let text = status_lines(&overview(ws.path(), &on(), unix(NOW))).join("\n");
    assert!(text.contains("History: off (switched off with `delphi history off`"), "{text}");
}

// ---------------------------------------------------------------------------------------------------------------------------
// the timeline switch (§7 item 8)

#[test]
fn the_timeline_switch_turns_the_report_off_without_reading_anything() {
    let ws = tempfile::tempdir().unwrap();
    let missing = ws.path().join("no-such-mission");
    assert!(crate::history_report::timeline_of_if(&missing, false).unwrap().is_none(), "off: nothing is read");
    assert!(Settings::default().timeline, "the timeline is on by default, and stays when records are off");
}
