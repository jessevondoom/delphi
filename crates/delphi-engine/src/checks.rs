//! The deterministic checks behind the built-in skills (`skills/CATALOG.md`, D-02) that judge one document's text.
//!
//! One implementation serves two callers: `delphi check <kind> <path>` on the command line, and the `check` tool an
//! agent calls through the engine (so a role with no shell still gets the verdict). A check takes text and returns
//! an [`Outcome`]; it never touches the filesystem itself, so the tool can feed it a file it read through the path
//! guard, with the read's trust tier already recorded.

use crate::frontmatter::{self, get_str, ROLES};
use regex::Regex;
use serde_yaml::{Mapping, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The kinds that judge a single document's text; `delphi check` adds directory kinds on top (`kb`, `tui`, `skill`).
pub const TEXT_KINDS: [&str; 6] = ["dag", "handoff", "provenance", "rules", "review-md", "spec"];

const COGNITIVE: [&str; 4] = ["deep-reasoning", "code-craft", "high-context-discovery", "fast-triage"];
const REVIEW_MD_CAP: usize = 300;
const SPEC_SECTIONS: [(&str, &str); 3] = [("scope", "Scope"), ("simpler alternative", "Simpler Alternative Considered"), ("verification plan", "Verification Plan")];
const ENVELOPE_BYTES: usize = 8000; // 2,000 tokens at bytes / 4
const RULE_CHARS: usize = 350;
const RULE_COUNT: usize = 25;
const RULE_BYTES: usize = 6144;

/// What a check found. `fails` make it fail; `warns` do not; `notes` are facts worth reporting on success.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub fails: Vec<String>,
    pub warns: Vec<String>,
    pub notes: Vec<String>,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        self.fails.is_empty()
    }

    fn fail(&mut self, at: &str, msg: impl AsRef<str>) {
        self.fails.push(if at.is_empty() { msg.as_ref().to_string() } else { format!("{}: {}", at, msg.as_ref()) });
    }

    /// The verdict as the compact text an agent reads: one line per finding, a header that says pass or fail.
    pub fn render(&self, kind: &str, target: &str) -> String {
        let mut out = format!("{} {}: {}\n", kind, target, if self.passed() { "pass" } else { "FAIL" });
        for (tag, items) in [("fail", &self.fails), ("warn", &self.warns), ("note", &self.notes)] {
            for m in items {
                out.push_str(&format!("[{}] {}\n", tag, m));
            }
        }
        out
    }
}

/// Run a text check. `spec_exists` answers whether a `specs/…` path a task links to is real (`dag` only).
pub fn check_text(kind: &str, text: &str, spec_exists: &dyn Fn(&str) -> bool) -> Result<Outcome, String> {
    let mut o = Outcome::default();
    match kind {
        "dag" => dag(text, spec_exists, &mut o),
        "handoff" => o.fails = envelope_errors(text),
        "provenance" => o.fails = provenance(text),
        "rules" => o.fails = cap_errors(text),
        "review-md" => review_md(text, &mut o),
        "spec" => spec(text, &mut o),
        other => return Err(format!("unknown check `{}` (one of: {})", other, TEXT_KINDS.join(", "))),
    }
    Ok(o)
}

