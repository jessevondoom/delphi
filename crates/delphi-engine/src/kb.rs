//! The knowledge base: lexical archival search over a workspace's Markdown (`specs/004` §3).
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

use crate::embed::{self, Embedder};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const SCHEMA_VERSION: &str = "1";
/// Reciprocal-rank constant (`specs/004` §3.4).
const RRF_K: f64 = 60.0;
/// A chunk longer than this is split at blank lines outside code fences.
const MAX_CHUNK_CHARS: usize = 3500;
/// Files larger than this are not indexed.
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// A chunk is embedded in windows of about this many characters, so the model reads all of a long section.
const WINDOW_CHARS: usize = 1600;
/// Most windows embedded per chunk; a chunk's vectors live at `chunk_id * VEC_STRIDE + window`.
const VEC_STRIDE: i64 = 16;
/// Version of what is embedded for a chunk (the windowing and the text around each window). Changing either bumps
/// this, and every vector is made again, just as for a new model.
const EMBED_FORMAT: &str = "1";
/// Chunks embedded per model call and per write transaction.
const EMBED_BATCH: usize = 16;
/// How many nearest windows the vector ranking looks at before duplicates and visibility are applied.
const VEC_CANDIDATES: usize = 120;
/// Default weight of the vector ranking in the fusion (`specs/004` §3.4); each lexical ranking weighs 1.0, definitions 2.0.
const VEC_WEIGHT: f64 = 3.0;
/// Default factor on the lexical rankings when only the any-word pass found something.
const LOOSE_WEIGHT: f64 = 0.15;

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
    updated_at      TEXT NOT NULL
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
}

const UNTRUSTED: &str = "external_untrusted";

