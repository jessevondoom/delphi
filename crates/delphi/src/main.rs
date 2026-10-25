//! `delphi`: roadmap Step 1 commands (`specs/001`, `specs/006`).

mod avatar;
mod check;
mod chat;
mod console;
mod decisions;
mod doctor;
mod escalation;
// Moved to the engine so the daemon's `check` tool and the CLI share one implementation.
mod frontmatter {
    pub use delphi_engine::frontmatter::*;
}
mod home;
mod lint;
mod needs;
mod xenia_cmd;
mod observer;
mod ratify_console;
mod relay;
mod report;
mod run;
mod seed;
mod selfupdate;
mod setup;
mod start;
mod theme;
mod timers_cmd;
mod update;

use std::path::PathBuf;
use std::process::exit;

const USAGE: &str = "usage:
  delphi [--no-onboard] [--plain] [--workspace <name>] [--home <dir>]   (set up whatever is missing, then open the console)
  delphi update [--check] [--version vX.Y.Z | --dev] [--rollback] [--no-restart] [--allow-downgrade] [--workspace <name>] [--home <dir>]   (the program, then the workspaces' built-in files, then running daemons; --dev: the latest build of main; --rollback undoes all three)
  delphi setup [--yes] [--check] [--workspace <name>] [--home <dir>]
  delphi init [--non-interactive] [--workspace <name>] [--project <path>]... [--theme <name>] [--home <dir>]
  delphi doctor [--fix] [--workspace <name>] [--home <dir>] [--require-providers]
  delphi lint --repo <path>
  delphi check <dag|handoff|provenance|rules|review-md|spec|kb|tui|skill> <path> [--repo <path>]
  delphi decisions [list|show <id>|ratify <id…|all-routine>|reject <id> [why]] [--mission <id>] [--workspace <name>] [--home <dir>]   (review an optimistic build's provisional decisions)
  delphi timers [list | set <role> (--in <5m> | --at <time>) [--every <5m>] <note…> | cancel <id>] [--mission <id>] [--workspace <name>] [--home <dir>]   (the wakes the daemon is keeping for roles)
  delphi relay [check|totp-enroll|totp-check <code>] [--workspace <name>] [--home <dir>]   (send approval cards to your email or Telegram and take your replies; run it in a terminal or tmux pane)
  delphi run \"<task>\" [--on-card deny|policy:<file>] [--permissions <mode>] [--budget <usd>] [--timeout <dur>] [--json|--jsonl] [--mission <id>] [--keep-daemon] [--workspace <name>] [--home <dir>]   (no terminal needed; exit 0 ok · 1 gates · 2 card denied · 3 budget · 4 infrastructure)
  delphi chat [--plain] [--inline] [--no-mouse] [--workspace <name>] [--mission <id>] [--home <dir>]
  delphi endpoints test [<endpoint id>] [--probe] [--workspace <name>] [--home <dir>]
  delphi models [--probe] [--workspace <name>] [--home <dir>]
  delphi auth login <name> [--manual] | status | logout <name> [--workspace <name>] [--home <dir>]
  delphi connectors status | test <name> | pin <name> [--workspace <name>] [--home <dir>]
  delphi needs [add <item> <why>] [--workspace <name>] [--home <dir>]
  delphi kb index | reindex | stats | search <query> [--role <role>] [--untrusted] [--workspace <name>] [--home <dir>]
  delphi observe [council|epic|forge|agents|spend|archives|thinking] [--once] [--workspace <name>] [--home <dir>]   (also: delphi narrator)
  delphi daemon status|stop|restart [--workspace <name>] [--home <dir>]
  delphi xenia whoami [--home <dir>]
  delphi help [<command>]   (also: <command> --help)
  delphi --version";

/// Flags that take a value, so the word after one is not the command (`delphi --home X doctor`).
const VALUE_FLAGS: &[&str] = &["--home", "--workspace"];

/// The first word that is not a flag or a flag's value: the command, wherever the flags put it.
fn command_index(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if VALUE_FLAGS.contains(&a) {
            i += 2;
        } else if a.starts_with('-') && a != "-" {
            i += 1;
        } else {
            return Some(i);
        }
    }
    None
}

/// The usage lines of one command (`delphi <cmd> …`), from the one table above.
fn usage_of(cmd: &str) -> Option<String> {
    let prefix = format!("  delphi {}", cmd);
    let lines: Vec<&str> = USAGE.lines().filter(|l| l.starts_with(&prefix) && l[prefix.len()..].chars().next().is_none_or(|c| c == ' ')).collect();
    if lines.is_empty() { None } else { Some(format!("usage:\n{}", lines.join("\n"))) }
}

const COMMANDS: &[&str] = &[
    "lint", "check", "setup", "update", "init", "doctor", "chat", "run", "relay", "decisions", "timers", "daemon", "kb", "needs", "xenia", "connectors", "narrator", "observe", "auth",
    "endpoints", "models", "version", "help",
];

/// Help is answered before anything runs: it never creates a workspace, starts a daemon or asks a question.
/// Returns the exit code when `args` asked for help.
fn help_requested(args: &[String]) -> Option<i32> {
    let ci = command_index(args);
    if let Some(i) = ci.filter(|&i| args[i] == "help") {
        return Some(print_help(args.get(i + 1).filter(|s| !s.starts_with('-')).map(|s| s.as_str())));
    }
    // Only words before the command's own arguments count, so `delphi run "fix -h"` still runs.
    let before = &args[..ci.unwrap_or(args.len())];
    let after = &args[ci.map_or(args.len(), |i| i + 1)..];
    let asked = |a: &String| a == "--help" || a == "-h";
    let free_text = ci.is_some_and(|i| args[i] == "run");
    if before.iter().any(asked) || after.iter().filter(|a| !free_text || a.starts_with('-')).any(asked) {
        return Some(print_help(ci.map(|i| args[i].as_str())));
    }
    None
}

fn print_help(cmd: Option<&str>) -> i32 {
    match cmd {
        None => {
            println!("{}", USAGE);
            0
        }
        Some(c) => match usage_of(c) {
            Some(u) => {
                println!("{}", u);
                0
            }
            None if c == "narrator" => print_help(Some("observe")),
            None if c == "version" => print_help(None),
            None => {
                eprintln!("delphi: no command `{}`; `delphi --help` lists them", c);
                2
            }
        },
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) != Some("__core") {
        if let Some(code) = help_requested(&args) {
            exit(code);
        }
    }
    // Flags may come before the command; the command's own handler reads them from anywhere.
    let ci = command_index(&args);
    let cmd = ci.map(|i| args[i].clone());
    let rest: Vec<String> = match ci {
        Some(i) => args.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, a)| a.clone()).collect(),
        None => args.clone(),
    };
    let args = rest;
    let code = match cmd.as_deref() {
        None if args.iter().any(|a| a == "--version") => {
            println!("delphi {}", env!("CARGO_PKG_VERSION"));
            0
        }
        None => cmd_start(&args),
        // The daemon and its maintenance commands run from this same executable.
        Some("__core") if ci == Some(0) => {
            // The daemon reports the release of this program, not of the library crate it is built from, so a client
            // can tell a daemon left over from an older release.
            // SAFETY: set before any thread exists.
            unsafe { std::env::set_var("DELPHI_RELEASE_VERSION", env!("CARGO_PKG_VERSION")) };
            delphi_engine::core_main::run(args.clone())
        }
        Some("lint") => cmd_lint(&args),
        Some("check") => cmd_check(&args),
        Some("setup") => cmd_setup(&args),
        Some("update") => cmd_update(&args),
        Some("init") => cmd_init(&args),
        Some("doctor") => cmd_doctor(&args),
        Some("chat") => cmd_chat(&args),
        Some("run") => cmd_run(&args),
        Some("relay") => cmd_relay(&args),
        Some("decisions") => cmd_decisions(&args),
        Some("timers") => cmd_timers(&args),
        Some("daemon") => cmd_daemon(&args),
        Some("kb") => cmd_kb(&args),
        Some("needs") => cmd_needs(&args),
        Some("xenia") => xenia_cmd::run(&args),
        Some("connectors") => cmd_core("connectors", &args),
        Some("narrator") | Some("observe") => cmd_narrator(&args),
        Some("auth") => cmd_core("auth", &args),
        Some("endpoints") => cmd_core("endpoints", &args),
        Some("models") => cmd_core("models", &args),
        Some("version") => {
            println!("delphi {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some(other) => {
            eprintln!("delphi: unknown command `{}`; `delphi --help` lists them", other);
            eprintln!("commands: {}", COMMANDS.join(" "));
            2
        }
    };
    exit(code);
}

