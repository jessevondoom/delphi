//! What an operator is shown before and after adding a bundle (`specs/024` §4.2 step 5), and the identity the install
//! computes for it. Pure: it reads the tree and decides nothing.

use crate::ids::{body_of, frontmatter_of, get_str};
use crate::manifest::Manifest;
use crate::tree::Tree;
use crate::{files, Host, Options, Report};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;

/// The first lines of `README.md` shown at add; the rest is named by path (`specs/024` §4.2).
pub const README_LINES: usize = 40;

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub readme_head: Vec<String>,
    pub readme_more: usize,
    pub knowledge_files: usize,
    pub knowledge_bytes: u64,
    /// The `sources/` section: ids with licence and sizes, never text (`specs/023` §2.1).
    pub sources: Vec<crate::sources::SourceSummary>,
    pub skills: Vec<(String, Vec<String>)>,
    /// (agent, host role) with the tools the host would drop.
    pub agents: Vec<(String, Option<String>, Vec<String>)>,
    pub guidance_roles: Vec<String>,
    pub goals: usize,
    pub perspectives: usize,
    pub rules: usize,
    pub policy_entries: usize,
    pub vector_caches: Vec<String>,
    /// The text of `.delphi-bundle/eval.json` when the publisher recorded one (`specs/027` §7.1); shown, never trusted.
    pub eval_record: Option<String>,
}

/// A validated look at a bundle directory: the report, the identity the install computes, and the summary.
pub struct Inspected {
    pub report: Report,
    pub manifest: Option<Manifest>,
    /// `blake3:<hex>` of `files.json`: the one in the tree when it is there and right, else the one the packager would write.
    pub hash: Option<String>,
    /// The exact bytes of that `files.json`, for the cache entry.
    pub files_json: Option<String>,
    pub ids: BTreeSet<String>,
    pub summary: Summary,
}

pub fn inspect(dir: &Path, opts: &Options, host: &dyn Host) -> Inspected {
    let mut report = crate::validate(dir, opts, host);
    let manifest = report.manifest.clone();
    let mut scratch = Report::default();
    let tree = crate::tree::scan(dir, &mut scratch);
    let (hash, files_json) = match (&manifest, tree.text(".delphi-bundle/files.json")) {
        (_, Some(json)) if report.passed() => (Some(files::bundle_hash(json.as_bytes())), Some(json)),
        (Some(m), _) if report.passed() => {
            let json = files::build_files_json(&tree, m);
            (Some(files::bundle_hash(json.as_bytes())), Some(json))
        }
        _ => (None, None),
    };
    if !report.passed() {
        report.bundle_hash = None;
    }
    let ids = crate::ids::collect(&tree);
    let summary = summarize(&tree, manifest.as_ref(), host);
    Inspected { report, manifest, hash, files_json, ids, summary }
}

fn summarize(tree: &Tree, m: Option<&Manifest>, host: &dyn Host) -> Summary {
    let mut s = Summary::default();
    if let Some(text) = tree.text("README.md") {
        let lines: Vec<&str> = text.lines().collect();
        s.readme_head = lines.iter().take(README_LINES).map(|l| l.to_string()).collect();
        s.readme_more = lines.len().saturating_sub(README_LINES);
    }
    for e in tree.under("knowledge/").filter(|e| e.path.ends_with(".md")) {
        s.knowledge_files += 1;
        s.knowledge_bytes += e.bytes;
    }
    s.sources = crate::sources::list(tree);
    for name in crate::skills::skill_names(tree) {
        let roles = tree
            .text(&format!("skills/{}/SKILL.md", name))
            .and_then(|t| frontmatter_of(&t))
            .and_then(|fm| crate::skills::delphi_field(&fm, "authorized_agents"))
            .and_then(|v| v.as_sequence().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()))
            .unwrap_or_default();
        s.skills.push((name, roles));
    }
    for e in tree.under("agents/").filter(|e| e.path.ends_with(".md")) {
        let stem = e.path.trim_start_matches("agents/").trim_end_matches(".md").to_string();
        let fm = tree.text(&e.path).and_then(|t| frontmatter_of(&t));
        let role = m.and_then(|m| m.agents.get(&stem)).filter(|r| !r.is_empty()).cloned().or_else(|| fm.as_ref().and_then(|f| get_str(f, "delphi-role").map(String::from)));
        let wanted: Vec<String> = fm
            .as_ref()
            .and_then(|f| f.get(serde_yaml::Value::String("tools".into())))
            .map(|v| match v {
                serde_yaml::Value::String(t) => t.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                serde_yaml::Value::Sequence(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default();
        let dropped = match role.as_deref().and_then(|r| host.role_scope(r)) {
            Some(scope) => wanted.into_iter().filter(|t| !scope.tools.iter().any(|x| x.eq_ignore_ascii_case(t))).collect(),
            None => Vec::new(),
        };
        s.agents.push((stem, role, dropped));
    }
    s.guidance_roles = tree.under("behavior/roles/").filter_map(|e| e.path.strip_prefix("behavior/roles/")?.strip_suffix(".md").map(String::from)).collect();
    let count = |path: &str, prefix: &str| -> usize { tree.text(path).map(|t| Regex::new(&format!(r"(?m)^## \[{}-", prefix)).unwrap().find_iter(body_of(&t)).count()).unwrap_or(0) };
    s.goals = count("behavior/goals.md", "G");
    s.perspectives = count("behavior/perspectives.md", "P");
    s.rules = count("behavior/heuristics.md", "T");
    s.policy_entries = tree.text("behavior/policies.toml").map(|t| t.lines().filter(|l| l.starts_with("[[policy.")).count()).unwrap_or(0);
    if let Some(idx) = tree.text("vectors/index.json").and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        s.vector_caches = idx["caches"].as_array().map(|a| a.iter().filter_map(|c| c["model_id"].as_str().map(String::from)).collect()).unwrap_or_default();
    }
    s.eval_record = tree.text(".delphi-bundle/eval.json").filter(|t| t.len() <= 4096);
    s
}