pub fn provenance(text: &str) -> Vec<String> {
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
/// On success notes the parallel waves and the weighted critical path.
fn dag(text: &str, spec_exists: &dyn Fn(&str) -> bool, o: &mut Outcome) {
    let root: Value = match serde_yaml::from_str(text) {
        Ok(v) => v,
        Err(e) => return o.fail("", format!("not valid YAML: {}", e)),
    };
    let (key, items) = match ["tasks", "milestones"].iter().find_map(|k| root.get(*k).and_then(|v| v.as_sequence()).map(|s| (*k, s))) {
        Some(x) if !x.1.is_empty() => x,
        _ => return o.fail("", "needs a non-empty `tasks:` or `milestones:` list"),
    };
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(m) = item.as_mapping() else {
            o.fail(&format!("{}[{}]", key, i), "entry must be a mapping");
            continue;
        };
        let id = get_str(m, "id").unwrap_or("").trim().to_string();
        if id.is_empty() {
            o.fail(&format!("{}[{}]", key, i), "missing `id`");
            continue;
        }
        if key == "tasks" && !get_str(m, "tier").is_some_and(|t| COGNITIVE.contains(&t)) {
            o.fail(&id, format!("`tier` must be one of {:?}", COGNITIVE));
        }
        if let Some(spec) = get_str(m, "spec") {
            let path = spec.split('#').next().unwrap_or("");
            if !path.starts_with("specs/") || !path.ends_with(".md") {
                o.fail(&id, format!("spec link `{}` must be a `specs/*.md` path", spec));
            } else if !spec_exists(path) {
                o.fail(&id, format!("spec `{}` does not exist", path));
            }
        }
        let deps: Vec<String> = match m.get(Value::String("depends_on".into())) {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => match v.as_sequence().map(|s| s.iter().map(|x| x.as_str().map(String::from)).collect::<Option<Vec<_>>>()) {
                Some(Some(d)) => d,
                _ => {
                    o.fail(&id, "`depends_on` must be a list of ids");
                    Vec::new()
                }
            },
        };
        let weight = m.get(Value::String("weight".into())).map(|w| w.as_u64()).unwrap_or(Some(1));
        if weight.is_none() {
            o.fail(&id, "`weight` must be a non-negative integer");
        }
        if nodes.insert(id.clone(), Node { deps, weight: weight.unwrap_or(1) }).is_some() {
            o.fail(&id, "duplicate id");
        }
    }
    for (id, n) in &nodes {
        for d in &n.deps {
            if d == id {
                o.fail(id, "depends on itself");
            } else if !nodes.contains_key(d) {
                o.fail(id, format!("depends on unknown id `{}`", d));
            }
        }
    }
    if !o.fails.is_empty() {
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
                    return o.fail("", format!("dependency cycle: {}", cycle.join(" -> ")));
                }
                seen.push(next);
                at = next;
            }
            return o.fail("", "dependency cycle");
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
    o.notes.push(format!("{} node(s) in {} wave(s): {}", nodes.len(), waves.len(), waves.iter().map(|w| w.join(" ")).collect::<Vec<_>>().join(" | ")));
    o.notes.push(format!("critical path (weight {}): {}", total, path.join(" -> ")));
}

// ── review-md: the rubric budget ────────────────────────────────────────────

fn review_md(text: &str, o: &mut Outcome) {
    let lines = text.lines().count();
    if lines >= REVIEW_MD_CAP {
        o.fail("", format!("{} lines; the budget is under {} (promote twice-violated rules to checks and prune)", lines, REVIEW_MD_CAP));
    } else if lines * 10 >= REVIEW_MD_CAP * 9 {
        o.warns.push(format!("{} lines; within 10% of the {}-line budget", lines, REVIEW_MD_CAP));
    }
    let id = Regex::new(r"^\s*[-*]\s+\*\*`\[(REV-[A-Z]+-\d+)\]`").unwrap();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(c) = id.captures(line) {
            if let Some(first) = seen.insert(c[1].to_string(), i + 1) {
                o.fail(&format!("line {}", i + 1), format!("rule `{}` is defined twice (first at line {})", &c[1], first));
            }
        }
    }
}

// ── spec: the structure an Architect owes a spec ────────────────────────────

fn spec(text: &str, o: &mut Outcome) {
    let doc = match frontmatter::parse(text) {
        Ok(d) => d,
        Err(e) => return o.fail("", e),
    };
    match &doc.frontmatter {
        None => o.fail("", "missing provenance frontmatter (specs/001 §5)"),
        Some(fm) => o.fails.extend(frontmatter::check_provenance(fm)),
    }
    let headings: Vec<String> = doc.body.lines().filter(|l| l.starts_with('#')).map(|l| l.to_lowercase()).collect();
    for (needle, name) in SPEC_SECTIONS {
        if !headings.iter().any(|h| h.contains(needle)) {
            o.fail("", format!("missing a `{}` section", name));
        }
    }
}

