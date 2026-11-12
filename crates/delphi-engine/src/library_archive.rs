//! Archived copies (`specs/101` §5, slice C): a page the daemon can no longer read live, read from a web archive instead.
//!
//! **Read-only.** Delphi asks one pinned endpoint whether it holds a copy of a public URL and reads the copy back. It never asks an
//! archive to save a page (saving would publish the address). What is sent is the URL of a source the daemon itself fetched and that is
//! not private: the one guard is [`crate::egress::check`] on the source's own record with `Channel::ArchiveLookup`, so a private source,
//! a record that cannot be read, a URL the daemon never fetched and a name that is not a public address send nothing.
//!
//! **A locator, not a source.** The copy is stored as a *version* of the original source (`versions/<n>/source.md`), headed by a banner
//! and an H1 that every chunk of the search index inherits ("Archived copy from <date>: <title>"). Its origin is the original page's
//! origin, and while the current version is a copy it adds no origin and no trusted source (`claims::cites_of`): a live re-check that
//! succeeds stores a version without the mark and the source counts again. A claim anchored only in a copy is marked
//! "checked against archived copy of <date>" (`support.archived`), never "checked".
//!
//! **Untrusted content.** The copy comes through the daemon's fetcher: https only, public addresses only, same-origin redirects only, a
//! size cap, text types only, and the cloaking check. A copy that differs for a browser is refused.
//!
//! What was verified about the archive (2026-11-11): the Internet Archive's documented Availability API, `GET /wayback/available?url=`,
//! answers `{"archived_snapshots":{"closest":{"available":true,"url":…,"timestamp":"YYYYMMDDhhmmss","status":"200"}}}` and
//! `{"archived_snapshots":{}}` when there is none; it works over https; it also echoes `url` and `timestamp`, which the page does not
//! list. Its page says nothing about rate limits, terms or authentication. The raw capture form `/web/<timestamp>id_/<url>` (original
//! bytes, no toolbar) works but is not in the archive's own documentation. The snapshot `url` the API returns is NOT used: it is `http`
//! and may embed credentials from the original; the copy's address is built from the timestamp and the source's own URL.

use super::*;
use crate::fact_fetch::{fetch_catalog, fetch_snapshot_checked, FetchPolicy};
use delphi_adapter::reqwest::Url;

/// The one pinned endpoint (`specs/101` §7): the Internet Archive's Availability API.
pub const DEFAULT_ENDPOINT: &str = "https://archive.org/wayback/available";
/// Where a copy is read from: `<base><timestamp>id_/<original url>`.
pub const DEFAULT_RAW_BASE: &str = "https://web.archive.org/web/";
const MAX_URL: usize = 2000;
/// Names that are not on the public internet; such a name is never sent to an archive.
const NOT_PUBLIC: [&str; 9] = [".local", ".localhost", ".internal", ".lan", ".home", ".corp", ".intranet", ".private", ".home.arpa"];

/// `[research] archive_fallback` and `archive_endpoint` (`specs/101` §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveConfig {
    /// Look for an archived copy on its own, when a re-check finds a public source gone. The explicit command works either way.
    pub enabled: bool,
    pub endpoint: String,
    /// Where copies are read from; fixed in the code, set apart only by tests.
    pub raw_base: String,
}

impl Default for ArchiveConfig {
    fn default() -> ArchiveConfig {
        ArchiveConfig { enabled: false, endpoint: DEFAULT_ENDPOINT.into(), raw_base: DEFAULT_RAW_BASE.into() }
    }
}

