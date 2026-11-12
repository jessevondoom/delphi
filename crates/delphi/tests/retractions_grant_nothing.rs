//! `specs/101` §12 items 3, 6 and 9: a retraction mark grants nothing, the retraction check opens no connection, and nothing is deleted.
//!
//! A `retracted` mark is a record the daemon writes from a local file and a label the operator reads. It only takes things away (a source
//! stops counting as an origin; a claim only it supports reads contested); no promotion, no trust, no tool access, no approval and no
//! queue order reads it as a reason to give anything. Like `search_log_grants_nothing.rs` the test is a net, not a proof: it reads the
//! non-test source of every crate and fails when a file not on the short list below names the retraction machinery, so a new reader has
//! to be added by a reviewer, who then has to ask the question this test exists for: does it decide anything?

use regex::Regex;
use std::path::{Path, PathBuf};

/// Where the machinery may be named, and why each file may. Nothing here decides a permission, a tier, a taint level or an order.
const READERS: &[(&str, &str)] = &[
    ("crates/delphi-engine/src/retractions.rs", "defines the snapshot, its reader, its index and the policy; opens no connection"),
    ("crates/delphi-engine/src/library_retraction.rs", "looks a source's DOI up in the snapshot and writes or lifts the mark; only downgrades"),
    ("crates/delphi-engine/src/library.rs", "holds the policy, carries a mark across a new version of a capture, and checks a source after it is stored"),
    ("crates/delphi-engine/src/library_claims.rs", "a source with a dropping mark is not an origin; a retracted source contests a claim nothing else supports"),
    ("crates/delphi-engine/src/library_recheck.rs", "checks the source again after the 30-day re-check"),
    ("crates/delphi-engine/src/digest_review.rs", "offers the operator a line saying a source was marked; keep or dismiss, nothing else"),
    ("crates/delphi-engine/src/research_run.rs", "prints the snapshot's state in the coverage report"),
    ("crates/delphi-engine/src/research_host.rs", "gives the daemon's capture the policy"),
    ("crates/delphi-engine/src/service.rs", "loads the policy from the setting and passes the report line"),
    ("crates/delphi-engine/src/engine.rs", "holds the two settings"),
    ("crates/delphi-engine/src/boot.rs", "parses the two settings"),
    ("crates/delphi/src/research_cmd.rs", "imports a file the operator names and prints the snapshot's state"),
    ("crates/delphi/src/doctor.rs", "warns when the snapshot is missing or old"),
    ("crates/delphi/src/main.rs", "registers `delphi research retractions` and its help text"),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            if !matches!(p.file_name().and_then(|n| n.to_str()), Some("target" | "tests" | "node_modules")) {
                rust_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") && !p.file_stem().and_then(|n| n.to_str()).is_some_and(|n| n == "tests" || n.ends_with("_tests")) {
            out.push(p);
        }
    }
}

/// The text before the file's inline test module (the last `mod tests`); test code may name the machinery.
fn production(src: &str) -> &str {
    let cut = src.rfind("\n#[cfg(test)]\nmod tests").or_else(|| src.rfind("\nmod tests")).unwrap_or(src.len());
    &src[..cut]
}

/// Code only: comment lines may name it.
fn code(src: &str) -> String {
    production(src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

fn pattern() -> Regex {
    Regex::new(r#"crate::retractions|retractions::|check_retraction|check_all_retractions|retracted_sources|drops_origin|live_origin|\["retraction"\]|with_retractions|\.retractions\b|retractions_stale_days|source_retraction_changed|Kind::Retracted|Kind::ExpressionOfConcern|Kind::Corrected"#).unwrap()
}

#[test]
fn no_file_outside_the_list_names_the_retraction_machinery() {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(files.len() > 100, "the scan found {} files; it is looking in the wrong place", files.len());
    let pat = pattern();
    let (mut stray, mut seen) = (Vec::new(), Vec::<String>::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if pat.is_match(&code(&std::fs::read_to_string(f).unwrap())) {
            seen.push(rel.clone());
            if !READERS.iter().any(|(p, _)| *p == rel) {
                stray.push(rel);
            }
        }
    }
    assert!(stray.is_empty(), "these files name the retraction machinery but are not on the list in {}: {stray:?}", file!());
    let gone: Vec<&str> = READERS.iter().map(|(p, _)| *p).filter(|p| !seen.iter().any(|s| s == p)).collect();
    assert!(gone.is_empty(), "these files are on the list but no longer name it; remove them from the list: {gone:?}");
}

#[test]
fn a_line_that_names_it_never_also_names_a_permission_a_tier_or_an_approval() {
    // A word list is crude, which is why the list above is short and each entry says what it does; this catches the careless edit.
    let authority = Regex::new(r"(?i)\b(approve[sd]?|approval|permit|permission|grant|taint|tier|trust_level|trusted|allow(ed)?_tools?|sandbox|elevate|promote|ratif|validated?)\b").unwrap();
    let pat = pattern();
    let root = root();
    let mut bad = Vec::new();
    for (rel, _) in READERS {
        let src = std::fs::read_to_string(root.join(rel)).unwrap();
        for (n, line) in production(&src).lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || !pat.is_match(line) {
                continue;
            }
            if authority.is_match(line) {
                bad.push(format!("{rel}:{}: {}", n + 1, t.chars().take(120).collect::<String>()));
            }
        }
    }
    assert!(bad.is_empty(), "a line names the retraction machinery beside an authority word:\n{}", bad.join("\n"));
}

#[test]
fn the_two_modules_that_decide_it_open_no_connection_and_delete_nothing() {
    // `specs/101` §12 item 3 (no outside call) and item 9 (nothing deleted). The snapshot module and the module that writes the mark use no
    // network client, no fetcher and no deletion of a source, a capture or a claim.
    let root = root();
    let net = Regex::new(r"TcpStream|UdpSocket|reqwest|hyper::|ureq|curl|fact_fetch|fetch_snapshot|add_url|Transport|\bconnect\(|FetchPolicy").unwrap();
    let delete = Regex::new(r"remove_dir_all|remove_file|\.purge\(|fn purge|delete_claim|retire\(").unwrap();
    for rel in ["crates/delphi-engine/src/retractions.rs", "crates/delphi-engine/src/library_retraction.rs"] {
        let c = code(&std::fs::read_to_string(root.join(rel)).unwrap());
        assert!(!net.is_match(&c), "{rel} uses a network client or the fetcher: a retraction lookup never leaves the machine");
        let hit = delete.find(&c).map(|m| m.as_str().to_string());
        assert!(hit.is_none(), "{rel} deletes something ({hit:?}): a retraction never deletes");
    }
}

#[test]
fn the_mark_is_only_ever_written_by_the_module_that_checks_a_snapshot() {
    // The key `"retraction"` is assigned in one place: `check_retraction` in `library_retraction.rs` (and carried, unchanged, across a new
    // version in `library.rs`). Nothing else invents a mark, so a mark always comes from a snapshot the operator imported.
    let root = root();
    let assign = Regex::new(r#"\["retraction"\]\s*=[^=]|insert\("retraction""#).unwrap();
    let mut writers = Vec::new();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if assign.is_match(&code(&std::fs::read_to_string(f).unwrap())) {
            writers.push(rel);
        }
    }
    writers.sort();
    assert_eq!(writers, ["crates/delphi-engine/src/library.rs", "crates/delphi-engine/src/library_retraction.rs"], "who writes a retraction mark");
}
