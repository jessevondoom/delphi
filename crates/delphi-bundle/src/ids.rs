//! The structured ids a bundle promises to keep stable (`specs/023` §9): removing or renaming one is a MAJOR change.

use crate::tree::Tree;
use regex::Regex;
use std::collections::BTreeSet;

/// The frontmatter of a document as a mapping, if it has one.
pub fn frontmatter_of(text: &str) -> Option<serde_yaml::Mapping> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n").or_else(|| rest.strip_suffix("\n---").map(|s| s.len()))?;
    match serde_yaml::from_str::<serde_yaml::Value>(&rest[..end]).ok()? {
        serde_yaml::Value::Mapping(m) => Some(m),
        _ => None,
    }
}

pub fn body_of(text: &str) -> &str {
    match text.strip_prefix("---\n").and_then(|r| r.find("\n---\n").map(|i| &r[i + 5..])) {
        Some(b) => b,
        None => text,
    }
}

pub fn get_str<'a>(m: &'a serde_yaml::Mapping, k: &str) -> Option<&'a str> {
    m.get(serde_yaml::Value::String(k.into())).and_then(|v| v.as_str())
}

/// Every id the bundle names, each prefixed by its kind so a knowledge id and a skill name never collide.
pub fn collect(tree: &Tree) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (path, text) in tree.texts("knowledge/", ".md") {
        let _ = path;
        if let Some(id) = frontmatter_of(&text).and_then(|fm| get_str(&fm, "id").map(String::from)) {
            out.insert(format!("knowledge:{}", id));
        }
    }
    for e in tree.under("skills/") {
        if let Some(rest) = e.path.strip_prefix("skills/") {
            if let Some((name, "SKILL.md")) = rest.split_once('/') {
                out.insert(format!("skill:{}", name));
            }
        }
    }
    for e in tree.under("agents/") {
        if let Some(n) = e.path.strip_prefix("agents/").and_then(|p| p.strip_suffix(".md")) {
            out.insert(format!("agent:{}", n));
        }
    }
    let goal = Regex::new(r"(?m)^## \[(G-[A-Za-z0-9-]+)\]").unwrap();
    let persp = Regex::new(r"(?m)^## \[(P-[A-Za-z0-9-]+)\]").unwrap();
    if let Some(t) = tree.text("behavior/goals.md") {
        out.extend(goal.captures_iter(&t).map(|c| format!("goal:{}", &c[1])));
    }
    if let Some(t) = tree.text("behavior/perspectives.md") {
        out.extend(persp.captures_iter(&t).map(|c| format!("perspective:{}", &c[1])));
    }
    out
}
