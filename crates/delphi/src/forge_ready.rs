//! `delphi forge ready <PR>` (spec 135 §14.13, I1): mark a draft ready only once the CI run of its latest push has registered,
//! then confirm that the ready event started a run of its own (macOS runs only on that one), toggling draft and ready once when
//! it did not. Every `gh` call is read-only except `gh pr ready` (and its `--undo`). The process starts through the gates' one
//! spawn site ([`Runner`], `finisher_gates::SystemRunner`), so the tests use a fake and a sleep that only counts.

use crate::finisher_gates::{GateCmd, Runner};
use crate::slice_claims::Out;
use std::path::Path;
use std::time::Duration;

/// How often the runs are listed while waiting.
pub const POLL: Duration = Duration::from_secs(15);
/// The default bound of each wait.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(180);

/// The pull request as `gh pr view` gives it.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pr {
    state: String,
    is_draft: bool,
    head_ref_name: String,
    head_ref_oid: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    database_id: u64,
    head_sha: String,
}

/// The JSON in `gh`'s output (the runner joins standard output and error; a stray line on either side is skipped).
fn json_part(s: &str, open: char, close: char) -> &str {
    match (s.find(open), s.rfind(close)) {
        (Some(a), Some(b)) if a <= b => &s[a..=b],
        _ => "",
    }
}

struct Gh<'a> {
    repo: &'a Path,
    runner: &'a mut dyn Runner,
}

impl Gh<'_> {
    fn call(&mut self, args: &[&str]) -> Result<String, String> {
        let r = self.runner.run(self.repo, &GateCmd { program: "gh".into(), args: args.iter().map(|a| a.to_string()).collect(), env: Vec::new() });
        match (r.started, r.ok) {
            (true, true) => Ok(r.output),
            _ => Err(r.output.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("gh failed").chars().filter(|c| !c.is_control()).take(200).collect()),
        }
    }

    /// The ids of the runs of commit `sha` on `branch`.
    fn runs(&mut self, branch: &str, sha: &str) -> Result<Vec<u64>, String> {
        let out = self.call(&["run", "list", "--branch", branch, "--json", "databaseId,headSha,event", "--limit", "50"])?;
        let runs: Vec<Run> = serde_json::from_str(json_part(&out, '[', ']')).map_err(|e| format!("cannot read `gh run list`: {}", e))?;
        Ok(runs.into_iter().filter(|r| r.head_sha == sha).map(|r| r.database_id).collect())
    }

    /// Polls until `found` holds for the runs of `sha`, at most `tries` lists; returns the runs then, or `None`.
    fn wait_for(&mut self, branch: &str, sha: &str, tries: u32, sleep: &mut dyn FnMut(Duration), found: &dyn Fn(&[u64]) -> bool) -> Result<Option<Vec<u64>>, String> {
        for i in 0..tries {
            if i > 0 {
                sleep(POLL);
            }
            let ids = self.runs(branch, sha)?;
            if found(&ids) {
                return Ok(Some(ids));
            }
        }
        Ok(None)
    }
}

fn end(code: i32, out: String, err: String) -> Out {
    Out { code, out, err }
}

