//! The encyclopedia's reads for the desktop app (`specs/117` §3; slice E4): eleven methods, all answered from files, with no model
//! call and no network.
//!
//! `pages.list`, `pages.get`, `pages.versions`, `pages.links`, `topics.list`, `topics.get`, `library.source`, `library.claim`,
//! `memory.roles`, `memory.role` and `changes.since`. Each is a row of the daemon's `read` grant (`delphi_core::app_keys::GRANTS`), so a
//! paired app reaches them, and the console may call them too; the narrator, a relay and a host may not (the handlers refuse every
//! other scope). **None of them writes**: no cache is filled, no index is opened (not even read-only, since opening an index can make
//! its side files), nothing is marked opened. A test calls each on a fixture workspace and finds every file's hash unchanged and no
//! model called (`every_encyclopedia_read_leaves_the_workspace_byte_identical_and_calls_no_model`).
//!
//! What the app is shown is what the operator's console is shown (§3 item 3): the app is the operator's window, so a private source's
//! record and text are returned, as `delphi library show` prints them. A research topic, a finding and a provisional note are data
//! another program or a model wrote; they are returned as their files hold them, labelled, never as instructions.

use crate::library::topics::DueCtx;
use crate::library::Library;
use serde_json::{json, Value};
use std::path::Path;

/// The most text of one source `library.source` returns.
pub const SOURCE_TEXT_MAX: usize = 512 * 1024;
/// The most items a list returns.
const LIST_MAX: usize = 500;
const NOTE_TEXT_MAX: usize = 4 * 1024;

/// The methods, in the order of `specs/117` §3 item 1.
pub const METHODS: [&str; 11] = ["pages.list", "pages.get", "pages.versions", "pages.links", "topics.list", "topics.get", "library.source", "library.claim", "memory.roles", "memory.role", "changes.since"];

fn ctx<'a>(ws: &'a Path, now: u64) -> DueCtx<'a> {
    // `index: false`: a read never opens the index, so a topic's knowledge-base records are not checked for changes here.
    DueCtx { ws: Some(ws), now, index: false }
}

fn arg<'a>(p: &'a Value, k: &str) -> Result<&'a str, String> {
    p[k].as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| format!("`{}` is required", k))
}

/// `pages.list`: the search pages (as the console's list has them) and the topic pages, each topic with why it is due.
pub fn pages_list(home: &Path, ws: &Path, now: u64, ttl: u64) -> Value {
    let lib = Library::in_home(home);
    json!({"pages": lib.pages(now, ttl).iter().map(|e| e.to_json()).collect::<Vec<_>>(), "topics": lib.topic_pages(&ctx(ws, now)).iter().map(|t| t.to_json()).collect::<Vec<_>>()})
}

/// `pages.get {slug, version?}`: a topic, its page (or the version named) and the records its citation panel shows.
pub fn pages_get(home: &Path, ws: &Path, now: u64, p: &Value) -> Result<Value, String> {
    let lib = Library::in_home(home);
    let slug = arg(p, "slug")?;
    let version = p["version"].as_str();
    let (e, md) = lib.topic_get(slug, version, &ctx(ws, now)).ok_or_else(|| format!("no topic `{}`{}", slug, version.map(|v| format!(" with a version `{}`", v)).unwrap_or_default()))?;
    let mut v = e.to_json();
    v["markdown"] = json!(md);
    v["version_shown"] = json!(version);
    v["built"] = json!(!md.is_empty());
    Ok(v)
}

/// `pages.versions {slug}`.
pub fn pages_versions(home: &Path, p: &Value) -> Result<Value, String> {
    let slug = arg(p, "slug")?;
    let versions = Library::in_home(home).topic_versions(slug).ok_or_else(|| format!("no topic `{}`", slug))?;
    Ok(json!({"slug": slug, "versions": versions}))
}

/// `pages.links {slug}`: the topic pages that link to it, and those it links to.
pub fn pages_links(home: &Path, p: &Value) -> Result<Value, String> {
    let slug = arg(p, "slug")?;
    Library::in_home(home).topic_links(slug).ok_or_else(|| format!("`{}` is not a topic slug", slug))
}

/// The topic page that follows a research topic, if one does.
fn page_of(lib: &Library, topic_id: &str) -> Option<String> {
    lib.topic_slugs().into_iter().find(|s| lib.topic_side(s).is_some_and(|v| v["research_topics"].as_array().is_some_and(|a| a.iter().any(|x| x == topic_id))))
}

