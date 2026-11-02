//! The research planner and the dedupe rules (`specs/066` §5, §6): which few queries a job sends, what is taken out of
//! them first, and when two URLs, two texts or two claims are the same thing.
//!
//! Deterministic code only: no model, no network. The runner of S4 calls it.
//!
//! * **At most [`MAX_QUERIES`] planned queries**: the topic, a trusted-constrained one, and one gap query. `quick` sends one.
//! * **Query privacy.** A host's question is cleaned before any engine sees it: secrets (the daemon's redactor), file paths,
//!   internal hostnames and addresses, and e-mail addresses are removed, and a question that is mostly such tokens yields
//!   no query at all. Host context pointers are never part of a query.
//! * **Trusted sources are a ranking and capture policy** (`delphi_core::research::capture_policy`); here they only shape the
//!   second query and decide whether the web is searched at all.

use delphi_adapter::research::search::site_queries;
use delphi_adapter::research::trusted::is_internal_host;
use delphi_core::redact::Redactor;
use delphi_core::research::Depth;
use std::collections::{BTreeMap, HashSet};

pub const MAX_QUERIES: usize = 3;
/// A trusted query names at most this many domains in `site:` filters.
const SITES_PER_QUERY: usize = 5;
/// Fewer kept trusted results than this and the wider web is searched too (`specs/066` §5).
pub const TRUSTED_ENOUGH: usize = 3;
const MAX_QUERY_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryKind {
    Topic,
    Trusted,
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedQuery {
    pub kind: QueryKind,
    pub text: String,
}

fn private_token(tok: &str) -> bool {
    let t = tok.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | ',' | ';' | '[' | ']' | '<' | '>'));
    if t.is_empty() {
        return false;
    }
    if t.starts_with('/') || t.starts_with("~/") || t.starts_with("./") || t.starts_with("../") || t.contains('\\') || t.get(1..3) == Some(":\\") {
        return true;
    }
    if t.contains('@') && t.contains('.') {
        return true;
    }
    let has_scheme = t.contains("://");
    let host = t.split("://").last().unwrap_or(t).split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    let host = host.rsplit_once(':').filter(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())).map(|(h, _)| h).unwrap_or(host);
    // A single plain word is never a host; a dotted token, an address or anything with a scheme is checked.
    (has_scheme || host.contains('.') || host.parse::<std::net::IpAddr>().is_ok()) && is_internal_host(host)
}

