//! Tests of held text and its one predicate (`specs/142` slice B: AC-8 to AC-10, AC-27, AC-31; AC-9 blocks the slice's merge).

use super::*;
use crate::library::{AddOpts, Budgets, Channel, Library};
use serde_json::json;

/// A held source's text: long enough that its aggregate cap is 1,200 characters (14,800 characters, a tenth is 1,480).
fn long_text() -> String {
    (0..400).map(|i| format!("Held sentence {i:03} of the tides runs on. ")).collect()
}

/// Keep `paragraphs` as a held source through the library's own capture path (`local_text`), as a research run does. Returns its id
/// and the text the library stored.
fn held_source(lib: &Library, url: &str, paragraphs: &[&str]) -> (String, String) {
    let snap = crate::fact_fetch::test_snapshot(url, "Tides held", paragraphs);
    let s = lib.add_fetched(url, &snap, &AddOpts { keep_closed_held: true, ..Default::default() }, &Budgets::default()).unwrap();
    let text = std::fs::read_to_string(lib.source_dir(&s.id).join("source.md")).unwrap();
    (s.id, text)
}

fn held_lib(home: &Path) -> (Library, String, String) {
    let lib = Library::in_home(home);
    let text = long_text();
    let (id, stored) = held_source(&lib, "https://held.example/tides", &[&text]);
    (lib, id, stored)
}

fn take(text: &str, skip: usize, n: usize) -> String {
    text.chars().skip(skip).take(n).collect()
}

/// Ten quotations of 290 characters, far apart: each under the run bound, 2,900 characters together.
fn ten_quotes(text: &str) -> String {
    (0..10).map(|k| take(text, 200 + k * 1300, 290)).collect::<Vec<_>>().join("\n\n")
}

// ---- the predicate ----------------------------------------------------------------------------------------------------------------

/// AC-27 and the run bound: ten quotations of 290 characters fail the aggregate cap while four pass; a source of 2,000 characters lets
/// out no more than 200 in all; one run of 301 characters fails and 300 pass; re-wrapping a run hides nothing; a cut output passes.
#[test]
fn the_run_bound_and_the_aggregate_cap_decide_what_of_a_held_source_may_leave() {
    let src = long_text();
    assert_eq!(cap_of(collapse(&src).len()), AGGREGATE_MAX);
    assert!(!body_may_leave(&src, &ten_quotes(&src)), "ten under the run bound are over the cap together");
    let four: String = (0..4).map(|k| take(&src, 200 + k * 1300, 290)).collect::<Vec<_>>().join("\n\n");
    assert!(body_may_leave(&src, &four), "four quotations, 1,160 characters, pass");
    assert!(body_may_leave(&src, &take(&src, 12, 300)) && !body_may_leave(&src, &take(&src, 12, 301)));
    let wrapped = take(&src, 10, 400).replace(' ', "\n  ");
    assert!(!body_may_leave(&src, &wrapped), "whitespace is compared collapsed");
    let short: String = format!("{}x", take(&src, 0, 1999));
    assert_eq!(cap_of(collapse(&short).len()), 200);
    assert!(body_may_leave(&short, &take(&short, 100, 200)) && !body_may_leave(&short, &take(&short, 100, 241)));
    let two: String = format!("{}\n\n{}", take(&short, 100, 120), take(&short, 900, 120));
    assert!(!body_may_leave(&short, &two), "no more than 200 characters in all of a short source");
    let cut = shorten(&src, &format!("Before. {} After.", take(&src, 10, 900)));
    assert!(cut.contains(SHORTENED) && !carries_run(&src, &cut), "{cut}");
    assert!(body_may_leave(&src, "Nothing of it."), "an output that quotes nothing passes");
}

// ---- the block index (AC-31) ------------------------------------------------------------------------------------------------------

