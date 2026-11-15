//! Research recall across projects (`specs/115`): what a session working in one project of a workspace may read of the others.
//!
//! **This widens what a session may read, on the operator's decision (`specs/115` §8), and nothing else.** It adds no action, raises
//! no tier and writes nothing. The whole of the widening is here and in the one filter of the index (`kb::allowed_sql`):
//!
//! * [`view`] decides whether anything is shared and from whom: only when `[projects] research_sharing` is `workspace`
//!   (`scope::research_sharing`, read from the file at each call), only for a session inside a registered project, and only from the
//!   projects of the same workspace's list that are not marked private (`scope::project_entries`). A list or a setting that cannot
//!   be read shares nothing.
//! * What is shared is research and nothing else of a project: its `research/**` files (`projects/<id>/files/research/…` in the
//!   index), the library claims its research kept (`research/projects/<id>/…`, `research_index::files`), and its findings. Specs,
//!   plans, context, work output, memory and staged notes are not admitted by the filter under any setting.
//! * Every shared result names its project, says `shared`, and is labelled [`SHARED_LABEL`] unless its claim status says more.
//! * A claim that rests on a private or internal source crosses under three guards (`specs/115` §2 item 3): it is labelled
//!   [`INTERNAL_LABEL`]; its source is never an origin for corroboration (`Library::cites_of`); and the session that is given it
//!   takes the private-read mark ([`mark_entry`], `specs/103`). The source's own record and text are not in the index at all.
//! * A shared result that names one of the workspace's `[privacy] identifiers` is withheld and counted.
//!
//!
//! [`host_search`] is the one reader built so far: the `project_research` group of `host.search_kb`, which also returns the
//! session's own project's claims (a project reads its own on the same listing). An agent's `kb_search`, the passages a span
//! starts with, `kb_retrieve`, `get_agent`'s task section and the claim pull do not ask for shared research: they set no
//! `SearchOpts::shared`, so they are as isolated as before (`specs/115` slice X4 adds the `scope` parameter and the claim pull).
//! Only a session inside a registered project is given another project's research; a home-layer or default-project session is not.

use crate::kb::{FindingsView, Hit, SearchOpts, SharedResearch};
use crate::scope::{self, ProjectId, ResearchSharing};
use serde_json::{json, Value};
use std::path::Path;

/// The label of a shared result whose claim status says no more (`specs/115` §3 item 2).
pub const SHARED_LABEL: &str = "another project's research, not checked";
/// The label of a shared claim that rests on a private or internal source (`specs/115` §2 item 3). It wins over every status.
pub const INTERNAL_LABEL: &str = "another project's research, from an internal source, host-reported";

/// The label of a claim of the session's OWN project that rests on a private or internal source. It marks the session as well.
pub const OWN_INTERNAL_LABEL: &str = "this project's research, from an internal source, host-reported";

/// `[projects] own_project_boost` (`specs/115` §3 item 3): what is added to the score of a result of the session's own project, so
/// it ranks ahead of a shared one at equal score. Small against one rank of the fusion (about 0.016 for a first place).
pub const DEFAULT_OWN_BOOST: f64 = 0.001;
pub const MAX_OWN_BOOST: f64 = 0.1;

/// The workspace's boost, read from `config.toml`; the default when the key is absent or is not a number in `0..=MAX_OWN_BOOST`.
pub fn own_boost(ws_dir: &Path) -> f64 {
    let set = std::fs::read_to_string(ws_dir.join("config.toml")).ok().and_then(|t| toml::from_str::<toml::Value>(&t).ok()).and_then(|v| v.get("projects").and_then(|p| p.get("own_project_boost")).cloned());
    match set.and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))) {
        Some(b) if b.is_finite() && (0.0..=MAX_OWN_BOOST).contains(&b) => b,
        _ => DEFAULT_OWN_BOOST,
    }
}

/// A project's display name as a result carries it: one line, no control characters, at most 80 characters.
fn shown_name(name: &str) -> String {
    name.chars().map(|c| if c.is_control() { ' ' } else { c }).take(80).collect::<String>().trim().to_string()
}

/// What a session of project `own` may read of the workspace's other projects, or `None` when nothing is shared with it: the
/// setting is `project`, `own` is not a registered project of this workspace (the default project and a scope that failed closed
/// have no project), or no other project is left once the private ones are taken out. A private project still reads what the
/// others share (`specs/115` §2 item 2); it is only never in another's list.
pub fn view(ws_dir: &Path, own: &ProjectId) -> Option<SharedResearch> {
    if own.as_str() == scope::DEFAULT_PROJECT || scope::research_sharing(ws_dir) != ResearchSharing::Workspace {
        return None;
    }
    let entries = scope::project_entries(ws_dir);
    if !entries.iter().any(|e| e.id == *own) {
        return None;
    }
    let projects: Vec<(String, String)> = entries.into_iter().filter(|e| !e.private && e.id != *own).map(|e| (e.id.as_str().to_string(), shown_name(&e.name))).collect();
    if projects.is_empty() {
        return None;
    }
    Some(SharedResearch { projects, only: false, own_boost: own_boost(ws_dir), identifiers: crate::query_guard::identifiers(ws_dir) })
}

/// [`view`] for the project the folder `cwd` belongs to, with that project's id.
pub fn view_at(ws_dir: &Path, cwd: &Path) -> Option<(ProjectId, SharedResearch)> {
    let own = scope::resolve(ws_dir, cwd).project?;
    view(ws_dir, &own).map(|v| (own, v))
}

/// The private-read entry a session is marked with when it is given a shared claim that rests on an internal source: the claim's
/// id, never its text. `None` for any other path.
pub fn mark_entry(rel_path: &str) -> Option<String> {
    let id = rel_path.strip_prefix("research/projects/")?.split('/').nth(2)?.strip_suffix(".json")?;
    (rel_path.split('/').nth(3) == Some(crate::research_index::INTERNAL_DIR) && !id.is_empty()).then(|| format!("shared-claim/{id}"))
}

