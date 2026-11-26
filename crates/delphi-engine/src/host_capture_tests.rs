//! Tests of `capture_source` (`specs/142` §2.2; slice C), one or more per acceptance criterion of the slice.

use super::*;
use crate::host_research::Fetched;
use crate::library::Channel;
use std::cell::RefCell;

/// The library's exit check, by another name: tests call it directly, production goes through `egress::check`.
const GATE: fn(&Value, Channel) -> Result<(), String> = crate::library::may_leave;

const NOW: &str = "2026-11-26T10:00:00Z";
const NOW_UNIX: i64 = 1_795_687_200;
const TIDES: &str = "The spring tide at the north harbour rises four metres above the chart datum, twice the neap range, and the harbour master closes the inner basin an hour before high water.";

struct Fx {
    _t: tempfile::TempDir,
    home: PathBuf,
    ws: PathBuf,
    project: PathBuf,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().canonicalize().unwrap();
    let ws = home.join("workspaces").join("w");
    let project = home.join("project");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(Ctx::handover_of(&home, "sess0001")).unwrap();
    Fx { _t: t, home, ws, project }
}

fn ctx<'a>(f: &'a Fx, session: &'a str) -> Ctx<'a> {
    Ctx {
        home: &f.home,
        session,
        label: "host:claude-code/opus",
        project_root: Some(&f.project),
        handover: Ctx::handover_of(&f.home, session),
        read_marked: false,
        identifiers: &[],
        keep: "local_text",
        cache_max: Some(1024 * 1024 * 1024),
        limits: Limits::default(),
        now: NOW.into(),
        now_unix: NOW_UNIX,
        limited: true,
    }
}

fn same(s: &str) -> String {
    s.to_string()
}

/// A web that serves `pages` and records every address it is asked for; any other address is off its lists.
fn web<'a>(asked: &'a RefCell<Vec<String>>, pages: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Result<Fetched, String> + 'a {
    move |url: &str| {
        asked.borrow_mut().push(url.to_string());
        let (_, text) = pages.iter().find(|(u, _)| *u == url).ok_or_else(|| "OffList".to_string())?;
        Ok((url.to_string(), crate::fact_fetch::test_text_snapshot(url, text)))
    }
}

fn nowhere(asked: &RefCell<Vec<String>>) -> impl Fn(&str) -> Result<Fetched, String> + '_ {
    move |url: &str| {
        asked.borrow_mut().push(url.to_string());
        Err("OffList".to_string())
    }
}

fn lib(f: &Fx) -> Library {
    Library::in_home(&f.home)
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(dir).into_iter().flatten() {
        if e.file_type().is_file() {
            out.push(e.path().to_path_buf());
        }
    }
    out.sort();
    out
}

/// AC-4: `text` is kept as a capture `captured_by: host:…`, extractor `host`, with both digests; a search that asks for untrusted text
/// (as an agent does when it reads the library's web text) finds a passage of it at `external_untrusted`, and the source reads not
/// validated.
#[test]
fn a_handed_text_is_kept_as_the_hosts_capture_and_found_by_a_search_labelled_not_validated() {
    let f = fx();
    let asked = RefCell::new(Vec::new());
    let r = capture(&ctx(&f, "sess0001"), &[json!({"locator": "https://harbour.example.net/tides", "text": TIDES, "title": "Harbour tides"})], false, &same, &nowhere(&asked)).unwrap();
    let item = &r["sources"][0];
    assert_eq!(item["outcome"], "added", "{r}");
    let id = item["source_id"].as_str().unwrap();
    let rec = lib(&f).capture_record(id).unwrap();
    assert_eq!(rec["captured_by"], "host:claude-code/opus");
    assert_eq!(rec["extractor"]["name"], "host");
    assert_eq!(rec["rights"], "held");
    assert_eq!(item["text_sha256"], rec["text_sha256"]);
    let meta = lib(&f).read_meta(id).unwrap();
    assert_eq!(meta["status"], "not validated");
    assert_eq!(crate::library::capture::host_word(&meta), Some(crate::library::capture::NOT_CONFIRMED));
    assert_eq!(asked.borrow().as_slice(), ["https://harbour.example.net/tides"], "the daemon tried its own read to compare, under its rules");
    let files = crate::kb::sources(&f.ws);
    let doc = files.iter().find(|d| d.rel_path == format!("library/sources/{id}/source.md")).expect("the host capture is indexed");
    assert_eq!(doc.default_source, "external_untrusted");
    let mut kb = crate::kb::Kb::open_in_memory().unwrap();
    kb.sync_lexical(&files, NOW).unwrap();
    let hits = kb.search("inner basin harbour master", &crate::kb::SearchOpts { include_untrusted: true, ..Default::default() }).unwrap();
    let hit = hits.iter().find(|h| h.rel_path == doc.rel_path).expect("a passage of it is found");
    assert_eq!(hit.source_type, "external_untrusted");
}

