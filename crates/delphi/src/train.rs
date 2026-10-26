//! `delphi train`: training mode's cases and its safety rules (`specs/031` §4, slice L6a).
//!
//! A suite is a directory of cases under `dev/train/suites/<suite>/cases/<id>/`. This module reads and checks them, and
//! prices a run before any is made. It spends nothing and starts no daemon; running the cases comes in the next slice.
//! The rules a run must follow are here too, so they are tested before anything depends on them:
//! the working home must be fresh (`check_work_home`), and a budget is always required (`plan`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::frontmatter::ROLES;

/// The mission-level ceiling for one Council (`specs/003` §3.2): no single case may be priced above it.
const MAX_CASE_COST_USD: f64 = 20.0;
const MAX_CASE_SECS: u64 = 3600;
/// Written into a working home by training mode, so that a later run knows the directory is its own.
pub const MARKER: &str = ".delphi-train";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One prompt against a fixture repository.
    Task,
    /// A scripted operator conversation, with the card answers it gives.
    Scenario,
    /// A case written by a probe run and curated by a person.
    Probe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    Dev,
    Holdout,
}

/// A deterministic check on what a run left behind. Model-graded rubrics are separate (`rubric.md`, later).
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    /// The task ended `done`, `escalated`, `failed` or `cancelled`.
    Outcome(String),
    /// The number of events of a type in the run's log is at least, at most or exactly this.
    EventCount { event: String, at_least: Option<u64>, at_most: Option<u64> },
    /// The log holds no event of this type.
    EventAbsent(String),
    /// A fixture file is byte-identical after the run.
    FileUnchanged(String),
    FileExists(String),
    /// A fixture file contains this text after the run.
    FileContains { path: String, text: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub id: String,
    pub suite: String,
    pub kind: Kind,
    pub role: String,
    pub split: Split,
    pub tags: Vec<String>,
    pub max_cost_usd: f64,
    pub timeout_secs: u64,
    pub prompt: Option<String>,
    pub turns: Vec<String>,
    pub checks: Vec<Check>,
    pub dir: PathBuf,
}

/// Problems with one case or suite, one per line, each naming the file.
pub type Problems = Vec<String>;

fn str_of<'a>(t: &'a toml::Table, k: &str) -> Option<&'a str> {
    t.get(k).and_then(|v| v.as_str())
}

