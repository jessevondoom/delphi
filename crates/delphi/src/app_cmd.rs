//! `delphi app` (`specs/120` §2, §5): start the desktop app from a terminal, which is what vouches for it, and list,
//! revoke and grant its keys.
//!
//! * Every form is the operator's, at a terminal: refused inside an agent seat, and refused when the input is not a
//!   terminal (the test the console's attach uses, [`crate::chat::is_tty`]). The daemon holds the same line from its
//!   side: it gives a pairing ticket, revokes and grants only for a console client attached to a terminal.
//! * `delphi app` asks the daemon for a pairing ticket and starts the app's executable directly, with the ticket on the
//!   app's standard input, a pipe only the two of them hold. The ticket is never in an argument, an environment variable or
//!   a file. Nothing is asked of the operator.
//! * `grant` takes only a grant the code's table defines ([`delphi_core::app_keys::GRANTS`]).
//! * `delphi app approve` (`specs/120` §13.1): an app with no key shows a code in its window; the operator types that code
//!   here. Enter alone approves nothing, and nothing a process does for itself can type it. A key approved this way holds
//!   `read`; `--grant request` adds `request`.

use delphi_core::app_keys;
use delphi_core::client::{Client, ClientError};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Pair and start the app.
    Start,
    List,
    Log,
    Revoke(String),
    Grant { name: String, grant: String, remove: bool },
    /// Approve a pairing request by the code typed at a prompt (§13.1); `request` adds that grant to the new key.
    Approve { request: bool },
}

/// The refusal every form of the command shares, before a workspace is looked up or a daemon is spoken to.
pub(crate) fn gate(agent_seat: bool, input_is_terminal: bool) -> Result<(), &'static str> {
    if agent_seat {
        return Err("refused; the desktop app is paired, granted and revoked by the operator, and this process was started by an agent seat");
    }
    if !input_is_terminal {
        return Err("refused; run it in a terminal (its input is not one). Starting the app from a terminal is what vouches for it");
    }
    Ok(())
}

/// What was asked, from the arguments after `app`. `--workspace` and `--home` (with their values) are the caller's.
pub(crate) fn parse(args: &[String]) -> Result<Action, String> {
    let mut words: Vec<&str> = Vec::new();
    let mut remove = false;
    let mut grant_flag: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--workspace" | "--home" => {
                it.next();
            }
            "--remove" => remove = true,
            "--grant" => grant_flag = Some(it.next().ok_or("`--grant` takes a grant: request")?.clone()),
            other if other.starts_with("--") => return Err(format!("unknown option `{}`", other)),
            other => words.push(other),
        }
    }
    let key = |name: &str| -> Result<String, String> {
        if app_keys::is_name(name) {
            Ok(name.to_string())
        } else {
            Err("that is not a key's name (one looks like app-1a2b3c; `delphi app list` shows them)".to_string())
        }
    };
    let action = match words[..] {
        [] => Action::Start,
        ["list"] => Action::List,
        ["log"] => Action::Log,
        ["revoke", name] => Action::Revoke(key(name)?),
        ["approve"] => Action::Approve { request: false },
        ["grant", name, grant] => {
            // Only a grant the table defines (§5 item 1); the daemon holds the same line.
            if !app_keys::is_grant(grant) {
                return Err(format!("no such grant; the grants are: {}", app_keys::GRANTS.iter().map(|g| g.name).collect::<Vec<_>>().join(", ")));
            }
            Action::Grant { name: key(name)?, grant: grant.to_string(), remove }
        }
        _ => return Err("usage: delphi app | app list | app log | app approve [--grant request] | app revoke <name> | app grant <name> [--remove] <grant>".to_string()),
    };
    let action = match (action, grant_flag.as_deref()) {
        (Action::Approve { .. }, Some("request")) => Action::Approve { request: true },
        (Action::Approve { .. }, Some(_)) => return Err("a key approved by code may be given `request` here and nothing else: `--grant request`".to_string()),
        (_, Some(_)) => return Err("`--grant` goes with `approve`".to_string()),
        (a, None) => a,
    };
    if remove && !matches!(action, Action::Grant { .. }) {
        return Err("`--remove` goes with `grant`".to_string());
    }
    Ok(action)
}

/// Is this a file the operator (or root) alone can change, and can it be run? A ticket is about to be handed to it.
fn runnable(p: &Path) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let Ok(m) = std::fs::metadata(p) else { return false };
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    m.is_file() && m.permissions().mode() & 0o111 != 0 && m.permissions().mode() & 0o022 == 0 && (m.uid() == me || m.uid() == 0)
}

/// The executable inside a macOS `.app` folder (`Contents/MacOS/delphi-app`, as the app's build makes it), or the file itself.
fn executable_of(p: &Path) -> Option<PathBuf> {
    if p.is_dir() {
        let inner = p.join("Contents/MacOS/delphi-app");
        return runnable(&inner).then_some(inner);
    }
    runnable(p).then(|| p.to_path_buf())
}

