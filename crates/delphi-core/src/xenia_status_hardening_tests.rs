//! X9b hardening of the surfaces: they change nothing, damaged state is named, hostile or huge lists stay small and clean.

use super::*;
use crate::xenia_fixture::*;

fn home(fx: &Fx) -> std::path::PathBuf {
    fx.dir.join("home")
}

fn read(fx: &Fx) -> Status {
    let r = list_stores(&home(fx)).remove(0);
    status(&fx.store, &r, &cfg()).unwrap()
}

fn listing(root: &std::path::Path) -> Vec<(String, u64, u32)> {
    use std::os::unix::fs::PermissionsExt;
    fn walk(p: &std::path::Path, out: &mut Vec<(String, u64, u32)>) {
        let mut es: Vec<_> = std::fs::read_dir(p).unwrap().flatten().collect();
        es.sort_by_key(|e| e.file_name());
        for e in es {
            let m = std::fs::symlink_metadata(e.path()).unwrap();
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

#[test]
fn reading_status_changes_nothing_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    let before = listing(&home(&fx));
    let _ = read(&fx);
    let _ = all(&home(&fx), &cfg());
    assert_eq!(listing(&home(&fx)), before, "a surface writes nothing, creates nothing and changes no mode");
}

#[test]
fn a_loosened_pins_or_journal_file_is_a_failure_with_its_name() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    assert!(!read(&fx).warnings.iter().any(|w| w.id == "state-permissions"), "a private file is fine");
    let pins = fx.store.paths.state.join("pins.toml");
    std::fs::set_permissions(&pins, std::fs::Permissions::from_mode(0o644)).unwrap();
    let w = read(&fx).warnings;
    assert!(w.iter().any(|w| w.id == "state-permissions" && w.severity == Severity::Fail && w.text.starts_with("pins.toml")), "{w:?}");
    std::fs::set_permissions(&pins, std::fs::Permissions::from_mode(0o600)).unwrap();
    let _ = sync_and_record(&fx.store, &cfg());
    std::fs::set_permissions(fx.store.paths.state.join("journal.jsonl"), std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(read(&fx).warnings.iter().any(|w| w.id == "state-permissions" && w.text.starts_with("journal.jsonl")));
}

#[test]
fn a_store_whose_pins_are_damaged_is_reported_not_silently_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let (fx, _) = Fx::installed(tmp.path(), &[]);
    std::fs::write(fx.store.paths.state.join("pins.toml"), b"\xff not toml \x1b[2J").unwrap();
    assert!(list_stores(&home(&fx)).is_empty(), "no longer listed as joined");
    let got = all(&home(&fx), &cfg());
    assert_eq!(got.len(), 1);
    assert!(got[0].1.as_ref().unwrap_err().contains("pins.toml is damaged"), "{:?}", got[0].1);
}

fn synthetic(n: usize) -> Status {
    let evil = "records/memory/\x1b[2J\x1b]0;pwned\x07\u{202e}x.md";
    Status {
        store: StoreRef { org: "acme".into(), store: "main".into(), url: "u".into() },
        head: Some("0123456789abcdef".into()),
        roster: None,
        freshness: None,
        l2: 0,
        l1: 0,
        hidden: (0..n).map(|i| (format!("h{i}\x1b[31m"), "author-withdrawn")).collect(),
        contested: (0..n).map(|i| format!("c{i}\u{202e}")).collect(),
        quarantine: std::iter::once((evil.to_string(), "bad-field")).chain((0..n).map(|i| (format!("records/memory/{i}.md"), "not-tracked"))).collect(),
        last_sync: None,
        tampering: (0..n).map(|i| (format!("records/memory/{i}.md\x1b[1m"), "path-deleted\u{202e}".to_string())).collect(),
        warnings: vec![],
    }
}

#[test]
fn ten_thousand_hostile_entries_print_and_serialize_a_few_clean_lines() {
    let st = synthetic(10_000);
    let text = render(&st, 0);
    assert!(text.lines().count() < 260, "{} lines", text.lines().count());
    assert!(text.contains("and 9951 more"), "{}", text.lines().filter(|l| l.contains("more")).collect::<Vec<_>>().join("|"));
    assert!(!text.chars().any(|c| (c.is_control() && c != '\n') || ('\u{202a}'..='\u{202e}').contains(&c) || ('\u{2066}'..='\u{2069}').contains(&c)), "no control or bidi character reaches the terminal");
    let j = to_json(&st);
    for k in ["hidden", "contested", "quarantine", "tampering"] {
        assert_eq!(j[k].as_array().unwrap().len(), JSON_CAP, "{k}");
    }
    assert_eq!(j["quarantine_total"], 10_001);
    let s = j.to_string();
    assert!(!s.contains("\\u001b") && !s.contains('\u{202e}') && !s.contains("\\u202e"), "the JSON is clean too: {}", &s[..200.min(s.len())]);
}
