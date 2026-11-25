//! A topic page's material through the retrieval service (`specs/117` §2 item 4; slice E2).
//!
//! A topic page is gathered from the whole corpus, not only the library: the library's claims and sources as before, research answers,
//! findings (`specs/116` §5) and knowledge-base passages, each as a record the page may cite. One search of the index (`retrieval::search`,
//! read-only, the operator's view with untrusted documents included) finds them; a hit is then sorted by where it lives:
//!
//! * a library source's text (`library/sources/<id>/source.md`) joins the page's sources as `s-<id>`;
//! * a library claim of zone `research` (`research/claims/<id>.json`) joins its claims as `c-<id>`, if it is live and rests on no
//!   private source;
//! * a research answer (`research/topics/<id>/answer.md`) and any other passage become `[s-kb-<16 hex>]` records, a finding
//!   (zone `findings`) an `[f-<16 hex>]` record. The label is a hash of where the passage is, so it is the same from one build to the
//!   next; the record keeps a hash of the passage's text, so a later due check sees it change.
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

/// Sort the hits of one search into `g` (see the module's comment). `texts` are the hits' passages, in order.
pub fn sort_hits(lib: &Library, g: &mut Gathered, hits: &[Hit], texts: &[String]) {
    for (h, text) in hits.iter().zip(texts) {
        if h.internal || h.project.is_some() || h.shared {
            continue;
        }
        let rel = h.rel_path.as_str();
        if rel.starts_with("library/pages/") {
            continue;
        }
        if let Some(id) = rel.strip_prefix("library/sources/").and_then(|r| r.strip_suffix("/source.md")) {
            if g.sources.len() < MAX_SOURCES + 2 && !g.sources.iter().any(|(s, _, _)| s == id) {
                if let Some(meta) = lib.read_meta(id).filter(|m| !is_private(m) && m["purged"].is_null() && m["retired"].is_null()) {
                    g.sources.push((id.to_string(), meta, clip(text, RECORD_TEXT)));
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
        }
        if matches!(h.zone.as_str(), "staged" | "episodes" | "xenia") || h.zone == crate::provisional_notes::ZONE || g.records.len() >= MAX_RECORDS {
            continue;
        }
        let finding = h.zone == crate::finding_record::ZONE;
        let label = label_of(finding, rel, h.start_line);
        if g.records.iter().any(|r| r.label == label) || text.trim().is_empty() {
            continue;
        }
        let kind = if finding { "finding" } else if research { "research answer" } else { "passage" };
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
    sort_hits(lib, &mut g, &found.hits, &texts);
    g
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
            for (i, l) in lines.iter().enumerate() {
                let Some(id) = l.strip_prefix("RECORD ").and_then(|r| r.split(' ').next()) else { continue };
                if id.starts_with("c-") {
                    out.push_str(&format!("{} [{}].\n\n", lines.get(i + 1).unwrap_or(&"").trim_end_matches('.'), id));
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
        fn validate<'a>(&'a self, _q: &'a PageQuery) -> ValidatorFuture<'a> {
            Box::pin(async move { Ok(ValidatorReply { assertions: vec![Assertion { text: "x".into(), judgement: Judgement::Supported, record: None }], model: "m".into(), family: "f".into(), tokens: 10 }) })
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

        let topic_lib = Library::in_home(home.path()).with_records(g.records.clone());
        let cfg = delphi_adapter::config::PagesConfig::default();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let built = rt.block_on(topic_lib.topic_rebuild(TopicRequest { slug: &e.slug, now: 100, settings: &cfg, style: crate::library::bib::Format::Plain, gathered: g, gaps: vec![], seed: Default::default(), held: &crate::topic_seed::NoHeldText, cancel: None }, &CiteAll, &Pass, &TokenLedger::new(1_000_000), &|| Ok(()))).unwrap();
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