// ── handoff envelopes (specs/001 §4) ────────────────────────────────────────

pub fn envelope_errors(text: &str) -> Vec<String> {
    let mut errs = Vec::new();
    if text.len() > ENVELOPE_BYTES {
        errs.push(format!("envelope is {} bytes (cap {}, ~2,000 tokens)", text.len(), ENVELOPE_BYTES));
    }
    let m: Mapping = match serde_yaml::from_str(text) {
        Ok(m) => m,
        Err(e) => return vec![format!("not a YAML mapping: {}", e)],
    };
    for k in ["handoff_id", "mission_id", "parent_span_id", "caller_agent", "target_agent", "objective"] {
        if get_str(&m, k).map(|v| v.trim().is_empty()).unwrap_or(true) {
            errs.push(format!("missing `{}`", k));
        }
    }
    for k in ["caller_agent", "target_agent"] {
        if let Some(a) = get_str(&m, k) {
            if !ROLES.contains(&a) {
                errs.push(format!("{} `{}` is not a canonical role", k, a));
            }
        }
    }
    let anchor = Regex::new(r"^#L\d+(-L\d+)?$").unwrap();
    match m.get(Value::String("context_pointers".into())).map(|v| v.as_sequence()) {
        None | Some(None) => errs.push("context_pointers must be a list (pointer-based context, specs/001 §4)".into()),
        Some(Some(ptrs)) => {
            for p in ptrs {
                let pm = p.as_mapping();
                let uri = pm.and_then(|x| get_str(x, "uri")).unwrap_or("");
                let (file, frag) = match uri.find('#') {
                    Some(i) => (&uri[..i], &uri[i..]),
                    None => (uri, ""),
                };
                if delphi_core::pathguard::canonical(file).is_err() {
                    errs.push(format!("context pointer `{}` is not a workspace-relative logical path", uri));
                }
                if !anchor.is_match(frag) {
                    errs.push(format!("context pointer `{}` needs a `#Lstart[-Lend]` line anchor", uri));
                }
                for k in ["heading", "blake3", "reason"] {
                    if pm.and_then(|x| get_str(x, k)).map(|v| v.trim().is_empty()).unwrap_or(true) {
                        errs.push(format!("context pointer `{}` lacks `{}`", uri, k));
                    }
                }
            }
        }
    }
    match m.get(Value::String("expected_output".into())).and_then(|v| v.as_mapping()) {
        Some(eo) if get_str(eo, "artifact_type").is_some() => {}
        _ => errs.push("expected_output.artifact_type is required".into()),
    }
    if let Some(rd) = m.get(Value::String("routing_directive".into())).and_then(|v| v.as_mapping()) {
        if get_str(rd, "model_tier").is_none() {
            errs.push("routing_directive.model_tier is required (tiers, never model IDs)".into());
        }
        for k in ["model", "model_id", "endpoint", "fallback_endpoint"] {
            if rd.contains_key(Value::String(k.into())) {
                errs.push(format!("routing_directive.{} is not allowed: models resolve from config tiers and failover follows ratified [access] order", k));
            }
        }
    }
    errs
}

// ── Rule of 25 caps (specs/004 §2.2, D-09) ──────────────────────────────────

