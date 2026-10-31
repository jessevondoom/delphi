//! Web search for the research job (`specs/028` §6 and §10.8 Q2): one provider at a time, in the operator's order, the next only when
//! the last one failed. A query goes to one third party, never to several. Results from whichever engine answered are filtered by
//! the trusted lists afterwards, so no engine decides what is trusted.

use super::transport::{Response, Transport};
use super::trusted::{Source, TrustedList};
use reqwest::Url;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Parallel,
    Brave,
    Mojeek,
    Searxng,
    Kagi,
    Google,
}

impl Provider {
    pub const ALL: [Provider; 6] = [Provider::Parallel, Provider::Brave, Provider::Mojeek, Provider::Searxng, Provider::Kagi, Provider::Google];

    pub fn name(self) -> &'static str {
        match self {
            Provider::Parallel => "parallel",
            Provider::Brave => "brave",
            Provider::Mojeek => "mojeek",
            Provider::Searxng => "searxng",
            Provider::Kagi => "kagi",
            Provider::Google => "google",
        }
    }

    pub fn parse(s: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.name() == s)
    }

    /// SearXNG and Parallel are keyless; every other engine needs a key.
    pub fn needs_key(self) -> bool {
        !matches!(self, Provider::Searxng | Provider::Parallel)
    }
}

/// One engine as configured, with its key already resolved (the caller owns credential handling, `specs/009`).
#[derive(Clone)]
pub struct ProviderSpec {
    pub provider: Provider,
    pub key: Option<String>,
    /// SearXNG: the instance's base URL. Google: unused.
    pub base_url: Option<String>,
    /// Google Programmable Search: the engine id.
    pub cx: Option<String>,
}

impl std::fmt::Debug for ProviderSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSpec").field("provider", &self.provider).field("key", &self.key.as_ref().map(|_| "…")).finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// No engine is configured: the config key to set is named by the caller.
    NotConfigured,
    /// Every engine failed; each with the short reason it was skipped.
    AllFailed(Vec<(Provider, String)>),
}

#[derive(Debug, Clone)]
pub struct Found {
    pub provider: Provider,
    pub rows: Vec<Row>,
    /// The engines tried before this one, and why each was passed over.
    pub passed_over: Vec<(Provider, String)>,
}

pub const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY: usize = 2 * 1024 * 1024;
pub const RESULTS_PER_QUERY: usize = 20;

/// Parallel's hosted MCP server (`specs/028` §12): keyless and rate limited.
pub const PARALLEL_URL: &str = "https://mcp.parallel.ai/mcp";
const SNIPPET_CHARS: usize = 400;

/// A stateless MCP `tools/call` of `web_search` (no `initialize`; the server answers one call on its own).
fn parallel_body(query: &str) -> String {
    serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "web_search", "arguments": {"objective": query, "search_queries": [query]}}}).to_string()
}

fn url_with(base: &str, query: &[(&str, &str)]) -> Option<String> {
    let mut u = Url::parse(base).ok()?;
    u.query_pairs_mut().extend_pairs(query);
    Some(u.into())
}

