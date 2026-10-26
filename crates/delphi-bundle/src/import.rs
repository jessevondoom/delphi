//! `delphi bundle import <plugin dir>` (`specs/025` §7.1): a plain Claude Code plugin becomes a bundle directory.
//!
//! The source is hostile. Nothing is run and nothing is copied that is not named below: `skills/`, `agents/` (top-level
//! files), the three documents (`README.md`, `LICENSE`, `CHANGELOG.md`), and the declared MCP servers rewritten as inert
//! entries without a single value. Everything else a plugin brings is reported as ignored, with the reason, and left
//! behind. `hooks/` and `bin/` are never imported (a `bin/` refuses the import). The result is validated by the same
//! [`crate::validate`] as an added bundle, and is a directory: import never adds.

use crate::claude_plugin::{self, PluginMeta};
use crate::mcp_decl;
use crate::tree::{bad_char, MAX_FILES, MAX_FILE_BYTES, MAX_TREE_BYTES};
use crate::{Host, Options, Report, ROLES};
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Default, Clone)]
pub struct ImportOpts {
    /// `--as`: the bundle name (default: `plugin.json` `name`, else the directory's name).
    pub name: Option<String>,
    /// `--org`: the org id (default: the author's name made into an id, else `imported`).
    pub org: Option<String>,
    /// `--agents`: the roles every imported skill is authorized for. The operator's decision; a foreign skill's own list is dropped.
    pub skill_roles: Vec<String>,
    /// `--agent-role <agent>=<role>`: the host role of an agent file.
    pub agent_roles: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Imported,
    Ignored,
    Skipped,
}

impl Outcome {
    pub fn word(self) -> &'static str {
        match self {
            Outcome::Imported => "imported",
            Outcome::Ignored => "ignored",
            Outcome::Skipped => "skipped",
        }
    }
}

/// One line of the component table: what the plugin brought, what became of it, and why.
#[derive(Debug, Clone)]
pub struct Row {
    pub component: String,
    pub outcome: Outcome,
    pub why: String,
}

#[derive(Debug)]
pub struct Imported {
    pub rows: Vec<Row>,
    pub name: String,
    pub org: String,
    pub report: Report,
}

impl Imported {
    pub fn render_table(&self) -> String {
        let w = self.rows.iter().map(|r| r.component.chars().count()).max().unwrap_or(0);
        self.rows.iter().map(|r| format!("  {:<w$}  {:<8}  {}\n", r.component, r.outcome.word(), r.why, w = w)).collect()
    }
}

fn row(rows: &mut Vec<Row>, component: &str, outcome: Outcome, why: impl Into<String>) {
    rows.push(Row { component: component.into(), outcome, why: why.into() });
}

/// `Some(id)` when `s` can be an id of at most `max` characters: lowercase letters, digits and hyphens.
fn id_from(s: &str, max: usize) -> Option<String> {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if c == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(c);
    }
    let out: String = out.chars().take(max).collect::<String>().trim_end_matches('-').to_string();
    (!out.is_empty()).then_some(out)
}

/// Text as a bundle wants it: LF line endings and NFC. `None` for bytes that are not UTF-8.
fn normalized(bytes: &[u8]) -> Option<String> {
    let t = std::str::from_utf8(bytes).ok()?;
    let t = t.replace("\r\n", "\n").replace('\r', "\n");
    Some(t.nfc().collect())
}

const IGNORED_TOP: [(&str, &str); 12] = [
    ("commands", "a command is a slash prompt; Delphi's console commands are code"),
    ("hooks", "hooks run arbitrary commands; never imported and never translated into Delphi hooks"),
    ("output-styles", "Delphi themes are ratified packs, not plugin files"),
    ("themes", "Delphi themes are ratified packs, not plugin files"),
    ("workflows", "workflows execute; a bundle never runs anything"),
    ("monitors", "monitors execute; a bundle never runs anything"),
    (".lsp.json", "LSP servers are programs that run"),
    ("settings.json", "session defaults for Claude Code; a bundle sets none"),
    ("CLAUDE.md", "Claude Code ignores a plugin's root CLAUDE.md too"),
    ("AGENTS.md", "a project's AGENTS.md is read in place as project knowledge, not packaged"),
    (".git", "a bundle is a tree, not a repository"),
    ("marketplace.json", "a marketplace entry is a locator; export writes one"),
];

