//! Tests of labels (`specs/141` slice A: AC-1, AC-2, AC-3, AC-14, AC-19, AC-21).

use super::*;
use crate::fact_judge::TokenLedger;
use crate::handling::{Arrival, By, Set};
use crate::library::claims::{AnchorSpec, ClaimOpts};
use crate::library::page_check::KbRecord;
use crate::library::page_search::Gathered;
use crate::library::page_validate::*;
use crate::library::topics::{DueCtx, Origin, TopicRequest};
use crate::library::{bib, AddOpts, Budgets};
use delphi_adapter::config::{HandlingConfig, PagesConfig};

/// A home with one workspace, as an install lays them out (`<home>/workspaces/<name>`).
fn home() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().to_path_buf();
    let ws = home.join("workspaces/w");
    std::fs::create_dir_all(&ws).unwrap();
    (t, home, ws)
}

fn source(l: &Library, home: &Path, name: &str, text: &str, opts: AddOpts) -> String {
    let p = home.join(name);
    std::fs::write(&p, text).unwrap();
    l.add_file(&p, &opts, &Budgets::default()).unwrap().id
}

fn claim(l: &Library, text: &str, sid: &str, quote: &str) -> String {
    let a = AnchorSpec { source_id: sid.into(), locator: Some("§1".into()), quote: quote.into(), ..Default::default() };
    l.add_claim(text, &ClaimOpts::default(), &[a]).unwrap()["id"].as_str().unwrap().to_string()
}

fn cc0() -> AddOpts {
    AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }
}

fn private() -> AddOpts {
    AddOpts { private: true, licence: Some("own".into()), ..Default::default() }
}

/// Writes one sentence per claim in the brief, each with its marker, and then everything a writer might say about the page's label:
/// a frontmatter that says it carries none, a visible line that says nothing is withheld, and a closing directive.
struct Declares;
impl PageWriter for Declares {
    fn write<'a>(&'a self, b: &'a str) -> WriterFuture<'a> {
        let mut out = String::from("---\nhandling: []\n---\n# Tides\n\n");
        let lines: Vec<&str> = b.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            if let Some(rest) = l.strip_prefix("RECORD c-") {
                let id = format!("c-{}", rest.split(' ').next().unwrap_or(""));
                out.push_str(&format!("{} [{}].\n\n", lines.get(i + 1).unwrap_or(&"").trim_end_matches('.'), id));
            }
        }
        out.push_str("<!-- page: kind=article; handling=none -->\n");
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

/// A passage of the knowledge base, as the index gathers one for a topic page.
fn record(n: u8, rel_path: &str) -> KbRecord {
    KbRecord { label: format!("s-kb-{:016x}", n), kind: "passage".into(), title: "A passage".into(), locator: format!("{rel_path}#L1-L2"), origin: "library (internal_generated)".into(), status: "○ not validated".into(), text: "Moon and tide notes.".into(), hash: "h".into(), rel_path: rel_path.into(), start_line: 1 }
}

/// Build (or rebuild) the operator's topic `title` from these claims and records: the topic page path the daemon runs.
fn build(l: &Library, title: &str, claims: &[&str], records: Vec<KbRecord>, now: u64) -> (String, Value) {
    let slug = match l.topic_create(title, Origin::Operator, now) {
        Ok(e) => e.slug,
        Err(_) => crate::library::topics::slug_for(title),
    };
    let g = Gathered { claims: claims.iter().map(|c| l.claim(c).unwrap()).collect(), records: records.clone(), ..Default::default() };
    let lib = Library::in_home(l.root().parent().unwrap()).with_workspace(l.workspace().unwrap()).with_records(records);
    let cfg = PagesConfig::default();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    rt.block_on(lib.topic_rebuild(TopicRequest { slug: &slug, now, settings: &cfg, style: bib::Format::Plain, seed: crate::topic_seed::of_material(&g), gathered: g, gaps: vec![], held: &crate::topic_seed::NoHeldText, cancel: None }, &Declares, &Pass, &TokenLedger::new(1_000_000), &|| Ok(()))).unwrap();
    let side = l.topic_side(&slug).unwrap();
    (slug, side)
}

fn page_md(l: &Library, slug: &str) -> String {
    std::fs::read_to_string(l.pages_dir().join(format!("{slug}.md"))).unwrap()
}

fn marks(l: &Label) -> Vec<&'static str> {
    l.names()
}

