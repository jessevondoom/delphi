//! The one thing `delphi mcp` does beyond reading: ask the running daemon to stage a proposal or keep a handoff (`specs/062` §8).
//!
//! * The socket is the daemon's host socket (`<hash>.host.sock`), owned by the same user; its UID check is the authentication.
//! * Nothing here starts a daemon. No daemon means `daemon_not_running`, and the host is told to use pack mode.
//! * The process, never the model, supplies `session`, `tool` and `cwd`. The daemon decides scope, tier and role rules.

use delphi_core::client::{Client, ClientError};
use delphi_core::proto::Scope;
use serde_json::{json, Value};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct HostLink {
    sock: PathBuf,
    session: String,
    tool: String,
    cwd: PathBuf,
}

/// 16 lowercase hex characters, unpredictable per process (`RandomState` is seeded by the OS).
fn new_session() -> String {
    let mut h = RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}

impl HostLink {
    /// `ws` is the workspace directory (`<home>/workspaces/<name>`).
    pub fn for_workspace(ws: &Path, cwd: &Path) -> Option<HostLink> {
        let home = ws.parent()?.parent()?;
        let l = delphi_core::spawn::layout(home, ws).ok()?;
        Some(HostLink::at(l.host_sock, cwd))
    }

    pub fn at(sock: PathBuf, cwd: &Path) -> HostLink {
        HostLink { sock, session: new_session(), tool: "unknown".into(), cwd: cwd.to_path_buf() }
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    /// The host's own name from `initialize` `clientInfo.name`: lowercase letters, digits, `-`, `_`, `.`, at most 32; else `unknown`.
    pub fn set_tool(&mut self, name: Option<&str>) {
        self.tool = match name.map(str::trim) {
            Some(n) if !n.is_empty() && n.len() <= 32 && n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) => n.to_ascii_lowercase(),
            _ => "unknown".into(),
        };
    }

    pub fn set_cwd(&mut self, cwd: &Path) {
        self.cwd = cwd.to_path_buf();
    }

    /// Call one `host.*` method. `args` is what the model gave; the caller's identity fields are overwritten here.
    pub fn call(&self, method: &str, args: &Value) -> Result<Value, String> {
        self.call_within(method, args, Duration::from_secs(20))
    }

    /// As [`HostLink::call`], for a method that may take a while (a help request is answered by an agent).
    pub fn call_within(&self, method: &str, args: &Value, wait: Duration) -> Result<Value, String> {
        let mut c = Client::connect(&self.sock).map_err(|_| "daemon_not_running: Delphi's daemon is not running for this workspace, so nothing was recorded. Start `delphi` in a terminal, or keep working in pack mode (roles, specs and memory still read)".to_string())?;
        c.set_read_timeout(Some(wait)).map_err(|e| e.to_string())?;
        let token = c.issue_token(Scope::Host).map_err(|e| e.to_string())?;
        c.hello(&token).map_err(|e| e.to_string())?;
        let mut p = args.as_object().cloned().unwrap_or_default();
        p.insert("session".into(), json!(self.session));
        p.insert("tool".into(), json!(self.tool));
        p.insert("cwd".into(), json!(self.cwd.to_string_lossy()));
        c.call(method, Value::Object(p)).map_err(|e| match e {
            ClientError::Rpc { message, .. } => message,
            other => other.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_is_sixteen_hex_and_differs_per_link_and_the_tool_name_is_sanitized() {
        let a = HostLink::at(PathBuf::from("/nonexistent"), Path::new("/x"));
        let b = HostLink::at(PathBuf::from("/nonexistent"), Path::new("/x"));
        assert!(a.session().len() == 16 && a.session().chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.session(), b.session());
        let mut a = a;
        a.set_tool(Some("Claude-Code"));
        assert_eq!(a.tool, "claude-code");
        a.set_tool(Some("bad name; rm"));
        assert_eq!(a.tool, "unknown");
    }

    #[test]
    fn no_daemon_is_an_error_that_says_so_and_nothing_is_started() {
        let a = HostLink::at(PathBuf::from("/nonexistent/h.sock"), Path::new("/x"));
        let e = a.call("host.handoff_get", &json!({})).unwrap_err();
        assert!(e.starts_with("daemon_not_running"), "{e}");
    }
}
