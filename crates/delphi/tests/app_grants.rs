//! The app's grant table against its gate file (`specs/120` §4, AC-6): every method the app socket can reach is a row of
//! `delphi_core::app_keys::GRANTS`, and the rows are the ones `app_grants.toml` lists, grant by grant, in order. A method
//! added to the table and not to the file, or the other way round, fails here.
//!
//! The file is a gate path (`merge-gates.toml`): a pull request that changes it cannot be merged on a chat quote. It is
//! added by a pull request of its own, which the operator merges at a terminal. Until that one has merged, the file is
//! absent and `merge-gates.toml` does not name it; this test then holds the table to the copy below, so the table is
//! pinned either way. Once `merge-gates.toml` names the file, its absence fails.

use delphi_core::app_keys::{all_methods, GRANTS, PAIRING_GRANTS, REQUEST_METHODS};
use std::path::{Path, PathBuf};

const FILE: &str = "crates/delphi/tests/app_grants.toml";

/// The table as the spec approved it: `read` is the narrator's twelve reads and the encyclopedia's eleven (spec 117 §3);
/// `request` is the four methods of spec 120 §4.2. A row is added here, and in the gate file, only with the approved spec
/// or amendment that names it (§5 item 1).
const EXPECTED: [(&str, &[&str]); 2] = [
    (
        "read",
        &[
            "session.ping",
            "events.subscribe",
            "session.snapshot",
            "observe.snapshot",
            "observe.state",
            "kb.search",
            "kb.get",
            "usage.get",
            "budget.get",
            "approval.list",
            "permissions.get",
            "ratify.list",
            "pages.list",
            "pages.get",
            "pages.versions",
            "pages.links",
            "topics.list",
            "topics.get",
            "library.source",
            "library.claim",
            "memory.roles",
            "memory.role",
            "changes.since",
        ],
    ),
    ("request", &["app.research_continue", "app.research_ask", "app.page_rebuild", "app.page_dispute"]),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn table() -> Vec<(String, Vec<String>)> {
    GRANTS.iter().map(|g| (g.name.to_string(), g.methods.iter().map(|m| m.to_string()).collect())).collect()
}

/// The gate file's grants, in its order: `[[grant]]` tables with a `name` and a list of `methods`, and nothing else.
fn listed(text: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let doc: toml::Table = text.parse().map_err(|e| format!("{FILE} is not TOML: {e}"))?;
    if doc.keys().any(|k| k != "grant") {
        return Err(format!("{FILE} holds a key other than `grant`"));
    }
    let grants = doc.get("grant").and_then(|g| g.as_array()).ok_or(format!("{FILE} lists no [[grant]]"))?;
    let mut out = Vec::new();
    for g in grants {
        let t = g.as_table().ok_or("a grant is a table")?;
        if t.keys().any(|k| k != "name" && k != "methods") {
            return Err("a grant holds a `name` and its `methods`, nothing else".into());
        }
        let name = t.get("name").and_then(|n| n.as_str()).ok_or("a grant has a name")?;
        let methods = t.get("methods").and_then(|m| m.as_array()).ok_or("a grant lists its methods")?;
        let methods: Option<Vec<String>> = methods.iter().map(|m| m.as_str().map(String::from)).collect();
        out.push((name.to_string(), methods.ok_or("a method is a string")?));
    }
    Ok(out)
}

/// Does `merge-gates.toml` name the gate file (by itself or by the pattern `app_grants.*`)?
fn gated() -> bool {
    let gates = std::fs::read_to_string(root().join("merge-gates.toml")).expect("merge-gates.toml is at the repository root");
    gates.lines().filter(|l| !l.trim_start().starts_with('#')).any(|l| l.contains("crates/delphi/tests/app_grants."))
}

#[test]
fn every_method_the_app_socket_can_reach_is_listed_under_a_grant_in_the_gate_file() {
    let code = table();
    let expected: Vec<(String, Vec<String>)> = EXPECTED.iter().map(|(n, ms)| (n.to_string(), ms.iter().map(|m| m.to_string()).collect())).collect();
    assert_eq!(code, expected, "the grant table in delphi-core changed; a row is added only with an approved spec, this test's copy and {FILE}");
    match std::fs::read_to_string(root().join(FILE)) {
        Ok(text) => {
            let file = listed(&text).unwrap();
            assert_eq!(file, code, "{FILE} and `delphi_core::app_keys::GRANTS` differ: a method reachable on the app socket must be listed in the gate file under its grant, and the file must list nothing the code does not have");
        }
        Err(_) => assert!(!gated(), "merge-gates.toml names {FILE}, so the file must exist"),
    }
    // The shape of the table, whatever lists it: no method twice, the acting grant is exactly the four, and pairing
    // gives only grants that exist.
    let all = all_methods();
    let mut once: Vec<&str> = all.iter().map(|(_, m)| *m).collect();
    once.sort();
    once.dedup();
    assert_eq!((all.len(), once.len()), (27, 27));
    assert_eq!(GRANTS[1].methods, &REQUEST_METHODS[..]);
    assert!(PAIRING_GRANTS.iter().all(|p| GRANTS.iter().any(|g| g.name == *p)));
}

/// AC-6's second half, shown on the comparison itself: a file that lacks a method the code has, lists one the code lacks,
/// puts a method under another grant, or is not of the shape, is not equal to the table.
#[test]
fn a_gate_file_that_differs_from_the_table_is_found() {
    let render = |t: &[(String, Vec<String>)]| t.iter().map(|(n, ms)| format!("[[grant]]\nname = \"{n}\"\nmethods = [{}]\n", ms.iter().map(|m| format!("\"{m}\"")).collect::<Vec<_>>().join(", "))).collect::<Vec<_>>().join("\n");
    let code = table();
    assert_eq!(listed(&render(&code)).unwrap(), code, "the table written as the file reads back as the table");
    let mut missing = code.clone();
    missing[1].1.pop();
    let mut extra = code.clone();
    extra[1].1.push("approval.resolve".into());
    let mut moved = code.clone();
    let m = moved[1].1.remove(0);
    moved[0].1.push(m);
    let mut renamed = code.clone();
    renamed[1].0 = "answer".into();
    for (what, other) in [("a method the file lacks", missing), ("a method only the file has", extra), ("a method under another grant", moved), ("a grant by another name", renamed)] {
        assert_ne!(listed(&render(&other)).unwrap(), code, "{what}");
    }
    for bad in ["", "grant = 3", "[[grant]]\nname = \"read\"", "[[grant]]\nname = \"read\"\nmethods = [1]", "[[grant]]\nname = \"read\"\nmethods = []\nwildcard = true", "[other]\nx = 1\n[[grant]]\nname = \"read\"\nmethods = []"] {
        assert!(listed(bad).is_err(), "{bad:?}");
    }
}
