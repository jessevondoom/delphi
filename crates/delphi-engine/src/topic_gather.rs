//! A topic page's material through the retrieval service (`specs/117` §2 item 4; slice E2).
//!
//! A topic page is gathered from the whole corpus, not only the library: the library's claims and sources as before, research answers,
//! findings (`specs/116` §5) and knowledge-base passages, each as a record the page may cite. One search of the index (`retrieval::search`,
//! read-only, the operator's view with untrusted documents included) finds them; a hit is then sorted by where it lives:
//!
//! * a library source's text (`library/sources/<id>/source.md`) joins the page's sources as `s-<id>`;
//! * a library claim of zone `research` (`research/claims/<id>.json`) joins its claims as `c-<id>`, if it is live and rests on no
//!   private source;
//! * any other passage becomes an `[s-kb-<16 hex>]` record, a finding (zone `findings`) an `[f-<16 hex>]` record. The label is a hash
//!   of where the passage is, so it is the same from one build to the next; the record keeps a hash of the passage's text, so a later
//!   due check sees it change;
//! * a research answer (`research/topics/<id>/answer.md`) is an outline, not a record (`specs/144` §2.1 item 3): the claims its markers
//!   name join the claims, and its prose is cited nowhere;
//! * a library source without citable text (`specs/144` §2.1 item 1) is named under the titles read, never a record.
//!
//! **Research topics leave out the repository** (`specs/144` §2.1 item 4): for a topic made from research, a repository file (a
//! registered project's file outside its `research/` folder) is never gathered. A topic the operator made keeps them.
//!
//! **Left out:** a topic record's own JSON, staged notes, episodes and provisional notes (the search does not ask for them), a hit of
//! another project, and a hit the index flags `internal` (a claim resting on a private or internal source, which a session must be
//! marked for: the writer is not a session, so it is not given one). The index lists no private library source. Untrusted documents
//! are gathered and shown with their status (`○ not validated`), as the spec says; the status comes from the index, never from the
//! passage's words. A generated page is never in the index, so pages are not gathered into pages.

use crate::kb::{Hit, SearchOpts};
use crate::library::page_search::{Gathered, MAX_CLAIMS, MAX_SOURCES};
use crate::library::pages::{record_status, KbRecord};
use crate::library::{is_private, Library};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The most records of the index a page is given.
pub const MAX_RECORDS: usize = 8;
/// How many results the one search asks for.
const SEARCH_LIMIT: usize = 20;
/// A record's text as the writer and the check see it.
const RECORD_TEXT: usize = 3 * 1024;
const TITLE_MAX: usize = 200;

/// The index a workspace's searches read.
pub fn db_of(ws: &Path) -> PathBuf {
    ws.join("local/state.db")
}

