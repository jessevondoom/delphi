//! The search log of a research run (`specs/101` §3): what was searched, of whom, when, and how much came back, written by the
//! daemon beside the result so a reader can see the record of a search the way a systematic review shows one.
//!
//! * **One line per search**: `rung`, `source` (a closed set), `purpose`, `query`, `at`, `hits`, `kept`. `query` is the SAME
//!   redacted string that was sent (`research_plan::clean_query`, `specs/066` §6). A search withheld because its claim rests on a
//!   private source carries `withheld: "private"` and **no query text at all**.
//! * **A flow summary** after PRISMA's flow diagram. Every line balances on its own: `hits = duplicates + kept + Σ not_kept`, so the
//!   flow is a plain sum over the lines and no model is in it.
//! * **Daemon-written.** The host cannot add, edit or delete a line. A search a host ran with its own tools can be *declared*
//!   ([`SearchLog::declare`]); it is logged `declared: true`, `kept` is always 0, and it is excluded from every count.
//! * **Private by inheritance.** The log carries the strictest `private` flag of the claims it served. [`may_leave`] is the library's
//!   own check (`specs/055` §10.1), so a private log is refused on every outbound channel, like a private source.
//! * Nothing here is read as a reason to trust, promote or permit anything (`specs/101` §12 item 6).

use crate::library::{self, Channel};
use crate::research_plan::clean_query;
use delphi_core::redact::Redactor;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Lines kept per run. A run is bounded by its step budget (at most a few dozen searches), so this is a guard and not a limit anyone meets.
pub const MAX_LINES: usize = 200;
/// Searches a host may declare per request.
pub const MAX_DECLARED: usize = 20;
const QUERY_CHARS: usize = 200;
const ID_CHARS: usize = 40;

/// The closed list of reasons a result found by a search was not kept. Nothing else can appear in `not_kept`.
pub const NOT_KEPT: [&str; 7] = ["page_cap", "budget", "refused", "fetch_failed", "capture_failed", "lead_not_stored", "not_followed"];

/// Of whom a search was made. The ids after the colon are lower-case words from `[a-z0-9._-]`, at most 40 characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Kb,
    Trusted,
    Host(String),
    Search(String),
    /// Reserved for slices B to D of `specs/101`; nothing in slice A produces them.
    Doi(String),
    Archive(String),
    Citation(String),
}

/// A word safe to put after a colon: lower-cased, only `[a-z0-9._-]`, never empty.
pub fn clean_id(s: &str) -> Option<String> {
    let id: String = s.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')).take(ID_CHARS).collect();
    (!id.is_empty()).then_some(id)
}

impl Source {
    pub fn as_string(&self) -> String {
        match self {
            Source::Kb => "kb".into(),
            Source::Trusted => "trusted".into(),
            Source::Host(t) => format!("host:{t}"),
            Source::Search(p) => format!("search:{p}"),
            Source::Doi(p) => format!("doi:{p}"),
            Source::Archive(p) => format!("archive:{p}"),
            Source::Citation(p) => format!("citation:{p}"),
        }
    }

    /// Parse a source name; anything outside the closed set, or an id that is not clean, is `None`.
    pub fn parse(s: &str) -> Option<Source> {
        match s {
            "kb" => return Some(Source::Kb),
            "trusted" => return Some(Source::Trusted),
            _ => {}
        }
        let (kind, id) = s.split_once(':')?;
        if clean_id(id).as_deref() != Some(id) {
            return None;
        }
        let id = id.to_string();
        Some(match kind {
            "host" => Source::Host(id),
            "search" => Source::Search(id),
            "doi" => Source::Doi(id),
            "archive" => Source::Archive(id),
            "citation" => Source::Citation(id),
            _ => return None,
        })
    }
}

