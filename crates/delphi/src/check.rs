//! `delphi check <kind> <path>`: the deterministic helpers behind the built-in skills (`skills/CATALOG.md`, D-02).
//!
//! Each kind reads files and prints a verdict; none touches a workspace, the network or the daemon, so an agent
//! (or a human) can run one anywhere. A skill calls a check instead of re-deriving the rule in prose: the model
//! spends tokens on judgement and the binary spends none on arithmetic, graph walks or schema rules.

use crate::frontmatter::{self, get_str};
use crate::lint;
use crate::report::{Level, Report};
use regex::Regex;
use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use unicode_width::UnicodeWidthStr;

/// Every kind `delphi check` accepts. `lint` verifies that a `SKILL.md` names only these.
pub const KINDS: [&str; 9] = ["dag", "handoff", "provenance", "rules", "review-md", "spec", "kb", "tui", "skill"];

const COGNITIVE: [&str; 4] = ["deep-reasoning", "code-craft", "high-context-discovery", "fast-triage"];
const REVIEW_MD_CAP: usize = 300;
const SPEC_SECTIONS: [(&str, &str); 3] = [("scope", "Scope"), ("simpler alternative", "Simpler Alternative Considered"), ("verification plan", "Verification Plan")];

/// Run one check. `target` is a file (a directory for `kb`, `skill` and `tui`); `repo` is the root against which
/// `dag` resolves its `spec:` links.
pub fn run(kind: &str, target: &Path, repo: Option<&Path>) -> Result<Report, String> {
    let mut r = Report::default();
    match kind {
        "dag" => dag(&read(target)?, repo, &mut r),
        "handoff" => each(kind, lint::envelope_errors(&read(target)?), &mut r),
        "provenance" => each(kind, provenance(&read(target)?), &mut r),
        "rules" => lint::cap_errors(&read(target)?).into_iter().for_each(|(l, m)| r.push(l, kind, "", m)),
        "review-md" => review_md(&read(target)?, &mut r),
        "spec" => spec(&read(target)?, &mut r),
        "kb" => kb(target, &mut r)?,
        "tui" => tui(target, &mut r)?,
        "skill" => skill(target, &mut r)?,
        other => return Err(format!("unknown check `{}` (one of: {})", other, KINDS.join(", "))),
    }
    if r.findings.iter().all(|f| f.level != Level::Fail) {
        r.push(Level::Ok, kind, target.display().to_string(), "passes");
    }
    Ok(r)
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))
}

fn each(kind: &str, errs: Vec<String>, r: &mut Report) {
    for e in errs {
        r.fail(kind, "", e);
    }
}

fn provenance(text: &str) -> Vec<String> {
    match frontmatter::parse(text) {
        Err(e) => vec![e],
        Ok(doc) => match doc.frontmatter {
            None => vec!["missing provenance frontmatter (specs/001 §5)".into()],
            Some(fm) => frontmatter::check_provenance(&fm),
        },
    }
}

// ── dag: task graphs and milestone plans ────────────────────────────────────

struct Node {
    deps: Vec<String>,
    weight: u64,
}

