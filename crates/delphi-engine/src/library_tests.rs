use super::*;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};

fn lib() -> (tempfile::TempDir, Library) {
    let home = tempfile::tempdir().unwrap();
    let l = Library::in_home(home.path());
    (home, l)
}

fn file(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

fn open(licence: &str) -> AddOpts {
    AddOpts { licence: Some(licence.into()), ..Default::default() }
}

fn serve(body: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let r = format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
            let _ = s.write_all(r.as_bytes());
        }
    });
    addr
}

#[test]
fn a_full_capture_over_a_budget_is_refused_with_the_reason_and_falls_back() {
    let (d, l) = lib();
    let f = file(d.path(), "a.md", &"x".repeat(100));
    let tiny = Budgets { text: 10, ..Budgets::default() };
    let s = l.add_file(&f, &open("CC0-1.0"), &tiny).unwrap();
    assert_eq!(s.level, Level::Excerpts);
    assert!(s.notes.iter().any(|n| n.contains("limit for one source's text")), "{:?}", s.notes);
    assert!(!l.source_dir(&s.id).join("source.md").exists(), "no text is kept past the limit");
    // the operator can raise the limit for that one source
    let raised = AddOpts { raise_text: Some(1000), ..open("CC0-1.0") };
    let s = l.add_file(&f, &raised, &tiny).unwrap();
    assert_eq!((s.level, s.outcome.clone()), (Level::Full, Outcome::NewVersion(2)));
    let kept = s.id.clone();
    // the library budget stops a capture and evicts nothing
    let g = file(d.path(), "b.md", &"y".repeat(100));
    let full = Budgets { library: 150, ..Budgets::default() };
    let s = l.add_file(&g, &open("CC0-1.0"), &full).unwrap();
    assert_eq!(s.level, Level::Pointer);
    assert!(l.source_dir(&kept).join("source.md").exists());
    // a restricted licence cannot be kept whole unless the operator flags it private
    assert!(l.add_file(&g, &AddOpts { level: Some(Level::Full), ..open("all-rights-reserved") }, &Budgets::default()).is_err());
}

#[test]
fn a_private_source_never_leaves_and_is_never_indexed() {
    let (d, l) = lib();
    let f = file(d.path(), "p.md", "schematic of the secret valve");
    let o = AddOpts { private: true, level: Some(Level::Full), ..open("all-rights-reserved") };
    let s = l.add_file(&f, &o, &Budgets::default()).unwrap();
    assert_eq!(s.level, Level::Full);
    let meta = l.read_meta(&s.id).unwrap();
    for c in Channel::ALL {
        assert!(may_leave(&meta, c).is_err(), "{:?}", c);
    }
    assert!(l.indexable().is_empty(), "a private source is not in the index");
    // a restricted body does not travel either, and an open one may
    assert!(may_leave(&json!({"licence": "all-rights-reserved"}), Channel::Xenia).is_err());
    assert!(may_leave(&json!({"licence": "CC-BY-4.0"}), Channel::Bundle).is_ok());
    // without the flag, a restricted licence cannot be kept whole
    let err = l.add_file(&f, &AddOpts { level: Some(Level::Full), ..open("unknown") }, &Budgets::default()).unwrap_err();
    assert!(err.contains("--private"), "{}", err);
}

#[test]
fn a_changed_capture_is_a_new_version_and_a_purge_keeps_only_the_citation_and_the_hash() {
    let (d, l) = lib();
    let f = file(d.path(), "n.md", "# One\n\nfirst");
    let s = l.add_file(&f, &open("own"), &Budgets::default()).unwrap();
    assert_eq!(s.outcome, Outcome::Added);
    assert_eq!(l.add_file(&f, &open("own"), &Budgets::default()).unwrap().outcome, Outcome::Unchanged);
    file(d.path(), "n.md", "# One\n\nfirst\n\n# Two\n\nsecond");
    let s2 = l.add_file(&f, &open("own"), &Budgets::default()).unwrap();
    assert_eq!(s2.outcome, Outcome::NewVersion(2));
    let dir = l.source_dir(&s.id);
    assert_eq!(std::fs::read_to_string(dir.join("versions/1/source.md")).unwrap(), "# One\n\nfirst");
    assert!(l.read_meta(&s.id).unwrap()["change_note"].as_str().unwrap().contains("new sections: Two"));
    let hash = l.read_meta(&s.id).unwrap()["content_hash"].clone();
    assert!(l.purge(&s.id).unwrap().unwrap() > 0);
    assert!(!dir.join("source.md").exists() && !dir.join("versions").exists());
    let m = l.read_meta(&s.id).unwrap();
    assert_eq!((m["content_hash"].clone(), m["title"].as_str()), (hash, Some("n.md")));
    assert!(l.indexable().is_empty());
    assert!(std::fs::read_to_string(l.root().join("log.jsonl")).unwrap().contains("\"purge\""));
    assert_eq!(l.purge("no-such-id").unwrap(), None);
}

