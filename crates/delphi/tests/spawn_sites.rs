//! Spawn-site ratchet (spec 093 K1): every place non-test Rust starts a process is listed, with a class, in
//! `spawn_sites.toml`. A new site, an unlisted file, a higher count or a stale entry fails the build, so a process start
//! cannot appear without a reviewer choosing its class. The scan reads source text; it is a net for the habit of
//! reaching for `Command::new`, not a proof about every way a process can start.

use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const LIST: &str = "crates/delphi/tests/spawn_sites.toml";

const CLASSES: &[&str] = &["sandbox", "seat", "self", "git", "probe", "opener", "operator-config", "operator-cli", "train", "test-support"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn pattern() -> Regex {
    Regex::new(r"Command::new\(|Command::from\(|CommandBuilder::new\(|\bopenpty\(|\bnative_pty_system\(|\bforkpty\(").unwrap()
}

/// The process-start sites in one file's text, leaving out comments and test code. A `#[cfg(test)] mod` runs to the end of
/// the file (clippy's `items_after_test_module` forbids code after it); any other `#[cfg(test)]` item ends at the
/// closing line at its own indent, or at its own line when it ends with `;`.
fn count_sites(src: &str) -> usize {
    let pat = pattern();
    let lines: Vec<&str> = src.split('\n').collect();
    let (mut n, mut i) = (0usize, 0usize);
    while i < lines.len() {
        let line = lines[i];
        let s = line.trim_start();
        if s.starts_with("#![cfg(test)]") {
            return 0;
        }
        if s.starts_with("#[cfg(test)]") {
            let indent = &line[..line.len() - s.len()];
            i += 1;
            while i < lines.len() && lines[i].trim_start().starts_with("#[") {
                i += 1;
            }
            if i >= lines.len() {
                break;
            }
            let first = lines[i].trim_end();
            let head = first.trim_start();
            if (head.starts_with("mod ") || head.starts_with("pub mod ") || head.starts_with("pub(crate) mod ")) && !first.ends_with(';') {
                return n;
            }
            if first.ends_with(';') {
                i += 1;
                continue;
            }
            i += 1;
            let close = format!("{indent}}}");
            while i < lines.len() && lines[i].trim_end() != close {
                i += 1;
            }
            i += 1;
            continue;
        }
        if !s.starts_with("//") {
            n += pat.find_iter(line).count();
        }
        i += 1;
    }
    n
}

fn is_test_file(name: &str) -> bool {
    name == "tests.rs" || name.ends_with("_tests.rs")
}

fn scan(dir: &Path, root: &Path, out: &mut BTreeMap<String, usize>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if matches!(name.as_str(), "target" | "tests" | "node_modules" | "fixtures") || name.starts_with('.') {
                continue;
            }
            scan(&p, root, out);
        } else if name.ends_with(".rs") && !is_test_file(&name) {
            let n = count_sites(&std::fs::read_to_string(&p).unwrap_or_default());
            if n > 0 {
                out.insert(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), n);
            }
        }
    }
}

/// The listed sites per file, plus the problems with the list itself (an unknown class, a zero count).
fn listed(text: &str) -> (BTreeMap<String, usize>, Vec<String>) {
    let v: toml::Value = toml::from_str(text).expect("spawn_sites.toml is TOML");
    let mut out = BTreeMap::new();
    let mut bad = vec![];
    for s in v.get("site").and_then(|s| s.as_array()).into_iter().flatten() {
        let file = s.get("file").and_then(|f| f.as_str()).unwrap_or("").to_string();
        let count = s.get("count").and_then(|c| c.as_integer()).unwrap_or(0);
        let class = s.get("class").and_then(|c| c.as_str()).unwrap_or("");
        if !CLASSES.contains(&class) {
            bad.push(format!("{file}: class `{class}` is not one of {CLASSES:?}"));
        }
        if file.is_empty() || count < 1 {
            bad.push(format!("an entry needs a file and a count of at least 1 (got `{file}`, {count})"));
        }
        *out.entry(file).or_insert(0usize) += usize::try_from(count.max(0)).unwrap_or(0);
    }
    (out, bad)
}

/// Every way the scan and the list can disagree, as messages that say what to do.
fn compare(found: &BTreeMap<String, usize>, list: &BTreeMap<String, usize>) -> Vec<String> {
    let mut problems = vec![];
    for (file, n) in found {
        match list.get(file) {
            None => problems.push(format!("{file}: {n} process-start site(s) and no entry in {LIST}. Add an entry with the class that is true; if none fits, ask a reviewer.")),
            Some(l) if l < n => problems.push(format!("{file}: {n} site(s) found, {l} listed. Add the new site to {LIST} with its class.")),
            Some(l) if l > n => problems.push(format!("{file}: {n} site(s) found, {l} listed. A site was removed: lower the count in {LIST}.")),
            Some(_) => {}
        }
    }
    for file in list.keys() {
        if !found.contains_key(file) {
            problems.push(format!("{file}: listed in {LIST} but it starts no process now. Remove the entry."));
        }
    }
    problems
}