/// Value of `--flag <value>` in `args`.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// `delphi check <kind> <path>`: the deterministic helpers behind the built-in skills (see `check.rs`). Reads files only.
fn cmd_check(args: &[String]) -> i32 {
    let positional: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || args[*i - 1] != "--repo")).map(|(_, a)| a).collect();
    let [kind, target] = positional[..] else {
        eprintln!("{}", USAGE);
        return 2;
    };
    let repo = flag(args, "--repo").map(PathBuf::from);
    match check::run(kind, std::path::Path::new(target), repo.as_deref()) {
        Ok(report) => {
            report.print();
            report.exit_code()
        }
        Err(e) => {
            eprintln!("delphi check: {}", e);
            2
        }
    }
}

fn cmd_lint(args: &[String]) -> i32 {
    let root = match flag(args, "--repo") {
        Some(p) => PathBuf::from(p),
        None if args.is_empty() && std::env::current_dir().ok().and_then(|d| home::git_root(&d)).is_some() => PathBuf::from("."),
        None if args.is_empty() => {
            eprintln!("delphi lint: not in a git repository; pass --repo <path>");
            return 2;
        }
        None if args.iter().any(|a| a == "--repo") => PathBuf::from("."),
        None => {
            eprintln!("{}", USAGE);
            return 2;
        }
    };
    let tree = match lint::Tree::from_repo(&root) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi lint: cannot read {}: {}", root.display(), e);
            return 2;
        }
    };
    let report = lint::run(&tree);
    report.print();
    println!(
        "delphi lint: {} file(s), {} fail, {} warn",
        tree.files.len(),
        report.count(report::Level::Fail),
        report.count(report::Level::Warn)
    );
    report.exit_code()
}