/// `tasks:` (task decomposition) or `milestones:` (roadmap) as a graph: unique ids, known dependencies, no cycle.
/// On success reports the parallel waves and the weighted critical path.
fn dag(text: &str, repo: Option<&Path>, r: &mut Report) {
    let root: Value = match serde_yaml::from_str(text) {
        Ok(v) => v,
        Err(e) => return r.fail("dag", "", format!("not valid YAML: {}", e)),
    };
    let (key, items) = match ["tasks", "milestones"].iter().find_map(|k| root.get(*k).and_then(|v| v.as_sequence()).map(|s| (*k, s))) {
        Some(x) if !x.1.is_empty() => x,
        _ => return r.fail("dag", "", "needs a non-empty `tasks:` or `milestones:` list"),
    };
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(m) = item.as_mapping() else {
            r.fail("dag", format!("{}[{}]", key, i), "entry must be a mapping");
            continue;
        };
        let id = get_str(m, "id").unwrap_or("").trim().to_string();
        if id.is_empty() {
            r.fail("dag", format!("{}[{}]", key, i), "missing `id`");
            continue;
        }
        let at = format!("{}:{}", key, id);
        if key == "tasks" {
            match get_str(m, "tier") {
                Some(t) if COGNITIVE.contains(&t) => {}
                _ => r.fail("dag", &at, format!("`tier` must be one of {:?}", COGNITIVE)),
            }
        }
        if let Some(spec) = get_str(m, "spec") {
            let path = spec.split('#').next().unwrap_or("");
            if !path.starts_with("specs/") || !path.ends_with(".md") {
                r.fail("dag", &at, format!("spec link `{}` must be a `specs/*.md` path", spec));
            } else if repo.map(|root| !root.join(path).is_file()).unwrap_or(false) {
                r.fail("dag", &at, format!("spec `{}` does not exist", path));
            }
        }
        let deps: Vec<String> = match m.get(Value::String("depends_on".into())) {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => match v.as_sequence().map(|s| s.iter().map(|x| x.as_str().map(String::from)).collect::<Option<Vec<_>>>()) {
                Some(Some(d)) => d,
                _ => {
                    r.fail("dag", &at, "`depends_on` must be a list of ids");
                    Vec::new()
                }
            },
        };
        let weight = m.get(Value::String("weight".into())).map(|w| w.as_u64()).unwrap_or(Some(1));
        if weight.is_none() {
            r.fail("dag", &at, "`weight` must be a non-negative integer");
        }
        if nodes.insert(id.clone(), Node { deps, weight: weight.unwrap_or(1) }).is_some() {
            r.fail("dag", &at, "duplicate id");
        }
    }
    for (id, n) in &nodes {
        for d in &n.deps {
            if d == id {
                r.fail("dag", id, "depends on itself");
            } else if !nodes.contains_key(d) {
                r.fail("dag", id, format!("depends on unknown id `{}`", d));
            }
        }
    }
    if r.count(Level::Fail) > 0 {
        return;
    }
    // Kahn's algorithm, one wave at a time.
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let mut waves: Vec<Vec<&str>> = Vec::new();
    while done.len() < nodes.len() {
        let wave: Vec<&str> = nodes.iter().filter(|(id, n)| !done.contains(id.as_str()) && n.deps.iter().all(|d| done.contains(d.as_str()))).map(|(id, _)| id.as_str()).collect();
        if wave.is_empty() {
            let mut at = nodes.keys().find(|id| !done.contains(id.as_str())).unwrap().as_str();
            let mut seen = vec![at];
            while let Some(next) = nodes[at].deps.iter().map(String::as_str).find(|d| !done.contains(d)) {
                if let Some(i) = seen.iter().position(|s| *s == next) {
                    let mut cycle: Vec<&str> = seen[i..].to_vec();
                    cycle.push(next);
                    return r.fail("dag", "", format!("dependency cycle: {}", cycle.join(" -> ")));
                }
                seen.push(next);
                at = next;
            }
            return r.fail("dag", "", "dependency cycle");
        }
        done.extend(wave.iter().copied());
        waves.push(wave);
    }
    // Longest weighted path, walking the waves in order.
    let mut best: BTreeMap<&str, (u64, Option<&str>)> = BTreeMap::new();
    for id in waves.iter().flatten() {
        let n = &nodes[*id];
        let from = n.deps.iter().map(|d| (best[d.as_str()].0, d.as_str())).max();
        best.insert(id, (from.map(|f| f.0).unwrap_or(0) + n.weight, from.map(|f| f.1)));
    }
    let (end, (total, _)) = best.iter().max_by_key(|(id, (w, _))| (*w, std::cmp::Reverse(**id))).map(|(i, v)| (*i, *v)).unwrap();
    let mut path = vec![end];
    while let Some(prev) = best[path.last().unwrap()].1 {
        path.push(prev);
    }
    path.reverse();
    r.push(Level::Ok, "dag", "", format!("{} node(s) in {} wave(s): {}", nodes.len(), waves.len(), waves.iter().map(|w| w.join(" ")).collect::<Vec<_>>().join(" | ")));
    r.push(Level::Ok, "dag", "", format!("critical path (weight {}): {}", total, path.join(" -> ")));
}

// ── review-md: the rubric budget ────────────────────────────────────────────

fn review_md(text: &str, r: &mut Report) {
    let lines = text.lines().count();
    if lines >= REVIEW_MD_CAP {
        r.fail("review-md", "", format!("{} lines; the budget is under {} (promote twice-violated rules to checks and prune)", lines, REVIEW_MD_CAP));
    } else if lines * 10 >= REVIEW_MD_CAP * 9 {
        r.warn("review-md", "", format!("{} lines; within 10% of the {}-line budget", lines, REVIEW_MD_CAP));
    }
    let id = Regex::new(r"^\s*[-*]\s+\*\*`\[(REV-[A-Z]+-\d+)\]`").unwrap();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = id.captures(line) {
            if let Some(first) = seen.insert(c[1].to_string(), i + 1) {
                r.fail("review-md", format!("line {}", i + 1), format!("rule `{}` is defined twice (first at line {})", &c[1], first));
            }
        }
    }
}

