//! `.claude-plugin/marketplace.json` (`specs/025` §7.3): written for a set of bundles, and read for one entry.
//!
//! A marketplace is a locator, never a trust source. The reader keeps only what a fetch needs and refuses what Delphi
//! will not fetch: a source without a full commit `sha`, a branch or tag standing where the `sha` should be, and any
//! source type that runs something (`npm`, `archive`, `command`). Everything in the file is hostile.

use regex::Regex;
use serde_json::{json, Value};

/// Where an entry's bytes come from. `sha` is a full commit id in lowercase hex; nothing here is a branch or tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locator {
    /// `https://…` repository URL, resolved from `github` (`owner/repo`) or given by `url` / `git-subdir`.
    pub url: String,
    /// The directory of the repository that holds the bundle (`git-subdir`), without leading or trailing slash.
    pub path: Option<String>,
    pub git_ref: Option<String>,
    pub sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketEntry {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    /// `metadata.delphi.bundle_hash`: a claim the fetch is checked against, never a way to skip a check.
    pub bundle_hash: Option<String>,
    pub source: Result<Locator, String>,
}

/// A full commit id: 40 hex digits (SHA-1) or 64 (SHA-256 repositories), lowercase.
pub fn is_sha(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn github_url(repo: &str) -> Result<String, String> {
    if Regex::new(r"^[A-Za-z0-9_.-]{1,100}/[A-Za-z0-9_.-]{1,100}$").unwrap().is_match(repo) {
        Ok(format!("https://github.com/{}.git", repo))
    } else {
        Err(format!("github repo `{}` is not owner/repo", repo.chars().take(60).collect::<String>()))
    }
}

fn safe_subdir(p: &str) -> Result<String, String> {
    let p = p.trim_start_matches("./").trim_end_matches('/');
    if p.is_empty() || p.starts_with('/') || p.split('/').any(|c| c.is_empty() || c == "." || c == ".." || c.contains('\\')) {
        return Err(format!("path `{}` is not a plain relative directory", p.chars().take(60).collect::<String>()));
    }
    Ok(p.to_string())
}

fn locator(src: &Value) -> Result<Locator, String> {
    let Some(o) = src.as_object() else {
        return Err("the source is a string path or not an object; only github, url and git-subdir sources are fetched".into());
    };
    let kind = o.get("source").and_then(|k| k.as_str()).unwrap_or("");
    let get = |k: &str| o.get(k).and_then(|v| v.as_str());
    let (url, path) = match kind {
        "github" => (github_url(get("repo").ok_or("a github source needs `repo`")?)?, None),
        "url" => (get("url").ok_or("a url source needs `url`")?.to_string(), None),
        "git-subdir" => (get("url").ok_or("a git-subdir source needs `url`")?.to_string(), Some(safe_subdir(get("path").ok_or("a git-subdir source needs `path`")?)?)),
        other => return Err(format!("source type `{}` is not fetched (Delphi fetches github, url and git-subdir by commit only)", other.chars().take(30).collect::<String>())),
    };
    if !url.starts_with("https://") || url.contains(char::is_whitespace) || url.chars().any(|c| c.is_control()) {
        return Err("the repository URL must be https".into());
    }
    let git_ref = get("ref").map(String::from);
    // The pin. A missing `sha` is refused even when a `ref` is present: a ref moves, and the point is bytes that do not.
    let sha = match get("sha") {
        Some(s) if is_sha(s) => s.to_string(),
        Some(_) => return Err("`sha` is not a full commit id (40 or 64 lowercase hex digits); a branch or tag name is refused".into()),
        None if git_ref.is_some() => return Err("the source has a `ref` but no `sha`; a ref moves, so it is refused (the entry must pin a commit sha)".into()),
        None => return Err("the source has no `sha`; the entry must pin a commit".into()),
    };
    Ok(Locator { url, path, git_ref, sha })
}

/// The entries of a marketplace file. A file that is not a marketplace is an error; one bad entry is not (it carries its
/// reason, so that `add` can say why *that* entry was refused).
pub fn parse(text: &str) -> Result<Vec<MarketEntry>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("marketplace.json is not valid JSON: {}", e))?;
    let plugins = v.get("plugins").and_then(|p| p.as_array()).ok_or("marketplace.json has no `plugins` list")?;
    let mut out = Vec::new();
    for p in plugins {
        let Some(name) = p.get("name").and_then(|n| n.as_str()) else { continue };
        let s = |k: &str| p.get(k).and_then(|x| x.as_str()).map(String::from);
        out.push(MarketEntry {
            name: name.to_string(),
            version: s("version"),
            description: s("description"),
            bundle_hash: p.get("metadata").and_then(|m| m.get("delphi.bundle_hash").or_else(|| m.get("delphi").and_then(|d| d.get("bundle_hash")))).and_then(|h| h.as_str()).map(String::from),
            source: p.get("source").ok_or_else(|| "the entry has no `source`".to_string()).and_then(locator),
        });
    }
    Ok(out)
}

/// What the writer needs to know about one bundle.
pub struct Listing<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub description: &'a str,
    pub tags: Vec<String>,
    pub bundle_hash: &'a str,
}

/// Where the listed bundles are published: one repository and one commit. `subdir` is the directory that holds each
/// bundle's folder (`<subdir>/<name>`); without it every bundle is a repository root.
pub struct Published<'a> {
    pub marketplace_name: &'a str,
    pub owner: &'a str,
    pub repo: &'a str,
    pub git_ref: Option<&'a str>,
    pub sha: &'a str,
    pub subdir: Option<&'a str>,
}

pub fn write(p: &Published, bundles: &[Listing]) -> Result<String, String> {
    if !is_sha(p.sha) {
        return Err("--sha must be a full commit id (40 or 64 lowercase hex digits): an entry without a pinned commit would be refused by every reader".into());
    }
    let (kind, key) = if p.repo.starts_with("https://") { ("url", "url") } else { ("github", "repo") };
    if kind == "github" {
        github_url(p.repo)?;
    }
    if let Some(d) = p.subdir {
        safe_subdir(d)?;
    }
    let mut plugins = Vec::new();
    for b in bundles {
        let mut src = serde_json::Map::new();
        src.insert("source".into(), if p.subdir.is_some() { "git-subdir" } else { kind }.into());
        if let Some(d) = p.subdir {
            // `git-subdir` takes a URL, so a `owner/repo` is written out.
            src.insert("url".into(), if kind == "github" { github_url(p.repo)?.into() } else { p.repo.into() });
            src.insert("path".into(), format!("{}/{}", d.trim_matches('/'), b.name).into());
        } else {
            src.insert(key.into(), p.repo.into());
        }
        if let Some(r) = p.git_ref {
            src.insert("ref".into(), r.into());
        }
        src.insert("sha".into(), p.sha.into());
        plugins.push(json!({
            "name": b.name,
            "source": src,
            "version": b.version,
            "description": b.description,
            "category": "org-bundle",
            "tags": b.tags,
            "metadata": {"delphi.bundle_hash": b.bundle_hash},
        }));
    }
    let v = json!({"name": p.marketplace_name, "owner": {"name": p.owner}, "plugins": plugins});
    Ok(serde_json::to_string_pretty(&v).unwrap() + "\n")
}
