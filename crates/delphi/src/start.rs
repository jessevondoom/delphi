//! Bare `delphi`: get from nothing to the console in one command, and `delphi doctor --fix`.
//!
//! A preflight runs before the console opens. It creates the workspace for the current project (or, outside any
//! git repository, a `default` workspace that holds only the built-in memories and context), runs the model
//! access setup when no endpoint exists, and runs the doctor when the install is new or changed. A healthy
//! install pays a few file reads. Nothing is written into the project (D-03); everything lives under the home.

use crate::home::{self, InitOptions, Resolve, Workspace};
use crate::report::{Level, Report};
use crate::setup::{self, Ui};
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// The workspace used outside any project.
pub const DEFAULT_WORKSPACE: &str = "default";

/// Find the workspace for `cwd`, creating it if none exists. The returned notes say what was created.
pub fn ensure_workspace(home: &Path, cwd: &Path, explicit: Option<String>) -> Result<(Workspace, Vec<String>)> {
    let mut notes = Vec::new();
    let ws_dir = |n: &str| home.join("workspaces").join(n);
    let name = match home::resolve_workspace(home, cwd, explicit) {
        Resolve::Found(n) => n,
        Resolve::NoneRegistered(root) => {
            let name = free_name(home, &root);
            home::init(&InitOptions { home: home.to_path_buf(), workspace: name.clone(), projects: vec![root.clone()], theme: "greek".into() })?;
            notes.push(format!("First time in {}: created the workspace `{}` under {} (nothing is written into your project).", root.display(), name, home.join("workspaces").display()));
            name
        }
        Resolve::NotInGitRepo => {
            if !ws_dir(DEFAULT_WORKSPACE).join("config.toml").exists() {
                home::init(&InitOptions { home: home.to_path_buf(), workspace: DEFAULT_WORKSPACE.into(), projects: vec![], theme: "greek".into() })?;
                notes.push(format!("Not inside a git repository: created the `{}` workspace, which has only the built-in memories and context.", DEFAULT_WORKSPACE));
            } else {
                notes.push(format!("Not inside a git repository: using the `{}` workspace (built-in memories and context only).", DEFAULT_WORKSPACE));
            }
            DEFAULT_WORKSPACE.into()
        }
        Resolve::Ambiguous(v) => return Err(anyhow!("several workspaces register this project ({}); pass --workspace or set DELPHI_WORKSPACE", v.join(", "))),
        Resolve::NoSuchWorkspace(n) => return Err(anyhow!("no workspace named `{}` in {}", n, home.display())),
    };
    Ok((Workspace { dir: ws_dir(&name), name }, notes))
}

/// A workspace name from the project directory that no existing workspace uses.
fn free_name(home: &Path, root: &Path) -> String {
    let base: String = root
        .file_name()
        .map(|n| n.to_string_lossy().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect())
        .filter(|s: &String| !s.is_empty() && !s.starts_with('.'))
        .unwrap_or_else(|| "project".into());
    let mut name = base.clone();
    let mut n = 2;
    while home.join("workspaces").join(&name).exists() {
        name = format!("{}-{}", base, n);
        n += 1;
    }
    name
}

pub fn endpoint_count(ws_dir: &Path) -> usize {
    std::fs::read_to_string(ws_dir.join("config.toml"))
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .and_then(|v| v.get("endpoints").and_then(|e| e.as_array()).map(|a| a.len()))
        .unwrap_or(0)
}

/// What the doctor can offer to do about a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remedy {
    /// No model access yet: the guided setup.
    Setup,
    /// `claude` is installed but signed out.
    ClaudeLogin,
    /// No (or stale, or failed) probe results: one tiny call per endpoint.
    Probe,
    /// The endpoint/access block differs from its ratified version: show the diff and ask (never defaulted by `--yes`).
    RatifyConfig,
}

