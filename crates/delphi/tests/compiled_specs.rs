//! The compiled specs (`compiled/specs/data/`, spec 088 stage 1) are a derived view and never the authority. Four guards:
//!
//! 1. No Rust code outside the compiler, the command table and these tests names the directory, so nothing in the program can
//!    read a compiled file to decide anything (the "no approval channel" invariant, spec 088 section 2).
//! 2. The repository's own specs compile, and the committed files are exactly what a fresh compile writes.
//! 3. Every committed file's quotations are the source cut at their recorded spans, and none carries an approval field.
//! 4. The release seed ships only what it lists, and it does not list the compiled directory.
//! 5. Two branches that each add a spec merge cleanly into a current tree (no whole-set line, spec 088 §13).
//!
//! The indexer and the project-files tool have their own tests beside their code (`kb.rs`, `mcp_project.rs`).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Files that may name the compiled directory: the compiler, the lookup that serves it (`delphi specs show`, and the MCP tools
/// that will share it), the command table (its help text), and the tests that read the files to check them. Anything else naming
/// it fails guard 1.
const MAY_NAME_IT: [&str; 5] = ["crates/delphi/src/spec_compile.rs", "crates/delphi-engine/src/spec_lookup.rs", "crates/delphi/src/main.rs", "crates/delphi/tests/compiled_specs.rs", "crates/delphi/tests/docs.rs"];

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
    // Spec 088 §14: the index is the one committed page; each group's page is printed on demand by `delphi specs view <group>`.
    assert_eq!(pages.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), vec!["index.md"], "only the index is committed");
    let list = Command::new(DELPHI).args(["specs", "view", "--repo"]).arg(&root).output().unwrap();
    assert!(list.status.success(), "{}", String::from_utf8_lossy(&list.stderr));
    let ids: Vec<String> = String::from_utf8_lossy(&list.stdout).lines().map(|l| l.split('\t').next().unwrap().to_string()).collect();
    assert!(ids.len() >= 9, "the groups: {:?}", ids);
    for id in &ids {
        let page = Command::new(DELPHI).args(["specs", "view", id, "--repo"]).arg(&root).output().unwrap();
        assert!(page.status.success(), "{}: {}", id, String::from_utf8_lossy(&page.stderr));
        pages.push((format!("{id}.md"), String::from_utf8(page.stdout).unwrap()));
    }
    for (name, text) in &pages {
        assert!(text.starts_with("> Generated by `delphi specs compile` or `delphi specs view`"), "{} starts with the banner", name);
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
        assert_eq!(index.matches(&format!("| [{}](../../specs/", stem)).count(), 1, "{} is in the index once", stem);
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

/// Runs git in `dir` with a fixed identity and no signing, and returns its output.
fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"]).args(args).output().expect("run git")
}

