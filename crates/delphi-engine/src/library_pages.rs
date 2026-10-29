//! Generated pages and the dispute action (`specs/055` §6; slice SK5).
//!
//! A page is a view of claims for a topic, never a source and never trusted text. Every sentence on it carries the ids of
//! the claims behind it and the status the daemon derives for them; a sentence with no claim is not on the page, it is
//! flagged. A model may propose wording (`draft`), but the check below rejects the whole page if any sentence is uncited,
//! cites a claim that is not there or not citable, or uses a word none of its cited claims use. Without a draft the page
//! is the claims' own text, one sentence per claim. Unvalidated claims sit under a visible *noted, not validated* heading.
//! A page cannot be edited: `dispute` files a contested mark on the claim, which changes the next build. Pages are cached
//! by the hash of the claim set they were built from.

use super::bib::{self, Format};
use super::claims::status;
use super::*;
use delphi_core::knowledge_status::Status;
use std::collections::{BTreeSet, HashSet};

pub const NOTED_HEADING: &str = "Noted, not validated";
const NOTE_MAX: usize = 500;
const FUNCTION_WORDS: [&str; 40] = [
    "the", "and", "for", "are", "was", "were", "has", "have", "had", "with", "that", "this", "from", "its", "not", "but", "can", "may", "will", "also", "than", "then", "into", "per", "who", "which", "when", "where", "while", "their", "there", "these", "those", "been", "being", "such", "each", "any", "all", "our",
];

/// What the check found wrong with one sentence of a draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// No claim id: prose the library cannot account for.
    Uncited(String),
    UnknownClaim { sentence: String, id: String },
    /// The claim exists but may not be cited (disproved, retired, or resting on a private source).
    NotCitable { sentence: String, id: String },
    /// Words in the sentence that none of its cited claims contain.
    AddsContent { sentence: String, words: Vec<String> },
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Finding::Uncited(s) => write!(f, "uncited sentence: {}", short(s)),
            Finding::UnknownClaim { sentence, id } => write!(f, "unknown claim `{}` in: {}", id, short(sentence)),
            Finding::NotCitable { sentence, id } => write!(f, "claim `{}` may not be cited (disproved, retired or private) in: {}", id, short(sentence)),
            Finding::AddsContent { sentence, words } => write!(f, "adds content no cited claim states ({}) in: {}", words.join(", "), short(sentence)),
        }
    }
}

fn short(s: &str) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > 80 { format!("{}…", one.chars().take(79).collect::<String>()) } else { one }
}

/// One sentence of a draft: its text without markers and the claim ids it cites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    pub text: String,
    pub claims: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub claims: Vec<(String, Status)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub topic: String,
    pub validated: Vec<Line>,
    pub noted: Vec<Line>,
    pub sources: Vec<String>,
    /// Claims left off because they rest on a private source.
    pub withheld: usize,
    /// Hash of the claim set (ids, statuses, texts) this page was built from.
    pub hash: String,
    pub markdown: String,
    pub cached: bool,
}

fn glyph(s: Status) -> &'static str {
    match s {
        Status::Validated => "●",
        Status::Checking => "◐",
        Status::Disproved => "✕",
        Status::Contested => "◆",
        _ => "○",
    }
}

fn content_words(s: &str) -> BTreeSet<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).filter(|w| w.chars().any(|c| c.is_ascii_digit()) || (w.chars().count() >= 3 && !FUNCTION_WORDS.contains(&w.as_str()))).collect()
}

/// `[c-1]`, `[c-1, c-2]`, or the form the page itself prints, `[c-1 ● validated; c-2 ○ not validated]`.
fn marker(s: &str) -> Option<Vec<String>> {
    let inner = s.strip_prefix('[')?.strip_suffix(']')?;
    let shown = ["● validated", "○ not validated", "◆ not validated", "◐ not validated", "✕ disproved"];
    let mut ids = Vec::new();
    for seg in inner.split([',', ';']) {
        let seg = seg.trim();
        let (id, rest) = seg.split_once(' ').map_or((seg, ""), |(a, b)| (a, b.trim()));
        if !id.starts_with("c-") || !(rest.is_empty() || shown.contains(&rest)) {
            return None;
        }
        ids.push(id.to_string());
    }
    Some(ids)
}

