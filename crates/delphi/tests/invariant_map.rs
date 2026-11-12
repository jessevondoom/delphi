//! The map from the authority invariants of `specs/098` §12 to the tests that enforce them (`context/invariant-enforcement.md`; spec 098
//! §13.1 wave 5, slice S9: "every enforced row mapped").
//!
//! Three things fail here: a §12 item with no row (or a row for an item that does not exist), a named test that no longer exists (the file
//! is gone or the function was renamed), and a row whose status does not match what it names (an `enforced` row with no test, a `partly` or
//! `not yet` row with no note). GOVERNANCE §5.6: an invariant counts as enforced only when code enforces it, so the table must not drift
//! away from the tests. Whether a named test checks what the row says is for a reader; this test only keeps the names true.

use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// The numbers of the invariants in spec 098 §12.
fn spec_items(root: &Path) -> BTreeSet<u32> {
    let spec = std::fs::read_dir(root.join("specs")).unwrap().flatten().map(|e| e.path()).find(|p| p.file_name().unwrap().to_string_lossy().starts_with("098-")).expect("spec 098 is in specs/");
    let text = std::fs::read_to_string(spec).unwrap();
    let section = text.split("\n## 12. ").nth(1).expect("spec 098 has a section 12").split("\n## 13. ").next().unwrap();
    let item = Regex::new(r"(?m)^(\d+)\. ").unwrap();
    item.captures_iter(section).map(|c| c[1].parse().unwrap()).collect()
}

#[derive(Debug)]
struct Row {
    item: u32,
    tests: Vec<String>,
    status: String,
    note: String,
}

/// The rows of the spec 098 table: `| 098 §12 item N | invariant | tests | status | note |`.
fn rows(map: &str) -> Vec<Row> {
    let test = Regex::new(r"`([^`\s]+::[A-Za-z0-9_]+)`").unwrap();
    let mut out = Vec::new();
    for line in map.lines().filter(|l| l.starts_with("| 098 §12 item ")) {
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        assert_eq!(cells.len(), 5, "a row has five cells (item, invariant, tests, status, note): {line}");
        // The label is this table's own (not spec text), so plain string handling reads it; spec text is parsed by `delphi_core::specdoc`.
        let item = cells[0].strip_prefix("098 §12 item ").and_then(|n| n.parse::<u32>().ok()).unwrap_or_else(|| panic!("a row label reads `098 §12 item N`: {}", cells[0]));
        out.push(Row { item, tests: test.captures_iter(cells[2]).map(|c| c[1].to_string()).collect(), status: cells[3].to_string(), note: cells[4].to_string() });
    }
    out
}

/// A test written `path::name` exists when the file is there and defines `fn name`.
fn test_exists(root: &Path, named: &str) -> bool {
    let Some((path, name)) = named.rsplit_once("::") else { return false };
    let Ok(text) = std::fs::read_to_string(root.join(path)) else { return false };
    Regex::new(&format!(r"\bfn {}\s*\(", regex::escape(name))).unwrap().is_match(&text)
}

fn map_text(root: &Path) -> String {
    std::fs::read_to_string(root.join("context/invariant-enforcement.md")).unwrap()
}

#[test]
fn every_authority_invariant_of_spec_098_has_exactly_one_row() {
    let root = root();
    let want = spec_items(&root);
    assert!(want.len() >= 18, "spec 098 §12 lists its invariants as a numbered list: {want:?}");
    let have: Vec<u32> = rows(&map_text(&root)).iter().map(|r| r.item).collect();
    let set: BTreeSet<u32> = have.iter().copied().collect();
    assert_eq!(have.len(), set.len(), "an item has two rows: {have:?}");
    let missing: Vec<_> = want.difference(&set).collect();
    let stray: Vec<_> = set.difference(&want).collect();
    assert!(missing.is_empty(), "spec 098 §12 items with no row in context/invariant-enforcement.md: {missing:?}");
    assert!(stray.is_empty(), "rows for items that spec 098 §12 does not have: {stray:?}");
}

#[test]
fn every_test_a_row_names_exists() {
    let root = root();
    let mut gone = Vec::new();
    for r in rows(&map_text(&root)) {
        for t in &r.tests {
            if !test_exists(&root, t) {
                gone.push(format!("item {}: {t}", r.item));
            }
        }
    }
    assert!(gone.is_empty(), "tests named in context/invariant-enforcement.md that do not exist (renamed or removed?): {gone:#?}");
}

#[test]
fn a_status_matches_what_the_row_names_and_says() {
    let root = root();
    for r in rows(&map_text(&root)) {
        let kind = ["enforced", "partly", "not yet"].into_iter().find(|k| r.status == *k).unwrap_or_else(|| panic!("item {}: status `{}` is enforced, partly or not yet", r.item, r.status));
        match kind {
            "enforced" => assert!(!r.tests.is_empty(), "item {}: an enforced row names at least one test", r.item),
            "partly" => assert!(!r.tests.is_empty() && !r.note.is_empty(), "item {}: a partly row names a test and says what is not covered", r.item),
            _ => assert!(!r.note.is_empty(), "item {}: a not-yet row says why nothing tests it", r.item),
        }
    }
}

#[test]
fn the_check_can_fail() {
    // A name that is not there, a file that is not there, and a name without a path are all reported missing; a real one is found.
    let root = root();
    assert!(test_exists(&root, "crates/delphi/tests/invariant_map.rs::the_check_can_fail"));
    assert!(!test_exists(&root, "crates/delphi/tests/invariant_map.rs::a_test_nobody_wrote"));
    assert!(!test_exists(&root, "crates/delphi/tests/no_such_file.rs::the_check_can_fail"));
    assert!(!test_exists(&root, "the_check_can_fail"));
    // A row for an item that does not exist, and an item with no row, are what the first test reports.
    let table = "| 098 §12 item 99 | x | `crates/delphi/tests/invariant_map.rs::the_check_can_fail` | enforced | |\n";
    assert_eq!(rows(table).iter().map(|r| r.item).collect::<Vec<_>>(), vec![99]);
}