/// The question as a query: secrets, paths, internal hosts and addresses and e-mail addresses removed, whitespace folded,
/// at most [`MAX_QUERY_CHARS`]. `None` when nothing useful is left (empty, or more than half of the words were removed).
pub fn clean_query(question: &str, redactor: &Redactor) -> Option<String> {
    let first = question.split(['\n', '\r']).find(|l| !l.trim().is_empty())?;
    let (masked, _) = redactor.redact_str(first);
    let words: Vec<&str> = masked.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let kept: Vec<&str> = words.iter().copied().filter(|w| !private_token(w) && !w.contains("[REDACTED")).collect();
    if kept.is_empty() || kept.len() * 2 < words.len() {
        return None;
    }
    let mut out = String::new();
    for w in kept {
        if out.chars().count() + w.chars().count() + 1 > MAX_QUERY_CHARS {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    (!out.is_empty()).then_some(out)
}

/// The queries a job sends, in order. `sites` are the trusted-list domains (`TrustedList::site_filters`).
pub fn plan(question: &str, depth: Depth, sites: &[String], redactor: &Redactor) -> Vec<PlannedQuery> {
    let Some(topic) = clean_query(question, redactor) else { return Vec::new() };
    let trusted = site_queries(&topic, sites, SITES_PER_QUERY).into_iter().next().map(|text| PlannedQuery { kind: QueryKind::Trusted, text });
    let mut out = Vec::new();
    match depth {
        Depth::Quick => out.push(trusted.unwrap_or(PlannedQuery { kind: QueryKind::Topic, text: topic })),
        Depth::Survey | Depth::Deep => {
            out.push(PlannedQuery { kind: QueryKind::Topic, text: topic.clone() });
            out.extend(trusted);
            out.push(PlannedQuery { kind: QueryKind::Gap, text: format!("{} limitations OR criticism OR disputed", topic) });
        }
    }
    out.truncate(MAX_QUERIES);
    out
}

/// Search the wider web too? Only when the trusted domains answered too little.
pub fn widen_to_web(trusted_kept: usize) -> bool {
    trusted_kept < TRUSTED_ENOUGH
}

/// Query parameters that only say where a visitor came from.
fn tracking(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    k.starts_with("utm_") || matches!(k.as_str(), "fbclid" | "gclid" | "mc_cid" | "mc_eid" | "ref" | "ref_src" | "igshid" | "yclid")
}

/// One URL for one page: lower-case host, no fragment, no default port, no tracking parameters, the rest sorted, no
/// trailing slash except the root. `None` if it is not an http(s) URL.
pub fn canonical_url(url: &str) -> Option<String> {
    let mut u = delphi_adapter::reqwest::Url::parse(url.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    u.set_fragment(None);
    let _ = u.set_username("");
    let _ = u.set_password(None);
    let mut pairs: Vec<(String, String)> = u.query_pairs().filter(|(k, _)| !tracking(k)).map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    pairs.sort();
    if pairs.is_empty() {
        u.set_query(None);
    } else {
        u.query_pairs_mut().clear().extend_pairs(pairs);
    }
    let path = u.path().to_string();
    if path.len() > 1 && path.ends_with('/') {
        u.set_path(path.trim_end_matches('/'));
    }
    Some(u.to_string())
}

fn normal(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfkc().collect::<String>().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A hash of a page's text that ignores case, punctuation and spacing, so a re-flowed copy is the same copy.
pub fn content_hash(text: &str) -> String {
    blake3::hash(normal(text).as_bytes()).to_hex().to_string()
}

/// The identity of a claim: the same subject, predicate and object in the same scope and time is the same claim, however
/// it is worded around them. A paraphrase that changes those words is not caught (`specs/066` §11).
pub fn claim_fingerprint(subject: &str, predicate: &str, object: &str, scope: &str, time: &str) -> String {
    let joined = [subject, predicate, object, scope, time].iter().map(|p| normal(p)).collect::<Vec<_>>().join("\u{1f}");
    blake3::hash(joined.as_bytes()).to_hex()[..32].to_string()
}

/// What `ClaimSet::add` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// A claim this job and the library had not seen: it counts in `new_claims`.
    New,
    /// More evidence for a claim already in this job; the citation was merged.
    Merged,
    /// A claim the library already holds: reported in `kb_hits`, never staged again.
    Known,
}

/// The claims one job found, deduplicated by fingerprint, with every source that supports each.
#[derive(Debug, Default)]
pub struct ClaimSet {
    known: HashSet<String>,
    found: BTreeMap<String, Vec<String>>,
    kb_hits: Vec<String>,
}

impl ClaimSet {
    /// `known`: fingerprints of claims the library already holds.
    pub fn new(known: impl IntoIterator<Item = String>) -> ClaimSet {
        ClaimSet { known: known.into_iter().collect(), ..Default::default() }
    }

    pub fn add(&mut self, fingerprint: &str, source_id: &str) -> Added {
        if self.known.contains(fingerprint) {
            if !self.kb_hits.iter().any(|f| f == fingerprint) {
                self.kb_hits.push(fingerprint.to_string());
            }
            return Added::Known;
        }
        match self.found.get_mut(fingerprint) {
            Some(sources) => {
                if !sources.iter().any(|s| s == source_id) {
                    sources.push(source_id.to_string());
                }
                Added::Merged
            }
            None => {
                self.found.insert(fingerprint.to_string(), vec![source_id.to_string()]);
                Added::New
            }
        }
    }

    /// Deduplicated staged candidates: what `new_claims` reports.
    pub fn new_claims(&self) -> usize {
        self.found.len()
    }

    pub fn kb_hits(&self) -> &[String] {
        &self.kb_hits
    }

    pub fn sources_of(&self, fingerprint: &str) -> &[String] {
        self.found.get(fingerprint).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// URLs and texts already captured in this job, so one page is fetched and kept once.
#[derive(Debug, Default)]
pub struct Seen {
    urls: HashSet<String>,
    hashes: HashSet<String>,
}

impl Seen {
    /// `true` the first time this page (by canonical URL) is offered; `false` for a repeat or a non-http URL.
    pub fn url(&mut self, url: &str) -> bool {
        canonical_url(url).is_some_and(|c| self.urls.insert(c))
    }

    /// `true` the first time this text is offered, whatever URL it came from.
    pub fn text(&mut self, text: &str) -> bool {
        self.hashes.insert(content_hash(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sites() -> Vec<String> {
        vec!["who.int".into(), "nih.gov".into()]
    }

    #[test]
    fn a_quick_job_sends_one_trusted_query_and_a_deep_one_at_most_three() {
        let r = Redactor::new();
        let q = plan("How does intermittent fasting affect blood pressure?", Depth::Quick, &sites(), &r);
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].kind, QueryKind::Trusted);
        assert!(q[0].text.contains("site:who.int") && q[0].text.contains("site:nih.gov"), "{:?}", q[0]);
        let q = plan("How does intermittent fasting affect blood pressure?", Depth::Deep, &sites(), &r);
        assert_eq!(q.iter().map(|p| p.kind).collect::<Vec<_>>(), [QueryKind::Topic, QueryKind::Trusted, QueryKind::Gap]);
        assert!(plan("x y z", Depth::Survey, &[], &r).iter().all(|p| p.kind != QueryKind::Trusted));
        assert_eq!(plan("topic words here", Depth::Quick, &[], &r)[0].kind, QueryKind::Topic);
    }

    #[test]
    fn secrets_paths_internal_hosts_and_emails_never_reach_a_query() {
        let r = Redactor::new();
        let q = clean_query("why does rust borrow checker fail in /srv/app/src/main.rs for jess@corp.example on https://git.corp/x with ghp_abcdefghijklmnopqrstuvwxyz0123456789 at 10.0.0.5", &r).unwrap();
        for bad in ["/srv", "jess@", "git.corp", "ghp_", "10.0.0.5", "REDACTED"] {
            assert!(!q.contains(bad), "{bad} in {q:?}");
        }
        assert!(q.contains("rust borrow checker"), "{q:?}");
    }

    #[test]
    fn a_question_that_is_mostly_private_yields_no_query() {
        let r = Redactor::new();
        assert_eq!(clean_query("/etc/passwd /var/log/x 10.1.2.3 why", &r), None);
        assert_eq!(clean_query("   \n  ", &r), None);
        assert!(plan("/etc/passwd /var/log/x 10.1.2.3 why", Depth::Deep, &sites(), &r).is_empty());
    }

    #[test]
    fn only_the_first_line_is_used_and_the_query_is_bounded() {
        let r = Redactor::new();
        let long = "word ".repeat(100);
        assert!(clean_query(&long, &r).unwrap().chars().count() <= 200);
        assert_eq!(clean_query("first line\nsecret second line", &r).unwrap(), "first line");
    }

    #[test]
    fn the_web_is_searched_too_only_when_trusted_sources_answered_too_little() {
        assert!(widen_to_web(0) && widen_to_web(2));
        assert!(!widen_to_web(3) && !widen_to_web(10));
    }

    #[test]
    fn urls_that_are_one_page_get_one_canonical_form() {
        let a = canonical_url("HTTPS://Example.COM:443/a/b/?utm_source=x&b=2&a=1#frag").unwrap();
        let b = canonical_url("https://example.com/a/b?a=1&b=2&fbclid=zzz").unwrap();
        assert_eq!(a, b);
        assert_eq!(canonical_url("https://example.com/").unwrap(), "https://example.com/");
        assert_eq!(canonical_url("https://user:pw@example.com/x").unwrap(), "https://example.com/x");
        assert_eq!(canonical_url("ftp://example.com/x"), None);
        assert_eq!(canonical_url("not a url"), None);
    }

    #[test]
    fn a_reflowed_copy_has_the_same_content_hash_and_a_changed_one_does_not() {
        assert_eq!(content_hash("The  sky is BLUE.\n"), content_hash("the sky is blue"));
        assert_ne!(content_hash("the sky is blue"), content_hash("the sky is green"));
    }

    #[test]
    fn a_claim_is_the_same_claim_across_wording_and_punctuation_but_not_across_scope() {
        let a = claim_fingerprint("Aspirin", "reduces", "fever", "adults", "2024");
        assert_eq!(a, claim_fingerprint("aspirin ", "Reduces", "fever.", "Adults", "2024"));
        assert_ne!(a, claim_fingerprint("aspirin", "reduces", "fever", "children", "2024"));
        assert_ne!(a, claim_fingerprint("aspirin", "reduces", "fever", "adults", "2019"));
    }

    #[test]
    fn more_evidence_merges_a_known_claim_is_not_new_and_new_claims_counts_candidates() {
        let held = claim_fingerprint("a", "b", "c", "", "");
        let (x, y) = (claim_fingerprint("x", "is", "y", "", ""), claim_fingerprint("p", "is", "q", "", ""));
        let mut set = ClaimSet::new([held.clone()]);
        assert_eq!(set.add(&x, "s1"), Added::New);
        assert_eq!(set.add(&x, "s2"), Added::Merged);
        assert_eq!(set.add(&x, "s2"), Added::Merged);
        assert_eq!(set.add(&y, "s1"), Added::New);
        assert_eq!(set.add(&held, "s1"), Added::Known);
        assert_eq!(set.add(&held, "s3"), Added::Known);
        assert_eq!((set.new_claims(), set.kb_hits().len()), (2, 1));
        assert_eq!(set.sources_of(&x), ["s1", "s2"]);
    }

    #[test]
    fn one_page_and_one_text_are_kept_once() {
        let mut seen = Seen::default();
        assert!(seen.url("https://a.example/x?utm_medium=y"));
        assert!(!seen.url("https://A.example/x/"));
        assert!(!seen.url("mailto:a@b.c"));
        assert!(seen.text("hello world") && !seen.text("Hello, world!") && seen.text("other"));
    }
}
