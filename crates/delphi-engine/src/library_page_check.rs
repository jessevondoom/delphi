//! The mechanical check of a generated page (`specs/057` §3.1; slice SK8a). No model, no network.
//!
//! A page is banner, body, bibliography table, in that order. The body cites with `[s-<source id>]` and `[c-<claim id>]`
//! markers; a marker supports the sentences back to the previous marker, so a paragraph needs one marker, not one per
//! sentence. The check rejects a marker naming a missing, retired, purged or disproved record (or a private one on an
//! external page), a status the marker misstates, a quotation that is not verbatim in a cited source's stored text, a
//! table with no citation, an image that is not an asset of a cited library source, a link no cited source's record
//! contains, a banner or table the writer wrote or changed, and a table with no row for a cited record. Numbers, dates
//! and proper names that appear in none of the cited material are flagged, not rejected.

use super::claims::status;
use super::excerpts::PAGE_QUOTE_MAX;
use super::*;
use delphi_core::knowledge_status::Status;
use std::collections::{BTreeMap, BTreeSet};

pub const TABLE_HEADING: &str = "Bibliography and citations";
pub const NO_SOURCES: &str = "no sources cited";
const BANNER_HEAD: &str = "Generated page. Not authoritative. Written by the writer role on ";
const BANNER_MID: &str = " from the sources listed below; ";
const BANNER_TAIL: &str = "). Read the cited sources before relying on it.";
const PENDING: &str = "not yet checked by the validator (";
const CHECKED: &str = "checked by the validator on ";
const SHOWN: [&str; 5] = ["● validated", "○ not validated", "◆ not validated", "◐ not validated", "✕ disproved"];
const MAX_FLAGS: usize = 50;

/// Where a page will be read: an external page never carries a private record (`specs/055` §10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Internal,
    External,
}

/// What the check found wrong with a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    BannerMissing,
    BannerAltered(String),
    /// The writer's own text contains the banner or the table heading, which only the daemon writes.
    BannerInBody,
    TableMissing,
    TableRowMissing(String),
    UnknownRecord(String),
    NotCitable { id: String, why: &'static str },
    StatusMisstated { id: String, shown: String },
    QuoteNotFound { quote: String },
    QuoteUnattributed { quote: String },
    QuoteTooLong { bytes: usize },
    SourceAltered(String),
    UnknownLink(String),
    UnknownImage(String),
    ImageUncited(String),
    TableUncited,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Finding::BannerMissing => write!(f, "the generated banner is missing from the top of the page"),
            Finding::BannerAltered(l) => write!(f, "the generated banner was changed: {}", short(l)),
            Finding::BannerInBody => write!(f, "the text contains the banner or the bibliography heading; only the daemon writes them"),
            Finding::TableMissing => write!(f, "the bibliography and citations table is missing"),
            Finding::TableRowMissing(id) => write!(f, "the table has no row for the cited record `{}`", id),
            Finding::UnknownRecord(id) => write!(f, "`{}` names no source or claim in the library", id),
            Finding::NotCitable { id, why } => write!(f, "`{}` may not be cited: {}", id, why),
            Finding::StatusMisstated { id, shown } => write!(f, "`{}` is shown as `{}`, which is not its status", id, shown),
            Finding::QuoteNotFound { quote } => write!(f, "quotation not verbatim in any cited source: {}", short(quote)),
            Finding::QuoteUnattributed { quote } => write!(f, "quotation block cites no source: {}", short(quote)),
            Finding::QuoteTooLong { bytes } => write!(f, "a quotation is {} bytes, over the {}-byte limit", bytes, PAGE_QUOTE_MAX),
            Finding::SourceAltered(id) => write!(f, "the stored text of `{}` no longer matches its recorded hash; its quotations cannot be checked", id),
            Finding::UnknownLink(u) => write!(f, "link not in any cited source's record: {}", short(u)),
            Finding::UnknownImage(r) => write!(f, "image is not an asset of a library source: {}", short(r)),
            Finding::ImageUncited(id) => write!(f, "image from `{}`, which the page does not cite", id),
            Finding::TableUncited => write!(f, "a table has no citation in or beside it"),
        }
    }
}

