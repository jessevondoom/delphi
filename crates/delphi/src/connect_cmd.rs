//! Spec 074 C1 and C2: `delphi connect claude-code [--dry-run | --status | --remove]`. `--dry-run` and `--status` execute nothing and write nothing,
//! and never runs the host CLI (not even `--version`). The execution path (C2) is in `connect_run`.

use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use delphi_engine::mcp_check;

use crate::connect_run;

pub const HOST: &str = "claude-code";
pub const SOURCE: &str = "jessevondoom/delphi";
const MAX_EXE: u64 = 1024 * 1024 * 1024;

/// A host CLI found in a fixed directory, resolved and checked.
#[derive(Debug, PartialEq, Eq)]
pub struct HostExe {
    pub listed: PathBuf,
    pub canonical: PathBuf,
    pub uid: u32,
    pub mode: u32,
    pub sha256: String,
}

/// Replace every control character so a path or value cannot forge a line of the plan or move the cursor.
pub fn sanitize(s: &str) -> String {
    s.chars().flat_map(|c| if c.is_control() { c.escape_default().collect::<Vec<_>>() } else { vec![c] }).collect()
}

/// The fixed directories searched for the host CLI, in order. The operator's `PATH` is never a source.
pub fn search_dirs(home: &Path) -> Vec<PathBuf> {
    vec![home.join(".local/bin"), home.join(".claude/local"), "/opt/homebrew/bin".into(), "/usr/local/bin".into(), "/usr/bin".into()]
}