/// The request for `query`: its URL and headers.
type Headers = Vec<(&'static str, String)>;

fn request(spec: &ProviderSpec, query: &str) -> Result<(String, Headers), String> {
    let key = || spec.key.clone().filter(|k| !k.is_empty()).ok_or_else(|| "no key".to_string());
    let n = RESULTS_PER_QUERY.to_string();
    match spec.provider {
        Provider::Brave => Ok((url_with("https://api.search.brave.com/res/v1/web/search", &[("q", query), ("count", &n)]).ok_or("bad request")?, vec![("X-Subscription-Token", key()?), ("Accept", "application/json".into())])),
        Provider::Mojeek => Ok((url_with("https://api.mojeek.com/search", &[("api_key", &key()?), ("q", query), ("fmt", "json"), ("t", &n)]).ok_or("bad request")?, vec![("Accept", "application/json".into())])),
        Provider::Kagi => Ok((url_with("https://kagi.com/api/v0/search", &[("q", query), ("limit", &n)]).ok_or("bad request")?, vec![("Authorization", format!("Bot {}", key()?)), ("Accept", "application/json".into())])),
        Provider::Google => {
            let cx = spec.cx.clone().filter(|c| !c.is_empty()).ok_or("no engine id")?;
            Ok((url_with("https://www.googleapis.com/customsearch/v1", &[("key", &key()?), ("cx", &cx), ("q", query), ("num", "10")]).ok_or("bad request")?, vec![("Accept", "application/json".into())]))
        }
        Provider::Parallel => Err("not a GET engine".into()),
        Provider::Searxng => {
            let base = spec.base_url.clone().filter(|b| !b.is_empty()).ok_or("no instance URL")?;
            let base = format!("{}/search", base.trim_end_matches('/'));
            Ok((url_with(&base, &[("q", query), ("format", "json")]).ok_or("bad instance URL")?, vec![("Accept", "application/json".into())]))
        }
    }
}

fn text(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string()
}

/// The JSON-RPC message in an MCP answer, which is plain JSON or one or more server-sent events.
fn mcp_message(body: &[u8]) -> Result<Value, String> {
    let text = String::from_utf8_lossy(body);
    let mut data = text.lines().filter_map(|l| l.strip_prefix("data:")).filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok());
    data.find(|v| v.get("result").is_some() || v.get("error").is_some()).or_else(|| serde_json::from_str(&text).ok()).ok_or_else(|| "the answer was not JSON".to_string())
}

fn parallel_rows(body: &[u8]) -> Result<Vec<Row>, String> {
    let msg = mcp_message(body)?;
    let result = msg.get("result").ok_or_else(|| "the answer was an error".to_string())?;
    if result.get("isError").and_then(Value::as_bool).unwrap_or(false) {
        return Err("the search was refused".into());
    }
    let inner = match result.get("structuredContent") {
        Some(v) if v.get("results").is_some() => v.clone(),
        _ => result.pointer("/content/0/text").and_then(Value::as_str).and_then(|t| serde_json::from_str(t).ok()).ok_or_else(|| "the answer had no results".to_string())?,
    };
    let list = inner.get("results").and_then(Value::as_array).ok_or_else(|| "the answer had no results".to_string())?;
    Ok(list
        .iter()
        .filter_map(|r| {
            let url = text(r, "url");
            let snippet: String = r.get("excerpts").and_then(Value::as_array).and_then(|e| e.first()).and_then(Value::as_str).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(SNIPPET_CHARS).collect();
            (!url.is_empty()).then(|| Row { title: text(r, "title"), url, snippet })
        })
        .collect())
}

/// The rows of one engine's JSON answer.
pub fn parse_rows(p: Provider, body: &[u8]) -> Result<Vec<Row>, String> {
    if p == Provider::Parallel {
        return parallel_rows(body);
    }
    let v: Value = serde_json::from_slice(body).map_err(|_| "the answer was not JSON".to_string())?;
    let row = |t: String, u: String, s: String| (!u.is_empty()).then_some(Row { title: t, url: u, snippet: s });
    let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().ok_or_else(|| "the answer had no results".to_string());
    let rows: Vec<Row> = match p {
        Provider::Parallel => unreachable!(),
        Provider::Brave => list(v.pointer("/web/results"))?.iter().filter_map(|r| row(text(r, "title"), text(r, "url"), text(r, "description"))).collect(),
        Provider::Mojeek => list(v.pointer("/response/results"))?.iter().filter_map(|r| row(text(r, "title"), text(r, "url"), text(r, "desc"))).collect(),
        Provider::Searxng => list(v.get("results"))?.iter().filter_map(|r| row(text(r, "title"), text(r, "url"), text(r, "content"))).collect(),
        // Kagi's list mixes results (t = 0) with related searches (t = 1).
        Provider::Kagi => list(v.get("data"))?.iter().filter(|r| r.get("t").and_then(Value::as_i64).unwrap_or(0) == 0).filter_map(|r| row(text(r, "title"), text(r, "url"), text(r, "snippet"))).collect(),
        Provider::Google => list(v.get("items")).unwrap_or_default().iter().filter_map(|r| row(text(r, "title"), text(r, "link"), text(r, "snippet"))).collect(),
    };
    Ok(rows)
}

fn reason(r: &Response) -> String {
    match r.status {
        401 | 403 => "the key was refused".into(),
        402 | 429 => "over its limit".into(),
        s if s >= 500 => "the service is down".into(),
        s => format!("answered {}", s),
    }
}

