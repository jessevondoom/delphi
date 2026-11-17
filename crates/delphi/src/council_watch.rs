//! `delphi council watch` (`specs/123` §7): follow a Council or a Panel from a second terminal.
//!
//! It reads the mission's event log from disk; it asks no daemon, calls no model and writes nothing. Every line it prints is a fixed
//! form: a header from `council.started` (roles, families and tiers checked as identifiers), the status tail's lines
//! (`progress::tail_line`), a progress line (the phase and a token count) and the end state with the record's path. Seat text
//! (a stance's or a critique's `text`) is never read into the output. Plain lines only: no spinner and nothing redrawn in place
//! (the reduce-motion rule); colour only on a terminal without `NO_COLOR`.

use delphi_core::event::EventEnvelope;
use delphi_engine::progress;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

/// The states a watch ends on.
const ENDS: [&str; 5] = ["decided", "needs_operator", "not_decided", "failed", "cancelled"];

/// At most this many earlier tail lines are printed when a watch starts on a Council already under way.
const BACKLOG: usize = 20;

/// The Council or Panel a watch follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub mission: String,
    pub dir: PathBuf,
    pub task_id: String,
}

fn of_task<'a>(events: &'a [EventEnvelope], task_id: &str) -> Vec<&'a EventEnvelope> {
    events.iter().filter(|e| e.task_id.as_deref() == Some(task_id) || (e.kind == delphi_engine::host_rpc::COUNCIL_EVENT && e.payload["task_id"] == task_id)).collect()
}

/// The Council and Panel tasks of a log, in the order they were raised: a host's request or a `council.started`.
fn councils_in(events: &[EventEnvelope]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in events {
        let id = match e.kind.as_str() {
            k if k == delphi_engine::host_rpc::COUNCIL_EVENT => e.payload["task_id"].as_str(),
            "council.started" => e.task_id.as_deref(),
            _ => None,
        };
        if let Some(id) = id.filter(|i| !out.iter().any(|o| o == i)) {
            out.push(id.to_string());
        }
    }
    out
}

/// A Council's state from its events, as `host_rpc::council_view` derives it: convening or deliberating while it runs.
pub fn state(task: &[&EventEnvelope]) -> &'static str {
    let done = task.iter().find(|e| e.kind == "task.completed");
    let verdict = task.iter().any(|e| e.kind == "council.verdict");
    match (done, verdict) {
        (Some(d), true) if d.payload["status"] == "done" => "decided",
        (Some(_), true) => "needs_operator",
        (Some(_), false) => "not_decided",
        (None, _) if task.iter().any(|e| e.kind == "task.cancelled") => "cancelled",
        (None, _) if task.iter().any(|e| e.kind == "task.failed") => "failed",
        (None, _) if task.iter().any(|e| e.kind == "council.started") => "deliberating",
        (None, _) => "convening",
    }
}

/// Pick the Council to watch across the workspace's missions (host missions included): `id` when given, else the latest one still
/// running, else the latest one. `mission` narrows the search. A task id is per mission, so an id found in several is the latest.
pub fn find(ws_dir: &Path, id: Option<&str>, mission: Option<&str>) -> Result<Found, String> {
    // (running, timestamp it was raised, found)
    let mut best: Option<(bool, String, Found)> = None;
    for (mid, dir) in delphi_core::sessions::dirs(ws_dir) {
        if mission.is_some_and(|m| m != mid) {
            continue;
        }
        let Ok(events) = delphi_core::log::replay(&dir) else { continue };
        for tid in councils_in(&events) {
            if id.is_some_and(|i| i != tid) {
                continue;
            }
            let task = of_task(&events, &tid);
            let running = id.is_none() && !ENDS.contains(&state(&task));
            let at = task.first().map(|e| e.timestamp.clone()).unwrap_or_default();
            if best.as_ref().is_none_or(|(r, t, _)| (running, &at) > (*r, t)) {
                best = Some((running, at, Found { mission: mid.clone(), dir: dir.clone(), task_id: tid }));
            }
        }
    }
    best.map(|(_, _, f)| f).ok_or_else(|| match (id, mission) {
        (Some(i), Some(m)) => format!("no Council `{i}` in mission `{m}`"),
        (Some(i), None) => format!("no Council `{i}` in this workspace's missions"),
        (None, Some(m)) => format!("no Council or Panel in mission `{m}`"),
        (None, None) => "no Council or Panel in this workspace's missions".to_string(),
    })
}

