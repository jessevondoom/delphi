//! Zone `research` of the search index (`specs/112` §3.5): topic records, their answers, and library claims.
//!
//! * A topic of the workspace gives two documents: `research/topics/<topic-id>/record.json` (what was asked, searched, read and
//!   found) and `research/topics/<topic-id>/answer.md` (the written answer).
//! * A library claim gives one: `research/claims/<claim-id>.json`, labelled by the claim's **derived** status
//!   (`library::claims::status`): a new research claim reads *noted*. The status is computed from the claim's own record every time
//!   the claim changes; the index stores the word and nothing a document says about itself can set it.
//!
//! **What a role may see follows the rules that already apply to the library.** Every document of the zone is indexed at the
//! library's one fixed tier (`research::TIER`), whatever its text or frontmatter says, exactly as `library/sources/<id>/source.md`
//! is, and reads *not validated* unless the claim's own derived status says more. Being in the index lifts nothing.
//!
//! **What stays out.** Nothing private is listed, so no search, serve or export path can carry it: a claim that rests on a private
//! source (`Library::rests_on_private`), and a topic whose record is private (one of its claims rests on a private source), with
//! its answer. A claim scoped to a project is listed under that project (`research/projects/<id>/claims/`), and one of those that
//! rests on a private or internal source apart (`research/projects/<id>/internal/`): the index's filter shows them to their own
//! project and, where the operator shares research (`specs/115`), to the workspace's other projects, the second kind only to a
//! reader that marks its session. A claim scoped `personal` (a host's claim Delphi could not check, `specs/113`) is not listed
//! at all. A topic that lives in a project's folder is that project's file and is read under the project's
//! rules (the project layer of the index), not here.
//!
//! The records and the claims are JSON; the index holds Markdown. [`text_of`] writes the Markdown the index holds for one, by
//! code, from the file's own fields. No model is called and nothing is fetched.

use crate::kb::SourceFile;
use serde_json::Value;
use std::path::Path;

pub const ZONE: &str = "research";
/// A document the index will not read whole: larger than any record or claim this program writes.
const MAX_BYTES: u64 = 1024 * 1024;
const LINE: usize = 300;
const LIST: usize = 40;

/// The statuses a claim can derive to, as the index stores them (`knowledge_status::Status::as_str`).
pub const STATUSES: [&str; 9] = ["noted", "corroborated", "substantiated", "vetted", "checking", "validated", "contested", "disproved", "retired"];

/// The status a document of the zone reads, from the word the index stored for it: a library claim's own derived status, and
/// *noted* for anything else (a record, an answer, or a word that is not a status).
pub fn status_of(word: Option<&str>) -> delphi_core::knowledge_status::Status {
    word.and_then(delphi_core::knowledge_status::Status::from_word).unwrap_or(delphi_core::knowledge_status::Status::Noted)
}

/// A logical path whose text the index derives with [`text_of`] instead of reading it as Markdown.
pub fn is_derived(rel_path: &str) -> bool {
    rel_path.starts_with("research/") && rel_path.ends_with(".json")
}

fn clip(s: &str, n: usize) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(n).collect::<String>().trim().to_string()
}

fn yaml(s: &str) -> String {
    serde_json::to_string(&clip(s, LINE)).unwrap_or_else(|_| "\"\"".into())
}

