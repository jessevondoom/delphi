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
//! **Seeded** (`specs/139` §2.2; slice B). When no writer can be reached, the page is the topic's seed ([`Library::topic_seed_build`]):
//! the research's own words, assembled and checked by code, with outcome `seed` and its own banner. It calls no model and counts against
//! no budget. A topic with no span gets no page: it is `waiting for material`. The stub that said the library held nothing is no longer
//! written.
//!
//! **Expanded** (`specs/139` §2.3; slice C). The writer writes a topic page at the `long` depth from the seed page, given as
//! `DRAFT TO EXPAND`, and the spans, claims, sources and assets as records; the page is capped at `[library.pages] long_max_tokens`.
//! Every rebuild assembles the seed again from the stored documents: the page it replaces and its versions are read only for the change
//! line and to keep the version, never put in a brief. When the writer errors, its page fails the check, or the budget or the share
//! refuses the call, the seed is the page ([`Library::topic_keep_seed`]); the claims alone never are.
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
use super::page_check::Cite;
use super::page_validate::{CheckedPage, Outcome, PageValidator, PageVerdict, PageWriter};
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

/// The anchor a section's heading makes, as the page check makes it for a page's own `#section` links: what the app names
/// a section by (`specs/120` §4.2).
pub fn section_anchor(heading: &str) -> String {
    super::page_check::slug_of(heading)
}

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

/// The longest title a `subject` or the writer may give (`specs/139` §2.4 item 3).
pub const MAX_CONCEPT_TITLE: usize = 80;
/// The lead-ins a derived title loses, and that a `subject` or a writer's title may not begin with (`specs/139` §2.4 item 2).
pub const LEAD_INS: [&str; 11] = ["what is", "what are", "how do", "how does", "how to", "why", "should", "can", "is", "are", "does"];
/// Words that do not count as shared between a title and its question.
const STOP_WORDS: [&str; 24] = ["a", "an", "the", "of", "and", "or", "to", "in", "on", "for", "with", "by", "at", "from", "as", "it", "its", "be", "this", "that", "what", "how", "why", "which"];

/// Where a topic's title came from (`specs/139` §2.4 item 1), best first. A sidecar with no `title_source` is read as `operator`
/// when the operator made the topic and `question` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleSource {
    Operator,
    Subject,
    Writer,
    Derived,
    Question,
}

impl TitleSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TitleSource::Operator => "operator",
            TitleSource::Subject => "subject",
            TitleSource::Writer => "writer",
            TitleSource::Derived => "derived",
            TitleSource::Question => "question",
        }
    }

    /// The source a topic's record names.
    pub fn of(side: &Value) -> TitleSource {
        match side["title_source"].as_str() {
            Some("operator") => TitleSource::Operator,
            Some("subject") => TitleSource::Subject,
            Some("writer") => TitleSource::Writer,
            Some("derived") => TitleSource::Derived,
            Some("question") => TitleSource::Question,
            _ if side["origin"]["kind"] == "operator" => TitleSource::Operator,
            _ => TitleSource::Question,
        }
    }

    /// A title from a better source may replace this one: only a title made by code from the question (`derived`, `question`).
    pub fn replaceable(self) -> bool {
        matches!(self, TitleSource::Derived | TitleSource::Question)
    }
}

/// The words of `s` that count when a title and a question are compared: lowercase letters and digits, stop words left out.
fn content_words(s: &str) -> BTreeSet<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty() && !STOP_WORDS.contains(w)).map(String::from).collect()
}

/// The lead-in `s` begins with, as whole words and in any case, if any.
fn lead_in(s: &str) -> Option<&'static str> {
    let s = s.trim_start();
    LEAD_INS.iter().copied().filter(|l| s.len() >= l.len() && s.as_bytes()[..l.len()].eq_ignore_ascii_case(l.as_bytes()) && s.is_char_boundary(l.len()) && s[l.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric())).max_by_key(|l| l.len())
}

/// The title code makes from a question (`specs/139` §2.4 item 2, `derived`): the lead-ins of [`LEAD_INS`] at its start and its
/// closing question mark removed, the first letter made a capital. `None` when nothing is left, or what is left is still a question
/// or longer than [`MAX_CONCEPT_TITLE`]: the title is then the question.
pub fn derived_title(question: &str) -> Option<String> {
    let mut t = norm_ws(&question.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>());
    while let Some(l) = lead_in(&t) {
        t = t.trim_start()[l.len()..].trim_start().to_string();
    }
    let t = t.trim_end().trim_end_matches('?').trim_end().to_string();
    let mut chars = t.chars();
    let t: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    (!t.is_empty() && !t.contains('?') && t.chars().count() <= MAX_CONCEPT_TITLE).then_some(t)
}

/// Whether a title from `subject` or the writer is taken (`specs/139` §2.4 item 3): at most [`MAX_CONCEPT_TITLE`] characters, no
/// question mark and no control character, not beginning with a lead-in, and sharing at least one content word with the question or
/// one of the seed's headings. Returns the title, trimmed, or why it is refused.
pub fn accept_title(title: &str, question: &str, headings: &[String]) -> Result<String, &'static str> {
    if title.chars().any(char::is_control) {
        return Err("it holds a control character");
    }
    let t = norm_ws(title);
    if t.is_empty() {
        return Err("it is empty");
    }
    if t.chars().count() > MAX_CONCEPT_TITLE {
        return Err("it is longer than 80 characters");
    }
    if t.contains('?') {
        return Err("it holds a question mark");
    }
    if lead_in(&t).is_some() {
        return Err("it begins like a question");
    }
    let mut known = content_words(question);
    for h in headings {
        known.extend(content_words(h));
    }
    if content_words(&t).is_disjoint(&known) {
        return Err("it shares no word with the question or the seed's headings");
    }
    Ok(t)
}

/// The Markdown headings of a text (`#` to `######`), without their marks.
pub fn headings_of(md: &str) -> Vec<String> {
    md.lines().filter_map(|l| {
        let t = l.trim_start();
        let h = t.trim_start_matches('#');
        (t.starts_with('#') && t.len() - h.len() <= 6 && h.starts_with(' ')).then(|| h.trim().to_string())
    }).filter(|h| !h.is_empty()).collect()
}

/// The best title code can give a research topic's question (`specs/139` §2.4 item 2): its `subject` when [`accept_title`] takes it
/// (against the question and `headings`, the answer's), else the derived title, else the question.
pub fn research_title(question: &str, subject: Option<&str>, headings: &[String]) -> (String, TitleSource) {
    if let Some(s) = subject.and_then(|s| accept_title(s, question, headings).ok()) {
        return (s, TitleSource::Subject);
    }
    match derived_title(question) {
        Some(d) => (d, TitleSource::Derived),
        None => (clip_title(question), TitleSource::Question),
    }
}

/// What a topic's brief adds when the writer may name the page (`specs/139` §2.4 item 2, `writer`).
pub const TITLE_HINT: &str = "\nIn that last comment you may add `title=<the concept this page is about, as a short name: at most 80 characters, not a question>`; the daemon takes it only if it shares a word with the question.\n";

/// Whether the writer's `title=` is taken for this topic: its title was made by code from the question, and this is its first written
/// page (none before, or only a seed or the claims alone).
fn may_take_title(side: &Value) -> bool {
    let first = side["written"] != true && matches!(side["outcome"].as_str(), None | Some("seed") | Some("claims_only"));
    first && TitleSource::of(side).replaceable()
}

/// The headings of a research topic's answer: what a `subject` may share a word with besides the question.
fn research_headings(t: &crate::research_record::Topic) -> Vec<String> {
    headings_of(&std::fs::read_to_string(t.answer_path()).unwrap_or_default())
}

/// One line of `delphi pages retitle` (`specs/139` §2.4 item 4): a topic whose title code would now make differently.
#[derive(Debug, Clone, PartialEq)]
pub struct Retitle {
    pub slug: String,
    pub new_slug: String,
    pub title: String,
    pub new_title: String,
    pub source: TitleSource,
    /// Another topic has `new_slug`, or would take it: printed, and `--apply` leaves it as it is.
    pub collision: bool,
}

impl Retitle {
    pub fn to_json(&self) -> Value {
        json!({"slug": self.slug, "new_slug": self.new_slug, "title": self.title, "new_title": self.new_title, "title_source": self.source.as_str(), "collision": self.collision})
    }
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
    /// Never built, and its research gives its seed nothing: listed as `waiting for material` (`specs/139` §2.2 item 4).
    pub waiting: bool,
    /// The record as stored, for the fields later slices add.
    pub side: Value,
}

impl TopicEntry {
    /// Set `waiting` from the due check just made: never built, the workspace known, and not due.
    fn with_waiting(mut self, ctx: Option<&DueCtx<'_>>) -> TopicEntry {
        self.waiting = self.built.is_none() && self.due.is_none() && ctx.is_some_and(|c| c.ws.is_some());
        self
    }

    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "slug": self.slug, "title": self.title, "origin": self.origin, "research_topics": self.research_topics, "created": self.created,
            "built": self.built, "version": self.version, "change": self.change, "outcome": self.outcome, "valid_until": self.valid_until,
            "valid_until_date": self.valid_until.map(page_meta::ymd), "cited": self.cited, "due": self.due, "status": "generated",
            "waiting": self.waiting,
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
    /// The topic's seed (`specs/139` §2.1), assembled again by the caller for every rebuild (`topic_seed::assemble`): the writer's draft,
    /// and the page when the writer's does not land (§2.3 items 5 and 6).
    pub seed: crate::topic_seed::Seed,
    /// The held-text predicate the seed page is checked with (`specs/142` §2.5 item 4; [`crate::topic_seed::NoHeldText`] until it exists).
    pub held: &'a dyn crate::topic_seed::HeldText,
    /// A rebuild the desktop app asked for can be stopped by revoking its key (`specs/120` §2 item 8 as amended by `specs/129`
    /// §10.2). The flag is read where the rebuild asks its gate, before the writer's model call, and once more before the page file is
    /// written; past that last check the rebuild runs to its end. `None`: nothing can stop it.
    pub cancel: Option<&'a Cancel>,
}

/// What a seed page is built from (`specs/139` §2.2): no writer, no validator, no budget.
pub struct SeedRequest<'a> {
    pub slug: &'a str,
    pub now: u64,
    pub settings: &'a PagesConfig,
    pub style: bib::Format,
    pub seed: &'a crate::topic_seed::Seed,
    pub held: &'a dyn crate::topic_seed::HeldText,
    pub gaps: &'a [String],
    pub cancel: Option<&'a Cancel>,
}

/// What a rebuild says for a topic whose seed has no span (`specs/139` §2.2 item 4). No page is written.
pub fn waiting_message() -> String {
    format!("{}: no research answer, finding or note gives this topic's seed anything yet, so no page is written", crate::topic_seed::WAITING)
}

/// What [`Library::topic_rebuild`] says when its [`Cancel`] stopped it.
pub const CANCELLED: &str = "cancelled: the key that asked for it was revoked";

/// Why a seed page that nothing else made due is due: it waits for the writer's expansion (`specs/139` §2.5 item 1, "seed now,
/// expand at idle"). Only a writer's page ends it; a rebuild with no writer, or with nothing for one, writes no new seed version.
pub const SEED_EXPANSION: &str = "a seed page, due for the writer's expansion";

/// The record's mark on a topic whose writer's expansion is queued for the next idle time (`specs/139` §2.5 item 1): the time it was
/// queued. The idle rebuild takes queued topics first, the earliest first; a writer's page, or a seed the writer could not expand,
/// clears it.
pub const EXPANSION_QUEUED: &str = "expansion_queued";
/// A topic record's old slugs (`specs/139` §2.4, the operator's ruling of 2026-11-26): a topic re-slugged by `rename` or `retitle`
/// keeps its old slugs here, and a `page:<old slug>` link resolves to it. Other pages' text is not rewritten.
pub const ALIASES: &str = "aliases";

/// The start of what [`Library::topic_rebuild`] says when the writer's page did not land: the seed is the page (`specs/139` §2.3
/// item 5), and no page of the claims alone takes its place. The seed page is in place, with outcome `seed` and the reason.
pub const SEED_STANDS: &str = "the seed stands";

/// In a [`SEED_STANDS`] error: the build was refused before the writer was asked (`per_day`, the share, a gate), so a queued expansion
/// stays queued.
pub const NOT_ADMITTED: &str = "the build was not admitted";

/// The start of what [`Library::topic_rebuild`] says when the writer's page did not land on a topic whose page the writer wrote and
/// validated before: that page stands, and the topic stays due (a builder decision of `specs/139` slice C: a seed never replaces a
/// validated page).
pub const WRITTEN_STANDS: &str = "the written page stands";

