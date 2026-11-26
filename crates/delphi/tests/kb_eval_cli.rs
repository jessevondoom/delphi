//! `delphi kb eval` and the golden set at the command line (`specs/140`): building and accepting the set are the operator's, refused
//! inside an agent seat before anything is read or written (AC-3).

use std::process::{Command, Output, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

/// Run `delphi <args> --home <an empty home>` with no terminal on its input, and say whether the home was written to.
fn run(args: &[&str], env: &[(&str, &str)]) -> (Output, bool) {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let mut cmd = Command::new(DELPHI);
    cmd.args(args).arg("--home").arg(&home).env_remove("DELPHI_WORKSPACE").env_remove("DELPHI_BRIDGE_SOCKET").env_remove("DELPHI_BRIDGE_TOKEN").stdin(Stdio::null()).current_dir(t.path());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let touched = std::fs::read_dir(&home).unwrap().next().is_some();
    (out, touched)
}

#[test]
fn build_and_accept_are_refused_inside_an_agent_seat() {
    for seat in [("DELPHI_BRIDGE_SOCKET", "/tmp/x.sock"), ("DELPHI_BRIDGE_TOKEN", "t")] {
        for args in [&["kb", "eval", "build", "--real-only"][..], &["kb", "eval", "accept"]] {
            let (o, touched) = run(args, &[seat]);
            assert_eq!(o.status.code(), Some(2), "{args:?}");
            let err = String::from_utf8_lossy(&o.stderr);
            assert!(err.contains("refused") && err.contains("agent seat"), "{args:?}: {err}");
            assert!(!touched, "{args:?} wrote to the home");
        }
    }
}
