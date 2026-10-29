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
    use crate::library::pages::{parse_draft, Finding, NOTED_HEADING};
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

    #[test]
    fn every_sentence_carries_its_claim_and_unvalidated_ones_sit_under_the_noted_heading() {
        let (_d, l, a, b) = setup();
        let p = l.build_page("tides", None, Format::Plain).unwrap();
        let (head, tail) = p.markdown.split_once(&format!("## {}", NOTED_HEADING)).expect("noted heading");
        assert!(head.contains(&format!("[{} ● validated]", a)) && !head.contains(&b));
        assert!(tail.contains(&format!("[{} ○ not validated]", b)));
        assert!(p.markdown.contains("## Sources"));
        assert!(l.check_page(&parse_draft(&p.markdown.lines().filter(|x| x.contains("[c-")).collect::<Vec<_>>().join("\n"))).is_empty(), "a built page passes its own check");
    }

    #[test]
    fn the_check_rejects_uncited_unknown_and_content_adding_sentences() {
        let (_d, l, a, _b) = setup();
        let draft = format!("Tides come twice a day [{a}]. The moon is made of cheese. Tides come twice a day, usually near harbours [{a}]. Tides come twice a day [c-0000].");
        let f = l.build_page("tides", Some(&draft), Format::Plain).unwrap_err();
        assert!(f.iter().any(|x| matches!(x, Finding::Uncited(s) if s.contains("cheese"))));
        assert!(f.iter().any(|x| matches!(x, Finding::AddsContent { words, .. } if words.contains(&"harbours".to_string()))));
        assert!(f.iter().any(|x| matches!(x, Finding::UnknownClaim { .. })));
        assert_eq!(f.len(), 3);
        let ok = l.build_page("tides", Some(&format!("The tides come twice a day [{a}].")), Format::Plain).unwrap();
        assert_eq!(ok.validated.len(), 1);
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
        assert!(l.build_page("vault", Some(&format!("The vault code is long [{}].", c)), Format::Plain).is_err());
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
