//! `delphi bundle report <name>` (`specs/027` §7.3): aggregates of what the event logs say about one added bundle.
//!
//! Counts only. It never prints a span's text, a prompt or a tool result; the one thing taken from a payload besides a
//! number is a bundle file's logical path and a finding's id, both cleaned. Every figure is correlational and gates
//! nothing (OD-7), and the report says so.

use delphi_core::event::EventEnvelope;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A knowledge file never cited in this many accepted tasks is a candidate to shorten or drop (`specs/027` §7.3).
pub const CANDIDATE_AFTER: usize = 50;

#[derive(Debug, Default)]
pub struct Agg {
    /// Tasks that were accepted at all (a card answered yes, or a merge approved), in the logs read.
    pub accepted_tasks: usize,
    /// Tasks in which a file of this bundle was in context and the task was accepted.
    pub cited_tasks: usize,
    /// File -> number of accepted tasks it was in context for.
    pub per_file: BTreeMap<String, usize>,
    /// (finding id, level, code) -> count. Workspace-wide: findings are not attributed to one bundle.
    pub findings: BTreeMap<(String, String, String), usize>,
    pub context_builds: usize,
    pub context_trims: usize,
    pub vectors_percent: Option<u64>,
    pub vectors_rejected: usize,
}

fn clean(s: &str, n: usize) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || "-_./@:+".contains(*c)).take(n).collect()
}

/// Aggregate the events of any number of missions for the bundle `org/name`.
pub fn aggregate(missions: &[Vec<EventEnvelope>], org: &str, name: &str) -> Agg {
    let id = format!("{}/{}", org, name);
    let mut a = Agg::default();
    for events in missions {
        let mut accepted: BTreeSet<&str> = BTreeSet::new();
        let mut cited: BTreeSet<&str> = BTreeSet::new();
        let mut files: BTreeSet<(&str, String)> = BTreeSet::new();
        for e in events {
            let p = &e.payload;
            let task = e.task_id.as_deref().unwrap_or("");
            match e.kind.as_str() {
                "approval.resolved" if p["resolution"] == "approved" && !task.is_empty() => {
                    accepted.insert(task);
                }
                "merge.approved" if !task.is_empty() => {
                    accepted.insert(task);
                }
                "bundle.cited" if p["org"] == org && p["name"] == name => {
                    cited.insert(task);
                    for f in p["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
                        files.insert((task, clean(f, 200)));
                    }
                }
                "align.finding" => {
                    for f in p["findings"].as_array().into_iter().flatten() {
                        let key = (clean(f["id"].as_str().unwrap_or("?"), 80), clean(f["level"].as_str().unwrap_or("?"), 8), clean(f["code"].as_str().unwrap_or("?"), 20));
                        *a.findings.entry(key).or_default() += 1;
                    }
                }
                "org_context.built" => {
                    a.context_builds += 1;
                    if p["trimmed"].as_array().is_some_and(|t| t.iter().any(|x| x.as_str() == Some(id.as_str()))) {
                        a.context_trims += 1;
                    }
                }
                "bundle.vectors_progress" if p["org"] == org && p["name"] == name => a.vectors_percent = p["percent"].as_u64(),
                "bundle.vectors_rejected" if p["org"] == org && p["name"] == name => a.vectors_rejected += 1,
                _ => {}
            }
        }
        a.accepted_tasks += accepted.len();
        a.cited_tasks += cited.iter().filter(|t| accepted.contains(*t)).count();
        for (_, f) in files {
            *a.per_file.entry(f).or_default() += 1;
        }
    }
    a
}