impl Remedy {
    fn question(&self) -> &'static str {
        match self {
            Remedy::Setup => "No model access is set up. Run the guided setup now?",
            Remedy::ClaudeLogin => "Claude Code is installed but not signed in. Sign in now (`claude auth login`)?",
            Remedy::Probe => "Test each model connection now (one tiny call per endpoint)?",
            Remedy::RatifyConfig => "The model access settings changed since you last ratified them. Review the change now?",
        }
    }
}

/// The fixes on offer for a report, once each, in the order they should run.
pub fn remedies(r: &Report) -> Vec<Remedy> {
    let mut out: Vec<Remedy> = Vec::new();
    let mut add = |x: Remedy| {
        if !out.contains(&x) {
            out.push(x)
        }
    };
    for f in r.findings.iter().filter(|f| f.level >= Level::Warn) {
        let m = f.message.as_str();
        if f.check == "7 model-access" && m.starts_with("no endpoints configured") {
            add(Remedy::Setup);
        } else if f.check == "10 harness" && m.contains("not signed in") {
            add(Remedy::ClaudeLogin);
        } else if f.check == "7 model-access" && (m.contains("probe") || m.contains("last probe")) {
            add(Remedy::Probe);
        } else if f.check == "3 taint-audit" && f.location == "config.toml" && m.contains("differ from the ratified version") {
            add(Remedy::RatifyConfig);
        }
    }
    // The guided setup ends by putting that same change in front of the operator.
    if out.contains(&Remedy::Setup) {
        out.retain(|r| *r != Remedy::RatifyConfig);
    }
    out
}

/// Where the preflight remembers that this install passed the doctor, and for which release.
fn marker(ws_dir: &Path) -> PathBuf {
    ws_dir.join("local/onboarded")
}

fn healthy_before(ws_dir: &Path) -> bool {
    std::fs::read_to_string(marker(ws_dir)).map(|v| v.trim() == env!("CARGO_PKG_VERSION")).unwrap_or(false)
}

/// Run what `remedies` found, asking before each. Returns true if anything ran.
pub fn apply(ui: &mut dyn Ui, home: &Path, ws: &Workspace, cwd: &Path, found: &[Remedy]) -> bool {
    apply_asking(ui, home, ws, cwd, found).0
}

/// `apply`, also returning the remedies the operator declined, so the caller does not offer them again in the same run.
fn apply_asking(ui: &mut dyn Ui, home: &Path, ws: &Workspace, cwd: &Path, found: &[Remedy]) -> (bool, Vec<Remedy>) {
    let mut ran = false;
    let mut declined = Vec::new();
    for r in found {
        if !ui.yes_no(r.question(), true) {
            declined.push(r.clone());
            continue;
        }
        ran = true;
        match r {
            Remedy::Setup => {
                let o = setup::Opts { home: home.to_path_buf(), cwd: cwd.to_path_buf(), workspace: Some(ws.name.clone()), yes: false, check: false };
                if let Err(e) = setup::run(ui, &setup::RealEnv, &o) {
                    ui.say(&format!("  setup did not finish: {:#}", e));
                }
            }
            Remedy::ClaudeLogin => {
                if !setup::Env::run_tty(&setup::RealEnv, "claude", &["auth", "login"]) {
                    ui.say("  sign-in did not finish; run `claude auth login` when you are ready.");
                }
            }
            Remedy::RatifyConfig => {
                if !setup::ratify_config(ui, home, &ws.dir, false) {
                    ui.say("  Left as a pending proposal; `/ratify` in the console reviews it.");
                }
            }
            Remedy::Probe => {
                let code = delphi_engine::endpoints_cli::run(&ws.dir, &["test".into(), "--probe".into()]);
                if code != 0 {
                    ui.say("  some connections failed; the lines above say which.");
                }
            }
        }
    }
    (ran, declined)
}

/// Drifted files are never ratified in bulk: each is reviewed with its diff in the console.
fn drift_hint(r: &Report) -> Option<&'static str> {
    let drifted = r.findings.iter().any(|f| f.level == Level::Warn && f.check == "3 taint-audit" && f.location != "config.toml" && f.message.contains("pending proposal"));
    drifted.then_some("Changed or new files are pending proposals. Nothing loads them until you review each with its diff: run `delphi`, then `/ratify`.")
}