/// AC-4: both bodies, a file outside the two folders, a file through a link, and a call past a limit are refused and write nothing.
#[test]
fn a_call_with_both_bodies_a_file_outside_the_folders_a_link_or_over_a_limit_writes_nothing() {
    let f = fx();
    let outside = f.home.join("elsewhere.txt");
    std::fs::write(&outside, TIDES).unwrap();
    let hand = Ctx::handover_of(&f.home, "sess0001");
    std::os::unix::fs::symlink(&outside, hand.join("link.txt")).unwrap();
    std::fs::write(hand.join("ok.txt"), TIDES).unwrap();
    let asked = RefCell::new(Vec::new());
    let before = files_under(&f.home);
    for item in [
        json!({"locator": "https://a.example.net/1", "text": TIDES, "file": hand.join("ok.txt").display().to_string()}),
        json!({"locator": "https://a.example.net/2", "file": outside.display().to_string()}),
        json!({"locator": "https://a.example.net/3", "file": hand.join("link.txt").display().to_string()}),
        json!({"locator": "https://a.example.net/4", "file": "../../elsewhere.txt"}),
        json!({"locator": "https://a.example.net/5", "text": "x".repeat(crate::source_normalise::TEXT_MAX + 1)}),
    ] {
        let r = capture(&ctx(&f, "sess0001"), std::slice::from_ref(&item), false, &same, &nowhere(&asked)).unwrap();
        assert_eq!(r["sources"][0]["outcome"], "refused", "{item}: {r}");
    }
    // Over the call's limit: refused before anything is read.
    let many: Vec<Value> = (0..21).map(|i| json!({"locator": format!("https://a.example.net/{i}"), "text": TIDES})).collect();
    assert!(parse(&json!({"sources": many})).is_err());
    // Over the hour's limit: refused whole.
    let tight = Ctx { limits: Limits { session_bytes: 100, ..Limits::default() }, ..ctx(&f, "sess0001") };
    let e = capture(&tight, &[json!({"locator": "https://a.example.net/6", "text": TIDES})], false, &same, &nowhere(&asked)).unwrap_err();
    assert!(e.contains("handover_session_mib"), "{e}");
    assert_eq!(files_under(&f.home), before, "nothing was written");
    assert!(asked.borrow().is_empty(), "nothing refused was fetched");
}

