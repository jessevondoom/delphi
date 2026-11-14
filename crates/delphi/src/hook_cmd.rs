//! `delphi mcp --hook <event>` and `delphi mcp --hooks on|off|status` (`specs/105` §2, A2).
//!
//! `--hook` is what a host runs at its session events. It exits 0 always and prints nothing on any error, a missing workspace,
//! the switch off, or when its 2 s deadline passes (§2). It never echoes the operator's prompt. The hooks are on by default (§13);
//! `--hooks on` (restoring the default after `--hooks off`) is the operator's act at a terminal: stdin must be a terminal and
//! the operator types `yes`; there is no `--yes`. `--hooks off` needs neither.

use crate::home;
use delphi_engine::host_hooks;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// The workspace a hook serves, from the folder the host session runs in, quietly: `None` when there is none to serve.
fn workspace_for(args: &[String], cwd: &Path) -> Option<PathBuf> {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let default = || home.join("workspaces").join(crate::start::DEFAULT_WORKSPACE).join("config.toml").exists().then(|| crate::start::DEFAULT_WORKSPACE.to_string());
    let name = match home::resolve_workspace(&home, cwd, flag(args, "--workspace")) {
        home::Resolve::Found(n) => Some(n),
        home::Resolve::NoneRegistered(_) | home::Resolve::NotInGitRepo => default(),
        home::Resolve::Ambiguous(_) | home::Resolve::NoSuchWorkspace(_) => None,
    }?;
    let dir = home.join("workspaces").join(name);
    dir.join("config.toml").is_file().then_some(dir)
}

/// What one run of a hook prints, or `None` for nothing. Pure of the terminal: stdin is given.
pub fn hook_output(args: &[String], event: &str, host: &str, stdin: impl std::io::Read) -> Option<String> {
    let input = host_hooks::read_input(stdin)?;
    let cwd = input.cwd.clone().or_else(|| std::env::current_dir().ok())?;
    let ws = workspace_for(args, &cwd)?;
    if !host_hooks::switch_on(&ws) {
        return None;
    }
    // Spec 110 §4 item 3: counts only from a transcript format Delphi reads (Claude Code's); another host's marker has none.
    let root = if host == "claude-code" { host_hooks::claude_projects_root() } else { None };
    let text = match event {
        "session-start" => host_hooks::brief(&ws, &cwd, input.session_id.as_deref(), &host_hooks::nonce()),
        "prompt" => {
            // A4: the counter is the one workspace file besides the config a prompt with no `@` touches.
            let mut lines: Vec<String> = host_hooks::count_prompt(&ws, &input, host, &cwd, root.as_deref()).map(String::from).into_iter().collect();
            if input.prompt.as_deref().unwrap_or("").contains('@') {
                lines.extend(host_hooks::mention_lines(&ws, input.prompt.as_deref().unwrap_or(""), host));
                // Spec 106 §12 G6 item 2: the role this session adopts, for the status line (the role ID and a time, nothing else).
                if let (Some(role), Some(id)) = (host_hooks::mention_role(&ws, input.prompt.as_deref().unwrap_or("")), input.session_id.as_deref()) {
                    host_hooks::record_role(&ws, &role, Some((host, id)), None);
                }
            }
            lines.join("\n")
        }
        // Spec 106 §11 G2: only a starting subagent whose type is exactly one of the release's worker names gets a pack; any other
        // prints nothing. Claude Code is the one host whose subagent start can add context, so no other host is answered.
        "subagent-start" if host == "claude-code" => host_hooks::subagent_pack(&ws, input.agent_type.as_deref()?, host)?,
        "subagent-start" => return None,
        _ => {
            let marker = host_hooks::session_end(&ws, &input, host, &cwd, root.as_deref());
            // Spec 108 M3: one bounded call (2 s) to stage the transcript's notes when the project opted in; silent on any failure.
            if let Some(args) = host_hooks::ingest_args(&ws, &input, &cwd, root.as_deref(), marker.as_deref()) {
                if let Some(link) = delphi_engine::mcp_daemon::HostLink::for_workspace(&ws, &cwd) {
                    link.set_tool(Some(host));
                    let _ = link.call_within("host.ingest_transcript", &args, std::time::Duration::from_secs(2));
                }
            }
            String::new()
        }
    };
    host_hooks::host_output(host, event, &text)
}

