//! `delphi train run`: execute training cases against a baseline and, optionally, a candidate overlay (`specs/031` §4, slice L6b).
//!
//! Every run spends money, so the safety rules of L6a come first: the working directory must be training mode's own
//! (`train::check_work_home`), the budget is required, and a case whose cap would exceed what is left is never started.
//! Each case runs as a **subprocess** of `delphi run`, in a fresh copy of a template home (one an operator ratified once),
//! against a fresh git repository made from the case's `fixture/`. The runner never touches `$DELPHI_HOME`.
//!
//! The overlay (§ "Overlay" below) is how a candidate change to a ratified file reaches a run without touching any real
//! home: the runner writes the file into the temp home the way `/ratify` would, by hash.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use delphi_core::event::EventEnvelope;
use delphi_core::lock;
use delphi_engine::learn::{task_records, Outcome, TaskRecord};
use serde_json::{json, Value};

use crate::train::{self, Case, Check, Kind};

// ── overlay ─────────────────────────────────────────────────────────────────────────────────────────────────────

/// Replace ratified, protected files of a **temp** home's workspace with the ones under `overlay`, so a candidate
/// version of an agent manifest, a skill or a heuristics file can be run without ratifying anything real.
///
/// Spike result (2026-10-26): the daemon loads `local/ratified/<logical>` and checks it against `local/ratified.lock`;
/// release heuristics are checked against `base/manifest.lock`. Nothing is signed on load. So the overlay writes the
/// ratified copy, the live copy (so a later `doctor` agrees), and the matching lock entry. Only paths the lock already
/// treats as protected are accepted; anything else (a config file, a path with `..`) is refused.
pub fn apply_overlay(ws_dir: &Path, overlay: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    collect_files(overlay, overlay, &mut files)?;
    if files.is_empty() {
        return Err(format!("overlay {} holds no files", overlay.display()));
    }
    let lock_path = ws_dir.join("local/ratified.lock");
    let mut lk = lock::read(&lock_path).map_err(|e| format!("the template home has no usable ratified.lock: {}", e))?;
    let mut manifest = ws_dir.join("base/manifest.lock").is_file().then(|| lock::read(&ws_dir.join("base/manifest.lock"))).transpose()?;
    let mut applied = Vec::new();
    for logical in files {
        if logical.split('/').any(|p| p == ".." || p.is_empty()) || !lock::is_protected(&logical) {
            return Err(format!("overlay file `{}` is not a ratified, protected file (agents/*/AGENT.md, skills/*/SKILL.md, heuristics, GOVERNANCE.md, REVIEW.md)", logical));
        }
        let bytes = fs::read(overlay.join(&logical)).map_err(|e| format!("{}: {}", logical, e))?;
        let h = lock::hash(&bytes);
        let put = |p: PathBuf| -> Result<(), String> {
            fs::create_dir_all(p.parent().unwrap_or(ws_dir)).map_err(|e| e.to_string())?;
            fs::write(&p, &bytes).map_err(|e| format!("{}: {}", p.display(), e))
        };
        put(ws_dir.join("local/ratified").join(&logical))?;
        // The live copy lives where `init` put it: release files in base/, team files in team/, private ones in local/.
        let live = match logical.as_str() {
            "context/team-heuristics.md" => ws_dir.join("team").join(&logical),
            "heuristics.md" => ws_dir.join("local").join(&logical),
            _ => ws_dir.join("base").join(&logical),
        };
        put(live)?;
        lk.files.insert(logical.clone(), h.clone());
        if let (Some(m), "context/heuristics.md") = (manifest.as_mut(), logical.as_str()) {
            m.files.insert(logical.clone(), h);
        }
        applied.push(logical);
    }
    lock::write(&lock_path, &lk).map_err(|e| e.to_string())?;
    if let Some(m) = manifest {
        lock::write(&ws_dir.join("base/manifest.lock"), &m).map_err(|e| e.to_string())?;
    }
    Ok(applied)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir).map_err(|e| format!("{}: {}", dir.display(), e))?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() {
            return Err(format!("{} is a symlink; an overlay holds plain files", p.display()));
        } else if ft.is_dir() {
            collect_files(root, &p, out)?;
        } else {
            out.push(p.strip_prefix(root).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

// ── preparing a run ─────────────────────────────────────────────────────────────────────────────────────────────

fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("{}: {}", dst.display(), e))?;
    for e in fs::read_dir(src).map_err(|e| format!("{}: {}", src.display(), e))?.flatten() {
        let (from, to) = (e.path(), dst.join(e.file_name()));
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() {
            let target = fs::read_link(&from).map_err(|e| e.to_string())?;
            std::os::unix::fs::symlink(target, &to).map_err(|e| format!("{}: {}", to.display(), e))?;
        } else if ft.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| format!("{}: {}", to.display(), e))?;
        }
    }
    Ok(())
}

/// Point `[workspace] projects` at `repo`; the one line is the only edit, and it is not a pinned block.
pub fn set_projects(config: &str, repo: &Path) -> Result<String, String> {
    let re = regex::Regex::new(r"(?m)^projects\s*=\s*\[[^\]]*\]").expect("static pattern");
    if re.find_iter(config).count() != 1 {
        return Err("the template workspace config must have exactly one `projects = [...]` line".into());
    }
    let quoted = repo.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    Ok(re.replace(config, regex::NoExpand(&format!("projects = [\"{}\"]", quoted))).into_owned())
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("git").args(["-c", "user.name=delphi-train", "-c", "user.email=train@invalid", "-c", "commit.gpgsign=false"]).args(args).current_dir(dir).output().map_err(|e| format!("git: {}", e))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// A fresh repository holding exactly the case's fixture, committed.
fn make_repo(case: &Case, repo: &Path) -> Result<(), String> {
    if case.dir.join("fixture").is_dir() {
        copy_tree(&case.dir.join("fixture"), repo)?;
    } else {
        // A research case has no fixture; the workspace still needs a project, so it gets an empty repository.
        fs::create_dir_all(repo).map_err(|e| e.to_string())?;
    }
    git(repo, &["init", "-q", "-b", "main"])?;
    git(repo, &["add", "-A"])?;
    git(repo, &["commit", "-q", "--allow-empty", "-m", "fixture"])
}

// ── checks ──────────────────────────────────────────────────────────────────────────────────────────────────────

struct Ctx<'a> {
    events: &'a [EventEnvelope],
    record: Option<TaskRecord>,
    fixture: &'a Path,
    repo: &'a Path,
    /// What the task delivered: its final answer and any Markdown it wrote in the mission (`specs/037` §5.1).
    deliverable: String,
    truth: Option<&'a crate::train_truth::Truth>,
}

/// The task's final `task.output` text plus every Markdown file the mission wrote (reports, notes), in that order.
pub fn deliverable_of(events: &[EventEnvelope], task: Option<&str>, mission_dir: &Path) -> String {
    let mut out = String::new();
    if let Some(e) = events.iter().rev().find(|e| e.kind == "task.output" && (task.is_none() || e.task_id.as_deref() == task)) {
        out.push_str(e.payload["text"].as_str().unwrap_or(""));
    }
    let mut files = Vec::new();
    collect_md(mission_dir, &mut files);
    files.sort();
    for f in files {
        if let Ok(t) = fs::read_to_string(&f) {
            out.push_str("\n\n");
            out.push_str(&t);
        }
    }
    out
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let (p, name) = (e.path(), e.file_name().to_string_lossy().to_string());
        match e.file_type() {
            Ok(t) if t.is_dir() && name != "events" && !name.starts_with('.') => collect_md(&p, out),
            Ok(t) if t.is_file() && name.ends_with(".md") => out.push(p),
            _ => {}
        }
    }
}