/// AC-5: a `record_research` claim whose quote is in a host capture gains an anchor `in: host_capture`, reads `host-reported`, and adds
/// nothing to its corroboration count; the same when the capture arrives after the claim (its quote waited on the claim).
#[test]
fn a_quote_in_a_host_capture_anchors_there_stays_host_reported_and_counts_for_nothing_in_either_order() {
    use crate::host_research as hr;
    let quote = "closes the inner basin an hour before high water";
    let call = |url: &str| {
        json!({
            "question": "When does the harbour close the inner basin?",
            "answer": "An hour before high water.",
            "sources": [{"locator": url, "title": "Harbour tides", "kind": "web"}],
            "claims": [{"text": "The inner basin closes an hour before high water", "quote": quote, "source": 0}]
        })
    };
    for capture_first in [true, false] {
        let f = fx();
        let url = "https://harbour.example.net/tides";
        let asked = RefCell::new(Vec::new());
        let hand = || capture(&ctx(&f, "sess0001"), &[json!({"locator": url, "text": TIDES})], false, &same, &nowhere(&asked)).unwrap();
        if capture_first {
            hand();
        }
        let rctx = hr::Ctx { home: &f.home, root: &f.ws, project: None, label: "host:claude-code/opus", run_id: "h-1", read_marked: false, identifiers: &[], pictures: None };
        let reply = hr::record(&rctx, &hr::parse(&call(url), &same).unwrap(), &nowhere(&asked)).unwrap();
        if !capture_first {
            assert_eq!(reply["quotes_not_found"], 1, "nothing to look in yet: {reply}");
            hand();
        }
        let l = lib(&f);
        let claim = l.claims().into_iter().find(|c| c["text"].as_str().is_some_and(|t| t.contains("inner basin"))).unwrap();
        let anchors = claim["anchors"].as_array().cloned().unwrap_or_default();
        assert_eq!(anchors.len(), 1, "capture first {capture_first}: {claim}");
        assert_eq!(anchors[0]["in"], "host_capture");
        assert!(anchors[0]["start"].is_u64(), "found in the handed text");
        assert_eq!(claim["host_reported"], json!(["host-reported"]), "{claim}");
        assert!(l.cites_of(&claim).is_empty(), "no origin from a host's word");
        assert_eq!(claim["support"]["origins"].as_u64().unwrap_or(0), 0);
        assert!(claim["waiting"].is_null());
    }
}

/// AC-6: the daemon reading the address and getting the same `text_sha256` confirms the capture, and its anchor then counts; other text
/// is the daemon's own new version, and the host's anchor does not count.
#[test]
fn the_daemons_same_text_confirms_a_host_capture_and_other_text_is_its_own_new_version() {
    let url = "https://harbour.example.net/tides";
    for (page, confirms) in [(TIDES, true), ("The harbour is closed for repairs until spring; the inner basin is drained.", false)] {
        let f = fx();
        let asked = RefCell::new(Vec::new());
        let pages = [(url, page)];
        let r = capture(&ctx(&f, "sess0001"), &[json!({"locator": url, "text": TIDES})], false, &same, &web(&asked, &pages)).unwrap();
        let id = r["sources"][0]["source_id"].as_str().unwrap().to_string();
        let l = lib(&f);
        let meta = l.read_meta(&id).unwrap();
        let rec = l.capture_record(&id).unwrap();
        if confirms {
            assert_eq!(r["sources"][0]["confirmed"], true, "{r}");
            assert_eq!(rec["confirmed_by_daemon"], NOW);
            assert_eq!(meta["confirmed_by_daemon"], NOW);
            assert_eq!(meta["version"], "1", "a confirmation is no new version");
            assert_eq!(crate::library::capture::host_word(&meta), None);
        } else {
            assert_eq!(r["sources"][0]["confirmed"], false);
            assert_eq!(meta["version"], "2", "the daemon's copy is a new version");
            assert_eq!(rec["captured_by"], "daemon");
            assert!(meta["confirmed_by_daemon"].is_null());
        }
    }
}