pub fn short(s: &str) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > 80 { format!("{}…", one.chars().take(79).collect::<String>()) } else { one }
}

/// One citation marker entry: `c-…` for a claim, `s-<source id>` for a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cite {
    pub id: String,
    pub source: bool,
    /// The status text the marker itself printed, if any (`● validated`).
    pub shown: Option<String>,
}

impl Cite {
    pub fn label(&self) -> String {
        if self.source { format!("s-{}", self.id) } else { self.id.clone() }
    }
}

/// The record a marker resolved to.
pub enum Rec {
    Claim(Value),
    Source(String, Value),
}

#[derive(Debug, Default)]
pub struct Checked {
    pub findings: Vec<Finding>,
    /// Numbers, dates and proper names found in no cited material.
    pub flags: Vec<String>,
    /// Every distinct record cited, first appearance first.
    pub cited: Vec<Cite>,
}

pub fn glyph(s: Status) -> &'static str {
    match s {
        Status::Validated => "●",
        Status::Checking => "◐",
        Status::Disproved => "✕",
        Status::Contested => "◆",
        _ => "○",
    }
}

pub fn shown_status(s: Status) -> String {
    format!("{} {}", glyph(s), s.label())
}

fn marker(tok: &str) -> Option<Vec<Cite>> {
    let inner = tok.strip_prefix('[')?.strip_suffix(']')?;
    let mut out = Vec::new();
    for seg in inner.split([',', ';']) {
        let seg = seg.trim();
        let (id, rest) = seg.split_once(' ').map_or((seg, ""), |(a, b)| (a, b.trim()));
        let (source, bare) = match id.strip_prefix("s-") {
            Some(b) => (true, b),
            None if id.starts_with("c-") => (false, id),
            None => return None,
        };
        if bare.is_empty() || !bare.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || !(rest.is_empty() || SHOWN.contains(&rest)) {
            return None;
        }
        out.push(Cite { id: bare.to_string(), source, shown: (!rest.is_empty()).then(|| rest.to_string()) });
    }
    Some(out)
}

/// Marker spans (byte range and the records named) in `text`. A bracket followed by `(` is a link, not a marker.
fn scan(text: &str) -> Vec<(usize, usize, Vec<Cite>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = text[i..].find('[') {
        let start = i + p;
        if let Some(e) = text[start..].find(']') {
            let end = start + e + 1;
            if !text[start + 1..end - 1].contains('[') && !text[end..].starts_with('(') {
                if let Some(c) = marker(&text[start..end]) {
                    out.push((start, end, c));
                    i = end;
                    continue;
                }
            }
        }
        i = start + 1;
    }
    out
}

pub fn markers(text: &str) -> Vec<Cite> {
    let mut seen = BTreeSet::new();
    scan(text).into_iter().flat_map(|(_, _, c)| c).filter(|c| seen.insert((c.source, c.id.clone()))).collect()
}

fn strip_markers(text: &str) -> String {
    let mut out = String::new();
    let mut at = 0;
    for (s, e, _) in scan(text) {
        out.push_str(&text[at..s]);
        at = e;
    }
    out.push_str(&text[at..]);
    out
}

pub fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn blocks(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for l in body.lines() {
        if l.trim().is_empty() {
            if !cur.is_empty() {
                out.push(cur.join("\n"));
                cur.clear();
            }
        } else {
            cur.push(l);
        }
    }
    if !cur.is_empty() {
        out.push(cur.join("\n"));
    }
    out
}

fn date_ok(d: &str) -> bool {
    d.len() == 10 && d.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })
}

