//! `record_research` (`specs/113` §2): a host hands over a finished piece of research, and Delphi keeps it as fully as research its own
//! runner did: a topic record with `runner: "host"`, the sources, the claims with their quotes, and the searches the host ran.
//!
//! * **The daemon checks, the host reports.** A source with a public https address that the host does not mark private is fetched by the
//!   daemon itself, under the rules its own runner fetches under (the trusted lists, and an address a research search of the session
//!   returned; `research_host::LiveWeb`), and each claim's quote is looked for in the text the daemon read. A quote it finds becomes an
//!   anchor into the page the library keeps; a quote it does not find is kept as `host-reported, not found`. This is the rule
//!   `propose_kb_entry` follows (`host_noted::decide`). The host cannot make the daemon fetch an address its runner could not reach.
//! * **Nothing is raised.** Every claim starts *noted* and is derived from the library's own records (`library::claims::status`); the
//!   reply and the record say `not validated`. What the host says about trust, status or privacy is not read.
//! * **Limits** ([`MAX_CLAIMS`], [`MAX_SOURCES`] per call, [`MAX_CALLS_PER_HOUR`] per session): past one the call is refused with the
//!   limit named, and nothing is written.
//!
//! The fetcher is passed in, so this file opens no connection of its own; the daemon hands it `LiveWeb`, the tests a fake web.

use crate::host_rpc::HostErr;
use crate::library::claims::{AnchorSpec, ResearchClaim};
use crate::library::Library;
use crate::research_record::{Asker, Begin, Topic};
use serde_json::{json, Value};
use std::path::Path;

pub const MAX_CLAIMS: usize = 40;
pub const MAX_SOURCES: usize = 40;
pub const MAX_CALLS_PER_HOUR: usize = 10;
/// The answer, in bytes: 24 KiB.
pub const MAX_ANSWER: usize = 24 * 1024;
pub const MAX_QUESTION: usize = 2000;
pub const MAX_SEARCHES: usize = 40;
pub const MAX_GAPS: usize = 20;
const MAX_CLAIM_TEXT: usize = 500;
const MAX_QUOTE: usize = 400;
const MAX_LOCATOR: usize = 500;
const MAX_TITLE: usize = 300;
/// The kinds of source a host may name (§2 item 1).
pub const KINDS: [&str; 4] = ["web", "internal", "file", "person"];
/// The line every reply carries (§2 item 7).
pub const NOT_VALIDATED: &str = "not validated";
/// The mark on a claim whose quote the daemon looked for and did not find, or could not look for (§2 item 3).
pub const NOT_FOUND: &str = "host-reported, not found";
/// The event a call leaves in the host session's log: ids and counts, never text. The per-session limit counts these.
pub const EVENT: &str = "host.research.recorded";

#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub locator: String,
    pub title: String,
    pub retrieved: String,
    pub kind: String,
    pub private: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    pub text: String,
    pub quote: String,
    /// The index of its source in `sources`, when it names one.
    pub source: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Search {
    pub place: String,
    pub words: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub question: String,
    pub answer: String,
    pub sources: Vec<Source>,
    pub claims: Vec<Claim>,
    pub searches: Vec<Search>,
    pub gaps: Vec<String>,
    pub topic_id: Option<String>,
}

/// The tool's input as a JSON Schema: the MCP tool declares it (`mcp_host::tools`) and `schemas/record-research.schema.json` is the
/// same schema as a file (the docs test holds the two equal).
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "question": {"type": "string", "minLength": 1, "maxLength": MAX_QUESTION, "description": "The question you researched."},
            "answer": {"type": "string", "maxLength": MAX_ANSWER, "description": "Your answer, as Markdown, at most 24 KiB."},
            "sources": {"type": "array", "maxItems": MAX_SOURCES, "items": {"type": "object", "properties": {
                "locator": {"type": "string", "minLength": 1, "maxLength": MAX_LOCATOR, "description": "A URL, or a description of a source with no public address."},
                "title": {"type": "string", "maxLength": MAX_TITLE},
                "retrieved": {"type": "string", "maxLength": 40, "description": "When you read it."},
                "kind": {"type": "string", "enum": KINDS, "default": "web"},
                "private": {"type": "boolean", "default": false, "description": "True for anything not public: Delphi then never fetches it and keeps only its locator, title and date."}
            }, "required": ["locator"], "additionalProperties": false}},
            "claims": {"type": "array", "maxItems": MAX_CLAIMS, "items": {"type": "object", "properties": {
                "text": {"type": "string", "minLength": 1, "maxLength": MAX_CLAIM_TEXT, "description": "One sentence."},
                "quote": {"type": "string", "maxLength": MAX_QUOTE, "description": "The exact words of the source that support it."},
                "source": {"type": "integer", "minimum": 0, "description": "The index of its source in `sources`, counting from 0."}
            }, "required": ["text"], "additionalProperties": false}},
            "searches": {"type": "array", "maxItems": MAX_SEARCHES, "items": {"type": "object", "properties": {
                "where": {"type": "string", "maxLength": 40, "description": "Where you searched (the web, a connector, a folder)."},
                "query": {"type": "string", "minLength": 1, "maxLength": 300, "description": "The words you used."}
            }, "required": ["query"], "additionalProperties": false}},
            "gaps": {"type": "array", "maxItems": MAX_GAPS, "items": {"type": "string", "maxLength": 300}, "description": "What you looked for and did not find."},
            "topic_id": {"type": "string", "maxLength": 57, "description": "A research topic id Delphi gave you, to add this as a new run of it."},
            "model": {"type": "string", "maxLength": 64, "description": "Your model name, self-reported."}
        },
        "required": ["question", "answer"],
        "additionalProperties": false
    })
}

