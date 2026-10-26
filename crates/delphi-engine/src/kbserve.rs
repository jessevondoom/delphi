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
/// How long the index may go without being brought up to date (unchanged files cost a stat).
const REFRESH_EVERY: Duration = Duration::from_secs(10);
/// A document larger than this is not served whole (the indexer's own cap is 512 KiB).
const MAX_DOC_BYTES: u64 = 512 * 1024;

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
            "/v1/documents" => self.documents(&q),
            "/v1/search" => self.search(&q),
            "/v1/doc" => self.doc(&q),
            _ => return fail(404, "no such endpoint: /v1/health /v1/collections /v1/documents /v1/search /v1/doc"),
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

    fn documents(&self, q: &HashMap<String, String>) -> Result<Reply, String> {
        let limit = num(q, "limit", 20).clamp(1, 200);
        let offset = num(q, "offset", 0).min(100_000);
        let docs = self.kb.documents(q.get("collection").map(String::as_str), limit, offset, &self.view(q)).map_err(|e| e.to_string())?;
        Ok(reply(200, json!(docs)))
    }

    fn search(&self, q: &HashMap<String, String>) -> Result<Reply, String> {
        let query = q.get("q").map(|s| s.trim()).unwrap_or("");
        if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
            return Ok(fail(400, "`q` must be 1 to 500 characters"));
        }
        let limit = num(q, "limit", 10).clamp(1, MAX_LIMIT);
        let collection = q.get("collection").filter(|c| !c.is_empty());
        let mut opts = self.view(q);
        // A collection filter is applied after ranking, so ask for the most there is.
        opts.limit = if collection.is_some() { MAX_LIMIT } else { limit };
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
            out.push(json!({"pointer": h.pointer, "path": h.rel_path, "collection": d.zone, "trust": d.source_type, "status": d.status, "heading": h.heading_path, "preview": h.preview, "start_line": h.start_line, "end_line": h.end_line, "blake3": h.blake3, "score": h.score}));
            if out.len() >= limit {
                break;
            }
        }
        Ok(reply(200, json!({"query": query, "took_ms": started.elapsed().as_millis() as u64, "mode": if self.kb.embedder().is_some() { "hybrid" } else { "lexical" }, "results": out})))
    }

    fn doc(&self, q: &HashMap<String, String>) -> Result<Reply, String> {
        let Some(path) = q.get("path").filter(|p| !p.is_empty() && !p.contains('\0')) else { return Ok(fail(400, "`path` is required")) };
        let Some(info) = self.kb.document(path, &self.view(q)).map_err(|e| e.to_string())? else { return Ok(fail(404, "not in the knowledge base")) };
        // The file comes from the indexer's own list, so the logical path cannot name anything else.
        let Some(src) = kb::sources(self.ws).into_iter().find(|s| s.rel_path == info.rel_path) else { return Ok(fail(404, "no longer on disk")) };
        if std::fs::metadata(&src.path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_DOC_BYTES {
            return Ok(fail(413, "document too large to serve"));
        }
        let text = std::fs::read_to_string(&src.path).map_err(|e| e.to_string())?;
        let live = blake3::hash(text.as_bytes()).to_hex().to_string();
        let links = extract_links(&info.rel_path, &text, &|p| self.kb.document(p, &self.view(q)).ok().flatten().is_some());
        Ok(reply(200, json!({"path": info.rel_path, "collection": info.zone, "trust": info.source_type, "status": info.status, "blake3": live, "indexed_blake3": info.blake3, "stale": live != info.blake3, "text": text, "links": links})))
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

const USAGE: &str = "usage: delphi kb serve [--bind <host:port>] [--token-file <path>] [--allow-remote]\n  a read-only JSON API over the knowledge base: /v1/search?q=  /v1/doc?path=  /v1/collections  /v1/documents  /v1/health\n  every request needs `Authorization: Bearer <token>`; the token is made on first use and kept in the token file";

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
        assert_eq!(by("web")["kind"], "external");
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