/// Whether a piece of work may still be stopped, read and set without a lock. It starts going; a revoke stops it ([`Cancel::stop`])
/// unless the work has already passed its last check ([`Cancel::commit`]), in which case it runs to its end and the revoke is noted
/// ([`Cancel::revoked_late`]).
#[derive(Debug, Default)]
pub struct Cancel(std::sync::atomic::AtomicU8);

/// What [`Cancel::stop`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// The work had not passed its last check: it stops there.
    Now,
    /// The work had passed its last check and runs to its end.
    TooLate,
    /// It was stopped already.
    Already,
}

const GOING: u8 = 0;
const STOPPED: u8 = 1;
const COMMITTED: u8 = 2;
const COMMITTED_REVOKED: u8 = 3;

impl Cancel {
    /// Stop the work, if it has not passed its last check.
    pub fn stop(&self) -> Stopped {
        use std::sync::atomic::Ordering::SeqCst;
        match self.0.compare_exchange(GOING, STOPPED, SeqCst, SeqCst) {
            Ok(_) => Stopped::Now,
            Err(COMMITTED) => match self.0.compare_exchange(COMMITTED, COMMITTED_REVOKED, SeqCst, SeqCst) {
                Ok(_) => Stopped::TooLate,
                Err(_) => Stopped::Already,
            },
            Err(_) => Stopped::Already,
        }
    }

    /// Has it been stopped (before its last check)?
    pub fn stopped(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst) == STOPPED
    }

    /// The work's last check: `true` when it goes on to its end (nothing can stop it after this), `false` when it was stopped.
    pub fn commit(&self) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        match self.0.compare_exchange(GOING, COMMITTED, SeqCst, SeqCst) {
            Ok(_) => true,
            Err(v) => v != STOPPED,
        }
    }

    /// Was a revoke asked for after the work passed its last check?
    pub fn revoked_late(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst) == COMMITTED_REVOKED
    }
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