/// All values of a repeatable flag.
fn flags(args: &[String], name: &str) -> Vec<String> {
    args.iter().enumerate().filter(|(_, a)| *a == name).filter_map(|(i, _)| args.get(i + 1).cloned()).collect()
}

fn prompt(question: &str, default: &str) -> String {
    use std::io::{BufRead, Write};
    print!("{} [{}]: ", question, default);
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    let v = line.trim();
    if v.is_empty() { default.to_string() } else { v.to_string() }
}

/// `delphi update`: bring everything to the newest release, in one command. The program is replaced first (see
/// `selfupdate.rs`); then the freshly installed program refreshes the built-in agents, skills and themes in the
/// workspaces (they are compiled into it, so the old program would write the old files) and restarts any running
/// daemon on itself. If the program is already the newest, only the workspace files are refreshed.
/// `--rollback` puts the previous program and the previous workspace files back. `--workspaces-only` is the
/// workspace step alone, which is how the new program is asked to do it.
fn cmd_update(args: &[String]) -> i32 {
    let has = |n: &str| args.iter().any(|a| a == n);
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let only = flag(args, "--workspace");
    if has("--workspaces-only") {
        return update::run(&home, only.as_deref(), has("--check"), has("--allow-downgrade"), has("--rollback"));
    }
    let Ok(dest) = std::env::current_exe().and_then(std::fs::canonicalize) else {
        eprintln!("delphi update: cannot locate this executable");
        return 1;
    };
    let api = std::env::var("DELPHI_RELEASES_URL").unwrap_or_else(|_| selfupdate::DEFAULT_API.to_string());
    let mut workspace_code = 0;
    let code = selfupdate::run_with(args, &selfupdate::RealNet, &dest, &api, &mut |event| {
        if event == selfupdate::Event::UpToDate {
            workspace_code = update::run(&home, only.as_deref(), has("--check"), has("--allow-downgrade"), false);
            return;
        }
        let mut step = vec!["update".to_string(), "--workspaces-only".into(), "--home".into(), home.to_string_lossy().to_string()];
        step.extend(only.iter().flat_map(|w| ["--workspace".to_string(), w.clone()]));
        step.extend(["--allow-downgrade"].iter().filter(|f| has(f)).map(|f| f.to_string()));
        if event == selfupdate::Event::RolledBack {
            step.push("--rollback".into());
        }
        workspace_code = std::process::Command::new(&dest).args(&step).stdin(std::process::Stdio::null()).status().ok().and_then(|s| s.code()).unwrap_or(1);
        if !has("--no-restart") {
            restart_daemons(&dest, &home, only.as_deref());
        }
    });
    // No workspace yet is not a failure of the update, and neither is a rollback with nothing to restore there.
    if code != 0 || workspace_code == 2 || has("--rollback") { code } else { workspace_code }
}

