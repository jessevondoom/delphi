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
    for cmd in ["update", "setup", "init", "doctor", "lint", "check", "decisions", "timers", "relay", "run", "chat", "models", "auth", "connectors", "needs", "kb", "observe", "narrator", "daemon", "xenia", "endpoints", "status", "missions", "skills", "hooks"] {
        for flag in ["--help", "-h"] {
            let (o, touched) = run(&[cmd, flag]);
            assert_eq!(o.status.code(), Some(0), "{} {}", cmd, flag);
            let out = text(&o.stdout);
            let lines: Vec<&str> = out.lines().collect();
            assert_eq!(lines[0], "usage:", "{} {}: {}", cmd, flag, out);
            let name = if cmd == "narrator" { "observe" } else { cmd };
            assert!(lines[1].starts_with(&format!("  delphi {}", name)), "{} {}: {}", cmd, flag, out);
            assert!(lines.iter().take_while(|l| !l.is_empty()).skip(1).all(|l| l.starts_with("  delphi ") || l.starts_with("    ")), "{} {}: option lines only before the sentence: {}", cmd, flag, out);
            assert!(lines.len() < 12, "{} {} printed more than its own page", cmd, flag);
            assert!(lines.iter().all(|l| l.chars().count() < 120), "{} {}: a line is too long:\n{}", cmd, flag, out);
            assert!(!touched, "{} {} wrote to the home", cmd, flag);
        }
    }
}

#[test]
fn the_command_list_is_grouped_one_sentence_each_and_names_the_environment() {
    let (o, touched) = run(&["--help"]);
    let out = text(&o.stdout);
    assert!(!touched);
    for section in ["\nStart\n", "\nSet up\n", "\nWork\n", "\nInspect\n", "\nMaintain\n", "\nEnvironment\n"] {
        assert!(out.contains(section), "section {:?} in\n{}", section, out);
    }
    for cmd in ["chat", "run", "status", "missions", "doctor", "update", "observe"] {
        let line = out.lines().find(|l| l.starts_with(&format!("  delphi {} ", cmd))).unwrap_or_else(|| panic!("{} is listed:\n{}", cmd, out));
        assert!(line.len() > 26, "{} has a sentence: {}", cmd, line);
    }
    for var in ["DELPHI_HOME", "DELPHI_WORKSPACE", "DELPHI_UI", "DELPHI_THEME", "NO_COLOR", "DELPHI_REDUCE_MOTION", "DELPHI_NO_ONBOARD"] {
        assert!(out.contains(var), "{} is described", var);
    }
    assert!(out.lines().all(|l| l.chars().count() < 100), "no line wider than 100 columns:\n{}", out);
    assert!(!out.contains("self-update") && !out.contains("rollback]"), "retired commands are not listed");
}

/// A workspace in a scratch home, so `status` and `missions` have something to read.
fn workspace() -> (tempfile::TempDir, std::path::PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let proj = t.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    Command::new("git").args(["init", "-q"]).current_dir(&proj).status().unwrap();
    let ok = Command::new(DELPHI).args(["init", "--non-interactive", "--workspace", "w", "--project"]).arg(&proj).arg("--home").arg(&home).stdin(Stdio::null()).output().unwrap();
    assert!(ok.status.success(), "{}", text(&ok.stderr));
    (t, home)
}

#[test]
fn status_is_a_few_lines_without_a_terminal_or_a_daemon() {
    let (_t, home) = workspace();
    let o = Command::new(DELPHI).args(["status", "--workspace", "w", "--home"]).arg(&home).stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    let labels: Vec<&str> = out.lines().filter_map(|l| l.split_whitespace().next()).collect();
    assert_eq!(labels, ["workspace", "daemon", "models", "health", "mission"], "{}", out);
    assert!(out.contains("not running"), "{}", out);
    assert!(out.contains("none configured: run `delphi setup`"), "a fresh workspace has no model: {}", out);
    assert!(out.contains("none yet"), "no mission before the first run: {}", out);
    // Reading the status started nothing.
    assert!(!home.join("run").read_dir().map(|mut d| d.any(|e| e.unwrap().path().extension().is_some_and(|x| x == "sock"))).unwrap_or(false), "no daemon was started");
}

#[test]
fn missions_lists_what_is_on_disk_newest_first() {
    let (_t, home) = workspace();
    let o = Command::new(DELPHI).args(["missions", "--workspace", "w", "--home"]).arg(&home).stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o.stdout).contains("no missions yet"), "{}", text(&o.stdout));
    let ev = home.join("workspaces/w/local/missions/2026-10-25-session/events");
    std::fs::create_dir_all(&ev).unwrap();
    std::fs::write(ev.join("000001.jsonl"), "{}\n").unwrap();
    std::fs::create_dir_all(home.join("workspaces/w/local/missions/2026-10-20-session")).unwrap();
    let o = Command::new(DELPHI).args(["missions", "--workspace", "w", "--home"]).arg(&home).stdin(Stdio::null()).output().unwrap();
    let out = text(&o.stdout);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].contains("2026-10-25-session") && lines[0].ends_with("(latest)"), "{}", out);
    assert!(lines.iter().any(|l| l.contains("2026-10-20-session")), "{}", out);
    assert!(out.contains("delphi chat --mission <id>"), "{}", out);
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
    for bad in ["stat", "bogus"] {
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
