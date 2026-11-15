//! Spec 071 E2 and E4a/E4c: is the Delphi plugin set up in Claude Code, Copilot CLI or Codex? A read-only, offline look at the files each host keeps.
//! For Claude Code, the two files
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

/// The one path a host plugin launches, under the user's home (spec 074 §10.1).
pub const STABLE_REL: &str = ".local/bin/delphi";

/// The machine-wide directories an install puts a stable `delphi` in (Homebrew on Apple silicon and on Intel).
pub const MACHINE_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// Whether a host plugin can launch Delphi (spec 074 §10.1 item 3): read from the filesystem, never by running the binary and
/// never from a `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launchable {
    pub ok: bool,
    /// Whether the file a plugin would launch is the binary doing the check. False with `ok` means two installs: the plugin
    /// runs the other one.
    pub this_binary: bool,
    pub detail: String,
}

impl Launchable {
    pub fn line(&self) -> String {
        format!("{} ({})", if self.ok { "yes" } else { "no" }, self.detail)
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

/// The stable install path that holds `exe`, for the link at `~/.local/bin/delphi` to point at: `install_dir`
/// (`$DELPHI_INSTALL_DIR`) or a machine-wide directory, matched by identity. Never a build tree or a versioned Cellar path.
pub fn stable_target(home: &Path, install_dir: Option<&Path>, exe: &Path) -> Option<std::path::PathBuf> {
    stable_target_in(home, install_dir, &MACHINE_DIRS.map(std::path::PathBuf::from), exe)
}

fn stable_target_in(home: &Path, install_dir: Option<&Path>, machine_dirs: &[std::path::PathBuf], exe: &Path) -> Option<std::path::PathBuf> {
    let dirs = install_dir.filter(|d| d.is_absolute()).map(Path::to_path_buf).into_iter().chain(machine_dirs.iter().cloned());
    dirs.map(|d| d.join("delphi")).find(|cand| cand != &home.join(STABLE_REL) && same_file(cand, exe) && crate::mcp_hosts::absolute_command(cand, home.to_str()).is_ok())
}

/// A component of the resolved launch path that someone else could replace: owned by another user, or writable by group or
/// others (a sticky directory such as `/tmp` is not counted: its entries cannot be replaced by others).
fn caution(resolved: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    let mut cur = Some(resolved);
    while let Some(p) = cur {
        let m = std::fs::symlink_metadata(p).ok()?;
        if m.uid() != me && m.uid() != 0 {
            return Some(format!("{} is owned by another user", p.display()));
        }
        if m.mode() & 0o022 != 0 && m.mode() & 0o1000 == 0 {
            return Some(format!("{} is writable by group or others", p.display()));
        }
        cur = p.parent().filter(|q| !q.as_os_str().is_empty());
    }
    None
}

/// What someone else could do to `path` once it is resolved (spec 074 §10.1): `None` when every component belongs to the user
/// or root and none is writable by group or others, or when the path does not resolve.
pub fn path_caution(path: &Path) -> Option<String> {
    std::fs::canonicalize(path).ok().and_then(|r| caution(&r))
}

/// [`Launchable`] for the plugin of this release, which launches `~/.local/bin/delphi`. `exe` is this binary, `install_dir` is
/// `$DELPHI_INSTALL_DIR` if set.
pub fn launchable(home: &Path, install_dir: Option<&Path>, exe: &Path) -> Launchable {
    launchable_in(home, install_dir, &MACHINE_DIRS.map(std::path::PathBuf::from), exe)
}

fn launchable_in(home: &Path, install_dir: Option<&Path>, machine_dirs: &[std::path::PathBuf], exe: &Path) -> Launchable {
    use std::os::unix::fs::MetadataExt;
    let stable = home.join(STABLE_REL);
    let no = |detail: String| Launchable { ok: false, this_binary: false, detail };
    let cure = || match stable_target_in(home, install_dir, machine_dirs, exe) {
        Some(t) => format!(
            "link it: `mkdir -p {} && ln -s {} {}` (or `delphi connect <host>`, which shows the link in its plan and asks first)",
            stable.parent().unwrap_or(home).display(),
            t.display(),
            stable.display()
        ),
        None => "install Delphi with install.sh, or link your installed `delphi` there".to_string(),
    };
    let m = match std::fs::metadata(&stable) {
        Ok(m) => m,
        Err(_) if std::fs::symlink_metadata(&stable).is_ok() => return no(format!("{} is a link that does not resolve; remove it, then {}", stable.display(), cure())),
        Err(_) => return no(format!("{} does not exist, and the plugin launches that path; {}", stable.display(), cure())),
    };
    if !m.is_file() || m.mode() & 0o111 == 0 {
        return no(format!("{} is not an executable file", stable.display()));
    }
    let this_binary = same_file(&stable, exe);
    let mut detail = if this_binary {
        format!("{} is this binary", stable.display())
    } else {
        format!("{} is a different `delphi` from the one running this check; the plugin runs that one", stable.display())
    };
    if let Some(c) = path_caution(&stable) {
        detail.push_str(&format!("; caution: {c}"));
    }
    Launchable { ok: true, this_binary, detail }
}

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
        // The fix is one command: `claude plugin update` refreshes the marketplace clone itself (proven on 2.1.325, spec 074 §10.5).
        (Some(p), Some(b)) if p < b => Check { state: State::Stale, detail: format!("plugin {release} is older than this binary ({})", show(b)), fix: Some(format!("claude plugin update {PLUGIN_KEY}")) },
        (Some(_), Some(b)) => Check {
            state: State::Incompatible,
            detail: format!("plugin {release} is newer than this binary ({}) and may name tools it lacks", show(b)),
            fix: Some("update delphi (install.sh, or `brew upgrade delphi`)".into()),
        },
        _ => unknown("this binary's release cannot be compared"),
    }
}

