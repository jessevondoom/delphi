//! What changed between two versions of a bundle, for the update notice (`specs/024` §4.3): the top entry of
//! `CHANGELOG.md` and a per-section count from the two file manifests. Pure functions over text.

use std::collections::BTreeMap;

/// The sections the notice counts, in the order it prints them; every other file is "metadata" (`bundle.toml`, README, ...).
pub const SECTIONS: [&str; 6] = ["knowledge", "skills", "agents", "behavior", "evals", "vectors"];
const MAX_NOTICE_LINES: usize = 20;

/// The newest entry of a changelog: the first `## ` heading and what follows it up to the next one, at most 20 lines.
/// A changelog with no `## ` heading gives its first non-title lines instead, so a plain list still shows.
pub fn changelog_top(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.starts_with("## ")).unwrap_or_else(|| lines.iter().position(|l| !l.starts_with("# ") && !l.trim().is_empty()).unwrap_or(lines.len()));
    let mut top: Vec<String> = lines[start.min(lines.len())..]
        .iter()
        .enumerate()
        .take_while(|(i, l)| *i == 0 || !l.starts_with("## "))
        .map(|(_, l)| l.trim_end().to_string())
        .take(MAX_NOTICE_LINES)
        .collect();
    while top.last().is_some_and(|l| l.is_empty()) {
        top.pop();
    }
    top
}

/// Whether the version at `dir` is at least the smallest honest bump over the previous tree `prev` (`validate --against`,
/// `specs/023` §9), and the sentence that says so. `None` when either tree has no readable manifest. The update notice
/// reports this; it does not refuse the update, because the bundle's own `validate` is where a publisher is held to it.
pub fn version_bump(dir: &std::path::Path, prev: &std::path::Path, host: &dyn crate::Host) -> Option<(bool, String)> {
    let mut scratch = crate::Report::default();
    let tree = crate::tree::scan(dir, &mut scratch);
    let m = crate::manifest::check(&tree, &crate::Options::default(), &mut scratch)?;
    let mut r = crate::Report::default();
    crate::manifest::check_against(&tree, Some(&m), prev, host, &mut r);
    r.fails.first().map(|f| (false, f.msg.clone())).or_else(|| r.notes.first().map(|n| (true, n.msg.clone())))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
}

impl Counts {
    pub fn is_empty(&self) -> bool {
        self.added + self.changed + self.removed == 0
    }
}

fn hashes(files_json: &[u8]) -> BTreeMap<String, String> {
    let v: serde_json::Value = serde_json::from_slice(files_json).unwrap_or_default();
    v["files"].as_object().into_iter().flatten().filter_map(|(p, e)| Some((p.clone(), e["blake3"].as_str()?.to_string()))).collect()
}

fn section(path: &str) -> &'static str {
    let top = path.split('/').next().unwrap_or("");
    SECTIONS.iter().copied().find(|s| *s == top).unwrap_or("metadata")
}

/// Files added, changed and removed per section between two `files.json` texts. Sections with no change are left out.
pub fn section_changes(old_files_json: &[u8], new_files_json: &[u8]) -> BTreeMap<&'static str, Counts> {
    let (old, new) = (hashes(old_files_json), hashes(new_files_json));
    let mut out: BTreeMap<&'static str, Counts> = BTreeMap::new();
    for (p, h) in &new {
        let c = out.entry(section(p)).or_default();
        match old.get(p) {
            None => c.added += 1,
            Some(o) if o != h => c.changed += 1,
            Some(_) => {}
        }
    }
    for p in old.keys().filter(|p| !new.contains_key(*p)) {
        out.entry(section(p)).or_default().removed += 1;
    }
    out.retain(|_, c| !c.is_empty());
    out
}

/// `knowledge: 2 added, 1 changed`, `skills: 1 removed`: one line per changed section, in the notice's order.
pub fn render_sections(changes: &BTreeMap<&'static str, Counts>) -> Vec<String> {
    SECTIONS
        .iter()
        .chain(std::iter::once(&"metadata"))
        .filter_map(|s| {
            let c = changes.get(s)?;
            let parts: Vec<String> = [("added", c.added), ("changed", c.changed), ("removed", c.removed)].iter().filter(|(_, n)| *n > 0).map(|(w, n)| format!("{} {}", n, w)).collect();
            Some(format!("{}: {}", s, parts.join(", ")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fj(files: &[(&str, &str)]) -> Vec<u8> {
        let body: Vec<String> = files.iter().map(|(p, h)| format!("\"{p}\": {{\"blake3\": \"blake3:{h}\", \"bytes\": 1}}")).collect();
        format!("{{\"bundle\": 1, \"files\": {{{}}}}}", body.join(",")).into_bytes()
    }

    #[test]
    fn the_top_changelog_entry_is_the_first_section_only() {
        let t = "# Changelog\n\n## 1.5.0\n- Added a runbook.\n- Fixed a link.\n\n## 1.4.0\n- Older.\n";
        assert_eq!(changelog_top(t), vec!["## 1.5.0", "- Added a runbook.", "- Fixed a link."]);
        assert_eq!(changelog_top("# Changelog\n\n- first\n- second\n"), vec!["- first", "- second"]);
        assert!(changelog_top("").is_empty() && changelog_top("# Changelog\n").is_empty());
        let long = format!("## 2.0.0\n{}", "- x\n".repeat(50));
        assert_eq!(changelog_top(&long).len(), MAX_NOTICE_LINES);
    }

    #[test]
    fn sections_count_what_was_added_changed_and_removed() {
        let old = fj(&[("knowledge/a.md", "1"), ("knowledge/b.md", "2"), ("skills/s/SKILL.md", "3"), ("bundle.toml", "4")]);
        let new = fj(&[("knowledge/a.md", "1"), ("knowledge/b.md", "9"), ("knowledge/c.md", "5"), ("agents/x.md", "6"), ("bundle.toml", "7")]);
        let c = section_changes(&old, &new);
        assert_eq!(c["knowledge"], Counts { added: 1, changed: 1, removed: 0 });
        assert_eq!(c["skills"], Counts { added: 0, changed: 0, removed: 1 });
        assert_eq!(c["agents"], Counts { added: 1, changed: 0, removed: 0 });
        assert_eq!(c["metadata"], Counts { added: 0, changed: 1, removed: 0 });
        assert!(!c.contains_key("behavior"));
        assert_eq!(render_sections(&c), vec!["knowledge: 1 added, 1 changed", "skills: 1 removed", "agents: 1 added", "metadata: 1 changed"]);
        assert!(section_changes(&old, &old).is_empty());
        assert!(section_changes(b"not json", b"{}").is_empty());
    }
}
