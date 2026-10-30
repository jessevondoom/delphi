//! `delphi kb serve`: the knowledge base as a small, self-hosted, read-only JSON API (`specs/028` §6).
//!
//! * Read-only by construction: only `GET` and `HEAD` are routed, nothing is written to the workspace, and a file is
//!   served only when the index lists it as visible. Paths are logical (`specs/004`), never filesystem paths, so
//!   `..` cannot leave the indexed tree.
//! * What it serves is what an unprivileged search sees: trusted, active documents; no `memory/agents/`; nothing
//!   untrusted. There is no switch to widen that over the wire.
//! * Search is the one the console uses (keyword + vector + reranker, `specs/004` §3), with no model call to
//!   interpret the query or the answer: a request costs no tokens.
//! * A bearer token is always required, and on a loopback address the `Host` header must name the loopback address,
//!   so a web page cannot reach it through the operator's browser (DNS rebinding). No CORS headers are sent.
//! * The default bind is loopback. Anything else needs `--allow-remote` and is plain HTTP: put TLS in front of it.

use crate::kb::{self, Kb, SearchOpts};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_URL: usize = 2048;
const MAX_QUERY_CHARS: usize = 500;
const MAX_LIMIT: usize = 50;
/// How many ranked candidates a search looks at when it counts kinds (`facets`) or filters by one (`specs/043` §3.2).
const FACET_CANDIDATES: usize = 200;

/// The kind of a document, from where it lives (`specs/043` §4.1). A display and filter label: it carries no authority
/// and is never read from the document's own text. The first match wins; `lessons` is cut out of `context/` before
/// `docs` takes the rest of it.
pub fn kind_of(rel_path: &str, zone: &str) -> &'static str {
    let p = rel_path.trim_start_matches('/');
    if p.starts_with("knowledge/web/") {
        "research"
    } else if p.starts_with("context/lessons/") {
        "lesson"
    } else if p.starts_with("docs/") || p.starts_with("context/") {
        "docs"
    } else if p.starts_with("specs/") {
        "spec"
    } else if p.starts_with("missions/") {
        "mission"
    } else if p.starts_with("skills/") || zone == "skills" {
        "skill"
    } else if p.starts_with("memory/") {
        "memory"
    } else {
        "other"
    }
}
/// How long the index may go without being brought up to date (unchanged files cost a stat).
const REFRESH_EVERY: Duration = Duration::from_secs(10);
/// A document larger than this is not served whole (the indexer's own cap is 512 KiB).
const MAX_DOC_BYTES: u64 = 512 * 1024;
/// An image file larger than this is not drawn (`specs/029` §9); the reader shows its alt text instead.
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
/// The most pixels a side of the picture sent to the console has: the reader shrinks it to the window, so more is waste.
const IMAGE_SIDE: usize = 160;

pub struct Reply {
    pub status: u16,
    pub body: Value,
}

fn reply(status: u16, body: Value) -> Reply {
    Reply { status, body }
}

fn fail(status: u16, msg: &str) -> Reply {
    reply(status, json!({"error": msg}))
}

pub struct Api {
    kb: Kb,
    ws: PathBuf,
    refreshed: Instant,
}

impl Api {
    pub fn new(home: &Path, ws: &Path) -> Result<Api, String> {
        let _ = std::fs::create_dir_all(ws.join("local"));
        let mut kb = Kb::open(&ws.join("local/state.db")).map_err(|e| e.to_string())?;
        kb::attach_default(home, &mut kb).map_err(|e| e.to_string())?;
        kb.refresh(ws, "").map_err(|e| e.to_string())?;
        Ok(Api { kb, ws: ws.to_path_buf(), refreshed: Instant::now() })
    }

    fn fresh(&mut self) {
        if self.refreshed.elapsed() >= REFRESH_EVERY {
            let _ = self.kb.refresh(&self.ws, "");
            self.refreshed = Instant::now();
        }
    }

    /// Route one already-authenticated request.
    pub fn handle(&mut self, method: &str, target: &str) -> Reply {
        if method != "GET" && method != "HEAD" {
            return fail(405, "this API is read-only: only GET and HEAD");
        }
        self.fresh();
        Reader { kb: &self.kb, ws: &self.ws, operator: false }.get(target)
    }
}

/// The routes, over a knowledge base someone else keeps open and fresh. The HTTP server holds one for its lifetime;
/// the daemon opens one per call for `kb.get`, so the console and narrator read through the same code.
pub struct Reader<'a> {
    pub kb: &'a Kb,
    pub ws: &'a Path,
    /// A person at the narrator, through the daemon's socket, rather than a program on the HTTP port. The operator
    /// sees every document, private agent memory included, each tagged with its provenance, and may ask for the
    /// trusted ones only (`trusted_only=1`). Over HTTP the view is always the restricted one and the parameter is ignored.
    pub operator: bool,
}

