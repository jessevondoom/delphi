//! The sinks of `specs/141` slice B through their real entry points (AC-4, AC-5): a bundle (the copy of a source into a bundle's
//! `sources/`, and the files of a bundle before it is packaged, published or exported), a bibliography, and a Xenia publish. One test
//! per sink, named as §6 names them; the registry test in `egress.rs` fails when one of them is gone.

use super::*;
use crate::handling::{self, Handling, Trait};
use crate::label::{Mark, STALE};
use crate::library::{bib, may_leave as gate, AddOpts, Budgets, Library};
use serde_json::json;
use std::path::PathBuf;

/// A held source's text, long enough that four quotations of 290 characters fit its aggregate cap.
fn long_text() -> String {
    (0..400).map(|i| format!("Held sentence {i:03} of the tides runs on. ")).collect()
}

fn take(text: &str, skip: usize, n: usize) -> String {
    text.chars().skip(skip).take(n).collect()
}

struct Fx {
    _t: tempfile::TempDir,
    home: PathBuf,
    ws: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().to_path_buf();
        let ws = home.join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        Fx { _t: t, home, ws }
    }

    fn lib(&self) -> Library {
        Library::in_home(&self.home).with_workspace(&self.ws)
    }

    /// A source of the library carrying exactly `mark`, with `text` as its kept text. Returns its id.
    fn source(&self, name: &str, text: &str, mark: Mark) -> String {
        let p = self.home.join(name);
        std::fs::write(&p, text).unwrap();
        let opts = if mark == Mark::LibraryPrivate { AddOpts { private: true, licence: Some("own".into()), ..Default::default() } } else { AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() } };
        let id = self.lib().add_file(&p, &opts, &Budgets::default()).unwrap().id;
        let h = Handling::of(&self.ws, &self.home);
        match mark {
            Mark::Personal => {
                h.operator_set(&handling::source_item(&id), Trait::Personal).unwrap();
            }
            Mark::Internal => {
                h.operator_set(&handling::source_item(&id), Trait::Internal).unwrap();
            }
            Mark::Confidential => {
                h.operator_set(&handling::source_item(&id), Trait::Confidential).unwrap();
            }
            Mark::TextHeld => {
                let path = self.lib().source_dir(&id).join("meta.json");
                let mut meta: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                meta["text_held"] = json!(true);
                std::fs::write(&path, meta.to_string()).unwrap();
            }
            Mark::LibraryPrivate => {}
        }
        let label = crate::label::Labeller::new(Some(&self.lib()), Some(&self.ws)).source(&id).label;
        // confidential implies internal (`specs/121`); every other fixture carries exactly its one mark
        let extra: Vec<Mark> = if mark == Mark::Confidential { vec![Mark::Internal, Mark::Confidential] } else { vec![mark] };
        assert_eq!(label.marks(), extra, "the fixture carries its mark");
        id
    }

    /// The source's kept text, as the library stored it.
    fn text(&self, id: &str) -> String {
        std::fs::read_to_string(self.lib().source_dir(id).join("source.md")).unwrap()
    }

    /// A topic page of the library whose material is `sources`, its body `body`, labelled by the daemon (`delphi handling relabel`).
    fn page(&self, slug: &str, sources: &[&str], body: &str) -> String {
        let dir = self.lib().pages_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let material: Vec<Value> = sources.iter().map(|s| json!({"id": format!("s-{s}")})).collect();
        std::fs::write(dir.join(format!("{slug}.json")), json!({"kind": "topic_page", "slug": slug, "title": slug, "material": material}).to_string()).unwrap();
        std::fs::write(dir.join(format!("{slug}.md")), format!("---\ntitle: {slug}\n---\n\n{body}")).unwrap();
        crate::label::relabel(&self.home, &self.ws, None).unwrap();
        std::fs::read_to_string(dir.join(format!("{slug}.md"))).unwrap()
    }

    /// A bundle directory holding `files`.
    fn bundle(&self, files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        d
    }
}

const BODY: &str = "# Tides\n\nThe tide comes in twice a day, and spring tides follow the full moon.\n";

/// What a sink did with one output: the row's letter (R refuse, L send labelled, Q quotations, S send).
fn outcome(nobody: Result<Sent, String>, yes: Result<Sent, String>) -> char {
    match (nobody, yes) {
        (Err(_), Err(_)) => 'R',
        (Err(_), Ok(s)) => {
            assert_eq!(s.decision.kind, Kind::SendLabelled);
            'L'
        }
        (Ok(s), Ok(_)) if s.decision.quotations => 'Q',
        (Ok(_), Ok(_)) => 'S',
        (Ok(_), Err(e)) => panic!("a confirmed send refused what an unconfirmed one sent: {e}"),
    }
}

