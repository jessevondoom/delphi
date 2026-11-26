//! The console's view of a research run (`specs/112` §4 item 1).
//!
//! `research.start` (the console's `/research` and the knowledge window's expand page) and an inside agent's approved
//! `request_research` run on the one runner (`research_run`), in the one queue (`research_jobs`), and leave the same record and the
//! same library claims as a host's run. What they kept from the console's own job is here:
//!
//! * **One at a time.** The console draws one job and `research.status` and `research.cancel` name none, so the daemon holds one
//!   console run ([`Run`]) and refuses a second while it is going. Underneath it is an ordinary job of the queue, so it shares the
//!   running slots, the step budget and the time limit with every host's run.
//! * **The status the console polls** ([`status`]): the same fields the page-per-source job reported (phase, counts, one line per
//!   page, and a report when it ends), now read from the job and from the run's record, with the topic id.
//! * **Where a page went** ([`Dest`]): `shared` (a library source), `pending` (a weak source, kept as a pointer and never as text:
//!   what holding it outside the index was) or `project` (the run belongs to a registered project: its record is in the project's
//!   folder and its claims are scoped to the project).
//!
//! Everything here is counts, addresses and fixed words. No page text, title or answer is put into a status or an event.

use crate::research_record::Topic;
use serde_json::{json, Value};
use std::path::Path;

/// The session the daemon's own research runs are queued under: the console's and an inside agent's. No host has it (a host's
/// session id is sixteen hex characters), so no host can read, wait for or cancel one of these jobs.
pub const SESSION: &str = "daemonconsole";

/// Where one kept page went (`specs/058` §13, mapped to the record-and-library model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dest {
    Shared,
    Pending,
    Project,
}

impl Dest {
    pub fn as_str(self) -> &'static str {
        match self {
            Dest::Shared => "shared",
            Dest::Pending => "pending",
            Dest::Project => "project",
        }
    }
}

/// The run the console started, or an inside agent's approved request.
#[derive(Debug, Clone)]
pub struct Run {
    /// The topic as it was asked: what the console matches a status to.
    pub question: String,
    pub run_id: String,
    /// The run's record; `None` when it could not be made.
    pub topic: Option<Topic>,
    /// The run belongs to a registered project.
    pub project: bool,
}

fn host_of(url: &str) -> String {
    crate::research_run::host_of(url)
}

/// What the console says for a page that was not read or not kept, from the record's fixed word.
fn why(reason: &str) -> &'static str {
    match reason {
        "not_fetchable" => "not on your lists and not returned by a search of this run",
        "refused" => "refused by the research job's own rules",
        "fetch_failed" => "could not be read",
        "weak_source" => "a weak source, and `[validator] weak_sources` is off",
        "excluded_source" => "a shortener, cache, mirror or archive: use the original",
        _ => "could not be kept",
    }
}

fn finished(state: &str) -> bool {
    matches!(state, "done" | "partial" | "failed" | "cancelled")
}

