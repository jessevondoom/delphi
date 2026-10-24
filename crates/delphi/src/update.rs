//! `delphi update` and `delphi rollback`: refresh the built-in agents, skills, themes and constitution in a
//! workspace after the binary changed (`specs/006` §4, the v1 `.upstream` mode, D-06).
//!
//! Only Zone 1 (`base/`) is touched. A file the operator customized is never overwritten and never gets
//! conflict markers: the new version is written beside it as `<file>.upstream-v<release>` for the operator to
//! accept, keep or merge. Nothing here changes what agents run under: the daemon loads only ratified copies,
//! so every changed constitutional file or manifest shows up as a pending proposal (`/ratify`).

use crate::home::{hash, read_lock, write_lock};
use crate::seed;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// A file new in this release.
    Added,
    /// Unchanged locally, so `base/` takes the new version.
    Updated,
    /// Gone from this release and unmodified, so removed.
    Removed,
    /// The operator's customization stays; the new version was written (or would be) at this path.
    Upstream(PathBuf),
    /// Gone from this release but customized, so kept.
    KeptCustom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub logical: String,
    pub action: Action,
}

pub type Seed = Vec<(String, Vec<u8>)>;

/// The Zone 1 files of the running release.
pub fn release_seed() -> Seed {
    seed::files().into_iter().filter(|f| f.zone == seed::Zone::Base).map(|f| (f.logical, f.bytes.to_vec())).collect()
}

fn lock_release(base: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(&fs::read(base.join("manifest.lock")).ok()?).ok()?;
    v.get("release")?.as_str().map(String::from)
}

