//! `delphi sources add|list|show|purge` (`specs/055` §3): the central library's sources layer, per install.
//!
//! The operator's command. Inside an agent seat `add` and `purge` are refused: an agent cannot set a licence, raise a
//! capture level or delete a capture (§10). Capture goes through the V2a fetcher in `delphi-engine`; this file only
//! reads arguments and prints.

use delphi_engine::fact_fetch::FetchPolicy;
use delphi_engine::library::{AddOpts, Budgets, Level, Library, Outcome, Stored};
use serde_json::Value;
use std::path::Path;

const USAGE: &str = "usage:
  delphi sources add <url | file | doi> [--level full|excerpts|pointer] [--licence <id>] [--private]
    [--title <t>] [--rights-holder <name>] [--attribution <text>] [--max-text-mib <n>] [--max-source-mib <n>] [--home <dir>]
  delphi sources list [--json] [--home <dir>]
  delphi sources show <id> [--json] [--home <dir>]
  delphi sources purge <id> [--home <dir>]";

/// A line for a terminal: external titles never carry control characters into it.
fn clean(s: &str, n: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > n { format!("{}…", one.chars().take(n - 1).collect::<String>()) } else { one }
}

fn value_of(args: &[String], name: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => args.get(i + 1).filter(|v| !v.starts_with("--")).cloned().map(Some).ok_or_else(|| format!("{} needs a value", name)),
    }
}

fn mib(args: &[String], name: &str) -> Result<Option<u64>, String> {
    value_of(args, name)?.map(|v| v.parse::<u64>().ok().filter(|n| *n > 0).map(|n| n * 1024 * 1024).ok_or_else(|| format!("{} must be a whole number of MiB", name))).transpose()
}

/// The words that are not flags or flag values.
fn positional(args: &[String]) -> Vec<&String> {
    const WITH_VALUE: [&str; 9] = ["--level", "--licence", "--title", "--rights-holder", "--attribution", "--max-text-mib", "--max-source-mib", "--home", "--workspace"];
    args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !WITH_VALUE.contains(&args[*i - 1].as_str()))).map(|(_, a)| a).collect()
}

fn parse_opts(args: &[String]) -> Result<AddOpts, String> {
    let level = match value_of(args, "--level")? {
        None => None,
        Some(l) => Some(Level::parse(&l).ok_or("--level must be full, excerpts or pointer")?),
    };
    Ok(AddOpts {
        licence: value_of(args, "--licence")?,
        level,
        private: args.iter().any(|a| a == "--private"),
        title: value_of(args, "--title")?,
        rights_holder: value_of(args, "--rights-holder")?,
        attribution_text: value_of(args, "--attribution")?,
        doi: None,
        raise_text: mib(args, "--max-text-mib")?,
        raise_source: mib(args, "--max-source-mib")?,
    })
}

fn report(s: &Stored) {
    let what = match &s.outcome {
        Outcome::Added => "captured".to_string(),
        Outcome::NewVersion(n) => format!("captured as version {} (the earlier one is kept under versions/)", n),
        Outcome::Unchanged => "unchanged (last_checked updated)".to_string(),
        Outcome::KeptRicher => "not changed".to_string(),
    };
    println!("{} {} at level {} (not validated)", s.id, what, s.level.as_str());
    for n in &s.notes {
        println!("  note: {}", n);
    }
}