/// What `host.search_kb` adds for a host working in a project (`specs/115` §3).
#[derive(Debug, Default)]
pub struct HostFound {
    /// The `project_research` group: the project's own claims (`shared: false`), and each result of another project with
    /// `project: <display name>`, `shared: true` and its label.
    pub results: Vec<Value>,
    /// How many shared results were withheld because they name one of the workspace's privacy identifiers.
    pub withheld: usize,
    /// The private-read entries of the internal-source claims among `results` ([`mark_entry`]): the caller marks the session.
    pub internal: Vec<String>,
}

/// The project research a host whose folder is `cwd` may read, for `host.search_kb`: the claims its own project's research kept
/// (under either setting: a project reads its own), and, where the operator shares research ([`view`]), the other projects'
/// `research/**` files, claims and findings. Nothing else of any project. Empty for a folder in no registered project, or when
/// the index cannot be read. The preview is the index's own three lines, fenced under the result's label: this is data, never
/// instructions.
pub fn host_search(ws_dir: &Path, home: &Path, db: &Path, cwd: &Path, query: &str, limit: usize) -> HostFound {
    let Some(own) = scope::resolve(ws_dir, cwd).project.filter(|p| p.as_str() != scope::DEFAULT_PROJECT && scope::project_root(ws_dir, p).is_some()) else { return HostFound::default() };
    let shared = SharedResearch { only: true, ..view(ws_dir, &own).unwrap_or_default() };
    let opts = SearchOpts {
        role: None,
        limit: (limit * 2).min(10),
        include_untrusted: true,
        zones: Some(vec!["project", crate::research_index::ZONE]),
        project: Some(own.as_str().to_string()),
        findings: FindingsView::Scope(crate::finding_record::scope_word(Some(own.as_str()))),
        shared: Some(shared),
        ..Default::default()
    };
    // Two searches, one answer. Files and claims first: every row they may return is in a project's research scope, so the vector
    // leg takes its nearest among exactly those (`specs/115` §3 item 7). Findings apart: their project is the mission's stamp, so
    // they are judged after the cut and must not bring the home layer's vectors into the first search's candidates.
    let Ok(mut found) = crate::retrieval::search(home, db, query, opts.clone()) else { return HostFound::default() };
    if let Ok(findings) = crate::retrieval::search(home, db, query, SearchOpts { zones: Some(vec![crate::finding_record::ZONE]), ..opts }) {
        found.withheld += findings.withheld;
        found.hits.extend(findings.hits);
        found.hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    }
    let mut out = HostFound { withheld: found.withheld, ..Default::default() };
    // Kept here as well as in the filter, so a change to the filter cannot widen this answer by itself: a row of another project
    // only when the index says it is shared, and of this project only its own claims. A superseded or draft document is not offered.
    let own_claims = format!("research/projects/{}/", own.as_str());
    let mut seen: Vec<&str> = Vec::new();
    for h in found.hits.iter().filter(|h| (h.shared || h.rel_path.starts_with(&own_claims)) && h.doc_status == "active") {
        // One result a document: its best passage.
        if out.results.len() >= limit || seen.contains(&h.rel_path.as_str()) {
            continue;
        }
        seen.push(&h.rel_path);
        out.results.push(host_result(ws_dir, h));
        if let Some(entry) = mark_entry(&h.rel_path).filter(|_| h.internal) {
            if !out.internal.contains(&entry) {
                out.internal.push(entry);
            }
        }
    }
    out
}