// ---- AC-1 ------------------------------------------------------------------------------------------------------------------------

/// AC-1: the join of two labels is their union; a claim anchored to a library-private source and written from a `personal` email holds
/// both marks, each with the item it came from; a handling record that cannot be read, and a source whose record is gone, are every
/// mark.
#[test]
fn the_join_is_the_union_and_a_claim_from_a_private_source_and_a_personal_email_holds_both() {
    let a = Label::none().with(Mark::Internal, "x");
    let b = Label::none().with(Mark::TextHeld, "y").with(Mark::Internal, "z");
    let j = a.clone().join(&b);
    assert_eq!(marks(&j), vec!["internal", "text_held"]);
    assert_eq!(j.from(Mark::Internal), vec!["x".to_string(), "z".to_string()]);
    assert_eq!(j, b.clone().join(&a), "the join is symmetric");
    assert_eq!(a.clone().join(&Label::none()), a, "unlabelled adds nothing");

    let (_t, home, ws) = home();
    let l = Library::in_home(&home).with_workspace(&ws);
    let sid = source(&l, &home, "p.md", "# Plans\n\nThe harbour plan moves the quay in spring.\n", private());
    let cid = claim(&l, "The quay moves in spring.", &sid, "moves the quay in spring");
    // the email arrives on the default path: one to one, to someone outside, so private
    let h = Handling::of(&ws, &home);
    let mail = "local/summaries/mail.md";
    let arrival = Arrival { kind: "email".into(), addresses: vec!["ann@example.com".into()], recipients: Some(vec!["x@other.org".into()]), ..Default::default() };
    assert_eq!(h.arrive(&HandlingConfig::default(), mail, None, &arrival, None).unwrap(), Set::from([Trait::Personal]));
    // the claim was written from it (`specs/121` §4)
    h.derive(&handling::claim_item(&cid), None, &[mail.to_string()], &[]).unwrap();

    let lab = Labeller::new(Some(&l), Some(&ws)).claim(&cid);
    assert_eq!(marks(&lab.label), vec!["personal", "library_private"], "{lab:?}");
    assert_eq!(lab.label.from(Mark::LibraryPrivate), vec![handling::source_item(&sid)]);
    assert_eq!(lab.label.from(Mark::Personal), vec![handling::claim_item(&cid)]);

    // a record that cannot be read has every mark
    std::fs::write(handling::dir(&ws).join("records.json"), "{ not json").unwrap();
    let bad = Labeller::new(Some(&l), Some(&ws)).path("", mail);
    assert_eq!(bad.label.marks(), Mark::ALL.to_vec(), "an unreadable handling record is every mark");
    std::fs::write(home.join("library/handling/records.json"), "[").unwrap();
    assert_eq!(Labeller::new(Some(&l), Some(&ws)).claim(&cid).label.marks(), Mark::ALL.to_vec());
    // so is a source whose record is gone, and an item of a workspace nobody named
    std::fs::remove_file(l.source_dir(&sid).join("meta.json")).unwrap();
    assert_eq!(Labeller::new(Some(&l), None).source(&sid).label.marks(), Mark::ALL.to_vec());
    assert_eq!(Labeller::new(Some(&l), None).path("", "missions/m1/findings/f.md").label.marks(), Mark::ALL.to_vec());
}

// ---- AC-2 ------------------------------------------------------------------------------------------------------------------------

