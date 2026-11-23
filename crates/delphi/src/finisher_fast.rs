//! `delphi forge finish --gates`, the fast tier (spec 135 §14.12, H1): which workspace crates a change can reach. The files
//! changed against the merge base with `origin/main` (committed, staged, unstaged and untracked) are mapped to the crate whose
//! directory holds them, and the crates that depend on those (any kind of dependency, transitively) are added. The fast tier
//! runs Clippy and the tests for those crates only, then the lint; the full gates follow. When unsure it selects every
//! crate, and then there is no fast tier: a manifest, the lock file, a build script, the toolchain, the cargo or nextest
//! configuration, `schemas/`, `specs/`, `compiled/`, any path outside a crate that is not in [`LIGHT`], an unreadable
//! `git` or `cargo metadata` answer, or nothing changed. The processes start through the gates' [`Runner`], the one spawn
//! site, so the tests use a fake.

use crate::finisher_gates::{GateCmd, Runner};
use std::collections::BTreeSet;
use std::path::Path;

/// A workspace crate: its package name, its directory relative to the checkout (`/`-separated) and the workspace crates it
/// depends on (normal, build and dev dependencies alike).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub dir: String,
    pub deps: Vec<String>,
}

/// What the fast tier runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Every crate: the fast tier would be the full gates, so it is skipped. `why` says what forced it.
    All { why: String },
    /// These crates, sorted; `why` has one line per changed path and one for the dependents added.
    Crates { crates: Vec<String>, why: Vec<String> },
}

/// Paths outside every crate that some crates' tests read, and those crates (a path ending in `/` is a prefix; a path takes
/// the crates of every entry it matches). Any other path outside a crate selects every crate. The test
/// `light_table_names_every_crate_that_reads_the_path` scans the crates' sources and fails when a crate that names one of
/// these paths is in no entry under it. The narrow entries keep a tracker edit from reaching `delphi-xenia` or
/// `delphi-core`, on which nearly every crate depends.
pub const LIGHT: &[(&str, &[&str])] = &[
    ("missions/", &["delphi", "delphi-engine"]),
    ("missions/2026-10-23-foundational-design/xenia-x10", &["delphi-xenia"]),
    ("docs/", &["delphi", "delphi-engine"]),
    ("docs/guides/xenia-operator.md", &["delphi-core"]),
    ("docs/reference/", &["delphi-release"]),
    ("AGENTS.md", &["delphi", "delphi-engine"]),
    ("README.md", &["delphi", "delphi-bundle"]),
    ("CHANGELOG.md", &["delphi"]),
    ("CONTRIBUTING.md", &["delphi"]),
    ("llms.txt", &["delphi"]),
];

/// A path that changes how the whole workspace builds or what every crate's tests read.
fn whole_build(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    let first = path.split('/').next().unwrap_or(path);
    matches!(file, "Cargo.toml" | "Cargo.lock" | "build.rs" | "rust-toolchain" | "rust-toolchain.toml") || matches!(first, ".cargo" | ".config" | "schemas" | "specs" | "compiled")
}

