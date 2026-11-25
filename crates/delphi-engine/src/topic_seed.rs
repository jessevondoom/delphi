//! A topic page's seed (`specs/139` §2.1; slice A): what the research on a topic wrote, assembled by code, link first and search last.
//!
//! A **seed** is an ordered list of **spans**. A span is a run of whole lines of one stored document, cited by the record label a gathered
//! passage has (`topic_gather::label_of`): `[s-kb-…]`, or `[f-…]` for a finding. A live claim of a followed research topic is a span too,
//! cited as itself (`[c-…]`). The rows, in order (§2.1 item 2):
//!
//! 1. the answer of each research topic the page follows (`answer.md`, and each run's own answer the record keeps when `answer.md` does
//!    not already hold it), newest topic first, split at `## ` headings, in file order;
//! 2. the summary of each library source the topic's records read (`summary.md` beside the source's record, `specs/142` §2.6) and its
//!    kept excerpts, each at most [`SOURCE_PIECE_MAX`] characters. **A source's own text is never a span**; a source with neither is
//!    only named ([`Seed::named_sources`]);
//! 3. the findings of a session that ran or recorded research on the topic (the run pointer files `research/runs/<session>.<run>.json`
//!    name the session);
//! 4. kept summary pages (`local/summaries/`, `kb_import::is_kept`) whose bibliography names a page or source the topic's records read;
//! 5. a research file an ingest run recorded as the topic's answer by reference (`specs/113` §3 item 1), when its library source is not
//!    private. The ingest makes that source private, so today this row gives nothing (§1.1 and item 3 of §2.1 win);
//! 6. what today's search returns (`topic_gather::gather`), to fill what is left; never a library source's text or excerpts;
//! 7. the live claims of the followed topics, when there are any.
//!
//! **Caps** (§2.1 item 3): at most [`MAX_SPANS`] spans and [`MAX_BYTES`] of text; a span is at most [`SPAN_MAX`] and is cut at a
//! paragraph (a longer section becomes several spans). Row 1 is never dropped. Inside a row the order is by path, then line (row 1:
//! newest topic first, then file order), so the same files give the same seed, byte for byte.
//!
//! **Left out** (§2.1 item 4), as today's gather leaves out: a private research record, a hit flagged `internal`, another project's hit,
//! staged and provisional notes, episodes, and any generated page. Seed text is data from outside: it keeps its document's status and
//! passes the workspace's identifier mask (`host_research::mask`). Assembling a seed reads only; it calls no model.
//!
//! **The seed page** (§2.2; slice B): [`seed_page`] renders the seed by code (a lead, the answer under its own headings, then
//! `## Sources read`, `## Findings`, `## Related notes` and `## Claims`), each block a span's own words, cleaned by [`clean`], followed
//! by its marker; then checks it. The check drops a block that is not found letter for letter in its cleaned span, or that comes from a
//! document a seed leaves out, and passes the whole page through the held-text predicate ([`HeldText`], `specs/142` §2.5 item 4): a
//! block that quotes a held source past the bound is shortened and marked [`SHORTENED`], or dropped. A seed with no block left gives
//! no page.

use crate::library::pages::{record_status, KbRecord};
use crate::library::{is_private, Library};
use crate::topic_gather::{label_of, text_hash};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// The most spans a seed holds.
pub const MAX_SPANS: usize = 40;
/// The most text a seed holds, in bytes.
pub const MAX_BYTES: usize = 64 * 1024;
/// The longest span, in bytes; a longer section is cut at a paragraph into several.
pub const SPAN_MAX: usize = 8 * 1024;
/// The longest piece of a library source a seed holds: its summary, or one kept excerpt (§2.1 item 3), in characters.
pub const SOURCE_PIECE_MAX: usize = 300;
/// What a topic with no span is listed as (§2.2 item 4).
pub const WAITING: &str = "waiting for material";

/// The rows of §2.1 item 2, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Row {
    Answer = 1,
    SourceRead = 2,
    Finding = 3,
    SharedSource = 4,
    IngestFile = 5,
    Search = 6,
    Claim = 7,
}

impl Row {
    pub const ALL: [Row; 7] = [Row::Answer, Row::SourceRead, Row::Finding, Row::SharedSource, Row::IngestFile, Row::Search, Row::Claim];

    /// The `link` word of the table of §2.1 item 2.
    pub fn link(self) -> &'static str {
        match self {
            Row::Answer | Row::SourceRead | Row::IngestFile | Row::Claim => "research_topic",
            Row::Finding => "session",
            Row::SharedSource => "shared_source",
            Row::Search => "search",
        }
    }

    pub fn number(self) -> usize {
        self as usize
    }

    pub fn from_number(n: u64) -> Option<Row> {
        Row::ALL.into_iter().find(|r| r.number() as u64 == n)
    }

    pub fn name(self) -> &'static str {
        match self {
            Row::Answer => "answer",
            Row::SourceRead => "sources read",
            Row::Finding => "findings",
            Row::SharedSource => "summary pages",
            Row::IngestFile => "ingested files",
            Row::Search => "search",
            Row::Claim => "claims",
        }
    }
}

/// One span of a seed.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub row: Row,
    /// `s-kb-…`, `f-…`, or a claim's own id.
    pub label: String,
    /// `research answer`, `source summary`, `excerpt`, `finding`, `summary page`, `research file`, `passage` or `claim`.
    pub kind: String,
    /// Where the span is, as the index names documents (`research/topics/<id>/answer.md`, `missions/<m>/findings/<f>.md`, …).
    pub rel_path: String,
    pub start_line: usize,
    pub end_line: usize,
    /// The heading the span sits under, else its document's title.
    pub title: String,
    /// The span's words, masked; cleaned only when rendered and compared (`specs/139` §2.2 item 3).
    pub text: String,
    /// [`text_hash`] of `text`: a later due check compares it.
    pub hash: String,
    /// The shown status, from the document's record, never from its words.
    pub status: String,
    /// The research topic a row-1 span belongs to.
    pub topic: Option<String>,
}

impl Span {
    /// The record a page check resolves the span's marker to. `None` for a claim, which resolves as itself.
    pub fn to_record(&self) -> Option<KbRecord> {
        (self.row != Row::Claim).then(|| KbRecord {
            label: self.label.clone(),
            kind: self.kind.clone(),
            title: self.title.clone(),
            locator: format!("{}#L{}-L{}", self.rel_path, self.start_line, self.end_line),
            origin: format!("seed row {} ({})", self.row.number(), self.row.link()),
            status: self.status.clone(),
            text: self.text.clone(),
            hash: self.hash.clone(),
            rel_path: self.rel_path.clone(),
            start_line: self.start_line,
        })
    }

    /// What a page's record keeps of the span: where it is and its hash, never its text.
    pub fn to_json(&self) -> Value {
        json!({"label": self.label, "row": self.row.number(), "link": self.row.link(), "kind": self.kind, "rel_path": self.rel_path, "start_line": self.start_line, "end_line": self.end_line, "hash": self.hash})
    }
}

