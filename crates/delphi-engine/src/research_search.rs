//! The search client of the research job (`specs/028` §6, §10.8 Q2): one small client over a few engines, returning
//! `{title, url, snippet}` rows. It spends no model tokens.
//!
//! * `[search] providers` is an ordered list. A query goes to **one engine at a time**: the next is tried only when the
//!   previous failed (a network error, a rate limit, a missing key, an answer that is not what it should be). A query
//!   never goes to several engines at once, so it leaves the machine to one third party per job.
//! * Only engines with a documented API are here: Brave Search, Mojeek, a SearXNG instance the operator runs, and Google's
//!   Programmable Search (allowed by configuration only when a Gemini endpoint is set, since Google then already sees the
//!   operator's prompts). Nothing is scraped.
//! * Whatever engine answered, the **rows are filtered by the allow list in code** after they return
//!   ([`filter_rows`]): the job reads only listed hosts, and the engine's opinion of a site decides nothing.
//! * A key is resolved by the caller (the same secret handling as model endpoints, `specs/009`) and given here as text; it
//!   goes only into the request, and never into an error message.

use crate::research_fetch::{Http, HttpError};
use crate::research_sources::{https_host, Allow, Entry};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Brave,
    Mojeek,
    Searxng,
    Google,
}

impl Kind {
    pub const NAMES: [&'static str; 4] = ["brave", "mojeek", "searxng", "google"];
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "brave" => Some(Kind::Brave),
            "mojeek" => Some(Kind::Mojeek),
            "searxng" => Some(Kind::Searxng),
            "google" => Some(Kind::Google),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        Kind::NAMES[self as usize]
    }
}

/// One engine as configured: its kind, its key (resolved), and where it lives.
#[derive(Debug, Clone)]
pub struct Provider {
    pub kind: Kind,
    pub key: Option<String>,
    /// A SearXNG instance's base URL, or an override of an engine's endpoint (tests).
    pub base: Option<String>,
    /// Google's search-engine id.
    pub cx: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// No engine is configured: the search is off (`specs/028` §6).
    NotConfigured,
    /// Every engine failed; one line each, in the order tried, with no key in it.
    AllFailed(Vec<String>),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchError::NotConfigured => write!(f, "no search provider is configured (add a [search] section to config.toml)"),
            SearchError::AllFailed(v) => write!(f, "no search engine answered: {}", v.join("; ")),
        }
    }
}

/// Percent-encode a query value (RFC 3986 unreserved characters stay).
pub fn encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{:02X}", b));
        }
    }
    o
}

/// The request for one engine: a URL and headers.
pub fn request(p: &Provider, query: &str) -> Result<(String, Vec<(String, String)>), String> {
    let q = encode(query);
    let need_key = || p.key.clone().filter(|k| !k.is_empty()).ok_or_else(|| format!("{} needs a key", p.kind.name()));
    match p.kind {
        Kind::Brave => {
            let base = p.base.clone().unwrap_or_else(|| "https://api.search.brave.com/res/v1/web/search".into());
            Ok((format!("{}?q={}&count=20", base, q), vec![("x-subscription-token".into(), need_key()?), ("accept".into(), "application/json".into())]))
        }
        Kind::Mojeek => {
            let base = p.base.clone().unwrap_or_else(|| "https://api.mojeek.com/search".into());
            Ok((format!("{}?q={}&fmt=json&t=20&api_key={}", base, q, encode(&need_key()?)), vec![("accept".into(), "application/json".into())]))
        }
        Kind::Searxng => {
            let base = p.base.clone().ok_or_else(|| "searxng needs a url".to_string())?;
            Ok((format!("{}/search?q={}&format=json", base.trim_end_matches('/'), q), vec![("accept".into(), "application/json".into())]))
        }
        Kind::Google => {
            let base = p.base.clone().unwrap_or_else(|| "https://www.googleapis.com/customsearch/v1".into());
            let cx = p.cx.clone().filter(|c| !c.is_empty()).ok_or_else(|| "google needs a cx (search engine id)".to_string())?;
            Ok((format!("{}?key={}&cx={}&q={}&num=10", base, encode(&need_key()?), encode(&cx), q), vec![("accept".into(), "application/json".into())]))
        }
    }
}

