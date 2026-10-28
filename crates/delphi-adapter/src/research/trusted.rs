//! The trusted-source lists (`specs/028` §10.3): the default list in the repository and the operator's own, used together.
//!
//! A host is fetchable by the research job only if it is on the union of the two, over HTTPS, and not blocked by the operator.
//! Nothing here learns a host from a page: the lists are read from files the operator owns.

use serde::Deserialize;

/// What a source may be stored as (`specs/028` §10.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Reuse {
    #[serde(rename = "full")]
    Full,
    #[serde(rename = "summary-only")]
    SummaryOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub host: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default = "unknown")]
    pub licence: String,
    pub reuse: Reuse,
    #[serde(default)]
    pub share_alike: bool,
    /// Pictures served from this host carry the entry's licence (`specs/028` §11.1): set only where that is true of every picture the host
    /// serves, such as a host of public-domain works. Without it a picture needs a licence of its own, declared where the page shows it.
    #[serde(default)]
    pub images: bool,
    /// Operator list only: remove this host (and its subdomains) for this install, though the default list names it.
    #[serde(default)]
    pub block: bool,
}

fn unknown() -> String {
    "unknown".into()
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    source: Vec<Source>,
}

const KINDS: [&str; 6] = ["standards", "government", "journal", "encyclopedia", "documentation", "news"];

fn parse_file(text: &str, what: &str, operator: bool) -> Result<Vec<Source>, String> {
    let f: File = toml::from_str(text).map_err(|e| format!("{}: {}", what, e))?;
    for s in &f.source {
        let h = s.host.trim();
        let bare = h.strip_prefix("*.").unwrap_or(h);
        if bare.is_empty() || bare.contains(['/', ':', ' ', '*', '@']) || !bare.contains('.') && !bare.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(format!("{}: `{}` is not a domain", what, s.host));
        }
        if !s.kind.is_empty() && !KINDS.contains(&s.kind.as_str()) {
            return Err(format!("{}: `{}` has kind `{}` (expected one of {})", what, s.host, s.kind, KINDS.join(", ")));
        }
        if s.block && !operator {
            return Err(format!("{}: `block` is for the operator's list; the default list only adds sources", what));
        }
    }
    Ok(f.source)
}

/// The union of the default list and the operator's.
#[derive(Debug, Clone, Default)]
pub struct TrustedList {
    sources: Vec<Source>,
    /// Which list each entry came from: the operator's entries win over the default's for the same host.
    from_operator: Vec<bool>,
}

/// Is `host` covered by the entry `pat` (`example.org` covers itself and its subdomains; `*.gov` covers any host ending `.gov`)?
fn covers(pat: &str, host: &str) -> bool {
    let (pat, host) = (pat.trim().to_ascii_lowercase(), host.to_ascii_lowercase());
    match pat.strip_prefix("*.") {
        Some(rest) => host.ends_with(&format!(".{}", rest)),
        None => host == pat || host.ends_with(&format!(".{}", pat)),
    }
}

/// How specific an entry is for a host: an entry naming a longer domain beats a shorter one, and an exact name beats a wildcard.
fn specificity(pat: &str) -> usize {
    pat.trim_start_matches("*.").len() * 2 + usize::from(!pat.starts_with("*."))
}

/// A host the job may never fetch whatever the lists say: addresses, single-label names and local-network suffixes.
pub fn is_internal_host(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    h.parse::<std::net::IpAddr>().is_ok()
        || h.starts_with('[')
        || !h.contains('.')
        || ["localhost", "local", "internal", "lan", "home", "corp", "intranet", "localdomain"].iter().any(|s| h == *s || h.ends_with(&format!(".{}", s)))
}

impl TrustedList {
    /// `default` is `context/sources/trusted.toml`; `operator` is `$DELPHI_HOME/operator/trusted-sources.toml` when it exists.
    pub fn parse(default: &str, operator: Option<&str>) -> Result<TrustedList, String> {
        let mut list = TrustedList::default();
        for s in parse_file(default, "context/sources/trusted.toml", false)? {
            list.sources.push(s);
            list.from_operator.push(false);
        }
        if let Some(text) = operator {
            for s in parse_file(text, "operator/trusted-sources.toml", true)? {
                list.sources.push(s);
                list.from_operator.push(true);
            }
        }
        Ok(list)
    }

