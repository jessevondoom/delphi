//! `delphi run`: one task from a script or CI, with no terminal (gap-closure plan §4).
//!
//! It is a client of the same daemon as the console, driven by the same input grammar (`/build …`,
//! `@architect …`, `/council …`, or a plain prompt). Three things differ from the console:
//!
//! * **It starts its own daemon.** The daemon hands a console token only to a process with a controlling
//!   terminal or to the one that spawned it, so a script cannot attach to someone else's daemon. A run that
//!   finds one already running without a terminal says so and stops; it never goes around that rule.
//! * **Cards are answered by a written policy, never by guessing.** `--on-card deny` (the default) denies every
//!   card. `--on-card policy:<file>` allows the Amber cards the file lists. `--on-card relay` answers nothing:
//!   the cards wait for a person, through a relay (`delphi relay`) or a console, and the run waits with them.
//!   A Red card is never allowed by the run, whatever the file says, and a merge card is always denied by the
//!   run, so a headless build ends at "ready to merge" on its branch.
//! * **The result is a summary object and an exit code**, so a caller can act on it.
//!
//! Exit codes: 0 completed or ready to merge · 1 gates failed or the task escalated · 2 stopped because a card
//! was denied · 3 budget exceeded · 4 infrastructure (no daemon, lost connection, timeout, bad arguments) ·
//! 5 an optimistic build's provisional decisions await the operator's review (the branch and ledger are kept).

use crate::chat::{self, Input};
use delphi_core::client::{Client, ClientError};
use delphi_core::spawn::{connect_or_spawn, SpawnOptions};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const EXIT_OK: i32 = 0;
pub const EXIT_GATES: i32 = 1;
pub const EXIT_DENIED: i32 = 2;
pub const EXIT_BUDGET: i32 = 3;
pub const EXIT_INFRA: i32 = 4;
/// An optimistic build finished and its provisional decisions wait for the operator's review (`specs/015`).
pub const EXIT_REVIEW: i32 = 5;

/// Tools whose card is the merge decision itself. A headless run stops here and reports the branch.
const MERGE_TOOLS: [&str; 2] = ["merge", "pull_request"];

// ── arguments ───────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum OnCard {
    Deny,
    Policy(Policy),
    /// Answer nothing: leave every card (except the merge card) for the operator.
    Leave,
}

#[derive(Debug, PartialEq)]
pub struct Opts {
    pub input: String,
    pub permissions: Option<String>,
    pub on_card: OnCard,
    pub budget: Option<f64>,
    pub timeout: Option<Duration>,
    pub mission: Option<String>,
    pub jsonl: bool,
    pub json: bool,
    pub keep_daemon: bool,
}

const VALUE_FLAGS: [&str; 8] = ["--workspace", "--home", "--mission", "--permissions", "--on-card", "--budget", "--timeout", "--task"];

fn value_of(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// `30`, `30s`, `5m`, `2h`, `1d`.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1),
        Some('m') => (&s[..s.len() - 1], 60),
        Some('h') => (&s[..s.len() - 1], 3600),
        Some('d') => (&s[..s.len() - 1], 86_400),
        _ => (s, 1),
    };
    num.parse::<u64>().ok().filter(|n| *n > 0).map(|n| Duration::from_secs(n * mult)).ok_or_else(|| format!("`{}` is not a duration (use 90, 90s, 5m, 2h or 1d)", s))
}

pub fn parse_args(args: &[String]) -> Result<Opts, String> {
    // The task text: `--task <text>` or every argument that is neither a flag nor a flag's value.
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if VALUE_FLAGS.contains(&a) {
            i += 2;
        } else if a.starts_with("--") {
            i += 1;
        } else {
            words.push(a);
            i += 1;
        }
    }
    let input = match value_of(args, "--task") {
        Some(t) => t,
        None => words.join(" "),
    };
    if input.trim().is_empty() {
        return Err("give the task: delphi run \"/build implement @specs/011-x.md in @project/app\"  (or --task <text>)".into());
    }
    let on_card = match value_of(args, "--on-card").as_deref() {
        None | Some("deny") => OnCard::Deny,
        Some(p) if p.starts_with("policy:") => OnCard::Policy(Policy::load(Path::new(&p["policy:".len()..]))?),
        Some("relay") | Some("park") => OnCard::Leave,
        Some(other) => return Err(format!("--on-card takes deny, relay or policy:<file>, not `{}`", other)),
    };
    let permissions = value_of(args, "--permissions");
    if let Some(p) = &permissions {
        if !["default", "accept-edits", "mission-auto", "optimistic"].contains(&p.as_str()) {
            return Err(format!("--permissions takes default, accept-edits, mission-auto or optimistic, not `{}`", p));
        }
    }
    let budget = match value_of(args, "--budget") {
        None => None,
        Some(b) => Some(b.trim_start_matches('$').parse::<f64>().ok().filter(|x| *x > 0.0).ok_or_else(|| format!("--budget takes a dollar amount, not `{}`", b))?),
    };
    let timeout = value_of(args, "--timeout").map(|t| parse_duration(&t)).transpose()?;
    let jsonl = args.iter().any(|a| a == "--jsonl");
    let json = args.iter().any(|a| a == "--json");
    if jsonl && json {
        return Err("--json and --jsonl are alternatives: --jsonl streams events then the summary, --json prints only the summary".into());
    }
    Ok(Opts { input, permissions, on_card, budget, timeout, mission: value_of(args, "--mission"), jsonl, json, keep_daemon: args.iter().any(|a| a == "--keep-daemon") })
}