/// A topic's seed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seed {
    pub spans: Vec<Span>,
    /// Library sources the topic's records read that gave no span (no summary, no excerpt): named in the bibliography only.
    pub named_sources: Vec<String>,
    /// Spans each row would give before the caps.
    pub offered: [usize; 7],
    /// Every library source the followed topics' records read: the sources the seed page is checked against besides the block index.
    pub sources_read: Vec<String>,
}

impl Seed {
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The spans kept, by row.
    pub fn counts(&self) -> [usize; 7] {
        let mut out = [0usize; 7];
        for s in &self.spans {
            out[s.row.number() - 1] += 1;
        }
        out
    }

    pub fn bytes(&self) -> usize {
        self.spans.iter().map(|s| s.text.len()).sum()
    }

    /// The knowledge-base records a page check is given for the seed's spans.
    pub fn records(&self) -> Vec<KbRecord> {
        self.spans.iter().filter_map(Span::to_record).collect()
    }
}

/// What an assembly may read besides the files.
pub struct Assemble<'a> {
    /// The workspace's `[privacy] identifiers`, masked in every span.
    pub identifiers: &'a [String],
    /// Whether row 6 asks today's search. A due check leaves it off: it reads the linked rows alone.
    pub search: bool,
}

/// A followed research topic, as the seed reads it.
struct Followed {
    topic: crate::research_record::Topic,
    rec: Value,
    /// Where the topic's folder is, as the index names it: `research/topics/<id>` or `@<project>/research/topics/<id>`.
    rel: String,
}

/// The public research topics `side` follows, newest first (then by id).
fn followed(side: &Value, ws: &Path) -> Vec<Followed> {
    let projects = crate::scope::project_list(ws);
    let mut out: Vec<Followed> = Vec::new();
    for tid in side["research_topics"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let Some(topic) = crate::research_record::find(ws, tid) else { continue };
        let Ok(rec) = topic.read() else { continue };
        if rec["private"] != false || out.iter().any(|f| f.topic.id == topic.id) {
            continue;
        }
        let rel = match topic.dir.strip_prefix(ws) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => {
                let project = projects.iter().find(|(_, root)| topic.dir.starts_with(root)).map(|(n, _)| n.clone()).unwrap_or_else(|| "project".into());
                format!("@{}/research/topics/{}", project, topic.id)
            }
        };
        out.push(Followed { topic, rec, rel });
    }
    out.sort_by(|a, b| b.rec["updated"].as_str().unwrap_or("").cmp(a.rec["updated"].as_str().unwrap_or("")).then_with(|| a.topic.id.cmp(&b.topic.id)));
    out
}

fn norm_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first `n` characters of `s`, trimmed: a piece of a source is cut, never padded.
fn chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>().trim().to_string()
}

/// One section of a Markdown body: the heading it sits under (if any), the line its text starts on (1-based, in the whole file) and
/// its lines.
struct Section {
    heading: Option<String>,
    start: usize,
    lines: Vec<String>,
}

/// `body` split at `## ` headings. `first_line` is the file's line number of the body's first line. Blank sections are left out.
fn sections(body: &str, first_line: usize) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut cur = Section { heading: None, start: first_line, lines: Vec::new() };
    let mut fence = false;
    for (i, l) in body.lines().enumerate() {
        if l.trim_start().starts_with("```") {
            fence = !fence;
        }
        if !fence && l.starts_with("## ") {
            out.push(std::mem::replace(&mut cur, Section { heading: Some(l[3..].trim().to_string()), start: first_line + i + 1, lines: Vec::new() }));
            continue;
        }
        if cur.lines.is_empty() && l.trim().is_empty() {
            cur.start = first_line + i + 1;
            continue;
        }
        cur.lines.push(l.to_string());
    }
    out.push(cur);
    for s in &mut out {
        while s.lines.last().is_some_and(|l| l.trim().is_empty()) {
            s.lines.pop();
        }
    }
    out.retain(|s| s.lines.iter().any(|l| !l.trim().is_empty()));
    out
}

/// A section's lines cut into pieces of at most [`SPAN_MAX`] bytes, each ending at a paragraph where one fits: `(start line, lines)`.
/// A paragraph longer than a span is cut at a line, and a line longer than a span at a character.
fn pieces(s: &Section) -> Vec<(usize, usize, Vec<String>)> {
    // The paragraphs, each with its first line's number; blank lines between them are not kept.
    let mut paras: Vec<(usize, Vec<String>)> = Vec::new();
    for (i, l) in s.lines.iter().enumerate() {
        if l.trim().is_empty() {
            continue;
        }
        let blank_before = i == 0 || s.lines[i - 1].trim().is_empty();
        match paras.last_mut() {
            Some((_, p)) if !blank_before => p.push(l.clone()),
            _ => paras.push((s.start + i, vec![l.clone()])),
        }
    }
    // Lines and paragraphs too long for one span are cut first.
    let mut units: Vec<(usize, Vec<String>, bool)> = Vec::new(); // (line, lines, starts a paragraph)
    for (start, p) in paras {
        let size: usize = p.iter().map(|l| l.len() + 1).sum();
        if size <= SPAN_MAX {
            units.push((start, p, true));
            continue;
        }
        let mut first = true;
        for (k, l) in p.into_iter().enumerate() {
            let mut t = l.as_str();
            while t.len() > SPAN_MAX {
                let mut at = SPAN_MAX;
                while !t.is_char_boundary(at) {
                    at -= 1;
                }
                units.push((start + k, vec![t[..at].to_string()], first));
                first = false;
                t = &t[at..];
            }
            units.push((start + k, vec![t.to_string()], first));
            first = false;
        }
    }
    let mut out: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut size = 0usize;
    for (start, lines, para) in units {
        let end = start + lines.len() - 1;
        let add: usize = lines.iter().map(|l| l.len() + 1).sum::<usize>() + if para { 1 } else { 0 };
        match out.last_mut() {
            Some((_, e, cur)) if size + add <= SPAN_MAX => {
                if para {
                    cur.push(String::new());
                }
                cur.extend(lines);
                *e = end;
                size += add;
            }
            _ => {
                size = add;
                out.push((start, end, lines));
            }
        }
    }
    out
}

/// The spans of a Markdown body, one or more per `## ` section.
#[allow(clippy::too_many_arguments)]
fn body_spans(row: Row, finding: bool, kind: &str, rel: &str, body: &str, first_line: usize, title: &str, status: &str, topic: Option<&str>, mask: &dyn Fn(&str) -> String) -> Vec<Span> {
    let mut out = Vec::new();
    // Masked before it is cut, so a span is within its cap as stored; the mask keeps every line where it was.
    let body = mask(body);
    for s in sections(&body, first_line) {
        let heading = s.heading.clone().unwrap_or_else(|| title.to_string());
        for (start, end, lines) in pieces(&s) {
            let text = lines.join("\n");
            if text.trim().is_empty() {
                continue;
            }
            out.push(Span { row, label: label_of(finding, rel, start), kind: kind.into(), rel_path: rel.into(), start_line: start, end_line: end, title: mask(&heading), hash: text_hash(&text), text, status: status.into(), topic: topic.map(String::from) });
        }
    }
    out
}

