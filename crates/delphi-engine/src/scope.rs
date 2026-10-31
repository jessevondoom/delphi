//! Home and project scope routing for memory and context (`specs/058` PS2, §3.2 and §4).
//!
//! Pure reads: nothing here writes. [`resolve`] maps a working directory to the active [`Active`] scope from the
//! workspace's project registry (most specific registered root wins; outside every root the persistent `default`
//! project applies; a corrupt or ambiguous registry fails closed to the home layer alone). [`layered_sources`] lists
//! the home layer and the active project layer side by side, every item labelled with its layer, a project item that
//! shares a logical path with a home item labelled as an override and never merged into it. [`route_staged`] decides
//! where a staged note may go: the active project by default, anywhere else only by the operator.
//!
//! A project id reaches a path only through [`ProjectId::parse`], so `..`, separators and another project's id typed
//! as a path cannot name a directory outside `local/projects/<id>/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::kb::{SourceFile, md_files};

/// The reserved id of the fallback project (`specs/058` §3.1).
pub const DEFAULT_PROJECT: &str = "default";

/// A validated project id: `default` or the 32 lowercase hex characters registration assigns.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectId(String);

impl ProjectId {
    pub fn parse(s: &str) -> Result<ProjectId, String> {
        let hex = s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if s == DEFAULT_PROJECT || hex { Ok(ProjectId(s.to_string())) } else { Err(format!("`{}` is not a project id", s.chars().take(40).collect::<String>())) }
    }

    pub fn default_project() -> ProjectId {
        ProjectId(DEFAULT_PROJECT.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The provenance label of this layer's items.
    pub fn label(&self) -> String {
        format!("project:{}", self.0)
    }
}

/// The scope a directory resolves to. `project` is `None` when project-specific access failed closed; the home layer
/// is available either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Active {
    pub project: Option<ProjectId>,
    /// Why project access failed closed, or `None`.
    pub diagnostic: Option<String>,
}

#[derive(serde::Deserialize)]
struct Registry {
    schema: u32,
    projects: BTreeMap<String, Record>,
}

#[derive(serde::Deserialize)]
struct Record {
    project_id: String,
    root: PathBuf,
    #[serde(default)]
    display_name: String,
}

fn registry(ws_dir: &Path) -> Result<Vec<(ProjectId, PathBuf)>, String> {
    let path = ws_dir.join("local/projects.json");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("the project registry cannot be read: {}", e)),
    };
    let reg: Registry = serde_json::from_slice(&bytes).map_err(|e| format!("the project registry is corrupt: {}", e))?;
    if reg.schema != 1 {
        return Err(format!("the project registry has unsupported schema {}", reg.schema));
    }
    let mut out = Vec::new();
    for (key, rec) in reg.projects {
        let id = ProjectId::parse(&key).map_err(|e| format!("the project registry is corrupt: {}", e))?;
        if rec.project_id != key || id.as_str() == DEFAULT_PROJECT {
            return Err("the project registry is corrupt: an entry's id does not match its key or is reserved".into());
        }
        out.push((id, rec.root));
    }
    Ok(out)
}

/// The registered project ids, or `Err` when the registry cannot be trusted.
pub fn registered_ids(ws_dir: &Path) -> Result<std::collections::BTreeSet<String>, String> {
    registry(ws_dir).map(|r| r.into_iter().map(|(i, _)| i.0).collect())
}

/// The root a registered project lives at, or `None` for the default project, an unknown id or an untrusted registry.
pub fn project_root(ws_dir: &Path, id: &ProjectId) -> Option<PathBuf> {
    registry(ws_dir).ok()?.into_iter().find(|(i, _)| i == id).map(|(_, root)| root)
}

/// `(name, root)` of every registered project, by name; the name is the display name, else the directory's. Empty when the registry cannot be trusted.
pub fn project_list(ws_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(text) = std::fs::read(ws_dir.join("local/projects.json")) else { return Vec::new() };
    if registry(ws_dir).is_err() {
        return Vec::new();
    }
    let Ok(reg) = serde_json::from_slice::<Registry>(&text) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> = reg
        .projects
        .into_values()
        .map(|r| {
            let name = if r.display_name.is_empty() { r.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into()) } else { r.display_name };
            (name, r.root)
        })
        .collect();
    out.sort_by_key(|(n, _)| n.to_lowercase());
    out
}