fn host_result(ws_dir: &Path, h: &Hit) -> Value {
    let l = crate::host_labels::of_hit(ws_dir, h);
    // The path inside the other project (`research/…`), without the index's own prefix.
    let path = h.rel_path.strip_prefix("projects/").and_then(|r| r.split_once("/files/")).map(|(_, p)| p.to_string()).unwrap_or_else(|| h.rel_path.clone());
    let mut v = json!({
        "layer": "project_research",
        "project": h.project,
        "shared": h.shared,
        "pointer": crate::library::host_pointer(&h.pointer),
        "path": path,
        "heading": h.heading_path,
        "preview": crate::host_noted::fence(&l.label, crate::mcp_wire::cut_at_char(&h.preview, crate::host_labels::THIRD_PARTY_PREVIEW)),
    });
    if h.internal {
        v["internal_source"] = json!(true);
    }
    if let Some(o) = v.as_object_mut() {
        l.into_fields(o);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kb::Kb;
    use crate::library::claims::{AnchorSpec, ResearchClaim};
    use crate::library::{AddOpts, Budgets, Level, Library};
    use crate::research_record::{Asker, Begin, Topic};
    use std::path::PathBuf;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const P: &str = "cccccccccccccccccccccccccccccccc";

    /// A workspace with three registered projects: `alpha` (A), `beta` (B) and `hidden` (P). Each has a research file, a spec, a
    /// plan, a context file, work output, layer knowledge, layer memory and a staged note, all holding the word `quokkatide`.
    struct Fx {
        _root: tempfile::TempDir,
        home: PathBuf,
        ws: PathBuf,
        a: PathBuf,
        b: PathBuf,
        p: PathBuf,
    }

    fn put(path: PathBuf, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn register(f: &Fx, private: &[&str]) {
        let entry = |id: &str, root: &Path, name: &str| json!({"project_id": id, "root": root, "display_name": name, "private": private.contains(&id)});
        put(f.ws.join("local/projects.json"), &json!({"schema": 1, "projects": {A: entry(A, &f.a, "alpha"), B: entry(B, &f.b, "beta"), P: entry(P, &f.p, "hidden")}}).to_string());
    }

    fn fx() -> Fx {
        let root = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(root.path()).unwrap();
        let ws = home.join("workspaces/w");
        let f = Fx { ws, a: home.join("pa"), b: home.join("pb"), p: home.join("pp"), home, _root: root };
        for (id, dir, name) in [(A, &f.a, "alpha"), (B, &f.b, "beta"), (P, &f.p, "hidden")] {
            put(dir.join("research/notes.md"), &format!("# Research notes of {name}\n\nThe quokkatide survey of {name} found a spring range of four metres.\n"));
            put(dir.join("specs/s1.md"), &format!("---\nstatus: approved\n---\n# Spec of {name}\n\nquokkatide spec text of {name}.\n"));
            put(dir.join("plans/p1.md"), &format!("# Plan of {name}\n\nquokkatide plan text of {name}.\n"));
            put(dir.join("context/c1.md"), &format!("# Context of {name}\n\nquokkatide context text of {name}.\n"));
            put(dir.join("work/out.md"), &format!("# Work of {name}\n\nquokkatide work output of {name}.\n"));
            put(dir.join("_index.md"), &format!("# {name}\n\nquokkatide overview of {name}.\n"));
            let layer = f.ws.join("local/projects").join(id);
            put(layer.join("knowledge/k1.md"), &format!("# Layer knowledge of {name}\n\nquokkatide layer knowledge of {name}.\n"));
            put(layer.join("context/lc.md"), &format!("# Layer context of {name}\n\nquokkatide layer context of {name}.\n"));
            put(layer.join("memory/agents/builder/lessons.md"), &format!("# Memory of {name}\n\nquokkatide memory of {name}.\n"));
            put(layer.join("staged/host-abcd1234/n1.md"), &format!("# Staged of {name}\n\nquokkatide staged note of {name}.\n"));
        }
        register(&f, &[P]);
        f
    }

    fn sync(f: &Fx) {
        let mut kb = Kb::open(&f.ws.join("local/state.db")).unwrap();
        kb.refresh(&f.ws, "").unwrap();
    }

    /// The whole index as a session of project `own` would be given it, with every door a caller can open: untrusted text, staged
    /// notes, attributed records. Asked zone by zone, so nothing is cut off by the ten-result limit before it is looked at.
    fn search_as(f: &Fx, own: &str, query: &str) -> crate::kb::Found {
        sync(f);
        let kb = Kb::open_readonly(&f.ws.join("local/state.db")).unwrap();
        let shared = view(&f.ws, &ProjectId::parse(own).unwrap());
        let opts = SearchOpts { limit: 10, include_untrusted: true, include_staged: true, include_attributed: true, rerank_weight: 0.0, project: Some(own.into()), findings: FindingsView::Scope(own.into()), shared, ..Default::default() };
        let mut all = crate::kb::Found::default();
        for zone in ["project", "research", "findings", "memory", "staged", "knowledge", "context", "specs", "missions"] {
            let found = kb.search_found(query, &SearchOpts { zones: Some(vec![zone]), ..opts.clone() }).unwrap();
            all.withheld += found.withheld;
            all.hits.extend(found.hits);
        }
        all
    }

    fn paths(found: &crate::kb::Found) -> Vec<&str> {
        found.hits.iter().map(|h| h.rel_path.as_str()).collect()
    }

    fn set_sharing(f: &Fx, word: &str) {
        put(f.ws.join("config.toml"), &format!("[projects]\nresearch_sharing = \"{word}\"\n"));
    }

    fn source(f: &Fx, name: &str, text: &str, private: bool) -> String {
        let file = f.home.join(format!("{name}.md"));
        std::fs::write(&file, text).unwrap();
        Library::in_home(&f.home).add_file(&file, &AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Full), private, ..Default::default() }, &Budgets::default()).unwrap().id
    }

    /// A finished research topic in the folder of the project named `project`, with its record and answer on disk.
    fn topic(root: &Path, project: &str, question: &str, answer: &str, private: bool) -> Topic {
        let t = Topic::begin(root, &Begin { question, asked_by: Asker::host("host:claude-code/m"), project: Some(project), depth: delphi_core::research::Depth::Quick, run_id: "r-t-1" }).unwrap();
        let result = json!({"answer": answer, "budget_used": {"steps": 2, "check_steps": 1}, "coverage": "thin", "coverage_report": {"thin_below": 3, "counted": 1, "rungs": []}, "search_log": {"version": 1, "private": private, "lines": [], "flow": {}}});
        t.finish("r-t-1", "done", Some(&result), &[], None, 5).unwrap();
        t
    }

    fn claim(f: &Fx, text: &str, fp: &str, project: Option<&str>, topic: Option<&str>, source: &str, quote: &str) -> String {
        let anchors = [AnchorSpec { source_id: source.into(), locator: Some("https://example.org/page".into()), quote: quote.into(), prefix: None, suffix: None }];
        Library::in_home(&f.home).add_research_claim(&ResearchClaim { text, fingerprint: fp, project, topic, anchors: &anchors }).unwrap().0["id"].as_str().unwrap().to_string()
    }

    /// AC-1: with `research_sharing = "workspace"` (the default), a search from project B returns a research file of project A,
    /// with A's name, `shared` and the shared label; B's own results are not marked shared.
    #[test]
    fn with_sharing_on_a_search_from_b_returns_as_research_file_with_as_name_and_the_shared_label() {
        let f = fx();
        let found = search_as(&f, B, "quokkatide survey spring range");
        let theirs = found.hits.iter().find(|h| h.rel_path == format!("projects/{A}/files/research/notes.md")).unwrap_or_else(|| panic!("A's research file is not among {:?}", paths(&found)));
        assert_eq!((theirs.project.as_deref(), theirs.shared, theirs.label, theirs.internal), (Some("alpha"), true, SHARED_LABEL, false));
        assert_eq!((theirs.zone.as_str(), theirs.source_type.as_str()), ("project", "external_untrusted"), "no trust is raised: it reads as untrusted project text");
        let own = found.hits.iter().find(|h| h.rel_path == format!("projects/{B}/files/research/notes.md")).expect("B's own research file");
        assert_eq!((own.project.as_deref(), own.shared), (None, false));
        assert_ne!(own.label, SHARED_LABEL);
        assert_eq!(crate::host_labels::of_hit(&f.ws, theirs).label, SHARED_LABEL, "the host's label says the same");
    }

    /// AC-2: with A private the same search returns nothing of A's; with the setting `project`, likewise; and a private project
    /// still reads what the others share. The flag and the setting take effect at the next search: nothing is re-indexed.
    #[test]
    fn a_private_project_shares_nothing_and_the_project_setting_shares_nothing_and_both_take_effect_at_the_next_search() {
        let f = fx();
        let of = |found: &crate::kb::Found, id: &str| found.hits.iter().filter(|h| h.rel_path.contains(id)).map(|h| h.rel_path.clone()).collect::<Vec<_>>();
        // P is private from the start: nothing of it reaches B or A.
        for own in [A, B] {
            let found = search_as(&f, own, "quokkatide");
            assert!(of(&found, P).is_empty(), "{own}: {:?}", of(&found, P));
        }
        // P's own session reads A's and B's research.
        let from_p = search_as(&f, P, "quokkatide");
        assert_eq!(of(&from_p, A), [format!("projects/{A}/files/research/notes.md")]);
        assert_eq!(of(&from_p, B), [format!("projects/{B}/files/research/notes.md")]);
        // A is marked private: B's next search has nothing of A's.
        assert!(!of(&search_as(&f, B, "quokkatide"), A).is_empty());
        register(&f, &[P, A]);
        assert!(of(&search_as(&f, B, "quokkatide"), A).is_empty());
        register(&f, &[P]);
        assert!(!of(&search_as(&f, B, "quokkatide"), A).is_empty(), "cleared, it is shared again");
        // The setting `project`: the behaviour before this spec.
        set_sharing(&f, "project");
        let found = search_as(&f, B, "quokkatide");
        assert!(of(&found, A).is_empty() && of(&found, P).is_empty() && !of(&found, B).is_empty(), "{:?}", paths(&found));
        assert!(found.hits.iter().all(|h| !h.shared && h.project.is_none()));
        // A value that is neither word shares nothing.
        set_sharing(&f, "everyone");
        assert!(of(&search_as(&f, B, "quokkatide"), A).is_empty());
        set_sharing(&f, "workspace");
        assert!(!of(&search_as(&f, B, "quokkatide"), A).is_empty());
    }

    /// AC-3: a spec, a plan, a context file, work output, the overview, layer knowledge and context, a memory file and a staged
    /// note of project A are not returned to project B under either setting.
    #[test]
    fn nothing_but_research_of_another_project_is_returned_under_any_setting() {
        let f = fx();
        for word in ["workspace", "project"] {
            set_sharing(&f, word);
            let found = search_as(&f, B, "quokkatide");
            let of_a: Vec<&str> = paths(&found).into_iter().filter(|p| p.contains(A)).collect();
            let want: Vec<String> = if word == "workspace" { vec![format!("projects/{A}/files/research/notes.md")] } else { vec![] };
            assert_eq!(of_a, want.iter().map(String::as_str).collect::<Vec<_>>(), "{word}");
            let shown: String = found.hits.iter().map(|h| format!("{}\n{}\n", h.rel_path, h.preview)).collect();
            for kind in ["spec text of alpha", "plan text of alpha", "context text of alpha", "work output of alpha", "overview of alpha", "layer knowledge of alpha", "layer context of alpha", "memory of alpha", "staged note of alpha"] {
                assert!(!shown.contains(kind), "{word}: `{kind}` reached project B:\n{shown}");
            }
            // B still reads all of its own project, as before.
            for kind in ["spec text of beta", "plan text of beta", "context text of beta", "layer knowledge of beta"] {
                assert!(shown.contains(kind), "{word}: B lost its own `{kind}`:\n{shown}");
            }
        }
        // The fixture indexes what must stay hidden, so its absence above is the filter's doing.
        let all = Kb::open_readonly(&f.ws.join("local/state.db")).unwrap().indexed_paths().unwrap();
        for p in ["files/specs/s1.md", "files/plans/p1.md", "files/context/c1.md", "files/_index.md", "layer/knowledge/k1.md", "layer/context/lc.md"] {
            assert!(all.contains(&format!("projects/{A}/{p}")), "{p}");
        }
        // The filter cannot be widened by what a caller puts in the list: an id that is not an id, a path, a wildcard.
        let kb = Kb::open_readonly(&f.ws.join("local/state.db")).unwrap();
        for bad in ["%", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa%", "../aaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' OR 1=1 --", ""] {
            let shared = Some(SharedResearch { projects: vec![(bad.to_string(), "x".into())], ..Default::default() });
            let hits = kb.search("quokkatide", &SearchOpts { include_untrusted: true, rerank_weight: 0.0, zones: Some(vec!["project"]), project: Some(B.into()), shared, ..Default::default() }).unwrap();
            assert!(hits.iter().all(|h| h.rel_path.starts_with(&format!("projects/{B}/"))), "`{bad}`: {:?}", hits.iter().map(|h| &h.rel_path).collect::<Vec<_>>());
        }
        // A search that names no shared list is as isolated as before, whatever the setting.
        set_sharing(&f, "workspace");
        let hits = kb.search("quokkatide", &SearchOpts { include_untrusted: true, rerank_weight: 0.0, project: Some(B.into()), ..Default::default() }).unwrap();
        assert!(hits.iter().all(|h| !h.rel_path.contains(A) && !h.shared));
        assert!(kb.search("quokkatide", &SearchOpts { include_untrusted: true, rerank_weight: 0.0, zones: Some(vec!["project", "research"]), ..Default::default() }).unwrap().is_empty(), "no project, no project rows");
    }

    /// Only a session inside a registered project is given another project's research, and never its own project twice.
    #[test]
    fn the_view_is_for_a_registered_project_only_and_leaves_out_private_projects_and_itself() {
        let f = fx();
        let ids = |v: Option<SharedResearch>| v.map(|v| v.projects.into_iter().map(|(id, name)| format!("{id}:{name}")).collect::<Vec<_>>());
        assert_eq!(ids(view(&f.ws, &ProjectId::parse(B).unwrap())), Some(vec![format!("{A}:alpha")]));
        assert_eq!(ids(view(&f.ws, &ProjectId::parse(P).unwrap())), Some(vec![format!("{A}:alpha"), format!("{B}:beta")]), "a private project still reads the others");
        assert_eq!(view(&f.ws, &ProjectId::default_project()), None, "the default project is no project");
        assert_eq!(view(&f.ws, &ProjectId::parse("dddddddddddddddddddddddddddddddd").unwrap()), None, "an id the workspace does not list");
        assert!(view_at(&f.ws, &f.home).is_none(), "a folder in no project");
        assert_eq!(view_at(&f.ws, &f.b.join("research")).map(|(own, _)| own.as_str().to_string()), Some(B.to_string()));
        set_sharing(&f, "project");
        assert_eq!(view(&f.ws, &ProjectId::parse(B).unwrap()), None);
        set_sharing(&f, "workspace");
        // The boost is the config's, inside its bounds.
        assert_eq!(own_boost(&f.ws), DEFAULT_OWN_BOOST);
        put(f.ws.join("config.toml"), "[projects]\nown_project_boost = 0.01\n");
        assert_eq!(own_boost(&f.ws), 0.01);
        put(f.ws.join("config.toml"), "[projects]\nown_project_boost = 7\n");
        assert_eq!(own_boost(&f.ws), DEFAULT_OWN_BOOST);
        // A registry that cannot be trusted shares nothing.
        std::fs::write(f.ws.join("local/projects.json"), "{not json").unwrap();
        assert_eq!(view(&f.ws, &ProjectId::parse(B).unwrap()), None);
    }

    /// `specs/115` §3 item 3: at equal score a result of the session's own project ranks ahead of a shared one, by the boost.
    #[test]
    fn at_equal_score_the_sessions_own_project_ranks_ahead_of_a_shared_result() {
        let f = fx();
        // The same text in both projects, so the two rows score alike; A's was indexed first and sorts first without the boost.
        for dir in [&f.a, &f.b] {
            put(dir.join("research/same.md"), "# Wombatcurrent\n\nThe wombatcurrent runs north along the shelf.\n");
        }
        sync(&f);
        let kb = Kb::open_readonly(&f.ws.join("local/state.db")).unwrap();
        let order = |boost: f64| -> Vec<String> {
            let shared = Some(SharedResearch { own_boost: boost, ..view(&f.ws, &ProjectId::parse(B).unwrap()).unwrap() });
            kb.search("wombatcurrent shelf", &SearchOpts { include_untrusted: true, rerank_weight: 0.0, zones: Some(vec!["project"]), project: Some(B.into()), shared, ..Default::default() }).unwrap().into_iter().map(|h| h.rel_path).collect()
        };
        let (a, b) = (format!("projects/{A}/files/research/same.md"), format!("projects/{B}/files/research/same.md"));
        assert_eq!(order(0.0), [a.clone(), b.clone()], "the fixture: without a boost the other project's row comes first");
        assert_eq!(order(DEFAULT_OWN_BOOST), [b, a], "with the default boost the session's own comes first");
    }

    /// AC-4, the index half, and `specs/115` §3 item 4: a claim A's research kept is returned to B with A's name; one that rests on
    /// a private source carries the internal-source label and the `internal` word a reader marks on; a private topic's answer and
    /// the private source's own record and text are not readable from B; and the claim adds nothing to any origin count.
    #[test]
    fn a_claim_on_a_private_source_crosses_labelled_and_flagged_and_its_source_and_private_topic_do_not() {
        let f = fx();
        let lib = Library::in_home(&f.home);
        let open = source(&f, "open", "The numbatshelf drops to ninety metres, the public chart says.", false);
        let secret = source(&f, "secret", "The numbatshelf cable landing is at grid 44, the internal survey says. Keyword vaultword.", true);
        let t = topic(&f.a, "alpha", "How deep is the numbatshelf?", "Sources read say the numbatshelf drops to ninety metres.", false);
        let closed = topic(&f.a, "alpha", "Where is the numbatshelf cable landing?", "The numbatshelf cable landing is at grid 44 (vaultword).", true);
        let public = claim(&f, "the numbatshelf drops to ninety metres", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1", Some("alpha"), Some(&t.id), &open, "drops to ninety metres");
        let internal = claim(&f, "the numbatshelf cable landing is at grid 44", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2", Some("alpha"), Some(&t.id), &secret, "cable landing is at grid 44");
        // A claim that names the project but no topic on disk in it is not listed (another workspace's project of the same name).
        let stray = claim(&f, "the numbatshelf belongs to another workspace", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa3", Some("alpha"), Some("unrelated-topic-00000000"), &open, "drops to ninety metres");
        let found = search_as(&f, B, "numbatshelf cable landing grid drops ninety metres");
        let by = |needle: &str| found.hits.iter().find(|h| h.rel_path.contains(needle));
        let pub_hit = by(&public).unwrap_or_else(|| panic!("A's claim is not among {:?}", paths(&found)));
        assert_eq!((pub_hit.rel_path.as_str(), pub_hit.project.as_deref(), pub_hit.shared, pub_hit.internal, pub_hit.label), (format!("research/projects/{A}/claims/{public}.json").as_str(), Some("alpha"), true, false, SHARED_LABEL));
        let int_hit = by(&internal).unwrap_or_else(|| panic!("A's internal-source claim is not among {:?}", paths(&found)));
        assert_eq!((int_hit.rel_path.as_str(), int_hit.project.as_deref(), int_hit.shared, int_hit.internal, int_hit.label), (format!("research/projects/{A}/internal/{internal}.json").as_str(), Some("alpha"), true, true, INTERNAL_LABEL));
        assert_eq!(crate::host_labels::of_hit(&f.ws, int_hit).label, INTERNAL_LABEL);
        assert_eq!(mark_entry(&int_hit.rel_path), Some(format!("shared-claim/{internal}")));
        assert_eq!(mark_entry(&pub_hit.rel_path), None);
        assert!(by(&stray).is_none(), "a claim with no topic on disk in the project is not listed");
        // What crosses is the claim: its sentence, its quote and a pointer. Not the source's text, record or the private topic.
        let shown: String = found.hits.iter().map(|h| format!("{}\n{}\n", h.rel_path, h.preview)).collect();
        assert!(!shown.contains("vaultword") && !shown.contains(&closed.id) && !shown.contains(&format!("library/sources/{secret}")), "{shown}");
        assert!(by(&format!("topics/{}/answer.md", t.id)).is_some_and(|h| h.shared), "an open topic's answer is research and is shared: {:?}", paths(&found));
        let all = Kb::open_readonly(&f.ws.join("local/state.db")).unwrap().indexed_paths().unwrap();
        assert!(all.iter().all(|p| !p.contains(&secret)), "the private source is not in the index at all");
        assert!(all.contains(&format!("projects/{A}/files/research/topics/{}/answer.md", closed.id)), "the fixture: the private topic's answer is in A's own layer");
        // A reads its own private topic and its own claims, the internal one flagged for the mark, neither marked shared.
        let own = search_as(&f, A, "numbatshelf cable landing grid drops ninety metres");
        assert!(own.hits.iter().any(|h| h.rel_path.contains(&closed.id)), "{:?}", paths(&own));
        let own_int = own.hits.iter().find(|h| h.rel_path.contains(&internal)).expect("A finds its own internal-source claim");
        assert_eq!((own_int.shared, own_int.internal, own_int.label), (false, true, OWN_INTERNAL_LABEL));
        assert!(own.hits.iter().any(|h| h.rel_path.contains(&public) && !h.shared && !h.internal));
        // The claim adds nothing to any origin count: a private source is never a cite.
        let c = lib.claim(&internal).unwrap();
        assert!(lib.rests_on_internal(&c) && lib.cites_of(&c).is_empty());
        assert_eq!(lib.refresh_support(&internal).unwrap()["support"]["origins"], json!(0));
        assert!(!lib.rests_on_internal(&lib.claim(&public).unwrap()));
        // B cannot add to A's claim, so B's research cannot count it either.
        let anchors = [AnchorSpec { source_id: open.clone(), locator: Some("https://example.org/page".into()), quote: "drops to ninety metres".into(), prefix: None, suffix: None }];
        assert!(lib.add_research_claim(&ResearchClaim { text: "the numbatshelf cable landing is at grid 44", fingerprint: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2", project: Some("beta"), topic: None, anchors: &anchors }).is_err());
        // With A private, or the setting `project`, none of it reaches B.
        register(&f, &[P, A]);
        assert!(search_as(&f, B, "numbatshelf cable landing grid").hits.iter().all(|h| !h.rel_path.contains(A)));
        register(&f, &[P]);
        set_sharing(&f, "project");
        assert!(search_as(&f, B, "numbatshelf cable landing grid").hits.iter().all(|h| !h.rel_path.contains(A)));
    }

    /// `specs/113` §2 item 4 under the guards of `specs/115` §2 item 3: a claim a host reported on a source Delphi does not hold (a
    /// private pointer) is an internal-source claim. It crosses to another project under the internal label, flagged for the mark,
    /// and counts for no origin; the pointer's own record does not cross. A `personal` claim (a host's claim with no project) is in
    /// no index and reaches no project, its own neighbours included.
    #[test]
    fn a_host_reported_claim_on_a_private_pointer_crosses_as_internal_and_a_personal_one_is_never_shared() {
        let f = fx();
        let lib = Library::in_home(&f.home);
        // The predicate: every kind of private pointer is an internal source, and a public source is not.
        for kind in ["internal", "file", "person", "web"] {
            let id = lib.add_private_pointer(kind, &format!("{kind}: somewhere only the host read"), "A pointer", "2026-11-15T00:00:00Z", None).unwrap();
            assert!(id.starts_with("private-") && crate::library::claims::is_internal_source(&lib.read_meta(&id).unwrap()), "{kind}");
        }
        let open = source(&f, "open", "The wombatferry timetable is public.", false);
        assert!(!crate::library::claims::is_internal_source(&lib.read_meta(&open).unwrap()));

        let t = topic(&f.a, "alpha", "Where does the wombatferry dock?", "The host says the wombatferry docks at pier nine.", false);
        let pointer = lib.add_private_pointer("internal", "harbour desk wiki, page berths", "Harbour desk notes", "2026-11-15T00:00:00Z", None).unwrap();
        let anchors = [AnchorSpec { source_id: pointer.clone(), locator: Some("harbour desk wiki, page berths".into()), quote: "docks at pier nine".into(), prefix: None, suffix: None }];
        let mark = crate::host_research::HOST_REPORTED;
        let hosted = lib.add_host_claim(&ResearchClaim { text: "the wombatferry docks at pier nine", fingerprint: "ddddddddddddddddddddddddddddddd1", project: Some("alpha"), topic: Some(&t.id), anchors: &anchors }, mark).unwrap();
        let personal = lib.add_host_claim(&ResearchClaim { text: "the wombatferry docks at pier eleven", fingerprint: "ddddddddddddddddddddddddddddddd2", project: None, topic: Some(&t.id), anchors: &anchors }, mark).unwrap();
        let (hid, pid) = (hosted["id"].as_str().unwrap().to_string(), personal["id"].as_str().unwrap().to_string());
        assert_eq!((hosted["scope"].as_str(), hosted["host_reported"].clone(), personal["scope"].as_str()), (Some("project"), json!([mark]), Some("personal")));

        let query = "wombatferry docks pier nine eleven";
        let found = search_as(&f, B, query);
        let hit = found.hits.iter().find(|h| h.rel_path.contains(&hid)).unwrap_or_else(|| panic!("A's host-reported claim is not among {:?}", paths(&found)));
        assert_eq!((hit.rel_path.as_str(), hit.project.as_deref(), hit.shared, hit.internal, hit.label), (format!("research/projects/{A}/internal/{hid}.json").as_str(), Some("alpha"), true, true, INTERNAL_LABEL));
        assert_eq!(hit.label, "another project's research, from an internal source, host-reported");
        assert_eq!(crate::host_labels::of_hit(&f.ws, hit).label, INTERNAL_LABEL);
        assert_eq!(mark_entry(&hit.rel_path), Some(format!("shared-claim/{hid}")));
        // No origin: a private pointer is never a cite.
        assert!(lib.rests_on_internal(&hosted) && lib.cites_of(&hosted).is_empty());
        // A host-reported claim with no anchor (its quote was not found) rests on the host's word alone: internal too, and no origin.
        let bare = lib.add_host_claim(&ResearchClaim { text: "the wombatferry docks at pier twelve", fingerprint: "ddddddddddddddddddddddddddddddd3", project: Some("alpha"), topic: Some(&t.id), anchors: &[] }, mark).unwrap();
        assert!(lib.rests_on_internal(&bare) && lib.cites_of(&bare).is_empty());
        // An ordinary claim with no anchor and no host mark is not made internal by this rule.
        assert!(!lib.rests_on_internal(&json!({"anchors": []})));
        assert_eq!(lib.refresh_support(&hid).unwrap()["support"]["origins"].as_u64().unwrap_or(0), 0);
        // The pointer's own record does not cross: it is in no index, so no result of any project is that record.
        let all = Kb::open_readonly(&f.ws.join("local/state.db")).unwrap().indexed_paths().unwrap();
        assert!(all.iter().all(|p| !p.contains(&pointer)), "the private pointer is not in the index at all: {all:?}");
        assert!(found.hits.iter().all(|h| !h.rel_path.contains(&pointer)), "{:?}", paths(&found));
        // The personal claim is listed nowhere, so no project is given it: not B, and not A.
        assert!(all.iter().all(|p| !p.contains(&pid)), "a personal claim is not in the index");
        for own in [A, B] {
            let got = search_as(&f, own, query);
            assert!(got.hits.iter().all(|h| !h.rel_path.contains(&pid) && !h.preview.contains("pier eleven")), "{own}: {:?}", paths(&got));
        }
        // A reads its own host-reported claim, flagged for the mark and not marked shared.
        let own = search_as(&f, A, query);
        let own_hit = own.hits.iter().find(|h| h.rel_path.contains(&hid)).expect("A finds its own host-reported claim");
        assert_eq!((own_hit.shared, own_hit.internal, own_hit.label), (false, true, OWN_INTERNAL_LABEL));
        // With A private, or the setting `project`, it does not reach B.
        register(&f, &[P, A]);
        assert!(search_as(&f, B, query).hits.iter().all(|h| !h.rel_path.contains(&hid)));
        register(&f, &[P]);
        set_sharing(&f, "project");
        assert!(search_as(&f, B, query).hits.iter().all(|h| !h.rel_path.contains(&hid)));
    }

    /// AC-5: a result of another project that names one of the workspace's privacy identifiers is withheld from B and counted; the
    /// same text is still returned to its own project.
    #[test]
    fn a_shared_result_that_names_a_privacy_identifier_is_withheld_and_counted() {
        let f = fx();
        put(f.a.join("research/heron.md"), "# Bandicootledger\n\nThe bandicootledger is kept on Project-Heron's host.\n");
        put(f.a.join("research/plain.md"), "# Bandicootledger\n\nThe bandicootledger is balanced every quarter.\n");
        put(f.ws.join("config.toml"), "[privacy]\nidentifiers = [\"Project-Heron\"]\n");
        let found = search_as(&f, B, "bandicootledger");
        assert_eq!(found.withheld, 1);
        assert_eq!(paths(&found), [format!("projects/{A}/files/research/plain.md")]);
        let own = search_as(&f, A, "bandicootledger");
        assert_eq!((own.withheld, own.hits.len()), (0, 2), "its own project reads it: {:?}", paths(&own));
        // The reply of the host's search carries the count.
        let host = host_search(&f.ws, &f.home, &f.ws.join("local/state.db"), &f.b, "bandicootledger", 5);
        assert_eq!((host.withheld, host.results.len()), (1, 1), "{:?}", host.results);
    }

    /// Findings follow the same rule (`specs/115` §3 item 1): a finding of project A is returned to B, labelled and named; one of
    /// a private project is not; and with the setting `project` neither is.
    #[test]
    fn a_finding_of_another_project_is_shared_on_the_same_rule() {
        let f = fx();
        for (mission, project, name) in [("m-alpha", A, "alpha"), ("m-hidden", P, "hidden")] {
            let dir = delphi_core::sessions::dir(&f.ws, mission);
            put(dir.join("scope"), project);
            put(dir.join("findings/f1.md"), &format!("---\nsource_type: internal_generated\nstatus: active\n---\n# Finding\n\nThe potoroodrift was traced to the {name} intake valve.\n"));
        }
        let found = search_as(&f, B, "potoroodrift intake valve");
        assert_eq!(found.hits.len(), 1, "{:?}", paths(&found));
        let h = &found.hits[0];
        assert_eq!((h.zone.as_str(), h.project.as_deref(), h.shared, h.label), (crate::finding_record::ZONE, Some("alpha"), true, SHARED_LABEL));
        assert!(h.preview.contains("alpha intake"));
        set_sharing(&f, "project");
        assert!(search_as(&f, B, "potoroodrift intake valve").hits.is_empty());
    }

    /// The host's group (`host.search_kb`'s `project_research`): only project research, each result with its project, `shared`
    /// and its label; the session's own project's claims are found on the same listing; a folder in no project gets nothing.
    #[test]
    fn the_hosts_group_holds_only_project_research_and_names_the_claims_to_mark_on() {
        let f = fx();
        let secret = source(&f, "secret", "The bilbyvault code rotates monthly, the internal memo says.", true);
        let open = source(&f, "open", "The bilbyvault is in the north wing, the public map says.", false);
        let ta = topic(&f.a, "alpha", "Where is the bilbyvault?", "The bilbyvault is in the north wing.", false);
        let tb = topic(&f.b, "beta", "What about the bilbyvault door?", "The bilbyvault door is steel.", false);
        let theirs = claim(&f, "the bilbyvault code rotates monthly", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb1", Some("alpha"), Some(&ta.id), &secret, "code rotates monthly");
        let mine = claim(&f, "the bilbyvault is in the north wing", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb2", Some("beta"), Some(&tb.id), &open, "in the north wing");
        put(f.ws.join("team/context/home.md"), "---\nsource_type: user_ratified\n---\n# Home\n\nThe bilbyvault home-layer note.\n");
        put(f.b.join("specs/s2.md"), "# B spec\n\nbilbyvault spec of beta.\n");
        sync(&f);
        let db = f.ws.join("local/state.db");
        let host = host_search(&f.ws, &f.home, &db, &f.b, "bilbyvault code rotates north wing", 10);
        let shown = serde_json::to_string(&host.results).unwrap();
        assert!(host.results.iter().all(|r| r["layer"] == "project_research"), "{shown}");
        let of = |id: &str| host.results.iter().find(|r| r["pointer"].as_str().is_some_and(|p| p.contains(id))).unwrap_or_else(|| panic!("{id} is not in {shown}"));
        let t = of(&theirs);
        assert_eq!((t["project"].as_str(), t["shared"].as_bool(), t["label"].as_str(), t["internal_source"].as_bool()), (Some("alpha"), Some(true), Some(INTERNAL_LABEL), Some(true)), "{t}");
        assert!(t["preview"].as_str().unwrap().starts_with(&format!("<noted-entry label=\"{INTERNAL_LABEL}\">")), "fenced under its label: {t}");
        let m = of(&mine);
        assert_eq!((m["project"].as_str(), m["shared"].as_bool(), m.get("internal_source")), (None, Some(false), None), "a project finds its own claims, not as shared: {m}");
        assert_eq!(host.internal, [format!("shared-claim/{theirs}")], "the caller marks the session on exactly the internal-source claim");
        assert!(!shown.contains("home-layer note") && !shown.contains("spec of beta") && !shown.contains("internal memo"), "nothing of the home layer, of B's own files or of the source's text: {shown}");
        // From A, the internal claim is A's own: found, flagged for the mark, not shared.
        let from_a = host_search(&f.ws, &f.home, &db, &f.a, "bilbyvault code rotates north wing", 10);
        assert_eq!(from_a.internal, [format!("shared-claim/{theirs}")]);
        assert!(from_a.results.iter().any(|r| r["label"] == OWN_INTERNAL_LABEL && r["shared"] == false), "{:?}", from_a.results);
        // With the setting `project`, B still finds its own claim and nothing of A's.
        set_sharing(&f, "project");
        let alone = host_search(&f.ws, &f.home, &db, &f.b, "bilbyvault code rotates north wing", 10);
        assert!(alone.internal.is_empty() && alone.results.len() == 1 && alone.results[0]["pointer"].as_str().unwrap().contains(&mine), "{:?}", alone.results);
        set_sharing(&f, "workspace");
        // A folder in no registered project is given no project's research.
        let outside = host_search(&f.ws, &f.home, &db, &f.home, "bilbyvault code rotates north wing", 10);
        assert!(outside.results.is_empty() && outside.internal.is_empty());
    }

    /// The other two ways a session could be GIVEN an internal-source claim do not exist yet, and this bounds them: the passages
    /// a span starts with and an agent's search name no shared list, so they return no project claim at all; and the claim pull at
    /// mission start offers a project-scoped claim to its own project only (`specs/115` slice X4 changes both, and must mark).
    #[test]
    fn start_passages_and_the_claim_pull_give_no_session_another_projects_claim_yet() {
        let f = fx();
        let secret = source(&f, "secret", "The dunnartkey is under the third stone, the internal note says.", true);
        let ta = topic(&f.a, "alpha", "Where is the dunnartkey?", "Under the third stone.", false);
        let theirs = claim(&f, "the dunnartkey is under the third stone", "ccccccccccccccccccccccccccccccc1", Some("alpha"), Some(&ta.id), &secret, "under the third stone");
        sync(&f);
        let db = f.ws.join("local/state.db");
        // A mission of project B: its span's start passages.
        put(delphi_core::sessions::dir(&f.ws, "m-beta").join("scope"), B);
        let start = crate::retrieval::span_start(&f.home, &db, "m-beta", "Where is the dunnartkey kept under the third stone?", "builder", crate::retrieval::StartBudget { passages: 6, chars: 4000, ..Default::default() }, false);
        assert!(start.is_none_or(|s| !s.text.contains("dunnartkey")), "a span of B starts with nothing of A's");
        // The same question as an agent's `kb_search` asks it (a role, nothing untrusted, findings scoped to B).
        let kb = Kb::open_readonly(&db).unwrap();
        let hits = kb.search("dunnartkey third stone", &SearchOpts { role: Some("builder".into()), rerank_weight: 0.0, findings: FindingsView::Scope(B.into()), ..Default::default() }).unwrap();
        assert!(hits.is_empty(), "{:?}", hits.iter().map(|h| &h.rel_path).collect::<Vec<_>>());
        // The claim pull.
        let lib = Library::in_home(&f.home);
        assert!(lib.pull("dunnartkey third stone", Some("beta"), 5).iter().all(|p| p.id != theirs));
        assert!(lib.pull("dunnartkey third stone", None, 5).iter().all(|p| p.id != theirs));
        assert!(lib.claim_slice(&format!("@library/claim/{theirs}")).unwrap().contains("text withheld"), "and opening the pointer withholds the text, as before");
    }
}
