//! Spec 074 C2: the execution path of `delphi connect claude-code` and `--remove`. Reached only through `connect_cmd::cmd`, in the `delphi`
//! binary crate (the engine, MCP server and daemon crates cannot call it), and every run takes a `ConsentProof` that only the `/dev/tty`
//! confirmation can issue. Fixed argv, no shell, an allowlisted environment, a digest recheck before each exec, a hardened record.

use std::io::{BufRead, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use delphi_engine::mcp_check::{self, State};
use serde_json::{json, Value};

use crate::connect_cmd::{self, sanitize, HostExe, Source, Template};

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

/// Spec 074 §10.1 item 2: what a connect does about `~/.local/bin/delphi`, the path the plugin launches. Decided before the plan
/// is shown, so the plan the operator confirms says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Something is already there (the text is the `launchable` line); it is never replaced.
    Present(String),
    /// Nothing is there, and this binary sits at this stable install path: connect links it.
    Make(PathBuf),
    /// Nothing is there, and no stable install path holds this binary (a build tree, or an unusual location).
    Unavailable,
}

pub fn link_for(home: &Path, install_dir: Option<&Path>, exe: &Path) -> Link {
    if std::fs::symlink_metadata(home.join(mcp_check::STABLE_REL)).is_ok() {
        return Link::Present(mcp_check::launchable(home, install_dir, exe).line());
    }
    mcp_check::stable_target(home, install_dir, exe).map_or(Link::Unavailable, Link::Make)
}

/// [`link_for`] for the running binary and `$DELPHI_INSTALL_DIR`.
pub fn link_now(home: &Path) -> Link {
    let install = std::env::var_os("DELPHI_INSTALL_DIR").map(PathBuf::from);
    match std::env::current_exe() {
        Ok(exe) => link_for(home, install.as_deref(), &exe),
        Err(_) => Link::Unavailable,
    }
}

/// The plan's line about the launch path.
pub fn link_line(link: &Link, home: &Path) -> String {
    link_line_with(link, home, std::env::var_os("DELPHI_INSTALL_DIR").map(PathBuf::from).as_deref())
}

/// [`link_line`] with `$DELPHI_INSTALL_DIR` given. For a link that would be made it says where the target came from (an
/// environment variable can name the directory, so the operator sees that it did) and any caution about the resolved path,
/// before consent; and that the link stays.
fn link_line_with(link: &Link, home: &Path, install_dir: Option<&Path>) -> String {
    let stable = sanitize(&home.join(mcp_check::STABLE_REL).display().to_string());
    if let Link::Make(t) = link {
        let origin = match install_dir.filter(|d| t.starts_with(d)) {
            Some(d) => format!("the directory DELPHI_INSTALL_DIR names in this shell, {}", sanitize(&d.display().to_string())),
            None => "a standard install location".to_string(),
        };
        let caution = mcp_check::path_caution(t).map(|c| format!(" CAUTION: {}, so someone else on this machine could change what the plugin runs.", sanitize(&c))).unwrap_or_default();
        return format!(
            "launch path: the plugin runs {stable}, and nothing is there now; this would make it a link to {} ({origin}).{caution} The link stays if a later step fails, and `--remove` does not take it away: every host's plugin runs it\n",
            sanitize(&t.display().to_string())
        );
    }
    match link {
        Link::Present(line) => format!("launch path: the plugin runs {stable}; launchable: {}; nothing there is changed\n", sanitize(line)),
        Link::Make(_) => unreachable!("handled above"),
        Link::Unavailable => format!("launch path: the plugin runs {stable}, and nothing is there now; no link is made, because this binary is not in a stable install location. The plugin will not start Delphi until that path exists (install with install.sh, or link your installed `delphi` there)\n"),
    }
}

/// Make the link a confirmed plan named, and audit the attempt (spec 074 requirement 13). `Ok(true)` when a link was made.
/// Never replaces anything: if something appeared there since the plan, it fails.
fn planned_link(proof: &ConsentProof, link: &Link, home: &Path, delphi_home: &Path, t: &Template, exe: &HostExe) -> Result<bool, String> {
    if !matches!(link, Link::Make(_)) {
        return Ok(false);
    }
    let r = make_link(proof, link, home);
    audit(delphi_home, t, "link", exe, if r.is_ok() { "made" } else { "failed" });
    r.map(|()| true)
}