struct Copier<'a> {
    out: &'a Path,
    files: usize,
    bytes: u64,
    skipped: BTreeMap<String, Vec<String>>,
    /// Directories (relative paths) a round trip drops: skills a previous export generated.
    drop: Vec<String>,
    name_ok: Regex,
}

impl Copier<'_> {
    fn skip(&mut self, reason: &str, path: &str) {
        self.skipped.entry(reason.into()).or_default().push(path.into());
    }

    fn put(&mut self, rel: &str, bytes: &[u8]) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        self.files += 1;
        self.bytes += bytes.len() as u64;
        if self.files > MAX_FILES || self.bytes > MAX_TREE_BYTES {
            return Err(format!("the plugin is larger than a bundle may be ({} files, {} MiB; caps {} and {} MiB)", self.files, self.bytes / (1024 * 1024), MAX_FILES, MAX_TREE_BYTES / (1024 * 1024)));
        }
        let p = self.out.join(rel);
        fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("{}: {}", rel, e))?;
        fs::write(&p, bytes).map_err(|e| format!("{}: {}", rel, e))?;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).map_err(|e| format!("{}: {}", rel, e))
    }

    /// Copy `src` (a directory of the plugin) to `rel` in the output: regular files with safe names only.
    /// `binary_ok` says where a .png or .pdf may live. A skill's SKILL.md goes through `skill`.
    fn tree(&mut self, src: &Path, rel: &str, binary_ok: &dyn Fn(&str) -> bool, skill: &mut dyn FnMut(&str, String) -> String) -> Result<(), String> {
        let Ok(rd) = fs::read_dir(src) else {
            self.skip("unreadable directory", rel);
            return Ok(());
        };
        let mut names: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for name in names {
            let here = format!("{}/{}", rel, name);
            let path = src.join(&name);
            if name.starts_with('.') {
                self.skip("hidden entries", &here);
                continue;
            }
            if !self.name_ok.is_match(&name) {
                self.skip("names outside ^[A-Za-z0-9._-]{1,64}$", &here);
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(&path) else { continue };
            let ft = meta.file_type();
            if ft.is_symlink() {
                self.skip("symbolic links", &here);
            } else if ft.is_dir() && self.drop.contains(&here) {
                self.skip("generated knowledge-index skills (a round trip drops them rather than indexing twice)", &here);
            } else if ft.is_dir() {
                self.tree(&path, &here, binary_ok, skill)?;
            } else if ft.is_file() {
                if hard_linked(&meta) {
                    self.skip("hard links", &here);
                } else if meta.len() > MAX_FILE_BYTES {
                    self.skip("files over 1 MiB", &here);
                } else {
                    let bytes = fs::read(&path).map_err(|e| format!("{}: {}", here, e))?;
                    let ext = name.rsplit('.').next().unwrap_or("");
                    if matches!(ext, "png" | "pdf") {
                        if binary_ok(&here) {
                            self.put(&here, &bytes)?;
                        } else {
                            self.skip(".png/.pdf outside skills/*/templates and assets", &here);
                        }
                    } else {
                        match normalized(&bytes) {
                            Some(t) if t.chars().any(|c| bad_char(c).is_some()) => {
                                self.skip("text with control, bidi or tag characters", &here);
                            }
                            Some(t) => {
                                let t = if name == "SKILL.md" && here.matches('/').count() == 2 { skill(&here, t) } else { t };
                                self.put(&here, t.as_bytes())?
                            }
                            None => self.skip("files that are not UTF-8 text", &here),
                        }
                    }
                }
            } else {
                self.skip("devices, FIFOs and sockets", &here);
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn hard_linked(m: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    m.nlink() > 1
}

#[cfg(not(unix))]
fn hard_linked(_: &fs::Metadata) -> bool {
    false
}

fn binary_ok(path: &str) -> bool {
    path.starts_with("skills/") && (path.contains("/templates/") || path.contains("/assets/"))
}

/// The skill with the operator's roles in `metadata`, and a foreign `delphi.authorized_agents` dropped: a skill never
/// decides which roles use it (`skills import` says the same). A file whose frontmatter does not parse is kept as it
/// is, and step 9 refuses it.
fn with_roles(text: &str, roles: &[String]) -> String {
    use serde_yaml::{Mapping, Value};
    let Some(rest) = text.strip_prefix("---\n") else { return text.to_string() };
    let Some(end) = rest.find("\n---\n") else { return text.to_string() };
    let Ok(Value::Mapping(mut fm)) = serde_yaml::from_str::<Value>(&rest[..end]) else { return text.to_string() };
    let body = &rest[end + 5..];
    let key = |k: &str| Value::String(k.into());
    let mut meta: Mapping = fm.get(key("metadata")).and_then(|m| m.as_mapping()).cloned().unwrap_or_default();
    meta.remove(key("delphi.authorized_agents"));
    fm.remove(key("authorized_agents"));
    if !roles.is_empty() {
        meta.insert(key("delphi.authorized_agents"), Value::String(serde_json::to_string(roles).unwrap()));
    }
    if meta.is_empty() {
        fm.remove(key("metadata"));
    } else {
        fm.insert(key("metadata"), Value::Mapping(meta));
    }
    match serde_yaml::to_string(&Value::Mapping(fm)) {
        Ok(y) => format!("---\n{}---\n{}", y, body),
        Err(_) => text.to_string(),
    }
}

/// Write the component table's skills and agents, the manifest and the README into `out` and validate. `out` must not
/// exist or must be empty. Refusals (a `bin/` directory, a sensitive option, a credential-shaped MCP value, a plugin that
/// is already a bundle) are returned before anything is written.
pub fn import(src: &Path, out: &Path, o: &ImportOpts, host: &dyn Host) -> Result<Imported, Vec<String>> {
    let src_meta = fs::symlink_metadata(src).map_err(|e| vec![format!("{}: {}", src.display(), e)])?;
    if !src_meta.is_dir() {
        return Err(vec![format!("{} is not a directory", src.display())]);
    }
    if out.exists() && fs::read_dir(out).map(|mut r| r.next().is_some()).unwrap_or(true) {
        return Err(vec![format!("{} exists and is not empty; import writes a new directory", out.display())]);
    }
    for r in &o.skill_roles {
        if !ROLES.contains(&r.as_str()) {
            return Err(vec![format!("--agents: `{}` is not a canonical role id ({})", r, ROLES.join(", "))]);
        }
    }
    let top = |n: &str| fs::symlink_metadata(src.join(n)).ok();
    if top("bundle.toml").is_some() {
        return Err(vec!["this directory is already a bundle (it has bundle.toml): validate it and `delphi bundle add` it".into()]);
    }
    if top("bin").is_some() {
        return Err(vec!["refused: a `bin/` directory (Claude Code puts bin/ on the shell PATH; Delphi never adds executables, specs/023 §10 step 1)".into()]);
    }

    // 1. plugin.json and .mcp.json, read before anything is written.
    let mut notes: Vec<String> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    let plugin_path = src.join(".claude-plugin/plugin.json");
    let read_small = |p: &Path| -> Option<String> {
        let m = fs::symlink_metadata(p).ok()?;
        (m.is_file() && m.len() <= MAX_FILE_BYTES).then(|| fs::read_to_string(p).ok()).flatten()
    };
    let meta: PluginMeta = match read_small(&plugin_path) {
        Some(t) => claude_plugin::parse(&t).map_err(|e| vec![format!(".claude-plugin/plugin.json: {}", e)])?,
        None => PluginMeta::default(),
    };
    if !meta.sensitive.is_empty() {
        return Err(meta.sensitive.iter().map(|k| format!("refused: plugin.json `{}` is a sensitive option; a bundle must not ask for credentials (specs/025 §2.3)", k)).collect());
    }
    // The servers: a scratch tree of the two files is enough for the reader, which never touches the filesystem outside it.
    let scratch = tempdir_like(out)?;
    for (rel, p) in [(".mcp.json", src.join(".mcp.json")), (".claude-plugin/plugin.json", plugin_path.clone())] {
        if let Some(t) = read_small(&p) {
            let d = scratch.join(rel);
            fs::create_dir_all(d.parent().unwrap()).ok();
            fs::write(d, t).ok();
        }
    }
    let servers = mcp_decl::read(&crate::Tree::open(&scratch)).map_err(|e| vec![e]);
    let _ = fs::remove_dir_all(&scratch);
    let servers = servers?;
    let bad: Vec<String> = servers.iter().flat_map(|s| s.problems.iter().map(move |p| format!("refused: MCP server `{}`: {}", s.name, p))).collect();
    if !bad.is_empty() {
        return Err(bad);
    }

    // 2. identity.
    let name = match &o.name {
        Some(n) => {
            if !Regex::new(r"^[a-z0-9][a-z0-9-]{0,47}$").unwrap().is_match(n) {
                return Err(vec![format!("--as `{}` must match ^[a-z0-9][a-z0-9-]{{0,47}}$", n)]);
            }
            n.clone()
        }
        None => meta
            .name
            .as_deref()
            .and_then(|n| id_from(n, 48))
            .or_else(|| src.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).and_then(|n| id_from(&n, 48)))
            .ok_or_else(|| vec!["cannot name the bundle: pass --as <name>".to_string()])?,
    };
    let org = match &o.org {
        Some(g) => {
            if !Regex::new(r"^[a-z0-9-]{1,32}$").unwrap().is_match(g) {
                return Err(vec![format!("--org `{}` must match ^[a-z0-9-]{{1,32}}$", g)]);
            }
            g.clone()
        }
        None => meta.author_name.as_deref().and_then(|a| id_from(a, 32)).unwrap_or_else(|| "imported".into()),
    };

    // 3. copy.
    fs::create_dir_all(out).map_err(|e| vec![format!("{}: {}", out.display(), e)])?;
    let mut cp = Copier { out, files: 0, bytes: 0, skipped: BTreeMap::new(), drop: Vec::new(), name_ok: Regex::new(r"^[A-Za-z0-9._-]{1,64}$").unwrap() };
    let result = copy_all(src, &mut cp, &mut rows, &mut notes, o, &servers, &meta);
    let (skills, agents) = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(out);
            return Err(vec![e]);
        }
    };
    for (reason, paths) in &cp.skipped {
        let shown: Vec<&str> = paths.iter().take(4).map(|s| s.as_str()).collect();
        row(&mut rows, &format!("{} file(s)", paths.len()), Outcome::Skipped, format!("{}: {}{}", reason, shown.join(", "), if paths.len() > 4 { ", …" } else { "" }));
    }
    for (c, (outcome, why)) in top_rows(src) {
        row(&mut rows, &c, outcome, why);
    }

    // 4. bundle.toml and a README, then the same manifest and validation an added bundle gets.
    let toml = synth_toml(&name, &org, &meta, &skills, &agents, o, &servers, &mut notes);
    let write = |rel: &str, text: &str| fs::write(out.join(rel), text).map_err(|e| vec![format!("{}: {}", rel, e)]);
    write("bundle.toml", &toml)?;
    if !out.join("README.md").exists() {
        let mut readme = format!("# {}\n\nImported from a Claude Code plugin by `delphi bundle import`. Adding it brings:\n\n", name);
        readme.push_str(&format!("- {} skill(s): {}\n", skills.len(), if skills.is_empty() { "none".into() } else { skills.join(", ") }));
        readme.push_str(&format!("- {} agent(s): {}\n", agents.len(), if agents.is_empty() { "none".into() } else { agents.join(", ") }));
        readme.push_str(&format!("- {} MCP server declaration(s), inert until the operator maps them\n", servers.len()));
        write("README.md", &readme)?;
    }
    if let Err(e) = crate::write_manifest(out, true, host) {
        return Err(vec![format!("imported, but the bundle is not well-formed: {}", e)]);
    }
    let report = crate::validate(out, &Options { claude: true, ..Options::default() }, host);
    for n in notes {
        row(&mut rows, "note", Outcome::Imported, n);
    }
    Ok(Imported { rows, name, org, report })
}

fn tempdir_like(out: &Path) -> Result<PathBuf, Vec<String>> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let base = out.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let d = base.join(format!(".delphi-import-{}-{}", std::process::id(), nanos));
    fs::create_dir_all(&d).map_err(|e| vec![format!("{}: {}", d.display(), e)])?;
    Ok(d)
}

