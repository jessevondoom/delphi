//! The Delphi home, `delphi init`, ratification bootstrap, and workspace resolution
//! (`specs/006` §2.2 item 3, §3.1, amendment 006-A1).

use crate::frontmatter::ROLES;
use crate::seed::{self, Zone};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub use delphi_core::lock::{access_blocks, is_protected};

pub fn delphi_home(explicit: Option<&str>) -> PathBuf {
    if let Some(h) = explicit {
        return PathBuf::from(h);
    }
    if let Ok(h) = std::env::var("DELPHI_HOME") {
        return PathBuf::from(h);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".delphi")
}

pub fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Physical layout of one workspace.
pub struct Workspace {
    pub name: String,
    pub dir: PathBuf,
}

impl Workspace {
    pub fn zone_dir(&self, z: Zone) -> PathBuf {
        match z {
            Zone::Base => self.dir.join("base"),
            Zone::Team => self.dir.join("team"),
            Zone::Local => self.dir.join("local"),
        }
    }
}

/// Lock file format: logical path → blake3, plus metadata.
pub fn write_lock(path: &Path, entries: &BTreeMap<String, String>, meta: &[(&str, String)]) -> Result<()> {
    let mut root = serde_json::Map::new();
    for (k, v) in meta {
        root.insert((*k).into(), serde_json::Value::String(v.clone()));
    }
    let files: serde_json::Map<String, serde_json::Value> =
        entries.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect();
    root.insert("files".into(), serde_json::Value::Object(files));
    write_new(path, serde_json::to_string_pretty(&serde_json::Value::Object(root))?.as_bytes())
}

/// Strict: a lock without a `files` object, or with a non-string hash, is an error (fail closed).
pub fn read_lock(path: &Path) -> Result<BTreeMap<String, String>> {
    let v: serde_json::Value = serde_json::from_slice(&fs::read(path).with_context(|| format!("reading {}", path.display()))?)?;
    let files = v.get("files").and_then(|f| f.as_object()).with_context(|| format!("{}: no `files` object", path.display()))?;
    files
        .iter()
        .map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())).with_context(|| format!("{}: hash for `{}` is not a string", path.display(), k)))
        .collect()
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// Walk up from `start` to the directory containing `.git` (file or directory).
pub fn git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = fs::canonicalize(start).ok()?;
    loop {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Project paths registered in a workspace's `config.toml`.
pub fn projects(ws_dir: &Path) -> Vec<PathBuf> {
    let text = fs::read_to_string(ws_dir.join("config.toml")).unwrap_or_default();
    let v: toml::Value = match toml::from_str(&text) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    v.get("workspace")
        .and_then(|w| w.get("projects"))
        .and_then(|p| p.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).map(expand_home).map(|p| fs::canonicalize(&p).unwrap_or(p)).collect())
        .unwrap_or_default()
}

fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(p),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Resolve {
    Found(String),
    Ambiguous(Vec<String>),
    NoneRegistered(PathBuf),
    NotInGitRepo,
    NoSuchWorkspace(String),
}

