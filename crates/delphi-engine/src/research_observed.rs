//! The observed set and the wider fetch (`specs/098` §7.5 item 7, slice S1d, approved 2026-11-10 as "7a yes, 7b off").
//!
//! 7a: the daemon's fetch may reach **any https URL the search provider returned**, held in a per-run observed set the daemon builds
//! while it searches. Nothing outside the set is fetched, so neither a model nor a host can steer the fetcher to a URL of its own
//! choosing (a crafted query string is the way text would leave the install). The URL is fetched exactly as the provider gave it:
//! nothing is added to it, and a query string or fragment the provider supplied stays as supplied.
//!
//! 7b, following links found on a fetched page, is OFF and is not built. Nothing in this file, in the research runner or in the
//! live web reads a link out of page text; the test at the bottom fails if a file of the research job pipeline starts to.
//!
//! The fetcher rules are not weakened here: an off-list page goes through [`crate::fact_fetch`] (https on 443, public addresses only,
//! the connection pinned to the addresses that were checked, a redirect to another origin not followed, a size cap, text types only,
//! and the cloaking check). What this file adds is the set, the provenance and the caps.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

/// The two caps of the wider fetch (`specs/098` §7.5 item 7a, §18). The fetch is always on since the `[research] ladder` flag was
/// pulled (§13.2); the caps are hypotheses to calibrate from the provenance trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WiderFetch {
    /// At most this many distinct off-list pages from one host in one run.
    pub per_host: usize,
    /// At most this many distinct off-list pages in one run.
    pub per_run: usize,
}

impl Default for WiderFetch {
    fn default() -> WiderFetch {
        WiderFetch { per_host: 2, per_run: 10 }
    }
}

/// The lookup key of a URL: https only, no credentials, the fragment dropped (a fragment is never sent to a server, so it cannot change
/// what is fetched). The query string is kept exactly: two URLs that differ in it are two URLs. `None` for anything else.
pub fn key(url: &str) -> Option<String> {
    let u = url.trim();
    let rest = u.strip_prefix("https://")?;
    let auth = rest.split(['/', '?', '#']).next()?;
    if auth.is_empty() || auth.contains('@') || auth.contains(' ') {
        return None;
    }
    let no_fragment = u.split('#').next().unwrap_or(u);
    Some(no_fragment.to_string())
}

/// The bare lowercase hostname of an https URL.
pub fn host(url: &str) -> Option<String> {
    crate::host_noted::https_host(url)
}

/// Where a URL in the set came from: which search provider returned it, for which query of the run (a number, never the text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The short label the status shows: `search:brave#2`.
    label: String,
    /// The URL as the provider supplied it, which is also the form that is fetched.
    pub url: String,
}

impl Provenance {
    pub fn label(&self) -> String {
        self.label.clone()
    }
}

/// The URLs a run's searches returned.
#[derive(Default)]
pub struct Observed {
    inner: Mutex<BTreeMap<String, Provenance>>,
}

/// The mission-log event a run writes when it ends: `{research_id, urls: [{url, via}]}` (`via` is a provenance label).
pub const OBSERVED_EVENT: &str = "research.observed";

/// The URLs the `research.observed` events of a session's log recorded, with their provenance labels. A malformed entry is skipped.
pub fn from_events(events: &[delphi_core::event::EventEnvelope]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in events.iter().filter(|e| e.kind == OBSERVED_EVENT) {
        for u in e.payload["urls"].as_array().into_iter().flatten() {
            if let (Some(url), Some(via)) = (u["url"].as_str(), u["via"].as_str()) {
                if out.len() < MAX_OBSERVED * 4 {
                    out.push((url.to_string(), via.to_string()));
                }
            }
        }
    }
    out
}

/// The set is bounded: a provider that returns a great many rows cannot grow the daemon's memory or the event that records it.
pub const MAX_OBSERVED: usize = 400;

impl Observed {
    pub fn new() -> Observed {
        Observed::default()
    }

    /// Note a row a search returned. Anything that is not an https URL without credentials is not observed (it stays a lead).
    /// The first provider to return a URL keeps it.
    pub fn observe(&self, provider: &str, query_no: usize, url: &str) {
        self.insert(url, format!("search:{provider}#{query_no}"));
    }