/// Every component of `canonical` must be owned by the user or root and not writable by group or others, and the file regular and executable.
fn path_is_safe(canonical: &Path, floor: Option<&Path>) -> Result<(), String> {
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    let mut cur = Some(canonical);
    while let Some(p) = cur {
        if Some(p) == floor {
            break;
        }
        let m = std::fs::symlink_metadata(p).map_err(|_| format!("{} cannot be inspected", sanitize(&p.display().to_string())))?;
        if m.file_type().is_symlink() {
            return Err(format!("{} is a symlink after resolution", sanitize(&p.display().to_string())));
        }
        if m.uid() != me && m.uid() != 0 {
            return Err(format!("{} is owned by another user", sanitize(&p.display().to_string())));
        }
        if m.mode() & 0o022 != 0 {
            return Err(format!("{} is writable by group or others", sanitize(&p.display().to_string())));
        }
        cur = p.parent().filter(|q| !q.as_os_str().is_empty());
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path).map_err(|_| "the host CLI cannot be opened".to_string())?;
    let m = f.metadata().map_err(|_| "the host CLI cannot be inspected".to_string())?;
    if !m.is_file() || m.len() > MAX_EXE {
        return Err("the host CLI is not a regular file of a sane size".into());
    }
    let mut h = Sha256::new();
    let mut f = f.take(MAX_EXE);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|_| "the host CLI cannot be read".to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Find and check the host CLI named `name` in `dirs`; the first existing candidate decides (a later one is never a fallback for an unsafe one).
pub fn find_host(dirs: &[PathBuf], name: &str) -> Result<HostExe, String> {
    find_host_above(dirs, name, None)
}

/// As `find_host`, trusting everything at and above `floor` (tests only: a scratch directory sits under a sticky, world-writable `/tmp`).
pub fn find_host_above(dirs: &[PathBuf], name: &str, floor: Option<&Path>) -> Result<HostExe, String> {
    for d in dirs {
        let listed = d.join(name);
        if std::fs::symlink_metadata(&listed).is_err() {
            continue;
        }
        let canonical = std::fs::canonicalize(&listed).map_err(|_| format!("{} does not resolve", sanitize(&listed.display().to_string())))?;
        path_is_safe(&canonical, floor)?;
        let m = std::fs::symlink_metadata(&canonical).map_err(|_| "the host CLI cannot be inspected".to_string())?;
        if !m.is_file() || m.mode() & 0o111 == 0 {
            return Err(format!("{} is not an executable regular file", sanitize(&canonical.display().to_string())));
        }
        let sha256 = sha256_file(&canonical)?;
        return Ok(HostExe { listed, canonical, uid: m.uid(), mode: m.mode() & 0o7777, sha256 });
    }
    Err(format!("`{name}` was not found in {}", dirs.iter().map(|d| sanitize(&d.display().to_string())).collect::<Vec<_>>().join(", ")))
}

/// The fixed argv of the two steps (no shell; every element is a constant).
pub fn argv(exe: &Path) -> Vec<Vec<String>> {
    let e = exe.display().to_string();
    vec![
        vec![e.clone(), "plugin".into(), "marketplace".into(), "add".into(), SOURCE.into()],
        vec![e, "plugin".into(), "install".into(), "delphi@delphi".into()],
    ]
}

/// The plan text: everything the operator would consent to. Dynamic values are sanitised.
pub fn render_plan(exe: &HostExe, pre: &mcp_check::Check, addable: bool) -> String {
    let mut o = String::new();
    o.push_str("delphi connect claude-code: plan (nothing has been run or written)\n");
    o.push_str(&format!("host CLI: {} (resolves to {})\n", sanitize(&exe.listed.display().to_string()), sanitize(&exe.canonical.display().to_string())));
    o.push_str(&format!("  owner uid {}, mode {:o}, sha256 {}\n", exe.uid, exe.mode, exe.sha256));
    o.push_str("  the host version is not probed (that would run a program you have not consented to); the argv below was proven on Claude Code 2.1.314\n");
    for (i, a) in argv(&exe.canonical).iter().enumerate() {
        o.push_str(&format!("step {}: {}\n", i + 1, a.iter().map(|x| sanitize(x)).collect::<Vec<_>>().join(" ")));
    }
    o.push_str("working directory: a private temporary directory made at run time\n");
    o.push_str("environment: only a fixed safe PATH, HOME, LANG and LC_*, TERM and a private TMPDIR; proxy, token, loader, Git, npm, Python, Ruby and XDG variables are dropped\n");
    o.push_str(&format!("source: {SOURCE}, a GitHub repository, fetched by Claude Code at connect time; it then tracks Delphi's releases (no commit or version is pinned: spec 071 D-5(B))\n"));
    o.push_str("expected writes: ~/.claude/plugins/ (marketplace and plugin files), ~/.claude/settings.json (enabledPlugins, extraKnownMarketplaces)\n");
    o.push_str("network: github.com, as Claude Code decides\n");
    o.push_str(&format!("state found now: {}: {}\n", pre.state.name(), sanitize(&pre.detail)));
    if addable {
        o.push_str("this would add: the `delphi` marketplace and the `delphi@delphi` plugin\n");
    } else {
        o.push_str("connect would refuse: it only adds what is absent. Use `delphi connect claude-code --status` for the cure\n");
    }
    o.push_str("removal: `delphi connect claude-code --remove`, only for what connect added and only while the host's state still matches its record\n");
    o.push_str("failure: there is no automatic rollback; a partial result is reported with the manual cure\n");
    o
}

/// The confirmation the execution path will require for this plan, bound to the whole plan text.
pub fn confirmation(plan: &str) -> String {
    let h: String = Sha256::digest(plan.as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("CONNECT {HOST} {h}")
}

fn usage() -> i32 {
    eprintln!("usage: delphi connect claude-code [--dry-run | --status | --remove]");
    2
}

pub fn cmd(args: &[String]) -> i32 {
    let rest: Vec<&str> = args.iter().map(String::as_str).collect();
    let status = rest.contains(&"--status");
    let dry = rest.contains(&"--dry-run");
    let remove = rest.contains(&"--remove");
    let Some(host) = rest.iter().find(|a| !a.starts_with("--")) else { return usage() };
    if *host != HOST {
        eprintln!("delphi connect: only claude-code is supported so far; use the manual commands from `delphi mcp --print-config {}`", sanitize(host));
        return 2;
    }
    if [status, dry, remove].iter().filter(|x| **x).count() > 1 || rest.iter().any(|a| a.starts_with("--") && !matches!(*a, "--status" | "--dry-run" | "--remove")) {
        return usage();
    }
    let Some(home) = std::env::var("HOME").ok().filter(|h| Path::new(h).is_absolute()) else {
        eprintln!("delphi connect: HOME is unset or not an absolute path");
        return 2;
    };
    let home = Path::new(&home);
    let release = env!("CARGO_PKG_VERSION");
    let pre = mcp_check::check_claude_code(home, release);
    if status {
        println!("claude-code: {}: {}", pre.state.name(), sanitize(&pre.detail));
        println!("claude-code: launchable: {}", mcp_check::LAUNCHABLE);
        println!("claude-code: launch path: {}", crate::launch_path_line(home));
        println!("next: {}", pre.fix.as_deref().map(sanitize).unwrap_or_else(|| "nothing to do".into()));
        return 0;
    }
    if !dry {
        if let Err(e) = connect_run::may_run_here() {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    }
    let addable = connect_run::addable(&pre, home);
    let exe = match find_host(&search_dirs(home), "claude") {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("delphi connect: {e}. Manual commands: {}", if remove { connect_run::MANUAL_CURE_REMOVE } else { connect_run::MANUAL_CURE_ADD });
            return 1;
        }
    };
    if dry {
        let plan = render_plan(&exe, &pre, addable);
        print!("{plan}");
        println!("confirmation that `delphi connect claude-code` will require for exactly this plan: {}", confirmation(&plan));
        return 0;
    }
    let delphi_home = crate::home::delphi_home(None);
    let (plan, expect) = if remove {
        if let Err(e) = connect_run::removable(home, &delphi_home) {
            eprintln!("delphi connect: {e}");
            return 2;
        }
        let plan = connect_run::remove_plan(&exe);
        let expect = connect_run::remove_confirmation(&plan);
        (plan, expect)
    } else {
        if !addable {
            eprintln!("delphi connect: something of Delphi's is already present, and connect only adds what is absent: {}: {}", pre.state.name(), sanitize(&pre.detail));
            eprintln!("next: {}", pre.fix.as_deref().map(sanitize).unwrap_or_else(|| "see `delphi connect claude-code --status`".into()));
            return 2;
        }
        let plan = render_plan(&exe, &pre, true);
        let expect = confirmation(&plan);
        (plan, expect)
    };
    let proof = match connect_run::consent::ask(&plan, &expect) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    };
    let result = if remove { connect_run::remove(&proof, &exe, home, &delphi_home) } else { connect_run::connect(&proof, &exe, home, &delphi_home, release) };
    match result {
        Ok(m) => {
            println!("{m}");
            0
        }
        Err(e) => {
            eprintln!("delphi connect: {}", e.message());
            if matches!(e, connect_run::Refusal::NotAllowed(_)) {
                2
            } else {
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// The canonical scratch directory's parent: trusted, because the system temp directory is sticky and world-writable.
    fn root(t: &tempfile::TempDir) -> PathBuf {
        std::fs::canonicalize(t.path()).unwrap().parent().unwrap().to_path_buf()
    }

    fn exe_in(dir: &Path, mode: u32) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = dir.join("claude");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn a_symlinked_install_is_resolved_and_hashed_and_a_missing_cli_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let real = exe_in(&t.path().join("share/versions"), 0o755);
        std::fs::create_dir_all(t.path().join("bin")).unwrap();
        std::os::unix::fs::symlink(&real, t.path().join("bin/claude")).unwrap();
        let h = find_host_above(&[t.path().join("bin")], "claude", Some(&root(&t))).unwrap();
        assert_eq!(h.canonical, std::fs::canonicalize(&real).unwrap());
        assert_eq!(h.sha256.len(), 64);
        assert!(find_host_above(&[t.path().join("none")], "claude", Some(&root(&t))).unwrap_err().contains("was not found"));
    }

    #[test]
    fn a_writable_parent_a_writable_file_and_a_non_executable_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("a");
        exe_in(&d, 0o755);
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(find_host_above(std::slice::from_ref(&d), "claude", Some(&root(&t))).unwrap_err().contains("writable by group or others"));
        let d2 = t.path().join("b");
        exe_in(&d2, 0o757);
        assert!(find_host_above(&[d2], "claude", Some(&root(&t))).unwrap_err().contains("writable"));
        let d3 = t.path().join("c");
        exe_in(&d3, 0o644);
        assert!(find_host_above(&[d3], "claude", Some(&root(&t))).unwrap_err().contains("not an executable"));
    }

    #[test]
    fn the_first_candidate_decides_and_an_unsafe_one_is_not_skipped() {
        let t = tempfile::tempdir().unwrap();
        let bad = t.path().join("bad");
        exe_in(&bad, 0o777);
        let good = t.path().join("good");
        exe_in(&good, 0o755);
        assert!(find_host_above(&[bad, good], "claude", Some(&root(&t))).is_err());
    }

    #[test]
    fn the_plan_is_fixed_argv_sanitised_and_the_confirmation_binds_the_whole_plan() {
        let t = tempfile::tempdir().unwrap();
        exe_in(&t.path().join("x"), 0o755);
        let mut h = find_host_above(&[t.path().join("x")], "claude", Some(&root(&t))).unwrap();
        let pre = mcp_check::Check { state: mcp_check::State::Missing, detail: "not installed\nFORGED: yes".into(), fix: None };
        let plan = render_plan(&h, &pre, true);
        assert!(plan.contains("plugin marketplace add jessevondoom/delphi") && plan.contains("plugin install delphi@delphi"));
        assert!(plan.contains("not probed") && plan.contains("this would add"));
        assert!(!plan.contains("\nFORGED"), "a control character cannot start a new line");
        let c = confirmation(&plan);
        assert!(c.starts_with("CONNECT claude-code ") && c.len() == "CONNECT claude-code ".len() + 12);
        h.sha256 = "0".repeat(64);
        assert_ne!(confirmation(&render_plan(&h, &pre, true)), c, "another plan needs another confirmation");
        let present = mcp_check::Check { state: mcp_check::State::Configured, detail: "x".into(), fix: None };
        assert!(render_plan(&h, &present, false).contains("connect would refuse"));
    }

    #[test]
    fn nothing_executes_without_a_terminal_and_flags_are_exclusive() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cmd(&a(&["claude-code"])), 2);
        assert_eq!(cmd(&a(&["copilot", "--dry-run"])), 2);
        assert_eq!(cmd(&a(&["claude-code", "--dry-run", "--status"])), 2);
        assert_eq!(cmd(&a(&[])), 2);
    }
}
