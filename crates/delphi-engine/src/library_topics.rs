//! Topic pages (`specs/117` §2; slice E1): a page that belongs to a topic, not to a search, rebuilt in place.
//!
//! A topic's id is a slug (`web-bot-auth`). Its page is `library/pages/<slug>.md` and its record the sidecar `<slug>.json`
//! (`kind: topic_page`); the sidecar is written when the topic is made, before any page exists. A topic is made from a research
//! topic (`specs/112`; [`Library::topics_from_research`]), or by the operator (`delphi pages new <title>`).
//!
//! **Rebuilt in place.** Each rebuild moves the page it replaces, with its record, to `library/pages/versions/<slug>/<stamp>.md`
//! and `.json`, so the versions are listed and readable; the new record carries one line, computed by code from the two texts,
//! that says what changed (`3 statements added, 1 source replaced`). Nothing in the line is a model's.
//!
//! **Due.** A page is due when it has never been built, a claim or source it was built from changed or left the library, a
//! claim of one of its research topics is not yet in it, or its lifetime has passed ([`Library::topic_due`]). The daemon rebuilds
//! due pages while it is idle, within `[library.pages] per_day`, unless `auto_rebuild = false`; the operator can always ask
//! (`delphi pages rebuild <slug>`).
//!
//! **What a topic page may carry.** It is written by the writer from the material the daemon gathered and checked by the same
//! mechanical check and validator as a search page, so it is never a source and never trusted text. No private record is put on
//! it or in the writer's prompt (the page is built at `Scope::External`, as `[library] private_to_models = deny` asks), whatever
//! the reader: the operator reads private sources through the library's own views, not through a page a model wrote.
//!
//! Reading a topic, its versions or its links writes nothing ([`Library::topic_pages`], [`Library::topic_get`],
//! [`Library::topic_versions`], [`Library::topic_links`]).

use super::page_check::{blocks, markers, norm, strip_markers};
use super::page_meta::{self, Depth, Front};
use super::page_search::{claim_print, clean_draft, norm_ws, source_print, Gathered};
use super::page_validate::{CheckedPage, Outcome, PageValidator, PageWriter};
use super::pages::{slug, Scope};
use super::*;
use crate::fact_judge::TokenLedger;
use delphi_adapter::config::PagesConfig;
use std::collections::{BTreeMap, BTreeSet};

/// The sidecar's `kind`: a search page's sidecar says `search_page`, so neither reader ever takes the other's file.
pub const KIND: &str = "topic_page";
/// The longest slug.
pub const MAX_SLUG: usize = 60;
const TITLE_MAX: usize = 200;
/// Names under `library/pages/` that are not topics.
const RESERVED: [&str; 4] = ["budget", "history", "versions", "links"];
/// The link table: which topic pages link to which (`specs/117` §2 item 6).
pub const LINKS_FILE: &str = "links.json";
/// The page every topic is built at: never a private record (see the module's comment).
pub const SCOPE: Scope = Scope::External;
/// How many versions a topic keeps; the oldest goes first.
pub const MAX_VERSIONS: usize = 50;

/// A topic slug as this module makes and accepts them: lowercase letters, digits and single hyphens, at most [`MAX_SLUG`]. A
/// search page's key (24 hex characters), the mechanical page's `<slug>-<16 hex>` and the folder and file names of
/// `library/pages/` are refused, so a topic's files can never be another page's.
pub fn valid_slug(s: &str) -> bool {
    let shape = !s.is_empty() && s.len() <= MAX_SLUG && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !s.starts_with('-') && !s.ends_with('-') && !s.contains("--");
    let hashed = s.rsplit_once('-').is_some_and(|(_, t)| t.len() == 16 && t.bytes().all(|b| b.is_ascii_hexdigit()));
    shape && !hashed && !RESERVED.contains(&s) && !super::page_search::valid_key(s)
}

/// The slug a title is given: the page slug of its words, or `topic` when it has none.
pub fn slug_for(title: &str) -> String {
    let s = slug(title);
    let s = if s.is_empty() { "topic".to_string() } else { s };
    if valid_slug(&s) { s } else { format!("topic-{}", s.chars().take(MAX_SLUG - 6).collect::<String>().trim_end_matches('-')) }
}

/// Where a topic came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A research topic of `specs/112`, by its id.
    Research(String),
    /// The operator named it (`delphi pages new`).
    Operator,
}

impl Origin {
    fn to_json(&self) -> Value {
        match self {
            Origin::Research(id) => json!({"kind": "research", "topic_id": id}),
            Origin::Operator => json!({"kind": "operator"}),
        }
    }
}

/// One topic as the lists show it.
#[derive(Debug, Clone, PartialEq)]
pub struct TopicEntry {
    pub slug: String,
    pub title: String,
    pub origin: Value,
    pub research_topics: Vec<String>,
    pub created: u64,
    /// When the current version was built; `None` before the first build.
    pub built: Option<u64>,
    /// How many times it has been built.
    pub version: u64,
    /// What changed in the last build, one line.
    pub change: Option<String>,
    pub outcome: Option<String>,
    pub valid_until: Option<u64>,
    pub cited: Vec<String>,
    /// Why the page is due for a rebuild, or `None`.
    pub due: Option<String>,
    /// The record as stored, for the fields later slices add.
    pub side: Value,
}

impl TopicEntry {
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "slug": self.slug, "title": self.title, "origin": self.origin, "research_topics": self.research_topics, "created": self.created,
            "built": self.built, "version": self.version, "change": self.change, "outcome": self.outcome, "valid_until": self.valid_until,
            "valid_until_date": self.valid_until.map(page_meta::ymd), "cited": self.cited, "due": self.due, "status": "generated",
        });
        for k in ["key_terms", "start_here", "see_also", "open_questions", "links", "records"] {
            if !self.side[k].is_null() {
                v[k] = self.side[k].clone();
            }
        }
        v
    }
}

/// What a due check reads besides the library: the workspace, for the research topics a page follows.
pub struct DueCtx<'a> {
    pub ws: Option<&'a Path>,
    pub now: u64,
    /// Whether a cited knowledge-base passage or finding is looked up in the workspace's index to see whether it changed. A read for
    /// a reader leaves it off: opening an index can make its side files, and a read writes nothing.
    pub index: bool,
}

/// What a rebuild is given. The caller gathers (`Library::gather` and the retrieval service, `topic_gather`): the library does
/// not reach the index on its own.
pub struct TopicRequest<'a> {
    pub slug: &'a str,
    pub now: u64,
    pub settings: &'a PagesConfig,
    pub style: bib::Format,
    pub gathered: Gathered,
    /// The gaps of the topic's research records: the page's open questions, written by the daemon ([`Library::topic_gaps`]).
    pub gaps: Vec<String>,
}

