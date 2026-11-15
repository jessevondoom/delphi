//! `delphi app` at the command line (`specs/120` slice K2): help touches nothing, and every form refuses when its input is
//! not a terminal or when it runs inside an agent seat, before a daemon is started or a ticket is asked for (AC-2).

use std::path::Path;
use std::process::{Command, Output, Stdio};

const DELPHI: &str = env!("CARGO_BIN_EXE_delphi");

/// Run `delphi <args> --home <an empty home>` with no terminal on its input, and say whether the home was written to.
fn run(args: &[&str], env: &[(&str, &str)]) -> (Output, bool) {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let mut cmd = Command::new(DELPHI);
    cmd.args(args).arg("--home").arg(&home).env_remove("DELPHI_WORKSPACE").env_remove("DELPHI_BRIDGE_SOCKET").env_remove("DELPHI_BRIDGE_TOKEN").env_remove("DELPHI_APP").stdin(Stdio::null()).current_dir(t.path());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let touched = std::fs::read_dir(&home).unwrap().next().is_some();
    (out, touched)
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).to_string()
}

#[test]
fn help_is_one_short_page_and_touches_nothing() {
    for args in [&["app", "--help"][..], &["help", "app"]] {
        let (o, touched) = run(args, &[]);
        assert_eq!(o.status.code(), Some(0), "{args:?}");
        let out = text(&o.stdout);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "usage:", "{out}");
        assert!(lines[1].starts_with("  delphi app"), "{out}");
        assert!(lines.len() < 12 && lines.iter().all(|l| l.chars().count() < 120), "{out}");
        for word in ["list", "log", "revoke <name>", "grant <name> [--remove] <grant>", "terminal", "DELPHI_APP"] {
            assert!(out.contains(word), "the page names {word}: {out}");
        }
        assert!(!touched, "{args:?} wrote to the home");
    }
}

/// AC-2: with its input not a terminal, `delphi app` refuses. No daemon is started (nothing is written under the home, so
/// there is no socket to ask), so no ticket is issued. The same for every other form.
#[test]
fn every_form_refuses_when_its_input_is_not_a_terminal_and_no_ticket_is_issued() {
    for args in [&["app"][..], &["app", "list"], &["app", "log"], &["app", "revoke", "app-0a1b2c"], &["app", "grant", "app-0a1b2c", "request"], &["app", "grant", "app-0a1b2c", "--remove", "request"]] {
        let (o, touched) = run(args, &[]);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", text(&o.stderr));
        let err = text(&o.stderr);
        assert!(err.starts_with("delphi app: refused;") && err.contains("terminal"), "{args:?}: {err}");
        assert!(o.stdout.is_empty(), "{args:?} printed {}", text(&o.stdout));
        assert!(!touched, "{args:?} wrote to the home: a refusal starts no daemon");
    }
}

/// §8: no agent can pair, grant or revoke. Inside an agent seat every form is refused, whatever its input is.
#[test]
fn every_form_refuses_inside_an_agent_seat() {
    for seat in [("DELPHI_BRIDGE_SOCKET", "/tmp/x.sock"), ("DELPHI_BRIDGE_TOKEN", "t")] {
        for args in [&["app"][..], &["app", "list"], &["app", "log"], &["app", "revoke", "app-0a1b2c"], &["app", "grant", "app-0a1b2c", "request"]] {
            let (o, touched) = run(args, &[seat]);
            assert_eq!(o.status.code(), Some(2), "{args:?}");
            let err = text(&o.stderr);
            assert!(err.contains("refused") && err.contains("agent seat"), "{args:?}: {err}");
            assert!(!touched, "{args:?} wrote to the home");
        }
    }
}

/// The command starts the app with `Command::new` in one place only, and that place is compiled for tests until the
/// spawn-site list names it (see `app_cmd.rs`). This holds the two together: when the `cfg(test)` is removed, the list must
/// have the entry, and the spawn-site ratchet (`spawn_sites.rs`) says so; until then no shipped build starts a process here.
#[test]
fn starting_the_app_stays_switched_off_until_the_spawn_site_is_listed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = std::fs::read_to_string(root.join("crates/delphi/src/app_cmd.rs")).unwrap();
    let listed = std::fs::read_to_string(root.join("crates/delphi/tests/spawn_sites.toml")).unwrap().contains("crates/delphi/src/app_cmd.rs");
    let switched_off = src.contains("pub(crate) const START_ENABLED: bool = cfg!(test);") && src.contains("#[cfg(test)]\nfn start_with_ticket(");
    assert!(listed != switched_off, "either the spawn-site list names app_cmd.rs and starting is switched on, or neither (listed: {listed}, switched off: {switched_off})");
}