fn top_rows(src: &Path) -> Vec<(String, (Outcome, String))> {
    let mut out = Vec::new();
    for (n, why) in IGNORED_TOP {
        if fs::symlink_metadata(src.join(n)).is_ok() {
            out.push((format!("{}{}", n, if src.join(n).is_dir() { "/" } else { "" }), (Outcome::Ignored, why.to_string())));
        }
    }
    if fs::symlink_metadata(src.join(".claude-plugin/marketplace.json")).is_ok() {
        out.push((".claude-plugin/marketplace.json".into(), (Outcome::Ignored, "a marketplace entry is a locator; export writes one".into())));
    }
    out
}

type Names = Vec<String>;

fn copy_all(src: &Path, cp: &mut Copier, rows: &mut Vec<Row>, notes: &mut Vec<String>, o: &ImportOpts, servers: &[mcp_decl::McpServer], meta: &PluginMeta) -> Result<(Names, Names), String> {
    let (mut skills, mut agents): (Names, Names) = (Vec::new(), Vec::new());
    // documents
    for doc in ["README.md", "LICENSE", "CHANGELOG.md"] {
        let p = src.join(doc);
        if let Ok(m) = fs::symlink_metadata(&p) {
            if m.is_file() && m.len() <= MAX_FILE_BYTES && !hard_linked(&m) {
                if let Some(t) = fs::read(&p).ok().and_then(|b| normalized(&b)).filter(|t| !t.chars().any(|c| bad_char(c).is_some())) {
                    cp.put(doc, t.as_bytes())?;
                    row(rows, doc, Outcome::Imported, "copied");
                    continue;
                }
            }
            cp.skip("unreadable or non-text documents", doc);
        }
    }
    // skills
    let skills_dir = src.join("skills");
    if let Ok(m) = fs::symlink_metadata(&skills_dir) {
        if m.is_dir() {
            for d in fs::read_dir(&skills_dir).map(|r| r.filter_map(|e| e.ok()).collect::<Vec<_>>()).unwrap_or_default() {
                let md = d.path().join("SKILL.md");
                let small = fs::symlink_metadata(&md).map(|m| m.is_file() && m.len() <= MAX_FILE_BYTES).unwrap_or(false);
                if small && fs::read_to_string(&md).is_ok_and(|t| crate::export::is_generated_index(&t)) {
                    cp.drop.push(format!("skills/{}", d.file_name().to_string_lossy()));
                }
            }
            let mut stripped = 0usize;
            let mut cuts = Vec::new();
            let mut transform = |path: &str, text: String| -> String {
                let had = text.contains("delphi.authorized_agents") || text.contains("\nauthorized_agents:");
                if had {
                    stripped += 1;
                }
                if text.contains("\nallowed-tools:") {
                    cuts.push(path.to_string());
                }
                with_roles(&text, &o.skill_roles)
            };
            cp.tree(&skills_dir, "skills", &binary_ok, &mut transform)?;
            if stripped > 0 {
                notes.push(format!("{} skill(s) named roles of their own; dropped (the operator names them with --agents)", stripped));
            }
            for n in cuts {
                notes.push(format!("{}: `allowed-tools` is ignored (Delphi has no pre-approval a file can grant)", n));
            }
            let t = crate::Tree::open(cp.out);
            skills = crate::skills::skill_names(&t);
            let refs = t.entries.iter().filter(|e| e.path.starts_with("skills/") && e.path.contains("/references/")).count();
            let assets = t.entries.iter().filter(|e| e.path.starts_with("skills/") && e.path.contains("/assets/")).count();
            let scripts = t.entries.iter().filter(|e| e.path.starts_with("skills/") && e.path.contains("/scripts/")).count();
            row(rows, "skills/", Outcome::Imported, format!("{} skill(s){}", skills.len(), if o.skill_roles.is_empty() { "; no roles named, pass --agents <role,role> or the skills load for no one" } else { "" }));
            if refs + assets + scripts > 0 {
                row(rows, "skills/*/{references,assets,scripts}", Outcome::Imported, format!("{} references, {} assets (a synonym of templates/), {} scripts (copied without the execute bit, never run)", refs, assets, scripts));
            }
        } else {
            row(rows, "skills", Outcome::Skipped, "not a directory");
        }
    }
    // agents: top-level files only
    let agents_dir = src.join("agents");
    if let Ok(m) = fs::symlink_metadata(&agents_dir) {
        if m.is_dir() {
            let mut names: Vec<String> = fs::read_dir(&agents_dir).map(|r| r.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
            names.sort();
            for n in names {
                let path = agents_dir.join(&n);
                let Ok(am) = fs::symlink_metadata(&path) else { continue };
                let rel = format!("agents/{}", n);
                if am.is_dir() {
                    cp.skip("agents in subfolders (a bundle agent is agents/<name>.md)", &rel);
                } else if !am.is_file() || hard_linked(&am) {
                    cp.skip("links and special files", &rel);
                } else if !n.ends_with(".md") || !cp.name_ok.is_match(&n) || n.starts_with('.') {
                    cp.skip("agent files that are not agents/<name>.md", &rel);
                } else if am.len() > MAX_FILE_BYTES {
                    cp.skip("files over 1 MiB", &rel);
                } else if let Some(t) = fs::read(&path).ok().and_then(|b| normalized(&b)).filter(|t| !t.chars().any(|c| bad_char(c).is_some())) {
                    cp.put(&rel, t.as_bytes())?;
                    agents.push(n.trim_end_matches(".md").to_string());
                } else {
                    cp.skip("text with control characters or not UTF-8", &rel);
                }
            }
            for (a, role) in &o.agent_roles {
                if !agents.contains(a) {
                    return Err(format!("--agent-role: the plugin has no agent `{}`", a));
                }
                if !ROLES.contains(&role.as_str()) {
                    return Err(format!("--agent-role: `{}` is not a canonical role id ({})", role, ROLES.join(", ")));
                }
            }
            for a in &agents {
                match o.agent_roles.get(a) {
                    Some(r) => row(rows, &format!("agents/{}.md", a), Outcome::Imported, format!("runs as the {} role, with its tools intersected with that role's", r)),
                    None => row(rows, &format!("agents/{}.md", a), Outcome::Imported, "no host role: \"Claude Code only\", not dispatchable in Delphi; --agent-role <agent>=<role> sets one"),
                }
            }
        }
    } else if !o.agent_roles.is_empty() {
        return Err("--agent-role: the plugin has no agents/ directory".into());
    }
    // MCP
    if !servers.is_empty() {
        cp.put(".mcp.json", mcp_decl::sanitized_json(servers).as_bytes())?;
        for s in servers {
            row(rows, &format!("mcp:{}", s.name), Outcome::Imported, format!("{}; values dropped, nothing connected", s.describe()));
        }
    }
    // plugin.json keys
    for k in &meta.ignored {
        let why = match k.as_str() {
            "mcpServers" => continue,
            "userConfig" => "options are ignored (none is sensitive); a bundle has no per-install settings",
            "channels" => "ignored; none of its options is sensitive",
            _ => "ignored: it runs, grants or configures something, and stays with Claude Code",
        };
        row(rows, &format!("plugin.json {}", k), Outcome::Ignored, why);
    }
    Ok((skills, agents))
}

#[allow(clippy::too_many_arguments)]
fn synth_toml(name: &str, org: &str, meta: &PluginMeta, skills: &[String], agents: &[String], o: &ImportOpts, servers: &[mcp_decl::McpServer], notes: &mut Vec<String>) -> String {
    use toml::{Table, Value};
    let mut b = Table::new();
    b.insert("schema".into(), Value::Integer(1));
    b.insert("name".into(), name.into());
    b.insert("org".into(), org.into());
    let version = match meta.version.as_deref() {
        Some(v) if semver::Version::parse(v).is_ok() => v.to_string(),
        Some(v) => {
            notes.push(format!("plugin version `{}` is not semver; the bundle starts at 0.1.0", v.chars().take(40).collect::<String>()));
            "0.1.0".into()
        }
        None => {
            notes.push("the plugin has no version; the bundle starts at 0.1.0".into());
            "0.1.0".into()
        }
    };
    b.insert("version".into(), version.into());
    b.insert("title".into(), name.into());
    let one_line = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").replace("](", "] (");
    let desc = match meta.description.as_deref().map(one_line).filter(|d| !d.is_empty()) {
        Some(d) if d.chars().count() > 200 => {
            notes.push("the plugin's description is over 200 characters and was cut for bundle.toml; edit it".into());
            d.chars().take(200).collect()
        }
        Some(d) => d,
        None => format!("Imported Claude Code plugin {}", name),
    };
    b.insert("description".into(), desc.into());
    b.insert("license".into(), meta.license.as_deref().map(one_line).filter(|l| !l.is_empty() && l.chars().count() <= 80).unwrap_or_else(|| "UNLICENSED".into()).into());
    if let Some(h) = meta.homepage.as_deref().filter(|h| h.starts_with("https://") && !h.contains(char::is_whitespace)) {
        b.insert("homepage".into(), h.into());
    }
    b.insert("audience".into(), "team".into());
    b.insert("review_by".into(), crate::manifest::in_days(180).into());
    let mut root = Table::new();
    root.insert("bundle".into(), Value::Table(b));
    let mut p = Table::new();
    p.insert("name".into(), meta.author_name.as_deref().map(one_line).filter(|a| !a.is_empty()).map(|a| a.chars().take(80).collect::<String>()).unwrap_or_else(|| org.to_string()).into());
    if let Some(e) = meta.author_email.as_deref().filter(|e| Regex::new(r"^[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+$").unwrap().is_match(e)) {
        p.insert("contact".into(), e.into());
    }
    root.insert("publisher".into(), Value::Table(p));
    let tech: Vec<Value> = meta.keywords.iter().map(|k| one_line(k)).filter(|k| !k.is_empty() && k.chars().count() <= 40).take(10).map(Value::from).collect();
    if !tech.is_empty() {
        let mut a = Table::new();
        a.insert("tech".into(), Value::Array(tech));
        root.insert("applies_to".into(), Value::Table(a));
    }
    let mut s = Table::new();
    if !skills.is_empty() {
        s.insert("skills".into(), true.into());
    }
    if !agents.is_empty() {
        s.insert("agents".into(), true.into());
    }
    if !s.is_empty() {
        root.insert("sections".into(), Value::Table(s));
    }
    let mut ag = Table::new();
    for a in agents {
        if let Some(r) = o.agent_roles.get(a) {
            let mut t = Table::new();
            t.insert("role".into(), r.as_str().into());
            ag.insert(a.clone(), Value::Table(t));
        }
    }
    if !ag.is_empty() {
        root.insert("agents".into(), Value::Table(ag));
    }
    let mut roles: Vec<String> = o.skill_roles.iter().cloned().chain(o.agent_roles.iter().filter(|(a, _)| agents.contains(a)).map(|(_, r)| r.clone())).collect();
    roles.sort();
    roles.dedup();
    let mut t = Table::new();
    t.insert("roles".into(), Value::Array(roles.into_iter().map(Value::from).collect()));
    t.insert("connectors".into(), Value::Array(servers.iter().map(|s| Value::from(s.name.clone())).collect()));
    t.insert("network".into(), false.into());
    root.insert("touches".into(), Value::Table(t));
    toml::to_string(&root).unwrap_or_default()
}