/// The research documents the workspace indexes. Listing only reads; nothing is written here.
pub fn files(ws: &Path, out: &mut Vec<SourceFile>) {
    let small = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES);
    let doc = |rel_path: String, path: std::path::PathBuf| SourceFile { rel_path, zone: ZONE, path, default_source: crate::research::TIER, bundle: None, vouched_by: None };
    // A topic's evidence pack (`specs/144` §2.5 item 2) is untrusted text, and is listed only while no source its runs read is private
    // (§2.6 item 3), its label computed now from the records. A search computes it again before it returns the pack.
    let home = ws.parent().and_then(Path::parent);
    let pack_lib = home.map(|h| crate::library::Library::in_home(h).with_workspace(ws));
    let pack_labeller = crate::label::Labeller::new(pack_lib.as_ref(), Some(ws));
    if let Ok(rd) = std::fs::read_dir(crate::research_record::topics_dir(ws)) {
        for e in rd.flatten() {
            let Some(topic) = crate::research_record::Topic::at(ws, &e.file_name().to_string_lossy()) else { continue };
            // A record that cannot be read, or that is private, lists nothing: neither itself nor its answer.
            let Ok(rec) = topic.read() else { continue };
            if rec["private"] != false || !small(&topic.record_path()) {
                continue;
            }
            out.push(doc(format!("research/topics/{}/record.json", topic.id), topic.record_path()));
            if small(&topic.answer_path()) {
                out.push(doc(format!("research/topics/{}/answer.md", topic.id), topic.answer_path()));
            }
            let pack = crate::evidence_pack::path_of(&topic);
            if small(&pack) && crate::evidence_pack::listed(&pack_labeller, &topic.id) {
                out.push(SourceFile { rel_path: format!("research/topics/{}/{}", topic.id, crate::evidence_pack::FILE), zone: ZONE, path: pack, default_source: crate::evidence_pack::TAINT, bundle: None, vouched_by: None });
            }
        }
    }
    let Some(home) = ws.parent().and_then(Path::parent) else { return };
    let lib = crate::library::Library::in_home(home);
    let projects = crate::scope::project_entries(ws);
    for c in lib.claims() {
        let Some(id) = c["id"].as_str() else { continue };
        let path = lib.claims_dir().join(format!("{id}.json"));
        if !small(&path) {
            continue;
        }
        // A `personal` claim is this install's alone: a host-reported claim the daemon could not check (`specs/113` §2 items 3, 4).
        // It belongs to no project, so it is listed nowhere and shared with none.
        if c["scope"] == "personal" {
            continue;
        }
        if c["scope"] == "project" {
            // `specs/115` §3 item 4: a project's claim is listed under its project, for that project's own search and, where the
            // operator shares research, for the others (`kb::allowed_sql` decides who reads it; nothing here does).
            if let Some(project) = claim_project(&projects, &c) {
                let dir = if lib.rests_on_internal(&c) { INTERNAL_DIR } else { CLAIMS_DIR };
                out.push(doc(format!("research/projects/{}/{dir}/{id}.json", project.as_str()), path));
            }
            continue;
        }
        if lib.rests_on_private(&c) {
            continue;
        }
        out.push(doc(format!("research/claims/{id}.json"), path));
    }
}

/// Under `research/projects/<id>/`: a project's claims, and apart from them those that rest on a private or internal source
/// (`Library::rests_on_internal`). The directory is the index's record of which is which: it is chosen here, by code, from the
/// library's own source records, and a reader that is given a row of [`INTERNAL_DIR`] marks its session (`specs/115` §2 item 3).
pub const CLAIMS_DIR: &str = "claims";
pub const INTERNAL_DIR: &str = "internal";

