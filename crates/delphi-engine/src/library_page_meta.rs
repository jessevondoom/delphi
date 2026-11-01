//! Encyclopedia page metadata (`specs/061` §4.4): the lifetime the `writer` proposes, the daemon's clamp, and the YAML
//! frontmatter a cached page starts with.
//!
//! The writer ends its page with one comment line, `<!-- page: kind=article; volatility=low; days=90; reason=... -->`.
//! The daemon reads it, removes it from the body (it is never part of what the checks see), clamps `days` to
//! `[library.pages] min_ttl_days..=max_ttl_days`, and writes `valid_until` into the frontmatter and the sidecar. A page
//! with no such line is `medium`, 30 days. The frontmatter is for people and the knowledge index; every reader of a
//! page's text strips it (`strip`), so the banner is still the first line the check sees.

use serde_json::Value;

const MARK: &str = "<!-- page:";
const REASON_MAX: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Volatility {
    High,
    Medium,
    Low,
    Timeless,
}

impl Volatility {
    pub fn as_str(self) -> &'static str {
        match self {
            Volatility::High => "high",
            Volatility::Medium => "medium",
            Volatility::Low => "low",
            Volatility::Timeless => "timeless",
        }
    }

    pub fn parse(s: &str) -> Option<Volatility> {
        match s.trim().to_lowercase().as_str() {
            "high" => Some(Volatility::High),
            "medium" => Some(Volatility::Medium),
            "low" => Some(Volatility::Low),
            "timeless" => Some(Volatility::Timeless),
            _ => None,
        }
    }

    /// The lifetime used when the writer names a volatility but no number of days.
    pub fn default_days(self) -> u32 {
        match self {
            Volatility::High => 3,
            Volatility::Medium => 30,
            Volatility::Low => 90,
            Volatility::Timeless => 365,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Article,
    Disambiguation,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Article => "article",
            Kind::Disambiguation => "disambiguation",
        }
    }
}

/// How deep a page goes: the quick search result, or the one rebuilt with outside research (`specs/061` §4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Depth {
    #[default]
    Summary,
    Expanded,
}

impl Depth {
    pub fn as_str(self) -> &'static str {
        match self {
            Depth::Summary => "summary",
            Depth::Expanded => "expanded",
        }
    }
}

/// What the writer asked for, before the clamp.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proposal {
    pub kind: Option<Kind>,
    pub volatility: Option<Volatility>,
    pub days: Option<u64>,
    pub reason: Option<String>,
}

/// What the daemon keeps: the proposal after the clamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMeta {
    pub kind: Kind,
    pub volatility: Volatility,
    pub days: u32,
    pub reason: String,
}

impl Default for PageMeta {
    fn default() -> Self {
        PageMeta { kind: Kind::Article, volatility: Volatility::Medium, days: Volatility::Medium.default_days(), reason: "no lifetime was proposed; the default for a changing topic".into() }
    }
}

fn one_line(s: &str, max: usize) -> String {
    let t: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > max { format!("{}…", t.chars().take(max - 1).collect::<String>()) } else { t }
}

fn is_directive(line: &str) -> bool {
    let t = line.trim();
    t.starts_with(MARK) && t.ends_with("-->")
}

/// The body with every directive line removed, and the last directive's proposal.
pub fn extract(draft: &str) -> (String, Option<Proposal>) {
    let mut proposal = None;
    let mut kept: Vec<&str> = Vec::new();
    for line in draft.lines() {
        if is_directive(line) {
            let t = line.trim();
            let inner = &t[MARK.len()..t.len() - 3];
            let mut p = Proposal::default();
            for part in inner.split(';') {
                let Some((k, v)) = part.split_once('=') else { continue };
                let v = v.trim();
                match k.trim() {
                    "kind" => p.kind = match v { "article" => Some(Kind::Article), "disambiguation" => Some(Kind::Disambiguation), _ => None },
                    "volatility" => p.volatility = Volatility::parse(v),
                    "days" => p.days = v.parse().ok(),
                    "reason" if !v.is_empty() => p.reason = Some(one_line(v, REASON_MAX)),
                    _ => {}
                }
            }
            proposal = Some(p);
        } else {
            kept.push(line);
        }
    }
    (kept.join("\n").trim().to_string(), proposal)
}

