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
    /// `/council [--seats a,b,c] [--author family] <proposition>`: a three-round Council on a decision.
    Council { proposition: String, seats: Vec<String>, author: Option<String> },
    /// `/narrator [view]`: open the read-only window (a tmux split when inside tmux, else the command to run).
    Narrator(Option<String>),
    /// `/more`: the rest of a reply the inline console shortened.
    More,
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
            "narrator" | "observe" | "visualize" => match rest.first() {
                Some(v) if *v != "tile" && crate::observer::View::parse(v).is_none() => Input::Unknown(format!("/narrator takes council, epic, forge, agents, spend, archives or tile, not `{}`", v)),
                v => Input::Narrator(v.map(|s| s.to_string())),
            },
            "more" => Input::More,
            "council" => parse_council(cmd.trim_start().strip_prefix("council").unwrap_or("")),
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

fn parse_council(args: &str) -> Input {
    const USAGE: &str = "/council needs a proposition, for example `/council Should the retry queue be a table or a file?` (optional: --seats architect,strategist,red-team --author claude)";
    let (mut seats, mut author) = (Vec::new(), None);
    let mut words = args.split_whitespace().peekable();
    let mut rest: Vec<&str> = Vec::new();
    while let Some(w) = words.next() {
        match w {
            "--seats" if rest.is_empty() => match words.next() {
                Some(v) => seats = v.split(',').filter(|x| !x.is_empty()).map(String::from).collect(),
                None => return Input::Unknown(USAGE.into()),
            },
            "--author" if rest.is_empty() => match words.next() {
                Some(v) => author = Some(v.to_string()),
                None => return Input::Unknown(USAGE.into()),
            },
            _ => rest.push(w),
        }
    }
    if rest.is_empty() {
        return Input::Unknown(USAGE.into());
    }
    Input::Council { proposition: rest.join(" "), seats, author }
}

pub const HELP: &str = "\
Type a request and press Enter. Reference files with @project/<name>/path or @specs/…; route to a role with `@architect …`.
  /usage [mission|role|task|model]   token and cost totals
  /budget [dollars]                  show or change this mission's cost ceiling
  /permissions [default|accept-edits|mission-auto]
  /ratify                            pending proposals (config, manifests, specs)
  /mission                           the current mission
  /build <request>                   a Builder task: worktree, tests, audit, then your approval to merge
  /narrator [council|epic|forge|agents|spend|archives|tile]   the read-only live window (a tmux split, or the command to run in a second terminal)
  /council <proposition>             a blind three-round Council (architect, strategist, red-team); you decide if it deadlocks
  /resume [task…]  /pause            after a restart: continue the interrupted tasks, or leave them
  /more                              the rest of the last reply, when it was shortened
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
    let mut s = format!("\n── approval needed [{}] · {} · {}\n", badge, who, e["tool"].as_str().unwrap_or("?"));
    s.push_str(&format!("  {}\n", e["summary"].as_str().unwrap_or("")));
    if let Some(preview) = e["payload"]["preview"].as_str() {
        for l in preview.lines().take(6) {
            s.push_str(&format!("    {}\n", l.chars().take(100).collect::<String>()));
        }
    }
    let similar = if tier == "red" { "" } else { " · /a allow similar this task" };
    s.push_str(&format!("  /y approve once{} · /n reject · /d view all   (id {}, expires in {}s)\n", similar, e["approval_id"].as_str().unwrap_or("?"), e["timeout_secs"].as_u64().unwrap_or(0)));
    s
}

