//! The research job (`specs/028` §10.2): from a topic to new documents in the archive, bounded, and mostly code.
//!
//! 1. **Search** the trusted sources: site-restricted queries to the first search engine that answers; every row is
//!    filtered by the allow list in code ([`research_search::filter_rows`]).
//! 2. **Read, and follow references** (depth 1, capped): the pages found, and the *listed* pages they link to. A link to an
//!    unlisted host is recorded as "seen, not read" and never fetched, and the allow list is checked before every request.
//! 3. **Decide** (the scout, one `fast`-tier call for all the pages): per page, keep it whole, keep a summary and facts, or
//!    keep nothing. The scout reads the pages as untrusted text through [`Scout`], which has no tools.
//! 4. **Write**: the daemon writes the documents; what may be stored whole is the list's decision, and a fact is kept only
//!    if its quote is in the page ([`research_store`]).
//!
//! Everything here is behind [`Http`] and [`Scout`] so it is tested offline. The scout's call is the only spend of tokens.

use crate::research_fetch::{fetch, sort_links, FetchError, Fetched, Http, Limits};
use crate::research_search::{filter_rows, search, site_queries, Provider, SearchError};
use crate::research_sources::{form, Allow, Entry, Form};
use crate::research_store::{keep, logical_path, write, Keep, Provenance, Verdict, Wrote};
use std::path::Path;
use std::time::Instant;

/// The pages the scout reads, as untrusted text.
#[derive(Debug, Clone)]
pub struct PageForScout {
    pub url: String,
    pub publisher: String,
    pub licence: String,
    /// What the list allows storing of this page (the scout is told, but the daemon enforces it).
    pub form: Form,
    pub title: String,
    pub text: String,
}

/// The model's side of the job. A real scout makes one call with no tools; a test supplies the verdicts.
pub trait Scout {
    fn decide(&self, topic: &str, pages: &[PageForScout]) -> Result<Vec<Verdict>, String>;
}

/// What the page shows while the job runs (counters only; no model is involved in any of it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Searching,
    /// The search answered: how many listed rows, how many skipped (not on the list), and which engine.
    Found { listed: usize, skipped: usize, engine: String },
    Reading { url: String },
    Read { url: String, host: String, ok: bool, note: String },
    Deciding { pages: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub path: String,
    pub url: String,
    pub host: String,
    pub form: Form,
    pub licence: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub engine: String,
    pub read: usize,
    pub skipped_unlisted: usize,
    pub new: Vec<Written>,
    pub updated: usize,
    pub unchanged: usize,
    /// Pages read and not kept, with the reason.
    pub not_kept: Vec<(String, String)>,
    /// Hosts linked from the pages that are not on the list: never fetched.
    pub seen_not_read: Vec<String>,
    pub cancelled: bool,
    pub stopped_at_time_limit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobError {
    Search(SearchError),
    /// The scout failed: nothing is written.
    Scout(String),
    /// Nothing listed answered the search.
    NothingFound,
    Io(String),
}

impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobError::Search(e) => write!(f, "{}", e),
            JobError::Scout(e) => write!(f, "the scout could not decide: {} (nothing was written)", e),
            JobError::NothingFound => write!(f, "no page on the trusted list matched the topic"),
            JobError::Io(e) => write!(f, "could not write the entries: {}", e),
        }
    }
}

pub struct Job<'a> {
    pub topic: &'a str,
    pub mission: &'a str,
    /// An ISO time, for the documents' `retrieved`.
    pub now: &'a str,
    /// The directory that holds `knowledge/web/…`.
    pub base: &'a Path,
    pub allow: &'a Allow,
    pub providers: &'a [Provider],
    pub limits: Limits,
}

/// At most this many characters of one page go to the scout, and of all pages together.
const PAGE_CHARS: usize = 12_000;
const TOTAL_CHARS: usize = 60_000;