fn outcome_word(r: &Option<TaskRecord>) -> &'static str {
    match r.as_ref().map(|r| &r.outcome) {
        Some(Outcome::Done) => "done",
        Some(Outcome::Escalated) => "escalated",
        Some(Outcome::Failed) => "failed",
        Some(Outcome::Cancelled) => "cancelled",
        _ => "open",
    }
}

/// `Ok` when the check passes, else what failed, in words.
fn eval_check(c: &Check, ctx: &Ctx) -> Result<(), String> {
    match c {
        Check::Outcome(want) => {
            let got = outcome_word(&ctx.record);
            (got == want).then_some(()).ok_or_else(|| format!("outcome is {}, wanted {}", got, want))
        }
        Check::EventCount { event, at_least, at_most } => {
            let n = ctx.events.iter().filter(|e| &e.kind == event).count() as u64;
            if at_least.is_some_and(|lo| n < lo) || at_most.is_some_and(|hi| n > hi) {
                Err(format!("{} `{}` event(s); wanted {}", n, event, match (at_least, at_most) {
                    (Some(a), Some(b)) if a == b => format!("exactly {}", a),
                    (Some(a), Some(b)) => format!("{} to {}", a, b),
                    (Some(a), None) => format!("at least {}", a),
                    (None, Some(b)) => format!("at most {}", b),
                    (None, None) => "any".into(),
                }))
            } else {
                Ok(())
            }
        }
        Check::EventAbsent(event) => {
            let n = ctx.events.iter().filter(|e| &e.kind == event).count();
            (n == 0).then_some(()).ok_or_else(|| format!("{} `{}` event(s); wanted none", n, event))
        }
        Check::FileUnchanged(p) => match (fs::read(ctx.fixture.join(p)), fs::read(ctx.repo.join(p))) {
            (Ok(a), Ok(b)) if a == b => Ok(()),
            (Ok(_), Ok(_)) => Err(format!("{} changed", p)),
            (_, Err(_)) => Err(format!("{} is missing after the run", p)),
            (Err(_), _) => Err(format!("{} is not in the fixture", p)),
        },
        Check::FactRecall { min } => {
            let Some(t) = ctx.truth else { return Err("no truth pack to compare against".into()) };
            let r = t.recall(&ctx.deliverable);
            if r.fraction + 1e-9 >= *min {
                Ok(())
            } else {
                Err(format!("fact recall {:.0}%, wanted {:.0}% (missing: {})", r.fraction * 100.0, min * 100.0, r.missing.join(", ")))
            }
        }
        Check::NoTraps => {
            let Some(t) = ctx.truth else { return Err("no truth pack to compare against".into()) };
            let hits = t.trap_hits(&ctx.deliverable);
            if hits.is_empty() { Ok(()) } else { Err(format!("fell into trap(s): {}", hits.join(", "))) }
        }
        Check::TextMatches { pattern, present } => {
            let found = regex::RegexBuilder::new(pattern).case_insensitive(true).build().map(|r| r.is_match(&ctx.deliverable)).unwrap_or(false);
            if found == *present {
                Ok(())
            } else {
                Err(format!("pattern `{}` {} in the deliverable", pattern, if *present { "is missing" } else { "appears" }))
            }
        }
        Check::FileExists(p) => ctx.repo.join(p).is_file().then_some(()).ok_or_else(|| format!("{} does not exist", p)),
        Check::FileContains { path, text } => match fs::read_to_string(ctx.repo.join(path)) {
            Ok(t) if t.contains(text.as_str()) => Ok(()),
            Ok(_) => Err(format!("{} does not contain the expected text", path)),
            Err(_) => Err(format!("{} is missing after the run", path)),
        },
    }
}

// ── one run ─────────────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub case: String,
    pub arm: String,
    pub seed: u32,
    pub exit_code: Option<i32>,
    pub wall_secs: f64,
    pub cost_usd: f64,
    /// `None` when the run did not produce a log to check (an infrastructure problem), which is not a failed case.
    pub passed: Option<bool>,
    pub failed_checks: Vec<String>,
    /// Measurements worth reading beside the verdict, such as fact recall (`specs/037`).
    pub notes: Vec<String>,
    /// What each model used in this run (`specs/037` §5.3), by the model id the log recorded.
    pub usage: BTreeMap<String, ModelUse>,
    /// `build.attempt` events of the run's task (0 for work that never builds) and models the pool swapped in mid-run.
    pub attempts: u32,
    pub promotions: u32,
    pub error: Option<String>,
}

/// One model's share of a run, summed from its `agent.usage` events.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelUse {
    pub calls: u32,
    /// Uncached input tokens.
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
    pub thinking: u64,
    pub cost_usd: f64,
}

/// Usage per model id over a run's events. Calls without a model id are filed under `unknown`.
pub fn usage_by_model(events: &[EventEnvelope]) -> BTreeMap<String, ModelUse> {
    let mut out: BTreeMap<String, ModelUse> = BTreeMap::new();
    for e in events.iter().filter(|e| e.kind == "agent.usage") {
        let u = out.entry(e.model_id.clone().unwrap_or_else(|| "unknown".into())).or_default();
        let n = |k: &str| e.payload[k].as_u64().unwrap_or(0);
        u.calls += 1;
        u.input += n("input_tokens");
        u.cache_read += n("cache_read_tokens");
        u.cache_write += n("cache_write_tokens");
        u.output += n("output_tokens");
        u.thinking += n("thinking_tokens");
        u.cost_usd += e.payload["cost_usd"].as_f64().unwrap_or(0.0);
    }
    out
}

impl ModelUse {
    /// Tokens read in (cached or not) and tokens written out (answer and thinking): the two columns a cost question needs.
    pub fn tokens_in(&self) -> u64 {
        self.input + self.cache_read + self.cache_write
    }

    pub fn tokens_out(&self) -> u64 {
        self.output + self.thinking
    }
}

impl Row {
    pub fn to_json(&self) -> Value {
        json!({"case": self.case, "arm": self.arm, "seed": self.seed, "exit_code": self.exit_code, "wall_secs": (self.wall_secs * 10.0).round() / 10.0,
               "cost_usd": (self.cost_usd * 10000.0).round() / 10000.0, "passed": self.passed, "failed_checks": self.failed_checks, "notes": self.notes, "error": self.error,
               "attempts": self.attempts, "promotions": self.promotions,
               "tokens_in": self.usage.values().map(ModelUse::tokens_in).sum::<u64>(), "tokens_out": self.usage.values().map(ModelUse::tokens_out).sum::<u64>(),
               "models": self.usage.iter().map(|(m, u)| (m.clone(), json!({"calls": u.calls, "tokens_in": u.tokens_in(), "tokens_out": u.tokens_out(), "cost_usd": (u.cost_usd * 10000.0).round() / 10000.0}))).collect::<serde_json::Map<String, Value>>()})
    }
}

#[derive(Debug, Clone)]
pub struct Arm {
    pub name: String,
    pub overlay: Option<PathBuf>,
    /// This arm's own template home (its routing is configuration, `specs/037` §2); `None` uses the run's default.
    pub template_home: Option<PathBuf>,
    /// Model ids (or prefixes) this arm may use. A run that used any other model is not graded: the arm was not staffed
    /// as declared (a quota ran out and the pool swapped a model in, say). Empty means no check.
    pub expect_models: Vec<String>,
}

impl Arm {
    pub fn new(name: &str, overlay: Option<PathBuf>) -> Arm {
        Arm { name: name.into(), overlay, template_home: None, expect_models: Vec::new() }
    }
}

