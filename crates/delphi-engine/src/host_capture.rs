//! `capture_source` (`specs/142` §2.2; slice C): a host hands over the text it read, and the library keeps it as a capture that says
//! it is the host's word.
//!
//! * **Three body forms** (§2.2 item 1). `text`: the text the host read (at most 256 KiB). `file`: a file the host saved (at most 8 MiB)
//!   inside the session's project folder or its handover folder (`<home>/handover/<session>/`); the daemon opens the folder, then each
//!   component of the path beneath it without following a link, then checks what it opened on the descriptor and reads only from it
//!   ([`read_beneath`]). Neither: the locator alone asks the daemon to read the address under its own runner's rules; one it may not
//!   read is answered `not fetched: hand over the text` and nothing is stored.
//! * **Checked first, by code** (§2.2 item 9): every item goes through `source_normalise`, and a dry run (§2.2 item 10) runs the very
//!   same path with a library that writes nothing ([`crate::library::Library::previewing`]).
//! * **The host's word, never more** (§2.2 items 3 and 8). A host capture is not validated, counts as no origin for corroboration until
//!   the daemon reads the address itself and finds the same text, and a licence the host names is recorded as `licence_claimed` and
//!   never raises what is kept or what may leave. `labels` add handling traits; nothing here removes one.
//! * **The mark** (§2.2 item 5). Under the private-read mark the daemon fetches nothing, and every source is library-private, as are
//!   the kinds `internal`, `file` and `person` in any session; such a capture can never be confirmed.
//! * **Limits** (§2.2 item 7): 20 sources a call; for one session 120 sources and `[library] handover_session_mib` an hour; for all
//!   sessions together 300 sources and `[library] handover_hour_mib` an hour; and the cache cap of §2.4, which a host capture may not
//!   meet by evicting text used in the last seven days. Past any of them the call is refused with the limit named and nothing is
//!   written.
//!
//! The fetcher is passed in, so this file opens no connection of its own.

use crate::host_research::FetchFn;
use crate::host_rpc::HostErr;
use crate::library::capture::Handed;
use crate::library::{Budgets, Library, Outcome};
use crate::source_normalise::{self as norm, Finding, Kind};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Sources one call may hand over.
pub const MAX_ITEMS: usize = 20;
/// The largest file a host may name: 8 MiB.
pub const FILE_MAX: u64 = 8 * 1024 * 1024;
/// Sources one session may hand over in an hour.
pub const SESSION_SOURCES_HOUR: usize = 120;
/// Sources all sessions together may hand over in an hour.
pub const ALL_SOURCES_HOUR: usize = 300;
/// The answer to an address the daemon may not read, given with no text.
pub const NOT_FETCHED: &str = "not fetched: hand over the text";
/// The ledger of what was handed over in the last hour, under the library.
const LEDGER: &str = "handover.json";
const MIB: u64 = 1024 * 1024;

/// What the hour's limits are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub session_sources: usize,
    pub session_bytes: u64,
    pub all_sources: usize,
    pub all_bytes: u64,
}

impl Limits {
    /// From `[library] handover_session_mib` and `handover_hour_mib`.
    pub fn of(session_mib: u64, hour_mib: u64) -> Limits {
        Limits { session_sources: SESSION_SOURCES_HOUR, session_bytes: session_mib.saturating_mul(MIB), all_sources: ALL_SOURCES_HOUR, all_bytes: hour_mib.saturating_mul(MIB) }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits::of(16, 64)
    }
}

/// Where and for whom a call is kept.
pub struct Ctx<'a> {
    pub home: &'a Path,
    /// The host session's id.
    pub session: &'a str,
    /// `host:<tool>/<model>`, or `operator`.
    pub label: &'a str,
    /// The session's registered project folder, if it works inside one: a file may be named inside it.
    pub project_root: Option<&'a Path>,
    /// The session's handover folder (`<home>/handover/<session>/`).
    pub handover: PathBuf,
    /// The session carries the private-read mark of `specs/103`.
    pub read_marked: bool,
    /// The workspace's `[privacy] identifiers`, masked in what is stored.
    pub identifiers: &'a [String],
    /// `[library] unknown_licence`, as its word.
    pub keep: &'a str,
    /// `[library] text_cache_max_mib`, in bytes.
    pub cache_max: Option<u64>,
    pub limits: Limits,
    /// Now, as RFC 3339 and as unix seconds. A dry run and the call after it stamp the same time when given the same.
    pub now: String,
    pub now_unix: i64,
    /// Ask before a capture is stored whether the limits allow it (a host); the operator's own command passes false.
    pub limited: bool,
}

