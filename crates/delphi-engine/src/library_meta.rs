//! Deterministic metadata readers (`specs/055` §3.4): Wikipedia, a DOI's landing page and a Project Gutenberg header.
//!
//! The input is what the V2a fetcher already returned (URL, headers, body). No model runs and nothing is guessed: a
//! field the page does not state is left out. A reader never sets a licence it cannot establish (Wikipedia's is fixed
//! by the project; a Gutenberg text's public-domain basis is the operator's to record). Values come from an untrusted
//! page, so they are cleaned (control characters, length) before they reach a record.

use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

#[derive(Debug, Default)]
pub struct Found {
    pub fields: Map<String, Value>,
    pub notes: Vec<String>,
}

fn clean(s: &str, n: usize) -> String {
    let d = s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ");
    let one: String = d.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    one.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect()
}

fn date(s: &str) -> Option<Value> {
    static D: OnceLock<Regex> = OnceLock::new();
    let c = D.get_or_init(|| Regex::new(r"^\s*(\d{4})(?:[-/.](\d{1,2}))?(?:[-/.](\d{1,2}))?").unwrap()).captures(s)?;
    let mut parts: Vec<u32> = vec![c[1].parse().ok()?];
    for i in [2, 3] {
        match c.get(i).and_then(|m| m.as_str().parse::<u32>().ok()) {
            Some(n) if (i == 2 && (1..=12).contains(&n)) || (i == 3 && (1..=31).contains(&n)) => parts.push(n),
            _ => break,
        }
    }
    Some(json!({"date-parts": [parts]}))
}

fn name(s: &str) -> Value {
    let s = clean(s, 200);
    if let Some((f, g)) = s.split_once(',') {
        return json!({"family": f.trim(), "given": g.trim()});
    }
    match s.rsplit_once(' ') {
        Some((g, f)) => json!({"family": f, "given": g}),
        None => json!({"literal": s}),
    }
}

/// `<meta name=… content=…>` pairs, names lower-cased, in document order.
fn meta_tags(html: &str) -> Vec<(String, String)> {
    static TAG: OnceLock<Regex> = OnceLock::new();
    static ATTR: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r"(?is)<meta\b[^>]*>").unwrap());
    let attr = ATTR.get_or_init(|| Regex::new(r#"(?is)([a-z][a-z0-9:._-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap());
    let mut out = Vec::new();
    for t in tag.find_iter(html).take(400) {
        let (mut n, mut c) = (None, None);
        for a in attr.captures_iter(t.as_str()) {
            let v = a.get(2).or_else(|| a.get(3)).map_or("", |m| m.as_str());
            match a[1].to_ascii_lowercase().as_str() {
                "name" | "property" => n = Some(v.to_ascii_lowercase()),
                "content" => c = Some(v.to_string()),
                _ => {}
            }
        }
        if let (Some(n), Some(c)) = (n, c) {
            out.push((n, c));
        }
    }
    out
}

fn header<'a>(headers: &'a [(String, String)], k: &str) -> Option<&'a str> {
    headers.iter().find(|(h, _)| h.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
}

/// Read what `url`'s kind of page states. `doi` is set when the operator gave a DOI.
pub fn read(url: &str, headers: &[(String, String)], html: &str, text: &str, doi: Option<&str>) -> Found {
    let host = url.split_once("://").map(|(_, r)| r).unwrap_or(url).split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
    if host.ends_with(".wikipedia.org") {
        return wikipedia(&host, headers);
    }
    if host == "gutenberg.org" || host.ends_with(".gutenberg.org") {
        return gutenberg(url, text);
    }
    let from_doi_host = host == "doi.org" || host == "dx.doi.org";
    match doi.map(String::from).or_else(|| from_doi_host.then(|| url.split_once(&host).map(|(_, p)| p.trim_start_matches('/').to_string()).unwrap_or_default())) {
        Some(d) if !d.is_empty() => doi_page(&d, html),
        _ => Found::default(),
    }
}