pub fn cli(args: &[String], home: &Path) -> i32 {
    let lib = Library::in_home(home);
    let rest = &args[args.len().min(1)..];
    match args.first().map(String::as_str) {
        Some("add") => add(&lib, rest),
        Some("list") => list(&lib, rest),
        Some("show") => show(&lib, rest),
        Some("purge") => purge(&lib, rest),
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

fn operator_only(cmd: &str) -> bool {
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi sources {}: refused; the library's licences, capture levels and deletions are the operator's and this process was started by an agent seat", cmd);
        return false;
    }
    true
}

fn add(lib: &Library, args: &[String]) -> i32 {
    if !operator_only("add") {
        return 2;
    }
    let opts = match parse_opts(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("delphi sources add: {}\n{}", e, USAGE);
            return 2;
        }
    };
    let pos = positional(args);
    let [target] = pos[..] else {
        eprintln!("{}", USAGE);
        return 2;
    };
    let budgets = Budgets::default();
    let result = if target.starts_with("http://") || target.starts_with("https://") {
        capture_url(lib, target, &opts, &budgets)
    } else if target.starts_with("10.") && target.contains('/') {
        capture_url(lib, &format!("https://doi.org/{}", target), &AddOpts { doi: Some(target.to_string()), ..opts }, &budgets)
    } else if Path::new(target).is_file() {
        lib.add_file(Path::new(target), &opts, &budgets)
    } else {
        eprintln!("delphi sources add: `{}` is not an https URL, a DOI or a file", clean(target, 80));
        return 2;
    };
    match result {
        Ok(s) => {
            report(&s);
            0
        }
        Err(e) => {
            eprintln!("delphi sources add: {}", clean(&e, 300));
            1
        }
    }
}

fn capture_url(lib: &Library, url: &str, opts: &AddOpts, budgets: &Budgets) -> Result<Stored, String> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(lib.add_url(&FetchPolicy::default(), url, opts, budgets))
}

fn list(lib: &Library, args: &[String]) -> i32 {
    let all = lib.list();
    if args.iter().any(|a| a == "--json") {
        let v: Vec<Value> = all.iter().map(|(id, m)| serde_json::json!({"id": id, "meta": m})).collect();
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return 0;
    }
    if all.is_empty() {
        println!("the library has no sources; `delphi sources add <url | file | doi>` adds one");
        return 0;
    }
    println!("{} source(s), {} KiB in the library", all.len(), lib.used_bytes() / 1024);
    for (id, m) in all {
        let s = |k: &str| m[k].as_str().unwrap_or("?").to_string();
        let private = if m["private"] == true { " private" } else { "" };
        let purged = if m["purged"].is_null() { "" } else { " purged" };
        println!("  {:<52} {:<9} {:<22} v{}{}{}  {}", id, s("capture_level"), clean(&s("licence"), 22), s("version"), private, purged, clean(&s("title"), 48));
    }
    0
}

fn show(lib: &Library, args: &[String]) -> i32 {
    let pos = positional(args);
    let [id] = pos[..] else {
        eprintln!("{}", USAGE);
        return 2;
    };
    let Some(m) = lib.read_meta(id) else {
        eprintln!("delphi sources show: no source `{}`", clean(id, 80));
        return 1;
    };
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&m).unwrap_or_default());
        return 0;
    }
    let dir = lib.source_dir(id);
    let size = |p: &str| std::fs::metadata(dir.join(p)).map(|m| m.len()).ok();
    println!("{}  {}", id, clean(m["title"].as_str().unwrap_or("?"), 100));
    for k in ["type", "licence", "rights_holder", "share_alike", "capture_level", "version", "retrieved", "last_checked", "canonical_url", "content_hash", "origin", "source_class", "status", "imported_from", "change_note", "purged"] {
        if !m[k].is_null() {
            let v = m[k].as_str().map(String::from).unwrap_or_else(|| m[k].to_string());
            println!("  {:<14} {}", k, clean(&v, 120));
        }
    }
    if m["private"] == true {
        println!("  private        yes: never leaves this install and is not indexed");
    }
    if let Some(n) = size("source.md") {
        println!("  text           {} bytes at {}", n, dir.join("source.md").display());
    }
    let versions = std::fs::read_dir(dir.join("versions")).map(|d| d.flatten().count()).unwrap_or(0);
    if versions > 0 {
        println!("  earlier versions: {}", versions);
    }
    0
}

fn purge(lib: &Library, args: &[String]) -> i32 {
    if !operator_only("purge") {
        return 2;
    }
    let pos = positional(args);
    let [id] = pos[..] else {
        eprintln!("{}", USAGE);
        return 2;
    };
    match lib.purge(id) {
        Ok(Some(n)) => {
            println!("{}: capture deleted ({} bytes); the citation and the content hash are kept", id, n);
            0
        }
        Ok(None) => {
            eprintln!("delphi sources purge: no source `{}`", clean(id, 80));
            1
        }
        Err(e) => {
            eprintln!("delphi sources purge: {}", e);
            1
        }
    }
}
