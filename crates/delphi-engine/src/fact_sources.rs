//! Source classes for fact validation (`specs/040` §6.1, §6.3, Appendix A).
//!
//! The class of a page is decided here, by code, from its registrable domain and path, never by a model. The default
//! list is Appendix A; the operator extends it through `[validator.sources]` (their entries win). A page the list does
//! not know defaults to Weak, and Weak never counts toward corroboration.

use delphi_adapter::config::ValidatorSources;
use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Primary,
    ReputableSecondary,
    Weak,
    /// Hostile or unknown (§6.1): shorteners, caches, mirrors, archives. Noted in the record, never counted.
    Excluded,
}

impl Class {
    pub fn as_str(&self) -> &'static str {
        match self {
            Class::Primary => "primary",
            Class::ReputableSecondary => "reputable_secondary",
            Class::Weak => "weak",
            Class::Excluded => "excluded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub class: Class,
    /// An immutable or version-pinned artifact (an RFC, a tagged commit, a DOI, a filing): the only kind of page a
    /// known source alone may establish as a plain fact (`specs/040` §24.4).
    pub pinned: bool,
    /// Primary only for what the body publishes as official record (§6.3 governments); its other statements are
    /// attributed statements.
    pub record_only: bool,
    pub reason: &'static str,
}

fn c(class: Class, reason: &'static str) -> Classified {
    Classified { class, pinned: false, record_only: false, reason }
}

const EXCLUDED: [&str; 14] = [
    "bit.ly", "t.co", "tinyurl.com", "goo.gl", "ow.ly", "is.gd", "buff.ly", "webcache.googleusercontent.com", "cachedview.nl", "archive.org", "archive.ph", "archive.is", "web.archive.org", "r.jina.ai",
];
const WEAK: [&str; 21] = [
    "wikimedia.org", "wikia.com", "fandom.com", "reddit.com", "quora.com", "stackoverflow.com", "stackexchange.com", "medium.com", "substack.com", "x.com", "twitter.com", "facebook.com", "youtube.com", "discogs.com", "musicbrainz.org", "imdb.com", "genius.com", "blogspot.com", "wordpress.com", "tumblr.com", "linkedin.com",
];
const PRIMARY: [&str; 31] = [
    "ietf.org", "rfc-editor.org", "w3.org", "whatwg.org", "ecma-international.org", "unicode.org", "iso.org", "nist.gov", "legislation.gov.uk", "europa.eu", "sec.gov", "doi.org", "pubmed.ncbi.nlm.nih.gov", "clinicaltrials.gov", "arxiv.org", "crates.io", "pypi.org", "npmjs.com", "github.com", "copyright.gov", "loc.gov", "ascap.com", "bmi.com", "docs.rs", "rust-lang.org", "python.org", "kernel.org", "postgresql.org", "mozilla.org", "owasp.org", "iana.org",
];
const REPUTABLE: [&str; 48] = [
    "britannica.com", "apnews.com", "reuters.com", "bbc.co.uk", "bbc.com", "nytimes.com", "theguardian.com", "washingtonpost.com", "npr.org", "annualreviews.org", "si.edu", "smithsonianmag.com", "nationalgeographic.com", "billboard.com", "allmusic.com", "nature.com", "science.org",
    "wikipedia.org", "springer.com", "sciencedirect.com", "wiley.com", "tandfonline.com", "jstor.org", "cell.com", "thelancet.com", "nejm.org", "bmj.com", "pnas.org", "plos.org", "ieee.org", "acm.org", "aps.org", "academic.oup.com", "cambridge.org", "merriam-webster.com", "oed.com", "plato.stanford.edu", "worldbank.org", "who.int", "un.org", "oecd.org", "economist.com", "ft.com", "wsj.com", "bloomberg.com", "propublica.org", "ncbi.nlm.nih.gov", "europepmc.org",
];
/// Suffixes whose registrable domains are a class (§6.3), subject to the kind-of-claim rule.
const GOV_SUFFIXES: [&str; 3] = [".gov", ".gov.uk", ".gov.au"];
const EDU_SUFFIXES: [&str; 3] = [".edu", ".ac.uk", ".edu.au"];

fn matches(host: &str, dom: &str) -> bool {
    host == dom || host.ends_with(&format!(".{}", dom))
}

const JOURNAL_PUBLISHERS: [&str; 17] = [
    "springer.com", "sciencedirect.com", "wiley.com", "tandfonline.com", "jstor.org", "cell.com", "thelancet.com", "nejm.org", "bmj.com", "pnas.org", "plos.org", "ieee.org", "acm.org", "aps.org", "academic.oup.com", "cambridge.org", "nature.com",
];

fn host_of(url: &str) -> String {
    delphi_adapter::reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.trim_end_matches('.').to_ascii_lowercase())).unwrap_or_default()
}

