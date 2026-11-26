//! Validation and normalisation of a source handed to the library (`specs/142` §2.2 item 9), by code and before anything is stored.
//!
//! One function per field, and [`item`] over a whole item of `capture_source`. The same functions serve the host's tool, its dry run,
//! `delphi sources capture` and `delphi sources lint`, so what a dry run shows is what a call stores. Each problem found is a
//! [`Finding`] `{field, problem, fix}` that is either an `error` (the item is refused) or `normalised` (the item is stored as
//! corrected, and the correction is shown). Nothing here reads a file, the network or the clock but [`date`]'s default.
//!
//! What a host says is data: no field here sets trust, status, privacy or a licence the daemon keeps by (`specs/142` §2.2 item 8).

use serde_json::{json, Map, Value};

/// The most text one item may hand over: 256 KiB.
pub const TEXT_MAX: usize = 256 * 1024;
/// A title's length after cleaning.
pub const TITLE_MAX: usize = 300;
/// A note's length.
pub const NOTE_MAX: usize = 2000;

/// The fields one item of `capture_source` may carry (`specs/142` §2.2 item 1). Anything else is refused by name.
pub const ITEM_FIELDS: [&str; 14] = ["locator", "text", "file", "title", "retrieved", "kind", "media_type", "csl", "licence", "labels", "topics", "topic_id", "research_id", "note"];

/// The citation fields `csl` may carry: the library's own list (`library_export.rs`), less what is not a citation field.
pub const CSL_FIELDS: [&str; 18] = ["type", "title", "author", "editor", "translator", "container-title", "publisher", "publisher-place", "issued", "volume", "issue", "page", "edition", "URL", "DOI", "ISBN", "ISSN", "language"];

/// The closed list of citation types (CSL 1.0.2).
pub const CSL_TYPES: [&str; 44] = [
    "article", "article-journal", "article-magazine", "article-newspaper", "bill", "book", "broadcast", "chapter", "classic", "collection", "dataset", "document", "entry", "entry-dictionary", "entry-encyclopedia", "event", "figure", "graphic", "hearing", "interview", "legal_case", "legislation", "manuscript", "map", "motion_picture", "musical_score", "pamphlet", "paper-conference", "patent", "performance", "periodical", "personal_communication", "post", "post-weblog", "regulation", "report", "review", "review-book", "software", "song", "speech", "standard", "thesis", "webpage",
];

/// The media types the library makes text from. Anything else is handed over as text.
pub const MEDIA_TYPES: [&str; 3] = ["text/plain", "text/markdown", "text/html"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Error,
    Normalised,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub field: String,
    pub problem: String,
    pub fix: String,
    pub kind: Kind,
}

impl Finding {
    fn error(field: &str, problem: impl Into<String>, fix: impl Into<String>) -> Finding {
        Finding { field: field.into(), problem: problem.into(), fix: fix.into(), kind: Kind::Error }
    }

    fn normalised(field: &str, problem: impl Into<String>, fix: impl Into<String>) -> Finding {
        Finding { field: field.into(), problem: problem.into(), fix: fix.into(), kind: Kind::Normalised }
    }

    pub fn to_json(&self) -> Value {
        json!({"field": self.field, "problem": self.problem, "fix": self.fix, "kind": if self.kind == Kind::Error { "error" } else { "normalised" }})
    }
}

/// Any finding that refuses the item?
pub fn refused(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.kind == Kind::Error)
}

/// A locator made canonical: a web address loses its fragment and is spelled as the library spells it (so its source id is the one a
/// daemon capture of the same address gets); anything else is a one-line description. An address with credentials in it is refused,
/// in any kind. Returns the locator and whether it is a web address.
pub fn locator(raw: &str, findings: &mut Vec<Finding>) -> Option<(String, bool)> {
    let one = one_line(raw);
    if one.is_empty() {
        findings.push(Finding::error("locator", "missing", "give the page's address, or a description of where the source is"));
        return None;
    }
    if one.chars().count() > 500 {
        findings.push(Finding::error("locator", "longer than 500 characters", "shorten it"));
        return None;
    }
    let lower = one.to_ascii_lowercase();
    let looks_like_url = lower.contains("://");
    if !looks_like_url {
        return Some((one, false));
    }
    let Ok(mut u) = crate::fact_fetch::Address::parse(&one) else {
        findings.push(Finding::error("locator", "an address that does not parse", "give a whole address (https://...) or a description"));
        return None;
    };
    if !u.username().is_empty() || u.password().is_some() {
        findings.push(Finding::error("locator", "the address carries credentials", "remove the user name and password from the address"));
        return None;
    }
    if !matches!(u.scheme(), "http" | "https") {
        findings.push(Finding::error("locator", format!("a `{}` address", u.scheme()), "give an http(s) address, or a description; hand a file over with `file`"));
        return None;
    }
    u.set_fragment(None);
    let canonical = u.to_string();
    if canonical != one {
        findings.push(Finding::normalised("locator", "not in the library's canonical form", format!("stored as {canonical}")));
    }
    Some((canonical, true))
}

