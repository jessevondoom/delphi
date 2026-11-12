//! `delphi history off | purge | delete | status` (`specs/102` §7 item 6, §11 items 7, 10 and 15; slice S4), and the install question.
//!
//! The operator's, in a terminal. Every subcommand is refused inside an agent seat (§11 item 10), and there is no daemon method for any
//! of them, so a host or a seat has nothing to call. The logic lives in `delphi_engine::history_manage` (files only; no network, no
//! model); this is the reading of the flags and the printing. Nothing printed here is record text.

use delphi_engine::history_consent::{self, Answer, Human};
use delphi_engine::history_manage::{self as hm, Which};
use delphi_engine::history_record::Settings;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

const USAGE: &str = "usage: delphi history off [--project <id>]\n       delphi history purge [--since <duration | date | time>] [--project <id>] [--yes]\n       delphi history delete <id> [--project <id>] [--yes]\n       delphi history status\n       (all take [--workspace <name>] [--home <dir>])";

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn settings_of(ws: &Path) -> Settings {
    std::fs::read_to_string(ws.join("config.toml")).ok().and_then(|t| t.parse::<toml::Value>().ok()).map(|v| Settings::parse(&v).0).unwrap_or_default()
}

/// `--since`: a duration back from now (`7d`, `12h`), an RFC 3339 time, or a date (`2026-11-01`, the start of that day, UTC).
pub fn parse_since(s: &str, now: i64) -> Result<i64, String> {
    if let Ok(d) = crate::run::parse_duration(s) {
        return Ok(now - d.as_secs() as i64);
    }
    if let Some(t) = delphi_core::time::parse_rfc3339(s) {
        return Ok(t);
    }
    if s.len() == 10 {
        if let Some(t) = delphi_core::time::parse_rfc3339(&format!("{s}T00:00:00Z")) {
            return Ok(t);
        }
    }
    Err(format!("`{}` is not a time (use 7d, 12h, 2026-11-01 or 2026-11-01T09:30:00Z)", s.chars().filter(|c| !c.is_control()).take(40).collect::<String>()))
}

fn positional(args: &[String]) -> Vec<&String> {
    let mut v = Vec::new();
    let mut skip = false;
    for a in args.iter().skip(1) {
        if skip {
            skip = false;
        } else if matches!(a.as_str(), "--since" | "--project" | "--workspace" | "--home") {
            skip = true;
        } else if !a.starts_with("--") {
            v.push(a);
        }
    }
    v
}

fn confirm(question: &str, args: &[String]) -> bool {
    if args.iter().any(|a| a == "--yes") {
        return true;
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("delphi history: this erases text and cannot be undone; run it at a terminal, or pass --yes");
        return false;
    }
    print!("{question} Type `yes` to erase: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == "yes"
}

/// `delphi history ...` for the workspace at `ws`.
pub fn run(args: &[String], ws: &Path, flag: impl Fn(&str) -> Option<String>) -> i32 {
    let sub = args.first().map(String::as_str);
    if !matches!(sub, Some("off" | "purge" | "delete" | "status")) {
        eprintln!("{USAGE}");
        return 2;
    }
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi history {}: refused; this is the operator's and this process was started by an agent seat", sub.unwrap_or(""));
        return 2;
    }
    let settings = settings_of(ws);
    let project = flag("--project");
    match sub {
        Some("status") => {
            for l in hm::status_lines(&hm::overview(ws, &settings, now_unix())) {
                println!("{l}");
            }
            0
        }
        Some("off") => match hm::off(ws, project.as_deref()) {
            Ok(marker) => {
                println!("history is off{}: no new record is written from now on, and nothing was deleted.", project.as_deref().map(|p| format!(" for project {p}")).unwrap_or_default());
                println!("To turn it on again, remove {}", marker.display());
                0
            }
            Err(e) => {
                eprintln!("delphi history off: {e}");
                1
            }
        },
        Some("purge") => {
            let since = match flag("--since").map(|s| parse_since(&s, now_unix())) {
                None => None,
                Some(Ok(t)) => Some(t),
                Some(Err(e)) => {
                    eprintln!("delphi history purge: {e}");
                    return 2;
                }
            };
            let targets = match &project {
                Some(p) => vec![p.clone()],
                None => hm::projects(ws),
            };
            if targets.is_empty() {
                println!("nothing is stored; nothing to erase.");
                return 0;
            }
            let what = if since.is_some() { "the records written since then" } else { "every record" };
            if !confirm(&format!("This erases {what} in {} project(s); a tombstone (id, time, keyed hash) stays for each.", targets.len()), args) {
                eprintln!("nothing was erased.");
                return 2;
            }
            let at = delphi_core::time::now_rfc3339();
            let (mut records, mut voided) = (0, 0);
            for p in &targets {
                match hm::erase(ws, p, since.map(Which::Since).unwrap_or(Which::All), &at) {
                    Ok(e) => {
                        records += e.records;
                        voided += e.voided;
                    }
                    Err(e) => {
                        eprintln!("delphi history purge: project {p}: {e}");
                        return 1;
                    }
                }
            }
            println!("erased {records} record(s){}; a tombstone stays for each, and the chain still verifies.", if voided > 0 { format!(" and {voided} unreadable line(s)") } else { String::new() });
            0
        }
        _ => {
            let [id] = positional(args)[..] else {
                eprintln!("{USAGE}");
                return 2;
            };
            // An id is per project: with several stores, say which.
            let target = match &project {
                Some(p) => p.clone(),
                None => match &hm::projects(ws)[..] {
                    [only] => only.clone(),
                    [] => {
                        eprintln!("delphi history delete: nothing is stored");
                        return 1;
                    }
                    _ => {
                        eprintln!("delphi history delete: several projects have a history; say which with --project <id>");
                        return 2;
                    }
                },
            };
            if !confirm(&format!("This erases record {} of project {target}; a tombstone (id, time, keyed hash) stays.", id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(16).collect::<String>()), args) {
                eprintln!("nothing was erased.");
                return 2;
            }
            match hm::erase(ws, &target, Which::Id(id), &delphi_core::time::now_rfc3339()) {
                Ok(_) => {
                    println!("erased record {}; a tombstone stays and the chain still verifies.", id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(16).collect::<String>());
                    0
                }
                Err(e) => {
                    eprintln!("delphi history delete: {e}");
                    1
                }
            }
        }
    }
}

