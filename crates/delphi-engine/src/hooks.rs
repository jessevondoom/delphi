//! Operator hooks (`specs/018`): commands the operator wants run around Delphi's events and approvals.
//!
//! Two kinds, and neither can loosen anything:
//!
//! * **Observe hooks** run on events (a notification when a task ends, a line in a log). Their output is ignored and
//!   they run out of process (`delphi hooks`), so a slow or broken one cannot slow the daemon.
//! * **Deny hooks** run in the daemon before an approval card is raised, and can only *refuse* the action: exit 0 means
//!   "no objection", and anything else (a non-zero exit, a timeout, a command that will not start) means no. A hook
//!   that fails to run therefore blocks the action instead of letting it through. They never approve anything.
//!
//! Both live in the ratified `[hooks]` block (`lock::ACCESS_KEYS`): they name commands that run on the operator's
//! machine, so an edit to the file changes nothing until the operator ratifies it. A command is an argv array, never
//! a shell string, and runs with a cleared environment.

use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_SECS: u64 = 10;
const MAX_TIMEOUT_SECS: u64 = 120;
/// How much of a hook's output is kept (the first line is the reason a deny hook gives).
const KEEP_OUTPUT: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct ObserveHook {
    pub name: String,
    /// Event kinds: exact (`task.completed`), a prefix (`approval.*`), or `*`.
    pub events: Vec<String>,
    pub command: Vec<String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DenyHook {
    pub name: String,
    /// Tool names (`write_file`, `merge`, …) or `*`.
    pub tools: Vec<String>,
    pub command: Vec<String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hooks {
    pub observe: Vec<ObserveHook>,
    pub deny: Vec<DenyHook>,
}

fn strings(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

/// Read `[hooks]`. A malformed entry is left out and reported; a malformed *deny* hook is reported loudly because
/// leaving it out makes Delphi less strict than the operator meant.
pub fn parse(root: &toml::Value) -> (Hooks, Vec<String>) {
    let mut hooks = Hooks::default();
    let mut problems = Vec::new();
    let Some(h) = root.get("hooks") else { return (hooks, problems) };
    for (kind, list) in [("observe", h.get("observe")), ("deny", h.get("deny"))] {
        for (i, e) in list.and_then(|l| l.as_array()).into_iter().flatten().enumerate() {
            let label = format!("[[hooks.{}]] #{}", kind, i + 1);
            let name = e.get("name").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
            let command = strings(e.get("command"));
            let selector = strings(e.get(if kind == "observe" { "events" } else { "tools" }));
            let timeout = e.get("timeout_secs").and_then(|v| v.as_integer()).filter(|v| *v > 0).map(|v| (v as u64).min(MAX_TIMEOUT_SECS)).unwrap_or(DEFAULT_TIMEOUT_SECS);
            let (Some(name), false, false) = (name, command.is_empty(), selector.is_empty()) else {
                problems.push(format!("{} is ignored: it needs a `name` (letters, digits, - and _), a `command` (an array of strings) and {}", label, if kind == "observe" { "`events`" } else { "`tools`" }));
                continue;
            };
            if command[0].contains(char::is_whitespace) && command.len() == 1 {
                problems.push(format!("{} `{}` is ignored: `command` is an argv array, not a shell line (use [\"/bin/sh\", \"-c\", \"…\"] if you mean a shell)", label, name));
                continue;
            }
            let timeout = Duration::from_secs(timeout);
            if kind == "observe" {
                hooks.observe.push(ObserveHook { name: name.to_string(), events: selector, command, timeout });
            } else {
                hooks.deny.push(DenyHook { name: name.to_string(), tools: selector, command, timeout });
            }
        }
    }
    (hooks, problems)
}

/// Does an event kind match any of the patterns? Exact, `prefix.*`, or `*`.
pub fn matches_event(patterns: &[String], kind: &str) -> bool {
    patterns.iter().any(|p| p == "*" || p == kind || p.strip_suffix(".*").is_some_and(|pre| kind.strip_prefix(pre).is_some_and(|rest| rest.starts_with('.'))))
}

pub fn matches_tool(tools: &[String], tool: &str) -> bool {
    tools.iter().any(|t| t == "*" || t == tool)
}

/// What running a hook came to.
#[derive(Debug, Clone, Default)]
pub struct Run {
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub spawn_error: Option<String>,
    /// The start of what the command printed (stdout only).
    pub stdout: String,
    pub millis: u64,
}

impl Run {
    pub fn ok(&self) -> bool {
        self.exit == Some(0) && !self.timed_out && self.spawn_error.is_none()
    }

    /// Why a deny hook said no, in words for the agent and the operator.
    pub fn reason(&self) -> String {
        if let Some(e) = &self.spawn_error {
            return format!("the hook could not start ({})", e);
        }
        if self.timed_out {
            return "the hook did not answer in time".into();
        }
        let first: String = self.stdout.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().filter(|c| !c.is_control()).take(200).collect();
        if first.is_empty() { format!("the hook exited with status {}", self.exit.map(|e| e.to_string()).unwrap_or_else(|| "?".into())) } else { first }
    }
}

/// Run one hook: the argv with a cleared environment, `stdin` on its standard input, killed with its whole process
/// group at the timeout. Blocking; callers put it on a blocking thread.
pub fn run(argv: &[String], stdin: &str, timeout: Duration, env: &[(&str, &str)], cwd: &Path) -> Run {
    let started = Instant::now();
    let mut out = Run::default();
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).env_clear().env("PATH", "/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin").envs(env.iter().copied()).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    // Its own process group, so a timeout takes the children down with it.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            out.spawn_error = Some(e.to_string());
            out.millis = started.elapsed().as_millis() as u64;
            return out;
        }
    };
    if let Some(mut si) = child.stdin.take() {
        let data = stdin.to_string();
        std::thread::spawn(move || {
            let _ = si.write_all(data.as_bytes());
        });
    }
    let reader = child.stdout.take().map(|mut so| {
        std::thread::spawn(move || {
            let mut keep = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = so.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if keep.len() < KEEP_OUTPUT {
                    keep.extend_from_slice(&buf[..n.min(KEEP_OUTPUT - keep.len())]);
                }
            }
            keep
        })
    });
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                out.exit = status.code();
                break;
            }
            Ok(None) if started.elapsed() >= timeout => {
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.kill();
                let _ = child.wait();
                out.timed_out = true;
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                out.spawn_error = Some(e.to_string());
                break;
            }
        }
    }
    if let Some(r) = reader {
        out.stdout = String::from_utf8_lossy(&r.join().unwrap_or_default()).to_string();
    }
    out.millis = started.elapsed().as_millis() as u64;
    out
}