/// What a lookup came to. Nothing in it carries text from a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveOutcome {
    /// A copy was stored as this version of the source, taken at `archived_at`.
    Captured { version: u64, archived_at: String },
    /// The archive holds no copy.
    NoCopy,
    /// Nothing was sent. The reason is a fixed phrase.
    Refused(&'static str),
    /// Nothing changed because the library already holds what a copy would add.
    NotNeeded(&'static str),
    /// The archive or its copy could not be used (unreachable, not text, too large, cloaked, an answer that did not read). The run goes on.
    Unavailable(String),
}

impl ArchiveOutcome {
    /// One line for the search log and the coverage report (`archive: unavailable` and so on).
    pub fn line(&self) -> String {
        match self {
            ArchiveOutcome::Captured { version, archived_at } => format!("archive: copy of {} stored as version {}", &archived_at[..archived_at.len().min(10)], version),
            ArchiveOutcome::NoCopy => "archive: no copy".into(),
            ArchiveOutcome::Refused(why) => format!("archive: nothing sent ({})", why),
            ArchiveOutcome::NotNeeded(why) => format!("archive: not needed ({})", why),
            ArchiveOutcome::Unavailable(why) => format!("archive: unavailable ({})", why),
        }
    }
}

/// The address to ask an archive about: https or http, a public-looking name, no credentials, no fragment, at most 2,000 characters.
/// The query string stays exactly as the source's own address had it; nothing is added. `None` for anything else.
pub fn public_original(url: &str) -> Option<String> {
    if url.len() > MAX_URL {
        return None;
    }
    let mut u = Url::parse(url.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    let host = u.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') || !host.contains('.') || host == "localhost" {
        return None;
    }
    if NOT_PUBLIC.iter().any(|s| host.ends_with(s)) {
        return None;
    }
    u.set_fragment(None);
    Some(u.to_string())
}

/// The request that asks the archive whether it holds a copy: the endpoint plus `url=<original>`, nothing else.
pub fn lookup_url(endpoint: &str, original: &str) -> Option<String> {
    let mut u = Url::parse(endpoint).ok()?;
    u.query_pairs_mut().append_pair("url", original);
    Some(u.to_string())
}

fn timestamp_ok(ts: &str) -> bool {
    let b = ts.as_bytes();
    if b.len() != 14 || !b.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let n = |r: std::ops::Range<usize>| ts[r].parse::<u32>().unwrap_or(99);
    (1996..=2100).contains(&n(0..4)) && (1..=12).contains(&n(4..6)) && (1..=31).contains(&n(6..8)) && n(8..10) < 24 && n(10..12) < 60 && n(12..14) <= 60
}

/// The archive's answer: the timestamp of the closest copy, or `None` when it holds none. Only the documented fields are read, and only
/// a copy whose `status` is `200` counts. The `url` it offers is never used.
pub fn parse_availability(body: &[u8]) -> Result<Option<String>, &'static str> {
    let v: Value = serde_json::from_slice(body).map_err(|_| "the answer did not read")?;
    let snaps = v.get("archived_snapshots").and_then(Value::as_object).ok_or("the answer did not read")?;
    let Some(closest) = snaps.get("closest") else { return Ok(None) };
    let ok = closest["available"] == true && (closest["status"] == "200" || closest["status"] == 200);
    if !ok {
        return Ok(None);
    }
    match closest["timestamp"].as_str() {
        Some(ts) if timestamp_ok(ts) => Ok(Some(ts.to_string())),
        _ => Err("the answer did not read"),
    }
}

/// `20150102000131` as `2015-01-02T00:01:31Z`. The caller has checked the digits.
pub fn rfc3339_of(ts: &str) -> String {
    format!("{}-{}-{}T{}:{}:{}Z", &ts[0..4], &ts[4..6], &ts[6..8], &ts[8..10], &ts[10..12], &ts[12..14])
}

/// Where the raw copy is read from: `<base><timestamp>id_/<original>`.
pub fn copy_url(raw_base: &str, ts: &str, original: &str) -> String {
    format!("{}{}id_/{}", raw_base, ts, original)
}

fn document(title: &str, archived_at: &str, live_url: &str, host: &str, text: &str) -> String {
    let day = &archived_at[..10];
    let title = if title.trim().is_empty() { live_url } else { title.trim() };
    format!(
        "# Archived copy from {day}: {title}\n\n> **Archived copy from {day}, not the live page.** Live address: {live_url}. Read from {host}; the text below is the archive's and says nothing about what the page says now.\n\n{text}\n"
    )
}