/// A date as RFC 3339. A bare date (`2026-11-25`) is accepted as midnight UTC and said so; empty is `None` (the caller uses now).
pub fn date(field: &str, raw: &str, findings: &mut Vec<Finding>) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if delphi_core::time::parse_rfc3339(t).is_some() {
        return Some(t.to_string());
    }
    if bare_date(t).is_some() {
        let full = format!("{t}T00:00:00Z");
        findings.push(Finding::normalised(field, "a date without a time", format!("stored as {full}")));
        return Some(full);
    }
    findings.push(Finding::error(field, format!("`{}` is not a date", clip(t, 40)), "give it as 2026-11-25 or 2026-11-25T10:00:00Z"));
    None
}

fn bare_date(t: &str) -> Option<(i64, i64, i64)> {
    let b = t.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, m, d) = (t[..4].parse::<i64>().ok()?, t[5..7].parse::<i64>().ok()?, t[8..].parse::<i64>().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some((y, m, d))
}

/// A title: no control characters, one line, no surrounding space, at most [`TITLE_MAX`] characters.
pub fn title(raw: &str, findings: &mut Vec<Finding>) -> String {
    let t: String = one_line(raw).chars().take(TITLE_MAX).collect();
    if !raw.is_empty() && t != raw {
        findings.push(Finding::normalised("title", "control characters, extra space or past 300 characters", "cleaned"));
    }
    t
}

/// The text a host handed over: in the library's stored form (no byte-order mark, `\n` line endings, NFC), with NUL and other
/// control characters but tab and newline removed. At most [`TEXT_MAX`] bytes as handed over.
pub fn text(field: &str, raw: &str, findings: &mut Vec<Finding>) -> Option<String> {
    if raw.len() > TEXT_MAX {
        findings.push(Finding::error(field, format!("{} bytes, over the limit of {TEXT_MAX} (256 KiB)", raw.len()), "hand over a shorter text, or save it to a file (at most 8 MiB)"));
        return None;
    }
    if raw.trim().is_empty() {
        findings.push(Finding::error(field, "empty", "hand over the text that was read"));
        return None;
    }
    let cleaned: String = raw.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t' | '\r')).collect();
    let stored = crate::library::capture::normalise(&cleaned);
    if stored != raw {
        findings.push(Finding::normalised(field, "a byte-order mark, other line endings, control characters or another Unicode form", "stored in one normal form (NFC, \\n)"));
    }
    Some(stored)
}

/// Bytes read from a file, as text: refused when they are not UTF-8.
pub fn utf8(field: &str, bytes: &[u8], findings: &mut Vec<Finding>) -> Option<String> {
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(s.to_string()),
        Err(e) => {
            findings.push(Finding::error(field, format!("not UTF-8 text (at byte {})", e.valid_up_to()), "hand over UTF-8 text; Delphi does not convert other encodings"));
            None
        }
    }
}

/// What the bytes look like: HTML when they open with markup, else plain text.
pub fn sniff(bytes: &[u8]) -> &'static str {
    let head: String = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).trim_start().to_ascii_lowercase();
    if head.starts_with("<!doctype html") || head.starts_with("<html") || head.starts_with("<head") || head.starts_with("<body") || (head.starts_with('<') && head.contains("<p")) {
        "text/html"
    } else {
        "text/plain"
    }
}

/// The media type: the declared one compared with what the bytes look like. One Delphi does not extract is refused; HTML declared
/// for bytes with no markup, or markup declared as plain text, is corrected and said so. `text/markdown` is kept for text that is
/// not HTML.
pub fn media_type(declared: Option<&str>, bytes: &[u8], findings: &mut Vec<Finding>) -> Option<String> {
    let seen = sniff(bytes);
    let declared = declared.map(|d| d.split(';').next().unwrap_or("").trim().to_ascii_lowercase()).filter(|d| !d.is_empty());
    match declared.as_deref() {
        None => Some(seen.to_string()),
        Some(d) if !MEDIA_TYPES.contains(&d) => {
            findings.push(Finding::error("media_type", format!("`{}` is not a type Delphi makes text from", clip(d, 60)), "hand over the text with `text`, or a text, Markdown or HTML file"));
            None
        }
        Some("text/markdown") if seen == "text/plain" => Some("text/markdown".into()),
        Some(d) if d == seen => Some(d.to_string()),
        Some(d) => {
            findings.push(Finding::normalised("media_type", format!("declared {d}, and the bytes look like {seen}"), format!("read as {seen}")));
            Some(seen.to_string())
        }
    }
}

