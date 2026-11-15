//! `delphi research ingest` (`specs/113` §3): research that already exists as files (Markdown a host session wrote in a project's
//! `research/` folder, and the workspace's imported summaries) becomes research topics.
//!
//! * For each file: a topic record with `runner: "ingest"`, the file as a **private source** of kind `file` with its hash
//!   (`Library::add_private_pointer`: the locator, the title, the date, the hash; never a body), and the file's body as the answer **by
//!   reference**: `answer.md` names the file and its hash, and the text stays where it is.
//! * A file whose content hash a topic already records is skipped. A file a topic already names, with a new hash, adds a run to it.
//! * [`plan`] only reads. [`apply`] writes; the command calls it only with `--apply`, which is refused inside an agent seat.
//!
//! What a file says is data: nothing in it is read as an instruction, and nothing here validates or raises anything.

use crate::library::Library;
use crate::research_record::{self as rr, Asker, Begin, Topic};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The largest file read: a research note or summary, not a corpus.
pub const MAX_FILE: u64 = 2 * 1024 * 1024;

/// What ingest would do with one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No topic names the file: a new topic.
    New,
    /// A topic names the file and records another hash: a new run of it.
    Changed(String),
    /// A topic already records this content hash: nothing to do.
    Unchanged(String),
    /// The file cannot be ingested, and why.
    Refused(String),
}

