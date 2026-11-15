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
    /// The claims taken from the file (§3 item 2): each sentence and the line of the file it is on, at most [`MAX_CLAIMS`].
    pub claims: Vec<(String, usize)>,
    /// The URLs the file lists (§3 item 3), at most [`MAX_URLS`].
    pub urls: Vec<String>,
    /// The file lists no source: no URL, and no `sources` in its frontmatter (§3 item 6).
    pub no_sources: bool,
}

/// Claims taken from one file at most (§3 item 2).
pub const MAX_CLAIMS: usize = 20;
/// URLs kept as source pointers from one file at most.
pub const MAX_URLS: usize = 40;
/// The flag a file that lists no source carries, in its record and in search results (§3 item 6).
pub const NO_SOURCES: &str = "no sources listed";
/// The mark an ingested claim carries: the file said it, nothing checked it.
pub const INGESTED: &str = "ingested from a file";

/// The claims of a file: the sentences `delphi kb extract --auto` would take (`library::extract::sentences`), each with the line
/// (counting from 1, frontmatter included) it stands on, once each, at most [`MAX_CLAIMS`].
pub fn claims_of(text: &str) -> Vec<(String, usize)> {
    let body_start = text.len() - crate::workspace::split_frontmatter(text).1.len();
    let first_body_line = text[..body_start].lines().count() + 1;
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<(String, usize)> = Vec::new();
    for s in crate::library::extract::sentences(crate::workspace::split_frontmatter(text).1) {
        if out.iter().any(|(t, _)| *t == s) {
            continue;
        }
        if let Some(i) = lines.iter().enumerate().skip(first_body_line - 1).find(|(_, l)| l.contains(s.as_str())).map(|(i, _)| i) {
            out.push((s, i + 1));
        }
        if out.len() >= MAX_CLAIMS {
            break;
        }
    }
    out
}

/// The http(s) URLs a file lists, in order, once each, at most [`MAX_URLS`]: in its text and in a frontmatter `sources` list.
pub fn urls_of(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (at, _) in text.match_indices("http") {
        let rest = &text[at..];
        if !(rest.starts_with("https://") || rest.starts_with("http://")) || text[..at].chars().next_back().is_some_and(char::is_alphanumeric) {
            continue;
        }
        let end = rest.find(|c: char| c.is_whitespace() || matches!(c, ')' | '>' | ']' | '"' | '\'' | '<' | '`')).unwrap_or(rest.len());
        let url = rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']).to_string();
        if url.len() > 10 && url.len() <= 300 && !out.contains(&url) {
            out.push(url);
        }
        if out.len() >= MAX_URLS {
            break;
        }
    }
    out
}

/// Does the file list a source: a URL anywhere, or a non-empty `sources` list in its frontmatter?
fn lists_sources(text: &str) -> bool {
    let fm_sources = crate::workspace::split_frontmatter(text).0.as_ref().and_then(|v| v.get("sources")).and_then(|s| s.as_sequence()).is_some_and(|s| !s.is_empty());
    fm_sources || !urls_of(text).is_empty()
}

/// The flag a search result for an imported summary carries (§3 item 6): `summaries/<id>.md` whose file lists no source.
pub fn search_flag(ws: &Path, rel_path: &str) -> Option<&'static str> {
    let id = rel_path.strip_prefix("summaries/")?.strip_suffix(".md")?;
    if id.is_empty() || id.contains('/') || id.contains("..") {
        return None;
    }
    let text = std::fs::read_to_string(ws.join(crate::kb_import::DIR).join(format!("{id}.md"))).ok()?;
    (!lists_sources(&text)).then_some(NO_SOURCES)
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
        let refused = |why: &str| Item { path: p.clone(), hash: String::new(), root: ws.to_path_buf(), project: None, question: String::new(), action: Action::Refused(why.to_string()), body: String::new(), claims: vec![], urls: vec![], no_sources: false };
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
        out.push(Item { path, hash, root, project, question, action, body, claims: claims_of(&text), urls: urls_of(&text), no_sources: !lists_sources(&text) });
    }
    out
}

