//! Spec 135 §6, AC-8 and AC-10 for the pure parts and the order of a review (the seat and the runner are closures here; the daemon
//! path is tested in `service.rs`).

use super::*;
use std::sync::{Arc, Mutex};

fn diff_of(files: &[(&str, usize)]) -> String {
    files.iter().map(|(f, n)| format!("diff --git a/{f} b/{f}\n--- a/{f}\n+++ b/{f}\n@@ -1 +1 @@\n{}", "+x\n".repeat(*n))).collect()
}

#[test]
fn the_diff_is_cut_by_whole_files_from_the_end() {
    let d = diff_of(&[("a.rs", 10), ("b.rs", 10), ("c.rs", 1)]);
    let whole = cap_diff(&d, 1_000_000);
    assert_eq!(whole.files, vec!["a.rs", "b.rs", "c.rs"]);
    assert!(!whole.truncated() && whole.text == d);
    let one = d.find("diff --git a/b.rs").unwrap();
    let c = cap_diff(&d, one + 5);
    assert_eq!(c.text, d[..one]);
    // c.rs would fit, but files are dropped from the first that does not: what is shown is always a prefix.
    assert_eq!(c.dropped, vec!["b.rs", "c.rs"]);
    assert!(cap_diff(&d, 3).text.is_empty());
    assert_eq!(cap_diff("", 100), CappedDiff { text: String::new(), files: vec![], dropped: vec![] });
}

#[test]
fn an_answer_is_read_from_json_with_its_findings_checked() {
    let a = parse_answer("Here it is:\n```json\n{\"verdict\": \"changes\", \"findings\": [{\"severity\": \"high\", \"file\": \"src/a.rs\", \"line\": 3, \"claim\": \"off by one\", \"evidence\": \"x\", \"extra\": 1}, {\"severity\": \"weird\", \"claim\": \"nit\"}, {\"file\": \"no claim\"}], \"run\": [[\"cargo\", \"test\"], [\"ls\", 3]]}\n```", true).unwrap();
    assert_eq!(a.verdict, "changes");
    assert_eq!(a.findings.len(), 2, "a finding with no claim is dropped: {:?}", a.findings);
    assert_eq!(a.findings[0], json!({"severity": "high", "file": "src/a.rs", "line": 3, "claim": "off by one", "evidence": "x"}));
    assert_eq!(a.findings[1]["severity"], "low");
    assert_eq!(a.run, vec![vec!["cargo".to_string(), "test".to_string()], vec![]], "a list with a non-string is an empty argv, refused later");
    assert!(parse_answer("{\"verdict\": \"accept\", \"findings\": [], \"run\": [[\"ls\"]]}", false).unwrap().run.is_empty(), "round 2 cannot ask");
    for bad in ["no json", "{\"verdict\": \"maybe\", \"findings\": []}", "{\"verdict\": \"accept\"}", "[1,2]"] {
        assert!(parse_answer(bad, true).is_err(), "{bad}");
    }
}

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn requested_commands_are_plain_argv_not_denied_and_at_most_three() {
    let asked = vec![argv(&["cargo", "test", "-p", "x"]), argv(&["/bin/sh", "-c", "cargo test"]), argv(&["git", "push"]), argv(&[]), argv(&["ls"]), argv(&["grep", "-r", "x"])];
    let (ok, refused) = filter_requested(&asked);
    assert_eq!(ok, vec![argv(&["cargo", "test", "-p", "x"])]);
    let why: Vec<&str> = refused.iter().map(|r| r.refused.as_deref().unwrap()).collect();
    assert!(why[0].contains("shell line"), "{why:?}");
    assert!(why[1].contains("git push"), "an investigation deny rule: {why:?}");
    assert!(why[2].contains("past the third") && why[3].contains("past the third") && why[4].contains("past the third"), "{why:?}");
    assert!(refused.iter().all(|r| r.requested_by == "seat" && !r.ran()));
    let (_, r) = filter_requested(&[argv(&["env", "X=1", "cargo"])]);
    assert!(r[0].refused.as_deref().unwrap().contains("another program"));
}

fn input<'a>(diff: &'a CappedDiff) -> Input<'a> {
    Input { branch: "feature", base: "main", spec: Some(("135-x", "the spec says F2")), slice: Some("| 135 | F2 | review | built |"), stat: " a.rs | 1 +\n", diff, spec_chars: 1000, copy: None, context_chars: 0 }
}

type Log = Arc<Mutex<Vec<String>>>;