/// Restart every (or the named) workspace's running daemon with the freshly installed `exe`, so none keeps running the
/// old release. Each is restarted by that program itself (`daemon restart`).
fn restart_daemons(exe: &std::path::Path, home: &std::path::Path, only: Option<&str>) {
    let Ok(rd) = std::fs::read_dir(home.join("workspaces")) else { return };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|d| d.join("config.toml").exists()).collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let running = only.is_none_or(|o| o == name) && delphi_core::spawn::layout(home, &dir).is_ok_and(|l| delphi_core::client::Client::connect(&l.sock).is_ok());
        if !running {
            continue;
        }
        println!("Restarting the daemon of workspace `{}`…", name);
        let ok = std::process::Command::new(exe).args(["daemon", "restart", "--workspace", &name, "--home"]).arg(home).stdin(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
        if !ok {
            eprintln!("delphi update: could not restart it; run `delphi daemon restart --workspace {}`", name);
        }
    }
}

/// `delphi setup`: detect, ask, write, ratify, prove (see `setup.rs`).
fn cmd_setup(args: &[String]) -> i32 {
    let o = setup::Opts {
        home: home::delphi_home(flag(args, "--home").as_deref()),
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        workspace: flag(args, "--workspace"),
        yes: args.iter().any(|a| a == "--yes" || a == "-y"),
        check: args.iter().any(|a| a == "--check"),
    };
    match setup::run(&mut setup::TerminalUi { auto: o.yes }, &setup::RealEnv, &o) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("delphi setup: {:#}", e);
            1
        }
    }
}

