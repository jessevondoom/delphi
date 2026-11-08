//! Home isolation (spec 093 K3): commands that only print must not create a workspace, a log or anything else under the
//! caller's home. AGENTS.md promises that `delphi help <command>` prints one without creating anything, and that bare
//! `delphi` and `delphi init` create a real workspace under `~/.delphi`; this pins the first so the second stays the only
//! way a workspace appears by accident.

use regex::Regex;
use std::path::Path;
use std::process::{Command, Stdio};

/// Run the program as a caller whose home and working directory are `dir`, with no Delphi settings in the environment.
fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_delphi"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("xdg-config"))
        .env("XDG_DATA_HOME", dir.join("xdg-data"))
        .env("XDG_STATE_HOME", dir.join("xdg-state"))
        .env("XDG_CACHE_HOME", dir.join("xdg-cache"))
        .env_remove("DELPHI_HOME")
        .env_remove("DELPHI_WORKSPACE")
        .env_remove("DELPHI_LOG")
        .stdin(Stdio::null())
        .output()
        .expect("the delphi binary runs")
}

/// What is under `dir` now, as relative paths; empty when nothing was created.
fn contents(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            out.push(p.strip_prefix(dir).unwrap().to_string_lossy().to_string());
            if p.is_dir() {
                stack.push(p);
            }
        }
    }
    out.sort();
    out
}

/// The top-level command names, read from the generated reference so a new command is covered without editing this test.
fn command_names(dir: &Path) -> Vec<String> {
    let out = run(dir, &["docs", "cli"]);
    assert!(out.status.success(), "`delphi docs cli` runs");
    let re = Regex::new(r"(?m)^### `delphi ([a-z][a-z-]*)`").unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut names: Vec<String> = re.captures_iter(&text).map(|c| c[1].to_string()).collect();
    names.dedup();
    names
}

#[test]
fn printing_commands_create_nothing_under_the_callers_home() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().canonicalize().unwrap();
    let mut cases: Vec<Vec<String>> = vec![vec!["--version".into()], vec!["help".into()], vec!["docs".into(), "cli".into()], vec!["docs".into(), "catalog".into()], vec!["docs".into(), "man".into()]];
    // The old trap: a `--help` that was parsed as a command's own argument and went on to create a workspace.
    for n in ["init", "run", "setup", "doctor", "lint", "kb", "sessions"] {
        cases.push(vec![n.into(), "--help".into()]);
    }
    let names = command_names(&dir);
    assert!(names.len() >= 20, "the reference lists the commands (got {})", names.len());
    for n in &names {
        cases.push(vec!["help".into(), n.clone()]);
    }
    let mut problems = vec![];
    for args in &cases {
        let words: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let out = run(&dir, &words);
        let made = contents(&dir);
        if !made.is_empty() {
            problems.push(format!("`delphi {}` (exit {:?}) created: {}", args.join(" "), out.status.code(), made.join(", ")));
            break; // the first offender is the clearest; later ones would only repeat its files
        }
    }
    assert!(problems.is_empty(), "a printing command wrote under the caller's home:\n  {}", problems.join("\n  "));
}

#[test]
fn the_check_can_fail_because_init_does_create_a_workspace_under_the_callers_home() {
    // Negative control: without it the test above would pass just as well if the program ignored `HOME`. `delphi init` is the
    // documented way a workspace appears under `$HOME/.delphi`; with `HOME` pointed at an empty directory it must appear there.
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().canonicalize().unwrap();
    let out = run(&dir, &["init"]);
    let made = contents(&dir);
    assert!(made.iter().any(|p| p == ".delphi" || p.starts_with(".delphi/")), "`delphi init` (exit {:?}) should create ~/.delphi under the redirected HOME; saw {:?}", out.status.code(), made);
}