/// Read `arms.toml`: `[[arm]]` tables with `name`, `template_home` (relative paths are from the file), optional `overlay`
/// and `expect_models`. Names are lowercase words so they can name directories and columns.
pub fn parse_arms(file: &Path) -> Result<Vec<Arm>, String> {
    let text = fs::read_to_string(file).map_err(|e| format!("{}: {}", file.display(), e))?;
    let t: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("{}: {}", file.display(), e.message()))?;
    let base = file.parent().unwrap_or(Path::new("."));
    let resolve = |p: &str| if Path::new(p).is_absolute() { PathBuf::from(p) } else { base.join(p) };
    let mut arms: Vec<Arm> = Vec::new();
    for (i, a) in t.get("arm").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
        let at = format!("{}: arm #{}", file.display(), i + 1);
        let a = a.as_table().ok_or(format!("{} is not a table", at))?;
        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(format!("{}: `name` must be lowercase letters, digits and `-`", at));
        }
        if arms.iter().any(|x| x.name == name) {
            return Err(format!("{}: two arms are named `{}`", at, name));
        }
        let home = a.get("template_home").and_then(|v| v.as_str()).map(resolve).ok_or(format!("{}: `{}` needs `template_home`", at, name))?;
        if !home.join("workspaces").is_dir() {
            return Err(format!("{}: `{}` template_home {} is not a Delphi home (no workspaces/)", at, name, home.display()));
        }
        let expect_models = a.get("expect_models").and_then(|v| v.as_array()).map(|v| v.iter().filter_map(|m| m.as_str().map(String::from)).collect()).unwrap_or_default();
        arms.push(Arm { name: name.into(), overlay: a.get("overlay").and_then(|v| v.as_str()).map(resolve), template_home: Some(home), expect_models });
    }
    if arms.is_empty() {
        return Err(format!("{}: no [[arm]] tables", file.display()));
    }
    Ok(arms)
}

#[derive(Debug, Clone)]
pub struct Config {
    pub template_home: PathBuf,
    pub work_dir: PathBuf,
    pub workspace: String,
    pub budget_usd: f64,
    pub seeds: u32,
    pub arms: Vec<Arm>,
    pub delphi_bin: PathBuf,
    /// Extra environment for the subprocess; tests use it to steer a fake binary.
    pub env: Vec<(String, String)>,
    /// Added to each case's own timeout before the runner kills the process.
    pub kill_grace: Duration,
    /// Run a research case whose truth pack the operator has not verified (a draft).
    pub allow_unverified: bool,
}

/// Run `child` to completion or kill it at `deadline`; output goes to files so a full pipe never blocks it.
fn run_process(cmd: &mut Command, out: &Path, err: &Path, limit: Duration) -> Result<(Option<i32>, bool), String> {
    let stdout = fs::File::create(out).map_err(|e| e.to_string())?;
    let stderr = fs::File::create(err).map_err(|e| e.to_string())?;
    let mut child = cmd.stdin(Stdio::null()).stdout(stdout).stderr(stderr).spawn().map_err(|e| format!("could not start {:?}: {}", cmd.get_program(), e))?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok((status.code(), false));
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return Ok((None, true));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn summary_of(stdout: &str) -> Option<Value> {
    stdout.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok().filter(|v| v["type"] == "run.summary"))
}

/// One case, one seed, one arm, in its own directory under the working directory.
pub fn run_one(cfg: &Config, case: &Case, arm: &Arm, seed: u32, index: usize) -> Row {
    let label = format!("{}/{}", case.suite, case.id);
    let mut row = Row { case: label.clone(), arm: arm.name.clone(), seed, exit_code: None, wall_secs: 0.0, cost_usd: 0.0, passed: None, failed_checks: Vec::new(), notes: Vec::new(), usage: BTreeMap::new(), attempts: 0, promotions: 0, error: None };
    if let Err(e) = run_inner(cfg, case, arm, seed, index, &mut row) {
        row.error = Some(e);
        row.passed = None;
    }
    row
}

fn run_inner(cfg: &Config, case: &Case, arm: &Arm, seed: u32, index: usize, row: &mut Row) -> Result<(), String> {
    if case.kind == Kind::Scenario {
        return Err("scenario cases are not run yet (task and probe cases only)".into());
    }
    if case.truth.as_ref().is_some_and(|t| !t.verified()) && !cfg.allow_unverified {
        return Err("the truth pack is a draft (not verified by the operator); pass --allow-unverified to run it anyway".into());
    }
    let dir = cfg.work_dir.join(format!("{:03}-{}-{}-s{}", index, arm.name, case.id, seed));
    let (home, repo) = (dir.join("home"), dir.join("repo"));
    copy_tree(arm.template_home.as_ref().unwrap_or(&cfg.template_home), &home)?;
    let ws_dir = home.join("workspaces").join(&cfg.workspace);
    let cfg_path = ws_dir.join("config.toml");
    let text = fs::read_to_string(&cfg_path).map_err(|e| format!("template home has no workspace `{}` ({})", cfg.workspace, e))?;
    make_repo(case, &repo)?;
    fs::write(&cfg_path, set_projects(&text, &repo)?).map_err(|e| e.to_string())?;
    if let Some(o) = &arm.overlay {
        apply_overlay(&ws_dir, o)?;
    }

    let prompt = case.prompt.clone().unwrap_or_default();
    // A builder case goes through the build flow; any other role is addressed by name, as in the console.
    let input = if case.role == "builder" { format!("/build {}", prompt) } else { format!("@{} {}", case.role, prompt) };
    let mut cmd = Command::new(&cfg.delphi_bin);
    cmd.args(["run", &input, "--json", "--on-card", "deny", "--workspace", &cfg.workspace, "--home"]).arg(&home).args(["--budget", &format!("{:.2}", case.max_cost_usd), "--timeout", &format!("{}s", case.timeout_secs)]);
    cmd.current_dir(&repo).env_remove("DELPHI_HOME").env_remove("DELPHI_WORKSPACE");
    for (k, v) in &cfg.env {
        cmd.env(k, v);
    }
    let (stdout_path, stderr_path) = (dir.join("stdout.txt"), dir.join("stderr.txt"));
    let started = Instant::now();
    let (code, timed_out) = run_process(&mut cmd, &stdout_path, &stderr_path, Duration::from_secs(case.timeout_secs) + cfg.kill_grace)?;
    row.wall_secs = started.elapsed().as_secs_f64();
    row.exit_code = code;
    if timed_out {
        return Err(format!("killed after {}s", case.timeout_secs + cfg.kill_grace.as_secs()));
    }
    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let summary = summary_of(&stdout).ok_or_else(|| format!("no run summary (exit {:?}); see {}", code, stderr_path.display()))?;
    // Exit 4 is the runner's own trouble (daemon, access), not a verdict on the case.
    if code == Some(4) {
        return Err(format!("infrastructure failure: {}", summary["error"].as_str().or(summary["reason"].as_str()).unwrap_or("see stderr")));
    }
    let mission = summary["mission_id"].as_str().filter(|m| !m.is_empty() && !m.contains(['/', '\\', '.'])).ok_or("the summary names no mission")?;
    let events = delphi_core::log::replay(&ws_dir.join("local/missions").join(mission)).map_err(|e| format!("could not read the mission log: {}", e))?;
    row.usage = usage_by_model(&events);
    row.cost_usd = row.usage.values().map(|u| u.cost_usd).sum();
    row.promotions = events.iter().filter(|e| e.kind == "model.promoted").count() as u32;
    // An arm is a staffing decision. If any other model did the work, the run says nothing about the arm.
    let strangers: Vec<&String> = row.usage.keys().filter(|m| !arm.expect_models.is_empty() && !arm.expect_models.iter().any(|x| m.starts_with(x.as_str()))).collect();
    if !strangers.is_empty() {
        return Err(format!("arm staffing violated: used {} but `{}` expects {}", strangers.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), arm.name, arm.expect_models.join(", ")));
    }
    // A build stops on its own branch: check the commit it produced, as the bench does.
    let sha = summary["merge_commit"].as_str().or(summary["ready_to_merge"]["sha"].as_str());
    if let Some(sha) = sha.filter(|s| s.chars().all(|c| c.is_ascii_hexdigit())) {
        git(&repo, &["-c", "advice.detachedHead=false", "checkout", "-q", sha])?;
    }
    let records = task_records(&events);
    let task = summary["task_id"].as_str();
    row.attempts = records.iter().find(|r| Some(r.task_id.as_str()) == task).or(records.last()).map_or(0, |r| r.attempts);
    let record = records.iter().find(|r| Some(r.task_id.as_str()) == task).or(records.last()).cloned();
    let deliverable = deliverable_of(&events, task, &ws_dir.join("local/missions").join(mission));
    let ctx = Ctx { events: &events, record, fixture: &case.dir.join("fixture"), repo: &repo, deliverable, truth: case.truth.as_ref() };
    if let Some(t) = ctx.truth {
        let r = t.recall(&ctx.deliverable);
        row.notes.push(format!("fact recall {:.0}% ({} of {} facts{})", r.fraction * 100.0, r.stated.len(), t.facts.len(), if r.missing.is_empty() { String::new() } else { format!("; missing {}", r.missing.join(", ")) }));
        let traps = t.trap_hits(&ctx.deliverable);
        if !traps.is_empty() {
            row.notes.push(format!("traps: {}", traps.join(", ")));
        }
    }
    row.failed_checks = case.checks.iter().filter_map(|c| eval_check(c, &ctx).err()).collect();
    row.passed = Some(row.failed_checks.is_empty());
    Ok(())
}

