//! Tests of the evidence pack (`specs/144` slice C1: AC-11, AC-12, and the search rules of §2.6 items 2-3).

use super::*;
use crate::egress::{Confirm, Sink};
use crate::kb::{Kb, SearchOpts};
use crate::library::claims::{AnchorSpec, ResearchClaim};
use crate::library::{AddOpts, Budgets, Level, Library};
use crate::research_record::{Asker, Begin, Topic};
use serde_json::json;

struct Fx {
    _root: tempfile::TempDir,
    home: PathBuf,
    ws: PathBuf,
}

/// An install home with one workspace under it (`<home>/workspaces/<name>`), as the index and the labeller expect.
fn fx() -> Fx {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().to_path_buf();
    let ws = home.join("workspaces/w");
    std::fs::create_dir_all(ws.join("local")).unwrap();
    Fx { _root: root, home, ws }
}

/// A public source under an open licence, its text kept whole: no mark.
fn open_source(lib: &Library, dir: &Path, name: &str, text: &str) -> String {
    let f = dir.join(format!("{name}.md"));
    std::fs::write(&f, text).unwrap();
    lib.add_file(&f, &AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Full), ..Default::default() }, &Budgets::default()).unwrap().id
}

/// A held source, kept through the library's own capture path (`local_text`): `text_held`. Returns its id and its stored text.
fn held_source(lib: &Library, url: &str, text: &str) -> (String, String) {
    let snap = crate::fact_fetch::test_snapshot(url, "Quoll dens held", &[text]);
    let s = lib.add_fetched(url, &snap, &AddOpts { keep_closed_held: true, ..Default::default() }, &Budgets::default()).unwrap();
    (s.id.clone(), std::fs::read_to_string(lib.source_dir(&s.id).join("source.md")).unwrap())
}

fn held_text() -> String {
    (0..120).map(|i| format!("Quoll dens line {i:03} runs along the creek bank under the fallen logs. ")).collect()
}

/// Sixty characters of `text` from where `at` is: a quote with stored text on both sides.
fn mid_quote(text: &str, at: &str) -> String {
    text[text.find(at).unwrap()..].chars().take(60).collect::<String>().trim().to_string()
}

fn claim(lib: &Library, text: &str, fp: &str, topic: &str, source: &str, quote: &str) -> String {
    let anchors = [AnchorSpec { source_id: source.into(), locator: Some("https://example.org/p".into()), quote: quote.into(), prefix: None, suffix: None, read: None }];
    lib.add_research_claim(&ResearchClaim { text, fingerprint: fp, project: None, topic: Some(topic), anchors: &anchors }).unwrap().0["id"].as_str().unwrap().to_string()
}

/// A topic whose run read `read` (each `(url, source id)`) and kept `claims`, ended `done`.
fn topic(ws: &Path, question: &str, read: &[(&str, &str)]) -> Topic {
    let t = Topic::begin(ws, &Begin { question, asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap();
    for (url, sid) in read {
        t.page("r-1", url, sid, Some("sha256:00")).unwrap();
    }
    t
}

fn finish(t: &Topic, claims: &[String]) {
    let result = json!({"answer": "What the sources say (not validated).", "claims": claims, "budget_used": {"steps": 1, "check_steps": 0}, "search_log": {"version": 1, "private": false, "lines": [], "flow": {}}});
    t.finish("r-1", "done", Some(&result), claims, None, 1).unwrap();
}

fn search(ws: &Path, query: &str, refresh: bool) -> Vec<crate::kb::Hit> {
    let mut kb = Kb::open(&ws.join("local/state.db")).unwrap();
    if refresh {
        kb.refresh(ws, "").unwrap();
    }
    kb.search(query, &SearchOpts { zones: Some(vec![crate::research_index::ZONE]), include_held: true, limit: 20, rerank_weight: 0.0, ..Default::default() }).unwrap()
}

fn pack_hits<'a>(hits: &'a [crate::kb::Hit], t: &Topic) -> Vec<&'a crate::kb::Hit> {
    let p = format!("research/topics/{}/{FILE}", t.id);
    hits.iter().filter(|h| h.rel_path == p).collect()
}

fn set_private(lib: &Library, id: &str) {
    let path = lib.source_dir(id).join("meta.json");
    let mut meta: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    meta["private"] = json!(true);
    std::fs::write(&path, meta.to_string()).unwrap();
}