/// `specs/006` §3.1 workspace resolution.
pub fn resolve_workspace(home: &Path, cwd: &Path, explicit: Option<String>) -> Resolve {
    let explicit = explicit.or_else(|| std::env::var("DELPHI_WORKSPACE").ok());
    if let Some(name) = explicit {
        return if home.join("workspaces").join(&name).join("config.toml").exists() {
            Resolve::Found(name)
        } else {
            Resolve::NoSuchWorkspace(name)
        };
    }
    let root = match git_root(cwd) {
        Some(r) => r,
        None => return Resolve::NotInGitRepo,
    };
    let mut matches = Vec::new();
    if let Ok(entries) = fs::read_dir(home.join("workspaces")) {
        for e in entries.flatten() {
            if projects(&e.path()).contains(&root) {
                matches.push(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    matches.sort();
    match matches.len() {
        0 => Resolve::NoneRegistered(root),
        1 => Resolve::Found(matches.remove(0)),
        _ => Resolve::Ambiguous(matches),
    }
}

pub struct InitOptions {
    pub home: PathBuf,
    pub workspace: String,
    pub projects: Vec<PathBuf>,
    pub theme: String,
}

/// `delphi init`: offline; writes only under the Delphi home, never into a project.
pub fn init(o: &InitOptions) -> Result<Workspace> {
    if o.workspace.is_empty() || o.workspace.contains('/') || o.workspace.starts_with('.') {
        bail!("invalid workspace name `{}`", o.workspace);
    }
    let ws = Workspace { name: o.workspace.clone(), dir: o.home.join("workspaces").join(&o.workspace) };
    if ws.dir.join("config.toml").exists() {
        bail!("workspace `{}` already exists at {}", ws.name, ws.dir.display());
    }
    let user_theme = o.home.join("themes").join(&o.theme).join("theme.md");
    if !seed::builtin_themes().contains(&o.theme) {
        let text = fs::read_to_string(&user_theme).with_context(|| format!("unknown theme `{}` (built-in: {})", o.theme, seed::builtin_themes().join(", ")))?;
        if let Err(errs) = crate::theme::parse(&text) {
            bail!("theme `{}` is invalid: {}", o.theme, errs.join("; "));
        }
    }
    for d in ["core", "base", "team", "local/missions", "local/proposals", "local/ratified", "locks", "worktrees"] {
        fs::create_dir_all(ws.dir.join(d))?;
    }
    for r in ROLES.iter() {
        fs::create_dir_all(ws.dir.join("local/memory/agents").join(r))?;
    }
    fs::create_dir_all(o.home.join("run"))?;
    set_mode(&o.home.join("run"), 0o700);

    // Seed every zone and record the release snapshot.
    let mut manifest = BTreeMap::new();
    let mut ratified = BTreeMap::new();
    for f in seed::files() {
        let rel = match f.zone {
            Zone::Local => f.logical.clone(), // memory/agents/<role>/lessons.md → local/memory/…
            _ => f.logical.clone(),
        };
        let dest = ws.zone_dir(f.zone).join(&rel);
        write_new(&dest, f.bytes)?;
        let h = hash(f.bytes);
        if f.zone == Zone::Base {
            manifest.insert(f.logical.clone(), h.clone());
        }
        if is_protected(&f.logical) {
            ratified.insert(f.logical.clone(), h);
            write_new(&ws.dir.join("local/ratified").join(&f.logical), f.bytes)?;
        }
    }
    // 006-A2: empty team and workspace heuristics, each its own ratified scope file.
    for (logical, zone_dir, id, title, prefix) in [
        ("context/team-heuristics.md", "team", "team-heuristics", "Team Heuristics", "T"),
        ("heuristics.md", "local", "workspace-heuristics", "Workspace Heuristics", "W"),
    ] {
        let body = heuristics_stub(id, title, prefix);
        write_new(&ws.dir.join(zone_dir).join(logical), body.as_bytes())?;
        write_new(&ws.dir.join("local/ratified").join(logical), body.as_bytes())?;
        ratified.insert(logical.into(), hash(body.as_bytes()));
    }
    if !seed::builtin_themes().contains(&o.theme) {
        let bytes = fs::read(&user_theme)?;
        let logical = format!("themes/{}/theme.md", o.theme);
        write_new(&ws.dir.join("local/ratified").join(&logical), &bytes)?;
        ratified.insert(logical, hash(&bytes));
    }
    let version = env!("CARGO_PKG_VERSION").to_string();
    // Release signing lands with packaging (roadmap Step 5); a dev build records that it is unsigned.
    write_lock(&ws.dir.join("base/manifest.lock"), &manifest, &[("release", version.clone()), ("signature", "unsigned-dev-build".into())])?;

    // config.toml from the template, with this workspace's name, projects and theme.
    let config = render_config(&o.workspace, &o.projects, &o.theme);
    write_new(&ws.dir.join("config.toml"), config.as_bytes())?;
    // The endpoint/access/auth blocks are pinned by hash, and their ratified copy is what the daemon
    // trusts (`specs/009` §5): a live edit has no effect until the operator ratifies it.
    let blocks = access_blocks(&config);
    ratified.insert("config.toml#access".into(), hash(blocks.as_bytes()));
    write_new(&ws.dir.join("local/ratified/config.access.toml"), blocks.as_bytes())?;
    write_lock(&ws.dir.join("local/ratified.lock"), &ratified, &[("release", version), ("ratified_by", "release".into())])?;

    init_operator_home(&o.home)?;
    Ok(ws)
}

fn heuristics_stub(id: &str, title: &str, prefix: &str) -> String {
    format!("---\nid: {id}\ntitle: \"{title}\"\nauthor: operator\ntimestamp: 1970-01-01T00:00:00Z\nsource_type: user_ratified\nconfidence: high\nstatus: active\nmax_rules: 25\nactive_rules: 0\nsource_mission: init\ntags: [heuristics]\n---\n\n# {title} (`[{prefix}-…]`)\n")
}

fn render_config(name: &str, projects: &[PathBuf], theme: &str) -> String {
    let list = projects.iter().map(|p| format!("{:?}", p.to_string_lossy())).collect::<Vec<_>>().join(", ");
    let mut out = String::new();
    for line in seed::CONFIG_TEMPLATE.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("name = ") && !out.contains("[theme]") {
            out.push_str(&format!("name = {:?}\n", name));
        } else if trimmed.starts_with("projects = ") {
            out.push_str(&format!("projects = [{}]\n", list));
        } else if trimmed.starts_with("name = ") {
            out.push_str(&format!("name = {:?}                 # display names, labels and persona voice only (specs/001 §6)\n", theme));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Amendment 006-A1: the per-operator, cross-workspace home. Created once; never overwritten.
pub fn init_operator_home(home: &Path) -> Result<()> {
    let op = home.join("operator");
    if op.join("ratified.lock").exists() {
        return Ok(());
    }
    for d in ["core/agents", "skills", "base", "ratified", "proposals", "locks"] {
        fs::create_dir_all(op.join(d))?;
    }
    for r in ROLES.iter() {
        fs::create_dir_all(op.join("memory/agents").join(r))?;
    }
    let profile = "---\nid: operator-profile\ntitle: \"Operator Profile\"\nauthor: operator\ntimestamp: 1970-01-01T00:00:00Z\nsource_type: user_ratified\nconfidence: high\nstatus: active\nsource_mission: init\ntags: [operator, profile]\n---\n\n# Operator Profile\n\n> Your preferences (languages, tools, style, review strictness). Edit with `delphi customize profile`.\n";
    let heur = "---\nid: operator-heuristics\ntitle: \"Operator Heuristics\"\nauthor: operator\ntimestamp: 1970-01-01T00:00:00Z\nsource_type: user_ratified\nconfidence: high\nstatus: active\nmax_rules: 25\nactive_rules: 0\nsource_mission: init\ntags: [operator, heuristics]\n---\n\n# Operator Heuristics (`[O-…]`)\n";
    let mut lock = BTreeMap::new();
    let mut files: Vec<(String, String)> = vec![("profile.md".into(), profile.into()), ("heuristics.md".into(), heur.into())];
    for r in ROLES.iter().filter(|r| **r != "operator") {
        let lessons = format!("---\nid: operator-lessons-{r}\ntitle: \"Operator Lessons: {r}\"\nauthor: operator\ntimestamp: 1970-01-01T00:00:00Z\nsource_type: user_ratified\nconfidence: high\nstatus: active\nmax_rules: 25\nactive_rules: 0\nsource_mission: init\ntags: [operator, lessons, {r}]\n---\n\n# Operator Lessons: `{r}`\n");
        files.push((format!("memory/agents/{}/lessons.md", r), lessons));
    }
    for (name, body) in &files {
        write_new(&op.join(name), body.as_bytes())?;
        write_new(&op.join("ratified").join(name), body.as_bytes())?;
        lock.insert(name.clone(), hash(body.as_bytes()));
    }
    let mut manifest = BTreeMap::new();
    for f in seed::files().iter().filter(|f| f.zone == Zone::Base) {
        write_new(&op.join("base").join(&f.logical), f.bytes)?;
        manifest.insert(f.logical.clone(), hash(f.bytes));
    }
    write_lock(&op.join("base/manifest.lock"), &manifest, &[("release", env!("CARGO_PKG_VERSION").into()), ("signature", "unsigned-dev-build".into())])?;
    write_lock(&op.join("ratified.lock"), &lock, &[("ratified_by", "init".into())])
}

/// Entries `init_operator_home` ratifies; `doctor` fails if any goes missing.
pub fn operator_required_entries() -> Vec<String> {
    let mut v = vec!["profile.md".to_string(), "heuristics.md".to_string()];
    v.extend(ROLES.iter().filter(|r| **r != "operator").map(|r| format!("memory/agents/{}/lessons.md", r)));
    v
}

fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(mode));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        assert!(Command::new("git").args(["init", "-q"]).current_dir(dir).status().unwrap().success());
        fs::write(dir.join("README.md"), "x").unwrap();
    }

    fn snapshot(dir: &Path) -> BTreeMap<String, String> {
        walkdir::WalkDir::new(dir)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| (e.path().strip_prefix(dir).unwrap().to_string_lossy().to_string(), hash(&fs::read(e.path()).unwrap())))
            .collect()
    }

    // specs/006 §6 test 6 (layout and clean project part; doctor part in doctor.rs)
    #[test]
    fn init_seeds_home_and_never_touches_project() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let proj = t.path().join("proj");
        git_repo(&proj);
        let before = snapshot(&proj);
        let ws = init(&InitOptions { home: home.clone(), workspace: "w".into(), projects: vec![proj.clone()], theme: "greek".into() }).unwrap();
        for p in ["base/GOVERNANCE.md", "base/manifest.lock", "team/REVIEW.md", "base/context/heuristics.md", "team/context/team-heuristics.md", "local/heuristics.md", "local/ratified/heuristics.md", "local/ratified.lock", "local/ratified/GOVERNANCE.md", "local/memory/agents/builder/lessons.md", "config.toml"] {
            assert!(ws.dir.join(p).exists(), "missing {}", p);
        }
        for p in ["operator/profile.md", "operator/heuristics.md", "operator/ratified.lock", "operator/base/agents/builder/AGENT.md", "operator/memory/agents/builder/lessons.md", "operator/ratified/memory/agents/builder/lessons.md"] {
            assert!(home.join(p).exists(), "missing {}", p);
        }
        assert_eq!(before, snapshot(&proj), "init must not modify the project repo");
        let lock = read_lock(&ws.dir.join("local/ratified.lock")).unwrap();
        assert!(lock.contains_key("agents/red-team/AGENT.md") && lock.contains_key("themes/greek/theme.md") && lock.contains_key("config.toml#access"));
        assert_eq!(hash(&fs::read(ws.dir.join("local/ratified/config.access.toml")).unwrap()), lock["config.toml#access"], "the trusted copy matches its pin");
        assert!(lock.contains_key("context/team-heuristics.md") && lock.contains_key("heuristics.md"));
        assert!(!ws.dir.join("team/context/heuristics.md").exists(), "release heuristics belong in base/ (006-A2)");
        assert!(init(&InitOptions { home: home.clone(), workspace: "w".into(), projects: vec![], theme: "greek".into() }).is_err());
        assert!(init(&InitOptions { home, workspace: "x".into(), projects: vec![], theme: "nope".into() }).is_err());
    }

    // specs/006 §6 test 9
    #[test]
    fn workspace_resolution() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let proj = t.path().join("proj");
        git_repo(&proj);
        fs::create_dir_all(proj.join("src/deep")).unwrap();
        assert_eq!(resolve_workspace(&home, &proj.join("src/deep"), None), Resolve::NoneRegistered(fs::canonicalize(&proj).unwrap()));
        init(&InitOptions { home: home.clone(), workspace: "a".into(), projects: vec![proj.clone()], theme: "greek".into() }).unwrap();
        assert_eq!(resolve_workspace(&home, &proj.join("src/deep"), None), Resolve::Found("a".into()));
        init(&InitOptions { home: home.clone(), workspace: "b".into(), projects: vec![proj.clone()], theme: "greek".into() }).unwrap();
        assert_eq!(resolve_workspace(&home, &proj, None), Resolve::Ambiguous(vec!["a".into(), "b".into()]));
        assert_eq!(resolve_workspace(&home, &proj, Some("b".into())), Resolve::Found("b".into()));
        assert_eq!(resolve_workspace(&home, t.path(), None), Resolve::NotInGitRepo);
    }

    #[test]
    fn protection_is_case_insensitive() {
        assert!(is_protected("skills/custom/s/SCRIPTS/evil.sh") && is_protected("Agents/scout/agent.MD") && is_protected("governance.md"));
        assert!(!is_protected("context/domains/a.md"));
    }

    #[test]
    fn config_render_keeps_theme_and_sets_projects() {
        let c = render_config("w", &[PathBuf::from("/tmp/p")], "greek");
        let v: toml::Value = toml::from_str(&c).unwrap();
        assert_eq!(v["workspace"]["name"].as_str(), Some("w"));
        assert_eq!(v["workspace"]["projects"][0].as_str(), Some("/tmp/p"));
        assert_eq!(v["theme"]["name"].as_str(), Some("greek"));
    }
}