fn bad<T>(m: impl Into<String>) -> Result<T, HostErr> {
    Err(HostErr::Invalid(m.into()))
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn capped(v: &Value, field: &str, max: usize) -> Result<String, HostErr> {
    let t = one_line(v.as_str().unwrap_or(""));
    if t.chars().count() > max {
        return bad(format!("`{field}` is at most {max} characters"));
    }
    Ok(t)
}

fn list<'a>(p: &'a Value, field: &str, max: usize, what: &str) -> Result<&'a [Value], HostErr> {
    match &p[field] {
        Value::Null => Ok(&[]),
        Value::Array(a) if a.len() > max => bad(format!("refused: `{field}` holds at most {max} {what} per call (the limit is {max}); this call has {}. Nothing was recorded", a.len())),
        Value::Array(a) => Ok(a),
        _ => bad(format!("`{field}` must be a list of at most {max} {what}")),
    }
}

/// Check and clean a call (§2 items 1 and 6). `redact` is the secret scrubber, applied to every text field. Fields the host has no
/// business setting (taint, status, confidence, author) are not read.
pub fn parse(p: &Value, redact: &dyn Fn(&str) -> String) -> Result<Parsed, HostErr> {
    let claims_in = list(p, "claims", MAX_CLAIMS, "claims")?;
    let sources_in = list(p, "sources", MAX_SOURCES, "sources")?;
    let searches_in = list(p, "searches", MAX_SEARCHES, "searches")?;
    let gaps_in = list(p, "gaps", MAX_GAPS, "gaps")?;
    let question = redact(&capped(&p["question"], "question", MAX_QUESTION)?);
    if question.is_empty() {
        return bad("`question` is required (at most 2000 characters)");
    }
    let Some(answer) = p["answer"].as_str() else { return bad("`answer` is required: your answer, as Markdown (at most 24 KiB)") };
    if answer.len() > MAX_ANSWER {
        return bad(format!("refused: `answer` is at most {MAX_ANSWER} bytes (24 KiB; the limit is {MAX_ANSWER}); this one is {}. Nothing was recorded", answer.len()));
    }
    let answer = redact(&answer.chars().map(|c| if c.is_control() && c != '\n' && c != '\t' { ' ' } else { c }).collect::<String>());
    let mut sources = Vec::new();
    for s in sources_in {
        let locator = redact(&capped(&s["locator"], "sources[].locator", MAX_LOCATOR)?);
        if locator.is_empty() {
            return bad("each source needs a `locator`: its URL, or a description of where it is");
        }
        let kind = s["kind"].as_str().unwrap_or("web").trim().to_string();
        if !KINDS.contains(&kind.as_str()) {
            return bad(format!("`sources[].kind` is one of {}", KINDS.join(", ")));
        }
        sources.push(Source { locator, title: redact(&capped(&s["title"], "sources[].title", MAX_TITLE)?), retrieved: capped(&s["retrieved"], "sources[].retrieved", 40)?, kind, private: s["private"].as_bool() == Some(true) });
    }
    let mut claims = Vec::new();
    for c in claims_in {
        let text = redact(&capped(&c["text"], "claims[].text", MAX_CLAIM_TEXT)?);
        if text.is_empty() {
            return bad("each claim needs `text` (at most 500 characters)");
        }
        let source = match &c["source"] {
            Value::Null => None,
            v => match v.as_u64().map(|i| i as usize).filter(|i| *i < sources.len()) {
                Some(i) => Some(i),
                None => return bad("`claims[].source` is the index of one of `sources`, counting from 0"),
            },
        };
        claims.push(Claim { text, quote: redact(&capped(&c["quote"], "claims[].quote", MAX_QUOTE)?), source });
    }
    let mut searches = Vec::new();
    for s in searches_in {
        let words = redact(&capped(&s["query"], "searches[].query", 300)?);
        if !words.is_empty() {
            searches.push(Search { place: capped(&s["where"], "searches[].where", 40)?, words });
        }
    }
    let gaps = gaps_in.iter().map(|g| capped(g, "gaps[]", 300).map(|t| redact(&t))).collect::<Result<Vec<_>, _>>()?.into_iter().filter(|g| !g.is_empty()).collect();
    let topic_id = match p["topic_id"].as_str().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if crate::research_record::valid_topic_id(t) => Some(t.to_string()),
        Some(_) => return bad("`topic_id` is a research topic id as Delphi gave it"),
        None => None,
    };
    Ok(Parsed { question, answer, sources, claims, searches, gaps, topic_id })
}