/// AC-31: the block index is written when a held capture is kept; it finds a source no record names in a seed page, a page and a
/// document that quote 400 characters of it, and not a quotation of 50; evicting the text removes its blocks, and so does a purge.
#[test]
fn the_block_index_finds_a_held_source_no_record_names_and_forgets_it_when_its_text_goes() {
    let home = tempfile::tempdir().unwrap();
    let (lib, id, text) = held_lib(home.path());
    assert!(indexed(&lib).contains(&id), "written as the capture was kept");
    let q400 = take(&text, 300, 400);
    let held = lib.held();
    assert_eq!(held.found_in(&format!("A note copied: {q400}")), vec![id.clone()]);
    assert!(held.found_in(&take(&text, 300, 50)).is_empty(), "the stated limit: under 64 characters an unnamed source is not found");
    // A seed page: its span names no source; the page is cut where it quotes and passes.
    use crate::topic_seed::{seed_page, Row, Seed, Span};
    let span = Span { row: Row::Search, label: "s-kb-0000000000000001".into(), kind: "passage".into(), rel_path: "context/n.md".into(), start_line: 1, end_line: 1, title: "Notes".into(), text: format!("A note copied: {q400}"), hash: String::new(), status: "○ not validated".into(), topic: None };
    let page = seed_page("Tides", &Seed { spans: vec![span], ..Default::default() }, &held).unwrap();
    assert!(page.body.contains(SHORTENED) && body_may_leave(&text, &page.body), "{}", page.body);
    // A page: the check finds the source no marker names.
    let checked = lib.check_body(&format!("# Tides\n\nA note copied: {q400}\n"), crate::library::pages::Scope::Internal);
    assert!(checked.findings.contains(&crate::library::pages::Finding::HeldText(id.clone())), "{:?}", checked.findings);
    // A document.
    assert!(check_files(home.path(), &[("doc.md".into(), q400.clone().into_bytes())]).unwrap_err().contains(&format!("s-{id}")));
    // Evicted: the blocks go with the text.
    lib.cap_text_cache(0, "none", 0);
    assert!(!lib.source_dir(&id).join("source.md").exists() && !indexed(&lib).contains(&id));
    assert!(lib.held().found_in(&q400).is_empty() && check_files(home.path(), &[("doc.md".into(), q400.into_bytes())]).is_ok());
    // Purged: the same.
    let (id2, text2) = held_source(&lib, "https://held.example/second", &[&long_text().replace("tides", "harbour")]);
    assert!(indexed(&lib).contains(&id2) && !lib.held().found_in(&take(&text2, 100, 400)).is_empty());
    lib.purge(&id2).unwrap();
    assert!(!indexed(&lib).contains(&id2) && lib.held().found_in(&take(&text2, 100, 400)).is_empty());
}

// ---- the exits (AC-9) ------------------------------------------------------------------------------------------------------------

/// AC-9, a bundle's `sources/` and every registered channel: the library's exit check refuses a held source's body on every channel
/// that carries one, as it refuses a private one; the lookups, which send an address or a DOI, and the citation export still pass.
#[test]
fn a_held_source_never_leaves_by_a_bundle_copy() {
    let home = tempfile::tempdir().unwrap();
    let (lib, id, _) = held_lib(home.path());
    let meta = lib.read_meta(&id).unwrap();
    // The channels that carry a body; the others send an address, a DOI, tags or the citation only.
    let carries = [Channel::Bundle, Channel::Xenia, Channel::DebugBundle, Channel::Log, Channel::Relay, Channel::Connector, Channel::Mail, Channel::Outbound, Channel::Telemetry, Channel::History];
    for c in Channel::ALL {
        assert_eq!(crate::egress::check(&meta, c).is_err(), carries.contains(&c), "{c:?}");
    }
    assert_eq!(Channel::ALL.len() - carries.len(), 5, "three lookups, the web search and the citation export");
    let dir = tempfile::tempdir().unwrap();
    let out = lib.export_to_bundle(dir.path(), std::slice::from_ref(&id), false);
    assert!(out[0].1.as_ref().unwrap_err().contains("text_held"), "{:?}", out[0].1);
    assert!(!dir.path().join("sources").join(&id).exists(), "nothing of it was copied");
}