/// The status of a console run as `research.status` returns it. `live` is the job's own status while the daemon holds it
/// (`Jobs::status`); without it the record alone answers, and a run the record still calls queued or running was stopped.
/// `home` is where the library is, for each kept source's licence.
pub fn status(run: &Run, live: Option<&Value>, home: &Path) -> Value {
    let rec = run.topic.as_ref().and_then(|t| t.read().ok());
    let run_rec = rec.as_ref().and_then(|r| r["runs"].as_array()).and_then(|runs| runs.iter().find(|x| x["run_id"].as_str() == Some(run.run_id.as_str())));
    let recorded = run_rec.and_then(|r| r["state"].as_str()).unwrap_or("failed");
    let state = match live.and_then(|l| l["status"].as_str()) {
        Some(s) => s,
        None if finished(recorded) => recorded,
        None => "failed",
    };
    let of_run = |list: &str| -> Vec<&Value> { rec.as_ref().and_then(|r| r[list].as_array()).into_iter().flatten().filter(|x| x["run"].as_str() == Some(run.run_id.as_str())).collect() };
    let (pages, refused) = (of_run("pages"), of_run("refused"));
    let fetched: Vec<String> = match live.and_then(|l| l["fetched"].as_array()) {
        Some(f) => f.iter().filter_map(|u| u.as_str().map(String::from)).collect(),
        None => pages.iter().filter_map(|p| p["url"].as_str().map(String::from)).collect(),
    };
    let lib = crate::library::Library::in_home(home);
    let going = !finished(state);

    let mut lines: Vec<Value> = Vec::new();
    let mut entries: Vec<(Value, &str)> = Vec::new(); // the entry and the capture's outcome
    let mut pictures = 0usize;
    for p in &pages {
        let url = p["url"].as_str().unwrap_or("");
        let id = p["source_id"].as_str().unwrap_or("");
        let meta = lib.read_meta(id).unwrap_or(Value::Null);
        let level = p["kept"].as_str().or(meta["capture_level"].as_str()).unwrap_or("pointer");
        let form = if level == "full" { "full" } else { "pointer" };
        let licence = meta["licence"].as_str().unwrap_or("unknown").to_string();
        let weak = meta["source_class"].as_str() == Some("weak") && meta["source_unfamiliar"].as_bool() != Some(true);
        let dest = if run.project {
            Dest::Project
        } else if weak {
            Dest::Pending
        } else {
            Dest::Shared
        };
        let outcome = p["outcome"].as_str().unwrap_or("added");
        let kept_pictures = meta["images"].as_array().map_or(0, Vec::len);
        if matches!(outcome, "added" | "new_version") {
            pictures += kept_pictures;
        }
        lines.push(json!({"url": url, "host": host_of(url), "status": "done", "form": form, "licence": licence, "why": null}));
        // What the knowledge window opens for the entry: a source whose text is kept, at its place in the index; for a pointer,
        // which has no text of its own, the topic's written answer, where its claims are.
        let path = match (level, &run.topic) {
            ("full", _) => format!("library/sources/{}/source.md", id),
            (_, Some(t)) => format!("research/topics/{}/answer.md", t.id),
            _ => format!("library/sources/{}", id),
        };
        let publisher = meta["rights_holder"].as_str().or(meta["publisher"].as_str()).map(String::from).unwrap_or_else(|| host_of(url));
        entries.push((json!({"path": path, "title": meta["title"].as_str().unwrap_or(url), "publisher": publisher, "url": url, "form": form, "licence": licence, "dest": dest.as_str(), "pictures": kept_pictures}), outcome));
    }
    let mut not_kept: Vec<Value> = Vec::new();
    let mut unlisted: Vec<String> = Vec::new();
    for r in &refused {
        let (url, reason) = (r["url"].as_str().unwrap_or(""), r["reason"].as_str().unwrap_or(""));
        if reason == "not_fetchable" {
            let h = host_of(url);
            if !h.is_empty() && !unlisted.contains(&h) {
                unlisted.push(h);
            }
            continue;
        }
        lines.push(json!({"url": url, "host": host_of(url), "status": "skipped", "form": null, "licence": null, "why": why(reason)}));
        not_kept.push(json!([url, why(reason)]));
    }
    // A page the job fetched that is neither kept nor refused: the one being read now, or one that turned out to be a copy of a
    // page already read.
    let known = |u: &str| pages.iter().chain(&refused).any(|x| x["url"].as_str() == Some(u));
    for (i, u) in fetched.iter().enumerate() {
        if known(u) {
            continue;
        }
        if going && i + 1 == fetched.len() {
            lines.push(json!({"url": u, "host": host_of(u), "status": "reading", "form": null, "licence": null, "why": null}));
        } else if !going || i + 1 < fetched.len() {
            lines.push(json!({"url": u, "host": host_of(u), "status": "not kept", "form": null, "licence": null, "why": "nothing new was kept from it"}));
        }
    }
    let mut hosts: Vec<String> = Vec::new();
    for u in &fetched {
        let h = host_of(u);
        if !h.is_empty() && hosts.last() != Some(&h) {
            hosts.push(h);
        }
    }
    let n = hosts.len();
    if n > 5 {
        hosts.drain(..n - 5);
    }
    let phase = match state {
        "queued" => "searching",
        "running" if fetched.is_empty() => "searching",
        "running" => "reading",
        "done" | "partial" => "done",
        "cancelled" => "cancelled",
        _ => "failed",
    };
    let error = live.and_then(|l| l["error"].as_str()).or(run_rec.and_then(|r| r["error"].as_str())).map(String::from).or_else(|| (phase == "failed").then(|| "the research job was stopped before it finished".to_string()));
    let topic_id = run.topic.as_ref().map(|t| t.id.clone());
    let report = (phase != "failed" && !going).then(|| {
        let result = rec.as_ref().filter(|r| r["result_run"].as_str() == Some(run.run_id.as_str())).map(|r| &r["result"]);
        let of = |outcome: &[&str]| -> Vec<&Value> { entries.iter().filter(|(_, o)| outcome.contains(o)).map(|(e, _)| e).collect() };
        let provider = rec.as_ref().and_then(|r| r["queries"].as_array()).into_iter().flatten().filter(|q| q["run"].as_str() == Some(run.run_id.as_str())).filter_map(|q| q["provider"].as_str()).next_back().map(String::from);
        let stopped = match state {
            "cancelled" => Some("cancelled"),
            "partial" => Some("the step budget"),
            _ => None,
        };
        json!({
            "new": of(&["added"]), "updated": of(&["new_version"]), "unchanged": of(&["unchanged", "kept_richer"]).len(),
            "not_kept": not_kept, "unlisted_hosts": unlisted, "provider": provider, "tokens": 0, "pictures": pictures, "stopped": stopped,
            "topic_id": topic_id,
            "claims": result.and_then(|r| r["library_claims"].as_array()).map_or(0, Vec::len),
            "coverage": result.and_then(|r| r["coverage"].as_str()),
        })
    });
    // `specs/139` §2.5 item 4, as the operator ruled on 2026-11-26: a finished run whose topic has a seed or written page names it, as
    // the host's `research_result` and the `record_research` reply do.
    let page = (phase == "done").then(|| topic_id.as_deref().and_then(|id| lib.topic_for_research(id))).flatten().and_then(|t| t.outcome.map(|o| json!({"slug": t.slug, "outcome": o})));
    let mut v = json!({
        "topic": run.question, "phase": phase, "found": lines.len(), "read": fetched.len(), "skipped": refused.len(), "hosts": hosts, "sources": lines,
        "tokens": 0, "error": error, "report": report, "topic_id": topic_id, "research_id": run.run_id,
    });
    if let Some(p) = page {
        v["page"] = p;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::topics::SeedRequest;
    use crate::library::Library;
    use crate::research_record::{Asker, Begin};

    /// `specs/139` §2.5 item 4, as the operator ruled on 2026-11-26: the console's `research.status` of a finished run names the topic's
    /// page, `{slug, outcome}`, once a seed (or a written page) exists; before any page it names none.
    #[test]
    fn research_status_carries_the_page_of_a_finished_run() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let t = Topic::begin(&ws, &Begin { question: "Why do tides lag?", asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap();
        t.finish("r-1", "done", Some(&json!({"answer": "Tides lag because the estuary is shallow.", "claims": []})), &[], None, 0).unwrap();
        let run = Run { question: "Why do tides lag?".into(), run_id: "r-1".into(), topic: Some(t), project: false };
        assert!(status(&run, None, home.path()).get("page").is_none(), "no page yet: none is named");
        let lib = Library::in_home(home.path());
        let slug = lib.topics_from_research(&ws, 1).remove(0);
        let seed = crate::topic_seed::assemble_linked(&lib, &ws, &lib.topic_side(&slug).unwrap());
        let cfg = delphi_adapter::config::PagesConfig::default();
        let req = SeedRequest { slug: &slug, now: 2, settings: &cfg, style: crate::library::bib::Format::Plain, seed: &seed, held: &crate::topic_seed::NoHeldText, gaps: &[], cancel: None };
        lib.topic_seed_build(req).unwrap();
        let v = status(&run, None, home.path());
        assert_eq!(v["phase"], "done");
        assert_eq!(v["page"], json!({"slug": slug, "outcome": "seed"}), "{v}");
    }
}