/// How many `record_research` calls a session made in the hour before `now` (unix seconds), from its log. The limit is
/// [`MAX_CALLS_PER_HOUR`]; the caller refuses the next one.
pub fn calls_in_last_hour(events: &[delphi_core::event::EventEnvelope], now: i64) -> usize {
    events.iter().filter(|e| e.kind == EVENT).filter_map(|e| delphi_core::time::parse_rfc3339(&e.timestamp)).filter(|t| now - *t < 3600).count()
}

/// The refusal for the call past the per-session limit.
pub fn rate_refusal() -> String {
    format!("refused: at most {MAX_CALLS_PER_HOUR} record_research calls per session per hour (the limit is {MAX_CALLS_PER_HOUR}); this session has made {MAX_CALLS_PER_HOUR}. Nothing was recorded; try again later")
}

/// The source's public address, when the daemon may try to read it: kind `web`, not marked private, an https URL.
pub fn public_url(s: &Source) -> Option<&str> {
    (s.kind == "web" && !s.private && crate::host_noted::https_host(&s.locator).is_some()).then_some(s.locator.as_str())
}

/// Is `quote` (three words or more) in `page`, compared as `propose_kb_entry` compares (case, punctuation and spacing folded)?
pub fn quote_in(page: &str, quote: &str) -> bool {
    let q = crate::host_noted::folded(quote);
    q.split(' ').count() >= 3 && format!(" {} ", crate::host_noted::folded(page)).contains(&format!(" {q} "))
}

/// A fetched page: the address the text was read at and the daemon's snapshot of it.
pub type Fetched = (String, crate::fact_fetch::Snapshot);
/// A fetcher: the daemon's own (`research_host::LiveWeb`), or a test's fake web.
pub type FetchFn<'a> = &'a dyn Fn(&str) -> Result<Fetched, String>;

/// Where and for whom a call is recorded.
pub struct Ctx<'a> {
    /// The install's home (the library is `<home>/library`).
    pub home: &'a Path,
    /// The folder the topic lives under: the project's root, or the workspace.
    pub root: &'a Path,
    /// The registered project, by name, when the host works inside one.
    pub project: Option<&'a str>,
    /// `host:<tool>/<model>`.
    pub label: &'a str,
    /// The run's id.
    pub run_id: &'a str,
    /// The host session carries the private-read mark of `specs/103` (§2 item 5): nothing is fetched and every source is private.
    pub read_marked: bool,
    /// The workspace's `[privacy] identifiers` (`query_guard::identifiers`), applied to what is stored (§8).
    pub identifiers: &'a [String],
}

/// The mark a claim that rests on a private source carries (§2 item 4).
pub const HOST_REPORTED: &str = "host-reported";
/// What an identifier is replaced with in stored text.
pub const MASK: &str = "[private identifier]";

/// `text` with each of the workspace's privacy identifiers (lower-case, as `query_guard::identifiers` gives them) replaced by
/// [`MASK`] where it stands as a word or words: no letter or digit on either side, case ignored (ASCII).
pub fn mask(text: &str, identifiers: &[String]) -> String {
    let mut out = text.to_string();
    for id in identifiers.iter().map(|i| i.trim()).filter(|i| !i.is_empty()) {
        let mut from = 0;
        while from < out.len() {
            let Some(at) = out[from..].char_indices().map(|(i, _)| from + i).find(|&i| out.get(i..i + id.len()).is_some_and(|s| s.eq_ignore_ascii_case(id))) else { break };
            let end = at + id.len();
            let before = out[..at].chars().next_back().is_some_and(char::is_alphanumeric);
            let after = out[end..].chars().next().is_some_and(char::is_alphanumeric);
            if before || after {
                from = at + out[at..].chars().next().map_or(1, char::len_utf8);
                continue;
            }
            out.replace_range(at..end, MASK);
            from = at + MASK.len();
        }
    }
    out
}

/// Is this a source the daemon must not read, whose claims stay private (§2 items 4 and 5)?
pub fn is_private(s: &Source, read_marked: bool) -> bool {
    read_marked || s.private || s.kind != "web"
}

