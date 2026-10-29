//! `delphi pages show|check|dispute` (`specs/055` §6): the readable view of the library's claims.
//!
//! A page is generated, never edited and never trusted text: each sentence carries its claim ids and their derived
//! status, and the daemon's check rejects a page with a sentence no claim accounts for. `dispute` is the operator's
//! "this is wrong" on a sentence (refused inside an agent seat): it files a contested mark on the cited claim.

use delphi_engine::library::bib::Format;
use delphi_engine::library::pages::Finding;
use delphi_engine::library::Library;
use std::path::Path;

const USAGE: &str = "usage:
  delphi pages show <topic words...> [--draft <file>] [--style apa|mla|chicago|plain] [--home <dir>]
  delphi pages check <file> [--home <dir>]
  delphi pages dispute <claim> --note <what is wrong> [--home <dir>]";

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
    eprintln!("the page is rejected: {} sentence(s) are not accounted for by claims", findings.len());
    for f in findings {
        eprintln!("  {}", f);
    }
}

pub fn cli(args: &[String], home: &Path) -> i32 {
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
                let findings = lib.check_page(&delphi_engine::library::pages::parse_draft(&t));
                if findings.is_empty() {
                    println!("every sentence maps to claims");
                    0
                } else {
                    report(&findings);
                    1
                }
            }
            Err(e) => {
                eprintln!("delphi pages check: {}: {}", pos[0], e);
                2
            }
        },
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
