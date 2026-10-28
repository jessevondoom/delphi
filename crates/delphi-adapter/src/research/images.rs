//! Pictures for the research job (`specs/028` §11): which pictures a page shows, what licence each one declares, and the bytes of one,
//! fetched under the same host rules as a page. Deterministic code only; nothing here asks a model.
//!
//! A page's licence is not its pictures' licence, so a picture is kept only on evidence about the picture itself: the Wikimedia
//! Commons record of its file, a Creative Commons link the page puts on the figure that shows it, or a list entry that says every
//! picture on its host carries the entry's licence. Without one of those there is no licence, and no guess is made.

use super::html::{attrs, decode, image_key};
use super::transport::Transport;
use super::trusted::{Source, TrustedList};
use reqwest::Url;
use std::collections::HashSet;
use std::time::Duration;

pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 256 * 1024;
const MAX_HOPS: usize = 4;
const TIMEOUT: Duration = Duration::from_secs(20);
const COMMONS_API: &str = "https://commons.wikimedia.org/w/api.php";

/// A picture a page shows, with the licence evidence the page itself gives for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// Its address in the form the converted markdown uses (`html::image_key`).
    pub url: String,
    pub alt: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// The address of a licence link (`rel="license"`) inside the `<figure>` that holds the picture.
    pub declared: Option<String>,
    /// The `<figcaption>` of that figure, as text.
    pub caption: String,
    /// `File:Name.jpg` when the picture is the link to a Wikimedia file page.
    pub file_title: Option<String>,
}