/// Citation fields (`csl`): only the library's list, the type from the closed list, names as family and given, a DOI checked for
/// shape and lower-cased, an ISBN's check digit verified, `issued` as date parts.
pub fn csl(v: &Value, findings: &mut Vec<Finding>) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(m) = v.as_object() else {
        if !v.is_null() {
            findings.push(Finding::error("csl", "not an object", "give the citation fields as an object"));
        }
        return out;
    };
    for (k, val) in m {
        let field = format!("csl.{k}");
        if !CSL_FIELDS.contains(&k.as_str()) {
            findings.push(Finding::error(&field, "not a citation field Delphi keeps", format!("use one of {}", CSL_FIELDS.join(", "))));
            continue;
        }
        match k.as_str() {
            "type" => match val.as_str().map(str::trim) {
                Some(t) if CSL_TYPES.contains(&t) => {
                    out.insert(k.clone(), json!(t));
                }
                _ => findings.push(Finding::error(&field, "not a citation type", "use a CSL type such as article-journal, book, report or webpage")),
            },
            "author" | "editor" | "translator" => {
                if let Some(names) = names(&field, val, findings) {
                    out.insert(k.clone(), Value::Array(names));
                }
            }
            "DOI" => match val.as_str().and_then(|d| doi(&field, d, findings)) {
                Some(d) => {
                    out.insert(k.clone(), json!(d));
                }
                None if !val.is_string() => findings.push(Finding::error(&field, "not a string", "give the DOI as 10.xxxx/...")),
                None => {}
            },
            "ISBN" => {
                if let Some(i) = val.as_str().and_then(|s| isbn(&field, s, findings)) {
                    out.insert(k.clone(), json!(i));
                }
            }
            "issued" => {
                if let Some(d) = issued(&field, val, findings) {
                    out.insert(k.clone(), d);
                }
            }
            _ => match val {
                Value::String(s) => {
                    let t: String = one_line(s).chars().take(TITLE_MAX).collect();
                    if t != *s {
                        findings.push(Finding::normalised(&field, "control characters or extra space", "cleaned"));
                    }
                    if !t.is_empty() {
                        out.insert(k.clone(), json!(t));
                    }
                }
                Value::Number(n) => {
                    out.insert(k.clone(), json!(n.to_string()));
                }
                _ => findings.push(Finding::error(&field, "not text", "give it as a string")),
            },
        }
    }
    out
}

/// Names as CSL name objects. One string becomes family and given (`Doe, Jane` or `Jane Doe`); an object with `family` or `literal`
/// is kept with its strings cleaned.
fn names(field: &str, v: &Value, findings: &mut Vec<Finding>) -> Option<Vec<Value>> {
    let list: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        other => vec![other],
    };
    let mut out = Vec::new();
    for n in list {
        match n {
            Value::String(s) => {
                let s = one_line(s);
                if s.is_empty() {
                    continue;
                }
                let name = match s.split_once(',') {
                    Some((family, given)) => json!({"family": family.trim(), "given": given.trim()}),
                    None => match s.rsplit_once(' ') {
                        Some((given, family)) => json!({"family": family.trim(), "given": given.trim()}),
                        None => json!({"literal": s}),
                    },
                };
                findings.push(Finding::normalised(field, format!("the name `{}` given as one string", clip(&s, 60)), format!("stored as {name}")));
                out.push(name);
            }
            Value::Object(o) if o.contains_key("family") || o.contains_key("literal") => {
                let mut kept = Map::new();
                for k in ["family", "given", "literal", "suffix", "non-dropping-particle", "dropping-particle"] {
                    if let Some(t) = o.get(k).and_then(Value::as_str).map(one_line).filter(|t| !t.is_empty()) {
                        kept.insert(k.into(), json!(t));
                    }
                }
                out.push(Value::Object(kept));
            }
            _ => {
                findings.push(Finding::error(field, "a name that is neither a string nor {family, given}", "give each name as \"Family, Given\" or {\"family\": ..., \"given\": ...}"));
                return None;
            }
        }
    }
    Some(out)
}