fn light(path: &str) -> Option<Vec<&'static str>> {
    let mut out: Vec<&'static str> = Vec::new();
    for (p, crates) in LIGHT {
        // A top-level file is matched exactly; anything with a `/` is a prefix.
        let hit = if p.contains('/') { path.starts_with(p) } else { path == *p };
        for c in crates.iter().filter(|_| hit) {
            if !out.contains(c) {
                out.push(c);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The crates a change reaches (see the module comment). Fails safe: anything unrecognised selects every crate.
pub fn select(changed: &[String], members: &[Member]) -> Selection {
    if changed.is_empty() {
        return Selection::All { why: "nothing differs from the merge base".into() };
    }
    if members.is_empty() {
        return Selection::All { why: "no workspace crates were found".into() };
    }
    let mut direct: BTreeSet<String> = BTreeSet::new();
    let mut why = Vec::new();
    for p in changed {
        if whole_build(p) {
            return Selection::All { why: format!("{} changes the whole build", p) };
        }
        let owner = members.iter().filter(|m| m.dir.is_empty() || p.starts_with(&format!("{}/", m.dir))).max_by_key(|m| m.dir.len());
        if let Some(m) = owner {
            why.push(format!("{} -> {}", p, m.name));
            direct.insert(m.name.clone());
        } else if let Some(crates) = light(p) {
            if let Some(missing) = crates.iter().find(|c| !members.iter().any(|m| m.name == **c)) {
                // A table that names a crate this workspace does not have is out of date: trust nothing in it.
                return Selection::All { why: format!("{} is read by `{}`, which is not a workspace crate", p, missing) };
            }
            why.push(format!("{} -> {} (read by their tests)", p, crates.join(", ")));
            direct.extend(crates.iter().map(|c| c.to_string()));
        } else {
            return Selection::All { why: format!("{} is outside every crate", p) };
        }
    }
    let all = with_dependents(&direct, members);
    if all.len() >= members.len() {
        return Selection::All { why: "the change reaches every crate".into() };
    }
    let added: Vec<String> = all.difference(&direct).cloned().collect();
    if !added.is_empty() {
        why.push(format!("and their dependents: {}", added.join(", ")));
    }
    Selection::Crates { crates: all.into_iter().collect(), why }
}

/// `start` and every crate that depends on one of them, transitively.
pub fn with_dependents(start: &BTreeSet<String>, members: &[Member]) -> BTreeSet<String> {
    let mut out = start.clone();
    let mut todo: Vec<String> = start.iter().cloned().collect();
    while let Some(c) = todo.pop() {
        for m in members.iter().filter(|m| m.deps.contains(&c)) {
            if out.insert(m.name.clone()) {
                todo.push(m.name.clone());
            }
        }
    }
    out
}

/// The workspace crates from `cargo metadata --no-deps --format-version 1` (its JSON line; the runner joins standard error,
/// so other lines are skipped). A crate outside the checkout or a name that is not a plain package name is an error.
pub fn parse_metadata(output: &str, repo: &Path) -> Result<Vec<Member>, String> {
    let line = output.lines().find(|l| l.trim_start().starts_with('{')).ok_or("cargo metadata printed no JSON")?;
    let v: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("cargo metadata: {}", e))?;
    let root = std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    let mut out = Vec::new();
    for p in v["packages"].as_array().ok_or("cargo metadata has no packages")? {
        let name = p["name"].as_str().ok_or("a package has no name")?.to_string();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(format!("not a plain package name: {:?}", name));
        }
        let manifest = Path::new(p["manifest_path"].as_str().ok_or("a package has no manifest_path")?);
        let dir = manifest.parent().ok_or("a manifest has no directory")?;
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let rel = dir.strip_prefix(&root).map_err(|_| format!("{} is outside the checkout", dir.display()))?;
        let rel = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        let deps = p["dependencies"].as_array().map(|ds| ds.iter().filter(|d| d["path"].is_string()).filter_map(|d| d["name"].as_str().map(String::from)).collect()).unwrap_or_default();
        out.push(Member { name, dir: rel, deps });
    }
    Ok(out)
}

fn cmd(program: &str, args: &[&str]) -> GateCmd {
    GateCmd { program: program.into(), args: args.iter().map(|s| s.to_string()).collect(), env: Vec::new() }
}

/// Asks git and cargo, through `runner`, what changed and what the crates are, and selects. Any failure selects every crate.
pub fn selection(repo: &Path, runner: &mut dyn Runner) -> Selection {
    let all = |why: String| Selection::All { why };
    let base = runner.run(repo, &cmd("git", &["merge-base", "HEAD", "origin/main"]));
    let sha = base.output.lines().map(str::trim).find(|l| (l.len() == 40 || l.len() == 64) && l.chars().all(|c| c.is_ascii_hexdigit())).map(String::from);
    let Some(sha) = sha.filter(|_| base.started && base.ok) else { return all("no merge base with origin/main (fetch it?)".into()) };
    let diff = runner.run(repo, &cmd("git", &["-c", "core.quotePath=false", "diff", "--name-only", "--no-renames", &sha]));
    let untracked = runner.run(repo, &cmd("git", &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"]));
    if !(diff.started && diff.ok && untracked.started && untracked.ok) {
        return all("git could not list the changed files".into());
    }
    let mut changed: Vec<String> = Vec::new();
    for l in diff.output.lines().chain(untracked.output.lines()).map(str::trim).filter(|l| !l.is_empty()) {
        if !changed.iter().any(|c| c == l) {
            changed.push(l.to_string());
        }
    }
    let meta = runner.run(repo, &cmd("cargo", &["metadata", "--no-deps", "--format-version", "1"]));
    if !(meta.started && meta.ok) {
        return all("cargo metadata failed".into());
    }
    match parse_metadata(&meta.output, repo) {
        Ok(members) => select(&changed, &members),
        Err(e) => all(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, dir: &str, deps: &[&str]) -> Member {
        Member { name: name.into(), dir: dir.into(), deps: deps.iter().map(|s| s.to_string()).collect() }
    }

    /// The fixture: core <- engine <- app, core <- app, and docs on its own; `crates/core-extra` is a sibling, not inside core.
    fn ws() -> Vec<Member> {
        vec![m("core", "crates/core", &[]), m("core-extra", "crates/core-extra", &[]), m("engine", "crates/engine", &["core"]), m("delphi", "crates/delphi", &["engine", "core"]), m("delphi-core", "crates/dc", &[]), m("delphi-engine", "crates/de", &[])]
    }

    fn files(f: &[&str]) -> Vec<String> {
        f.iter().map(|s| s.to_string()).collect()
    }

    fn crates(s: &Selection) -> Vec<String> {
        match s {
            Selection::Crates { crates, .. } => crates.clone(),
            Selection::All { why } => panic!("selected everything: {}", why),
        }
    }

    #[test]
    fn h1_a_file_maps_to_its_crate_and_the_crates_that_depend_on_it() {
        assert_eq!(crates(&select(&files(&["crates/delphi/src/main.rs"]), &ws())), vec!["delphi"]);
        assert_eq!(crates(&select(&files(&["crates/engine/src/a.rs"]), &ws())), vec!["delphi", "engine"]);
        let s = select(&files(&["crates/core/tests/x.rs"]), &ws());
        assert_eq!(crates(&s), vec!["core", "delphi", "engine"]);
        let Selection::Crates { why, .. } = &s else { unreachable!() };
        assert_eq!(why, &files(&["crates/core/tests/x.rs -> core", "and their dependents: delphi, engine"]));
        // A sibling directory with a longer name is not inside `crates/core`.
        assert_eq!(crates(&select(&files(&["crates/core-extra/src/lib.rs"]), &ws())), vec!["core-extra"]);
    }

    #[test]
    fn h1_light_paths_select_the_crates_that_read_them() {
        assert_eq!(crates(&select(&files(&["missions/2026-10-23-foundational-design/spec-tracker.md", "crates/engine/src/a.rs"]), &ws())), vec!["delphi", "delphi-engine", "engine"]);
        assert_eq!(crates(&select(&files(&["AGENTS.md"]), &ws())), vec!["delphi", "delphi-engine"]);
        // A light path whose reader is not a crate of this workspace is not trusted.
        let lone = vec![m("x", "crates/x", &[])];
        assert!(matches!(select(&files(&["missions/a.md"]), &lone), Selection::All { .. }));
    }

    #[test]
    fn h1_build_wide_and_unknown_paths_select_everything() {
        for p in ["Cargo.lock", "Cargo.toml", "crates/engine/Cargo.toml", "crates/delphi/build.rs", ".cargo/config.toml", "rust-toolchain.toml", ".config/nextest.toml", "schemas/x.schema.json", "specs/135-x.md", "compiled/specs/index.md", "agents/builder/AGENT.md", "ui/src/app.tsx", "GOVERNANCE.md"] {
            let s = select(&files(&["crates/delphi/src/main.rs", p]), &ws());
            let Selection::All { why } = s else { panic!("{} did not select everything: {:?}", p, s) };
            assert!(why.starts_with(p), "{}", why);
        }
        assert!(matches!(select(&[], &ws()), Selection::All { .. }), "nothing changed: the full gates");
        assert!(matches!(select(&files(&["crates/x/src/a.rs"]), &[]), Selection::All { .. }));
        // Reaching every crate is the full gates too.
        let two = vec![m("a", "crates/a", &[]), m("b", "crates/b", &["a"])];
        assert_eq!(select(&files(&["crates/a/src/lib.rs"]), &two), Selection::All { why: "the change reaches every crate".into() });
    }

    fn write(p: &Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    /// A real three-crate workspace read by the real `cargo metadata`: a <- b (dev-dependency) <- c, and d alone.
    #[test]
    fn h1_cargo_metadata_of_a_fixture_workspace_gives_the_crates_and_their_dependents() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(&r.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/a\", \"crates/b\", \"crates/c\", \"crates/d\"]\nresolver = \"2\"\n");
        let pkg = |n: &str, deps: &str| format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{}", n, deps);
        write(&r.join("crates/a/Cargo.toml"), &pkg("a", ""));
        write(&r.join("crates/b/Cargo.toml"), &pkg("b", "\n[dev-dependencies]\na = { path = \"../a\" }\n"));
        write(&r.join("crates/c/Cargo.toml"), &pkg("c", "\n[dependencies]\nbee = { package = \"b\", path = \"../b\" }\n"));
        write(&r.join("crates/d/Cargo.toml"), &pkg("d", ""));
        for c in ["a", "b", "c", "d"] {
            write(&r.join(format!("crates/{}/src/lib.rs", c)), "");
        }
        let out = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())).args(["metadata", "--no-deps", "--format-version", "1", "--offline"]).current_dir(r).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let joined = format!("{}\n{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
        let mut members = parse_metadata(&joined, r).unwrap();
        members.sort_by(|x, y| x.name.cmp(&y.name));
        assert_eq!(members, vec![m("a", "crates/a", &[]), m("b", "crates/b", &["a"]), m("c", "crates/c", &["b"]), m("d", "crates/d", &[])]);
        assert_eq!(crates(&select(&files(&["crates/a/src/lib.rs"]), &members)), vec!["a", "b", "c"]);
        assert_eq!(crates(&select(&files(&["crates/c/src/lib.rs"]), &members)), vec!["c"]);
        assert!(!r.join("Cargo.lock").exists(), "metadata --no-deps writes no lock file");
        // Not JSON, or a crate outside the checkout: an error, so everything is selected.
        assert!(parse_metadata("error: could not find `Cargo.toml`", r).is_err());
        let elsewhere = "{\"packages\":[{\"name\":\"x\",\"manifest_path\":\"/elsewhere/x/Cargo.toml\",\"dependencies\":[]}]}";
        assert!(parse_metadata(elsewhere, r).is_err());
    }

    /// The fast tier asks git and cargo through the runner; a missing merge base or a failed metadata selects everything.
    #[test]
    fn h1_selection_asks_git_and_cargo_and_fails_safe() {
        struct Scripted(Vec<(bool, String)>, Vec<GateCmd>);
        impl Runner for Scripted {
            fn run(&mut self, _repo: &Path, cmd: &GateCmd) -> crate::finisher_gates::Ran {
                self.1.push(cmd.clone());
                let (ok, output) = self.0.remove(0);
                crate::finisher_gates::Ran { started: true, ok, output }
            }
        }
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap();
        let meta = format!("{{\"packages\":[{{\"name\":\"a\",\"manifest_path\":\"{0}/crates/a/Cargo.toml\",\"dependencies\":[]}},{{\"name\":\"b\",\"manifest_path\":\"{0}/crates/b/Cargo.toml\",\"dependencies\":[]}}]}}", root.display());
        let sha = "0123456789abcdef0123456789abcdef01234567".to_string();
        let mut s = Scripted(vec![(true, sha.clone()), (true, "crates/a/src/lib.rs\n".into()), (true, "crates/a/src/new.rs\n".into()), (true, format!("warning: x\n{}\n", meta))], Vec::new());
        assert_eq!(crates(&selection(&root, &mut s)), vec!["a"]);
        assert_eq!(s.1[1].args.last(), Some(&sha));
        assert_eq!(s.1[3].args, files(&["metadata", "--no-deps", "--format-version", "1"]));
        let mut s = Scripted(vec![(false, "fatal: Not a valid object name origin/main".into())], Vec::new());
        assert!(matches!(selection(&root, &mut s), Selection::All { why } if why.contains("merge base")));
        let mut s = Scripted(vec![(true, sha), (true, "crates/a/src/lib.rs\n".into()), (true, String::new()), (false, "error".into())], Vec::new());
        assert!(matches!(selection(&root, &mut s), Selection::All { why } if why.contains("metadata")));
    }

    fn rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() && p.file_name().is_some_and(|n| n != "target") {
                rs_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// [`LIGHT`] must name every crate whose code names one of its paths (`"../docs/`, `"docs/`, `"AGENTS.md"`, ...).
    #[test]
    fn light_table_names_every_crate_that_reads_the_path() {
        let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut missing = Vec::new();
        for e in std::fs::read_dir(&crates_dir).unwrap().flatten() {
            let Ok(manifest) = std::fs::read_to_string(e.path().join("Cargo.toml")) else { continue };
            let Some(name) = manifest.lines().find_map(|l| l.strip_prefix("name = ").map(|n| n.trim_matches('"').to_string())) else { continue };
            let mut rs = Vec::new();
            rs_files(&e.path(), &mut rs);
            let texts: Vec<String> = rs.iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
            for (path, _) in LIGHT {
                // The repository's own file: reached up from the crate (`"../../docs/`), or named as an argument
                // (`repo("AGENTS.md")`) in a file that finds the checkout from `CARGO_MANIFEST_DIR`.
                let (up, arg) = if path.ends_with('/') { (format!("../{}", path), format!("(\"{}", path)) } else { (format!("../{}\"", path), format!("(\"{}\")", path)) };
                let named = texts.iter().any(|t| t.contains(&up) || (t.contains("CARGO_MANIFEST_DIR") && t.contains(&arg)));
                let covered = LIGHT.iter().any(|(p, cs)| (p.starts_with(path) || path.starts_with(p)) && cs.contains(&name.as_str()));
                if named && name != "delphi" && !covered {
                    missing.push(format!("{} names {}", name, path));
                }
            }
        }
        assert!(missing.is_empty(), "add these crates to finisher_fast::LIGHT: {:?}", missing);
    }
}
