//! `/ratify` in the console (`specs/005` §3.1): review pending proposals one at a time, with their diffs,
//! and ratify or reject each individually. Ratifying is bound to the version you reviewed: if the file
//! changed after the list was fetched, the daemon refuses and you review it again.

use delphi_core::client::{Client, ClientError};
use serde_json::{json, Value};

#[derive(Debug, PartialEq)]
pub enum Cmd {
    List,
    Show(usize),
    Apply(usize),
    Reject(usize),
    Bad(String),
}

pub fn parse(args: &[String]) -> Cmd {
    let idx = |s: Option<&String>| s.and_then(|x| x.parse::<usize>().ok()).filter(|n| *n >= 1);
    match args.first().map(|s| s.as_str()) {
        None | Some("list") => Cmd::List,
        Some("show") => idx(args.get(1)).map(Cmd::Show).unwrap_or_else(|| Cmd::Bad("usage: /ratify show <number>".into())),
        Some("apply") | Some("ratify") => idx(args.get(1)).map(Cmd::Apply).unwrap_or_else(|| Cmd::Bad("usage: /ratify apply <number>".into())),
        Some("reject") => idx(args.get(1)).map(Cmd::Reject).unwrap_or_else(|| Cmd::Bad("usage: /ratify reject <number>".into())),
        Some(other) => Cmd::Bad(format!("unknown /ratify action `{}` (list, show, apply, reject)", other)),
    }
}

pub fn format_list(items: &[Value]) -> String {
    if items.is_empty() {
        return "no pending proposals\n".to_string();
    }
    let mut s = format!("{} pending proposal(s). Nothing below takes effect until you ratify it.\n", items.len());
    for (i, p) in items.iter().enumerate() {
        let red = if p["kind"] == "config-access" { " [RED]" } else { "" };
        s.push_str(&format!("{:>3}. [{}]{} {}\n", i + 1, p["kind"].as_str().unwrap_or("?"), red, p["summary"].as_str().unwrap_or("")));
    }
    s.push_str("/ratify show <n> · apply <n> · reject <n>\n");
    s
}

pub fn format_show(p: &Value) -> String {
    let diff = p["diff"].as_str().unwrap_or("");
    let alignment: Vec<&str> = p["alignment"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).take(20).collect()).unwrap_or_default();
    let notes = if alignment.is_empty() { String::new() } else { format!("\nAlignment (advice, never blocks your decision):\n{}", alignment.iter().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n")) };
    format!("{} — {}{}\n{}", p["id"].as_str().unwrap_or("?"), p["summary"].as_str().unwrap_or(""), notes, if diff.is_empty() { "(no diff for this kind of change)\n".to_string() } else { diff.to_string() })
}

fn rpc_error(e: &ClientError) -> String {
    crate::chat::rpc_error(e)
}

/// `last` holds the list the operator last saw: applying uses exactly that version.
pub fn handle(cmd: &mut Client, args: &[String], last: &mut Vec<Value>) -> String {
    match parse(args) {
        Cmd::Bad(m) => format!("{}\n", m),
        Cmd::List => match cmd.call("ratify.list", json!({})) {
            Ok(v) => {
                *last = v.as_array().cloned().unwrap_or_default();
                format_list(last)
            }
            Err(e) => format!("{}\n", rpc_error(&e)),
        },
        Cmd::Show(n) | Cmd::Apply(n) | Cmd::Reject(n) if last.is_empty() => format!("run /ratify first to list proposals (asked for #{})\n", n),
        Cmd::Show(n) => last.get(n - 1).map(format_show).unwrap_or_else(|| format!("no proposal #{}\n", n)),
        c @ (Cmd::Apply(_) | Cmd::Reject(_)) => {
            let (n, method) = match c {
                Cmd::Apply(n) => (n, "ratify.apply"),
                Cmd::Reject(n) => (n, "ratify.reject"),
                _ => unreachable!(),
            };
            let Some(p) = last.get(n - 1).cloned() else { return format!("no proposal #{}\n", n) };
            let Some(expected) = p["live_blake3"].as_str() else {
                // A deletion: the pinned version is the one being decided on.
                return match cmd.call(method, json!({"id": p["id"], "expected_blake3": p["ratified_blake3"]})) {
                    Ok(_) => format!("{} {}\n", if method == "ratify.apply" { "ratified" } else { "rejected" }, p["id"].as_str().unwrap_or("")),
                    Err(e) => format!("{}\n", rpc_error(&e)),
                };
            };
            match cmd.call(method, json!({"id": p["id"], "expected_blake3": expected})) {
                Ok(_) => {
                    last.remove(n - 1);
                    format!("{} {}\n", if method == "ratify.apply" { "ratified" } else { "rejected" }, p["id"].as_str().unwrap_or(""))
                }
                Err(e) => format!("{}\n", rpc_error(&e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn argument_parsing() {
        assert_eq!(parse(&a(&[])), Cmd::List);
        assert_eq!(parse(&a(&["list"])), Cmd::List);
        assert_eq!(parse(&a(&["show", "2"])), Cmd::Show(2));
        assert_eq!(parse(&a(&["apply", "1"])), Cmd::Apply(1));
        assert_eq!(parse(&a(&["reject", "3"])), Cmd::Reject(3));
        for bad in [vec!["show"], vec!["apply", "0"], vec!["apply", "x"], vec!["nuke"]] {
            assert!(matches!(parse(&a(&bad)), Cmd::Bad(_)), "{:?}", bad);
        }
    }

    #[test]
    fn the_list_marks_red_changes_and_says_nothing_is_active_yet() {
        let items = vec![json!({"id": "ws:REVIEW.md", "kind": "changed", "summary": "changed REVIEW.md"}), json!({"id": "ws:config.toml#access", "kind": "config-access", "summary": "endpoint settings changed"})];
        let s = format_list(&items);
        assert!(s.contains("2 pending") && s.contains("Nothing below takes effect") && s.contains("1. [changed] changed REVIEW.md") && s.contains("2. [config-access] [RED]"), "{}", s);
        assert_eq!(format_list(&[]), "no pending proposals\n");
        assert!(format_show(&json!({"id": "ws:x", "summary": "s", "diff": "- a\n+ b\n"})).contains("+ b"));
        assert!(format_show(&json!({"id": "ws:x", "summary": "s", "diff": ""})).contains("no diff"));
    }
}
