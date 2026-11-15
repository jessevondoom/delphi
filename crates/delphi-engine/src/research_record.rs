//! The research record (`specs/112` §2): what one research topic leaves on disk, written as the run goes.
//!
//! A topic is a folder, `<root>/research/topics/<topic-id>/`, where `<root>` is the workspace, or the project's own folder when the
//! run belongs to a registered project. It holds three files:
//!
//! * `record.json`: the question as asked, who asked, the project, the depth, the state, the plan, every query with its provider and
//!   count of results, every page read with its source id and content hash, the pages refused and why, the claim ids, the gaps, the
//!   step and time use, and `runs`, one entry per time the topic was run (`schemas/research-record.schema.json`);
//! * `answer.md`: the written answer of the newest run that drew claims, with what earlier runs wrote below it;
//! * `search-log.json`: the search log of `specs/101` §3 for that run, moved here from `research/search-logs/`.
//!
//! **Written as the run goes.** Every change is read, applied and written whole to a temporary file that is then renamed over the
//! record, so a run that is stopped leaves a record that says how far it got and never a half-written one.
//!
//! **Read from disk.** `research_result` and `research_status` answer from these files ([`result_of`], [`status_of`]), so a result
//! is there for as long as the record is, and survives a restart of the daemon. Which topic a research id belongs to is one small
//! pointer file per run, `<workspace>/research/runs/<session>.<research-id>.json`, named by the session that asked: a job is still
//! read only by the session that asked for it.
//!
//! **Record text is data** (§2.6). The question came from whoever asked and the answer was made from pages off the web: the record
//! says `taint: external_untrusted`, and nothing here reads a field of it as an instruction, a permission or a reason to trust.
//! This module opens no connection and calls no model.

use delphi_core::research::Depth;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const VERSION: u64 = 1;
/// The states of a record and of each of its runs (§2.2).
pub const STATES: [&str; 6] = ["queued", "running", "done", "partial", "failed", "cancelled"];
/// Who did a run's research: Delphi's own runner, a host with its own tools (`specs/113` §2), or a file already on disk (§3).
pub const RUNNERS: [&str; 3] = ["daemon", "host", "ingest"];
/// The tier every record and answer carries: text from outside, never validated by being written down.
pub const TAINT: &str = "external_untrusted";
/// What a list in the record holds at most. Past it nothing more is added; the counts in `runs` still say how much there was.
const LIST_CAP: usize = 500;
/// What the library kept of a page, and what the capture did (`Topic::kept`).
pub const KEPT_LEVELS: [&str; 3] = ["full", "excerpts", "pointer"];
pub const KEPT_OUTCOMES: [&str; 4] = ["added", "new_version", "unchanged", "kept_richer"];
const TEXT_CAP: usize = 300;
/// Who made a run's plan (`Topic::planned`).
pub const PLAN_KINDS: [&str; 3] = ["scout", "fixed", "continued"];
/// Why a run stopped sending queries (`research_method::Stop`).
pub const STOPS: [&str; 6] = ["covered", "no_new_claims", "step_budget", "page_budget", "no_more_queries", "plan_done"];
/// The statuses a sub-question of a plan carries.
pub const SUB_STATUSES: [&str; 3] = ["known", "partly", "open"];
/// A plan has five sub-questions at most; a record keeps no more than this of whatever it is handed.
const SUBS_CAP: usize = 8;
const SUB_TEXT_CAP: usize = 200;

/// One sub-question as the record keeps it: its number, its text and its status, and, once the run has ended, whether it was
/// covered and by how many claims from how many origins (`specs/114` §4 item 3). `None` for an entry with no number, no text or a
/// status that is not one of [`SUB_STATUSES`].
fn sub_question(q: &Value) -> Option<Value> {
    let (n, text, status) = (q["n"].as_u64().filter(|n| (1..=99).contains(n))?, clip(q["text"].as_str()?, SUB_TEXT_CAP), q["status"].as_str().filter(|s| SUB_STATUSES.contains(s))?);
    if text.is_empty() {
        return None;
    }
    let mut out = json!({"n": n, "text": text, "status": status});
    if let Some(covered) = q["covered"].as_bool() {
        out["covered"] = json!(covered);
        out["claims"] = json!(q["claims"].as_u64().unwrap_or(0));
        out["origins"] = json!(q["origins"].as_u64().unwrap_or(0));
    }
    Some(out)
}
/// A continued run's note: one line, at most this long.
pub const NOTE_CAP: usize = 120;
const SLUG_CAP: usize = 48;

/// One process writes a record at a time: each change is a read, an edit and a rename, and two runs may share a topic.
static WRITE: Mutex<()> = Mutex::new(());

