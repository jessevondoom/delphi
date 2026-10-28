//! The trusted-source lists of the research job (`specs/028` §10.3, amendment 028-A1).
//!
//! A source is trusted for the research job **only because the operator put it on a list**. There are two:
//!
//! * the **default list**, `context/sources/trusted.toml`, committed to the repository and shipped in the release;
//! * the **operator list**, `$DELPHI_HOME/operator/trusted-sources.toml`, which only the operator edits (agents cannot
//!   write under `operator/`, `specs/001` §2.2). An entry there adds a host, or with `block = true` removes one.
//!
//! The job's allow list is the **union**, read fresh at each job. Nothing a page says, and nothing a model chooses, can add
//! a host (`specs/001` §5: content read from outside never raises its own trust). A URL is matched by its **host** over
//! **HTTPS** only: an exact name, or a `*.suffix` wildcard on a short list of public-sector suffixes. A host the daemon
//! cannot classify (a non-ASCII name, an IP address, a port, a user name in the URL) matches nothing.

use serde::Deserialize;

/// The kinds a source may be (`specs/028` §10.3). A label for the operator; it carries no authority.
pub const KINDS: [&str; 6] = ["standards", "government", "journal", "encyclopedia", "documentation", "news"];

/// One source: a host (or `*.suffix`), who publishes it, what it is, and what may be copied from it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub host: String,
    pub publisher: String,
    pub kind: String,
    /// An SPDX id, or `unknown`. Never guessed from page text.
    #[serde(default = "unknown")]
    pub licence: String,
    /// `full` (the page may be stored as Markdown) or `summary-only` (a summary and facts, each with a quote of at most one sentence).
    #[serde(default = "summary_only")]
    pub reuse: String,
    /// In the operator list: remove this host from the default list for this install.
    #[serde(default)]
    pub block: bool,
}

fn unknown() -> String {
    "unknown".into()
}
fn summary_only() -> String {
    "summary-only".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    source: Vec<Entry>,
}

/// Wildcards are allowed only on public-sector suffixes, where everything under the suffix is run by that sector. A
/// wildcard on `*.com` would trust everyone.
fn wildcard_ok(suffix: &str) -> bool {
    let labels: Vec<&str> = suffix.split('.').collect();
    match labels.as_slice() {
        [one] => ["gov", "edu", "mil", "int"].contains(one),
        [first, _second] => ["gov", "ac", "edu", "gob", "go", "gouv"].contains(first),
        _ => false,
    }
}