/// The project of THIS workspace a project-scoped claim belongs to, or `None`. The library is one per install and a claim names
/// its project by display name, which another workspace's project may share, so the name alone decides nothing: the claim must
/// also name a research topic whose record is on disk under that project's own `research/topics/`. A claim that names no such
/// topic (one the operator added by hand, one of another workspace) is not listed.
fn claim_project(projects: &[crate::scope::ProjectEntry], c: &Value) -> Option<crate::scope::ProjectId> {
    let name = c["project"].as_str().map(str::trim).filter(|n| !n.is_empty())?;
    let topics: Vec<&str> = c["topics"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let mut found = projects.iter().filter(|p| p.name.eq_ignore_ascii_case(name) && topics.iter().any(|t| crate::research_record::Topic::at(&p.root, t).is_some_and(|t| t.read().is_ok())));
    match (found.next(), found.next()) {
        (Some(p), None) => Some(p.id.clone()),
        _ => None,
    }
}

/// The Markdown the index holds for a record or a claim file. `None` when the file is not one (or cannot be read).
pub fn text_of(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if path.file_name().is_some_and(|n| n == "record.json") {
        record_text(&v)
    } else {
        // `specs/115` §11 item 5: whether an internal source states the claim is asked of the library the claim file lives in
        // (`<home>/library/claims/<id>.json`), from its own source records; a claim file anywhere else is never confirmed.
        let lib = path.parent().filter(|d| d.file_name().is_some_and(|n| n == "claims")).and_then(Path::parent).filter(|d| d.file_name().is_some_and(|n| n == "library")).and_then(Path::parent).map(crate::library::Library::in_home);
        let confirmed = lib.and_then(|l| l.internal_confirmation(&v));
        claim_text_with(&v, confirmed.as_deref())
    }
}

/// A library claim as the index holds it: the statement, its derived status, and where it is anchored. The frontmatter's
/// `claim_status` is the derived status; the index reads it for this zone only (`kb.rs`), and only as one of [`STATUSES`].
pub fn claim_text(c: &Value) -> Option<String> {
    claim_text_with(c, None)
}

/// [`claim_text`] for a claim an internal source confirms (`specs/115` §11 item 5): `confirmed` is the project's name the library gave
/// (`Library::internal_confirmation`). It goes in the frontmatter as `internal_confirmation`, which the index reads for a claim filed
/// under a project's `internal/` folder only, as a label; a disagreement (a contested mark) is said beside it.
pub fn claim_text_with(c: &Value, confirmed: Option<&str>) -> Option<String> {
    let (id, text) = (c["id"].as_str()?, c["text"].as_str()?);
    let status = crate::library::claims::status(c);
    let confirmation = confirmed.map(|p| format!("internal_confirmation: {}\n", yaml(p))).unwrap_or_default();
    let mut out = format!("---\ntitle: {}\nclaim: {}\nclaim_status: {}\n{confirmation}author: scout\nconfidence: low\n---\n\n# Claim: {}\n\n", yaml(text), clip(id, 40), status.as_str(), clip(text, LINE));
    out.push_str(&format!("- status: {} ({})\n", status.as_str(), status.label()));
    if let Some(p) = confirmed {
        out.push_str(&format!("- confirmed by an internal source ({}); it lends no weight to any other claim\n", clip(p, 80)));
        if c["contested_by"].as_array().is_some_and(|m| !m.is_empty()) {
            out.push_str("- a public source disagrees; the confirmation stands for this claim, and the operator sees both\n");
        }
    }
    out.push_str(&format!("- kind: {}; scope: {}\n", clip(c["kind"].as_str().unwrap_or("fact"), 20), clip(c["scope"].as_str().unwrap_or("team"), 20)));
    let topics: Vec<String> = c["topics"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(|t| clip(t, 60))).take(LIST).collect();
    if !topics.is_empty() {
        out.push_str(&format!("- research topics: {}\n", topics.join(", ")));
    }
    let anchors: Vec<&Value> = c["anchors"].as_array().into_iter().flatten().filter(|a| a["state"].as_str().unwrap_or("active") == "active").take(LIST).collect();
    if !anchors.is_empty() {
        out.push_str("\n## Where it is stated\n\n");
        for a in anchors {
            out.push_str(&format!("- {}: \"{}\" (source {})\n", clip(a["locator"].as_str().unwrap_or(""), LINE), clip(a["quote"].as_str().unwrap_or(""), 400), clip(a["source_id"].as_str().unwrap_or(""), 80)));
        }
    }
    Some(out)
}

/// A topic's record as the index holds it: the question, how far the research got, what was searched and read, and what it did
/// not find. Every line is the record's own data.
pub fn record_text(rec: &Value) -> Option<String> {
    let (id, question) = (rec["topic_id"].as_str()?, rec["question"].as_str()?);
    let n = |k: &str| rec[k].as_array().map_or(0, Vec::len);
    let mut out = format!("---\ntitle: {}\ntopic: {}\nauthor: scout\nconfidence: low\n---\n\n# Research topic: {}\n\n", yaml(question), clip(id, 60), clip(question, LINE));
    out.push_str(&format!("- topic: {}\n- state: {}; depth: {}; runs: {}\n", clip(id, 60), clip(rec["state"].as_str().unwrap_or("?"), 20), clip(rec["depth"].as_str().unwrap_or("?"), 20), n("runs")));
    out.push_str(&format!("- asked by: {} ({})\n", clip(rec["asked_by"]["label"].as_str().unwrap_or("?"), 120), clip(rec["asked_by"]["kind"].as_str().unwrap_or("?"), 20)));
    out.push_str(&format!("- queries: {}; pages read: {}; pages not read: {}; claims: {}\n", n("queries"), n("pages"), n("refused"), n("claims")));
    let claims: Vec<String> = rec["claims"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(|c| clip(c, 40))).take(LIST).collect();
    if !claims.is_empty() {
        out.push_str(&format!("- claim ids: {}\n", claims.join(", ")));
    }
    let section = |out: &mut String, title: &str, lines: Vec<String>| {
        if !lines.is_empty() {
            out.push_str(&format!("\n## {title}\n\n"));
            for l in lines {
                out.push_str(&format!("- {l}\n"));
            }
        }
    };
    section(&mut out, "What it did not find", rec["gaps"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(|g| clip(g, LINE))).take(LIST).collect());
    section(
        &mut out,
        "What was searched",
        rec["queries"].as_array().into_iter().flatten().take(LIST).map(|q| format!("[{}] \"{}\"", clip(q["purpose"].as_str().unwrap_or("?"), 40), clip(q["query"].as_str().unwrap_or(""), LINE))).collect(),
    );
    section(&mut out, "Pages read", rec["pages"].as_array().into_iter().flatten().take(LIST).map(|p| format!("{} (source {})", clip(p["url"].as_str().unwrap_or(""), LINE), clip(p["source_id"].as_str().unwrap_or("-"), 80))).collect());
    Some(out)
}

// ---- what a search shows of a research run (`specs/144` §2.6 items 2 and 3; slice B) ----------------------------------------------

/// What a search shows for a research topic's answer (§2.6 item 2): a model's account of what the run read, never evidence.
pub const ANSWER_LABEL: &str = "research summary · not evidence";

/// What a file of a research topic's folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunFile {
    /// `evidence-pack.md` (`evidence_pack`).
    Pack,
    /// `answer.md`.
    Answer,
    /// Any other file of the folder the index holds (the record, a project topic's other Markdown).
    Other,
}

