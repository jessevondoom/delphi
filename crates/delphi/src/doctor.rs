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
        for e in walkdir::WalkDir::new(&root).into_iter().flatten().filter(|e| !e.file_type().is_dir()) {
            let rel = e.path().strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if rel == "manifest.lock" {
                continue;
            }
            files.insert(format!("{}{}", prefix, rel), e.path().to_path_buf());
        }
    };
    add(ws.dir.join("base"), "", &mut files);
    let builtin = files.clone();
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
    Tree { mode: Mode::Workspace, files, seeded, builtin }
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

    // 2. Zone 1 manifest lock integrity (workspace and operator release snapshots).
    check_manifest_lock(&ws.dir.join("base"), "base", &mut r);
    check_manifest_lock(&home.join("operator/base"), "operator/base", &mut r);
    match fs::read_to_string(ws.dir.join("config.toml")).map_err(|e| e.to_string()).and_then(|t| toml::from_str::<toml::Value>(&t).map_err(|e| e.to_string())) {
        Ok(_) => {}
        Err(e) => r.fail("3 taint-audit", "config.toml", format!("unreadable or invalid TOML: {}", e)),
    }

    // 3. Taint audit: protected files match ratified.lock (drift = pending proposal), ratified
    //    copies are intact, overrides are narrow-only, and the active theme is ratified.
    check_ratified(ws, home, &tree, &mut r);
    check_overrides(ws, home, &mut r);
    check_zone_layout(ws, home, &mut r);

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
        r.push(Level::Ok, "7 model-access", "", format!("{} endpoint(s) configured; `delphi endpoints test --probe` tests them", endpoints));
        // Cached probe results (specs/009 §6): read offline, never refreshed here.
        let cache = fs::read_to_string(ws.dir.join("local/endpoint-capabilities.json")).ok().and_then(|t| serde_json::from_str::<Vec<serde_json::Value>>(&t).ok()).unwrap_or_default();
        if cache.is_empty() {
            r.warn("7 model-access", "local/endpoint-capabilities.json", "no probe results yet: run `delphi endpoints test --probe` to learn what each endpoint really supports");
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        for c in &cache {
            let name = format!("{} / {}", c["endpoint_id"].as_str().unwrap_or("?"), c["model_id"].as_str().unwrap_or("?"));
            if c["auth"].as_str() != Some("ok") {
                r.warn("7 model-access", name, format!("last probe: {}", c["auth"].as_str().unwrap_or("unknown")));
            } else if now - c["probed_at"].as_i64().unwrap_or(0) >= 24 * 3600 {
                r.warn("7 model-access", name, "probe results are over 24 hours old: re-run `delphi endpoints test --probe`");
            } else if c["signature_round_trip"] == serde_json::Value::Bool(false) {
                r.warn("7 model-access", name, "reasoning signatures did not survive a round trip; Delphi will drop them for this endpoint");
            }
        }
    }

    // 8. Hygiene: clean projects, orphaned worktrees.
    check_hygiene(ws, home, &mut r);

    // 9. Sandbox (D-04): proven by running a trivial program under it, not by finding the binary.
    match delphi_sandbox::detect() {
        Some(mode) => r.push(Level::Ok, "9 sandbox", "", format!("{} is active: sandboxed execution is Green, harness mode is available", mode.name())),
        None => r.warn("9 sandbox", "", "no OS sandbox is available (sandbox-exec on macOS, bubblewrap on Linux): code execution falls back to Amber and harness mode is unavailable"),
    }
    r
}