/// A page on Wikipedia (`specs/040` §28.1).
pub fn is_wikipedia(url: &str) -> bool {
    matches(&host_of(url), "wikipedia.org")
}

/// A journal publisher's domain: a page there counts only when it is an article with a DOI (`specs/040` §28.2).
pub fn is_journal_publisher(url: &str) -> bool {
    let h = host_of(url);
    JOURNAL_PUBLISHERS.iter().any(|d| matches(&h, d))
}

fn pinned_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?ix)
            ^https?://(?:
                (?:www\.)?rfc-editor\.org/rfc/rfc\d+ |
                datatracker\.ietf\.org/doc/html/rfc\d+ |
                (?:www\.)?w3\.org/TR/\d{4}/ |
                (?:www\.)?github\.com/[^/]+/[^/]+/(?:blob|tree)/(?:[0-9a-f]{40}|v?\d[\w.\-]*)/ |
                (?:dx\.)?doi\.org/10\. |
                pubmed\.ncbi\.nlm\.nih\.gov/\d+ |
                arxiv\.org/abs/[\w.\-/]+v\d+ |
                (?:www\.)?sec\.gov/Archives/edgar/ |
                (?:www\.)?crates\.io/crates/[^/]+/\d |
                (?:www\.)?legislation\.gov\.uk/[^/]+/[^/]+/(?:enacted|\d{4}-\d{2}-\d{2}) |
                docs\.rs/[^/]+/\d
            )",
        )
        .unwrap()
    })
}

/// The default list (Appendix A) plus the operator's additions.
#[derive(Debug, Clone, Default)]
pub struct SourceList {
    extra: ValidatorSources,
    #[cfg(test)]
    pin_path: Option<String>,
}