impl Ctx<'_> {
    /// The session's handover folder under `home`.
    pub fn handover_of(home: &Path, session: &str) -> PathBuf {
        home.join("handover").join(session)
    }
}

fn bad<T>(m: impl Into<String>) -> Result<T, HostErr> {
    Err(HostErr::Invalid(m.into()))
}

/// The call's items and whether it is a dry run (`specs/142` §2.2 item 1): `sources` is one to [`MAX_ITEMS`] items, and one item may be
/// passed bare (an object, not a list).
pub fn parse(p: &Value) -> Result<(Vec<Value>, bool), HostErr> {
    let items = match &p["sources"] {
        Value::Array(a) if a.is_empty() => return bad("`sources` holds one to 20 items"),
        Value::Array(a) if a.len() > MAX_ITEMS => return bad(format!("refused: `sources` holds at most {MAX_ITEMS} items per call (the limit is {MAX_ITEMS}); this call has {}. Nothing was recorded", a.len())),
        Value::Array(a) => a.clone(),
        Value::Object(_) => vec![p["sources"].clone()],
        _ => return bad("`sources` is a list of one to 20 items (one item may be passed bare), each with a `locator`"),
    };
    let dry = match &p["dry_run"] {
        Value::Null => false,
        Value::Bool(b) => *b,
        _ => return bad("`dry_run` is true or false"),
    };
    Ok((items, dry))
}

/// One item, checked: its findings, and what would be kept, or why not.
#[derive(Debug, Clone)]
pub struct Checked {
    pub findings: Vec<Finding>,
    /// The capture, when the item hands over text or a file and nothing refused it.
    pub handed: Option<Handed>,
    /// The address to read, when the item is a locator alone.
    pub fetch: Option<String>,
    /// Bytes handed over (counted against the hour's limits).
    pub bytes: u64,
}