/// A DOI: `10.<registrant>/<suffix>`, any resolver prefix taken off, lower-cased.
pub fn doi(field: &str, raw: &str, findings: &mut Vec<Finding>) -> Option<String> {
    let t = raw.trim();
    let mut d = t.to_string();
    for p in ["https://doi.org/", "http://doi.org/", "https://dx.doi.org/", "http://dx.doi.org/", "doi:"] {
        if d.to_ascii_lowercase().starts_with(p) {
            d = d[p.len()..].to_string();
        }
    }
    let d = d.to_lowercase();
    let ok = d.starts_with("10.") && d.split_once('/').is_some_and(|(reg, suffix)| reg.len() >= 7 && reg[3..].chars().all(|c| c.is_ascii_digit() || c == '.') && !suffix.is_empty() && !suffix.chars().any(char::is_whitespace));
    if !ok {
        findings.push(Finding::error(field, format!("`{}` is not a DOI", clip(t, 60)), "give it as 10.xxxx/suffix"));
        return None;
    }
    if d != t {
        findings.push(Finding::normalised(field, "a DOI with upper case or a resolver prefix", format!("stored as {d}")));
    }
    Some(d)
}

/// An ISBN-10 or ISBN-13 with its check digit verified; hyphens and spaces taken out.
pub fn isbn(field: &str, raw: &str, findings: &mut Vec<Finding>) -> Option<String> {
    let digits: String = raw.chars().filter(|c| !matches!(c, '-' | ' ')).collect::<String>().to_ascii_uppercase();
    let valid = match digits.len() {
        10 => {
            let mut sum = 0u32;
            for (i, c) in digits.chars().enumerate() {
                let v = match c {
                    '0'..='9' => c.to_digit(10).unwrap(),
                    'X' if i == 9 => 10,
                    _ => return bad_isbn(field, raw, findings),
                };
                sum += v * (10 - i as u32);
            }
            sum % 11 == 0
        }
        13 => {
            if !digits.chars().all(|c| c.is_ascii_digit()) {
                return bad_isbn(field, raw, findings);
            }
            let sum: u32 = digits.chars().enumerate().map(|(i, c)| c.to_digit(10).unwrap() * if i % 2 == 0 { 1 } else { 3 }).sum();
            sum % 10 == 0
        }
        _ => false,
    };
    if !valid {
        return bad_isbn(field, raw, findings);
    }
    if digits != raw {
        findings.push(Finding::normalised(field, "hyphens or spaces in the ISBN", format!("stored as {digits}")));
    }
    Some(digits)
}

fn bad_isbn(field: &str, raw: &str, findings: &mut Vec<Finding>) -> Option<String> {
    findings.push(Finding::error(field, format!("`{}` is not an ISBN (its check digit does not match)", clip(raw, 40)), "check the ISBN"));
    None
}

/// `issued` as CSL date parts. A date string (`2024`, `2024-03`, `2024-03-05`, or RFC 3339) becomes `{"date-parts": [[y, m, d]]}`.
fn issued(field: &str, v: &Value, findings: &mut Vec<Finding>) -> Option<Value> {
    if v["date-parts"].is_array() {
        return Some(json!({"date-parts": v["date-parts"]}));
    }
    let s = v.as_str().map(str::trim).unwrap_or("");
    let head = s.get(..10.min(s.len())).unwrap_or("");
    let parts: Vec<i64> = head.split('-').map_while(|p| p.parse::<i64>().ok()).collect();
    let ok = !parts.is_empty() && parts[0] > 0 && parts.len() <= 3 && parts.get(1).is_none_or(|m| (1..=12).contains(m)) && parts.get(2).is_none_or(|d| (1..=31).contains(d));
    if !ok {
        findings.push(Finding::error(field, "not a date", "give it as 2024, 2024-03 or 2024-03-05"));
        return None;
    }
    let out = json!({"date-parts": [parts]});
    findings.push(Finding::normalised(field, "a date given as text", format!("stored as {out}")));
    Some(out)
}

fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The fields of one item that are not on [`ITEM_FIELDS`], each refused by name.
pub fn unknown_fields(item: &Map<String, Value>, findings: &mut Vec<Finding>) {
    for k in item.keys().filter(|k| !ITEM_FIELDS.contains(&k.as_str())) {
        findings.push(Finding::error(k, "a field capture_source does not know", format!("use only {}", ITEM_FIELDS.join(", "))));
    }
}

