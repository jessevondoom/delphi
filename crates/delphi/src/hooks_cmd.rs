//! `delphi hooks`: run the operator's observe hooks (`specs/018`).
//!
//! A separate process on the read-only Narrator socket, started in a terminal or a `tmux` pane like `delphi relay`,
//! so a slow or broken hook cannot slow the daemon and the process can only watch. For each event whose kind a hook
//! asked for, the hook's command runs with the event on standard input; its output is ignored and a failure is
//! printed here and nowhere else. The commands come from the **ratified** `[hooks]` block.

use delphi_core::client::{Client, ClientError};
use delphi_core::proto::{code, Scope};
use delphi_engine::hooks::{self, ObserveHook};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// At most this many hook runs wait their turn; past it, the oldest news is dropped, not the daemon's attention.
const QUEUE: usize = 100;
const WORKERS: usize = 4;

/// One run to make: the hook, what to put on its standard input, and its environment.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub hook: ObserveHook,
    pub stdin: String,
    pub env: Vec<(String, String)>,
}

/// The runs an event asks for.
pub fn jobs_for(hooks: &[ObserveHook], event: &Value) -> Vec<Job> {
    let kind = event["type"].as_str().unwrap_or("");
    let env = vec![
        ("DELPHI_HOOK".to_string(), "observe".to_string()),
        ("DELPHI_EVENT".to_string(), kind.to_string()),
        ("DELPHI_MISSION".to_string(), event["mission_id"].as_str().unwrap_or("").to_string()),
        ("DELPHI_TASK".to_string(), event["task_id"].as_str().unwrap_or("").to_string()),
    ];
    hooks.iter().filter(|h| hooks::matches_event(&h.events, kind)).map(|h| Job { hook: h.clone(), stdin: event.to_string(), env: env.clone() }).collect()
}

/// Run one job. `Err` carries what to print: an observe hook's failure is the operator's to see, nobody else's.
pub fn execute(job: &Job, cwd: &Path) -> Result<(), String> {
    let env: Vec<(&str, &str)> = job.env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let run = hooks::run(&job.hook.command, &job.stdin, job.hook.timeout, &env, cwd);
    if run.ok() { Ok(()) } else { Err(format!("hook `{}` on {}: {}", job.hook.name, job.env[1].1, run.reason())) }
}

fn attach(sock: &Path) -> Result<(Client, mpsc::Receiver<Option<Value>>), String> {
    let mut c = Client::connect(sock).map_err(|e| format!("no daemon is listening ({}); start `delphi` for this workspace first", e))?;
    let tok = c.issue_token(Scope::Narrator).map_err(|e| match e {
        ClientError::Rpc { code: k, .. } if k == code::TTY_REQUIRED => "the daemon only gives a token to a process attached to a terminal: run `delphi hooks` in a terminal or a tmux pane".to_string(),
        other => other.to_string(),
    })?;
    c.hello(&tok).map_err(|e| e.to_string())?;
    let mut ev = Client::connect(sock).map_err(|e| e.to_string())?;
    ev.hello(&tok).map_err(|e| e.to_string())?;
    ev.call("events.subscribe", json!({})).map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::channel();
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
    Ok((c, rx))
}