/// What the report prints. `knowledge` is the bundle's own knowledge files, so a never-cited one can be named.
pub fn render(id: &str, version: &str, a: &Agg, knowledge: &[String], stale: bool, disputes: Option<usize>, eval_line: Option<&str>) -> String {
    let mut o = format!("{}@{}{}\n", id, version, if stale { "  STALE (past its review date)" } else { "" });
    o.push_str(&format!("  accepted tasks in these logs: {}; in {} of them a file of this bundle was in context\n", a.accepted_tasks, a.cited_tasks));
    let mut cited: Vec<(&String, &usize)> = a.per_file.iter().collect();
    cited.sort_by(|x, y| y.1.cmp(x.1).then(x.0.cmp(y.0)));
    if cited.is_empty() {
        o.push_str("  no file of this bundle has been cited yet\n");
    } else {
        o.push_str("  cited files (accepted tasks):\n");
        for (f, n) in cited.iter().take(20) {
            o.push_str(&format!("    {:>4}  {}\n", n, f));
        }
    }
    let never: Vec<&String> = knowledge.iter().filter(|k| !a.per_file.keys().any(|f| f == *k || f.ends_with(k.as_str()))).collect();
    if !never.is_empty() {
        if a.accepted_tasks >= CANDIDATE_AFTER {
            o.push_str(&format!("  never cited in {} accepted tasks (candidates to shorten or drop): {}\n", a.accepted_tasks, never.iter().take(20).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
        } else {
            o.push_str(&format!("  {} knowledge file(s) not cited yet (too few accepted tasks to call any a candidate: {} of {})\n", never.len(), a.accepted_tasks, CANDIDATE_AFTER));
        }
    }
    if a.findings.is_empty() {
        o.push_str("  alignment findings: none recorded\n");
    } else {
        o.push_str("  alignment findings (workspace-wide, by goal or decision id and level):\n");
        for ((fid, level, code), n) in &a.findings {
            o.push_str(&format!("    {:>4}  {}  {}  {}\n", n, fid, level, code));
        }
    }
    o.push_str(&format!("  org context: built {} time(s), trimmed this bundle {} time(s)\n", a.context_builds, a.context_trims));
    match a.vectors_percent {
        Some(p) => o.push_str(&format!("  vectors: {}% ready at last report; {} shipped cache(s) rejected\n", p, a.vectors_rejected)),
        None => o.push_str(&format!("  vectors: no progress event in these logs; {} shipped cache(s) rejected\n", a.vectors_rejected)),
    }
    if let Some(n) = disputes {
        o.push_str(&format!("  disputes open: {}\n", n));
    }
    if let Some(l) = eval_line {
        o.push_str(&format!("  {}\n", l));
    }
    o.push_str("  These are correlational counts from event logs, never from a model's claim. They gate nothing and prove nothing about usefulness.\n  Endorsement latency for store bundles needs the xenia bundle record (slice B8) and is not shown.\n");
    o
}

/// The event logs of every mission in a workspace, oldest mission first.
pub fn read_missions(ws: &Path) -> Vec<Vec<EventEnvelope>> {
    let Ok(rd) = std::fs::read_dir(ws.join("local/missions")) else { return Vec::new() };
    let mut dirs: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.path()).collect();
    dirs.sort();
    dirs.iter().filter_map(|d| delphi_core::log::replay(d).ok()).collect()
}

/// `delphi bundle report <name>`.
pub fn report(home: &Path, ws: &Path, name: &str) -> i32 {
    let active = delphi_core::bundles::active_entries(ws);
    let found: Vec<_> = active.iter().filter(|e| e.name == name || e.id() == name).collect();
    let e = match found.as_slice() {
        [e] => *e,
        [] => {
            eprintln!("delphi bundle: no added bundle named `{}`", name);
            return 1;
        }
        many => {
            eprintln!("delphi bundle: `{}` is ambiguous ({}); use org/name", name, many.iter().map(|e| e.id()).collect::<Vec<_>>().join(", "));
            return 1;
        }
    };
    let agg = aggregate(&read_missions(ws), &e.org, &e.name);
    let cache = delphi_core::bundles::cache_dir(home, &e.org, &e.name, &e.hash);
    let mut knowledge = Vec::new();
    walk(&cache.join("knowledge"), "knowledge", &mut knowledge);
    knowledge.sort();
    let eval = std::fs::read_to_string(cache.join(delphi_engine::bundle_eval::RECORD)).ok().and_then(|t| delphi_engine::bundle_eval::summary_line(&t));
    print!("{}", render(&e.id(), &e.version, &agg, &knowledge, e.is_stale(&delphi_core::bundles::today()), None, eval.as_deref()));
    0
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let (n, Ok(t)) = (e.file_name().to_string_lossy().to_string(), e.file_type()) else { continue };
        if t.is_dir() {
            walk(&e.path(), &format!("{}/{}", prefix, n), out);
        } else if t.is_file() && n.ends_with(".md") {
            out.push(format!("{}/{}", prefix, n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(kind: &str, task: &str, payload: serde_json::Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m_1", kind, payload);
        if !task.is_empty() {
            e.task_id = Some(task.to_string());
        }
        e
    }

    fn log() -> Vec<EventEnvelope> {
        vec![
            ev("task.output", "t1", json!({"text": "CANARY-SECRET-TEXT the model wrote this"})),
            ev("tool.result", "t1", json!({"content": "CANARY-SECRET-TEXT from a file", "is_error": false})),
            ev("approval.resolved", "t1", json!({"resolution": "approved"})),
            ev("bundle.cited", "t1", json!({"org": "acme", "name": "plat", "files": ["knowledge/a.md", "knowledge/b.md"], "accepted_by": "approval"})),
            ev("merge.approved", "t2", json!({"sha": "abc"})),
            ev("bundle.cited", "t2", json!({"org": "acme", "name": "plat", "files": ["knowledge/a.md"], "accepted_by": "merge"})),
            ev("bundle.cited", "t2", json!({"org": "other", "name": "x", "files": ["knowledge/z.md"], "accepted_by": "merge"})),
            ev("approval.resolved", "t3", json!({"resolution": "denied"})),
            ev("align.finding", "t3", json!({"path": "specs/x.md", "findings": [{"code": "ALIGN-REF-01", "level": "P2", "id": "G-acme-01"}, {"code": "ALIGN-REF-03", "level": "P2", "id": "G-acme-01"}]})),
            ev("org_context.built", "t3", json!({"bundles": 2, "bytes": 3000, "trimmed": ["acme/plat"], "notes": []})),
            ev("org_context.built", "t4", json!({"bundles": 2, "bytes": 3000, "trimmed": [], "notes": []})),
            ev("bundle.vectors_progress", "", json!({"org": "acme", "name": "plat", "percent": 40})),
            ev("bundle.vectors_progress", "", json!({"org": "acme", "name": "plat", "percent": 100})),
            ev("bundle.vectors_rejected", "", json!({"org": "acme", "name": "plat", "file": "m.dlphvec"})),
        ]
    }

    #[test]
    fn counts_are_per_bundle_per_file_and_only_for_accepted_tasks() {
        let a = aggregate(&[log()], "acme", "plat");
        assert_eq!((a.accepted_tasks, a.cited_tasks), (2, 2), "t3 was denied");
        assert_eq!((a.per_file["knowledge/a.md"], a.per_file["knowledge/b.md"]), (2, 1));
        assert!(!a.per_file.contains_key("knowledge/z.md"), "another bundle's citation is not this bundle's");
        assert_eq!(a.findings[&("G-acme-01".into(), "P2".into(), "ALIGN-REF-01".into())], 1);
        assert_eq!((a.context_builds, a.context_trims, a.vectors_percent, a.vectors_rejected), (2, 1, Some(100), 1));
    }

    #[test]
    fn a_citation_in_a_task_that_was_never_accepted_does_not_count() {
        let evs = vec![ev("bundle.cited", "t9", json!({"org": "acme", "name": "plat", "files": ["knowledge/a.md"]}))];
        let a = aggregate(&[evs], "acme", "plat");
        assert_eq!((a.accepted_tasks, a.cited_tasks), (0, 0));
    }

    #[test]
    fn the_report_prints_counts_and_never_span_text() {
        let a = aggregate(&[log()], "acme", "plat");
        let text = render("acme/plat", "1.4.0", &a, &["knowledge/a.md".into(), "knowledge/c.md".into()], false, None, Some("retrieval (publisher-reported, hybrid ranking): recall@10 0.90"));
        assert!(!text.contains("CANARY") && !text.contains("the model wrote"), "{}", text);
        assert!(text.contains("accepted tasks in these logs: 2") && text.contains("knowledge/a.md") && text.contains("G-acme-01"));
        assert!(text.contains("1 knowledge file(s) not cited yet") && text.contains("correlational") && text.contains("gate nothing"), "{}", text);
        assert!(text.contains("recall@10 0.90"));
    }

    #[test]
    fn a_never_cited_file_becomes_a_candidate_only_after_fifty_accepted_tasks() {
        let mut evs = Vec::new();
        for i in 0..CANDIDATE_AFTER {
            evs.push(ev("merge.approved", &format!("t{}", i), json!({})));
        }
        let a = aggregate(&[evs], "acme", "plat");
        let text = render("acme/plat", "1.0.0", &a, &["knowledge/unused.md".into()], true, Some(0), None);
        assert!(text.contains("candidates to shorten or drop): knowledge/unused.md") && text.contains("STALE") && text.contains("disputes open: 0"), "{}", text);
    }

    #[test]
    fn hostile_file_names_in_a_log_are_cleaned() {
        let evs = vec![ev("merge.approved", "t1", json!({})), ev("bundle.cited", "t1", json!({"org": "acme", "name": "plat", "files": ["knowledge/\u{1b}[31mred .md"]}))];
        let a = aggregate(&[evs], "acme", "plat");
        assert!(a.per_file.keys().all(|k| !k.contains('\u{1b}') && !k.contains(' ')));
    }
}
