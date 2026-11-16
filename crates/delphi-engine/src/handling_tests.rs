use super::*;
use delphi_adapter::config::HandlingPath;

fn cfg() -> HandlingConfig {
    HandlingConfig { org_domains: vec!["example.com".into()], list_addresses: vec!["all@example.com".into()], ..HandlingConfig::default() }
}

fn set(ts: &[Trait]) -> Set {
    ts.iter().copied().collect()
}

fn ws() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let (ws, home) = (t.path().join("ws"), t.path().join("home"));
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    (t, ws, home)
}

/// AC-1, and the rest of the defaults of §3 item 1.
#[test]
fn the_default_rules_label_mail_messages_and_meetings_as_the_spec_says() {
    let c = cfg();
    let email = |from: &str, to: &[&str]| Arrival { kind: "email".into(), addresses: vec![from.into()], recipients: Some(to.iter().map(|s| s.to_string()).collect()), ..Default::default() };
    // a one-to-one email between two colleagues: private and internal
    assert_eq!(assign(&c, &email("ann@example.com", &["bo@example.com"])).traits, set(&[Trait::Personal, Trait::Internal]));
    // to someone outside: private only
    assert_eq!(assign(&c, &email("ann@example.com", &["x@other.org"])).traits, set(&[Trait::Personal]));
    // to a list address, or to more than ten people: internal only
    assert_eq!(assign(&c, &email("ann@example.com", &["all@example.com"])).traits, set(&[Trait::Internal]));
    let many: Vec<String> = (0..11).map(|i| format!("p{i}@other.org")).collect();
    assert_eq!(assign(&c, &email("ann@example.com", &many.iter().map(String::as_str).collect::<Vec<_>>())).traits, set(&[Trait::Internal]));
    // meetings
    let meeting = |n: usize, dom: &str| Arrival { kind: "meeting".into(), addresses: (0..n).map(|i| format!("p{i}@{dom}")).collect(), participants: Some(n), ..Default::default() };
    assert_eq!(assign(&c, &meeting(2, "other.org")).traits, set(&[Trait::Personal]));
    assert!(assign(&c, &meeting(5, "other.org")).traits.is_empty(), "a five-person meeting is not private");
    assert_eq!(assign(&c, &meeting(5, "example.com")).traits, set(&[Trait::Internal]));
    assert!(assign(&c, &Arrival::of("meeting", "")).traits.is_empty(), "participants unknown: unlabelled");
    // messages and documents
    assert_eq!(assign(&c, &Arrival { kind: "dm".into(), org_workspace: Some(true), ..Default::default() }).traits, set(&[Trait::Personal, Trait::Internal]));
    assert_eq!(assign(&c, &Arrival { kind: "slack".into(), direct: Some(false), ..Default::default() }).traits, set(&[Trait::Internal]));
    for k in ["wiki", "private-doc"] {
        assert_eq!(assign(&c, &Arrival::of(k, "x")).traits, set(&[Trait::Internal]), "{k}");
    }
    for k in ["web", "file", "paper", "import"] {
        let a = assign(&c, &Arrival::of(k, "x"));
        assert!(a.traits.is_empty() && !a.configured, "{k}: no rule, unlabelled");
    }
    // no default configures anything: only a row of the operator's does, and the most specific row wins
    assert!(!assign(&c, &Arrival::of("wiki", "x")).configured);
    let mut rows = cfg();
    rows.path = vec![
        HandlingPath { kind: "wiki".into(), under: None, traits: vec!["internal".into()] },
        HandlingPath { kind: "wiki".into(), under: Some("https://wiki.example.com/HR/".into()), traits: vec!["confidential".into()] },
        HandlingPath { kind: "web".into(), under: Some("https://intranet.example.com/".into()), traits: vec![] },
    ];
    let hr = assign(&rows, &Arrival::of("wiki", "https://wiki.example.com/hr/pay"));
    assert_eq!((hr.traits, hr.configured), (set(&[Trait::Confidential, Trait::Internal]), true), "confidential brings internal");
    let intranet = assign(&rows, &Arrival::of("web", "https://intranet.example.com/a"));
    assert_eq!((intranet.traits.is_empty(), intranet.configured), (true, true), "a row with no traits configures the path");
}