pub fn run(home: &Path, ws_dir: &Path) -> i32 {
    let live = match std::fs::read_to_string(ws_dir.join("config.toml")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi hooks: cannot read config.toml: {}", e);
            return 2;
        }
    };
    let text = match delphi_engine::boot::effective_config(ws_dir, &live) {
        Ok((t, _)) => t,
        Err(e) => {
            eprintln!("delphi hooks: {}", e);
            return 2;
        }
    };
    let (hooks, problems) = hooks::parse(&toml::from_str::<toml::Value>(&text).unwrap_or(toml::Value::Table(Default::default())));
    for p in problems {
        eprintln!("delphi hooks: {}", p);
    }
    if hooks.observe.is_empty() {
        eprintln!("delphi hooks: there are no observe hooks in the ratified [hooks] config. Add [[hooks.observe]] entries to config.toml, then /ratify them (see specs/018).");
        return 2;
    }
    let l = match delphi_core::spawn::layout(home, ws_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("delphi hooks: {}", e);
            return 2;
        }
    };
    // A small pool: runs happen in parallel but not without limit.
    let (tx, rx) = mpsc::sync_channel::<Job>(QUEUE);
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
    for _ in 0..WORKERS {
        let (rx, cwd) = (rx.clone(), ws_dir.to_path_buf());
        std::thread::spawn(move || loop {
            let Ok(job) = rx.lock().unwrap().recv() else { return };
            if let Err(e) = execute(&job, &cwd) {
                eprintln!("delphi hooks: {}", e);
            }
        });
    }
    println!("delphi hooks: ready ({} hook(s): {})", hooks.observe.len(), hooks.observe.iter().map(|h| h.name.as_str()).collect::<Vec<_>>().join(", "));
    let mut backoff = Duration::from_secs(2);
    let mut said = false;
    loop {
        let (_cmd, events) = match attach(&l.narrator_sock) {
            Ok(v) => {
                backoff = Duration::from_secs(2);
                said = false;
                v
            }
            Err(m) => {
                if !said {
                    eprintln!("delphi hooks: {}; trying again", m);
                    said = true;
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
                continue;
            }
        };
        while let Ok(Some(e)) = events.recv() {
            for job in jobs_for(&hooks.observe, &e) {
                if tx.try_send(job).is_err() {
                    eprintln!("delphi hooks: too many runs waiting; dropped one for {}", e["type"].as_str().unwrap_or("?"));
                }
            }
        }
        eprintln!("delphi hooks: lost the daemon; attaching again");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(name: &str, events: &[&str], script: &str) -> ObserveHook {
        ObserveHook { name: name.into(), events: events.iter().map(|s| s.to_string()).collect(), command: vec!["/bin/sh".into(), "-c".into(), script.into()], timeout: Duration::from_secs(10) }
    }

    #[test]
    fn an_event_asks_for_exactly_the_hooks_that_named_its_kind() {
        let hs = vec![hook("done", &["task.completed"], "true"), hook("cards", &["approval.*"], "true"), hook("all", &["*"], "true")];
        let ev = json!({"type": "task.completed", "mission_id": "m1", "task_id": "t_0004", "payload": {"status": "done"}});
        let jobs = jobs_for(&hs, &ev);
        assert_eq!(jobs.iter().map(|j| j.hook.name.as_str()).collect::<Vec<_>>(), vec!["done", "all"]);
        assert_eq!(jobs[0].stdin, ev.to_string());
        assert!(jobs[0].env.contains(&("DELPHI_EVENT".into(), "task.completed".into())) && jobs[0].env.contains(&("DELPHI_TASK".into(), "t_0004".into())) && jobs[0].env.contains(&("DELPHI_HOOK".into(), "observe".into())));
        assert_eq!(jobs_for(&hs, &json!({"type": "approval.requested"})).len(), 2);
        assert!(jobs_for(&hs, &json!({"payload": {}})).iter().all(|j| j.hook.name == "all"), "an event with no kind matches only `*`");
    }

    #[test]
    fn a_run_gets_the_event_and_the_environment_and_its_failure_is_reported_not_raised() {
        let t = tempfile::tempdir().unwrap();
        let out = t.path().join("out.txt");
        let h = hook("log", &["task.completed"], &format!("cat > {o}.json; echo \"$DELPHI_EVENT $DELPHI_TASK\" > {o}", o = out.display()));
        let job = jobs_for(&[h], &json!({"type": "task.completed", "task_id": "t_0009", "payload": {"status": "done"}})).remove(0);
        assert_eq!(execute(&job, t.path()), Ok(()));
        assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), "task.completed t_0009");
        assert!(std::fs::read_to_string(format!("{}.json", out.display())).unwrap().contains("\"status\":\"done\""));
        let bad = jobs_for(&[hook("broken", &["*"], "echo 'it broke'; exit 2")], &json!({"type": "task.failed"})).remove(0);
        let e = execute(&bad, t.path()).unwrap_err();
        assert!(e.contains("broken") && e.contains("task.failed") && e.contains("it broke"), "{}", e);
        let slow = ObserveHook { timeout: Duration::from_millis(150), ..hook("slow", &["*"], "sleep 30") };
        assert!(execute(&jobs_for(&[slow], &json!({"type": "x"})).remove(0), t.path()).unwrap_err().contains("in time"));
    }
}