/// AC-9, a bibliography: a held source's citation is sent, and a free-text field that carries a run of its text is cut to the bound.
#[test]
fn a_bibliography_sends_a_held_sources_citation_and_no_run_of_its_text() {
    let home = tempfile::tempdir().unwrap();
    let (lib, id, text) = held_lib(home.path());
    let path = lib.source_dir(&id).join("meta.json");
    let mut meta: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    meta["title"] = json!(take(&text, 40, 500));
    std::fs::write(&path, meta.to_string()).unwrap();
    for f in [crate::library::bib::Format::Plain, crate::library::bib::Format::CslJson, crate::library::bib::Format::BibTex] {
        let e = crate::library::bib::export(&lib, std::slice::from_ref(&id), f).unwrap();
        assert_eq!(e.withheld, 0, "the citation is sent");
        assert!(!carries_run(&text, &e.text) && e.text.contains(SHORTENED), "{f:?}: {}", e.text);
    }
}

/// AC-9 and AC-27 at the exits that take files (a Xenia publish, a bundle published through Xenia): a file that quotes 400 characters
/// is refused, a directory whose ten files each quote 290 characters is refused as one output, and a directory with none passes.
#[test]
fn a_file_that_quotes_a_held_source_is_refused_at_every_file_exit() {
    let home = tempfile::tempdir().unwrap();
    let (_lib, id, text) = held_lib(home.path());
    let work = tempfile::tempdir().unwrap();
    let one = work.path().join("note.md");
    std::fs::write(&one, take(&text, 500, 400)).unwrap();
    assert!(check_path(home.path(), &one).unwrap_err().contains(&format!("s-{id}")));
    let ten = work.path().join("ten");
    std::fs::create_dir_all(&ten).unwrap();
    for k in 0..10 {
        std::fs::write(ten.join(format!("q{k}.md")), take(&text, 200 + k * 1300, 290)).unwrap();
    }
    let err = check_path(home.path(), &ten).unwrap_err();
    assert!(err.contains("q0.md"), "the refusal names the files: {err}");
    let clean = work.path().join("clean");
    std::fs::create_dir_all(&clean).unwrap();
    std::fs::write(clean.join("a.md"), "Nothing quoted here.").unwrap();
    assert!(check_path(home.path(), &clean).is_ok());
}

/// A model that puts each text at a point set by its bytes: enough to embed and export.
struct Words;
impl crate::embed::Embedder for Words {
    fn model_id(&self) -> &str {
        "words-test"
    }
    fn dim(&self) -> usize {
        4
    }
    fn min_similarity(&self) -> f32 {
        0.0
    }
    fn embed_documents(&self, t: &[String]) -> Result<Vec<Vec<f32>>, String> {
        Ok(t.iter().map(|s| (0..4).map(|i| s.bytes().filter(|b| *b as usize % 4 == i).count() as f32 + 1.0).collect()).collect())
    }
    fn embed_query(&self, t: &str) -> Result<Vec<f32>, String> {
        Ok(self.embed_documents(&[t.to_string()])?.remove(0))
    }
}

fn index_with(docs: &[(&str, &str, &str, &str)]) -> crate::kb::Kb {
    let mut kb = crate::kb::Kb::open_in_memory().unwrap();
    kb.attach_embedder(std::sync::Arc::new(Words)).unwrap();
    kb.set_seed_vectors(None);
    for (path, zone, tier, text) in docs {
        kb.upsert_from(path, zone, tier, text, "2026-11-26T00:00:00Z").unwrap();
    }
    kb.embed_pending(&mut |_, _| {}).unwrap();
    kb
}

