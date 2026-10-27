//! `delphi bundle trust`, `delphi bundle trust-source` and the `--trust` / `--trust-source` flags of `add`
//! (`specs/024` §3.7, §3.8).
//!
//! Raising trust is a human act: it needs a terminal on stdin AND stdout and the operator typing the name back. There is no
//! `--yes`, no environment variable and no pipe that stands in for it; a process an agent starts has no terminal and is
//! refused. Lowering trust (`--level default`, `--revoke`) needs no confirmation. The logic is in `delphi_core::bundle_trust`,
//! which the console's `org.trust` calls with its own typed confirmation.

use crate::bundle_cmd::flag;
use delphi_core::bundle_trust::{self as core, Challenge, Req};
use delphi_core::bundles;
use std::path::Path;

/// The real confirmation: a terminal on both ends, and the typed word.
pub fn tty_confirm(c: &Challenge) -> Result<bool, String> {
    if !on_terminal() {
        return Err("raising trust is a human act: run this in a terminal (it has no non-interactive mode, no --yes and no environment switch)".into());
    }
    println!("{}", c.text);
    print!("type {} to confirm, anything else cancels: ", c.what);
    use std::io::{BufRead, Write};
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim() == c.expect)
}

pub fn on_terminal() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

fn go(ws: &Path, req: &Req, by: &str, confirm: &dyn Fn(&Challenge) -> Result<bool, String>) -> i32 {
    let lock = match bundles::read(ws) {
        Ok(l) => l,
        Err(e) => return crate::bundle_add::fail(e),
    };
    match core::prepare(&lock, req) {
        Err(e) => return crate::bundle_add::fail(e),
        Ok(Some(c)) => match confirm(&c) {
            Err(e) => return crate::bundle_add::fail(e),
            Ok(false) => {
                println!("cancelled: nothing changed");
                return 1;
            }
            Ok(true) => {}
        },
        Ok(None) => {}
    }
    match core::apply(ws, req, by) {
        Ok(msg) => {
            println!("{}", msg);
            0
        }
        Err(e) => crate::bundle_add::fail(e),
    }
}

/// `delphi bundle trust <bundle> --level user_ratified|default`.
pub fn trust(ws: &Path, args: &[String], positional: &[&String], confirm: &dyn Fn(&Challenge) -> Result<bool, String>) -> i32 {
    let (Some(name), Some(level)) = (positional.get(1), flag(args, "--level")) else {
        eprintln!("usage: delphi bundle trust <bundle> --level user_ratified|default");
        return 2;
    };
    match level.as_str() {
        "user_ratified" => go(ws, &Req::Raise { name: name.to_string() }, "cli", confirm),
        "default" => go(ws, &Req::Lower { name: name.to_string() }, "cli", confirm),
        other => {
            eprintln!("delphi bundle trust: --level is user_ratified or default, not `{}`", other);
            2
        }
    }
}

/// `delphi bundle trust-source <bundle|origin> [--level user_ratified|default] [--revoke]`.
pub fn trust_source(ws: &Path, args: &[String], positional: &[&String], confirm: &dyn Fn(&Challenge) -> Result<bool, String>) -> i32 {
    let Some(target) = positional.get(1) else {
        eprintln!("usage: delphi bundle trust-source <bundle|origin> [--level user_ratified|default] [--revoke]");
        return 2;
    };
    if args.iter().any(|a| a == "--revoke") {
        return go(ws, &Req::Revoke { target: target.to_string() }, "cli", confirm);
    }
    let level = flag(args, "--level").unwrap_or_else(|| "default".into());
    go(ws, &Req::Grant { target: target.to_string(), level }, "cli", confirm)
}

/// What `add` was asked to do about trust, checked before anything is added so that a refusal leaves nothing behind.
#[derive(Debug)]
pub struct AtAdd {
    pub raise: bool,
    pub source: bool,
}

pub fn at_add(args: &[String]) -> Result<AtAdd, String> {
    let raise = match flag(args, "--trust").as_deref() {
        None => false,
        Some("ratified") | Some("user_ratified") => true,
        Some(other) => return Err(format!("--trust is `ratified`, not `{}`", other)),
    };
    let source = args.iter().any(|a| a == "--trust-source");
    if (raise || source) && !on_terminal() {
        return Err("--trust and --trust-source need you at a terminal to confirm; nothing was added".into());
    }
    Ok(AtAdd { raise, source })
}

