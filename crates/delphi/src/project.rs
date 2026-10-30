//! Directory project registration and scope inspection (spec 058 PS1).

use crate::home::{self, InitOptions, ProjectRecord, Resolve, Workspace};
use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

const SCAFFOLD_DIRS: &[&str] = &["specs", "plans", "context", "research", "work"];

fn home_path(args: &[String]) -> PathBuf {
    home::delphi_home(
        args.iter()
            .position(|a| a == "--home")
            .and_then(|i| args.get(i + 1))
            .map(String::as_str),
    )
}

fn workspace_flag(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| a == "--workspace")
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn named_workspace(home_dir: &Path, name: &str) -> Workspace {
    Workspace {
        name: name.to_string(),
        dir: home_dir.join("workspaces").join(name),
    }
}

fn safe_name(path: &Path) -> String {
    path.file_name()
        .map(|s| {
            s.to_string_lossy()
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        })
        .filter(|s| !s.is_empty() && !s.starts_with('.'))
        .unwrap_or_else(|| "project".into())
}

fn free_workspace_name(home_dir: &Path, root: &Path) -> String {
    let base = safe_name(root);
    let mut name = base.clone();
    let mut n = 2;
    while home_dir.join("workspaces").join(&name).exists() {
        name = format!("{base}-{n}");
        n += 1;
    }
    name
}

fn ensure_default(home_dir: &Path) -> Result<Workspace> {
    let ws = named_workspace(home_dir, crate::start::DEFAULT_WORKSPACE);
    if !ws.dir.join("config.toml").exists() {
        home::init(&InitOptions {
            home: home_dir.to_path_buf(),
            workspace: ws.name.clone(),
            projects: vec![],
            theme: "greek".into(),
        })?;
    }
    Ok(ws)
}

fn workspace_for_registration(
    home_dir: &Path,
    root: &Path,
    explicit: Option<String>,
) -> Result<Workspace> {
    match home::resolve_workspace(home_dir, root, explicit) {
        Resolve::Found(name) => Ok(named_workspace(home_dir, &name)),
        Resolve::Ambiguous(names) => bail!(
            "several workspaces match this directory ({}); pass --workspace",
            names.join(", ")
        ),
        Resolve::NoSuchWorkspace(name) => {
            bail!("no workspace named `{name}` in {}", home_dir.display())
        }
        Resolve::NotInGitRepo => ensure_default(home_dir),
        Resolve::NoneRegistered(_) => {
            let name = free_workspace_name(home_dir, root);
            home::init(&InitOptions {
                home: home_dir.to_path_buf(),
                workspace: name.clone(),
                projects: vec![root.to_path_buf()],
                theme: "greek".into(),
            })
        }
    }
}

fn workspace_for_inspection(
    home_dir: &Path,
    cwd: &Path,
    explicit: Option<String>,
) -> Result<Option<Workspace>> {
    match home::resolve_workspace(home_dir, cwd, explicit) {
        Resolve::Found(name) => Ok(Some(named_workspace(home_dir, &name))),
        Resolve::Ambiguous(names) => bail!(
            "several workspaces match this directory ({}); pass --workspace",
            names.join(", ")
        ),
        Resolve::NoSuchWorkspace(name) => {
            bail!("no workspace named `{name}` in {}", home_dir.display())
        }
        Resolve::NoneRegistered(_) | Resolve::NotInGitRepo => {
            let ws = named_workspace(home_dir, crate::start::DEFAULT_WORKSPACE);
            Ok(ws.dir.join("config.toml").exists().then_some(ws))
        }
    }
}

