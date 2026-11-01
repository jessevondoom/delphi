//! `delphi logs` and `delphi debug bundle` (`specs/039`): read the central error log, and pack a redacted slice of it
//! for sharing. Both are read-only and offline; both take `--home`; neither calls a model or starts a daemon.

use crate::home;
use delphi_core::elog::{self, Filter, Level};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_LINES: usize = 50;
/// The most events of a mission the bundle carries.
const BUNDLE_EVENTS: usize = 200;
/// The most log records the bundle carries.
const BUNDLE_RECORDS: usize = 5000;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The filter the flags describe.
fn filter_from(args: &[String]) -> Result<Filter, String> {
    let mut f = Filter::default();
    if let Some(s) = flag(args, "--since") {
        f.since = Some(now_unix() - crate::run::parse_duration(&s)?.as_secs() as i64);
    }
    if let Some(l) = flag(args, "--level") {
        f.level = Some(Level::parse(&l).filter(|l| *l != Level::Off).ok_or_else(|| format!("`{}` is not a level (error, warn or debug)", l))?);
    }
    if let Some(c) = flag(args, "--component") {
        if !elog::COMPONENTS.contains(&c.as_str()) {
            return Err(format!("`{}` is not a component ({})", c, elog::COMPONENTS.join(", ")));
        }
        f.component = Some(c);
    }
    f.mission = flag(args, "--mission");
    Ok(f)
}

/// `delphi logs`.
pub fn logs(args: &[String]) -> i32 {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let path = elog::log_dir(&home).join(elog::FILE);
    if args.iter().any(|a| a == "--path") {
        println!("{}", path.display());
        return 0;
    }
    let filter = match filter_from(args) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("delphi logs: {}", e);
            return 2;
        }
    };
    let n = match flag(args, "--lines").or_else(|| flag(args, "-n")) {
        Some(v) => match v.parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                eprintln!("delphi logs: --lines takes a positive number");
                return 2;
            }
        },
        None => DEFAULT_LINES,
    };
    let json_out = args.iter().any(|a| a == "--json");
    let mut recs: Vec<Value> = elog::read_all(&path).into_iter().filter(|r| filter.matches(r)).collect();
    if recs.len() > n {
        recs.drain(..recs.len() - n);
    }
    if recs.is_empty() && !path.exists() {
        eprintln!("delphi logs: nothing logged yet ({}); errors are recorded here as they happen, `delphi doctor` shows the level", path.display());
    }
    let mut out = std::io::stdout().lock();
    for r in &recs {
        let _ = writeln!(out, "{}", render(r, json_out));
    }
    let _ = out.flush();
    drop(out);
    if args.iter().any(|a| a == "--follow" || a == "-f") {
        let stop_after = flag(args, "--follow-for").and_then(|s| s.parse::<u64>().ok()).map(Duration::from_secs);
        let started = std::time::Instant::now();
        follow(&path, &filter, json_out, &|| stop_after.is_some_and(|d| started.elapsed() >= d));
    }
    0
}

fn render(r: &Value, json_out: bool) -> String {
    if json_out {
        // Re-sanitized: the file may have been edited, and a terminal is not a safe place for raw escapes.
        let mut v = r.clone();
        elog::sanitize_value(&mut v, None);
        v.to_string()
    } else {
        elog::human(r)
    }
}

