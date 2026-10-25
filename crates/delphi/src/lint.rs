//! `delphi lint`: deterministic file-level checks (`specs/001` §7 test 1, §6.4; `specs/006` §5 checks 3–5).
//!
//! Every check works on a [`Tree`]: logical path → physical file. In `--repo` mode the logical path is
//! the repo-relative path. In workspace mode `doctor` builds an overlay of `base/`, `core/`, `team/`
//! and `local/` (`specs/006` §3.1).

use crate::frontmatter::{self, get_str, ROLES};
pub use delphi_engine::checks::{cap_errors, envelope_errors};
use crate::report::{Level, Report};
use crate::theme;
use regex::Regex;
use serde_yaml::{Mapping, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Repo,
    Workspace,
}

pub struct Tree {
    pub mode: Mode,
    pub files: BTreeMap<String, PathBuf>,
    /// Workspace mode: logical paths still byte-identical to the release seed.
    pub seeded: BTreeSet<String>,
    /// Workspace mode: the built-in (`base/`) files, before `core/` overrides are layered on.
    /// Narrow-only checks always compare against these (`specs/001` §2.3).
    pub builtin: BTreeMap<String, PathBuf>,
}

/// Directories never walked in a source repo.
const SKIP_DIRS: [&str; 5] = [".git", "target", "node_modules", ".delphi", ".claude"];
/// Markdown under these logical prefixes (or at these exact paths) must carry §5 provenance
/// frontmatter. Manifests, skills and theme packs have their own schemas (§2.1, §3.1, §6.2).
const GOVERNED: [&str; 8] = ["context/", "missions/", "specs/", "memory/agents/", "operator/memory/", "operator/profile.md", "operator/heuristics.md", "heuristics.md"];
/// Design records that are never seeded into a workspace; links *from seeded release files* to them
/// are `skipped` there. Links from the workspace's own files must resolve.
const UNSEEDED: [&str; 5] = ["specs/", "missions/", "OVERVIEW.md", "README.md", "crates/"];
/// Text files scanned for absolute home paths (REV-SEC-05).
const TEXT_EXTS: [&str; 11] = [".md", ".toml", ".rs", ".sh", ".py", ".json", ".yaml", ".yml", ".txt", ".js", ".ts"];
const COGNITIVE: [&str; 4] = ["deep-reasoning", "code-craft", "high-context-discovery", "fast-triage"];
pub const BODY_LINE_CAP: usize = 500;

impl Tree {
    pub fn from_repo(root: &Path) -> std::io::Result<Tree> {
        let mut files = BTreeMap::new();
        // `.git` is skipped at any depth; build and tooling directories only at the repo root, so a
        // governed directory such as `context/target/` is still linted.
        let walker = walkdir::WalkDir::new(root).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !(e.file_type().is_dir() && (name == ".git" || name == "node_modules" || (e.depth() == 1 && SKIP_DIRS.contains(&name.as_ref()))))
        });
        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_dir() {
                let rel = entry.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                files.insert(rel, entry.path().to_path_buf());
            }
        }
        Ok(Tree { mode: Mode::Repo, files, seeded: BTreeSet::new(), builtin: BTreeMap::new() })
    }

    fn read(&self, logical: &str) -> Option<String> {
        self.files.get(logical).and_then(|p| fs::read_to_string(p).ok())
    }

    fn exists(&self, logical: &str) -> bool {
        let dir = format!("{}/", logical.trim_end_matches('/'));
        self.files.contains_key(logical) || self.files.keys().any(|k| k.starts_with(&dir))
    }

    fn with_ext<'a>(&'a self, exts: &'a [&'a str]) -> impl Iterator<Item = &'a String> + 'a {
        self.files.keys().filter(move |k| exts.iter().any(|e| k.ends_with(e)))
    }
}

pub fn run(tree: &Tree) -> Report {
    let mut r = Report::default();
    let names = theme_names(tree, &mut r);
    check_frontmatter(tree, &mut r);
    check_manifests(tree, &mut r);
    check_skills(tree, &mut r);
    check_envelopes(tree, &mut r);
    check_names(tree, &names, &mut r);
    check_caps(tree, &mut r);
    check_links(tree, &mut r);
    check_abs_paths(tree, &mut r);
    check_councils(tree, &mut r);
    r
}

// ── Council artifacts (specs/003 §4.1) ──────────────────────────────────────

/// A Council record written by the runtime (`council_format: 1` in its frontmatter) must carry a Minority
/// Report that matches its own structured Round 2 output: nothing dropped, nothing invented as unanimity.
fn check_councils(tree: &Tree, r: &mut Report) {
    let council = Regex::new(r"^missions/[^/]+/councils/\d{3}-[^/]+\.md$").unwrap();
    for path in tree.with_ext(&[".md"]).filter(|p| council.is_match(p)) {
        let text = tree.read(path).unwrap_or_default();
        let head = text.split("\n---").next().unwrap_or("");
        if !head.contains("council_format: 1") {
            continue;
        }
        for problem in delphi_engine::council::lint_artifact(&text) {
            r.fail("council", path.clone(), problem);
        }
    }
}

// ── theme packs ─────────────────────────────────────────────────────────────

