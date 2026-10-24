//! Bare `delphi`: it creates what is missing and opens the console, with nothing to type first.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

/// Run `delphi` (piped, so no prompts) in `cwd` with `/quit` on stdin; returns (exit code, stderr+stdout).
fn run_bare(home: &Path, cwd: &Path, extra: &[&str]) -> (i32, String) {
    let mut child = Command::new(DELPHI)
        .args(extra)
        .current_dir(cwd)
        .env("DELPHI_HOME", home)
        .env_remove("DELPHI_WORKSPACE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"/quit\n").unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout)))
}

fn stop_daemon(home: &Path, ws: &str) {
    let _ = Command::new(DELPHI).args(["daemon", "stop", "--workspace", ws]).env("DELPHI_HOME", home).output();
}

#[test]
fn outside_a_repository_it_uses_a_default_workspace_with_only_the_built_in_context() {
    let t = tempfile::tempdir().unwrap();
    let (home, cwd) = (t.path().join("home"), t.path().join("scratch"));
    std::fs::create_dir_all(&cwd).unwrap();
    let (code, out) = run_bare(&home, &cwd, &[]);
    stop_daemon(&home, "default");
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("created the `default` workspace"), "{}", out);
    assert!(out.contains("/help for commands"), "the console opened: {}", out);
    assert!(home.join("workspaces/default/config.toml").exists());
}

#[test]
fn in_a_new_project_it_creates_the_workspace_and_writes_nothing_into_the_project() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let proj = t.path().join("app");
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    let (code, out) = run_bare(&home, &proj, &[]);
    stop_daemon(&home, "app");
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("created the workspace `app`"), "{}", out);
    assert!(home.join("workspaces/app/config.toml").exists());
    let names: Vec<_> = std::fs::read_dir(&proj).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names, vec![".git"], "the project is untouched");
    // The second launch finds it: no note.
    let (_, again) = run_bare(&home, &proj, &[]);
    stop_daemon(&home, "app");
    assert!(!again.contains("First time in"), "{}", again);
}

#[test]
fn an_unknown_workspace_is_an_error_and_creates_nothing() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let (code, out) = run_bare(&home, t.path(), &["--workspace", "nope"]);
    assert_eq!(code, 2, "{}", out);
    assert!(out.contains("no workspace named `nope`"), "{}", out);
    assert!(!home.join("workspaces/nope").exists());
}
