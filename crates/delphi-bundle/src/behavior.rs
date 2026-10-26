//! Step 10 of `specs/023` §10: the behavior caps. Goals, perspectives, role guidance, rules and tighten-only policy.

use crate::ids::{body_of, frontmatter_of};
use crate::manifest::Manifest;
use crate::relax::no_relaxation_doc;
use crate::tree::Tree;
use crate::{Host, Report, ROLES};
use regex::Regex;

pub const MAX_ACTIVE_GOALS: usize = 12;
pub const GOAL_CHARS: usize = 600;
pub const MAX_PERSPECTIVES: usize = 12;
pub const PERSPECTIVE_CHARS: usize = 350;
pub const GUIDANCE_BYTES: usize = 2048;
pub const MAX_RULES: usize = 10;
pub const RULE_CHARS: usize = 350;

/// Words a perspective may not use: it is voice and stance, not a way to reach for tools or relax a gate (`specs/023` §6.2).
const PERSPECTIVE_BANNED: [&str; 8] = ["tool", "permission", "approv", "governance", "review.md", "taint", "ignore", "override"];
/// Manifest keys role guidance may not carry: guidance is bounded text, never a manifest (`specs/023` §6.3).
const MANIFEST_KEYS: [&str; 4] = ["default_access_scope", "authorized_skills", "execute", "write"];

/// The `## [ID] title` sections of a document body, as (id, section text including the heading).
pub(crate) fn sections(body: &str, id_prefix: &str) -> Vec<(String, String)> {
    let head = Regex::new(&format!(r"^## \[({}-[A-Za-z0-9-]+)\]", id_prefix)).unwrap();
    let mut out: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        if let Some(c) = head.captures(line) {
            out.push((c[1].to_string(), format!("{}\n", line)));
        } else if line.starts_with("## ") {
            out.push((String::new(), String::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push_str(line);
            last.1.push('\n');
        }
    }
    out.retain(|(id, _)| !id.is_empty());
    out
}

fn field(section: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r"(?mi)^\s*[-*]\s+\*\*{}:\*\*\s*(.*)$", regex::escape(name))).unwrap();
    re.captures(section).map(|c| c[1].trim().to_string())
}

pub fn check(tree: &Tree, m: Option<&Manifest>, _host: &dyn Host, r: &mut Report) {
    let org = m.map(|m| m.org.as_str()).unwrap_or("");
    if let Some(text) = tree.text("behavior/goals.md") {
        goals(&text, org, r);
    }
    if let Some(text) = tree.text("behavior/perspectives.md") {
        perspectives(&text, org, r);
    }
    for e in tree.under("behavior/roles/") {
        roles(tree, &e.path, r);
    }
    if let Some(text) = tree.text("behavior/heuristics.md") {
        rules(&text, org, r);
    }
    if let Some(text) = tree.text("behavior/policies.toml") {
        policies(&text, r);
    }
    crate::skills::check_touches(tree, m, r);
}

fn id_org_ok(id: &str, org: &str) -> bool {
    org.is_empty() || id.split_once('-').map(|x| x.1).is_some_and(|rest| rest.starts_with(&format!("{}-", org)))
}

fn goals(text: &str, org: &str, r: &mut Report) {
    let path = "behavior/goals.md";
    let mut active = 0;
    for (id, sec) in sections(body_of(text), "G") {
        if !id_org_ok(&id, org) {
            r.fail(10, path, format!("goal id [{}] must be [G-{}-NN]", id, org));
        }
        let status = field(&sec, "Status").unwrap_or_default().to_ascii_lowercase();
        let status = status.split_whitespace().next().unwrap_or("").to_string();
        if !["active", "met", "dropped", "superseded"].contains(&status.as_str()) {
            r.fail(10, path, format!("[{}] Status must be active, met, dropped or superseded", id));
        }
        if status == "active" {
            active += 1;
        }
        let n = sec.trim().chars().count();
        if n > GOAL_CHARS {
            r.fail(10, path, format!("[{}] is {} characters (cap {})", id, n, GOAL_CHARS));
        }
    }
    if active > MAX_ACTIVE_GOALS {
        r.fail(10, path, format!("{} active goals (cap {})", active, MAX_ACTIVE_GOALS));
    }
    relax_errors(path, text, r);
}

