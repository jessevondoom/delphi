//! The central error log from the outside (`specs/039`): what the binary records, what `delphi logs` and `delphi debug bundle`
//! show, and the promise that logging never changes what a command does. Every run uses a temporary `--home`.

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");
const SECRET: &str = "sk-ant-api03-FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF";
const DID: &str = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";

fn delphi(home: &Path, level: Option<&str>, args: &[&str]) -> Output {
    let mut c = Command::new(DELPHI);
    c.args(args).arg("--home").arg(home).env_remove("DELPHI_WORKSPACE").env_remove("DELPHI_LOG").env_remove("DELPHI_HOME").stdin(Stdio::null()).current_dir(home.parent().unwrap());
    if let Some(l) = level {
        c.env("DELPHI_LOG", l);
    }
    c.output().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn home() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let h = t.path().join("home");
    std::fs::create_dir(&h).unwrap();
    (t, h)
}

fn log_path(home: &Path) -> PathBuf {
    home.join("logs/delphi.log.jsonl")
}

fn records(home: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(log_path(home)).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// A failing command: `debug` with anything but `bundle` exits 2.
fn failing(home: &Path, level: Option<&str>) -> Output {
    delphi(home, level, &["debug", "--token", SECRET])
}

#[test]
fn a_failing_command_is_one_record_with_the_shape_of_its_arguments_and_none_of_their_values() {
    let (_t, h) = home();
    let o = failing(&h, Some("warn"));
    assert_eq!(o.status.code(), Some(2));
    let r = records(&h);
    assert_eq!(r.len(), 1, "{:?}", r);
    assert_eq!((r[0]["slug"].as_str(), r[0]["component"].as_str(), r[0]["level"].as_str()), (Some("cli_exit"), Some("cli"), Some("warn")));
    assert_eq!((r[0]["context"]["command"].as_str(), r[0]["context"]["exit_code"].as_i64()), (Some("debug"), Some(2)));
    let raw = std::fs::read_to_string(log_path(&h)).unwrap();
    assert!(!raw.contains("FFFFFFFF") && !raw.contains(&h.display().to_string()), "{raw}");
    assert_eq!(std::fs::metadata(log_path(&h)).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(h.join("logs")).unwrap().permissions().mode() & 0o777, 0o700);
}

#[test]
fn the_default_level_is_error_and_off_writes_nothing() {
    let (_t, h) = home();
    // Exit code 2 (a usage mistake) is a warning, so the default level does not record it.
    failing(&h, None);
    assert!(!h.join("logs").exists(), "the default keeps errors only");
    failing(&h, Some("off"));
    assert!(!h.join("logs").exists(), "off writes nothing, not even the directory");
    // The operator-scope config sets the level; the environment still wins.
    std::fs::create_dir_all(h.join("operator")).unwrap();
    std::fs::write(h.join("operator/config.toml"), "[logging]\nlevel = \"warn\"\n").unwrap();
    failing(&h, None);
    assert_eq!(records(&h).len(), 1);
    failing(&h, Some("off"));
    assert_eq!(records(&h).len(), 1, "DELPHI_LOG=off beats the config");
}

#[test]
fn an_unwritable_log_directory_changes_nothing_about_the_command() {
    let (_t, h) = home();
    let baseline = failing(&h, Some("off"));
    // `logs` is a file, so no log can be created; the command must end exactly as before.
    std::fs::write(h.join("logs"), b"in the way").unwrap();
    let blocked = failing(&h, Some("debug"));
    assert_eq!((blocked.status.code(), text(&blocked.stdout), text(&blocked.stderr)), (baseline.status.code(), text(&baseline.stdout), text(&baseline.stderr)));
    // And a read-only directory.
    std::fs::remove_file(h.join("logs")).unwrap();
    std::fs::create_dir(h.join("logs")).unwrap();
    std::fs::set_permissions(h.join("logs"), std::fs::Permissions::from_mode(0o500)).unwrap();
    let ro = failing(&h, Some("debug"));
    std::fs::set_permissions(h.join("logs"), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!((ro.status.code(), text(&ro.stdout), text(&ro.stderr)), (baseline.status.code(), text(&baseline.stdout), text(&baseline.stderr)));
}

fn seed_hostile_log(h: &Path) {
    std::fs::create_dir_all(h.join("logs")).unwrap();
    let hostile = serde_json::json!({"ts": "2999-01-01T00:00:00.000Z", "level": "error", "component": "tool", "slug": "tool_failed", "message": format!("\u{1b}[2J\u{1b}[31m{SECRET} {DID} \u{202e}reversed"), "context": {"mission": "m1", "tool": "write_file"}});
    let plain = serde_json::json!({"ts": "2999-01-01T00:00:01.000Z", "level": "warn", "component": "daemon", "slug": "rpc_forbidden", "message": "refused", "context": {"mission": "m2"}});
    std::fs::write(log_path(h), format!("{hostile}\nnot json at all\n{plain}\n")).unwrap();
}

#[test]
fn logs_prints_sanitized_records_and_filters() {
    let (_t, h) = home();
    let o = delphi(&h, None, &["logs", "--path"]);
    assert_eq!(text(&o.stdout).trim(), log_path(&h).display().to_string());
    assert!(!h.join("logs").exists(), "--path creates nothing");
    let none = delphi(&h, None, &["logs"]);
    assert_eq!(none.status.code(), Some(0));
    assert!(text(&none.stderr).contains("nothing logged yet"));

    seed_hostile_log(&h);
    let all = delphi(&h, None, &["logs"]);
    let out = text(&all.stdout);
    assert_eq!(out.lines().count(), 2, "the junk line is skipped: {out}");
    assert!(!out.contains('\u{1b}') && !out.contains('\u{202e}'), "a terminal must never get the raw escape: {out:?}");
    let json = delphi(&h, None, &["logs", "--json", "--level", "error"]);
    let j = text(&json.stdout);
    assert_eq!(j.lines().count(), 1);
    assert!(!j.contains('\u{1b}') && !j.contains("FFFFFFFF") && !j.contains("z6Mk"), "{j:?}");
    assert_eq!(text(&delphi(&h, None, &["logs", "--component", "daemon"]).stdout).lines().count(), 1);
    assert_eq!(text(&delphi(&h, None, &["logs", "--mission", "m1"]).stdout).lines().count(), 1);
    assert_eq!(text(&delphi(&h, None, &["logs", "--lines", "1"]).stdout).lines().count(), 1);
    assert_eq!(text(&delphi(&h, None, &["logs", "--since", "1h"]).stdout).lines().count(), 2, "the records are dated in the future, so they are inside any window");
    for bad in [&["logs", "--level", "loud"][..], &["logs", "--since", "soon"], &["logs", "--component", "nope"], &["logs", "--lines", "0"]] {
        assert_eq!(delphi(&h, None, bad).status.code(), Some(2), "{bad:?}");
    }
}

#[test]
fn logs_follow_prints_a_record_written_while_it_runs() {
    let (_t, h) = home();
    std::fs::create_dir_all(h.join("logs")).unwrap();
    std::fs::write(log_path(&h), "").unwrap();
    let mut child = Command::new(DELPHI).args(["logs", "--follow", "--follow-for", "3", "--home"]).arg(&h).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(800));
    failing(&h, Some("warn"));
    let mut out = String::new();
    child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    child.wait().unwrap();
    assert!(out.contains("cli_exit"), "{out}");
}

fn unpack(path: &Path) -> Vec<(String, String)> {
    let mut a = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap()));
    a.entries().unwrap().map(|e| {
        let mut e = e.unwrap();
        let mut s = String::new();
        e.read_to_string(&mut s).unwrap();
        (e.path().unwrap().to_string_lossy().into_owned(), s)
    }).collect()
}

