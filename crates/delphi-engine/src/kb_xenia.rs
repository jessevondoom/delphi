//! The Xenia zone of the knowledge base (`specs/011` §5, §7.2; `specs/012` §8; milestone X6).
//!
//! A child module of `kb`: it needs the chunker, the vector table and the private connection, and nothing outside
//! `kb` may reach them. The zone is *derived*: [`Kb::apply_xenia`] is handed what `delphi_core::xenia_index::derive`
//! computed from an accepted head, makes the zone equal to it in **one transaction**, and a refused or failed call
//! changes nothing. The zone is not made of files, so the file sync never sees it ([`Kb::sync_lexical`] skips it).
//!
//! Three rules the rows follow:
//!
//! * `source_type` and `status` are columns the caller computed from a tier. Nothing here reads them out of a
//!   record's text, and the cache entry's frontmatter is a display copy.
//! * A **tier change is an update, not a re-embedding.** What is embedded is the chunk text (title, summary, tags)
//!   under its path, and vectors are filed per chunk. When only the tier or status moved, the chunks stay and
//!   only `kb_documents.source_type/status`, the chunk breadcrumbs and each vector's `trusted` bit change
//!   (the embedding itself is read and put back unchanged). When the text changed too, vectors of chunks whose
//!   text did not change are carried over by the same key bundles use.
//! * The body of a record is **not a chunk**. It sits in `xenia_records.body` for the quarantine read only, so no
//!   search preview can carry it.

use super::*;
use delphi_core::xenia_index::IndexedMemory;

const SOURCE_TYPES_XENIA: [&str; 3] = ["federated_verified", "federated_peer", "external_untrusted"];

const TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS xenia_records (
    doc_id        INTEGER PRIMARY KEY REFERENCES kb_documents(doc_id) ON DELETE CASCADE,
    rel_path      TEXT NOT NULL UNIQUE,
    org           TEXT NOT NULL,
    store         TEXT NOT NULL,
    id            TEXT NOT NULL,
    rev           INTEGER NOT NULL,
    record_hash   TEXT NOT NULL,
    signer        TEXT NOT NULL,
    author_person TEXT NOT NULL,
    tier          TEXT NOT NULL CHECK (tier IN ('L0', 'L1', 'L2')),
    hidden        TEXT,
    superseded_by TEXT,
    title         TEXT NOT NULL,
    summary       TEXT NOT NULL,
    tags          TEXT NOT NULL,
    body          TEXT NOT NULL,
    pointers      TEXT NOT NULL,
    endorsed_by   TEXT NOT NULL,
    withheld      TEXT NOT NULL,
    disputed      INTEGER NOT NULL,
    clamped       INTEGER NOT NULL,
    fresh_until   INTEGER,
    text_sig      TEXT NOT NULL,
    head          TEXT NOT NULL
);
"#;

pub(super) fn ensure(conn: &Connection) -> Result<(), KbError> {
    conn.execute_batch(TABLE)?;
    Ok(())
}

/// What one [`Kb::apply_xenia`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct XeniaApply {
    pub added: usize,
    /// The text of the entry changed (a new revision): its chunks were made again, unchanged chunks kept their vectors.
    pub rewritten: usize,
    /// Only the tier, status or endorsement state moved: updated in place, no chunk touched, no embedding run.
    pub retiered: usize,
    pub removed: usize,
    pub unchanged: usize,
}

/// A record as the zone holds it, for the agent view and the quarantine read. Every text field was sanitized on the
/// way in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XeniaRow {
    pub rel_path: String,
    pub org: String,
    pub store: String,
    pub id: String,
    pub rev: u64,
    pub record_hash: String,
    /// `L0`, `L1` or `L2`.
    pub tier: String,
    pub source_type: String,
    pub status: String,
    pub hidden: Option<String>,
    pub superseded_by: Option<String>,
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub body: String,
    /// `(scheme, locator, label)`: display only.
    pub pointers: Vec<(String, String, String)>,
    pub endorsed_by: Vec<String>,
    pub withheld_by_staleness: Vec<String>,
    pub disputed: bool,
    pub clamped_by_ceiling: bool,
    pub fresh_until: Option<i64>,
    pub head: String,
}

fn text_sig(m: &IndexedMemory) -> String {
    blake3::hash(format!("{}\0{}\0{}", m.title, m.summary, m.tags.join("\0")).as_bytes()).to_hex().to_string()
}

