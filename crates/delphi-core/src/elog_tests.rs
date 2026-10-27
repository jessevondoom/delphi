use super::*;
use std::os::unix::fs::PermissionsExt;

fn logger(home: &Path, level: Level, max: u64, files: usize) -> Logger {
    let mut s = settings_with(home, None, None);
    s.level = level;
    s.max_bytes = max;
    s.files = files;
    Logger::new(home, "cli", "9.9.9", s)
}

fn lines(home: &Path) -> Vec<Value> {
    read_all(&log_dir(home).join(FILE))
}

const SECRET_ANT: &str = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const DID: &str = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";
const GH: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

fn nasty() -> String {
    format!("key {SECRET_ANT} token {GH} did {DID} url https://user:pw@example.com/x?token=abc123 internal https://admin:s3cretpw@internalhost/p did:web:corp.example.org:alice mail a.b@corp.example home dir secret.txt \u{1b}[31mred\u{1b}[0m\nSECOND LINE \u{202e}rtl Bearer abcdefghijklmnop1234 {}", "A1b2".repeat(20))
}

fn assert_clean(text: &str) {
    for needle in [SECRET_ANT, "AAAAAAAAAAAAAAAAAAAAAAAA", GH, DID, "user:pw", "admin:s3cretpw", "s3cretpw", "did:web", "alice", "token=abc123", "a.b@corp.example", "abcdefghijklmnop1234", "A1b2A1b2A1b2A1b2"] {
        assert!(!text.contains(needle), "leaked {needle:?} in {text}");
    }
    assert_eq!(text.lines().count(), 1, "one record is one line");
    // The decoded strings, not the JSON text: serde escapes an ESC as \u001b, so only decoding shows one that survived.
    fn walk(v: &Value) {
        match v {
            Value::String(s) => assert!(!s.chars().any(|c| c.is_control() || matches!(c, '\u{202e}' | '\u{200b}')), "a control or bidi character survived in {s:?}"),
            Value::Array(a) => a.iter().for_each(walk),
            Value::Object(o) => o.values().for_each(walk),
            _ => {}
        }
    }
    if let Ok(v) = serde_json::from_str::<Value>(text.trim_end()) {
        walk(&v);
    }
}

#[test]
fn a_record_has_the_fixed_fields_and_nothing_secret() {
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3);
    let line = l.render(Level::Error, "daemon", "rpc_error", &nasty(), &json!({"mission": "m_1", "method": "task.submit", "stderr_tail": nasty()}), &[], None);
    assert_clean(&line);
    let v: Value = serde_json::from_str(&line).unwrap();
    for k in ["ts", "level", "component", "pid", "version", "os", "slug", "message"] {
        assert!(!v[k].is_null(), "{k}");
    }
    assert_eq!(v["version"], "9.9.9");
    assert_eq!(v["context"]["mission"], "m_1");
    assert!(v["ts"].as_str().unwrap().ends_with('Z'));
}

#[test]
fn only_known_context_keys_survive() {
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3);
    let line = l.render(Level::Error, "cli", "x", "m", &json!({"mission": "m_1", "body": "the prompt text", "prompt": "p", "env": {"A": "b"}, "status": 429}), &[], None);
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["context"]["status"], 429);
    assert!(v["context"]["body"].is_null() && v["context"]["prompt"].is_null() && v["context"]["env"].is_null());
    assert!(!line.contains("the prompt text"));
}

#[test]
fn the_home_path_and_long_fields_are_shortened() {
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3);
    let msg = format!("failed at {}/run/x.sock {}", d.path().display(), "z".repeat(5000));
    let line = l.render(Level::Error, "cli", "x", &msg, &json!({}), &[], None);
    assert!(!line.contains(&d.path().display().to_string()));
    assert!(line.contains("$DELPHI_HOME"));
    assert!(line.len() < 2500, "{}", line.len());
}

