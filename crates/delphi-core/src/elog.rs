//! The central error log (`specs/039`): redacted, size-capped JSON Lines at `$DELPHI_HOME/logs/delphi.log.jsonl`,
//! written by the CLI, the daemon and the UI alike, so a testing session can be debugged with context afterwards.
//!
//! The contract, in order of importance:
//!
//! 1. Logging NEVER disturbs the operation. A write that fails (unwritable directory, full disk, a panic
//!    inside this module) drops the record, counts the drop and returns. Nothing is printed, no exit code
//!    changes, no error is returned. The next record that does get written carries `dropped: N`.
//! 2. Everything is sanitized BEFORE it is serialized: control, bidi and zero-width characters are replaced
//!    (one record is always one line), the secret redactor and a few log-specific rules (URL credentials, DIDs,
//!    long opaque blobs, e-mail addresses, the home path) run, and every field is truncated.
//! 3. Only a fixed list of context keys is kept ([`CONTEXT_KEYS`]). Bodies, prompts, file contents and
//!    environment values have no key, so a careless caller cannot put them in.
//! 4. The directory is outside every agent-readable zone (`pathguard` denies `logs/`): the file holds text
//!    echoed from tools and stores, which is a prompt-injection and leak channel.
//!
//! Levels: `off` (nothing is written), `error` (the default), `warn`, `debug`. `DELPHI_LOG` or `[logging] level`
//! in the OPERATOR-scope file `$DELPHI_HOME/operator/config.toml` sets it; the environment wins.

use crate::redact::{Redactor, MASK};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{OnceLock, RwLock};

/// The file name, inside `$DELPHI_HOME/logs/`.
pub const FILE: &str = "delphi.log.jsonl";
pub const DEFAULT_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const DEFAULT_FILES: usize = 3;
const MIN_MAX_BYTES: u64 = 1024;
const MAX_FILES: usize = 10;
/// A record longer than this is shrunk before it is written, so one `write(2)` stays whole.
const MAX_LINE: usize = 16 * 1024;
const MAX_MESSAGE: usize = 1500;
const MAX_CTX_VALUE: usize = 400;
const MAX_CHAIN: usize = 8;
const MAX_BACKTRACE: usize = 4000;
/// Redaction runs over at most this much of a string; the rest is cut first.
const PRE_CUT: usize = 16 * 1024;

pub const COMPONENTS: &[&str] = &["cli", "daemon", "engine", "harness", "provider", "ui", "xenia", "relay", "tool"];

/// The only context keys a record keeps. Anything else is dropped, whoever passes it.
pub const CONTEXT_KEYS: &[&str] = &[
    "mission", "task", "span", "command", "args_shape", "exit_code", "harness", "provider", "status", "duration_ms", "tool", "method", "kind",
    "model", "role", "attempt", "stderr_tail", "error_type", "reason", "from", "to", "retry_after_s", "bytes", "count", "store", "phase", "view",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Off = 0,
    Error = 1,
    Warn = 2,
    Debug = 3,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "0" => Some(Level::Off),
            "error" => Some(Level::Error),
            "warn" | "warning" => Some(Level::Warn),
            "debug" | "trace" => Some(Level::Debug),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Off => "off",
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Debug => "debug",
        }
    }
}

/// The effective settings and where each came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub level: Level,
    pub max_bytes: u64,
    pub files: usize,
    /// `default`, `DELPHI_LOG`, or the config path.
    pub source: String,
    pub dir: PathBuf,
}

impl Settings {
    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE)
    }
}

pub fn log_dir(home: &Path) -> PathBuf {
    home.join("logs")
}

/// Operator-scope settings: `$DELPHI_HOME/operator/config.toml` `[logging]`, then the environment.
/// Never a ratified file an agent can propose changes to (`pathguard` denies `operator/` to every agent).
pub fn settings_for(home: &Path) -> Settings {
    settings_with(home, std::env::var("DELPHI_LOG").ok().as_deref(), std::env::var("DELPHI_LOG_MAX_BYTES").ok().as_deref())
}