/// The most key terms a page carries (`specs/117` §2 item 5).
pub const MAX_KEY_TERMS: usize = 12;
/// How many records `## Start here` lists.
pub const START_HERE: std::ops::RangeInclusive<usize> = 3..=5;
/// The most open questions the daemon writes.
pub const MAX_OPEN_QUESTIONS: usize = 8;
pub const KEY_TERMS: &str = "Key terms";
pub const START_HERE_HEADING: &str = "Start here";
pub const SEE_ALSO: &str = "See also";
pub const OPEN_QUESTIONS: &str = "Open questions";

/// The lines of the `## <name>` section of `body` that are not blank, up to the next heading; `None` when there is no such section.
pub fn section<'a>(body: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let mut lines = body.lines();
    lines.find(|l| l.trim().strip_prefix("## ").is_some_and(|h| h.trim().eq_ignore_ascii_case(name)))?;
    Some(lines.take_while(|l| !l.trim_start().starts_with('#')).filter(|l| !l.trim().is_empty()).collect())
}

/// `- **Term**: definition [marker]` as (term, definition without markers, the labels it cites).
fn key_term(line: &str) -> Option<(String, String, Vec<String>)> {
    let rest = line.trim().strip_prefix("- **")?;
    let (term, def) = rest.split_once("**")?;
    let def = def.trim_start().strip_prefix(':')?.trim();
    let term = norm(term.trim_end_matches(':'));
    (!term.is_empty() && !def.is_empty()).then(|| (term, norm(&strip_markers(def)), markers(def).iter().map(|c| c.label()).collect()))
}

/// An item of a numbered list, without its number.
fn numbered(line: &str) -> Option<&str> {
    let t = line.trim();
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    (digits > 0).then(|| t[digits..].strip_prefix('.')).flatten().map(str::trim)
}

/// The `page:` slugs a line links to.
fn page_slugs(line: &str) -> Vec<String> {
    page_meta::page_links(line).into_iter().filter_map(|(_, t)| t.strip_prefix("page:").map(String::from)).collect()
}

/// What the check finds wrong with a topic page's own sections (`specs/117` §2 item 5): each is optional, but one that is there must be
/// right. Key terms: at most twelve, each `- **Term**: definition` with a marker (the marker itself is resolved by the check like any
/// other). Start here: three to five numbered items, each naming a record by its marker. See also: `page:` links only, each to a topic
/// that exists.
pub fn section_findings(body: &str, slugs: &BTreeSet<String>) -> Vec<super::pages::Finding> {
    use super::pages::Finding;
    let mut out = Vec::new();
    if let Some(lines) = section(body, KEY_TERMS) {
        if lines.len() > MAX_KEY_TERMS {
            out.push(Finding::KeyTerm(format!("{} terms; at most {}", lines.len(), MAX_KEY_TERMS)));
        }
        for l in &lines {
            match key_term(l) {
                None => out.push(Finding::KeyTerm(format!("not a `- **Term**: definition [marker]` line: {}", l.trim()))),
                Some((t, _, cites)) if cites.is_empty() => out.push(Finding::KeyTerm(format!("`{}` has no citation marker", t))),
                _ => {}
            }
        }
    }
    if let Some(lines) = section(body, START_HERE_HEADING) {
        if !START_HERE.contains(&lines.len()) {
            out.push(Finding::StartHere(format!("{} items; three to five", lines.len())));
        }
        for l in &lines {
            match numbered(l) {
                Some(item) if !markers(item).is_empty() => {}
                _ => out.push(Finding::StartHere(format!("an item names no record: {}", l.trim()))),
            }
        }
    }
    if let Some(lines) = section(body, SEE_ALSO) {
        for l in &lines {
            let found = page_slugs(l);
            if found.is_empty() {
                out.push(Finding::UnknownSeeAlso(l.trim().to_string()));
            }
            for s in found.into_iter().filter(|s| !slugs.contains(s)) {
                out.push(Finding::UnknownSeeAlso(s));
            }
        }
    }
    if section(body, OPEN_QUESTIONS).is_some() {
        out.push(Finding::KeyTerm("the page writes its own `## Open questions`; the daemon adds them from the topic's gaps".into()));
    }
    out
}

/// What a built page's sections say, for the topic's record: key terms, the start-here records in order, the see-also slugs.
pub fn sections_json(body: &str) -> Value {
    let terms: Vec<Value> = section(body, KEY_TERMS).unwrap_or_default().iter().filter_map(|l| key_term(l)).map(|(t, d, c)| json!({"term": t, "definition": d, "cites": c})).collect();
    let start: Vec<Value> = section(body, START_HERE_HEADING).unwrap_or_default().iter().filter_map(|l| numbered(l)).map(|i| json!({"cites": markers(i).iter().map(|c| c.label()).collect::<Vec<_>>(), "why": norm(&strip_markers(i))})).collect();
    let see: Vec<String> = section(body, SEE_ALSO).unwrap_or_default().iter().flat_map(|l| page_slugs(l)).collect::<BTreeSet<_>>().into_iter().collect();
    json!({"key_terms": terms, "start_here": start, "see_also": see})
}