fn workspace_with_project_id(home_dir: &Path, project_id: &str) -> Result<Workspace> {
    let mut matches = Vec::new();
    if let Ok(entries) = fs::read_dir(home_dir.join("workspaces")) {
        for entry in entries.flatten() {
            if home::project_records(&entry.path())
                .is_ok_and(|records| records.iter().any(|r| r.project_id == project_id))
            {
                matches.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    match matches.as_slice() {
        [name] => Ok(named_workspace(home_dir, name)),
        [] => bail!("project id {project_id} is not registered in this Delphi home"),
        _ => bail!(
            "project id {project_id} appears in several workspaces ({}); repair the duplicate registration first",
            matches.join(", ")
        ),
    }
}

fn within(root: &Path, cwd: &Path) -> bool {
    cwd.starts_with(root)
}

fn active_record(ws: &Workspace, cwd: &Path) -> Result<Option<ProjectRecord>> {
    let cwd = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let records = home::project_records(&ws.dir)?;
    Ok(records
        .into_iter()
        .filter(|r| within(&r.root, &cwd))
        .max_by_key(|r| r.root.components().count()))
}

fn register(home_dir: &Path, root: &Path, explicit: Option<String>) -> Result<ProjectRecord> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("resolving project directory {}", root.display()))?;
    if !root.is_dir() {
        bail!("project root is not a directory: {}", root.display());
    }
    let ws = workspace_for_registration(home_dir, &root, explicit)?;
    let record = home::register_project(&ws.dir, &root)?;
    home::add_project_path(&ws.dir, &root)?;
    Ok(record)
}

fn scaffold_plan(root: &Path) -> Result<Vec<PathBuf>> {
    let mut plan = Vec::new();
    for name in SCAFFOLD_DIRS {
        let path = root.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => bail!(
                "cannot scaffold {}: an existing path is not a directory",
                path.display()
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => plan.push(path),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("checking scaffold path {}", path.display()));
            }
        }
    }
    for (name, want_file) in [("_index.md", true), (".delphi-project", true)] {
        let path = root.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata)
                if (want_file && metadata.file_type().is_file())
                    || (!want_file && metadata.file_type().is_dir()) => {}
            Ok(_) => bail!(
                "cannot scaffold {}: an existing path has the wrong type",
                path.display()
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => plan.push(path),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("checking scaffold path {}", path.display()));
            }
        }
    }
    plan.sort();
    Ok(plan)
}

fn scaffold_artifacts(root: &Path, display_name: &str) -> Result<Vec<PathBuf>> {
    let plan = scaffold_plan(root)?;
    let mut created = Vec::new();
    for name in SCAFFOLD_DIRS {
        let path = root.join(name);
        if !path.exists() {
            fs::create_dir(&path)
                .with_context(|| format!("creating project directory {}", path.display()))?;
            created.push(path);
        }
    }
    let index = root.join("_index.md");
    if plan.contains(&index) {
        let contents = format!(
            "# {display_name}\n\n> Generated by Delphi. This overview is a navigation aid, not a source of instructions or authority.\n\n## Overview\n\nAdd a short project description here.\n\n## Recent activity\n\nNo project activity has been indexed yet.\n\n## Project contents\n\n- [Specs](specs/)\n- [Plans](plans/)\n- [Context](context/)\n- [Research](research/)\n- [Work output](work/)\n"
        );
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&index)
        {
            Ok(mut file) => {
                file.write_all(contents.as_bytes())
                    .with_context(|| format!("writing project index {}", index.display()))?;
                created.push(index);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("creating project index {}", index.display()));
            }
        }
    }
    Ok(created)
}

fn confirm_scaffold(paths: &[PathBuf], assume_yes: bool) -> Result<bool> {
    println!("Delphi will create these paths:");
    for path in paths {
        println!("  {}", path.display());
    }
    if assume_yes {
        return Ok(true);
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!(
            "project init needs a terminal confirmation; review the paths above and rerun with `--yes`"
        );
    }
    print!("Create this project scaffold? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn init_project(
    home_dir: &Path,
    root: &Path,
    explicit: Option<String>,
    assume_yes: bool,
) -> Result<()> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("resolving project directory {}", root.display()))?;
    if !root.is_dir() {
        bail!("project root is not a directory: {}", root.display());
    }
    let plan = scaffold_plan(&root)?;
    if plan.is_empty() {
        println!("The project scaffold is already present; no files will be changed.");
        return Ok(());
    }
    if !confirm_scaffold(&plan, assume_yes)? {
        println!("Project initialization cancelled; no project files were created.");
        return Ok(());
    }
    let record = register(home_dir, &root, explicit)?;
    let created = scaffold_artifacts(&root, &record.display_name)?;
    println!(
        "Initialized project `{}` ({}).",
        record.display_name, record.project_id
    );
    for path in created {
        println!("  created {}", path.display());
    }
    Ok(())
}