/// Split a draft into sentences and their `[c-…]` markers. A marker belongs to the sentence it is in, or to the one just
/// ended when it follows the full stop.
pub fn parse_draft(draft: &str) -> Vec<Sentence> {
    let mut out: Vec<Sentence> = Vec::new();
    let mut cur = String::new();
    let mut ids: Vec<String> = Vec::new();
    let chars: Vec<char> = draft.chars().collect();
    let flush = |out: &mut Vec<Sentence>, cur: &mut String, ids: &mut Vec<String>| {
        let t = cur.split_whitespace().collect::<Vec<_>>().join(" ");
        if !t.is_empty() {
            out.push(Sentence { text: t, claims: std::mem::take(ids) });
        }
        cur.clear();
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '[' {
            if let Some(end) = chars[i..].iter().position(|&x| x == ']') {
                let tok: String = chars[i..=i + end].iter().collect();
                if let Some(found) = marker(&tok) {
                    if cur.trim().is_empty() {
                        if let Some(last) = out.last_mut() {
                            last.claims.extend(found);
                        } else {
                            ids.extend(found);
                        }
                    } else {
                        ids.extend(found);
                    }
                    i += end + 1;
                    continue;
                }
            }
        }
        cur.push(c);
        let ends = matches!(c, '.' | '!' | '?') && chars.get(i + 1).is_none_or(|n| n.is_whitespace() || *n == '[');
        if ends || c == '\n' && chars.get(i + 1) == Some(&'\n') {
            flush(&mut out, &mut cur, &mut ids);
        }
        i += 1;
    }
    flush(&mut out, &mut cur, &mut ids);
    for s in &mut out {
        let mut seen = HashSet::new();
        s.claims.retain(|c| seen.insert(c.clone()));
    }
    out
}

impl Library {
    fn pages_dir(&self) -> PathBuf {
        self.root().join("pages")
    }

    fn citable(&self, c: &Value) -> bool {
        !matches!(status(c), Status::Disproved | Status::Retired) && c["superseded_by"].is_null() && !self.rests_on_private(c)
    }

    /// The check at the heart of §6. Every sentence must cite claims that exist and may be cited, and may use only words
    /// that one of them uses. It reads claim text and the sentence text and nothing else, so it needs no model.
    pub fn check_page(&self, sentences: &[Sentence]) -> Vec<Finding> {
        let mut found = Vec::new();
        for s in sentences {
            if s.claims.is_empty() {
                found.push(Finding::Uncited(s.text.clone()));
                continue;
            }
            let mut allowed: BTreeSet<String> = BTreeSet::new();
            let mut ok = true;
            for id in &s.claims {
                match self.claim(id) {
                    None => {
                        found.push(Finding::UnknownClaim { sentence: s.text.clone(), id: id.clone() });
                        ok = false;
                    }
                    Some(c) if !self.citable(&c) => {
                        found.push(Finding::NotCitable { sentence: s.text.clone(), id: id.clone() });
                        ok = false;
                    }
                    Some(c) => allowed.extend(content_words(c["text"].as_str().unwrap_or(""))),
                }
            }
            if ok {
                let extra: Vec<String> = content_words(&s.text).difference(&allowed).cloned().collect();
                if !extra.is_empty() {
                    found.push(Finding::AddsContent { sentence: s.text.clone(), words: extra });
                }
            }
        }
        found
    }

    /// Claims on a topic: those whose text shares a word with it, or whose project is named like it. Disproved, retired,
    /// superseded and private-source claims are left out (the last counted).
    fn topic_claims(&self, topic: &str) -> (Vec<Value>, usize) {
        let q = content_words(topic);
        let mut withheld = 0;
        let mut out = Vec::new();
        for c in self.claims() {
            let hit = c["project"].as_str().is_some_and(|p| p.eq_ignore_ascii_case(topic.trim())) || !content_words(c["text"].as_str().unwrap_or("")).is_disjoint(&q);
            if !hit || matches!(status(&c), Status::Disproved | Status::Retired) || !c["superseded_by"].is_null() {
                continue;
            }
            if self.rests_on_private(&c) {
                withheld += 1;
                continue;
            }
            out.push(c);
        }
        (out, withheld)
    }

    fn set_hash(topic: &str, claims: &[Value], draft: Option<&str>) -> String {
        let mut h = Sha256::new();
        h.update(topic.as_bytes());
        for c in claims {
            h.update([0]);
            for part in [c["id"].as_str().unwrap_or(""), status(c).as_str(), c["text"].as_str().unwrap_or("")] {
                h.update(part.as_bytes());
                h.update([0]);
            }
        }
        if let Some(d) = draft {
            h.update(b"draft\0");
            h.update(d.as_bytes());
        }
        h.finalize().iter().take(8).map(|b| format!("{:02x}", b)).collect()
    }

