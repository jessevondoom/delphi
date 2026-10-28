//! What the research job writes (`specs/028` §10.2 step 4, §10.5): one Markdown document per kept page under
//! `knowledge/web/<host>/`, a **full text** entry or a **summary and facts** entry, each with where it came from.
//!
//! Two rules are enforced here, in code, whatever the scout said:
//!
//! * **A summary's facts must be quotes.** Each fact carries a quote of at most one sentence, and the quote must appear in
//!   the page it was taken from (compared after collapsing whitespace and case). A fact whose quote is not in the page is
//!   dropped: the scout cannot invent what a source said.
//! * **The form is the list's, not the scout's.** The scout may ask for `full`, but [`crate::research_sources::form`]
//!   decides; a page that may only be summarised is never stored whole.
//!
//! The daemon writes these files, never the scout, and only under the research root.

use crate::research_sources::{share_alike, Entry, Form};
use std::path::{Path, PathBuf};

/// The scout's choice for one page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    Full,
    Summary,
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub fact: String,
    pub quote: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub url: String,
    pub keep: Keep,
    pub summary: String,
    pub facts: Vec<Fact>,
}

/// A page ready to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub url: String,
    pub form: Form,
    pub title: String,
    pub body: String,
}

const MAX_SUMMARY: usize = 1200;
const MAX_FACTS: usize = 12;
const MAX_FACT: usize = 300;
const MAX_QUOTE: usize = 300;

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// At most one sentence: no full stop, question or exclamation mark that is followed by a new sentence (a space and a capital
/// letter or a quote and a capital). A mark inside a number (`3.5`) or after a common abbreviation (`e.g.`, `Dr.`) is not an end.
fn one_sentence(q: &str) -> bool {
    const ABBREV: [&str; 14] = ["e.g", "i.e", "etc", "vs", "dr", "mr", "mrs", "ms", "fig", "no", "inc", "ltd", "st", "cf"];
    let t = q.trim();
    let chars: Vec<(usize, char)> = t.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let rest = &t[i + c.len_utf8()..];
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let next = rest.trim_start().trim_start_matches(['"', '\'', '\u{201c}', '\u{2018}', '(']).chars().next();
        if !next.is_some_and(|n| n.is_uppercase()) {
            continue;
        }
        let word: String = chars[..k].iter().rev().take_while(|(_, ch)| ch.is_alphanumeric() || *ch == '.').map(|(_, ch)| *ch).collect::<Vec<_>>().into_iter().rev().collect();
        if c == '.' && ABBREV.contains(&word.to_lowercase().as_str()) {
            continue;
        }
        return false;
    }
    true
}

fn clip(s: &str, n: usize) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > n { t.chars().take(n).collect::<String>() } else { t }
}

/// Turn a verdict into something to store, or `None`. `page` is the text the scout read; `allowed` is what the list allows
/// for this page (`Form`), and a verdict never gets more than that.
pub fn keep(v: &Verdict, title: &str, page: &str, allowed: Form) -> Option<Kept> {
    let form = match (v.keep, allowed) {
        (Keep::Nothing, _) => return None,
        (Keep::Full, Form::Full) => Form::Full,
        _ => Form::Summary,
    };
    if form == Form::Full {
        return Some(Kept { url: v.url.clone(), form, title: title.to_string(), body: page.trim().to_string() });
    }
    let haystack = norm(page);
    let facts: Vec<&Fact> = v.facts.iter().filter(|f| !f.fact.trim().is_empty() && !f.quote.trim().is_empty() && f.quote.chars().count() <= MAX_QUOTE && one_sentence(&f.quote) && haystack.contains(&norm(&f.quote))).take(MAX_FACTS).collect();
    let summary = clip(&v.summary, MAX_SUMMARY);
    if summary.is_empty() && facts.is_empty() {
        return None;
    }
    let mut body = String::new();
    if !summary.is_empty() {
        body.push_str(&summary);
        body.push_str("\n\n");
    }
    if !facts.is_empty() {
        body.push_str("## Facts\n\n");
        for f in &facts {
            body.push_str(&format!("- {} \u{2014} \u{201c}{}\u{201d}\n", clip(&f.fact, MAX_FACT), clip(&f.quote, MAX_QUOTE)));
        }
    }
    Some(Kept { url: v.url.clone(), form, title: title.to_string(), body: body.trim_end().to_string() })
}