// ── spec: the structure an Architect owes a spec ────────────────────────────

fn spec(text: &str, r: &mut Report) {
    let doc = match frontmatter::parse(text) {
        Ok(d) => d,
        Err(e) => return r.fail("spec", "", e),
    };
    match &doc.frontmatter {
        None => r.fail("spec", "", "missing provenance frontmatter (specs/001 §5)"),
        Some(fm) => each("spec", frontmatter::check_provenance(fm), r),
    }
    let headings: Vec<String> = doc.body.lines().filter(|l| l.starts_with('#')).map(|l| l.to_lowercase()).collect();
    for (needle, name) in SPEC_SECTIONS {
        if !headings.iter().any(|h| h.contains(needle)) {
            r.fail("spec", "", format!("missing a `{}` section", name));
        }
    }
}

// ── kb: stale claims and broken links ───────────────────────────────────────

fn kb(root: &Path, r: &mut Report) -> Result<(), String> {
    let tree = lint::Tree::from_repo(root).map_err(|e| format!("cannot read {}: {}", root.display(), e))?;
    r.findings.extend(lint::run(&tree).findings.into_iter().filter(|f| ["links", "frontmatter", "rule-caps"].contains(&f.check.as_str())));
    let superseded: BTreeSet<&String> = tree
        .files
        .iter()
        .filter(|(k, p)| k.ends_with(".md") && fs::read_to_string(p).ok().and_then(|t| lint::frontmatter_of(&t)).map(|fm| get_str(&fm, "status") == Some("superseded")).unwrap_or(false))
        .map(|(k, _)| k)
        .collect();
    let link = Regex::new(r"\]\(([^)\s#]+)").unwrap();
    for (path, phys) in tree.files.iter().filter(|(k, _)| k.ends_with(".md") && !superseded.contains(k)) {
        let dir = path.rsplit_once('/').map(|d| d.0).unwrap_or("");
        let text = fs::read_to_string(phys).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            for c in link.captures_iter(line) {
                let joined = if dir.is_empty() { c[1].to_string() } else { format!("{}/{}", dir, &c[1]) };
                if let Some(n) = lint::normalize(&joined) {
                    if superseded.contains(&n) {
                        r.warn("kb", format!("{}:{}", path, i + 1), format!("links to `{}`, which is superseded", n));
                    }
                }
            }
        }
    }
    Ok(())
}

// ── tui: character-cell invariants (skills/tui-design) ──────────────────────

fn is_emoji(c: char) -> bool {
    // ✓ ✔ ✗ ✖ and the ❯ prompt are the sanctioned glyphs; other pictographs vary in width by terminal.
    !matches!(c, '\u{2713}' | '\u{2714}' | '\u{2717}' | '\u{2716}' | '\u{276F}') && matches!(c, '\u{1F300}'..='\u{1FAFF}' | '\u{2600}'..='\u{27BF}' | '\u{2B50}' | '\u{2B55}' | '\u{FE0F}')
}

fn tui(target: &Path, r: &mut Report) -> Result<(), String> {
    let files: Vec<_> = if target.is_dir() {
        walkdir::WalkDir::new(target).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "rs")).map(|e| e.into_path()).collect()
    } else {
        vec![target.to_path_buf()]
    };
    let badge = Regex::new("⟦[^⟧]*⟧").unwrap();
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        for (i, line) in text.lines().enumerate() {
            let at = format!("{}:{}", path.display(), i + 1);
            if let Some(c) = line.chars().find(|c| is_emoji(*c)) {
                r.fail("tui", &at, format!("emoji `{}` (INV zero-emoji: width varies by terminal; use ○ ● ► ❯ ✔ ✖)", c));
            }
            if let Some(c) = line.chars().find(|c| matches!(c, '\u{258C}' | '\u{2590}')) {
                r.fail("tui", &at, format!("vertical half-block `{}` (INV-AVATAR-01)", c));
            }
            let t = line.trim();
            if (t.contains('┌') && t.contains('┐')) || (t.contains('└') && t.contains('┘')) {
                r.fail("tui", &at, "box border enclosing a region (borderless layout; use whitespace and `─` hairlines)");
            }
            // Only literal badges: a format template (`⟦{}⟧`) has no width until it is filled.
            for b in badge.find_iter(line).filter(|b| !b.as_str().contains(['{', '}', '\'', '"', '\\'])) {
                let w = UnicodeWidthStr::width(b.as_str());
                if w != 5 {
                    r.fail("tui", &at, format!("badge `{}` is {} cells wide (INV-AVATAR-05: exactly 5)", b.as_str(), w));
                }
            }
        }
    }
    Ok(())
}

