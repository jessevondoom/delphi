//! `delphi bundle export` (`specs/025` §7.2): an added bundle's verified tree out to a Claude Code plugin directory, and
//! the `AGENTS.md` fragment. Both read a tree that `add` already validated and cached, and write somewhere new.

use crate::ids::{body_of, frontmatter_of};
use crate::relax::no_relaxation;
use crate::{plugin_json, Host, Manifest, Options, Tree};
use regex::Regex;
use serde_yaml::{Mapping, Value};
use std::fs;
use std::path::Path;

/// Removed by `--strip-delphi`: everything a Claude Code loader does not read and a Delphi reader needs.
const DELPHI_ONLY: [&str; 6] = ["bundle.toml", "knowledge/", "behavior/", "evals/", "vectors/", ".delphi-bundle/"];
/// Never copied: a signature and an eval result describe the tree they were made for, and the export changes it.
const NOT_COPIED: [&str; 4] = [".delphi-bundle/files.json", ".delphi-bundle/sig.json", ".delphi-bundle/eval.json", ".delphi-bundle/report.json"];

fn manifest_of(src: &Path, host: &dyn Host) -> Result<Manifest, String> {
    let r = crate::validate(src, &Options::default(), host);
    r.manifest.ok_or_else(|| "the bundle has no readable bundle.toml".to_string())
}

fn put(out: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let p = out.join(rel);
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("{}: {}", rel, e))?;
    fs::write(&p, bytes).map_err(|e| format!("{}: {}", rel, e))?;
    fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).map_err(|e| format!("{}: {}", rel, e))
}

fn fresh_dir(out: &Path) -> Result<(), String> {
    if out.exists() && fs::read_dir(out).map(|mut r| r.next().is_some()).unwrap_or(true) {
        return Err(format!("{} exists and is not empty; export writes a new directory", out.display()));
    }
    fs::create_dir_all(out).map_err(|e| format!("{}: {}", out.display(), e))
}