impl Reader<'_> {
    /// The documents this request may see.
    fn view(&self, q: &HashMap<String, String>) -> SearchOpts {
        if self.operator {
            SearchOpts { role: None, include_untrusted: q.get("trusted_only").map(String::as_str) != Some("1"), ..Default::default() }
        } else {
            // Trusted and active documents, and no private agent memory (no role is called "none").
            SearchOpts { role: Some("none".into()), include_untrusted: false, ..Default::default() }
        }
    }

    /// `GET target` (a path and query such as `/v1/search?q=retry`). Never writes anything.
    pub fn get(&self, target: &str) -> Reply {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let q = parse_query(query);
        let r = match path {
            "/v1/health" => self.health(),
            "/v1/collections" => self.collections(&q),
            "/v1/kinds" => self.kinds(&q),
            "/v1/documents" => self.documents(&q),
            "/v1/search" => self.search(&q),
            "/v1/doc" => self.doc(&q),
            "/v1/image" => self.image(&q),
            _ => return fail(404, "no such endpoint: /v1/health /v1/collections /v1/kinds /v1/documents /v1/search /v1/doc /v1/image"),
        };
        r.unwrap_or_else(|e| fail(500, &e))
    }

    fn health(&self) -> Result<Reply, String> {
        let (documents, chunks) = self.kb.counts().map_err(|e| e.to_string())?;
        let vectors = self.kb.vector_counts().map(|v| v.0).unwrap_or(0);
        let model = self.kb.embedder().map(|e| e.model_id().to_string());
        Ok(reply(200, json!({"ok": true, "api": 1, "documents": documents, "passages": chunks, "vectors": vectors, "model": model, "reranker": self.kb.reranker().is_some()})))
    }

    fn collections(&self, q: &HashMap<String, String>) -> Result<Reply, String> {
        let c = self.kb.collections(&self.view(q)).map_err(|e| e.to_string())?;
        Ok(reply(200, json!(c.into_iter().map(|(name, count, updated)| json!({"name": name, "documents": count, "updated": updated})).collect::<Vec<_>>())))
    }

    /// Every visible document counted by kind (`specs/043` §4), newest change first among the kinds with the same count.
    fn kinds(&self, q: &HashMap<String, String>) -> Result<Reply, String> {
        let docs = self.kb.documents(None, 100_000, 0, &self.view(q)).map_err(|e| e.to_string())?;
        let mut n: std::collections::BTreeMap<&'static str, (usize, String)> = std::collections::BTreeMap::new();
        for d in &docs {
            let e = n.entry(kind_of(&d.rel_path, &d.zone)).or_insert((0, String::new()));
            e.0 += 1;
            if d.updated_at > e.1 {
                e.1 = d.updated_at.clone();
            }
        }
        Ok(reply(200, json!(n.into_iter().map(|(kind, (documents, updated))| json!({"kind": kind, "documents": documents, "updated": updated})).collect::<Vec<_>>())))
    }

    fn documents(&self, q: &HashMap<String, String>) -> Result<Reply, String> {
        let limit = num(q, "limit", 20).clamp(1, 200);
        let offset = num(q, "offset", 0).min(100_000);
        let collection = q.get("collection").map(String::as_str);
        let with_kind = |d: &kb::DocInfo| {
            let mut v = json!(d);
            v["kind"] = json!(kind_of(&d.rel_path, &d.zone));
            v
        };
        let Some(kind) = q.get("kind").filter(|k| !k.is_empty()) else {
            let docs = self.kb.documents(collection, limit, offset, &self.view(q)).map_err(|e| e.to_string())?;
            return Ok(reply(200, json!(docs.iter().map(with_kind).collect::<Vec<_>>())));
        };
        // A kind comes from the path, so it is filtered here, after the newest-first listing.
        let all = self.kb.documents(collection, 100_000, 0, &self.view(q)).map_err(|e| e.to_string())?;
        let docs: Vec<Value> = all.iter().filter(|d| kind_of(&d.rel_path, &d.zone) == kind.as_str()).skip(offset).take(limit).map(with_kind).collect();
        Ok(reply(200, json!(docs)))
    }

    fn search(&self, q: &HashMap<String, String>) -> Result<Reply, String> {
        let query = q.get("q").map(|s| s.trim()).unwrap_or("");
        if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
            return Ok(fail(400, "`q` must be 1 to 500 characters"));
        }
        let limit = num(q, "limit", 10).clamp(1, MAX_LIMIT);
        let collection = q.get("collection").filter(|c| !c.is_empty());
        let kind = q.get("kind").filter(|k| !k.is_empty());
        let facets = q.get("facets").map(String::as_str) == Some("1");
        let mut opts = self.view(q);
        // A collection or kind filter is applied after ranking, and the kind counts look at many candidates, so ask for
        // the most there is.
        opts.limit = if facets || kind.is_some() { FACET_CANDIDATES } else if collection.is_some() { MAX_LIMIT } else { limit };
        if q.get("rerank").map(String::as_str) == Some("0") {
            opts.rerank_weight = 0.0;
        }
        let started = Instant::now();
        let hits = self.kb.search(query, &opts).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for h in hits {
            let Some(d) = self.kb.document(&h.rel_path, &self.view(q)).map_err(|e| e.to_string())? else { continue };
            if collection.map(|c| c != &d.zone).unwrap_or(false) {
                continue;
            }
            out.push(json!({"pointer": h.pointer, "path": h.rel_path, "collection": d.zone, "kind": kind_of(&d.rel_path, &d.zone), "updated": d.updated_at, "trust": d.source_type, "status": d.status, "heading": h.heading_path, "preview": h.preview, "start_line": h.start_line, "end_line": h.end_line, "blake3": h.blake3, "score": h.score, "note": h.note, "knowledge_status": h.status, "label": h.shown_label(), "vouched_by": h.vouched_by}));
            if !facets && kind.is_none() && out.len() >= limit {
                break;
            }
        }
        let candidates = out.len();
        // The counts are over every candidate, before the kind filter, so the chips keep their numbers when one is chosen.
        let counts = facets.then(|| {
            let mut n: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
            for r in &out {
                *n.entry(r["kind"].as_str().unwrap_or("other").to_string()).or_insert(0) += 1;
            }
            n
        });
        if let Some(k) = kind {
            out.retain(|r| r["kind"].as_str() == Some(k.as_str()));
        }
        out.truncate(limit);
        let mut body = json!({"query": query, "took_ms": started.elapsed().as_millis() as u64, "mode": if self.kb.embedder().is_some() { "hybrid" } else { "lexical" }, "results": out});
        if let Some(n) = counts {
            body["facets"] = json!(n);
            body["candidates"] = json!(candidates);
            body["capped"] = json!(candidates >= FACET_CANDIDATES);
        }
        Ok(reply(200, body))
    }

    fn doc(&self, q: &HashMap<String, String>) -> Result<Reply, String> {
        let Some(path) = q.get("path").filter(|p| !p.is_empty() && !p.contains('\0')) else { return Ok(fail(400, "`path` is required")) };
        let Some(info) = self.kb.document(path, &self.view(q)).map_err(|e| e.to_string())? else { return Ok(fail(404, "not in the knowledge base")) };
        // The file comes from the indexer's own list, so the logical path cannot name anything else.
        let Some(src) = kb::sources(self.ws).into_iter().find(|s| s.rel_path == info.rel_path) else { return Ok(fail(404, "no longer on disk")) };
        let binary = crate::kb_docs::is_binary_doc(&src.rel_path);
        if !binary && std::fs::metadata(&src.path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_DOC_BYTES {
            return Ok(fail(413, "document too large to serve"));
        }
        // A PDF, DOCX or EPUB is served as the text the index holds for it (`specs/043` §6.2).
        let text = if binary { crate::kb_docs::text_of(&src.path).ok_or("no longer on disk")? } else { std::fs::read_to_string(&src.path).map_err(|e| e.to_string())? };
        let live = blake3::hash(text.as_bytes()).to_hex().to_string();
        let mut links = extract_links(&info.rel_path, &text, &|p| self.kb.document(p, &self.view(q)).ok().flatten().is_some());
        // An outside address is followed only if its host is on the operator's lists (`specs/043` §3.4); the console says so.
        let home = self.ws.parent().and_then(Path::parent).unwrap_or(self.ws);
        let lists = crate::research_setup::lists(self.ws, home).ok();
        for l in links.iter_mut().filter(|l| l["kind"] == "external") {
            l["listed"] = json!(lists.as_ref().zip(l["target"].as_str()).is_some_and(|(t, u)| t.allows_url(u).is_some()));
        }
        Ok(reply(200, json!({"path": info.rel_path, "collection": info.zone, "trust": info.source_type, "status": info.status, "blake3": live, "indexed_blake3": info.blake3, "stale": live != info.blake3, "text": text, "links": links})))
    }
}