fn sha16(s: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(s.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The label of the record for the passage at `rel_path` from `start_line`: `f-…` for a finding, `s-kb-…` for anything else.
pub fn label_of(finding: bool, rel_path: &str, start_line: usize) -> String {
    let h = sha16(&format!("{}#L{}", rel_path, start_line));
    if finding { format!("f-{h}") } else { format!("s-kb-{h}") }
}

/// The hash a record keeps of its passage's whole text.
pub fn text_hash(text: &str) -> String {
    sha16(text)
}

/// The hash of the passage a record names, as the index holds it now; `None` when the index has no such passage.
pub fn record_hash_now(ws: &Path, rel_path: &str, start_line: usize) -> Option<String> {
    let kb = crate::kb::Kb::open_readonly(&db_of(ws)).ok()?;
    kb.chunk_content(rel_path, start_line).map(|(_, t)| text_hash(&t))
}

fn clip(s: &str, n: usize) -> String {
    let t: String = s.chars().map(|c| if c.is_control() && c != '\n' { ' ' } else { c }).collect();
    t.chars().take(n).collect::<String>().trim().to_string()
}

/// The claim id of a library claim's document in zone `research` (`research/claims/<id>.json`, or a project's).
fn claim_of(rel: &str) -> Option<&str> {
    let (dir, file) = rel.rsplit_once('/')?;
    (dir.ends_with("/claims") || dir == "research/claims").then_some(())?;
    file.strip_suffix(".json").filter(|id| id.starts_with("c-"))
}

/// Is `rel_path` a repository file (`specs/144` §2.1 item 4): a file of a registered project as the index names it
/// (`projects/<id>/files/…`, or `@<project>/…`), outside the project's `research/` folder? A research topic's page takes no evidence
/// from one.
pub fn is_repository_file(rel_path: &str) -> bool {
    let inside = if let Some(rest) = rel_path.strip_prefix("projects/") {
        rest.split_once('/').and_then(|(_, r)| r.strip_prefix("files/"))
    } else if let Some(rest) = rel_path.strip_prefix('@') {
        rest.split_once('/').map(|(_, r)| r)
    } else {
        None
    };
    inside.is_some_and(|r| !(r == "research" || r.starts_with("research/")))
}

/// Was this topic made from research (`Origin::Research`)? Its page leaves the repository out (`specs/144` §2.1 item 4).
pub fn research_origin(side: &Value) -> bool {
    side["origin"]["kind"] == "research"
}

/// The claim ids a research answer's own markers name (`[c-…]`, or `(c-…)` once a seed has cleaned it): an answer is an outline, and
/// what a page may cite of it is the claims it points to (`specs/144` §2.1 item 3).
pub fn claim_ids_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, _) in text.match_indices("c-") {
        let before = text[..i].chars().next_back();
        if !matches!(before, Some('[' | '(' | ',' | ';' | ' ')) {
            continue;
        }
        let id: String = text[i..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        let id = id.trim_end_matches('-').to_string();
        let closed = text[i + id.len()..].starts_with([']', ')', ',', ';']);
        if id.len() > 2 && closed && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Sort the hits of one search into `g` (see the module's comment). `texts` are the hits' passages, in order. For a topic made from
/// research (`research_topic`), a repository file is left out (`specs/144` §2.1 item 4).
pub fn sort_hits(lib: &Library, g: &mut Gathered, hits: &[Hit], texts: &[String], research_topic: bool) {
    for (h, text) in hits.iter().zip(texts) {
        if h.internal || h.project.is_some() || h.shared {
            continue;
        }
        let rel = h.rel_path.as_str();
        if rel.starts_with("library/pages/") || (research_topic && is_repository_file(rel)) {
            continue;
        }
        if let Some(id) = rel.strip_prefix("library/sources/").and_then(|r| r.strip_suffix("/source.md")) {
            if g.sources.len() < MAX_SOURCES + 2 && !g.sources.iter().any(|(s, _, _)| s == id) {
                if let Some(meta) = lib.read_meta(id).filter(|m| !is_private(m) && m["purged"].is_null() && m["retired"].is_null()) {
                    // `specs/144` §2.1: a source without citable text is read for its title only.
                    if !lib.citable_logged(id) {
                        g.read_title_only(id, meta["title"].as_str().unwrap_or(id));
                        continue;
                    }
                    g.sources.push((id.to_string(), meta, clip(text, RECORD_TEXT)));
                    // a page build gathered it: a use (`specs/142` §2.4 item 2)
                    lib.note_use(&[id], delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap_or(0));
                }
            }
            continue;
        }
        let research = h.zone == crate::research_index::ZONE;
        if research {
            if let Some(cid) = claim_of(rel) {
                let live = |c: &Value| !matches!(crate::library::claims::status(c), delphi_core::knowledge_status::Status::Disproved | delphi_core::knowledge_status::Status::Retired) && c["superseded_by"].is_null();
                if g.claims.len() < 2 * MAX_CLAIMS && !g.claims.iter().any(|c| c["id"] == cid) {
                    if let Some(c) = lib.claim(cid).filter(|c| live(c) && !lib.rests_on_private(c)) {
                        g.claims.push(c);
                    }
                }
                continue;
            }
            if !rel.ends_with("/answer.md") {
                continue;
            }
            // A research answer is an outline, not a citable record (`specs/144` §2.1 item 3): its claim markers are followed to the
            // claims, and its own prose is cited nowhere.
            follow_claims(lib, g, text);
            continue;
        }
        if matches!(h.zone.as_str(), "staged" | "episodes" | "xenia") || h.zone == crate::provisional_notes::ZONE || g.records.len() >= MAX_RECORDS {
            continue;
        }
        let finding = h.zone == crate::finding_record::ZONE;
        let label = label_of(finding, rel, h.start_line);
        if g.records.iter().any(|r| r.label == label) || text.trim().is_empty() {
            continue;
        }
        let kind = if finding { "finding" } else { "passage" };
        let title = if h.heading_path.trim().is_empty() { rel.to_string() } else { h.heading_path.clone() };
        g.records.push(KbRecord {
            label,
            kind: kind.into(),
            title: clip(&title, TITLE_MAX),
            locator: clip(&h.pointer, TITLE_MAX),
            origin: format!("{} ({})", h.zone, h.source_type),
            status: record_status(h.status),
            text: clip(text, RECORD_TEXT),
            hash: text_hash(text),
            rel_path: h.rel_path.clone(),
            start_line: h.start_line,
        });
    }
}

/// A topic's whole material: the library's ([`Library::topic_gather_library`]) and what one search of the workspace's index adds.
/// With no index, or a search that fails, it is the library's alone. Reads only.
pub fn gather(lib: &Library, home: &Path, ws: &Path, side: &Value) -> Gathered {
    let mut g = lib.topic_gather_library(side, Some(ws));
    let db = db_of(ws);
    let query = side["query"].as_str().or(side["title"].as_str()).unwrap_or("").trim().to_string();
    if !db.is_file() || query.is_empty() {
        return g;
    }
    let opts = SearchOpts { limit: SEARCH_LIMIT, include_untrusted: true, ..Default::default() };
    let Ok(found) = crate::retrieval::search(home, &db, &query, opts) else { return g };
    let texts = crate::retrieval::passages(&db, &found.hits);
    sort_hits(lib, &mut g, &found.hits, &texts, research_origin(side));
    g
}

/// Add to `g` the live claims a research answer's markers name, resting on no private source (`specs/144` §2.1 item 3).
pub fn follow_claims(lib: &Library, g: &mut Gathered, answer: &str) {
    let live = |c: &Value| !matches!(crate::library::claims::status(c), delphi_core::knowledge_status::Status::Disproved | delphi_core::knowledge_status::Status::Retired) && c["superseded_by"].is_null();
    for id in claim_ids_in(answer) {
        if g.claims.len() >= 2 * MAX_CLAIMS || g.claims.iter().any(|c| c["id"] == id.as_str()) {
            continue;
        }
        if let Some(c) = lib.claim(&id).filter(|c| live(c) && !lib.rests_on_private(c)) {
            g.claims.push(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact_judge::TokenLedger;
    use crate::library::page_validate::*;
    use crate::library::topics::{Origin, TopicRequest};

    /// Cites every record of the brief once: a claim with its own words, a passage or a finding with a plain sentence.
    struct CiteAll;
    impl PageWriter for CiteAll {
        fn write<'a>(&'a self, b: &'a str) -> WriterFuture<'a> {
            let mut out = String::from("# Tides\n\n");
            let lines: Vec<&str> = b.lines().collect();
            // A finding is not source text (`specs/144` §2.3 item 4): its statement also cites a passage, whose text supports it.
            let passage = lines.iter().filter_map(|l| l.strip_prefix("RECORD s-kb-").and_then(|r| r.split(' ').next())).next().map(|h| format!("s-kb-{h}"));
            for (i, l) in lines.iter().enumerate() {
                let Some(id) = l.strip_prefix("RECORD ").and_then(|r| r.split(' ').next()) else { continue };
                if id.starts_with("c-") {
                    out.push_str(&format!("{} [{}].\n\n", lines.get(i + 1).unwrap_or(&"").trim_end_matches('.'), id));
                } else if let (true, Some(p)) = (id.starts_with("f-"), &passage) {
                    out.push_str(&format!("The workspace has a note that bears on this [{}, {}].\n\n", id, p));
                } else {
                    out.push_str(&format!("The workspace has a note that bears on this [{}].\n\n", id));
                }
            }
            Box::pin(async move { Ok(out) })
        }
        fn rewrite<'a>(&'a self, d: &'a str, _f: &'a [String]) -> WriterFuture<'a> {
            let d = d.to_string();
            Box::pin(async move { Ok(d) })
        }
    }
    struct Pass;
    impl PageValidator for Pass {
        fn validate<'a>(&'a self, q: &'a PageQuery) -> ValidatorFuture<'a> {
            let assertions = crate::library::page_validate::support_all(q);
            Box::pin(async move { Ok(ValidatorReply { assertions, model: "m".into(), family: "f".into(), tokens: 10 }) })
        }
    }

    /// AC-2: a topic page gathers a knowledge-base passage and a finding through the retrieval service and cites them; the check
    /// accepts the markers, the bibliography has a row for each, and a library without the records refuses the same markers.
    #[test]
    fn a_topic_page_gathers_and_cites_a_passage_and_a_finding() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        {
            let mut kb = crate::kb::Kb::open(&db_of(&ws)).unwrap();
            kb.upsert_from("context/estuary.md", "context", "external_untrusted", "# Estuary tides\n\nThe estuary tides turn twice a day at the harbour wall.\n", "t").unwrap();
            kb.upsert_from("missions/m1/findings/why-estuary-tides-lag.md", crate::finding_record::ZONE, "internal_generated", "---\nkind: finding\nstatus: active\n---\n# Why do estuary tides lag?\n\nConclusion (likely): the estuary tides lag the open sea by an hour.\n", "t").unwrap();
            kb.upsert_from("local/staged/n1.md", "staged", "external_untrusted", "# Estuary tides staged\n\nA staged note about estuary tides.\n", "t").unwrap();
        }
        let lib = Library::in_home(home.path());
        let e = lib.topic_create("Estuary tides", Origin::Operator, 1).unwrap();
        let side = lib.topic_side(&e.slug).unwrap();
        let g = gather(&lib, home.path(), &ws, &side);
        let kinds: Vec<&str> = g.records.iter().map(|r| r.kind.as_str()).collect();
        assert!(kinds.contains(&"passage") && kinds.contains(&"finding"), "{:?}", g.records);
        assert!(g.records.iter().all(|r| r.rel_path != "local/staged/n1.md"), "a staged note is never gathered");
        let passage = g.records.iter().find(|r| r.kind == "passage").unwrap().clone();
        let finding = g.records.iter().find(|r| r.kind == "finding").unwrap().clone();
        assert!(passage.label.starts_with("s-kb-") && finding.label.starts_with("f-"));
        assert_eq!(passage.status, "○ not validated", "untrusted is gathered, shown with its status");
        assert_eq!(label_of(false, "context/estuary.md", passage.start_line), passage.label, "the label is the passage's place");

        let topic_lib = Library::in_home(home.path()).with_workspace(&ws).with_records(g.records.clone());
        let cfg = delphi_adapter::config::PagesConfig::default();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let built = rt.block_on(topic_lib.topic_rebuild(TopicRequest { slug: &e.slug, now: 100, settings: &cfg, style: crate::library::bib::Format::Plain, seed: crate::topic_seed::of_material(&g), gathered: g, gaps: vec![], held: &crate::topic_seed::NoHeldText, cancel: None }, &CiteAll, &Pass, &TokenLedger::new(1_000_000), &|| Ok(()))).unwrap();
        assert_eq!(built.outcome.as_deref(), Some("pass"), "{built:?}");
        assert!(built.cited.contains(&passage.label) && built.cited.contains(&finding.label), "{:?}", built.cited);
        let md = std::fs::read_to_string(lib.root().join(format!("pages/{}.md", e.slug))).unwrap();
        assert!(md.contains(&format!("| {} | ○ not validated |", passage.label)) && md.contains(&format!("| {} |", finding.label)), "{md}");
        // The stored page checks clean on a library given the records, and not on one that was not.
        assert!(topic_lib.check_page_text(&md, crate::library::pages::Scope::External).findings.is_empty());
        let bare = lib.check_page_text(&md, crate::library::pages::Scope::External).findings;
        assert!(bare.iter().any(|f| matches!(f, crate::library::pages::Finding::UnknownRecord(id) if *id == passage.label)), "{bare:?}");
        // The record's text changing in the index makes the page due.
        let ctx = crate::library::topics::DueCtx { ws: Some(&ws), now: 101, index: true };
        assert_eq!(lib.topic_pages(&ctx)[0].due, None);
        {
            let mut kb = crate::kb::Kb::open(&db_of(&ws)).unwrap();
            kb.upsert_from("context/estuary.md", "context", "external_untrusted", "# Estuary tides\n\nThe estuary tides now turn three times a day.\n", "t").unwrap();
        }
        assert!(lib.topic_pages(&ctx)[0].due.as_deref().unwrap_or("").contains(&passage.label));
    }
}

/// `specs/144` slice S0: only text can be cited (§2.1).
#[cfg(test)]
mod citable_tests {
    use super::*;
    use crate::library::page_search::{brief_for, SOURCES_READ};
    use crate::library::pages::{Finding, Scope};
    use crate::library::{AddOpts, Budgets};
    use serde_json::json;

    const SOFT_404: &str = "# Page not found\n\n[Home](/) [Search](/search)\n\nSorry, the page you are looking for could not be found about estuary tides. It may have been moved.\n";
    const ARTICLE: &str = "# Estuary tides\n\nThe estuary tides turn later than on the open coast. Engineers measured a lag of about an hour at the harbour wall.\n";

    fn add(lib: &Library, dir: &Path, name: &str, text: &str) -> String {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        lib.add_file(&p, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id
    }

    /// AC-1 and AC-2: a source whose text is a soft 404 is in no `RECORD`; it is listed under `SOURCES READ` by its title, its exclusion
    /// is logged with the reason, and a draft that cites it fails the page check with `cited a source with no text`. An article is a
    /// record, and a draft citing it passes.
    #[test]
    fn a_source_without_citable_text_is_listed_by_title_and_cannot_be_cited() {
        let home = tempfile::tempdir().unwrap();
        let lib = Library::in_home(home.path());
        let bad = add(&lib, home.path(), "missing.md", SOFT_404);
        let good = add(&lib, home.path(), "article.md", ARTICLE);
        let g = lib.gather("estuary tides", Scope::External, &[]);
        assert!(g.sources.iter().any(|(id, _, _)| *id == good) && !g.sources.iter().any(|(id, _, _)| *id == bad), "{:?}", g.sources);
        assert_eq!(g.titles_only.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), vec![bad.as_str()]);
        let brief = brief_for("estuary tides", &g, 3000, crate::library::page_meta::Depth::Summary);
        assert!(!brief.contains(&format!("RECORD s-{bad}")) && brief.contains(&format!("RECORD s-{good}")), "{brief}");
        let listed = &brief[brief.find(SOURCES_READ).expect("the titles are listed")..];
        assert!(listed.contains("- missing.md"), "{listed}");
        let log = std::fs::read_to_string(lib.root().join("log.jsonl")).unwrap();
        assert!(log.contains("\"source_not_citable\"") && log.contains("soft 404") && log.contains(&bad), "{log}");
        // The page check.
        let cited = lib.check_body(&format!("# Estuary tides\n\nThe tides lag [s-{bad}].\n"), Scope::External);
        assert!(cited.findings.iter().any(|f| matches!(f, Finding::NoText(id) if *id == format!("s-{bad}"))), "{:?}", cited.findings);
        assert!(cited.findings.iter().any(|f| f.to_string().starts_with("cited a source with no text")));
        assert!(lib.check_body(&format!("# Estuary tides\n\nThe tides lag [s-{good}].\n"), Scope::External).findings.is_empty());
    }

    /// AC-1: a pointer with only a summary has no citable text: it is read for its title only, and cannot be cited.
    #[test]
    fn a_pointer_with_only_a_summary_is_read_for_its_title_only() {
        let home = tempfile::tempdir().unwrap();
        let lib = Library::in_home(home.path());
        let id = add(&lib, home.path(), "p.md", ARTICLE);
        std::fs::remove_file(lib.source_dir(&id).join("source.md")).unwrap();
        std::fs::write(lib.source_dir(&id).join(crate::source_summary::SUMMARY_MD), "The estuary tides lag, a model says.\n").unwrap();
        let why = lib.citable(&id).unwrap_err();
        assert!(why.contains("a summary is not citable text"), "{why}");
        assert!(lib.check_body(&format!("# T\n\nTides lag [s-{id}].\n"), Scope::External).findings.iter().any(|f| matches!(f, Finding::NoText(_))));
    }

    /// AC-3: a research answer is no record a written page may cite; the claims its markers name are.
    #[test]
    fn a_research_answer_is_an_outline_and_its_claim_markers_reach_the_claims() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let lib = Library::in_home(home.path()).with_workspace(&ws);
        let src = add(&lib, home.path(), "a.md", ARTICLE);
        let quote = "Engineers measured a lag of about an hour at the harbour wall.";
        let claim = lib.add_claim(quote, &Default::default(), &[crate::library::claims::AnchorSpec { source_id: src.clone(), locator: Some("¶1".into()), quote: quote.into(), prefix: None, suffix: None, read: None }]).unwrap();
        let cid = claim["id"].as_str().unwrap().to_string();
        let t = crate::research_record::Topic::begin(&ws, &crate::research_record::Begin { question: "Why do estuary tides lag?", asked_by: crate::research_record::Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap();
        t.finish("r-1", "done", Some(&json!({"answer": format!("Tides lag by an hour [{cid}]. The channel is shallow, which slows the flood."), "claims": []})), &[], None, 0).unwrap();
        let slug = lib.topics_from_research(&ws, 1).remove(0);
        let side = lib.topic_side(&slug).unwrap();
        let seed = crate::topic_seed::assemble_linked(&lib, &ws, &side);
        let answer = seed.spans.iter().find(|s| s.row == crate::topic_seed::Row::Answer).expect("the answer is in the seed").label.clone();
        assert!(seed.records().iter().any(|r| r.label == answer), "the seed page still cites its answer");
        assert!(!seed.citable_records().iter().any(|r| r.label == answer), "a written page may not");
        assert_eq!(seed.answer_claims(), vec![cid.clone()]);
        let g = lib.expansion_material(&seed, Gathered::default());
        assert!(g.claims.iter().any(|c| c["id"] == cid.as_str()) && !g.records.iter().any(|r| r.label == answer), "{:?}", g.records);
        // A draft citing the answer's prose names no record the page may cite.
        let check = Library::in_home(home.path()).with_records(g.records.clone()).check_body(&format!("# T\n\nThe channel is shallow [{answer}].\n"), Scope::External);
        assert!(check.findings.iter().any(|f| matches!(f, Finding::UnknownRecord(id) if *id == answer)), "{:?}", check.findings);
        // A hit on the answer in the index adds its claims, and no record.
        let mut g = Gathered::default();
        sort_hits(&lib, &mut g, &[hit("research/topics/t1/answer.md", crate::research_index::ZONE)], &[format!("Tides lag [{cid}].")], true);
        assert!(g.records.is_empty() && g.claims.iter().any(|c| c["id"] == cid.as_str()), "{:?}", g.records);
    }

    fn hit(rel: &str, zone: &str) -> Hit {
        Hit { pointer: format!("{rel}#L1-L2"), rel_path: rel.into(), start_line: 1, end_line: 2, heading_path: "Notes".into(), blake3: String::new(), preview: String::new(), score: 1.0, note: None, status: "not validated", label: "not validated", vouched_by: None, zone: zone.into(), source_type: "external_untrusted".into(), doc_status: String::new(), doc_author: None, project: None, shared: false, internal: false, confirmed_by: None, disputed: Vec::new(), marks: Default::default() }
    }

    /// AC-4: a research topic takes no passage from a repository file (a registered project's file outside `research/`); the project's
    /// research files stay. A topic the operator made keeps repository files.
    #[test]
    fn a_research_topic_leaves_out_repository_files_and_an_operator_topic_keeps_them() {
        assert!(is_repository_file("projects/p-1/files/src/main.md") && is_repository_file("@delphi/specs/001.md"));
        assert!(!is_repository_file("projects/p-1/files/research/notes.md") && !is_repository_file("@delphi/research/topics/t/answer.md"));
        assert!(!is_repository_file("context/estuary.md") && !is_repository_file("projects/p-1/layer/context/a.md"));
        let home = tempfile::tempdir().unwrap();
        let lib = Library::in_home(home.path());
        let hits = [hit("projects/p-1/files/specs/tides.md", "project"), hit("projects/p-1/files/research/tides.md", "project")];
        let texts = ["The repository's spec says the tides lag.".to_string(), "The project's research says the tides lag.".to_string()];
        let mut research = Gathered::default();
        sort_hits(&lib, &mut research, &hits, &texts, true);
        assert_eq!(research.records.iter().map(|r| r.rel_path.as_str()).collect::<Vec<_>>(), vec!["projects/p-1/files/research/tides.md"]);
        let mut operator = Gathered::default();
        sort_hits(&lib, &mut operator, &hits, &texts, false);
        assert_eq!(operator.records.len(), 2);
        // Which topics are research topics.
        let op = lib.topic_create("Estuary tides", crate::library::topics::Origin::Operator, 1).unwrap();
        assert!(!research_origin(&lib.topic_side(&op.slug).unwrap()));
        let rs = lib.topic_create("Harbour tides", crate::library::topics::Origin::Research("t-1".into()), 1).unwrap();
        assert!(research_origin(&lib.topic_side(&rs.slug).unwrap()));
    }
}
