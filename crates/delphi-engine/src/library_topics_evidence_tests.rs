//! `specs/144` slice D: pages built from evidence (§2.7 items 1 to 4; AC-14, AC-17).

use super::*;
use crate::egress::{Confirm, Sink};
use crate::label::Mark;
use crate::library::page_validate::{support_all, PageQuery, PageValidator, PageWriter, ValidatorFuture, ValidatorReply, WriterFuture, NOT_GIVEN};
use crate::research_record::{Asker, Begin, Topic};
use crate::topic_seed::{assemble_linked, NoHeldText, Row};
use std::sync::Mutex;

/// The source the research rests on. No word of it is in the topic's question, so the library's own gather does not find it: only
/// the topic's evidence pack brings it to the writer.
const QUAY: &str = "# Gauge notes\n\nWater at the quay arrives sixty minutes after the coast. Gauges at the quay show it every day.\n\nA second paragraph about boats moored in the basin at night.\n";
/// A source the run read and no claim quotes.
const KEEPER: &str = "# Keeper notes\n\nThe keeper logs the gauge every morning at six and keeps the book in the office.\n";
/// A source the writer is never given.
const BEES: &str = "# Bees\n\nBees dance to tell the hive where flowers are found in spring.\n";

struct Fx {
    _t: tempfile::TempDir,
    home: PathBuf,
    ws: PathBuf,
    lib: Library,
    quay: String,
    keeper: String,
    claim: String,
    slug: String,
    topic: Topic,
}

fn add(lib: &Library, dir: &Path, name: &str, text: &str) -> String {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    lib.add_file(&p, &crate::library::AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &crate::library::Budgets::default()).unwrap().id
}

/// A research topic whose run read the quay notes and the keeper's, kept one claim anchored in the quay notes, and answered in two
/// paragraphs: one that names the claim, one that rests on nothing.
fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().to_path_buf();
    let ws = home.join("workspaces/w");
    std::fs::create_dir_all(ws.join("local")).unwrap();
    let lib = Library::in_home(&home).with_workspace(&ws);
    let quay = add(&lib, &home, "quay.md", QUAY);
    let keeper = add(&lib, &home, "keeper.md", KEEPER);
    let quote = "Water at the quay arrives sixty minutes after the coast.";
    let claim = lib.add_claim(quote, &Default::default(), &[crate::library::claims::AnchorSpec { source_id: quay.clone(), locator: Some("¶1".into()), quote: quote.into(), ..Default::default() }]).unwrap()["id"].as_str().unwrap().to_string();
    let topic = Topic::begin(&ws, &Begin { question: "Why is the harbour tide late?", asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap();
    topic.page("r-1", "https://example.org/quay", &quay, None).unwrap();
    topic.page("r-1", "file:keeper", &keeper, None).unwrap();
    let answer = format!("The harbour tide is an hour late at the quay [{claim}].\n\nThe keeper blames the moon for the delay.");
    topic.finish("r-1", "done", Some(&json!({"answer": answer, "claims": [&claim]})), std::slice::from_ref(&claim), None, 0).unwrap();
    crate::evidence_pack::write(&lib, &topic).unwrap();
    let slug = lib.topics_from_research(&ws, 1).remove(0);
    Fx { _t: t, home, ws, lib, quay, keeper, claim, slug, topic }
}

/// A writer that keeps its brief and answers with one fixed draft.
struct Writer {
    draft: String,
    briefs: Mutex<Vec<String>>,
}
impl PageWriter for Writer {
    fn write<'a>(&'a self, b: &'a str) -> WriterFuture<'a> {
        self.briefs.lock().unwrap().push(b.to_string());
        let d = self.draft.clone();
        Box::pin(async move { Ok(d) })
    }
    fn rewrite<'a>(&'a self, d: &'a str, _f: &'a [String]) -> WriterFuture<'a> {
        let d = d.to_string();
        Box::pin(async move { Ok(d) })
    }
}

/// A validator that keeps what it was shown and supports every unit with its first cited record's source text.
#[derive(Default)]
struct Shown(Mutex<Vec<(String, bool, Vec<String>)>>);
impl PageValidator for Shown {
    fn validate<'a>(&'a self, q: &'a PageQuery) -> ValidatorFuture<'a> {
        *self.0.lock().unwrap() = q.material.iter().map(|m| (m.id.clone(), m.cited, m.parts.iter().map(|p| p.shown.clone()).collect())).collect();
        let assertions = support_all(q);
        Box::pin(async move { Ok(ValidatorReply { assertions, model: "m".into(), family: "f".into(), tokens: 10 }) })
    }
}

fn rebuild(f: &Fx, w: &Writer, v: &Shown, now: u64) -> Result<TopicEntry, String> {
    let side = f.lib.topic_side(&f.slug).unwrap();
    let seed = assemble_linked(&f.lib, &f.ws, &side);
    let g = f.lib.topic_gather_library(&side, Some(&f.ws));
    let cfg = PagesConfig::default();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    rt.block_on(f.lib.topic_rebuild(TopicRequest { slug: &f.slug, now, settings: &cfg, style: bib::Format::Plain, gathered: g, gaps: vec![], seed, held: &NoHeldText, cancel: None }, w, v, &TokenLedger::new(1_000_000), &|| Ok(())))
}

