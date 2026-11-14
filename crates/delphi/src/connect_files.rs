//! Spec 110 X2: `delphi connect codex|gemini [--dry-run | --status | --remove]`.
//!
//! Writes Delphi's own files (under Delphi's folder for the host, and Codex's agent files under the `delphi-` prefix) and nothing
//! else without a question. The host's own command that adds the server, and the marked block in the operator's instruction file,
//! happen only when stdin and stdout are a terminal and the operator types `yes` to each; there is no override (amendment A2).
//! Without a terminal the command prints the command and the block. The host's folder is the host's own variable (`CODEX_HOME`,
//! `GEMINI_CLI_HOME`), else under `HOME`.

use std::collections::BTreeMap;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use delphi_engine::host_install::{self as hi, Status};

use crate::connect_cmd::sanitize;

/// The fixed directories the host CLI is looked for in (relative ones under `HOME`); the operator's `PATH` is never a source.
const DIRS: &[&str] = &[".local/bin", "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

pub fn handles(host: &str) -> bool {
    hi::FILE_HOSTS.contains(&host)
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn at_terminal() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal() && !crate::xenia_terminal::inside_an_agent_seat()
}

/// Ask one question; true only on a typed `yes`.
fn yes(question: &str) -> bool {
    print!("{question} Type yes: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == "yes"
}

/// Run the host's own command (fixed argv, no shell) from the host CLI found in the fixed directories, with a small environment that
/// keeps the host's own folder variable.
fn run_host(home: &Path, argv: &[String]) -> Result<(), String> {
    let dirs: Vec<PathBuf> = DIRS.iter().map(|d| if d.starts_with('/') { PathBuf::from(d) } else { home.join(d) }).collect();
    let exe = crate::connect_cmd::find_host(&dirs, &argv[0])?;
    let mut cmd = std::process::Command::new(&exe.canonical);
    cmd.args(&argv[1..]).env_clear().env("PATH", "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin").env("HOME", home);
    for k in ["CODEX_HOME", "GEMINI_CLI_HOME", "LANG", "TERM"] {
        if let Some(v) = env(k) {
            cmd.env(k, v);
        }
    }
    let status = cmd.status().map_err(|e| format!("{}: {e}", argv[0]))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{}` exited with {status}", argv.join(" ")))
    }
}

/// `delphi mcp --check <host>` lines for these hosts: server, instruction section, role files and hooks.
pub fn check_lines(host: &str) -> Result<Vec<String>, String> {
    let root = hi::host_root(host, &env)?;
    Ok(hi::check(host, &root, env!("CARGO_PKG_VERSION")).into_iter().map(|(item, st, d)| format!("{host}: {item}: {}: {}", st.name(), sanitize(&d))).collect())
}

pub fn cmd(host: &str, status: bool, dry: bool, remove: bool, models: &BTreeMap<String, String>) -> i32 {
    let root = match hi::host_root(host, &env) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    };
    let Some(home) = env("HOME").filter(|h| Path::new(h).is_absolute()).map(PathBuf::from) else {
        eprintln!("delphi connect: HOME is unset or not an absolute path");
        return 2;
    };
    let release = env!("CARGO_PKG_VERSION");
    if status {
        for l in check_lines(host).unwrap_or_default() {
            println!("{l}");
        }
        return 0;
    }
    if !dry && crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi connect: connecting is a human act and is refused inside an agent seat");
        return 2;
    }
    if remove {
        return remove_cmd(host, &root, &home, release, dry);
    }
    let launch = std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| delphi_engine::mcp_hosts::absolute_command(&exe, env("HOME").as_deref()));
    let launch = match (launch, dry) {
        (Ok(l), _) => l,
        (Err(e), true) => {
            println!("(no absolute launch path: {e}; the plan shows a bare `delphi`)");
            "delphi".to_string()
        }
        (Err(e), false) => {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    };
    let plan = match hi::plan(host, &root, release, &launch, models) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    };
    print!("{}", sanitize_lines(&hi::render(&plan)));
    if dry {
        println!("Dry run: nothing was run or written.");
        return 0;
    }
    match hi::write_owned(&plan) {
        Ok((written, skipped)) => {
            for p in written {
                println!("wrote {}", sanitize(&p.display().to_string()));
            }
            for p in skipped {
                println!("left alone (Delphi did not write it): {}", sanitize(&p.display().to_string()));
            }
        }
        Err(e) => {
            eprintln!("delphi connect: {e}");
            return 1;
        }
    }
    let terminal = at_terminal();
    let mut code = 0;
    if let Some(argv) = &plan.add_server {
        if terminal && yes(&format!("Run `{}` to add the server to {}?", argv.join(" "), host)) {
            match run_host(&home, argv) {
                Ok(()) => println!("ran {}", argv.join(" ")),
                Err(e) => {
                    eprintln!("delphi connect: {e}");
                    code = 1;
                }
            }
        } else {
            println!("Not run. To add the server yourself: {}", argv.join(" "));
        }
    }
    if let Some((path, state, block)) = &plan.instruction {
        if *state == Status::Current {
            println!("{}: Delphi's block is already current", sanitize(&path.display().to_string()));
        } else if terminal && yes(&format!("Add the block above to {}?", sanitize(&path.display().to_string()))) {
            match hi::apply_block(path, block) {
                Ok(_) => println!("updated {} (only between Delphi's two marker lines)", sanitize(&path.display().to_string())),
                Err(e) => {
                    eprintln!("delphi connect: {e}");
                    code = 1;
                }
            }
        } else {
            println!("Not changed: {}. To add the section yourself, paste the block above into it.", sanitize(&path.display().to_string()));
        }
    }
    code
}

fn sanitize_lines(text: &str) -> String {
    text.lines().map(|l| sanitize(l) + "\n").collect()
}

fn remove_cmd(host: &str, root: &Path, home: &Path, release: &str, dry: bool) -> i32 {
    let plan = match hi::plan(host, root, release, "delphi", &BTreeMap::new()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    };
    println!("delphi connect {host} --remove: Delphi's files recorded in {} (a file changed since it was written is kept), then, at a terminal on a yes each:", sanitize(&hi::owned_dir(host, root).join(hi::MANIFEST).display().to_string()));
    if let Some(argv) = &plan.remove_server {
        println!("  {}", argv.join(" "));
    }
    if let Some((path, _, _)) = &plan.instruction {
        println!("  take Delphi's block out of {}", sanitize(&path.display().to_string()));
    }
    if dry {
        println!("Dry run: nothing was run or removed.");
        return 0;
    }
    let (removed, kept) = hi::remove_owned(host, root);
    for p in removed {
        println!("removed {}", sanitize(&p.display().to_string()));
    }
    for p in kept {
        println!("kept (changed since Delphi wrote it): {}", sanitize(&p.display().to_string()));
    }
    let terminal = at_terminal();
    let mut code = 0;
    if let Some(argv) = &plan.remove_server {
        if terminal && yes(&format!("Run `{}`?", argv.join(" "))) {
            if let Err(e) = run_host(home, argv) {
                eprintln!("delphi connect: {e}");
                code = 1;
            }
        } else {
            println!("Not run. To remove the server yourself: {}", argv.join(" "));
        }
    }
    if let Some((path, _, _)) = &plan.instruction {
        if terminal && yes(&format!("Take Delphi's block out of {}?", sanitize(&path.display().to_string()))) {
            match hi::remove_block(path) {
                Ok(true) => println!("updated {}", sanitize(&path.display().to_string())),
                Ok(false) => println!("{}: no Delphi block", sanitize(&path.display().to_string())),
                Err(e) => {
                    eprintln!("delphi connect: {e}");
                    code = 1;
                }
            }
        } else {
            println!("Not changed: {}. Delete the lines from `{}` to `{}` yourself.", sanitize(&path.display().to_string()), hi::BEGIN, hi::END);
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    // A2: with no terminal (as under `cargo test`), connect writes Delphi's own files, runs no host command and leaves the operator's
    // instruction file alone; `--remove` takes the files out again. All against a fake host folder.
    #[test]
    fn without_a_terminal_only_delphis_own_files_are_written_and_remove_takes_them_out() {
        if std::io::stdin().is_terminal() {
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("codex-home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("AGENTS.md"), "# Mine\n").unwrap();
        let plan = hi::plan("codex", &root, "1.0.0", "/usr/local/bin/delphi", &BTreeMap::new()).unwrap();
        hi::write_owned(&plan).unwrap();
        assert!(root.join("delphi/DELPHI.md").is_file() && root.join("agents/delphi-architect-worker.toml").is_file());
        assert_eq!(std::fs::read_to_string(root.join("AGENTS.md")).unwrap(), "# Mine\n");
        assert!(!root.join("config.toml").exists(), "no host command ran");
        let code = remove_cmd("codex", &root, t.path(), "1.0.0", false);
        assert_eq!(code, 0);
        assert!(!root.join("delphi").exists() && !root.join("agents/delphi-architect-worker.toml").exists());
        assert_eq!(std::fs::read_to_string(root.join("AGENTS.md")).unwrap(), "# Mine\n");
    }

    #[test]
    fn only_codex_and_gemini_are_handled_here() {
        assert!(handles("codex") && handles("gemini") && !handles("copilot") && !handles("claude-code"));
    }
}