// ── the card policy ─────────────────────────────────────────────────────────

/// What a `policy:<file>` allows. Amber only: a file that names a Red tier or a merge tool does not load.
///
/// ```toml
/// [[allow]]
/// tool = "write_file"
/// pattern = "write_file @worktree/*"   # `*` matches any run of characters; omit to match any pattern
/// ```
#[derive(Debug, PartialEq, Default)]
pub struct Policy {
    pub allow: Vec<Rule>,
}

#[derive(Debug, PartialEq)]
pub struct Rule {
    pub tool: String,
    pub pattern: Option<String>,
}

impl Policy {
    pub fn load(path: &Path) -> Result<Policy, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read the policy {}: {}", path.display(), e))?;
        Policy::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Policy, String> {
        let doc: toml::Value = toml::from_str(text).map_err(|e| format!("the policy is not valid TOML: {}", e))?;
        let mut allow = Vec::new();
        for (i, r) in doc.get("allow").and_then(|a| a.as_array()).into_iter().flatten().enumerate() {
            let n = i + 1;
            let tool = r.get("tool").and_then(|t| t.as_str()).filter(|t| !t.is_empty()).ok_or_else(|| format!("allow rule {} needs a `tool`", n))?;
            if MERGE_TOOLS.contains(&tool) {
                return Err(format!("allow rule {} names `{}`: merging is the operator's decision and no policy can allow it", n, tool));
            }
            if let Some(t) = r.get("tier").and_then(|t| t.as_str()) {
                if t != "amber" {
                    return Err(format!("allow rule {} has tier `{}`: a policy can allow Amber cards only", n, t));
                }
            }
            allow.push(Rule { tool: tool.to_string(), pattern: r.get("pattern").and_then(|p| p.as_str()).map(String::from) });
        }
        if let Some(k) = doc.as_table().and_then(|t| t.keys().find(|k| *k != "allow")) {
            return Err(format!("unknown policy key `{}` (only [[allow]] rules are understood)", k));
        }
        Ok(Policy { allow })
    }

    fn matches(&self, tool: &str, pattern: &str) -> bool {
        self.allow.iter().any(|r| r.tool == tool && r.pattern.as_deref().map(|p| glob(p, pattern)).unwrap_or(true))
    }
}

/// `*` matches any run of characters (including none); everything else matches itself.
pub fn glob(pat: &str, text: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            let Some(r) = rest.strip_prefix(part) else { return false };
            rest = r;
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            let Some(at) = rest.find(part) else { return false };
            rest = &rest[at + part.len()..];
        }
    }
    true
}

/// The answer to a card: `Some((decision, reason))`, or `None` to leave it for the operator (`--on-card relay`).
/// Red is never allowed, whatever the policy says, and the merge card is always denied so the run can end.
pub fn decide(on_card: &OnCard, card: &Value) -> Option<(&'static str, String)> {
    let (tier, tool, pattern) = (card["tier"].as_str().unwrap_or("amber"), card["tool"].as_str().unwrap_or(""), card["pattern"].as_str().unwrap_or(""));
    if MERGE_TOOLS.contains(&tool) {
        return Some(("deny", "headless run: stopped before the merge, which is the operator's decision".into()));
    }
    match on_card {
        OnCard::Leave => None,
        _ if tier == "red" => Some(("deny", "headless run: a Red action is never approved without the operator".into())),
        OnCard::Policy(p) if p.matches(tool, pattern) => Some(("allow", "allowed by the run's policy".into())),
        OnCard::Policy(_) => Some(("deny", "headless run: no policy rule allows this".into())),
        OnCard::Deny => Some(("deny", "headless run: --on-card deny".into())),
    }
}