/// The proposal after the clamp to the configured bounds. A missing proposal is the default; a missing number of days is
/// the volatility's own.
pub fn resolve(p: Option<&Proposal>, min_days: u32, max_days: u32) -> PageMeta {
    let (lo, hi) = (min_days.min(max_days).max(1), min_days.max(max_days).max(1));
    let Some(p) = p else { return PageMeta { days: PageMeta::default().days.clamp(lo, hi), ..PageMeta::default() } };
    let volatility = p.volatility.unwrap_or(Volatility::Medium);
    let want = p.days.map(|d| d.min(u32::MAX as u64) as u32).unwrap_or_else(|| volatility.default_days());
    PageMeta { kind: p.kind.unwrap_or(Kind::Article), volatility, days: want.clamp(lo, hi), reason: p.reason.clone().unwrap_or_else(|| "the writer gave no reason".into()) }
}

/// `YYYY-MM-DD` for a Unix time (UTC).
pub fn ymd(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

/// The `[label](page:slug)` links of a page body: its disambiguation targets.
pub fn page_links(body: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while let Some(p) = body[i..].find("](page:") {
        let at = i + p;
        let after = at + 2;
        let Some(close) = body[after..].find(')') else { break };
        let target = body[after..after + close].trim().to_string();
        let label = body[..at].rfind('[').map(|o| one_line(&body[o + 1..at], 80)).unwrap_or_default();
        if !out.iter().any(|(_, t)| *t == target) {
            out.push((label, target));
        }
        i = after + close + 1;
    }
    out
}

fn quoted(s: &str) -> String {
    serde_json::to_string(&one_line(s, 400)).unwrap_or_else(|_| "\"\"".into())
}

/// What the frontmatter says about a page.
pub struct Front<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub depth: Depth,
    pub meta: &'a PageMeta,
    /// `active`, or `unverified` for the claims-only fallback.
    pub status: &'a str,
    pub created: u64,
    pub confirmed: u64,
    pub valid_until: u64,
    pub links: &'a [(String, String)],
}

pub fn frontmatter(f: &Front<'_>) -> String {
    let mut s = String::from("---\n");
    s.push_str(&format!("id: page-{}\ntitle: {}\nquery_key: {}\nkind: {}\ndepth: {}\nauthor: writer\nsource_type: internal_generated\nstatus: {}\n", f.key, quoted(f.title), quoted(f.key), f.meta.kind.as_str(), f.depth.as_str(), f.status));
    s.push_str(&format!("created: {}\nlast_confirmed: {}\nvalid_until: {}\nvolatility: {}\nttl_reason: {}\n", ymd(f.created), ymd(f.confirmed), ymd(f.valid_until), f.meta.volatility.as_str(), quoted(&f.meta.reason)));
    if f.links.is_empty() {
        s.push_str("disambiguation: []\n");
    } else {
        s.push_str("disambiguation:\n");
        for (label, target) in f.links.iter().take(12) {
            s.push_str(&format!("  - label: {}\n    target: {}\n", quoted(label), quoted(target)));
        }
    }
    s.push_str("---\n\n");
    s
}

/// The frontmatter block of a page file, if it has one, and the rest.
pub fn split(md: &str) -> (Option<&str>, &str) {
    let Some(rest) = md.strip_prefix("---\n") else { return (None, md) };
    match rest.find("\n---\n") {
        Some(i) => (Some(&rest[..i]), rest[i + 5..].trim_start_matches('\n')),
        None => (None, md),
    }
}

