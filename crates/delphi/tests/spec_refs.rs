//! Cross-reference gate: a spec reference (`specs/NNN §X.Y`, `spec NNN §X`, `NNN §X`, `NNN-An`) must resolve.
//! Folding a spec (amendment text moved to `specs/history/`) may not silently break one.

use regex::Regex;
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

/// `§3.3.2` names item 2 of the numbered list under heading `3.3`; deeper refs resolve to their heading.
fn heading_of(num: &str) -> &str {
    match num.matches('.').count() {
        0 | 1 => num,
        _ => num.rsplit_once('.').unwrap().0,
    }
}

struct Spec {
    text: String,
    sections: BTreeSet<String>,
}

#[test]
fn spec_references_resolve() {
    let root = root();
    let heading = Regex::new(r"(?m)^#{2,5}\s+§?([0-9]+(?:\.[0-9]+)*)\.?(?:\s|$)").unwrap();

    // spec id ("004", "005a") -> normative files and history files
    let mut specs: HashMap<String, Vec<Spec>> = HashMap::new();
    let mut history: HashMap<String, String> = HashMap::new();
    for (dir, is_history) in [("specs", false), ("specs/history", true)] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some((id, _)) = name.split_once('-') else { continue };
            if !name.ends_with(".md") || !id.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            let text = std::fs::read_to_string(e.path()).unwrap();
            if is_history {
                history.entry(id.to_string()).or_default().push_str(&text);
            } else {
                let sections = heading.captures_iter(&text).map(|c| c[1].to_string()).collect();
                specs.entry(id.to_string()).or_default().push(Spec { text, sections });
            }
        }
    }

    let mut files = Vec::new();
    let skip = |p: &Path| {
        let s = p.to_string_lossy();
        s.ends_with("/tests/spec_refs.rs")
            || s.contains("/missions/") && (s.contains("/councils") || s.contains("/history"))
            || s.ends_with("/specs/history")
    };
    for d in ["specs", "docs", "schemas", "ui/src", "missions"] {
        walk(&root.join(d), &["md", "json", "ts", "tsx", "toml"], &skip, &mut files);
    }
    walk(&root.join("crates"), &["rs"], &skip, &mut files);
    files.push(root.join("CHANGELOG.md"));

    let sect = Regex::new(r"(?-u:\b)([0-9]{3}[a-z]?)(?:-[a-z0-9][a-z0-9-]*(?:\.md)?)?\s+§§?\s*([0-9]+(?:\.[0-9]+)*)").unwrap();
    let amend = Regex::new(r"(?-u:\b)([0-9]{3}[a-z]?)-A([0-9]+)(?-u:\b)").unwrap();

    let mut broken = BTreeSet::new();
    let mut checked = 0;
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        let (has_sect, has_amend) = (text.contains('§'), text.contains("-A"));
        for c in sect.captures_iter(if has_sect { &text } else { "" }) {
            checked += 1;
            let (id, num) = (&c[1], &c[2]);
            match specs.get(id) {
                None => {
                    broken.insert(format!("{}: {} §{} (no specs/{}-*.md)", rel, id, num, id));
                }
                Some(v) if !v.iter().any(|s| s.sections.contains(heading_of(num))) => {
                    broken.insert(format!("{}: {} §{} (no such heading)", rel, id, num));
                }
                _ => {}
            }
        }
        for c in amend.captures_iter(if has_amend { &text } else { "" }) {
            checked += 1;
            let (id, tag) = (&c[1], c[0].to_string());
            let found = specs.get(id).is_some_and(|v| v.iter().any(|s| s.text.contains(&tag)))
                || history.get(id).is_some_and(|h| h.contains(&tag));
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