/// The generated banner as the daemon writes it. `checked` is the validator's date and result once it has run.
pub fn banner(written: &str, checked: Option<(&str, &str)>, mechanical_note: &str) -> String {
    let middle = match checked {
        Some((date, result)) => format!("{}{} ({}", CHECKED, date, result),
        None => format!("{}{}", PENDING, mechanical_note),
    };
    format!("{}{}{}{}{}", BANNER_HEAD, written, BANNER_MID, middle, BANNER_TAIL)
}

/// The result text inside the banner's parentheses, if `line` is a well-formed banner.
fn parse_banner(line: &str) -> Option<String> {
    let rest = line.strip_prefix(BANNER_HEAD)?;
    let (date, rest) = rest.split_at_checked(10)?;
    let rest = rest.strip_prefix(BANNER_MID)?;
    let rest = if let Some(r) = rest.strip_prefix(PENDING) {
        r
    } else {
        let r = rest.strip_prefix(CHECKED)?;
        let (d, r) = r.split_at_checked(10)?;
        if !date_ok(d) {
            return None;
        }
        r.strip_prefix(" (")?
    };
    let result = rest.strip_suffix(BANNER_TAIL)?;
    (date_ok(date) && !result.trim().is_empty() && !result.contains(['\n', '(', ')'])).then(|| result.to_string())
}

fn is_banner_line(l: &str) -> bool {
    l.starts_with("Generated page.") || l.contains("Not authoritative.")
}

pub fn table_heading_line() -> String {
    format!("## {}", TABLE_HEADING)
}

fn separator_row(l: &str) -> bool {
    let t = l.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn is_table(block: &str) -> bool {
    let lines: Vec<&str> = block.lines().collect();
    lines.windows(2).any(|w| w[0].trim_start().starts_with('|') && separator_row(w[1]))
}

fn cell(s: &str) -> String {
    norm(&s.replace('|', "/"))
}

fn normalise_url(u: &str) -> String {
    u.split('#').next().unwrap_or("").trim_end_matches(['/', '.', ',', ';', ':']).to_string()
}

fn urls_of(meta: &Value, out: &mut BTreeSet<String>) {
    match meta {
        Value::String(s) if s.starts_with("http://") || s.starts_with("https://") => {
            out.insert(normalise_url(s));
        }
        Value::Array(a) => a.iter().for_each(|v| urls_of(v, out)),
        Value::Object(o) => o.values().for_each(|v| urls_of(v, out)),
        _ => {}
    }
    if let Some(d) = meta["DOI"].as_str() {
        out.insert(normalise_url(&format!("https://doi.org/{}", d.trim_start_matches("https://doi.org/"))));
    }
}

/// Every `](target)` of a markdown link or image, and bare http(s) URLs, with whether it is an image.
fn refs(text: &str) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = text[i..].find("](") {
        let at = i + p;
        let after = at + 2;
        let Some(close) = text[after..].find(')') else { break };
        let image = text[..at].rfind('[').is_some_and(|o| text[..o].ends_with('!'));
        out.push((image, text[after..after + close].trim().to_string()));
        i = after + close + 1;
    }
    for scheme in ["https://", "http://"] {
        let mut j = 0;
        while let Some(p) = text[j..].find(scheme) {
            let s = j + p;
            let e = text[s..].find(|c: char| c.is_whitespace() || matches!(c, ')' | '>' | ']' | '"' | '<')).map_or(text.len(), |x| s + x);
            let url = text[s..e].to_string();
            if !out.iter().any(|(_, t)| *t == url) {
                out.push((false, url));
            }
            j = e.max(s + 1);
        }
    }
    out
}

fn quotes_of(block: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut quoted = Vec::new();
    let mut rest = Vec::new();
    for l in block.lines() {
        match l.trim_start().strip_prefix('>') {
            Some(q) => quoted.push(q.trim().to_string()),
            None => rest.push(l),
        }
    }
    if !quoted.is_empty() {
        out.push((norm(&strip_markers(&quoted.join(" "))), true));
    }
    let text = strip_markers(&rest.join(" "));
    let mut open: Option<usize> = None;
    for (i, c) in text.char_indices() {
        match (c, open) {
            ('"' | '“', None) => open = Some(i + c.len_utf8()),
            ('"' | '”', Some(s)) => {
                let q = norm(&text[s..i]);
                if q.split_whitespace().count() >= 3 {
                    out.push((q, false));
                }
                open = None;
            }
            _ => {}
        }
    }
    out
}

