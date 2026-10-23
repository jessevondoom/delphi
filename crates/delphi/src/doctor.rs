//! `delphi doctor` (`specs/006` §5): eight read-only, offline checks reporting ok / warn / fail / skipped.

use crate::home::{self, hash, is_protected, read_lock, Workspace};
use crate::lint::{self, Mode, Tree};
use crate::report::{Level, Report};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Logical overlay of a workspace (`specs/006` §3.1): base < core for Zone 1; team; local; operator.
pub fn overlay(ws: &Workspace, home: &Path) -> Tree {
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut add = |root: PathBuf, prefix: &str| {
        for e in walkdir::WalkDir::new(&root).into_iter().flatten().filter(|e| e.file_type().is_file()) {
            let rel = e.path().strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if rel == "manifest.lock" {
                continue;
            }
            files.insert(format!("{}{}", prefix, rel), e.path().to_path_buf());
        }
    };
    add(ws.dir.join("base"), "");
    add(ws.dir.join("core"), "");
    add(ws.dir.join("team"), "");
    add(ws.dir.join("local/memory"), "memory/");
    add(ws.dir.join("local/missions"), "missions/");
    let op = home.join("operator/heuristics.md");
    if op.exists() {
        files.insert("operator/heuristics.md".into(), op);
    }
    Tree { mode: Mode::Workspace, files }
}

pub fn run(home: &Path, ws: &Workspace) -> Report {
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

    // 3. Taint audit: protected files match ratified.lock (drift = pending proposal).
    check_ratified(ws, home, &tree, &mut r);

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
            _ => {}
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
    if endpoints == 0 {
        r.warn("7 model-access", "", "no endpoints configured (`delphi endpoints add`, specs/009)");
    } else {
        r.push(Level::Ok, "7 model-access", "", format!("{} endpoint(s) configured; run with --probe to test them", endpoints));
    }

    // 8. Hygiene: clean projects, orphaned worktrees.
    check_hygiene(ws, &mut r);
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
    for (logical, phys) in &tree.files {
        if !is_protected(logical) {
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
    let cfg = fs::read_to_string(ws.dir.join("config.toml")).unwrap_or_default();
    if lock.get("config.toml#access").map(|h| *h != hash(home::access_blocks(&cfg).as_bytes())).unwrap_or(true) {
        drift += 1;
        r.warn("3 taint-audit", "config.toml", "endpoint/access/auth blocks differ from the ratified version: Red-tier proposal (specs/009 §5)");
    }
    let op_lock = read_lock(&home.join("operator/ratified.lock")).unwrap_or_default();
    for (name, h) in &op_lock {
        let live = hash(&fs::read(home.join("operator").join(name)).unwrap_or_default());
        if live != *h {
            drift += 1;
            r.warn("3 taint-audit", format!("operator/{}", name), "differs from the ratified copy: pending proposal");
        }
    }
    if drift == 0 && unratified == 0 {
        r.push(Level::Ok, "3 taint-audit", "", "every protected file matches ratified.lock");
    }
}

fn check_hygiene(ws: &Workspace, r: &mut Report) {
    let mut problems = 0;
    for p in home::projects(&ws.dir) {
        if p.join(".delphi").exists() {
            problems += 1;
            r.fail("8 hygiene", p.display().to_string(), "project contains a `.delphi/` directory; Delphi state belongs in the Delphi home (D-03)");
        }
        if !p.join(".git").exists() {
            r.warn("8 hygiene", p.display().to_string(), "registered project is not a git repository");
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
    if problems == 0 {
        r.push(Level::Ok, "8 hygiene", "", "projects clean; no orphaned worktrees");
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
        let r = run(&home, &ws);
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
        let r = run(&home, &ws);
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
        assert!(run(&home, &ws).findings.iter().any(|f| f.check.starts_with("8") && f.level == Level::Ok));
        fs::create_dir_all(proj.join(".delphi")).unwrap();
        assert_eq!(run(&home, &ws).exit_code(), 1);
    }

    #[test]
    fn modified_release_snapshot_fails() {
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("base/GOVERNANCE.md"), "edited").unwrap();
        assert_eq!(run(&home, &ws).exit_code(), 1);
    }
}
