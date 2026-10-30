//! `delphi sources acquire ...` and `delphi sources reading-list ...` (`specs/055` §8): the operator's side of library
//! acquisition. Proposals normally arrive as one card in the digest; these commands do the same from a terminal. Everything
//! here is the operator's: inside an agent seat it is refused. The licence, level and status of a capture are never arguments.

use delphi_adapter::config::Config;
use delphi_engine::fact_fetch::FetchPolicy;
use delphi_engine::library::acquire::{ArchiveList, JobConfig};
use delphi_engine::library::Library;
use serde_json::Value;
use std::path::Path;

pub const USAGE: &str = "usage:
  delphi sources acquire status [--home <dir>]
  delphi sources acquire run [--home <dir>]
  delphi sources acquire batch [<batch>] [--json] [--home <dir>]
  delphi sources acquire accept|skip <batch> [<source-id>...] [--home <dir>]
  delphi sources acquire rule add --topic <t>... --archive <id>... [--max-mib <n>] [--home <dir>]
  delphi sources acquire rule list|remove <rule> [--home <dir>]
  delphi sources reading-list list|show <list> [--home <dir>]
  delphi sources reading-list add <list> <source-id>... [--title <t>] [--home <dir>]
  delphi sources reading-list remove <list> <source-id> [--home <dir>]";

fn all(args: &[String], name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == name {
            if let Some(v) = args.get(i + 1).filter(|v| !v.starts_with("--")) {
                out.push(v.clone());
            }
        }
        i += 1;
    }
    out
}

fn positional(args: &[String]) -> Vec<String> {
    const WITH_VALUE: [&str; 5] = ["--topic", "--archive", "--max-mib", "--title", "--home"];
    args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !WITH_VALUE.contains(&args[*i - 1].as_str()))).map(|(_, a)| a.clone()).collect()
}

fn refused(cmd: &str) -> bool {
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi sources {}: refused; what the library acquires is the operator's and this process was started by an agent seat", cmd);
        return true;
    }
    false
}

fn config(home: &Path) -> (Config, Vec<String>) {
    match std::fs::read_to_string(home.join("config.toml")) {
        Ok(t) => match Config::parse(&t) {
            Ok(c) => (c, Vec::new()),
            Err(e) => (Config::default(), e),
        },
        Err(_) => (Config::default(), Vec::new()),
    }
}

fn job_config(c: &Config) -> JobConfig {
    let a = &c.library.acquire;
    JobConfig { byte_budget: a.byte_budget_mib * 1024 * 1024, max_item: a.max_item_mib * 1024 * 1024, batch_size: a.batch_size, token_share_percent: a.token_share_percent }
}

fn line(s: &str, n: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > n { format!("{}…", one.chars().take(n - 1).collect::<String>()) } else { one }
}

fn show_batch(b: &Value) {
    println!("{} ({} item(s), from topics: {})", b["id"].as_str().unwrap_or("?"), b["items"].as_array().map_or(0, |a| a.len()), b["topics"].as_array().map_or(String::new(), |t| t.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")));
    for i in b["items"].as_array().into_iter().flatten() {
        let s = |k: &str| i[k].as_str().unwrap_or("?").to_string();
        println!("  {:<9} {:<48} {} · {} · {}", s("status"), s("id"), line(&s("title"), 48), line(&s("archive"), 20), line(&s("basis"), 60));
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())
}

fn capture_accepted(lib: &Library, archives: &ArchiveList, cfg: &JobConfig) -> i32 {
    let rt = match runtime() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi sources acquire: {}", e);
            return 4;
        }
    };
    let policy = FetchPolicy::default().with_max_bytes(cfg.max_item as usize);
    let mut code = 0;
    for (id, r) in rt.block_on(lib.run_accepted(&policy, archives, cfg)) {
        match r {
            Ok(s) => println!("{} captured at level {} (not validated)", id, s.level.as_str()),
            Err(e) => {
                println!("{} not captured: {}", id, line(&e, 160));
                code = 1;
            }
        }
    }
    code
}