fn capitalised(w: &str) -> bool {
    let mut cs = w.chars();
    cs.next().is_some_and(|c| c.is_uppercase()) && w.chars().count() >= 2 && w.chars().all(|c| c.is_alphabetic() || c == '-' || c == '\'' || c == '’')
}

/// Numbers, dates and proper names in `body` that `material` (lowercased) does not contain.
fn unverified(body: &str, material: &str) -> Vec<String> {
    let mut flags: Vec<String> = Vec::new();
    let mut fenced = false;
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || t.starts_with('#') {
            continue;
        }
        let mut plain = strip_markers(line);
        while let Some(p) = plain.find("](") {
            let end = plain[p..].find(')').map_or(plain.len(), |e| p + e + 1);
            plain.replace_range(p..end, "]");
        }
        let plain: String = plain.split_whitespace().filter(|w| !w.starts_with("http://") && !w.starts_with("https://")).collect::<Vec<_>>().join(" ");
        let toks: Vec<&str> = plain.split_whitespace().collect();
        let trim = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        let mut name: Vec<String> = Vec::new();
        let flush = |name: &mut Vec<String>, flags: &mut Vec<String>| {
            if !name.is_empty() {
                let n = name.join(" ");
                if !material.contains(&n.to_lowercase()) {
                    flags.push(n);
                }
                name.clear();
            }
        };
        for (k, w) in toks.iter().enumerate() {
            let core = trim(w);
            if core.chars().any(|c| c.is_ascii_digit()) {
                flush(&mut name, &mut flags);
                let num = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-').to_lowercase();
                if !material.contains(&num) && !material.contains(&core.to_lowercase()) {
                    flags.push(w.trim_matches(|c: char| !c.is_alphanumeric()).to_string());
                }
                continue;
            }
            let starts_sentence = k == 0 || matches!(toks[k - 1], "-" | "*" | ">" | "|") || toks[k - 1].ends_with(['.', '!', '?', ':', '|']);
            if capitalised(&core) && !starts_sentence && !w.starts_with('|') {
                name.push(core);
            } else {
                flush(&mut name, &mut flags);
            }
        }
        flush(&mut name, &mut flags);
    }
    let mut seen = BTreeSet::new();
    flags.retain(|f| !f.is_empty() && seen.insert(f.clone()));
    flags.truncate(MAX_FLAGS);
    flags
}

impl Library {
    pub(super) fn resolve(&self, c: &Cite, scope: Scope) -> Result<Rec, Finding> {
        let external = scope == Scope::External;
        if c.source {
            let meta = self.read_meta(&c.id).ok_or_else(|| Finding::UnknownRecord(c.label()))?;
            let why = if !meta["purged"].is_null() {
                Some("the source was purged")
            } else if !meta["retired"].is_null() {
                Some("the source is retired")
            } else if external && is_private(&meta) {
                Some("a private source on an external page")
            } else {
                None
            };
            return match why {
                Some(why) => Err(Finding::NotCitable { id: c.label(), why }),
                None => Ok(Rec::Source(c.id.clone(), meta)),
            };
        }
        let claim = self.claim(&c.id).ok_or_else(|| Finding::UnknownRecord(c.label()))?;
        let st = status(&claim);
        let why = if st == Status::Disproved {
            Some("the claim is disproved")
        } else if st == Status::Retired || !claim["superseded_by"].is_null() {
            Some("the claim is retired or superseded")
        } else if external && self.rests_on_private(&claim) {
            Some("the claim rests on a private source, on an external page")
        } else {
            None
        };
        if let Some(why) = why {
            return Err(Finding::NotCitable { id: c.label(), why });
        }
        if let Some(shown) = c.shown.as_ref().filter(|s| **s != shown_status(st)) {
            return Err(Finding::StatusMisstated { id: c.label(), shown: shown.clone() });
        }
        Ok(Rec::Claim(claim))
    }