// ── the summary ─────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct Summary {
    pub task_id: Option<String>,
    pub mission_id: String,
    pub cards: Vec<Value>,
    pub denied_non_merge: bool,
    pub ready: Option<Value>,
    pub gates: Value,
    pub merge_commit: Option<String>,
    pub pr_url: Option<String>,
    pub status: Option<String>,
    /// Set when an optimistic build is waiting for the operator to review its provisional decisions.
    pub review_pending: Option<Value>,
    pub reason: String,
    pub text: String,
    pub failed: Option<String>,
}

impl Summary {
    /// Fold one event of this run's task into the summary.
    pub fn apply(&mut self, e: &Value) {
        let (kind, p) = (e["type"].as_str().unwrap_or(""), &e["payload"]);
        match kind {
            "gates.evaluated" => self.gates = p["gates"].clone(),
            "action.result" if p["ok"] == true => {
                if let Some(c) = p["detail"]["merge_commit"].as_str() {
                    self.merge_commit = Some(c.to_string());
                }
                if let Some(u) = p["detail"]["url"].as_str() {
                    self.pr_url = Some(u.to_string());
                }
            }
            "approval.resolved" => {
                // Whoever answered a card the run left alone: record how it ended.
                let id = p["approval_id"].as_str().unwrap_or("");
                if let Some(c) = self.cards.iter_mut().find(|c| c["approval_id"] == id && c["decision"] == "left") {
                    let how = match p["resolution"].as_str().unwrap_or("") {
                        "approved" => "allow",
                        _ => "deny",
                    };
                    c["decision"] = json!(how);
                    c["reason"] = json!(format!("{} by {}", p["resolution"].as_str().unwrap_or("resolved"), p["by"].as_str().unwrap_or("someone")));
                    if how == "deny" && !MERGE_TOOLS.contains(&c["tool"].as_str().unwrap_or("")) {
                        self.denied_non_merge = true;
                    }
                }
            }
            "decisions.review_requested" => self.review_pending = Some(p.clone()),
            "task.output" => self.text = p["text"].as_str().unwrap_or("").to_string(),
            "task.completed" => {
                self.status = p["status"].as_str().map(String::from);
                self.reason = p["reason"].as_str().unwrap_or("").to_string();
            }
            "task.failed" => self.failed = Some(p["error"].as_str().unwrap_or("the task failed").to_string()),
            _ => {}
        }
    }

    /// Record a card and what was done with it. A merge card also captures what is ready to merge.
    pub fn card(&mut self, card: &Value, decision: &str, reason: &str) {
        if decision == "left" {
            self.cards.push(json!({"approval_id": card["approval_id"], "tool": card["tool"], "tier": card["tier"], "summary": card["summary"], "decision": "left", "reason": reason}));
            return;
        }
        let tool = card["tool"].as_str().unwrap_or("");
        if MERGE_TOOLS.contains(&tool) {
            let pl = &card["payload"];
            self.ready = Some(json!({"sha": pl["sha"], "branch": pl["branch"], "files": pl["files"], "host_execution": pl["host_execution"]}));
            if self.gates.is_null() {
                self.gates = pl["gates"].clone();
            }
        } else if decision == "deny" {
            self.denied_non_merge = true;
        }
        self.cards.push(json!({"approval_id": card["approval_id"], "tool": tool, "tier": card["tier"], "summary": card["summary"], "decision": decision, "reason": reason}));
    }

    /// `(outcome, exit code)`. The order matters: a task that failed outright is infrastructure whatever else
    /// was seen, and a budget stop is reported as such even if a card was also denied.
    pub fn outcome(&self) -> (&'static str, i32) {
        if self.failed.is_some() {
            return ("failed", EXIT_INFRA);
        }
        if self.review_pending.is_some() && self.merge_commit.is_none() {
            return ("awaiting_decision_review", EXIT_REVIEW);
        }
        if self.merge_commit.is_some() {
            return ("merged", EXIT_OK);
        }
        if self.pr_url.is_some() {
            return ("pr_opened", EXIT_OK);
        }
        match self.status.as_deref() {
            Some("done") if self.ready.is_some() => ("ready_to_merge", EXIT_OK),
            Some("done") => ("completed", EXIT_OK),
            Some(_) if self.reason.contains("mission budget") => ("budget_exceeded", EXIT_BUDGET),
            Some(_) if self.denied_non_merge => ("denied", EXIT_DENIED),
            Some(_) => ("escalated", EXIT_GATES),
            None => ("incomplete", EXIT_INFRA),
        }
    }