/// AC-11: a finished run writes a pack with frontmatter, each claim with its quote, the passage around it, its source and the hash of
/// that source's stored text, and an empty support verdict; a claim whose quote is not in its source's stored text is left out; the
/// pack is indexed at `external_untrusted`, labelled, with the mark of the held source the run read; after a source is made private
/// the very next search, before any refresh, returns it no more, and the exit check reads the new mark.
#[test]
fn a_finished_run_writes_a_labelled_indexed_pack_of_its_anchored_claims_and_a_reclassified_source_reclassifies_it() {
    let f = fx();
    let lib = Library::in_home(&f.home);
    let open = open_source(&lib, &f.home, "dens", "Quoll dens are found under fallen logs near permanent water, the survey says.");
    let (held, held_stored) = held_source(&lib, "https://held.example/quolls", &held_text());
    let t = topic(&f.ws, "Where do quolls make their dens?", &[("https://example.org/dens", &open), ("https://held.example/quolls", &held)]);
    let c_open = claim(&lib, "quoll dens are under fallen logs near permanent water", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &t.id, &open, "found under fallen logs near permanent water");
    let held_quote = mid_quote(&held_stored, "line 040");
    let c_held = claim(&lib, "quoll dens run along the creek bank", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", &t.id, &held, held_quote.trim());
    let c_lost = claim(&lib, "quoll dens face north", "cccccccccccccccccccccccccccccccc", &t.id, &open, "found under fallen logs");
    // The lost claim's quote is changed on disk to words its source does not hold: it is no longer anchored in the stored text.
    let cpath = lib.claims_dir().join(format!("{c_lost}.json"));
    let mut cv: Value = serde_json::from_str(&std::fs::read_to_string(&cpath).unwrap()).unwrap();
    cv["anchors"][0]["quote"] = json!("dens always face due north");
    std::fs::write(&cpath, cv.to_string()).unwrap();
    finish(&t, &[c_open.clone(), c_held.clone(), c_lost.clone()]);
    after_run(&f.home, &t);

    let md = std::fs::read_to_string(path_of(&t)).expect("the run wrote its pack");
    assert!(md.starts_with("---\n") && md.contains(&format!("kind: {KIND}\n")) && md.contains(&format!("topic: {}\n", t.id)) && md.contains("source_type: external_untrusted\n"), "{md}");
    let sha = |id: &str| sha256_hex(std::fs::read(lib.source_dir(id).join("source.md")).unwrap().as_slice());
    for (c, s) in [(&c_open, &open), (&c_held, &held)] {
        let section = md.split("\n## ").find(|sec| sec.contains(&format!("`{c}`"))).unwrap_or_else(|| panic!("{c} is not in the pack:\n{md}"));
        assert!(section.contains(&format!("- source: `s-{s}`")) && section.contains(&format!("- text sha256: `{}`", sha(s))) && section.contains("- support:\n"), "{section}");
    }
    assert!(md.contains("«found under fallen logs near permanent water»") && md.contains("> Quoll dens are "), "the quote with the passage around it:\n{md}");
    assert!(!md.contains(&c_lost) && md.contains("left_out: 1\n"), "a claim whose quote is not in the stored text is left out:\n{md}");

    // The label: the strongest mark of every source the run read, computed from the records.
    let l = label(&f.home, &f.ws, &t.id);
    assert!(l.has(Mark::TextHeld) && !l.has(Mark::LibraryPrivate), "{:?}", l.names());
    // Indexed, labelled, at `external_untrusted`, with the mark computed at the search.
    let hits = search(&f.ws, "quoll dens fallen logs permanent water", true);
    let ph = pack_hits(&hits, &t);
    assert!(!ph.is_empty(), "the pack is found: {:?}", hits.iter().map(|h| &h.rel_path).collect::<Vec<_>>());
    assert!(ph.iter().all(|h| h.label == SEARCH_LABEL && h.source_type == TAINT && h.marks.has(Mark::TextHeld) && h.status == "noted"), "{:?}", ph.iter().map(|h| (h.label, &h.source_type, h.marks.names())).collect::<Vec<_>>());

    // Reclassified: the open source is made private. The next search, before any refresh, returns the pack no more, and the exit
    // check reads the new mark.
    set_private(&lib, &open);
    assert!(label(&f.home, &f.ws, &t.id).has(Mark::LibraryPrivate));
    assert!(pack_hits(&search(&f.ws, "quoll dens fallen logs permanent water", false), &t).is_empty(), "withheld at the search itself");
    let err = crate::egress::check_path(&f.home, Some(&f.ws), &path_of(&t), Sink::Bundle, Confirm::Nobody).unwrap_err();
    assert!(err.contains("private"), "{err}");
    // And the next refresh takes it out of the index.
    assert!(pack_hits(&search(&f.ws, "quoll dens fallen logs permanent water", true), &t).is_empty());
    let mut listed = Vec::new();
    crate::research_index::files(&f.ws, &mut listed);
    assert!(!listed.iter().any(|s| s.rel_path.ends_with(FILE)));
}

/// §2.6 item 3: a pack from a run that read a private source, quoted or not, is in no agent's search and no index listing; one whose
/// record cannot be read is every mark, so private.
#[test]
fn no_search_returns_a_pack_from_a_run_that_read_a_private_source_or_one_that_cannot_be_traced() {
    let f = fx();
    let lib = Library::in_home(&f.home);
    let open = open_source(&lib, &f.home, "bilby", "Bilbies dig spiral burrows up to two metres deep in sandy soil.");
    let secret = open_source(&lib, &f.home, "notes", "The bilby enclosure gate code is changed every month.");
    set_private(&lib, &secret);
    let t = topic(&f.ws, "How deep do bilby burrows go?", &[("https://example.org/bilby", &open), ("file:notes", &secret)]);
    let c = claim(&lib, "bilby burrows go two metres deep", "dddddddddddddddddddddddddddddddd", &t.id, &open, "spiral burrows up to two metres deep");
    finish(&t, std::slice::from_ref(&c));
    assert_eq!(write(&lib, &t).unwrap(), Written::Pack { claims: 1, left_out: 0 });
    assert!(!std::fs::read_to_string(path_of(&t)).unwrap().contains("gate code"), "the private source is not quoted");
    let l = label(&f.home, &f.ws, &t.id);
    assert!(l.has(Mark::LibraryPrivate) && withheld(&l), "read, not quoted, still marks it: {:?}", l.names());
    assert!(pack_hits(&search(&f.ws, "bilby spiral burrows two metres deep", true), &t).is_empty());
    let mut listed = Vec::new();
    crate::research_index::files(&f.ws, &mut listed);
    assert!(!listed.iter().any(|s| s.rel_path.ends_with(FILE)), "not listed either");
    // Untraceable: a record that cannot be read is every mark, so withheld, at an exit and at a search.
    std::fs::write(t.record_path(), "not json").unwrap();
    let l = label(&f.home, &f.ws, &t.id);
    assert!(Mark::ALL.iter().all(|m| l.has(*m)) && withheld(&l));
    assert!(search_label(None, &t.id).is_none(), "an index whose workspace cannot be told cannot trace a pack");
}

/// AC-12: a pack is listed in `held_text::SENDERS`, and a bundle, an export, a Xenia publish or a push that carries a pack quoting a
/// held source past `body_may_leave`, or a pack of a run that read a private source, is refused; a copy of the pack made elsewhere,
/// with its frontmatter or without it, is refused the same; a pack of open sources only passes.
#[test]
fn a_bundle_export_or_push_that_carries_a_pack_of_a_held_or_private_source_is_refused() {
    assert!(crate::held_text::SENDERS.iter().any(|s| s.name.contains("evidence pack")), "the pack is on the list of senders");
    let f = fx();
    let lib = Library::in_home(&f.home);
    let open = open_source(&lib, &f.home, "echidna", "Echidnas lay a single leathery egg into a pouch that forms during breeding.");
    let (held, held_stored) = held_source(&lib, "https://held.example/quolls", &held_text());
    let sinks = [Sink::Bundle, Sink::PageExport, Sink::Xenia, Sink::Repository { internal_remote: false }, Sink::Document];
    let refused_everywhere = |path: &Path, ws: Option<&Path>| {
        for s in sinks {
            assert!(crate::egress::check_path(&f.home, ws, path, s, Confirm::Nobody).is_err(), "{} sent {}", s.name(), path.display());
        }
    };
    // a pack of open sources only: sent
    let clean = topic(&f.ws, "How do echidnas lay eggs?", &[("https://example.org/echidna", &open)]);
    let c = claim(&lib, "echidnas lay one egg into a pouch", "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", &clean.id, &open, "a single leathery egg into a pouch");
    finish(&clean, std::slice::from_ref(&c));
    write(&lib, &clean).unwrap();
    for s in sinks {
        assert!(crate::egress::check_path(&f.home, Some(&f.ws), &path_of(&clean), s, Confirm::Nobody).is_ok(), "{}", s.name());
    }
    // a pack that quotes a held source: its passage carries more than 300 characters of the held text
    let quoting = topic(&f.ws, "Where do quolls den along the creek?", &[("https://held.example/quolls", &held)]);
    // One passage (460 characters, cut by the quote's marks) is under the bound; three far apart are past the cap of a tenth of the source.
    let one = claim(&lib, "quoll dens run along the creek", "ffffffffffffffffffffffffffffffff", &quoting.id, &held, &mid_quote(&held_stored, "line 060"));
    finish(&quoting, std::slice::from_ref(&one));
    write(&lib, &quoting).unwrap();
    assert!(crate::egress::check_path(&f.home, Some(&f.ws), &path_of(&quoting), Sink::Bundle, Confirm::Nobody).is_ok(), "one quotation within the bound may leave");
    let two = claim(&lib, "quoll dens are under the fallen logs", "fafafafafafafafafafafafafafafafa", &quoting.id, &held, &mid_quote(&held_stored, "line 020"));
    let three = claim(&lib, "quoll dens follow the creek bank", "fbfbfbfbfbfbfbfbfbfbfbfbfbfbfbfb", &quoting.id, &held, &mid_quote(&held_stored, "line 100"));
    finish(&quoting, &[one, two, three]);
    write(&lib, &quoting).unwrap();
    refused_everywhere(&path_of(&quoting), Some(&f.ws));
    let err = crate::egress::check_path(&f.home, Some(&f.ws), &path_of(&quoting), Sink::Bundle, Confirm::Nobody).unwrap_err();
    assert!(err.contains(&format!("s-{held}")), "the bound names the held source: {err}");
    // a pack of a run that read a private source, which it does not quote
    let secret = open_source(&lib, &f.home, "keeper", "The echidna keeper's phone number is on the office wall.");
    set_private(&lib, &secret);
    let private = topic(&f.ws, "What do echidnas lay into?", &[("https://example.org/echidna", &open), ("file:keeper", &secret)]);
    let c = claim(&lib, "echidnas lay into a pouch", "abababababababababababababababab", &private.id, &open, "into a pouch that forms during breeding");
    finish(&private, std::slice::from_ref(&c));
    write(&lib, &private).unwrap();
    refused_everywhere(&path_of(&private), Some(&f.ws));
    // copies made outside the workspace: with its frontmatter (it names its topic), and the body alone (a run of the pack)
    let out = tempfile::tempdir().unwrap();
    let copy = out.path().join("notes.md");
    let md = std::fs::read_to_string(path_of(&private)).unwrap();
    std::fs::write(&copy, &md).unwrap();
    refused_everywhere(&copy, Some(&f.ws));
    refused_everywhere(&copy, None);
    let body = out.path().join("body.md");
    std::fs::write(&body, crate::label::split_page(&md).1).unwrap();
    refused_everywhere(&body, Some(&f.ws));
}

/// §2.5 item 5 and AC-11: `research pack --all` writes packs for runs that ended before packs existed, from their records and the stored
/// text; a topic with no anchored claim keeps none.
#[test]
fn packs_are_written_for_earlier_runs_from_their_records() {
    let f = fx();
    let lib = Library::in_home(&f.home);
    let open = open_source(&lib, &f.home, "wombat", "Wombats produce cube-shaped droppings to mark their territory.");
    let earlier = topic(&f.ws, "Why are wombat droppings cubes?", &[("https://example.org/wombat", &open)]);
    let c = claim(&lib, "wombat droppings are cube-shaped", "acacacacacacacacacacacacacacacac", &earlier.id, &open, "cube-shaped droppings");
    finish(&earlier, std::slice::from_ref(&c));
    let empty = topic(&f.ws, "Do wombats swim?", &[("https://example.org/wombat", &open)]);
    finish(&empty, &[]);
    assert!(!path_of(&earlier).exists(), "a run of before has no pack");
    let done = write_all(&f.home, &f.ws);
    assert_eq!(done.len(), 2);
    assert!(done.iter().any(|(id, r)| *id == earlier.id && *r == Ok(Written::Pack { claims: 1, left_out: 0 })), "{done:?}");
    assert!(done.iter().any(|(id, r)| *id == empty.id && *r == Ok(Written::Empty { left_out: 0 })), "{done:?}");
    assert!(std::fs::read_to_string(path_of(&earlier)).unwrap().contains(&c) && !path_of(&empty).exists());
    // Written twice, it is the same pack (but for its date), and it is indexed.
    let hits = search(&f.ws, "wombat cube-shaped droppings", true);
    assert!(!pack_hits(&hits, &earlier).is_empty());
}

// ---- slice B (`specs/144` §2.6 items 2-3; AC-9, AC-10): what agents' searches return of a research run --------------------------

fn finish_with(t: &Topic, claims: &[String], answer: &str) {
    let result = json!({"answer": answer, "claims": claims, "budget_used": {"steps": 1, "check_steps": 0}, "search_log": {"version": 1, "private": false, "lines": [], "flow": {}}});
    t.finish("r-1", "done", Some(&result), claims, None, 1).unwrap();
}

fn agent_search(ws: &Path, query: &str, zones: Vec<&'static str>, held: bool) -> Vec<crate::kb::Hit> {
    let kb = Kb::open(&ws.join("local/state.db")).unwrap();
    kb.search(query, &SearchOpts { role: Some("scout".into()), zones: Some(zones), include_held: held, limit: 30, rerank_weight: 0.0, ..Default::default() }).unwrap()
}

fn answer_of(t: &Topic) -> String {
    format!("research/topics/{}/answer.md", t.id)
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// AC-10: an answer and a pack come back labelled (`research summary · not evidence`, `evidence pack · not validated`); an answer from
/// a run that read held text and only paraphrases it is `external_untrusted`, marked `text_held`, returned only to a search that admits
/// held text, and a span that starts with it is tainted. Under the ratified table a builder's start passages do not carry it; under
/// 145's table (the switch, `retrieval::held_for`) they do, at `L0`.
#[test]
fn answers_and_packs_are_labelled_and_an_answer_of_a_run_that_read_held_text_is_untrusted_and_taints_its_span() {
    use crate::research_index::ANSWER_LABEL;
    use delphi_core::provenance::TaintTable;
    use delphi_core::provenance::Tier;
    let f = fx();
    let lib = Library::in_home(&f.home);
    let open = open_source(&lib, &f.home, "numbat", "Numbats eat termites almost exclusively, about twenty thousand a day.");
    let (held, _) = held_source(&lib, "https://held.example/quolls", &held_text());
    let a = topic(&f.ws, "What do numbats eat?", &[("https://example.org/numbat", &open)]);
    let c = claim(&lib, "numbats eat termites", "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", &a.id, &open, "eat termites almost exclusively");
    finish_with(&a, std::slice::from_ref(&c), "Numbats feed on termites, by the survey's account (not validated).");
    after_run(&f.home, &a);
    // Read held text, quoted nothing, kept no claim: the answer only paraphrases it.
    let h = topic(&f.ws, "Where do quolls make dens?", &[("https://held.example/quolls", &held)]);
    finish_with(&h, &[], "Wallaroo keepers report dens by creek banks beneath logs (not validated).");
    Kb::open(&f.ws.join("local/state.db")).unwrap().refresh(&f.ws, "").unwrap();

    // The open run's answer: in a default search, labelled, at the research tier, no mark.
    let hits = agent_search(&f.ws, "numbats feed termites survey", vec![crate::research_index::ZONE], false);
    let ans = hits.iter().find(|x| x.rel_path == answer_of(&a)).unwrap_or_else(|| panic!("{:?}", hits.iter().map(|x| &x.rel_path).collect::<Vec<_>>()));
    assert_eq!((ans.label, ans.source_type.as_str(), ans.status), (ANSWER_LABEL, crate::research::TIER, "noted"));
    assert!(ans.marks.names().is_empty(), "{:?}", ans.marks.names());
    // Its pack: labelled, untrusted, with held text admitted.
    let hits = agent_search(&f.ws, "numbats eat termites almost exclusively", vec![crate::research_index::ZONE], true);
    assert!(pack_hits(&hits, &a).iter().all(|x| x.label == SEARCH_LABEL && x.source_type == TAINT) && !pack_hits(&hits, &a).is_empty());

    // The held run's answer: not in a search that admits no held text; with it, untrusted, labelled, marked.
    let q = "wallaroo keepers dens creek banks";
    assert!(!agent_search(&f.ws, q, vec![crate::research_index::ZONE], false).iter().any(|x| x.rel_path.starts_with(&format!("research/topics/{}/", h.id))), "a paraphrase of held text is held text's");
    let hits = agent_search(&f.ws, q, vec![crate::research_index::ZONE], true);
    let ans = hits.iter().find(|x| x.rel_path == answer_of(&h)).expect("returned with held text");
    assert_eq!((ans.label, ans.source_type.as_str()), (ANSWER_LABEL, "external_untrusted"));
    assert!(ans.marks.has(Mark::TextHeld));
    assert!(hits.iter().filter(|x| x.rel_path.starts_with(&format!("research/topics/{}/", h.id))).all(|x| x.source_type == "external_untrusted"), "its record too");

    // A span that starts with it is tainted. A builder under the ratified table is not given it; under 145's table it is.
    let db = f.ws.join("local/state.db");
    let (w, x) = (strs(&["@worktree/**"]), strs(&["shell-worktree-scoped", "git-worktree"]));
    let objective = "Report what wallaroo keepers say about dens by creek banks";
    let off = crate::retrieval::held_for(TaintTable::Ratified, "builder", &w, &x);
    assert!(!off);
    let start = crate::retrieval::span_start(&f.home, &db, "m-b1", objective, "builder", crate::retrieval::StartBudget { held: off, ..Default::default() }, false);
    assert!(start.as_ref().is_none_or(|s| !s.text.contains(&answer_of(&h)) && s.tier > Tier::L1), "{:?}", start.map(|s| s.text));
    let on = crate::retrieval::held_for(TaintTable::WhatLeaves, "builder", &w, &x);
    assert!(on);
    let start = crate::retrieval::span_start(&f.home, &db, "m-b1", objective, "builder", crate::retrieval::StartBudget { held: on, ..Default::default() }, false).expect("passages");
    assert!(start.text.contains(&format!("{}#L", answer_of(&h))) && start.text.contains(ANSWER_LABEL), "{}", start.text);
    assert_eq!(start.tier, Tier::L0, "the span is tainted as if it had read the held text");
}

/// AC-9, AC-10 (§2.6 item 3): no agent search returns a library source made private, nor the answer, record or pack of a run that read
/// it, quoted or not, from the very next search, before any refresh; a run whose record cannot be read is untraceable, so withheld; an
/// index whose workspace cannot be told returns nothing of any run; topic pages are not indexed at all.
#[test]
fn no_search_returns_a_private_source_or_an_answer_record_or_pack_of_a_run_that_read_one_or_cannot_be_traced() {
    let f = fx();
    let lib = Library::in_home(&f.home);
    let open = open_source(&lib, &f.home, "echidna", "Echidnas lay a single leathery egg into a pouch each breeding season.");
    let other = open_source(&lib, &f.home, "platypus", "Platypus mothers nurse their young without nipples, from patches of skin.");
    let t = topic(&f.ws, "How do echidnas breed?", &[("https://example.org/echidna", &open), ("https://example.org/platypus", &other)]);
    let c = claim(&lib, "echidnas lay one egg into a pouch", "f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0", &t.id, &open, "a single leathery egg into a pouch");
    finish_with(&t, std::slice::from_ref(&c), "Echidnas lay one leathery egg into a pouch (not validated).");
    after_run(&f.home, &t);
    let u = topic(&f.ws, "How do bettongs forage?", &[]);
    finish_with(&u, &[], "Bettongs forage for truffles at night (not validated).");
    // A page file under the library's pages: never a document of the index.
    std::fs::create_dir_all(lib.root().join("pages")).unwrap();
    std::fs::write(lib.root().join("pages/echidna-0000.md"), "# Echidna\n\nEchidnas lay a single leathery egg.\n").unwrap();
    Kb::open(&f.ws.join("local/state.db")).unwrap().refresh(&f.ws, "").unwrap();
    assert!(!crate::kb::sources(&f.ws).iter().any(|s| s.rel_path.contains("pages/")), "no topic page is indexed");

    let zones = || vec![crate::research_index::ZONE, "library"];
    let of_run = |hits: &[crate::kb::Hit], t: &Topic| hits.iter().filter(|x| x.rel_path.starts_with(&format!("research/topics/{}/", t.id))).count();
    let q = "echidnas leathery egg pouch";
    let hits = agent_search(&f.ws, q, zones(), true);
    assert!(of_run(&hits, &t) >= 2 && hits.iter().any(|x| x.rel_path == format!("library/sources/{open}/source.md")), "{:?}", hits.iter().map(|x| &x.rel_path).collect::<Vec<_>>());

    // The source the run read but does not quote is made private: the next search, before any refresh, returns nothing of the run.
    set_private(&lib, &other);
    let hits = agent_search(&f.ws, q, zones(), true);
    assert_eq!(of_run(&hits, &t), 0, "{:?}", hits.iter().map(|x| &x.rel_path).collect::<Vec<_>>());
    assert!(hits.iter().any(|x| x.rel_path == format!("library/sources/{open}/source.md")), "the open source itself is still returned");
    let start = crate::retrieval::span_start(&f.home, &f.ws.join("local/state.db"), "m-b2", "Explain how echidnas lay a leathery egg into a pouch", "scout", crate::retrieval::StartBudget { held: true, ..Default::default() }, false);
    assert!(start.as_ref().is_none_or(|s| !s.text.contains(&format!("research/topics/{}/", t.id))), "nor do span start passages");
    // The quoted source made private: it is not returned itself either.
    set_private(&lib, &open);
    let hits = agent_search(&f.ws, q, zones(), true);
    assert!(!hits.iter().any(|x| x.rel_path.contains(&format!("library/sources/{open}/"))), "a source made private since the sync is out at the search");

    // Untraceable: a record that cannot be read withholds the run's answer.
    assert!(agent_search(&f.ws, "bettongs forage truffles night", zones(), false).iter().any(|x| x.rel_path == answer_of(&u)));
    std::fs::write(u.record_path(), "not json").unwrap();
    assert!(!agent_search(&f.ws, "bettongs forage truffles night", zones(), false).iter().any(|x| x.rel_path == answer_of(&u)));
    // An index whose workspace cannot be told: nothing of a run.
    let elsewhere = f.home.join("elsewhere.db");
    let mut kb = Kb::open(&elsewhere).unwrap();
    kb.upsert_from(&answer_of(&a_topic_id_holder(&f.ws)), crate::research_index::ZONE, crate::research::TIER, "# Answer\n\nKoalas sleep twenty hours a day.\n", "t").unwrap();
    assert!(kb.search("koalas sleep twenty hours", &SearchOpts { role: Some("scout".into()), limit: 10, ..Default::default() }).unwrap().is_empty());
}

/// A topic of the workspace for a path only (no run).
fn a_topic_id_holder(ws: &Path) -> Topic {
    Topic::at(ws, &crate::research_record::topic_id("Do koalas sleep all day?")).unwrap()
}

/// The path and frontmatter readers name only a topic id this module makes.
#[test]
fn a_pack_is_known_by_its_path_and_its_own_frontmatter_names_its_topic() {
    assert_eq!(topic_of_path("research/topics/quolls-0123abcd/evidence-pack.md"), Some("quolls-0123abcd"));
    assert_eq!(topic_of_path("research/topics/quolls-0123abcd/answer.md"), None);
    assert_eq!(topic_of_path("research/topics/../evidence-pack.md"), None);
    assert_eq!(named_in("---\nkind: evidence_pack\ntopic: quolls-0123abcd\n---\n# x\n").as_deref(), Some("quolls-0123abcd"));
    assert_eq!(named_in("---\nkind: page\ntopic: quolls-0123abcd\n---\n"), None);
    assert_eq!(named_in("---\nkind: evidence_pack\ntopic: ../../etc\n---\n"), None);
}
