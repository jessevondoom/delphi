//! `bundle.cited` (`specs/027` §7.3): a task's context included a file of an added bundle **and** its output was accepted.
//!
//! Derived from the log, never from a model's claim. "In context" means a successful `read_file`, `read_doc` or `search`
//! whose result carries a `@bundle/<org>/<name>/…` path. "Accepted" means an operator's card answered yes
//! (`approval.resolved` approved) or a merge was approved (`merge.approved`). The event is written at that moment for
//! the files read so far, and again for files read later, so a file is named at most once per task. Descriptive only:
//! these are correlational signals and gate nothing.

use crate::mission::EngineError;
use crate::mission::MissionState;
use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const READS: [&str; 3] = ["read_file", "read_doc", "search"];
const PREFIX: &str = "@bundle/";

/// `(org, name, file)` from a logical bundle path, if it is one with a file part.
fn split(path: &str) -> Option<(String, String, String)> {
    let rest = path.strip_prefix(PREFIX)?;
    let mut parts = rest.splitn(3, '/');
    let (org, name, file) = (parts.next()?, parts.next()?, parts.next()?);
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if !plain(org) || !plain(name) || file.is_empty() || file.contains("..") {
        return None;
    }
    Some((org.to_string(), name.to_string(), file.trim_end_matches([':', ',']).to_string()))
}

/// The bundle files whose text reached `task_id`'s context, by `org/name`.
pub fn in_context(events: &[EventEnvelope], task_id: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut calls: BTreeMap<String, (String, String)> = BTreeMap::new(); // call id -> (tool, path argument)
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for e in events.iter().filter(|e| e.task_id.as_deref() == Some(task_id)) {
        match e.kind.as_str() {
            "agent.block.complete" => {
                let b = &e.payload["block"];
                if b["type"] == "tool_call" {
                    if let (Some(id), Some(name)) = (b["id"].as_str(), b["name"].as_str()) {
                        if READS.contains(&name) {
                            calls.insert(id.to_string(), (name.to_string(), b["arguments"]["path"].as_str().unwrap_or("").to_string()));
                        }
                    }
                }
            }
            "tool.result" if e.payload["is_error"] != true => {
                let Some((tool, path)) = e.payload["tool_call_id"].as_str().and_then(|id| calls.get(id)) else { continue };
                if tool == "search" {
                    // `path:line: text` rows; a row naming a bundle file puts that file's text in context.
                    for line in e.payload["content"].as_str().unwrap_or("").lines() {
                        let head = line.split_whitespace().next().unwrap_or("");
                        if let Some((org, name, file)) = split(head.split(':').next().unwrap_or("")) {
                            out.entry(format!("{}/{}", org, name)).or_default().insert(file);
                        }
                    }
                } else if let Some((org, name, file)) = split(path) {
                    out.entry(format!("{}/{}", org, name)).or_default().insert(file);
                }
            }
            _ => {}
        }
    }
    out
}

