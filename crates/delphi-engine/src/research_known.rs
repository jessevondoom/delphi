//! Known first, with the caller's visibility (`specs/114` §3 item 1): the search a research run makes before its first query.
//!
//! The run asks the retrieval service for its question over library claims, research topics and answers, kept source text and
//! the knowledge base. **Who asked decides what the search may return, and it is never more than that caller could have found by
//! searching:**
//!
//! * a host session ([`Caller::Host`]) is given the rows of its own `search_kb`: the shared zones through
//!   [`crate::host_labels::search`] with the same zone list, and its own project's claims through
//!   [`crate::shared_research::host_hits`], the one search a host's project research is read through. Two kinds of row that
//!   `search_kb` can return are **left out** here, so this search is narrower than the host's own: a claim that rests on a
//!   private or internal source (handing one to a session marks that session, `specs/115` §2 item 3, and what is found here is
//!   read by the plan call, whose queries go to a search engine), and another project's shared research;
//! * an inside agent ([`Caller::Role`]) is given what its `kb_search` returns: the index's own rule for that role
//!   (`kb::allowed_sql`), over the same zones, with no project layer;
//! * the operator's console ([`Caller::Operator`]) is given the operator's ordinary search over those zones.
//!
//! No caller is given a private topic, a claim on a private source, a staged note, a memory file or a finding: none of those
//! zones is asked for. What a row becomes is decided by its path in the index, which code wrote: a claim file is read back from
//! the library for its id, status and one-line statement; a topic for its question; a source for its address and the day it was
//! last checked. A row's own preview is never kept, so no page text leaves here.
//!
//! This module opens no connection and calls no model. It reads the index and the files the index points at.

use crate::kb::{Hit, SearchOpts};
use crate::research_method::{overlap, Known, KnownClaim, KnownFirst, KnownSource, KnownTopic};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The zones a run looks in before it searches. The same list a host's `search_kb` reads its shared results from.
pub const ZONES: [&str; 7] = ["core", "context", "specs", "knowledge", "library", "bundles", crate::research_index::ZONE];
/// Results asked of each search: a search returns ten at most.
const LIMIT: usize = 10;

/// Who a run searches as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// A host session working in this folder.
    Host { cwd: PathBuf },
    /// An inside agent, by its role id.
    Role(String),
    /// The operator's own console.
    Operator,
}

/// The live search: the workspace's index, read as `caller` may read it. One per run; what it found for the run's question is
/// kept, so the request that queued the run and the run itself read the same answer.
pub struct LiveKnown {
    pub ws: PathBuf,
    pub home: PathBuf,
    pub caller: Caller,
    found: Mutex<Option<(String, Known)>>,
}

impl LiveKnown {
    pub fn new(ws: &Path, home: &Path, caller: Caller) -> LiveKnown {
        LiveKnown { ws: ws.to_path_buf(), home: home.to_path_buf(), caller, found: Mutex::new(None) }
    }

    /// The rows the caller's own search returns for `query`, and nothing else.
    fn hits(&self, db: &Path, query: &str) -> Vec<Hit> {
        match &self.caller {
            Caller::Host { cwd } => {
                let mut hits = match crate::host_labels::search(&self.home, db, &self.ws, query, ZONES.to_vec(), LIMIT, crate::host_labels::Use::Explore, false) {
                    Ok(found) => found.rest.into_iter().chain(found.third_party).collect::<Vec<_>>(),
                    Err(_) => Vec::new(),
                };
                // The session's own project's claims. A claim on an internal source, and another project's research, stay out.
                let (project, _) = crate::shared_research::host_hits(&self.ws, &self.home, db, cwd, query, LIMIT);
                hits.extend(project.into_iter().filter(|h| !h.internal && !h.shared));
                hits
            }
            Caller::Role(role) => crate::retrieval::search(&self.home, db, query, SearchOpts { role: Some(role.clone()), limit: LIMIT, zones: Some(ZONES.to_vec()), ..Default::default() }).map(|r| r.hits).unwrap_or_default(),
            Caller::Operator => crate::retrieval::search(&self.home, db, query, SearchOpts { role: None, limit: LIMIT, zones: Some(ZONES.to_vec()), ..Default::default() }).map(|r| r.hits).unwrap_or_default(),
        }
    }