fn json_list(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn context_line(rel_path: &str, source_type: &str, status: &str, start: i64, end: i64, heading: &str, raw: &str) -> String {
    format!("[File: {} | Lines: {}-{} | Section: {} | Source: {} | Status: {}]\n{}", rel_path, start, end, if heading.is_empty() { "(top)" } else { heading }, source_type, status, raw)
}

fn put_row(tx: &Connection, doc_id: i64, m: &IndexedMemory, head: &str) -> Result<(), KbError> {
    let pointers: Vec<(&str, &str, &str)> = m.pointers.iter().map(|p| (p.scheme, p.locator.as_str(), p.label.as_str())).collect();
    tx.execute(
        "INSERT OR REPLACE INTO xenia_records(doc_id, rel_path, org, store, id, rev, record_hash, signer, author_person, tier, hidden, superseded_by, title, summary, tags, body, pointers, endorsed_by, withheld, disputed, clamped, fresh_until, text_sig, head)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
        params![
            doc_id,
            m.rel_path,
            m.org,
            m.store,
            m.id,
            m.rev as i64,
            m.hash,
            m.signer,
            m.author_person,
            m.tier.as_str(),
            m.hidden,
            m.superseded_by,
            m.title,
            m.summary,
            json_list(&m.tags),
            m.body,
            serde_json::to_string(&pointers).unwrap_or_else(|_| "[]".into()),
            json_list(&m.endorsed_by),
            json_list(&m.withheld_by_staleness),
            m.disputed as i64,
            m.clamped_by_ceiling as i64,
            m.fresh_until,
            text_sig(m),
            head
        ],
    )?;
    Ok(())
}

/// Same chunks, new tier: the documents row, each chunk's breadcrumb and (only if the `trusted` bit moved) each
/// vector, read back and put in again unchanged.
fn retier(tx: &Connection, doc_id: i64, m: &IndexedMemory, now: &str) -> Result<(), KbError> {
    let (old_src, old_status): (String, String) = tx.query_row("SELECT source_type, status FROM kb_documents WHERE doc_id = ?1", [doc_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let hash = m.hash.trim_start_matches("blake3:");
    tx.execute(
        "UPDATE kb_documents SET source_type = ?2, status = ?3, content_blake3 = ?4, author = ?5, confidence = ?6, updated_at = ?7 WHERE doc_id = ?1",
        params![doc_id, m.source_type, m.status, hash, m.author_role, m.confidence, now],
    )?;
    let chunks: Vec<(i64, i64, i64, String, String)> = {
        let mut st = tx.prepare("SELECT chunk_id, start_line, end_line, heading_path, raw_content FROM kb_chunks WHERE doc_id = ?1")?;
        let rows = st.query_map([doc_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        rows.flatten().collect()
    };
    for (id, a, b, heading, raw) in &chunks {
        tx.execute("UPDATE kb_chunks SET contextual_text = ?2 WHERE chunk_id = ?1", params![id, context_line(&m.rel_path, m.source_type, m.status, *a, *b, heading, raw)])?;
    }
    if trusted(&old_src, &old_status) != trusted(m.source_type, m.status) && has_vec(tx) {
        let now_trusted = trusted(m.source_type, m.status) as i64;
        for (chunk, ..) in &chunks {
            let windows: Option<i64> = tx.query_row("SELECT windows FROM kb_chunk_vec WHERE chunk_id = ?1", [chunk], |r| r.get(0)).ok();
            for w in 0..windows.unwrap_or(0) {
                let rowid = chunk * VEC_STRIDE + w;
                let blob: Vec<u8> = tx.query_row("SELECT embedding FROM kb_vec WHERE rowid = ?1", [rowid], |r| r.get(0))?;
                tx.execute("DELETE FROM kb_vec WHERE rowid = ?1", [rowid])?;
                tx.execute("INSERT INTO kb_vec(rowid, trusted, scope, embedding) VALUES (?1, ?2, ?3, ?4)", params![rowid, now_trusted, vec_scope("xenia", &m.rel_path), blob])?;
            }
        }
    }
    Ok(())
}

/// New or changed text: the document is made again, and a chunk whose text did not change keeps its vectors.
fn replace(tx: &Connection, existing: Option<i64>, m: &IndexedMemory, now: &str) -> Result<(), KbError> {
    let carried = match existing {
        Some(id) => carry_vectors(tx, id)?,
        None => HashMap::new(),
    };
    if let Some(id) = existing {
        drop_vectors(tx, Some(id))?;
        tx.execute("DELETE FROM kb_documents WHERE doc_id = ?1", [id])?;
    }
    let text = m.cache_entry();
    let hash = m.hash.trim_start_matches("blake3:");
    tx.execute(
        "INSERT INTO kb_documents(rel_path, zone, content_blake3, author, source_type, confidence, status, updated_at) VALUES (?1, 'xenia', ?2, ?3, ?4, ?5, ?6, ?7)",
        params![m.rel_path, hash, m.author_role, m.source_type, m.confidence, m.status, now],
    )?;
    let doc_id = tx.last_insert_rowid();
    for (i, c) in chunks_for("xenia", &text).into_iter().enumerate() {
        let ids = identifiers(&c.raw);
        tx.execute(
            "INSERT INTO kb_chunks(doc_id, chunk_index, start_line, end_line, rel_path, heading_path, identifiers, raw_content, contextual_text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![doc_id, i as i64, c.start_line as i64, c.end_line as i64, m.rel_path, c.heading_path, ids, c.raw, context_line(&m.rel_path, m.source_type, m.status, c.start_line as i64, c.end_line as i64, &c.heading_path, &c.raw)],
        )?;
        if !carried.is_empty() {
            let chunk_id = tx.last_insert_rowid();
            let pieces: Vec<String> = embed::windows(&c.raw, WINDOW_CHARS, VEC_STRIDE as usize).into_iter().map(|p| embed_text(&m.rel_path, &c.heading_path, &p)).collect();
            if let Some(blobs) = carried.get(&embed::vectors::key(&pieces)).filter(|b| b.len() == pieces.len()) {
                tx.execute("INSERT INTO kb_chunk_vec(chunk_id, windows) VALUES (?1, ?2)", params![chunk_id, blobs.len() as i64])?;
                for (w, blob) in blobs.iter().enumerate() {
                    tx.execute("INSERT INTO kb_vec(rowid, trusted, scope, embedding) VALUES (?1, ?2, ?3, ?4)", params![chunk_id * VEC_STRIDE + w as i64, trusted(m.source_type, m.status) as i64, vec_scope("xenia", &m.rel_path), blob])?;
                }
            }
        }
    }
    Ok(())
}

fn row_of(r: &rusqlite::Row) -> rusqlite::Result<XeniaRow> {
    let list = |s: String| serde_json::from_str::<Vec<String>>(&s).unwrap_or_default();
    let pointers: Vec<(String, String, String)> = serde_json::from_str(&r.get::<_, String>(15)?).unwrap_or_default();
    Ok(XeniaRow {
        rel_path: r.get(0)?,
        org: r.get(1)?,
        store: r.get(2)?,
        id: r.get(3)?,
        rev: r.get::<_, i64>(4)?.max(0) as u64,
        record_hash: r.get(5)?,
        tier: r.get(6)?,
        source_type: r.get(7)?,
        status: r.get(8)?,
        hidden: r.get(9)?,
        superseded_by: r.get(10)?,
        title: r.get(11)?,
        summary: r.get(12)?,
        tags: list(r.get(13)?),
        body: r.get(14)?,
        pointers,
        endorsed_by: list(r.get(16)?),
        withheld_by_staleness: list(r.get(17)?),
        disputed: r.get::<_, i64>(18)? != 0,
        clamped_by_ceiling: r.get::<_, i64>(19)? != 0,
        fresh_until: r.get(20)?,
        head: r.get(21)?,
    })
}

const ROW_COLS: &str = "x.rel_path, x.org, x.store, x.id, x.rev, x.record_hash, x.tier, d.source_type, d.status, x.hidden, x.superseded_by, x.title, x.summary, x.tags, x.body, x.pointers, x.endorsed_by, x.withheld, x.disputed, x.clamped, x.fresh_until, x.head";

impl Kb {
    /// Make the zone of `org`/`store` equal to `memories`, in one transaction. Documents of the store that are not in
    /// `memories` are removed (a record that left force is gone from every search); an entry whose chunk text is
    /// unchanged is updated in place and keeps its vectors; `held_back` (path and fixed slug) is kept for `status`.
    ///
    /// Refuses (changing nothing) a row whose path is not under this store's prefix or whose `source_type` is not one
    /// of the three a Xenia record can have.
    pub fn apply_xenia(&mut self, org: &str, store: &str, head: &str, memories: &[IndexedMemory], held_back: &[(String, &'static str)], now: &str) -> Result<XeniaApply, KbError> {
        let prefix = format!("xenia/{org}/{store}/memory/");
        for m in memories {
            let id_ok = !m.id.is_empty() && m.id.bytes().all(|b| b.is_ascii_alphanumeric());
            if m.org != org || m.store != store || !id_ok || m.rel_path != format!("{prefix}{}", m.id) || !SOURCE_TYPES_XENIA.contains(&m.source_type) || !matches!(m.status, "active" | "superseded") {
                return Err(KbError::Io(format!("refused a xenia row ({}): outside the store's prefix or with a tier a record cannot have", m.rel_path)));
            }
            if (m.source_type == "federated_verified") != (m.tier.as_str() == "L2") || (m.status == "active") == m.hidden.is_some() {
                return Err(KbError::Io(format!("refused a xenia row ({}): tier, source_type and status disagree", m.rel_path)));
            }
        }
        let mut rep = XeniaApply::default();
        let tx = self.conn.transaction()?;
        let existing: HashMap<String, (i64, String, String)> = {
            let mut st = tx.prepare("SELECT d.rel_path, d.doc_id, x.text_sig, x.record_hash FROM kb_documents d LEFT JOIN xenia_records x ON x.doc_id = d.doc_id WHERE d.zone = 'xenia' AND d.rel_path LIKE ?1 ESCAPE '\\'")?;
            let like = format!("{}%", prefix.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            let rows = st.query_map([like], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default(), r.get::<_, Option<String>>(3)?.unwrap_or_default()))))?;
            rows.flatten().collect()
        };
        let mut keep: HashSet<&str> = HashSet::new();
        for m in memories {
            keep.insert(m.rel_path.as_str());
            match existing.get(&m.rel_path) {
                None => {
                    replace(&tx, None, m, now)?;
                    let doc: i64 = tx.query_row("SELECT doc_id FROM kb_documents WHERE rel_path = ?1", [&m.rel_path], |r| r.get(0))?;
                    put_row(&tx, doc, m, head)?;
                    rep.added += 1;
                }
                Some((doc, sig, _)) if *sig == text_sig(m) => {
                    // Skip the write when nothing at all moved (same record bytes, tier, status and endorsement state).
                    let same: bool = tx
                        .query_row(
                            "SELECT x.record_hash = ?2 AND x.tier = ?3 AND d.status = ?4 AND d.source_type = ?5 AND x.endorsed_by = ?6 AND x.withheld = ?7 AND x.disputed = ?8 AND x.clamped = ?9 AND x.fresh_until IS ?10 AND x.superseded_by IS ?11 AND x.hidden IS ?12 AND x.head = ?13 AND x.body = ?14 AND x.pointers = ?15
                             FROM xenia_records x JOIN kb_documents d ON d.doc_id = x.doc_id WHERE x.doc_id = ?1",
                            params![
                                doc,
                                m.hash,
                                m.tier.as_str(),
                                m.status,
                                m.source_type,
                                json_list(&m.endorsed_by),
                                json_list(&m.withheld_by_staleness),
                                m.disputed as i64,
                                m.clamped_by_ceiling as i64,
                                m.fresh_until,
                                m.superseded_by,
                                m.hidden,
                                head,
                                m.body,
                                serde_json::to_string(&m.pointers.iter().map(|p| (p.scheme, p.locator.as_str(), p.label.as_str())).collect::<Vec<_>>()).unwrap_or_default()
                            ],
                            |r| r.get::<_, Option<bool>>(0),
                        )
                        .ok()
                        .flatten()
                        .unwrap_or(false);
                    if same {
                        rep.unchanged += 1;
                    } else {
                        retier(&tx, *doc, m, now)?;
                        put_row(&tx, *doc, m, head)?;
                        rep.retiered += 1;
                    }
                }
                Some((doc, ..)) => {
                    replace(&tx, Some(*doc), m, now)?;
                    let new_doc: i64 = tx.query_row("SELECT doc_id FROM kb_documents WHERE rel_path = ?1", [&m.rel_path], |r| r.get(0))?;
                    put_row(&tx, new_doc, m, head)?;
                    rep.rewritten += 1;
                }
            }
        }
        for (path, (doc, ..)) in &existing {
            if !keep.contains(path.as_str()) {
                drop_vectors(&tx, Some(*doc))?;
                tx.execute("DELETE FROM kb_documents WHERE doc_id = ?1", [doc])?;
                rep.removed += 1;
            }
        }
        let held: Vec<(&str, &str)> = held_back.iter().map(|(p, w)| (p.as_str(), *w)).collect();
        tx.execute("INSERT OR REPLACE INTO kb_meta(key, value) VALUES (?1, ?2)", params![format!("xenia:{org}/{store}:held"), serde_json::to_string(&held).unwrap_or_default()])?;
        tx.execute("INSERT OR REPLACE INTO kb_meta(key, value) VALUES (?1, ?2)", params![format!("xenia:{org}/{store}:head"), head])?;
        tx.commit()?;
        Ok(rep)
    }

    /// The head the zone of this store was last derived from.
    pub fn xenia_head(&self, org: &str, store: &str) -> Option<String> {
        self.conn.query_row("SELECT value FROM kb_meta WHERE key = ?1", [format!("xenia:{org}/{store}:head")], |r| r.get(0)).ok()
    }

    /// Records the last derivation did not put in force (path and fixed slug), for `status`.
    pub fn xenia_held_back(&self, org: &str, store: &str) -> Vec<(String, String)> {
        let raw: Option<String> = self.conn.query_row("SELECT value FROM kb_meta WHERE key = ?1", [format!("xenia:{org}/{store}:held")], |r| r.get(0)).ok();
        raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// The rows for documents (by `rel_path`), in the order asked. Any view: the caller filters by tier.
    pub fn xenia_rows(&self, rel_paths: &[String]) -> Result<Vec<XeniaRow>, KbError> {
        let mut out = Vec::new();
        let mut st = self.conn.prepare(&format!("SELECT {ROW_COLS} FROM xenia_records x JOIN kb_documents d ON d.doc_id = x.doc_id WHERE x.rel_path = ?1"))?;
        for p in rel_paths {
            if let Ok(r) = st.query_row([p], row_of) {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// One record by id, any tier (the quarantine read by id).
    pub fn xenia_get(&self, org: &str, store: &str, id: &str) -> Result<Option<XeniaRow>, KbError> {
        let mut st = self.conn.prepare(&format!("SELECT {ROW_COLS} FROM xenia_records x JOIN kb_documents d ON d.doc_id = x.doc_id WHERE x.org = ?1 AND x.store = ?2 AND x.id = ?3"))?;
        Ok(st.query_row(params![org, store, id], row_of).ok())
    }

    /// Every record of the zone, newest id last. Console listing; never offered to an agent.
    pub fn xenia_all(&self) -> Result<Vec<XeniaRow>, KbError> {
        let mut st = self.conn.prepare(&format!("SELECT {ROW_COLS} FROM xenia_records x JOIN kb_documents d ON d.doc_id = x.doc_id ORDER BY x.org, x.store, x.id"))?;
        let rows = st.query_map([], row_of)?;
        Ok(rows.flatten().collect())
    }

    /// The `trusted` bit of every vector of one record's chunks (a test hook: the bit must follow the tier).
    pub fn xenia_vector_trust(&self, rel_path: &str) -> Result<Vec<i64>, KbError> {
        if !has_vec(&self.conn) {
            return Ok(vec![]);
        }
        let chunks: Vec<(i64, i64)> = {
            let mut st = self.conn.prepare("SELECT c.chunk_id, m.windows FROM kb_chunks c JOIN kb_chunk_vec m ON m.chunk_id = c.chunk_id WHERE c.rel_path = ?1 ORDER BY c.chunk_id")?;
            let rows = st.query_map([rel_path], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.flatten().collect()
        };
        let mut out = Vec::new();
        for (chunk, windows) in chunks {
            for w in 0..windows {
                out.push(self.conn.query_row("SELECT trusted FROM kb_vec WHERE rowid = ?1", [chunk * VEC_STRIDE + w], |r| r.get(0))?);
            }
        }
        Ok(out)
    }
}