/// Is `id` the default project or registered in this workspace?
pub fn is_registered(ws_dir: &Path, id: &ProjectId) -> bool {
    id.as_str() == DEFAULT_PROJECT || registry(ws_dir).is_ok_and(|r| r.iter().any(|(i, _)| i == id))
}

/// `specs/058` §3.2: the active scope for `cwd`.
pub fn resolve(ws_dir: &Path, cwd: &Path) -> Active {
    let closed = |why: String| Active { project: None, diagnostic: Some(why) };
    let records = match registry(ws_dir) {
        Ok(r) => r,
        Err(why) => return closed(why),
    };
    if records.is_empty() {
        return Active { project: Some(ProjectId::default_project()), diagnostic: None };
    }
    let Ok(cwd) = std::fs::canonicalize(cwd) else { return closed(format!("the directory {} cannot be resolved", cwd.display())) };
    let depth = |p: &Path| p.components().count();
    let Some(best) = records.iter().filter(|(_, root)| cwd.starts_with(root)).map(|(_, root)| depth(root)).max() else {
        return Active { project: Some(ProjectId::default_project()), diagnostic: None };
    };
    let mut hits = records.into_iter().filter(|(_, root)| cwd.starts_with(root) && depth(root) == best);
    match (hits.next(), hits.next()) {
        (Some((id, _)), None) => Active { project: Some(id), diagnostic: None },
        _ => closed("several project ids are registered at the same root; repair the duplicate registration".into()),
    }
}

/// `<ws>/local/projects/<id>`: refused when the directory is a symlink, so a layer cannot point at another's files.
pub fn layer_dir(ws_dir: &Path, id: &ProjectId) -> Result<PathBuf, String> {
    let dir = ws_dir.join("local/projects").join(id.as_str());
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.file_type().is_symlink() => Err(format!("the layer of project {} is a symlink", id.as_str())),
        _ => Ok(dir),
    }
}

/// One loadable file with the layer it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedSource {
    /// `home` or `project:<id>`.
    pub layer: String,
    /// For a project file: the home logical path it specializes. Both are listed; neither replaces the other.
    pub overrides: Option<String>,
    pub file: SourceFile,
}

/// The Markdown directories a project layer holds, as `(subdirectory, zone, default source)`. Everything a project
/// layer lists is untrusted until its own tier says otherwise; registration ratifies nothing (`specs/058` §4.1).
const PROJECT_DIRS: [(&str, &str, &str); 4] = [
    ("memory/agents", "memory", "external_untrusted"),
    ("knowledge", "knowledge", "external_untrusted"),
    ("context", "context", "external_untrusted"),
    ("staged", "staged", "external_untrusted"),
];

/// The active project's layer files alone, with the layer label.
fn project_files(ws_dir: &Path, active: &Active) -> (String, Vec<SourceFile>) {
    let mut project: Vec<SourceFile> = Vec::new();
    let mut label = String::new();
    if let Some(id) = &active.project {
        if let Ok(dir) = layer_dir(ws_dir, id) {
            label = id.label();
            for (sub, zone, source) in PROJECT_DIRS {
                md_files(&dir.join(sub), sub, zone, source, &mut project);
            }
        }
    }
    (label, project)
}

/// The home layer, then the active project's layer. A project file's `rel_path` is its logical path inside the layer,
/// so the two layers are told apart by `layer`, never by a rewritten name.
pub fn layered_sources(ws_dir: &Path, active: &Active) -> Vec<ScopedSource> {
    let home = crate::kb::sources(ws_dir);
    let (label, project) = project_files(ws_dir, active);
    let home_paths: std::collections::BTreeSet<String> = home.iter().map(|f| f.rel_path.clone()).collect();
    let mut out: Vec<ScopedSource> = home.into_iter().map(|file| ScopedSource { layer: "home".into(), overrides: None, file }).collect();
    for file in project {
        let overrides = home_paths.contains(&file.rel_path).then(|| file.rel_path.clone());
        out.push(ScopedSource { layer: label.clone(), overrides, file });
    }
    out
}

/// The project a mission was stamped with at creation (`<mission>/scope`), only while it is still the default project or
/// registered here. No file, a malformed id or an unregistered one means the mission is home-only.
pub fn mission_scope(ws_dir: &Path, mission_dir: &Path) -> Option<ProjectId> {
    let id = ProjectId::parse(std::fs::read_to_string(mission_dir.join("scope")).ok()?.trim()).ok()?;
    is_registered(ws_dir, &id).then_some(id)
}