/// AC-7: under the private-read mark `capture_source` asks the web for nothing, keeps the text, and the source is library-private: no
/// channel that carries a body may send it.
#[test]
fn under_the_mark_capture_source_makes_no_request_keeps_the_text_and_the_source_is_library_private() {
    let f = fx();
    let asked = RefCell::new(Vec::new());
    let marked = Ctx { read_marked: true, ..ctx(&f, "sess0001") };
    let r = capture(&marked, &[json!({"locator": "https://harbour.example.net/tides", "text": TIDES}), json!({"locator": "https://harbour.example.net/other"})], false, &same, &nowhere(&asked)).unwrap();
    assert!(asked.borrow().is_empty(), "no request under the mark");
    assert_eq!(r["sources"][1]["outcome"], "refused");
    assert_eq!(r["sources"][1]["why"], NOT_FETCHED);
    let id = r["sources"][0]["source_id"].as_str().unwrap();
    assert!(id.starts_with("private-"), "{id}");
    let meta = lib(&f).read_meta(id).unwrap();
    assert!(crate::library::is_private(&meta));
    assert!(std::fs::read_to_string(lib(&f).source_dir(id).join("source.md")).unwrap().contains("inner basin"), "the text is kept");
    assert_eq!(crate::library::capture::host_word(&meta), Some(crate::library::capture::CANNOT_CONFIRM));
    assert!(r["sources"][0]["label"].as_array().unwrap().contains(&json!("library_private")));
    for c in Channel::ALL {
        assert!(GATE(&meta, c).is_err(), "{c:?}");
    }
    assert!(!crate::kb::sources(&f.ws).iter().any(|d| d.rel_path.contains(id)), "a library-private source is in no index");
}

/// AC-16: five items in one call, each answered on its own: text, a file, an address with text, an address alone on the lists, an
/// address alone off them. Metadata: `csl` fills the citation, `topics` attach, `labels` add a trait and cannot remove one.
#[test]
fn five_items_are_answered_each_on_its_own_and_metadata_adds_without_removing() {
    let f = fx();
    let hand = Ctx::handover_of(&f.home, "sess0001");
    std::fs::write(hand.join("saved.html"), "<!doctype html><html><head><title>Saved page</title></head><body><p>The lock gates open at half tide.</p></body></html>").unwrap();
    lib(&f).topic_create("Harbour tides", crate::library::topics::Origin::Operator, NOW_UNIX as u64).unwrap();
    let asked = RefCell::new(Vec::new());
    let pages = [("https://listed.example.org/tables", "Tide tables for the north harbour are published each week.")];
    let items = [
        json!({"locator": "the harbour master's notice board", "kind": "internal", "text": "Inner basin closed on Sunday.", "labels": ["internal"]}),
        json!({"locator": "https://harbour.example.net/saved", "file": "saved.html", "media_type": "text/html"}),
        json!({"locator": "https://harbour.example.net/tides", "text": TIDES, "csl": {"type": "report", "author": "Harbour Office", "DOI": "10.5555/Tide.1"}, "topics": ["harbour-tides"], "labels": ["confidential"]}),
        json!({"locator": "https://listed.example.org/tables"}),
        json!({"locator": "https://unlisted.example.org/x"}),
    ];
    let r = capture(&ctx(&f, "sess0001"), &items, false, &same, &web(&asked, &pages)).unwrap();
    let outcomes: Vec<&str> = r["sources"].as_array().unwrap().iter().map(|i| i["outcome"].as_str().unwrap()).collect();
    assert_eq!(outcomes, ["added", "added", "added", "added", "refused"], "{r}");
    assert_eq!(r["sources"][4]["why"], NOT_FETCHED);
    let l = lib(&f);
    let saved = l.capture_record(r["sources"][1]["source_id"].as_str().unwrap()).unwrap();
    assert_eq!(saved["extractor"]["name"], crate::fact_fetch::EXTRACTOR_NAME, "the file's text is made by the library's extractor");
    assert_eq!(l.read_meta(r["sources"][1]["source_id"].as_str().unwrap()).unwrap()["title"], "Saved page");
    assert_eq!(l.capture_record(r["sources"][3]["source_id"].as_str().unwrap()).unwrap()["captured_by"], "daemon", "an address alone is the daemon's read");
    let id = r["sources"][2]["source_id"].as_str().unwrap();
    let meta = l.read_meta(id).unwrap();
    assert_eq!(meta["type"], "report");
    assert_eq!(meta["DOI"], "10.5555/tide.1");
    assert_eq!(meta["author"], json!([{"family": "Office", "given": "Harbour"}]));
    assert_eq!(meta["topics"], json!(["harbour-tides"]));
    let handling = crate::handling::Handling::library_only(&f.home);
    assert!(handling.set(&crate::handling::source_item(id)).contains(&crate::handling::Trait::Confidential));
    let internal = r["sources"][0]["source_id"].as_str().unwrap();
    assert!(handling.set(&crate::handling::source_item(internal)).contains(&crate::handling::Trait::Internal));
    // `labels` add; a second call naming none removes nothing.
    capture(&ctx(&f, "sess0001"), &[json!({"locator": "https://harbour.example.net/tides", "text": TIDES, "labels": []})], false, &same, &nowhere(&asked)).unwrap();
    assert!(handling.set(&crate::handling::source_item(id)).contains(&crate::handling::Trait::Confidential));
}