fn perspectives(text: &str, org: &str, r: &mut Report) {
    let path = "behavior/perspectives.md";
    let all = sections(body_of(text), "P");
    if all.len() > MAX_PERSPECTIVES {
        r.fail(10, path, format!("{} perspectives (cap {})", all.len(), MAX_PERSPECTIVES));
    }
    for (id, sec) in &all {
        if !id_org_ok(id, org) {
            r.fail(10, path, format!("perspective id [{}] must be [P-{}-NN]", id, org));
        }
        let lines: String = sec.lines().skip(1).map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
        let n = lines.chars().count();
        if n > PERSPECTIVE_CHARS {
            r.fail(10, path, format!("[{}] is {} characters across its lines (cap {})", id, n, PERSPECTIVE_CHARS));
        }
        let lower = sec.to_ascii_lowercase();
        for w in PERSPECTIVE_BANNED {
            if lower.contains(w) {
                r.fail(10, path, format!("[{}] mentions `{}`: a perspective is a stance, not a way to reach tools or relax a gate", id, w));
            }
        }
    }
    relax_errors(path, text, r);
}

fn roles(tree: &Tree, path: &str, r: &mut Report) {
    let Some(stem) = path.strip_prefix("behavior/roles/").and_then(|p| p.strip_suffix(".md")) else {
        return r.fail(10, path, "role guidance is behavior/roles/<role>.md");
    };
    if !ROLES.contains(&stem) {
        return r.fail(10, path, format!("`{}` is not a canonical role id", stem));
    }
    let Some(text) = tree.text(path) else { return };
    let body = body_of(&text);
    if body.len() > GUIDANCE_BYTES {
        r.fail(10, path, format!("guidance is {} bytes (cap {})", body.len(), GUIDANCE_BYTES));
    }
    if let Some(fm) = frontmatter_of(&text) {
        match crate::ids::get_str(&fm, "role") {
            Some(role) if role == stem => {}
            Some(role) => r.fail(10, path, format!("frontmatter role `{}` differs from the file name `{}`", role, stem)),
            None => r.fail(10, path, "frontmatter needs `role: <canonical id>`"),
        }
        for k in MANIFEST_KEYS {
            if fm.contains_key(serde_yaml::Value::String(k.into())) {
                r.fail(10, path, format!("manifest key `{}`: guidance is bounded text and never a manifest", k));
            }
        }
    }
    if body.contains("default_access_scope") || body.contains("authorized_skills") {
        r.fail(10, path, "names a manifest field (`default_access_scope`/`authorized_skills`): guidance never merges with a manifest");
    }
    relax_errors(path, &text, r);
}

fn rules(text: &str, org: &str, r: &mut Report) {
    let path = "behavior/heuristics.md";
    let all = sections(body_of(text), "T");
    if all.len() > MAX_RULES {
        r.fail(10, path, format!("{} rules (cap {})", all.len(), MAX_RULES));
    }
    for (id, sec) in &all {
        if !id_org_ok(id, org) {
            r.fail(10, path, format!("rule id [{}] must be [T-{}-NN]", id, org));
        }
        let n = sec.lines().skip(1).collect::<Vec<_>>().join("\n").trim().chars().count();
        if n > RULE_CHARS {
            r.fail(10, path, format!("[{}] is {} characters (cap {})", id, n, RULE_CHARS));
        }
        for f in ["Trigger", "Directive", "Falsification"] {
            if field(sec, f).is_none() {
                r.fail(10, path, format!("[{}] lacks a `- **{}:**` line", id, f));
            }
        }
    }
    relax_errors(path, text, r);
}