/// After a successful add: confirm and apply what the flags asked for, on the bundle that was just added. A cancelled or
/// refused confirmation leaves the bundle added at the default tier and says so.
pub fn after_add(ws: &Path, started: &str, want: &AtAdd, confirm: &dyn Fn(&Challenge) -> Result<bool, String>) -> i32 {
    let Ok(lock) = bundles::read(ws) else { return 1 };
    let Some(e) = lock.bundle.iter().filter(|e| e.is_active() && e.added_at.as_str() >= started).max_by(|a, b| a.added_at.cmp(&b.added_at)) else { return 0 };
    let id = e.id();
    let mut rc = 0;
    if want.source {
        let level = if want.raise { "user_ratified" } else { "default" };
        rc = go(ws, &Req::Grant { target: id.clone(), level: level.into() }, "at-add", confirm);
    } else if want.raise {
        rc = go(ws, &Req::Raise { name: id.clone() }, "at-add", confirm);
    }
    if rc != 0 {
        println!("{} stays added at the default tier (validated bundle source); `delphi bundle trust {}` raises it later", id, id);
    }
    rc
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::bundles::{Entry, Lock, Source};
    use std::collections::BTreeMap;

    fn ws_with(kid: &str) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("local")).unwrap();
        let e = Entry {
            org: "acme".into(), name: "plat".into(), version: "1.0.0".into(), hash: "blake3:ab".into(), publisher_kid: kid.into(), provenance: "signed".into(),
            source: Source { kind: "directory".into(), path: "/x".into() }, added_at: "2026-10-27T00:00:00Z".into(), order: 10, status: "active".into(),
            review_by: String::new(), title: "T".into(), skill_roles: BTreeMap::new(), history: vec![], inert_servers: vec![],
            trust: String::new(), trust_hash: String::new(), trust_kid: String::new(), trust_log: vec![],
        };
        bundles::write(t.path(), &Lock { bundle: vec![e], ..Default::default() }).unwrap();
        t
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn run(ws: &Path, v: &[&str], confirm: &dyn Fn(&Challenge) -> Result<bool, String>) -> i32 {
        let a = args(v);
        let pos: Vec<&String> = a.iter().filter(|s| !s.starts_with("--") && s.as_str() != "user_ratified" && s.as_str() != "default").collect();
        if v[0] == "trust" { trust(ws, &a[1..], &pos, confirm) } else { trust_source(ws, &a[1..], &pos, confirm) }
    }

    fn typed(word: &'static str) -> impl Fn(&Challenge) -> Result<bool, String> {
        move |c| Ok(c.expect == word)
    }

    #[test]
    fn the_real_confirmation_refuses_without_a_terminal_and_so_do_the_flags() {
        // `cargo test` has no terminal on stdin: this is the agent's situation too.
        let ws = ws_with("did:key:zA");
        let c = Challenge { text: "t".into(), expect: "acme/plat".into(), what: "x".into() };
        assert!(tty_confirm(&c).unwrap_err().contains("human act"));
        assert_eq!(run(ws.path(), &["trust", "plat", "--level", "user_ratified"], &tty_confirm), 1);
        assert!(!bundles::read(ws.path()).unwrap().bundle[0].raised(), "an attempt without a terminal changes nothing");
        assert_eq!(run(ws.path(), &["trust-source", "plat", "--level", "user_ratified"], &tty_confirm), 1);
        assert!(bundles::read(ws.path()).unwrap().trusted_source.is_empty());
        assert!(at_add(&args(&["--trust", "ratified"])).unwrap_err().contains("terminal"));
        assert!(at_add(&args(&["--trust-source"])).is_err() && at_add(&args(&["--trust", "bogus"])).is_err());
        assert!(at_add(&args(&["--yes"])).is_ok(), "--yes alone is an ordinary add; it never stands in for a confirmation");
    }

    #[test]
    fn the_wrong_word_cancels_and_the_right_one_raises_and_default_lowers_without_asking() {
        let ws = ws_with("did:key:zA");
        assert_eq!(run(ws.path(), &["trust", "plat", "--level", "user_ratified"], &typed("nope")), 1);
        assert!(!bundles::read(ws.path()).unwrap().bundle[0].raised());
        assert_eq!(run(ws.path(), &["trust", "plat", "--level", "user_ratified"], &typed("acme/plat")), 0);
        assert!(bundles::read(ws.path()).unwrap().bundle[0].raised());
        let never = |_: &Challenge| -> Result<bool, String> { panic!("lowering must not ask") };
        assert_eq!(run(ws.path(), &["trust", "plat", "--level", "default"], &never), 0);
        assert!(!bundles::read(ws.path()).unwrap().bundle[0].raised());
    }

    #[test]
    fn trust_source_grants_on_the_typed_origin_and_revokes_without_asking() {
        let ws = ws_with("did:key:zA");
        assert_eq!(run(ws.path(), &["trust-source", "plat", "--level", "user_ratified"], &typed("acme/plat")), 1, "the source id is the origin, not the bundle name");
        assert_eq!(run(ws.path(), &["trust-source", "plat", "--level", "user_ratified"], &typed("directory:/x")), 0);
        let l = bundles::read(ws.path()).unwrap();
        assert!(l.trusted_source.len() == 1 && l.bundle[0].raised() && l.bundle[0].raised_by() == Some("trusted-source"));
        let never = |_: &Challenge| -> Result<bool, String> { panic!("revoking must not ask") };
        assert_eq!(run(ws.path(), &["trust-source", "plat", "--revoke"], &never), 0);
        let l = bundles::read(ws.path()).unwrap();
        assert!(l.trusted_source.is_empty() && !l.bundle[0].raised());
    }

    #[test]
    fn an_unsigned_bundle_cannot_anchor_a_trusted_source() {
        let ws = ws_with("unsigned");
        assert_eq!(run(ws.path(), &["trust-source", "plat"], &typed("directory:/x")), 1);
        assert!(bundles::read(ws.path()).unwrap().trusted_source.is_empty());
    }
}