    pub(super) fn rec_sources(rec: &Rec) -> Vec<String> {
        match rec {
            Rec::Source(id, _) => vec![id.clone()],
            Rec::Claim(c) => c["anchors"].as_array().into_iter().flatten().filter_map(|a| a["source_id"].as_str().map(String::from)).collect(),
        }
    }

    /// The stored text of a source (or its kept excerpts), after checking it against the recorded hash where that hash
    /// is of the stored bytes (a file capture).
    fn stored_text(&self, id: &str) -> Result<String, Finding> {
        let meta = self.read_meta(id).unwrap_or(Value::Null);
        match std::fs::read(self.source_dir(id).join("source.md")) {
            Ok(bytes) => {
                if meta["type"] == "document" && meta["content_hash"].as_str().is_some_and(|h| h != format!("sha256:{}", sha256_hex(&bytes))) {
                    return Err(Finding::SourceAltered(id.to_string()));
                }
                Ok(String::from_utf8_lossy(&bytes).into_owned())
            }
            Err(_) => Ok(self.excerpts(id).iter().filter_map(|e| e["text"].as_str()).collect::<Vec<_>>().join("\n")),
        }
    }

    /// The body checks of §3.1: records, quotations, tables, images, links; plus the unverified-figure flags.
    pub fn check_body(&self, body: &str, scope: Scope) -> Checked {
        let mut out = Checked::default();
        let push = |f: Finding, found: &mut Vec<Finding>| {
            if !found.contains(&f) {
                found.push(f);
            }
        };
        if body.lines().any(|l| is_banner_line(l) || l.trim() == table_heading_line()) {
            push(Finding::BannerInBody, &mut out.findings);
        }
        out.cited = markers(body);
        let mut recs: BTreeMap<String, Rec> = BTreeMap::new();
        for c in &out.cited {
            match self.resolve(c, scope) {
                Ok(r) => {
                    recs.insert(c.label(), r);
                }
                Err(f) => push(f, &mut out.findings),
            }
        }
        let sources_of = |text: &str| -> BTreeSet<String> {
            markers(text).iter().filter_map(|c| recs.get(&c.label())).flat_map(Self::rec_sources).collect()
        };
        let cited_sources: BTreeSet<String> = recs.values().flat_map(Self::rec_sources).collect();
        let direct: BTreeSet<String> = recs.values().filter_map(|r| if let Rec::Source(id, _) = r { Some(id.clone()) } else { None }).collect();

        let mut texts: BTreeMap<String, String> = BTreeMap::new();
        for id in &cited_sources {
            match self.stored_text(id) {
                Ok(t) => {
                    texts.insert(id.clone(), t);
                }
                Err(f) => push(f, &mut out.findings),
            }
        }

        let all = blocks(body);
        for (n, block) in all.iter().enumerate() {
            let sids = sources_of(block);
            for (q, is_block) in quotes_of(block) {
                if q.len() > PAGE_QUOTE_MAX {
                    push(Finding::QuoteTooLong { bytes: q.len() }, &mut out.findings);
                    continue;
                }
                if sids.is_empty() {
                    if is_block {
                        push(Finding::QuoteUnattributed { quote: q }, &mut out.findings);
                    }
                    continue;
                }
                let loose = q.trim_end_matches([',', '.', ';']);
                let found = sids.iter().filter_map(|s| texts.get(s)).any(|t| {
                    let t = norm(t);
                    t.contains(&q) || t.contains(loose)
                });
                let altered = out.findings.iter().any(|f| matches!(f, Finding::SourceAltered(id) if sids.contains(id)));
                if !found && !altered {
                    push(Finding::QuoteNotFound { quote: q }, &mut out.findings);
                }
            }
            if is_table(block) {
                let adjacent = |k: Option<usize>| k.and_then(|k| all.get(k)).is_some_and(|b| b.trim_start_matches(['*', '_', ' ']).to_lowercase().starts_with("table") && !markers(b).is_empty());
                if markers(block).is_empty() && !adjacent(n.checked_sub(1)) && !adjacent(Some(n + 1)) {
                    push(Finding::TableUncited, &mut out.findings);
                }
            }
        }

        let mut allowed = BTreeSet::new();
        for id in &cited_sources {
            if let Some(m) = self.read_meta(id) {
                urls_of(&m, &mut allowed);
            }
        }
        for (image, target) in refs(body) {
            if let Some(r) = target.strip_prefix("lib:") {
                let ok = image
                    && r.split_once('/').is_some_and(|(sid, file)| {
                        let file_ok = !file.is_empty() && !file.contains(['/', '\\']) && file != "..";
                        let known = self.read_meta(sid).is_some_and(|m| m["purged"].is_null() && !(scope == Scope::External && is_private(&m)));
                        file_ok && known && self.source_dir(sid).join("assets").join(file).is_file()
                    });
                if !ok {
                    push(Finding::UnknownImage(target), &mut out.findings);
                } else if let Some((sid, _)) = r.split_once('/') {
                    if !direct.contains(sid) {
                        push(Finding::ImageUncited(sid.to_string()), &mut out.findings);
                    }
                }
            } else if image {
                push(Finding::UnknownImage(target), &mut out.findings);
            } else if target.starts_with("http://") || target.starts_with("https://") {
                if !allowed.contains(&normalise_url(&target)) {
                    push(Finding::UnknownLink(target), &mut out.findings);
                }
            } else if !target.starts_with('#') {
                push(Finding::UnknownLink(target), &mut out.findings);
            }
        }

        let mut material = String::new();
        for r in recs.values() {
            if let Rec::Claim(c) = r {
                material.push_str(c["text"].as_str().unwrap_or(""));
                material.push('\n');
                for a in c["anchors"].as_array().into_iter().flatten() {
                    material.push_str(a["quote"].as_str().unwrap_or(""));
                    material.push('\n');
                }
            }
        }
        for t in texts.values() {
            material.push_str(t);
            material.push('\n');
        }
        out.flags = unverified(body, &norm(&material).to_lowercase());
        out
    }