/// Where the app's executable is. `DELPHI_APP` when it is set, and then nothing else is tried. Otherwise the build beside this
/// program (a source checkout: `app/dev/build.sh` leaves `target/app/release/bundle/macos/Delphi.app` and
/// `target/app/release/delphi-app`, and this program is `target/<profile>/delphi`), then `/Applications/Delphi.app`, then
/// `~/Applications/Delphi.app`. Never a path under the working directory: a folder the operator happens to stand in does not
/// get to name the program that is handed a ticket.
pub(crate) fn find_app(delphi_app: Option<&str>, this_exe: Option<&Path>, user_home: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = delphi_app.filter(|p| !p.is_empty()) {
        let p = Path::new(p);
        if !p.is_absolute() {
            return Err("DELPHI_APP must be an absolute path".to_string());
        }
        return executable_of(p).ok_or_else(|| format!("DELPHI_APP names {}, which is not an executable only you can change (or a Delphi.app holding one)", p.display()));
    }
    let mut places: Vec<PathBuf> = Vec::new();
    // Only a program that is itself a build in a `target/` folder has a build beside it: an installed `delphi`
    // (`~/.local/bin/delphi`) looks nowhere near itself.
    let profile = this_exe.and_then(|e| e.parent()).filter(|d| d.file_name().is_some_and(|n| n == "debug" || n == "release"));
    if let Some(target) = profile.and_then(|d| d.parent()).filter(|t| t.file_name().is_some_and(|n| n == "target")) {
        places.push(target.join("app/release/bundle/macos/Delphi.app"));
        places.push(target.join("app/release/delphi-app"));
    }
    places.push(PathBuf::from("/Applications/Delphi.app"));
    if let Some(h) = user_home {
        places.push(h.join("Applications/Delphi.app"));
    }
    places.iter().find_map(|p| executable_of(p)).ok_or_else(|| "the desktop app was not found: build it with `app/dev/build.sh` in a source checkout, put Delphi.app in /Applications, or set DELPHI_APP to its executable".to_string())
}

/// Is starting the app switched on in this build? See [`start_with_ticket`].
pub(crate) const START_ENABLED: bool = true;

