//! `delphi status`: where things stand, in a few lines and without a terminal (`specs/005e` finding 6): the workspace,
//! the daemon, the models, the health that `doctor` reports, and the mission. The daemon is read through the narrator
//! socket, whose scope is read-only and needs no terminal.

use crate::report::Level;
use crate::{doctor, home, missions};
use delphi_core::client::Client;
use delphi_core::proto::Scope;
use delphi_core::spawn::Layout;
use serde_json::json;
use std::path::Path;
use std::time::SystemTime;

/// What a running daemon says about itself: release, mission, tasks running and approvals waiting. `None` when no
/// daemon answers on the narrator socket.
pub struct DaemonInfo {
    pub version: String,
    pub mission: String,
    pub running: usize,
    pub waiting: usize,
}

pub fn daemon_info(l: &Layout) -> Option<DaemonInfo> {
    let mut c = Client::connect(&l.narrator_sock).ok()?;
    c.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
    let token = c.issue_token(Scope::Narrator).ok()?;
    let hello = c.hello(&token).ok()?;
    let snap = c.call("session.snapshot", json!({})).ok()?;
    let count = |k: &str| snap[k].as_array().map(|a| a.len()).unwrap_or(0);
    Some(DaemonInfo { version: hello["binary_version"].as_str().unwrap_or("?").to_string(), mission: snap["mission_id"].as_str().unwrap_or("?").to_string(), running: count("running_tasks"), waiting: count("pending_approvals") })
}

/// The daemon line, shared with `delphi daemon status`.
pub fn daemon_line(info: Option<&DaemonInfo>) -> String {
    match info {
        None => "not running (`delphi` starts it)".to_string(),
        Some(d) => {
            let new = if d.version != env!("CARGO_PKG_VERSION") { format!(" (this program is {}: `delphi daemon restart` runs the new one)", env!("CARGO_PKG_VERSION")) } else { String::new() };
            format!("running · delphi {}{} · mission {} · {} task(s) running · {} approval(s) waiting", d.version, new, d.mission, d.running, d.waiting)
        }
    }
}

fn tilde(p: &Path) -> String {
    match std::env::var_os("HOME").map(std::path::PathBuf::from).and_then(|h| p.strip_prefix(&h).ok().map(|r| r.to_path_buf())) {
        Some(r) => format!("~/{}", r.display()),
        None => p.display().to_string(),
    }
}

/// The report: lines of `label  value`, and the exit code (1 only when `doctor` finds a failure).
pub fn report(home_dir: &Path, ws: &home::Workspace, daemon: Option<&DaemonInfo>, now: SystemTime) -> (String, i32) {
    let health = doctor::run(home_dir, ws, &doctor::Options::default());
    let worst = health.worst();
    let models = health
        .findings
        .iter()
        .find(|f| f.check.starts_with("11 model pool"))
        .map(|f| if f.level == Level::Skipped { "none configured: run `delphi setup`".to_string() } else { f.message.clone() })
        .unwrap_or_else(|| "unknown".into());
    let rows = missions::list(&ws.dir);
    let mut out = String::new();
    let mut line = |label: &str, value: String| out.push_str(&format!("{:<10} {}\n", label, value));
    line("workspace", format!("{} ({})", ws.name, tilde(&ws.dir)));
    line("daemon", daemon_line(daemon));
    line("models", models);
    let (warns, fails) = (health.count(Level::Warn), health.count(Level::Fail));
    line("health", if worst == Level::Fail || worst == Level::Warn { format!("{} fail, {} warn: `delphi doctor` lists them", fails, warns) } else { "ok".to_string() });
    line("mission", match rows.first() {
        Some(r) => format!("{} ({}, {} in all: `delphi missions`)", daemon.map(|d| d.mission.as_str()).unwrap_or(&r.id), missions::ago(r.last, now), rows.len()),
        None => "none yet".into(),
    });
    (out, if worst == Level::Fail { 1 } else { 0 })
}

/// `delphi status`.
pub fn cli(home_dir: &Path, ws: &home::Workspace) -> i32 {
    let info = delphi_core::spawn::layout(home_dir, &ws.dir).ok().and_then(|l| daemon_info(&l));
    let (text, code) = report(home_dir, ws, info.as_ref(), SystemTime::now());
    print!("{}", text);
    code
}