/// The call with the identifiers masked in everything that is stored but the quotes, which are checked against a page first and masked
/// where they are stored without one.
fn masked(p: &Parsed, ids: &[String]) -> Parsed {
    if ids.is_empty() {
        return p.clone();
    }
    let m = |s: &str| mask(s, ids);
    Parsed {
        question: m(&p.question),
        answer: m(&p.answer),
        sources: p.sources.iter().map(|s| Source { locator: m(&s.locator), title: m(&s.title), ..s.clone() }).collect(),
        claims: p.claims.iter().map(|c| Claim { text: m(&c.text), ..c.clone() }).collect(),
        searches: p.searches.iter().map(|s| Search { place: m(&s.place), words: m(&s.words) }).collect(),
        gaps: p.gaps.iter().map(|g| m(g)).collect(),
        topic_id: p.topic_id.clone(),
    }
}

/// The fingerprint of a claim of the host's: its words, folded (`host_noted::fingerprint`).
fn fingerprint(text: &str) -> String {
    crate::host_noted::fingerprint(text)
}

/// The fingerprint a claim the daemon could not check is kept under: of a domain of its own, so no research run and no public claim
/// is merged with it by fingerprint.
fn private_fingerprint(text: &str) -> String {
    blake3::hash(format!("host-reported\0{}", fingerprint(text)).as_bytes()).to_hex()[..32].to_string()
}

/// The fixed word for a page the daemon did not read.
fn refusal_word(e: &str) -> &'static str {
    if e.contains("OffList") || e.contains("not_observed") {
        "not_fetchable"
    } else {
        "fetch_failed"
    }
}