/// The words a marking is, and the words that are not one.
#[test]
fn a_marking_is_a_header_footer_or_frontmatter_line_and_not_a_sentence() {
    assert_eq!(marking("CONFIDENTIAL\n\n# Plan\n\nBody."), set(&[Trait::Confidential, Trait::Internal]));
    assert_eq!(marking("# Plan\n\nInternal use only\n\nBody."), set(&[Trait::Internal]));
    assert_eq!(marking("---\nclassification: internal\n---\n# Plan\n"), set(&[Trait::Internal]));
    assert_eq!(marking("---\nhandling: [confidential]\n---\n# Plan\n"), Set::new(), "the daemon's own view is not a marking");
    assert_eq!(marking("# Plan\n\nWe discussed whether the numbers in this report should be treated as confidential by the partners before release.\n"), Set::new());
    assert_eq!(marking("Not confidential\n# Plan\n"), Set::new());
    let long: String = std::iter::repeat_n("Body line with several words in it here.\n", 30).collect();
    assert_eq!(marking(&format!("# Plan\n{long}Confidential\n")), set(&[Trait::Confidential, Trait::Internal]), "a footer");
}

/// AC-2: a marking sets a trait only on a configured path; elsewhere it is noted (`says`) and sets nothing.
#[test]
fn a_marking_counts_only_on_a_configured_path() {
    let (_t, ws, home) = ws();
    let h = Handling::of(&ws, &home);
    let text = "CONFIDENTIAL\n\n# Pay bands\n";
    let mut rows = cfg();
    rows.path = vec![HandlingPath { kind: "private-doc".into(), under: Some("drive://hr/".into()), traits: vec![] }];
    let got = h.arrive(&rows, "local/summaries/pay.md", Some("h1"), &Arrival::of("private-doc", "drive://hr/pay"), Some(text)).unwrap();
    assert_eq!(got, set(&[Trait::Confidential, Trait::Internal]));
    assert_eq!(h.record("local/summaries/pay.md").unwrap()["traits"].as_array().unwrap().iter().filter(|t| t["by"] == "marking").count(), 2);
    let web = h.arrive(&rows, "local/summaries/web.md", Some("h2"), &Arrival::of("web", "https://news.example.org/a"), Some(text)).unwrap();
    assert!(web.is_empty(), "the same text fetched from the web carries neither");
    assert_eq!(h.says("local/summaries/web.md"), set(&[Trait::Confidential, Trait::Internal]), "and shows the note");
    // §7: only a configured path or the operator gives the weight
    assert!(!h.has_weight("local/summaries/web.md"));
    let wiki = h.arrive(&cfg(), "local/summaries/wiki.md", None, &Arrival::of("wiki", "x"), None).unwrap();
    assert_eq!(wiki, set(&[Trait::Internal]));
    assert!(!h.has_weight("local/summaries/wiki.md"), "a built-in default labels and does not weigh");
    let mut wiki_row = cfg();
    wiki_row.path = vec![HandlingPath { kind: "wiki".into(), under: None, traits: vec!["internal".into()] }];
    h.arrive(&wiki_row, "local/summaries/wiki2.md", None, &Arrival::of("wiki", "x"), None).unwrap();
    assert!(h.has_weight("local/summaries/wiki2.md"));
    h.add("local/summaries/web.md", None, &set(&[Trait::Internal]), &By::Agent("scout".into()), &[]).unwrap();
    h.add("library/sources/s1", None, &set(&[Trait::Internal]), &By::Host, &[]).unwrap();
    assert!(!h.has_weight("local/summaries/web.md") && !h.has_weight("library/sources/s1"), "an agent's or a host's internal has no weight");
    h.operator_set("local/summaries/web.md", Trait::Internal).unwrap();
    assert!(h.has_weight("local/summaries/web.md"), "the operator's does");
}

