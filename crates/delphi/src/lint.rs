//! `delphi lint`: deterministic file-level checks (`specs/001` §7 test 1, §6.4; `specs/006` §5 checks 3–5).
//!
//! Every check works on a [`Tree`]: logical path → physical file. In `--repo` mode the logical path is
//! the repo-relative path. In workspace mode `doctor` builds an overlay of `base/`, `core/`, `team/`
//! and `local/` (`specs/006` §3.1).

use crate::frontmatter::{self, get_str, ROLES};
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
}

/// Directories never walked in a source repo.
const SKIP_DIRS: [&str; 4] = [".git", "target", "node_modules", ".delphi"];
/// Markdown under these logical prefixes must carry §5 provenance frontmatter.
const GOVERNED: [&str; 5] = ["context/", "missions/", "specs/", "memory/agents/", "themes/"];
/// Design records that are never seeded into a workspace; links to them are `skipped` there.
const UNSEEDED: [&str; 5] = ["specs/", "missions/", "OVERVIEW.md", "README.md", "crates/"];
const COGNITIVE: [&str; 4] = ["deep-reasoning", "code-craft", "high-context-discovery", "fast-triage"];
const BODY_LINE_CAP: usize = 500;
const RULE_CHARS: usize = 350;
const RULE_COUNT: usize = 25;
const RULE_BYTES: usize = 6144;