fn git_ok(dir: &Path, args: &[&str]) -> String {
    let out = git(dir, args);
    assert!(out.status.success(), "git {:?} failed:\n{}{}", args, String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// A spec file the compile accepts.
fn fixture_spec(stem: &str) -> String {
    format!("---\nid: spec-{stem}\ntitle: \"Spec {stem}\"\nstatus: draft\n---\n\n# Spec {stem}\n\n## 1. Scope and objectives\n\nThe summary of {stem}.\n\n## 2. Next\n\ntext\n")
}

/// Adds a spec on the current branch: its file, its line in the groups file after the line `after`, a compile, a commit.
fn add_spec_on_branch(dir: &Path, stem: &str, group: &str, after: &str) {
    std::fs::write(dir.join(format!("specs/{}.md", stem)), fixture_spec(stem)).unwrap();
    let groups = std::fs::read_to_string(dir.join("compiled/groups.toml")).unwrap();
    let line = format!("\"{}\" = \"{}\"\n", stem, group);
    let at = groups.find(after).expect("the anchor line") + after.len();
    std::fs::write(dir.join("compiled/groups.toml"), format!("{}{}{}", &groups[..at], line, &groups[at..])).unwrap();
    let c = Command::new(DELPHI).args(["specs", "compile", "--repo"]).arg(dir).output().unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    git_ok(dir, &["add", "-A"]);
    git_ok(dir, &["commit", "-qm", &format!("add {}", stem)]);
}

/// Spec 088 §13.7 test 23, the 2026-11-20 incident as a fixture: two branches each add a spec to a different group, merge with
/// no conflict, and the result is current without a recompile. Under the renderer before S3c the merge was just as clean, but the
/// index said "5 specs in 2 groups" over six specs, and `--check` failed on main.
#[test]
fn two_specs_added_on_two_branches_merge_without_conflict_and_the_result_is_current() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    std::fs::create_dir_all(dir.join("specs")).unwrap();
    std::fs::create_dir_all(dir.join("compiled")).unwrap();
    for stem in ["001-a", "002-b", "003-c", "004-d"] {
        std::fs::write(dir.join(format!("specs/{}.md", stem)), fixture_spec(stem)).unwrap();
    }
    let groups = "[[group]]\nid = \"one\"\ntitle = \"Group one\"\norder = 1\n\n[[group]]\nid = \"two\"\ntitle = \"Group two\"\norder = 2\n\n[specs]\n\"001-a\" = \"one\"\n\"002-b\" = \"one\"\n\"003-c\" = \"two\"\n\"004-d\" = \"two\"\n";
    std::fs::write(dir.join("compiled/groups.toml"), groups).unwrap();
    git_ok(dir, &["init", "-q"]);
    git_ok(dir, &["checkout", "-q", "-b", "base"]);
    let c = Command::new(DELPHI).args(["specs", "compile", "--repo"]).arg(dir).output().unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    git_ok(dir, &["add", "-A"]);
    git_ok(dir, &["commit", "-qm", "base"]);
    // Branch A adds a spec to the first group, its groups line near the top; branch B one to the second, its line at the end, so
    // the two hunks of the groups file are separated by unchanged lines.
    git_ok(dir, &["checkout", "-q", "-b", "a", "base"]);
    add_spec_on_branch(dir, "005-e", "one", "\"001-a\" = \"one\"\n");
    git_ok(dir, &["checkout", "-q", "-b", "b", "base"]);
    add_spec_on_branch(dir, "006-f", "two", "\"004-d\" = \"two\"\n");
    git_ok(dir, &["checkout", "-q", "a"]);
    let merge = git(dir, &["merge", "--no-edit", "-q", "b"]);
    assert!(merge.status.success(), "the merge of B into A is clean:\n{}{}", String::from_utf8_lossy(&merge.stdout), String::from_utf8_lossy(&merge.stderr));
    assert_eq!(git_ok(dir, &["diff", "--name-only", "--diff-filter=U"]), "", "no conflicted file");
    assert_eq!(git_ok(dir, &["status", "--porcelain"]), "", "the merge committed everything");
    // The merged tree is current as it stands: no recompile.
    let check = Command::new(DELPHI).args(["specs", "compile", "--check", "--repo"]).arg(dir).output().unwrap();
    assert!(check.status.success(), "the merge result is current without a recompile:\n{}", String::from_utf8_lossy(&check.stderr));
    let index = std::fs::read_to_string(dir.join("compiled/specs/index.md")).unwrap();
    for stem in ["001-a", "002-b", "003-c", "004-d", "005-e", "006-f"] {
        assert_eq!(index.matches(&format!("| [{}](../../specs/", stem)).count(), 1, "{} is in the merged index once", stem);
    }
}

#[test]
fn the_release_seed_lists_only_specs_and_the_reference() {
    let build = std::fs::read_to_string(root().join("crates/delphi/build.rs")).unwrap();
    assert!(build.contains("(\"specs\", \"about/specs\")") && build.contains("(\"docs/reference\", \"about/reference\")"), "the seed pairs are what this test expects");
    assert!(!names_the_compiled_directory(&build) && !build.contains("\"compiled\""), "the release seed does not ship the compiled directory");
}