fn tag_ok(t: &str) -> bool {
    let b = t.as_bytes();
    let fm = b.len() == 6 && t.starts_with("FM-") && b[3].is_ascii_digit() && b[4] == b'.' && b[5].is_ascii_digit();
    fm || (!t.is_empty() && t.len() <= 40 && t.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'))
}

/// A path inside the fixture: relative, no `..`, no empty segments.
fn rel_ok(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.contains('\\') && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

fn parse_check(t: &toml::Table, at: &str, problems: &mut Problems) -> Option<Check> {
    let path = |problems: &mut Problems| -> Option<String> {
        match str_of(t, "path") {
            Some(p) if rel_ok(p) => Some(p.to_string()),
            Some(p) => {
                problems.push(format!("{}: path `{}` must be relative and stay inside the fixture", at, p));
                None
            }
            None => {
                problems.push(format!("{}: a `{}` check needs `path`", at, str_of(t, "kind").unwrap_or("?")));
                None
            }
        }
    };
    match str_of(t, "kind") {
        Some("outcome") => match str_of(t, "is") {
            Some(o @ ("done" | "escalated" | "failed" | "cancelled")) => Some(Check::Outcome(o.to_string())),
            _ => {
                problems.push(format!("{}: `outcome` needs `is` = done, escalated, failed or cancelled", at));
                None
            }
        },
        Some("event_count") => {
            let n = |k: &str| t.get(k).and_then(|v| v.as_integer()).filter(|n| *n >= 0).map(|n| n as u64);
            let (eq, lo, hi) = (n("eq"), n("at_least"), n("at_most"));
            let Some(event) = str_of(t, "event").filter(|e| !e.is_empty()) else {
                problems.push(format!("{}: `event_count` needs `event`", at));
                return None;
            };
            if eq.is_none() && lo.is_none() && hi.is_none() {
                problems.push(format!("{}: `event_count` needs `eq`, `at_least` or `at_most`", at));
                return None;
            }
            if eq.is_some() && (lo.is_some() || hi.is_some()) {
                problems.push(format!("{}: `eq` cannot be combined with `at_least` or `at_most`", at));
                return None;
            }
            if let (Some(lo), Some(hi)) = (lo, hi) {
                if lo > hi {
                    problems.push(format!("{}: `at_least` is above `at_most`", at));
                    return None;
                }
            }
            Some(Check::EventCount { event: event.to_string(), at_least: eq.or(lo), at_most: eq.or(hi) })
        }
        Some("event_absent") => match str_of(t, "event").filter(|e| !e.is_empty()) {
            Some(e) => Some(Check::EventAbsent(e.to_string())),
            None => {
                problems.push(format!("{}: `event_absent` needs `event`", at));
                None
            }
        },
        Some("file_unchanged") => path(problems).map(Check::FileUnchanged),
        Some("file_exists") => path(problems).map(Check::FileExists),
        Some("file_contains") => {
            let p = path(problems)?;
            match str_of(t, "text").filter(|x| !x.is_empty()) {
                Some(text) => Some(Check::FileContains { path: p, text: text.to_string() }),
                None => {
                    problems.push(format!("{}: `file_contains` needs `text`", at));
                    None
                }
            }
        }
        other => {
            problems.push(format!("{}: unknown check kind `{}`", at, other.unwrap_or("(none)")));
            None
        }
    }
}

/// Read one case directory. `suite` is the suite directory's name, which the case must agree with.
pub fn load_case(dir: &Path, suite: &str) -> Result<Case, Problems> {
    let file = dir.join("case.toml");
    let name = format!("{}/{}", suite, dir.file_name().and_then(|n| n.to_str()).unwrap_or("?"));
    let text = fs::read_to_string(&file).map_err(|e| vec![format!("{}: cannot read case.toml ({})", name, e)])?;
    let t = text.parse::<toml::Table>().map_err(|e| vec![format!("{}: case.toml is not valid TOML ({})", name, e.message())])?;
    let mut p: Problems = Vec::new();
    let at = name.clone();

    let id = str_of(&t, "id").unwrap_or("").to_string();
    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if id.is_empty() || id != dir_name {
        p.push(format!("{}: `id` must be present and equal the directory name `{}`", at, dir_name));
    }
    if str_of(&t, "suite").is_some_and(|s| s != suite) {
        p.push(format!("{}: `suite` says `{}` but the case sits in suite `{}`", at, str_of(&t, "suite").unwrap_or(""), suite));
    }
    let kind = match str_of(&t, "kind") {
        Some("task") => Kind::Task,
        Some("scenario") => Kind::Scenario,
        Some("probe") => Kind::Probe,
        _ => {
            p.push(format!("{}: `kind` must be task, scenario or probe", at));
            Kind::Task
        }
    };
    let role = str_of(&t, "role").unwrap_or("").to_string();
    if !ROLES.contains(&role.as_str()) {
        p.push(format!("{}: `role` `{}` is not a canonical role ID", at, role));
    }
    let split = match str_of(&t, "split") {
        Some("dev") => Split::Dev,
        Some("holdout") => Split::Holdout,
        _ => {
            p.push(format!("{}: `split` must be dev or holdout", at));
            Split::Dev
        }
    };
    let tags: Vec<String> = t.get("tags").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    for tag in &tags {
        if !tag_ok(tag) {
            p.push(format!("{}: tag `{}` is neither a failure-mode ID (FM-1.2) nor a lowercase word", at, tag));
        }
    }
    if kind == Kind::Probe && tags.is_empty() {
        p.push(format!("{}: a probe case names the failure mode it probes in `tags`", at));
    }
    let max_cost_usd = t.get("max_cost_usd").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).unwrap_or(0.0);
    if !(max_cost_usd > 0.0 && max_cost_usd <= MAX_CASE_COST_USD) {
        p.push(format!("{}: `max_cost_usd` must be above 0 and at most {}", at, MAX_CASE_COST_USD));
    }
    let timeout_secs = t.get("timeout_secs").and_then(|v| v.as_integer()).filter(|n| *n > 0).map(|n| n as u64).unwrap_or(0);
    if timeout_secs == 0 || timeout_secs > MAX_CASE_SECS {
        p.push(format!("{}: `timeout_secs` must be between 1 and {}", at, MAX_CASE_SECS));
    }
    let prompt = str_of(&t, "prompt").map(String::from).filter(|s| !s.trim().is_empty());
    let turns: Vec<String> = t.get("turns").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_table().and_then(|x| str_of(x, "say")).map(String::from)).collect()).unwrap_or_default();
    match kind {
        Kind::Task | Kind::Probe if prompt.is_none() => p.push(format!("{}: a {} case needs a `prompt`", at, if kind == Kind::Task { "task" } else { "probe" })),
        Kind::Scenario if turns.is_empty() => p.push(format!("{}: a scenario needs at least one `[[turns]]` with `say`", at)),
        _ => {}
    }
    if kind == Kind::Scenario && prompt.is_some() {
        p.push(format!("{}: a scenario's words go in `[[turns]]`, not `prompt`", at));
    }
    if matches!(kind, Kind::Task | Kind::Probe) && !dir.join("fixture").is_dir() {
        p.push(format!("{}: a {} case needs a `fixture/` directory", at, if kind == Kind::Task { "task" } else { "probe" }));
    }
    let mut checks = Vec::new();
    for (i, c) in t.get("check").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
        match c.as_table() {
            Some(ct) => {
                if let Some(c) = parse_check(ct, &format!("{} check #{}", at, i + 1), &mut p) {
                    checks.push(c);
                }
            }
            None => p.push(format!("{}: check #{} is not a table", at, i + 1)),
        }
    }
    if t.get("check").and_then(|v| v.as_array()).is_none_or(|a| a.is_empty()) {
        p.push(format!("{}: at least one `[[check]]` is required; deterministic checks come first (specs/031 §4.3)", at));
    }
    if !p.is_empty() {
        return Err(p);
    }
    Ok(Case { id, suite: suite.to_string(), kind, role, split, tags, max_cost_usd, timeout_secs, prompt, turns, checks, dir: dir.to_path_buf() })
}

