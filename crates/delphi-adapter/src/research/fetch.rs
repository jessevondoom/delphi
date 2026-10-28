//! Reading a trusted page (`specs/028` §10.2 to §10.5): every hop is checked against the lists, links found in a page are followed only
//! when their host is listed too, and what may be stored (the page, or only a summary) is decided here in code, never by a model.

use super::transport::Transport;
use super::trusted::{Reuse, Source, TrustedList};
use reqwest::Url;
use std::time::Duration;

pub const MAX_HOPS: usize = 4;
pub const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone)]
pub struct Page {
    /// Where the content actually came from, after redirects between listed hosts.
    pub url: String,
    pub body: String,
    /// The list entry for `url`'s host.
    pub source: Source,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The address, or a redirect target, is not on the lists. Nothing was requested from it.
    OffList(String),
    Network(String),
    Status(u16),
    /// Not a page of text (an image, a PDF, a download).
    NotText(String),
    TooManyRedirects,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::OffList(h) => write!(f, "{} is not on the trusted lists", h),
            FetchError::Network(w) => write!(f, "{}", w),
            FetchError::Status(s) => write!(f, "answered {}", s),
            FetchError::NotText(t) => write!(f, "not a text page ({})", t),
            FetchError::TooManyRedirects => write!(f, "too many redirects"),
        }
    }
}

fn host_of(url: &str) -> String {
    Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| "that address".into())
}

fn is_text(ct: &str) -> bool {
    let ct = ct.to_ascii_lowercase();
    ct.is_empty() || ct.starts_with("text/") || ct.contains("xhtml") || ct.starts_with("application/xml")
}

/// GET `url` if its host is listed, following redirects only to listed hosts.
pub fn fetch(transport: &dyn Transport, list: &TrustedList, url: &str) -> Result<Page, FetchError> {
    let mut at = url.to_string();
    for _ in 0..=MAX_HOPS {
        let source = list.allows_url(&at).ok_or_else(|| FetchError::OffList(host_of(&at)))?.clone();
        let r = transport.get(&at, &[("Accept", "text/html, text/plain, text/markdown;q=0.9")], MAX_PAGE_BYTES, FETCH_TIMEOUT).map_err(FetchError::Network)?;
        match r.status {
            200 => {
                let ct = r.content_type.clone().unwrap_or_default();
                if !is_text(&ct) {
                    return Err(FetchError::NotText(ct.split(';').next().unwrap_or("").trim().to_string()));
                }
                return Ok(Page { url: at, body: String::from_utf8_lossy(&r.body).into_owned(), source, truncated: r.truncated });
            }
            301 | 302 | 303 | 307 | 308 => {
                let loc = r.location.ok_or(FetchError::Status(r.status))?;
                at = Url::parse(&at).and_then(|b| b.join(&loc)).map_err(|_| FetchError::Status(r.status))?.into();
            }
            s => return Err(FetchError::Status(s)),
        }
    }
    Err(FetchError::TooManyRedirects)
}

/// The absolute `http(s)` links of an HTML page (`href` values), resolved against `base`, without fragments, in order and once each.
pub fn extract_links(html: &str, base: &str) -> Vec<String> {
    let Ok(base) = Url::parse(base) else { return Vec::new() };
    let (lower, bytes) = (html.to_ascii_lowercase(), html.as_bytes()); // lowercasing ASCII keeps every byte offset
    let (mut out, mut seen, mut i) = (Vec::new(), std::collections::HashSet::new(), 0);
    let skip_ws = |mut k: usize| {
        while k < bytes.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        k
    };
    while let Some(p) = lower[i..].find("href") {
        let k = skip_ws(i + p + 4);
        i = k;
        if bytes.get(k) != Some(&b'=') {
            continue;
        }
        let k = skip_ws(k + 1);
        let (start, end) = match bytes.get(k) {
            Some(&q) if q == b'"' || q == b'\'' => match html[k + 1..].find(q as char) {
                Some(e) => (k + 1, k + 1 + e),
                None => break,
            },
            Some(_) => (k, html[k..].find(|c: char| c.is_whitespace() || c == '>').map_or(html.len(), |e| k + e)),
            None => break,
        };
        i = end;
        if let Ok(mut u) = base.join(html[start..end].trim()) {
            u.set_fragment(None);
            if matches!(u.scheme(), "http" | "https") && seen.insert(u.to_string()) {
                out.push(u.into());
            }
        }
    }
    out
}