fn strip_tags(s: &str) -> String {
    let (mut out, mut depth) = (String::new(), 0);
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    decode(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        if b[i] == b'%' {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The `<figure>` around `pos`, as byte offsets into `lower`, when there is one.
fn figure_around(lower: &str, pos: usize) -> Option<(usize, usize)> {
    let mut from = 0;
    let mut start = None;
    while let Some(p) = lower[from..pos].find("<figure") {
        let s = from + p;
        if matches!(lower.as_bytes().get(s + 7), Some(b' ' | b'\t' | b'\n' | b'>' | b'/')) {
            start = Some(s);
        }
        from = s + 7;
    }
    let s = start?;
    if lower[s..pos].contains("</figure") {
        return None;
    }
    let e = lower[pos..].find("</figure").map_or(lower.len(), |n| pos + n);
    Some((s, e))
}

/// The pictures of an HTML page (`<img>` with an `http(s)` address once resolved against `base`), in order and once each, with what the
/// page says about each one's licence.
pub fn extract(html: &str, base: &str) -> Vec<ImageRef> {
    let Ok(base) = Url::parse(base) else { return Vec::new() };
    let lower = html.to_ascii_lowercase(); // ASCII lowercasing keeps every byte offset
    let (mut out, mut seen, mut i) = (Vec::new(), HashSet::new(), 0);
    while let Some(p) = lower[i..].find("<img") {
        let s = i + p;
        if !matches!(lower.as_bytes().get(s + 4), Some(b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>')) {
            i = s + 4;
            continue;
        }
        let Some(gt) = html[s..].find('>') else { break };
        let a = attrs(&html[s + 1..s + gt]);
        i = s + gt + 1;
        let get = |k: &str| a.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        let Some(src) = get("src").or_else(|| get("data-src")) else { continue };
        let Some(url) = base.join(&decode(src.trim())).ok().filter(|u| matches!(u.scheme(), "http" | "https")).map(|mut u| {
            u.set_fragment(None);
            image_key(&u)
        }) else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let alt = decode(get("alt").unwrap_or("")).split_whitespace().collect::<Vec<_>>().join(" ");
        let dim = |k: &str| get(k).and_then(|v| v.trim().trim_end_matches("px").parse::<u32>().ok());
        let (mut declared, mut caption) = (None, String::new());
        if let Some((fs, fe)) = figure_around(&lower, s) {
            let mut k = fs;
            while let Some(q) = lower[k..fe].find("<a ") {
                let at = k + q;
                let Some(end) = html[at..fe].find('>') else { break };
                let la = attrs(&html[at + 1..at + end]);
                let rel = la.iter().find(|(n, _)| n == "rel").map(|(_, v)| v.to_ascii_lowercase()).unwrap_or_default();
                if declared.is_none() && (rel.split_whitespace().any(|w| w == "license" || w == "licence")) {
                    declared = la.iter().find(|(n, _)| n == "href").and_then(|(_, v)| base.join(&decode(v.trim())).ok()).map(|u| u.to_string());
                }
                k = at + end;
            }
            if let (Some(cs), Some(ce)) = (lower[fs..fe].find("<figcaption"), lower[fs..fe].find("</figcaption")) {
                if let Some(gt) = lower[fs + cs..fs + ce].find('>') {
                    caption = strip_tags(&html[fs + cs + gt + 1..fs + ce]).chars().take(300).collect();
                }
            }
        }
        // `<a href=".../wiki/File:Name.jpg"><img ...></a>`: the picture is a link to its file page
        let mut file_title = None;
        if let Some(a_at) = lower[..s].rfind("<a ") {
            if !lower[a_at..s].contains("</a") {
                if let Some(end) = html[a_at..s].find('>') {
                    let la = attrs(&html[a_at + 1..a_at + end]);
                    if let Some(u) = la.iter().find(|(n, _)| n == "href").and_then(|(_, v)| base.join(&decode(v.trim())).ok()) {
                        if let Some(t) = u.path().strip_prefix("/wiki/").filter(|t| t.starts_with("File:")) {
                            file_title = Some(percent_decode(t));
                        }
                    }
                }
            }
        }
        out.push(ImageRef { url, alt, width: dim("width"), height: dim("height"), declared, caption, file_title });
    }
    out
}

// ── licences ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Licence {
    /// `CC-BY-4.0`, `CC-BY-SA-4.0`, `CC0-1.0` or `public-domain`.
    pub spdx: String,
    pub share_alike: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allowed(Licence),
    /// Why not, in words for the results page.
    Refused(String),
}

/// A licence as a person or Commons writes it (`CC BY-SA 4.0`, `Public domain`, `CC0`). Only Creative Commons attribution (with or without
/// share-alike), CC0 and the public domain allow a picture to be kept; non-commercial and no-derivatives licences, GFDL, fair use and
/// anything not recognised do not.
pub fn judge_name(name: &str) -> Verdict {
    let s: String = name.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.').collect();
    let ok = |spdx: String, sa: bool| Verdict::Allowed(Licence { spdx, share_alike: sa });
    let version = |v: &str| {
        let v: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let v = v.trim_matches('.').to_string();
        if v.is_empty() { String::new() } else { format!("-{}", v) }
    };
    if let Some(rest) = s.strip_prefix("ccby") {
        if rest.starts_with("nc") || rest.starts_with("nd") {
            return Verdict::Refused(format!("licence {} does not allow reuse here (non-commercial or no-derivatives)", name.trim()));
        }
        if let Some(v) = rest.strip_prefix("sa") {
            return ok(format!("CC-BY-SA{}", version(v)), true);
        }
        if rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            return ok(format!("CC-BY{}", version(rest)), false);
        }
    }
    if s.starts_with("cc0") || s.starts_with("cczero") {
        return ok("CC0-1.0".into(), false);
    }
    if s.contains("publicdomain") || s == "pd" || s.starts_with("pd") && !s.starts_with("pdf") || s.contains("govpd") || s == "usgov" {
        return ok("public-domain".into(), false);
    }
    Verdict::Refused(format!("licence {} is not one that allows a picture to be kept", if name.trim().is_empty() { "(none)" } else { name.trim() }))
}

/// A licence link: a Creative Commons deed (`creativecommons.org/licenses/by-sa/4.0/`, `/publicdomain/zero/1.0/`, `/publicdomain/mark/1.0/`).
pub fn judge_url(url: &str) -> Verdict {
    let refused = || Verdict::Refused("the licence link on the page is not a Creative Commons deed".into());
    let Ok(u) = Url::parse(url) else { return refused() };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    if host != "creativecommons.org" && !host.ends_with(".creativecommons.org") {
        return refused();
    }
    let seg: Vec<&str> = u.path().split('/').filter(|s| !s.is_empty()).collect();
    match seg.as_slice() {
        ["licenses", kind, version, ..] => judge_name(&format!("cc {} {}", kind, version)),
        ["publicdomain", "zero", ..] => judge_name("cc0"),
        ["publicdomain", "mark", ..] => judge_name("public domain"),
        _ => refused(),
    }
}

// ── Wikimedia Commons ───────────────────────────────────────────────────────────────────────────────────────────────────────────

/// What Commons says about a file: its licence as a short name, the author and whether it is non-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commons {
    pub licence: String,
    pub author: String,
    pub non_free: bool,
}

/// The Commons API address for the metadata of `File:Name.ext`. `None` when the title is not a plausible file title.
pub fn commons_api_url(title: &str) -> Option<String> {
    if !title.starts_with("File:") || title.len() > 300 || title.contains(['\0', '\n', '|']) {
        return None;
    }
    let mut u = Url::parse(COMMONS_API).ok()?;
    u.query_pairs_mut().append_pair("action", "query").append_pair("format", "json").append_pair("prop", "imageinfo").append_pair("iiprop", "extmetadata").append_pair("titles", title);
    Some(u.into())
}

/// The metadata of a Commons API answer; `None` when the file is not there or says nothing about its licence.
pub fn parse_commons(json: &str) -> Option<Commons> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let page = v.get("query")?.get("pages")?.as_object()?.values().next()?;
    if page.get("missing").is_some() {
        return None;
    }
    let meta = page.get("imageinfo")?.get(0)?.get("extmetadata")?;
    let field = |k: &str| meta.get(k).and_then(|f| f.get("value")).and_then(|x| x.as_str()).map(strip_tags).unwrap_or_default();
    let licence = field("LicenseShortName");
    if licence.is_empty() {
        return None;
    }
    Some(Commons { licence, author: field("Artist").chars().take(200).collect(), non_free: field("NonFree").eq_ignore_ascii_case("true") })
}