    /// Build the page for `topic`. With a `draft` (wording a model proposed, `[c-…]` after each sentence) the page is the
    /// draft if the check passes and an error naming every finding if not; without one it is the claims' own sentences.
    /// A page built from the same claim set is read from the cache, so a library pays only for pages somebody reads.
    pub fn build_page(&self, topic: &str, draft: Option<&str>, style: Format) -> Result<Page, Vec<Finding>> {
        let topic = topic.split_whitespace().collect::<Vec<_>>().join(" ");
        let (claims, withheld) = self.topic_claims(&topic);
        let sentences: Vec<Sentence> = match draft {
            Some(d) => parse_draft(d),
            None => claims.iter().map(|c| Sentence { text: c["text"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" "), claims: vec![c["id"].as_str().unwrap_or("").into()] }).collect(),
        };
        let findings = self.check_page(&sentences);
        if !findings.is_empty() {
            return Err(findings);
        }
        let mut validated = Vec::new();
        let mut noted = Vec::new();
        let mut cited_sources: BTreeSet<String> = BTreeSet::new();
        for s in &sentences {
            let with: Vec<(String, Status)> = s.claims.iter().filter_map(|id| self.claim(id).map(|c| (id.clone(), status(&c)))).collect();
            for id in &s.claims {
                for a in self.claim(id).iter().flat_map(|c| c["anchors"].as_array().cloned().unwrap_or_default()) {
                    if let Some(sid) = a["source_id"].as_str() {
                        cited_sources.insert(sid.to_string());
                    }
                }
            }
            let line = Line { text: s.text.clone(), claims: with };
            if line.claims.iter().all(|(_, st)| *st == Status::Validated) { validated.push(line) } else { noted.push(line) }
        }
        let used: Vec<Value> = sentences.iter().flat_map(|s| s.claims.iter()).filter_map(|id| self.claim(id)).collect();
        let hash = Self::set_hash(&topic, if draft.is_some() { &used } else { &claims }, draft);
        let cache = self.pages_dir().join(format!("{}-{}.md", slug(&topic), hash));
        if let Ok(md) = std::fs::read_to_string(&cache) {
            return Ok(Page { topic, validated, noted, sources: cited_sources.into_iter().collect(), withheld, hash, markdown: md, cached: true });
        }
        let sources: Vec<String> = cited_sources.into_iter().collect();
        let bibliography = bib::export(self, &sources, style).map(|e| e.text).unwrap_or_default();
        let markdown = render(&topic, &validated, &noted, &bibliography);
        if self.ensure().is_ok() {
            let _ = std::fs::write(&cache, &markdown);
        }
        Ok(Page { topic, validated, noted, sources, withheld, hash, markdown, cached: false })
    }

    /// "This is wrong" on a sentence: a contested mark on the cited claim with the operator's note. The claim reads
    /// *not validated* from then on, so the next build moves its sentence under the noted heading and a validation
    /// cannot be recorded until the contest is resolved. Callers must be the operator's own process.
    pub fn dispute_claim(&self, id: &str, note: &str) -> Result<Value, String> {
        let note: String = note.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
        if note.is_empty() {
            return Err("a dispute needs the operator's note: what is wrong".into());
        }
        if note.chars().count() > NOTE_MAX {
            return Err(format!("a dispute note is at most {} characters", NOTE_MAX));
        }
        let mut c = self.claim(id).ok_or_else(|| format!("no claim `{}`", id))?;
        let mut marks = c["contested_by"].as_array().cloned().unwrap_or_default();
        marks.push(json!({"reason": "disputed", "source": "operator", "note": note, "at": delphi_core::time::now_rfc3339()}));
        c["contested_by"] = Value::Array(marks);
        self.save_claim(&c)?;
        self.log("claim_disputed", json!({"id": id}));
        Ok(c)
    }
}

fn slug(topic: &str) -> String {
    let s: String = topic.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    s.chars().take(40).collect::<String>().trim_end_matches('-').to_string()
}

fn cite(line: &Line) -> String {
    let ids: Vec<String> = line.claims.iter().map(|(id, st)| format!("{} {} {}", id, glyph(*st), st.label())).collect();
    format!("{} [{}]", line.text, ids.join("; "))
}

fn render(topic: &str, validated: &[Line], noted: &[Line], bibliography: &str) -> String {
    let mut s = format!("# {}\n\n_Generated from the library's claims. A view, not a source: it cannot be edited; change a claim or dispute it._\n\n", topic);
    for l in validated {
        s.push_str(&format!("{}\n\n", cite(l)));
    }
    if !noted.is_empty() {
        s.push_str(&format!("## {}\n\n", NOTED_HEADING));
        for l in noted {
            s.push_str(&format!("{}\n\n", cite(l)));
        }
    }
    if validated.is_empty() && noted.is_empty() {
        s.push_str("No claims on this topic yet.\n\n");
    }
    if !bibliography.trim().is_empty() {
        s.push_str(&format!("## Sources\n\n{}", bibliography));
    }
    s
}