/// Every case of one suite directory, in id order, and the problems found. A directory in `cases/` without a
/// `case.toml` is a problem, not something to skip.
pub fn load_suite(suite_dir: &Path) -> (Vec<Case>, Problems) {
    let suite = suite_dir.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
    let mut cases = Vec::new();
    let mut problems = Vec::new();
    let mut dirs: Vec<PathBuf> = match fs::read_dir(suite_dir.join("cases")) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect(),
        Err(_) => {
            problems.push(format!("{}: no `cases/` directory", suite));
            return (cases, problems);
        }
    };
    dirs.sort();
    let mut seen = BTreeSet::new();
    for d in dirs {
        match load_case(&d, &suite) {
            Ok(c) => {
                if !seen.insert(c.id.clone()) {
                    problems.push(format!("{}: duplicate case id `{}`", suite, c.id));
                }
                cases.push(c);
            }
            Err(mut p) => problems.append(&mut p),
        }
    }
    if cases.is_empty() && problems.is_empty() {
        problems.push(format!("{}: the suite has no cases", suite));
    }
    (cases, problems)
}

/// The suites under `<repo>/dev/train/suites`, by name. `None` when the repository has no training directory:
/// training mode is for a source checkout (`specs/031` OD-3).
pub fn suites_dir(repo: &Path) -> Option<PathBuf> {
    let d = repo.join("dev/train/suites");
    d.is_dir().then_some(d)
}