fn now() -> String {
    delphi_core::time::now_rfc3339()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn clip(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect()
}

/// The id of the topic a question belongs to: a slug of the question and eight hex characters of its hash (§2.1). The hash is of
/// the question with its case and spacing folded, so the same question asked again is the same topic.
pub fn topic_id(question: &str) -> String {
    let folded = question.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let mut slug = String::new();
    for w in folded.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        if slug.len() + w.len() + 1 > SLUG_CAP {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(w);
    }
    if slug.is_empty() {
        slug.push_str("topic");
    }
    format!("{}-{}", slug, &sha256_hex(folded.as_bytes())[..8])
}

/// A topic id as this module makes them: lowercase words and digits joined by `-`, ending in eight hex characters. Nothing else
/// is ever joined to a path.
pub fn valid_topic_id(id: &str) -> bool {
    let ok_chars = !id.is_empty() && id.len() <= SLUG_CAP + 9 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let tail = id.rsplit('-').next().unwrap_or("");
    ok_chars && !id.starts_with('-') && !id.contains("--") && tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_run_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn valid_session(s: &str) -> bool {
    (8..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// `<root>/research/topics`.
pub fn topics_dir(root: &Path) -> PathBuf {
    root.join("research").join("topics")
}

/// Write `bytes` to `path` whole: a temporary file beside it (mode 0600 on Unix), then a rename over it.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = path.parent().ok_or("no parent")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("no file name")?;
    let tmp = dir.join(format!(".{name}.{}.{}.tmp", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Who asked (§2.2): the operator, a Delphi role, or a host and the model it says it runs. A label, and nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asker {
    pub kind: &'static str,
    pub label: String,
}

impl Asker {
    /// A host agent: `label` is `host:<tool>/<model>` (`host_rpc::Caller::label`).
    pub fn host(label: &str) -> Asker {
        Asker { kind: "host", label: clip(label, 120) }
    }

    pub fn operator() -> Asker {
        Asker { kind: "operator", label: "operator".into() }
    }

    pub fn role(role: &str) -> Asker {
        Asker { kind: "role", label: clip(role, 60) }
    }

    fn to_json(&self) -> Value {
        json!({"kind": self.kind, "label": self.label})
    }
}

/// What starting a run needs to say.
pub struct Begin<'a> {
    pub question: &'a str,
    pub asked_by: Asker,
    /// The registered project the run belongs to, by name, when it has one. The record then lives in that project's folder.
    pub project: Option<&'a str>,
    pub depth: Depth,
    /// The run's id (a host run's `research_id`).
    pub run_id: &'a str,
}

fn depth_word(d: Depth) -> &'static str {
    match d {
        Depth::Quick => "quick",
        Depth::Survey => "survey",
        Depth::Deep => "deep",
    }
}

/// One topic's folder.
#[derive(Debug, Clone)]
pub struct Topic {
    pub id: String,
    pub dir: PathBuf,
}

impl Topic {
    /// The topic `id` under `root` (a workspace or a project folder), or `None` for an id this module did not make.
    pub fn at(root: &Path, id: &str) -> Option<Topic> {
        valid_topic_id(id).then(|| Topic { id: id.to_string(), dir: topics_dir(root).join(id) })
    }

    pub fn record_path(&self) -> PathBuf {
        self.dir.join("record.json")
    }

    pub fn answer_path(&self) -> PathBuf {
        self.dir.join("answer.md")
    }

    pub fn search_log_path(&self) -> PathBuf {
        self.dir.join("search-log.json")
    }

    pub fn exists(&self) -> bool {
        self.record_path().is_file()
    }

    /// The record as it is on disk.
    pub fn read(&self) -> Result<Value, String> {
        let text = std::fs::read_to_string(self.record_path()).map_err(|_| format!("no research record for `{}`", self.id))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("the research record for `{}` is not readable: {e}", self.id))?;
        if v["topic_id"].as_str() != Some(self.id.as_str()) {
            return Err(format!("the research record in `{}` is not that topic's", self.id));
        }
        Ok(v)
    }

    /// The written answer of the newest finished run: the text of `answer.md` below its frontmatter and heading. Empty when none is kept.
    pub fn answer(&self) -> String {
        std::fs::read_to_string(self.answer_path()).map(|t| answer_body(&t)).unwrap_or_default()
    }

    pub fn search_log(&self) -> Option<Value> {
        serde_json::from_str(&std::fs::read_to_string(self.search_log_path()).ok()?).ok()
    }

    /// Start a run on the topic of `b.question` under `root`: the record is made if this is the topic's first run, and gains one
    /// entry in `runs`, state `queued`. A record that is already there keeps everything earlier runs wrote.
    pub fn begin(root: &Path, b: &Begin<'_>) -> Result<Topic, String> {
        if !valid_run_id(b.run_id) {
            return Err("not a research id".into());
        }
        let question = b.question.trim();
        if question.is_empty() {
            return Err("a research topic needs a question".into());
        }
        let id = topic_id(question);
        let topic = Topic { dir: topics_dir(root).join(&id), id };
        let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
        let at = now();
        let mut rec = topic.read().unwrap_or_else(|_| {
            json!({
                "version": VERSION, "topic_id": topic.id, "question": question, "asked_by": b.asked_by.to_json(), "project": b.project,
                "taint": TAINT, "private": false, "created": at,
                "plan": [], "queries": [], "pages": [], "refused": [], "claims": [], "gaps": [],
                "usage": {"steps": 0, "check_steps": 0, "ms": 0, "tokens": null}, "runs": [],
            })
        });
        rec["depth"] = json!(depth_word(b.depth));
        rec["state"] = json!("queued");
        rec["updated"] = json!(at);
        let run = json!({"run_id": b.run_id, "asked_by": b.asked_by.to_json(), "depth": depth_word(b.depth), "state": "queued", "started": at, "ended": null, "steps": 0, "check_steps": 0, "ms": 0});
        match rec["runs"].as_array_mut() {
            Some(runs) => {
                runs.retain(|r| r["run_id"].as_str() != Some(b.run_id));
                runs.push(run);
            }
            None => rec["runs"] = json!([run]),
        }
        topic.write(&rec)?;
        Ok(topic)
    }

    fn write(&self, rec: &Value) -> Result<(), String> {
        write_atomic(&self.record_path(), &serde_json::to_vec_pretty(rec).map_err(|e| e.to_string())?)
    }

    /// One step of a run: read the record, change it, and put it back whole (§2.4).
    fn update(&self, f: impl FnOnce(&mut Value)) -> Result<(), String> {
        let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
        let mut rec = self.read()?;
        f(&mut rec);
        rec["updated"] = json!(now());
        self.write(&rec)
    }

    fn push(rec: &mut Value, list: &str, item: Value) {
        if let Some(v) = rec[list].as_array_mut() {
            if v.len() < LIST_CAP {
                v.push(item);
            }
        }
    }

    fn set_run(rec: &mut Value, run_id: &str, f: impl FnOnce(&mut Value)) {
        if let Some(run) = rec["runs"].as_array_mut().and_then(|runs| runs.iter_mut().find(|r| r["run_id"].as_str() == Some(run_id))) {
            f(run);
        }
    }

    /// Who did the research of a run (`specs/113` §2 item 2, §3 item 1): one of [`RUNNERS`]. A record without the field was written by
    /// Delphi's own runner. The newest run's runner is also the record's.
    pub fn runner(&self, run_id: &str, runner: &str) -> Result<(), String> {
        if !RUNNERS.contains(&runner) {
            return Err("not a runner".into());
        }
        self.update(|rec| {
            rec["runner"] = json!(runner);
            Self::set_run(rec, run_id, |r| r["runner"] = json!(runner));
        })
    }

    /// One search a host says it ran with its own tools (`specs/113` §2 item 1): where, and the words. Logged as the host declared it,
    /// with no count: nothing here ran it.
    pub fn host_search(&self, run_id: &str, place: &str, words: &str) -> Result<(), String> {
        self.update(|rec| Self::push(rec, "queries", json!({"run": run_id, "query": clip(words, TEXT_CAP), "purpose": "host", "provider": clip(place, 40), "results": null})))
    }

    /// A source the run rests on that Delphi did not read (`specs/113` §2 item 4): its library id and kind. Its locator and title are in
    /// the library's private record, not here. The record is then private: it stays on this machine and is not indexed.
    pub fn private_source(&self, run_id: &str, source_id: &str, kind: &str) -> Result<(), String> {
        self.update(|rec| {
            rec["private"] = json!(true);
            let item = json!({"run": run_id, "source_id": clip(source_id, 80), "kind": clip(kind, 20)});
            match rec["private_sources"].as_array_mut() {
                Some(v) if v.len() < LIST_CAP && !v.contains(&item) => v.push(item),
                Some(_) => {}
                None => rec["private_sources"] = json!([item]),
            }
        })
    }

    /// Change the record by code inside this crate, as one whole write (a host's or an ingest's own fields).
    pub(crate) fn amend(&self, f: impl FnOnce(&mut Value)) -> Result<(), String> {
        self.update(f)
    }

    /// The run left the queue and is working.
    pub fn running(&self, run_id: &str) -> Result<(), String> {
        self.update(|rec| {
            rec["state"] = json!("running");
            Self::set_run(rec, run_id, |r| r["state"] = json!("running"));
        })
    }

    /// The queries the run planned, by kind (`trusted`, `topic`, `gap`), before any is sent. A later run's plan replaces an earlier one's.
    pub fn plan(&self, queries: &[(&str, &str)]) -> Result<(), String> {
        self.update(|rec| rec["plan"] = queries.iter().take(LIST_CAP).map(|(kind, q)| json!({"kind": kind, "query": clip(q, TEXT_CAP)})).collect())
    }

    /// One query the run sent: its text as it was sent, the provider that answered when the transport said, and how many results
    /// came back; or the fixed reason it failed.
    pub fn query(&self, run_id: &str, query: &str, purpose: &str, provider: Option<&str>, results: Result<usize, &str>) -> Result<(), String> {
        self.update(|rec| {
            let mut q = json!({"run": run_id, "query": clip(query, TEXT_CAP), "purpose": clip(purpose, 40), "provider": provider.map(|p| clip(p, 40)), "results": results.ok()});
            if let Err(why) = results {
                q["failed"] = json!(clip(why, 40));
            }
            Self::push(rec, "queries", q);
        })
    }

    /// One page the run read and the library kept: its address, the source it became, and the hash of what was read.
    pub fn page(&self, run_id: &str, url: &str, source_id: &str, content_hash: Option<&str>) -> Result<(), String> {
        self.update(|rec| {
            let url = clip(url, TEXT_CAP);
            if let Some(pages) = rec["pages"].as_array_mut() {
                pages.retain(|p| p["url"].as_str() != Some(url.as_str()));
            }
            Self::push(rec, "pages", json!({"run": run_id, "url": url, "source_id": clip(source_id, 80), "content_hash": content_hash.map(|h| clip(h, 80))}));
        })
    }

    /// What the library kept of a page already in the record (`specs/112` §4 item 1): `kept` is the level (`full` when the
    /// source's text is kept, `excerpts`, `pointer`) and `outcome` what the capture did (`added`, `new_version`, `unchanged`,
    /// `kept_richer`). Words outside those sets are not written.
    pub fn kept(&self, url: &str, level: &str, outcome: &str) -> Result<(), String> {
        if !KEPT_LEVELS.contains(&level) || !KEPT_OUTCOMES.contains(&outcome) {
            return Err("not a capture level and outcome".into());
        }
        self.update(|rec| {
            let url = clip(url, TEXT_CAP);
            if let Some(p) = rec["pages"].as_array_mut().and_then(|pages| pages.iter_mut().find(|p| p["url"].as_str() == Some(url.as_str()))) {
                p["kept"] = json!(level);
                p["outcome"] = json!(outcome);
            }
        })
    }

    /// Start a run that continues this topic (`specs/112` §5.1): one more entry in `runs`, marked `continued`, with the note that
    /// was given. The question, the pages, the claims and the answer of earlier runs stay. `Err` when the topic has no record.
    pub fn continue_run(&self, asked_by: Asker, depth: Depth, run_id: &str, note: Option<&str>) -> Result<(), String> {
        if !valid_run_id(run_id) {
            return Err("not a research id".into());
        }
        self.update(|rec| {
            let at = now();
            rec["depth"] = json!(depth_word(depth));
            rec["state"] = json!("queued");
            let mut run = json!({"run_id": run_id, "asked_by": asked_by.to_json(), "depth": depth_word(depth), "state": "queued", "started": at, "ended": null, "steps": 0, "check_steps": 0, "ms": 0, "continued": true, "unchanged": 0});
            if let Some(n) = note.map(|n| clip(n.lines().next().unwrap_or("").trim(), NOTE_CAP)).filter(|n| !n.is_empty()) {
                run["note"] = json!(n);
            }
            match rec["runs"].as_array_mut() {
                Some(runs) => {
                    runs.retain(|r| r["run_id"].as_str() != Some(run_id));
                    runs.push(run);
                }
                None => rec["runs"] = json!([run]),
            }
        })
    }

    /// What a run that continues this topic starts from: the record's gaps and, for each page it holds, the content hash of what
    /// was read. A page with no hash is not listed, so it is read again.
    ///
    /// When the topic's newest run had a plan of sub-questions (`specs/114` §4), its gaps are the sub-questions it left
    /// uncovered: they are handed over as `open`, for the continued run to search for each, and not as the fixed phrases that
    /// choose among planned queries. They are read from `sub_questions`, which code wrote with each one's coverage, never parsed
    /// out of a phrase.
    pub fn resume(&self, note: Option<&str>) -> Result<crate::research_run::Resume, String> {
        let rec = self.read()?;
        let open: Vec<String> = if matches!(rec["plan_kind"].as_str(), Some("scout" | "continued")) { open_sub_questions(&rec["sub_questions"]) } else { Vec::new() };
        let gaps = if open.is_empty() { rec["gaps"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(String::from)).collect() } else { Vec::new() };
        let held = rec["pages"].as_array().into_iter().flatten().filter_map(|p| Some((p["url"].as_str()?.to_string(), p["content_hash"].as_str()?.to_string()))).collect();
        Ok(crate::research_run::Resume { gaps, held, note: note.map(|n| clip(n.lines().next().unwrap_or("").trim(), NOTE_CAP)).filter(|n| !n.is_empty()), open })
    }

    /// A continued run fetched a page the record holds and found it unchanged: counted on the run, and nothing else changes.
    pub fn unchanged(&self, run_id: &str) -> Result<(), String> {
        self.update(|rec| Self::set_run(rec, run_id, |run| run["unchanged"] = json!(run["unchanged"].as_u64().unwrap_or(0) + 1)))
    }

    /// Who made the newest run's plan and its sub-questions (`specs/114` §4 items 1 and 2): `scout` for the plan call, `fixed` when
    /// that call failed or returned nothing usable and the fixed plan of before was used, `continued` for a run that took its
    /// sub-questions from this record. A sub-question is the scout's words: data, like the question.
    pub fn planned(&self, kind: &str, subs: &Value) -> Result<(), String> {
        if !PLAN_KINDS.contains(&kind) {
            return Err("not a plan kind".into());
        }
        let subs: Vec<Value> = subs.as_array().into_iter().flatten().take(SUBS_CAP).filter_map(sub_question).collect();
        self.update(|rec| {
            rec["plan_kind"] = json!(kind);
            rec["sub_questions"] = json!(subs);
        })
    }

    /// What Delphi already held on the question before the run searched (`specs/114` §3 item 2): claim ids with their status,
    /// topics that overlap, sources held with their age, as the asker's own search found them (`research_method::Known::to_json`).
    /// The newest run's replaces an earlier one's. It is data, like the rest of the record.
    pub fn known(&self, run_id: &str, known: &Value) -> Result<(), String> {
        let list = |k: &str| -> Vec<Value> { known[k].as_array().map(|a| a.iter().take(LIST_CAP).cloned().collect()).unwrap_or_default() };
        self.update(|rec| rec["known"] = json!({"run": run_id, "claims": list("claims"), "topics": list("topics"), "sources": list("sources"), "kb": list("kb")}))
    }

    /// A page already in the record was read from the library and not fetched (`specs/114` §3 item 4).
    pub fn held(&self, url: &str) -> Result<(), String> {
        self.update(|rec| {
            let url = clip(url, TEXT_CAP);
            if let Some(p) = rec["pages"].as_array_mut().and_then(|pages| pages.iter_mut().find(|p| p["url"].as_str() == Some(url.as_str()))) {
                p["held"] = json!(true);
            }
        })
    }

    /// One page the run did not read, and the fixed word for why.
    pub fn refused(&self, run_id: &str, url: &str, why: &str) -> Result<(), String> {
        self.update(|rec| Self::push(rec, "refused", json!({"run": run_id, "url": clip(url, TEXT_CAP), "reason": clip(why, 40)})))
    }

    /// The run ended. `state` is how (`done`, `partial`, `failed`, `cancelled`); `result` is the runner's value when it produced
    /// one. The answer goes to `answer.md` and the search log to `search-log.json`; the rest of the result is kept in the record so
    /// `research_result` can be answered from disk. `claim_ids` are the claims the run produced, as the record lists them.
    pub fn finish(&self, run_id: &str, state: &str, result: Option<&Value>, claim_ids: &[String], error: Option<&str>, ms: u64) -> Result<(), String> {
        if !STATES.contains(&state) || matches!(state, "queued" | "running") {
            return Err("not a finished state".into());
        }
        let question = self.read()?["question"].as_str().unwrap_or("").to_string();
        if let Some(r) = result {
            if let Some(answer) = r["answer"].as_str() {
                // A topic's answer is not lost to a later run of it. A run that drew no claim of its own (every claim it met was
                // already the library's, or it read nothing new) leaves the answer as it was; one that drew some puts its answer
                // first and what the earlier runs wrote below it.
                let earlier = self.answer();
                let drew_none = r["claims"].as_array().is_some_and(Vec::is_empty);
                if earlier.is_empty() || !drew_none {
                    write_atomic(&self.answer_path(), answer_file(&self.id, &question, run_id, &with_earlier(answer, &earlier)).as_bytes())?;
                }
            }
            if r["search_log"].is_object() {
                write_atomic(&self.search_log_path(), &serde_json::to_vec_pretty(&r["search_log"]).map_err(|e| e.to_string())?)?;
            }
        }
        self.update(|rec| {
            let ended = now();
            let (steps, check_steps) = result.map(|r| (r["budget_used"]["steps"].as_u64().unwrap_or(0), r["budget_used"]["check_steps"].as_u64().unwrap_or(0))).unwrap_or((0, 0));
            rec["state"] = json!(state);
            Self::set_run(rec, run_id, |run| {
                run["state"] = json!(state);
                run["ended"] = json!(ended);
                run["steps"] = json!(steps);
                run["check_steps"] = json!(check_steps);
                run["ms"] = json!(ms);
                if let Some(e) = error {
                    run["error"] = json!(clip(e, TEXT_CAP));
                }
                // Why the run stopped sending queries, and after how many rounds (`specs/114` §4 item 5): one of the fixed words.
                if let Some(why) = result.and_then(|r| r["stopped"].as_str()).filter(|w| STOPS.contains(w)) {
                    run["stopped"] = json!(why);
                    run["rounds"] = json!(result.and_then(|r| r["rounds"].as_u64()).unwrap_or(0));
                }
            });
            // The plan's sub-questions as the run left them, each with its coverage: what the gaps are read from.
            if let Some(r) = result.filter(|r| r["sub_questions"].is_array() && r["plan"].as_str().is_some_and(|k| PLAN_KINDS.contains(&k))) {
                rec["plan_kind"] = r["plan"].clone();
                rec["sub_questions"] = json!(r["sub_questions"].as_array().into_iter().flatten().take(SUBS_CAP).filter_map(sub_question).collect::<Vec<_>>());
            }
            let total = |rec: &Value, k: &str| rec["runs"].as_array().map(|runs| runs.iter().filter_map(|r| r[k].as_u64()).sum::<u64>()).unwrap_or(0);
            rec["usage"] = json!({"steps": total(rec, "steps"), "check_steps": total(rec, "check_steps"), "ms": total(rec, "ms"), "tokens": null});
            if let Some(r) = result {
                let mut kept = r.clone();
                if let Some(o) = kept.as_object_mut() {
                    o.remove("answer");
                    o.remove("search_log");
                }
                rec["result"] = kept;
                rec["result_run"] = json!(run_id);
                rec["gaps"] = json!(gaps_of(r, state));
                rec["private"] = json!(rec["private"] == true || r["search_log"]["private"] == true);
                let mut ids: Vec<String> = rec["claims"].as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect()).unwrap_or_default();
                for id in claim_ids {
                    if !ids.contains(id) && ids.len() < LIST_CAP * 4 {
                        ids.push(id.clone());
                    }
                }
                rec["claims"] = json!(ids);
            }
        })
    }
}

/// The texts of the sub-questions in `subs` (a run's `sub_questions`, with coverage) that the run left uncovered.
fn open_sub_questions(subs: &Value) -> Vec<String> {
    subs.as_array().into_iter().flatten().filter(|q| q["covered"] == false).filter_map(|q| q["text"].as_str()).map(|t| clip(t, SUB_TEXT_CAP)).filter(|t| !t.is_empty()).take(SUBS_CAP).collect()
}

/// The gaps a run reports (`specs/114` §4): for a run that had a plan of sub-questions, the sub-questions it left uncovered,
/// which is what a continued run searches for. They are the scout's words: data, like the question.
///
/// For a run on the fixed plan, which has no sub-questions, the gaps are read by code from its coverage report, as before:
/// each rung that was searched and found nothing (with the fixed reason the report gives), and thin coverage. Fixed phrases;
/// no page or model text.
pub fn gaps_of(result: &Value, state: &str) -> Vec<String> {
    if matches!(result["plan"].as_str(), Some("scout" | "continued")) {
        return open_sub_questions(&result["sub_questions"]);
    }
    let mut out = Vec::new();
    for r in result["coverage_report"]["rungs"].as_array().into_iter().flatten() {
        if let (Some(name), Some(reason), true) = (r["rung"].as_str(), r["reason"].as_str(), r["searched"] == true) {
            out.push(format!("{}: {}", clip(name, 20), clip(reason, 40)));
        }
    }
    if result["coverage"] == "thin" {
        out.push(format!("coverage is thin: {} kept, cited claim(s) from the knowledge base and the trusted sources, under {}", result["coverage_report"]["counted"].as_u64().unwrap_or(0), result["coverage_report"]["thin_below"].as_u64().unwrap_or(0)));
    }
    if state == "partial" {
        out.push("a step budget ran out before the run finished".into());
    }
    out
}

/// The line that separates a run's answer from what earlier runs of the topic wrote.
const EARLIER: &str = "From earlier runs of this topic:";
/// What `answer.md` holds at most, in characters, as the runner caps one answer.
const ANSWER_CAP: usize = 7900;

/// A run's answer with the topic's earlier answer below it, under one heading however many runs came before. The same text twice
/// is written once.
fn with_earlier(answer: &str, earlier: &str) -> String {
    let earlier: String = earlier.lines().filter(|l| l.trim() != EARLIER).collect::<Vec<_>>().join("\n");
    let earlier = earlier.trim();
    if earlier.is_empty() || earlier == answer.trim() {
        return answer.to_string();
    }
    format!("{}\n\n{}\n{}", answer.trim_end(), EARLIER, earlier).chars().take(ANSWER_CAP).collect()
}

/// `answer.md`: a few lines of frontmatter that say what it is (not validated, from outside), the question as a heading, the answer.
fn answer_file(topic_id: &str, question: &str, run_id: &str, answer: &str) -> String {
    let title = serde_json::to_string(&clip(question, TEXT_CAP)).unwrap_or_else(|_| "\"\"".into());
    format!("---\ntitle: {title}\ntopic: {topic_id}\nrun: {run_id}\nauthor: scout\nsource_type: {TAINT}\nconfidence: low\nupdated: {}\n---\n\n# {}\n\n{}\n", now(), clip(question, TEXT_CAP), answer.trim_end())
}

/// The answer inside an `answer.md` this module wrote: everything below the frontmatter and the heading.
fn answer_body(file: &str) -> String {
    let rest = file.strip_prefix("---\n").and_then(|r| r.split_once("\n---\n")).map(|(_, body)| body).unwrap_or(file);
    let rest = rest.trim_start_matches('\n');
    let rest = match rest.split_once('\n') {
        Some((first, more)) if first.starts_with("# ") => more,
        _ => rest,
    };
    rest.trim_start_matches('\n').trim_end().to_string()
}

fn run_of<'a>(rec: &'a Value, run_id: &str) -> Option<&'a Value> {
    rec["runs"].as_array()?.iter().find(|r| r["run_id"].as_str() == Some(run_id))
}