/// Turns events and frames into terminal text, tracking which role owns which span.
pub struct Renderer {
    names: HashMap<String, String>,
    spans: HashMap<String, (String, u64)>,
    streamed: HashSet<String>,
    last_span: Option<String>,
    /// Council seats answer in parallel, so their live text is not streamed; their stances are shown whole.
    council: bool,
    quiet: HashSet<String>,
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
        Renderer { names, spans: HashMap::new(), streamed: HashSet::new(), last_span: None, council: false, quiet: HashSet::new() }
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
            if text.is_empty() || self.quiet.contains(k.as_str()) {
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

    /// The new text of the working agents' replies, with no labels, for the inline console's live row.
    pub fn live_text(&mut self, params: &Value) -> String {
        let Some(spans) = params["spans"].as_object() else { return String::new() };
        let mut keys: Vec<&String> = spans.keys().collect();
        keys.sort();
        keys.into_iter().filter(|k| !self.quiet.contains(k.as_str())).filter_map(|k| spans[k.as_str()]["text"].as_str()).collect::<Vec<_>>().concat()
    }

    /// A finished task's output as one unit, spoken by the newest top-level agent.
    pub fn reply_for(&mut self, e: &Value) -> Reply {
        self.last_span = None;
        self.council = false;
        let num = |k: &str| k.rsplit('_').next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
        let role = self.spans.iter().filter(|(_, (_, d))| *d == 0).max_by_key(|(k, _)| num(k)).map(|(_, (r, _))| r.clone()).unwrap_or_else(|| "orchestrator".into());
        Reply {
            task_id: e["task_id"].as_str().unwrap_or("").to_string(),
            name: self.name(&role),
            role,
            text: delphi_engine::observe::sanitize(e["payload"]["text"].as_str().unwrap_or("")),
            status: if e["payload"]["status"] == "escalated" { "escalated".into() } else { "done".into() },
        }
    }

    /// One persisted event. `None` means nothing to show.
    pub fn event(&mut self, e: &Value) -> Option<String> {
        let p = &e["payload"];
        let span = e["span_id"].as_str().unwrap_or("");
        match e["type"].as_str()? {
            "dag.node.added" => {
                let (role, depth) = (p["role"].as_str().unwrap_or("?").to_string(), p["depth"].as_u64().unwrap_or(0));
                self.spans.insert(p["node_id"].as_str().unwrap_or("").to_string(), (role.clone(), depth));
                if self.council {
                    self.quiet.insert(p["node_id"].as_str().unwrap_or("").to_string());
                    return None;
                }
                (depth > 0).then(|| format!("\n  ↳ {} takes this on\n", self.name(&role)))
            }
            "council.started" => {
                self.council = true;
                let seats = p["seats"].as_array().map(|a| a.iter().map(|s| format!("{} ({})", self.name(s["role"].as_str().unwrap_or("?")), s["family"].as_str().unwrap_or("?"))).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                Some(format!("\nCouncil convened — {}{}\n  Round 1 is blind: nobody sees anyone else's answer until all are in.\n", seats, if p["homogeneous"] == true { " · one model family only (homogeneous)" } else { "" }))
            }
            "council.barrier.released" => Some("\n  Round 1 is in; the stances are now visible to every seat.\n".to_string()),
            "council.stance" => {
                let who = self.name(p["role"].as_str().unwrap_or("?"));
                let text = p["text"].as_str().unwrap_or("");
                Some(match p["status"].as_str() {
                    Some("answered") => format!("\n  {}: {}\n", who, text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(140).collect::<String>()),
                    _ => format!("\n  {} abstained: {}\n", who, text.chars().take(140).collect::<String>()),
                })
            }
            "council.critique" => {
                let who = self.name(p["role"].as_str().unwrap_or("?"));
                Some(match p["recommendation"].as_str() {
                    Some(rec) => format!("  {} → {}{}\n", who, rec, match p["findings"].as_array().map(|f| f.len()).unwrap_or(0) { 0 => String::new(), n => format!(" · {} finding(s)", n) }),
                    None => format!("  {} → no readable critique\n", who),
                })
            }
            "agent.block.complete" if p["block"]["type"] == "tool_call" => Some(format!("\n  → {}({})", p["block"]["name"].as_str().unwrap_or("?"), brief(&p["block"]["arguments"]))),
            "tool.result" if p["is_error"] == true => Some(format!("\n    ✗ {}", p["content"].as_str().unwrap_or("").lines().next().unwrap_or("").chars().take(120).collect::<String>())),
            "approval.resolved" => Some(format!("\n  approval {}: {}{}\n", p["approval_id"].as_str().unwrap_or("?"), p["resolution"].as_str().unwrap_or("?"), p["by"].as_str().map(|b| format!(" (by {})", b)).unwrap_or_default())),
            "span.taint" => Some(format!("\n  ! {} read untrusted content (tier {}); its actions are restricted\n", self.span_name(span), p["ceiling"].as_str().unwrap_or("?"))),
            "endpoint.throttled" => Some(format!("\n  ! {} is throttled; trying the next endpoint\n", p["endpoint_id"].as_str().unwrap_or("?"))),
            "model.homogeneous" => Some(format!("\n  ! no model outside {} was usable; this review is by the same family as the author\n", p["avoided"])),
            "task.blocked" => Some(format!("\n  ! waiting for a model: {}{}\n", p["why"].as_str().unwrap_or("?"), p["back_at"].as_i64().map(|t| format!(" (back at {})", delphi_core::time::format_unix_millis(t, 0))).unwrap_or_default())),
            "task.unblocked" => Some(if p["resumed"] == true { "\n  a model is back; resuming\n".to_string() } else { "\n  ! stopped waiting for a model\n".to_string() }),
            "model.degraded_confirm" if p["state"] == "asked" => Some("\n  ! no model as capable as the target is available; approve or decline the weaker one below\n".to_string()),
            "model.recovered" => Some(format!("\n  {} is available again ({})\n", p["endpoint_id"].as_str().unwrap_or("?"), p["model_id"].as_str().unwrap_or("?"))),
            "model.exhausted" => Some(format!("\n  ! {} is out of quota ({}){}; trying the next endpoint\n", p["endpoint_id"].as_str().unwrap_or("?"), p["reason"].as_str().unwrap_or("?"), p["reset_at"].as_i64().map(|t| format!(", back at {}", delphi_core::time::format_unix_millis(t, 0))).unwrap_or_default())),
            "endpoint.reauth_required" => Some(format!("\n  ! re-authenticate {} (e.g. `aws sso login`), then run the task again\n", p["endpoint_id"].as_str().unwrap_or("?"))),
            "turn.interrupted" => Some("\n  ! the stream was interrupted; retrying the turn\n".to_string()),
            "task.failed" => Some(format!("\n✗ task failed: {}\n", p["error"].as_str().unwrap_or(""))),
            "task.output" => {
                let root_streamed = self.streamed.iter().any(|s| self.spans.get(s).map(|(_, d)| *d == 0).unwrap_or(false));
                let tid = e["task_id"].as_str().unwrap_or("");
                self.last_span = None;
                self.council = false;
                let text = p["text"].as_str().unwrap_or("");
                Some(match p["status"].as_str() {
                    Some("escalated") => format!("\n! {} needs you: {}\n", tid, text),
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

pub(crate) struct Shared {
    pub(crate) card: Mutex<Option<Card>>,
    /// What the daemon has told this console, folded for the header and footer (no model text is kept).
    pub(crate) obs: Mutex<delphi_engine::observe::ObserverState>,
    renderer: Mutex<Renderer>,
    names: HashMap<String, String>,
    /// The lines of the last reply that were cut for brevity, for `/more`.
    pub(crate) more: Mutex<Vec<String>>,
    pub(crate) running: AtomicUsize,
    pub(crate) failures: AtomicUsize,
    pub(crate) closed: AtomicBool,
}

/// Where console output goes. Plain mode writes to stdout; the inline console installs a channel and prints
/// the text above its pinned block.
static SINK: Mutex<Option<std::sync::mpsc::Sender<Out>>> = Mutex::new(None);

/// What the console is asked to show. The plain console only ever sees `Text`; the inline one also gets the
/// unfinished text of a reply (shown on its live row, never printed into the scrollback) and the finished
/// reply as a unit, so it can keep it short.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Text(String),
    Live(String),
    LiveClear,
    Reply(Reply),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub task_id: String,
    pub role: String,
    pub name: String,
    pub text: String,
    /// `done` or `escalated`.
    pub status: String,
}

pub fn set_sink(tx: Option<std::sync::mpsc::Sender<Out>>) {
    *SINK.lock().unwrap() = tx;
}

/// Is an inline console attached (so replies are delivered whole, not streamed)?
fn inline() -> bool {
    SINK.lock().unwrap().is_some()
}

fn emit(o: Out) {
    if let Some(tx) = SINK.lock().unwrap().as_ref() {
        let _ = tx.send(o);
    }
}

/// Everything that reaches the screen passes through here: text from models and tools is stripped of terminal
/// control sequences first, so it can neither move the cursor nor forge the pinned block.
pub fn say(s: &str) {
    let clean = delphi_engine::observe::sanitize(s);
    if let Some(tx) = SINK.lock().unwrap().as_ref() {
        let _ = tx.send(Out::Text(clean));
        return;
    }
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(clean.as_bytes());
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
                    shared.obs.lock().unwrap().apply_frame(&n["params"]);
                    if inline() {
                        let t = shared.renderer.lock().unwrap().live_text(&n["params"]);
                        if !t.is_empty() {
                            emit(Out::Live(delphi_engine::observe::sanitize(&t)));
                        }
                    } else {
                        let s = shared.renderer.lock().unwrap().frame(&n["params"]);
                        if !s.is_empty() {
                            say(&s);
                        }
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
    shared.obs.lock().unwrap().apply(e);
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
    if inline() && kind == "task.output" {
        let reply = shared.renderer.lock().unwrap().reply_for(e);
        emit(Out::LiveClear);
        emit(Out::Reply(reply));
        return;
    }
    if let Some(s) = shared.renderer.lock().unwrap().event(e) {
        say(&s);
    }
}

pub(crate) fn is_tty() -> bool {
    unsafe { libc::isatty(0) == 1 }
}

/// One attached console: the daemon connection, the mission, and the event-driven state the reader thread keeps
/// current. Both the plain console and the inline one drive it through [`Session::handle`], so every command
/// behaves the same in either.
pub(crate) struct Session {
    cmd: Client,
    pub(crate) mission_id: String,
    pub(crate) shared: Arc<Shared>,
    ws_dir: PathBuf,
    home: PathBuf,
    last_ratify: Vec<Value>,
}

/// Start (or join) the daemon, open the mission, start the event reader, and print what the operator should see first.
pub(crate) fn connect(home: &Path, ws_dir: &Path, mission: Option<String>, names: HashMap<String, String>) -> Result<Session, i32> {
    let Some(exe) = daemon_exe() else {
        eprintln!("delphi chat: cannot locate this program to start the daemon");
        return Err(1);
    };
    let mut attached = match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()] }) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi chat: {}", rpc_error(&e));
            return Err(1);
        }
    };
    let opened = match attached.client.call("mission.open", json!({"mission_id": mission})) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("delphi chat: {}", rpc_error(&e));
            return Err(1);
        }
    };
    let mission_id = opened["mission_id"].as_str().unwrap_or("").to_string();
    let recovery_note = render_recovery(&opened["recovery"]);
    let mut ev = match Client::connect(&attached.sock).map_err(ClientError::from).and_then(|mut c| c.hello(&attached.token).map(|_| c)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi chat: {}", rpc_error(&e));
            return Err(1);
        }
    };
    let _ = ev.call("events.subscribe", json!({}));
    let mut cmd = attached.client;
    // What the header and footer need: the roles, and history so a console opened mid-mission is not blank.
    let mut obs = delphi_engine::observe::ObserverState::new(&[]);
    if let Ok(snap) = cmd.call("observe.snapshot", json!({"mission_id": mission_id})) {
        let roles: Vec<String> = snap["enabled_agents"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        obs = delphi_engine::observe::ObserverState::new(&roles);
        for e in snap["events"].as_array().into_iter().flatten() {
            obs.apply(e);
        }
    }
    let shared = Arc::new(Shared { card: Mutex::new(None), obs: Mutex::new(obs), renderer: Mutex::new(Renderer::new(names.clone())), names, more: Mutex::new(Vec::new()), running: AtomicUsize::new(0), failures: AtomicUsize::new(0), closed: AtomicBool::new(false) });
    let s2 = shared.clone();
    std::thread::spawn(move || reader(ev, s2));

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
            say(&format!("! {}\n", n.as_str().unwrap_or("")));
        }
        let pending = snap["pending_proposals"].as_u64().unwrap_or(0);
        if pending > 0 {
            say(&format!("{} proposal(s) await your ratification: /ratify\n", pending));
        }
    }
    Ok(Session { cmd, mission_id, shared, ws_dir: ws_dir.to_path_buf(), home: home.to_path_buf(), last_ratify: Vec::new() })
}