fn make_link(_proof: &ConsentProof, link: &Link, home: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    let Link::Make(target) = link else { return Ok(()) };
    let stable = home.join(mcp_check::STABLE_REL);
    let dir = stable.parent().ok_or_else(|| "the launch path has no parent".to_string())?;
    std::fs::DirBuilder::new().recursive(true).mode(0o755).create(dir).map_err(|e| format!("{} could not be made: {e}", sanitize(&dir.display().to_string())))?;
    std::os::unix::fs::symlink(target, &stable).map_err(|e| format!("the link at {} could not be made ({e}); nothing was run", sanitize(&stable.display().to_string())))
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

/// The source an earlier connect recorded for this host (spec 074 §11 item 3): the operator consented to it then, so
/// `--update`, `--remove` and `--status` take it without `--source` being typed again. `None` when there is no readable record.
pub fn recorded_source(delphi_home: &Path, host: &str) -> Option<Source> {
    let rec = read_record(delphi_home, host).ok()??;
    let repo = rec["marketplace"]["source"].as_str()?;
    let arg = match rec["marketplace"]["ref"].as_str() {
        Some(r) => format!("{repo}#{r}"),
        None => repo.to_string(),
    };
    Source::parse(&arg).ok()
}

/// `--status`: the repository the host's `delphi` marketplace names, against the one expected.
pub fn source_line(t: &Template, home: &Path, expected: &Source) -> String {
    match marketplace_source(t.host, home) {
        None => format!("no `delphi` marketplace in {}; a connect would add {}", t.host, expected.arg()),
        Some(Some(repo)) if repo == expected.repo && expected.is_builtin() => repo,
        Some(Some(repo)) if repo == expected.repo => format!("{repo} (not this build's own repository, {})", connect_cmd::SOURCE),
        Some(Some(repo)) => format!("{repo}, where {} was expected; name it with `--source {repo}` if it is the one you mean", expected.repo),
        Some(None) => "a `delphi` marketplace that is not a GitHub repository".to_string(),
    }
}

/// [`connect_from`] for this build's own repository.
#[cfg(test)]
pub fn connect(t: &Template, proof: &ConsentProof, exe: &HostExe, home: &Path, delphi_home: &Path, release: &str, link: &Link) -> Result<String, Refusal> {
    connect_from(t, proof, exe, home, delphi_home, release, link, &Source::default())
}

/// Add the marketplace and the plugin. The caller has already shown the plan and obtained `proof`.
#[allow(clippy::too_many_arguments)]
pub fn connect_from(t: &Template, proof: &ConsentProof, exe: &HostExe, home: &Path, delphi_home: &Path, release: &str, link: &Link, source: &Source) -> Result<String, Refusal> {
    let pre = mcp_check::check_host(t.host, home, release).map_err(Refusal::Failed)?;
    if !addable(t, &pre, home) {
        return Err(Refusal::NotAllowed("something of Delphi's is already present, and connect only adds what is absent; see `delphi connect claude-code --status`".into()));
    }
    // The link first: the plugin about to be installed launches it, and a failure here leaves the host untouched.
    let linked = planned_link(proof, link, home, delphi_home, t, exe).map_err(Refusal::Failed)?;
    let kept = if linked { " The link at ~/.local/bin/delphi was made and stays." } else { "" };
    let tmp = run_dir(delphi_home).map_err(Refusal::Failed)?;
    let r = Run { exe, home, tmp: &tmp };
    let steps = connect_cmd::add_argv(&exe.canonical, t, source);
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
            ("failed", format!("{e}. No marketplace or plugin was added.{kept} Manual commands: {}", t.manual_add(source)))
        } else {
            ("partial", format!("partial: the marketplace was added but the plugin was not installed ({e}).{kept} Cure: claude plugin install delphi@delphi (or undo with: claude plugin marketplace remove delphi)"))
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
    // What the host recorded must be what the operator consented to; otherwise nothing is recorded and removal stays manual.
    let recorded = marketplace_source(t.host, home).flatten().unwrap_or_default();
    if recorded != source.repo {
        return Err(Refusal::Partial(format!("the commands ran, but the host's `delphi` marketplace names another repository than the plan did; no record was written. Manual commands to undo: {}", t.manual_remove)));
    }
    let source = recorded;
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
        // The record was written after the operator confirmed a plan naming this source, so the record is the identity.
        Some(Some(s)) if !recorded.is_empty() && s == recorded => {}
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

/// Spec 074 §10.2 item 3: whether an update may run. The plugin must be installed and the host's `delphi` marketplace must be
/// ours. No connection record is asked for: an update removes nothing, and a plugin installed by hand is the common case.
#[cfg(test)]
pub fn updatable(t: &Template, home: &Path) -> Result<(), String> {
    updatable_from(t, home, &Source::default())
}

/// [`updatable`] against the source the operator named, an earlier connect recorded, or this build's own.
pub fn updatable_from(t: &Template, home: &Path, source: &Source) -> Result<(), String> {
    if t.update.is_empty() {
        return Err(format!("no update commands are recorded for {}", t.host));
    }
    if installed_release(t.host, home).is_none() {
        return Err(format!("the Delphi plugin is not installed in {}, so there is nothing to update; `delphi connect {}` adds it", t.host, t.host));
    }
    match marketplace_source(t.host, home) {
        Some(Some(s)) if s == source.repo => Ok(()),
        Some(Some(s)) => Err(format!(
            "the host's `delphi` marketplace is {}, not {}, so connect will not update from it. If that is the repository you mean to follow, say so: `delphi connect {} --update --source {}`. Manual commands: {}",
            sanitize(&s),
            sanitize(&source.repo),
            t.host,
            sanitize(&s),
            t.manual_update
        )),
        _ => Err(format!("the host's `delphi` marketplace is not a GitHub repository, so connect will not update from it. Manual commands: {}", t.manual_update)),
    }
}

/// Whether connect's record still describes the copy the host has installed: the release it recorded, and for Copilot the
/// content digest. Only such a record may be rewritten by an update, or a hand-reinstalled copy would become removable.
fn record_describes_installed(t: &Template, rec: &Value, home: &Path) -> bool {
    let release_matches = installed_release(t.host, home).is_some_and(|r| rec["plugin_release"].as_str() == Some(r.as_str()));
    if t.host == "copilot" {
        return copilot_installed(home).is_some_and(|(_, d)| rec["content_sha256"].as_str() == Some(d.as_str()));
    }
    release_matches
}

/// The plan of an update: everything the operator would consent to.
#[cfg(test)]
pub fn update_plan(t: &Template, exe: &HostExe, pre: &mcp_check::Check, link: &Link, home: &Path) -> String {
    update_plan_from(t, exe, pre, link, home, &Source::default())
}

pub fn update_plan_from(t: &Template, exe: &HostExe, pre: &mcp_check::Check, link: &Link, home: &Path, source: &Source) -> String {
    let mut o = format!("delphi connect {} --update: plan (nothing has been run or written)\n", t.host);
    o.push_str(&format!("host CLI: {} (resolves to {})\n", sanitize(&exe.listed.display().to_string()), sanitize(&exe.canonical.display().to_string())));
    o.push_str(&format!("  owner uid {}, mode {:o}, sha256 {}\n", exe.uid, exe.mode, exe.sha256));
    o.push_str(&format!("  the host version is not probed (that would run a program you have not consented to); the argv below was proven on {}\n", t.proven_update));
    for (i, a) in connect_cmd::argv(&exe.canonical, t.update).iter().enumerate() {
        o.push_str(&format!("step {}: {}\n", i + 1, a.iter().map(|x| sanitize(x)).collect::<Vec<_>>().join(" ")));
    }
    o.push_str("working directory: a private temporary directory made at run time\n");
    o.push_str("environment: only a fixed safe PATH, HOME, LANG and LC_*, TERM and a private TMPDIR; proxy, token, loader, Git, npm, Python, Ruby and XDG variables are dropped\n");
    o.push_str(&format!("source: the host's own `delphi` marketplace ({}, a GitHub repository), fetched by {} now; the release it publishes is what gets installed (no commit or version is pinned: spec 071 D-5(B))\n", sanitize(&source.repo), t.exe));
    if !source.is_builtin() {
        o.push_str(&format!("WARNING: {} is not the repository this build of Delphi installs from ({}). The skill text, hooks and server declaration that steer {} come from it. Go on only if you trust everyone who can push there\n", sanitize(&source.repo), connect_cmd::SOURCE, t.exe));
    }
    o.push_str(&format!("expected writes: {}\n", t.writes));
    o.push_str(&link_line(link, home));
    o.push_str("network: github.com, as the host decides\n");
    o.push_str(&format!("state found now: {}: {}\n", pre.state.name(), sanitize(&pre.detail)));
    o.push_str("this would update: the installed `delphi@delphi` plugin, to the release the marketplace publishes\n");
    o.push_str("nothing is removed; if connect made a record of this host and it still describes the installed copy, it is rewritten to the updated copy (otherwise it is left, and `--remove` keeps refusing)\n");
    o.push_str("failure: there is no automatic rollback; the host's own message is not shown, only its exit status, with the manual cure\n");
    o
}

pub fn update_confirmation(host: &str, plan: &str) -> String {
    connect_cmd::confirmation(host, plan).replacen("CONNECT", "UPDATE", 1)
}

/// Run the host's own update of the installed plugin (spec 074 §10.2). The caller has shown the plan and obtained `proof`.
#[cfg(test)]
pub fn update(t: &Template, proof: &ConsentProof, exe: &HostExe, home: &Path, delphi_home: &Path, release: &str, link: &Link) -> Result<String, Refusal> {
    update_from(t, proof, exe, home, delphi_home, release, link, &Source::default())
}

/// [`update`] against a named source (spec 074 §11).
#[allow(clippy::too_many_arguments)]
pub fn update_from(t: &Template, proof: &ConsentProof, exe: &HostExe, home: &Path, delphi_home: &Path, release: &str, link: &Link, source: &Source) -> Result<String, Refusal> {
    updatable_from(t, home, source).map_err(Refusal::NotAllowed)?;
    // Read before anything runs: the record may follow the update only if it described the copy that was installed (the
    // removal binding of §9). A record that is unreadable stops here, with the host untouched.
    let record = read_record(delphi_home, t.host).map_err(|e| Refusal::NotAllowed(format!("the connection record is unreadable ({e}); nothing was run")))?;
    let record_was_true = record.as_ref().is_some_and(|rec| record_describes_installed(t, rec, home));
    let linked = planned_link(proof, link, home, delphi_home, t, exe).map_err(Refusal::Failed)?;
    let kept = if linked { " The link at ~/.local/bin/delphi was made and stays." } else { "" };
    let tmp = run_dir(delphi_home).map_err(Refusal::Failed)?;
    let r = Run { exe, home, tmp: &tmp };
    let mut failed = None;
    for s in &connect_cmd::argv(&exe.canonical, t.update) {
        if let Err(m) = r.step(proof, s) {
            failed = Some(m);
            break;
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if let Some(m) = failed {
        audit(delphi_home, t, "update", exe, "failed");
        return Err(Refusal::Failed(format!("{m}. Nothing is rolled back.{kept} Manual commands: {}", t.manual_update)));
    }
    let post = mcp_check::check_host(t.host, home, release).map_err(Refusal::Failed)?;
    audit(delphi_home, t, "update", exe, post.state.name());
    let plugin_release = installed_release(t.host, home).unwrap_or_default();
    // The record follows the update only when it was true before it and the host is in a state an update can end in.
    let settled = matches!(post.state, State::Configured | State::Stale);
    let record_note = match (&record, record_was_true && settled) {
        (None, _) | (Some(_), true) => "",
        (Some(_), false) => " Connect's record was left as it was, because the copy installed before this update was not the one it recorded (or the update did not settle), so `--remove` still refuses.",
    };
    if record_was_true && settled {
        let content = copilot_installed(home).filter(|_| t.host == "copilot").map(|(_, d)| d);
        update_record(delphi_home, |d| {
            if let Some(Value::Object(rec)) = d.get_mut(t.host) {
                rec.insert("plugin_release".into(), json!(plugin_release));
                rec.insert("content_sha256".into(), json!(content));
                rec.insert("binary_release".into(), json!(release));
                rec.insert("updated_at".into(), json!(now()));
            }
            Ok(())
        })
        .map_err(|e| Refusal::Partial(format!("updated, but the record could not be rewritten ({e}); `--remove` may refuse")))?;
    }
    match post.state {
        State::Configured => Ok(format!("updated: {}; this binary is {release}.{record_note}", sanitize(&post.detail))),
        // The host ran cleanly and the plugin is still older: the marketplace has not published this binary's release.
        State::Stale => Ok(format!("the host's update ran, and the plugin is still behind this binary ({}): the `delphi` marketplace has not published release {release} yet.{record_note}", sanitize(&post.detail))),
        _ => Err(Refusal::Partial(format!("the commands ran, but the check reads {}: {}. {}", post.state.name(), sanitize(&post.detail), post.fix.as_deref().map(sanitize).unwrap_or_else(|| format!("see `delphi connect {} --status`", t.host))))),
    }
}

pub fn remove_plan(t: &Template, exe: &HostExe) -> String {
    let mut o = format!("delphi connect {} --remove: plan (nothing has been run or written)\n", t.host);
    o.push_str(&format!("host CLI: {} (resolves to {})\n  sha256 {}\n", sanitize(&exe.listed.display().to_string()), sanitize(&exe.canonical.display().to_string()), exe.sha256));
    for (i, a) in connect_cmd::argv(&exe.canonical, &t.remove).iter().enumerate() {
        o.push_str(&format!("step {}: {}\n", i + 1, a.iter().map(|x| sanitize(x)).collect::<Vec<_>>().join(" ")));
    }
    o.push_str("environment: only a fixed safe PATH, HOME, LANG and LC_*, TERM and a private TMPDIR\n");
    o.push_str(&format!("expected writes: {}\n", t.writes));
    o.push_str("only the plugin and the marketplace connect added are removed, and only while the host's state still matches its record; its record is then deleted\n");
    o.push_str("a link at ~/.local/bin/delphi is left where it is, whoever made it: every host's plugin runs it\n");
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
        // An update installs this release again; Copilot's copy also changes, so its content digest moves.
        let on_update = if tm.host == "copilot" { format!(r#"{on_install}; echo '{{"updated":true}}' > "$HOME/.copilot/installed-plugins/delphi/delphi/.mcp.json""#) } else { on_install.clone() };
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> \"$HOME/argv.log\"\nenv | sort > \"$HOME/env.log\"\ncase \"$*\" in *\"{fail_on}\"*) exit 3;; esac\ncase \"$*\" in\n  \"plugin marketplace add jessevondoom/delphi\") {on_add};;\n  \"plugin install delphi@delphi\") {on_install};;\n  \"plugin update delphi@delphi\") {on_update};;\n  \"plugin marketplace update delphi\") :;;\n  \"plugin uninstall delphi@delphi\") {on_uninstall};;\n  \"plugin marketplace remove delphi\") {on_remove};;\nesac\n"
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
            let out = connect(tm, &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION"), &Link::Present(String::new())).unwrap();
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
            assert!(connect(tm, &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION"), &Link::Present(String::new())).unwrap_err().message().contains("already present"));

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
        let e = connect(claude(), &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION"), &Link::Present(String::new())).unwrap_err();
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
        let e = connect(claude(), &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION"), &Link::Present(String::new())).unwrap_err();
        assert!(e.message().contains("changed after the plan"));
        assert!(!home.join("swapped").exists() && read_record(&dh, "claude-code").unwrap().is_none());
    }

    #[test]
    fn removal_refuses_when_the_marketplace_is_not_ours_or_the_record_is_unsafe() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let exe = fake_host(&t, claude(), "never-matches");
        connect(claude(), &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION"), &Link::Present(String::new())).unwrap();
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
        connect(copilot(), &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION"), &Link::Present(String::new())).unwrap();
        assert!(removable(copilot(), &home, &dh).is_ok());
        std::fs::write(home.join(".copilot/installed-plugins/delphi/delphi/.mcp.json"), r#"{"edited":true}"#).unwrap();
        assert!(removable(copilot(), &home, &dh).unwrap_err().contains("not the copy connect recorded"));

        let t2 = tempfile::tempdir().unwrap();
        let (home2, dh2) = homes(&t2);
        let exe2 = fake_host(&t2, copilot(), "never-matches");
        std::fs::create_dir_all(home2.join(".copilot/installed-plugins/delphi")).unwrap();
        std::fs::write(home2.join(".copilot/settings.json"), r#"{"enabledPlugins":{"delphi@delphi":false}}"#).unwrap();
        let e = connect(copilot(), &consent::for_test(), &exe2, &home2, &dh2, env!("CARGO_PKG_VERSION"), &Link::Present(String::new())).unwrap_err();
        assert!(e.message().contains("already present") && !home2.join("argv.log").exists(), "a disabled or leftover install is not absent");
    }

    /// Spec 074 §10.1 item 2: the link is made before the host runs, only where nothing is, and a failure runs nothing.
    #[test]
    fn a_connect_makes_the_planned_link_first_and_never_replaces_what_is_there() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let exe = fake_host(&t, claude(), "never-matches");
        let target = t.path().join("brew/bin/delphi");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "x").unwrap();
        let stable = home.join(mcp_check::STABLE_REL);
        connect(claude(), &consent::for_test(), &exe, &home, &dh, env!("CARGO_PKG_VERSION"), &Link::Make(target.clone())).unwrap();
        assert_eq!(std::fs::read_link(&stable).unwrap(), target, "the link the plan named");
        let line = link_line_with(&Link::Make(target.clone()), &home, None);
        assert!(line.contains("this would make it a link to") && line.contains("a standard install location") && line.contains("`--remove` does not take it away"), "{line}");
        let named = link_line_with(&Link::Make(target.clone()), &home, target.parent());
        assert!(named.contains("the directory DELPHI_INSTALL_DIR names in this shell"), "an environment variable chose the directory, and the plan says so: {named}");
        std::fs::set_permissions(target.parent().unwrap(), std::fs::Permissions::from_mode(0o775)).unwrap();
        let warned = link_line_with(&Link::Make(target.clone()), &home, None);
        assert!(warned.contains("CAUTION:") && warned.contains("writable by group or others"), "the caution is shown before consent: {warned}");
        std::fs::set_permissions(target.parent().unwrap(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let audit = std::fs::read_to_string(dh.join("operator/connect-audit.jsonl")).unwrap();
        assert!(audit.lines().any(|l| l.contains("\"action\":\"link\"") && l.contains("\"result\":\"made\"")), "the link is audited: {audit}");
        assert!(remove_plan(claude(), &exe).contains("a link at ~/.local/bin/delphi is left"));
        assert!(matches!(link_for(&home, None, &target), Link::Present(_)), "once something is there, a later plan changes nothing");

        // Something appeared there after the plan was shown: the link fails and the host is never run.
        let t2 = tempfile::tempdir().unwrap();
        let (home2, dh2) = homes(&t2);
        let exe2 = fake_host(&t2, claude(), "never-matches");
        std::fs::create_dir_all(home2.join(".local/bin")).unwrap();
        std::fs::write(home2.join(mcp_check::STABLE_REL), "mine").unwrap();
        let e = connect(claude(), &consent::for_test(), &exe2, &home2, &dh2, env!("CARGO_PKG_VERSION"), &Link::Make(target)).unwrap_err();
        assert!(matches!(e, Refusal::Failed(_)) && e.message().contains("could not be made"), "{}", e.message());
        assert_eq!(std::fs::read_to_string(home2.join(mcp_check::STABLE_REL)).unwrap(), "mine", "never replaced");
        assert!(!home2.join("argv.log").exists(), "the host CLI was not run");
        let audit2 = std::fs::read_to_string(dh2.join("operator/connect-audit.jsonl")).unwrap();
        assert!(audit2.lines().any(|l| l.contains("\"action\":\"link\"") && l.contains("\"result\":\"failed\"")), "a failed link is audited too: {audit2}");

        // A link that was made stays when the host then fails, and the message says so.
        let t3 = tempfile::tempdir().unwrap();
        let (home3, dh3) = homes(&t3);
        let failing = fake_host(&t3, claude(), "plugin marketplace add");
        let target3 = t3.path().join("brew/bin/delphi");
        std::fs::create_dir_all(target3.parent().unwrap()).unwrap();
        std::fs::write(&target3, "x").unwrap();
        let e = connect(claude(), &consent::for_test(), &failing, &home3, &dh3, env!("CARGO_PKG_VERSION"), &Link::Make(target3)).unwrap_err();
        assert!(e.message().contains("No marketplace or plugin was added") && e.message().contains("was made and stays"), "{}", e.message());
        assert!(std::fs::symlink_metadata(home3.join(mcp_check::STABLE_REL)).is_ok());
        // With no stable install path there is nothing to link, and the plan says the plugin will not start.
        assert!(link_line(&Link::Unavailable, &home2).contains("no link is made"));
    }

    /// Spec 074 §10.2: an update runs the host's own fixed argv for a plugin installed by hand, needs no record and writes none.
    #[test]
    fn an_update_runs_the_fixed_argv_for_a_hand_installed_plugin_and_writes_no_record() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let exe = fake_host(&t, claude(), "never-matches");
        std::fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        std::fs::write(home.join(".claude/plugins/known_marketplaces.json"), r#"{"delphi":{"source":{"source":"github","repo":"jessevondoom/delphi"}}}"#).unwrap();
        std::fs::write(home.join(".claude/plugins/installed_plugins.json"), r#"{"version":2,"plugins":{"delphi@delphi":[{"scope":"user","version":"0.0.1"}]}}"#).unwrap();
        std::fs::write(home.join(".claude/settings.json"), r#"{"enabledPlugins":{"delphi@delphi":true}}"#).unwrap();
        let release = env!("CARGO_PKG_VERSION");
        let pre = mcp_check::check_host("claude-code", &home, release).unwrap();
        assert_eq!(pre.state, State::Stale);
        let plan = update_plan(claude(), &exe, &pre, &Link::Present("yes (x)".into()), &home);
        assert!(plan.contains("--update: plan") && plan.contains("plugin update delphi@delphi") && plan.contains("nothing is removed") && plan.contains("launch path:"), "{plan}");
        assert!(plan.contains("proven on Claude Code 2.1.325") && !plan.contains("2.1.314"), "the plan names the proof that was made for this row: {plan}");
        assert!(update_confirmation("claude-code", &plan).starts_with("UPDATE claude-code "));

        let out = update(claude(), &consent::for_test(), &exe, &home, &dh, release, &Link::Present(String::new())).unwrap();
        assert!(out.starts_with("updated: ") && out.contains(release), "{out}");
        assert_eq!(std::fs::read_to_string(home.join("argv.log")).unwrap(), "plugin update delphi@delphi\n", "one fixed command, nothing else");
        let env = std::fs::read_to_string(home.join("env.log")).unwrap();
        assert!(env.contains(&format!("PATH={SAFE_PATH}")) && !env.contains("GITHUB_TOKEN"));
        assert!(read_record(&dh, "claude-code").unwrap().is_none(), "an update of a hand-installed plugin makes no record, so it authorises no removal");
        assert!(std::fs::read_to_string(dh.join("operator/connect-audit.jsonl")).unwrap().contains("\"action\":\"update\""));
    }

    #[test]
    fn an_update_is_refused_for_an_absent_plugin_or_a_foreign_marketplace_before_anything_runs() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let exe = fake_host(&t, claude(), "never-matches");
        let release = env!("CARGO_PKG_VERSION");
        let e = update(claude(), &consent::for_test(), &exe, &home, &dh, release, &Link::Unavailable).unwrap_err();
        assert!(matches!(e, Refusal::NotAllowed(_)) && e.message().contains("not installed") && e.message().contains("`delphi connect claude-code` adds it"), "{}", e.message());
        std::fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        std::fs::write(home.join(".claude/plugins/installed_plugins.json"), r#"{"version":2,"plugins":{"delphi@delphi":[{"scope":"user","version":"0.0.1"}]}}"#).unwrap();
        std::fs::write(home.join(".claude/plugins/known_marketplaces.json"), r#"{"delphi":{"source":{"source":"github","repo":"someone/else"}}}"#).unwrap();
        let e = update(claude(), &consent::for_test(), &exe, &home, &dh, release, &Link::Unavailable).unwrap_err();
        assert!(matches!(e, Refusal::NotAllowed(_)) && e.message().contains("is someone/else, not") && e.message().contains("--update --source someone/else"), "{}", e.message());
        // Spec 074 §11: the same marketplace is updatable once the operator names it.
        assert!(updatable_from(claude(), &home, &Source::parse("someone/else").unwrap()).is_ok());
        let pre = mcp_check::check_host("claude-code", &home, release).unwrap();
        let plan = update_plan_from(claude(), &exe, &pre, &Link::Unavailable, &home, &Source::parse("someone/else").unwrap());
        assert!(plan.contains("WARNING: someone/else is not the repository this build"), "{plan}");
        std::fs::write(home.join(".claude/plugins/known_marketplaces.json"), r#"{"delphi":{"source":{"source":"directory","path":"/somewhere"}}}"#).unwrap();
        assert!(updatable(claude(), &home).is_err(), "a marketplace that is not a GitHub source is not ours either");
        assert!(!home.join("argv.log").exists(), "the host CLI was never run");
    }

    #[test]
    fn an_update_rewrites_the_record_so_removal_still_recognises_the_copy_and_a_failed_step_is_reported() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let exe = fake_host(&t, copilot(), "never-matches");
        let release = env!("CARGO_PKG_VERSION");
        connect(copilot(), &consent::for_test(), &exe, &home, &dh, release, &Link::Present(String::new())).unwrap();
        let before = read_record(&dh, "copilot").unwrap().unwrap()["content_sha256"].clone();
        let out = update(copilot(), &consent::for_test(), &exe, &home, &dh, release, &Link::Present(String::new())).unwrap();
        assert!(out.starts_with("updated: "), "{out}");
        assert!(std::fs::read_to_string(home.join("argv.log")).unwrap().ends_with("plugin marketplace update delphi\nplugin update delphi@delphi\n"));
        let rec = read_record(&dh, "copilot").unwrap().unwrap();
        assert_ne!(rec["content_sha256"], before, "the record follows the updated copy");
        assert!(rec["updated_at"].is_i64() && rec["marketplace"]["source"] == "jessevondoom/delphi");
        assert!(removable(copilot(), &home, &dh).is_ok(), "removal still recognises what is installed");

        // The removal binding holds through an update: a copy reinstalled by hand is not the one connect recorded, so the
        // record is left alone and `--remove` keeps refusing (Panel t_0001, P0).
        std::fs::write(home.join(".copilot/installed-plugins/delphi/delphi/.mcp.json"), r#"{"reinstalled":"by hand"}"#).unwrap();
        assert!(removable(copilot(), &home, &dh).is_err());
        let rec_then = read_record(&dh, "copilot").unwrap();
        let out = update(copilot(), &consent::for_test(), &exe, &home, &dh, release, &Link::Present(String::new())).unwrap();
        assert!(out.contains("Connect's record was left as it was"), "{out}");
        assert_eq!(read_record(&dh, "copilot").unwrap(), rec_then.clone(), "the record was not rewritten");
        // Removal now depends only on whether what the host installed is byte-identical to what connect recorded earlier; the
        // record was never bent to fit the hand-installed copy.
        assert_eq!(read_record(&dh, "copilot").unwrap().unwrap()["content_sha256"], rec_then.unwrap()["content_sha256"]);

        // A host that fails: nothing is rewritten, and the manual cure is named.
        let t2 = tempfile::tempdir().unwrap();
        let (home2, dh2) = homes(&t2);
        let ok = fake_host(&t2, copilot(), "never-matches");
        connect(copilot(), &consent::for_test(), &ok, &home2, &dh2, release, &Link::Present(String::new())).unwrap();
        let rec_before = read_record(&dh2, "copilot").unwrap();
        let failing = fake_host(&t2, copilot(), "plugin update");
        let e = update(copilot(), &consent::for_test(), &failing, &home2, &dh2, release, &Link::Present(String::new())).unwrap_err();
        assert!(matches!(e, Refusal::Failed(_)) && e.message().contains("copilot plugin update delphi@delphi"), "{}", e.message());
        assert_eq!(read_record(&dh2, "copilot").unwrap(), rec_before);
        let audit = std::fs::read_to_string(dh2.join("operator/connect-audit.jsonl")).unwrap();
        assert!(audit.lines().any(|l| l.contains("\"action\":\"update\"") && l.contains("\"result\":\"failed\"")), "{audit}");
    }

    /// Spec 074 §11: a connect from a source the operator named runs that one source, records it, and the record then
    /// stands for it in an update and a removal without the flag.
    #[test]
    fn a_named_source_is_what_runs_is_recorded_and_is_what_update_and_remove_then_expect() {
        let t = tempfile::tempdir().unwrap();
        let (home, dh) = homes(&t);
        let release = env!("CARGO_PKG_VERSION");
        // A fake host that records whatever source it was given, as the real ones do.
        let dir = t.path().join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let script = r##"#!/bin/sh
echo "$@" >> "$HOME/argv.log"
mkdir -p "$HOME/.claude/plugins"
case "$*" in
  "plugin marketplace add "*)
    repo="${4%%#*}"
    printf '{"delphi":{"source":{"source":"github","repo":"%s"}}}\n' "$repo" > "$HOME/.claude/plugins/known_marketplaces.json";;
  "plugin install delphi@delphi"|"plugin update delphi@delphi")
    echo '{"version":2,"plugins":{"delphi@delphi":[{"scope":"user","version":"@REL@"}]}}' > "$HOME/.claude/plugins/installed_plugins.json"
    echo '{"enabledPlugins":{"delphi@delphi":true}}' > "$HOME/.claude/settings.json";;
  "plugin uninstall delphi@delphi")
    echo '{"version":2,"plugins":{}}' > "$HOME/.claude/plugins/installed_plugins.json";;
  "plugin marketplace remove delphi")
    echo '{}' > "$HOME/.claude/plugins/known_marketplaces.json";;
esac
exit 0
"##
        .replace("@REL@", release);
        std::fs::write(dir.join("claude"), &script).unwrap();
        std::fs::set_permissions(dir.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let exe = connect_cmd::find_host_above(&[dir], "claude", Some(&root(&t))).unwrap();

        let fork = Source::parse("fastly/delphi#stable").unwrap();
        let out = connect_from(claude(), &consent::for_test(), &exe, &home, &dh, release, &Link::Present(String::new()), &fork).unwrap();
        assert!(out.contains("installed from fastly/delphi"), "{out}");
        assert_eq!(std::fs::read_to_string(home.join("argv.log")).unwrap(), "plugin marketplace add fastly/delphi#stable\nplugin install delphi@delphi\n");
        assert_eq!(read_record(&dh, "claude-code").unwrap().unwrap()["marketplace"]["source"], "fastly/delphi");
        assert_eq!(recorded_source(&dh, "claude-code").map(|s| s.repo).as_deref(), Some("fastly/delphi"), "the record stands for the source from now on");
        assert!(source_line(claude(), &home, &recorded_source(&dh, "claude-code").unwrap()).starts_with("fastly/delphi (not this build's own repository"));

        // Without the record's source, this build's own repository is expected, and the fork is refused by name.
        assert!(updatable(claude(), &home).unwrap_err().contains("--update --source fastly/delphi"));
        let rec = recorded_source(&dh, "claude-code").unwrap();
        assert!(update_from(claude(), &consent::for_test(), &exe, &home, &dh, release, &Link::Present(String::new()), &rec).is_ok());
        assert!(removable(claude(), &home, &dh).is_ok(), "removal goes by the record, whatever repository it names");
        assert!(remove(claude(), &consent::for_test(), &exe, &home, &dh).unwrap().starts_with("removed"));
        assert_eq!(recorded_source(&dh, "claude-code"), None);

        // A host that records another repository than the one consented to gets no record.
        let t2 = tempfile::tempdir().unwrap();
        let (home2, dh2) = homes(&t2);
        let dir2 = t2.path().join("bin");
        std::fs::create_dir_all(&dir2).unwrap();
        std::fs::set_permissions(&dir2, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir2.join("claude"), script.replace(r#"repo="${4%%#*}""#, r#"repo="other/place""#)).unwrap();
        std::fs::set_permissions(dir2.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let exe2 = connect_cmd::find_host_above(&[dir2], "claude", Some(&root(&t2))).unwrap();
        let e = connect_from(claude(), &consent::for_test(), &exe2, &home2, &dh2, release, &Link::Present(String::new()), &fork).unwrap_err();
        assert!(matches!(e, Refusal::Partial(_)) && e.message().contains("names another repository than the plan did"), "{}", e.message());
        assert!(read_record(&dh2, "claude-code").unwrap().is_none());
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
