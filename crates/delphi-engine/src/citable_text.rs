//! Citable text (`specs/144` §2.1 item 1; slice S0): whether a source's stored text holds content a page may cite.
//!
//! A fixed rule in code, no model. For a web capture the HTTP status was a success. The text is not empty. It is not only
//! navigation, a "not found" page (a soft 404), a parked domain, a CAPTCHA, a paywall notice or a cookie banner. And its **content
//! share** passes [`CONTENT_SHARE_MIN`]: the characters of its content lines (ordinary sentences and table rows) against the characters
//! of all its lines, where links, menus, headings, repeated lines and the boilerplate of the kinds above count as no content. A local
//! file or a table of figures is judged by the same share, without the HTTP test. A summary is never citable text: callers never pass
//! one here.

/// The least share of a text's characters that must be content (sentences of ordinary text and table rows).
pub const CONTENT_SHARE_MIN: f64 = 0.35;
/// The fewest words a line or sentence needs to count as ordinary text rather than a menu item or a label.
pub const SENTENCE_WORDS_MIN: usize = 4;

/// Why a source's text is not citable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    HttpStatus(u16),
    Empty,
    SoftNotFound,
    ParkedDomain,
    Captcha,
    Paywall,
    CookieBanner,
    Navigation,
    /// Some content, but its share is under [`CONTENT_SHARE_MIN`] (in whole percent).
    LowContent(u32),
}

impl std::fmt::Display for Exclusion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Exclusion::HttpStatus(s) => write!(f, "the capture's HTTP status was {s}, not a success"),
            Exclusion::Empty => write!(f, "the stored text is empty"),
            Exclusion::SoftNotFound => write!(f, "the text is only a \"not found\" page (a soft 404)"),
            Exclusion::ParkedDomain => write!(f, "the text is only a parked domain's page"),
            Exclusion::Captcha => write!(f, "the text is only a CAPTCHA or a bot check"),
            Exclusion::Paywall => write!(f, "the text is only a paywall notice"),
            Exclusion::CookieBanner => write!(f, "the text is only a cookie banner"),
            Exclusion::Navigation => write!(f, "the text is only navigation (links, menus and repeated lines)"),
            Exclusion::LowContent(p) => write!(f, "only {p}% of the text is content, under the {}% the rule asks", (CONTENT_SHARE_MIN * 100.0) as u32),
        }
    }
}

/// The boilerplate kinds, each with the phrases that mark a sentence as that kind (lowercase).
const BOILERPLATE: [(Exclusion, &[&str]); 5] = [
    (Exclusion::SoftNotFound, &["page not found", "error 404", "404 error", "404 not found", "could not be found", "cannot be found", "can't be found", "does not exist", "doesn't exist", "no longer available", "the page you requested", "the page you are looking for", "the page you were looking for", "nothing was found"]),
    (Exclusion::ParkedDomain, &["domain is for sale", "domain may be for sale", "buy this domain", "this domain is parked", "parked free", "domain parking", "inquire about this domain", "make an offer on this domain"]),
    (Exclusion::Captcha, &["captcha", "verify you are human", "verify that you are human", "are you a robot", "not a robot", "checking your browser", "checking if the site connection is secure", "enable javascript and cookies to continue", "press and hold", "unusual traffic"]),
    (Exclusion::Paywall, &["subscribe to continue", "subscribe to read", "to continue reading", "subscribers only", "for subscribers", "already a subscriber", "sign in to read", "log in to continue", "create a free account to continue", "this content is for members"]),
    (Exclusion::CookieBanner, &["we use cookies", "this site uses cookies", "this website uses cookies", "accept all cookies", "accept cookies", "cookie settings", "cookie policy", "manage cookies", "your privacy choices", "reject all"]),
];

fn boilerplate_kind(sentence: &str) -> Option<Exclusion> {
    let low = sentence.to_lowercase();
    BOILERPLATE.iter().find(|(_, phrases)| phrases.iter().any(|p| low.contains(p))).map(|(k, _)| *k)
}