/// The body of a Markdown file below its frontmatter, and the line it starts on (1-based).
fn below_frontmatter(file: &str) -> (&str, usize) {
    if let Some(rest) = file.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            let body = &rest[end + 5..];
            let line = file[..file.len() - body.len()].lines().count() + 1;
            return (body, line);
        }
    }
    (file, 1)
}

/// The body below its first `# ` heading (when it opens with one), its line, and the heading.
fn below_title(body: &str, line: usize) -> (&str, usize, Option<String>) {
    let lead = body.lines().take_while(|l| l.trim().is_empty()).count();
    let mut rest = body;
    for _ in 0..lead {
        rest = rest.split_once('\n').map_or("", |(_, r)| r);
    }
    match rest.split_once('\n') {
        Some((first, more)) if first.starts_with("# ") => (more, line + lead + 1, Some(first[2..].trim().to_string())),
        None if rest.starts_with("# ") => ("", line + lead + 1, Some(rest[2..].trim().to_string())),
        _ => (rest, line + lead, None),
    }
}

fn row_answers(f: &Followed, mask: &dyn Fn(&str) -> String) -> Vec<Span> {
    let question = f.rec["question"].as_str().unwrap_or(&f.topic.id).to_string();
    let mut out = Vec::new();
    let file = std::fs::read_to_string(f.topic.answer_path()).unwrap_or_default();
    let (body, line) = below_frontmatter(&file);
    let (body, line, _) = below_title(body, line);
    let held = norm_ws(body);
    out.extend(body_spans(Row::Answer, false, "research answer", &format!("{}/answer.md", f.rel), body, line, &question, "○ not validated", Some(&f.topic.id), mask));
    // Each run's own answer the record keeps, newest first, when `answer.md` does not already hold it.
    let results: Vec<&Value> = f.rec["results"].as_array().into_iter().flatten().collect();
    for e in results.into_iter().rev() {
        let (Some(run), Some(answer)) = (e["run"].as_str(), e["result"]["answer"].as_str()) else { continue };
        let a = norm_ws(answer);
        if a.is_empty() || held.contains(&a) {
            continue;
        }
        out.extend(body_spans(Row::Answer, false, "research answer", &format!("{}/results/{}", f.rel, run), answer, 1, &question, "○ not validated", Some(&f.topic.id), mask));
    }
    out
}

/// The source ids and page addresses the followed topics' records read.
fn read_by(fs: &[Followed]) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut ids, mut urls) = (BTreeSet::new(), BTreeSet::new());
    for f in fs {
        for p in f.rec["pages"].as_array().into_iter().flatten() {
            if let Some(id) = p["source_id"].as_str().filter(|s| !s.is_empty()) {
                ids.insert(id.to_string());
            }
            if let Some(u) = p["url"].as_str().filter(|s| !s.is_empty()) {
                urls.insert(u.to_string());
            }
        }
    }
    (ids, urls)
}

fn row_sources(lib: &Library, ids: &BTreeSet<String>, mask: &dyn Fn(&str) -> String, named: &mut Vec<String>) -> Vec<Span> {
    let mut out = Vec::new();
    for id in ids {
        let Some(meta) = lib.read_meta(id) else { continue };
        if is_private(&meta) || !meta["purged"].is_null() || !meta["retired"].is_null() {
            continue;
        }
        let status = record_status(meta["status"].as_str().unwrap_or(""));
        let title = meta["title"].as_str().unwrap_or(id).chars().take(200).collect::<String>();
        let mut gave = false;
        if let Ok(file) = std::fs::read_to_string(lib.source_dir(id).join("summary.md")) {
            let (body, line) = below_frontmatter(&file);
            let text = mask(&chars(body.trim(), SOURCE_PIECE_MAX));
            if !text.is_empty() {
                let rel = format!("library/sources/{}/summary.md", id);
                let lines = text.lines().count().max(1);
                out.push(Span { row: Row::SourceRead, label: label_of(false, &rel, line), kind: "source summary".into(), rel_path: rel, start_line: line, end_line: line + lines - 1, title: mask(&title), hash: text_hash(&text), text, status: status.clone(), topic: None });
                gave = true;
            }
        }
        for (i, e) in lib.excerpts(id).iter().enumerate() {
            let text = mask(&chars(e["text"].as_str().unwrap_or(""), SOURCE_PIECE_MAX));
            if text.is_empty() {
                continue;
            }
            let rel = format!("library/sources/{}/excerpts.json", id);
            out.push(Span { row: Row::SourceRead, label: label_of(false, &rel, i + 1), kind: "excerpt".into(), rel_path: rel, start_line: i + 1, end_line: i + 1, title: mask(&title), hash: text_hash(&text), text, status: status.clone(), topic: None });
            gave = true;
        }
        if !gave {
            named.push(id.clone());
        }
    }
    out
}

/// The sessions whose research runs the followed topics hold: from `research/runs/<session>.<run>.json`.
fn sessions_of(ws: &Path, fs: &[Followed]) -> BTreeSet<String> {
    let ids: BTreeSet<&str> = fs.iter().map(|f| f.topic.id.as_str()).collect();
    let mut out = BTreeSet::new();
    for e in std::fs::read_dir(ws.join("research/runs")).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(session) = name.strip_suffix(".json").and_then(|n| n.split_once('.')).map(|(s, _)| s.to_string()) else { continue };
        let Some(p) = std::fs::read_to_string(e.path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
        if p["topic_id"].as_str().is_some_and(|t| ids.contains(t)) && !session.is_empty() && session.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            out.insert(session);
        }
    }
    out
}

fn row_findings(ws: &Path, sessions: &BTreeSet<String>, mask: &dyn Fn(&str) -> String) -> Vec<Span> {
    let private: BTreeSet<String> = crate::scope::project_entries(ws).into_iter().filter(|p| p.private).map(|p| p.id.as_str().to_string()).collect();
    let mut missions: BTreeSet<String> = BTreeSet::new();
    for s in sessions {
        missions.insert(format!("host-{}", s));
        missions.insert(s.clone());
    }
    let mut out = Vec::new();
    for m in missions {
        let dir = delphi_core::sessions::dir(ws, &m);
        if crate::scope::mission_scope(ws, &dir).is_some_and(|p| private.contains(p.as_str())) {
            continue;
        }
        let mut files: Vec<String> = std::fs::read_dir(dir.join("findings")).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".md")).collect();
        files.sort();
        for name in files {
            let Ok(file) = std::fs::read_to_string(dir.join("findings").join(&name)) else { continue };
            let (front, _) = crate::workspace::split_frontmatter(&file);
            if front.as_ref().and_then(|f| f.get("status")).and_then(|v| v.as_str()).is_some_and(|s| s != "active") {
                continue;
            }
            let (body, line) = below_frontmatter(&file);
            let (body, line, heading) = below_title(body, line);
            let title = front.as_ref().and_then(|f| f.get("title")).and_then(|v| v.as_str()).map(String::from).or(heading).unwrap_or_else(|| name.clone());
            let rel = format!("missions/{}/findings/{}", m, name);
            out.extend(body_spans(Row::Finding, true, "finding", &rel, body, line, &title, "○ not validated", None, mask));
        }
    }
    out
}