/// A runner whose command passes: its output has the runner's result line, judged as the daemon judges it (`runner_result`).
fn runner(log: Log) -> impl Fn(Vec<String>, &'static str, Option<String>) -> std::future::Ready<CommandResult> {
    move |argv: Vec<String>, by: &'static str, result: Option<String>| {
        log.lock().unwrap().push(format!("{by}: {}", argv.join(" ")));
        let out = format!("OUTPUT-OF-{}\ntest result: ok. 1 passed; 0 failed; 0 ignored", argv.join("-"));
        let (result_seen, failed) = if by == "delphi" { runner_result(&argv, result.as_deref(), &out) } else { (None, None) };
        std::future::ready(CommandResult { tail: out, argv, exit: Some(0), elapsed_ms: 1, requested_by: by, refused: None, timed_out: false, started: true, result_seen, failed })
    }
}

#[tokio::test]
async fn ac8_configured_commands_run_requested_ones_are_filtered_and_round_two_sees_their_output() {
    let d = cap_diff(&diff_of(&[("a.rs", 1)]), 10_000);
    let (ran, briefs): (Log, Log) = Default::default();
    let b2 = briefs.clone();
    let seat = move |brief: String| {
        b2.lock().unwrap().push(brief.clone());
        let n = b2.lock().unwrap().len();
        std::future::ready(Ok(if n == 1 {
            r#"{"verdict": "unsure", "findings": [], "run": [["cargo", "test", "-p", "a"], ["git", "push"]]}"#.to_string()
        } else {
            r#"{"verdict": "changes", "findings": [{"severity": "medium", "claim": "the test fails"}]}"#.to_string()
        }))
    };
    let out = review(&input(&d), &[argv(&["cargo", "test"]).into()], true, seat, runner(ran.clone())).await;
    assert_eq!(*ran.lock().unwrap(), vec!["delphi: cargo test", "seat: cargo test -p a"]);
    let briefs = briefs.lock().unwrap();
    assert_eq!(briefs.len(), 2);
    assert!(briefs[0].contains("OUTPUT-OF-cargo-test") && briefs[0].contains("the spec says F2") && briefs[0].contains("| 135 | F2 |"), "round 1 has the spec, the slice and the tests");
    assert!(briefs[1].contains("OUTPUT-OF-cargo-test--p-a") && briefs[1].contains("refused, not run") && briefs[1].contains("FINAL answer"), "round 2 sees the requested output and the refusal");
    assert_eq!((out.rounds, out.tests_run), (2, true));
    assert_eq!(out.answer.unwrap().verdict, "changes");
    let by: Vec<(&str, bool)> = out.commands.iter().map(|c| (c.requested_by, c.ran())).collect();
    assert_eq!(by, vec![("delphi", true), ("seat", false), ("seat", true)]);
}