fn finished(state: &str) -> bool {
    matches!(state, "done" | "partial" | "failed" | "cancelled")
}

/// What `research_result` returns for `run_id`, from the topic's files (§2.5): the runner's result with its answer and search
/// log put back and the run's `status`. `None` while the run has not finished (or is not one of this topic's). A run whose result
/// a later run of the same topic replaced says so instead of returning the newer result under the older id.
pub fn result_of(topic: &Topic, run_id: &str) -> Option<Value> {
    let rec = topic.read().ok()?;
    let run = run_of(&rec, run_id)?;
    let state = run["state"].as_str().filter(|s| finished(s))?;
    let mut v = if rec["result_run"].as_str() == Some(run_id) && rec["result"].is_object() {
        let mut v = rec["result"].clone();
        v["answer"] = json!(topic.answer());
        if let Some(log) = topic.search_log() {
            v["search_log"] = log;
        }
        v
    } else if rec["result_run"].is_string() && rec["result_run"].as_str() != Some(run_id) && matches!(state, "done" | "partial") {
        json!({"superseded_by": rec["result_run"], "note": "a later run of the same topic replaced this result; ask for that run, or read the topic with `delphi research show`"})
    } else {
        json!({})
    };
    v["status"] = json!(state);
    v["topic_id"] = json!(topic.id);
    if let Some(e) = run["error"].as_str() {
        v["error"] = json!(e);
    }
    Some(v)
}