fn cmd_init(args: &[String]) -> i32 {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut projects: Vec<PathBuf> = flags(args, "--project").iter().map(PathBuf::from).collect();
    if projects.is_empty() {
        if let Some(root) = home::git_root(&cwd) {
            projects.push(root);
        }
    }
    let projects: Vec<PathBuf> = projects.iter().map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())).collect();
    let default_name = projects
        .first()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".into());
    let interactive = !args.iter().any(|a| a == "--non-interactive");
    let mut name = flag(args, "--workspace").unwrap_or(default_name);
    let mut theme = flag(args, "--theme").unwrap_or_else(|| "greek".into());
    if interactive {
        name = prompt("Workspace name", &name);
        theme = prompt("Theme (built-in: greek)", &theme);
    }
    match home::init(&home::InitOptions { home: home.clone(), workspace: name, projects: projects.clone(), theme }) {
        Ok(ws) => {
            println!("delphi init: workspace `{}` created at {}", ws.name, ws.dir.display());
            for p in &projects {
                println!("  project: {} (unchanged; Delphi writes nothing into projects)", p.display());
            }
            println!("  next: run `delphi setup` to connect a model, then `delphi`");
            0
        }
        Err(e) => {
            eprintln!("delphi init: {:#}", e);
            1
        }
    }
}

/// Find the workspace for a command: `--workspace`, `DELPHI_WORKSPACE`, or the project containing the
/// current directory.
fn resolve_ws(cmd: &str, args: &[String]) -> Result<(PathBuf, home::Workspace), i32> {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let name = match home::resolve_workspace(&home, &cwd, flag(args, "--workspace")) {
        home::Resolve::Found(n) => n,
        home::Resolve::Ambiguous(v) => {
            eprintln!("delphi {}: several workspaces register this project ({}); pass --workspace or set DELPHI_WORKSPACE", cmd, v.join(", "));
            return Err(2);
        }
        home::Resolve::NoneRegistered(root) => {
            eprintln!("delphi {}: no workspace registers {}; run `delphi init --project {}`", cmd, root.display(), root.display());
            return Err(2);
        }
        // Outside any project, the workspace that bare `delphi` made for that case, if it exists.
        home::Resolve::NotInGitRepo if home.join("workspaces").join(start::DEFAULT_WORKSPACE).join("config.toml").exists() => start::DEFAULT_WORKSPACE.to_string(),
        home::Resolve::NotInGitRepo => {
            eprintln!("delphi {}: not inside a git repository; pass --workspace <name>, or run `delphi` to create the default workspace", cmd);
            return Err(2);
        }
        home::Resolve::NoSuchWorkspace(n) => {
            eprintln!("delphi {}: no workspace named `{}` in {}", cmd, n, home.display());
            return Err(2);
        }
    };
    let ws = home::Workspace { dir: home.join("workspaces").join(&name), name };
    Ok((home, ws))
}

/// Display names for the active theme, from its ratified pack (display only; behavior never depends on it).
fn theme_names(ws: &home::Workspace) -> std::collections::HashMap<String, String> {
    let cfg = std::fs::read_to_string(ws.dir.join("config.toml")).unwrap_or_default();
    let theme = toml::from_str::<toml::Value>(&cfg).ok().and_then(|v| v.get("theme").and_then(|t| t.get("name")).and_then(|n| n.as_str()).map(String::from)).unwrap_or_else(|| "greek".into());
    let text = std::fs::read_to_string(ws.dir.join("local/ratified/themes").join(&theme).join("theme.md")).unwrap_or_default();
    theme::parse(&text).map(|t| t.roles.into_iter().map(|(role, name, _, _)| (role, name)).collect()).unwrap_or_default()
}

/// The full-screen console (D-28): `$DELPHI_UI`, or `delphi-ui` next to this executable. Absent, the inline console runs.
fn ui_executable() -> Option<std::path::PathBuf> {
    let p = std::env::var_os("DELPHI_UI").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("delphi-ui"))))?;
    p.is_file().then_some(p)
}