impl Library {
    /// Look for an archived copy of a source the daemon already holds. `automatic` is true when the library itself decides to look (a
    /// re-check found the page gone): then `[research] archive_fallback` must be on. An operator's explicit command passes false.
    ///
    /// Sends nothing unless: the source exists; its record is public and readable (`egress::check`); the daemon fetched the page itself;
    /// the capture was not purged and is not generated; and its address looks public. Nothing is looked up for a source whose text is
    /// already held (a re-check keeps the last capture, which has the quote), or whose current version is already a copy.
    pub async fn add_archived_copy(&self, policy: &FetchPolicy, cfg: &ArchiveConfig, id: &str, budgets: &Budgets, automatic: bool) -> ArchiveOutcome {
        if automatic && !cfg.enabled {
            return ArchiveOutcome::Refused("archive lookups are off ([research] archive_fallback)");
        }
        let Some(meta) = self.read_meta(id) else { return ArchiveOutcome::Refused("no such source") };
        // The one guard, before anything leaves: a private source or an unreadable record sends nothing (`specs/098` §12 item 16).
        if crate::egress::check(&meta, Channel::ArchiveLookup).is_err() {
            return ArchiveOutcome::Refused("a private source, or a record that cannot be read, sends nothing");
        }
        if !meta["purged"].is_null() {
            return ArchiveOutcome::Refused("the capture was purged");
        }
        let generated = meta["derived"].as_bool() == Some(true) || meta["origin"].as_str().is_some_and(|o| o.starts_with("page:"));
        let (Some(canonical), true, false) = (meta["canonical_url"].as_str(), meta["content_hash"].is_string(), generated) else {
            return ArchiveOutcome::Refused("the daemon did not fetch this page itself");
        };
        if meta["archived_copy"].is_object() {
            return ArchiveOutcome::NotNeeded("the current version is already an archived copy");
        }
        if self.source_dir(id).join("source.md").is_file() {
            return ArchiveOutcome::NotNeeded("a readable capture is already held");
        }
        let Some(original) = public_original(canonical) else { return ArchiveOutcome::Refused("not a public web address") };
        self.capture_archived(policy, cfg, id, &original, Some(meta), None, budgets).await
    }

    /// Capture a copy of a public URL the operator names, as a new source (or the next version of the one already held, when that one
    /// has no readable text). The operator's own act: nothing here is looked up on its own.
    pub async fn add_archived_url(&self, policy: &FetchPolicy, cfg: &ArchiveConfig, url: &str, opts: &AddOpts, budgets: &Budgets) -> ArchiveOutcome {
        let Some(original) = public_original(url) else { return ArchiveOutcome::Refused("not a public web address") };
        if opts.private {
            return ArchiveOutcome::Refused("a private capture is never looked up");
        }
        let id = source_id(&original);
        if let Some(meta) = self.read_meta(&id) {
            if crate::egress::check(&meta, Channel::ArchiveLookup).is_err() {
                return ArchiveOutcome::Refused("a private source, or a record that cannot be read, sends nothing");
            }
            if self.source_dir(&id).join("source.md").is_file() {
                return ArchiveOutcome::NotNeeded("a readable capture is already held");
            }
            return self.capture_archived(policy, cfg, &id, &original, Some(meta), None, budgets).await;
        }
        self.capture_archived(policy, cfg, &id, &original, None, Some(opts), budgets).await
    }