/// The folder Claude Code installed the Delphi plugin into (user scope), read from `installed_plugins.json` and accepted only below
/// `~/.claude/plugins` with no `..`; `None` when it cannot be read safely.
fn claude_plugin_folder(home: &Path) -> Option<std::path::PathBuf> {
    let plugins = home.join(".claude").join("plugins");
    let installed = safe_read(&plugins.join("installed_plugins.json")).ok()??;
    let doc: serde_json::Value = serde_json::from_slice(&installed).ok()?;
    if doc["version"] != 2 {
        return None;
    }
    let entry = doc["plugins"].get(PLUGIN_KEY)?.as_array()?.iter().find(|e| e["scope"] == "user")?;
    let install = Path::new(entry["installPath"].as_str()?);
    if !install.is_absolute() || !install.starts_with(&plugins) || install.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return None;
    }
    Some(install.to_path_buf())
}

/// Spec 105 §6, A7: is the installed Claude Code plugin's `hooks/hooks.json` the one this binary generates? `Some((same, detail))`,
/// with the file's path in the detail, or `None` when the plugin's folder cannot be read safely (as [`check_claude_adapter`]).
pub fn check_claude_hooks(home: &Path, expected_json: &str) -> Option<(bool, String)> {
    let install = claude_plugin_folder(home)?;
    let path = install.join("hooks").join("hooks.json");
    match safe_read(&path) {
        Ok(Some(bytes)) if bytes == expected_json.as_bytes() => Some((true, format!("installed at {} (the hooks this binary declares)", path.display()))),
        Ok(Some(_)) => Some((false, format!("{} differs from the hooks this binary declares; update the plugin (`claude plugin update {PLUGIN_KEY}`)", path.display()))),
        Ok(None) if install.is_dir() => Some((false, format!("the installed plugin has no hooks file (it predates spec 105; expected at {})", path.display()))),
        _ => None,
    }
}

/// The command a Claude Code status line runs to name the adopted role (spec 106 §12 G6 item 2). Delphi never writes it for you.
pub const STATUSLINE_COMMAND: &str = "delphi mcp --statusline --host claude-code";

