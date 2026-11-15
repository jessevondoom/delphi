//! Zone `research` of the search index (`specs/112` §3.5): topic records, their answers, and library claims.
//!
//! * A topic of the workspace gives two documents: `research/topics/<topic-id>/record.json` (what was asked, searched, read and
//!   found) and `research/topics/<topic-id>/answer.md` (the written answer).
//! * A library claim gives one: `research/claims/<claim-id>.json`, labelled by the claim's **derived** status
//!   (`library::claims::status`): a new research claim reads *noted*. The status is computed from the claim's own record every time
//!   the claim changes; the index stores the word and nothing a document says about itself can set it.
//!
//! **What a role may see follows the rules that already apply to the library.** Every document of the zone is indexed at the
//! library's one fixed tier (`research::TIER`), whatever its text or frontmatter says, exactly as `library/sources/<id>/source.md`
//! is, and reads *not validated* unless the claim's own derived status says more. Being in the index lifts nothing.
//!
//! **What stays out.** Nothing private is listed, so no search, serve or export path can carry it: a claim that rests on a private
//! source (`Library::rests_on_private`), and a topic whose record is private (one of its claims rests on a private source), with
//! its answer. A claim scoped to a project is listed under that project (`research/projects/<id>/claims/`), and one of those that
//! rests on a private or internal source apart (`research/projects/<id>/internal/`): the index's filter shows them to their own
//! project and, where the operator shares research (`specs/115`), to the workspace's other projects, the second kind only to a
//! reader that marks its session. A claim scoped `personal` (a host's claim Delphi could not check, `specs/113`) is not listed
//! at all. A topic that lives in a project's folder is that project's file and is read under the project's
//! rules (the project layer of the index), not here.
//!
//! The records and the claims are JSON; the index holds Markdown. [`text_of`] writes the Markdown the index holds for one, by
//! code, from the file's own fields. No model is called and nothing is fetched.

use crate::kb::SourceFile;
use serde_json::Value;
use std::path::Path;

pub const ZONE: &str = "research";
/// A document the index will not read whole: larger than any record or claim this program writes.
const MAX_BYTES: u64 = 1024 * 1024;
const LINE: usize = 300;
const LIST: usize = 40;

/// The statuses a claim can derive to, as the index stores them (`knowledge_status::Status::as_str`).
pub const STATUSES: [&str; 9] = ["noted", "corroborated", "substantiated", "vetted", "checking", "validated", "contested", "disproved", "retired"];

/// The status a document of the zone reads, from the word the index stored for it: a library claim's own derived status, and
/// *noted* for anything else (a record, an answer, or a word that is not a status).
pub fn status_of(word: Option<&str>) -> delphi_core::knowledge_status::Status {
    word.and_then(delphi_core::knowledge_status::Status::from_word).unwrap_or(delphi_core::knowledge_status::Status::Noted)
}

/// A logical path whose text the index derives with [`text_of`] instead of reading it as Markdown.
pub fn is_derived(rel_path: &str) -> bool {
    rel_path.starts_with("research/") && rel_path.ends_with(".json")
}

fn clip(s: &str, n: usize) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(n).collect::<String>().trim().to_string()
}

fn yaml(s: &str) -> String {
    serde_json::to_string(&clip(s, LINE)).unwrap_or_else(|_| "\"\"".into())
}