/// A research topic's file as the index names it: the workspace's, `research/topics/<id>/<file>` in zone `research`, or a registered
/// project's, `projects/<project-id>/files/research/topics/<id>/<file>` in zone `project`. `(project id, topic id, kind)`.
pub fn run_file<'a>(zone: &str, rel_path: &'a str) -> Option<(Option<&'a str>, &'a str, RunFile)> {
    let p = rel_path.split('#').next().unwrap_or(rel_path);
    let (project, rest) = match zone {
        ZONE => (None, p.strip_prefix("research/topics/")?),
        "project" => {
            let (pid, r) = p.strip_prefix("projects/")?.split_once('/')?;
            (Some(pid), r.strip_prefix("files/research/topics/")?)
        }
        _ => return None,
    };
    let (id, file) = rest.split_once('/')?;
    if !crate::research_record::valid_topic_id(id) || file.is_empty() {
        return None;
    }
    let kind = match file {
        crate::evidence_pack::FILE => RunFile::Pack,
        "answer.md" => RunFile::Answer,
        _ => RunFile::Other,
    };
    Some((project, id, kind))
}

/// What a search may show of a research run's file, computed at the search: the label of every source its run read (an answer may
/// paraphrase what it does not quote), and whether that makes it untrusted text (a run that read held text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunShown {
    pub label: crate::label::Label,
    /// The run read held text: its file is `external_untrusted`, like the held text itself (§2.6 item 2).
    pub untrusted: bool,
}

/// What a search traces its results to (`specs/144` §2.6 item 3): the workspace and the library of the index it reads, told from the
/// index's own path (`<workspace>/local/state.db`), never from a result. An index anywhere else has no workspace: what it holds of a
/// research run cannot be traced, so it is shown nothing of one.
pub struct Trace {
    ws: Option<std::path::PathBuf>,
    lib: Option<crate::library::Library>,
}

impl Trace {
    /// The trace of the index whose database is `db`.
    pub fn of_db(db: Option<&Path>) -> Trace {
        let ws = db.and_then(|db| {
            let local = db.parent().filter(|p| p.file_name().is_some_and(|n| n == "local") && db.file_name().is_some_and(|n| n == "state.db"))?;
            local.parent().map(Path::to_path_buf)
        });
        Trace::of_ws(ws.as_deref())
    }

    /// The trace of the workspace `ws` (and its home's library).
    pub fn of_ws(ws: Option<&Path>) -> Trace {
        let lib = ws.and_then(|ws| crate::held_text::home_of_ws(ws).map(|h| crate::library::Library::in_home(h).with_workspace(ws)));
        Trace { ws: ws.map(Path::to_path_buf), lib }
    }

    /// The labeller over this trace's records. It reads the handling records once: make it when a result needs it, and once a search.
    pub fn labeller(&self) -> crate::label::Labeller<'_> {
        crate::label::Labeller::new(self.lib.as_ref(), self.ws.as_deref())
    }

    /// The topic a run file names, in its own folder: the workspace's, or the registered project's root. `None`: not found here.
    fn topic(&self, project: Option<&str>, id: &str) -> Option<crate::research_record::Topic> {
        let ws = self.ws.as_deref()?;
        let root = match project {
            None => ws.to_path_buf(),
            Some(pid) => crate::scope::project_root(ws, &crate::scope::ProjectId::parse(pid).ok()?)?,
        };
        crate::research_record::Topic::at(&root, id).filter(crate::research_record::Topic::exists)
    }

    /// What a search shows of the run file `(project, id, kind)` ([`run_file`]), from the records now: `None` when it is withheld,
    /// because a source its run read is private (`library_private`, `personal`) or because it cannot be traced (every mark).
    pub fn shown(&self, l: &crate::label::Labeller<'_>, project: Option<&str>, id: &str, kind: RunFile) -> Option<RunShown> {
        let topic = self.topic(project, id);
        let facts = match kind {
            RunFile::Pack => crate::evidence_pack::label_of_topic(l, id, topic.as_ref()),
            RunFile::Answer | RunFile::Other => crate::evidence_pack::run_label_of_topic(l, id, topic.as_ref()),
        };
        if crate::evidence_pack::withheld(&facts.label) {
            return None;
        }
        let untrusted = kind == RunFile::Pack || facts.label.has(crate::label::Mark::TextHeld);
        Some(RunShown { label: facts.label, untrusted })
    }

    /// Is the library source a row of the index names (`library/sources/<id>/…`: its text, excerpts or summary) private now? Asked of
    /// an index in a workspace only: the sync never lists a private source, and this keeps out one made private since the last sync.
    /// A row whose source the library no longer holds a record of (removed since the sync) is left to the next sync, which drops it.
    pub fn source_withheld(&self, l: &crate::label::Labeller<'_>, rel_path: &str) -> bool {
        let (Some(_), Some(lib)) = (&self.ws, &self.lib) else { return false };
        let Some(id) = rel_path.strip_prefix("library/sources/").and_then(|r| r.split('/').next()).filter(|id| !id.is_empty()) else { return false };
        lib.read_meta(id).is_some() && crate::evidence_pack::withheld(&l.source(id).label)
    }
}

