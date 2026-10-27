//! `delphi xenia admin ...` as a real process: what a script, a pipe or an environment variable can and cannot do.
//! Every run uses a temporary `--home`; nothing here touches the operator's own home.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

/// Run `delphi xenia admin <args>` with `stdin_text` piped in and approval-looking environment variables set.
fn admin(home: &std::path::Path, args: &[&str], stdin_text: &str) -> Output {
    let mut child = Command::new(DELPHI)
        .args(["xenia", "admin"])
        .args(args)
        .arg("--home")
        .arg(home)
        .env_remove("DELPHI_WORKSPACE")
        .env("DELPHI_XENIA_APPROVE", "yes")
        .env("XENIA_YES", "1")
        .env("CI", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(home.parent().unwrap())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(stdin_text.as_bytes());
    child.wait_with_output().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_string()
}

fn empty(home: &std::path::Path) -> bool {
    std::fs::read_dir(home).unwrap().next().is_none()
}

#[test]
fn a_piped_stdin_cannot_sign_anything_and_the_refusal_comes_before_any_key_or_store_is_read() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["sign", "2"],
        vec!["publish", "2"],
        vec!["refresh"],
        vec!["init", "sign", "nofile", "--org", "acme", "--store", "acme-main", "--out", "o"],
        vec!["init", "create", "nofile", "--org", "acme", "--store", "acme-main", "--url", "https://example.invalid/x", "--sig", "s"],
    ];
    for args in cases {
        // The right-looking answers on stdin, as an attacker who read the source would send them.
        let o = admin(&home, &args, "yes\ny\nsign\nblake3\n\n");
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}{}", text(&o.stdout), text(&o.stderr));
        assert!(text(&o.stderr).contains("human act"), "{args:?}: {}", text(&o.stderr));
        assert!(empty(&home), "{args:?} wrote into the home (a key, a store, a lock)");
    }
}

#[test]
fn there_is_no_flag_that_answers_for_a_person() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    std::fs::create_dir(&home).unwrap();
    for flag in ["--yes", "-y", "--force", "--no-confirm", "--approve", "--non-interactive"] {
        let o = admin(&home, &["refresh", flag], "");
        assert_eq!(o.status.code(), Some(2), "{flag}: {}", text(&o.stderr));
        assert!(text(&o.stderr).contains("no flag that answers for you"), "{flag}: {}", text(&o.stderr));
        assert!(empty(&home));
    }
}

#[test]
fn rotate_without_a_kind_is_a_usage_error_that_explains_both() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let o = admin(&home, &["rotate", "did:key:zOld", "did:key:zNew"], "");
    assert_eq!(o.status.code(), Some(2), "{}", text(&o.stderr));
    let e = text(&o.stderr);
    assert!(e.contains("--routine") && e.contains("--compromised") && e.contains("no default"), "{e}");
    assert!(empty(&home));
}