fn file_hash(p: &Path) -> Option<String> {
    fs::read(p).ok().map(|b| hash(&b))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("delphi-update-tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

/// Merge `seed` (release `release`) into `base/`, with the operator's customizations in `core/`.
/// With `apply == false` nothing is written. A previous `base.prev/` is replaced by a copy of what this
/// update changes, which `rollback` restores.
pub fn merge(base: &Path, core: &Path, release: &str, seed: &Seed, apply: bool, allow_downgrade: bool) -> Result<Vec<Change>> {
    let mut manifest = read_lock(&base.join("manifest.lock")).with_context(|| format!("{} has no manifest.lock; run `delphi init` first", base.display()))?;
    if let (Some(installed), false) = (lock_release(base), allow_downgrade) {
        if crate::selfupdate::newer(&installed, release) {
            bail!("this binary is release {} but the workspace was updated to {}; refusing to downgrade (pass --allow-downgrade to override)", release, installed);
        }
    }
    let prev = base.with_file_name("base.prev");
    let mut changes = Vec::new();
    let mut backup: Vec<(String, Option<Vec<u8>>)> = Vec::new(); // (logical, original bytes or None if added)
    let mut writes: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    let mut new_manifest = manifest.clone();
    let seed_paths: BTreeMap<&str, &Vec<u8>> = seed.iter().map(|(l, b)| (l.as_str(), b)).collect();

    for (logical, bytes) in seed {
        let new = hash(bytes);
        let (base_file, core_file) = (base.join(logical), core.join(logical));
        let base_hash = file_hash(&base_file);
        match manifest.get(logical).cloned() {
            None => match &base_hash {
                None => {
                    backup.push((logical.clone(), None));
                    writes.push((base_file, bytes.clone()));
                    new_manifest.insert(logical.clone(), new);
                    changes.push(Change { logical: logical.clone(), action: Action::Added });
                }
                Some(h) if *h == new => {
                    new_manifest.insert(logical.clone(), new);
                }
                Some(_) => {
                    let up = upstream_path(&base_file, release);
                    writes.push((up.clone(), bytes.clone()));
                    changes.push(Change { logical: logical.clone(), action: Action::Upstream(up) });
                }
            },
            Some(locked) if locked == new => {}
            Some(locked) => {
                let core_custom = core_file.exists();
                let base_edited = base_hash.as_ref().is_some_and(|h| *h != locked);
                if core_custom || base_edited {
                    // Both sides changed: keep the operator's file, put the release's beside it.
                    let beside = if core_custom { &core_file } else { &base_file };
                    let up = upstream_path(beside, release);
                    writes.push((up.clone(), bytes.clone()));
                    if core_custom && !base_edited {
                        // The ancestor advances; the customization lives in core/ and is untouched.
                        backup.push((logical.clone(), fs::read(&base_file).ok()));
                        writes.push((base_file, bytes.clone()));
                        new_manifest.insert(logical.clone(), new);
                    }
                    changes.push(Change { logical: logical.clone(), action: Action::Upstream(up) });
                } else {
                    backup.push((logical.clone(), fs::read(&base_file).ok()));
                    writes.push((base_file, bytes.clone()));
                    new_manifest.insert(logical.clone(), new);
                    changes.push(Change { logical: logical.clone(), action: Action::Updated });
                }
            }
        }
    }
    // Files the release no longer ships.
    let gone: Vec<String> = manifest.keys().filter(|k| !seed_paths.contains_key(k.as_str())).cloned().collect();
    let mut removals: Vec<PathBuf> = Vec::new();
    for logical in gone {
        let base_file = base.join(&logical);
        let untouched = file_hash(&base_file).as_ref() == manifest.get(&logical) && !core.join(&logical).exists();
        if untouched {
            backup.push((logical.clone(), fs::read(&base_file).ok()));
            removals.push(base_file);
            new_manifest.remove(&logical);
            changes.push(Change { logical, action: Action::Removed });
        } else {
            changes.push(Change { logical, action: Action::KeptCustom });
        }
    }
    manifest.clear();
    if !apply || changes.is_empty() && new_manifest == read_lock(&base.join("manifest.lock"))? {
        return Ok(changes);
    }

    // Keep what is about to change so `rollback` can put it back.
    if prev.exists() {
        fs::remove_dir_all(&prev)?;
    }
    fs::create_dir_all(&prev)?;
    fs::copy(base.join("manifest.lock"), prev.join("manifest.lock"))?;
    let mut added = String::new();
    for (logical, original) in &backup {
        match original {
            Some(bytes) => write_atomic(&prev.join("files").join(logical), bytes)?,
            None => added.push_str(&format!("{}\n", logical)),
        }
    }
    fs::write(prev.join("added.txt"), added)?;
    for (path, bytes) in writes {
        write_atomic(&path, bytes.as_slice())?;
    }
    for p in removals {
        let _ = fs::remove_file(p);
    }
    let sig = fs::read_to_string(base.join("manifest.lock")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v.get("signature").and_then(|s| s.as_str().map(String::from))).unwrap_or_else(|| "unsigned-dev-build".into());
    write_lock(&base.join("manifest.lock"), &new_manifest, &[("release", release.to_string()), ("signature", sig)])?;
    Ok(changes)
}

fn upstream_path(beside: &Path, release: &str) -> PathBuf {
    let name = beside.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    beside.with_file_name(format!("{}.upstream-v{}", name, release.trim_start_matches('v')))
}

/// Put `base/` back as it was before the last `merge`. Returns how many files were restored or removed.
pub fn rollback(base: &Path) -> Result<usize> {
    let prev = base.with_file_name("base.prev");
    if !prev.join("manifest.lock").exists() {
        bail!("there is nothing to roll back ({} does not exist)", prev.display());
    }
    let mut n = 0;
    let files = prev.join("files");
    for e in walkdir::WalkDir::new(&files).into_iter().flatten().filter(|e| e.file_type().is_file()) {
        let rel = e.path().strip_prefix(&files)?;
        write_atomic(&base.join(rel), &fs::read(e.path())?)?;
        n += 1;
    }
    for logical in fs::read_to_string(prev.join("added.txt")).unwrap_or_default().lines().filter(|l| !l.is_empty()) {
        if fs::remove_file(base.join(logical)).is_ok() {
            n += 1;
        }
    }
    write_atomic(&base.join("manifest.lock"), &fs::read(prev.join("manifest.lock"))?)?;
    fs::remove_dir_all(&prev)?;
    Ok(n)
}

fn describe(c: &Change) -> String {
    match &c.action {
        Action::Added => format!("  + {}", c.logical),
        Action::Updated => format!("  ~ {}", c.logical),
        Action::Removed => format!("  - {}", c.logical),
        Action::Upstream(p) => format!("  ! {} is customized: the new version is at {}", c.logical, p.display()),
        Action::KeptCustom => format!("  ! {} is no longer in this release but is customized; kept", c.logical),
    }
}

fn workspaces(home: &Path, only: Option<&str>) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = fs::read_dir(home.join("workspaces"))
        .map(|rd| rd.flatten().filter(|e| e.path().join("config.toml").exists()).map(|e| (e.file_name().to_string_lossy().to_string(), e.path())).collect())
        .unwrap_or_default();
    v.sort();
    v.retain(|(n, _)| only.is_none_or(|o| o == n));
    v
}

/// `delphi update [--check] [--allow-downgrade] [--workspace <name>]` and `delphi rollback`.
pub fn run(home: &Path, only: Option<&str>, check: bool, allow_downgrade: bool, rollback_instead: bool) -> i32 {
    let list = workspaces(home, only);
    if list.is_empty() {
        eprintln!("delphi update: no workspace found in {}{}", home.display(), only.map(|o| format!(" named `{}`", o)).unwrap_or_default());
        return 2;
    }
    let release = env!("CARGO_PKG_VERSION");
    let seed = release_seed();
    let (mut code, mut rolled_back) = (0, 0);
    // The operator-wide Zone 1 snapshot is shared by every workspace.
    let mut targets: Vec<(String, PathBuf, PathBuf, PathBuf)> = vec![("operator".into(), home.join("operator/base"), home.join("operator/core"), home.to_path_buf())];
    for (n, dir) in &list {
        targets.push((n.clone(), dir.join("base"), dir.join("core"), dir.clone()));
    }
    for (name, base, core, _dir) in &targets {
        if !base.join("manifest.lock").exists() {
            continue;
        }
        if rollback_instead {
            match rollback(base) {
                Ok(n) => {
                    rolled_back += 1;
                    println!("{}: rolled back ({} file(s))", name, n);
                }
                Err(e) => eprintln!("{}: {}", name, e),
            }
            continue;
        }
        match merge(base, core, release, &seed, !check, allow_downgrade) {
            Ok(changes) if changes.is_empty() => println!("{}: already at release {}", name, release),
            Ok(changes) => {
                println!("{}: {} {} change(s) for release {}", name, if check { "would make" } else { "made" }, changes.len(), release);
                for c in &changes {
                    println!("{}", describe(c));
                }
            }
            Err(e) => {
                eprintln!("delphi update: {}: {:#}", name, e);
                code = 1;
            }
        }
    }
    if rollback_instead && rolled_back == 0 {
        code = 1; // nothing anywhere to roll back
    }
    if !check && !rollback_instead && code == 0 {
        for (name, dir) in &list {
            let pending = delphi_engine::ratify::list(home, dir).len();
            if pending > 0 {
                println!("{}: {} pending proposal(s). Agents keep running the ratified copies until you review them: `delphi chat`, then /ratify.", name, pending);
            }
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let (base, core) = (t.path().join("base"), t.path().join("core"));
        let mut m = BTreeMap::new();
        for (l, c) in files {
            fs::create_dir_all(base.join(l).parent().unwrap()).unwrap();
            fs::write(base.join(l), c).unwrap();
            m.insert(l.to_string(), hash(c.as_bytes()));
        }
        fs::create_dir_all(&core).unwrap();
        write_lock(&base.join("manifest.lock"), &m, &[("release", "0.1.0".into()), ("signature", "unsigned-dev-build".into())]).unwrap();
        (t, base, core)
    }

    fn seed(files: &[(&str, &str)]) -> Seed {
        files.iter().map(|(l, c)| (l.to_string(), c.as_bytes().to_vec())).collect()
    }

    fn snap(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        walkdir::WalkDir::new(dir).into_iter().flatten().filter(|e| e.file_type().is_file()).map(|e| (e.path().strip_prefix(dir).unwrap().to_string_lossy().to_string(), fs::read(e.path()).unwrap())).collect()
    }

    #[test]
    fn untouched_files_fast_forward_new_ones_arrive_and_dropped_ones_leave() {
        let (_t, base, core) = setup(&[("GOVERNANCE.md", "v1"), ("agents/a/AGENT.md", "a1"), ("old.md", "gone")]);
        let new = seed(&[("GOVERNANCE.md", "v2"), ("agents/a/AGENT.md", "a1"), ("agents/b/AGENT.md", "b1")]);
        let c = merge(&base, &core, "0.2.0", &new, true, false).unwrap();
        let actions: BTreeMap<&str, &Action> = c.iter().map(|c| (c.logical.as_str(), &c.action)).collect();
        assert_eq!((actions["GOVERNANCE.md"], actions["agents/b/AGENT.md"], actions["old.md"]), (&Action::Updated, &Action::Added, &Action::Removed));
        assert_eq!(fs::read_to_string(base.join("GOVERNANCE.md")).unwrap(), "v2");
        assert!(base.join("agents/b/AGENT.md").exists() && !base.join("old.md").exists());
        let lock = read_lock(&base.join("manifest.lock")).unwrap();
        assert_eq!(lock["GOVERNANCE.md"], hash(b"v2"));
        assert!(!lock.contains_key("old.md") && lock.contains_key("agents/b/AGENT.md"));
        assert_eq!(lock_release(&base).as_deref(), Some("0.2.0"));
        // Running it again changes nothing.
        assert!(merge(&base, &core, "0.2.0", &new, true, false).unwrap().is_empty());
    }

    #[test]
    fn a_customized_file_is_never_overwritten_and_gets_an_upstream_copy_beside_it() {
        let (_t, base, core) = setup(&[("agents/a/AGENT.md", "a1")]);
        fs::create_dir_all(core.join("agents/a")).unwrap();
        fs::write(core.join("agents/a/AGENT.md"), "my customization").unwrap();
        let new = seed(&[("agents/a/AGENT.md", "a2")]);
        let c = merge(&base, &core, "0.2.0", &new, true, false).unwrap();
        let up = core.join("agents/a/AGENT.md.upstream-v0.2.0");
        assert_eq!(c[0].action, Action::Upstream(up.clone()));
        assert_eq!(fs::read_to_string(core.join("agents/a/AGENT.md")).unwrap(), "my customization");
        assert_eq!(fs::read_to_string(&up).unwrap(), "a2");
        assert_eq!(fs::read_to_string(base.join("agents/a/AGENT.md")).unwrap(), "a2", "the pristine ancestor advances");
        // An edit made directly in base/ is kept too, with the manifest left alone so the next update still sees it.
        let (_t2, base2, core2) = setup(&[("x.md", "x1")]);
        fs::write(base2.join("x.md"), "edited in place").unwrap();
        let c = merge(&base2, &core2, "0.2.0", &seed(&[("x.md", "x2")]), true, false).unwrap();
        assert!(matches!(c[0].action, Action::Upstream(_)));
        assert_eq!(fs::read_to_string(base2.join("x.md")).unwrap(), "edited in place");
        assert_eq!(read_lock(&base2.join("manifest.lock")).unwrap()["x.md"], hash(b"x1"));
    }

    #[test]
    fn check_mode_writes_nothing_and_rollback_restores_the_previous_release_exactly() {
        let (_t, base, core) = setup(&[("GOVERNANCE.md", "v1"), ("old.md", "gone")]);
        let new = seed(&[("GOVERNANCE.md", "v2"), ("fresh.md", "f")]);
        let before = snap(&base);
        let planned = merge(&base, &core, "0.2.0", &new, false, false).unwrap();
        assert_eq!(planned.len(), 3);
        assert_eq!(snap(&base), before, "--check changes nothing");
        assert!(!base.with_file_name("base.prev").exists());
        merge(&base, &core, "0.2.0", &new, true, false).unwrap();
        assert_ne!(snap(&base), before);
        assert_eq!(rollback(&base).unwrap(), 3);
        assert_eq!(snap(&base), before, "byte for byte");
        assert!(rollback(&base).is_err(), "only one step back");
    }

    #[test]
    fn a_workspace_is_not_downgraded_by_an_older_binary() {
        let (_t, base, core) = setup(&[("a.md", "1")]);
        merge(&base, &core, "0.3.0", &seed(&[("a.md", "2")]), true, false).unwrap();
        let err = merge(&base, &core, "0.2.0", &seed(&[("a.md", "1")]), true, false).unwrap_err().to_string();
        assert!(err.contains("downgrade"), "{}", err);
        assert_eq!(fs::read_to_string(base.join("a.md")).unwrap(), "2");
        assert!(merge(&base, &core, "0.2.0", &seed(&[("a.md", "1")]), true, true).is_ok());
    }

    #[test]
    fn zone_three_is_never_touched() {
        let t = tempfile::tempdir().unwrap();
        let (home, proj) = (t.path().join("home"), t.path().join("proj"));
        fs::create_dir_all(&proj).unwrap();
        assert!(std::process::Command::new("git").args(["init", "-q"]).current_dir(&proj).status().unwrap().success());
        let ws = crate::home::init(&crate::home::InitOptions { home: home.clone(), workspace: "w".into(), projects: vec![proj], theme: "greek".into() }).unwrap();
        fs::write(ws.dir.join("local/memory/agents/builder/lessons.md"), "canary lesson").unwrap();
        fs::create_dir_all(ws.dir.join("local/missions/m1")).unwrap();
        fs::write(ws.dir.join("local/missions/m1/notes.md"), "canary mission").unwrap();
        let zone3 = |d: &Path| (snap(&d.join("local/memory")), snap(&d.join("local/missions")), snap(&d.join("team")), fs::read(d.join("config.toml")).unwrap());
        let before = zone3(&ws.dir);
        // Pretend the workspace was installed from an older release: change a lock entry, so the seed differs.
        let lock_path = ws.dir.join("base/manifest.lock");
        let mut lock = read_lock(&lock_path).unwrap();
        lock.insert("GOVERNANCE.md".into(), hash(b"older"));
        fs::write(ws.dir.join("base/GOVERNANCE.md"), "older").unwrap();
        write_lock(&lock_path, &lock, &[("release", "0.0.1".into()), ("signature", "unsigned-dev-build".into())]).unwrap();
        // ...and ratified under that older text, as it would have been.
        fs::write(ws.dir.join("local/ratified/GOVERNANCE.md"), "older").unwrap();
        let rl_path = ws.dir.join("local/ratified.lock");
        let mut rl = read_lock(&rl_path).unwrap();
        rl.insert("GOVERNANCE.md".into(), hash(b"older"));
        write_lock(&rl_path, &rl, &[("release", "0.0.1".into()), ("ratified_by", "release".into())]).unwrap();
        let pending_before: Vec<String> = delphi_engine::ratify::list(&home, &ws.dir).into_iter().map(|p| p.id).collect();
        assert!(!pending_before.iter().any(|p| p.contains("GOVERNANCE")), "{:?}", pending_before);
        assert_eq!(run(&home, Some("w"), false, false, false), 0);
        assert_eq!(zone3(&ws.dir), before, "Zone 2 and 3 are byte-identical after an update");
        assert_ne!(fs::read_to_string(ws.dir.join("base/GOVERNANCE.md")).unwrap(), "older", "Zone 1 was refreshed");
        // The refreshed constitution is a pending proposal; the ratified copy still governs.
        let pending = delphi_engine::ratify::list(&home, &ws.dir);
        assert!(pending.iter().any(|p| p.logical == "GOVERNANCE.md"), "{:?}", pending.iter().map(|p| &p.id).collect::<Vec<_>>());
        assert_eq!(fs::read_to_string(ws.dir.join("local/ratified/GOVERNANCE.md")).unwrap(), "older", "the ratified copy is untouched");
        assert_eq!(run(&home, Some("w"), false, false, true), 0, "rollback");
        assert_eq!(fs::read_to_string(ws.dir.join("base/GOVERNANCE.md")).unwrap(), "older");
    }
}