/// AC-17: an author as one string, a date with no time and an upper-case DOI are stored normalised and the three corrections listed; an
/// unknown field, a locator with credentials and a file that is not UTF-8 are refused with the field named; a dry run returns the record
/// the same call then writes, byte for byte, and writes nothing.
#[test]
fn findings_are_listed_unknown_fields_refused_and_a_dry_run_is_the_record_the_call_writes() {
    let f = fx();
    let asked = RefCell::new(Vec::new());
    let item = json!({"locator": "https://harbour.example.net/tides", "text": TIDES, "retrieved": "2026-11-20", "csl": {"author": "Jane Doe", "DOI": "10.5555/ABC"}});
    let c = ctx(&f, "sess0001");
    let before = files_under(&f.home);
    let dry = capture(&c, std::slice::from_ref(&item), true, &same, &nowhere(&asked)).unwrap();
    assert_eq!(files_under(&f.home), before, "a dry run writes nothing");
    assert!(asked.borrow().is_empty(), "a dry run reads no address");
    let mut fields: Vec<&str> = dry["sources"][0]["findings"].as_array().unwrap().iter().map(|x| x["field"].as_str().unwrap()).collect();
    fields.sort();
    assert_eq!(fields, ["csl.DOI", "csl.author", "retrieved"], "{dry}");
    assert!(dry["sources"][0]["findings"].as_array().unwrap().iter().all(|x| x["kind"] == "normalised"));
    let real = capture(&c, &[item], false, &same, &nowhere(&asked)).unwrap();
    let id = real["sources"][0]["source_id"].as_str().unwrap();
    let dir = lib(&f).source_dir(id);
    assert_eq!(serde_json::to_vec_pretty(&dry["sources"][0]["record"]["meta"]).unwrap(), std::fs::read(dir.join("meta.json")).unwrap(), "meta.json, byte for byte");
    assert_eq!(serde_json::to_vec_pretty(&dry["sources"][0]["record"]["capture"]).unwrap(), std::fs::read(dir.join("capture.json")).unwrap(), "capture.json, byte for byte");
    let meta = lib(&f).read_meta(id).unwrap();
    assert_eq!((meta["retrieved"].as_str(), meta["DOI"].as_str()), (Some("2026-11-20T00:00:00Z"), Some("10.5555/abc")));
    // Refused, each with its field named.
    let hand = Ctx::handover_of(&f.home, "sess0001");
    std::fs::write(hand.join("latin1.txt"), [0x63, 0x61, 0x66, 0xe9]).unwrap();
    for (item, field) in [
        (json!({"locator": "https://harbour.example.net/a", "text": TIDES, "colour": "blue"}), "colour"),
        (json!({"locator": "https://user:secret@harbour.example.net/a", "text": TIDES}), "locator"),
        (json!({"locator": "https://harbour.example.net/b", "file": "latin1.txt"}), "file"),
    ] {
        let r = capture(&c, &[item], false, &same, &nowhere(&asked)).unwrap();
        assert_eq!(r["sources"][0]["outcome"], "refused");
        assert!(r["sources"][0]["findings"].as_array().unwrap().iter().any(|x| x["field"] == field && x["kind"] == "error"), "{field}: {r}");
    }
}