impl Reader<'_> {
    /// `GET /v1/image?path=<document>&src=<href as written in it>`: a picture the document points at, decoded and
    /// shrunk to at most 160 pixels a side, as `{width, height, rgba}` (base64, four bytes a pixel). The reader paints
    /// it in half-block cells. The file must sit inside the document's own directory (no `..`, no absolute path, no
    /// symlink out of it) and be a PNG, JPEG or GIF of at most 8 MiB; the document must be one this view may read.
    fn image(&self, q: &HashMap<String, String>) -> Result<Reply, String> {
        let (Some(path), Some(href)) = (q.get("path").filter(|p| !p.is_empty()), q.get("src").filter(|s| !s.is_empty())) else {
            return Ok(fail(400, "`path` and `src` are required"));
        };
        let href = href.split(['#', '?']).next().unwrap_or("");
        let lower = href.to_ascii_lowercase();
        let named_ok = !href.is_empty()
            && !href.starts_with('/')
            && !href.contains(['\0', '\\', ':'])
            && href.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
            && [".png", ".jpg", ".jpeg", ".gif"].iter().any(|e| lower.ends_with(e));
        if !named_ok {
            return Ok(fail(400, "`src` must be a relative path to a .png, .jpg, .jpeg or .gif file below the document's directory"));
        }
        let Some(info) = self.kb.document(path, &self.view(q)).map_err(|e| e.to_string())? else { return Ok(fail(404, "not in the knowledge base")) };
        let Some(src) = kb::sources(self.ws).into_iter().find(|s| s.rel_path == info.rel_path) else { return Ok(fail(404, "no longer on disk")) };
        let Some(dir) = src.path.parent().and_then(|d| d.canonicalize().ok()) else { return Ok(fail(404, "no longer on disk")) };
        let Ok(file) = dir.join(href).canonicalize() else { return Ok(fail(404, "no such image next to the document")) };
        if !file.starts_with(&dir) || !file.is_file() {
            return Ok(fail(404, "no such image next to the document"));
        }
        if std::fs::metadata(&file).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_IMAGE_BYTES {
            return Ok(fail(413, "image too large to draw"));
        }
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        let img = match delphi_docs::pixels::decode(&bytes) {
            Ok(i) => delphi_docs::pixels::fit(&i, IMAGE_SIDE, IMAGE_SIDE),
            Err(e) => return Ok(fail(415, &e.0)),
        };
        use base64::Engine as _;
        Ok(reply(200, json!({"width": img.width, "height": img.height, "rgba": base64::engine::general_purpose::STANDARD.encode(&img.rgba)})))
    }
}

fn num(q: &HashMap<String, String>, key: &str, default: usize) -> usize {
    q.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn hex_val(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() && hex_val(b[i + 1]).is_some() && hex_val(b[i + 2]).is_some() => {
                out.push(hex_val(b[i + 1]).unwrap() * 16 + hex_val(b[i + 2]).unwrap());
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&').filter(|p| !p.is_empty()).map(|p| {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        (percent_decode(k), percent_decode(v))
    }).collect()
}

