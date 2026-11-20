//! `delphi self-install` and `delphi app opt-out` / `opt-in` at the command line (`specs/122` §10, AC-8 and AC-9): they need no
//! terminal, and an agent seat is refused before anything is written.

use std::path::Path;
use std::process::{Command, Output, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

fn run(args: &[&str], dir: &Path, env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(DELPHI);
    cmd.args(args).env_remove("DELPHI_WORKSPACE").env_remove("DELPHI_BRIDGE_SOCKET").env_remove("DELPHI_BRIDGE_TOKEN").env("HOME", dir).env("DELPHI_INSTALL_DIR", dir.join("bin")).stdin(Stdio::null()).current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_string()
}

/// The built program installs itself with no terminal; running it again changes nothing; a different earlier build of it (the
/// same program with a byte added) is replaced; a file that is not a delphi, and a link, are not.
#[test]
fn self_install_copies_the_program_and_refuses_what_is_not_one() {
    let t = tempfile::tempdir().unwrap();
    let dest = t.path().join("bin/delphi");
    let o = run(&["self-install"], t.path(), &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(text(&o.stdout).starts_with("Installed delphi "), "{}", text(&o.stdout));
    assert_eq!(std::fs::read(&dest).unwrap(), std::fs::read(DELPHI).unwrap());
    let o = run(&["self-install"], t.path(), &[]);
    assert!(o.status.success() && text(&o.stdout).contains("same program already"), "{}", text(&o.stdout));

    // An earlier build: this program with a byte more. Real delphi bytes carry the marker, so it is replaced.
    let mut older = std::fs::read(DELPHI).unwrap();
    older.push(0);
    std::fs::write(&dest, &older).unwrap();
    let o = run(&["self-install"], t.path(), &[]);
    assert!(o.status.success() && text(&o.stdout).contains("earlier delphi there was replaced"), "{}{}", text(&o.stdout), text(&o.stderr));
    assert_eq!(std::fs::read(&dest).unwrap(), std::fs::read(DELPHI).unwrap());

    // Not a delphi: refused, and left as it was.
    std::fs::write(&dest, b"#!/bin/sh\necho mine\n").unwrap();
    let o = run(&["self-install"], t.path(), &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("not a delphi"), "{}", text(&o.stderr));
    assert_eq!(std::fs::read(&dest).unwrap(), b"#!/bin/sh\necho mine\n");

    // A link: refused, and its target is untouched.
    std::fs::remove_file(&dest).unwrap();
    let target = t.path().join("victim");
    std::fs::write(&target, b"keep").unwrap();
    std::os::unix::fs::symlink(&target, &dest).unwrap();
    let o = run(&["self-install"], t.path(), &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("symbolic link"), "{}", text(&o.stderr));
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
}

#[test]
fn self_install_is_refused_inside_an_agent_seat_and_writes_nothing() {
    let t = tempfile::tempdir().unwrap();
    for var in ["DELPHI_BRIDGE_SOCKET", "DELPHI_BRIDGE_TOKEN"] {
        let o = run(&["self-install"], t.path(), &[(var, "x")]);
        assert_eq!(o.status.code(), Some(2), "{var}");
        assert!(text(&o.stderr).contains("agent seat"), "{}", text(&o.stderr));
        assert!(!t.path().join("bin").exists(), "{var}: nothing was written");
    }
}

#[test]
fn opt_out_and_opt_in_need_no_terminal_and_an_agent_seat_cannot_use_them() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("dh");
    let h = home.to_str().unwrap();
    let file = home.join("app/opt-out");
    for var in ["DELPHI_BRIDGE_SOCKET", "DELPHI_BRIDGE_TOKEN"] {
        let o = run(&["app", "opt-out", "--home", h], t.path(), &[(var, "x")]);
        assert_eq!(o.status.code(), Some(2), "{var}");
        assert!(!file.exists() && !home.exists(), "{var}: nothing was written");
    }
    let o = run(&["app", "opt-out", "--home", h], t.path(), &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(file.is_file() && std::fs::metadata(&file).unwrap().len() == 0);
    // An agent seat cannot undo it either.
    let o = run(&["app", "opt-in", "--home", h], t.path(), &[("DELPHI_BRIDGE_TOKEN", "x")]);
    assert_eq!(o.status.code(), Some(2));
    assert!(file.exists());
    let o = run(&["app", "opt-in", "--home", h], t.path(), &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(!file.exists());
}