/// Why a search was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Kb,
    Topic,
    Trusted,
    Gap,
    Support,
    Contradiction,
    /// A corroboration query that was not sent because the claim rests on a private source.
    Corroboration,
    Declared,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::Kb => "kb",
            Purpose::Topic => "topic",
            Purpose::Trusted => "trusted",
            Purpose::Gap => "gap",
            Purpose::Support => "support",
            Purpose::Contradiction => "contradiction",
            Purpose::Corroboration => "corroboration",
            Purpose::Declared => "declared",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Line {
    /// `kb`, `trusted`, `close` or `web`: the rungs of `specs/098` §7.1.
    pub rung: &'static str,
    pub source: Source,
    pub purpose: Purpose,
    /// The redacted string that was sent. `None` when nothing was sent (withheld) or when a declared query had nothing usable left.
    pub query: Option<String>,
    /// `true` when the query was not sent, or not recorded, because it would have carried private text.
    pub withheld: bool,
    pub at: String,
    pub hits: usize,
    pub kept: usize,
    pub duplicates: usize,
    pub not_kept: BTreeMap<&'static str, usize>,
    /// The provider that answered a trusted-list search, which has no provider of its own in `source`.
    pub via: Option<String>,
    /// One of the fixed failure words of `research_run::failure_reason` when the search itself failed.
    pub failed: Option<&'static str>,
    /// Reported by the host, not made by the daemon. Counts as nothing.
    pub declared: bool,
}

impl Line {
    fn new(rung: &'static str, source: Source, purpose: Purpose) -> Line {
        Line { rung, source, purpose, query: None, withheld: false, at: delphi_core::time::now_rfc3339(), hits: 0, kept: 0, duplicates: 0, not_kept: BTreeMap::new(), via: None, failed: None, declared: false }
    }

    /// Does the line account for every result it found? A declared line, a withheld one and the knowledge-base line are not searches the
    /// daemon ran against the world, so the question does not apply to them.
    pub fn balanced(&self) -> bool {
        self.declared || self.source == Source::Kb || self.hits == self.duplicates + self.kept + self.not_kept.values().sum::<usize>()
    }

    fn counts(&self) -> bool {
        !self.declared && self.source != Source::Kb
    }

    fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("rung".into(), json!(self.rung));
        o.insert("source".into(), json!(self.source.as_string()));
        o.insert("purpose".into(), json!(self.purpose.as_str()));
        if let Some(q) = &self.query {
            o.insert("query".into(), json!(q));
        }
        if self.withheld {
            o.insert("withheld".into(), json!("private"));
        }
        o.insert("at".into(), json!(self.at));
        o.insert("hits".into(), json!(self.hits));
        o.insert("kept".into(), json!(self.kept));
        o.insert("duplicates".into(), json!(self.duplicates));
        o.insert("not_kept".into(), json!(self.not_kept));
        if let Some(v) = &self.via {
            o.insert("via".into(), json!(v));
        }
        if let Some(f) = self.failed {
            o.insert("failed".into(), json!(f));
        }
        if self.declared {
            o.insert("declared".into(), json!(true));
        }
        Value::Object(o)
    }
}

/// The flow of a run, summed over the lines the daemon ran. `found = duplicates_removed + kept + Σ not_kept`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flow {
    pub found: usize,
    pub duplicates_removed: usize,
    pub kept: usize,
    pub not_kept: BTreeMap<&'static str, usize>,
    /// Searches the daemon ran (not counting the knowledge-base line), searches withheld, and searches a host declared.
    pub searches: usize,
    pub withheld: usize,
    pub declared: usize,
}

impl Flow {
    pub fn balanced(&self) -> bool {
        self.found == self.duplicates_removed + self.kept + self.not_kept.values().sum::<usize>()
    }

    pub fn to_json(&self) -> Value {
        json!({"found": self.found, "duplicates_removed": self.duplicates_removed, "kept": self.kept, "not_kept": self.not_kept,
               "searches": self.searches, "withheld": self.withheld, "declared": self.declared})
    }
}

/// A search the host says it ran. Only these three fields are read; any other field the host sends is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSearch {
    pub tool: String,
    pub query: String,
    pub hits: usize,
}