/// AC-9, `kb seed-export`: no vector of a held source's chunks is written out. The export with the held source in the index equals the
/// export of an index without it.
#[test]
fn seed_export_writes_no_vector_of_a_held_sources_chunks() {
    let home = tempfile::tempdir().unwrap();
    let (_lib, id, text) = held_lib(home.path());
    let held_path = format!("library/sources/{id}/source.md");
    let other = "# Notes\n\nThe harbour master keeps the tide tables.\n";
    let with = index_with(&[(&held_path, "library", "federated_verified", &text), ("context/notes.md", "context", "federated_verified", other)]);
    let without = index_with(&[("context/notes.md", "context", "federated_verified", other)]);
    let all = crate::embed::vectors::VecCache::parse(&with.export_vectors().unwrap()).unwrap();
    let out = crate::embed::vectors::VecCache::parse(&with.export_vectors_except(&|p| withholds_path(home.path(), p)).unwrap()).unwrap();
    let want = crate::embed::vectors::VecCache::parse(&without.export_vectors().unwrap()).unwrap();
    assert!(all.len() > out.len(), "the held source had chunks to withhold");
    assert_eq!(out.len(), want.len());
    assert!(out.entries().all(|(k, v)| want.get(k) == Some(v)));
    assert!(withholds_path(home.path(), &held_path) && !withholds_path(home.path(), "context/notes.md") && withholds_path(home.path(), "library/sources/no-such-source/source.md"), "a record that cannot be read is withheld");
}

/// AC-9, `kb serve --allow-remote`: over HTTP (the view of a client that may be off this machine) a held source's chunks are not
/// returned by a search and its document is not served, even where an older index holds it at a trusted tier; the operator's own
/// view through the daemon's socket is given it.
#[test]
fn kb_serve_returns_no_chunk_of_a_held_source() {
    let home = tempfile::tempdir().unwrap();
    let (_lib, id, text) = held_lib(home.path());
    let ws = home.path().join("workspaces/w");
    std::fs::create_dir_all(&ws).unwrap();
    let held_path = format!("library/sources/{id}/source.md");
    let kb = index_with(&[(&held_path, "library", "federated_verified", &text), ("context/notes.md", "context", "federated_verified", "---\nstatus: active\n---\n# Notes\n\nHeld sentence about the tides, in a note of our own.\n")]);
    let remote = crate::kbserve::Reader { kb: &kb, ws: &ws, operator: false, home: None, remote: false };
    let r = remote.get("/v1/search?q=held%20sentence%20tides&limit=10");
    assert_eq!(r.status, 200, "{}", r.body);
    let paths: Vec<&str> = r.body["results"].as_array().unwrap().iter().filter_map(|h| h["path"].as_str()).collect();
    assert!(!paths.contains(&held_path.as_str()) && paths.contains(&"context/notes.md"), "{paths:?}");
    assert_eq!(remote.get(&format!("/v1/doc?path={held_path}")).status, 404);
    let mine = crate::kbserve::Reader { kb: &kb, ws: &ws, operator: true, home: Some(home.path()), remote: false };
    let paths: Vec<String> = mine.get("/v1/search?q=held%20sentence%20tides&limit=10").body["results"].as_array().unwrap().iter().filter_map(|h| h["path"].as_str().map(String::from)).collect();
    assert!(paths.contains(&held_path), "the operator's view: {paths:?}");
}

/// AC-9 and AC-27, a page draft: a draft that quotes 400 characters of a held source no marker names, or ten quotations of 290
/// characters of it, fails its mechanical check, so the writer's page falls back as for any failed check; four quotations pass.
#[test]
fn a_page_draft_that_quotes_a_held_source_past_the_bound_fails_its_check() {
    let home = tempfile::tempdir().unwrap();
    let (lib, id, text) = held_lib(home.path());
    let scope = crate::library::pages::Scope::Internal;
    let held = crate::library::pages::Finding::HeldText(id.clone());
    assert!(lib.check_body(&format!("# Tides\n\n{}\n", take(&text, 30, 400)), scope).findings.contains(&held));
    assert!(lib.check_body(&format!("# Tides\n\n{}\n", ten_quotes(&text)), scope).findings.contains(&held));
    let four: String = (0..4).map(|k| take(&text, 200 + k * 1300, 290)).collect::<Vec<_>>().join("\n\n");
    assert!(!lib.check_body(&format!("# Tides\n\n{four}\n"), scope).findings.contains(&held));
}

