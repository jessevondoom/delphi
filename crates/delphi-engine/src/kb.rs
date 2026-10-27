//! The knowledge base: hybrid archival search (keywords, meaning and a reranking pass) over a workspace's Markdown
//! (`specs/004` §3).
//!
//! `local/state.db` holds a *derived* index: delete it and `delphi kb reindex` rebuilds it from the files.
//! Documents are chunked along `#`/`##`/`###` headings (fenced code stays whole), each chunk records its
//! exact 1-indexed line range and a breadcrumb, and two FTS5 tables over the chunks are fused by reciprocal
//! rank: an identifier index that keeps tokens such as `REV-SEC-04` and `Part.thoughtSignature` whole, and a
//! prose index. Local embeddings (`sqlite-vec`, `specs/004` §3.5) add a third ranking to the same fusion; they
//! are derived like the rest of the index and are absent, with search unchanged, when no model is installed.
//!
//! Visibility is decided by the query, not the caller's good manners: an agent sees `memory/agents/<role>/`
//! only for its own role, and nothing from untrusted or inactive documents unless asked.

use crate::embed::{self, Embedder, Reranker};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "kb_xenia.rs"]
mod xenia;
pub use xenia::{XeniaApply, XeniaRow};

pub const SCHEMA_VERSION: &str = "1";
/// Version of how documents are cut into chunks. An index made with another version is read again, once, because a
/// file that has not changed would otherwise keep the chunks the old rules made. (2: frontmatter summaries of
/// agents and skills.)
pub const CHUNKER_VERSION: &str = "2";
/// Reciprocal-rank constant (`specs/004` §3.4).
const RRF_K: f64 = 60.0;
/// A chunk longer than this is split at blank lines outside code fences.
const MAX_CHUNK_CHARS: usize = 3500;
/// Files larger than this are not indexed.
pub(crate) const MAX_FILE_BYTES: u64 = 512 * 1024;
/// A chunk is embedded in windows of about this many characters, so the model reads all of a long section.
pub(crate) const WINDOW_CHARS: usize = 1600;
/// Most windows embedded per chunk; a chunk's vectors live at `chunk_id * VEC_STRIDE + window`.
pub(crate) const VEC_STRIDE: i64 = 16;
/// Version of what is embedded for a chunk (the windowing and the text around each window). Changing either bumps
/// this, and every vector is made again, just as for a new model.
pub const EMBED_FORMAT: &str = "1";
/// Chunks embedded per model call and per write transaction.
const EMBED_BATCH: usize = 16;
/// How many nearest windows the vector ranking looks at before duplicates and visibility are applied.
const VEC_CANDIDATES: usize = 120;
/// Default weight of the vector ranking in the fusion (`specs/004` §3.4); each lexical ranking weighs 1.0, definitions 2.0.
const VEC_WEIGHT: f64 = 3.0;
/// Default factor on the keyword rankings when the query is a sentence (`is_natural_question`).
const NATURAL_LEX_WEIGHT: f64 = 0.1;
/// Default factor on the lexical rankings when only the any-word pass found something.
const LOOSE_WEIGHT: f64 = 0.15;
/// Default weight of the reranker's ranking when it is added to the fusion, and how many of the best fused results
/// it reads (`specs/004` §3.7).
const RERANK_WEIGHT: f64 = 6.0;
const RERANK_TOP: usize = 10;
/// A passage is shown to the reranker in at most this many windows of about `RERANK_CHARS`; its best window counts.
const RERANK_WINDOWS: usize = 1;
const RERANK_CHARS: usize = 600;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS kb_documents (
    doc_id          INTEGER PRIMARY KEY,
    rel_path        TEXT NOT NULL UNIQUE,
    zone            TEXT NOT NULL,
    content_blake3  TEXT NOT NULL,
    author          TEXT,
    source_type     TEXT NOT NULL DEFAULT 'external_untrusted'
                    CHECK (source_type IN ('user_ratified', 'internal_empirical', 'internal_generated', 'federated_verified', 'federated_peer', 'external_untrusted')),
    confidence      TEXT NOT NULL DEFAULT 'low',
    status          TEXT NOT NULL DEFAULT 'active',
    updated_at      TEXT NOT NULL,
    mtime_ns        INTEGER NOT NULL DEFAULT 0,
    size            INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS kb_chunks (
    chunk_id        INTEGER PRIMARY KEY,
    doc_id          INTEGER NOT NULL REFERENCES kb_documents(doc_id) ON DELETE CASCADE,
    chunk_index     INTEGER NOT NULL,
    start_line      INTEGER NOT NULL,
    end_line        INTEGER NOT NULL,
    rel_path        TEXT NOT NULL,
    heading_path    TEXT NOT NULL,
    identifiers     TEXT NOT NULL DEFAULT '',
    raw_content     TEXT NOT NULL,
    contextual_text TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS kb_fts_ident USING fts5(
    rel_path, heading_path, identifiers,
    content='kb_chunks', content_rowid='chunk_id',
    tokenize="unicode61 tokenchars '_-.:/@#'"
);
CREATE VIRTUAL TABLE IF NOT EXISTS kb_fts_prose USING fts5(
    raw_content,
    content='kb_chunks', content_rowid='chunk_id',
    tokenize="porter unicode61"
);
CREATE TRIGGER IF NOT EXISTS kb_chunks_ai AFTER INSERT ON kb_chunks BEGIN
  INSERT INTO kb_fts_ident(rowid, rel_path, heading_path, identifiers) VALUES (new.chunk_id, new.rel_path, new.heading_path, new.identifiers);
  INSERT INTO kb_fts_prose(rowid, raw_content) VALUES (new.chunk_id, new.raw_content);
END;
CREATE TRIGGER IF NOT EXISTS kb_chunks_ad AFTER DELETE ON kb_chunks BEGIN
  INSERT INTO kb_fts_ident(kb_fts_ident, rowid, rel_path, heading_path, identifiers) VALUES ('delete', old.chunk_id, old.rel_path, old.heading_path, old.identifiers);
  INSERT INTO kb_fts_prose(kb_fts_prose, rowid, raw_content) VALUES ('delete', old.chunk_id, old.raw_content);
END;
CREATE TABLE IF NOT EXISTS kb_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- Which chunks have vectors (the vectors themselves are in kb_vec, created when an embedding model is attached).
CREATE TABLE IF NOT EXISTS kb_chunk_vec (
    chunk_id        INTEGER PRIMARY KEY REFERENCES kb_chunks(chunk_id) ON DELETE CASCADE,
    windows         INTEGER NOT NULL
);
"#;

#[derive(Debug)]
pub enum KbError {
    Db(rusqlite::Error),
    Io(String),
}

impl std::fmt::Display for KbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KbError::Db(e) => write!(f, "knowledge base: {}", e),
            KbError::Io(e) => write!(f, "knowledge base: {}", e),
        }
    }
}

impl std::error::Error for KbError {}

impl From<rusqlite::Error> for KbError {
    fn from(e: rusqlite::Error) -> KbError {
        KbError::Db(e)
    }
}

// ── chunking ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub start_line: usize,
    pub end_line: usize,
    pub heading_path: String,
    pub raw: String,
}

/// Lines of the document body after any frontmatter, numbered from 1 in the original file.
fn body_start(lines: &[&str]) -> usize {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return 0;
    }
    lines.iter().enumerate().skip(1).find(|(_, l)| l.trim_end() == "---").map(|(i, _)| i + 1).unwrap_or(0)
}

fn heading_level(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=3).contains(&hashes) && line[hashes..].starts_with(' ') {
        Some((hashes, line[hashes..].trim()))
    } else {
        None
    }
}

/// Split Markdown along H1 to H3, keeping fenced blocks (code, tables inside them, Mermaid) whole.
pub fn chunk_markdown(text: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = text.lines().collect();
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut fence: Option<String> = None;
    let mut start = body_start(&lines);
    let mut cur_path = String::new();
    let flush = |chunks: &mut Vec<Chunk>, from: usize, to: usize, path: &str| {
        // `from..to` are 0-based, `to` exclusive. Blank-only sections are skipped.
        let body = &lines[from..to];
        if body.iter().all(|l| l.trim().is_empty()) {
            return;
        }
        let mut piece_start = from;
        let mut size = 0usize;
        let mut in_fence: Option<String> = None;
        for (i, l) in body.iter().enumerate() {
            let t = l.trim_start();
            match &in_fence {
                None => {
                    if let Some(f) = ["```", "~~~"].iter().find(|f| t.starts_with(*f)) {
                        in_fence = Some(f.to_string());
                    }
                }
                Some(f) if t.starts_with(f.as_str()) => in_fence = None,
                _ => {}
            }
            size += l.len() + 1;
            let abs = from + i;
            let last = i + 1 == body.len();
            // Split only at a blank line outside a fence once the piece is large enough.
            if last || (size >= MAX_CHUNK_CHARS && in_fence.is_none() && l.trim().is_empty()) {
                let mut end = abs;
                while end > piece_start && lines[end].trim().is_empty() {
                    end -= 1;
                }
                let piece = &lines[piece_start..=end];
                if piece.iter().any(|l| !l.trim().is_empty()) {
                    chunks.push(Chunk { start_line: piece_start + 1, end_line: end + 1, heading_path: path.to_string(), raw: piece.join("\n") });
                }
                piece_start = abs + 1;
                size = 0;
            }
        }
    };
    for (i, l) in lines.iter().enumerate().skip(start) {
        let t = l.trim_start();
        match &fence {
            None => {
                if let Some(f) = ["```", "~~~"].iter().find(|f| t.starts_with(*f)) {
                    fence = Some(f.to_string());
                } else if let Some((level, title)) = heading_level(l) {
                    flush(&mut chunks, start, i, &cur_path);
                    while stack.last().is_some_and(|(lv, _)| *lv >= level) {
                        stack.pop();
                    }
                    stack.push((level, title.to_string()));
                    cur_path = stack.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join(" > ");
                    start = i;
                }
            }
            Some(f) if t.starts_with(f.as_str()) => fence = None,
            _ => {}
        }
    }
    flush(&mut chunks, start, lines.len(), &cur_path);
    chunks
}

/// Heading of the chunk built from an agent's or skill's frontmatter.
const SUMMARY_HEADING: &str = "Summary (frontmatter)";

/// What a shipped agent or skill says about itself in its frontmatter, as one chunk that points at those lines. The
/// frontmatter is not chunked like the body, but the fields a person or an agent looks things up by (the role, the
/// description, the skills and agents it is tied to) live there.
fn frontmatter_summary(text: &str) -> Option<Chunk> {
    let (fm, _) = crate::workspace::split_frontmatter(text);
    let fm = fm?;
    let lines: Vec<&str> = text.lines().collect();
    let end = body_start(&lines);
    if end == 0 {
        return None;
    }
    let mut rows: Vec<String> = Vec::new();
    for (key, label) in [("title", "Title"), ("role", "Role"), ("name", "Name"), ("description", "Description")] {
        if let Some(v) = fm.get(key).and_then(|v| v.as_str()) {
            rows.push(format!("{}: {}", label, v));
        }
    }
    for (key, label) in [("authorized_skills", "Authorized skills"), ("authorized_agents", "Authorized agents")] {
        let names: Vec<&str> = fm.get(key).and_then(|v| v.as_sequence()).map(|seq| seq.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
        if !names.is_empty() {
            rows.push(format!("{}: {}", label, names.join(", ")));
        }
    }
    (!rows.is_empty()).then(|| Chunk { start_line: 1, end_line: end, heading_path: SUMMARY_HEADING.into(), raw: rows.join("\n") })
}

/// The chunks a document is indexed as: its sections and, for agents and skills, first the frontmatter summary.
pub fn chunks_for(zone: &str, text: &str) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    if matches!(zone, "agents" | "skills") {
        out.extend(frontmatter_summary(text));
    }
    out.extend(chunk_markdown(text));
    out
}

/// For each chunk of a document at `rel_path`, the texts the model reads for it and their cache key
/// (`embed::vectors::key`). The one place that says what a chunk's vectors are filed under, shared by the embedder, the
/// export and a bundle's vector check, so a shipped cache is found by exactly the keys this install would make.
pub fn embed_keys(rel_path: &str, zone: &str, text: &str) -> Vec<(embed::vectors::Key, Vec<String>)> {
    chunks_for(zone, text)
        .into_iter()
        .map(|c| {
            let pieces: Vec<String> = embed::windows(&c.raw, WINDOW_CHARS, VEC_STRIDE as usize).into_iter().map(|p| embed_text(rel_path, &c.heading_path, &p)).collect();
            (embed::vectors::key(&pieces), pieces)
        })
        .collect()
}

/// Identifier-like tokens, and a `def:<token>` marker for each one a line *opens with* (a definition such as
/// `**[REV-SEC-04]** …`), so that a chunk defining an identifier can outrank the many that only mention it.
pub fn identifiers(text: &str) -> String {
    fn clean(tok: &str) -> Option<String> {
        let t = tok.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '#' || c == '@'));
        let has_join = t.chars().any(|c| matches!(c, '_' | '-' | '.' | ':' | '/' | '@' | '#'));
        let inner = t.chars().filter(|c| c.is_alphanumeric()).count();
        (has_join && inner >= 2 && t.len() <= 80 && !t.contains("..")).then(|| t.to_string())
    }
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let mut toks = line.split(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | '(' | ')' | ',' | ';' | '`' | '*' | '"' | '\'' | '<' | '>' | '|')).filter(|t| !t.is_empty());
        let lead = line.trim_start().trim_start_matches(['-', '*', '>', ' ', '#']).trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
        let lead_tok = lead.trim_start_matches(['*', '[', '`', ' ']).split(|c: char| c.is_whitespace() || matches!(c, ']' | '*' | '`' | ':')).next().and_then(clean);
        for t in toks.by_ref() {
            if let Some(c) = clean(t) {
                out.push(c);
            }
        }
        if let Some(d) = lead_tok {
            out.push(format!("def:{}", d));
        }
    }
    out.join(" ")
}

// ── indexing ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub rel_path: String,
    pub zone: &'static str,
    pub path: PathBuf,
    /// The `source_type` for a file whose frontmatter has none. Documents Delphi ships (`agents/`, `skills/`) are
    /// release-ratified, so they are trusted unless the file says otherwise; for everything else it is untrusted.
    pub default_source: &'static str,
    /// Set for a file of an added bundle (`specs/024` §3.6): where it came from, for the breadcrumb and the ranking.
    pub bundle: Option<BundleTag>,
}

/// An added bundle as the index sees it. The zone fixes the tier: a bundle's own `source_type` is a display copy and
/// the install's add is the act that lifts it to `federated_verified` (`specs/024` §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleTag {
    /// `org/name`.
    pub id: String,
    /// `acme/acme-platform@1.4.0 · unsigned`, with `· stale` past `review_by`.
    pub label: String,
    pub stale: bool,
}

const UNTRUSTED: &str = "external_untrusted";
/// A fixed share of the fused score a stale bundle's result keeps (`specs/024` §4.6): lower, never hidden.
const STALE_BUNDLE_FACTOR: f64 = 0.8;

/// The Markdown of every active bundle's `knowledge/`, by logical path `bundles/<org>/<name>/knowledge/…`. The lock
/// names the bundles and the cache holds the files; a removed bundle is simply not listed, so its documents leave the
/// index at the next sync. `home` is derived from the workspace path (`<home>/workspaces/<name>`).
fn bundle_files(ws: &Path, out: &mut Vec<SourceFile>) {
    let Some(home) = ws.parent().and_then(Path::parent) else { return };
    let Ok(lock) = delphi_core::bundles::read(ws) else { return };
    let today = delphi_core::bundles::today();
    for e in lock.bundle.iter().filter(|e| e.is_active() && delphi_core::bundles::plain_id(&e.org) && delphi_core::bundles::plain_id(&e.name)) {
        let root = delphi_core::bundles::cache_dir(home, &e.org, &e.name, &e.hash).join("knowledge");
        let tag = BundleTag { id: e.id(), label: e.label(&today), stale: e.is_stale(&today) };
        let mut files = Vec::new();
        md_files(&root, &format!("bundles/{}/{}/knowledge", e.org, e.name), "bundles", "federated_verified", &mut files);
        out.extend(files.into_iter().map(|f| SourceFile { bundle: Some(tag.clone()), ..f }));
        // A skill's `references/` (`specs/025` §3): text to retrieve, indexed under zone `skills` at the bundle's tier, and
        // read only through `@bundle/<org>/<name>/skills/<skill>/references/**`; never inlined into a prompt.
        let skills = delphi_core::bundles::cache_dir(home, &e.org, &e.name, &e.hash).join("skills");
        out.extend(reference_files(&skills, &e.org, &e.name).into_iter().map(|f| SourceFile { bundle: Some(tag.clone()), ..f }));
    }
}

/// The Markdown under `skills/<skill>/references/` of one cached bundle, by logical path
/// `bundles/<org>/<name>/skills/<skill>/references/…`, zone `skills`.
fn reference_files(skills: &Path, org: &str, name: &str) -> Vec<SourceFile> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(skills) else { return out };
    let mut dirs: Vec<_> = rd.flatten().filter(|d| d.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink())).collect();
    dirs.sort_by_key(|d| d.file_name());
    for d in dirs {
        let skill = d.file_name().to_string_lossy().to_string();
        let refs = d.path().join("references");
        if !skill.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || !std::fs::symlink_metadata(&refs).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        md_files(&refs, &format!("bundles/{}/{}/skills/{}/references", org, name, skill), "skills", "federated_verified", &mut out);
    }
    out
}

pub(crate) fn md_files(root: &Path, logical_prefix: &str, zone: &'static str, default_source: &'static str, out: &mut Vec<SourceFile>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(ft) = e.file_type() else { continue };
        // Symlinks are never followed: the index covers what is in the tree.
        if ft.is_symlink() || name.starts_with('.') {
            continue;
        }
        let rel = if logical_prefix.is_empty() { name.clone() } else { format!("{}/{}", logical_prefix, name) };
        if ft.is_dir() {
            md_files(&e.path(), &rel, zone, default_source, out);
        } else if ft.is_file() && name.ends_with(".md") && e.metadata().map(|m| m.len() <= MAX_FILE_BYTES).unwrap_or(false) {
            out.push(SourceFile { rel_path: rel, zone, path: e.path(), default_source, bundle: None });
        }
    }
}

