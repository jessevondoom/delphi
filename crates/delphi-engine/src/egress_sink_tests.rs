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
        std::fs::write(dir.join(format!("{slug}.json")), json!({"kind": "topic_page", "material": material}).to_string()).unwrap();
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