/// The brief for a topic's page: a search page's brief at the `long` depth (layout, citation rules, records), the three sections of
/// `specs/117` §2 item 5, the topics `## See also` may name, and the seed page as `DRAFT TO EXPAND` (`specs/139` §2.3 items 2 and 3).
/// `max_tokens` is `[library.pages] long_max_tokens`. Nothing of the page it replaces is in it (§2.3 item 6): `draft` is the seed page code
/// just rendered.
pub fn topic_brief(title: &str, g: &Gathered, max_tokens: u64, topics: &[(String, String)], draft: &str) -> String {
    let mut s = super::page_search::brief_for(title, g, max_tokens, Depth::Long);
    s.push_str("\nAfter your last section, and only as far as the records support them, add these sections in this order:\n");
    s.push_str(&format!("- `## {}`: at most {} lines, each `- **Term**: one-line definition [marker]`, the marker citing the record the definition comes from.\n", KEY_TERMS, MAX_KEY_TERMS));
    s.push_str(&format!("- `## {}`: a numbered list of three to five records to read first, in the order to read them, each line naming one record by its marker and saying in a few words why it comes first.\n", START_HERE_HEADING));
    s.push_str(&format!("- `## {}`: lines `- [Title](page:slug)` naming only topics from the TOPICS list below; leave the section out when none is related.\n", SEE_ALSO));
    s.push_str(&format!("Do not write a `## {}` section: the daemon adds it.\n\nTOPICS (other topic pages that exist; data):\n", OPEN_QUESTIONS));
    for (slug, t) in topics.iter().take(60) {
        s.push_str(&format!("TOPIC {} | {}\n", slug, t.replace('\n', " ").chars().take(120).collect::<String>()));
    }
    s.push_str(&super::page_search::draft_to_expand(draft));
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

    /// The sidecar of the page a reader is shown (`specs/141` §2.2, read at the sink): the current one, or the record an earlier
    /// version was kept with. `None` when there is none, which a sink reads as every mark.
    pub fn topic_side_shown(&self, slug: &str, version: Option<&str>) -> Option<Value> {
        let slug = self.topic_resolve(slug)?;
        match version {
            None => self.topic_side(&slug),
            Some(v) if valid_stamp(v) => serde_json::from_slice(&std::fs::read(self.topic_versions_dir(&slug).join(format!("{}.json", v))).ok()?).ok(),
            Some(_) => None,
        }
    }

    fn write_topic_side(&self, slug: &str, v: &Value) -> Result<(), String> {
        self.ensure().map_err(|e| e.to_string())?;
        self.write_json(&self.topic_side_path(slug), v)
    }

    /// Make a topic: its record, no page yet. A topic that already has `slug` is returned as it is (making it again changes
    /// nothing), except that a research topic named again is added to the ones it follows.
    pub fn topic_create(&self, title: &str, origin: Origin, now: u64) -> Result<TopicEntry, String> {
        let source = if origin == Origin::Operator { TitleSource::Operator } else { TitleSource::Question };
        self.topic_create_titled(title, source, title, origin, now)
    }

    /// [`Library::topic_create`] with a title that is not the question (`specs/139` §2.4): `query` is kept as the words a rebuild
    /// gathers with, and `source` says where the title came from.
    pub fn topic_create_titled(&self, title: &str, source: TitleSource, query: &str, origin: Origin, now: u64) -> Result<TopicEntry, String> {
        let title = clip_title(title);
        if title.is_empty() {
            return Err("a topic needs a title".into());
        }
        let query = clip_title(query);
        let query = if query.is_empty() { title.clone() } else { query };
        let mut slug = slug_for(&title);
        // A slug that is an old slug of another topic is not taken: the new topic gets the next free one, `<slug>-2` and on, so a
        // `page:` link to the old slug still opens the topic it was written for.
        if self.topic_side(&slug).is_none() && self.alias_owner(&slug).is_some() {
            slug = self.free_slug(&slug)?;
        }
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
        let side = json!({"kind": KIND, "status": "generated", "slug": slug, "title": title, "title_source": source.as_str(), "query": query, "origin": origin.to_json(), "research_topics": research, "created": now, "built": null, "version": 0, "material": [], "cited": []});
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
            // The title is a concept where code can make one (`specs/139` §2.4): the host's `subject`, else the question without its
            // lead-in; the question stays the topic's `query`.
            let (title, source) = research_title(q, rec["subject"].as_str(), &research_headings(&t));
            if let Ok(e) = self.topic_create_titled(&title, source, q, Origin::Research(t.id.clone()), now) {
                made.push(e.slug);
            }
        }
        made
    }

    /// `delphi pages rename <slug> <title>` (`specs/139` §2.4 item 2): the operator's title, which always wins. A topic with no
    /// writer's page (none yet, or seed pages only) takes the slug of its new title, and its old slug becomes an alias; one with a
    /// writer's page keeps its slug for good (§2.4 item 4, as the operator ruled on 2026-11-26).
    pub fn topic_rename(&self, slug: &str, title: &str) -> Result<TopicEntry, String> {
        self.topic_set_title(slug, title, TitleSource::Operator)
    }

    /// Give `slug` the title `title` from `source`, and the slug of that title when the topic has no writer's page. Refused when that
    /// slug is another topic's, or an old slug of another topic. A re-slugged topic keeps its old slug as an alias, and its seed page,
    /// its versions and its row of the link table move with it.
    fn topic_set_title(&self, slug: &str, title: &str, source: TitleSource) -> Result<TopicEntry, String> {
        let mut side = self.topic_side(slug).ok_or_else(|| format!("no topic `{}`", slug))?;
        let title = clip_title(title);
        if title.is_empty() {
            return Err("a topic needs a title".into());
        }
        let new_slug = if self.topic_has_writers_page(slug, &side) { slug.to_string() } else { slug_for(&title) };
        if new_slug != slug && self.topic_side_path(&new_slug).exists() {
            return Err(format!("the topic `{}` already exists, so `{}` keeps its slug and its title", new_slug, slug));
        }
        if let Some(owner) = self.alias_owner(&new_slug).filter(|o| o != slug) {
            return Err(format!("`{}` is an old slug of the topic `{}`, so `{}` keeps its slug and its title", new_slug, owner, slug));
        }
        let old_title = side["title"].as_str().unwrap_or(slug).to_string();
        side["title"] = json!(title);
        side["title_source"] = json!(source.as_str());
        side["slug"] = json!(new_slug);
        if new_slug != slug {
            let mut aliases = aliases_of(&side);
            aliases.retain(|a| *a != new_slug);
            if !aliases.iter().any(|a| a == slug) {
                aliases.push(slug.to_string());
            }
            side[ALIASES] = json!(aliases);
        }
        self.write_topic_side(&new_slug, &side)?;
        // A seed page is code's: its frontmatter and its heading take the new slug and title. A writer's page is never re-slugged.
        let page = self.topic_file(slug);
        if let Ok(md) = std::fs::read_to_string(&page) {
            let seed = side["outcome"] == Outcome::Seed.as_str();
            let md = if seed && (new_slug != slug || old_title != title) { retitle_seed_text(&md, &new_slug, &title) } else { md };
            std::fs::write(self.topic_file(&new_slug), md).map_err(|e| e.to_string())?;
            if new_slug != slug {
                let _ = std::fs::remove_file(&page);
            }
        }
        if new_slug != slug {
            let (from, to) = (self.topic_versions_dir(slug), self.topic_versions_dir(&new_slug));
            if from.is_dir() {
                std::fs::rename(&from, &to).map_err(|e| e.to_string())?;
            }
            let mut t = self.link_table();
            if let Some(out) = t.remove(slug) {
                t.insert(new_slug.clone(), out);
                let _ = self.write_json(&self.links_path(), &json!(t));
            }
            let _ = std::fs::remove_file(self.topic_side_path(slug));
        }
        self.log("topic_retitled", json!({"slug": slug, "new_slug": new_slug, "title_source": source.as_str()}));
        Ok(self.topic_entry(&new_slug, &side, None))
    }

    /// Has the topic a writer's page, now or among its versions: one of any outcome but `seed`? Such a topic keeps its slug for good.
    /// A version whose record cannot be read counts as one.
    fn topic_has_writers_page(&self, slug: &str, side: &Value) -> bool {
        let writers = |v: &Value| v["built"].as_u64().is_some() && v["outcome"].as_str() != Some(Outcome::Seed.as_str());
        if writers(side) || side["written"] == true {
            return true;
        }
        let Ok(rd) = std::fs::read_dir(self.topic_versions_dir(slug)) else { return false };
        rd.flatten()
            .filter(|e| e.file_name().to_str().is_some_and(|n| n.ends_with(".json")))
            .any(|e| std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).is_none_or(|v| writers(&v)))
    }

    /// The topic whose old slugs hold `slug`, if one does.
    fn alias_owner(&self, slug: &str) -> Option<String> {
        if !valid_slug(slug) {
            return None;
        }
        self.topic_sides().into_iter().find(|(_, side)| aliases_of(side).iter().any(|a| a == slug)).map(|(s, _)| s)
    }

    /// The first of `<base>-2`, `<base>-3`, ... that is no topic's slug and no topic's old slug.
    fn free_slug(&self, base: &str) -> Result<String, String> {
        (2..1000)
            .map(|n| {
                let tail = format!("-{}", n);
                format!("{}{}", base.chars().take(MAX_SLUG - tail.len()).collect::<String>().trim_end_matches('-'), tail)
            })
            .find(|s| valid_slug(s) && self.topic_side(s).is_none() && self.alias_owner(s).is_none())
            .ok_or_else(|| format!("no free slug for `{}`", base))
    }

    /// The live slug `slug` names: its own, when it is a topic, else the topic it is an old slug of. A live topic's own slug always wins
    /// over an alias. Reads only.
    pub fn topic_resolve(&self, slug: &str) -> Option<String> {
        if self.topic_side(slug).is_some() {
            return Some(slug.to_string());
        }
        self.alias_owner(slug)
    }

    /// What a `page:` link may name: every topic's slug and every old slug (an alias). The page check is given these.
    pub fn topic_link_names(&self) -> BTreeSet<String> {
        self.topic_sides().into_iter().flat_map(|(s, side)| std::iter::once(s).chain(aliases_of(&side))).collect()
    }

    /// `delphi pages retitle [--apply]` (`specs/139` §2.4 item 4): for each topic whose title code made from its question, the title
    /// code would give it now (the research's accepted `subject`, else the derived title), and the slug it would take. A topic with a
    /// page keeps its slug. Two topics that would take one slug, or a slug another topic has, are a collision and are left as they are.
    /// With `apply` the others are written; without it nothing is. Running it again lists nothing.
    pub fn topic_retitle(&self, ws: &Path, apply: bool) -> Vec<Retitle> {
        let sides = self.topic_sides();
        let mut plan: Vec<Retitle> = Vec::new();
        for (slug, side) in &sides {
            if !TitleSource::of(side).replaceable() {
                continue;
            }
            let query = side["query"].as_str().or(side["title"].as_str()).unwrap_or(slug);
            let research = side["research_topics"].as_array().into_iter().flatten().filter_map(Value::as_str).filter_map(|id| crate::research_record::find(ws, id)).filter_map(|t| t.read().ok().map(|r| (t, r)));
            let (mut subject, mut headings) = (None, Vec::new());
            for (t, rec) in research {
                if let (None, Some(s)) = (&subject, rec["subject"].as_str()) {
                    subject = Some(s.to_string());
                }
                headings.extend(research_headings(&t));
            }
            let (title, source) = research_title(query, subject.as_deref(), &headings);
            let current = side["title"].as_str().unwrap_or(slug);
            if title == current {
                continue;
            }
            let new_slug = if self.topic_has_writers_page(slug, side) { slug.clone() } else { slug_for(&title) };
            plan.push(Retitle { slug: slug.clone(), new_slug, title: current.to_string(), new_title: title, source, collision: false });
        }
        let existing: BTreeSet<&str> = sides.iter().map(|(s, _)| s.as_str()).collect();
        // An old slug of another topic is taken too; a topic's own old slug is its to take back.
        let alias_of = |a: &str| sides.iter().find(|(_, side)| aliases_of(side).iter().any(|x| x == a)).map(|(s, _)| s.clone());
        let taken: Vec<String> = plan.iter().map(|p| p.new_slug.clone()).collect();
        for (i, p) in plan.iter_mut().enumerate() {
            let aliased = alias_of(&p.new_slug).is_some_and(|o| o != p.slug);
            p.collision = p.new_slug != p.slug && (existing.contains(p.new_slug.as_str()) || aliased || taken.iter().enumerate().any(|(j, s)| j != i && *s == p.new_slug));
        }
        if apply {
            for p in plan.iter().filter(|p| !p.collision) {
                if let Err(e) = self.topic_set_title(&p.slug, &p.new_title, p.source) {
                    self.log("topic_retitle_failed", json!({"slug": p.slug, "error": e}));
                }
            }
        }
        plan
    }

    /// Does a topic page that follows the research topic `research_topic` have a section whose anchor is `anchor`
    /// (`specs/120` §4.2: the daemon looks up what an id refers to)? The anchor is the page check's own
    /// ([`crate::library_page_check::slug_of`]). Reads only.
    pub fn topic_has_section(&self, research_topic: &str, anchor: &str) -> bool {
        self.topic_sides().into_iter().filter(|(_, side)| side["research_topics"].as_array().into_iter().flatten().any(|t| t == research_topic)).any(|(slug, _)| {
            let md = std::fs::read_to_string(self.topic_file(&slug)).unwrap_or_default();
            page_meta::strip(&md).lines().filter_map(|l| l.strip_prefix("## ").or_else(|| l.strip_prefix("### "))).any(|h| section_anchor(h.trim()) == anchor)
        })
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
            waiting: false,
            side: side.clone(),
        }
        .with_waiting(due)
    }

    /// Every topic, by slug, with why each is due. Reads only.
    pub fn topic_pages(&self, ctx: &DueCtx<'_>) -> Vec<TopicEntry> {
        self.topic_sides().iter().map(|(s, v)| self.topic_entry(s, v, Some(ctx))).collect()
    }

    /// One topic and its page (the current one, or the version `version` names), the frontmatter left off. A topic not yet
    /// built has an empty page. Reads only.
    pub fn topic_get(&self, slug: &str, version: Option<&str>, ctx: &DueCtx<'_>) -> Option<(TopicEntry, String)> {
        let slug = &self.topic_resolve(slug)?;
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
        let slug = &self.topic_resolve(slug)?;
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
    ///
    /// A topic never built is due unless, with the workspace known, its research gives its seed nothing: it is then waiting for material
    /// (`specs/139` §2.2 item 4), and stays so until a research run gives it some. A seed page is due when a span it was built from
    /// changed or went, or a linked row of its seed now offers more than it did (`specs/139` §4 slice B: the due check reads span hashes).
    pub fn topic_due(&self, side: &Value, ctx: &DueCtx<'_>) -> Option<String> {
        if side["built"].as_u64().is_none() {
            if let Some(ws) = ctx.ws {
                if crate::topic_seed::assemble_linked(self, ws, side).is_empty() {
                    return None;
                }
            }
            return Some("not built yet".into());
        }
        if let (Some(stored), Some(ws)) = (side["seed"].as_array(), ctx.ws) {
            use crate::topic_seed::Row;
            let now = crate::topic_seed::assemble_linked(self, ws, side);
            let current: BTreeMap<&str, &str> = now.spans.iter().map(|s| (s.label.as_str(), s.hash.as_str())).collect();
            for s in stored.iter().filter(|s| s["row"].as_u64() != Some(Row::Search.number() as u64)) {
                let label = s["label"].as_str().unwrap_or("");
                match current.get(label) {
                    None => return Some(format!("`{}` is gone from the research its seed was assembled from", label)),
                    Some(h) if *h != s["hash"].as_str().unwrap_or("") => return Some(format!("`{}` changed", label)),
                    _ => {}
                }
            }
            for r in Row::ALL.into_iter().filter(|r| *r != Row::Search) {
                let was = side["seed_offered"][r.number() - 1].as_u64().unwrap_or(0) as usize;
                if now.offered[r.number() - 1] > was {
                    return Some(format!("its seed has new material ({})", r.name()));
                }
            }
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
        // A label-bearing fact of its material changed (`specs/141` §2.1 item 4, AC-3): the label is recomputed at the next build.
        if let Some(ws) = ctx.ws {
            if crate::label::stale(self, Some(ws), side) {
                return Some("a label of its material changed".into());
            }
        }
        if side["valid_until"].as_u64().is_some_and(|u| ctx.now > u) {
            return Some(format!("its lifetime ended on {}", page_meta::ymd(side["valid_until"].as_u64().unwrap_or(0))));
        }
        if side["outcome"].as_str() == Some(Outcome::Seed.as_str()) {
            return Some(SEED_EXPANSION.into());
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

    /// Rebuild a topic's page (§2 item 2, as `specs/139` §2.3 amends it): the writer expands the topic's seed into a `long` page from the
    /// seed's spans and `r.gathered`, the page passes the same check and validator as a search page, the page it replaces becomes a
    /// version, and the record says what changed. The knowledge-base records of `r.gathered` and the seed's spans are cited only if this
    /// library was given the same ones ([`Library::with_records`]; the seed's are added here). `gate` is asked before any model is called;
    /// a page counts against `[library.pages] per_day`. A topic with no span gets no page; a writer's page that does not land leaves the
    /// seed as the page ([`Library::topic_keep_seed`]).
    pub async fn topic_rebuild(&self, r: TopicRequest<'_>, writer: &dyn PageWriter, validator: &dyn PageValidator, ledger: &TokenLedger, gate: super::page_search::Gate<'_>) -> Result<TopicEntry, String> {
        if !r.settings.enabled {
            return Err("generated pages are off ([library.pages] enabled = false)".into());
        }
        let side = self.topic_side(r.slug).ok_or_else(|| format!("no topic `{}`", r.slug))?;
        let title = side["title"].as_str().unwrap_or(r.slug).to_string();
        let seed_req = SeedRequest { slug: r.slug, now: r.now, settings: r.settings, style: r.style, seed: &r.seed, held: r.held, gaps: &r.gaps, cancel: r.cancel };
        // The rebuild starts from the seed, assembled again from the stored documents (`specs/139` §2.3 item 6). No span, or no block that
        // passes the seed check: no page, and no writer is asked (§2.2 items 3 and 4).
        let Ok(draft) = crate::topic_seed::seed_page(&title, &r.seed, r.held) else {
            return self.topic_seed_build(seed_req);
        };
        // The writer's records: the seed's spans first, then what the search found that the seed does not hold, and the seed's claims.
        let g = self.expansion_material(&r.seed, r.gathered);
        let lib = self.with_more_records(&g.records);
        if self.pages_today(r.now) >= r.settings.per_day {
            return self.topic_keep_seed(&seed_req, &side, &format!("{NOT_ADMITTED}: today's page budget ({} pages) is spent; the topic stays due", r.settings.per_day));
        }
        // A rebuild that was stopped asks no model (`specs/129` §10.2): the flag is read where the gate is asked.
        if r.cancel.is_some_and(Cancel::stopped) {
            return Err(CANCELLED.into());
        }
        if let Err(e) = gate() {
            return self.topic_keep_seed(&seed_req, &side, &format!("{NOT_ADMITTED}: {e}"));
        }
        self.count_page(r.now);
        let topics: Vec<(String, String)> = self.topic_sides().into_iter().filter(|(s, _)| s != r.slug).map(|(s, v)| (s, v["title"].as_str().unwrap_or("").to_string())).collect();
        let mut brief = topic_brief(&title, &g, r.settings.long_max_tokens, &topics, &draft.body);
        if may_take_title(&side) {
            brief.push_str(TITLE_HINT);
        }
        let (checked, proposal) = match writer.write(&brief).await {
            Ok(d) => {
                // The closing comment is read from this first draft only (§2.3 item 7): a rewrite's is taken off and ignored.
                let (body, proposal) = page_meta::extract(&d);
                match lib.build_page_checked(&title, &clean_draft(&body, r.settings.long_max_tokens), r.style, SCOPE, validator, writer, ledger).await {
                    Ok(c) if c.verdict.outcome != Outcome::ClaimsOnly => (c, proposal),
                    Ok(c) => return self.topic_keep_seed(&seed_req, &side, &format!("the writer's page did not pass its check: {}", c.verdict.reason.as_deref().unwrap_or("no reason given"))),
                    Err(e) => return self.topic_keep_seed(&seed_req, &side, &format!("the writer's page could not be checked: {e}")),
                }
            }
            Err(e) => return self.topic_keep_seed(&seed_req, &side, &format!("the writer could not expand it: {e}")),
        };
        // The last check: a rebuild stopped by now leaves the page as it was; past here it is written whatever is asked.
        if r.cancel.is_some_and(|c| !c.commit()) {
            return Err(CANCELLED.into());
        }
        lib.topic_store(r.slug, side, &g, checked, proposal.as_ref(), r.now, r.settings, &r.gaps, None)
    }

    /// The writer's material for a topic page (`specs/139` §2.3 item 3): the seed's spans as records, in seed order, then the records of
    /// `gathered` the seed does not hold (one label is one passage), and the live claims among the seed's spans beside `gathered`'s.
    pub fn expansion_material(&self, seed: &crate::topic_seed::Seed, gathered: Gathered) -> Gathered {
        let mut g = gathered;
        let mut records = seed.records();
        for r in std::mem::take(&mut g.records) {
            if !records.iter().any(|x| x.label == r.label) {
                records.push(r);
            }
        }
        g.records = records;
        for s in seed.spans.iter().filter(|s| s.row == crate::topic_seed::Row::Claim) {
            if !g.claims.iter().any(|c| c["id"] == s.label.as_str()) {
                g.claims.extend(self.claim(&s.label));
            }
        }
        g
    }

    /// This library, resolving `records` besides its own (the page check of a topic page resolves the seed's spans as records).
    fn with_more_records(&self, records: &[super::pages::KbRecord]) -> Library {
        let mut all = self.records.clone();
        for r in records {
            if !all.iter().any(|x| x.label == r.label) {
                all.push(r.clone());
            }
        }
        Library { root: self.root.clone(), support: self.support.clone(), retractions: self.retractions.clone(), records: all, topic_slugs: self.topic_slugs.clone(), text_cache: self.text_cache, preview: self.preview.clone(), fixed_now: self.fixed_now.clone(), workspace: self.workspace.clone() }
    }

    /// A failure keeps the seed (`specs/139` §2.3 item 5): the writer errored, its page failed the check, or `per_day`, the share or a gate
    /// refused the call. The seed page is stored with outcome `seed` and `why` as its reason, or, when the page is already this seed (the
    /// same spans), stays and only its reason is updated. A page the writer wrote and the validator passed is not replaced (a builder
    /// decision: a seed never replaces a validated page); it stands and the topic stays due. Always `Err`, saying which page stands.
    pub fn topic_keep_seed(&self, r: &SeedRequest<'_>, side: &Value, why: &str) -> Result<TopicEntry, String> {
        self.log("topic_seed_kept", json!({"slug": r.slug, "reason": why}));
        let outcome = side["outcome"].as_str();
        if matches!(outcome, Some(o) if o == Outcome::Pass.as_str() || o == Outcome::PassContested.as_str()) {
            return Err(format!("{WRITTEN_STANDS}: {why}"));
        }
        let same: Vec<Value> = r.seed.spans.iter().map(|x| x.to_json()).collect();
        if outcome == Some(Outcome::Seed.as_str()) && side["seed"].as_array() == Some(&same) && self.topic_file(r.slug).is_file() {
            let mut s = side.clone();
            s["reason"] = json!(why);
            self.write_topic_side(r.slug, &s)?;
            return Err(format!("{SEED_STANDS}: {why}"));
        }
        self.seed_build_with(r, why)?;
        Err(format!("{SEED_STANDS}: {why}"))
    }

    /// Write a topic's seed page (`specs/139` §2.2): rendered and checked by code (`topic_seed::seed_page`), outcome `seed`, its own
    /// banner, the bibliography the daemon writes. No model is called, no token spent, and it does not count against `per_day`. A seed
    /// with no span writes no page (`waiting for material`); a seed whose every block fails its check writes none either and says
    /// `check_failed`. The page it replaces becomes a version, as any rebuild's does.
    pub fn topic_seed_build(&self, r: SeedRequest<'_>) -> Result<TopicEntry, String> {
        self.seed_build_with(&r, "a seed draft: no writer has expanded it yet")
    }

    /// [`Library::topic_seed_build`] with the reason the seed page carries.
    fn seed_build_with(&self, r: &SeedRequest<'_>, reason: &str) -> Result<TopicEntry, String> {
        if !r.settings.enabled {
            return Err("generated pages are off ([library.pages] enabled = false)".into());
        }
        let side = self.topic_side(r.slug).ok_or_else(|| format!("no topic `{}`", r.slug))?;
        if r.seed.is_empty() {
            self.log("topic_waiting", json!({"slug": r.slug}));
            return Err(waiting_message());
        }
        let title = side["title"].as_str().unwrap_or(r.slug).to_string();
        let page = match crate::topic_seed::seed_page(&title, r.seed, r.held) {
            Ok(p) => p,
            Err(dropped) => {
                // `check_failed` (`specs/143` §2.7 names the notice; until it exists the cause is the library's log and this error).
                self.log("topic_seed_check_failed", json!({"slug": r.slug, "dropped": dropped.iter().map(|(l, why)| json!({"label": l, "reason": why})).collect::<Vec<_>>()}));
                return Err(format!("check_failed: every block of the seed failed its check ({}); the topic is {}", dropped.first().map(|(l, why)| format!("`{}`: {}", l, why)).unwrap_or_default(), crate::topic_seed::WAITING));
            }
        };
        if r.cancel.is_some_and(|c| c.stopped() || !c.commit()) {
            return Err(CANCELLED.into());
        }
        // The bibliography resolves the seed's spans as records, and names the sources read that gave no span.
        let lib = Library { root: self.root.clone(), support: self.support.clone(), retractions: self.retractions.clone(), records: r.seed.records(), topic_slugs: None, text_cache: self.text_cache, preview: None, fixed_now: None, workspace: self.workspace.clone() };
        let mut cited: Vec<Cite> = Vec::new();
        for c in markers(&page.body).into_iter().chain(r.seed.named_sources.iter().map(|id| Cite { id: id.clone(), source: true, shown: None })) {
            if !cited.iter().any(|x| x.label() == c.label()) {
                cited.push(c);
            }
        }
        let markdown = format!("{}\n\n{}\n{}", crate::topic_seed::SEED_BANNER, page.body, lib.bibliography_table(&cited, r.style, SCOPE));
        let at = delphi_core::time::now_rfc3339().chars().take(10).collect::<String>();
        let reason = reason.to_string();
        let verdict = PageVerdict { outcome: Outcome::Seed, reason: Some(reason), model: String::new(), family: String::new(), tokens: 0, rewrites: 0, assertions: Vec::new(), figure_failures: Vec::new(), at };
        let hash = crate::topic_gather::text_hash(&markdown);
        let checked = CheckedPage { page: super::pages::Page { topic: title, validated: Vec::new(), noted: Vec::new(), sources: r.seed.named_sources.clone(), withheld: 0, hash, markdown, cached: false, flags: Vec::new(), figure_failures: Vec::new() }, verdict };
        // The material a due check compares by the index: the search rows' records and the claims, as for a written page.
        let used: BTreeSet<&str> = page.cited.iter().map(String::as_str).collect();
        let mut g = Gathered::default();
        for s in r.seed.spans.iter().filter(|s| used.contains(s.label.as_str())) {
            match s.row {
                crate::topic_seed::Row::Search => g.records.extend(s.to_record()),
                crate::topic_seed::Row::Claim => g.claims.extend(self.claim(&s.label)),
                _ => {}
            }
        }
        let dropped: Vec<Value> = page.dropped.iter().map(|(l, why)| json!({"label": l, "reason": why})).collect();
        self.topic_store(r.slug, side, &g, checked, None, r.now, r.settings, r.gaps, Some((r.seed, &dropped)))
    }

    /// The topic that follows the research topic `research_topic`, if one does. Reads only.
    pub fn topic_for_research(&self, research_topic: &str) -> Option<TopicEntry> {
        self.topic_sides().into_iter().find(|(_, side)| side["research_topics"].as_array().into_iter().flatten().any(|t| t == research_topic)).map(|(slug, side)| self.topic_entry(&slug, &side, None))
    }

    /// Queue the writer's expansion of `slug` for the next idle time (`specs/139` §2.5 item 1): it is marked with the time, and the idle
    /// rebuild takes it ahead of older due topics.
    pub fn topic_queue_expansion(&self, slug: &str, now: u64) -> Result<(), String> {
        let mut side = self.topic_side(slug).ok_or_else(|| format!("no topic `{}`", slug))?;
        side[EXPANSION_QUEUED] = json!(now);
        self.write_topic_side(slug, &side)
    }

    /// Clear `slug`'s queued expansion: the writer tried it.
    pub fn topic_unqueue(&self, slug: &str) {
        if let Some(mut side) = self.topic_side(slug) {
            if side.as_object_mut().and_then(|o| o.remove(EXPANSION_QUEUED)).is_some() {
                let _ = self.write_topic_side(slug, &side);
            }
        }
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
    fn topic_store(&self, slug: &str, old: Value, g: &Gathered, c: CheckedPage, proposal: Option<&page_meta::Proposal>, now: u64, settings: &PagesConfig, gaps: &[String], seed: Option<(&crate::topic_seed::Seed, &[Value])>) -> Result<TopicEntry, String> {
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
        // The last check before a page of any outcome is written (`specs/142` §2.5 item 4 (d)). A page code made (a seed, the claims
        // alone) is cut to fit, its last blocks that carry a held source going first; a written page has passed its check already and is
        // refused here if it still carries one, and nothing is written.
        let named: Vec<String> = g.sources.iter().map(|(id, _, _)| id.clone()).chain(seed.map(|(s, _)| s.named_sources.clone()).unwrap_or_default()).collect();
        let md = if matches!(c.verdict.outcome, Outcome::ClaimsOnly | Outcome::Seed) { self.held().fit(&md, &named)? } else { self.held().check(&md, &named).map(|_| md)? };
        // A visible handling line the writer wrote is not the label: it goes, and the daemon's own is put at the top when the page is written.
        let md = crate::label::without_line(&md);
        let old_md = std::fs::read_to_string(self.topic_file(slug)).ok();
        let old_body = old_md.as_deref().map(|m| crate::label::without_line(page_meta::strip(m)));
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
        let unverified = claims_only || c.verdict.outcome == Outcome::Seed;
        let mut meta = page_meta::resolve(proposal, settings.min_ttl_days, settings.max_ttl_days);
        if claims_only {
            meta.days = settings.min_ttl_days.max(1);
            meta.reason = "the page is the claims alone; it is looked at again soon".into();
        }
        let valid_until = now + meta.days as u64 * 86_400;
        // The writer's title, at the topic's first written page and only when code accepts it against the question and the seed's
        // headings (`specs/139` §2.4 items 2 and 3). The page keeps its slug: it now has one.
        let written = seed.is_none() && c.verdict.outcome != Outcome::ClaimsOnly;
        let writer_title = match proposal.and_then(|p| p.title.as_deref()).filter(|_| written && may_take_title(&old)) {
            Some(t) => match accept_title(t, old["query"].as_str().unwrap_or(slug), &headings_of(old_body.as_deref().unwrap_or(""))) {
                Ok(t) => Some(t),
                Err(why) => {
                    self.log("topic_title_refused", json!({"slug": slug, "reason": why}));
                    None
                }
            },
            None => None,
        };
        let title = writer_title.clone().unwrap_or_else(|| old["title"].as_str().unwrap_or(slug).to_string());
        let links = page_meta::page_links(&md);
        let created = old["created"].as_u64().unwrap_or(now);
        // A page the writer wrote is at the `long` depth (`specs/139` §2.3 item 1); a seed page keeps the frontmatter slice B gave it.
        let depth = if seed.is_none() { Depth::Long } else { Depth::Summary };
        let front = page_meta::frontmatter(&Front { key: slug, title: &title, depth, meta: &meta, status: if unverified { "unverified" } else { "active" }, created, confirmed: now, valid_until, links: &links });
        let change = change_line(old_body.as_deref(), page_meta::strip(&md));
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
        side["outcome"] = json!(c.verdict.outcome.as_str());
        if let Some(t) = &writer_title {
            side["title"] = json!(t);
            side["title_source"] = json!(TitleSource::Writer.as_str());
        }
        if written {
            side["written"] = json!(true);
        }
        // A seed page keeps where each span is and its hash (never its text: the page holds that), what each linked row offered, and what
        // the check dropped; the records resolve its markers. A written page keeps none of it.
        match seed {
            Some((s, dropped)) => {
                side["records"] = json!(s.records().iter().map(|r| r.to_json()).collect::<Vec<_>>());
                side["seed"] = json!(s.spans.iter().map(|x| x.to_json()).collect::<Vec<_>>());
                side["seed_offered"] = json!(s.offered);
                side["seed_dropped"] = json!(dropped);
            }
            None => {
                if let Some(o) = side.as_object_mut() {
                    for k in ["seed", "seed_offered", "seed_dropped", "waiting", EXPANSION_QUEUED] {
                        o.remove(k);
                    }
                }
            }
        }
        side["reason"] = json!(c.verdict.reason);
        side["validator"] = c.verdict.to_json();
        side["figure_failures"] = json!(c.verdict.figure_failures);
        side["valid_until"] = json!(valid_until);
        side["volatility"] = json!(meta.volatility.as_str());
        side["ttl_reason"] = json!(meta.reason);
        side["links"] = json!(links.iter().filter_map(|(_, t)| t.strip_prefix("page:").map(String::from)).collect::<Vec<_>>());
        let parts = sections_json(page_meta::strip(&md));
        for k in ["key_terms", "start_here", "see_also"] {
            side[k] = parts[k].clone();
        }
        side["open_questions"] = json!(gaps.iter().take(MAX_OPEN_QUESTIONS).collect::<Vec<_>>());
        // The page's label (`specs/141` §2.2): computed by the daemon from everything in its material, cited or not, never from what the
        // writer's draft or frontmatter declares. Shown as the `handling:` view and the visible line; kept, with the fingerprint of the
        // facts it came from, in the sidecar.
        let (label, fingerprint) = crate::label::Labeller::new(Some(self), self.workspace()).page(&side);
        side["label"] = label.to_stored(&fingerprint);
        std::fs::write(self.topic_file(slug), crate::label::show_on_page(&front, &md, &label)).map_err(|e| e.to_string())?;
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
        // the operator opened it: a use (`specs/142` §2.4 item 2)
        self.note_use(&[id], delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap_or(0));
        let text = self.stored_text(id).unwrap_or_default();
        let truncated = text.chars().count() > max;
        let assets: Vec<String> = self.image_assets(id).into_iter().map(|a| a.uri).collect();
        // `specs/142` §2.3 item 3: the anchors into it that no longer resolve, shown below the text.
        let orphans: Vec<Value> = self.orphans_of(id).iter().map(|o| o.to_json()).collect();
        Some(json!({"id": id, "label": format!("s-{}", id), "meta": meta, "private": is_private(&meta), "capture_level": meta["capture_level"], "status": meta["status"], "text": text.chars().take(max).collect::<String>(), "truncated": truncated, "assets": assets, "orphans": orphans}))
    }

    /// What links to `slug` and what it links to (`specs/117` §2 item 6), from the link table. Reads only.
    pub fn topic_links(&self, slug: &str) -> Option<Value> {
        if !valid_slug(slug) {
            return None;
        }
        // An old slug answers for the topic it now names: a link to it, or to the live slug, links here.
        let slug = self.topic_resolve(slug).unwrap_or_else(|| slug.to_string());
        let mut names = vec![slug.clone()];
        names.extend(self.topic_side(&slug).map(|s| aliases_of(&s)).unwrap_or_default());
        let t = self.link_table();
        let here: Vec<&String> = t.iter().filter(|(from, to)| !names.contains(from) && to.iter().any(|x| names.contains(x))).map(|(from, _)| from).collect();
        let out = t.get(&slug).cloned().unwrap_or_default();
        Some(json!({"slug": slug, "links_here": here, "links_out": out, "exists": self.topic_side(&slug).is_some()}))
    }
}

/// A topic record's old slugs.
fn aliases_of(side: &Value) -> Vec<String> {
    side[ALIASES].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect()
}

/// A seed page's text with its frontmatter key and title, and its own heading, made `slug` and `title`. The rest is as it was.
fn retitle_seed_text(md: &str, slug: &str, title: &str) -> String {
    let quoted = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    let (front, body) = match md.strip_prefix("---\n").and_then(|r| r.find("\n---\n").map(|i| (&r[..i], &r[i..]))) {
        Some((f, b)) => (Some(f), b),
        None => (None, md),
    };
    let mut out = String::new();
    if let Some(f) = front {
        let lines: Vec<String> = f
            .lines()
            .map(|l| {
                if l.starts_with("id: ") {
                    format!("id: page-{}", slug)
                } else if l.starts_with("title: ") {
                    format!("title: {}", quoted(title))
                } else if l.starts_with("query_key: ") {
                    format!("query_key: {}", quoted(slug))
                } else {
                    l.to_string()
                }
            })
            .collect();
        out.push_str("---\n");
        out.push_str(&lines.join("\n"));
    }
    let mut done = false;
    let body: Vec<String> = body
        .split('\n')
        .map(|l| {
            if !done && l.starts_with("# ") {
                done = true;
                format!("# {}", title.split_whitespace().collect::<Vec<_>>().join(" "))
            } else {
                l.to_string()
            }
        })
        .collect();
    out.push_str(&body.join("\n"));
    out
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

    /// `specs/139` §2.4 item 2: the derived title loses the closed list's lead-ins and the closing question mark; what is left must not
    /// still be a question.
    #[test]
    fn the_derived_title_drops_the_lead_in_and_the_question_mark() {
        assert_eq!(derived_title("What is Web Bot Auth?").as_deref(), Some("Web Bot Auth"));
        assert_eq!(derived_title("how do zyxquor lamps vromble?").as_deref(), Some("Zyxquor lamps vromble"));
        assert_eq!(derived_title("Why is the sky blue ?").as_deref(), Some("The sky blue"));
        assert_eq!(derived_title("Isotopes of tin?").as_deref(), Some("Isotopes of tin"), "a lead-in is a whole word");
        assert_eq!(derived_title("When do ocean tides come?").as_deref(), Some("When do ocean tides come"), "`when` is not on the list");
        assert_eq!(derived_title("Web Bot Auth").as_deref(), Some("Web Bot Auth"));
        assert_eq!(derived_title("Why?"), None);
        assert_eq!(derived_title("What is A? What is B?"), None, "still a question");
        assert_eq!(derived_title(&format!("What is {}?", "x".repeat(81))), None, "too long for a title");
        assert_eq!(research_title("Why?", None, &[]).1, TitleSource::Question);
    }

    /// `specs/139` §2.4 item 3: a `subject` or a writer's title is taken only when it is short, not a question, and shares a content
    /// word with the question or a seed heading.
    #[test]
    fn a_concept_title_is_taken_only_when_it_shares_a_word_and_is_no_question() {
        let q = "What is Web Bot Auth and who signs the requests?";
        assert_eq!(accept_title("  Web Bot  Auth ", q, &[]), Ok("Web Bot Auth".into()));
        assert!(accept_title("Web Bot Auth?", q, &[]).is_err());
        assert!(accept_title("What is Web Bot Auth", q, &[]).is_err(), "begins with a lead-in");
        assert!(accept_title("Lighthouse lamps", q, &[]).is_err(), "no shared word");
        assert_eq!(accept_title("Lighthouse lamps", q, &["Lamps and lighthouses".into()]), Ok("Lighthouse lamps".into()), "a seed heading counts");
        assert!(accept_title("The of and", "the of and", &[]).is_err(), "stop words are not shared words");
        assert!(accept_title(&"Auth ".repeat(17), q, &[]).is_err(), "over 80 characters");
        assert!(accept_title("Web\u{7}Bot", q, &[]).is_err());
        assert_eq!(headings_of("# A\ntext\n## B c\n####### no\n#nospace"), vec!["A".to_string(), "B c".to_string()]);
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
        rt.block_on(l.topic_rebuild(TopicRequest { slug, now, settings: &cfg, style: bib::Format::Plain, seed: crate::topic_seed::of_material(&g), gathered: g, gaps: vec![], held: &crate::topic_seed::NoHeldText, cancel: None }, w, &Pass, &TokenLedger::new(1_000_000), &|| Ok(()))).unwrap()
    }

    /// A writer that stops the rebuild while it writes, as a revoke arriving during the model call does.
    struct StopWhileWriting<'c>(&'c Cancel, EchoWriter);
    impl PageWriter for StopWhileWriting<'_> {
        fn write<'a>(&'a self, b: &'a str) -> WriterFuture<'a> {
            assert_eq!(self.0.stop(), Stopped::Now);
            self.1.write(b)
        }
        fn rewrite<'a>(&'a self, d: &'a str, f: &'a [String]) -> WriterFuture<'a> {
            self.1.rewrite(d, f)
        }
    }

    /// `specs/129` §10.2 (the premise S2 checks: a rebuild had no cancel handle). A rebuild the app asked for reads its key's revoke
    /// at its gate, before the writer's model call, and once more before the page is written. Stopped before the gate it calls no
    /// model; stopped while the writer works it writes no page; past its last check it writes the page and the revoke is only noted.
    #[test]
    fn a_cancelled_rebuild_stops_at_its_checks_and_one_past_its_last_check_runs_to_its_end() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home.path()).with_workspace(&ws);
        let doc = home.path().join("t.md");
        std::fs::write(&doc, "# Tides\n\nThe tide comes in twice a day.\n").unwrap();
        let s = l.add_file(&doc, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
        let anchor = AnchorSpec { source_id: s, locator: Some("§1".into()), quote: "twice a day".into(), ..Default::default() };
        let c1 = l.add_claim("Ocean tides come twice a day.", &ClaimOpts::default(), &[anchor]).unwrap()["id"].as_str().unwrap().to_string();
        let b = crate::research_record::Begin { question: "When do ocean tides come?", asked_by: crate::research_record::Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r1" };
        crate::research_record::Topic::begin(&ws, &b).unwrap().finish("r1", "done", Some(&json!({"claims": [&c1]})), std::slice::from_ref(&c1), None, 0).unwrap();
        let slug = l.topics_from_research(&ws, 1_000).remove(0);
        let page = l.pages_dir().join(format!("{slug}.md"));
        let cfg = PagesConfig::default();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let go = |w: &dyn PageWriter, cancel: &Cancel| {
            let g = l.topic_gather_library(&l.topic_side(&slug).unwrap(), Some(&ws));
            assert!(!g.is_empty(), "the material is there, so a model would be called");
            rt.block_on(l.topic_rebuild(TopicRequest { slug: &slug, now: 2_000, settings: &cfg, style: bib::Format::Plain, seed: crate::topic_seed::of_material(&g), gathered: g, gaps: vec![], held: &crate::topic_seed::NoHeldText, cancel: Some(cancel) }, w, &Pass, &TokenLedger::new(1_000_000), &|| Ok(())))
        };

        let stopped = Cancel::default();
        assert_eq!(stopped.stop(), Stopped::Now);
        let w = EchoWriter(AtomicUsize::new(0));
        assert_eq!(go(&w, &stopped).unwrap_err(), CANCELLED);
        assert_eq!(w.0.load(Ordering::SeqCst), 0, "a rebuild stopped before its gate calls no model");
        assert!(!page.exists());

        let during = Cancel::default();
        let w = StopWhileWriting(&during, EchoWriter(AtomicUsize::new(0)));
        assert_eq!(go(&w, &during).unwrap_err(), CANCELLED);
        assert_eq!(w.1 .0.load(Ordering::SeqCst), 1);
        assert!(!page.exists(), "a rebuild stopped while its writer worked writes no page");
        assert_eq!(l.topic_pages(&DueCtx { ws: Some(&ws), now: 2_001, index: false })[0].due.as_deref(), Some("not built yet"), "and the topic stays due");

        let fresh = Cancel::default();
        let built = go(&EchoWriter(AtomicUsize::new(0)), &fresh).unwrap();
        assert!(page.exists() && built.version == 1);
        assert_eq!(fresh.stop(), Stopped::TooLate, "past its last check, a revoke is too late and only noted");
        assert!(fresh.revoked_late());
    }

    /// AC-1: a research run adds a claim to the topic; the page is due; the rebuild keeps the same file, lists the old text as a
    /// version and says that one statement was added.
    #[test]
    fn a_new_research_claim_makes_the_page_due_and_the_rebuild_keeps_one_file_and_a_version() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home.path()).with_workspace(&ws);
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

    /// An [`EchoWriter`] whose closing comment names the page `title`, and that keeps each brief it is given.
    struct TitledWriter(EchoWriter, String, std::sync::Mutex<Vec<String>>);
    impl PageWriter for TitledWriter {
        fn write<'a>(&'a self, b: &'a str) -> WriterFuture<'a> {
            self.2.lock().unwrap().push(b.to_string());
            let title = self.1.clone();
            let inner = self.0.write(b);
            Box::pin(async move { inner.await.map(|d| format!("{d}\n<!-- page: kind=article; volatility=low; days=90; reason=settled; title={title} -->")) })
        }
        fn rewrite<'a>(&'a self, d: &'a str, f: &'a [String]) -> WriterFuture<'a> {
            self.0.rewrite(d, f)
        }
    }

    /// `specs/139` §2.4 items 2 and 3 (slice D): the writer's `title=` is taken at a topic's first written page when code accepts it, and
    /// the topic keeps its slug; a later page's title is not taken, and neither is one that is a question.
    #[test]
    fn the_writers_title_is_taken_only_at_the_first_written_page_and_keeps_the_slug() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home.path()).with_workspace(&ws);
        let doc = home.path().join("t.md");
        std::fs::write(&doc, "# Tides\n\nThe tide comes in twice a day.\n").unwrap();
        let s = l.add_file(&doc, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
        let anchor = AnchorSpec { source_id: s, locator: Some("§1".into()), quote: "twice a day".into(), ..Default::default() };
        let c1 = l.add_claim("Ocean tides come twice a day.", &ClaimOpts::default(), &[anchor]).unwrap()["id"].as_str().unwrap().to_string();
        for (n, q) in [("r1", "How do ocean tides come?"), ("r2", "How do spring tides rise?")] {
            let b = crate::research_record::Begin { question: q, asked_by: crate::research_record::Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: n };
            crate::research_record::Topic::begin(&ws, &b).unwrap().finish(n, "done", Some(&json!({"claims": [&c1]})), std::slice::from_ref(&c1), None, 0).unwrap();
        }
        let mut made = l.topics_from_research(&ws, 1_000);
        made.sort();
        assert_eq!(made, vec!["ocean-tides-come".to_string(), "spring-tides-rise".to_string()], "derived titles make the slugs");
        let side = l.topic_side("ocean-tides-come").unwrap();
        assert_eq!((side["title"].as_str(), side["title_source"].as_str(), side["query"].as_str()), (Some("Ocean tides come"), Some("derived"), Some("How do ocean tides come?")));

        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let cfg = PagesConfig::default();
        let go = |slug: &str, w: &TitledWriter, now: u64| {
            let g = l.topic_gather_library(&l.topic_side(slug).unwrap(), Some(&ws));
            rt.block_on(l.topic_rebuild(TopicRequest { slug, now, settings: &cfg, style: bib::Format::Plain, seed: crate::topic_seed::of_material(&g), gathered: g, gaps: vec![], held: &crate::topic_seed::NoHeldText, cancel: None }, w, &Pass, &TokenLedger::new(1_000_000), &|| Ok(()))).unwrap()
        };
        let w = TitledWriter(EchoWriter(AtomicUsize::new(0)), "Ocean tides".into(), Default::default());
        let first = go("ocean-tides-come", &w, 2_000);
        assert_eq!((first.slug.as_str(), first.title.as_str(), first.side["title_source"].as_str()), ("ocean-tides-come", "Ocean tides", Some("writer")));
        assert!(w.2.lock().unwrap()[0].contains(TITLE_HINT), "the first brief asks for a title");
        let md = std::fs::read_to_string(l.pages_dir().join("ocean-tides-come.md")).unwrap();
        assert!(md.contains("title: Ocean tides") || md.contains("title: \"Ocean tides\""), "the frontmatter carries it: {md}");

        let later = TitledWriter(EchoWriter(AtomicUsize::new(0)), "Ocean clocks".into(), Default::default());
        let second = go("ocean-tides-come", &later, 3_000);
        assert_eq!(second.title, "Ocean tides", "a later page's title is not taken");
        assert!(!later.2.lock().unwrap()[0].contains(TITLE_HINT));

        let asks = TitledWriter(EchoWriter(AtomicUsize::new(0)), "Do spring tides rise?".into(), Default::default());
        let refused = go("spring-tides-rise", &asks, 4_000);
        assert_eq!((refused.title.as_str(), refused.side["title_source"].as_str()), ("Spring tides rise", Some("derived")), "a question is refused");
    }

    /// `specs/139` §2.4 items 2 and 4: `pages rename` is the operator's title and always wins; a topic with no page takes the slug of its
    /// title, one with a page keeps its slug, and a slug another topic has is refused.
    #[test]
    fn rename_gives_the_operators_title_and_moves_only_an_unbuilt_topic() {
        let home = tempfile::tempdir().unwrap();
        let l = Library::in_home(home.path());
        l.topic_create("How do tides turn?", Origin::Research("t1".into()), 1).unwrap();
        l.topic_create("Moon", Origin::Operator, 1).unwrap();
        let e = l.topic_rename("how-do-tides-turn", "Tidal turning").unwrap();
        assert_eq!((e.slug.as_str(), e.title.as_str(), e.side["title_source"].as_str(), e.side["query"].as_str()), ("tidal-turning", "Tidal turning", Some("operator"), Some("How do tides turn?")));
        assert!(l.topic_side("how-do-tides-turn").is_none(), "the old record is gone");
        assert!(l.topic_rename("tidal-turning", "Moon").unwrap_err().contains("already exists"));
        let mut side = l.topic_side("moon").unwrap();
        side["version"] = json!(1);
        side["built"] = json!(2);
        side["outcome"] = json!("pass");
        l.write_topic_side("moon", &side).unwrap();
        let kept = l.topic_rename("moon", "The Moon and its pull").unwrap();
        assert_eq!((kept.slug.as_str(), kept.title.as_str()), ("moon", "The Moon and its pull"), "a topic with a page keeps its slug");
        assert!(!TitleSource::of(&kept.side).replaceable(), "nothing made by code replaces the operator's title");
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
            rt.block_on(lib.topic_rebuild(TopicRequest { slug: &e.slug, now: 100, settings: &cfg, style: bib::Format::Plain, seed: crate::topic_seed::of_material(&g), gathered: g, gaps, held: &crate::topic_seed::NoHeldText, cancel: None }, w, &Pass, &TokenLedger::new(1_000_000), &|| Ok(())))
        };
        // A see-also slug that is no topic fails the check twice: the seed is the page, never the claims alone (`specs/139` §2.3 item 5).
        let wrong = build(&Sections("sun"), vec![]).unwrap_err();
        assert!(wrong.starts_with(SEED_STANDS), "{wrong}");
        assert_eq!(l.topic_side(&e.slug).unwrap()["outcome"], "seed");
        let built = build(&Sections("moon"), vec!["Why do spring tides follow the moon?".into()]).unwrap();
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