pub fn settings_with(home: &Path, env_level: Option<&str>, env_max: Option<&str>) -> Settings {
    let mut s = Settings { level: Level::Error, max_bytes: DEFAULT_MAX_BYTES, files: DEFAULT_FILES, source: "default".into(), dir: log_dir(home) };
    let cfg_path = home.join("operator").join("config.toml");
    if let Some(v) = fs::read_to_string(&cfg_path).ok().and_then(|t| toml::from_str::<toml::Value>(&t).ok()) {
        if let Some(l) = v.get("logging") {
            if let Some(level) = l.get("level").and_then(|x| x.as_str()).and_then(Level::parse) {
                s.level = level;
                s.source = cfg_path.display().to_string();
            }
            if let Some(mb) = l.get("max_mb").and_then(|x| x.as_integer()).filter(|n| *n > 0) {
                s.max_bytes = (mb as u64).saturating_mul(1024 * 1024).max(MIN_MAX_BYTES);
            }
            if let Some(n) = l.get("files").and_then(|x| x.as_integer()).filter(|n| *n > 0) {
                s.files = (n as usize).min(MAX_FILES);
            }
        }
    }
    if let Some(level) = env_level.and_then(Level::parse) {
        s.level = level;
        s.source = "DELPHI_LOG".into();
    }
    if let Some(n) = env_max.and_then(|x| x.trim().parse::<u64>().ok()) {
        s.max_bytes = n.max(MIN_MAX_BYTES);
    }
    s
}

// ── sanitizing ────────────────────────────────────────────────────────────────────────────────────

struct Rules {
    redactor: RwLock<Redactor>,
    url_cred: Regex,
    url_query: Regex,
    bearer: Regex,
    did: Regex,
    jwt: Regex,
    blob: Regex,
    hex: Regex,
    email: Regex,
}

fn rules() -> &'static Rules {
    static R: OnceLock<Rules> = OnceLock::new();
    R.get_or_init(|| Rules {
        redactor: RwLock::new(Redactor::new()),
        url_cred: Regex::new(r"([A-Za-z][A-Za-z0-9+.\-]*://)[^/\s@]+@").unwrap(),
        url_query: Regex::new(r"(?i)([?&](?:token|access_token|key|api_key|apikey|password|passwd|secret|sig|signature|code)=)[^&\s]+").unwrap(),
        bearer: Regex::new(r"(?i)\b(bearer|basic|token)\s+[A-Za-z0-9._~+/=\-]{8,}").unwrap(),
        did: Regex::new(r"did:[a-z0-9]+:[A-Za-z0-9._%:\-]+").unwrap(),
        jwt: Regex::new(r"eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.?[A-Za-z0-9_\-]*").unwrap(),
        blob: Regex::new(r"[A-Za-z0-9+_\-]{40,}={0,2}").unwrap(),
        hex: Regex::new(r"\b[0-9a-fA-F]{32,}\b").unwrap(),
        email: Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").unwrap(),
    })
}

/// Teach the log a runtime-resolved credential, in every form the redactor knows (the daemon calls this where
/// the engine registers the same literal).
pub fn register_secret(literal: &str) {
    if let Ok(mut r) = rules().redactor.write() {
        r.register_literal(literal);
    }
}