/// The page text without its frontmatter: what the check, the banner test and every reader see.
pub fn strip(md: &str) -> &str {
    split(md).1
}

/// A frontmatter field's raw value.
pub fn field<'a>(front: &'a str, name: &str) -> Option<&'a str> {
    front.lines().find_map(|l| l.strip_prefix(name).and_then(|r| r.strip_prefix(':')).map(|v| v.trim()))
}

/// The frontmatter as JSON for the console: `valid_until`, `volatility` and the rest.
pub fn front_json(front: &str) -> Value {
    let mut m = serde_json::Map::new();
    for k in ["kind", "depth", "status", "created", "last_confirmed", "valid_until", "volatility", "ttl_reason", "title"] {
        if let Some(v) = field(front, k) {
            let v = v.trim_matches('"');
            m.insert(k.into(), Value::String(v.into()));
        }
    }
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directive_is_read_and_removed() {
        let (body, p) = extract("# T\n\nText [s-a].\n\n<!-- page: kind=article; volatility=low; days=90; reason=Stable spec -->\n");
        assert_eq!(body, "# T\n\nText [s-a].");
        let p = p.unwrap();
        assert_eq!((p.kind, p.volatility, p.days, p.reason.as_deref()), (Some(Kind::Article), Some(Volatility::Low), Some(90), Some("Stable spec")));
    }

    #[test]
    fn a_missing_or_wild_proposal_is_clamped() {
        assert_eq!(resolve(None, 1, 365).days, 30);
        assert_eq!(resolve(None, 1, 365).volatility, Volatility::Medium);
        let long = Proposal { volatility: Some(Volatility::Timeless), days: Some(9999), ..Default::default() };
        assert_eq!(resolve(Some(&long), 1, 365).days, 365);
        let zero = Proposal { volatility: Some(Volatility::High), days: Some(0), ..Default::default() };
        assert_eq!(resolve(Some(&zero), 1, 365).days, 1);
        let unnamed = Proposal { volatility: Some(Volatility::Low), ..Default::default() };
        assert_eq!(resolve(Some(&unnamed), 1, 365).days, 90);
        // the operator's bounds win, and swapped bounds still make sense
        assert_eq!(resolve(Some(&long), 7, 30).days, 30);
        assert_eq!(resolve(Some(&zero), 30, 7).days, 7);
    }

    #[test]
    fn dates_are_utc_calendar_days() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(951_782_400), "2000-02-29");
        assert_eq!(ymd(1_793_404_800), "2026-10-31");
    }

    #[test]
    fn frontmatter_round_trips_and_strips() {
        let meta = PageMeta { kind: Kind::Article, volatility: Volatility::Low, days: 90, reason: "has \"quotes\": and a colon".into() };
        let links = page_links("> *See [Other: thing](page:other-thing).*\n\n# T");
        assert_eq!(links, vec![("Other: thing".to_string(), "page:other-thing".to_string())]);
        let f = frontmatter(&Front { key: "abc", title: "A: title", depth: Depth::Summary, meta: &meta, status: "active", created: 1_793_404_800, confirmed: 1_793_404_800, valid_until: 1_793_404_800 + 90 * 86_400, links: &links });
        let page = format!("{}Body line\n", f);
        let (front, body) = split(&page);
        assert_eq!(body, "Body line\n");
        let v: serde_yaml::Value = serde_yaml::from_str(front.unwrap()).expect("the frontmatter is YAML");
        assert_eq!(v["valid_until"].as_str(), Some("2027-01-29"));
        assert_eq!(v["author"].as_str(), Some("writer"));
        assert_eq!(v["ttl_reason"].as_str(), Some("has \"quotes\": and a colon"));
        assert_eq!(v["disambiguation"][0]["target"].as_str(), Some("page:other-thing"));
        assert_eq!(front_json(front.unwrap())["volatility"], "low");
        assert_eq!(strip("no front matter"), "no front matter");
    }
}