/// The Markdown a workspace indexes, by logical path. The team copy of a path wins over the built-in one.
/// Operator-scope files are never listed; neither are live worktrees or source code.
pub fn sources(ws: &Path) -> Vec<SourceFile> {
    let mut all: Vec<SourceFile> = Vec::new();
    // The release's own agents and skills are ratified with the release; a team copy is the team's own work.
    for (base, shipped) in [(ws.join("base"), "user_ratified"), (ws.join("team"), "internal_generated")] {
        let mut core = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&base) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if e.file_type().map(|t| t.is_file()).unwrap_or(false) && n.ends_with(".md") {
                    core.push(SourceFile { rel_path: n, zone: "core", path: e.path(), default_source: UNTRUSTED, bundle: None });
                }
            }
        }
        all.extend(core);
        md_files(&base.join("context"), "context", "context", UNTRUSTED, &mut all);
        md_files(&base.join("specs"), "specs", "specs", UNTRUSTED, &mut all);
        md_files(&base.join("agents"), "agents", "agents", shipped, &mut all);
        md_files(&base.join("skills"), "skills", "skills", shipped, &mut all);
    }
    md_files(&ws.join("local/missions"), "missions", "missions", UNTRUSTED, &mut all);
    md_files(&ws.join("local/memory/agents"), "memory/agents", "memory", UNTRUSTED, &mut all);
    // Staged notes (`learn::project_staged` writes them): each file names its own source_type from its tier.
    md_files(&ws.join("local/staged"), "staged", "staged", UNTRUSTED, &mut all);
    bundle_files(ws, &mut all);
    // Later entries (team) override earlier ones (base) for the same logical path.
    let mut by_path: BTreeMap<String, SourceFile> = BTreeMap::new();
    for s in all {
        by_path.insert(s.rel_path.clone(), s);
    }
    by_path.into_values().collect()
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
}

pub struct Kb {
    conn: Connection,
    embedder: Option<Arc<dyn Embedder>>,
    /// Replaces the model's similarity floor (the recall eval tries other values).
    floor: Option<f32>,
    /// Vectors made at release time for the documents Delphi ships (`specs/004` §3.6), used before the model.
    seed: Option<Arc<embed::vectors::VecCache>>,
    /// How many chunks the last `embed_pending` took from `seed` instead of running the model.
    seeded: usize,
    /// Reads a question with each of the best results and reorders them (`specs/004` §3.7).
    reranker: Option<Arc<dyn Reranker>>,
}

fn frontmatter_field(text: &str, key: &str) -> Option<String> {
    let (fm, _) = crate::workspace::split_frontmatter(text);
    fm?.get(key)?.as_str().map(String::from)
}

const SOURCE_TYPES: [&str; 6] = ["user_ratified", "internal_empirical", "internal_generated", "federated_verified", "federated_peer", "external_untrusted"];

impl Kb {
    pub fn open(db: &Path) -> Result<Kb, KbError> {
        crate::vecext::register();
        let conn = Connection::open(db)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Kb::init(conn)
    }

    pub fn open_in_memory() -> Result<Kb, KbError> {
        crate::vecext::register();
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Kb::init(conn)
    }

    fn init(conn: Connection) -> Result<Kb, KbError> {
        conn.execute_batch(SCHEMA)?;
        xenia::ensure(&conn)?;
        // An index made before files were compared by modification time gains the columns; 0 means "read it once".
        let columns: Vec<String> = conn.prepare("PRAGMA table_info(kb_documents)")?.query_map([], |r| r.get::<_, String>(1))?.flatten().collect();
        for (name, ddl) in [("mtime_ns", "ALTER TABLE kb_documents ADD COLUMN mtime_ns INTEGER NOT NULL DEFAULT 0"), ("size", "ALTER TABLE kb_documents ADD COLUMN size INTEGER NOT NULL DEFAULT 0")] {
            if !columns.iter().any(|c| c == name) {
                conn.execute(ddl, [])?;
            }
        }
        conn.execute("INSERT OR IGNORE INTO kb_meta(key, value) VALUES ('schema_version', ?1)", [SCHEMA_VERSION])?;
        let chunker: Option<String> = conn.query_row("SELECT value FROM kb_meta WHERE key = 'chunker_version'", [], |r| r.get(0)).ok();
        if chunker.as_deref() != Some(CHUNKER_VERSION) {
            // Forget what each document looked like, so that the next sync reads and chunks all of them afresh.
            conn.execute("UPDATE kb_documents SET content_blake3 = '', mtime_ns = 0", [])?;
            conn.execute("INSERT OR REPLACE INTO kb_meta(key, value) VALUES ('chunker_version', ?1)", [CHUNKER_VERSION])?;
        }
        Ok(Kb { conn, embedder: None, floor: None, seed: None, seeded: 0, reranker: None })
    }

    /// Index one document, replacing any earlier version. Returns whether it changed anything.
    pub fn upsert(&mut self, rel_path: &str, zone: &str, text: &str, now: &str) -> Result<bool, KbError> {
        self.upsert_from(rel_path, zone, UNTRUSTED, text, now)
    }

    /// `upsert` for a file whose frontmatter may not name its `source_type`: `default_source` stands in.
    pub fn upsert_from(&mut self, rel_path: &str, zone: &str, default_source: &str, text: &str, now: &str) -> Result<bool, KbError> {
        self.upsert_with(rel_path, zone, default_source, None, text, now)
    }

    /// `upsert_from` for a file that may belong to an added bundle. A bundle file's tier is the zone's, whatever its
    /// frontmatter says (that is a display copy, `specs/024` §3.2), and its breadcrumb names the bundle and how it was
    /// signed. Such a file is always rewritten when its bytes were touched, so a new version's label is never stale.
    pub fn upsert_with(&mut self, rel_path: &str, zone: &str, default_source: &str, bundle: Option<&BundleTag>, text: &str, now: &str) -> Result<bool, KbError> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let existing: Option<(i64, String)> = self.conn.query_row("SELECT doc_id, content_blake3 FROM kb_documents WHERE rel_path = ?1", [rel_path], |r| Ok((r.get(0)?, r.get(1)?))).ok();
        if bundle.is_none() && existing.as_ref().is_some_and(|(_, h)| *h == hash) {
            return Ok(false);
        }
        let source_type = if bundle.is_some() { default_source.to_string() } else { frontmatter_field(text, "source_type").filter(|s| SOURCE_TYPES.contains(&s.as_str())).unwrap_or_else(|| default_source.to_string()) };
        let from = bundle.map(|b| format!(" | Bundle: {}", b.label)).unwrap_or_default();
        let (author, confidence, status) = (frontmatter_field(text, "author"), frontmatter_field(text, "confidence").unwrap_or_else(|| "low".into()), frontmatter_field(text, "status").unwrap_or_else(|| "active".into()));
        let tx = self.conn.transaction()?;
        // A bundle's file is rewritten on every add (its label may have changed), but a chunk whose text did not change
        // keeps its vectors: they are filed under a hash of that text, so an update embeds only what was edited.
        let carried = match (&existing, bundle) {
            (Some((id, _)), Some(_)) => carry_vectors(&tx, *id)?,
            _ => HashMap::new(),
        };
        if let Some((id, _)) = &existing {
            drop_vectors(&tx, Some(*id))?;
            tx.execute("DELETE FROM kb_documents WHERE doc_id = ?1", [id])?; // cascades to the chunks, whose trigger clears both indexes
        }
        tx.execute("INSERT INTO kb_documents(rel_path, zone, content_blake3, author, source_type, confidence, status, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![rel_path, zone, hash, author, source_type, confidence, status, now])?;
        let doc_id = tx.last_insert_rowid();
        for (i, c) in chunks_for(zone, text).into_iter().enumerate() {
            let ids = identifiers(&c.raw);
            let ctx = format!("[File: {} | Lines: {}-{} | Section: {} | Source: {} | Status: {}{}]\n{}", rel_path, c.start_line, c.end_line, if c.heading_path.is_empty() { "(top)" } else { &c.heading_path }, source_type, status, from, c.raw);
            tx.execute(
                "INSERT INTO kb_chunks(doc_id, chunk_index, start_line, end_line, rel_path, heading_path, identifiers, raw_content, contextual_text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![doc_id, i as i64, c.start_line as i64, c.end_line as i64, rel_path, c.heading_path, ids, c.raw, ctx],
            )?;
            if !carried.is_empty() {
                let chunk_id = tx.last_insert_rowid();
                let pieces: Vec<String> = embed::windows(&c.raw, WINDOW_CHARS, VEC_STRIDE as usize).into_iter().map(|p| embed_text(rel_path, &c.heading_path, &p)).collect();
                if let Some(blobs) = carried.get(&embed::vectors::key(&pieces)).filter(|b| b.len() == pieces.len()) {
                    tx.execute("INSERT INTO kb_chunk_vec(chunk_id, windows) VALUES (?1, ?2)", params![chunk_id, blobs.len() as i64])?;
                    for (w, blob) in blobs.iter().enumerate() {
                        tx.execute("INSERT INTO kb_vec(rowid, trusted, scope, embedding) VALUES (?1, ?2, ?3, ?4)", params![chunk_id * VEC_STRIDE + w as i64, trusted(&source_type, &status) as i64, vec_scope(zone, rel_path), blob])?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn remove(&mut self, rel_path: &str) -> Result<bool, KbError> {
        let id: Option<i64> = self.conn.query_row("SELECT doc_id FROM kb_documents WHERE rel_path = ?1", [rel_path], |r| r.get(0)).ok();
        if let Some(id) = id {
            drop_vectors(&self.conn, Some(id))?;
        }
        Ok(self.conn.execute("DELETE FROM kb_documents WHERE rel_path = ?1", [rel_path])? > 0)
    }

    /// Bring the index in line with the files, then give any chunk without a vector one (when a model is attached).
    pub fn sync(&mut self, files: &[SourceFile], now: &str) -> Result<IndexReport, KbError> {
        let rep = self.sync_lexical(files, now)?;
        if self.embedder.is_some() {
            self.embed_pending(&mut |_, _| {})?;
        }
        Ok(rep)
    }

    /// Everything a search needs to see the current files: the words are brought up to date (unchanged files are
    /// found by their modification time and size, not read), and vectors the release shipped are applied. No model
    /// is run; chunks that still lack a vector are embedded by `embed_pending`, which callers start in the background.
    pub fn refresh(&mut self, ws: &Path, now: &str) -> Result<IndexReport, KbError> {
        crate::learn::project_staged(ws);
        let rep = self.sync_lexical(&sources(ws), now)?;
        if self.embedder.is_some() {
            self.apply_seed_vectors()?;
        }
        Ok(rep)
    }

    /// New and changed documents indexed, vanished ones dropped. Fast: no model is run.
    pub fn sync_lexical(&mut self, files: &[SourceFile], now: &str) -> Result<IndexReport, KbError> {
        let mut rep = IndexReport::default();
        let known: HashMap<String, (i64, i64)> = {
            // The Xenia zone is derived from a store clone, not from files: a file sync neither sees nor drops it.
            let mut st = self.conn.prepare("SELECT rel_path, mtime_ns, size FROM kb_documents WHERE zone != 'xenia'")?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))))?;
            rows.flatten().collect()
        };
        let mut seen: HashMap<&str, ()> = HashMap::new();
        for f in files {
            let Ok(meta) = std::fs::metadata(&f.path) else { continue };
            let stamp = (meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i64).unwrap_or(0), meta.len() as i64);
            seen.insert(&f.rel_path, ());
            // A file with the modification time and size it had when it was indexed is not read again: this is
            // what lets every search check the whole tree. `kb reindex` reads everything regardless.
            if stamp.0 != 0 && known.get(&f.rel_path) == Some(&stamp) {
                rep.unchanged += 1;
                continue;
            }
            let Ok(bytes) = std::fs::read(&f.path) else {
                seen.remove(f.rel_path.as_str());
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            let existed = known.contains_key(&f.rel_path);
            if self.upsert_with(&f.rel_path, f.zone, f.default_source, f.bundle.as_ref(), &text, now)? {
                if existed {
                    rep.updated += 1;
                } else {
                    rep.added += 1;
                }
            } else {
                rep.unchanged += 1;
            }
            self.conn.execute("UPDATE kb_documents SET mtime_ns = ?2, size = ?3 WHERE rel_path = ?1", params![f.rel_path, stamp.0, stamp.1])?;
        }
        for p in known.keys().filter(|p| !seen.contains_key(p.as_str())) {
            self.remove(p)?;
            rep.removed += 1;
        }
        let stale: std::collections::BTreeSet<&str> = files.iter().filter_map(|f| f.bundle.as_ref().filter(|b| b.stale).map(|b| b.id.as_str())).collect();
        self.conn.execute("INSERT OR REPLACE INTO kb_meta(key, value) VALUES ('stale_bundles', ?1)", [stale.into_iter().collect::<Vec<_>>().join(",")])?;
        Ok(rep)
    }

    /// Drop everything and index again from the files.
    pub fn rebuild(&mut self, files: &[SourceFile], now: &str) -> Result<IndexReport, KbError> {
        let rep = self.rebuild_lexical(files, now)?;
        if self.embedder.is_some() {
            self.embed_pending(&mut |_, _| {})?;
        }
        Ok(rep)
    }

    /// `rebuild` without running the model; `embed_pending` gives the chunks their vectors afterwards.
    pub fn rebuild_lexical(&mut self, files: &[SourceFile], now: &str) -> Result<IndexReport, KbError> {
        drop_vectors(&self.conn, None)?;
        // Only what files made: the Xenia zone comes from the store clone and `xenia_index` brings it up to date.
        self.conn.execute("DELETE FROM kb_documents WHERE zone != 'xenia'", [])?;
        self.conn.execute("INSERT INTO kb_fts_ident(kb_fts_ident) VALUES('rebuild')", [])?;
        self.conn.execute("INSERT INTO kb_fts_prose(kb_fts_prose) VALUES('rebuild')", [])?;
        self.sync_lexical(files, now)
    }

    /// FTS5's own consistency check of both indexes against the chunk table.
    pub fn integrity_check(&self) -> Result<(), KbError> {
        self.conn.execute("INSERT INTO kb_fts_ident(kb_fts_ident, rank) VALUES('integrity-check', 1)", [])?;
        self.conn.execute("INSERT INTO kb_fts_prose(kb_fts_prose, rank) VALUES('integrity-check', 1)", [])?;
        Ok(())
    }

    pub fn counts(&self) -> Result<(usize, usize), KbError> {
        let d: i64 = self.conn.query_row("SELECT COUNT(*) FROM kb_documents", [], |r| r.get(0))?;
        let c: i64 = self.conn.query_row("SELECT COUNT(*) FROM kb_chunks", [], |r| r.get(0))?;
        Ok((d as usize, c as usize))
    }
}

// ── vectors ─────────────────────────────────────────────────────────────────

fn has_vec(conn: &Connection) -> bool {
    conn.query_row("SELECT 1 FROM sqlite_master WHERE name = 'kb_vec'", [], |_| Ok(())).is_ok()
}

/// The vectors of a document's chunks by cache key, as stored, to be given back to chunks whose text is unchanged.
fn carry_vectors(conn: &Connection, doc_id: i64) -> Result<HashMap<embed::vectors::Key, Vec<Vec<u8>>>, KbError> {
    let mut out = HashMap::new();
    if !has_vec(conn) {
        return Ok(out);
    }
    let rows: Vec<(i64, String, String, String, i64)> = {
        let mut st = conn.prepare("SELECT c.chunk_id, c.rel_path, c.heading_path, c.raw_content, m.windows FROM kb_chunks c JOIN kb_chunk_vec m ON m.chunk_id = c.chunk_id WHERE c.doc_id = ?1")?;
        let rows = st.query_map([doc_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        rows.flatten().collect()
    };
    for (id, path, heading, raw, windows) in rows {
        let pieces: Vec<String> = embed::windows(&raw, WINDOW_CHARS, VEC_STRIDE as usize).into_iter().map(|p| embed_text(&path, &heading, &p)).collect();
        if pieces.len() as i64 != windows {
            continue;
        }
        let mut blobs = Vec::new();
        for w in 0..windows {
            blobs.push(conn.query_row("SELECT embedding FROM kb_vec WHERE rowid = ?1", [id * VEC_STRIDE + w], |r| r.get::<_, Vec<u8>>(0))?);
        }
        out.insert(embed::vectors::key(&pieces), blobs);
    }
    Ok(out)
}

/// Delete the vectors of one document, or of all of them. Done before the chunks go: the mapping table cascades,
/// but the vectors live in a virtual table that knows nothing of foreign keys.
fn drop_vectors(conn: &Connection, doc_id: Option<i64>) -> Result<(), KbError> {
    if !has_vec(conn) {
        return Ok(());
    }
    let Some(doc) = doc_id else {
        conn.execute("DELETE FROM kb_vec", [])?;
        conn.execute("DELETE FROM kb_chunk_vec", [])?;
        return Ok(());
    };
    let rows: Vec<(i64, i64)> = {
        let mut st = conn.prepare("SELECT m.chunk_id, m.windows FROM kb_chunk_vec m JOIN kb_chunks c ON c.chunk_id = m.chunk_id WHERE c.doc_id = ?1")?;
        let rows = st.query_map([doc], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.flatten().collect()
    };
    for (chunk, windows) in rows {
        for w in 0..windows {
            conn.execute("DELETE FROM kb_vec WHERE rowid = ?1", [chunk * VEC_STRIDE + w])?;
        }
    }
    Ok(())
}

/// Who may see a vector: `''` for shared text, the role's name for `memory/agents/<role>/`, `'~'` for any other
/// memory file (visible to no role). The authoritative check is the SQL filter in `allowed`; this only lets the
/// nearest-neighbour search skip what could never be returned.
fn vec_scope(zone: &str, rel_path: &str) -> String {
    if zone != "memory" {
        return String::new();
    }
    rel_path.strip_prefix("memory/agents/").and_then(|r| r.split('/').next()).filter(|r| !r.is_empty()).map(String::from).unwrap_or_else(|| "~".into())
}

fn trusted(source_type: &str, status: &str) -> bool {
    status == "active" && !matches!(source_type, "external_untrusted" | "federated_peer")
}

/// What the model reads for a window of a chunk: where it is, then the text.
fn embed_text(rel_path: &str, heading_path: &str, piece: &str) -> String {
    if heading_path.is_empty() {
        format!("{}\n{}", rel_path, piece)
    } else {
        format!("{} > {}\n{}", rel_path, heading_path, piece)
    }
}

/// The SQL condition (over `kb_documents d`) for the documents `opts` may see: nothing untrusted or inactive unless
/// asked, and a role sees only its own private memory. Search, listing and reading all use this one filter.
fn allowed_sql(opts: &SearchOpts) -> String {
    let mut allowed = String::from("1=1");
    if !opts.include_untrusted {
        allowed.push_str(" AND d.status = 'active' AND d.source_type NOT IN ('external_untrusted', 'federated_peer')");
    }
    if let Some(r) = &opts.role {
        // An agent sees only its own private memory (a whole path segment, not a prefix). A role name that is
        // not a plain identifier matches nothing, so it cannot be used to widen the filter.
        if valid_role(r) {
            allowed.push_str(&format!(" AND (d.zone != 'memory' OR d.rel_path LIKE 'memory/agents/{}/%')", r));
        } else {
            allowed.push_str(" AND d.zone != 'memory'");
        }
    }
    if opts.no_bundles {
        allowed.push_str(" AND d.zone != 'bundles' AND d.rel_path NOT LIKE 'bundles/%'");
    }
    if !opts.include_staged {
        allowed.push_str(" AND d.zone != 'staged'");
    }
    // The Xenia zone is judged by its own rule, whatever `include_untrusted` says: invisible unless a view is asked
    // for, and then L2 only (agents) or the rest (the quarantine read).
    match opts.xenia {
        XeniaView::Off => allowed.push_str(" AND d.zone != 'xenia'"),
        XeniaView::Agent => allowed.push_str(" AND d.zone = 'xenia' AND d.status = 'active' AND d.source_type = 'federated_verified'"),
        XeniaView::Quarantine => allowed.push_str(" AND d.zone = 'xenia' AND NOT (d.status = 'active' AND d.source_type = 'federated_verified')"),
    }
    if let Some(b) = &opts.bundle {
        // `org` or `org/name`, plain ids only: anything else matches nothing, so it cannot widen the filter.
        let parts: Vec<&str> = b.split('/').collect();
        if parts.len() <= 2 && parts.iter().all(|p| delphi_core::bundles::plain_id(p)) {
            allowed.push_str(&format!(" AND d.rel_path LIKE 'bundles/{}/%'", b));
        } else {
            allowed.push_str(" AND 1=0");
        }
    }
    allowed
}

/// A document as the index knows it (no text).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DocInfo {
    pub rel_path: String,
    pub zone: String,
    pub source_type: String,
    pub status: String,
    pub blake3: String,
    pub updated_at: String,
}

fn doc_info(r: &rusqlite::Row) -> rusqlite::Result<DocInfo> {
    Ok(DocInfo { rel_path: r.get(0)?, zone: r.get(1)?, source_type: r.get(2)?, status: r.get(3)?, blake3: r.get(4)?, updated_at: r.get(5)? })
}

/// A document's last change on disk: the indexer keeps the file's modification time, while `updated_at` is empty in an
/// index the daemon built, which has no clock to give it.
const DOC_COLS: &str = "d.rel_path, d.zone, d.source_type, d.status, d.content_blake3, strftime('%Y-%m-%dT%H:%M:%SZ', d.mtime_ns / 1000000000, 'unixepoch')";

impl Kb {
    /// The collections (zones) with how many visible documents each holds and when the newest changed.
    pub fn collections(&self, opts: &SearchOpts) -> Result<Vec<(String, usize, String)>, KbError> {
        let sql = format!("SELECT d.zone, COUNT(*), strftime('%Y-%m-%dT%H:%M:%SZ', MAX(d.mtime_ns) / 1000000000, 'unixepoch') FROM kb_documents d WHERE {} GROUP BY d.zone ORDER BY d.zone", allowed_sql(opts));
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize, r.get::<_, String>(2)?)))?;
        Ok(rows.flatten().collect())
    }