/// One file of the plan.
#[derive(Debug, Clone)]
pub struct Item {
    /// The file, canonical.
    pub path: PathBuf,
    /// `sha256:<hex>` of its bytes.
    pub hash: String,
    /// The folder its topic lives under (the project's root, or the workspace) and the project's name.
    pub root: PathBuf,
    pub project: Option<String>,
    /// The question the topic carries: the file's title.
    pub question: String,
    pub action: Action,
    /// The body, below any frontmatter (read once, never stored).
    pub body: String,
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn clip(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect::<String>().trim().to_string()
}

/// The title of a research file: its frontmatter `title:`, else its first `# ` heading, else its file name.
pub fn title_of(path: &Path, text: &str) -> String {
    let (fm, body) = crate::workspace::split_frontmatter(text);
    let from_fm = fm.as_ref().and_then(|v| v.get("title")).and_then(|t| t.as_str()).map(|t| t.trim().to_string());
    let from_heading = || body.lines().find_map(|l| l.strip_prefix("# ")).map(|t| t.trim().to_string());
    let name = || path.file_stem().map(|s| s.to_string_lossy().replace(['-', '_'], " ")).unwrap_or_else(|| "research".into());
    let t = from_fm.filter(|t| !t.is_empty()).or_else(from_heading).filter(|t| !t.is_empty()).unwrap_or_else(name);
    clip(&t, 200)
}

/// The topics under `root` that ingest wrote, with the file each names and every hash it recorded.
fn ingested(root: &Path) -> Vec<(Topic, String, Vec<String>)> {
    let Ok(rd) = std::fs::read_dir(rr::topics_dir(root)) else { return vec![] };
    rd.flatten()
        .filter_map(|e| Topic::at(root, &e.file_name().to_string_lossy()))
        .filter_map(|t| {
            let rec = t.read().ok()?;
            let path = rec["ingest"]["path"].as_str()?.to_string();
            let hashes = rec["ingest"]["hashes"].as_array().map(|a| a.iter().filter_map(|h| h.as_str().map(String::from)).collect()).unwrap_or_default();
            Some((t, path, hashes))
        })
        .collect()
}

/// The registered project a path lies in, as `(name, root)`; `None` for the workspace.
fn project_of(ws: &Path, path: &Path) -> Option<(String, PathBuf)> {
    crate::scope::project_list(ws).into_iter().filter(|(_, root)| path.starts_with(root)).max_by_key(|(_, root)| root.components().count())
}

/// Every file `--all` reads (§3 item 5): the Markdown under each registered project's `research/` folder (not the topics ingest and
/// research runs write there) and the workspace's imported summaries (`local/summaries/*.md`).
pub fn all_paths(ws: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let Ok(t) = e.file_type() else { continue };
            if t.is_symlink() {
                continue;
            }
            if t.is_dir() && depth < 4 && !matches!(e.file_name().to_str(), Some("topics" | "runs" | "search-logs")) {
                walk(&p, depth + 1, out);
            } else if t.is_file() && p.extension().is_some_and(|x| x == "md") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for (_, root) in crate::scope::project_list(ws) {
        walk(&root.join("research"), 0, &mut out);
    }
    let summaries = ws.join(crate::kb_import::DIR);
    if let Ok(rd) = std::fs::read_dir(&summaries) {
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md")).collect();
        files.sort();
        out.extend(files);
    }
    out
}

/// What ingest would do with each file. Reads only.
pub fn plan(ws: &Path, paths: &[PathBuf]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for p in paths {
        let refused = |why: &str| Item { path: p.clone(), hash: String::new(), root: ws.to_path_buf(), project: None, question: String::new(), action: Action::Refused(why.to_string()), body: String::new() };
        let Ok(path) = p.canonicalize() else {
            out.push(refused("no such file"));
            continue;
        };
        if out.iter().any(|i| i.path == path) {
            continue;
        }
        let meta = std::fs::metadata(&path);
        if !meta.as_ref().is_ok_and(|m| m.is_file()) || path.extension().is_none_or(|x| x != "md") {
            out.push(refused("not a Markdown file"));
            continue;
        }
        if meta.is_ok_and(|m| m.len() > MAX_FILE) {
            out.push(refused("larger than 2 MiB"));
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            out.push(refused("cannot be read"));
            continue;
        };
        let Ok(text) = String::from_utf8(bytes.clone()) else {
            out.push(refused("not UTF-8 text"));
            continue;
        };
        let hash = format!("sha256:{}", sha256_hex(&bytes));
        let (root, project) = match project_of(ws, &path) {
            Some((name, root)) => (root, Some(name)),
            None => (ws.to_path_buf(), None),
        };
        let shown = path.to_string_lossy().to_string();
        let mine = ingested(&root);
        let action = if let Some((t, _, _)) = mine.iter().find(|(_, _, hashes)| hashes.contains(&hash)) {
            Action::Unchanged(t.id.clone())
        } else if let Some((t, _, _)) = mine.iter().find(|(_, at, _)| *at == shown) {
            Action::Changed(t.id.clone())
        } else {
            Action::New
        };
        let mut question = title_of(&path, &text);
        // A new file whose title is already another file's topic gets the file name beside it, so two files are two topics.
        if action == Action::New && Topic::at(&root, &rr::topic_id(&question)).is_some_and(|t| t.exists()) {
            question = format!("{question} ({})", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        }
        if let Action::Changed(id) = &action {
            if let Some(q) = Topic::at(&root, id).and_then(|t| t.read().ok()).and_then(|r| r["question"].as_str().map(String::from)) {
                question = q;
            }
        }
        let body = crate::workspace::split_frontmatter(&text).1.to_string();
        out.push(Item { path, hash, root, project, question, action, body });
    }
    out
}

/// `answer.md` of an ingested topic: the file by reference, never a copy of it.
fn answer_reference(item: &Item) -> String {
    format!("The answer is the file `{}` ({}), read where it is; its text is not copied here. It was written outside Delphi and is not validated.", item.path.display(), item.hash)
}

/// Write one file of the plan (§3 item 1). Nothing for a file that is unchanged or refused.
pub fn apply(home: &Path, item: &Item, run_id: &str) -> Result<Option<Value>, String> {
    if matches!(item.action, Action::Unchanged(_) | Action::Refused(_)) {
        return Ok(None);
    }
    let started = std::time::Instant::now();
    let lib = Library::in_home(home);
    let shown = item.path.to_string_lossy().to_string();
    let source = lib.add_private_pointer("file", &shown, &item.question, &delphi_core::time::now_rfc3339(), Some(&item.hash))?;
    let topic = Topic::begin(&item.root, &Begin { question: &item.question, asked_by: Asker::operator(), project: item.project.as_deref(), depth: delphi_core::research::Depth::Survey, run_id })?;
    topic.runner(run_id, "ingest")?;
    topic.running(run_id)?;
    topic.private_source(run_id, &source, "file")?;
    let result = json!({"runner": "ingest", "answer": answer_reference(item), "source_id": source, "content_hash": item.hash, "knowledge_status": "noted", "note": "not validated"});
    topic.finish(run_id, "done", Some(&result), &[], None, started.elapsed().as_millis() as u64)?;
    let (path, hash) = (shown.clone(), item.hash.clone());
    topic.amend(|rec| {
        let mut hashes: Vec<Value> = rec["ingest"]["hashes"].as_array().cloned().unwrap_or_default();
        if !hashes.iter().any(|h| h == hash.as_str()) {
            hashes.push(json!(hash));
        }
        rec["ingest"] = json!({"path": path, "content_hash": hash, "source_id": source, "hashes": hashes});
    })?;
    Ok(Some(json!({"topic_id": topic.id, "run_id": run_id})))
}

/// A run id for one file of an apply: `i-<time>-<n>`.
pub fn run_id(n: usize) -> String {
    format!("i-{}-{n}", delphi_core::time::now_rfc3339().chars().filter(char::is_ascii_digit).take(14).collect::<String>())
}

/// The plan as lines, for the command.
pub fn render(items: &[Item], applied: bool) -> String {
    if items.is_empty() {
        return "nothing to ingest: name Markdown files, or pass --all\n".into();
    }
    let mut out = String::new();
    for i in items {
        let what = match &i.action {
            Action::New => format!("{} a new topic: \"{}\"", if applied { "ingested as" } else { "would ingest as" }, clip(&i.question, 120)),
            Action::Changed(id) => format!("{} a run to topic {id} (the file changed)", if applied { "added" } else { "would add" }),
            Action::Unchanged(id) => format!("unchanged, skipped (topic {id})"),
            Action::Refused(why) => format!("skipped: {why}"),
        };
        out.push_str(&format!("{}\n    {what}\n", i.path.display()));
    }
    if !applied {
        out.push_str("\nNothing was written: pass --apply to write these topics.\n");
    }
    out
}

/// The plan as JSON, for `--json`.
pub fn to_json(items: &[Item], applied: bool) -> Value {
    json!({"applied": applied, "files": items.iter().map(|i| {
        let (action, topic) = match &i.action {
            Action::New => ("new", None),
            Action::Changed(id) => ("changed", Some(id.clone())),
            Action::Unchanged(id) => ("unchanged", Some(id.clone())),
            Action::Refused(_) => ("refused", None),
        };
        json!({"path": i.path, "action": action, "topic_id": topic, "content_hash": i.hash, "question": i.question, "reason": if let Action::Refused(w) = &i.action { json!(w) } else { Value::Null }})
    }).collect::<Vec<_>>()})
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fx {
        _t: tempfile::TempDir,
        home: PathBuf,
        ws: PathBuf,
    }

    fn fx() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().canonicalize().unwrap();
        let ws = home.join("workspaces").join("w");
        std::fs::create_dir_all(&ws).unwrap();
        Fx { _t: t, home, ws }
    }

    fn tree(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        fn go(d: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                if e.path().is_dir() {
                    go(&e.path(), out);
                } else {
                    out.push(format!("{} {}", e.path().display(), std::fs::metadata(e.path()).map(|m| m.len()).unwrap_or(0)));
                }
            }
        }
        go(dir, &mut out);
        out.sort();
        out
    }