/// AC-18: a host that claims an open licence for a page whose licence the daemon does not find: `rights: held`, `licence_claimed`
/// recorded, and no exit sends its body.
#[test]
fn a_claimed_licence_is_recorded_and_never_raises_rights() {
    let f = fx();
    let asked = RefCell::new(Vec::new());
    let r = capture(&ctx(&f, "sess0001"), &[json!({"locator": "https://harbour.example.net/tides", "text": TIDES, "licence": "CC-BY-4.0"})], false, &same, &nowhere(&asked)).unwrap();
    let id = r["sources"][0]["source_id"].as_str().unwrap();
    let l = lib(&f);
    let meta = l.read_meta(id).unwrap();
    assert_eq!(meta["licence_claimed"], "CC-BY-4.0");
    assert_eq!(meta["licence"], "unknown");
    assert_eq!(l.capture_record(id).unwrap()["rights"], "held");
    assert!(crate::library::is_held(&meta));
    for c in Channel::ALL.into_iter().filter(|c| !matches!(c, Channel::ArchiveLookup | Channel::CitationLookup | Channel::DoiResolve | Channel::Export | Channel::WebSearch)) {
        assert!(GATE(&meta, c).is_err(), "{c:?}");
    }
    let out = tempfile::tempdir().unwrap();
    assert!(l.export_to_bundle(out.path(), &[id.to_string()], false)[0].1.is_err(), "a bundle copy is refused");
}

/// AC-23 and the fitness function: the file is opened, then checked on its descriptor, and read from it only. A file swapped for a
/// link between the open and the read is read as the file first opened.
#[test]
fn the_file_is_opened_then_checked_on_its_descriptor() {
    let f = fx();
    let hand = Ctx::handover_of(&f.home, "sess0001");
    let secret = f.home.join("secret.txt");
    std::fs::write(&secret, "the operator's secret").unwrap();
    std::fs::write(hand.join("page.txt"), "the page the host saved").unwrap();
    let swap = || {
        std::fs::remove_file(hand.join("page.txt")).unwrap();
        std::os::unix::fs::symlink(&secret, hand.join("page.txt")).unwrap();
    };
    let got = read_file(&[hand.as_path()], "page.txt", &swap).unwrap();
    assert_eq!(got, b"the page the host saved", "read from the descriptor first opened");
    // A second read now meets the link and is refused.
    assert!(read_file(&[hand.as_path()], "page.txt", &|| {}).is_err());
    // A folder, not a regular file, is refused on the descriptor.
    std::fs::create_dir_all(hand.join("dir")).unwrap();
    assert!(read_file(&[hand.as_path()], "dir", &|| {}).unwrap_err().contains("not a regular file"));
}