    pub fn len(&self) -> usize {
        self.sources.iter().filter(|s| !s.block).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn sources(&self) -> impl Iterator<Item = &Source> {
        self.sources.iter().filter(|s| !s.block)
    }

    /// The entry that governs `host`, or `None` when the host is not trusted (not listed, blocked, or an internal address).
    pub fn lookup(&self, host: &str) -> Option<&Source> {
        if is_internal_host(host) {
            return None;
        }
        if self.sources.iter().any(|s| s.block && covers(&s.host, host)) {
            return None;
        }
        self.sources
            .iter()
            .zip(&self.from_operator)
            .filter(|(s, _)| !s.block && covers(&s.host, host))
            .max_by_key(|(s, op)| (specificity(&s.host), **op))
            .map(|(s, _)| s)
    }

    /// The entry for a URL the job may fetch: HTTPS, the default port, no credentials in the address, and a trusted host.
    pub fn allows_url(&self, url: &str) -> Option<&Source> {
        let u = reqwest::Url::parse(url).ok()?;
        if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() || u.port().is_some() {
            return None;
        }
        self.lookup(u.host_str()?)
    }

    /// The domains to put in `site:` filters, with the wildcard entries as their bare suffix (`gov`).
    pub fn site_filters(&self) -> Vec<String> {
        let mut v: Vec<String> = self.sources().map(|s| s.host.trim_start_matches("*.").to_ascii_lowercase()).collect();
        v.sort();
        v.dedup();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: &str = r#"
[[source]]
host = "en.wikipedia.org"
publisher = "Wikipedia"
kind = "encyclopedia"
licence = "CC-BY-SA-4.0"
reuse = "full"
share_alike = true
[[source]]
host = "*.gov"
reuse = "full"
[[source]]
host = "nih.gov"
reuse = "summary-only"
"#;

    fn list(op: Option<&str>) -> TrustedList {
        TrustedList::parse(DEFAULT, op).unwrap()
    }

    #[test]
    fn the_shipped_default_list_parses_and_names_wikipedia() {
        let l = TrustedList::parse(include_str!("../../../../context/sources/trusted.toml"), None).unwrap();
        let w = l.allows_url("https://en.wikipedia.org/wiki/Mitochondrion").unwrap();
        assert_eq!((w.reuse, w.share_alike), (Reuse::Full, true));
        assert!(l.allows_url("https://example.com/").is_none());
        assert!(l.len() >= 20);
    }

    #[test]
    fn a_host_matches_itself_and_its_subdomains_only_on_a_dot_boundary() {
        let l = list(None);
        assert!(l.lookup("en.wikipedia.org").is_some());
        assert!(l.lookup("EN.Wikipedia.ORG").is_some());
        assert!(l.lookup("a.en.wikipedia.org").is_some(), "a subdomain of a listed domain");
        assert!(l.lookup("xen.wikipedia.org").is_none(), "a longer label is not a subdomain");
        assert!(l.lookup("wikipedia.org").is_none(), "a parent domain is not covered by a subdomain entry");
        assert!(l.lookup("en.wikipedia.org.evil.com").is_none());
        assert!(l.lookup("notnih.gov").is_some(), "*.gov covers it, and nih.gov does not claim it");
        assert_eq!(l.lookup("notnih.gov").unwrap().reuse, Reuse::Full);
    }

    #[test]
    fn the_most_specific_entry_governs() {
        let l = list(None);
        assert_eq!(l.lookup("pubmed.nih.gov").unwrap().reuse, Reuse::SummaryOnly, "nih.gov is more specific than *.gov");
        assert_eq!(l.lookup("nasa.gov").unwrap().reuse, Reuse::Full);
    }

    #[test]
    fn the_operator_list_adds_overrides_and_blocks() {
        let op = r#"
[[source]]
host = "example.org"
publisher = "Example"
reuse = "summary-only"
[[source]]
host = "en.wikipedia.org"
reuse = "summary-only"
[[source]]
host = "nih.gov"
reuse = "summary-only"
block = true
"#;
        let l = list(Some(op));
        assert!(l.lookup("example.org").is_some(), "the operator's addition");
        assert_eq!(l.lookup("en.wikipedia.org").unwrap().reuse, Reuse::SummaryOnly, "the operator's entry wins for the same host");
        assert!(l.lookup("pubmed.nih.gov").is_none(), "blocked, though *.gov would cover it");
        assert!(l.lookup("nasa.gov").is_some());
        assert!(!l.sources().any(|s| s.block));
    }

    #[test]
    fn only_https_on_the_default_port_without_credentials() {
        let l = list(None);
        assert!(l.allows_url("https://en.wikipedia.org/wiki/X").is_some());
        for bad in ["http://en.wikipedia.org/wiki/X", "https://en.wikipedia.org:8443/", "https://user:pw@en.wikipedia.org/", "https://u@en.wikipedia.org/", "ftp://en.wikipedia.org/", "not a url", "https://en.wikipedia.org.evil.com/"] {
            assert!(l.allows_url(bad).is_none(), "{}", bad);
        }
    }

    #[test]
    fn internal_addresses_are_never_trusted_even_under_a_wildcard() {
        let l = TrustedList::parse("[[source]]\nhost = \"*.local\"\nreuse = \"full\"\n[[source]]\nhost = \"localhost\"\nreuse = \"full\"\n", None).unwrap();
        for h in ["localhost", "printer.local", "127.0.0.1", "10.0.0.5", "[::1]", "intranet", "wiki.corp"] {
            assert!(l.lookup(h).is_none(), "{}", h);
        }
    }

    #[test]
    fn bad_lists_are_refused_with_the_file_named() {
        let bad = |t: &str, op: bool| if op { TrustedList::parse(DEFAULT, Some(t)) } else { TrustedList::parse(t, None) }.unwrap_err();
        assert!(bad("[[source]]\nhost = \"https://x.org/\"\nreuse = \"full\"\n", false).contains("is not a domain"));
        assert!(bad("[[source]]\nhost = \"x.org\"\nreuse = \"sometimes\"\n", false).contains("trusted.toml"));
        assert!(bad("[[source]]\nhost = \"x.org\"\nreuse = \"full\"\nkind = \"blog\"\n", false).contains("kind"));
        assert!(bad("[[source]]\nhost = \"x.org\"\nreuse = \"full\"\nblock = true\n", false).contains("operator's list"));
        assert!(bad("[[source]]\nhost = \"x.org\"\nreuse = \"full\"\nextra = 1\n", true).contains("operator/trusted-sources.toml"));
    }

    #[test]
    fn site_filters_drop_the_wildcard_marker() {
        let f = list(None).site_filters();
        assert!(f.contains(&"gov".to_string()) && f.contains(&"en.wikipedia.org".to_string()));
    }
}
