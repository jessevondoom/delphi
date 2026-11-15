//! `delphi research topics`, `delphi research show <topic-id>`, `delphi research log <id>` and `delphi research retractions import|status`.
//!
//! `topics` lists the research records on disk and `show` prints one (`specs/112` §2, §5.3): read-only, from the files a run left in
//! `research/topics/<topic-id>/` of the workspace or of a registered project. Record text is data and is printed as such.
//!
//! `migrate` writes the claims an older `research/host-claims.jsonl` lists as library claims (`specs/112` §3.4). It is a dry run
//! unless `--apply` is given, and `--apply` is refused inside an agent seat.
//!
//! `log` prints the search log of a finished host research job (`specs/101` §3): read-only, it prints what the daemon wrote beside the
//! result, and calls no model and no network.
//!
//! `retractions` is the local Retraction Watch snapshot (`specs/101` §4). `import <file>` reads a file from disk, builds the local index,
//! and, when `[research] retractions` is on, checks every source in the library against it. **It never calls a service**, and takes a path,
//! not a URL: downloading the file is the operator's act (the repository is `gitlab.com/crossref/retraction-watch-data`).

use delphi_engine::{research_record, search_log};
use std::path::Path;

pub struct Options {
    pub json: bool,
}

const USAGE: &str = "usage: delphi research topics [--json] [--home <dir>] [--workspace <name>]\n       delphi research migrate [--apply] [--json] [--home <dir>] [--workspace <name>]\n       delphi research show <topic-id> [--json] [--home <dir>] [--workspace <name>]\n       delphi research log <topic-id | research-id> [--json] [--home <dir>] [--workspace <name>]\n       delphi research retractions import <file> [--home <dir>] [--workspace <name>]\n       delphi research retractions status [--home <dir>] [--workspace <name>]";