fn row_summary_pages(ws: &Path, ids: &BTreeSet<String>, urls: &BTreeSet<String>, mask: &dyn Fn(&str) -> String) -> Vec<Span> {
    let dir = ws.join(crate::kb_import::DIR);
    let mut pages: Vec<String> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".md") && !n.ends_with(".bib.md")).collect();
    pages.sort();
    let mut out = Vec::new();
    for name in pages {
        let id = name.trim_end_matches(".md");
        let rel = format!("summaries/{}", name);
        if !crate::kb_import::is_kept(ws, &rel) {
            continue;
        }
        let bib = std::fs::read_to_string(dir.join(format!("{}.bib.md", id))).unwrap_or_default();
        let names = bib.lines().any(|l| {
            let l = l.trim();
            l.strip_prefix("- locator:").is_some_and(|u| urls.contains(u.trim())) || l.strip_prefix("source_id:").is_some_and(|s| ids.contains(s.trim()))
        });
        if !names {
            continue;
        }
        let Ok(file) = std::fs::read_to_string(dir.join(&name)) else { continue };
        let (body, line) = below_frontmatter(&file);
        let (body, line, heading) = below_title(body, line);
        out.extend(body_spans(Row::SharedSource, false, "summary page", &rel, body, line, &heading.unwrap_or_else(|| id.to_string()), "○ not validated", None, mask));
    }
    out
}

fn row_ingest(lib: &Library, fs: &[Followed], mask: &dyn Fn(&str) -> String) -> Vec<Span> {
    let mut out = Vec::new();
    for f in fs {
        let (Some(path), Some(sid)) = (f.rec["ingest"]["path"].as_str(), f.rec["ingest"]["source_id"].as_str()) else { continue };
        // The ingested file is a library source; a private one is never a span (§1.1, §2.1 item 3).
        if lib.read_meta(sid).is_none_or(|m| is_private(&m)) {
            continue;
        }
        let Ok(file) = std::fs::read_to_string(path) else { continue };
        let (body, line) = below_frontmatter(&file);
        let rel = format!("{}/ingest/{}", f.rel, sid);
        out.extend(body_spans(Row::IngestFile, false, "research file", &rel, body, line, f.rec["question"].as_str().unwrap_or(""), "○ not validated", Some(&f.topic.id), mask));
    }
    out
}

fn row_search(lib: &Library, home: &Path, ws: &Path, side: &Value, fs: &[Followed], mask: &dyn Fn(&str) -> String) -> Vec<Span> {
    let own: BTreeSet<String> = fs.iter().flat_map(|f| [format!("{}/answer.md", f.rel), format!("{}/record.json", f.rel)]).collect();
    let g = crate::topic_gather::gather(lib, home, ws, side);
    let mut out = Vec::new();
    for r in g.records {
        // A library source's text or its excerpts are never a span, whatever the index holds of them.
        if r.rel_path.starts_with("library/") || own.contains(&r.rel_path) {
            continue;
        }
        let text = mask(&r.text);
        if text.trim().is_empty() {
            continue;
        }
        let end = r.start_line + text.lines().count().saturating_sub(1);
        out.push(Span { row: Row::Search, label: r.label, kind: r.kind, rel_path: r.rel_path, start_line: r.start_line, end_line: end, title: mask(&r.title), hash: r.hash, text, status: r.status, topic: None });
    }
    out
}

fn row_claims(lib: &Library, side: &Value, ws: &Path, mask: &dyn Fn(&str) -> String) -> Vec<Span> {
    let mut out: Vec<Span> = lib
        .topic_research_claims(side, ws)
        .iter()
        .filter_map(|c| {
            let id = c["id"].as_str()?.to_string();
            let text = mask(c["text"].as_str().unwrap_or(""));
            (!text.trim().is_empty()).then(|| Span { row: Row::Claim, label: id.clone(), kind: "claim".into(), rel_path: format!("library/claims/{}.json", id), start_line: 1, end_line: 1, title: String::new(), hash: crate::library::page_search::claim_print(c), text, status: crate::library::pages::shown_status(crate::library::claims::status(c)), topic: None })
        })
        .collect();
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    out
}

/// A topic's seed (§2.1): reads the files, never writes; with `a.search`, also asks today's search (read-only) for row 6.
pub fn assemble(lib: &Library, home: &Path, ws: &Path, side: &Value, a: &Assemble<'_>) -> Seed {
    let ids = a.identifiers.to_vec();
    let mask = move |t: &str| crate::host_research::mask(t, &ids);
    let fs = followed(side, ws);
    let (read_ids, read_urls) = read_by(&fs);
    let mut seed = Seed { sources_read: read_ids.iter().cloned().collect(), ..Default::default() };
    let mut rows: Vec<Vec<Span>> = Vec::new();
    rows.push(fs.iter().flat_map(|f| row_answers(f, &mask)).collect());
    rows.push(row_sources(lib, &read_ids, &mask, &mut seed.named_sources));
    rows.push(row_findings(ws, &sessions_of(ws, &fs), &mask));
    rows.push(row_summary_pages(ws, &read_ids, &read_urls, &mask));
    rows.push(row_ingest(lib, &fs, &mask));
    rows.push(if a.search { row_search(lib, home, ws, side, &fs, &mask) } else { Vec::new() });
    rows.push(row_claims(lib, side, ws, &mask));
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (i, mut row) in rows.into_iter().enumerate() {
        if i > 0 {
            row.sort_by(|x, y| (x.rel_path.as_str(), x.start_line).cmp(&(y.rel_path.as_str(), y.start_line)));
        }
        row.retain(|s| seen.insert(s.label.clone()));
        seed.offered[i] = row.len();
        for s in row {
            let fits = seed.spans.len() < MAX_SPANS && seed.bytes() + s.text.len() <= MAX_BYTES;
            // Row 1 is never dropped to make room for the others.
            if s.row == Row::Answer || fits {
                seed.spans.push(s);
            }
        }
    }
    seed
}

/// The seed `side` would get from its linked rows alone (no search): what a due check compares and what a list asks to say a topic
/// is waiting for material. Reads only.
pub fn assemble_linked(lib: &Library, ws: &Path, side: &Value) -> Seed {
    let ids = crate::query_guard::identifiers(ws);
    assemble(lib, ws, ws, side, &Assemble { identifiers: &ids, search: false })
}

/// One topic's line of `delphi pages seeds --count` (§4 slice A, AC-17): the spans each row would give, before and after the caps.
pub fn count_json(slug: &str, seed: &Seed) -> Value {
    let kept = seed.counts();
    let rows: Vec<Value> = Row::ALL.iter().map(|r| json!({"row": r.number(), "name": r.name(), "link": r.link(), "offered": seed.offered[r.number() - 1], "kept": kept[r.number() - 1]})).collect();
    json!({"slug": slug, "rows": rows, "spans": seed.spans.len(), "bytes": seed.bytes(), "named_sources": seed.named_sources.len(), "answer_chars": seed.spans.iter().filter(|s| s.row == Row::Answer).map(|s| s.text.chars().count()).sum::<usize>()})
}