/// Print records appended to `path` from now on, until `stop` says so. Survives rotation (the file shrinks or is replaced).
pub fn follow(path: &Path, filter: &Filter, json_out: bool, stop: &dyn Fn() -> bool) {
    use std::io::{Read, Seek, SeekFrom};
    use std::os::unix::fs::MetadataExt;
    let mut pos = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut ino = std::fs::metadata(path).map(|m| m.ino()).unwrap_or(0);
    let mut partial = String::new();
    while !stop() {
        std::thread::sleep(Duration::from_millis(300));
        let Ok(meta) = std::fs::metadata(path) else { continue };
        if meta.ino() != ino || meta.len() < pos {
            ino = meta.ino();
            pos = 0;
            partial.clear();
        }
        if meta.len() == pos {
            continue;
        }
        let Ok(mut f) = std::fs::File::open(path) else { continue };
        if f.seek(SeekFrom::Start(pos)).is_err() {
            continue;
        }
        let mut buf = Vec::new();
        let _ = f.take(1 << 20).read_to_end(&mut buf);
        pos += buf.len() as u64;
        partial.push_str(&String::from_utf8_lossy(&buf));
        while let Some(i) = partial.find('\n') {
            let line: String = partial.drain(..=i).collect();
            if let Ok(v) = serde_json::from_str::<Value>(line.trim_end()) {
                if v.is_object() && filter.matches(&v) {
                    println!("{}", render(&v, json_out));
                }
            }
        }
        let _ = std::io::stdout().flush();
    }
}

// ── delphi debug bundle ───────────────────────────────────────────────────────────────────────────

/// What goes in the archive: (name inside it, bytes).
pub struct Bundle {
    pub files: Vec<(String, Vec<u8>)>,
    pub notes: Vec<String>,
}

fn sanitize_text(s: &str, home: &Path) -> String {
    // Line by line, so the cap applies to each line and the shape of the text survives.
    s.lines().map(|l| elog::sanitize(l, 1500, Some(home))).collect::<Vec<_>>().join("\n") + "\n"
}

/// The config with every value replaced by its type, except booleans and numbers: key paths tell the story, values can hold secrets.
pub fn redact_config(text: &str) -> String {
    fn walk(prefix: &str, v: &toml::Value, out: &mut Vec<String>) {
        match v {
            toml::Value::Table(t) => {
                for (k, x) in t {
                    walk(&if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) }, x, out);
                }
            }
            toml::Value::Array(a) => {
                out.push(format!("{} = [{} item(s)]", prefix, a.len()));
                for (i, x) in a.iter().enumerate().take(50) {
                    if matches!(x, toml::Value::Table(_)) {
                        walk(&format!("{}[{}]", prefix, i), x, out);
                    }
                }
            }
            toml::Value::String(s) => out.push(format!("{} = \"[redacted: {} chars]\"", prefix, s.chars().count())),
            toml::Value::Boolean(b) => out.push(format!("{} = {}", prefix, b)),
            toml::Value::Integer(n) => out.push(format!("{} = {}", prefix, n)),
            toml::Value::Float(n) => out.push(format!("{} = {}", prefix, n)),
            toml::Value::Datetime(_) => out.push(format!("{} = <datetime>", prefix)),
        }
    }
    match toml::from_str::<toml::Value>(text) {
        Ok(v) => {
            let mut out = Vec::new();
            walk("", &v, &mut out);
            out.join("\n") + "\n"
        }
        Err(_) => "(not valid TOML; omitted)\n".into(),
    }
}

/// Payload keys a bundle keeps from a mission event; everything else (prompts, outputs, blocks, file contents) is dropped.
const EVENT_KEYS: &[&str] = &["error", "reason", "status", "exit_code", "name", "tool", "is_error", "kind", "stop_reason", "code", "count", "outcome", "from", "to", "endpoint_id", "model_id", "role", "target", "task_id"];

/// The last `n` events of a mission as one-line summaries: the envelope's fields and an allow-listed slice of the payload.
pub fn event_summaries(mission_dir: &Path, n: usize, home: &Path) -> Result<String, String> {
    let events = delphi_core::log::replay(mission_dir).map_err(|e| format!("could not read the mission log: {}", e))?;
    let from = events.len().saturating_sub(n);
    let mut out = String::new();
    for e in &events[from..] {
        let mut payload = serde_json::Map::new();
        if let Some(o) = e.payload.as_object() {
            for k in EVENT_KEYS {
                if let Some(v @ (Value::String(_) | Value::Number(_) | Value::Bool(_))) = o.get(*k) {
                    payload.insert((*k).into(), v.clone());
                }
            }
        }
        let mut v = json!({"seq": e.seq, "ts": e.timestamp, "type": e.kind, "task": e.task_id, "span": e.span_id, "agent": e.agent, "endpoint": e.endpoint_id, "model": e.model_id, "attempt": e.attempt, "payload": payload});
        elog::sanitize_value(&mut v, Some(home));
        out.push_str(&v.to_string());
        out.push('\n');
    }
    Ok(out)
}

