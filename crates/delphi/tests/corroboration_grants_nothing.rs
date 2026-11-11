//! `specs/098` §12 item 17: `corroborated` and `trusted_sources` grant nothing. They are a count the daemon writes and a word it
//! shows; no promotion, no trust, no tool access, no approval and no queue order reads them. The test is a net, not a proof: it
//! reads the non-test source of every crate and fails when a file not on the short list below mentions the status, the raising
//! function, the `trusted_sources` tag or the stored `support` record. A new reader has to be added to the list by a reviewer,
//! who then has to ask the question this test exists for: does it decide anything?

use regex::Regex;
use std::path::{Path, PathBuf};

/// Where the count may be named, and why each file may. Nothing here decides a permission, a tier, a taint level or an order.
const READERS: &[(&str, &str)] = &[
    ("crates/delphi-core/src/knowledge_status.rs", "defines the status, its label and the raising rule"),
    ("crates/delphi-engine/src/corroboration.rs", "computes the count from citations; takes no policy input"),
    ("crates/delphi-engine/src/library_claims.rs", "stores the tags, derives the displayed status, ranks a pull with the same weight as noted"),
    ("crates/delphi-engine/src/research_run.rs", "prints the count in a research answer; the corroboration pass sends only public claims' words to search"),
    ("crates/delphi-engine/src/research_host.rs", "records a staged claim's tags in the ledger"),
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

/// The text before the first `#[cfg(test)]` at column zero or the first `mod tests`: test code is allowed to name the status.
fn production(src: &str) -> &str {
    let cut = src.find("\n#[cfg(test)]").or_else(|| src.find("\nmod tests")).unwrap_or(src.len());
    &src[..cut]
}

fn pattern() -> Regex {
    Regex::new(r#"Corroborated|raise_for_support|support_detail|trusted_sources|\["support"\]|CORROBORATED_MIN"#).unwrap()
}

#[test]
fn no_file_outside_the_list_reads_the_count_and_the_list_decides_nothing() {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(files.len() > 100, "the scan found {} files; it is looking in the wrong place", files.len());
    let pat = pattern();
    let mut stray = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        let body = production(&src);
        // comments may name the count; code may not
        let code: String = body.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if pat.is_match(&code) {
            seen.push(rel.clone());
            if !READERS.iter().any(|(p, _)| *p == rel) {
                stray.push(rel);
            }
        }
    }
    assert!(stray.is_empty(), "these files name the corroboration count but are not on the list in {}: {stray:?}", file!());
    let gone: Vec<&str> = READERS.iter().map(|(p, _)| *p).filter(|p| !seen.iter().any(|s| s == p)).collect();
    assert!(gone.is_empty(), "these files are on the list but no longer name the count; remove them from the list: {gone:?}");
}

#[test]
fn the_files_that_do_read_the_count_never_put_it_beside_a_permission_a_tier_or_an_approval() {
    // Within the listed files, a line that names the count must not also name an authority word. A word list is crude, which is
    // why the list above is short and each entry says what it does; this catches the careless edit.
    let authority = Regex::new(r"(?i)\b(approve[sd]?|approval|permit|permission|grant|taint|tier|trust_level|allow(ed)?_tools?|sandbox|egress|elevate|promote|ratif)").unwrap();
    let pat = pattern();
    let root = root();
    let mut bad = Vec::new();
    for (rel, _) in READERS {
        let src = std::fs::read_to_string(root.join(rel)).unwrap();
        for (i, line) in production(&src).lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with("///") {
                continue;
            }
            if pat.is_match(line) && authority.is_match(line) {
                bad.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(bad.is_empty(), "a line names the count and an authority word:\n{}", bad.join("\n"));
}

#[test]
fn the_status_table_keeps_corroborated_at_the_not_validated_label_and_raises_only_noted_and_queued_claims() {
    use delphi_core::knowledge_status::{raise_for_support, Status};
    assert_eq!(Status::Corroborated.label(), "not validated", "the operator still reads one of three words");
    let from = [Status::Validated, Status::Contested, Status::Disproved, Status::Retired];
    for s in from {
        assert_eq!(raise_for_support(s, 1000), s, "{s:?}: a count never overrides another status");
    }
    // the only transitions a count makes
    for (s, want) in [(Status::Noted, Status::Corroborated), (Status::Checking, Status::Corroborated)] {
        assert_eq!(raise_for_support(s, 3), want);
        assert_eq!(raise_for_support(s, 2), s);
    }
}
