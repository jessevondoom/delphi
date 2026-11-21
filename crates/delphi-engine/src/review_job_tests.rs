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
    Input { branch: "feature", base: "main", spec: Some(("135-x", "the spec says F2")), slice: Some("| 135 | F2 | review | built |"), stat: " a.rs | 1 +\n", diff, spec_chars: 1000 }
}

type Log = Arc<Mutex<Vec<String>>>;

fn runner(log: Log) -> impl Fn(Vec<String>, &'static str) -> std::future::Ready<CommandResult> {
    move |argv: Vec<String>, by: &'static str| {
        log.lock().unwrap().push(format!("{by}: {}", argv.join(" ")));
        std::future::ready(CommandResult { tail: format!("OUTPUT-OF-{}", argv.join("-")), argv, exit: Some(0), elapsed_ms: 1, requested_by: by, refused: None, timed_out: false })
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
    let out = review(&input(&d), &[argv(&["cargo", "test"])], true, seat, runner(ran.clone())).await;
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
    let out = review(&input(&d), &[argv(&["cargo", "test"])], false, seat, runner(ran.clone())).await;
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