fn separator_row(l: &str) -> bool {
    l.starts_with('|') && l.contains('-') && l.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// A table row with at least two cells that are not empty.
fn table_row(l: &str) -> bool {
    l.starts_with('|') && !separator_row(l) && l.trim_matches('|').split('|').filter(|c| !c.trim().is_empty()).count() >= 2
}

/// The line with its Markdown links and images reduced to their words, and how much of it was link.
fn unlinked(l: &str) -> (String, usize) {
    let mut out = String::new();
    let mut linked = 0usize;
    let mut rest = l;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|c| open + c) else { break };
        let Some(end) = rest[close..].find(')').map(|e| close + e) else { break };
        out.push_str(rest[..open].trim_end_matches('!'));
        let words = &rest[open + 1..close];
        linked += words.len();
        out.push_str(words);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    let bare: Vec<&str> = out.split_whitespace().filter(|w| w.starts_with("http://") || w.starts_with("https://") || w.starts_with("www.")).collect();
    linked += bare.iter().map(|w| w.len()).sum::<usize>();
    let out = out.split_whitespace().filter(|w| !(w.starts_with("http://") || w.starts_with("https://") || w.starts_with("www."))).collect::<Vec<_>>().join(" ");
    (out, linked)
}

/// The sentences of a line of ordinary text: cut after `.`, `!` or `?` followed by a space.
fn sentences(l: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let b = l.as_bytes();
    for i in 0..b.len() {
        if matches!(b[i], b'.' | b'!' | b'?') && b.get(i + 1).is_some_and(|c| *c == b' ') {
            out.push(l[start..=i].trim());
            start = i + 1;
        }
    }
    if start < l.len() {
        out.push(l[start..].trim());
    }
    out.retain(|s| !s.is_empty());
    out
}

/// A line made of Markdown images and nothing else.
fn only_images(l: &str) -> bool {
    let mut rest = l.trim();
    if !rest.starts_with("![") {
        return false;
    }
    while let Some(r) = rest.strip_prefix("![") {
        let Some(end) = r.find("](").and_then(|c| r[c..].find(')').map(|e| c + e)) else { return false };
        rest = r[end + 1..].trim_start();
    }
    rest.is_empty()
}

fn words(s: &str) -> usize {
    s.split_whitespace().filter(|w| w.chars().any(char::is_alphabetic)).count()
}

/// The rule of `specs/144` §2.1 item 1. `http_status` is the status of a web capture (`None` for a local file, a table or text a
/// host handed over with no fetch). `Ok` when the text is citable.
pub fn citable_text(text: &str, http_status: Option<u16>) -> Result<(), Exclusion> {
    if let Some(s) = http_status.filter(|s| !(200..300).contains(s)) {
        return Err(Exclusion::HttpStatus(s));
    }
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() || lines.iter().all(|l| l.chars().all(|c| !c.is_alphanumeric())) {
        return Err(Exclusion::Empty);
    }
    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for l in &lines {
        *seen.entry(l).or_default() += 1;
    }
    let (mut total, mut content) = (0usize, 0usize);
    let mut boiler: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    let mut kinds: Vec<Exclusion> = Vec::new();
    let mut counted: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for l in &lines {
        // A line that only places pictures is not text: it counts neither way.
        if only_images(l) {
            continue;
        }
        let size = l.chars().count();
        total += size;
        // A line that repeats (a menu on every page of a site, a footer) counts once at most, and then only as itself.
        if seen[l] > 1 && !counted.insert(l) {
            continue;
        }
        if separator_row(l) {
            continue;
        }
        if table_row(l) {
            if let Some(k) = boilerplate_kind(l) {
                *boiler.entry(k.discriminant()).or_default() += size;
                kinds.push(k);
            } else {
                content += size;
            }
            continue;
        }
        if l.starts_with('#') {
            continue;
        }
        let (plain, linked) = unlinked(l.trim_start_matches(['-', '*', '>', ' ']));
        // A line that is mostly link is navigation, whatever its words.
        if linked * 2 >= l.len() {
            continue;
        }
        for s in sentences(&plain) {
            if let Some(k) = boilerplate_kind(s) {
                *boiler.entry(k.discriminant()).or_default() += s.chars().count();
                kinds.push(k);
            } else if words(s) >= SENTENCE_WORDS_MIN {
                content += s.chars().count();
            }
        }
    }
    let share = content as f64 / total.max(1) as f64;
    if share >= CONTENT_SHARE_MIN {
        return Ok(());
    }
    // Not enough content: say what the text is, the boilerplate kind that fills most of it if any.
    if let Some(k) = BOILERPLATE.iter().map(|(k, _)| *k).filter(|k| kinds.contains(k)).max_by_key(|k| boiler.get(&k.discriminant()).copied().unwrap_or(0)) {
        return Err(k);
    }
    if content == 0 {
        return Err(Exclusion::Navigation);
    }
    Err(Exclusion::LowContent((share * 100.0) as u32))
}

