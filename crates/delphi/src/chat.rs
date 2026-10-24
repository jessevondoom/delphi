//! `delphi chat`: the operator's console (`specs/005` §3).
//!
//! A line-oriented REPL over two connections to the workspace daemon: one streams events and live
//! frames and prints them as they arrive, the other carries the operator's commands. Every prompt
//! becomes a task; `@role` routes to a role directly; `/usage`, `/budget`, `/permissions` read and
//! adjust the daemon. Approval cards follow the spec: the answer is bound to the exact payload shown
//! (`approval.resolve` carries its blake3), a card never captures what you were typing (answers are
//! explicit `/y /a /n /d`), and answers in the first 300 ms after a card appears are ignored.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Answers in this window after a card renders are ignored (`specs/005` §3.1 focus guard).
pub const FOCUS_GUARD: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    AllowSimilar,
    Deny,
    /// `[d]`: show the full payload without answering.
    View,
}

#[derive(Debug, PartialEq)]
pub enum Input {
    Empty,
    Help,
    Quit,
    Usage(Option<String>),
    Budget(Option<f64>),
    Permissions(Option<String>),
    Ratify(Vec<String>),
    Mission,
    Resume(Vec<String>),
    Pause,
    Answer(Decision),
    Prompt { target: Option<String>, text: String },
    /// `/build …`: a Builder task under the retry ladder, audit and merge gates.
    Build(String),
    Unknown(String),
}

