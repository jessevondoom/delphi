//! Who may call what (spec 093 J1). The daemon's socket fixes a caller's scope (console, narrator, host, relay), and the
//! scope is what stops a seat, the narrator or a relay from changing a limit, answering an approval or merging. This reads
//! the dispatcher and the handlers it names and requires that every RPC method either checks the caller's scope at the top of
//! its handler or is on a short, reviewed list of getters. A new method that changes something and checks nothing is a red
//! build, not a surprise. It reads source text, like `xenia_scope_tests`: a net for forgetting the check, not a proof that the
//! check is right (each gated handler's own tests do that).

use regex::Regex;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Methods any scope may call because they only read. Adding one is a decision: say what it reads and who may see it.
const OPEN_GETTERS: &[&str] = &[
    "approval.list", "budget.get", "decision.get", "decision.list", "decisions.list", "kb.get", "kb.search", "models.get", "observe.snapshot", "observe.state",
    "permissions.get", "ratify.list", "session.snapshot", "timer.list", "usage.get",
];

/// Open getters whose ANSWER depends on the caller's scope without refusing any scope (`specs/103` §4.2, slice M3): a relay asking for the pending
/// cards is sent the list with the details of a withheld card replaced. Each must also be on [`OPEN_GETTERS`], and its handler must refuse no scope
/// (it names none of the `need_*` checks), so naming `ctx.scope` to choose what to return is not read as a gate.
const SCOPE_AWARE_GETTERS: &[&str] = &["approval.list"];

/// What counts as a scope check in a handler's opening lines.
const GATES: &[&str] = &["need_console(", "need_host(", "need_relay(", "need_narrator(", "ctx.scope"];

/// How far into a handler to look for the check, and what in a getter would show it is not one.
const WINDOW: usize = 16;
const WRITES: &[&str] = &[".append(", "fs::write(", "write_all(", "remove_file(", "rename("];

fn sources() -> Vec<(String, Vec<String>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = vec![];
    // Production code ends where the test module starts. A single `#[cfg(test)]` item earlier in the file (a test-only
    // constant, say) is not the end of it; a first draft cut there and lost the dispatcher.
    let test_mod = Regex::new(r"(?m)^#\[cfg\(test\)\]\s*\n(?:#\[[^\n]*\]\s*\n)*(?:pub(?:\([a-z]+\))?\s+)?mod\s").unwrap();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("service") && name.ends_with(".rs") && !name.ends_with("_tests.rs") {
            let text = std::fs::read_to_string(e.path()).unwrap();
            let prod = match test_mod.find(&text) {
                Some(m) => text[..m.start()].to_string(),
                None => text,
            };
            out.push((name, prod.split('\n').map(String::from).collect()));
        }
    }
    out.sort();
    out
}

fn fn_start() -> Regex {
    Regex::new(r"^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:async\s+)?fn\s+([a-z_0-9]+)\s*[<(]").unwrap()
}

/// A handler's opening lines: from its signature to the next function or `WINDOW` lines.
fn body_of(name: &str, files: &[(String, Vec<String>)]) -> Option<String> {
    let re = fn_start();
    for (_, lines) in files {
        for (i, l) in lines.iter().enumerate() {
            if re.captures(l).is_some_and(|c| &c[1] == name) {
                let mut out = vec![l.as_str()];
                for nxt in lines.iter().skip(i + 1).take(WINDOW) {
                    if re.is_match(nxt) {
                        break;
                    }
                    out.push(nxt.as_str());
                }
                return Some(out.join("\n"));
            }
        }
    }
    None
}

/// Each dispatcher arm as (method, the arm's own text, the handler name when it calls `svc.<handler>(`).
fn arms(files: &[(String, Vec<String>)]) -> Vec<(String, String, Option<String>)> {
    let arm = Regex::new(r#"^\s*"([a-z_]+(?:\.[a-z_]+)+)"\s*=>"#).unwrap();
    let handler = Regex::new(r"svc\.([a-z_]+)\(").unwrap();
    let main = &files.iter().find(|(n, _)| n == "service.rs").expect("service.rs").1;
    let starts: Vec<usize> = (0..main.len()).filter(|i| arm.is_match(&main[*i])).collect();
    let mut out = vec![];
    for (k, &i) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(i + 12).min(i + 14).min(main.len());
        let own = main[i..end].join("\n");
        let method = arm.captures(&main[i]).unwrap()[1].to_string();
        let h = handler.captures(&main[i]).map(|c| c[1].to_string());
        out.push((method, own, h));
    }
    out
}

