//! The research job's deterministic fetching (`specs/028` §10.2, §10.6): a GET that only ever reaches listed hosts, pages
//! turned into Markdown, and the links in them sorted into "follow" and "seen, not read". No model is involved.
//!
//! The network sits behind [`Http`], so everything here is tested with fake pages. A real transport must obey the trait's
//! contract: **do not follow redirects** (this module follows them itself, and only between listed hosts), cap the body
//! at `max_bytes`, and give up after `timeout`.

use crate::research_sources::{https_host, Allow, Entry};
use std::time::Duration;

/// The job's limits (`specs/028` §10.6), provisional until `[research]` is configurable.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// At most this many sources are read per job.
    pub max_sources: usize,
    /// Links are followed to this depth only (1: the pages the search found, and the listed pages they link to).
    pub depth: usize,
    pub fetch_timeout: Duration,
    pub max_page_bytes: usize,
    pub job_timeout: Duration,
    /// Redirects followed per fetch, each one to a listed host.
    pub max_redirects: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_sources: 15, depth: 1, fetch_timeout: Duration::from_secs(20), max_page_bytes: 2 * 1024 * 1024, job_timeout: Duration::from_secs(90), max_redirects: 5 }
    }
}

/// A response, as far as the job reads one.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub status: u16,
    /// Header names are lowercase.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The connection failed, or the host could not be reached.
    Network(String),
    Timeout,
    /// The body was larger than `max_bytes`.
    TooLarge,
}

/// One GET. **Contract:** never follow a redirect (return the 3xx), read at most `max_bytes` (else `TooLarge`), stop after
/// `timeout`, and send nothing but the headers given.
pub trait Http: Send + Sync {
    fn get(&self, url: &str, headers: &[(&str, &str)], max_bytes: usize, timeout: Duration) -> Result<Reply, HttpError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The URL, or a redirect's target, is not on the allow list. Nothing was requested for it.
    OffList(String),
    Http(HttpError),
    /// A non-success status the job does not retry.
    Status(u16),
    TooManyRedirects,
    /// Not a page the job can read (a PDF, an image, a script…).
    NotText(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::OffList(u) => write!(f, "{} is not on the trusted list", u),
            FetchError::Http(HttpError::Timeout) => write!(f, "timed out"),
            FetchError::Http(HttpError::TooLarge) => write!(f, "the page is too large"),
            FetchError::Http(HttpError::Network(e)) => write!(f, "{}", e),
            FetchError::Status(s) => write!(f, "HTTP {}", s),
            FetchError::TooManyRedirects => write!(f, "too many redirects"),
            FetchError::NotText(t) => write!(f, "not a text page ({})", t),
        }
    }
}

/// A page read, ready for the scout.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// The URL asked for, and where the content was (after listed redirects).
    pub url: String,
    pub final_url: String,
    /// The list entry that allowed the final URL.
    pub entry: Entry,
    pub title: String,
    pub markdown: String,
    /// `https://` links in the page, as written, fragments removed, in order, without repeats.
    pub links: Vec<String>,
    pub bytes: usize,
}


/// `Location` resolved against the URL it came from. Only absolute `https://` URLs and absolute paths are understood;
/// anything else (a scheme-relative `//host`, a `javascript:` URL, a relative path) is `None`, so a redirect is never
/// followed on a guess.
pub fn resolve_location(from: &str, location: &str) -> Option<String> {
    let loc = location.trim();
    if loc.starts_with("https://") || loc.starts_with("HTTPS://") {
        return Some(loc.to_string());
    }
    if loc.starts_with('/') && !loc.starts_with("//") {
        let (host, _) = https_host(from)?;
        return Some(format!("https://{}{}", host, loc));
    }
    None
}

/// Fetch `url`, following redirects only to listed hosts, and read it. The allow list is checked **before every request**,
/// so an unlisted host is never contacted, not by a link, not by a redirect.
pub fn fetch(http: &dyn Http, allow: &Allow, url: &str, limits: &Limits) -> Result<Fetched, FetchError> {
    let mut current = url.to_string();
    for _ in 0..=limits.max_redirects {
        let entry = allow.lookup(&current).ok_or_else(|| FetchError::OffList(current.clone()))?.clone();
        let reply = http.get(&current, &[("accept", "text/html, text/plain, text/markdown;q=0.9"), ("user-agent", "delphi-research/1 (a local knowledge-base builder)")], limits.max_page_bytes, limits.fetch_timeout).map_err(FetchError::Http)?;
        if (300..400).contains(&reply.status) {
            let next = reply.header("location").and_then(|l| resolve_location(&current, l)).ok_or(FetchError::Status(reply.status))?;
            current = next;
            continue;
        }
        if reply.status != 200 {
            return Err(FetchError::Status(reply.status));
        }
        let ctype = reply.header("content-type").unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        let is_html = ctype.is_empty() || ctype == "text/html" || ctype == "application/xhtml+xml";
        if !is_html && !ctype.starts_with("text/plain") && ctype != "text/markdown" {
            return Err(FetchError::NotText(ctype));
        }
        let text = String::from_utf8_lossy(&reply.body).into_owned();
        let (markdown, title) = if is_html {
            let md = match delphi_docs::extract(&reply.body, "page.html", &delphi_docs::Request::default(), &delphi_docs::Limits::default()) {
                Ok(x) if x.supported => x.text,
                _ => return Err(FetchError::NotText("html that could not be read".into())),
            };
            let title = md.lines().next().and_then(|l| l.strip_prefix("Title: ")).unwrap_or("").trim().to_string();
            (md, title)
        } else {
            (text.clone(), String::new())
        };
        let links = links_of(&markdown);
        return Ok(Fetched { url: url.to_string(), final_url: current, entry, title, markdown, links, bytes: reply.body.len() });
    }
    Err(FetchError::TooManyRedirects)
}