    /// Visible documents, newest first, optionally of one collection.
    pub fn documents(&self, zone: Option<&str>, limit: usize, offset: usize, opts: &SearchOpts) -> Result<Vec<DocInfo>, KbError> {
        let sql = format!("SELECT {DOC_COLS} FROM kb_documents d WHERE {} AND (?1 IS NULL OR d.zone = ?1) ORDER BY d.mtime_ns DESC, d.rel_path LIMIT ?2 OFFSET ?3", allowed_sql(opts));
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(rusqlite::params![zone, limit as i64, offset as i64], doc_info)?;
        Ok(rows.flatten().collect())
    }

    /// One visible document by logical path; `None` when it is not indexed or not visible to `opts`.
    pub fn document(&self, rel_path: &str, opts: &SearchOpts) -> Result<Option<DocInfo>, KbError> {
        let sql = format!("SELECT {DOC_COLS} FROM kb_documents d WHERE {} AND d.rel_path = ?1", allowed_sql(opts));
        Ok(self.conn.query_row(&sql, [rel_path], doc_info).ok())
    }
}

fn valid_role(r: &str) -> bool {
    !r.is_empty() && r.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_alphanumeric() || c == '-')
}

impl Kb {
    /// Use `e` for vectors. A vector table made by another model (or another size) is dropped and refilled.
    pub fn attach_embedder(&mut self, e: Arc<dyn Embedder>) -> Result<(), KbError> {
        let meta = |c: &Connection, k: &str| -> Option<String> { c.query_row("SELECT value FROM kb_meta WHERE key = ?1", [k], |r| r.get(0)).ok() };
        let (id, dim) = (e.model_id().to_string(), e.dim().to_string());
        let same = meta(&self.conn, "embedding_model_id").as_deref() == Some(id.as_str()) && meta(&self.conn, "embedding_dim").as_deref() == Some(dim.as_str()) && meta(&self.conn, "embedding_format").as_deref() == Some(EMBED_FORMAT);
        if !same || !has_vec(&self.conn) {
            let tx = self.conn.transaction()?;
            tx.execute_batch("DROP TABLE IF EXISTS kb_vec; DELETE FROM kb_chunk_vec;")?;
            tx.execute_batch(&format!("CREATE VIRTUAL TABLE kb_vec USING vec0(trusted INTEGER, scope TEXT, embedding float[{}] distance_metric=cosine);", e.dim()))?;
            tx.execute("INSERT OR REPLACE INTO kb_meta(key, value) VALUES ('embedding_model_id', ?1), ('embedding_dim', ?2), ('embedding_format', ?3)", params![id, dim, EMBED_FORMAT])?;
            tx.commit()?;
        }
        self.seed = embed::vectors::load_seed(e.model_id(), e.dim(), EMBED_FORMAT);
        self.embedder = Some(e);
        Ok(())
    }

    pub fn attach_reranker(&mut self, r: Option<Arc<dyn Reranker>>) {
        self.reranker = r;
    }

    pub fn reranker(&self) -> Option<&Arc<dyn Reranker>> {
        self.reranker.as_ref()
    }

    /// Use these preloaded vectors (tests; `attach_embedder` loads the shipped ones itself).
    pub fn set_seed_vectors(&mut self, seed: Option<Arc<embed::vectors::VecCache>>) {
        self.seed = seed;
    }

    /// Also use these vectors, which the caller has already checked against this install's embedder (a bundle's
    /// shipped cache, `bundle_vectors`), beside the release's own.
    pub fn add_seed_vectors(&mut self, extra: &embed::vectors::VecCache) {
        let mut all: Vec<(embed::vectors::Key, Vec<Vec<f32>>)> = extra.entries().map(|(k, v)| (*k, v.clone())).collect();
        if let Some(have) = &self.seed {
            all.extend(have.entries().map(|(k, v)| (*k, v.clone())));
        }
        self.seed = Some(Arc::new(embed::vectors::VecCache::from_entries(&extra.model_id, extra.dim, &extra.format, all)));
    }

    /// `(chunks with a vector, chunks)` for one added bundle (`org/name`).
    pub fn bundle_vector_counts(&self, id: &str) -> Result<(usize, usize), KbError> {
        let prefix = format!("bundles/{}/", id);
        let total: i64 = self.conn.query_row("SELECT COUNT(*) FROM kb_chunks WHERE substr(rel_path, 1, ?2) = ?1", params![prefix, prefix.len() as i64], |r| r.get(0))?;
        let done: i64 = self.conn.query_row("SELECT COUNT(*) FROM kb_chunks c JOIN kb_chunk_vec m ON m.chunk_id = c.chunk_id WHERE substr(c.rel_path, 1, ?2) = ?1", params![prefix, prefix.len() as i64], |r| r.get(0))?;
        Ok((done as usize, total as usize))
    }

    /// Chunks the last `embed_pending` filled from the preloaded vectors without running the model.
    pub fn seeded(&self) -> usize {
        self.seeded
    }

    pub fn embedder(&self) -> Option<&Arc<dyn Embedder>> {
        self.embedder.as_ref()
    }

    pub fn set_floor(&mut self, floor: Option<f32>) {
        self.floor = floor;
    }

    /// Cosine similarity of the nearest window to `query`, whatever the floor says (for calibrating the floor).
    pub fn top_similarity(&self, query: &str) -> Option<f32> {
        let q = self.embedder.as_ref()?.embed_query(query).ok()?;
        if !has_vec(&self.conn) {
            return None;
        }
        let d: f64 = self.conn.query_row("SELECT distance FROM kb_vec WHERE embedding MATCH ?1 AND k = 1", [embed::blob(&q)], |r| r.get(0)).ok()?;
        Some(1.0 - d as f32)
    }

    /// `(chunks with a vector, chunks)`.
    pub fn vector_counts(&self) -> Result<(usize, usize), KbError> {
        let v: i64 = self.conn.query_row("SELECT COUNT(*) FROM kb_chunk_vec", [], |r| r.get(0))?;
        let c: i64 = self.conn.query_row("SELECT COUNT(*) FROM kb_chunks", [], |r| r.get(0))?;
        Ok((v as usize, c as usize))
    }

    /// Embed every chunk that has no vector yet, a few at a time so that no write lock is held while the model
    /// runs. `progress(done, total)` is called after each batch. Returns how many chunks were embedded.
    pub fn embed_pending(&mut self, progress: &mut dyn FnMut(usize, usize)) -> Result<usize, KbError> {
        self.embed_batches(progress, true)
    }

    /// Give chunks their vectors from the preloaded cache only; the model is not run, so this is quick enough to do
    /// before a search. Chunks the cache does not cover stay pending for `embed_pending`.
    pub fn apply_seed_vectors(&mut self) -> Result<usize, KbError> {
        if self.seed.is_none() {
            return Ok(0);
        }
        self.embed_batches(&mut |_, _| {}, false)
    }

    fn embed_batches(&mut self, progress: &mut dyn FnMut(usize, usize), run_model: bool) -> Result<usize, KbError> {
        let Some(model) = self.embedder.clone() else { return Ok(0) };
        let total: i64 = self.conn.query_row("SELECT COUNT(*) FROM kb_chunks c LEFT JOIN kb_chunk_vec m ON m.chunk_id = c.chunk_id WHERE m.chunk_id IS NULL", [], |r| r.get(0))?;
        let mut done = 0usize;
        let mut seeded = 0usize;
        // Chunks are taken in order and never revisited in one pass, so a chunk that is skipped cannot stall it.
        let mut after = 0i64;
        loop {
            struct Pending {
                id: i64,
                trusted: bool,
                scope: String,
                pieces: Vec<String>,
            }
            let batch: Vec<Pending> = {
                let mut st = self.conn.prepare(
                    "SELECT c.chunk_id, c.rel_path, c.heading_path, c.raw_content, d.source_type, d.status, d.zone FROM kb_chunks c JOIN kb_documents d ON d.doc_id = c.doc_id LEFT JOIN kb_chunk_vec m ON m.chunk_id = c.chunk_id WHERE m.chunk_id IS NULL AND c.chunk_id > ?2 ORDER BY c.chunk_id LIMIT ?1",
                )?;
                let rows = st.query_map([EMBED_BATCH as i64, after], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?)))?;
                rows.flatten()
                    .map(|(id, path, heading, raw, src, status, zone)| Pending {
                        id,
                        trusted: trusted(&src, &status),
                        scope: vec_scope(&zone, &path),
                        pieces: embed::windows(&raw, WINDOW_CHARS, VEC_STRIDE as usize).into_iter().map(|p| embed_text(&path, &heading, &p)).collect(),
                    })
                    .collect()
            };
            let Some(last) = batch.last().map(|p| p.id) else { break };
            after = last;
            // Vectors made at release time for exactly this text are used as they are; the model reads the rest.
            let have: Vec<Option<Vec<Vec<f32>>>> = batch.iter().map(|p| self.seed.as_ref().and_then(|c| c.get(&embed::vectors::key(&p.pieces))).filter(|v| v.len() == p.pieces.len()).cloned()).collect();
            let texts: Vec<String> = if run_model { batch.iter().zip(&have).filter(|(_, h)| h.is_none()).flat_map(|(p, _)| p.pieces.iter().cloned()).collect() } else { Vec::new() };
            let mut fresh = if texts.is_empty() { Vec::new().into_iter() } else { model.embed_documents(&texts).map_err(KbError::Io)?.into_iter() };
            let tx = self.conn.transaction()?;
            for (p, h) in batch.iter().zip(have) {
                let mine: Vec<Vec<f32>> = match h {
                    Some(v) => {
                        seeded += 1;
                        v
                    }
                    None if !run_model => continue,
                    None => fresh.by_ref().take(p.pieces.len()).collect(),
                };
                // The chunk may have been replaced while the model ran; then its vectors are not wanted.
                let claimed = tx.execute("INSERT OR IGNORE INTO kb_chunk_vec(chunk_id, windows) SELECT chunk_id, ?2 FROM kb_chunks WHERE chunk_id = ?1", params![p.id, mine.len() as i64])?;
                if claimed == 1 {
                    for (w, v) in mine.iter().enumerate() {
                        tx.execute("INSERT INTO kb_vec(rowid, trusted, scope, embedding) VALUES (?1, ?2, ?3, ?4)", params![p.id * VEC_STRIDE + w as i64, p.trusted as i64, p.scope, embed::blob(v)])?;
                    }
                }
            }
            tx.commit()?;
            done += batch.len();
            progress(done, total as usize);
        }
        self.seeded = seeded;
        Ok(if run_model { done } else { seeded })
    }

    /// The vectors of every embedded chunk, as a preloaded-vector cache for the model in use (`specs/004` §3.6).
    /// Keys are hashes of the text given to the model, so the cache is valid for any workspace that has the same text.
    pub fn export_vectors(&self) -> Result<Vec<u8>, KbError> {
        let model = self.embedder.as_ref().ok_or_else(|| KbError::Io("there is no embedding model to export vectors for".into()))?;
        let rows: Vec<(i64, String, String, String, i64)> = {
            let mut st = self.conn.prepare("SELECT c.chunk_id, c.rel_path, c.heading_path, c.raw_content, m.windows FROM kb_chunks c JOIN kb_chunk_vec m ON m.chunk_id = c.chunk_id ORDER BY c.chunk_id")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
            rows.flatten().collect()
        };
        let mut seen = HashSet::new();
        let mut entries: Vec<(embed::vectors::Key, Vec<Vec<f32>>)> = Vec::new();
        for (id, path, heading, raw, windows) in rows {
            let pieces: Vec<String> = embed::windows(&raw, WINDOW_CHARS, VEC_STRIDE as usize).into_iter().map(|p| embed_text(&path, &heading, &p)).collect();
            let k = embed::vectors::key(&pieces);
            if pieces.len() as i64 != windows || !seen.insert(k) {
                continue;
            }
            let mut vecs = Vec::new();
            for w in 0..windows {
                let bytes: Vec<u8> = self.conn.query_row("SELECT embedding FROM kb_vec WHERE rowid = ?1", [id * VEC_STRIDE + w], |r| r.get(0))?;
                vecs.push(bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect());
            }
            entries.push((k, vecs));
        }
        Ok(embed::vectors::VecCache::serialize(model.model_id(), model.dim(), EMBED_FORMAT, &entries))
    }

    /// Chunks nearest the query, best first, each once, with cosine similarity at or above the model's floor.
    /// Visibility is decided by `allowed` (the same filter the lexical rankings use), not by the vector table.
    fn vector_ranked(&self, query: &str, opts: &SearchOpts, allowed: &str) -> Result<Vec<(i64, f32)>, KbError> {
        let Some(model) = &self.embedder else { return Ok(vec![]) };
        if !has_vec(&self.conn) {
            return Ok(vec![]);
        }
        let Ok(q) = model.embed_query(query) else { return Ok(vec![]) };
        let mut sql = format!("SELECT rowid, distance FROM kb_vec WHERE embedding MATCH ?1 AND k = {}", VEC_CANDIDATES);
        if !opts.include_untrusted {
            sql.push_str(" AND trusted = 1");
        }
        if let Some(r) = &opts.role {
            if valid_role(r) {
                sql.push_str(&format!(" AND scope IN ('', '{}')", r));
            } else {
                sql.push_str(" AND scope = ''");
            }
        }
        sql.push_str(" ORDER BY distance");
        let near: Vec<(i64, f32)> = {
            let mut st = self.conn.prepare(&sql)?;
            let rows = st.query_map([embed::blob(&q)], |r| Ok((r.get::<_, i64>(0)? / VEC_STRIDE, 1.0 - r.get::<_, f64>(1)? as f32)))?;
            rows.flatten().filter(|(_, sim)| *sim >= self.floor.unwrap_or(model.min_similarity())).collect()
        };
        let mut seen = HashSet::new();
        let near: Vec<(i64, f32)> = near.into_iter().filter(|(id, _)| seen.insert(*id)).collect();
        if near.is_empty() {
            return Ok(near);
        }
        let list = near.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>().join(",");
        let visible: HashSet<i64> = {
            let mut st = self.conn.prepare(&format!("SELECT c.chunk_id FROM kb_chunks c JOIN kb_documents d ON d.doc_id = c.doc_id WHERE c.chunk_id IN ({}) AND {}", list, allowed))?;
            let rows = st.query_map([], |r| r.get::<_, i64>(0))?;
            rows.flatten().collect()
        };
        Ok(near.into_iter().filter(|(id, _)| visible.contains(id)).take(50).collect())
    }
}


// ── search ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SearchOpts {
    /// The calling agent's role: `memory/agents/<role>/` is visible only to it. `None` is the operator.
    pub role: Option<String>,
    pub limit: usize,
    /// Include documents whose provenance is untrusted or federated-peer, and inactive ones.
    pub include_untrusted: bool,
    /// Which rankings take part. Everything by default; the recall eval compares them.
    pub legs: Legs,
    /// Weight of the vector ranking in the fusion (lexical rankings weigh 1.0; definitions 2.0).
    pub vec_weight: f64,
    /// Factor on the keyword rankings when the query is a sentence rather than keywords or an identifier.
    pub natural_lex_weight: f64,
    /// Factor applied to the lexical rankings when no chunk had every query word and the any-word pass was used.
    pub loose_weight: f64,
    /// Weight of the reranker's ranking in the fusion; 0 leaves the reranker out.
    pub rerank_weight: f64,
    /// How many of the best fused results the reranker reads.
    pub rerank_top: usize,
    /// How many windows of a result the reranker reads, of about `rerank_chars` characters each; its best counts.
    pub rerank_windows: usize,
    pub rerank_chars: usize,
    /// If the best reranker score is below this, nothing is returned: the reranker has read the best the search
    /// found and none of it answers the question. `None` uses the floor the reranker's package carries.
    pub rerank_floor: Option<f32>,
    /// Only this added bundle (`org/name`), or only this org's bundles (`org`) (`specs/026` §6 AC-6).
    pub bundle: Option<String>,
    /// Leave added bundles out of the results.
    pub no_bundles: bool,
    /// Include zone `staged`: notes the daemon captured from what happened (`specs/031` §3.1). Off by default, so an
    /// ordinary search never mixes unreviewed candidate lessons into its answer.
    pub include_staged: bool,
    /// Which Xenia records a search may see (`specs/011` §5.3). The zone is invisible unless asked for, and each view
    /// is exclusive: it returns Xenia records and nothing else.
    pub xenia: XeniaView,
}