pub fn acquire(home: &Path, args: &[String]) -> i32 {
    let lib = Library::in_home(home);
    let (cfg, errs) = config(home);
    if let Some(e) = errs.first() {
        eprintln!("delphi sources acquire: config.toml: {}", e);
        return 2;
    }
    let job = job_config(&cfg);
    let archives = match ArchiveList::load(home) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi sources acquire: {}", e);
            return 2;
        }
    };
    let rest = &args[args.len().min(1)..];
    let pos = positional(rest);
    match args.first().map(String::as_str) {
        Some("status") => {
            let a = &cfg.library.acquire;
            println!("acquisition: {} (config [library.acquire])", if a.enabled { "on" } else { "off" });
            println!("held: {} MiB of {} MiB", lib.acquired_bytes() / (1024 * 1024), a.byte_budget_mib);
            println!("archives: {}", archives.archives().iter().map(|a| a.id.as_str()).collect::<Vec<_>>().join(", "));
            println!("standing rules: {}", lib.rules().len());
            match lib.open_batch() {
                Some(b) => println!("a batch waits: {} (digest, or `delphi sources acquire batch`)", b["id"].as_str().unwrap_or("?")),
                None => println!("no batch waits"),
            }
            0
        }
        Some("batch") => match pos.first() {
            Some(id) => match lib.batch(id) {
                Some(b) if rest.iter().any(|a| a == "--json") => {
                    println!("{}", serde_json::to_string_pretty(&b).unwrap_or_default());
                    0
                }
                Some(b) => {
                    show_batch(&b);
                    0
                }
                None => {
                    eprintln!("delphi sources acquire: no batch `{}`", line(id, 40));
                    1
                }
            },
            None => {
                for b in lib.batches() {
                    show_batch(&b);
                }
                0
            }
        },
        Some("run") => {
            if refused("acquire run") {
                return 2;
            }
            if !cfg.library.acquire.enabled {
                eprintln!("delphi sources acquire: acquisition is off; set `[library.acquire] enabled = true` in config.toml");
                return 2;
            }
            let rt = match runtime() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("delphi sources acquire: {}", e);
                    return 4;
                }
            };
            let policy = FetchPolicy::default().with_max_bytes(job.max_item as usize);
            let seed = lib.seed_topics(&cfg.library.acquire.interests, &[]);
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            let r = rt.block_on(lib.run_job(&policy, &archives, &job, seed, None, 0, now));
            println!("{} proposed, {} taken by a standing rule, {} captured", r.proposed, r.taken_by_rule, r.captured.len());
            for n in &r.notes {
                println!("  note: {}", line(n, 160));
            }
            0
        }
        Some(a @ ("accept" | "skip")) => {
            if refused(&format!("acquire {}", a)) {
                return 2;
            }
            let Some(batch) = pos.first() else {
                eprintln!("{}", USAGE);
                return 2;
            };
            match lib.decide_batch(batch, a, &pos[1..]) {
                Ok(ids) => {
                    println!("{} item(s) {}", ids.len(), if a == "accept" { "accepted" } else { "skipped" });
                    if a == "accept" { capture_accepted(&lib, &archives, &job) } else { 0 }
                }
                Err(e) => {
                    eprintln!("delphi sources acquire {}: {}", a, e);
                    1
                }
            }
        }
        Some("rule") => rule(&lib, &archives, &job, rest),
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

fn rule(lib: &Library, archives: &ArchiveList, job: &JobConfig, args: &[String]) -> i32 {
    let rest = &args[args.len().min(1)..];
    match args.first().map(String::as_str) {
        Some("list") => {
            for r in lib.rules() {
                println!("{}  topics: {}  archives: {}  up to {} KiB", r.id, r.topics.join(", "), r.archives.join(", "), r.max_bytes / 1024);
            }
            0
        }
        Some("add") => {
            if refused("acquire rule add") {
                return 2;
            }
            let max = match all(rest, "--max-mib").first() {
                Some(v) => match v.parse::<u64>() {
                    Ok(n) if n > 0 => n * 1024 * 1024,
                    _ => {
                        eprintln!("delphi sources acquire rule add: --max-mib must be a whole number");
                        return 2;
                    }
                },
                None => job.max_item,
            };
            match lib.add_rule(&all(rest, "--topic"), &all(rest, "--archive"), max.min(job.max_item), Some(archives)) {
                Ok(r) => {
                    println!("{} recorded: take {} from {} up to {} KiB", r.id, r.topics.join(", "), r.archives.join(", "), r.max_bytes / 1024);
                    0
                }
                Err(e) => {
                    eprintln!("delphi sources acquire rule add: {}", e);
                    2
                }
            }
        }
        Some("remove") => {
            if refused("acquire rule remove") {
                return 2;
            }
            let Some(id) = positional(rest).first().cloned() else {
                eprintln!("{}", USAGE);
                return 2;
            };
            match lib.remove_rule(&id) {
                Ok(()) => {
                    println!("{} removed", id);
                    0
                }
                Err(e) => {
                    eprintln!("delphi sources acquire rule remove: {}", e);
                    1
                }
            }
        }
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

pub fn reading_list(home: &Path, args: &[String]) -> i32 {
    let lib = Library::in_home(home);
    let rest = &args[args.len().min(1)..];
    let pos = positional(rest);
    match args.first().map(String::as_str) {
        Some("list") => {
            for l in lib.reading_lists() {
                println!("{}  {}  ({} source(s))", l["id"].as_str().unwrap_or("?"), line(l["title"].as_str().unwrap_or(""), 48), l["items"].as_array().map_or(0, |s| s.len()));
            }
            0
        }
        Some("show") => match pos.first().and_then(|id| lib.reading_list(id)) {
            Some(l) => {
                println!("{}", line(l["title"].as_str().unwrap_or(""), 80));
                for s in l["items"].as_array().into_iter().flatten().filter_map(|s| s["source"].as_str()) {
                    let m = lib.read_meta(s);
                    let title = m.as_ref().and_then(|m| m["title"].as_str()).unwrap_or("(not in the library)");
                    println!("  {:<52} {}", s, line(title, 60));
                }
                0
            }
            None => {
                eprintln!("delphi sources reading-list: no such list");
                1
            }
        },
        Some(op @ ("add" | "remove")) => {
            if refused(&format!("reading-list {}", op)) {
                return 2;
            }
            let Some((id, sources)) = pos.split_first() else {
                eprintln!("{}", USAGE);
                return 2;
            };
            let r = if op == "add" { lib.reading_add(id, all(rest, "--title").first().map(String::as_str), sources).map(|_| ()) } else { sources.first().map_or(Err("name a source".to_string()), |s| lib.reading_remove(id, s)) };
            match r {
                Ok(()) => {
                    println!("{} {}", id, if op == "add" { "updated" } else { "source removed" });
                    0
                }
                Err(e) => {
                    eprintln!("delphi sources reading-list {}: {}", op, e);
                    1
                }
            }
        }
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}