fn is_bad_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2028}' | '\u{2029}' | '\u{2060}'..='\u{2069}' | '\u{061C}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Control, bidi and zero-width characters out (newlines become the two characters `\n`); at most `max` characters out.
pub fn clean_chars(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len().min(max + 8));
    for (n, c) in s.chars().enumerate() {
        if n >= max {
            break;
        }
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push(' '),
            c if is_bad_char(c) => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

fn truncate(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('…');
    t
}

/// The full pipeline for one string: clean, redact, shorten. `home` is replaced by `~`.
pub fn sanitize(input: &str, max: usize, home: Option<&Path>) -> String {
    let r = rules();
    // Controls first, so a secret split by an escape sequence is rejoined for the redactor.
    let mut s = clean_chars(input, PRE_CUT);
    if let Ok(red) = r.redactor.read() {
        s = red.redact_str(&s).0;
    }
    s = r.url_cred.replace_all(&s, format!("${{1}}{}@", MASK).as_str()).into_owned();
    s = r.url_query.replace_all(&s, format!("${{1}}{}", MASK).as_str()).into_owned();
    s = r.bearer.replace_all(&s, format!("${{1}} {}", MASK).as_str()).into_owned();
    s = r.jwt.replace_all(&s, MASK).into_owned();
    s = r.did.replace_all(&s, "[DID]").into_owned();
    s = r.email.replace_all(&s, "[EMAIL]").into_owned();
    s = r.hex.replace_all(&s, "[HEX]").into_owned();
    s = r.blob.replace_all(&s, |c: &regex::Captures| {
        let m = &c[0];
        if m.chars().any(|x| x.is_ascii_digit()) && m.chars().any(|x| x.is_ascii_alphabetic()) { "[BLOB]".to_string() } else { m.to_string() }
    }).into_owned();
    if let Some(h) = home {
        let hs = h.to_string_lossy();
        if hs.len() > 1 {
            s = s.replace(hs.as_ref(), "$DELPHI_HOME");
        }
    }
    if let Ok(u) = std::env::var("HOME") {
        if u.len() > 1 {
            s = s.replace(&u, "~");
        }
    }
    truncate(s, max)
}

/// Sanitize every string in `v` in place (keys too), for anything that leaves the machine: a debug bundle re-runs the
/// log's own rules over what it reads back, so a file edited by hand or written by an older build is still safe to share.
pub fn sanitize_value(v: &mut Value, home: Option<&Path>) {
    match v {
        Value::String(s) => *s = sanitize(s, MAX_BACKTRACE, home),
        Value::Array(a) => a.iter_mut().for_each(|x| sanitize_value(x, home)),
        Value::Object(o) => {
            let old = std::mem::take(o);
            for (k, mut x) in old {
                sanitize_value(&mut x, home);
                o.insert(sanitize(&k, 80, home), x);
            }
        }
        _ => {}
    }
}

// ── the logger ────────────────────────────────────────────────────────────────────────────────────

pub struct Logger {
    pub settings: Settings,
    home: PathBuf,
    version: String,
    default_component: String,
    drops: AtomicU64,
}

impl Logger {
    pub fn new(home: &Path, component: &str, version: &str, settings: Settings) -> Logger {
        Logger { settings, home: home.to_path_buf(), version: version.to_string(), default_component: component.to_string(), drops: AtomicU64::new(0) }
    }

    pub fn enabled(&self, level: Level) -> bool {
        level != Level::Off && level <= self.settings.level
    }

    /// Records dropped in this process that no later record has reported yet.
    pub fn pending_drops(&self) -> u64 {
        self.drops.load(Ordering::Relaxed)
    }

    /// Build the one-line record (without the newline). Pure: the tests use it for the canary.
    #[allow(clippy::too_many_arguments)]
    pub fn render(&self, level: Level, component: &str, slug: &str, message: &str, ctx: &Value, chain: &[String], backtrace: Option<&str>) -> String {
        let home = Some(self.home.as_path());
        let component = if COMPONENTS.contains(&component) { component } else { self.default_component.as_str() };
        let mut rec = Map::new();
        rec.insert("ts".into(), json!(crate::time::now_rfc3339()));
        rec.insert("level".into(), json!(level.as_str()));
        rec.insert("component".into(), json!(component));
        rec.insert("pid".into(), json!(std::process::id()));
        rec.insert("version".into(), json!(self.version));
        rec.insert("os".into(), json!(format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)));
        rec.insert("slug".into(), json!(sanitize(slug, 80, home)));
        rec.insert("message".into(), json!(sanitize(message, MAX_MESSAGE, home)));
        let mut c = Map::new();
        if let Some(o) = ctx.as_object() {
            for (k, v) in o {
                if !CONTEXT_KEYS.contains(&k.as_str()) {
                    continue;
                }
                let v = match v {
                    Value::String(s) => json!(sanitize(s, MAX_CTX_VALUE, home)),
                    Value::Number(_) | Value::Bool(_) => v.clone(),
                    Value::Null => continue,
                    other => json!(sanitize(&other.to_string(), MAX_CTX_VALUE, home)),
                };
                c.insert(k.clone(), v);
            }
        }
        if !c.is_empty() {
            rec.insert("context".into(), Value::Object(c));
        }
        if self.settings.level == Level::Debug && !chain.is_empty() {
            rec.insert("chain".into(), json!(chain.iter().take(MAX_CHAIN).map(|s| sanitize(s, MAX_CTX_VALUE, home)).collect::<Vec<_>>()));
        }
        if self.settings.level == Level::Debug {
            if let Some(b) = backtrace {
                rec.insert("backtrace".into(), json!(sanitize(b, MAX_BACKTRACE, home)));
            }
        }
        let mut line = Value::Object(rec.clone()).to_string();
        if line.len() > MAX_LINE {
            rec.remove("backtrace");
            rec.remove("chain");
            line = Value::Object(rec.clone()).to_string();
        }
        if line.len() > MAX_LINE {
            rec.remove("context");
            rec.insert("message".into(), json!(truncate(sanitize(message, 300, home), 300)));
            line = Value::Object(rec).to_string();
        }
        line
    }

    /// Write one record if the level allows. Never fails, never panics, never prints.
    #[allow(clippy::too_many_arguments)]
    pub fn log(&self, level: Level, component: &str, slug: &str, message: &str, ctx: &Value, chain: &[String], backtrace: Option<&str>) {
        if !self.enabled(level) {
            return;
        }
        let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut line = self.render(level, component, slug, message, ctx, chain, backtrace);
            let dropped = self.drops.swap(0, Ordering::Relaxed);
            if dropped > 0 {
                // Splice `dropped` in before the closing brace; the line is valid JSON by construction.
                line.pop();
                line.push_str(&format!(",\"dropped\":{}}}", dropped));
            }
            line.push('\n');
            let ok = self.append(line.as_bytes()).is_ok();
            if !ok && dropped > 0 {
                self.drops.fetch_add(dropped, Ordering::Relaxed);
            }
            ok
        }));
        if !matches!(done, Ok(true)) {
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn append(&self, line: &[u8]) -> std::io::Result<()> {
        let dir = &self.settings.dir;
        if !dir.is_dir() {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        let path = self.settings.path();
        self.rotate_if_needed(&path);
        // O_APPEND and one write call: concurrent writers from other processes cannot interleave inside a line.
        let mut f = OpenOptions::new().create(true).append(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&path)?;
        f.write_all(line)
    }

    fn rotate_if_needed(&self, path: &Path) {
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if size < self.settings.max_bytes {
            return;
        }
        // Only one process rotates; the others append to whatever file exists and move on.
        let lock = match OpenOptions::new().create(true).append(true).mode(0o600).open(path.with_extension("jsonl.lock")) {
            Ok(f) => f,
            Err(_) => return,
        };
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return;
        }
        if fs::metadata(path).map(|m| m.len()).unwrap_or(0) < self.settings.max_bytes {
            return;
        }
        let rotated = self.settings.files.saturating_sub(1);
        let name = |n: usize| PathBuf::from(format!("{}.{}", path.display(), n));
        if rotated == 0 {
            let _ = fs::remove_file(path);
            return;
        }
        let _ = fs::remove_file(name(rotated));
        for n in (1..rotated).rev() {
            let _ = fs::rename(name(n), name(n + 1));
        }
        let _ = fs::rename(path, name(1));
    }
}