/// What a seed page's banner says (§2.2 item 2). It stands where a written page's banner stands.
pub const SEED_BANNER: &str = "Seed draft: assembled by code from research notes. Not written up and not validated.";
/// The mark the held-text predicate puts where it cut a block's quotation of a held source back to the bound (§2.2 item 3).
pub const SHORTENED: &str = "[shortened: quoted source text]";

/// The held-text predicate of `specs/142` §2.5 item 4, as the seed check asks it, and nothing more. `specs/142` slice B owns it: it
/// supplies the one shared `body_may_leave` and the block index behind [`HeldText::sharing`], and passes that to the seed check in place
/// of [`NoHeldText`]. Nothing here decides what of a held source may leave.
pub trait HeldText: Sync {
    /// The held sources that share a 64-character block with `output` (the block index): a note may quote a source no record names.
    fn sharing(&self, output: &str) -> Vec<String>;
    /// Whether `source` is a held source (`text_held`).
    fn is_held(&self, source: &str) -> bool;
    /// `body_may_leave(source, output)`: no run of more than 300 characters of the source's text, and no more of it in all than the
    /// aggregate cap.
    fn body_may_leave(&self, source: &str, output: &str) -> bool;
    /// `output` with each run of `source`'s text that is past the bound cut back to it, and [`SHORTENED`] written where it was cut.
    fn shorten(&self, source: &str, output: &str) -> String;
}

/// The predicate as it is until `specs/142` slice B: no source is held (every source is a pointer), so nothing is shared and
/// everything may leave. A seed then carries of a source only its summary and excerpts of at most 300 characters (§2.1 item 3).
pub struct NoHeldText;

impl HeldText for NoHeldText {
    fn sharing(&self, _output: &str) -> Vec<String> {
        Vec::new()
    }
    fn is_held(&self, _source: &str) -> bool {
        false
    }
    fn body_may_leave(&self, _source: &str, _output: &str) -> bool {
        true
    }
    fn shorten(&self, _source: &str, output: &str) -> String {
        output.to_string()
    }
}

fn res() -> &'static [regex::Regex; 7] {
    static R: std::sync::OnceLock<[regex::Regex; 7]> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let r = |p: &str| regex::Regex::new(p).expect("a fixed pattern");
        [
            r(r"(?s)<!--.*?-->"),
            r(r"!\[([^\]\n]*)\]\([^)\n]*\)"),
            r(r"\[([^\]\n]+)\]\(([^)\n]*)\)"),
            r(r"</?[A-Za-z][^>\n]*>"),
            r(r"https?://[^\s)\]>]+"),
            r(r"\[((?:s|c|f)-[^\]\n]*)\]"),
            r(r"\n{3,}"),
        ]
    })
}

/// The one cleaning of a span's text (§2.2 item 3), used both to render a block and to compare it with its span: a link other than a
/// `#section` or `page:` link keeps its words and loses its address; an image becomes its alternative text; raw HTML (comments and
/// tags) and a bare address are removed; control characters are removed; a bracketed record id loses its brackets, so a span can cite
/// nothing; a top-level heading is made a third-level one.
pub fn clean(text: &str) -> String {
    let [comment, image, link, tag, url, marker, blank] = res();
    let t: String = text.chars().filter_map(|c| if c == '\n' { Some(c) } else if c == '\t' { Some(' ') } else if c.is_control() { None } else { Some(c) }).collect();
    let t = comment.replace_all(&t, "");
    let t = image.replace_all(&t, "$1");
    let t = link.replace_all(&t, |c: &regex::Captures<'_>| {
        let to = c[2].trim();
        if to.starts_with('#') || to.starts_with("page:") { c[0].to_string() } else { c[1].to_string() }
    });
    let t = tag.replace_all(&t, "");
    let t = url.replace_all(&t, "");
    let t = marker.replace_all(&t, "($1)");
    let lines: Vec<String> = t.lines().map(|l| if let Some(h) = l.strip_prefix("# ").or_else(|| l.strip_prefix("## ")) { format!("### {}", h) } else { l.to_string() }).map(|l| l.trim_end().to_string()).collect();
    blank.replace_all(&lines.join("\n"), "\n\n").trim().to_string()
}

/// A heading or title as one line: cleaned, no `#`, at most 200 characters.
fn one_line(s: &str) -> String {
    let t = clean(s).replace('\n', " ");
    norm_ws(t.trim_start_matches('#')).chars().take(200).collect()
}

/// One block of a seed page.
#[derive(Debug, Clone)]
struct Block {
    span: usize,
    /// The section heading the block goes under.
    group: String,
    text: String,
}

/// A seed page's body and what its check dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct SeedPage {
    /// `# <title>`, the blocks under their headings, each ending with its marker.
    pub body: String,
    /// The labels the body cites, in order.
    pub cited: Vec<String>,
    /// `(label, reason)` for each block the check dropped (the sidecar's `seed_dropped`).
    pub dropped: Vec<(String, String)>,
}

fn render(title: &str, seed: &Seed, blocks: &[Block]) -> String {
    let mut s = format!("# {}\n\n", one_line(title));
    let mut group: Option<&str> = None;
    for b in blocks {
        // The lead (the newest answer's first span) has no heading; every other change of group opens one.
        if group != Some(b.group.as_str()) {
            if !b.group.is_empty() {
                s.push_str(&format!("## {}\n\n", b.group));
            }
            group = Some(b.group.as_str());
        }
        s.push_str(&format!("{} [{}]\n\n", b.text.trim_end(), seed.spans[b.span].label));
    }
    s.trim_end().to_string() + "\n"
}

/// Is the block's text in its cleaned span, letter for letter? A shortened block is compared piece by piece around its marks.
fn verbatim(block: &str, span: &str) -> bool {
    let clean_span = clean(span);
    block.split(SHORTENED).map(str::trim).filter(|p| !p.is_empty()).all(|p| clean_span.contains(p))
}

/// Does a span come from a document a seed leaves out (§2.1 item 4, item 3)? A generated page, or a library source's own text.
fn left_out(s: &Span) -> bool {
    let src = s.rel_path.strip_prefix("library/sources/").is_some_and(|r| !(r.ends_with("/summary.md") || r.ends_with("/excerpts.json")));
    s.rel_path.starts_with("library/pages/") || src
}

