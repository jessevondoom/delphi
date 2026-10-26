//! Hidden tests for a code case harvested from history (`specs/037` §3, §5.2).
//!
//! The tests of a real change live in the files it changed (a Rust `#[cfg(test)] mod tests`). To score an agent on them
//! without letting it read them, the case keeps the reference version of each such module in `hidden/`, and after the run
//! the runner **replaces the module in the agent's file** with the reference one and runs the case's command. A case is
//! valid only if the reference passes (three runs, so a flaky test is caught) and the base commit fails; `validate`
//! proves that against real trees and records it in `hidden/validated.toml`, which any edit to the case invalidates.
//!
//! The case's spec must therefore name the files and interfaces the tests use; a correct solution in another shape
//! fails (a known limit, `specs/037` §11).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::train_run::run_process;

pub type Problems = Vec<String>;

#[derive(Debug, Clone, PartialEq)]
pub struct HiddenTest {
    /// The file in the repository whose test module is replaced.
    pub file: String,
    /// The reference module, relative to the case directory.
    pub module: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hidden {
    pub base_sha: String,
    pub reference_sha: String,
    pub tests: Vec<HiddenTest>,
    pub cmd: Vec<String>,
    pub timeout_secs: u64,
    /// The command must report at least this many passing tests (a filter that matches nothing passes vacuously).
    pub min_passed: u32,
    /// Hash of everything the validation depends on; the validation record is stale when it differs.
    pub fingerprint: String,
}

fn sha_ok(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn rel_ok(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.contains('\\') && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

/// Read `hidden/hidden.toml` of a case, if it has one: `[source]` (`base_sha`, `reference_sha`), `[[hidden_test]]`
/// (`file`, `module`) and `[hidden_run]` (`cmd`, `timeout_secs`, `min_passed`).
pub fn load(case_dir: &Path, at: &str) -> Result<Option<Hidden>, Problems> {
    let path = case_dir.join("hidden/hidden.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let mut p: Problems = Vec::new();
    let t: toml::Table = match fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| s.parse::<toml::Table>().map_err(|e| e.message().to_string())) {
        Ok(t) => t,
        Err(e) => return Err(vec![format!("{}: hidden/hidden.toml: {}", at, e)]),
    };
    let src = t.get("source").and_then(|v| v.as_table());
    let (base, reference) = (src.and_then(|s| s.get("base_sha")).and_then(|v| v.as_str()).unwrap_or(""), src.and_then(|s| s.get("reference_sha")).and_then(|v| v.as_str()).unwrap_or(""));
    if !sha_ok(base) || !sha_ok(reference) || base == reference {
        p.push(format!("{}: hidden.toml [source] needs distinct `base_sha` and `reference_sha` (git object ids)", at));
    }
    let mut tests = Vec::new();
    for (i, h) in t.get("hidden_test").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
        let (file, module) = (h.get("file").and_then(|v| v.as_str()).unwrap_or(""), h.get("module").and_then(|v| v.as_str()).unwrap_or(""));
        if !rel_ok(file) || !rel_ok(module) {
            p.push(format!("{}: hidden_test #{} needs relative `file` and `module` paths", at, i + 1));
        } else if !case_dir.join("hidden").join(module).is_file() {
            p.push(format!("{}: hidden_test #{} module hidden/{} does not exist", at, i + 1, module));
        }
        tests.push(HiddenTest { file: file.into(), module: module.into() });
    }
    if tests.is_empty() {
        p.push(format!("{}: hidden.toml needs at least one [[hidden_test]]", at));
    }
    let run = t.get("hidden_run").and_then(|v| v.as_table());
    let cmd: Vec<String> = run.and_then(|r| r.get("cmd")).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    if cmd.is_empty() {
        p.push(format!("{}: hidden.toml [hidden_run] needs `cmd`, a list of program and arguments (no shell)", at));
    }
    let timeout_secs = run.and_then(|r| r.get("timeout_secs")).and_then(|v| v.as_integer()).filter(|n| (1..=7200).contains(n)).unwrap_or(1800) as u64;
    let min_passed = run.and_then(|r| r.get("min_passed")).and_then(|v| v.as_integer()).filter(|n| *n >= 1).unwrap_or(1) as u32;
    if !p.is_empty() {
        return Err(p);
    }
    let mut h = blake3::Hasher::new();
    for part in [base, reference, &cmd.join("\u{1}"), &min_passed.to_string()] {
        h.update(part.as_bytes());
        h.update(b"\0");
    }
    for ht in &tests {
        h.update(ht.file.as_bytes());
        h.update(&fs::read(case_dir.join("hidden").join(&ht.module)).unwrap_or_default());
        h.update(b"\0");
    }
    Ok(Some(Hidden { base_sha: base.into(), reference_sha: reference.into(), tests, cmd, timeout_secs, min_passed, fingerprint: h.finalize().to_hex().to_string() }))
}

/// Has validation been recorded for exactly this case content?
pub fn validated(case_dir: &Path, h: &Hidden) -> bool {
    fs::read_to_string(case_dir.join("hidden/validated.toml")).ok().and_then(|t| t.parse::<toml::Table>().ok()).is_some_and(|t| t.get("fingerprint").and_then(|v| v.as_str()) == Some(h.fingerprint.as_str()) && t.get("base_fails").and_then(|v| v.as_bool()) == Some(true))
}

// ── finding and replacing a test module ─────────────────────────────────────────────────────────────────────────

/// The byte range of the `#[cfg(test)] mod tests { ... }` block: from its attribute to its closing brace.
/// Strings, raw strings, characters and comments are skipped when matching braces.
pub fn module_range(src: &str) -> Option<(usize, usize)> {
    let b = src.as_bytes();
    let mut from = 0;
    while let Some(i) = src[from..].find("#[cfg(test)]") {
        let start = from + i;
        let rest = &src[start + "#[cfg(test)]".len()..];
        let trimmed = rest.trim_start();
        // The module may be `pub(crate) mod tests` (some files share helpers with other test modules).
        let vis = ["pub(crate) ", "pub(super) ", "pub "].iter().find(|v| trimmed.starts_with(**v)).map_or(0, |v| v.len());
        if let Some(after) = trimmed[vis..].strip_prefix("mod tests") {
            let name_end = start + "#[cfg(test)]".len() + (rest.len() - trimmed.len()) + vis + "mod tests".len();
            if after.trim_start().starts_with('{') {
                let open = name_end + (after.len() - after.trim_start().len());
                return matching_brace(b, open).map(|close| (start, close + 1));
            }
        }
        from = start + 1;
    }
    None
}

/// Index of the `}` that closes the `{` at `open`.
fn matching_brace(b: &[u8], open: usize) -> Option<usize> {
    let (mut depth, mut i) = (0usize, open);
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut d = 1;
                i += 2;
                while i < b.len() && d > 0 {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        d += 1;
                        i += 1;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        d -= 1;
                        i += 1;
                    }
                    i += 1;
                }
                continue;
            }
            b'r' if matches!(b.get(i + 1), Some(b'"') | Some(b'#')) && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) => {
                // A raw string r#"..."#.
                let mut j = i + 1;
                let mut hashes = 0;
                while b.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if b.get(j) == Some(&b'"') {
                    j += 1;
                    loop {
                        match b.get(j) {
                            None => return None,
                            Some(b'"') if b[j + 1..].iter().take(hashes).filter(|c| **c == b'#').count() == hashes && b.len() >= j + 1 + hashes => {
                                i = j + 1 + hashes;
                                break;
                            }
                            _ => j += 1,
                        }
                    }
                    continue;
                }
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'\'' => {
                // A character literal ('x', '\n', '{'), not a lifetime ('a): a closing quote within a few bytes.
                let close = if b.get(i + 1) == Some(&b'\\') { b[i + 2..].iter().take(8).position(|c| *c == b'\'').map(|p| i + 2 + p) } else { (b.get(i + 2) == Some(&b'\'')).then_some(i + 2) };
                if let Some(c) = close {
                    i = c;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The test module of `src`, attribute included.
pub fn extract_module(src: &str) -> Option<String> {
    module_range(src).map(|(a, z)| src[a..z].to_string())
}

/// `src` with its test module replaced by `module`, or `module` appended when `src` has none.
pub fn inject(src: &str, module: &str) -> String {
    match module_range(src) {
        Some((a, z)) => format!("{}{}{}", &src[..a], module, &src[z..]),
        None => format!("{}\n\n{}\n", src.trim_end(), module),
    }
}

/// Replace the test module in each file of `repo` named by the case's hidden tests.
pub fn apply(repo: &Path, case_dir: &Path, h: &Hidden) -> Result<(), String> {
    for t in &h.tests {
        let file = repo.join(&t.file);
        let src = fs::read_to_string(&file).map_err(|e| format!("{} is missing after the run ({}); the hidden tests need it", t.file, e))?;
        let module = fs::read_to_string(case_dir.join("hidden").join(&t.module)).map_err(|e| e.to_string())?;
        fs::write(&file, inject(&src, &module)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── running them ────────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct TestRun {
    pub passed: bool,
    pub tests_passed: u32,
    /// The last lines of output, for a failure message.
    pub tail: String,
}

/// `test result: ok. 12 passed; 0 failed; ...` lines summed; any line with failures makes the run fail.
fn tally(out: &str) -> (u32, bool) {
    let (mut passed, mut failed) = (0u32, false);
    for l in out.lines().filter(|l| l.trim_start().starts_with("test result:")) {
        let num = |word: &str| l.split(';').chain(l.split('.')).find_map(|part| part.trim().strip_suffix(word).and_then(|n| n.trim().rsplit(' ').next()).and_then(|n| n.parse::<u32>().ok()));
        passed += num("passed").unwrap_or(0);
        failed |= num("failed").unwrap_or(0) > 0;
    }
    (passed, failed)
}

/// Run the case's command in `dir`. Builds share `target` so runs do not each rebuild the dependencies, and are lean
/// (no incremental state, no debug info) so they do not fill the disk.
pub fn run_in(dir: &Path, h: &Hidden, target: &Path, scratch: &Path) -> Result<TestRun, String> {
    fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let (out, err) = (scratch.join("hidden.out"), scratch.join("hidden.err"));
    let mut cmd = Command::new(&h.cmd[0]);
    cmd.args(&h.cmd[1..]).current_dir(dir).env("CARGO_TARGET_DIR", target).env("CARGO_INCREMENTAL", "0").env("CARGO_PROFILE_DEV_DEBUG", "0").env_remove("DELPHI_HOME");
    let (code, timed_out) = run_process(&mut cmd, &out, &err, Duration::from_secs(h.timeout_secs))?;
    let stdout = fs::read_to_string(&out).unwrap_or_default();
    let stderr = fs::read_to_string(&err).unwrap_or_default();
    let (n, failed) = tally(&format!("{}\n{}", stdout, stderr));
    let both = format!("{}\n{}", stderr.trim_end(), stdout.trim_end());
    let tail: String = both.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    Ok(TestRun { passed: !timed_out && code == Some(0) && !failed && n >= h.min_passed, tests_passed: n, tail: if timed_out { format!("timed out after {}s\n{}", h.timeout_secs, tail) } else { tail } })
}

// ── trees from git ──────────────────────────────────────────────────────────────────────────────────────────────

/// Export the tree at `sha` into the new directory `dir`: no history, no remote, so an agent cannot read what came later.
pub fn export_tree(source_repo: &Path, sha: &str, dir: &Path) -> Result<(), String> {
    if !sha_ok(sha) {
        return Err(format!("`{}` is not a commit id", sha));
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    let mut archive = Command::new("git").args(["archive", "--format=tar", sha]).current_dir(source_repo).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("git archive: {}", e))?;
    let tar = Command::new("tar").args(["-x", "-C"]).arg(dir).stdin(archive.stdout.take().ok_or("no archive output")?).output().map_err(|e| format!("tar: {}", e))?;
    let git = archive.wait_with_output().map_err(|e| e.to_string())?;
    if !git.status.success() {
        return Err(format!("git archive {}: {}", sha, String::from_utf8_lossy(&git.stderr).trim()));
    }
    if !tar.status.success() {
        return Err(format!("tar: {}", String::from_utf8_lossy(&tar.stderr).trim()));
    }
    Ok(())
}

// ── validation ──────────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    pub base_failed: bool,
    pub reference_runs: Vec<TestRun>,
}

impl Validation {
    pub fn ok(&self) -> bool {
        self.base_failed && !self.reference_runs.is_empty() && self.reference_runs.iter().all(|r| r.passed)
    }
}

/// Prove the case scores something: the hidden tests fail on the base commit and pass, `flake_runs` times in a row, on
/// the reference commit. Writes `hidden/validated.toml` when they do. Costs no model money; it builds and runs the project.
pub fn validate(case_dir: &Path, h: &Hidden, source_repo: &Path, scratch: &Path, flake_runs: u32) -> Result<Validation, String> {
    let target = scratch.join("target");
    let base = scratch.join("base");
    let reference = scratch.join("reference");
    let _ = fs::remove_dir_all(&base);
    let _ = fs::remove_dir_all(&reference);
    export_tree(source_repo, &h.base_sha, &base)?;
    // A file the change creates does not exist at the base, so the tests cannot even be placed there: that is a failure,
    // which is what the base must do.
    let base_run = match apply(&base, case_dir, h) {
        Ok(()) => run_in(&base, h, &target, &scratch.join("base-out"))?,
        Err(e) => TestRun { passed: false, tests_passed: 0, tail: e },
    };
    export_tree(source_repo, &h.reference_sha, &reference)?;
    apply(&reference, case_dir, h)?;
    let mut reference_runs = Vec::new();
    for i in 0..flake_runs {
        let r = run_in(&reference, h, &target, &scratch.join(format!("reference-out-{}", i)))?;
        let stop = !r.passed;
        reference_runs.push(r);
        if stop {
            break;
        }
    }
    let v = Validation { base_failed: !base_run.passed, reference_runs };
    if v.ok() {
        let n = v.reference_runs.first().map_or(0, |r| r.tests_passed);
        let text = format!("# Written by `delphi train check-hidden`; any edit to the case invalidates it.\nfingerprint = \"{}\"\nbase_fails = true\nreference_runs = {}\nreference_tests_passed = {}\n", h.fingerprint, v.reference_runs.len(), n);
        fs::write(case_dir.join("hidden/validated.toml"), text).map_err(|e| e.to_string())?;
    }
    Ok(v)
}

pub fn render(v: &Validation, h: &Hidden) -> String {
    let mut out = String::new();
    out.push_str(&format!("base {}: hidden tests {} (they must fail here)\n", &h.base_sha[..h.base_sha.len().min(10)], if v.base_failed { "fail" } else { "PASS: the case would score nothing" }));
    for (i, r) in v.reference_runs.iter().enumerate() {
        out.push_str(&format!("reference {} run {}: {} ({} test(s) passed)\n", &h.reference_sha[..h.reference_sha.len().min(10)], i + 1, if r.passed { "pass" } else { "FAIL" }, r.tests_passed));
        if !r.passed {
            out.push_str(&format!("{}\n", r.tail));
        }
    }
    out
}

/// What `delphi train harvest` is told about a change.
pub struct Harvest<'a> {
    pub source_repo: &'a Path,
    pub base: &'a str,
    pub reference: &'a str,
    /// Files whose test module is withheld.
    pub files: &'a [String],
    pub cmd: &'a [String],
    pub timeout_secs: u64,
    pub min_passed: u32,
}

/// Write `hidden/hidden.toml` and the extracted modules for a case, from the reference commit.
pub fn write_hidden(case_dir: &Path, h: &Harvest) -> Result<(), String> {
    let Harvest { source_repo, base, reference, files, cmd, timeout_secs, min_passed } = *h;
    if !sha_ok(base) || !sha_ok(reference) {
        return Err("--base and --reference must be git object ids".into());
    }
    if files.is_empty() || cmd.is_empty() {
        return Err("give at least one --test-file and a --cmd".into());
    }
    let dir = case_dir.join("hidden");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut text = format!("# Made by `delphi train harvest`: the reference change's test modules, withheld from the agent (specs/037 section 5.2).\n[source]\nbase_sha = \"{}\"\nreference_sha = \"{}\"\n", base, reference);
    for f in files {
        if !rel_ok(f) {
            return Err(format!("`{}` is not a relative path", f));
        }
        let out = Command::new("git").arg("show").arg(format!("{}:{}", reference, f)).current_dir(source_repo).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("git show {}:{}: {}", reference, f, String::from_utf8_lossy(&out.stderr).trim()));
        }
        let module = extract_module(&String::from_utf8_lossy(&out.stdout)).ok_or(format!("{} has no `#[cfg(test)] mod tests {{ ... }}` at {}", f, reference))?;
        let name = format!("{}.txt", f.replace('/', "__"));
        fs::write(dir.join(&name), module + "\n").map_err(|e| e.to_string())?;
        text.push_str(&format!("\n[[hidden_test]]\nfile = \"{}\"\nmodule = \"{}\"\n", f, name));
    }
    text.push_str(&format!("\n[hidden_run]\ncmd = [{}]\ntimeout_secs = {}\nmin_passed = {}\n", cmd.iter().map(|c| format!("{:?}", c)).collect::<Vec<_>>().join(", "), timeout_secs, min_passed));
    let _ = fs::remove_file(dir.join("validated.toml"));
    fs::write(dir.join("hidden.toml"), text).map_err(|e| e.to_string())
}

pub fn scratch_dir(base: Option<&str>) -> PathBuf {
    match base {
        Some(b) => PathBuf::from(b),
        None => std::env::temp_dir().join(format!("delphi-train-validate-{}", std::process::id())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "pub fn f() -> u32 { 1 }\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn t() {\n        let s = \"}{ not a brace\"; let c = '}'; let r = r#\"}\"#; // }\n        /* } */\n        assert_eq!(f(), 1);\n    }\n}\n\nfn after() {}\n";

    #[test]
    fn the_test_module_is_found_whatever_braces_its_strings_and_comments_hold() {
        let m = extract_module(SRC).unwrap();
        assert!(m.starts_with("#[cfg(test)]\nmod tests {") && m.ends_with("    }\n}"), "{}", m);
        assert!(m.contains("assert_eq!(f(), 1)") && !m.contains("fn after"));
        assert_eq!(extract_module("fn a() {}\n"), None);
        assert_eq!(extract_module("#[cfg(test)]\nmod other { }\n"), None, "only `mod tests`");
        for vis in ["pub(crate) ", "pub "] {
            let src = format!("fn a() {{}}\n#[cfg(test)]\nmod tool_schema_tests {{ }}\n#[cfg(test)]\n{}mod tests {{\n    fn t() {{ let _ = \"}}\"; }}\n}}\nfn b() {{}}\n", vis);
            let m = extract_module(&src).unwrap();
            assert!(m.contains(&format!("{}mod tests", vis)) && m.ends_with("}\n}") && !m.contains("tool_schema_tests") && !m.contains("fn b"), "{}", m);
            assert!(inject(&src, "#[cfg(test)]\nmod tests { fn new() {} }").contains("fn new()"));
        }
        // A lifetime is not a character literal.
        let lt = "fn g<'a>(x: &'a str) -> &'a str { x }\n#[cfg(test)]\nmod tests { fn ok() { let _ = '{'; } }\nfn z() {}\n";
        assert!(extract_module(lt).unwrap().ends_with("'{'; } }"));
    }

    #[test]
    fn injection_replaces_the_module_and_leaves_the_rest_alone() {
        let new = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn hidden() {}\n}";
        let out = inject(SRC, new);
        assert!(out.starts_with("pub fn f() -> u32 { 1 }") && out.ends_with("fn after() {}\n") && out.contains("fn hidden()") && !out.contains("not a brace"), "{}", out);
        let none = inject("fn only() {}\n", new);
        assert!(none.starts_with("fn only() {}") && none.ends_with("}\n") && none.contains("fn hidden()"), "appended when the agent's file has no module");
    }

    #[test]
    fn test_output_is_tallied_and_a_filter_that_matches_nothing_does_not_count() {
        assert_eq!(tally("test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 1s"), (12, false));
        assert_eq!(tally("test result: ok. 2 passed; 0 failed; 0 ignored\ntest result: ok. 3 passed; 0 failed; 0 ignored"), (5, false));
        assert_eq!(tally("test result: FAILED. 4 passed; 1 failed; 0 ignored"), (4, true));
        assert_eq!(tally("running 0 tests\ntest result: ok. 0 passed; 0 failed"), (0, false));
        assert_eq!(tally("no summary"), (0, false));
    }

    /// A git repository with two commits: the base has no `fixed` marker in src/lib.rs, the reference has it and a test module.
    fn repo(root: &Path) -> (PathBuf, String, String) {
        let r = root.join("src-repo");
        fs::create_dir_all(r.join("src")).unwrap();
        let git = |a: &[&str]| {
            let o = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(a).current_dir(&r).output().unwrap();
            assert!(o.status.success(), "{:?} {}", a, String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        fs::write(r.join("src/lib.rs"), "pub fn f() -> u32 { 0 }\n\n#[cfg(test)]\nmod tests {\n    // old tests\n}\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "base"]);
        let base = git(&["rev-parse", "HEAD"]);
        fs::write(r.join("src/lib.rs"), "pub fn f() -> u32 { 1 } // fixed\n\n#[cfg(test)]\nmod tests {\n    // HIDDEN-MARK real tests live here\n}\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "reference"]);
        let reference = git(&["rev-parse", "HEAD"]);
        (r.clone(), base, reference)
    }

    /// The "test command": passes only if the file has both the implementation marker and the injected module.
    const CMD: &str = "grep -q 'fixed' src/lib.rs && grep -q 'HIDDEN-MARK' src/lib.rs && echo 'test result: ok. 3 passed; 0 failed' || { echo 'test result: FAILED. 0 passed; 1 failed'; exit 1; }";

    fn case(root: &Path, r: &Path, base: &str, reference: &str, cmd: &str) -> (PathBuf, Hidden) {
        let dir = root.join("case");
        fs::create_dir_all(&dir).unwrap();
        write_hidden(&dir, &Harvest { source_repo: r, base, reference, files: &["src/lib.rs".into()], cmd: &["sh".into(), "-c".into(), cmd.into()], timeout_secs: 60, min_passed: 1 }).unwrap();
        let h = load(&dir, "s/c").unwrap().unwrap();
        (dir, h)
    }

    #[test]
    fn harvesting_extracts_the_reference_module_and_validation_proves_fail_to_pass() {
        let t = tempfile::tempdir().unwrap();
        let (r, base, reference) = repo(t.path());
        let (dir, h) = case(t.path(), &r, &base, &reference, CMD);
        assert!(fs::read_to_string(dir.join("hidden/src__lib.rs.txt")).unwrap().contains("HIDDEN-MARK"));
        assert!(!validated(&dir, &h), "nothing has been validated yet");
        let v = validate(&dir, &h, &r, &t.path().join("scratch"), 3).unwrap();
        assert!(v.ok() && v.base_failed && v.reference_runs.len() == 3, "{}", render(&v, &h));
        assert!(validated(&dir, &h));
        assert!(render(&v, &h).contains("hidden tests fail"));
        // Any edit to the case or its modules makes the record stale.
        fs::write(dir.join("hidden/src__lib.rs.txt"), "#[cfg(test)]\nmod tests { /* HIDDEN-MARK changed */ }\n").unwrap();
        let edited = load(&dir, "s/c").unwrap().unwrap();
        assert!(!validated(&dir, &edited));
    }

    #[test]
    fn a_case_whose_tests_also_pass_on_the_base_or_fail_on_the_reference_is_not_valid() {
        let t = tempfile::tempdir().unwrap();
        let (r, base, reference) = repo(t.path());
        // The command always passes: the base passes too, so the case scores nothing.
        let (dir, h) = case(t.path(), &r, &base, &reference, "echo 'test result: ok. 1 passed; 0 failed'");
        let v = validate(&dir, &h, &r, &t.path().join("s1"), 3).unwrap();
        assert!(!v.ok() && !v.base_failed && !validated(&dir, &h));
        assert!(render(&v, &h).contains("the case would score nothing"));
        // The command fails everywhere: the reference does not pass.
        let (dir2, h2) = case(t.path(), &r, &base, &reference, "echo 'test result: FAILED. 0 passed; 1 failed'; exit 1");
        let v2 = validate(&dir2, &h2, &r, &t.path().join("s2"), 3).unwrap();
        assert!(!v2.ok() && v2.reference_runs.len() == 1, "stops at the first failing reference run");
        // A flaky reference (passes once, then fails) is caught by the repeat runs.
        let state = t.path().join("state");
        let flaky = format!("grep -q fixed src/lib.rs && grep -q HIDDEN-MARK src/lib.rs || {{ echo 'test result: FAILED. 0 passed; 1 failed'; exit 1; }}; if test -f {s}; then echo 'test result: FAILED. 0 passed; 1 failed'; exit 1; fi; touch {s}; echo 'test result: ok. 3 passed; 0 failed'", s = state.display());
        let (dir3, h3) = case(t.path(), &r, &base, &reference, &flaky);
        let v3 = validate(&dir3, &h3, &r, &t.path().join("s3"), 3).unwrap();
        assert!(v3.base_failed && !v3.ok() && v3.reference_runs.len() == 2 && v3.reference_runs[0].passed, "a test that passes once and then fails is flaky: {}", render(&v3, &h3));
        assert!(!validated(&dir3, &h3));
    }

    #[test]
    fn a_tree_is_exported_without_history_and_without_a_future() {
        let t = tempfile::tempdir().unwrap();
        let (r, base, _reference) = repo(t.path());
        let out = t.path().join("out");
        export_tree(&r, &base, &out).unwrap();
        assert!(fs::read_to_string(out.join("src/lib.rs")).unwrap().contains("{ 0 }") && !out.join(".git").exists());
        assert!(export_tree(&r, "not-a-sha", &t.path().join("o2")).is_err());
        assert!(export_tree(&r, "0123456789abcdef", &t.path().join("o3")).is_err(), "an unknown commit is an error");
    }

    #[test]
    fn hidden_toml_is_checked() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        assert_eq!(load(d, "s/c").unwrap(), None);
        fs::create_dir_all(d.join("hidden")).unwrap();
        fs::write(d.join("hidden/hidden.toml"), "[source]\nbase_sha = \"zz\"\nreference_sha = \"zz\"\n[[hidden_test]]\nfile = \"../x.rs\"\nmodule = \"m.txt\"\n").unwrap();
        let e = load(d, "s/c").unwrap_err().join("\n");
        for want in ["distinct `base_sha` and `reference_sha`", "relative `file` and `module`", "needs `cmd`"] {
            assert!(e.contains(want), "{} in {}", want, e);
        }
        assert!(write_hidden(d, &Harvest { source_repo: d, base: "abc1234", reference: "def5678", files: &[], cmd: &["x".into()], timeout_secs: 10, min_passed: 1 }).is_err());
    }
}