// ── the process-wide logger ───────────────────────────────────────────────────────────────────────

static GLOBAL: RwLock<Option<Logger>> = RwLock::new(None);
static LEVEL: AtomicU8 = AtomicU8::new(0);

/// Start logging in this process. Call once, early, with the operator's home. `component` is the default for
/// records whose caller names none. Reads the level from `DELPHI_LOG` / the operator config.
pub fn init(home: &Path, component: &str, version: &str) {
    install(Logger::new(home, component, version, settings_for(home)));
}

pub fn install(l: Logger) {
    LEVEL.store(l.settings.level as u8, Ordering::Relaxed);
    if let Ok(mut g) = GLOBAL.write() {
        *g = Some(l);
    }
}

/// Stop logging in this process (tests).
pub fn uninstall() {
    LEVEL.store(0, Ordering::Relaxed);
    if let Ok(mut g) = GLOBAL.write() {
        *g = None;
    }
}

/// Cheap: one atomic load. Guard any expensive formatting with it.
pub fn enabled(level: Level) -> bool {
    level != Level::Off && (level as u8) <= LEVEL.load(Ordering::Relaxed)
}

/// Drops not yet reported by a written record, in this process.
pub fn pending_drops() -> u64 {
    GLOBAL.read().ok().and_then(|g| g.as_ref().map(|l| l.pending_drops())).unwrap_or(0)
}

fn global(level: Level, component: &str, slug: &str, message: &str, ctx: Value, chain: &[String], backtrace: Option<&str>) {
    if !enabled(level) {
        return;
    }
    if let Ok(g) = GLOBAL.read() {
        if let Some(l) = g.as_ref() {
            l.log(level, component, slug, message, &ctx, chain, backtrace);
        }
    }
}

/// An error that matters. `ctx` is an object; keys outside [`CONTEXT_KEYS`] are dropped.
pub fn error(component: &str, slug: &str, message: &str, ctx: Value) {
    global(Level::Error, component, slug, message, ctx, &[], None);
}