/// The fitness function of §6: no file outside the project folder and the handover folder is read: not by a link (to a file or a
/// folder), not by `..`, not by an absolute path elsewhere.
#[test]
fn a_path_outside_the_project_and_handover_folders_is_refused() {
    let f = fx();
    let hand = Ctx::handover_of(&f.home, "sess0001");
    let other = f.home.join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("x.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(&other, f.project.join("linked")).unwrap();
    std::os::unix::fs::symlink(other.join("x.txt"), hand.join("x.txt")).unwrap();
    std::fs::write(f.project.join("in.txt"), "inside the project").unwrap();
    let roots: [&Path; 2] = [hand.as_path(), f.project.as_path()];
    for p in ["linked/x.txt", "x.txt", "../other/x.txt", &other.join("x.txt").display().to_string(), "/etc/hosts"] {
        assert!(read_file(&roots, p, &|| {}).is_err(), "{p}");
    }
    assert_eq!(read_file(&roots, &f.project.join("in.txt").display().to_string(), &|| {}).unwrap(), b"inside the project");
    assert_eq!(read_file(&roots, "in.txt", &|| {}).unwrap(), b"inside the project");
}

/// AC-23: the seventeenth megabyte a session hands over in an hour is refused with the limit named; the next hour it is allowed.
#[test]
fn the_seventeenth_megabyte_a_session_hands_over_in_an_hour_is_refused() {
    let f = fx();
    let limits = Limits::default();
    let mib = 1024 * 1024;
    for i in 0..16 {
        within(&f.home, "sess0001", 1, mib, &limits, NOW_UNIX + i).unwrap();
        count(&f.home, "sess0001", 1, mib, NOW_UNIX + i).unwrap();
    }
    let e = within(&f.home, "sess0001", 1, mib, &limits, NOW_UNIX + 20).unwrap_err();
    assert!(e.contains("16 MiB an hour for one session") && e.contains("handover_session_mib"), "{e}");
    assert!(within(&f.home, "sess0001", 1, mib, &limits, NOW_UNIX + 3700).is_ok(), "an hour later");
}

/// AC-29, first clause: three sessions together cannot hand over more than the hour's limit for all sessions, though each is inside its
/// own; and a host capture is refused `cache full` rather than evicting text used this week.
#[test]
fn three_sessions_together_cannot_pass_the_hourly_limit_for_all_sessions() {
    let f = fx();
    let limits = Limits { all_bytes: 40 * 1024 * 1024, ..Limits::default() };
    let mib = 1024 * 1024;
    for s in ["sessaaaa", "sessbbbb"] {
        within(&f.home, s, 1, 16 * mib, &limits, NOW_UNIX).unwrap();
        count(&f.home, s, 1, 16 * mib, NOW_UNIX).unwrap();
    }
    let e = within(&f.home, "sesscccc", 1, 9 * mib, &limits, NOW_UNIX).unwrap_err();
    assert!(e.contains("all sessions together") && e.contains("handover_hour_mib"), "{e}");
    assert!(within(&f.home, "sesscccc", 1, 8 * mib, &limits, NOW_UNIX).is_ok());
    // Sources: 300 for all sessions.
    let f = fx();
    for s in ["sessaaaa", "sessbbbb"] {
        count(&f.home, s, 120, 0, NOW_UNIX).unwrap();
    }
    assert!(within(&f.home, "sesscccc", 61, 0, &Limits::default(), NOW_UNIX).unwrap_err().contains("300 sources an hour for all sessions"));
    // The cache: full of text used today, a host capture is refused and nothing is evicted.
    let f = fx();
    let asked = RefCell::new(Vec::new());
    let big = "tide ".repeat(4000);
    let first = capture(&ctx(&f, "sess0001"), &[json!({"locator": "https://harbour.example.net/a", "text": big})], false, &same, &nowhere(&asked)).unwrap();
    let id = first["sources"][0]["source_id"].as_str().unwrap().to_string();
    let l = lib(&f);
    l.note_use(&[id.as_str()], NOW_UNIX);
    l.flush_uses();
    let small = Ctx { cache_max: Some(30_000), ..ctx(&f, "sess0001") };
    let e = capture(&small, &[json!({"locator": "https://harbour.example.net/b", "text": big})], false, &same, &nowhere(&asked)).unwrap_err();
    assert!(e.contains("cache full"), "{e}");
    assert!(l.source_dir(&id).join("source.md").is_file(), "nothing was evicted");
    assert!(l.read_meta(&crate::library::source_id("https://harbour.example.net/b")).is_none());
}

/// `delphi sources lint`: the stored record of a source whose DOI was kept in upper case reads as a normalisation to make.
#[test]
fn lint_lists_what_normalising_a_stored_record_again_would_change() {
    let f = fx();
    let asked = RefCell::new(Vec::new());
    capture(&ctx(&f, "sess0001"), &[json!({"locator": "https://harbour.example.net/tides", "text": TIDES})], false, &same, &nowhere(&asked)).unwrap();
    assert!(lint(&f.home, None).is_empty(), "what capture_source stored is already normal");
    let id = crate::library::source_id("https://harbour.example.net/tides");
    let path = lib(&f).source_dir(&id).join("meta.json");
    let mut m: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    m["DOI"] = json!("10.5555/UPPER");
    std::fs::write(&path, serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    let found = lint(&f.home, Some(&id));
    assert_eq!(found.len(), 1);
    assert!(found[0].1.iter().any(|x| x.field == "csl.DOI" && x.kind == Kind::Normalised));
}