    pub fn to_json(&self, outcome: &str, code: i32, wall: Duration, usage: Value) -> Value {
        let allowed = self.cards.iter().filter(|c| c["decision"] == "allow").count();
        let denied = self.cards.iter().filter(|c| c["decision"] == "deny").count();
        json!({
            "type": "run.summary", "outcome": outcome, "exit_code": code,
            "mission_id": self.mission_id, "task_id": self.task_id,
            "ready_to_merge": self.ready, "merge_commit": self.merge_commit, "pr_url": self.pr_url, "decisions_awaiting_review": self.review_pending,
            "gates": self.gates, "reason": self.reason, "error": self.failed,
            "text": self.text.chars().take(4000).collect::<String>(),
            "cards": {"total": self.cards.len(), "allowed": allowed, "denied": denied, "left": self.cards.len() - allowed - denied, "list": self.cards},
            "usage": usage, "wall_secs": wall.as_secs_f64(),
        })
    }
}

// ── driving the daemon ──────────────────────────────────────────────────────

fn rpc(e: &ClientError) -> String {
    crate::chat::rpc_error(e)
}

fn say_err(m: &str) {
    eprintln!("delphi run: {}", m);
}

/// Run one task. Returns the process exit code.
pub fn run(home: &Path, ws_dir: &Path, opts: Opts) -> i32 {
    let started = Instant::now();
    let Some(exe) = chat::daemon_exe() else {
        say_err("cannot locate this program to start the daemon");
        return EXIT_INFRA;
    };
    let mut attached = match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()], own_version: Some(env!("CARGO_PKG_VERSION")) }) {
        Ok(a) => a,
        Err(ClientError::Rpc { code, message, .. }) if code == delphi_core::proto::code::FORBIDDEN => {
            say_err(&format!("a daemon is already running for this workspace and this process has no terminal, so it cannot be attached to ({}). Stop it with `delphi daemon stop`, or run in another workspace or --home.", message));
            return EXIT_INFRA;
        }
        Err(e) => {
            say_err(&rpc(&e));
            return EXIT_INFRA;
        }
    };
    let spawned = attached.spawned;
    let code = drive(&mut attached, &opts, started);
    if spawned && !opts.keep_daemon {
        let _ = attached.client.call("daemon.shutdown", json!({}));
    }
    code
}

