//! `delphi handling` (`specs/121` §3 items 3, 5 and 7): the operator's view of the handling traits (shown as private, internal and
//! confidential), the operator's set and clear, and the reconciliation.
//!
//! `set` and `clear` are the operator's alone: both are refused inside an agent seat and without a terminal. `clear` declassifies
//! and is the one caller of `Handling::operator_clear` (a test in `delphi_engine::handling` enumerates the callers); `set` gives an
//! `internal` the research weight of §7, which no agent or host may give.

use std::io::IsTerminal;
use std::path::Path;

use delphi_engine::handling::{self, Handling, Trait};
use delphi_engine::handling_reconcile;

pub const USAGE: &str = "usage: delphi handling status [--json]\n       delphi handling set <item> <private|internal|confidential>\n       delphi handling clear <item> <private|internal|confidential>\n       delphi handling reconcile [--dry-run | --apply] [--json]\n(each takes [--workspace <name>] [--home <dir>])";

/// The marker of a reconciliation that was applied: until there is one, `reconcile` with no flag is a dry run (§3 item 7).
const APPLIED: &str = "reconciled";

fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if matches!(a.as_str(), "--workspace" | "--home") {
            skip = true;
            continue;
        }
        if !a.starts_with("--") {
            out.push(a.as_str());
        }
    }
    out
}

/// An item as the operator names it: an id (`local/summaries/x.md`, `library/claims/<id>`), a library pointer
/// (`@library/claim/<id>`), or a path to a file inside the workspace.
pub fn item_named(ws_dir: &Path, name: &str) -> Option<String> {
    let name = name.trim();
    if let Some(i) = handling::item_of("", name) {
        return Some(i);
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return handling::item_in_ws(ws_dir, &std::fs::canonicalize(p).ok()?).or_else(|| handling::item_in_ws(ws_dir, p));
    }
    let item = name.trim_start_matches("./").to_string();
    handling::item_ok(&item).then_some(item)
}

fn refused_unless_operator(cmd: &str) -> Option<i32> {
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi handling {cmd}: refused; handling traits are set and cleared by the operator, and this process was started by an agent seat");
        return Some(2);
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("delphi handling {cmd}: refused; run it yourself at a terminal (this process has none)");
        return Some(2);
    }
    None
}

pub fn run(args: &[String], home: &Path, ws_dir: &Path) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let pos = positional(args);
    match pos.first().copied() {
        Some("status") => match handling_reconcile::status(ws_dir, home) {
            Ok(r) => {
                if json {
                    println!("{}", r.to_json());
                } else {
                    println!("handling (what would be labelled if the rules ran now; nothing was changed):");
                    for l in r.lines().iter().filter(|l| !l.starts_with("run `")).map(|l| l.trim_start_matches("dry run: nothing was changed. ")) {
                        println!("  {l}");
                    }
                    for bad in Handling::of(ws_dir, home).unreadable() {
                        println!("  the record {} cannot be read", bad.display());
                    }
                }
                0
            }
            Err(e) => {
                eprintln!("delphi handling status: {e}");
                1
            }
        },
        Some(cmd @ ("set" | "clear")) => {
            if let Some(code) = refused_unless_operator(cmd) {
                return code;
            }
            let (Some(name), Some(t)) = (pos.get(1), pos.get(2)) else {
                eprintln!("{USAGE}");
                return 2;
            };
            let Some(t) = Trait::parse(t) else {
                eprintln!("delphi handling {cmd}: `{t}` is not a trait (private, internal or confidential)");
                return 2;
            };
            let Some(item) = item_named(ws_dir, name) else {
                eprintln!("delphi handling {cmd}: `{name}` names no item (an id such as local/summaries/x.md or library/claims/<id>, or a file in the workspace)");
                return 2;
            };
            let h = Handling::of(ws_dir, home);
            let done = if cmd == "set" { h.operator_set(&item, t).map(|_| ()) } else { h.operator_clear(&item, t).map(|_| ()) };
            match done {
                Ok(()) => {
                    let now = h.set(&item);
                    println!("{item}: {}", if now.is_empty() { "unlabelled".to_string() } else { handling::shown(&now) });
                    if cmd == "clear" {
                        println!("records derived from it lose the trait at the next `delphi handling reconcile`");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("delphi handling {cmd}: {e}");
                    1
                }
            }
        }
        Some("reconcile") => {
            let marker = handling::dir(ws_dir).join(APPLIED);
            let dry = args.iter().any(|a| a == "--dry-run") || (!args.iter().any(|a| a == "--apply") && !marker.exists());
            match handling_reconcile::reconcile(ws_dir, home, dry) {
                Ok(r) => {
                    if !dry {
                        let _ = std::fs::write(&marker, delphi_core::time::now_rfc3339());
                    }
                    if json {
                        println!("{}", r.to_json());
                    } else {
                        if dry && !args.iter().any(|a| a == "--dry-run") {
                            println!("the first reconciliation is a dry run: read the counts, then run it with --apply");
                        }
                        for l in r.lines() {
                            println!("{l}");
                        }
                    }
                    0
                }
                Err(e) => {
                    eprintln!("delphi handling reconcile: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}