/// What `research_status` returns for `run_id`, from the record: the state and how far the run got. `live` says whether the
/// daemon still holds the job; a run the record calls `queued` or `running` that the daemon no longer holds was stopped (the
/// daemon restarted), and is reported `failed` with what the record has, without the record being changed.
pub fn status_of(topic: &Topic, run_id: &str, live: bool) -> Option<Value> {
    let rec = topic.read().ok()?;
    let run = run_of(&rec, run_id)?;
    let recorded = run["state"].as_str().unwrap_or("failed");
    let stopped = !finished(recorded) && !live;
    let of_run = |list: &str, key: &str| -> Vec<Value> { rec[list].as_array().into_iter().flatten().filter(|x| x["run"].as_str() == Some(run_id)).map(|x| x[key].clone()).collect() };
    let mut v = json!({
        "research_id": run_id, "topic_id": topic.id, "status": if stopped { "failed" } else { recorded }, "depth": run["depth"], "queued_ahead": 0,
        "steps": run["steps"], "check_steps": run["check_steps"],
        "queries": of_run("queries", "query"), "fetched": of_run("pages", "url"), "refused": of_run("refused", "url"),
        "error": run["error"],
    });
    if stopped {
        v["error"] = json!("the run was stopped before it finished (the daemon restarted); the record holds what it had found");
    }
    Some(v)
}

fn runs_dir(ws: &Path) -> PathBuf {
    ws.join("research").join("runs")
}

/// Remember which topic a run belongs to, for the session that asked: `<workspace>/research/runs/<session>.<run-id>.json` holds
/// the topic id and the project's name (never a path), so the record is found again after the daemon restarts.
pub fn point_run(ws: &Path, session: &str, run_id: &str, topic: &Topic, project: Option<&str>) -> Result<(), String> {
    if !valid_session(session) || !valid_run_id(run_id) {
        return Err("not a session and a research id".into());
    }
    write_atomic(&runs_dir(ws).join(format!("{session}.{run_id}.json")), json!({"topic_id": topic.id, "project": project}).to_string().as_bytes())
}

fn topic_of_pointer(ws: &Path, path: &Path) -> Option<Topic> {
    let p: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let root = match p["project"].as_str() {
        Some(name) => crate::scope::project_list(ws).into_iter().find(|(n, _)| n == name)?.1,
        None => ws.to_path_buf(),
    };
    Topic::at(&root, p["topic_id"].as_str()?).filter(Topic::exists)
}

/// The topic of the run `run_id` that `session` asked for; `None` for any other session's run, and for a run with no record.
pub fn find_run(ws: &Path, session: &str, run_id: &str) -> Option<Topic> {
    if !valid_session(session) || !valid_run_id(run_id) {
        return None;
    }
    topic_of_pointer(ws, &runs_dir(ws).join(format!("{session}.{run_id}.json")))
}

/// The topic of a run by its research id alone, for the operator's own commands (`delphi research log <research-id>`): the newest
/// run of that id, whichever session asked. Never used to answer a host.
pub fn find_run_for_operator(ws: &Path, run_id: &str) -> Option<Topic> {
    if !valid_run_id(run_id) {
        return None;
    }
    let suffix = format!(".{run_id}.json");
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(runs_dir(ws)).ok()?.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(&suffix)).filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    found.sort();
    found.into_iter().rev().find_map(|(_, p)| topic_of_pointer(ws, &p))
}

/// Every topic under the workspace and under each registered project, newest first: `(topic, record)`.
pub fn list(ws: &Path) -> Vec<(Topic, Value)> {
    let mut roots = vec![ws.to_path_buf()];
    roots.extend(crate::scope::project_list(ws).into_iter().map(|(_, root)| root));
    let mut out: Vec<(Topic, Value)> = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(topics_dir(&root)) else { continue };
        for e in rd.flatten() {
            let Some(topic) = Topic::at(&root, &e.file_name().to_string_lossy()) else { continue };
            if out.iter().any(|(t, _)| t.dir == topic.dir) {
                continue;
            }
            if let Ok(rec) = topic.read() {
                out.push((topic, rec));
            }
        }
    }
    out.sort_by(|a, b| b.1["updated"].as_str().cmp(&a.1["updated"].as_str()).then_with(|| a.0.id.cmp(&b.0.id)));
    out
}

/// The topic `id` wherever it lives: the workspace first, then each registered project.
pub fn find(ws: &Path, id: &str) -> Option<Topic> {
    let mut roots = vec![ws.to_path_buf()];
    roots.extend(crate::scope::project_list(ws).into_iter().map(|(_, root)| root));
    roots.iter().filter_map(|r| Topic::at(r, id)).find(Topic::exists)
}

fn count(rec: &Value, list: &str) -> usize {
    rec[list].as_array().map_or(0, Vec::len)
}

/// One line per topic, for `delphi research topics`.
pub fn render_list(topics: &[(Topic, Value)]) -> String {
    if topics.is_empty() {
        return "no research topics yet: a research run leaves one\n".into();
    }
    let mut out = String::new();
    for (t, rec) in topics {
        let project = rec["project"].as_str().map(|p| format!("  [{}]", clip(p, 60))).unwrap_or_default();
        out.push_str(&format!(
            "{}  {}  {} run(s), {} page(s), {} claim(s){}{}\n    {}\n",
            t.id,
            rec["state"].as_str().unwrap_or("?"),
            count(rec, "runs"),
            count(rec, "pages"),
            count(rec, "claims"),
            project,
            if rec["private"] == true { "  (private)" } else { "" },
            clip(rec["question"].as_str().unwrap_or(""), 160)
        ));
    }
    out
}