/// The JSON a deny hook receives on standard input: what is being asked, as the daemon classified it.
pub fn deny_input(tool: &str, pattern: &str, summary: &str, tier: &str, role: &str, task_id: &str, payload: &Value) -> String {
    serde_json::json!({"tool": tool, "pattern": pattern, "summary": summary, "tier": tier, "role": role, "task_id": task_id, "payload": payload}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toml_of(s: &str) -> toml::Value {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn a_hooks_table_gives_observe_and_deny_hooks_with_clamped_timeouts() {
        let (h, problems) = parse(&toml_of(
            r#"
[[hooks.observe]]
name = "tell-me"
events = ["task.completed", "approval.*"]
command = ["/usr/local/bin/tell", "--loud"]
[[hooks.deny]]
name = "no-secrets"
tools = ["write_file"]
command = ["/bin/sh", "-c", "exit 1"]
timeout_secs = 9999
"#,
        ));
        assert!(problems.is_empty(), "{:?}", problems);
        assert_eq!((h.observe.len(), h.deny.len()), (1, 1));
        assert_eq!((h.observe[0].timeout, h.deny[0].timeout), (Duration::from_secs(DEFAULT_TIMEOUT_SECS), Duration::from_secs(MAX_TIMEOUT_SECS)));
        assert_eq!(h.observe[0].command, vec!["/usr/local/bin/tell", "--loud"]);
        assert_eq!(parse(&toml_of("[budget]\nx = 1\n")).0, Hooks::default());
    }

    #[test]
    fn a_malformed_entry_is_left_out_and_named_and_a_shell_line_is_not_an_argv() {
        let (h, problems) = parse(&toml_of(
            r#"
[[hooks.deny]]
name = "no command"
tools = ["merge"]
[[hooks.deny]]
name = "bad name!"
tools = ["merge"]
command = ["x"]
[[hooks.deny]]
name = "shell-line"
tools = ["merge"]
command = ["echo hi && rm -rf /"]
[[hooks.observe]]
name = "no-events"
command = ["x"]
"#,
        ));
        assert!(h.deny.is_empty() && h.observe.is_empty());
        assert_eq!(problems.len(), 4, "{:?}", problems);
        assert!(problems.iter().any(|p| p.contains("not a shell line")) && problems.iter().any(|p| p.contains("`events`")), "{:?}", problems);
    }

    #[test]
    fn events_match_exactly_by_prefix_or_all_and_tools_exactly_or_all() {
        let pats = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(matches_event(&pats(&["task.completed"]), "task.completed"));
        assert!(matches_event(&pats(&["approval.*"]), "approval.requested") && matches_event(&pats(&["*"]), "anything"));
        assert!(!matches_event(&pats(&["approval.*"]), "approvals.requested"), "a prefix ends at a dot");
        assert!(!matches_event(&pats(&["approval.*"]), "approval") && !matches_event(&pats(&["task.completed"]), "task.completed.x"));
        assert!(matches_tool(&pats(&["write_file", "merge"]), "merge") && matches_tool(&pats(&["*"]), "x") && !matches_tool(&pats(&["write_file"]), "write_files"));
    }

    fn sh(script: &str) -> Vec<String> {
        vec!["/bin/sh".into(), "-c".into(), script.into()]
    }

    #[test]
    fn a_command_gets_the_json_on_stdin_a_cleared_environment_and_its_first_line_is_the_reason() {
        let t = tempfile::tempdir().unwrap();
        let r = run(&sh("cat > in.json; env > env.txt; echo 'no thanks'; echo second; exit 3"), "{\"tool\":\"merge\"}", Duration::from_secs(10), &[("DELPHI_HOOK", "deny")], t.path());
        assert_eq!(r.exit, Some(3));
        assert!(!r.ok() && r.reason() == "no thanks", "{:?}", r);
        assert_eq!(std::fs::read_to_string(t.path().join("in.json")).unwrap(), "{\"tool\":\"merge\"}");
        let env = std::fs::read_to_string(t.path().join("env.txt")).unwrap();
        assert!(env.contains("DELPHI_HOOK=deny") && env.contains("PATH="), "{}", env);
        assert!(!env.contains("HOME=") && !env.contains("USER=") && !env.contains("SSH_AUTH_SOCK"), "nothing is inherited: {}", env);
    }

    #[test]
    fn exit_zero_is_no_objection_and_everything_else_is_no() {
        let t = tempfile::tempdir().unwrap();
        assert!(run(&sh("cat >/dev/null; exit 0"), "{}", Duration::from_secs(10), &[], t.path()).ok());
        let nonzero = run(&sh("exit 1"), "{}", Duration::from_secs(10), &[], t.path());
        assert!(!nonzero.ok() && nonzero.reason().contains("status 1"));
        let missing = run(&["/no/such/hook".into()], "{}", Duration::from_secs(10), &[], t.path());
        assert!(!missing.ok() && missing.reason().contains("could not start"), "a hook that will not start blocks, it does not wave things through");
        let slow = run(&sh("sleep 30"), "{}", Duration::from_millis(200), &[], t.path());
        assert!(slow.timed_out && !slow.ok() && slow.reason().contains("in time") && slow.millis < 5000, "{:?}", slow);
    }

    #[test]
    fn a_timeout_takes_the_whole_process_group_down_and_output_is_capped() {
        let t = tempfile::tempdir().unwrap();
        let marker = t.path().join("child-ran");
        let script = format!("(sleep 2; touch {}) & sleep 30", marker.display());
        let r = run(&sh(&script), "{}", Duration::from_millis(200), &[], t.path());
        assert!(r.timed_out);
        std::thread::sleep(Duration::from_millis(2500));
        assert!(!marker.exists(), "the background child was killed with the hook");
        let big = run(&sh("head -c 1000000 /dev/zero | tr '\\0' 'x'"), "{}", Duration::from_secs(10), &[], t.path());
        assert!(big.stdout.len() <= KEEP_OUTPUT, "{}", big.stdout.len());
    }
}