/// The `## Open questions` section the daemon writes from the topic's gaps: the research record's words, labelled as such.
pub fn open_questions_section(gaps: &[String]) -> Option<String> {
    let qs: Vec<String> = gaps.iter().map(|g| norm(&g.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>())).filter(|g| !g.is_empty()).take(MAX_OPEN_QUESTIONS).collect();
    if qs.is_empty() {
        return None;
    }
    let mut s = format!("## {}

*What the research on this topic has not answered yet, as its record lists it; the page does not answer it.*

", OPEN_QUESTIONS);
    for q in qs {
        s.push_str(&format!("- {}
", q.replace(['[', ']'], "")));
    }
    Some(s)
}

/// The brief for a topic's page: a search page's brief (layout, citation rules, records), the three sections of `specs/117` §2 item 5,
/// and the topics `## See also` may name.
pub fn topic_brief(title: &str, g: &Gathered, max_tokens: u64, topics: &[(String, String)]) -> String {
    let mut s = super::page_search::brief_for(title, g, max_tokens, Depth::Summary);
    s.push_str("\nAfter your last section, and only as far as the records support them, add these sections in this order:\n");
    s.push_str(&format!("- `## {}`: at most {} lines, each `- **Term**: one-line definition [marker]`, the marker citing the record the definition comes from.\n", KEY_TERMS, MAX_KEY_TERMS));
    s.push_str(&format!("- `## {}`: a numbered list of three to five records to read first, in the order to read them, each line naming one record by its marker and saying in a few words why it comes first.\n", START_HERE_HEADING));
    s.push_str(&format!("- `## {}`: lines `- [Title](page:slug)` naming only topics from the TOPICS list below; leave the section out when none is related.\n", SEE_ALSO));
    s.push_str(&format!("Do not write a `## {}` section: the daemon adds it.\n\nTOPICS (other topic pages that exist; data):\n", OPEN_QUESTIONS));
    for (slug, t) in topics.iter().take(60) {
        s.push_str(&format!("TOPIC {} | {}\n", slug, t.replace('\n', " ").chars().take(120).collect::<String>()));
    }
    s
}

/// The change line of `specs/117` §2 item 2, from the page's previous body and its new one: statements (paragraphs or items that
/// carry a citation marker) added and removed, and cited records added, removed and replaced (one removed and one added is one
/// replaced). Computed by code.
pub fn change_line(old: Option<&str>, new: &str) -> String {
    let Some(old) = old else { return "first version".into() };
    let statements = |body: &str| -> BTreeSet<String> { blocks(body).iter().filter(|b| !markers(b).is_empty()).map(|b| norm(&strip_markers(b)).to_lowercase()).filter(|s| !s.is_empty()).collect() };
    let cited = |body: &str| -> BTreeSet<String> { markers(body).iter().map(|c| c.label()).collect() };
    let (so, sn) = (statements(old), statements(new));
    let (co, cn) = (cited(old), cited(new));
    let (s_add, s_del) = (sn.difference(&so).count(), so.difference(&sn).count());
    let (c_add, c_del) = (cn.difference(&co).count(), co.difference(&cn).count());
    let replaced = c_add.min(c_del);
    let mut parts: Vec<String> = Vec::new();
    let n = |k: usize, one: &str, many: &str, what: &str| format!("{} {} {}", k, if k == 1 { one } else { many }, what);
    if s_add > 0 {
        parts.push(n(s_add, "statement", "statements", "added"));
    }
    if s_del > 0 {
        parts.push(n(s_del, "statement", "statements", "removed"));
    }
    if replaced > 0 {
        parts.push(n(replaced, "source", "sources", "replaced"));
    }
    if c_add > replaced {
        parts.push(n(c_add - replaced, "source", "sources", "added"));
    }
    if c_del > replaced {
        parts.push(n(c_del - replaced, "source", "sources", "removed"));
    }
    if parts.is_empty() { "no statement or source changed".into() } else { parts.join(", ") }
}

/// `YYYYMMDDTHHMMSSZ` for a Unix time: a version's file name.
fn stamp(secs: u64) -> String {
    let day = page_meta::ymd(secs).replace('-', "");
    let s = secs % 86_400;
    format!("{}T{:02}{:02}{:02}Z", day, s / 3600, (s / 60) % 60, s % 60)
}

fn valid_stamp(s: &str) -> bool {
    let (base, n) = s.split_once('-').map_or((s, None), |(b, n)| (b, Some(n)));
    base.len() == 16 && base.as_bytes()[8] == b'T' && base.ends_with('Z') && base.bytes().enumerate().all(|(i, b)| i == 8 || i == 15 || b.is_ascii_digit()) && n.is_none_or(|n| !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

fn clip_title(s: &str) -> String {
    let t = norm_ws(&s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>());
    t.chars().take(TITLE_MAX).collect()
}

impl Library {
    fn topic_file(&self, slug: &str) -> PathBuf {
        self.pages_dir().join(format!("{}.md", slug))
    }

    fn topic_side_path(&self, slug: &str) -> PathBuf {
        self.pages_dir().join(format!("{}.json", slug))
    }

    fn topic_versions_dir(&self, slug: &str) -> PathBuf {
        self.pages_dir().join("versions").join(slug)
    }

    /// A topic's record, if `slug` is a topic.
    pub fn topic_side(&self, slug: &str) -> Option<Value> {
        if !valid_slug(slug) {
            return None;
        }
        let v: Value = serde_json::from_slice(&std::fs::read(self.topic_side_path(slug)).ok()?).ok()?;
        (v["kind"] == KIND && v["slug"] == slug).then_some(v)
    }

    fn write_topic_side(&self, slug: &str, v: &Value) -> Result<(), String> {
        self.ensure().map_err(|e| e.to_string())?;
        self.write_json(&self.topic_side_path(slug), v)
    }

    /// Make a topic: its record, no page yet. A topic that already has `slug` is returned as it is (making it again changes
    /// nothing), except that a research topic named again is added to the ones it follows.
    pub fn topic_create(&self, title: &str, origin: Origin, now: u64) -> Result<TopicEntry, String> {
        let title = clip_title(title);
        if title.is_empty() {
            return Err("a topic needs a title".into());
        }
        let slug = slug_for(&title);
        if let Some(mut side) = self.topic_side(&slug) {
            if let Origin::Research(id) = &origin {
                let mut ids: Vec<String> = side["research_topics"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
                if !ids.contains(id) {
                    ids.push(id.clone());
                    side["research_topics"] = json!(ids);
                    self.write_topic_side(&slug, &side)?;
                }
            }
            return Ok(self.topic_entry(&slug, &side, None));
        }
        let research: Vec<String> = match &origin {
            Origin::Research(id) => vec![id.clone()],
            Origin::Operator => vec![],
        };
        let side = json!({"kind": KIND, "status": "generated", "slug": slug, "title": title, "query": title, "origin": origin.to_json(), "research_topics": research, "created": now, "built": null, "version": 0, "material": [], "cited": []});
        self.write_topic_side(&slug, &side)?;
        self.log("topic_created", json!({"slug": slug, "origin": origin.to_json()}));
        Ok(self.topic_entry(&slug, &side, None))
    }

    /// One topic made for each research topic of the workspace and its projects that is not private and has none yet (§2 item 1).
    /// Returns the slugs it made.
    pub fn topics_from_research(&self, ws: &Path, now: u64) -> Vec<String> {
        let followed: BTreeSet<String> = self.topic_sides().iter().flat_map(|(_, s)| s["research_topics"].as_array().cloned().unwrap_or_default()).filter_map(|x| x.as_str().map(String::from)).collect();
        let mut made = Vec::new();
        for (t, rec) in crate::research_record::list(ws) {
            if rec["private"] != false || followed.contains(&t.id) {
                continue;
            }
            let Some(q) = rec["question"].as_str() else { continue };
            if let Ok(e) = self.topic_create(q, Origin::Research(t.id.clone()), now) {
                made.push(e.slug);
            }
        }
        made
    }

    fn topic_sides(&self) -> Vec<(String, Value)> {
        let Ok(rd) = std::fs::read_dir(self.pages_dir()) else { return Vec::new() };
        let mut out: Vec<(String, Value)> = rd.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(String::from)).filter_map(|s| self.topic_side(&s).map(|v| (s, v))).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn topic_entry(&self, slug: &str, side: &Value, due: Option<&DueCtx<'_>>) -> TopicEntry {
        let strs = |k: &str| side[k].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>();
        TopicEntry {
            slug: slug.to_string(),
            title: side["title"].as_str().unwrap_or(slug).to_string(),
            origin: side["origin"].clone(),
            research_topics: strs("research_topics"),
            created: side["created"].as_u64().unwrap_or(0),
            built: side["built"].as_u64(),
            version: side["version"].as_u64().unwrap_or(0),
            change: side["change"].as_str().map(String::from),
            outcome: side["outcome"].as_str().map(String::from),
            valid_until: side["valid_until"].as_u64(),
            cited: strs("cited"),
            due: due.and_then(|d| self.topic_due(side, d)),
            side: side.clone(),
        }
    }

    /// Every topic, by slug, with why each is due. Reads only.
    pub fn topic_pages(&self, ctx: &DueCtx<'_>) -> Vec<TopicEntry> {
        self.topic_sides().iter().map(|(s, v)| self.topic_entry(s, v, Some(ctx))).collect()
    }

    /// One topic and its page (the current one, or the version `version` names), the frontmatter left off. A topic not yet
    /// built has an empty page. Reads only.
    pub fn topic_get(&self, slug: &str, version: Option<&str>, ctx: &DueCtx<'_>) -> Option<(TopicEntry, String)> {
        let side = self.topic_side(slug)?;
        let entry = self.topic_entry(slug, &side, Some(ctx));
        let md = match version {
            None => std::fs::read_to_string(self.topic_file(slug)).unwrap_or_default(),
            Some(v) if valid_stamp(v) => std::fs::read_to_string(self.topic_versions_dir(slug).join(format!("{}.md", v))).ok()?,
            Some(_) => return None,
        };
        Some((entry, page_meta::strip(&md).to_string()))
    }

    /// The earlier versions of a topic's page, newest first: their stamp, when each was built and what it changed. Reads only.
    pub fn topic_versions(&self, slug: &str) -> Option<Vec<Value>> {
        self.topic_side(slug)?;
        let Ok(rd) = std::fs::read_dir(self.topic_versions_dir(slug)) else { return Some(Vec::new()) };
        let mut out: Vec<Value> = rd
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".md")).filter(|n| valid_stamp(n)).map(String::from))
            .map(|v| {
                let side: Value = std::fs::read(self.topic_versions_dir(slug).join(format!("{}.json", v))).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
                json!({"version": v, "built": side["built"], "number": side["version"], "change": side["change"], "outcome": side["outcome"]})
            })
            .collect();
        out.sort_by(|a, b| b["version"].as_str().cmp(&a["version"].as_str()));
        Some(out)
    }

    /// Why a topic's page is due for a rebuild (§2 item 3), or `None`. Reads only.
    pub fn topic_due(&self, side: &Value, ctx: &DueCtx<'_>) -> Option<String> {
        if side["built"].as_u64().is_none() {
            return Some("not built yet".into());
        }
        let mut held: BTreeSet<String> = BTreeSet::new();
        for m in side["material"].as_array().into_iter().flatten() {
            let (id, was) = (m["id"].as_str().unwrap_or(""), m["hash"].as_str().unwrap_or(""));
            held.insert(id.to_string());
            // A knowledge-base passage or a finding: its text as the index holds it now (`specs/117` §2 item 4). Checked when the
            // workspace is known.
            if super::page_check::is_record_label(id) {
                let (Some(ws), true) = (ctx.ws, ctx.index) else { continue };
                let Some(r) = side["records"].as_array().into_iter().flatten().filter_map(super::page_check::KbRecord::from_json).find(|r| r.label == id) else { continue };
                match crate::topic_gather::record_hash_now(ws, &r.rel_path, r.start_line) {
                    None => return Some(format!("`{}` is gone from the knowledge base", id)),
                    Some(h) if h != was => return Some(format!("`{}` changed", id)),
                    _ => continue,
                }
            }
            let now_print = match id.strip_prefix("s-") {
                Some(sid) => self.read_meta(sid).map(|meta| source_print(sid, &meta)),
                None => self.claim(id).map(|c| claim_print(&c)),
            };
            match now_print {
                None => return Some(format!("`{}` is gone from the library", id)),
                Some(p) if p != was => return Some(format!("`{}` changed", id)),
                _ => {}
            }
        }
        if let Some(ws) = ctx.ws {
            for tid in side["research_topics"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                let Some(rec) = crate::research_record::find(ws, tid).and_then(|t| t.read().ok()) else { continue };
                if let Some(c) = rec["claims"].as_array().into_iter().flatten().filter_map(Value::as_str).find(|c| !held.contains(*c) && self.claim(c).is_some_and(|cl| live(&cl))) {
                    return Some(format!("`{}` is a new claim of research topic `{}`", c, tid));
                }
            }
        }
        if side["valid_until"].as_u64().is_some_and(|u| ctx.now > u) {
            return Some(format!("its lifetime ended on {}", page_meta::ymd(side["valid_until"].as_u64().unwrap_or(0))));
        }
        None
    }

    /// The claims of the research topics `side` follows that are still live: a rebuild's material always holds them, so a claim a
    /// research run added to the topic reaches the page whatever its words.
    pub fn topic_research_claims(&self, side: &Value, ws: &Path) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for tid in side["research_topics"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let Some(rec) = crate::research_record::find(ws, tid).and_then(|t| t.read().ok()) else { continue };
            if rec["private"] != false {
                continue;
            }
            for id in rec["claims"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if let Some(c) = self.claim(id).filter(|c| live(c) && !self.rests_on_private(c)) {
                    if !out.iter().any(|x| x["id"] == c["id"]) {
                        out.push(c);
                    }
                }
            }
        }
        out
    }

    /// The material for a topic's page from the library alone: its research topics' claims first, then what its words find.
    pub fn topic_gather_library(&self, side: &Value, ws: Option<&Path>) -> Gathered {
        let mut g = self.gather(side["query"].as_str().or(side["title"].as_str()).unwrap_or(""), SCOPE, &[]);
        if let Some(ws) = ws {
            let mut first = self.topic_research_claims(side, ws);
            for c in std::mem::take(&mut g.claims) {
                if !first.iter().any(|x| x["id"] == c["id"]) {
                    first.push(c);
                }
            }
            first.truncate(2 * super::page_search::MAX_CLAIMS);
            g.claims = first;
        }
        g
    }

    /// Rebuild a topic's page (§2 item 2): the writer writes from `r.gathered`, the page passes the same check and validator as a
    /// search page, the page it replaces becomes a version, and the record says what changed. The knowledge-base records of
    /// `r.gathered` are cited only if this library was given the same ones ([`Library::with_records`]). `gate` is asked before any model is
    /// called; a page counts against `[library.pages] per_day`.
    pub async fn topic_rebuild(&self, r: TopicRequest<'_>, writer: &dyn PageWriter, validator: &dyn PageValidator, ledger: &TokenLedger, gate: super::page_search::Gate<'_>) -> Result<TopicEntry, String> {
        if !r.settings.enabled {
            return Err("generated pages are off ([library.pages] enabled = false)".into());
        }
        let side = self.topic_side(r.slug).ok_or_else(|| format!("no topic `{}`", r.slug))?;
        let title = side["title"].as_str().unwrap_or(r.slug).to_string();
        let g = r.gathered;
        let mut proposal = None;
        let checked = if g.is_empty() {
            self.claims_only_page(&title, "the library holds nothing on this topic yet", None, Vec::new(), 0, r.style, SCOPE)?
        } else {
            if self.pages_today(r.now) >= r.settings.per_day {
                return Err(format!("today's page budget ({} pages) is spent; the topic stays due", r.settings.per_day));
            }
            gate()?;
            self.count_page(r.now);
            let topics: Vec<(String, String)> = self.topic_sides().into_iter().filter(|(s, _)| s != r.slug).map(|(s, v)| (s, v["title"].as_str().unwrap_or("").to_string())).collect();
            match writer.write(&topic_brief(&title, &g, r.settings.max_tokens, &topics)).await {
                Ok(d) => {
                    let (body, p) = page_meta::extract(&d);
                    proposal = p;
                    self.build_page_checked(&title, &clean_draft(&body, r.settings.max_tokens), r.style, SCOPE, validator, writer, ledger).await?
                }
                Err(e) => self.claims_only_page(&title, &format!("the writer could not write the page: {}", e), None, Vec::new(), 0, r.style, SCOPE)?,
            }
        };
        self.topic_store(r.slug, side, &g, checked, proposal.as_ref(), r.now, r.settings, &r.gaps)
    }

    /// Every topic's slug: what a topic page's `## See also` may name. The library that checks a topic page is given them
    /// ([`Library::with_topic_slugs`]).
    pub fn topic_slugs(&self) -> BTreeSet<String> {
        self.topic_sides().into_iter().map(|(s, _)| s).collect()
    }

    /// The gaps of the research topics `side` follows, as their records list them (`specs/117` §2 item 5: the open questions).
    pub fn topic_gaps(&self, side: &Value, ws: &Path) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for tid in side["research_topics"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let Some(rec) = crate::research_record::find(ws, tid).and_then(|t| t.read().ok()) else { continue };
            for g in rec["gaps"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if !out.iter().any(|x| x == g) {
                    out.push(g.to_string());
                }
            }
        }
        out
    }

    /// Put a built page in place: the current page and its record become a version, the new page and record are written, and the
    /// link table is brought up to date.
    #[allow(clippy::too_many_arguments)]
    fn topic_store(&self, slug: &str, old: Value, g: &Gathered, c: CheckedPage, proposal: Option<&page_meta::Proposal>, now: u64, settings: &PagesConfig, gaps: &[String]) -> Result<TopicEntry, String> {
        self.ensure().map_err(|e| e.to_string())?;
        let mut md = format!("{}\n", page_meta::extract(&c.page.markdown).0);
        // The open questions are the daemon's, from the research record, put just above the bibliography: the writer's text and the
        // validator never saw them, and they cite nothing.
        if let Some(q) = open_questions_section(gaps) {
            let heading = super::page_check::table_heading_line();
            match md.rfind(&format!("\n{}", heading)) {
                Some(i) => md.insert_str(i + 1, &format!("{}\n", q)),
                None => md.push_str(&format!("\n{}", q)),
            }
        }
        let old_md = std::fs::read_to_string(self.topic_file(slug)).ok();
        let old_body = old_md.as_deref().map(|m| page_meta::strip(m).to_string());
        if let (Some(prev), Some(built)) = (old_md.as_ref(), old["built"].as_u64()) {
            let dir = self.topic_versions_dir(slug);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let base = stamp(built);
            let name = (0..1000).map(|n| if n == 0 { base.clone() } else { format!("{}-{}", base, n) }).find(|n| !dir.join(format!("{}.md", n)).exists()).ok_or("no free name for the version")?;
            std::fs::write(dir.join(format!("{}.md", name)), prev).map_err(|e| e.to_string())?;
            self.write_json(&dir.join(format!("{}.json", name)), &old)?;
            self.prune_versions(slug);
        }
        let claims_only = c.verdict.outcome == Outcome::ClaimsOnly;
        let mut meta = page_meta::resolve(proposal, settings.min_ttl_days, settings.max_ttl_days);
        if claims_only {
            meta.days = settings.min_ttl_days.max(1);
            meta.reason = "the page is the claims alone; it is looked at again soon".into();
        }
        let valid_until = now + meta.days as u64 * 86_400;
        let title = old["title"].as_str().unwrap_or(slug).to_string();
        let links = page_meta::page_links(&md);
        let created = old["created"].as_u64().unwrap_or(now);
        let front = page_meta::frontmatter(&Front { key: slug, title: &title, depth: Depth::Summary, meta: &meta, status: if claims_only { "unverified" } else { "active" }, created, confirmed: now, valid_until, links: &links });
        let change = change_line(old_body.as_deref(), page_meta::strip(&md));
        std::fs::write(self.topic_file(slug), format!("{}{}", front, md)).map_err(|e| e.to_string())?;
        // The mechanical page's own cache file is not a topic's.
        let temp = format!("{}-{}", super::pages::slug(&c.page.topic), c.page.hash);
        let _ = std::fs::remove_file(self.pages_dir().join(format!("{}.md", temp)));
        let _ = std::fs::remove_file(self.pages_dir().join(format!("{}.json", temp)));
        let mut material: Vec<Value> = g.claims.iter().map(|cl| json!({"id": cl["id"], "hash": claim_print(cl)})).collect();
        material.extend(g.sources.iter().map(|(id, m, _)| json!({"id": format!("s-{}", id), "hash": source_print(id, m)})));
        material.extend(g.records.iter().map(|r| json!({"id": r.label, "hash": r.hash})));
        let mut side = old.clone();
        side["records"] = json!(g.records.iter().map(|r| r.to_json()).collect::<Vec<_>>());
        side["built"] = json!(now);
        side["version"] = json!(old["version"].as_u64().unwrap_or(0) + 1);
        side["material"] = json!(material);
        side["cited"] = json!(self.cited_labels(&md));
        side["change"] = json!(change);
        side["outcome"] = json!(match c.verdict.outcome {
            Outcome::Pass => "pass",
            Outcome::PassContested => "pass_contested",
            Outcome::ClaimsOnly => "claims_only",
        });
        side["reason"] = json!(c.verdict.reason);
        side["validator"] = c.verdict.to_json();
        side["valid_until"] = json!(valid_until);
        side["volatility"] = json!(meta.volatility.as_str());
        side["ttl_reason"] = json!(meta.reason);
        side["links"] = json!(links.iter().filter_map(|(_, t)| t.strip_prefix("page:").map(String::from)).collect::<Vec<_>>());
        let parts = sections_json(page_meta::strip(&md));
        for k in ["key_terms", "start_here", "see_also"] {
            side[k] = parts[k].clone();
        }
        side["open_questions"] = json!(gaps.iter().take(MAX_OPEN_QUESTIONS).collect::<Vec<_>>());
        self.write_topic_side(slug, &side)?;
        self.update_links(slug, &side);
        self.log("topic_built", json!({"slug": slug, "version": side["version"], "outcome": side["outcome"], "change": change}));
        Ok(self.topic_entry(slug, &side, None))
    }

    fn prune_versions(&self, slug: &str) {
        let dir = self.topic_versions_dir(slug);
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let mut names: Vec<String> = rd.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".md")).filter(|n| valid_stamp(n)).map(String::from)).collect();
        names.sort();
        while names.len() > MAX_VERSIONS {
            let n = names.remove(0);
            let _ = std::fs::remove_file(dir.join(format!("{}.md", n)));
            let _ = std::fs::remove_file(dir.join(format!("{}.json", n)));
        }
    }

    fn links_path(&self) -> PathBuf {
        self.pages_dir().join(LINKS_FILE)
    }

    /// The link table as stored: `{"<from slug>": ["<to slug>", ...]}`.
    fn link_table(&self) -> BTreeMap<String, Vec<String>> {
        std::fs::read(self.links_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn update_links(&self, slug: &str, side: &Value) {
        let mut t = self.link_table();
        let to: Vec<String> = side["links"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).filter(|s| s != slug).collect::<BTreeSet<_>>().into_iter().collect();
        if to.is_empty() {
            t.remove(slug);
        } else {
            t.insert(slug.to_string(), to);
        }
        let _ = self.write_json(&self.links_path(), &json!(t));
    }

    /// One source as the operator's views show it (`specs/117` §3 item 3, `library.source`): its record, whether it is private, its kept
    /// text or excerpts up to `max` characters, and its pictures. Reached only by the console and the paired app, never by a seat or a
    /// host. Reads only.
    pub fn source_view(&self, id: &str, max: usize) -> Option<Value> {
        let meta = self.read_meta(id)?;
        let text = self.stored_text(id).unwrap_or_default();
        let truncated = text.chars().count() > max;
        let assets: Vec<String> = self.image_assets(id).into_iter().map(|a| a.uri).collect();
        Some(json!({"id": id, "label": format!("s-{}", id), "meta": meta, "private": is_private(&meta), "capture_level": meta["capture_level"], "status": meta["status"], "text": text.chars().take(max).collect::<String>(), "truncated": truncated, "assets": assets}))
    }

    /// What links to `slug` and what it links to (`specs/117` §2 item 6), from the link table. Reads only.
    pub fn topic_links(&self, slug: &str) -> Option<Value> {
        if !valid_slug(slug) {
            return None;
        }
        let t = self.link_table();
        let here: Vec<&String> = t.iter().filter(|(from, to)| *from != slug && to.iter().any(|x| x == slug)).map(|(from, _)| from).collect();
        let out = t.get(slug).cloned().unwrap_or_default();
        Some(json!({"slug": slug, "links_here": here, "links_out": out, "exists": self.topic_side(slug).is_some()}))
    }
}

/// Live: not disproved, retired or superseded.
fn live(c: &Value) -> bool {
    !matches!(super::claims::status(c), delphi_core::knowledge_status::Status::Disproved | delphi_core::knowledge_status::Status::Retired) && c["superseded_by"].is_null()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_cannot_name_another_pages_file() {
        assert!(valid_slug("web-bot-auth") && valid_slug("tides") && valid_slug("a1"));
        for bad in ["", "-a", "a-", "a--b", "Tides", "a/b", "../x", "budget", "versions", "links", "history", "0123456789abcdef01234567", "tides-0123456789abcdef", &"a".repeat(61)] {
            assert!(!valid_slug(bad), "{bad}");
        }
        assert_eq!(slug_for("Web Bot Auth!"), "web-bot-auth");
        assert_eq!(slug_for("???"), "topic");
        assert_eq!(slug_for("budget"), "topic-budget");
    }

    #[test]
    fn the_change_line_counts_statements_and_sources_by_code() {
        assert_eq!(change_line(None, "x"), "first version");
        let old = "# T\n\nTides come twice a day [c-01].\n\nThe moon pulls the sea [s-a].";
        let new = "# T\n\nTides come twice a day [c-01].\n\nThe moon pulls the sea [s-b].\n\nSpring tides are higher [c-02].\n\nNeap tides are lower [c-03].";
        // "the moon" statement is the same words (markers aside), so it is not counted; two statements are new; s-a was replaced by s-b; two claims added.
        assert_eq!(change_line(Some(old), new), "2 statements added, 1 source replaced, 2 sources added");
        assert_eq!(change_line(Some(new), new), "no statement or source changed");
        assert_eq!(change_line(Some(new), "# T\n\nTides come twice a day [c-01]."), "3 statements removed, 3 sources removed");
    }

    #[test]
    fn a_stamp_is_the_only_version_name_read() {
        assert_eq!(stamp(0), "19700101T000000Z");
        assert!(valid_stamp("20261115T010203Z") && valid_stamp("20261115T010203Z-2"));
        for bad in ["", "../x", "20261115T010203", "20261115X010203Z", "20261115T010203Z-", "20261115T010203Z-1234"] {
            assert!(!valid_stamp(bad), "{bad}");
        }
    }
}

#[cfg(test)]
mod rebuild_tests {
    use super::*;
    use crate::library::claims::{AnchorSpec, ClaimOpts};
    use crate::library::page_validate::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Writes one sentence per claim record in the brief, each with its marker: the page follows the material exactly.
    pub(crate) struct EchoWriter(pub AtomicUsize);
    impl PageWriter for EchoWriter {
        fn write<'a>(&'a self, b: &'a str) -> WriterFuture<'a> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let mut out = String::from("# Tides\n\n");
            let lines: Vec<&str> = b.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if let Some(rest) = l.strip_prefix("RECORD c-") {
                    let id = format!("c-{}", rest.split(' ').next().unwrap_or(""));
                    out.push_str(&format!("{} [{}].\n\n", lines.get(i + 1).unwrap_or(&"").trim_end_matches('.'), id));
                }
            }
            Box::pin(async move { Ok(out) })
        }
        fn rewrite<'a>(&'a self, d: &'a str, _f: &'a [String]) -> WriterFuture<'a> {
            let d = d.to_string();
            Box::pin(async move { Ok(d) })
        }
    }
    pub(crate) struct Pass;
    impl PageValidator for Pass {
        fn validate<'a>(&'a self, _q: &'a PageQuery) -> ValidatorFuture<'a> {
            Box::pin(async move { Ok(ValidatorReply { assertions: vec![Assertion { text: "x".into(), judgement: Judgement::Supported, record: None }], model: "m".into(), family: "f".into(), tokens: 10 }) })
        }
    }

    fn rebuild(l: &Library, slug: &str, ws: &Path, now: u64, w: &EchoWriter) -> TopicEntry {
        let side = l.topic_side(slug).unwrap();
        let g = l.topic_gather_library(&side, Some(ws));
        let cfg = PagesConfig::default();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(l.topic_rebuild(TopicRequest { slug, now, settings: &cfg, style: bib::Format::Plain, gathered: g, gaps: vec![] }, w, &Pass, &TokenLedger::new(1_000_000), &|| Ok(()))).unwrap()
    }

    /// AC-1: a research run adds a claim to the topic; the page is due; the rebuild keeps the same file, lists the old text as a
    /// version and says that one statement was added.
    #[test]
    fn a_new_research_claim_makes_the_page_due_and_the_rebuild_keeps_one_file_and_a_version() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home.path());
        let doc = home.path().join("t.md");
        std::fs::write(&doc, "# Tides\n\nThe tide comes in twice a day. Spring tides follow the full moon.\n").unwrap();
        let s = l.add_file(&doc, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
        let anchor = |q: &str| AnchorSpec { source_id: s.clone(), locator: Some("§1".into()), quote: q.into(), ..Default::default() };
        let c1 = l.add_claim("Ocean tides come twice a day.", &ClaimOpts::default(), &[anchor("twice a day")]).unwrap()["id"].as_str().unwrap().to_string();
        let b = crate::research_record::Begin { question: "When do ocean tides come?", asked_by: crate::research_record::Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r1" };
        let topic = crate::research_record::Topic::begin(&ws, &b).unwrap();
        topic.finish("r1", "done", Some(&json!({"claims": [&c1]})), std::slice::from_ref(&c1), None, 0).unwrap();

        let made = l.topics_from_research(&ws, 1_000);
        assert_eq!(made, vec!["when-do-ocean-tides-come".to_string()]);
        assert!(l.topics_from_research(&ws, 1_001).is_empty(), "a research topic makes one topic page, once");
        let slug = &made[0];
        let ctx = DueCtx { ws: Some(&ws), now: 1_000, index: false };
        assert_eq!(l.topic_pages(&ctx)[0].due.as_deref(), Some("not built yet"));

        let w = EchoWriter(AtomicUsize::new(0));
        let first = rebuild(&l, slug, &ws, 2_000, &w);
        assert_eq!((first.version, first.change.as_deref(), first.outcome.as_deref()), (1, Some("first version"), Some("pass")), "{first:?}");
        let page = l.pages_dir().join(format!("{slug}.md"));
        let v1 = std::fs::read_to_string(&page).unwrap();
        assert!(v1.contains("Ocean tides come twice a day") && v1.contains("Generated page. Not authoritative."));
        assert_eq!(l.topic_pages(&DueCtx { ws: Some(&ws), now: 2_001, index: false })[0].due, None, "just built: not due");

        // A later research run adds a claim to the topic.
        let c2 = l.add_claim("Spring tides follow the full moon.", &ClaimOpts::default(), &[anchor("Spring tides follow the full moon")]).unwrap()["id"].as_str().unwrap().to_string();
        let b2 = crate::research_record::Begin { run_id: "r2", ..b };
        crate::research_record::Topic::begin(&ws, &b2).unwrap().finish("r2", "done", Some(&json!({"claims": [&c2]})), &[c1.clone(), c2.clone()], None, 0).unwrap();
        let due = l.topic_pages(&DueCtx { ws: Some(&ws), now: 2_002, index: false }).remove(0).due.unwrap();
        assert!(due.contains(&c2) && due.contains("new claim"), "{due}");

        let second = rebuild(&l, slug, &ws, 3_000, &w);
        assert_eq!((second.version, second.change.as_deref()), (2, Some("1 statement added, 1 source added")), "{second:?}");
        let v2 = std::fs::read_to_string(&page).unwrap();
        assert!(v2.contains("Spring tides follow the full moon") && v2 != v1, "the same file holds the new text");
        let versions = l.topic_versions(slug).unwrap();
        assert_eq!(versions.len(), 1);
        let stamp = versions[0]["version"].as_str().unwrap().to_string();
        assert_eq!(versions[0]["change"], "first version");
        let (_, old) = l.topic_get(slug, Some(&stamp), &DueCtx { ws: Some(&ws), now: 3_001, index: false }).unwrap();
        assert!(old.contains("Ocean tides come twice a day") && !old.contains("Spring tides"), "the earlier text is readable as a version");
        assert!(l.topic_get(slug, Some("../../x"), &DueCtx { ws: None, now: 0, index: false }).is_none());
        // A cited claim that changes makes the page due again.
        l.dispute_claim(&c2, "the moon is not the only cause").unwrap();
        assert!(l.topic_pages(&DueCtx { ws: Some(&ws), now: 3_002, index: false })[0].due.as_deref().unwrap().contains("changed"));
        // Its lifetime passing does too.
        let late = second.valid_until.unwrap() + 1;
        let mut fresh = l.topic_side(slug).unwrap();
        fresh["material"] = json!([]);
        fresh["research_topics"] = json!([]);
        assert!(l.topic_due(&fresh, &DueCtx { ws: None, now: late, index: false }).unwrap().contains("lifetime"));
    }

    #[test]
    fn the_operator_makes_a_topic_and_a_search_page_never_reads_it() {
        let home = tempfile::tempdir().unwrap();
        let l = Library::in_home(home.path());
        let e = l.topic_create("Web Bot Auth", Origin::Operator, 5).unwrap();
        assert_eq!((e.slug.as_str(), e.built, e.version), ("web-bot-auth", None, 0));
        assert_eq!(l.topic_create("web bot auth", Origin::Operator, 6).unwrap().created, 5, "the same slug is the same topic");
        assert!(l.topic_create("  ", Origin::Operator, 7).is_err());
        assert!(l.pages(10, 100).is_empty(), "a topic's record is not a search page's");
        assert!(l.topic_get("web-bot-auth", None, &DueCtx { ws: None, now: 10, index: false }).unwrap().1.is_empty(), "not built yet: no page");
    }
}