/// A family or model name as it may be printed: letters, digits, `.`, `-`, `_`, `:`, `/`, at most 64 characters; else `other`.
fn ident(raw: &str) -> &str {
    if !raw.is_empty() && raw.len() <= 64 && raw.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-:/".contains(&b)) {
        raw
    } else {
        "other"
    }
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn venue(task: &[&EventEnvelope]) -> &'static str {
    let panel = task.iter().any(|e| (e.kind == delphi_engine::host_rpc::COUNCIL_EVENT || e.kind == "council.started" || e.kind == "task.created") && e.payload["venue"] == "panel");
    if panel { "panel" } else { "daemon" }
}

/// The planned seats line, from `council.started`; `None` before the seats are chosen.
fn seats_line(task: &[&EventEnvelope]) -> Option<String> {
    let started = task.iter().find(|e| e.kind == "council.started")?;
    let seats: Vec<String> = started.payload["seats"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let tier = s["tier"].as_str().filter(|t| matches!(*t, "fast" | "workhorse" | "frontier")).unwrap_or("default tier");
            format!("{} ({}, {tier})", progress::role_name(s["role"].as_str().unwrap_or("")), ident(s["family"].as_str().unwrap_or("unknown")))
        })
        .collect();
    let ceiling = match (started.payload["budget_tokens"].as_u64(), started.payload["budget_usd"].as_f64()) {
        (Some(0), _) => "no token ceiling".to_string(),
        (Some(n), _) => format!("ceiling {} tokens", thousands(n)),
        (None, Some(usd)) => format!("ceiling ${usd:.2} (council_budget_usd)"),
        (None, None) => "no ceiling recorded".to_string(),
    };
    Some(format!("seats: {}; {ceiling}", if seats.is_empty() { "none planned".to_string() } else { seats.join(", ") }))
}

fn tokens(task: &[&EventEnvelope]) -> u64 {
    task.iter().filter(|e| e.kind == "agent.usage").map(|e| e.payload["input_tokens"].as_u64().unwrap_or(0) + e.payload["output_tokens"].as_u64().unwrap_or(0)).sum()
}

fn progress_line(events: &[EventEnvelope], task: &[&EventEnvelope], task_id: &str, now_ms: i64) -> String {
    let phase = if task.iter().any(|e| e.kind == "council.started") { progress::progress(events, task_id, now_ms, 0, 1).phase } else { None };
    let ceiling = task.iter().find(|e| e.kind == "council.started").and_then(|e| e.payload["budget_tokens"].as_u64()).filter(|n| *n > 0);
    let spent = tokens(task);
    let k = |n: u64| if n >= 1000 { format!("{}k", n / 1000) } else { n.to_string() };
    let cost = match ceiling {
        Some(c) => format!("{} of {} tokens", k(spent), k(c)),
        None => format!("{} tokens", k(spent)),
    };
    format!("{} progress: {}; {cost}", progress::clock_of_ms(now_ms), phase.unwrap_or_else(|| "convening: choosing the seats".to_string()))
}

/// The record's path for the end line: the verdict's `missions/<id>/councils/<file>`, found in this mission's `councils/`.
fn record_path(dir: &Path, task: &[&EventEnvelope]) -> Option<PathBuf> {
    let rel = task.iter().find(|e| e.kind == "council.verdict")?.payload["path"].as_str()?.to_string();
    let file = rel.rsplit('/').next().filter(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))?;
    Some(dir.join("councils").join(file))
}