/// Ask the engines in order and return the first answer. An engine with no key, a network error, a refusal, a limit or an answer that
/// is not results is passed over for the next. An empty result list is an answer (the topic has no hits), not a failure.
pub fn search(transport: &dyn Transport, engines: &[ProviderSpec], query: &str) -> Result<Found, SearchError> {
    if engines.is_empty() {
        return Err(SearchError::NotConfigured);
    }
    let mut passed_over = Vec::new();
    for spec in engines {
        let attempt = if spec.provider == Provider::Parallel {
            transport.post_json(PARALLEL_URL, &[("Accept", "application/json, text/event-stream")], &parallel_body(query), MAX_BODY, TIMEOUT)
        } else {
            request(spec, query).and_then(|(url, headers)| {
                let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
                transport.get(&url, &h, MAX_BODY, TIMEOUT)
            })
        };
        match attempt {
            Ok(r) if r.status == 200 => match parse_rows(spec.provider, &r.body) {
                Ok(rows) => return Ok(Found { provider: spec.provider, rows, passed_over }),
                Err(why) => passed_over.push((spec.provider, why)),
            },
            Ok(r) => passed_over.push((spec.provider, reason(&r))),
            Err(why) => passed_over.push((spec.provider, why)),
        }
    }
    Err(SearchError::AllFailed(passed_over))
}

/// The topic as one or more queries, each limited with `site:` filters to at most `per_query` of the trusted domains. A filter on
/// every listed domain in one query would be too long for any engine, so the job asks for as many queries as it can afford.
pub fn site_queries(topic: &str, sites: &[String], per_query: usize) -> Vec<String> {
    let topic = topic.split_whitespace().collect::<Vec<_>>().join(" ");
    sites.chunks(per_query.max(1)).map(|c| format!("{} ({})", topic, c.iter().map(|s| format!("site:{}", s)).collect::<Vec<_>>().join(" OR "))).collect()
}