#[cfg(test)]
mod section_tests {
    use super::*;
    use crate::library::claims::{AnchorSpec, ClaimOpts};
    use crate::library::page_validate::*;
    use crate::library::pages::Finding;

    fn slugs(s: &[&str]) -> BTreeSet<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn key_terms_start_here_and_see_also_are_checked_by_code() {
        let ok = "# T\n\nBody [c-01].\n\n## Key terms\n\n- **Spring tide**: the larger range after a full moon [c-01].\n\n## Start here\n\n1. [s-a] the tide table.\n2. [c-01] the claim.\n3. [s-kb-0123456789abcdef] the note.\n\n## See also\n\n- [Moon](page:moon)\n";
        assert!(section_findings(ok, &slugs(&["moon"])).is_empty(), "{:?}", section_findings(ok, &slugs(&["moon"])));
        let f = section_findings(ok, &slugs(&[]));
        assert_eq!(f, vec![Finding::UnknownSeeAlso("moon".into())]);
        let bad = "## Key terms\n\n- **Neap**: smaller range.\n- a loose line [c-01]\n\n## Start here\n\n1. read the table\n2. [c-01] the claim\n\n## See also\n\n- Moon\n\n## Open questions\n\n- why?\n";
        let f = section_findings(bad, &slugs(&["moon"]));
        assert!(f.iter().any(|x| matches!(x, Finding::KeyTerm(w) if w.contains("`Neap` has no citation"))), "{f:?}");
        assert!(f.iter().any(|x| matches!(x, Finding::KeyTerm(w) if w.contains("not a `- **Term**"))), "{f:?}");
        assert!(f.iter().any(|x| matches!(x, Finding::StartHere(w) if w.contains("2 items"))), "{f:?}");
        assert!(f.iter().any(|x| matches!(x, Finding::StartHere(w) if w.contains("names no record"))), "{f:?}");
        assert!(f.iter().any(|x| matches!(x, Finding::UnknownSeeAlso(w) if w == "- Moon")), "{f:?}");
        assert!(f.iter().any(|x| matches!(x, Finding::KeyTerm(w) if w.contains("Open questions"))), "the daemon writes the open questions");
        let many: String = (0..13).map(|i| format!("- **T{i}**: d [c-01].\n")).collect();
        assert!(section_findings(&format!("## Key terms\n\n{many}"), &slugs(&[])).iter().any(|x| matches!(x, Finding::KeyTerm(w) if w.contains("13 terms"))));
        let parts = sections_json(ok);
        assert_eq!(parts["key_terms"][0]["term"], "Spring tide");
        assert_eq!(parts["key_terms"][0]["cites"], json!(["c-01"]));
        assert_eq!(parts["start_here"].as_array().unwrap().len(), 3);
        assert_eq!(parts["see_also"], json!(["moon"]));
        assert!(open_questions_section(&[]).is_none());
        let q = open_questions_section(&["Why [do] neap tides lag?".to_string()]).unwrap();
        assert!(q.starts_with("## Open questions") && q.contains("- Why do neap tides lag?"));
    }

