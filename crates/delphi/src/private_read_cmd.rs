//! `delphi private-read` (`specs/103` slice M1): the operator's view of the private-read marks and the observe counter, and the one way a
//! mark is cleared. OBSERVE ONLY: a mark changes no send, no read and no prompt in this slice.
//!
//! Console scope only: the daemon refuses `private_read.status` and `private_read.clear` to anything else (a tool, a host method, a relay
//! reply and a handoff cannot reach them), and this command is the operator typing it. Ids and counts only; never an entry's text.

use crate::decisions::Rpc;
use delphi_core::client::ClientError;
use serde_json::{json, Value};
use std::path::Path;

const USAGE: &str = "usage:
  delphi private-read [status]
  delphi private-read clear <key>";

fn err_text(e: &ClientError) -> String {
    match e {
        ClientError::Rpc { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

/// Strip control characters from anything the daemon returned before it is printed (the answer holds ids, but print nothing raw).
fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// The status answer as text: the live marks and the counter line.
pub fn render(v: &Value) -> String {
    let mut out = String::new();
    let marks = v["marks"].as_array().cloned().unwrap_or_default();
    out.push_str(&format!("{} task(s) marked (a marked task's web requests and connector calls ask you first)\n", marks.len()));
    for m in &marks {
        let entries: Vec<String> = m["entries"].as_array().map(|a| a.iter().filter_map(|e| e.as_str().map(safe)).collect()).unwrap_or_default();
        out.push_str(&format!("  {}  {} entr(y/ies) read: {}\n", safe(m["key"].as_str().unwrap_or("?")), entries.len(), entries.join(", ")));
    }
    out.push_str(&format!("{}\n", safe(v["line"].as_str().unwrap_or(""))));
    out
}

/// Run one `delphi private-read` command and return what to print.
pub fn handle(rpc: &mut dyn Rpc, args: &[String]) -> String {
    match args.first().map(String::as_str) {
        None | Some("status") => match rpc.call("private_read.status", json!({})) {
            Ok(v) => render(&v),
            Err(e) => format!("{}\n", err_text(&e)),
        },
        Some("clear") => match args.get(1) {
            None => format!("{}\n", USAGE),
            Some(key) => match rpc.call("private_read.clear", json!({"key": key})) {
                Ok(v) => format!("{}: cleared {} entr(y/ies); a later read of another private entry marks it again\n", safe(key), v["entries_cleared"].as_u64().unwrap_or(0)),
                Err(e) => format!("{}\n", err_text(&e)),
            },
        },
        _ => format!("{}\n", USAGE),
    }
}

/// `delphi private-read …`: connect to the workspace's daemon (starting one if needed), run the command, stop a daemon this command started.
pub fn cli(home: &Path, ws_dir: &Path, args: &[String]) -> i32 {
    use delphi_core::spawn::{connect_or_spawn, SpawnOptions};
    let Some(exe) = crate::chat::daemon_exe() else {
        eprintln!("delphi private-read: cannot locate this program to start the daemon");
        return 4;
    };
    let mut attached = match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()], own_version: Some(env!("CARGO_PKG_VERSION")) }) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi private-read: {}", err_text(&e));
            return 4;
        }
    };
    print!("{}", handle(&mut attached.client, args));
    if attached.spawned {
        let _ = attached.client.call("daemon.shutdown", json!({}));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        calls: Vec<(String, Value)>,
    }

    impl Rpc for Fake {
        fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
            self.calls.push((method.into(), params.clone()));
            match method {
                "private_read.status" => Ok(json!({"marks": [{"key": "host-abcd1234", "entries": ["host-abcd1234/n1\u{1b}[31m"], "first_at": 5}], "line": "private-read mark (observe only, changes nothing): 1 task(s) marked"})),
                "private_read.clear" if params["key"] == "host-abcd1234" => Ok(json!({"key": "host-abcd1234", "entries_cleared": 1})),
                "private_read.clear" => Err(ClientError::Rpc { code: -32602, message: "not a valid key".into(), data: None }),
                _ => Err(ClientError::Closed),
            }
        }
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_status_lists_marks_by_id_and_strips_control_characters() {
        let out = handle(&mut Fake { calls: vec![] }, &[]);
        assert!(out.contains("1 task(s) marked (a marked task's web requests and connector calls ask you first)") && out.contains("host-abcd1234  1 entr(y/ies) read: host-abcd1234/n1 [31m"), "{out:?}");
        assert!(!out.contains('\u{1b}'), "{out:?}");
    }

    #[test]
    fn clearing_names_one_key_and_says_a_later_read_marks_again() {
        let mut f = Fake { calls: vec![] };
        let out = handle(&mut f, &args(&["clear", "host-abcd1234"]));
        assert!(out.contains("cleared 1 entr") && out.contains("marks it again"), "{out}");
        assert_eq!(f.calls, vec![("private_read.clear".to_string(), json!({"key": "host-abcd1234"}))]);
        assert_eq!(handle(&mut Fake { calls: vec![] }, &args(&["clear"])), format!("{}\n", USAGE));
        assert!(handle(&mut Fake { calls: vec![] }, &args(&["clear", "../x"])).contains("not a valid key"));
        assert_eq!(handle(&mut Fake { calls: vec![] }, &args(&["bogus"])), format!("{}\n", USAGE));
    }
}
