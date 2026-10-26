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

/// The skill directories of a cache entry, sorted.
pub fn skill_dirs(cache: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(cache.join("skills")).into_iter().flatten().flatten().filter(|e| e.path().join("SKILL.md").is_file()).filter_map(|e| e.file_name().to_str().map(String::from)).collect();
    out.sort();
    out
}