fn drive(attached: &mut delphi_core::spawn::Attached, opts: &Opts, started: Instant) -> i32 {
    let cmd = &mut attached.client;
    let opened = match cmd.call("mission.open", json!({"mission_id": opts.mission, "cwd": std::env::current_dir().ok().map(|p| p.display().to_string())})) {
        Ok(v) => v,
        Err(e) => {
            say_err(&rpc(&e));
            return EXIT_INFRA;
        }
    };
    let mission_id = opened["mission_id"].as_str().unwrap_or("").to_string();
    let mut ev = match Client::connect(&attached.sock).map_err(ClientError::from).and_then(|mut c| c.hello(&attached.token).map(|_| c)) {
        Ok(c) => c,
        Err(e) => {
            say_err(&rpc(&e));
            return EXIT_INFRA;
        }
    };
    if let Err(e) = ev.call("events.subscribe", json!({})) {
        say_err(&rpc(&e));
        return EXIT_INFRA;
    }
    // Events come through a thread so the main loop can time out without losing a half-read line.
    let (tx, rx) = mpsc::channel::<Option<Value>>();
    std::thread::spawn(move || loop {
        match ev.next_notification() {
            Ok(n) if n["method"] == "event" => {
                if tx.send(Some(n["params"].clone())).is_err() {
                    return;
                }
            }
            Ok(_) => {}
            Err(_) => {
                let _ = tx.send(None);
                return;
            }
        }
    });

    if let Some(usd) = opts.budget {
        if let Err(e) = cmd.call("budget.set", json!({"mission_id": mission_id, "usd": usd})) {
            say_err(&rpc(&e));
            return EXIT_INFRA;
        }
    }
    let prior_mode = opts.permissions.as_ref().and_then(|_| cmd.call("permissions.get", json!({})).ok()).and_then(|v| v["mode"].as_str().map(String::from));
    if let Some(mode) = &opts.permissions {
        if let Err(e) = cmd.call("permissions.set", json!({"mode": mode})) {
            say_err(&rpc(&e));
            return EXIT_INFRA;
        }
    }

    let (method, params) = match chat::parse_input(&opts.input) {
        Input::Build(t) => {
            let (optimistic, text) = chat::split_build_flags(&t);
            ("build.start", json!({"mission_id": mission_id, "prompt": text, "optimistic": optimistic}))
        }
        Input::Prompt { target, text } => ("task.submit", chat::submit_params(&mission_id, target.as_deref(), &text)),
        Input::Council { proposition, seats, author } => ("council.start", json!({"mission_id": mission_id, "proposition": proposition, "seats": seats, "author_family": author})),
        Input::Distill(from) => ("bundle.distill", json!({"mission_id": mission_id, "from": from})),
        _ => {
            say_err("the task must be a prompt, `@role …`, `/build …`, `/council …` or `/distill …`");
            return EXIT_INFRA;
        }
    };
    let task_id = match cmd.call(method, params) {
        Ok(v) => v["task_id"].as_str().unwrap_or("").to_string(),
        Err(e) => {
            say_err(&rpc(&e));
            return EXIT_INFRA;
        }
    };
    let mut sum = Summary { task_id: Some(task_id.clone()), mission_id: mission_id.clone(), ..Default::default() };
    let deadline = opts.timeout.map(|t| started + t);
    let mut timed_out = false;
    let mut lost = false;
    loop {
        let wait = deadline.map(|d| d.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(3600));
        let e = match rx.recv_timeout(wait.min(Duration::from_secs(3600))) {
            Ok(Some(e)) => e,
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                lost = true;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    timed_out = true;
                    break;
                }
                continue;
            }
        };
        if opts.jsonl {
            println!("{}", e);
        }
        let mine = e["task_id"].as_str() == Some(task_id.as_str());
        let kind = e["type"].as_str().unwrap_or("");
        if kind == "approval.requested" && e["payload"]["task_id"].as_str() == Some(task_id.as_str()) {
            let card = &e["payload"];
            match decide(&opts.on_card, card) {
                Some((decision, reason)) => {
                    sum.card(card, decision, &reason);
                    if let Err(err) = cmd.call("approval.resolve", json!({"approval_id": card["approval_id"], "payload_blake3": card["payload_blake3"], "decision": decision, "reason": reason})) {
                        // A card that already timed out or was answered elsewhere is not an error for this run.
                        if !matches!(err, ClientError::Rpc { .. }) {
                            lost = true;
                            break;
                        }
                    }
                }
                None => {
                    sum.card(card, "left", "left for the operator (--on-card relay)");
                    say_err(&format!("waiting for the operator to answer: {} ({})", card["summary"].as_str().unwrap_or("a card"), card["approval_id"].as_str().unwrap_or("?")));
                }
            }
            continue;
        }
        if kind == "approval.resolved" {
            sum.apply(&e);
            continue;
        }
        if mine && kind == "decisions.review_requested" {
            // The build is finished and only the operator can go on. A script cannot review, so the run ends here and
            // stops the waiting task: the branch and the ledger stay, and `/decisions` then `/merge` finish it later.
            sum.apply(&e);
            let _ = cmd.call("task.cancel", json!({"task_id": task_id}));
            break;
        }
        if mine {
            sum.apply(&e);
            if kind == "task.completed" || kind == "task.failed" || kind == "task.cancelled" {
                if kind == "task.cancelled" {
                    sum.failed = Some("the task was cancelled".into());
                }
                break;
            }
        }
    }
    if timed_out {
        let _ = cmd.call("task.cancel", json!({"task_id": task_id}));
        sum.failed = Some(format!("timed out after {}s; the task was cancelled", started.elapsed().as_secs()));
    }
    if lost {
        sum.failed = Some("the connection to the daemon was lost".into());
    }
    let usage = cmd.call("usage.get", json!({"mission_id": mission_id, "group": "mission"})).unwrap_or(Value::Null);
    if let (Some(mode), Some(_)) = (prior_mode, &opts.permissions) {
        let _ = cmd.call("permissions.set", json!({"mode": mode}));
    }
    let (outcome, code) = if timed_out { ("timeout", EXIT_INFRA) } else { sum.outcome() };
    let summary = sum.to_json(outcome, code, started.elapsed(), usage);
    if opts.jsonl || opts.json {
        println!("{}", summary);
    } else {
        println!("{}", human(&summary));
    }
    code
}