/// The `https://` links in a Markdown page, in order, fragments removed, without repeats.
pub fn links_of(markdown: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\]\((https://[^)\s]+)\)|<(https://[^>\s]+)>").expect("static regex");
    let mut out: Vec<String> = Vec::new();
    for c in re.captures_iter(markdown) {
        let raw = c.get(1).or_else(|| c.get(2)).map(|m| m.as_str()).unwrap_or("");
        let u = raw.split('#').next().unwrap_or("").trim_end_matches(['.', ',', ';']).to_string();
        if !u.is_empty() && !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

/// What a page's links come to for the job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Follow {
    /// Listed links to read next, at most `room` of them.
    pub follow: Vec<String>,
    /// Links to hosts that are not on the list: recorded as "seen, not read" by host, and never fetched.
    pub seen_not_read: Vec<String>,
}

/// Sort a page's links: those on the allow list (and not already `have`) are followed up to `room`; the rest are noted by
/// host. A link is followed only because its host is listed, never because the page asked.
pub fn sort_links(links: &[String], allow: &Allow, have: &[String], room: usize) -> Follow {
    let mut f = Follow::default();
    for l in links {
        if allow.lookup(l).is_some() {
            if f.follow.len() < room && !have.contains(l) && !f.follow.contains(l) {
                f.follow.push(l.clone());
            }
        } else if let Some((host, _)) = https_host(l) {
            if !f.seen_not_read.contains(&host) {
                f.seen_not_read.push(host);
            }
        }
    }
    f
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Fake pages by URL; records every URL asked for, so a test can prove an unlisted host was never contacted.
    #[derive(Default)]
    pub struct Fake {
        pub pages: BTreeMap<String, Reply>,
        pub asked: Mutex<Vec<String>>,
    }

    impl Fake {
        pub fn page(mut self, url: &str, status: u16, ctype: &str, body: &str) -> Fake {
            self.pages.insert(url.into(), Reply { status, headers: vec![("content-type".into(), ctype.into())], body: body.as_bytes().to_vec() });
            self
        }
        pub fn redirect(mut self, from: &str, to: &str) -> Fake {
            self.pages.insert(from.into(), Reply { status: 302, headers: vec![("location".into(), to.into())], body: vec![] });
            self
        }
    }

    impl Http for Fake {
        fn get(&self, url: &str, _headers: &[(&str, &str)], max_bytes: usize, _t: Duration) -> Result<Reply, HttpError> {
            self.asked.lock().unwrap().push(url.to_string());
            match self.pages.get(url) {
                Some(r) if r.body.len() > max_bytes => Err(HttpError::TooLarge),
                Some(r) => Ok(r.clone()),
                None => Err(HttpError::Network(format!("no such page {}", url))),
            }
        }
    }

    fn allow() -> Allow {
        let e = |h: &str| Entry { host: h.into(), publisher: "P".into(), kind: "encyclopedia".into(), licence: "CC-BY-SA-4.0".into(), reuse: "full".into(), block: false };
        Allow::union(&[e("en.wikipedia.org"), e("*.gov"), e("nist.gov")], &[])
    }

    const HTML: &str = "<html><head><title>Byzantine fault</title></head><body><h1>Byzantine fault</h1><p>A fault in <a href=\"https://en.wikipedia.org/wiki/Paxos\">Paxos</a> and <a href=\"https://nist.gov/x#sec\">NIST</a> and <a href=\"https://evil.example.com/p\">other</a>.</p></body></html>";

    #[test]
    fn a_listed_page_is_read_as_markdown_with_its_title_and_links() {
        let http = Fake::default().page("https://en.wikipedia.org/wiki/BFT", 200, "text/html; charset=utf-8", HTML);
        let f = fetch(&http, &allow(), "https://en.wikipedia.org/wiki/BFT", &Limits::default()).unwrap();
        assert_eq!((f.title.as_str(), f.entry.host.as_str()), ("Byzantine fault", "en.wikipedia.org"));
        assert!(f.markdown.contains("A fault in"));
        assert_eq!(f.links, ["https://en.wikipedia.org/wiki/Paxos", "https://nist.gov/x", "https://evil.example.com/p"], "fragments are dropped, order is kept");
    }

    #[test]
    fn an_unlisted_host_is_never_contacted_directly_or_by_redirect() {
        let http = Fake::default().page("https://evil.example.com/p", 200, "text/html", "x").redirect("https://en.wikipedia.org/go", "https://evil.example.com/p");
        assert_eq!(fetch(&http, &allow(), "https://evil.example.com/p", &Limits::default()).unwrap_err(), FetchError::OffList("https://evil.example.com/p".into()));
        assert!(http.asked.lock().unwrap().is_empty(), "nothing was requested for it");
        assert_eq!(fetch(&http, &allow(), "https://en.wikipedia.org/go", &Limits::default()).unwrap_err(), FetchError::OffList("https://evil.example.com/p".into()));
        assert_eq!(*http.asked.lock().unwrap(), ["https://en.wikipedia.org/go"], "the redirect was seen, its target was not requested");
        assert!(matches!(fetch(&http, &allow(), "http://en.wikipedia.org/x", &Limits::default()), Err(FetchError::OffList(_))), "plain http is off the list");
    }

    #[test]
    fn redirects_between_listed_hosts_are_followed_up_to_a_limit() {
        let http = Fake::default().redirect("https://en.wikipedia.org/a", "https://nist.gov/b").redirect("https://nist.gov/b", "/c").page("https://nist.gov/c", 200, "text/plain", "plain text body");
        let f = fetch(&http, &allow(), "https://en.wikipedia.org/a", &Limits::default()).unwrap();
        assert_eq!((f.final_url.as_str(), f.entry.host.as_str(), f.markdown.as_str()), ("https://nist.gov/c", "nist.gov", "plain text body"));
        let loopy = Fake::default().redirect("https://nist.gov/a", "https://nist.gov/b").redirect("https://nist.gov/b", "https://nist.gov/a");
        assert_eq!(fetch(&loopy, &allow(), "https://nist.gov/a", &Limits::default()).unwrap_err(), FetchError::TooManyRedirects);
        let odd = Fake::default().redirect("https://nist.gov/a", "//evil.example.com/x").redirect("https://nist.gov/b", "javascript:alert(1)").redirect("https://nist.gov/c", "relative/path");
        for p in ["a", "b", "c"] {
            assert!(matches!(fetch(&odd, &allow(), &format!("https://nist.gov/{p}"), &Limits::default()), Err(FetchError::Status(302))), "{p}: a redirect that cannot be understood is not guessed at");
        }
    }

    #[test]
    fn what_cannot_be_read_is_reported_not_stored() {
        let http = Fake::default().page("https://nist.gov/a.pdf", 200, "application/pdf", "%PDF").page("https://nist.gov/missing", 404, "text/html", "").page("https://nist.gov/big", 200, "text/html", &"x".repeat(100));
        assert_eq!(fetch(&http, &allow(), "https://nist.gov/a.pdf", &Limits::default()).unwrap_err(), FetchError::NotText("application/pdf".into()));
        assert_eq!(fetch(&http, &allow(), "https://nist.gov/missing", &Limits::default()).unwrap_err(), FetchError::Status(404));
        let small = Limits { max_page_bytes: 10, ..Limits::default() };
        assert_eq!(fetch(&http, &allow(), "https://nist.gov/big", &small).unwrap_err(), FetchError::Http(HttpError::TooLarge));
        assert_eq!(FetchError::Status(404).to_string(), "HTTP 404");
    }

    #[test]
    fn links_are_followed_only_when_listed_the_rest_are_noted_by_host() {
        let links = ["https://en.wikipedia.org/wiki/Paxos".to_string(), "https://nist.gov/x".into(), "https://evil.example.com/p".into(), "https://evil.example.com/q".into(), "https://other.example.org/".into(), "https://nist.gov/x".into()];
        let f = sort_links(&links, &allow(), &[], 10);
        assert_eq!(f.follow, ["https://en.wikipedia.org/wiki/Paxos", "https://nist.gov/x"], "a repeat is followed once");
        assert_eq!(f.seen_not_read, ["evil.example.com", "other.example.org"], "unlisted hosts are noted once, by host");
        let capped = sort_links(&links, &allow(), &[], 1);
        assert_eq!(capped.follow.len(), 1);
        let have = sort_links(&links, &allow(), &["https://nist.gov/x".to_string()], 10);
        assert_eq!(have.follow, ["https://en.wikipedia.org/wiki/Paxos"], "a page already held is not fetched again");
    }

    #[test]
    fn link_extraction_reads_markdown_and_angle_links_only_over_https() {
        let md = "See [a](https://a.org/x#top), <https://b.org/y>, [c](http://c.org/z), [d](/rel), [e](https://a.org/x). Done [f](https://f.org/q).";
        assert_eq!(links_of(md), ["https://a.org/x", "https://b.org/y", "https://f.org/q"]);
    }
}