fn check_manifest_lock(base: &Path, label: &str, r: &mut Report) {
    let path = base.join("manifest.lock");
    let lock = match read_lock(&path) {
        Ok(l) => l,
        Err(e) => return r.fail("2 manifest-lock", format!("{}/manifest.lock", label), e.to_string()),
    };
    let mut bad = 0;
    for (logical, h) in &lock {
        match fs::read(base.join(logical)) {
            Ok(b) if hash(&b) == *h => {}
            Ok(_) => {
                bad += 1;
                r.fail("2 manifest-lock", format!("{}/{}", label, logical), "release snapshot was modified (it is read-only)");
            }
            Err(_) => {
                bad += 1;
                r.fail("2 manifest-lock", format!("{}/{}", label, logical), "release snapshot file is missing");
            }
        }
    }
    let upstream: Vec<String> = walkdir::WalkDir::new(base.parent().unwrap_or(base))
        .into_iter()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".upstream-"))
        .map(|e| e.path().display().to_string())
        .collect();
    for u in &upstream {
        r.warn("2 manifest-lock", u.clone(), "unresolved upstream merge");
    }
    for e in walkdir::WalkDir::new(base).into_iter().flatten().filter(|e| !e.file_type().is_dir()) {
        let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
        if rel != "manifest.lock" && !lock.contains_key(&rel) {
            bad += 1;
            r.fail("2 manifest-lock", format!("{}/{}", label, rel), "file is not part of the release snapshot");
        }
    }
    let signed = fs::read_to_string(&path).map(|t| !t.contains("unsigned-dev-build")).unwrap_or(false);
    if !signed && label == "base" {
        r.warn("2 manifest-lock", "", "development build: release signature not present (signing ships with Step 5 packaging)");
    }
    if bad == 0 {
        r.push(Level::Ok, "2 manifest-lock", "", format!("{} release files match {}/manifest.lock", lock.len(), label));
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
    // A ratified file whose live copy vanished is drift too (the overlay can't see it).
    for logical in lock.keys().filter(|k| !k.contains('#')) {
        if !tree.files.contains_key(logical) {
            drift += 1;
            r.warn("3 taint-audit", logical.clone(), "live file is missing: pending proposal to delete it; agents keep loading the ratified copy");
        }
    }
    // A lock that lost entries must not pass silently: every file `init` ratified is required.
    for req in required_workspace_entries() {
        if !lock.contains_key(&req) {
            broken += 1;
            r.fail("3 taint-audit", format!("local/ratified.lock: {}", req), "required entry missing from the lock");
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
            for req in home::operator_required_entries() {
                if !op_lock.contains_key(&req) {
                    broken += 1;
                    r.fail("3 taint-audit", format!("operator/ratified.lock: {}", req), "required entry missing from the lock");
                }
            }
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

fn required_workspace_entries() -> Vec<String> {
    let mut v: Vec<String> = crate::seed::files().into_iter().map(|f| f.logical).filter(|l| is_protected(l)).collect();
    v.extend(["context/team-heuristics.md", "heuristics.md", "config.toml#access"].iter().map(|s| s.to_string()));
    v
}

/// Every lock entry (except pinned config blocks) must have an intact copy under `dir`, and `dir`
/// must hold nothing else: the path guard serves protected files from here.
fn verify_copies(lock: &BTreeMap<String, String>, dir: &Path, label: &str, r: &mut Report) -> usize {
    let mut broken = 0;
    for e in walkdir::WalkDir::new(dir).into_iter().flatten().filter(|e| !e.file_type().is_dir()) {
        let rel = e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
        // `config.access.toml` is the ratified copy of the pinned endpoint blocks (`config.toml#access`).
        if rel == "config.access.toml" {
            let ok = lock.get("config.toml#access").map(|h| fs::read(e.path()).map(|b| hash(&b) == *h).unwrap_or(false)).unwrap_or(false);
            if !ok {
                broken += 1;
                r.fail("3 taint-audit", format!("{}/{}", label, rel), "ratified endpoint config does not match its pin (tampered or planted)");
            }
        } else if !lock.contains_key(&rel) {
            broken += 1;
            r.fail("3 taint-audit", format!("{}/{}", label, rel), "file in the ratified directory has no lock entry (planted)");
        }
    }
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

/// Zone discipline (`specs/006` §3.1): `team/` may not shadow Zone 1 release paths (overrides live in
/// `agents/custom/` and `skills/custom/`, which get narrow-only checks), and no zone may contain
/// symlinks, which would let a live file stand in for something outside the Delphi home.
fn check_zone_layout(ws: &Workspace, home: &Path, r: &mut Report) {
    let team = ws.dir.join("team");
    for e in walkdir::WalkDir::new(&team).into_iter().flatten().filter(|e| !e.file_type().is_dir()) {
        let rel = e.path().strip_prefix(&team).unwrap().to_string_lossy().replace('\\', "/");
        let rel = rel.to_lowercase();
        let zone1 = rel == "governance.md"
            || rel == "spec.md"
            || rel == "context/heuristics.md"
            || rel.starts_with("themes/")
            || (rel.starts_with("agents/") && !rel.starts_with("agents/custom/"))
            || (rel.starts_with("skills/") && !rel.starts_with("skills/custom/"));
        if zone1 {
            r.fail("3 zones", format!("team/{}", rel), "team/ may not shadow a release path; overrides go in agents/custom/ or skills/custom/");
        }
    }
    let roots = [ws.dir.join("base"), ws.dir.join("core"), team, ws.dir.join("local"), home.join("operator"), home.join("themes")];
    for root in roots {
        // A dangling symlink at a zone root must not look like an absent zone.
        if fs::symlink_metadata(&root).is_err() {
            continue;
        }
        for e in walkdir::WalkDir::new(&root) {
            match e {
                Ok(e) if e.path_is_symlink() => r.fail("3 zones", e.path().display().to_string(), "symlinks are not allowed in Delphi zones"),
                Ok(_) => {}
                // Unreadable directories would otherwise drop their files from every check.
                Err(err) => r.fail("3 zones", err.path().map(|p| p.display().to_string()).unwrap_or_default(), format!("cannot read: {}", err)),
            }
        }
    }
}

/// Narrow-only (`specs/001` §2.3, 001-A1): `core/` and `operator/core/` overrides get the full manifest
/// checks, compared against the built-in in `base/`.
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
            let text = fs::read_to_string(e.path().join("AGENT.md")).unwrap_or_default();
            let (fails, warns) = lint::manifest_findings(&tree, &text, &id, Some(lint::builtin_fm(&tree, &id)));
            for f in fails {
                r.fail("3 overrides", path.clone(), f);
            }
            for w in warns {
                r.warn("3 overrides", path.clone(), w);
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
                if !gitdir.trim().is_empty() && !e.path().join(gitdir.trim()).exists() {
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
        assert!(fs::read_to_string(ws.dir.join("local/ratified/REVIEW.md")).unwrap().len() > 100);
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
    fn emptied_locks_planted_copies_and_bad_config_fail() {
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("local/ratified.lock"), "{\"files\": {}}").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::write(home.join("operator/ratified.lock"), "{}").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::create_dir_all(ws.dir.join("local/ratified/agents/custom/builder")).unwrap();
        fs::write(ws.dir.join("local/ratified/agents/custom/builder/AGENT.md"), "planted").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::write(ws.dir.join("config.toml"), "not = [valid").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::write(home.join("operator/base/GOVERNANCE.md"), "edited").unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
    }

    #[test]
    fn core_override_cannot_attach_out_of_scope_skill() {
        let (_t, home, ws, _p) = setup();
        let base = fs::read_to_string(ws.dir.join("base/agents/scout/AGENT.md")).unwrap();
        fs::create_dir_all(ws.dir.join("core/agents/scout")).unwrap();
        fs::write(ws.dir.join("core/agents/scout/AGENT.md"), base.replace("authorized_skills:\n", "authorized_skills:\n  - evil\n")).unwrap();
        fs::create_dir_all(ws.dir.join("team/skills/custom/evil")).unwrap();
        fs::write(ws.dir.join("team/skills/custom/evil/SKILL.md"), "---\nname: evil\ndescription: d\nauthorized_agents: [scout]\ninput_schema: {type: object}\noutput_schema: {type: object}\nrequired_permissions: [write:worktree]\n---\nb\n").unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(r.findings.iter().any(|f| f.check == "3 overrides" && f.message.contains("evil")), "{:#?}", r.findings);
    }

    #[test]
    fn team_shadowing_symlinks_and_missing_live_files() {
        let (_t, home, ws, _p) = setup();
        fs::create_dir_all(ws.dir.join("team/agents/scout")).unwrap();
        fs::copy(ws.dir.join("base/agents/scout/AGENT.md"), ws.dir.join("team/agents/scout/AGENT.md")).unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.check == "3 zones" && f.level == Level::Fail));
        let (t, home, ws, _p) = setup();
        fs::write(t.path().join("elsewhere.md"), "x").unwrap();
        std::os::unix::fs::symlink(t.path().join("elsewhere.md"), ws.dir.join("team/context/link.md")).unwrap();
        assert_eq!(run(&home, &ws, &Options::default()).exit_code(), 1);
        let (_t, home, ws, _p) = setup();
        fs::remove_file(ws.dir.join("team/context/team-heuristics.md")).unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.location == "context/team-heuristics.md" && f.message.contains("missing")));
    }

    // An operator override and skill carry manifest/skill schemas, not §5 provenance frontmatter.
    #[test]
    fn case_variant_shadowing_and_unreadable_dirs_fail() {
        use std::os::unix::fs::PermissionsExt;
        let (_t, home, ws, _p) = setup();
        fs::create_dir_all(ws.dir.join("team/Agents/scout")).unwrap();
        fs::write(ws.dir.join("team/Agents/scout/AGENT.md"), "x").unwrap();
        assert!(run(&home, &ws, &Options::default()).findings.iter().any(|f| f.check == "3 zones" && f.level == Level::Fail));
        let (_t, home, ws, _p) = setup();
        let d = ws.dir.join("team/context/locked");
        fs::create_dir_all(&d).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o000)).unwrap();
        let failed = run(&home, &ws, &Options::default()).exit_code();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(failed, 1);
    }

    #[test]
    fn operator_overrides_are_not_provenance_documents() {
        let (_t, home, ws, _p) = setup();
        let base = fs::read_to_string(ws.dir.join("base/agents/scout/AGENT.md")).unwrap();
        fs::create_dir_all(home.join("operator/core/agents/scout")).unwrap();
        fs::write(home.join("operator/core/agents/scout/AGENT.md"), base).unwrap();
        let r = run(&home, &ws, &Options::default());
        assert!(!r.findings.iter().any(|f| f.check == "3 frontmatter"), "{:#?}", r.findings);
        // A faithful copy of the built-in keeps its relative links resolving (counterpart path).
        assert!(!r.findings.iter().any(|f| f.check == "5 links" && f.level == Level::Fail), "{:#?}", r.findings);
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