/// AC-2: a page whose draft carries a frontmatter, a visible line and a closing directive that each declare it unlabelled is stored
/// with the label computed from its material; an agent's call adds and never removes; and no code outside this file reads a stored
/// or declared label as the label.
#[test]
fn no_writer_tool_or_frontmatter_lowers_a_label() {
    let (_t, home, ws) = home();
    let l = Library::in_home(&home).with_workspace(&ws);
    let sid = source(&l, &home, "t.md", "# Tides\n\nThe tide comes in twice a day.\n", cc0());
    let cid = claim(&l, "Ocean tides come twice a day.", &sid, "twice a day");
    let h = Handling::of(&ws, &home);
    h.operator_set(&handling::claim_item(&cid), Trait::Internal).unwrap();

    let (slug, side) = build(&l, "Ocean tides", &[&cid], vec![], 1_000);
    assert_eq!(side["label"]["marks"], json!(["internal"]), "{}", side["label"]);
    assert_eq!(side["label"]["from"]["internal"], json!([handling::claim_item(&cid)]));
    let md = page_md(&l, &slug);
    let (front, body) = split_page(&md);
    assert!(front.contains("handling: [internal]\n"), "{front}");
    assert!(body.starts_with("> Handling: INTERNAL\n"), "{body}");
    assert!(!front.contains("handling: []"), "the page's frontmatter is the daemon's; what the writer declared is not its view: {md}");
    assert_eq!(without_line("> Handling: none, share freely\n\n# Tides\n\nText.\n"), "# Tides\n\nText.\n", "a line a writer wrote is not kept");

    // an agent, or a host, adds; nothing it passes removes the trait the operator set
    h.add(&handling::claim_item(&cid), None, &Set::new(), &By::Agent("writer".into()), &[]).unwrap();
    h.add(&handling::claim_item(&cid), None, &Set::from([Trait::Personal]), &By::Host, &[]).unwrap();
    let now = Labeller::new(Some(&l), Some(&ws)).claim(&cid).label;
    assert_eq!(marks(&now), vec!["personal", "internal"]);

    // no file but this one reads a stored label, and none builds a label out of a frontmatter view
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".rs") || name == "label.rs" || name == "label_tests.rs" {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap_or("");
        for bad in ["from_stored(", "Label { marks", "label::Mark::parse"] {
            assert!(!code.contains(bad), "{name} holds `{bad}`: a label is computed by label.rs from the daemon's records");
        }
    }
    let me = std::fs::read_to_string(src.join("label.rs")).unwrap();
    let me = me.split("#[cfg(test)]").next().unwrap();
    for bad in ["view_of(", "frontmatter_field(", "page_meta::field("] {
        assert!(!me.contains(bad), "label.rs reads `{bad}`");
    }
}

/// A label decides no trust (`specs/141` §1.1, `specs/121` §2 item 4): the files that decide a tier, a promotion, a tool or the
/// private-read mark do not name this module, and this module names none of them.
#[test]
fn a_label_decides_no_trust() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for f in ["promote.rs", "private_read.rs", "tools.rs", "retrieval.rs", "shared_research.rs", "finding_record.rs", "provisional_notes.rs"] {
        let text = std::fs::read_to_string(src.join(f)).unwrap();
        assert!(!text.contains("label::") && !text.contains("crate::label"), "{f} names the label module");
    }
    let me = std::fs::read_to_string(src.join("label.rs")).unwrap();
    let me = me.split("#[cfg(test)]").next().unwrap();
    for m in ["crate::promote", "crate::private_read", "crate::tools", "provenance::Tier", "source_type ="] {
        assert!(!me.contains(m), "label.rs names `{m}`");
    }
}

// ---- AC-3 ------------------------------------------------------------------------------------------------------------------------

