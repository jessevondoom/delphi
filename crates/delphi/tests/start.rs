//! Bare `delphi`: it creates what is missing and opens the console, with nothing to type first.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

/// Run `delphi` (stdin piped, so no prompts) in `cwd` with `/quit` on stdin; returns (exit code, stderr+stdout).
///
/// Output goes to files, not pipes: `delphi` starts a daemon that inherits its stdio, and `wait_with_output` on a
/// pipe waits for every holder of the write end to let go, so the test could block until that daemon exited (a hang
/// seen on the macOS runner). A file needs no EOF.
fn run_bare(home: &Path, cwd: &Path, extra: &[&str]) -> (i32, String) {
    use std::io::{Read, Seek, SeekFrom};
    let (mut out, mut err) = (tempfile::tempfile().unwrap(), tempfile::tempfile().unwrap());
    let mut child = Command::new(DELPHI)
        .args(extra)
        .current_dir(cwd)
        .env("DELPHI_HOME", home)
        .env_remove("DELPHI_WORKSPACE")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(out.try_clone().unwrap()))
        .stderr(Stdio::from(err.try_clone().unwrap()))
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"/quit\n").unwrap();
    let status = child.wait().unwrap();
    let read = |f: &mut std::fs::File| {
        let mut s = String::new();
        f.seek(SeekFrom::Start(0)).unwrap();
        f.read_to_string(&mut s).unwrap();
        s
    };
    (status.code().unwrap_or(-1), format!("{}{}", read(&mut err), read(&mut out)))
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
