//! `delphi skills check|export|import`: Delphi skills and the agentskills.io format, both ways (gap plan §3.1).
//!
//! **Export** makes a skill other tools can load. The standard has no place for Delphi's four structured fields
//! (`authorized_agents`, `input_schema`, `output_schema`, `required_permissions`), and its `metadata` is a map of
//! strings, so each is stored there as a JSON string under a `delphi.` key. Nothing is lost: importing the export
//! gives the original fields back.
//!
//! **Import** brings a skill in from anywhere, and treats it as untrusted instructions that will be put in front
//! of an agent:
//! * it is written under `team/skills/custom/<name>/`, which is protected: the new skill shows up as a pending Red
//!   proposal and is not loaded until the operator reads it and ratifies it (`/ratify`);
//! * the operator must say which roles may use it (`--agents`): a foreign skill never decides that for itself;
//! * **scripts are never imported** (a skill's `scripts/` directory is skipped and listed), as are symlinks and
//!   hidden files; declared `deterministic_scripts` are dropped. Executable code comes in only when the operator
//!   adds it on purpose;
//! * nothing existing is overwritten, and the result must pass the same checks as any custom skill.

use crate::frontmatter::{self, get_str};
use crate::lint;
use serde_json::Value as Json;
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};

/// The structured fields Delphi adds to the standard's.
const DELPHI_FIELDS: [&str; 4] = ["authorized_agents", "input_schema", "output_schema", "required_permissions"];
const MAX_FILES: usize = 100;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024;

fn key(k: &str) -> Value {
    Value::String(k.into())
}

fn split(text: &str) -> Result<(Mapping, String), String> {
    let doc = frontmatter::parse(text)?;
    Ok((doc.frontmatter.ok_or("the file has no frontmatter (a skill starts with `---`, then name and description)")?, doc.body))
}

fn join(fm: &Mapping, body: &str) -> Result<String, String> {
    let yaml = serde_yaml::to_string(&Value::Mapping(fm.clone())).map_err(|e| e.to_string())?;
    Ok(format!("---\n{}---\n{}", yaml, body))
}

/// What the agentskills.io specification requires of a skill, as of the version this was written against.
pub fn standard_errors(fm: &Mapping, dir: &str) -> Vec<String> {
    let mut e = Vec::new();
    match get_str(fm, "name") {
        Some(n) if n.chars().count() <= 64 && regex::Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*$").unwrap().is_match(n) && n == dir => {}
        n => e.push(format!("name `{}` must be 1-64 lowercase letters, digits and single hyphens (not starting or ending with one) and equal its directory `{}`", n.unwrap_or(""), dir)),
    }
    match get_str(fm, "description") {
        Some(d) if !d.trim().is_empty() && d.chars().count() <= 1024 => {}
        _ => e.push("description must be 1-1024 characters".into()),
    }
    for k in ["license", "allowed-tools"] {
        if fm.get(key(k)).is_some_and(|v| !v.is_string()) {
            e.push(format!("{} must be a string", k));
        }
    }
    if let Some(v) = fm.get(key("compatibility")) {
        if !v.as_str().is_some_and(|s| !s.is_empty() && s.chars().count() <= 500) {
            e.push("compatibility must be 1-500 characters".into());
        }
    }
    if let Some(v) = fm.get(key("metadata")) {
        if !v.as_mapping().is_some_and(|m| m.iter().all(|(k, v)| k.is_string() && v.is_string())) {
            e.push("metadata must map strings to strings".into());
        }
    }
    e
}