    fn search(&self, question: &str) -> Known {
        let db = self.ws.join("local/state.db");
        let query: String = question.lines().next().unwrap_or("").chars().take(500).collect();
        if query.trim().is_empty() || !db.is_file() {
            return Known::default();
        }
        // A source newer than the index is indexed first, within the bound every search has (`specs/109` §3.1).
        let _ = crate::service::refresh_within(&self.ws, &self.home, crate::service::SEARCH_REFRESH_BOUND);
        let lib = crate::library::Library::in_home(&self.home);
        let now = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap_or(0);
        let host = matches!(self.caller, Caller::Host { .. });
        let mut known = Known::default();
        for h in self.hits(&db, &query).iter().filter(|h| h.doc_status == "active") {
            match row_of(&h.rel_path) {
                Row::Claim(id) => {
                    let Some(c) = lib.claim(id) else { continue };
                    // Kept here as well as in the index's listing, so a change there cannot widen this answer by itself.
                    if lib.rests_on_private(&c) || lib.rests_on_internal(&c) || c["scope"] == "personal" || known.claims.iter().any(|k| k.id == id) {
                        continue;
                    }
                    known.claims.push(KnownClaim { id: id.to_string(), status: crate::library::claims::status(&c).as_str().to_string(), text: c["text"].as_str().unwrap_or("").to_string() });
                }
                Row::Topic(id) => {
                    let Some(rec) = crate::research_record::Topic::at(&self.ws, id).and_then(|t| t.read().ok()) else { continue };
                    if rec["private"] != false {
                        continue;
                    }
                    let question_of = rec["question"].as_str().unwrap_or("").to_string();
                    match known.topics.iter_mut().find(|t| t.id == id) {
                        Some(t) => t.score = Some(t.score.map_or(h.score, |s| s.max(h.score))),
                        None => known.topics.push(KnownTopic { id: id.to_string(), overlap: overlap(question, &question_of), question: question_of, score: Some(h.score), workspace: true }),
                    }
                }
                Row::Source(id) => {
                    let Some(meta) = lib.read_meta(id) else { continue };
                    if crate::library::is_private(&meta) || !meta["purged"].is_null() || known.sources.iter().any(|s| s.id == id) {
                        continue;
                    }
                    let age = lib.checked_age(id, now);
                    known.sources.push(KnownSource { id: id.to_string(), url: meta["canonical_url"].as_str().unwrap_or("").to_string(), age_days: age.map(|a| a.0), fresh: age.is_some_and(|a| a.1) && meta["capture_level"] == "full" });
                }
                Row::Other => {
                    let pointer = if host { crate::library::host_pointer(&h.pointer) } else { h.pointer.clone() };
                    if !known.kb.contains(&pointer) {
                        known.kb.push(pointer);
                    }
                }
            }
        }
        known.capped()
    }
}

impl KnownFirst for LiveKnown {
    fn find(&self, question: &str) -> Known {
        let mut found = self.found.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((q, known)) = found.as_ref() {
            if q == question {
                return known.clone();
            }
        }
        let known = self.search(question);
        *found = Some((question.to_string(), known.clone()));
        known
    }
}

/// The topic a new question continues instead of starting a new one (`specs/114` §3 item 3), with that topic's own question;
/// `None` starts a new topic. Two cases, in order:
///
/// * the same question was asked before in the same place (`root`: the workspace, or the asker's project): its topic id is the one
///   a new run would write into anyway, so the run continues it;
/// * a topic of the workspace that `known` holds (so: one the asker's own search returned) overlaps the question over both bars.
///
/// Only a topic Delphi's own runner researched, whose newest run finished, and whose record is not private is continued: a
/// private record stays the operator's to continue.
pub fn topic_to_continue(known: &Known, ws: &Path, root: &Path, question: &str, bar: &crate::research_method::ContinueBar) -> Option<(crate::research_record::Topic, String)> {
    use crate::research_record::Topic;
    let usable = |t: Topic| -> Option<(Topic, String)> {
        let rec = t.read().ok()?;
        let ours = rec["runner"].is_null() || rec["runner"] == "daemon";
        let finished = matches!(rec["state"].as_str(), Some("done" | "partial"));
        let question = rec["question"].as_str().map(str::trim).filter(|q| !q.is_empty())?.to_string();
        (rec["private"] == false && ours && finished).then_some((t, question))
    };
    if let Some(same) = Topic::at(root, &crate::research_record::topic_id(question.trim())).and_then(usable) {
        return Some(same);
    }
    let t = crate::research_method::strongest(known, bar)?;
    Topic::at(ws, &t.id).and_then(usable)
}