/// Most bytes of project layer a span's prompt carries.
const PROMPT_LAYER_BYTES: usize = 12_000;

/// What a span of `role` in a mission scoped to a project reads from that project's layer, as one slice text with each
/// file under its layer label, or `None` for a home-only mission or an empty layer. Never another project's layer, never
/// another role's memory, never staged notes (those load only through an explicit search). The caller marks the span
/// tainted: project content is untrusted until its own tier says otherwise.
pub fn prompt_layer(ws_dir: &Path, mission_dir: &Path, role: &str) -> Option<String> {
    let id = mission_scope(ws_dir, mission_dir)?;
    let (label, files) = project_files(ws_dir, &Active { project: Some(id), diagnostic: None });
    let own_memory = format!("memory/agents/{}/", role);
    let mut text = String::new();
    for f in files.iter().filter(|f| f.zone != "staged" && (f.zone != "memory" || f.rel_path.starts_with(&own_memory))) {
        let Ok(body) = std::fs::read_to_string(&f.path) else { continue };
        let entry = format!("[layer: {}, path: {}, source: {}]\n{}\n\n", label, f.rel_path, f.default_source, body.trim());
        if text.len() + entry.len() > PROMPT_LAYER_BYTES {
            break;
        }
        text.push_str(&entry);
    }
    (!text.is_empty()).then_some(text)
}

/// Where a staged note goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Home,
    Project(ProjectId),
}