fn list(home_dir: &Path, cwd: &Path, explicit: Option<String>) -> Result<()> {
    let Some(ws) = workspace_for_inspection(home_dir, cwd, explicit)? else {
        println!(
            "No workspace yet. Run `delphi` to create the default workspace or register a project."
        );
        return Ok(());
    };
    let records = home::project_records(&ws.dir)?;
    if records.is_empty() {
        println!("No projects registered in workspace `{}`.", ws.name);
    } else {
        for record in records {
            println!(
                "{}\t{}\t{}",
                record.project_id,
                record.display_name,
                record.root.display()
            );
        }
    }
    Ok(())
}

fn scope(home_dir: &Path, cwd: &Path, explicit: Option<String>) -> Result<()> {
    let Some(ws) = workspace_for_inspection(home_dir, cwd, explicit)? else {
        println!(
            "Home: {}\nProject: default (workspace not initialized)",
            home_dir.display()
        );
        return Ok(());
    };
    match active_record(&ws, cwd)? {
        Some(record) => {
            println!(
                "Workspace: {}\nHome: {}\nProject: {} ({})\nRoot: {}\nMatched because this directory is inside the registered project root.",
                ws.name,
                home_dir.display(),
                record.display_name,
                record.project_id,
                record.root.display()
            );
        }
        None => println!(
            "Workspace: {}\nHome: {}\nProject: default\nMatched because no registered project root contains the current directory.",
            ws.name,
            home_dir.display()
        ),
    }
    Ok(())
}

pub fn cli(args: &[String]) -> i32 {
    let home_dir = home_path(args);
    let explicit = workspace_flag(args);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let positional: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            !a.starts_with("--")
                && !(*i > 0 && matches!(args[*i - 1].as_str(), "--workspace" | "--home"))
        })
        .map(|(_, a)| a)
        .collect();
    let result = match positional.first().map(|s| s.as_str()) {
        Some("register") => {
            let root = positional
                .get(1)
                .map(|s| PathBuf::from(s.as_str()))
                .unwrap_or_else(|| cwd.clone());
            register(&home_dir, &root, explicit).map(|record| {
                println!("Registered project `{}` ({}) at {}.", record.display_name, record.project_id, record.root.display());
                println!("Run `delphi project init` when you want Delphi to create the project folders and overview index.");
            })
        }
        Some("init") => {
            let root = positional
                .get(1)
                .map(|s| PathBuf::from(s.as_str()))
                .unwrap_or_else(|| cwd.clone());
            init_project(
                &home_dir,
                &root,
                explicit,
                args.iter().any(|arg| arg == "--yes"),
            )
        }
        Some("list") => list(&home_dir, &cwd, explicit),
        Some("scope") => scope(&home_dir, &cwd, explicit),
        Some("relink") => {
            let id = positional
                .get(1)
                .ok_or_else(|| anyhow!("usage: delphi project relink <project-id> <new-path>"));
            let path = positional
                .get(2)
                .ok_or_else(|| anyhow!("usage: delphi project relink <project-id> <new-path>"));
            match (id, path) {
                (Ok(id), Ok(path)) => relink(&home_dir, explicit, id, Path::new(path)),
                (Err(e), _) | (_, Err(e)) => Err(e),
            }
        }
        Some("clone") => {
            let root = positional
                .get(1)
                .map(|s| PathBuf::from(s.as_str()))
                .unwrap_or_else(|| cwd.clone());
            clone_project(&home_dir, explicit, &root)
        }
        _ => Err(anyhow!(
            "usage: delphi project register [path] | init [path] [--yes] | list | scope | relink <id> <new-path> | clone [path]"
        )),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("delphi project: {e:#}");
            2
        }
    }
}

