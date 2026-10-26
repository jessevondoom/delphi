//! Reading an added bundle back from the install's cache for use in a prompt (`specs/024` §3.4, §4.2).
//!
//! Nothing here trusts the cache directory: every file is checked against the bundle's `files.json`, and that against the
//! bundle hash the lock recorded at add. A cache entry that was edited after the operator added it loads nothing.

use crate::ids::{body_of, frontmatter_of, get_str};
use std::path::Path;

/// The text of `rel` in the cache entry `cache`, after proving that it is the file the operator added: `files.json` must
/// hash to `bundle_hash` and list `rel` with its own hash.
pub fn read_verified(cache: &Path, bundle_hash: &str, rel: &str) -> Result<String, String> {
    let manifest = std::fs::read(cache.join(".delphi-bundle/files.json")).map_err(|_| "its cache entry is missing".to_string())?;
    if crate::bundle_hash(&manifest) != bundle_hash {
        return Err("its cache entry no longer matches the hash recorded when it was added".into());
    }
    let json: serde_json::Value = serde_json::from_slice(&manifest).map_err(|e| format!("files.json: {}", e))?;
    let want = json["files"][rel]["blake3"].as_str().ok_or_else(|| format!("{} is not in files.json", rel))?;
    let bytes = std::fs::read(cache.join(rel)).map_err(|e| format!("{}: {}", rel, e))?;
    if format!("{}{}", crate::BUNDLE_HASH_PREFIX, blake3::hash(&bytes).to_hex()) != want {
        return Err(format!("{} was changed after the bundle was added", rel));
    }
    String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", rel))
}

/// A document's body: everything after its frontmatter.
pub fn body(text: &str) -> &str {
    body_of(text)
}

/// One `## [T-<org>-NN] title` rule of `behavior/heuristics.md`, with its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDoc {
    pub id: String,
    /// The section as written, heading first.
    pub text: String,
}

pub fn rules(text: &str) -> Vec<RuleDoc> {
    crate::behavior::sections(body_of(text), "T").into_iter().map(|(id, text)| RuleDoc { id, text: text.trim_end().to_string() }).collect()
}

fn field_of(section: &str, name: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r"(?mi)^\s*[-*]\s+\*\*{}:\*\*\s*(.*)$", regex::escape(name))).ok()?;
    re.captures(section).map(|c| c[1].trim().to_string())
}

/// One `## [G-<org>-NN] title` goal of `behavior/goals.md` (`specs/023` §6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalDoc {
    pub id: String,
    pub title: String,
    pub horizon: String,
    pub measure: String,
    /// `active` | `met` | `dropped` | `superseded` (lowercased, first word).
    pub status: String,
}

pub fn goals(text: &str) -> Vec<GoalDoc> {
    crate::behavior::sections(body_of(text), "G")
        .into_iter()
        .map(|(id, sec)| {
            let heading = sec.lines().next().unwrap_or("");
            GoalDoc {
                title: heading.split_once(']').map(|x| x.1).unwrap_or("").trim().to_string(),
                horizon: field_of(&sec, "Horizon").unwrap_or_default(),
                measure: field_of(&sec, "Measure").unwrap_or_default(),
                status: field_of(&sec, "Status").unwrap_or_default().to_ascii_lowercase().split_whitespace().next().unwrap_or("").to_string(),
                id,
            }
        })
        .collect()
}

/// One `## [P-<org>-NN] title` perspective of `behavior/perspectives.md` (`specs/023` §6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerspectiveDoc {
    pub id: String,
    pub title: String,
    pub statement: String,
    pub apply_when: String,
    pub counter_case: String,
}

pub fn perspectives(text: &str) -> Vec<PerspectiveDoc> {
    crate::behavior::sections(body_of(text), "P")
        .into_iter()
        .map(|(id, sec)| PerspectiveDoc {
            title: sec.lines().next().unwrap_or("").split_once(']').map(|x| x.1).unwrap_or("").trim().to_string(),
            statement: field_of(&sec, "Statement").unwrap_or_default(),
            apply_when: field_of(&sec, "Apply when").unwrap_or_default(),
            counter_case: field_of(&sec, "Counter-case").unwrap_or_default(),
            id,
        })
        .collect()
}

/// The list items of `knowledge/index.md`: (title, path relative to `knowledge/`, summary), in the author's order.
pub fn index_entries(text: &str) -> Vec<(String, String, String)> {
    let item = regex::Regex::new(r"(?m)^\s*[-*]\s+\[([^\]]*)\]\(([^)]*)\)\s*:\s*(.*)$").unwrap();
    item.captures_iter(body_of(text)).map(|c| (c[1].trim().to_string(), c[2].trim().trim_start_matches("./").to_string(), c[3].trim().to_string())).collect()
}

/// `[applies_to]` of a `bundle.toml`: (repos, paths) globs.
pub fn applies_to(bundle_toml: &str) -> (Vec<String>, Vec<String>) {
    let Ok(t) = bundle_toml.parse::<toml::Table>() else { return (vec![], vec![]) };
    let list = |k: &str| t.get("applies_to").and_then(|a| a.as_table()).and_then(|a| a.get(k)).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    (list("repos"), list("paths"))
}

/// What a prompt needs of a bundle skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDoc {
    pub name: String,
    pub description: String,
    /// `authorized_agents`: the roles it loads for (the operator may have narrowed them at add).
    pub roles: Vec<String>,
    pub body: String,
}