/// What a row of the index is, read from its path alone.
enum Row<'a> {
    /// A library claim, by its id: `research/claims/<id>.json`, or a project's own `research/projects/<project>/claims/<id>.json`.
    Claim(&'a str),
    /// A topic of the workspace, by its id: its record or its answer.
    Topic(&'a str),
    /// A library source, by its id.
    Source(&'a str),
    Other,
}

fn row_of(rel_path: &str) -> Row<'_> {
    let parts: Vec<&str> = rel_path.split('/').collect();
    match parts.as_slice() {
        ["research", "claims", file] => file.strip_suffix(".json").map_or(Row::Other, Row::Claim),
        ["research", "projects", _, dir, file] if *dir == crate::research_index::CLAIMS_DIR => file.strip_suffix(".json").map_or(Row::Other, Row::Claim),
        ["research", "topics", id, file] if matches!(*file, "record.json" | "answer.md") && crate::research_record::valid_topic_id(id) => Row::Topic(id),
        ["library", "sources", id, "source.md" | "excerpts.md"] => Row::Source(id),
        _ => Row::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kb::Kb;
    use crate::library::claims::{AnchorSpec, ResearchClaim};
    use crate::library::{AddOpts, Budgets, Level, Library};
    use crate::research_record::{Asker, Begin, Topic};
    use delphi_core::research::Depth;
    use serde_json::json;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// An install home with one workspace and two registered projects, `alpha` (A) and `beta` (B).
    struct Fx {
        _root: tempfile::TempDir,
        home: PathBuf,
        ws: PathBuf,
        a: PathBuf,
        b: PathBuf,
    }

    fn fx() -> Fx {
        let root = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(root.path()).unwrap();
        let f = Fx { ws: home.join("workspaces/w"), a: home.join("pa"), b: home.join("pb"), home, _root: root };
        for d in [&f.ws.join("local"), &f.a, &f.b] {
            std::fs::create_dir_all(d).unwrap();
        }
        let entry = |id: &str, root: &Path, name: &str| json!({"project_id": id, "root": root, "display_name": name, "private": false});
        std::fs::write(f.ws.join("local/projects.json"), json!({"schema": 1, "projects": {A: entry(A, &f.a, "alpha"), B: entry(B, &f.b, "beta")}}).to_string()).unwrap();
        f
    }

    fn sync(f: &Fx) {
        let mut kb = Kb::open(&f.ws.join("local/state.db")).unwrap();
        kb.refresh(&f.ws, "").unwrap();
    }

    fn source(f: &Fx, name: &str, text: &str, private: bool) -> String {
        let file = f.home.join(format!("{name}.md"));
        std::fs::write(&file, text).unwrap();
        Library::in_home(&f.home).add_file(&file, &AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Full), private, ..Default::default() }, &Budgets::default()).unwrap().id
    }

    /// A finished topic under `root` with a written answer; `private` marks its record as one that rests on a private source.
    fn topic(root: &Path, project: Option<&str>, question: &str, answer: &str, private: bool) -> Topic {
        let t = Topic::begin(root, &Begin { question, asked_by: Asker::operator(), project, depth: Depth::Quick, run_id: "r-t-1" }).unwrap();
        let result = json!({"answer": answer, "budget_used": {"steps": 1, "check_steps": 0}, "coverage": "ok", "coverage_report": {"thin_below": 1, "counted": 1, "rungs": []}, "claims": [{"id": "x"}], "search_log": {"private": private}});
        t.finish("r-t-1", "done", Some(&result), &[], None, 5).unwrap();
        t
    }

    fn claim(f: &Fx, text: &str, fingerprint: &str, project: Option<&str>, topic: Option<&str>, source_id: &str, quote: &str) -> String {
        let anchors = [AnchorSpec { source_id: source_id.into(), locator: Some("https://example.org/page".into()), quote: quote.into(), prefix: None, suffix: None, read: None }];
        Library::in_home(&f.home).add_research_claim(&ResearchClaim { text, fingerprint, project, topic, anchors: &anchors }).unwrap().0["id"].as_str().unwrap().to_string()
    }

    fn as_caller(f: &Fx, caller: Caller, question: &str) -> Known {
        sync(f);
        LiveKnown::new(&f.ws, &f.home, caller).find(question)
    }

    fn ids(known: &Known) -> (Vec<&str>, Vec<&str>) {
        (known.claims.iter().map(|c| c.id.as_str()).collect(), known.topics.iter().map(|t| t.id.as_str()).collect())
    }

    /// The whole of what a run is told was known, as one text: what a host could read back from `known_before`.
    fn shown(known: &Known) -> String {
        known.to_json().to_string()
    }

    // specs/114 §3 items 1 and 2: what the workspace holds on the question is found as claims with their status, topics that
    // overlap and sources with their age, for every kind of caller; no row's own text (a quote, a page) is kept.
    #[test]
    fn a_question_the_workspace_holds_is_found_as_claims_topics_and_sources() {
        let f = fx();
        let src = source(&f, "tanks", "# Tank husbandry\n\nWhat temperature do quollfish tanks need? Quollfish tanks are held at twenty-eight degrees in every survey we read.\n", false);
        let t = topic(&f.ws, None, "What temperature do quollfish tanks need?", "Quollfish tanks need twenty-eight degrees.", false);
        let c = claim(&f, "quollfish tanks need twenty-eight degrees", "f000000000000000000000000000000000000000000000000000000000000001", None, Some(&t.id), &src, "held at twenty-eight degrees");
        for caller in [Caller::Operator, Caller::Role("scout".into()), Caller::Host { cwd: f.home.join("elsewhere") }] {
            let known = as_caller(&f, caller.clone(), "what temperature do quollfish tanks need");
            let (claims, topics) = ids(&known);
            assert_eq!(claims, [c.as_str()], "{caller:?}: {known:?}");
            assert_eq!(known.claims[0].status, "noted");
            assert_eq!(known.claims[0].text, "quollfish tanks need twenty-eight degrees");
            assert_eq!(topics, [t.id.as_str()], "{caller:?}: {known:?}");
            assert_eq!(known.topics[0].overlap, 1.0);
            assert!(known.topics[0].score.is_some_and(|s| s > 0.0) && known.topics[0].workspace);
            let s = known.sources.iter().find(|s| s.id == src).unwrap_or_else(|| panic!("{caller:?}: the kept source is not among {known:?}"));
            assert_eq!((s.age_days, s.fresh), (Some(0), true));
            assert!(!shown(&known).contains("held at twenty-eight degrees") && !shown(&known).contains("every survey we read"), "a quote or a page's text was kept: {}", shown(&known));
        }
        // No index yet: nothing is known, and nothing fails.
        let empty = fx();
        assert!(LiveKnown::new(&empty.ws, &empty.home, Caller::Operator).find("what temperature do quollfish tanks need").is_empty());
    }

    // specs/114 §3 item 1, §10: a host learns nothing through what was known that its own search could not return. A private
    // topic, a claim on a private source, another project's claims and research, its own project's internal-source claim, a
    // staged note and another role's memory are all absent, whatever the question's words match.
    #[test]
    fn a_host_is_told_nothing_its_own_search_could_not_return() {
        let f = fx();
        let question = "how deep is the numbatshelf cable landing";
        let open = source(&f, "open", "# Survey\n\nThe numbatshelf cable landing drops to ninety metres at the shelf edge.\n", false);
        let secret = source(&f, "secret", "# Private survey\n\nThe numbatshelf cable landing vaultword drops to ninety-five metres.\n", true);
        // The workspace: one open topic and its claim, one private topic, one claim on a private source.
        let ws_topic = topic(&f.ws, None, "How deep is the numbatshelf cable landing?", "The numbatshelf cable landing is ninety metres deep.", false);
        let ws_claim = claim(&f, "the numbatshelf cable landing is ninety metres deep", "f000000000000000000000000000000000000000000000000000000000000011", None, Some(&ws_topic.id), &open, "drops to ninety metres");
        let ws_private_topic = topic(&f.ws, None, "How deep is the numbatshelf cable landing by the private survey?", "The numbatshelf cable landing vaultword is ninety-five metres deep.", true);
        let ws_private_claim = claim(&f, "the numbatshelf cable landing vaultword is ninety-five metres deep", "f000000000000000000000000000000000000000000000000000000000000012", None, Some(&ws_private_topic.id), &secret, "drops to ninety-five metres");
        // Project A: a public claim and an internal-source claim, each on a topic of A's own.
        let a_topic = topic(&f.a, Some("alpha"), "How deep is the numbatshelf cable landing for alpha?", "Alpha says the numbatshelf cable landing is ninety metres deep.", false);
        let a_claim = claim(&f, "alpha measured the numbatshelf cable landing at ninety metres", "f000000000000000000000000000000000000000000000000000000000000013", Some("alpha"), Some(&a_topic.id), &open, "drops to ninety metres");
        let a_internal = claim(&f, "alpha's private survey puts the numbatshelf cable landing at ninety-five metres", "f000000000000000000000000000000000000000000000000000000000000014", Some("alpha"), Some(&a_topic.id), &secret, "drops to ninety-five metres");
        // Project B: a claim of its own.
        let b_topic = topic(&f.b, Some("beta"), "How deep is the numbatshelf cable landing for beta?", "Beta says the numbatshelf cable landing is ninety metres deep.", false);
        let b_claim = claim(&f, "beta measured the numbatshelf cable landing at ninety metres", "f000000000000000000000000000000000000000000000000000000000000015", Some("beta"), Some(&b_topic.id), &open, "drops to ninety metres");
        // A staged note and a role's memory that hold the question's words.
        for (path, text) in [("local/staged/host-abcd1234/n1.md", "# Staged\n\nnumbatshelf cable landing staged note stagedword.\n"), ("local/memory/agents/builder/lessons.md", "# Memory\n\nnumbatshelf cable landing memory memoryword.\n")] {
            std::fs::create_dir_all(f.ws.join(path).parent().unwrap()).unwrap();
            std::fs::write(f.ws.join(path), text).unwrap();
        }

        // A host in project B: the workspace's open claim and topic, and B's own claim. Nothing of A, nothing private.
        let from_b = as_caller(&f, Caller::Host { cwd: f.b.clone() }, question);
        let (claims, topics) = ids(&from_b);
        assert!(claims.contains(&ws_claim.as_str()) && claims.contains(&b_claim.as_str()), "{from_b:?}");
        assert_eq!(topics, [ws_topic.id.as_str()], "only the workspace's open topic: {from_b:?}");
        for hidden in [&ws_private_claim, &a_claim, &a_internal] {
            assert!(!claims.contains(&hidden.as_str()), "{hidden} reached a host in B: {from_b:?}");
        }
        let text = shown(&from_b);
        for word in ["vaultword", "ninety-five", "alpha", "stagedword", "memoryword", ws_private_topic.id.as_str(), a_topic.id.as_str(), secret.as_str()] {
            assert!(!text.contains(word), "`{word}` reached a host in B through what was known: {text}");
        }
        // The private source is never listed as held, and no source is offered that the index does not list.
        assert!(from_b.sources.iter().all(|s| s.id != secret), "{from_b:?}");

        // A host in project A reads A's public claim and not A's own internal-source claim: handing that one over would mark
        // the session, and what is known is read by the plan call.
        let from_a = as_caller(&f, Caller::Host { cwd: f.a.clone() }, question);
        let (claims, _) = ids(&from_a);
        assert!(claims.contains(&a_claim.as_str()) && !claims.contains(&a_internal.as_str()) && !claims.contains(&b_claim.as_str()), "{from_a:?}");
        assert!(!shown(&from_a).contains("ninety-five"), "{}", shown(&from_a));

        // A host in no project: the workspace's open research alone.
        let outside = as_caller(&f, Caller::Host { cwd: f.home.join("elsewhere") }, question);
        assert_eq!(ids(&outside), (vec![ws_claim.as_str()], vec![ws_topic.id.as_str()]), "{outside:?}");

        // Every row a host is told of is one its own search returns: the shared zones' rows and its project's own claims.
        let db = f.ws.join("local/state.db");
        let own_search: Vec<String> = {
            let shared = crate::host_labels::search(&f.home, &db, &f.ws, question, ZONES.to_vec(), 10, crate::host_labels::Use::Explore, false).unwrap();
            let project = crate::shared_research::host_hits(&f.ws, &f.home, &db, &f.b, question, 10).0;
            shared.rest.iter().chain(&shared.third_party).chain(&project).map(|h| h.rel_path.clone()).collect()
        };
        for c in &from_b.claims {
            assert!(own_search.iter().any(|p| p.ends_with(&format!("/{}.json", c.id))), "claim {} is not a row of the host's own search: {own_search:?}", c.id);
        }
        for t in &from_b.topics {
            assert!(own_search.iter().any(|p| p.contains(&format!("research/topics/{}/", t.id))), "topic {} is not a row of the host's own search", t.id);
        }

        // An inside agent and the operator are not given a project's claims either (no project layer is asked for), nor
        // anything private.
        for caller in [Caller::Role("scout".into()), Caller::Operator] {
            let known = as_caller(&f, caller.clone(), question);
            let (claims, topics) = ids(&known);
            assert_eq!((claims, topics), (vec![ws_claim.as_str()], vec![ws_topic.id.as_str()]), "{caller:?}: {known:?}");
            assert!(!shown(&known).contains("vaultword") && !shown(&known).contains("memoryword") && !shown(&known).contains("stagedword"), "{caller:?}");
        }
    }

    // specs/114 §3 item 3: which topic a new question continues. The same question asked again in the same place, or a topic
    // the asker's own search returned that overlaps over both bars. Never a private topic, one whose newest run did not finish,
    // one a host or an ingested file wrote, or one the asker's search did not return.
    #[test]
    fn a_question_continues_the_same_topic_or_one_that_overlaps_and_never_a_private_or_unfinished_one() {
        use crate::research_method::ContinueBar;
        let f = fx();
        let bar = ContinueBar::default();
        let elsewhere = Caller::Host { cwd: f.home.join("elsewhere") };
        let t = topic(&f.ws, None, "What temperature do quollfish tanks need?", "Quollfish tanks need twenty-eight degrees.", false);
        // The same question, whatever its case and spacing: its topic id is the one a new run would write into.
        let same = topic_to_continue(&Known::default(), &f.ws, &f.ws, "  what temperature do   QUOLLFISH tanks need? ", &bar);
        assert_eq!(same.map(|(t, q)| (t.id, q)), Some((t.id.clone(), "What temperature do quollfish tanks need?".to_string())));
        // A question most of whose words the topic has, which the asker's search finds.
        let near = "which temperature do quollfish tanks need in winter";
        let known = as_caller(&f, elsewhere.clone(), near);
        assert_eq!(topic_to_continue(&known, &f.ws, &f.ws, near, &bar).map(|(t, _)| t.id), Some(t.id.clone()), "{known:?}");
        assert!(topic_to_continue(&known, &f.ws, &f.ws, near, &ContinueBar { overlap: 0.95, score: 0.0 }).is_none(), "under the overlap bar");
        assert!(topic_to_continue(&known, &f.ws, &f.ws, near, &ContinueBar { overlap: 0.5, score: 0.9 }).is_none(), "under the score bar");
        // The same words, but the asker's search did not return the topic: nothing is offered.
        assert!(topic_to_continue(&Known::default(), &f.ws, &f.ws, near, &bar).is_none());
        // An unrelated question.
        let other = "how are numbat burrows dug";
        assert!(topic_to_continue(&as_caller(&f, elsewhere.clone(), other), &f.ws, &f.ws, other, &bar).is_none());

        // A private topic is never continued, by the same question or by overlap: it is not in the index, and its record says so.
        let private = topic(&f.ws, None, "How deep is the numbatshelf cable landing?", "Ninety-five metres.", true);
        assert!(topic_to_continue(&Known::default(), &f.ws, &f.ws, "How deep is the numbatshelf cable landing?", &bar).is_none());
        let asked = "how deep is the numbatshelf cable landing at low tide";
        let known = as_caller(&f, elsewhere.clone(), asked);
        assert!(known.topics.iter().all(|k| k.id != private.id) && topic_to_continue(&known, &f.ws, &f.ws, asked, &bar).is_none(), "{known:?}");
        // Even handed a list that names it (which no search returns), the record's own flag refuses it.
        let forged = Known { topics: vec![KnownTopic { id: private.id.clone(), question: "How deep is the numbatshelf cable landing?".into(), overlap: 1.0, score: Some(1.0), workspace: true }], ..Default::default() };
        assert!(topic_to_continue(&forged, &f.ws, &f.ws, asked, &bar).is_none());

        // A topic whose run has not finished, and one a host recorded with its own tools, are not continued by the daemon's runner.
        Topic::begin(&f.ws, &Begin { question: "How long do quollfish live?", asked_by: Asker::operator(), project: None, depth: Depth::Quick, run_id: "r-q-1" }).unwrap();
        assert!(topic_to_continue(&Known::default(), &f.ws, &f.ws, "How long do quollfish live?", &bar).is_none(), "queued, not finished");
        let by_host = topic(&f.ws, None, "What do quollfish eat?", "Brine shrimp.", false);
        by_host.runner("r-t-1", "host").unwrap();
        assert!(topic_to_continue(&Known::default(), &f.ws, &f.ws, "What do quollfish eat?", &bar).is_none(), "a host's own research is not the daemon's to continue");

        // In a project: the same question continues the project's own topic, and another project's is not found there.
        let in_a = topic(&f.a, Some("alpha"), "What salinity do quollfish need?", "Brackish.", false);
        assert_eq!(topic_to_continue(&Known::default(), &f.ws, &f.a, "what salinity do quollfish need?", &bar).map(|(t, _)| t.dir), Some(in_a.dir.clone()));
        assert!(topic_to_continue(&Known::default(), &f.ws, &f.b, "what salinity do quollfish need?", &bar).is_none(), "project B does not continue project A's topic");
        // Nor by overlap: another project's topic is not a row a host is told of, so it cannot be the one that is offered.
        let from_b = as_caller(&f, Caller::Host { cwd: f.b.clone() }, "what salinity do quollfish need in summer");
        assert!(from_b.topics.iter().all(|k| k.id != in_a.id), "{from_b:?}");
        assert!(topic_to_continue(&from_b, &f.ws, &f.b, "what salinity do quollfish need in summer", &bar).is_none_or(|(t, _)| t.dir != in_a.dir && t.dir.starts_with(&f.ws)));
        // The default bars are the spec's: half of the question's content words. A question that shares half its words with a
        // workspace topic continues it, though it asks something else; `new: true` and `[research] continue_overlap` are the ways out.
        let half = topic_to_continue(&from_b, &f.ws, &f.b, "what salinity do quollfish need in summer", &bar);
        assert_eq!(half.map(|(t, _)| t.id), Some(t.id.clone()), "{from_b:?}");
        assert!(topic_to_continue(&from_b, &f.ws, &f.b, "what salinity do quollfish need in summer", &ContinueBar { overlap: 0.75, score: bar.score }).is_none());
    }

    #[test]
    fn a_row_is_what_its_path_says_and_nothing_else() {
        assert!(matches!(row_of("research/claims/c-0123456789ab.json"), Row::Claim("c-0123456789ab")));
        assert!(matches!(row_of(&format!("research/projects/{A}/claims/c-0123456789ab.json")), Row::Claim("c-0123456789ab")));
        assert!(matches!(row_of(&format!("research/projects/{A}/internal/c-0123456789ab.json")), Row::Other), "an internal-source claim is not read as a claim");
        assert!(matches!(row_of("research/topics/how-deep-0123abcd/answer.md"), Row::Topic("how-deep-0123abcd")));
        assert!(matches!(row_of("research/topics/../../etc/record.json"), Row::Other));
        assert!(matches!(row_of(&format!("projects/{A}/files/research/topics/how-deep-0123abcd/answer.md")), Row::Other), "a project's file is not a workspace topic");
        assert!(matches!(row_of("library/sources/s-1/source.md"), Row::Source("s-1")));
        assert!(matches!(row_of("knowledge/notes.md"), Row::Other));
    }
}