static EMPTY: Value = Value::Array(Vec::new());

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The rows of an engine's JSON answer; `None` when the answer is not the shape that engine returns.
pub fn parse_rows(kind: Kind, body: &str) -> Option<Vec<Row>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let (list, title, url, snippet) = match kind {
        Kind::Brave => (v.pointer("/web/results")?, "title", "url", "description"),
        Kind::Mojeek => (v.pointer("/response/results")?, "title", "url", "desc"),
        Kind::Searxng => (v.get("results")?, "title", "url", "content"),
        Kind::Google => (v.get("items").unwrap_or(&EMPTY), "title", "link", "snippet"),
    };
    Some(list.as_array()?.iter().map(|r| Row { title: s(&r[title]), url: s(&r[url]), snippet: s(&r[snippet]) }).filter(|r| !r.url.is_empty()).collect())
}

/// Search with the configured engines, one at a time, in order. Returns the rows of the first engine that answers and its
/// name. An engine that answers with no rows still answered: the next one is not asked.
pub fn search(providers: &[Provider], query: &str, http: &dyn Http, timeout: Duration) -> Result<(Vec<Row>, &'static str), SearchError> {
    if providers.is_empty() {
        return Err(SearchError::NotConfigured);
    }
    let mut failed = Vec::new();
    for p in providers {
        let (url, headers) = match request(p, query) {
            Ok(r) => r,
            Err(e) => {
                failed.push(format!("{}: {}", p.kind.name(), e));
                continue;
            }
        };
        let hs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        match http.get(&url, &hs, 1024 * 1024, timeout) {
            Ok(r) if r.status == 200 => match parse_rows(p.kind, &String::from_utf8_lossy(&r.body)) {
                Some(rows) => return Ok((rows, p.kind.name())),
                None => failed.push(format!("{}: an answer in an unexpected shape", p.kind.name())),
            },
            Ok(r) if r.status == 429 => failed.push(format!("{}: rate limited", p.kind.name())),
            Ok(r) if r.status == 401 || r.status == 403 => failed.push(format!("{}: the key was refused", p.kind.name())),
            Ok(r) => failed.push(format!("{}: HTTP {}", p.kind.name(), r.status)),
            Err(HttpError::Timeout) => failed.push(format!("{}: timed out", p.kind.name())),
            Err(HttpError::TooLarge) => failed.push(format!("{}: the answer was too large", p.kind.name())),
            Err(HttpError::Network(_)) => failed.push(format!("{}: could not be reached", p.kind.name())),
        }
    }
    Err(SearchError::AllFailed(failed))
}

/// Site-restricted queries for a topic: the topic with `(site:a OR site:b …)` over the exact listed hosts, in chunks so a
/// query stays short. Wildcard entries are not named (a search engine cannot be asked for `*.gov` portably); their pages are
/// found by the unrestricted topic query and kept by [`filter_rows`].
pub fn site_queries(topic: &str, hosts: &[&str], per_query: usize, max_queries: usize) -> Vec<String> {
    let topic = topic.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: Vec<String> = hosts.chunks(per_query.max(1)).take(max_queries.max(1)).map(|c| format!("{} ({})", topic, c.iter().map(|h| format!("site:{}", h)).collect::<Vec<_>>().join(" OR "))).collect();
    if out.is_empty() {
        out.push(topic);
    }
    out
}