fn suite_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// A working home is safe for a run only when training mode owns it: it does not exist yet, it is empty, or it
/// carries [`MARKER`]. Anything else may be an operator's real home (profile, ratified files, missions), and a run
/// would write into it. Stricter than a profile check on purpose: an empty-looking real home is still refused if it
/// holds anything at all.
pub fn check_work_home(home: &Path) -> Result<(), String> {
    if !home.exists() {
        return Ok(());
    }
    if !home.is_dir() {
        return Err(format!("{} exists and is not a directory", home.display()));
    }
    if home.join(MARKER).is_file() {
        return Ok(());
    }
    let empty = fs::read_dir(home).map(|mut rd| rd.next().is_none()).unwrap_or(false);
    if empty {
        return Ok(());
    }
    Err(format!("{} is not empty and was not made by training mode; training mode will not run in a home that may be yours (use a new directory)", home.display()))
}

/// Create the working home and mark it as training mode's own (`delphi train run` calls this before anything else).
pub fn prepare_work_home(home: &Path) -> Result<(), String> {
    check_work_home(home)?;
    fs::create_dir_all(home).map_err(|e| format!("cannot create {}: {}", home.display(), e))?;
    fs::write(home.join(MARKER), "made by `delphi train`; safe to delete\n").map_err(|e| format!("cannot mark {}: {}", home.display(), e))
}