pub fn parse_input(line: &str) -> Input {
    let line = line.trim();
    if line.is_empty() {
        return Input::Empty;
    }
    if let Some(cmd) = line.strip_prefix('/') {
        let mut it = cmd.split_whitespace();
        let name = it.next().unwrap_or("");
        let rest: Vec<&str> = it.collect();
        return match name {
            "help" | "?" => Input::Help,
            "quit" | "exit" | "q" => Input::Quit,
            "usage" => Input::Usage(rest.first().map(|s| s.to_string())),
            "budget" => match rest.first() {
                None => Input::Budget(None),
                Some(v) => v.trim_start_matches('$').parse::<f64>().map(|x| Input::Budget(Some(x))).unwrap_or_else(|_| Input::Unknown(format!("/budget takes a dollar amount, not `{}`", v))),
            },
            "permissions" | "perm" => Input::Permissions(rest.first().map(|s| s.to_string())),
            "ratify" => Input::Ratify(rest.iter().map(|s| s.to_string()).collect()),
            "mission" => Input::Mission,
            "resume" => Input::Resume(rest.iter().map(|s| s.to_string()).collect()),
            "pause" => Input::Pause,
            "build" => {
                let text = cmd.trim_start().strip_prefix("build").unwrap_or("").trim();
                if text.is_empty() { Input::Unknown("/build needs a request, for example `/build implement @specs/011-x.md in @project/app`".into()) } else { Input::Build(text.to_string()) }
            }
            "y" | "yes" => Input::Answer(Decision::Allow),
            "a" | "always" => Input::Answer(Decision::AllowSimilar),
            "n" | "no" => Input::Answer(Decision::Deny),
            "d" | "diff" => Input::Answer(Decision::View),
            "approve" => match rest.first().copied() {
                Some("y") => Input::Answer(Decision::Allow),
                Some("a") => Input::Answer(Decision::AllowSimilar),
                Some("n") => Input::Answer(Decision::Deny),
                Some("d") => Input::Answer(Decision::View),
                _ => Input::Unknown("/approve takes y, a, n or d".into()),
            },
            other => Input::Unknown(format!("unknown command /{} (try /help)", other)),
        };
    }
    if let Some(rest) = line.strip_prefix('@') {
        // `@architect do the thing` routes to a role; `@project/x/y` alone is a path mention in a prompt.
        let (first, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if !first.contains('/') && !first.is_empty() && !text.trim().is_empty() {
            return Input::Prompt { target: Some(first.to_string()), text: text.trim().to_string() };
        }
    }
    Input::Prompt { target: None, text: line.to_string() }
}

pub const HELP: &str = "\
Type a request and press Enter. Reference files with @project/<name>/path or @specs/…; route to a role with `@architect …`.
  /usage [mission|role|task|model]   token and cost totals
  /budget [dollars]                  show or change this mission's cost ceiling
  /permissions [default|accept-edits|mission-auto]
  /ratify                            pending proposals (config, manifests, specs)
  /mission                           the current mission
  /build <request>                   a Builder task: worktree, tests, audit, then your approval to merge
  /resume [task…]  /pause            after a restart: continue the interrupted tasks, or leave them
  /y /a /n /d                        answer an approval card: yes · allow similar · no · view the full payload
  /quit";

/// A pending approval card as the console tracks it.
#[derive(Debug, Clone)]
pub struct Card {
    pub approval_id: String,
    pub payload_blake3: String,
    pub tier: String,
    pub payload: Value,
    pub rendered_at: Instant,
}

impl Card {
    pub fn from_event(payload: &Value, now: Instant) -> Card {
        Card { approval_id: payload["approval_id"].as_str().unwrap_or("").into(), payload_blake3: payload["payload_blake3"].as_str().unwrap_or("").into(), tier: payload["tier"].as_str().unwrap_or("amber").into(), payload: payload["payload"].clone(), rendered_at: now }
    }

    /// The focus guard: refuse answers that arrive too soon after the card appeared.
    pub fn may_answer(&self, now: Instant) -> Result<(), String> {
        let age = now.saturating_duration_since(self.rendered_at);
        if age < FOCUS_GUARD {
            Err(format!("ignored: the card appeared {} ms ago; answer again if you meant it", age.as_millis()))
        } else {
            Ok(())
        }
    }
}

pub fn render_card(e: &Value, who: &str) -> String {
    let tier = e["tier"].as_str().unwrap_or("amber");
    let badge = if tier == "red" { "RED — never auto-approved" } else { "amber" };
    let mut s = format!("\n┌─ approval needed [{}] · {} · {}\n", badge, who, e["tool"].as_str().unwrap_or("?"));
    s.push_str(&format!("│ {}\n", e["summary"].as_str().unwrap_or("")));
    if let Some(preview) = e["payload"]["preview"].as_str() {
        for l in preview.lines().take(6) {
            s.push_str(&format!("│   {}\n", l.chars().take(100).collect::<String>()));
        }
    }
    let similar = if tier == "red" { "" } else { " · /a allow similar this task" };
    s.push_str(&format!("└─ /y approve once{} · /n reject · /d view all   (id {}, expires in {}s)\n", similar, e["approval_id"].as_str().unwrap_or("?"), e["timeout_secs"].as_u64().unwrap_or(0)));
    s
}

/// Turns events and frames into terminal text, tracking which role owns which span.
pub struct Renderer {
    names: HashMap<String, String>,
    spans: HashMap<String, (String, u64)>,
    streamed: HashSet<String>,
    last_span: Option<String>,
}

fn brief(args: &Value) -> String {
    let pick = ["path", "pattern", "role"].iter().find_map(|k| args[*k].as_str());
    let s = pick.map(String::from).or_else(|| args.as_object().and_then(|o| o.values().find_map(|v| v.as_str().map(String::from)))).unwrap_or_default();
    let mut t: String = s.chars().take(60).collect();
    if s.chars().count() > 60 {
        t.push('…');
    }
    t
}

impl Renderer {
    pub fn new(names: HashMap<String, String>) -> Renderer {
        Renderer { names, spans: HashMap::new(), streamed: HashSet::new(), last_span: None }
    }

    fn name(&self, role: &str) -> String {
        self.names.get(role).cloned().unwrap_or_else(|| role.to_string())
    }

    fn span_name(&self, span: &str) -> String {
        self.spans.get(span).map(|(r, _)| self.name(r)).unwrap_or_else(|| "agent".into())
    }

    /// Live text from a coalesced frame. Thinking is not shown.
    pub fn frame(&mut self, params: &Value) -> String {
        let mut out = String::new();
        let Some(spans) = params["spans"].as_object() else { return out };
        let mut keys: Vec<&String> = spans.keys().collect();
        keys.sort();
        for k in keys {
            let text = spans[k.as_str()]["text"].as_str().unwrap_or("");
            if text.is_empty() {
                continue;
            }
            if self.last_span.as_deref() != Some(k.as_str()) {
                out.push_str(&format!("\n{}: ", self.span_name(k)));
                self.last_span = Some(k.clone());
            }
            self.streamed.insert(k.clone());
            out.push_str(text);
        }
        out
    }

    /// One persisted event. `None` means nothing to show.
    pub fn event(&mut self, e: &Value) -> Option<String> {
        let p = &e["payload"];
        let span = e["span_id"].as_str().unwrap_or("");
        match e["type"].as_str()? {
            "dag.node.added" => {
                let (role, depth) = (p["role"].as_str().unwrap_or("?").to_string(), p["depth"].as_u64().unwrap_or(0));
                self.spans.insert(p["node_id"].as_str().unwrap_or("").to_string(), (role.clone(), depth));
                (depth > 0).then(|| format!("\n  ↳ {} takes this on\n", self.name(&role)))
            }
            "agent.block.complete" if p["block"]["type"] == "tool_call" => Some(format!("\n  → {}({})", p["block"]["name"].as_str().unwrap_or("?"), brief(&p["block"]["arguments"]))),
            "tool.result" if p["is_error"] == true => Some(format!("\n    ✗ {}", p["content"].as_str().unwrap_or("").lines().next().unwrap_or("").chars().take(120).collect::<String>())),
            "approval.resolved" => Some(format!("\n  approval {}: {}{}\n", p["approval_id"].as_str().unwrap_or("?"), p["resolution"].as_str().unwrap_or("?"), p["by"].as_str().map(|b| format!(" (by {})", b)).unwrap_or_default())),
            "span.taint" => Some(format!("\n  ⚠ {} read untrusted content (tier {}); its actions are restricted\n", self.span_name(span), p["ceiling"].as_str().unwrap_or("?"))),
            "endpoint.throttled" => Some(format!("\n  ⚠ {} is throttled; trying the next endpoint\n", p["endpoint_id"].as_str().unwrap_or("?"))),
            "endpoint.reauth_required" => Some(format!("\n  ⚠ re-authenticate {} (e.g. `aws sso login`), then run the task again\n", p["endpoint_id"].as_str().unwrap_or("?"))),
            "turn.interrupted" => Some("\n  ⚠ the stream was interrupted; retrying the turn\n".to_string()),
            "task.failed" => Some(format!("\n✗ task failed: {}\n", p["error"].as_str().unwrap_or(""))),
            "task.output" => {
                let root_streamed = self.streamed.iter().any(|s| self.spans.get(s).map(|(_, d)| *d == 0).unwrap_or(false));
                let tid = e["task_id"].as_str().unwrap_or("");
                self.last_span = None;
                let text = p["text"].as_str().unwrap_or("");
                Some(match p["status"].as_str() {
                    Some("escalated") => format!("\n⚠ {} needs you: {}\n", tid, text),
                    _ if root_streamed => format!("\n✓ {} done\n", tid),
                    _ => {
                        // A reply that finished before any live frame arrived still says who is speaking.
                        let mut roots: Vec<&String> = self.spans.iter().filter(|(_, (_, d))| *d == 0).map(|(k, _)| k).collect();
                        roots.sort();
                        match roots.first() {
                            Some(k) => format!("\n{}: {}\n✓ {} done\n", self.span_name(k), text, tid),
                            None => format!("\n{}: {}\n✓ {} done\n", self.name("orchestrator"), text, tid),
                        }
                    }
                })
            }
            _ => None,
        }
    }
}

/// What the console shows on attach when the daemon recovered unfinished work.
pub fn render_recovery(r: &Value) -> Option<String> {
    let tasks = r["tasks"].as_array()?;
    if tasks.is_empty() {
        return None;
    }
    let mut s = String::from("Recovered after a restart; nothing has resumed yet:\n");
    for t in tasks {
        s.push_str(&format!("  {} ({}): {}\n", t["task_id"].as_str().unwrap_or("?"), t["role"].as_str().unwrap_or("?"), t["action"].as_str().unwrap_or("")));
    }
    let charged = r["charged_usd"].as_f64().unwrap_or(0.0);
    if charged > 0.0 {
        s.push_str(&format!("  ${:.4} was charged for calls the restart interrupted (their full reservations).\n", charged));
    }
    let expired = r["expired_approvals"].as_array().map(|a| a.len()).unwrap_or(0);
    if expired > 0 {
        s.push_str(&format!("  {} approval card(s) expired with the daemon; those actions will be asked about again.\n", expired));
    }
    s.push_str("/resume to continue them · /pause to leave them\n");
    Some(s)
}

pub fn render_usage(v: &Value) -> String {
    let mut s = format!("usage for mission {}\n", v["mission_id"].as_str().unwrap_or("?"));
    s.push_str(&format!("{:<22} {:>5} {:>10} {:>10} {:>10} {:>9}\n", "", "calls", "in", "cache", "out", "cost"));
    let mut total = 0.0;
    for r in v["rows"].as_array().cloned().unwrap_or_default() {
        let cache = r["cache_read_tokens"].as_u64().unwrap_or(0) + r["cache_write_tokens"].as_u64().unwrap_or(0);
        total += r["cost_usd"].as_f64().unwrap_or(0.0);
        s.push_str(&format!("{:<22} {:>5} {:>10} {:>10} {:>10} {:>9}\n", r["key"].as_str().unwrap_or("?"), r["calls"], r["input_tokens"], cache, r["output_tokens"], format!("${:.4}", r["cost_usd"].as_f64().unwrap_or(0.0))));
    }
    let b = &v["budget"];
    s.push_str(&format!("total ${:.4} of ${:.2} ceiling (${:.4} reserved, ${:.4} available)", total, b["cap_usd"].as_f64().unwrap_or(0.0), b["reserved_usd"].as_f64().unwrap_or(0.0), b["available_usd"].as_f64().unwrap_or(0.0)));
    s
}

pub fn submit_params(mission: &str, target: Option<&str>, text: &str) -> Value {
    let mut p = json!({"mission_id": mission, "prompt": text});
    if let Some(t) = target {
        p["target"] = json!(t);
    }
    p
}

// ── terminal wiring ─────────────────────────────────────────────────────────

use delphi_core::client::{Client, ClientError};
use delphi_core::spawn::{connect_or_spawn, layout, SpawnOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct Shared {
    card: Mutex<Option<Card>>,
    renderer: Mutex<Renderer>,
    names: HashMap<String, String>,
    running: AtomicUsize,
    failures: AtomicUsize,
    closed: AtomicBool,
}

fn say(s: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

/// This executable: the daemon is `delphi __core --daemon …`, so there is nothing else to find.
pub fn daemon_exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

fn rpc_error(e: &ClientError) -> String {
    match e {
        ClientError::Rpc { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

fn reader(mut ev: Client, shared: Arc<Shared>) {
    loop {
        match ev.next_notification() {
            Ok(n) => match n["method"].as_str() {
                Some("delta.frame") => {
                    let s = shared.renderer.lock().unwrap().frame(&n["params"]);
                    if !s.is_empty() {
                        say(&s);
                    }
                }
                Some("event") => handle_event(&shared, &n["params"]),
                Some("resync_required") => say("\n(the console fell behind; some live text was skipped)\n"),
                _ => {}
            },
            Err(_) => {
                shared.closed.store(true, Ordering::SeqCst);
                say("\n(the daemon connection closed)\n");
                return;
            }
        }
    }
}

fn handle_event(shared: &Shared, e: &Value) {
    let kind = e["type"].as_str().unwrap_or("");
    let p = &e["payload"];
    match kind {
        "approval.requested" => {
            *shared.card.lock().unwrap() = Some(Card::from_event(p, Instant::now()));
            let who = shared.names.get(p["role"].as_str().unwrap_or("")).cloned().unwrap_or_else(|| p["role"].as_str().unwrap_or("agent").to_string());
            say(&render_card(p, &who));
            return;
        }
        "approval.resolved" => {
            let mut c = shared.card.lock().unwrap();
            if c.as_ref().map(|c| c.approval_id.as_str()) == p["approval_id"].as_str() {
                *c = None;
            }
        }
        "task.output" | "task.failed" => {
            let escalated = p["status"] == "escalated" || kind == "task.failed";
            if escalated {
                shared.failures.fetch_add(1, Ordering::SeqCst);
            }
            // Only this thread decrements, so the check-then-subtract cannot underflow.
            if shared.running.load(Ordering::SeqCst) > 0 {
                shared.running.fetch_sub(1, Ordering::SeqCst);
            }
        }
        _ => {}
    }
    if let Some(s) = shared.renderer.lock().unwrap().event(e) {
        say(&s);
    }
}

fn is_tty() -> bool {
    unsafe { libc::isatty(0) == 1 }
}

/// Run the console. Returns the process exit code.
pub fn run(home: &Path, ws_dir: &Path, mission: Option<String>, names: HashMap<String, String>) -> i32 {
    let Some(exe) = daemon_exe() else {
        eprintln!("delphi chat: cannot locate this program to start the daemon");
        return 1;
    };
    let mut attached = match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()] }) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi chat: {}", rpc_error(&e));
            return 1;
        }
    };
    let opened = match attached.client.call("mission.open", json!({"mission_id": mission})) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("delphi chat: {}", rpc_error(&e));
            return 1;
        }
    };
    let mission_id = opened["mission_id"].as_str().unwrap_or("").to_string();
    let recovery_note = render_recovery(&opened["recovery"]);
    let mut ev = match Client::connect(&attached.sock).map_err(ClientError::from).and_then(|mut c| c.hello(&attached.token).map(|_| c)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi chat: {}", rpc_error(&e));
            return 1;
        }
    };
    let _ = ev.call("events.subscribe", json!({}));
    let shared = Arc::new(Shared { card: Mutex::new(None), renderer: Mutex::new(Renderer::new(names.clone())), names, running: AtomicUsize::new(0), failures: AtomicUsize::new(0), closed: AtomicBool::new(false) });
    let s2 = shared.clone();
    std::thread::spawn(move || reader(ev, s2));

    let mut cmd = attached.client;
    say(&format!("delphi · mission {} · budget ${:.2} · daemon pid {}{}\n/help for commands\n", mission_id, opened["budget"]["cap_usd"].as_f64().unwrap_or(0.0), attached.daemon_pid, if attached.spawned { " (started)" } else { "" }));
    if let Ok(list) = cmd.call("approval.list", json!({})) {
        for card in list.as_array().cloned().unwrap_or_default() {
            *shared.card.lock().unwrap() = Some(Card::from_event(&card, Instant::now()));
            say(&render_card(&card, card["role"].as_str().unwrap_or("agent")));
        }
    }

    if let Some(note) = recovery_note {
        say(&note);
    }
    // The console announces pending proposals and config notices on attach.
    if let Ok(snap) = cmd.call("session.snapshot", json!({"mission_id": mission_id})) {
        for n in snap["notices"].as_array().cloned().unwrap_or_default() {
            say(&format!("⚠ {}\n", n.as_str().unwrap_or("")));
        }
        let pending = snap["pending_proposals"].as_u64().unwrap_or(0);
        if pending > 0 {
            say(&format!("{} proposal(s) await your ratification: /ratify\n", pending));
        }
    }
    let mut last_ratify: Vec<Value> = Vec::new();
    let interactive = is_tty();
    let stdin = std::io::stdin();
    let mut line = String::new();
    loop {
        if interactive {
            say("› ");
        }
        line.clear();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 || shared.closed.load(Ordering::SeqCst) {
            break;
        }
        match parse_input(&line) {
            Input::Empty => {}
            Input::Quit => break,
            Input::Help => say(&format!("{}\n", HELP)),
            Input::Unknown(m) => say(&format!("{}\n", m)),
            Input::Mission => say(&format!("mission {}\n", mission_id)),
            Input::Resume(tasks) => match cmd.call("recovery.resume", json!({"mission_id": mission_id, "tasks": if tasks.is_empty() { Value::Null } else { json!(tasks) }})) {
                Ok(v) => {
                    let n = v["resumed"].as_array().map(|a| a.len()).unwrap_or(0);
                    shared.running.fetch_add(n, Ordering::SeqCst);
                    say(&if n == 0 { "nothing to resume\n".to_string() } else { format!("resuming {} task(s)…\n", n) });
                }
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Pause => match cmd.call("recovery.pause", json!({})) {
                Ok(_) => say("left as they are; they will be offered again after the next restart\n"),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Usage(group) => match cmd.call("usage.get", json!({"mission_id": mission_id, "group": group.unwrap_or_else(|| "role".into())})) {
                Ok(v) => say(&format!("{}\n", render_usage(&v))),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Budget(None) => match cmd.call("budget.get", json!({"mission_id": mission_id})) {
                Ok(b) => say(&format!("ceiling ${:.2} · spent ${:.4} · reserved ${:.4} · available ${:.4}\n", b["cap_usd"].as_f64().unwrap_or(0.0), b["spent_usd"].as_f64().unwrap_or(0.0), b["reserved_usd"].as_f64().unwrap_or(0.0), b["available_usd"].as_f64().unwrap_or(0.0))),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Budget(Some(usd)) => match cmd.call("budget.set", json!({"mission_id": mission_id, "usd": usd})) {
                Ok(b) => say(&format!("ceiling is now ${:.2}\n", b["cap_usd"].as_f64().unwrap_or(0.0))),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Permissions(None) => match cmd.call("permissions.get", json!({})) {
                Ok(v) => {
                    let sim: Vec<String> = v["allow_similar"].as_array().cloned().unwrap_or_default().iter().map(|s| format!("{} ({})", s["pattern"].as_str().unwrap_or(""), s["task_id"].as_str().unwrap_or(""))).collect();
                    say(&format!("mode: {} · allowed similar this task: {}\n", v["mode"].as_str().unwrap_or("?"), if sim.is_empty() { "none".to_string() } else { sim.join(", ") }));
                }
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Permissions(Some(mode)) => match cmd.call("permissions.set", json!({"mode": mode})) {
                Ok(v) => say(&format!("mode is now {}\n", v["mode"].as_str().unwrap_or("?"))),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Ratify(args) => say(&crate::ratify_console::handle(&mut cmd, &args, &mut last_ratify)),
            Input::Answer(d) => {
                let card = shared.card.lock().unwrap().clone();
                let Some(card) = card else {
                    say("no approval is waiting\n");
                    continue;
                };
                if d == Decision::View {
                    say(&format!("{}\n", serde_json::to_string_pretty(&card.payload).unwrap_or_default()));
                    continue;
                }
                if d == Decision::AllowSimilar && card.tier == "red" {
                    say("`allow similar` never applies to a Red action; use /y to approve this once or /n\n");
                    continue;
                }
                if let Err(m) = card.may_answer(Instant::now()) {
                    say(&format!("{}\n", m));
                    continue;
                }
                let decision = match d {
                    Decision::Allow => "allow",
                    Decision::AllowSimilar => "allow_similar",
                    _ => "deny",
                };
                if let Err(e) = cmd.call("approval.resolve", json!({"approval_id": card.approval_id, "payload_blake3": card.payload_blake3, "decision": decision})) {
                    say(&format!("{}\n", rpc_error(&e)));
                }
            }
            Input::Build(text) => match cmd.call("build.start", json!({"mission_id": mission_id, "prompt": text})) {
                Ok(v) => {
                    shared.running.fetch_add(1, Ordering::SeqCst);
                    say(&format!("({} build started; you will be asked before anything is merged)\n", v["task_id"].as_str().unwrap_or("task")));
                }
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Prompt { target, text } => match cmd.call("task.submit", submit_params(&mission_id, target.as_deref(), &text)) {
                Ok(v) => {
                    shared.running.fetch_add(1, Ordering::SeqCst);
                    say(&format!("({} started)\n", v["task_id"].as_str().unwrap_or("task")));
                }
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
        }
    }

    // Input ended: nobody can answer a waiting card any more, so deny it, then let running tasks finish.
    if !interactive {
        loop {
            let card = shared.card.lock().unwrap().clone();
            if let Some(c) = card {
                say("(input closed; denying the waiting approval)\n");
                let _ = cmd.call("approval.resolve", json!({"approval_id": c.approval_id, "payload_blake3": c.payload_blake3, "decision": "deny", "reason": "input closed"}));
                std::thread::sleep(Duration::from_millis(50));
            }
            if shared.running.load(Ordering::SeqCst) == 0 || shared.closed.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    say("\n");
    if shared.failures.load(Ordering::SeqCst) > 0 {
        1
    } else {
        0
    }
}

/// `delphi daemon status|stop|restart`.
pub fn daemon(home: &Path, ws_dir: &Path, action: &str) -> i32 {
    let l = match layout(home, ws_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("delphi daemon: {}", e);
            return 1;
        }
    };
    let connect = || -> Result<Client, ClientError> {
        let mut c = Client::connect(&l.sock)?;
        let t = c.issue_token(delphi_core::proto::Scope::Console)?;
        c.hello(&t)?;
        Ok(c)
    };
    match action {
        "status" => match connect() {
            Ok(mut c) => match c.call("session.snapshot", json!({})) {
                Ok(s) => {
                    println!("running · mission {} · {} task(s) running · {} approval(s) waiting", s["mission_id"].as_str().unwrap_or("?"), s["running_tasks"].as_array().map(|a| a.len()).unwrap_or(0), s["pending_approvals"].as_array().map(|a| a.len()).unwrap_or(0));
                    0
                }
                Err(e) => {
                    eprintln!("{}", rpc_error(&e));
                    1
                }
            },
            Err(_) => {
                println!("not running");
                0
            }
        },
        "stop" | "restart" => {
            match connect() { Ok(mut c) => {
                let _ = c.call("daemon.shutdown", json!({}));
                for _ in 0..100 {
                    if Client::connect(&l.sock).is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                println!("daemon stopped");
            } _ => if action == "stop" {
                println!("not running");
            }}
            if action == "restart" {
                let Some(exe) = daemon_exe() else {
                    eprintln!("cannot locate this program to start the daemon");
                    return 1;
                };
                return match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()] }) {
                    Ok(a) => {
                        println!("daemon started (pid {})", a.daemon_pid);
                        0
                    }
                    Err(e) => {
                        eprintln!("{}", rpc_error(&e));
                        1
                    }
                };
            }
            0
        }
        other => {
            eprintln!("delphi daemon: unknown action `{}` (status, stop, restart)", other);
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> HashMap<String, String> {
        [("orchestrator", "Athena"), ("architect", "Daedalus")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn input_parsing() {
        assert_eq!(parse_input("  "), Input::Empty);
        assert_eq!(parse_input("/help"), Input::Help);
        assert_eq!(parse_input("/usage role"), Input::Usage(Some("role".into())));
        assert_eq!(parse_input("/budget"), Input::Budget(None));
        assert_eq!(parse_input("/budget $7.5"), Input::Budget(Some(7.5)));
        assert!(matches!(parse_input("/budget lots"), Input::Unknown(_)));
        assert_eq!(parse_input("/permissions accept-edits"), Input::Permissions(Some("accept-edits".into())));
        assert_eq!(parse_input("/build implement @specs/011-x.md in @project/app"), Input::Build("implement @specs/011-x.md in @project/app".into()));
        assert!(matches!(parse_input("/build"), Input::Unknown(m) if m.contains("needs a request")));
        assert_eq!(parse_input("/resume"), Input::Resume(vec![]));
        assert_eq!(parse_input("/resume t_0001 t_0003"), Input::Resume(vec!["t_0001".into(), "t_0003".into()]));
        assert_eq!(parse_input("/pause"), Input::Pause);
        assert_eq!(parse_input("/y"), Input::Answer(Decision::Allow));
        assert_eq!(parse_input("/a"), Input::Answer(Decision::AllowSimilar));
        assert_eq!(parse_input("/approve n"), Input::Answer(Decision::Deny));
        assert_eq!(parse_input("/d"), Input::Answer(Decision::View));
        assert!(matches!(parse_input("/launch-missiles"), Input::Unknown(m) if m.contains("/help")));
        assert_eq!(parse_input("explain the ladder"), Input::Prompt { target: None, text: "explain the ladder".into() });
        assert_eq!(parse_input("@architect draft a spec for X"), Input::Prompt { target: Some("architect".into()), text: "draft a spec for X".into() });
        // A path mention is not a role route.
        assert_eq!(parse_input("@project/app/src/lib.rs what does this do"), Input::Prompt { target: None, text: "@project/app/src/lib.rs what does this do".into() });
        assert_eq!(parse_input("@architect"), Input::Prompt { target: None, text: "@architect".into() });
    }

    #[test]
    fn the_focus_guard_ignores_early_answers() {
        let t0 = Instant::now();
        let card = Card::from_event(&json!({"approval_id": "ap_1", "payload_blake3": "h", "tier": "amber", "payload": {}}), t0);
        assert!(card.may_answer(t0 + Duration::from_millis(100)).unwrap_err().contains("ignored"));
        assert!(card.may_answer(t0 + Duration::from_millis(299)).is_err());
        assert!(card.may_answer(t0 + Duration::from_millis(300)).is_ok());
    }

    #[test]
    fn cards_show_the_action_a_preview_and_the_available_answers() {
        let amber = json!({"approval_id": "ap_0001", "tier": "amber", "tool": "write_file", "summary": "write specs/009-x.md (12 bytes)", "payload": {"preview": "# New\nbody"}, "timeout_secs": 600});
        let s = render_card(&amber, "Daedalus");
        assert!(s.contains("Daedalus") && s.contains("write specs/009-x.md") && s.contains("│   # New") && s.contains("/a allow similar") && s.contains("ap_0001"), "{}", s);
        let red = render_card(&json!({"approval_id": "ap_2", "tier": "red", "tool": "dispatch_agent", "summary": "dispatch", "payload": {}, "timeout_secs": 60}), "Athena");
        assert!(red.contains("RED — never auto-approved") && !red.contains("allow similar"), "{}", red);
    }

    #[test]
    fn events_render_with_theme_names_and_stream_text_is_not_printed_twice() {
        let mut r = Renderer::new(names());
        assert_eq!(r.event(&json!({"type": "dag.node.added", "payload": {"node_id": "sp_orchestrator_1", "role": "orchestrator", "depth": 0}})), None);
        assert!(r.event(&json!({"type": "dag.node.added", "payload": {"node_id": "sp_architect_2", "role": "architect", "depth": 1}})).unwrap().contains("Daedalus takes this on"));
        let live = r.frame(&json!({"spans": {"sp_orchestrator_1": {"text": "Hello ", "thinking": "secret"}}}));
        assert_eq!(live, "\nAthena: Hello ", "the header appears once and thinking is never shown");
        assert_eq!(r.frame(&json!({"spans": {"sp_orchestrator_1": {"text": "world", "thinking": ""}}})), "world");
        let done = r.event(&json!({"type": "task.output", "task_id": "t_0001", "payload": {"status": "done", "text": "Hello world"}})).unwrap();
        assert_eq!(done, "\n✓ t_0001 done\n", "already streamed, so only the status line");
        // A task whose text was never streamed prints it at the end.
        let mut fresh = Renderer::new(names());
        assert!(fresh.event(&json!({"type": "task.output", "task_id": "t_0002", "payload": {"status": "done", "text": "The answer."}})).unwrap().contains("The answer."));
        let esc = fresh.event(&json!({"type": "task.output", "task_id": "t_0003", "payload": {"status": "escalated", "text": "[escalated to the operator: mission budget]"}})).unwrap();
        assert!(esc.contains("t_0003 needs you") && esc.contains("mission budget"), "{}", esc);
    }

    #[test]
    fn tool_activity_and_warnings_are_narrated_briefly() {
        let mut r = Renderer::new(names());
        let call = r.event(&json!({"type": "agent.block.complete", "payload": {"block": {"type": "tool_call", "name": "read_file", "arguments": {"path": "@project/app/src/lib.rs"}}}})).unwrap();
        assert_eq!(call, "\n  → read_file(@project/app/src/lib.rs)");
        assert!(r.event(&json!({"type": "agent.block.complete", "payload": {"block": {"type": "text", "text": "x"}}})).is_none());
        assert!(r.event(&json!({"type": "tool.result", "payload": {"is_error": true, "content": "denied: outside your role's access scope\nmore"}})).unwrap().contains("✗ denied: outside your role"));
        assert!(r.event(&json!({"type": "tool.result", "payload": {"is_error": false, "content": "fine"}})).is_none());
        assert!(r.event(&json!({"type": "span.taint", "span_id": "x", "payload": {"ceiling": "L0"}})).unwrap().contains("untrusted content (tier L0)"));
        assert!(r.event(&json!({"type": "endpoint.reauth_required", "payload": {"endpoint_id": "bedrock-us"}})).unwrap().contains("re-authenticate bedrock-us"));
        assert!(r.event(&json!({"type": "approval.resolved", "payload": {"approval_id": "ap_1", "resolution": "approved", "by": "console-3"}})).unwrap().contains("approved (by console-3)"));
        assert!(r.event(&json!({"type": "budget.reserve", "payload": {}})).is_none());
    }

    #[test]
    fn the_recovery_summary_says_what_will_happen_and_what_it_cost() {
        let r = json!({"tasks": [{"task_id": "t_0001", "role": "architect", "action": "architect retries its interrupted turn"}], "charged_usd": 0.0225, "expired_approvals": ["ap_1"]});
        let s = render_recovery(&r).unwrap();
        assert!(s.contains("nothing has resumed yet") && s.contains("t_0001 (architect): architect retries") && s.contains("$0.0225 was charged") && s.contains("1 approval card(s) expired") && s.contains("/resume"), "{}", s);
        assert_eq!(render_recovery(&json!({"tasks": []})), None);
        assert_eq!(render_recovery(&json!({})), None);
    }

    #[test]
    fn the_usage_table_totals_and_shows_the_ceiling() {
        let v = json!({"mission_id": "m1", "rows": [{"key": "orchestrator", "calls": 2, "input_tokens": 2000, "cache_read_tokens": 100, "cache_write_tokens": 50, "output_tokens": 400, "cost_usd": 0.06}, {"key": "architect", "calls": 1, "input_tokens": 900, "cache_read_tokens": 0, "cache_write_tokens": 0, "output_tokens": 100, "cost_usd": 0.02}],
            "budget": {"cap_usd": 5.0, "reserved_usd": 0.0, "available_usd": 4.92}});
        let s = render_usage(&v);
        assert!(s.contains("orchestrator") && s.contains("$0.0600") && s.contains("total $0.0800 of $5.00 ceiling"), "{}", s);
    }
}