/// `delphi mcp --hook <event> [--host <id>]`. Always 0.
pub fn run_hook(args: &[String], event: Option<&str>) -> i32 {
    let host = flag(args, "--host").unwrap_or_else(|| "claude-code".into());
    let Some(event) = event.filter(|e| host_hooks::EVENTS.contains(e)).map(String::from) else { return 0 };
    if !delphi_engine::mcp_hosts::HOSTS.contains(&host.as_str()) {
        return 0;
    }
    let args = args.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::panic::catch_unwind(|| hook_output(&args, &event, &host, std::io::stdin().lock())).ok().flatten();
        let _ = tx.send(out);
    });
    // Past the deadline nothing is printed; the worker is ended with the process.
    if let Ok(Some(out)) = rx.recv_timeout(host_hooks::DEADLINE) {
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(out.as_bytes()).and_then(|_| so.write_all(b"\n")).and_then(|_| so.flush());
    }
    0
}

/// What `delphi mcp --statusline` prints for the host's status-line input, or `None` (spec 106 §12 G6 item 2).
pub fn statusline_output(args: &[String], host: &str, no_color: bool, stdin: impl std::io::Read) -> Option<String> {
    let input = host_hooks::read_input(stdin)?;
    let cwd = input.cwd.clone()?;
    let ws = workspace_for(args, &cwd)?;
    host_hooks::statusline(&ws, &input, host, no_color)
}

/// `delphi mcp --statusline [--host <id>]`: one line for a host's status line, or nothing. Exits 0 always; nothing after the deadline.
pub fn run_statusline(args: &[String]) -> i32 {
    let host = flag(args, "--host").unwrap_or_else(|| "claude-code".into());
    if !delphi_engine::mcp_hosts::HOSTS.contains(&host.as_str()) {
        return 0;
    }
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let args = args.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::panic::catch_unwind(|| statusline_output(&args, &host, no_color, std::io::stdin().lock())).ok().flatten();
        let _ = tx.send(out);
    });
    if let Ok(Some(out)) = rx.recv_timeout(host_hooks::DEADLINE) {
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(out.as_bytes()).and_then(|_| so.write_all(b"\n")).and_then(|_| so.flush());
    }
    0
}

