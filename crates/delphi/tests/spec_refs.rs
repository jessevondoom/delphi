//! Cross-reference gate: a spec reference (`specs/NNN §X.Y`, `spec NNN §X`, `NNN §X`, `NNN-An`) must resolve.
//! Folding a spec (amendment text moved to `specs/history/`) may not silently break one.

use delphi_core::specdoc;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// References that were already broken when this gate was added, as `file: reference`.
/// All pre-date the gate (checked against main): the target section is a list item or was never a heading.
/// Fix the reference (or the spec) and delete the entry; never add one for a new break.
const ALLOWLIST: &[&str] = &[
    "crates/delphi-core/src/pathguard.rs: 010 §8.2 (no such heading)",
    "crates/delphi-engine/src/engine.rs: 023 §5 (no such heading)",
    "crates/delphi-engine/src/harness.rs: 051 §1.1 (no such heading)",
    "docs/reference/harness-coverage.md: 047 §1.1 (no such heading)",
    "docs/reference/harness-coverage.md: 051 §1.1 (no such heading)",
    "missions/2026-10-23-foundational-design/decisions.md: 009 §7.2 (no such heading)",
    "missions/2026-10-23-foundational-design/step-3-plan.md: 009 §7.7 (no such heading)",
    "missions/2026-10-23-foundational-design/xenia-phase-1-plan.md: 011 §11.2 (no such heading)",
    "missions/2026-10-23-foundational-design/xenia-phase-1-plan.md: 011 §11.8 (no such heading)",
    "missions/2026-10-23-foundational-design/xenia-phase-1-plan.md: 011 §6.2 (no such heading)",
    "specs/011-xenia-trust-model.md: 011 §11.1 (no such heading)",
    "specs/011-xenia-trust-model.md: 011 §11.5 (no such heading)",
    "specs/012-xenia-record-schema.md: 011 §11.1 (no such heading)",
    "specs/012-xenia-record-schema.md: 011 §11.3 (no such heading)",
    "specs/012-xenia-record-schema.md: 011 §11.4 (no such heading)",
    "specs/012-xenia-record-schema.md: 011 §11.5 (no such heading)",
    "specs/022-xenia-sharing-overview.md: 023 §5 (no such heading)",
    "specs/023-org-bundle-standard.md: 027 §6.1 (no such heading)",
    "specs/026-team-alignment-and-strategist-integration.md: 027 §6.3 (no such heading)",
    "specs/049-context-management-recoverable-compaction-and-handles.md: 045 §2.1 (no such heading)",
    "specs/051-copilot-cli-harness.md: 047 §1.1 (no such heading)",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn walk(dir: &Path, exts: &[&str], skip: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name == "target" || name == "node_modules" || name.starts_with('.') || skip(&p) {
            continue;
        }
        if p.is_dir() {
            walk(&p, exts, skip, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)) {
            out.push(p);
        }
    }
}

/// The directories searched (beside `crates/` and the changelog) for references. The compiled specs repeat the sources' text and are
/// a derived view, so they are not searched: a reference inside a quotation of a source is checked once, in the source itself.
const SCANNED_DIRS: [&str; 5] = ["specs", "docs", "schemas", "ui/src", "missions"];

struct Spec {
    text: String,
    sections: BTreeSet<String>,
}

