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

    // Another build of the same version: this program with a byte more. Real delphi bytes carry the marker, so it is replaced.
    let mut older = std::fs::read(DELPHI).unwrap();
    older.push(0);
    std::fs::write(&dest, &older).unwrap();
    let o = run(&["self-install"], t.path(), &[]);
    assert!(o.status.success() && text(&o.stdout).contains(&format!("another build of delphi {} there was replaced", env!("CARGO_PKG_VERSION"))), "{}{}", text(&o.stdout), text(&o.stderr));
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

/// V3-2: the built program carries its version as a fixed text, and a delphi whose text names a newer version is kept unless
/// `--allow-downgrade` is given.
#[test]
fn self_install_keeps_a_newer_delphi_unless_allow_downgrade() {
    let t = tempfile::tempdir().unwrap();
    let dest = t.path().join("bin/delphi");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let ours = std::fs::read(DELPHI).unwrap();
    let tag = format!("\0delphi-version:{}\0", env!("CARGO_PKG_VERSION")).into_bytes();
    let at = ours.windows(tag.len()).position(|w| w == tag.as_slice()).expect("the program carries its version text");
    // The same program, its version text changed to a newer one of the same length (the major version raised by 1..9).
    let newer: String = env!("CARGO_PKG_VERSION").chars().enumerate().map(|(i, c)| if i == 0 { char::from_digit(c.to_digit(10).unwrap() % 9 + 1, 10).unwrap() } else { c }).collect();
    assert_eq!(newer.len(), env!("CARGO_PKG_VERSION").len());
    let mut theirs = ours.clone();
    theirs[at..at + tag.len()].copy_from_slice(format!("\0delphi-version:{newer}\0").as_bytes());
    std::fs::write(&dest, &theirs).unwrap();
    let o = run(&["self-install"], t.path(), &[]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stdout));
    assert!(text(&o.stderr).contains(&format!("is delphi {newer}, newer than this")) && text(&o.stderr).contains("--allow-downgrade"), "{}", text(&o.stderr));
    assert_eq!(std::fs::read(&dest).unwrap(), theirs, "left as it was");
    let o = run(&["self-install", "--allow-downgrade"], t.path(), &[]);
    assert!(o.status.success() && text(&o.stdout).contains(&format!("a newer delphi {newer}")), "{}{}", text(&o.stdout), text(&o.stderr));
    assert_eq!(std::fs::read(&dest).unwrap(), ours);
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

/// Spec 122 §10.1 item 4: `delphi update` run from the copy inside the desktop app's bundle refuses before it reads a release,
/// names `self-install`, and leaves the copy as it was; `--check` writes nothing and is let through.
#[test]
fn update_from_inside_the_app_bundle_is_refused_and_names_self_install() {
    let t = tempfile::tempdir().unwrap();
    let inner = t.path().join("Delphi.app/Contents/Resources/delphi");
    std::fs::create_dir_all(inner.parent().unwrap()).unwrap();
    std::fs::copy(DELPHI, &inner).unwrap();
    let before = std::fs::read(&inner).unwrap();
    let home = t.path().join("dh");
    let o = Command::new(&inner).args(["update", "--home", home.to_str().unwrap()]).env("HOME", t.path()).env("DELPHI_RELEASES_URL", "http://127.0.0.1:9/releases").env("DELPHI_NO_APP", "1").stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("inside the desktop app") && text(&o.stderr).contains("self-install"), "{}", text(&o.stderr));
    assert!(!text(&o.stderr).contains("127.0.0.1"), "refused before any release is read: {}", text(&o.stderr));
    assert_eq!(std::fs::read(&inner).unwrap(), before);
    let o = Command::new(&inner).args(["update", "--check", "--home", home.to_str().unwrap()]).env("HOME", t.path()).env("DELPHI_RELEASES_URL", "http://127.0.0.1:9/releases").stdin(Stdio::null()).output().unwrap();
    assert!(!text(&o.stderr).contains("inside the desktop app"), "{}", text(&o.stderr));
}

/// Spec 122 §10.3 (V3-1): the opt-out written by `delphi app opt-out --home A` is the one `delphi update --home A` reads, and an
/// opt-out under another home is not. Shown with `--rollback`, which puts a kept app back unless the home has opted out.
#[cfg(target_os = "macos")]
#[test]
fn update_reads_the_opt_out_of_the_home_it_was_given() {
    let t = tempfile::tempdir().unwrap();
    let bin = t.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(DELPHI, bin.join("delphi")).unwrap();
    let apps = t.path().join("Applications");
    let (a, b) = (t.path().join("home-a"), t.path().join("home-b"));
    let o = run(&["app", "opt-out", "--home", a.to_str().unwrap()], t.path(), &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let rollback = |home: &Path| {
        std::fs::copy(DELPHI, bin.join("delphi.prev")).unwrap();
        std::fs::create_dir_all(apps.join(".delphi-app.prev/Contents")).unwrap();
        let o = Command::new(bin.join("delphi")).args(["update", "--rollback", "--no-restart", "--home", home.to_str().unwrap()]).env("HOME", t.path()).env_remove("DELPHI_HOME").env_remove("DELPHI_NO_APP").env_remove("DELPHI_WORKSPACE").stdin(Stdio::null()).output().unwrap();
        assert!(text(&o.stdout).contains("Restored the previous version"), "{}{}", text(&o.stdout), text(&o.stderr));
    };
    rollback(&a);
    assert!(!apps.join("Delphi.app").exists() && apps.join(".delphi-app.prev").is_dir(), "home A opted out, so the app was left alone");
    rollback(&b);
    assert!(apps.join("Delphi.app").is_dir(), "home B did not opt out, so the kept app was put back");
}