/// The three ways to see the Xenia zone. The default sees none of it, so every ordinary search, listing and read is
/// blind to org records: an agent gets L2 summaries only through [`XeniaView::Agent`], and an L1 or L0 record (or a
/// hidden one) only through [`XeniaView::Quarantine`], an explicit act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XeniaView {
    #[default]
    Off,
    /// Active `federated_verified` records.
    Agent,
    /// Everything else in the zone: unendorsed, clamped, stale-demoted, hidden.
    Quarantine,
}

impl SearchOpts {
    /// The view an agent gets: L2 summaries.
    pub fn xenia_agent(limit: usize) -> SearchOpts {
        SearchOpts { xenia: XeniaView::Agent, limit, ..SearchOpts::default() }
    }

    /// The explicit quarantine read.
    pub fn xenia_quarantine(limit: usize) -> SearchOpts {
        SearchOpts { xenia: XeniaView::Quarantine, include_untrusted: true, limit, ..SearchOpts::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Legs {
    Hybrid,
    LexicalOnly,
    VectorOnly,
}

impl Default for SearchOpts {
    fn default() -> Self {
        SearchOpts { role: None, limit: 10, include_untrusted: false, legs: Legs::Hybrid, vec_weight: VEC_WEIGHT, natural_lex_weight: NATURAL_LEX_WEIGHT, loose_weight: LOOSE_WEIGHT, rerank_weight: RERANK_WEIGHT, rerank_top: RERANK_TOP, rerank_windows: RERANK_WINDOWS, rerank_chars: RERANK_CHARS, rerank_floor: None, bundle: None, no_bundles: false, include_staged: false, xenia: XeniaView::Off }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub pointer: String,
    pub rel_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub heading_path: String,
    pub blake3: String,
    pub preview: String,
    pub score: f64,
    /// `(lexical only: vectors N% ready)` on a result from an added bundle whose chunks are not all embedded yet
    /// (`specs/026` §6): the order of results may improve when they are.
    pub note: Option<String>,
}

/// Words that carry no topic in a question. They are left out of a query unless nothing else is left, so that
/// "how are untrusted results kept out" is matched on `untrusted results kept`, not on `how are out`.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "can", "could", "do", "does", "did", "for", "from", "had", "has", "have", "how", "if", "in", "into", "is", "it", "its", "may", "of", "on", "or", "our", "should", "so", "than",
    "that", "the", "their", "them", "then", "there", "these", "they", "this", "to", "was", "we", "were", "what", "when", "where", "which", "who", "whom", "why", "will", "with", "would", "you", "your", "about", "not", "any",
];

/// Does this word look like an identifier (`H-004`, `resolve_model_route`, `specs/004`, `thoughtSignature`,
/// `core-manifest.json`) rather than a word of prose?
fn identifier_like(tok: &str) -> bool {
    let t = tok.trim_matches(|c: char| !c.is_alphanumeric());
    t.chars().any(|c| c.is_ascii_digit() || matches!(c, '_' | '.' | '/' | ':' | '@' | '#')) || t.chars().skip(1).any(|c| c.is_uppercase())
}

/// A question asked in sentences ("who splits a big task and hands the parts out") rather than a keyword query
/// (`frozen boot snapshot`) or an identifier (`[REV-SEC-04]`). For a sentence the keyword rankings mostly match
/// filler and common words, so their weight is lowered (`specs/004` §3.7); for the others they decide.
pub fn is_natural_question(q: &str) -> bool {
    let toks: Vec<&str> = q.split_whitespace().collect();
    if toks.iter().any(|t| identifier_like(t)) {
        return false;
    }
    let fillers = toks.iter().filter(|t| STOPWORDS.contains(&t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase().as_str())).count();
    toks.len() >= 5 || (toks.len() >= 3 && fillers >= 1)
}

/// Query words as FTS5 phrases: quoted so punctuation cannot be read as query syntax.
fn match_terms(q: &str) -> Vec<String> {
    let mut terms: Vec<String> = q
        .split(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | '(' | ')' | ',' | ';' | '`' | '*' | '"' | '\'' | '<' | '>' | '|' | '?' | '!'))
        .map(|t| t.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '#' || c == '@')).to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let topical: Vec<String> = terms.iter().filter(|t| !STOPWORDS.contains(&t.to_lowercase().as_str())).cloned().collect();
    if !topical.is_empty() {
        terms = topical;
    }
    terms.dedup();
    terms.truncate(16);
    terms.into_iter().map(|t| format!("\"{}\"", t.replace('"', ""))).collect()
}

impl Kb {
    fn ranked(&self, table: &str, weights: &str, expr: &str, allowed: &str) -> Result<Vec<i64>, KbError> {
        let sql = format!(
            "SELECT t.rowid FROM {t} t JOIN kb_chunks c ON c.chunk_id = t.rowid JOIN kb_documents d ON d.doc_id = c.doc_id WHERE {t} MATCH ?1 AND {allowed} ORDER BY bm25({t}{w}) LIMIT 50",
            t = table,
            w = weights,
            allowed = allowed
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([expr], |r| r.get::<_, i64>(0))?;
        Ok(rows.flatten().collect())
    }

    /// At most `limit` pointers with three-line previews, best first (`specs/004` §3.4).
    pub fn search(&self, query: &str, opts: &SearchOpts) -> Result<Vec<Hit>, KbError> {
        Ok(self.search_scored(query, opts)?.0)
    }

    /// `search`, and the best score the reranker gave any result it read (`None` when it did not run).
    pub fn search_scored(&self, query: &str, opts: &SearchOpts) -> Result<(Vec<Hit>, Option<f32>), KbError> {
        let terms = match_terms(query);
        if terms.is_empty() {
            return Ok((vec![], None));
        }
        let allowed = allowed_sql(opts);
        let mut score: HashMap<i64, f64> = HashMap::new();
        let nearest = if opts.legs != Legs::LexicalOnly { self.vector_ranked(query, opts, &allowed)? } else { vec![] };
        if opts.legs != Legs::VectorOnly {
            // Precise first (every term), then any term, so a long question still finds something.
            let mut ident = Vec::new();
            let mut prose = Vec::new();
            let (mut factor, mut loose) = (if is_natural_question(query) { opts.natural_lex_weight } else { 1.0 }, false);
            for joiner in [" AND ", " OR "] {
                let expr = terms.join(joiner);
                ident = self.ranked("kb_fts_ident", ", 2.0, 5.0, 10.0", &expr, &allowed)?;
                prose = self.ranked("kb_fts_prose", ", 1.0", &expr, &allowed)?;
                if !ident.is_empty() || !prose.is_empty() {
                    break;
                }
                // Nothing held every word: the any-word pass that follows is a weaker signal than a full match.
                (factor, loose) = if terms.len() > 1 { (factor.min(opts.loose_weight), true) } else { (factor, false) };
            }
            // With a model installed, a question that no vector is near and that only some of whose words appear
            // somewhere is not answered by those words alone: that is how off-topic questions get "results".
            if loose && opts.legs == Legs::Hybrid && self.embedder.is_some() && has_vec(&self.conn) && nearest.is_empty() {
                ident.clear();
                prose.clear();
            }
            // Chunks that *define* a queried identifier (a line opening with it) get a third, double-weight ranking.
            let defs: Vec<String> = terms.iter().map(|t| format!("\"def:{}\"", t.trim_matches('"'))).collect();
            let defined = self.ranked("kb_fts_ident", ", 0.0, 0.0, 1.0", &defs.join(" OR "), &allowed)?;
            for (r, id) in defined.iter().enumerate() {
                *score.entry(*id).or_default() += 2.0 / (RRF_K + r as f64 + 1.0);
            }
            for (r, id) in ident.iter().enumerate() {
                *score.entry(*id).or_default() += factor / (RRF_K + r as f64 + 1.0);
            }
            for (r, id) in prose.iter().enumerate() {
                *score.entry(*id).or_default() += factor / (RRF_K + r as f64 + 1.0);
            }
        }
        for (r, (id, _)) in nearest.iter().enumerate() {
            *score.entry(*id).or_default() += opts.vec_weight / (RRF_K + r as f64 + 1.0);
        }
        let mut order: Vec<(i64, f64)> = score.into_iter().collect();
        // A stale bundle's results keep a fixed share of their score: lower, never hidden (`specs/024` §4.6).
        let stale: Vec<String> = self.conn.query_row("SELECT value FROM kb_meta WHERE key = 'stale_bundles'", [], |r| r.get::<_, String>(0)).ok().map(|v| v.split(',').filter(|s| !s.is_empty()).map(|s| format!("bundles/{}/", s)).collect()).unwrap_or_default();
        if !stale.is_empty() {
            for (id, s) in order.iter_mut() {
                let rel: Option<String> = self.conn.query_row("SELECT rel_path FROM kb_chunks WHERE chunk_id = ?1", [*id], |r| r.get(0)).ok();
                if rel.is_some_and(|r| stale.iter().any(|p| r.starts_with(p.as_str()))) {
                    *s *= STALE_BUNDLE_FACTOR;
                }
            }
        }
        order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
        let mut best_rerank = None;
        if opts.legs == Legs::Hybrid && opts.rerank_weight > 0.0 {
            best_rerank = self.rerank(query, &mut order, opts)?;
            // The reranker read the best the search found and none of it answers the question.
            if let (Some(best), Some(floor)) = (best_rerank, opts.rerank_floor.or_else(|| self.reranker.as_ref().and_then(|r| r.min_score()))) {
                if best < floor {
                    order.clear();
                }
            }
        }
        let mut hits = Vec::new();
        let mut readiness: HashMap<String, Option<String>> = HashMap::new();
        for (id, s) in order.into_iter().take(opts.limit.min(10)) {
            let row = self.conn.query_row("SELECT c.rel_path, c.start_line, c.end_line, c.heading_path, c.raw_content, d.content_blake3 FROM kb_chunks c JOIN kb_documents d ON d.doc_id = c.doc_id WHERE c.chunk_id = ?1", [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?)));
            let Ok((rel_path, a, b, heading_path, raw, blake3)) = row else { continue };
            let preview = raw.lines().filter(|l| !l.trim().is_empty()).take(3).map(|l| l.chars().take(160).collect::<String>()).collect::<Vec<_>>().join("\n");
            let pointer = match rel_path.strip_prefix("bundles/") {
                Some(rest) => format!("@bundle/{}#L{}-L{}", rest, a, b), // the logical path an agent reads it by
                None => format!("{}#L{}-L{}", rel_path, a, b),
            };
            let note = match rel_path.strip_prefix("bundles/").map(|r| r.splitn(3, '/').take(2).collect::<Vec<_>>().join("/")) {
                Some(bundle) => readiness
                    .entry(bundle.clone())
                    .or_insert_with(|| self.bundle_vector_counts(&bundle).ok().filter(|(done, total)| done < total).map(|(done, total)| format!("(lexical only: vectors {}% ready)", done * 100 / total)))
                    .clone(),
                None => None,
            };
            hits.push(Hit { pointer, rel_path, start_line: a as usize, end_line: b as usize, heading_path, blake3, preview, score: s, note });
        }
        Ok((hits, best_rerank))
    }
}

impl Kb {
    /// Let the reranker read the question with each of the best fused results, and add its ranking to theirs. The
    /// reranker only reorders what the search already found and is allowed to see; if it fails the order stands.
    fn rerank(&self, query: &str, order: &mut [(i64, f64)], opts: &SearchOpts) -> Result<Option<f32>, KbError> {
        let Some(rr) = &self.reranker else { return Ok(None) };
        let top = opts.rerank_top.min(order.len());
        if top < 1 {
            return Ok(None);
        }
        let mut texts: Vec<String> = Vec::new();
        let mut owner: Vec<usize> = Vec::new();
        for (i, (id, _)) in order[..top].iter().enumerate() {
            let (heading, raw): (String, String) = self.conn.query_row("SELECT heading_path, raw_content FROM kb_chunks WHERE chunk_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            for w in embed::windows(&raw, opts.rerank_chars.max(200), opts.rerank_windows.max(1)) {
                texts.push(if heading.is_empty() { w } else { format!("{}\n{}", heading, w) });
                owner.push(i);
            }
        }
        let scores = match rr.score(query, &texts) {
            Ok(s) => s,
            Err(e) => {
                // Search carries on with the order it has, but a reranker that never works must not go unnoticed.
                static REPORTED: std::sync::Once = std::sync::Once::new();
                REPORTED.call_once(|| eprintln!("delphi kb: the reranker failed and is being skipped: {}", e));
                return Ok(None);
            }
        };
        let mut best = vec![f32::NEG_INFINITY; top];
        for (o, s) in owner.iter().zip(scores) {
            best[*o] = best[*o].max(s);
        }
        let mut by_score: Vec<usize> = (0..top).collect();
        by_score.sort_by(|a, b| best[*b].partial_cmp(&best[*a]).unwrap_or(std::cmp::Ordering::Equal));
        let top_score = best.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        for (rank, i) in by_score.into_iter().enumerate() {
            order[i].1 += opts.rerank_weight / (RRF_K + rank as f64 + 1.0);
        }
        order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
        Ok(Some(top_score))
    }
}

/// A pointer's lines still say what they said when indexed? `Err` names why not (the file changed or is gone).
/// When the file was edited, the chunk is re-found by its heading; if the heading is gone the pointer is stale.
pub fn resolve(ws: &Path, hit: &Hit) -> Result<(usize, usize), String> {
    let src = sources(ws).into_iter().find(|s| s.rel_path == hit.rel_path).ok_or_else(|| format!("{} is no longer indexed", hit.rel_path))?;
    let text = std::fs::read_to_string(&src.path).map_err(|e| e.to_string())?;
    if blake3::hash(text.as_bytes()).to_hex().as_str() == hit.blake3 {
        return Ok((hit.start_line, hit.end_line));
    }
    chunks_for(src.zone, &text).into_iter().find(|c| c.heading_path == hit.heading_path).map(|c| (c.start_line, c.end_line)).ok_or_else(|| format!("stale: the section `{}` of {} no longer exists", hit.heading_path, hit.rel_path))
}

/// What `delphi doctor` reports about the index.
#[derive(Debug)]
pub struct Health {
    pub documents: usize,
    pub chunks: usize,
    /// Chunks that have a vector.
    pub vectors: usize,
    /// The model that made the vectors in the index, and the one that is installed now.
    pub indexed_model: Option<String>,
    pub installed_model: Option<String>,
    /// The installed reranking model, if any (it has no state in the index).
    pub reranker: Option<String>,
}

pub fn health(home: &Path, ws: &Path) -> Result<Health, KbError> {
    let kb = Kb::open(&db_path(ws))?;
    kb.integrity_check()?;
    let (documents, chunks) = kb.counts()?;
    let indexed_model: Option<String> = kb.conn.query_row("SELECT value FROM kb_meta WHERE key = 'embedding_model_id'", [], |r| r.get(0)).ok();
    let vectors = if has_vec(&kb.conn) { kb.vector_counts()?.0 } else { 0 };
    Ok(Health { documents, chunks, vectors, indexed_model, installed_model: embed::installed_model_id(Some(home)), reranker: embed::installed_reranker_id(Some(home)) })
}

// ── recorded needs (Step 6 evidence) ────────────────────────────────────────

fn misses_path(ws: &Path) -> PathBuf {
    ws.join("local/kb-misses.jsonl")
}

fn append_line(path: &Path, v: &serde_json::Value) {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", v);
    }
}

/// A search that found nothing is a candidate miss; it is written down so that "vectors would have helped" can
/// become a fact (D-01's reopen trigger needs a *confirmed* miss, which the operator records with `kb miss`).
pub fn log_search(ws: &Path, query: &str, role: Option<&str>, results: usize) {
    if results == 0 {
        append_line(&misses_path(ws), &serde_json::json!({"t": now(), "query": query.chars().take(200).collect::<String>(), "role": role, "results": 0, "confirmed": false}));
    }
}

/// The operator says a search should have found something that uses different words (`delphi kb miss`).
pub fn confirm_miss(ws: &Path, query: &str, note: &str) {
    append_line(&misses_path(ws), &serde_json::json!({"t": now(), "query": query.chars().take(200).collect::<String>(), "note": note.chars().take(300).collect::<String>(), "confirmed": true}));
}

/// `(candidate misses, confirmed misses)` recorded so far.
pub fn miss_counts(ws: &Path) -> (usize, usize) {
    let text = std::fs::read_to_string(misses_path(ws)).unwrap_or_default();
    let rows: Vec<serde_json::Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    (rows.iter().filter(|r| r["confirmed"] == false).count(), rows.iter().filter(|r| r["confirmed"] == true).count())
}

// ── command line ────────────────────────────────────────────────────────────

/// Bring the workspace's lexical index in line with its files, without running a model: what `delphi bundle add` and
/// `remove` do so that the words of a bundle are searchable at once (`specs/024` §4.2 step 6).
pub fn sync_workspace(ws: &Path) -> Result<IndexReport, KbError> {
    let mut kb = Kb::open(&db_path(ws))?;
    crate::learn::project_staged(ws);
    kb.sync_lexical(&sources(ws), &now())
}

fn db_path(ws: &Path) -> PathBuf {
    ws.join("local/state.db")
}

pub(crate) fn now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}", secs)
}

/// Open the installed embedding model for `kb`, if there is one. A model that is installed but cannot be used is
/// reported once and search carries on without it.
pub fn attach_default(home: &Path, kb: &mut Kb) -> Result<bool, KbError> {
    match embed::open_default_reranker(Some(home)) {
        Ok(r) => kb.attach_reranker(r),
        Err(e) => eprintln!("delphi kb: the reranking model is not used: {}", e),
    }
    match embed::open_default(Some(home)) {
        Ok(Some(e)) => {
            kb.attach_embedder(e)?;
            Ok(true)
        }
        Ok(None) => Ok(false),
        Err(e) => {
            eprintln!("delphi kb: the embedding model is not used: {}", e);
            Ok(false)
        }
    }
}