/// One topic as text, for `delphi research show <id>`. Everything printed is the record's own data, labelled not validated.
pub fn render(topic: &Topic, rec: &Value) -> String {
    let s = |k: &str| clip(rec[k].as_str().unwrap_or(""), TEXT_CAP);
    let mut out = format!("research topic {}{}\n", topic.id, if rec["private"] == true { "  (private: it stays on this machine)" } else { "" });
    out.push_str(&format!("  question   {}\n", s("question")));
    out.push_str(&format!("  asked by   {} ({})\n", clip(rec["asked_by"]["label"].as_str().unwrap_or("?"), 120), clip(rec["asked_by"]["kind"].as_str().unwrap_or("?"), 20)));
    if let Some(r) = rec["runner"].as_str() {
        out.push_str(&format!("  runner     {}\n", clip(r, 20)));
    }
    if let Some(p) = rec["project"].as_str() {
        out.push_str(&format!("  project    {}\n", clip(p, 80)));
    }
    out.push_str(&format!("  state      {}  (depth {})\n", s("state"), s("depth")));
    out.push_str(&format!("  use        {} step(s), {} check step(s), {} ms\n", rec["usage"]["steps"].as_u64().unwrap_or(0), rec["usage"]["check_steps"].as_u64().unwrap_or(0), rec["usage"]["ms"].as_u64().unwrap_or(0)));
    out.push_str(&format!("  runs       {}\n", count(rec, "runs")));
    for r in rec["runs"].as_array().into_iter().flatten() {
        out.push_str(&format!("    {}  {}  {}  {}\n", clip(r["run_id"].as_str().unwrap_or("?"), 64), clip(r["state"].as_str().unwrap_or("?"), 20), clip(r["started"].as_str().unwrap_or(""), 30), clip(r["asked_by"]["label"].as_str().unwrap_or(""), 120)));
    }
    out.push_str(&format!("  queries    {}\n", count(rec, "queries")));
    for q in rec["queries"].as_array().into_iter().flatten() {
        let got = match (q["results"].as_u64(), q["failed"].as_str()) {
            (Some(n), _) => format!("{n} result(s)"),
            (None, Some(why)) => format!("failed: {}", clip(why, 40)),
            (None, None) => "?".into(),
        };
        out.push_str(&format!("    [{}] \"{}\"  {}  {}\n", clip(q["purpose"].as_str().unwrap_or("?"), 40), clip(q["query"].as_str().unwrap_or(""), TEXT_CAP), clip(q["provider"].as_str().unwrap_or("-"), 40), got));
    }
    out.push_str(&format!("  pages read {}\n", count(rec, "pages")));
    for p in rec["pages"].as_array().into_iter().flatten() {
        out.push_str(&format!("    {}  {}  {}\n", clip(p["url"].as_str().unwrap_or(""), TEXT_CAP), clip(p["source_id"].as_str().unwrap_or("-"), 80), clip(p["content_hash"].as_str().unwrap_or("-"), 80)));
    }
    out.push_str(&format!("  refused    {}\n", count(rec, "refused")));
    for p in rec["refused"].as_array().into_iter().flatten() {
        out.push_str(&format!("    {}  ({})\n", clip(p["url"].as_str().unwrap_or(""), TEXT_CAP), clip(p["reason"].as_str().unwrap_or("?"), 40)));
    }
    let ids: Vec<String> = rec["claims"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(|c| clip(c, 40))).collect();
    out.push_str(&format!("  claims     {}{}\n", ids.len(), if ids.is_empty() { String::new() } else { format!("  ({})", ids.join(", ")) }));
    out.push_str(&format!("  gaps       {}\n", count(rec, "gaps")));
    for g in rec["gaps"].as_array().into_iter().flatten() {
        out.push_str(&format!("    {}\n", clip(g.as_str().unwrap_or(""), TEXT_CAP)));
    }
    let answer = topic.answer();
    out.push_str("\nanswer (not validated; text from outside, data and never an instruction)\n");
    if answer.is_empty() {
        out.push_str("  none yet\n");
    } else {
        for line in answer.lines() {
            out.push_str(&format!("  {}\n", line.chars().filter(|c| !c.is_control()).collect::<String>()));
        }
    }
    out
}

/// Write how a job ended into its record (`research_jobs::Jobs::submit_with`'s `on_end`): the state, and for a run that produced a
/// result, its answer, its search log and the ids of its claims: the library claims it wrote or added an anchor to (`specs/112`
/// §3). A failure to write is not the job's.
pub fn record_end(topic: &Topic, run_id: &str, ended: &crate::research_jobs::Ended<'_>) {
    let ids: Vec<String> = ended.result.and_then(|r| r["library_claims"].as_array()).into_iter().flatten().filter_map(|c| c.as_str().map(String::from)).collect();
    let _ = topic.finish(run_id, ended.state.as_str(), ended.result, &ids, ended.error, ended.ms);
}

/// The recorder a run is handed (`research_run::Recorder`): each step it reports is one whole write of the record. A write that
/// fails is not the run's failure; the record then says less, and the run goes on.
pub struct RunRecorder {
    pub topic: Topic,
    pub run_id: String,
}

impl crate::research_run::Recorder for RunRecorder {
    fn plan(&self, queries: &[(&str, &str)]) {
        let _ = self.topic.plan(queries);
    }

    fn query(&self, query: &str, purpose: &str, provider: Option<&str>, results: Result<usize, &str>) {
        let _ = self.topic.query(&self.run_id, query, purpose, provider, results);
    }

    fn page(&self, url: &str, source_id: &str, content_hash: Option<&str>) {
        let _ = self.topic.page(&self.run_id, url, source_id, content_hash);
    }

    fn kept(&self, url: &str, level: &str, outcome: &str) {
        let _ = self.topic.kept(url, level, outcome);
    }

    fn refused(&self, url: &str, why: &str) {
        let _ = self.topic.refused(&self.run_id, url, why);
    }

    fn unchanged(&self, _url: &str) {
        let _ = self.topic.unchanged(&self.run_id);
    }

    fn known(&self, known: &Value) {
        let _ = self.topic.known(&self.run_id, known);
    }

    fn held(&self, url: &str) {
        let _ = self.topic.held(url);
    }

    fn planned(&self, kind: &str, subs: &Value) {
        let _ = self.topic.planned(kind, subs);
    }
}

// ── due topics (`specs/112` §5.2) ─────────────────────────────────────────────────────────────────────────────────────────

/// `[research] refresh_days` and its bounds: how old a topic's newest run may be before the topic is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refresh {
    /// A topic whose record names no `refresh_days` of its own.
    pub days: u64,
    /// What a record's own `refresh_days` is clamped to.
    pub min: u64,
    pub max: u64,
}

impl Default for Refresh {
    fn default() -> Refresh {
        Refresh { days: 90, min: 7, max: 730 }
    }
}

impl Refresh {
    /// The days a record is refreshed after: its own `refresh_days` inside the bounds, else the default.
    pub fn days_of(&self, rec: &Value) -> u64 {
        rec["refresh_days"].as_u64().map_or(self.days, |d| d.clamp(self.min, self.max.max(self.min)))
    }
}

/// Why a topic is due, in fixed words; `None` when it is not. A topic is due when its newest finished run is older than its
/// refresh days, or when a source it rests on has changed or been retracted since the record read it. `source` gives a
/// source's own record in the library (`Library::read_meta`). A topic with a run going, or with no finished run, is not due.
/// This only reads: nothing is run again without the operator or an agent asking.
pub fn due(rec: &Value, now_unix: i64, refresh: &Refresh, source: &dyn Fn(&str) -> Option<Value>) -> Option<String> {
    if matches!(rec["state"].as_str(), Some("queued" | "running")) {
        return None;
    }
    let newest = rec["runs"].as_array()?.iter().filter(|r| matches!(r["state"].as_str(), Some("done" | "partial"))).filter_map(|r| delphi_core::time::parse_rfc3339(r["ended"].as_str()?)).max()?;
    for p in rec["pages"].as_array().into_iter().flatten() {
        let Some(meta) = p["source_id"].as_str().and_then(source) else { continue };
        if meta["retraction"]["kind"].as_str() == Some("retracted") {
            return Some("a source it rests on was retracted".into());
        }
        if let (Some(read), Some(held)) = (p["content_hash"].as_str(), meta["content_hash"].as_str()) {
            if read != held {
                return Some("a source it rests on has changed".into());
            }
        }
    }
    let (days, age) = (refresh.days_of(rec), (now_unix - newest).max(0) / 86_400);
    (age as u64 > days).then(|| format!("its newest run is {age} day(s) old, over its {days}"))
}

/// The topics that are due, with why, in the order of [`list`].
pub fn due_topics(ws: &Path, home: &Path, refresh: &Refresh) -> Vec<(Topic, Value, String)> {
    let lib = crate::library::Library::in_home(home);
    let now_unix = delphi_core::time::parse_rfc3339(&now()).unwrap_or(0);
    list(ws).into_iter().filter_map(|(t, rec)| due(&rec, now_unix, refresh, &|id| lib.read_meta(id)).map(|why| (t, rec, why))).collect()
}

