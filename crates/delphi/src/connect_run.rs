//! Spec 074 C2: the execution path of `delphi connect claude-code` and `--remove`. Reached only through `connect_cmd::cmd`, in the `delphi`
//! binary crate (the engine, MCP server and daemon crates cannot call it), and every run takes a `ConsentProof` that only the `/dev/tty`
//! confirmation can issue. Fixed argv, no shell, an allowlisted environment, a digest recheck before each exec, a hardened record.

use std::io::{BufRead, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use delphi_engine::mcp_check::{self, State};
use serde_json::{json, Value};

use crate::connect_cmd::{self, sanitize, HostExe, Template, SOURCE};

const MAX_RECORD: u64 = 64 * 1024;
const SAFE_PATH: &str = "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";

pub mod consent {
    //! The only place a `ConsentProof` is made: the field is private to this module.
    use super::*;

    pub struct ConsentProof(());

    /// Show `plan` on the controlling terminal and require the exact plan-bound `expect` phrase back.
    pub fn ask(plan: &str, expect: &str) -> Result<ConsentProof, String> {
        let tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").map_err(|_| "there is no controlling terminal to ask on".to_string())?;
        let mut out = tty.try_clone().map_err(|e| e.to_string())?;
        write!(out, "{plan}\nTo go ahead, enter exactly: {expect}\n(pasting is fine; the phrase is bound to this plan, so a stale or different plan will not match)\n> ").map_err(|e| e.to_string())?;
        out.flush().ok();
        let mut line = String::new();
        std::io::BufReader::new(tty).read_line(&mut line).map_err(|e| e.to_string())?;
        if line.trim() == expect {
            Ok(ConsentProof(()))
        } else {
            Err("not confirmed; nothing was run".into())
        }
    }

    #[cfg(test)]
    pub fn for_test() -> ConsentProof {
        ConsentProof(())
    }
}
use consent::ConsentProof;

/// Why a connection could not be made or removed.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    NotAllowed(String),
    Failed(String),
    Partial(String),
}

impl Refusal {
    pub fn message(&self) -> &str {
        match self {
            Refusal::NotAllowed(m) | Refusal::Failed(m) | Refusal::Partial(m) => m,
        }
    }
}

/// Where the command may run: a person at a terminal, not an agent seat, entered as `delphi connect`.
pub fn may_run_here() -> Result<(), String> {
    if crate::xenia_terminal::inside_an_agent_seat() {
        return Err("connecting is a human act and is refused inside an agent seat".into());
    }
    if std::env::args().nth(1).as_deref() != Some("connect") {
        return Err("connecting is refused unless entered as `delphi connect`".into());
    }
    // SAFETY: isatty has no preconditions.
    if unsafe { libc::isatty(0) != 1 || libc::isatty(1) != 1 } {
        return Err("connecting needs a terminal on both stdin and stdout (use --dry-run to preview)".into());
    }
    Ok(())
}

/// The environment a host CLI gets: an allowlist, never a pass-through.
pub fn host_env(home: &Path, tmp: &Path) -> Vec<(String, String)> {
    host_env_from(home, tmp, std::env::vars())
}

fn host_env_from(home: &Path, tmp: &Path, vars: impl Iterator<Item = (String, String)>) -> Vec<(String, String)> {
    let mut e = vec![("PATH".to_string(), SAFE_PATH.to_string()), ("HOME".into(), home.display().to_string()), ("TMPDIR".into(), tmp.display().to_string())];
    for (k, v) in vars {
        if k == "LANG" || k == "TERM" || k.starts_with("LC_") {
            e.push((k, v));
        }
    }
    e
}

fn private_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    if let Ok(m) = std::fs::symlink_metadata(path) {
        // SAFETY: geteuid has no preconditions.
        if m.file_type().is_symlink() || !m.is_dir() || m.uid() != unsafe { libc::geteuid() } {
            return Err(format!("{} is not a directory you own", sanitize(&path.display().to_string())));
        }
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700)).map_err(|e| e.to_string())?;
        return Ok(());
    }
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path).map_err(|e| e.to_string())
}