#[test]
fn the_debug_bundle_is_private_redacted_and_never_overwrites() {
    let (t, h) = home();
    seed_hostile_log(&h);
    std::fs::create_dir_all(h.join("operator")).unwrap();
    std::fs::write(h.join("operator/config.toml"), "[relay]\ntoken = \"hunter2-hunter2\"\n").unwrap();
    let out = t.path().join("share.tar.gz");
    let o = delphi(&h, None, &["debug", "bundle", "-o", out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let said = text(&o.stdout);
    for part in ["MANIFEST.txt", "delphi.log.jsonl", "config.txt", "version.txt"] {
        assert!(said.contains(part), "it prints what it included: {said}");
    }
    assert_eq!(std::fs::metadata(&out).unwrap().permissions().mode() & 0o777, 0o600);
    let before = std::fs::read(&out).unwrap();
    let again = delphi(&h, None, &["debug", "bundle", "-o", out.to_str().unwrap()]);
    assert_eq!(again.status.code(), Some(2));
    assert!(text(&again.stderr).contains("already exists"));
    assert_eq!(std::fs::read(&out).unwrap(), before, "the first bundle is untouched");
    for (n, body) in unpack(&out) {
        for bad in [SECRET, "FFFFFFFF", DID, "z6Mkha", "hunter2", "\u{1b}", "\u{202e}", &h.display().to_string()] {
            assert!(!body.contains(bad), "{n} contains {bad:?}");
        }
    }
    // Nothing is written next to the home, and the bundle command itself logs nothing at the default level.
    assert_eq!(delphi(&h, None, &["debug"]).status.code(), Some(2));
}

#[test]
fn doctor_reports_the_level_the_path_the_size_and_the_drops() {
    let (_t, h) = home();
    let init = delphi(&h, None, &["init", "--non-interactive", "--workspace", "w"]);
    assert_eq!(init.status.code(), Some(0), "{}", text(&init.stderr));
    let d = delphi(&h, None, &["doctor", "--workspace", "w"]);
    let out = text(&d.stdout);
    let line = out.lines().find(|l| l.contains("14 error-log")).unwrap_or_else(|| panic!("{out}"));
    assert!(line.contains("level error") && line.contains("default") && line.contains("delphi.log.jsonl") && line.contains("dropped"), "{line}");
    let off = delphi(&h, Some("off"), &["doctor", "--workspace", "w"]);
    assert!(text(&off.stdout).lines().any(|l| l.contains("14 error-log") && l.contains("off")));
}