fn writer(draft: String) -> Writer {
    Writer { draft, briefs: Mutex::new(Vec::new()) }
}

/// AC-14: a topic page is written from the topic's evidence merged (its evidence pack brings the quay notes and the claim the library's
/// own gather did not find); the seed reaches the writer as an outline whose markers are re-pointed to the claims the answer rests on,
/// and the answer's unmarked paragraph comes with no marker; the validator is shown that same evidence, with the passage the writer saw,
/// and the topic's claims; the sidecar names the pack. The page is never written from the previous page: the canary test
/// `expand_tests::no_brief_holds_text_of_the_previous_page` stands for §2.3 item 6 of `specs/139`.
#[test]
fn a_topic_page_is_written_from_merged_evidence_with_the_seed_as_an_outline() {
    let f = fx();
    let side = f.lib.topic_side(&f.slug).unwrap();
    let before = f.lib.topic_gather_library(&side, Some(&f.ws));
    assert!(!before.sources.iter().any(|(id, _, _)| *id == f.quay), "the library's gather alone does not find the quay notes");
    let w = writer(format!("# Harbour tide\n\nWater at the quay arrives sixty minutes after the coast [s-{}].\n", f.quay));
    let v = Shown::default();
    let built = rebuild(&f, &w, &v, 100).unwrap();
    assert_eq!(built.outcome.as_deref(), Some("pass"), "{built:?}");
    let brief = w.briefs.lock().unwrap().remove(0);
    // The pack's evidence: the claim and the paragraph of the quay notes that holds its quote, as a record.
    let record = format!("RECORD s-{} |", f.quay);
    let at = brief.find(&record).unwrap_or_else(|| panic!("the pack's source is a record:\n{brief}"));
    assert!(brief[at..].contains("Water at the quay arrives sixty minutes after the coast. Gauges at the quay show it every day."), "{brief}");
    assert!(brief.contains(&format!("RECORD {} |", f.claim)), "{brief}");
    // The seed as outline: structure and order, the answer's markers re-pointed to the claim, the unmarked prose with no marker.
    let draft = &brief[brief.rfind("\nDRAFT TO EXPAND (").expect("the outline is in the brief")..];
    let seed = assemble_linked(&f.lib, &f.ws, &side);
    let answer = seed.spans.iter().find(|s| s.row == Row::Answer).unwrap().label.clone();
    assert!(draft.contains(&format!("The harbour tide is an hour late at the quay. [{}]", f.claim)) || draft.contains(&format!("The harbour tide is an hour late at the quay [{}]", f.claim)), "{draft}");
    assert!(!draft.contains(&answer), "the answer's own marker is gone: {draft}");
    assert!(!draft.contains(&format!("({})", f.claim)), "the claim marks in the prose are taken out: {draft}");
    let unmarked = draft.lines().find(|l| l.contains("blames the moon")).unwrap();
    assert!(!unmarked.contains('['), "text that rests on nothing carries no marker: {unmarked}");
    assert!(brief.contains(OUTLINE_NOTE));
    // The validator: the writer's evidence and the contradicting records, nothing else; the cited source with the cut the writer saw.
    let shown = v.0.lock().unwrap().clone();
    let g = {
        let mut g = f.lib.expansion_material(&seed, f.lib.topic_gather_library(&side, Some(&f.ws)));
        f.lib.merge_packs(&mut g, &side);
        g
    };
    let given = writer_evidence(&g);
    let stored = f.lib.topic_side(&f.slug).unwrap();
    let contradicting: Vec<&str> = stored[CONTRADICTING].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    for (id, _, _) in &shown {
        assert!(given.contains(id) || contradicting.contains(&id.as_str()), "the validator was shown `{id}`, which the writer was not given");
    }
    let (_, cited, parts) = shown.iter().find(|(id, _, _)| *id == format!("s-{}", f.quay)).expect("the cited source is shown");
    let cut = g.sources.iter().find(|(id, _, _)| *id == f.quay).unwrap().2.clone();
    assert!(*cited && parts == &vec![cut], "{parts:?}");
    assert!(shown.iter().any(|(id, cited, _)| *id == f.claim && !cited), "the topic's claim is shown as a record a statement may contradict");
    // The sidecar names what the writer was given besides its material.
    assert_eq!(stored[EVIDENCE_PACKS], json!([format!("research/topics/{}/{}", f.topic.id, crate::evidence_pack::FILE)]), "{stored}");
    assert!(stored[OUTLINE].as_array().is_some_and(|o| o.iter().any(|s| s["label"] == answer.as_str())), "{stored}");
}