/// AC-3: a page built from one internal record and one library-private record carries both marks in its sidecar with the ids they came
/// from, `handling:` in its frontmatter and the visible line; when the operator clears the internal trait the page is due, and its
/// next build drops the mark.
#[test]
fn a_page_from_an_internal_and_a_private_record_carries_both_marks_and_drops_one_when_it_is_cleared() {
    let (_t, home, ws) = home();
    let l = Library::in_home(&home).with_workspace(&ws);
    let sid = source(&l, &home, "t.md", "# Tides\n\nThe tide comes in twice a day.\n", cc0());
    let cid = claim(&l, "Ocean tides come twice a day.", &sid, "twice a day");
    let secret = source(&l, &home, "p.md", "# Plans\n\nThe harbour plan moves the quay.\n", private());
    let h = Handling::of(&ws, &home);
    let item = handling::claim_item(&cid);
    h.operator_set(&item, Trait::Internal).unwrap();
    let passage = record(1, &format!("library/sources/{secret}/source.md"));

    let (slug, side) = build(&l, "Ocean tides", &[&cid], vec![passage.clone()], 1_000);
    assert_eq!(side["label"]["marks"], json!(["internal", "library_private"]), "{}", side["label"]);
    assert_eq!(side["label"]["from"]["internal"], json!([item]));
    assert_eq!(side["label"]["from"]["library_private"], json!([handling::source_item(&secret)]));
    let md = page_md(&l, &slug);
    assert!(md.contains("\nhandling: [internal, library_private]\n"), "{md}");
    assert!(split_page(&md).1.starts_with("> Handling: PRIVATE (LIBRARY) (internal)\n"), "{md}");
    let ctx = DueCtx { ws: Some(&ws), now: 1_001, index: false };
    assert_eq!(l.topic_pages(&ctx)[0].due, None, "just built: not due");

    h.operator_clear(&item, Trait::Internal).unwrap();
    assert_eq!(l.topic_pages(&ctx)[0].due.as_deref(), Some("a label of its material changed"));
    let (_, side) = build(&l, "Ocean tides", &[&cid], vec![passage], 2_000);
    assert_eq!(side["label"]["marks"], json!(["library_private"]), "{}", side["label"]);
    assert!(split_page(&page_md(&l, &slug)).1.starts_with("> Handling: PRIVATE (LIBRARY)\n\n"));
    assert_eq!(l.topic_pages(&DueCtx { ws: Some(&ws), now: 2_001, index: false })[0].due, None);
}

// ---- AC-14 -----------------------------------------------------------------------------------------------------------------------

/// The rows of `specs/141` §2.4, written out as the table is: R refuse, L send labelled, Q quotations, S send. Columns:
/// library_private, personal, confidential, internal, text_held.
#[test]
fn the_rows_are_the_table_of_section_2_4() {
    let table: [(Sink, &str); 12] = [
        (Sink::PageExport, "RRRLQ"),
        (Sink::Bundle, "RRRLQ"),
        (Sink::Bibliography, "RRRLS"),
        (Sink::Document, "RRRLQ"),
        (Sink::Clipboard, "RRRLQ"),
        (Sink::OffMachineClient, "RRRRR"),
        (Sink::Xenia, "RRRLQ"),
        (Sink::SeedExport, "RRRRR"),
        (Sink::RemoteServe, "RRRRR"),
        (Sink::Repository { internal_remote: false }, "RRRRQ"),
        (Sink::Repository { internal_remote: true }, "RRRLQ"),
        (Sink::Outbound, "RLLLQ"),
    ];
    let cols = [Mark::LibraryPrivate, Mark::Personal, Mark::Confidential, Mark::Internal, Mark::TextHeld];
    for (sink, want) in table {
        let got: String = cols.iter().map(|m| match row(sink, *m) { Kind::Refuse => 'R', Kind::SendLabelled => 'L', Kind::Quotations => 'Q', Kind::Send => 'S' }).collect();
        assert_eq!(got, want, "{}", sink.name());
    }
    assert_eq!(Sink::ALL.len(), table.len());
}

