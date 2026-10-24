//! The knowledge base: lexical archival search over a workspace's Markdown (`specs/004` §3).
//!
//! `local/state.db` holds a *derived* index: delete it and `delphi kb reindex` rebuilds it from the files.
//! Documents are chunked along `#`/`##`/`###` headings (fenced code stays whole), each chunk records its
//! exact 1-indexed line range and a breadcrumb, and two FTS5 tables over the chunks are fused by reciprocal
//! rank: an identifier index that keeps tokens such as `REV-SEC-04` and `Part.thoughtSignature` whole, and a
//! prose index. Vectors (`sqlite-vec`) are deferred by D-01.
//!
//! Visibility is decided by the query, not the caller's good manners: an agent sees `memory/agents/<role>/`
//! only for its own role, and nothing from untrusted or inactive documents unless asked.

use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: &str = "1";
/// Reciprocal-rank constant (`specs/004` §3.4).
const RRF_K: f64 = 60.0;
/// A chunk longer than this is split at blank lines outside code fences.
const MAX_CHUNK_CHARS: usize = 3500;
/// Files larger than this are not indexed.
const MAX_FILE_BYTES: u64 = 512 * 1024;

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
}

fn md_files(root: &Path, logical_prefix: &str, zone: &'static str, out: &mut Vec<SourceFile>) {
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
            md_files(&e.path(), &rel, zone, out);
        } else if ft.is_file() && name.ends_with(".md") && e.metadata().map(|m| m.len() <= MAX_FILE_BYTES).unwrap_or(false) {
            out.push(SourceFile { rel_path: rel, zone, path: e.path() });
        }
    }
}

/// The Markdown a workspace indexes, by logical path. The team copy of a path wins over the built-in one.
/// Operator-scope files are never listed; neither are live worktrees or source code.
pub fn sources(ws: &Path) -> Vec<SourceFile> {
    let mut all: Vec<SourceFile> = Vec::new();
    for base in [ws.join("base"), ws.join("team")] {
        let mut core = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&base) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if e.file_type().map(|t| t.is_file()).unwrap_or(false) && n.ends_with(".md") {
                    core.push(SourceFile { rel_path: n, zone: "core", path: e.path() });
                }
            }
        }
        all.extend(core);
        md_files(&base.join("context"), "context", "context", &mut all);
        md_files(&base.join("specs"), "specs", "specs", &mut all);
    }
    md_files(&ws.join("local/missions"), "missions", "missions", &mut all);
    md_files(&ws.join("local/memory/agents"), "memory/agents", "memory", &mut all);
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
}

fn frontmatter_field(text: &str, key: &str) -> Option<String> {
    let (fm, _) = crate::workspace::split_frontmatter(text);
    fm?.get(key)?.as_str().map(String::from)
}

const SOURCE_TYPES: [&str; 6] = ["user_ratified", "internal_empirical", "internal_generated", "federated_verified", "federated_peer", "external_untrusted"];

impl Kb {
    pub fn open(db: &Path) -> Result<Kb, KbError> {
        let conn = Connection::open(db)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Kb::init(conn)
    }