/// The research documents the workspace indexes. Listing only reads; nothing is written here.
pub fn files(ws: &Path, out: &mut Vec<SourceFile>) {
    let small = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES);
    let doc = |rel_path: String, path: std::path::PathBuf| SourceFile { rel_path, zone: ZONE, path, default_source: crate::research::TIER, bundle: None, vouched_by: None };
    if let Ok(rd) = std::fs::read_dir(crate::research_record::topics_dir(ws)) {
        for e in rd.flatten() {
            let Some(topic) = crate::research_record::Topic::at(ws, &e.file_name().to_string_lossy()) else { continue };
            // A record that cannot be read, or that is private, lists nothing: neither itself nor its answer.
            let Ok(rec) = topic.read() else { continue };
            if rec["private"] != false || !small(&topic.record_path()) {
                continue;
            }
            out.push(doc(format!("research/topics/{}/record.json", topic.id), topic.record_path()));
            if small(&topic.answer_path()) {
                out.push(doc(format!("research/topics/{}/answer.md", topic.id), topic.answer_path()));
            }
        }
    }
    let Some(home) = ws.parent().and_then(Path::parent) else { return };
    let lib = crate::library::Library::in_home(home);
    let projects = crate::scope::project_entries(ws);
    for c in lib.claims() {
        let Some(id) = c["id"].as_str() else { continue };
        let path = lib.claims_dir().join(format!("{id}.json"));
        if !small(&path) {
            continue;
        }
        // A `personal` claim is this install's alone: a host-reported claim the daemon could not check (`specs/113` §2 items 3, 4).
        // It belongs to no project, so it is listed nowhere and shared with none.
        if c["scope"] == "personal" {
            continue;
        }
        if c["scope"] == "project" {
            // `specs/115` §3 item 4: a project's claim is listed under its project, for that project's own search and, where the
            // operator shares research, for the others (`kb::allowed_sql` decides who reads it; nothing here does).
            if let Some(project) = claim_project(&projects, &c) {
                let dir = if lib.rests_on_internal(&c) { INTERNAL_DIR } else { CLAIMS_DIR };
                out.push(doc(format!("research/projects/{}/{dir}/{id}.json", project.as_str()), path));
            }
            continue;
        }
        if lib.rests_on_private(&c) {
            continue;
        }
        out.push(doc(format!("research/claims/{id}.json"), path));
    }
}

/// Under `research/projects/<id>/`: a project's claims, and apart from them those that rest on a private or internal source
/// (`Library::rests_on_internal`). The directory is the index's record of which is which: it is chosen here, by code, from the
/// library's own source records, and a reader that is given a row of [`INTERNAL_DIR`] marks its session (`specs/115` §2 item 3).
pub const CLAIMS_DIR: &str = "claims";
pub const INTERNAL_DIR: &str = "internal";