/// AC-9 and AC-27, a page of every outcome as code makes it: the claims-only page whose claims copy the held text loses its last
/// lines until it passes; a page cut to fit loses its last blocks that quote; what is left passes the predicate.
#[test]
fn a_page_of_every_outcome_is_cut_or_refused_before_it_is_written() {
    let home = tempfile::tempdir().unwrap();
    let (lib, _id, text) = held_lib(home.path());
    for k in 0..10 {
        lib.add_claim(&take(&text, 200 + k * 1300, 290), &Default::default(), &[]).unwrap();
    }
    let page = lib.claims_only_page("tides", "no writer", None, Vec::new(), 0, crate::library::bib::Format::Plain, crate::library::pages::Scope::Internal).unwrap();
    let md = &page.page.markdown;
    assert!(body_may_leave(&text, md), "{md}");
    assert!(md.matches("Held sentence").count() >= 3, "the lines that fit are kept: {md}");
    // A page code made, cut to fit: ten quoted blocks become four.
    let fitted = lib.held().fit(&format!("# Tides\n\n{}", ten_quotes(&text)), &[]).unwrap();
    assert!(body_may_leave(&text, &fitted) && fitted.split("\n\n").count() == 5, "{fitted}");
    // A written page past the bound is refused at the last check, not cut.
    assert!(lib.held().check(&format!("# Tides\n\n{}", take(&text, 0, 500)), &[]).is_err());
}

/// 139 AC-18 with the real predicate (`specs/142` §2.5 item 4): a seed page whose spans quote a held source at length, named or not,
/// passes `body_may_leave` for it; a seed over the aggregate cap loses the blocks that quote. The daemon builds every seed page with
/// the library's predicate (`page_run.rs`, `SENDERS`).
#[test]
fn a_seed_page_built_by_the_daemon_passes_body_may_leave_for_a_held_source() {
    use crate::topic_seed::{seed_page, Row, Seed, Span};
    let home = tempfile::tempdir().unwrap();
    let (lib, id, text) = held_lib(home.path());
    let span = |n: usize, t: String| Span { row: Row::Search, label: format!("s-kb-{:016}", n), kind: "passage".into(), rel_path: format!("context/n{n}.md"), start_line: 1, end_line: 1, title: "Notes".into(), text: t, hash: String::new(), status: "○ not validated".into(), topic: None };
    let held = lib.held();
    let named = Seed { spans: vec![span(1, format!("The answer opens. {}", take(&text, 30, 600)))], sources_read: vec![id.clone()], ..Default::default() };
    let p = seed_page("Tides", &named, &held).unwrap();
    assert!(body_may_leave(&text, &p.body) && p.body.contains(SHORTENED), "{}", p.body);
    let over = Seed { spans: (0..10).map(|k| span(10 + k, take(&text, 200 + k * 1300, 290))).collect(), ..Default::default() };
    let p = seed_page("Tides", &over, &held).unwrap();
    assert!(body_may_leave(&text, &p.body) && !p.dropped.is_empty(), "{:?}", p.dropped);
}