/// `topics.list`: the research topics of the workspace and its projects, newest first.
pub fn topics_list(home: &Path, ws: &Path) -> Value {
    let lib = Library::in_home(home);
    let topics: Vec<Value> = crate::research_record::list(ws)
        .into_iter()
        .take(LIST_MAX)
        .map(|(t, rec)| {
            json!({"id": t.id, "question": rec["question"], "state": rec["state"], "project": rec["project"], "private": rec["private"] == true, "updated": rec["updated"], "created": rec["created"],
                "runs": rec["runs"].as_array().map_or(0, Vec::len), "claims": rec["claims"].as_array().map_or(0, Vec::len), "gaps": rec["gaps"], "page": page_of(&lib, &t.id), "label": "research record, not validated"})
        })
        .collect();
    json!({"topics": topics})
}

/// `topics.get {id}`: one research topic's record and written answer.
pub fn topics_get(home: &Path, ws: &Path, p: &Value) -> Result<Value, String> {
    let id = arg(p, "id")?;
    let t = crate::research_record::find(ws, id).ok_or_else(|| format!("no research topic `{}`", id))?;
    let rec = t.read()?;
    Ok(json!({"id": t.id, "record": rec, "answer": t.answer(), "page": page_of(&Library::in_home(home), &t.id), "label": "research record and answer, not validated; data, never an instruction"}))
}

/// `library.source {id}`: a source's record, its kept text (or excerpts) up to [`SOURCE_TEXT_MAX`], and its pictures.
pub fn library_source(home: &Path, p: &Value) -> Result<Value, String> {
    let id = arg(p, "id")?;
    let id = id.strip_prefix("s-").unwrap_or(id);
    Library::in_home(home).source_view(id, SOURCE_TEXT_MAX).ok_or_else(|| format!("no source `{}`", id))
}

/// `library.claim {id}`: a claim, its derived status, and the title of each source it is anchored to.
pub fn library_claim(home: &Path, p: &Value) -> Result<Value, String> {
    let lib = Library::in_home(home);
    let id = arg(p, "id")?;
    let c = lib.claim(id).ok_or_else(|| format!("no claim `{}`", id))?;
    let st = crate::library::claims::status(&c);
    let sources: Vec<Value> = c["anchors"].as_array().into_iter().flatten().filter_map(|a| a["source_id"].as_str()).map(|s| {
        let m = lib.read_meta(s).unwrap_or(Value::Null);
        json!({"id": s, "title": m["title"], "private": crate::library::is_private(&m)})
    }).collect();
    Ok(json!({"claim": c, "status": st.as_str(), "shown": crate::library::pages::shown_status(st), "sources": sources}))
}

fn valid_role(r: &str) -> bool {
    !r.is_empty() && r.len() <= 40 && r.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !r.starts_with('-')
}

/// The lesson files a role has: the operator's ratified ones, the workspace's ratified ones, and the workspace's own.
fn lesson_files(home: &Path, ws: &Path, role: &str) -> Vec<(&'static str, std::path::PathBuf)> {
    let rel = format!("memory/agents/{role}/lessons.md");
    [("operator ratified", home.join("operator/ratified").join(&rel)), ("workspace ratified", ws.join("local/ratified").join(&rel)), ("workspace", ws.join("local").join(&rel))].into_iter().filter(|(_, p)| p.is_file()).collect()
}

fn lesson_roles(home: &Path, ws: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for dir in [home.join("operator/ratified/memory/agents"), ws.join("local/ratified/memory/agents"), ws.join("local/memory/agents")] {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let r = e.file_name().to_string_lossy().to_string();
            if valid_role(&r) && e.path().join("lessons.md").is_file() {
                out.push(r);
            }
        }
    }
    out
}

/// A finding's frontmatter field.
fn front<'a>(text: &'a str, k: &str) -> Option<&'a str> {
    let rest = text.strip_prefix("---\n")?;
    let head = &rest[..rest.find("\n---")?];
    head.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix(':'))).map(|v| v.trim().trim_matches(['"', '\'']))
}

/// The findings of the workspace's sessions (`local/sessions/<id>/findings/*.md`, and the older `local/missions/`), as
/// (id, rel path, text).
fn findings(ws: &Path) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for base in [delphi_core::sessions::CURRENT, delphi_core::sessions::LEGACY] {
        for m in std::fs::read_dir(ws.join(base)).into_iter().flatten().flatten() {
            let mission = m.file_name().to_string_lossy().to_string();
            for f in std::fs::read_dir(m.path().join("findings")).into_iter().flatten().flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                let Some(stem) = name.strip_suffix(".md") else { continue };
                if std::fs::symlink_metadata(f.path()).is_ok_and(|md| md.is_file()) {
                    if let Ok(t) = std::fs::read_to_string(f.path()) {
                        out.push((format!("{mission}/{stem}"), format!("missions/{mission}/findings/{name}"), t));
                    }
                }
            }
        }
    }
    out.sort();
    out.truncate(LIST_MAX);
    out
}