/// Record one call (§2 items 2 and 3): the topic (a new one, or `parsed.topic_id`'s), one run with `runner: "host"`, the host's
/// searches as declared, each public source fetched once by `fetch` and kept in the library, each claim as a library claim. Returns the
/// reply (§2 item 7).
pub fn record(ctx: &Ctx<'_>, parsed: &Parsed, fetch: &dyn Fn(&str) -> Result<Fetched, String>) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let parsed = &masked(parsed, ctx.identifiers);
    let question = match &parsed.topic_id {
        Some(id) => {
            let t = Topic::at(ctx.root, id).filter(Topic::exists).ok_or_else(|| format!("no research topic `{id}` here to add to; leave out topic_id to start one"))?;
            t.read()?["question"].as_str().unwrap_or(&parsed.question).to_string()
        }
        None => parsed.question.clone(),
    };
    let topic = Topic::begin(ctx.root, &Begin { question: &question, asked_by: Asker::host(ctx.label), project: ctx.project, depth: delphi_core::research::Depth::Survey, run_id: ctx.run_id })?;
    let run = ctx.run_id;
    topic.runner(run, "host")?;
    topic.running(run)?;
    for s in &parsed.searches {
        topic.host_search(run, &s.place, &s.words)?;
    }
    let lib = Library::in_home(ctx.home);
    // One fetch per public source; what the daemon read is kept and is what the quotes are checked against.
    let mut read: Vec<Read> = Vec::new();
    let mut sources_out = Vec::new();
    for (i, s) in parsed.sources.iter().enumerate() {
        // A private source (marked so, of a kind other than web, carrying a private identifier, or any source of a session under the
        // private-read mark) is a pointer: locator, title, date. It is never fetched.
        if is_private(s, ctx.read_marked) || s.locator.contains(MASK) {
            let sid = lib.add_private_pointer(&s.kind, &s.locator, &s.title, &s.retrieved, None)?;
            topic.private_source(run, &sid, &s.kind)?;
            sources_out.push(json!({"index": i, "kind": s.kind, "fetched": false, "private": true, "source_id": sid}));
            read.push(Read::Private(sid, s.locator.clone()));
            continue;
        }
        let Some(url) = public_url(s) else {
            read.push(Read::Nothing);
            sources_out.push(json!({"index": i, "kind": s.kind, "fetched": false}));
            continue;
        };
        let got = fetch(url).and_then(|(at, snap)| {
            let text = snap.text.for_daemon().clone();
            let stored = lib.add_fetched(&at, &snap, &crate::library::AddOpts::default(), &crate::library::Budgets::default()).map_err(|_| "capture_failed".to_string())?;
            Ok((stored.id, at, text))
        });
        match got {
            Ok((sid, at, text)) => {
                let hash = lib.read_meta(&sid).and_then(|m| m["content_hash"].as_str().map(String::from));
                topic.page(run, &at, &sid, hash.as_deref())?;
                sources_out.push(json!({"index": i, "kind": s.kind, "fetched": true, "url": at, "source_id": sid}));
                read.push(Read::Page(sid, at, text));
            }
            Err(e) => {
                topic.refused(run, url, if e == "capture_failed" { "capture_failed" } else { refusal_word(&e) })?;
                sources_out.push(json!({"index": i, "kind": s.kind, "fetched": false, "url": url}));
                read.push(Read::Nothing);
            }
        }
    }
    let (mut ids, mut found, mut not_found, mut private_claims, mut claims_out) = (Vec::new(), 0usize, 0usize, 0usize, Vec::new());
    for c in &parsed.claims {
        let page = c.source.and_then(|i| read.get(i)).unwrap_or(&Read::Nothing);
        let (written, mark) = match page {
            Read::Page(sid, at, text) if quote_in(text, &c.quote) => {
                let fp = fingerprint(&c.text);
                let anchors = [AnchorSpec { source_id: sid.clone(), locator: Some(at.clone()), quote: c.quote.clone(), prefix: None, suffix: None }];
                (lib.add_research_claim(&ResearchClaim { text: &c.text, fingerprint: &fp, project: ctx.project, topic: Some(&topic.id), anchors: &anchors }).map(|(v, _)| v), None)
            }
            // A claim on a private source: kept as the host reported it, anchored to the pointer with its quote, private to its place.
            Read::Private(sid, locator) => {
                let fp = private_fingerprint(&c.text);
                let quote = mask(&c.quote, ctx.identifiers);
                let anchors: Vec<AnchorSpec> = (!quote.trim().is_empty()).then(|| AnchorSpec { source_id: sid.clone(), locator: Some(locator.clone()), quote, prefix: None, suffix: None }).into_iter().collect();
                (lib.add_host_claim(&ResearchClaim { text: &c.text, fingerprint: &fp, project: ctx.project, topic: Some(&topic.id), anchors: &anchors }, HOST_REPORTED), Some(HOST_REPORTED))
            }
            _ => {
                let fp = private_fingerprint(&c.text);
                (lib.add_host_claim(&ResearchClaim { text: &c.text, fingerprint: &fp, project: ctx.project, topic: Some(&topic.id), anchors: &[] }, NOT_FOUND), Some(NOT_FOUND))
            }
        };
        match written {
            Ok(v) => {
                let id = v["id"].as_str().unwrap_or("").to_string();
                match mark {
                    None => found += 1,
                    Some(HOST_REPORTED) => private_claims += 1,
                    Some(_) => not_found += 1,
                }
                claims_out.push(json!({"id": id, "quote_found": mark.is_none(), "mark": mark}));
                if !id.is_empty() && !ids.contains(&id) {
                    ids.push(id);
                }
            }
            Err(e) => claims_out.push(json!({"id": null, "quote_found": false, "error": crate::research::clip(&e, 200)})),
        }
    }
    let result = json!({
        "runner": "host", "answer": parsed.answer, "requested_by": ctx.label, "knowledge_status": "noted", "note": NOT_VALIDATED,
        "claims": claims_out, "library_claims": ids, "sources": sources_out, "quotes_found": found, "quotes_not_found": not_found, "private_claims": private_claims,
    });
    let private_sources = read.iter().filter(|r| matches!(r, Read::Private(..))).count();
    topic.finish(run, "done", Some(&result), &ids, None, started.elapsed().as_millis() as u64)?;
    let gaps = parsed.gaps.clone();
    topic.amend(|rec| rec["gaps"] = json!(gaps))?;
    Ok(json!({
        "recorded": true, "topic_id": topic.id, "research_id": run, "claims": parsed.claims.len(), "library_claims": ids.len(),
        "quotes_found": found, "quotes_not_found": not_found, "private_sources": private_sources, "private_claims": private_claims, "fetched_nothing": ctx.read_marked, "status": NOT_VALIDATED,
        "note": "not validated. Delphi fetched each public source itself and looked for each quote in what it read: a quote it found is an anchor into the page it keeps; one it did not find (or could not look for) is kept as `host-reported, not found`. A private, internal, file or person source was not fetched: only its locator, title and date are kept, and its claims stay private and count for no corroboration. Read it back with research_result and this research_id, or find it with search_kb.",
    }))
}

/// What the daemon has of one source of a call.
enum Read {
    /// It read the page: the library source, the address read, the text.
    Page(String, String, String),
    /// A private pointer: the library source and the locator.
    Private(String, String),
    /// Nothing (not public, or the fetch failed).
    Nothing,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const PAGE_A: &str = "Zebrafish are kept at 28 degrees in most labs. Water is changed weekly.";
    const PAGE_B: &str = "Adult zebrafish eat twice a day. Larvae need paramecia in the first week.";

    fn id(s: &str) -> String {
        s.to_string()
    }