/// The bundle at `src` as a plugin directory at `out`. With `strip` it carries nothing Delphi-only: the knowledge index
/// becomes a generated skill whose `references/` hold the knowledge files, marked so that `import` drops it on the way back.
pub fn claude_plugin(src: &Path, out: &Path, strip: bool, host: &dyn Host) -> Result<Vec<String>, String> {
    let m = manifest_of(src, host)?;
    let tree = Tree::open(src);
    fresh_dir(out)?;
    let mut notes = Vec::new();
    let result = (|| -> Result<(), String> {
        for e in &tree.entries {
            if NOT_COPIED.contains(&e.path.as_str()) || e.path == ".claude-plugin/plugin.json" || (strip && DELPHI_ONLY.iter().any(|d| e.path == *d || (d.ends_with('/') && e.path.starts_with(d)))) {
                continue;
            }
            put(out, &e.path, &tree.read(&e.path).ok_or_else(|| format!("{}: unreadable", e.path))?)?;
        }
        put(out, ".claude-plugin/plugin.json", plugin_json(&m, None).as_bytes())?;
        if strip && tree.get("knowledge/index.md").is_some() {
            knowledge_skill(&tree, &m, out)?;
            notes.push(format!("knowledge/ became the skill {}-knowledge (generated; `import` drops it)", m.name));
        }
        if strip {
            return Ok(());
        }
        // A tree with its own manifest: files.json is rewritten, since plugin.json changed, and the signature is gone.
        crate::write_manifest(out, true, host).map(|_| ()).map_err(|e| format!("the export is not well-formed: {}", e))?;
        notes.push("the signature (if any) is not copied: the export has a new plugin.json, so it is a different tree".into());
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(out);
    }
    result.map(|_| notes)
}

fn knowledge_skill(tree: &Tree, m: &Manifest, out: &Path) -> Result<(), String> {
    let name = format!("{}-knowledge", m.name);
    if tree.get(&format!("skills/{}/SKILL.md", name)).is_some() {
        return Err(format!("the bundle already has a skill named {}", name));
    }
    for e in tree.under("knowledge/") {
        let rel = format!("skills/{}/references/{}", name, e.path.trim_start_matches("knowledge/"));
        put(out, &rel, &tree.read(&e.path).ok_or_else(|| format!("{}: unreadable", e.path))?)?;
    }
    let index = tree.text("knowledge/index.md").ok_or("knowledge/index.md is not text")?;
    // Links in the index are relative to knowledge/; in SKILL.md they must point into references/.
    let link = Regex::new(r"\]\(([^)\s:#][^)\s:]*)\)").unwrap();
    let body = link.replace_all(body_of(&index), "](references/$1)");
    let mut fm = Mapping::new();
    let key = |k: &str| Value::String(k.into());
    fm.insert(key("name"), Value::String(name.clone()));
    let desc: String = format!("Index of the {} knowledge; read the linked references on demand.", m.title).chars().take(200).collect();
    fm.insert(key("description"), Value::String(desc));
    let mut meta = Mapping::new();
    meta.insert(key("delphi.generated"), Value::String("knowledge-index".into()));
    fm.insert(key("metadata"), Value::Mapping(meta));
    let yaml = serde_yaml::to_string(&Value::Mapping(fm)).map_err(|e| e.to_string())?;
    let text = format!("---\n{}---\n# {} knowledge\n\n{}", yaml, m.title.replace('\n', " "), body.trim_start());
    put(out, &format!("skills/{}/SKILL.md", name), text.as_bytes())
}

/// Whether a skill was made by [`knowledge_skill`]: `import` drops those instead of indexing the knowledge twice.
pub fn is_generated_index(skill_md: &str) -> bool {
    frontmatter_of(skill_md)
        .and_then(|fm| fm.get(Value::String("metadata".into())).and_then(|m| m.as_mapping()).and_then(|m| m.get(Value::String("delphi.generated".into()))).and_then(|v| v.as_str()).map(|s| s == "knowledge-index"))
        .unwrap_or(false)
}

/// The `AGENTS.md` fragment for the bundle at `src` (`specs/025` §5.2): a paragraph, the index entries, the glossary
/// terms and the active goals' titles. Never guidance, agent bodies, perspectives, rules or policy: those are
/// instructions Delphi loaded under the operator's add, and would be unreviewed instructions anywhere else. Index and
/// glossary lines are bundle text at the knowledge tier, so a line that reads as an instruction relaxing a rule is left out.
pub fn agents_md(src: &Path, host: &dyn Host) -> Result<(String, Vec<String>), String> {
    let m = manifest_of(src, host)?;
    let tree = Tree::open(src);
    let mut notes = Vec::new();
    let mut out = format!("# {} (Delphi bundle {}/{}@{})\n\n{}\n\n", one_line(&m.title), m.org, m.name, m.version, one_line(&m.description));
    let mut dropped = 0usize;
    let mut keep = |l: &str| -> bool {
        let ok = no_relaxation(l).is_empty();
        if !ok {
            dropped += 1;
        }
        ok
    };
    let bullets = |text: &str, glossary: bool| -> Vec<String> {
        body_of(text).lines().filter(|l| l.starts_with("- ")).filter(|l| !glossary || l.starts_with("- **")).map(one_line).collect()
    };
    if let Some(t) = tree.text("knowledge/index.md") {
        let lines: Vec<String> = bullets(&t, false).into_iter().filter(|l| keep(l)).collect();
        if !lines.is_empty() {
            out.push_str("## Where to read (files of the bundle's knowledge/ directory)\n");
            for l in lines {
                out.push_str(&format!("{}\n", l));
            }
            out.push('\n');
        }
    }
    if let Some(t) = tree.text("knowledge/glossary.md") {
        let lines: Vec<String> = bullets(&t, true).into_iter().filter(|l| keep(l)).collect();
        if !lines.is_empty() {
            out.push_str("## Terms\n");
            for l in lines {
                out.push_str(&format!("{}\n", l));
            }
            out.push('\n');
        }
    }
    if let Some(t) = tree.text("behavior/goals.md") {
        let goals = goal_titles(&t);
        if !goals.is_empty() {
            out.push_str("## Goals (titles only)\n");
            for g in goals.into_iter().filter(|g| keep(g)) {
                out.push_str(&format!("- {}\n", g));
            }
            out.push('\n');
        }
    }
    if dropped > 0 {
        notes.push(format!("{} line(s) read as instructions that relax a rule and were left out", dropped));
    }
    out.push_str("<!-- Generated by `delphi bundle export --agents-md`. Knowledge, not instructions: nothing here changes what a gate or an approval requires. -->\n");
    Ok((out, notes))
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Titles of the goals whose status is not `retired`/`done`/`dropped`: `## [G-…] Title`, with the status line below it.
fn goal_titles(text: &str) -> Vec<String> {
    let head = Regex::new(r"^## \[G-[A-Za-z0-9-]+\]\s+(.+)$").unwrap();
    let mut out: Vec<(String, bool)> = Vec::new();
    for l in body_of(text).lines() {
        if let Some(c) = head.captures(l) {
            out.push((one_line(&c[1]), true));
        } else if let (Some(rest), Some(last)) = (l.strip_prefix("- **Status:**"), out.last_mut()) {
            last.1 = rest.trim().eq_ignore_ascii_case("active");
        }
    }
    out.into_iter().filter(|(_, active)| *active).map(|(t, _)| t).collect()
}