fn valid_host_chars(h: &str) -> bool {
    !h.is_empty() && h.len() <= 253 && h.split('.').all(|l| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}

/// Check one entry; the problems found, if any.
pub fn check(e: &Entry) -> Vec<String> {
    let mut p = Vec::new();
    let (wild, name) = match e.host.strip_prefix("*.") {
        Some(rest) => (true, rest),
        None => (false, e.host.as_str()),
    };
    if !valid_host_chars(name) || !name.contains('.') && !wild {
        p.push(format!("`{}`: a host is a lowercase ASCII name such as `example.org`, with no scheme, port or path", e.host));
    } else if wild && !wildcard_ok(name) {
        p.push(format!("`{}`: a wildcard is allowed only on a public-sector suffix (`*.gov`, `*.edu`, `*.mil`, `*.int`, `*.gov.uk`, `*.ac.uk`, …)", e.host));
    }
    if e.publisher.trim().is_empty() {
        p.push(format!("`{}`: `publisher` is required", e.host));
    }
    if !KINDS.contains(&e.kind.as_str()) {
        p.push(format!("`{}`: `kind` must be one of {}", e.host, KINDS.join(", ")));
    }
    if !["full", "summary-only"].contains(&e.reuse.as_str()) {
        p.push(format!("`{}`: `reuse` must be `full` or `summary-only`", e.host));
    }
    if e.licence.trim().is_empty() || e.licence.chars().any(|c| c.is_whitespace() || c.is_control()) {
        p.push(format!("`{}`: `licence` is an SPDX id such as `CC-BY-SA-4.0`, or `unknown`", e.host));
    }
    if e.reuse == "full" && e.licence == "unknown" {
        p.push(format!("`{}`: `reuse = \"full\"` needs a known licence (an unknown licence is summary-only)", e.host));
    }
    p
}

/// Parse a list. Every problem in it is returned, not just the first. `origin` names the file in the messages.
pub fn parse(text: &str, origin: &str) -> Result<Vec<Entry>, Vec<String>> {
    let file: File = toml::from_str(text).map_err(|e| vec![format!("{}: not a valid trusted-source list: {}", origin, e)])?;
    let mut errs: Vec<String> = file.source.iter().flat_map(check).map(|m| format!("{}: {}", origin, m)).collect();
    let mut seen = std::collections::BTreeSet::new();
    for e in &file.source {
        if !seen.insert((e.host.clone(), e.block)) {
            errs.push(format!("{}: `{}` is listed twice", origin, e.host));
        }
    }
    if errs.is_empty() { Ok(file.source) } else { Err(errs) }
}

/// A URL's host and path if it is a plain `https://` URL a source can be matched against; otherwise `None`.
///
/// The URL must name a lowercase-able ASCII host with no user name, no port other than the default, and no IP literal.
/// A name the daemon cannot classify is not guessed at.
pub fn https_host(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("HTTPS://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (auth, tail) = rest.split_at(end);
    if auth.contains('@') || auth.contains('[') || auth.is_empty() || auth.contains('\\') || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let host = match auth.rsplit_once(':') {
        Some((h, "443")) => h,
        Some(_) => return None,
        None => auth,
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if !valid_host_chars(&host) || !host.contains('.') || host.split('.').all(|l| l.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    Some((host, tail.to_string()))
}

/// The allow list of one job: the default list and the operator's, with the operator's blocks applied.
#[derive(Debug, Clone, Default)]
pub struct Allow {
    entries: Vec<Entry>,
    blocked: Vec<String>,
}

fn matches(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(suffix) => host.len() > suffix.len() + 1 && host.ends_with(suffix) && host.as_bytes()[host.len() - suffix.len() - 1] == b'.',
        None => pattern == host,
    }
}

impl Allow {
    /// `operator` entries add hosts; one with `block = true` removes that host (or wildcard) from the default list, and a
    /// URL matching a blocked pattern is refused even if a wider default pattern would have allowed it.
    pub fn union(default: &[Entry], operator: &[Entry]) -> Allow {
        let blocked: Vec<String> = operator.iter().filter(|e| e.block).map(|e| e.host.clone()).collect();
        let mut entries: Vec<Entry> = default.iter().filter(|e| !e.block && !blocked.contains(&e.host)).cloned().collect();
        for e in operator.iter().filter(|e| !e.block) {
            entries.retain(|d| d.host != e.host);
            entries.push(e.clone());
        }
        Allow { entries, blocked }
    }

    /// Read both lists fresh: the shipped one under `<ws>/base/context/sources/trusted.toml` and the operator's under
    /// `<home>/operator/trusted-sources.toml`. A missing file is an empty list; an invalid one is an error naming the file.
    pub fn load(ws: &std::path::Path, home: &std::path::Path) -> Result<Allow, Vec<String>> {
        let read = |p: std::path::PathBuf| -> Result<Vec<Entry>, Vec<String>> {
            match std::fs::read_to_string(&p) {
                Ok(t) => parse(&t, &p.display().to_string()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(e) => Err(vec![format!("{}: {}", p.display(), e)]),
            }
        };
        let default = read(ws.join("base/context/sources/trusted.toml"))?;
        let operator = read(home.join("operator/trusted-sources.toml"))?;
        Ok(Allow::union(&default, &operator))
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry that allows `url`, or `None`: it is not plain HTTPS, its host cannot be classified, it is blocked, or no
    /// entry names it. An exact entry wins over a wildcard.
    pub fn lookup(&self, url: &str) -> Option<&Entry> {
        let (host, _) = https_host(url)?;
        if self.blocked.iter().any(|b| matches(b, &host)) {
            return None;
        }
        self.entries.iter().find(|e| e.host == host).or_else(|| self.entries.iter().find(|e| e.host.starts_with("*.") && matches(&e.host, &host)))
    }

    /// The exact (non-wildcard) hosts, for building a site-restricted search query.
    pub fn exact_hosts(&self) -> Vec<&str> {
        self.entries.iter().filter(|e| !e.host.starts_with("*.")).map(|e| e.host.as_str()).collect()
    }
}

/// What may be stored of a page (`specs/028` §10.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// The page as Markdown, with its licence and an attribution line.
    Full,
    /// A summary and facts, each fact with its source URL and a quote of at most one sentence. No page body is copied.
    Summary,
}

/// Wording that says a page is stricter than its source's list entry. Found in the page's own text, it turns a `full`
/// source into a summary for that page. The job never *guesses a licence* from page text; this only withholds.
const STRICTER: [&str; 8] = ["all rights reserved", "no derivative", "noderivs", "not for redistribution", "do not reproduce", "may not be reproduced", "permission is required to reproduce", "no reproduction"];

pub fn stricter_notice(page_text: &str) -> bool {
    let t = page_text.to_ascii_lowercase();
    STRICTER.iter().any(|m| t.contains(m))
}

/// Full text only when the entry says `full`, the licence is known, and the page carries no stricter notice. Everything
/// else is a summary; an unknown licence is always a summary.
pub fn form(entry: &Entry, page_text: &str) -> Form {
    if entry.reuse == "full" && entry.licence != "unknown" && !stricter_notice(page_text) { Form::Full } else { Form::Summary }
}

/// A share-alike licence (`CC-BY-SA-*`, `ODbL`, `GPL`…): an export of the collection must carry it.
pub fn share_alike(licence: &str) -> bool {
    let l = licence.to_ascii_uppercase();
    l.contains("-SA") || l.starts_with("ODBL") || l.contains("GPL")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(host: &str, reuse: &str, licence: &str) -> Entry {
        Entry { host: host.into(), publisher: "P".into(), kind: "encyclopedia".into(), licence: licence.into(), reuse: reuse.into(), block: false }
    }

    #[test]
    fn a_list_parses_and_every_problem_is_reported() {
        let ok = "[[source]]\nhost = \"en.wikipedia.org\"\npublisher = \"Wikimedia\"\nkind = \"encyclopedia\"\nlicence = \"CC-BY-SA-4.0\"\nreuse = \"full\"\n\n[[source]]\nhost = \"*.gov\"\npublisher = \"US government\"\nkind = \"government\"\nlicence = \"CC0-1.0\"\nreuse = \"full\"\n";
        let v = parse(ok, "t").unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].host, "*.gov");
        let bad = "[[source]]\nhost = \"https://x.org/a\"\npublisher = \"\"\nkind = \"blog\"\nreuse = \"everything\"\n\n[[source]]\nhost = \"*.com\"\npublisher = \"x\"\nkind = \"news\"\n\n[[source]]\nhost = \"a.org\"\npublisher = \"x\"\nkind = \"news\"\nreuse = \"full\"\n\n[[source]]\nhost = \"dup.org\"\npublisher = \"x\"\nkind = \"news\"\n\n[[source]]\nhost = \"dup.org\"\npublisher = \"x\"\nkind = \"news\"\n";
        let errs = parse(bad, "t.toml").unwrap_err().join("\n");
        for want in ["lowercase ASCII name", "`publisher` is required", "`kind` must be one of", "`reuse` must be", "public-sector suffix", "needs a known licence", "is listed twice"] {
            assert!(errs.contains(want), "{want}\n{errs}");
        }
        assert!(parse("[[source]]\nhost = \"a.org\"\nsurprise = 1\n", "t").is_err(), "an unknown key in an entry is refused");
        assert!(parse("not toml [", "t").is_err());
        assert!(parse("surprise = 1\n", "t").is_err(), "an unknown top-level key is refused");
        assert_eq!(parse("", "t").unwrap(), vec![], "an empty list is fine");
    }

    #[test]
    fn a_url_is_matched_by_its_https_host_and_nothing_it_cannot_classify_matches() {
        assert_eq!(https_host("https://En.Wikipedia.org/wiki/X?a=b#c"), Some(("en.wikipedia.org".into(), "/wiki/X?a=b#c".into())));
        assert_eq!(https_host("https://example.org:443/x").map(|h| h.0), Some("example.org".into()));
        assert_eq!(https_host("https://example.org./x").map(|h| h.0), Some("example.org".into()), "a trailing dot is the same host");
        for bad in ["http://example.org/", "ftp://example.org/", "https://example.org:8443/", "https://user@example.org/", "https://user:pw@example.org/", "https://[::1]/", "https://127.0.0.1/", "https://10.0.0.1:443/", "https://localhost/", "https://exämple.org/", "https://xn--e1afmkfd.org\\@evil.org/", "https:///x", "https://exam ple.org/", "https://example..org/", "https://-bad.org/", "example.org"] {
            assert!(https_host(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_allow_list_is_a_union_of_two_lists_and_the_operator_can_add_and_block() {
        let default = vec![e("en.wikipedia.org", "full", "CC-BY-SA-4.0"), e("*.gov", "full", "CC0-1.0"), e("arxiv.org", "summary-only", "unknown")];
        let operator = vec![e("example.edu", "summary-only", "unknown"), Entry { block: true, ..e("arxiv.org", "summary-only", "unknown") }, Entry { block: true, ..e("hhs.gov", "full", "CC0-1.0") }];
        let a = Allow::union(&default, &operator);
        assert!(a.lookup("https://en.wikipedia.org/wiki/A").is_some());
        assert!(a.lookup("https://example.edu/x").is_some(), "the operator added it");
        assert!(a.lookup("https://arxiv.org/abs/1").is_none(), "the operator blocked a default host");
        assert!(a.lookup("https://nasa.gov/x").is_some());
        assert!(a.lookup("https://hhs.gov/x").is_none(), "a block beats a wider wildcard");
        assert!(a.lookup("https://deep.space.nasa.gov/x").is_some(), "a wildcard matches any depth under the suffix");
        assert!(a.lookup("https://gov/x").is_none(), "the bare suffix is not a host under it");
        assert!(a.lookup("https://evilgov.com/x").is_none() && a.lookup("https://nasa.gov.evil.com/x").is_none(), "suffix means a dot boundary at the end");
        assert!(a.lookup("http://en.wikipedia.org/wiki/A").is_none(), "https only");
        assert!(a.lookup("https://not-wikipedia.org/").is_none());
        let both = Allow::union(&[e("a.org", "summary-only", "unknown")], &[e("a.org", "full", "CC-BY-4.0")]);
        assert_eq!(both.lookup("https://a.org/").unwrap().licence, "CC-BY-4.0", "an operator entry for the same host replaces the default one");
        assert_eq!(a.exact_hosts().len(), 2);
    }

    #[test]
    fn an_exact_entry_wins_over_a_wildcard() {
        let a = Allow::union(&[e("*.gov", "summary-only", "unknown"), e("data.gov", "full", "CC0-1.0")], &[]);
        assert_eq!(a.lookup("https://data.gov/x").unwrap().reuse, "full");
        assert_eq!(a.lookup("https://other.gov/x").unwrap().reuse, "summary-only");
    }

    #[test]
    fn full_text_needs_a_full_source_a_known_licence_and_no_stricter_notice() {
        let full = e("a.org", "full", "CC-BY-4.0");
        assert_eq!(form(&full, "A plain page."), Form::Full);
        assert_eq!(form(&full, "Copyright 2026. All Rights Reserved."), Form::Summary, "a stricter notice on the page withholds the body");
        assert_eq!(form(&full, "Licensed with NoDerivs terms"), Form::Summary);
        assert_eq!(form(&e("a.org", "summary-only", "CC-BY-4.0"), "plain"), Form::Summary);
        // A hand-built entry that says full with an unknown licence still yields a summary (parse refuses it, this is the second lock).
        assert_eq!(form(&e("a.org", "full", "unknown"), "plain"), Form::Summary);
        assert!(share_alike("CC-BY-SA-4.0") && share_alike("ODbL-1.0") && !share_alike("CC-BY-4.0") && !share_alike("CC0-1.0"));
    }

    #[test]
    fn the_shipped_default_list_is_valid_and_says_what_the_spec_says() {
        let list = parse(include_str!("../../../context/sources/trusted.toml"), "trusted.toml").unwrap_or_else(|e| panic!("{}", e.join("\n")));
        assert!(list.len() >= 25);
        let a = Allow::union(&list, &[]);
        let get = |u: &str| a.lookup(u).map(|e| (e.reuse.as_str(), e.licence.as_str()));
        assert_eq!(get("https://en.wikipedia.org/wiki/BFT"), Some(("full", "CC-BY-SA-4.0")), "Wikipedia: full text, share-alike");
        assert_eq!(get("https://nist.gov/x"), Some(("full", "LicenseRef-PD-US-Gov")), "an exact federal host beats the wildcard");
        assert_eq!(get("https://somestate.gov/x"), Some(("summary-only", "unknown")), "any other .gov is a summary");
        assert_eq!(get("https://arxiv.org/abs/1"), Some(("summary-only", "unknown")));
        assert_eq!(get("https://legislation.gov.uk/x"), Some(("full", "OGL-UK-3.0")));
        for off in ["https://example.com/", "https://medium.com/x", "https://en.wikipedia.org.evil.com/", "http://en.wikipedia.org/wiki/BFT"] {
            assert!(a.lookup(off).is_none(), "{off}");
        }
        assert!(share_alike("CC-BY-SA-4.0"));
    }

    #[test]
    fn both_lists_load_fresh_from_disk_and_a_bad_one_names_its_file() {
        let (ws, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        assert!(Allow::load(ws.path(), home.path()).unwrap().entries().is_empty(), "no files: an empty list");
        std::fs::create_dir_all(ws.path().join("base/context/sources")).unwrap();
        std::fs::write(ws.path().join("base/context/sources/trusted.toml"), "[[source]]\nhost = \"ietf.org\"\npublisher = \"IETF\"\nkind = \"standards\"\n").unwrap();
        std::fs::create_dir_all(home.path().join("operator")).unwrap();
        std::fs::write(home.path().join("operator/trusted-sources.toml"), "[[source]]\nhost = \"example.edu\"\npublisher = \"E\"\nkind = \"journal\"\n").unwrap();
        let a = Allow::load(ws.path(), home.path()).unwrap();
        assert!(a.lookup("https://ietf.org/rfc/rfc1.txt").is_some() && a.lookup("https://example.edu/").is_some());
        std::fs::write(home.path().join("operator/trusted-sources.toml"), "[[source]]\nhost = \"BAD HOST\"\n").unwrap();
        let err = Allow::load(ws.path(), home.path()).unwrap_err().join("\n");
        assert!(err.contains("trusted-sources.toml"), "{err}");
    }
}