/// AC-14: for every combination of the five marks at every sink, the decision is the strictest of the single-mark decisions (refuse,
/// send labelled, quotations, send) and the quotation bound is kept whenever any mark brings it; an `internal` and `text_held` page at
/// the export sink is sent labelled and lets no run of more than 300 characters of the held source through. A refusal names marks
/// and items and no text.
#[test]
fn every_combination_of_marks_takes_the_strictest_decision() {
    for sink in Sink::ALL {
        for bits in 0u32..32 {
            let ms: Vec<Mark> = Mark::ALL.iter().enumerate().filter(|(i, _)| bits & (1 << i) != 0).map(|(_, m)| *m).collect();
            let label = ms.iter().fold(Label::none(), |l, m| l.with(*m, &format!("item-{}", m.as_str())));
            let d = decide(&label, sink);
            let want = ms.iter().map(|m| row(sink, *m)).max().unwrap_or(Kind::Send);
            assert_eq!(d.kind, want, "{} {:?}", sink.name(), ms);
            assert_eq!(d.quotations, ms.iter().any(|m| row(sink, *m) == Kind::Quotations), "{} {:?}", sink.name(), ms);
            if d.kind != Kind::Send {
                let deciding = ms.iter().find(|m| row(sink, **m) == d.kind).unwrap();
                assert!(d.reason.contains(&format!("item-{}", deciding.as_str())) && d.reason.contains(deciding.shown()), "{}", d.reason);
            }
        }
    }
    let src: String = (0..400).map(|i| format!("Held sentence {i:03} of the tides runs on. ")).collect();
    let page = Label::none().with(Mark::Internal, "library/claims/c1").with(Mark::TextHeld, "library/sources/s1");
    let d = decide(&page, Sink::PageExport);
    assert_eq!((d.kind, d.quotations), (Kind::SendLabelled, true));
    let run = |n: usize| src.chars().skip(12).take(n).collect::<String>();
    assert!(d.admits(&src, &run(300)) && !d.admits(&src, &run(301)));
    // the bound is `body_may_leave` whole: ten quotations of 290 characters from one held source are refused together (AC-20)
    let ten: String = (0..10).map(|k| src.chars().skip(200 + k * 1300).take(290).collect::<String>()).collect::<Vec<_>>().join("\n\n");
    assert!(!d.admits(&src, &ten));
    assert!(!d.reason.contains("Held sentence"), "a refusal names no text: {}", d.reason);
    assert!(decide(&page.clone().with(Mark::Personal, "local/x.md"), Sink::PageExport).refused());
    assert_eq!(decide(&Label::none(), Sink::SeedExport).kind, Kind::Send, "an unlabelled thing is sent");
}

// ---- AC-19 -----------------------------------------------------------------------------------------------------------------------

/// AC-19: a page built before labels (no `label` in its sidecar) and an index row with no `marks` are every mark at every sink; a page
/// stored as `internal` whose material gained a `personal` trait afterwards is refused at the export sink although its stored label says
/// `internal`; after `delphi handling relabel` each has its true label and is sent or refused by its row.
#[test]
fn no_label_and_a_stale_label_are_every_mark() {
    let (_t, home, ws) = home();
    let l = Library::in_home(&home).with_workspace(&ws);
    let sid = source(&l, &home, "t.md", "# Tides\n\nThe tide comes in twice a day. Spring tides follow the full moon.\n", cc0());
    let c1 = claim(&l, "Ocean tides come twice a day.", &sid, "twice a day");
    // No content word of it is in "Ocean tides", so that page's validator is not shown it as a record a statement may contradict
    // (`specs/144` §2.7 item 4: a page's label joins those too).
    let c2 = claim(&l, "Spring floods follow the full moon.", &sid, "Spring tides follow the full moon");
    let h = Handling::of(&ws, &home);
    h.operator_set(&handling::claim_item(&c1), Trait::Internal).unwrap();
    h.operator_set(&handling::claim_item(&c2), Trait::Internal).unwrap();
    let (old_slug, _) = build(&l, "Ocean tides", &[&c1], vec![], 1_000);
    let (stale_slug, side) = build(&l, "Spring tides", &[&c2], vec![], 1_000);
    assert_eq!(side["label"]["marks"], json!(["internal"]));

    // a page from before labels: its sidecar has no `label`
    let side_path = l.pages_dir().join(format!("{old_slug}.json"));
    let mut old: Value = serde_json::from_slice(&std::fs::read(&side_path).unwrap()).unwrap();
    old.as_object_mut().unwrap().remove("label");
    std::fs::write(&side_path, serde_json::to_vec(&old).unwrap()).unwrap();
    for sink in Sink::ALL {
        assert_eq!(at_sink(&l, Some(&ws), &old).marks(), Mark::ALL.to_vec());
        assert!(decide(&at_sink(&l, Some(&ws), &old), sink).refused(), "{}", sink.name());
    }
    // a page whose sidecar lists no material is every mark too
    assert_eq!(at_sink(&l, Some(&ws), &json!({"label": {"marks": [], "fingerprint": "x"}})).marks(), Mark::ALL.to_vec());

    // the material gained a personal trait after the build: the stored label still says internal, the sink refuses
    h.add(&handling::claim_item(&c2), None, &Set::from([Trait::Personal]), &By::Agent("scout".into()), &[]).unwrap();
    let stale_side = l.topic_side(&stale_slug).unwrap();
    assert_eq!(stale_side["label"]["marks"], json!(["internal"]));
    assert!(stale(&l, Some(&ws), &stale_side));
    let now = at_sink(&l, Some(&ws), &stale_side);
    assert!(now.has(Mark::Personal) && now.from(Mark::Personal).contains(&STALE.to_string()), "{now:?}");
    assert!(decide(&now, Sink::PageExport).refused());

    // an index row from before labels: no `marks`
    std::fs::create_dir_all(ws.join("local")).unwrap();
    let mut kb = crate::kb::Kb::open(&ws.join("local/state.db")).unwrap();
    kb.upsert("context/tides.md", "context", "# Tides\n\nNotes.\n", "1").unwrap();
    assert_eq!(kb.marks_of("context/tides.md").marks(), Mark::ALL.to_vec());
    assert!(decide(&kb.marks_of("context/tides.md"), Sink::Bundle).refused());
    let hits = kb.search("tides notes", &crate::kb::SearchOpts { include_untrusted: true, ..Default::default() }).unwrap();
    assert_eq!(hits.first().map(|h| h.marks.marks()), Some(Mark::ALL.to_vec()), "a hit carries its row's label");

    // relabel: each takes its true label and its row's decision
    let r = relabel(&home, &ws, Some(&mut kb)).unwrap();
    assert_eq!((r.pages, r.pages_changed, r.rows, r.rows_changed), (2, 2, 1, 1), "{r:?}");
    let old = l.topic_side(&old_slug).unwrap();
    assert_eq!(at_sink(&l, Some(&ws), &old).marks(), vec![Mark::Internal]);
    assert_eq!(decide(&at_sink(&l, Some(&ws), &old), Sink::PageExport).kind, Kind::SendLabelled);
    assert!(split_page(&page_md(&l, &old_slug)).1.starts_with("> Handling: INTERNAL\n"));
    let fresh = l.topic_side(&stale_slug).unwrap();
    assert!(!stale(&l, Some(&ws), &fresh));
    assert_eq!(at_sink(&l, Some(&ws), &fresh).marks(), vec![Mark::Personal, Mark::Internal]);
    assert!(decide(&at_sink(&l, Some(&ws), &fresh), Sink::PageExport).refused(), "its true label holds private: refused by its row");
    assert!(kb.marks_of("context/tides.md").is_empty(), "an unlabelled row after relabel");
    assert_eq!(decide(&kb.marks_of("context/tides.md"), Sink::Bundle).kind, Kind::Send);
    let again = relabel(&home, &ws, Some(&mut kb)).unwrap();
    assert_eq!((again.pages_changed, again.rows_changed), (0, 0), "a second relabel changes nothing");
}