/// A registered project's files (`mcp_project::scaffold_files`, by path under its root) without those of a research topic whose run is
/// withheld ([`Trace::shown`]): what a host's word-ranked project search, its `kb_retrieve` project leg and its resources may read.
pub fn visible_project_files(ws: &Path, root: &Path, files: Vec<(String, std::path::PathBuf)>) -> Vec<(String, std::path::PathBuf)> {
    if !files.iter().any(|(rel, _)| rel.starts_with("research/topics/")) {
        return files;
    }
    let trace = Trace::of_ws(Some(ws));
    let l = trace.labeller();
    let mut seen: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    files
        .into_iter()
        .filter(|(rel, _)| {
            let Some(rest) = rel.strip_prefix("research/topics/") else { return true };
            let Some((id, _)) = rest.split_once('/').filter(|(id, _)| crate::research_record::valid_topic_id(id)) else { return true };
            *seen.entry(id.to_string()).or_insert_with(|| {
                let topic = crate::research_record::Topic::at(root, id).filter(crate::research_record::Topic::exists);
                !crate::evidence_pack::withheld(&crate::evidence_pack::run_label_of_topic(&l, id, topic.as_ref()).label)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kb::{Kb, SearchOpts};
    use crate::library::claims::{AnchorSpec, ClaimOpts, ResearchClaim};
    use crate::library::{AddOpts, Budgets, Level, Library};
    use crate::research_record::{Asker, Begin, Topic};
    use serde_json::json;

    struct Fx {
        _root: tempfile::TempDir,
        home: std::path::PathBuf,
        ws: std::path::PathBuf,
    }

    /// An install home with one workspace under it, as the index expects (`<home>/workspaces/<name>`).
    fn fx() -> Fx {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().to_path_buf();
        let ws = home.join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        Fx { _root: root, home, ws }
    }

    fn source(lib: &Library, dir: &Path, name: &str, text: &str, private: bool) -> String {
        let f = dir.join(format!("{name}.md"));
        std::fs::write(&f, text).unwrap();
        lib.add_file(&f, &AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Full), private, ..Default::default() }, &Budgets::default()).unwrap().id
    }

    fn research_claim(lib: &Library, text: &str, fp: &str, project: Option<&str>, topic: Option<&str>, source: &str, quote: &str) -> String {
        let anchors = [AnchorSpec { source_id: source.into(), locator: Some("https://example.org/page".into()), quote: quote.into(), prefix: None, suffix: None, read: None }];
        lib.add_research_claim(&ResearchClaim { text, fingerprint: fp, project, topic, anchors: &anchors }).unwrap().0["id"].as_str().unwrap().to_string()
    }

    fn finished_topic(ws: &Path, question: &str, answer: &str, claims: &[String], private: bool) -> Topic {
        let t = Topic::begin(ws, &Begin { question, asked_by: Asker::host("host:claude-code/m"), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-t-1" }).unwrap();
        t.query("r-t-1", "zebrafish husbandry temperature", "topic", Some("brave"), Ok(2)).unwrap();
        // The page it read is a source the library holds (`specs/144` §2.6 item 3: a run whose sources cannot be traced is withheld).
        let home = ws.parent().and_then(Path::parent).unwrap();
        let read = source(&Library::in_home(home), home, &format!("page-{}", crate::research_record::topic_id(question)), "A page the run read.", false);
        t.page("r-t-1", "https://example.org/page", &read, Some("sha256:00")).unwrap();
        let result = json!({"answer": answer, "budget_used": {"steps": 2, "check_steps": 1}, "coverage": "thin", "coverage_report": {"thin_below": 3, "counted": 1, "rungs": []}, "search_log": {"version": 1, "private": private, "lines": [], "flow": {}}});
        t.finish("r-t-1", "done", Some(&result), claims, None, 5).unwrap();
        t
    }

    fn search(f: &Fx, query: &str, opts: SearchOpts) -> Vec<crate::kb::Hit> {
        let mut kb = Kb::open(&f.ws.join("local/state.db")).unwrap();
        kb.refresh(&f.ws, "").unwrap();
        kb.search(query, &SearchOpts { rerank_weight: 0.0, ..opts }).unwrap()
    }

    fn zone() -> SearchOpts {
        SearchOpts { zones: Some(vec![ZONE]), limit: 20, ..Default::default() }
    }

    /// AC-2, the index half: after a run that kept claims, a search returns the claim labelled `noted`, the topic's record and its
    /// answer, all in zone `research` at the library's tier.
    #[test]
    fn a_kept_claim_its_topic_and_its_answer_are_found_in_zone_research_and_the_claim_reads_noted() {
        let f = fx();
        let lib = Library::in_home(&f.home);
        let s = source(&lib, &f.home, "tanks", "Zebrafish tanks are held at 28 degrees, the husbandry guide says.", false);
        let t = finished_topic(&f.ws, "What temperature do zebrafish tanks need?", "What the sources read say, claim by claim (not validated):\n- zebrafish tanks need 28 degrees (\"tanks are held at 28 degrees\")\n\nlist of claims (no synthesis)", &[], false);
        let id = research_claim(&lib, "zebrafish tanks need 28 degrees", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None, Some(&t.id), &s, "tanks are held at 28 degrees");
        let hits = search(&f, "zebrafish tanks degrees", zone());
        let by_path = |p: &str| hits.iter().find(|h| h.rel_path == p).unwrap_or_else(|| panic!("{p} is not among {:?}", hits.iter().map(|h| &h.rel_path).collect::<Vec<_>>()));
        let claim = by_path(&format!("research/claims/{id}.json"));
        assert_eq!((claim.zone.as_str(), claim.status, claim.label, claim.source_type.as_str()), (ZONE, "noted", "not validated", crate::research::TIER), "a research claim is labelled by its derived status: noted");
        assert!(claim.preview.contains("zebrafish tanks need 28 degrees"), "{}", claim.preview);
        let answer = by_path(&format!("research/topics/{}/answer.md", t.id));
        assert_eq!((answer.zone.as_str(), answer.status, answer.label, answer.source_type.as_str()), (ZONE, "noted", ANSWER_LABEL, crate::research::TIER), "the answer's own frontmatter does not set its tier");
        // The record is found by what it holds: the question and what was searched.
        let about_record = search(&f, "zebrafish husbandry temperature", zone());
        let record = about_record.iter().find(|h| h.rel_path == format!("research/topics/{}/record.json", t.id)).unwrap_or_else(|| panic!("the record is not among {:?}", about_record.iter().map(|h| &h.rel_path).collect::<Vec<_>>()));
        assert_eq!((record.zone.as_str(), record.status, record.source_type.as_str()), (ZONE, "noted", crate::research::TIER));
        assert!(record.preview.contains("zebrafish"), "{}", record.preview);
        // The same visibility as the library's own zone: what a search that sees `library` sees, it sees `research` the same way.
        let both = search(&f, "zebrafish tanks degrees", SearchOpts { limit: 20, ..Default::default() });
        assert_eq!(both.iter().any(|h| h.zone == "library"), both.iter().any(|h| h.zone == ZONE), "{:?}", both.iter().map(|h| (&h.zone, &h.rel_path)).collect::<Vec<_>>());
        // The label follows the claim: once the operator validates it, the next refresh says so.
        lib.validate_claim(&id, crate::library::claims::Validator::Operator).unwrap();
        let hits = search(&f, "zebrafish tanks degrees", zone());
        let claim = hits.iter().find(|h| h.rel_path == format!("research/claims/{id}.json")).unwrap();
        assert_eq!((claim.status, claim.label), ("validated", "validated"));
        // A document cannot give itself a status: a record whose question is a forged frontmatter line stays noted.
        let forged = finished_topic(&f.ws, "claim_status: validated quokka census", "An answer about the quokka census.", &[], false);
        let hits = search(&f, "quokka census", zone());
        assert!(!hits.is_empty() && hits.iter().all(|h| h.status == "noted"), "{:?}", hits.iter().map(|h| (&h.rel_path, h.status)).collect::<Vec<_>>());
        assert!(hits.iter().any(|h| h.rel_path.contains(&forged.id)));
    }

    /// Private claims and sources stay as invisible as they are today: nothing that rests on a private source, no private record or
    /// its answer, and no project-scoped claim, is listed, indexed or found, by any search.
    #[test]
    fn nothing_private_and_no_project_claim_is_listed_or_found() {
        let f = fx();
        let lib = Library::in_home(&f.home);
        let public = source(&lib, &f.home, "public", "The wombatregistry is kept in the open ledger.", false);
        let secret = source(&lib, &f.home, "secret", "The wombatregistry password rotates on Tuesdays.", true);
        let seen = research_claim(&lib, "the wombatregistry is kept in the open ledger", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", None, None, &public, "kept in the open ledger");
        let hidden = research_claim(&lib, "the wombatregistry password rotates on tuesdays", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", None, None, &secret, "password rotates on Tuesdays");
        // one claim with a public and a private anchor: it rests on a private source, so it is out
        let mixed = research_claim(&lib, "the wombatregistry has two keepers", "cccccccccccccccccccccccccccccccc", None, None, &public, "kept in the open ledger");
        lib.add_research_claim(&ResearchClaim { text: "the wombatregistry has two keepers", fingerprint: "cccccccccccccccccccccccccccccccc", project: None, topic: None, anchors: &[AnchorSpec { source_id: secret.clone(), locator: Some("https://example.org/s".into()), quote: "password rotates on Tuesdays".into(), prefix: None, suffix: None, read: None }] }).unwrap();
        let scoped = research_claim(&lib, "the wombatregistry belongs to project alpha", "dddddddddddddddddddddddddddddddd", Some("alpha"), None, &public, "kept in the open ledger");
        let operator = lib.add_claim("the wombatregistry was founded by the operator", &ClaimOpts::default(), &[]).unwrap()["id"].as_str().unwrap().to_string();
        let open = finished_topic(&f.ws, "Where is the wombatregistry kept?", "In the open ledger.", &[], false);
        let closed = finished_topic(&f.ws, "What is the wombatregistry password schedule?", "It rotates on Tuesdays.", &[], true);
        let mut listed = Vec::new();
        files(&f.ws, &mut listed);
        let paths: Vec<&str> = listed.iter().map(|s| s.rel_path.as_str()).collect();
        for id in [&seen, &operator] {
            assert!(paths.contains(&format!("research/claims/{id}.json").as_str()), "{id}: {paths:?}");
        }
        for id in [&hidden, &mixed, &scoped] {
            assert!(!paths.iter().any(|p| p.contains(id.as_str())), "{id} must not be listed: {paths:?}");
        }
        assert!(paths.iter().any(|p| p.contains(&open.id)) && !paths.iter().any(|p| p.contains(&closed.id)), "{paths:?}");
        assert!(listed.iter().all(|s| s.zone == ZONE && s.default_source == crate::research::TIER && s.vouched_by.is_none()));
        // And by search, with every door open that a caller can open.
        let wide = SearchOpts { limit: 50, include_untrusted: true, include_staged: true, include_attributed: true, ..Default::default() };
        let hits = search(&f, "wombatregistry password rotates Tuesdays keepers alpha", wide);
        let text: String = hits.iter().map(|h| format!("{} {}\n", h.rel_path, h.preview)).collect();
        for needle in ["password", "Tuesdays", "two keepers", "project alpha", hidden.as_str(), mixed.as_str(), scoped.as_str(), closed.id.as_str()] {
            assert!(!text.contains(needle), "`{needle}` reached a search:\n{text}");
        }
        assert!(text.contains("open ledger"), "{text}");
        // A claim that comes to rest on a private source leaves the index at the next refresh.
        lib.add_research_claim(&ResearchClaim { text: "the wombatregistry is kept in the open ledger", fingerprint: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", project: None, topic: None, anchors: &[AnchorSpec { source_id: secret, locator: Some("https://example.org/s".into()), quote: "password rotates on Tuesdays".into(), prefix: None, suffix: None, read: None }] }).unwrap();
        let hits = search(&f, "wombatregistry open ledger", zone());
        assert!(!hits.iter().any(|h| h.rel_path.contains(seen.as_str())), "{:?}", hits.iter().map(|h| &h.rel_path).collect::<Vec<_>>());
    }

    /// `specs/116` §4 item 3: the zone is among those a span starts from. A mission whose objective matches a topic's answer is
    /// given a passage of it, with its pointer and its label, and starts at the library's tier as it would for library text. A
    /// private topic's answer and a claim that rests on a private source are not in the zone, so neither arrives.
    #[test]
    fn a_span_starts_with_a_labelled_passage_of_a_matching_topics_answer_and_nothing_of_a_private_one() {
        use crate::retrieval::{span_start, StartBudget, START_ZONES};
        use delphi_core::provenance::Tier;
        assert!(START_ZONES.contains(&ZONE));
        let f = fx();
        let lib = Library::in_home(&f.home);
        let open = finished_topic(&f.ws, "Where do numbats shelter overnight?", "Numbats shelter overnight in hollow logs lined with shredded bark.", &[], false);
        let closed = finished_topic(&f.ws, "What is the code of the gate where numbats shelter overnight?", "Numbats shelter overnight in hollow logs behind the gate whose code is 4417.", &[], true);
        let secret = source(&lib, &f.home, "keeper", "Numbats shelter overnight in hollow logs that the keeper locks with key 9931.", true);
        let hidden = research_claim(&lib, "numbats shelter overnight in hollow logs locked with key 9931", "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", None, None, &secret, "the keeper locks with key 9931");
        let db = f.ws.join("local/state.db");
        Kb::open(&db).unwrap().refresh(&f.ws, "").unwrap();
        let objective = "Numbats shelter overnight in hollow logs";
        let start = span_start(&f.home, &db, "m-numbat", objective, "builder", StartBudget::default(), false).expect("the answer is found at the span's start");
        let line = start.text.lines().find(|l| l.contains(&format!("research/topics/{}/answer.md#L", open.id))).unwrap_or_else(|| panic!("no passage of the answer:\n{}", start.text));
        assert!(line.starts_with("- research/topics/") && line.contains(&format!(" [{ANSWER_LABEL}]: ")) && line.contains("hollow logs lined with shredded bark"), "its pointer, its label, its text: {line}");
        assert_eq!(start.tier, Tier::L2, "the span starts at the library's tier ({}), the lowest among its passages", crate::research::TIER);
        assert_eq!(crate::retrieval::tier_of(crate::research::TIER), Tier::L2);
        for needle in ["4417", "9931", closed.id.as_str(), hidden.as_str()] {
            assert!(!start.text.contains(needle), "`{needle}` reached a span's start:\n{}", start.text);
        }
        assert!(start.provisional.is_empty());
        // Every role is given it under the same label; a root span asks once per mission.
        let again = span_start(&f.home, &db, "m-numbat-2", objective, "architect", StartBudget::default(), true).expect("a root span");
        assert!(again.text.contains(&format!("research/topics/{}/answer.md#L", open.id)) && again.text.contains(&format!("[{ANSWER_LABEL}]")) && !again.text.contains("4417"), "{}", again.text);
    }

    #[test]
    fn the_text_the_index_holds_is_built_from_the_files_own_fields_and_carries_no_control_characters() {
        let c = json!({"id": "c-0123456789ab", "text": "aspirin lowers fever\u{1b}[31m", "kind": "fact", "scope": "team", "topics": ["does-aspirin-lower-fever-deadbeef"], "support": {"origins": 2, "trusted_sources": 1},
            "anchors": [{"source_id": "s-1", "locator": "https://who.int/a", "quote": "lowers fever\nin adults", "state": "active"}, {"source_id": "s-2", "locator": "https://x.example/b", "quote": "gone", "state": "lost"}]});
        let t = claim_text(&c).unwrap();
        assert!(t.starts_with("---\ntitle: \"aspirin lowers fever [31m\"\nclaim: c-0123456789ab\nclaim_status: noted\n"), "{t}");
        assert!(t.contains("- status: noted (not validated)") && t.contains("- research topics: does-aspirin-lower-fever-deadbeef"), "{t}");
        assert!(!t.contains("origins"), "the count of origins is the library's to show; the index text carries the status and no count: {t}");
        assert!(t.contains("- https://who.int/a: \"lowers fever in adults\" (source s-1)") && !t.contains("gone"), "an anchor that was lost is not shown: {t}");
        assert!(!t.contains('\u{1b}'));
        assert!(claim_text(&json!({"id": "c-1"})).is_none() && record_text(&json!({"question": "q"})).is_none());
        let r = json!({"topic_id": "q-deadbeef", "question": "A question?", "state": "partial", "depth": "survey", "asked_by": {"kind": "host", "label": "host:x/y"}, "runs": [{}], "claims": ["c-1"], "gaps": ["kb: kb_empty"],
            "queries": [{"purpose": "trusted", "query": "a question site:who.int"}], "pages": [{"url": "https://who.int/a", "source_id": "s-1"}], "refused": []});
        let t = record_text(&r).unwrap();
        for needle in ["# Research topic: A question?", "- state: partial; depth: survey; runs: 1", "- claim ids: c-1", "## What it did not find", "- kb: kb_empty", "- [trusted] \"a question site:who.int\"", "- https://who.int/a (source s-1)"] {
            assert!(t.contains(needle), "{needle}\n{t}");
        }
        // The stored word is read back as the status it names, and anything else is noted.
        for w in STATUSES {
            assert_eq!(status_of(Some(w)).as_str(), w);
        }
        for w in [None, Some(""), Some("ratified"), Some("VALIDATED"), Some("validated ")] {
            assert_eq!(status_of(w).as_str(), "noted", "{w:?}");
        }
        assert!(is_derived("research/claims/c-1.json") && is_derived("research/topics/q-deadbeef/record.json"));
        assert!(!is_derived("research/topics/q-deadbeef/answer.md") && !is_derived("context/data.json") && !is_derived("library/claims/c-1.json"));
    }
}
