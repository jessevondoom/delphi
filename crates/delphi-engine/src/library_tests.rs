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
    // licence is information, not a gate: only private is refused (specs/055 §11.1)
    for lic in ["all-rights-reserved", "internal", "unknown", "CC-BY-4.0"] {
        for c in Channel::ALL {
            assert!(may_leave(&json!({"licence": lic}), c).is_ok(), "{} {:?}", lic, c);
        }
    }
    // without the flag, a restricted licence cannot be kept whole
    let err = l.add_file(&f, &AddOpts { level: Some(Level::Full), ..open("unknown") }, &Budgets::default()).unwrap_err();
    assert!(err.contains("--private"), "{}", err);
}

#[test]
fn the_history_lists_every_capture_with_its_change_note_and_the_anchors_pointing_at_it() {
    let (d, l) = lib();
    let f = file(d.path(), "h.md", "# One\n\nfirst fact stands here");
    let s = l.add_file(&f, &open("own"), &Budgets::default()).unwrap();
    let spec = crate::library::claims::AnchorSpec { source_id: s.id.clone(), quote: "first fact stands here".into(), locator: Some("One".into()), ..Default::default() };
    l.add_claim("the first fact stands", &crate::library::claims::ClaimOpts::default(), &[spec]).unwrap();
    file(d.path(), "h.md", "# One\n\nfirst fact stands here\n\n# Two\n\nmore");
    l.add_file(&f, &open("own"), &Budgets::default()).unwrap();
    let h = l.history(&s.id).unwrap();
    assert_eq!(h.iter().map(|e| (e.version, e.current)).collect::<Vec<_>>(), [(1, false), (2, true)]);
    assert!(h[0].change_note.is_none(), "the first capture has nothing to differ from");
    assert!(h[1].change_note.as_deref().unwrap().contains("new sections: Two"));
    assert_eq!((h[0].anchors, h[1].anchors), (0, 1), "the anchor moved to the version that holds it");
    assert!(h[0].content_hash.is_some() && h[0].content_hash != h[1].content_hash);
    assert!(l.history("no-such-id").is_none());
    l.purge(&s.id).unwrap();
    assert_eq!(l.history(&s.id).unwrap().len(), 1, "a purge deletes the earlier captures");
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

fn origin(hash: &str) -> Origin {
    Origin { bundle: "acme/acme-platform".into(), hash: hash.into(), kid: Some("kid1".into()) }
}

fn entry(hash: &str, kid: &str) -> delphi_core::bundles::Entry {
    let lock: delphi_core::bundles::Lock = toml::from_str(&format!(
        "[[bundle]]\norg = \"acme\"\nname = \"acme-platform\"\nversion = \"1.4.0\"\nhash = \"{}\"\npublisher_kid = \"{}\"\nprovenance = \"signed\"\nadded_at = \"2026-01-01T00:00:00Z\"\norder = 10\nstatus = \"active\"\nreview_by = \"\"\ntitle = \"t\"\n[bundle.source]\nkind = \"directory\"\npath = \"/x\"\n",
        hash, kid
    ))
    .unwrap();
    lock.bundle.into_iter().next().unwrap()
}

/// A bundle directory holding one source per `(id, text)`.
fn bundle_with(root: &Path, sources: &[(&str, &str)]) -> PathBuf {
    let dir = root.join("b");
    for (id, text) in sources {
        let src = dir.join("sources").join(id);
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("meta.json"), r#"{"type":"book","title":"T","licence":"CC0-1.0","capture_level":"full"}"#).unwrap();
        std::fs::write(src.join("source.md"), text).unwrap();
    }
    dir
}

#[test]
fn a_bundles_sources_enter_with_the_licence_intact_and_are_vouched_only_by_the_lock() {
    let (_d, l) = lib();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dev/bundles/acme-platform");
    let r = l.import_bundle(&golden, "acme/acme-platform@1.4.0", &origin("h1"), &Budgets::default());
    assert_eq!(r.len(), 3);
    assert!(r.iter().all(|(_, s)| s.is_ok()), "{:?}", r.iter().map(|(i, s)| (i, s.as_ref().err())).collect::<Vec<_>>());
    let m = l.read_meta("oecd-2024-report").unwrap();
    assert_eq!((m["licence"].as_str(), m["capture_level"].as_str(), m["imported_from"].as_str()), (Some("CC-BY-4.0"), Some("full"), Some("acme/acme-platform@1.4.0")));
    assert_eq!((m["bundle_origin"]["bundle"].as_str(), m["bundle_origin"]["hash"].as_str(), m["bundle_origin"]["kid"].as_str()), (Some("acme/acme-platform"), Some("h1"), Some("kid1")));
    assert!(m["attribution_text"].as_str().unwrap().contains("CC BY 4.0") && m["share_alike"] == false);
    assert!(l.source_dir("oecd-2024-report").join("assets/figure-1.png").is_file());
    assert_eq!(l.read_meta("acme-style-guide").unwrap()["licence"], "own");
    assert_eq!(l.read_meta("acme-runbook-notes").unwrap()["licence"], "internal");
    let ids: Vec<String> = l.indexable().into_iter().map(|(i, _)| i).collect();
    assert!(ids.contains(&"oecd-2024-report".to_string()) && ids.contains(&"acme-runbook-notes".to_string()), "{:?}", ids);
    // vouched only while the lock holds the entry at the hash and key the content came in with
    assert_eq!(l.vouched_by(&m, &[entry("h1", "kid1")]).as_deref(), Some("acme"));
    assert_eq!(l.vouched_by(&m, &[]), None);
    assert_eq!(l.vouched_by(&m, &[entry("h2", "kid1")]), None, "a new hash");
    assert_eq!(l.vouched_by(&m, &[entry("h1", "kid2")]), None, "a new publisher key");
    // a source fetched any other way is never vouched
    let fetched = json!({"status": "validated", "licence": "CC0-1.0"});
    assert_eq!(l.vouched_by(&fetched, &[entry("h1", "kid1")]), None);
    // adopting the same bundle again changes nothing
    assert!(l.import_bundle(&golden, "x", &origin("h1"), &Budgets::default()).iter().all(|(_, s)| s.as_ref().is_ok_and(|s| s.outcome == Outcome::Unchanged)));
}

#[test]
fn a_bundles_own_meta_cannot_vouch_for_itself_and_private_is_refused() {
    let (d, l) = lib();
    let dir = bundle_with(d.path(), &[("a", "text")]);
    std::fs::write(dir.join("sources/a/meta.json"), r#"{"type":"book","title":"T","licence":"CC0-1.0","status":"validated","bundle_origin":{"bundle":"evil/x","hash":"h","kid":"k"},"cited_by":["c1"]}"#).unwrap();
    l.import_bundle(&dir, "x", &origin("h1"), &Budgets::default());
    let m = l.read_meta("a").unwrap();
    assert_eq!((m["status"].as_str(), m["bundle_origin"]["bundle"].as_str(), m["cited_by"].clone()), (Some("not validated"), Some("acme/acme-platform"), json!([])));
    assert_eq!(l.vouched_by(&m, &[]), None);
    let p = bundle_with(&d.path().join("p"), &[("secret", "text")]);
    std::fs::write(p.join("sources/secret/meta.json"), r#"{"type":"book","title":"T","private":true}"#).unwrap();
    let r = l.import_bundle(&p, "x", &origin("h1"), &Budgets::default());
    assert!(r[0].1.as_ref().unwrap_err().contains("private"));
    assert!(l.read_meta("secret").is_none());
}

#[test]
fn revoking_deletes_the_unused_keeps_the_cited_and_contests_their_claims_and_a_re_add_restores() {
    use crate::library::claims::{status, AnchorSpec, ClaimOpts};
    use delphi_core::knowledge_status::Status;
    let (d, l) = lib();
    let dir = bundle_with(d.path(), &[("used", "The valve opens at 4 bar."), ("idle", "Nothing cites this."), ("noted", "Marginal notes live here.")]);
    l.import_bundle(&dir, "x", &origin("h1"), &Budgets::default());
    let c = l.add_claim("The valve opens at 4 bar", &ClaimOpts::default(), &[AnchorSpec { source_id: "used".into(), locator: Some("§1".into()), quote: "opens at 4 bar".into(), ..Default::default() }]).unwrap();
    let cid = c["id"].as_str().unwrap().to_string();
    // the operator excerpted `noted` themselves
    std::fs::write(l.source_dir("noted").join("excerpts.json"), r#"[{"id":"e1","quote":"Marginal","bytes":8}]"#).unwrap();
    let before = status(&l.claim(&cid).unwrap());
    assert_ne!(before, Status::Contested);

    // a changed hash with the same key ends it for the old version
    let r = l.revoke_bundle("acme/acme-platform", Some(("h2", Some("kid1"))));
    assert_eq!((r.deleted.as_slice(), r.kept.len()), (&["idle".to_string()][..], 2), "{:?}", r);
    assert!(l.read_meta("idle").is_none() && l.read_meta("used").is_some());
    assert!(l.read_meta("used").unwrap()["bundle_origin"]["revoked"].is_string());
    assert_eq!(l.vouched_by(&l.read_meta("used").unwrap(), &[entry("h1", "kid1")]), None);
    assert_eq!(status(&l.claim(&cid).unwrap()), Status::Contested);

    // the same content comes back under the old hash: the label and the claim return
    l.import_bundle(&dir, "x", &origin("h1"), &Budgets::default());
    let used = l.read_meta("used").unwrap();
    assert!(used["bundle_origin"]["revoked"].is_null());
    assert_eq!(l.vouched_by(&used, &[entry("h1", "kid1")]).as_deref(), Some("acme"));
    assert_ne!(status(&l.claim(&cid).unwrap()), Status::Contested);

    // removal (no entry left) and a changed key end it the same way
    assert_eq!(l.revoke_bundle("acme/acme-platform", Some(("h1", Some("kid1")))), Revoked::default());
    let r = l.revoke_bundle("acme/acme-platform", None);
    assert_eq!((r.deleted.len(), r.kept.len()), (1, 2), "removal: the unused one goes, the cited and the excerpted stay: {:?}", r);
    assert_eq!(status(&l.claim(&cid).unwrap()), Status::Contested);
    let (_d2, l2) = lib();
    l2.import_bundle(&dir, "x", &origin("h1"), &Budgets::default());
    let r = l2.revoke_bundle("acme/acme-platform", Some(("h1", Some("other-key"))));
    assert_eq!(r.deleted.len() + r.kept.len(), 3, "a new key ends the vouching: {:?}", r);
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

fn xo(locator: &str) -> excerpts::ExcerptOpts {
    excerpts::ExcerptOpts { locator: locator.into(), purpose: "analysis".into(), ..Default::default() }
}

#[test]
fn excerpts_are_bounded_per_excerpt_per_restricted_source_and_per_page() {
    let (d, l) = lib();
    // a restricted work of 1,000 bytes may keep 10% (100 bytes) in total; several excerpts are fine within it
    let f = file(d.path(), "r.md", &"w".repeat(1000));
    let s = l.add_file(&f, &AddOpts { level: Some(Level::Excerpts), ..open("all-rights-reserved") }, &Budgets::default()).unwrap();
    assert_eq!(l.read_meta(&s.id).unwrap()["work_bytes"], 1000);
    l.add_excerpt(&s.id, &"a".repeat(60), &xo("p. 1")).unwrap();
    l.add_excerpt(&s.id, &"b".repeat(30), &xo("p. 2")).unwrap();
    let e = l.add_excerpt(&s.id, &"c".repeat(20), &xo("p. 3")).unwrap_err();
    assert!(e.contains("10% of the work or 16 KiB"), "{}", e);
    assert_eq!(l.excerpts(&s.id).len(), 2);
    // 16 KiB is the other bound: a 1 MB work allows 4 x 4 KiB and no more
    let big = file(d.path(), "big.md", &"w".repeat(1_000_000));
    let b = l.add_file(&big, &AddOpts { level: Some(Level::Excerpts), ..open("all-rights-reserved") }, &Budgets::default()).unwrap();
    for i in 0..4 {
        l.add_excerpt(&b.id, &"x".repeat(4096), &xo(&format!("s{}", i))).unwrap();
    }
    assert!(l.add_excerpt(&b.id, "one more", &xo("s5")).unwrap_err().contains("10% of the work or 16 KiB"));
    // 4 KiB per excerpt, even from an open source
    let o = l.add_file(&file(d.path(), "o.md", &format!("open short {}", "q".repeat(2000))), &open("CC0-1.0"), &Budgets::default()).unwrap();
    assert!(l.add_excerpt(&o.id, &"y".repeat(4097), &xo("all")).unwrap_err().contains("4096-byte limit"));
    // a locator and a purpose are required, and an unknown licence stays a pointer
    assert!(l.add_excerpt(&o.id, "open", &excerpts::ExcerptOpts::default()).is_err());
    let u = l.add_file(&file(d.path(), "u.md", "mystery"), &AddOpts::default(), &Budgets::default()).unwrap();
    assert!(l.add_excerpt(&u.id, "mystery", &xo("x")).unwrap_err().contains("unknown"));
    // a quotation on a page is at most 1 KiB
    l.add_excerpt(&o.id, &"q".repeat(2000), &xo("long")).unwrap();
    l.add_excerpt(&o.id, "short", &xo("brief")).unwrap();
    assert!(l.page_quotation(&o.id, "e1").unwrap_err().contains("1024"));
    assert_eq!(l.page_quotation(&o.id, "e2").unwrap(), "short");
    assert!(excerpts::page_quotation(&"q".repeat(1024)).is_ok());
}

#[test]
fn an_anchor_is_found_again_by_its_quote_or_by_its_context_and_never_re_pointed() {
    let doc = "Intro text.\n\nThe valve must be turned left until it stops. Then wait.\n\nOther text. The valve must be turned left until it stops. Done.";
    let a = excerpts::Anchor::new("s", "1", "sec 2", "The valve must be turned left until it stops.", Some(doc), None, None).unwrap();
    assert_eq!((a.start, a.prefix.ends_with("\n\n"), a.suffix.starts_with(" Then")), (Some(13), true, true));
    // the same text moved down and a copy added before it: the occurrence with the same surroundings wins
    let moved = format!("A new paragraph here.\n\nThe valve must be turned left until it stops. Rest.\n\n{}", doc);
    let r = a.resolve(&moved);
    let excerpts::Resolution::Found { start, how, .. } = r else { panic!("{:?}", r) };
    assert_eq!(how, excerpts::How::Exact);
    assert!(moved.chars().skip(start).collect::<String>().starts_with("The valve must be turned left until it stops. Then wait."));
    // the quote was reworded slightly, the surroundings are intact: found by prefix and suffix
    let reworded = "Intro text.\n\nThe valve must be turned left until it fully stops. Then wait.\n\nOther.";
    assert!(matches!(a.resolve(reworded), excerpts::Resolution::Found { how: excerpts::How::Context, .. }));
    // the claim was replaced by different text between the same neighbours: not found, not re-pointed
    let replaced = "Intro text.\n\nNever touch the red lever under any circumstance whatsoever. Then wait.\n\nOther.";
    assert_eq!(a.resolve(replaced), excerpts::Resolution::NotFound);
    // a quote that is not in the text cannot be anchored
    assert!(excerpts::Anchor::new("s", "1", "x", "absent", Some(doc), None, None).is_err());
}

#[tokio::test]
async fn a_new_version_re_resolves_the_excerpts_and_marks_the_ones_it_cannot_find() {
    let (_d, l) = lib();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let h = hits.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            // a capture is two fetches (daemon and browser): 0-1 the first page, 2-3 a changed page, then gone
            let n = h.fetch_add(1, std::sync::atomic::Ordering::SeqCst) / 2;
            let (status, body) = match n {
                0 => ("200 OK", "<html><head><title>Guide</title></head><body><p>Turn it left. Open the red valve. Close the door.</p></body></html>"),
                1 => ("200 OK", "<html><head><title>Guide</title></head><body><p>Intro added. Turn it left. Close the door.</p></body></html>"),
                _ => ("404 Not Found", "gone"),
            };
            let r = format!("HTTP/1.1 {}\r\nConnection: close\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}", status, body.len(), body);
            let _ = s.write_all(r.as_bytes());
        }
    });
    let policy = FetchPolicy::default().with_fixture_addrs([addr]);
    let url = format!("http://{}/guide", addr);
    let s = l.add_url(&policy, &url, &open("CC0-1.0"), &Budgets::default()).await.unwrap();
    l.add_excerpt(&s.id, "Turn it left.", &xo("para 1")).unwrap();
    l.add_excerpt(&s.id, "Open the red valve.", &xo("para 1")).unwrap();
    assert_eq!(l.excerpts(&s.id).len(), 2, "more than one excerpt per source");
    let day = 86_400;
    let t0 = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap();
    // within 30 days: nothing is fetched
    assert_eq!(l.recheck_at(&policy, &s.id, &Budgets::default(), t0 + 29 * day).await.unwrap(), Recheck::Fresh);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 2);
    // after 30 days: the fresh fetch differs, so version 2; the old one is kept and the anchors are re-resolved
    assert_eq!(l.recheck_at(&policy, &s.id, &Budgets::default(), t0 + 31 * day).await.unwrap(), Recheck::Changed(2));
    assert!(l.source_dir(&s.id).join("versions/1/source.md").is_file());
    let ex = l.excerpts(&s.id);
    assert_eq!((ex[0]["anchor"]["version"].as_str(), ex[0]["contested"].is_null()), (Some("2"), true));
    assert_eq!((ex[1]["anchor"]["version"].as_str(), ex[1]["contested"] == true), (Some("1"), true), "the vanished quote is contested and keeps its old anchor");
    assert!(l.source_dir(&s.id).join("versions/1/excerpts.json").is_file());
    // gone: the capture and the claims' anchors stay, the source is retired
    assert_eq!(l.recheck_at(&policy, &s.id, &Budgets::default(), t0 + 80 * day).await.unwrap(), Recheck::Retired);
    assert!(l.read_meta(&s.id).unwrap()["retired"].is_string() && l.source_dir(&s.id).join("source.md").is_file());
    // a file has no URL to check
    let f = file(_d.path(), "f.md", "text");
    let fs = l.add_file(&f, &open("CC0-1.0"), &Budgets::default()).unwrap();
    assert!(matches!(l.recheck_at(&policy, &fs.id, &Budgets::default(), t0 + 90 * day).await.unwrap(), Recheck::NotApplicable(_)));
}

#[test]
fn a_bibliography_has_citation_fields_only_and_a_private_source_is_never_exported() {
    let (d, l) = lib();
    let o = AddOpts { title: Some("A Study of Valves".into()), rights_holder: Some("Ada Lovelace".into()), ..open("CC-BY-4.0") };
    let s = l.add_file(&file(d.path(), "a.md", "valve text"), &o, &Budgets::default()).unwrap();
    let mut m = l.read_meta(&s.id).unwrap();
    m["author"] = json!([{"family": "Lovelace", "given": "Ada Augusta"}, {"family": "Babbage", "given": "Charles"}]);
    m["issued"] = json!({"date-parts": [[2021, 3, 9]]});
    m["container-title"] = json!("Journal of Valves");
    m["type"] = json!("article-journal");
    m["volume"] = json!("12");
    m["issue"] = json!("3");
    m["page"] = json!("45-67");
    m["DOI"] = json!("10.1234/valves.5");
    std::fs::write(l.source_dir(&s.id).join("meta.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    let secret = file(d.path(), "p.md", "schematic of the secret thing");
    let p = l.add_file(&secret, &AddOpts { level: Some(Level::Full), private: true, title: Some("Secret Schematic".into()), ..open("all-rights-reserved") }, &Budgets::default()).unwrap();
    let get = |f: bib::Format| bib::export(&l, &[], f).unwrap();
    let apa = get(bib::Format::Apa);
    assert_eq!(apa.text.trim(), "Lovelace, A. A., & Babbage, C. (2021). A Study of Valves. Journal of Valves, 12(3), 45-67. https://doi.org/10.1234/valves.5");
    assert_eq!(apa.withheld, 1);
    assert_eq!(get(bib::Format::Mla).text.trim(), "Lovelace, Ada Augusta, and Charles Babbage. \u{201c}A Study of Valves.\u{201d} Journal of Valves, vol. 12, no. 3, 9 Mar. 2021, pp. 45-67, https://doi.org/10.1234/valves.5.");
    assert!(get(bib::Format::Chicago).text.starts_with("Lovelace, Ada Augusta, and Charles Babbage. \u{201c}A Study of Valves.\u{201d} Journal of Valves 12, no. 3. 2021."));
    assert!(get(bib::Format::Plain).text.contains("9 March 2021"));
    let bibtex = get(bib::Format::BibTex).text;
    assert!(bibtex.contains("@article{") && bibtex.contains("author = {Lovelace, Ada Augusta and Babbage, Charles}") && bibtex.contains("journal = {Journal of Valves}"), "{}", bibtex);
    let csl: Value = serde_json::from_str(&get(bib::Format::CslJson).text).unwrap();
    assert_eq!(csl.as_array().unwrap().len(), 1);
    assert_eq!((csl[0]["DOI"].as_str(), csl[0]["licence"].is_null(), csl[0]["content_hash"].is_null()), (Some("10.1234/valves.5"), true, true), "CSL fields only");
    // the private source is in no format, not even when named
    for f in [bib::Format::Apa, bib::Format::Mla, bib::Format::Chicago, bib::Format::Plain, bib::Format::BibTex, bib::Format::CslJson] {
        let x = bib::export(&l, std::slice::from_ref(&p.id), f).unwrap();
        assert!(x.text.trim().is_empty() || x.text.trim() == "[]", "{:?}", x.text);
        assert!(!get(f).text.contains("Secret"));
        assert_eq!(x.withheld, 1);
    }
    assert!(bib::export(&l, &["nope".into()], bib::Format::Apa).is_err());
}

#[test]
fn metadata_is_read_by_code_for_wikipedia_dois_and_gutenberg() {
    let h = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
    let w = meta::read("https://en.wikipedia.org/wiki/Valve", &[("etag".into(), "W/\"1234567/abc-def\"".into()), ("last-modified".into(), "Tue, 15 Nov 2022 08:12:31 GMT".into())], "", "", None);
    assert_eq!((w.fields["licence"].as_str(), w.fields["edition"].as_str(), w.fields["language"].as_str()), (Some("CC-BY-SA-4.0"), Some("Revision 1234567"), Some("en")));
    assert_eq!(w.fields["issued"], json!({"date-parts": [[2022, 11, 15]]}));
    assert!(meta::read("https://en.wikipedia.org/wiki/Valve", &h("x", "y"), "", "", None).notes[0].contains("no revision"));
    let html = r#"<html><head><meta name="citation_title" content="Flow &amp; Valves"><meta name="citation_author" content="Lovelace, Ada"><meta name="citation_author" content="Charles Babbage"><meta content="Journal of Valves" name="citation_journal_title"><meta name="citation_publication_date" content="2021/03/09"><meta name="citation_firstpage" content="45"><meta name="citation_lastpage" content="67"></head></html>"#;
    let d = meta::read("https://doi.org/10.1234/v.5", &[], html, "", None);
    assert_eq!((d.fields["title"].as_str(), d.fields["type"].as_str(), d.fields["page"].as_str(), d.fields["DOI"].as_str()), (Some("Flow & Valves"), Some("article-journal"), Some("45-67"), Some("10.1234/v.5")));
    assert_eq!(d.fields["author"], json!([{"family": "Lovelace", "given": "Ada"}, {"family": "Babbage", "given": "Charles"}]));
    assert_eq!(d.fields["issued"], json!({"date-parts": [[2021, 3, 9]]}));
    assert!(!d.fields.contains_key("licence"), "a DOI page does not establish a licence");
    let g = meta::read("https://www.gutenberg.org/cache/epub/1342/pg1342.txt", &[], "", "The Project Gutenberg eBook\n\nTitle: Pride and Prejudice\nAuthor: Austen, Jane\nLanguage: English\n", None);
    assert_eq!((g.fields["title"].as_str(), g.fields["collection-number"].as_str()), (Some("Pride and Prejudice"), Some("1342")));
    assert_eq!(g.fields["author"], json!([{"family": "Austen", "given": "Jane"}]));
    assert!(!g.fields.contains_key("licence"));
    assert!(meta::read("https://example.com/x", &[], "<meta name=\"citation_title\" content=\"x\">", "", None).fields.is_empty());
}

mod claims {
    use super::*;
    use crate::library::claims::{status, AnchorSpec, ClaimOpts, Validator};
    use delphi_core::knowledge_status::Status;

    fn spec(id: &str, quote: &str) -> AnchorSpec {
        AnchorSpec { source_id: id.into(), locator: Some("§1".into()), quote: quote.into(), ..Default::default() }
    }

    fn source(d: &Path, l: &Library, name: &str, text: &str) -> String {
        l.add_file(&file(d, name, text), &open("CC0-1.0"), &Budgets::default()).unwrap().id
    }

    const BODY: &str = "# Tides\n\nThe tide comes in twice a day, driven mostly by the moon.\n\nSpring tides are the highest.\n";

    #[test]
    fn status_is_derived_never_declared_and_cited_by_is_kept() {
        let (d, l) = lib();
        let s = source(d.path(), &l, "t.md", BODY);
        let forged = ClaimOpts { kind: Some("fact".into()), ..Default::default() };
        let c = l.add_claim("Tides come twice a day.", &forged, &[spec(&s, "twice a day")]).unwrap();
        let id = c["id"].as_str().unwrap().to_string();
        assert_eq!(status(&c), Status::Noted, "a new claim reads not validated");
        assert_eq!(l.read_meta(&s).unwrap()["cited_by"], json!([id]), "the source records who cites it");
        // a hand-edited record cannot declare itself validated
        let mut edited = c.clone();
        edited["status"] = json!("validated");
        edited["lifecycle"] = json!("active");
        assert_eq!(status(&edited), Status::Noted);
        assert!(l.add_claim("x", &ClaimOpts::default(), &[spec(&s, "not in the text")]).is_err(), "a quote that is not in the source is refused");
        let v = l.validate_claim(&id, Validator::Operator).unwrap();
        assert_eq!(status(&v), Status::Validated);
        let op = ClaimOpts { kind: Some("attributed".into()), ..Default::default() };
        let a = l.add_claim("Tides are caused by the moon, says the author.", &op, &[spec(&s, "the moon")]).unwrap();
        assert!(l.validate_claim(a["id"].as_str().unwrap(), Validator::Operator).is_err());
        let bare = l.add_claim("An operator observation.", &ClaimOpts::default(), &[]).unwrap();
        assert!(l.validate_claim(bare["id"].as_str().unwrap(), Validator::Operator).is_err(), "no anchor, no validation");
    }

    #[test]
    fn a_new_version_moves_found_anchors_and_contests_a_claim_whose_anchor_is_gone() {
        let (d, l) = lib();
        let s = source(d.path(), &l, "t.md", BODY);
        let kept = l.add_claim("Tides come twice a day.", &ClaimOpts::default(), &[spec(&s, "twice a day")]).unwrap()["id"].as_str().unwrap().to_string();
        let gone = l.add_claim("Spring tides are highest.", &ClaimOpts::default(), &[spec(&s, "Spring tides are the highest.")]).unwrap()["id"].as_str().unwrap().to_string();
        l.validate_claim(&gone, Validator::Operator).unwrap();
        let next = BODY.replace("Spring tides are the highest.", "Neap tides are the lowest.").replace("# Tides", "# Tides, revised");
        let st = l.add_file(&file(d.path(), "t.md", &next), &open("CC0-1.0"), &Budgets::default()).unwrap();
        assert_eq!(st.outcome, Outcome::NewVersion(2));
        let k = l.claim(&kept).unwrap();
        assert_eq!(k["anchors"][0]["version"], "2");
        assert_eq!(status(&k), Status::Noted);
        let g = l.claim(&gone).unwrap();
        assert_eq!(status(&g), Status::Contested, "a validated claim whose anchor cannot be found is lowered");
        assert_eq!(g["anchors"][0]["state"], "lost");
        assert_eq!(g["anchors"][0]["version"], "1", "the old anchor stays pointed at the old version");
        assert!(l.source_dir(&s).join("versions/1/source.md").is_file());
    }

    #[test]
    fn retiring_a_source_contests_only_the_claims_it_alone_supports_and_reviving_clears_it() {
        let (d, l) = lib();
        let a = source(d.path(), &l, "a.md", BODY);
        let b = source(d.path(), &l, "b.md", &BODY.replace("Tides", "Ocean tides"));
        let sole = l.add_claim("Tides come twice a day.", &ClaimOpts::default(), &[spec(&a, "twice a day")]).unwrap()["id"].as_str().unwrap().to_string();
        let both = l.add_claim("The moon drives tides.", &ClaimOpts::default(), &[spec(&a, "the moon"), spec(&b, "the moon")]).unwrap()["id"].as_str().unwrap().to_string();
        let mut m = l.read_meta(&a).unwrap();
        m["retired"] = json!("now");
        std::fs::write(l.source_dir(&a).join("meta.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        l.source_retired(&a);
        assert_eq!(status(&l.claim(&sole).unwrap()), Status::Contested);
        assert_eq!(status(&l.claim(&both).unwrap()), Status::Noted);
        l.source_revived(&a);
        assert_eq!(status(&l.claim(&sole).unwrap()), Status::Noted);
    }

    #[test]
    fn a_purge_makes_anchors_pointer_only_and_a_claim_another_source_supports_keeps_its_status() {
        let (d, l) = lib();
        let a = source(d.path(), &l, "a.md", BODY);
        let b = source(d.path(), &l, "b.md", &BODY.replace("Tides", "Ocean tides"));
        let sole = l.add_claim("Tides come twice a day.", &ClaimOpts::default(), &[spec(&a, "twice a day")]).unwrap()["id"].as_str().unwrap().to_string();
        let both = l.add_claim("The moon drives tides.", &ClaimOpts::default(), &[spec(&a, "the moon"), spec(&b, "the moon")]).unwrap()["id"].as_str().unwrap().to_string();
        for id in [&sole, &both] {
            l.validate_claim(id, Validator::Operator).unwrap();
        }
        l.purge(&a).unwrap();
        let s = l.claim(&sole).unwrap();
        assert_eq!(status(&s), Status::Noted, "pointer-only reads not validated");
        assert_eq!(s["anchors"][0]["state"], "pointer_only");
        assert_eq!(s["anchors"][0]["quote"], "", "no text of the purged capture is kept");
        assert!(s["anchors"][0]["quote_hash"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(status(&l.claim(&both).unwrap()), Status::Validated);
    }

    #[test]
    fn mission_start_pulls_pointers_without_text_ranked_and_records_reliance() {
        let (d, l) = lib();
        let s = source(d.path(), &l, "t.md", BODY);
        let v = l.add_claim("Tides come twice a day.", &ClaimOpts::default(), &[spec(&s, "twice a day")]).unwrap()["id"].as_str().unwrap().to_string();
        l.validate_claim(&v, Validator::Operator).unwrap();
        let n = l.add_claim("Tides are strong near the harbour.", &ClaimOpts::default(), &[]).unwrap()["id"].as_str().unwrap().to_string();
        let proj = ClaimOpts { scope: Some("project".into()), project: Some("harbour".into()), ..Default::default() };
        let p = l.add_claim("Tides close the harbour gate.", &proj, &[]).unwrap()["id"].as_str().unwrap().to_string();
        l.add_claim("Unrelated gardening note.", &ClaimOpts::default(), &[]).unwrap();
        let ptrs = l.pull_for_mission("what do we know about tides", None, "m1");
        let ids: Vec<&str> = ptrs.iter().map(|p| p.uri.trim_start_matches("@library/claim/")).collect();
        assert_eq!(ids, [v.as_str(), n.as_str()], "validated first; the project claim and the unrelated one are not offered");
        assert!(ptrs.iter().all(|p| !p.reason.contains("twice a day") && !p.reason.contains("harbour")), "pointers carry no claim text");
        assert!(ptrs[0].reason.contains("validated") && ptrs[1].reason.contains("not validated"));
        assert_eq!(l.claim(&v).unwrap()["cited_by"], json!(["m1"]));
        l.pull_for_mission("tides again", None, "m1");
        assert_eq!(l.claim(&v).unwrap()["cited_by"], json!(["m1"]), "once per mission");
        let ids: Vec<String> = l.pull("tides", Some("harbour"), 5).into_iter().map(|p| p.id).collect();
        assert!(ids.contains(&p), "a project claim is offered to its project");
        let slice = l.claim_slice(&ptrs[0].uri).unwrap();
        assert!(slice.contains("validated") && slice.contains("untrusted data") && slice.contains("Tides come twice a day."));
        assert!(!slice.contains("twice a day\n"), "no quote of the source");
    }

    #[test]
    fn reliability_counts_what_stood_and_what_was_contested() {
        let (d, l) = lib();
        let s = source(d.path(), &l, "t.md", BODY);
        l.add_claim("Tides come twice a day.", &ClaimOpts::default(), &[spec(&s, "twice a day")]).unwrap();
        l.add_claim("Spring tides are highest.", &ClaimOpts::default(), &[spec(&s, "Spring tides are the highest.")]).unwrap();
        l.add_file(&file(d.path(), "t.md", &BODY.replace("Spring tides are the highest.", "x")), &open("CC0-1.0"), &Budgets::default()).unwrap();
        assert_eq!(l.reliability_of_source(&s), crate::library::claims::Reliability { claims: 2, stood: 1, contested: 1, disproved: 0 });
    }
}

mod pages {
    use super::*;
    use crate::library::bib::Format;
    use crate::library::claims::{status, AnchorSpec, ClaimOpts, Validator};
    use crate::library::pages::{Finding, Scope, NOTED_HEADING, NO_SOURCES, TABLE_HEADING};
    use delphi_core::knowledge_status::Status;

    const BODY: &str = "# Tides\n\nThe tide comes in twice a day, driven mostly by the moon.\n\nSpring tides are the highest.\n";

    fn setup() -> (tempfile::TempDir, Library, String, String) {
        let (d, l) = lib();
        let s = l.add_file(&file(d.path(), "t.md", BODY), &open("CC0-1.0"), &Budgets::default()).unwrap().id;
        let spec = |q: &str| AnchorSpec { source_id: s.clone(), locator: Some("§1".into()), quote: q.into(), ..Default::default() };
        let a = l.add_claim("Tides come twice a day.", &ClaimOpts::default(), &[spec("twice a day")]).unwrap()["id"].as_str().unwrap().to_string();
        l.validate_claim(&a, Validator::Operator).unwrap();
        let b = l.add_claim("Spring tides are the highest tides.", &ClaimOpts::default(), &[]).unwrap()["id"].as_str().unwrap().to_string();
        (d, l, a, b)
    }

    fn src(l: &Library) -> String {
        l.list().into_iter().find(|(_, m)| m["title"] == "t.md").map(|(id, _)| id).unwrap()
    }

    fn page_of(l: &Library, body: &str) -> String {
        l.build_page("tides", Some(body), Format::Plain).unwrap().markdown
    }

    #[test]
    fn a_built_page_has_the_banner_the_noted_heading_and_a_table_row_per_claim_and_passes_its_own_check() {
        let (_d, l, a, b) = setup();
        let p = l.build_page("tides", None, Format::Plain).unwrap();
        assert!(p.markdown.starts_with("Generated page. Not authoritative. Written by the writer role on "));
        let (head, tail) = p.markdown.split_once(&format!("## {}", NOTED_HEADING)).expect("noted heading");
        assert!(head.contains(&format!("[{} ● validated]", a)) && !head.contains(&b));
        assert!(tail.contains(&format!("[{} ○ not validated]", b)));
        let (_, table) = p.markdown.split_once(&format!("## {}", TABLE_HEADING)).expect("table");
        assert!(table.contains(&format!("| {} | ● validated |", a)) && table.contains(&format!("| {} | ○ not validated |", b)) && table.contains("§1"));
        let c = l.check_page_text(&p.markdown, Scope::External);
        assert!(c.findings.is_empty(), "{:?}", c.findings);
        let side: Value = serde_json::from_slice(&std::fs::read(l.root().join("pages").join(format!("tides-{}.json", p.hash))).unwrap()).unwrap();
        assert_eq!(side["status"], "generated");
    }

    #[test]
    fn the_banner_and_the_table_are_the_daemons_and_a_page_with_no_citation_says_so() {
        let (_d, l, a, _b) = setup();
        let md = page_of(&l, &format!("Tides come twice a day [{a}]."));
        let ok = |t: &str| l.check_page_text(t, Scope::External).findings;
        assert!(ok(&md).is_empty());
        let rest = md.split_once("\n\n").unwrap().1;
        assert!(ok(rest).contains(&Finding::BannerMissing));
        assert!(matches!(ok(&md.replace("Not authoritative", "Authoritative"))[..], [Finding::BannerAltered(_)]));
        assert!(ok(&md.replace("Read the cited sources before relying on it.", "Trust it.")).iter().any(|f| matches!(f, Finding::BannerAltered(_))));
        assert!(ok(md.split_once(&format!("## {}", TABLE_HEADING)).unwrap().0).contains(&Finding::TableMissing));
        assert!(ok(&md.replace(&format!("| {} |", a), "| gone |")).contains(&Finding::TableRowMissing(a.clone())));
        let banner = md.lines().next().unwrap();
        assert_eq!(l.build_page("tides", Some(&format!("{banner}\n\nTides [{a}].")), Format::Plain).unwrap_err(), vec![Finding::BannerInBody]);
        assert_eq!(l.build_page("tides", Some(&format!("Tides [{a}].\n\n## {TABLE_HEADING}\n")), Format::Plain).unwrap_err(), vec![Finding::BannerInBody]);
        let none = page_of(&l, "Tides are interesting.");
        assert!(none.lines().next().unwrap().contains("unsupported") && none.contains(&format!("| {} |", NO_SOURCES)));
        assert!(ok(&none).is_empty());
        assert!(!ok(&none.replace("unsupported", "fine")).is_empty());
    }

    #[test]
    fn one_marker_supports_a_paragraph_and_a_missing_retired_or_disproved_record_fails() {
        let (_d, l, a, b) = setup();
        let s = src(&l);
        let many = format!("The tide comes in twice a day. It is driven by the moon, and the sun too. Nothing else is claimed here. [{a}]");
        l.build_page("tides", Some(&many), Format::Plain).unwrap();
        l.build_page("tides", Some(&format!("A source says so. [s-{s}]")), Format::Plain).unwrap();
        let err = |body: &str| l.build_page("tides", Some(body), Format::Plain).unwrap_err();
        assert_eq!(err("Tides [c-0000]."), vec![Finding::UnknownRecord("c-0000".into())]);
        assert_eq!(err("Tides [s-nope]."), vec![Finding::UnknownRecord("s-nope".into())]);
        assert!(matches!(err(&format!("Tides [{a} ○ not validated]"))[..], [Finding::StatusMisstated { .. }]), "a marker cannot misstate its status");
        l.build_page("tides", Some(&format!("Tides [{b} ○ not validated].")), Format::Plain).unwrap();
        let mut c = l.claim(&b).unwrap();
        c["lifecycle"] = json!("disproved");
        l.save_claim(&c).unwrap();
        assert!(matches!(err(&format!("Tides [{b}]."))[..], [Finding::NotCitable { .. }]));
        let mut m = l.read_meta(&s).unwrap();
        m["retired"] = json!("2026-10-29T00:00:00Z");
        l.write_json(&l.source_dir(&s).join("meta.json"), &m).unwrap();
        assert!(matches!(err(&format!("Tides [s-{s}]."))[..], [Finding::NotCitable { .. }]));
    }

    #[test]
    fn a_quotation_must_be_verbatim_in_a_cited_source_and_a_quote_block_must_cite_one() {
        let (_d, l, a, _b) = setup();
        let s = src(&l);
        l.build_page("tides", Some(&format!("The page says \"driven mostly by the moon\". [s-{s}]")), Format::Plain).unwrap();
        l.build_page("tides", Some(&format!("> The tide comes in twice\n> a day, driven mostly by the moon. [s-{s}]")), Format::Plain).unwrap();
        let err = |body: &str| l.build_page("tides", Some(body), Format::Plain).unwrap_err();
        assert!(matches!(err(&format!("The page says \"driven entirely by the sun\". [s-{s}]"))[..], [Finding::QuoteNotFound { .. }]));
        assert!(matches!(err(&format!("> Spring tides are the lowest. [s-{s}]"))[..], [Finding::QuoteNotFound { .. }]));
        assert!(matches!(err("> The tide comes in twice a day.")[..], [Finding::QuoteUnattributed { .. }]));
        assert!(matches!(err(&format!("> {} [s-{s}]", "tide ".repeat(300)))[..], [Finding::QuoteTooLong { .. }]));
        l.build_page("tides", Some(&format!("A scare \"quote\" and \"two words\" are not checked. [{a}]")), Format::Plain).unwrap();
        std::fs::write(l.source_dir(&s).join("source.md"), "# Tides\n\nChanged after capture.\n").unwrap();
        assert!(err(&format!("The page says \"driven mostly by the moon\". [s-{s}]")).contains(&Finding::SourceAltered(s.clone())));
    }

    #[test]
    fn tables_images_and_links_must_be_known_to_the_library() {
        let (d, l, a, _b) = setup();
        let s = src(&l);
        let err = |body: &str| l.build_page("tides", Some(body), Format::Plain).unwrap_err();
        let table = "| Day | Tides |\n| --- | --- |\n| Mon | 2 |";
        assert_eq!(err(table), vec![Finding::TableUncited]);
        l.build_page("tides", Some(&format!("{table}\n\nTable 1: tides per day. [{a}]")), Format::Plain).unwrap();
        assert!(matches!(err(&format!("![chart](lib:{s}/chart.png) [s-{s}]"))[..], [Finding::UnknownImage(_)]));
        assert!(matches!(err(&format!("![chart](https://example.org/c.png) [s-{s}]"))[..], [Finding::UnknownImage(_)]));
        let assets = d.path().join("assets-in");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("chart.png"), b"png").unwrap();
        std::fs::create_dir_all(l.source_dir(&s).join("assets")).unwrap();
        std::fs::copy(assets.join("chart.png"), l.source_dir(&s).join("assets").join("chart.png")).unwrap();
        l.build_page("tides", Some(&format!("![Tide chart](lib:{s}/chart.png)\n\nFigure: tide chart. [s-{s}]")), Format::Plain).unwrap();
        assert_eq!(err(&format!("![Tide chart](lib:{s}/chart.png) [{a}]")), vec![Finding::ImageUncited(s.clone())]);
        assert_eq!(err(&format!("![x](lib:{s}/../meta.json) [s-{s}]")), vec![Finding::UnknownImage(format!("lib:{s}/../meta.json"))]);
        assert_eq!(err(&format!("See [the tides](https://example.org/tides). [s-{s}]")), vec![Finding::UnknownLink("https://example.org/tides".into())]);
        let mut m = l.read_meta(&s).unwrap();
        m["URL"] = json!("https://example.org/tides/");
        l.write_json(&l.source_dir(&s).join("meta.json"), &m).unwrap();
        l.build_page("tides", Some(&format!("See [the tides](https://example.org/tides). [s-{s}]")), Format::Plain).unwrap();
    }

    #[test]
    fn numbers_and_names_in_no_material_are_flagged_not_rejected() {
        let (_d, l, a, _b) = setup();
        let p = l.build_page("tides", Some(&format!("Tides come twice a day, said Jacques Cousteau in 1953, with 14 metres at Fundy. [{a}]")), Format::Plain).unwrap();
        assert!(p.flags.contains(&"1953".to_string()) && p.flags.contains(&"14".to_string()) && p.flags.contains(&"Jacques Cousteau".to_string()) && p.flags.contains(&"Fundy".to_string()), "{:?}", p.flags);
        assert!(!p.flags.iter().any(|f| f == "Tides" || f == "twice"));
    }

    #[test]
    fn a_page_is_rebuilt_when_a_claim_changes_and_cached_when_it_does_not() {
        let (_d, l, a, _b) = setup();
        assert!(!l.build_page("tides", None, Format::Plain).unwrap().cached);
        assert!(l.build_page("tides", None, Format::Plain).unwrap().cached);
        l.dispute_claim(&a, "the source says otherwise in v3").unwrap();
        let p = l.build_page("tides", None, Format::Plain).unwrap();
        assert!(!p.cached && p.validated.is_empty());
        assert!(p.markdown.contains("◆"), "a contested claim shows its own glyph under the noted heading");
    }

    #[test]
    fn dispute_files_a_contested_mark_with_the_note_and_blocks_validation() {
        let (_d, l, a, _b) = setup();
        assert!(l.dispute_claim(&a, "  ").is_err());
        let c = l.dispute_claim(&a, "wrong\nsince 2020").unwrap();
        assert_eq!(c["contested_by"][0]["note"], "wrong since 2020");
        assert_eq!(status(&c), Status::Contested);
        assert!(l.validate_claim(&a, Validator::Operator).is_err());
        assert!(l.dispute_claim("c-zzzz", "x").is_err());
    }

    #[test]
    fn a_claim_resting_on_a_private_source_never_reaches_a_page() {
        let (d, l, _a, _b) = setup();
        let p = l.add_file(&file(d.path(), "p.md", "# Secret\n\nThe vault code is long.\n"), &AddOpts { private: true, ..open("own") }, &Budgets::default()).unwrap().id;
        let spec = AnchorSpec { source_id: p, locator: Some("§1".into()), quote: "vault code".into(), ..Default::default() };
        let c = l.add_claim("The vault code is long.", &ClaimOpts::default(), &[spec]).unwrap()["id"].as_str().unwrap().to_string();
        let page = l.build_page("vault", None, Format::Plain).unwrap();
        assert!(!page.markdown.contains("vault code") && page.withheld == 1);
        let body = format!("The vault code is long [{}].", c);
        assert!(matches!(l.build_page("vault", Some(&body), Format::Plain).unwrap_err()[..], [Finding::NotCitable { .. }]));
        let internal = l.build_page_scoped("vault", Some(&body), Format::Plain, Scope::Internal).unwrap();
        assert!(internal.markdown.contains(&format!("| {} |", c)) && !internal.markdown.contains("§1\n"));
        assert!(!l.check_page_text(&internal.markdown, Scope::External).findings.is_empty());
    }
}

mod export {
    use super::*;
    use crate::library::excerpts::ExcerptOpts;

    #[test]
    fn a_copy_carries_citation_and_licence_but_no_status_note_or_purge_survivor() {
        let (d, l) = lib();
        let s = l.add_file(&file(d.path(), "w.md", "# W\n\nBody text.\n"), &open("CC-BY-4.0"), &Budgets::default()).unwrap().id;
        l.add_excerpt(&s, "Body text", &ExcerptOpts { locator: "§1".into(), purpose: "my private reason".into(), ..Default::default() }).unwrap();
        let bundle = tempfile::tempdir().unwrap();
        let r = l.export_to_bundle(bundle.path(), &[s.clone(), "nope".into()], false);
        assert!(r[0].1.is_ok() && r[1].1.is_err());
        let dir = bundle.path().join("sources").join(&s);
        assert!(!std::fs::read_to_string(dir.join("excerpts.json")).unwrap().contains("private reason"));
        let meta: Value = serde_json::from_slice(&std::fs::read(dir.join("meta.json")).unwrap()).unwrap();
        assert_eq!(meta["licence"], "CC-BY-4.0");
        l.purge(&s).unwrap();
        let again = l.export_to_bundle(bundle.path(), std::slice::from_ref(&s), true);
        let e = again[0].1.as_ref().unwrap();
        assert_eq!((e.level, e.text_bytes), (Level::Pointer, 0), "a purged capture leaves no text to copy");
        assert!(!bundle.path().join("sources").join(&s).join("source.md").exists());
    }
}

mod page_validation {
    use super::*;
    use crate::fact_judge::TokenLedger;
    use crate::library::bib::Format;
    use crate::library::claims::{status, AnchorSpec, ClaimOpts, Validator};
    use crate::library::page_validate::*;
    use crate::library::page_validate::Outcome;
    use crate::library::pages::Scope;
    use delphi_core::knowledge_status::Status;
    use std::sync::Mutex;

    struct Fake {
        replies: Mutex<Vec<Result<Vec<Assertion>, String>>>,
        seen: Mutex<Vec<PageQuery>>,
    }
    impl Fake {
        fn new(r: Vec<Result<Vec<Assertion>, String>>) -> Fake {
            Fake { replies: Mutex::new(r), seen: Mutex::new(vec![]) }
        }
    }
    impl PageValidator for Fake {
        fn validate<'a>(&'a self, q: &'a PageQuery) -> ValidatorFuture<'a> {
            self.seen.lock().unwrap().push(q.clone());
            let r = self.replies.lock().unwrap().remove(0);
            Box::pin(async move { r.map(|assertions| ValidatorReply { assertions, model: "m/x".into(), family: "f".into(), tokens: 50 }) })
        }
    }
    struct Rewriter {
        out: Result<String, String>,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl PageWriter for Rewriter {
        fn rewrite<'a>(&'a self, _d: &'a str, f: &'a [String]) -> WriterFuture<'a> {
            self.calls.lock().unwrap().push(f.to_vec());
            let r = self.out.clone();
            Box::pin(async move { r })
        }
    }

    fn a(j: Judgement, rec: Option<&str>, t: &str) -> Assertion {
        Assertion { text: t.into(), judgement: j, record: rec.map(String::from) }
    }

    /// (dir, library, validated claim, noted claim)
    fn setup() -> (tempfile::TempDir, Library, String, String) {
        let (d, l) = lib();
        let s = l.add_file(&file(d.path(), "t.md", "# Tides\n\nThe tide comes in twice a day.\n"), &open("CC0-1.0"), &Budgets::default()).unwrap().id;
        let spec = AnchorSpec { source_id: s, locator: Some("§1".into()), quote: "twice a day".into(), ..Default::default() };
        let v = l.add_claim("Tides come twice a day.", &ClaimOpts::default(), &[spec]).unwrap()["id"].as_str().unwrap().to_string();
        l.validate_claim(&v, Validator::Operator).unwrap();
        let n = l.add_claim("Spring tides are the highest tides.", &ClaimOpts::default(), &[]).unwrap()["id"].as_str().unwrap().to_string();
        (d, l, v, n)
    }

    fn run(l: &Library, draft: &str, v: &Fake, w: &Rewriter, cap: u64) -> Result<CheckedPage, String> {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(l.build_page_checked("tides", draft, Format::Plain, Scope::External, v, w, &TokenLedger::new(cap)))
    }

    fn side(l: &Library, p: &CheckedPage) -> Value {
        serde_json::from_slice(&std::fs::read(l.root().join("pages").join(format!("tides-{}.json", p.page.hash))).unwrap()).unwrap()
    }

    fn rw(out: Result<&str, &str>) -> Rewriter {
        Rewriter { out: out.map(String::from).map_err(String::from), calls: Mutex::new(vec![]) }
    }

    #[test]
    fn a_clean_page_passes_stays_generated_and_records_the_verdict() {
        let (_d, l, v, _n) = setup();
        let f = Fake::new(vec![Ok(vec![a(Judgement::Supported, None, "Tides come twice a day."), a(Judgement::UncitedConsistent, None, "The moon matters.")])]);
        let w = rw(Err("unused"));
        let p = run(&l, &format!("Tides come twice a day [{v}]."), &f, &w, 1_000_000).unwrap();
        assert_eq!(p.verdict.outcome, Outcome::Pass);
        assert!(p.page.markdown.lines().next().unwrap().contains("checked by the validator on") && p.page.markdown.contains("1 unchecked"));
        assert!(!p.page.markdown.contains("validated by"));
        assert!(p.page.markdown.contains("## Unchecked") && p.page.markdown.contains("- The moon matters."));
        let s = side(&l, &p);
        assert_eq!(s["status"], "generated");
        assert_eq!(s["validator"]["result"], "pass");
        assert_eq!(s["validator"]["family"], "f");
        assert!(w.calls.lock().unwrap().is_empty());
        assert!(l.check_page_text(&p.page.markdown, Scope::External).findings.is_empty());
    }

    #[test]
    fn a_contradiction_with_a_validated_claim_fails_once_is_rewritten_with_the_findings_and_is_not_shown() {
        let (_d, l, v, _n) = setup();
        let bad = Assertion { text: "Tides come once a day.".into(), judgement: Judgement::Contradicts, record: Some(v.clone()) };
        let f = Fake::new(vec![Ok(vec![bad]), Ok(vec![a(Judgement::Supported, None, "ok")])]);
        let w = Rewriter { out: Ok(format!("Tides come twice a day, again [{v}].")), calls: Mutex::new(vec![]) };
        let p = run(&l, &format!("Tides come once a day [{v}]."), &f, &w, 1_000_000).unwrap();
        assert_eq!(w.calls.lock().unwrap().len(), 1);
        assert!(w.calls.lock().unwrap()[0][0].contains("contradicts a validated record"));
        assert_eq!((p.verdict.outcome.clone(), p.verdict.rewrites), (Outcome::Pass, 1));
        assert!(!p.page.markdown.contains("once a day"));
    }

    #[test]
    fn two_failures_give_the_claims_only_page_with_the_reason_in_the_banner() {
        let (_d, l, v, _n) = setup();
        let bad = || Assertion { text: "Cited text says nothing like it.".into(), judgement: Judgement::FabricatedCitation, record: Some(v.clone()) };
        let f = Fake::new(vec![Ok(vec![bad()]), Ok(vec![bad()])]);
        let w = Rewriter { out: Ok(format!("Another try [{v}].")), calls: Mutex::new(vec![]) };
        let p = run(&l, &format!("Made up [{v}]."), &f, &w, 1_000_000).unwrap();
        assert_eq!(p.verdict.outcome, Outcome::ClaimsOnly);
        assert_eq!(w.calls.lock().unwrap().len(), 1, "one rewrite, no more");
        let first = p.page.markdown.lines().next().unwrap();
        assert!(first.contains("claims-only page: the draft failed the check twice") && first.contains("fabricated citation"), "{first}");
        assert!(!p.page.markdown.contains("Made up") && !p.page.markdown.contains("Another try"));
        assert!(p.page.markdown.contains("Tides come twice a day."));
        assert_eq!(side(&l, &p)["validator"]["result"], "claims_only");
        assert_eq!(side(&l, &p)["status"], "generated");
    }

    #[test]
    fn a_contradiction_with_only_a_noted_claim_passes_under_contested() {
        let (_d, l, v, n) = setup();
        let c = Assertion { text: "Spring tides are the lowest.".into(), judgement: Judgement::Contradicts, record: Some(n.clone()) };
        let f = Fake::new(vec![Ok(vec![c])]);
        let w = rw(Err("unused"));
        let p = run(&l, &format!("Tides come twice a day [{v}]. Spring tides are the lowest."), &f, &w, 1_000_000).unwrap();
        assert_eq!(p.verdict.outcome, Outcome::PassContested);
        let (_, tail) = p.page.markdown.split_once("## Contested").expect("contested section");
        assert!(tail.contains("Table 1:") && tail.contains(&format!("[{n}]")) && tail.contains("Spring tides are the lowest."));
        assert!(l.check_page_text(&p.page.markdown, Scope::External).findings.is_empty());
        assert_eq!(status(&l.claim(&n).unwrap()), Status::Noted);
    }

    #[test]
    fn a_contradiction_naming_an_unknown_record_fails_closed_and_an_unavailable_validator_falls_back() {
        let (_d, l, v, _n) = setup();
        let c = Assertion { text: "x".into(), judgement: Judgement::Contradicts, record: Some("c-nope".into()) };
        let f = Fake::new(vec![Ok(vec![c.clone()]), Ok(vec![c])]);
        let w = Rewriter { out: Ok(format!("Again [{v}].")), calls: Mutex::new(vec![]) };
        assert_eq!(run(&l, &format!("T [{v}]."), &f, &w, 1_000_000).unwrap().verdict.outcome, Outcome::ClaimsOnly);
        let down = Fake::new(vec![Err("no model".into())]);
        let w = rw(Err("unused"));
        let p = run(&l, &format!("T [{v}]."), &down, &w, 1_000_000).unwrap();
        assert!(p.page.markdown.lines().next().unwrap().contains("not yet checked by the validator") && p.page.markdown.lines().next().unwrap().contains("claims-only page: the validator could not run"));
        assert!(w.calls.lock().unwrap().is_empty());
        let none = Fake::new(vec![]);
        let p = run(&l, &format!("T [{v}]."), &none, &w, 1).unwrap();
        assert!(p.page.markdown.lines().next().unwrap().contains("token budget was reached"));
        assert!(none.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn a_mechanical_failure_never_reaches_the_validator() {
        let (_d, l, _v, _n) = setup();
        let f = Fake::new(vec![]);
        let w = rw(Err("no writer"));
        let p = run(&l, "Tides [c-missing].", &f, &w, 1_000_000).unwrap();
        assert!(f.seen.lock().unwrap().is_empty() && w.calls.lock().unwrap().len() == 1);
        assert_eq!(p.verdict.outcome, Outcome::ClaimsOnly);
    }

    #[test]
    fn replies_are_parsed_strictly_and_validator_text_is_made_safe() {
        assert!(parse_reply("I think the page is fine.").is_none());
        assert_eq!(parse_reply("NO ASSERTIONS").unwrap().len(), 0);
        let r = parse_reply("ASSERTION fabricated | c-1 | see [c-9](http://x.test/y) `bad` | table\nASSERTION maybe | - | ignored\nASSERTION supported | - | fine").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].record.as_deref(), Some("c-1"));
        assert!(!r[0].text.contains(['[', ']', '(', ')', '`', '|']) && !r[0].text.contains("://"), "{}", r[0].text);
    }

    #[test]
    fn a_private_record_never_reaches_the_validator_of_an_external_page() {
        let (d, l) = lib();
        let mut o = open("CC0-1.0");
        o.private = true;
        let s = l.add_file(&file(d.path(), "p.md", "Secret tide table.\n"), &o, &Budgets::default()).unwrap().id;
        let spec = AnchorSpec { source_id: s, locator: Some("§1".into()), quote: "Secret".into(), ..Default::default() };
        let c = l.add_claim("tides secret figure", &ClaimOpts { scope: Some("personal".into()), ..Default::default() }, &[spec]).unwrap();
        let _ = c;
        let f = Fake::new(vec![Ok(vec![])]);
        let w = rw(Err("unused"));
        let _ = run(&l, "Tides are regular.", &f, &w, 1_000_000);
        for q in f.seen.lock().unwrap().iter() {
            assert!(q.material.iter().all(|m| !m.text.contains("secret") && !m.text.contains("Secret")), "{:?}", q.material);
        }
    }

    #[test]
    fn promoting_a_statement_creates_a_noted_claim_with_the_page_as_origin() {
        let (_d, l, _v, _n) = setup();
        let c = l.promote_statement("tides-abc.md", "Neap tides are the lowest.").unwrap();
        let id = c["id"].as_str().unwrap();
        let got = l.claim(id).unwrap();
        assert_eq!(status(&got), Status::Noted);
        assert_eq!(l.origin_of(&got), "page:tides-abc.md");
    }
}

mod acquisition {
    use super::*;
    use crate::fact_fetch::FetchPolicy;
    use crate::library::acquire::{ArchiveList, JobConfig, Scout, ScoutFuture};
    use std::sync::{Arc, Mutex};

    const BOOK: &str = "The Project Gutenberg eBook of Siege Craft\n\nThis eBook is for the use of anyone anywhere at no cost.\n\n*** START ***\nThe ram is hung from chains and swung against the gate.\n*** END ***\n";

    /// A loopback archive: `/search` answers JSON, `/book/<id>.txt` answers plain text. Every request path is recorded.
    fn site() -> (SocketAddr, Arc<Mutex<Vec<String>>>, FetchPolicy) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for mut st in listener.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let n = st.read(&mut buf).unwrap_or(0);
                let path = String::from_utf8_lossy(&buf[..n]).split_whitespace().nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push(path.clone());
                let (ct, body) = if path.starts_with("/search") {
                    ("application/json", r#"{"results":[
                        {"id":1,"title":"Siege Craft","author":"A. Old","died":1850,"size":2000},
                        {"id":2,"title":"Modern Sieges","author":"B. New","died":1950,"size":2000},
                        {"id":3,"title":"Huge Siege Atlas","author":"C. Old","died":1800,"size":900000000},
                        {"id":4,"title":"Undated Sieges","author":"D. Unknown","size":2000}]}"#
                        .to_string())
                } else {
                    ("text/plain", BOOK.to_string())
                };
                let r = format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n{}", ct, body.len(), body);
                let _ = st.write_all(r.as_bytes());
            }
        });
        (addr, seen, FetchPolicy::default().with_fixture_addrs([addr]))
    }

    fn archives(addr: SocketAddr, max_requests: usize) -> ArchiveList {
        ArchiveList::fixture(&format!(
            r#"[[archive]]
id = "fix"
name = "Fixture Archive"
hosts = ["127.0.0.1"]
search_url = "http://127.0.0.1:{p}/search?q={{query}}"
results_path = "results"
id_path = "id"
title_path = "title"
author_path = "author"
year_path = "died"
size_path = "size"
item_url = "http://127.0.0.1:{p}/book/{{id}}.txt"
notice = "Free to use; keep this notice."
attribution = "{{title}} by {{author}} from {{archive}}"
min_delay_ms = 0
max_requests = {m}
[archive.public_domain]
jurisdiction = "US"
field = "author died"
before = 1900
"#,
            p = addr.port(),
            m = max_requests
        ))
    }

    fn cfg() -> JobConfig {
        JobConfig { byte_budget: 10 * 1024 * 1024, max_item: 1024 * 1024, batch_size: 8, token_share_percent: 5 }
    }

    fn topics(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn a_proposal_is_one_batch_with_the_public_domain_basis_and_only_what_the_archives_rule_clears() {
        let (_d, l) = lib();
        let (addr, _seen, policy) = site();
        let a = archives(addr, 3);
        let b = l.propose(&policy, &a, &cfg(), &topics(&["siege craft"])).await.unwrap().unwrap();
        let items = b["items"].as_array().unwrap();
        assert_eq!(items.len(), 1, "a 1950 author, an undated work and an over-size item are not proposed: {:?}", items);
        assert_eq!(items[0]["title"], "Siege Craft");
        assert!(items[0]["licence"].as_str().unwrap().starts_with("public-domain:US:"));
        assert!(items[0]["basis"].as_str().unwrap().contains("author died 1850"));
        assert_eq!(items[0]["status"], "proposed");
        assert!(l.propose(&policy, &a, &cfg(), &topics(&["siege craft"])).await.unwrap().is_none(), "one batch waits at a time");
        assert!(!l.job_due(0) || l.open_batch().is_some());
        assert!(l.list().is_empty(), "nothing is fetched or stored until accepted");
    }

    #[tokio::test]
    async fn an_accepted_source_is_captured_whole_with_notice_attribution_and_stays_not_validated() {
        let (_d, l) = lib();
        let (addr, _seen, policy) = site();
        let a = archives(addr, 3);
        let b = l.propose(&policy, &a, &cfg(), &topics(&["siege craft"])).await.unwrap().unwrap();
        let bid = b["id"].as_str().unwrap();
        assert!(l.decide_batch(bid, "approve", &[]).is_err(), "only accept or skip");
        assert_eq!(l.decide_batch(bid, "accept", &[]).unwrap().len(), 1);
        let done = l.run_accepted(&policy, &a, &cfg()).await;
        assert_eq!(done.len(), 1);
        let s = done[0].1.as_ref().unwrap();
        assert_eq!(s.level, Level::Full);
        let m = l.read_meta(&s.id).unwrap();
        assert!(m["licence"].as_str().unwrap().starts_with("public-domain:US:"), "{}", m["licence"]);
        assert_eq!(m["capture_level"], "full");
        assert_eq!(m["archive_notice"], "Free to use; keep this notice.");
        assert_eq!(m["acquired"]["validated"], false);
        assert!(m["acquired"]["basis"].as_str().unwrap().contains("1850"));
        assert!(m["private"] != true);
        assert!(std::fs::read_to_string(l.source_dir(&s.id).join("source.md")).unwrap().contains("This eBook is for the use of anyone"), "the archive's header stays");
        assert!(l.acquired_bytes() > 0);
        assert_eq!(l.reading_list("acquired-siege-craft").unwrap()["items"][0]["source"], s.id.as_str());
        // a claim made from it starts noted
        let c = l.add_claim("a ram is swung from chains", &crate::library::claims::ClaimOpts::default(), &[]).unwrap();
        assert_eq!(crate::library::claims::status(&l.claim(c["id"].as_str().unwrap()).unwrap()), delphi_core::knowledge_status::Status::Noted);
    }

    #[tokio::test]
    async fn a_skipped_item_is_not_proposed_again() {
        let (_d, l) = lib();
        let (addr, _s, policy) = site();
        let a = archives(addr, 3);
        let b = l.propose(&policy, &a, &cfg(), &topics(&["siege craft"])).await.unwrap().unwrap();
        l.decide_batch(b["id"].as_str().unwrap(), "skip", &[]).unwrap();
        assert!(l.propose(&policy, &a, &cfg(), &topics(&["siege craft"])).await.unwrap().is_none());
        assert!(l.list().is_empty());
    }

    #[tokio::test]
    async fn the_byte_budget_stops_acquisition_and_evicts_nothing() {
        let (_d, l) = lib();
        let (addr, _s, policy) = site();
        let a = archives(addr, 3);
        let keep = l.add_file(&file(_d.path(), "k.md", &"k".repeat(500)), &open("CC0-1.0"), &Budgets::default()).unwrap();
        let b = l.propose(&policy, &a, &cfg(), &topics(&["siege craft"])).await.unwrap().unwrap();
        l.decide_batch(b["id"].as_str().unwrap(), "accept", &[]).unwrap();
        let tight = JobConfig { byte_budget: 1000, ..cfg() };
        assert!(l.run_accepted(&policy, &a, &tight).await.is_empty());
        let after = l.batch(b["id"].as_str().unwrap()).unwrap();
        assert_eq!(after["items"][0]["status"], "budget");
        assert!(after["items"][0]["reason"].as_str().unwrap().contains("byte budget"));
        assert!(l.read_meta(&keep.id).is_some() && l.list().len() == 1, "nothing is evicted and nothing was added");
    }

    #[tokio::test]
    async fn a_standing_rule_takes_matching_items_up_to_its_size_and_only_the_operator_adds_one() {
        let (_d, l) = lib();
        let (addr, _s, policy) = site();
        let a = archives(addr, 3);
        assert!(l.add_rule(&topics(&["sieges"]), &topics(&["nowhere"]), 5000, Some(&a)).is_err(), "an unlisted archive is refused");
        l.add_rule(&topics(&["siege craft"]), &topics(&["fix"]), 1000, Some(&a)).unwrap();
        let b = l.propose(&policy, &a, &cfg(), &topics(&["siege craft"])).await.unwrap().unwrap();
        assert_eq!(b["items"][0]["status"], "proposed", "the item is larger than the rule allows, so it still asks");
        l.decide_batch(b["id"].as_str().unwrap(), "skip", &[]).unwrap();
        let (_d2, l2) = lib();
        l2.add_rule(&topics(&["siege craft"]), &topics(&["fix"]), 5000, Some(&a)).unwrap();
        let b = l2.propose(&policy, &a, &cfg(), &topics(&["siege craft"])).await.unwrap().unwrap();
        assert_eq!((b["items"][0]["status"].as_str(), b["items"][0]["rule"].as_str()), (Some("accepted"), Some("rule-1")));
        assert_eq!(l2.run_accepted(&policy, &a, &cfg()).await.len(), 1);
        let rid = l2.rules()[0].id.clone();
        l2.remove_rule(&rid).unwrap();
        assert!(l2.rules().is_empty());
    }

    #[tokio::test]
    async fn an_archive_is_asked_politely_and_nothing_else_is_asked() {
        let (_d, l) = lib();
        let (addr, seen, policy) = site();
        let a = archives(addr, 2);
        l.propose(&policy, &a, &cfg(), &topics(&["alpha topic", "beta topic", "gamma topic"])).await.unwrap();
        let paths = seen.lock().unwrap().clone();
        assert_eq!(paths.len(), 2, "max_requests holds across topics: {:?}", paths);
        assert!(paths.iter().all(|p| p.starts_with("/search?q=")));
        // an archive's item address off its own hosts is refused when the list is read
        let bad = ArchiveList::parse(
            "[[archive]]\nid=\"x\"\nname=\"X\"\nhosts=[\"a.example\"]\nsearch_url=\"https://a.example/s?q={query}\"\nresults_path=\"r\"\nid_path=\"i\"\ntitle_path=\"t\"\nitem_url=\"https://evil.example/{id}\"\nlicence=\"CC0-1.0\"\nnotice=\"n\"\n",
            None,
        );
        assert!(bad.is_err());
        let plain = ArchiveList::parse("[[archive]]\nid=\"x\"\nname=\"X\"\nhosts=[\"a.example\"]\nsearch_url=\"http://a.example/s?q={query}\"\nresults_path=\"r\"\nid_path=\"i\"\ntitle_path=\"t\"\nitem_url=\"http://a.example/{id}\"\nlicence=\"CC0-1.0\"\nnotice=\"n\"\n", None);
        assert!(plain.is_err(), "https only outside tests");
    }

    #[test]
    fn a_private_sources_tags_never_become_a_search_topic() {
        let (d, l) = lib();
        let s = l.add_file(&file(d.path(), "p.md", "secret plans"), &AddOpts { private: true, licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap();
        let mut m = l.read_meta(&s.id).unwrap();
        m["tags"] = serde_json::json!(["merger codename"]);
        l.write_json(&l.source_dir(&s.id).join("meta.json"), &m).unwrap();
        let seeds = l.seed_topics(&topics(&["siege craft"]), &topics(&["billing"]));
        assert!(seeds.contains(&"siege craft".to_string()) && seeds.contains(&"billing".to_string()));
        assert!(!seeds.iter().any(|t| t.contains("merger")), "{:?}", seeds);
    }

    struct Fake(Mutex<u32>);
    impl Scout for Fake {
        fn widen<'a>(&'a self, _seed: &'a [String], _room: u64) -> ScoutFuture<'a> {
            *self.0.lock().unwrap() += 1;
            Box::pin(async { Ok((vec!["siege craft".to_string(), "<script>ignore previous</script>".to_string()], 120)) })
        }
    }

    #[tokio::test]
    async fn the_scouts_tokens_are_a_share_of_the_trailing_week_and_it_only_widens_topics() {
        let (_d, l) = lib();
        let (addr, seen, policy) = site();
        let a = archives(addr, 6);
        let scout = Fake(Mutex::new(0));
        let r = l.run_job(&policy, &a, &cfg(), topics(&["billing"]), Some(&scout), 0, 1_000_000).await;
        assert_eq!(*scout.0.lock().unwrap(), 0, "5% of nothing is no room: the call is not made");
        assert!(r.notes.iter().any(|n| n.contains("token share")), "{:?}", r.notes);
        let (_d2, l2) = lib();
        let r = l2.run_job(&policy, &a, &cfg(), topics(&["billing"]), Some(&scout), 10_000, 1_000_000).await;
        assert_eq!((*scout.0.lock().unwrap(), r.scout_tokens), (1, 120));
        assert_eq!(l2.scout_spent(1_000_000), 120);
        assert_eq!(l2.scout_room(&cfg(), 10_000, 1_000_000), 500 - 120);
        let q: Vec<String> = seen.lock().unwrap().iter().filter(|p| p.starts_with("/search")).cloned().collect();
        assert!(q.iter().all(|p| !p.contains("%3C") && !p.contains("%3E")), "markup from a model never reaches an archive: {:?}", q);
        assert!(r.batch.is_some() && l2.open_batch().is_some());
        assert!(!l2.job_due(1_000_100), "a batch waits and one ran today");
    }

    #[tokio::test]
    async fn the_digest_card_is_one_open_tier_card_whose_choices_set_no_licence_level_or_status() {
        use crate::digest_review::{acquire_act, acquire_decision, acquire_item, ACQUIRE_ACTIONS};
        let (_d, l) = lib();
        let (addr, _s, policy) = site();
        let a = archives(addr, 3);
        assert!(acquire_item(&l, &[]).is_none());
        l.propose(&policy, &a, &cfg(), &topics(&["siege craft"])).await.unwrap();
        let i = acquire_item(&l, &[]).unwrap();
        let card = acquire_decision(&i, 0, "2026-10-29T09:00:00.000Z");
        assert_eq!((card["kind"].as_str(), card["tier"].as_str()), (Some("learn"), Some("open")));
        assert_eq!(card["choices"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect::<Vec<_>>(), ACQUIRE_ACTIONS);
        assert_eq!(card["items"][0]["status"], "noted");
        assert!(acquire_act("m1", &i, "licence=cc0").is_err() && acquire_act("m1", &i, "validate").is_err());
        let ev = acquire_act("m1", &i, "keep").unwrap();
        assert_eq!(ev.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["learn.digested"]);
        assert!(acquire_item(&l, &ev).is_none(), "an acted card is not shown again");
    }

    #[test]
    fn reading_lists_hold_existing_source_ids_only() {
        let (d, l) = lib();
        let s = l.add_file(&file(d.path(), "r.md", "text"), &open("CC0-1.0"), &Budgets::default()).unwrap();
        assert!(l.reading_add("tides", Some("Tides"), &["no-such-source".to_string()]).is_err());
        l.reading_add("tides", Some("Tides"), &[s.id.clone(), s.id.clone()]).unwrap();
        assert_eq!(l.reading_list("tides").unwrap()["items"].as_array().unwrap().len(), 1);
        assert_eq!(l.reading_lists().len(), 1);
        l.reading_remove("tides", &s.id).unwrap();
        assert!(l.reading_list("tides").unwrap()["items"].as_array().unwrap().is_empty());
    }
}

mod page_search_cache {
    use super::*;
    use crate::fact_judge::TokenLedger;
    use crate::library::bib::Format;
    use crate::library::claims::{AnchorSpec, ClaimOpts, Validator};
    use crate::library::page_search::PageRequest;
    use crate::library::page_validate::*;
    use crate::library::pages::Scope;
    use delphi_adapter::config::PagesConfig;
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct W {
        draft: String,
        writes: AtomicUsize,
    }
    impl PageWriter for W {
        fn write<'a>(&'a self, _b: &'a str) -> WriterFuture<'a> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            let d = self.draft.clone();
            Box::pin(async move { Ok(d) })
        }
        fn rewrite<'a>(&'a self, _d: &'a str, _f: &'a [String]) -> WriterFuture<'a> {
            let d = self.draft.clone();
            Box::pin(async move { Ok(d) })
        }
    }
    struct V(Mutex<Vec<Judgement>>);
    impl PageValidator for V {
        fn validate<'a>(&'a self, _q: &'a PageQuery) -> ValidatorFuture<'a> {
            let j = { let mut g = self.0.lock().unwrap(); if g.len() > 1 { g.remove(0) } else { g[0] } };
            Box::pin(async move { Ok(ValidatorReply { assertions: vec![Assertion { text: "x".into(), judgement: j, record: None }], model: "m".into(), family: "f".into(), tokens: 10 }) })
        }
    }

    fn setup() -> (tempfile::TempDir, Library, W) {
        let (d, l) = lib();
        let s = l.add_file(&file(d.path(), "t.md", "# Tides\n\nThe tide comes in twice a day.\n"), &open("CC0-1.0"), &Budgets::default()).unwrap().id;
        let spec = AnchorSpec { source_id: s, locator: Some("§1".into()), quote: "twice a day".into(), ..Default::default() };
        let c = l.add_claim("Tides come twice a day.", &ClaimOpts::default(), &[spec]).unwrap()["id"].as_str().unwrap().to_string();
        l.validate_claim(&c, Validator::Operator).unwrap();
        (d, l, W { draft: format!("Tides come twice a day [{c}]."), writes: AtomicUsize::new(0) })
    }

    #[allow(clippy::too_many_arguments)]
    fn go(l: &Library, q: &str, external: bool, now: u64, cfg: &PagesConfig, w: &W, v: &V, gate: &(dyn Fn() -> Result<(), String> + Sync)) -> Result<crate::library::page_search::SearchedPage, String> {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(l.search_page(&PageRequest { query: q, external, extra_sources: &[], style: Format::Plain, now, settings: cfg, refresh: false, depth: Default::default() }, w, v, &TokenLedger::new(1_000_000), gate))
    }

    fn pass() -> V {
        V(Mutex::new(vec![Judgement::Supported]))
    }

    #[test]
    fn a_second_search_is_served_from_the_cache_and_calls_no_model() {
        let (_d, l, w) = setup();
        let cfg = PagesConfig::default();
        let a = go(&l, "tides", false, 100, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        assert!(!a.cached && a.markdown.contains("Tides come twice a day"));
        let b = go(&l, "tides", false, 200, &cfg, &w, &pass(), &|| Err("gate must not be asked".into())).unwrap();
        assert!(b.cached);
        assert_eq!(w.writes.load(Ordering::SeqCst), 1);
        assert_eq!(a.entry.key, b.entry.key);
    }

    #[test]
    fn a_changed_claim_is_a_new_page_and_the_old_one_goes_to_history_or_is_stale() {
        let (_d, l, w) = setup();
        let cfg = PagesConfig::default();
        let a = go(&l, "tides", false, 100, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        l.add_claim("Tides follow the moon.", &ClaimOpts::default(), &[]).unwrap();
        let b = go(&l, "tides", false, 200, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        assert!(!b.cached && b.entry.key != a.entry.key);
        assert_eq!(w.writes.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn an_external_page_expires_after_its_ttl_and_is_rebuilt() {
        let (_d, l, w) = setup();
        let cfg = PagesConfig { external_ttl_secs: 1000, ..Default::default() };
        let a = go(&l, "tides", true, 100, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        assert!(go(&l, "tides", true, 600, &cfg, &w, &pass(), &|| Ok(())).unwrap().cached);
        let late = go(&l, "tides", true, 1200, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        assert!(!late.cached);
        assert_eq!(late.entry.key, a.entry.key);
        assert_eq!(w.writes.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_double_failure_is_cached_as_failed_for_an_hour() {
        let (_d, l, w) = setup();
        let cfg = PagesConfig::default();
        let bad = || V(Mutex::new(vec![Judgement::FabricatedCitation]));
        let a = go(&l, "tides", false, 100, &cfg, &w, &bad(), &|| Ok(())).unwrap();
        assert_eq!(a.entry.outcome, "claims_only");
        assert!(a.entry.failed);
        assert!(go(&l, "tides", false, 200, &cfg, &w, &bad(), &|| Ok(())).unwrap().cached);
        assert!(!go(&l, "tides", false, 100 + 3601, &cfg, &w, &bad(), &|| Ok(())).unwrap().cached);
    }

    #[test]
    fn the_per_day_budget_and_the_share_gate_refuse_a_new_page_but_not_a_cached_one() {
        let (_d, l, w) = setup();
        let cfg = PagesConfig { per_day: 1, ..Default::default() };
        go(&l, "tides", false, 100, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        let e = go(&l, "spring tides", false, 200, &cfg, &w, &pass(), &|| Ok(())).unwrap_err();
        assert!(e.contains("per_day") || e.contains("today"), "{e}");
        assert!(go(&l, "tides", false, 300, &cfg, &w, &pass(), &|| Err("no".into())).unwrap().cached);
        let cfg = PagesConfig::default();
        assert!(go(&l, "spring tides", false, 200, &cfg, &w, &pass(), &|| Err("share spent".into())).unwrap_err().contains("share spent"));
    }

    #[test]
    fn external_off_and_disabled_refuse_before_any_work() {
        let (_d, l, w) = setup();
        let off = PagesConfig { external: delphi_adapter::config::PagesExternal::Off, ..Default::default() };
        assert!(go(&l, "tides", true, 1, &off, &w, &pass(), &|| Ok(())).is_err());
        let dis = PagesConfig { enabled: false, ..Default::default() };
        assert!(go(&l, "tides", false, 1, &dis, &w, &pass(), &|| Ok(())).is_err());
        assert_eq!(w.writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_private_source_is_left_out_of_external_material_and_named_queries_are_caught() {
        let (d, l, _w) = setup();
        let o = AddOpts { private: true, level: Some(Level::Full), title: Some("Secret tides ledger".into()), ..open("all-rights-reserved") };
        let s = l.add_file(&file(d.path(), "p.md", "tides secret valve schematic"), &o, &Budgets::default()).unwrap().id;
        let ext = l.gather("tides", Scope::External, std::slice::from_ref(&s));
        assert!(ext.sources.iter().all(|(id, _, _)| *id != s));
        let int = l.gather("tides", Scope::Internal, &[]);
        assert!(int.sources.iter().any(|(id, _, _)| *id == s));
        assert_eq!(l.private_named_in("what is in the Secret tides ledger"), Some(s));
        assert_eq!(l.private_named_in("tides"), None);
    }

    fn edit_meta(l: &Library, id: &str, f: impl Fn(&mut Value)) {
        let path = l.source_dir(id).join("meta.json");
        let mut m: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        f(&mut m);
        std::fs::write(&path, serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    }

    /// The oracle for `specs/057` §5.1: what gather chooses today, for every kind of source it has to handle.
    #[test]
    fn gather_parity_fixtures_fix_what_each_kind_of_source_does() {
        let (d, l, _w) = setup();
        let b = Budgets::default();
        let add = |name: &str, text: &str, o: &AddOpts| l.add_file(&file(d.path(), name, text), o, &b).unwrap().id;
        let public = l.list().into_iter().find(|(_, m)| m["title"] == "t.md").map(|(id, _)| id).unwrap();
        let private = add("p.md", "tides in the private ledger", &AddOpts { private: true, level: Some(Level::Full), title: Some("Private ledger".into()), ..open("all-rights-reserved") });
        let retired = add("r.md", "tides in a retired source", &open("CC0-1.0"));
        edit_meta(&l, &retired, |m| m["retired"] = json!("2026-10-30"));
        let big = add("big.md", &format!("tides {}{}", "filler ".repeat(100_000), "lunarwave"), &open("CC0-1.0"));
        let tampered = add("tamper.md", "tides in a document that changes", &open("CC0-1.0"));
        std::fs::write(l.source_dir(&tampered).join("source.md"), "something else entirely").unwrap();
        let pointer = add("ptr.md", "tides pointer body", &AddOpts { level: Some(Level::Pointer), title: Some("Tides pointer".into()), ..open("CC0-1.0") });
        let unrelated = add("u.md", "gardening notes", &open("CC0-1.0"));
        let on_private = l.add_claim("Tides rest on the private ledger.", &ClaimOpts::default(), &[AnchorSpec { source_id: private.clone(), locator: Some("§1".into()), quote: "private ledger".into(), ..Default::default() }]).unwrap()["id"].as_str().unwrap().to_string();

        let internal = l.gather("tides", Scope::Internal, &[]);
        let ids = |g: &crate::library::page_search::Gathered| g.sources.iter().map(|(id, _, _)| id.clone()).collect::<BTreeSet<_>>();
        let want: BTreeSet<String> = [&public, &private, &big, &pointer].into_iter().cloned().collect();
        assert_eq!(ids(&internal), want, "retired, tampered (empty text, no title match) and unrelated sources are left out");
        assert!(internal.claims.iter().any(|c| c["id"] == on_private.as_str()));
        assert_eq!(internal.withheld, 0);
        assert!(internal.sources.iter().find(|(id, _, _)| *id == big).unwrap().2.chars().count() <= 3 * 1024);
        assert!(l.gather("lunarwave", Scope::Internal, &[]).sources.is_empty(), "only the first 20 KiB of text is scanned");

        let external = l.gather("tides", Scope::External, &[]);
        let want: BTreeSet<String> = [&public, &big, &pointer].into_iter().cloned().collect();
        assert_eq!(ids(&external), want, "a private source is left out of an external page");
        assert!(external.claims.iter().all(|c| c["id"] != on_private.as_str()));
        assert_eq!(external.withheld, 1, "a claim resting on a private source is withheld, and counted");

        let forced = l.gather("tides", Scope::Internal, std::slice::from_ref(&unrelated));
        assert_eq!(forced.sources.first().map(|s| s.0.clone()), Some(unrelated), "a source a search just captured joins whether or not it matches");
    }

    /// The baseline of `specs/057` §5.1: run with `cargo test -p delphi-engine --release --lib gather_baseline -- --ignored --nocapture`.
    /// It records where gather's time goes; it asserts nothing about speed.
    #[test]
    #[ignore]
    fn gather_baseline_report() {
        for (sources, claims) in [(100usize, 100usize), (1_000, 1_000), (3_000, 3_000)] {
            let (d, l) = lib();
            let b = Budgets::default();
            for i in 0..sources {
                let body = if i % 50 == 0 { format!("tides {}", "filler words here ".repeat(60_000)) } else { format!("source {i} about tides and harbours. {}", "ordinary words ".repeat(200)) };
                l.add_file(&file(d.path(), &format!("s{i}.md"), &body), &open("CC0-1.0"), &b).unwrap();
            }
            for i in 0..claims {
                l.add_claim(&format!("Claim {i} says the tide turns at the harbour."), &ClaimOpts::default(), &[]).unwrap();
            }
            let mut best = None;
            for _ in 0..5 {
                let g = l.gather("tides harbour", Scope::Internal, &[]);
                if best.is_none_or(|b: crate::library::page_search::GatherStats| g.stats.total_ns < b.total_ns) {
                    best = Some(g.stats);
                }
            }
            let st = best.unwrap();
            let ms = |n: u64| n as f64 / 1e6;
            println!("sources {sources:>5} claims {claims:>5}: total {:>8.1} ms | list {:>7.1} read {:>7.1} hash {:>7.1} tokenize {:>7.1} | {} MiB read, {} reads", ms(st.total_ns), ms(st.list_ns), ms(st.read_ns), ms(st.hash_ns), ms(st.tokenize_ns), st.bytes_read >> 20, st.text_reads);
        }
    }

    #[test]
    fn gather_counts_what_it_reads() {
        let (d, l, _w) = setup();
        for i in 0..3 {
            l.add_file(&file(d.path(), &format!("n{i}.md"), &format!("note {i} about tides and more words")), &open("CC0-1.0"), &Budgets::default()).unwrap();
        }
        let g = l.gather("tides", Scope::Internal, &[]);
        let st = g.stats;
        assert_eq!((st.sources_seen, st.text_reads, st.claims_seen), (4, 4, 1));
        assert!(st.bytes_read > 0 && st.total_ns >= st.read_ns + st.hash_ns);
        assert_eq!(st.to_json()["text_reads"], 4);
    }

    fn go_with(l: &Library, q: &str, now: u64, cfg: &PagesConfig, w: &W, refresh: bool, gate: &(dyn Fn() -> Result<(), String> + Sync)) -> Result<crate::library::page_search::SearchedPage, String> {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(l.search_page(&PageRequest { query: q, external: false, extra_sources: &[], style: Format::Plain, now, settings: cfg, refresh, depth: Default::default() }, w, &pass(), &TokenLedger::new(1_000_000), gate))
    }

    fn png() -> Vec<u8> {
        let rgba = (0..64 * 64).flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 4) as u8, 120, 255]).collect();
        delphi_docs::png::encode(&delphi_docs::png::Image { width: 64, height: 64, rgba }, &[])
    }

    #[test]
    fn the_writers_lifetime_is_written_as_frontmatter_and_ends_the_cache() {
        let (_d, l, mut w) = setup();
        w.draft = format!("{}\n\n<!-- page: kind=article; volatility=low; days=90; reason=A settled fact -->", w.draft);
        let cfg = PagesConfig::default();
        let (t0, day) = (1_000_000u64, 86_400u64);
        let a = go(&l, "tides", false, t0, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        assert_eq!((a.entry.valid_until, a.entry.volatility.as_deref(), a.entry.ttl_reason.as_deref()), (Some(t0 + 90 * day), Some("low"), Some("A settled fact")));
        assert!(!a.markdown.contains("<!--") && !a.markdown.starts_with("---"), "{}", a.markdown);
        let on_disk = std::fs::read_to_string(&a.path).unwrap();
        assert!(on_disk.starts_with("---\n") && on_disk.contains("valid_until: 1970-04-12") && on_disk.contains("author: writer") && on_disk.contains("volatility: low"), "{on_disk}");
        assert!(l.check_page_text(&on_disk, Scope::Internal).findings.is_empty(), "the frontmatter is not part of what the check reads");
        // fresh until the date, rebuilt after it, and a refresh rebuilds at once
        assert!(go(&l, "tides", false, t0 + 89 * day, &cfg, &w, &pass(), &|| Err("no model".into())).unwrap().cached);
        let late = go(&l, "tides", false, t0 + 91 * day, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        assert!(!late.cached);
        assert_eq!(late.entry.key, a.entry.key);
        assert!(!go_with(&l, "tides", t0 + 92 * day, &cfg, &w, true, &|| Ok(())).unwrap().cached);
        assert_eq!(w.writes.load(Ordering::SeqCst), 3);
        let (opened, md) = l.open_page(&a.entry.key, t0 + 93 * day, cfg.external_ttl_secs).unwrap();
        assert!(!md.starts_with("---") && opened.stale.is_none(), "{:?}", opened.stale);
    }

    #[test]
    fn a_missing_or_wild_lifetime_is_clamped_to_the_operators_bounds() {
        let day = 86_400u64;
        let (_d, l, w) = setup();
        let none = go(&l, "tides", false, 0, &PagesConfig::default(), &w, &pass(), &|| Ok(())).unwrap();
        assert_eq!((none.entry.volatility.as_deref(), none.entry.valid_until), (Some("medium"), Some(30 * day)));
        let wild = |days: &str| W { draft: format!("{}\n<!-- page: volatility=timeless; days={}; reason=Forever -->", w.draft, days), writes: AtomicUsize::new(0) };
        let (_d, l, _w) = setup();
        assert_eq!(go(&l, "tides", false, 0, &PagesConfig::default(), &wild("9999"), &pass(), &|| Ok(())).unwrap().entry.valid_until, Some(365 * day));
        let (_d, l, _w) = setup();
        let tight = PagesConfig { min_ttl_days: 2, max_ttl_days: 10, ..Default::default() };
        assert_eq!(go(&l, "tides", false, 0, &tight, &wild("9999"), &pass(), &|| Ok(())).unwrap().entry.valid_until, Some(10 * day));
    }

    #[test]
    fn a_page_may_show_a_cited_sources_picture_link_a_section_and_point_to_another_page() {
        let (_d, l, _w) = setup();
        let sid = l.list().into_iter().find(|(_, m)| m["title"] == "t.md").map(|(id, _)| id).unwrap();
        std::fs::create_dir_all(l.source_dir(&sid).join("assets")).unwrap();
        std::fs::write(l.source_dir(&sid).join("assets/chart.png"), png()).unwrap();
        std::fs::write(l.source_dir(&sid).join("assets/notes.txt"), "not a picture").unwrap();
        std::fs::write(l.source_dir(&sid).join("assets/has space.png"), png()).unwrap();
        let g = l.gather("tides", Scope::Internal, &[]);
        assert_eq!(g.assets.iter().map(|a| a.uri.clone()).collect::<Vec<_>>(), vec![format!("lib:{sid}/chart.png")]);
        let brief = crate::library::page_search::brief_for("tides", &g, 3000, Default::default());
        assert!(brief.contains(&format!("ASSET lib:{sid}/chart.png | from s-{sid}")) && brief.contains("page:slug") && !brief.contains("Do not write images or links"));

        let good = format!("# Tides\n\n**Tides** rise twice a day [s-{sid}].\n\n## Contents\n\n1. [The cause](#the-cause)\n\n## The cause\n\n![A tide chart](lib:{sid}/chart.png)\n*Figure 1: Chart [s-{sid}].*\n\n> *For the Greek figure, see [Tithonus](page:tithonus).*\n");
        let found = |body: &str| l.check_body(body, Scope::Internal).findings.iter().map(|f| f.to_string()).collect::<Vec<_>>();
        assert!(found(&good).is_empty(), "{:?}", found(&good));
        assert!(found(&good.replace("(#the-cause)", "(#nowhere)")).iter().any(|f| f.contains("names no heading")));
        assert!(found(&good.replace("page:tithonus", "page:Bad Slug")).iter().any(|f| f.contains("`page:` link")));
        assert!(found(&good.replace(&format!("lib:{sid}/chart.png"), "kb:docs/a.md#b.png")).iter().any(|f| f.contains("not an asset")));
        assert!(found(&good.replace("chart.png", "missing.png")).iter().any(|f| f.contains("not an asset")));
        assert!(found(&good.replace("(page:tithonus)", "(https://example.org/x)")).iter().any(|f| f.contains("link not in any cited")));
    }

    #[test]
    fn a_workspace_document_becomes_a_noted_internal_source_with_its_pictures() {
        let (d, l) = lib();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(ws.join("docs/img")).unwrap();
        std::fs::write(ws.join("docs/img/chart.png"), png()).unwrap();
        std::fs::write(ws.join("docs/spec.md"), "# Harbour tides\n\nTides turn at the harbour wall.\n\n![Chart](img/chart.png)\n![Remote](https://example.org/r.png)\n").unwrap();
        let b = Budgets::default();
        let s = l.capture_workspace_doc(&ws, &ws.join("docs/spec.md"), "docs/spec.md", &b).unwrap();
        assert_eq!(s.level, Level::Full);
        let m = l.read_meta(&s.id).unwrap();
        assert_eq!((m["status"].as_str(), m["licence"].as_str(), m["title"].as_str(), m["workspace_path"].as_str()), (Some("not validated"), Some("internal"), Some("Harbour tides"), Some("docs/spec.md")));
        assert!(l.asset_file(&s.id, "chart.png").is_some());
        assert!(l.asset_file(&s.id, "../meta.json").is_none() && l.asset_file(&s.id, "nope.png").is_none() && l.asset_file("no-such", "chart.png").is_none());
        assert_eq!(l.capture_workspace_doc(&ws, &ws.join("docs/spec.md"), "docs/spec.md", &b).unwrap().outcome, crate::library::Outcome::Unchanged);
        let outside = file(d.path(), "o.md", "# Outside");
        assert!(l.capture_workspace_doc(&ws, &outside, "o.md", &b).is_err(), "a file outside the workspace is not taken");
        std::fs::write(ws.join("docs/data.bin"), [0u8, 1]).unwrap();
        assert!(l.capture_workspace_doc(&ws, &ws.join("docs/data.bin"), "docs/data.bin", &b).is_err());
        let g = l.gather("harbour tides", Scope::Internal, &[]);
        assert!(g.sources.iter().any(|(id, _, _)| *id == s.id) && g.assets.iter().any(|a| a.uri == format!("lib:{}/chart.png", s.id)));

        // the console's picture reader serves it to the operator only, and nothing outside `assets/`
        let kb = crate::kb::Kb::open_in_memory().unwrap();
        let get = |operator: bool, src: &str| crate::kbserve::Reader { kb: &kb, ws: &ws, operator, home: Some(d.path()) }.get(&format!("/v1/image?src={src}"));
        let ok = get(true, &format!("lib:{}/chart.png", s.id));
        assert_eq!((ok.status, ok.body["width"].as_u64()), (200, Some(64)), "{}", ok.body);
        assert_eq!(get(false, &format!("lib:{}/chart.png", s.id)).status, 403);
        assert_eq!(get(true, &format!("lib:{}/../meta.json", s.id)).status, 404);
        assert_eq!(get(true, &format!("lib:{}/source.md", s.id)).status, 404);
        assert_eq!(get(true, "lib:no-such/chart.png").status, 404);
    }

    #[test]
    fn eviction_drops_the_least_recently_opened_and_never_a_pinned_page() {
        let (_d, l, w) = setup();
        let cfg = PagesConfig::default();
        let a = go(&l, "tides", false, 100, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        let b = go(&l, "spring tides", false, 200, &cfg, &w, &pass(), &|| Ok(())).unwrap();
        l.pin_page(&a.entry.key, true, 300, cfg.external_ttl_secs).unwrap();
        let dropped = l.evict_pages(1);
        assert!(dropped.contains(&b.entry.key) && !dropped.contains(&a.entry.key), "{dropped:?}");
        assert!(l.open_page(&a.entry.key, 400, cfg.external_ttl_secs).is_some());
    }

    #[test]
    fn expand_page_with_reputable_sources_and_images() {
        let (_d, l, _w) = setup();
        let sid = l.list().into_iter().find(|(_, m)| m["title"] == "t.md").map(|(id, _)| id).unwrap();
        std::fs::create_dir_all(l.source_dir(&sid).join("assets")).unwrap();
        std::fs::write(l.source_dir(&sid).join("assets/chart.png"), png()).unwrap();
        let cfg = PagesConfig::default();
        let w = W {
            draft: format!(
                "# Tides\n\n**Tides** are the rise and fall of sea levels [s-{sid}].\n\n## The cause\n\n![A tide chart](lib:{sid}/chart.png)\n*Figure 1: Chart [s-{sid}].*\n<!-- page: kind=article; depth=expanded; volatility=low; days=90; reason=Settled oceanography -->"
            ),
            writes: AtomicUsize::new(0),
        };
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let req = PageRequest {
            query: "tides",
            external: false,
            extra_sources: &[],
            style: Format::Plain,
            now: 1000,
            settings: &cfg,
            refresh: true,
            depth: crate::library::page_meta::Depth::Expanded,
        };
        let res = rt.block_on(l.search_page(&req, &w, &pass(), &TokenLedger::new(1_000_000), &|| Ok(()))).unwrap();
        assert!(!res.cached);
        assert_eq!(res.entry.depth, "expanded");
        assert_eq!(res.entry.volatility.as_deref(), Some("low"));
        let on_disk = std::fs::read_to_string(&res.path).unwrap();
        assert!(on_disk.contains("depth: expanded"));
        assert!(on_disk.contains("volatility: low"));
        assert!(res.markdown.contains("![A tide chart](lib:"));
    }
}