#[tokio::test]
async fn ac10_with_no_sandbox_nothing_runs_and_the_brief_says_so() {
    let d = cap_diff(&diff_of(&[("a.rs", 1)]), 10_000);
    let (ran, briefs): (Log, Log) = Default::default();
    let b2 = briefs.clone();
    let seat = move |brief: String| {
        b2.lock().unwrap().push(brief);
        std::future::ready(Ok(r#"{"verdict": "accept", "findings": [], "run": [["cargo", "test"]]}"#.to_string()))
    };
    let out = review(&input(&d), &[argv(&["cargo", "test"]).into()], false, seat, runner(ran.clone())).await;
    assert!(ran.lock().unwrap().is_empty(), "nothing runs without the sandbox");
    assert!(!out.tests_run && out.rounds == 1);
    assert!(briefs.lock().unwrap()[0].contains("no OS sandbox"));
    assert!(out.commands.iter().all(|c| !c.ran()) && out.commands.len() == 1, "the request is listed as refused: {:?}", out.commands);
    assert_eq!(out.answer.unwrap().verdict, "accept");
}

#[tokio::test]
async fn an_unreadable_answer_gets_one_retry_then_is_unanswered() {
    let d = cap_diff("", 100);
    let calls: Log = Default::default();
    let c2 = calls.clone();
    let seat = move |brief: String| {
        c2.lock().unwrap().push(brief);
        std::future::ready(Ok::<String, String>("I think it is fine.".to_string()))
    };
    let out = review(&input(&d), &[], true, seat, runner(Default::default())).await;
    assert_eq!(calls.lock().unwrap().len(), 2, "one retry");
    assert!(calls.lock().unwrap()[1].contains("could not be read"));
    assert_eq!((out.answer, out.unanswered.as_deref()), (None, Some("I think it is fine.")));
    // A retry that reads is the answer.
    let n: Log = Default::default();
    let seat = move |_b: String| {
        n.lock().unwrap().push(String::new());
        let first = n.lock().unwrap().len() == 1;
        std::future::ready(Ok::<String, String>(if first { "nope".into() } else { r#"{"verdict": "accept", "findings": []}"#.into() }))
    };
    assert_eq!(review(&input(&d), &[], true, seat, runner(Default::default())).await.answer.unwrap().verdict, "accept");
}

#[test]
fn a_long_output_keeps_its_end() {
    let t = tail(&format!("{}SUMMARY: 3 failed", "x".repeat(10_000)), 100);
    assert!(t.ends_with("SUMMARY: 3 failed") && t.starts_with("[... 9917 earlier characters cut ...]"), "{t}");
    assert_eq!(tail("short", 100), "short");
}

#[test]
fn the_branch_is_read_from_a_checkout_or_a_worktree_and_a_detached_head_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let main = t.path().join("repo");
    std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
    std::fs::create_dir_all(main.join("src")).unwrap();
    std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    assert_eq!(branch_of(&main.join("src")).unwrap(), "main");
    let wt = main.join(".claude/worktrees/wt");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", main.join(".git/worktrees/wt").display())).unwrap();
    std::fs::write(main.join(".git/worktrees/wt/HEAD"), "ref: refs/heads/spec-135-f2\n").unwrap();
    assert_eq!(branch_of(&wt).unwrap(), "spec-135-f2");
    std::fs::write(main.join(".git/worktrees/wt/HEAD"), "0123456789abcdef0123456789abcdef01234567\n").unwrap();
    assert!(branch_of(&wt).unwrap_err().contains("detached"));
    std::fs::write(main.join(".git/worktrees/wt/HEAD"), "ref: refs/heads/-x\n").unwrap();
    assert!(branch_of(&wt).is_err(), "a name that reads as an option is refused");
    // The project a worktree under it belongs to.
    let projects = vec![("app".to_string(), main.clone()), ("other".to_string(), t.path().join("other"))];
    assert_eq!(project_of(&projects, &wt).map(|(n, _)| n).as_deref(), Some("app"));
    assert_eq!(project_of(&projects, t.path()), None);
}

#[test]
fn the_slice_row_is_found_by_spec_number_and_slice() {
    let t = "| Spec | Slice | Content | State |\n| 135 | F2 | the review | built |\n| 135 | F0 | claims | merged |\n";
    assert_eq!(slice_row(t, "135", "f2").as_deref(), Some("| 135 | F2 | the review | built |"));
    assert_eq!(slice_row(t, "135", "F9"), None);
}

/// A runner whose configured command cannot start (the macOS sandbox's `execvp` failure) or exits non-zero, by program name.
fn failing_runner() -> impl Fn(Vec<String>, &'static str, Option<String>) -> std::future::Ready<CommandResult> {
    move |argv: Vec<String>, by: &'static str, _result: Option<String>| {
        // The program is the first word, or the word after `env`.
        let program = if argv[0].ends_with("/env") { argv[1].as_str() } else { argv[0].as_str() };
        let (exit, out) = match program {
            "cargo" => (Some(71), "sandbox-exec: execvp() of 'cargo' failed: No such file or directory".to_string()),
            "missing" => (Some(127), "env: missing: No such file or directory".to_string()),
            _ => (Some(101), "test result: FAILED. 1 failed".to_string()),
        };
        std::future::ready(CommandResult { started: !could_not_start(exit, &out), tail: out, argv, exit, elapsed_ms: 1, requested_by: by, refused: None, timed_out: false, result_seen: None, failed: None })
    }
}

/// Spec 135 AC-12 (§14.1 item 1): a command that cannot start reports `tests_run: false`, `started: false` and the fix; one that exits
/// non-zero reports `tests_run: true`.
#[tokio::test]
async fn ac12_a_command_that_cannot_start_is_not_a_test_run_and_the_note_says_how_to_fix_it() {
    assert!(could_not_start(Some(71), "sandbox-exec: execvp() of 'cargo' failed: Operation not permitted"));
    assert!(could_not_start(Some(126), "") && could_not_start(Some(127), ""));
    assert!(could_not_start(Some(1), "bwrap: execvp cargo: No such file or directory"));
    assert!(!could_not_start(Some(71), "a test that exits 71 on its own") && !could_not_start(Some(1), "FAILED") && !could_not_start(None, "execvp"));
    let d = cap_diff(&diff_of(&[("a.rs", 1)]), 10_000);
    let briefs: Log = Default::default();
    let b2 = briefs.clone();
    let seat = move |brief: String| {
        b2.lock().unwrap().push(brief);
        std::future::ready(Ok(r#"{"verdict": "unsure", "findings": []}"#.to_string()))
    };
    let out = review(&input(&d), &[argv(&["cargo", "test"]).into(), argv(&["/usr/bin/env", "missing"]).into()], true, seat, failing_runner()).await;
    assert!(!out.tests_run, "nothing started: {:?}", out.commands);
    assert!(out.commands.iter().all(|c| !c.started && c.to_json()["started"] == json!(false)), "{:?}", out.commands);
    let note = out.tests_note.as_deref().unwrap();
    assert!(note.contains("2 configured command(s) could not start") && note.contains("[exec] toolchain") && note.contains("[exec] env.PATH"), "{note}");
    assert!(briefs.lock().unwrap()[0].contains("could not start this program"), "the seat is told it did not run");
    // One that ran and failed is a test run: its exit is evidence.
    let seat = |_b: String| std::future::ready(Ok::<String, String>(r#"{"verdict": "changes", "findings": []}"#.to_string()));
    let out = review(&input(&d), &[argv(&["cargo", "test"]).into(), argv(&["make", "test"]).into()], true, seat, failing_runner()).await;
    assert!(out.tests_run, "a non-zero exit is a run");
    assert_eq!(out.commands.iter().map(|c| (c.started, c.exit)).collect::<Vec<_>>(), vec![(false, Some(71)), (true, Some(101))]);
    assert!(out.tests_note.as_deref().is_some_and(|n| n.contains("1 configured command(s)")), "the one that did not start is still named");
    // Nothing configured: no run, and the note says why.
    let seat = |_b: String| std::future::ready(Ok::<String, String>(r#"{"verdict": "accept", "findings": []}"#.to_string()));
    let out = review(&input(&d), &[], true, seat, failing_runner()).await;
    assert!(!out.tests_run && out.tests_note.as_deref().is_some_and(|n| n.contains("no command is configured")));
    assert_eq!(out.tests_reason, Some("not_configured"));
}

/// A runner that prints the output given per program and exits with the code given, judged as the daemon judges it.
fn scripted_runner(script: Vec<(&'static str, i32, &'static str)>) -> impl Fn(Vec<String>, &'static str, Option<String>) -> std::future::Ready<CommandResult> {
    move |argv: Vec<String>, by: &'static str, result: Option<String>| {
        let (_, code, out) = *script.iter().find(|(p, _, _)| argv.iter().any(|a| a == p)).expect("scripted");
        let (result_seen, failed) = runner_result(&argv, result.as_deref(), out);
        std::future::ready(CommandResult { tail: tail(out, 40), argv, exit: Some(code), elapsed_ms: 82, requested_by: by, refused: None, timed_out: false, started: true, result_seen, failed })
    }
}

#[test]
fn the_test_runners_result_is_read_from_the_whole_output() {
    let a = |v: &[&str]| argv(v);
    assert!(is_cargo_test(&a(&["cargo", "test"])) && is_cargo_test(&a(&["/x/.cargo/bin/cargo", "+nightly", "--locked", "test", "-p", "x"])) && is_cargo_test(&a(&["/usr/bin/env", "A=1", "cargo", "test"])));
    assert!(!is_cargo_test(&a(&["cargo", "clippy"])) && !is_cargo_test(&a(&["make", "test"])) && !is_cargo_test(&a(&["cargo"])));
    let two = "running 3 tests\ntest result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured\n...\ntest result: ok. 4 passed; 0 failed; 0 ignored\n";
    assert_eq!(runner_result(&a(&["cargo", "test"]), None, two), (Some(true), Some(2)), "summed over the test binaries");
    assert_eq!(runner_result(&a(&["cargo", "test"]), None, "error: could not compile `x`"), (Some(false), None));
    assert_eq!(runner_result(&a(&["make", "test"]), None, "anything"), (None, None), "no runner known: the exit status counts");
    // A configured pattern, with a `failed` group, for another runner; it overrides the cargo rule.
    let pat = r"^=+ (?:(?P<failed>\d+) failed, )?\d+ passed";
    assert_eq!(runner_result(&a(&["pytest"]), Some(pat), "== 3 failed, 9 passed in 1s ==\n"), (Some(true), Some(3)));
    assert_eq!(runner_result(&a(&["pytest"]), Some(pat), "== 9 passed in 1s ==\n"), (Some(true), None));
    assert_eq!(runner_result(&a(&["pytest"]), Some(pat), "ImportError: no module named x\n"), (Some(false), None));
    assert!(result_pattern("(").is_err() && result_pattern("").is_err() && result_pattern(&"x".repeat(MAX_RESULT_PATTERN + 1)).is_err());
}

/// Spec 135 AC-21 (§14.9 item 1): a configured command that started and stopped before any test result (F4b: `cargo` exit 1 after 82
/// ms) is `tests_run: false, reason: "setup"`, with its last lines, and the note and the brief say the tests did not run; a failing test
/// is `tests_run: true` with the failing count.
#[tokio::test]
async fn ac21_a_start_up_failure_is_setup_and_a_failing_test_is_a_test_run() {
    let d = cap_diff(&diff_of(&[("a.rs", 1)]), 10_000);
    let briefs: Log = Default::default();
    let b2 = briefs.clone();
    let seat = move |brief: String| {
        b2.lock().unwrap().push(brief);
        std::future::ready(Ok(r#"{"verdict": "unsure", "findings": []}"#.to_string()))
    };
    let setup_fail = "    Updating crates.io index\nerror: failed to get `serde` as a dependency of package `x`\n  network is unreachable";
    let out = review(&input(&d), &[argv(&["cargo", "test", "--offline"]).into()], true, seat, scripted_runner(vec![("cargo", 1, setup_fail)])).await;
    assert_eq!((out.tests_run, out.tests_reason, out.tests_failed), (false, Some("setup"), None), "{:?}", out.commands);
    let c = out.commands[0].to_json();
    assert_eq!((c["tests_run"].clone(), c["reason"].clone(), c["started"].clone(), c["exit"].clone()), (json!(false), json!("setup"), json!(true), json!(1)), "{c}");
    assert!(c["tail"].as_str().unwrap().ends_with("network is unreachable"), "its last lines are kept: {c}");
    let note = out.tests_note.as_deref().unwrap();
    assert!(note.contains("showed no test result") && note.contains("the tests did not run") && note.contains("`cargo`"), "{note}");
    assert!(briefs.lock().unwrap()[0].contains("no test result: it stopped before the tests ran"), "the seat is told too");

    // A failing test is a test run, with the count; a check command beside it (no runner) counts by its exit status as before.
    let seat = |_b: String| std::future::ready(Ok::<String, String>(r#"{"verdict": "changes", "findings": []}"#.to_string()));
    let failing = "test a ... FAILED\ntest result: FAILED. 5 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out";
    let out = review(&input(&d), &[argv(&["cargo", "test"]).into(), argv(&["make", "lint"]).into()], true, seat, scripted_runner(vec![("cargo", 101, failing), ("make", 0, "ok")])).await;
    assert_eq!((out.tests_run, out.tests_reason, out.tests_failed), (true, None, Some(2)));
    let c = out.commands[0].to_json();
    assert_eq!((c["tests_run"].clone(), c["failed"].clone(), c.get("reason")), (json!(true), json!(2), None), "{c}");
    assert_eq!(out.commands[1].to_json().get("tests_run"), None, "no runner known: no judgement either way");
    assert_eq!(out.tests_note, None);

    // A setup failure beside a check that passed: tests_run stays true by the exit-status rule, and the note still names the setup failure.
    let seat = |_b: String| std::future::ready(Ok::<String, String>(r#"{"verdict": "accept", "findings": []}"#.to_string()));
    let out = review(&input(&d), &[argv(&["cargo", "test"]).into(), argv(&["make", "lint"]).into()], true, seat, scripted_runner(vec![("cargo", 1, setup_fail), ("make", 0, "ok")])).await;
    assert!(out.tests_run && out.tests_note.as_deref().is_some_and(|n| n.contains("1 configured command(s) started but showed no test result")), "{:?}", out.tests_note);
    // A configured pattern decides for its own runner.
    let seat = |_b: String| std::future::ready(Ok::<String, String>(r#"{"verdict": "accept", "findings": []}"#.to_string()));
    let cmd = ReviewCommand { argv: argv(&["pytest", "-q"]), result: Some(r"(?P<failed>\d+) failed".into()) };
    let out = review(&input(&d), &[cmd], true, seat, scripted_runner(vec![("pytest", 2, "ModuleNotFoundError: x")])).await;
    assert_eq!((out.tests_run, out.tests_reason), (false, Some("setup")));
}

fn ev(kind: &str, payload: Value) -> delphi_core::event::EventEnvelope {
    let mut e = delphi_core::event::EventEnvelope::new("m", kind, payload);
    e.task_id = Some("t1".into());
    e
}

fn receipt(endpoint: &str, outcome: &str) -> delphi_core::event::EventEnvelope {
    ev(crate::seat::RECEIPT_KIND, json!({"requested": {"endpoint": endpoint, "model": "m"}, "served": {"model": "gemini-x", "family": "gemini"}, "outcome": outcome}))
}

/// Spec 135 AC-23 (§14.9 item 3): the result names the endpoint that served each round, and a round that fell back to another endpoint of
/// the family says from which, to which, and why (F4b: a fall-back to `gemini-key` that timed out was invisible).
#[test]
fn ac23_the_endpoint_of_each_round_and_any_fallback_are_named() {
    let events = vec![
        // Round 0 on the CLI endpoint.
        receipt("gemini-cli", "ok"),
        // Round 1: the CLI gave no first event, the span fell back to the API, where `gemini-key` timed out.
        ev("harness.fallback", json!({"from": "gemini-cli", "to": "api", "reason": "agy gave no first event"})),
        receipt("gemini-key", "error"),
        // Round 1's repair call (a second call of the same round), then round 2 out of quota on one key and served by another.
        receipt("gemini-key", "ok"),
        ev("model.exhausted", json!({"endpoint_id": "gemini-key", "model_id": "m", "reason": "daily quota"})),
        receipt("gemini-key-2", "ok"),
    ];
    let rounds = served_by_round(&events, &[1, 3, 4, 6], &[0, 1, 1, 2]);
    assert_eq!(rounds.len(), 3, "{rounds:?}");
    assert_eq!((rounds[0]["round"].clone(), rounds[0]["endpoint"].clone(), rounds[0]["fallback"].clone()), (json!(0), json!("gemini-cli"), json!([])));
    assert_eq!((rounds[1]["endpoint"].clone(), rounds[1]["family"].clone(), rounds[1]["model"].clone(), rounds[1]["outcome"].clone()), (json!("gemini-key"), json!("gemini"), json!("gemini-x"), json!("ok")));
    assert_eq!(rounds[1]["fallback"], json!([{"from": "gemini-cli", "to": "gemini-key", "why": "agy gave no first event"}]), "`to: api` names the endpoint that served");
    assert_eq!(rounds[2]["endpoint"], "gemini-key-2");
    assert_eq!(rounds[2]["fallback"], json!([{"from": "gemini-key", "to": "gemini-key-2", "why": "out of quota: daily quota"}]));
    let line = fallback_line(&rounds).unwrap();
    assert!(line.contains("round 1 fell back from `gemini-cli` to `gemini-key` (agy gave no first event)") && line.contains("round 2 fell back from `gemini-key` to `gemini-key-2`"), "{line}");
    // A review with no fall-back says nothing; a round whose only receipt failed still names its endpoint.
    let plain = served_by_round(&[receipt("gemini-cli", "error")], &[1], &[1]);
    assert_eq!((plain[0]["endpoint"].clone(), plain[0]["outcome"].clone()), (json!("gemini-cli"), json!("error")));
    assert_eq!(fallback_line(&plain), None);
}

/// §14.9 item 3: `review` notes the round of every seat call, a repair included, so the daemon can split the receipts by round.
#[tokio::test]
async fn every_seat_call_is_noted_with_its_round() {
    let d = cap_diff(DIFF14, 10_000);
    let c = corpus();
    let n: Log = Default::default();
    let n2 = n.clone();
    let seat = move |brief: String| {
        n2.lock().unwrap().push(brief.clone());
        let k = n2.lock().unwrap().len();
        std::future::ready(Ok::<String, String>(match k {
            1 => r#"{"need": []}"#.into(),
            2 => "not json".into(),
            3 => r#"{"verdict": "unsure", "findings": [], "run": [["ls"]]}"#.into(),
            _ => r#"{"verdict": "accept", "findings": []}"#.into(),
        }))
    };
    let out = review(&Input { copy: Some(&c), context_chars: 1000, ..input(&d) }, &[], true, seat, runner(Default::default())).await;
    assert_eq!((out.seat_calls.clone(), out.rounds), (vec![0, 1, 1, 2], 2));
}

/// A copy of a small project, as the review would read it.
fn corpus() -> Corpus {
    let f = |p: &str, t: &str| (p.to_string(), t.to_string());
    Corpus {
        files: vec![
            f("src/lib.rs", "pub fn changed_fn(x: u32) -> u32 {\n    x + 1\n}\n"),
            f("src/user.rs", &format!("{}fn caller() {{\n    let y = changed_fn(3);\n    not_changed_fn(4);\n}}\n", "// filler\n".repeat(20))),
            f("src/types.rs", "use crate::x;\n/// The type the seat asks for.\npub struct NeededType {\n    pub a: u32,\n}\nfn mentions(_: NeededType) {}\n"),
            f("src/msgs.rs", "pub const M: &str = \"a literal message\";\n"),
        ],
    }
}

const DIFF14: &str = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@\n-pub fn changed_fn(x: u32) -> u32 {\n+pub fn changed_fn(x: u32) -> u32 {\n-    x\n+    x + 1\n }\n";

#[test]
fn the_diffs_functions_and_added_lines_are_read_from_its_hunks() {
    let d = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -10,4 +10,5 @@ impl Foo { pub fn in_header(&self) {\n ctx\n-    fn removed_one() {}\n+    pub(crate) async fn added_one<T>(t: T) {}\n+    fn new() -> Self {}\n ctx2\n+let x = confn x;\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-fn not_rust()\n+fn not_rust_either()\n";
    let s = diff_shape(d);
    assert_eq!(s.fns, vec!["in_header", "removed_one", "added_one"], "`new` is too common to search for; only .rs files count");
    assert_eq!(s.added["src/a.rs"], [11, 12, 14].into_iter().collect());
    assert_eq!(s.added["README.md"], [1].into_iter().collect());
}

#[test]
fn round_zeros_items_are_plain_identifiers_or_short_quoted_strings_at_most_five() {
    let long = format!("\"{}\"", "x".repeat(81));
    let answer = json!({"need": ["NeededType", "\"a literal message\"", "foo bar", "_ok", long, "past_five", 7]}).to_string();
    let (ok, refused) = parse_needs(&answer).unwrap();
    assert_eq!(ok, vec![Need::Ident("NeededType".into()), Need::Literal("a literal message".into()), Need::Ident("_ok".into())]);
    assert_eq!(refused.len(), 4, "{refused:?}");
    assert!(refused[0].contains("foo bar") && refused[0].contains("not a plain identifier"), "{refused:?}");
    assert!(refused[1].contains("not a plain identifier") && refused[1].chars().count() < 200, "a long item is cut in the refusal: {refused:?}");
    assert!(refused[2].contains("past_five") && refused[2].contains("past the fifth") && refused[3].contains("not a string"), "{refused:?}");
    assert!(parse_needs(&json!({"need": ["a::b", "rm -rf /"]}).to_string()).unwrap().1.len() == 2, "a path or a shell line is not an identifier");
    assert_eq!(parse_needs("{\"need\": []}").unwrap(), (vec![], vec![]), "asking for nothing is allowed");
    assert_eq!(parse_needs("{\"verdict\": \"accept\"}").unwrap(), (vec![], vec![]));
    assert!(parse_needs("{\"need\": \"NeededType\"}").is_err() && parse_needs("nothing").is_err());
    assert!(parse_needs(&json!({"need": ["\"\"", "\"line\nbreak\""]}).to_string()).unwrap().1.len() == 2, "an empty or multi-line string is refused");
}

/// Spec 135 AC-14 (§14.3): with a scripted seat, Round 0's requested identifier and string and the diff's call sites reach Round 1's
/// brief, fenced and within the cap; a refused item is named and not searched.
#[tokio::test]
async fn ac14_round_zeros_items_and_the_call_sites_reach_round_one_within_the_cap() {
    let d = cap_diff(DIFF14, 10_000);
    let c = corpus();
    let briefs: Log = Default::default();
    let b2 = briefs.clone();
    let seat = move |brief: String| {
        b2.lock().unwrap().push(brief.clone());
        std::future::ready(Ok(if brief.contains("ROUND 0") {
            r#"{"need": ["NeededType", "\"a literal message\"", "rm -rf /"]}"#.to_string()
        } else {
            r#"{"verdict": "accept", "findings": []}"#.to_string()
        }))
    };
    let ran: Log = Default::default();
    let i = Input { copy: Some(&c), context_chars: DEFAULT_CONTEXT_CHARS, ..input(&d) };
    let out = review(&i, &[argv(&["cargo", "test"]).into()], true, seat, runner(ran.clone())).await;
    let briefs = briefs.lock().unwrap().clone();
    assert_eq!(briefs.len(), 2, "round 0, then round 1");
    assert!(briefs[0].contains("ROUND 0") && briefs[0].contains("changed_fn") && !briefs[0].contains("CALL SITES") && !briefs[0].contains("OUTPUT-OF"), "round 0 sees the diff only");
    let b1 = &briefs[1];
    let fenced = |label: &str| {
        let start = b1.find(&format!("----- BEGIN {label}")).unwrap_or_else(|| panic!("no {label} section: {b1}"));
        let end = b1[start..].find(&format!("----- END {label}")).unwrap() + start;
        b1[start..end].to_string()
    };
    let sites = fenced("CALL SITES");
    assert!(sites.contains("call site of changed_fn") && sites.contains("src/user.rs") && sites.contains("let y = changed_fn(3);"), "{sites}");
    assert_eq!(sites.matches("call site of").count(), 1, "a longer name (not_changed_fn) is not a call site: {sites}");
    let asked = fenced("WHAT YOU ASKED TO SEE");
    assert!(asked.contains("src/types.rs") && asked.contains("pub struct NeededType"), "{asked}");
    assert!(asked.contains("src/msgs.rs") && asked.contains("a literal message"), "{asked}");
    assert!(asked.contains("Refused, not searched") && asked.contains("rm -rf /"), "{asked}");
    assert!(b1.find("BEGIN CALL SITES").unwrap() > b1.find("BEGIN DIFF (").unwrap() && b1.contains("OUTPUT-OF-cargo-test"), "round 1 still has the diff and the tests");
    assert_eq!(out.answer.unwrap().verdict, "accept");
    assert_eq!(out.context["functions"], json!(["changed_fn"]));
    assert_eq!((out.context["call_sites"].as_u64(), out.context["refused"].as_array().map(Vec::len)), (Some(1), Some(1)), "{}", out.context);
    assert_eq!(out.context["asked"], json!([{"item": "NeededType", "hits": 2}, {"item": "\"a literal message\"", "hits": 1}]));
    assert_eq!(*ran.lock().unwrap(), vec!["delphi: cargo test"]);

    // Within the cap: a small cap keeps what fits whole and counts the rest.
    let ctx = gather_context(&c, &d, Ok((vec![Need::Ident("NeededType".into()), Need::Literal("a literal message".into())], vec![])), 400);
    let excerpts: usize = ctx.brief.split("----- BEGIN ").skip(1).map(|s| s.split_once('\n').map(|(_, b)| b.split("----- END ").next().unwrap_or("").chars().count()).unwrap_or(0)).sum();
    assert!(excerpts <= 400, "{excerpts}: {}", ctx.brief);
    assert!(ctx.report["left_out"].as_u64().unwrap() > 0 && ctx.brief.contains("left out to keep the context under 400"), "{}", ctx.brief);
    // 0 turns the context and Round 0 off.
    let n: Log = Default::default();
    let n2 = n.clone();
    let seat = move |b: String| {
        n2.lock().unwrap().push(b);
        std::future::ready(Ok::<String, String>(r#"{"verdict": "accept", "findings": []}"#.into()))
    };
    let out = review(&Input { copy: Some(&c), context_chars: 0, ..input(&d) }, &[], true, seat, runner(Default::default())).await;
    assert_eq!((n.lock().unwrap().len(), out.context.clone()), (1, Value::Null));
    // A seat that gives no answer in Round 0 stops the review before any command runs.
    let ran: Log = Default::default();
    let seat = |_b: String| std::future::ready(Err::<String, String>("quota".into()));
    let out = review(&Input { copy: Some(&c), context_chars: 1000, ..input(&d) }, &[argv(&["cargo", "test"]).into()], true, seat, runner(ran.clone())).await;
    assert!(out.answer.is_none() && out.unanswered.as_deref().is_some_and(|u| u.contains("round 0")) && ran.lock().unwrap().is_empty());
}

/// AC-14: the search reads only the copy: a link to a file or a directory outside it is not followed; `.git` and build output are
/// skipped; a link to a file inside the copy is read.
#[test]
fn ac14_nothing_outside_the_copy_is_read() {
    let t = tempfile::tempdir().unwrap();
    let (copy, outside) = (t.path().join("copy"), t.path().join("outside"));
    for d in [copy.join("src"), copy.join(".git"), copy.join("target/debug"), outside.join("d")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(copy.join("src/a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(copy.join(".git/HEAD"), "SECRET_GIT\n").unwrap();
    std::fs::write(copy.join("target/debug/x.rs"), "SECRET_TARGET\n").unwrap();
    std::fs::write(outside.join("secret.rs"), "SECRET_OUTSIDE\n").unwrap();
    std::fs::write(outside.join("d/more.rs"), "SECRET_DIR\n").unwrap();
    std::os::unix::fs::symlink(outside.join("secret.rs"), copy.join("src/out.rs")).unwrap();
    std::os::unix::fs::symlink(outside.join("d"), copy.join("linkdir")).unwrap();
    std::os::unix::fs::symlink(copy.join("src/a.rs"), copy.join("inside.rs")).unwrap();
    std::os::unix::fs::symlink(copy.join(".git/HEAD"), copy.join("src/gitlink.rs")).unwrap();
    std::os::unix::fs::symlink("../../outside/secret.rs", copy.join("src/rel.rs")).unwrap();
    std::fs::write(copy.join("big.txt"), "x".repeat(600 * 1024)).unwrap();
    std::fs::write(copy.join("bin.dat"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    let c = Corpus::load(&copy);
    let paths: Vec<&str> = c.files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, vec!["inside.rs", "src/a.rs"]);
    assert!(c.files.iter().all(|(_, t)| !t.contains("SECRET")), "{c:?}");
    // A search for what lies outside finds nothing.
    let ctx = gather_context(&c, &cap_diff("", 100), Ok((vec![Need::Ident("SECRET_OUTSIDE".into()), Need::Literal("SECRET_DIR".into())], vec![])), 5_000);
    assert_eq!(ctx.report["excerpts"], 0, "{}", ctx.report);
    assert!(ctx.brief.contains("(nothing found)"));
}