fn relax_errors(path: &str, text: &str, r: &mut Report) {
    for (line, quote) in no_relaxation_doc(text) {
        r.fail(10, &format!("{}:{}", path, line), format!("tries to relax a rule (`{}`); behavior text cannot override governance", quote));
    }
}

const POLICY_KEYS: [&str; 5] = ["schema", "deny_commands", "red_paths", "connector_denied", "require_card"];
const POLICY_REFUSED: [&str; 7] = ["allow", "allow_commands", "green", "auto_approve", "permission_mode", "env", "approve"];

fn policies(text: &str, r: &mut Report) {
    let path = "behavior/policies.toml";
    let t: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => return r.fail(10, path, format!("parse error: {}", e.message())),
    };
    for k in t.keys() {
        if k != "policy" {
            r.fail(10, path, format!("unknown top-level key `{}` (only [policy])", k));
        }
    }
    let Some(p) = t.get("policy").and_then(|v| v.as_table()) else {
        return r.fail(10, path, "needs a [policy] table");
    };
    for (k, v) in p {
        let lower = k.to_ascii_lowercase();
        if POLICY_REFUSED.contains(&lower.as_str()) || lower.contains("allow") {
            r.fail(10, path, format!("`{}` is refused: a policy can only make Delphi ask more or refuse more (D-21)", k));
            continue;
        }
        if !POLICY_KEYS.contains(&k.as_str()) {
            r.fail(10, path, format!("unknown policy key `{}`", k));
            continue;
        }
        if k == "schema" {
            if v.as_integer() != Some(1) {
                r.fail(10, path, "policy.schema must be 1");
            }
            continue;
        }
        let Some(items) = v.as_array() else {
            r.fail(10, path, format!("policy.{} must be an array of tables", k));
            continue;
        };
        let (need, regexes): (&[&str], &[&str]) = match k.as_str() {
            "deny_commands" => (&["pattern", "reason"], &["pattern"]),
            "red_paths" => (&["glob", "reason"], &[]),
            "connector_denied" => (&["tool", "roles"], &[]),
            _ => (&["pattern"], &["pattern"]),
        };
        for (i, item) in items.iter().enumerate() {
            let Some(it) = item.as_table() else {
                r.fail(10, path, format!("policy.{}[{}] must be a table", k, i + 1));
                continue;
            };
            for key in it.keys() {
                if !need.contains(&key.as_str()) && !(k == "require_card" && key == "reason") {
                    let refused = key.to_ascii_lowercase().contains("allow");
                    r.fail(10, path, format!("policy.{}[{}]: {}key `{}`", k, i + 1, if refused { "refused " } else { "unknown " }, key));
                }
            }
            for key in need {
                if !it.contains_key(*key) {
                    r.fail(10, path, format!("policy.{}[{}] needs `{}`", k, i + 1, key));
                }
            }
            if k == "red_paths" {
                if let Some(g) = it.get("glob").and_then(|v| v.as_str()) {
                    if crate::load::red_path_pattern(g).is_none() {
                        r.fail(10, path, format!("policy.red_paths[{}]: glob `{}` must be `dir/**`, `dir/`, `*.ext` or one path (the merge gate matches nothing else)", i + 1, g));
                    }
                }
            }
            for key in regexes {
                if let Some(pat) = it.get(*key).and_then(|v| v.as_str()) {
                    if Regex::new(pat).is_err() {
                        r.fail(10, path, format!("policy.{}[{}]: `{}` is not a valid regular expression", k, i + 1, pat));
                    }
                }
            }
            if let Some(roles) = it.get("roles") {
                let ok = roles.as_array().is_some_and(|a| a.iter().all(|x| x.as_str().is_some_and(|s| s == "*" || ROLES.contains(&s))));
                if !ok {
                    r.fail(10, path, format!("policy.{}[{}]: roles must be canonical role ids or \"*\"", k, i + 1));
                }
            }
        }
    }
}