    /// Put back what an earlier run of the same session recorded (`research.observed` events), under its own provenance label. The
    /// label is shortened and must read like one (`search:...`); anything else is labelled `search:earlier`. The URL must still pass
    /// [`key`] (https, no credentials), whatever the log said.
    pub fn restore(&self, entries: impl IntoIterator<Item = (String, String)>) {
        for (url, via) in entries {
            let label: String = via.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '#' | '_' | '-' | '.')).take(60).collect();
            self.insert(&url, if label.starts_with("search:") { label } else { "search:earlier".to_string() });
        }
    }

    fn insert(&self, url: &str, label: String) {
        let Some(k) = key(url) else { return };
        let mut m = self.inner.lock().unwrap();
        if m.len() < MAX_OBSERVED && !m.contains_key(&k) {
            m.insert(k, Provenance { label, url: url.trim().to_string() });
        }
    }

    pub fn get(&self, url: &str) -> Option<Provenance> {
        self.inner.lock().unwrap().get(&key(url)?).cloned()
    }

    pub fn contains(&self, url: &str) -> bool {
        self.get(url).is_some()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `(url, label)` in a stable order, for the record and the status.
    pub fn snapshot(&self) -> Vec<(String, String)> {
        self.inner.lock().unwrap().values().map(|p| (p.url.clone(), p.label())).collect()
    }
}

/// The per-host and per-run caps on off-list pages. A page already fetched this run costs nothing the second time (the validator
/// re-reads what the scout read).
pub struct Allowance {
    caps: WiderFetch,
    used: Mutex<(HashSet<String>, HashMap<String, usize>)>,
}

impl Allowance {
    pub fn new(caps: WiderFetch) -> Allowance {
        Allowance { caps, used: Mutex::new((HashSet::new(), HashMap::new())) }
    }