fn read_json(path: &Path) -> Option<Value> {
    let m = std::fs::symlink_metadata(path).ok()?;
    if !m.is_file() || m.len() > MAX_RECORD * 64 {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The GitHub repository a host's own state records for its marketplace named `delphi`. `Some(None)`: present, but not a GitHub source.
pub fn marketplace_source(host: &str, home: &Path) -> Option<Option<String>> {
    match host {
        "claude-code" | "copilot" => {
            let path = if host == "copilot" { home.join(".copilot/settings.json") } else { home.join(".claude/plugins/known_marketplaces.json") };
            let v = read_json(&path)?;
            let m = if host == "copilot" { v.get("extraKnownMarketplaces")?.get("delphi")? } else { v.get("delphi")? };
            Some(if m["source"]["source"] == "github" { m["source"]["repo"].as_str().map(str::to_string) } else { None })
        }
        _ => None,
    }
}

/// The plugin's release as the host's own state records it (`unrecorded` where the host keeps none); `None` when it is not installed.
pub fn installed_release(host: &str, home: &Path) -> Option<String> {
    match host {
        "claude-code" => {
            let v = read_json(&home.join(".claude/plugins/installed_plugins.json"))?;
            v["plugins"]["delphi@delphi"].as_array()?.iter().find(|e| e["scope"] == "user")?["version"].as_str().map(str::to_string)
        }
        "copilot" => {
            read_json(&home.join(".copilot/settings.json"))?["enabledPlugins"].get("delphi@delphi")?.as_bool()?;
            Some(copilot_installed(home).map_or_else(|| "unrecorded".to_string(), |(v, _)| v))
        }
        _ => None,
    }
}

fn other_plugins_use_marketplace(host: &str, home: &Path) -> bool {
    let keys: Vec<String> = match host {
        "claude-code" => read_json(&home.join(".claude/plugins/installed_plugins.json")).and_then(|v| v["plugins"].as_object().map(|p| p.keys().cloned().collect())).unwrap_or_default(),
        "copilot" => read_json(&home.join(".copilot/settings.json")).and_then(|v| v["enabledPlugins"].as_object().map(|p| p.keys().cloned().collect())).unwrap_or_default(),
        _ => Vec::new(),
    };
    keys.iter().any(|k| k.ends_with("@delphi") && k != "delphi@delphi")
}

/// Copilot's own installed copy: the release in its `plugin.json` and a digest of the two files that define the plugin. Bound into the record so
/// a later manual reinstall is not mistaken for what connect added.
pub fn copilot_installed(home: &Path) -> Option<(String, String)> {
    let dir = home.join(".copilot/installed-plugins/delphi/delphi");
    let manifest = std::fs::read(dir.join(".claude-plugin/plugin.json")).ok()?;
    let mcp = std::fs::read(dir.join(".mcp.json")).unwrap_or_default();
    let v: Value = serde_json::from_slice(&manifest).ok()?;
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(&manifest);
    h.update([0u8]);
    h.update(&mcp);
    Some((v["version"].as_str()?.to_string(), h.finalize().iter().map(|b| format!("{b:02x}")).collect()))
}

/// Delphi artifacts the host keeps that its settings do not show: a leftover installed copy is not "absent".
fn leftover(host: &str, home: &Path) -> bool {
    host == "copilot" && std::fs::symlink_metadata(home.join(".copilot/installed-plugins/delphi")).is_ok()
}

/// Whether the host stands where connect may add the plugin: nothing of Delphi's is present.
pub fn addable(t: &Template, pre: &mcp_check::Check, home: &Path) -> bool {
    let clean = (pre.state == State::Missing && pre.detail == "the Delphi plugin is not installed") || (pre.state == State::Unknown && pre.detail.starts_with("no "));
    clean && marketplace_source(t.host, home).is_none() && installed_release(t.host, home).is_none() && !leftover(t.host, home)
}

struct Run<'a> {
    exe: &'a HostExe,
    home: &'a Path,
    tmp: &'a Path,
}

impl Run<'_> {
    /// One step: recheck the digest, then exec with the fixed argv and the allowlisted environment.
    fn step(&self, _proof: &ConsentProof, argv: &[String]) -> Result<(), String> {
        let now = connect_cmd::sha256_file(&self.exe.canonical)?;
        if now != self.exe.sha256 {
            return Err("the host CLI changed after the plan was shown; nothing more was run".into());
        }
        let status = Command::new(&argv[0])
            .args(&argv[1..])
            .env_clear()
            .envs(host_env(self.home, self.tmp))
            .current_dir(self.tmp)
            .stdin(Stdio::null())
            .status()
            .map_err(|e| format!("the host CLI could not be started: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("the host CLI exited with {}", status.code().map_or("a signal".to_string(), |c| c.to_string())))
        }
    }
}

fn run_dir(delphi_home: &Path) -> Result<PathBuf, String> {
    let op = delphi_home.join("operator");
    private_dir(&op)?;
    let d = op.join(format!("connect-tmp-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_nanos())));
    private_dir(&d)?;
    Ok(d)
}

fn audit(delphi_home: &Path, t: &Template, action: &str, exe: &HostExe, result: &str) {
    let rec = json!({"type": "connect.attempt", "at": now(), "host": t.host, "template": t.id, "action": action, "exe_sha256": exe.sha256, "result": result});
    let op = delphi_home.join("operator");
    if private_dir(&op).is_ok() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(op.join("connect-audit.jsonl")) {
            let _ = writeln!(f, "{rec}");
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_secs() as i64)
}

/// `connections.json`: no symlink, a directory you own at 0700, 0600, regular, at most 64 KiB, locked, replaced atomically; fails closed.
fn update_record(delphi_home: &Path, f: impl FnOnce(&mut serde_json::Map<String, Value>) -> Result<(), String>) -> Result<(), String> {
    let op = delphi_home.join("operator");
    private_dir(&op)?;
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(op.join("connections.lock")).map_err(|e| e.to_string())?;
    // SAFETY: the descriptor is open for the call.
    if unsafe { libc::flock(std::os::unix::io::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) } != 0 {
        return Err("the connections record is locked".into());
    }
    let path = op.join("connections.json");
    let mut doc = match std::fs::symlink_metadata(&path) {
        Err(_) => serde_json::Map::new(),
        Ok(m) => {
            if !m.is_file() || m.len() > MAX_RECORD || m.mode() & 0o077 != 0 {
                return Err("connections.json is not a private regular file of a sane size; it was left alone".into());
            }
            let mut s = String::new();
            std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(&path).and_then(|mut f| f.read_to_string(&mut s)).map_err(|e| e.to_string())?;
            match serde_json::from_str::<Value>(&s) {
                Ok(Value::Object(o)) => o,
                _ => return Err("connections.json is malformed; it was left alone".into()),
            }
        }
    };
    f(&mut doc)?;
    let tmp = op.join(format!("connections.json.tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut t = std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp).map_err(|e| e.to_string())?;
    t.write_all(Value::Object(doc).to_string().as_bytes()).and_then(|_| t.sync_all()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

pub fn read_record(delphi_home: &Path, host: &str) -> Result<Option<Value>, String> {
    let path = delphi_home.join("operator/connections.json");
    let Ok(m) = std::fs::symlink_metadata(&path) else { return Ok(None) };
    if !m.is_file() || m.len() > MAX_RECORD || m.mode() & 0o077 != 0 {
        return Err("connections.json is not a private regular file of a sane size".into());
    }
    let mut s = String::new();
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(&path).and_then(|mut f| f.read_to_string(&mut s)).map_err(|e| e.to_string())?;
    match serde_json::from_str::<Value>(&s) {
        Ok(v @ Value::Object(_)) => Ok(v.get(host).cloned()),
        _ => Err("connections.json is malformed".into()),
    }
}

/// Add the marketplace and the plugin. The caller has already shown the plan and obtained `proof`.
pub fn connect(t: &Template, proof: &ConsentProof, exe: &HostExe, home: &Path, delphi_home: &Path, release: &str) -> Result<String, Refusal> {
    let pre = mcp_check::check_host(t.host, home, release).map_err(Refusal::Failed)?;
    if !addable(t, &pre, home) {
        return Err(Refusal::NotAllowed("something of Delphi's is already present, and connect only adds what is absent; see `delphi connect claude-code --status`".into()));
    }
    let tmp = run_dir(delphi_home).map_err(Refusal::Failed)?;
    let r = Run { exe, home, tmp: &tmp };
    let steps = connect_cmd::argv(&exe.canonical, &t.add);
    let mut done = 0;
    let mut err = None;
    for s in &steps {
        match r.step(proof, s) {
            Ok(()) => done += 1,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if let Some(e) = err {
        let (kind, msg) = if done == 0 {
            ("failed", format!("{e}. Nothing was added. Manual commands: {}", t.manual_add))
        } else {
            ("partial", format!("partial: the marketplace was added but the plugin was not installed ({e}). Cure: claude plugin install delphi@delphi (or undo with: claude plugin marketplace remove delphi)"))
        };
        audit(delphi_home, t, "add", exe, kind);
        return Err(if done == 0 { Refusal::Failed(msg) } else { Refusal::Partial(msg) });
    }
    let post = mcp_check::check_host(t.host, home, release).map_err(Refusal::Failed)?;
    audit(delphi_home, t, "add", exe, post.state.name());
    if post.state != State::Configured {
        return Err(Refusal::Partial(format!("the commands ran, but the check reads {}: {}. {}", post.state.name(), sanitize(&post.detail), post.fix.as_deref().unwrap_or("see `delphi connect claude-code --status`"))));
    }
    let plugin_release = installed_release(t.host, home).unwrap_or_default();
    let source = marketplace_source(t.host, home).flatten().unwrap_or_default();
    let content = copilot_installed(home).filter(|_| t.host == "copilot").map(|(_, d)| d);
    update_record(delphi_home, |d| {
        d.insert(t.host.into(), json!({"template": t.id, "at": now(), "plugin": "delphi@delphi", "plugin_release": plugin_release, "content_sha256": content, "binary_release": release, "marketplace": {"name": "delphi", "source": source}}));
        Ok(())
    })
    .map_err(|e| Refusal::Partial(format!("connected, but the record could not be written ({e}); `--remove` will not be available")))?;
    Ok(format!("connected: plugin {} installed from {}; this binary is {release}", sanitize(&plugin_release), sanitize(&source)))
}

/// Whether removal may proceed: the record, the plugin and the marketplace identity all still match.
pub fn removable(t: &Template, home: &Path, delphi_home: &Path) -> Result<(), String> {
    let rec = read_record(delphi_home, t.host)?.ok_or_else(|| format!("delphi has no record of connecting {}, so it will not remove anything. Manual commands: {}", t.host, t.manual_remove))?;
    let recorded = rec["marketplace"]["source"].as_str().unwrap_or_default();
    match marketplace_source(t.host, home) {
        Some(Some(s)) if s == recorded && s == SOURCE => {}
        _ => return Err(format!("the host's `delphi` marketplace no longer matches the record. Manual commands: {}", t.manual_remove)),
    }
    if t.host == "copilot" {
        let recorded = rec["content_sha256"].as_str().unwrap_or_default();
        if copilot_installed(home).is_none_or(|(_, d)| d != recorded) {
            return Err(format!("the installed plugin is not the copy connect recorded (it was updated or reinstalled). Manual commands: {}", t.manual_remove));
        }
    }
    if installed_release(t.host, home).is_none() {
        return Err(format!("the plugin is not installed as the record says. Manual commands: {}", t.manual_remove));
    }
    if other_plugins_use_marketplace(t.host, home) {
        return Err(format!("other plugins use the `delphi` marketplace, so it is not removed. Manual commands: {}", t.manual_remove));
    }
    Ok(())
}

pub fn remove_plan(t: &Template, exe: &HostExe) -> String {
    let mut o = format!("delphi connect {} --remove: plan (nothing has been run or written)\n", t.host);
    o.push_str(&format!("host CLI: {} (resolves to {})\n  sha256 {}\n", sanitize(&exe.listed.display().to_string()), sanitize(&exe.canonical.display().to_string()), exe.sha256));
    for (i, a) in connect_cmd::argv(&exe.canonical, &t.remove).iter().enumerate() {
        o.push_str(&format!("step {}: {}\n", i + 1, a.iter().map(|x| sanitize(x)).collect::<Vec<_>>().join(" ")));
    }
    o.push_str("environment: only a fixed safe PATH, HOME, LANG and LC_*, TERM and a private TMPDIR\n");
    o.push_str(&format!("expected writes: {}\n", t.writes));
    o.push_str("only what connect added is removed, and only while the host's state still matches its record; its record is then deleted\n");
    o
}

pub fn remove_confirmation(host: &str, plan: &str) -> String {
    connect_cmd::confirmation(host, plan).replacen("CONNECT", "REMOVE", 1)
}

pub fn remove(t: &Template, proof: &ConsentProof, exe: &HostExe, home: &Path, delphi_home: &Path) -> Result<String, Refusal> {
    removable(t, home, delphi_home).map_err(Refusal::NotAllowed)?;
    let tmp = run_dir(delphi_home).map_err(Refusal::Failed)?;
    let r = Run { exe, home, tmp: &tmp };
    let steps = connect_cmd::argv(&exe.canonical, &t.remove);
    let mut failed = None;
    for s in &steps {
        if let Err(m) = r.step(proof, s) {
            failed = Some(m);
            break;
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if let Some(m) = failed {
        audit(delphi_home, t, "remove", exe, "failed");
        return Err(Refusal::Partial(format!("{m}. Nothing is rolled back. Manual commands: {}", t.manual_remove)));
    }
    audit(delphi_home, t, "remove", exe, "removed");
    if installed_release(t.host, home).is_some() || marketplace_source(t.host, home).is_some() || leftover(t.host, home) {
        return Err(Refusal::Partial(format!("the commands ran but Delphi's entries are still present. Manual commands: {}", t.manual_remove)));
    }
    update_record(delphi_home, |d| {
        d.remove(t.host);
        Ok(())
    })
    .map_err(|e| Refusal::Partial(format!("removed, but the record could not be cleared ({e})")))?;
    Ok("removed: the Delphi plugin and marketplace are gone from Claude Code".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn root(t: &tempfile::TempDir) -> PathBuf {
        std::fs::canonicalize(t.path()).unwrap().parent().unwrap().to_path_buf()
    }

    /// A fake host CLI that records argv and environment and writes the host state the real one writes.
    fn fake_host(t: &tempfile::TempDir, tm: &Template, fail_on: &str) -> HostExe {
        let dir = t.path().join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let rel = env!("CARGO_PKG_VERSION");
        let (on_add, on_install, on_uninstall, on_remove) = if tm.host == "copilot" {
            (
                r#"mkdir -p "$HOME/.copilot"; echo '{"theme":"keep","extraKnownMarketplaces":{"delphi":{"source":{"source":"github","repo":"jessevondoom/delphi"}}}}' > "$HOME/.copilot/settings.json""#.to_string(),
                format!(r#"d="$HOME/.copilot/installed-plugins/delphi/delphi"; mkdir -p "$d/.claude-plugin"; echo '{{"version":"{rel}"}}' > "$d/.claude-plugin/plugin.json"; echo '{{}}' > "$d/.mcp.json"; echo '{{"theme":"keep","extraKnownMarketplaces":{{"delphi":{{"source":{{"source":"github","repo":"jessevondoom/delphi"}}}}}},"enabledPlugins":{{"delphi@delphi":true}}}}' > "$HOME/.copilot/settings.json""#),
                r#"rm -rf "$HOME/.copilot/installed-plugins/delphi"; echo '{"theme":"keep","extraKnownMarketplaces":{"delphi":{"source":{"source":"github","repo":"jessevondoom/delphi"}}},"enabledPlugins":{}}' > "$HOME/.copilot/settings.json""#.to_string(),
                r#"echo '{"theme":"keep","extraKnownMarketplaces":{},"enabledPlugins":{}}' > "$HOME/.copilot/settings.json""#.to_string(),
            )
        } else {
            (
                r#"mkdir -p "$HOME/.claude/plugins"; echo '{"delphi":{"source":{"source":"github","repo":"jessevondoom/delphi"}}}' > "$HOME/.claude/plugins/known_marketplaces.json""#.to_string(),
                format!(r#"echo '{{"version":2,"plugins":{{"delphi@delphi":[{{"scope":"user","version":"{rel}"}}]}}}}' > "$HOME/.claude/plugins/installed_plugins.json"; echo '{{"enabledPlugins":{{"delphi@delphi":true}}}}' > "$HOME/.claude/settings.json""#),
                r#"echo '{"version":2,"plugins":{}}' > "$HOME/.claude/plugins/installed_plugins.json""#.to_string(),
                r#"echo '{}' > "$HOME/.claude/plugins/known_marketplaces.json""#.to_string(),
            )
        };
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> \"$HOME/argv.log\"\nenv | sort > \"$HOME/env.log\"\ncase \"$*\" in *\"{fail_on}\"*) exit 3;; esac\ncase \"$*\" in\n  \"plugin marketplace add jessevondoom/delphi\") {on_add};;\n  \"plugin install delphi@delphi\") {on_install};;\n  \"plugin uninstall delphi@delphi\") {on_uninstall};;\n  \"plugin marketplace remove delphi\") {on_remove};;\nesac\n"
        );
        let p = dir.join(tm.exe);
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        connect_cmd::find_host_above(&[dir], tm.exe, Some(&root(t))).unwrap()
    }

    fn homes(t: &tempfile::TempDir) -> (PathBuf, PathBuf) {
        let h = t.path().join("home");
        let d = t.path().join("dh");
        std::fs::create_dir_all(&h).unwrap();
        std::fs::create_dir_all(&d).unwrap();
        (h, d)
    }

    fn claude() -> &'static Template {
        connect_cmd::template("claude-code").unwrap()
    }

    fn copilot() -> &'static Template {
        connect_cmd::template("copilot").unwrap()
    }

    #[test]
    fn a_connection_runs_the_fixed_argv_with_a_scrubbed_environment_and_records_it_then_removal_is_symmetric() {
        for tm in [claude(), copilot()] {
            let t = tempfile::tempdir().unwrap();
            let (home, dh) = homes(&t);
            let exe = fake_host(&t, tm, "never-matches");
            let out = connect(tm, &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION")).unwrap();
            assert!(out.starts_with("connected"), "{out}");
            assert_eq!(std::fs::read_to_string(home.join("argv.log")).unwrap(), "plugin marketplace add jessevondoom/delphi\nplugin install delphi@delphi\n");
            let env = std::fs::read_to_string(home.join("env.log")).unwrap();
            assert!(!env.contains("GITHUB_TOKEN"), "an unlisted variable is dropped");
            assert!(env.contains(&format!("PATH={SAFE_PATH}")) && env.contains("TMPDIR="));
            let rec = read_record(&dh, tm.host).unwrap().unwrap();
            assert_eq!(rec["marketplace"]["source"], "jessevondoom/delphi");
            assert_eq!(std::fs::metadata(dh.join("operator/connections.json")).unwrap().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(dh.join("operator")).unwrap().mode() & 0o777, 0o700);
            assert!(std::fs::read_to_string(dh.join("operator/connect-audit.jsonl")).unwrap().contains("\"result\":\"configured\""));
            assert!(connect(tm, &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION")).unwrap_err().message().contains("already present"));

            let out = remove(tm, &consent::for_test(), &exe, &home, &dh).unwrap();
            assert!(out.starts_with("removed"), "{out}");
            assert!(read_record(&dh, tm.host).unwrap().is_none());
            assert!(removable(tm, &home, &dh).unwrap_err().contains("no record"));
            if tm.host == "copilot" {
                let s = std::fs::read_to_string(home.join(".copilot/settings.json")).unwrap();
                assert!(s.contains("\"theme\":\"keep\""), "the host's other settings survive: {s}");
            }
        }
    }

    #[test]
    fn a_failed_second_step_is_partial_and_writes_no_record_and_authorises_no_removal() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let exe = fake_host(&t, claude(), "plugin install");
        let e = connect(claude(), &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION")).unwrap_err();
        assert!(matches!(e, Refusal::Partial(_)) && e.message().contains("marketplace remove delphi"));
        assert!(read_record(&dh, "claude-code").unwrap().is_none());
        assert!(removable(claude(), &home, &dh).is_err());
        assert!(std::fs::read_to_string(dh.join("operator/connect-audit.jsonl")).unwrap().contains("\"result\":\"partial\""));
    }

    #[test]
    fn a_changed_host_binary_aborts_before_anything_runs() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let exe = fake_host(&t, claude(), "never-matches");
        std::fs::write(&exe.canonical, "#!/bin/sh\necho swapped > \"$HOME/swapped\"\n").unwrap();
        let e = connect(claude(), &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION")).unwrap_err();
        assert!(e.message().contains("changed after the plan"));
        assert!(!home.join("swapped").exists() && read_record(&dh, "claude-code").unwrap().is_none());
    }

    #[test]
    fn removal_refuses_when_the_marketplace_is_not_ours_or_the_record_is_unsafe() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let exe = fake_host(&t, claude(), "never-matches");
        connect(claude(), &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION")).unwrap();
        std::fs::write(home.join(".claude/plugins/known_marketplaces.json"), r#"{"delphi":{"source":{"source":"github","repo":"someone/else"}}}"#).unwrap();
        assert!(removable(claude(), &home, &dh).unwrap_err().contains("no longer matches"));
        std::fs::write(home.join(".claude/plugins/known_marketplaces.json"), r#"{"delphi":{"source":{"source":"github","repo":"jessevondoom/delphi"}}}"#).unwrap();
        assert!(removable(claude(), &home, &dh).is_ok());
        std::fs::set_permissions(dh.join("operator/connections.json"), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(removable(claude(), &home, &dh).is_err(), "a group-readable record fails closed");
    }

    #[test]
    fn copilot_removal_is_bound_to_the_copy_connect_installed_and_any_leftover_blocks_a_connect() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let exe = fake_host(&t, copilot(), "never-matches");
        connect(copilot(), &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION")).unwrap();
        assert!(removable(copilot(), &home, &dh).is_ok());
        std::fs::write(home.join(".copilot/installed-plugins/delphi/delphi/.mcp.json"), r#"{"edited":true}"#).unwrap();
        assert!(removable(copilot(), &home, &dh).unwrap_err().contains("not the copy connect recorded"));

        let t2 = tempfile::tempdir().unwrap();
        let (home2, dh2) = homes(&t2);
        let exe2 = fake_host(&t2, copilot(), "never-matches");
        std::fs::create_dir_all(home2.join(".copilot/installed-plugins/delphi")).unwrap();
        std::fs::write(home2.join(".copilot/settings.json"), r#"{"enabledPlugins":{"delphi@delphi":false}}"#).unwrap();
        let e = connect(copilot(), &consent::for_test(), &exe2, &home2, &dh2, env!("CARGO_PKG_VERSION")).unwrap_err();
        assert!(e.message().contains("already present") && !home2.join("argv.log").exists(), "a disabled or leftover install is not absent");
    }

    #[test]
    fn the_environment_is_an_allowlist() {
        let vars = ["GITHUB_TOKEN", "DYLD_INSERT_LIBRARIES", "HTTPS_PROXY", "LANG", "LC_ALL", "TERM", "PATH"].map(|k| (k.to_string(), "x".to_string()));
        let e = host_env_from(Path::new("/h"), Path::new("/t"), vars.into_iter());
        assert_eq!(e.iter().filter(|(k, _)| k == "PATH").count(), 1, "the operator's PATH is replaced, not passed on");
        let keys: Vec<&str> = e.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"PATH") && keys.contains(&"HOME") && keys.contains(&"TMPDIR"));
        assert!(keys.iter().all(|k| matches!(*k, "PATH" | "HOME" | "TMPDIR" | "LANG" | "TERM") || k.starts_with("LC_")));
    }

    #[test]
    fn connect_is_refused_without_a_terminal_or_inside_an_agent_seat() {
        assert!(may_run_here().is_err(), "tests run with neither `delphi connect` as argv nor a terminal");
    }
}