/// How the watch prints, reads and waits; injected so the loop is tested without a disk, a terminal or real sleeps.
pub struct Io<'a> {
    pub read: &'a mut dyn FnMut() -> Vec<EventEnvelope>,
    pub now_ms: &'a dyn Fn() -> i64,
    pub sleep: &'a mut dyn FnMut(u64),
    pub out: &'a mut dyn Write,
    pub color: bool,
}

/// Follow `found` until it ends (or once). Returns the exit code: 0.
pub fn watch(found: &Found, interval: u64, once: bool, io: &mut Io) -> i32 {
    let tid = found.task_id.as_str();
    let paint = |color: bool, code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    let mut seen = 0usize;
    let mut seats_shown = false;
    let mut first = true;
    let mut changed = false;
    loop {
        let events = (io.read)();
        let task = of_task(&events, tid);
        let mut lines: Vec<String> = Vec::new();
        if first {
            lines.push(paint(io.color, "1", &format!("Council {} in mission {} (venue {})", progress::role_name(tid), found.mission, venue(&task))));
        }
        if !seats_shown {
            match seats_line(&task) {
                Some(l) => {
                    lines.push(l);
                    seats_shown = true;
                }
                None if first => lines.push("seats: not chosen yet".to_string()),
                None => {}
            }
        }
        let fresh: Vec<String> = task.get(seen..).unwrap_or(&[]).iter().filter_map(|e| progress::tail_line(e)).collect();
        if first && fresh.len() > BACKLOG {
            lines.push(format!("({} earlier lines not shown)", fresh.len() - BACKLOG));
            lines.extend(fresh[fresh.len() - BACKLOG..].iter().cloned());
        } else {
            lines.extend(fresh);
        }
        changed |= first || task.len() > seen;
        seen = task.len();
        first = false;
        let st = state(&task);
        let end = ENDS.contains(&st);
        if !end && changed {
            lines.push(progress_line(&events, &task, tid, (io.now_ms)()));
            changed = false;
        }
        if end || once {
            let colour = match st {
                "decided" => "32",
                "needs_operator" | "not_decided" => "33",
                "failed" | "cancelled" => "31",
                _ => "0",
            };
            let record = record_path(&found.dir, &task).map(|p| format!("; record: {}", p.display())).unwrap_or_else(|| if end { "; no record was written".to_string() } else { String::new() });
            lines.push(format!("state: {}{record}", paint(io.color && colour != "0", colour, st)));
        }
        for l in &lines {
            let _ = writeln!(io.out, "{l}");
        }
        let _ = io.out.flush();
        if end || once {
            return 0;
        }
        (io.sleep)(interval);
    }
}