/// Of the links in a page, those the job may follow (host on the lists) and the hosts of those it may not, counted once each.
pub fn followable(links: Vec<String>, list: &TrustedList) -> (Vec<String>, Vec<String>) {
    let (mut yes, mut no) = (Vec::new(), Vec::<String>::new());
    for l in links {
        if list.allows_url(&l).is_some() {
            yes.push(l);
        } else {
            let h = host_of(&l);
            if !no.contains(&h) {
                no.push(h);
            }
        }
    }
    (yes, no)
}

/// What may be stored of a page (`specs/028` §10.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// The page as markdown.
    Full,
    /// A summary and facts, each with its source and a one-sentence quote. No body is copied.
    Summary,
}

/// Decide the form from the list entry and the page itself. A page is stored in full only if its entry allows it, its licence is
/// known, and the page shows no stricter notice: a rights-reserved line without a Creative Commons or public-domain marker beside it
/// makes it a summary. This errs towards summary; it never guesses a licence from page text.
pub fn decide_form(source: &Source, html: &str) -> Form {
    if source.reuse != Reuse::Full || source.licence.eq_ignore_ascii_case("unknown") {
        return Form::Summary;
    }
    let t = html.to_ascii_lowercase();
    let permissive = t.contains("creativecommons.org") || t.contains("public domain") || t.contains("opensource.org") || t.contains("apache.org/licenses");
    let reserved = t.contains("all rights reserved") || t.contains("no part of this") || t.contains("may not be reproduced") || t.contains("do not copy");
    if reserved && !permissive {
        Form::Summary
    } else {
        Form::Full
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::transport::Response;
    use std::sync::Mutex;

    struct Fake {
        pages: Vec<(&'static str, Response)>,
        asked: Mutex<Vec<String>>,
    }
    impl Transport for Fake {
        fn get(&self, url: &str, _h: &[(&str, &str)], _m: usize, _t: Duration) -> Result<Response, String> {
            self.asked.lock().unwrap().push(url.to_string());
            self.pages.iter().find(|(u, _)| *u == url).map(|(_, r)| r.clone()).ok_or_else(|| "could not connect".to_string())
        }
    }
    fn page(body: &str) -> Response {
        Response { status: 200, content_type: Some("text/html; charset=utf-8".into()), body: body.as_bytes().to_vec(), ..Default::default() }
    }
    fn redirect(to: &str) -> Response {
        Response { status: 301, location: Some(to.into()), ..Default::default() }
    }
    fn list() -> TrustedList {
        TrustedList::parse("[[source]]\nhost = \"en.wikipedia.org\"\nlicence = \"CC-BY-SA-4.0\"\nreuse = \"full\"\n[[source]]\nhost = \"w3.org\"\nreuse = \"summary-only\"\n", None).unwrap()
    }
    fn fake(pages: Vec<(&'static str, Response)>) -> Fake {
        Fake { pages, asked: Mutex::new(Vec::new()) }
    }

    #[test]
    fn a_listed_page_is_fetched_and_carries_its_entry() {
        let t = fake(vec![("https://en.wikipedia.org/wiki/A", page("<p>hi</p>"))]);
        let p = fetch(&t, &list(), "https://en.wikipedia.org/wiki/A").unwrap();
        assert_eq!((p.body.as_str(), p.source.licence.as_str()), ("<p>hi</p>", "CC-BY-SA-4.0"));
    }

    #[test]
    fn an_unlisted_host_is_never_requested() {
        let t = fake(vec![("https://example.com/", page("x"))]);
        assert_eq!(fetch(&t, &list(), "https://example.com/").unwrap_err(), FetchError::OffList("example.com".into()));
        assert_eq!(fetch(&t, &list(), "http://en.wikipedia.org/wiki/A").unwrap_err(), FetchError::OffList("en.wikipedia.org".into()), "plain http is refused");
        assert!(t.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn redirects_stay_between_listed_hosts() {
        let t = fake(vec![("https://en.wikipedia.org/a", redirect("/wiki/B")), ("https://en.wikipedia.org/wiki/B", redirect("https://www.w3.org/TR/x")), ("https://www.w3.org/TR/x", page("spec"))]);
        let p = fetch(&t, &list(), "https://en.wikipedia.org/a").unwrap();
        assert_eq!((p.url.as_str(), p.source.host.as_str()), ("https://www.w3.org/TR/x", "w3.org"));

        let t = fake(vec![("https://en.wikipedia.org/a", redirect("https://evil.example/landing")), ("https://evil.example/landing", page("x"))]);
        assert_eq!(fetch(&t, &list(), "https://en.wikipedia.org/a").unwrap_err(), FetchError::OffList("evil.example".into()));
        assert_eq!(t.asked.lock().unwrap().len(), 1, "the off-list hop was not requested");
    }

    #[test]
    fn a_redirect_loop_stops() {
        let t = fake(vec![("https://en.wikipedia.org/a", redirect("/b")), ("https://en.wikipedia.org/b", redirect("/a"))]);
        assert_eq!(fetch(&t, &list(), "https://en.wikipedia.org/a").unwrap_err(), FetchError::TooManyRedirects);
    }

    #[test]
    fn errors_and_non_text_are_named() {
        let pdf = Response { status: 200, content_type: Some("application/pdf".into()), ..Default::default() };
        let t = fake(vec![("https://en.wikipedia.org/p", pdf), ("https://en.wikipedia.org/n", Response { status: 404, ..Default::default() })]);
        assert_eq!(fetch(&t, &list(), "https://en.wikipedia.org/p").unwrap_err(), FetchError::NotText("application/pdf".into()));
        assert_eq!(fetch(&t, &list(), "https://en.wikipedia.org/n").unwrap_err(), FetchError::Status(404));
        assert_eq!(fetch(&t, &list(), "https://en.wikipedia.org/gone").unwrap_err(), FetchError::Network("could not connect".into()));
    }

    #[test]
    fn links_are_resolved_deduplicated_and_filtered_by_host() {
        let html = r#"<a href="/wiki/B#cite">b</a> <a HREF='https://www.w3.org/TR/x'>w</a> <a href=https://example.com/z>e</a>
            <a href="/wiki/B">again</a> <a href="mailto:a@b.c">m</a> <a href="javascript:void(0)">j</a> <a name="no-href">n</a> <a href="https://en.wikipedia.org/wiki/B?x=1">q</a>"#;
        let links = extract_links(html, "https://en.wikipedia.org/wiki/A");
        assert_eq!(links, vec!["https://en.wikipedia.org/wiki/B", "https://www.w3.org/TR/x", "https://example.com/z", "https://en.wikipedia.org/wiki/B?x=1"]);
        let (yes, no) = followable(links, &list());
        assert_eq!(yes, vec!["https://en.wikipedia.org/wiki/B", "https://www.w3.org/TR/x", "https://en.wikipedia.org/wiki/B?x=1"]);
        assert_eq!(no, vec!["example.com"]);
    }

    #[test]
    fn link_extraction_survives_broken_html() {
        for h in ["<a href=", "<a href='unterminated", "href href = href", "<a href=\"\">", ""] {
            let _ = extract_links(h, "https://en.wikipedia.org/");
        }
    }

    #[test]
    fn the_form_follows_the_entry_the_licence_and_any_stricter_notice() {
        let l = list();
        let wiki = l.lookup("en.wikipedia.org").unwrap();
        let w3 = l.lookup("w3.org").unwrap();
        assert_eq!(decide_form(wiki, "<p>text</p>"), Form::Full);
        assert_eq!(decide_form(w3, "<p>text</p>"), Form::Summary, "summary-only entry");
        assert_eq!(decide_form(wiki, "<footer>© 2026 All Rights Reserved</footer>"), Form::Summary, "a stricter notice");
        assert_eq!(decide_form(wiki, "<a href=\"https://creativecommons.org/licenses/by-sa/4.0/\">CC</a> all rights reserved by authors"), Form::Full, "a Creative Commons marker beside it");
        let unknown = Source { licence: "unknown".into(), ..wiki.clone() };
        assert_eq!(decide_form(&unknown, "<p>text</p>"), Form::Summary, "an unknown licence is never guessed");
    }
}