#[test]
fn a_bundles_sources_enter_not_validated_with_the_licence_intact_and_are_indexed() {
    let (_d, l) = lib();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dev/bundles/acme-platform");
    let r = l.import_bundle(&golden, "acme/acme-platform@1.4.0", &Budgets::default());
    assert_eq!(r.len(), 2);
    assert!(r.iter().all(|(_, s)| s.is_ok()), "{:?}", r.iter().map(|(i, s)| (i, s.as_ref().err())).collect::<Vec<_>>());
    let m = l.read_meta("oecd-2024-report").unwrap();
    assert_eq!((m["licence"].as_str(), m["status"].as_str(), m["capture_level"].as_str(), m["imported_from"].as_str()), (Some("CC-BY-4.0"), Some("not validated"), Some("full"), Some("acme/acme-platform@1.4.0")));
    assert!(m["attribution_text"].as_str().unwrap().contains("CC BY 4.0") && m["share_alike"] == false);
    assert!(l.source_dir("oecd-2024-report").join("assets/figure-1.png").is_file());
    assert_eq!(l.read_meta("acme-style-guide").unwrap()["licence"], "own");
    assert_eq!(l.indexable().into_iter().map(|(i, _)| i).collect::<Vec<_>>(), ["oecd-2024-report"]);
    // adopting the same bundle again changes nothing
    assert!(l.import_bundle(&golden, "x", &Budgets::default()).iter().all(|(_, s)| s.as_ref().is_ok_and(|s| s.outcome == Outcome::Unchanged)));
}

#[test]
fn a_source_that_may_not_travel_is_refused_again_at_adoption() {
    let (d, l) = lib();
    let src = d.path().join("b/sources/bad");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("meta.json"), r#"{"type":"book","title":"T","licence":"all-rights-reserved"}"#).unwrap();
    let r = l.import_bundle(&d.path().join("b"), "x", &Budgets::default());
    assert!(r[0].1.as_ref().unwrap_err().contains("refused"));
    assert!(l.list().is_empty());
}

#[tokio::test]
async fn capture_goes_through_the_fetcher_and_its_guards() {
    let (_d, l) = lib();
    // the SSRF guard: a loopback address is refused, and nothing is stored
    let e = l.add_url(&FetchPolicy::default(), "https://127.0.0.1/", &open("CC0-1.0"), &Budgets::default()).await.unwrap_err();
    assert!(e.contains("address refused"), "{}", e);
    assert!(l.list().is_empty());
    let addr = serve("<html><head><title>Valve guide</title></head><body><p>Turn it left.</p></body></html>");
    let policy = FetchPolicy::default().with_fixture_addrs([addr]);
    let url = format!("http://{}/guide#top", addr);
    let s = l.add_url(&policy, &url, &open("CC-BY-4.0"), &Budgets::default()).await.unwrap();
    assert_eq!((s.level, s.outcome.clone()), (Level::Full, Outcome::Added));
    let m = l.read_meta(&s.id).unwrap();
    assert_eq!((m["title"].as_str(), m["status"].as_str(), m["fetch_mode"].as_str()), (Some("Valve guide"), Some("not validated"), Some("operator")));
    assert!(m["content_hash"].as_str().unwrap().starts_with("sha256:") && m["attribution_text"].as_str().unwrap().contains("CC-BY-4.0"));
    assert!(std::fs::read_to_string(l.source_dir(&s.id).join("source.md")).unwrap().contains("Turn it left."));
    // an unknown licence is a pointer, with the hash and the date but no text
    let u = l.add_url(&policy, &format!("http://{}/other", addr), &AddOpts::default(), &Budgets::default()).await.unwrap();
    assert_eq!(u.level, Level::Pointer);
    assert!(!l.source_dir(&u.id).join("source.md").exists() && l.read_meta(&u.id).unwrap()["content_hash"].is_string());
    // a page over the text budget is kept as a pointer, and says why
    let tiny = Budgets { text: 20, ..Budgets::default() };
    let big = l.add_url(&policy, &format!("http://{}/big", addr), &open("CC0-1.0"), &tiny).await.unwrap();
    assert_eq!(big.level, Level::Pointer);
    assert!(big.notes.iter().any(|n| n.contains("text limit")), "{:?}", big.notes);
}
