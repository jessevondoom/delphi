//! `delphi research log <id>`: the search log of a finished host research job (`specs/101` §3). Read-only: it prints what the daemon
//! wrote beside the result when `[research] search_log` was on, and calls no model and no network.

use delphi_engine::search_log;
use std::path::Path;

pub struct Options {
    pub json: bool,
}

const USAGE: &str = "usage: delphi research log <research-id> [--json] [--home <dir>] [--workspace <name>]";

/// `args` is what follows the command name: `log <id> [flags]`.
pub fn cli(ws_dir: &Path, args: &[String], o: &Options) -> i32 {
    if args.first().map(String::as_str) != Some("log") {
        eprintln!("{USAGE}");
        return 2;
    }
    // The id is the first word after `log` that is not a flag or a flag's value.
    let mut id = None;
    let mut skip = false;
    for a in &args[1..] {
        if skip {
            skip = false;
        } else if matches!(a.as_str(), "--home" | "--workspace") {
            skip = true;
        } else if !a.starts_with('-') {
            id = Some(a.as_str());
            break;
        }
    }
    let Some(id) = id else {
        eprintln!("{USAGE}");
        return 2;
    };
    match search_log::read(ws_dir, id) {
        Ok(log) if o.json => {
            println!("{}", serde_json::to_string_pretty(&log).unwrap_or_else(|_| "{}".into()));
            0
        }
        Ok(log) => {
            print!("{}", search_log::render(&log, id));
            0
        }
        Err(e) => {
            eprintln!("delphi research log: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strings(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn it_prints_a_kept_log_and_refuses_what_it_cannot_find_or_parse() {
        let d = tempfile::tempdir().unwrap();
        let log = json!({"version": 1, "private": false, "lines": [], "flow": {"found": 0, "duplicates_removed": 0, "kept": 0, "not_kept": {}, "searches": 0, "withheld": 0, "declared": 0}});
        search_log::persist(d.path(), "r-t-1", &log).unwrap();
        assert_eq!(cli(d.path(), &strings(&["log", "r-t-1"]), &Options { json: false }), 0);
        assert_eq!(cli(d.path(), &strings(&["log", "--home", "/x", "r-t-1"]), &Options { json: true }), 0, "a flag's value is not the id");
        assert_eq!(cli(d.path(), &strings(&["log", "r-none"]), &Options { json: false }), 1, "no such log");
        assert_eq!(cli(d.path(), &strings(&["log", "../etc/passwd"]), &Options { json: false }), 1, "only a clean id names a file");
        assert_eq!(cli(d.path(), &strings(&["log"]), &Options { json: false }), 2);
        assert_eq!(cli(d.path(), &strings(&["show", "r-t-1"]), &Options { json: false }), 2);
    }
}