fn theme_names(tree: &Tree, r: &mut Report) -> Vec<String> {
    let mut names = Vec::new();
    let packs: Vec<&String> = tree.files.keys().filter(|k| k.starts_with("themes/") && k.ends_with("/theme.md")).collect();
    for p in packs {
        match theme::parse(&tree.read(p).unwrap_or_default()) {
            Ok(t) => names.extend(t.display_names()),
            Err(errs) => {
                for e in errs {
                    r.fail("theme", p.clone(), e);
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

// ── provenance frontmatter (specs/001 §5) ───────────────────────────────────

fn check_frontmatter(tree: &Tree, r: &mut Report) {
    for path in tree.with_ext(&[".md"]) {
        if !GOVERNED.iter().any(|g| if g.ends_with('/') { path.starts_with(g) } else { path == g }) {
            continue;
        }
        let text = tree.read(path).unwrap_or_default();
        match frontmatter::parse(&text) {
            Err(e) => r.fail("frontmatter", path.clone(), e),
            Ok(doc) => match doc.frontmatter {
                None => r.fail("frontmatter", path.clone(), "missing provenance frontmatter (specs/001 §5)"),
                Some(fm) => {
                    for e in frontmatter::check_provenance(&fm) {
                        r.fail("frontmatter", path.clone(), e);
                    }
                }
            },
        }
    }
}

// ── agent manifests (specs/001 §2) ──────────────────────────────────────────

pub fn catalog_skills(tree: &Tree) -> BTreeSet<String> {
    let re = Regex::new(r"(?m)^\| `([a-z0-9-]+)` \|").unwrap();
    let text = tree.read("skills/CATALOG.md").unwrap_or_default();
    re.captures_iter(&text).map(|c| c[1].to_string()).collect()
}

fn check_manifests(tree: &Tree, r: &mut Report) {
    let manifests: Vec<String> = tree
        .files
        .keys()
        .filter(|k| k.starts_with("agents/") && k.ends_with("/AGENT.md"))
        .cloned()
        .collect();
    for path in manifests {
        let dir = path.trim_start_matches("agents/").trim_end_matches("/AGENT.md").to_string();
        let text = tree.read(&path).unwrap_or_default();
        let builtin = dir.strip_prefix("custom/").map(|id| builtin_fm(tree, id));
        let (fails, warns) = manifest_findings(tree, &text, &dir, builtin);
        for e in fails {
            r.fail("manifest", path.clone(), e);
        }
        for w in warns {
            r.warn("manifest", path.clone(), w);
        }
    }
}

/// The built-in manifest for `id`, from `base/` in workspace mode.
pub fn builtin_fm(tree: &Tree, id: &str) -> Option<Mapping> {
    let key = format!("agents/{}/AGENT.md", id);
    let path = tree.builtin.get(&key).or_else(|| tree.files.get(&key))?;
    fs::read_to_string(path).ok().and_then(|t| frontmatter::parse(&t).ok()).and_then(|d| d.frontmatter)
}

/// All checks for one manifest. `dir` is `<id>` or `custom/<id>`. `builtin` is `Some` for an override
/// (`custom/`, `core/`, `operator/core/`); its inner `None` means there is no built-in to narrow.
/// Returns (failures, warnings).
pub fn manifest_findings(tree: &Tree, text: &str, dir: &str, builtin: Option<Option<Mapping>>) -> (Vec<String>, Vec<String>) {
    let mut fails = Vec::new();
    let mut warns = Vec::new();
    let doc = match frontmatter::parse(text) {
        Ok(d) => d,
        Err(e) => return (vec![e], warns),
    };
    let fm = match doc.frontmatter {
        Some(m) => m,
        None => return (vec!["missing frontmatter".into()], warns),
    };
    fails.extend(manifest_errors(&fm, dir));
    let lines = doc.body.lines().count();
    if lines > BODY_LINE_CAP {
        fails.push(format!("body is {} lines (cap {})", lines, BODY_LINE_CAP));
    }
    let id = get_str(&fm, "id").unwrap_or("").to_string();
    let catalog = catalog_skills(tree);
    let mut planned = 0;
    for s in str_list(&fm, "authorized_skills") {
        if let Some(pkg) = skill_package(tree, &s) {
            let agents = load_fm(tree, &pkg).map(|m| str_list(&m, "authorized_agents")).unwrap_or_default();
            if !agents.contains(&id) {
                fails.push(format!("skill `{}` does not list `{}` in authorized_agents (specs/001 §2.3)", s, id));
            }
        } else if catalog.contains(&s) {
            planned += 1;
        } else {
            fails.push(format!("authorized skill `{}` is neither packaged nor in skills/CATALOG.md", s));
        }
    }
    if planned > 0 {
        warns.push(format!("{} authorized skill(s) are `planned` (catalog only, no SKILL.md yet)", planned));
    }
    match builtin {
        None => {}
        Some(None) => fails.push(format!("override of `{}` has no built-in manifest", id)),
        Some(Some(b)) => fails.extend(override_widening(&b, &fm, &|s| attachable_skill(tree, &b, s))),
    }
    (fails, warns)
}

/// Package path for a skill, by precedence: operator, team custom, built-in.
fn skill_package(tree: &Tree, skill: &str) -> Option<String> {
    [format!("operator/skills/{}/SKILL.md", skill), format!("skills/custom/{}/SKILL.md", skill), format!("skills/{}/SKILL.md", skill)]
        .into_iter()
        .find(|k| tree.files.contains_key(k))
}

/// Frontmatter of a document's text, when it has a parseable one.
pub fn frontmatter_of(text: &str) -> Option<Mapping> {
    frontmatter::parse(text).ok().and_then(|d| d.frontmatter)
}

pub fn load_fm(tree: &Tree, logical: &str) -> Option<Mapping> {
    tree.read(logical).and_then(|t| frontmatter::parse(&t).ok()).and_then(|d| d.frontmatter)
}

/// D-21: a ratified custom/operator skill may be attached when each of its `required_permissions`
/// is already granted in full by the built-in role's scope. (Ratification is checked by `doctor`.)
pub fn attachable_skill(tree: &Tree, builtin: &Mapping, skill: &str) -> bool {
    let pkg = ["operator/skills/", "skills/custom/"].iter().map(|p| format!("{}{}/SKILL.md", p, skill)).find(|k| tree.files.contains_key(k));
    let skill_fm = match pkg.and_then(|k| load_fm(tree, &k)) {
        Some(m) => m,
        None => return false,
    };
    let perms = match string_seq(skill_fm.get(Value::String("required_permissions".into()))) {
        Some(p) => p,
        None => return false,
    };
    let scope = builtin.get(Value::String("default_access_scope".into())).and_then(|v| v.as_mapping());
    let has = |k: &str, want: &str| scope.map(|s| seq_strs(s.get(Value::String(k.into()))).iter().any(|g| g == want)).unwrap_or(false);
    perms.iter().all(|p| match p.as_str() {
        "read:all" => has("read", "*"),
        "write:worktree" => has("write", "@worktree/**"),
        "write:missions" => has("write", "missions/**"),
        "shell:scoped_scripts" => has("execute", "shell-worktree-scoped"),
        _ => false,
    })
}

/// `Some(list)` when `v` is a sequence of strings; `None` when it is missing, not a list, or holds
/// anything other than strings.
fn string_seq(v: Option<&Value>) -> Option<Vec<String>> {
    v?.as_sequence()?.iter().map(|x| x.as_str().map(String::from)).collect()
}

// ── skill packages (specs/001 §3.1) ─────────────────────────────────────────

const SKILL_PERMISSIONS: [&str; 5] = ["read:all", "write:worktree", "write:missions", "shell:scoped_scripts", "network:gated"];
const SANDBOX_MODES: [&str; 3] = ["readonly", "worktree-isolated", "outbound-gated"];

fn check_skills(tree: &Tree, r: &mut Report) {
    let pkgs: Vec<String> = tree.files.keys().filter(|k| (k.starts_with("skills/") || k.starts_with("operator/skills/")) && k.ends_with("/SKILL.md")).cloned().collect();
    for path in pkgs {
        let text = tree.read(&path).unwrap_or_default();
        let doc = match frontmatter::parse(&text) {
            Ok(d) => d,
            Err(e) => {
                r.fail("skill", path.clone(), e);
                continue;
            }
        };
        let fm = match doc.frontmatter {
            Some(m) => m,
            None => {
                r.fail("skill", path.clone(), "missing frontmatter");
                continue;
            }
        };
        let custom = path.starts_with("skills/custom/") || path.starts_with("operator/skills/");
        let dir = path.trim_end_matches("/SKILL.md").rsplit('/').next().unwrap_or("").to_string();
        for e in skill_errors(&fm, &dir, custom) {
            r.fail("skill", path.clone(), e);
        }
        let lines = doc.body.lines().count();
        if lines > BODY_LINE_CAP {
            r.fail("skill", path.clone(), format!("body is {} lines (cap {})", lines, BODY_LINE_CAP));
        }
        let pkg = path.trim_end_matches("SKILL.md");
        for e in skill_package_errors(&doc.body, &fm, &|rel| tree.files.contains_key(&format!("{}{}", pkg, rel))) {
            r.fail("skill", path.clone(), e);
        }
    }
}

/// What a `SKILL.md` promises about its own package: every `templates/` file and declared script it names exists, and
/// every `delphi check <kind>` it tells an agent to run is a kind the binary has. `has` probes the package directory.
pub fn skill_package_errors(body: &str, fm: &Mapping, has: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut errs = Vec::new();
    let template = Regex::new(r"\btemplates/[A-Za-z0-9_.-]+").unwrap();
    for m in template.find_iter(body) {
        if !has(m.as_str()) {
            errs.push(format!("names `{}`, which is not in the package", m.as_str()));
        }
    }
    let check = Regex::new(r"delphi check ([a-z][a-z-]*)").unwrap();
    for c in check.captures_iter(body) {
        if !crate::check::KINDS.contains(&&c[1]) {
            errs.push(format!("runs `delphi check {}`, which is not a check kind ({})", &c[1], crate::check::KINDS.join(", ")));
        }
    }
    for sc in fm.get(Value::String("deterministic_scripts".into())).and_then(|v| v.as_sequence()).into_iter().flatten() {
        if let Some(p) = sc.as_mapping().and_then(|m| get_str(m, "path")) {
            if !has(p) {
                errs.push(format!("declares script `{}`, which is not in the package", p));
            }
        }
    }
    errs
}

pub fn skill_errors(fm: &Mapping, dir: &str, custom: bool) -> Vec<String> {
    let mut errs = Vec::new();
    let kebab = Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*$").unwrap();
    match get_str(fm, "name") {
        Some(n) if n == dir && kebab.is_match(n) => {}
        n => errs.push(format!("name `{}` must be kebab-case and match directory `{}`", n.unwrap_or(""), dir)),
    }
    match get_str(fm, "description") {
        Some(d) if !d.trim().is_empty() && d.chars().count() <= 200 => {}
        _ => errs.push("description must be 1–200 characters".into()),
    }
    let agents = str_list(fm, "authorized_agents");
    if agents.is_empty() {
        errs.push("authorized_agents must list at least one role".into());
    }
    for a in &agents {
        if !ROLES.contains(&a.as_str()) || a == "operator" {
            errs.push(format!("authorized_agents: `{}` is not an agent role", a));
        }
    }
    for k in ["input_schema", "output_schema"] {
        let ok = fm.get(Value::String(k.into())).and_then(|v| v.as_mapping()).map(|m| get_str(m, "type").is_some()).unwrap_or(false);
        if !ok {
            errs.push(format!("{} must be a JSON Schema object with a `type`", k));
        }
    }
    match string_seq(fm.get(Value::String("required_permissions".into()))) {
        None => errs.push("required_permissions must be a list of strings".into()),
        Some(perms) => {
            for p in perms {
                if !SKILL_PERMISSIONS.contains(&p.as_str()) {
                    errs.push(format!("unknown required_permission `{}`", p));
                }
            }
        }
    }
    if string_seq(fm.get(Value::String("authorized_agents".into()))).is_none() {
        errs.push("authorized_agents must be a list of strings".into());
    }
    let scripts_v = fm.get(Value::String("deterministic_scripts".into()));
    if scripts_v.map(|v| !v.is_sequence() && !v.is_null()).unwrap_or(false) {
        errs.push("deterministic_scripts must be a list".into());
    }
    if let Some(scripts) = scripts_v.and_then(|v| v.as_sequence()) {
        if !custom && !scripts.is_empty() {
            errs.push("deterministic_scripts are allowed only in custom skills (built-in checks are compiled in, D-02)".into());
        }
        for sc in scripts {
            let m = match sc.as_mapping() {
                Some(m) => m,
                None => {
                    errs.push("deterministic_scripts entries must be mappings".into());
                    continue;
                }
            };
            let path = get_str(m, "path").unwrap_or("");
            if !path.starts_with("scripts/") || path.split('/').any(|s| s == "..") {
                errs.push(format!("script path `{}` must be under scripts/", path));
            }
            if !m.get(Value::String("timeout_ms".into())).and_then(|v| v.as_u64()).map(|n| n > 0 && n <= 600_000).unwrap_or(false) {
                errs.push(format!("script `{}`: timeout_ms must be 1–600000", path));
            }
            if !get_str(m, "sandbox_mode").map(|x| SANDBOX_MODES.contains(&x)).unwrap_or(false) {
                errs.push(format!("script `{}`: sandbox_mode must be one of {:?}", path, SANDBOX_MODES));
            }
        }
    }
    errs
}

// ── handoff envelopes (specs/001 §4) ────────────────────────────────────────


fn check_envelopes(tree: &Tree, r: &mut Report) {
    let files: Vec<String> = tree.files.keys().filter(|k| k.starts_with("missions/") && k.contains("/handoffs/") && (k.ends_with(".yaml") || k.ends_with(".yml"))).cloned().collect();
    for path in files {
        let text = tree.read(&path).unwrap_or_default();
        for e in envelope_errors(&text) {
            r.fail("envelope", path.clone(), e);
        }
    }
}


/// Schema checks for one manifest. `dir` is `<id>` for built-ins or `custom/<id>` for overrides.
pub fn manifest_errors(fm: &Mapping, dir: &str) -> Vec<String> {
    let mut errs = Vec::new();
    let id = get_str(fm, "id").unwrap_or("");
    let expected = dir.rsplit('/').next().unwrap_or(dir);
    if !ROLES.contains(&id) {
        errs.push(format!("id `{}` is not a canonical role", id));
    } else if id != expected {
        errs.push(format!("id `{}` does not match directory `{}`", id, dir));
    }
    for k in ["title", "role", "cognitive_profile", "memory_scope"] {
        if get_str(fm, k).is_none() {
            errs.push(format!("missing `{}`", k));
        }
    }
    if let Some(c) = get_str(fm, "cognitive_profile") {
        if !COGNITIVE.contains(&c) {
            errs.push(format!("cognitive_profile `{}` is not one of {:?}", c, COGNITIVE));
        }
    }
    if ROLES.contains(&id) && get_str(fm, "memory_scope") != Some(format!("memory/agents/{}/", id).as_str()) {
        errs.push(format!("memory_scope must be `memory/agents/{}/`", id));
    }
    match fm.get(Value::String("max_turn_budget".into())).and_then(|v| v.as_u64()) {
        Some(n) if n > 0 => {}
        _ => errs.push("max_turn_budget must be a positive integer".into()),
    }
    let scope = fm.get(Value::String("default_access_scope".into())).and_then(|v| v.as_mapping());
    match scope {
        None => errs.push("missing default_access_scope".into()),
        Some(s) => {
            for k in ["read", "write", "execute"] {
                if string_seq(s.get(Value::String(k.into()))).is_none() {
                    errs.push(format!("default_access_scope.{} must be a list of strings", k));
                }
            }
            for g in seq_strs(s.get(Value::String("read".into()))) {
                if let Some(e) = read_glob_error(&g) {
                    errs.push(e);
                }
            }
            for g in seq_strs(s.get(Value::String("write".into()))) {
                if let Some(e) = write_glob_error(id, &g) {
                    errs.push(e);
                }
            }
        }
    }
    if string_seq(fm.get(Value::String("authorized_skills".into()))).is_none() {
        errs.push("authorized_skills must be a list of strings".into());
    }
    errs
}

/// True when some non-trivial segment of `glob` (anything but `*`/`**`) could match `name`. Bare
/// `*`/`**` segments are left to the runtime guard, which denies these names on every access.
fn names_segment(glob: &str, name: &str) -> bool {
    glob.split('/').any(|s| s != "**" && s != "*" && delphi_core::pathguard::glob_match(s, name))
}

fn logical_glob_error(glob: &str) -> Option<&'static str> {
    let lower = glob.to_lowercase();
    if glob.is_empty() || glob.starts_with('/') || glob.starts_with('~') || glob.contains('$') || glob.contains('\\') || glob.split('/').any(|s| s == "..") {
        return Some("must be a logical path (no absolute, home, variable or `..` paths)");
    }
    if lower.starts_with(".delphi") || lower.split('/').any(|s| s == ".delphi") {
        return Some("physical `.delphi/` paths never appear in manifests");
    }
    if names_segment(&lower, ".git") {
        return Some("may never match a `.git` file or directory");
    }
    if ["operator/", "run/", "local/", "proposals/"].iter().any(|p| lower.starts_with(p))
        || ["state.db", "config.toml", "ratified", "ratified.lock", "provenance", "provenance.db"].iter().any(|n| names_segment(&lower, n) || lower.split('/').next() == Some(*n))
    {
        return Some("operator home, daemon state, proposals, ratification and provenance state are never agent-accessible (§2.2, 001-A1)");
    }
    if names_segment(&lower, "events.jsonl") || names_segment(&lower, "events") || names_segment(&lower, ".barrier") {
        return Some("the event log and Council barrier are never agent-accessible");
    }
    None
}

/// Read globs are `*` or logical paths (`specs/001` §2.1).
pub fn read_glob_error(glob: &str) -> Option<String> {
    if glob == "*" {
        return None;
    }
    if let Some(m) = prefix_glob_error(glob) {
        return Some(format!("read glob `{}`: {}", glob, m));
    }
    logical_glob_error(glob).map(|m| format!("read glob `{}`: {}", glob, m))
}

/// Wildcards in the leading segments may still reach a never-readable zone (`l*/**` → `local/`).
fn prefix_glob_error(glob: &str) -> Option<&'static str> {
    let lower = glob.to_lowercase();
    let segs: Vec<&str> = lower.split('/').skip_while(|s| *s == "**").collect();
    let first = *segs.first()?;
    if first == "*" {
        return None;
    }
    let m = |seg: &str, name: &str| delphi_core::pathguard::glob_match(seg, name);
    let hits = ["operator", "local", "run", "proposals"].iter().any(|d| m(first, d))
        || (m(first, "memory") && segs.get(1).map(|s| m(s, "operator")).unwrap_or(false));
    if hits {
        Some("may reach operator home, workspace-local state, proposals or daemon state (§2.2, 001-A1)")
    } else {
        None
    }
}

/// `[REV-SEC-02]` and `specs/001` §2.2: write globs are logical, zone-scoped, never reach `.git`, and
/// each zone is writable only by the roles whose duties need it.
pub fn write_glob_error(role: &str, glob: &str) -> Option<String> {
    let bad = |m: &str| Some(format!("write glob `{}`: {}", glob, m));
    if let Some(m) = logical_glob_error(glob) {
        return bad(m);
    }
    let lower = glob.to_lowercase();
    let segs: Vec<&str> = lower.split('/').collect();
    if segs.iter().any(|s| *s != "**" && *s != "*" && delphi_core::pathguard::glob_match(s, ".github")) {
        return bad("CI configuration (`.github/**`) is never agent-writable");
    }
    if lower == "**" || lower == "*" || lower.starts_with("**/") || lower.starts_with("*/") {
        return bad("unscoped glob");
    }
    let protected_name = ["heuristics.md", "team-heuristics.md", "lessons.md", "governance.md", "review.md"].iter().any(|n| names_segment(&lower, n) && !lower.starts_with("@worktree/"));
    if protected_name || lower.starts_with("memory/") || lower.starts_with("agents/") || lower.starts_with("themes/") {
        return bad("heuristics, lessons, memory and manifests change only through ratified proposals");
    }
    let only = |roles: &[&str], zone: &str| if roles.contains(&role) { None } else { bad(&format!("only {} may write {}", roles.join(", "), zone)) };
    if let Some(rest) = lower.strip_prefix("@worktree/") {
        return match role {
            "builder" => None,
            "writer" if rest == "readme.md" || rest.starts_with("docs/") => None,
            _ => bad("only builder (and writer, for README.md and docs/**) may write `@worktree/**`"),
        };
    }
    if lower.starts_with("@project/") {
        return bad("project repos are read-only (001-A2)");
    }
    if lower.starts_with("skills/custom/") {
        let at = segs.iter().position(|s| *s == "scripts");
        if !at.map(|i| i >= 3 && i + 1 < segs.len()).unwrap_or(false) || names_segment(&lower, "skill.md") {
            return bad("only `skills/custom/**/scripts/**` is writable; SKILL.md changes through ratified proposals");
        }
        return only(&["builder"], "`skills/custom/**/scripts/**`");
    }
    if lower.starts_with("skills/") {
        return bad("built-in skills are never agent-writable");
    }
    if lower.starts_with("specs/") {
        return only(&["architect"], "`specs/**`");
    }
    if lower == "context/index.md" || lower.starts_with("context/domains/") {
        return only(&["orchestrator", "scout", "writer"], "`context/index.md` and `context/domains/**`");
    }
    if lower.starts_with("missions/") {
        return None;
    }
    bad("not under a writable logical zone (missions/, context/domains/, specs/, skills/custom/, @worktree/)")
}

/// `specs/001` §2.3 narrow-only rule: an override may not widen the built-in role's scope or skills,
/// except to attach a ratified custom/operator skill whose permissions fit the built-in scope (D-21).
pub fn override_widening(builtin: &Mapping, ovr: &Mapping, attachable: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut errs = Vec::new();
    let scope = |m: &Mapping, k: &str| -> BTreeSet<String> {
        m.get(Value::String("default_access_scope".into()))
            .and_then(|v| v.as_mapping())
            .map(|s| seq_strs(s.get(Value::String(k.into()))).into_iter().collect())
            .unwrap_or_default()
    };
    // `g` is within `b` when `b` matches it as a path and `b`'s only wildcards are `**` (so a `*` in
    // `g` can only meet a `**` in `b`, never a narrower pattern).
    let within = |b: &str, g: &str| b == g || (b.split('/').all(|s| s == "**" || !s.contains(['*', '?'])) && delphi_core::pathguard::glob_match(b, g));
    for k in ["read", "write", "execute"] {
        let base = scope(builtin, k);
        for g in scope(ovr, k) {
            let ok = if k == "execute" { base.contains(&g) } else { base.contains("*") && k == "read" || base.iter().any(|b| within(b, &g)) };
            if !ok {
                errs.push(format!("override widens default_access_scope.{} with `{}` (narrow-only, specs/001 §2.3)", k, g));
            }
        }
    }
    let base: BTreeSet<String> = str_list(builtin, "authorized_skills").into_iter().collect();
    for s in str_list(ovr, "authorized_skills") {
        if !base.contains(&s) && !attachable(&s) {
            errs.push(format!("override adds skill `{}` that is not a ratified within-scope custom/operator skill", s));
        }
    }
    errs
}

fn seq_strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str().map(|y| y.to_string())).collect())
        .unwrap_or_default()
}