fn human(s: &Value) -> String {
    let mut out = format!("outcome: {} (exit {})\nmission {} · task {}\n", s["outcome"].as_str().unwrap_or("?"), s["exit_code"], s["mission_id"].as_str().unwrap_or("?"), s["task_id"].as_str().unwrap_or("?"));
    if let Some(r) = s["ready_to_merge"].as_object() {
        out.push_str(&format!("ready to merge: branch {} at {}\n", r["branch"].as_str().unwrap_or("?"), r["sha"].as_str().unwrap_or("?")));
    }
    if let Some(c) = s["merge_commit"].as_str() {
        out.push_str(&format!("merged: {}\n", c));
    }
    if let Some(d) = s["decisions_awaiting_review"].as_object() {
        out.push_str(&format!("{} provisional decision(s) await your review: /decisions in the console, then /merge {}\n", d.get("count").and_then(|c| c.as_u64()).unwrap_or(0), s["task_id"].as_str().unwrap_or("<task>")));
    }
    if let Some(u) = s["pr_url"].as_str() {
        out.push_str(&format!("pull request: {}\n", u));
    }
    if !s["reason"].as_str().unwrap_or("").is_empty() {
        out.push_str(&format!("reason: {}\n", s["reason"].as_str().unwrap_or("")));
    }
    if let Some(e) = s["error"].as_str() {
        out.push_str(&format!("error: {}\n", e));
    }
    out.push_str(&format!("cards: {} ({} allowed, {} denied) · {:.1}s\n", s["cards"]["total"], s["cards"]["allowed"], s["cards"]["denied"], s["wall_secs"].as_f64().unwrap_or(0.0)));
    let text = s["text"].as_str().unwrap_or("");
    let attention = crate::escalation::classify(if text.contains("[escalated to the operator:") { text } else { s["reason"].as_str().unwrap_or("") });
    match attention.kind {
        // The wrapper is engine plumbing (specs/005e §6.6); say the stop and what to do about it instead.
        "no_model_configured" | "budget" | "reauth_required" | "exhausted" if text.contains("[escalated to the operator:") || s["outcome"] == "escalated" => {
            out.push_str(&format!("\n{}\n", attention.headline));
            // The hints name console commands; from `delphi run` the same remedies are flags and commands.
            let next = match attention.kind {
                "budget" => "raise the ceiling with --budget <usd>".to_string(),
                "no_model_configured" => "run `delphi setup` to connect a model".to_string(),
                _ => attention.hints.first().map(|h| h.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default(),
            };
            out.push_str(&format!("next: {}\n", next));
        }
        _ if !text.is_empty() => out.push_str(&format!("\n{}\n", crate::escalation::strip_wrapper(text))),
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_stopped_run_says_what_to_do_not_the_engine_wrapper() {
        let s = json!({"outcome": "escalated", "exit_code": 1, "cards": {"total": 0, "allowed": 0, "denied": 0}, "reason": "no_model_configured: none",
            "text": "[escalated to the operator: no_model_configured: no model is configured for this workspace]"});
        let out = super::human(&s);
        assert!(!out.contains("[escalated"), "{}", out);
        assert!(out.contains("Stopped: no model is configured"), "{}", out);
        assert!(out.contains("next: run `delphi setup`"), "{}", out);
    }

    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_task_is_the_positional_words_or_task_flag_and_flag_values_are_not_words() {
        let o = parse_args(&args(&["--workspace", "w", "/build", "do", "it", "--budget", "3", "--json"])).unwrap();
        assert_eq!(o.input, "/build do it");
        assert_eq!((o.budget, o.json, o.jsonl), (Some(3.0), true, false));
        assert_eq!(parse_args(&args(&["--task", "@architect draft x"])).unwrap().input, "@architect draft x");
        assert!(parse_args(&args(&["--json"])).unwrap_err().contains("give the task"));
    }

    #[test]
    fn bad_flag_values_are_refused_not_ignored() {
        for (a, want) in [
            (vec!["x", "--budget", "lots"], "dollar amount"),
            (vec!["x", "--budget", "0"], "dollar amount"),
            (vec!["x", "--permissions", "yolo"], "--permissions takes"),
            (vec!["x", "--timeout", "soon"], "not a duration"),
            (vec!["x", "--on-card", "yes"], "--on-card takes"),
            (vec!["x", "--json", "--jsonl"], "alternatives"),
        ] {
            let e = parse_args(&args(&a)).unwrap_err();
            assert!(e.contains(want), "{:?}: {}", a, e);
        }
    }

    #[test]
    fn durations_take_seconds_minutes_and_hours() {
        assert_eq!(parse_duration("90").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86_400));
        assert!(parse_duration("0").is_err() && parse_duration("m").is_err());
    }

    #[test]
    fn a_policy_can_allow_amber_cards_and_nothing_else() {
        let p = Policy::parse("[[allow]]\ntool = \"write_file\"\npattern = \"write_file @worktree/*\"\n[[allow]]\ntool = \"run_tests\"\n").unwrap();
        assert!(p.matches("write_file", "write_file @worktree/src/a.rs") && !p.matches("write_file", "write_file @project/x"));
        assert!(p.matches("run_tests", "anything"), "no pattern means any pattern");
        assert!(Policy::parse("[[allow]]\ntool = \"merge\"\n").unwrap_err().contains("no policy can allow it"));
        assert!(Policy::parse("[[allow]]\ntool = \"pull_request\"\n").is_err());
        assert!(Policy::parse("[[allow]]\ntool = \"x\"\ntier = \"red\"\n").unwrap_err().contains("Amber cards only"));
        assert!(Policy::parse("[[allow]]\npattern = \"x\"\n").unwrap_err().contains("needs a `tool`"));
        assert!(Policy::parse("[surprise]\nx = 1\n").unwrap_err().contains("unknown policy key"));
        assert!(Policy::parse("").unwrap().allow.is_empty());
    }

    #[test]
    fn globs_match_runs_of_characters_and_nothing_cleverer() {
        assert!(glob("a*c", "abc") && glob("a*c", "ac") && glob("*", "") && glob("a*b*c", "aXbYc"));
        assert!(!glob("a*c", "ab") && !glob("abc", "abcd") && glob("abc", "abc"));
        assert!(!glob("a*b*c", "acb"));
    }

    #[test]
    fn red_is_never_allowed_whatever_the_policy_says() {
        let policy = OnCard::Policy(Policy { allow: vec![Rule { tool: "write_file".into(), pattern: None }, Rule { tool: "danger".into(), pattern: None }] });
        let amber = json!({"tier": "amber", "tool": "write_file", "pattern": "write_file @worktree/a"});
        let red_same_tool = json!({"tier": "red", "tool": "danger", "pattern": "x"});
        let merge = json!({"tier": "red", "tool": "merge", "pattern": "merge abc"});
        let unlisted = json!({"tier": "amber", "tool": "other", "pattern": "x"});
        let d = |on: &OnCard, c: &Value| decide(on, c).map(|x| x.0);
        assert_eq!(d(&policy, &amber), Some("allow"));
        assert_eq!(d(&policy, &red_same_tool), Some("deny"), "a policy that lists a Red tool still cannot allow it");
        assert_eq!(d(&policy, &merge), Some("deny"));
        assert_eq!(d(&policy, &unlisted), Some("deny"));
        assert_eq!(d(&OnCard::Deny, &amber), Some("deny"));
        // Even if a card claims to be amber, a merge tool is never allowed.
        let sneaky = json!({"tier": "amber", "tool": "merge", "pattern": ""});
        assert_eq!(d(&policy, &sneaky), Some("deny"));
    }

    #[test]
    fn relay_mode_answers_nothing_except_the_merge_card_which_always_ends_the_run() {
        assert_eq!(parse_args(&args(&["x", "--on-card", "relay"])).unwrap().on_card, OnCard::Leave);
        assert_eq!(parse_args(&args(&["x", "--on-card", "park"])).unwrap().on_card, OnCard::Leave);
        let amber = json!({"tier": "amber", "tool": "write_file", "pattern": "p"});
        let red = json!({"tier": "red", "tool": "gh_pr_create", "pattern": "p"});
        let merge = json!({"tier": "red", "tool": "merge", "pattern": "merge abc"});
        assert_eq!(decide(&OnCard::Leave, &amber), None);
        assert_eq!(decide(&OnCard::Leave, &red), None, "a Red card waits for the operator, who can deny it by reply or approve it at the console");
        assert_eq!(decide(&OnCard::Leave, &merge).map(|x| x.0), Some("deny"));
    }

    #[test]
    fn a_build_waiting_for_review_ends_the_run_with_its_own_exit_code_and_names_the_next_step() {
        let mut s = done();
        s.task_id = Some("t_0007".into());
        s.apply(&json!({"type": "decisions.review_requested", "payload": {"count": 3, "decisions": []}}));
        assert_eq!(s.outcome(), ("awaiting_decision_review", EXIT_REVIEW));
        let j = s.to_json("awaiting_decision_review", EXIT_REVIEW, Duration::from_secs(1), Value::Null);
        assert_eq!(j["decisions_awaiting_review"]["count"], 3);
        let text = human(&j);
        assert!(text.contains("3 provisional decision(s) await your review") && text.contains("/merge t_0007"), "{}", text);
        // A failure still outranks it, and a merge that happened afterwards outranks the wait.
        s.apply(&json!({"type": "task.failed", "payload": {"error": "boom"}}));
        assert_eq!(s.outcome().1, EXIT_INFRA);
    }

    #[test]
    fn a_card_left_for_the_operator_is_recorded_with_how_it_ended() {
        let mut s = Summary::default();
        s.card(&json!({"approval_id": "ap_1", "tool": "write_file", "tier": "amber"}), "left", "left");
        s.card(&json!({"approval_id": "ap_2", "tool": "write_file", "tier": "amber"}), "left", "left");
        s.apply(&json!({"type": "approval.resolved", "payload": {"approval_id": "ap_1", "resolution": "approved", "by": "relay:email:me@example.com"}}));
        s.apply(&json!({"type": "approval.resolved", "payload": {"approval_id": "ap_2", "resolution": "timed_out", "by": "timeout"}}));
        let j = s.to_json("completed", 0, Duration::from_secs(1), Value::Null);
        assert_eq!((j["cards"]["allowed"].as_u64(), j["cards"]["denied"].as_u64(), j["cards"]["left"].as_u64()), (Some(1), Some(1), Some(0)));
        assert!(j["cards"]["list"][0]["reason"].as_str().unwrap().contains("relay:email:me@example.com"));
        assert!(s.denied_non_merge, "a timeout the operator never answered counts as a refusal");
    }

    fn done() -> Summary {
        let mut s = Summary::default();
        s.apply(&json!({"type": "task.completed", "payload": {"status": "done", "reason": ""}}));
        s
    }

    #[test]
    fn outcomes_and_exit_codes_follow_the_documented_order() {
        assert_eq!(done().outcome(), ("completed", 0));
        assert_eq!(Summary::default().outcome(), ("incomplete", 4));

        let mut ready = done();
        ready.card(&json!({"tool": "merge", "tier": "red", "approval_id": "a1", "payload": {"sha": "abc", "branch": "delphi/t_1", "files": [], "gates": [{"n": 1}]}}), "deny", "r");
        assert_eq!(ready.outcome(), ("ready_to_merge", 0));
        assert_eq!(ready.ready.as_ref().unwrap()["branch"], "delphi/t_1");
        assert!(!ready.denied_non_merge, "denying the merge card is the design, not a refusal");

        let mut merged = done();
        merged.apply(&json!({"type": "action.result", "payload": {"ok": true, "detail": {"merge_commit": "m1"}}}));
        assert_eq!(merged.outcome(), ("merged", 0));

        let mut gates = Summary::default();
        gates.apply(&json!({"type": "task.completed", "payload": {"status": "escalated", "reason": "gate 1: tests failed"}}));
        assert_eq!(gates.outcome(), ("escalated", 1));

        let mut budget = Summary::default();
        budget.apply(&json!({"type": "task.completed", "payload": {"status": "escalated", "reason": "mission budget: this call could cost up to $1"}}));
        budget.card(&json!({"tool": "write_file", "tier": "amber"}), "deny", "r");
        assert_eq!(budget.outcome(), ("budget_exceeded", 3), "a budget stop wins over a denied card");

        let mut denied = Summary::default();
        denied.card(&json!({"tool": "write_file", "tier": "amber"}), "deny", "r");
        denied.apply(&json!({"type": "task.completed", "payload": {"status": "escalated", "reason": "denied"}}));
        assert_eq!(denied.outcome(), ("denied", 2));

        let mut failed = done();
        failed.apply(&json!({"type": "task.failed", "payload": {"error": "boom"}}));
        assert_eq!(failed.outcome(), ("failed", 4), "an outright failure outranks everything");
    }

    #[test]
    fn the_summary_counts_cards_and_carries_the_ready_branch() {
        let mut s = done();
        s.mission_id = "m1".into();
        s.card(&json!({"tool": "write_file", "tier": "amber", "approval_id": "a1"}), "allow", "ok");
        s.card(&json!({"tool": "merge", "tier": "red", "approval_id": "a2", "payload": {"sha": "s", "branch": "b"}}), "deny", "stop");
        let j = s.to_json("ready_to_merge", 0, Duration::from_secs(3), Value::Null);
        assert_eq!((j["cards"]["total"].as_u64(), j["cards"]["allowed"].as_u64(), j["cards"]["denied"].as_u64()), (Some(2), Some(1), Some(1)));
        assert_eq!(j["ready_to_merge"]["sha"], "s");
        assert!(human(&j).contains("ready to merge: branch b at s"));
    }
}
