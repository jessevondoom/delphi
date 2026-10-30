//! The Delphi home, `delphi init`, ratification bootstrap, and workspace resolution
//! (`specs/006` §2.2 item 3, §3.1, amendment 006-A1).

use crate::frontmatter::ROLES;
use crate::seed::{self, Zone};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectRecord {
    pub project_id: String,
    pub root: PathBuf,
    pub display_name: String,
    pub created_unix: u64,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct ProjectRegistry {
    schema: u32,
    projects: BTreeMap<String, ProjectRecord>,
}

fn registry_path(ws_dir: &Path) -> PathBuf {
    ws_dir.join("local/projects.json")
}

fn read_registry(ws_dir: &Path) -> Result<ProjectRegistry> {
    let path = registry_path(ws_dir);
    match fs::read(&path) {
        Ok(bytes) => {
            let registry: ProjectRegistry = serde_json::from_slice(&bytes).with_context(|| format!("reading project registry {}", path.display()))?;
            if registry.schema != 1 {
                bail!("unsupported project registry schema {} in {}", registry.schema, path.display());
            }
            Ok(registry)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectRegistry { schema: 1, projects: BTreeMap::new() }),
        Err(e) => Err(e).with_context(|| format!("reading project registry {}", path.display())),
    }
}

fn write_registry(ws_dir: &Path, registry: &ProjectRegistry) -> Result<()> {
    let path = registry_path(ws_dir);
    let parent = path.parent().expect("registry has parent");
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".projects-{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(registry)?;
    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("installing {}", path.display()))
}

fn new_project_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Register a directory identity in a workspace and write its non-secret marker.
pub fn register_project(ws_dir: &Path, root: &Path) -> Result<ProjectRecord> {
    let root = fs::canonicalize(root).with_context(|| format!("resolving project directory {}", root.display()))?;
    if !root.is_dir() {
        bail!("project root is not a directory: {}", root.display());
    }
    let marker = root.join(".delphi-project");
    let mut registry = read_registry(ws_dir)?;
    if marker.exists() {
        let bytes = fs::read(&marker).with_context(|| format!("reading {}", marker.display()))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).with_context(|| format!("invalid project marker {}", marker.display()))?;
        let id = value.get("project_id").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("project marker has no project_id: {}", marker.display()))?;
        if let Some(record) = registry.projects.get(id) {
            if record.root != root {
                bail!("project id {} is already registered at {}; use `delphi project clone` for a copy or `project relink` after a move", id, record.root.display());
            }
            return Ok(record.clone());
        }
        bail!("project marker {} is not present in this workspace registry; use `delphi project relink` or `project clone`", marker.display());
    }
    if let Some(record) = registry.projects.values().find(|record| record.root == root) {
        return Ok(record.clone());
    }

    let project_id = new_project_id()?;
    let display_name = root.file_name().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "project".into());
    let record = ProjectRecord { project_id: project_id.clone(), root: root.clone(), display_name, created_unix: unix_now() };
    let marker_bytes = serde_json::to_vec_pretty(&serde_json::json!({"schema": 1, "project_id": project_id}))?;
    fs::write(&marker, marker_bytes).with_context(|| format!("writing project marker {}", marker.display()))?;
    registry.projects.insert(record.project_id.clone(), record.clone());
    if let Err(e) = write_registry(ws_dir, &registry) {
        let _ = fs::remove_file(&marker);
        return Err(e);
    }
    Ok(record)
}

pub fn project_records(ws_dir: &Path) -> Result<Vec<ProjectRecord>> {
    Ok(read_registry(ws_dir)?.projects.into_values().collect())
}

pub fn relink_project(ws_dir: &Path, project_id: &str, new_root: &Path) -> Result<ProjectRecord> {
    let new_root = fs::canonicalize(new_root).with_context(|| format!("resolving project directory {}", new_root.display()))?;
    if !new_root.is_dir() {
        bail!("project root is not a directory: {}", new_root.display());
    }
    let marker = new_root.join(".delphi-project");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&marker).with_context(|| format!("reading {}", marker.display()))?)?;
    if value.get("project_id").and_then(|v| v.as_str()) != Some(project_id) {
        bail!("project marker at {} does not match id {project_id}", new_root.display());
    }
    let mut registry = read_registry(ws_dir)?;
    if registry.projects.values().any(|r| r.project_id != project_id && r.root == new_root) {
        bail!("another project is already registered at {}", new_root.display());
    }
    let record = registry.projects.get_mut(project_id).ok_or_else(|| anyhow::anyhow!("project id {project_id} is not registered"))?;
    let old_root = record.root.clone();
    record.root = new_root.clone();
    let updated = record.clone();
    update_project_path(ws_dir, &old_root, &new_root)?;
    write_registry(ws_dir, &registry)?;
    Ok(updated)
}