/// Start the app's executable directly and hand it the ticket on its standard input (§2 item 3). No argument, no environment
/// variable and no file carries the ticket; the pipe is written once and closed. The app runs in a session of its own, so
/// closing the terminal does not close the window, and is told which daemon to pair with by the same three variables the
/// bridge already reads (`DELPHI_EXE`, `DELPHI_HOME`, `DELPHI_WORKSPACE`): paths and a name, nothing secret.
///
/// This is a place where the program starts a process: it is listed, with its class, in
/// `crates/delphi/tests/spawn_sites.toml` (spec 093 K1).
fn start_with_ticket(exe: &Path, ticket: &str, daemon: &[(&str, String)]) -> std::io::Result<u32> {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    for (k, v) in daemon {
        cmd.env(k, v);
    }
    // SAFETY: setsid is async-signal-safe and touches nothing of the parent's.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    let mut pipe = child.stdin.take().ok_or_else(|| std::io::Error::other("no pipe to the app"))?;
    pipe.write_all(ticket.as_bytes())?;
    pipe.write_all(b"\n")?;
    drop(pipe);
    let pid = child.id();
    // The app is its own program from here; do not leave a zombie behind while this one waits for the pairing.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

fn day(ts: &str) -> &str {
    ts.get(..10).unwrap_or(ts)
}

/// `delphi app list`, from the daemon's `app.keys`.
pub(crate) fn render_list(listed: &Value) -> String {
    let keys = listed["keys"].as_array().cloned().unwrap_or_default();
    if keys.is_empty() {
        return "no app is paired. `delphi app`, in a terminal, starts the desktop app and pairs it.\n".to_string();
    }
    let mut out = format!("{:<12}{:<18}{:<12}{:<12}{:<10}{}\n", "NAME", "GRANTS", "PAIRED", "LAST USED", "APP", "OPEN");
    for k in &keys {
        let grants = k["grants"].as_array().map(|g| g.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let grants = if grants.is_empty() { "(none)".to_string() } else { grants };
        out.push_str(&format!("{:<12}{:<18}{:<12}{:<12}{:<10}{}\n", k["name"].as_str().unwrap_or("?"), grants, day(k["paired_at"].as_str().unwrap_or("")), day(k["last_used"].as_str().unwrap_or("")), k["app_version"].as_str().filter(|v| !v.is_empty()).unwrap_or("-"), k["sessions"].as_u64().unwrap_or(0)));
    }
    out.push_str("a key unused for 180 days is removed. `delphi app revoke <name>` removes one now and closes its sessions.\n");
    out
}

/// One line of `delphi app log` for an event of the app socket: when, what, which key, and the fixed words that go with it.
pub(crate) fn render_event(timestamp: &str, kind: &str, p: &Value) -> String {
    let name = p["name"].as_str().unwrap_or("-");
    let list = |v: &Value| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let what = match kind {
        "app.paired" => format!("paired with {}", list(&p["grants"])),
        "app.pair_requested" => format!("asked to pair (key {}, app {})", p["fingerprint"].as_str().unwrap_or("?"), p["app_version"].as_str().filter(|v| !v.is_empty()).unwrap_or("-")),
        "app.pair_approved" => format!("approved by code with {}", list(&p["grants"])),
        "app.refused" => format!("refused: {}{}", p["reason"].as_str().unwrap_or("?"), if p["locked_out"] == true { " (the socket is closed to new connections for a minute)" } else { "" }),
        "app.revoked" => format!("revoked; {} session(s) closed", p["sessions_closed"].as_u64().unwrap_or(0)),
        "app.granted" => format!("{} {}", if p["removed"] == true { "lost" } else { "given" }, p["grant"].as_str().unwrap_or("?")),
        "app.expired" => "removed: unused for 180 days".to_string(),
        // `app.request` (§4.2 item 4): the method, the ids it named and where the request stands. Never what was typed: the
        // daemon does not record it. The reason is the daemon's own sentence, cut to one short line when it was written.
        "app.request" => {
            let state = match (p["state"].as_str(), p["why"].as_str()) {
                (Some(s), Some(why)) => format!(": {} ({})", s, why.chars().filter(|c| !c.is_control()).take(200).collect::<String>()),
                (Some(s), None) => format!(": {}", s),
                _ => String::new(),
            };
            format!("asked {} {}{}", p["method"].as_str().unwrap_or("?"), p["ids"], state)
        }
        other => other.to_string(),
    };
    format!("{}  {:<12}{}", timestamp, name, what)
}

/// `delphi app log`: the app socket's events, oldest first, read from the logs of the daemon's own day sessions (where it
/// writes them). At most the newest `max` lines.
pub(crate) fn render_log(ws_dir: &Path, max: usize) -> String {
    let mut dirs: Vec<(String, PathBuf)> = delphi_core::sessions::dirs(ws_dir).into_iter().filter(|(id, _)| id.ends_with("-session")).collect();
    dirs.sort();
    let mut lines: Vec<String> = Vec::new();
    for (_, dir) in dirs {
        for e in delphi_core::log::replay(&dir).unwrap_or_default() {
            if e.kind.starts_with("app.") {
                lines.push(render_event(&e.timestamp, &e.kind, &e.payload));
            }
        }
    }
    if lines.is_empty() {
        return "nothing yet: no app has paired, asked or been refused in this workspace.\n".to_string();
    }
    let from = lines.len().saturating_sub(max);
    let mut out = lines[from..].join("\n");
    out.push('\n');
    out
}

fn fail(e: &ClientError) -> i32 {
    eprintln!("delphi app: {}", crate::chat::rpc_error(e));
    1
}

/// Ask for a ticket, start the app with it, and say what became of it. `started` starts the app (a test gives its own).
pub(crate) fn pair(client: &mut Client, exe: &Path, start: &dyn Fn(&Path, &str) -> std::io::Result<u32>, out: &mut dyn std::io::Write) -> i32 {
    let t = match client.call("auth.pair_ticket", json!({})) {
        Ok(t) => t,
        Err(e) => return fail(&e),
    };
    let (Some(ticket), Some(name)) = (t["ticket"].as_str(), t["name"].as_str()) else {
        eprintln!("delphi app: the daemon gave no ticket");
        return 1;
    };
    let secs = t["expires_in"].as_u64().unwrap_or(60);
    if let Err(e) = start(exe, ticket) {
        eprintln!("delphi app: {} could not be started: {}", exe.display(), e);
        return 1;
    }
    let _ = writeln!(out, "started {} with a pairing ticket (good once, for {} seconds)", exe.display(), secs);
    // The app pairs on its own; watch for what became of the ticket. Nothing is asked of the operator (§2 item 6).
    let deadline = Instant::now() + Duration::from_secs(secs + 5);
    loop {
        let listed = match client.call("app.keys", json!({})) {
            Ok(v) => v,
            Err(e) => return fail(&e),
        };
        let spent = listed["spent"].as_array().and_then(|s| s.iter().find(|s| s["ticket"] == name).cloned());
        if let Some(s) = spent {
            let key = s["key"].as_str().unwrap_or("");
            let grants = listed["keys"].as_array().and_then(|k| k.iter().find(|k| k["name"] == key)).map(|k| k["grants"].as_array().map(|g| g.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()).unwrap_or_default();
            return match s["outcome"].as_str() {
                Some("paired") => {
                    let _ = writeln!(out, "paired: {} may {}. `delphi app list` shows it; `delphi app revoke {}` removes it.", key, if grants.is_empty() { "do nothing".to_string() } else { format!("use the grants {}", grants) }, key);
                    0
                }
                Some("already_paired") => {
                    let _ = writeln!(out, "the app already holds a paired key, {} ({}); nothing was changed.", key, if grants.is_empty() { "no grants" } else { &grants });
                    0
                }
                other => {
                    eprintln!("delphi app: the pairing was refused ({}); `delphi app log` shows it", other.unwrap_or("?"));
                    1
                }
            };
        }
        let pending = listed["pending"].as_array().is_some_and(|p| p.iter().any(|p| p == name));
        if !pending || Instant::now() >= deadline {
            eprintln!("delphi app: the app did not pair within {} seconds, and the ticket is no longer good. Run `delphi app` again.", secs);
            return 1;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// `delphi app approve` (§13.1 item 3): list the requests that wait, ask for the code shown in the app's window, and send it with
/// the request's id. A line with nothing in it approves nothing and sends nothing; the daemon holds the code, so a wrong one is
/// refused there and three cancel the request.
pub(crate) fn approve(client: &mut Client, request: bool, input: &mut dyn std::io::BufRead, out: &mut dyn std::io::Write) -> i32 {
    let listed = match client.call("app.pair_pending", json!({})) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let pending = listed["pending"].as_array().cloned().unwrap_or_default();
    if pending.is_empty() {
        let _ = writeln!(out, "no app is waiting to be approved. Start the desktop app; with no key it shows a code and waits.");
        return 1;
    }
    let _ = writeln!(out, "{:<4}{:<16}{:<12}ASKED", "NO.", "KEY", "APP");
    for (i, p) in pending.iter().enumerate() {
        let _ = writeln!(out, "{:<4}{:<16}{:<12}{}s ago (good for {}s more)", i + 1, p["fingerprint"].as_str().unwrap_or("?"), p["app_version"].as_str().filter(|v| !v.is_empty()).unwrap_or("-"), p["age_secs"].as_u64().unwrap_or(0), p["expires_in"].as_u64().unwrap_or(0));
    }
    let mut ask = |question: &str, out: &mut dyn std::io::Write| -> Option<String> {
        let _ = write!(out, "{}", question);
        let _ = out.flush();
        let mut line = String::new();
        (input.read_line(&mut line).ok()? > 0).then(|| line.trim().to_string())
    };
    let chosen = if pending.len() == 1 {
        0
    } else {
        match ask("which one? (the number): ", out).and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=pending.len()).contains(n)) {
            Some(n) => n - 1,
            None => {
                let _ = writeln!(out, "nothing was approved.");
                return 1;
            }
        }
    };
    let typed = ask("type the code shown in the app window: ", out).unwrap_or_default();
    if typed.is_empty() {
        let _ = writeln!(out, "nothing was approved: approval is the code typed, not Enter.");
        return 1;
    }
    let grants: Vec<&str> = if request { vec!["request"] } else { vec![] };
    match client.call("app.pair_approve", json!({"request_id": pending[chosen]["request_id"], "code": typed, "grants": grants})) {
        Ok(v) => {
            let held = v["grants"].as_array().map(|g| g.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            let name = v["name"].as_str().unwrap_or("?");
            let _ = writeln!(out, "approved: {} may use the grants {}. `delphi app list` shows it; `delphi app grant {} request` gives `request`; `delphi app revoke {}` removes it.", name, held, name, name);
            0
        }
        Err(e) => fail(&e),
    }
}

/// The forms that speak to the daemon over an attached console connection.
pub(crate) fn act(client: &mut Client, action: &Action, out: &mut dyn std::io::Write) -> i32 {
    match action {
        Action::List => match client.call("app.keys", json!({})) {
            Ok(v) => {
                let _ = write!(out, "{}", render_list(&v));
                0
            }
            Err(e) => fail(&e),
        },
        Action::Revoke(name) => match client.call("app.revoke", json!({"name": name})) {
            Ok(v) => {
                let _ = writeln!(out, "revoked {}; {} open session(s) closed", name, v["sessions_closed"].as_u64().unwrap_or(0));
                0
            }
            Err(e) => fail(&e),
        },
        Action::Grant { name, grant, remove } => match client.call("app.grant", json!({"name": name, "grant": grant, "remove": remove})) {
            Ok(v) => {
                let held = v["grants"].as_array().map(|g| g.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                let change = match (v["changed"] == true, *remove) {
                    (true, false) => format!("{} now holds {}", name, grant),
                    (true, true) => format!("{} no longer holds {}; {} open session(s) closed", name, grant, v["sessions_closed"].as_u64().unwrap_or(0)),
                    (false, false) => format!("{} already holds {}", name, grant),
                    (false, true) => format!("{} did not hold {}", name, grant),
                };
                let _ = writeln!(out, "{} (grants: {})", change, if held.is_empty() { "none" } else { &held });
                0
            }
            Err(e) => fail(&e),
        },
        Action::Start | Action::Log | Action::Approve { .. } => 2,
    }
}

/// `delphi app …`, past the gate, for the workspace `ws_dir` (named `ws_name`) under `home`.
pub(crate) fn run(action: Action, home: &Path, ws_dir: &Path, ws_name: &str) -> i32 {
    let mut out = std::io::stdout();
    if action == Action::Log {
        print!("{}", render_log(ws_dir, 200));
        return 0;
    }
    // Find the app before a ticket is asked for: a ticket nobody can use is not issued.
    let exe = if action == Action::Start {
        if !START_ENABLED {
            eprintln!("delphi app: starting the app is not switched on in this build. It adds a place where this program starts a process, which must first be listed in crates/delphi/tests/spawn_sites.toml (specs/120, slice K2). `delphi app list`, `log`, `revoke` and `grant` work.");
            return 1;
        }
        let this = std::env::current_exe().ok();
        match find_app(std::env::var("DELPHI_APP").ok().as_deref(), this.as_deref(), std::env::var_os("HOME").map(PathBuf::from).as_deref()) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("delphi app: {}", e);
                return 1;
            }
        }
    } else {
        None
    };
    let Some(daemon_exe) = crate::chat::daemon_exe() else {
        eprintln!("delphi app: cannot locate this program to start the daemon");
        return 1;
    };
    // Attach as a terminal client on the console socket (§2 item 2), starting the daemon if none is running.
    let mut attached = match delphi_core::spawn::connect_or_spawn(&delphi_core::spawn::SpawnOptions { home, workspace: ws_dir, daemon_exe: &daemon_exe, extra_args: vec![], prefix_args: vec!["__core".into()], own_version: Some(env!("CARGO_PKG_VERSION")) }) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    match exe {
        Some(exe) => {
            let daemon = [("DELPHI_EXE", daemon_exe.display().to_string()), ("DELPHI_HOME", home.display().to_string()), ("DELPHI_WORKSPACE", ws_name.to_string())];
            pair(&mut attached.client, &exe, &|exe, ticket| start_with_ticket(exe, ticket, &daemon), &mut out)
        }
        None => match action {
            Action::Approve { request } => approve(&mut attached.client, request, &mut std::io::stdin().lock(), &mut out),
            _ => act(&mut attached.client, &action, &mut out),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::server::{Daemon, DaemonConfig, TtyPolicy};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    /// AC-2: an agent seat is refused, and so is input that is not a terminal, before anything else happens.
    #[test]
    fn every_form_is_refused_inside_an_agent_seat_or_without_a_terminal() {
        assert!(gate(true, true).unwrap_err().contains("agent seat"));
        assert!(gate(true, false).unwrap_err().contains("agent seat"));
        assert!(gate(false, false).unwrap_err().contains("terminal"));
        assert_eq!(gate(false, true), Ok(()));
    }

    #[test]
    fn the_forms_parse_and_grant_takes_only_a_grant_the_table_defines() {
        assert_eq!(parse(&args("")), Ok(Action::Start));
        assert_eq!(parse(&args("--workspace w --home /h")), Ok(Action::Start));
        assert_eq!(parse(&args("list --home /h")), Ok(Action::List));
        assert_eq!(parse(&args("log")), Ok(Action::Log));
        assert_eq!(parse(&args("approve")), Ok(Action::Approve { request: false }));
        assert_eq!(parse(&args("approve --grant request --home /h")), Ok(Action::Approve { request: true }));
        for bad in ["approve --grant update", "approve --grant", "approve extra", "list --grant request", "approve --remove"] {
            assert!(parse(&args(bad)).is_err(), "{bad:?} must not parse");
        }
        assert_eq!(parse(&args("revoke app-0a1b2c")), Ok(Action::Revoke("app-0a1b2c".into())));
        assert_eq!(parse(&args("grant app-0a1b2c request")), Ok(Action::Grant { name: "app-0a1b2c".into(), grant: "request".into(), remove: false }));
        assert_eq!(parse(&args("grant app-0a1b2c --remove request")), Ok(Action::Grant { name: "app-0a1b2c".into(), grant: "request".into(), remove: true }));
        for g in ["answer", "settings", "review", "curate", "console", "*", "Read"] {
            assert!(parse(&args(&format!("grant app-0a1b2c {g}"))).unwrap_err().contains("the grants are: read, request"), "{g}");
        }
        for bad in ["revoke", "revoke ../x", "revoke app-XYZ", "grant app-0a1b2c", "grant request", "pair", "list extra", "list --remove", "--remove", "revoke app-0a1b2c --yes", "start"] {
            assert!(parse(&args(bad)).is_err(), "{bad:?} must not parse");
        }
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{}\n", body)).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn the_app_is_the_one_named_else_the_build_beside_this_program_and_never_a_file_others_can_change() {
        let t = tempfile::tempdir().unwrap();
        let d = std::fs::canonicalize(t.path()).unwrap();
        // DELPHI_APP: an executable, or a Delphi.app holding one; when set, nothing else is tried.
        let exe = script(&d, "my-app", "exit 0");
        assert_eq!(find_app(Some(exe.to_str().unwrap()), None, None), Ok(exe.clone()));
        std::fs::create_dir_all(d.join("Delphi.app/Contents/MacOS")).unwrap();
        let inner = script(&d.join("Delphi.app/Contents/MacOS"), "delphi-app", "exit 0");
        assert_eq!(find_app(Some(d.join("Delphi.app").to_str().unwrap()), None, None), Ok(inner));
        assert!(find_app(Some("relative/app"), None, None).unwrap_err().contains("absolute"));
        assert!(find_app(Some(d.join("missing").to_str().unwrap()), Some(&d.join("target/debug/delphi")), Some(&d)).is_err(), "a DELPHI_APP that is wrong is an error, not a reason to look elsewhere");
        // A file that is not executable, or that group or others may write, is not started.
        let plain = d.join("plain");
        std::fs::write(&plain, "x").unwrap();
        assert!(find_app(Some(plain.to_str().unwrap()), None, None).is_err());
        let loose = script(&d, "loose", "exit 0");
        std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(find_app(Some(loose.to_str().unwrap()), None, None).is_err());
        // With no DELPHI_APP: the build beside this program (target/<profile>/delphi -> target/app/release/...).
        let this = d.join("target/debug/delphi");
        assert!(find_app(None, Some(&this), Some(&d.join("nobody"))).is_err() || Path::new("/Applications/Delphi.app").exists());
        std::fs::create_dir_all(d.join("target/app/release")).unwrap();
        let built = script(&d.join("target/app/release"), "delphi-app", "exit 0");
        assert_eq!(find_app(None, Some(&this), Some(&d.join("nobody"))), Ok(built));
        std::fs::create_dir_all(d.join("target/app/release/bundle/macos/Delphi.app/Contents/MacOS")).unwrap();
        let bundled = script(&d.join("target/app/release/bundle/macos/Delphi.app/Contents/MacOS"), "delphi-app", "exit 0");
        assert_eq!(find_app(None, Some(&this), None), Ok(bundled), "the app bundle before the bare executable");
        // An installed program is not a build in a `target/` folder, so nothing beside it is looked at.
        std::fs::create_dir_all(d.join("local/app/release")).unwrap();
        script(&d.join("local/app/release"), "delphi-app", "exit 0");
        let found = find_app(None, Some(&d.join("local/bin/delphi")), Some(&d.join("nobody")));
        assert!(found.as_ref().is_ok_and(|p| p.starts_with("/Applications")) || found.is_err(), "{found:?}");
    }

    fn wait_for(p: &Path) -> String {
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            if let Ok(s) = std::fs::read_to_string(p) {
                if s.ends_with("END\n") {
                    return s;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{} was not written", p.display());
    }

    /// §2 item 3: the ticket reaches the app on an inherited pipe and nowhere else: not in its arguments, not in its
    /// environment, and in no file this program wrote.
    #[test]
    fn the_ticket_is_handed_over_on_a_pipe_and_is_in_no_argument_or_variable() {
        let t = tempfile::tempdir().unwrap();
        let d = std::fs::canonicalize(t.path()).unwrap();
        let seen = d.join("seen");
        // A stand-in app: writes down its arguments, its environment and what its standard input held.
        let exe = script(&d, "app", &format!("{{ echo \"ARGS:$#:$*\"; env; echo STDIN:; cat; echo END; }} > '{}'", seen.display()));
        let ticket = "7a".repeat(32);
        let pid = start_with_ticket(&exe, &ticket, &[("DELPHI_HOME", "/h".to_string()), ("DELPHI_WORKSPACE", "w".to_string())]).unwrap();
        assert!(pid > 0);
        let got = wait_for(&seen);
        let (before, stdin) = got.split_once("STDIN:\n").unwrap();
        assert_eq!(stdin, format!("{}\nEND\n", ticket), "the pipe holds the ticket, one line, and is then closed");
        assert!(before.starts_with("ARGS:0:\n"), "no arguments: {before}");
        assert!(!before.contains(&ticket), "the ticket is in no environment variable");
        assert!(before.contains("DELPHI_HOME=/h\n") && before.contains("DELPHI_WORKSPACE=w\n"), "the app is told which daemon to pair with");
        // Nothing else in the folder holds it (no file was written with it).
        for e in std::fs::read_dir(&d).unwrap().flatten().filter(|e| e.path() != seen) {
            assert!(!std::fs::read_to_string(e.path()).unwrap_or_default().contains(&ticket), "{}", e.path().display());
        }
    }

    /// An in-process daemon with an app socket, serving on a thread of its own; the test is a blocking console client.
    struct Served {
        _dir: tempfile::TempDir,
        home: PathBuf,
        app_sock: PathBuf,
        daemon: Arc<Daemon>,
        console: Client,
    }

    fn serve() -> Served {
        let dir = tempfile::Builder::new().prefix("dc").tempdir_in("/tmp").unwrap();
        let home = dir.path().to_path_buf();
        let (sock, app_sock) = (home.join("d.sock"), home.join("a.sock"));
        let mut cfg = DaemonConfig::new(home.clone(), home.clone());
        cfg.tty_policy = TtyPolicy::Disabled;
        let daemon = Daemon::new(cfg).unwrap();
        let (d, s, a) = (daemon.clone(), sock.clone(), app_sock.clone());
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let console = tokio::net::UnixListener::bind(&s).unwrap();
                let app = tokio::net::UnixListener::bind(&a).unwrap();
                ready_tx.send(()).unwrap();
                d.serve_with_app(console, None, None, None, Some(app)).await;
            });
        });
        ready_rx.recv().unwrap();
        let mut console = Client::connect(&sock).unwrap();
        console.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        console.hello(daemon.console_token()).unwrap();
        Served { _dir: dir, home, app_sock, daemon, console }
    }

    /// What the app does with a ticket, as slice K3 builds it: answer the challenge's connection with `app.pair`.
    fn app_pairs(app_sock: &Path, ticket: &str, seed: [u8; 32]) -> Result<Value, ClientError> {
        let mut c = Client::connect(app_sock)?;
        c.set_read_timeout(Some(Duration::from_secs(5)))?;
        let ch = c.next_notification()?;
        let (public_key, _) = app_keys::test_sign(&seed, ch["params"]["nonce"].as_str().unwrap(), ch["params"]["instance"].as_str().unwrap(), 1, &app_sock.to_string_lossy());
        c.call("app.pair", json!({"ticket": ticket, "public_key": public_key, "app_version": "0.0.0"}))
    }

    /// AC-1, the terminal's half: `delphi app` gets a ticket, starts the app with it, and when the app has paired prints
    /// the key's name and grants. The record then holds one key with `read` and `request`. Then list, grant, revoke.
    #[test]
    fn pairing_prints_the_key_and_its_grants_and_the_other_forms_drive_the_daemon() {
        let mut s = serve();
        let sock = s.app_sock.clone();
        let exe = Path::new("/the/app");
        let mut out: Vec<u8> = Vec::new();
        let handed: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
        let start = |_: &Path, ticket: &str| {
            *handed.lock().unwrap() = ticket.to_string();
            app_pairs(&sock, ticket, [5u8; 32]).map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok(1)
        };
        assert_eq!(pair(&mut s.console, exe, &start, &mut out), 0);
        let ticket = handed.lock().unwrap().clone();
        assert_eq!(ticket.len(), 64);
        let keys = app_keys::KeyStore::at(&s.home).load().unwrap();
        assert_eq!((keys.len(), keys[0].grants.clone()), (1, vec!["read".to_string(), "request".to_string()]));
        let name = keys[0].name.clone();
        let said = String::from_utf8(out).unwrap();
        assert!(said.contains("started /the/app with a pairing ticket") && said.contains(&format!("paired: {} may use the grants read, request", name)), "{said}");
        assert!(!said.contains(&ticket), "the ticket is not printed: {said}");

        // The same app started again from a terminal: its key is kept as it is.
        let sock = s.app_sock.clone();
        let mut out: Vec<u8> = Vec::new();
        let again = move |_: &Path, ticket: &str| app_pairs(&sock, ticket, [5u8; 32]).map(|_| 1).map_err(|e| std::io::Error::other(e.to_string()));
        assert_eq!(pair(&mut s.console, exe, &again, &mut out), 0);
        assert!(String::from_utf8(out).unwrap().contains(&format!("already holds a paired key, {} (read, request); nothing was changed", name)));
        // An app that could not be started: no key, and the command fails.
        let mut out: Vec<u8> = Vec::new();
        assert_eq!(pair(&mut s.console, exe, &|_, _| Err(std::io::Error::other("no such file")), &mut out), 1);
        assert_eq!(app_keys::KeyStore::at(&s.home).load().unwrap().len(), 1);

        let mut out: Vec<u8> = Vec::new();
        assert_eq!(act(&mut s.console, &Action::List, &mut out), 0);
        let listed = String::from_utf8(out).unwrap();
        assert!(listed.lines().nth(1).unwrap().starts_with(&format!("{}  read, request", name)) && listed.contains("0.0.0"), "{listed}");

        let mut out: Vec<u8> = Vec::new();
        assert_eq!(act(&mut s.console, &Action::Grant { name: name.clone(), grant: "request".into(), remove: true }, &mut out), 0);
        assert_eq!(act(&mut s.console, &Action::Grant { name: name.clone(), grant: "request".into(), remove: true }, &mut out), 0);
        assert_eq!(act(&mut s.console, &Action::Grant { name: name.clone(), grant: "request".into(), remove: false }, &mut out), 0);
        let said = String::from_utf8(out).unwrap();
        assert_eq!(said.lines().collect::<Vec<_>>(), [format!("{name} no longer holds request; 0 open session(s) closed (grants: read)"), format!("{name} did not hold request (grants: read)"), format!("{name} now holds request (grants: read, request)")]);
        // A grant the table does not define never reaches the record, even past `parse`.
        assert_eq!(act(&mut s.console, &Action::Grant { name: name.clone(), grant: "answer".into(), remove: false }, &mut Vec::new()), 1);
        assert_eq!(app_keys::KeyStore::at(&s.home).load().unwrap()[0].grants, ["read", "request"]);

        let mut out: Vec<u8> = Vec::new();
        assert_eq!(act(&mut s.console, &Action::Revoke(name.clone()), &mut out), 0);
        assert_eq!(String::from_utf8(out).unwrap(), format!("revoked {name}; 0 open session(s) closed\n"));
        assert_eq!(act(&mut s.console, &Action::Revoke(name), &mut Vec::new()), 1, "it is gone");
        let mut out: Vec<u8> = Vec::new();
        act(&mut s.console, &Action::List, &mut out);
        assert!(String::from_utf8(out).unwrap().starts_with("no app is paired"));
        s.daemon.request_shutdown();
    }

    /// What an app with no key does (`specs/120` §13.1): ask on a fresh connection, and hold it open while it waits.
    fn app_asks(app_sock: &Path, seed: [u8; 32]) -> (Client, String, String) {
        let mut c = Client::connect(app_sock).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let ch = c.next_notification().unwrap();
        let (public_key, _) = app_keys::test_sign(&seed, ch["params"]["nonce"].as_str().unwrap(), ch["params"]["instance"].as_str().unwrap(), 1, &app_sock.to_string_lossy());
        let r = c.call("app.pair_request", json!({"public_key": public_key, "app_version": "0.0.0"})).unwrap();
        (c, r["code"].as_str().unwrap().to_string(), r["name"].as_str().unwrap().to_string())
    }

    /// AC-12: Enter alone approves nothing, a wrong code approves nothing, the code typed approves, with `read` only, or with
    /// `request` when the flag says so.
    #[test]
    fn approve_takes_the_code_typed_and_nothing_else() {
        let mut s = serve();
        // Nothing waiting: nothing to approve.
        let mut out: Vec<u8> = Vec::new();
        assert_eq!(approve(&mut s.console, false, &mut std::io::Cursor::new(b"\n".to_vec()), &mut out), 1);
        assert!(String::from_utf8(out).unwrap().contains("no app is waiting"));

        let (mut app, code, name) = app_asks(&s.app_sock, [9u8; 32]);
        // A process that filed a request and waits for an Enter: the empty line approves nothing, and the daemon is not asked.
        let mut out: Vec<u8> = Vec::new();
        assert_eq!(approve(&mut s.console, false, &mut std::io::Cursor::new(b"\n".to_vec()), &mut out), 1);
        let said = String::from_utf8(out).unwrap();
        assert!(said.contains("type the code shown in the app window") && said.contains("nothing was approved"), "{said}");
        assert!(!said.contains(&code), "the code is not printed here");
        // End of input is no code either.
        assert_eq!(approve(&mut s.console, false, &mut std::io::Cursor::new(Vec::new()), &mut Vec::new()), 1);
        assert!(app_keys::KeyStore::at(&s.home).load().unwrap().is_empty());
        // A wrong code is refused by the daemon.
        let wrong = if code == "AAA-AAA" { "BBB-BBB" } else { "AAA-AAA" };
        assert_eq!(approve(&mut s.console, false, &mut std::io::Cursor::new(format!("{wrong}\n").into_bytes()), &mut Vec::new()), 1);
        assert!(app_keys::KeyStore::at(&s.home).load().unwrap().is_empty());
        // The code, as typed from the window.
        let mut out: Vec<u8> = Vec::new();
        assert_eq!(approve(&mut s.console, false, &mut std::io::Cursor::new(format!("{code}\n").into_bytes()), &mut out), 0);
        let said = String::from_utf8(out).unwrap();
        assert!(said.contains(&format!("approved: {name} may use the grants read.")), "{said}");
        let keys = app_keys::KeyStore::at(&s.home).load().unwrap();
        assert_eq!((keys.len(), keys[0].grants.clone()), (1, vec!["read".to_string()]));
        assert_eq!(app.next_notification().unwrap()["params"]["outcome"], "approved");

        // `--grant request`.
        let (mut app2, code2, name2) = app_asks(&s.app_sock, [8u8; 32]);
        let mut out: Vec<u8> = Vec::new();
        assert_eq!(approve(&mut s.console, true, &mut std::io::Cursor::new(format!("{}\n", code2.to_lowercase()).into_bytes()), &mut out), 0);
        assert!(String::from_utf8(out).unwrap().contains(&format!("approved: {name2} may use the grants read, request.")));
        assert_eq!(app2.next_notification().unwrap()["params"]["outcome"], "approved");
        let keys = app_keys::KeyStore::at(&s.home).load().unwrap();
        assert_eq!(keys.iter().find(|k| k.name == name2).unwrap().grants, ["read", "request"]);
        s.daemon.request_shutdown();
    }

    #[test]
    fn the_log_says_who_did_what_in_fixed_words() {
        assert_eq!(render_event("T", "app.pair_requested", &json!({"name": "app-0a1b2c", "fingerprint": "0123456789ab", "app_version": "0.1.0"})), "T  app-0a1b2c  asked to pair (key 0123456789ab, app 0.1.0)");
        assert_eq!(render_event("T", "app.pair_approved", &json!({"name": "app-0a1b2c", "grants": ["read"]})), "T  app-0a1b2c  approved by code with read");
        assert_eq!(render_event("2026-11-15T10:00:00.000Z", "app.paired", &json!({"name": "app-0a1b2c", "grants": ["read", "request"]})), "2026-11-15T10:00:00.000Z  app-0a1b2c  paired with read, request");
        assert_eq!(render_event("T", "app.refused", &json!({"name": null, "reason": "bad_signature", "locked_out": true})), "T  -           refused: bad_signature (the socket is closed to new connections for a minute)");
        assert_eq!(render_event("T", "app.revoked", &json!({"name": "app-0a1b2c", "sessions_closed": 2})), "T  app-0a1b2c  revoked; 2 session(s) closed");
        assert_eq!(render_event("T", "app.granted", &json!({"name": "app-0a1b2c", "grant": "request", "removed": true})), "T  app-0a1b2c  lost request");
        assert_eq!(render_event("T", "app.request", &json!({"name": "app-0a1b2c", "method": "app.page_rebuild", "ids": {"slug": "x"}})), "T  app-0a1b2c  asked app.page_rebuild {\"slug\":\"x\"}");
        assert_eq!(render_event("T", "app.request", &json!({"name": "app-0a1b2c", "request": "aq-1-1", "method": "app.research_continue", "ids": {"topic_id": "tides-0123abcd"}, "state": "queued"})), "T  app-0a1b2c  asked app.research_continue {\"topic_id\":\"tides-0123abcd\"}: queued");
        assert_eq!(render_event("T", "app.request", &json!({"name": "app-0a1b2c", "method": "app.page_dispute", "ids": {"claim": "c-1"}, "state": "refused", "why": "no claim has\nthat id"})), "T  app-0a1b2c  asked app.page_dispute {\"claim\":\"c-1\"}: refused (no claim hasthat id)");
        let t = tempfile::tempdir().unwrap();
        assert!(render_log(t.path(), 200).starts_with("nothing yet"));
    }
}