/// AC-3: adding only; the operator's clear removes, is remembered against the rules that set the trait, and is logged; confidential
/// implies internal.
#[test]
fn agents_only_add_and_only_the_operators_clear_removes() {
    let (_t, ws, home) = ws();
    let h = Handling::of(&ws, &home);
    let item = "local/summaries/a.md";
    h.arrive(&cfg(), item, Some("v1"), &Arrival::of("wiki", "x"), None).unwrap();
    // an agent adds: it can only add, and re-adding or adding nothing changes nothing
    assert_eq!(h.add(item, None, &set(&[Trait::Personal]), &By::Agent("scout".into()), &[]).unwrap(), set(&[Trait::Personal]));
    assert!(h.add(item, None, &Set::new(), &By::Agent("scout".into()), &[]).unwrap().is_empty());
    assert_eq!(h.set(item), set(&[Trait::Personal, Trait::Internal]));
    // confidential brings internal; internal cannot be cleared under it
    h.add(item, None, &set(&[Trait::Confidential]), &By::Agent("scout".into()), &[]).unwrap();
    assert!(h.operator_clear(item, Trait::Internal).unwrap_err().contains("Clear `confidential` first"));
    assert!(h.operator_clear(item, Trait::Confidential).unwrap());
    assert!(h.operator_clear(item, Trait::Internal).unwrap());
    assert_eq!(h.set(item), set(&[Trait::Personal]));
    let log = std::fs::read_to_string(dir(&ws).join("log.jsonl")).unwrap();
    assert_eq!(log.matches(CLEARED_EVENT).count(), 2, "every clear is logged, and the refused one changed nothing: {log}");
    // the path rule does not put back what the operator cleared while the content is the same; new content is new
    h.arrive(&cfg(), item, Some("v1"), &Arrival::of("wiki", "x"), None).unwrap();
    assert!(!h.set(item).contains(&Trait::Internal));
    h.arrive(&cfg(), item, Some("v2"), &Arrival::of("wiki", "x"), None).unwrap();
    assert!(h.set(item).contains(&Trait::Internal), "a new version is assigned again");
    // the operator's set lifts the clear
    h.operator_clear(item, Trait::Personal).unwrap();
    h.operator_set(item, Trait::Personal).unwrap();
    assert!(h.set(item).contains(&Trait::Personal));
    // the display name is the operator's word
    assert_eq!(shown(&h.set(item)), "private, internal");
    assert_eq!(Trait::parse("private"), Some(Trait::Personal));
}

/// An item id is an identifier; a hostile one is refused.
#[test]
fn item_ids_are_checked_and_index_paths_map_to_items() {
    for bad in ["", "/etc/passwd", "../x", "a//b", "a/./b", "a\\b", "a\nb"] {
        assert!(!item_ok(bad), "{bad:?}");
    }
    let (_t, ws, home) = ws();
    assert!(Handling::of(&ws, &home).add("../escape", None, &set(&[Trait::Internal]), &By::Operator, &[]).is_err());
    assert_eq!(item_of("local", "summaries/a.md").as_deref(), Some("local/summaries/a.md"));
    assert_eq!(item_of("findings", "missions/m1/findings/q.md").as_deref(), Some("local/sessions/m1/findings/q.md"));
    assert_eq!(item_of("provisional", "provisional/builder/n.md").as_deref(), Some("local/memory/provisional/builder/n.md"));
    assert_eq!(item_of("library", "library/sources/abc/source.md").as_deref(), Some("library/sources/abc"));
    assert_eq!(item_of("x", "@library/claim/c1").as_deref(), Some("library/claims/c1"));
    assert_eq!(item_of("core", "about/x.md"), None);
    assert_eq!(item_in_ws(&ws, &ws.join("local/missions/m1/findings/q.md")).as_deref(), Some("local/sessions/m1/findings/q.md"));
}

/// AC-4: a record takes the traits of what it used, as `derived` with their ids; what the span was given and did not use is a
/// `may_rest_on` note and no trait. A view that dropped a trait is put back.
#[test]
fn a_record_takes_what_it_used_and_notes_what_it_was_given() {
    let (_t, ws, home) = ws();
    let h = Handling::of(&ws, &home);
    let (mail, other) = ("local/summaries/mail.md", "local/summaries/other.md");
    h.arrive(&cfg(), mail, None, &Arrival::of("email", "x"), None).unwrap();
    h.arrive(&cfg(), other, None, &Arrival::of("wiki", "x"), None).unwrap();
    let key = span_key("m1", "writer");
    assert_eq!(note_given(&h, &ws, &key, &[mail.into(), other.into(), "local/summaries/plain.md".into()]), 2, "only labelled items are noted");
    assert_eq!(given(&ws, &key), [mail, other]);
    // the summary cites the mail page and not the other
    let file = ws.join("local/summaries/sum.md");
    let text = format!("---\nkind: summary\n---\n\nAs {mail} says, the launch moves.\n");
    let shown = label_file(&ws, Some(&home), &file, &text, Some(&key), &[]).unwrap();
    assert!(shown.contains("handling: [personal]") && shown.contains("handling_may_rest_on: [\"local/summaries/other.md\"]"), "{shown}");
    let rec = h.record("local/summaries/sum.md").unwrap();
    assert_eq!(rec["traits"][0]["by"], "derived");
    assert_eq!(rec["traits"][0]["from"], json!([mail]));
    assert_eq!(h.set("local/summaries/sum.md"), set(&[Trait::Personal]), "what was given and not used adds no trait");
    // a summary that cites nothing: the note only
    let file2 = ws.join("local/summaries/sum2.md");
    label_file(&ws, Some(&home), &file2, "---\nkind: summary\n---\n\nA paraphrase.\n", Some(&key), &[]).unwrap();
    assert!(h.set("local/summaries/sum2.md").is_empty());
    assert_eq!(h.record("local/summaries/sum2.md").unwrap()["may_rest_on"].as_array().unwrap().len(), 2);
    // an edit that drops the view is put back from the record; the view is never read as authority
    let edited = "---\nkind: summary\nhandling: []\n---\n\nAs before.\n";
    assert_eq!(view_of(edited), Some(Set::new()));
    assert!(view_for(&ws, &file, edited).contains("handling: [personal]"));
    // the writer's arguments cannot lower anything: deriving again with nothing used removes nothing
    h.derive("local/summaries/sum.md", None, &[], &[]).unwrap();
    assert_eq!(h.set("local/summaries/sum.md"), set(&[Trait::Personal]));
}