/// `delphi doctor --fix`: the report, then an offer for each fixable finding, then the report again.
pub fn doctor_fix(ui: &mut dyn Ui, home: &Path, ws: &Workspace, cwd: &Path, opts: &crate::doctor::Options) -> i32 {
    let report = crate::doctor::run(home, ws, opts);
    report.print();
    let found = remedies(&report);
    if found.is_empty() {
        if let Some(h) = drift_hint(&report) {
            println!("{}", h);
        }
        println!("delphi doctor: workspace `{}`: {}", ws.name, report.worst());
        return report.exit_code();
    }
    let report = if apply(ui, home, ws, cwd, &found) { crate::doctor::run(home, ws, opts) } else { report };
    println!();
    report.print();
    if let Some(h) = drift_hint(&report) {
        println!("{}", h);
    }
    println!("delphi doctor: workspace `{}`: {}", ws.name, report.worst());
    report.exit_code()
}

/// Everything before the console opens. `interactive` is false when piped or for `--no-onboard`: then nothing is
/// asked, and only the workspace is created.
pub fn preflight(home: &Path, cwd: &Path, explicit: Option<String>, interactive: bool) -> Result<Workspace> {
    let (ws, notes) = ensure_workspace(home, cwd, explicit)?;
    for n in &notes {
        eprintln!("delphi: {}", n);
    }
    if !interactive {
        return Ok(ws);
    }
    onboard(&mut setup::TerminalUi { auto: false }, home, &ws, cwd);
    Ok(ws)
}

