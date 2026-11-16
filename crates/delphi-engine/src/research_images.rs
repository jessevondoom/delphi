//! Pictures saved with a full-text research entry (`specs/028` §11), the engine's half: for the pictures in one kept page it decides,
//! in code, which may be stored, writes them beside the entry, and rewrites the entry's markdown so a saved picture stands alone in
//! its paragraph with its attribution under it and every other one is its alt text.
//!
//! The adapter says what a picture's licence is and fetches bytes under the host rules (`delphi_adapter::research::images`). Here the
//! bytes are checked once more, by the decoders the reader uses (a file that does not decode is dropped), and named by their hash.
//! No model sees a picture, and nothing here is asked to guess a licence.

use delphi_adapter::research::html::{self, ImageTok};
use delphi_adapter::research::images::{self, ImageRef, ImgError, MAX_IMAGE_BYTES};
use delphi_adapter::research::transport::Transport;
use delphi_adapter::research::trusted::TrustedList;
use std::collections::HashMap;
use std::path::Path;

/// A picture smaller than this on either side is an icon, a flag or a logo, not something to keep.
const MIN_SIDE: usize = 48;

/// A picture that was kept: where it is, whose it is and under which licence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// Relative to the entry: `images/<blake3>.<png|jpg|gif>`.
    pub file: String,
    /// The picture's own address.
    pub source_url: String,
    pub licence: String,
    pub share_alike: bool,
    pub author: String,
    /// The line shown under the picture.
    pub attribution: String,
    pub blake3: String,
    /// The file did not exist before this run wrote it.
    pub new_file: bool,
    /// Where the licence is stated, for the credit line.
    pub credit_url: String,
    /// Which check decided the licence and where its evidence is (`specs/118` §2 item 1).
    pub evidence: &'static str,
    pub evidence_url: String,
    pub width: usize,
    pub height: usize,
    /// When the picture was an SVG: the PNG above is its drawing, and this is the SVG, kept beside it for the record and never
    /// served (`specs/118` §4 item 2).
    pub svg: Option<SvgOriginal>,
}

/// The SVG a kept PNG was drawn from: `images/<the SVG's own blake3>.svg`, and that hash. Two SVGs that draw to the same PNG are
/// two files, each named by its own bytes, so a record's hash is always the hash of the file it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvgOriginal {
    pub file: String,
    pub blake3: String,
}

/// What became of one page's pictures.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The entry's markdown with its pictures rewritten.
    pub markdown: String,
    pub saved: Vec<Saved>,
    /// Pictures not kept, each with its address and why.
    pub not_saved: Vec<(String, String)>,
    /// How many pictures were fetched (counted against the job's limit).
    pub fetched: usize,
}

fn text(s: &str, n: usize) -> String {
    crate::observe::sanitize(s).replace(['*', '_', '[', ']', '`', '\n'], " ").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect()
}

fn why(e: ImgError) -> String {
    match e {
        ImgError::OffList(h) => format!("it redirected to {}, which is not on the trusted lists", h),
        ImgError::TooLarge => format!("larger than {} MiB", MAX_IMAGE_BYTES / (1024 * 1024)),
        ImgError::Status(s) => format!("the host answered {}", s),
        ImgError::Network(w) => w,
        ImgError::TooManyRedirects => "too many redirects".into(),
    }
}

/// Is the text before a picture inside a link's label? A link is `[label](address)`; a picture can be drawn only on its own, so one in a
/// link with other words is not kept.
fn inside_link(before: &str) -> bool {
    let para = &before[before.rfind("\n\n").map_or(0, |p| p + 2)..];
    para.matches('[').count() > para.matches(']').count()
}