/// AC-4 through the library: a claim anchored to a source a host declared `person` carries `personal` as `derived` with the
/// source's id, and its JSON shows the view.
#[test]
fn a_claim_takes_the_traits_of_the_sources_it_is_anchored_to() {
    let (_t, _ws, home) = ws();
    let lib = crate::library::Library::in_home(&home);
    let sid = lib.add_private_pointer("person", "talk with the lab manager", "conversation", "2026-11-15", None).unwrap();
    let h = Handling::library_only(&home);
    h.add(&source_item(&sid), None, &host_kind("person"), &By::Host, &[]).unwrap();
    let anchors = [crate::library::claims::AnchorSpec { source_id: sid.clone(), locator: Some("talk".into()), quote: "the tank is cleaned on Mondays".into(), prefix: None, suffix: None }];
    let fp = "0123456789abcdef0123";
    let c = lib.add_host_claim(&crate::library::claims::ResearchClaim { text: "The tank is cleaned on Mondays.", fingerprint: fp, project: None, topic: None, anchors: &anchors }, "host-reported").unwrap();
    let id = c["id"].as_str().unwrap();
    let item = claim_item(id);
    assert_eq!(h.set(&item), set(&[Trait::Personal]));
    let rec = h.record(&item).unwrap();
    assert_eq!((rec["traits"][0]["by"].as_str(), rec["traits"][0]["from"][0].as_str()), (Some("derived"), Some(source_item(&sid).as_str())));
    assert_eq!(lib.claim(id).unwrap()[VIEW_KEY]["traits"], json!(["personal"]));
    assert!(!h.has_weight(&item));
}

/// `may_rest_on` and `from` lists are bounded; the stores are written whole and an unreadable one is never written over.
#[test]
fn the_records_are_bounded_and_an_unreadable_store_is_kept() {
    let (_t, ws, home) = ws();
    let h = Handling::of(&ws, &home);
    let used: Vec<String> = (0..80).map(|i| format!("local/summaries/u{i}.md")).collect();
    for u in &used {
        h.add(u, None, &set(&[Trait::Internal]), &By::Operator, &[]).unwrap();
    }
    h.derive("local/out.md", None, &used, &[]).unwrap();
    assert_eq!(h.record("local/out.md").unwrap()["traits"][0]["from"].as_array().unwrap().len(), MAX_FROM);
    std::fs::write(dir(&ws).join("records.json"), "{broken").unwrap();
    assert!(h.set("local/out.md").is_empty(), "an unreadable store reads as unlabelled: nothing is withheld");
    assert!(h.add("local/x.md", None, &set(&[Trait::Internal]), &By::Operator, &[]).unwrap_err().contains("cannot be read"));
    assert_eq!(std::fs::read_to_string(dir(&ws).join("records.json")).unwrap(), "{broken");
    assert_eq!(h.unreadable().len(), 1);
}

// ---- separation (AC-5) and the inventories (§14) -----------------------------------------------------------------------------

fn src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The lines of a file before its first test module.
fn non_test(text: &str) -> String {
    text.split("\n#[cfg(test)]\nmod ").next().unwrap_or("").to_string()
}