static BACKGROUND_EMBEDDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Give chunks without vectors their vectors on a background thread (at most one per process), so that a search
/// is never held up by the model. Search uses the vectors that exist so far.
pub fn embed_in_background(home: &Path, ws: &Path) {
    use std::sync::atomic::Ordering;
    if BACKGROUND_EMBEDDING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }
    let (home, db) = (home.to_path_buf(), db_path(ws));
    std::thread::spawn(move || {
        let _ = (|| -> Result<(), KbError> {
            let mut kb = Kb::open(&db)?;
            if attach_default(&home, &mut kb)? {
                kb.embed_pending(&mut |_, _| {})?;
            }
            Ok(())
        })();
        BACKGROUND_EMBEDDING.store(false, Ordering::SeqCst);
    });
}

// ── recall evaluation ───────────────────────────────────────────────────────

/// The Markdown under `docs` that an index would hold, by logical path, without the workspace layout.
fn eval_sources(docs: &Path) -> Vec<SourceFile> {
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(docs) {
        let mut top: Vec<_> = rd.flatten().filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false) && e.file_name().to_string_lossy().ends_with(".md")).collect();
        top.sort_by_key(|e| e.file_name());
        files.extend(top.into_iter().map(|e| SourceFile { rel_path: e.file_name().to_string_lossy().to_string(), zone: "core", path: e.path(), default_source: UNTRUSTED, bundle: None }));
    }
    for (dir, zone) in [("context", "context"), ("specs", "specs"), ("missions", "missions"), ("memory", "memory")] {
        md_files(&docs.join(dir), dir, zone, UNTRUSTED, &mut files);
    }
    for dir in ["agents", "skills"] {
        md_files(&docs.join(dir), dir, dir, "user_ratified", &mut files);
    }
    files
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Recall {
    pub at1: f64,
    pub at3: f64,
    pub at5: f64,
    pub at10: f64,
    pub mrr: f64,
    /// Mean milliseconds per question.
    pub ms: f64,
}

/// One question and where its answer is. `none` marks a question the documents do not answer.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EvalCase {
    pub q: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub heading: String,
    #[serde(default)]
    pub none: bool,
}

#[derive(Debug, Default)]
pub struct EvalReport {
    pub cases: usize,
    pub per_mode: Vec<(&'static str, Recall)>,
    /// Questions the documents do not answer, and how many of them still got results, per mode.
    pub off_topic: Vec<(&'static str, usize, usize)>,
    pub misses: Vec<String>,
    /// Best vector similarity for each answerable question, and for each off-topic one (sorted, ascending).
    pub answerable_sims: Vec<f32>,
    pub off_topic_sims: Vec<f32>,
    /// The best reranker score for each answerable and each off-topic question (sorted, ascending).
    pub answerable_rerank: Vec<f32>,
    pub off_topic_rerank: Vec<f32>,
}

/// Recall of each ranking on `cases`. A result is right when it is in the expected file (if one is named) under a
/// heading containing the expected text.
pub fn evaluate(kb: &Kb, cases: &[EvalCase], base: &SearchOpts) -> Result<EvalReport, KbError> {
    let mut rep = EvalReport::default();
    let positives: Vec<&EvalCase> = cases.iter().filter(|c| !c.none).collect();
    let negatives: Vec<&EvalCase> = cases.iter().filter(|c| c.none).collect();
    rep.cases = positives.len();
    let mut modes: Vec<(&'static str, Legs, f64)> = vec![("lexical", Legs::LexicalOnly, 0.0)];
    if kb.embedder.is_some() {
        modes.push(("vector", Legs::VectorOnly, 0.0));
        modes.push(("hybrid", Legs::Hybrid, 0.0));
        if kb.reranker.is_some() {
            modes.push(("+rerank", Legs::Hybrid, base.rerank_weight));
        }
    }
    let last = modes.len() - 1;
    for (m, (name, legs, rerank_weight)) in modes.into_iter().enumerate() {
        let opts = SearchOpts { legs, include_untrusted: true, rerank_weight, ..base.clone() };
        let mut r = Recall::default();
        let started = std::time::Instant::now();
        for c in &positives {
            let hits = kb.search(&c.q, &opts)?;
            let rank = hits.iter().position(|h| (c.path.is_empty() || h.rel_path == c.path) && h.heading_path.to_lowercase().contains(&c.heading.to_lowercase()));
            if let Some(i) = rank {
                r.mrr += 1.0 / (i as f64 + 1.0);
                r.at1 += (i < 1) as u8 as f64;
                r.at3 += (i < 3) as u8 as f64;
                r.at5 += (i < 5) as u8 as f64;
                r.at10 += 1.0;
            } else if m == last {
                rep.misses.push(c.q.clone());
            }
        }
        let n = positives.len().max(1) as f64;
        rep.per_mode.push((name, Recall { at1: r.at1 / n, at3: r.at3 / n, at5: r.at5 / n, at10: r.at10 / n, mrr: r.mrr / n, ms: started.elapsed().as_secs_f64() * 1000.0 / n }));
        if !negatives.is_empty() {
            let answered = negatives.iter().filter(|c| kb.search(&c.q, &opts).map(|h| !h.is_empty()).unwrap_or(false)).count();
            rep.off_topic.push((name, answered, negatives.len()));
        }
    }
    let sims = |cs: &[&EvalCase]| {
        let mut v: Vec<f32> = cs.iter().filter_map(|c| kb.top_similarity(&c.q)).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v
    };
    rep.answerable_sims = sims(&positives);
    rep.off_topic_sims = sims(&negatives);
    if kb.reranker.is_some() {
        let rr = |cs: &[&EvalCase]| {
            let opts = SearchOpts { include_untrusted: true, rerank_floor: None, ..base.clone() };
            let mut v: Vec<f32> = cs.iter().filter_map(|c| kb.search_scored(&c.q, &opts).ok().and_then(|(_, b)| b)).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v
        };
        rep.answerable_rerank = rr(&positives);
        rep.off_topic_rerank = rr(&negatives);
    }
    Ok(rep)
}

pub(crate) fn print_eval(rep: &EvalReport) {
    println!("recall over {} question(s), top 10:", rep.cases);
    println!("  {:<8} {:>6} {:>6} {:>6} {:>6} {:>6} {:>8}", "ranking", "@1", "@3", "@5", "@10", "MRR", "ms/q");
    for (name, r) in &rep.per_mode {
        println!("  {:<8} {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>6.3} {:>8.0}", name, r.at1, r.at3, r.at5, r.at10, r.mrr, r.ms);
    }
    for (name, answered, of) in &rep.off_topic {
        println!("  {:<8} answered {} of {} off-topic question(s) (fewer is better)", name, answered, of);
    }
    let show = |v: &[f32]| if v.is_empty() { "-".to_string() } else { format!("{:.2} to {:.2}, median {:.2}", v[0], v[v.len() - 1], v[v.len() / 2]) };
    if !rep.answerable_sims.is_empty() {
        println!("  best similarity: answerable {}; off-topic {}", show(&rep.answerable_sims), show(&rep.off_topic_sims));
        let top_off = rep.off_topic_sims.last().copied().unwrap_or(0.0);
        let low_ans = rep.answerable_sims.get(rep.answerable_sims.len() / 10).copied().unwrap_or(1.0);
        println!("  a floor between {:.2} (highest off-topic) and {:.2} (10th percentile of answerable) separates them", top_off, low_ans);
    }
    if !rep.answerable_rerank.is_empty() {
        println!("  best reranker score: answerable {}; off-topic {}", show(&rep.answerable_rerank), show(&rep.off_topic_rerank));
        let top_off = rep.off_topic_rerank.last().copied().unwrap_or(f32::NEG_INFINITY);
        let low_ans = rep.answerable_rerank.get(rep.answerable_rerank.len() / 10).copied().unwrap_or(f32::INFINITY);
        println!("  a reranker floor between {:.2} (highest off-topic) and {:.2} (10th percentile of answerable) separates them", top_off, low_ans);
    }
    for q in &rep.misses {
        println!("  not in the top 10: {}", q);
    }
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn model_cmd(home: &Path, args: &[String]) -> Result<i32, KbError> {
    match args.first().map(String::as_str) {
        Some("info") | None => {
            // delphi kb model info [--check]: what is installed and where; --check also loads each model.
            let check = args.iter().any(|a| a == "--check");
            let mut broken = false;
            for (label, src, off, absent) in [
                ("embedding model", embed::locate(Some(home)), "switched off (DELPHI_EMBED_MODEL=off)", "none installed; search is lexical only (a release build has one, or install one with `delphi kb model pack`)"),
                ("reranking model", embed::locate_reranker(Some(home)), "switched off (DELPHI_RERANK_MODEL=off)", "none installed; results are not reranked"),
            ] {
                match src {
                    embed::ModelSource::Disabled => println!("{}: {}", label, off),
                    embed::ModelSource::Absent => println!("{}: {}", label, absent),
                    src => {
                        #[cfg(feature = "local-embeddings")]
                        {
                            let origin = match &src {
                                embed::ModelSource::File(p) => p.display().to_string(),
                                _ => "compiled into this binary".into(),
                            };
                            let meta = match &src {
                                embed::ModelSource::File(p) => embed::local::peek_file(p),
                                _ => (if label == "embedding model" { embed::embedded_bytes() } else { embed::embedded_reranker_bytes() }).ok_or_else(|| "no model is compiled in".to_string()).and_then(embed::local::peek),
                            };
                            match meta {
                                Ok(m) => println!("{}: {} ({}){}", label, m.id, origin, if m.source.is_empty() { String::new() } else { format!(", from {}", m.source) }),
                                Err(e) => {
                                    broken = true;
                                    println!("{}: installed at {} but unreadable: {}", label, origin, e);
                                }
                            }
                            if check {
                                let loaded = if label == "embedding model" { embed::open_default(Some(home)).map(|_| ()) } else { embed::open_default_reranker(Some(home)).map(|_| ()) };
                                match loaded {
                                    Ok(()) => println!("{}: loads and runs on {}", label, embed::local::device_name()),
                                    Err(e) => {
                                        broken = true;
                                        println!("{}: does not load: {}", label, e);
                                    }
                                }
                            }
                        }
                        #[cfg(not(feature = "local-embeddings"))]
                        println!("{}: {:?} (this build has no embedding runtime)", label, src);
                    }
                }
            }
            #[cfg(feature = "local-embeddings")]
            if let Ok(Some(e)) = embed::open_default(Some(home)).map(|o| o.filter(|_| check)) {
                match embed::vectors::load_seed(e.model_id(), e.dim(), EMBED_FORMAT) {
                    Some(c) => println!("preloaded vectors: {} chunk(s) for {}", c.len(), e.model_id()),
                    None => println!("preloaded vectors: none for {}", e.model_id()),
                }
            }
            Ok(if broken { 1 } else { 0 })
        }
        #[cfg(feature = "local-embeddings")]
        Some("score") => {
            // delphi kb model score <reranker package> <question> <passage>...: the raw scores, for checking a reranker.
            let (Some(pkg), Some(q)) = (args.get(1), args.get(2)) else {
                eprintln!("usage: delphi kb model score <reranker package> <question> <passage>...");
                return Ok(2);
            };
            let rr = embed::local::open_reranker(Path::new(pkg)).map_err(KbError::Io)?;
            let docs: Vec<String> = args[3..].to_vec();
            for (d, s) in docs.iter().zip(rr.score(q, &docs).map_err(KbError::Io)?) {
                println!("{:>9.3}  {}", s, d.chars().take(80).collect::<String>());
            }
            Ok(0)
        }
        #[cfg(feature = "local-embeddings")]
        Some("pack") => {
            let (Some(dir), Some(out)) = (args.get(1), args.get(2)) else {
                eprintln!("usage: delphi kb model pack <model dir> <out file> --id <id> --kind bert|modernbert --pooling cls|mean --dim <n> [--max-tokens <n>] [--min-similarity <f>] [--query-prefix <s>] [--doc-prefix <s>] [--source <repo@revision>]");
                return Ok(2);
            };
            let need = |n: &str| flag_value(args, n).ok_or_else(|| KbError::Io(format!("pack needs {}", n)));
            let meta = embed::local::Meta {
                id: need("--id")?,
                kind: need("--kind")?,
                pooling: flag_value(args, "--pooling").unwrap_or_else(|| "cls".into()),
                query_prefix: flag_value(args, "--query-prefix").unwrap_or_default(),
                doc_prefix: flag_value(args, "--doc-prefix").unwrap_or_default(),
                max_tokens: flag_value(args, "--max-tokens").and_then(|v| v.parse().ok()).unwrap_or(512),
                min_similarity: flag_value(args, "--min-similarity").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                dim: need("--dim")?.parse().map_err(|_| KbError::Io("--dim is a number".into()))?,
                source: flag_value(args, "--source").unwrap_or_default(),
            };
            let rep = embed::local::pack(Path::new(dir), &meta, Path::new(out)).map_err(KbError::Io)?;
            println!("packed {}: {} tensors ({} int8), {:.1} MB, largest weight error {:.5}", out, rep.tensors, rep.quantized, rep.bytes as f64 / 1e6, rep.max_abs_error);
            Ok(0)
        }
        _ => {
            eprintln!("usage: delphi kb model info | pack <model dir> <out file> --id <id> --kind <kind> --dim <n> ...");
            Ok(2)
        }
    }
}

/// `delphi kb index|reindex|search|stats|miss|eval|model` for the workspace at `ws` under `home`.
pub fn run(home: &Path, ws: &Path, args: &[String]) -> i32 {
    let sub = args.first().map(String::as_str).unwrap_or("");
    let open = || Kb::open(&db_path(ws));
    let go = || -> Result<i32, KbError> {
        match sub {
            "index" | "reindex" => {
                let mut kb = open()?;
                let model = attach_default(home, &mut kb)?;
                let files = sources(ws);
                let rep = if sub == "reindex" { kb.rebuild_lexical(&files, &now())? } else { kb.sync_lexical(&files, &now())? };
                kb.integrity_check()?;
                let (d, c) = kb.counts()?;
                println!("kb: {} document(s), {} chunk(s): {} added, {} updated, {} removed, {} unchanged", d, c, rep.added, rep.updated, rep.removed, rep.unchanged);
                if model {
                    let started = std::time::Instant::now();
                    let n = kb.embed_pending(&mut |done, total| eprint!("\rembedding {}/{} chunks", done, total))?;
                    if n > 0 {
                        eprintln!();
                        println!("kb: gave {} chunk(s) vectors in {:.0}s: {} from the preloaded vectors, {} by the model", n, started.elapsed().as_secs_f64(), kb.seeded(), n - kb.seeded());
                    }
                }
                Ok(0)
            }
            "miss" => {
                let rest: Vec<&str> = args[1..].iter().map(String::as_str).collect();
                let (note, query): (Vec<&str>, Vec<&str>) = match rest.iter().position(|a| *a == "--note") {
                    Some(i) => (rest[i + 1..].to_vec(), rest[..i].to_vec()),
                    None => (vec![], rest),
                };
                if query.is_empty() {
                    eprintln!("usage: delphi kb miss <the query that should have found it> [--note <what it was actually called>]");
                    return Ok(2);
                }
                confirm_miss(ws, &query.join(" "), &note.join(" "));
                println!("recorded. (Confirmed misses are the questions to add to `dev/kb-eval.json`, so that `delphi kb eval` can measure the fix.)");
                Ok(0)
            }
            "stats" => {
                let mut kb = open()?;
                let model = attach_default(home, &mut kb)?;
                let (d, c) = kb.counts()?;
                println!("kb: {} document(s), {} chunk(s) in {}", d, c, db_path(ws).display());
                match (model, kb.embedder()) {
                    (true, Some(e)) => {
                        let (v, _) = kb.vector_counts()?;
                        println!("kb: vectors for {} of {} chunk(s), model {}{}", v, c, e.model_id(), if v < c { " (run `delphi kb index` to embed the rest)" } else { "" });
                    }
                    _ => println!("kb: no embedding model; search is lexical only"),
                }
                // Per added bundle, so the operator can see a long local embedding move (`specs/027` §5.4).
                for b in delphi_core::bundles::active_entries(ws) {
                    let (v, c) = kb.bundle_vector_counts(&b.id())?;
                    if c > 0 {
                        println!("kb: bundle {}: vectors for {} of {} chunk(s), {}%{}", b.id(), v, c, v * 100 / c, if v < c { " (searched by words only until complete)" } else { "" });
                    }
                }
                Ok(0)
            }
            "serve" => Ok(crate::kbserve::run(home, ws, &args[1..])),
            "search" => {
                let query: Vec<&str> = args[1..].iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
                let role = args.iter().position(|a| a == "--role").and_then(|i| args.get(i + 1)).cloned();
                let bundle = args.iter().position(|a| a == "--bundle" || a == "--org").and_then(|i| args.get(i + 1)).cloned();
                let query: Vec<&str> = query.into_iter().filter(|a| Some(*a) != role.as_deref() && Some(*a) != bundle.as_deref()).collect();
                if query.is_empty() {
                    eprintln!("usage: delphi kb search <query> [--role <role>] [--untrusted] [--lexical] [--no-rerank] [--bundle <org/name> | --org <org> | --no-bundles]");
                    return Ok(2);
                }
                let mut kb = open()?;
                let model = attach_default(home, &mut kb)?;
                // The index is brought up to date first (words, and the release's preloaded vectors); the model is not
                // run here, so `kb index` is what makes vectors for anything else.
                kb.refresh(ws, &now())?;
                if model {
                    let (v, c) = kb.vector_counts()?;
                    if v < c {
                        eprintln!("note: {} of {} chunk(s) have no vector yet; run `delphi kb index` for the best results", c - v, c);
                    }
                }
                let legs = if args.iter().any(|a| a == "--lexical") { Legs::LexicalOnly } else { Legs::Hybrid };
                let mut opts = SearchOpts { role, include_untrusted: args.iter().any(|a| a == "--untrusted"), legs, bundle, no_bundles: args.iter().any(|a| a == "--no-bundles"), include_staged: args.iter().any(|a| a == "--staged"), ..Default::default() };
                if args.iter().any(|a| a == "--no-rerank") {
                    opts.rerank_weight = 0.0;
                }
                let hits = kb.search(&query.join(" "), &opts)?;
                log_search(ws, &query.join(" "), opts.role.as_deref(), hits.len());
                if hits.is_empty() {
                    println!("no results");
                }
                for (i, h) in hits.iter().enumerate() {
                    println!("{}. {}  [{}]{}", i + 1, h.pointer, if h.heading_path.is_empty() { "(top)" } else { &h.heading_path }, h.note.as_ref().map(|n| format!("  {}", n)).unwrap_or_default());
                    for l in h.preview.lines() {
                        println!("     {}", l);
                    }
                }
                Ok(0)
            }
            "eval" => {
                // delphi kb eval [--docs <dir>] [--file <cases.json>] [--db <cache file>]
                // delphi kb eval --bundle <dir> [--lexical]: the bundle's own questions against an index of only that bundle
                // (`specs/027` §7.1). Advisory: a measured result exits 0 whatever the numbers.
                if let Some(dir) = flag_value(args, "--bundle") {
                    return Ok(match crate::bundle_eval::run(home, Path::new(&dir), args.iter().any(|a| a == "--lexical"), &mut |done, total| eprint!("\rembedding {}/{} chunks", done, total)) {
                        Ok(r) => {
                            eprintln!();
                            print_eval(&r.report);
                            println!("{}", crate::bundle_eval::verdict(&r.gate));
                            0
                        }
                        Err(e) => {
                            eprintln!("delphi kb eval --bundle: {}", e);
                            2
                        }
                    });
                }
                let docs = PathBuf::from(flag_value(args, "--docs").unwrap_or_else(|| ".".into()));
                let file = flag_value(args, "--file").map(PathBuf::from).unwrap_or_else(|| docs.join("dev/kb-eval.json"));
                let text = std::fs::read_to_string(&file).map_err(|e| KbError::Io(format!("{}: {}", file.display(), e)))?;
                let cases: Vec<EvalCase> = serde_json::from_str(&text).map_err(|e| KbError::Io(format!("{}: {}", file.display(), e)))?;
                let mut kb = match flag_value(args, "--db") {
                    Some(db) => Kb::open(Path::new(&db))?,
                    None => Kb::open_in_memory()?,
                };
                if !args.iter().any(|a| a == "--lexical") {
                    attach_default(home, &mut kb)?;
                }
                let num = |n: &str| flag_value(args, n).and_then(|v| v.parse::<f64>().ok());
                kb.set_floor(num("--floor").map(|f| f as f32));
                let mut base = SearchOpts::default();
                base.vec_weight = num("--vec-weight").unwrap_or(base.vec_weight);
                base.loose_weight = num("--loose-weight").unwrap_or(base.loose_weight);
                base.natural_lex_weight = num("--nl-weight").unwrap_or(base.natural_lex_weight);
                base.rerank_weight = num("--rerank-weight").unwrap_or(base.rerank_weight);
                base.rerank_top = num("--rerank-top").map(|n| n as usize).unwrap_or(base.rerank_top);
                base.rerank_windows = num("--rerank-windows").map(|n| n as usize).unwrap_or(base.rerank_windows);
                base.rerank_chars = num("--rerank-chars").map(|n| n as usize).unwrap_or(base.rerank_chars);
                base.rerank_floor = num("--rerank-floor").map(|f| f as f32).or(base.rerank_floor);
                let started = std::time::Instant::now();
                kb.sync_lexical(&eval_sources(&docs), &now())?;
                let (d, c) = kb.counts()?;
                eprintln!("indexed {} document(s), {} chunk(s)", d, c);
                kb.embed_pending(&mut |done, total| eprint!("\rembedding {}/{} chunks", done, total))?;
                eprintln!("\nindexed in {:.0}s", started.elapsed().as_secs_f64());
                print_eval(&evaluate(&kb, &cases, &base)?);
                Ok(0)
            }
            "seed-export" => {
                // delphi kb seed-export <out>: the vectors of this workspace's chunks, to ship as preloaded vectors.
                let Some(out) = args.get(1) else {
                    eprintln!("usage: delphi kb seed-export <out file>   (run `delphi kb index` first, with the model to ship)");
                    return Ok(2);
                };
                let mut kb = open()?;
                if !attach_default(home, &mut kb)? {
                    eprintln!("delphi kb: no embedding model is installed, so there are no vectors to export");
                    return Ok(1);
                }
                let (v, c) = kb.vector_counts()?;
                if v < c {
                    eprintln!("delphi kb: {} of {} chunk(s) have no vector; run `delphi kb index` first", c - v, c);
                    return Ok(1);
                }
                let bytes = kb.export_vectors()?;
                std::fs::write(out, &bytes).map_err(|e| KbError::Io(format!("{}: {}", out, e)))?;
                println!("wrote {} ({:.1} KB) for {}", out, bytes.len() as f64 / 1024.0, kb.embedder().map(|e| e.model_id()).unwrap_or(""));
                Ok(0)
            }
            "model" => model_cmd(home, &args[1..]),
            _ => {
                eprintln!("usage: delphi kb index | reindex | stats | search <query> [--role <role>] [--untrusted] [--lexical] [--no-rerank] | serve | miss | eval [--bundle <dir>] | model | seed-export <out>");
                Ok(2)
            }
        }
    };
    match go() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi kb: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\nid: x\nsource_type: internal_generated\nauthor: orchestrator\nstatus: active\n---\n\n# Title\n\nIntro text.\n\n## Rubric\n\n- **`[REV-SEC-04]` Ratified Rubric Loading:** audits load REVIEW.md from ratified copies.\n- `[REV-ARCH-05]` Names.\n\n```text\n## not a heading\n```\n\n### Deep\n\nThe `Part.thoughtSignature` field must round-trip.\n";

    fn kb_with(docs: &[(&str, &str, &str)]) -> Kb {
        let mut kb = Kb::open_in_memory().unwrap();
        for (p, z, t) in docs {
            kb.upsert(p, z, t, "t").unwrap();
        }
        kb
    }

    #[test]
    fn chunks_follow_headings_with_exact_lines_and_breadcrumbs_and_keep_fences_whole() {
        let c = chunk_markdown(DOC);
        let paths: Vec<&str> = c.iter().map(|c| c.heading_path.as_str()).collect();
        assert_eq!(paths, vec!["Title", "Title > Rubric", "Title > Rubric > Deep"]);
        let rubric = &c[1];
        assert_eq!((rubric.start_line, rubric.end_line), (12, 19));
        assert!(rubric.raw.contains("## not a heading"), "a heading inside a fence does not split");
        assert_eq!(c[0].start_line, 8, "frontmatter lines are counted but not indexed");
        assert!(chunk_markdown("").is_empty() && chunk_markdown("---\na: b\n---\n").is_empty());
        // A long section splits at blank lines, never inside a fence.
        let long = format!("# T\n\n{}\n```\n{}\n```\n\n{}", "para\n\n".repeat(900), "code\n".repeat(50), "tail\n\n".repeat(10));
        let parts = chunk_markdown(&long);
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|p| p.raw.matches("```").count() % 2 == 0), "no fence is split across chunks");
        assert!(parts.windows(2).all(|w| w[0].end_line < w[1].start_line));
    }

    #[test]
    fn identifiers_keep_technical_tokens_whole_and_weight_definitions() {
        let ids = identifiers("- **`[REV-SEC-04]` Ratified:** loads REVIEW.md and Part.thoughtSignature via specs/004.\nSee [H-004] and snake_case_name.");
        let toks: Vec<&str> = ids.split(' ').collect();
        for want in ["REV-SEC-04", "Part.thoughtSignature", "specs/004", "H-004", "snake_case_name", "REVIEW.md"] {
            assert!(toks.contains(&want), "{} in {:?}", want, toks);
        }
        assert!(toks.contains(&"def:REV-SEC-04"), "the opening token is marked as a definition: {:?}", toks);
        assert!(!toks.iter().any(|t| t.starts_with("def:H-004")), "a mere mention is not");
    }

    #[test]
    fn an_exact_identifier_ranks_the_defining_chunk_first() {
        let mentions = "---\nsource_type: user_ratified\n---\n# Spec\n\nRatified rubric loading is described by REV-SEC-04 and other rubric loading rules, a long list of them: REV-SEC-04 again.\n";
        let kb = kb_with(&[("REVIEW.md", "core", DOC), ("specs/009.md", "specs", mentions)]);
        let hits = kb.search("[REV-SEC-04]", &SearchOpts::default()).unwrap();
        assert_eq!(hits[0].rel_path, "REVIEW.md", "{:?}", hits.iter().map(|h| &h.pointer).collect::<Vec<_>>());
        assert_eq!(hits[0].pointer, "REVIEW.md#L12-L19");
        let hits = kb.search("Part.thoughtSignature", &SearchOpts::default()).unwrap();
        assert_eq!(hits[0].heading_path, "Title > Rubric > Deep");
        assert!(hits[0].preview.lines().count() <= 3);
        assert!(kb.search("???", &SearchOpts::default()).unwrap().is_empty());
        // Query syntax cannot be injected.
        assert!(kb.search("\" OR NEAR( ) AND *", &SearchOpts::default()).is_ok());
    }

    #[test]
    fn prose_matches_stem_and_a_long_question_still_finds_something() {
        let kb = kb_with(&[("context/a.md", "context", "---\nsource_type: user_ratified\n---\n# A\n\nThe retry ladder escalates after three failed attempts.\n")]);
        assert_eq!(kb.search("escalating retries", &SearchOpts::default()).unwrap().len(), 1, "porter stemming");
        assert_eq!(kb.search("what happens to the retry ladder when unrelated words appear", &SearchOpts::default()).unwrap().len(), 1, "falls back to any term");
    }

    #[test]
    fn agents_see_only_their_own_memory_and_nothing_untrusted_or_inactive_by_default() {
        let mk = |body: &str, status: &str, src: &str| format!("---\nsource_type: {}\nstatus: {}\n---\n# L\n\n{}\n", src, status, body);
        let kb = kb_with(&[
            ("memory/agents/architect/lessons.md", "memory", &mk("zebra pattern lesson", "active", "internal_empirical")),
            ("memory/agents/builder/lessons.md", "memory", &mk("zebra pattern lesson", "active", "internal_empirical")),
            ("context/web.md", "context", &mk("zebra from the web", "active", "external_untrusted")),
            ("context/old.md", "context", &mk("zebra superseded", "superseded", "user_ratified")),
            ("context/ok.md", "context", &mk("zebra ratified", "active", "user_ratified")),
        ]);
        let paths = |o: &SearchOpts| kb.search("zebra", o).unwrap().into_iter().map(|h| h.rel_path).collect::<Vec<_>>();
        let mut architect = paths(&SearchOpts { role: Some("architect".into()), ..Default::default() });
        architect.sort();
        assert_eq!(architect, vec!["context/ok.md", "memory/agents/architect/lessons.md"]);
        let operator = paths(&SearchOpts::default());
        assert!(operator.contains(&"memory/agents/builder/lessons.md".to_string()), "the operator sees every role's memory");
        assert!(!operator.contains(&"context/web.md".to_string()) && !operator.contains(&"context/old.md".to_string()));
        assert_eq!(paths(&SearchOpts { include_untrusted: true, ..Default::default() }).len(), 5);
        // A role name cannot be used to widen the filter.
        assert!(paths(&SearchOpts { role: Some("%".into()), ..Default::default() }).iter().all(|p| !p.starts_with("memory/")));
    }

    #[test]
    fn insert_update_and_delete_keep_both_indexes_in_sync() {
        let mut kb = Kb::open_in_memory().unwrap();
        let t = |body: &str| format!("---\nsource_type: user_ratified\n---\n# H\n\n{}\n", body);
        assert!(kb.upsert("a.md", "core", &t("alpha token"), "t").unwrap());
        assert!(!kb.upsert("a.md", "core", &t("alpha token"), "t").unwrap(), "unchanged content is skipped by hash");
        kb.integrity_check().unwrap();
        assert_eq!(kb.search("alpha", &SearchOpts::default()).unwrap().len(), 1);
        assert!(kb.upsert("a.md", "core", &t("beta token"), "t").unwrap());
        kb.integrity_check().unwrap();
        assert!(kb.search("alpha", &SearchOpts::default()).unwrap().is_empty(), "the old text is gone from both indexes");
        assert_eq!(kb.search("beta", &SearchOpts::default()).unwrap().len(), 1);
        assert!(kb.remove("a.md").unwrap());
        kb.integrity_check().unwrap();
        assert_eq!(kb.counts().unwrap(), (0, 0));
        assert!(kb.search("beta", &SearchOpts::default()).unwrap().is_empty());
    }

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (p, t) in files {
            let path = d.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, t).unwrap();
        }
        d
    }