/// The seed page of §2.2: rendered by code from `seed` and checked (§2.2 item 3). `Err` holds what was dropped when no block is left: no
/// page is written then.
pub fn seed_page(title: &str, seed: &Seed, held: &dyn HeldText) -> Result<SeedPage, Vec<(String, String)>> {
    let mut dropped: Vec<(String, String)> = Vec::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut lead: Option<String> = None;
    for (i, s) in seed.spans.iter().enumerate() {
        let group = match s.row {
            Row::Answer => {
                // The lead and the rest of its section have no heading; any other section of an answer opens its own.
                let h = one_line(&s.title);
                let lead_title = lead.get_or_insert_with(|| h.clone());
                if *lead_title == h { String::new() } else { h }
            }
            Row::SourceRead => "Sources read".into(),
            Row::Finding => "Findings".into(),
            Row::SharedSource | Row::IngestFile | Row::Search => "Related notes".into(),
            Row::Claim => "Claims".into(),
        };
        if left_out(s) {
            dropped.push((s.label.clone(), "a document a seed leaves out".into()));
            continue;
        }
        let text = clean(&s.text);
        if text.is_empty() {
            dropped.push((s.label.clone(), "nothing left once cleaned".into()));
            continue;
        }
        blocks.push(Block { span: i, group, text });
    }
    // The held-text predicate, for every held source the page shares a block with and every held source the topic read.
    let mut sources: BTreeSet<String> = held.sharing(&render(title, seed, &blocks)).into_iter().collect();
    sources.extend(seed.sources_read.iter().filter(|s| held.is_held(s)).cloned());
    for src in &sources {
        let mut kept: Vec<Block> = Vec::new();
        for mut b in std::mem::take(&mut blocks) {
            if !held.body_may_leave(src, &b.text) {
                b.text = held.shorten(src, &b.text);
                if !held.body_may_leave(src, &b.text) {
                    dropped.push((seed.spans[b.span].label.clone(), format!("quotes the held source `{}` past the bound", src)));
                    continue;
                }
            }
            kept.push(b);
        }
        blocks = kept;
        // The aggregate cap over the whole page: the last blocks that quote the source go first.
        while !held.body_may_leave(src, &render(title, seed, &blocks)) {
            let Some(i) = blocks.iter().rposition(|b| held.sharing(&b.text).iter().any(|s| s == src) || !held.body_may_leave(src, &b.text)) else {
                dropped.extend(blocks.drain(..).map(|b| (seed.spans[b.span].label.clone(), format!("the page passes the aggregate cap of the held source `{}`", src))));
                break;
            };
            let b = blocks.remove(i);
            dropped.push((seed.spans[b.span].label.clone(), format!("the page passes the aggregate cap of the held source `{}`", src)));
        }
    }
    // Every block is its span's own words, letter for letter, once both are cleaned.
    blocks.retain(|b| {
        let ok = verbatim(&b.text, &seed.spans[b.span].text);
        if !ok {
            dropped.push((seed.spans[b.span].label.clone(), "not verbatim in its span".into()));
        }
        ok
    });
    if blocks.is_empty() {
        return Err(dropped);
    }
    let body = render(title, seed, &blocks);
    let mut cited: Vec<String> = Vec::new();
    for b in &blocks {
        let l = &seed.spans[b.span].label;
        if !cited.contains(l) {
            cited.push(l.clone());
        }
    }
    Ok(SeedPage { body, cited, dropped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::topics::Origin;
    use crate::library::{AddOpts, Budgets};
    use crate::research_record::{Asker, Begin, Topic};

    fn excerpts(lib: &Library, id: &str, texts: &[String]) {
        let all: Vec<Value> = texts.iter().enumerate().map(|(i, t)| json!({"id": format!("e{}", i + 1), "text": t})).collect();
        std::fs::write(lib.source_dir(id).join("excerpts.json"), serde_json::to_vec(&all).unwrap()).unwrap();
    }

    fn none() -> Assemble<'static> {
        Assemble { identifiers: &[], search: true }
    }

    /// A research run that finished with `answer` and `pages` (url, source id) read.
    fn research(ws: &Path, question: &str, run: &str, answer: &str, pages: &[(&str, &str)]) -> Topic {
        let t = Topic::begin(ws, &Begin { question, asked_by: Asker::operator(), project: None, depth: delphi_core::research::Depth::Quick, run_id: run }).unwrap();
        for (u, s) in pages {
            t.page(run, u, s, None).unwrap();
        }
        t.finish(run, "done", Some(&json!({"answer": answer, "claims": []})), &[], None, 0).unwrap();
        t
    }

    fn answer_of(n: usize) -> String {
        // About 6,000 characters in three sections, each paragraph distinct.
        let para = |i: usize| format!("Paragraph {i} says the estuary tide turns at the harbour wall and the lag grows by {i} minutes each spring cycle.");
        let mut s = String::new();
        for (k, h) in ["", "## History", "## Measurements"].iter().enumerate() {
            if !h.is_empty() {
                s.push_str(&format!("{h}\n\n"));
            }
            for i in 0..n {
                s.push_str(&format!("{}\n\n", para(k * 100 + i)));
            }
        }
        s
    }

    /// AC-1: the whole of a 6,000-character answer is in the seed as spans, in file order, before any span search found.
    #[test]
    fn a_topics_answer_is_the_seeds_first_spans_whole_and_in_file_order() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let answer = answer_of(17);
        assert!((5_800..=6_400).contains(&answer.len()), "{}", answer.len());
        research(&ws, "Why do estuary tides lag?", "r-1", &answer, &[]);
        {
            let mut kb = crate::kb::Kb::open(&crate::topic_gather::db_of(&ws)).unwrap();
            kb.upsert_from("context/estuary.md", "context", "external_untrusted", "# Estuary tides\n\nWhy do estuary tides lag behind the open sea?\n", "t").unwrap();
        }
        let lib = Library::in_home(home.path());
        let slug = lib.topics_from_research(&ws, 1).remove(0);
        let side = lib.topic_side(&slug).unwrap();
        let seed = assemble(&lib, home.path(), &ws, &side, &none());
        let answers: Vec<&Span> = seed.spans.iter().filter(|s| s.row == Row::Answer).collect();
        assert!(seed.spans.iter().take(answers.len()).all(|s| s.row == Row::Answer), "the answer comes first");
        assert!(seed.spans.iter().any(|s| s.row == Row::Search), "search fills what is left: {:?}", seed.counts());
        let joined = answers.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n");
        for line in answer.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("## ")) {
            assert!(joined.contains(line), "missing: {line}");
        }
        assert!(answers.windows(2).all(|w| w[0].start_line < w[1].start_line), "file order");
        assert_eq!(answers.iter().filter_map(|s| (s.title != "Why do estuary tides lag?").then_some(s.title.as_str())).collect::<Vec<_>>(), vec!["History", "Measurements"]);
        // The line numbers name the file's own lines.
        let file = std::fs::read_to_string(ws.join(format!("research/topics/{}/answer.md", side["research_topics"][0].as_str().unwrap()))).unwrap();
        let first = answers[0];
        assert_eq!(file.lines().nth(first.start_line - 1).unwrap(), first.text.lines().next().unwrap());
    }

    /// AC-2: the same files give the same seed byte for byte, whatever order the directories list them in; a private record, an
    /// `internal` hit and a generated page are never spans; and no span is a library source's own text.
    #[test]
    fn the_seed_is_deterministic_and_leaves_out_what_the_gather_leaves_out() {
        let base = tempfile::tempdir().unwrap();
        let build = |order: &[usize]| {
            // The same home path each time, so a source's id (which names its file) is the same.
            let home = base.path().join("h");
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).unwrap();
            let home = &home;
            let ws = home.join("workspaces/w");
            std::fs::create_dir_all(ws.join("local")).unwrap();
            let lib = Library::in_home(home);
            let doc = home.join("s.md");
            std::fs::write(&doc, format!("# Tides\n\n{}\n", "The whole text of the source says the tide turns twice a day. ".repeat(20))).unwrap();
            let held = lib.add_file(&doc, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
            let pointer = home.join("p.md");
            std::fs::write(&pointer, "# Pointer\n\nA pointer's words.\n").unwrap();
            let other = lib.add_file(&pointer, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
            excerpts(&lib, &held, &["An excerpt that is long. ".repeat(30)]);
            // Two findings in the session that ran the research, written in the order given.
            let t = research(&ws, "Why do tides lag?", "r-1", "Tides lag because the estuary is shallow.\n\n## More\n\nThe lag is an hour.", &[("https://e.org/a", &held), ("https://e.org/b", &other)]);
            crate::research_record::point_run(&ws, "abcdef0123456789", "r-1", &t, None).unwrap();
            let fdir = delphi_core::sessions::dir(&ws, "host-abcdef0123456789").join("findings");
            std::fs::create_dir_all(&fdir).unwrap();
            for i in order {
                std::fs::write(fdir.join(format!("f{i}.md")), format!("---\nstatus: active\ntitle: Finding {i}\n---\n# Finding {i}\n\nConclusion (likely): finding {i} holds.\n")).unwrap();
            }
            std::fs::write(fdir.join("old.md"), "---\nstatus: superseded\n---\n# Old\n\nGone.\n").unwrap();
            // A private research topic on the same question words, and a generated page in the index.
            let p = research(&ws, "Why are tides private?", "r-2", "Private words about tides.", &[]);
            p.amend(|r| r["private"] = json!(true)).unwrap();
            {
                let mut kb = crate::kb::Kb::open(&crate::topic_gather::db_of(&ws)).unwrap();
                kb.upsert_from("library/pages/tides.md", "library", "external_untrusted", "# Tides\n\nA generated page about why tides lag.\n", "t").unwrap();
                kb.upsert_from(&format!("library/sources/{}/source.md", held), "library", "external_untrusted", "# Tides\n\nThe whole text of the source says why tides lag.\n", "t").unwrap();
            }
            let slug = lib.topics_from_research(&ws, 1).into_iter().find(|s| s.contains("lag")).unwrap();
            let mut side = lib.topic_side(&slug).unwrap();
            side["research_topics"].as_array_mut().unwrap().push(json!(p.id));
            let seed = assemble(&lib, home, &ws, &side, &none());
            let bytes = serde_json::to_string(&seed.spans.iter().map(|s| json!([s.to_json(), s.text, s.title, s.status])).collect::<Vec<_>>()).unwrap();
            (seed, bytes, held, other)
        };
        let (seed, a, held, other) = build(&[1, 2, 3]);
        let (_, b, _, _) = build(&[3, 1, 2]);
        assert_eq!(a, b, "the same files give the same seed");
        assert_eq!(seed.counts()[Row::Finding.number() - 1], 3, "{:?}", seed.counts());
        assert!(seed.spans.iter().all(|s| !s.text.contains("Private words") && !s.rel_path.starts_with("library/pages/") && !s.text.contains("Gone.")));
        assert!(seed.spans.iter().all(|s| !s.text.contains("The whole text of the source")), "no span is a source's own text");
        let ex: Vec<&Span> = seed.spans.iter().filter(|s| s.kind == "excerpt").collect();
        assert_eq!(ex.len(), 1);
        assert!(ex[0].text.chars().count() <= SOURCE_PIECE_MAX && ex[0].rel_path.contains(&held));
        assert_eq!(seed.named_sources, vec![other], "a source with no summary and no excerpt is only named");
    }

    /// `topic_seed::tests::no_span_is_a_source_body` (AC-2's fitness function): a source held whole, with a long summary, gives its
    /// summary cut to 300 characters and nothing of its text.
    #[test]
    fn no_span_is_a_source_body() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let lib = Library::in_home(home.path());
        let doc = home.path().join("s.md");
        let body = "Source body sentence that must never be a span. ".repeat(40);
        std::fs::write(&doc, format!("# S\n\n{body}\n")).unwrap();
        let id = lib.add_file(&doc, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
        std::fs::write(lib.source_dir(&id).join("summary.md"), format!("---\nmodel: m\n---\n{}", "A summary in the model's words. ".repeat(30))).unwrap();
        research(&ws, "What does the source say?", "r-1", "It says things.", &[("https://e.org/s", &id)]);
        let slug = lib.topics_from_research(&ws, 1).remove(0);
        let seed = assemble(&lib, home.path(), &ws, &lib.topic_side(&slug).unwrap(), &none());
        let sum: Vec<&Span> = seed.spans.iter().filter(|s| s.kind == "source summary").collect();
        assert_eq!(sum.len(), 1);
        assert!(sum[0].text.chars().count() <= SOURCE_PIECE_MAX && sum[0].text.starts_with("A summary"));
        assert!(seed.spans.iter().all(|s| !s.text.contains("must never be a span")));
    }

    /// `topic_seed::tests::a_private_record_is_never_a_span`; and AC-12: a record with `claims: []` gives a seed from its answer.
    #[test]
    fn a_private_record_is_never_a_span() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let lib = Library::in_home(home.path());
        // A private library source the run read: its excerpts are never a span, and it is not named either.
        let ps = lib.add_private_pointer("file", "/notes/private.md", "Private notes", "2026-11-25T00:00:00Z", None).unwrap();
        excerpts(&lib, &ps, &["A private excerpt.".to_string()]);
        let t = research(&ws, "What is private here?", "r-1", "Secret answer text.", &[("file:///notes/private.md", &ps)]);
        let slug = lib.topic_create("What is private here?", Origin::Research(t.id.clone()), 1).unwrap().slug;
        let side = lib.topic_side(&slug).unwrap();
        let open = assemble(&lib, home.path(), &ws, &side, &none());
        assert_eq!(open.counts()[0], 1, "public, claims: [], an answer: a seed");
        assert!(open.spans.iter().all(|s| !s.text.contains("private excerpt")) && open.named_sources.is_empty(), "{:?}", open.spans);
        assert_eq!(open.counts()[Row::Claim.number() - 1], 0);
        t.amend(|r| r["private"] = json!(true)).unwrap();
        assert!(assemble(&lib, home.path(), &ws, &side, &none()).is_empty());
    }

    /// The caps: at most 40 spans and 64 KiB, a span at most 8 KiB cut at a paragraph, row 1 never dropped; masked identifiers.
    #[test]
    fn the_caps_hold_and_row_one_is_never_dropped() {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let lib = Library::in_home(home.path());
        let doc = home.path().join("s.md");
        std::fs::write(&doc, "# S\n\nText.\n").unwrap();
        let id = lib.add_file(&doc, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
        excerpts(&lib, &id, &(0..50).map(|i| format!("Excerpt number {i} of the source.")).collect::<Vec<_>>());
        let long: String = (0..60).map(|i| format!("Paragraph {i} of a long answer about acme widgets and their tides, written to fill the span cap.\n\n")).collect::<String>().repeat(2);
        research(&ws, "How long is long?", "r-1", &long, &[("https://e.org/s", &id)]);
        let slug = lib.topics_from_research(&ws, 1).remove(0);
        let ids = vec!["acme".to_string()];
        let seed = assemble(&lib, home.path(), &ws, &lib.topic_side(&slug).unwrap(), &Assemble { identifiers: &ids, search: false });
        assert!(seed.spans.iter().all(|s| s.text.len() <= SPAN_MAX && !s.text.to_lowercase().contains("acme")));
        assert!(seed.counts()[0] >= 2, "a long answer is several spans");
        assert!(seed.spans.len() <= MAX_SPANS && seed.bytes() <= MAX_BYTES);
        assert_eq!(seed.offered[1], 50);
        assert!(seed.counts()[1] < 50, "row 2 is cut to the cap");
    }

    /// A held source for the tests: its text, a run bound of 300 characters, an aggregate cap, and a block index of 64 characters.
    struct Held(Vec<(String, String)>);

    impl Held {
        fn text(&self, id: &str) -> Vec<char> {
            self.0.iter().find(|(i, _)| i == id).map(|(_, t)| t.chars().collect()).unwrap_or_default()
        }
        /// Which characters of `out` lie in a common run of at least `n` characters with the source.
        fn covered(&self, id: &str, out: &str, n: usize) -> Vec<bool> {
            let src = self.text(id);
            let o: Vec<char> = out.chars().collect();
            let wins: std::collections::HashSet<&[char]> = if src.len() >= n { src.windows(n).collect() } else { Default::default() };
            let mut cov = vec![false; o.len()];
            if o.len() >= n {
                for i in 0..=o.len() - n {
                    if wins.contains(&o[i..i + n]) {
                        cov[i..i + n].iter_mut().for_each(|c| *c = true);
                    }
                }
            }
            cov
        }
    }

    impl HeldText for Held {
        fn sharing(&self, output: &str) -> Vec<String> {
            self.0.iter().filter(|(id, _)| self.covered(id, output, 64).iter().any(|c| *c)).map(|(id, _)| id.clone()).collect()
        }
        fn is_held(&self, source: &str) -> bool {
            self.0.iter().any(|(i, _)| i == source)
        }
        fn body_may_leave(&self, source: &str, output: &str) -> bool {
            let run = !self.covered(source, output, 301).iter().any(|c| *c);
            let cap = 1200.min(self.text(source).len() / 10);
            run && self.covered(source, output, 40).iter().filter(|c| **c).count() <= cap
        }
        fn shorten(&self, source: &str, output: &str) -> String {
            let o: Vec<char> = output.chars().collect();
            let cov = self.covered(source, output, 301);
            let mut out = String::new();
            let (mut i, mut run) = (0, 0);
            while i < o.len() {
                if cov[i] {
                    run += 1;
                    // Cut one short of the bound: the mark's leading space could carry the run one further.
                    if run == 300 {
                        out.push_str(&format!(" {} ", SHORTENED));
                        while i < o.len() && cov[i] {
                            i += 1;
                        }
                        run = 0;
                        continue;
                    }
                } else {
                    run = 0;
                }
                if run < 300 {
                    out.push(o[i]);
                }
                i += 1;
            }
            out
        }
    }

    fn span(row: Row, label: &str, rel: &str, text: &str) -> Span {
        Span { row, label: label.into(), kind: "research answer".into(), rel_path: rel.into(), start_line: 1, end_line: 1, title: "T".into(), text: text.into(), hash: text_hash(text), status: "○ not validated".into(), topic: None }
    }

    /// `topic_seed::tests::a_seed_page_passes_body_may_leave_for_every_source` (AC-18): an answer and a finding that quote a held source
    /// at length, the finding quoting one no record of the topic names, are shortened and marked where they quote; the page then passes
    /// the predicate for every held source; a page over the aggregate cap loses the blocks that quote; a span with a link, an image and
    /// raw HTML renders with none of them and passes the comparison; a block that fails is dropped and counted; a seed with no block
    /// left gives no page.
    #[test]
    fn a_seed_page_passes_body_may_leave_for_every_source() {
        let src_a: String = (0..400).map(|i| format!("Held sentence {i:03} of source A runs on. ")).collect();
        let src_b: String = (0..400).map(|i| format!("Held sentence {i:03} of source B runs on. ")).collect();
        let held = Held(vec![("a".into(), src_a.clone()), ("b".into(), src_b.clone())]);
        let quote_a: String = src_a.chars().take(400).collect();
        let quote_b: String = src_b.chars().skip(100).take(400).collect();
        let messy = "Read [the spec](https://example.org/spec) and [above](#history), see ![a tide chart](lib:x/chart.png) <b>now</b><!-- hidden --> [s-abc].";
        let seed = Seed {
            spans: vec![
                span(Row::Answer, "s-kb-0000000000000001", "research/topics/t/answer.md", &format!("The answer opens here. {quote_a}")),
                span(Row::Finding, "f-0000000000000002", "missions/m/findings/x.md", &format!("A finding copies: {quote_b}")),
                span(Row::Search, "s-kb-0000000000000003", "context/n.md", messy),
                span(Row::Search, "s-kb-0000000000000004", "library/pages/other.md", "A generated page."),
            ],
            sources_read: vec!["a".into()],
            ..Default::default()
        };
        let page = seed_page("Tides", &seed, &held).unwrap();
        for id in ["a", "b"] {
            assert!(held.body_may_leave(id, &page.body), "{id}: {}", page.body);
        }
        assert_eq!(page.body.matches(SHORTENED).count(), 2, "{} {:?}", page.body, page.dropped);
        assert!(page.body.contains("The answer opens here.") && page.body.contains("[s-kb-0000000000000001]"));
        assert!(page.body.contains("the spec") && page.body.contains("[above](#history)") && page.body.contains("a tide chart") && page.body.contains("now"));
        for gone in ["https://", "chart.png", "<b>", "hidden", "[s-abc]"] {
            assert!(!page.body.contains(gone), "{gone}: {}", page.body);
        }
        assert!(page.cited.contains(&"s-kb-0000000000000003".to_string()), "the cleaned span passes the comparison");
        assert_eq!(page.dropped, vec![("s-kb-0000000000000004".to_string(), "a document a seed leaves out".to_string())]);
        assert!(page.body.contains("## Findings") && page.body.contains("## Related notes") && !page.body.contains("## T\n"));

        // Over the aggregate cap: many short quotes of A, each under the run bound, together past a tenth of it.
        let pieces: Vec<Span> = (0..6).map(|k| span(Row::Search, &format!("s-kb-00000000000001{k:02}"), &format!("context/q{k}.md"), &src_a.chars().skip(k * 250).take(250).collect::<String>())).collect();
        let over = Seed { spans: pieces, ..Default::default() };
        let p = seed_page("Tides", &over, &held).unwrap();
        assert!(held.body_may_leave("a", &p.body) && !p.dropped.is_empty(), "{:?}", p.dropped);

        // A predicate that cannot cut: every block that quotes is dropped and counted; with none left there is no page.
        struct Refuses;
        impl HeldText for Refuses {
            fn sharing(&self, _o: &str) -> Vec<String> {
                vec!["a".into()]
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
        let dropped = seed_page("Tides", &seed, &Refuses).unwrap_err();
        assert_eq!(dropped.len(), 4, "{dropped:?}");
        // Today's predicate holds nothing, and changes nothing.
        let today = seed_page("Tides", &seed, &NoHeldText).unwrap();
        assert!(today.body.contains(&quote_a) && !today.body.contains(SHORTENED));
    }
}