/// Check and normalise one item (`specs/142` §2.2 items 1, 5, 8 and 9). `redact` is the secret scrubber. A file is read here, from the
/// descriptor opened beneath one of the two folders.
pub fn check(ctx: &Ctx<'_>, item: &Value, redact: &dyn Fn(&str) -> String) -> Checked {
    let mut f = Vec::new();
    let mut out = Checked { findings: Vec::new(), handed: None, fetch: None, bytes: 0 };
    let Some(m) = item.as_object() else {
        out.findings.push(Finding { field: "sources[]".into(), problem: "not an object".into(), fix: "give each source as an object with a `locator`".into(), kind: Kind::Error });
        return out;
    };
    norm::unknown_fields(m, &mut f);
    let str_of = |k: &str, f: &mut Vec<Finding>| -> Option<String> {
        match m.get(k) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => {
                f.push(Finding { field: k.into(), problem: "not a string".into(), fix: "give it as text".into(), kind: Kind::Error });
                None
            }
        }
    };
    let kind = str_of("kind", &mut f).map(|k| k.trim().to_string()).unwrap_or_else(|| "web".into());
    if !crate::host_research::KINDS.contains(&kind.as_str()) {
        f.push(Finding { field: "kind".into(), problem: format!("`{}` is not a kind", kind.chars().take(20).collect::<String>()), fix: format!("use one of {}", crate::host_research::KINDS.join(", ")), kind: Kind::Error });
    }
    let raw_locator = redact(&str_of("locator", &mut f).unwrap_or_default());
    let located = norm::locator(&raw_locator, &mut f);
    let (text, file) = (str_of("text", &mut f), str_of("file", &mut f));
    if text.is_some() && file.is_some() {
        f.push(Finding { field: "file".into(), problem: "both `text` and `file`".into(), fix: "hand over one body: the text, or the file".into(), kind: Kind::Error });
    }
    let title = norm::title(&redact(&str_of("title", &mut f).unwrap_or_default()), &mut f);
    let retrieved = str_of("retrieved", &mut f).and_then(|r| norm::date("retrieved", &r, &mut f)).unwrap_or_else(|| ctx.now.clone());
    let csl = norm::csl(m.get("csl").unwrap_or(&Value::Null), &mut f);
    let licence_claimed = str_of("licence", &mut f).map(|l| l.chars().filter(|c| !c.is_control()).take(80).collect::<String>().trim().to_string()).filter(|l| !l.is_empty());
    let note = str_of("note", &mut f).map(|n| redact(&n).chars().filter(|c| !c.is_control() || *c == '\n').take(norm::NOTE_MAX).collect::<String>());
    let list = |k: &str, f: &mut Vec<Finding>| -> Vec<String> {
        match m.get(k) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) if a.len() <= 20 && a.iter().all(Value::is_string) => a.iter().filter_map(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            Some(_) => {
                f.push(Finding { field: k.into(), problem: "not a list of at most 20 strings".into(), fix: "give a list of names".into(), kind: Kind::Error });
                Vec::new()
            }
        }
    };
    let labels = list("labels", &mut f);
    for l in &labels {
        if crate::handling::Trait::parse(l).is_none() {
            f.push(Finding { field: "labels".into(), problem: format!("`{}` is not a handling trait", l.chars().take(30).collect::<String>()), fix: "use personal, internal or confidential".into(), kind: Kind::Error });
        }
    }
    let lib = Library::in_home(ctx.home);
    let topics = list("topics", &mut f);
    for t in &topics {
        if lib.topic_side(t).is_none() {
            f.push(Finding { field: "topics".into(), problem: format!("no topic `{}`", t.chars().take(60).collect::<String>()), fix: "name a topic that exists (`delphi pages list`)".into(), kind: Kind::Error });
        }
    }
    let research = match (str_of("topic_id", &mut f), str_of("research_id", &mut f)) {
        (Some(t), _) if crate::research_record::valid_topic_id(t.trim()) => Some(t.trim().to_string()),
        (Some(_), _) => {
            f.push(Finding { field: "topic_id".into(), problem: "not a research topic id".into(), fix: "give the topic id Delphi gave you".into(), kind: Kind::Error });
            None
        }
        (None, Some(r)) => Some(r.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(64).collect()),
        (None, None) => None,
    };
    let Some((locator, is_url)) = located else {
        out.findings = f;
        return out;
    };
    if kind == "web" && !is_url {
        f.push(Finding { field: "locator".into(), problem: "a web source with no address".into(), fix: "give its address, or kind internal, file or person for a source with none".into(), kind: Kind::Error });
    }
    // Private: the mark, a kind the daemon never reads, or a locator that names one of the workspace's private identifiers.
    let masked_locator = crate::host_research::mask(&locator, ctx.identifiers);
    let private = ctx.read_marked || kind != "web" || masked_locator != locator;
    let unconfirmable = private;
    let (body, from_file) = match (&text, &file) {
        (Some(t), None) => (norm::text("text", t, &mut f).map(|t| crate::host_research::mask(&redact(&t), ctx.identifiers).into_bytes()), false),
        (None, Some(p)) => {
            let roots: Vec<&Path> = [Some(ctx.handover.as_path()), ctx.project_root].into_iter().flatten().collect();
            match read_file(&roots, p, &|| {}) {
                Ok(bytes) => {
                    out.bytes = bytes.len() as u64;
                    match norm::utf8("file", &bytes, &mut f) {
                        // Secrets and identifiers are taken out of a file too; what is kept as `raw` is what the text was made from.
                        Some(s) => {
                            let cleaned = crate::host_research::mask(&redact(&s), ctx.identifiers);
                            (Some(cleaned.into_bytes()), true)
                        }
                        None => (None, true),
                    }
                }
                Err(e) => {
                    f.push(Finding { field: "file".into(), problem: e, fix: "save the file inside the project folder or the handover folder, as a regular file of at most 8 MiB, and name it by a path with no link and no `..`".into(), kind: Kind::Error });
                    (None, true)
                }
            }
        }
        _ => (None, false),
    };
    if text.is_some() {
        out.bytes = text.as_deref().map_or(0, |t| t.len() as u64);
    }
    let media_type = match &body {
        Some(b) => norm::media_type(str_of("media_type", &mut f).as_deref(), b, &mut f),
        None => None,
    };
    if !from_file && media_type.as_deref() == Some("text/html") {
        f.push(Finding { field: "text".into(), problem: "the text is HTML".into(), fix: "hand over the page's text, or save the HTML to a file and pass `file`".into(), kind: Kind::Error });
    }
    let (Some(body), Some(media_type)) = (body, media_type) else {
        // The locator alone: the daemon reads the address under its own rules, never under the mark.
        if text.is_none() && file.is_none() && !norm::refused(&f) {
            out.fetch = (is_url && !private).then(|| locator.clone());
            if out.fetch.is_none() {
                f.push(Finding { field: "locator".into(), problem: NOT_FETCHED.into(), fix: "pass `text` or `file`".into(), kind: Kind::Error });
            }
        }
        out.findings = f;
        return out;
    };
    // A private source is kept under the id `record_research` gives the same locator (`host_research::record`), so either may come first.
    let id = if private { crate::library::private_pointer_id(&kind, &crate::host_research::mask(&raw_locator, ctx.identifiers)) } else { crate::library::source_id(&locator) };
    out.handed = Some(Handed {
        id,
        url: is_url.then(|| masked_locator.clone()),
        locator: masked_locator,
        kind,
        title: crate::host_research::mask(&title, ctx.identifiers),
        retrieved,
        rights_notice: delphi_adapter::research::fetch::shows_rights_notice(&String::from_utf8_lossy(&body)),
        raw: body,
        media_type,
        from_file,
        captured_by: ctx.label.to_string(),
        private,
        unconfirmable,
        csl,
        licence_claimed,
        note,
        topics,
        research,
        keep: ctx.keep.to_string(),
        labels,
    });
    out.findings = f;
    out
}