/// `forge ready <PR>`: `wait` bounds each wait; `sleep` waits between two lists.
pub fn ready(repo: &Path, pr: &str, wait: Duration, runner: &mut dyn Runner, sleep: &mut dyn FnMut(Duration)) -> Out {
    if pr.is_empty() || !pr.chars().all(|c| c.is_ascii_digit()) {
        return end(2, String::new(), format!("delphi forge ready: `{}` is not a pull request number", pr));
    }
    let tries = (wait.as_secs() / POLL.as_secs()).max(1) as u32 + 1;
    let mut gh = Gh { repo, runner };
    let mut out = String::new();
    let view = match gh.call(&["pr", "view", pr, "--json", "number,state,isDraft,headRefName,headRefOid"]) {
        Ok(v) => v,
        Err(e) => return end(4, out, format!("gh pr view {} failed: {}", pr, e)),
    };
    let p: Pr = match serde_json::from_str(json_part(&view, '{', '}')) {
        Ok(p) => p,
        Err(e) => return end(4, out, format!("cannot read `gh pr view {}`: {}", pr, e)),
    };
    if p.state != "OPEN" {
        return end(1, out, format!("#{} is {}, not open: nothing to mark ready", pr, p.state.to_lowercase()));
    }
    if !p.is_draft {
        out.push_str(&format!("#{} is already ready; nothing done. If macOS did not run on its head, run `gh pr ready {} --undo` and then this command.\n", pr, pr));
        return end(0, out, String::new());
    }
    let branch = p.head_ref_name;
    if branch.starts_with('-') || branch.chars().any(|c| c.is_control()) || branch.is_empty() {
        return end(4, out, format!("#{}'s head branch name cannot be passed to gh", pr));
    }
    let sha = p.head_ref_oid;
    let short = &sha[..sha.len().min(12)];
    let steps = |gh: &mut Gh, tries: u32, sleep: &mut dyn FnMut(Duration), found: &dyn Fn(&[u64]) -> bool| gh.wait_for(&branch, &sha, tries, sleep, found);
    // 1. The push's own run must exist before the ready event, or GitHub may never run the full set on this head.
    let before = match steps(&mut gh, tries, sleep, &|ids| !ids.is_empty()) {
        Ok(Some(ids)) => ids,
        Ok(None) => return end(1, out, format!("no CI run registered for #{}'s head {} within {} s; NOT marked ready. Wait for the push's run (`gh run list --branch {}`) and run this again.", pr, short, wait.as_secs(), branch)),
        Err(e) => return end(4, out, format!("gh run list failed: {}", e)),
    };
    out.push_str(&format!("the push's run of {} registered (run {})\n", short, before[0]));
    // 2. Ready, then a run of the ready event; toggled once when none comes.
    for toggle in [false, true] {
        if toggle {
            if let Err(e) = gh.call(&["pr", "ready", pr, "--undo"]) {
                return end(4, out, format!("gh pr ready {} --undo failed: {}", pr, e));
            }
            out.push_str(&format!("no new run within {} s: toggled #{} back to draft and ready again\n", wait.as_secs(), pr));
        }
        if let Err(e) = gh.call(&["pr", "ready", pr]) {
            return end(4, out, format!("gh pr ready {} failed: {}", pr, e));
        }
        if !toggle {
            out.push_str(&format!("marked #{} ready\n", pr));
        }
        let new = |ids: &[u64]| ids.iter().any(|i| !before.contains(i));
        match steps(&mut gh, tries, sleep, &new) {
            Ok(Some(ids)) => {
                let run = ids.iter().find(|i| !before.contains(i)).copied().unwrap_or_default();
                out.push_str(&format!("the ready event started run {} on {}: the full set (macOS included) runs there\n", run, short));
                return end(0, out, String::new());
            }
            Ok(None) => {}
            Err(e) => return end(4, out, format!("gh run list failed: {}", e)),
        }
    }
    end(1, out, format!("#{} is ready, but no second run of {} appeared even after a toggle: check `gh pr checks {}` for the macOS jobs", pr, short, pr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finisher_gates::Ran;
    use std::collections::VecDeque;

    /// Answers each `gh` call with the next scripted result and records the arguments.
    struct FakeGh {
        answers: VecDeque<(bool, String)>,
        ran: Vec<String>,
    }

    impl Runner for FakeGh {
        fn run(&mut self, _repo: &Path, cmd: &GateCmd) -> Ran {
            assert_eq!(cmd.program, Path::new("gh"));
            self.ran.push(cmd.args.join(" "));
            let (ok, output) = self.answers.pop_front().unwrap_or_else(|| panic!("an unexpected call: gh {}", cmd.args.join(" ")));
            Ran { started: true, ok, output }
        }
    }

    const SHA: &str = "abc123def4567890";
    const DRAFT: &str = r#"{"number":7,"state":"OPEN","isDraft":true,"headRefName":"feat-x","headRefOid":"abc123def4567890"}"#;

    fn runs(ids: &[u64]) -> String {
        let mut v: Vec<String> = ids.iter().map(|i| format!(r#"{{"databaseId":{},"headSha":"{}","event":"pull_request"}}"#, i, SHA)).collect();
        v.push(r#"{"databaseId":1,"headSha":"0ld","event":"pull_request"}"#.into());
        format!("[{}]", v.join(","))
    }

    fn go(answers: Vec<(bool, String)>) -> (Out, Vec<String>, u32) {
        let mut f = FakeGh { answers: answers.into(), ran: Vec::new() };
        let mut slept = 0;
        let o = ready(Path::new("/r"), "7", Duration::from_secs(30), &mut f, &mut |d| {
            assert_eq!(d, POLL);
            slept += 1;
        });
        assert!(f.answers.is_empty(), "unused answers: {:?}", f.answers);
        (o, f.ran, slept)
    }

    #[test]
    fn ac44_not_ready_until_the_pushs_run_exists() {
        // 30 s with a 15 s poll: three lists, none with a run of the head.
        let (o, ran, slept) = go(vec![(true, DRAFT.into()), (true, runs(&[])), (true, runs(&[])), (true, "[]".into())]);
        assert_eq!(o.code, 1, "{}", o.err);
        assert!(o.err.contains("NOT marked ready"));
        assert!(!ran.iter().any(|a| a.starts_with("pr ready")));
        assert_eq!(slept, 2);
        assert_eq!(ran[1], "run list --branch feat-x --json databaseId,headSha,event --limit 50");
    }

    #[test]
    fn ac44_ac45_marked_once_the_run_exists_and_no_toggle_when_a_second_run_comes() {
        let (o, ran, _) = go(vec![(true, DRAFT.into()), (true, runs(&[])), (true, runs(&[10])), (true, "".into()), (true, runs(&[10])), (true, format!("warning: x\n{}", runs(&[10, 11])))]);
        assert_eq!(o.code, 0, "{}", o.err);
        assert_eq!(ran.iter().filter(|a| a.starts_with("pr ready")).count(), 1);
        assert!(o.out.contains("started run 11"));
        assert!(!o.out.contains("toggled"));
    }

    #[test]
    fn ac45_no_second_run_toggles_draft_and_ready_once() {
        let mut a = vec![(true, DRAFT.to_string()), (true, runs(&[10])), (true, String::new())];
        a.extend((0..3).map(|_| (true, runs(&[10]))));
        a.extend([(true, String::new()), (true, String::new()), (true, runs(&[10, 12]))]);
        let (o, ran, _) = go(a);
        assert_eq!(o.code, 0, "{}", o.err);
        assert!(o.out.contains("toggled #7 back to draft and ready again"));
        let readies: Vec<&String> = ran.iter().filter(|a| a.starts_with("pr ready")).collect();
        assert_eq!(readies, vec!["pr ready 7", "pr ready 7 --undo", "pr ready 7"]);
    }

    #[test]
    fn still_no_run_after_the_toggle_is_exit_1() {
        let mut a = vec![(true, DRAFT.to_string()), (true, runs(&[10])), (true, String::new())];
        a.extend((0..3).map(|_| (true, runs(&[10]))));
        a.extend([(true, String::new()), (true, String::new())]);
        a.extend((0..3).map(|_| (true, runs(&[10]))));
        let (o, _, _) = go(a);
        assert_eq!(o.code, 1);
        assert!(o.err.contains("gh pr checks 7"));
    }

    #[test]
    fn a_ready_or_closed_pr_or_a_bad_number_does_nothing() {
        let (o, ran, _) = go(vec![(true, DRAFT.replace("true", "false"))]);
        assert_eq!((o.code, ran.len()), (0, 1));
        assert!(o.out.contains("already ready"));
        let (o, _, _) = go(vec![(true, DRAFT.replace("OPEN", "MERGED"))]);
        assert_eq!(o.code, 1);
        let (o, _, _) = go(vec![(false, "gh: not logged in".into())]);
        assert_eq!(o.code, 4);
        assert!(o.err.contains("not logged in"));
        let mut f = FakeGh { answers: VecDeque::new(), ran: Vec::new() };
        assert_eq!(ready(Path::new("/r"), "--x", DEFAULT_WAIT, &mut f, &mut |_| {}).code, 2);
        let (o, _, _) = go(vec![(true, DRAFT.replace("feat-x", "-x"))]);
        assert_eq!(o.code, 4);
    }
}
