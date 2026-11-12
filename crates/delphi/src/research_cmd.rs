//! `delphi research log <id>` and `delphi research retractions import|status`.
//!
//! `log` prints the search log of a finished host research job (`specs/101` §3): read-only, it prints what the daemon wrote beside the
//! result when `[research] search_log` was on, and calls no model and no network.
//!
//! `retractions` is the local Retraction Watch snapshot (`specs/101` §4). `import <file>` reads a file from disk, builds the local index,
//! and, when `[research] retractions` is on, checks every source in the library against it. **It never calls a service**, and takes a path,
//! not a URL: downloading the file is the operator's act (the repository is `gitlab.com/crossref/retraction-watch-data`).

use delphi_engine::search_log;
use std::path::Path;

pub struct Options {
    pub json: bool,
}

const USAGE: &str = "usage: delphi research log <research-id> [--json] [--home <dir>] [--workspace <name>]\n       delphi research retractions import <file> [--home <dir>] [--workspace <name>]\n       delphi research retractions status [--home <dir>] [--workspace <name>]";

/// `args` is what follows the command name: `log <id> [flags]` or `retractions import|status [<file>] [flags]`.
pub fn cli(home: &Path, ws_dir: &Path, args: &[String], o: &Options) -> i32 {
    match args.first().map(String::as_str) {
        Some("log") => log(ws_dir, args, o),
        Some("retractions") => retractions(home, ws_dir, args, o),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

/// The first word after the subcommand words that is not a flag or a flag's value.
fn operand(args: &[String], after: usize) -> Option<&str> {
    let mut skip = false;
    for a in &args[after.min(args.len())..] {
        if skip {
            skip = false;
        } else if matches!(a.as_str(), "--home" | "--workspace") {
            skip = true;
        } else if !a.starts_with('-') {
            return Some(a.as_str());
        }
    }
    None
}

fn log(ws_dir: &Path, args: &[String], o: &Options) -> i32 {
    let Some(id) = operand(args, 1) else {
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

/// `[research]` settings from the workspace's `config.toml`; the defaults when it cannot be read.
fn settings(ws_dir: &Path) -> delphi_engine::engine::Settings {
    let text = std::fs::read_to_string(ws_dir.join("config.toml")).unwrap_or_default();
    let root: toml::Value = toml::from_str(&text).unwrap_or_else(|_| toml::Value::Table(Default::default()));
    delphi_engine::boot::settings_from(&root)
}

fn retractions(home: &Path, ws_dir: &Path, args: &[String], o: &Options) -> i32 {
    use delphi_engine::{library::Library, retractions as rw};
    let lib_root = home.join("library");
    let s = settings(ws_dir);
    let now = delphi_core::time::now_rfc3339();
    match args.get(1).map(String::as_str) {
        Some("status") => {
            let policy = rw::Policy::load(&lib_root, s.retractions);
            if o.json {
                let st = rw::load(&lib_root);
                println!(
                    "{}",
                    serde_json::json!({"switch": s.retractions, "line": policy.status_line(&now, s.retractions_stale_days), "snapshot": st.as_ref().map(|i| serde_json::json!({"version": i.version, "imported": i.imported, "rows": i.rows, "works": i.len(), "stale": i.is_stale(&now, s.retractions_stale_days)}))})
                );
            } else {
                print!("{}", rw::render_status(&lib_root, &now, s.retractions_stale_days));
                println!("{}", policy.status_line(&now, s.retractions_stale_days));
                if !s.retractions {
                    println!("[research] retractions is off: sources are not checked (turn it on in config.toml)");
                }
            }
            0
        }
        Some("import") => {
            let Some(file) = operand(args, 2) else {
                eprintln!("{USAGE}");
                return 2;
            };
            if file.starts_with("http://") || file.starts_with("https://") {
                eprintln!("delphi research retractions import: give a file on disk, not a URL. Download the Retraction Watch CSV yourself (the repository is gitlab.com/crossref/retraction-watch-data); Delphi makes no call to fetch it.");
                return 2;
            }
            let bytes = match std::fs::read(file) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("delphi research retractions import: {file}: {e}");
                    return 1;
                }
            };
            let index = match rw::import(&lib_root, &bytes, &now) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("delphi research retractions import: {e}");
                    return 1;
                }
            };
            println!("imported snapshot {}: {} data rows, {} works with a standing notice; skipped {} rows with an unknown nature and {} with no usable DOI; {} notice(s) kept because a date could not be read", index.version, index.rows, index.len(), index.unknown_nature, index.no_doi, index.unresolved);
            if s.retractions {
                let applied = Library::in_home(home).with_retractions(rw::Policy::with_index(index)).check_all_retractions();
                println!("checked {} source(s): {} marked, {} lifted, {} already marked, {} with no DOI to match", applied.checked, applied.marked, applied.lifted, applied.unchanged, applied.no_identifier);
            } else {
                println!("[research] retractions is off: the snapshot is stored and no source was checked or marked");
            }
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
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
        let log = json!({"version": 1, "private": false, "lines": [], "flow": {"found": 0, "duplicates_removed": 0, "kept": 0, "not_kept": {}, "searches": 0, "withheld": 0}});
        search_log::persist(d.path(), "r-t-1", &log).unwrap();
        let h = d.path();
        assert_eq!(cli(h, d.path(), &strings(&["log", "r-t-1"]), &Options { json: false }), 0);
        assert_eq!(cli(h, d.path(), &strings(&["log", "--home", "/x", "r-t-1"]), &Options { json: true }), 0, "a flag's value is not the id");
        assert_eq!(cli(h, d.path(), &strings(&["log", "r-none"]), &Options { json: false }), 1, "no such log");
        assert_eq!(cli(h, d.path(), &strings(&["log", "../etc/passwd"]), &Options { json: false }), 1, "only a clean id names a file");
        assert_eq!(cli(h, d.path(), &strings(&["log"]), &Options { json: false }), 2);
        assert_eq!(cli(h, d.path(), &strings(&["show", "r-t-1"]), &Options { json: false }), 2);
    }

    #[test]
    fn it_imports_a_file_checks_nothing_while_the_switch_is_off_and_refuses_a_url_or_a_wrong_file() {
        let home = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let csv = home.path().join("rw.csv");
        std::fs::write(&csv, "Record ID,Title,OriginalPaperDOI,RetractionNature,RetractionDate\n1,t,10.1001/aaaa,Retraction,2021-03-04\n").unwrap();
        let o = Options { json: false };
        let run = |a: &[&str]| cli(home.path(), ws.path(), &strings(a), &o);
        assert_eq!(run(&["retractions", "import", csv.to_str().unwrap()]), 0);
        assert!(delphi_engine::retractions::load(&home.path().join("library")).unwrap().lookup("doi:10.1001/AAAA").is_some());
        assert_eq!(run(&["retractions", "status"]), 0);
        assert_eq!(run(&["retractions", "import", "https://example.com/rw.csv"]), 2, "a URL is refused: Delphi makes no call to fetch it");
        assert_eq!(run(&["retractions", "import", "/no/such/file.csv"]), 1);
        let bad = home.path().join("bad.csv");
        std::fs::write(&bad, "a,b\n1,2\n").unwrap();
        assert_eq!(run(&["retractions", "import", bad.to_str().unwrap()]), 1, "a file without the columns is refused");
        assert_eq!(run(&["retractions", "import"]), 2);
        assert_eq!(run(&["retractions"]), 2);
    }

    #[test]
    fn with_the_switch_on_an_import_marks_the_sources_it_finds() {
        use delphi_engine::library::{AddOpts, Budgets, Library};
        let home = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(ws.path().join("config.toml"), "[research]\nretractions = true\n").unwrap();
        let page = home.path().join("p.md");
        std::fs::write(&page, "# p\n\nText.\n").unwrap();
        let l = Library::in_home(home.path());
        let id = l.add_file(&page, &AddOpts { licence: Some("CC0-1.0".into()), doi: Some("10.1001/aaaa".into()), ..AddOpts::default() }, &Budgets::default()).unwrap().id;
        assert!(l.read_meta(&id).unwrap()["retraction"].is_null());
        let csv = home.path().join("rw.csv");
        std::fs::write(&csv, "Record ID,Title,OriginalPaperDOI,RetractionNature,RetractionDate\n1,t,10.1001/aaaa,Retraction,2021-03-04\n").unwrap();
        assert_eq!(cli(home.path(), ws.path(), &strings(&["retractions", "import", csv.to_str().unwrap()]), &Options { json: false }), 0);
        assert_eq!(l.read_meta(&id).unwrap()["retraction"]["kind"], "retracted");
    }
}