#[test]
fn spec_references_resolve() {
    let root = root();

    // spec id ("004", "005a") -> normative files and history files
    let mut specs: HashMap<String, Vec<Spec>> = HashMap::new();
    let mut history: HashMap<String, String> = HashMap::new();
    for (dir, is_history) in [("specs", false), ("specs/history", true)] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(id) = specdoc::spec_number(&name) else { continue };
            let text = std::fs::read_to_string(e.path()).unwrap();
            if is_history {
                history.entry(id).or_default().push_str(&text);
            } else {
                let sections = specdoc::heading_numbers(&text);
                specs.entry(id).or_default().push(Spec { text, sections });
            }
        }
    }

    let mut files = Vec::new();
    let skip = |p: &Path| {
        let s = p.to_string_lossy();
        s.ends_with("/tests/spec_refs.rs")
            || s.ends_with("/delphi-core/src/specdoc.rs") // the parser's own tests cite sample numbers
            || s.contains("/missions/") && (s.contains("/councils") || s.contains("/history"))
            || s.ends_with("/specs/history")
    };
    for d in SCANNED_DIRS {
        walk(&root.join(d), &["md", "json", "ts", "tsx", "toml"], &skip, &mut files);
    }
    walk(&root.join("crates"), &["rs"], &skip, &mut files);
    files.push(root.join("CHANGELOG.md"));

    // The patterns live in `delphi_core::specdoc`, the one parser of spec text (`one_parser_for_spec_text` below keeps it that way).
    let mut broken = BTreeSet::new();
    let mut checked = 0;
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        for r in specdoc::section_refs(&text) {
            checked += 1;
            let (id, num) = (r.spec.as_str(), r.section.as_str());
            match specs.get(id) {
                None => {
                    broken.insert(format!("{}: {} §{} (no specs/{}-*.md)", rel, id, num, id));
                }
                Some(v) if !v.iter().any(|s| s.sections.contains(specdoc::heading_of(num))) => {
                    broken.insert(format!("{}: {} §{} (no such heading)", rel, id, num));
                }
                _ => {}
            }
        }
        for t in specdoc::amendment_tags(&text) {
            checked += 1;
            let (id, tag) = (t.spec.as_str(), t.tag.as_str());
            let found = specs.get(id).is_some_and(|v| v.iter().any(|s| s.text.contains(tag)))
                || history.get(id).is_some_and(|h| h.contains(tag));
            if !found {
                broken.insert(format!("{}: {} (not in specs/{}-*.md or specs/history/)", rel, tag, id));
            }
        }
    }
    assert!(checked > 100, "the scan found only {} references; the patterns or paths are broken", checked);

    let allowed: BTreeSet<String> = ALLOWLIST.iter().map(|s| s.to_string()).collect();
    let new: Vec<_> = broken.difference(&allowed).collect();
    let stale: Vec<_> = allowed.difference(&broken).collect();
    assert!(new.is_empty(), "broken spec references:\n{}", new.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
    assert!(stale.is_empty(), "allowlist entries that now resolve (delete them):\n{}", stale.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
}

/// The lines of one Rust file that build a regular expression for spec text outside the parser: a section reference (the `§`
/// mark), an amendment tag (`-A` and a number group), a numbered heading (a `#{` repeat) or a spec file name (three digits and an
/// optional letter). The parser module and this gate are exempt. It is a search for the shape of the pattern, so it cannot prove
/// there is no other parser, but a copy of one of the four patterns fails it.
fn second_parser_lines(rel: &str, text: &str) -> Vec<String> {
    if rel == "crates/delphi-core/src/specdoc.rs" || rel == "crates/delphi/tests/spec_refs.rs" {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") || !(line.contains("Regex") || line.contains("regex!")) {
            continue;
        }
        let section = line.contains('§');
        let amendment = line.contains("-A(") || line.contains("-A[0-9") || line.contains("-A\\d");
        let heading = line.contains("#{");
        let file_name = line.contains("{3}[a-z]");
        if section || amendment || heading || file_name {
            out.push(format!("{}:{}", rel, i + 1));
        }
    }
    out
}

#[test]
fn the_compiled_specs_are_not_a_searched_directory() {
    assert!(SCANNED_DIRS.iter().all(|d| !d.starts_with("compiled")), "the compiled specs quote the sources; they are checked there, once");
    // Not vacuous: the compiled files do hold quotations of text this gate would read.
    let any = std::fs::read_dir(root().join("compiled").join("specs").join("data")).map(|rd| rd.flatten().count()).unwrap_or(0);
    assert!(any > 100, "the compiled directory has {} files", any);
}

#[test]
fn one_parser_for_spec_text() {
    let root = root();
    let mut files = Vec::new();
    walk(&root.join("crates"), &["rs"], &|_| false, &mut files);
    assert!(files.len() > 100, "the walk found only {} Rust files; the path is broken", files.len());
    let mut second = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        second.extend(second_parser_lines(&rel, &text));
    }
    assert!(second.is_empty(), "a pattern for spec text outside delphi_core::specdoc (use the parser, spec 088 section 2):\n{}", second.join("\n"));
}

#[test]
fn the_one_parser_gate_catches_a_copy_of_each_pattern() {
    let a = "let re = Regex::new(r\"([0-9]{3}[a-z]?) §§?([0-9.]+)\").unwrap();";
    let b = "static R: LazyLock<Regex> = LazyLock::new(|| Regex::new(r\"(\\d{3})-A(\\d+)\").unwrap());";
    let c = "let h = Regex::new(r\"(?m)^#{2,5}\\s+([0-9.]+)\").unwrap();";
    let d = "let n = Regex::new(r\"^specs/(\\d{3}[a-z]?)-[^/]+\\.md$\").unwrap();";
    for (name, line) in [("section reference", a), ("amendment tag", b), ("numbered heading", c), ("spec file name", d)] {
        assert_eq!(second_parser_lines("crates/x/src/a.rs", line).len(), 1, "{} was not caught", name);
    }
    // Not flagged: a comment, a line that builds no regex, an unrelated regex, the parser itself and this gate.
    for line in ["// Regex::new(r\"(\\d{3})-A(\\d+)\") in a comment", "let x = 12; // § only", "let r = Regex::new(r\"^[a-z]+$\").unwrap();"] {
        assert!(second_parser_lines("crates/x/src/a.rs", line).is_empty(), "{}", line);
    }
    assert!(second_parser_lines("crates/delphi-core/src/specdoc.rs", a).is_empty());
    assert!(second_parser_lines("crates/delphi/tests/spec_refs.rs", a).is_empty());
}