impl Exclusion {
    fn discriminant(&self) -> u8 {
        match self {
            Exclusion::HttpStatus(_) => 0,
            Exclusion::Empty => 1,
            Exclusion::SoftNotFound => 2,
            Exclusion::ParkedDomain => 3,
            Exclusion::Captcha => 4,
            Exclusion::Paywall => 5,
            Exclusion::CookieBanner => 6,
            Exclusion::Navigation => 7,
            Exclusion::LowContent(_) => 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = "# The estuary tide\n\n[Home](/) [News](/news) [About](/about)\n\nBy a staff writer\n\nThe tide in the estuary turns later than on the open coast. Engineers who measured it at the harbour wall found a lag of about an hour.\n\nThe lag grows in spring, when the river runs high and the channel holds more water.\n\n[Home](/) [News](/news) [About](/about)\n\n© 2026 The Coast Paper\n";
    const DOCS: &str = "# Configuration\n\n- [Install](/install)\n- [Configure](/configure)\n- [Reference](/ref)\n\n## The `timeout` key\n\nThe `timeout` key sets how long a request may wait before it is cancelled. It takes a number of seconds and defaults to thirty.\n\n| Key | Type | Default |\n| :-- | :-- | :-- |\n| timeout | integer | 30 |\n| retries | integer | 3 |\n\nA value of zero turns the limit off.\n";
    const PAPER: &str = "Abstract\n\nWe measure tidal lag in twelve estuaries and find that channel depth explains most of the variance. Shallow channels delay high water by up to ninety minutes.\n\n1 Introduction\n\nTidal lag has been studied since the nineteenth century. Earlier work relied on single gauges; we combine gauge records with bathymetry.\n\nReferences\n\n[1] A. Author, Tides, 1901.\n";
    const SOFT_404: &str = "# Page not found\n\n[Home](/) [Search](/search)\n\nSorry, the page you are looking for could not be found. It may have been moved or deleted.\n\n[Home](/) [Search](/search)\n";
    const PARKED: &str = "example-tides.com\n\nThis domain is for sale! Make an offer on this domain today.\n\nRelated searches: tides, estuary, harbour\n\nPrivacy Policy\n";
    const CAPTCHA: &str = "Just a moment...\n\nChecking your browser before accessing the site.\n\nPlease verify you are human by completing the action below.\n\nRay ID: 8f2c\n";

    #[test]
    fn the_accept_fixtures_are_citable() {
        for (name, t) in [("article", ARTICLE), ("docs page", DOCS), ("paper", PAPER)] {
            assert_eq!(citable_text(t, Some(200)), Ok(()), "{name}");
            assert_eq!(citable_text(t, None), Ok(()), "{name} as a local file");
        }
        // A table of figures, judged by its rows.
        assert_eq!(citable_text("| Year | Lag (min) |\n| :-- | --: |\n| 2020 | 61 |\n| 2021 | 64 |\n", None), Ok(()));
        // A one-sentence text of ordinary prose.
        assert_eq!(citable_text("The tide turns twice a day.", None), Ok(()));
    }

    #[test]
    fn the_reject_fixtures_are_not_citable_and_say_why() {
        assert_eq!(citable_text(SOFT_404, Some(200)), Err(Exclusion::SoftNotFound));
        assert_eq!(citable_text(PARKED, Some(200)), Err(Exclusion::ParkedDomain));
        assert_eq!(citable_text(CAPTCHA, Some(200)), Err(Exclusion::Captcha));
        assert_eq!(citable_text("We use cookies to improve your experience. Accept all cookies or manage cookies.", None), Err(Exclusion::CookieBanner));
        assert_eq!(citable_text("Subscribe to continue reading. Already a subscriber? Sign in to read.", Some(200)), Err(Exclusion::Paywall));
        assert_eq!(citable_text("", None), Err(Exclusion::Empty));
        assert_eq!(citable_text("  \n\n---\n", None), Err(Exclusion::Empty));
        assert_eq!(citable_text("[Home](/)\n[News](/news)\n[About](/about)\nMenu\nLogin\n", None), Err(Exclusion::Navigation));
        assert_eq!(citable_text(ARTICLE, Some(404)), Err(Exclusion::HttpStatus(404)));
        assert_eq!(citable_text(ARTICLE, Some(503)), Err(Exclusion::HttpStatus(503)));
        assert!(Exclusion::SoftNotFound.to_string().contains("soft 404"));
    }

    /// A page that mentions cookies or a 404 in passing is still an article: the share decides, not a word.
    #[test]
    fn a_boilerplate_word_in_an_article_does_not_reject_it() {
        let t = format!("{ARTICLE}\nWe use cookies.\n");
        assert_eq!(citable_text(&t, Some(200)), Ok(()));
    }
}