// ── fetching ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImgError {
    /// The address or a redirect target is not on the lists. Nothing was requested from it.
    OffList(String),
    Network(String),
    Status(u16),
    /// Longer than the cap.
    TooLarge,
    TooManyRedirects,
}

impl std::fmt::Display for ImgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImgError::OffList(h) => write!(f, "{} is not on the trusted lists", h),
            ImgError::Network(w) => write!(f, "{}", w),
            ImgError::Status(s) => write!(f, "answered {}", s),
            ImgError::TooLarge => write!(f, "larger than the limit"),
            ImgError::TooManyRedirects => write!(f, "too many redirects"),
        }
    }
}

fn host_of(url: &str) -> String {
    Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| "that address".into())
}

/// GET `url` if its host is listed, following redirects only to listed hosts, and refuse a body that was cut at `max`.
pub fn get_bytes(transport: &dyn Transport, list: &TrustedList, url: &str, accept: &str, max: usize) -> Result<Vec<u8>, ImgError> {
    let mut at = url.to_string();
    for _ in 0..=MAX_HOPS {
        list.allows_url(&at).ok_or_else(|| ImgError::OffList(host_of(&at)))?;
        let r = transport.get(&at, &[("Accept", accept)], max, TIMEOUT).map_err(ImgError::Network)?;
        match r.status {
            200 if r.truncated || r.body.len() > max => return Err(ImgError::TooLarge),
            200 => return Ok(r.body),
            301 | 302 | 303 | 307 | 308 => {
                let loc = r.location.ok_or(ImgError::Status(r.status))?;
                at = Url::parse(&at).and_then(|b| b.join(&loc)).map_err(|_| ImgError::Status(r.status))?.into();
            }
            s => return Err(ImgError::Status(s)),
        }
    }
    Err(ImgError::TooManyRedirects)
}

/// What a file is by its first bytes (not by its name or the server's word): `png`, `jpg` or `gif`; anything else, SVG and WebP included, is `None`.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("gif")
    } else {
        None
    }
}

// ── the licence of one picture ──────────────────────────────────────────────────────────────────────────────────────────────────

/// A picture's licence and where to credit it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub licence: Licence,
    /// The author when the evidence names one.
    pub author: String,
    /// Where the licence is stated: the Commons file page, the page that shows the picture, or the picture's own address.
    pub credit_url: String,
}