/// Spec 106 §12 G6 item 2: does the operator's Claude Code user settings (`~/.claude/settings.json`) run Delphi's status line?
/// Read-only, never through a symlink; the line it prints names no setting's value but Delphi's own command.
pub fn check_statusline(home: &Path) -> String {
    let path = home.join(".claude").join("settings.json");
    match safe_read(&path) {
        Ok(Some(bytes)) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(v) if v["statusLine"]["command"].as_str().is_some_and(|c| c.contains("delphi mcp --statusline")) => format!("set ({} runs `delphi mcp --statusline`)", path.display()),
            Ok(v) if v["statusLine"].is_object() => format!("not set: {} has another status line", path.display()),
            Ok(_) => format!("not set: add \"statusLine\": {{\"type\": \"command\", \"command\": \"{STATUSLINE_COMMAND}\"}} to {}", path.display()),
            Err(_) => format!("unknown: {} is not JSON", path.display()),
        },
        Ok(None) => format!("not set: add \"statusLine\": {{\"type\": \"command\", \"command\": \"{STATUSLINE_COMMAND}\"}} to {} (it does not exist)", path.display()),
        Err(e) => format!("unknown: {e}"),
    }
}

/// Spec 098 §4: does the host adapter shipped in the installed Claude Code plugin (`adapter.json`) match the one this binary writes
/// into its pack headers? `Some((matches, detail))`, or `None` when the plugin's folder cannot be read safely: no plugin state, an
/// install path outside `~/.claude/plugins` (the path comes from a file Claude Code writes, so it is read only below its own
/// plugins folder, never through a symlink), or an unreadable file. Read-only and offline; it says nothing the release check
/// (`check_claude_code`) has not, except that a plugin of the right release can still carry an adapter of another hash.
pub fn check_claude_adapter(home: &Path, expected_json: &str) -> Option<(bool, String)> {
    let install = claude_plugin_folder(home)?;
    let install = install.as_path();
    let ours = crate::mcp_hosts::adapter_hash(expected_json);
    match safe_read(&install.join("adapter.json")) {
        Ok(Some(bytes)) => {
            let theirs = crate::mcp_hosts::adapter_hash(&String::from_utf8_lossy(&bytes));
            if theirs == ours {
                Some((true, format!("the installed plugin's adapter (v{}, hash {ours}) is the one this binary writes", crate::mcp_hosts::ADAPTER_VERSION)))
            } else {
                Some((false, format!("the installed plugin's adapter (hash {theirs}) is not the one this binary writes (hash {ours}); a pack header names a mapping your plugin does not have")))
            }
        }
        Ok(None) if install.is_dir() => Some((false, format!("the installed plugin has no adapter.json (it predates spec 098); this binary's adapter has hash {ours}"))),
        _ => None,
    }
}

/// Spec 074 L1: whether a stable install location holds this binary, from `stat` alone (nothing is executed). The directories are
/// `install_dir` (`$DELPHI_INSTALL_DIR`, if set), `~/.local/bin`, `/opt/homebrew/bin` and `/usr/local/bin`. It says nothing about a host's own `PATH`.
pub fn launch_path(home: &Path, install_dir: Option<&Path>, exe: &Path) -> String {
    launch_path_in(home, install_dir, &MACHINE_DIRS.map(std::path::PathBuf::from), exe)
}

/// [`launch_path`] with the machine-wide install directories given, so a test can look only where it says: the real
/// `/usr/local/bin` and `/opt/homebrew/bin` hold a `delphi` on any machine that installed one with Homebrew.
fn launch_path_in(home: &Path, install_dir: Option<&Path>, machine_dirs: &[std::path::PathBuf], exe: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let Ok(me) = std::fs::metadata(exe) else { return "unknown (this binary cannot be inspected)".into() };
    let mut dirs: Vec<std::path::PathBuf> = install_dir.filter(|d| d.is_absolute()).map(Path::to_path_buf).into_iter().collect();
    dirs.push(home.join(".local/bin"));
    dirs.extend(machine_dirs.iter().cloned());
    let mut other = false;
    for d in dirs {
        let cand = d.join("delphi");
        match std::fs::metadata(&cand) {
            Ok(m) if m.dev() == me.dev() && m.ino() == me.ino() => {
                return format!("stable path {} is this binary; a config you place by hand can name it with `delphi mcp --print-config <host> --absolute`", cand.display());
            }
            Ok(_) => other = true,
            Err(_) => {}
        }
    }
    if other {
        "no stable path holds this binary (a different `delphi` is installed in a standard location); run `delphi mcp --print-config <host> --absolute` from the one you want".into()
    } else {
        "no stable path holds this binary (it is not in a standard install location); install with install.sh or Homebrew, or use `delphi mcp --print-config <host> --absolute`".into()
    }
}

