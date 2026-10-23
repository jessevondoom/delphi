//! `delphi doctor` (`specs/006` §5): eight read-only, offline checks reporting ok / warn / fail / skipped.

use crate::home::{self, hash, is_protected, read_lock, Workspace};
use crate::lint::{self, Mode, Tree};
use crate::report::{Level, Report};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Logical overlay of a workspace (`specs/006` §3.1): base < core for Zone 1; team; local; operator
/// files under `operator/`; user theme packs under `themes/`.
pub fn overlay(ws: &Workspace, home: &Path) -> Tree {
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let add = |root: PathBuf, prefix: &str, files: &mut BTreeMap<String, PathBuf>| {
        for e in walkdir::WalkDir::new(&root).into_iter().flatten().filter(|e| e.file_type().is_file()) {
            let rel = e.path().strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if rel == "manifest.lock" {
                continue;
            }
            files.insert(format!("{}{}", prefix, rel), e.path().to_path_buf());
        }
    };
    add(ws.dir.join("base"), "", &mut files);
    add(home.join("themes"), "themes/", &mut files);
    add(ws.dir.join("core"), "", &mut files);
    add(ws.dir.join("team"), "", &mut files);
    add(ws.dir.join("local/memory"), "memory/", &mut files);
    add(ws.dir.join("local/missions"), "missions/", &mut files);
    let local_heur = ws.dir.join("local/heuristics.md");
    if local_heur.exists() {
        files.insert("heuristics.md".into(), local_heur);
    }
    let op = home.join("operator");
    for f in ["profile.md", "heuristics.md"] {
        if op.join(f).exists() {
            files.insert(format!("operator/{}", f), op.join(f));
        }
    }
    for d in ["core", "skills", "memory"] {
        add(op.join(d), &format!("operator/{}/", d), &mut files);
    }
    // Files still byte-identical to the release seed (any zone) may link to unseeded design records.
    let seed: BTreeMap<String, &[u8]> = crate::seed::files().into_iter().map(|f| (f.logical, f.bytes)).collect();
    let seeded: BTreeSet<String> = files.iter().filter(|(k, p)| seed.get(*k).map(|b| fs::read(p).map(|x| x == *b).unwrap_or(false)).unwrap_or(false)).map(|(k, _)| k.clone()).collect();
    Tree { mode: Mode::Workspace, files, seeded }
}

/// `doctor` options beyond the workspace.
#[derive(Default)]
pub struct Options {
    /// `--require-providers`: no configured endpoint is a failure, not a warning (`specs/006` §5).
    pub require_providers: bool,
}

