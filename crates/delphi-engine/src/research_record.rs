//! The research record (`specs/112` §2): what one research topic leaves on disk, written as the run goes.
//!
//! A topic is a folder, `<root>/research/topics/<topic-id>/`, where `<root>` is the workspace, or the project's own folder when the
//! run belongs to a registered project. It holds three files:
//!
//! * `record.json`: the question as asked, who asked, the project, the depth, the state, the plan, every query with its provider and
//!   count of results, every page read with its source id and content hash, the pages refused and why, the claim ids, the gaps, the
//!   step and time use, and `runs`, one entry per time the topic was run (`schemas/research-record.schema.json`);
//! * `answer.md`: the written answer of the newest finished run;
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
/// The tier every record and answer carries: text from outside, never validated by being written down.
pub const TAINT: &str = "external_untrusted";
/// What a list in the record holds at most. Past it nothing more is added; the counts in `runs` still say how much there was.
const LIST_CAP: usize = 500;
const TEXT_CAP: usize = 300;
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
                write_atomic(&self.answer_path(), answer_file(&self.id, &question, run_id, answer).as_bytes())?;
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
            });
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

/// The gaps a run reports, read by code from its coverage report: each rung that was searched and found nothing (with the fixed
/// reason the report gives), and thin coverage. Fixed phrases; no page or model text.
pub fn gaps_of(result: &Value, state: &str) -> Vec<String> {
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
/// result, its answer, its search log and the ids of its claims. A failure to write is not the job's.
pub fn record_end(topic: &Topic, run_id: &str, ended: &crate::research_jobs::Ended<'_>) {
    let ids: Vec<String> = ended.result.and_then(|r| r["claims"].as_array()).into_iter().flatten().filter_map(|c| c["id"].as_str().map(String::from)).collect();
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

    fn refused(&self, url: &str, why: &str) {
        let _ = self.topic.refused(&self.run_id, url, why);
    }
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
        let answer = "Sources read say:\n- aspirin reduces fever (\"lowers fever in adults\")\n\n# not a heading of ours (not validated: model synthesis)";
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
        // A second run of the topic replaces the answer; the first run's id then says so instead of returning the newer result.
        Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-2")).unwrap();
        t.finish("r-t-2", "done", Some(&result("a newer answer")), &["abc123abc123".to_string(), "def456def456".to_string()], None, 10).unwrap();
        assert_eq!(result_of(&t, "r-t-2").unwrap()["answer"], "a newer answer");
        let old = result_of(&t, "r-t-1").unwrap();
        assert_eq!((old["status"].as_str(), old["superseded_by"].as_str(), old.get("answer").is_none()), (Some("partial"), Some("r-t-2"), true));
        let rec = t.read().unwrap();
        assert_eq!(rec["claims"], json!(["abc123abc123", "def456def456"]), "claim ids gather across runs, once each");
        assert_eq!(rec["usage"]["ms"], 1244);
        // A run that failed has no result; its error is kept.
        Topic::begin(ws.path(), &begin("Does aspirin reduce fever?", "r-t-3")).unwrap();
        t.finish("r-t-3", "failed", None, &[], Some("the job ran out of time"), 5).unwrap();
        let failed = result_of(&t, "r-t-3").unwrap();
        assert_eq!((failed["status"].as_str(), failed["error"].as_str()), (Some("failed"), Some("the job ran out of time")));
        assert_eq!(t.answer(), "a newer answer", "a failed run leaves the last answer in place");
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
}