impl Session {
    pub(crate) fn ws_dir(&self) -> &Path {
        &self.ws_dir
    }

    /// Run one parsed input. `false` means the operator asked to quit.
    pub(crate) fn handle(&mut self, input: Input) -> bool {
        match input {
            Input::Empty => {}
            Input::Quit => return false,
            Input::Help => say(&format!("{}\n", HELP)),
            Input::Unknown(m) => say(&format!("{}\n", m)),
            Input::Mission => say(&format!("mission {}\n", self.mission_id)),
            Input::More => {
                let rest = std::mem::take(&mut *self.shared.more.lock().unwrap());
                say(&if rest.is_empty() { "nothing more: the last reply was shown whole\n".to_string() } else { format!("{}\n", rest.join("\n")) });
            }
            Input::Resume(tasks) => match self.cmd.call("recovery.resume", json!({"mission_id": self.mission_id, "tasks": if tasks.is_empty() { Value::Null } else { json!(tasks) }})) {
                Ok(v) => {
                    let n = v["resumed"].as_array().map(|a| a.len()).unwrap_or(0);
                    self.shared.running.fetch_add(n, Ordering::SeqCst);
                    say(&if n == 0 { "nothing to resume\n".to_string() } else { format!("resuming {} task(s)…\n", n) });
                }
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Pause => match self.cmd.call("recovery.pause", json!({})) {
                Ok(_) => say("left as they are; they will be offered again after the next restart\n"),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Usage(group) => match self.cmd.call("usage.get", json!({"mission_id": self.mission_id, "group": group.unwrap_or_else(|| "role".into())})) {
                Ok(v) => say(&format!("{}\n", render_usage(&v))),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Budget(None) => match self.cmd.call("budget.get", json!({"mission_id": self.mission_id})) {
                Ok(b) => say(&format!("ceiling ${:.2} · spent ${:.4} · reserved ${:.4} · available ${:.4}\n", b["cap_usd"].as_f64().unwrap_or(0.0), b["spent_usd"].as_f64().unwrap_or(0.0), b["reserved_usd"].as_f64().unwrap_or(0.0), b["available_usd"].as_f64().unwrap_or(0.0))),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Budget(Some(usd)) => match self.cmd.call("budget.set", json!({"mission_id": self.mission_id, "usd": usd})) {
                Ok(b) => say(&format!("ceiling is now ${:.2}\n", b["cap_usd"].as_f64().unwrap_or(0.0))),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Permissions(None) => match self.cmd.call("permissions.get", json!({})) {
                Ok(v) => {
                    let sim: Vec<String> = v["allow_similar"].as_array().cloned().unwrap_or_default().iter().map(|s| format!("{} ({})", s["pattern"].as_str().unwrap_or(""), s["task_id"].as_str().unwrap_or(""))).collect();
                    say(&format!("mode: {} · allowed similar this task: {}\n", v["mode"].as_str().unwrap_or("?"), if sim.is_empty() { "none".to_string() } else { sim.join(", ") }));
                }
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Permissions(Some(mode)) => match self.cmd.call("permissions.set", json!({"mode": mode})) {
                Ok(v) => say(&format!("mode is now {}\n", v["mode"].as_str().unwrap_or("?"))),
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Ratify(args) => say(&crate::ratify_console::handle(&mut self.cmd, &args, &mut self.last_ratify)),
            Input::Answer(d) => {
                let card = self.shared.card.lock().unwrap().clone();
                let Some(card) = card else {
                    say("no approval is waiting\n");
                    return true;
                };
                if d == Decision::View {
                    say(&format!("{}\n", serde_json::to_string_pretty(&card.payload).unwrap_or_default()));
                    return true;
                }
                if d == Decision::AllowSimilar && card.tier == "red" {
                    say("`allow similar` never applies to a Red action; use /y to approve this once or /n\n");
                    return true;
                }
                if let Err(m) = card.may_answer(Instant::now()) {
                    say(&format!("{}\n", m));
                    return true;
                }
                let decision = match d {
                    Decision::Allow => "allow",
                    Decision::AllowSimilar => "allow_similar",
                    _ => "deny",
                };
                if let Err(e) = self.cmd.call("approval.resolve", json!({"approval_id": card.approval_id, "payload_blake3": card.payload_blake3, "decision": decision})) {
                    say(&format!("{}\n", rpc_error(&e)));
                }
            }
            Input::Build(text) => match self.cmd.call("build.start", json!({"mission_id": self.mission_id, "prompt": text})) {
                Ok(v) => {
                    self.shared.running.fetch_add(1, Ordering::SeqCst);
                    say(&format!("({} build started; you will be asked before anything is merged)\n", v["task_id"].as_str().unwrap_or("task")));
                }
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Narrator(view) => {
                let ws = self.ws_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let exe = daemon_exe().map(|p| p.display().to_string()).unwrap_or_else(|| "delphi".into());
                let tile = view.as_deref() == Some("tile");
                let views: Vec<String> = if tile { ["council", "epic", "forge", "agents"].iter().map(|s| s.to_string()).collect() } else { vec![view.unwrap_or_default()] };
                let lines: Vec<String> = views.iter().map(|v| format!("{} observe {} --workspace {} --home {}", exe, v, ws, self.home.display())).collect();
                if std::env::var_os("TMUX").is_some() {
                    // One window per view, laid out by tmux: Delphi never tiles inside a window.
                    let opened = lines.iter().enumerate().all(|(i, l)| std::process::Command::new("tmux").args(["split-window", if i % 2 == 0 { "-h" } else { "-v" }, l]).status().map(|s| s.success()).unwrap_or(false));
                    if opened {
                        let _ = std::process::Command::new("tmux").args(["select-layout", "tiled"]).status();
                        say(&format!("({} observer window(s) opened in tmux)\n", lines.len()));
                    } else {
                        say(&format!("could not open tmux panes; run these in other terminals:\n  {}\n", lines.join("\n  ")));
                    }
                } else {
                    say(&format!("Run {} in another terminal (it only watches):\n  {}\n", if tile { "each of these" } else { "this" }, lines.join("\n  ")));
                }
            }
            Input::Council { proposition, seats, author } => match self.cmd.call("council.start", json!({"mission_id": self.mission_id, "proposition": proposition, "seats": seats, "author_family": author})) {
                Ok(v) => {
                    self.shared.running.fetch_add(1, Ordering::SeqCst);
                    say(&format!("({} council convened; the seats answer blind, then critique each other)\n", v["task_id"].as_str().unwrap_or("task")));
                }
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
            Input::Prompt { target, text } => match self.cmd.call("task.submit", submit_params(&self.mission_id, target.as_deref(), &text)) {
                Ok(v) => {
                    self.shared.running.fetch_add(1, Ordering::SeqCst);
                    say(&format!("({} started)\n", v["task_id"].as_str().unwrap_or("task")));
                }
                Err(e) => say(&format!("{}\n", rpc_error(&e))),
            },
        }
        true
    }

    /// The approval mode, for the footer.
    pub(crate) fn permission_mode(&mut self) -> String {
        self.cmd.call("permissions.get", json!({})).ok().and_then(|v| v["mode"].as_str().map(String::from)).unwrap_or_else(|| "default".into())
    }

    /// Input has ended and nobody can answer a waiting card any more: deny it, then let running tasks finish.
    pub(crate) fn drain(&mut self) {
        loop {
            let card = self.shared.card.lock().unwrap().clone();
            if let Some(c) = card {
                say("(input closed; denying the waiting approval)\n");
                let _ = self.cmd.call("approval.resolve", json!({"approval_id": c.approval_id, "payload_blake3": c.payload_blake3, "decision": "deny", "reason": "input closed"}));
                std::thread::sleep(Duration::from_millis(50));
            }
            if self.shared.running.load(Ordering::SeqCst) == 0 || self.shared.closed.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Run the plain, line-based console. Returns the process exit code.
pub fn run(home: &Path, ws_dir: &Path, mission: Option<String>, names: HashMap<String, String>) -> i32 {
    let mut session = match connect(home, ws_dir, mission, names) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let interactive = is_tty();
    let stdin = std::io::stdin();
    let mut line = String::new();
    loop {
        if interactive {
            say("› ");
        }
        line.clear();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 || session.shared.closed.load(Ordering::SeqCst) {
            break;
        }
        if !session.handle(parse_input(&line)) {
            break;
        }
    }
    if !interactive {
        session.drain();
    }
    say("\n");
    if session.shared.failures.load(Ordering::SeqCst) > 0 {
        1
    } else {
        0
    }
}

/// Start (or join) the daemon, then run the full-screen UI as a child of this process. The UI gets what it needs to
/// connect (socket paths, mission, role names and colours) as one JSON argument: no token, no secret. It asks the
/// daemon for its own token through the socket, which checks that it was started from a terminal.
pub fn launch_ui(ui: &Path, home: &Path, ws_dir: &Path, mission: Option<String>, names: &HashMap<String, String>) -> i32 {
    let Some(exe) = daemon_exe() else {
        eprintln!("delphi chat: cannot locate this program to start the daemon");
        return 1;
    };
    if let Err(e) = connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()] }) {
        eprintln!("delphi chat: {}", rpc_error(&e));
        return 1;
    }
    let l = match layout(home, ws_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("delphi chat: {}", e);
            return 1;
        }
    };
    let info = ui_info(&l.sock, &l.narrator_sock, ws_dir, mission, names);
    match std::process::Command::new(ui).arg("--info").arg(info.to_string()).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("delphi chat: cannot start the full-screen console ({}); try `delphi chat --inline`", e);
            1
        }
    }
}

/// The full-screen Observer: the same UI executable in its read-only mode. It needs a running daemon (it only watches).
/// Exit code 3 means the UI is not this release and the classic Observer should run instead.
pub fn launch_ui_observer(ui: &Path, home: &Path, ws_dir: &Path, view: Option<&str>, names: &HashMap<String, String>) -> i32 {
    let l = match layout(home, ws_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("delphi observe: {}", e);
            return 1;
        }
    };
    if std::os::unix::net::UnixStream::connect(&l.narrator_sock).is_err() {
        eprintln!("delphi observe: no daemon is running for this workspace. Start the console (`delphi chat`) first: the Observer only watches.");
        return 1;
    }
    let info = ui_info(&l.sock, &l.narrator_sock, ws_dir, None, names);
    let mut cmd = std::process::Command::new(ui);
    cmd.arg("--info").arg(info.to_string()).arg("--observe");
    if let Some(v) = view {
        cmd.arg(v);
    }
    match cmd.status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("delphi observe: cannot start the full-screen Observer ({}); try `delphi observe --classic`", e);
            1
        }
    }
}

/// What the UI needs to know that only this program knows: where things are, and how roles look.
pub(crate) fn ui_info(console: &Path, narrator: &Path, ws_dir: &Path, mission: Option<String>, names: &HashMap<String, String>) -> Value {
    // Where `delphi` itself is, and which workspace and home this is, for the few things the UI asks the CLI for
    // (`delphi connectors status`). Paths only; nothing secret.
    let exe = std::env::current_exe().ok().map(|p| p.display().to_string()).unwrap_or_else(|| "delphi".into());
    let ws_name = ws_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let home = ws_dir.parent().and_then(|p| p.parent()).map(|p| p.display().to_string()).unwrap_or_default();
    let roles: serde_json::Map<String, Value> = crate::avatar::ROLES
        .iter()
        .map(|t| {
            let (r, g, b) = t.chassis_rgb;
            (t.role_id.to_string(), json!({"name": names.get(t.role_id).cloned().unwrap_or_else(|| t.role_id.to_string()), "chassis": format!("#{:02x}{:02x}{:02x}", r, g, b), "face": t.badge_face, "crown": t.crown_slice, "crown_rgb": format!("#{:02x}{:02x}{:02x}", t.crown_rgb.0, t.crown_rgb.1, t.crown_rgb.2)}))
        })
        .collect();
    let cwd = std::env::current_dir().ok().map(|p| p.display().to_string()).unwrap_or_default();
    json!({"console_socket": console, "narrator_socket": narrator, "workspace": ws_dir, "mission": mission, "version": env!("CARGO_PKG_VERSION"), "cwd": cwd, "roles": roles, "avatar": crate::avatar::ui_spec(), "delphi_exe": exe, "workspace_name": ws_name, "home": home})
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
        // For clients written in other languages: where the daemon's sockets are.
        "where" => {
            println!("{}", ui_info(&l.sock, &l.narrator_sock, ws_dir, None, &HashMap::new()));
            0
        }
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
            eprintln!("delphi daemon: unknown action `{}` (status, stop, restart, where)", other);
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
        assert_eq!(parse_input("/narrator"), Input::Narrator(None));
        assert_eq!(parse_input("/observe council"), Input::Narrator(Some("council".into())));
        assert!(matches!(parse_input("/narrator nonsense"), Input::Unknown(m) if m.contains("council, epic")));
        assert_eq!(parse_input("/narrator tile"), Input::Narrator(Some("tile".into())));
        assert_eq!(parse_input("/observe spend"), Input::Narrator(Some("spend".into())));
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
        assert!(s.contains("Daedalus") && s.contains("write specs/009-x.md") && s.contains("    # New") && s.contains("/a allow similar") && s.contains("ap_0001"), "{}", s);
        let red = render_card(&json!({"approval_id": "ap_2", "tier": "red", "tool": "dispatch_agent", "summary": "dispatch", "payload": {}, "timeout_secs": 60}), "Athena");
        assert!(red.contains("RED — never auto-approved") && !red.contains("allow similar"), "{}", red);
    }

    #[test]
    fn ui_info_gives_the_ui_sockets_and_role_looks_and_no_secrets() {
        let mut n = HashMap::new();
        n.insert("orchestrator".to_string(), "Orch".to_string());
        let v = ui_info(Path::new("/run/c.sock"), Path::new("/run/n.sock"), Path::new("/ws"), Some("m1".into()), &n);
        assert_eq!((v["console_socket"].as_str(), v["narrator_socket"].as_str(), v["mission"].as_str()), (Some("/run/c.sock"), Some("/run/n.sock"), Some("m1")));
        assert_eq!(v["roles"]["orchestrator"]["name"], "Orch");
        assert_eq!(v["roles"]["orchestrator"]["chassis"], "#a855f7");
        assert_eq!(v["roles"]["builder"]["name"], "builder", "an unthemed role keeps its id");
        assert!(!v.to_string().to_lowercase().contains("token"), "nothing secret goes to the UI");
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