impl Tree {
    pub fn from_repo(root: &Path) -> std::io::Result<Tree> {
        let mut files = BTreeMap::new();
        let walker = walkdir::WalkDir::new(root).into_iter().filter_entry(|e| {
            !(e.file_type().is_dir() && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        });
        for entry in walker {
            let entry = entry?;
            if entry.file_type().is_file() {
                let rel = entry.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                files.insert(rel, entry.path().to_path_buf());
            }
        }
        Ok(Tree { mode: Mode::Repo, files })
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
    check_names(tree, &names, &mut r);
    check_caps(tree, &mut r);
    check_links(tree, &mut r);
    check_abs_paths(tree, &mut r);
    r
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
        if !GOVERNED.iter().any(|g| path.starts_with(g)) {
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
    let catalog = catalog_skills(tree);
    let manifests: Vec<String> = tree
        .files
        .keys()
        .filter(|k| k.starts_with("agents/") && k.ends_with("/AGENT.md"))
        .cloned()
        .collect();
    for path in manifests {
        let dir = path.trim_start_matches("agents/").trim_end_matches("/AGENT.md").to_string();
        let text = tree.read(&path).unwrap_or_default();
        let doc = match frontmatter::parse(&text) {
            Ok(d) => d,
            Err(e) => {
                r.fail("manifest", path.clone(), e);
                continue;
            }
        };
        let fm = match doc.frontmatter {
            Some(m) => m,
            None => {
                r.fail("manifest", path.clone(), "missing frontmatter");
                continue;
            }
        };
        for e in manifest_errors(&fm, &dir) {
            r.fail("manifest", path.clone(), e);
        }
        let lines = doc.body.lines().count();
        if lines > BODY_LINE_CAP {
            r.fail("manifest", path.clone(), format!("body is {} lines (cap {})", lines, BODY_LINE_CAP));
        }
        let mut planned = 0;
        for s in str_list(&fm, "authorized_skills") {
            let packaged = tree.files.contains_key(&format!("skills/{}/SKILL.md", s));
            if packaged {
                continue;
            }
            if catalog.contains(&s) {
                planned += 1;
            } else {
                r.fail("manifest", path.clone(), format!("authorized skill `{}` is neither packaged nor in skills/CATALOG.md", s));
            }
        }
        if planned > 0 {
            r.warn("manifest", path.clone(), format!("{} authorized skill(s) are `planned` (catalog only, no SKILL.md yet)", planned));
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
                if s.get(Value::String(k.into())).and_then(|v| v.as_sequence()).is_none() {
                    errs.push(format!("default_access_scope.{} must be a list", k));
                }
            }
            for g in seq_strs(s.get(Value::String("write".into()))) {
                if let Some(e) = write_glob_error(id, &g) {
                    errs.push(e);
                }
            }
        }
    }
    if fm.get(Value::String("authorized_skills".into())).and_then(|v| v.as_sequence()).is_none() {
        errs.push("authorized_skills must be a list".into());
    }
    errs
}

/// `[REV-SEC-02]` and `specs/001` §2.2: write globs are logical, zone-scoped, and never reach `.git`.
pub fn write_glob_error(role: &str, glob: &str) -> Option<String> {
    let bad = |m: &str| Some(format!("write glob `{}`: {}", glob, m));
    if glob.is_empty() || glob.starts_with('/') || glob.starts_with('~') || glob.contains('$') || glob.split('/').any(|s| s == "..") {
        return bad("must be a logical path (no absolute, home, variable or `..` paths)");
    }
    if glob.starts_with(".delphi") {
        return bad("physical `.delphi/` paths never appear in manifests");
    }
    if glob.split('/').any(|s| s == ".git" || s == "**" && false) || glob.split('/').any(|s| s.starts_with(".git") && s != ".github") {
        return bad("may never match a `.git` file or directory");
    }
    if glob.split('/').any(|s| s == ".github") {
        return bad("CI configuration (`.github/**`) is never agent-writable");
    }
    if glob == "**" || glob == "*" || glob.starts_with("**/") {
        return bad("unscoped glob");
    }
    if let Some(rest) = glob.strip_prefix("@worktree/") {
        return match role {
            "builder" => None,
            "writer" if rest == "README.md" || rest.starts_with("docs/") => None,
            _ => bad("only builder (and writer, for README.md and docs/**) may write `@worktree/**`"),
        };
    }
    if glob.starts_with("skills/") && !glob.starts_with("skills/custom/") {
        return bad("built-in skills are never agent-writable");
    }
    let allowed = ["missions/", "context/", "specs/", "agents/custom/", "skills/custom/"];
    if allowed.iter().any(|p| glob.starts_with(p)) {
        None
    } else {
        bad("not under a writable logical zone (missions/, context/, specs/, agents/custom/, skills/custom/, @worktree/)")
    }
}

/// `specs/001` §2.3 narrow-only rule: an override may not widen the built-in role's scope or skills,
/// except to attach a ratified custom/operator skill whose permissions fit the built-in scope (D-21).
#[cfg_attr(not(test), allow(dead_code))] // called by override loading in Step 2
pub fn override_widening(builtin: &Mapping, ovr: &Mapping, attachable: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut errs = Vec::new();
    let scope = |m: &Mapping, k: &str| -> BTreeSet<String> {
        m.get(Value::String("default_access_scope".into()))
            .and_then(|v| v.as_mapping())
            .map(|s| seq_strs(s.get(Value::String(k.into()))).into_iter().collect())
            .unwrap_or_default()
    };
    for k in ["read", "write", "execute"] {
        let base = scope(builtin, k);
        for g in scope(ovr, k) {
            if !base.contains(&g) && !(k == "read" && base.contains("*")) {
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
    let re = Regex::new(&format!(r"\b({})\b", alternation)).unwrap();
    for path in tree.with_ext(&[".md", ".toml"]) {
        if path.starts_with("themes/") || path.contains("/councils/") {
            continue;
        }
        let text = tree.read(path).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            let lower = line.to_lowercase();
            if lower.contains("theme") {
                continue; // explicit references to the theme (e.g. "the Greek default") are allowed
            }
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
            k.as_str() == "context/heuristics.md"
                || (k.starts_with("memory/agents/") && k.ends_with("/lessons.md"))
                || k.as_str() == "operator/heuristics.md"
        })
        .cloned()
        .collect();
    for path in files {
        let text = tree.read(&path).unwrap_or_default();
        for (lvl, msg) in cap_errors(&text) {
            r.push(lvl, "rule-caps", path.clone(), msg);
        }
    }
}

pub fn cap_errors(text: &str) -> Vec<(Level, String)> {
    let mut out = Vec::new();
    if text.len() > RULE_BYTES {
        out.push((Level::Fail, format!("{} bytes exceeds the {}-byte cap", text.len(), RULE_BYTES)));
    }
    let doc = match frontmatter::parse(text) {
        Ok(d) => d,
        Err(e) => return vec![(Level::Fail, e)],
    };
    let mut rules: Vec<(String, Vec<String>)> = Vec::new();
    let mut in_fence = false;
    for line in doc.body.lines() {
        if line.starts_with("```") {
            in_fence = !in_fence;
        }
        if in_fence {
            continue;
        }
        if let Some(h) = line.strip_prefix("### ") {
            rules.push((h.to_string(), Vec::new()));
        } else if line.starts_with("- ") {
            if let Some(last) = rules.last_mut() {
                last.1.push(line.to_string());
            }
        }
    }
    if rules.len() > RULE_COUNT {
        out.push((Level::Fail, format!("{} rules exceeds the Rule of {}", rules.len(), RULE_COUNT)));
    }
    if let Some(fm) = doc.frontmatter {
        if let Some(n) = fm.get(Value::String("active_rules".into())).and_then(|v| v.as_u64()) {
            if n as usize != rules.len() {
                out.push((Level::Fail, format!("frontmatter active_rules is {} but the file has {} rules", n, rules.len())));
            }
        }
    }
    for (head, lines) in &rules {
        let block = lines.join("\n");
        let chars = block.chars().count();
        if chars > RULE_CHARS {
            out.push((Level::Fail, format!("rule `{}` is {} characters (cap {})", head, chars, RULE_CHARS)));
        }
        for tag in ["**T:**", "**D:**", "**F:**"] {
            if !block.contains(tag) {
                out.push((Level::Fail, format!("rule `{}` lacks a {} line", head, tag)));
            }
        }
    }
    out
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
        let dir = match path.rfind('/') {
            Some(i) => &path[..i],
            None => "",
        };
        for (i, line) in text.lines().enumerate() {
            for c in re.captures_iter(line) {
                let target = &c[1];
                if target.contains("://") || target.starts_with("mailto:") {
                    continue;
                }
                let joined = if dir.is_empty() { target.to_string() } else { format!("{}/{}", dir, target) };
                let loc = format!("{}:{}", path, i + 1);
                match normalize(&joined) {
                    None => r.fail("links", loc, format!("link `{}` escapes the tree", target)),
                    Some(n) if tree.exists(&n) || n.is_empty() => {}
                    Some(n) => {
                        if tree.mode == Mode::Workspace && UNSEEDED.iter().any(|u| n.starts_with(u)) {
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

fn check_abs_paths(tree: &Tree, r: &mut Report) {
    let re = Regex::new(r"/(Users|home)/[A-Za-z0-9._-]+/").unwrap();
    for path in tree.with_ext(&[".md", ".toml"]) {
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

    #[test]
    fn caps() {
        let ok = "---\nactive_rules: 1\n---\n### `[H-1]` X\n- **T:** a\n- **D:** b\n- **F:** c\n";
        assert!(cap_errors(ok).is_empty());
        let long = ok.replace("- **D:** b", &format!("- **D:** {}", "x".repeat(400)));
        assert!(!cap_errors(&long).is_empty());
        let wrong_count = ok.replace("active_rules: 1", "active_rules: 2");
        assert!(!cap_errors(&wrong_count).is_empty());
    }

    // specs/006 §6 test 8: this source repository lints clean. In-progress council transcripts
    // (`*.raw.md`, converted before commit) are ignored here.
    #[test]
    fn source_repo_lints_clean() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut tree = Tree::from_repo(&root).unwrap();
        tree.files.retain(|k, _| !k.ends_with(".raw.md"));
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
