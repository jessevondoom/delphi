//! The evidence pack of a research topic (`specs/144` §2.5; slice C1).
//!
//! At the end of a research run (`done` or `partial`) code writes the topic's pack, `research/topics/<topic-id>/evidence-pack.md`:
//! for each claim the topic holds, the quote, the stored passage around it, the source id and the hash of the source's stored text,
//! and a place for a support verdict (empty: the support check of §2.4 is not built). A claim whose quote is not found, letter for
//! letter, in its source's stored text is left out. No model is called and nothing is fetched; the claims and the text are the
//! library's, the claim ids the topic's record.
//!
//! **Its label is computed, never stored** (§2.5 item 3, §2.6 item 2). [`label_of`] joins the label of **every source the topic's
//! runs read**, quoted or not (its record's pages and private sources), and of every source its claims are anchored to, with the
//! record's own private flag: `library_private`, `text_held`, `personal`, `internal`, `confidential`. It is read from the records each
//! time it is asked for: by every exit (`label::of_files`, through `Labeller::path`), by every search (`kb` withholds a private pack
//! and labels the rest, [`search_label`]) and by the index listing ([`listed`]). A pack whose record or sources cannot be read is every
//! mark, so private. The frontmatter's fields are a view and are never read back as the label.
//!
//! **Its exits.** The pack is in `held_text::SENDERS`: every exit that carries it asks `held_text::check_files` (the one
//! `body_may_leave`) and 141's label decision (`egress::check_output`), so a pack that quotes a held source past the bound, or any
//! private source, does not leave.

use crate::label::{Facts, Label, Labeller, Mark};
use crate::library::Library;
use crate::research_record::{self, Topic};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The pack's file name in its topic's folder.
pub const FILE: &str = "evidence-pack.md";
/// The `kind` its frontmatter says. A view: an exit that finds it in a file joins the label of the topic it names (only ever more marks).
pub const KIND: &str = "evidence_pack";
/// What a search shows for a pack (§2.6 item 2).
pub const SEARCH_LABEL: &str = "evidence pack · not validated";
/// The taint a pack is indexed at (§2.5 item 2): untrusted text, as the research record says.
pub const TAINT: &str = research_record::TAINT;
/// What a claim's text is cut to in the pack's heading and its line.
const CLAIM_CAP: usize = 300;

/// One claim of a pack, at one of its sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub claim_id: String,
    pub claim: String,
    pub source_id: String,
    /// The sha256 of the source's stored text, `source.md`, as it was when the pack was written.
    pub text_sha256: String,
    pub quote: String,
    /// The stored text on either side of the quote (`library::claims::context_of`).
    pub before: String,
    pub after: String,
}

/// A topic's pack, before it is written.
#[derive(Debug, Clone, Default)]
pub struct Pack {
    pub topic_id: String,
    pub question: String,
    pub run_id: String,
    pub entries: Vec<Entry>,
    /// Anchors left out because their quote is not in the source's stored text (or the text is not kept).
    pub left_out: usize,
    /// Every source the topic's runs read and its claims rest on, by id.
    pub sources: Vec<String>,
}

/// The pack's path in a topic's folder.
pub fn path_of(topic: &Topic) -> PathBuf {
    topic.dir.join(FILE)
}

/// The topic a logical path names when it is a pack, `research/topics/<topic-id>/evidence-pack.md`.
pub fn topic_of_path(rel_path: &str) -> Option<&str> {
    let rest = rel_path.trim_start_matches("./").strip_prefix("research/topics/")?;
    let (id, file) = rest.split_once('/')?;
    (file == FILE && research_record::valid_topic_id(id)).then_some(id)
}

fn clip(s: &str, n: usize) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(n).collect::<String>().trim().to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The claim ids a topic's record lists.
fn claim_ids(rec: &Value) -> Vec<String> {
    rec["claims"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(String::from)).collect()
}

/// Every source a topic's runs read (its pages, its private sources) and every source its claims are anchored to, by id, once each.
/// `None` when the library is not known here (a claim's anchors cannot be read): the pack cannot be traced.
fn sources_of(lib: Option<&Library>, rec: &Value) -> Option<Vec<String>> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for list in ["pages", "private_sources"] {
        for p in rec[list].as_array().into_iter().flatten() {
            if let Some(s) = p["source_id"].as_str().filter(|s| !s.is_empty()) {
                out.insert(s.to_string());
            }
        }
    }
    let ids = claim_ids(rec);
    if !ids.is_empty() {
        let lib = lib?;
        for id in ids {
            // A claim the library no longer holds is in no pack; its sources are still in the record's pages.
            for a in lib.claim(&id).map(|c| c["anchors"].clone()).unwrap_or_default().as_array().into_iter().flatten() {
                if let Some(s) = a["source_id"].as_str().filter(|s| !s.is_empty()) {
                    out.insert(s.to_string());
                }
            }
        }
    }
    Some(out.into_iter().collect())
}

