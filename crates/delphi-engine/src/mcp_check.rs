//! Spec 071 E2: is the Delphi plugin set up in Claude Code? A read-only, offline look at the two files Claude Code keeps
//! (`~/.claude/plugins/installed_plugins.json`, schema `version: 2`, and `~/.claude/settings.json`, key `enabledPlugins`),
//! recorded on Claude Code 2.1.314. It never runs the binary, never follows a symlink, and never echoes a value from a file.

use std::io::Read;
use std::path::Path;

/// The Claude Code release whose files fixed the schema below (spec 071 Q-8).
pub const CLAUDE_CODE_VERSION: &str = "2.1.314";
const PLUGIN_KEY: &str = "delphi@delphi";
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The expected plugin entry is installed and enabled at this release. Says nothing about whether the binary can be launched.
    Configured,
    /// Not installed, or installed but disabled.
    Missing,
    /// Installed at an older release than this binary.
    Stale,
    /// Installed at a newer release than this binary supports.
    Incompatible,
    /// The files are absent, unreadable or not in the recorded schema.
    Unknown,
    /// No check exists for this host.
    Unverified,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Configured => "configured",
            State::Missing => "missing",
            State::Stale => "stale",
            State::Incompatible => "incompatible",
            State::Unknown => "unknown",
            State::Unverified => "unverified",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub state: State,
    pub detail: String,
    /// The command that cures the state, printed for the operator and never run.
    pub fix: Option<String>,
}

/// Launchability is never established here: it needs the stable executable path of spec 071 E1s, and is never read from this process's `PATH`.
pub const LAUNCHABLE: &str = "unknown (the binary is not executed and the host's own PATH cannot be observed)";

const FIX_INSTALL: &str = "claude plugin marketplace add jessevondoom/delphi && claude plugin install delphi@delphi";

/// Read a small regular file the user owns, without following a symlink or blocking on a FIFO.
fn safe_read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let f = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("cannot be opened (a symlink, or no permission)".into()),
    };
    let m = f.metadata().map_err(|_| "cannot be inspected".to_string())?;
    // SAFETY: geteuid has no preconditions.
    if !m.is_file() || m.uid() != unsafe { libc::geteuid() } {
        return Err("is not a regular file owned by you".into());
    }
    if m.len() > MAX_BYTES {
        return Err("is larger than 1 MiB".into());
    }
    let mut buf = Vec::new();
    f.take(MAX_BYTES + 1).read_to_end(&mut buf).map_err(|_| "cannot be read".to_string())?;
    Ok(Some(buf))
}

fn unknown(why: impl Into<String>) -> Check {
    Check { state: State::Unknown, detail: why.into(), fix: None }
}

fn parse_release(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let t = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if it.next().is_some() {
        return None;
    }
    Some(t)
}