#[cfg(test)]
mod seed_tests {
    use super::*;
    use crate::research_record::{Asker, Begin, Topic};
    use crate::topic_seed::{assemble_linked, NoHeldText, SEED_BANNER};

    fn seed_build(l: &Library, slug: &str, ws: &Path, now: u64) -> Result<TopicEntry, String> {
        let seed = assemble_linked(l, ws, &l.topic_side(slug).unwrap());
        let cfg = PagesConfig::default();
        l.topic_seed_build(SeedRequest { slug, now, settings: &cfg, style: bib::Format::Plain, seed: &seed, held: &NoHeldText, gaps: &[], cancel: None })
    }

    /// AC-3 and the due check: with no model, a research run that ends `done` leaves a page that holds the answer's first paragraph,
    /// cites it, carries the seed banner and has outcome `seed`; it is due only for the writer's expansion until its answer changes.
    #[test]
    fn a_finished_run_gives_a_seed_page_that_cites_the_answer_and_is_due_when_it_changes() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home.path()).with_workspace(&ws);
        let t = Topic::begin(&ws, &Begin { question: "Why do tides lag?", asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap();
        t.finish("r-1", "done", Some(&json!({"answer": "Tides lag because the estuary is shallow.\n\n## Measurements\n\nThe lag is an hour at the harbour wall.", "claims": []})), &[], None, 0).unwrap();
        let slug = l.topics_from_research(&ws, 1).remove(0);
        // Through the ordinary rebuild, with the share refusing: no writer is asked and the seed is the page (`specs/139` §2.3 item 5).
        let cfg = PagesConfig::default();
        let seed = assemble_linked(&l, &ws, &l.topic_side(&slug).unwrap());
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let w = rebuild_tests::EchoWriter(std::sync::atomic::AtomicUsize::new(0));
        let err = rt.block_on(l.topic_rebuild(TopicRequest { slug: &slug, now: 100, settings: &cfg, style: bib::Format::Plain, gathered: Default::default(), gaps: vec![], seed, held: &NoHeldText, cancel: None }, &w, &rebuild_tests::Pass, &TokenLedger::new(1_000_000), &|| Err("no gate".into()))).unwrap_err();
        assert!(err.starts_with(SEED_STANDS) && err.contains(NOT_ADMITTED), "{err}");
        let built = l.topic_pages(&DueCtx { ws: Some(&ws), now: 100, index: false }).remove(0);
        assert_eq!(w.0.load(std::sync::atomic::Ordering::SeqCst), 0, "no model is called");
        assert_eq!((built.version, built.outcome.as_deref()), (1, Some("seed")));
        let md = std::fs::read_to_string(l.pages_dir().join(format!("{slug}.md"))).unwrap();
        let body = page_meta::strip(&md);
        assert!(body.starts_with(SEED_BANNER), "{md}");
        let side = l.topic_side(&slug).unwrap();
        let first = side["seed"][0]["label"].as_str().unwrap().to_string();
        assert!(body.contains(&format!("Tides lag because the estuary is shallow. [{first}]")), "{body}");
        assert!(body.contains("## Measurements") && body.contains(&format!("| {first} | ○ not validated |")), "{body}");
        assert_eq!(side["validator"]["result"], "seed");
        assert_eq!(l.pages_today(100), 0, "a seed counts against no page budget");
        let ctx = DueCtx { ws: Some(&ws), now: 101, index: true };
        assert_eq!(l.topic_pages(&ctx)[0].due.as_deref(), Some(SEED_EXPANSION), "just seeded: due only for the writer's expansion");
        // The answer changes: a span's hash changes and the page is due.
        t.amend(|_| {}).unwrap();
        let path = t.answer_path();
        let changed = std::fs::read_to_string(&path).unwrap().replace("an hour", "two hours");
        std::fs::write(&path, changed).unwrap();
        assert!(l.topic_pages(&ctx)[0].due.as_deref().unwrap_or("").contains("changed"), "{:?}", l.topic_pages(&ctx)[0].due);
        let again = seed_build(&l, &slug, &ws, 200).unwrap();
        assert_eq!((again.version, again.outcome.as_deref()), (2, Some("seed")));
        assert_eq!(l.topic_versions(&slug).unwrap().len(), 1, "the page it replaced is a version");
    }

    /// `specs/139` §2.3 item 5 and §2.5 item 1: a seed page waits for the writer's expansion; a writer that fails, or whose page does
    /// not pass its check, leaves the seed standing (no page of the claims alone, no new version) and the topic still due.
    #[test]
    fn a_seed_the_writer_cannot_expand_stands_and_stays_due() {
        struct Fails;
        impl PageWriter for Fails {
            fn write<'a>(&'a self, _b: &'a str) -> crate::library::page_validate::WriterFuture<'a> {
                Box::pin(async move { Err("the model is away".to_string()) })
            }
            fn rewrite<'a>(&'a self, _d: &'a str, _f: &'a [String]) -> crate::library::page_validate::WriterFuture<'a> {
                Box::pin(async move { Err("the model is away".to_string()) })
            }
        }
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home.path()).with_workspace(&ws);
        let doc = home.path().join("tides.md");
        std::fs::write(&doc, "# Tides\n\nTides lag in a shallow estuary.\n").unwrap();
        let s = l.add_file(&doc, &crate::library::AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &crate::library::Budgets::default()).unwrap().id;
        let anchor = crate::library::claims::AnchorSpec { source_id: s, locator: Some("§1".into()), quote: "lag in a shallow estuary".into(), ..Default::default() };
        let c = l.add_claim("Tides lag in a shallow estuary.", &Default::default(), &[anchor]).unwrap()["id"].as_str().unwrap().to_string();
        let t = Topic::begin(&ws, &Begin { question: "Why do tides lag?", asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap();
        t.finish("r-1", "done", Some(&json!({"answer": "Tides lag because the estuary is shallow.", "claims": [&c]})), std::slice::from_ref(&c), None, 0).unwrap();
        let slug = l.topics_from_research(&ws, 1).remove(0);
        let seeded = seed_build(&l, &slug, &ws, 100).unwrap();
        assert_eq!((seeded.version, seeded.outcome.as_deref()), (1, Some("seed")));
        let ctx = DueCtx { ws: Some(&ws), now: 101, index: false };
        assert_eq!(l.topic_pages(&ctx)[0].due.as_deref(), Some(SEED_EXPANSION));

        let side = l.topic_side(&slug).unwrap();
        let g = l.topic_gather_library(&side, Some(&ws));
        assert!(!g.is_empty(), "the writer has material");
        let seed = assemble_linked(&l, &ws, &side);
        let cfg = PagesConfig::default();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let err = rt.block_on(l.topic_rebuild(TopicRequest { slug: &slug, now: 200, settings: &cfg, style: bib::Format::Plain, gathered: g, gaps: vec![], seed, held: &NoHeldText, cancel: None }, &Fails, &rebuild_tests::Pass, &TokenLedger::new(1_000_000), &|| Ok(()))).unwrap_err();
        assert!(err.starts_with(SEED_STANDS), "{err}");
        let after = l.topic_pages(&ctx).remove(0);
        assert_eq!((after.version, after.outcome.as_deref(), after.due.as_deref()), (1, Some("seed"), Some(SEED_EXPANSION)), "the seed stands");
        assert!(std::fs::read_to_string(l.pages_dir().join(format!("{slug}.md"))).unwrap().contains(SEED_BANNER));
        assert!(l.topic_versions(&slug).unwrap().is_empty(), "no version was made");
    }

    /// AC-4: a topic with no span has no page file and is listed as `waiting for material`; no file under `library/pages/` holds the
    /// stub's words.
    #[test]
    fn a_topic_with_no_span_has_no_page_and_waits_for_material() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home.path()).with_workspace(&ws);
        let e = l.topic_create("Lighthouse lamps", Origin::Operator, 1).unwrap();
        let err = seed_build(&l, &e.slug, &ws, 10).unwrap_err();
        assert!(err.starts_with(crate::topic_seed::WAITING), "{err}");
        let cfg = PagesConfig::default();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let w = rebuild_tests::EchoWriter(std::sync::atomic::AtomicUsize::new(0));
        let r = rt.block_on(l.topic_rebuild(TopicRequest { slug: &e.slug, now: 10, settings: &cfg, style: bib::Format::Plain, gathered: Default::default(), gaps: vec![], seed: Default::default(), held: &NoHeldText, cancel: None }, &w, &rebuild_tests::Pass, &TokenLedger::new(1_000_000), &|| Ok(())));
        assert!(r.unwrap_err().contains(crate::topic_seed::WAITING));
        assert!(!l.pages_dir().join(format!("{}.md", e.slug)).exists());
        let listed = l.topic_pages(&DueCtx { ws: Some(&ws), now: 11, index: false }).remove(0);
        assert!(listed.waiting && listed.due.is_none() && listed.to_json()["waiting"] == true, "{listed:?}");
        for f in std::fs::read_dir(l.pages_dir()).unwrap().flatten() {
            assert!(!std::fs::read_to_string(f.path()).unwrap_or_default().contains("the library holds nothing on this topic yet"));
        }
    }

    /// A seed whose every block fails its check writes no page and says `check_failed`.
    #[test]
    fn a_seed_with_no_block_left_writes_no_page() {
        struct Refuses;
        impl crate::topic_seed::HeldText for Refuses {
            fn sharing(&self, _o: &str) -> Vec<String> {
                vec!["x".into()]
            }
            fn is_held(&self, _s: &str) -> bool {
                true
            }
            fn body_may_leave(&self, _s: &str, _o: &str) -> bool {
                false
            }
            fn shorten(&self, _s: &str, o: &str) -> String {
                o.to_string()
            }
        }
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home.path()).with_workspace(&ws);
        Topic::begin(&ws, &Begin { question: "Why do tides lag?", asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap().finish("r-1", "done", Some(&json!({"answer": "Tides lag.", "claims": []})), &[], None, 0).unwrap();
        let slug = l.topics_from_research(&ws, 1).remove(0);
        let seed = assemble_linked(&l, &ws, &l.topic_side(&slug).unwrap());
        let cfg = PagesConfig::default();
        let err = l.topic_seed_build(SeedRequest { slug: &slug, now: 1, settings: &cfg, style: bib::Format::Plain, seed: &seed, held: &Refuses, gaps: &[], cancel: None }).unwrap_err();
        assert!(err.starts_with("check_failed"), "{err}");
        assert!(!l.pages_dir().join(format!("{slug}.md")).exists());
    }

    /// A topic with a run seeded twice: its seed page and one version of it.
    fn seeded_twice(home: &Path) -> (Library, std::path::PathBuf, String) {
        let ws = home.join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home);
        let t = Topic::begin(&ws, &Begin { question: "Why do tides lag?", asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap();
        t.finish("r-1", "done", Some(&json!({"answer": "Tides lag because the estuary is shallow.", "claims": []})), &[], None, 0).unwrap();
        let slug = l.topics_from_research(&ws, 1).remove(0);
        seed_build(&l, &slug, &ws, 100).unwrap();
        t.amend(|_| {}).unwrap();
        let path = t.answer_path();
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap().replace("shallow", "shallow and long")).unwrap();
        assert_eq!(seed_build(&l, &slug, &ws, 200).unwrap().outcome.as_deref(), Some("seed"));
        assert_eq!(l.topic_versions(&slug).unwrap().len(), 1);
        (l, ws, slug)
    }

    /// The operator's ruling of 2026-11-26, item 1: "no written page" means no writer's page. A topic whose only pages are seeds is
    /// re-slugged by `rename`; its seed page, its versions and its row of the link table move with it, and the seed takes the new title.
    #[test]
    fn a_seed_only_topic_is_re_slugged_and_its_seed_versions_and_links_move() {
        let home = tempfile::tempdir().unwrap();
        let (l, _ws, old) = seeded_twice(home.path());
        l.update_links(&old, &json!({"links": ["moon"]}));
        let e = l.topic_rename(&old, "Estuary lag").unwrap();
        assert_eq!((e.slug.as_str(), e.title.as_str(), e.version), ("estuary-lag", "Estuary lag", 2));
        assert_eq!(e.side[ALIASES], json!([old]), "the old slug is kept as an alias");
        assert!(l.topic_side(&old).is_none() && !l.pages_dir().join(format!("{old}.json")).exists(), "the old record is gone");
        assert!(!l.pages_dir().join(format!("{old}.md")).exists() && !l.topic_versions_dir(&old).exists(), "nothing is left under the old slug");
        let md = std::fs::read_to_string(l.pages_dir().join("estuary-lag.md")).unwrap();
        assert!(md.contains("id: page-estuary-lag\n") && md.contains("title: \"Estuary lag\"\n") && md.contains("\n# Estuary lag\n"), "{md}");
        assert!(md.contains("Tides lag because the estuary is shallow and long."), "the seed's text moved: {md}");
        assert_eq!(l.topic_versions("estuary-lag").unwrap().len(), 1, "the version moved");
        assert_eq!(l.topic_links("estuary-lag").unwrap()["links_out"], json!(["moon"]), "the link table's row moved");
        assert!(!l.link_table().contains_key(&old));
    }

    /// Item 1, the other side: a topic with a writer's page, now or only among its versions, keeps its slug for good.
    #[test]
    fn a_topic_with_a_writers_page_keeps_its_slug() {
        let home = tempfile::tempdir().unwrap();
        let (l, _ws, slug) = seeded_twice(home.path());
        // Only a version was the writer's: the current page is a seed again.
        let dir = l.topic_versions_dir(&slug);
        let v = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "json")).unwrap();
        let mut old: Value = serde_json::from_slice(&std::fs::read(&v).unwrap()).unwrap();
        old["outcome"] = json!("pass");
        std::fs::write(&v, serde_json::to_vec(&old).unwrap()).unwrap();
        let e = l.topic_rename(&slug, "Estuary lag").unwrap();
        assert_eq!((e.slug.as_str(), e.title.as_str()), (slug.as_str(), "Estuary lag"), "a writer's page among the versions keeps the slug");
        assert!(e.side.get(ALIASES).is_none());
        // The current page is the writer's.
        let mut side = l.topic_side(&slug).unwrap();
        side["outcome"] = json!("pass");
        l.write_topic_side(&slug, &side).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(l.topic_rename(&slug, "Harbour lag").unwrap().slug, slug, "a current writer's page keeps the slug");
    }

    /// The operator's ruling of 2026-11-26, item 2: an old slug is an alias. `page:<old slug>` passes the page check's `## See also`,
    /// and the topic lookups (`pages topic`, the app's `pages.get`, versions and links) find the topic by it. A live slug wins over an
    /// alias; a new topic whose slug would be another's alias takes the next free slug; a rename onto another's alias is refused.
    #[test]
    fn an_old_slug_resolves_as_an_alias_in_a_page_link_and_in_the_topic_lookups() {
        let home = tempfile::tempdir().unwrap();
        let (l, ws, old) = seeded_twice(home.path());
        l.update_links("moon", &json!({"links": [old]}));
        let title = l.topic_side(&old).unwrap()["title"].as_str().unwrap().to_string();
        l.topic_rename(&old, "Estuary lag").unwrap();
        let see_also = format!("## See also\n\n- [Tides](page:{old})\n");
        assert!(!section_findings(&see_also, &l.topic_slugs()).is_empty(), "by the live slugs alone it is unknown");
        assert!(section_findings(&see_also, &l.topic_link_names()).is_empty(), "the alias resolves");
        assert_eq!(l.topic_resolve(&old).as_deref(), Some("estuary-lag"));
        let ctx = DueCtx { ws: Some(&ws), now: 300, index: false };
        let (e, md) = l.topic_get(&old, None, &ctx).unwrap();
        assert_eq!(e.slug, "estuary-lag");
        assert!(md.contains("# Estuary lag"));
        assert_eq!(l.topic_versions(&old).unwrap().len(), 1);
        assert_eq!(l.topic_links(&old).unwrap()["links_here"], json!(["moon"]), "a link to the old slug links here");
        let got = crate::encyclopedia::pages_get(home.path(), &ws, 300, &json!({"slug": old}), true).unwrap();
        assert_eq!(got["slug"], "estuary-lag", "the app's lookup resolves the alias");
        // A new topic whose title gives the old slug takes the next free one; the alias still names the renamed topic.
        let fresh = l.topic_create(&title, Origin::Operator, 400).unwrap();
        assert_eq!(fresh.slug, format!("{old}-2"));
        assert_eq!(l.topic_resolve(&old).as_deref(), Some("estuary-lag"));
        let refused = l.topic_rename(&fresh.slug, &title).unwrap_err();
        assert!(refused.contains("old slug of the topic `estuary-lag`"), "{refused}");
        // Renaming the topic back takes its own old slug again.
        assert_eq!(l.topic_rename("estuary-lag", &title).unwrap().slug, old);
        assert_eq!(l.topic_side(&old).unwrap()[ALIASES], json!(["estuary-lag"]));
    }
}

