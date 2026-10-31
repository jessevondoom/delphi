//! The weekly digest of research that is waiting outside the knowledge base (`specs/058` §17, `specs/040` §26).
//!
//! Research held in the default scope's pending folder, and research a project keeps that has not been ratified into the shared
//! base, is counted here. When enough has piled up and a week has passed since the last nudge, the orchestrator says so once, in
//! the chat. It is a message, never a card: nothing is asked, nothing is approved or added, and counting reads only file names
//! and the front matter the research job wrote.

use crate::research::DIR;
use crate::research_pending::{Held, list};
use std::path::Path;

/// Fewer waiting pages than this are not worth a message.
pub const THRESHOLD: usize = 5;
/// Seconds between two nudges.
pub const EVERY_SECS: u64 = 7 * 24 * 60 * 60;
const STATE: &str = "local/digest.json";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Digest {
    /// Pages held in the default scope.
    pub held: usize,
    /// `(project name, pages not yet ratified into the shared base)`, projects with none left out.
    pub projects: Vec<(String, usize)>,
    /// Of all of them, the pages from weak sources (they would be labelled, never counted as corroboration).
    pub weak: usize,
}

impl Digest {
    pub fn total(&self) -> usize {
        self.held + self.projects.iter().map(|(_, n)| n).sum::<usize>()
    }
}

/// What is waiting now. A project page that already sits in the shared base (ratified) is not waiting.
pub fn collect(ws: &Path) -> Digest {
    let mut d = Digest::default();
    let pending = list(ws, &Held::Pending);
    d.held = pending.len();
    d.weak = pending.iter().filter(|i| i.class == "weak").count();
    for (name, root) in crate::scope::project_list(ws) {
        let waiting: Vec<_> = list(ws, &Held::Project(root)).into_iter().filter(|i| !ws.join(DIR).join(&i.rel).exists()).collect();
        if !waiting.is_empty() {
            d.weak += waiting.iter().filter(|i| i.class == "weak").count();
            d.projects.push((name, waiting.len()));
        }
    }
    d
}

fn last_nudge(ws: &Path) -> Option<u64> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(ws.join(STATE)).ok()?).ok()?;
    v["last_unix"].as_u64()
}

/// Is a nudge due: enough is waiting, and none was made in the last week (a clock set back counts as a nudge just made).
pub fn due(ws: &Path, d: &Digest, now: u64) -> bool {
    d.total() >= THRESHOLD && last_nudge(ws).is_none_or(|t| now >= t && now - t >= EVERY_SECS)
}

/// Record that a nudge was made now.
pub fn mark(ws: &Path, now: u64) {
    let _ = std::fs::write(ws.join(STATE), serde_json::json!({"schema": 1, "last_unix": now}).to_string());
}

/// The message. Names come from directory names, so they are shown as data.
pub fn text(d: &Digest) -> String {
    let mut parts = Vec::new();
    if d.held > 0 {
        parts.push(format!("{} held in the default scope", d.held));
    }
    for (name, n) in &d.projects {
        parts.push(format!("{} in project {}", n, crate::observe::sanitize(name)));
    }
    let weak = if d.weak > 0 { format!(" {} come from weak sources and would be labelled unverified.", d.weak) } else { String::new() };
    format!(
        "{} research pages are waiting outside the knowledge base: {}.{} /validate lists them (/validate project=<name> for a project); nothing is added until you ratify it.",
        d.total(),
        parts.join(", "),
        weak
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(root: &Path, rel: &str, class: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("---\ntitle: \"T\"\nsource_url: \"https://example.org/x\"\nsource_class: {}\nretrieved: 2026-10-31\n---\n\n# T\n", class)).unwrap();
    }

    #[test]
    fn a_nudge_needs_enough_waiting_pages_and_a_week_since_the_last_and_ratified_pages_do_not_count() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path().join("ws");
        let proj = std::fs::canonicalize(t.path()).unwrap().join("proj");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(ws.join("local/projects.json"), serde_json::json!({"schema": 1, "projects": {"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa": {"project_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "root": proj, "display_name": "proj"}}}).to_string()).unwrap();
        for n in 0..2 {
            page(&ws, &format!("{}/blog.example/p{}.md", crate::research::PENDING_DIR, n), "weak");
        }
        for n in 0..3 {
            page(&proj, &format!("research/web/site.example/q{}.md", n), "primary");
        }
        page(&proj, "research/web/site.example/done.md", "primary");
        page(&ws, &format!("{}/site.example/done.md", DIR), "primary");
        let d = collect(&ws);
        assert_eq!((d.held, d.weak, d.projects.clone()), (2, 2, vec![("proj".to_string(), 3)]), "a ratified project page is not waiting");
        assert!(due(&ws, &d, 1_000_000), "five waiting and never nudged");
        let text = text(&d);
        assert!(text.contains("5 research pages") && text.contains("2 held in the default scope") && text.contains("3 in project proj") && text.contains("2 come from weak sources"), "{text}");
        mark(&ws, 1_000_000);
        assert!(!due(&ws, &d, 1_000_000 + EVERY_SECS - 1), "not twice in a week");
        assert!(due(&ws, &d, 1_000_000 + EVERY_SECS));
        assert!(!due(&ws, &d, 10), "a clock set back is not a new week");
        let few = Digest { held: THRESHOLD - 1, ..Digest::default() };
        assert!(!due(&ws, &few, 9_000_000_000), "below the threshold nothing is said");
    }
}