/// The project of THIS workspace a project-scoped claim belongs to, or `None`. The library is one per install and a claim names
/// its project by display name, which another workspace's project may share, so the name alone decides nothing: the claim must
/// also name a research topic whose record is on disk under that project's own `research/topics/`. A claim that names no such
/// topic (one the operator added by hand, one of another workspace) is not listed.
fn claim_project(projects: &[crate::scope::ProjectEntry], c: &Value) -> Option<crate::scope::ProjectId> {
    let name = c["project"].as_str().map(str::trim).filter(|n| !n.is_empty())?;
    let topics: Vec<&str> = c["topics"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let mut found = projects.iter().filter(|p| p.name.eq_ignore_ascii_case(name) && topics.iter().any(|t| crate::research_record::Topic::at(&p.root, t).is_some_and(|t| t.read().is_ok())));
    match (found.next(), found.next()) {
        (Some(p), None) => Some(p.id.clone()),
        _ => None,
    }
}

/// The Markdown the index holds for a record or a claim file. `None` when the file is not one (or cannot be read).
pub fn text_of(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if path.file_name().is_some_and(|n| n == "record.json") {
        record_text(&v)
    } else {
        claim_text(&v)
    }
}

/// A library claim as the index holds it: the statement, its derived status, and where it is anchored. The frontmatter's
/// `claim_status` is the derived status; the index reads it for this zone only (`kb.rs`), and only as one of [`STATUSES`].
pub fn claim_text(c: &Value) -> Option<String> {
    let (id, text) = (c["id"].as_str()?, c["text"].as_str()?);
    let status = crate::library::claims::status(c);
    let mut out = format!("---\ntitle: {}\nclaim: {}\nclaim_status: {}\nauthor: scout\nconfidence: low\n---\n\n# Claim: {}\n\n", yaml(text), clip(id, 40), status.as_str(), clip(text, LINE));
    out.push_str(&format!("- status: {} ({})\n", status.as_str(), status.label()));
    out.push_str(&format!("- kind: {}; scope: {}\n", clip(c["kind"].as_str().unwrap_or("fact"), 20), clip(c["scope"].as_str().unwrap_or("team"), 20)));
    let topics: Vec<String> = c["topics"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(|t| clip(t, 60))).take(LIST).collect();
    if !topics.is_empty() {
        out.push_str(&format!("- research topics: {}\n", topics.join(", ")));
    }
    let anchors: Vec<&Value> = c["anchors"].as_array().into_iter().flatten().filter(|a| a["state"].as_str().unwrap_or("active") == "active").take(LIST).collect();
    if !anchors.is_empty() {
        out.push_str("\n## Where it is stated\n\n");
        for a in anchors {
            out.push_str(&format!("- {}: \"{}\" (source {})\n", clip(a["locator"].as_str().unwrap_or(""), LINE), clip(a["quote"].as_str().unwrap_or(""), 400), clip(a["source_id"].as_str().unwrap_or(""), 80)));
        }
    }
    Some(out)
}

/// A topic's record as the index holds it: the question, how far the research got, what was searched and read, and what it did
/// not find. Every line is the record's own data.
pub fn record_text(rec: &Value) -> Option<String> {
    let (id, question) = (rec["topic_id"].as_str()?, rec["question"].as_str()?);
    let n = |k: &str| rec[k].as_array().map_or(0, Vec::len);
    let mut out = format!("---\ntitle: {}\ntopic: {}\nauthor: scout\nconfidence: low\n---\n\n# Research topic: {}\n\n", yaml(question), clip(id, 60), clip(question, LINE));
    out.push_str(&format!("- topic: {}\n- state: {}; depth: {}; runs: {}\n", clip(id, 60), clip(rec["state"].as_str().unwrap_or("?"), 20), clip(rec["depth"].as_str().unwrap_or("?"), 20), n("runs")));
    out.push_str(&format!("- asked by: {} ({})\n", clip(rec["asked_by"]["label"].as_str().unwrap_or("?"), 120), clip(rec["asked_by"]["kind"].as_str().unwrap_or("?"), 20)));
    out.push_str(&format!("- queries: {}; pages read: {}; pages not read: {}; claims: {}\n", n("queries"), n("pages"), n("refused"), n("claims")));
    let claims: Vec<String> = rec["claims"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(|c| clip(c, 40))).take(LIST).collect();
    if !claims.is_empty() {
        out.push_str(&format!("- claim ids: {}\n", claims.join(", ")));
    }
    let section = |out: &mut String, title: &str, lines: Vec<String>| {
        if !lines.is_empty() {
            out.push_str(&format!("\n## {title}\n\n"));
            for l in lines {
                out.push_str(&format!("- {l}\n"));
            }
        }
    };
    section(&mut out, "What it did not find", rec["gaps"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(|g| clip(g, LINE))).take(LIST).collect());
    section(
        &mut out,
        "What was searched",
        rec["queries"].as_array().into_iter().flatten().take(LIST).map(|q| format!("[{}] \"{}\"", clip(q["purpose"].as_str().unwrap_or("?"), 40), clip(q["query"].as_str().unwrap_or(""), LINE))).collect(),
    );
    section(&mut out, "Pages read", rec["pages"].as_array().into_iter().flatten().take(LIST).map(|p| format!("{} (source {})", clip(p["url"].as_str().unwrap_or(""), LINE), clip(p["source_id"].as_str().unwrap_or("-"), 80))).collect());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kb::{Kb, SearchOpts};
    use crate::library::claims::{AnchorSpec, ClaimOpts, ResearchClaim};
    use crate::library::{AddOpts, Budgets, Level, Library};
    use crate::research_record::{Asker, Begin, Topic};
    use serde_json::json;

    struct Fx {
        _root: tempfile::TempDir,
        home: std::path::PathBuf,
        ws: std::path::PathBuf,
    }

    /// An install home with one workspace under it, as the index expects (`<home>/workspaces/<name>`).
    fn fx() -> Fx {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().to_path_buf();
        let ws = home.join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        Fx { _root: root, home, ws }
    }

    fn source(lib: &Library, dir: &Path, name: &str, text: &str, private: bool) -> String {
        let f = dir.join(format!("{name}.md"));
        std::fs::write(&f, text).unwrap();
        lib.add_file(&f, &AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Full), private, ..Default::default() }, &Budgets::default()).unwrap().id
    }

    fn research_claim(lib: &Library, text: &str, fp: &str, project: Option<&str>, topic: Option<&str>, source: &str, quote: &str) -> String {
        let anchors = [AnchorSpec { source_id: source.into(), locator: Some("https://example.org/page".into()), quote: quote.into(), prefix: None, suffix: None }];
        lib.add_research_claim(&ResearchClaim { text, fingerprint: fp, project, topic, anchors: &anchors }).unwrap().0["id"].as_str().unwrap().to_string()
    }

    fn finished_topic(ws: &Path, question: &str, answer: &str, claims: &[String], private: bool) -> Topic {
        let t = Topic::begin(ws, &Begin { question, asked_by: Asker::host("host:claude-code/m"), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-t-1" }).unwrap();
        t.query("r-t-1", "zebrafish husbandry temperature", "topic", Some("brave"), Ok(2)).unwrap();
        t.page("r-t-1", "https://example.org/page", "s-1", Some("sha256:00")).unwrap();
        let result = json!({"answer": answer, "budget_used": {"steps": 2, "check_steps": 1}, "coverage": "thin", "coverage_report": {"thin_below": 3, "counted": 1, "rungs": []}, "search_log": {"version": 1, "private": private, "lines": [], "flow": {}}});
        t.finish("r-t-1", "done", Some(&result), claims, None, 5).unwrap();
        t
    }

    fn search(f: &Fx, query: &str, opts: SearchOpts) -> Vec<crate::kb::Hit> {
        let mut kb = Kb::open(&f.ws.join("local/state.db")).unwrap();
        kb.refresh(&f.ws, "").unwrap();
        kb.search(query, &SearchOpts { rerank_weight: 0.0, ..opts }).unwrap()
    }

    fn zone() -> SearchOpts {
        SearchOpts { zones: Some(vec![ZONE]), limit: 20, ..Default::default() }
    }

    /// AC-2, the index half: after a run that kept claims, a search returns the claim labelled `noted`, the topic's record and its
    /// answer, all in zone `research` at the library's tier.
    #[test]
    fn a_kept_claim_its_topic_and_its_answer_are_found_in_zone_research_and_the_claim_reads_noted() {
        let f = fx();
        let lib = Library::in_home(&f.home);
        let s = source(&lib, &f.home, "tanks", "Zebrafish tanks are held at 28 degrees, the husbandry guide says.", false);
        let t = finished_topic(&f.ws, "What temperature do zebrafish tanks need?", "Sources read say:\n- zebrafish tanks need 28 degrees (\"tanks are held at 28 degrees\") (not validated: model synthesis)", &[], false);
        let id = research_claim(&lib, "zebrafish tanks need 28 degrees", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None, Some(&t.id), &s, "tanks are held at 28 degrees");
        let hits = search(&f, "zebrafish tanks degrees", zone());
        let by_path = |p: &str| hits.iter().find(|h| h.rel_path == p).unwrap_or_else(|| panic!("{p} is not among {:?}", hits.iter().map(|h| &h.rel_path).collect::<Vec<_>>()));
        let claim = by_path(&format!("research/claims/{id}.json"));
        assert_eq!((claim.zone.as_str(), claim.status, claim.label, claim.source_type.as_str()), (ZONE, "noted", "not validated", crate::research::TIER), "a research claim is labelled by its derived status: noted");
        assert!(claim.preview.contains("zebrafish tanks need 28 degrees"), "{}", claim.preview);
        let answer = by_path(&format!("research/topics/{}/answer.md", t.id));
        assert_eq!((answer.zone.as_str(), answer.status, answer.source_type.as_str()), (ZONE, "noted", crate::research::TIER), "the answer's own frontmatter does not set its tier");
        // The record is found by what it holds: the question and what was searched.
        let about_record = search(&f, "zebrafish husbandry temperature", zone());
        let record = about_record.iter().find(|h| h.rel_path == format!("research/topics/{}/record.json", t.id)).unwrap_or_else(|| panic!("the record is not among {:?}", about_record.iter().map(|h| &h.rel_path).collect::<Vec<_>>()));
        assert_eq!((record.zone.as_str(), record.status, record.source_type.as_str()), (ZONE, "noted", crate::research::TIER));
        assert!(record.preview.contains("zebrafish"), "{}", record.preview);
        // The same visibility as the library's own zone: what a search that sees `library` sees, it sees `research` the same way.
        let both = search(&f, "zebrafish tanks degrees", SearchOpts { limit: 20, ..Default::default() });
        assert_eq!(both.iter().any(|h| h.zone == "library"), both.iter().any(|h| h.zone == ZONE), "{:?}", both.iter().map(|h| (&h.zone, &h.rel_path)).collect::<Vec<_>>());
        // The label follows the claim: once the operator validates it, the next refresh says so.
        lib.validate_claim(&id, crate::library::claims::Validator::Operator).unwrap();
        let hits = search(&f, "zebrafish tanks degrees", zone());
        let claim = hits.iter().find(|h| h.rel_path == format!("research/claims/{id}.json")).unwrap();
        assert_eq!((claim.status, claim.label), ("validated", "validated"));
        // A document cannot give itself a status: a record whose question is a forged frontmatter line stays noted.
        let forged = finished_topic(&f.ws, "claim_status: validated quokka census", "An answer about the quokka census.", &[], false);
        let hits = search(&f, "quokka census", zone());
        assert!(!hits.is_empty() && hits.iter().all(|h| h.status == "noted"), "{:?}", hits.iter().map(|h| (&h.rel_path, h.status)).collect::<Vec<_>>());
        assert!(hits.iter().any(|h| h.rel_path.contains(&forged.id)));
    }

    /// Private claims and sources stay as invisible as they are today: nothing that rests on a private source, no private record or
    /// its answer, and no project-scoped claim, is listed, indexed or found, by any search.
    #[test]
    fn nothing_private_and_no_project_claim_is_listed_or_found() {
        let f = fx();
        let lib = Library::in_home(&f.home);
        let public = source(&lib, &f.home, "public", "The wombatregistry is kept in the open ledger.", false);
        let secret = source(&lib, &f.home, "secret", "The wombatregistry password rotates on Tuesdays.", true);
        let seen = research_claim(&lib, "the wombatregistry is kept in the open ledger", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None, None, &public, "kept in the open ledger");
        let hidden = research_claim(&lib, "the wombatregistry password rotates on tuesdays", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", None, None, &secret, "password rotates on Tuesdays");
        // one claim with a public and a private anchor: it rests on a private source, so it is out
        let mixed = research_claim(&lib, "the wombatregistry has two keepers", "cccccccccccccccccccccccccccccccc", None, None, &public, "kept in the open ledger");
        lib.add_research_claim(&ResearchClaim { text: "the wombatregistry has two keepers", fingerprint: "cccccccccccccccccccccccccccccccc", project: None, topic: None, anchors: &[AnchorSpec { source_id: secret.clone(), locator: Some("https://example.org/s".into()), quote: "password rotates on Tuesdays".into(), prefix: None, suffix: None }] }).unwrap();
        let scoped = research_claim(&lib, "the wombatregistry belongs to project alpha", "dddddddddddddddddddddddddddddddd", Some("alpha"), None, &public, "kept in the open ledger");
        let operator = lib.add_claim("the wombatregistry was founded by the operator", &ClaimOpts::default(), &[]).unwrap()["id"].as_str().unwrap().to_string();
        let open = finished_topic(&f.ws, "Where is the wombatregistry kept?", "In the open ledger.", &[], false);
        let closed = finished_topic(&f.ws, "What is the wombatregistry password schedule?", "It rotates on Tuesdays.", &[], true);
        let mut listed = Vec::new();
        files(&f.ws, &mut listed);
        let paths: Vec<&str> = listed.iter().map(|s| s.rel_path.as_str()).collect();
        for id in [&seen, &operator] {
            assert!(paths.contains(&format!("research/claims/{id}.json").as_str()), "{id}: {paths:?}");
        }
        for id in [&hidden, &mixed, &scoped] {
            assert!(!paths.iter().any(|p| p.contains(id.as_str())), "{id} must not be listed: {paths:?}");
        }
        assert!(paths.iter().any(|p| p.contains(&open.id)) && !paths.iter().any(|p| p.contains(&closed.id)), "{paths:?}");
        assert!(listed.iter().all(|s| s.zone == ZONE && s.default_source == crate::research::TIER && s.vouched_by.is_none()));
        // And by search, with every door open that a caller can open.
        let wide = SearchOpts { limit: 50, include_untrusted: true, include_staged: true, include_attributed: true, ..Default::default() };
        let hits = search(&f, "wombatregistry password rotates Tuesdays keepers alpha", wide);
        let text: String = hits.iter().map(|h| format!("{} {}\n", h.rel_path, h.preview)).collect();
        for needle in ["password", "Tuesdays", "two keepers", "project alpha", hidden.as_str(), mixed.as_str(), scoped.as_str(), closed.id.as_str()] {
            assert!(!text.contains(needle), "`{needle}` reached a search:\n{text}");
        }
        assert!(text.contains("open ledger"), "{text}");
        // A claim that comes to rest on a private source leaves the index at the next refresh.
        lib.add_research_claim(&ResearchClaim { text: "the wombatregistry is kept in the open ledger", fingerprint: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", project: None, topic: None, anchors: &[AnchorSpec { source_id: secret, locator: Some("https://example.org/s".into()), quote: "password rotates on Tuesdays".into(), prefix: None, suffix: None }] }).unwrap();
        let hits = search(&f, "wombatregistry open ledger", zone());
        assert!(!hits.iter().any(|h| h.rel_path.contains(seen.as_str())), "{:?}", hits.iter().map(|h| &h.rel_path).collect::<Vec<_>>());
    }

    #[test]
    fn the_text_the_index_holds_is_built_from_the_files_own_fields_and_carries_no_control_characters() {
        let c = json!({"id": "c-0123456789ab", "text": "aspirin lowers fever\u{1b}[31m", "kind": "fact", "scope": "team", "topics": ["does-aspirin-lower-fever-deadbeef"], "support": {"origins": 2, "trusted_sources": 1},
            "anchors": [{"source_id": "s-1", "locator": "https://who.int/a", "quote": "lowers fever\nin adults", "state": "active"}, {"source_id": "s-2", "locator": "https://x.example/b", "quote": "gone", "state": "lost"}]});
        let t = claim_text(&c).unwrap();
        assert!(t.starts_with("---\ntitle: \"aspirin lowers fever [31m\"\nclaim: c-0123456789ab\nclaim_status: noted\n"), "{t}");
        assert!(t.contains("- status: noted (not validated)") && t.contains("- research topics: does-aspirin-lower-fever-deadbeef"), "{t}");
        assert!(!t.contains("origins"), "the count of origins is the library's to show; the index text carries the status and no count: {t}");
        assert!(t.contains("- https://who.int/a: \"lowers fever in adults\" (source s-1)") && !t.contains("gone"), "an anchor that was lost is not shown: {t}");
        assert!(!t.contains('\u{1b}'));
        assert!(claim_text(&json!({"id": "c-1"})).is_none() && record_text(&json!({"question": "q"})).is_none());
        let r = json!({"topic_id": "q-deadbeef", "question": "A question?", "state": "partial", "depth": "survey", "asked_by": {"kind": "host", "label": "host:x/y"}, "runs": [{}], "claims": ["c-1"], "gaps": ["kb: kb_empty"],
            "queries": [{"purpose": "trusted", "query": "a question site:who.int"}], "pages": [{"url": "https://who.int/a", "source_id": "s-1"}], "refused": []});
        let t = record_text(&r).unwrap();
        for needle in ["# Research topic: A question?", "- state: partial; depth: survey; runs: 1", "- claim ids: c-1", "## What it did not find", "- kb: kb_empty", "- [trusted] \"a question site:who.int\"", "- https://who.int/a (source s-1)"] {
            assert!(t.contains(needle), "{needle}\n{t}");
        }
        // The stored word is read back as the status it names, and anything else is noted.
        for w in STATUSES {
            assert_eq!(status_of(Some(w)).as_str(), w);
        }
        for w in [None, Some(""), Some("ratified"), Some("VALIDATED"), Some("validated ")] {
            assert_eq!(status_of(w).as_str(), "noted", "{w:?}");
        }
        assert!(is_derived("research/claims/c-1.json") && is_derived("research/topics/q-deadbeef/record.json"));
        assert!(!is_derived("research/topics/q-deadbeef/answer.md") && !is_derived("context/data.json") && !is_derived("library/claims/c-1.json"));
    }
}
