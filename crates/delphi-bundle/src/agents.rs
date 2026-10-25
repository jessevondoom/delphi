//! Step 9, agents half (`specs/023` §5B): Claude Code agent files, mapped to a Delphi host role that caps them.

use crate::ids::{frontmatter_of, get_str};
use crate::manifest::Manifest;
use crate::relax::no_relaxation_doc;
use crate::tree::Tree;
use crate::{Host, Report, ROLES};
use serde_yaml::Value;

pub const MAX_AGENT_BYTES: u64 = 8 * 1024;

/// Delphi routes by tier, so a Claude model name is advisory (`specs/002`).
pub fn tier_of(model: &str) -> Option<&'static str> {
    match model.to_ascii_lowercase().as_str() {
        "opus" => Some("frontier"),
        "sonnet" => Some("workhorse"),
        "haiku" => Some("fast"),
        _ => None,
    }
}

fn tool_list(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
        Value::Sequence(s) => s.iter().filter_map(|x| x.as_str().map(|t| t.trim().to_string())).collect(),
        _ => Vec::new(),
    }
}

pub fn check(tree: &Tree, m: Option<&Manifest>, host: &dyn Host, r: &mut Report) {
    for e in tree.under("agents/").filter(|e| e.path.ends_with(".md")) {
        let path = e.path.as_str();
        let stem = path.trim_start_matches("agents/").trim_end_matches(".md");
        if e.bytes > MAX_AGENT_BYTES {
            r.fail(9, path, format!("{} bytes (cap {} for an agent file)", e.bytes, MAX_AGENT_BYTES));
            continue;
        }
        let Some(text) = tree.text(path) else { continue };
        let Some(fm) = frontmatter_of(&text) else {
            r.fail(9, path, "no frontmatter (an agent file starts with `---`, then name and description)");
            continue;
        };
        if get_str(&fm, "name").is_none_or(|n| n.trim().is_empty()) {
            r.fail(9, path, "frontmatter needs a `name`");
        } else if get_str(&fm, "name") != Some(stem) {
            r.warn(9, path, format!("name `{}` differs from the file name `{}` (Claude Code namespaces by the name)", get_str(&fm, "name").unwrap_or(""), stem));
        }
        if get_str(&fm, "description").is_none_or(|n| n.trim().is_empty()) {
            r.fail(9, path, "frontmatter needs a `description`");
        }
        let from_manifest = m.and_then(|m| m.agents.get(stem)).filter(|s| !s.is_empty()).cloned();
        let from_file = get_str(&fm, "delphi-role").map(String::from);
        if let (Some(a), Some(b)) = (&from_manifest, &from_file) {
            if a != b {
                r.warn(9, path, format!("bundle.toml says host role `{}`, the file says `{}`; bundle.toml wins", a, b));
            }
        }
        let role = from_manifest.or(from_file);
        match &role {
            None => r.note(9, path, "no host role: marked \"Claude Code only\" and not dispatchable in Delphi (specs/022 B-17)"),
            Some(role) if !ROLES.contains(&role.as_str()) => r.fail(9, path, format!("host role `{}` is not a canonical role id", role)),
            Some(role) => {
                let wanted = fm.get(Value::String("tools".into())).map(tool_list).unwrap_or_default();
                if let Some(scope) = host.role_scope(role) {
                    let dropped: Vec<&String> = wanted.iter().filter(|t| !scope.tools.iter().any(|s| s.eq_ignore_ascii_case(t))).collect();
                    if !dropped.is_empty() {
                        r.note(9, path, format!("tools the {} role lacks are dropped at add: {}", role, dropped.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
                    }
                }
            }
        }
        if let Some(model) = get_str(&fm, "model") {
            match tier_of(model) {
                Some(t) => r.note(9, path, format!("model `{}` is advisory; routed as tier `{}`", model, t)),
                None => r.note(9, path, format!("model `{}` is ignored (Delphi routes by tier)", model)),
            }
        }
        if fm.contains_key(Value::String("hooks".into())) {
            r.warn(9, path, "`hooks` is ignored by Delphi and the agent behaves differently in Claude Code");
        }
        if fm.contains_key(Value::String("permissionMode".into())) {
            r.note(9, path, "`permissionMode` is ignored: the host role's scope and approvals apply");
        }
        for (line, quote) in no_relaxation_doc(&text) {
            r.fail(9, &format!("{}:{}", path, line), format!("tries to relax a rule (`{}`); an agent body cannot skip approvals or gates", quote));
        }
    }
}
