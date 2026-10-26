//! The alignment pre-pass (`specs/026` §5): does a spec or a review cite goals, perspectives and decisions of the added
//! bundles that still stand?
//!
//! Deterministic and free: no model reads anything here. It extracts the ids a text cites (`[G-acme-01]`, `[P-acme-03]`,
//! `knowledge/decisions/0042-…`, "decision 0042") and checks each against the added bundles, every file proven against its
//! recorded hash first. A finding is advice to the reader: it is `P2`, never `P0`, and nothing in the daemon gates on it.
//! The strategist's judgment of whether a change serves each goal is a separate, model-written table (§5); this is the part
//! that needs no judgment.

use crate::bundle_load::read;
use crate::workspace::WorkspacePaths;
use delphi_bundle::load;
use delphi_core::bundles::{self, Entry};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

/// The most findings one text gets.
pub const MAX_FINDINGS: usize = 50;

/// One thing the text cites that does not stand.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// `ALIGN-REF-01` superseded or withdrawn, `-02` not defined by any added bundle, `-03` past its review or horizon.
    pub code: &'static str,
    /// Always `P2` here: alignment findings advise, they never block (§5).
    pub level: &'static str,
    pub id: String,
    pub message: String,
}

impl Finding {
    fn new(code: &'static str, id: &str, message: String) -> Finding {
        Finding { code, level: "P2", id: id.to_string(), message }
    }

    /// `ALIGN-REF-01 (P2) [G-acme-04]: …`, the line a card and a review show.
    pub fn line(&self) -> String {
        format!("{} ({}) {}: {}", self.code, self.level, self.id, self.message)
    }
}

/// The ids a text cites: goal and perspective ids, and decisions by number (the path form or "decision 0042").
pub fn cited(text: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let ids = Regex::new(r"\b([GP]-[a-z0-9]+(?:-[a-z0-9]+)*-\d{2,})\b").unwrap();
    let paths = Regex::new(r"decisions/(\d{4})-[a-z0-9][a-z0-9-]*").unwrap();
    let words = Regex::new(r"(?i)\bdecisions?\s+(\d{4})\b").unwrap();
    let ids_found = ids.captures_iter(text).map(|c| c[1].to_string()).collect();
    let numbers = paths.captures_iter(text).map(|c| c[1].to_string()).chain(words.captures_iter(text).map(|c| c[1].to_string())).collect();
    (ids_found, numbers)
}

/// What the added bundles define, read once.
struct Known {
    /// goal id to (status, horizon, bundle)
    goals: BTreeMap<String, (String, String, String)>,
    perspectives: BTreeSet<String>,
    /// decision number to (bundle, cached path relative to the bundle root)
    decisions: BTreeMap<String, (Entry, String)>,
}

fn known(ws: &WorkspacePaths, notes: &mut Vec<String>) -> Known {
    let mut k = Known { goals: BTreeMap::new(), perspectives: BTreeSet::new(), decisions: BTreeMap::new() };
    for e in bundles::active_entries(&ws.dir) {
        if let Some(t) = read(ws, &e, "behavior/goals.md", "goals", notes) {
            for g in load::goals(&t) {
                k.goals.insert(g.id, (g.status, g.horizon, e.id()));
            }
        }
        if let Some(t) = read(ws, &e, "behavior/perspectives.md", "perspectives", notes) {
            k.perspectives.extend(load::perspectives(&t).into_iter().map(|p| p.id));
        }
        // Decisions are found by their file names in the bundle's own manifest, which is proven against the locked hash.
        let cache = bundles::cache_dir(&ws.home, &e.org, &e.name, &e.hash);
        let Ok(manifest) = std::fs::read(cache.join(".delphi-bundle/files.json")) else { continue };
        if delphi_bundle::bundle_hash(&manifest) != e.hash {
            notes.push(format!("bundle {}: decisions not checked: its cache entry no longer matches the hash recorded when it was added", e.id()));
            continue;
        }
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&manifest) else { continue };
        for path in json["files"].as_object().into_iter().flatten().map(|(p, _)| p) {
            if let Some(num) = path.strip_prefix("knowledge/decisions/").and_then(|n| n.get(..4)).filter(|n| n.chars().all(|c| c.is_ascii_digit())) {
                if path.ends_with(".md") {
                    k.decisions.insert(num.to_string(), (e.clone(), path.clone()));
                }
            }
        }
    }
    k
}

/// `YYYY-Qn` or a date at the end of a horizon, as the last day it covers.
fn horizon_end(h: &str) -> Option<String> {
    let last = h.split_whitespace().last()?;
    if let Some((y, q)) = last.split_once("-Q") {
        let day = match q {
            "1" => "03-31",
            "2" => "06-30",
            "3" => "09-30",
            "4" => "12-31",
            _ => return None,
        };
        return (y.len() == 4 && y.bytes().all(|c| c.is_ascii_digit())).then(|| format!("{}-{}", y, day));
    }
    (last.len() == 10 && last.as_bytes()[4] == b'-' && last.as_bytes()[7] == b'-').then(|| last.to_string())
}