pub fn run(home: &Path, ws: &Workspace, opts: &Options) -> Report {
    let mut r = Report::default();
    let tree = overlay(ws, home);

    // 1. Binary ↔ daemon version (no daemon until Step 2).
    let socks = fs::read_dir(home.join("run")).map(|d| d.flatten().filter(|e| e.path().extension().map(|x| x == "sock").unwrap_or(false)).count()).unwrap_or(0);
    if socks == 0 {
        r.push(Level::Skipped, "1 daemon-version", "", "no daemon running");
    } else {
        r.push(Level::Warn, "1 daemon-version", "", "socket present but the version handshake ships with the Step 2 daemon");
    }

    // 2. Zone 1 manifest lock integrity.
    check_manifest_lock(ws, &mut r);

    // 3. Taint audit: protected files match ratified.lock (drift = pending proposal), ratified
    //    copies are intact, overrides are narrow-only, and the active theme is ratified.
    check_ratified(ws, home, &tree, &mut r);
    check_overrides(ws, home, &mut r);

    // 4 + 5. Rule caps and links on the workspace overlay (lint checks reused).
    let lint_report = lint::run(&tree);
    let mut caps = 0;
    let mut links = 0;
    for f in lint_report.findings {
        match f.check.as_str() {
            "rule-caps" => {
                caps += 1;
                r.push(f.level, "4 rule-caps", f.location, f.message);
            }
            "links" | "abs-paths" => {
                if f.level != Level::Skipped {
                    links += 1;
                    r.push(f.level, "5 links", f.location, f.message);
                }
            }
            // Schema, theme and canonical-name findings (specs/001 §1.2, §6.4 rule 3).
            other => r.push(f.level, &format!("3 {}", other), f.location, f.message),
        }
    }
    if caps == 0 {
        r.push(Level::Ok, "4 rule-caps", "", "heuristics and lessons within the Rule of 25, 6 KB and 350-character caps");
    }
    if links == 0 {
        r.push(Level::Ok, "5 links", "", "all links resolve (links to unseeded design records skipped)");
    }

    // 6. Retrieval index (state.db arrives with the Step 2 daemon).
    if ws.dir.join("local/state.db").exists() {
        r.push(Level::Warn, "6 retrieval-index", "", "state.db present; integrity checks ship with Step 2");
    } else {
        r.push(Level::Skipped, "6 retrieval-index", "", "no state.db yet");
    }

    // 7. Model access: offline, from config only (probing is `--probe`, Step 3).
    let cfg = fs::read_to_string(ws.dir.join("config.toml")).unwrap_or_default();
    let endpoints = toml::from_str::<toml::Value>(&cfg).ok().and_then(|v| v.get("endpoints").and_then(|e| e.as_array()).map(|a| a.len())).unwrap_or(0);
    if endpoints == 0 && opts.require_providers {
        r.fail("7 model-access", "", "no endpoints configured and --require-providers was given (specs/009)");
    } else if endpoints == 0 {
        r.warn("7 model-access", "", "no endpoints configured (`delphi endpoints add`, specs/009)");
    } else {
        r.push(Level::Ok, "7 model-access", "", format!("{} endpoint(s) configured; run with --probe to test them", endpoints));
    }

    // 8. Hygiene: clean projects, orphaned worktrees.
    check_hygiene(ws, home, &mut r);
    r
}

fn check_manifest_lock(ws: &Workspace, r: &mut Report) {
    let path = ws.dir.join("base/manifest.lock");
    let lock = match read_lock(&path) {
        Ok(l) => l,
        Err(e) => return r.fail("2 manifest-lock", path.display().to_string(), e.to_string()),
    };
    let mut bad = 0;
    for (logical, h) in &lock {
        match fs::read(ws.dir.join("base").join(logical)) {
            Ok(b) if hash(&b) == *h => {}
            Ok(_) => {
                bad += 1;
                r.fail("2 manifest-lock", format!("base/{}", logical), "release snapshot was modified (base/ is read-only)");
            }
            Err(_) => {
                bad += 1;
                r.fail("2 manifest-lock", format!("base/{}", logical), "release snapshot file is missing");
            }
        }
    }
    let upstream: Vec<String> = walkdir::WalkDir::new(&ws.dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".upstream-"))
        .map(|e| e.path().display().to_string())
        .collect();
    for u in &upstream {
        r.warn("2 manifest-lock", u.clone(), "unresolved upstream merge");
    }
    let base = ws.dir.join("base");
    for e in walkdir::WalkDir::new(&base).into_iter().flatten().filter(|e| !e.file_type().is_dir()) {
        let rel = e.path().strip_prefix(&base).unwrap().to_string_lossy().replace('\\', "/");
        if rel != "manifest.lock" && !lock.contains_key(&rel) {
            bad += 1;
            r.fail("2 manifest-lock", format!("base/{}", rel), "file is not part of the release snapshot");
        }
    }
    let signed = fs::read_to_string(&path).map(|t| !t.contains("unsigned-dev-build")).unwrap_or(false);
    if !signed {
        r.warn("2 manifest-lock", "", "development build: release signature not present (signing ships with Step 5 packaging)");
    }
    if bad == 0 {
        r.push(Level::Ok, "2 manifest-lock", "", format!("{} release files match base/manifest.lock", lock.len()));
    }
}

