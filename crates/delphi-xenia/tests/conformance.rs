//! X10: the conformance map is checked, not trusted. Every numbered test of `specs/011` §10 and `specs/012` §10 must
//! have a row in `missions/2026-10-23-foundational-design/xenia-x10-conformance.md`, and every test name in a row must
//! be a real `#[test]` function in the file the row names. Add a numbered test to either spec and this goes red until
//! a test and a row exist.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The numbers of the list items of a spec's `## 10. Verification Plan` section.
fn spec_numbers(spec: &str) -> BTreeSet<u32> {
    let text = std::fs::read_to_string(repo().join(spec)).unwrap();
    let start = text.find("\n## 10. Verification Plan").unwrap_or_else(|| panic!("{spec} has no section 10"));
    let rest = &text[start + 1..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |i| i + 3);
    rest[..end]
        .lines()
        .filter_map(|l| {
            let (n, tail) = l.split_once(". ")?;
            (tail.starts_with("**") && n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty()).then(|| n.parse().unwrap())
        })
        .collect()
}

/// `spec prefix -> number -> [(path, name)]` from the table rows.
fn map_rows() -> BTreeMap<String, Vec<(String, String)>> {
    let doc = std::fs::read_to_string(repo().join("missions/2026-10-23-foundational-design/xenia-x10-conformance.md")).unwrap();
    let mut rows = BTreeMap::new();
    for line in doc.lines() {
        let Some(rest) = line.strip_prefix("| ") else { continue };
        let key = rest.split(" | ").next().unwrap_or("");
        if !(key.starts_with("011.") || key.starts_with("012.")) {
            continue;
        }
        let mut pairs = Vec::new();
        for token in line.split('`').skip(1).step_by(2) {
            let (path, name) = token.split_once("::").unwrap_or_else(|| panic!("row {key}: `{token}` is not path::name"));
            pairs.push((path.to_string(), name.to_string()));
        }
        assert!(rows.insert(key.to_string(), pairs).is_none(), "row {key} appears twice");
    }
    rows
}

/// Is there `fn <name>(` in `file`, with a `#[test]` among the six lines before it?
fn is_test_in(file: &Path, name: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(file) else { return false };
    let lines: Vec<&str> = text.lines().collect();
    lines.iter().enumerate().any(|(i, l)| {
        let t = l.trim_start();
        let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
        t.strip_prefix("fn ").and_then(|r| r.strip_prefix(name)).is_some_and(|r| r.starts_with('('))
            && lines[i.saturating_sub(6)..i].iter().any(|p| p.trim() == "#[test]")
    })
}

#[test]
fn every_numbered_test_in_the_two_specs_has_a_row_and_every_row_names_real_tests() {
    let rows = map_rows();
    let mut problems = Vec::new();
    for (spec, prefix) in [("specs/011-xenia-trust-model.md", "011"), ("specs/012-xenia-record-schema.md", "012")] {
        let numbers = spec_numbers(spec);
        assert!(numbers.len() >= 23 && numbers.iter().copied().eq(1..=numbers.len() as u32), "{spec}: the numbered list was misread: {numbers:?}");
        for n in &numbers {
            let key = format!("{prefix}.{n}");
            match rows.get(&key) {
                None => problems.push(format!("{key}: no row in the conformance map")),
                Some(p) if p.is_empty() => problems.push(format!("{key}: the row names no test")),
                Some(p) => {
                    for (path, name) in p {
                        if !is_test_in(&repo().join("crates").join(path), name) {
                            problems.push(format!("{key}: `{path}::{name}` is not a #[test] function"));
                        }
                    }
                }
            }
        }
        for key in rows.keys().filter(|k| k.starts_with(prefix)) {
            let n: u32 = key[4..].parse().unwrap();
            if !numbers.contains(&n) {
                problems.push(format!("{key}: the spec has no such numbered test"));
            }
        }
    }
    assert!(problems.is_empty(), "the conformance map is out of step with the specs:\n{}", problems.join("\n"));
}

#[test]
fn the_checker_can_fail() {
    // The checker is only worth something if it notices: a missing name, a missing file, a non-test function.
    let here = repo().join("crates/delphi-xenia/tests/conformance.rs");
    assert!(is_test_in(&here, "the_checker_can_fail"));
    assert!(!is_test_in(&here, "no_such_test_exists_anywhere"));
    assert!(!is_test_in(&here, "is_test_in"), "a helper is not a test");
    assert!(!is_test_in(&repo().join("crates/delphi-xenia/tests/no-such-file.rs"), "the_checker_can_fail"));
    assert_eq!(spec_numbers("specs/011-xenia-trust-model.md").len(), 31);
    assert_eq!(spec_numbers("specs/012-xenia-record-schema.md").len(), 23);
}

/// Every `#[test]` function name under `crates/`.
fn all_test_names() -> BTreeSet<String> {
    fn walk(dir: &Path, out: &mut BTreeSet<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "target" || n == "node_modules") {
                    continue;
                }
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                let lines: Vec<&str> = text.lines().collect();
                for (i, l) in lines.iter().enumerate() {
                    let t = l.trim_start();
                    let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
                    if let Some(rest) = t.strip_prefix("fn ") {
                        if let Some(name) = rest.split('(').next() {
                            if lines[i.saturating_sub(6)..i].iter().any(|p| p.trim() == "#[test]") {
                                out.insert(name.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(&repo().join("crates"), &mut out);
    out
}

#[test]
fn every_test_the_red_team_checklist_names_exists() {
    let doc = std::fs::read_to_string(repo().join("missions/2026-10-23-foundational-design/xenia-x10-redteam-checklist.md")).unwrap();
    let names = all_test_names();
    let mut cited = 0;
    let mut missing = Vec::new();
    for token in doc.split('`').skip(1).step_by(2) {
        for part in token.split([';', ',', ' ']) {
            let name = part.rsplit("::").next().unwrap_or("").trim_matches(|c| c == '.' || c == '(' || c == ')');
            let identifier = !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if identifier && name.matches('_').count() >= 3 {
                cited += 1;
                if !names.contains(name) {
                    missing.push(name.to_string());
                }
            }
        }
    }
    assert!(cited > 100, "the checklist cites its tests by name ({cited})");
    assert!(missing.is_empty(), "the red-team checklist names tests that do not exist: {missing:?}");
}
