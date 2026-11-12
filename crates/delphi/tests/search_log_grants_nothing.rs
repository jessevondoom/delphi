//! `specs/101` §12 item 6: the search log grants nothing. It is a record the daemon writes and a text the operator reads; no promotion,
//! no trust, no tool access, no approval and no queue order reads it. Like `corroboration_grants_nothing.rs`, the test is a net, not a
//! proof: it reads the non-test source of every crate and fails when a file not on the short list below names the log, so a new reader
//! has to be added by a reviewer, who then has to ask the question this test exists for: does it decide anything?

use regex::Regex;
use std::path::{Path, PathBuf};

/// Where the log may be named, and why each file may. Nothing here decides a permission, a tier, a taint level or an order.
const READERS: &[(&str, &str)] = &[
    ("crates/delphi-engine/src/search_log.rs", "defines the record, its flow arithmetic, its persistence and its outbound check"),
    ("crates/delphi-engine/src/research_run.rs", "writes lines as the run searches and attaches the log to the result; no branch reads it"),
    ("crates/delphi-engine/src/service.rs", "reads the host's declared searches and keeps the finished log beside the result"),
    ("crates/delphi-engine/src/mcp_host.rs", "declares the optional `host_searches` input of `request_research`; the daemon reads it"),
    ("crates/delphi-engine/src/lib.rs", "declares the module"),
    ("crates/delphi/src/doctor.rs", "names the removed `[research] search_log` key so an old config is told it does nothing (specs/098 section 13.2); it reads no log"),
    ("crates/delphi-engine/src/egress.rs", "names the search-log module's test among the proofs that the observed-URL fetch carries no entry text; it reads no log"),
    ("crates/delphi/src/research_cmd.rs", "prints a kept log for the operator"),
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

/// The text before the file's inline test module, which is the last `mod tests`: test code may name the log. (The older net cuts at the
/// first `#[cfg(test)]`, which hides everything after an early `#[cfg(test)] mod x_tests;` line; cutting at the last test module can only
/// scan more, never less.)
fn production(src: &str) -> &str {
    let cut = src.rfind("\n#[cfg(test)]\nmod tests").or_else(|| src.rfind("\nmod tests")).unwrap_or(src.len());
    &src[..cut]
}

fn pattern() -> Regex {
    Regex::new(r"SearchLog|search_log|HostSearch|host_searches").unwrap()
}

#[test]
fn no_file_outside_the_list_names_the_search_log() {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(files.len() > 100, "the scan found {} files; it is looking in the wrong place", files.len());
    let pat = pattern();
    let (mut stray, mut seen) = (Vec::new(), Vec::<String>::new());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(f).unwrap();
        // comments may name the log; code may not
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if pat.is_match(&code) {
            seen.push(rel.clone());
            if !READERS.iter().any(|(p, _)| *p == rel) {
                stray.push(rel);
            }
        }
    }
    assert!(stray.is_empty(), "these files name the search log but are not on the list in {}: {stray:?}", file!());
    let gone: Vec<&str> = READERS.iter().map(|(p, _)| *p).filter(|p| !seen.iter().any(|s| s == p)).collect();
    assert!(gone.is_empty(), "these files are on the list but no longer name the log; remove them from the list: {gone:?}");
}

#[test]
fn a_line_that_names_the_log_never_also_names_a_permission_a_tier_or_an_approval() {
    // Within the listed files, a line that names the log must not also name an authority word. A word list is crude, which is why the
    // list above is short and each entry says what it does; this catches the careless edit.
    let authority = Regex::new(r"(?i)\b(approve[sd]?|approval|permit|permission|grant|taint|tier|trust_level|allow(ed)?_tools?|sandbox|elevate|promote|ratif)").unwrap();
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
    assert!(bad.is_empty(), "a line names the search log beside an authority word:\n{}", bad.join("\n"));
}

#[test]
fn the_runner_only_writes_to_the_log_and_never_branches_on_it() {
    // In `research_run.rs`, every use of the `log` value is a call that records (`begin`, `withheld`, `rows`, `failed`, `kept`,
    // `duplicate`, `dropped`, `kb`, `declare`), the final read of its private flag and its two renderings. A condition on its content
    // (`log.lines()`/`log.flow()` inside an `if` or `match`) would let a result of the log change what the run does.
    let root = root();
    let src = std::fs::read_to_string(root.join("crates/delphi-engine/src/research_run.rs")).unwrap();
    let body = production(&src);
    let uses = Regex::new(r"\blog\.(\w+)").unwrap();
    let allowed = ["begin", "withheld", "rows", "failed", "kept", "duplicate", "dropped", "kb", "declare", "private", "flow", "lines", "to_json"];
    let mut other = Vec::new();
    let mut reads = Vec::new();
    for (n, line) in body.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for c in uses.captures_iter(line) {
            let m = &c[1];
            if !allowed.contains(&m) {
                other.push(format!("{}: log.{m}", n + 1));
            }
            if matches!(m, "flow" | "lines" | "to_json") {
                reads.push((n + 1, line.trim().to_string()));
            }
        }
    }
    assert!(other.is_empty(), "the runner uses the log in a way this test does not know: {other:?}");
    // the only reads are the summary and the attachment the runner writes into the result
    for (n, l) in &reads {
        assert!(!l.starts_with("if ") && !l.contains("match ") && !l.contains("&&") && !l.contains("||"), "line {n} branches on the log's content: {l}");
    }
}