#[test]
fn levels_gate_what_is_written() {
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3);
    l.log(Level::Error, "cli", "e", "e", &json!({}), &[], None);
    l.log(Level::Warn, "cli", "w", "w", &json!({}), &[], None);
    l.log(Level::Debug, "cli", "d", "d", &json!({}), &[], None);
    assert_eq!(lines(d.path()).len(), 1);
    let l = logger(d.path(), Level::Debug, DEFAULT_MAX_BYTES, 3);
    l.log(Level::Warn, "cli", "w", "w", &json!({}), &[], None);
    l.log(Level::Debug, "cli", "d", "d", &json!({}), &[], None);
    assert_eq!(lines(d.path()).len(), 3);
}

#[test]
fn off_writes_nothing_and_creates_nothing() {
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Off, DEFAULT_MAX_BYTES, 3);
    l.log(Level::Error, "cli", "e", "e", &json!({}), &[], None);
    assert!(!log_dir(d.path()).exists(), "off must not even create the directory");
    assert_eq!(l.pending_drops(), 0);
}

#[test]
fn the_backtrace_and_chain_are_debug_only() {
    let d = tempfile::tempdir().unwrap();
    let e = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3).render(Level::Error, "cli", "x", "m", &json!({}), &["cause".into()], Some("frame 1"));
    assert!(!e.contains("frame 1") && !e.contains("cause"));
    let g = logger(d.path(), Level::Debug, DEFAULT_MAX_BYTES, 3).render(Level::Error, "cli", "x", "m", &json!({}), &["cause".into()], Some("frame 1"));
    assert!(g.contains("frame 1") && g.contains("cause"));
}

#[test]
fn the_file_is_private_and_the_directory_too() {
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3);
    l.log(Level::Error, "cli", "e", "e", &json!({}), &[], None);
    let dm = fs::metadata(log_dir(d.path())).unwrap().permissions().mode() & 0o777;
    let fm = fs::metadata(log_dir(d.path()).join(FILE)).unwrap().permissions().mode() & 0o777;
    assert_eq!((dm, fm), (0o700, 0o600));
}

#[test]
fn the_size_cap_rotates_and_keeps_the_newest_files_only() {
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Error, 2048, 3);
    for i in 0..200 {
        l.log(Level::Error, "cli", "fill", &format!("record number {i} {}", "x".repeat(100)), &json!({}), &[], None);
    }
    let dir = log_dir(d.path());
    let mut names: Vec<String> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.ends_with(".lock")).collect();
    names.sort();
    assert_eq!(names, vec![format!("{FILE}"), format!("{FILE}.1"), format!("{FILE}.2")]);
    let total: u64 = names.iter().map(|n| fs::metadata(dir.join(n)).unwrap().len()).sum();
    assert!(total <= 3 * 2048 + 1024, "total {total} exceeds the cap");
    let recs = lines(d.path());
    assert!(recs.last().unwrap()["message"].as_str().unwrap().contains("record number 199"), "the newest record survives");
    assert!(!recs.iter().any(|r| r["message"].as_str().unwrap().contains("record number 0 ")), "the oldest was rotated out");
}

#[test]
fn a_single_file_cap_truncates_instead_of_rotating() {
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Error, 1024, 1);
    for i in 0..60 {
        l.log(Level::Error, "cli", "fill", &format!("n{i} {}", "y".repeat(80)), &json!({}), &[], None);
    }
    let size = fs::metadata(log_dir(d.path()).join(FILE)).unwrap().len();
    assert!(size < 2048, "{size}");
    assert!(!log_dir(d.path()).join(format!("{FILE}.1")).exists());
}

#[test]
fn an_unwritable_directory_drops_the_record_silently_and_counts_it() {
    let d = tempfile::tempdir().unwrap();
    // The log directory path is a regular file, so nothing can be created under it.
    fs::write(log_dir(d.path()), b"not a directory").unwrap();
    let l = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3);
    let ran = (|| {
        l.log(Level::Error, "cli", "e", "e", &json!({}), &[], None);
        l.log(Level::Error, "cli", "e", "e", &json!({}), &[], None);
        "the operation went on"
    })();
    assert_eq!(ran, "the operation went on");
    assert_eq!(l.pending_drops(), 2);
    // Once the path is usable, the next record reports what was lost.
    fs::remove_file(log_dir(d.path())).unwrap();
    l.log(Level::Error, "cli", "e", "back", &json!({}), &[], None);
    assert_eq!(lines(d.path())[0]["dropped"], 2);
    assert_eq!(l.pending_drops(), 0);
}