/// A Delphi skill, as a standard one: the four structured fields go to `metadata` as `delphi.*` JSON strings.
pub fn export_text(skill_text: &str) -> Result<String, String> {
    let (fm, body) = split(skill_text)?;
    let mut out = Mapping::new();
    for k in ["name", "description", "license", "compatibility"] {
        if let Some(v) = fm.get(key(k)) {
            out.insert(key(k), v.clone());
        }
    }
    let mut meta = fm.get(key("metadata")).and_then(|m| m.as_mapping()).cloned().unwrap_or_default();
    for f in DELPHI_FIELDS.iter().copied().chain(["deterministic_scripts"]) {
        if let Some(v) = fm.get(key(f)) {
            let json = serde_json::to_string(&serde_json::to_value(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            meta.insert(key(&format!("delphi.{}", f)), Value::String(json));
        }
    }
    if !meta.is_empty() {
        out.insert(key("metadata"), Value::Mapping(meta));
    }
    join(&out, &body)
}

pub struct ImportOpts {
    /// The roles allowed to use the skill. The operator's decision; a foreign skill does not make it.
    pub agents: Vec<String>,
    /// A shorter description, when the original is over Delphi's 200 characters.
    pub description: Option<String>,
}

pub struct Imported {
    pub text: String,
    pub notes: Vec<String>,
}

/// A standard skill (or a Delphi export) as a Delphi custom skill.
pub fn import_text(std_text: &str, dir: &str, o: &ImportOpts) -> Result<Imported, Vec<String>> {
    let (fm, body) = split(std_text).map_err(|e| vec![e])?;
    let errs = standard_errors(&fm, dir);
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut notes = Vec::new();
    let mut meta = fm.get(key("metadata")).and_then(|m| m.as_mapping()).cloned().unwrap_or_default();
    let from_meta = |name: &str, meta: &mut Mapping| -> Option<Value> {
        let v = meta.remove(key(&format!("delphi.{}", name)))?;
        serde_json::from_str::<Json>(v.as_str()?).ok().and_then(|j| serde_yaml::to_value(j).ok())
    };
    let mut out = Mapping::new();
    out.insert(key("name"), fm.get(key("name")).cloned().unwrap_or_default());
    let mut desc = get_str(&fm, "description").unwrap_or("").to_string();
    if let Some(d) = &o.description {
        desc = d.clone();
    }
    out.insert(key("description"), Value::String(desc));
    // The roles come from the operator, never from the skill; an export's own list is shown as a hint only.
    let hint = from_meta("authorized_agents", &mut meta);
    if o.agents.is_empty() {
        let was = hint.and_then(|v| serde_yaml::to_string(&v).ok()).map(|s| s.replace('\n', " ")).unwrap_or_default();
        return Err(vec![format!("say which roles may use this skill: --agents scout,architect (roles: orchestrator, builder, architect, investigator, scout, strategist, red-team, writer, integrator){}", if was.is_empty() { String::new() } else { format!("; the export listed: {}", was.trim()) })]);
    }
    out.insert(key("authorized_agents"), Value::Sequence(o.agents.iter().map(|a| Value::String(a.clone())).collect()));
    let object = || {
        let mut m = Mapping::new();
        m.insert(key("type"), key("object"));
        Value::Mapping(m)
    };
    for f in ["input_schema", "output_schema"] {
        let v = from_meta(f, &mut meta).filter(|v| v.as_mapping().is_some_and(|m| m.contains_key(key("type"))));
        if v.is_none() {
            notes.push(format!("{} was not given: set to a bare `{{type: object}}`", f));
        }
        out.insert(key(f), v.unwrap_or_else(object));
    }
    let perms = from_meta("required_permissions", &mut meta);
    if perms.is_none() {
        notes.push("required_permissions was not given: set to `read:all` only".into());
    }
    out.insert(key("required_permissions"), perms.unwrap_or_else(|| Value::Sequence(vec![key("read:all")])));
    if from_meta("deterministic_scripts", &mut meta).is_some() {
        notes.push("declared deterministic_scripts were dropped: scripts are never imported".into());
    }
    for k in ["license", "compatibility"] {
        if let Some(v) = fm.get(key(k)) {
            out.insert(key(k), v.clone());
        }
    }
    if fm.get(key("allowed-tools")).is_some() {
        notes.push("allowed-tools was ignored: it names another tool's tools, and Delphi grants tools through required_permissions and the role".into());
    }
    meta.insert(key("delphi.imported"), Value::String("true".into()));
    out.insert(key("metadata"), Value::Mapping(meta));

    let mut problems = lint::skill_errors(&out, dir, true);
    let lines = body.lines().count();
    if lines > lint::BODY_LINE_CAP {
        problems.push(format!("body is {} lines (cap {})", lines, lint::BODY_LINE_CAP));
    }
    if problems.iter().any(|p| p.starts_with("description must be")) {
        problems.push("shorten it for Delphi (200 characters at most) with --description \"…\"".into());
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let text = join(&out, &body).map_err(|e| vec![e])?;
    Ok(Imported { text, notes })
}

/// The other files of a skill package, safe to copy: regular files only, no scripts, no symlinks, no hidden files.
#[derive(Debug)]
pub struct Package {
    pub files: Vec<(String, Vec<u8>)>,
    pub skipped: Vec<String>,
}

pub fn package_files(dir: &Path) -> Result<Package, String> {
    let mut pkg = Package { files: Vec::new(), skipped: Vec::new() };
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(dir).follow_links(false).sort_by_file_name() {
        let entry = entry.map_err(|e| e.to_string())?;
        let rel = entry.path().strip_prefix(dir).map_err(|e| e.to_string())?.to_string_lossy().to_string();
        if rel.is_empty() || rel == "SKILL.md" {
            continue;
        }
        let ft = entry.file_type();
        if rel.split('/').any(|s| s.starts_with('.')) {
            if ft.is_file() || ft.is_symlink() {
                pkg.skipped.push(format!("{} (hidden)", rel));
            }
            continue;
        }
        if ft.is_symlink() {
            pkg.skipped.push(format!("{} (symbolic link)", rel));
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        if rel.starts_with("scripts/") {
            pkg.skipped.push(format!("{} (script: never imported)", rel));
            continue;
        }
        let len = entry.metadata().map_err(|e| e.to_string())?.len();
        total += len;
        if len > MAX_FILE_BYTES || total > MAX_TOTAL_BYTES || pkg.files.len() >= MAX_FILES {
            return Err(format!("the package is too large (at most {} files, {} KiB each, {} KiB in all)", MAX_FILES, MAX_FILE_BYTES / 1024, MAX_TOTAL_BYTES / 1024));
        }
        pkg.files.push((rel, std::fs::read(entry.path()).map_err(|e| e.to_string())?));
    }
    Ok(pkg)
}

fn write_package(dest: &Path, skill_text: &str, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    if dest.exists() {
        return Err(format!("{} already exists; nothing was overwritten", dest.display()));
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join("SKILL.md"), skill_text).map_err(|e| e.to_string())?;
    for (rel, bytes) in files {
        let p = dest.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Where a skill of this name lives in a workspace, most specific first.
pub(crate) fn find_skill(ws_dir: &Path, name: &str) -> Option<PathBuf> {
    [ws_dir.join("team/skills/custom").join(name), ws_dir.join("core/skills").join(name), ws_dir.join("base/skills").join(name)].into_iter().find(|p| p.join("SKILL.md").is_file())
}

pub(crate) fn dir_name(dir: &Path) -> String {
    std::fs::canonicalize(dir).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_default()
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// `delphi skills check <dir>`.
pub fn check(dir: &Path) -> i32 {
    let text = match std::fs::read_to_string(dir.join("SKILL.md")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi skills: cannot read {}/SKILL.md: {}", dir.display(), e);
            return 2;
        }
    };
    let (fm, body) = match split(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("delphi skills: {}", e);
            return 1;
        }
    };
    let name = dir_name(dir);
    let std = standard_errors(&fm, &name);
    let has_delphi = DELPHI_FIELDS.iter().all(|f| fm.contains_key(key(f)) || fm.get(key("metadata")).and_then(|m| m.as_mapping()).is_some_and(|m| m.contains_key(key(&format!("delphi.{}", f)))));
    println!("{}: {}", name, if std.is_empty() { "valid agentskills.io skill" } else { "NOT a valid agentskills.io skill" });
    for e in &std {
        println!("  - {}", e);
    }
    println!("  body: {} lines{}", body.lines().count(), if body.lines().count() > 500 { " (the standard recommends under 500)" } else { "" });
    println!("  Delphi fields (authorized_agents, schemas, permissions): {}", if has_delphi { "present" } else { "absent: import needs --agents and will default the rest" });
    if let Ok(p) = package_files(dir) {
        for s in &p.skipped {
            println!("  would not import: {}", s);
        }
    }
    if std.is_empty() { 0 } else { 1 }
}

/// `delphi skills export <name> [--out <dir>]`.
pub fn export(ws_dir: &Path, name: &str, out_dir: &Path) -> i32 {
    let Some(src) = find_skill(ws_dir, name) else {
        eprintln!("delphi skills: no skill named `{}` in this workspace (looked in team/skills/custom, core/skills, base/skills)", name);
        return 1;
    };
    let text = match std::fs::read_to_string(src.join("SKILL.md")).map_err(|e| e.to_string()).and_then(|t| export_text(&t)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi skills: {}", e);
            return 1;
        }
    };
    let (fm, _) = split(&text).expect("an export parses");
    let errs = standard_errors(&fm, name);
    if !errs.is_empty() {
        eprintln!("delphi skills: `{}` would not be a valid standard skill: {}", name, errs.join("; "));
        return 1;
    }
    // Everything else in the package goes as it is, scripts included: this is the operator's own skill.
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&src).follow_links(false).sort_by_file_name().into_iter().flatten() {
        let rel = entry.path().strip_prefix(&src).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        if entry.file_type().is_file() && rel != "SKILL.md" {
            match std::fs::read(entry.path()) {
                Ok(b) => files.push((rel, b)),
                Err(e) => {
                    eprintln!("delphi skills: cannot read {}: {}", rel, e);
                    return 1;
                }
            }
        }
    }
    let dest = out_dir.join(name);
    match write_package(&dest, &text, &files) {
        Ok(()) => {
            println!("delphi skills: exported `{}` to {} ({} other file(s)); Delphi's fields are in metadata as delphi.* JSON strings", name, dest.display(), files.len());
            0
        }
        Err(e) => {
            eprintln!("delphi skills: {}", e);
            1
        }
    }
}

/// `delphi skills import <dir> --agents a,b [--description "…"]`.
pub fn import(ws_dir: &Path, src: &Path, opts: &ImportOpts) -> i32 {
    let name = dir_name(src);
    let text = match std::fs::read_to_string(src.join("SKILL.md")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi skills: cannot read {}/SKILL.md: {}", src.display(), e);
            return 2;
        }
    };
    let imported = match import_text(&text, &name, opts) {
        Ok(i) => i,
        Err(errs) => {
            eprintln!("delphi skills: `{}` cannot be imported:", name);
            for e in errs {
                eprintln!("  - {}", e);
            }
            return 1;
        }
    };
    let pkg = match package_files(src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi skills: {}", e);
            return 1;
        }
    };
    if let Some(existing) = find_skill(ws_dir, &name) {
        eprintln!("delphi skills: a skill named `{}` already exists ({}); nothing was imported. Import it under another directory name.", name, existing.display());
        return 1;
    }
    let dest = ws_dir.join("team/skills/custom").join(&name);
    if let Err(e) = write_package(&dest, &imported.text, &pkg.files) {
        eprintln!("delphi skills: {}", e);
        return 1;
    }
    println!("delphi skills: imported `{}` into {} ({} other file(s))", name, dest.display(), pkg.files.len());
    for n in &imported.notes {
        println!("  note: {}", n);
    }
    for s in &pkg.skipped {
        println!("  not imported: {}", s);
    }
    println!("It is not loaded yet. It is a pending proposal (Red): read it, then `/ratify` in the console. Imported text is put in front of an agent as instructions, so read the whole of it.");
    0
}

/// The entry point: `delphi skills <check|export|import> …`.
pub fn cli(ws_dir: &Path, args: &[String]) -> i32 {
    let positional: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !matches!(args[*i - 1].as_str(), "--agents" | "--description" | "--out"))).map(|(_, a)| a).collect();
    match (positional.first().map(|s| s.as_str()), positional.get(1)) {
        (Some("check"), Some(d)) => check(Path::new(d.as_str())),
        (Some("export"), Some(n)) => export(ws_dir, n, Path::new(&flag(args, "--out").unwrap_or_else(|| ".".into()))),
        (Some("import"), Some(d)) => {
            let agents: Vec<String> = flag(args, "--agents").map(|a| a.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
            import(ws_dir, Path::new(d.as_str()), &ImportOpts { agents, description: flag(args, "--description") })
        }
        _ => {
            eprintln!("usage: delphi skills check <dir> | export <name> [--out <dir>] | import <dir> --agents <role,role> [--description \"…\"]");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const STANDARD: &str = "---\nname: pdf-notes\ndescription: Summarise a PDF into notes. Use when the user shares a PDF.\nlicense: MIT\nallowed-tools: Bash(pdftotext:*) Read\nmetadata:\n  author: someone\n---\n# PDF notes\n\n1. Read it.\n2. Write notes.\n";

    fn opts(agents: &[&str]) -> ImportOpts {
        ImportOpts { agents: agents.iter().map(|s| s.to_string()).collect(), description: None }
    }

    fn fm_of(text: &str) -> Mapping {
        split(text).unwrap().0
    }

    #[test]
    fn the_standard_rules_are_checked_name_description_and_metadata() {
        assert!(standard_errors(&fm_of(STANDARD), "pdf-notes").is_empty());
        for (name, ok) in [("pdf-notes", true), ("PDF-notes", false), ("-pdf", false), ("pdf-", false), ("pdf--notes", false), ("pdf_notes", false), ("", false)] {
            let fm = fm_of(&format!("---\nname: {}\ndescription: x\n---\n", if name.is_empty() { "''" } else { name }));
            assert_eq!(standard_errors(&fm, name).is_empty(), ok, "{:?}", name);
        }
        assert!(standard_errors(&fm_of(STANDARD), "other-dir")[0].contains("equal its directory"));
        let long = format!("---\nname: a\ndescription: {}\n---\n", "x".repeat(1025));
        assert!(standard_errors(&fm_of(&long), "a")[0].contains("1-1024"));
        let bad_meta = "---\nname: a\ndescription: x\nmetadata:\n  n: 3\n---\n";
        assert!(standard_errors(&fm_of(bad_meta), "a")[0].contains("strings to strings"));
        assert!(standard_errors(&fm_of("---\nname: a\ndescription: x\ncompatibility: ''\n---\n"), "a")[0].contains("compatibility"));
    }

    #[test]
    fn a_standard_skill_needs_the_operator_to_name_the_roles_and_gets_safe_defaults() {
        let e = import_text(STANDARD, "pdf-notes", &opts(&[])).err().unwrap();
        assert!(e[0].contains("--agents"), "{:?}", e);
        let i = import_text(STANDARD, "pdf-notes", &opts(&["scout", "writer"])).unwrap();
        let fm = fm_of(&i.text);
        assert_eq!(serde_json::to_value(fm.get(key("authorized_agents")).unwrap()).unwrap(), serde_json::json!(["scout", "writer"]));
        assert_eq!(serde_json::to_value(fm.get(key("required_permissions")).unwrap()).unwrap(), serde_json::json!(["read:all"]), "the least permission, not the skill's say-so");
        assert_eq!(serde_json::to_value(fm.get(key("input_schema")).unwrap()).unwrap(), serde_json::json!({"type": "object"}));
        assert!(i.notes.iter().any(|n| n.contains("allowed-tools was ignored")) && i.notes.iter().any(|n| n.contains("required_permissions was not given")));
        assert!(!fm.contains_key(key("allowed-tools")));
        assert_eq!(fm.get(key("license")).and_then(|v| v.as_str()), Some("MIT"));
        let meta = fm.get(key("metadata")).unwrap().as_mapping().unwrap();
        assert_eq!(meta.get(key("author")).and_then(|v| v.as_str()), Some("someone"));
        assert_eq!(meta.get(key("delphi.imported")).and_then(|v| v.as_str()), Some("true"));
        assert!(i.text.contains("# PDF notes"), "the body is carried over unchanged");
        assert!(lint::skill_errors(&fm, "pdf-notes", true).is_empty(), "and it passes the same checks as any custom skill");
    }

    #[test]
    fn a_foreign_skill_cannot_name_its_own_roles_or_bring_scripts_or_unknown_roles() {
        // A skill (or an export) that lists roles in metadata does not decide them: they are a hint in the error only.
        let claims = "---\nname: x-skill\ndescription: d\nmetadata:\n  delphi.authorized_agents: '[\"builder\"]'\n---\nbody\n";
        let e = import_text(claims, "x-skill", &opts(&[])).err().unwrap();
        assert!(e[0].contains("the export listed") && e[0].contains("builder"), "{:?}", e);
        let i = import_text(claims, "x-skill", &opts(&["scout"])).unwrap();
        assert_eq!(serde_json::to_value(fm_of(&i.text).get(key("authorized_agents")).unwrap()).unwrap(), serde_json::json!(["scout"]), "the operator's list wins");
        let scripts = "---\nname: x-skill\ndescription: d\nmetadata:\n  delphi.deterministic_scripts: '[{\"path\": \"scripts/a.sh\", \"timeout_ms\": 1000, \"sandbox_mode\": \"readonly\"}]'\n---\nbody\n";
        let i = import_text(scripts, "x-skill", &opts(&["scout"])).unwrap();
        assert!(!fm_of(&i.text).contains_key(key("deterministic_scripts")) && i.notes.iter().any(|n| n.contains("scripts were dropped")));
        assert!(import_text(STANDARD, "pdf-notes", &opts(&["root"])).err().unwrap().iter().any(|e| e.contains("not an agent role")));
        assert!(import_text(STANDARD, "pdf-notes", &opts(&["operator"])).is_err(), "operator is not an agent role");
    }

    #[test]
    fn a_long_description_must_be_shortened_by_the_operator_not_cut_for_them() {
        let long = format!("---\nname: long-one\ndescription: {}\n---\nbody\n", "word ".repeat(60));
        let e = import_text(&long, "long-one", &opts(&["scout"])).err().unwrap();
        assert!(e.iter().any(|m| m.contains("description must be")) && e.iter().any(|m| m.contains("--description")), "{:?}", e);
        let ok = import_text(&long, "long-one", &ImportOpts { agents: vec!["scout".into()], description: Some("Short and useful.".into()) }).unwrap();
        assert_eq!(get_str(&fm_of(&ok.text), "description"), Some("Short and useful."));
        let big_body = format!("---\nname: big-one\ndescription: d\n---\n{}", "line\n".repeat(600));
        assert!(import_text(&big_body, "big-one", &opts(&["scout"])).err().unwrap().iter().any(|m| m.contains("lines (cap")));
        assert!(import_text("no frontmatter", "x", &opts(&["scout"])).err().unwrap()[0].contains("no frontmatter"));
        assert!(import_text("---\nname: x\n", "x", &opts(&["scout"])).is_err(), "unclosed frontmatter");
    }

    #[test]
    fn every_built_in_skill_exports_as_a_valid_standard_skill_and_imports_back_unchanged() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let mut n = 0;
        for e in fs::read_dir(&root).unwrap().flatten().filter(|e| e.path().join("SKILL.md").is_file()) {
            let name = e.file_name().to_string_lossy().to_string();
            let original = fs::read_to_string(e.path().join("SKILL.md")).unwrap();
            let exported = export_text(&original).unwrap_or_else(|er| panic!("{}: {}", name, er));
            let (efm, ebody) = split(&exported).unwrap();
            assert!(standard_errors(&efm, &name).is_empty(), "{}: {:?}", name, standard_errors(&efm, &name));
            assert!(!efm.contains_key(key("authorized_agents")), "{}: the extension fields are in metadata, not top level", name);
            let (ofm, obody) = split(&original).unwrap();
            assert_eq!(ebody, obody, "{}: the body is untouched", name);
            // The round trip: the operator re-states the roles (here, the same ones) and everything else comes back.
            let roles: Vec<String> = ofm.get(key("authorized_agents")).unwrap().as_sequence().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
            let back = import_text(&exported, &name, &ImportOpts { agents: roles, description: None }).unwrap_or_else(|er| panic!("{}: {:?}", name, er));
            let bfm = fm_of(&back.text);
            for f in DELPHI_FIELDS {
                assert_eq!(serde_json::to_value(bfm.get(key(f)).unwrap()).unwrap(), serde_json::to_value(ofm.get(key(f)).unwrap()).unwrap(), "{}: {} survives the round trip", name, f);
            }
            assert_eq!(get_str(&bfm, "description"), get_str(&ofm, "description"));
            n += 1;
        }
        assert!(n >= 40, "{} skills checked", n);
    }

    #[test]
    fn a_package_brings_documents_but_never_scripts_links_or_hidden_files() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("pdf-notes");
        fs::create_dir_all(d.join("scripts")).unwrap();
        fs::create_dir_all(d.join("references")).unwrap();
        fs::create_dir_all(d.join(".git")).unwrap();
        fs::write(d.join("SKILL.md"), STANDARD).unwrap();
        fs::write(d.join("scripts/run.sh"), "rm -rf ~").unwrap();
        fs::write(d.join("references/guide.md"), "guide").unwrap();
        fs::write(d.join(".env"), "SECRET=1").unwrap();
        fs::write(d.join(".git/config"), "x").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", d.join("passwd")).unwrap();
        let p = package_files(&d).unwrap();
        assert_eq!(p.files.iter().map(|(r, _)| r.as_str()).collect::<Vec<_>>(), vec!["references/guide.md"]);
        for want in ["scripts/run.sh (script: never imported)", ".env (hidden)", "passwd (symbolic link)"] {
            assert!(p.skipped.iter().any(|s| s == want), "{:?} missing {}", p.skipped, want);
        }
        fs::write(d.join("references/huge.bin"), vec![0u8; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        assert!(package_files(&d).unwrap_err().contains("too large"));
    }

    fn workspace() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("ws/base/skills/repo-cartography")).unwrap();
        fs::write(t.path().join("ws/base/skills/repo-cartography/SKILL.md"), fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/repo-cartography/SKILL.md")).unwrap()).unwrap();
        t
    }

    #[test]
    fn import_writes_under_the_protected_custom_directory_and_never_overwrites() {
        let t = workspace();
        let ws = t.path().join("ws");
        let src = t.path().join("src/pdf-notes");
        fs::create_dir_all(src.join("references")).unwrap();
        fs::write(src.join("SKILL.md"), STANDARD).unwrap();
        fs::write(src.join("references/guide.md"), "guide").unwrap();
        assert_eq!(import(&ws, &src, &opts(&["scout"])), 0);
        let dest = ws.join("team/skills/custom/pdf-notes");
        assert!(dest.join("SKILL.md").is_file() && dest.join("references/guide.md").is_file());
        assert!(crate::home::is_protected("skills/custom/pdf-notes/SKILL.md"), "so it is a pending Red proposal, not a loaded skill");
        let before = fs::read_to_string(dest.join("SKILL.md")).unwrap();
        assert_eq!(import(&ws, &src, &opts(&["writer"])), 1, "a second import is refused");
        assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), before);
        assert_eq!(import(&ws, &src, &opts(&[])), 1, "and one that names no roles never starts");
        // A skill that shares a name with a built-in is refused too.
        let shadow = t.path().join("src/repo-cartography");
        fs::create_dir_all(&shadow).unwrap();
        fs::write(shadow.join("SKILL.md"), "---\nname: repo-cartography\ndescription: mine\n---\nbody\n").unwrap();
        assert_eq!(import(&ws, &shadow, &opts(&["scout"])), 1);
        assert!(!ws.join("team/skills/custom/repo-cartography").exists());
    }

    #[test]
    fn export_then_import_through_a_workspace_restores_a_built_in_skill() {
        let t = workspace();
        let ws = t.path().join("ws");
        assert_eq!(export(&ws, "repo-cartography", &t.path().join("out")), 0);
        let exported = t.path().join("out/repo-cartography");
        assert!(exported.join("SKILL.md").is_file());
        assert_eq!(check(&exported), 0, "the export is a valid standard skill");
        assert_eq!(export(&ws, "repo-cartography", &t.path().join("out")), 1, "an existing export is not overwritten");
        assert_eq!(export(&ws, "no-such-skill", &t.path().join("out")), 1);
        // Import it into a clean workspace under another home.
        let ws2 = t.path().join("ws2");
        fs::create_dir_all(&ws2).unwrap();
        assert_eq!(import(&ws2, &exported, &opts(&["scout", "architect"])), 0);
        let (fm, _) = split(&fs::read_to_string(ws2.join("team/skills/custom/repo-cartography/SKILL.md")).unwrap()).unwrap();
        assert!(lint::skill_errors(&fm, "repo-cartography", true).is_empty());
    }

    #[test]
    fn the_command_line_picks_its_arguments_apart() {
        let t = workspace();
        let ws = t.path().join("ws");
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cli(&ws, &a(&[])), 2);
        assert_eq!(cli(&ws, &a(&["import"])), 2);
        assert_eq!(cli(&ws, &a(&["export", "repo-cartography", "--out", t.path().join("o").to_str().unwrap()])), 0);
        assert!(t.path().join("o/repo-cartography/SKILL.md").is_file());
    }
}
