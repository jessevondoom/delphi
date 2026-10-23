//! `delphi`: roadmap Step 1 commands (`specs/001`, `specs/006`).

mod doctor;
mod frontmatter;
mod home;
mod lint;
#[allow(dead_code)] // used by the daemon file tools from Step 2; tested now (specs/001 §1.2)
mod pathguard;
mod report;
mod seed;
mod theme;

use std::path::PathBuf;
use std::process::exit;

const USAGE: &str = "usage:
  delphi init [--non-interactive] [--workspace <name>] [--project <path>]... [--theme <name>] [--home <dir>]
  delphi doctor [--workspace <name>] [--home <dir>] [--require-providers]
  delphi lint --repo <path>
  delphi --version";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        Some("lint") => cmd_lint(&args[1..]),
        Some("init") => cmd_init(&args[1..]),
        Some("doctor") => cmd_doctor(&args[1..]),
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
            println!("  next: configure model access (specs/009), then run `delphi doctor`");
            0
        }
        Err(e) => {
            eprintln!("delphi init: {:#}", e);
            1
        }
    }
}

fn cmd_doctor(args: &[String]) -> i32 {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let name = match home::resolve_workspace(&home, &cwd, flag(args, "--workspace")) {
        home::Resolve::Found(n) => n,
        home::Resolve::Ambiguous(v) => {
            eprintln!("delphi doctor: several workspaces register this project ({}); pass --workspace or set DELPHI_WORKSPACE", v.join(", "));
            return 2;
        }
        home::Resolve::NoneRegistered(root) => {
            eprintln!("delphi doctor: no workspace registers {}; run `delphi init --project {}`", root.display(), root.display());
            return 2;
        }
        home::Resolve::NotInGitRepo => {
            eprintln!("delphi doctor: not inside a git repository; pass --workspace <name>");
            return 2;
        }
        home::Resolve::NoSuchWorkspace(n) => {
            eprintln!("delphi doctor: no workspace named `{}` in {}", n, home.display());
            return 2;
        }
    };
    let ws = home::Workspace { dir: home.join("workspaces").join(&name), name };
    let opts = doctor::Options { require_providers: args.iter().any(|a| a == "--require-providers") };
    let report = doctor::run(&home, &ws, &opts);
    report.print();
    println!("delphi doctor: workspace `{}`: {}", ws.name, report.worst());
    report.exit_code()
}