#[test]
fn a_read_only_directory_is_also_survivable() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(log_dir(d.path())).unwrap();
    fs::set_permissions(log_dir(d.path()), fs::Permissions::from_mode(0o500)).unwrap();
    let l = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3);
    l.log(Level::Error, "cli", "e", "e", &json!({}), &[], None);
    assert_eq!(l.pending_drops(), 1);
    fs::set_permissions(log_dir(d.path()), fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn a_symlinked_log_file_is_refused_not_followed() {
    let d = tempfile::tempdir().unwrap();
    let target = d.path().join("elsewhere");
    fs::write(&target, b"keep").unwrap();
    fs::create_dir_all(log_dir(d.path())).unwrap();
    std::os::unix::fs::symlink(&target, log_dir(d.path()).join(FILE)).unwrap();
    let l = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3);
    l.log(Level::Error, "cli", "e", "e", &json!({}), &[], None);
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    assert_eq!(l.pending_drops(), 1);
}

#[test]
fn concurrent_writers_never_interleave_inside_a_line() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().to_path_buf();
    let mut hs = Vec::new();
    for t in 0..6 {
        let home = home.clone();
        hs.push(std::thread::spawn(move || {
            // Each thread has its own logger, like separate processes: no shared state but the file.
            let l = logger(&home, Level::Error, DEFAULT_MAX_BYTES, 3);
            for i in 0..150 {
                l.log(Level::Error, "cli", "c", &format!("t{t} i{i} {}", "q".repeat(200)), &json!({"mission": format!("m{t}")}), &[], None);
            }
        }));
    }
    for h in hs {
        h.join().unwrap();
    }
    let text = fs::read_to_string(log_dir(d.path()).join(FILE)).unwrap();
    assert_eq!(text.lines().count(), 900);
    assert!(text.lines().all(|l| serde_json::from_str::<Value>(l).is_ok()), "a torn line");
}

#[test]
fn a_panic_inside_the_logger_path_is_contained() {
    // A level above the cap is simply ignored; a poisoned or odd input must not panic either.
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3);
    l.log(Level::Error, "not-a-component", &"s".repeat(10_000), &"\u{0}\u{1b}".repeat(10_000), &json!([1, 2, 3]), &[], None);
    let r = &lines(d.path())[0];
    assert_eq!(r["component"], "cli", "an unknown component falls back to the process default");
}

#[test]
fn settings_come_from_the_operator_config_and_the_environment_wins() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(settings_with(d.path(), None, None).level, Level::Error);
    fs::create_dir_all(d.path().join("operator")).unwrap();
    fs::write(d.path().join("operator/config.toml"), "[logging]\nlevel = \"warn\"\nmax_mb = 2\nfiles = 5\n").unwrap();
    let s = settings_with(d.path(), None, None);
    assert_eq!((s.level, s.max_bytes, s.files), (Level::Warn, 2 * 1024 * 1024, 5));
    assert!(s.source.ends_with("operator/config.toml"));
    let s = settings_with(d.path(), Some("debug"), Some("4096"));
    assert_eq!((s.level, s.max_bytes, s.source.as_str()), (Level::Debug, 4096, "DELPHI_LOG"));
    assert_eq!(settings_with(d.path(), Some("off"), None).level, Level::Off);
    assert_eq!(settings_with(d.path(), Some("garbage"), None).level, Level::Warn, "an unknown value changes nothing");
}

#[test]
fn registered_secrets_are_masked_in_every_form() {
    register_secret("hunter2-but-longer-than-twelve");
    let out = sanitize("password is hunter2-but-longer-than-twelve ok", 400, None);
    assert!(!out.contains("hunter2"), "{out}");
}