/// `answer.md` of an ingested topic: the file by reference, never a copy of it.
fn answer_reference(item: &Item) -> String {
    format!("The answer is the file `{}` ({}), read where it is; its text is not copied here. It was written outside Delphi and is not validated.", item.path.display(), item.hash)
}

/// Write one file of the plan (§3 item 1). Nothing for a file that is unchanged or refused.
///
/// Claims (§3 item 2): each sentence [`claims_of`] took is a library claim anchored to its line of the file (`<path>#L<n>-L<n>`), kept
/// as `ingested from a file`, private to the file's project (or this install), and counting as no origin. URLs (§3 item 3): each is
/// a source pointer in the record. With `fetch` (the command's `--fetch`), each public one is read by the daemon's own fetcher, kept in
/// the library, and a claim whose sentence is on that page gains a second anchor there; without it nothing is fetched. A file that lists
/// no source is flagged `no sources listed` (§3 item 6). The workspace's privacy identifiers are masked in the claims.
pub fn apply(home: &Path, item: &Item, run_id: &str, identifiers: &[String], fetch: Option<crate::host_research::FetchFn<'_>>) -> Result<Option<Value>, String> {
    use crate::library::claims::{AnchorSpec, ResearchClaim};
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
    // The claims, each anchored to its line of the file.
    let mut claims: Vec<(String, String)> = Vec::new(); // (claim id, the sentence as it is on the page)
    for (sentence, line) in &item.claims {
        let text = crate::host_research::mask(sentence, identifiers);
        let fp = blake3::hash(format!("ingest\0{}", crate::host_noted::fingerprint(&text)).as_bytes()).to_hex()[..32].to_string();
        let anchors = [AnchorSpec { source_id: source.clone(), locator: Some(format!("{shown}#L{line}-L{line}")), quote: text.clone(), prefix: None, suffix: None }];
        if let Ok(c) = lib.add_host_claim(&ResearchClaim { text: &text, fingerprint: &fp, project: item.project.as_deref(), topic: Some(&topic.id), anchors: &anchors }, INGESTED) {
            if let Some(id) = c["id"].as_str() {
                claims.push((id.to_string(), sentence.clone()));
            }
        }
    }
    // The URLs: pointers, and with `fetch` a public page the daemon reads and keeps, where the claims' sentences gain a second anchor.
    let mut pages = 0usize;
    if let Some(fetch) = fetch {
        for url in item.urls.iter().filter(|u| crate::host_noted::https_host(u).is_some() && crate::host_research::mask(u, identifiers) == **u) {
            let got = fetch(url).and_then(|(at, snap)| {
                let text = snap.text.for_daemon().clone();
                let stored = lib.add_fetched(&at, &snap, &crate::library::AddOpts::default(), &crate::library::Budgets::default()).map_err(|_| "capture_failed".to_string())?;
                Ok((stored.id, at, text))
            });
            match got {
                Ok((sid, at, text)) => {
                    pages += 1;
                    let hash = lib.read_meta(&sid).and_then(|m| m["content_hash"].as_str().map(String::from));
                    topic.page(run_id, &at, &sid, hash.as_deref())?;
                    for (id, sentence) in &claims {
                        if !text.contains(sentence.as_str()) {
                            continue;
                        }
                        let Some(c) = lib.claim(id) else { continue };
                        let Some(fp) = c["fingerprints"][0].as_str() else { continue };
                        let anchors = [AnchorSpec { source_id: sid.clone(), locator: Some(at.clone()), quote: sentence.clone(), prefix: None, suffix: None }];
                        let _ = lib.add_research_claim(&ResearchClaim { text: c["text"].as_str().unwrap_or(""), fingerprint: fp, project: item.project.as_deref(), topic: Some(&topic.id), anchors: &anchors });
                    }
                }
                Err(e) => topic.refused(run_id, url, if e == "capture_failed" { "capture_failed" } else if e.contains("OffList") { "not_fetchable" } else { "fetch_failed" })?,
            }
        }
    }
    let ids: Vec<String> = claims.iter().map(|(id, _)| id.clone()).collect();
    let result = json!({"runner": "ingest", "answer": answer_reference(item), "source_id": source, "content_hash": item.hash, "knowledge_status": "noted", "note": "not validated", "library_claims": ids, "pages_fetched": pages});
    topic.finish(run_id, "done", Some(&result), &ids, None, started.elapsed().as_millis() as u64)?;
    let (path, hash) = (shown.clone(), item.hash.clone());
    let pointers: Vec<String> = item.urls.iter().map(|u| crate::host_research::mask(u, identifiers)).collect();
    let no_sources = item.no_sources;
    topic.amend(|rec| {
        rec["pointers"] = json!(pointers);
        rec["flags"] = json!(if no_sources { vec![NO_SOURCES] } else { vec![] });
        let mut hashes: Vec<Value> = rec["ingest"]["hashes"].as_array().cloned().unwrap_or_default();
        if !hashes.iter().any(|h| h == hash.as_str()) {
            hashes.push(json!(hash));
        }
        rec["ingest"] = json!({"path": path, "content_hash": hash, "source_id": source, "hashes": hashes});
    })?;
    Ok(Some(json!({"topic_id": topic.id, "run_id": run_id, "claims": ids.len(), "pointers": item.urls.len(), "pages_fetched": pages, "no_sources": no_sources})))
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
        if matches!(i.action, Action::New | Action::Changed(_)) {
            out.push_str(&format!("    {} claim(s) anchored to lines of the file, {} source URL(s){}\n", i.claims.len(), i.urls.len(), if i.no_sources { "; flagged: no sources listed" } else { "" }));
        }
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
        json!({"path": i.path, "action": action, "topic_id": topic, "content_hash": i.hash, "question": i.question, "claims": i.claims.len(), "urls": i.urls, "no_sources": i.no_sources, "reason": if let Action::Refused(w) = &i.action { json!(w) } else { Value::Null }})
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
        let done = apply(&f.home, &p[0], "i-1", &[], None).unwrap().unwrap();
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
        assert!(apply(&f.home, &again[0], "i-2", &[], None).unwrap().is_none());
        assert_eq!(tree(&f.home), before);
        // A changed file adds a run to its topic.
        std::fs::write(&file, format!("{NOTE}\nLarvae are fed paramecia during the first week.\n")).unwrap();
        let changed = plan(&f.ws, std::slice::from_ref(&file));
        assert_eq!(changed[0].action, Action::Changed(topic.id.clone()));
        apply(&f.home, &changed[0], "i-3", &[], None).unwrap().unwrap();
        let rec = topic.read().unwrap();
        assert_eq!((rec["runs"].as_array().unwrap().len(), rec["ingest"]["hashes"].as_array().unwrap().len()), (2, 2));
    }

    const SOURCED: &str = "# Tank temperature\n\nSee https://fish.example.org/a and https://offlist.example.net/b for more.\n\nZebrafish tanks are kept at about 28 degrees in most labs.\nThe heron facility changes the water once every single week.\n";

    /// AC-4 (second half): claims are taken from the file, each anchored to its line; the URLs are pointers; without --fetch nothing is
    /// fetched, with it a public page is kept and a sentence it holds gains a second anchor there.
    #[test]
    fn claims_are_anchored_to_lines_urls_are_pointers_and_fetch_adds_a_second_anchor() {
        let f = fx();
        let file = f.ws.join("tank.md");
        std::fs::write(&file, SOURCED).unwrap();
        let p = plan(&f.ws, std::slice::from_ref(&file));
        assert_eq!(p[0].claims, vec![("Zebrafish tanks are kept at about 28 degrees in most labs".to_string(), 5), ("The heron facility changes the water once every single week".to_string(), 6)]);
        assert_eq!(p[0].urls, vec!["https://fish.example.org/a".to_string(), "https://offlist.example.net/b".to_string()]);
        assert!(!p[0].no_sources);
        // Without --fetch: nothing fetched, the claims are anchored to their lines of the file.
        let ids = vec!["heron".to_string()];
        let done = apply(&f.home, &p[0], "i-1", &ids, None).unwrap().unwrap();
        assert_eq!((done["claims"].as_u64(), done["pages_fetched"].as_u64()), (Some(2), Some(0)));
        let lib = Library::in_home(&f.home);
        let topic = Topic::at(&f.ws, done["topic_id"].as_str().unwrap()).unwrap();
        let rec = topic.read().unwrap();
        assert_eq!((rec["pointers"].as_array().unwrap().len(), rec["pages"].as_array().unwrap().len(), rec["claims"].as_array().unwrap().len()), (2, 0, 2));
        assert_eq!(rec["flags"], json!([]));
        let path = file.canonicalize().unwrap().display().to_string();
        for c in lib.claims() {
            assert_eq!((c["host_reported"].clone(), c["scope"].as_str(), c["support"]["origins"].as_u64().unwrap_or(0)), (json!([INGESTED]), Some("personal"), 0), "{c}");
            let loc = c["anchors"][0]["locator"].as_str().unwrap();
            assert!(loc == format!("{path}#L5-L5") || loc == format!("{path}#L6-L6"), "{loc}");
            assert!(!c["text"].as_str().unwrap().to_lowercase().contains("heron"), "identifiers are masked: {c}");
        }
        // With --fetch, on a changed file: the public page is read, kept, and the sentence it holds gains a second anchor there.
        std::fs::write(&file, format!("{SOURCED}\nLarvae are fed paramecia during their first week of life.\n")).unwrap();
        let asked = std::cell::RefCell::new(Vec::new());
        let web = |u: &str| -> Result<crate::host_research::Fetched, String> {
            asked.borrow_mut().push(u.to_string());
            match u {
                "https://fish.example.org/a" => Ok((u.to_string(), crate::fact_fetch::test_text_snapshot(u, "In short, Zebrafish tanks are kept at about 28 degrees in most labs. Nothing else."))),
                _ => Err("OffList".into()),
            }
        };
        let p = plan(&f.ws, std::slice::from_ref(&file));
        let done = apply(&f.home, &p[0], "i-2", &ids, Some(&web)).unwrap().unwrap();
        assert_eq!(asked.borrow().len(), 2, "each URL is asked for once");
        assert_eq!(done["pages_fetched"].as_u64(), Some(1));
        let rec = topic.read().unwrap();
        assert_eq!((rec["pages"].as_array().unwrap().len(), rec["refused"][0]["reason"].as_str()), (1, Some("not_fetchable")));
        let warm = lib.claims().into_iter().find(|c| c["text"] == "Zebrafish tanks are kept at about 28 degrees in most labs").unwrap();
        assert_eq!(warm["anchors"].as_array().unwrap().len(), 2, "a second anchor, on the page: {warm}");
        assert_eq!(warm["scope"], "personal", "the second anchor does not widen who is offered the claim");
    }

    /// §3 item 6: a summary that lists no source is ingested, flagged in its record and in search results.
    #[test]
    fn a_summary_with_no_sources_is_ingested_and_flagged() {
        let f = fx();
        let dir = f.ws.join(crate::kb_import::DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s1.md"), "---\ntitle: Vendor overview\n---\n\nThe vendor ships a quarterly release to every customer.\n").unwrap();
        std::fs::write(dir.join("s2.md"), "---\ntitle: Sourced overview\nsources:\n  - a report\n---\n\nThe vendor ships a quarterly release to every customer.\n").unwrap();
        let p = plan(&f.ws, &all_paths(&f.ws));
        assert_eq!((p[0].no_sources, p[1].no_sources), (true, false));
        assert!(render(&p, false).contains("flagged: no sources listed"));
        let done = apply(&f.home, &p[0], "i-1", &[], None).unwrap().unwrap();
        let rec = Topic::at(&f.ws, done["topic_id"].as_str().unwrap()).unwrap().read().unwrap();
        assert_eq!(rec["flags"], json!([NO_SOURCES]));
        assert_eq!(search_flag(&f.ws, "summaries/s1.md"), Some(NO_SOURCES));
        assert_eq!(search_flag(&f.ws, "summaries/s2.md"), None);
        assert_eq!(search_flag(&f.ws, "summaries/../x.md"), None);
        assert_eq!(search_flag(&f.ws, "context/s1.md"), None);
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