impl SourceList {
    pub fn new(extra: ValidatorSources) -> SourceList {
        SourceList { extra, #[cfg(test)] pin_path: None }
    }

    /// Test only: a loopback fixture cannot match the pinned-host patterns, so a path prefix stands in for one.
    #[cfg(test)]
    pub(crate) fn with_test_pin(mut self, prefix: &str) -> SourceList {
        self.pin_path = Some(prefix.into());
        self
    }

    /// The class of the page at `final_url`. Only the final URL counts: a redirect does not lend its class.
    pub fn classify(&self, final_url: &str) -> Classified {
        let Ok(u) = delphi_adapter::reqwest::Url::parse(final_url) else {
            return c(Class::Excluded, "not a URL");
        };
        let host = u.host_str().unwrap_or("").trim_end_matches('.').to_ascii_lowercase();
        let path = u.path();
        #[allow(unused_mut)]
        let mut pinned = pinned_re().is_match(final_url);
        #[cfg(test)]
        if self.pin_path.as_deref().is_some_and(|p| path.starts_with(p)) {
            pinned = true;
        }
        let listed = |l: &[String]| l.iter().any(|d| matches(&host, &d.to_ascii_lowercase()));
        if EXCLUDED.iter().any(|d| matches(&host, d)) {
            return c(Class::Excluded, "a shortener, cache, mirror or archive: use the original");
        }
        if listed(&self.extra.weak) {
            return c(Class::Weak, "weak by the operator's list");
        }
        if listed(&self.extra.primary) {
            return Classified { class: Class::Primary, pinned, record_only: false, reason: "primary by the operator's list" };
        }
        if listed(&self.extra.reputable) {
            return c(Class::ReputableSecondary, "reputable by the operator's list");
        }
        if WEAK.iter().any(|d| matches(&host, d)) {
            return c(Class::Weak, "weak: a pointer to sources, never evidence");
        }
        if path.starts_with("/~") || path.contains("/~") {
            return c(Class::Weak, "a personal page");
        }
        if host == "github.com" || host.ends_with(".github.com") {
            return if pinned { Classified { class: Class::Primary, pinned, record_only: false, reason: "a repository at a tagged ref" } } else { c(Class::Weak, "github.com is primary only for a repository at a tagged ref") };
        }
        if host == "arxiv.org" {
            return Classified { class: Class::Primary, pinned, record_only: true, reason: "a preprint: authorship and what was claimed only" };
        }
        if GOV_SUFFIXES.iter().any(|s| host.ends_with(s)) || matches(&host, "europa.eu") {
            return Classified { class: Class::Primary, pinned, record_only: true, reason: "a government: official record only" };
        }
        if PRIMARY.iter().any(|d| matches(&host, d)) {
            return Classified { class: Class::Primary, pinned, record_only: false, reason: "primary (Appendix A)" };
        }
        if REPUTABLE.iter().any(|d| matches(&host, d)) || EDU_SUFFIXES.iter().any(|s| host.ends_with(s)) {
            return c(Class::ReputableSecondary, "reputable secondary (Appendix A)");
        }
        c(Class::Weak, "not on the list: unfamiliar domains default to weak")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(url: &str) -> Class {
        SourceList::default().classify(url).class
    }

    #[test]
    fn appendix_a_classes() {
        assert_eq!(class("https://www.rfc-editor.org/rfc/rfc2324"), Class::Primary);
        assert_eq!(class("https://www.britannica.com/topic/x"), Class::ReputableSecondary);
        assert_eq!(class("https://en.wikipedia.org/wiki/One_(song)"), Class::ReputableSecondary);
        assert_eq!(class("https://link.springer.com/article/1"), Class::ReputableSecondary);
        assert_eq!(class("https://commons.wikimedia.org/wiki/File:x.png"), Class::Weak);
        assert_eq!(class("https://www.biorxiv.org/content/1v1"), Class::Weak);
        assert_eq!(class("https://www.discogs.com/release/1"), Class::Weak);
        assert_eq!(class("https://bit.ly/abc"), Class::Excluded);
        assert_eq!(class("https://web.archive.org/web/2020/https://x.org"), Class::Excluded);
        assert_eq!(class("https://unknown-blog.example/post"), Class::Weak);
        assert_eq!(class("https://www.sec.gov/Archives/edgar/data/1/x.htm"), Class::Primary);
    }

    #[test]
    fn governments_schools_and_personal_pages_follow_the_kind_of_claim_rule() {
        let g = SourceList::default().classify("https://www.state.gov/briefing");
        assert_eq!((g.class, g.record_only), (Class::Primary, true));
        assert_eq!(class("https://cs.example.edu/research"), Class::ReputableSecondary);
        assert_eq!(class("https://cs.example.edu/~student/notes"), Class::Weak);
    }

    #[test]
    fn github_is_primary_only_at_a_tagged_ref_and_that_is_pinned() {
        let l = SourceList::default();
        let t = l.classify("https://github.com/o/r/blob/v1.2.3/README.md");
        assert_eq!((t.class, t.pinned), (Class::Primary, true));
        assert_eq!(l.classify("https://github.com/o/r/issues/4").class, Class::Weak);
        assert!(!l.classify("https://www.w3.org/TR/html/").pinned);
        assert!(l.classify("https://www.rfc-editor.org/rfc/rfc2324").pinned);
    }

    #[test]
    fn the_operators_list_wins_over_the_defaults() {
        let l = SourceList::new(ValidatorSources { primary: vec!["wiki.corp.example".into()], reputable: vec![], weak: vec!["britannica.com".into()] });
        assert_eq!(l.classify("https://wiki.corp.example/page").class, Class::Primary);
        assert_eq!(l.classify("https://www.britannica.com/x").class, Class::Weak);
    }
}