/// An error with its `source()` chain; the chain is kept at debug level only.
pub fn error_chain(component: &str, slug: &str, err: &(dyn std::error::Error + 'static), ctx: Value) {
    if !enabled(Level::Error) {
        return;
    }
    let mut chain = Vec::new();
    let mut cur = err.source();
    while let Some(e) = cur {
        chain.push(e.to_string());
        cur = e.source();
    }
    global(Level::Error, component, slug, &err.to_string(), ctx, &chain, None);
}

pub fn warn(component: &str, slug: &str, message: &str, ctx: Value) {
    global(Level::Warn, component, slug, message, ctx, &[], None);
}

pub fn debug(component: &str, slug: &str, message: &str, ctx: Value) {
    global(Level::Debug, component, slug, message, ctx, &[], None);
}

/// Debug record built only when debug is on: the closure never runs otherwise.
pub fn debug_with(component: &str, slug: &str, f: impl FnOnce() -> (String, Value)) {
    if enabled(Level::Debug) {
        let (m, c) = f();
        global(Level::Debug, component, slug, &m, c, &[], None);
    }
}

/// A panic hook for every binary: logs the message and location (and a backtrace at debug level), then runs the
/// hook that was there before, so the terminal still sees what it saw.
pub fn install_panic_hook(component: &'static str) {
    thread_local! {
        static IN_HOOK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !IN_HOOK.with(|c| c.replace(true)) {
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "panic".to_string()
            };
            let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
            let bt = if enabled(Level::Debug) { Some(std::backtrace::Backtrace::force_capture().to_string()) } else { None };
            let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
            global(Level::Error, component, "panic", &format!("{} (at {}, thread {})", msg, loc, thread), json!({}), &[], bt.as_deref());
            IN_HOOK.with(|c| c.set(false));
        }
        prev(info);
    }));
}

// ── reading (for `delphi logs`, `delphi doctor`, `delphi debug bundle`) ───────────────────────────

/// What `delphi doctor` shows.
pub struct Status {
    pub settings: Settings,
    pub path: PathBuf,
    pub exists: bool,
    pub bytes: u64,
    pub rotated_files: usize,
    /// Sum of `dropped` over the records on disk: records some process lost before a later one got through.
    pub dropped_reported: u64,
    pub writable: bool,
}

pub fn status(home: &Path) -> Status {
    let settings = settings_for(home);
    let path = settings.path();
    let files = existing_files(&path);
    let bytes = files.iter().filter_map(|f| fs::metadata(f).ok()).map(|m| m.len()).sum();
    let dropped_reported = read_all(&path).iter().filter_map(|r| r["dropped"].as_u64()).sum();
    let writable = if settings.dir.is_dir() {
        // A directory we can write into; the file's own mode is checked by the write itself.
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&settings.dir).map(|m| m.permissions().mode() & 0o200 != 0).unwrap_or(false)
    } else {
        true
    };
    Status { exists: path.exists(), rotated_files: files.len().saturating_sub(1), settings, path, bytes, dropped_reported, writable }
}

/// The log files that exist, oldest first.
pub fn existing_files(path: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for n in (1..=MAX_FILES).rev() {
        let p = PathBuf::from(format!("{}.{}", path.display(), n));
        if p.is_file() {
            v.push(p);
        }
    }
    if path.is_file() {
        v.push(path.to_path_buf());
    }
    v
}

/// Every parseable record, oldest first. Lines that are not JSON objects are skipped.
pub fn read_all(path: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    for f in existing_files(path) {
        if let Ok(t) = fs::read_to_string(&f) {
            out.extend(t.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v.is_object()));
        }
    }
    out
}

#[derive(Default, Clone)]
pub struct Filter {
    /// Keep records at or after this Unix time.
    pub since: Option<i64>,
    /// Keep records at this level or more severe.
    pub level: Option<Level>,
    pub component: Option<String>,
    pub mission: Option<String>,
}

impl Filter {
    pub fn matches(&self, r: &Value) -> bool {
        if let Some(s) = self.since {
            if r["ts"].as_str().and_then(crate::time::parse_rfc3339).is_none_or(|t| t < s) {
                return false;
            }
        }
        if let Some(l) = self.level {
            if r["level"].as_str().and_then(Level::parse).is_none_or(|x| x > l) {
                return false;
            }
        }
        if let Some(c) = &self.component {
            if r["component"].as_str() != Some(c.as_str()) {
                return false;
            }
        }
        if let Some(m) = &self.mission {
            if r["context"]["mission"].as_str() != Some(m.as_str()) {
                return false;
            }
        }
        true
    }
}

/// One record as a human line. Re-sanitizes the text: the file may have been edited, and the terminal is not a safe place for raw escapes.
pub fn human(r: &Value) -> String {
    let g = |k: &str| clean_chars(r[k].as_str().unwrap_or(""), 2000);
    let mut ctx = String::new();
    if let Some(o) = r["context"].as_object() {
        for (k, v) in o {
            let s = match v {
                Value::String(s) => clean_chars(s, MAX_CTX_VALUE),
                other => other.to_string(),
            };
            ctx.push_str(&format!(" {}={}", clean_chars(k, 40), s));
        }
    }
    format!("{} {:<5} {:<8} {} {}{}", g("ts"), g("level").to_uppercase(), g("component"), g("slug"), g("message"), ctx)
}

#[cfg(test)]
#[path = "elog_tests.rs"]
mod tests;