    pub fn open_in_memory() -> Result<Kb, KbError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Kb::init(conn)
    }

    fn init(conn: Connection) -> Result<Kb, KbError> {
        conn.execute_batch(SCHEMA)?;
        conn.execute("INSERT OR IGNORE INTO kb_meta(key, value) VALUES ('schema_version', ?1)", [SCHEMA_VERSION])?;
        Ok(Kb { conn })
    }

    /// Index one document, replacing any earlier version. Returns whether it changed anything.
    pub fn upsert(&mut self, rel_path: &str, zone: &str, text: &str, now: &str) -> Result<bool, KbError> {
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        let existing: Option<(i64, String)> = self.conn.query_row("SELECT doc_id, content_blake3 FROM kb_documents WHERE rel_path = ?1", [rel_path], |r| Ok((r.get(0)?, r.get(1)?))).ok();
        if existing.as_ref().is_some_and(|(_, h)| *h == hash) {
            return Ok(false);
        }
        let source_type = frontmatter_field(text, "source_type").filter(|s| SOURCE_TYPES.contains(&s.as_str())).unwrap_or_else(|| "external_untrusted".into());
        let (author, confidence, status) = (frontmatter_field(text, "author"), frontmatter_field(text, "confidence").unwrap_or_else(|| "low".into()), frontmatter_field(text, "status").unwrap_or_else(|| "active".into()));
        let tx = self.conn.transaction()?;
        if let Some((id, _)) = &existing {
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
        Ok(self.conn.execute("DELETE FROM kb_documents WHERE rel_path = ?1", [rel_path])? > 0)
    }

    /// Bring the index in line with the files: new and changed documents indexed, vanished ones dropped.
    pub fn sync(&mut self, files: &[SourceFile], now: &str) -> Result<IndexReport, KbError> {
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
            if self.upsert(&f.rel_path, f.zone, &text, now)? {
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
        self.conn.execute("DELETE FROM kb_documents", [])?;
        self.conn.execute("INSERT INTO kb_fts_ident(kb_fts_ident) VALUES('rebuild')", [])?;
        self.conn.execute("INSERT INTO kb_fts_prose(kb_fts_prose) VALUES('rebuild')", [])?;
        self.sync(files, now)
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

// ── search ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SearchOpts {
    /// The calling agent's role: `memory/agents/<role>/` is visible only to it. `None` is the operator.
    pub role: Option<String>,
    pub limit: usize,
    /// Include documents whose provenance is untrusted or federated-peer, and inactive ones.
    pub include_untrusted: bool,
}

impl Default for SearchOpts {
    fn default() -> Self {
        SearchOpts { role: None, limit: 10, include_untrusted: false }
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

/// Query words as FTS5 phrases: quoted so punctuation cannot be read as query syntax.
fn match_terms(q: &str) -> Vec<String> {
    let mut terms: Vec<String> = q
        .split(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | '(' | ')' | ',' | ';' | '`' | '*' | '"' | '\'' | '<' | '>' | '|' | '?' | '!'))
        .map(|t| t.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '#' || c == '@')).to_string())
        .filter(|t| !t.is_empty())
        .collect();
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
            if !r.is_empty() && r.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                allowed.push_str(&format!(" AND (d.zone != 'memory' OR d.rel_path LIKE 'memory/agents/{}/%')", r));
            } else {
                allowed.push_str(" AND d.zone != 'memory'");
            }
        }
        // Precise first (every term), then any term, so a long question still finds something.
        let mut ident = Vec::new();
        let mut prose = Vec::new();
        for joiner in [" AND ", " OR "] {
            let expr = terms.join(joiner);
            ident = self.ranked("kb_fts_ident", ", 2.0, 5.0, 10.0", &expr, &allowed)?;
            prose = self.ranked("kb_fts_prose", ", 1.0", &expr, &allowed)?;
            if !ident.is_empty() || !prose.is_empty() {
                break;
            }
        }
        // Chunks that *define* a queried identifier (a line opening with it) get a third, double-weight ranking.
        let defs: Vec<String> = terms.iter().map(|t| format!("\"def:{}\"", t.trim_matches('"'))).collect();
        let defined = self.ranked("kb_fts_ident", ", 0.0, 0.0, 1.0", &defs.join(" OR "), &allowed)?;
        let mut score: HashMap<i64, f64> = HashMap::new();
        for (r, id) in defined.iter().enumerate() {
            *score.entry(*id).or_default() += 2.0 / (RRF_K + r as f64 + 1.0);
        }
        for (r, id) in ident.iter().enumerate() {
            *score.entry(*id).or_default() += 1.0 / (RRF_K + r as f64 + 1.0);
        }
        for (r, id) in prose.iter().enumerate() {
            *score.entry(*id).or_default() += 1.0 / (RRF_K + r as f64 + 1.0);
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

// ── command line ────────────────────────────────────────────────────────────

fn db_path(ws: &Path) -> PathBuf {
    ws.join("local/state.db")
}

fn now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}", secs)
}

/// `delphi kb index|reindex|search|stats` for the workspace at `ws`.
pub fn run(ws: &Path, args: &[String]) -> i32 {
    let sub = args.first().map(String::as_str).unwrap_or("");
    let open = || Kb::open(&db_path(ws));
    let go = || -> Result<i32, KbError> {
        match sub {
            "index" | "reindex" => {
                let mut kb = open()?;
                let files = sources(ws);
                let rep = if sub == "reindex" { kb.rebuild(&files, &now())? } else { kb.sync(&files, &now())? };
                kb.integrity_check()?;
                let (d, c) = kb.counts()?;
                println!("kb: {} document(s), {} chunk(s): {} added, {} updated, {} removed, {} unchanged", d, c, rep.added, rep.updated, rep.removed, rep.unchanged);
                Ok(0)
            }
            "stats" => {
                let kb = open()?;
                let (d, c) = kb.counts()?;
                println!("kb: {} document(s), {} chunk(s) in {}", d, c, db_path(ws).display());
                Ok(0)
            }
            "search" => {
                let query: Vec<&str> = args[1..].iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
                let role = args.iter().position(|a| a == "--role").and_then(|i| args.get(i + 1)).cloned();
                let query: Vec<&str> = query.into_iter().filter(|a| Some(*a) != role.as_deref()).collect();
                if query.is_empty() {
                    eprintln!("usage: delphi kb search <query> [--role <role>] [--untrusted]");
                    return Ok(2);
                }
                let mut kb = open()?;
                // An index that has never been built is built now; later changes are picked up by `kb index`.
                if kb.counts()?.0 == 0 {
                    kb.sync(&sources(ws), &now())?;
                }
                let opts = SearchOpts { role, limit: 10, include_untrusted: args.iter().any(|a| a == "--untrusted") };
                let hits = kb.search(&query.join(" "), &opts)?;
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
            _ => {
                eprintln!("usage: delphi kb index | reindex | stats | search <query> [--role <role>] [--untrusted]");
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

    #[test]
    fn the_source_repo_answers_an_identifier_query_with_the_rubric_first() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut kb = Kb::open_in_memory().unwrap();
        let mut files = Vec::new();
        for f in ["REVIEW.md", "GOVERNANCE.md", "SPEC.md"] {
            files.push(SourceFile { rel_path: f.into(), zone: "core", path: repo.join(f) });
        }
        md_files(&repo.join("specs"), "specs", "specs", &mut files);
        kb.sync(&files, "t").unwrap();
        kb.integrity_check().unwrap();
        let hits = kb.search("[REV-SEC-04]", &SearchOpts { include_untrusted: true, ..Default::default() }).unwrap();
        assert_eq!(hits[0].rel_path, "REVIEW.md", "{:?}", hits.iter().map(|h| &h.pointer).collect::<Vec<_>>());
    }
}