/// Copilot CLI (1.0.105): `~/.copilot/settings.json` `enabledPlugins`. A plugin from a local marketplace is loaded live and records no
/// release, so only enabled or not is known and the release is never compared.
pub fn check_copilot(home: &Path) -> Check {
    let settings = match safe_read(&home.join(".copilot/settings.json")) {
        Ok(Some(b)) => b,
        Ok(None) => return unknown("no Copilot CLI settings were found, so this cannot be checked"),
        Err(e) => return unknown(format!("settings.json {e}")),
    };
    let Ok(settings) = serde_json::from_slice::<serde_json::Value>(&settings) else { return unknown("settings.json is not valid JSON") };
    let fix = "copilot plugin marketplace add jessevondoom/delphi && copilot plugin install delphi@delphi";
    match settings["enabledPlugins"].get(PLUGIN_KEY).and_then(|v| v.as_bool()) {
        Some(true) => Check { state: State::Configured, detail: "the Delphi plugin is installed and enabled (Copilot CLI records no release, so it is not compared)".into(), fix: None },
        Some(false) => Check { state: State::Missing, detail: "the Delphi plugin is installed but disabled".into(), fix: Some(format!("copilot plugin install {PLUGIN_KEY}")) },
        None => Check { state: State::Missing, detail: "the Delphi plugin is not installed".into(), fix: Some(fix.into()) },
    }
}

/// Codex (0.166.0): `~/.codex/config.toml` `[plugins."delphi@delphi"] enabled`, and the release from the cache directory name under
/// `~/.codex/plugins/cache/delphi/delphi/`.
pub fn check_codex(home: &Path, binary_release: &str) -> Check {
    let cfg = match safe_read(&home.join(".codex/config.toml")) {
        Ok(Some(b)) => b,
        Ok(None) => return unknown("no Codex configuration was found, so this cannot be checked"),
        Err(e) => return unknown(format!("config.toml {e}")),
    };
    let Ok(cfg) = String::from_utf8(cfg).map_err(|_| ()).and_then(|t| t.parse::<toml::Table>().map_err(|_| ())) else { return unknown("config.toml is not valid TOML") };
    let fix = "codex plugin marketplace add jessevondoom/delphi && codex plugin add delphi@delphi";
    let enabled = cfg.get("plugins").and_then(|p| p.as_table()).and_then(|p| p.get(PLUGIN_KEY)).and_then(|p| p.as_table()).map(|p| p.get("enabled").and_then(|e| e.as_bool()));
    match enabled {
        None => return Check { state: State::Missing, detail: "the Delphi plugin is not installed".into(), fix: Some(fix.into()) },
        Some(Some(false)) => return Check { state: State::Missing, detail: "the Delphi plugin is installed but disabled".into(), fix: Some(fix.into()) },
        Some(_) => {}
    }
    let cache = home.join(".codex/plugins/cache/delphi/delphi");
    let mut best: Option<(u64, u64, u64)> = None;
    if let Ok(rd) = std::fs::read_dir(&cache) {
        for e in rd.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                best = best.max(e.file_name().to_str().and_then(parse_release));
            }
        }
    }
    let Some(plugin_release) = best else { return unknown("the plugin is enabled but its installed release cannot be read") };
    compare(plugin_release, binary_release, &format!("codex plugin add {PLUGIN_KEY}"))
}