fn check_ratified(ws: &Workspace, home: &Path, tree: &Tree, r: &mut Report) {
    let lock = match read_lock(&ws.dir.join("local/ratified.lock")) {
        Ok(l) => l,
        Err(e) => return r.fail("3 taint-audit", "local/ratified.lock", e.to_string()),
    };
    let mut drift = 0;
    let mut unratified = 0;
    let mut broken = 0;
    for (logical, phys) in &tree.files {
        if logical.starts_with("operator/") || !is_protected(logical) {
            continue;
        }
        let live = hash(&fs::read(phys).unwrap_or_default());
        match lock.get(logical) {
            Some(h) if *h == live => {}
            Some(_) => {
                drift += 1;
                r.warn("3 taint-audit", logical.clone(), "differs from the ratified copy: staged as a pending proposal; agents keep loading local/ratified/");
            }
            None => {
                unratified += 1;
                r.warn("3 taint-audit", logical.clone(), "not ratified: pending proposal; not loaded until the operator ratifies it");
            }
        }
    }
    // The ratified copies are what agents load: each must exist and match its lock entry.
    broken += verify_copies(&lock, &ws.dir.join("local/ratified"), "local/ratified", r);
    // Approved specs must be ratified by exact hash (specs/003 §3.1.2, 006-A2).
    for (logical, phys) in tree.files.iter().filter(|(k, _)| k.starts_with("specs/") && k.ends_with(".md")) {
        let text = fs::read_to_string(phys).unwrap_or_default();
        let approved = crate::frontmatter::parse(&text).ok().and_then(|d| d.frontmatter).map(|m| crate::frontmatter::get_str(&m, "status") == Some("approved") || m.contains_key(serde_yaml::Value::String("approved_sections".into()))).unwrap_or(false);
        if approved && lock.get(logical) != Some(&hash(text.as_bytes())) {
            broken += 1;
            r.fail("3 taint-audit", logical.clone(), "marked approved but its exact hash is not ratified by the operator");
        }
    }
    let cfg = fs::read_to_string(ws.dir.join("config.toml")).unwrap_or_default();
    if lock.get("config.toml#access").map(|h| *h != hash(home::access_blocks(&cfg).as_bytes())).unwrap_or(true) {
        drift += 1;
        r.warn("3 taint-audit", "config.toml", "endpoint/access/auth blocks differ from the ratified version: Red-tier proposal (specs/009 §5)");
    }
    let theme = toml::from_str::<toml::Value>(&cfg).ok().and_then(|v| v.get("theme").and_then(|t| t.get("name")).and_then(|n| n.as_str()).map(String::from)).unwrap_or_else(|| "greek".into());
    if !lock.contains_key(&format!("themes/{}/theme.md", theme)) {
        broken += 1;
        r.fail("3 taint-audit", format!("themes/{}/theme.md", theme), "configured theme is not ratified (specs/001 §6.4); it will not load");
    }
    // Operator scope (001-A1): fail closed when the lock is unreadable.
    let op = home.join("operator");
    match read_lock(&op.join("ratified.lock")) {
        Err(e) => {
            broken += 1;
            r.fail("3 taint-audit", "operator/ratified.lock", format!("unreadable: {}", e));
        }
        Ok(op_lock) => {
            for (name, h) in &op_lock {
                let live = fs::read(op.join(name)).map(|b| hash(&b)).unwrap_or_default();
                if live != *h {
                    drift += 1;
                    r.warn("3 taint-audit", format!("operator/{}", name), "differs from the ratified copy: pending proposal");
                }
            }
            for (logical, _) in tree.files.iter().filter(|(k, _)| k.starts_with("operator/")) {
                let name = &logical["operator/".len()..];
                if !op_lock.contains_key(name) {
                    unratified += 1;
                    r.warn("3 taint-audit", logical.clone(), "not ratified in operator/ratified.lock: pending proposal; not loaded");
                }
            }
            broken += verify_copies(&op_lock, &op.join("ratified"), "operator/ratified", r);
        }
    }
    if drift == 0 && unratified == 0 && broken == 0 {
        r.push(Level::Ok, "3 taint-audit", "", "every protected file and ratified copy matches its lock");
    }
}