/// Everything that decides whether one picture is kept, and keeping it. `Err` is the reason it was not.
#[allow(clippy::too_many_arguments)]
fn keep(transport: &dyn Transport, list: &TrustedList, dir: &Path, host: &str, page_url: &str, tok: &ImageTok, r: &ImageRef, fetched: &mut usize, may_fetch: bool) -> Result<Saved, String> {
    let entry = list.allows_url(&tok.url).ok_or_else(|| format!("the picture's host ({}) is not on the trusted lists", images_host(&tok.url)))?;
    if tok.alt.trim().is_empty() {
        return Err("no description, so it is taken as decoration".into());
    }
    if r.width.is_some_and(|w| (w as usize) < MIN_SIDE) || r.height.is_some_and(|h| (h as usize) < MIN_SIDE) {
        return Err("too small to be a picture (an icon or a logo)".into());
    }
    let resolved = images::resolve(transport, list, r, entry, page_url)?;
    if !may_fetch {
        return Err("past the limit on pictures".into());
    }
    *fetched += 1;
    let fetched_bytes = images::get_bytes(transport, list, &tok.url, "image/png, image/jpeg, image/gif, image/webp, image/svg+xml", MAX_IMAGE_BYTES).map_err(why)?;
    let ext = images::sniff(&fetched_bytes).ok_or("not a PNG, JPEG, GIF, WebP or SVG picture")?;
    // An SVG is never kept to be shown as SVG (`specs/118` §4 item 2): it is drawn to a PNG here, under the bounds of
    // `research_svg`, and the SVG is kept beside the PNG for the record only. A WebP is kept as it is (§4 item 1).
    let (bytes, ext, svg) = if ext == "svg" {
        let drawn = crate::research_svg::render(&fetched_bytes).map_err(|w| format!("the SVG could not be drawn: {}", w))?;
        (drawn.png, "png", Some(fetched_bytes))
    } else {
        (fetched_bytes, ext, None)
    };
    let pic = delphi_docs::pixels::decode(&bytes).map_err(|_| "the file could not be decoded as a picture".to_string())?;
    if pic.width < MIN_SIDE || pic.height < MIN_SIDE {
        return Err("too small to be a picture (an icon or a logo)".into());
    }
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let file = format!("images/{}.{}", hash, ext);
    let root = dir.to_path_buf();
    let folder = root.join(host).join("images");
    let path = folder.join(format!("{}.{}", hash, ext));
    if !path.starts_with(&root) || host.contains("..") {
        return Err("the address cannot be stored".into());
    }
    let new_file = !path.exists();
    if new_file {
        std::fs::create_dir_all(&folder).map_err(|e| format!("could not create the folder: {}", e))?;
        std::fs::write(&path, &bytes).map_err(|e| format!("could not write the picture: {}", e))?;
    }
    let svg = match svg {
        Some(s) => {
            let svg_hash = blake3::hash(&s).to_hex().to_string();
            let svg_path = folder.join(format!("{}.svg", svg_hash));
            if !svg_path.exists() {
                std::fs::write(&svg_path, &s).map_err(|e| format!("could not write the picture: {}", e))?;
            }
            Some(SvgOriginal { file: format!("images/{}.svg", svg_hash), blake3: svg_hash })
        }
        None => None,
    };
    let licence = if resolved.licence.spdx == "public-domain" { "public domain".to_string() } else { resolved.licence.spdx.clone() };
    let author = text(&resolved.author, 200);
    let attribution = format!("Image: {}, {}, {}", if author.is_empty() { "author not stated" } else { author.as_str() }, licence, text(&resolved.credit_url, 300));
    Ok(Saved {
        file,
        source_url: tok.url.clone(),
        licence: resolved.licence.spdx,
        share_alike: resolved.licence.share_alike,
        author,
        attribution,
        blake3: hash,
        new_file,
        credit_url: text(&resolved.credit_url, 300),
        evidence: resolved.evidence,
        evidence_url: text(&resolved.evidence_url, 500),
        width: pic.width,
        height: pic.height,
        svg,
    })
}

fn images_host(url: &str) -> String {
    delphi_adapter::reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| "that address".into())
}

/// Process the pictures of one kept page. `md` is the page converted with its images (`html::to_markdown_with_images`), `html_body` the
/// page itself, whose figures say what licence a picture declares. At most `per_entry` pictures are fetched, and no more than `job_left`.
/// `root` is the folder the entries live in; pictures go beside them. Blocking: it makes requests.
#[allow(clippy::too_many_arguments)]
pub fn process(transport: &dyn Transport, list: &TrustedList, root: &Path, host: &str, page_url: &str, html_body: &str, md: &str, per_entry: usize, job_left: usize) -> Outcome {
    process_some(transport, list, root, host, page_url, html_body, md, per_entry, job_left, None)
}