fn wikipedia(host: &str, headers: &[(String, String)]) -> Found {
    let mut f = Found::default();
    let lang = host.trim_end_matches(".wikipedia.org");
    if lang.chars().all(|c| c.is_ascii_alphabetic() || c == '-') && !lang.is_empty() && lang != "www" {
        f.fields.insert("language".into(), json!(lang));
    }
    f.fields.insert("licence".into(), json!("CC-BY-SA-4.0"));
    f.fields.insert("rights_holder".into(), json!("Wikipedia contributors"));
    f.fields.insert("publisher".into(), json!("Wikimedia Foundation"));
    f.fields.insert("container-title".into(), json!("Wikipedia"));
    f.fields.insert("author".into(), json!([{"literal": "Wikipedia contributors"}]));
    f.fields.insert("type".into(), json!("entry-encyclopedia"));
    // the revision is the number before the first `/` of the ETag (`W/"1234567/uuid"`)
    static REV: OnceLock<Regex> = OnceLock::new();
    let rev = header(headers, "etag").and_then(|e| REV.get_or_init(|| Regex::new(r#"^(?:W/)?"(\d+)/"#).unwrap()).captures(e).map(|c| c[1].to_string()));
    match rev {
        Some(r) => {
            f.fields.insert("edition".into(), json!(format!("Revision {}", r)));
            f.fields.insert("revision".into(), json!(r));
        }
        None => f.notes.push("the page named no revision id; the citation has none".into()),
    }
    if let Some(d) = header(headers, "last-modified").and_then(http_date) {
        f.fields.insert("issued".into(), d);
    }
    f
}

fn http_date(s: &str) -> Option<Value> {
    // `Tue, 15 Nov 2022 08:12:31 GMT`
    let p: Vec<&str> = s.split_whitespace().collect();
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|m| p.get(2) == Some(m))? as u32 + 1;
    Some(json!({"date-parts": [[p.get(3)?.parse::<u32>().ok()?, month, p.get(1)?.parse::<u32>().ok()?]]}))
}

fn gutenberg(url: &str, text: &str) -> Found {
    let mut f = Found::default();
    f.fields.insert("publisher".into(), json!("Project Gutenberg"));
    f.fields.insert("type".into(), json!("book"));
    static N: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = N.get_or_init(|| Regex::new(r"/(?:ebooks/|cache/epub/|files/)(\d+)").unwrap()).captures(url) {
        f.fields.insert("collection-number".into(), json!(c[1].to_string()));
    }
    let head: String = text.chars().take(4000).collect();
    let field = |k: &str| -> Option<String> {
        let re = Regex::new(&format!(r"(?m)^\s*{}:[ \t]*(.+?)\s*$", k)).ok()?;
        re.captures(&head).map(|c| clean(&c[1], 300)).filter(|v| !v.is_empty())
    };
    if let Some(t) = field("Title") {
        f.fields.insert("title".into(), json!(t));
    }
    if let Some(a) = field("Author") {
        f.fields.insert("author".into(), json!([name(&a)]));
    }
    if let Some(l) = field("Language") {
        f.fields.insert("language".into(), json!(l));
    }
    if let Some(o) = field("Original publication").and_then(|o| date(o.rsplit(' ').next().unwrap_or(""))) {
        f.fields.insert("issued".into(), o);
    }
    if !f.fields.contains_key("title") {
        f.notes.push("no Gutenberg header was found in the text; title and author were left empty".into());
    }
    f.notes.push("the licence is left unknown: record the public-domain basis with `--licence public-domain:<jurisdiction>:<basis>`".into());
    f
}

fn doi_page(doi: &str, html: &str) -> Found {
    let mut f = Found::default();
    let tags = meta_tags(html);
    let all = |keys: &[&str]| -> Vec<String> { tags.iter().filter(|(n, _)| keys.contains(&n.as_str())).map(|(_, v)| clean(v, 300)).filter(|v| !v.is_empty()).collect() };
    let one = |keys: &[&str]| all(keys).into_iter().next();
    f.fields.insert("DOI".into(), json!(clean(doi, 200)));
    f.fields.insert("URL".into(), json!(format!("https://doi.org/{}", clean(doi, 200))));
    if let Some(t) = one(&["citation_title", "dc.title", "og:title"]) {
        f.fields.insert("title".into(), json!(t));
    }
    let authors = all(&["citation_author", "dc.creator"]);
    if !authors.is_empty() {
        f.fields.insert("author".into(), Value::Array(authors.iter().take(50).map(|a| name(a)).collect()));
    }
    let journal = one(&["citation_journal_title"]);
    f.fields.insert("type".into(), json!(if journal.is_some() { "article-journal" } else { "article" }));
    for (k, keys) in [("container-title", &["citation_journal_title", "citation_conference_title", "citation_book_title"][..]), ("publisher", &["citation_publisher", "dc.publisher"]), ("volume", &["citation_volume"]), ("issue", &["citation_issue"]), ("ISSN", &["citation_issn"]), ("ISBN", &["citation_isbn"]), ("language", &["citation_language", "dc.language"])] {
        if let Some(v) = one(keys) {
            f.fields.insert(k.into(), json!(v));
        }
    }
    if let Some(d) = one(&["citation_publication_date", "citation_date", "citation_online_date", "dc.date", "article:published_time"]).and_then(|d| date(&d)) {
        f.fields.insert("issued".into(), d);
    }
    match (one(&["citation_firstpage"]), one(&["citation_lastpage"])) {
        (Some(a), Some(b)) => f.fields.insert("page".into(), json!(format!("{}-{}", a, b))),
        (Some(a), None) => f.fields.insert("page".into(), json!(a)),
        _ => None,
    };
    if !f.fields.contains_key("title") {
        f.notes.push("the landing page states no citation metadata; only the DOI is recorded".into());
    }
    f
}