/// Read a file the host names (`specs/142` §2.2 item 1, AC-23): `path`, absolute inside one of `roots` or relative to one of them
/// (tried in order), opened one component at a time beneath the root without following a link, then checked on the descriptor (a
/// regular file of at most [`FILE_MAX`]) and read from it only. `between` runs after the open and before the read (a test swaps the
/// file there).
pub fn read_file(roots: &[&Path], path: &str, between: &dyn Fn()) -> Result<Vec<u8>, String> {
    let p = Path::new(path.trim());
    let mut last = "the path is not inside the project folder or the handover folder".to_string();
    for root in roots {
        let rel: PathBuf = if p.is_absolute() {
            let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            match p.strip_prefix(root).or_else(|_| p.strip_prefix(&canon)) {
                Ok(r) => r.to_path_buf(),
                Err(_) => continue,
            }
        } else {
            p.to_path_buf()
        };
        match read_beneath(root, &rel, between) {
            Ok(b) => return Ok(b),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Open `rel` beneath `root` and read it (see [`read_file`]).
#[cfg(unix)]
pub fn read_beneath(root: &Path, rel: &Path, between: &dyn Fn()) -> Result<Vec<u8>, String> {
    use std::ffi::CString;
    use std::io::Read;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::FromRawFd;
    let mut parts = Vec::new();
    for c in rel.components() {
        match c {
            std::path::Component::Normal(s) => parts.push(CString::new(s.as_bytes()).map_err(|_| "a path with a NUL in it".to_string())?),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => return Err("a path with `..` in it".into()),
            _ => return Err("the path is not inside the project folder or the handover folder".into()),
        }
    }
    let Some((name, dirs)) = parts.split_last() else { return Err("no file named".into()) };
    let root_c = CString::new(root.as_os_str().as_bytes()).map_err(|_| "a folder with a NUL in its name".to_string())?;
    // SAFETY: plain syscalls on NUL-terminated names and descriptors this function owns; each descriptor is closed once, the last one
    // by the `File` that takes it.
    unsafe {
        let mut dir = libc::open(root_c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC);
        if dir < 0 {
            return Err("the folder cannot be opened".into());
        }
        for d in dirs {
            let next = libc::openat(dir, d.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
            libc::close(dir);
            if next < 0 {
                return Err("a folder on the path is a link, or is not there".into());
            }
            dir = next;
        }
        let fd = libc::openat(dir, name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
        libc::close(dir);
        if fd < 0 {
            return Err("the file is a link, or is not there".into());
        }
        let file = std::fs::File::from_raw_fd(fd);
        let mut st: libc::stat = std::mem::zeroed();
        if libc::fstat(fd, &mut st) != 0 || st.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err("not a regular file".into());
        }
        if st.st_size as u64 > FILE_MAX {
            return Err(format!("{} bytes, over the limit of {FILE_MAX} (8 MiB)", st.st_size));
        }
        between();
        let mut bytes = Vec::new();
        file.take(FILE_MAX + 1).read_to_end(&mut bytes).map_err(|e| format!("could not be read: {e}"))?;
        if bytes.len() as u64 > FILE_MAX {
            return Err(format!("over the limit of {FILE_MAX} (8 MiB)"));
        }
        Ok(bytes)
    }
}

#[cfg(not(unix))]
pub fn read_beneath(_root: &Path, _rel: &Path, _between: &dyn Fn()) -> Result<Vec<u8>, String> {
    Err("a file can be handed over on macOS and Linux only; hand over the text".into())
}

// ---- the hour's limits ----

fn ledger_path(home: &Path) -> PathBuf {
    home.join("library").join(LEDGER)
}

/// What was handed over in the hour before `now`: `(session, sources, bytes)` per call.
fn hour(home: &Path, now: i64) -> Vec<(String, usize, u64, i64)> {
    let v: Value = std::fs::read(ledger_path(home)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    v["calls"].as_array().into_iter().flatten().filter_map(|c| Some((c["session"].as_str()?.to_string(), c["sources"].as_u64()? as usize, c["bytes"].as_u64()?, c["at"].as_i64()?))).filter(|c| now - c.3 < 3600 && c.3 <= now).collect()
}

/// May `session` hand over `sources` more sources and `bytes` more bytes now? The refusal names the limit.
pub fn within(home: &Path, session: &str, sources: usize, bytes: u64, limits: &Limits, now: i64) -> Result<(), String> {
    let calls = hour(home, now);
    let (mut s_src, mut s_b, mut a_src, mut a_b) = (0usize, 0u64, 0usize, 0u64);
    for (who, n, b, _) in &calls {
        a_src += n;
        a_b += b;
        if who == session {
            s_src += n;
            s_b += b;
        }
    }
    let mib = |b: u64| b / MIB;
    if s_src + sources > limits.session_sources {
        return Err(format!("refused: at most {} sources an hour for one session (the limit is {}); this session has handed over {s_src}. Nothing was recorded; try again later", limits.session_sources, limits.session_sources));
    }
    if s_b + bytes > limits.session_bytes {
        return Err(format!("refused: at most {} MiB an hour for one session (the limit is [library] handover_session_mib = {}); this session has handed over {} bytes and this call is {bytes}. Nothing was recorded; try again later", mib(limits.session_bytes), mib(limits.session_bytes), s_b));
    }
    if a_src + sources > limits.all_sources {
        return Err(format!("refused: at most {} sources an hour for all sessions together (the limit is {}); {a_src} were handed over this hour. Nothing was recorded; try again later", limits.all_sources, limits.all_sources));
    }
    if a_b + bytes > limits.all_bytes {
        return Err(format!("refused: at most {} MiB an hour for all sessions together (the limit is [library] handover_hour_mib = {}); {a_b} bytes were handed over this hour. Nothing was recorded; try again later", mib(limits.all_bytes), mib(limits.all_bytes)));
    }
    Ok(())
}

/// Count a call that was kept.
fn count(home: &Path, session: &str, sources: usize, bytes: u64, now: i64) -> Result<(), String> {
    let mut calls: Vec<Value> = hour(home, now).into_iter().map(|(s, n, b, at)| json!({"session": s, "sources": n, "bytes": b, "at": at})).collect();
    calls.push(json!({"session": session, "sources": sources, "bytes": bytes, "at": now}));
    let path = ledger_path(home);
    std::fs::create_dir_all(path.parent().unwrap_or(home)).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&json!({"calls": calls})).unwrap_or_default()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

// ---- the call ----

/// The label of a kept source, as its marks and handling traits (`specs/141` §2.1).
fn label_of(lib: &Library, home: &Path, id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(m) = lib.read_meta(id) {
        if crate::library::is_private(&m) {
            out.push("library_private".into());
        }
        if crate::library::is_held(&m) {
            out.push("text_held".into());
        }
    }
    let traits = crate::handling::Handling::library_only(home).set(&crate::handling::source_item(id));
    out.extend(crate::handling::names(&traits).into_iter().map(String::from));
    out
}

fn outcome_word(o: &Outcome) -> &'static str {
    match o {
        Outcome::Added => "added",
        Outcome::NewVersion(_) => "new_version",
        Outcome::Unchanged => "revisit",
        Outcome::KeptRicher => "refused",
    }
}

/// Run one call (`specs/142` §2.2): check every item, refuse the whole call past a limit, then keep each item on its own (one refused
/// item does not stop the others), or, with `dry`, answer each with the record exactly as it would be written and write nothing.
/// `fetch` is the daemon's reader for an address alone and for comparing an address handed over with its text.
pub fn capture(ctx: &Ctx<'_>, items: &[Value], dry: bool, redact: &dyn Fn(&str) -> String, fetch: FetchFn<'_>) -> Result<Value, String> {
    let checked: Vec<Checked> = items.iter().map(|i| check(ctx, i, redact)).collect();
    let keeps: Vec<&Checked> = checked.iter().filter(|c| !norm::refused(&c.findings) && (c.handed.is_some() || c.fetch.is_some())).collect();
    let bytes: u64 = keeps.iter().map(|c| c.bytes).sum();
    if ctx.limited && !dry {
        within(ctx.home, ctx.session, keeps.len(), bytes, &ctx.limits, ctx.now_unix)?;
    }
    // The cache cap (§2.2 item 7, §2.4 item 3): a host capture may not push out text used in the last seven days.
    if ctx.limited {
        if let Some(max) = ctx.cache_max {
            // At most its `raw` and its text, which is no longer than the bytes it came from.
            let incoming: u64 = keeps.iter().filter_map(|c| c.handed.as_ref()).map(|h| 2 * h.raw.len() as u64).sum();
            Library::in_home(ctx.home).cache_admits_host(incoming, max, ctx.now_unix)?;
        }
    }
    let mut out = Vec::new();
    let mut kept = 0usize;
    for (index, c) in checked.iter().enumerate() {
        let findings: Vec<Value> = c.findings.iter().map(Finding::to_json).collect();
        if norm::refused(&c.findings) {
            let why = if c.findings.iter().any(|f| f.problem == NOT_FETCHED) { NOT_FETCHED.to_string() } else { "refused: see the findings".to_string() };
            out.push(json!({"index": index, "outcome": "refused", "why": why, "findings": findings}));
            continue;
        }
        let answer = match (&c.handed, &c.fetch) {
            (Some(h), _) => keep_handed(ctx, h, dry, fetch),
            (None, Some(url)) => keep_fetched(ctx, url, dry, fetch),
            (None, None) => Err("nothing to keep".into()),
        };
        match answer {
            Ok(mut v) => {
                v["index"] = json!(index);
                v["findings"] = Value::Array(findings);
                if v["outcome"] != "refused" {
                    kept += 1;
                }
                out.push(v);
            }
            Err(e) => out.push(json!({"index": index, "outcome": "refused", "why": e, "findings": findings})),
        }
    }
    if !dry && kept > 0 && ctx.limited {
        count(ctx.home, ctx.session, keeps.len(), bytes, ctx.now_unix)?;
    }
    Ok(json!({
        "dry_run": dry, "sources": out, "status": crate::host_research::NOT_VALIDATED, "handover_folder": ctx.handover.display().to_string(),
        "note": if dry { "dry run: nothing was written. Each item shows the record that the same call without dry_run writes." } else { "not validated. Text you hand over is kept as your word: it is searchable and labelled not validated, a quote found in it anchors there and stays host-reported, and it counts for no corroboration until Delphi reads the address itself and finds the same text. A licence you name is recorded, never acted on." },
    }))
}

fn library(ctx: &Ctx<'_>) -> Library {
    Library::in_home(ctx.home).with_text_cache(ctx.cache_max).stamped(&ctx.now)
}

/// Keep, or preview, one capture a host handed over; then compare it with the daemon's own read where its rules let it read the address.
fn keep_handed(ctx: &Ctx<'_>, h: &Handed, dry: bool, fetch: FetchFn<'_>) -> Result<Value, String> {
    let budgets = Budgets::default();
    if dry {
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        let lib = library(ctx).previewing(slot.clone());
        let s = lib.add_handed(h, &budgets)?;
        let record = slot.lock().map(|g| g.clone()).unwrap_or(None).unwrap_or(Value::Null);
        let real = Library::in_home(ctx.home);
        let (text, _) = h.text();
        let would: Vec<Value> = real.waiting_on(&h.id).into_iter().map(|(claim, quote)| json!({"claim": claim, "anchors": crate::host_research::quote_in(&text, &quote)})).collect();
        let rights = record["capture"]["rights"].clone();
        let mut label: Vec<String> = Vec::new();
        if record["meta"]["private"] == true {
            label.push("library_private".into());
        }
        if crate::library::is_held(&record["meta"]) {
            label.push("text_held".into());
        }
        label.extend(h.labels.iter().filter_map(|l| crate::handling::Trait::parse(l)).map(|t| t.as_str().to_string()));
        return Ok(json!({"source_id": s.id, "outcome": outcome_word(&s.outcome), "level": s.level.as_str(), "rights": rights, "label": label, "record": record, "waiting_claims": would, "notes": s.notes}));
    }
    let lib = library(ctx);
    let s = lib.add_handed(h, &budgets)?;
    // `labels` add handling traits (§2.2 item 8), as a host's kind does; nothing here removes one.
    let mut traits = crate::handling::host_kind(&h.kind);
    traits.extend(h.labels.iter().filter_map(|l| crate::handling::Trait::parse(l)));
    if !traits.is_empty() {
        crate::handling::Handling::library_only(ctx.home).add(&crate::handling::source_item(&s.id), None, &traits, &crate::handling::By::Host, &[])?;
    }
    // Quotes that waited on this source (§2.2 item 6).
    let anchored = lib.attach_waiting(&s.id);
    // Where the daemon may read the address itself it does, and the same text confirms the capture (§2.2 items 3 and 4).
    let mut compared = Value::Null;
    if let Some(url) = h.url.as_deref().filter(|_| !h.unconfirmable && !ctx.read_marked && crate::host_noted::https_host(h.url.as_deref().unwrap_or("")).is_some()) {
        compared = match fetch(url) {
            Ok((at, snap)) => match lib.add_fetched(&at, &snap, &daemon_opts(ctx), &budgets) {
                Ok(d) if d.id == s.id => json!(if lib.read_meta(&s.id).is_some_and(|m| !m["confirmed_by_daemon"].is_null()) { "confirmed" } else { "differs: the daemon's copy is the current version" }),
                Ok(_) => json!("read at another address"),
                Err(_) => json!("not compared"),
            },
            Err(_) => json!("not fetched"),
        };
    }
    let rec = lib.capture_record(&s.id).unwrap_or(Value::Null);
    let meta = lib.read_meta(&s.id).unwrap_or(Value::Null);
    Ok(json!({
        "source_id": s.id, "version": meta["version"], "outcome": outcome_word(&s.outcome), "level": s.level.as_str(),
        "raw_sha256": rec["raw_sha256"], "text_sha256": rec["text_sha256"], "rights": rec["rights"], "label": label_of(&lib, ctx.home, &s.id),
        "captured_by": rec["captured_by"], "confirmed": !meta["confirmed_by_daemon"].is_null(), "daemon_read": compared,
        "anchored_waiting_quotes": anchored, "notes": s.notes, "licence_claimed": meta["licence_claimed"],
    }))
}

/// The options the daemon keeps a page it read under: the operator's `[library] unknown_licence`.
fn daemon_opts(ctx: &Ctx<'_>) -> crate::library::AddOpts {
    crate::library::AddOpts { keep_closed_held: ctx.keep == "local_text", keep_closed_privately: ctx.keep == "private_text", ..Default::default() }
}

/// A locator alone: the daemon reads the address under its own rules, or answers [`NOT_FETCHED`] and stores nothing.
fn keep_fetched(ctx: &Ctx<'_>, url: &str, dry: bool, fetch: FetchFn<'_>) -> Result<Value, String> {
    if dry {
        return Ok(json!({"source_id": crate::library::source_id(url), "outcome": "fetch", "note": "Delphi would read this address itself if its rules let it; otherwise it answers not fetched: hand over the text"}));
    }
    let (at, snap) = match fetch(url) {
        Ok(p) => p,
        Err(_) => return Ok(json!({"source_id": crate::library::source_id(url), "outcome": "refused", "why": NOT_FETCHED})),
    };
    let lib = library(ctx);
    let s = lib.add_fetched(&at, &snap, &daemon_opts(ctx), &Budgets::default())?;
    let anchored = lib.attach_waiting(&s.id);
    let rec = lib.capture_record(&s.id).unwrap_or(Value::Null);
    let meta = lib.read_meta(&s.id).unwrap_or(Value::Null);
    Ok(json!({
        "source_id": s.id, "version": meta["version"], "outcome": outcome_word(&s.outcome), "level": s.level.as_str(),
        "raw_sha256": rec["raw_sha256"], "text_sha256": rec["text_sha256"], "rights": rec["rights"], "label": label_of(&lib, ctx.home, &s.id),
        "captured_by": rec["captured_by"], "anchored_waiting_quotes": anchored, "notes": s.notes,
    }))
}

/// The tool's input as a JSON Schema (`mcp_host::tools`).
pub fn input_schema() -> Value {
    let item = json!({"type": "object", "properties": {
        "locator": {"type": "string", "minLength": 1, "maxLength": 500, "description": "The page's address, or a description of where a source with no public address is."},
        "text": {"type": "string", "maxLength": norm::TEXT_MAX, "description": "The text you read, at most 256 KiB. Not with `file`."},
        "file": {"type": "string", "maxLength": 1024, "description": "A file you saved (text, Markdown or HTML; at most 8 MiB) inside your project folder or the session's handover folder, by an absolute path or one relative to the handover folder. Not with `text`."},
        "title": {"type": "string", "maxLength": 300},
        "retrieved": {"type": "string", "maxLength": 40, "description": "When you read it: 2026-11-25 or RFC 3339."},
        "kind": {"type": "string", "enum": crate::host_research::KINDS, "default": "web"},
        "media_type": {"type": "string", "maxLength": 80, "description": "text/plain, text/markdown or text/html; checked against the bytes."},
        "csl": {"type": "object", "description": "Citation fields (CSL): type, title, author, editor, translator, container-title, publisher, publisher-place, issued, volume, issue, page, edition, URL, DOI, ISBN, ISSN, language."},
        "licence": {"type": "string", "maxLength": 80, "description": "The licence you believe applies. Recorded as licence_claimed; it never changes what Delphi keeps or what may leave."},
        "labels": {"type": "array", "maxItems": 20, "items": {"type": "string", "enum": ["personal", "internal", "confidential"]}, "description": "Handling traits to add. They only add; none is removed."},
        "topics": {"type": "array", "maxItems": 20, "items": {"type": "string", "maxLength": 80}, "description": "Topic page slugs that exist."},
        "topic_id": {"type": "string", "maxLength": 57, "description": "A research topic id Delphi gave you."},
        "research_id": {"type": "string", "maxLength": 64},
        "note": {"type": "string", "maxLength": norm::NOTE_MAX}
    }, "required": ["locator"], "additionalProperties": false});
    json!({
        "type": "object",
        "properties": {
            "sources": {"description": "One to 20 sources (one may be passed bare).", "oneOf": [{"type": "array", "minItems": 1, "maxItems": MAX_ITEMS, "items": item}, item]},
            "dry_run": {"type": "boolean", "default": false, "description": "Show the record each item would be stored as, and the findings; write nothing."},
            "model": {"type": "string", "maxLength": 64, "description": "Your model name, self-reported."}
        },
        "required": ["sources"],
        "additionalProperties": false
    })
}

/// `delphi sources capture <file.json> [--dry-run]` (§2.2 item 10): the operator's own call, the same checks and the same path, kept
/// as `captured_by: operator`. The file holds `{"sources": [...]}` or one item. Files are read beneath the folder of the JSON file.
pub fn operator_call(home: &Path, json_file: &Path, dry: bool, keep: &str, cache_max: Option<u64>) -> Result<Value, String> {
    let bytes = std::fs::read(json_file).map_err(|e| format!("{}: {e}", json_file.display()))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: not JSON ({e})", json_file.display()))?;
    let call = if v.get("sources").is_some() { v } else { json!({"sources": v}) };
    let (items, d) = parse(&call).map_err(|e| e.to_string())?;
    let folder = json_file.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let now = delphi_core::time::now_rfc3339();
    let ctx = Ctx {
        home,
        session: "operator",
        label: "operator",
        project_root: Some(&folder),
        handover: Ctx::handover_of(home, "operator"),
        read_marked: false,
        identifiers: &[],
        keep,
        cache_max,
        limits: Limits::default(),
        now_unix: delphi_core::time::parse_rfc3339(&now).unwrap_or(0),
        now,
        limited: false,
    };
    capture(&ctx, &items, dry || d, &|s| s.to_string(), &|_| Err("the operator's command reads no address; hand over the text".into()))
}

/// `delphi sources lint [<id>]` (§2.2 item 10): the validation run over stored records, and what normalising each again would change.
pub fn lint(home: &Path, id: Option<&str>) -> Vec<(String, Vec<Finding>)> {
    let lib = Library::in_home(home);
    let all: Vec<(String, Value)> = match id {
        Some(i) => lib.read_meta(i).map(|m| vec![(i.to_string(), m)]).unwrap_or_default(),
        None => lib.list(),
    };
    all.into_iter().map(|(i, m)| (i, norm::lint_record(&m))).filter(|(_, f)| !f.is_empty()).collect()
}

/// A finding as one line, for the operator.
pub fn finding_line(f: &Finding) -> String {
    format!("{} {}: {} ({})", if f.kind == Kind::Error { "error" } else { "normalised" }, f.field, f.problem, f.fix)
}

#[cfg(test)]
#[path = "host_capture_tests.rs"]
mod tests;