/// One line per due topic, for `delphi research topics --due` and the digest.
pub fn render_due(due: &[(Topic, Value, String)]) -> String {
    if due.is_empty() {
        return "no research topic is due\n".into();
    }
    let mut out = String::new();
    for (t, rec, why) in due {
        out.push_str(&format!("{}  due: {}\n    {}\n", t.id, why, clip(rec["question"].as_str().unwrap_or(""), 160)));
    }
    out.push_str("nothing is run again by itself: `delphi research continue <topic-id>` starts a run (it spends tokens and reaches the network)\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn begin<'a>(question: &'a str, run_id: &'a str) -> Begin<'a> {
        Begin { question, asked_by: Asker::host("host:claude-code/some-model"), project: None, depth: Depth::Survey, run_id }
    }

    fn result(answer: &str) -> Value {
        json!({
            "coverage": "thin", "coverage_report": {"thin_below": 3, "counted": 1, "rungs": [{"rung": "kb", "searched": true, "entries_found": 0, "kept": 0, "reason": "kb_empty"}, {"rung": "close", "searched": false, "entries_found": 0, "kept": 0, "reason": "not_connected"}], "host_rungs": ["close", "web"], "leads_kept": 0},
            "answer": answer, "claims": [{"id": "abc123abc123", "assertion": "aspirin reduces fever", "knowledge_status": "noted"}], "source_ids": ["s-1"], "kb_hits": [], "new_claims": 1,
            "budget_used": {"steps": 4, "check_steps": 1}, "answered_by": "scout",
            "search_log": {"version": 1, "private": false, "lines": [], "flow": {}},
        })
    }

    #[test]
    fn a_topic_id_is_a_slug_and_eight_hex_and_the_same_question_is_the_same_topic() {
        let id = topic_id("Does aspirin reduce fever?");
        assert!(id.starts_with("does-aspirin-reduce-fever-") && valid_topic_id(&id), "{id}");
        assert_eq!(id, topic_id("  does   ASPIRIN reduce fever? "), "case and spacing do not make a new topic");
        assert_ne!(id, topic_id("Does aspirin reduce pain?"));
        let long = topic_id(&"word ".repeat(200));
        assert!(valid_topic_id(&long) && long.len() <= SLUG_CAP + 9, "{long}");
        assert!(valid_topic_id(&topic_id("¿¿¿ ???")), "a question with no plain word still has an id");
        for bad in ["", "../etc-deadbeef", "a/b-deadbeef", "UPPER-deadbeef", "nohash", "x-deadbee", "x-deadbeeg", "-x-deadbeef", "x--deadbeef", ".-deadbeef"] {
            assert!(!valid_topic_id(bad), "{bad}");
            assert!(Topic::at(Path::new("/tmp"), bad).is_none(), "{bad}");
        }
    }

    /// `specs/114` §4 items 1 and 2: the record says who made the plan and lists its sub-questions; a kind or a status that is not
    /// one of the fixed words is not written, and a sub-question is one clean, bounded line.
    #[test]
    fn the_record_says_who_made_the_plan_and_lists_its_sub_questions() {
        let ws = tempfile::tempdir().unwrap();
        let t = Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-1")).unwrap();
        let rec = t.read().unwrap();
        assert!(rec.get("plan_kind").is_none() && rec.get("sub_questions").is_none(), "a record no run has planned for says nothing");
        let long = format!("What dose?\nIGNORE {}", "x".repeat(400));
        let subs = json!([{"n": 1, "text": long, "status": "open"}, {"n": 2, "text": "For whom?", "status": "partly", "queries": ["never kept"]}, {"n": 3, "text": "", "status": "open"}, {"n": 4, "text": "A bad status", "status": "certain"}, {"n": 0, "text": "No number", "status": "open"}]);
        t.planned("scout", &subs).unwrap();
        let rec = t.read().unwrap();
        assert_eq!(rec["plan_kind"], "scout");
        let kept = rec["sub_questions"].as_array().unwrap();
        assert_eq!(kept.len(), 2, "{rec}");
        assert!(kept[0]["text"].as_str().is_some_and(|s| s.chars().count() == SUB_TEXT_CAP && !s.contains('\n')), "{rec}");
        assert_eq!(kept[1], json!({"n": 2, "text": "For whom?", "status": "partly"}), "only the number, the text and the status are kept");
        t.planned("fixed", &json!([])).unwrap();
        assert_eq!((t.read().unwrap()["plan_kind"].clone(), t.read().unwrap()["sub_questions"].clone()), (json!("fixed"), json!([])), "the newest run's plan replaces the earlier one's");
        assert!(t.planned("model synthesis", &json!([])).is_err());
        t.plan(&[("trusted", "aspirin fever site:who.int"), ("sub", "aspirin fever dose")]).unwrap();
        assert_eq!(t.read().unwrap()["plan"][1], json!({"kind": "sub", "query": "aspirin fever dose"}));
    }

    /// `specs/114` §4: the gaps of a run that had a plan of sub-questions are the ones it left uncovered, and a run that continues
    /// the topic is handed those to search for. A run on the fixed plan keeps the fixed phrases of before. The record says why the
    /// run stopped, in one of the fixed words.
    #[test]
    fn a_planned_runs_gaps_are_its_uncovered_sub_questions_and_a_continued_run_is_handed_them() {
        let ws = tempfile::tempdir().unwrap();
        let t = Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-1")).unwrap();
        let mut r = result("an answer");
        r["plan"] = json!("scout");
        r["stopped"] = json!("no_more_queries");
        r["rounds"] = json!(2);
        r["sub_questions"] = json!([
            {"n": 1, "text": "What dose lowers a fever?", "status": "open", "covered": true, "claims": 2, "origins": 2},
            {"n": 2, "text": "Is it safe for children?", "status": "partly", "covered": false, "claims": 1, "origins": 1},
            {"n": 3, "text": "What are the risks?", "status": "open", "covered": false, "claims": 0, "origins": 0},
        ]);
        t.finish("r-t-1", "done", Some(&r), &[], None, 5).unwrap();
        let rec = t.read().unwrap();
        assert_eq!(rec["gaps"], json!(["Is it safe for children?", "What are the risks?"]), "the uncovered sub-questions, and no fixed phrase beside them: {rec}");
        assert_eq!((rec["plan_kind"].as_str(), rec["sub_questions"][1].clone()), (Some("scout"), json!({"n": 2, "text": "Is it safe for children?", "status": "partly", "covered": false, "claims": 1, "origins": 1})));
        assert_eq!((rec["runs"][0]["stopped"].as_str(), rec["runs"][0]["rounds"].as_u64()), (Some("no_more_queries"), Some(2)), "{rec}");
        let resume = t.resume(Some("in adults")).unwrap();
        assert_eq!(resume.open, ["Is it safe for children?", "What are the risks?"]);
        assert!(resume.gaps.is_empty(), "sub-questions are searched for; they do not choose among the fixed queries: {:?}", resume.gaps);
        assert_eq!(resume.note.as_deref(), Some("in adults"));

        // A continued run that covers the rest leaves no gap, and the next run of the topic is a refresh as before.
        t.continue_run(Asker::operator(), Depth::Survey, "r-t-2", None).unwrap();
        r["plan"] = json!("continued");
        r["stopped"] = json!("covered");
        r["sub_questions"] = json!([{"n": 1, "text": "Is it safe for children?", "status": "open", "covered": true, "claims": 2, "origins": 2}, {"n": 2, "text": "What are the risks?", "status": "open", "covered": true, "claims": 2, "origins": 3}]);
        t.finish("r-t-2", "done", Some(&r), &[], None, 5).unwrap();
        let rec = t.read().unwrap();
        assert_eq!((rec["gaps"].clone(), rec["plan_kind"].as_str(), rec["runs"][1]["stopped"].as_str()), (json!([]), Some("continued"), Some("covered")), "{rec}");
        let resume = t.resume(None).unwrap();
        assert!(resume.open.is_empty() && resume.gaps.is_empty());

        // The fixed plan: the gaps are the fixed phrases read from the coverage report, and they are handed over as such.
        let fixed = Topic::begin(ws.path(), &begin("Does ibuprofen reduce fever?", "r-f-1")).unwrap();
        let mut r = result("another answer");
        r["plan"] = json!("fixed");
        r["stopped"] = json!("because the page said so");
        r["sub_questions"] = json!([]);
        fixed.finish("r-f-1", "done", Some(&r), &[], None, 5).unwrap();
        let rec = fixed.read().unwrap();
        assert!(rec["gaps"].as_array().unwrap().iter().any(|g| g == "kb: kb_empty") && rec["gaps"].as_array().unwrap().iter().any(|g| g.as_str().unwrap().starts_with("coverage is thin")), "{rec}");
        assert!(rec["runs"][0].get("stopped").is_none(), "a word that is not one of the fixed ones is not written: {rec}");
        let resume = fixed.resume(None).unwrap();
        assert!(resume.open.is_empty() && resume.gaps.len() == 2, "{resume:?}");
    }

    /// §2.4: each step is on disk when it returns, the record is whole at every moment, and no temporary file is left behind.
    #[test]
    fn the_record_is_written_as_the_run_goes_and_a_stopped_run_says_how_far_it_got() {
        let ws = tempfile::tempdir().unwrap();
        let t = Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-1")).unwrap();
        assert_eq!(t.dir, ws.path().join("research/topics").join(&t.id));
        let rec = t.read().unwrap();
        assert_eq!((rec["state"].as_str(), rec["question"].as_str(), rec["depth"].as_str()), (Some("queued"), Some("Does aspirin reduce fever?"), Some("survey")));
        assert_eq!(rec["asked_by"], json!({"kind": "host", "label": "host:claude-code/some-model"}));
        assert_eq!((rec["taint"].as_str(), rec["project"].is_null()), (Some(TAINT), true));
        t.running("r-t-1").unwrap();
        t.plan(&[("trusted", "aspirin fever site:who.int"), ("topic", "aspirin fever")]).unwrap();
        t.query("r-t-1", "aspirin fever site:who.int", "trusted", Some("brave"), Ok(3)).unwrap();
        t.query("r-t-1", "aspirin fever", "topic", None, Err("no_results")).unwrap();
        t.page("r-t-1", "https://who.int/a", "s-abc", Some("sha256:00")).unwrap();
        t.refused("r-t-1", "https://blog.example/x", "not_fetchable").unwrap();
        // The run is stopped here: nothing calls `finish`. What it wrote is all there, and the state says it was running.
        let rec = Topic::at(ws.path(), &t.id).unwrap().read().unwrap();
        assert_eq!(rec["state"], "running");
        assert_eq!(rec["plan"].as_array().unwrap().len(), 2);
        assert_eq!(rec["queries"][0], json!({"run": "r-t-1", "query": "aspirin fever site:who.int", "purpose": "trusted", "provider": "brave", "results": 3}));
        assert_eq!((rec["queries"][1]["failed"].as_str(), rec["queries"][1]["results"].is_null()), (Some("no_results"), true));
        assert_eq!(rec["pages"][0], json!({"run": "r-t-1", "url": "https://who.int/a", "source_id": "s-abc", "content_hash": "sha256:00"}));
        assert_eq!(rec["refused"][0]["reason"], "not_fetchable");
        assert_eq!(rec["runs"][0]["state"], "running");
        let left: Vec<String> = std::fs::read_dir(&t.dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(left, ["record.json"], "no temporary file is left, and no answer before the run ends");
        // The status read from disk: live while the daemon holds the job; stopped, and reported failed, once it does not.
        assert_eq!(status_of(&t, "r-t-1", true).unwrap()["status"], "running");
        let stopped = status_of(&t, "r-t-1", false).unwrap();
        assert_eq!(stopped["status"], "failed");
        assert!(stopped["error"].as_str().unwrap().contains("stopped before it finished"));
        assert_eq!((stopped["queries"].as_array().unwrap().len(), stopped["fetched"][0].as_str()), (2, Some("https://who.int/a")));
        assert_eq!(t.read().unwrap()["state"], "running", "reading the status does not rewrite the record");
        assert!(result_of(&t, "r-t-1").is_none(), "no result for a run that did not finish");
        // Resuming: the same question asked again is the same topic; the new run is a second entry and nothing earlier is lost.
        let again = Topic::begin(ws.path(), &begin("does aspirin reduce FEVER?", "r-t-2")).unwrap();
        assert_eq!(again.id, t.id);
        let rec = again.read().unwrap();
        assert_eq!(rec["runs"].as_array().unwrap().len(), 2);
        assert_eq!((rec["runs"][0]["state"].as_str(), rec["runs"][1]["state"].as_str(), rec["state"].as_str()), (Some("running"), Some("queued"), Some("queued")));
        assert_eq!((rec["queries"].as_array().unwrap().len(), rec["pages"].as_array().unwrap().len()), (2, 1));
        assert_eq!(rec["question"], "Does aspirin reduce fever?", "the question as first asked");
    }

    #[test]
    fn a_finished_run_leaves_its_answer_and_log_as_files_and_the_result_reads_back_whole() {
        let ws = tempfile::tempdir().unwrap();
        let t = Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-1")).unwrap();
        t.running("r-t-1").unwrap();
        let answer = "What the sources read say, claim by claim (not validated):\n- aspirin reduces fever (\"lowers fever in adults\")\n\n# not a heading of ours\n\nlist of claims (no synthesis)";
        let mut r = result(answer);
        t.finish("r-t-1", "partial", Some(&r), &["abc123abc123".to_string()], None, 1234).unwrap();
        let file = std::fs::read_to_string(t.answer_path()).unwrap();
        assert!(file.starts_with("---\ntitle: \"Does aspirin reduce fever?\"\n") && file.contains("\nsource_type: external_untrusted\n") && file.contains("\n# Does aspirin reduce fever?\n"), "{file}");
        assert_eq!(t.answer(), answer, "the answer reads back exactly");
        assert_eq!(t.search_log().unwrap()["version"], 1);
        let rec = t.read().unwrap();
        assert_eq!((rec["state"].as_str(), rec["result_run"].as_str()), (Some("partial"), Some("r-t-1")));
        assert!(rec["result"].get("answer").is_none() && rec["result"].get("search_log").is_none(), "the answer and the log are files, not copies in the record");
        assert_eq!(rec["claims"], json!(["abc123abc123"]));
        assert_eq!(rec["gaps"], json!(["kb: kb_empty", "coverage is thin: 1 kept, cited claim(s) from the knowledge base and the trusted sources, under 3", "a step budget ran out before the run finished"]), "a rung that was not searched is not a gap");
        assert_eq!(rec["usage"], json!({"steps": 4, "check_steps": 1, "ms": 1234, "tokens": null}));
        assert_eq!((rec["runs"][0]["state"].as_str(), rec["runs"][0]["ms"].as_u64(), rec["runs"][0]["ended"].is_string()), (Some("partial"), Some(1234), true));
        // The result from disk is the runner's value, with the run's status.
        r["status"] = json!("partial");
        r["topic_id"] = json!(t.id);
        assert_eq!(result_of(&t, "r-t-1").unwrap(), r);
        assert_eq!(status_of(&t, "r-t-1", false).unwrap()["status"], "partial");
        assert!(result_of(&t, "r-other").is_none() && status_of(&t, "r-other", false).is_none());
        assert!(t.finish("r-t-1", "running", None, &[], None, 0).is_err() && t.finish("r-t-1", "bogus", None, &[], None, 0).is_err());
        // A second run of the topic that drew claims puts its answer first and keeps what the first wrote below it; the first run's
        // id then says it was superseded instead of returning the newer result.
        let first = t.answer();
        Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-2")).unwrap();
        t.finish("r-t-2", "done", Some(&result("a newer answer")), &["abc123abc123".to_string(), "def456def456".to_string()], None, 10).unwrap();
        let second = result_of(&t, "r-t-2").unwrap()["answer"].as_str().unwrap().to_string();
        assert_eq!(second, format!("a newer answer\n\nFrom earlier runs of this topic:\n{first}"));
        // A run that drew no claim of its own leaves the topic's answer as it was, and one more with claims does not nest headings.
        Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-2b")).unwrap();
        let mut none = result("No claim could be drawn from the sources read. (not validated)");
        none["claims"] = json!([]);
        t.finish("r-t-2b", "done", Some(&none), &[], None, 0).unwrap();
        assert_eq!(result_of(&t, "r-t-2b").unwrap()["answer"], second.as_str(), "the answer is not lost to a run that found nothing new");
        Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-2c")).unwrap();
        t.finish("r-t-2c", "done", Some(&result("a third answer")), &[], None, 0).unwrap();
        assert_eq!(t.answer().matches("From earlier runs of this topic:").count(), 1, "{}", t.answer());
        assert!(t.answer().starts_with("a third answer\n\nFrom earlier runs of this topic:\na newer answer\n"), "{}", t.answer());
        let last = t.answer();
        let old = result_of(&t, "r-t-1").unwrap();
        assert_eq!((old["status"].as_str(), old["superseded_by"].as_str(), old.get("answer").is_none()), (Some("partial"), Some("r-t-2c"), true));
        let rec = t.read().unwrap();
        assert_eq!(rec["claims"], json!(["abc123abc123", "def456def456"]), "claim ids gather across runs, once each");
        assert_eq!(rec["usage"]["ms"], 1244);
        // A run that failed has no result; its error is kept.
        Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-3")).unwrap();
        t.finish("r-t-3", "failed", None, &[], Some("the job ran out of time"), 5).unwrap();
        let failed = result_of(&t, "r-t-3").unwrap();
        assert_eq!((failed["status"].as_str(), failed["error"].as_str()), (Some("failed"), Some("the job ran out of time")));
        assert_eq!(t.answer(), last, "a failed run leaves the last answer in place");
    }

    #[test]
    fn a_run_is_found_again_only_by_the_session_that_asked() {
        let ws = tempfile::tempdir().unwrap();
        let t = Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-1")).unwrap();
        point_run(ws.path(), "abcdef0123456789", "r-t-1", &t, None).unwrap();
        assert_eq!(find_run(ws.path(), "abcdef0123456789", "r-t-1").unwrap().id, t.id);
        assert!(find_run(ws.path(), "0123456789abcdef", "r-t-1").is_none(), "another session's run is not found");
        assert!(find_run(ws.path(), "abcdef0123456789", "r-t-2").is_none());
        for (s, r) in [("../../etc", "r-t-1"), ("abcdef0123456789", "../x"), ("SHOUT000", "r-t-1"), ("short", "r-t-1")] {
            assert!(find_run(ws.path(), s, r).is_none() && point_run(ws.path(), s, r, &t, None).is_err(), "{s} {r}");
        }
        assert_eq!(find_run_for_operator(ws.path(), "r-t-1").unwrap().id, t.id, "the operator's own command finds it by the research id");
        assert!(find_run_for_operator(ws.path(), "r-t-9").is_none());
        // A pointer names a topic id and a project's name, never a path: one that names something else finds nothing.
        std::fs::write(ws.path().join("research/runs/abcdef0123456789.r-t-5.json"), r#"{"topic_id": "../../outside-deadbeef", "project": null}"#).unwrap();
        assert!(find_run(ws.path(), "abcdef0123456789", "r-t-5").is_none());
        std::fs::write(ws.path().join("research/runs/abcdef0123456789.r-t-6.json"), format!(r#"{{"topic_id": "{}", "project": "no-such-project"}}"#, t.id)).unwrap();
        assert!(find_run(ws.path(), "abcdef0123456789", "r-t-6").is_none(), "a project that is not registered has no folder to read");
    }

    #[test]
    fn topics_are_listed_newest_first_and_shown_as_labelled_data() {
        let ws = tempfile::tempdir().unwrap();
        assert!(list(ws.path()).is_empty());
        assert!(render_list(&[]).contains("no research topics yet"));
        let a = Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-1")).unwrap();
        a.finish("r-t-1", "done", Some(&result("An answer.\u{1b}[31m with an escape")), &["abc123abc123".to_string()], None, 7).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let b = Topic::begin(ws.path(), &begin("What is a quokka?", "r-t-2")).unwrap();
        std::fs::create_dir_all(ws.path().join("research/topics/not a topic")).unwrap();
        let all = list(ws.path());
        assert_eq!(all.iter().map(|(t, _)| t.id.as_str()).collect::<Vec<_>>(), [b.id.as_str(), a.id.as_str()]);
        let text = render_list(&all);
        assert!(text.contains(&a.id) && text.contains("What is a quokka?") && text.contains("1 claim(s)"), "{text}");
        assert_eq!(find(ws.path(), &a.id).unwrap().dir, a.dir);
        assert!(find(ws.path(), "nope-deadbeef").is_none() && find(ws.path(), "../x").is_none());
        let shown = render(&a, &a.read().unwrap());
        assert!(shown.contains("question   Does aspirin reduce fever?") && shown.contains("state      done") && shown.contains("abc123abc123"), "{shown}");
        assert!(shown.contains("answer (not validated") && shown.contains("An answer."), "{shown}");
        assert!(!shown.contains('\u{1b}'), "control characters in the answer are not printed");
    }

    // ---- `specs/112` R6: continue and due ----

    #[test]
    fn a_continued_run_is_one_more_run_of_the_record_and_starts_from_its_gaps_and_the_pages_it_holds() {
        let ws = tempfile::tempdir().unwrap();
        let t = Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-1")).unwrap();
        t.page("r-t-1", "https://who.int/a", "src-who", Some("sha256:aa")).unwrap();
        t.page("r-t-1", "https://nohash.example/b", "src-b", None).unwrap();
        t.finish("r-t-1", "done", Some(&result("An answer.")), &["abc123abc123".to_string()], None, 7).unwrap();
        let resume = t.resume(Some("  in children \n second line")).unwrap();
        assert_eq!(resume.gaps, ["kb: kb_empty", "coverage is thin: 1 kept, cited claim(s) from the knowledge base and the trusted sources, under 3"]);
        assert_eq!(resume.held.iter().collect::<Vec<_>>(), [(&"https://who.int/a".to_string(), &"sha256:aa".to_string())], "a page with no hash is read again");
        assert_eq!(resume.note.as_deref(), Some("in children"), "one line");
        t.continue_run(Asker::operator(), Depth::Quick, "r-t-2", Some("in children\u{1b}[2J\nmore")).unwrap();
        t.unchanged("r-t-2").unwrap();
        t.unchanged("r-t-2").unwrap();
        let rec = t.read().unwrap();
        let run = &rec["runs"][1];
        assert_eq!((rec["runs"].as_array().unwrap().len(), run["continued"].as_bool(), run["note"].as_str(), run["unchanged"].as_u64(), run["state"].as_str(), run["depth"].as_str()), (2, Some(true), Some("in children[2J"), Some(2), Some("queued"), Some("quick")), "{rec}");
        assert_eq!((rec["question"].as_str(), rec["pages"].as_array().unwrap().len(), rec["claims"].as_array().unwrap().len(), t.answer().as_str()), (Some("Does aspirin reduce fever?"), 2, 1, "An answer."), "what earlier runs wrote stays");
        assert!(t.continue_run(Asker::operator(), Depth::Quick, "../x", None).is_err());
        let none = Topic::at(ws.path(), "no-such-topic-0123abcd").unwrap();
        assert!(none.continue_run(Asker::operator(), Depth::Quick, "r-t-9", None).is_err() && none.resume(None).is_err(), "a topic with no record is not continued");
    }

    #[test]
    fn a_topic_is_due_when_its_newest_run_is_old_or_a_source_changed_or_was_retracted_and_never_while_it_runs() {
        let day = 86_400;
        let ended = delphi_core::time::parse_rfc3339("2026-01-01T00:00:00Z").unwrap();
        let rec = |state: &str, runs: Value| json!({"state": state, "runs": runs, "pages": [{"url": "https://who.int/a", "source_id": "src-who", "content_hash": "sha256:aa"}]});
        let done = rec("done", json!([{"state": "failed", "ended": "2026-06-01T00:00:00Z"}, {"state": "done", "ended": "2026-01-01T00:00:00Z"}]));
        let r = Refresh::default();
        let same = |_: &str| Some(json!({"content_hash": "sha256:aa"}));
        assert_eq!(due(&done, ended + 90 * day, &r, &same), None, "ninety days is not over ninety");
        assert_eq!(due(&done, ended + 91 * day, &r, &same).as_deref(), Some("its newest run is 91 day(s) old, over its 90"), "a failed run is not the newest run");
        assert_eq!(due(&done, ended + 91 * day, &r, &|_| None), due(&done, ended + 91 * day, &r, &same), "a source the library no longer holds changes nothing");
        // A source it rests on changed, or was retracted: due at once, whatever the age.
        assert_eq!(due(&done, ended + day, &r, &|_| Some(json!({"content_hash": "sha256:bb"}))).as_deref(), Some("a source it rests on has changed"));
        assert_eq!(due(&done, ended + day, &r, &|_| Some(json!({"content_hash": "sha256:aa", "retraction": {"kind": "retracted"}}))).as_deref(), Some("a source it rests on was retracted"));
        assert_eq!(due(&done, ended + day, &r, &|_| Some(json!({"content_hash": "sha256:aa", "retraction": {"kind": "corrected"}}))), None, "a correction is not a retraction");
        assert_eq!(due(&done, ended + day, &r, &|_| Some(json!({}))), None, "a source with no hash cannot be said to have changed");
        // The record's own figure, held inside the bounds; the config's default otherwise.
        let mut own = done.clone();
        own["refresh_days"] = json!(30);
        assert!(due(&own, ended + 31 * day, &r, &same).is_some() && due(&own, ended + 30 * day, &r, &same).is_none());
        own["refresh_days"] = json!(1);
        assert_eq!((r.days_of(&own), due(&own, ended + 7 * day, &r, &same)), (7, None), "held to the least");
        own["refresh_days"] = json!(100_000);
        assert_eq!(r.days_of(&own), 730, "held to the most");
        assert!(due(&done, ended + 11 * day, &Refresh { days: 10, min: 7, max: 730 }, &same).is_some());
        // Never while a run is going, and never with no finished run.
        assert_eq!(due(&rec("running", done["runs"].clone()), ended + 500 * day, &r, &same), None);
        assert_eq!(due(&rec("failed", json!([{"state": "failed", "ended": "2026-01-01T00:00:00Z"}])), ended + 500 * day, &r, &same), None);
        assert_eq!(due(&rec("cancelled", json!([])), ended + 500 * day, &r, &same), None);
    }

    #[test]
    fn due_topics_are_listed_from_disk_with_why_and_the_list_says_nothing_is_run() {
        let (ws, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let t = Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-1")).unwrap();
        t.finish("r-t-1", "done", Some(&result("An answer.")), &[], None, 7).unwrap();
        let r = Refresh::default();
        assert!(due_topics(ws.path(), home.path(), &r).is_empty(), "a run of a moment ago is not old");
        assert!(render_due(&[]).contains("no research topic is due"));
        // The same record with its run a year back.
        let mut rec = t.read().unwrap();
        rec["runs"][0]["ended"] = json!("2025-01-01T00:00:00Z");
        t.write(&rec).unwrap();
        let listed = due_topics(ws.path(), home.path(), &r);
        assert_eq!(listed.len(), 1);
        let text = render_due(&listed);
        assert!(text.contains(&t.id) && text.contains("due: its newest run is") && text.contains("Does aspirin reduce fever?") && text.contains("nothing is run again by itself"), "{text}");
    }
}