/// [`process`], judging only the pictures whose addresses are in `named` when it is given (a host's `record_research` names the
/// pictures of a source it wants kept, `specs/118` §3 item 3): any other picture is its alt text, judged not at all and fetched never.
#[allow(clippy::too_many_arguments)]
fn process_some(transport: &dyn Transport, list: &TrustedList, root: &Path, host: &str, page_url: &str, html_body: &str, md: &str, per_entry: usize, job_left: usize, named: Option<&[String]>) -> Outcome {
    let toks: Vec<ImageTok> = html::find_images(md);
    let refs: HashMap<String, ImageRef> = images::extract(html_body, page_url).into_iter().map(|r| (r.url.clone(), r)).collect();
    let mut out = Outcome::default();
    let (mut text_out, mut at) = (String::with_capacity(md.len()), 0usize);
    let mut done: HashMap<String, Option<Saved>> = HashMap::new(); // a picture shown twice is judged once
    for tok in &toks {
        text_out.push_str(&md[at..tok.start]);
        at = tok.end;
        if named.is_some_and(|n| !n.contains(&tok.url)) {
            text_out.push_str(&tok.alt);
            continue;
        }
        let kept = match done.get(&tok.url) {
            Some(s) => s.clone(),
            None => {
                let verdict = match refs.get(&tok.url) {
                    None => Err("not among the page's pictures".to_string()),
                    Some(_) if inside_link(&md[..tok.start]) => Err("the picture is inside a link with other words".to_string()),
                    Some(r) => {
                        let may_fetch = out.fetched < per_entry && out.fetched < job_left;
                        keep(transport, list, root, host, page_url, tok, r, &mut out.fetched, may_fetch)
                    }
                };
                let s = match verdict {
                    Ok(s) => Some(s),
                    Err(w) => {
                        out.not_saved.push((tok.url.clone(), w));
                        None
                    }
                };
                done.insert(tok.url.clone(), s.clone());
                s
            }
        };
        match kept {
            Some(s) => {
                text_out.push_str(&format!("\n\n![{}]({})\n\n*{}*\n\n", text(&tok.alt, 200), s.file, s.attribution));
                if !out.saved.iter().any(|x| x.file == s.file) {
                    out.saved.push(s);
                }
            }
            None => text_out.push_str(&tok.alt),
        }
    }
    text_out.push_str(&md[at..]);
    while text_out.contains("\n\n\n") {
        text_out = text_out.replace("\n\n\n", "\n\n");
    }
    out.markdown = text_out.trim().to_string();
    out
}

/// The fixed word a picture's refusal is recorded under (`specs/118` §3 item 4). The sentences [`keep`] and the adapter give can carry
/// words of the page (a licence name as Commons states it, a host); the record keeps only the word, chosen here by code.
pub fn refusal_word(why: &str) -> &'static str {
    let w = why.to_ascii_lowercase();
    let table: [(&str, &'static str); 16] = [
        // First: the renderer's sentences can hold words ("too many", "text") that other rows look for.
        ("the svg could not be drawn", "svg_not_drawn"),
        ("not on the trusted lists", "host_not_listed"),
        ("no description", "no_description"),
        ("too small", "too_small"),
        ("past the limit", "past_limit"),
        ("no licence is declared", "no_licence"),
        ("non-free", "licence_refused"),
        ("licence", "licence_refused"),
        ("not a png, jpeg, gif, webp or svg", "format_not_kept"),
        ("could not be decoded", "not_decoded"),
        ("inside a link", "inside_link"),
        ("not among the page's pictures", "not_on_page"),
        ("redirect", "fetch_failed"),
        ("mib", "too_large"),
        ("answered", "fetch_failed"),
        ("cannot be stored", "not_stored"),
    ];
    table.iter().find(|(k, _)| w.contains(k)).map(|(_, v)| *v).unwrap_or("fetch_failed")
}

/// What became of the pictures of one library source (`specs/118` §3): the licence records kept, and each picture left out with the
/// fixed word for why.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SourcePictures {
    pub kept: Vec<serde_json::Value>,
    pub refused: Vec<(String, &'static str)>,
    /// Requests made for pictures, counted against a run's limit.
    pub fetched: usize,
}

/// The rules one capture of pictures runs under: the transport (the daemon's own, or a test's fake web), the trusted lists, how many
/// pictures one source may take, and how many the run has left.
pub struct PictureRules<'a> {
    pub transport: &'a dyn Transport,
    pub list: &'a TrustedList,
    pub per_source: usize,
    pub left: usize,
}