pub fn clone_project(ws_dir: &Path, root: &Path) -> Result<ProjectRecord> {
    let root = fs::canonicalize(root).with_context(|| format!("resolving project directory {}", root.display()))?;
    let marker = root.join(".delphi-project");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&marker).with_context(|| format!("reading {}", marker.display()))?)?;
    let old_id = value.get("project_id").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("project marker has no project_id"))?;
    let mut registry = read_registry(ws_dir)?;
    let source = registry.projects.get(old_id).ok_or_else(|| anyhow::anyhow!("project id {old_id} is not registered in this workspace"))?;
    if source.root == root {
        bail!("{} is already the registered project; clone requires a copied directory", root.display());
    }
    if registry.projects.values().any(|r| r.root == root) {
        bail!("a project is already registered at {}", root.display());
    }
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let new_id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let new_name = root.file_name().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "project".into());
    let new_record = ProjectRecord { project_id: new_id.clone(), root: root.clone(), display_name: new_name, created_unix: unix_now() };
    let marker_bytes = serde_json::to_vec_pretty(&serde_json::json!({"schema": 1, "project_id": new_id}))?;
    fs::write(&marker, marker_bytes).with_context(|| format!("writing project marker {}", marker.display()))?;
    registry.projects.insert(new_record.project_id.clone(), new_record.clone());
    if let Err(e) = add_project_path(ws_dir, &root).and_then(|()| write_registry(ws_dir, &registry)) {
        let _ = fs::write(&marker, serde_json::to_vec_pretty(&value)?);
        return Err(e);
    }
    Ok(new_record)
}

/// Add a project root to the workspace's existing path-based project list.
pub fn add_project_path(ws_dir: &Path, root: &Path) -> Result<()> {
    let path = root.to_string_lossy().into_owned();
    let config = ws_dir.join("config.toml");
    let text = fs::read_to_string(&config).with_context(|| format!("reading {}", config.display()))?;
    let mut doc: toml_edit::DocumentMut = text.parse().with_context(|| format!("parsing {}", config.display()))?;
    if doc["workspace"].is_none() {
        doc["workspace"] = toml_edit::table();
    }
    if doc["workspace"]["projects"].is_none() {
        doc["workspace"]["projects"] = toml_edit::value(toml_edit::Array::new());
    }
    let array = doc["workspace"]["projects"].as_array_mut().ok_or_else(|| anyhow::anyhow!("workspace.projects in {} is not an array", config.display()))?;
    let already_present = array.iter().filter_map(|v| v.as_str()).any(|v| fs::canonicalize(v).unwrap_or_else(|_| PathBuf::from(v)) == root);
    if !already_present {
        array.push(path);
        fs::write(&config, doc.to_string()).with_context(|| format!("updating {}", config.display()))?;
    }
    Ok(())
}

