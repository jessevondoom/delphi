//! The single-turn harness bridge (`specs/143` §2.2): one prompt and one answer through the `claude` CLI, with its tools off.
//!
//! [`Engine::one_shot_raw`](crate::engine::Engine) comes here when the key it is given is a harness endpoint. A call is:
//!
//! 1. **A home built empty for the launch** ([`CallHome`]): a fresh directory with two empty directories in it, the CLI's home and
//!    the working directory, under the daemon's own run directory or, when anything on that path to the filesystem root is an
//!    instruction file or a CLI project directory ([`INSTRUCTION_NAMES`]), under the per-user temporary directory. It is removed
//!    on every way out of the call, a cancelled one included (it is removed when dropped).
//! 2. **One launch** ([`launch`], the only place a `Launch` is built for the bridge): `Tools::None`, the deny-all and no-connector
//!    flags of [`crate::harness`]'s single-call arguments, every home variable set to that home, the environment the short
//!    keep-list and nothing else, the subscription token in the child's environment only, the prompt on standard input.
//! 3. **One answer** ([`run`]): the final text. A tool use, a permission request, or a CLI that comes up offering a tool or a
//!    connector ends the call (`the harness asked for a tool in a single call`); nothing is ever granted. Past the timeout the
//!    process group gets `SIGTERM` and, five seconds later, `SIGKILL`.
//!
//! The recipe was checked flag by flag against `claude` 2.1.332 on 2026-11-25 (the spike of slice B, in the slice's tracker file).
//! There is no OS sandbox behind it: the existing sandbox crate has no profile that leaves the network open, and the CLI opens a
//! per-user directory outside its home (the same record).
//!
//! Whether a call may start at all, and when, is not decided here: the ledger, the admission function and the gate are
//! [`crate::job_admission`], and `Engine::one_shot_raw` asks them before it comes here.

use crate::harness::{Cli, Event, Launch, LaunchProfile, Opts, Session, Tools};
use crate::summary::CallError;
use delphi_adapter::credential::Secret;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What counts as an instruction file or a CLI project directory (`specs/143` §7, fixed by the spike): none may lie in any
/// directory from the working directory up to the filesystem root. Closed.
pub const INSTRUCTION_NAMES: [&str; 6] = ["CLAUDE.md", "CLAUDE.local.md", "AGENTS.md", ".claude", ".mcp.json", ".claude.json"];

/// Every variable the CLI finds a home or a configuration directory by. Each names the launch's own home, explicitly.
pub const HOME_VARS: [&str; 5] = ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "CLAUDE_CONFIG_DIR"];

/// The variable the subscription token travels in: the child's environment and nowhere else.
pub const TOKEN_VAR: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// What is kept of the daemon's own environment: the launcher's keep-list (`harness::cli_env_with`) without its home variables,
/// which are overridden. `LC_*` is kept with it. No key, token or cloud credential is on it.
const KEEP: [&str; 7] = ["PATH", "USER", "LOGNAME", "SHELL", "LANG", "TERM", "TMPDIR"];

/// How long a process group has after `SIGTERM` before `SIGKILL` (§2.3 item 4).
pub const KILL_GRACE: Duration = Duration::from_secs(5);

/// The first instruction file or CLI project directory on the path from `dir` to the filesystem root, or `None` when the whole
/// path is clean. The CLI walks up to the root, not to the daemon's home, so the check does too.
pub fn instruction_file_above(dir: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut at = Some(real.as_path());
    while let Some(d) = at {
        for name in INSTRUCTION_NAMES {
            let f = d.join(name);
            if f.symlink_metadata().is_ok() {
                return Some(f);
            }
        }
        at = d.parent();
    }
    None
}

/// Where a launch's home may be built, in the order tried: the daemon's own run directory, then the per-user temporary
/// directory (named by the daemon's home, so two installs never share it).
pub fn roots(daemon_home: &Path) -> Vec<PathBuf> {
    let tag: String = blake3::hash(daemon_home.to_string_lossy().as_bytes()).to_hex().chars().take(12).collect();
    vec![daemon_home.join("run/bridge"), std::env::temp_dir().join(format!("delphi-bridge-{tag}"))]
}

/// The directories of one bridged call. Removed when dropped: after an answer, an error, a timeout, a tool request, and when the
/// job that asked is cancelled.
pub struct CallHome {
    base: PathBuf,
    /// The CLI's home: empty when the CLI starts.
    pub home: PathBuf,
    /// The working directory: empty, and nothing above it up to the root is an instruction file.
    pub work: PathBuf,
}