fn compare(plugin_release: (u64, u64, u64), binary_release: &str, update: &str) -> Check {
    let release = show(plugin_release);
    match parse_release(binary_release) {
        Some(b) if plugin_release == b => Check { state: State::Configured, detail: format!("plugin {release} is installed and enabled"), fix: None },
        Some(b) if plugin_release < b => Check { state: State::Stale, detail: format!("plugin {release} is older than this binary ({})", show(b)), fix: Some(update.into()) },
        Some(b) => Check {
            state: State::Incompatible,
            detail: format!("plugin {release} is newer than this binary ({}) and may name tools it lacks", show(b)),
            fix: Some("update delphi (install.sh, or `brew upgrade delphi`)".into()),
        },
        None => unknown("this binary's release cannot be compared"),
    }
}

/// A release printed from its parsed numbers only, so no text from a file reaches the output.
fn show(r: (u64, u64, u64)) -> String {
    format!("{}.{}.{}", r.0, r.1, r.2)
}

/// The check for any host name; Claude Code, Copilot and Codex are read; `agy` is `unverified` until its slice exists.
pub fn check_host(host: &str, home: &Path, binary_release: &str) -> Result<Check, String> {
    match host {
        "claude-code" => Ok(check_claude_code(home, binary_release)),
        "copilot" => Ok(check_copilot(home)),
        "codex" => Ok(check_codex(home, binary_release)),
        "agy" => Ok(Check { state: State::Unverified, detail: "no check exists for this host yet".into(), fix: None }),
        "gemini" => Ok(Check { state: State::Unverified, detail: "Gemini CLI could not be run in a live check (spec 110 section 8); the lines below read what `delphi connect gemini` wrote".into(), fix: None }),
        other => Err(format!("unknown host `{other}`; one of {}", crate::mcp_hosts::HOSTS.join(", "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec 106 §12 G6: `--check` reads, in a fake home, whether the status line is set; it never writes.
    #[test]
    fn the_status_line_check_reads_the_users_settings_and_writes_nothing() {
        let t = tempfile::tempdir().unwrap();
        assert!(check_statusline(t.path()).starts_with("not set") && !t.path().join(".claude").exists(), "nothing is created");
        std::fs::create_dir_all(t.path().join(".claude")).unwrap();
        std::fs::write(t.path().join(".claude/settings.json"), format!("{{\"statusLine\": {{\"type\": \"command\", \"command\": \"{STATUSLINE_COMMAND}\"}}}}")).unwrap();
        assert!(check_statusline(t.path()).starts_with("set"));
        std::fs::write(t.path().join(".claude/settings.json"), r#"{"statusLine": {"type": "command", "command": "my-line"}}"#).unwrap();
        assert!(check_statusline(t.path()).contains("another status line") && !check_statusline(t.path()).contains("my-line"));
        std::fs::write(t.path().join(".claude/settings.json"), "{").unwrap();
        assert!(check_statusline(t.path()).starts_with("unknown"));
    }

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
    }

    /// Spec 074 §10.1 item 3: launchability is what `~/.local/bin/delphi` is, from `stat` alone.
    #[test]
    fn launchable_is_read_from_the_stable_path_and_names_the_cure() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(t.path()).unwrap();
        let brew = home.join("brew/bin");
        std::fs::create_dir_all(&brew).unwrap();
        let exe = brew.join("delphi");
        std::fs::write(&exe, "x").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let look = |exe: &Path| launchable_in(&home, None, std::slice::from_ref(&brew), exe);
        let stable = home.join(STABLE_REL);

        // Nothing there: no, and the cure links the stable install path that holds this binary.
        let l = look(&exe);
        assert!(!l.ok && l.detail.contains("does not exist") && l.detail.contains(&format!("ln -s {} {}", exe.display(), stable.display())), "{}", l.line());
        assert!(l.line().starts_with("no ("));
        // A binary that is in no stable location has no link to offer.
        let stray = home.join("elsewhere");
        std::fs::write(&stray, "y").unwrap();
        assert!(look(&stray).detail.contains("install Delphi with install.sh"));

        // A link to this binary: yes.
        std::fs::create_dir_all(stable.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&exe, &stable).unwrap();
        let l = look(&exe);
        assert!(l.ok && l.detail.contains("is this binary"), "{}", l.line());
        // The same path seen from another binary: yes, and it says which one a plugin runs.
        let other = look(&stray);
        assert!(other.ok && !other.this_binary && other.detail.contains("a different `delphi`"), "{}", other.line());
        assert!(look(&exe).this_binary);
        assert_eq!(path_caution(&stable), None);
        assert_eq!(path_caution(&home.join("nothing-here")), None, "a path that does not resolve has nothing to say");
        // A parent that group or others can write earns a caution, not a refusal.
        std::fs::set_permissions(&brew, std::fs::Permissions::from_mode(0o775)).unwrap();
        let l = look(&exe);
        assert!(l.ok && l.detail.contains("caution") && l.detail.contains("writable by group or others"), "{}", l.line());
        assert!(path_caution(&exe).unwrap().contains("writable by group or others"), "the same finding is there to show before a link is made");
        std::fs::set_permissions(&brew, std::fs::Permissions::from_mode(0o755)).unwrap();

        // A dangling link and a file that is not executable: no.
        std::fs::remove_file(&stable).unwrap();
        std::os::unix::fs::symlink(home.join("gone"), &stable).unwrap();
        assert!(look(&exe).detail.contains("does not resolve"));
        std::fs::remove_file(&stable).unwrap();
        std::fs::write(&stable, "z").unwrap();
        std::fs::set_permissions(&stable, std::fs::Permissions::from_mode(0o644)).unwrap();
        let l = look(&exe);
        assert!(!l.ok && l.detail.contains("not an executable file"), "{}", l.line());
        // The real file, as install.sh leaves it: yes.
        std::fs::set_permissions(&stable, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(look(&stable).ok);
    }

    /// The link's target is a stable install path only: never a build tree, a versioned Cellar path or the stable path itself.
    #[test]
    fn the_stable_target_is_an_install_path_that_holds_this_binary() {
        let t = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(t.path()).unwrap();
        let real = home.join("Cellar/delphi/1.0/bin/delphi");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "x").unwrap();
        let brew = home.join("brew/bin");
        std::fs::create_dir_all(&brew).unwrap();
        assert_eq!(stable_target_in(&home, None, std::slice::from_ref(&brew), &real), None, "no stable location holds it");
        std::os::unix::fs::symlink(&real, brew.join("delphi")).unwrap();
        assert_eq!(stable_target_in(&home, None, std::slice::from_ref(&brew), &real), Some(brew.join("delphi")), "the unversioned link, not the Cellar file");
        let cellar_dir = real.parent().unwrap().to_path_buf();
        assert_eq!(stable_target_in(&home, None, &[cellar_dir], &real), None, "a versioned Cellar path is never a target");
        let build = home.join("target/debug");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::hard_link(&real, build.join("delphi")).unwrap();
        assert_eq!(stable_target_in(&home, Some(&build), &[], &real), None, "a build tree is never a target");
        let custom = home.join("custom");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::hard_link(&real, custom.join("delphi")).unwrap();
        assert_eq!(stable_target_in(&home, Some(&custom), std::slice::from_ref(&brew), &real), Some(custom.join("delphi")), "$DELPHI_INSTALL_DIR comes first");
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
    fn the_adapter_in_the_installed_plugin_is_compared_with_this_binarys_and_read_only_below_the_plugins_folder() {
        let want = crate::mcp_hosts::claude_code_adapter_json();
        let t = tempfile::tempdir().unwrap();
        let listing = |path: &std::path::Path| format!(r#"{{"version":2,"plugins":{{"delphi@delphi":[{{"scope":"user","version":"0.6.0","installPath":{}}}]}}}}"#, serde_json::json!(path.to_str().unwrap()));
        let install = t.path().join(".claude/plugins/cache/delphi/delphi/0.6.0");
        write(t.path(), ".claude/plugins/installed_plugins.json", &listing(&install));
        assert!(check_claude_adapter(t.path(), &want).is_none(), "no folder, nothing to say");
        std::fs::create_dir_all(&install).unwrap();
        let (ok, why) = check_claude_adapter(t.path(), &want).unwrap();
        assert!(!ok && why.contains("no adapter.json"), "{why}");
        std::fs::write(install.join("adapter.json"), &want).unwrap();
        let (ok, why) = check_claude_adapter(t.path(), &want).unwrap();
        assert!(ok && why.contains("is the one this binary writes"), "{why}");
        std::fs::write(install.join("adapter.json"), want.replace("\"Read\"", "\"Cat\"")).unwrap();
        let (ok, why) = check_claude_adapter(t.path(), &want).unwrap();
        assert!(!ok && why.contains("is not the one this binary writes"), "{why}");
        // The install path is a value from a file Claude Code writes: outside ~/.claude/plugins, or with `..`, it is never read.
        let outside = t.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("adapter.json"), &want).unwrap();
        write(t.path(), ".claude/plugins/installed_plugins.json", &listing(&outside));
        assert!(check_claude_adapter(t.path(), &want).is_none(), "outside the plugins folder");
        write(t.path(), ".claude/plugins/installed_plugins.json", &listing(&install.join("../../../../../../elsewhere")));
        assert!(check_claude_adapter(t.path(), &want).is_none(), "a `..` component");
        // A symlinked adapter.json is refused, not followed.
        write(t.path(), ".claude/plugins/installed_plugins.json", &listing(&install));
        std::fs::remove_file(install.join("adapter.json")).unwrap();
        std::os::unix::fs::symlink(outside.join("adapter.json"), install.join("adapter.json")).unwrap();
        assert!(check_claude_adapter(t.path(), &want).is_none(), "a symlink is not followed");
    }

    #[test]
    fn the_hooks_file_in_the_installed_plugin_is_compared_with_this_binarys_and_never_followed_through_a_symlink() {
        let want = crate::host_hooks::claude_hooks_json();
        let t = tempfile::tempdir().unwrap();
        let install = t.path().join(".claude/plugins/cache/delphi/delphi/0.6.0");
        write(t.path(), ".claude/plugins/installed_plugins.json", &format!(r#"{{"version":2,"plugins":{{"delphi@delphi":[{{"scope":"user","version":"0.6.0","installPath":{}}}]}}}}"#, serde_json::json!(install.to_str().unwrap())));
        assert!(check_claude_hooks(t.path(), &want).is_none(), "no folder, nothing to say");
        std::fs::create_dir_all(install.join("hooks")).unwrap();
        let (ok, why) = check_claude_hooks(t.path(), &want).unwrap();
        assert!(!ok && why.contains("no hooks file"), "{why}");
        std::fs::write(install.join("hooks/hooks.json"), &want).unwrap();
        let (ok, why) = check_claude_hooks(t.path(), &want).unwrap();
        assert!(ok && why.contains("hooks/hooks.json"), "{why}");
        std::fs::write(install.join("hooks/hooks.json"), want.replace("session-end", "x")).unwrap();
        assert!(!check_claude_hooks(t.path(), &want).unwrap().0);
        std::fs::remove_file(install.join("hooks/hooks.json")).unwrap();
        let elsewhere = t.path().join("h.json");
        std::fs::write(&elsewhere, &want).unwrap();
        std::os::unix::fs::symlink(&elsewhere, install.join("hooks/hooks.json")).unwrap();
        assert!(check_claude_hooks(t.path(), &want).is_none(), "a symlink is not followed");
    }

    #[test]
    fn agy_is_unverified_and_an_unknown_host_is_refused() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check_host("agy", t.path(), "0.5.3").unwrap().state, State::Unverified);
        assert!(check_host("vim", t.path(), "0.5.3").is_err());
    }

    #[test]
    fn launch_path_finds_a_stable_location_holding_this_binary_by_identity() {
        let t = tempfile::tempdir().unwrap();
        let exe = t.path().join("elsewhere/delphi");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "x").unwrap();
        // The machine-wide directories are given as none: the real /usr/local/bin and /opt/homebrew/bin hold a `delphi` on any
        // machine that installed one with Homebrew, and this test must say the same thing there as on a clean runner.
        let look = |install: Option<&Path>, exe: &Path| launch_path_in(t.path(), install, &[], exe);
        assert!(look(None, &exe).starts_with("no stable path holds this binary (it is not"));
        let bin = t.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("delphi"), "y").unwrap();
        assert!(look(None, &exe).contains("a different `delphi`"));
        std::fs::remove_file(bin.join("delphi")).unwrap();
        std::os::unix::fs::symlink(&exe, bin.join("delphi")).unwrap();
        assert!(look(None, &exe).starts_with(&format!("stable path {}", bin.join("delphi").display())));
        let custom = t.path().join("custom");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::hard_link(&exe, custom.join("delphi")).unwrap();
        assert!(look(Some(&custom), &exe).contains(&custom.join("delphi").display().to_string()));
        assert!(look(None, &t.path().join("missing")).starts_with("unknown"));
    }

    #[test]
    fn launch_path_looks_in_the_machine_wide_directories_it_is_given() {
        let t = tempfile::tempdir().unwrap();
        let exe = t.path().join("elsewhere/delphi");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "x").unwrap();
        let (first, second) = (t.path().join("brew"), t.path().join("usr-local"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let dirs = [first.clone(), second.clone()];
        assert!(launch_path_in(t.path(), None, &dirs, &exe).contains("it is not in a standard install location"));
        // A different binary in a machine-wide directory is reported as such, not as this one.
        std::fs::write(second.join("delphi"), "other").unwrap();
        assert!(launch_path_in(t.path(), None, &dirs, &exe).contains("a different `delphi`"));
        // A link to this binary in the second directory is found by identity.
        std::fs::remove_file(second.join("delphi")).unwrap();
        std::os::unix::fs::symlink(&exe, second.join("delphi")).unwrap();
        assert!(launch_path_in(t.path(), None, &dirs, &exe).starts_with(&format!("stable path {}", second.join("delphi").display())));
        // The home directory is searched before the machine-wide ones.
        let bin = t.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(&exe, bin.join("delphi")).unwrap();
        assert!(launch_path_in(t.path(), None, &dirs, &exe).starts_with(&format!("stable path {}", bin.join("delphi").display())));
    }

    #[test]
    fn copilot_is_read_from_enabled_plugins_and_never_echoes_a_secret() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check_copilot(t.path()).state, State::Unknown);
        write(t.path(), ".copilot/settings.json", r#"{"enabledPlugins":{"delphi@delphi":true},"token":"sk-SECRET"}"#);
        let c = check_copilot(t.path());
        assert_eq!(c.state, State::Configured);
        assert!(!format!("{c:?}").contains("SECRET"));
        write(t.path(), ".copilot/settings.json", r#"{"enabledPlugins":{"delphi@delphi":false}}"#);
        assert_eq!(check_copilot(t.path()).state, State::Missing);
        write(t.path(), ".copilot/settings.json", "{}");
        assert_eq!(check_copilot(t.path()).state, State::Missing);
        write(t.path(), ".copilot/settings.json", "// comment\n{");
        assert_eq!(check_copilot(t.path()).state, State::Unknown);
    }

    #[test]
    fn codex_reads_the_toml_flag_and_the_cache_release() {
        let on = "[plugins.\"delphi@delphi\"]\nenabled = true\n";
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check_codex(t.path(), "0.5.3").state, State::Unknown);
        write(t.path(), ".codex/config.toml", "[tui]\nx = 1\n");
        assert_eq!(check_codex(t.path(), "0.5.3").state, State::Missing);
        write(t.path(), ".codex/config.toml", on);
        assert_eq!(check_codex(t.path(), "0.5.3").state, State::Unknown, "no cache directory");
        std::fs::create_dir_all(t.path().join(".codex/plugins/cache/delphi/delphi/0.5.3")).unwrap();
        assert_eq!(check_codex(t.path(), "0.5.3").state, State::Configured);
        assert_eq!(check_codex(t.path(), "0.6.0").state, State::Stale);
        assert_eq!(check_codex(t.path(), "0.5.0").state, State::Incompatible);
        write(t.path(), ".codex/config.toml", "[plugins.\"delphi@delphi\"]\nenabled = false\n");
        assert_eq!(check_codex(t.path(), "0.5.3").state, State::Missing);
        write(t.path(), ".codex/config.toml", "= not toml");
        assert_eq!(check_codex(t.path(), "0.5.3").state, State::Unknown);
    }
}