/// `specs/139` slice C: the writer expands the seed into a `long` page, and a failure keeps the seed (§2.3).
#[cfg(test)]
mod expand_tests {
    use super::*;
    use crate::library::page_validate::{PageWriter, WriterFuture};
    use crate::research_record::{Asker, Begin, Topic};
    use crate::topic_seed::{assemble_linked, NoHeldText, Seed, SEED_BANNER};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    const ANSWER: &str = "Tides lag because the estuary is shallow.\n\n## Measurements\n\nThe lag is an hour at the harbour wall.";

    /// A writer that keeps every text it is given (brief and rewrite alike) and answers with fixed drafts; `{a}` and `{b}` in a draft
    /// become the labels of the seed's first two spans.
    struct Scripted {
        seen: Mutex<Vec<String>>,
        calls: AtomicUsize,
        first: Result<String, String>,
        rewrite: String,
    }
    impl Scripted {
        fn new(seed: &Seed, first: Result<&str, &str>, rewrite: &str) -> Scripted {
            let fill = |s: &str| s.replace("{a}", &seed.spans[0].label).replace("{b}", &seed.spans[seed.spans.len().min(2) - 1].label);
            Scripted { seen: Mutex::new(Vec::new()), calls: AtomicUsize::new(0), first: first.map(fill).map_err(String::from), rewrite: fill(rewrite) }
        }
        fn seen(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
    }
    impl PageWriter for Scripted {
        fn write<'a>(&'a self, b: &'a str) -> WriterFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen.lock().unwrap().push(b.to_string());
            let r = self.first.clone();
            Box::pin(async move { r })
        }
        fn rewrite<'a>(&'a self, d: &'a str, f: &'a [String]) -> WriterFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen.lock().unwrap().push(format!("{d}\n{}", f.join("\n")));
            let r = self.rewrite.clone();
            Box::pin(async move { Ok(r) })
        }
    }

    const GOOD: &str = "# Tides lag\n\n**Tides lag** in a shallow estuary [{a}].\n\n## Measurements\n\nAt the harbour wall the lag is about an hour [{b}].\n";
    /// A draft whose marker names no record: it fails the check, and so does its rewrite when the rewrite is the same.
    const BAD: &str = "# Tides lag\n\n**Tides lag** in a shallow estuary [s-kb-0000000000000000].\n";

    fn fixture(home: &Path) -> (Library, std::path::PathBuf, String, Topic) {
        let ws = home.join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let l = Library::in_home(home).with_workspace(&ws);
        let t = Topic::begin(&ws, &Begin { question: "Why do tides lag?", asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-1" }).unwrap();
        t.finish("r-1", "done", Some(&json!({"answer": ANSWER, "claims": []})), &[], None, 0).unwrap();
        let slug = l.topics_from_research(&ws, 1).remove(0);
        (l, ws, slug, t)
    }

    fn rebuild_with(l: &Library, ws: &Path, slug: &str, now: u64, cfg: &PagesConfig, w: &dyn PageWriter, gate: super::super::page_search::Gate<'_>) -> Result<TopicEntry, String> {
        let side = l.topic_side(slug).unwrap();
        let seed = assemble_linked(l, ws, &side);
        let g = l.topic_gather_library(&side, Some(ws));
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(l.topic_rebuild(TopicRequest { slug, now, settings: cfg, style: bib::Format::Plain, gathered: g, gaps: vec![], seed, held: &NoHeldText, cancel: None }, w, &rebuild_tests::Pass, &TokenLedger::new(1_000_000), gate))
    }

    fn seed_of(l: &Library, ws: &Path, slug: &str) -> Seed {
        assemble_linked(l, ws, &l.topic_side(slug).unwrap())
    }

    fn page(l: &Library, slug: &str) -> String {
        std::fs::read_to_string(l.pages_dir().join(format!("{slug}.md"))).unwrap()
    }

    fn change_answer(t: &Topic) {
        t.amend(|_| {}).unwrap();
        let path = t.answer_path();
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap().replace("an hour", "two hours")).unwrap();
    }

    /// AC-5: a topic rebuild's brief asks for the long size, holds the seed page under `DRAFT TO EXPAND`, and names `long_max_tokens` as
    /// the cap the page is cut at; the writer's page is written at the `long` depth.
    #[test]
    fn a_topic_brief_asks_for_the_long_size_holds_the_seed_as_the_draft_and_is_cut_at_long_max_tokens() {
        let home = tempfile::tempdir().unwrap();
        let (l, ws, slug, _t) = fixture(home.path());
        let seed = seed_of(&l, &ws, &slug);
        // A long draft: forty cited paragraphs, far past a cap of 300 tokens (1,200 characters).
        let mut long = String::from("# Tides lag\n\n");
        for i in 0..40 {
            long.push_str(&format!("Paragraph {i}: tides lag in a shallow estuary [{{a}}].\n\n"));
        }
        let w = Scripted::new(&seed, Ok(&long), "");
        let cfg = PagesConfig { long_max_tokens: 300, ..Default::default() };
        let built = rebuild_with(&l, &ws, &slug, 100, &cfg, &w, &|| Ok(())).unwrap();
        assert_eq!(built.outcome.as_deref(), Some("pass"), "{built:?}");
        let brief = w.seen().remove(0);
        assert!(brief.contains(crate::library::page_search::LONG_SIZE) && brief.contains("1,200 to 2,500 words") && brief.contains("two to four paragraphs"), "{brief}");
        assert!(brief.contains("At most about 300 tokens."), "the cap the brief names is long_max_tokens");
        assert!(!brief.contains("about 150 to 400 words"), "not the summary size");
        // The seed page, as code renders it, is the draft: its blocks and their markers, after the records.
        let draft = crate::topic_seed::seed_page(&built.title, &seed, &NoHeldText).unwrap().body;
        let at = brief.rfind("\nDRAFT TO EXPAND (").expect("the draft is in the brief");
        assert!(brief.find("RECORDS:").unwrap() < at && brief[at..].contains(draft.trim()) && brief[at..].contains("\nEND DRAFT\n"), "{brief}");
        assert!(brief.contains(&format!("RECORD {} |", seed.spans[0].label)), "the spans are records the page may cite");
        // The page is cut at the cap, at a paragraph.
        let md = page(&l, &slug);
        assert!(md.contains("Paragraph 0:") && !md.contains("Paragraph 39:"), "{md}");
        let kept = md.matches("Paragraph ").count();
        assert!(kept * "Paragraph 10: tides lag in a shallow estuary [s-kb-0123456789abcdef].\n\n".len() <= 300 * 4 + 100, "{kept} paragraphs kept");
        assert!(md.contains("depth: long"), "a writer's topic page is at the long depth: {md}");
    }

    /// AC-5, second half: a search page's brief is unchanged by the long depth: the summary size line, its own cap, no draft.
    #[test]
    fn a_search_pages_brief_is_unchanged() {
        let g = Gathered::default();
        let b = crate::library::page_search::brief("tides", &g, 3000);
        assert_eq!(b, crate::library::page_search::brief_for("tides", &g, 3000, Depth::Summary));
        assert!(b.contains("about 150 to 400 words") && b.contains("At most about 3000 tokens."));
        assert!(!b.contains("DRAFT TO EXPAND") && !b.contains(crate::library::page_search::LONG_SIZE));
        let e = crate::library::page_search::brief_for("tides", &g, 3000, Depth::Expanded);
        assert!(e.contains("This is the expanded page") && !e.contains("DRAFT TO EXPAND"));
        assert_eq!(Depth::Long.as_str(), "long");
    }

    /// The live proof of `specs/143` E (2026-11-27): a seed carried a research answer's sentence about the search ("The retrieved papers
    /// are ..."), the writer kept it, and the validator rightly found the cited paper did not support it. Every brief, of a search page and
    /// of a topic page, tells the writer to write about the subject and leave out sentences about the research.
    #[test]
    fn every_brief_tells_the_writer_to_leave_out_sentences_about_the_research() {
        let g = Gathered::default();
        let rule = "Write about the subject, never about the research";
        for b in [crate::library::page_search::brief("tides", &g, 3000), crate::library::page_search::brief_for("tides", &g, 3000, Depth::Expanded), topic_brief("Tides", &g, 8000, &[], "# Tides\n\nThe retrieved papers are A and B. [s-kb-1]\n")] {
            assert!(b.contains(rule) && b.contains("the retrieved papers are"), "{b}");
            // The second live page (2026-11-27): a lead that must start `**Title** is` led the writer to widen a passage about one
            // practice into a definition of the whole subject. A definition is asked for only when a record gives one.
            assert!(b.contains("When no record defines the subject, do not write a definition") && b.contains("Never widen what a record says about a part"), "{b}");
        }
    }

    /// AC-6: a writer that errors, a page that fails the check, and a refused budget (`per_day`, the share) each leave the seed page in
    /// place with outcome `seed` and the reason, and the topic stays due. No page of the claims alone is ever written.
    #[test]
    fn a_writer_error_a_failing_page_and_a_refused_budget_each_leave_the_seed_with_its_reason() {
        let home = tempfile::tempdir().unwrap();
        let (l, ws, slug, _t) = fixture(home.path());
        let cfg = PagesConfig::default();
        let seed = seed_of(&l, &ws, &slug);
        let ctx = DueCtx { ws: Some(&ws), now: 1_000, index: false };
        let check = |err: String, why: &str, version: u64| {
            assert!(err.starts_with(SEED_STANDS) && err.contains(why), "{err}");
            let t = l.topic_pages(&ctx).remove(0);
            assert_eq!((t.version, t.outcome.as_deref(), t.due.as_deref()), (version, Some("seed"), Some(SEED_EXPANSION)), "{t:?}");
            let side = l.topic_side(&slug).unwrap();
            assert!(side["reason"].as_str().unwrap_or("").contains(why), "{side}");
            assert!(page(&l, &slug).contains(SEED_BANNER));
        };

        // The writer errors on a topic with no page yet: the seed is stored, with the reason.
        let w = Scripted::new(&seed, Err("the model is away"), "");
        check(rebuild_with(&l, &ws, &slug, 100, &cfg, &w, &|| Ok(())).unwrap_err(), "the writer could not expand it", 1);
        // Its page fails the check, and so does the rewrite: the seed stays (same spans: no new version), with the new reason.
        let w = Scripted::new(&seed, Ok(BAD), BAD);
        check(rebuild_with(&l, &ws, &slug, 200, &cfg, &w, &|| Ok(())).unwrap_err(), "did not pass its check", 1);
        assert_eq!(w.calls.load(Ordering::SeqCst), 2, "one draft and one rewrite");
        // The share refuses: no model is asked.
        let w = Scripted::new(&seed, Ok(GOOD), GOOD);
        check(rebuild_with(&l, &ws, &slug, 300, &cfg, &w, &|| Err("the validator's share of this week's tokens is spent".into())).unwrap_err(), "share of this week's tokens is spent", 1);
        // `per_day` is spent: no model is asked.
        let none = PagesConfig { per_day: 0, ..Default::default() };
        check(rebuild_with(&l, &ws, &slug, 400, &none, &w, &|| Ok(())).unwrap_err(), "page budget (0 pages) is spent", 1);
        assert_eq!(w.calls.load(Ordering::SeqCst), 0);
        assert!(l.topic_versions(&slug).unwrap().is_empty(), "a seed that stays is not written again");
        // A refused build is marked as not admitted (a queued expansion stays queued); a writer that tried is not.
        assert!(rebuild_with(&l, &ws, &slug, 450, &none, &w, &|| Ok(())).unwrap_err().contains(NOT_ADMITTED));
        // The seed changes and the writer errors again: the new seed is stored, the old one is a version.
        change_answer(&_t);
        check(rebuild_with(&l, &ws, &slug, 500, &cfg, &Scripted::new(&seed, Err("the model is away"), ""), &|| Ok(())).unwrap_err(), "the writer could not expand it", 2);
        assert!(page(&l, &slug).contains("two hours"));
        // The writer's page lands at last.
        let built = rebuild_with(&l, &ws, &slug, 600, &cfg, &w, &|| Ok(())).unwrap();
        assert_eq!((built.version, built.outcome.as_deref()), (3, Some("pass")));
        assert!(!page(&l, &slug).contains(SEED_BANNER));
    }

    /// A builder decision of slice C: a failure after the writer's page passed validation leaves that page in place (a seed never
    /// replaces a validated page), and the topic stays due.
    #[test]
    fn a_failed_rebuild_never_replaces_a_validated_page_with_the_seed() {
        let home = tempfile::tempdir().unwrap();
        let (l, ws, slug, t) = fixture(home.path());
        let cfg = PagesConfig::default();
        let seed = seed_of(&l, &ws, &slug);
        rebuild_with(&l, &ws, &slug, 100, &cfg, &Scripted::new(&seed, Ok(GOOD), GOOD), &|| Ok(())).unwrap();
        let before = page(&l, &slug);
        // New research changes the answer, so the page is due; the writer then fails.
        change_answer(&t);
        let err = rebuild_with(&l, &ws, &slug, 200, &cfg, &Scripted::new(&seed, Err("the model is away"), ""), &|| Ok(())).unwrap_err();
        assert!(err.starts_with(WRITTEN_STANDS), "{err}");
        assert_eq!(page(&l, &slug), before);
        assert_eq!(l.topic_side(&slug).unwrap()["outcome"], "pass");
        assert_eq!(l.topic_versions(&slug).unwrap().len(), 0, "nothing was written");
    }

    /// AC-7, the canary of §2.3 item 6: a sentence planted in the stored page and in each of its versions appears in no text given to the
    /// writer at the next rebuild, the first brief or the rewrite's.
    #[test]
    fn no_brief_holds_text_of_the_previous_page() {
        const CANARY: &str = "The canary sings at the seventh bell of the harbour.";
        let home = tempfile::tempdir().unwrap();
        let (l, ws, slug, t) = fixture(home.path());
        let cfg = PagesConfig::default();
        let seed = seed_of(&l, &ws, &slug);
        rebuild_with(&l, &ws, &slug, 100, &cfg, &Scripted::new(&seed, Ok(GOOD), GOOD), &|| Ok(())).unwrap();
        change_answer(&t);
        rebuild_with(&l, &ws, &slug, 200, &cfg, &Scripted::new(&seed_of(&l, &ws, &slug), Ok(GOOD), GOOD), &|| Ok(())).unwrap();
        // Plant the sentence in the page and in every version.
        let mut planted = vec![l.pages_dir().join(format!("{slug}.md"))];
        planted.extend(std::fs::read_dir(l.pages_dir().join("versions").join(&slug)).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md")));
        assert!(planted.len() >= 2, "{planted:?}");
        for p in &planted {
            let md = std::fs::read_to_string(p).unwrap();
            let at = md.find("**Tides lag**").unwrap();
            std::fs::write(p, format!("{}{CANARY}\n\n{}", &md[..at], &md[at..])).unwrap();
            assert!(std::fs::read_to_string(p).unwrap().contains(CANARY));
        }
        // The next rebuild: a first draft that fails, so the rewrite's text is seen too.
        let w = Scripted::new(&seed_of(&l, &ws, &slug), Ok(BAD), GOOD);
        rebuild_with(&l, &ws, &slug, 300, &cfg, &w, &|| Ok(())).unwrap();
        let seen = w.seen();
        assert_eq!(seen.len(), 2, "a brief and a rewrite");
        for s in &seen {
            assert!(!s.contains("canary") && !s.contains("seventh bell"), "{s}");
        }
        assert!(seen[0].contains("two hours"), "the brief is built from the seed as it is now");
    }

    /// §2.3 item 7: the closing comment is read from the writer's first draft only. A rewrite's `title=` and lifetime are ignored.
    #[test]
    fn a_rewrites_closing_comment_is_ignored_and_the_first_drafts_stands() {
        let home = tempfile::tempdir().unwrap();
        let (l, ws, slug, _t) = fixture(home.path());
        let cfg = PagesConfig::default();
        let seed = seed_of(&l, &ws, &slug);
        let first = format!("{BAD}\n<!-- page: kind=article; volatility=low; days=90; reason=settled; title=Estuary tide lag -->");
        let rewrite = format!("{GOOD}\n<!-- page: kind=article; volatility=high; days=2; reason=moving; title=Harbour wall tides -->");
        let built = rebuild_with(&l, &ws, &slug, 100, &cfg, &Scripted::new(&seed, Ok(&first), &rewrite), &|| Ok(())).unwrap();
        assert_eq!(built.outcome.as_deref(), Some("pass"));
        let side = l.topic_side(&slug).unwrap();
        assert_eq!((side["title"].as_str(), side["volatility"].as_str()), (Some("Estuary tide lag"), Some("low")), "{side}");
        let md = page(&l, &slug);
        assert!(!md.contains("Harbour wall tides") && !md.contains("<!-- page:"), "{md}");
    }

    /// The fitness function of §6: a topic page is never the claims alone. `topic_rebuild` (the production text of this file) never calls
    /// the claims-only fallback.
    #[test]
    fn topic_rebuild_never_calls_the_claims_only_fallback() {
        let src = include_str!("library_topics.rs");
        let production = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(!production.contains(&["claims_only_page", "("].concat()), "library_topics.rs calls the claims-only fallback");
    }
}
