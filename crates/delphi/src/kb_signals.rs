//! `delphi kb reads`, `delphi kb flag` and `delphi kb disputes` (`specs/116` §7, `specs/080`): the read-count report, the operator's
//! own dispute, and the review of disputes. The report reads the mission logs and writes nothing; a flag and a clear write one line of
//! the daemon's disputes ledger (`delphi_engine::use_signals`) and the copy the index reads. Nothing here deletes or edits an item.

use delphi_engine::use_signals as us;
use std::path::Path;

const USAGE: &str = "usage: delphi kb reads [--top <n>] [--since <date>] [--json]\n       delphi kb flag <pointer> --why <what is wrong>\n       delphi kb disputes [--json] | disputes clear <pointer>";

fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// `args` starts at the subcommand (`reads`, `flag`, `disputes`).
pub fn run(ws_dir: &Path, args: &[String]) -> i32 {
    let now = delphi_core::time::now_rfc3339();
    let db = ws_dir.join("local/state.db");
    let json = args.iter().any(|a| a == "--json");
    match args.first().map(String::as_str) {
        Some("reads") => {
            let top = match value(args, "--top").map(str::parse::<usize>) {
                None => 20,
                Some(Ok(n)) if n > 0 => n,
                _ => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            };
            let since = value(args, "--since");
            let counts = us::read_counts(ws_dir, since);
            if json {
                let rows: Vec<serde_json::Value> = counts.iter().map(|(f, r)| serde_json::json!({"fact": f, "agent": r.agent, "host": r.host})).collect();
                println!("{}", serde_json::json!({"scope": "agent and host reads, from the mission logs", "since": since, "facts": rows, "never_read": serde_json::Value::Null, "never_read_withheld": "until facts can carry an invariant tag (specs/080 W1.3)"}));
            } else {
                print!("{}", us::render_reads(&counts, top, since));
            }
            0
        }
        Some("flag") => {
            let (Some(pointer), Some(why)) = (args.get(1).filter(|a| !a.starts_with("--")), value(args, "--why")) else {
                eprintln!("{USAGE}");
                return 2;
            };
            match us::flag(ws_dir, &db, pointer, why, us::Flagger::Operator, &now) {
                Ok(r) => {
                    println!("disputed {}: it keeps its place in search, labelled `{}`, and is not handed to agents automatically until you clear it (`delphi kb disputes clear {}`). Nothing was deleted or edited.", r["flagged"].as_str().unwrap_or(""), r["label"].as_str().unwrap_or(""), r["flagged"].as_str().unwrap_or(""));
                    0
                }
                Err(e) => {
                    eprintln!("delphi kb flag: {e}");
                    1
                }
            }
        }
        Some("disputes") if args.get(1).map(String::as_str) == Some("clear") => {
            let Some(pointer) = args.get(2) else {
                eprintln!("{USAGE}");
                return 2;
            };
            match us::clear(ws_dir, &db, pointer, &now) {
                Ok(n) => {
                    println!("cleared {n} dispute(s) of {pointer}: it is handed to agents automatically again. Nothing was deleted or edited.");
                    0
                }
                Err(e) => {
                    eprintln!("delphi kb disputes clear: {e}");
                    1
                }
            }
        }
        Some("disputes") => {
            let all = us::list(ws_dir);
            let state = us::state(ws_dir);
            if json {
                let rows: Vec<serde_json::Value> = all.iter().map(|d| serde_json::json!({"target": d.target, "by": d.by, "counted": d.counted, "at": d.at, "why": d.why})).collect();
                println!("{}", serde_json::json!({"disputes": rows, "dropped": state.iter().filter(|(_, s)| s.dropped()).map(|(t, _)| t).collect::<Vec<_>>()}));
                return 0;
            }
            if all.is_empty() {
                println!("no disputes: agents and hosts dispute an item with flag_wrong, you with `delphi kb flag`");
                return 0;
            }
            for (t, s) in &state {
                println!("{t}  {}{}", s.label(), if s.dropped() { "  [not handed to agents automatically]" } else { "" });
                for d in all.iter().filter(|d| &d.target == t) {
                    // The reason is a disputer's own words: data, shown cut and on one line.
                    println!("    {}  {}{}  {}", d.at.get(..10).unwrap_or(&d.at), d.by, if d.counted || d.by == "operator" { "" } else { " (not counted)" }, d.why.chars().take(200).collect::<String>());
                }
            }
            println!("`delphi kb disputes clear <pointer>` after you have looked: the item is handed out automatically again. Nothing is ever deleted or edited by a dispute.");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}