fn str_list(m: &Mapping, key: &str) -> Vec<String> {
    seq_strs(m.get(Value::String(key.into())))
}

// ── canonical role names (REV-ARCH-05) ──────────────────────────────────────

fn check_names(tree: &Tree, names: &[String], r: &mut Report) {
    if names.is_empty() {
        return;
    }
    let alternation = names.iter().map(|n| regex::escape(n)).collect::<Vec<_>>().join("|");
    let re = Regex::new(&format!(r"(?i)\b({})\b", alternation)).unwrap();
    let council = Regex::new(r"^missions/[^/]+/councils/").unwrap();
    for path in tree.with_ext(&[".md", ".toml"]) {
        // Theme packs define the names; historical council records are verbatim (specs/001 §7).
        if path.starts_with("themes/") || council.is_match(path) {
            continue;
        }
        let text = tree.read(path).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            for m in re.find_iter(line) {
                let next = line[m.end()..].chars().next();
                if next == Some('-') {
                    continue; // e.g. the external project `hermes-agent`
                }
                r.fail(
                    "canonical-names",
                    format!("{}:{}", path, i + 1),
                    format!("theme name `{}` outside themes/ (REV-ARCH-05: use the canonical role ID)", m.as_str()),
                );
            }
        }
    }
}

// ── Rule of 25 caps (specs/004 §2.2, D-09; doctor check 4) ──────────────────