/// AC-9, the app: `library.source` says a held source may not be exported, copied or printed past the bound, and an open one may;
/// `pages.get` says so of a page. These reads answer only the console and the app, both on sockets of this machine: a relay, a
/// narrator or a host is refused, so no client off this machine is sent any of the text.
#[test]
fn the_apps_views_say_a_held_source_may_not_be_exported_and_no_client_off_this_machine_reads_them() {
    let home = tempfile::tempdir().unwrap();
    let (lib, id, _) = held_lib(home.path());
    let ws = home.path().join("workspaces/w");
    let v = crate::encyclopedia::library_source(home.path(), &ws, &json!({"id": format!("s-{id}")}), true).unwrap();
    assert_eq!((v["text_held"].as_bool(), v["may_export"].as_bool()), (Some(true), Some(false)), "{}", v["may_export"]);
    assert!(v["text"].as_str().is_some_and(|t| t.contains("Held sentence")), "the operator's own view is shown the text");
    let open_file = home.path().join("open.md");
    std::fs::write(&open_file, "# Open\n\nAn open text.\n").unwrap();
    let open = lib.add_file(&open_file, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
    let v = crate::encyclopedia::library_source(home.path(), &ws, &json!({"id": open}), true).unwrap();
    assert_eq!((v["text_held"].as_bool(), v["may_export"].as_bool()), (Some(false), Some(true)));
    // pages.get carries the same answer for a page (the held refusal is part of its export decision since `specs/141` slice C), and
    // the reads are the console's and the app's only.
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/encyclopedia.rs")).unwrap();
    assert!(src.contains("lib.held().refused(&md, &[])") && src.contains("view_decision(&label, crate::egress::Sink::PageExport, held)"));
    let service = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/service.rs")).unwrap();
    let reader = service.split("fn need_reader(scope: Scope)").nth(1).unwrap();
    assert!(reader[..reader.find("\n}\n").unwrap()].contains("matches!(scope, Scope::Console | Scope::App)"));
    for m in ["library_source", "pages_get"] {
        let body = service.split(&format!("async fn {m}(")).nth(1).unwrap_or_else(|| panic!("{m}"));
        assert!(body[..body.find("\n    }\n").unwrap()].contains("need_reader(ctx.scope)?"), "{m}");
    }
}

/// AC-10: a held source is indexed at `external_untrusted`; a span that is given a passage of it starts at that tier (L0), and from
/// there it can write no heuristic, lesson, review rule or skill (the promotion gate) and holds no outbound action.
#[test]
fn a_span_given_held_text_cannot_promote() {
    use delphi_core::provenance::{gate, gate_write, Action, Gate, Span, Tier};
    let home = tempfile::tempdir().unwrap();
    let (_lib, id, _) = held_lib(home.path());
    let ws = home.path().join("workspaces/w");
    std::fs::create_dir_all(&ws).unwrap();
    let files = crate::kb::sources(&ws);
    let f = files.iter().find(|f| f.rel_path == format!("library/sources/{id}/source.md")).expect("the held source is indexed");
    assert_eq!(f.default_source, "external_untrusted");
    let mut kb = crate::kb::Kb::open_in_memory().unwrap();
    kb.sync_lexical(&files, "2026-11-26T00:00:00Z").unwrap();
    let hits = kb.search("held sentence tides", &crate::kb::SearchOpts { include_untrusted: true, ..Default::default() }).unwrap();
    let hit = hits.iter().find(|h| h.rel_path == f.rel_path).expect("found where untrusted text is asked for");
    assert_eq!(hit.source_type, "external_untrusted");
    let tier = crate::retrieval::tier_of(&hit.source_type);
    assert_eq!(tier, Tier::L0);
    let mut span = Span::root("s1");
    span.observe_input(tier);
    assert_eq!(span.ceiling(), Tier::L0);
    for p in ["context/heuristics.md", "REVIEW.md", "memory/agents/builder/lessons.md", "skills/x/SKILL.md"] {
        assert!(gate_write(p, span.ceiling()).is_err(), "{p}");
    }
    assert_ne!(gate(span.ceiling(), Action::Outbound), Gate::Allow);
    // The operator's rulings of 2026-11-27: a search that asks for nothing untrusted is not given it, one that asks for held text is,
    // labelled, at the same tier.
    let plain = kb.search("held sentence tides", &crate::kb::SearchOpts::default()).unwrap();
    assert!(!plain.iter().any(|h| h.rel_path == f.rel_path), "a search that asks for nothing untrusted is not given it");
    let asked = kb.search("held sentence tides", &crate::kb::SearchOpts { include_held: true, ..Default::default() }).unwrap();
    let hit = asked.iter().find(|h| h.rel_path == f.rel_path).expect("a search that asks for held text is given it");
    assert_eq!((hit.label, hit.source_type.as_str()), (crate::kb::HELD_LABEL, "external_untrusted"));
}

/// The operator's rulings of 2026-11-27 (they reverse slice B's builder decision 1): a role that does not act (here the scout) is
/// given a held source's passage by its default search and its span's start, labelled held text, untrusted and not validated; a
/// role that acts (the builder) is not, unless it asks with `include_held`. Its taint is unchanged: the span that is given it starts
/// at `external_untrusted` (L0). A private source is never returned to anyone, and what may leave the machine is unchanged.
#[test]
fn a_role_that_does_not_act_is_given_held_text_by_default_and_a_builder_only_when_it_asks() {
    use delphi_core::provenance::Tier;
    let home = tempfile::tempdir().unwrap();
    let (lib, id, stored) = held_lib(home.path());
    let private = home.path().join("private.md");
    std::fs::write(&private, "Held sentence of the tides, a private copy of the tides survey.\n").unwrap();
    let p = lib.add_file(&private, &AddOpts { private: true, level: Some(crate::library::Level::Full), licence: Some("all-rights-reserved".into()), ..Default::default() }, &Budgets::default()).unwrap();
    let ws = home.path().join("workspaces/w");
    std::fs::create_dir_all(ws.join("local")).unwrap();
    let db = ws.join("local/state.db");
    {
        let mut kb = crate::kb::Kb::open(&db).unwrap();
        kb.sync_lexical(&crate::kb::sources(&ws), "2026-11-27T00:00:00Z").unwrap();
    }
    let held = format!("library/sources/{id}/source.md");
    let strs = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let scout = crate::retrieval::held_by_default("scout", &strs(&["missions/**/research/**", "context/domains/**"]), &strs(&["repo-mapper", "web-search", "dependency-auditor"]));
    let builder = crate::retrieval::held_by_default("builder", &strs(&["@worktree/**"]), &strs(&["shell-worktree-scoped", "test-runner", "git-worktree", "cli-harness-delegate"]));
    assert!(scout && !builder);
    let ask = |role: &str, include_held: bool| crate::retrieval::search(home.path(), &db, "held sentence tides", crate::kb::SearchOpts { role: Some(role.into()), include_held, ..Default::default() }).unwrap().hits;
    // The scout's default search, as `kb_search` asks for it.
    let r = ask("scout", scout);
    let hit = r.iter().find(|h| h.rel_path == held).expect("a role that does not act is given the held passage");
    assert_eq!((hit.shown_label().as_str(), hit.source_type.as_str()), (crate::kb::HELD_LABEL, "external_untrusted"));
    assert_eq!(crate::retrieval::tier_of(&hit.source_type), Tier::L0, "its taint is unchanged");
    // The builder's default search: not given it. Asked for, it is.
    assert!(!ask("builder", builder).iter().any(|h| h.rel_path == held), "a builder's default search is not given held text");
    assert!(ask("builder", true).iter().any(|h| h.rel_path == held && h.label == crate::kb::HELD_LABEL), "a builder that asks is given it");
    for role in ["scout", "builder"] {
        for include_held in [false, true] {
            assert!(!ask(role, include_held).iter().any(|h| h.rel_path.contains(&p.id)), "a private source is never returned");
        }
    }
    // A span's start passages: the scout's carry it, labelled, and start at L0; the builder's do not.
    let budget = |held: bool| crate::retrieval::StartBudget { held, ..Default::default() };
    let start = crate::retrieval::span_start(home.path(), &db, "m-held", "Summarise the held sentence of the tides", "scout", budget(scout), false).expect("passages");
    let line = start.text.lines().find(|l| l.starts_with(&format!("- {held}#"))).unwrap_or_else(|| panic!("a held passage starts the span: {}", start.text));
    assert!(line.contains(&format!(" [{}]: ", crate::kb::HELD_LABEL)), "{line}");
    assert_eq!(start.tier, Tier::L0, "the span is tainted by the held passage as before");
    assert!(!start.text.contains(&p.id) && !start.text.contains("private copy"), "a private source never starts a span");
    let built = crate::retrieval::span_start(home.path(), &db, "m-held", "Summarise the held sentence of the tides", "builder", budget(builder), false);
    assert!(built.as_ref().is_none_or(|s| !s.text.contains(&held)), "a builder's span does not start with held text");
    // Nothing about leaving changed: the passage the span was given is held text, and a whole run of it may not leave.
    assert!(!body_may_leave(&stored, &take(&stored, 0, 400)));
    assert!(crate::library::is_held(&lib.read_meta(&id).unwrap()));
}

// ---- the inventory (AC-9) -------------------------------------------------------------------------------------------------------

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// The production text of a source file: everything before its last test module.
fn production(src: &str) -> &str {
    let cut = src.rfind("\n#[cfg(test)]\nmod tests").or_else(|| src.rfind("\nmod tests")).unwrap_or(src.len());
    &src[..cut]
}

/// AC-9 (blocks the merge of slice B), and `specs/141` §2.4 item 8: every exit still asks the predicate (its file holds its call), every
/// proof test exists, and every file that can send (a network crate, an archive, a vector file, a push, an HTTP server, the app's
/// commands) is in the inventory. Removing the predicate from any one exit fails here; so does a new sender nobody listed.
#[test]
fn every_sender_is_listed_and_every_exit_asks() {
    let root = repo();
    for s in SENDERS {
        let text = std::fs::read_to_string(root.join(s.file)).unwrap_or_else(|e| panic!("{}: {e}", s.file));
        if let How::Asks { call } = s.how {
            assert!(production(&text).contains(call), "{} ({}): the exit no longer asks the predicate (`{call}`)", s.name, s.file);
        }
        if let Some((file, test)) = s.proof {
            let t = std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{}: proof {file}: {e}", s.name));
            assert!(test.is_empty() || t.contains(&format!("fn {test}(")), "{}: the proof test `{test}` is missing from {file}", s.name);
        }
        if let How::NotASink { why } = s.how {
            assert!(!why.is_empty(), "{}", s.name);
        }
    }
    let exits: Vec<&Sender> = SENDERS.iter().filter(|s| matches!(s.how, How::Asks { .. })).collect();
    assert!(exits.iter().all(|s| s.proof.is_some()), "every exit has a test that sends held text through it");
    // The scan: every production file that can send is listed.
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                if !matches!(p.file_name().and_then(|n| n.to_str()), Some("target" | "tests" | "node_modules" | "fixtures")) {
                    walk(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root.join("crates"), &mut files);
    walk(&root.join("app/src-tauri/src"), &mut files);
    let listed: BTreeSet<&str> = SENDERS.iter().map(|s| s.file).collect();
    let mut missing = Vec::new();
    for f in files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if rel.ends_with("_tests.rs") || rel == "crates/delphi-engine/src/held_text.rs" || rel == "crates/delphi-engine/src/egress.rs" {
            continue;
        }
        let src = std::fs::read_to_string(&f).unwrap_or_default();
        let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        if SEND_MARKERS.iter().any(|m| code.contains(m)) && !listed.contains(rel.as_str()) {
            missing.push(rel);
        }
    }
    assert!(missing.is_empty(), "these files can send and are not in held_text::SENDERS (read each: does it carry held text, and does it ask body_may_leave?): {missing:?}");
}

/// The fitness function of `specs/142` §6: one predicate decides what of a held source an output may carry. No production file but
/// this one runs the run scan (`shares_run`) of a source's text against an output; the private one of 119 goes through
/// [`carries_run`] here too.
#[test]
fn every_output_that_can_carry_held_text_calls_body_may_leave() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") || matches!(name.as_str(), "held_text.rs" | "library_private.rs") {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).unwrap();
        assert!(!production(&text).contains("shares_run("), "{name} scans for a source's runs itself: ask crate::held_text");
    }
}