#[test]
fn every_process_start_is_listed_and_classified() {
    let root = root();
    let mut found = BTreeMap::new();
    scan(&root.join("crates"), &root, &mut found);
    let (list, mut problems) = listed(&std::fs::read_to_string(root.join(LIST)).expect("spawn_sites.toml exists"));
    problems.extend(compare(&found, &list));
    assert!(problems.is_empty(), "the process-start sites no longer match {LIST}:\n  {}", problems.join("\n  "));
}

#[test]
fn the_scan_finds_the_sites_that_matter_and_only_those() {
    // Spec 093 AC-K1a: the seat launcher is a site, and it is listed as one.
    let root = root();
    let mut found = BTreeMap::new();
    scan(&root.join("crates"), &root, &mut found);
    assert_eq!(found.get("crates/delphi-engine/src/harness.rs"), Some(&1), "the harness seat start (`Command::from`) must be seen");
    assert!(found.contains_key("crates/delphi-sandbox/src/lib.rs"));
    assert!(!found.keys().any(|f| f.contains("/tests/")), "integration tests are not scanned");
}

#[test]
fn counting_skips_comments_and_test_code() {
    assert_eq!(count_sites("fn a() { Command::new(\"git\"); }\n"), 1);
    assert_eq!(count_sites("// Command::new(\"git\")\n    // Command::new(\"git\")\n"), 0);
    assert_eq!(count_sites("let c = tokio::process::Command::from(std_cmd);\n"), 1);
    assert_eq!(count_sites("let (m, s) = openpty(None, None);\n"), 1);
    // A test module runs to the end of the file, even past a lone `}` inside a string.
    let module = "fn real() { Command::new(\"a\"); }\n#[cfg(test)]\nmod tests {\n    let s = \"\n}\n\";\n    Command::new(\"b\");\n}\n";
    assert_eq!(count_sites(module), 1);
    // A cfg(test) item ends at its own closing line; the code after it counts.
    let item = "#[cfg(test)]\nfn helper() {\n    Command::new(\"t\");\n}\nfn real() { Command::new(\"r\"); }\n";
    assert_eq!(count_sites(item), 1);
    // A cfg(test) line that ends with `;` is one line.
    assert_eq!(count_sites("#[cfg(test)]\nuse std::process::Command;\nfn real() { Command::new(\"r\"); }\n"), 1);
    // An indented cfg(test) item closes at its own indent.
    assert_eq!(count_sites("impl A {\n    #[cfg(test)]\n    fn t() {\n        Command::new(\"t\");\n    }\n    fn r() { Command::new(\"r\"); }\n}\n"), 1);
    assert_eq!(count_sites("#![cfg(test)]\nCommand::new(\"x\");\n"), 0);
}

#[test]
fn the_list_and_the_scan_disagree_loudly() {
    let found: BTreeMap<String, usize> = [("a.rs".to_string(), 2usize), ("b.rs".to_string(), 1)].into();
    let same = found.clone();
    assert!(compare(&found, &same).is_empty());
    // A new file, a higher count, a lower count and a stale entry each fail, naming the file.
    let mut list = same.clone();
    list.remove("b.rs");
    assert!(compare(&found, &list).iter().any(|p| p.starts_with("b.rs") && p.contains("no entry")));
    let mut list = same.clone();
    list.insert("a.rs".into(), 1);
    assert!(compare(&found, &list).iter().any(|p| p.starts_with("a.rs") && p.contains("Add the new site")));
    let mut list = same.clone();
    list.insert("a.rs".into(), 3);
    assert!(compare(&found, &list).iter().any(|p| p.starts_with("a.rs") && p.contains("lower the count")));
    let mut list = same;
    list.insert("gone.rs".into(), 1);
    assert!(compare(&found, &list).iter().any(|p| p.starts_with("gone.rs") && p.contains("Remove the entry")));
}

#[test]
fn the_list_rejects_an_unknown_class_and_an_empty_entry() {
    let (_, bad) = listed("[[site]]\nfile = \"a.rs\"\ncount = 1\nclass = \"whatever\"\n");
    assert!(bad.iter().any(|b| b.contains("not one of")));
    let (_, bad) = listed("[[site]]\nfile = \"a.rs\"\ncount = 0\nclass = \"git\"\n");
    assert!(bad.iter().any(|b| b.contains("at least 1")));
    let (map, bad) = listed("[[site]]\nfile = \"a.rs\"\ncount = 1\nclass = \"git\"\n\n[[site]]\nfile = \"a.rs\"\ncount = 2\nclass = \"probe\"\n");
    assert!(bad.is_empty());
    assert_eq!(map.get("a.rs"), Some(&3), "a file with two classes adds up");
}