/// The `bundle.cited` payloads this task has earned and not yet been given: files in context minus files already named.
pub fn pending(events: &[EventEnvelope], task_id: &str, by: &str) -> Vec<Value> {
    let mut named: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for e in events.iter().filter(|e| e.kind == "bundle.cited" && e.task_id.as_deref() == Some(task_id)) {
        let id = format!("{}/{}", e.payload["org"].as_str().unwrap_or(""), e.payload["name"].as_str().unwrap_or(""));
        named.entry(id).or_default().extend(e.payload["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(String::from)));
    }
    in_context(events, task_id)
        .into_iter()
        .filter_map(|(id, files)| {
            let fresh: Vec<String> = files.into_iter().filter(|f| !named.get(&id).is_some_and(|n| n.contains(f))).collect();
            let (org, name) = id.split_once('/')?;
            (!fresh.is_empty()).then(|| json!({"org": org, "name": name, "files": fresh, "accepted_by": by}))
        })
        .collect()
}

/// Write `bundle.cited` for `task_id`, which has just been accepted (`by` is `approval` or `merge`).
pub async fn emit(m: &Arc<MissionState>, task_id: &str, by: &str) -> Result<(), EngineError> {
    let events = m.replay()?;
    for payload in pending(&events, task_id, by) {
        let mut e = EventEnvelope::new(&m.id, "bundle.cited", payload);
        e.task_id = Some(task_id.to_string());
        m.append(e).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, task: &str, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m_1", kind, payload);
        e.task_id = Some(task.to_string());
        e
    }

    fn call(task: &str, id: &str, name: &str, path: &str) -> EventEnvelope {
        ev("agent.block.complete", task, json!({"turn_index": 1, "role": "assistant", "block_index": 0, "block": {"type": "tool_call", "id": id, "name": name, "arguments": {"path": path}}}))
    }

    fn result(task: &str, id: &str, content: &str, is_error: bool) -> EventEnvelope {
        ev("tool.result", task, json!({"turn_index": 2, "tool_call_id": id, "name": "x", "content": content, "is_error": is_error}))
    }

    #[test]
    fn a_bundle_file_read_or_found_by_search_is_in_context_and_an_error_or_another_zone_is_not() {
        let evs = vec![
            call("t1", "c1", "read_file", "@bundle/acme/plat/knowledge/a.md"),
            result("t1", "c1", "text", false),
            call("t1", "c2", "read_file", "@bundle/acme/plat/knowledge/missing.md"),
            result("t1", "c2", "not found", true),
            call("t1", "c3", "read_file", "@project/app/src/lib.rs"),
            result("t1", "c3", "code", false),
            call("t1", "c4", "search", "@bundle/acme/plat/knowledge"),
            result("t1", "c4", "@bundle/acme/plat/knowledge/b.md:3: needle\n@bundle/other/x/knowledge/c.md:9: needle", false),
            call("t1", "c5", "list_dir", "@bundle/acme/plat/knowledge"),
            result("t1", "c5", "a.md\nz.md", false),
            call("t2", "c6", "read_file", "@bundle/acme/plat/knowledge/zz.md"),
            result("t2", "c6", "other task", false),
        ];
        let got = in_context(&evs, "t1");
        assert_eq!(got["acme/plat"].iter().cloned().collect::<Vec<_>>(), vec!["knowledge/a.md", "knowledge/b.md"]);
        assert_eq!(got["other/x"].iter().cloned().collect::<Vec<_>>(), vec!["knowledge/c.md"]);
        assert_eq!(got.len(), 2, "a failed read, a project file, a directory listing and another task's read add nothing");
    }

    #[test]
    fn emission_matrix_in_context_times_accepted() {
        let read = vec![call("t1", "c1", "read_file", "@bundle/acme/plat/knowledge/a.md"), result("t1", "c1", "text", false)];
        let none: Vec<EventEnvelope> = vec![call("t1", "c1", "read_file", "@project/app/x"), result("t1", "c1", "text", false)];
        // The caller asks only when something was accepted; the matrix is the context side plus the dedupe.
        let p = pending(&read, "t1", "merge");
        assert_eq!(p.len(), 1);
        assert_eq!((p[0]["org"].as_str(), p[0]["name"].as_str(), p[0]["accepted_by"].as_str()), (Some("acme"), Some("plat"), Some("merge")));
        assert_eq!(p[0]["files"], json!(["knowledge/a.md"]));
        assert!(pending(&none, "t1", "merge").is_empty(), "accepted without a bundle in context: no event");
        assert!(pending(&[], "t1", "merge").is_empty());
        // Accepted twice (a card, then the merge): the second adds only what was read in between.
        let mut again = read.clone();
        again.push(ev("bundle.cited", "t1", p[0].clone()));
        assert!(pending(&again, "t1", "merge").is_empty(), "a file is named at most once per task");
        again.push(call("t1", "c2", "read_doc", "@bundle/acme/plat/knowledge/b.md"));
        again.push(result("t1", "c2", "more", false));
        let q = pending(&again, "t1", "approval");
        assert_eq!(q.len(), 1);
        assert_eq!(q[0]["files"], json!(["knowledge/b.md"]));
    }

    #[test]
    fn a_path_that_escapes_or_names_no_file_is_not_a_citation() {
        assert!(split("@bundle/acme/plat").is_none() && split("@bundle/acme/plat/").is_none());
        assert!(split("@bundle/acme/plat/../x.md").is_none() && split("@bundle/a b/plat/x.md").is_none());
        assert!(split("@bundle/acme/plat/knowledge/x.md").is_some());
    }
}