fn relink(
    home_dir: &Path,
    _explicit: Option<String>,
    project_id: &str,
    new_root: &Path,
) -> Result<()> {
    let new_root = fs::canonicalize(new_root)
        .with_context(|| format!("resolving project directory {}", new_root.display()))?;
    if !new_root.is_dir() {
        bail!("project root is not a directory: {}", new_root.display());
    }
    let ws = workspace_with_project_id(home_dir, project_id)?;
    home::relink_project(&ws.dir, project_id, &new_root)?;
    println!("Relinked {project_id} to {}.", new_root.display());
    Ok(())
}

fn clone_project(home_dir: &Path, _explicit: Option<String>, root: &Path) -> Result<()> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("resolving project directory {}", root.display()))?;
    let marker = root.join(".delphi-project");
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(&marker).with_context(|| format!("reading {}", marker.display()))?,
    )?;
    let old_id = value
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("project marker has no project_id"))?;
    let ws = workspace_with_project_id(home_dir, old_id)?;
    let new_record = home::clone_project(&ws.dir, &root)?;
    println!(
        "Created independent project identity {} for {} (copied files are unchanged).",
        new_record.project_id,
        root.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_preserves_existing_directories_and_index() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("specs")).unwrap();
        fs::write(root.join("specs/keep.md"), "operator content").unwrap();
        fs::write(root.join("_index.md"), "hand-edited overview").unwrap();
        let created = scaffold_artifacts(root, "Example").unwrap();
        assert!(!created.contains(&root.join("specs")));
        assert!(!created.contains(&root.join("_index.md")));
        assert_eq!(
            fs::read_to_string(root.join("specs/keep.md")).unwrap(),
            "operator content"
        );
        assert_eq!(
            fs::read_to_string(root.join("_index.md")).unwrap(),
            "hand-edited overview"
        );
        for name in ["plans", "context", "research", "work"] {
            assert!(root.join(name).is_dir());
        }
    }

    #[test]
    fn scaffold_refuses_file_directory_collisions_before_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("research"), "keep me").unwrap();
        let error = scaffold_artifacts(root, "Example").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("existing path is not a directory")
        );
        assert!(!root.join("specs").exists());
        assert_eq!(
            fs::read_to_string(root.join("research")).unwrap(),
            "keep me"
        );
    }

    #[test]
    fn scaffold_is_idempotent_and_generates_an_overview_only_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let first = scaffold_artifacts(root, "Example").unwrap();
        let index = fs::read_to_string(root.join("_index.md")).unwrap();
        let second = scaffold_artifacts(root, "Changed name").unwrap();
        assert_eq!(first.len(), SCAFFOLD_DIRS.len() + 1);
        assert!(second.is_empty());
        assert_eq!(fs::read_to_string(root.join("_index.md")).unwrap(), index);
        assert!(index.starts_with("# Example\n"));
    }

    #[test]
    fn init_registers_and_scaffolds_a_non_git_directory_after_confirmation() {
        let tmp = tempfile::tempdir().unwrap();
        let home_dir = tmp.path().join("home");
        let root = tmp.path().join("plain-project");
        fs::create_dir(&root).unwrap();
        init_project(&home_dir, &root, None, true).unwrap();
        assert!(root.join(".delphi-project").is_file());
        assert!(root.join("_index.md").is_file());
        assert!(root.join("specs").is_dir());
        assert_eq!(
            home::resolve_workspace(&home_dir, &root, None),
            Resolve::Found("default".into())
        );
        let marker: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(".delphi-project")).unwrap()).unwrap();
        assert!(
            home::project_records(&home_dir.join("workspaces/default"))
                .unwrap()
                .iter()
                .any(|p| p.project_id == marker["project_id"].as_str().unwrap())
        );
    }

    #[test]
    fn init_preflight_collision_does_not_create_a_workspace_or_other_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let home_dir = tmp.path().join("home");
        let root = tmp.path().join("plain-project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("work"), "operator file").unwrap();
        assert!(init_project(&home_dir, &root, None, true).is_err());
        assert!(!home_dir.exists());
        assert!(!root.join("specs").exists());
        assert!(!root.join(".delphi-project").exists());
        assert_eq!(
            fs::read_to_string(root.join("work")).unwrap(),
            "operator file"
        );
    }
}