/// `delphi council watch [<council_id>] [--mission <id>] [--once] [--interval <secs>]`, for the workspace in `ws_dir`.
pub fn cli(ws_dir: &Path, args: &[String]) -> i32 {
    let value_flags = ["--mission", "--interval", "--home", "--workspace"];
    let positional: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !value_flags.contains(&args[*i - 1].as_str()))).map(|(_, a)| a).collect();
    if positional.first().map(|s| s.as_str()) != Some("watch") || positional.len() > 2 {
        eprintln!("usage: delphi council watch [<council_id>] [--mission <id>] [--once] [--interval <secs>] [--workspace <name>] [--home <dir>]");
        return 2;
    }
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let interval = match flag("--interval") {
        None => 10,
        Some(v) => match v.parse::<u64>() {
            Ok(n) => n.clamp(2, 300),
            Err(_) => {
                eprintln!("delphi council watch: --interval is a number of seconds (2 to 300)");
                return 2;
            }
        },
    };
    let found = match find(ws_dir, positional.get(1).map(|s| s.as_str()), flag("--mission").as_deref()) {
        Ok(f) => f,
        Err(why) => {
            eprintln!("delphi council watch: {why}");
            return 1;
        }
    };
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) && !std::env::var("TERM").is_ok_and(|t| t == "dumb");
    let dir = found.dir.clone();
    let mut read = move || delphi_core::log::replay(&dir).unwrap_or_default();
    let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let mut sleep = |s: u64| std::thread::sleep(std::time::Duration::from_secs(s));
    let mut stdout = std::io::stdout();
    watch(&found, interval, args.iter().any(|a| a == "--once"), &mut Io { read: &mut read, now_ms: &now, sleep: &mut sleep, out: &mut stdout, color })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const MARKER: &str = "SEAT-TEXT-MARKER-7f3a";

    fn ev(secs: i64, kind: &str, agent: Option<&str>, payload: Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("host-abc", kind, payload);
        e.timestamp = format!("2026-11-17T10:{:02}:{:02}.000Z", secs / 60, secs % 60);
        e.task_id = Some("t_0002".into());
        e.agent = agent.map(String::from);
        e
    }

    /// A daemon Council from a host, in the order its events are written; every seat text carries the marker.
    fn log() -> Vec<EventEnvelope> {
        let mut raised = EventEnvelope::new("host-abc", delphi_engine::host_rpc::COUNCIL_EVENT, json!({"task_id": "t_0002", "source": "claude-code"}));
        raised.timestamp = "2026-11-17T10:00:00.000Z".into();
        vec![
            raised,
            ev(1, "council.started", None, json!({"proposition": format!("q {MARKER}"), "seats": [{"role": "architect", "family": "claude", "tier": null}, {"role": "strategist", "family": "gemini", "tier": "frontier"}, {"role": "red-team", "family": "bad family!", "tier": "x"}], "homogeneous": false, "budget_usd": 20.0})),
            ev(5, "harness.tool", Some("architect"), json!({"tool": "read_file", "input": MARKER})),
            ev(9, "agent.usage", Some("architect"), json!({"input_tokens": 1500, "output_tokens": 500})),
            ev(30, "council.barrier.released", None, json!({"round": 1})),
            ev(31, "council.stance", None, json!({"round": 1, "role": "architect", "status": "answered", "text": MARKER})),
            ev(32, "council.stance", None, json!({"round": 1, "role": "red-team", "status": "abstained", "text": MARKER})),
            ev(50, "council.critique", None, json!({"role": "strategist", "readable": true, "kept": true, "text": MARKER, "findings": [{"title": MARKER}]})),
            ev(60, "council.verdict", None, json!({"decision": MARKER, "path": "missions/host-abc/councils/002-q.md", "options": [], "steps": []})),
            ev(61, "task.completed", None, json!({"status": "done", "reason": MARKER})),
        ]
    }

    fn run(log: Vec<EventEnvelope>, steps: Vec<usize>, once: bool) -> (String, Vec<u64>, i32) {
        // `steps[i]` is how many events of the log exist at the i-th read (the last count repeats).
        let found = Found { mission: "host-abc".into(), dir: PathBuf::from("/ws/local/sessions/host-abc"), task_id: "t_0002".into() };
        let mut reads = 0usize;
        let mut read = || {
            let n = steps[reads.min(steps.len() - 1)];
            reads += 1;
            log[..n].to_vec()
        };
        let now = || 1_763_373_700_000i64;
        let mut slept = Vec::new();
        let mut sleep = |s: u64| slept.push(s);
        let mut out = Vec::new();
        let code = watch(&found, 10, once, &mut Io { read: &mut read, now_ms: &now, sleep: &mut sleep, out: &mut out, color: false });
        (String::from_utf8(out).unwrap(), slept, code)
    }

    // specs/123 AC-6: --once prints the header, the tail and the state, and never seat text.
    #[test]
    fn once_prints_the_header_the_tail_and_the_state_and_no_seat_text() {
        let (out, slept, code) = run(log(), vec![7], true);
        assert_eq!(code, 0);
        assert!(slept.is_empty(), "--once never waits");
        assert!(out.starts_with("Council t_0002 in mission host-abc (venue daemon)\n"), "{out}");
        assert!(out.contains("seats: architect (claude, default tier), strategist (gemini, frontier), red-team (other, default tier); ceiling $20.00 (council_budget_usd)"), "{out}");
        assert!(out.contains("10:00:05 architect tool read_file\n10:00:31 architect stance answered\n10:00:32 red-team stance abstained\n"), "{out}");
        assert!(out.contains("progress: round 2:") && out.contains("; 2k tokens\n"), "{out}");
        assert!(out.ends_with("state: deliberating\n"), "{out}");
        assert!(!out.contains(MARKER), "seat text never appears: {out}");
    }

    // specs/123 AC-6: on a live log it prints new lines as they come, a progress line only when something changed, and exits 0 at the end.
    #[test]
    fn a_live_watch_prints_new_lines_and_exits_0_when_the_council_ends() {
        let (out, slept, code) = run(log(), vec![2, 4, 4, 8, 10], false);
        assert_eq!(code, 0);
        assert_eq!(slept, vec![10, 10, 10, 10], "one wait between reads, none after the end");
        assert_eq!(out.matches("progress:").count(), 3, "a read with nothing new prints no progress line: {out}");
        assert_eq!(out.matches("seats:").count(), 1);
        assert!(out.contains("10:00:50 strategist critique kept"), "{out}");
        assert!(out.ends_with("state: decided; record: /ws/local/sessions/host-abc/councils/002-q.md\n"), "{out}");
        assert!(!out.contains(MARKER), "seat text never appears: {out}");
        assert!(!out.contains('\r') && !out.contains('\x1b'), "plain lines, nothing redrawn, no colour when not asked");
    }

    #[test]
    fn the_end_states_are_derived_as_the_status_call_derives_them() {
        let l = log();
        let t = |evs: &[EventEnvelope]| state(&of_task(evs, "t_0002")).to_string();
        assert_eq!(t(&l[..1]), "convening");
        assert_eq!(t(&l[..3]), "deliberating");
        assert_eq!(t(&l), "decided");
        let mut esc = l.clone();
        esc[9].payload["status"] = json!("escalated");
        assert_eq!(t(&esc), "needs_operator");
        let mut nd = l[..8].to_vec();
        nd.push(ev(70, "task.completed", None, json!({"status": "escalated"})));
        assert_eq!(t(&nd), "not_decided");
        let mut failed = l[..3].to_vec();
        failed.push(ev(70, "task.failed", None, json!({"error": MARKER})));
        assert_eq!(t(&failed), "failed");
        let (out, _, _) = run(failed, vec![4], false);
        assert!(out.ends_with("state: failed; no record was written\n") && !out.contains(MARKER), "{out}");
    }

    #[test]
    fn find_picks_the_running_council_across_missions_and_narrows_by_id_and_mission() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path();
        let write = |mission: &str, evs: &[EventEnvelope]| {
            let dir = delphi_core::sessions::roots(ws)[0].join(mission);
            let mut w = delphi_core::log::EventLog::open(&dir, Default::default()).unwrap();
            // A usage event needs span fields the log checks; `find` does not read it.
            for e in evs.iter().filter(|e| e.kind != "agent.usage") {
                w.append(e.clone()).unwrap();
            }
        };
        write("host-old", &log());
        write("m-live", &log()[..4]);
        assert_eq!(find(ws, None, None).unwrap().mission, "m-live", "the one still running");
        assert_eq!(find(ws, None, Some("host-old")).unwrap().mission, "host-old");
        assert_eq!(find(ws, Some("t_0002"), Some("host-old")).unwrap().task_id, "t_0002");
        assert!(find(ws, Some("t_9999"), None).unwrap_err().contains("no Council `t_9999`"));
        assert!(find(&ws.join("empty"), None, None).is_err());
    }
}
