//! Generated pages and the dispute action (`specs/055` §6, amended by `specs/057`; slices SK5, SK8a).
//!
//! A page is a generated view for a topic, never a source and never trusted text: the daemon writes the banner that says
//! so and the bibliography and citations table, and a model may propose only the body between them (`draft`). The check
//! (`library_page_check.rs`) is mechanical: it rejects a body that cites a record that is missing, retired, disproved or
//! private on an external page, quotes text a cited source does not hold, or points at a table, image or link the library
//! does not know. It no longer asks that every sentence be cited or use a cited claim's words; a marker supports the
//! sentences back to the previous marker. Without a draft the page is the claims' own text, one line per claim, with the
//! unvalidated ones under a visible *noted, not validated* heading. A page cannot be edited: `dispute` files a contested
//! mark on the claim, which changes the next build. Pages are cached by the hash of what they were built from.

use super::bib::Format;
use super::claims::status;
use super::*;
use delphi_core::knowledge_status::Status;
use std::collections::BTreeSet;

pub use super::page_check::{banner, markers, shown_status, Checked, Cite, Finding, Scope, NO_SOURCES, TABLE_HEADING};
use super::page_check::{glyph, norm, Rec};

/// Bumped when the page layout changes, so cached pages of an older layout are rebuilt.
pub(super) const TEMPLATE_VERSION: &str = "2";

pub const NOTED_HEADING: &str = "Noted, not validated";
const NOTE_MAX: usize = 500;
const FUNCTION_WORDS: [&str; 40] = [
    "the", "and", "for", "are", "was", "were", "has", "have", "had", "with", "that", "this", "from", "its", "not", "but", "can", "may", "will", "also", "than", "then", "into", "per", "who", "which", "when", "where", "while", "their", "there", "these", "those", "been", "being", "such", "each", "any", "all", "our",
];

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
    /// Numbers, dates and proper names no cited material contains (`unverified figure`), kept in the sidecar.
    pub flags: Vec<String>,
}

pub(super) fn content_words(s: &str) -> BTreeSet<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).filter(|w| w.chars().any(|c| c.is_ascii_digit()) || (w.chars().count() >= 3 && !FUNCTION_WORDS.contains(&w.as_str()))).collect()
}

impl Library {
    pub(super) fn pages_dir(&self) -> PathBuf {
        self.root().join("pages")
    }