    /// `id` is the source the copy becomes a version of: the one the caller named, or the one the URL maps to.
    #[allow(clippy::too_many_arguments)]
    async fn capture_archived(&self, policy: &FetchPolicy, cfg: &ArchiveConfig, id: &str, original: &str, existing: Option<Value>, opts: Option<&AddOpts>, budgets: &Budgets) -> ArchiveOutcome {
        let Some(ask) = lookup_url(&cfg.endpoint, original) else { return ArchiveOutcome::Unavailable("the endpoint is not a URL".into()) };
        let answer = match fetch_catalog(policy, &ask).await {
            Ok(b) => b,
            Err(e) => return ArchiveOutcome::Unavailable(format!("the lookup failed: {}", e)),
        };
        let ts = match parse_availability(&answer) {
            Ok(Some(ts)) => ts,
            Ok(None) => return ArchiveOutcome::NoCopy,
            Err(why) => return ArchiveOutcome::Unavailable(why.to_string()),
        };
        let archived_at = rfc3339_of(&ts);
        let mut copy_policy = policy.clone();
        copy_policy.max_bytes = budgets.text as usize;
        let (snap, _) = match fetch_snapshot_checked(&copy_policy, &copy_url(&cfg.raw_base, &ts, original)).await {
            Ok(x) => x,
            Err(e) => return ArchiveOutcome::Unavailable(format!("the copy could not be read: {}", e)),
        };
        if snap.cloaked {
            return ArchiveOutcome::Unavailable("the copy showed different text to a browser".into());
        }
        if snap.truncated {
            return ArchiveOutcome::Unavailable("the copy is over the size limit".into());
        }
        let host = Url::parse(&cfg.raw_base).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_else(|| "the archive".into());
        let live_host = Url::parse(original).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
        let id = id.to_string();
        let (mut meta, requested) = match (&existing, opts) {
            (Some(Value::Object(m)), _) => (m.clone(), None),
            (_, Some(o)) => {
                let mut m = Self::base_meta(o, "webpage", &live_host);
                m.insert("URL".into(), json!(original));
                m.insert("canonical_url".into(), json!(original));
                // The origin is the ORIGINAL page's, never the archive's: a copy shares the origin of the page it copies.
                m.insert("origin".into(), json!(crate::fact_fetch::registrable_domain(&live_host)));
                m.insert("source_class".into(), json!(SourceList::default().classify(original).class.as_str()));
                m.insert("source_unfamiliar".into(), json!(SourceList::default().unfamiliar(original)));
                m.insert("tags".into(), json!([]));
                (m, o.level)
            }
            _ => return ArchiveOutcome::Refused("no source to attach a copy to"),
        };
        let title = meta.get("title").and_then(Value::as_str).filter(|t| !t.is_empty() && *t != live_host).map(String::from).unwrap_or_else(|| snap.title.for_daemon().clone());
        if !title.is_empty() {
            meta.insert("title".into(), json!(title));
        }
        meta.insert("retrieved".into(), json!(snap.retrieved));
        meta.insert("content_hash".into(), json!(format!("sha256:{}", snap.id)));
        meta.insert("archived_copy".into(), json!({"archive": host, "archived_at": archived_at, "retrieved_at": snap.retrieved, "live_url": original}));
        let text = document(&title, &archived_at, original, &host, snap.text.for_daemon());
        let prepared = Prepared { id: id.clone(), meta, text: Some(text), assets: None, requested, excerpts: None, imported: false };
        match self.store(prepared, budgets) {
            Ok(s) => match s.outcome {
                Outcome::Added => self.captured(&id, 1, archived_at),
                Outcome::NewVersion(n) => self.captured(&id, n, archived_at),
                _ => ArchiveOutcome::NotNeeded("the library already holds the same or a richer capture"),
            },
            Err(e) => ArchiveOutcome::Unavailable(e),
        }
    }

