//! The plain command line: help and mistakes must be answered without side effects.

use std::process::{Command, Output, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

/// Run `delphi` against an empty home and return the output plus whether anything was written to the home.
fn run(args: &[&str]) -> (Output, bool) {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let out = Command::new(DELPHI)
        .args(args)
        .arg("--home")
        .arg(&home)
        .env_remove("DELPHI_WORKSPACE")
        .stdin(Stdio::null())
        .current_dir(t.path())
        .output()
        .unwrap();
    let touched = std::fs::read_dir(&home).unwrap().next().is_some();
    (out, touched)
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_string()
}

#[test]
fn help_is_stdout_exit_zero_and_touches_nothing() {
    for args in [&["--help"][..], &["-h"], &["help"]] {
        let (o, touched) = run(args);
        assert_eq!(o.status.code(), Some(0), "{:?}", args);
        assert!(text(&o.stdout).starts_with("usage:"), "{:?}", args);
        assert!(!touched, "{:?} wrote to the home", args);
    }
}

#[test]
fn every_command_answers_help_with_only_its_own_usage() {
    for cmd in ["update", "setup", "init", "doctor", "lint", "check", "decisions", "timers", "relay", "run", "chat", "models", "auth", "connectors", "needs", "kb", "observe", "narrator", "daemon", "xenia", "endpoints"] {
        for flag in ["--help", "-h"] {
            let (o, touched) = run(&[cmd, flag]);
            assert_eq!(o.status.code(), Some(0), "{} {}", cmd, flag);
            let out = text(&o.stdout);
            assert!(out.starts_with("usage:"), "{} {}: {}", cmd, flag, out);
            assert!(out.lines().skip(1).all(|l| l.starts_with("  delphi ")), "{} {}: {}", cmd, flag, out);
            assert!(out.lines().count() < 6, "{} {} printed the whole table", cmd, flag);
            assert!(!touched, "{} {} wrote to the home", cmd, flag);
        }
    }
}

#[test]
fn help_for_a_command_works_before_or_after_flags() {
    let (o, touched) = run(&["--workspace", "w", "init", "--help"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o.stdout).contains("delphi init"));
    assert!(!touched);
    let (o, _) = run(&["help", "doctor"]);
    assert!(text(&o.stdout).contains("delphi doctor"));
}

#[test]
fn an_unknown_command_is_named_and_exits_two() {
    for bad in ["status", "bogus"] {
        let (o, touched) = run(&[bad]);
        assert_eq!(o.status.code(), Some(2));
        let err = text(&o.stderr);
        assert!(err.lines().next().unwrap().contains(&format!("`{}`", bad)), "{}", err);
        assert!(!err.contains("usage:"), "full usage dumped: {}", err);
        assert!(!touched);
    }
}

#[test]
fn flags_before_the_command_do_not_swallow_it() {
    // `--home X doctor` runs doctor, not the console; with no workspace it says so rather than creating one.
    let (o, touched) = run(&["doctor"]);
    let (o2, touched2) = {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let o = Command::new(DELPHI).arg("--home").arg(&home).arg("doctor").stdin(Stdio::null()).current_dir(t.path()).output().unwrap();
        (o, std::fs::read_dir(&home).unwrap().next().is_some())
    };
    assert_eq!(o.status.code(), o2.status.code());
    assert_eq!(touched, touched2);
    assert!(!text(&o2.stdout).contains("created the workspace"));
}

#[test]
fn lint_outside_a_repository_is_an_error() {
    let t = tempfile::tempdir().unwrap();
    let o = Command::new(DELPHI).arg("lint").current_dir(t.path()).stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("not in a git repository"));
}

#[test]
fn the_classic_observer_is_gone_and_the_retired_spend_view_is_refused() {
    for args in [&["observe", "--classic"][..], &["narrator", "--classic"]] {
        let (o, touched) = run(args);
        assert_eq!(o.status.code(), Some(2), "{:?}", args);
        assert!(text(&o.stderr).contains("unknown option `--classic`"), "{}", text(&o.stderr));
        assert!(!touched);
    }
    let (o, touched) = run(&["observe", "spend", "--workspace", "none"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("unknown view `spend`") || text(&o.stderr).contains("no workspace"), "{}", text(&o.stderr));
    assert!(!touched);
}