/// Keep the pictures of a page whose text the library holds as source `id` (`specs/118` §3): each is judged by [`process`]'s gates and
/// licence decision, fetched under the trusted-list rules, and handed to the library with its licence record (`specs/118` §2 item 1).
/// `named`, when given, limits the pictures judged to those addresses (a host's call names them); a named address the page does not
/// show is refused `not_on_page`. The one capture path of the research runner, `record_research` and page expansion. Blocking.
pub fn keep_for_source(lib: &crate::library::Library, id: &str, page_url: &str, html_body: &str, rules: &PictureRules<'_>, named: Option<&[String]>) -> SourcePictures {
    let mut out = SourcePictures::default();
    let host = crate::research_run::host_of(page_url);
    if rules.per_source == 0 || rules.left == 0 || host.is_empty() || host.contains("..") || host.contains('/') {
        if let Some(n) = named {
            out.refused = n.iter().map(|u| (u.clone(), "past_limit")).collect();
        }
        return out;
    }
    let md = html::to_markdown_with_images(html_body, page_url, 200_000).markdown;
    let stage = lib.root().join(".staging").join(format!("pictures-{}-{}", id, std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let done = process_some(rules.transport, rules.list, &stage, &host, page_url, html_body, &md, rules.per_source, rules.left, named);
    out.fetched = done.fetched;
    let retrieved = delphi_core::time::now_rfc3339();
    let records: Vec<serde_json::Value> = done
        .saved
        .iter()
        .filter_map(|s| {
            let file = s.file.strip_prefix("images/")?;
            let mut rec = crate::library::assets::picture_record(&crate::library::assets::PictureFacts {
                file,
                source_id: id,
                source_url: &s.source_url,
                licence: &s.licence,
                evidence: s.evidence,
                evidence_url: &s.evidence_url,
                author: &s.author,
                credit_url: &s.credit_url,
                share_alike: s.share_alike,
                retrieved: &retrieved,
                blake3: &s.blake3,
                width: s.width,
                height: s.height,
            });
            if let Some(svg) = &s.svg {
                rec[crate::library::assets::RENDERED_FROM] = serde_json::json!({"format": "svg", "file": svg.file.strip_prefix("images/")?, "blake3": svg.blake3});
            }
            Some(rec)
        })
        .collect();
    out.refused = done.not_saved.iter().map(|(u, w)| (u.clone(), refusal_word(w))).collect();
    if !records.is_empty() {
        match lib.attach_pictures(id, &stage.join(&host).join("images"), &records, &crate::library::Budgets::default()) {
            Ok(_) => out.kept = records,
            Err(e) => {
                let word = if e.contains("budget") { "over_budget" } else { "text_not_kept" };
                out.refused.extend(records.iter().map(|r| (r["source_url"].as_str().unwrap_or("").to_string(), word)));
            }
        }
    }
    if let Some(n) = named {
        for u in n {
            let seen = out.refused.iter().any(|(r, _)| r == u) || out.kept.iter().any(|k| k["source_url"].as_str() == Some(u.as_str()));
            if !seen {
                out.refused.push((u.clone(), "not_on_page"));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&stage);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_adapter::research::transport::Response;
    use std::sync::Mutex;
    use std::time::Duration;

    struct Web {
        pages: HashMap<String, Response>,
        asked: Mutex<Vec<String>>,
    }
    impl Transport for Web {
        fn get(&self, url: &str, _h: &[(&str, &str)], _m: usize, _t: Duration) -> Result<Response, String> {
            self.asked.lock().unwrap().push(url.to_string());
            self.pages.get(url).cloned().ok_or_else(|| "could not connect".to_string())
        }
    }
    fn web(pairs: Vec<(&str, Response)>) -> Web {
        Web { pages: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(), asked: Mutex::new(Vec::new()) }
    }
    fn ok(body: Vec<u8>) -> Response {
        Response { status: 200, body, ..Default::default() }
    }

    const LIST: &str = "[[source]]\nhost = \"en.wikipedia.org\"\npublisher = \"Wikipedia\"\nlicence = \"CC-BY-SA-4.0\"\nreuse = \"full\"\nshare_alike = true\n[[source]]\nhost = \"upload.wikimedia.org\"\nlicence = \"unknown\"\nreuse = \"summary-only\"\n[[source]]\nhost = \"commons.wikimedia.org\"\nlicence = \"unknown\"\nreuse = \"summary-only\"\n[[source]]\nhost = \"w3.org\"\nreuse = \"summary-only\"\n";
    const BASE: &str = "https://en.wikipedia.org/wiki/Page";
    /// A 64x64 PNG: big enough to count as a picture (the fixtures of `delphi-docs` are 40x24).
    fn pic() -> Vec<u8> {
        let rgba = (0..64 * 64).flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 4) as u8, 120, 255]).collect();
        delphi_docs::png::encode(&delphi_docs::png::Image { width: 64, height: 64, rgba }, &[])
    }
    fn commons(licence: &str, artist: &str) -> Response {
        ok(format!("{{\"query\":{{\"pages\":{{\"1\":{{\"imageinfo\":[{{\"extmetadata\":{{\"LicenseShortName\":{{\"value\":\"{}\"}},\"Artist\":{{\"value\":\"{}\"}}}}}}]}}}}}}}}", licence, artist).into_bytes())
    }
    fn api(title: &str) -> String {
        delphi_adapter::research::images::commons_api_url(title).unwrap()
    }
    fn run(web: &Web, html: &str, dir: &Path) -> Outcome {
        run_with(web, html, dir, LIST, 6, 24)
    }
    fn run_with(web: &Web, html: &str, dir: &Path, list: &str, per: usize, job: usize) -> Outcome {
        let md = html::to_markdown_with_images(&format!("<html><body>{}</body></html>", html), BASE, 100_000).markdown;
        process(web, &TrustedList::parse(list, None).unwrap(), &dir.join(crate::research::DIR), "en.wikipedia.org", BASE, html, &md, per, job)
    }
    fn files(dir: &Path) -> Vec<String> {
        fn walk(p: &Path, out: &mut Vec<String>) {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        walk(&e.path(), out);
                    } else {
                        out.push(e.path().strip_prefix(p.ancestors().last().unwrap()).unwrap().to_string_lossy().into_owned());
                    }
                }
            }
        }
        let mut v = Vec::new();
        walk(dir, &mut v);
        v.sort();
        v
    }

    const COMMONS_IMG: &str = "https://upload.wikimedia.org/wikipedia/commons/a/ab/Tree.png";
    const COMMONS_HTML: &str = "<p>Intro text. <a href=\"/wiki/File:Tree.png\"><img alt=\"A fault tree\" src=\"//upload.wikimedia.org/wikipedia/commons/a/ab/Tree.png\" width=\"200\" height=\"120\"></a> More text.</p>";

    #[test]
    fn a_picture_with_a_licence_of_its_own_is_saved_beside_the_entry_with_its_attribution() {
        let dir = tempfile::tempdir().unwrap();
        let w = web(vec![(COMMONS_IMG, ok(pic())), (&api("File:Tree.png"), commons("CC BY-SA 4.0", "Ann &amp; Bo"))]);
        let o = run(&w, COMMONS_HTML, dir.path());
        assert!(o.not_saved.is_empty(), "{:?}", o.not_saved);
        assert_eq!(o.saved.len(), 1);
        let s = &o.saved[0];
        assert_eq!((s.licence.as_str(), s.share_alike, s.author.as_str(), s.new_file), ("CC-BY-SA-4.0", true, "Ann & Bo", true));
        assert!(s.file.starts_with("images/") && s.file.ends_with(".png") && s.attribution == "Image: Ann & Bo, CC-BY-SA-4.0, https://commons.wikimedia.org/wiki/File:Tree.png", "{:?}", s);
        let saved = std::fs::read(dir.path().join("local/knowledge/web/en.wikipedia.org").join(&s.file)).unwrap();
        assert_eq!(saved, pic(), "the file is the bytes that were fetched");
        assert_eq!(s.file, format!("images/{}.png", blake3::hash(&saved).to_hex()));
        assert!(o.markdown.contains(&format!("\n\n![A fault tree]({})\n\n*{}*\n\n", s.file, s.attribution)), "alone in its paragraph, attribution below:\n{}", o.markdown);
        assert!(o.markdown.starts_with("Intro text.") && o.markdown.ends_with("More text."), "{}", o.markdown);
        assert_eq!(o.fetched, 1);
    }

    #[test]
    fn a_figure_that_declares_a_creative_commons_licence_keeps_its_picture_and_a_page_licence_alone_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let w = web(vec![("https://en.wikipedia.org/img/c.png", ok(pic())), ("https://en.wikipedia.org/img/d.png", ok(pic()))]);
        let html = "<figure><img src=\"/img/c.png\" alt=\"Chart\"><figcaption>Growth</figcaption><a rel=\"license\" href=\"https://creativecommons.org/licenses/by/4.0/\">CC BY</a></figure><p><img src=\"/img/d.png\" alt=\"Undeclared\"></p>";
        let o = run(&w, html, dir.path());
        assert_eq!(o.saved.len(), 1);
        assert_eq!((o.saved[0].licence.as_str(), o.saved[0].source_url.as_str()), ("CC-BY-4.0", "https://en.wikipedia.org/img/c.png"));
        assert_eq!(o.not_saved, vec![("https://en.wikipedia.org/img/d.png".to_string(), "no licence is declared for the picture itself".to_string())], "the page is CC BY-SA, which is not its pictures' licence");
        assert!(o.markdown.contains("Undeclared") && !o.markdown.contains("d.png"), "an unsaved picture is its alt text:\n{}", o.markdown);
        assert!(!w.asked.lock().unwrap().iter().any(|u| u.ends_with("d.png")), "no bytes are fetched for a picture with no licence");
    }

    #[test]
    fn every_way_a_picture_can_fail_leaves_the_text_and_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let big = Response { status: 200, body: vec![0x89; 10], truncated: true, ..Default::default() };
        let u = |n: &str| format!("https://upload.wikimedia.org/wikipedia/commons/{}", n);
        let case = |name: &str, bytes: Response| (u(name), bytes, api(&format!("File:{}", name)));
        let cases = vec![
            case("nc.png", ok(pic())),
            case("svg.png", ok(b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><text x=\"1\" y=\"9\">a label</text></svg>".to_vec())),
            case("webp.png", ok(b"RIFF\0\0\0\0WEBPVP8 xxxxxxxxxxxxxxxx".to_vec())),
            case("svgbig.png", ok({ let mut s = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\">".to_vec(); s.resize(3 << 20, b' '); s.extend_from_slice(b"</svg>"); s })),
            case("html.png", ok(b"<!doctype html><title>not a picture</title>".to_vec())),
            case("big.png", big),
            case("cut.png", ok({ let g = pic(); g[..g.len() / 2].to_vec() })),
            case("gone.png", Response { status: 404, ..Default::default() }),
        ];
        let mut pages: Vec<(String, Response)> = Vec::new();
        let mut html = String::from("<p>Text.</p>");
        for (url, bytes, meta_url) in &cases {
            let name = url.rsplit('/').next().unwrap();
            pages.push((url.clone(), bytes.clone()));
            pages.push((meta_url.clone(), commons(if name == "nc.png" { "CC BY-NC 4.0" } else { "CC BY 4.0" }, "Ann")));
            html.push_str(&format!("<a href=\"/wiki/File:{n}\"><img alt=\"pic {n}\" src=\"{u}\" width=\"200\" height=\"100\"></a>", n = name, u = url));
        }
        let w = Web { pages: pages.into_iter().collect(), asked: Mutex::new(Vec::new()) };
        let o = run_with(&w, &html, dir.path(), LIST, 10, 24);
        assert!(o.saved.is_empty(), "{:?}", o.saved);
        let reasons: HashMap<&str, &str> = o.not_saved.iter().map(|(u, w)| (u.rsplit('/').next().unwrap(), w.as_str())).collect();
        assert!(reasons["nc.png"].contains("non-commercial"), "{:?}", reasons);
        assert_eq!(reasons["svg.png"], "the SVG could not be drawn: the SVG has text, and text is not drawn here", "{:?}", reasons);
        assert_eq!(reasons["svgbig.png"], "the SVG could not be drawn: an SVG larger than 2 MiB is not drawn", "{:?}", reasons);
        assert!(reasons["webp.png"].contains("could not be decoded") && reasons["html.png"].contains("not a PNG, JPEG, GIF, WebP or SVG"), "{:?}", reasons);
        assert!(reasons["big.png"].contains("larger than 8 MiB"), "{:?}", reasons);
        assert!(reasons["cut.png"].contains("could not be decoded"), "{:?}", reasons);
        assert!(reasons["gone.png"].contains("404"), "{:?}", reasons);
        assert_eq!(o.not_saved.len(), 8);
        let words: HashMap<&str, &str> = o.not_saved.iter().map(|(u, w)| (u.rsplit('/').next().unwrap(), refusal_word(w))).collect();
        assert_eq!((words["svg.png"], words["svgbig.png"], words["webp.png"], words["html.png"]), ("svg_not_drawn", "svg_not_drawn", "not_decoded", "format_not_kept"), "{:?}", words);
        assert!(o.markdown.starts_with("Text.") && o.markdown.contains("pic svg.png") && !o.markdown.contains("!["), "{}", o.markdown);
        assert_eq!(files(dir.path()), Vec::<String>::new(), "nothing was written");
    }

    #[test]
    fn a_picture_on_a_host_off_the_list_is_never_requested_and_nor_is_a_redirect_off_it() {
        let dir = tempfile::tempdir().unwrap();
        let hop = Response { status: 301, location: Some("https://evil.example/x.png".into()), ..Default::default() };
        let w = web(vec![("https://upload.wikimedia.org/wikipedia/commons/h.png", hop), (&api("File:H.png"), commons("CC BY 4.0", "A")), ("https://evil.example/x.png", ok(pic())), ("https://cdn.example.com/p.png", ok(pic()))]);
        let html = "<a href=\"/wiki/File:H.png\"><img alt=\"hop\" src=\"https://upload.wikimedia.org/wikipedia/commons/h.png\" width=\"90\" height=\"90\"></a><figure><img alt=\"cdn\" src=\"https://cdn.example.com/p.png\"><a rel=\"license\" href=\"https://creativecommons.org/licenses/by/4.0/\">CC</a></figure>";
        let o = run(&w, html, dir.path());
        assert!(o.saved.is_empty());
        let by_name: HashMap<_, _> = o.not_saved.iter().map(|(u, w)| (u.rsplit('/').next().unwrap().to_string(), w.clone())).collect();
        assert!(by_name["h.png"].contains("evil.example"), "{:?}", by_name);
        assert!(by_name["p.png"].contains("cdn.example.com"), "{:?}", by_name);
        let asked = w.asked.lock().unwrap().clone();
        assert!(!asked.iter().any(|u| u.contains("evil.example") || u.contains("cdn.example.com")), "{:?}", asked);
    }

    #[test]
    fn small_undescribed_and_linked_pictures_are_not_kept_and_the_limits_hold() {
        let dir = tempfile::tempdir().unwrap();
        let mut pages = vec![];
        let mut html = String::from("<p>x</p>");
        for i in 0..4 {
            let url = format!("https://en.wikipedia.org/img/{}.png", i);
            pages.push((url.clone(), ok(pic())));
            html.push_str(&format!("<figure><img alt=\"p{i}\" src=\"{url}\"><a rel=\"license\" href=\"https://creativecommons.org/publicdomain/zero/1.0/\">CC0</a></figure>"));
        }
        html.push_str("<figure><img alt=\"tiny\" src=\"/t.png\" width=\"16\" height=\"16\"><a rel=\"license\" href=\"https://creativecommons.org/publicdomain/zero/1.0/\">CC0</a></figure>");
        html.push_str("<figure><img alt=\"\" src=\"/n.png\"><a rel=\"license\" href=\"https://creativecommons.org/publicdomain/zero/1.0/\">CC0</a></figure>");
        html.push_str("<a href=\"/x\">words <img alt=\"in link\" src=\"/l.png\"></a>");
        let w = Web { pages: pages.into_iter().collect(), asked: Mutex::new(Vec::new()) };
        let o = run_with(&w, &html, dir.path(), LIST, 2, 24);
        assert_eq!(o.saved.len(), 1, "four pictures share the same bytes, so one file: {:?}", o.saved.iter().map(|s| &s.file).collect::<Vec<_>>());
        assert_eq!(o.fetched, 2, "the per-entry limit stops the fetching");
        let why: Vec<&str> = o.not_saved.iter().map(|(_, w)| w.as_str()).collect();
        assert_eq!(why.iter().filter(|w| w.contains("past the limit")).count(), 2, "{:?}", why);
        assert!(why.iter().any(|w| w.contains("too small")) && why.iter().any(|w| w.contains("decoration")) && why.iter().any(|w| w.contains("inside a link")), "{:?}", why);
        let o2 = run_with(&w, &html, dir.path(), LIST, 6, 1);
        assert_eq!(o2.fetched, 1, "the job's limit stops it too");
    }

    /// The red-team's P2-2: two SVGs that differ only in a comment draw the same PNG. Each is kept under its own hash, so each
    /// record's hash is the hash of the file it names (the SVG was named by the PNG's hash, and the second was not written).
    #[test]
    fn two_svgs_that_draw_the_same_png_are_each_kept_under_their_own_hash() {
        let dir = tempfile::tempdir().unwrap();
        let body = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"80\"><rect width=\"100\" height=\"80\" fill=\"#336699\"/></svg>";
        let (a, b) = (body.to_string(), body.replace("<rect", "<!-- another file --><rect"));
        let u = |n: &str| format!("https://en.wikipedia.org/img/{}.svg", n);
        let w = web(vec![(&u("a"), ok(a.clone().into_bytes())), (&u("b"), ok(b.clone().into_bytes()))]);
        let fig = |n: &str| format!("<figure><img alt=\"diagram {n}\" src=\"/img/{n}.svg\"><a rel=\"license\" href=\"https://creativecommons.org/publicdomain/zero/1.0/\">CC0</a></figure>");
        let o = run(&w, &format!("{}{}", fig("a"), fig("b")), dir.path());
        assert_eq!(o.saved.len(), 1, "one PNG: {:?}", o.not_saved);
        let host = dir.path().join(crate::research::DIR).join("en.wikipedia.org");
        let mut originals = Vec::new();
        for bytes in [&a, &b] {
            let hash = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let file = host.join("images").join(format!("{hash}.svg"));
            assert_eq!(std::fs::read(&file).unwrap(), bytes.as_bytes(), "each SVG is kept under its own hash");
            originals.push(hash);
        }
        let s = o.saved[0].svg.as_ref().unwrap();
        assert!(originals.contains(&s.blake3) && s.file == format!("images/{}.svg", s.blake3));
        assert_eq!(blake3::hash(&std::fs::read(host.join(&s.file)).unwrap()).to_hex().to_string(), s.blake3, "the record's hash is the file's");
    }

    #[test]
    fn a_second_pass_writes_no_new_file_and_gives_the_same_entry() {
        let dir = tempfile::tempdir().unwrap();
        let w = web(vec![(COMMONS_IMG, ok(pic())), (&api("File:Tree.png"), commons("CC BY-SA 4.0", "Ann"))]);
        let (a, b) = (run(&w, COMMONS_HTML, dir.path()), run(&w, COMMONS_HTML, dir.path()));
        assert_eq!((a.saved[0].new_file, b.saved[0].new_file), (true, false));
        assert_eq!(a.markdown, b.markdown);
        assert_eq!(files(dir.path()).len(), 1);
    }

    #[test]
    fn a_host_entry_that_says_its_pictures_carry_its_licence_is_enough_and_the_default_is_no() {
        let dir = tempfile::tempdir().unwrap();
        let gov = |images: bool| format!("[[source]]\nhost = \"*.gov\"\npublisher = \"US government\"\nlicence = \"US-Gov-PD\"\nreuse = \"full\"\nimages = {}\n", images);
        let w = web(vec![("https://www.nasa.gov/p.png", ok(pic()))]);
        let html = "<img alt=\"A rocket\" src=\"https://www.nasa.gov/p.png\">";
        let md = html::to_markdown_with_images(html, "https://www.nasa.gov/x", 10_000).markdown;
        let go = |images: bool| process(&w, &TrustedList::parse(&gov(images), None).unwrap(), &dir.path().join(crate::research::DIR), "www.nasa.gov", "https://www.nasa.gov/x", html, &md, 6, 24);
        let off = go(false);
        assert!(off.saved.is_empty() && off.not_saved[0].1.contains("no licence"), "{:?}", off.not_saved);
        let on = go(true);
        assert_eq!((on.saved.len(), on.saved[0].licence.as_str()), (1, "public-domain"));
        assert!(on.saved[0].attribution.starts_with("Image: US government, public domain, "), "{}", on.saved[0].attribution);
    }

    #[test]
    fn what_a_page_says_cannot_break_the_markdown_around_a_saved_picture() {
        let dir = tempfile::tempdir().unwrap();
        let w = web(vec![(COMMONS_IMG, ok(pic())), (&api("File:Tree.png"), commons("CC BY 4.0", "*Ann* [x](https://evil.example) \\u001b[2J\\n---\\nsource_type: user_ratified"))]);
        let o = run(&w, COMMONS_HTML, dir.path());
        let line = o.saved[0].attribution.clone();
        assert!(!line.contains(['*', '[', ']', '\n', '\x1b']), "{:?}", line);
        assert_eq!(o.markdown.matches("![").count(), 1);
        assert!(!o.markdown.contains("\n---\n"), "{}", o.markdown);
    }
}
