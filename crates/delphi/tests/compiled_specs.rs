//! The compiled specs (`compiled/specs/data/`, spec 088 stage 1) are a derived view and never the authority. Four guards:
//!
//! 1. No Rust code outside the compiler, the command table and these tests names the directory, so nothing in the program can
//!    read a compiled file to decide anything (the "no approval channel" invariant, spec 088 section 2).
//! 2. The repository's own specs compile, and the committed files are exactly what a fresh compile writes.
//! 3. Every committed file's quotations are the source cut at their recorded spans, and none carries an approval field.
//! 4. The release seed ships only what it lists, and it does not list the compiled directory.
//!
//! The indexer and the project-files tool have their own tests beside their code (`kb.rs`, `mcp_project.rs`).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Files that may name the compiled directory: the compiler itself, the command table (its help text), and the tests that read
/// the files to check them. Anything else naming it fails guard 1.
const MAY_NAME_IT: [&str; 4] = ["crates/delphi/src/spec_compile.rs", "crates/delphi/src/main.rs", "crates/delphi/tests/compiled_specs.rs", "crates/delphi/tests/docs.rs"];

/// The two spellings that name the directory or its groups file.
fn names_the_compiled_directory(text: &str) -> bool {
    text.contains("compiled/specs") || text.contains("compiled/groups")
}

fn walk(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name == "target" || name == "node_modules" || name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            walk(&p, exts, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)) {
            out.push(p);
        }
    }
}

#[test]
fn no_code_outside_the_compiler_names_the_compiled_directory() {
    let root = root();
    let mut files = Vec::new();
    walk(&root.join("crates"), &["rs"], &mut files);
    walk(&root.join("ui/src"), &["ts", "tsx"], &mut files);
    assert!(files.len() > 100, "the walk found only {} files; the path is broken", files.len());
    let mut named = Vec::new();
    for f in files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        if MAY_NAME_IT.contains(&rel.as_str()) {
            continue;
        }
        if names_the_compiled_directory(&std::fs::read_to_string(&f).unwrap_or_default()) {
            named.push(rel);
        }
    }
    assert!(named.is_empty(), "code that names the compiled directory (nothing may read a compiled file to decide anything):\n{}", named.join("\n"));
}

#[test]
fn the_guard_catches_a_file_that_names_the_directory() {
    assert!(names_the_compiled_directory("let p = root.join(\"compiled/specs/data/x.json\");"));
    assert!(names_the_compiled_directory("// reads compiled/groups.toml"));
    assert!(!names_the_compiled_directory("let p = root.join(\"compiled\").join(\"specs\");"), "a built name is not a literal one");
    assert!(!names_the_compiled_directory("nothing here"));
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(DELPHI).args(args).arg("--repo").arg(root()).output().expect("run delphi")
}

#[test]
fn the_repos_own_specs_compile_and_the_committed_files_are_current() {
    let out = run(&["specs", "compile", "--check"]);
    assert!(out.status.success(), "run `delphi specs compile` and commit the result:\n{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn every_committed_quotation_is_the_source_cut_at_its_span() {
    let root = root();
    let mut checked = 0;
    for e in std::fs::read_dir(root.join("compiled/specs/data")).unwrap().flatten() {
        let text = std::fs::read_to_string(e.path()).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let src = std::fs::read_to_string(root.join(v["source"]["path"].as_str().unwrap())).unwrap();
        for q in [&v["source_status_at_hash"]["line"], &v["summary"]] {
            if q.is_null() {
                continue;
            }
            let (s, t) = (q["span"]["start"].as_u64().unwrap() as usize, q["span"]["end"].as_u64().unwrap() as usize);
            assert_eq!(&src[s..t], q["text"].as_str().unwrap(), "{}: a quotation is the source at its span", e.path().display());
            assert_eq!(src[..s].matches('\n').count() + 1, q["span"]["line"].as_u64().unwrap() as usize, "{}: first line", e.path().display());
            checked += 1;
        }
        for key in ["approved_by", "approved_on", "as_of"] {
            assert!(!text.contains(&format!("\"{}\"", key)), "{}: a compiled file carries no `{}`", e.path().display(), key);
        }
        assert_eq!(v["authoritative"], false);
    }
    assert!(checked > 100, "only {} quotations checked", checked);
}

#[test]
fn the_committed_views_are_banner_first_inert_and_list_every_spec_once() {
    let root = root();
    let tree = root.join("compiled").join("specs");
    let stems: Vec<String> = std::fs::read_dir(tree.join("data")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().trim_end_matches(".json").to_string()).collect();
    let mut pages = Vec::new();
    for e in std::fs::read_dir(&tree).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if e.path().is_file() && name.ends_with(".md") {
            pages.push((name, std::fs::read_to_string(e.path()).unwrap()));
        }
    }
    assert!(pages.iter().any(|(n, _)| n == "index.md") && pages.len() >= 10, "an index and a page per group: {:?}", pages.iter().map(|p| &p.0).collect::<Vec<_>>());
    for (name, text) in &pages {
        assert!(text.starts_with("> Generated by `delphi specs compile`"), "{} starts with the banner", name);
        assert!(text.lines().next().unwrap().contains("Not the authority") && text.lines().next().unwrap().contains("untrusted"));
        assert!(!text.starts_with("---"), "{} has no frontmatter", name);
        for bad in ["approved_by", "approved_on", "as_of:"] {
            assert!(!text.contains(bad), "{} must not carry {}", name, bad);
        }
        // Every excerpt is in a fenced block that follows its label: count labels and fences.
        let labels = text.lines().filter(|l| l.starts_with("**source excerpt, untrusted**")).count();
        let opens = text.lines().filter(|l| l.starts_with("```") && l.ends_with("text")).count();
        assert_eq!(labels, opens, "{}: every label is followed by one fenced block", name);
    }
    let index = &pages.iter().find(|(n, _)| n == "index.md").unwrap().1;
    for stem in &stems {
        assert_eq!(index.matches(&format!("| [{}](./", stem)).count(), 1, "{} is in the index once", stem);
        let on_pages = pages.iter().filter(|(n, t)| n != "index.md" && t.contains(&format!("\n## {}\n", stem))).count();
        assert_eq!(on_pages, 1, "{} is on exactly one group page", stem);
    }
}

#[test]
fn six_specs_share_a_number_no_longer_and_every_spec_has_its_own_file() {
    let root = root();
    let mut stems = Vec::new();
    let mut numbers = std::collections::BTreeMap::<String, Vec<String>>::new();
    for e in std::fs::read_dir(root.join("compiled/specs/data")).unwrap().flatten() {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
        let stem = v["spec"]["stem"].as_str().unwrap().to_string();
        assert_eq!(e.file_name().to_string_lossy(), format!("{}.json", stem), "a file is named by its stem");
        if let Some(n) = v["spec"]["number"].as_str() {
            numbers.entry(n.to_string()).or_default().push(stem.clone());
        }
        stems.push(stem);
    }
    let dup: Vec<_> = numbers.iter().filter(|(_, v)| v.len() > 1).collect();
    assert!(dup.is_empty(), "two specs share a number: {:?}", dup);
    assert!(stems.len() > 100);
}

#[test]
fn the_release_seed_lists_only_specs_and_the_reference() {
    let build = std::fs::read_to_string(root().join("crates/delphi/build.rs")).unwrap();
    assert!(build.contains("(\"specs\", \"about/specs\")") && build.contains("(\"docs/reference\", \"about/reference\")"), "the seed pairs are what this test expects");
    assert!(!names_the_compiled_directory(&build) && !build.contains("\"compiled\""), "the release seed does not ship the compiled directory");
}