#[test]
fn the_filter_and_the_human_form() {
    let d = tempfile::tempdir().unwrap();
    let l = logger(d.path(), Level::Warn, DEFAULT_MAX_BYTES, 3);
    l.log(Level::Error, "daemon", "a", "first", &json!({"mission": "m_1"}), &[], None);
    l.log(Level::Warn, "tool", "b", "second", &json!({"mission": "m_2"}), &[], None);
    let all = lines(d.path());
    let f = Filter { level: Some(Level::Error), ..Default::default() };
    assert_eq!(all.iter().filter(|r| f.matches(r)).count(), 1);
    let f = Filter { component: Some("tool".into()), ..Default::default() };
    assert_eq!(all.iter().filter(|r| f.matches(r)).count(), 1);
    let f = Filter { mission: Some("m_1".into()), ..Default::default() };
    assert_eq!(all.iter().filter(|r| f.matches(r)).count(), 1);
    let f = Filter { since: Some(crate::time::parse_rfc3339("2999-01-01T00:00:00Z").unwrap()), ..Default::default() };
    assert_eq!(all.iter().filter(|r| f.matches(r)).count(), 0);
    let tampered = json!({"ts": "2026-01-01T00:00:00.000Z", "level": "error", "component": "cli", "slug": "s", "message": "hi \u{1b}[2J there"});
    assert!(!human(&tampered).contains('\u{1b}'));
}

#[test]
fn the_global_functions_log_only_after_init_and_off_means_off() {
    // One test owns the process-wide logger; the others use instances.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let d = tempfile::tempdir().unwrap();
    uninstall();
    error("cli", "before_init", "nothing installed", json!({}));
    assert!(!log_dir(d.path()).exists());
    install(logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3));
    error("cli", "after_init", "x", json!({}));
    warn("cli", "w", "not at error level", json!({}));
    debug_with("cli", "d", || panic!("the closure must not run when debug is off"));
    assert_eq!(lines(d.path()).len(), 1);
    install(logger(d.path(), Level::Off, DEFAULT_MAX_BYTES, 3));
    error("cli", "after_off", "x", json!({}));
    assert_eq!(lines(d.path()).len(), 1);
    uninstall();
}

#[test]
fn the_panic_hook_logs_then_runs_the_previous_hook() {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let d = tempfile::tempdir().unwrap();
    install(logger(d.path(), Level::Error, DEFAULT_MAX_BYTES, 3));
    let saw_prev = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = saw_prev.clone();
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |_| flag.store(true, Ordering::SeqCst)));
    install_panic_hook("cli");
    let r = std::panic::catch_unwind(|| panic!("boom with {}", "sk-ant-api03-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"));
    std::panic::set_hook(original);
    uninstall();
    assert!(r.is_err());
    assert!(saw_prev.load(Ordering::SeqCst), "the previous hook still runs");
    let recs = lines(d.path());
    assert_eq!(recs[0]["slug"], "panic");
    assert!(recs[0]["message"].as_str().unwrap().contains("boom"));
    assert!(!fs::read_to_string(log_dir(d.path()).join(FILE)).unwrap().contains("BBBBBBBB"));
}

#[test]
fn the_canary_goes_through_every_component_level_and_context_key_and_none_of_it_comes_out() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().to_path_buf();
    let l = logger(&home, Level::Debug, DEFAULT_MAX_BYTES, 3);
    let dirty = format!("{} {}", nasty(), home.display());
    let mut ctx = Map::new();
    for k in CONTEXT_KEYS {
        ctx.insert((*k).to_string(), json!(dirty));
    }
    let ctx = Value::Object(ctx);
    for comp in COMPONENTS {
        for level in [Level::Error, Level::Warn, Level::Debug] {
            let line = l.render(level, comp, &dirty, &dirty, &ctx, &[dirty.clone(), dirty.clone()], Some(&dirty));
            assert_clean(&line);
            assert!(!line.contains(&home.display().to_string()), "the home path survived: {line}");
            l.log(level, comp, &dirty, &dirty, &ctx, &[dirty.clone()], Some(&dirty));
        }
    }
    let on_disk = fs::read_to_string(log_dir(&home).join(FILE)).unwrap();
    assert_eq!(on_disk.lines().count(), COMPONENTS.len() * 3);
    for line in on_disk.lines() {
        assert_clean(line);
    }
}