pub fn run(job: &Job<'_>, http: &dyn Http, scout: &dyn Scout, cancelled: &dyn Fn() -> bool, progress: &mut dyn FnMut(Progress)) -> Result<Outcome, JobError> {
    let started = Instant::now();
    let mut out = Outcome::default();
    let timed_out = |s: &Instant| s.elapsed() > job.limits.job_timeout;

    // 1. search: one engine at a time, and once one has answered it is the only one this job asks.
    progress(Progress::Searching);
    let mut providers: Vec<Provider> = job.providers.to_vec();
    let mut rows = Vec::new();
    for q in site_queries(job.topic, &job.allow.exact_hosts(), 6, 3) {
        if cancelled() {
            out.cancelled = true;
            return Ok(out);
        }
        match search(&providers, &q, http, job.limits.fetch_timeout) {
            Ok((r, who)) => {
                out.engine = who.to_string();
                providers.retain(|p| p.kind.name() == who);
                rows.extend(r);
            }
            Err(e) if rows.is_empty() && out.engine.is_empty() => return Err(JobError::Search(e)),
            Err(_) => break,
        }
    }
    let (listed, skipped) = filter_rows(rows, job.allow);
    out.skipped_unlisted = skipped;
    progress(Progress::Found { listed: listed.len(), skipped, engine: out.engine.clone() });
    if listed.is_empty() {
        return Err(JobError::NothingFound);
    }

    // 2. read the pages found, then the listed pages they link to (depth 1), within the cap.
    let mut pages: Vec<Fetched> = Vec::new();
    let mut have: Vec<String> = Vec::new();
    let mut queue: Vec<(String, usize)> = listed.iter().map(|(r, _)| (r.url.clone(), 0)).collect();
    let mut qi = 0;
    while qi < queue.len() && pages.len() < job.limits.max_sources {
        if cancelled() {
            out.cancelled = true;
            return Ok(out);
        }
        if timed_out(&started) {
            out.stopped_at_time_limit = true;
            break;
        }
        let (url, depth) = queue[qi].clone();
        qi += 1;
        if have.contains(&url) {
            continue;
        }
        progress(Progress::Reading { url: url.clone() });
        match fetch(http, job.allow, &url, &job.limits) {
            Ok(f) => {
                have.push(url.clone());
                have.push(f.final_url.clone());
                progress(Progress::Read { url: url.clone(), host: f.entry.host.clone(), ok: true, note: String::new() });
                if depth < job.limits.depth {
                    let room = job.limits.max_sources.saturating_sub(pages.len() + queue.len().saturating_sub(qi) + 1);
                    let follow = sort_links(&f.links, job.allow, &have, room);
                    for h in follow.seen_not_read {
                        if !out.seen_not_read.contains(&h) {
                            out.seen_not_read.push(h);
                        }
                    }
                    for u in follow.follow {
                        if !queue.iter().any(|(q, _)| *q == u) {
                            queue.push((u, depth + 1));
                        }
                    }
                }
                pages.push(f);
            }
            Err(e) => {
                let host = crate::research_sources::https_host(&url).map(|h| h.0).unwrap_or_default();
                let note = match &e {
                    FetchError::OffList(_) => "not on the trusted list".to_string(),
                    other => other.to_string(),
                };
                out.not_kept.push((url.clone(), note.clone()));
                progress(Progress::Read { url, host, ok: false, note });
            }
        }
    }
    out.read = pages.len();
    if pages.is_empty() {
        return Err(JobError::NothingFound);
    }

    // 3. decide: one scout call for all the pages, with what each page's list entry allows.
    let mut budget = TOTAL_CHARS;
    let for_scout: Vec<PageForScout> = pages
        .iter()
        .map(|p| {
            let take = PAGE_CHARS.min(budget);
            budget -= take;
            PageForScout { url: p.final_url.clone(), publisher: p.entry.publisher.clone(), licence: p.entry.licence.clone(), form: form(&p.entry, &p.markdown), title: p.title.clone(), text: p.markdown.chars().take(take).collect() }
        })
        .collect();
    if cancelled() {
        out.cancelled = true;
        return Ok(out);
    }
    progress(Progress::Deciding { pages: for_scout.len() });
    let verdicts = scout.decide(job.topic, &for_scout).map_err(JobError::Scout)?;

    // 4. write: the daemon, from what the list allows and what the page really says.
    for p in &pages {
        if cancelled() {
            out.cancelled = true;
            break;
        }
        let Some(v) = verdicts.iter().find(|v| v.url == p.final_url || v.url == p.url) else {
            out.not_kept.push((p.final_url.clone(), "the scout did not choose it".into()));
            continue;
        };
        if v.keep == Keep::Nothing {
            out.not_kept.push((p.final_url.clone(), "nothing pertinent".into()));
            continue;
        }
        // The scout read a clipped page; the whole page is what a full entry stores and what a quote is checked against.
        let allowed = form(&p.entry, &p.markdown);
        let Some(k) = keep(v, &p.title, &p.markdown, allowed) else {
            out.not_kept.push((p.final_url.clone(), "no usable summary or facts".into()));
            continue;
        };
        let host = crate::research_sources::https_host(&p.final_url).unwrap_or_default();
        let logical = logical_path(&p.entry.host, &host.1);
        let prov = Provenance { mission: job.mission, topic: job.topic, retrieved: job.now, entry: &p.entry as &Entry };
        let stored = k.form;
        match write(job.base, &logical, &k, &prov) {
            Ok(Wrote::New) => out.new.push(Written { path: logical, url: p.final_url.clone(), host: p.entry.host.clone(), form: stored, licence: p.entry.licence.clone() }),
            Ok(Wrote::Updated) => out.updated += 1,
            Ok(Wrote::Unchanged) => {
                out.unchanged += 1;
                out.not_kept.push((p.final_url.clone(), "duplicate: the archive already holds it".into()));
            }
            Err(e) => return Err(JobError::Io(e.to_string())),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research_fetch::tests::Fake;
    use crate::research_search::Kind;
    use crate::research_store::Fact;
    use std::cell::RefCell;

    fn e(h: &str, reuse: &str, lic: &str) -> Entry {
        Entry { host: h.into(), publisher: format!("pub {h}"), kind: "encyclopedia".into(), licence: lic.into(), reuse: reuse.into(), block: false }
    }
    fn allow() -> Allow {
        Allow::union(&[e("en.wikipedia.org", "full", "CC-BY-SA-4.0"), e("arxiv.org", "summary-only", "unknown"), e("nist.gov", "full", "LicenseRef-PD-US-Gov")], &[])
    }
    fn brave() -> Provider {
        Provider { kind: Kind::Brave, key: Some("K".into()), base: Some("https://brave.test/api".into()), cx: None }
    }
    const WIKI: &str = "<html><head><title>Byzantine fault</title></head><body><h1>Byzantine fault</h1><p>A Byzantine fault is a condition in which components fail in arbitrary ways. See <a href=\"https://nist.gov/glossary\">NIST</a> and <a href=\"https://spam.example.com/p\">spam</a>.</p></body></html>";
    const NIST: &str = "<html><head><title>Glossary</title></head><body><p>Fault tolerance is the ability to keep working despite faults.</p></body></html>";
    const ARXIV: &str = "<html><head><title>PBFT</title></head><body><p>Practical Byzantine fault tolerance tolerates Byzantine faults in asynchronous systems. All rights reserved.</p></body></html>";

    fn search_url() -> String {
        format!("https://brave.test/api?q={}&count=20", crate::research_search::encode("byzantine fault (site:en.wikipedia.org OR site:arxiv.org OR site:nist.gov)"))
    }
    fn answer(urls: &[&str]) -> String {
        format!("{{\"web\":{{\"results\":[{}]}}}}", urls.iter().map(|u| format!("{{\"title\":\"t\",\"url\":\"{u}\",\"description\":\"d\"}}")).collect::<Vec<_>>().join(","))
    }
    fn http() -> Fake {
        Fake::default()
            .page(&search_url(), 200, "application/json", &answer(&["https://en.wikipedia.org/wiki/BFT", "https://arxiv.org/abs/pbft", "https://spam.example.com/x"]))
            .page("https://en.wikipedia.org/wiki/BFT", 200, "text/html", WIKI)
            .page("https://nist.gov/glossary", 200, "text/html", NIST)
            .page("https://arxiv.org/abs/pbft", 200, "text/html", ARXIV)
            .page("https://spam.example.com/p", 200, "text/html", "SHOULD NEVER BE FETCHED")
            .page("https://spam.example.com/x", 200, "text/html", "SHOULD NEVER BE FETCHED")
    }

    struct Fixed(Result<Vec<Verdict>, String>, RefCell<Vec<PageForScout>>);
    impl Scout for Fixed {
        fn decide(&self, _t: &str, pages: &[PageForScout]) -> Result<Vec<Verdict>, String> {
            *self.1.borrow_mut() = pages.to_vec();
            self.0.clone()
        }
    }
    fn verdict(url: &str, keep: Keep, summary: &str, facts: Vec<Fact>) -> Verdict {
        Verdict { url: url.into(), keep, summary: summary.into(), facts }
    }
    fn job<'a>(base: &'a Path, a: &'a Allow, ps: &'a [Provider]) -> Job<'a> {
        Job { topic: "byzantine fault", mission: "m_1", now: "2026-10-28T10:00:00Z", base, allow: a, providers: ps, limits: Limits::default() }
    }

    #[test]
    fn a_topic_becomes_documents_and_nothing_unlisted_is_ever_requested() {
        let base = tempfile::tempdir().unwrap();
        let (a, ps, h) = (allow(), [brave()], http());
        let scout = Fixed(
            Ok(vec![
                verdict("https://en.wikipedia.org/wiki/BFT", Keep::Full, "", vec![]),
                verdict("https://nist.gov/glossary", Keep::Full, "", vec![]),
                verdict("https://arxiv.org/abs/pbft", Keep::Summary, "PBFT tolerates faults.", vec![Fact { fact: "it tolerates faults".into(), quote: "tolerates Byzantine faults in asynchronous systems".into() }, Fact { fact: "invented".into(), quote: "it won a prize".into() }]),
            ]),
            RefCell::default(),
        );
        let mut events = Vec::new();
        let out = run(&job(base.path(), &a, &ps), &h, &scout, &|| false, &mut |p| events.push(p)).unwrap();
        assert_eq!((out.engine.as_str(), out.read, out.skipped_unlisted), ("brave", 3, 1));
        assert_eq!(out.new.len(), 3, "{:?}", out);
        let asked = h.asked.lock().unwrap().clone();
        assert!(!asked.iter().any(|u| u.contains("spam.example.com")), "an unlisted host is never requested: {asked:?}");
        assert_eq!(out.seen_not_read, ["spam.example.com"], "the link to it was noted, not followed");
        let forms: Vec<(String, Form)> = out.new.iter().map(|w| (w.host.clone(), w.form)).collect();
        assert!(forms.contains(&("en.wikipedia.org".into(), Form::Full)) && forms.contains(&("nist.gov".into(), Form::Full)) && forms.contains(&("arxiv.org".into(), Form::Summary)));
        let arxiv = std::fs::read_to_string(base.path().join("knowledge/web/arxiv.org/abs-pbft.md")).unwrap();
        assert!(arxiv.contains("tolerates Byzantine faults in asynchronous systems") && !arxiv.contains("won a prize") && !arxiv.contains("Practical Byzantine fault tolerance tolerates Byzantine faults in asynchronous systems. All"), "{arxiv}");
        let wiki = std::fs::read_to_string(base.path().join("knowledge/web/en.wikipedia.org/wiki-bft.md")).unwrap();
        assert!(wiki.contains("share_alike: true") && wiki.contains("kind: full") && wiki.contains("A Byzantine fault is a condition"));
        assert!(events.contains(&Progress::Searching) && events.iter().any(|p| matches!(p, Progress::Found { listed: 2, skipped: 1, .. })) && events.iter().any(|p| matches!(p, Progress::Deciding { pages: 3 })));
        // The scout was told what each page may be stored as: the arxiv page says "all rights reserved" and its entry is summary-only.
        let seen = scout.1.borrow();
        assert_eq!(seen.iter().find(|p| p.url.contains("arxiv")).unwrap().form, Form::Summary);
        assert_eq!(seen.iter().find(|p| p.url.contains("wikipedia")).unwrap().form, Form::Full);
    }

    #[test]
    fn what_the_scout_asks_never_exceeds_what_the_list_allows_and_a_repeat_run_adds_nothing() {
        let base = tempfile::tempdir().unwrap();
        let (a, ps, h) = (allow(), [brave()], http());
        // The scout asks for the whole arxiv page, which is summary-only: it is stored as a summary or not at all.
        let scout = Fixed(Ok(vec![verdict("https://arxiv.org/abs/pbft", Keep::Full, "PBFT.", vec![]), verdict("https://en.wikipedia.org/wiki/BFT", Keep::Full, "", vec![])]), RefCell::default());
        let out = run(&job(base.path(), &a, &ps), &h, &scout, &|| false, &mut |_| {}).unwrap();
        let arxiv = out.new.iter().find(|w| w.host == "arxiv.org").unwrap();
        assert_eq!(arxiv.form, Form::Summary);
        let text = std::fs::read_to_string(base.path().join(&arxiv.path)).unwrap();
        assert!(!text.contains("asynchronous systems"), "the page body was not copied: {text}");
        let again = run(&job(base.path(), &a, &ps), &http(), &scout, &|| false, &mut |_| {}).unwrap();
        assert_eq!((again.new.len(), again.unchanged), (0, 2), "the same pages again add nothing");
        assert!(again.not_kept.iter().any(|(_, why)| why.starts_with("duplicate")));
    }

    #[test]
    fn a_scout_that_fails_writes_nothing_and_so_does_a_scout_that_answers_for_unknown_pages() {
        let base = tempfile::tempdir().unwrap();
        let (a, ps) = (allow(), [brave()]);
        let failing = Fixed(Err("quota out".into()), RefCell::default());
        let err = run(&job(base.path(), &a, &ps), &http(), &failing, &|| false, &mut |_| {}).unwrap_err();
        assert_eq!(err, JobError::Scout("quota out".into()));
        assert!(err.to_string().contains("nothing was written"));
        assert!(!base.path().join("knowledge").exists());
        let strange = Fixed(Ok(vec![verdict("https://evil.example.com/not-fetched", Keep::Full, "x", vec![])]), RefCell::default());
        let out = run(&job(base.path(), &a, &ps), &http(), &strange, &|| false, &mut |_| {}).unwrap();
        assert!(out.new.is_empty() && out.not_kept.iter().all(|(_, why)| why == "the scout did not choose it"), "{:?}", out.not_kept);
    }

    #[test]
    fn search_problems_are_reported_and_nothing_is_read() {
        let base = tempfile::tempdir().unwrap();
        let a = allow();
        let h = Fake::default();
        let scout = Fixed(Ok(vec![]), RefCell::default());
        assert_eq!(run(&job(base.path(), &a, &[]), &h, &scout, &|| false, &mut |_| {}).unwrap_err(), JobError::Search(SearchError::NotConfigured));
        assert!(matches!(run(&job(base.path(), &a, &[brave()]), &h, &scout, &|| false, &mut |_| {}).unwrap_err(), JobError::Search(SearchError::AllFailed(_))), "an unreachable engine");
        let none = Fake::default().page(&search_url(), 200, "application/json", &answer(&["https://spam.example.com/x"]));
        assert_eq!(run(&job(base.path(), &a, &[brave()]), &none, &scout, &|| false, &mut |_| {}).unwrap_err(), JobError::NothingFound);
        assert_eq!(none.asked.lock().unwrap().len(), 1, "only the search was made");
    }

    #[test]
    fn cancelling_stops_the_job_and_entries_already_written_stay() {
        let base = tempfile::tempdir().unwrap();
        let (a, ps, h) = (allow(), [brave()], http());
        let scout = Fixed(Ok(vec![verdict("https://en.wikipedia.org/wiki/BFT", Keep::Full, "", vec![]), verdict("https://nist.gov/glossary", Keep::Full, "", vec![])]), RefCell::default());
        let calls = std::cell::Cell::new(0);
        // Cancel after the scout has decided and one entry is written: the polls are search, 3 reads, the pre-decision check, then each write.
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() >= 7
        };
        let out = run(&job(base.path(), &a, &ps), &h, &scout, &cancel, &mut |_| {}).unwrap();
        assert!(out.cancelled);
        assert!(out.new.len() < 3, "{:?}", out.new.len());
        let early = run(&job(base.path(), &a, &ps), &http(), &scout, &|| true, &mut |_| {}).unwrap();
        assert!(early.cancelled && early.new.is_empty() && early.read == 0, "cancelled before anything was read");
    }

    #[test]
    fn the_source_cap_bounds_the_pages_read() {
        let base = tempfile::tempdir().unwrap();
        let (a, ps, h) = (allow(), [brave()], http());
        let scout = Fixed(Ok(vec![]), RefCell::default());
        let mut j = job(base.path(), &a, &ps);
        j.limits.max_sources = 1;
        let out = run(&j, &h, &scout, &|| false, &mut |_| {}).unwrap();
        assert_eq!(out.read, 1);
        assert_eq!(h.asked.lock().unwrap().iter().filter(|u| !u.contains("brave.test")).count(), 1, "one page was requested");
    }
}