/// Check Claude Code's plugin state under `home` (the user's home directory) against this binary's release.
pub fn check_claude_code(home: &Path, binary_release: &str) -> Check {
    let dir = home.join(".claude");
    let installed = match safe_read(&dir.join("plugins/installed_plugins.json")) {
        Ok(Some(b)) => b,
        Ok(None) => return unknown("no Claude Code plugin state was found, so this cannot be checked"),
        Err(e) => return unknown(format!("installed_plugins.json {e}")),
    };
    let Ok(doc) = serde_json::from_slice::<serde_json::Value>(&installed) else { return unknown("installed_plugins.json is not valid JSON") };
    if doc["version"] != 2 || !doc["plugins"].is_object() {
        return unknown(format!("installed_plugins.json is not the schema checked on Claude Code {CLAUDE_CODE_VERSION}"));
    }
    let Some(entries) = doc["plugins"].get(PLUGIN_KEY).and_then(|e| e.as_array()).filter(|e| !e.is_empty()) else {
        return Check { state: State::Missing, detail: "the Delphi plugin is not installed".into(), fix: Some(FIX_INSTALL.into()) };
    };
    let Some(entry) = entries.iter().find(|e| e["scope"] == "user") else {
        return unknown("the Delphi plugin is installed only for a project or local scope, which is not checked");
    };
    let Some(plugin_release) = entry["version"].as_str().and_then(parse_release) else {
        return unknown("the installed plugin has no readable release");
    };
    let settings = match safe_read(&dir.join("settings.json")) {
        Ok(Some(b)) => b,
        Ok(None) => return unknown("Claude Code settings.json was not found, so whether the plugin is enabled cannot be checked"),
        Err(e) => return unknown(format!("settings.json {e}")),
    };
    let Ok(settings) = serde_json::from_slice::<serde_json::Value>(&settings) else { return unknown("settings.json is not valid JSON") };
    let release = show(plugin_release);
    match settings["enabledPlugins"].get(PLUGIN_KEY).and_then(|v| v.as_bool()) {
        Some(true) => {}
        Some(false) => return Check { state: State::Missing, detail: format!("the Delphi plugin {release} is installed but disabled"), fix: Some(format!("claude plugin enable {PLUGIN_KEY}")) },
        None => return Check { state: State::Missing, detail: format!("the Delphi plugin {release} is installed but not enabled in your settings"), fix: Some(format!("claude plugin enable {PLUGIN_KEY}")) },
    }
    match (Some(plugin_release), parse_release(binary_release)) {
        (Some(p), Some(b)) if p == b => Check { state: State::Configured, detail: format!("plugin {release} is installed and enabled"), fix: None },
        (Some(p), Some(b)) if p < b => Check { state: State::Stale, detail: format!("plugin {release} is older than this binary ({})", show(b)), fix: Some(format!("claude plugin update {PLUGIN_KEY}")) },
        (Some(_), Some(b)) => Check {
            state: State::Incompatible,
            detail: format!("plugin {release} is newer than this binary ({}) and may name tools it lacks", show(b)),
            fix: Some("update delphi (install.sh, or `brew upgrade delphi`)".into()),
        },
        _ => unknown("this binary's release cannot be compared"),
    }
}

/// A release printed from its parsed numbers only, so no text from a file reaches the output.
fn show(r: (u64, u64, u64)) -> String {
    format!("{}.{}.{}", r.0, r.1, r.2)
}