/// `delphi sources lint` (§2.2 item 10): the validation run over a stored record. What normalising it again would change: the
/// locator, the dates, the title and the citation fields. Reads only.
pub fn lint_record(meta: &Value) -> Vec<Finding> {
    let mut f = Vec::new();
    if let Some(u) = meta["canonical_url"].as_str().or(meta["URL"].as_str()) {
        locator(u, &mut f);
    }
    if let Some(r) = meta["retrieved"].as_str() {
        date("retrieved", r, &mut f);
    }
    if let Some(t) = meta["title"].as_str() {
        title(t, &mut f);
    }
    let mut c = Map::new();
    for k in CSL_FIELDS.iter().filter(|k| **k != "title" && **k != "URL") {
        if let Some(v) = meta.get(*k).filter(|v| !v.is_null()) {
            c.insert(k.to_string(), v.clone());
        }
    }
    // A stored citation type of the library's own (`webpage`, `document`, a host's kind) is checked against the closed list too.
    csl(&Value::Object(c), &mut f);
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_doi_is_lower_cased_and_its_prefix_taken_off_and_a_bad_one_is_refused() {
        let mut f = Vec::new();
        assert_eq!(doi("DOI", "https://doi.org/10.1234/ABC.5", &mut f).as_deref(), Some("10.1234/abc.5"));
        assert_eq!(f[0].kind, Kind::Normalised);
        let mut f = Vec::new();
        assert_eq!(doi("DOI", "11.1234/x", &mut f), None);
        assert!(refused(&f));
    }

    #[test]
    fn an_isbns_check_digit_is_verified() {
        let mut f = Vec::new();
        assert_eq!(isbn("ISBN", "978-0-306-40615-7", &mut f).as_deref(), Some("9780306406157"));
        assert_eq!(isbn("ISBN", "0-306-40615-2", &mut f).as_deref(), Some("0306406152"));
        let mut f = Vec::new();
        assert_eq!(isbn("ISBN", "978-0-306-40615-8", &mut f), None);
        assert!(refused(&f));
    }

    #[test]
    fn a_locator_with_credentials_is_refused_and_an_address_loses_its_fragment() {
        let mut f = Vec::new();
        assert_eq!(locator("https://user:pw@example.org/a", &mut f), None);
        assert_eq!(f[0].field, "locator");
        let mut f = Vec::new();
        assert_eq!(locator("https://Example.org/a#part", &mut f), Some(("https://example.org/a".into(), true)));
        assert_eq!(locator("  the minutes of the March board meeting ", &mut f), Some(("the minutes of the March board meeting".into(), false)));
    }

    #[test]
    fn names_dates_and_types_are_normalised_or_refused() {
        let mut f = Vec::new();
        let c = csl(&json!({"author": "Doe, Jane", "issued": "2024-03-05", "type": "article-journal"}), &mut f);
        assert_eq!(c["author"], json!([{"family": "Doe", "given": "Jane"}]));
        assert_eq!(c["issued"], json!({"date-parts": [[2024, 3, 5]]}));
        assert_eq!(f.len(), 2);
        let mut f = Vec::new();
        csl(&json!({"type": "blog", "rating": 5}), &mut f);
        assert_eq!(f.iter().map(|x| x.field.as_str()).collect::<Vec<_>>(), ["csl.rating", "csl.type"]);
        let mut f = Vec::new();
        assert_eq!(date("retrieved", "2026-11-25", &mut f).as_deref(), Some("2026-11-25T00:00:00Z"));
        assert_eq!(date("retrieved", "last tuesday", &mut f), None);
        assert!(refused(&f));
    }

    #[test]
    fn text_is_stored_in_one_normal_form_and_bytes_that_are_not_utf8_are_refused() {
        let mut f = Vec::new();
        assert_eq!(text("text", "\u{feff}Cafe\u{301}\r\nnext", &mut f).as_deref(), Some("Caf\u{e9}\nnext"));
        assert_eq!(f[0].kind, Kind::Normalised);
        let mut f = Vec::new();
        assert_eq!(utf8("file", &[0x66, 0xff, 0x66], &mut f), None);
        assert_eq!(f[0].field, "file");
        let mut f = Vec::new();
        assert_eq!(media_type(Some("application/pdf"), b"%PDF", &mut f), None);
        assert!(refused(&f));
        let mut f = Vec::new();
        assert_eq!(media_type(Some("text/plain"), b"<!DOCTYPE html><p>x", &mut f).as_deref(), Some("text/html"));
        assert_eq!(f[0].kind, Kind::Normalised);
    }
}
