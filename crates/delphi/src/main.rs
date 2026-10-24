//! `delphi`: roadmap Step 1 commands (`specs/001`, `specs/006`).

mod avatar;
mod chat;
mod console;
mod doctor;
mod frontmatter;
mod home;
mod lint;
mod needs;
mod observer;
mod ratify_console;
mod report;
mod seed;
mod selfupdate;
mod setup;
mod start;
mod theme;
mod update;

use std::path::PathBuf;
use std::process::exit;

const USAGE: &str = "usage:
  delphi [--no-onboard] [--plain] [--workspace <name>] [--home <dir>]   (set up whatever is missing, then open the console)
  delphi update [--check] [--allow-downgrade] [--workspace <name>] [--home <dir>]
  delphi rollback [--workspace <name>] [--home <dir>]
  delphi self-update [--check] [--version vX.Y.Z] [--rollback]
  delphi setup [--yes] [--check] [--workspace <name>] [--home <dir>]
  delphi init [--non-interactive] [--workspace <name>] [--project <path>]... [--theme <name>] [--home <dir>]
  delphi doctor [--fix] [--workspace <name>] [--home <dir>] [--require-providers]
  delphi lint --repo <path>
  delphi chat [--plain] [--workspace <name>] [--mission <id>] [--home <dir>]
  delphi endpoints test [<endpoint id>] [--probe] [--workspace <name>] [--home <dir>]
  delphi models [--probe] [--workspace <name>] [--home <dir>]
  delphi auth login <name> [--manual] | status | logout <name> [--workspace <name>] [--home <dir>]
  delphi connectors status | test <name> | pin <name> [--workspace <name>] [--home <dir>]
  delphi needs [add <item> <why>] [--workspace <name>] [--home <dir>]
  delphi kb index | reindex | stats | search <query> [--role <role>] [--untrusted] [--workspace <name>] [--home <dir>]
  delphi observe [council|epic|forge|agents|spend|archives] [--once] [--workspace <name>] [--home <dir>]   (also: delphi narrator)
  delphi daemon status|stop|restart [--workspace <name>] [--home <dir>]
  delphi --version";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        None => cmd_start(&args),
        Some(a) if a.starts_with("--") && a != "--version" => cmd_start(&args),
        // The daemon and its maintenance commands run from this same executable.
        Some("__core") => delphi_engine::core_main::run(args[1..].to_vec()),
        Some("lint") => cmd_lint(&args[1..]),
        Some("setup") => cmd_setup(&args[1..]),
        Some("self-update") => cmd_self_update(&args[1..]),
        Some("update") => cmd_update(&args[1..], false),
        Some("rollback") => cmd_update(&args[1..], true),
        Some("init") => cmd_init(&args[1..]),
        Some("doctor") => cmd_doctor(&args[1..]),
        Some("chat") => cmd_chat(&args[1..]),
        Some("daemon") => cmd_daemon(&args[1..]),
        Some("kb") => cmd_kb(&args[1..]),
        Some("needs") => cmd_needs(&args[1..]),
        Some("connectors") => cmd_core("connectors", &args[1..]),
        Some("narrator") | Some("observe") => cmd_narrator(&args[1..]),
        Some("auth") => cmd_core("auth", &args[1..]),
        Some("endpoints") => cmd_core("endpoints", &args[1..]),
        Some("models") => cmd_core("models", &args[1..]),
        Some("--version") | Some("version") => {
            println!("delphi {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    };
    exit(code);
}

/// Value of `--flag <value>` in `args`.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn cmd_lint(args: &[String]) -> i32 {
    let root = match flag(args, "--repo") {
        Some(p) => PathBuf::from(p),
        None if args.iter().any(|a| a == "--repo") || args.is_empty() => PathBuf::from("."),
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

/// `delphi self-update`: replace this executable with the latest release (see `selfupdate.rs`).
fn cmd_self_update(args: &[String]) -> i32 {
    let Ok(dest) = std::env::current_exe().and_then(std::fs::canonicalize) else {
        eprintln!("delphi self-update: cannot locate this executable");
        return 1;
    };
    let api = std::env::var("DELPHI_RELEASES_URL").unwrap_or_else(|_| selfupdate::DEFAULT_API.to_string());
    selfupdate::run(args, &selfupdate::RealNet, &dest, &api)
}

/// `delphi update` / `delphi rollback`: refresh (or restore) the built-in Zone 1 files of the workspaces.
fn cmd_update(args: &[String], rollback: bool) -> i32 {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let only = flag(args, "--workspace");
    update::run(&home, only.as_deref(), args.iter().any(|a| a == "--check"), args.iter().any(|a| a == "--allow-downgrade"), rollback)
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
            println!("  next: configure model access (specs/009), then run `delphi`");
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
    if on_terminal && !args.iter().any(|a| a == "--plain") {
        console::run(&home, &ws.dir, flag(args, "--mission"), names)
    } else {
        chat::run(&home, &ws.dir, flag(args, "--mission"), names)
    }
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
    let view = match args.iter().find(|a| !a.starts_with("--") && Some(a.as_str()) != flag(args, "--workspace").as_deref() && Some(a.as_str()) != flag(args, "--home").as_deref()) {
        Some(v) => match observer::View::parse(v) {
            Some(v) => Some(v),
            None => {
                eprintln!("delphi narrator: unknown view `{}` (council, epic, forge, agents, spend or archives)", v);
                return 2;
            }
        },
        None => None,
    };
    // The full-screen Observer (D-28) on a terminal, unless asked for the classic one or a single text frame.
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if on_terminal && !args.iter().any(|a| a == "--once" || a == "--classic") {
        if let Some(ui) = ui_executable() {
            let code = chat::launch_ui_observer(&ui, &home, &ws.dir, view.map(observer::View::name), &theme_names(&ws));
            if code != 3 {
                return code;
            }
        }
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
    let (_home, ws) = match resolve_ws("kb", args) {
        Ok(v) => v,
        Err(code) => return code,
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
    delphi_engine::kb::run(&ws.dir, &rest)
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