/// The pack of `topic` as its record and the library hold it now. `Err` when the record cannot be read.
pub fn build(lib: &Library, topic: &Topic) -> Result<Pack, String> {
    let rec = topic.read()?;
    let run_id = rec["runs"].as_array().and_then(|r| r.iter().rev().find(|r| matches!(r["state"].as_str(), Some("done" | "partial")))).and_then(|r| r["run_id"].as_str()).unwrap_or("").to_string();
    let mut pack = Pack { topic_id: topic.id.clone(), question: clip(rec["question"].as_str().unwrap_or(""), CLAIM_CAP), run_id, sources: sources_of(Some(lib), &rec).unwrap_or_default(), ..Default::default() };
    for id in claim_ids(&rec) {
        let Some(c) = lib.claim(&id) else { continue };
        let claim = clip(c["text"].as_str().unwrap_or(""), CLAIM_CAP);
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        for a in c["anchors"].as_array().into_iter().flatten() {
            let (Some(sid), Some(quote)) = (a["source_id"].as_str(), a["quote"].as_str()) else { continue };
            if a["state"].as_str().unwrap_or("active") != "active" || quote.trim().is_empty() || !seen.insert((sid.to_string(), quote.to_string())) {
                continue;
            }
            // Anchored in the stored text, letter for letter, or left out (§2.5 item 1).
            let text = (!sid.is_empty() && lib.read_meta(sid).is_some()).then(|| std::fs::read_to_string(lib.source_dir(sid).join("source.md")).ok()).flatten();
            match text.as_deref().and_then(|t| crate::library::claims::context_of(t, quote).map(|(b, af)| (t, b, af))) {
                Some((t, before, after)) => pack.entries.push(Entry { claim_id: id.clone(), claim: claim.clone(), source_id: sid.to_string(), text_sha256: sha256_hex(t.as_bytes()), quote: quote.trim().to_string(), before, after }),
                None => pack.left_out += 1,
            }
        }
    }
    Ok(pack)
}

fn yaml(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// A passage as quoted lines: every line starts with `> `, so nothing in it reads as a heading or as frontmatter.
fn quoted(before: &str, quote: &str, after: &str) -> String {
    let text = format!("{}«{}»{}", before, quote.trim(), after);
    text.lines().map(|l| format!("> {}", l.chars().filter(|c| !c.is_control() || *c == '\t').collect::<String>()).trim_end().to_string()).collect::<Vec<_>>().join("\n")
}

/// The pack as its file: frontmatter that says what it is (a view; nothing reads its label back), then one section per claim at one
/// source, so a chunk of the index holds a claim, its passage and its source together.
pub fn render(pack: &Pack, now: &str) -> String {
    let mut out = format!(
        "---\ntitle: {}\nkind: {KIND}\ntopic: {}\nrun: {}\nauthor: delphi\nsource_type: {TAINT}\nconfidence: low\nclaims: {}\nleft_out: {}\nsources_read: {}\nsupport: unchecked\nupdated: {}\n---\n\n# Evidence pack: {}\n\n{}. Each claim below is shown with the passage of its source's stored text that holds its quote (the quote between « and »). The support check has not run: no claim here is validated.\n",
        yaml(&format!("Evidence pack: {}", pack.question)),
        pack.topic_id,
        pack.run_id,
        pack.entries.len(),
        pack.left_out,
        pack.sources.len(),
        now,
        pack.question,
        SEARCH_LABEL.replace(" · ", ", ")
    );
    for e in &pack.entries {
        out.push_str(&format!(
            "\n## {}\n\n- claim: `{}`\n- source: `s-{}`\n- text sha256: `{}`\n- support:\n\n{}\n",
            e.claim, e.claim_id, e.source_id, e.text_sha256, quoted(&e.before, &e.quote, &e.after)
        ));
    }
    out
}

/// What writing a pack did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    /// Written, with its claims (one per claim at one source) and the anchors left out.
    Pack { claims: usize, left_out: usize },
    /// No claim of the topic has an anchored quote: no pack is kept (an earlier one is removed).
    Empty { left_out: usize },
}