/// Every lock entry (except pinned config blocks) must have an intact copy under `dir`.
fn verify_copies(lock: &BTreeMap<String, String>, dir: &Path, label: &str, r: &mut Report) -> usize {
    let mut broken = 0;
    for (logical, h) in lock.iter().filter(|(k, _)| !k.contains('#')) {
        match fs::read(dir.join(logical)) {
            Ok(b) if hash(&b) == *h => {}
            Ok(_) => {
                broken += 1;
                r.fail("3 taint-audit", format!("{}/{}", label, logical), "ratified copy does not match the lock (tampered)");
            }
            Err(_) => {
                broken += 1;
                r.fail("3 taint-audit", format!("{}/{}", label, logical), "ratified copy is missing");
            }
        }
    }
    broken
}

/// Narrow-only (`specs/001` §2.3, 001-A1): `core/` and `operator/core/` overrides vs the built-in.
fn check_overrides(ws: &Workspace, home: &Path, r: &mut Report) {
    let tree = overlay(ws, home);
    for (root, label) in [(ws.dir.join("core/agents"), "core/agents"), (home.join("operator/core/agents"), "operator/core/agents")] {
        let entries = match fs::read_dir(&root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            let path = format!("{}/{}/AGENT.md", label, id);
            let ovr = fs::read_to_string(e.path().join("AGENT.md")).ok().and_then(|t| crate::frontmatter::parse(&t).ok()).and_then(|d| d.frontmatter);
            let builtin = fs::read_to_string(ws.dir.join("base/agents").join(&id).join("AGENT.md")).ok().and_then(|t| crate::frontmatter::parse(&t).ok()).and_then(|d| d.frontmatter);
            match (ovr, builtin) {
                (Some(o), Some(b)) => {
                    for err in lint::manifest_errors(&o, &id).into_iter().chain(lint::override_widening(&b, &o, &|s| tree.files.contains_key(&format!("skills/custom/{}/SKILL.md", s)) || tree.files.contains_key(&format!("operator/skills/{}/SKILL.md", s)))) {
                        r.fail("3 overrides", path.clone(), err);
                    }
                }
                (None, _) => r.fail("3 overrides", path, "override has no parseable frontmatter"),
                (_, None) => r.fail("3 overrides", path, format!("no built-in role `{}` to override", id)),
            }
        }
    }
}

