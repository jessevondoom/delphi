//! `delphi timers`: see, set and cancel the wakes the daemon is keeping (`specs/017`). A timer is a promise that a role
//! will be woken; this is where the operator sees every one of them, who made it, and whether it fired.

use crate::decisions::Rpc;
use delphi_core::client::ClientError;
use serde_json::{json, Value};
use std::path::Path;

const USAGE: &str = "usage:
  delphi timers [list]
  delphi timers set <role> (--in <5m|2h|90s> | --at <RFC 3339 time>) [--every <5m>] <note…>
  delphi timers cancel <id>";

fn err_text(e: &ClientError) -> String {
    match e {
        ClientError::Rpc { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

fn short(s: &str, n: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > n { format!("{}…", one.chars().take(n - 1).collect::<String>()) } else { one }
}

/// The timers as a short table: what is pending first, then what recently fired or was cancelled.
pub fn render(v: &Value) -> String {
    let all = v["timers"].as_array().cloned().unwrap_or_default();
    let now = v["now"].as_str().unwrap_or("?");
    let pending: Vec<&Value> = all.iter().filter(|t| t["state"] == "pending").collect();
    let mut out = format!("{} pending timer(s) · now {}\n", pending.len(), now);
    for t in &pending {
        let every = t["every_secs"].as_i64().map(|e| format!(" every {}s", e)).unwrap_or_default();
        out.push_str(&format!("  {:<8} {:<13} {:<11} {}{}  by {}: {}\n", t["id"].as_str().unwrap_or("?"), t["target"].as_str().unwrap_or("?"), t["relative"].as_str().unwrap_or("?"), t["due_at"].as_str().unwrap_or("?"), every, t["creator"].as_str().unwrap_or("?"), short(t["note"].as_str().unwrap_or(""), 70)));
    }
    let done: Vec<&Value> = all.iter().filter(|t| t["state"] != "pending").take(8).collect();
    if !done.is_empty() {
        out.push_str("recent:\n");
        for t in done {
            let how = match t["state"].as_str() {
                Some("cancelled") => format!("cancelled by {}", t["cancelled_by"].as_str().unwrap_or("?")),
                _ => format!("fired {}×", t["fires"]),
            };
            out.push_str(&format!("  {:<8} {:<13} {:<24} {}\n", t["id"].as_str().unwrap_or("?"), t["target"].as_str().unwrap_or("?"), how, short(t["note"].as_str().unwrap_or(""), 60)));
        }
    }
    out
}

/// Run one `delphi timers` command and return what to print.
pub fn handle(rpc: &mut dyn Rpc, mission: &str, args: &[String]) -> String {
    match args.first().map(String::as_str) {
        None | Some("list") => match rpc.call("timer.list", json!({})) {
            Ok(v) => render(&v),
            Err(e) => format!("{}\n", err_text(&e)),
        },
        Some("cancel") => match args.get(1) {
            None => format!("{}\n", USAGE),
            Some(id) => match rpc.call("timer.cancel", json!({"id": id})) {
                Ok(_) => format!("{}: cancelled\n", id),
                Err(e) => format!("{}\n", err_text(&e)),
            },
        },
        Some("set") => match parse_set(&args[1..]) {
            Err(m) => format!("{}\n{}\n", m, USAGE),
            Ok(mut p) => {
                p["mission_id"] = json!(mission);
                match rpc.call("timer.set", p) {
                    Ok(t) => format!("{} set: wakes {} at {}\n", t["id"].as_str().unwrap_or("?"), t["target"].as_str().unwrap_or("?"), t["due_at"].as_str().unwrap_or("?")),
                    Err(e) => format!("{}\n", err_text(&e)),
                }
            }
        },
        Some(other) => format!("unknown timers command `{}`\n{}\n", other, USAGE),
    }
}

/// The words after `/timer` in a chat, as `delphi timers` arguments: `/timer 20m check CI` is
/// `set orchestrator --in 20m check CI`, `/timer @architect at 2026-10-26T09:00:00Z review` names the role and the time.
pub fn chat_args(words: &[String]) -> Vec<String> {
    if matches!(words.first().map(String::as_str), None | Some("list") | Some("cancel")) {
        return words.to_vec();
    }
    let (role, rest) = match words.first() {
        Some(w) if w.starts_with('@') => (w[1..].to_string(), &words[1..]),
        _ => ("orchestrator".to_string(), words),
    };
    let mut out = vec!["set".to_string(), role];
    match rest.first().map(String::as_str) {
        Some(flag @ ("at" | "in")) => {
            out.push(format!("--{}", flag));
            out.extend(rest[1..].iter().cloned());
        }
        Some(span) => {
            out.push("--in".into());
            out.push(span.to_string());
            out.extend(rest[1..].iter().cloned());
        }
        None => {}
    }
    out
}

fn parse_set(args: &[String]) -> Result<Value, String> {
    let (mut role, mut note, mut p) = (None, Vec::new(), json!({}));
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| it.next().cloned().ok_or_else(|| format!("{} needs a value", flag));
        match a.as_str() {
            "--in" => p["in_secs"] = json!(crate::run::parse_duration(&value("--in")?)?.as_secs()),
            "--every" => p["every_secs"] = json!(crate::run::parse_duration(&value("--every")?)?.as_secs()),
            "--at" => p["at"] = json!(value("--at")?),
            _ if role.is_none() => role = Some(a.clone()),
            _ => note.push(a.clone()),
        }
    }
    p["target"] = json!(role.ok_or("which role should be woken?")?);
    if note.is_empty() {
        return Err("a note is required: say why the role is being woken".into());
    }
    p["note"] = json!(note.join(" "));
    if p.get("in_secs").is_none() && p.get("at").is_none() {
        return Err("give --in or --at".into());
    }
    Ok(p)
}

pub fn cli(home: &Path, ws_dir: &Path, mission: Option<String>, args: &[String]) -> i32 {
    use delphi_core::spawn::{connect_or_spawn, SpawnOptions};
    let Some(exe) = crate::chat::daemon_exe() else {
        eprintln!("delphi timers: cannot locate this program to start the daemon");
        return 4;
    };
    let mut attached = match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()], own_version: Some(env!("CARGO_PKG_VERSION")) }) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi timers: {}", err_text(&e));
            return 4;
        }
    };
    let mission_id = match attached.client.call("mission.open", json!({"mission_id": mission})) {
        Ok(v) => v["mission_id"].as_str().unwrap_or("").to_string(),
        Err(e) => {
            eprintln!("delphi timers: {}", err_text(&e));
            return 4;
        }
    };
    print!("{}", handle(&mut attached.client, &mission_id, args));
    // A timer only fires while a daemon runs. One this command started would stop with it, so say so.
    if attached.spawned {
        let pending = attached.client.call("timer.list", json!({})).map(|v| v["timers"].as_array().map(|a| a.iter().filter(|t| t["state"] == "pending").count()).unwrap_or(0)).unwrap_or(0);
        if pending > 0 {
            eprintln!("note: the daemon was started for this command and is stopping; timers fire only while a daemon is running (`delphi` or `delphi daemon status`). Late ones fire when it next starts.");
        }
        let _ = attached.client.call("daemon.shutdown", json!({}));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        calls: Vec<(String, Value)>,
    }

    impl Rpc for Fake {
        fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
            self.calls.push((method.into(), params.clone()));
            match method {
                "timer.list" => Ok(json!({"now": "2027-01-15T08:00:00Z", "timers": [
                    {"id": "tm_0002", "target": "orchestrator", "state": "pending", "relative": "in 4m 10s", "due_at": "2027-01-15T08:04:10Z", "creator": "orchestrator", "note": "check\nCI", "every_secs": 600},
                    {"id": "tm_0001", "target": "orchestrator", "state": "fired", "fires": 1, "creator": "operator", "note": "first"},
                    {"id": "tm_0003", "target": "builder", "state": "cancelled", "cancelled_by": "operator", "creator": "builder", "note": "never mind"}]})),
                "timer.set" => Ok(json!({"id": "tm_0009", "target": params["target"], "due_at": "2027-01-15T08:10:00Z"})),
                "timer.cancel" if params["id"] == "tm_0009" => Ok(json!({})),
                "timer.cancel" => Err(ClientError::Rpc { code: -32004, message: format!("no timer `{}`", params["id"].as_str().unwrap()), data: None }),
                _ => Err(ClientError::Closed),
            }
        }
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_list_shows_what_is_pending_with_its_wake_time_and_owner_then_what_happened_to_the_rest() {
        let out = handle(&mut Fake { calls: vec![] }, "m1", &[]);
        assert!(out.starts_with("1 pending timer(s) · now 2027-01-15T08:00:00Z"), "{}", out);
        assert!(out.contains("tm_0002") && out.contains("in 4m 10s") && out.contains("every 600s") && out.contains("by orchestrator: check CI"), "{}", out);
        assert!(out.contains("fired 1×") && out.contains("cancelled by operator"), "{}", out);
    }

    #[test]
    fn setting_a_timer_sends_the_role_the_time_and_the_whole_note() {
        let mut f = Fake { calls: vec![] };
        let out = handle(&mut f, "m1", &args(&["set", "orchestrator", "--in", "10m", "--every", "1h", "check", "the", "CI"]));
        assert_eq!(out, "tm_0009 set: wakes orchestrator at 2027-01-15T08:10:00Z\n");
        assert_eq!(f.calls[0].1, json!({"target": "orchestrator", "in_secs": 600, "every_secs": 3600, "note": "check the CI", "mission_id": "m1"}));
        let mut f = Fake { calls: vec![] };
        handle(&mut f, "m1", &args(&["set", "builder", "retry", "--at", "2027-02-01T09:00:00Z"]));
        assert_eq!(f.calls[0].1["at"], "2027-02-01T09:00:00Z");
    }

    #[test]
    fn an_incomplete_set_says_what_is_missing_and_never_calls_the_daemon() {
        for (a, why) in [(vec!["set"], "which role"), (vec!["set", "orchestrator", "--in", "5m"], "a note is required"), (vec!["set", "orchestrator", "note"], "give --in or --at"), (vec!["set", "orchestrator", "--in", "soon", "x"], "not a duration"), (vec!["set", "orchestrator", "x", "--in"], "needs a value")] {
            let mut f = Fake { calls: vec![] };
            let out = handle(&mut f, "m1", &args(&a));
            assert!(out.contains(why) && out.contains("usage:") && f.calls.is_empty(), "{:?}: {}", a, out);
        }
    }

    #[test]
    fn chat_words_become_timers_arguments() {
        let w = |v: &[&str]| chat_args(&args(v));
        assert_eq!(w(&[]), Vec::<String>::new());
        assert_eq!(w(&["list"]), args(&["list"]));
        assert_eq!(w(&["cancel", "tm_0001"]), args(&["cancel", "tm_0001"]));
        assert_eq!(w(&["20m", "check", "CI"]), args(&["set", "orchestrator", "--in", "20m", "check", "CI"]));
        assert_eq!(w(&["in", "2h", "check"]), args(&["set", "orchestrator", "--in", "2h", "check"]));
        assert_eq!(w(&["@architect", "1d", "review", "--every", "1d"]), args(&["set", "architect", "--in", "1d", "review", "--every", "1d"]));
        assert_eq!(w(&["at", "2026-10-26T09:00:00Z", "standup"]), args(&["set", "orchestrator", "--at", "2026-10-26T09:00:00Z", "standup"]));
        // And the result is something `set` accepts.
        let mut f = Fake { calls: vec![] };
        handle(&mut f, "m1", &w(&["@builder", "90s", "retry", "the", "build"]));
        assert_eq!(f.calls[0].1, json!({"target": "builder", "in_secs": 90, "note": "retry the build", "mission_id": "m1"}));
    }

    #[test]
    fn cancelling_reports_success_or_the_daemons_reason() {
        assert_eq!(handle(&mut Fake { calls: vec![] }, "m1", &args(&["cancel", "tm_0009"])), "tm_0009: cancelled\n");
        assert_eq!(handle(&mut Fake { calls: vec![] }, "m1", &args(&["cancel", "tm_1"])), "no timer `tm_1`\n");
        assert!(handle(&mut Fake { calls: vec![] }, "m1", &args(&["cancel"])).starts_with("usage:"));
        assert!(handle(&mut Fake { calls: vec![] }, "m1", &args(&["wat"])).contains("unknown timers command"));
    }
}