    /// The whole page as a file: banner first, body, then the table last (`specs/057` §3.1). This is what `delphi pages
    /// check` runs.
    pub fn check_page_text(&self, text: &str, scope: Scope) -> Checked {
        let mut found = Vec::new();
        let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
        let first = lines.next().unwrap_or("");
        let result = parse_banner(first);
        if result.is_none() {
            found.push(if is_banner_line(first) { Finding::BannerAltered(first.to_string()) } else { Finding::BannerMissing });
        }
        let rest: Vec<&str> = lines.collect();
        let heading = table_heading_line();
        let at = rest.iter().rposition(|l| l.trim_end() == heading);
        let (body, table) = match at {
            Some(i) => (rest[..i].join("\n"), Some(rest[i + 1..].join("\n"))),
            None => (rest.join("\n"), None),
        };
        let mut checked = self.check_body(&body, scope);
        found.append(&mut checked.findings);
        match table {
            None => found.push(Finding::TableMissing),
            Some(t) => {
                let rows: Vec<Vec<String>> = t.lines().filter(|l| l.trim_start().starts_with('|') && !separator_row(l)).map(|l| l.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect()).collect();
                if rows.is_empty() {
                    found.push(Finding::TableMissing);
                }
                if checked.cited.is_empty() {
                    if !rows.iter().any(|r| r.first().is_some_and(|c| c == NO_SOURCES)) {
                        found.push(Finding::TableRowMissing(NO_SOURCES.into()));
                    }
                    if result.as_deref().is_some_and(|r| !r.contains("unsupported")) {
                        found.push(Finding::BannerAltered("a page with no citation is marked unsupported in its banner".into()));
                    }
                }
                for c in &checked.cited {
                    if !rows.iter().any(|r| r.first().is_some_and(|x| *x == c.label())) {
                        found.push(Finding::TableRowMissing(c.label()));
                    }
                }
            }
        }
        checked.findings = found;
        checked
    }

