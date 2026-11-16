//! `delphi library assets check [--apply]` (`specs/118` §2 item 3): the pictures of the library that have no licence record.
//!
//! Listing reads only. `--apply` moves each such picture to its source's `assets/unlicensed/`, which is never served, and is the
//! operator's: it is refused inside an agent seat. Nothing is deleted.

use delphi_engine::library::Library;
use serde_json::json;
use std::path::Path;

const USAGE: &str = "usage:
  delphi library assets check [--apply] [--json] [--home <dir>]";

pub fn cli(args: &[String], home: &Path) -> i32 {
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("assets"), Some("check")) => check(&Library::in_home(home), &args[2..]),
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

fn check(lib: &Library, args: &[String]) -> i32 {
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--apply" | "--json" => {}
            "--home" => i += 1,
            other => {
                eprintln!("delphi library assets check: unknown argument `{}`\n{}", other, USAGE);
                return 2;
            }
        }
        i += 1;
    }
    let apply = args.iter().any(|a| a == "--apply");
    if apply && crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi library assets check --apply: refused; moving the library's pictures is the operator's and this process was started by an agent seat");
        return 2;
    }
    let found = lib.assets_check(apply);
    if args.iter().any(|a| a == "--json") {
        let rows: Vec<_> = found.iter().map(|u| json!({"source_id": u.source_id, "file": u.file, "moved": u.moved})).collect();
        println!("{}", json!({"unrecorded": rows, "applied": apply}));
        return 0;
    }
    if found.is_empty() {
        println!("every picture in the library has a licence record");
        return 0;
    }
    println!("{} picture(s) with no licence record (not served to a page, the console or the app):", found.len());
    for u in &found {
        println!("  s-{}/{}{}", u.source_id, u.file, if u.moved { "  moved to assets/unlicensed/" } else { "" });
    }
    if !apply {
        println!("`delphi library assets check --apply` moves them to assets/unlicensed/, which is never served; nothing is deleted");
    }
    0
}
