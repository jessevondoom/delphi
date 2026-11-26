//! `delphi handling` (`specs/121` §3 items 3, 5 and 7): the operator's view of the handling traits (shown as private, internal and
//! confidential), the operator's set and clear, the reconciliation, and `relabel` (`specs/141` §2.1 item 4).
//!
//! `set` and `clear` are the operator's alone: both are refused inside an agent seat and without a terminal. `clear` declassifies
//! and is the one caller of `Handling::operator_clear` (a test in `delphi_engine::handling` enumerates the callers); `set` gives an
//! `internal` the research weight of §7, which no agent or host may give.

use std::io::IsTerminal;
use std::path::Path;

use delphi_engine::handling::{self, Handling, Trait};
use delphi_engine::handling_reconcile;

pub const USAGE: &str = "usage: delphi handling status [--json]\n       delphi handling set <item> <private|internal|confidential>\n       delphi handling clear <item> <private|internal|confidential>\n       delphi handling reconcile [--dry-run | --apply] [--json]\n       delphi handling relabel [--json]\n(each takes [--workspace <name>] [--home <dir>])";

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
        Some("relabel") => relabel(home, ws_dir, json),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

/// `delphi handling relabel` (`specs/141` §2.1 item 4): recompute the label of every page from its recorded material and of every row
/// of the index, from the daemon's records. No model is called. A page from before labels, a stale page and a row with no `marks` read
/// as every mark at every sink until this runs (or the page is rebuilt, or the index synced). It computes; it lowers nothing below
/// what the records say.
fn relabel(home: &Path, ws_dir: &Path, json: bool) -> i32 {
    let db = ws_dir.join("local/state.db");
    let mut kb = if db.is_file() { delphi_engine::kb::Kb::open(&db).ok() } else { None };
    match delphi_engine::label::relabel(home, ws_dir, kb.as_mut()) {
        Ok(r) => {
            if json {
                println!("{}", r.to_json());
            } else {
                println!("pages: {} read, {} relabelled", r.pages, r.pages_changed);
                match kb {
                    Some(_) => println!("index rows: {} read, {} relabelled", r.rows, r.rows_changed),
                    None => println!("index rows: no index yet (it is labelled when it is first built)"),
                }
            }
            0
        }
        Err(e) => {
            eprintln!("delphi handling relabel: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-19's command: `relabel` gives a page from before labels its label from its material, with the view and the visible line; a
    /// second run changes nothing.
    #[test]
    fn relabel_gives_a_page_from_before_labels_its_label_and_a_second_run_changes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let ws = home.join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let pages = home.join("library/pages");
        std::fs::create_dir_all(&pages).unwrap();
        // a page built before labels: its sidecar lists a passage of an imported page the operator marked internal, and no label
        std::fs::write(pages.join("tides.md"), "---\ntitle: Tides\n---\n\n# Tides\n\nText.\n").unwrap();
        let side = serde_json::json!({"kind": "topic_page", "title": "Tides", "records": [{"id": "s-kb-0000000000000001", "kind": "passage", "rel_path": "summaries/notes.md", "origin": "summaries (external_untrusted)"}], "material": [{"id": "s-kb-0000000000000001", "hash": "h"}]});
        std::fs::write(pages.join("tides.json"), side.to_string()).unwrap();
        Handling::of(&ws, home).operator_set("local/summaries/notes.md", Trait::Internal).unwrap();
        assert_eq!(relabel(home, &ws, true), 0);
        let side: serde_json::Value = serde_json::from_slice(&std::fs::read(pages.join("tides.json")).unwrap()).unwrap();
        assert_eq!(side["label"]["marks"], serde_json::json!(["internal"]));
        let md = std::fs::read_to_string(pages.join("tides.md")).unwrap();
        assert!(md.contains("handling: [internal]\n") && md.contains("> Handling: INTERNAL\n"), "{md}");
        let r = delphi_engine::label::relabel(home, &ws, None).unwrap();
        assert_eq!((r.pages, r.pages_changed), (1, 0));
    }
}