/// The links of a Markdown document, in reading order, each with where it points. Fenced code is skipped.
/// `kind`: `doc` (another indexed document, resolved to its logical path), `missing` (a relative link to nothing
/// indexed), `anchor` (a heading of this document), `external` (never fetched by this API), `other`.
pub fn extract_links(rel_path: &str, text: &str, exists: &dyn Fn(&str) -> bool) -> Vec<Value> {
    let re = regex::Regex::new(r#"\[([^\]\n]{1,200})\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)"#).unwrap();
    let dir = rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut out = Vec::new();
    let mut fenced = false;
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for c in re.captures_iter(line) {
            let (label, target) = (&c[1], &c[2]);
            let mut l = json!({"text": label, "target": target, "line": n + 1});
            let (kind, resolved, anchor) = resolve_link(dir, target, exists);
            l["kind"] = json!(kind);
            if let Some(p) = resolved {
                l["path"] = json!(p);
            }
            if let Some(a) = anchor {
                l["anchor"] = json!(a);
            }
            out.push(l);
        }
    }
    out
}

fn resolve_link(dir: &str, target: &str, exists: &dyn Fn(&str) -> bool) -> (&'static str, Option<String>, Option<String>) {
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
        return ("external", None, None);
    }
    if let Some(a) = target.strip_prefix('#') {
        return ("anchor", None, Some(a.to_string()));
    }
    if target.contains(':') {
        return ("other", None, None); // another scheme (`file:`, `javascript:`): never followed
    }
    let (file, anchor) = match target.split_once('#') {
        Some((f, a)) => (f, Some(a.to_string())),
        None => (target, None),
    };
    let mut parts: Vec<&str> = if file.starts_with('/') { vec![] } else { dir.split('/').filter(|s| !s.is_empty()).collect() };
    for seg in file.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return ("other", None, None); // would leave the indexed tree
                }
            }
            s => parts.push(s),
        }
    }
    let path = parts.join("/");
    if !path.is_empty() && exists(&path) {
        ("doc", Some(path), anchor)
    } else {
        ("missing", None, anchor)
    }
}