// ── the whole run ───────────────────────────────────────────────────────────────────────────────────────────────

/// Run `cases` for every arm and seed, stopping before any run whose cap would exceed the budget left.
/// Returns the rows and the cases not started for lack of budget.
pub fn run_all(cfg: &Config, cases: &[&Case]) -> (Vec<Row>, Vec<String>) {
    let mut rows = Vec::new();
    let mut skipped = Vec::new();
    let mut spent = 0.0;
    let mut index = 0;
    for seed in 1..=cfg.seeds {
        for case in cases {
            for arm in &cfg.arms {
                if spent + case.max_cost_usd > cfg.budget_usd {
                    skipped.push(format!("{}/{} [{}] seed {}", case.suite, case.id, arm.name, seed));
                    continue;
                }
                index += 1;
                let row = run_one(cfg, case, arm, seed, index);
                // What the run really cost counts; a run that could not be read is charged its whole cap.
                spent += if row.passed.is_some() { row.cost_usd } else { case.max_cost_usd };
                rows.push(row);
            }
        }
    }
    (rows, skipped)
}

// ── the report ──────────────────────────────────────────────────────────────────────────────────────────────────

/// Numbers only: pass rate per case and arm, the cost, and the change from the first arm (the baseline) to each other arm.
/// A change is called evidence only when there are at least three seeds and the arms are fully separated (every run of one
/// passes and every run of the other fails); anything else is labelled "no evidence", because with few seeds a model run's
/// own spread is as large as most effects. Model output is never printed.
pub fn report(rows: &[Row], arms: &[String], skipped: &[String]) -> String {
    let mut by: BTreeMap<(&str, &str), Vec<&Row>> = BTreeMap::new();
    for r in rows {
        by.entry((r.case.as_str(), r.arm.as_str())).or_default().push(r);
    }
    let cases: Vec<&str> = by.keys().map(|(c, _)| *c).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    let mut out = String::new();
    for case in cases {
        out.push_str(&format!("{}\n", case));
        let rate = |arm: &str| -> Option<(usize, usize, Vec<bool>)> {
            let rs = by.get(&(case, arm))?;
            let graded: Vec<bool> = rs.iter().filter_map(|r| r.passed).collect();
            Some((graded.iter().filter(|p| **p).count(), graded.len(), graded))
        };
        for a in arms {
            if let Some((pass, n, _)) = rate(a) {
                let rs = &by[&(case, a.as_str())];
                let cost: f64 = rs.iter().map(|r| r.cost_usd).sum();
                let ungraded = rs.len() - n;
                out.push_str(&format!("  {:<12} {}/{} passed   ${:.2}{}\n", a, pass, n, cost, if ungraded > 0 { format!("   ({} run(s) not graded: infrastructure)", ungraded) } else { String::new() }));
            }
        }
        if let (Some(base), Some(first)) = (arms.first(), arms.first().and_then(|a| rate(a))) {
            for a in &arms[1..] {
                let Some(second) = rate(a) else { continue };
                if first.1 == 0 || second.1 == 0 {
                    continue;
                }
                let delta = second.0 as f64 / second.1 as f64 - first.0 as f64 / first.1 as f64;
                let separated = first.1 >= 3 && second.1 >= 3 && ((first.0 == 0 && second.0 == second.1) || (first.0 == first.1 && second.0 == 0));
                out.push_str(&format!("  {} vs {}: {:+.0} points, {}\n", a, base, delta * 100.0, if separated { "separated (every run of one arm passed, every run of the other failed)" } else { "no evidence (arms overlap or fewer than 3 seeds each)" }));
            }
        }
    }
    let total: f64 = rows.iter().map(|r| r.cost_usd).sum();
    out.push_str(&format!("{} run(s), ${:.2} spent\n", rows.len(), total));
    for s in skipped {
        out.push_str(&format!("not run (budget): {}\n", s));
    }
    out
}

