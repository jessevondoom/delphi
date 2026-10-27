//! X10: the static half of the no-fetch rule (spec 011 test 23, spec 012 test 11). Pointers are display-only in
//! Phase 1: no code in the Xenia crates may open a network connection of its own. The only outbound path is `git`
//! to the store URL, spawned as a child process by `xenia_store.rs`; no pointer target reaches it.
//!
//! The dynamic half is `a_pointer_is_never_fetched_a_listener_on_its_target_sees_no_connection` in
//! `delphi-core/src/xenia_sync_tests.rs` (a live loopback listener named by a pointer sees no connection through a
//! full sync). This scan catches the other way to break the rule: someone adds an HTTP client and a resolver.

use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &["TcpStream", "TcpListener", "UdpSocket", "ToSocketAddrs", "std::net", "reqwest", "ureq", "hyper::", "curl::", "isahc", "surf::", "attohttpc", "tokio::net"];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn production_text(p: &Path) -> String {
    let text = std::fs::read_to_string(p).unwrap();
    // Everything before the first test module is production code; test files are whole-file tests.
    match text.find("#[cfg(test)]") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}

fn is_test_file(name: &str) -> bool {
    name.ends_with("_tests.rs") || name == "xenia_fixture.rs" || name == "xenia_testkit.rs" || name == "testkit.rs" || name == "testkit_records.rs"
}

#[test]
fn no_xenia_source_names_a_network_api() {
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in ["crates/delphi-xenia/src", "crates/delphi-core/src"] {
        for e in std::fs::read_dir(repo().join(dir)).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let xenia = dir.ends_with("delphi-xenia/src") || name.starts_with("xenia_") || name.starts_with("bundle_xenia");
            if xenia && name.ends_with(".rs") && !is_test_file(&name) {
                files.push(p);
            }
        }
    }
    files.push(repo().join("crates/delphi/src/xenia_cmd.rs"));
    files.push(repo().join("crates/delphi/src/xenia_admin_cmd.rs"));
    assert!(files.len() > 25, "the scan found the Xenia sources ({})", files.len());
    let mut hits = Vec::new();
    for f in &files {
        let text = production_text(f);
        for word in FORBIDDEN {
            if text.contains(word) {
                hits.push(format!("{} names {word}", f.display()));
            }
        }
    }
    assert!(hits.is_empty(), "Xenia code must not open connections of its own (pointers are never fetched in Phase 1): {hits:#?}");
}

#[test]
fn the_pure_crate_has_no_networking_dependency() {
    let toml = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let deps = toml.split("[dev-dependencies]").next().unwrap();
    for bad in ["reqwest", "ureq", "hyper", "tokio", "curl", "isahc", "attohttpc", "surf", "socket2", "mio"] {
        assert!(!deps.contains(bad), "delphi-xenia depends on {bad}");
    }
}