/// Collect everything a bundle holds. `ws` is the workspace for the doctor report and the mission; absent, those parts are skipped and say so.
pub fn collect(home: &Path, ws: Option<&home::Workspace>, since: Option<i64>, mission: Option<&str>) -> Bundle {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut notes = Vec::new();
    let st = elog::status(home);
    let version = format!("delphi {}\nos {}-{}\n", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    files.push(("version.txt".into(), version.into_bytes()));

    let filter = Filter { since, ..Default::default() };
    let mut recs: Vec<Value> = elog::read_all(&st.path).into_iter().filter(|r| filter.matches(r)).collect();
    if recs.len() > BUNDLE_RECORDS {
        recs.drain(..recs.len() - BUNDLE_RECORDS);
    }
    let mut text = String::new();
    for mut r in recs.clone() {
        elog::sanitize_value(&mut r, Some(home));
        text.push_str(&r.to_string());
        text.push('\n');
    }
    notes.push(format!("delphi.log.jsonl: {} record(s){}", recs.len(), since.map(|_| " in the window").unwrap_or("")));
    files.push(("delphi.log.jsonl".into(), text.into_bytes()));

    let settings = format!(
        "logging level: {} (from {})\nlog size cap: {} bytes x {} file(s)\nlog files: {} bytes on disk\nrecords reported dropped: {}\n",
        st.settings.level.as_str(),
        sanitize_text(&st.settings.source, home).trim_end(),
        st.settings.max_bytes,
        st.settings.files,
        st.bytes,
        st.dropped_reported
    );
    let mut config = settings;
    for (label, path) in [("operator/config.toml", home.join("operator/config.toml")), ("workspace config.toml", ws.map(|w| w.dir.join("config.toml")).unwrap_or_default())] {
        if path.as_os_str().is_empty() {
            continue;
        }
        if let Ok(t) = std::fs::read_to_string(&path) {
            config.push_str(&format!("\n# {} (keys and types; strings are not included)\n{}", label, redact_config(&t)));
        }
    }
    notes.push("config.txt: effective logging settings and the key paths of the config files (values redacted)".into());
    files.push(("config.txt".into(), sanitize_text(&config, home).into_bytes()));

    match ws {
        Some(ws) => {
            let report = crate::doctor::run(home, ws, &crate::doctor::Options::default());
            let mut d = String::new();
            for f in &report.findings {
                d.push_str(&format!("[{}] {}: {}: {}\n", f.level, f.check, f.location, f.message));
            }
            d.push_str(&format!("delphi doctor: workspace `{}`: {}\n", ws.name, report.worst()));
            notes.push(format!("doctor.txt: the `delphi doctor` report for workspace `{}`", ws.name));
            files.push(("doctor.txt".into(), sanitize_text(&d, home).into_bytes()));
            if let Some(id) = mission {
                if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
                    notes.push(format!("events.jsonl: skipped, `{}` is not a mission id", id));
                } else {
                    let dir = delphi_core::sessions::dir(&ws.dir, id);
                    match event_summaries(&dir, BUNDLE_EVENTS, home) {
                        Ok(t) => {
                            notes.push(format!("events.jsonl: the last {} events of mission {} (types, ids and short status fields only; no prompts, outputs or file contents)", t.lines().count(), id));
                            files.push(("events.jsonl".into(), t.into_bytes()));
                        }
                        Err(e) => notes.push(format!("events.jsonl: skipped ({})", e)),
                    }
                }
            }
        }
        None => notes.push("doctor.txt and events.jsonl: skipped (no workspace; pass --workspace <name>)".into()),
    }
    Bundle { files, notes }
}