    #[test]
    fn a_workspace_indexes_its_markdown_incrementally_and_rebuilds_identically() {
        let ok = |b: &str| format!("---\nsource_type: user_ratified\n---\n# T\n\n{}\n", b);
        let d = tree(&[
            ("base/GOVERNANCE.md", &ok("constitution words")),
            ("base/context/index.md", &ok("base context")),
            ("team/context/index.md", &ok("team context wins")),
            ("team/specs/001-x.md", &ok("spec text")),
            ("local/missions/m1/councils/001-c.md", &ok("council text")),
            ("local/memory/agents/builder/lessons.md", &ok("builder lesson")),
            ("local/state.db-notes.txt", "not markdown"),
            ("team/context/.hidden.md", &ok("hidden")),
            ("team/worktrees-ignored.txt", "x"),
        ]);
        std::os::unix::fs::symlink("/etc/hosts", d.path().join("team/context/link.md")).unwrap();
        let files = sources(d.path());
        let names: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(names, vec!["GOVERNANCE.md", "context/index.md", "memory/agents/builder/lessons.md", "missions/m1/councils/001-c.md", "specs/001-x.md"]);
        assert!(files.iter().find(|f| f.rel_path == "context/index.md").unwrap().path.starts_with(d.path().join("team")), "the team copy wins");
        let mut kb = Kb::open(&d.path().join("local/state.db")).unwrap();
        let first = kb.sync(&files, "t").unwrap();
        assert_eq!((first.added, first.updated, first.removed, first.unchanged), (5, 0, 0, 0));
        assert_eq!(kb.sync(&files, "t").unwrap().unchanged, 5, "nothing changed, nothing re-indexed");
        std::fs::write(d.path().join("team/specs/001-x.md"), ok("spec text edited")).unwrap();
        std::fs::remove_file(d.path().join("team/context/index.md")).unwrap();
        let second = kb.sync(&sources(d.path()), "t").unwrap();
        assert_eq!((second.updated, second.removed), (2, 0), "the base copy now shows through, so the context file is updated, not removed");
        kb.integrity_check().unwrap();
        let before: Vec<String> = kb.search("spec", &SearchOpts::default()).unwrap().into_iter().map(|h| h.pointer).collect();
        // Delete the whole database and rebuild from the files.
        drop(kb);
        std::fs::remove_file(d.path().join("local/state.db")).unwrap();
        let mut kb = Kb::open(&d.path().join("local/state.db")).unwrap();
        kb.rebuild(&sources(d.path()), "t").unwrap();
        kb.integrity_check().unwrap();
        assert_eq!(kb.search("spec", &SearchOpts::default()).unwrap().into_iter().map(|h| h.pointer).collect::<Vec<_>>(), before);
        // A rebuild over a populated index is the same.
        kb.rebuild(&sources(d.path()), "t").unwrap();
        kb.integrity_check().unwrap();
        assert_eq!(kb.counts().unwrap().0, 5);
    }

    #[test]
    fn a_pointer_reanchors_by_heading_after_an_edit_and_reports_stale_when_the_heading_is_gone() {
        let t = |extra: &str, h: &str| format!("---\nsource_type: user_ratified\n---\n{}# T\n\n## {}\n\nfindme body\n", extra, h);
        let d = tree(&[("team/specs/001-x.md", &t("", "Sec"))]);
        let mut kb = Kb::open_in_memory().unwrap();
        kb.sync(&sources(d.path()), "t").unwrap();
        let hit = kb.search("findme", &SearchOpts::default()).unwrap().remove(0);
        assert_eq!(resolve(d.path(), &hit), Ok((hit.start_line, hit.end_line)));
        std::fs::write(d.path().join("team/specs/001-x.md"), t("\nadded line\n\n", "Sec")).unwrap();
        let moved = resolve(d.path(), &hit).unwrap();
        assert!(moved.0 > hit.start_line, "{:?} vs {:?}", moved, (hit.start_line, hit.end_line));
        std::fs::write(d.path().join("team/specs/001-x.md"), t("", "Renamed")).unwrap();
        assert!(resolve(d.path(), &hit).unwrap_err().contains("stale"));
    }

    fn concepts() -> Arc<dyn Embedder> {
        Arc::new(crate::embed::testing::ConceptEmbedder::new(&[&["hang", "hangs", "deadlock", "stuck", "frozen"], &["subprocess", "pipe", "buffer", "child"], &["zebra", "stripes"]]))
    }

    fn ok_doc(body: &str) -> String {
        format!("---\nsource_type: user_ratified\n---\n# Note\n\n{}\n", body)
    }

