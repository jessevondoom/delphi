//! `delphi pages show|check|dispute|search|list|pin` (`specs/055` §6): the readable view of the library's claims.
//!
//! A page is generated, never edited and never trusted text: it carries a banner and a bibliography table the daemon
//! writes, and the daemon's mechanical check (`specs/057` §3.1) rejects a page that cites what the library does not hold
//! or quotes what a source does not say. `dispute` is the operator's
//! "this is wrong" on a sentence (refused inside an agent seat): it files a contested mark on the cited claim.

use delphi_engine::library::bib::Format;
use delphi_engine::library::pages::{Finding, Scope};
use delphi_engine::library::Library;
use serde_json::json;
use std::path::Path;

const USAGE: &str = "usage:
  delphi pages show <topic words...> [--draft <file>] [--style apa|mla|chicago|plain] [--home <dir>]
  delphi pages check <file> [--home <dir>]
  delphi pages dispute <claim> --note <what is wrong> [--home <dir>]
  delphi pages search <question words...> [--internal | --external] [--json] [--home <dir>]
  delphi pages list [--json] [--home <dir>]
  delphi pages pin|unpin <key> [--home <dir>]";

fn value_of(args: &[String], name: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => args.get(i + 1).filter(|v| !v.starts_with("--")).cloned().map(Some).ok_or_else(|| format!("{} needs a value", name)),
    }
}

fn positional(args: &[String]) -> Vec<&String> {
    args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !["--draft", "--style", "--note", "--home", "--workspace"].contains(&args[*i - 1].as_str()))).map(|(_, a)| a).collect()
}

fn report(findings: &[Finding]) {
    eprintln!("the page is rejected: {} finding(s)", findings.len());
    for f in findings {
        eprintln!("  {}", f);
    }
}

fn search(home: &Path, ws_dir: &Path, query: &str, external: Option<bool>, as_json: bool) -> i32 {
    use delphi_core::spawn::{connect_or_spawn, SpawnOptions};
    let Some(exe) = crate::chat::daemon_exe() else {
        eprintln!("delphi pages search: cannot locate this program to start the daemon");
        return 4;
    };
    let mut attached = match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()], own_version: Some(env!("CARGO_PKG_VERSION")) }) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi pages search: {}", e);
            return 4;
        }
    };
    let r = attached.client.call("pages.search", json!({"query": query, "external": external}));
    if attached.spawned {
        let _ = attached.client.call("daemon.shutdown", json!({}));
    }
    match r {
        Ok(v) if as_json => {
            println!("{}", v);
            0
        }
        Ok(v) => {
            print!("{}", v["markdown"].as_str().unwrap_or(""));
            for n in v["notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str()) {
                eprintln!("{}", n);
            }
            eprintln!("{} · {}{}", v["path"].as_str().unwrap_or(""), v["outcome"].as_str().unwrap_or(""), if v["cached"] == true { " · cached" } else { "" });
            if v["outcome"] == "claims_only" { 1 } else { 0 }
        }
        Err(e) => {
            eprintln!("delphi pages search: {}", e);
            1
        }
    }
}

pub fn cli(args: &[String], home: &Path, ws_dir: Option<&Path>) -> i32 {
    let lib = Library::in_home(home);
    let rest = &args[args.len().min(1)..];
    let pos = positional(rest);
    let (draft, style, note) = match (value_of(rest, "--draft"), value_of(rest, "--style"), value_of(rest, "--note")) {
        (Ok(d), Ok(s), Ok(n)) => (d, s, n),
        (d, s, n) => {
            eprintln!("delphi pages: {}", d.err().or(s.err()).or(n.err()).unwrap_or_default());
            return 2;
        }
    };
    match args.first().map(String::as_str) {
        Some("show") if !pos.is_empty() => {
            let Some(style) = style.as_deref().map_or(Some(Format::Apa), Format::parse).filter(|f| !matches!(f, Format::CslJson | Format::BibTex)) else {
                eprintln!("delphi pages: --style is apa, mla, chicago or plain");
                return 2;
            };
            let draft = match draft.map(|p| std::fs::read_to_string(&p).map_err(|e| format!("{}: {}", p, e))).transpose() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("delphi pages: {}", e);
                    return 2;
                }
            };
            let topic = pos.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
            match lib.build_page(&topic, draft.as_deref(), style) {
                Ok(p) => {
                    print!("{}", p.markdown);
                    if !p.flags.is_empty() {
                        eprintln!("unverified figure (in no cited material): {}", p.flags.join(", "));
                    }
                    if p.withheld > 0 {
                        eprintln!("{} claim(s) rest on a private source and are left off", p.withheld);
                    }
                    0
                }
                Err(f) => {
                    report(&f);
                    1
                }
            }
        }
        Some("check") if pos.len() == 1 => match std::fs::read_to_string(pos[0]) {
            Ok(t) => {
                let checked = lib.check_page_text(&t, Scope::External);
                if checked.findings.is_empty() {
                    println!("the page passes the mechanical check");
                    if !checked.flags.is_empty() {
                        println!("unverified figure (in no cited material): {}", checked.flags.join(", "));
                    }
                    0
                } else {
                    report(&checked.findings);
                    1
                }
            }
            Err(e) => {
                eprintln!("delphi pages check: {}: {}", pos[0], e);
                2
            }
        },
        Some("search") if !pos.is_empty() => {
            let (internal, external) = (rest.iter().any(|a| a == "--internal"), rest.iter().any(|a| a == "--external"));
            if internal && external {
                eprintln!("delphi pages search: --internal and --external are alternatives");
                return 2;
            }
            let Some(ws) = ws_dir else { return 4 };
            let mode = if external { Some(true) } else if internal { Some(false) } else { None };
            search(home, ws, &pos.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "), mode, rest.iter().any(|a| a == "--json"))
        }
        Some("list") => {
            let ttl = delphi_adapter::config::PagesConfig::default().external_ttl_secs;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let pages = lib.pages(now, ttl);
            if rest.iter().any(|a| a == "--json") {
                println!("{}", json!(pages.iter().map(|p| p.to_json()).collect::<Vec<_>>()));
            } else if pages.is_empty() {
                println!("no cached pages");
            }
            for p in pages.iter().filter(|_| !rest.iter().any(|a| a == "--json")) {
                println!("{}  {}{}  {}{}", p.key, if p.pinned { "pinned " } else { "" }, p.outcome, p.query, p.stale.as_ref().map(|s| format!("  ({})", s)).unwrap_or_default());
            }
            0
        }
        Some(c @ ("pin" | "unpin")) if pos.len() == 1 => {
            let ttl = delphi_adapter::config::PagesConfig::default().external_ttl_secs;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            match lib.pin_page(pos[0], c == "pin", now, ttl) {
                Ok(_) => {
                    println!("{} {}", pos[0], if c == "pin" { "pinned" } else { "unpinned" });
                    0
                }
                Err(e) => {
                    eprintln!("delphi pages {}: {}", c, e);
                    1
                }
            }
        }
        Some("dispute") if pos.len() == 1 => {
            if crate::xenia_terminal::inside_an_agent_seat() {
                eprintln!("delphi pages dispute: refused; a dispute is the operator's to file and this process was started by an agent seat");
                return 2;
            }
            match lib.dispute_claim(pos[0], note.as_deref().unwrap_or("")) {
                Ok(c) => {
                    println!("{} contested: {}", pos[0], delphi_engine::library::claims::status(&c).label());
                    0
                }
                Err(e) => {
                    eprintln!("delphi pages dispute: {}", e);
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