fn update_project_path(ws_dir: &Path, old_root: &Path, new_root: &Path) -> Result<()> {
    let config = ws_dir.join("config.toml");
    let text = fs::read_to_string(&config).with_context(|| format!("reading {}", config.display()))?;
    let mut doc: toml_edit::DocumentMut = text.parse().with_context(|| format!("parsing {}", config.display()))?;
    let array = doc["workspace"]["projects"].as_array_mut().ok_or_else(|| anyhow::anyhow!("workspace.projects in {} is not an array", config.display()))?;
    for value in array.iter_mut() {
        if let Some(value_path) = value.as_str().map(PathBuf::from) {
            let canonical = fs::canonicalize(&value_path).unwrap_or_else(|_| {
                match (value_path.parent(), value_path.file_name()) {
                    (Some(parent), Some(name)) => fs::canonicalize(parent).map(|p| p.join(name)).unwrap_or(value_path.clone()),
                    _ => value_path.clone(),
                }
            });
            if canonical == old_root {
                *value = toml_edit::Value::from(new_root.to_string_lossy().into_owned());
            }
        }
    }
    fs::write(&config, doc.to_string()).with_context(|| format!("updating {}", config.display()))
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
    let cwd = match fs::canonicalize(cwd) {
        Ok(p) => p,
        Err(_) => return git_root(cwd).map(Resolve::NoneRegistered).unwrap_or(Resolve::NotInGitRepo),
    };
    let mut matches = Vec::<(String, usize)>::new();
    if let Ok(entries) = fs::read_dir(home.join("workspaces")) {
        for e in entries.flatten() {
            let roots = projects(&e.path());
            if let Some(specificity) = roots.iter().filter(|root| cwd.starts_with(root)).map(|root| root.components().count()).max() {
                matches.push((e.file_name().to_string_lossy().to_string(), specificity));
            }
        }
    }
    if let Some(max_specificity) = matches.iter().map(|(_, n)| *n).max() {
        let mut best: Vec<String> = matches.into_iter().filter(|(_, n)| *n == max_specificity).map(|(name, _)| name).collect();
        best.sort();
        return match best.len() {
            1 => Resolve::Found(best.remove(0)),
            _ => Resolve::Ambiguous(best),
        };
    }
    git_root(&cwd).map(Resolve::NoneRegistered).unwrap_or(Resolve::NotInGitRepo)
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
    write_lock(&ws.dir.join("base/manifest.lock"), &manifest, &[("release", version.clone()), ("signature", crate::release::lock_signature())])?;

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
    write_lock(&op.join("base/manifest.lock"), &manifest, &[("release", env!("CARGO_PKG_VERSION").into()), ("signature", crate::release::lock_signature())])?;
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
    fn directory_projects_resolve_without_git_and_nested_projects_take_precedence() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let parent = t.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(child.join("src")).unwrap();
        fs::create_dir_all(parent.join("docs")).unwrap();
        let parent_ws = init(&InitOptions { home: home.clone(), workspace: "parent-ws".into(), projects: vec![parent.clone()], theme: "greek".into() }).unwrap();
        let child_ws = init(&InitOptions { home: home.clone(), workspace: "child-ws".into(), projects: vec![child.clone()], theme: "greek".into() }).unwrap();
        register_project(&parent_ws.dir, &parent).unwrap();
        register_project(&child_ws.dir, &child).unwrap();
        assert_eq!(resolve_workspace(&home, &child.join("src"), None), Resolve::Found("child-ws".into()));
        assert_eq!(resolve_workspace(&home, &parent.join("docs"), None), Resolve::Found("parent-ws".into()));
        assert_eq!(resolve_workspace(&home, t.path(), None), Resolve::NotInGitRepo);
    }

    #[test]
    fn project_registration_marker_and_registry_agree() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let root = t.path().join("plain-folder");
        fs::create_dir_all(root.join("nested")).unwrap();
        let ws = init(&InitOptions { home: home.clone(), workspace: "default".into(), projects: vec![], theme: "greek".into() }).unwrap();
        add_project_path(&ws.dir, &root).unwrap();
        let record = register_project(&ws.dir, &root).unwrap();
        let marker: serde_json::Value = serde_json::from_slice(&fs::read(root.join(".delphi-project")).unwrap()).unwrap();
        assert_eq!(marker["project_id"].as_str(), Some(record.project_id.as_str()));
        assert_eq!(project_records(&ws.dir).unwrap(), vec![record.clone()]);
        assert_eq!(resolve_workspace(&home, &root.join("nested"), None), Resolve::Found("default".into()));
    }

    #[test]
    fn relink_updates_registry_and_path_resolution_after_move() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let root = t.path().join("old-root");
        fs::create_dir_all(&root).unwrap();
        let ws = init(&InitOptions { home: home.clone(), workspace: "default".into(), projects: vec![root.clone()], theme: "greek".into() }).unwrap();
        let record = register_project(&ws.dir, &root).unwrap();
        let moved = t.path().join("new-root");
        fs::rename(&root, &moved).unwrap();
        fs::create_dir_all(moved.join("src")).unwrap();
        relink_project(&ws.dir, &record.project_id, &moved).unwrap();
        assert_eq!(project_records(&ws.dir).unwrap()[0].root, fs::canonicalize(&moved).unwrap());
        assert_eq!(resolve_workspace(&home, &moved.join("src"), None), Resolve::Found("default".into()));
    }

    #[test]
    fn clone_project_gets_a_distinct_identity() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let root = t.path().join("original");
        fs::create_dir_all(&root).unwrap();
        let ws = init(&InitOptions { home, workspace: "default".into(), projects: vec![root.clone()], theme: "greek".into() }).unwrap();
        let original = register_project(&ws.dir, &root).unwrap();
        let clone = t.path().join("copy");
        fs::create_dir_all(&clone).unwrap();
        fs::copy(root.join(".delphi-project"), clone.join(".delphi-project")).unwrap();
        let independent = clone_project(&ws.dir, &clone).unwrap();
        assert_ne!(original.project_id, independent.project_id);
        assert_eq!(project_records(&ws.dir).unwrap().len(), 2);
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