fn check_caps(tree: &Tree, r: &mut Report) {
    let files: Vec<String> = tree
        .files
        .keys()
        .filter(|k| {
            k.ends_with("heuristics.md") && (k.starts_with("context/") || k.as_str() == "heuristics.md" || k.starts_with("operator/"))
                || (k.starts_with("memory/agents/") || k.starts_with("operator/memory/agents/")) && k.ends_with("/lessons.md")
        })
        .cloned()
        .collect();
    for path in files {
        let text = tree.read(&path).unwrap_or_default();
        for msg in cap_errors(&text) {
            r.fail("rule-caps", path.clone(), msg);
        }
    }
}


// ── links and absolute paths (doctor check 5) ───────────────────────────────

fn strip_fences(text: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push('\n');
            continue;
        }
        if !in_fence {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Normalize `a/b/../c` → `a/c`. Returns None if it climbs above the root.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn check_links(tree: &Tree, r: &mut Report) {
    let re = Regex::new(r"\]\(([^)\s#]+)(#[^)\s]*)?\)").unwrap();
    for path in tree.with_ext(&[".md"]) {
        let text = strip_fences(&tree.read(path).unwrap_or_default());
        // Operator overrides and skills are loaded as if they sat at their logical counterpart, so
        // their relative links resolve from there.
        let logical = if let Some(rest) = path.strip_prefix("operator/core/") {
            rest.to_string()
        } else if path.starts_with("operator/skills/") {
            path.trim_start_matches("operator/").to_string()
        } else {
            path.clone()
        };
        let dir = match logical.rfind('/') {
            Some(i) => &logical[..i],
            None => "",
        };
        let seeded = tree.seeded.contains(path);
        for (i, line) in text.lines().enumerate() {
            for c in re.captures_iter(line) {
                let target = &c[1];
                let anchor = c.get(2).map(|a| a.as_str()).unwrap_or("");
                if target.contains("://") || target.starts_with("mailto:") {
                    continue;
                }
                let joined = if dir.is_empty() { target.to_string() } else { format!("{}/{}", dir, target) };
                let loc = format!("{}:{}", path, i + 1);
                match normalize(&joined) {
                    None => r.fail("links", loc, format!("link `{}` escapes the tree", target)),
                    Some(n) if n.is_empty() => {}
                    Some(n) if tree.exists(&n) => {
                        if let Some(e) = line_anchor_error(tree, &n, anchor) {
                            r.fail("links", loc, format!("link `{}{}`: {}", target, anchor, e));
                        }
                    }
                    Some(n) => {
                        if tree.mode == Mode::Workspace && seeded && UNSEEDED.iter().any(|u| n.starts_with(u)) {
                            r.push(Level::Skipped, "links", loc, format!("link to unseeded design record `{}`", n));
                        } else {
                            r.fail("links", loc, format!("broken link `{}` (resolves to `{}`)", target, n));
                        }
                    }
                }
            }
        }
    }
}

/// `#Lstart-Lend` anchors must point inside the target file (`specs/006` §5 check 5).
fn line_anchor_error(tree: &Tree, target: &str, anchor: &str) -> Option<String> {
    let re = Regex::new(r"^#L(\d+)(?:-L(\d+))?$").unwrap();
    let c = re.captures(anchor)?;
    let start: usize = c[1].parse().ok()?;
    let end: usize = c.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(start);
    let lines = tree.read(target)?.lines().count();
    if start == 0 || end < start || end > lines {
        return Some(format!("line range {}-{} is outside the file ({} lines)", start, end, lines));
    }
    None
}

fn check_abs_paths(tree: &Tree, r: &mut Report) {
    let re = Regex::new(r"/(Users|home)/[A-Za-z0-9._-]+/").unwrap();
    for path in tree.with_ext(&TEXT_EXTS) {
        let text = tree.read(path).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                r.fail("abs-paths", format!("{}:{}", path, i + 1), "absolute home path (REV-SEC-05)");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_glob_rules() {
        assert!(write_glob_error("builder", "@worktree/**").is_none());
        assert!(write_glob_error("writer", "@worktree/docs/**").is_none());
        assert!(write_glob_error("red-team", "@worktree/**").is_some());
        assert!(write_glob_error("builder", "@worktree/.git/hooks/x").is_some());
        assert!(write_glob_error("integrator", ".github/**").is_some());
        assert!(write_glob_error("builder", "skills/review/scripts/**").is_some());
        assert!(write_glob_error("builder", "skills/custom/x/scripts/**").is_none());
        assert!(write_glob_error("scout", "/etc/passwd").is_some());
        assert!(write_glob_error("scout", ".delphi/worktrees/**").is_some());
        assert!(write_glob_error("scout", "missions/**/research/**").is_none());
        assert!(write_glob_error("builder", "@worktree/.GIT/**").is_some());
        assert!(write_glob_error("builder", "@worktree/*git*/x").is_some());
        assert!(write_glob_error("builder", "@worktree/.GITHUB/**").is_some());
        assert!(write_glob_error("scout", "specs/**").is_some());
        assert!(write_glob_error("architect", "specs/**").is_none());
        assert!(write_glob_error("scout", "skills/custom/**").is_some());
        assert!(write_glob_error("orchestrator", "context/heuristics.md").is_some());
        assert!(write_glob_error("red-team", "missions/**/councils/.barrier/**").is_some());
        assert!(write_glob_error("orchestrator", "missions/m1/events.jsonl").is_some());
        assert!(write_glob_error("builder", "@project/app/**").is_some());
        assert!(read_glob_error("/etc/passwd").is_some());
        assert!(read_glob_error("operator/**").is_some());
        assert!(read_glob_error("@worktree/.git/**").is_some());
        assert!(read_glob_error("*").is_none() && read_glob_error("specs/**").is_none());
        for g in ["l*/**", "o*/**", "prop*/x", "memory/op*/x", "r?n/x", "**/local/**"] {
            assert!(read_glob_error(g).is_some(), "{}", g);
        }
        assert!(read_glob_error("lib*/x").is_none() && read_glob_error("@worktree/local/x").is_none());
        for g in ["missions/**/events*.jsonl", "missions/**/councils/.barrier*", "missions/**/councils/.*/x", "local/**", "config.toml", "ratified/**", "proposals/**"] {
            assert!(read_glob_error(g).is_some(), "{}", g);
            assert!(write_glob_error("orchestrator", g).is_some(), "{}", g);
        }
        assert!(write_glob_error("builder", "skills/custom/**").is_some());
        assert!(write_glob_error("builder", "skills/custom/**/scripts/**").is_none());
        for g in ["skills/custom/scripts/**", "skills/custom/scripts/SKILL.md", "skills/custom/x/scripts", "skills/custom/x/scripts/SKILL.md"] {
            assert!(write_glob_error("builder", g).is_some(), "{}", g);
        }
        assert!(write_glob_error("scout", "context/domains/*heuristics*").is_some());
    }

    // specs/001 §7 test 15 (manifest part)
    #[test]
    fn override_cannot_widen() {
        let b: Mapping = serde_yaml::from_str("default_access_scope: {read: ['*'], write: ['@worktree/**'], execute: ['test-runner']}\nauthorized_skills: [tdd-execution]").unwrap();
        let o: Mapping = serde_yaml::from_str("default_access_scope: {read: ['*'], write: ['**'], execute: ['test-runner']}\nauthorized_skills: [tdd-execution, my-skill, evil]").unwrap();
        let errs = override_widening(&b, &o, &|s| s == "my-skill");
        assert_eq!(errs.len(), 2, "{:?}", errs);
        assert!(errs.iter().any(|e| e.contains("`**`")));
        assert!(errs.iter().any(|e| e.contains("evil")));
    }

    fn tree_of(files: &[(&str, &str)]) -> (tempfile::TempDir, Tree) {
        let t = tempfile::tempdir().unwrap();
        for (k, v) in files {
            let p = t.path().join(k);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, v).unwrap();
        }
        let tree = Tree::from_repo(t.path()).unwrap();
        (t, tree)
    }

    fn fails(tree: &Tree, check: &str) -> Vec<String> {
        run(tree).findings.into_iter().filter(|f| f.level == Level::Fail && f.check == check).map(|f| format!("{} {}", f.location, f.message)).collect()
    }

    const BUILDER: &str = "---\nid: builder\ntitle: B\nrole: b\ncognitive_profile: code-craft\nmemory_scope: memory/agents/builder/\nmax_turn_budget: 5\ndefault_access_scope: {read: ['*'], write: ['@worktree/**'], execute: ['test-runner']}\nauthorized_skills: [tdd-execution]\n---\nbody\n";

    // specs/001 §7 tests 1 and 15, end to end through `lint::run`
    #[test]
    fn team_override_widening_fails_lint() {
        let ovr = BUILDER.replace("['@worktree/**']", "['@worktree/**', 'specs/**']").replace("['test-runner']", "['test-runner', 'shell-host']");
        let (_t, tree) = tree_of(&[("agents/builder/AGENT.md", BUILDER), ("agents/custom/builder/AGENT.md", &ovr), ("skills/tdd-execution/SKILL.md", &skill("tdd-execution", "[builder]"))]);
        let f = fails(&tree, "manifest");
        assert!(f.iter().any(|e| e.contains("widens default_access_scope.write")), "{:?}", f);
        assert!(f.iter().any(|e| e.contains("widens default_access_scope.execute")), "{:?}", f);
        // Narrowing to a sub-glob is allowed.
        let narrow = BUILDER.replace("['@worktree/**']", "['@worktree/crates/**']");
        let (_t, tree) = tree_of(&[("agents/builder/AGENT.md", BUILDER), ("agents/custom/builder/AGENT.md", &narrow), ("skills/tdd-execution/SKILL.md", &skill("tdd-execution", "[builder]"))]);
        assert!(fails(&tree, "manifest").is_empty(), "{:?}", fails(&tree, "manifest"));
    }

    // D-21: a custom skill attaches only when the built-in scope already grants its permissions.
    #[test]
    fn custom_skill_attachment_needs_full_permissions() {
        let ovr = BUILDER.replace("[tdd-execution]", "[tdd-execution, mine]");
        let mine = skill("mine", "[builder]").replace("[read:all]", "[read:all, shell:scoped_scripts]");
        let (_t, tree) = tree_of(&[("agents/builder/AGENT.md", BUILDER), ("agents/custom/builder/AGENT.md", &ovr), ("skills/tdd-execution/SKILL.md", &skill("tdd-execution", "[builder]")), ("skills/custom/mine/SKILL.md", &mine)]);
        assert!(fails(&tree, "manifest").iter().any(|e| e.contains("`mine`")), "builder lacks shell-worktree-scoped here");
        let no_perms = skill("mine", "[builder]").replace("required_permissions: [read:all]\n", "");
        let (_t, tree) = tree_of(&[("agents/builder/AGENT.md", BUILDER), ("agents/custom/builder/AGENT.md", &ovr), ("skills/tdd-execution/SKILL.md", &skill("tdd-execution", "[builder]")), ("skills/custom/mine/SKILL.md", &no_perms)]);
        assert!(fails(&tree, "manifest").iter().any(|e| e.contains("`mine`")), "missing permissions must not attach vacuously");
    }

    fn skill(name: &str, agents: &str) -> String {
        format!("---\nname: {}\ndescription: d\nauthorized_agents: {}\ninput_schema: {{type: object}}\noutput_schema: {{type: object}}\nrequired_permissions: [read:all]\n---\nbody\n", name, agents)
    }

    #[test]
    fn skill_must_authorize_the_role_and_follow_schema() {
        let (_t, tree) = tree_of(&[("agents/builder/AGENT.md", BUILDER), ("skills/tdd-execution/SKILL.md", &skill("tdd-execution", "[writer]"))]);
        assert!(fails(&tree, "manifest").iter().any(|e| e.contains("authorized_agents")));
        let bad = skill("wrong-name", "[builder]") .replace("---\nbody", "deterministic_scripts: [{path: scripts/x.sh, timeout_ms: 1, sandbox_mode: readonly}]\n---\nbody");
        let (_t, tree) = tree_of(&[("skills/tdd-execution/SKILL.md", &bad)]);
        let f = fails(&tree, "skill");
        assert!(f.iter().any(|e| e.contains("match directory")) && f.iter().any(|e| e.contains("custom skills")), "{:?}", f);
    }

    #[test]
    fn envelopes_validated() {
        let good = "handoff_id: h\nmission_id: m\nparent_span_id: s\ncaller_agent: orchestrator\ntarget_agent: architect\nobjective: o\ncontext_pointers: [{uri: 'specs/a.md#L1-L2', heading: h, blake3: x, reason: r}]\nexpected_output: {artifact_type: spec}\nrouting_directive: {model_tier: workhorse}\n";
        assert!(envelope_errors(good).is_empty(), "{:?}", envelope_errors(good));
        let bad = good.replace("architect", "athena").replace("specs/a.md", "/etc/passwd").replace("{model_tier: workhorse}", "{model_id: x}");
        assert_eq!(envelope_errors(&bad).len(), 4, "{:?}", envelope_errors(&bad));
        assert_eq!(envelope_errors(&good.replace("#L1-L2", "")).len(), 1);
        assert_eq!(envelope_errors(&good.replace("heading: h", "heading: ''")).len(), 1);
        let no_ptrs: String = good.lines().filter(|l| !l.starts_with("context_pointers")).map(|l| format!("{}\n", l)).collect();
        assert_eq!(envelope_errors(&no_ptrs).len(), 1);
    }

    #[test]
    fn runtime_council_records_must_carry_a_matching_minority_report() {
        let ok = "---\ncouncil_format: 1\n---\n\n## Round 2 structured output\n\n- red-team | P0 | Drops work\n- red-team | concur | false\n\n## Verdict\n\nx\n\n## Accepted Risks & Minority Report (Red-Team / Dissent)\n\n- [P0] red-team: Drops work\n";
        let (_t, tree) = tree_of(&[("missions/m/councils/001-x.md", ok)]);
        let bad = |t: &Tree| fails(t, "council").len();
        assert_eq!(bad(&tree), 0);
        let forged = ok.replace("- [P0] red-team: Drops work\n", "Unanimous — no unresolved P0/P1 (Red-Team concurs).\n");
        let (_t, tree) = tree_of(&[("missions/m/councils/001-x.md", &forged)]);
        assert!(bad(&tree) > 0);
        // Historical records (no `council_format`) are verbatim and exempt.
        let (_t, tree) = tree_of(&[("missions/m/councils/001-old.md", "# old\nno section")]);
        assert_eq!(bad(&tree), 0);
    }

    // REV-ARCH-05: case-insensitive, no "theme" substring exemption, only missions/*/councils/ exempt
    #[test]
    fn theme_names_caught_in_any_case() {
        let pack = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/greek/theme.md")).unwrap();
        let (_t, tree) = tree_of(&[("themes/greek/theme.md", &pack), ("context/a.md", "x\nif the theme is greek, athena reviews\n"), ("missions/m/councils/r1.md", "Athena said")]);
        let f = fails(&tree, "canonical-names");
        assert_eq!(f.len(), 1, "{:?}", f);
        assert!(f[0].starts_with("context/a.md:2"));
    }

    #[test]
    fn nested_target_dirs_are_still_linted() {
        let (_t, tree) = tree_of(&[("context/target/a.md", "no frontmatter\n"), ("target/ignored.md", "x"), ("sub/.git/x.md", "x")]);
        assert!(tree.files.contains_key("context/target/a.md") && !tree.files.contains_key("target/ignored.md") && !tree.files.contains_key("sub/.git/x.md"));
        assert!(!fails(&tree, "frontmatter").is_empty());
    }

    #[test]
    fn line_anchors_and_abs_paths() {
        let (_t, tree) = tree_of(&[("a.md", "[x](b.md#L2-L9)\n"), ("b.md", "1\n2\n3\n"), ("tool.py", &format!("p = '/{}/someone/x'\n", "Users"))]);
        assert_eq!(fails(&tree, "links").len(), 1);
        assert_eq!(fails(&tree, "abs-paths").len(), 1);
    }

    // specs/006 §6 test 8: this source repository lints clean.
    #[test]
    fn source_repo_lints_clean() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let tree = Tree::from_repo(&root).unwrap();
        let r = run(&tree);
        let fails: Vec<_> = r.findings.iter().filter(|f| f.level == Level::Fail).map(|f| format!("{} {}", f.location, f.message)).collect();
        assert!(fails.is_empty(), "{:#?}", fails);
    }

    #[test]
    fn normalize_paths() {
        assert_eq!(normalize("a/b/../c").as_deref(), Some("a/c"));
        assert_eq!(normalize("../x"), None);
    }
}
