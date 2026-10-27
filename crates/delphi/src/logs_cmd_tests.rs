use super::*;
use delphi_core::elog::{Logger, FILE};
use delphi_core::event::EventEnvelope;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;

const SECRET: &str = "sk-ant-api03-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC";
const DID: &str = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";

fn write_log(home: &Path, n: usize) {
    let mut s = elog::settings_with(home, None, None);
    s.level = Level::Debug;
    let l = Logger::new(home, "cli", "1.2.3", s);
    for i in 0..n {
        l.log(Level::Error, "daemon", "rpc_error", &format!("record {i} {SECRET} {DID}"), &json!({"mission": "m1", "method": "task.submit"}), &[], None);
    }
}

fn unpack(path: &Path) -> Vec<(String, String)> {
    let f = std::fs::File::open(path).unwrap();
    let mut a = tar::Archive::new(flate2::read::GzDecoder::new(f));
    let mut out = Vec::new();
    for e in a.entries().unwrap() {
        let mut e = e.unwrap();
        let name = e.path().unwrap().to_string_lossy().to_string();
        let mut s = String::new();
        e.read_to_string(&mut s).unwrap();
        out.push((name, s));
    }
    out
}

#[test]
fn config_values_are_reduced_to_key_paths_and_lengths() {
    let out = redact_config("theme = \"greek\"\n[[endpoints]]\nid = \"direct\"\ncredential = \"keychain:abc-secret-value\"\nport = 8080\n[harness.claude]\nenabled = true\n");
    assert!(out.contains("endpoints = [1 item(s)]") && out.contains("endpoints[0].credential = \"[redacted: 25 chars]\"") && out.contains("endpoints[0].port = 8080") && out.contains("harness.claude.enabled = true"), "{out}");
    assert!(!out.contains("greek") && !out.contains("abc-secret") && !out.contains("direct"), "{out}");
    assert!(redact_config("= not toml [").contains("omitted"));
}

#[test]
fn mission_events_keep_ids_and_short_status_fields_only() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("m1");
    let mut log = delphi_core::log::EventLog::open(&dir, delphi_core::log::LogOptions::default()).unwrap();
    for i in 0..5 {
        let mut e = EventEnvelope::new("m1", "task.failed", json!({"error": format!("failed {i} {SECRET}"), "prompt": "SECRET PROMPT BODY", "text": "MODEL OUTPUT BODY", "content": "FILE CONTENTS", "block": {"text": "nested body"}}));
        e.task_id = Some("t_1".into());
        log.append(e).unwrap();
    }
    let out = event_summaries(&dir, 3, t.path()).unwrap();
    assert_eq!(out.lines().count(), 3, "only the last N");
    for bad in ["SECRET PROMPT BODY", "MODEL OUTPUT BODY", "FILE CONTENTS", "nested body", SECRET] {
        assert!(!out.contains(bad), "{bad} leaked: {out}");
    }
    assert!(out.contains("task.failed") && out.contains("t_1") && out.contains("failed 4"));
}

#[test]
fn a_bundle_is_redacted_private_and_never_overwrites() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    std::fs::create_dir_all(home.join("operator")).unwrap();
    std::fs::write(home.join("operator/config.toml"), "[logging]\nlevel = \"debug\"\n[relay]\ntoken = \"hunter2-hunter2-hunter2\"\n").unwrap();
    write_log(&home, 3);
    // A hand-edited line with an escape sequence and a secret: the bundle cleans what it reads back.
    let path = home.join("logs").join(FILE);
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(&format!("{}\n", json!({"ts": "2999-01-01T00:00:00.000Z", "level": "error", "component": "cli", "slug": "edited", "message": format!("\u{1b}[31m{SECRET} \u{202e}x")})));
    std::fs::write(&path, text).unwrap();

    let b = collect(&home, None, None, None);
    let out = t.path().join("bundle.tar.gz");
    write_archive(&out, &b).unwrap();
    assert_eq!(std::fs::metadata(&out).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(write_archive(&out, &b).unwrap_err().contains("cannot create"), "an existing file is never overwritten");
    let files = unpack(&out);
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["delphi-debug/MANIFEST.txt", "delphi-debug/version.txt", "delphi-debug/delphi.log.jsonl", "delphi-debug/config.txt"]);
    for (n, body) in &files {
        for bad in [SECRET, "CCCCCCCCCCCC", DID, "hunter2", "\u{1b}", "\u{202e}", &home.display().to_string()] {
            assert!(!body.contains(bad), "{n} leaked {bad:?}");
        }
    }
    let manifest = &files[0].1;
    assert!(manifest.contains("4 record(s)") && manifest.contains("skipped (no workspace"), "{manifest}");
    assert!(files[3].1.contains("relay.token = \"[redacted: 23 chars]\"") && files[3].1.contains("logging level: debug"));
}

#[test]
fn since_limits_the_slice() {
    let t = tempfile::tempdir().unwrap();
    write_log(t.path(), 2);
    let future = Some(now_unix() + 3600);
    let b = collect(t.path(), None, future, None);
    let log = b.files.iter().find(|(n, _)| n == "delphi.log.jsonl").unwrap();
    assert!(log.1.is_empty());
}

#[test]
fn follow_prints_what_is_appended_and_survives_rotation() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().to_path_buf();
    write_log(&home, 1);
    let path = home.join("logs").join(FILE);
    let started = std::time::Instant::now();
    let p2 = home.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        write_log(&p2, 1);
        let _ = std::fs::rename(p2.join("logs").join(FILE), p2.join("logs").join(format!("{FILE}.1")));
        std::thread::sleep(Duration::from_millis(500));
        write_log(&p2, 1);
    });
    follow(&path, &Filter::default(), true, &|| started.elapsed() > Duration::from_millis(2500));
    writer.join().unwrap();
}

#[test]
fn filters_reject_bad_input() {
    assert!(filter_from(&["--level".into(), "loud".into()]).is_err());
    assert!(filter_from(&["--component".into(), "nope".into()]).is_err());
    assert!(filter_from(&["--since".into(), "soon".into()]).is_err());
    let f = filter_from(&["--since".into(), "1h".into(), "--level".into(), "warn".into(), "--component".into(), "tool".into(), "--mission".into(), "m1".into()]).unwrap();
    assert_eq!((f.level, f.component.as_deref(), f.mission.as_deref()), (Some(Level::Warn), Some("tool"), Some("m1")));
    assert!(f.since.unwrap() <= now_unix() - 3590);
}