// ── the command ─────────────────────────────────────────────────────────────────────────────────────────────────

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// `delphi train run ...`; `cases` are already loaded and validated by the caller.
pub fn cli(args: &[String], cases: &[&Case]) -> i32 {
    let Some(budget) = flag(args, "--budget-usd").and_then(|b| b.parse::<f64>().ok()).filter(|b| *b > 0.0) else {
        eprintln!("delphi train run: --budget-usd <dollars> is required; a run is always capped");
        return 2;
    };
    let arms_file = flag(args, "--arms-file");
    let (template, work) = (flag(args, "--template-home"), flag(args, "--work-dir"));
    // The arms file gives each arm its own home; otherwise one --template-home serves the baseline and the candidate.
    let (Some(work), true) = (work, template.is_some() || arms_file.is_some()) else {
        eprintln!("delphi train run: --work-dir <new dir> and either --template-home <dir> (a home whose model access you ratified once) or --arms-file <arms.toml> are required");
        return 2;
    };
    let (template, work) = (PathBuf::from(template.unwrap_or_default()), PathBuf::from(work));
    let mut arms = match &arms_file {
        Some(f) => match parse_arms(Path::new(f)) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("delphi train run: {}", e);
                return 2;
            }
        },
        None => vec![Arm::new("baseline", None)],
    };
    // The operator's own home is never a template or a working directory.
    let real = crate::home::delphi_home(None);
    let same = |a: &Path, b: &Path| a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(x, y)| x == y);
    let homes: Vec<&Path> = if arms_file.is_some() { arms.iter().filter_map(|a| a.template_home.as_deref()).collect() } else { vec![template.as_path()] };
    if homes.iter().any(|h| same(h, &real)) || same(&work, &real) || work.starts_with(&real) {
        eprintln!("delphi train run: {} is your real Delphi home; copy it to a new directory and use that as a template home", real.display());
        return 2;
    }
    if let Some(bad) = homes.iter().find(|h| !h.join("workspaces").is_dir()) {
        eprintln!("delphi train run: {} is not a Delphi home (no workspaces/)", bad.display());
        return 2;
    }
    if let Err(e) = train::check_work_home(&work) {
        eprintln!("delphi train run: {}", e);
        return 2;
    }
    let seeds: u32 = flag(args, "--seeds").and_then(|s| s.parse().ok()).filter(|s| (1..=20).contains(s)).unwrap_or(1);
    if let (None, Some(o)) = (&arms_file, flag(args, "--overlay")) {
        arms.push(Arm::new("candidate", Some(PathBuf::from(o))));
    }
    let worst = train::worst_case_usd(cases, seeds) * arms.len() as f64;
    println!("{} case(s) x {} seed(s) x {} arm(s): at most ${:.2} against a budget of ${:.2}", cases.len(), seeds, arms.len(), worst, budget);
    if args.iter().any(|a| a == "--dry-run") {
        println!("(dry run: nothing was started; `delphi train plan` lists the cases)");
        return 0;
    }
    if cases.is_empty() {
        eprintln!("delphi train run: no cases chosen");
        return 1;
    }
    let drafts: Vec<String> = cases.iter().filter(|c| c.truth.as_ref().is_some_and(|t| !t.verified())).map(|c| format!("{}/{}", c.suite, c.id)).collect();
    if !drafts.is_empty() && !args.iter().any(|a| a == "--allow-unverified") {
        eprintln!("delphi train run: these cases have a truth pack the operator has not verified: {}\n  verify the pack (set `verified_by` and `verified_on` in its meta.toml) or pass --allow-unverified", drafts.join(", "));
        return 2;
    }
    let delphi_bin = flag(args, "--delphi-bin").map(PathBuf::from).or_else(|| std::env::current_exe().ok()).unwrap_or_else(|| PathBuf::from("delphi"));
    if let Err(e) = train::prepare_work_home(&work) {
        eprintln!("delphi train run: {}", e);
        return 2;
    }
    let cfg = Config { template_home: template, work_dir: work.clone(), workspace: flag(args, "--workspace").unwrap_or_else(|| "train".into()), budget_usd: budget, seeds, arms: arms.clone(), delphi_bin, env: Vec::new(), kill_grace: Duration::from_secs(60), allow_unverified: args.iter().any(|a| a == "--allow-unverified") };
    let (rows, skipped) = run_all(&cfg, cases);
    let out_path = flag(args, "--out").map(PathBuf::from).unwrap_or_else(|| work.join("rows.jsonl"));
    let lines: String = rows.iter().map(|r| r.to_json().to_string() + "\n").collect();
    if let Err(e) = fs::write(&out_path, lines) {
        eprintln!("delphi train run: could not write {}: {}", out_path.display(), e);
    }
    print!("{}", report(&rows, &arms.iter().map(|a| a.name.clone()).collect::<Vec<_>>(), &skipped));
    println!("rows: {}", out_path.display());
    i32::from(rows.iter().any(|r| r.passed.is_none()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::log::{EventLog, LogOptions};
    use std::os::unix::fs::PermissionsExt;

    const BUILDER: &str = include_str!("../../../agents/builder/AGENT.md");

    /// A template home with a ratified builder manifest, as `init` lays it out.
    fn template(root: &Path) -> PathBuf {
        let home = root.join("template");
        let ws = home.join("workspaces/train");
        for (logical, text) in [("agents/builder/AGENT.md", BUILDER)] {
            for p in [ws.join("local/ratified").join(logical), ws.join("base").join(logical)] {
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(p, text).unwrap();
            }
        }
        fs::write(ws.join("local/ratified.lock"), json!({"files": {"agents/builder/AGENT.md": lock::hash(BUILDER.as_bytes())}}).to_string()).unwrap();
        fs::write(ws.join("config.toml"), "[workspace]\nprojects = [\"/somewhere/else\"]\n").unwrap();
        home
    }

    fn event(kind: &str, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m_fake", kind, payload);
        e.task_id = Some("t_0001".into());
        if kind.starts_with("agent.") {
            // `agent.*` events carry who and how (`event.rs` validate).
            (e.agent, e.span_id, e.endpoint_id, e.access_path, e.model_tier, e.model_id, e.attempt) = (Some("builder".into()), Some("sp_1".into()), Some("ep".into()), Some("api".into()), Some("standard".into()), Some("m".into()), Some(1));
        }
        e
    }

    /// A canned mission log the fake binary copies into the run's home.
    fn mission(root: &Path, kinds: &[(&str, Value)]) -> PathBuf {
        let dir = root.join("canned/m_fake");
        let mut log = EventLog::open(&dir, LogOptions::default()).unwrap();
        for (k, p) in kinds {
            log.append(event(k, p.clone())).unwrap();
        }
        dir
    }

    fn fake_bin(root: &Path) -> PathBuf {
        let p = root.join("fake-delphi.sh");
        fs::write(&p, "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in --home) H=\"$2\";; --workspace) W=\"$2\";; esac; shift; done\n\
                       mkdir -p \"$H/workspaces/$W/local/missions\"\ncp -R \"$FAKE_MISSION\" \"$H/workspaces/$W/local/missions/m_fake\"\n\
                       echo '{\"type\":\"run.summary\",\"outcome\":\"escalated\",\"exit_code\":0,\"mission_id\":\"m_fake\",\"task_id\":\"t_0001\"}'\nexit ${FAKE_EXIT:-0}\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn case(root: &Path, checks: &str) -> Case {
        let dir = root.join("suites/s/cases/c1");
        fs::create_dir_all(dir.join("fixture")).unwrap();
        fs::write(dir.join("fixture/keep.txt"), "keep\n").unwrap();
        fs::write(dir.join("case.toml"), format!("id = \"c1\"\nsuite = \"s\"\nkind = \"task\"\nrole = \"builder\"\nsplit = \"dev\"\nmax_cost_usd = 2.0\ntimeout_secs = 30\nprompt = \"do it\"\n{}", checks)).unwrap();
        train::load_case(&dir, "s").unwrap()
    }

    fn cfg(root: &Path, canned: &Path, arms: Vec<Arm>, budget: f64) -> Config {
        Config { template_home: template(root), work_dir: root.join("work"), workspace: "train".into(), budget_usd: budget, seeds: 1, arms, delphi_bin: fake_bin(root), env: vec![("FAKE_MISSION".into(), canned.display().to_string())], kill_grace: Duration::from_secs(5), allow_unverified: false }
    }

    fn baseline() -> Arm {
        Arm::new("baseline", None)
    }

    const CHECKS: &str = "[[check]]\nkind = \"outcome\"\nis = \"escalated\"\n[[check]]\nkind = \"event_count\"\nevent = \"build.attempt\"\neq = 3\n[[check]]\nkind = \"file_unchanged\"\npath = \"keep.txt\"\n";

    fn ladder_log() -> Vec<(&'static str, Value)> {
        vec![("build.attempt", json!({"attempt": 1})), ("build.attempt", json!({"attempt": 2})), ("agent.usage", json!({"provider": "anthropic", "input_tokens": 10, "output_tokens": 5, "cost_usd": 0.5, "reserved_usd": 0.5, "usage_source": "reported", "cost_model": "metered", "outcome": "ok"})), ("build.attempt", json!({"attempt": 3})), ("task.completed", json!({"status": "escalated", "reason": "gates"}))]
    }

    #[test]
    fn a_run_executes_the_binary_in_a_fresh_home_and_grades_the_log() {
        let t = tempfile::tempdir().unwrap();
        let canned = mission(t.path(), &ladder_log());
        let c = case(t.path(), CHECKS);
        let cfg = cfg(t.path(), &canned, vec![baseline()], 10.0);
        train::prepare_work_home(&cfg.work_dir).unwrap();
        let (rows, skipped) = run_all(&cfg, &[&c]);
        assert!(skipped.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].passed, rows[0].failed_checks.clone(), rows[0].cost_usd), (Some(true), vec![], 0.5), "{:?}", rows[0]);
        // The template was copied, not used: its config still points elsewhere; the run's home points at its own repo.
        assert!(fs::read_to_string(cfg.template_home.join("workspaces/train/config.toml")).unwrap().contains("/somewhere/else"));
        let run_cfg = fs::read_to_string(cfg.work_dir.join("001-baseline-c1-s1").join("home").join("workspaces/train/config.toml")).unwrap();
        assert!(run_cfg.contains("001-baseline-c1-s1/repo"), "{}", run_cfg);
        assert!(cfg.work_dir.join("001-baseline-c1-s1/repo/.git").is_dir() && cfg.work_dir.join("001-baseline-c1-s1/repo/keep.txt").is_file());
    }

    /// A research case with a truth pack beside it (verified or a draft).
    fn research_case(root: &Path, verified: bool, checks: &str) -> Case {
        let dir = root.join("suites/r/cases/q1");
        fs::create_dir_all(&dir).unwrap();
        crate::train_truth::tests::pack(&dir);
        if verified {
            fs::write(dir.join("truth/meta.toml"), "as_of = \"2026-10-01\"\nverified_by = \"operator\"\nverified_on = \"2026-10-26\"\n").unwrap();
        }
        fs::write(dir.join("case.toml"), format!("id = \"q1\"\nsuite = \"r\"\nkind = \"research\"\nrole = \"strategist\"\nsplit = \"dev\"\nmax_cost_usd = 2.0\ntimeout_secs = 30\nprompt = \"What changes in 2027?\"\n{}", checks)).unwrap();
        train::load_case(&dir, "r").unwrap()
    }

    const RESEARCH_CHECKS: &str = "[[check]]\nkind = \"fact_recall\"\nmin = 0.6\n[[check]]\nkind = \"no_traps\"\n[[check]]\nkind = \"text_matches\"\npattern = \"cannot find|no such\"\npresent = false\n";

    #[test]
    fn a_research_case_is_graded_on_what_the_task_delivered_against_its_truth_pack() {
        let t = tempfile::tempdir().unwrap();
        let good = "From 2027 the bikes use 850cc engines and the calendar has 20 rounds.";
        let canned = mission(t.path(), &[("task.output", json!({"status": "done", "text": good})), ("task.completed", json!({"status": "done"}))]);
        let c = research_case(t.path(), true, RESEARCH_CHECKS);
        let config = cfg(t.path(), &canned, vec![baseline()], 10.0);
        train::prepare_work_home(&config.work_dir).unwrap();
        let (rows, _) = run_all(&config, &[&c]);
        assert_eq!((rows[0].passed, rows[0].failed_checks.clone()), (Some(true), vec![]), "{:?}", rows[0]);
        assert!(rows[0].notes[0].contains("fact recall 100%"), "{:?}", rows[0].notes);
        // The task is addressed by role and works in an empty repository (a research case has no fixture).
        let repo = config.work_dir.join("001-baseline-q1-s1").join("repo");
        assert!(repo.join(".git").is_dir() && fs::read_dir(&repo).unwrap().flatten().all(|e| e.file_name() == ".git"));
    }

    #[test]
    fn a_research_answer_that_misses_facts_or_falls_into_a_trap_fails_in_words() {
        let t = tempfile::tempdir().unwrap();
        let bad = "The new rules ban V4 engines. I cannot find more.";
        let canned = mission(t.path(), &[("task.output", json!({"status": "done", "text": bad})), ("task.completed", json!({"status": "done"}))]);
        let c = research_case(t.path(), true, RESEARCH_CHECKS);
        let config = cfg(t.path(), &canned, vec![baseline()], 10.0);
        train::prepare_work_home(&config.work_dir).unwrap();
        let (rows, _) = run_all(&config, &[&c]);
        let f = rows[0].failed_checks.join(" | ");
        for want in ["fact recall 0%", "missing: engine, rounds", "fell into trap(s): v4-ban", "`cannot find|no such` appears"] {
            assert!(f.contains(want), "{} in {}", want, f);
        }
        assert!(rows[0].notes.iter().any(|n| n.contains("traps: v4-ban")));
    }

    #[test]
    fn markdown_the_mission_wrote_counts_as_part_of_the_deliverable() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("m");
        fs::create_dir_all(dir.join("research")).unwrap();
        fs::write(dir.join("research/report.md"), "850cc engines").unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let events = vec![event("task.output", json!({"status": "done", "text": "short answer"}))];
        let d = deliverable_of(&events, Some("t_0001"), &dir);
        assert!(d.starts_with("short answer") && d.contains("850cc engines") && !d.contains("ignored"), "{}", d);
    }

    #[test]
    fn a_draft_truth_pack_is_not_run_without_the_flag() {
        let t = tempfile::tempdir().unwrap();
        let canned = mission(t.path(), &[("task.completed", json!({"status": "done"}))]);
        let c = research_case(t.path(), false, RESEARCH_CHECKS);
        let mut config = cfg(t.path(), &canned, vec![baseline()], 10.0);
        train::prepare_work_home(&config.work_dir).unwrap();
        let (rows, _) = run_all(&config, &[&c]);
        assert_eq!(rows[0].passed, None);
        assert!(rows[0].error.as_deref().unwrap_or("").contains("draft"), "{:?}", rows[0].error);
        config.allow_unverified = true;
        let (rows, _) = run_all(&config, &[&c]);
        assert!(rows[0].error.is_none() && rows[0].passed.is_some(), "{:?}", rows[0]);
        // The command refuses up front, before any directory is made.
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (tpl, work) = (template(t.path()), t.path().join("fresh"));
        let base = ["run", "--budget-usd", "9", "--template-home", tpl.to_str().unwrap(), "--work-dir", work.to_str().unwrap()];
        assert_eq!(cli(&a(&base), &[&c]), 2);
        assert!(!work.exists());
    }

    /// A canned mission whose usage is spread over models: `(model, input, cache_read, output, thinking, cost)`.
    fn usage_mission(root: &Path, usage: &[(&str, u64, u64, u64, u64, f64)]) -> PathBuf {
        let dir = root.join("canned/m_fake");
        let mut log = EventLog::open(&dir, LogOptions::default()).unwrap();
        for (model, input, cache, output, thinking, cost) in usage {
            let mut e = event("agent.usage", json!({"provider": "p", "input_tokens": input, "cache_read_tokens": cache, "output_tokens": output, "thinking_tokens": thinking, "cost_usd": cost, "reserved_usd": cost, "usage_source": "reported", "cost_model": "metered", "outcome": "ok"}));
            e.model_id = Some((*model).into());
            log.append(e).unwrap();
        }
        for (k, p) in [("build.attempt", json!({"attempt": 1})), ("build.attempt", json!({"attempt": 2})), ("model.promoted", json!({})), ("task.completed", json!({"status": "escalated"}))] {
            log.append(event(k, p)).unwrap();
        }
        dir
    }

    #[test]
    fn a_row_records_tokens_by_model_attempts_and_promotions() {
        let t = tempfile::tempdir().unwrap();
        let canned = usage_mission(t.path(), &[("haiku-x", 1000, 4000, 200, 0, 0.01), ("haiku-x", 500, 0, 100, 50, 0.01), ("fable-x", 2000, 0, 800, 600, 0.9)]);
        let c = case(t.path(), CHECKS);
        let config = cfg(t.path(), &canned, vec![baseline()], 10.0);
        train::prepare_work_home(&config.work_dir).unwrap();
        let (rows, _) = run_all(&config, &[&c]);
        let r = &rows[0];
        assert_eq!((r.attempts, r.promotions), (2, 1));
        let h = &r.usage["haiku-x"];
        assert_eq!((h.calls, h.tokens_in(), h.tokens_out()), (2, 5500, 350));
        assert_eq!((r.usage["fable-x"].tokens_in(), r.usage["fable-x"].tokens_out()), (2000, 1400));
        assert!((r.cost_usd - 0.92).abs() < 1e-9, "{}", r.cost_usd);
        let j = r.to_json();
        assert_eq!((j["tokens_in"].as_u64(), j["tokens_out"].as_u64()), (Some(7500), Some(1750)));
        assert_eq!(j["models"]["fable-x"]["calls"], 1);
    }

    #[test]
    fn a_run_staffed_by_a_model_the_arm_did_not_declare_is_not_graded() {
        let t = tempfile::tempdir().unwrap();
        let canned = usage_mission(t.path(), &[("haiku-x", 100, 0, 50, 0, 0.01), ("sonnet-y", 100, 0, 50, 0, 0.02)]);
        let c = case(t.path(), CHECKS);
        let mut arm = baseline();
        arm.expect_models = vec!["haiku".into()];
        let config = cfg(t.path(), &canned, vec![arm.clone()], 10.0);
        train::prepare_work_home(&config.work_dir).unwrap();
        let (rows, _) = run_all(&config, &[&c]);
        assert_eq!(rows[0].passed, None);
        let e = rows[0].error.as_deref().unwrap_or("");
        assert!(e.contains("arm staffing violated") && e.contains("sonnet-y") && !e.contains("used haiku"), "{}", e);
        assert!((rows[0].cost_usd - 0.03).abs() < 1e-9, "what was spent is still recorded");
        // Declaring both prefixes, or none, makes the same run gradable.
        for expect in [vec!["haiku".to_string(), "sonnet".to_string()], vec![]] {
            let mut ok = arm.clone();
            ok.expect_models = expect;
            let config = cfg(t.path(), &canned, vec![ok], 10.0);
            let _ = fs::remove_dir_all(&config.work_dir);
            train::prepare_work_home(&config.work_dir).unwrap();
            assert!(run_all(&config, &[&c]).0[0].passed.is_some());
        }
    }

    #[test]
    fn each_arm_runs_in_a_copy_of_its_own_template_home() {
        let t = tempfile::tempdir().unwrap();
        let canned = mission(t.path(), &ladder_log());
        let c = case(t.path(), CHECKS);
        let mut config = cfg(t.path(), &canned, vec![], 10.0);
        let other = t.path().join("other-template");
        copy_tree(&config.template_home, &other).unwrap();
        fs::write(other.join("MARK"), "second").unwrap();
        let mut second = Arm::new("second", None);
        second.template_home = Some(other);
        config.arms = vec![baseline(), second];
        train::prepare_work_home(&config.work_dir).unwrap();
        let (rows, _) = run_all(&config, &[&c]);
        assert_eq!(rows.len(), 2);
        assert!(!config.work_dir.join("001-baseline-c1-s1/home/MARK").exists());
        assert_eq!(fs::read_to_string(config.work_dir.join("002-second-c1-s1/home/MARK")).unwrap(), "second");
    }

    #[test]
    fn an_arms_file_names_each_arm_its_home_and_staffing() {
        let t = tempfile::tempdir().unwrap();
        let home = template(t.path());
        let f = t.path().join("arms.toml");
        let body = format!("[[arm]]\nname = \"solo-haiku\"\ntemplate_home = \"{}\"\nexpect_models = [\"claude-haiku\"]\n\n[[arm]]\nname = \"cheap-first\"\ntemplate_home = \"template\"\noverlay = \"cand\"\n", home.display());
        fs::write(&f, &body).unwrap();
        let arms = parse_arms(&f).unwrap();
        assert_eq!((arms.len(), arms[0].expect_models.clone(), arms[0].overlay.clone()), (2, vec!["claude-haiku".to_string()], None));
        assert_eq!((arms[1].template_home.clone(), arms[1].overlay.clone()), (Some(t.path().join("template")), Some(t.path().join("cand"))), "relative paths are from the file");
        for (bad, want) in [
            (body.replace("solo-haiku", "Solo Haiku"), "`name` must be lowercase"),
            (body.replace("cheap-first", "solo-haiku"), "two arms are named"),
            (body.replace(&home.display().to_string(), "/nowhere"), "is not a Delphi home"),
            ("[[arm]]\nname = \"x\"\n".to_string(), "needs `template_home`"),
            ("# none\n".to_string(), "no [[arm]] tables"),
        ] {
            fs::write(&f, bad).unwrap();
            assert!(parse_arms(&f).unwrap_err().contains(want), "{}", want);
        }
    }

    #[test]
    fn the_command_takes_arms_from_a_file_and_refuses_a_real_home_in_it() {
        let t = tempfile::tempdir().unwrap();
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let home = template(t.path());
        let f = t.path().join("arms.toml");
        fs::write(&f, format!("[[arm]]\nname = \"one\"\ntemplate_home = \"{}\"\n", home.display())).unwrap();
        let work = t.path().join("fresh");
        let base = ["run", "--budget-usd", "9", "--work-dir", work.to_str().unwrap(), "--arms-file", f.to_str().unwrap(), "--dry-run"];
        let c = case(t.path(), CHECKS);
        assert_eq!(cli(&a(&base), &[&c]), 0, "no --template-home is needed when every arm has its own");
        assert_eq!(cli(&a(&["run", "--budget-usd", "9", "--work-dir", work.to_str().unwrap(), "--dry-run"]), &[&c]), 2, "neither a template nor an arms file");
        assert!(!work.exists());
    }

    #[test]
    fn each_check_kind_fails_in_words() {
        let t = tempfile::tempdir().unwrap();
        let canned = mission(t.path(), &[("build.attempt", json!({"attempt": 1})), ("task.completed", json!({"status": "done"}))]);
        let c = case(t.path(), &format!("{}[[check]]\nkind = \"event_absent\"\nevent = \"build.attempt\"\n[[check]]\nkind = \"file_exists\"\npath = \"new.rs\"\n[[check]]\nkind = \"file_contains\"\npath = \"keep.txt\"\ntext = \"zzz\"\n", CHECKS));
        let cfg = cfg(t.path(), &canned, vec![baseline()], 10.0);
        train::prepare_work_home(&cfg.work_dir).unwrap();
        let (rows, _) = run_all(&cfg, &[&c]);
        assert_eq!(rows[0].passed, Some(false));
        let f = rows[0].failed_checks.join(" | ");
        for want in ["outcome is done, wanted escalated", "1 `build.attempt` event(s); wanted exactly 3", "1 `build.attempt` event(s); wanted none", "new.rs does not exist", "keep.txt does not contain"] {
            assert!(f.contains(want), "missing `{}` in {}", want, f);
        }
        assert!(!f.contains("keep.txt changed"), "an untouched file passes file_unchanged: {}", f);
    }

    #[test]
    fn the_budget_stops_the_run_before_a_case_that_would_exceed_it() {
        let t = tempfile::tempdir().unwrap();
        let canned = mission(t.path(), &ladder_log());
        let c = case(t.path(), CHECKS);
        // Cap 2.0 each, each run really costs 0.5, budget 2.9: the first fits, the second needs 0.5 + 2.0 <= 2.9, the third needs 3.0.
        let mut config = cfg(t.path(), &canned, vec![baseline()], 2.9);
        config.seeds = 3;
        train::prepare_work_home(&config.work_dir).unwrap();
        let (rows, skipped) = run_all(&config, &[&c]);
        assert_eq!((rows.len(), skipped.len()), (2, 1), "{:?}", skipped);
        assert!(skipped[0].contains("seed 3"));
    }

    #[test]
    fn infrastructure_trouble_is_not_a_failed_case() {
        let t = tempfile::tempdir().unwrap();
        let canned = mission(t.path(), &ladder_log());
        let c = case(t.path(), CHECKS);
        let mut config = cfg(t.path(), &canned, vec![baseline()], 10.0);
        config.env.push(("FAKE_EXIT".into(), "4".into()));
        train::prepare_work_home(&config.work_dir).unwrap();
        let (rows, _) = run_all(&config, &[&c]);
        assert_eq!((rows[0].passed, rows[0].exit_code), (None, Some(4)));
        assert!(rows[0].error.as_deref().unwrap_or("").contains("infrastructure"), "{:?}", rows[0].error);
        assert!(report(&rows, &["baseline".into()], &[]).contains("0/0 passed") && report(&rows, &["baseline".into()], &[]).contains("not graded"));
    }

    #[test]
    fn a_process_that_outlives_its_time_is_killed() {
        let t = tempfile::tempdir().unwrap();
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let started = Instant::now();
        let (code, timed_out) = run_process(&mut cmd, &t.path().join("o"), &t.path().join("e"), Duration::from_millis(300)).unwrap();
        assert!(timed_out && code.is_none() && started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn the_overlay_loads_in_the_temp_home_and_leaves_every_other_home_alone() {
        let t = tempfile::tempdir().unwrap();
        let canned = mission(t.path(), &ladder_log());
        let c = case(t.path(), CHECKS);
        let overlay = t.path().join("overlay");
        fs::create_dir_all(overlay.join("agents/builder")).unwrap();
        let changed = format!("{}\nCandidate note: stop after two failed attempts.\n", BUILDER);
        fs::write(overlay.join("agents/builder/AGENT.md"), &changed).unwrap();
        let arms = vec![baseline(), Arm::new("candidate", Some(overlay))];
        let config = cfg(t.path(), &canned, arms, 10.0);
        train::prepare_work_home(&config.work_dir).unwrap();
        let (rows, _) = run_all(&config, &[&c]);
        assert_eq!(rows.len(), 2);
        let load = |run: &str| delphi_engine::workspace::WorkspacePaths::new(&config.work_dir.join(run).join("home"), "train").load_manifest("builder");
        // The overlay is loaded by the daemon's own loader, hash check included.
        assert!(load("002-candidate-c1-s1").is_ok(), "{:?}", load("002-candidate-c1-s1").err());
        assert!(fs::read_to_string(config.work_dir.join("002-candidate-c1-s1").join("home").join("workspaces/train/local/ratified/agents/builder/AGENT.md")).unwrap().contains("Candidate note"));
        // The baseline arm and the template keep the original text.
        for p in [config.work_dir.join("001-baseline-c1-s1/home"), config.template_home.clone()] {
            let text = fs::read_to_string(p.join("workspaces/train/local/ratified/agents/builder/AGENT.md")).unwrap();
            assert!(!text.contains("Candidate note"), "{}", p.display());
        }
        assert!(delphi_engine::workspace::WorkspacePaths::new(&config.template_home, "train").load_manifest("builder").is_ok());
    }

    #[test]
    fn an_overlay_may_only_hold_protected_ratified_files() {
        let t = tempfile::tempdir().unwrap();
        let ws = template(t.path()).join("workspaces/train");
        for bad in ["config.toml", "local/ratified.lock", "src/main.rs"] {
            let o = t.path().join("o");
            let _ = fs::remove_dir_all(&o);
            fs::create_dir_all(o.join(Path::new(bad).parent().unwrap())).unwrap();
            fs::write(o.join(bad), "x").unwrap();
            assert!(apply_overlay(&ws, &o).unwrap_err().contains("not a ratified, protected file"), "{}", bad);
        }
        let empty = t.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(apply_overlay(&ws, &empty).is_err());
    }

    #[test]
    fn the_report_labels_deltas_as_no_evidence_unless_the_arms_separate() {
        let row = |arm: &str, seed: u32, pass: bool| Row { case: "s/c1".into(), arm: arm.into(), seed, exit_code: Some(0), wall_secs: 1.0, cost_usd: 0.25, passed: Some(pass), failed_checks: vec![], notes: vec![], usage: BTreeMap::new(), attempts: 0, promotions: 0, error: None };
        let arms = vec!["baseline".to_string(), "candidate".to_string()];
        let two: Vec<Row> = vec![row("baseline", 1, false), row("baseline", 2, false), row("candidate", 1, true), row("candidate", 2, true)];
        let r = report(&two, &arms, &[]);
        assert!(r.contains("baseline     0/2 passed") && r.contains("+100 points, no evidence"), "{}", r);
        let three: Vec<Row> = (1..=3).flat_map(|s| [row("baseline", s, false), row("candidate", s, true)]).collect();
        assert!(report(&three, &arms, &[]).contains("separated"));
        let mixed: Vec<Row> = (1..=3).flat_map(|s| [row("baseline", s, s == 1), row("candidate", s, s != 2)]).collect();
        assert!(report(&mixed, &arms, &["s/c1 [baseline] seed 4".into()]).contains("no evidence") && report(&mixed, &arms, &["s/c1 [baseline] seed 4".into()]).contains("not run (budget): s/c1 [baseline] seed 4"));
    }

    #[test]
    fn set_projects_edits_only_the_projects_line() {
        let cfg = "[workspace]\nname = \"x\"\nprojects = [\"/a\", \"/b\"]\n[access]\nk = 1\n";
        let out = set_projects(cfg, Path::new("/tmp/r\"q")).unwrap();
        assert_eq!(out, "[workspace]\nname = \"x\"\nprojects = [\"/tmp/r\\\"q\"]\n[access]\nk = 1\n");
        assert!(set_projects("x = 1\n", Path::new("/r")).is_err());
        assert!(set_projects("projects = []\nprojects = []\n", Path::new("/r")).is_err());
    }

    #[test]
    fn the_command_refuses_without_a_budget_a_template_or_a_fresh_work_dir() {
        let t = tempfile::tempdir().unwrap();
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cli(&a(&["run"]), &[]), 2);
        assert_eq!(cli(&a(&["run", "--budget-usd", "5"]), &[]), 2);
        let tpl = template(t.path());
        let busy = t.path().join("busy");
        fs::create_dir_all(&busy).unwrap();
        fs::write(busy.join("precious"), "x").unwrap();
        assert_eq!(cli(&a(&["run", "--budget-usd", "5", "--template-home", tpl.to_str().unwrap(), "--work-dir", busy.to_str().unwrap()]), &[]), 2);
        let fresh = t.path().join("fresh");
        assert_eq!(cli(&a(&["run", "--budget-usd", "5", "--template-home", tpl.to_str().unwrap(), "--work-dir", fresh.to_str().unwrap(), "--dry-run"]), &[]), 0);
        assert!(!fresh.exists(), "a dry run creates nothing");
    }
}