/// §2.7 item 3: the validator checks against exactly the writer's evidence. A draft that cites a library source the brief did not
/// hold fails the page check, before any validator is asked, and the seed stands.
#[test]
fn a_marker_naming_a_record_the_writer_was_not_given_fails_the_page() {
    let f = fx();
    let bees = add(&f.lib, &f.home, "bees.md", BEES);
    let w = writer(format!("# Harbour tide\n\nBees dance to tell the hive where flowers are found in spring [s-{bees}].\n"));
    let v = Shown::default();
    let err = rebuild(&f, &w, &v, 100).unwrap_err();
    assert!(err.starts_with(SEED_STANDS) && err.contains(NOT_GIVEN) && err.contains(&format!("s-{bees}")), "{err}");
    assert!(v.0.lock().unwrap().is_empty(), "no validator was asked");
}

/// AC-17: a written page's label is the join over everything its writer and validator were given, cited or not. It cites only the
/// quay notes (open), yet its research topic's run also read the keeper's notes: when those are made private after the build, the page
/// is private at the next exit check, and a bundle, an export or a push of it is refused.
#[test]
fn a_source_reclassified_after_the_build_changes_the_pages_label_at_the_next_exit() {
    let f = fx();
    let w = writer(format!("# Harbour tide\n\nWater at the quay arrives sixty minutes after the coast [s-{}].\n", f.quay));
    let built = rebuild(&f, &w, &Shown::default(), 100).unwrap();
    assert_eq!(built.outcome.as_deref(), Some("pass"), "{built:?}");
    let side = f.lib.topic_side(&f.slug).unwrap();
    let page = f.lib.pages_dir().join(format!("{}.md", f.slug));
    assert!(!crate::label::at_sink(&f.lib, Some(&f.ws), &side).has(Mark::LibraryPrivate));
    assert!(crate::egress::check_path(&f.home, Some(&f.ws), &page, Sink::Bundle, Confirm::Nobody).is_ok(), "an open page may leave");
    // The keeper's notes, which the page does not cite, are made private.
    let meta = f.lib.source_dir(&f.keeper).join("meta.json");
    let mut m: Value = serde_json::from_str(&std::fs::read_to_string(&meta).unwrap()).unwrap();
    m["private"] = json!(true);
    std::fs::write(&meta, m.to_string()).unwrap();
    let label = crate::label::at_sink(&f.lib, Some(&f.ws), &side);
    assert!(label.has(Mark::LibraryPrivate), "{:?}", label.names());
    for s in [Sink::Bundle, Sink::PageExport, Sink::Repository { internal_remote: false }] {
        let err = crate::egress::check_path(&f.home, Some(&f.ws), &page, s, Confirm::Nobody).unwrap_err();
        assert!(err.contains("private"), "{}: {err}", s.name());
    }
}

/// AC-17: a page that paraphrases private seed material while citing only a public source is private. The page's material is the open
/// quay notes alone; its outline holds a kept excerpt of a private source, its packs one of a run that read a private source: each
/// makes the page private, and so does a research topic it follows whose run read one. A contradicting claim is labelled as a claim.
#[test]
fn a_page_that_paraphrases_private_seed_material_while_citing_only_a_public_source_is_private() {
    let f = fx();
    let secret = add(&f.lib, &f.home, "secret.md", "# Secret\n\nThe gate code of the quay office is written under the desk.\n");
    let meta = f.lib.source_dir(&secret).join("meta.json");
    let mut m: Value = serde_json::from_str(&std::fs::read_to_string(&meta).unwrap()).unwrap();
    m["private"] = json!(true);
    std::fs::write(&meta, m.to_string()).unwrap();
    let l = crate::label::Labeller::new(Some(&f.lib), Some(&f.ws));
    let base = json!({"kind": KIND, "material": [{"id": format!("s-{}", f.quay)}]});
    assert!(!l.page(&base).0.has(Mark::LibraryPrivate), "the cited source alone is open");
    let mut outline = base.clone();
    outline[OUTLINE] = json!([{"label": "s-kb-0123456789abcdef", "rel_path": format!("library/sources/{secret}/excerpts.json")}]);
    assert!(l.page(&outline).0.has(Mark::LibraryPrivate), "a private span of the outline");
    // A pack of a run that read the private source, which it does not quote.
    let t = Topic::begin(&f.ws, &Begin { question: "Who keeps the quay office?", asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-2" }).unwrap();
    t.page("r-2", "file:secret", &secret, None).unwrap();
    t.finish("r-2", "done", Some(&json!({"answer": "The office is kept.", "claims": [&f.claim]})), std::slice::from_ref(&f.claim), None, 0).unwrap();
    let mut packed = base.clone();
    packed[EVIDENCE_PACKS] = json!([format!("research/topics/{}/{}", t.id, crate::evidence_pack::FILE)]);
    assert!(l.page(&packed).0.has(Mark::LibraryPrivate), "a pack of a run that read a private source");
    // A research topic the page follows: its answer is in the seed, and its run read the private source.
    let mut followed = base.clone();
    followed["research_topics"] = json!([t.id]);
    assert!(l.page(&followed).0.has(Mark::LibraryPrivate), "a followed topic whose run read a private source");
    let mut contra = base;
    contra[CONTRADICTING] = json!([f.claim]);
    assert!(!l.page(&contra).0.has(Mark::LibraryPrivate), "a contradicting claim on an open source adds nothing");
}