impl CallHome {
    /// Build the home under the first of `roots` whose path to the filesystem root is clean and which lies outside every directory
    /// of `outside` (the workspace and the projects). `Err` names the file that made the last root unusable.
    pub fn make(roots: &[PathBuf], outside: &[PathBuf]) -> Result<CallHome, String> {
        use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
        static N: AtomicU64 = AtomicU64::new(0);
        let call = format!("{}-{}-{}", std::process::id(), delphi_core::time::now_rfc3339().replace([':', '.'], ""), N.fetch_add(1, SeqCst));
        let mut why = "there is no place to build it".to_string();
        for root in roots {
            let base = root.join(&call);
            let (home, work) = (base.join("home"), base.join("work"));
            if let Err(e) = make_private(&home).and_then(|()| make_private(&work)) {
                why = format!("{}: {}", base.display(), e);
                let _ = std::fs::remove_dir_all(&base);
                continue;
            }
            let made = CallHome { base, home, work };
            let real = std::fs::canonicalize(&made.work).unwrap_or_else(|_| made.work.clone());
            if let Some(inside) = outside.iter().find(|o| real.starts_with(std::fs::canonicalize(o).unwrap_or_else(|_| o.to_path_buf()))) {
                why = format!("{} lies inside {}", made.work.display(), inside.display());
                continue;
            }
            match instruction_file_above(&made.work) {
                None => return Ok(made),
                Some(f) => why = format!("{} is an instruction file or a CLI directory on the path from the working directory to the root", f.display()),
            }
        }
        Err(format!("no clean place for the launch: {why}"))
    }
}

impl CallHome {
    /// Note the process group of the CLI this call started, with the moment its leader started, so that a daemon started after a
    /// crash can end it ([`sweep`]). The moment is what makes the note safe: a process id is reused, a start time with it is not.
    pub fn record_group(&self, pgid: i32) {
        if let Some(started) = proc_start(pgid) {
            let _ = std::fs::write(self.base.join(GROUP_FILE), format!("{pgid} {started}"));
        }
    }
}

/// The note [`CallHome::record_group`] leaves, beside the two directories.
const GROUP_FILE: &str = "group";

/// When process `pid` started, as the kernel tells it (an opaque number: only equality is used), or `None` when there is no such
/// process or the platform does not say.
#[cfg(target_os = "macos")]
pub fn proc_start(pid: i32) -> Option<u64> {
    // SAFETY: `proc_pidinfo` fills at most `size` bytes of the zeroed struct it is given and returns how many it wrote.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let wrote = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut libc::proc_bsdinfo as *mut libc::c_void, size) };
    (wrote == size).then(|| info.pbi_start_tvsec.wrapping_mul(1_000_000).wrapping_add(info.pbi_start_tvusec))
}