// ── skill: a package an agent or operator wrote ─────────────────────────────

fn skill(dir: &Path, r: &mut Report) -> Result<(), String> {
    let text = read(&dir.join("SKILL.md"))?;
    let doc = frontmatter::parse(&text).map_err(|e| format!("SKILL.md: {}", e))?;
    let Some(fm) = doc.frontmatter else {
        r.fail("skill", "SKILL.md", "missing frontmatter");
        return Ok(());
    };
    let name = dir.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
    let custom = dir.canonicalize().map(|p| p.to_string_lossy().contains("/custom/") || p.to_string_lossy().contains("/operator/skills/")).unwrap_or(false);
    each("skill", lint::skill_errors(&fm, &name, custom), r);
    each("skill", lint::skill_package_errors(&doc.body, &fm, &|rel| dir.join(rel).is_file()), r);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(name);
        fs::write(&p, body).unwrap();
        (d, p)
    }

    fn fails(kind: &str, body: &str) -> Vec<String> {
        let (_d, p) = tmp("f", body);
        run(kind, &p, None).unwrap().findings.into_iter().filter(|f| f.level == Level::Fail).map(|f| f.message).collect()
    }

    fn infos(kind: &str, body: &str) -> Vec<String> {
        let (_d, p) = tmp("f", body);
        run(kind, &p, None).unwrap().findings.into_iter().filter(|f| f.level == Level::Ok).map(|f| f.message).collect()
    }

    const DAG: &str = "tasks:\n  - {id: a, tier: code-craft, weight: 2}\n  - {id: b, tier: code-craft, depends_on: [a], weight: 3}\n  - {id: c, tier: fast-triage, depends_on: [a]}\n  - {id: d, tier: deep-reasoning, depends_on: [b, c]}\n";

    #[test]
    fn dag_orders_and_finds_the_critical_path() {
        assert!(fails("dag", DAG).is_empty());
        let out = infos("dag", DAG).join("\n");
        assert!(out.contains("a | b c | d"), "{}", out);
        assert!(out.contains("weight 6): a -> b -> d"), "{}", out);
    }

    #[test]
    fn dag_rejects_cycles_unknowns_and_bad_tiers() {
        let cyc = "tasks:\n  - {id: a, tier: code-craft, depends_on: [b]}\n  - {id: b, tier: code-craft, depends_on: [a]}\n";
        assert!(fails("dag", cyc).iter().any(|m| m.contains("cycle: a -> b -> a")), "{:?}", fails("dag", cyc));
        assert!(fails("dag", "tasks:\n  - {id: a, tier: code-craft, depends_on: [zzz]}\n")[0].contains("unknown id `zzz`"));
        assert!(fails("dag", "tasks:\n  - {id: a, tier: code-craft, depends_on: [a]}\n")[0].contains("itself"));
        assert!(fails("dag", "tasks:\n  - {id: a, tier: smart}\n")[0].contains("tier"));
        assert!(fails("dag", "tasks:\n  - {id: a, tier: fast-triage}\n  - {id: a, tier: fast-triage}\n")[0].contains("duplicate"));
        assert!(fails("dag", "tasks: []\n")[0].contains("non-empty"));
        assert!(fails("dag", "tasks:\n  - {id: a, tier: fast-triage, spec: notes.md}\n")[0].contains("specs/*.md"));
    }

    #[test]
    fn dag_checks_spec_links_against_the_repo() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("specs")).unwrap();
        fs::write(d.path().join("specs/001-x.md"), "x").unwrap();
        let (_t, p) = tmp("plan.yaml", "milestones:\n  - {id: m1, spec: 'specs/001-x.md#L3'}\n  - {id: m2, spec: specs/999-y.md}\n");
        let r = run("dag", &p, Some(d.path())).unwrap();
        let f: Vec<_> = r.findings.iter().filter(|f| f.level == Level::Fail).collect();
        assert_eq!(f.len(), 1, "{:?}", f);
        assert!(f[0].message.contains("999-y"));
    }

    #[test]
    fn review_md_budget_and_duplicate_ids() {
        assert!(fails("review-md", "# R\n- **`[REV-SEC-01]`** a\n").is_empty());
        assert!(fails("review-md", &"line\n".repeat(300))[0].contains("300"));
        assert!(fails("review-md", "- **`[REV-SEC-01]`** a\n- **`[REV-SEC-01]`** b\n")[0].contains("defined twice"));
    }

    #[test]
    fn spec_needs_provenance_and_sections() {
        let fm = "---\nid: s\ntitle: S\nauthor: architect\ntimestamp: \"2026-10-25T00:00:00Z\"\nsource_type: internal_generated\nconfidence: medium\nstatus: draft\nsource_mission: m\ntags: [spec]\n---\n";
        let good = format!("{}## 1. Scope\n### 1.1 Simpler Alternative Considered\n## 9. Verification Plan\n", fm);
        assert!(fails("spec", &good).is_empty(), "{:?}", fails("spec", &good));
        let bare = format!("{}## 1. Scope\n", fm);
        assert_eq!(fails("spec", &bare).len(), 2);
        assert!(fails("spec", "## 1. Scope\n")[0].contains("frontmatter"));
    }

    #[test]
    fn tui_invariants() {
        assert!(fails("tui", "let s = \"⟦•─•⟧ ○ ● ► ❯ ✔ ✖\";\n").is_empty());
        assert!(fails("tui", "let s = \"🤖 ready\";\n")[0].contains("emoji"));
        assert!(fails("tui", "let s = \"▌body\";\n")[0].contains("INV-AVATAR-01"));
        assert!(fails("tui", "let s = \"⟦••⟧\";\n")[0].contains("exactly 5"));
        assert!(fails("tui", "let top = \"┌────┐\";\n")[0].contains("border"));
        assert!(fails("tui", "let s = \"⭐ ok\";\n")[0].contains("emoji"));
    }

    #[test]
    fn provenance_handoff_and_rules_delegate_to_lint() {
        assert!(fails("provenance", "no frontmatter\n")[0].contains("missing"));
        assert!(!fails("handoff", "handoff_id: x\n").is_empty());
        assert!(!fails("rules", "---\nactive_rules: 1\n---\n### r\nbody\n").is_empty());
    }

    #[test]
    fn kb_flags_broken_links_and_superseded_targets() {
        let d = tempfile::tempdir().unwrap();
        let fm = |status: &str| format!("---\nid: x\ntitle: X\nauthor: architect\ntimestamp: \"2026-10-25T00:00:00Z\"\nsource_type: internal_generated\nconfidence: medium\nstatus: {}\nsuperseded_by: new\nsource_mission: m\ntags: [t]\n---\n", status);
        fs::create_dir(d.path().join("context")).unwrap();
        fs::write(d.path().join("context/old.md"), fm("superseded")).unwrap();
        fs::write(d.path().join("context/new.md"), format!("{}see [old](old.md) and [gone](gone.md)\n", fm("active"))).unwrap();
        let r = run("kb", d.path(), None).unwrap();
        assert!(r.findings.iter().any(|f| f.level == Level::Fail && f.message.contains("broken link")));
        assert!(r.findings.iter().any(|f| f.level == Level::Warn && f.message.contains("superseded")));
    }

    /// The shipped templates are the starting point agents copy, so each must pass the check it is paired with.
    #[test]
    fn shipped_templates_pass_their_checks() {
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (kind, file) in [
            ("dag", "task-decomposition/templates/task-dag.yaml"),
            ("dag", "roadmap-sequencing/templates/milestones.yaml"),
            ("handoff", "handoff-packaging/templates/handoff-envelope.yaml"),
            ("spec", "spec-authoring/templates/spec-skeleton.md"),
        ] {
            let r = run(kind, &skills.join(file), Some(&repo)).unwrap();
            let bad: Vec<_> = r.findings.iter().filter(|f| f.level == Level::Fail).map(|f| f.message.clone()).collect();
            assert!(bad.is_empty(), "{}: {:?}", file, bad);
        }
        let rule = fs::read_to_string(skills.join("lesson-promotion/templates/lesson-rule.md")).unwrap();
        let body = rule.split("<!--").next().unwrap();
        let file = format!("---\nactive_rules: 1\n---\n{}", body);
        assert!(lint::cap_errors(&file).is_empty(), "{:?}", lint::cap_errors(&file));
    }

    #[test]
    fn every_built_in_skill_package_is_valid() {
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let mut n = 0;
        for e in fs::read_dir(&skills).unwrap().flatten().filter(|e| e.path().join("SKILL.md").is_file()) {
            let r = run("skill", &e.path(), None).unwrap();
            let bad: Vec<_> = r.findings.iter().filter(|f| f.level == Level::Fail).map(|f| f.message.clone()).collect();
            assert!(bad.is_empty(), "{}: {:?}", e.path().display(), bad);
            n += 1;
        }
        assert!(n >= 43, "expected the full catalog, found {} packages", n);
    }

    #[test]
    fn unknown_kind_is_an_error() {
        assert!(run("nope", Path::new("."), None).is_err());
    }
}