/// The interactive part of the preflight. The guided setup is offered once per run: a decline is remembered, and the
/// doctor's own `Setup` remedy is skipped afterwards (`specs/005e` §6.2).
fn onboard(ui: &mut dyn Ui, home: &Path, ws: &Workspace, cwd: &Path) {
    let mut declined: Vec<Remedy> = Vec::new();
    if endpoint_count(&ws.dir) == 0 {
        eprintln!("delphi: no model access is configured yet, so let's connect one.\n");
        declined.extend(apply_asking(ui, home, ws, cwd, &[Remedy::Setup]).1);
    }
    if !healthy_before(&ws.dir) {
        let opts = crate::doctor::Options { require_providers: false };
        let report = crate::doctor::run(home, ws, &opts);
        let found: Vec<Remedy> = remedies(&report).into_iter().filter(|r| !declined.contains(r)).collect();
        if !found.is_empty() {
            apply(ui, home, ws, cwd, &found);
        }
        // Only a clean bill of health is remembered; a failure is shown again next time.
        let after = if found.is_empty() { report } else { crate::doctor::run(home, ws, &opts) };
        if after.worst() < Level::Fail {
            let _ = std::fs::write(marker(&ws.dir), env!("CARGO_PKG_VERSION"));
        } else {
            eprintln!("delphi: the doctor found problems that need your attention:");
            for f in after.findings.iter().filter(|f| f.level == Level::Fail) {
                eprintln!("  [fail] {}: {} {}", f.check, f.location, f.message);
            }
            eprintln!("  run `delphi doctor` for the full report.\n");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_dir(parent: &Path, name: &str) -> PathBuf {
        let d = parent.join(name);
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::canonicalize(d).unwrap()
    }

    #[test]
    fn creates_a_workspace_for_a_new_project_then_reuses_it() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let proj = git_dir(t.path(), "my app");
        let (ws, notes) = ensure_workspace(&home, &proj, None).unwrap();
        assert_eq!(ws.name, "my-app");
        assert_eq!(notes.len(), 1);
        assert!(ws.dir.join("config.toml").exists());
        let (again, notes) = ensure_workspace(&home, &proj, None).unwrap();
        assert_eq!(again.name, "my-app");
        assert!(notes.is_empty());
    }

    #[test]
    fn a_name_clash_with_another_project_gets_a_suffix() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let a = git_dir(&t.path().join("a"), "app");
        let b = git_dir(&t.path().join("b"), "app");
        assert_eq!(ensure_workspace(&home, &a, None).unwrap().0.name, "app");
        assert_eq!(ensure_workspace(&home, &b, None).unwrap().0.name, "app-2");
    }

    #[test]
    fn outside_a_repository_the_default_workspace_has_no_projects() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let plain = t.path().join("scratch");
        std::fs::create_dir_all(&plain).unwrap();
        let (ws, notes) = ensure_workspace(&home, &plain, None).unwrap();
        assert_eq!(ws.name, DEFAULT_WORKSPACE);
        assert!(notes[0].contains("created"));
        assert!(home::projects(&ws.dir).is_empty());
        assert!(ws.dir.join("local/memory/agents").is_dir());
        let (_, notes) = ensure_workspace(&home, &plain, None).unwrap();
        assert!(notes[0].contains("using"));
    }

    #[test]
    fn an_unknown_explicit_workspace_is_an_error_not_a_new_one() {
        let t = tempfile::tempdir().unwrap();
        assert!(ensure_workspace(&t.path().join("home"), t.path(), Some("nope".into())).is_err());
    }

    #[test]
    fn remedies_are_picked_from_findings_once_each() {
        let mut r = Report::default();
        r.warn("7 model-access", "", "no endpoints configured (`delphi endpoints add`, specs/009)");
        r.warn("10 harness", "claude-sub", "`claude` is installed but not signed in: run `claude auth login`");
        r.warn("7 model-access", "x", "last probe: expired");
        r.warn("7 model-access", "y", "probe results are over 24 hours old");
        r.push(Level::Ok, "9 sandbox", "", "off");
        assert_eq!(remedies(&r), vec![Remedy::Setup, Remedy::ClaudeLogin, Remedy::Probe]);
        assert!(remedies(&Report::default()).is_empty());
    }

    #[test]
    fn access_drift_offers_a_review_unless_setup_will_show_it_anyway() {
        let mut r = Report::default();
        r.warn("3 taint-audit", "config.toml", "endpoint/access/auth blocks differ from the ratified version: Red-tier proposal (specs/009 §5)");
        assert_eq!(remedies(&r), vec![Remedy::RatifyConfig]);
        r.warn("7 model-access", "", "no endpoints configured (`delphi endpoints add`, specs/009)");
        assert_eq!(remedies(&r), vec![Remedy::Setup], "setup ends with the same review");
    }

    /// Answers "no" to everything and remembers what it was asked.
    #[derive(Default)]
    struct Decliner {
        asked: Vec<String>,
    }

    impl Ui for Decliner {
        fn say(&mut self, _text: &str) {}
        fn yes_no(&mut self, question: &str, _default: bool) -> bool {
            self.asked.push(question.to_string());
            false
        }
        fn line(&mut self, _question: &str, default: &str) -> String {
            default.to_string()
        }
        fn secret(&mut self, _question: &str) -> String {
            String::new()
        }
        fn open_url(&mut self, _url: &str) {}
    }

    #[test]
    fn declining_the_guided_setup_is_asked_once_not_twice() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let proj = git_dir(t.path(), "app");
        let (ws, _) = ensure_workspace(&home, &proj, None).unwrap();
        assert_eq!(endpoint_count(&ws.dir), 0, "a new workspace has no model access");
        let mut ui = Decliner::default();
        onboard(&mut ui, &home, &ws, &proj);
        let setup_asks = ui.asked.iter().filter(|q| q.as_str() == Remedy::Setup.question()).count();
        assert_eq!(setup_asks, 1, "asked: {:?}", ui.asked);
    }

    #[test]
    fn drifted_files_get_a_hint_not_a_bulk_ratify() {
        let mut r = Report::default();
        r.warn("3 taint-audit", "REVIEW.md", "differs from the ratified copy: staged as a pending proposal; agents keep loading local/ratified/");
        assert!(remedies(&r).is_empty());
        assert!(drift_hint(&r).unwrap().contains("/ratify"));
        assert!(drift_hint(&Report::default()).is_none());
    }
}