    const NOTE: &str = "---\ntitle: Zebrafish husbandry notes\n---\n\n# Zebrafish\n\nZebrafish tanks are kept at about 28 degrees in most labs.\nWater is changed once a week in a standard system.\n";

    /// AC-4 (first half): without --apply nothing changes; with it the file is a topic with a private file source and the answer by
    /// reference; a second run on the unchanged file does nothing; a changed file adds a run.
    #[test]
    fn ingest_is_a_dry_run_until_applied_then_a_topic_and_an_unchanged_file_is_skipped() {
        let f = fx();
        let file = f.ws.join("notes.md");
        std::fs::write(&file, NOTE).unwrap();
        let before = tree(&f.home);
        let p = plan(&f.ws, std::slice::from_ref(&file));
        assert_eq!(p[0].action, Action::New);
        assert!(render(&p, false).contains("Nothing was written"));
        assert_eq!(tree(&f.home), before, "a dry run changes nothing");
        let done = apply(&f.home, &p[0], "i-1").unwrap().unwrap();
        let topic = Topic::at(&f.ws, done["topic_id"].as_str().unwrap()).unwrap();
        let rec = topic.read().unwrap();
        assert_eq!((rec["runner"].as_str(), rec["question"].as_str(), rec["private"].as_bool()), (Some("ingest"), Some("Zebrafish husbandry notes"), Some(true)));
        assert_eq!(rec["asked_by"]["kind"], "operator");
        assert_eq!(rec["ingest"]["content_hash"], json!(p[0].hash));
        let sid = rec["private_sources"][0]["source_id"].as_str().unwrap();
        let meta = Library::in_home(&f.home).read_meta(sid).unwrap();
        assert_eq!((meta["private"].as_bool(), meta["source_kind"].as_str(), meta["content_hash"].as_str(), meta["capture_level"].as_str()), (Some(true), Some("file"), Some(p[0].hash.as_str()), Some("pointer")));
        assert!(!Library::in_home(&f.home).source_dir(sid).join("source.md").exists(), "the file is a pointer: no copy of its text");
        let answer = topic.answer();
        assert!(answer.contains(&file.canonicalize().unwrap().display().to_string()) && !answer.contains("28 degrees"), "the answer is by reference: {answer}");
        // A second run on the unchanged file does nothing.
        let again = plan(&f.ws, std::slice::from_ref(&file));
        assert_eq!(again[0].action, Action::Unchanged(topic.id.clone()));
        let before = tree(&f.home);
        assert!(apply(&f.home, &again[0], "i-2").unwrap().is_none());
        assert_eq!(tree(&f.home), before);
        // A changed file adds a run to its topic.
        std::fs::write(&file, format!("{NOTE}\nLarvae are fed paramecia during the first week.\n")).unwrap();
        let changed = plan(&f.ws, std::slice::from_ref(&file));
        assert_eq!(changed[0].action, Action::Changed(topic.id.clone()));
        apply(&f.home, &changed[0], "i-3").unwrap().unwrap();
        let rec = topic.read().unwrap();
        assert_eq!((rec["runs"].as_array().unwrap().len(), rec["ingest"]["hashes"].as_array().unwrap().len()), (2, 2));
    }