fn check_hygiene(ws: &Workspace, home: &Path, r: &mut Report) {
    use std::os::unix::fs::PermissionsExt;
    let mut problems = 0;
    for p in home::projects(&ws.dir) {
        let loc = p.display().to_string();
        if !p.exists() {
            problems += 1;
            r.warn("8 hygiene", loc, "registered project path does not exist");
            continue;
        }
        if fs::symlink_metadata(p.join(".delphi")).is_ok() {
            problems += 1;
            r.fail("8 hygiene", loc.clone(), "project contains a `.delphi` entry; Delphi state belongs in the Delphi home (D-03)");
        }
        if !p.join(".git").exists() {
            problems += 1;
            r.warn("8 hygiene", loc.clone(), "registered project is not a git repository");
        }
        // Git worktree registrations whose directory is gone (`git worktree prune` candidates).
        if let Ok(entries) = fs::read_dir(p.join(".git/worktrees")) {
            for e in entries.flatten() {
                let gitdir = fs::read_to_string(e.path().join("gitdir")).unwrap_or_default();
                if !gitdir.trim().is_empty() && !Path::new(gitdir.trim()).exists() {
                    problems += 1;
                    r.warn("8 hygiene", e.path().display().to_string(), "stale git worktree registration");
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(ws.dir.join("worktrees")) {
        for e in entries.flatten() {
            if !e.path().join(".git").exists() {
                problems += 1;
                r.warn("8 hygiene", e.path().display().to_string(), "orphaned worktree directory");
            }
        }
    }
    let run = home.join("run");
    if let Ok(m) = fs::metadata(&run) {
        if m.permissions().mode() & 0o077 != 0 {
            problems += 1;
            r.fail("8 hygiene", run.display().to_string(), "run/ must be 0700");
        }
    }
    if let Ok(entries) = fs::read_dir(&run) {
        for e in entries.flatten() {
            if e.path().extension().map(|x| x == "sock").unwrap_or(false) {
                let mode = fs::symlink_metadata(e.path()).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
                if mode & 0o077 != 0 {
                    problems += 1;
                    r.fail("8 hygiene", e.path().display().to_string(), format!("socket mode {:o}; must be 0600", mode));
                }
            }
        }
    }
    if problems == 0 {
        r.push(Level::Ok, "8 hygiene", "", "projects clean; no orphaned worktrees; run/ private");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::{init, InitOptions};
    use std::process::Command;

    fn setup() -> (tempfile::TempDir, PathBuf, Workspace, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let proj = t.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        Command::new("git").args(["init", "-q"]).current_dir(&proj).status().unwrap();
        let ws = init(&InitOptions { home: home.clone(), workspace: "w".into(), projects: vec![proj.clone()], theme: "greek".into() }).unwrap();
        (t, home, ws, proj)
    }

    // specs/006 §6 test 6 (doctor part): a fresh offline init passes doctor with exit 0.
    #[test]
    fn fresh_init_passes() {
        let (_t, home, ws, _p) = setup();
        let r = run(&home, &ws, &Options::default());
        r.print();
        assert_eq!(r.exit_code(), 0);
        assert!(r.findings.iter().any(|f| f.check.starts_with("3") && f.level == Level::Ok));
        assert!(r.findings.iter().any(|f| f.check.starts_with("5") && f.level == Level::Ok));
    }

    // specs/006 §6 test 7
    #[test]
    fn drift_is_reported_as_pending_proposal() {
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("team/REVIEW.md"), "tampered").unwrap();
        fs::create_dir_all(ws.dir.join("core/agents/builder")).unwrap();
        fs::write(ws.dir.join("core/agents/builder/AGENT.md"), "override").unwrap();
        let r = run(&home, &ws, &Options::default());
        let drift: Vec<_> = r.findings.iter().filter(|f| f.check.starts_with("3") && f.level == Level::Warn).map(|f| f.location.clone()).collect();
        assert!(drift.contains(&"REVIEW.md".to_string()) && drift.contains(&"agents/builder/AGENT.md".to_string()), "{:?}", drift);
        assert_eq!(fs::read_to_string(ws.dir.join("local/ratified/REVIEW.md")).unwrap().len() > 100, true);
    }

    // specs/006 §6 test 10
    #[test]
    fn clean_repo_rule_is_provenance_not_content() {
        let (_t, home, ws, proj) = setup();
        fs::write(proj.join("GOVERNANCE.md"), "a project may contain anything").unwrap();
        fs::create_dir_all(proj.join("agents/builder")).unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.check.starts_with("8") && f.level == Level::Ok));
        fs::create_dir_all(proj.join(".delphi")).unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }

    #[test]
    fn tampered_or_missing_ratified_copies_fail() {
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("local/ratified/GOVERNANCE.md"), "tampered").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::remove_file(home.join("operator/ratified.lock")).unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }

    #[test]
    fn lint_findings_and_widening_overrides_fail_doctor() {
        let (_t, home, ws, _p) = setup();
        let base = fs::read_to_string(ws.dir.join("base/agents/scout/AGENT.md")).unwrap();
        fs::create_dir_all(ws.dir.join("core/agents/scout")).unwrap();
        fs::write(ws.dir.join("core/agents/scout/AGENT.md"), base.replace("write: [", "write: [\"specs/**\", ")).unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().any(|f| f.check.starts_with("3") && f.level == Level::Fail && f.message.contains("widens")), "{:#?}", r.findings);
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("team/context/bad.md"), "no frontmatter\n").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }

    #[test]
    fn unratified_theme_and_require_providers() {
        let (_t, home, ws, _p) = setup();
        let cfg = fs::read_to_string(ws.dir.join("config.toml")).unwrap().replace("name = \"greek\"", "name = \"norse\"");
        fs::write(ws.dir.join("config.toml"), cfg).unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.message.contains("theme is not ratified")));
        let (_t, home, ws, _p) = setup();
        assert_eq!(run(&home, &ws, &Options { require_providers: true }).exit_code(), 1);
    }

    #[test]
    fn modified_release_snapshot_fails() {
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("base/GOVERNANCE.md"), "edited").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("base/extra.md"), "planted").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }
}