#[cfg(target_os = "linux")]
pub fn proc_start(pid: i32) -> Option<u64> {
    // Field 22 of `/proc/<pid>/stat`, counted after the command's closing parenthesis (the command may hold spaces).
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn proc_start(_pid: i32) -> Option<u64> {
    None
}

impl Drop for CallHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn make_private(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// At the daemon's start (§2.3 item 4): end the process groups a crashed daemon recorded as running, and remove the call
/// directories it left behind. A group is ended only when its leader is still the process that was recorded (the same process id
/// **and** the same start time), so a process id the system has since given to something else is never signalled. Returns how
/// many directories were removed and how many groups were ended.
pub fn sweep(roots: &[PathBuf]) -> (usize, usize) {
    let (mut dirs, mut groups) = (0, 0);
    for root in roots {
        for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let note = std::fs::read_to_string(e.path().join(GROUP_FILE)).unwrap_or_default();
            if let Some((pgid, started)) = note.split_once(' ').and_then(|(p, s)| Some((p.parse::<i32>().ok()?, s.trim().parse::<u64>().ok()?))) {
                if pgid > 1 && proc_start(pgid) == Some(started) {
                    // SAFETY: plain signal delivery to a process group this daemon's predecessor created and recorded.
                    unsafe {
                        libc::kill(-pgid, libc::SIGKILL);
                    }
                    groups += 1;
                }
            }
            if std::fs::remove_dir_all(e.path()).is_ok() {
                dirs += 1;
            }
        }
    }
    (dirs, groups)
}

/// The launch of a bridged call. **The one place a `Launch` is built for the bridge**, so what it always has is read here:
/// no tools, the single-call arguments (every deny-all and no-connector flag, `harness::single_call_args`), a fresh session,
/// the prompt on standard input, the empty working directory, every home variable naming the launch's home, the keep-list and the
/// token and nothing else in the environment. `ANTHROPIC_API_KEY` is never in it: left in, it would switch the CLI to API billing.
pub fn launch(program: &str, model: &str, prompt: String, home: &CallHome, token: Option<Secret>) -> Launch {
    let mut env: BTreeMap<String, String> = std::env::vars().filter(|(k, _)| KEEP.contains(&k.as_str()) || k.starts_with("LC_")).collect();
    for var in HOME_VARS {
        env.insert(var.to_string(), home.home.display().to_string());
    }
    Launch {
        cli: Cli::Claude,
        program: program.to_string(),
        model: model.to_string(),
        // The request's system text travels with the contract, as the one user prompt, on standard input: never an argument.
        system_prompt: String::new(),
        user_prompt: prompt,
        cwd: home.work.clone(),
        tools: Tools::None,
        env,
        profile: LaunchProfile { single_call: true, ..Default::default() },
        opts: Opts { oauth_token: token, ..Default::default() },
    }
}

/// What a bridged call brought back.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// The final text, as the CLI gave it: not cut, not redacted.
    pub text: String,
    /// The model the CLI said it served.
    pub served: Option<String>,
    /// The tokens the CLI reported, or an estimate from the prompt and the answer.
    pub tokens: u64,
    /// Of those, the tokens of the answer.
    pub tokens_out: u64,
    pub estimated: bool,
}

/// What a call showed besides its answer, kept also when it fails: for the ledger, the receipt and the endpoint's cooldown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    pub served: Option<String>,
    /// The tokens the CLI reported before the call ended; 0 when it reported none.
    pub tokens: u64,
    /// When the CLI said its quota comes back, in Unix seconds.
    pub resets_at: Option<i64>,
}

/// Run the launch for one answer. Nothing is retried and nothing is granted.
pub async fn run(l: &Launch, timeout: Duration) -> Result<Answer, CallError> {
    run_in(l, None, timeout, &mut Seen::default()).await
}