/// Why a request is refused before routing, if it is: the token, and (on loopback) the `Host` header.
pub fn refuse(auth: Option<&str>, host: Option<&str>, token: &str, loopback: bool) -> Option<Reply> {
    let given = auth.and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    if token.is_empty() || !constant_eq(given.as_bytes(), token.as_bytes()) {
        return Some(fail(401, "a bearer token is required"));
    }
    if loopback {
        let h = host.unwrap_or("");
        let name = h.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map(|(n, _)| n).unwrap_or(h);
        if !matches!(name, "127.0.0.1" | "localhost" | "[::1]") {
            return Some(fail(421, "this server answers only to its loopback address"));
        }
    }
    None
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().min(b.len()) {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

pub fn is_loopback_bind(bind: &str) -> bool {
    let host = bind.rsplit_once(':').map(|(h, _)| h).unwrap_or(bind);
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

pub struct Running {
    pub addr: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Running {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    /// Serve until the process is stopped.
    pub fn wait(mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start serving on `bind` (`host:port`; port 0 picks one). The index is built before the first request.
pub fn start(home: &Path, ws: &Path, bind: &str, token: &str) -> Result<Running, String> {
    let mut api = Api::new(home, ws)?;
    let server = tiny_http::Server::http(bind).map_err(|e| format!("cannot listen on {}: {}", bind, e))?;
    let addr = server.server_addr().to_ip().ok_or("not an IP address")?;
    let (stop, loopback, token) = (Arc::new(AtomicBool::new(false)), is_loopback_bind(bind), token.to_string());
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            let Ok(Some(req)) = server.recv_timeout(Duration::from_millis(200)) else { continue };
            let header = |name: &str| req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string());
            let (method, url) = (req.method().as_str().to_string(), req.url().to_string());
            let r = if url.len() > MAX_URL {
                fail(414, "URL too long")
            } else if let Some(r) = refuse(header("Authorization").as_deref(), header("Host").as_deref(), &token, loopback) {
                r
            } else {
                api.handle(&method, &url)
            };
            let body = if method == "HEAD" { String::new() } else { r.body.to_string() };
            let mut resp = tiny_http::Response::from_string(body).with_status_code(r.status);
            for (k, v) in [("Content-Type", "application/json; charset=utf-8"), ("Cache-Control", "no-store"), ("X-Content-Type-Options", "nosniff")] {
                resp.add_header(tiny_http::Header::from_bytes(k, v).unwrap());
            }
            if r.status == 401 {
                resp.add_header(tiny_http::Header::from_bytes("WWW-Authenticate", "Bearer").unwrap());
            }
            if r.status == 405 {
                resp.add_header(tiny_http::Header::from_bytes("Allow", "GET, HEAD").unwrap());
            }
            let _ = req.respond(resp);
        }
    });
    Ok(Running { addr, stop, thread: Some(thread) })
}

/// The token for this workspace: read from `path`, or made (32 random bytes, hex, mode 0600) when missing.
pub fn token_at(path: &Path) -> Result<String, String> {
    if let Ok(t) = std::fs::read_to_string(path) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    let mut bytes = [0u8; 32];
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).map_err(|e| format!("cannot make a token: {}", e))?;
    let token: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).map_err(|e| e.to_string())?;
        f.write_all(token.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(token)
}

const USAGE: &str = "usage: delphi kb serve [--bind <host:port>] [--token-file <path>] [--allow-remote]\n  a read-only JSON API over the knowledge base: /v1/search?q=  /v1/doc?path=  /v1/collections  /v1/kinds  /v1/documents  /v1/health\n  every request needs `Authorization: Bearer <token>`; the token is made on first use and kept in the token file";

/// `delphi kb serve`.
pub fn run(home: &Path, ws: &Path, args: &[String]) -> i32 {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", USAGE);
        return 0;
    }
    let bind = flag("--bind").unwrap_or_else(|| "127.0.0.1:7878".into());
    if !is_loopback_bind(&bind) && !args.iter().any(|a| a == "--allow-remote") {
        eprintln!("refusing to listen on {}: that is not this machine's loopback address. The API is plain HTTP; add --allow-remote only behind TLS you control.", bind);
        return 2;
    }
    let token_path = flag("--token-file").map(PathBuf::from).unwrap_or_else(|| ws.join("local/kb-serve.token"));
    let token = match token_at(&token_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    match start(home, ws, &bind, &token) {
        Ok(run) => {
            println!("kb: serving read-only on http://{}", run.addr);
            println!("kb: token file {} (send it as `Authorization: Bearer <token>`)", token_path.display());
            println!("kb: try  curl -H \"Authorization: Bearer $(cat {})\" 'http://{}/v1/search?q=council+deadlock'", token_path.display(), run.addr);
            run.wait();
            0
        }
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn doc(source: &str, body: &str) -> String {
        format!("---\ntitle: t\nsource_type: {}\nstatus: active\n---\n{}", source, body)
    }

    fn workspace() -> (tempfile::TempDir, tempfile::TempDir) {
        let (home, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let specs = ws.path().join("base/specs");
        std::fs::create_dir_all(&specs).unwrap();
        std::fs::write(specs.join("alpha.md"), doc("internal_generated", "# Alpha\n\nThe zebra council escalates a deadlock to the operator. See [beta](beta.md#intro), [gone](nope.md), [web](https://example.com/x), [here](#alpha) and [out](../../etc/passwd).\n\n```\n[ignored](beta.md)\n```\n")).unwrap();
        std::fs::write(specs.join("beta.md"), doc("internal_generated", "# Beta\n\nIntro text about zebra crossings.\n")).unwrap();
        std::fs::write(specs.join("web.md"), doc("external_untrusted", "# Web\n\nzebra from the open web.\n")).unwrap();
        let mem = ws.path().join("local/memory/agents/builder");
        std::fs::create_dir_all(&mem).unwrap();
        std::fs::write(mem.join("notes.md"), doc("internal_generated", "# Private\n\nzebra secret of the builder.\n")).unwrap();
        (home, ws)
    }

    fn get(api: &mut Api, target: &str) -> (u16, Value) {
        let r = api.handle("GET", target);
        (r.status, r.body)
    }

    #[test]
    fn a_kind_comes_from_where_a_document_lives() {
        for (path, zone, want) in [
            ("knowledge/web/en.wikipedia.org/bft.md", "knowledge", "research"),
            ("context/lessons/council-deadlock.md", "context", "lesson"),
            ("context/architecture/overview.md", "context", "docs"),
            ("docs/reference/council.md", "docs", "docs"),
            ("specs/043-knowledge-window.md", "specs", "spec"),
            ("missions/m1/notes.md", "missions", "mission"),
            ("skills/repo-cartography/SKILL.md", "skills", "skill"),
            ("bundles/acme/platform/skills/s/references/a.md", "skills", "skill"),
            ("memory/agents/builder/notes.md", "memory", "memory"),
            ("GOVERNANCE.md", "core", "other"),
            ("agents/architect/AGENT.md", "agents", "other"),
        ] {
            assert_eq!(kind_of(path, zone), want, "{path}");
        }
        assert_eq!(kind_of("/specs/a.md", "specs"), "spec", "a leading slash changes nothing");
        assert_eq!(kind_of("context/lessons/x.md", "context"), "lesson", "lessons are cut out of context before docs takes the rest");
    }

    #[test]
    fn search_counts_kinds_over_the_candidates_and_a_kind_filter_keeps_the_counts() {
        let (home, ws) = workspace();
        let lessons = ws.path().join("base/context/lessons");
        std::fs::create_dir_all(&lessons).unwrap();
        std::fs::write(lessons.join("zebra.md"), doc("internal_generated", "# Lesson\n\nA zebra lesson about deadlocks.\n")).unwrap();
        std::fs::write(ws.path().join("base/context/notes.md"), doc("internal_generated", "# Notes\n\nZebra reference notes.\n")).unwrap();
        let mut api = Api::new(home.path(), ws.path()).unwrap();
        let (s, all) = get(&mut api, "/v1/search?q=zebra&rerank=0&facets=1");
        assert_eq!(s, 200, "{all}");
        assert_eq!(all["facets"], json!({"docs": 1, "lesson": 1, "spec": 2}), "untrusted and private memory stay out of the counts: {all}");
        assert_eq!((all["candidates"].as_u64(), all["capped"].as_bool()), (Some(4), Some(false)));
        assert!(all["results"].as_array().unwrap().iter().all(|r| r["kind"].is_string() && r["updated"].is_string()));
        let (_, specs) = get(&mut api, "/v1/search?q=zebra&rerank=0&facets=1&kind=spec");
        let kinds: Vec<&str> = specs["results"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["spec", "spec"]);
        assert_eq!(specs["facets"], all["facets"], "the chips keep their counts when one is chosen");
        let (_, one) = get(&mut api, "/v1/search?q=zebra&rerank=0&kind=spec&limit=1");
        assert_eq!(one["results"].as_array().unwrap().len(), 1);
        assert!(one.get("facets").is_none(), "no counts unless asked for");
        let (_, none) = get(&mut api, "/v1/search?q=zebra&rerank=0&kind=research");
        assert_eq!(none["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn kinds_lists_every_visible_document_by_kind_and_documents_can_be_filtered_by_one() {
        let (home, ws) = workspace();
        let lessons = ws.path().join("base/context/lessons");
        std::fs::create_dir_all(&lessons).unwrap();
        std::fs::write(lessons.join("a.md"), doc("internal_generated", "# A\n\nx\n")).unwrap();
        let mut api = Api::new(home.path(), ws.path()).unwrap();
        let (s, k) = get(&mut api, "/v1/kinds");
        assert_eq!(s, 200, "{k}");
        let n = |kind: &str| k.as_array().unwrap().iter().find(|e| e["kind"] == kind).map(|e| e["documents"].as_u64().unwrap());
        assert_eq!((n("spec"), n("lesson"), n("memory")), (Some(2), Some(1), None), "the untrusted web page and private memory are not counted: {k}");
        let (_, docs) = get(&mut api, "/v1/documents?kind=lesson");
        assert_eq!(docs.as_array().unwrap().len(), 1);
        assert_eq!(docs[0]["kind"], "lesson", "a listing says each document's kind");
        let (_, plain) = get(&mut api, "/v1/documents?limit=5");
        assert!(plain.as_array().unwrap().iter().all(|d| d["kind"].is_string()), "also without a kind filter");
        let (_, specs) = get(&mut api, "/v1/documents?kind=spec&limit=1&offset=1");
        assert_eq!(specs.as_array().unwrap().len(), 1, "offset and limit apply after the kind filter");
        assert_eq!(get(&mut api, "/v1/nonsense").0, 404);
    }

    #[test]
    fn search_is_ranked_trusted_only_and_filterable_by_collection() {
        let (home, ws) = workspace();
        let mut api = Api::new(home.path(), ws.path()).unwrap();
        let (s, b) = get(&mut api, "/v1/search?q=zebra&rerank=0");
        assert_eq!(s, 200, "{b}");
        let paths: Vec<&str> = b["results"].as_array().unwrap().iter().map(|r| r["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"specs/alpha.md") && paths.contains(&"specs/beta.md"), "{paths:?}");
        assert!(!paths.iter().any(|p| p.contains("web.md") || p.contains("memory")), "untrusted and private memory stay out: {paths:?}");
        let r0 = &b["results"][0];
        assert!(r0["collection"] == "specs" && r0["pointer"].is_string() && r0["preview"].is_string() && r0["start_line"].is_number());
        let (_, none) = get(&mut api, "/v1/search?q=zebra&collection=skills&rerank=0");
        assert_eq!(none["results"].as_array().unwrap().len(), 0);
        assert_eq!(get(&mut api, "/v1/search?q=").0, 400);
        assert_eq!(get(&mut api, &format!("/v1/search?q={}", "a".repeat(501))).0, 400);
        let (_, lim) = get(&mut api, "/v1/search?q=zebra&limit=1&rerank=0");
        assert_eq!(lim["results"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn images_come_from_beside_a_visible_document_and_nowhere_else() {
        let (home, ws) = workspace();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-docs/tests/images");
        let specs = ws.path().join("base/specs");
        std::fs::create_dir_all(specs.join("img")).unwrap();
        std::fs::copy(fixtures.join("grad.png"), specs.join("img/grad.png")).unwrap();
        std::fs::copy(fixtures.join("grad_420.jpg"), specs.join("photo.JPG")).unwrap();
        std::fs::write(specs.join("notes.txt"), "not an image").unwrap();
        std::fs::write(specs.join("broken.png"), b"\x89PNG not really").unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::copy(fixtures.join("grad.png"), elsewhere.path().join("secret.png")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path().join("secret.png"), specs.join("leak.png")).unwrap();
        let mut api = Api::new(home.path(), ws.path()).unwrap();
        let (s, b) = get(&mut api, "/v1/image?path=specs/alpha.md&src=img/grad.png");
        assert_eq!(s, 200, "{b}");
        assert_eq!((b["width"].as_u64(), b["height"].as_u64()), (Some(40), Some(24)));
        use base64::Engine as _;
        let px = base64::engine::general_purpose::STANDARD.decode(b["rgba"].as_str().unwrap()).unwrap();
        assert_eq!(px.len(), 40 * 24 * 4);
        assert_eq!(get(&mut api, "/v1/image?path=specs/alpha.md&src=photo.JPG").0, 200, "the extension is matched without regard to case");
        for (src, want) in [("../web.png", 400), ("/etc/passwd", 400), ("img/../photo.JPG", 400), ("notes.txt", 400), ("file:///x.png", 400), ("img%5Cgrad.png", 400), ("missing.png", 404), ("leak.png", 404), ("broken.png", 415)] {
            let (s, b) = get(&mut api, &format!("/v1/image?path=specs/alpha.md&src={src}"));
            assert_eq!(s, want, "{src}: {b}");
        }
        assert_eq!(get(&mut api, "/v1/image?path=specs/web.md&src=img/grad.png").0, 404, "an untrusted document is not served over this view, nor are its images");
        assert_eq!(get(&mut api, "/v1/image?path=specs/alpha.md").0, 400);
        assert_eq!(get(&mut api, "/v1/image?path=specs/nope.md&src=img/grad.png").0, 404);
    }

    #[test]
    fn a_document_comes_with_its_links_and_only_if_visible() {
        let (home, ws) = workspace();
        let mut api = Api::new(home.path(), ws.path()).unwrap();
        let (s, d) = get(&mut api, "/v1/doc?path=specs/alpha.md");
        assert_eq!(s, 200, "{d}");
        assert!(d["text"].as_str().unwrap().contains("zebra council") && d["stale"] == false);
        let links = d["links"].as_array().unwrap();
        let by = |t: &str| links.iter().find(|l| l["text"] == t).unwrap_or_else(|| panic!("no link {t}: {links:?}"));
        assert_eq!((by("beta")["kind"].as_str(), by("beta")["path"].as_str(), by("beta")["anchor"].as_str()), (Some("doc"), Some("specs/beta.md"), Some("intro")));
        assert_eq!(by("gone")["kind"], "missing");
        assert_eq!((by("web")["kind"].as_str(), by("web")["listed"].as_bool()), (Some("external"), Some(false)), "example.com is on no list");
        assert!(by("beta").get("listed").is_none(), "only an outside address has a list to be on");
        assert_eq!((by("here")["kind"].as_str(), by("here")["anchor"].as_str()), (Some("anchor"), Some("alpha")));
        assert_eq!(by("out")["kind"], "other", "a link that leaves the tree is not resolved");
        assert!(!links.iter().any(|l| l["line"] == 9), "links inside a fence are not links");
        for p in ["specs/web.md", "memory/agents/builder/notes.md", "../../etc/passwd", "/etc/passwd", "specs/../specs/alpha.md", "specs/nope.md"] {
            assert_eq!(get(&mut api, &format!("/v1/doc?path={}", p)).0, 404, "{p}");
        }
        assert_eq!(get(&mut api, "/v1/doc").0, 400);
    }

    #[test]
    fn collections_documents_and_health() {
        let (home, ws) = workspace();
        let mut api = Api::new(home.path(), ws.path()).unwrap();
        let (_, c) = get(&mut api, "/v1/collections");
        assert_eq!(c.as_array().unwrap().len(), 1, "{c}");
        assert_eq!((c[0]["name"].as_str(), c[0]["documents"].as_u64()), (Some("specs"), Some(2)));
        assert!(c[0]["updated"].as_str().unwrap().starts_with("20"), "a real date, from the files: {c}");
        let (_, d) = get(&mut api, "/v1/documents?collection=specs&limit=1");
        assert_eq!(d.as_array().unwrap().len(), 1);
        let (s, h) = get(&mut api, "/v1/health");
        assert!(s == 200 && h["ok"] == true && h["documents"].as_u64().unwrap() >= 2, "{h}");
        assert_eq!(get(&mut api, "/v1/nope").0, 404);
    }

    #[test]
    fn an_outside_link_says_whether_its_host_is_on_the_operators_list() {
        let (home, ws) = workspace();
        std::fs::write(ws.path().join("base/specs/refs.md"), doc("internal_generated", "# Refs\n\nSee [wiki](https://en.wikipedia.org/wiki/Zebra) and [lookalike](https://en.wikipedia.org.evil.example/) and [plain](http://en.wikipedia.org/x).\n")).unwrap();
        let api = Api::new(home.path(), ws.path()).unwrap();
        let d = Reader { kb: &api.kb, ws: &api.ws, operator: true }.get("/v1/doc?path=specs/refs.md").body;
        let listed: Vec<bool> = d["links"].as_array().unwrap().iter().map(|l| l["listed"].as_bool().unwrap()).collect();
        assert_eq!(listed, [true, false, false]);
    }

    #[test]
    fn a_media_file_and_an_unreadable_pdf_open_in_the_reader_as_their_listing() {
        let (home, ws) = workspace();
        let ctx = ws.path().join("base/context");
        std::fs::create_dir_all(&ctx).unwrap();
        std::fs::write(ctx.join("clip.mp4"), b"x").unwrap();
        std::fs::write(ctx.join("scan.pdf"), b"%PDF-1.4 junk").unwrap();
        let api = Api::new(home.path(), ws.path()).unwrap();
        let op = Reader { kb: &api.kb, ws: &api.ws, operator: true };
        let clip = op.get("/v1/doc?path=context/clip.mp4");
        assert_eq!(clip.status, 200, "{}", clip.body);
        assert!(clip.body["text"].as_str().unwrap().contains("video file clip.mp4") && clip.body["stale"] == false);
        let scan = op.get("/v1/doc?path=context/scan.pdf");
        assert!(scan.body["text"].as_str().unwrap().contains("by content"), "{}", scan.body);
        assert_eq!(kind_of("context/scan.pdf", "context"), "docs");
    }

    #[test]
    fn the_operator_sees_everything_with_its_provenance_and_http_cannot_widen_its_view() {
        let (home, ws) = workspace();
        let mut api = Api::new(home.path(), ws.path()).unwrap();
        let op = Reader { kb: &api.kb, ws: &api.ws, operator: true };
        let paths = |r: Reply| -> Vec<String> { r.body["results"].as_array().unwrap().iter().map(|x| x["path"].as_str().unwrap().to_string()).collect() };
        let all = op.get("/v1/search?q=zebra&rerank=0");
        let listed = paths(Reply { status: all.status, body: all.body.clone() });
        assert!(listed.iter().any(|p| p == "specs/web.md") && listed.iter().any(|p| p == "memory/agents/builder/notes.md"), "{listed:?}");
        let web = all.body["results"].as_array().unwrap().iter().find(|x| x["path"] == "specs/web.md").unwrap();
        assert_eq!((web["trust"].as_str(), web["status"].as_str()), (Some("external_untrusted"), Some("active")), "tagged, so a person can tell");
        let trusted = paths(op.get("/v1/search?q=zebra&rerank=0&trusted_only=1"));
        assert!(!trusted.iter().any(|p| p == "specs/web.md") && trusted.iter().any(|p| p == "memory/agents/builder/notes.md"), "{trusted:?}");
        assert_eq!(op.get("/v1/doc?path=specs/web.md").status, 200);
        assert_eq!(op.get("/v1/doc?path=specs/web.md&trusted_only=1").status, 404);
        assert_eq!(op.get("/v1/doc?path=memory/agents/builder/notes.md").status, 200);
        let names: Vec<String> = op.get("/v1/collections").body.as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, vec!["memory", "specs"]);
        // Over HTTP the same parameter changes nothing.
        let (_, http) = get(&mut api, "/v1/search?q=zebra&rerank=0&trusted_only=0");
        let h: Vec<&str> = http["results"].as_array().unwrap().iter().map(|x| x["path"].as_str().unwrap()).collect();
        assert!(!h.iter().any(|p| p.contains("web.md") || p.contains("memory")), "{h:?}");
        assert_eq!(get(&mut api, "/v1/doc?path=specs/web.md&trusted_only=0").0, 404);
    }

    #[test]
    fn it_is_read_only() {
        let (home, ws) = workspace();
        let mut api = Api::new(home.path(), ws.path()).unwrap();
        for m in ["POST", "PUT", "DELETE", "PATCH"] {
            assert_eq!(api.handle(m, "/v1/search?q=zebra").status, 405, "{m}");
        }
        let before = std::fs::read_to_string(ws.path().join("base/specs/alpha.md")).unwrap();
        let _ = api.handle("GET", "/v1/doc?path=specs/alpha.md");
        assert_eq!(std::fs::read_to_string(ws.path().join("base/specs/alpha.md")).unwrap(), before);
    }

    #[test]
    fn the_token_and_the_host_are_checked() {
        let t = "t".repeat(40);
        let ok = format!("Bearer {}", t);
        assert!(refuse(Some(&ok), Some("127.0.0.1:7878"), &t, true).is_none());
        assert!(refuse(Some(&ok), Some("localhost:7878"), &t, true).is_none());
        assert_eq!(refuse(None, Some("127.0.0.1:7878"), &t, true).unwrap().status, 401);
        assert_eq!(refuse(Some("Bearer wrong"), Some("127.0.0.1:7878"), &t, true).unwrap().status, 401);
        assert_eq!(refuse(Some(&ok), Some("evil.example:7878"), &t, true).unwrap().status, 421, "DNS rebinding");
        assert_eq!(refuse(Some(&ok), Some("127.0.0.1.evil.example"), &t, true).unwrap().status, 421);
        assert!(refuse(Some(&ok), Some("kb.example.org"), &t, false).is_none(), "off loopback the host is the operator's to name");
        assert_eq!(refuse(Some("Bearer "), None, "", true).unwrap().status, 401, "an empty token never matches");
        assert!(is_loopback_bind("127.0.0.1:0") && is_loopback_bind("localhost:1") && !is_loopback_bind("0.0.0.0:7878"));
    }

    #[test]
    fn percent_decoding_and_the_token_file() {
        assert_eq!(parse_query("q=council+dead%20lock&x=%E2%9C%94&bad=%zz&e=")["q"], "council dead lock");
        assert_eq!(parse_query("x=%E2%9C%94")["x"], "✔");
        assert_eq!(parse_query("bad=%zz")["bad"], "%zz");
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/kb.token");
        let a = token_at(&p).unwrap();
        assert_eq!(a.len(), 64);
        assert_eq!(token_at(&p).unwrap(), a, "kept, not remade");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    }

    fn http(addr: std::net::SocketAddr, request: &str) -> (u16, String) {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        s.write_all(request.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        let status = out.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        (status, out.split("\r\n\r\n").nth(1).unwrap_or("").to_string())
    }

    #[test]
    fn over_the_wire() {
        let (home, ws) = workspace();
        let token = "k".repeat(48);
        let run = start(home.path(), ws.path(), "127.0.0.1:0", &token).unwrap();
        let req = |method: &str, path: &str, auth: Option<&str>, host: &str| {
            let a = auth.map(|t| format!("Authorization: Bearer {}\r\n", t)).unwrap_or_default();
            format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n{a}Connection: close\r\n\r\n")
        };
        let host = run.addr.to_string();
        let (s, body) = http(run.addr, &req("GET", "/v1/search?q=zebra&rerank=0", Some(&token), &host));
        assert_eq!(s, 200, "{body}");
        assert!(serde_json::from_str::<Value>(&body).unwrap()["results"].as_array().unwrap().len() >= 2);
        assert_eq!(http(run.addr, &req("GET", "/v1/health", None, &host)).0, 401);
        assert_eq!(http(run.addr, &req("GET", "/v1/health", Some("nope"), &host)).0, 401);
        assert_eq!(http(run.addr, &req("GET", "/v1/health", Some(&token), "evil.example")).0, 421);
        assert_eq!(http(run.addr, &req("POST", "/v1/search?q=zebra", Some(&token), &host)).0, 405);
        let (s, body) = http(run.addr, &req("HEAD", "/v1/health", Some(&token), &host));
        assert!(s == 200 && body.is_empty());
        run.stop();
    }
}