    fn captured(&self, id: &str, version: u64, archived_at: String) -> ArchiveOutcome {
        self.log("archive_copy", json!({"id": id, "version": version, "archived_at": archived_at}));
        // The claims that cite the source are counted again at once: the copy adds no origin, and they read "checked against archived copy".
        self.refresh_citing(id);
        ArchiveOutcome::Captured { version, archived_at }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    const PAGE: &str = "<html><head><title>Old Harbour Notes</title></head><body><p>The harbour wall was built in 1843 by the town.</p><p>Spring tides reach the second step.</p></body></html>";

    /// A loopback archive. `/wayback/available` answers JSON (`snapshot` true: a copy at 2015-01-02), `/web/<ts>id_/<url>` answers HTML.
    /// Every request path is recorded.
    fn archive(snapshot: bool) -> (std::net::SocketAddr, Arc<Mutex<Vec<String>>>, FetchPolicy, ArchiveConfig) {
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
                let (ct, body) = if path.starts_with("/wayback/available") {
                    let b = if snapshot { r#"{"url":"x","archived_snapshots":{"closest":{"status":"200","available":true,"url":"http://web.archive.org/web/20150102000131/http://user:pass@old.example.org/harbour","timestamp":"20150102000131"}}}"# } else { r#"{"archived_snapshots":{}}"# };
                    ("application/json", b.to_string())
                } else {
                    ("text/html", PAGE.to_string())
                };
                let r = format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n{}", ct, body.len(), body);
                let _ = st.write_all(r.as_bytes());
            }
        });
        let cfg = ArchiveConfig { enabled: true, endpoint: format!("http://127.0.0.1:{}/wayback/available", addr.port()), raw_base: format!("http://127.0.0.1:{}/web/", addr.port()) };
        (addr, seen, FetchPolicy::default().with_fixture_addrs([addr]), cfg)
    }

    fn lib() -> (tempfile::TempDir, Library) {
        let home = tempfile::tempdir().unwrap();
        let l = Library::in_home(home.path());
        (home, l)
    }

    /// A source as the fetcher leaves it when it could keep only a pointer: a public address, an origin and a hash, no text.
    fn pointer_source(l: &Library, d: &Path, private: bool) -> String {
        let f = d.join("h.md");
        std::fs::write(&f, "x").unwrap();
        let opts = AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Pointer), private, ..Default::default() };
        let id = l.add_file(&f, &opts, &Budgets::default()).unwrap().id;
        let mut m = l.read_meta(&id).unwrap();
        m["title"] = json!("Old Harbour Notes");
        m["canonical_url"] = json!("https://old.example.org/harbour");
        m["URL"] = json!("https://old.example.org/harbour");
        m["origin"] = json!("example.org");
        m["content_hash"] = json!("sha256:live");
        m["source_class"] = json!("reputable_secondary");
        m["source_unfamiliar"] = json!(false);
        std::fs::write(l.source_dir(&id).join("meta.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        id
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    fn only_a_public_looking_address_is_ever_sent() {
        for ok in ["https://example.org/a?b=c", "http://news.example.co.uk/x#frag"] {
            assert!(public_original(ok).is_some(), "{ok}");
        }
        assert_eq!(public_original("https://example.org/a#frag").unwrap(), "https://example.org/a", "a fragment is dropped");
        assert!(public_original("https://example.org/a?q=1").unwrap().ends_with("?q=1"), "the query stays exactly as the source had it");
        for bad in ["ftp://example.org/", "https://user:pw@example.org/", "https://127.0.0.1/", "https://[::1]/", "http://localhost/x", "https://intranet/wiki", "https://wiki.corp/x", "https://printer.local/", "https://a.home.arpa/", "not a url", &format!("https://example.org/{}", "a".repeat(2100))] {
            assert!(public_original(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_request_carries_the_address_and_nothing_else() {
        let u = lookup_url(DEFAULT_ENDPOINT, "https://example.org/a?q=1&r=2").unwrap();
        assert!(u.starts_with("https://archive.org/wayback/available?url="), "{u}");
        assert_eq!(Url::parse(&u).unwrap().query_pairs().count(), 1, "one parameter: the address, percent-encoded");
        assert_eq!(copy_url(DEFAULT_RAW_BASE, "20150102000131", "https://example.org/a"), "https://web.archive.org/web/20150102000131id_/https://example.org/a");
    }

    #[test]
    fn only_the_documented_fields_of_the_answer_are_read() {
        let ok = br#"{"archived_snapshots":{"closest":{"available":true,"url":"http://web.archive.org/x","timestamp":"20150102000131","status":"200"}}}"#;
        assert_eq!(parse_availability(ok).unwrap().as_deref(), Some("20150102000131"));
        assert_eq!(parse_availability(br#"{"archived_snapshots":{}}"#).unwrap(), None);
        // a copy that was not a 200, or is marked unavailable, is none
        assert_eq!(parse_availability(br#"{"archived_snapshots":{"closest":{"available":true,"timestamp":"20150102000131","status":"404"}}}"#).unwrap(), None);
        assert_eq!(parse_availability(br#"{"archived_snapshots":{"closest":{"available":false,"timestamp":"20150102000131","status":"200"}}}"#).unwrap(), None);
        // an answer that does not read is an error, never a guess
        for bad in [&b"not json"[..], br#"{"x":1}"#, br#"{"archived_snapshots":{"closest":{"available":true,"timestamp":"2015","status":"200"}}}"#, br#"{"archived_snapshots":{"closest":{"available":true,"timestamp":"20151302000131","status":"200"}}}"#] {
            assert!(parse_availability(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
        assert_eq!(rfc3339_of("20150102000131"), "2015-01-02T00:01:31Z");
    }

    #[test]
    fn a_copy_is_stored_as_a_labelled_version_of_the_source_with_the_originals_origin() {
        let (d, l) = lib();
        let id = pointer_source(&l, d.path(), false);
        let (_, seen, policy, cfg) = archive(true);
        let out = rt().block_on(l.add_archived_copy(&policy, &cfg, &id, &Budgets::default(), true));
        assert_eq!(out, ArchiveOutcome::Captured { version: 2, archived_at: "2015-01-02T00:01:31Z".into() }, "{out:?}");
        let m = l.read_meta(&id).unwrap();
        assert_eq!(m["archived_copy"]["live_url"], "https://old.example.org/harbour");
        assert_eq!(m["archived_copy"]["archived_at"], "2015-01-02T00:01:31Z");
        assert_eq!(m["origin"], "example.org", "a copy shares the origin of the page it copies, not the archive's");
        assert_eq!(m["canonical_url"], "https://old.example.org/harbour");
        let text = std::fs::read_to_string(l.source_dir(&id).join("source.md")).unwrap();
        assert!(text.starts_with("# Archived copy from 2015-01-02: Old Harbour Notes\n"), "the H1 every chunk inherits: {text}");
        assert!(text.contains("not the live page") && text.contains("https://old.example.org/harbour") && text.contains("The harbour wall was built in 1843"), "{text}");
        // the address of the copy was built from the timestamp and the source's own URL, not from the answer's `url`
        let paths = seen.lock().unwrap().clone();
        assert!(paths.iter().any(|p| p.starts_with("/web/20150102000131id_/https://old.example.org/harbour")), "{paths:?}");
        assert!(!paths.iter().any(|p| p.contains("user:pass")), "credentials from the answer are never used: {paths:?}");
        assert!(l.indexable().iter().any(|(i, _)| i == &id), "it is indexed like any captured page");
    }

    #[test]
    fn a_private_source_a_missing_one_and_a_source_the_daemon_never_fetched_send_nothing() {
        let (d, l) = lib();
        let (_, seen, policy, cfg) = archive(true);
        let private = pointer_source(&l, d.path(), true);
        let o = rt().block_on(l.add_archived_copy(&policy, &cfg, &private, &Budgets::default(), false));
        assert!(matches!(o, ArchiveOutcome::Refused(_)), "{o:?}");
        assert!(matches!(rt().block_on(l.add_archived_copy(&policy, &cfg, "no-such-source", &Budgets::default(), false)), ArchiveOutcome::Refused(_)));
        // a file the operator added has no address the daemon fetched
        let f = d.path().join("own.md");
        std::fs::write(&f, "a note").unwrap();
        let own = l.add_file(&f, &AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Pointer), ..Default::default() }, &Budgets::default()).unwrap().id;
        assert!(matches!(rt().block_on(l.add_archived_copy(&policy, &cfg, &own, &Budgets::default(), false)), ArchiveOutcome::Refused(_)));
        // a source whose record cannot be read is treated as private
        let bad = d.path().join("library/sources/broken");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("meta.json"), "not json").unwrap();
        assert!(matches!(rt().block_on(l.add_archived_copy(&policy, &cfg, "broken", &Budgets::default(), false)), ArchiveOutcome::Refused(_)));
        assert!(seen.lock().unwrap().is_empty(), "zero requests reached the archive: {:?}", seen.lock().unwrap());
        // the explicit command refuses a private capture and a non-public address too
        let o = rt().block_on(l.add_archived_url(&policy, &cfg, "https://old.example.org/harbour", &AddOpts { private: true, ..Default::default() }, &Budgets::default()));
        assert!(matches!(o, ArchiveOutcome::Refused(_)));
        assert!(matches!(rt().block_on(l.add_archived_url(&policy, &cfg, "http://intranet/wiki", &AddOpts::default(), &Budgets::default())), ArchiveOutcome::Refused(_)));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn with_the_switch_off_the_library_does_not_look_on_its_own() {
        let (d, l) = lib();
        let id = pointer_source(&l, d.path(), false);
        let (_, seen, policy, mut cfg) = archive(true);
        cfg.enabled = false;
        let o = rt().block_on(l.add_archived_copy(&policy, &cfg, &id, &Budgets::default(), true));
        assert!(matches!(o, ArchiveOutcome::Refused(why) if why.contains("archive_fallback")), "{o:?}");
        assert!(seen.lock().unwrap().is_empty(), "off sends nothing");
        // the operator's own command works either way
        let o = rt().block_on(l.add_archived_copy(&policy, &cfg, &id, &Budgets::default(), false));
        assert!(matches!(o, ArchiveOutcome::Captured { .. }), "{o:?}");
    }

    #[test]
    fn no_copy_a_readable_capture_and_a_second_copy_change_nothing() {
        let (d, l) = lib();
        let id = pointer_source(&l, d.path(), false);
        let (_, _, policy, cfg) = archive(false);
        assert_eq!(rt().block_on(l.add_archived_copy(&policy, &cfg, &id, &Budgets::default(), true)), ArchiveOutcome::NoCopy);
        assert!(l.read_meta(&id).unwrap()["archived_copy"].is_null(), "nothing was stored");
        // a source whose text is held needs no copy, and is not even looked up
        let f = d.path().join("held.md");
        std::fs::write(&f, "# Held\n\nSome words here.\n").unwrap();
        let held = l.add_file(&f, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
        let mut m = l.read_meta(&held).unwrap();
        m["canonical_url"] = json!("https://held.example.org/p");
        m["content_hash"] = json!("sha256:held");
        std::fs::write(l.source_dir(&held).join("meta.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        let (_, seen, policy2, cfg2) = archive(true);
        assert!(matches!(rt().block_on(l.add_archived_copy(&policy2, &cfg2, &held, &Budgets::default(), true)), ArchiveOutcome::NotNeeded(_)));
        assert!(seen.lock().unwrap().is_empty());
        // once a copy is the current version, asking again is not needed
        let (_, _, policy3, cfg3) = archive(true);
        let id2 = pointer_source(&l, d.path(), false);
        assert_eq!(id2, id);
        assert!(matches!(rt().block_on(l.add_archived_copy(&policy3, &cfg3, &id, &Budgets::default(), true)), ArchiveOutcome::Captured { .. }));
        assert!(matches!(rt().block_on(l.add_archived_copy(&policy3, &cfg3, &id, &Budgets::default(), true)), ArchiveOutcome::NotNeeded(_)));
    }

    #[test]
    fn a_url_the_operator_names_becomes_a_new_source_whose_origin_is_the_pages() {
        let (_d, l) = lib();
        let (_, _, policy, cfg) = archive(true);
        let opts = AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() };
        let o = rt().block_on(l.add_archived_url(&policy, &cfg, "https://old.example.org/harbour#top", &opts, &Budgets::default()));
        assert_eq!(o, ArchiveOutcome::Captured { version: 1, archived_at: "2015-01-02T00:01:31Z".into() }, "{o:?}");
        let id = source_id("https://old.example.org/harbour");
        let m = l.read_meta(&id).unwrap();
        assert_eq!((m["origin"].as_str(), m["canonical_url"].as_str()), (Some("example.org"), Some("https://old.example.org/harbour")));
        assert!(m["archived_copy"].is_object());
        assert!(std::fs::read_to_string(l.source_dir(&id).join("source.md")).unwrap().contains("Archived copy from 2015-01-02"));
    }

    #[test]
    fn the_outcome_line_never_carries_page_text() {
        for o in [ArchiveOutcome::NoCopy, ArchiveOutcome::Refused("x"), ArchiveOutcome::NotNeeded("y"), ArchiveOutcome::Unavailable("the lookup failed: network error or timeout".into()), ArchiveOutcome::Captured { version: 2, archived_at: "2015-01-02T00:01:31Z".into() }] {
            assert!(o.line().starts_with("archive: "), "{}", o.line());
        }
        assert_eq!(ArchiveOutcome::Unavailable("z".into()).line(), "archive: unavailable (z)");
    }
}