/// `args` is what follows the command name: `log <id> [flags]` or `retractions import|status [<file>] [flags]`.
pub fn cli(home: &Path, ws_dir: &Path, args: &[String], o: &Options) -> i32 {
    match args.first().map(String::as_str) {
        Some("topics") => topics(ws_dir, o),
        Some("show") => show(ws_dir, args, o),
        Some("log") => log(ws_dir, args, o),
        Some("migrate") => migrate(home, ws_dir, args, o),
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

/// `delphi research topics`: every research record of the workspace and of its registered projects, newest first.
fn topics(ws_dir: &Path, o: &Options) -> i32 {
    let all = research_record::list(ws_dir);
    if o.json {
        let rows: Vec<serde_json::Value> = all
            .iter()
            .map(|(t, r)| serde_json::json!({"topic_id": t.id, "question": r["question"], "state": r["state"], "project": r["project"], "private": r["private"], "updated": r["updated"], "runs": r["runs"].as_array().map_or(0, Vec::len), "pages": r["pages"].as_array().map_or(0, Vec::len), "claims": r["claims"].as_array().map_or(0, Vec::len)}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into()));
    } else {
        print!("{}", research_record::render_list(&all));
    }
    0
}

/// `delphi research show <topic-id>`: one record and its answer.
fn show(ws_dir: &Path, args: &[String], o: &Options) -> i32 {
    let Some(id) = operand(args, 1) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let Some(topic) = research_record::find(ws_dir, id) else {
        eprintln!("delphi research show: no research topic `{}`: `delphi research topics` lists them", id.chars().filter(|c| !c.is_control()).take(80).collect::<String>());
        return 1;
    };
    match topic.read() {
        Ok(rec) if o.json => {
            println!("{}", serde_json::to_string_pretty(&rec).unwrap_or_else(|_| "{}".into()));
            0
        }
        Ok(rec) => {
            print!("{}", research_record::render(&topic, &rec));
            0
        }
        Err(e) => {
            eprintln!("delphi research show: {e}");
            1
        }
    }
}

/// `delphi research migrate [--apply]` (`specs/112` §3.4): the claims `research/host-claims.jsonl` lists, as library claims. It
/// prints what it would do and changes nothing; only `--apply` writes, and only the operator may ask for that.
fn migrate(home: &Path, ws_dir: &Path, args: &[String], o: &Options) -> i32 {
    use delphi_engine::{library::Library, research_migrate as rm};
    let apply = args.iter().any(|a| a == "--apply");
    if apply && crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi research migrate --apply: refused; writing claims into the library is the operator's to ask for and this process was started by an agent seat. Without --apply it prints what it would do.");
        return 2;
    }
    let lib = Library::in_home(home).with_support(settings(ws_dir).support);
    let plan = rm::plan(ws_dir, &lib);
    let applied = (apply && plan.found).then(|| rm::apply(&plan, &lib));
    if o.json {
        println!("{}", serde_json::to_string_pretty(&rm::to_json(&plan, applied.as_ref())).unwrap_or_else(|_| "{}".into()));
    } else {
        print!("{}", rm::render(&plan, applied.as_ref()));
    }
    i32::from(applied.is_some_and(|a| a.failed > 0))
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
        assert_eq!(cli(h, d.path(), &strings(&["frobnicate", "r-t-1"]), &Options { json: false }), 2, "not a subcommand");
    }

    /// `specs/112` AC-8, through the command: `migrate` without `--apply` writes nothing, in the library or the workspace; with it,
    /// the claims the file lists are library claims, and the file is as it was.
    #[test]
    fn migrate_is_a_dry_run_until_apply_is_given() {
        use delphi_engine::library::{AddOpts, Budgets, Level, Library};
        fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
            let (mut out, mut stack) = (Vec::new(), vec![dir.to_path_buf()]);
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        out.push((p.strip_prefix(dir).unwrap().to_string_lossy().to_string(), std::fs::read(&p).unwrap()));
                    }
                }
            }
            out.sort();
            out
        }
        let (home, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let lib = Library::in_home(home.path());
        let page = ws.path().join("page.md");
        std::fs::write(&page, "Aspirin lowers fever in adults, the trial found.").unwrap();
        let source = lib.add_file(&page, &AddOpts { licence: Some("CC0-1.0".into()), level: Some(Level::Full), ..AddOpts::default() }, &Budgets::default()).unwrap().id;
        std::fs::create_dir_all(ws.path().join("research")).unwrap();
        let line = json!({"fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "subject": "aspirin", "predicate": "lowers", "object": "fever", "quote": "lowers fever in adults", "source_ids": [source], "source_urls": ["https://example.org/a"], "status": "validated"});
        std::fs::write(ws.path().join("research/host-claims.jsonl"), format!("{line}\n")).unwrap();
        let (home_before, ws_before) = (tree(home.path()), tree(ws.path()));
        for json in [false, true] {
            assert_eq!(cli(home.path(), ws.path(), &strings(&["migrate"]), &Options { json }), 0);
        }
        assert_eq!(tree(home.path()), home_before, "a dry run changed the library");
        assert_eq!(tree(ws.path()), ws_before, "a dry run changed the workspace");
        assert!(lib.claims().is_empty());
        assert_eq!(cli(home.path(), ws.path(), &strings(&["migrate", "--apply"]), &Options { json: false }), 0);
        let claims = lib.claims();
        assert_eq!(claims.len(), 1);
        assert_eq!((claims[0]["text"].as_str(), delphi_engine::library::claims::status(&claims[0]).as_str()), (Some("aspirin lowers fever"), "noted"), "what the line called itself is not copied");
        assert_eq!(tree(ws.path()), ws_before, "the file is a log: applying does not change it");
        assert_eq!(cli(home.path(), ws.path(), &strings(&["migrate", "--apply"]), &Options { json: true }), 0, "again: nothing new");
        assert_eq!(lib.claims().len(), 1);
        // A workspace with no file has nothing to migrate, and says so without making anything.
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(cli(home.path(), empty.path(), &strings(&["migrate", "--apply"]), &Options { json: false }), 0);
        assert!(tree(empty.path()).is_empty());
    }

    /// `specs/112` §5.3: `topics` lists the records on disk and `show` prints one; both only read.
    #[test]
    fn it_lists_the_research_topics_and_shows_one_and_writes_nothing() {
        use delphi_engine::research_record::{Asker, Begin, Topic};
        let d = tempfile::tempdir().unwrap();
        let h = d.path();
        let o = Options { json: false };
        assert_eq!(cli(h, d.path(), &strings(&["topics"]), &o), 0, "no topics yet is not an error");
        assert!(!d.path().join("research").exists(), "listing creates nothing");
        let t = Topic::begin(d.path(), &Begin { question: "Does aspirin reduce fever?", asked_by: Asker::host("host:claude-code/m"), project: None, depth: delphi_core::research::Depth::Quick, run_id: "r-t-1" }).unwrap();
        let result = json!({"answer": "An answer.", "budget_used": {"steps": 2, "check_steps": 1}, "search_log": {"version": 1, "private": false, "lines": [], "flow": {"found": 0, "duplicates_removed": 0, "kept": 0, "not_kept": {}, "searches": 0, "withheld": 0}}});
        t.finish("r-t-1", "done", Some(&result), &[], None, 5).unwrap();
        let before = std::fs::read(t.record_path()).unwrap();
        for json in [false, true] {
            let o = Options { json };
            assert_eq!(cli(h, d.path(), &strings(&["topics"]), &o), 0);
            assert_eq!(cli(h, d.path(), &strings(&["show", t.id.as_str()]), &o), 0);
            assert_eq!(cli(h, d.path(), &strings(&["show", "--workspace", "w", t.id.as_str()]), &o), 0, "a flag's value is not the id");
            assert_eq!(cli(h, d.path(), &strings(&["log", t.id.as_str()]), &o), 0, "the topic's search log, by the topic's id");
        }
        assert_eq!(std::fs::read(t.record_path()).unwrap(), before, "showing a record does not change it");
        assert_eq!(cli(h, d.path(), &strings(&["show", "r-t-1"]), &o), 1, "a research id is not a topic id");
        assert_eq!(cli(h, d.path(), &strings(&["show", "nope-deadbeef"]), &o), 1, "no such topic");
        assert_eq!(cli(h, d.path(), &strings(&["show", "../../etc/passwd"]), &o), 1, "only a topic id names a folder");
        assert_eq!(cli(h, d.path(), &strings(&["show"]), &o), 2);
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