    #[test]
    fn all_walks_each_projects_research_folder_and_the_summaries_but_not_topics() {
        let f = fx();
        let proj = f.home.join("proj");
        std::fs::create_dir_all(proj.join("research/topics/x-deadbeef")).unwrap();
        std::fs::write(proj.join("research/a.md"), "# A\n").unwrap();
        std::fs::write(proj.join("research/topics/x-deadbeef/answer.md"), "# x\n").unwrap();
        std::fs::write(proj.join("research/notes.txt"), "no").unwrap();
        let id = "a".repeat(32);
        std::fs::create_dir_all(f.ws.join("local")).unwrap();
        std::fs::write(f.ws.join("local/projects.json"), json!({"schema": 1, "projects": {id.clone(): {"project_id": id, "root": proj, "display_name": "proj", "created_unix": 1}}}).to_string()).unwrap();
        std::fs::create_dir_all(f.ws.join("local/summaries")).unwrap();
        std::fs::write(f.ws.join("local/summaries/s1.md"), "# S\n").unwrap();
        let all = all_paths(&f.ws);
        assert_eq!(all, vec![proj.join("research/a.md"), f.ws.join("local/summaries/s1.md")]);
        let p = plan(&f.ws, &all);
        assert_eq!((p[0].project.as_deref(), p[0].root.as_path()), (Some("proj"), proj.as_path()), "a project's file is that project's topic");
        assert_eq!(p[1].project, None);
        assert!(matches!(plan(&f.ws, &[f.ws.join("missing.md")])[0].action, Action::Refused(_)));
    }
}