/// The install question (`delphi init`, spec 102 §7 item 3). **Not shown today:** `SHOW_AT_INSTALL` is `false` until slice S8, so this
/// returns at once and prints nothing. When S8 flips it, a person at a terminal is asked once; an agent seat, a missing terminal or an
/// answer already given asks nothing.
pub fn ask_at_install(ws: &Path) {
    ask_with(ws, history_consent::SHOW_AT_INSTALL, std::io::stdin().is_terminal() && std::io::stdout().is_terminal(), crate::xenia_terminal::inside_an_agent_seat(), &mut std::io::stdin().lock(), &mut std::io::stdout());
}

/// The same with every input given, so a test can turn the switch on and answer from a buffer. Returns whether the question was shown.
pub(crate) fn ask_with(ws: &Path, show: bool, terminal: bool, agent_seat: bool, input: &mut dyn BufRead, out: &mut dyn Write) -> bool {
    let Some(question) = history_consent::question_to_show(ws, show) else { return false };
    let Ok(human) = Human::at_terminal(terminal, agent_seat, false) else { return false };
    let _ = write!(out, "{question} ");
    let _ = out.flush();
    let mut line = String::new();
    let answer = if input.read_line(&mut line).is_ok() && matches!(line.trim().to_ascii_lowercase().as_str(), "n" | "no") { Answer::No } else { Answer::Yes };
    if let Err(e) = history_consent::record_answer(ws, &human, answer) {
        let _ = writeln!(out, "delphi: history: {e}");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_takes_a_duration_a_date_or_a_time_and_refuses_anything_else() {
        let now = delphi_core::time::parse_rfc3339("2026-11-12T12:00:00Z").unwrap();
        assert_eq!(parse_since("1d", now).unwrap(), now - 86_400);
        assert_eq!(parse_since("2h", now).unwrap(), now - 7_200);
        assert_eq!(parse_since("2026-11-01", now).unwrap(), delphi_core::time::parse_rfc3339("2026-11-01T00:00:00Z").unwrap());
        assert_eq!(parse_since("2026-11-01T09:30:00Z", now).unwrap(), delphi_core::time::parse_rfc3339("2026-11-01T09:30:00Z").unwrap());
        let e = parse_since("yesterday\u{1b}[31m", now).unwrap_err();
        assert!(e.contains("is not a time") && !e.contains('\u{1b}'), "{e:?}");
    }

    #[test]
    fn nothing_is_asked_today_and_with_the_switch_on_only_a_human_at_a_terminal_is() {
        let ws = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        // The real switch is off: nothing is printed or read, whatever the terminal says.
        assert!(!ask_with(ws.path(), history_consent::SHOW_AT_INSTALL, true, false, &mut &b"n\n"[..], &mut out));
        assert!(out.is_empty());
        // Switch on, but an agent seat or no terminal: not asked, not answered.
        assert!(!ask_with(ws.path(), true, true, true, &mut &b"n\n"[..], &mut out));
        assert!(!ask_with(ws.path(), true, false, false, &mut &b"n\n"[..], &mut out));
        assert!(out.is_empty() && history_consent::answered(ws.path()).is_none());
        // Switch on and a human: asked once, and the answer is recorded.
        assert!(ask_with(ws.path(), true, true, false, &mut &b"n\n"[..], &mut out));
        assert!(String::from_utf8(out.clone()).unwrap().starts_with("Keep a history of your prompts"));
        assert_eq!(history_consent::answered(ws.path()), Some(Answer::No));
        assert!(!ask_with(ws.path(), true, true, false, &mut &b"y\n"[..], &mut out), "asked once");
    }

    #[test]
    fn a_bare_enter_is_yes_as_the_spec_words_it() {
        let ws = tempfile::tempdir().unwrap();
        assert!(ask_with(ws.path(), true, true, false, &mut &b"\n"[..], &mut Vec::new()));
        assert_eq!(history_consent::answered(ws.path()), Some(Answer::Yes));
    }

    #[test]
    fn the_positional_argument_skips_flag_values() {
        let a: Vec<String> = ["delete", "ex_0002", "--project", "home", "--yes"].iter().map(|s| s.to_string()).collect();
        assert_eq!(positional(&a), vec![&"ex_0002".to_string()]);
    }
}