    #[test]
    fn a_query_in_other_words_finds_the_note_that_lexical_search_cannot() {
        let mut kb = kb_with(&[("missions/m1/note.md", "missions", &ok_doc("The pipe buffer deadlock stalled the child.")), ("missions/m2/other.md", "missions", &ok_doc("Unrelated notes on release packaging."))]);
        kb.attach_embedder(concepts()).unwrap();
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 2);
        assert_eq!(kb.vector_counts().unwrap(), (2, 2));
        let lexical = SearchOpts { legs: Legs::LexicalOnly, ..Default::default() };
        assert!(kb.search("subprocess hangs frozen", &lexical).unwrap().is_empty(), "no word in common");
        let hits = kb.search("subprocess hangs frozen", &SearchOpts::default()).unwrap();
        assert_eq!(hits.first().map(|h| h.rel_path.as_str()), Some("missions/m1/note.md"), "{:?}", hits);
        assert_eq!(hits.len(), 1, "the unrelated note is under the model's similarity floor");
        // Exact identifiers still win through the lexical rankings when both are in play.
        assert_eq!(kb.search("release packaging", &SearchOpts::default()).unwrap()[0].rel_path, "missions/m2/other.md");
    }

    #[test]
    fn vectors_obey_the_same_visibility_rules_as_words() {
        let mk = |status: &str, src: &str| format!("---\nsource_type: {}\nstatus: {}\n---\n# L\n\nzebra stripes\n", src, status);
        let mut kb = kb_with(&[
            ("memory/agents/architect/lessons.md", "memory", &mk("active", "internal_empirical")),
            ("memory/agents/builder/lessons.md", "memory", &mk("active", "internal_empirical")),
            ("context/web.md", "context", &mk("active", "external_untrusted")),
            ("context/old.md", "context", &mk("superseded", "user_ratified")),
            ("context/ok.md", "context", &mk("active", "user_ratified")),
        ]);
        kb.attach_embedder(concepts()).unwrap();
        kb.embed_pending(&mut |_, _| {}).unwrap();
        let paths = |o: SearchOpts| {
            let mut p: Vec<String> = kb.search("striped zebra", &SearchOpts { legs: Legs::VectorOnly, ..o }).unwrap().into_iter().map(|h| h.rel_path).collect();
            p.sort();
            p
        };
        assert_eq!(paths(SearchOpts { role: Some("architect".into()), ..Default::default() }), vec!["context/ok.md", "memory/agents/architect/lessons.md"]);
        assert_eq!(paths(SearchOpts { role: Some("%".into()), ..Default::default() }), vec!["context/ok.md"], "a role name cannot widen the filter");
        assert_eq!(paths(SearchOpts::default()).len(), 3, "the operator sees every role's memory, nothing untrusted or inactive");
        assert_eq!(paths(SearchOpts { include_untrusted: true, ..Default::default() }).len(), 5);
    }

    #[test]
    fn vectors_follow_edits_and_deletes_and_a_new_model_refills_them() {
        let mut kb = Kb::open_in_memory().unwrap();
        kb.attach_embedder(concepts()).unwrap();
        kb.upsert("a.md", "core", &ok_doc("zebra stripes"), "t").unwrap();
        kb.upsert("b.md", "core", &ok_doc("pipe deadlock"), "t").unwrap();
        kb.embed_pending(&mut |_, _| {}).unwrap();
        assert_eq!(kb.vector_counts().unwrap(), (2, 2));
        kb.upsert("a.md", "core", &ok_doc("stuck child"), "t").unwrap();
        assert_eq!(kb.vector_counts().unwrap(), (1, 2), "the edited chunk lost its old vector");
        kb.remove("b.md").unwrap();
        assert_eq!(kb.vector_counts().unwrap(), (0, 1));
        let left: i64 = kb.conn.query_row("SELECT COUNT(*) FROM kb_vec", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0, "no orphaned vectors");
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 1, "only the chunk without a vector is embedded");
        // Another model: everything is embedded again, and the old vectors are gone.
        struct Other(Arc<dyn Embedder>);
        impl Embedder for Other {
            fn model_id(&self) -> &str {
                "another-model"
            }
            fn dim(&self) -> usize {
                self.0.dim()
            }
            fn min_similarity(&self) -> f32 {
                self.0.min_similarity()
            }
            fn embed_documents(&self, t: &[String]) -> Result<Vec<Vec<f32>>, String> {
                self.0.embed_documents(t)
            }
            fn embed_query(&self, t: &str) -> Result<Vec<f32>, String> {
                self.0.embed_query(t)
            }
        }
        kb.attach_embedder(Arc::new(Other(concepts()))).unwrap();
        assert_eq!(kb.vector_counts().unwrap(), (0, 1));
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 1);
        // An index with vectors still works when opened without a model: lexical results are unchanged.
        let hits = kb.search("stuck", &SearchOpts::default()).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn a_sentence_is_told_from_keywords_and_from_an_identifier() {
        for q in ["who splits a big task and hands the parts out", "what is the builder's job title", "which agent may use the tdd-execution skill", "how are keyword and similarity results combined"] {
            assert!(is_natural_question(q), "{}", q);
        }
        for q in ["frozen boot snapshot", "delphi update state machine", "Sandwich Prompting", "[REV-SEC-04]", "resolve_model_route", "Rule of 25", "ExpeL contrastive distillation", "how does specs/004 work in practice", "embedded core seed core-manifest.json", "unicode61 tokenchars", "what is H-004"] {
            assert!(!is_natural_question(q), "{}", q);
        }
    }

    #[test]
    fn filler_words_are_left_out_of_a_question_unless_nothing_else_is_left() {
        assert_eq!(match_terms("how are untrusted results kept out?"), vec!["\"untrusted\"", "\"results\"", "\"kept\"", "\"out\""]);
        assert_eq!(match_terms("what is the"), vec!["\"what\"", "\"is\"", "\"the\""], "a query made only of filler is used as it is");
        assert_eq!(match_terms("[H-004]"), vec!["\"H-004\""]);
    }

    #[test]
    fn a_loose_word_match_is_not_an_answer_when_a_model_finds_nothing_near() {
        let mut kb = kb_with(&[("context/a.md", "context", &ok_doc("The retry ladder escalates after three failed attempts."))]);
        let lexical = || kb.search("retry unicorn zebra", &SearchOpts::default()).unwrap().len();
        assert_eq!(lexical(), 1, "without a model, any matching word is a result");
        kb.attach_embedder(concepts()).unwrap();
        kb.embed_pending(&mut |_, _| {}).unwrap();
        assert!(kb.search("retry unicorn zebra", &SearchOpts::default()).unwrap().is_empty(), "with a model, a loose match that nothing is near is dropped");
        assert_eq!(kb.search("retry unicorn zebra", &SearchOpts { legs: Legs::LexicalOnly, ..Default::default() }).unwrap().len(), 1, "unless words alone were asked for");
        assert_eq!(kb.search("retry ladder", &SearchOpts::default()).unwrap().len(), 1, "a full match always counts");
    }

    #[test]
    fn health_counts_what_is_indexed_and_checks_the_full_text_tables() {
        let d = tree(&[("local/missions/m1/a.md", &ok_doc("alpha"))]);
        let mut kb = Kb::open(&d.path().join("local/state.db")).unwrap();
        kb.sync_lexical(&sources(d.path()), "t").unwrap();
        drop(kb);
        let h = health(d.path(), d.path()).unwrap();
        assert_eq!((h.documents, h.chunks, h.vectors), (1, 1, 0));
        assert_eq!(h.indexed_model, None);
    }

    #[test]
    fn a_long_section_is_embedded_in_windows_and_found_by_its_tail() {
        let long = format!("{}\n\nThe tail says the zebra is frozen.\n", "filler words about nothing in particular\n\n".repeat(120));
        let mut kb = kb_with(&[("context/long.md", "context", &ok_doc(&long))]);
        kb.attach_embedder(concepts()).unwrap();
        kb.embed_pending(&mut |_, _| {}).unwrap();
        let windows: i64 = kb.conn.query_row("SELECT windows FROM kb_chunk_vec", [], |r| r.get(0)).unwrap();
        assert!(windows > 1, "{} window(s)", windows);
        assert_eq!(kb.search("zebra frozen", &SearchOpts { legs: Legs::VectorOnly, ..Default::default() }).unwrap().len(), 1);
    }

    /// Counts the texts the model is asked to read.
    struct Counting {
        inner: Arc<dyn Embedder>,
        texts: std::sync::atomic::AtomicUsize,
    }

    impl Embedder for Counting {
        fn model_id(&self) -> &str {
            self.inner.model_id()
        }
        fn dim(&self) -> usize {
            self.inner.dim()
        }
        fn min_similarity(&self) -> f32 {
            self.inner.min_similarity()
        }
        fn embed_documents(&self, t: &[String]) -> Result<Vec<Vec<f32>>, String> {
            self.texts.fetch_add(t.len(), std::sync::atomic::Ordering::SeqCst);
            self.inner.embed_documents(t)
        }
        fn embed_query(&self, t: &str) -> Result<Vec<f32>, String> {
            self.inner.embed_query(t)
        }
    }

    fn counting() -> Arc<Counting> {
        Arc::new(Counting { inner: concepts(), texts: Default::default() })
    }

    #[test]
    fn preloaded_vectors_replace_the_model_for_unchanged_text_and_only_for_it() {
        let docs = |extra: &str| vec![("context/a.md", "context", ok_doc("pipe buffer deadlock stalled the child")), ("context/b.md", "context", ok_doc(&format!("zebra stripes{}", extra)))];
        let build = |docs: &[(&str, &str, String)], model: Arc<Counting>, seed: Option<Arc<embed::vectors::VecCache>>| {
            let mut kb = Kb::open_in_memory().unwrap();
            kb.attach_embedder(model).unwrap();
            kb.set_seed_vectors(seed);
            for (p, z, t) in docs {
                kb.upsert(p, z, t, "t").unwrap();
            }
            kb.embed_pending(&mut |_, _| {}).unwrap();
            kb
        };
        // The release job: embed the shipped documents and export the vectors.
        let maker = counting();
        let made = build(&docs(""), maker.clone(), None);
        assert_eq!(maker.texts.load(std::sync::atomic::Ordering::SeqCst), 2);
        let cache = Arc::new(embed::vectors::VecCache::parse(&made.export_vectors().unwrap()).unwrap());
        assert_eq!(cache.len(), 2);
        // A fresh workspace: nothing is read by the model, and search works at once.
        let user = counting();
        let kb = build(&docs(""), user.clone(), Some(cache.clone()));
        assert_eq!(user.texts.load(std::sync::atomic::Ordering::SeqCst), 0, "the model was not run");
        assert_eq!(kb.seeded(), 2);
        assert_eq!(kb.vector_counts().unwrap(), (2, 2));
        let hits = kb.search("subprocess hangs frozen", &SearchOpts::default()).unwrap();
        assert_eq!(hits[0].rel_path, "context/a.md");
        // Before a search the cache alone is applied, without loading or running the model, and a chunk it does not
        // cover is left for the model rather than stalling the pass.
        let quick = counting();
        let mut kb = Kb::open_in_memory().unwrap();
        kb.attach_embedder(quick.clone()).unwrap();
        kb.set_seed_vectors(Some(cache.clone()));
        for (p, z, t) in docs(" plus a note the cache has never seen") {
            kb.upsert(p, z, &t, "t").unwrap();
        }
        assert_eq!(kb.apply_seed_vectors().unwrap(), 1, "only the unchanged document is covered");
        assert_eq!(kb.vector_counts().unwrap(), (1, 2));
        assert_eq!(quick.texts.load(std::sync::atomic::Ordering::SeqCst), 0, "the model was not run");
        assert_eq!(kb.embed_pending(&mut |_, _| {}).unwrap(), 1, "the model reads what the cache lacked");
        assert_eq!(quick.texts.load(std::sync::atomic::Ordering::SeqCst), 1);
        // The operator edits one document: that one is read by the model, the other still comes from the cache.
        let edited = counting();
        let kb = build(&docs(" and more words"), edited.clone(), Some(cache));
        assert_eq!(edited.texts.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(kb.seeded(), 1);
    }

    /// A reranker that likes passages containing "gold", or one that always fails.
    struct Gold {
        broken: bool,
        read: std::sync::atomic::AtomicUsize,
    }

    impl Reranker for Gold {
        fn model_id(&self) -> &str {
            "test-gold"
        }
        fn score(&self, _q: &str, docs: &[String]) -> Result<Vec<f32>, String> {
            self.read.fetch_add(docs.len(), std::sync::atomic::Ordering::SeqCst);
            if self.broken {
                return Err("the model is not there".into());
            }
            Ok(docs.iter().map(|d| if d.contains("gold") { 8.0 } else { -8.0 }).collect())
        }
    }

    fn zebra_docs() -> Kb {
        // Lexically the first document is the best match for "zebra"; the one with the gold is not.
        kb_with(&[
            ("context/a.md", "context", &ok_doc("zebra zebra zebra zebra zebra")),
            ("context/b.md", "context", &ok_doc("zebra and the gold")),
            ("context/c.md", "context", &ok_doc("zebra zebra stripes")),
        ])
    }

    #[test]
    fn the_reranker_reorders_the_best_results_and_can_be_left_out() {
        let mut kb = zebra_docs();
        let first = |kb: &Kb, o: &SearchOpts| kb.search("zebra", o).unwrap()[0].rel_path.clone();
        assert_eq!(first(&kb, &SearchOpts::default()), "context/a.md", "without a reranker the lexical order stands");
        let gold = Arc::new(Gold { broken: false, read: Default::default() });
        kb.attach_reranker(Some(gold.clone()));
        let strong = SearchOpts { rerank_weight: 30.0, ..Default::default() };
        assert_eq!(first(&kb, &strong), "context/b.md", "the reranker moved the passage it prefers to the top");
        assert!(gold.read.load(std::sync::atomic::Ordering::SeqCst) >= 3);
        let off = SearchOpts { rerank_weight: 0.0, ..Default::default() };
        let before = gold.read.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(first(&kb, &off), "context/a.md", "weight 0 leaves the reranker out");
        assert_eq!(gold.read.load(std::sync::atomic::Ordering::SeqCst), before, "and it is not even asked");
        // It reads only as many results as it is allowed to.
        let before = gold.read.load(std::sync::atomic::Ordering::SeqCst);
        kb.search("zebra", &SearchOpts { rerank_top: 2, rerank_weight: 30.0, ..Default::default() }).unwrap();
        assert_eq!(gold.read.load(std::sync::atomic::Ordering::SeqCst) - before, 2);
        // Only the hybrid search is reranked; asking for words alone is not.
        assert_eq!(kb.search("zebra", &SearchOpts { legs: Legs::LexicalOnly, ..strong.clone() }).unwrap()[0].rel_path, "context/a.md");
    }

    #[test]
    fn a_reranker_score_floor_returns_nothing_when_no_passage_answers() {
        let mut kb = kb_with(&[("context/a.md", "context", &ok_doc("zebra zebra zebra")), ("context/b.md", "context", &ok_doc("zebra stripes"))]);
        kb.attach_reranker(Some(Arc::new(Gold { broken: false, read: Default::default() })));
        // Neither passage has the gold, so the best score is -8.
        let (hits, best) = kb.search_scored("zebra", &SearchOpts::default()).unwrap();
        assert_eq!((hits.len(), best), (2, Some(-8.0)));
        assert!(kb.search("zebra", &SearchOpts { rerank_floor: Some(-5.0), ..Default::default() }).unwrap().is_empty());
        assert_eq!(kb.search("zebra", &SearchOpts { rerank_floor: Some(-9.0), ..Default::default() }).unwrap().len(), 2);
    }

    #[test]
    fn a_failing_reranker_is_skipped_and_search_still_answers() {
        let mut kb = zebra_docs();
        kb.attach_reranker(Some(Arc::new(Gold { broken: true, read: Default::default() })));
        let hits = kb.search("zebra", &SearchOpts { rerank_weight: 30.0, ..Default::default() }).unwrap();
        assert_eq!(hits[0].rel_path, "context/a.md", "the order the search had stands");
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn a_search_that_found_nothing_is_a_candidate_miss_and_the_operator_can_confirm_one() {
        let d = tempfile::tempdir().unwrap();
        log_search(d.path(), "pipe buffer deadlock", None, 0);
        assert_eq!(miss_counts(d.path()), (1, 0), "an empty search alone is only a candidate");
        confirm_miss(d.path(), "subprocess hang", "it was filed as pipe buffer deadlock");
        // Searches that found something are not recorded at all.
        log_search(d.path(), "found it", None, 3);
        assert_eq!(miss_counts(d.path()), (1, 1));
    }

    #[test]
    fn a_search_sees_files_that_changed_without_reading_those_that_did_not() {
        let ok = |b: &str| format!("---\nsource_type: user_ratified\n---\n# T\n\n{}\n", b);
        let d = tree(&[("team/context/a.md", &ok("alpha words")), ("team/context/b.md", &ok("beta words")), ("local/.keep", "")]);
        let mut kb = Kb::open(&d.path().join("local/state.db")).unwrap();
        let first = kb.refresh(d.path(), "t").unwrap();
        assert_eq!((first.added, first.unchanged), (2, 0));
        let again = kb.refresh(d.path(), "t").unwrap();
        assert_eq!((again.added, again.updated, again.unchanged), (0, 0, 2), "nothing changed, nothing read");
        // A file the index has stamped is not read: replace its text but keep its size and modification time,
        // and the index (by design) still holds the old text until `reindex`.
        let a = d.path().join("team/context/a.md");
        let stamp = std::fs::metadata(&a).unwrap().modified().unwrap();
        std::fs::write(&a, ok("omega words")).unwrap();
        std::fs::File::options().write(true).open(&a).unwrap().set_modified(stamp).unwrap();
        kb.refresh(d.path(), "t").unwrap();
        assert_eq!(kb.search("alpha", &SearchOpts::default()).unwrap().len(), 1, "the fast path trusts an unchanged stamp");
        kb.rebuild(&sources(d.path()), "t").unwrap();
        assert_eq!(kb.search("omega", &SearchOpts::default()).unwrap().len(), 1, "a reindex reads everything");
        // A real edit (new text, new size and time) is picked up by the next search; so are new and removed files.
        std::fs::write(&a, ok("gamma words and more")).unwrap();
        std::fs::write(d.path().join("team/context/c.md"), ok("delta words")).unwrap();
        std::fs::remove_file(d.path().join("team/context/b.md")).unwrap();
        let rep = kb.refresh(d.path(), "t").unwrap();
        assert_eq!((rep.added, rep.updated, rep.removed), (1, 1, 1));
        assert_eq!(kb.search("gamma", &SearchOpts::default()).unwrap().len(), 1);
        assert_eq!(kb.search("delta", &SearchOpts::default()).unwrap().len(), 1);
        assert!(kb.search("beta", &SearchOpts::default()).unwrap().is_empty());
        kb.integrity_check().unwrap();
    }

    #[test]
    fn an_index_made_with_older_chunking_rules_is_chunked_again_once() {
        let manifest = "---\nid: builder\nrole: \"Lead Engineer\"\n---\n# Builder\n\nWrites code.\n";
        let d = tree(&[("base/agents/builder/AGENT.md", manifest), ("local/.keep", "")]);
        let db = d.path().join("local/state.db");
        {
            let mut kb = Kb::open(&db).unwrap();
            kb.refresh(d.path(), "t").unwrap();
            // Make it look like an index from before summaries: no summary chunk, and the old version recorded.
            kb.conn.execute("DELETE FROM kb_chunks WHERE heading_path = ?1", [SUMMARY_HEADING]).unwrap();
            kb.conn.execute("UPDATE kb_meta SET value = '1' WHERE key = 'chunker_version'", []).unwrap();
        }
        let mut kb = Kb::open(&db).unwrap();
        let rep = kb.refresh(d.path(), "t").unwrap();
        assert_eq!(rep.updated, 1, "the unchanged file was read again");
        assert_eq!(kb.search("Lead Engineer", &SearchOpts::default()).unwrap()[0].heading_path, SUMMARY_HEADING);
        assert_eq!(kb.refresh(d.path(), "t").unwrap().unchanged, 1, "and only once");
        kb.integrity_check().unwrap();
    }

    #[test]
    fn an_index_made_before_files_were_stamped_gains_the_columns_and_is_read_once() {
        let ok = "---\nsource_type: user_ratified\n---\n# T\n\nalpha words\n";
        let d = tree(&[("team/context/a.md", ok), ("local/.keep", "")]);
        let db = d.path().join("local/state.db");
        {
            let mut kb = Kb::open(&db).unwrap();
            kb.refresh(d.path(), "t").unwrap();
            kb.conn.execute_batch("ALTER TABLE kb_documents DROP COLUMN mtime_ns; ALTER TABLE kb_documents DROP COLUMN size;").unwrap();
        }
        let mut kb = Kb::open(&db).unwrap();
        let rep = kb.refresh(d.path(), "t").unwrap();
        assert_eq!((rep.added, rep.updated, rep.unchanged), (0, 0, 1), "read once to learn its stamp, and found unchanged");
        assert_eq!(kb.refresh(d.path(), "t").unwrap().unchanged, 1);
        let stamped: i64 = kb.conn.query_row("SELECT COUNT(*) FROM kb_documents WHERE mtime_ns != 0", [], |r| r.get(0)).unwrap();
        assert_eq!(stamped, 1);
    }

    #[test]
    fn agents_and_skills_are_indexed_trusted_unless_a_file_says_otherwise() {
        let manifest = |role: &str| format!("---\nid: {}\nrole: \"Lead Engineer\"\n---\n# {}\n\nWrites zebra code.\n", role, role);
        let d = tree(&[
            ("base/agents/builder/AGENT.md", &manifest("builder")),
            ("base/skills/tdd/SKILL.md", "---\nname: tdd\n---\n# TDD\n\nWrite the zebra test first.\n"),
            ("team/agents/builder/AGENT.md", &manifest("builder").replace("zebra", "team zebra")),
            ("team/skills/odd/SKILL.md", "---\nname: odd\nsource_type: external_untrusted\n---\n# Odd\n\nA zebra skill from the web.\n"),
        ]);
        let files = sources(d.path());
        let names: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(names, vec!["agents/builder/AGENT.md", "skills/odd/SKILL.md", "skills/tdd/SKILL.md"]);
        let mut kb = Kb::open_in_memory().unwrap();
        kb.sync_lexical(&files, "t").unwrap();
        let paths = |o: &SearchOpts| kb.search("zebra", o).unwrap().into_iter().map(|h| h.rel_path).collect::<Vec<_>>();
        let mut found = paths(&SearchOpts::default());
        found.sort();
        assert_eq!(found, vec!["agents/builder/AGENT.md", "skills/tdd/SKILL.md"], "shipped and team files are trusted by default; a file that says it is untrusted is not");
        assert_eq!(paths(&SearchOpts { include_untrusted: true, ..Default::default() }).len(), 3);
        // What a manifest says about itself in its frontmatter can be looked up, and points at those lines.
        let by_role = kb.search("Lead Engineer", &SearchOpts::default()).unwrap();
        assert_eq!((by_role[0].heading_path.as_str(), by_role[0].pointer.as_str()), (SUMMARY_HEADING, "agents/builder/AGENT.md#L1-L4"));
        assert_eq!(resolve(d.path(), &by_role[0]), Ok((1, 4)), "the summary's pointer stays valid");
        let doc_tier: String = kb.conn.query_row("SELECT source_type FROM kb_documents WHERE rel_path = 'agents/builder/AGENT.md'", [], |r| r.get(0)).unwrap();
        assert_eq!(doc_tier, "internal_generated", "the team copy replaced the shipped one and carries the team's tier");
        let shipped: String = kb.conn.query_row("SELECT source_type FROM kb_documents WHERE rel_path = 'skills/tdd/SKILL.md'", [], |r| r.get(0)).unwrap();
        assert_eq!(shipped, "user_ratified");
    }

    #[test]
    fn the_source_repo_answers_an_identifier_query_with_the_rubric_first() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut kb = Kb::open_in_memory().unwrap();
        let mut files = Vec::new();
        for f in ["REVIEW.md", "GOVERNANCE.md", "SPEC.md"] {
            files.push(SourceFile { rel_path: f.into(), zone: "core", path: repo.join(f), default_source: UNTRUSTED, bundle: None });
        }
        md_files(&repo.join("specs"), "specs", "specs", UNTRUSTED, &mut files);
        kb.sync(&files, "t").unwrap();
        kb.integrity_check().unwrap();
        let hits = kb.search("[REV-SEC-04]", &SearchOpts { include_untrusted: true, ..Default::default() }).unwrap();
        assert_eq!(hits[0].rel_path, "REVIEW.md", "{:?}", hits.iter().map(|h| &h.pointer).collect::<Vec<_>>());
    }

    // ── added bundles (specs/024 §3, §4.6; specs/026 AC-6) ──────────────────────

    /// A home with one workspace `w` and the given bundles added: (org, name, hash, review_by, [(knowledge path, text)]).
    type Added<'a> = (&'a str, &'a str, &'a str, &'a str, &'a [(&'a str, &'a str)]);

    fn home_with_bundles(bundles: &[Added]) -> (tempfile::TempDir, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(ws.join("local")).unwrap();
        let mut lock = delphi_core::bundles::Lock::default();
        for (org, name, hash, review_by, files) in bundles {
            for (p, t) in *files {
                let path = delphi_core::bundles::cache_dir(home.path(), org, name, hash).join("knowledge").join(p);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, t).unwrap();
            }
            lock.bundle.push(delphi_core::bundles::Entry {
                org: org.to_string(),
                name: name.to_string(),
                version: "1.4.0".into(),
                hash: hash.to_string(),
                publisher_kid: "unsigned".into(),
                provenance: "unsigned".into(),
                source: delphi_core::bundles::Source { kind: "directory".into(), path: "/x".into() },
                added_at: "2026-10-25T00:00:00Z".into(),
                order: 10,
                status: "active".into(),
                review_by: review_by.to_string(),
                title: "T".into(),
                skill_roles: Default::default(),
                history: Vec::new(),
                inert_servers: Vec::new(),
            });
        }
        delphi_core::bundles::write(&ws, &lock).unwrap();
        (home, ws)
    }

    const RUNBOOK: &str = "---\nid: acme/runbook\ntitle: Runbook\nsource_type: external_untrusted\n---\n# Rolling deploy\n\nShip the gizmo service one cluster at a time using the quokka procedure.\n";

    #[test]
    fn an_added_bundles_knowledge_is_indexed_at_tier_l2_whatever_its_frontmatter_says() {
        let (_h, ws) = home_with_bundles(&[("acme", "plat", "blake3:aa", "2999-01-01", &[("runbooks/deploy.md", RUNBOOK)])]);
        let files = sources(&ws);
        let f = files.iter().find(|f| f.zone == "bundles").expect("the bundle's file is a source");
        assert_eq!(f.rel_path, "bundles/acme/plat/knowledge/runbooks/deploy.md");
        assert_eq!(f.default_source, "federated_verified");
        let mut kb = Kb::open_in_memory().unwrap();
        kb.sync_lexical(&files, "t").unwrap();
        // the frontmatter claims external_untrusted; a bundle's own claim is a display copy, so default search finds it
        let hits = kb.search("quokka procedure", &SearchOpts::default()).unwrap();
        assert_eq!(hits.len(), 1, "{:?}", hits);
        assert_eq!(hits[0].pointer, "@bundle/acme/plat/knowledge/runbooks/deploy.md#L6-L8", "an agent-readable logical path");
        let (tier, ctx): (String, String) = kb.conn.query_row("SELECT d.source_type, c.contextual_text FROM kb_documents d JOIN kb_chunks c ON c.doc_id = d.doc_id", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(tier, "federated_verified");
        assert!(ctx.contains("Bundle: acme/plat@1.4.0 · unsigned]"), "{}", ctx);
    }

    // specs/025 AC-5: a skill's references/ is indexed under zone `skills` at the bundle's tier and found by its words, with
    // a pointer an agent reads through `@bundle/`; SKILL.md, scripts and templates of the same skill are not indexed.
    #[test]
    fn a_skills_references_are_indexed_under_zone_skills_and_nothing_else_of_the_skill_is() {
        let (h, ws) = home_with_bundles(&[("acme", "plat", "blake3:aa", "2999-01-01", &[("runbooks/deploy.md", RUNBOOK)])]);
        let skill = delphi_core::bundles::cache_dir(h.path(), "acme", "plat", "blake3:aa").join("skills/deploy-helper");
        for (p, t) in [
            ("references/rollout.md", "---\ntitle: Rollout\nsource_type: user_ratified\n---\n# Rollout\n\nThe wombat checklist for cutting over a cluster.\n"),
            ("references/deep/more.md", "# More\n\nA second wombat note.\n"),
            ("references/notes.txt", "wombat in a text file"),
            ("SKILL.md", "---\nname: deploy-helper\ndescription: x\n---\n# Skill\n\nwombat in the skill body\n"),
            ("templates/t.md", "# T\n\nwombat template\n"),
            ("scripts/run.md", "# S\n\nwombat script notes\n"),
        ] {
            std::fs::create_dir_all(skill.join(p).parent().unwrap()).unwrap();
            std::fs::write(skill.join(p), t).unwrap();
        }
        // a linked references directory and a hidden file are never followed or indexed
        std::os::unix::fs::symlink("../../../knowledge", skill.parent().unwrap().join("other-ref-target")).unwrap();
        let sneaky = skill.parent().unwrap().join("sneaky");
        std::fs::create_dir_all(&sneaky).unwrap();
        std::os::unix::fs::symlink("../../../knowledge", sneaky.join("references")).unwrap();
        std::fs::write(skill.join("references/.hidden.md"), "# H\n\nwombat hidden\n").unwrap();
        let files = sources(&ws);
        let refs: Vec<&str> = files.iter().filter(|f| f.zone == "skills").map(|f| f.rel_path.as_str()).collect();
        assert_eq!(refs, vec!["bundles/acme/plat/skills/deploy-helper/references/deep/more.md", "bundles/acme/plat/skills/deploy-helper/references/rollout.md"]);
        assert!(files.iter().filter(|f| f.zone == "skills").all(|f| f.default_source == "federated_verified" && f.bundle.is_some()), "the bundle's tier, whatever the file's frontmatter says");
        let mut kb = Kb::open_in_memory().unwrap();
        kb.sync_lexical(&files, "t").unwrap();
        let hits = kb.search("wombat checklist", &SearchOpts::default()).unwrap();
        assert_eq!(hits.first().map(|h| h.pointer.as_str()), Some("@bundle/acme/plat/skills/deploy-helper/references/rollout.md#L5-L7"), "{:?}", hits.iter().map(|h| &h.pointer).collect::<Vec<_>>());
        assert!(hits.iter().all(|h| h.rel_path.contains("/references/")), "SKILL.md, templates and scripts are not indexed");
        let (tier, zone): (String, String) = kb.conn.query_row("SELECT source_type, zone FROM kb_documents WHERE rel_path LIKE '%rollout.md'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((tier.as_str(), zone.as_str()), ("federated_verified", "skills"));
        // the bundle filters cover them; --no-bundles leaves them out; the knowledge is untouched
        let only = SearchOpts { bundle: Some("acme/plat".into()), ..Default::default() };
        assert!(!kb.search("wombat checklist", &only).unwrap().is_empty());
        assert!(kb.search("wombat checklist", &SearchOpts { no_bundles: true, ..Default::default() }).unwrap().is_empty());
        assert!(!kb.search("quokka procedure", &SearchOpts::default()).unwrap().is_empty());
    }

    #[test]
    fn a_bundle_result_says_lexical_only_until_the_bundles_vectors_are_complete_and_then_says_nothing() {
        let docs: &[(&str, &str)] = &[("a.md", RUNBOOK), ("b.md", "# Rollback\n\nUndo the quokka procedure by redeploying the last good build.\n")];
        let (_h, ws) = home_with_bundles(&[("acme", "plat", "blake3:aa", "2999-01-01", docs)]);
        let mut kb = Kb::open_in_memory().unwrap();
        kb.attach_embedder(concepts()).unwrap();
        kb.sync_lexical(&sources(&ws), "t").unwrap();
        let opts = SearchOpts::default();
        let before = kb.search("quokka procedure", &opts).unwrap();
        assert_eq!(before.len(), 2);
        assert!(before.iter().all(|h| h.note.as_deref() == Some("(lexical only: vectors 0% ready)")), "{:?}", before.iter().map(|h| &h.note).collect::<Vec<_>>());
        assert_eq!(kb.bundle_vector_counts("acme/plat").unwrap(), (0, 2));
        // half way: one chunk has its vector (a restart would resume from here)
        kb.conn.execute("INSERT INTO kb_chunk_vec(chunk_id, windows) SELECT MIN(chunk_id), 1 FROM kb_chunks", []).unwrap();
        let half = kb.search("quokka procedure", &opts).unwrap();
        assert!(half.iter().all(|h| h.note.as_deref() == Some("(lexical only: vectors 50% ready)")), "{:?}", half.iter().map(|h| &h.note).collect::<Vec<_>>());
        kb.conn.execute("DELETE FROM kb_chunk_vec", []).unwrap();
        kb.embed_pending(&mut |_, _| {}).unwrap();
        let after = kb.search("quokka procedure", &opts).unwrap();
        assert!(after.iter().all(|h| h.note.is_none()), "the note is gone when the vectors are complete");
        // a document that is not a bundle's never carries the note
        kb.upsert("context/x.md", "context", "# X\n\nthe quokka procedure\n", "t").unwrap();
        let plain = kb.search("quokka procedure", &SearchOpts { include_untrusted: true, ..Default::default() }).unwrap();
        assert!(plain.iter().filter(|h| h.rel_path == "context/x.md").all(|h| h.note.is_none()));
    }

    #[test]
    fn bundle_filters_narrow_the_results_and_a_stale_bundle_ranks_lower_but_is_not_hidden() {
        let fresh: &[(&str, &str)] = &[("a.md", "---\nid: a\n---\n# Deploy\n\nthe quokka procedure for deploys\n")];
        let old: &[(&str, &str)] = &[("b.md", "---\nid: b\n---\n# Deploy\n\nthe quokka procedure for deploys\n")];
        let (_h, ws) = home_with_bundles(&[("acme", "old", "blake3:bb", "2020-01-01", old), ("acme", "new", "blake3:cc", "2999-01-01", fresh), ("other", "x", "blake3:dd", "2999-01-01", fresh)]);
        let mut kb = Kb::open_in_memory().unwrap();
        kb.sync_lexical(&sources(&ws), "t").unwrap();
        let all = kb.search("quokka procedure", &SearchOpts::default()).unwrap();
        assert_eq!(all.len(), 3);
        assert!(all.last().unwrap().rel_path.contains("acme/old/"), "stale ranks last: {:?}", all.iter().map(|h| &h.rel_path).collect::<Vec<_>>());
        let only = |b: &str| SearchOpts { bundle: Some(b.into()), ..Default::default() };
        assert_eq!(kb.search("quokka procedure", &only("acme/new")).unwrap().len(), 1);
        assert_eq!(kb.search("quokka procedure", &only("acme")).unwrap().len(), 2, "an org filter");
        assert!(kb.search("quokka procedure", &only("../x")).unwrap().is_empty(), "a filter that is not a plain id matches nothing");
        assert!(kb.search("quokka procedure", &SearchOpts { no_bundles: true, ..Default::default() }).unwrap().is_empty());
    }

    #[test]
    fn a_removed_bundle_leaves_the_index_and_the_next_sync_says_so() {
        let (home, ws) = home_with_bundles(&[("acme", "plat", "blake3:aa", "2999-01-01", &[("a.md", RUNBOOK)])]);
        let mut kb = Kb::open_in_memory().unwrap();
        assert_eq!(kb.sync_lexical(&sources(&ws), "t").unwrap().added, 1);
        let mut lock = delphi_core::bundles::read(&ws).unwrap();
        lock.bundle[0].status = "removed".into();
        delphi_core::bundles::write(&ws, &lock).unwrap();
        let rep = kb.sync_lexical(&sources(&ws), "t").unwrap();
        assert_eq!(rep.removed, 1);
        assert!(kb.search("quokka", &SearchOpts { include_untrusted: true, ..Default::default() }).unwrap().is_empty());
        assert!(delphi_core::bundles::cache_dir(home.path(), "acme", "plat", "blake3:aa").is_dir(), "the cache stays until gc");
    }

    /// A workspace dir with one mission log holding `events`.
    fn staged_ws(events: Vec<delphi_core::event::EventEnvelope>) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let mut log = delphi_core::log::EventLog::open(&d.path().join("local/missions/m1"), delphi_core::log::LogOptions::default()).unwrap();
        for e in events {
            log.append(e).unwrap();
        }
        d
    }

    fn staged_event(id: &str, tier: &str, summary: &str) -> delphi_core::event::EventEnvelope {
        let mut e = delphi_core::event::EventEnvelope::new("m1", "learn.staged", serde_json::json!({"id": id, "trigger": "denied_card", "summary": summary, "event_seqs": [1], "tier": tier}));
        e.task_id = Some("t_0001".into());
        e
    }

    #[test]
    fn staged_notes_are_a_zone_that_only_an_explicit_search_sees_and_taint_still_applies() {
        let d = staged_ws(vec![staged_event("n3", "L5", "never touch the quokka lockfile"), staged_event("n4", "L0", "quokka text from the web")]);
        crate::learn::project_staged(d.path());
        let mut kb = Kb::open_in_memory().unwrap();
        kb.sync_lexical(&sources(d.path()), "t").unwrap();
        let q = |o: SearchOpts| kb.search("quokka", &SearchOpts { legs: Legs::LexicalOnly, ..o }).unwrap().into_iter().map(|h| h.rel_path).collect::<Vec<_>>();
        assert!(q(SearchOpts::default()).is_empty(), "an ordinary search never returns staged notes");
        assert_eq!(q(SearchOpts { include_staged: true, ..Default::default() }), vec!["staged/m1/n3.md"], "the web-tainted note stays out of a trusted search");
        let all = q(SearchOpts { include_staged: true, include_untrusted: true, ..Default::default() });
        assert_eq!(all.len(), 2, "{:?}", all);
        let doc = std::fs::read_to_string(d.path().join("local/staged/m1/n3.md")).unwrap();
        assert!(doc.contains("denied_card") && doc.contains("tier L5") && doc.contains("not ratified"), "{}", doc);
    }

    #[test]
    fn a_dropped_note_leaves_the_zone_at_the_next_projection() {
        let d = staged_ws(vec![staged_event("n3", "L5", "never touch the quokka lockfile")]);
        crate::learn::project_staged(d.path());
        assert!(d.path().join("local/staged/m1/n3.md").is_file());
        // Appending a drop changes the log after the projection stamp, so the mission is read again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut log = delphi_core::log::EventLog::open(&d.path().join("local/missions/m1"), delphi_core::log::LogOptions::default()).unwrap();
        log.append(delphi_core::event::EventEnvelope::new("m1", "learn.dropped", serde_json::json!({"id": "n3", "reason": "operator"}))).unwrap();
        drop(log);
        crate::learn::project_staged(d.path());
        assert!(!d.path().join("local/staged/m1/n3.md").exists());
        let mut kb = Kb::open_in_memory().unwrap();
        kb.sync_lexical(&sources(d.path()), "t").unwrap();
        assert!(kb.search("quokka", &SearchOpts { include_staged: true, include_untrusted: true, legs: Legs::LexicalOnly, ..Default::default() }).unwrap().is_empty());
    }

    #[test]
    fn a_workspace_with_no_lock_indexes_exactly_what_it_did_before() {
        let d = tree(&[("team/context/index.md", "---\nsource_type: user_ratified\n---\n# T\n\nplain\n")]);
        assert!(sources(d.path()).iter().all(|f| f.zone != "bundles" && f.bundle.is_none()));
    }
}