/// The check for any host name; every host but `claude-code` is `unverified` until its slice exists.
pub fn check_host(host: &str, home: &Path, binary_release: &str) -> Result<Check, String> {
    match host {
        "claude-code" => Ok(check_claude_code(home, binary_release)),
        "copilot" | "codex" | "agy" => Ok(Check { state: State::Unverified, detail: "no check exists for this host yet".into(), fix: None }),
        other => Err(format!("unknown host `{other}`; one of {}", crate::mcp_hosts::HOSTS.join(", "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(home: &Path, rel: &str, text: &str) {
        let p = home.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn installed(version: &str) -> String {
        format!(r#"{{"version":2,"plugins":{{"delphi@delphi":[{{"scope":"user","version":"{version}","installPath":"/x"}}]}}}}"#)
    }

    fn fixture(version: &str, enabled: &str) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), ".claude/plugins/installed_plugins.json", &installed(version));
        write(t.path(), ".claude/settings.json", &format!(r#"{{"enabledPlugins":{enabled}}}"#));
        t
    }

    #[test]
    fn the_recorded_files_give_each_state_and_never_launchability() {
        let on = r#"{"delphi@delphi":true}"#;
        assert_eq!(check_claude_code(fixture("0.5.3", on).path(), "0.5.3").state, State::Configured);
        let stale = check_claude_code(fixture("0.5.2", on).path(), "0.5.3");
        assert_eq!((stale.state, stale.fix.as_deref()), (State::Stale, Some("claude plugin update delphi@delphi")));
        assert_eq!(check_claude_code(fixture("0.6.0", on).path(), "0.5.3").state, State::Incompatible);
        let off = check_claude_code(fixture("0.5.3", r#"{"delphi@delphi":false}"#).path(), "0.5.3");
        assert_eq!((off.state, off.fix.as_deref()), (State::Missing, Some("claude plugin enable delphi@delphi")));
        assert_eq!(check_claude_code(fixture("0.5.3", "{}").path(), "0.5.3").state, State::Missing);
        assert!(LAUNCHABLE.starts_with("unknown"));
    }

    #[test]
    fn an_absent_or_unrecognised_file_is_unknown_and_an_empty_plugin_list_is_missing() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check_claude_code(t.path(), "0.5.3").state, State::Unknown);
        write(t.path(), ".claude/plugins/installed_plugins.json", r#"{"version":2,"plugins":{}}"#);
        let m = check_claude_code(t.path(), "0.5.3");
        assert_eq!((m.state, m.fix.as_deref()), (State::Missing, Some(FIX_INSTALL)));
        write(t.path(), ".claude/plugins/installed_plugins.json", r#"{"version":3,"plugins":{}}"#);
        assert_eq!(check_claude_code(t.path(), "0.5.3").state, State::Unknown);
        write(t.path(), ".claude/plugins/installed_plugins.json", "{not json");
        assert_eq!(check_claude_code(t.path(), "0.5.3").state, State::Unknown);
        write(t.path(), ".claude/plugins/installed_plugins.json", &installed("0.5.3"));
        assert_eq!(check_claude_code(t.path(), "0.5.3").state, State::Unknown, "settings.json is missing");
        assert_eq!(check_claude_code(fixture("not-a-release", r#"{"delphi@delphi":true}"#).path(), "0.5.3").state, State::Unknown);
    }

    #[test]
    fn a_symlink_a_fifo_an_oversized_file_and_a_secret_are_refused_or_never_printed() {
        let on = r#"{"delphi@delphi":true}"#;
        let t = fixture("0.5.3", on);
        let real = t.path().join("real.json");
        std::fs::rename(t.path().join(".claude/plugins/installed_plugins.json"), &real).unwrap();
        std::os::unix::fs::symlink(&real, t.path().join(".claude/plugins/installed_plugins.json")).unwrap();
        assert_eq!(check_claude_code(t.path(), "0.5.3").state, State::Unknown);

        let f = tempfile::tempdir().unwrap();
        write(f.path(), ".claude/settings.json", "{}");
        let fifo = f.path().join(".claude/plugins/installed_plugins.json");
        std::fs::create_dir_all(fifo.parent().unwrap()).unwrap();
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert_eq!(check_claude_code(f.path(), "0.5.3").state, State::Unknown);

        let big = tempfile::tempdir().unwrap();
        write(big.path(), ".claude/plugins/installed_plugins.json", &" ".repeat(2 * 1024 * 1024));
        assert_eq!(check_claude_code(big.path(), "0.5.3").state, State::Unknown);

        let s = tempfile::tempdir().unwrap();
        write(s.path(), ".claude/plugins/installed_plugins.json", r#"{"version":2,"plugins":{"delphi@delphi":[{"scope":"user","version":"0.5.3","token":"sk-SECRET-123"}]}}"#);
        write(s.path(), ".claude/settings.json", r#"{"enabledPlugins":{"delphi@delphi":true},"env":{"API_KEY":"sk-SECRET-123"}}"#);
        let r = check_claude_code(s.path(), "0.5.3");
        assert!(!format!("{r:?}").contains("SECRET"));
        write(s.path(), ".claude/plugins/installed_plugins.json", r#"{"version":2,"plugins":{"delphi@delphi":[{"scope":"user","version":"1.0.0-sk-SECRET"}]}}"#);
        assert!(!format!("{:?}", check_claude_code(s.path(), "0.5.3")).contains("SECRET"));
    }

    #[test]
    fn other_hosts_are_unverified_and_an_unknown_host_is_refused() {
        let t = tempfile::tempdir().unwrap();
        for h in ["copilot", "codex", "agy"] {
            assert_eq!(check_host(h, t.path(), "0.5.3").unwrap().state, State::Unverified);
        }
        assert!(check_host("vim", t.path(), "0.5.3").is_err());
    }
}