    /// Writes a page with the three sections, citing the first claim of the brief and linking to `moon`.
    struct Sections(&'static str);
    impl PageWriter for Sections {
        fn write<'a>(&'a self, b: &'a str) -> WriterFuture<'a> {
            let id = b.lines().find_map(|l| l.strip_prefix("RECORD c-").map(|r| format!("c-{}", r.split(' ').next().unwrap_or("")))).unwrap_or_default();
            let topics_listed = b.contains("TOPIC moon | Moon");
            let d = format!("# Tides\n\nTides come twice a day [{id}].\n\n## Key terms\n\n- **Tide**: the rise and fall of the sea [{id}].\n\n## Start here\n\n1. [{id}] the main claim.\n2. [{id}] again, for its source.\n3. [{id}] once more.\n\n## See also\n\n- [Moon](page:{})\n", if topics_listed { self.0 } else { "no-topics-listed" });
            Box::pin(async move { Ok(d) })
        }
        fn rewrite<'a>(&'a self, d: &'a str, _f: &'a [String]) -> WriterFuture<'a> {
            let d = d.to_string();
            Box::pin(async move { Ok(d) })
        }
    }
    struct Pass;
    impl PageValidator for Pass {
        fn validate<'a>(&'a self, _q: &'a PageQuery) -> ValidatorFuture<'a> {
            Box::pin(async move { Ok(ValidatorReply { assertions: vec![Assertion { text: "x".into(), judgement: Judgement::Supported, record: None }], model: "m".into(), family: "f".into(), tokens: 10 }) })
        }
    }

    /// `specs/117` §2 items 5 and 6: the sections a topic page carries are checked, stored on its record, the open questions are the
    /// daemon's from the gaps, and the link table answers what links here (AC-5).
    #[test]
    fn a_topic_page_keeps_its_sections_open_questions_and_links() {
        let home = tempfile::tempdir().unwrap();
        let l = Library::in_home(home.path());
        let doc = home.path().join("t.md");
        std::fs::write(&doc, "# Tides\n\nThe tide comes in twice a day.\n").unwrap();
        let s = l.add_file(&doc, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
        l.add_claim("Tides come twice a day.", &ClaimOpts::default(), &[AnchorSpec { source_id: s, locator: Some("§1".into()), quote: "twice a day".into(), ..Default::default() }]).unwrap();
        l.topic_create("Moon", Origin::Operator, 1).unwrap();
        let e = l.topic_create("Tides", Origin::Operator, 1).unwrap();
        let cfg = PagesConfig::default();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let build = |w: &Sections, gaps: Vec<String>| {
            let side = l.topic_side(&e.slug).unwrap();
            let g = l.topic_gather_library(&side, None);
            let lib = Library::in_home(home.path()).with_topic_slugs(l.topic_slugs());
            rt.block_on(lib.topic_rebuild(TopicRequest { slug: &e.slug, now: 100, settings: &cfg, style: bib::Format::Plain, gathered: g, gaps }, w, &Pass, &TokenLedger::new(1_000_000), &|| Ok(()))).unwrap()
        };
        // A see-also slug that is no topic fails the check twice: the page falls back to the claims alone.
        let wrong = build(&Sections("sun"), vec![]);
        assert_eq!(wrong.outcome.as_deref(), Some("claims_only"), "{wrong:?}");
        let built = build(&Sections("moon"), vec!["Why do spring tides follow the moon?".into()]);
        assert_eq!(built.outcome.as_deref(), Some("pass"), "{built:?}");
        let side = l.topic_side(&e.slug).unwrap();
        assert_eq!(side["key_terms"][0]["term"], "Tide");
        assert_eq!(side["start_here"].as_array().unwrap().len(), 3);
        assert_eq!(side["see_also"], json!(["moon"]));
        assert_eq!(side["open_questions"], json!(["Why do spring tides follow the moon?"]));
        let md = std::fs::read_to_string(l.root().join(format!("pages/{}.md", e.slug))).unwrap();
        let (q, table) = (md.find("## Open questions").unwrap(), md.find("## Bibliography and citations").unwrap());
        assert!(q < table, "the open questions sit just above the bibliography");
        assert!(l.check_page_text(&md, Scope::External).findings.is_empty(), "the stored page still checks clean");
        // AC-5: `moon` is linked from `tides`.
        let links = l.topic_links("moon").unwrap();
        assert_eq!((links["links_here"].clone(), links["exists"].clone()), (json!(["tides"]), json!(true)));
        assert_eq!(l.topic_links("tides").unwrap()["links_out"], json!(["moon"]));
    }
}