/// A file name from a URL's path: lowercase ASCII letters, digits and `-`, never empty, at most 80 characters.
pub fn slug(path_and_query: &str) -> String {
    let p = path_and_query.split(['?', '#']).next().unwrap_or("");
    let mut out = String::new();
    for c in p.trim_matches('/').chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    let out: String = out.chars().take(80).collect();
    let out = out.trim_end_matches('-');
    if out.is_empty() { "index".into() } else { out.to_string() }
}

/// The logical path of a stored page: `knowledge/web/<host>/<slug>.md`. The host and slug are plain ASCII, so the path
/// cannot climb out of the research root.
pub fn logical_path(host: &str, path_and_query: &str) -> String {
    format!("knowledge/web/{}/{}.md", host.trim_start_matches("*."), slug(path_and_query))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    New,
    Updated,
    Unchanged,
}

/// Everything the document says about where it came from.
pub struct Provenance<'a> {
    pub mission: &'a str,
    pub topic: &'a str,
    pub retrieved: &'a str,
    pub entry: &'a Entry,
}

fn yaml(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace(['\n', '\r'], " "))
}

/// The document as stored. The frontmatter is the daemon's: `source_type` is the research tier (L2, a validated source,
/// `specs/001` §5) and the index fixes that tier by the folder, so an entry cannot claim a higher one.
pub fn render(k: &Kept, p: &Provenance<'_>) -> String {
    let hash = blake3::hash(k.body.as_bytes()).to_hex().to_string();
    let form = if k.form == Form::Full { "full" } else { "summary" };
    let sa = share_alike(&p.entry.licence);
    let title = if k.title.trim().is_empty() { k.url.clone() } else { k.title.trim().to_string() };
    let attribution = format!("Source: {} \u{b7} {} \u{b7} licence {}{}", k.url, p.entry.publisher, p.entry.licence, if sa { " (share-alike)" } else { "" });
    let mut s = String::new();
    s.push_str("---\n");
    s.push_str(&format!("id: web-{}\n", &hash[..12]));
    s.push_str(&format!("title: {}\n", yaml(&title)));
    s.push_str("author: scout\n");
    s.push_str(&format!("timestamp: {}\n", p.retrieved));
    s.push_str("source_type: federated_verified\n");
    s.push_str("confidence: low\nstatus: active\n");
    s.push_str(&format!("source_mission: {}\n", p.mission));
    s.push_str(&format!("tags: [research, {}]\n", p.entry.kind));
    s.push_str(&format!("source_url: {}\n", yaml(&k.url)));
    s.push_str(&format!("publisher: {}\n", yaml(&p.entry.publisher)));
    s.push_str(&format!("retrieved: {}\n", p.retrieved));
    s.push_str(&format!("licence: {}\n", p.entry.licence));
    s.push_str(&format!("share_alike: {}\n", sa));
    s.push_str(&format!("kind: {}\n", form));
    s.push_str(&format!("topic: {}\n", yaml(p.topic)));
    s.push_str("trust: trusted_source\n");
    s.push_str(&format!("content_blake3: {}\n", hash));
    s.push_str("---\n\n");
    s.push_str(&format!("# {}\n\n> {}\n\n", title, attribution));
    s.push_str(&k.body);
    s.push('\n');
    s
}

fn stored_hash(text: &str) -> Option<&str> {
    text.lines().take(30).find_map(|l| l.strip_prefix("content_blake3: ")).map(str::trim)
}

/// Write one entry under `root` (the research root: `<workspace>/local/knowledge/web`'s parent, so that `logical` is
/// `knowledge/web/…` below `base`). Returns what happened. An entry with the same content is left alone (and not counted).
pub fn write(base: &Path, logical: &str, k: &Kept, p: &Provenance<'_>) -> std::io::Result<Wrote> {
    if !logical.starts_with("knowledge/web/") || logical.contains("..") || logical.contains("//") || logical.contains('\0') {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "a research entry is written only under knowledge/web/"));
    }
    let path: PathBuf = base.join(logical);
    let text = render(k, p);
    let hash = blake3::hash(k.body.as_bytes()).to_hex().to_string();
    let existed = match std::fs::read_to_string(&path) {
        Ok(old) => {
            if stored_hash(&old) == Some(hash.as_str()) {
                return Ok(Wrote::Unchanged);
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // A symlink in the way is refused: the research root is the daemon's to write, not a place to follow a link out of.
    if std::fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "refusing to write through a symlink"));
    }
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(if existed { Wrote::Updated } else { Wrote::New })
}