/// Keep the rows whose URL the allow list allows, once each, and say how many were skipped. This is where "no open-web
/// result enters the job" is enforced.
pub fn filter_rows(rows: Vec<Row>, allow: &Allow) -> (Vec<(Row, Entry)>, usize) {
    let mut kept: Vec<(Row, Entry)> = Vec::new();
    let mut skipped = 0;
    for r in rows {
        let key = https_host(&r.url).map(|(h, p)| format!("{}{}", h, p.split('#').next().unwrap_or("")));
        match (allow.lookup(&r.url), key) {
            (Some(e), Some(k)) if !kept.iter().any(|(x, _)| https_host(&x.url).map(|(h, p)| format!("{}{}", h, p.split('#').next().unwrap_or(""))).as_deref() == Some(k.as_str())) => kept.push((r, e.clone())),
            (Some(_), _) => {}
            (None, _) => skipped += 1,
        }
    }
    (kept, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research_fetch::tests::Fake;

    fn p(kind: Kind, key: Option<&str>) -> Provider {
        Provider { kind, key: key.map(String::from), base: Some(format!("https://{}.test/api", kind.name())), cx: Some("cx1".into()) }
    }
    const BRAVE: &str = r#"{"web":{"results":[{"title":"BFT  overview","url":"https://en.wikipedia.org/wiki/BFT","description":"A fault\n tolerance"},{"title":"Spam","url":"https://spam.example.com/x","description":"buy"},{"title":"","url":"","description":""}]}}"#;

    #[test]
    fn each_engine_builds_its_request_and_never_puts_a_missing_key_on_the_wire() {
        let (u, h) = request(&p(Kind::Brave, Some("K")), "a b&c").unwrap();
        assert_eq!(u, "https://brave.test/api?q=a%20b%26c&count=20");
        assert!(h.contains(&("x-subscription-token".to_string(), "K".to_string())));
        assert!(request(&p(Kind::Mojeek, Some("K y")), "q").unwrap().0.ends_with("&api_key=K%20y"));
        assert_eq!(request(&Provider { kind: Kind::Searxng, key: None, base: Some("https://s.example.org/".into()), cx: None }, "q").unwrap().0, "https://s.example.org/search?q=q&format=json");
        assert!(request(&p(Kind::Google, Some("K")), "q").unwrap().0.contains("cx=cx1&q=q"));
        for bad in [p(Kind::Brave, None), p(Kind::Mojeek, Some("")), Provider { kind: Kind::Searxng, key: None, base: None, cx: None }, Provider { kind: Kind::Google, key: Some("K".into()), base: None, cx: None }] {
            assert!(request(&bad, "q").is_err(), "{:?}", bad.kind);
        }
        assert_eq!(Kind::parse("kagi"), None);
        assert_eq!(Kind::Searxng.name(), "searxng");
    }

    #[test]
    fn each_engines_answer_becomes_rows_and_a_wrong_shape_is_none() {
        let rows = parse_rows(Kind::Brave, BRAVE).unwrap();
        assert_eq!(rows.len(), 2, "a row with no url is dropped");
        assert_eq!((rows[0].title.as_str(), rows[0].snippet.as_str()), ("BFT overview", "A fault tolerance"), "whitespace is collapsed");
        assert_eq!(parse_rows(Kind::Mojeek, r#"{"response":{"results":[{"title":"t","url":"https://a.org/","desc":"d"}]}}"#).unwrap()[0].snippet, "d");
        assert_eq!(parse_rows(Kind::Searxng, r#"{"results":[{"title":"t","url":"https://a.org/","content":"c"}]}"#).unwrap()[0].snippet, "c");
        assert_eq!(parse_rows(Kind::Google, r#"{"items":[{"title":"t","link":"https://a.org/","snippet":"s"}]}"#).unwrap()[0].url, "https://a.org/");
        assert_eq!(parse_rows(Kind::Google, "{}").unwrap(), vec![], "Google omits `items` for no results");
        assert!(parse_rows(Kind::Brave, "not json").is_none() && parse_rows(Kind::Brave, r#"{"other":1}"#).is_none() && parse_rows(Kind::Searxng, "[]").is_none());
    }

    #[test]
    fn engines_are_tried_one_at_a_time_in_order_and_only_after_a_failure() {
        let http = Fake::default().page("https://brave.test/api?q=bft&count=20", 429, "application/json", "").page("https://mojeek.test/api?q=bft&fmt=json&t=20&api_key=M", 200, "application/json", r#"{"response":{"results":[{"title":"t","url":"https://en.wikipedia.org/wiki/BFT","desc":"d"}]}}"#);
        let (rows, who) = search(&[p(Kind::Brave, Some("B")), p(Kind::Mojeek, Some("M")), p(Kind::Searxng, None)], "bft", &http, Duration::from_secs(5)).unwrap();
        assert_eq!((who, rows.len()), ("mojeek", 1));
        assert_eq!(http.asked.lock().unwrap().len(), 2, "brave failed, mojeek answered, searxng was never asked");
        // An engine that answers with nothing still answered.
        let empty = Fake::default().page("https://brave.test/api?q=zzz&count=20", 200, "application/json", r#"{"web":{"results":[]}}"#);
        let (rows, who) = search(&[p(Kind::Brave, Some("B")), p(Kind::Mojeek, Some("M"))], "zzz", &empty, Duration::from_secs(5)).unwrap();
        assert_eq!((who, rows.len(), empty.asked.lock().unwrap().len()), ("brave", 0, 1));
    }

    #[test]
    fn when_every_engine_fails_the_reasons_come_back_without_a_key_in_them() {
        let http = Fake::default().page("https://brave.test/api?q=q&count=20", 401, "", "secret-key-echo").page("https://searxng.test/api/search?q=q&format=json", 200, "application/json", "<html>oops</html>");
        let providers = [p(Kind::Brave, Some("secret-key-echo")), p(Kind::Mojeek, None), Provider { kind: Kind::Searxng, key: None, base: Some("https://searxng.test/api".into()), cx: None }, p(Kind::Google, Some("g"))];
        let err = search(&providers, "q", &http, Duration::from_secs(5)).unwrap_err();
        let text = err.to_string();
        for want in ["brave: the key was refused", "mojeek: mojeek needs a key", "searxng: an answer in an unexpected shape", "google: could not be reached"] {
            assert!(text.contains(want), "{want}\n{text}");
        }
        assert!(!text.contains("secret-key-echo"), "a key never reaches an error message");
        assert_eq!(search(&[], "q", &http, Duration::from_secs(5)).unwrap_err(), SearchError::NotConfigured);
        assert!(SearchError::NotConfigured.to_string().contains("[search]"));
    }

    #[test]
    fn site_queries_chunk_the_listed_hosts_and_never_exceed_the_cap() {
        let hosts = ["a.org", "b.org", "c.org", "d.org", "e.org"];
        assert_eq!(site_queries("  byzantine   fault ", &hosts, 2, 3), ["byzantine fault (site:a.org OR site:b.org)", "byzantine fault (site:c.org OR site:d.org)", "byzantine fault (site:e.org)"]);
        assert_eq!(site_queries("x", &hosts, 2, 1).len(), 1);
        assert_eq!(site_queries("x y", &[], 4, 3), ["x y"], "with no exact hosts the plain topic is asked");
    }

    #[test]
    fn rows_are_filtered_by_the_allow_list_after_the_engine_answers_once_each() {
        let e = |h: &str| Entry { host: h.into(), publisher: "P".into(), kind: "encyclopedia".into(), licence: "CC0-1.0".into(), reuse: "full".into(), block: false };
        let allow = Allow::union(&[e("en.wikipedia.org"), e("*.gov")], &[]);
        let row = |u: &str| Row { title: "t".into(), url: u.into(), snippet: "s".into() };
        let rows = vec![row("https://en.wikipedia.org/wiki/BFT"), row("https://spam.example.com/x"), row("https://en.wikipedia.org/wiki/BFT#History"), row("http://en.wikipedia.org/wiki/Insecure"), row("https://nist.gov/a"), row("https://nist.gov.evil.com/a")];
        let (kept, skipped) = filter_rows(rows, &allow);
        assert_eq!(kept.iter().map(|(r, _)| r.url.as_str()).collect::<Vec<_>>(), ["https://en.wikipedia.org/wiki/BFT", "https://nist.gov/a"], "a repeat (even with a fragment) is kept once");
        assert_eq!(skipped, 3, "an unlisted host, plain http and a look-alike are skipped");
        assert_eq!(kept[1].1.host, "*.gov");
    }
}