/// What a run of `cases` would cost at most: each case's cap, once per seed.
pub fn worst_case_usd(cases: &[&Case], seeds: u32) -> f64 {
    cases.iter().map(|c| c.max_cost_usd).sum::<f64>() * f64::from(seeds)
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// `delphi train validate|plan`.
pub fn run(args: &[String]) -> i32 {
    let sub = args.first().map(|s| s.as_str());
    let repo = PathBuf::from(flag(args, "--repo").unwrap_or_else(|| ".".into()));
    let Some(root) = suites_dir(&repo) else {
        eprintln!("delphi train: {} has no dev/train/suites; training mode is for a source checkout (pass --repo <path>)", repo.display());
        return 2;
    };
    let only = flag(args, "--suite");
    let mut names = suite_names(&root);
    if let Some(s) = &only {
        if !names.contains(s) {
            eprintln!("delphi train: no suite `{}` (have: {})", s, names.join(", "));
            return 2;
        }
        names = vec![s.clone()];
    }
    let mut all: Vec<Case> = Vec::new();
    let mut problems: Problems = Vec::new();
    for n in &names {
        let (c, mut p) = load_suite(&root.join(n));
        all.extend(c);
        problems.append(&mut p);
    }
    match sub {
        Some("validate") => {
            for n in &names {
                let cs: Vec<&Case> = all.iter().filter(|c| &c.suite == n).collect();
                let hold = cs.iter().filter(|c| c.split == Split::Holdout).count();
                println!("{:<18}{} case(s), {} holdout", n, cs.len(), hold);
            }
            for p in &problems {
                println!("FAIL {}", p);
            }
            println!("delphi train validate: {} suite(s), {} case(s), {} problem(s)", names.len(), all.len(), problems.len());
            i32::from(!problems.is_empty())
        }
        Some("plan") => {
            if !problems.is_empty() {
                for p in &problems {
                    eprintln!("FAIL {}", p);
                }
                eprintln!("delphi train: fix the cases first (`delphi train validate`)");
                return 1;
            }
            let Some(budget) = flag(args, "--budget-usd").and_then(|b| b.parse::<f64>().ok()).filter(|b| *b > 0.0) else {
                eprintln!("delphi train plan: --budget-usd <dollars> is required; a run is always capped");
                return 2;
            };
            if let Some(h) = flag(args, "--home") {
                match check_work_home(Path::new(&h)) {
                    Ok(()) => println!("working home {}: acceptable", h),
                    Err(e) => {
                        eprintln!("delphi train plan: {}", e);
                        return 2;
                    }
                }
            }
            let seeds: u32 = flag(args, "--seeds").and_then(|s| s.parse().ok()).filter(|s| (1..=20).contains(s)).unwrap_or(1);
            let split = flag(args, "--split").unwrap_or_else(|| "dev".into());
            let chosen: Vec<&Case> = match split.as_str() {
                "dev" => all.iter().filter(|c| c.split == Split::Dev).collect(),
                "holdout" => all.iter().filter(|c| c.split == Split::Holdout).collect(),
                "all" => all.iter().collect(),
                other => {
                    eprintln!("delphi train plan: --split is dev, holdout or all, not `{}`", other);
                    return 2;
                }
            };
            for c in &chosen {
                println!("{}/{:<34}{:<9}{:<13}up to ${:.2}, {}s", c.suite, c.id, format!("{:?}", c.kind).to_lowercase(), c.role, c.max_cost_usd, c.timeout_secs);
            }
            let worst = worst_case_usd(&chosen, seeds);
            println!("{} case(s) x {} seed(s): at most ${:.2} against a budget of ${:.2} (nothing was run)", chosen.len(), seeds, worst, budget);
            if chosen.is_empty() {
                eprintln!("delphi train plan: no cases in the `{}` split", split);
                return 1;
            }
            if worst > budget {
                eprintln!("delphi train plan: the worst case is over the budget; raise --budget-usd, cut --seeds, or choose a smaller suite");
                return 3;
            }
            0
        }
        Some("run") => {
            if !problems.is_empty() {
                for p in &problems {
                    eprintln!("FAIL {}", p);
                }
                eprintln!("delphi train: fix the cases first (`delphi train validate`)");
                return 1;
            }
            let split = flag(args, "--split").unwrap_or_else(|| "dev".into());
            let one = flag(args, "--case");
            let chosen: Vec<&Case> = all
                .iter()
                .filter(|c| match split.as_str() {
                    "holdout" => c.split == Split::Holdout,
                    "all" => true,
                    _ => c.split == Split::Dev,
                })
                .filter(|c| one.as_ref().is_none_or(|id| &c.id == id))
                .collect();
            crate::train_run::cli(args, &chosen)
        }
        _ => {
            eprintln!("usage: delphi train validate [--suite <name>] [--repo <path>]\n       delphi train plan --budget-usd <n> [--suite <name>] [--split dev|holdout|all] [--seeds <n>] [--home <new dir>] [--repo <path>]\n       delphi train run --budget-usd <n> --template-home <dir> --work-dir <new dir> [--overlay <dir>] [--suite <name>] [--case <id>] [--split dev|holdout|all] [--seeds <n>] [--workspace <name>] [--delphi-bin <path>] [--out <file>] [--dry-run]");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
id = "ladder"
kind = "task"
role = "builder"
split = "dev"
tags = ["FM-3.1", "retry-ladder"]
max_cost_usd = 2.0
timeout_secs = 600
prompt = "Make the failing test pass."

[[check]]
kind = "outcome"
is = "escalated"

[[check]]
kind = "event_count"
event = "build.attempt"
eq = 3

[[check]]
kind = "file_unchanged"
path = "tests/keep.rs"
"#;

    fn suite(t: &Path, suite: &str, id: &str, case: &str, fixture: bool) -> PathBuf {
        let s = t.join(suite);
        let c = s.join("cases").join(id);
        fs::create_dir_all(&c).unwrap();
        if fixture {
            fs::create_dir_all(c.join("fixture")).unwrap();
        }
        fs::write(c.join("case.toml"), case).unwrap();
        s
    }

    fn problems_of(case: &str) -> Problems {
        let t = tempfile::tempdir().unwrap();
        let s = suite(t.path(), "orch", "ladder", case, true);
        load_suite(&s).1
    }

    #[test]
    fn a_good_case_loads_with_its_checks() {
        let t = tempfile::tempdir().unwrap();
        let s = suite(t.path(), "orch", "ladder", GOOD, true);
        let (cases, p) = load_suite(&s);
        assert!(p.is_empty(), "{:?}", p);
        assert_eq!(cases.len(), 1);
        let c = &cases[0];
        assert_eq!((c.kind, c.split, c.role.as_str(), c.timeout_secs), (Kind::Task, Split::Dev, "builder", 600));
        assert_eq!(c.checks.len(), 3);
        assert_eq!(c.checks[1], Check::EventCount { event: "build.attempt".into(), at_least: Some(3), at_most: Some(3) });
    }

    #[test]
    fn each_malformed_field_is_named() {
        let bad = |from: &str, to: &str, want: &str| {
            let p = problems_of(&GOOD.replace(from, to));
            assert!(p.iter().any(|x| x.contains(want)), "expected `{}` in {:?}", want, p);
        };
        bad("id = \"ladder\"", "id = \"other\"", "equal the directory name");
        bad("kind = \"task\"", "kind = \"chat\"", "`kind` must be");
        bad("role = \"builder\"", "role = \"hermes\"", "canonical role ID");
        bad("split = \"dev\"", "split = \"test\"", "`split` must be");
        bad("\"retry-ladder\"", "\"Retry Ladder\"", "neither a failure-mode ID");
        bad("max_cost_usd = 2.0", "max_cost_usd = 99", "`max_cost_usd`");
        bad("timeout_secs = 600", "timeout_secs = 0", "`timeout_secs`");
        bad("prompt = \"Make the failing test pass.\"", "", "needs a `prompt`");
        bad("is = \"escalated\"", "is = \"great\"", "`outcome` needs `is`");
        bad("eq = 3", "eq = 3\nat_most = 4", "cannot be combined");
        bad("path = \"tests/keep.rs\"", "path = \"../escape\"", "stay inside the fixture");
        bad("kind = \"file_unchanged\"", "kind = \"vibes\"", "unknown check kind");
    }

    #[test]
    fn a_case_needs_a_fixture_and_a_check() {
        let t = tempfile::tempdir().unwrap();
        let s = suite(t.path(), "orch", "ladder", GOOD, false);
        assert!(load_suite(&s).1.iter().any(|x| x.contains("`fixture/`")));
        let no_checks = GOOD.split("[[check]]").next().unwrap();
        assert!(problems_of(no_checks).iter().any(|x| x.contains("at least one `[[check]]`")));
    }

    #[test]
    fn a_scenario_speaks_in_turns_and_a_probe_names_its_failure_mode() {
        let scenario = GOOD.replace("kind = \"task\"", "kind = \"scenario\"").replace("prompt = \"Make the failing test pass.\"", "[[turns]]\nsay = \"no, use pnpm\"\n");
        let t = tempfile::tempdir().unwrap();
        let s = suite(t.path(), "orch", "ladder", &scenario, false);
        let (cases, p) = load_suite(&s);
        assert!(p.is_empty(), "a scenario needs no fixture: {:?}", p);
        assert_eq!(cases[0].turns, vec!["no, use pnpm"]);
        let probe = GOOD.replace("kind = \"task\"", "kind = \"probe\"").replace("tags = [\"FM-3.1\", \"retry-ladder\"]", "tags = []");
        assert!(problems_of(&probe).iter().any(|x| x.contains("names the failure mode")));
    }

    #[test]
    fn a_suite_reports_duplicates_stray_directories_and_emptiness() {
        let t = tempfile::tempdir().unwrap();
        let s = suite(t.path(), "orch", "ladder", GOOD, true);
        fs::create_dir_all(s.join("cases/stray")).unwrap();
        assert!(load_suite(&s).1.iter().any(|x| x.contains("cannot read case.toml")));
        let e = t.path().join("empty");
        fs::create_dir_all(e.join("cases")).unwrap();
        assert!(load_suite(&e).1.iter().any(|x| x.contains("no cases")));
        assert!(load_suite(&t.path().join("none")).1.iter().any(|x| x.contains("no `cases/`")));
    }

    #[test]
    fn a_working_home_must_be_new_empty_or_ours() {
        let t = tempfile::tempdir().unwrap();
        assert!(check_work_home(&t.path().join("fresh")).is_ok());
        assert!(check_work_home(t.path()).is_ok(), "an empty directory is fine");
        let theirs = t.path().join("theirs");
        fs::create_dir_all(theirs.join("operator")).unwrap();
        fs::write(theirs.join("operator/profile.md"), "I like pnpm\n").unwrap();
        assert!(check_work_home(&theirs).unwrap_err().contains("not empty"));
        // Even a home holding only an empty-looking config is refused unless training mode made it.
        let cfg = t.path().join("cfg");
        fs::create_dir_all(&cfg).unwrap();
        fs::write(cfg.join("config.toml"), "").unwrap();
        assert!(check_work_home(&cfg).is_err());
        let ours = t.path().join("ours");
        prepare_work_home(&ours).unwrap();
        fs::write(ours.join("anything"), "x").unwrap();
        assert!(check_work_home(&ours).is_ok(), "a marked home may be reused");
        let file = t.path().join("afile");
        fs::write(&file, "x").unwrap();
        assert!(check_work_home(&file).unwrap_err().contains("not a directory"));
    }

    #[test]
    fn the_worst_case_is_each_cap_once_per_seed() {
        let t = tempfile::tempdir().unwrap();
        let s = suite(t.path(), "orch", "ladder", GOOD, true);
        let (cases, _) = load_suite(&s);
        let refs: Vec<&Case> = cases.iter().collect();
        assert_eq!(worst_case_usd(&refs, 3), 6.0);
    }

    #[test]
    fn plan_requires_a_budget_and_refuses_one_that_is_too_small() {
        let t = tempfile::tempdir().unwrap();
        suite(&t.path().join("dev/train/suites"), "orch", "ladder", GOOD, true);
        let a = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter().chain(["--repo".to_string(), t.path().display().to_string()]).collect() };
        assert_eq!(run(&a(&["plan"])), 2, "no budget");
        assert_eq!(run(&a(&["plan", "--budget-usd", "0"])), 2);
        assert_eq!(run(&a(&["plan", "--budget-usd", "1.5"])), 3, "one case may cost 2");
        assert_eq!(run(&a(&["plan", "--budget-usd", "2"])), 0);
        let real = t.path().join("real-home");
        fs::create_dir_all(real.join("operator")).unwrap();
        assert_eq!(run(&a(&["plan", "--budget-usd", "2", "--home", &real.display().to_string()])), 2, "a home that may be yours is refused");
        assert_eq!(run(&a(&["plan", "--budget-usd", "2", "--home", &t.path().join("new").display().to_string()])), 0);
        assert_eq!(run(&a(&["plan", "--budget-usd", "2", "--seeds", "2"])), 3);
        assert_eq!(run(&a(&["plan", "--budget-usd", "9", "--split", "holdout"])), 1, "no holdout cases");
        assert_eq!(run(&a(&["validate"])), 0);
        assert_eq!(run(&a(&["validate", "--suite", "missing"])), 2);
        let not_a_checkout = tempfile::tempdir().unwrap();
        assert_eq!(run(&["validate".into(), "--repo".into(), not_a_checkout.path().display().to_string()]), 2);
    }
}