/// The row of `specs/141` §2.4 for `sink`, columns in [`Mark::ALL`] order (personal, internal, confidential, library_private, text_held).
fn want(sink: Sink) -> String {
    Mark::ALL.iter().map(|m| match label::row(sink, *m) { Kind::Refuse => 'R', Kind::SendLabelled => 'L', Kind::Quotations => 'Q', Kind::Send => 'S' }).collect()
}

/// AC-4, the bundle row, through both of its entry points. (1) `delphi bundle sources add` (`Library::export_to_bundle_with`): a
/// source with each one mark is copied, refused or asked for as the row says (a whole held text is more than the quotation bound
/// lets out, so it is refused). (2) The files of a bundle about to be packaged, published or exported (`egress::check_path` at
/// `Sink::Bundle`): a copy of a source made earlier is labelled as the source is now, and a held source's quotation of 250 characters
/// passes under the bound while 400 are refused. A refusal names the marks and the items, and no text.
#[test]
fn sink_bundle_follows_its_row() {
    let fx = Fx::new();
    let held = long_text();
    let mut got_copy = String::new();
    let mut got_files = String::new();
    for m in Mark::ALL {
        let text = if m == Mark::TextHeld { held.clone() } else { BODY.to_string() };
        let id = fx.source(&format!("{}.md", m.as_str()), &text, m);
        // (1) the copy into a bundle's sources/
        let copy = |confirm: Confirm<'_>| {
            let d = tempfile::tempdir().unwrap();
            let r = fx.lib().export_to_bundle_with(d.path(), std::slice::from_ref(&id), false, confirm).remove(0).1;
            assert_eq!(r.is_ok(), d.path().join("sources").join(&id).join("meta.json").is_file(), "a refused copy writes nothing");
            r
        };
        let (nobody, yes) = (copy(Confirm::Nobody), copy(Confirm::Terminal(&mut |_| true)));
        got_copy.push(match (nobody, yes) {
            (Err(_), Err(_)) => 'R',
            (Err(_), Ok(_)) => 'L',
            (Ok(_), _) => 'S',
        });
        if let Err(e) = copy(Confirm::Nobody) {
            assert!(e.contains(&id) || e.contains("text_held") || e.contains("private"), "the refusal names the item or the mark: {e}");
            assert!(!e.contains("tide comes in") && !e.contains("Held sentence"), "a refusal names no text: {e}");
        }
        // (2) the bundle's files: a copy of the source made earlier, a short quotation of it, the operator's own note
        let quoted = take(&fx.text(&id), 0, 250);
        let meta_rel = format!("sources/{id}/meta.json");
        let b = fx.bundle(&[(meta_rel.as_str(), r#"{"title": "t"}"#), ("knowledge/note.md", quoted.as_str())]);
        let check = |confirm: Confirm<'_>| check_path(&fx.home, Some(&fx.ws), b.path(), Sink::Bundle, confirm);
        got_files.push(outcome(check(Confirm::Nobody), check(Confirm::Terminal(&mut |_| true))));
        if let Err(e) = check(Confirm::Nobody) {
            assert!(e.contains(&handling::source_item(&id)) && e.contains(m.shown()), "the refusal names the mark and its item: {e}");
            assert!(!e.contains("tide comes in") && !e.contains("Held sentence"), "a refusal names no text: {e}");
        }
        if m == Mark::TextHeld {
            let over = fx.bundle(&[("knowledge/note.md", take(&held, 0, 400).as_str())]);
            let e = check_path(&fx.home, Some(&fx.ws), over.path(), Sink::Bundle, Confirm::Nobody).unwrap_err();
            assert!(e.contains(&format!("s-{id}")) && !e.contains("Held sentence"), "{e}");
        }
    }
    // the copy: R for every refusing mark and for a whole held text; L for internal
    assert_eq!(got_copy, "RLRRR");
    assert_eq!(got_files, want(Sink::Bundle));
    // an internal file without its visible line is not sent, even when the operator would say yes
    let id = fx.source("plain-internal.md", BODY, Mark::Internal);
    let b = fx.bundle(&[(format!("sources/{id}/source.md").as_str(), BODY)]);
    let e = check_path(&fx.home, Some(&fx.ws), b.path(), Sink::Bundle, Confirm::Terminal(&mut |_| true)).unwrap_err();
    assert!(e.contains("> Handling: INTERNAL"), "{e}");
    // and the operator who says no keeps it here
    let b = fx.bundle(&[(format!("sources/{id}/source.md").as_str(), format!("> Handling: INTERNAL\n\n{BODY}").as_str())]);
    assert!(check_path(&fx.home, Some(&fx.ws), b.path(), Sink::Bundle, Confirm::Terminal(&mut |_| false)).is_err());
    let mut asked = String::new();
    check_path(&fx.home, Some(&fx.ws), b.path(), Sink::Bundle, Confirm::Terminal(&mut |q| {
        asked = q.to_string();
        true
    }))
    .unwrap();
    assert!(asked.contains("> Handling: INTERNAL") && asked.contains(&handling::source_item(&id)), "the operator is shown the marks and the items: {asked}");
    // a confirmed copy of an internal source carries the line, written by the daemon
    let d = tempfile::tempdir().unwrap();
    fx.lib().export_to_bundle_with(d.path(), std::slice::from_ref(&id), false, Confirm::Terminal(&mut |_| true)).remove(0).1.unwrap();
    assert!(std::fs::read_to_string(d.path().join("sources").join(&id).join("source.md")).unwrap().starts_with("> Handling: INTERNAL\n\n"));
    assert!(check_path(&fx.home, Some(&fx.ws), d.path(), Sink::Bundle, Confirm::Terminal(&mut |_| true)).is_ok(), "and the bundle that holds it is sent once confirmed");
}

/// AC-4, the bibliography row, through `delphi sources bib` (`bib::export_with`): a refusing mark withholds the citation; an internal
/// source's citation is sent only when the operator confirms, under the visible line; a held source's citation is sent.
#[test]
fn sink_bibliography_follows_its_row() {
    let fx = Fx::new();
    let mut got = String::new();
    for m in Mark::ALL {
        let text = if m == Mark::TextHeld { long_text() } else { BODY.to_string() };
        let id = fx.source(&format!("{}.md", m.as_str()), &text, m);
        let ids = std::slice::from_ref(&id);
        let nobody = bib::export_with(&fx.lib(), ids, bib::Format::Plain, Confirm::Nobody).unwrap();
        let yes = bib::export_with(&fx.lib(), ids, bib::Format::Plain, Confirm::Terminal(&mut |_| true)).unwrap();
        got.push(match (nobody.withheld, yes.withheld) {
            (1, 1) => 'R',
            (1, 0) => {
                assert!(yes.text.starts_with("> Handling: INTERNAL\n"), "a labelled bibliography carries its line: {}", yes.text);
                'L'
            }
            (0, 0) => 'S',
            other => panic!("{other:?}"),
        });
        // csl-json has no place for the visible line: a labelled citation is not sent in it
        if m == Mark::Internal {
            assert_eq!(bib::export_with(&fx.lib(), ids, bib::Format::CslJson, Confirm::Terminal(&mut |_| true)).unwrap().withheld, 1);
            assert!(bib::export_with(&fx.lib(), ids, bib::Format::BibTex, Confirm::Terminal(&mut |_| true)).unwrap().text.starts_with("% Handling: INTERNAL\n"));
        }
    }
    assert_eq!(got, want(Sink::Bibliography));
}

/// AC-4, the Xenia row: a file named for a publish, at the check both senders run (`egress::check_path` at `Sink::Xenia`, behind the
/// publish card). A source's own file is refused or sent as the row says; an internal file passes with its visible line, its marks
/// for the card; a held source's file is refused whole and a note quoting 250 characters of it passes.
#[test]
fn sink_xenia_follows_its_row() {
    let fx = Fx::new();
    let mut got = String::new();
    for m in Mark::ALL {
        let text = if m == Mark::TextHeld { long_text() } else { BODY.to_string() };
        let id = fx.source(&format!("{}.md", m.as_str()), &text, m);
        let file = fx.lib().source_dir(&id).join("source.md");
        if m == Mark::Internal {
            let e = check_path(&fx.home, Some(&fx.ws), &file, Sink::Xenia, Confirm::Card).unwrap_err();
            assert!(e.contains("> Handling: INTERNAL"), "{e}");
            std::fs::write(&file, format!("> Handling: INTERNAL\n\n{BODY}")).unwrap();
        }
        let nobody = check_path(&fx.home, Some(&fx.ws), &file, Sink::Xenia, Confirm::Nobody);
        let card = check_path(&fx.home, Some(&fx.ws), &file, Sink::Xenia, Confirm::Card);
        let mut c = outcome(nobody, card.clone());
        if m == Mark::TextHeld {
            assert_eq!(c, 'R', "the whole held text is more than the bound lets out");
            let note = fx.home.join("note.md");
            std::fs::write(&note, take(&text, 0, 250)).unwrap();
            check_path(&fx.home, Some(&fx.ws), &note, Sink::Xenia, Confirm::Nobody).expect("a quotation of 250 characters is sent under the bound");
            std::fs::write(&note, take(&text, 0, 400)).unwrap();
            assert!(check_path(&fx.home, Some(&fx.ws), &note, Sink::Xenia, Confirm::Card).is_err(), "400 are not");
            c = 'Q';
        }
        if m == Mark::Internal {
            assert!(card.unwrap().line().contains("> Handling: INTERNAL"), "the card shows the marks");
        }
        got.push(c);
    }
    assert_eq!(got, want(Sink::Xenia));
}

/// AC-5: a bundle that includes a page whose material holds a held source carries no run of more than 300 characters of that source:
/// a copy of the page that quotes 400 characters is refused before anything is packaged, naming the mark and the source and no
/// passage; a page that quotes 250 is sent under the bound. A copy of a personal page is refused wherever it was copied to.
#[test]
fn a_bundle_with_a_page_of_held_material_carries_no_run_of_its_text() {
    let fx = Fx::new();
    let text = long_text();
    let held = fx.source("held.md", &text, Mark::TextHeld);
    let own: String = (0..12).map(|i| format!("The writer's own line {i:02} on the tides. ")).collect();
    let long = fx.page("tides-long", &[&held], &format!("# Tides\n\n{own}\n\n{}\n", take(&text, 100, 400)));
    let short = fx.page("tides-short", &[&held], &format!("# Tides short\n\n{own}\n\n{}\n", take(&text, 100, 250)));
    assert!(long.contains("> Handling: TEXT KEPT FOR LOCAL READING"), "{long}");
    let b = fx.bundle(&[("knowledge/tides.md", &long), ("bundle.toml", "name = \"b\"\n")]);
    let e = check_path(&fx.home, Some(&fx.ws), b.path(), Sink::Bundle, Confirm::Nobody).unwrap_err();
    assert!(e.contains(&format!("s-{held}")) && e.contains("text_held"), "the refusal names the mark and the item: {e}");
    assert!(!e.contains("Held sentence"), "and no passage: {e}");
    let b = fx.bundle(&[("knowledge/tides.md", &short)]);
    let s = check_path(&fx.home, Some(&fx.ws), b.path(), Sink::Bundle, Confirm::Nobody).unwrap();
    assert!(s.label.has(Mark::TextHeld) && s.decision.quotations, "{:?}", s.label);
    // a personal page, copied out of the library under another name with its line removed
    let me = fx.source("me.md", "# Me\n\nA personal note about the tides and the moon.\n", Mark::Personal);
    let body: String = (0..20).map(|i| format!("Personal line {i:02} about the tides. ")).collect();
    let page = fx.page("mine", &[&me], &body);
    let copied = crate::label::without_line(crate::label::split_page(&page).1);
    let b = fx.bundle(&[("knowledge/renamed.md", copied.as_str())]);
    let e = check_path(&fx.home, Some(&fx.ws), b.path(), Sink::Bundle, Confirm::Terminal(&mut |_| true)).unwrap_err();
    assert!(e.contains("private") && e.contains(&handling::source_item(&me)) && e.contains("knowledge/renamed.md"), "{e}");
    assert!(!e.contains("Personal line"), "{e}");
}

/// AC-19 at these sinks: a page copied into a bundle is labelled as the daemon's records say at the send, never by its stored label
/// alone: a page stored as unlabelled whose source was flagged personal afterwards is refused; a page with no stored label is every
/// mark.
#[test]
fn every_sink_recomputes_the_label_from_the_records() {
    let fx = Fx::new();
    let id = fx.source("plain.md", BODY, Mark::Internal);
    let body: String = (0..20).map(|i| format!("Plain line {i:02} about the tides. ")).collect();
    let page = fx.page("plain", &[&id], &body);
    Handling::of(&fx.ws, &fx.home).operator_set(&handling::source_item(&id), Trait::Personal).unwrap();
    let b = fx.bundle(&[("knowledge/plain.md", page.as_str())]);
    let e = check_path(&fx.home, Some(&fx.ws), b.path(), Sink::Bundle, Confirm::Terminal(&mut |_| true)).unwrap_err();
    assert!(e.contains(STALE) || e.contains("private"), "{e}");
    // no stored label at all: every mark
    let side = fx.lib().pages_dir().join("plain.json");
    let mut v: Value = serde_json::from_slice(&std::fs::read(&side).unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("label");
    std::fs::write(&side, v.to_string()).unwrap();
    let s = label::of_files(&fx.home, Some(&fx.ws), Some(b.path()), &crate::held_text::files_under(b.path()));
    assert_eq!(s.label.marks(), Mark::ALL.to_vec());
}

/// `egress::check` is `check_label` over the record's own label, and agrees with the library's gate on every channel: nothing that
/// was refused is now sent, and nothing that was sent is now refused.
#[test]
fn check_is_check_label_over_the_records_own_label() {
    let fx = Fx::new();
    let held = fx.source("held.md", &long_text(), Mark::TextHeld);
    let metas = [
        json!({"private": true}),
        json!({"licence": "private"}),
        json!({"private": false}),
        json!({"licence": "CC0-1.0", "capture_level": "full"}),
        json!({"licence": "all rights reserved", "capture_level": "full"}),
        json!({"text_held": true}),
        fx.lib().read_meta(&held).unwrap(),
        json!("unreadable"),
        Value::Null,
    ];
    for c in Channel::ALL {
        for m in &metas {
            assert_eq!(check(m, c).is_ok(), m.is_object() && gate(m, c).is_ok(), "{c:?} {m}");
        }
    }
    assert_eq!(label::of_meta(&json!("x")).marks(), Mark::ALL.to_vec(), "an unreadable record carries every mark");
}

/// The `Q` column is the one predicate (AC-20): an output of ten quotations of 290 characters from one held source is refused at the
/// bundle and Xenia sinks, as `held_text::body_may_leave` refuses it.
#[test]
fn q_is_body_may_leave() {
    let fx = Fx::new();
    let text = long_text();
    let id = fx.source("held.md", &text, Mark::TextHeld);
    let stored = fx.text(&id);
    let ten: String = (0..10).map(|k| take(&stored, 200 + k * 1300, 290)).collect::<Vec<_>>().join("\n\n");
    assert!(!crate::held_text::body_may_leave(&stored, &ten));
    let note = fx.home.join("ten.md");
    std::fs::write(&note, &ten).unwrap();
    for sink in [Sink::Bundle, Sink::Xenia] {
        assert!(check_path(&fx.home, Some(&fx.ws), &note, sink, Confirm::Nobody).is_err(), "{}", sink.name());
    }
    let four: String = (0..4).map(|k| take(&stored, 200 + k * 1300, 290)).collect::<Vec<_>>().join("\n\n");
    assert!(crate::held_text::body_may_leave(&stored, &four));
    std::fs::write(&note, &four).unwrap();
    assert!(check_path(&fx.home, Some(&fx.ws), &note, Sink::Bundle, Confirm::Nobody).is_ok());
}

/// AC-16: a bibliography export of a held source whose title holds 400 characters of the source's text sends at most 300 of them,
/// and sends no abstract and no note of its record, in every format.
#[test]
fn no_exported_field_holds_a_run_of_held_text() {
    let fx = Fx::new();
    let id = fx.source("held.md", &long_text(), Mark::TextHeld);
    let text = fx.text(&id);
    let path = fx.lib().source_dir(&id).join("meta.json");
    let mut meta: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    meta["title"] = json!(take(&text, 40, 400));
    meta["abstract"] = json!(format!("ABSTRACT-MARK {}", take(&text, 1000, 120)));
    meta["note"] = json!(format!("NOTE-MARK {}", take(&text, 2000, 120)));
    std::fs::write(&path, meta.to_string()).unwrap();
    for f in [bib::Format::Plain, bib::Format::CslJson, bib::Format::BibTex] {
        let e = bib::export_with(&fx.lib(), std::slice::from_ref(&id), f, Confirm::Nobody).unwrap();
        assert_eq!(e.withheld, 0, "{f:?}: the citation is sent");
        assert!(!crate::held_text::carries_run(&text, &e.text), "{f:?}: no run of more than 300 characters: {}", e.text);
        assert!(e.text.contains(&take(&text, 40, 120)), "{f:?}: the title is cut, not dropped: {}", e.text);
        assert!(!e.text.contains("ABSTRACT-MARK") && !e.text.contains("NOTE-MARK"), "{f:?}: no abstract and no note: {}", e.text);
    }
}

// ---- slice C: the page reply, the app's export, print and copy, a client off this machine, a document, seed-export, remote serve ----

/// The row's letter for a decision the window is given (`egress::view_decision`).
fn letter(d: &Value) -> char {
    match (d["decision"].as_str(), d["quotations"] == true) {
        (Some("refuse"), _) => 'R',
        (Some("labelled"), _) => 'L',
        (Some("send"), true) => 'Q',
        (Some("send"), false) => 'S',
        other => panic!("not a decision: {other:?} in {d}"),
    }
}

impl Fx {
    /// One source with exactly `m` and a topic page built from it alone, labelled by the daemon. Returns the source and the slug.
    fn page_of(&self, m: Mark) -> (String, String) {
        let text = if m == Mark::TextHeld { long_text() } else { BODY.to_string() };
        let id = self.source(&format!("{}.md", m.as_str()), &text, m);
        let slug = format!("page-{}", m.as_str().replace('_', "-"));
        self.page(&slug, &[&id], &format!("Generated page. Not authoritative.\n\n{BODY}"));
        (id, slug)
    }

    fn get(&self, slug: &str, here: bool) -> Result<Value, String> {
        crate::encyclopedia::pages_get(&self.home, &self.ws, 1_000, &json!({"slug": slug}), here)
    }
}

/// AC-4 and AC-6, the page-export row through its real entry point, the page reply (`pages.get`) the app's export and print act on:
/// a page built from a source with each one mark is refused, sent labelled with its visible line, or sent under the quotation bound
/// as the row says; an unlabelled page is sent as before; a page that quotes more of a held source than may leave is refused whole;
/// a refusal names the marks and the items and no text. The app's side (`app/src-tauri/src/sink.rs`) refuses, asks once and writes
/// the line first, by its own tests.
#[test]
fn sink_page_export_follows_its_row() {
    let fx = Fx::new();
    let mut got = String::new();
    for m in Mark::ALL {
        let (id, slug) = fx.page_of(m);
        let v = fx.get(&slug, true).unwrap();
        got.push(letter(&v["export"]));
        assert_eq!(v["may_export"] == false, v["export"]["decision"] == "refuse", "{}", v["export"]);
        if v["export"]["decision"] == "refuse" {
            let why = v["export_refused"].as_str().unwrap();
            assert!(why.contains(&handling::source_item(&id)) && why.contains(m.shown()), "the refusal names the mark and its item: {why}");
            assert!(!why.contains("tide comes in") && !why.contains("Held sentence"), "and no text: {why}");
        }
        if m == Mark::Internal {
            assert_eq!(v["export"]["line"], "> Handling: INTERNAL");
            assert!(v["markdown"].as_str().unwrap().starts_with("> Handling: INTERNAL"), "the page the app exports begins with its line");
        }
    }
    assert_eq!(got, want(Sink::PageExport));
    // an unlabelled page (its one source is open and carries no trait): as before labels
    let p = fx.home.join("open.md");
    std::fs::write(&p, "An open note about harbours.").unwrap();
    let open = fx.lib().add_file(&p, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
    fx.page("plain", &[&open], "Generated page. Not authoritative.\n\nNothing labelled.");
    let side_path = fx.lib().pages_dir().join("plain.json");
    let v = fx.get("plain", true).unwrap();
    assert_eq!((v["export"]["decision"].as_str(), v["export"]["line"].is_null(), v["may_export"].as_bool()), (Some("send"), true, Some(true)), "{}", v["export"]);
    // a page whose body quotes 400 characters of a held source is refused whole, whatever its label
    let held_id = fx.page_of(Mark::TextHeld).0;
    let quoting = format!("Generated page. Not authoritative.\n\n{}", take(&fx.text(&held_id), 0, 400));
    fx.page("quoting", &[&held_id], &quoting);
    let v = fx.get("quoting", true).unwrap();
    assert_eq!(v["export"]["decision"], "refuse", "{}", v["export"]);
    assert!(!v["export"]["reason"].as_str().unwrap().contains("Held sentence"));
    // AC-19 at this sink: a page with no stored label is every mark
    let mut side: Value = serde_json::from_str(&std::fs::read_to_string(&side_path).unwrap()).unwrap();
    side.as_object_mut().unwrap().remove("label");
    std::fs::write(&side_path, side.to_string()).unwrap();
    assert_eq!(fx.get("plain", true).unwrap()["export"]["decision"], "refuse");
}

/// AC-15, the clipboard row: the page reply and the source reply say what the window's copy may do, by the label of the page or of
/// the source; a held one's copy is bounded (the window refuses a selection past 300 characters, `app/test/held.test.ts`).
#[test]
fn sink_clipboard_follows_its_row() {
    let fx = Fx::new();
    let (mut pages, mut sources) = (String::new(), String::new());
    for m in Mark::ALL {
        let (id, slug) = fx.page_of(m);
        pages.push(letter(&fx.get(&slug, true).unwrap()["copy"]));
        let s = crate::encyclopedia::library_source(&fx.home, &fx.ws, &json!({"id": id}), true).unwrap();
        sources.push(letter(&s["copy"]));
        assert!(s["text"].is_string() && s["label"].is_string(), "the operator's own view is shown the source, its own `label` kept");
        if s["copy"]["decision"] != "send" {
            assert_eq!(s["may_export"], false);
        }
    }
    assert_eq!(pages, want(Sink::Clipboard));
    assert_eq!(sources, want(Sink::Clipboard));
}

/// AC-15, a client off this machine: no such client exists today (every listener of the daemon is a Unix socket, below), so the row
/// is proved with a fixture sender: the page and source replies asked for a client off this machine refuse every labelled page and
/// source, and send an unlabelled one; a reply whose label was not computed sends nothing.
#[test]
fn sink_off_machine_client_follows_its_row() {
    let fx = Fx::new();
    let mut got = String::new();
    for m in Mark::ALL {
        let (id, slug) = fx.page_of(m);
        let page = fx.get(&slug, false);
        let source = crate::encyclopedia::library_source(&fx.home, &fx.ws, &json!({"id": id}), false);
        assert_eq!(page.is_err(), source.is_err(), "{m:?}");
        got.push(if page.is_err() { 'R' } else { 'S' });
        let e = page.unwrap_err();
        assert!(e.contains("off this machine") && !e.contains("tide comes in") && !e.contains("Held sentence"), "{e}");
        assert!(fx.get(&slug, true).is_ok(), "the operator's own view on this machine is not a sink");
    }
    assert_eq!(got, want(Sink::OffMachineClient));
    assert!(reply_to_client(Some(&Label::none()), false).is_ok());
    assert!(reply_to_client(None, false).is_err(), "a reply whose label is not computed is every mark");
}

/// The fact the row of a client off this machine rests on today: no listener of the daemon can be reached from another machine. The
/// daemon's server and the engine's sockets bind Unix sockets only; the one HTTP listener, `kb serve`, is its own sink (remote serve).
#[test]
fn no_daemon_listener_is_reachable_from_another_machine() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for f in ["delphi-core/src/server.rs", "delphi-engine/src/service.rs", "delphi-engine/src/bridge.rs", "delphi-engine/src/host_rpc.rs"] {
        let src = std::fs::read_to_string(crates.join(f)).unwrap_or_default();
        let prod = &src[..src.find("\n#[cfg(test)]").unwrap_or(src.len())];
        for net in ["TcpListener", "UdpSocket", "tiny_http::", "tokio::net::Tcp", "hyper::server"] {
            assert!(!prod.contains(net), "{f} names `{net}`: a client off this machine may reach it; make `client_on_this_machine` say so");
        }
    }
    let server = std::fs::read_to_string(crates.join("delphi-core/src/server.rs")).unwrap();
    assert!(server.contains("UnixListener"), "the daemon serves Unix sockets");
    let service = std::fs::read_to_string(crates.join("delphi-engine/src/service.rs")).unwrap();
    let f = service.split("fn client_on_this_machine(").nth(1).unwrap();
    assert!(f[..f.find("\n}\n").unwrap()].contains("true"));
    for m in ["async fn pages_get(", "async fn library_source("] {
        let body = service.split(m).nth(1).unwrap();
        assert!(body[..body.find("\n    }\n").unwrap()].contains("client_on_this_machine(&ctx)"), "{m} tells the reply who asks");
    }
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

const NOTE: &str = "---\nstatus: active\n---\n# Notes\n\nThe harbour master keeps the tide tables of our own.\n";

impl Fx {
    /// An index of one source with each mark and one unlabelled note, each row labelled from the records.
    fn index(&self) -> (crate::kb::Kb, Vec<(Mark, String)>) {
        let mut kb = crate::kb::Kb::open_in_memory().unwrap();
        kb.attach_embedder(std::sync::Arc::new(Words)).unwrap();
        kb.set_seed_vectors(None);
        let mut paths = Vec::new();
        for m in Mark::ALL {
            let text = if m == Mark::TextHeld { long_text() } else { format!("# {}\n\nThe tide note, {}.\n", m.as_str(), m.as_str()) };
            let id = self.source(&format!("{}.md", m.as_str()), &text, m);
            let path = format!("library/sources/{id}/source.md");
            kb.upsert_from(&path, "library", "federated_verified", &format!("---\nstatus: active\n---\n{text}"), "2026-11-27T00:00:00Z").unwrap();
            paths.push((m, path));
        }
        kb.upsert_from("context/notes.md", "context", "federated_verified", NOTE, "2026-11-27T00:00:00Z").unwrap();
        crate::label::relabel_rows(&mut kb, &self.home, &self.ws).unwrap();
        kb.embed_pending(&mut |_, _| {}).unwrap();
        (kb, paths)
    }
}

/// AC-4 and AC-7, the seed-export row through `kb::seed_export` (what `delphi kb seed-export` writes): no vector of a chunk of a
/// document with any mark is written, and the count of what was left out is returned; the unlabelled note's vectors are; a row the
/// index wrote before labels existed (no `marks`) is every mark (AC-19).
#[test]
fn sink_seed_export_writes_no_labelled_vector() {
    let fx = Fx::new();
    let (mut kb, paths) = fx.index();
    let (bytes, left_out) = crate::kb::seed_export(&kb, &fx.home, &fx.ws).unwrap();
    let out = crate::embed::vectors::VecCache::parse(&bytes).unwrap();
    let all = crate::embed::vectors::VecCache::parse(&kb.export_vectors().unwrap()).unwrap();
    let only_note = {
        let mut k = crate::kb::Kb::open_in_memory().unwrap();
        k.attach_embedder(std::sync::Arc::new(Words)).unwrap();
        k.set_seed_vectors(None);
        k.upsert_from("context/notes.md", "context", "federated_verified", NOTE, "2026-11-27T00:00:00Z").unwrap();
        k.embed_pending(&mut |_, _| {}).unwrap();
        crate::embed::vectors::VecCache::parse(&k.export_vectors().unwrap()).unwrap()
    };
    assert_eq!(out.len(), only_note.len(), "only the unlabelled note's vectors are written");
    assert!(out.entries().all(|(k, v)| only_note.get(k) == Some(v)));
    assert!(all.len() > out.len() && left_out >= paths.len(), "every labelled document had a chunk left out ({left_out})");
    let mut got = String::new();
    for (m, p) in &paths {
        let lib = fx.lib();
        let labeller = crate::label::Labeller::new(Some(&lib), Some(&fx.ws));
        got.push(if check_label(&index_label(&labeller, &kb, p), Sink::SeedExport).refused() { 'R' } else { 'S' });
        assert!(index_label(&labeller, &kb, p).has(*m), "{p}");
    }
    assert_eq!(got, want(Sink::SeedExport));
    // a row from before labels
    kb.upsert_from("context/old.md", "context", "federated_verified", "---\nstatus: active\n---\n# Old\n\nAn older harbour note.\n", "2026-11-27T00:00:00Z").unwrap();
    kb.embed_pending(&mut |_, _| {}).unwrap();
    let lib = fx.lib();
    let labeller = crate::label::Labeller::new(Some(&lib), Some(&fx.ws));
    if kb.row_label("context/old.md").is_some_and(|(_, l)| !l.is_empty()) {
        assert!(check_label(&index_label(&labeller, &kb, "context/old.md"), Sink::SeedExport).refused(), "a row with no marks is every mark");
        let (_, more) = crate::kb::seed_export(&kb, &fx.home, &fx.ws).unwrap();
        assert!(more > left_out, "its chunks are left out too");
    }
}

/// AC-4 and AC-7, the remote-serve row through `kb serve`'s reader on an address that is not loopback: no labelled document is found
/// by a search, listed or served, and the unlabelled note is; on loopback the results are as they were (only a held source's text is
/// withheld there, as before this slice).
#[test]
fn sink_remote_serve_returns_no_labelled_document() {
    let fx = Fx::new();
    let (kb, paths) = fx.index();
    let reader = |remote: bool| crate::kbserve::Reader { kb: &kb, ws: &fx.ws, operator: false, home: None, remote };
    let found = |remote: bool| -> Vec<String> {
        let r = reader(remote).get("/v1/search?q=tide%20note&limit=20");
        assert_eq!(r.status, 200, "{}", r.body);
        r.body["results"].as_array().unwrap().iter().filter_map(|h| h["path"].as_str().map(String::from)).collect()
    };
    let listed = |remote: bool| -> Vec<String> { reader(remote).get("/v1/documents?limit=200").body.as_array().unwrap().iter().filter_map(|d| d["rel_path"].as_str().map(String::from)).collect() };
    let (remote_found, local_found) = (found(true), found(false));
    let (remote_listed, local_listed) = (listed(true), listed(false));
    let mut got = String::new();
    for (m, p) in &paths {
        let reached = remote_found.contains(p) || remote_listed.contains(p);
        got.push(if reached { 'S' } else { 'R' });
        // the document itself: refused before the index or the disk is read
        assert_eq!(reader(true).get(&format!("/v1/doc?path={p}")).body["error"], "not in the knowledge base", "{p}");
        // loopback: unchanged; only a held or a private source's text is withheld there, as before
        let local = local_found.contains(p);
        assert_eq!(local, *m != Mark::TextHeld && *m != Mark::LibraryPrivate, "{p} on loopback: {local_found:?}");
        assert_eq!(local_listed.contains(p), local || *m == Mark::TextHeld || *m == Mark::LibraryPrivate, "{p} is listed on loopback as before");
    }
    assert_eq!(got, want(Sink::RemoteServe));
    let note = "context/notes.md".to_string();
    assert!(remote_found.contains(&note) && local_found.contains(&note) && remote_listed.contains(&note), "an unlabelled document reaches a remote reader: {remote_found:?}");
}