pub fn cap_errors(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    // The entry-metadata lines (`specs/050` ML-1, OD-2) are metadata: they do not count toward the byte cap.
    let meta_bytes = if let Some(rest) = text.strip_prefix("---\n") {
        let head_end = rest.find("\n---\n").map_or(0, |i| i + 4);
        text[..head_end].lines().filter(|l| crate::frontmatter::ENTRY_META_KEYS.iter().any(|k| l.strip_prefix(k).is_some_and(|r| r.starts_with(':')))).map(|l| l.len() + 1).sum()
    } else {
        0
    };
    if text.len() - meta_bytes > RULE_BYTES {
        out.push(format!("{} bytes exceeds the {}-byte cap", text.len() - meta_bytes, RULE_BYTES));
    }
    let doc = match frontmatter::parse(text) {
        Ok(d) => d,
        Err(e) => return vec![e],
    };
    // A rule is a `### ` heading plus every non-blank line until the next `#`/`##`/`###` heading.
    let mut rules: Vec<(String, Vec<String>)> = Vec::new();
    let mut open = false;
    for line in doc.body.lines() {
        if let Some(h) = line.strip_prefix("### ") {
            rules.push((h.to_string(), Vec::new()));
            open = true;
        } else if line.starts_with('#') {
            open = false;
        } else if open && !line.trim().is_empty() {
            if let Some(last) = rules.last_mut() {
                last.1.push(line.to_string());
            }
        }
    }
    if rules.len() > RULE_COUNT {
        out.push(format!("{} rules exceeds the Rule of {}", rules.len(), RULE_COUNT));
    }
    match doc.frontmatter.as_ref().and_then(|fm| fm.get(Value::String("active_rules".into()))).and_then(|v| v.as_u64()) {
        Some(n) if n as usize == rules.len() => {}
        Some(n) => out.push(format!("frontmatter active_rules is {} but the file has {} rules", n, rules.len())),
        None => out.push("frontmatter must declare `active_rules`".into()),
    }
    for (head, lines) in &rules {
        let block = lines.join("\n");
        let chars = block.chars().count();
        if chars > RULE_CHARS {
            out.push(format!("rule `{}` is {} characters (cap {})", head, chars, RULE_CHARS));
        }
        for tag in ["**T:**", "**D:**", "**F:**"] {
            if !block.contains(tag) {
                out.push(format!("rule `{}` lacks a {} line", head, tag));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(kind: &str, text: &str) -> Outcome {
        check_text(kind, text, &|_| true).unwrap()
    }

    const DAG: &str = "tasks:\n  - {id: a, tier: code-craft, weight: 2}\n  - {id: b, tier: code-craft, depends_on: [a], weight: 3}\n  - {id: c, tier: fast-triage, depends_on: [a]}\n  - {id: d, tier: deep-reasoning, depends_on: [b, c]}\n";

    #[test]
    fn dag_orders_and_finds_the_critical_path() {
        let o = run("dag", DAG);
        assert!(o.passed(), "{:?}", o);
        let notes = o.notes.join("\n");
        assert!(notes.contains("a | b c | d"), "{}", notes);
        assert!(notes.contains("weight 6): a -> b -> d"), "{}", notes);
    }

    #[test]
    fn dag_rejects_cycles_unknowns_and_bad_tiers() {
        let cyc = "tasks:\n  - {id: a, tier: code-craft, depends_on: [b]}\n  - {id: b, tier: code-craft, depends_on: [a]}\n";
        assert!(run("dag", cyc).fails.iter().any(|m| m.contains("cycle: a -> b -> a")), "{:?}", run("dag", cyc));
        assert!(run("dag", "tasks:\n  - {id: a, tier: code-craft, depends_on: [zzz]}\n").fails[0].contains("unknown id `zzz`"));
        assert!(run("dag", "tasks:\n  - {id: a, tier: code-craft, depends_on: [a]}\n").fails[0].contains("itself"));
        assert!(run("dag", "tasks:\n  - {id: a, tier: smart}\n").fails[0].contains("tier"));
        assert!(run("dag", "tasks:\n  - {id: a, tier: fast-triage}\n  - {id: a, tier: fast-triage}\n").fails[0].contains("duplicate"));
        assert!(run("dag", "tasks: []\n").fails[0].contains("non-empty"));
        assert!(run("dag", "tasks:\n  - {id: a, tier: fast-triage, spec: notes.md}\n").fails[0].contains("specs/*.md"));
    }

    #[test]
    fn dag_asks_the_caller_whether_a_linked_spec_exists() {
        let text = "milestones:\n  - {id: m1, spec: 'specs/001-x.md#L3'}\n  - {id: m2, spec: specs/999-y.md}\n";
        let o = check_text("dag", text, &|p| p == "specs/001-x.md").unwrap();
        assert_eq!(o.fails.len(), 1, "{:?}", o);
        assert!(o.fails[0].contains("999-y"));
    }

    #[test]
    fn review_md_budget_and_duplicate_ids() {
        assert!(run("review-md", "# R\n- **`[REV-SEC-01]`** a\n").passed());
        assert!(run("review-md", &"line\n".repeat(300)).fails[0].contains("300"));
        assert!(run("review-md", "- **`[REV-SEC-01]`** a\n- **`[REV-SEC-01]`** b\n").fails[0].contains("defined twice"));
        assert_eq!(run("review-md", &"line\n".repeat(280)).warns.len(), 1);
    }

    #[test]
    fn spec_needs_provenance_and_sections() {
        let fm = "---\nid: s\ntitle: S\nauthor: architect\ntimestamp: \"2026-10-25T00:00:00Z\"\nsource_type: internal_generated\nconfidence: medium\nstatus: draft\nsource_mission: m\ntags: [spec]\n---\n";
        let good = format!("{}## 1. Scope\n### 1.1 Simpler Alternative Considered\n## 9. Verification Plan\n", fm);
        assert!(run("spec", &good).passed(), "{:?}", run("spec", &good));
        assert_eq!(run("spec", &format!("{}## 1. Scope\n", fm)).fails.len(), 2);
        assert!(run("spec", "## 1. Scope\n").fails[0].contains("frontmatter"));
    }

    #[test]
    fn provenance_handoff_and_rules() {
        assert!(run("provenance", "no frontmatter\n").fails[0].contains("missing"));
        assert!(!run("handoff", "handoff_id: x\n").passed());
        assert!(!run("rules", "---\nactive_rules: 1\n---\n### r\nbody\n").passed());
    }

    #[test]
    fn entry_metadata_does_not_count_toward_the_byte_cap() {
        let pad = "p".repeat(300);
        let rule = |n: usize| (0..n).map(|i| format!("### `[H-{i}]` X\n- **T:** {pad}\n- **D:** b\n- **F:** c\n")).collect::<String>();
        let bare = format!("---\nactive_rules: 17\n---\n{}", rule(17));
        assert!(bare.len() > 5600 && bare.len() < 6144, "{}", bare.len());
        let meta = "kind: procedure\ncreated: 2026-10-01\nlast_confirmed: 2026-10-20\nvalid_until: 2027-01-01\n".repeat(8);
        let with = bare.replacen("active_rules: 17\n", &format!("active_rules: 17\n{}", meta), 1);
        assert!(with.len() > 6144 && cap_errors(&with).iter().all(|e| !e.contains("-byte cap")), "metadata is not counted: {:?}", cap_errors(&with));
        assert!(cap_errors(&format!("{}{}", bare, "z".repeat(600))).iter().any(|e| e.contains("-byte cap")), "prose still is");
    }

    #[test]
    fn caps() {
        let ok = "---\nactive_rules: 1\n---\n### `[H-1]` X\n- **T:** a\n- **D:** b\n- **F:** c\n";
        assert!(cap_errors(ok).is_empty());
        let long = ok.replace("- **D:** b", &format!("- **D:** {}", "x".repeat(400)));
        assert!(!cap_errors(&long).is_empty());
        assert!(!cap_errors(&ok.replace("active_rules: 1", "active_rules: 2")).is_empty());
        // Prose (not only `- ` bullets) counts toward the cap.
        assert!(!cap_errors(&format!("{}{}\n", ok, "y".repeat(400))).is_empty());
        // Bullets under a later `## ` heading don't count toward the last rule.
        assert!(cap_errors(&format!("{}## Notes\n- {}\n", ok, "z".repeat(400))).is_empty());
        assert!(!cap_errors(&ok.replace("active_rules: 1\n", "")).is_empty());
    }

    #[test]
    fn unknown_kind_is_an_error() {
        assert!(check_text("nope", "", &|_| true).is_err());
    }

    #[test]
    fn render_is_compact() {
        let o = run("dag", DAG);
        let s = o.render("dag", "plan.yaml");
        assert!(s.starts_with("dag plan.yaml: pass\n") && s.contains("[note] critical path"), "{}", s);
        assert!(run("dag", "tasks: []").render("dag", "p").contains("FAIL"));
    }
}