/// Read the optional `host_searches` array of a request: at most [`MAX_DECLARED`] objects, each with a `tool` word, a `query`
/// string and a `hits` count. A malformed entry is dropped; `kept`, `source`, `at`, `rung` and anything else is never read.
pub fn parse_declared(v: &Value) -> Vec<HostSearch> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let tool = clean_id(e["tool"].as_str()?)?;
            let query: String = e["query"].as_str()?.chars().take(QUERY_CHARS * 2).collect();
            let hits = e["hits"].as_u64().unwrap_or(0).min(1000) as usize;
            Some(HostSearch { tool, query, hits })
        })
        .take(MAX_DECLARED)
        .collect()
}

#[derive(Debug, Default)]
pub struct SearchLog {
    lines: Vec<Line>,
    /// The strictest `private` flag of the claims this run produced (`specs/101` §3 item 3).
    pub private: bool,
}

impl SearchLog {
    pub fn new() -> SearchLog {
        SearchLog::default()
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    fn push(&mut self, l: Line) -> usize {
        if self.lines.len() >= MAX_LINES {
            return usize::MAX;
        }
        self.lines.push(l);
        self.lines.len() - 1
    }

    fn at(&mut self, i: usize) -> Option<&mut Line> {
        self.lines.get_mut(i)
    }

    /// A search the daemon ran. `query` is the string that was sent. The line starts empty; set its results with [`rows`](Self::rows).
    pub fn begin(&mut self, rung: &'static str, source: Source, purpose: Purpose, query: &str, via: Option<&str>) -> usize {
        let mut l = Line::new(rung, source, purpose);
        l.query = Some(query.chars().take(QUERY_CHARS).collect());
        l.via = via.and_then(clean_id);
        self.push(l)
    }

    /// A search the daemon did not send because its claim rests on a private source: no query text is recorded.
    pub fn withheld(&mut self, source: Source, purpose: Purpose) -> usize {
        let mut l = Line::new("web", source, purpose);
        l.withheld = true;
        self.push(l)
    }

    pub fn rows(&mut self, i: usize, hits: usize) {
        if let Some(l) = self.at(i) {
            l.hits = hits;
        }
    }

    pub fn failed(&mut self, i: usize, why: &'static str) {
        if let Some(l) = self.at(i) {
            l.failed = Some(why);
        }
    }

    pub fn kept(&mut self, i: usize) {
        if let Some(l) = self.at(i) {
            l.kept += 1;
        }
    }

    pub fn duplicate(&mut self, i: usize) {
        if let Some(l) = self.at(i) {
            l.duplicates += 1;
        }
    }

    /// Results that were found and not kept, `n` of them, for a reason from [`NOT_KEPT`]. A reason outside the list is a programming
    /// error and is counted as `refused`, so the flow still balances.
    pub fn dropped(&mut self, i: usize, reason: &'static str, n: usize) {
        if n == 0 {
            return;
        }
        let reason = NOT_KEPT.iter().copied().find(|r| *r == reason).unwrap_or("refused");
        if let Some(l) = self.at(i) {
            *l.not_kept.entry(reason).or_insert(0) += n;
        }
    }

    /// The knowledge-base line, placed first: the claims the run found the library already held. It is shown, and it is not part of the flow.
    pub fn kb(&mut self, query: Option<&str>, hits: usize) {
        let mut l = Line::new("kb", Source::Kb, Purpose::Kb);
        l.query = query.map(|q| q.chars().take(QUERY_CHARS).collect());
        l.hits = hits;
        l.kept = hits;
        self.lines.insert(0, l);
    }

    /// Record a search the host says it ran. The query is cleaned the way a daemon query is; if nothing usable is left, the line
    /// says `withheld` and carries no query. The line is `declared`, its `kept` is 0, and it is excluded from the flow.
    pub fn declare(&mut self, h: &HostSearch, redactor: &Redactor) {
        let mut l = Line::new("web", Source::Host(h.tool.clone()), Purpose::Declared);
        match clean_query(&h.query, redactor) {
            Some(q) => l.query = Some(q),
            None => l.withheld = true,
        }
        l.declared = true;
        l.hits = h.hits;
        self.push(l);
    }

    pub fn flow(&self) -> Flow {
        let mut f = Flow::default();
        for l in &self.lines {
            if l.declared {
                f.declared += 1;
            }
            if l.withheld && !l.declared {
                f.withheld += 1;
            }
            if !l.counts() {
                continue;
            }
            if !l.withheld {
                f.searches += 1;
            }
            f.found += l.hits;
            f.duplicates_removed += l.duplicates;
            f.kept += l.kept;
            for (r, n) in &l.not_kept {
                *f.not_kept.entry(r).or_insert(0) += n;
            }
        }
        f
    }

    pub fn to_json(&self) -> Value {
        json!({"version": 1, "private": self.private, "lines": self.lines.iter().map(Line::to_json).collect::<Vec<_>>(), "flow": self.flow().to_json()})
    }
}

/// May this log go out through `channel`? The library's own check, on the log's `private` flag: a private log is refused everywhere.
pub fn may_leave(log: &Value, channel: Channel) -> Result<(), String> {
    library::may_leave(log, channel).map_err(|_| format!("a private search log never leaves the install ({:?})", channel))
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Where a run's log is kept: `<workspace>/research/search-logs/<research_id>.json`.
pub fn path(ws: &Path, research_id: &str) -> Option<PathBuf> {
    id_ok(research_id).then(|| ws.join("research").join("search-logs").join(format!("{research_id}.json")))
}

/// Keep a run's log (mode 0600 on Unix, written whole then moved into place).
pub fn persist(ws: &Path, research_id: &str, log: &Value) -> Result<(), String> {
    let p = path(ws, research_id).ok_or_else(|| "not a research id".to_string())?;
    let dir = p.parent().ok_or("no parent")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{research_id}.tmp"));
    let bytes = serde_json::to_vec_pretty(log).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

pub fn read(ws: &Path, research_id: &str) -> Result<Value, String> {
    let p = path(ws, research_id).ok_or_else(|| format!("`{research_id}` is not a research id"))?;
    let text = std::fs::read_to_string(&p).map_err(|_| format!("no search log for `{research_id}`: it is written only when [research] search_log is on, and only for a research job that has finished"))?;
    serde_json::from_str(&text).map_err(|e| format!("the search log for `{research_id}` is not readable: {e}"))
}

/// The log as text, for `delphi research log <id>`.
pub fn render(log: &Value, research_id: &str) -> String {
    let mut out = format!("search log for {research_id}{}\n", if log["private"] == true { "  (private: it stays on this machine)" } else { "" });
    let lines = log["lines"].as_array().cloned().unwrap_or_default();
    if lines.is_empty() {
        out.push_str("  no searches were recorded\n");
    }
    for l in &lines {
        let src = l["source"].as_str().unwrap_or("?");
        let what = match (l["query"].as_str(), l["withheld"].as_str()) {
            (Some(q), _) => format!("\"{q}\""),
            (None, Some(_)) => "(withheld: private; nothing was sent)".to_string(),
            (None, None) => "(no query)".to_string(),
        };
        let tag = if l["declared"] == true { "  [declared by the host; counts as nothing]" } else { "" };
        let failed = l["failed"].as_str().map(|f| format!("  failed: {f}")).unwrap_or_default();
        out.push_str(&format!("  {} {:<22} {:<13} {}  hits {} kept {}{}{}\n", l["at"].as_str().unwrap_or("").get(..19).unwrap_or(""), src, l["purpose"].as_str().unwrap_or(""), what, l["hits"], l["kept"], failed, tag));
    }
    let f = &log["flow"];
    out.push_str(&format!("  flow: {} found, {} duplicates removed, {} kept as noted", f["found"], f["duplicates_removed"], f["kept"]));
    if let Some(nk) = f["not_kept"].as_object().filter(|m| !m.is_empty()) {
        out.push_str(&format!(", not kept: {}", nk.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")));
    }
    out.push_str(&format!("\n  {} searches, {} withheld, {} declared by the host\n", f["searches"], f["withheld"], f["declared"]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn red() -> Redactor {
        Redactor::new()
    }

    #[test]
    fn the_source_set_is_closed_and_ids_are_clean() {
        for ok in ["kb", "trusted", "host:grep", "search:brave", "doi:crossref", "archive:ia", "citation:opencitations"] {
            assert_eq!(Source::parse(ok).map(|s| s.as_string()).as_deref(), Some(ok));
        }
        for bad in ["", "web", "search", "search:", "search:Brave", "search:a b", "host:../x", "mail:smtp", "kb:x", "search:a:b"] {
            assert!(Source::parse(bad).is_none(), "{bad:?} is outside the set");
        }
        assert_eq!(clean_id("  Web Search!  ").as_deref(), Some("websearch"));
        assert_eq!(clean_id("!!!"), None);
        assert!(clean_id(&"a".repeat(100)).unwrap().len() <= ID_CHARS);
    }

    /// A small run: a trusted search with three rows, one web search with four, one that failed.
    fn run() -> SearchLog {
        let mut log = SearchLog::new();
        let t = log.begin("trusted", Source::Trusted, Purpose::Trusted, "postgres index bloat site:postgresql.org", Some("Brave"));
        log.rows(t, 3);
        log.kept(t);
        log.kept(t);
        log.duplicate(t);
        let w = log.begin("web", Source::Search("brave".into()), Purpose::Topic, "postgres index bloat", Some("brave"));
        log.rows(w, 4);
        log.kept(w);
        log.dropped(w, "fetch_failed", 1);
        log.dropped(w, "budget", 2);
        let f = log.begin("web", Source::Search("unknown".into()), Purpose::Gap, "postgres index bloat criticism", None);
        log.failed(f, "no_results");
        log.kb(Some("postgres index bloat"), 2);
        log
    }

    #[test]
    fn every_line_balances_and_the_flow_is_a_plain_sum() {
        let log = run();
        assert!(log.lines().iter().all(|l| l.balanced()), "{:?}", log.lines());
        let f = log.flow();
        assert_eq!((f.found, f.duplicates_removed, f.kept), (7, 1, 3));
        assert_eq!(f.not_kept.get("fetch_failed"), Some(&1));
        assert_eq!(f.not_kept.get("budget"), Some(&2));
        assert!(f.balanced(), "found = duplicates + kept + not kept: {f:?}");
        assert_eq!((f.searches, f.withheld, f.declared), (3, 0, 0), "the knowledge-base line is not a search of the world");
        assert_eq!(log.lines()[0].source, Source::Kb, "the knowledge base is first");
    }

    #[test]
    fn a_reason_outside_the_list_still_balances_as_refused() {
        let mut log = SearchLog::new();
        let i = log.begin("web", Source::Search("x".into()), Purpose::Topic, "q", None);
        log.rows(i, 2);
        log.dropped(i, "because I said so", 2);
        assert_eq!(log.flow().not_kept.get("refused"), Some(&2));
        assert!(log.flow().balanced());
        assert!(NOT_KEPT.contains(&"page_cap") && NOT_KEPT.len() == 7);
    }

    #[test]
    fn the_query_is_the_redacted_string_and_a_withheld_search_carries_none() {
        let mut log = SearchLog::new();
        log.withheld(Source::Search("brave".into()), Purpose::Corroboration);
        let j = log.to_json();
        let line = &j["lines"][0];
        assert_eq!(line["withheld"], "private");
        assert!(line.get("query").is_none(), "no query text at all: {line}");
        assert_eq!(j["flow"]["withheld"], 1);
        assert_eq!(j["flow"]["searches"], 0, "a withheld search was not run");
        let text = serde_json::to_string(&j).unwrap();
        assert!(!text.contains("secret"), "{text}");
    }

    #[test]
    fn a_host_declared_search_counts_as_nothing() {
        let mut log = run();
        let before = log.flow();
        let h = HostSearch { tool: "slack".into(), query: "what did we decide about index bloat".into(), hits: 12 };
        log.declare(&h, &red());
        let after = log.flow();
        assert_eq!((after.found, after.kept, after.duplicates_removed, after.searches), (before.found, before.kept, before.duplicates_removed, before.searches), "unchanged");
        assert_eq!(after.declared, 1);
        let l = log.lines().last().unwrap();
        assert!(l.declared && l.kept == 0 && l.hits == 12);
        assert_eq!(l.source, Source::Host("slack".into()));
        assert!(log.to_json()["lines"].as_array().unwrap().last().unwrap()["declared"] == true);
    }

    #[test]
    fn a_declared_query_with_private_text_is_not_recorded() {
        let mut log = SearchLog::new();
        log.declare(&HostSearch { tool: "grep".into(), query: "/srv/team/secrets/project/notes.md".into(), hits: 1 }, &red());
        let l = &log.lines()[0];
        assert!(l.withheld && l.query.is_none(), "{l:?}");
    }

    #[test]
    fn the_host_sets_only_tool_query_and_hits() {
        let v = json!([
            {"tool": "Slack Search", "query": "q1", "hits": 3, "kept": 99, "source": "kb", "at": "1999-01-01T00:00:00Z", "rung": "kb", "declared": false, "withheld": "no"},
            {"tool": "", "query": "no tool"},
            {"query": "no tool at all"},
            {"tool": "x", "hits": 1},
            "not an object",
            {"tool": "ok", "query": "q2", "hits": 5000}
        ]);
        let got = parse_declared(&v);
        assert_eq!(got, vec![HostSearch { tool: "slacksearch".into(), query: "q1".into(), hits: 3 }, HostSearch { tool: "ok".into(), query: "q2".into(), hits: 1000 }]);
        // and what comes out of a declaration cannot claim a kept result or another source
        let mut log = SearchLog::new();
        for h in &got {
            log.declare(h, &red());
        }
        for l in log.lines() {
            assert!(l.declared && l.kept == 0 && l.duplicates == 0 && l.not_kept.is_empty(), "{l:?}");
            assert!(matches!(l.source, Source::Host(_)));
            assert_ne!(l.at, "1999-01-01T00:00:00Z", "the daemon stamps the time");
        }
        assert_eq!(parse_declared(&json!((0..40).map(|i| json!({"tool": "t", "query": format!("q{i}")})).collect::<Vec<_>>())).len(), MAX_DECLARED);
        assert!(parse_declared(&json!("nope")).is_empty() && parse_declared(&Value::Null).is_empty());
    }

    #[test]
    fn a_private_log_is_refused_on_every_outbound_channel_and_a_public_one_is_not_changed() {
        let mut log = run();
        for ch in Channel::ALL {
            assert!(may_leave(&log.to_json(), ch).is_ok(), "{ch:?}");
        }
        log.private = true;
        for ch in Channel::ALL {
            let e = may_leave(&log.to_json(), ch).unwrap_err();
            assert!(e.contains("private search log"), "{ch:?}: {e}");
        }
    }

    #[test]
    fn the_log_is_kept_whole_read_back_and_named_only_by_a_clean_id() {
        let d = tempfile::tempdir().unwrap();
        let log = run().to_json();
        persist(d.path(), "r-t-0007", &log).unwrap();
        assert_eq!(read(d.path(), "r-t-0007").unwrap(), log);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path(d.path(), "r-t-0007").unwrap()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "it may be private");
        }
        for bad in ["", "../x", "a/b", "a b", &"x".repeat(65)] {
            assert!(path(d.path(), bad).is_none(), "{bad:?}");
            assert!(persist(d.path(), bad, &log).is_err());
            assert!(read(d.path(), bad).is_err());
        }
        assert!(read(d.path(), "r-none").unwrap_err().contains("no search log"));
    }

    #[test]
    fn the_text_view_names_what_was_searched_withheld_and_declared() {
        let mut log = run();
        log.withheld(Source::Search("brave".into()), Purpose::Corroboration);
        log.declare(&HostSearch { tool: "slack".into(), query: "index bloat thread".into(), hits: 2 }, &red());
        log.private = true;
        let t = render(&log.to_json(), "r-1");
        for needle in ["search log for r-1", "(private", "search:brave", "\"postgres index bloat\"", "withheld: private", "declared by the host", "flow: 7 found, 1 duplicates removed, 3 kept", "budget 2", "failed: no_results"] {
            assert!(t.contains(needle), "{needle}\n{t}");
        }
    }
}