fn cmd_chat(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("chat", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let names = theme_names(&ws);
    if args.iter().any(|a| a == "--no-mouse") {
        // SAFETY: set before any thread exists; the full-screen console reads it at start.
        unsafe { std::env::set_var("DELPHI_NO_MOUSE", "1") };
    }
    // The inline console on a terminal; the plain line console when piped, or when asked for with --plain.
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if on_terminal && !args.iter().any(|a| a == "--plain" || a == "--inline") {
        if let Some(ui) = ui_executable() {
            // Exit code 3: the console is not the same release as this program; the inline console takes over.
            let code = chat::launch_ui(&ui, &home, &ws.dir, flag(args, "--mission"), &names);
            if code != 3 {
                return code;
            }
        }
    }
    // An older `delphi self-update` replaced only this program: say what is missing and how to get it.
    let notice = (!args.iter().any(|a| a == "--inline") && ui_executable().is_none())
        .then_some("(the full-screen console, delphi-ui, is not installed next to this program, so this is the inline console. Install both: curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh)");
    if on_terminal && !args.iter().any(|a| a == "--plain") {
        console::run(&home, &ws.dir, flag(args, "--mission"), names, notice)
    } else {
        chat::run(&home, &ws.dir, flag(args, "--mission"), names)
    }
}

/// `delphi decisions …`: review an optimistic build's provisional decisions from a shell (`decisions.rs`).
fn cmd_decisions(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("decisions", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let rest: Vec<String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !matches!(args[*i - 1].as_str(), "--workspace" | "--home" | "--mission"))).map(|(_, a)| a.clone()).collect();
    decisions::cli(&home, &ws.dir, flag(args, "--mission"), &rest)
}