fn md_files(root: &Path, logical_prefix: &str, zone: &'static str, default_source: &'static str, out: &mut Vec<SourceFile>) {
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
            out.push(SourceFile { rel_path: rel, zone, path: e.path(), default_source });
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
                    core.push(SourceFile { rel_path: n, zone: "core", path: e.path(), default_source: UNTRUSTED });
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
        conn.execute("INSERT OR IGNORE INTO kb_meta(key, value) VALUES ('schema_version', ?1)", [SCHEMA_VERSION])?;
        Ok(Kb { conn, embedder: None, floor: None, seed: None, seeded: 0 })
    }

    /// Index one document, replacing any earlier version. Returns whether it changed anything.
    pub fn upsert(&mut self, rel_path: &str, zone: &str, text: &str, now: &str) -> Result<bool, KbError> {
        self.upsert_from(rel_path, zone, UNTRUSTED, text, now)
    }

    /// `upsert` for a file whose frontmatter may not name its `source_type`: `default_source` stands in.
    pub fn upsert_from(&mut self, rel_path: &str, zone: &str, default_source: &str, text: &str, now: &str) -> Result<bool, KbError> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let existing: Option<(i64, String)> = self.conn.query_row("SELECT doc_id, content_blake3 FROM kb_documents WHERE rel_path = ?1", [rel_path], |r| Ok((r.get(0)?, r.get(1)?))).ok();
        if existing.as_ref().is_some_and(|(_, h)| *h == hash) {
            return Ok(false);
        }
        let source_type = frontmatter_field(text, "source_type").filter(|s| SOURCE_TYPES.contains(&s.as_str())).unwrap_or_else(|| default_source.to_string());
        let (author, confidence, status) = (frontmatter_field(text, "author"), frontmatter_field(text, "confidence").unwrap_or_else(|| "low".into()), frontmatter_field(text, "status").unwrap_or_else(|| "active".into()));
        let tx = self.conn.transaction()?;
        if let Some((id, _)) = &existing {
            drop_vectors(&tx, Some(*id))?;
            tx.execute("DELETE FROM kb_documents WHERE doc_id = ?1", [id])?; // cascades to the chunks, whose trigger clears both indexes
        }
        tx.execute("INSERT INTO kb_documents(rel_path, zone, content_blake3, author, source_type, confidence, status, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![rel_path, zone, hash, author, source_type, confidence, status, now])?;
        let doc_id = tx.last_insert_rowid();
        for (i, c) in chunk_markdown(text).into_iter().enumerate() {
            let ids = identifiers(&c.raw);
            let ctx = format!("[File: {} | Lines: {}-{} | Section: {} | Source: {} | Status: {}]\n{}", rel_path, c.start_line, c.end_line, if c.heading_path.is_empty() { "(top)" } else { &c.heading_path }, source_type, status, c.raw);
            tx.execute(
                "INSERT INTO kb_chunks(doc_id, chunk_index, start_line, end_line, rel_path, heading_path, identifiers, raw_content, contextual_text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![doc_id, i as i64, c.start_line as i64, c.end_line as i64, rel_path, c.heading_path, ids, c.raw, ctx],
            )?;
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

    /// New and changed documents indexed, vanished ones dropped. Fast: no model is run.
    pub fn sync_lexical(&mut self, files: &[SourceFile], now: &str) -> Result<IndexReport, KbError> {
        let mut rep = IndexReport::default();
        let known: HashMap<String, ()> = {
            let mut st = self.conn.prepare("SELECT rel_path FROM kb_documents")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            rows.flatten().map(|p| (p, ())).collect()
        };
        let mut seen: HashMap<&str, ()> = HashMap::new();
        for f in files {
            let Ok(bytes) = std::fs::read(&f.path) else { continue };
            let text = String::from_utf8_lossy(&bytes);
            seen.insert(&f.rel_path, ());
            let existed = known.contains_key(&f.rel_path);
            if self.upsert_from(&f.rel_path, f.zone, f.default_source, &text, now)? {
                if existed {
                    rep.updated += 1;
                } else {
                    rep.added += 1;
                }
            } else {
                rep.unchanged += 1;
            }
        }
        for p in known.keys().filter(|p| !seen.contains_key(p.as_str())) {
            self.remove(p)?;
            rep.removed += 1;
        }
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
        self.conn.execute("DELETE FROM kb_documents", [])?;
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

    /// Use these preloaded vectors (tests; `attach_embedder` loads the shipped ones itself).
    pub fn set_seed_vectors(&mut self, seed: Option<Arc<embed::vectors::VecCache>>) {
        self.seed = seed;
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
    /// Factor applied to the lexical rankings when no chunk had every query word and the any-word pass was used.
    pub loose_weight: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Legs {
    Hybrid,
    LexicalOnly,
    VectorOnly,
}

impl Default for SearchOpts {
    fn default() -> Self {
        SearchOpts { role: None, limit: 10, include_untrusted: false, legs: Legs::Hybrid, vec_weight: VEC_WEIGHT, loose_weight: LOOSE_WEIGHT }
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
}

/// Words that carry no topic in a question. They are left out of a query unless nothing else is left, so that
/// "how are untrusted results kept out" is matched on `untrusted results kept`, not on `how are out`.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "can", "could", "do", "does", "did", "for", "from", "had", "has", "have", "how", "if", "in", "into", "is", "it", "its", "may", "of", "on", "or", "our", "should", "so", "than",
    "that", "the", "their", "them", "then", "there", "these", "they", "this", "to", "was", "we", "were", "what", "when", "where", "which", "who", "whom", "why", "will", "with", "would", "you", "your", "about", "not", "any",
];

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
        let terms = match_terms(query);
        if terms.is_empty() {
            return Ok(vec![]);
        }
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
        let mut score: HashMap<i64, f64> = HashMap::new();
        let nearest = if opts.legs != Legs::LexicalOnly { self.vector_ranked(query, opts, &allowed)? } else { vec![] };
        if opts.legs != Legs::VectorOnly {
            // Precise first (every term), then any term, so a long question still finds something.
            let mut ident = Vec::new();
            let mut prose = Vec::new();
            let (mut factor, mut loose) = (1.0, false);
            for joiner in [" AND ", " OR "] {
                let expr = terms.join(joiner);
                ident = self.ranked("kb_fts_ident", ", 2.0, 5.0, 10.0", &expr, &allowed)?;
                prose = self.ranked("kb_fts_prose", ", 1.0", &expr, &allowed)?;
                if !ident.is_empty() || !prose.is_empty() {
                    break;
                }
                // Nothing held every word: the any-word pass that follows is a weaker signal than a full match.
                (factor, loose) = if terms.len() > 1 { (opts.loose_weight, true) } else { (1.0, false) };
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
        order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
        let mut hits = Vec::new();
        for (id, s) in order.into_iter().take(opts.limit.min(10)) {
            let row = self.conn.query_row("SELECT c.rel_path, c.start_line, c.end_line, c.heading_path, c.raw_content, d.content_blake3 FROM kb_chunks c JOIN kb_documents d ON d.doc_id = c.doc_id WHERE c.chunk_id = ?1", [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?)));
            let Ok((rel_path, a, b, heading_path, raw, blake3)) = row else { continue };
            let preview = raw.lines().filter(|l| !l.trim().is_empty()).take(3).map(|l| l.chars().take(160).collect::<String>()).collect::<Vec<_>>().join("\n");
            hits.push(Hit { pointer: format!("{}#L{}-L{}", rel_path, a, b), rel_path, start_line: a as usize, end_line: b as usize, heading_path, blake3, preview, score: s });
        }
        Ok(hits)
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
    chunk_markdown(&text).into_iter().find(|c| c.heading_path == hit.heading_path).map(|c| (c.start_line, c.end_line)).ok_or_else(|| format!("stale: the section `{}` of {} no longer exists", hit.heading_path, hit.rel_path))
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
}

pub fn health(home: &Path, ws: &Path) -> Result<Health, KbError> {
    let kb = Kb::open(&db_path(ws))?;
    kb.integrity_check()?;
    let (documents, chunks) = kb.counts()?;
    let indexed_model: Option<String> = kb.conn.query_row("SELECT value FROM kb_meta WHERE key = 'embedding_model_id'", [], |r| r.get(0)).ok();
    let vectors = if has_vec(&kb.conn) { kb.vector_counts()?.0 } else { 0 };
    Ok(Health { documents, chunks, vectors, indexed_model, installed_model: embed::installed_model_id(Some(home)) })
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

fn db_path(ws: &Path) -> PathBuf {
    ws.join("local/state.db")
}

fn now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}", secs)
}

/// Open the installed embedding model for `kb`, if there is one. A model that is installed but cannot be used is
/// reported once and search carries on without it.
pub fn attach_default(home: &Path, kb: &mut Kb) -> Result<bool, KbError> {
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
        files.extend(top.into_iter().map(|e| SourceFile { rel_path: e.file_name().to_string_lossy().to_string(), zone: "core", path: e.path(), default_source: UNTRUSTED }));
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
}

/// Recall of each ranking on `cases`. A result is right when it is in the expected file (if one is named) under a
/// heading containing the expected text.
pub fn evaluate(kb: &Kb, cases: &[EvalCase], base: &SearchOpts) -> Result<EvalReport, KbError> {
    let mut rep = EvalReport::default();
    let positives: Vec<&EvalCase> = cases.iter().filter(|c| !c.none).collect();
    let negatives: Vec<&EvalCase> = cases.iter().filter(|c| c.none).collect();
    rep.cases = positives.len();
    let modes: Vec<(&'static str, Legs)> = if kb.embedder.is_some() { vec![("lexical", Legs::LexicalOnly), ("vector", Legs::VectorOnly), ("hybrid", Legs::Hybrid)] } else { vec![("lexical", Legs::LexicalOnly)] };
    for (name, legs) in modes {
        let opts = SearchOpts { legs, include_untrusted: true, ..base.clone() };
        let mut r = Recall::default();
        for c in &positives {
            let hits = kb.search(&c.q, &opts)?;
            let rank = hits.iter().position(|h| (c.path.is_empty() || h.rel_path == c.path) && h.heading_path.to_lowercase().contains(&c.heading.to_lowercase()));
            if let Some(i) = rank {
                r.mrr += 1.0 / (i as f64 + 1.0);
                r.at1 += (i < 1) as u8 as f64;
                r.at3 += (i < 3) as u8 as f64;
                r.at5 += (i < 5) as u8 as f64;
                r.at10 += 1.0;
            } else if name == "hybrid" || legs == Legs::LexicalOnly && kb.embedder.is_none() {
                rep.misses.push(c.q.clone());
            }
        }
        let n = positives.len().max(1) as f64;
        rep.per_mode.push((name, Recall { at1: r.at1 / n, at3: r.at3 / n, at5: r.at5 / n, at10: r.at10 / n, mrr: r.mrr / n }));
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
    Ok(rep)
}

fn print_eval(rep: &EvalReport) {
    println!("recall over {} question(s), top 10:", rep.cases);
    println!("  {:<8} {:>6} {:>6} {:>6} {:>6} {:>6}", "ranking", "@1", "@3", "@5", "@10", "MRR");
    for (name, r) in &rep.per_mode {
        println!("  {:<8} {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>6.3}", name, r.at1, r.at3, r.at5, r.at10, r.mrr);
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
            match embed::locate(Some(home)) {
                embed::ModelSource::Disabled => println!("embedding model: switched off (DELPHI_EMBED_MODEL=off)"),
                embed::ModelSource::Absent => println!("embedding model: none installed; search is lexical only (install one with `delphi kb model pack` or a release build)"),
                src => {
                    #[cfg(feature = "local-embeddings")]
                    {
                        let bytes = match &src {
                            embed::ModelSource::File(p) => std::fs::read(p).map_err(|e| KbError::Io(format!("{}: {}", p.display(), e)))?,
                            _ => Vec::new(),
                        };
                        let origin = match &src {
                            embed::ModelSource::File(p) => p.display().to_string(),
                            _ => "compiled into this binary".into(),
                        };
                        match embed::open_default(Some(home)) {
                            Ok(Some(e)) => println!("embedding model: {} ({} dimensions, {}){}", e.model_id(), e.dim(), origin, embed::local::peek(&bytes).map(|m| format!(", from {}", m.source)).unwrap_or_default()),
                            Ok(None) => println!("embedding model: none"),
                            Err(e) => println!("embedding model: installed at {} but not usable: {}", origin, e),
                        }
                    }
                    #[cfg(not(feature = "local-embeddings"))]
                    println!("embedding model: {:?} (this build has no embedding runtime)", src);
                }
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
                println!("recorded. (`delphi needs` weighs confirmed misses when it considers a better embedding model.)");
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
                Ok(0)
            }
            "search" => {
                let query: Vec<&str> = args[1..].iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
                let role = args.iter().position(|a| a == "--role").and_then(|i| args.get(i + 1)).cloned();
                let query: Vec<&str> = query.into_iter().filter(|a| Some(*a) != role.as_deref()).collect();
                if query.is_empty() {
                    eprintln!("usage: delphi kb search <query> [--role <role>] [--untrusted] [--lexical]");
                    return Ok(2);
                }
                let mut kb = open()?;
                let model = attach_default(home, &mut kb)?;
                // An index that has never been built is built now (words only, which is quick); `kb index` adds vectors.
                if kb.counts()?.0 == 0 {
                    kb.sync_lexical(&sources(ws), &now())?;
                }
                if model {
                    kb.apply_seed_vectors()?;
                    let (v, c) = kb.vector_counts()?;
                    if v < c {
                        eprintln!("note: {} of {} chunk(s) have no vector yet; run `delphi kb index` for the best results", c - v, c);
                    }
                }
                let legs = if args.iter().any(|a| a == "--lexical") { Legs::LexicalOnly } else { Legs::Hybrid };
                let opts = SearchOpts { role, include_untrusted: args.iter().any(|a| a == "--untrusted"), legs, ..Default::default() };
                let hits = kb.search(&query.join(" "), &opts)?;
                log_search(ws, &query.join(" "), opts.role.as_deref(), hits.len());
                if hits.is_empty() {
                    println!("no results");
                }
                for (i, h) in hits.iter().enumerate() {
                    println!("{}. {}  [{}]", i + 1, h.pointer, if h.heading_path.is_empty() { "(top)" } else { &h.heading_path });
                    for l in h.preview.lines() {
                        println!("     {}", l);
                    }
                }
                Ok(0)
            }
            "eval" => {
                // delphi kb eval [--docs <dir>] [--file <cases.json>] [--db <cache file>]
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
                eprintln!("usage: delphi kb index | reindex | stats | search <query> [--role <role>] [--untrusted] [--lexical] | miss | eval | model | seed-export <out>");
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
            files.push(SourceFile { rel_path: f.into(), zone: "core", path: repo.join(f), default_source: UNTRUSTED });
        }
        md_files(&repo.join("specs"), "specs", "specs", UNTRUSTED, &mut files);
        kb.sync(&files, "t").unwrap();
        kb.integrity_check().unwrap();
        let hits = kb.search("[REV-SEC-04]", &SearchOpts { include_untrusted: true, ..Default::default() }).unwrap();
        assert_eq!(hits[0].rel_path, "REVIEW.md", "{:?}", hits.iter().map(|h| &h.pointer).collect::<Vec<_>>());
    }
}