/// Check what `text` cites against the added bundles. `today` is `YYYY-MM-DD`. Returns the findings (sorted, at most
/// [`MAX_FINDINGS`]) and notes about bundle files that could not be used.
pub fn prepass(ws: &WorkspacePaths, text: &str, today: &str) -> (Vec<Finding>, Vec<String>) {
    let (ids, numbers) = cited(text);
    let mut notes = Vec::new();
    if ids.is_empty() && numbers.is_empty() {
        return (Vec::new(), notes);
    }
    let k = known(ws, &mut notes);
    let mut out = BTreeSet::new();
    for id in &ids {
        if id.starts_with("G-") {
            match k.goals.get(id) {
                None => {
                    out.insert(Finding::new("ALIGN-REF-02", id, "no added bundle defines this goal".into()));
                }
                Some((status, horizon, bundle)) => match status.as_str() {
                    "superseded" | "dropped" => {
                        out.insert(Finding::new("ALIGN-REF-01", id, format!("this goal is {} in {}", status, bundle)));
                    }
                    "met" => {
                        out.insert(Finding::new("ALIGN-REF-03", id, format!("this goal is already met in {}", bundle)));
                    }
                    _ => {
                        if horizon_end(horizon).is_some_and(|end| end.as_str() < today) {
                            out.insert(Finding::new("ALIGN-REF-03", id, format!("this goal is past its horizon ({}) and still active in {}", horizon, bundle)));
                        }
                    }
                },
            }
        } else if !k.perspectives.contains(id) {
            out.insert(Finding::new("ALIGN-REF-02", id, "no added bundle defines this perspective".into()));
        }
    }
    for n in &numbers {
        let id = format!("decision {}", n);
        let Some((e, path)) = k.decisions.get(n) else {
            out.insert(Finding::new("ALIGN-REF-02", &id, "no added bundle has a decision with this number".into()));
            continue;
        };
        let Some(doc) = read(ws, e, path, "decision", &mut notes) else { continue };
        let fm = delphi_bundle::load::frontmatter(&doc);
        let field = |k: &str| fm.as_ref().and_then(|m| m.get(k)).cloned().unwrap_or_default();
        if field("status") == "superseded" {
            let by = field("superseded_by");
            out.insert(Finding::new("ALIGN-REF-01", &id, format!("superseded{} in {}", if by.is_empty() { String::new() } else { format!(" by {}", by) }, e.id())));
        } else if !field("review_by").is_empty() && field("review_by").as_str() < today {
            out.insert(Finding::new("ALIGN-REF-03", &id, format!("past its review date ({}) in {}", field("review_by"), e.id())));
        }
    }
    (out.into_iter().take(MAX_FINDINGS).collect(), notes)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bundle_load::tests::{add_bundle, FM};
    use crate::workspace::tests::make_workspace;

    pub(crate) const GOALS: &str = "## [G-acme-01] Cut p99 latency\n- **Horizon:** 2026-Q4 to 2999-Q1\n- **Measure:** p99 weekly\n- **Status:** active\n\n## [G-acme-02] Old goal\n- **Horizon:** 2026-Q1\n- **Status:** superseded\n\n## [G-acme-03] Late goal\n- **Horizon:** 2025-Q1 to 2026-Q1\n- **Status:** active\n\n## [G-acme-04] Done goal\n- **Horizon:** 2026-Q1\n- **Status:** met\n";
    pub(crate) const PERSPECTIVES: &str = "## [P-acme-03] Boring technology\n- **Statement:** choose the well-understood option.\n- **Apply when:** picking a datastore.\n- **Counter-case:** it cannot meet a goal.\n\n## [P-acme-05] Every change has a way back\n- **Statement:** ship the rollback with the change.\n- **Apply when:** a change touches data.\n- **Counter-case:** the rollback costs more than the failure.\n";

    /// A decision file: the standard provenance frontmatter with this status and review date, plus `extra` lines.
    fn decision(status: &str, review_by: &str, extra: &str) -> String {
        format!(
            "---\nid: acme-platform/decisions/x\ntitle: D\nauthor: operator\ntimestamp: \"2026-09-12T10:00:00Z\"\nsource_type: internal_generated\nconfidence: medium\nstatus: {}\nreview_by: \"{}\"\n{}source_mission: acme\ntags: [a]\n---\n## Decision\nx\n",
            status, review_by, extra
        )
    }

    pub(crate) fn fixture(ws: &WorkspacePaths) {
        let live = decision("active", "2999-01-01", "");
        let superseded = decision("superseded", "2999-01-01", "superseded_by: 0057\n");
        let stale = decision("active", "2025-01-01", "");
        add_bundle(
            ws,
            "acme",
            "platform",
            10,
            "2026-10-01",
            &[
                ("behavior/goals.md", &format!("{}{}", FM, GOALS)),
                ("behavior/perspectives.md", &format!("{}{}", FM, PERSPECTIVES)),
                ("knowledge/decisions/0031-latency-budget.md", &stale),
                ("knowledge/decisions/0042-postgres-over-dynamo.md", &superseded),
                ("knowledge/decisions/0057-next.md", &live),
            ],
        );
    }

    #[test]
    fn citations_are_found_in_every_form_the_bundle_standard_uses() {
        let (ids, nums) = cited("Serves [G-acme-01] and G-acme-12; honors [P-acme-03]. See knowledge/decisions/0042-postgres-over-dynamo.md, decisions/0031-x and Decision 0057. Not an id: G-1, [T-acme-02], 12345.");
        assert_eq!(ids.into_iter().collect::<Vec<_>>(), ["G-acme-01", "G-acme-12", "P-acme-03"]);
        assert_eq!(nums.into_iter().collect::<Vec<_>>(), ["0031", "0042", "0057"]);
        assert_eq!(cited("nothing cited here"), (BTreeSet::new(), BTreeSet::new()));
    }

    #[test]
    fn a_text_citing_what_no_longer_stands_gets_p2_findings_and_one_citing_what_does_gets_none() {
        let t = tempfile::tempdir().unwrap();
        let ws = make_workspace(t.path());
        fixture(&ws);
        let (none, _) = prepass(&ws, "This spec serves [G-acme-01] and honors [P-acme-05]; it follows decision 0057.", "2026-10-26");
        assert!(none.is_empty(), "{:?}", none);
        let text = "Serves [G-acme-01], [G-acme-02], [G-acme-03], [G-acme-04], [G-acme-99] and [P-acme-09]. Uses knowledge/decisions/0042-postgres-over-dynamo.md and decision 0031; also decision 0999.";
        let (f, notes) = prepass(&ws, text, "2026-10-26");
        let lines: Vec<String> = f.iter().map(|x| x.line()).collect();
        assert!(notes.is_empty(), "{:?}", notes);
        assert!(f.iter().all(|x| x.level == "P2"), "alignment never rises above P2 here");
        for want in [
            "ALIGN-REF-01 (P2) G-acme-02: this goal is superseded in acme/platform",
            "ALIGN-REF-01 (P2) decision 0042: superseded by 0057 in acme/platform",
            "ALIGN-REF-02 (P2) G-acme-99: no added bundle defines this goal",
            "ALIGN-REF-02 (P2) P-acme-09: no added bundle defines this perspective",
            "ALIGN-REF-02 (P2) decision 0999: no added bundle has a decision with this number",
            "ALIGN-REF-03 (P2) G-acme-03: this goal is past its horizon (2025-Q1 to 2026-Q1) and still active in acme/platform",
            "ALIGN-REF-03 (P2) G-acme-04: this goal is already met in acme/platform",
            "ALIGN-REF-03 (P2) decision 0031: past its review date (2025-01-01) in acme/platform",
        ] {
            assert!(lines.iter().any(|l| l == want), "`{}` missing from:\n{}", want, lines.join("\n"));
        }
        assert_eq!(f.len(), 8, "{:#?}", lines);
        assert!(!lines.iter().any(|l| l.contains("G-acme-01")), "a live goal is not a finding");
        assert_eq!(prepass(&ws, text, "2026-10-26").0, f, "deterministic");
    }

    #[test]
    fn with_no_bundle_every_citation_is_unknown_and_a_changed_cache_file_is_a_note_not_a_verdict() {
        let t = tempfile::tempdir().unwrap();
        let ws = make_workspace(t.path());
        let (f, _) = prepass(&ws, "Serves [G-acme-01].", "2026-10-26");
        assert_eq!(f.iter().map(|x| x.code).collect::<Vec<_>>(), ["ALIGN-REF-02"]);
        assert_eq!(prepass(&ws, "no ids", "2026-10-26").0, vec![]);
        fixture(&ws);
        let e = delphi_core::bundles::active_entries(&ws.dir).remove(0);
        let goals = delphi_core::bundles::cache_dir(&ws.home, "acme", "platform", &e.hash).join("behavior/goals.md");
        std::fs::write(&goals, "tampered").unwrap();
        let (f, notes) = prepass(&ws, "Serves [G-acme-01].", "2026-10-26");
        assert!(notes.iter().any(|n| n.contains("changed after the bundle was added")), "{:?}", notes);
        assert_eq!(f.iter().map(|x| x.code).collect::<Vec<_>>(), ["ALIGN-REF-02"], "a goal that cannot be read is unknown, never assumed live");
    }
}