    /// Claims on a topic: those whose text shares a word with it, or whose project is named like it. Disproved, retired,
    /// superseded and private-source claims are left out (the last counted).
    pub(super) fn topic_claims(&self, topic: &str) -> (Vec<Value>, usize) {
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

    fn set_hash(&self, topic: &str, claims: &[Value], sources: &[String], draft: Option<&str>, scope: Scope) -> String {
        let mut h = Sha256::new();
        h.update(TEMPLATE_VERSION.as_bytes());
        h.update([scope as u8]);
        h.update(topic.as_bytes());
        for c in claims {
            h.update([0]);
            for part in [c["id"].as_str().unwrap_or(""), status(c).as_str(), c["text"].as_str().unwrap_or("")] {
                h.update(part.as_bytes());
                h.update([0]);
            }
        }
        for s in sources {
            h.update(b"source\0");
            h.update(s.as_bytes());
            h.update(self.read_meta(s).and_then(|m| m["content_hash"].as_str().map(String::from)).unwrap_or_default().as_bytes());
            h.update([0]);
        }
        if let Some(d) = draft {
            h.update(b"draft\0");
            h.update(d.as_bytes());
        }
        h.finalize().iter().take(8).map(|b| format!("{:02x}", b)).collect()
    }

    /// Build the page for `topic` with the strictest scope: a private record never appears (`Scope::External`).
    pub fn build_page(&self, topic: &str, draft: Option<&str>, style: Format) -> Result<Page, Vec<Finding>> {
        self.build_page_scoped(topic, draft, style, Scope::External)
    }

    /// Build the page for `topic`. With a `draft` (the body a model wrote, `[c-…]` and `[s-…]` markers after the sentences
    /// they support) the page is banner, draft and table if the mechanical check passes, and an error naming every finding
    /// if not; without one the body is the claims' own text. A page built from the same material is read from the cache,
    /// so a library pays only for pages somebody reads. This is the mechanical page only: its banner says the validator
    /// has not run (`build_page_checked` runs it).
    pub fn build_page_scoped(&self, topic: &str, draft: Option<&str>, style: Format, scope: Scope) -> Result<Page, Vec<Finding>> {
        let topic = topic.split_whitespace().collect::<Vec<_>>().join(" ");
        let (claims, withheld) = self.topic_claims(&topic);
        let (body, lines) = match draft {
            Some(d) => (d.trim().to_string(), None),
            None => {
                let lines = claim_lines(&claims);
                (render_body(&topic, &lines.0, &lines.1), Some(lines))
            }
        };
        let checked = self.check_body(&body, scope);
        if !checked.findings.is_empty() {
            return Err(checked.findings);
        }
        let hash_claims: Option<&[Value]> = if draft.is_some() { None } else { Some(&claims) };
        Ok(self.finish_page(PageParts { topic, body, lines, checked, hash_claims, hash_draft: draft.map(str::to_string), style, scope, withheld, stamp: Stamp::Mechanical, extra: Value::Null, use_cache: true }))
    }

    /// Lay the page out and store it: the banner for `parts.stamp`, the body, the table; the file and its sidecar go to
    /// `pages/`. The status is always `generated`.
    pub(super) fn finish_page(&self, p: PageParts) -> Page {
        let PageParts { topic, body, lines, checked, hash_claims, hash_draft, style, scope, withheld, stamp, extra, use_cache } = p;
        let (validated, noted) = lines.unwrap_or_else(|| self.body_lines(&body));
        let recs: Vec<Rec> = checked.cited.iter().filter_map(|c| self.resolve(c, scope).ok()).collect();
        let sources: Vec<String> = recs.iter().flat_map(Self::rec_sources).collect::<BTreeSet<_>>().into_iter().collect();
        let used: Vec<Value> = recs.iter().filter_map(|r| if let Rec::Claim(c) = r { Some(c.clone()) } else { None }).collect();
        let hash = self.set_hash(&topic, hash_claims.unwrap_or(&used), &sources, hash_draft.as_deref(), scope);
        let cache = self.pages_dir().join(format!("{}-{}.md", slug(&topic), hash));
        if use_cache {
            if let Ok(md) = std::fs::read_to_string(&cache) {
                return Page { topic, validated, noted, sources, withheld, hash, markdown: md, cached: true, flags: checked.flags };
            }
        }
        let date = delphi_core::time::now_rfc3339().chars().take(10).collect::<String>();
        let unsupported = if checked.cited.is_empty() { format!("; unsupported: {}", NO_SOURCES) } else { String::new() };
        let head = match &stamp {
            Stamp::Mechanical => banner(&date, None, &format!("mechanical checks passed on {}{}", date, unsupported)),
            Stamp::Validator { result } => banner(&date, Some((&date, &format!("{}{}", result, unsupported))), ""),
            Stamp::NotRun { note } => banner(&date, None, &format!("{}{}", note, unsupported)),
        };
        let markdown = format!("{}\n\n{}\n\n{}", head, body, self.bibliography_table(&checked.cited, style, scope));
        if self.ensure().is_ok() {
            let _ = std::fs::create_dir_all(self.pages_dir());
            let _ = std::fs::write(&cache, &markdown);
            let mut sidecar = json!({"topic": topic, "template": TEMPLATE_VERSION, "written": date, "scope": format!("{:?}", scope).to_lowercase(), "cited": checked.cited.iter().map(Cite::label).collect::<Vec<_>>(), "unverified_figures": checked.flags, "status": "generated"});
            if let (Some(o), Some(e)) = (sidecar.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            let _ = std::fs::write(cache.with_extension("json"), serde_json::to_vec_pretty(&sidecar).unwrap_or_default());
        }
        Page { topic, validated, noted, sources, withheld, hash, markdown, cached: false, flags: checked.flags }
    }

    /// The paragraphs of a written body that carry a marker, split by whether every claim they cite is validated.
    fn body_lines(&self, body: &str) -> (Vec<Line>, Vec<Line>) {
        let (mut validated, mut noted) = (Vec::new(), Vec::new());
        for block in body.split("\n\n") {
            let cites = markers(block);
            if cites.is_empty() {
                continue;
            }
            let claims: Vec<(String, Status)> = cites.iter().filter(|c| !c.source).filter_map(|c| self.claim(&c.id).map(|cl| (c.id.clone(), status(&cl)))).collect();
            let line = Line { text: norm(block), claims };
            if !line.claims.is_empty() && line.claims.iter().all(|(_, st)| *st == Status::Validated) { validated.push(line) } else { noted.push(line) }
        }
        (validated, noted)
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

pub(super) fn slug(topic: &str) -> String {
    let s: String = topic.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    s.chars().take(40).collect::<String>().trim_end_matches('-').to_string()
}

/// What the banner says about the check.
pub(super) enum Stamp {
    Mechanical,
    /// The validator ran; `result` goes in the banner's parentheses.
    Validator { result: String },
    /// The validator did not run (`note` says why).
    NotRun { note: String },
}

pub(super) struct PageParts<'a> {
    pub topic: String,
    pub body: String,
    pub lines: Option<(Vec<Line>, Vec<Line>)>,
    pub checked: Checked,
    pub hash_claims: Option<&'a [Value]>,
    pub hash_draft: Option<String>,
    pub style: Format,
    pub scope: Scope,
    pub withheld: usize,
    pub stamp: Stamp,
    pub extra: Value,
    pub use_cache: bool,
}

pub(super) fn claim_lines(claims: &[Value]) -> (Vec<Line>, Vec<Line>) {
    let (mut validated, mut noted) = (Vec::new(), Vec::new());
    for c in claims {
        let id = c["id"].as_str().unwrap_or("").to_string();
        let line = Line { text: norm(c["text"].as_str().unwrap_or("")), claims: vec![(id, status(c))] };
        if line.claims.iter().all(|(_, st)| *st == Status::Validated) { validated.push(line) } else { noted.push(line) }
    }
    (validated, noted)
}

fn cite(line: &Line) -> String {
    let ids: Vec<String> = line.claims.iter().map(|(id, st)| format!("{} {} {}", id, glyph(*st), st.label())).collect();
    format!("{} [{}]", line.text, ids.join("; "))
}

pub(super) fn render_body(topic: &str, validated: &[Line], noted: &[Line]) -> String {
    let mut s = format!("# {}\n\n", topic);
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
    s.trim_end().to_string()
}