/// What the lists leave of a result list: the rows on a trusted host (with the entry that governs each), those not on one, and
/// duplicates dropped. Done in code after the engine answered, whichever engine it was.
pub fn keep_trusted(rows: Vec<Row>, list: &TrustedList) -> (Vec<(Row, &Source)>, Vec<Row>) {
    let (mut kept, mut skipped, mut seen) = (Vec::new(), Vec::new(), std::collections::HashSet::new());
    for r in rows {
        if !seen.insert(r.url.clone()) {
            continue;
        }
        match list.allows_url(&r.url) {
            Some(s) => kept.push((r, s)),
            None => skipped.push(r),
        }
    }
    (kept, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A transport that answers from a script and records the URLs asked.
    struct Fake {
        script: Mutex<Vec<Result<Response, String>>>,
        asked: Mutex<Vec<String>>,
    }
    impl Fake {
        fn new(script: Vec<Result<Response, String>>) -> Fake {
            Fake { script: Mutex::new(script), asked: Mutex::new(Vec::new()) }
        }
    }
    impl Transport for Fake {
        fn get(&self, url: &str, _h: &[(&str, &str)], _m: usize, _t: Duration) -> Result<Response, String> {
            self.asked.lock().unwrap().push(url.to_string());
            self.script.lock().unwrap().remove(0)
        }
        fn post_json(&self, url: &str, h: &[(&str, &str)], _b: &str, m: usize, t: Duration) -> Result<Response, String> {
            self.get(url, h, m, t)
        }
    }
    fn ok(body: &str) -> Result<Response, String> {
        Ok(Response { status: 200, body: body.as_bytes().to_vec(), ..Default::default() })
    }
    fn status(s: u16) -> Result<Response, String> {
        Ok(Response { status: s, ..Default::default() })
    }
    fn spec(p: Provider, key: Option<&str>) -> ProviderSpec {
        ProviderSpec { provider: p, key: key.map(str::to_string), base_url: (p == Provider::Searxng).then(|| "https://searx.example.org/".into()), cx: (p == Provider::Google).then(|| "cx1".into()) }
    }
    const BRAVE: &str = r#"{"web":{"results":[{"title":"Mito","url":"https://en.wikipedia.org/wiki/Mito","description":"An organelle"},{"title":"x","url":"https://blog.example.com/a","description":"y"}]}}"#;

    #[test]
    fn the_first_engine_that_answers_wins_and_the_rest_are_not_asked() {
        let t = Fake::new(vec![ok(BRAVE)]);
        let f = search(&t, &[spec(Provider::Brave, Some("k")), spec(Provider::Mojeek, Some("k"))], "mitochondria").unwrap();
        assert_eq!((f.provider, f.rows.len(), f.passed_over.len()), (Provider::Brave, 2, 0));
        assert_eq!(t.asked.lock().unwrap().len(), 1, "one query goes to one third party");
        assert!(t.asked.lock().unwrap()[0].starts_with("https://api.search.brave.com/res/v1/web/search?q=mitochondria"));
    }

    #[test]
    fn failures_fall_through_in_order_with_a_reason_each() {
        let t = Fake::new(vec![status(429), Err("timed out".into()), ok(r#"{"results":[{"title":"a","url":"https://w3.org/x","content":"c"}]}"#)]);
        let engines = [spec(Provider::Brave, Some("k")), spec(Provider::Mojeek, Some("k")), spec(Provider::Searxng, None)];
        let f = search(&t, &engines, "q").unwrap();
        assert_eq!(f.provider, Provider::Searxng);
        assert_eq!(f.passed_over, vec![(Provider::Brave, "over its limit".to_string()), (Provider::Mojeek, "timed out".to_string())]);
        assert!(t.asked.lock().unwrap()[2].starts_with("https://searx.example.org/search?q=q&format=json"));
    }

    #[test]
    fn an_engine_without_its_key_is_skipped_without_a_request() {
        let t = Fake::new(vec![ok(BRAVE)]);
        let f = search(&t, &[spec(Provider::Kagi, None), spec(Provider::Brave, Some("k"))], "q").unwrap();
        assert_eq!(f.passed_over, vec![(Provider::Kagi, "no key".to_string())]);
        assert_eq!(t.asked.lock().unwrap().len(), 1);
    }

    #[test]
    fn all_failing_reports_every_reason_and_nothing_configured_says_so() {
        let t = Fake::new(vec![status(401), status(503)]);
        let e = search(&t, &[spec(Provider::Brave, Some("k")), spec(Provider::Mojeek, Some("k"))], "q").unwrap_err();
        assert_eq!(e, SearchError::AllFailed(vec![(Provider::Brave, "the key was refused".into()), (Provider::Mojeek, "the service is down".into())]));
        assert_eq!(search(&Fake::new(vec![]), &[], "q").unwrap_err(), SearchError::NotConfigured);
    }

    #[test]
    fn an_empty_answer_is_an_answer_but_junk_is_a_failure() {
        let t = Fake::new(vec![ok(r#"{"web":{"results":[]}}"#)]);
        assert_eq!(search(&t, &[spec(Provider::Brave, Some("k")), spec(Provider::Mojeek, Some("k"))], "q").unwrap().rows.len(), 0);
        let t = Fake::new(vec![ok("<html>"), ok(r#"{"response":{"results":[{"title":"t","url":"https://arxiv.org/abs/1","desc":"d"}]}}"#)]);
        let f = search(&t, &[spec(Provider::Brave, Some("k")), spec(Provider::Mojeek, Some("k"))], "q").unwrap();
        assert_eq!((f.provider, f.passed_over[0].1.as_str()), (Provider::Mojeek, "the answer was not JSON"));
    }

    struct Poster(Mutex<Vec<(String, String)>>, Vec<u8>);
    impl Transport for Poster {
        fn get(&self, _u: &str, _h: &[(&str, &str)], _m: usize, _t: Duration) -> Result<Response, String> {
            Err("GET not expected".into())
        }
        fn post_json(&self, u: &str, h: &[(&str, &str)], body: &str, _m: usize, _t: Duration) -> Result<Response, String> {
            assert!(h.iter().all(|(k, _)| !k.eq_ignore_ascii_case("authorization")));
            self.0.lock().unwrap().push((u.into(), body.into()));
            Ok(Response { status: 200, body: self.1.clone(), ..Default::default() })
        }
    }

    const PARALLEL_JSON: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"{}"}],"isError":false,"structuredContent":{"results":[{"url":"https://en.wikipedia.org/wiki/Mitochondrion","title":"Mitochondrion","excerpts":["An  organelle\nwith two membranes","more"]},{"url":"","title":"no url"}]}}}"#;

    #[test]
    fn parallel_posts_one_keyless_mcp_call_and_reads_json_or_an_event() {
        let sse = format!("event: message\ndata: {}\n\n", PARALLEL_JSON);
        for body in [PARALLEL_JSON.to_string(), sse] {
            let t = Poster(Mutex::new(vec![]), body.into_bytes());
            let f = search(&t, &[spec(Provider::Parallel, None)], "mito site:wikipedia.org").unwrap();
            assert_eq!(f.provider, Provider::Parallel);
            assert_eq!(f.rows, vec![Row { title: "Mitochondrion".into(), url: "https://en.wikipedia.org/wiki/Mitochondrion".into(), snippet: "An organelle with two membranes".into() }]);
            let sent = t.0.lock().unwrap();
            assert_eq!(sent[0].0, PARALLEL_URL);
            let v: Value = serde_json::from_str(&sent[0].1).unwrap();
            assert_eq!((v["method"].as_str(), v["params"]["name"].as_str(), v["params"]["arguments"]["search_queries"][0].as_str()), (Some("tools/call"), Some("web_search"), Some("mito site:wikipedia.org")));
        }
    }

    #[test]
    fn a_parallel_error_or_limit_falls_through_to_the_next_engine() {
        let err = br#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"Missing"}],"isError":true}}"#;
        assert!(parse_rows(Provider::Parallel, err).is_err());
        assert!(parse_rows(Provider::Parallel, b"not json").is_err());
        let t = Fake::new(vec![status(429), ok(BRAVE)]);
        let f = search(&t, &[spec(Provider::Parallel, None), spec(Provider::Brave, Some("k"))], "q").unwrap();
        assert_eq!((f.provider, f.passed_over.len()), (Provider::Brave, 1));
    }

    #[test]
    fn each_engine_answer_parses() {
        let kagi = br#"{"data":[{"t":0,"title":"a","url":"https://x.org/a","snippet":"s"},{"t":1,"list":["related"]}]}"#;
        assert_eq!(parse_rows(Provider::Kagi, kagi).unwrap(), vec![Row { title: "a".into(), url: "https://x.org/a".into(), snippet: "s".into() }]);
        let google = br#"{"items":[{"title":"g","link":"https://y.org/g","snippet":"s"}]}"#;
        assert_eq!(parse_rows(Provider::Google, google).unwrap()[0].url, "https://y.org/g");
        assert_eq!(parse_rows(Provider::Google, b"{}").unwrap(), vec![], "Google omits `items` when nothing matched");
        assert!(parse_rows(Provider::Brave, b"{}").is_err());
    }

    #[test]
    fn the_key_never_reaches_an_error_or_a_debug_print() {
        let s = spec(Provider::Mojeek, Some("SECRET-KEY"));
        assert!(!format!("{:?}", s).contains("SECRET-KEY"));
        let t = Fake::new(vec![Err("could not connect".into())]);
        let e = format!("{:?}", search(&t, &[s], "q").unwrap_err());
        assert!(!e.contains("SECRET-KEY") && !e.contains("api_key"));
    }

    #[test]
    fn site_queries_chunk_the_filters() {
        let sites: Vec<String> = ["a.org", "b.org", "c.org"].map(String::from).to_vec();
        assert_eq!(site_queries("  two   words ", &sites, 2), vec!["two words (site:a.org OR site:b.org)", "two words (site:c.org)"]);
    }

    #[test]
    fn untrusted_rows_and_duplicates_are_dropped_after_the_engine_answers() {
        let list = TrustedList::parse("[[source]]\nhost = \"en.wikipedia.org\"\nreuse = \"full\"\n", None).unwrap();
        let rows = parse_rows(Provider::Brave, BRAVE.as_bytes()).unwrap();
        let mut dup = rows.clone();
        dup.extend(rows);
        let (kept, skipped) = keep_trusted(dup, &list);
        assert_eq!((kept.len(), skipped.len()), (1, 1));
        assert_eq!(kept[0].0.url, "https://en.wikipedia.org/wiki/Mito");
    }
}