/// Write `topic`'s pack from its record and the library's stored text, through the research record's own writer (a whole file renamed
/// into place, noted as Delphi's own write). A topic with no anchored claim keeps no pack.
pub fn write(lib: &Library, topic: &Topic) -> Result<Written, String> {
    let pack = build(lib, topic)?;
    let path = path_of(topic);
    if pack.entries.is_empty() {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        return Ok(Written::Empty { left_out: pack.left_out });
    }
    research_record::write_atomic(&path, render(&pack, &delphi_core::time::now_rfc3339()).as_bytes())?;
    Ok(Written::Pack { claims: pack.entries.len(), left_out: pack.left_out })
}

/// The pack of a run that just ended `done` or `partial`. A failure to write is not the run's.
pub fn after_run(home: &Path, topic: &Topic) {
    let _ = write(&Library::in_home(home), topic);
}

/// `delphi research pack --all`: a pack for every topic of the workspace and its registered projects, from their records.
pub fn write_all(home: &Path, ws: &Path) -> Vec<(String, Result<Written, String>)> {
    let lib = Library::in_home(home);
    research_record::list(ws).into_iter().map(|(t, _)| (t.id.clone(), write(&lib, &t))).collect()
}

// ---- the label (§2.5 item 3) ------------------------------------------------------------------------------------------------------

/// The pack of `topic_id`'s label, computed now from the records: the research record's own private flag and the label of every
/// source its runs read or its claims rest on. A topic that cannot be found or read, a pack with no source to trace, and a source
/// whose record cannot be read: every mark.
pub fn label_of(l: &Labeller<'_>, topic_id: &str) -> Facts {
    let item = format!("research/topics/{topic_id}/{FILE}");
    let every = |why: &str| Facts { label: Label::every(&item), line: format!("{item}={why}") };
    let Some(ws) = l.workspace() else { return every("unknown") };
    let Some(rec) = research_record::find(ws, topic_id).and_then(|t| t.read().ok()) else { return every("unknown") };
    let Some(sources) = sources_of(l.library(), &rec) else { return every("untraced") };
    if sources.is_empty() {
        return every("untraced");
    }
    let r = l.research_record(topic_id);
    let mut label = r.label;
    let mut line = format!("{item}|{}", r.line);
    for s in &sources {
        let f = l.source(s);
        label = label.join(&f.label);
        line.push_str(&format!("|{}", f.line));
    }
    Facts { label, line }
}

/// The pack's label with a home and a workspace only.
pub fn label(home: &Path, ws: &Path, topic_id: &str) -> Label {
    let lib = Library::in_home(home).with_workspace(ws);
    label_of(&Labeller::new(Some(&lib), Some(ws)), topic_id).label
}

/// Is a pack of this label kept from every agent's search (§2.6 item 3)? A pack from a run that read a private source (a library
/// private one, or one the operator marked private), and one whose label could not be computed (every mark).
pub fn withheld(label: &Label) -> bool {
    label.has(Mark::LibraryPrivate) || label.has(Mark::Personal)
}

/// Is the pack of `topic_id` listed in the index now? Not when it is withheld.
pub fn listed(l: &Labeller<'_>, topic_id: &str) -> bool {
    !withheld(&label_of(l, topic_id).label)
}

/// The label a search shows for a pack row of the index whose database is `db` (`<workspace>/local/state.db`), computed again from the
/// records at the search (§2.6 item 3): `None` when the pack is withheld, or when its workspace cannot be told from the index (an
/// index in memory, or one elsewhere), so it cannot be traced.
pub fn search_label(db: Option<&Path>, topic_id: &str) -> Option<Label> {
    let db = db?;
    let local = db.parent().filter(|p| p.file_name().is_some_and(|n| n == "local") && db.file_name().is_some_and(|n| n == "state.db"))?;
    let ws = local.parent()?;
    let home = crate::held_text::home_of_ws(ws)?;
    let l = label(home, ws, topic_id);
    (!withheld(&l)).then_some(l)
}

/// The topic a file's own frontmatter names as a pack's (`kind: evidence_pack`, `topic: <id>`). A view, used by an exit only to add
/// that topic's label to the file's (never to lower it).
pub fn named_in(text: &str) -> Option<String> {
    let inner = text.strip_prefix("---\n")?;
    let front = &inner[..inner.find("\n---")?];
    let field = |k: &str| front.lines().find_map(|l| l.strip_prefix(k).map(|v| v.trim().trim_matches('"').to_string()));
    (field("kind:").as_deref() == Some(KIND)).then(|| field("topic:")).flatten().filter(|id| research_record::valid_topic_id(id))
}

#[cfg(test)]
#[path = "evidence_pack_tests.rs"]
mod tests;
