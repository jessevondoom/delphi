//! Step 9, skills half (`specs/023` §5A): each `skills/<name>/SKILL.md` is a valid Agent Skill with Delphi's fields in
//! `metadata`, never asks for more than its roles hold, and its `scripts/` are listed and never run.

use crate::ids::{body_of, frontmatter_of, get_str};
use crate::manifest::Manifest;
use crate::tree::Tree;
use crate::{Host, Report, ROLES};
use serde_yaml::{Mapping, Value};
use std::collections::BTreeSet;

pub const DESCRIPTION_CHARS: usize = 200;
const SKILL_LINES: usize = 500;
const PERMISSIONS: [&str; 5] = ["read:all", "write:worktree", "write:missions", "shell:scoped_scripts", "network:gated"];

fn key(k: &str) -> Value {
    Value::String(k.into())
}

/// A Delphi field either as a native top-level key or as a `metadata.delphi.<name>` JSON string (`specs/019` §3).
pub fn delphi_field(fm: &Mapping, name: &str) -> Option<Value> {
    if let Some(v) = fm.get(key(name)) {
        return Some(v.clone());
    }
    let s = fm.get(key("metadata"))?.as_mapping()?.get(key(&format!("delphi.{}", name)))?.as_str()?;
    serde_json::from_str::<serde_json::Value>(s).ok().and_then(|j| serde_yaml::to_value(j).ok())
}

fn string_list(v: Option<Value>) -> Option<Vec<String>> {
    v?.as_sequence()?.iter().map(|x| x.as_str().map(String::from)).collect()
}

pub fn skill_names(tree: &Tree) -> Vec<String> {
    let mut out: Vec<String> = tree.entries.iter().filter_map(|e| e.path.strip_prefix("skills/")?.strip_suffix("/SKILL.md").filter(|n| !n.contains('/')).map(String::from)).collect();
    out.sort();
    out
}

/// The roles each skill names in `authorized_agents`.
pub fn roles_of_skills(tree: &Tree) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for name in skill_names(tree) {
        if let Some(fm) = tree.text(&format!("skills/{}/SKILL.md", name)).and_then(|t| frontmatter_of(&t)) {
            out.extend(string_list(delphi_field(&fm, "authorized_agents")).unwrap_or_default());
        }
    }
    out
}

pub fn check(tree: &Tree, m: Option<&Manifest>, host: &dyn Host, r: &mut Report) {
    let _ = m;
    let mut unknown_roles: BTreeSet<String> = BTreeSet::new();
    for name in skill_names(tree) {
        let path = format!("skills/{}/SKILL.md", name);
        let Some(text) = tree.text(&path) else { continue };
        let Some(fm) = frontmatter_of(&text) else {
            r.fail(9, &path, "no frontmatter (a skill starts with `---`, then name and description)");
            continue;
        };
        for e in host.skill_errors(&fm, &name) {
            r.fail(9, &path, e);
        }
        if let Some(d) = get_str(&fm, "description") {
            if d.chars().count() > DESCRIPTION_CHARS {
                r.fail(9, &path, format!("description is {} characters; Delphi caps it at {}: shorten it (it is never cut for you)", d.chars().count(), DESCRIPTION_CHARS));
            }
        }
        let lines = body_of(&text).lines().count();
        if lines > SKILL_LINES {
            r.fail(9, &path, format!("{} body lines (cap {})", lines, SKILL_LINES));
        }
        if fm.contains_key(key("allowed-tools")) {
            r.note(9, &path, "`allowed-tools` is ignored: Delphi has no pre-approval a file can grant");
        }
        let scripts: Vec<&str> = tree.under(&format!("skills/{}/scripts/", name)).map(|e| e.path.as_str()).collect();
        if !scripts.is_empty() {
            r.note(9, &path, format!("scripts/ ({} files) is copied without the execute bit and never run by Delphi", scripts.len()));
        }
        if fm.contains_key(key("deterministic_scripts")) {
            r.fail(9, &path, "deterministic_scripts is refused in a bundle skill (scripts are never executed)");
        }
        let roles = match delphi_field(&fm, "authorized_agents") {
            None => {
                r.note(9, &path, "no authorized_agents: the operator names the roles at add (--skill-roles)");
                Vec::new()
            }
            Some(v) => match string_list(Some(v)) {
                Some(l) => l,
                None => {
                    r.fail(9, &path, "authorized_agents must be a list of role ids");
                    Vec::new()
                }
            },
        };
        for role in &roles {
            if !ROLES.contains(&role.as_str()) {
                r.fail(9, &path, format!("authorized_agents: `{}` is not a canonical role", role));
            }
        }
        let perms = match delphi_field(&fm, "required_permissions") {
            None => Vec::new(),
            Some(v) => match string_list(Some(v)) {
                Some(l) => l,
                None => {
                    r.fail(9, &path, "required_permissions must be a list of strings");
                    Vec::new()
                }
            },
        };
        for p in &perms {
            if !PERMISSIONS.contains(&p.as_str()) {
                r.fail(9, &path, format!("unknown required_permission `{}`", p));
            }
        }
        for role in roles.iter().filter(|r| ROLES.contains(&r.as_str())) {
            let Some(scope) = host.role_scope(role) else {
                unknown_roles.insert(role.clone());
                continue;
            };
            for p in perms.iter().filter(|p| PERMISSIONS.contains(&p.as_str())) {
                if !fits(&scope, p) {
                    r.fail(9, &path, format!("requires `{}` but the built-in {} role's scope does not allow it (specs/001 §2.3)", p, role));
                }
            }
        }
    }
    for role in unknown_roles {
        r.note(9, "skills/", format!("the built-in scope of `{}` is unknown to this host; required_permissions were not fitted for it", role));
    }
}

fn fits(scope: &crate::RoleScope, perm: &str) -> bool {
    let has = |list: &[String], want: &str| list.iter().any(|g| g == want);
    match perm {
        "read:all" => has(&scope.read, "*"),
        "write:worktree" => has(&scope.write, "@worktree/**"),
        // Only the orchestrator holds all of missions/**; a planning role holds a slice of it. The skill asks for the
        // area, the role's scope caps what it may actually write, so any missions write glob satisfies the request.
        "write:missions" => scope.write.iter().any(|g| g.starts_with("missions/")),
        "shell:scoped_scripts" => has(&scope.execute, "shell-worktree-scoped"),
        _ => false,
    }
}

/// `touches.roles` must cover every role a skill, agent or role-guidance file names (`specs/023` §10 step 9).
pub fn check_touches(tree: &Tree, m: Option<&Manifest>, r: &mut Report) {
    let Some(m) = m else { return };
    let mut named = roles_of_skills(tree);
    named.extend(m.agents.values().filter(|v| !v.is_empty()).cloned());
    named.extend(tree.under("behavior/roles/").filter_map(|e| e.path.strip_prefix("behavior/roles/")?.strip_suffix(".md").map(String::from)));
    for role in named.iter().filter(|x| ROLES.contains(&x.as_str())) {
        if !m.touches_roles.contains(role) {
            r.fail(9, "bundle.toml", format!("touches.roles does not list `{}`, which the bundle names", role));
        }
    }
}