    /// The table of §3.1: one row per cited record, or the single "no sources cited" row.
    pub(super) fn bibliography_table(&self, cited: &[Cite], style: bib::Format, scope: Scope) -> String {
        let mut s = format!("{}\n\n| Id | Status | Title | Author or origin | Date | Locator | Citation |\n| :-- | :-- | :-- | :-- | :-- | :-- | :-- |\n", table_heading_line());
        let mut rows: Vec<(String, String)> = Vec::new();
        for c in cited {
            let Ok(rec) = self.resolve(c, scope) else { continue };
            let citation = |ids: &[String]| -> String {
                let text: Vec<String> = ids.iter().filter_map(|id| self.read_meta(id).map(|m| (id, m))).map(|(id, m)| if is_private(&m) { "(private source)".to_string() } else { bib::export(self, std::slice::from_ref(id), style).map(|e| norm(&e.text)).unwrap_or_default() }).filter(|t| !t.is_empty()).collect();
                if text.is_empty() { "—".into() } else { text.join(" ") }
            };
            let row = match &rec {
                Rec::Claim(cl) => {
                    let anchor = cl["anchors"].as_array().and_then(|a| a.first());
                    let date = cl["created"].as_str().map_or("—", |d| d.get(..10).unwrap_or(d));
                    format!("| {} | {} | {} | {} | {} | {} | {} |", c.label(), shown_status(status(cl)), cell(&short(cl["text"].as_str().unwrap_or(""))), cell(&self.origin_of(cl)), date, cell(anchor.and_then(|a| a["locator"].as_str()).filter(|l| !l.is_empty()).unwrap_or("—")), cell(&citation(&Self::rec_sources(&rec))))
                }
                Rec::Source(id, m) => {
                    let st = format!("○ {}", m["status"].as_str().unwrap_or("not validated"));
                    format!("| {} | {} | {} | {} | {} | — | {} |", c.label(), cell(&st), cell(m["title"].as_str().unwrap_or("—")), cell(&author_of(m)), date_of(m), cell(&citation(std::slice::from_ref(id))))
                }
            };
            rows.push((c.label(), row));
        }
        rows.sort();
        if rows.is_empty() {
            s.push_str(&format!("| {} | — | — | — | — | — | — |\n", NO_SOURCES));
        }
        for (_, r) in rows {
            s.push_str(&r);
            s.push('\n');
        }
        s
    }
}

fn author_of(m: &Value) -> String {
    let names: Vec<String> = m["author"].as_array().into_iter().flatten().filter_map(|a| a["literal"].as_str().map(String::from).or_else(|| a["family"].as_str().map(|f| format!("{}{}", a["given"].as_str().map_or(String::new(), |g| format!("{} ", g)), f)))).collect();
    if !names.is_empty() {
        return names.join("; ");
    }
    m["rights_holder"].as_str().or_else(|| m["origin"].as_str()).unwrap_or("—").to_string()
}

fn date_of(m: &Value) -> String {
    if let Some(p) = m["issued"]["date-parts"][0].as_array() {
        let parts: Vec<String> = p.iter().filter_map(|v| v.as_u64()).enumerate().map(|(i, n)| if i == 0 { n.to_string() } else { format!("{:02}", n) }).collect();
        if !parts.is_empty() {
            return parts.join("-");
        }
    }
    m["retrieved"].as_str().map_or("—".into(), |d| d.get(..10).unwrap_or(d).to_string())
}