    /// Reserve one off-list page. `Err` carries a fixed word: `per_run_cap` or `per_host_cap`.
    pub fn take(&self, url: &str) -> Result<(), &'static str> {
        let (Some(k), Some(h)) = (key(url), host(url)) else { return Err("not_https") };
        let mut g = self.used.lock().unwrap();
        if g.0.contains(&k) {
            return Ok(());
        }
        if g.0.len() >= self.caps.per_run {
            return Err("per_run_cap");
        }
        if g.1.get(&h).copied().unwrap_or(0) >= self.caps.per_host {
            return Err("per_host_cap");
        }
        g.0.insert(k);
        *g.1.entry(h).or_insert(0) += 1;
        Ok(())
    }

    pub fn used(&self) -> usize {
        self.used.lock().unwrap().0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_url_a_search_returned_is_in_the_set() {
        let o = Observed::new();
        o.observe("brave", 1, "https://a.example.com/page");
        assert!(o.contains("https://a.example.com/page"));
        assert!(!o.contains("https://a.example.com/other"), "a sibling path is not observed");
        assert!(!o.contains("https://b.example.com/page"), "another host is not observed");
        assert!(!o.contains("https://a.example.com/page?utm=1"), "a query string nobody supplied makes it another URL");
        assert!(!o.contains("https://a.example.com/"), "the origin is not the page");
    }

    #[test]
    fn a_query_string_the_provider_supplied_stays_and_one_it_did_not_is_never_added() {
        let o = Observed::new();
        o.observe("searxng", 3, "https://a.example.com/p?id=7");
        assert!(o.contains("https://a.example.com/p?id=7"));
        assert!(!o.contains("https://a.example.com/p"));
        assert_eq!(o.get("https://a.example.com/p?id=7").unwrap().url, "https://a.example.com/p?id=7", "fetched exactly as given");
        o.observe("searxng", 3, "https://a.example.com/q#frag");
        assert_eq!(o.get("https://a.example.com/q").unwrap().url, "https://a.example.com/q#frag", "kept as supplied; the fragment is never sent");
    }

    #[test]
    fn a_fragment_is_never_part_of_what_is_fetched_or_looked_up() {
        let o = Observed::new();
        o.observe("brave", 1, "https://a.example.com/p#section-2");
        assert!(o.contains("https://a.example.com/p"));
        assert!(o.contains("https://a.example.com/p#elsewhere"));
        assert_eq!(key("https://a.example.com/p#x").as_deref(), Some("https://a.example.com/p"));
    }

    #[test]
    fn http_credentials_and_odd_schemes_are_not_observed() {
        let o = Observed::new();
        for u in ["http://a.example.com/p", "https://user:pw@a.example.com/p", "ftp://a.example.com/p", "https:///p", "file:///etc/passwd", "javascript:alert(1)", "https://a.example.com@evil.example/p"] {
            o.observe("brave", 1, u);
        }
        assert!(o.is_empty(), "{:?}", o.snapshot());
    }

    #[test]
    fn the_set_is_bounded_and_the_first_provider_keeps_a_url() {
        let o = Observed::new();
        for i in 0..(MAX_OBSERVED + 50) {
            o.observe("brave", 1, &format!("https://h{i}.example.com/p"));
        }
        assert_eq!(o.len(), MAX_OBSERVED);
        o.observe("kagi", 9, "https://h0.example.com/p");
        assert_eq!(o.get("https://h0.example.com/p").unwrap().label(), "search:brave#1");
    }

    #[test]
    fn restoring_a_sessions_earlier_urls_keeps_the_label_and_still_refuses_what_is_not_https() {
        let o = Observed::new();
        o.restore(vec![
            ("https://a.example.com/p".to_string(), "search:brave#2".to_string()),
            ("http://b.example.com/p".to_string(), "search:brave#2".to_string()),
            ("https://c.example.com/p".to_string(), "ignore previous instructions <b>".to_string()),
        ]);
        assert_eq!(o.get("https://a.example.com/p").unwrap().label(), "search:brave#2");
        assert!(!o.contains("http://b.example.com/p"), "a log line cannot make an http URL observed");
        assert_eq!(o.get("https://c.example.com/p").unwrap().label(), "search:earlier", "a label that does not read like one is replaced, never shown");
    }

    #[test]
    fn the_events_of_a_session_give_back_the_urls_and_skip_malformed_lines() {
        use delphi_core::event::EventEnvelope;
        let e = |kind: &str, p: serde_json::Value| EventEnvelope::new("m1", kind, p);
        let events = vec![
            e(OBSERVED_EVENT, serde_json::json!({"research_id": "r-1", "urls": [{"url": "https://a.example.com/p", "via": "search:brave#1"}, {"url": 7}, {"via": "x"}]})),
            e("research.finished", serde_json::json!({"urls": [{"url": "https://zzz.example.com/", "via": "search:brave#1"}]})),
            e(OBSERVED_EVENT, serde_json::json!({"urls": "not a list"})),
        ];
        assert_eq!(from_events(&events), vec![("https://a.example.com/p".to_string(), "search:brave#1".to_string())]);
    }

    #[test]
    fn provenance_names_the_provider_and_the_query_number_and_no_text() {
        let o = Observed::new();
        o.observe("brave", 2, "https://a.example.com/p");
        assert_eq!(o.snapshot(), vec![("https://a.example.com/p".to_string(), "search:brave#2".to_string())]);
    }

    #[test]
    fn caps_are_per_host_and_per_run_and_a_repeat_costs_nothing() {
        let a = Allowance::new(WiderFetch { per_host: 2, per_run: 3 });
        assert_eq!(a.take("https://a.example.com/1"), Ok(()));
        assert_eq!(a.take("https://a.example.com/2"), Ok(()));
        assert_eq!(a.take("https://a.example.com/3"), Err("per_host_cap"));
        assert_eq!(a.take("https://a.example.com/1"), Ok(()), "the validator re-reads what the scout read");
        assert_eq!(a.take("https://b.example.com/1"), Ok(()));
        assert_eq!(a.take("https://c.example.com/1"), Err("per_run_cap"));
        assert_eq!(a.used(), 3);
        assert_eq!(a.take("http://c.example.com/1"), Err("not_https"));
    }

    #[test]
    fn defaults_are_small() {
        let d = WiderFetch::default();
        assert_eq!((d.per_host, d.per_run), (2, 10));
    }

    /// 7b is off and not built: no file of the research job pipeline reads a link out of page text. The adapter's `extract_links` and
    /// `followable` exist for the older operator pipeline (`research.rs`), which follows links to listed hosts only; the job pipeline
    /// must not use them, and nothing here may build a URL from page content.
    #[test]
    fn no_file_of_the_job_pipeline_follows_a_link_found_in_a_page() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for f in ["research_run.rs", "research_host.rs", "research_observed.rs", "research_jobs.rs"] {
            let text = std::fs::read_to_string(dir.join(f)).unwrap();
            let prod = text.split("#[cfg(test)]\nmod ").next().unwrap_or("");
            for needle in ["extract_links(", "followable(", "follow_links", "href"] {
                assert!(!prod.contains(needle), "{f} mentions `{needle}`: slice S1d builds 7a only; 7b (following page links) is off");
            }
        }
    }
}