fn gated(text: &str) -> bool {
    GATES.iter().any(|g| text.contains(g))
}

/// The methods with no scope check, and the handlers that were not found.
fn ungated(files: &[(String, Vec<String>)]) -> (BTreeSet<String>, Vec<String>) {
    let (mut open, mut missing) = (BTreeSet::new(), vec![]);
    for (method, own, h) in arms(files) {
        let mut text = own;
        if let Some(h) = &h {
            match body_of(h, files) {
                Some(b) => text = format!("{text}\n{b}"),
                None => missing.push(format!("{method}: handler `{h}` not found in src/service*.rs")),
            }
        }
        // a scope-aware getter chooses what to return by scope; it is a gate only if it refuses one (a `need_*` check)
        let refuses = ["need_console(", "need_host(", "need_relay(", "need_narrator("].iter().any(|g| text.contains(g));
        if !gated(&text) || (SCOPE_AWARE_GETTERS.contains(&method.as_str()) && !refuses) {
            open.insert(method);
        }
    }
    (open, missing)
}

#[test]
fn every_rpc_method_checks_the_callers_scope_or_is_a_reviewed_getter() {
    let files = sources();
    let all = arms(&files);
    assert!(all.len() >= 100, "the scan found the dispatcher ({} arms)", all.len());
    let (open, missing) = ungated(&files);
    assert!(missing.is_empty(), "{missing:?}");
    let listed: BTreeSet<String> = OPEN_GETTERS.iter().map(|s| s.to_string()).collect();
    for m in SCOPE_AWARE_GETTERS {
        assert!(listed.contains(*m) && open.contains(*m), "{m} is scope-aware: it must stay on OPEN_GETTERS and refuse no scope");
    }
    let new: Vec<&String> = open.difference(&listed).collect();
    let stale: Vec<&String> = listed.difference(&open).collect();
    assert!(new.is_empty(), "these RPC methods check no scope at the top of their handler and are not on OPEN_GETTERS: {new:?}. Gate them (need_console / need_host / a ctx.scope check) or, if they only read, add them to OPEN_GETTERS with a reason");
    assert!(stale.is_empty(), "OPEN_GETTERS lists methods that are gated now or no longer exist: {stale:?}. Remove them");
}

#[test]
fn an_open_getter_writes_nothing_in_its_opening_lines() {
    let files = sources();
    for (method, own, h) in arms(&files) {
        if !OPEN_GETTERS.contains(&method.as_str()) {
            continue;
        }
        let text = format!("{own}\n{}", h.and_then(|h| body_of(&h, &files)).unwrap_or_default());
        for w in WRITES {
            assert!(!text.contains(w), "{method} is on OPEN_GETTERS but its handler mentions `{w}`: a getter must not write");
        }
    }
}

#[test]
fn the_narrator_can_call_only_getters() {
    // The narrator socket answers a fixed list (spec 005 D-25); every method on it that is a dispatcher arm must be a getter.
    let files = sources();
    let all: BTreeSet<String> = arms(&files).into_iter().map(|(m, _, _)| m).collect();
    for m in delphi_core::server::NARRATOR_METHODS {
        if all.contains(m) {
            assert!(OPEN_GETTERS.contains(&m), "{m} is callable on the narrator socket but is not a reviewed getter");
        }
    }
}

#[test]
fn the_scan_knows_a_gate_from_a_neighbours_gate() {
    // Spec 093 AC-J1: a handler is judged on its own lines. This is the shape that fooled a first draft of the scan.
    let src = "fn first() {\n    need_console(&ctx)?;\n}\nfn second(&self) -> R {\n    Ok(json!({}))\n}\n";
    let files = vec![("service.rs".to_string(), src.split('\n').map(String::from).collect::<Vec<_>>())];
    assert!(gated(&body_of("first", &files).unwrap()));
    assert!(!gated(&body_of("second", &files).unwrap()), "second must not borrow first's gate");
}