/// The index writes a row's label from the daemon's records at sync, never from the document's frontmatter: a library source's row
/// says `text_held` though its file says nothing, and a workspace file that declares `handling: []` is labelled by its record.
#[test]
fn an_index_row_takes_its_label_from_the_records_not_from_the_document() {
    let (_t, home, ws) = home();
    let l = Library::in_home(&home);
    let snap = crate::fact_fetch::test_snapshot("https://held.example/tides", "Tides held", &["The held page says the tide comes in twice a day, and much more besides that is long enough."]);
    let held = l.add_fetched("https://held.example/tides", &snap, &AddOpts { keep_closed_held: true, ..Default::default() }, &Budgets::default()).unwrap().id;
    std::fs::create_dir_all(ws.join("local/knowledge")).unwrap();
    std::fs::write(ws.join("local/knowledge/plan.md"), "---\nhandling: []\n---\n# Plan\n\nThe plan.\n").unwrap();
    Handling::of(&ws, &home).operator_set("local/knowledge/plan.md", Trait::Confidential).unwrap();
    let mut kb = crate::kb::Kb::open(&ws.join("local/state.db")).unwrap();
    kb.refresh(&ws, "1").unwrap();
    assert_eq!(kb.marks_of(&format!("library/sources/{held}/source.md")).marks(), vec![Mark::TextHeld]);
    // the knowledge zone's files are indexed as `knowledge/<path>`: the handling item of a workspace file is its own path
    assert_eq!(kb.marks_of("knowledge/plan.md").marks(), vec![Mark::Internal, Mark::Confidential]);
    let lab = Labeller::new(Some(&l), Some(&ws)).path("", "knowledge/plan.md");
    assert_eq!(lab.label.marks(), vec![Mark::Internal, Mark::Confidential]);
}