/// AC-5, by the code: the files that decide a `source_type`, the promotion check, the tools a span holds, or the spec 103 mark
/// call nothing in this module that reads a set. The readers among them only note what a span was given.
#[test]
fn no_trust_decision_reads_handling() {
    const DECIDES: [&str; 8] = ["promote.rs", "private_read.rs", "tools.rs", "retrieval.rs", "shared_research.rs", "library.rs", "finding_record.rs", "provisional_notes.rs"];
    const MAY_CALL: [&str; 9] = ["note_given(", "item_of(", "span_key(", "Handling::of(", "Handling::workspace_only(", "label_file(", "source_item(", "item_in_ws(", "view_for("];
    for f in DECIDES {
        let text = non_test(&std::fs::read_to_string(src().join(f)).unwrap());
        for (i, line) in text.lines().enumerate() {
            let mut rest = line;
            while let Some(at) = rest.find("handling::") {
                let call = &rest[at + "handling::".len()..];
                assert!(MAY_CALL.iter().any(|m| call.starts_with(m)), "{f}:{}: `{}` reads handling where a trust decision is made", i + 1, line.trim());
                rest = call;
            }
            for word in [".has_weight(", "set_of("] {
                assert!(!line.contains(word) || !line.contains("handling"), "{f}:{}: {}", i + 1, line.trim());
            }
        }
    }
    // and nothing here names the modules that decide
    let me = non_test(&std::fs::read_to_string(src().join("handling.rs")).unwrap());
    for m in ["crate::promote", "crate::private_read", "crate::tools", "provenance::Tier", "source_type ="] {
        assert!(!me.contains(m), "handling.rs names `{m}`");
    }
}

/// AC-5, by behaviour: labelling every item changes neither the spec 103 mark nor anything a reader returns: the given list is a
/// note under `local/handling/`, and the mark's record is untouched.
#[test]
fn labels_set_no_private_read_mark() {
    let (_t, ws, home) = ws();
    let h = Handling::of(&ws, &home);
    for (i, t) in Trait::ALL.iter().enumerate() {
        h.operator_set(&format!("local/summaries/{i}.md"), *t).unwrap();
    }
    let items: Vec<String> = (0..3).map(|i| format!("local/summaries/{i}.md")).collect();
    note_given(&h, &ws, &span_key("m1", "writer"), &items);
    h.derive("local/out.md", None, &items, &items).unwrap();
    for key in ["m1", "m1/writer", "m1/t1"] {
        assert_eq!(crate::private_read::state(&ws, key), crate::private_read::State::Unmarked, "{key}");
    }
    let written: Vec<PathBuf> = walk(&ws);
    assert!(written.iter().all(|p| p.starts_with(dir(&ws))), "only the handling folder was written: {written:?}");
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// §14: every file of this crate that writes a derived record is listed, with what it does about propagation; one that says it
/// derives calls into this module; and every listed file still writes one.
#[test]
fn every_write_path_is_listed_with_what_it_does() {
    let mut found: Vec<String> = Vec::new();
    for e in std::fs::read_dir(src()).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".rs") || name.ends_with("tests.rs") || name.starts_with("handling") {
            continue;
        }
        let text = non_test(&std::fs::read_to_string(e.path()).unwrap());
        if WRITERS.iter().any(|w| text.contains(w)) {
            found.push(name);
        }
    }
    found.sort();
    let mut listed: Vec<String> = WRITES.iter().map(|w| w.file.to_string()).collect();
    listed.sort();
    assert_eq!(found, listed, "a file writes a derived record and is not in handling::WRITES (or one listed no longer does)");
    for w in WRITES.iter().filter(|w| w.how == Propagation::Derives) {
        let text = non_test(&std::fs::read_to_string(src().join(w.file)).unwrap());
        assert!(text.contains("handling::") || text.contains("label_stored("), "{} says it derives and never calls handling", w.file);
    }
    for w in &WRITES {
        if let Propagation::NotYet { why } | Propagation::NoRecord { why } = w.how {
            assert!(why.len() > 20, "{}: say why", w.file);
        }
    }
}

/// §3 item 3: the operator's clear has one caller outside this module and its tests: the terminal command, which refuses an agent
/// seat and a process without a terminal. No tool, host method, relay reply or handoff reaches it.
#[test]
fn only_the_operators_terminal_command_clears() {
    let roots = [src(), PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../delphi/src")];
    let mut callers: Vec<String> = Vec::new();
    for root in roots {
        for p in walk(&root).into_iter().filter(|p| p.extension().is_some_and(|x| x == "rs")) {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with("tests.rs") {
                continue;
            }
            let text = non_test(&std::fs::read_to_string(&p).unwrap());
            if text.contains("operator_clear(") && !p.ends_with("handling.rs") {
                callers.push(name);
            }
        }
    }
    assert_eq!(callers, ["handling_cmd.rs"]);
    let cmd = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../delphi/src/handling_cmd.rs")).unwrap();
    assert!(cmd.contains("inside_an_agent_seat()") && cmd.contains("is_terminal()"), "the command checks the seat and the terminal");
}