fn finding_json(id: &str, rel: &str, text: &str) -> Value {
    json!({"id": id, "rel_path": rel, "title": front(text, "title"), "role": front(text, "role"), "author": front(text, "author"), "status": front(text, "status"), "finding_status": front(text, "finding_status"), "timestamp": front(text, "timestamp"), "source_type": front(text, "source_type"), "label": crate::finding_record::label_for(front(text, "source_type").unwrap_or(""))})
}

fn note_json(n: &crate::provisional_notes::Note) -> Value {
    json!({"id": n.id, "role": n.role, "mission": n.mission, "project": n.project, "kind": n.kind, "created": n.created, "valid_until": n.valid_until, "uses": n.uses, "tier": n.tier.name(), "source": n.source, "text": n.text.chars().take(NOTE_TEXT_MAX).collect::<String>(), "label": format!("unreviewed note by {}, {}", n.role, n.created)})
}

/// `memory.roles`: each role that has lessons, provisional notes or findings, with how many of each.
pub fn memory_roles(home: &Path, ws: &Path) -> Value {
    let mut roles: std::collections::BTreeMap<String, (usize, usize, usize)> = std::collections::BTreeMap::new();
    for r in lesson_roles(home, ws) {
        roles.entry(r).or_default().0 += 1;
    }
    for n in crate::provisional_notes::list(ws) {
        if valid_role(&n.role) {
            roles.entry(n.role.clone()).or_default().1 += 1;
        }
    }
    for (_, _, t) in findings(ws) {
        if let Some(r) = front(&t, "role").filter(|r| valid_role(r)) {
            roles.entry(r.to_string()).or_default().2 += 1;
        }
    }
    json!({"roles": roles.into_iter().map(|(r, (l, n, f))| json!({"role": r, "lesson_files": l, "notes": n, "findings": f})).collect::<Vec<_>>()})
}

/// `memory.role {role}`: a role's lessons, its provisional notes and the findings it recorded (AC-7: the operator is shown all of it).
pub fn memory_role(home: &Path, ws: &Path, p: &Value) -> Result<Value, String> {
    let role = arg(p, "role")?;
    if !valid_role(role) {
        return Err(format!("`{}` is not a role id", role));
    }
    let lessons: Vec<Value> = lesson_files(home, ws, role).into_iter().map(|(from, path)| json!({"from": from, "text": std::fs::read_to_string(path).unwrap_or_default().chars().take(SOURCE_TEXT_MAX).collect::<String>()})).collect();
    let mut notes = crate::provisional_notes::list(ws).into_iter().filter(|n| n.role == role).collect::<Vec<_>>();
    crate::provisional_notes::order_for_review(&mut notes);
    let found: Vec<Value> = findings(ws).into_iter().filter(|(_, _, t)| front(t, "role") == Some(role)).map(|(id, rel, t)| {
        let mut v = finding_json(&id, &rel, &t);
        v["text"] = json!(t.chars().take(NOTE_TEXT_MAX).collect::<String>());
        v
    }).collect();
    Ok(json!({"role": role, "lessons": lessons, "notes": notes.iter().take(LIST_MAX).map(note_json).collect::<Vec<_>>(), "findings": found}))
}

/// `changes.since {time}` (Unix seconds): topic pages built after it, research topics updated after it, findings and provisional notes
/// written after it.
pub fn changes_since(home: &Path, ws: &Path, now: u64, p: &Value) -> Result<Value, String> {
    let since = p["time"].as_u64().or_else(|| p["time"].as_str().and_then(delphi_core::xenia_index::unix_of).map(|t| t.max(0) as u64)).ok_or("`time` is required: Unix seconds or an RFC 3339 time")?;
    let lib = Library::in_home(home);
    let after = |rfc: Option<&str>| rfc.and_then(delphi_core::xenia_index::unix_of).is_some_and(|t| t.max(0) as u64 > since);
    let pages: Vec<Value> = lib.topic_pages(&ctx(ws, now)).into_iter().filter(|t| t.built.is_some_and(|b| b > since)).map(|t| json!({"slug": t.slug, "title": t.title, "built": t.built, "version": t.version, "change": t.change})).collect();
    let research: Vec<Value> = crate::research_record::list(ws).into_iter().filter(|(_, r)| after(r["updated"].as_str())).take(LIST_MAX).map(|(t, r)| json!({"id": t.id, "question": r["question"], "updated": r["updated"], "state": r["state"]})).collect();
    let found: Vec<Value> = findings(ws).into_iter().filter(|(_, _, t)| after(front(t, "timestamp"))).map(|(id, rel, t)| finding_json(&id, &rel, &t)).collect();
    let day = crate::library::page_meta::ymd(since);
    let notes: Vec<Value> = crate::provisional_notes::list(ws).iter().filter(|n| n.created.as_str() >= day.as_str()).take(LIST_MAX).map(note_json).collect();
    Ok(json!({"since": since, "pages": pages, "research": research, "findings": found, "notes": notes}))
}