/// [`run`], noting the CLI's process group in the call's `home` and leaving what the call showed in `seen`.
pub async fn run_in(l: &Launch, home: Option<&CallHome>, timeout: Duration, seen: &mut Seen) -> Result<Answer, CallError> {
    if !l.profile.single_call || l.tools != Tools::None {
        return Err(CallError::Launch);
    }
    let mut s = match Session::start(l).await {
        Ok(s) => s,
        Err(e) => {
            delphi_core::elog::error("harness", "single_call_launch", &e, serde_json::json!({"harness": "claude"}));
            return Err(CallError::Launch);
        }
    };
    if let Some(home) = home {
        home.record_group(s.pgid());
    }
    let deadline = tokio::time::Instant::now() + timeout;
    let (mut text, mut served, mut reported, mut written, mut rejected) = (String::new(), None::<String>, 0u64, 0u64, false);
    let mut ended: Option<Result<String, CallError>> = None;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        // The session's own idle wait kills at once; the bridge's timeout escalates, so it is the one that fires.
        let ev = match tokio::time::timeout(left, s.next(left + Duration::from_secs(3600))).await {
            Ok(ev) => ev,
            Err(_) => {
                s.terminate(KILL_GRACE).await;
                return Err(CallError::Timeout);
            }
        };
        match ev {
            Some(Event::Init { tools, mcp_servers, .. }) if !tools.is_empty() || mcp_servers.as_ref().is_some_and(|m| !m.is_empty()) => {
                // Tools are off by construction; a CLI that comes up offering one, or a connector, is not the launch that was built.
                s.terminate(KILL_GRACE).await;
                return Err(CallError::HarnessTool);
            }
            Some(Event::ToolUse { .. } | Event::Permission(_)) => {
                s.terminate(KILL_GRACE).await;
                return Err(CallError::HarnessTool);
            }
            Some(Event::Served(model)) => {
                seen.served = Some(model.clone());
                served = Some(model);
            }
            Some(Event::Text(t)) => text = t,
            Some(Event::Usage(u)) => {
                reported += u.input + u.cache_read + u.cache_write + u.output;
                written += u.output;
                seen.tokens = reported;
            }
            Some(Event::RateLimit { status, resets_at, .. }) if status == "rejected" => {
                rejected = true;
                // The CLI sends seconds or milliseconds.
                seen.resets_at = resets_at.map(|t| if t > 100_000_000_000 { t / 1000 } else { t });
            }
            Some(Event::Result { text: result, is_error, usage, .. }) => {
                let total = usage.input + usage.cache_read + usage.cache_write + usage.output;
                reported = reported.max(total);
                written = written.max(usage.output);
                seen.tokens = reported;
                ended = Some(if is_error {
                    Err(if rejected {
                        CallError::Quota
                    } else if result.contains("Not logged in") {
                        // The token did not sign the CLI in: the launch could not be made (§2.2 item 4).
                        CallError::Launch
                    } else {
                        CallError::Provider
                    })
                } else {
                    Ok(if result.is_empty() { std::mem::take(&mut text) } else { result })
                });
                s.close_input();
            }
            Some(Event::Exit { .. }) | None => break,
            Some(_) => {}
        }
    }
    let answer = match ended {
        Some(r) => r?,
        // The process ended with no result line.
        None if rejected => return Err(CallError::Quota),
        None => return Err(CallError::Provider),
    };
    if answer.trim().is_empty() {
        return Err(CallError::Empty);
    }
    let estimated = reported == 0;
    let tokens = if estimated { ((l.user_prompt.len() + answer.len()) / 4) as u64 } else { reported };
    let tokens_out = if estimated { (answer.len() / 4) as u64 } else { written };
    Ok(Answer { text: answer, served, tokens, tokens_out, estimated })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in `claude` that records how it was started (its arguments, its environment, its working directory, what its home
    /// and working directory held, its standard input) under `rec`, then runs `body`. The launch scrubs the environment, so the
    /// record's place is written into the script.
    pub(crate) fn recording_cli(dir: &Path, rec: &Path, body: &str) -> String {
        std::fs::create_dir_all(rec).unwrap();
        let p = dir.join("fake-claude");
        let r = rec.display();
        let script = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{r}/args.txt'\nenv > '{r}/env.txt'\npwd -P > '{r}/cwd.txt'\nls -A \"$HOME\" > '{r}/home.txt'\nls -A . > '{r}/work.txt'\nread first\nprintf '%s' \"$first\" > '{r}/stdin.txt'\n{body}\n");
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    }

    pub(crate) const INIT: &str = r#"echo '{"type":"system","subtype":"init","session_id":"s1","tools":[],"mcp_servers":[],"model":"claude-served-1"}'"#;

    pub(crate) fn answers(text: &str) -> String {
        format!("{INIT}\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"{text}\"}}],\"usage\":{{\"input_tokens\":40,\"output_tokens\":2}}}}}}'\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"{text}\",\"usage\":{{\"input_tokens\":40,\"output_tokens\":2}}}}'")
    }

    struct Fx {
        d: tempfile::TempDir,
        rec: PathBuf,
        roots: Vec<PathBuf>,
    }

    fn fx() -> Fx {
        let d = tempfile::tempdir().unwrap();
        let rec = d.path().join("rec");
        let roots = vec![d.path().join("daemon/run/bridge"), d.path().join("tmp/delphi-bridge")];
        Fx { d, rec, roots }
    }

    fn read(f: &Fx, name: &str) -> String {
        std::fs::read_to_string(f.rec.join(name)).unwrap_or_default()
    }

    fn token() -> Option<Secret> {
        Some(Secret::new("sk-fixture-bridge-token-0001".into(), None))
    }

    // The fitness functions "a bridged call never has tools" and "tools are off by construction": the one place a `Launch` is
    // built for the bridge always has `Tools::None`, the single-call profile and every deny-all and no-connector flag, and
    // neither the prompt nor the token is an argument (AC-3, AC-12 d, AC-21).
    #[test]
    fn a_single_call_launch_is_always_tools_none() {
        let f = fx();
        let home = CallHome::make(&f.roots, &[]).unwrap();
        let l = launch("claude", "sonnet", "the prompt text".into(), &home, token());
        assert!(l.tools == Tools::None && l.profile.single_call && l.cli == Cli::Claude);
        assert!(l.system_prompt.is_empty() && l.cwd == home.work);
        let source = include_str!("harness_single.rs").split("\n#[cfg(test)]\npub(crate) mod ").next().unwrap().to_string();
        assert_eq!(source.matches("\n    Launch {").count(), 1, "one place builds a Launch for the bridge");
    }

    #[test]
    fn the_launch_always_denies_every_tool_and_loads_no_connector() {
        let f = fx();
        let home = CallHome::make(&f.roots, &[]).unwrap();
        let l = launch("claude", "sonnet", "the prompt text".into(), &home, token());
        // `harness::args` gives a single-call launch exactly these (tested there), whatever else the launch carries.
        let args = crate::harness::single_call_args(&l.model);
        let has = |flag: &str, value: &str| args.windows(2).any(|w| w[0] == flag && w[1] == value);
        assert!(has("--tools", "") && has("--disallowedTools", "mcp__*") && has("--max-turns", "1") && has("--permission-prompts", "none") && has("--model", "sonnet"), "{args:?}");
        for flag in ["-p", "--safe-mode", "--strict-mcp-config", "--disable-slash-commands", "--no-session-persistence"] {
            assert!(args.iter().any(|a| a == flag), "{flag}: {args:?}");
        }
        for never in ["--bare", "--mcp-config", "--permission-prompt-tool", "--append-system-prompt", "--system-prompt", "--add-dir", "--settings", "--plugin-dir", "--dangerously-skip-permissions", "--allowedTools"] {
            assert!(!args.iter().any(|a| a == never), "{never}: {args:?}");
        }
        assert!(!args.iter().any(|a| a.contains("the prompt text") || a.contains("sk-fixture")), "neither the prompt nor the token is an argument: {args:?}");
    }

    // AC-12 (d), AC-21 and the fitness functions on the home and the environment: every home variable names the home built for
    // this call, which was empty when the CLI started; the environment is the keep-list and the token and nothing else, whatever
    // the daemon's own environment holds; the prompt arrived on standard input; the directories are gone afterwards (AC-3, AC-17).
    #[tokio::test]
    async fn every_home_variable_names_the_launch_home() {
        let f = fx();
        let prog = recording_cli(f.d.path(), &f.rec, &answers("the answer"));
        let home = CallHome::make(&f.roots, &[]).unwrap();
        let (base, home_dir, work_dir) = (home.base.clone(), std::fs::canonicalize(&home.home).unwrap(), std::fs::canonicalize(&home.work).unwrap());
        let mut l = launch(&prog, "sonnet", "what is the answer?".into(), &home, token());
        // An API key the launch options carry (the way a span is given one) never reaches a bridged call either.
        l.opts.api_key = Some(Secret::new("sk-ant-planted".into(), None));
        assert!(l.env.keys().all(|k| KEEP.contains(&k.as_str()) || HOME_VARS.contains(&k.as_str()) || k.starts_with("LC_")), "the launch's environment is built from the keep-list alone: {:?}", l.env.keys().collect::<Vec<_>>());
        let a = run(&l, Duration::from_secs(20)).await.unwrap();
        assert_eq!((a.text.as_str(), a.served.as_deref(), a.tokens, a.estimated), ("the answer", Some("claude-served-1"), 42, false));
        let env: BTreeMap<String, String> = read(&f, "env.txt").lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        for var in HOME_VARS {
            assert_eq!(env.get(var).map(|v| std::fs::canonicalize(v).unwrap_or_default()), Some(home_dir.clone()), "{var}");
        }
        assert_eq!(env.get(TOKEN_VAR).map(String::as_str), Some("sk-fixture-bridge-token-0001"));
        let allowed = |k: &str| KEEP.contains(&k) || HOME_VARS.contains(&k) || k == TOKEN_VAR || k.starts_with("LC_") || ["PWD", "OLDPWD", "SHLVL", "_"].contains(&k);
        assert!(env.keys().all(|k| allowed(k)), "only the keep-list, the home and the token: {:?}", env.keys().collect::<Vec<_>>());
        assert!(!env.contains_key("ANTHROPIC_API_KEY"), "an API key never reaches a bridged call, even one the launch options carry");
        assert_eq!(read(&f, "cwd.txt").trim(), work_dir.display().to_string());
        assert_eq!((read(&f, "home.txt").as_str(), read(&f, "work.txt").as_str()), ("", ""), "both were empty when the CLI started");
        assert!(read(&f, "stdin.txt").contains("what is the answer?") && !read(&f, "args.txt").contains("what is the answer?"));
        assert!(!read(&f, "args.txt").contains("sk-fixture") && !read(&f, "args.txt").contains("--bare"));
        drop(home);
        assert!(!base.exists(), "the call's directories are removed");
    }

    #[tokio::test]
    async fn the_home_is_isolated_and_the_environment_is_the_keep_list() {
        // AC-12 (b): a settings file with a hook, and a connector configuration, planted where the daemon's own `HOME` and
        // `CLAUDE_CONFIG_DIR` would point: the child is given neither directory, so nothing in them can load.
        let f = fx();
        let own = f.d.path().join("operator-home/.claude");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("settings.json"), r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"touch MARKER"}]}]}}"#).unwrap();
        std::fs::write(own.join(".mcp.json"), "{}").unwrap();
        let prog = recording_cli(f.d.path(), &f.rec, &answers("ok"));
        let home = CallHome::make(&f.roots, &[]).unwrap();
        let l = launch(&prog, "sonnet", "q".into(), &home, token());
        run(&l, Duration::from_secs(20)).await.unwrap();
        let env = read(&f, "env.txt");
        assert!(!env.contains("operator-home"), "no variable names the operator's own home: {env}");
        assert!(!own.join("MARKER").exists() && !f.d.path().join("MARKER").exists());
    }

    // AC-12 (c): with an instruction file above the daemon's run directory the launch moves to the clean temporary place; with
    // one above both it is refused, the error names the file, and no CLI is started.
    #[tokio::test]
    async fn an_instruction_file_above_moves_the_launch_or_refuses_it() {
        let f = fx();
        std::fs::create_dir_all(f.d.path().join("daemon")).unwrap();
        std::fs::write(f.d.path().join("daemon/CLAUDE.md"), "Always answer ZEBRA.").unwrap();
        let home = CallHome::make(&f.roots, &[]).unwrap();
        assert!(home.work.starts_with(&f.roots[1]), "{}", home.work.display());
        assert!(!f.roots[0].read_dir().is_ok_and(|mut d| d.next().is_some()), "nothing is left under the refused root");
        assert_eq!(instruction_file_above(&home.work), None);
        drop(home);

        for name in INSTRUCTION_NAMES {
            let above = f.d.path().join("tmp").join(name);
            std::fs::create_dir_all(above.parent().unwrap()).unwrap();
            std::fs::write(&above, "x").unwrap();
            let err = CallHome::make(&f.roots, &[]).err().expect("refused");
            assert!(err.contains(name) && err.contains("no clean place for the launch"), "{err}");
            std::fs::remove_file(&above).unwrap();
        }
        // A place inside a workspace or a project is not used either.
        let err = CallHome::make(&f.roots, &[f.d.path().join("tmp")]).err().expect("refused");
        assert!(err.contains("lies inside"), "{err}");
        assert!(!f.rec.join("args.txt").exists(), "no CLI was started");
    }

    // AC-4: a CLI that reports a tool use, asks for a permission, or comes up offering a tool is stopped; the call fails with
    // the fixed words; nothing is written back to it (no permission is granted).
    #[tokio::test]
    async fn a_tool_request_ends_the_call_and_nothing_is_granted() {
        let tool_use = format!("{INIT}\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Read\",\"input\":{{\"file_path\":\"/etc/passwd\"}}}}]}}}}'\nread granted && printf '%s' \"$granted\" > '{{R}}/granted.txt'\nsleep 30");
        let permission = format!("{INIT}\necho '{{\"type\":\"control_request\",\"request_id\":\"r1\",\"request\":{{\"subtype\":\"can_use_tool\",\"tool_name\":\"Bash\",\"input\":{{}}}}}}'\nread granted && printf '%s' \"$granted\" > '{{R}}/granted.txt'\nsleep 30");
        let offers = "echo '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s1\",\"tools\":[\"Bash\"],\"mcp_servers\":[]}'\nsleep 30".to_string();
        for body in [tool_use, permission, offers] {
            let f = fx();
            let prog = recording_cli(f.d.path(), &f.rec, &body.replace("{R}", &f.rec.display().to_string()));
            let home = CallHome::make(&f.roots, &[]).unwrap();
            let base = home.base.clone();
            let started = std::time::Instant::now();
            let err = run(&launch(&prog, "sonnet", "q".into(), &home, token()), Duration::from_secs(20)).await.unwrap_err();
            assert_eq!(err, CallError::HarnessTool);
            assert_eq!(err.to_string(), "the harness asked for a tool in a single call (harness_tool)");
            assert!(started.elapsed() < Duration::from_secs(15), "stopped, not waited for");
            assert!(!f.rec.join("granted.txt").exists(), "no answer was sent to the request");
            drop(home);
            assert!(!base.exists());
        }
    }

    // AC-6 and AC-17: past the timeout the process group gets SIGTERM, and SIGKILL when it is still there after the grace; the
    // directories are gone either way.
    #[tokio::test]
    async fn a_call_past_the_timeout_is_terminated_then_killed() {
        // One CLI leaves on SIGTERM and says so; one ignores it and is killed.
        for (trap, leaves) in [("trap 'echo term > \"{R}/signal.txt\"; exit 0' TERM", true), ("trap '' TERM", false)] {
            let f = fx();
            let body = format!("{}\n{INIT}\nwhile :; do sleep 1; done", trap.replace("{R}", &f.rec.display().to_string()));
            let prog = recording_cli(f.d.path(), &f.rec, &body);
            let home = CallHome::make(&f.roots, &[]).unwrap();
            let base = home.base.clone();
            let l = launch(&prog, "sonnet", "q".into(), &home, token());
            let started = std::time::Instant::now();
            let mut s = Session::start(&l).await.unwrap();
            let pid = s.pgid();
            drop(s.next(Duration::from_secs(5)).await);
            s.terminate(Duration::from_millis(1500)).await;
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            assert!(!alive, "the process is gone");
            assert_eq!(read(&f, "signal.txt").trim() == "term", leaves, "SIGTERM first; SIGKILL only for one that stays");
            assert_eq!(started.elapsed() >= Duration::from_millis(1400), !leaves, "the grace is waited only for one that stays");
            // And through `run`: the error is `Timeout`.
            let err = run(&l, Duration::from_millis(600)).await.unwrap_err();
            assert_eq!(err, CallError::Timeout);
            drop(home);
            assert!(!base.exists());
        }
    }

    // The failures a CLI can report, each with its cause (AC-24): spent quota, a login that did not hold, an error, an exit with
    // no result, an empty answer; and an estimate when the CLI reports no tokens (AC-5's `estimated`).
    #[tokio::test]
    async fn each_way_a_call_ends_has_its_cause() {
        let result = |text: &str, extra: &str| format!("{INIT}\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"{text}\"{extra}}}'");
        let cases = [
            (format!("{INIT}\necho '{{\"type\":\"rate_limit_event\",\"rate_limit_info\":{{\"status\":\"rejected\",\"rateLimitType\":\"five_hour\"}}}}'\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"limit reached\"}}'"), Err(CallError::Quota)),
            (result("Not logged in · Please run /login", ",\"is_error\":true"), Err(CallError::Launch)),
            (result("overloaded", ",\"is_error\":true"), Err(CallError::Provider)),
            (format!("{INIT}\nexit 3"), Err(CallError::Provider)),
            (result("   ", ""), Err(CallError::Empty)),
            (result("a longer answer", ""), Ok(("a longer answer".to_string(), true))),
        ];
        for (body, want) in cases {
            let f = fx();
            let prog = recording_cli(f.d.path(), &f.rec, &body);
            let home = CallHome::make(&f.roots, &[]).unwrap();
            let got = run(&launch(&prog, "sonnet", "a prompt of forty characters, more or less".into(), &home, token()), Duration::from_secs(20)).await;
            assert_eq!(got.as_ref().map(|a| (a.text.clone(), a.estimated)).map_err(|e| *e), want, "{body}");
            if let Ok(a) = got {
                assert_eq!(a.tokens, ((42 + 15) / 4) as u64, "an estimate from the prompt and the answer");
            }
        }
        // A launch that was not built for the bridge is never run by it.
        let f = fx();
        let home = CallHome::make(&f.roots, &[]).unwrap();
        let mut l = launch("claude", "sonnet", "q".into(), &home, None);
        l.tools = Tools::Standard;
        assert_eq!(run(&l, Duration::from_secs(1)).await.unwrap_err(), CallError::Launch);
        // A program that does not exist is a failed launch.
        let l = launch("/nonexistent/claude", "sonnet", "q".into(), &home, None);
        assert_eq!(run(&l, Duration::from_secs(1)).await.unwrap_err(), CallError::Launch);
    }

    // AC-17: a daemon started after a crash removes the directories the crashed one left.
    #[test]
    fn the_sweep_removes_what_a_crash_left() {
        let f = fx();
        for (i, root) in f.roots.iter().enumerate() {
            std::fs::create_dir_all(root.join(format!("left-{i}/home/plugins"))).unwrap();
            std::fs::write(root.join(format!("left-{i}/home/.claude.json")), "{}").unwrap();
        }
        assert_eq!(sweep(&f.roots), (2, 0));
        assert!(f.roots.iter().all(|r| r.read_dir().unwrap().next().is_none()));
        assert_eq!(sweep(&[f.d.path().join("missing")]), (0, 0));
        let named = roots(Path::new("/a/home"));
        assert!(named[0].ends_with("run/bridge") && named[1] != roots(Path::new("/another/home"))[1], "two installs never share a temporary root");
    }

    // AC-17: a daemon started after a crash ends the process groups the crashed one left, and only those: a recorded process id
    // whose start time is not the recorded one (the id was given to another process since) is never signalled.
    #[tokio::test]
    async fn the_sweep_ends_a_recorded_process_group_and_never_a_reused_process_id() {
        let alive = |pid: i32| unsafe { libc::kill(pid, 0) } == 0;
        let f = fx();
        let prog = recording_cli(f.d.path(), &f.rec, &format!("{INIT}\nwhile :; do sleep 1; done"));
        // The crashed daemon's call: its CLI still runs, its directory and its note are still there.
        let home = CallHome::make(&f.roots, &[]).unwrap();
        let mut left = Session::start(&launch(&prog, "sonnet", "q".into(), &home, token())).await.unwrap();
        drop(left.next(Duration::from_secs(5)).await);
        let pid = left.pgid();
        home.record_group(pid);
        assert!(proc_start(pid).is_some() && alive(pid));
        // Another process, whose id a stale note names with a start time that is not its own.
        let other_home = CallHome::make(&f.roots, &[]).unwrap();
        let mut other = Session::start(&launch(&prog, "sonnet", "q".into(), &other_home, token())).await.unwrap();
        drop(other.next(Duration::from_secs(5)).await);
        let other_pid = other.pgid();
        std::fs::write(other_home.base.join(GROUP_FILE), format!("{other_pid} {}", proc_start(other_pid).unwrap() + 1)).unwrap();
        let (base, other_base) = (home.base.clone(), other_home.base.clone());
        // The crash: nothing is dropped, so nothing is cleaned up by the daemon that died.
        std::mem::forget(home);
        std::mem::forget(other_home);

        assert_eq!(sweep(&f.roots), (2, 1), "both directories go; one group is ended");
        assert!(!base.exists() && !other_base.exists());
        // The recorded group is gone (its leader is reaped by the session that still holds it here).
        let gone = tokio::time::timeout(Duration::from_secs(5), async { while let Some(ev) = left.next(Duration::from_secs(5)).await { if matches!(ev, Event::Exit { .. }) { break; } } }).await;
        assert!(gone.is_ok(), "the recorded CLI ended");
        assert!(alive(other_pid), "a process the note did not truly name is left alone");
        other.kill().await;
        assert_eq!(proc_start(i32::MAX - 7), None);
    }

    // The call's own note: `run_in` records the group in the call's home, and what the call showed is kept when it fails.
    #[tokio::test]
    async fn a_call_notes_its_group_and_keeps_what_it_showed() {
        let f = fx();
        let home = CallHome::make(&f.roots, &[]).unwrap();
        let rate = r#"echo '{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1795000000000}}'"#;
        let result = r#"echo '{"type":"result","subtype":"success","is_error":true,"result":"limit","usage":{"input_tokens":7,"output_tokens":1}}'"#;
        // The note is written just after the CLI starts, so the stand-in waits a moment before it reads it.
        let body = format!("sleep 1\ncat '{}/../group' > '{}/group.txt' 2>/dev/null\n{INIT}\n{rate}\n{result}", home.home.display(), f.rec.display());
        let prog = recording_cli(f.d.path(), &f.rec, &body);
        let mut seen = Seen::default();
        let err = run_in(&launch(&prog, "sonnet", "q".into(), &home, token()), Some(&home), Duration::from_secs(20), &mut seen).await.unwrap_err();
        assert_eq!(err, CallError::Quota);
        assert_eq!(seen, Seen { served: Some("claude-served-1".into()), tokens: 8, resets_at: Some(1_795_000_000) });
        let note = read(&f, "group.txt");
        let (pid, started) = note.trim().split_once(' ').expect("the note");
        assert!(pid.parse::<i32>().is_ok_and(|p| p > 1) && started.parse::<u64>().is_ok(), "{note}");
    }
}