/// `delphi mcp --hooks on|off|status`.
pub fn switch(what: Option<&str>, ws_dir: &Path) -> i32 {
    match what {
        Some("status") => {
            let key = host_hooks::session_hooks_key(ws_dir);
            let state = match key {
                Some(true) if host_hooks::switch_on(ws_dir) => "on",
                Some(true) => "on, but silenced: [mcp] enabled = false",
                Some(false) => "off",
                None => "unknown: config.toml or its [mcp] table cannot be read (the hooks print nothing)",
            };
            println!("session hooks: {state} ([mcp] session_hooks in {})", ws_dir.join("config.toml").display());
            println!("hooks file: {}", hooks_file_line());
            0
        }
        Some("off") => match host_hooks::set_switch(ws_dir, false) {
            Ok(()) => {
                println!("session hooks: off. The host's hooks print nothing from now on. `delphi mcp --hooks on` turns them back on (on is the default).");
                0
            }
            Err(e) => {
                eprintln!("delphi mcp --hooks off: {e}");
                1
            }
        },
        Some("on") => {
            if crate::xenia_terminal::inside_an_agent_seat() {
                eprintln!("delphi mcp --hooks on: turning the session hooks on is a human act and is refused inside an agent seat");
                return 2;
            }
            if !std::io::stdin().is_terminal() {
                eprintln!("delphi mcp --hooks on: this needs you at a terminal (stdin is not one), and there is no --yes");
                return 2;
            }
            println!("At each session start, prompt and session end, your host will run `delphi mcp --hook <event>`. It reads Delphi's files, prints the team, how to address it, this project's spec names and statuses (as fenced data) and a pointer to the last session, resolves an @name you type, and at the end writes a marker of counts. It runs no model and approves nothing; at session end, for a project you opted in to transcript ingest (`delphi learn ingest --enable`), it asks the running daemon once to stage notes from the transcript at L0. `delphi mcp --hooks off` turns it off.");
            print!("Type yes to turn the session hooks on: ");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            if std::io::stdin().lock().read_line(&mut line).is_err() || line.trim() != "yes" {
                eprintln!("delphi mcp --hooks on: not turned on (no yes)");
                return 2;
            }
            match host_hooks::set_switch(ws_dir, true) {
                Ok(()) => {
                    println!("session hooks: on.");
                    println!("hooks file: {}", hooks_file_line());
                    0
                }
                Err(e) => {
                    eprintln!("delphi mcp --hooks on: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!("usage: delphi mcp --hooks on|off|status");
            2
        }
    }
}

/// Where the installed Claude Code plugin's hooks file is and whether it is this binary's (A7). Read-only.
pub fn hooks_file_line() -> String {
    let Some(h) = std::env::var("HOME").ok().filter(|h| Path::new(h).is_absolute()) else { return "unknown (HOME is unset)".into() };
    match delphi_engine::mcp_check::check_claude_hooks(Path::new(&h), &host_hooks::claude_hooks_json()) {
        Some((true, d)) => format!("ok: {d}"),
        Some((false, d)) => format!("not current: {d}"),
        None => "unknown (no Claude Code plugin folder of Delphi's could be read)".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_hook_with_no_workspace_bad_input_or_an_unknown_event_prints_nothing_and_exits_zero() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().to_str().unwrap();
        let input = format!("{{\"cwd\": \"{}\"}}", home);
        assert_eq!(hook_output(&a(&["--home", home]), "session-start", "claude-code", input.as_bytes()), None, "no workspace");
        assert_eq!(hook_output(&a(&["--home", home]), "session-start", "claude-code", &b"not json"[..]), None);
        assert_eq!(run_hook(&a(&["--home", home]), Some("bogus")), 0);
        assert_eq!(run_hook(&a(&["--home", home, "--host", "vim"]), Some("prompt")), 0);
        assert_eq!(run_hook(&a(&["--home", home]), None), 0);
    }

    /// Spec 106 §11 G2: the subagent-start hook obeys the switch, answers only an exact worker name and only for Claude Code.
    #[test]
    fn the_subagent_start_hook_prints_a_workers_pack_only_with_the_switch_on_and_for_claude_code() {
        let t = tempfile::tempdir().unwrap();
        let ws = delphi_engine::workspace::make_test_workspace(t.path());
        std::fs::write(ws.dir.join("config.toml"), "[mcp]\nsession_hooks = false\n").unwrap();
        let home = t.path().to_str().unwrap();
        let args = a(&["--home", home, "--workspace", "w"]);
        let input = |agent: &str| format!("{{\"session_id\": \"s1\", \"cwd\": \"{home}\", \"hook_event_name\": \"SubagentStart\", \"agent_id\": \"x\", \"agent_type\": \"{agent}\"}}");
        assert_eq!(hook_output(&args, "subagent-start", "claude-code", input("delphi:architect-worker").as_bytes()), None, "the switch is off");
        host_hooks::set_switch(&ws.dir, true).unwrap();
        let out = hook_output(&args, "subagent-start", "claude-code", input("delphi:architect-worker").as_bytes()).expect("a worker's pack");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let ctx = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
        assert!(ctx.starts_with("You are ") && ctx.contains("# Delphi agent pack: architect"), "{}", &ctx[..200.min(ctx.len())]);
        assert_eq!(hook_output(&args, "subagent-start", "claude-code", input("general-purpose").as_bytes()), None);
        assert_eq!(hook_output(&args, "subagent-start", "claude-code", input("evil:architect-worker").as_bytes()), None);
        assert_eq!(hook_output(&args, "subagent-start", "copilot", input("delphi-architect-worker").as_bytes()), None);
        assert_eq!(hook_output(&args, "subagent-start", "claude-code", &b"{\"cwd\": 3}"[..]), None, "bad input prints nothing");
    }

    /// Spec 106 §12 G6: a mention at the prompt records the session's role and the status line names it; nothing for another session.
    #[test]
    fn a_mention_at_the_prompt_names_the_role_on_the_status_line() {
        let t = tempfile::tempdir().unwrap();
        let ws = delphi_engine::workspace::make_test_workspace(t.path());
        std::fs::write(ws.dir.join("config.toml"), "[mcp]\nsession_hooks = true\nbadge_style = \"ansi\"\n").unwrap();
        let home = t.path().to_str().unwrap();
        let args = a(&["--home", home, "--workspace", "w"]);
        let status = |id: &str, nc: bool| statusline_output(&args, "claude-code", nc, format!("{{\"session_id\": \"{id}\", \"cwd\": \"{home}\"}}").as_bytes());
        assert_eq!(status("s1", false), None);
        let prompt = format!("{{\"session_id\": \"s1\", \"cwd\": \"{home}\", \"prompt\": \"@builder SECRET-TEXT fix it\"}}");
        let _ = hook_output(&args, "prompt", "claude-code", prompt.as_bytes());
        let line = status("s1", false).expect("the adopted role");
        assert!(line.starts_with("\u{1b}[1;38;2;245;158;11m") && line.ends_with("\u{1b}[0m") && !line.contains("SECRET"), "{line:?}");
        assert!(!status("s1", true).unwrap().contains('\u{1b}'), "NO_COLOR");
        assert_eq!(status("s2", false), None);
    }

    /// Spec 105 §13: the hooks are on by default. A workspace whose config has no `session_hooks` key prints the brief;
    /// `session_hooks = false`, `[mcp] enabled = false` and an `[mcp]` table that cannot be read print nothing.
    #[test]
    fn a_workspace_with_no_session_hooks_key_prints_the_brief_and_each_way_to_silence_it_prints_nothing() {
        let t = tempfile::tempdir().unwrap();
        let ws = delphi_engine::workspace::make_test_workspace(t.path());
        let home = t.path().to_str().unwrap();
        let args = a(&["--home", home, "--workspace", "w"]);
        let input = format!("{{\"session_id\": \"s1\", \"cwd\": \"{home}\", \"hook_event_name\": \"SessionStart\"}}");
        let run = |cfg: &str| {
            std::fs::write(ws.dir.join("config.toml"), cfg).unwrap();
            hook_output(&args, "session-start", "claude-code", input.as_bytes())
        };
        for cfg in ["", "[mcp]\n", "[mcp]\nbadge_ascii = false\n", "[mcp]\nsession_hooks = true\n"] {
            let out = run(cfg).unwrap_or_else(|| panic!("the brief is printed for {cfg:?}"));
            assert!(out.contains("Delphi"), "{out}");
        }
        assert_eq!(run("[mcp]\nsession_hooks = false\n"), None, "an explicit false is off");
        assert_eq!(run("[mcp]\nenabled = false\n"), None, "[mcp] enabled = false silences the hooks although the default is on");
        assert_eq!(run("[mcp]\nbadge_style = \"neon\"\n"), None, "an unreadable [mcp] is silent");
        assert_eq!(run("[mcp\n"), None, "a config that cannot be parsed is silent");
    }

    #[test]
    fn hooks_on_is_refused_without_a_terminal_and_a_bad_word_is_usage() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("config.toml"), "[mcp]\nsession_hooks = false\n").unwrap();
        if !std::io::stdin().is_terminal() {
            assert_eq!(switch(Some("on"), t.path()), 2);
            assert!(!host_hooks::switch_on(t.path()), "nothing was written");
        }
        assert_eq!(switch(Some("maybe"), t.path()), 2);
        assert_eq!(switch(Some("off"), t.path()), 0);
        assert_eq!(host_hooks::session_hooks_key(t.path()), Some(false));
    }
}