/// The licence of `img`, from the first evidence there is (`specs/028` §11.1): the Commons record of its file (when the Commons API host is
/// on the list), a Creative Commons link on its figure, or the list entry of the picture's host when that entry says its pictures carry
/// its licence. `Err` says why there is none or why it does not allow keeping the picture. `page_url` is the page that shows it.
pub fn resolve(transport: &dyn Transport, list: &TrustedList, img: &ImageRef, host_entry: &Source, page_url: &str) -> Result<Resolved, String> {
    if let Some(title) = &img.file_title {
        if let Some(api) = commons_api_url(title).filter(|u| list.allows_url(u).is_some()) {
            if let Ok(body) = get_bytes(transport, list, &api, "application/json", MAX_JSON_BYTES) {
                if let Some(c) = parse_commons(&String::from_utf8_lossy(&body)) {
                    if c.non_free {
                        return Err("Commons marks the file non-free".into());
                    }
                    return match judge_name(&c.licence) {
                        Verdict::Allowed(licence) => Ok(Resolved { licence, author: c.author, credit_url: format!("https://commons.wikimedia.org/wiki/{}", title.replace(' ', "_")) }),
                        Verdict::Refused(why) => Err(why),
                    };
                }
            }
        }
    }
    if let Some(link) = &img.declared {
        return match judge_url(link) {
            Verdict::Allowed(licence) => Ok(Resolved { licence, author: String::new(), credit_url: page_url.to_string() }),
            Verdict::Refused(why) => Err(why),
        };
    }
    if host_entry.images {
        return match judge_name(&host_entry.licence) {
            Verdict::Allowed(licence) => Ok(Resolved { licence, author: host_entry.publisher.clone(), credit_url: img.url.clone() }),
            Verdict::Refused(why) => Err(why),
        };
    }
    Err("no licence is declared for the picture itself".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::transport::Response;
    use std::sync::Mutex;

    struct Fake {
        pages: Vec<(String, Response)>,
        asked: Mutex<Vec<String>>,
    }
    impl Transport for Fake {
        fn get(&self, url: &str, _h: &[(&str, &str)], _m: usize, _t: Duration) -> Result<Response, String> {
            self.asked.lock().unwrap().push(url.to_string());
            self.pages.iter().find(|(u, _)| u == url).map(|(_, r)| r.clone()).ok_or_else(|| "could not connect".to_string())
        }
    }
    fn fake(pages: Vec<(&str, Response)>) -> Fake {
        Fake { pages: pages.into_iter().map(|(u, r)| (u.to_string(), r)).collect(), asked: Mutex::new(Vec::new()) }
    }
    fn ok(body: &[u8]) -> Response {
        Response { status: 200, body: body.to_vec(), ..Default::default() }
    }
    const LIST: &str = "[[source]]\nhost = \"en.wikipedia.org\"\nlicence = \"CC-BY-SA-4.0\"\nreuse = \"full\"\n[[source]]\nhost = \"commons.wikimedia.org\"\nlicence = \"unknown\"\nreuse = \"summary-only\"\n[[source]]\nhost = \"upload.wikimedia.org\"\nlicence = \"unknown\"\nreuse = \"summary-only\"\n[[source]]\nhost = \"*.gov\"\npublisher = \"US government\"\nlicence = \"US-Gov-PD\"\nreuse = \"full\"\nimages = true\n";
    fn list() -> TrustedList {
        TrustedList::parse(LIST, None).unwrap()
    }

    #[test]
    fn pictures_are_found_resolved_and_given_the_evidence_their_page_gives() {
        let html = r#"<p><a class="mw-file-description" href="/wiki/File:Fault%20tree.png"><img alt="A fault tree" src="//upload.wikimedia.org/wikipedia/commons/a/ab/Fault_tree_(x).png" width="220" height="140"></a></p>
            <figure><img src="/img/chart.jpg" alt='A "chart"'><figcaption>Fig 1. <b>Growth</b></figcaption>
              <a rel="nofollow license" href="https://creativecommons.org/licenses/by/4.0/">CC BY 4.0</a></figure>
            <img src="data:image/png;base64,AAAA"><img alt="no src"><img src="mailto:x"><IMG SRC="/img/chart.jpg"><img src="/b.gif" width="16px">"#;
        let imgs = extract(html, "https://en.wikipedia.org/wiki/Fault");
        assert_eq!(imgs.iter().map(|i| i.url.as_str()).collect::<Vec<_>>(), vec!["https://upload.wikimedia.org/wikipedia/commons/a/ab/Fault_tree_%28x%29.png", "https://en.wikipedia.org/img/chart.jpg", "https://en.wikipedia.org/b.gif"], "parentheses are encoded, duplicates and unusable addresses dropped");
        let (a, b, c) = (&imgs[0], &imgs[1], &imgs[2]);
        assert_eq!((a.alt.as_str(), a.width, a.height, a.file_title.as_deref()), ("A fault tree", Some(220), Some(140), Some("File:Fault tree.png")));
        assert_eq!(a.declared, None);
        assert_eq!((b.declared.as_deref(), b.caption.as_str(), b.alt.as_str()), (Some("https://creativecommons.org/licenses/by/4.0/"), "Fig 1. Growth", "A \"chart\""));
        assert_eq!((c.width, c.declared.clone(), c.file_title.clone()), (Some(16), None, None), "the licence of one figure is not another picture's");
    }

    #[test]
    fn extraction_survives_broken_html() {
        for h in ["<img", "<img src=", "<img src='x", "<figure><img src=/a.png", "<a href=\"/wiki/File:x.png\"><img src=/a.png", "", "<img/><img>"] {
            let _ = extract(h, "https://en.wikipedia.org/");
        }
    }

    #[test]
    fn only_attribution_share_alike_cc0_and_the_public_domain_allow_a_picture_to_be_kept() {
        let allowed = |n: &str| match judge_name(n) {
            Verdict::Allowed(l) => Some((l.spdx, l.share_alike)),
            Verdict::Refused(_) => None,
        };
        assert_eq!(allowed("CC BY 4.0"), Some(("CC-BY-4.0".into(), false)));
        assert_eq!(allowed("CC BY-SA 3.0"), Some(("CC-BY-SA-3.0".into(), true)));
        assert_eq!(allowed("cc-by-sa-2.5"), Some(("CC-BY-SA-2.5".into(), true)));
        assert_eq!(allowed("CC0"), Some(("CC0-1.0".into(), false)));
        assert_eq!(allowed("Public domain"), Some(("public-domain".into(), false)));
        assert_eq!(allowed("PD-USGov-NASA"), Some(("public-domain".into(), false)));
        for no in ["CC BY-NC 4.0", "CC BY-ND 2.0", "CC BY-NC-SA 3.0", "CC BY-NC-ND 4.0", "GFDL", "Fair use", "All rights reserved", "", "Attribution", "CC BY-X 1"] {
            assert!(matches!(judge_name(no), Verdict::Refused(_)), "{} must not be allowed", no);
        }
        let Verdict::Refused(why) = judge_name("CC BY-NC 4.0") else { panic!() };
        assert!(why.contains("non-commercial"), "{}", why);
    }

    #[test]
    fn a_licence_link_must_be_a_creative_commons_deed() {
        assert!(matches!(judge_url("https://creativecommons.org/licenses/by-sa/4.0/"), Verdict::Allowed(l) if l.spdx == "CC-BY-SA-4.0" && l.share_alike));
        assert!(matches!(judge_url("https://creativecommons.org/publicdomain/zero/1.0/"), Verdict::Allowed(l) if l.spdx == "CC0-1.0"));
        assert!(matches!(judge_url("https://creativecommons.org/publicdomain/mark/1.0/"), Verdict::Allowed(l) if l.spdx == "public-domain"));
        for no in ["https://creativecommons.org/licenses/by-nc/4.0/", "https://creativecommons.org.evil.example/licenses/by/4.0/", "https://example.com/licenses/by/4.0/", "https://creativecommons.org/about/", "not a url"] {
            assert!(matches!(judge_url(no), Verdict::Refused(_)), "{}", no);
        }
    }

    #[test]
    fn commons_metadata_is_read_for_licence_author_and_non_free() {
        let json = r#"{"query":{"pages":{"123":{"title":"File:X.png","imageinfo":[{"extmetadata":{"LicenseShortName":{"value":"CC BY-SA 4.0"},"Artist":{"value":"<a href=\"//commons.wikimedia.org/wiki/User:Ann\">Ann &amp; Bo</a>"},"NonFree":{"value":"false"}}}]}}}}"#;
        let c = parse_commons(json).unwrap();
        assert_eq!((c.licence.as_str(), c.author.as_str(), c.non_free), ("CC BY-SA 4.0", "Ann & Bo", false));
        assert!(parse_commons(&json.replace("false", "true")).unwrap().non_free);
        assert!(parse_commons(r#"{"query":{"pages":{"-1":{"title":"File:Gone.png","missing":""}}}}"#).is_none());
        assert!(parse_commons(r#"{"query":{"pages":{"1":{"imageinfo":[{"extmetadata":{}}]}}}}"#).is_none(), "no licence is no metadata");
        assert!(parse_commons("not json").is_none());
        let url = commons_api_url("File:A b&c.png").unwrap();
        assert!(url.starts_with("https://commons.wikimedia.org/w/api.php?") && url.contains("titles=File%3AA+b%26c.png"), "{}", url);
        assert!(commons_api_url("Main_Page").is_none() && commons_api_url("File:x|y").is_none());
    }

    #[test]
    fn bytes_are_fetched_from_listed_hosts_only_and_a_cut_body_is_refused() {
        let t = fake(vec![
            ("https://upload.wikimedia.org/a.png", ok(b"\x89PNG\r\n\x1a\nrest")),
            ("https://upload.wikimedia.org/cut.png", Response { status: 200, body: vec![1, 2, 3], truncated: true, ..Default::default() }),
            ("https://upload.wikimedia.org/hop.png", Response { status: 301, location: Some("https://evil.example/x.png".into()), ..Default::default() }),
            ("https://upload.wikimedia.org/gone.png", Response { status: 404, ..Default::default() }),
            ("https://upload.wikimedia.org/loop.png", Response { status: 302, location: Some("/loop.png".into()), ..Default::default() }),
            ("https://evil.example/x.png", ok(b"x")),
        ]);
        let l = list();
        assert_eq!(get_bytes(&t, &l, "https://upload.wikimedia.org/a.png", "image/*", 100).unwrap().len(), 12);
        assert_eq!(get_bytes(&t, &l, "https://upload.wikimedia.org/cut.png", "image/*", 100), Err(ImgError::TooLarge));
        assert_eq!(get_bytes(&t, &l, "https://upload.wikimedia.org/hop.png", "image/*", 100), Err(ImgError::OffList("evil.example".into())));
        assert_eq!(get_bytes(&t, &l, "https://upload.wikimedia.org/gone.png", "image/*", 100), Err(ImgError::Status(404)));
        assert_eq!(get_bytes(&t, &l, "https://upload.wikimedia.org/loop.png", "image/*", 100), Err(ImgError::TooManyRedirects));
        assert_eq!(get_bytes(&t, &l, "https://blog.example.com/p.png", "image/*", 100), Err(ImgError::OffList("blog.example.com".into())));
        assert_eq!(get_bytes(&t, &l, "http://upload.wikimedia.org/a.png", "image/*", 100), Err(ImgError::OffList("upload.wikimedia.org".into())), "plain http is refused");
        let asked = t.asked.lock().unwrap().clone();
        assert!(!asked.iter().any(|u| u.contains("evil.example") || u.contains("blog.example.com") || u.starts_with("http://")), "{:?}", asked);
    }

    #[test]
    fn a_file_is_known_by_its_bytes_not_its_name_or_its_server() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n\0\0"), Some("png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(sniff(b"GIF89a.."), Some("gif"));
        for no in [&b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"[..], b"RIFF\0\0\0\0WEBPVP8 ", b"<!doctype html><title>x</title>", b"", b"\x89PNG"] {
            assert_eq!(sniff(no), None, "{:?}", String::from_utf8_lossy(no));
        }
    }

    fn img(url: &str) -> ImageRef {
        ImageRef { url: url.into(), alt: "a".into(), width: None, height: None, declared: None, caption: String::new(), file_title: None }
    }
    const COMMONS_OK: &str = r#"{"query":{"pages":{"1":{"imageinfo":[{"extmetadata":{"LicenseShortName":{"value":"CC BY-SA 4.0"},"Artist":{"value":"Ann"}}}]}}}}"#;

    #[test]
    fn the_licence_comes_from_commons_then_the_figure_then_the_host_and_otherwise_there_is_none() {
        let l = list();
        let en = l.lookup("en.wikipedia.org").unwrap();
        let t = fake(vec![("https://commons.wikimedia.org/w/api.php?action=query&format=json&prop=imageinfo&iiprop=extmetadata&titles=File%3AX.png", ok(COMMONS_OK.as_bytes()))]);
        let mut a = img("https://upload.wikimedia.org/x.png");
        a.file_title = Some("File:X.png".into());
        let r = resolve(&t, &l, &a, en, "https://en.wikipedia.org/wiki/P").unwrap();
        assert_eq!((r.licence.spdx.as_str(), r.licence.share_alike, r.author.as_str(), r.credit_url.as_str()), ("CC-BY-SA-4.0", true, "Ann", "https://commons.wikimedia.org/wiki/File:X.png"));

        let mut b = img("https://en.wikipedia.org/b.jpg");
        b.declared = Some("https://creativecommons.org/licenses/by/4.0/".into());
        let r = resolve(&t, &l, &b, en, "https://en.wikipedia.org/wiki/P").unwrap();
        assert_eq!((r.licence.spdx.as_str(), r.credit_url.as_str()), ("CC-BY-4.0", "https://en.wikipedia.org/wiki/P"));

        let gov = l.lookup("nasa.gov").unwrap();
        let r = resolve(&t, &l, &img("https://www.nasa.gov/p.jpg"), gov, "https://www.nasa.gov/x").unwrap();
        assert_eq!((r.licence.spdx.as_str(), r.author.as_str()), ("public-domain", "US government"));

        assert_eq!(resolve(&t, &l, &img("https://en.wikipedia.org/c.png"), en, "p").unwrap_err(), "no licence is declared for the picture itself", "the page's own licence is not the picture's");
    }

    #[test]
    fn evidence_that_refuses_is_final_and_commons_without_its_host_listed_is_not_asked() {
        let l = list();
        let en = l.lookup("en.wikipedia.org").unwrap();
        let nc = COMMONS_OK.replace("CC BY-SA 4.0", "CC BY-NC 4.0");
        let t = fake(vec![("https://commons.wikimedia.org/w/api.php?action=query&format=json&prop=imageinfo&iiprop=extmetadata&titles=File%3AX.png", ok(nc.as_bytes()))]);
        let mut a = img("https://upload.wikimedia.org/x.png");
        a.file_title = Some("File:X.png".into());
        a.declared = Some("https://creativecommons.org/licenses/by/4.0/".into());
        assert!(resolve(&t, &l, &a, en, "p").unwrap_err().contains("non-commercial"), "an NC file is not rescued by a link on the page");
        let nonfree = COMMONS_OK.replace("\"Artist\"", "\"NonFree\":{\"value\":\"true\"},\"Artist\"");
        let t = fake(vec![("https://commons.wikimedia.org/w/api.php?action=query&format=json&prop=imageinfo&iiprop=extmetadata&titles=File%3AX.png", ok(nonfree.as_bytes()))]);
        assert!(resolve(&t, &l, &a, en, "p").unwrap_err().contains("non-free"));

        let no_commons = TrustedList::parse("[[source]]\nhost = \"en.wikipedia.org\"\nlicence = \"CC-BY-SA-4.0\"\nreuse = \"full\"\n", None).unwrap();
        let t = fake(vec![]);
        let mut b = img("https://en.wikipedia.org/x.png");
        b.file_title = Some("File:X.png".into());
        assert!(resolve(&t, &no_commons, &b, no_commons.lookup("en.wikipedia.org").unwrap(), "p").is_err());
        assert!(t.asked.lock().unwrap().is_empty(), "the Commons host is not on the list, so it is not requested");
    }
}