/// `delphi timers [list|set|cancel]`: the wakes the daemon is keeping (`timers_cmd.rs`, `specs/017`).
fn cmd_timers(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("timers", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    // `--in`, `--at` and `--every` belong to `set`; only the shared flags are taken out here.
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if matches!(a.as_str(), "--workspace" | "--home" | "--mission") {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    timers_cmd::cli(&home, &ws.dir, flag(args, "--mission"), &rest)
}

/// `delphi relay [check|totp-enroll|totp-check]`: the relay for approval cards (`relay/`, `specs/014`).
fn cmd_relay(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("relay", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let rest: Vec<String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !matches!(args[*i - 1].as_str(), "--workspace" | "--home"))).map(|(_, a)| a.clone()).collect();
    relay::cli::run(&home, &ws.dir, &rest)
}

/// `delphi run "<task>"`: one task with no terminal, ending in a summary and an exit code (`run.rs`).
fn cmd_run(args: &[String]) -> i32 {
    let opts = match run::parse_args(args) {
        Ok(o) => o,
        Err(m) => {
            eprintln!("delphi run: {}", m);
            return 4;
        }
    };
    let (home, ws) = match resolve_ws("run", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    run::run(&home, &ws.dir, opts)
}

fn cmd_daemon(args: &[String]) -> i32 {
    let Some(action) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("{}", USAGE);
        return 2;
    };
    let (home, ws) = match resolve_ws("daemon", &args[1..]) {
        Ok(v) => v,
        Err(code) => return code,
    };
    chat::daemon(&home, &ws.dir, action)
}

/// `delphi auth …` and `delphi endpoints …`, in this process.
fn cmd_core(sub: &str, args: &[String]) -> i32 {
    let (_home, ws) = match resolve_ws(sub, args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--workspace" || a == "--home" {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    match sub {
        "connectors" => delphi_engine::connectors_cli::run(&ws.dir, &rest),
        "auth" => delphi_engine::auth_cli::run(&ws.dir, &rest),
        "models" => delphi_engine::models_cli::run(&ws.dir, &rest),
        _ => delphi_engine::endpoints_cli::run(&ws.dir, &rest),
    }
}

fn cmd_narrator(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("narrator", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let view_arg = args.iter().find(|a| !a.starts_with("--") && Some(a.as_str()) != flag(args, "--workspace").as_deref() && Some(a.as_str()) != flag(args, "--home").as_deref());
    // `thinking` exists only in the full-screen Observer: live reasoning is context, kept in that window's memory.
    let thinking = view_arg.is_some_and(|v| matches!(v.to_lowercase().as_str(), "thinking" | "reasoning" | "7"));
    let view = match view_arg.filter(|_| !thinking) {
        Some(v) => match observer::View::parse(v) {
            Some(v) => Some(v),
            None => {
                eprintln!("delphi narrator: unknown view `{}` (council, epic, forge, agents, spend, archives or thinking)", v);
                return 2;
            }
        },
        None => None,
    };
    // The full-screen Observer (D-28) on a terminal, unless asked for the classic one or a single text frame.
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if on_terminal && !args.iter().any(|a| a == "--once" || a == "--classic") {
        if let Some(ui) = ui_executable() {
            let code = chat::launch_ui_observer(&ui, &home, &ws.dir, if thinking { Some("thinking") } else { view.map(observer::View::name) }, &theme_names(&ws));
            if code != 3 {
                return code;
            }
        }
    }
    if thinking {
        eprintln!("delphi observe thinking needs the full-screen Observer (the delphi-ui program, on a terminal)");
        return 1;
    }
    observer::run(&home, &ws.dir, view, theme_names(&ws), args.iter().any(|a| a == "--once"))
}

fn cmd_needs(args: &[String]) -> i32 {
    let (_home, ws) = match resolve_ws("needs", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--workspace" || a == "--home" {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    match rest.first().map(String::as_str) {
        None => {
            print!("{}", needs::render(&needs::report(&ws.dir)));
            0
        }
        Some("add") if rest.len() >= 2 => match needs::add(&ws.dir, &rest[1], &rest[2..].join(" ")) {
            Ok(()) => {
                println!("recorded in {}", ws.dir.join("local/needs.jsonl").display());
                0
            }
            Err(e) => {
                eprintln!("delphi needs: {:#}", e);
                2
            }
        },
        _ => {
            eprintln!("usage: delphi needs [add <item> <why>]  (items: {})", needs::item_ids().join(", "));
            2
        }
    }
}

fn cmd_kb(args: &[String]) -> i32 {
    // `eval` and `model` work on documents and model files, not on a workspace.
    let (home, ws_dir) = if matches!(args.first().map(String::as_str), Some("eval" | "model")) {
        (home::delphi_home(flag(args, "--home").as_deref()), PathBuf::new())
    } else {
        match resolve_ws("kb", args) {
            Ok((home, ws)) => (home, ws.dir),
            Err(code) => return code,
        }
    };
    // The flags that select the workspace are not part of the query.
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--workspace" || a == "--home" {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    delphi_engine::kb::run(&home, &ws_dir, &rest)
}

/// Bare `delphi`: create what is missing (workspace, model access), then open the console.
fn cmd_start(args: &[String]) -> i32 {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    let interactive = on_terminal && !args.iter().any(|a| a == "--no-onboard") && std::env::var_os("DELPHI_NO_ONBOARD").is_none();
    let ws = match start::preflight(&home, &cwd, flag(args, "--workspace"), interactive) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("delphi: {:#}", e);
            return 2;
        }
    };
    // From here it is `delphi chat` on the workspace that was just resolved.
    let mut rest: Vec<String> = args.iter().filter(|a| *a != "--no-onboard").cloned().collect();
    if flag(&rest, "--workspace").is_none() {
        rest.extend(["--workspace".into(), ws.name]);
    }
    cmd_chat(&rest)
}

fn cmd_doctor(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("doctor", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let opts = doctor::Options { require_providers: args.iter().any(|a| a == "--require-providers") };
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if args.iter().any(|a| a == "--fix") {
        if !on_terminal {
            eprintln!("delphi doctor --fix: needs a terminal (it asks before each fix); run `delphi doctor` for the report");
            return 2;
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        return start::doctor_fix(&mut setup::TerminalUi { auto: false }, &home, &ws, &cwd, &opts);
    }
    let report = doctor::run(&home, &ws, &opts);
    report.print();
    println!("delphi doctor: workspace `{}`: {}", ws.name, report.worst());
    report.exit_code()
}