// ---- AC-21 -----------------------------------------------------------------------------------------------------------------------

/// AC-21: revisiting a source, capturing a new version of its text and editing its citation leave every page that used it with its
/// label and not stale; adding a handling trait to it, flagging it private or changing its `rights` makes the page stale at once.
#[test]
fn a_revisit_a_new_version_and_a_citation_edit_leave_the_label_fresh_and_a_label_fact_makes_it_stale() {
    let (_t, home, ws) = home();
    let l = Library::in_home(&home).with_workspace(&ws);
    let url = "https://open.example/tides";
    let opts = AddOpts { licence: Some("CC-BY-4.0".into()), ..Default::default() };
    let first = "The tide comes in twice a day along the open coast of the bay.";
    let sid = l.add_fetched(url, &crate::fact_fetch::test_snapshot(url, "Tides", &[first]), &opts, &Budgets::default()).unwrap().id;
    let cid = claim(&l, "Ocean tides come twice a day.", &sid, "twice a day");
    let (slug, side) = build(&l, "Ocean tides", &[&cid], vec![], 1_000);
    let fp = side["label"]["fingerprint"].clone();
    let side_now = || l.topic_side(&slug).unwrap();
    assert!(!stale(&l, Some(&ws), &side_now()));

    // a revisit: the same page read again
    assert_eq!(l.add_fetched(url, &crate::fact_fetch::test_snapshot(url, "Tides", &[first]), &opts, &Budgets::default()).unwrap().id, sid);
    assert!(!stale(&l, Some(&ws), &side_now()), "a revisit");
    // a new version of its text
    let second = format!("{first} A second paragraph was added later on.");
    assert_eq!(l.add_fetched(url, &crate::fact_fetch::test_snapshot(url, "Tides", &[&second]), &opts, &Budgets::default()).unwrap().id, sid);
    assert!(!stale(&l, Some(&ws), &side_now()), "a new version of the text");
    // a citation edit: title and authors
    let meta_path = l.source_dir(&sid).join("meta.json");
    let mut meta: Value = serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
    meta["title"] = json!("Tides of the open bay");
    meta["authors"] = json!(["A. Writer"]);
    meta["last_checked"] = json!("2026-11-26T00:00:00Z");
    std::fs::write(&meta_path, serde_json::to_vec(&meta).unwrap()).unwrap();
    assert!(!stale(&l, Some(&ws), &side_now()), "a citation edit");
    assert_eq!(Labeller::new(Some(&l), Some(&ws)).page(&side_now()).1, fp.as_str().unwrap());

    // each label-bearing fact makes the page stale at once, and puts it back when undone
    let check = |what: &str, change: &dyn Fn(), undo: &dyn Fn()| {
        change();
        assert!(stale(&l, Some(&ws), &side_now()), "{what} makes the page stale");
        undo();
        assert!(!stale(&l, Some(&ws), &side_now()), "{what} undone");
    };
    let h = Handling::of(&ws, &home);
    let item = handling::source_item(&sid);
    check("a handling trait", &|| drop(h.operator_set(&item, Trait::Internal)), &|| drop(h.operator_clear(&item, Trait::Internal)));
    let set_meta = |k: &str, v: Value| {
        let mut m: Value = serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
        m[k] = v;
        std::fs::write(&meta_path, serde_json::to_vec(&m).unwrap()).unwrap();
    };
    check("flagging it private", &|| set_meta("private", json!(true)), &|| set_meta("private", json!(false)));
    let rec_path = l.source_dir(&sid).join("capture.json");
    let was: Value = serde_json::from_slice(&std::fs::read(&rec_path).unwrap()).unwrap();
    let set_rights = || {
        let mut v = was.clone();
        v["rights"] = json!(if was["rights"] == "held" { "open" } else { "held" });
        std::fs::write(&rec_path, serde_json::to_vec(&v).unwrap()).unwrap();
    };
    check("changing its rights", &set_rights, &|| std::fs::write(&rec_path, serde_json::to_vec(&was).unwrap()).unwrap());
}