pub fn skill(text: &str) -> Option<SkillDoc> {
    let fm = frontmatter_of(text)?;
    let roles = crate::skills::delphi_field(&fm, "authorized_agents")?.as_sequence()?.iter().filter_map(|x| x.as_str().map(String::from)).collect();
    Some(SkillDoc { name: get_str(&fm, "name")?.to_string(), description: get_str(&fm, "description")?.to_string(), roles, body: body_of(text).trim().to_string() })
}

/// What a dispatch needs of a bundle agent (`specs/023` §5B).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDoc {
    pub name: String,
    pub description: String,
    /// The host role: `bundle.toml`'s `[agents.<name>] role`, else the file's `delphi-role`. `None` is "Claude Code only".
    pub host_role: Option<String>,
    /// The `tools` list as written (Claude Code names); `None` when the file lists none, which means the host's own.
    pub tools: Option<Vec<String>>,
    pub body: String,
}

/// `[agents.<name>] role` of a `bundle.toml`.
pub fn manifest_agent_role(bundle_toml: &str, agent: &str) -> Option<String> {
    let t: toml::Table = bundle_toml.parse().ok()?;
    t.get("agents")?.as_table()?.get(agent)?.as_table()?.get("role")?.as_str().map(String::from)
}

pub fn agent(text: &str, agent: &str, manifest_role: Option<String>) -> Option<AgentDoc> {
    let fm = frontmatter_of(text)?;
    let tools = fm.get(serde_yaml::Value::String("tools".into())).map(|v| match v {
        serde_yaml::Value::String(s) => s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
        serde_yaml::Value::Sequence(a) => a.iter().filter_map(|x| x.as_str().map(|t| t.trim().to_string())).collect(),
        _ => Vec::new(),
    });
    Some(AgentDoc {
        name: agent.to_string(),
        description: get_str(&fm, "description")?.to_string(),
        host_role: manifest_role.filter(|r| !r.is_empty()).or_else(|| get_str(&fm, "delphi-role").map(String::from)),
        tools,
        body: body_of(text).trim().to_string(),
    })
}

/// A `red_paths` glob in the forms the merge gate can enforce (`worktree::is_host_exec`): a directory as `dir/**` or
/// `dir/`, a whole extension as `*.ext`, or one path. Returns the form the gate takes, or `None` for any other glob.
pub fn red_path_pattern(glob: &str) -> Option<String> {
    let g = glob.trim().trim_start_matches("./");
    if g.is_empty() || g.starts_with('/') || g.split('/').any(|s| s == "..") {
        return None;
    }
    if let Some(dir) = g.strip_suffix("/**") {
        return (!dir.is_empty() && !dir.contains('*')).then(|| dir.to_string());
    }
    if let Some(ext) = g.strip_prefix("*.") {
        return (!ext.is_empty() && !ext.contains(['*', '/'])).then(|| g.to_string());
    }
    (!g.contains('*')).then(|| g.trim_end_matches('/').to_string())
}

/// `behavior/policies.toml` once checked: every rule can only make Delphi refuse or ask more (`specs/023` §6.5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policies {
    /// (regular expression over the command line, reason)
    pub deny_commands: Vec<(String, String)>,
    /// (pattern in the form the merge gate takes, reason)
    pub red_paths: Vec<(String, String)>,
    /// (`connector.tool`, roles or `*`)
    pub connector_denied: Vec<(String, Vec<String>)>,
}

pub fn policies(text: &str) -> Result<Policies, String> {
    let t: toml::Table = text.parse().map_err(|e: toml::de::Error| e.message().to_string())?;
    let p = t.get("policy").and_then(|v| v.as_table()).ok_or("no [policy] table")?;
    let mut out = Policies::default();
    for (k, v) in p {
        if k.to_ascii_lowercase().contains("allow") || ["green", "auto_approve", "permission_mode", "env", "approve"].contains(&k.as_str()) {
            return Err(format!("`{}` is refused: a policy can only refuse or ask more", k));
        }
        let items = || v.as_array().map(|a| a.iter().filter_map(|x| x.as_table()).collect::<Vec<_>>()).unwrap_or_default();
        let s = |t: &toml::Table, k: &str| t.get(k).and_then(|x| x.as_str()).map(String::from);
        match k.as_str() {
            "schema" | "require_card" => {}
            "deny_commands" => {
                for it in items() {
                    let (pat, why) = (s(it, "pattern").ok_or("deny_commands needs `pattern`")?, s(it, "reason").unwrap_or_default());
                    regex::Regex::new(&pat).map_err(|e| format!("deny_commands pattern `{}`: {}", pat, e))?;
                    out.deny_commands.push((pat, why));
                }
            }
            "red_paths" => {
                for it in items() {
                    let g = s(it, "glob").ok_or("red_paths needs `glob`")?;
                    let pat = red_path_pattern(&g).ok_or_else(|| format!("red_paths glob `{}` is not `dir/**`, `dir/`, `*.ext` or a path", g))?;
                    out.red_paths.push((pat, s(it, "reason").unwrap_or_default()));
                }
            }
            "connector_denied" => {
                for it in items() {
                    let tool = s(it, "tool").ok_or("connector_denied needs `tool`")?;
                    let roles = it.get("roles").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_else(|| vec!["*".to_string()]);
                    out.connector_denied.push((tool, roles));
                }
            }
            other => return Err(format!("unknown policy key `{}`", other)),
        }
    }
    Ok(out)
}

/// The skill directories of a cache entry, sorted.
pub fn skill_dirs(cache: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(cache.join("skills")).into_iter().flatten().flatten().filter(|e| e.path().join("SKILL.md").is_file()).filter_map(|e| e.file_name().to_str().map(String::from)).collect();
    out.sort();
    out
}