    fn call() -> Value {
        json!({
            "question": "How are zebrafish kept in the lab?",
            "answer": "At 28 degrees, fed twice a day.",
            "sources": [
                {"locator": "https://fish.example.org/a", "title": "Husbandry", "retrieved": "2026-11-15", "kind": "web", "private": false},
                {"locator": "https://fish.example.org/b", "title": "Feeding", "retrieved": "2026-11-15", "kind": "web", "private": false}
            ],
            "claims": [
                {"text": "Zebrafish are kept at 28 degrees", "quote": "kept at 28 degrees in most labs", "source": 0},
                {"text": "Zebrafish water is changed weekly", "quote": "Water is changed weekly", "source": 0},
                {"text": "Adult zebrafish eat twice a day", "quote": "Adult zebrafish eat twice a day", "source": 1},
                {"text": "Larvae eat brine shrimp", "quote": "larvae eat brine shrimp from day one", "source": 1}
            ],
            "searches": [{"where": "web", "query": "zebrafish husbandry temperature"}],
            "gaps": ["no source on lighting cycles"],
            "taint": "user_ratified", "status": "validated"
        })
    }

    /// The fake web: it serves the two pages and records every address it is asked for.
    fn web<'a>(asked: &'a RefCell<Vec<String>>) -> impl Fn(&str) -> Result<Fetched, String> + 'a {
        move |url: &str| {
            asked.borrow_mut().push(url.to_string());
            let text = match url {
                "https://fish.example.org/a" => PAGE_A,
                "https://fish.example.org/b" => PAGE_B,
                _ => return Err("OffList".into()),
            };
            Ok((url.to_string(), crate::fact_fetch::test_text_snapshot(url, text)))
        }
    }

    struct Fx {
        _t: tempfile::TempDir,
        home: std::path::PathBuf,
        ws: std::path::PathBuf,
    }

    fn fx() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().to_path_buf();
        let ws = home.join("workspaces").join("w");
        std::fs::create_dir_all(&ws).unwrap();
        Fx { _t: t, home, ws }
    }

    fn ctx<'a>(f: &'a Fx, run: &'a str) -> Ctx<'a> {
        Ctx { home: &f.home, root: &f.ws, project: None, label: "host:claude-code/opus", run_id: run, read_marked: false, identifiers: &[] }
    }

    fn private_call() -> Value {
        json!({
            "question": "What does our tank SOP say?",
            "answer": "The heron lab keeps tanks at 28 degrees.",
            "sources": [
                {"locator": "https://fish.example.org/a", "title": "Husbandry", "kind": "web", "private": true},
                {"locator": "wiki page Heron Tank SOP", "title": "Heron tank SOP", "retrieved": "2026-11-14", "kind": "internal"},
                {"locator": "talk with the lab manager", "title": "conversation", "kind": "person"}
            ],
            "claims": [
                {"text": "Zebrafish are kept at 28 degrees", "quote": "kept at 28 degrees in most labs", "source": 0},
                {"text": "Heron tanks are cleaned on Mondays", "quote": "Heron tanks are cleaned every Monday", "source": 1},
                {"text": "The lab manager checks tanks daily", "quote": "I check them every day", "source": 2}
            ]
        })
    }

    /// AC-2: a source marked private (or internal, or a person) is never fetched; it is a pointer with its locator, title and date;
    /// its claims are host-reported, private to their place, and raise no origin count. The privacy identifiers are masked in what is kept.
    #[test]
    fn a_private_source_is_not_fetched_and_its_claims_raise_no_origin() {
        let f = fx();
        // A public claim with the same words already stands on a page the daemon read: one origin.
        let asked = RefCell::new(Vec::new());
        record(&ctx(&f, "h-1"), &parse(&call(), &id).unwrap(), &web(&asked)).unwrap();
        let lib = Library::in_home(&f.home);
        let public = lib.claims().into_iter().find(|c| c["text"] == "Zebrafish are kept at 28 degrees" && c["scope"] == "team").unwrap();
        assert_eq!(public["support"]["origins"], 1);
        asked.borrow_mut().clear();
        let ids = vec!["heron".to_string()];
        let c = Ctx { identifiers: &ids, ..ctx(&f, "h-2") };
        let reply = record(&c, &parse(&private_call(), &id).unwrap(), &web(&asked)).unwrap();
        assert!(asked.borrow().is_empty(), "no private source is fetched: {:?}", asked.borrow());
        assert_eq!((reply["private_sources"].as_u64(), reply["private_claims"].as_u64(), reply["quotes_found"].as_u64()), (Some(3), Some(3), Some(0)), "{reply}");
        let topic = Topic::at(&f.ws, reply["topic_id"].as_str().unwrap()).unwrap();
        let rec = topic.read().unwrap();
        assert_eq!((rec["private"].as_bool(), rec["private_sources"].as_array().unwrap().len()), (Some(true), 3), "the record is private and names the pointers");
        assert!(!serde_json::to_string(&rec).unwrap().to_lowercase().contains("heron"), "the identifier is masked in the record");
        assert!(!topic.answer().to_lowercase().contains("heron"));
        for s in rec["private_sources"].as_array().unwrap() {
            let meta = lib.read_meta(s["source_id"].as_str().unwrap()).unwrap();
            assert_eq!((meta["private"].as_bool(), meta["capture_level"].as_str(), meta["canonical_url"].is_null()), (Some(true), Some("pointer"), true), "{meta}");
            assert!(meta["locator"].is_string() && meta["title"].is_string() && meta["retrieved"].is_string());
            assert!(!lib.source_dir(s["source_id"].as_str().unwrap()).join("source.md").exists(), "no body is kept");
            assert!(!meta["locator"].as_str().unwrap().to_lowercase().contains("heron"));
        }
        let private: Vec<Value> = lib.claims().into_iter().filter(|c| c["host_reported"] == json!([HOST_REPORTED])).collect();
        assert_eq!(private.len(), 3);
        for c in &private {
            assert_eq!((c["scope"].as_str(), c["support"]["origins"].as_u64().unwrap_or(0)), (Some("personal"), 0), "private to its place, no origin: {c}");
            assert!(lib.rests_on_private(c), "anchored only to a private pointer");
            assert!(!c["text"].as_str().unwrap().to_lowercase().contains("heron") && !c["anchors"][0]["quote"].as_str().unwrap_or("").to_lowercase().contains("heron"), "{c}");
        }
        let public = lib.claim(public["id"].as_str().unwrap()).unwrap();
        assert_eq!(public["support"]["origins"], 1, "the private source added no origin to the public claim with the same words");
        assert!(public["anchors"].as_array().unwrap().iter().all(|a| !lib.read_meta(a["source_id"].as_str().unwrap()).is_some_and(|m| crate::library::is_private(&m))));
        // In a project, the private claims are the project's.
        let proj = f.home.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let pc = Ctx { root: &proj, project: Some("proj"), ..ctx(&f, "h-3") };
        record(&pc, &parse(&private_call(), &id).unwrap(), &web(&asked)).unwrap();
        assert!(lib.claims().iter().any(|c| c["scope"] == "project" && c["project"] == "proj" && c["host_reported"] == json!([HOST_REPORTED])));
    }

    /// AC-3: under the private-read mark the call makes no request at all, and every source is kept private.
    #[test]
    fn under_the_private_read_mark_nothing_is_fetched_and_every_source_is_private() {
        let f = fx();
        let asked = RefCell::new(Vec::new());
        let c = Ctx { read_marked: true, ..ctx(&f, "h-1") };
        let reply = record(&c, &parse(&call(), &id).unwrap(), &web(&asked)).unwrap();
        assert!(asked.borrow().is_empty(), "no network request under the mark: {:?}", asked.borrow());
        assert_eq!((reply["private_sources"].as_u64(), reply["quotes_found"].as_u64(), reply["fetched_nothing"].as_bool()), (Some(2), Some(0), Some(true)), "{reply}");
        let lib = Library::in_home(&f.home);
        assert!(lib.list().iter().all(|(_, m)| crate::library::is_private(m)), "every source is a private pointer");
        assert!(lib.claims().iter().all(|c| c["scope"] == "personal"), "and no claim is offered beyond this install");
    }

    #[test]
    fn identifiers_are_masked_as_words_only() {
        let ids = vec!["heron".to_string(), "blue lab".to_string()];
        assert_eq!(mask("Heron's tank, project-heron, herons, the Blue Lab", &ids), "[private identifier]'s tank, project-[private identifier], herons, the [private identifier]");
        assert_eq!(mask("nothing here", &ids), "nothing here");
    }

    /// AC-1: two public sources and four claims leave a topic record, two library sources and four library claims; the quotes the
    /// pages hold are anchors, and the one they do not is kept as host-reported, not found.
    #[test]
    fn a_call_with_two_public_sources_and_four_claims_leaves_a_topic_two_sources_and_four_claims() {
        let f = fx();
        let parsed = parse(&call(), &id).unwrap();
        let asked = RefCell::new(Vec::new());
        let reply = record(&ctx(&f, "h-1"), &parsed, &web(&asked)).unwrap();
        assert_eq!(*asked.borrow(), ["https://fish.example.org/a", "https://fish.example.org/b"], "each public source is fetched once");
        assert_eq!((reply["quotes_found"].as_u64(), reply["quotes_not_found"].as_u64(), reply["status"].as_str()), (Some(3), Some(1), Some(NOT_VALIDATED)), "{reply}");
        let topic = Topic::at(&f.ws, reply["topic_id"].as_str().unwrap()).unwrap();
        let rec = topic.read().unwrap();
        assert_eq!((rec["runner"].as_str(), rec["runs"][0]["runner"].as_str(), rec["state"].as_str()), (Some("host"), Some("host"), Some("done")));
        assert_eq!(rec["asked_by"], json!({"kind": "host", "label": "host:claude-code/opus"}), "the host and model asked");
        assert_eq!(rec["pages"].as_array().unwrap().len(), 2);
        assert_eq!(rec["queries"][0], json!({"run": "h-1", "query": "zebrafish husbandry temperature", "purpose": "host", "provider": "web", "results": null}));
        assert_eq!(rec["gaps"], json!(["no source on lighting cycles"]));
        assert_eq!(rec["claims"].as_array().unwrap().len(), 4);
        assert_eq!(topic.answer(), "At 28 degrees, fed twice a day.");
        let lib = Library::in_home(&f.home);
        assert_eq!(lib.list().len(), 2, "two library sources");
        let claims = lib.claims();
        assert_eq!(claims.len(), 4, "four library claims");
        for c in &claims {
            assert_eq!(crate::library::claims::status(c), delphi_core::knowledge_status::Status::Noted, "what the host says about status is not read: {c}");
        }
        let anchored: Vec<&Value> = claims.iter().filter(|c| c["anchors"].as_array().is_some_and(|a| !a.is_empty())).collect();
        assert_eq!(anchored.len(), 3, "the quotes present on the pages are anchors");
        assert!(anchored.iter().all(|c| c["scope"] == "team" && c["anchors"][0]["source_id"].as_str().is_some_and(|s| lib.read_meta(s).is_some())));
        let missing = claims.iter().find(|c| c["text"] == "Larvae eat brine shrimp").unwrap();
        assert_eq!((missing["host_reported"].clone(), missing["scope"].as_str()), (json!([NOT_FOUND]), Some("personal")), "kept, marked, and offered to no one else");
        // A second call adds a run to the same topic and merges the same claims: no second claim, no second source.
        let mut again = call();
        again["topic_id"] = json!(topic.id);
        record(&ctx(&f, "h-2"), &parse(&again, &id).unwrap(), &web(&asked)).unwrap();
        assert_eq!((lib.claims().len(), lib.list().len(), topic.read().unwrap()["runs"].as_array().unwrap().len()), (4, 2, 2));
        let mut unknown = call();
        unknown["topic_id"] = json!("no-such-topic-deadbeef");
        assert!(record(&ctx(&f, "h-3"), &parse(&unknown, &id).unwrap(), &web(&asked)).unwrap_err().contains("no research topic"));
    }

    /// AC-6: the 41st claim (and the 41st source) in one call is refused with the limit named; nothing is parsed past it.
    #[test]
    fn the_41st_claim_or_source_is_refused_with_the_limit_named() {
        let mut p = call();
        p["claims"] = json!((0..41).map(|i| json!({"text": format!("claim {i}")})).collect::<Vec<_>>());
        let e = parse(&p, &id).unwrap_err().to_string();
        assert!(e.contains("at most 40 claims") && e.contains("the limit is 40"), "{e}");
        p["claims"] = json!((0..40).map(|i| json!({"text": format!("claim {i}")})).collect::<Vec<_>>());
        assert!(parse(&p, &id).is_ok(), "forty is allowed");
        p["sources"] = json!((0..41).map(|i| json!({"locator": format!("https://x.example/{i}")})).collect::<Vec<_>>());
        assert!(parse(&p, &id).unwrap_err().to_string().contains("at most 40 sources"));
        let mut big = call();
        big["answer"] = json!("x".repeat(MAX_ANSWER + 1));
        assert!(parse(&big, &id).unwrap_err().to_string().contains("24 KiB"));
        assert!(rate_refusal().contains("at most 10 record_research calls per session per hour"));
        let mut bad_index = call();
        bad_index["claims"][0]["source"] = json!(7);
        assert!(parse(&bad_index, &id).is_err());
        let mut bad_kind = call();
        bad_kind["sources"][0]["kind"] = json!("telepathy");
        assert!(parse(&bad_kind, &id).is_err());
    }

    #[test]
    fn the_hourly_limit_counts_this_sessions_calls_of_the_last_hour() {
        let at = |secs: i64| {
            let mut e = delphi_core::event::EventEnvelope::new("host-abcdef0123456789", EVENT, json!({}));
            e.timestamp = delphi_core::time::format_unix_millis(secs, 0);
            e
        };
        let now = 1_800_000_000;
        let events: Vec<_> = (0..12).map(|i| at(now - i * 400)).collect();
        assert_eq!(calls_in_last_hour(&events, now), 9, "calls older than an hour do not count");
    }

    #[test]
    fn a_quote_is_found_as_propose_kb_entry_finds_it() {
        assert!(quote_in(PAGE_A, "KEPT at 28 degrees, in most labs"));
        assert!(!quote_in(PAGE_A, "28 degrees"), "two words are not a quote");
        assert!(!quote_in(PAGE_A, "kept at 30 degrees"));
    }
}