/// `specs/058` §4.2. With nothing requested a note goes to the active project. An agent can ask for nothing else; the
/// operator may name home or another registered project, and the id is validated here.
pub fn route_staged(ws_dir: &Path, active: &Active, requested: Option<&Destination>, by_operator: bool) -> Result<Destination, String> {
    match (requested, &active.project) {
        (None, Some(id)) => Ok(Destination::Project(id.clone())),
        (None, None) => Err(active.diagnostic.clone().unwrap_or_else(|| "no project scope is active".into())),
        (Some(Destination::Project(id)), Some(a)) if id == a => Ok(Destination::Project(id.clone())),
        (Some(_), _) if !by_operator => Err("only the operator can choose a scope other than the active one".into()),
        (Some(Destination::Project(id)), _) if is_registered(ws_dir, id) => Ok(Destination::Project(id.clone())),
        (Some(Destination::Project(id)), _) => Err(format!("project {} is not registered in this workspace", id.as_str())),
        (Some(Destination::Home), _) => Ok(Destination::Home),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const C: &str = "cccccccccccccccccccccccccccccccc";

    fn register(ws: &Path, entries: &[(&str, &Path)]) {
        let projects: serde_json::Map<String, serde_json::Value> =
            entries.iter().map(|(id, root)| (id.to_string(), serde_json::json!({"project_id": id, "root": root, "display_name": id, "created_unix": 1}))).collect();
        fs::create_dir_all(ws.join("local")).unwrap();
        fs::write(ws.join("local/projects.json"), serde_json::json!({"schema": 1, "projects": projects}).to_string()).unwrap();
    }

    fn put(path: PathBuf, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    struct Fx {
        _d: tempfile::TempDir,
        ws: PathBuf,
        a: PathBuf,
        b: PathBuf,
        other: PathBuf,
    }

    fn fx() -> Fx {
        let d = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(d.path()).unwrap();
        let (ws, a, b, other) = (base.join("ws"), base.join("a"), base.join("a/inner/b"), base.join("elsewhere"));
        for p in [&ws, &a, &b, &other, &a.join("src/deep")] {
            fs::create_dir_all(p).unwrap();
        }
        register(&ws, &[(A, &a), (B, &b)]);
        Fx { _d: d, ws, a, b, other }
    }

    fn id(s: &str) -> ProjectId {
        ProjectId::parse(s).unwrap()
    }

    #[test]
    fn a_subdirectory_of_a_root_resolves_to_it_and_the_most_specific_nested_root_wins() {
        let f = fx();
        assert_eq!(resolve(&f.ws, &f.a.join("src/deep")).project, Some(id(A)));
        assert_eq!(resolve(&f.ws, &f.a.join("inner")).project, Some(id(A)), "the parent resumes outside the child's subtree");
        assert_eq!(resolve(&f.ws, &f.b).project, Some(id(B)));
        assert_eq!(resolve(&f.ws, &f.a.join("inner/b")).project, Some(id(B)));
    }

    #[test]
    fn an_unregistered_directory_gets_the_persistent_default_project_layer() {
        let f = fx();
        let active = resolve(&f.ws, &f.other);
        assert_eq!(active.project, Some(ProjectId::default_project()));
        put(layer_dir(&f.ws, &ProjectId::default_project()).unwrap().join("memory/agents/builder/lessons.md"), "default lesson\n");
        let items = layered_sources(&f.ws, &active);
        assert!(items.iter().any(|s| s.layer == "project:default" && s.file.rel_path == "memory/agents/builder/lessons.md"));
        assert!(resolve(&f.ws, &f.ws.join("nope")).project.is_none(), "a directory that cannot be resolved fails closed");
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(resolve(empty.path(), &f.other).project, Some(ProjectId::default_project()), "no registry at all is the default project");
    }

    #[test]
    fn a_corrupt_or_ambiguous_registry_fails_closed_to_home_only() {
        let f = fx();
        fs::write(f.ws.join("local/projects.json"), "{not json").unwrap();
        let a = resolve(&f.ws, &f.a);
        assert!(a.project.is_none() && a.diagnostic.as_deref().unwrap().contains("corrupt"));
        register(&f.ws, &[(A, &f.a), (C, &f.a)]);
        let a = resolve(&f.ws, &f.a);
        assert!(a.project.is_none() && a.diagnostic.as_deref().unwrap().contains("same root"));
        fs::write(f.ws.join("local/projects.json"), serde_json::json!({"schema": 1, "projects": {"../x": {"project_id": "../x", "root": "/"}}}).to_string()).unwrap();
        assert!(resolve(&f.ws, &f.a).project.is_none(), "a spoofed id in the registry is corrupt");
        put(f.ws.join("local/memory/agents/builder/lessons.md"), "home lesson\n");
        assert!(layered_sources(&f.ws, &resolve(&f.ws, &f.a)).iter().all(|s| s.layer == "home"), "home stays available");
    }

    #[test]
    fn the_read_path_lists_home_and_project_items_with_labels_and_labels_an_override() {
        let f = fx();
        put(f.ws.join("local/memory/agents/builder/lessons.md"), "home lesson\n");
        put(f.ws.join("local/memory/agents/builder/only-home.md"), "x\n");
        let pa = layer_dir(&f.ws, &id(A)).unwrap();
        put(pa.join("memory/agents/builder/lessons.md"), "project lesson\n");
        put(pa.join("context/notes.md"), "project note\n");
        let items = layered_sources(&f.ws, &resolve(&f.ws, &f.a.join("src")));
        let find = |layer: &str, rel: &str| items.iter().find(|s| s.layer == layer && s.file.rel_path == rel);
        assert!(find("home", "memory/agents/builder/lessons.md").unwrap().overrides.is_none());
        assert!(find("home", "memory/agents/builder/only-home.md").is_some());
        let over = find(&id(A).label(), "memory/agents/builder/lessons.md").unwrap();
        assert_eq!(over.overrides.as_deref(), Some("memory/agents/builder/lessons.md"), "labelled, not silently merged");
        assert!(find(&id(A).label(), "context/notes.md").unwrap().overrides.is_none());
        assert_eq!(items.iter().filter(|s| s.file.rel_path == "memory/agents/builder/lessons.md").count(), 2, "both layers are listed");
        assert!(items.iter().filter(|s| s.layer != "home").all(|s| s.file.default_source == "external_untrusted"), "a project file gains no trust");
    }

    #[test]
    fn project_b_never_sees_project_a_and_a_spoofed_id_or_path_is_refused() {
        let f = fx();
        put(layer_dir(&f.ws, &id(A)).unwrap().join("memory/agents/builder/lessons.md"), "SECRET-A\n");
        let items = layered_sources(&f.ws, &resolve(&f.ws, &f.b));
        assert!(items.iter().all(|s| s.layer == "home" || s.layer == id(B).label()));
        assert!(items.iter().all(|s| !fs::read_to_string(&s.file.path).unwrap_or_default().contains("SECRET-A")));
        let default = layered_sources(&f.ws, &resolve(&f.ws, &f.other));
        assert!(default.iter().all(|s| !fs::read_to_string(&s.file.path).unwrap_or_default().contains("SECRET-A")), "nor does the default project");
        for bad in ["../" , "..", "../a", "a/../b", "/etc", "", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "aaaa", "default/../x", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{0}"] {
            assert!(ProjectId::parse(bad).is_err(), "{:?}", bad);
        }
        let link = f.ws.join("local/projects").join(C);
        std::os::unix::fs::symlink(layer_dir(&f.ws, &id(A)).unwrap(), &link).unwrap();
        assert!(layer_dir(&f.ws, &id(C)).is_err(), "a layer that is a symlink to another's is refused");
    }

    #[test]
    fn staged_writes_route_to_the_active_scope_and_only_the_operator_chooses_another() {
        let f = fx();
        let in_a = resolve(&f.ws, &f.a);
        assert_eq!(route_staged(&f.ws, &in_a, None, false), Ok(Destination::Project(id(A))));
        assert_eq!(route_staged(&f.ws, &resolve(&f.ws, &f.other), None, false), Ok(Destination::Project(ProjectId::default_project())));
        assert_eq!(route_staged(&f.ws, &in_a, Some(&Destination::Project(id(A))), false), Ok(Destination::Project(id(A))));
        assert!(route_staged(&f.ws, &in_a, Some(&Destination::Project(id(B))), false).is_err(), "an agent cannot reach project B");
        assert!(route_staged(&f.ws, &in_a, Some(&Destination::Home), false).is_err(), "nor the broader home layer");
        assert_eq!(route_staged(&f.ws, &in_a, Some(&Destination::Home), true), Ok(Destination::Home));
        assert_eq!(route_staged(&f.ws, &in_a, Some(&Destination::Project(id(B))), true), Ok(Destination::Project(id(B))));
        assert!(route_staged(&f.ws, &in_a, Some(&Destination::Project(id(C))), true).is_err(), "an unregistered id is refused even for the operator");
        let closed = Active { project: None, diagnostic: Some("corrupt".into()) };
        assert!(route_staged(&f.ws, &closed, None, false).is_err(), "a failed-closed scope writes nowhere");
    }

    #[test]
    fn a_missions_prompt_layer_is_its_own_projects_labelled_and_never_another_projects() {
        let f = fx();
        for (id, body) in [(A, "alpha convention"), (B, "bravo convention")] {
            put(layer_dir(&f.ws, &id_of(id)).unwrap().join("context/conv.md"), body);
            put(layer_dir(&f.ws, &id_of(id)).unwrap().join("memory/agents/builder/lessons.md"), &format!("{} lesson", body));
            put(layer_dir(&f.ws, &id_of(id)).unwrap().join("memory/agents/auditor/lessons.md"), &format!("{} auditor", body));
            put(layer_dir(&f.ws, &id_of(id)).unwrap().join("staged/m/n.md"), &format!("{} staged", body));
        }
        let mission = |name: &str, scope: Option<&str>| {
            let d = f.ws.join("local/missions").join(name);
            fs::create_dir_all(&d).unwrap();
            if let Some(s) = scope {
                fs::write(d.join("scope"), s).unwrap();
            }
            d
        };
        let text = prompt_layer(&f.ws, &mission("ma", Some(A)), "builder").unwrap();
        assert!(text.contains(&format!("[layer: project:{}, path: context/conv.md, source: external_untrusted]", A)), "{}", text);
        assert!(text.contains("alpha convention") && text.contains("alpha convention lesson"));
        assert!(!text.contains("bravo") && !text.contains("auditor") && !text.contains("staged"));
        assert!(prompt_layer(&f.ws, &mission("mb", Some(B)), "builder").unwrap().contains("bravo convention"));
        assert_eq!(prompt_layer(&f.ws, &mission("none", None), "builder"), None, "a mission without scope loads nothing extra");
        let unregistered = format!("{}ccc", &C[..29]);
        for bad in [C, "../b", "", unregistered.as_str()] {
            assert_eq!(prompt_layer(&f.ws, &mission("bad", Some(bad)), "builder"), None, "{:?}", bad);
        }
    }

    fn id_of(s: &str) -> ProjectId {
        ProjectId::parse(s).unwrap()
    }
}