/// The scout's answer, read leniently: a JSON array (optionally in a code fence) of
/// `{url, keep: "full"|"summary"|"none", summary, facts: [{fact, quote}]}`. `None` when it is not that.
pub fn parse_verdicts(text: &str) -> Option<Vec<Verdict>> {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).map(|r| r.trim_start()).unwrap_or(t);
    let t = t.strip_suffix("```").map(str::trim_end).unwrap_or(t);
    let start = t.find('[')?;
    let end = t.rfind(']')?;
    let v: serde_json::Value = serde_json::from_str(&t[start..=end]).ok()?;
    let mut out = Vec::new();
    for r in v.as_array()? {
        let keep = match r["keep"].as_str()? {
            "full" => Keep::Full,
            "summary" => Keep::Summary,
            "none" => Keep::Nothing,
            _ => return None,
        };
        let facts = r["facts"].as_array().map(|a| a.iter().filter_map(|f| Some(Fact { fact: f["fact"].as_str()?.to_string(), quote: f["quote"].as_str()?.to_string() })).collect()).unwrap_or_default();
        out.push(Verdict { url: r["url"].as_str()?.to_string(), keep, summary: r["summary"].as_str().unwrap_or("").to_string(), facts });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(reuse: &str, licence: &str) -> Entry {
        Entry { host: "en.wikipedia.org".into(), publisher: "Wikimedia Foundation".into(), kind: "encyclopedia".into(), licence: licence.into(), reuse: reuse.into(), block: false }
    }
    const PAGE: &str = "A Byzantine fault is a condition in which components fail in arbitrary ways. A system is Byzantine fault tolerant when it keeps working correctly despite such failures.";
    fn fact(f: &str, q: &str) -> Fact {
        Fact { fact: f.into(), quote: q.into() }
    }
    fn v(keep: Keep, summary: &str, facts: Vec<Fact>) -> Verdict {
        Verdict { url: "https://en.wikipedia.org/wiki/BFT".into(), keep, summary: summary.into(), facts }
    }

    #[test]
    fn full_text_is_stored_only_when_the_list_allows_it_whatever_the_scout_asked() {
        let k = keep(&v(Keep::Full, "", vec![]), "BFT", PAGE, Form::Full).unwrap();
        assert_eq!((k.form, k.body.as_str()), (Form::Full, PAGE));
        let asked_more = keep(&v(Keep::Full, "A short summary.", vec![fact("faults are arbitrary", "components fail in arbitrary ways")]), "BFT", PAGE, Form::Summary).unwrap();
        assert_eq!(asked_more.form, Form::Summary, "a summary-only page is never stored whole");
        assert!(!asked_more.body.contains("A system is Byzantine fault tolerant"), "no page body is copied");
        assert!(keep(&v(Keep::Nothing, "x", vec![]), "t", PAGE, Form::Full).is_none());
        assert!(keep(&v(Keep::Summary, "", vec![]), "t", PAGE, Form::Full).is_none(), "nothing to store");
    }

    #[test]
    fn a_fact_survives_only_if_its_quote_is_one_sentence_taken_from_the_page() {
        let facts = vec![
            fact("real", "components fail in arbitrary ways"),
            fact("real with other spacing and case", "A  BYZANTINE fault is a condition"),
            fact("invented", "Byzantine faults were discovered in 1982"),
            fact("two sentences", "A Byzantine fault is a condition in which components fail in arbitrary ways. A system is Byzantine fault tolerant"),
            fact("", "components fail"),
            fact("empty quote", ""),
            fact("too long", &"components fail in arbitrary ways ".repeat(20)),
        ];
        let k = keep(&v(Keep::Summary, "About faults.", facts), "BFT", PAGE, Form::Summary).unwrap();
        let lines: Vec<&str> = k.body.lines().filter(|l| l.starts_with("- ")).collect();
        assert_eq!(lines.len(), 2, "{}", k.body);
        assert!(lines[0].contains("\u{201c}components fail in arbitrary ways\u{201d}") && lines[0].contains("real"));
        assert!(!k.body.contains("1982") && !k.body.contains("two sentences"));
        assert!(one_sentence("e.g. 3.5 percent is fine") && one_sentence("Version 1.2 shipped") && one_sentence("Dr. Lamport wrote it") && !one_sentence("One. Two.") && !one_sentence("Really? Yes.") && !one_sentence("It failed! \"Then\" it ran"));
    }

    #[test]
    fn slugs_and_paths_stay_plain_ascii_under_the_research_folder() {
        assert_eq!(slug("/wiki/Byzantine_fault?action=raw#History"), "wiki-byzantine-fault");
        assert_eq!(slug("/"), "index");
        assert_eq!(slug("/\u{202e}../etc/passwd"), "etc-passwd");
        assert!(slug(&"a".repeat(500)).len() <= 80);
        assert_eq!(logical_path("en.wikipedia.org", "/wiki/BFT"), "knowledge/web/en.wikipedia.org/wiki-bft.md");
        assert_eq!(logical_path("*.gov", "/x"), "knowledge/web/gov/x.md", "a wildcard entry never names a folder with a star");
    }

    #[test]
    fn the_document_says_where_it_came_from_and_a_share_alike_licence_is_flagged() {
        let k = keep(&v(Keep::Full, "", vec![]), "Byzantine fault", PAGE, Form::Full).unwrap();
        let e = entry("full", "CC-BY-SA-4.0");
        let text = render(&k, &Provenance { mission: "m_1", topic: "byzantine \"fault\"\ntolerance", retrieved: "2026-10-28T10:00:00Z", entry: &e });
        for want in ["source_type: federated_verified", "status: active", "source_url: \"https://en.wikipedia.org/wiki/BFT\"", "publisher: \"Wikimedia Foundation\"", "licence: CC-BY-SA-4.0", "share_alike: true", "kind: full", "trust: trusted_source", "topic: \"byzantine \\\"fault\\\" tolerance\"", "tags: [research, encyclopedia]", "# Byzantine fault", "(share-alike)", PAGE] {
            assert!(text.contains(want), "{want}\n{text}");
        }
        assert!(text.starts_with("---\nid: web-"));
        let free = render(&k, &Provenance { mission: "m_1", topic: "t", retrieved: "2026-10-28T10:00:00Z", entry: &entry("full", "CC0-1.0") });
        assert!(free.contains("share_alike: false") && !free.contains("(share-alike)"));
    }

    #[test]
    fn writing_is_idempotent_a_changed_page_is_an_update_and_the_path_cannot_climb() {
        let base = tempfile::tempdir().unwrap();
        let e = entry("full", "CC-BY-SA-4.0");
        let p = Provenance { mission: "m_1", topic: "t", retrieved: "2026-10-28T10:00:00Z", entry: &e };
        let k = keep(&v(Keep::Full, "", vec![]), "BFT", PAGE, Form::Full).unwrap();
        let logical = logical_path("en.wikipedia.org", "/wiki/BFT");
        assert_eq!(write(base.path(), &logical, &k, &p).unwrap(), Wrote::New);
        assert!(std::fs::read_to_string(base.path().join(&logical)).unwrap().contains("content_blake3: "));
        assert_eq!(write(base.path(), &logical, &k, &p).unwrap(), Wrote::Unchanged, "the same page again is not counted");
        let changed = Kept { body: format!("{PAGE} A new sentence."), ..k.clone() };
        assert_eq!(write(base.path(), &logical, &changed, &p).unwrap(), Wrote::Updated);
        for bad in ["knowledge/web/../../x.md", "specs/x.md", "knowledge/web//x.md", "/etc/passwd", "knowledge/web/a\0b.md"] {
            assert!(write(base.path(), bad, &k, &p).is_err(), "{bad}");
        }
        assert!(!base.path().join("specs").exists());
        #[cfg(unix)]
        {
            let target = tempfile::tempdir().unwrap();
            let l2 = logical_path("evil.org", "/p");
            std::fs::create_dir_all(base.path().join("knowledge/web/evil.org")).unwrap();
            std::os::unix::fs::symlink(target.path().join("x.md"), base.path().join(&l2)).unwrap();
            assert!(write(base.path(), &l2, &k, &p).is_err(), "a symlink in the way is refused");
            assert!(!target.path().join("x.md").exists());
        }
    }

    #[test]
    fn the_scouts_answer_is_read_leniently_and_a_wrong_shape_is_refused() {
        let ok = "```json\n[{\"url\":\"https://a.org/x\",\"keep\":\"summary\",\"summary\":\"S.\",\"facts\":[{\"fact\":\"f\",\"quote\":\"q\"},{\"fact\":1}]},{\"url\":\"https://b.org/\",\"keep\":\"none\"}]\n```";
        let vs = parse_verdicts(ok).unwrap();
        assert_eq!((vs.len(), vs[0].keep, vs[0].facts.len(), vs[1].keep), (2, Keep::Summary, 1, Keep::Nothing), "a malformed fact is dropped, not the whole answer");
        assert!(parse_verdicts("Here you go: [{\"url\":\"https://a.org/\",\"keep\":\"full\"}] thanks").is_some());
        for bad in ["no json at all", "[{\"url\":\"u\",\"keep\":\"everything\"}]", "[{\"keep\":\"full\"}]", "{\"url\":\"u\"}"] {
            assert!(parse_verdicts(bad).is_none(), "{bad}");
        }
    }
}