/// Write the archive: a gzip-compressed tar, created new (never over a file), mode 0600.
pub fn write_archive(path: &Path, b: &Bundle) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut manifest = String::from("delphi debug bundle\nEverything below was redacted: secrets, URL credentials, DIDs, e-mail addresses, long opaque strings, control characters and your home path.\nNot included: prompts, model output, file contents, environment values, keys.\n\n");
    for n in &b.notes {
        manifest.push_str(&format!("- {}\n", n));
    }
    let f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map_err(|e| format!("cannot create {}: {}", path.display(), e))?;
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
        let mut h = tar::Header::new_gnu();
        h.set_size(bytes.len() as u64);
        h.set_mode(0o600);
        h.set_mtime(now_unix().max(0) as u64);
        h.set_cksum();
        tar.append_data(&mut h, format!("delphi-debug/{}", name), bytes).map_err(|e| e.to_string())
    };
    add("MANIFEST.txt", manifest.as_bytes())?;
    for (n, bytes) in &b.files {
        add(n, bytes)?;
    }
    tar.into_inner().and_then(|gz| gz.finish()).map_err(|e| e.to_string())?;
    Ok(())
}

/// `delphi debug bundle [--since 24h] [-o file] [--mission <id>] [--workspace <name>] [--home <dir>]`.
pub fn debug(args: &[String]) -> i32 {
    if args.first().map(String::as_str) != Some("bundle") {
        eprintln!("usage: delphi debug bundle [--since 24h] [-o <file>] [--mission <id>] [--workspace <name>] [--home <dir>]");
        return 2;
    }
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let since = match flag(args, "--since").map(|s| crate::run::parse_duration(&s)) {
        Some(Ok(d)) => Some(now_unix() - d.as_secs() as i64),
        Some(Err(e)) => {
            eprintln!("delphi debug: {}", e);
            return 2;
        }
        None => Some(now_unix() - 24 * 3600),
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // The workspace is optional: without one the bundle still has the log, the version and the logging settings.
    let ws = match home::resolve_workspace(&home, &cwd, flag(args, "--workspace")) {
        home::Resolve::Found(n) => Some(home::Workspace { dir: home.join("workspaces").join(&n), name: n }),
        home::Resolve::NotInGitRepo if home.join("workspaces").join(crate::start::DEFAULT_WORKSPACE).join("config.toml").exists() => {
            Some(home::Workspace { dir: home.join("workspaces").join(crate::start::DEFAULT_WORKSPACE), name: crate::start::DEFAULT_WORKSPACE.to_string() })
        }
        _ => None,
    };
    let mission = flag(args, "--mission").or_else(|| ws.as_ref().and_then(|w| crate::missions::list(&w.dir).into_iter().next().map(|r| r.id)));
    let out = flag(args, "-o").or_else(|| flag(args, "--out")).map(PathBuf::from).unwrap_or_else(|| {
        let stamp = delphi_core::time::now_rfc3339().replace([':', '.'], "-");
        PathBuf::from(format!("delphi-debug-{}.tar.gz", stamp.trim_end_matches('Z')))
    });
    if out.exists() {
        eprintln!("delphi debug: {} already exists; choose another name with -o (nothing is overwritten)", out.display());
        return 2;
    }
    let b = collect(&home, ws.as_ref(), since, mission.as_deref());
    if let Err(e) = write_archive(&out, &b) {
        eprintln!("delphi debug: {}", e);
        return 1;
    }
    println!("wrote {} (mode 0600). It contains:", out.display());
    println!("  MANIFEST.txt");
    for (n, bytes) in &b.files {
        println!("  {} ({} bytes)", n, bytes.len());
    }
    for n in &b.notes {
        println!("  - {}", n);
    }
    println!("Everything in it is redacted, but read it before you share it.");
    0
}

#[cfg(test)]
#[path = "logs_cmd_tests.rs"]
mod tests;
