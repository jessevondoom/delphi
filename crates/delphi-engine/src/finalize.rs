//! Finishing a Builder task (`specs/003` §3): commit the worktree, evaluate the five merge gates against
//! that one commit, ask the operator, and merge back.
//!
//! The Builder never merges and never decides any gate. Everything here runs in the daemon, from facts the
//! daemon gathers itself: the sandboxed test run, the provenance index, the ratification lock, and the
//! recorded audit and approval events.

use crate::approval::Tier as ApprovalTier;
use crate::engine::{Asker, Engine};
use crate::exec;
use crate::gates::{self, Audit, Finding, Report, RunSummary, SpecLink, State};
use crate::mission::{EngineError, MissionState};
use crate::worktree::MergeOutcome;
use delphi_core::lock;
use delphi_core::provenance::Zone;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// `Implements-Spec` against the ratification lock: the spec's *current* content must be the ratified one.
pub fn spec_link(ws_dir: &Path, spec: Option<&str>) -> SpecLink {
    let Some(spec) = spec else { return SpecLink::None };
    let ok_shape = spec.starts_with("specs/") && spec.ends_with(".md") && !spec.contains("..") && spec[6..].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    let not = |why: &str| SpecLink::NotRatified { spec: spec.chars().take(80).collect(), why: why.into() };
    if !ok_shape {
        return not("not a `specs/NNN-name.md` path");
    }
    let Ok(text) = std::fs::read(ws_dir.join("team").join(spec)) else { return not("the file does not exist") };
    match lock::read(&ws_dir.join("local/ratified.lock")).ok().and_then(|l| l.files.get(spec).cloned()) {
        None => not("it was never ratified"),
        Some(h) if h == lock::hash(&text) => SpecLink::Ratified(spec.to_string()),
        Some(_) => not("it was edited after it was ratified"),
    }
}

/// Everything known about a commit after gates 1 to 4 were evaluated.
pub(crate) struct Evaluation {
    pub wt: crate::worktree::Worktree,
    pub sha: String,
    pub diff: crate::worktree::DiffSummary,
    pub report: Report,
    pub host_exec: Vec<String>,
}

#[derive(Debug)]
pub enum BuildOutcome {
    /// Gates 1 to 4 are not all green: nothing was shown to the operator.
    Blocked(Report),
    /// The operator said no at gate 5.
    Declined(Report),
    /// Approved, but the merge itself could not happen; the project is untouched.
    MergeRefused { report: Report, why: String },
    Merged { report: Report, merge_commit: String },
}

fn summarize(r: &exec::CommandReport, argv: &[String]) -> RunSummary {
    let (text, is_error) = exec::render(r, argv);
    RunSummary { ok: !is_error, detail: text.chars().rev().take(1200).collect::<String>().chars().rev().collect() }
}

fn events_for(m: &MissionState, task_id: &str, kind: &str) -> Result<Vec<delphi_core::event::EventEnvelope>, EngineError> {
    Ok(m.replay()?.into_iter().filter(|e| e.kind == kind && e.task_id.as_deref() == Some(task_id)).collect())
}

impl Engine {
    /// Run the test and check commands. Unsandboxed, the ones no rule covers are shown to the operator in
    /// one card for the pair, not one card each.
    async fn run_gate_commands(self: &Arc<Self>, m: &Arc<MissionState>, task_id: &str, wt: &crate::worktree::Worktree, test: Option<Vec<String>>, check: Option<Vec<String>>) -> Result<(Option<RunSummary>, Option<RunSummary>), EngineError> {
        let mut refused: [Option<String>; 2] = [None, None];
        let mut ask: Vec<&Vec<String>> = Vec::new();
        for (i, c) in [&test, &check].into_iter().enumerate() {
            if let Some(argv) = c {
                match self.exec.verdict(argv) {
                    exec::Verdict::Refused(why) => refused[i] = Some(format!("refused by policy: {}", why)),
                    exec::Verdict::Allowed(_) => {}
                    exec::Verdict::Ask => ask.push(argv),
                }
            }
        }
        if !ask.is_empty() {
            let lines: Vec<String> = ask.iter().map(|a| a.join(" ")).collect();
            let payload = json!({"tool": "gate_command", "task_id": task_id, "commands": lines, "cwd": "@worktree", "sandboxed": false});
            let who = Asker { m, task_id, span_id: "merge", role: "orchestrator" };
            let declined = self.ask_as(&who, ApprovalTier::Amber, "gate_command", &format!("gate_command {}", ask[0][0]), &format!("run for the merge gates (not sandboxed): {}", lines.iter().map(|l| format!("`{}`", l)).collect::<Vec<_>>().join(" and ")), &payload).await?.is_some();
            if declined {
                for (i, c) in [&test, &check].into_iter().enumerate() {
                    if c.as_ref().is_some_and(|a| ask.contains(&a)) {
                        refused[i] = Some("the operator did not approve running this command".into());
                    }
                }
            }
        }
        let mut out: [Option<RunSummary>; 2] = [None, None];
        for (i, c) in [test, check].into_iter().enumerate() {
            let Some(argv) = c else { continue };
            if let Some(why) = refused[i].take() {
                out[i] = Some(RunSummary { ok: false, detail: why });
                continue;
            }
            let (exec, wt, a2) = (self.exec.clone(), wt.clone(), argv.clone());
            let r = tokio::task::spawn_blocking(move || exec.run(&wt, &a2, None)).await.map_err(|e| EngineError::Invalid(e.to_string()))?;
            out[i] = Some(match r {
                Ok(report) => summarize(&report, &argv),
                Err(e) => RunSummary { ok: false, detail: e.to_string() },
            });
        }
        let [t, c] = out;
        Ok((t, c))
    }

    /// Record a `REVIEW.md` audit of one commit (what gate 4 reads). Called with the Red-Team's verdict; the
    /// SHA is the commit it examined, so a later commit needs a fresh audit.
    pub async fn record_audit(&self, m: &Arc<MissionState>, task_id: &str, sha: &str, role: &str, findings: &[(String, String)]) -> Result<(), EngineError> {
        let list: Vec<Value> = findings.iter().map(|(sev, title)| json!({"severity": sev, "title": title})).collect();
        let mut e = delphi_core::event::EventEnvelope::new(&m.id, "audit.recorded", json!({"sha": sha, "role": role, "findings": list}));
        e.task_id = Some(task_id.to_string());
        e.agent = Some(role.to_string());
        m.append(e).await?;
        Ok(())
    }

    /// Commit the worktree and evaluate gates 1 to 4 against that commit (gate 5 is shown as waiting). Runs
    /// the configured test and check commands, so unsandboxed it may ask the operator first.
    pub(crate) async fn evaluate_gates(self: &Arc<Self>, m: &Arc<MissionState>, task_id: &str) -> Result<Evaluation, EngineError> {
        let exec = self.exec.clone();
        let wt = match exec.known_worktree(task_id) {
            Some(w) => w,
            None => exec.worktrees.adopt(task_id).map_err(|e| EngineError::Invalid(e.to_string()))?,
        };
        let host_exec = exec.cfg.host_exec_paths.clone();
        let spec = events_for(m, task_id, "worktree.created")?.last().and_then(|e| e.payload["implements_spec"].as_str().map(String::from));

        // 1. Commit what the build produced; every gate below is about this one SHA.
        let message = format!("Delphi build {}{}", task_id, spec.as_ref().map(|s| format!("\n\nImplements-Spec: {}", s)).unwrap_or_default());
        let (w, msg) = (wt.clone(), message.clone());
        let sha = tokio::task::spawn_blocking(move || w.commit_all(&msg, "builder").and_then(|c| c.map(Ok).unwrap_or_else(|| w.head()))).await.map_err(|e| EngineError::Invalid(e.to_string()))?.map_err(|e| EngineError::Invalid(e.to_string()))?;
        let (w, he) = (wt.clone(), host_exec.clone());
        let diff = tokio::task::spawn_blocking(move || w.diff(&he)).await.map_err(|e| EngineError::Invalid(e.to_string()))?.map_err(|e| EngineError::Invalid(e.to_string()))?;

        // Gates 1 and 2 run the commands the operator configured. Unsandboxed, each is shown to the operator
        // first unless a rule or the sandbox already covers it, exactly like any other command.
        let (tests, check) = self.run_gate_commands(m, task_id, &wt, exec.cfg.test_command.clone(), exec.cfg.check_command.clone()).await?;

        // Provenance: every changed file must carry a stamp for exactly the content that is committed.
        let mut unstamped = Vec::new();
        for f in diff.files.iter().filter(|f| f.status != 'D') {
            let (w, p, sha2) = (wt.clone(), f.path.clone(), sha.clone());
            let bytes = tokio::task::spawn_blocking(move || w.blob(&sha2, &p)).await.map_err(|e| EngineError::Invalid(e.to_string()))?;
            let stamped = match bytes {
                Ok(b) => m.index.lock().unwrap().lookup(Zone::Worktree, &f.path, &blake3::hash(&b).to_hex()).is_some(),
                Err(_) => false,
            };
            if !stamped {
                unstamped.push(f.path.clone());
            }
        }

        let audits: Vec<Audit> = events_for(m, task_id, "audit.recorded")?
            .iter()
            .map(|e| Audit {
                sha: e.payload["sha"].as_str().unwrap_or("").into(),
                role: e.payload["role"].as_str().unwrap_or("").into(),
                findings: e.payload["findings"].as_array().map(|a| a.iter().map(|f| Finding { severity: f["severity"].as_str().unwrap_or("").into(), title: f["title"].as_str().unwrap_or("").into() }).collect()).unwrap_or_default(),
            })
            .collect();
        let exemption = !events_for(m, task_id, "exemption.approved")?.is_empty();
        let approved: Vec<String> = events_for(m, task_id, "merge.approved")?.iter().filter_map(|e| e.payload["sha"].as_str().map(String::from)).collect();

        let report = Report {
            sha: sha.clone(),
            gates: vec![gates::tests(tests), gates::checks(check, &unstamped, &diff), gates::spec(&spec_link(&self.ws.dir, spec.as_deref()), &diff, exemption), gates::audit(&sha, &audits), gates::approval(&sha, &approved)],
        };
        let mut ev = delphi_core::event::EventEnvelope::new(&m.id, "gates.evaluated", json!({"sha": sha, "gates": report.gates}));
        ev.task_id = Some(task_id.to_string());
        m.append(ev).await?;
        Ok(Evaluation { wt, sha, diff, report, host_exec })
    }

    /// Commit the task's worktree, evaluate the gates, ask the operator, merge.
    pub async fn finalize_build(self: &Arc<Self>, m: &Arc<MissionState>, task_id: &str) -> Result<BuildOutcome, EngineError> {
        let exec = self.exec.clone();
        let Evaluation { wt, sha, diff, mut report, host_exec } = self.evaluate_gates(m, task_id).await?;
        if report.gates[..4].iter().any(|g| g.state == State::Red) {
            return Ok(BuildOutcome::Blocked(report));
        }

        // Gate 5: the operator, shown the host-execution files first.
        let mut hx = Vec::new();
        for f in diff.host_exec().into_iter().take(5) {
            let (w, p) = (wt.clone(), f.path.clone());
            let d = tokio::task::spawn_blocking(move || w.file_diff(&p, 4000)).await.map_err(|e| EngineError::Invalid(e.to_string()))?.unwrap_or_default();
            hx.push(json!({"path": f.path, "diff": d, "reason": f.special.unwrap_or("runs on your machine")}));
        }
        let payload = json!({
            "tool": "merge", "task_id": task_id, "sha": sha, "branch": wt.branch,
            "host_execution": hx, "host_execution_warning": if diff.host_exec().is_empty() { Value::Null } else { json!("these files run on your machine, outside any sandbox, once merged and opened by a shell, IDE or git") },
            "files": diff.files.iter().map(|f| json!({"path": f.path, "status": f.status.to_string(), "added": f.added, "removed": f.removed})).collect::<Vec<_>>(),
            "gates": report.gates,
        });
        let summary = format!("merge {} ({} file(s), +{} -{}){}", &sha[..sha.len().min(8)], diff.files.len(), diff.added(), diff.removed(), if diff.host_exec().is_empty() { "" } else { " — touches host-executed files" });
        let who = Asker { m, task_id, span_id: "merge", role: "orchestrator" };
        if self.ask_as(&who, ApprovalTier::Red, "merge", &format!("merge {}", sha), &summary, &payload).await?.is_some() {
            return Ok(BuildOutcome::Declined(report));
        }
        let mut e = delphi_core::event::EventEnvelope::new(&m.id, "merge.approved", json!({"sha": sha}));
        e.task_id = Some(task_id.to_string());
        m.append(e).await?;
        report.gates[4] = gates::approval(&sha, std::slice::from_ref(&sha));

        // Merge-back is a side effect in the operator's checkout: write-ahead pair around it.
        let mut e = delphi_core::event::EventEnvelope::new(&m.id, "action.intent", json!({"action_id": format!("merge-{}", sha), "tool": "merge", "branch": wt.branch}));
        e.task_id = Some(task_id.to_string());
        m.append(e).await?;
        let (w, s2, he) = (wt.clone(), sha.clone(), host_exec);
        let merge_msg = format!("Merge {} ({})", wt.branch, task_id);
        let outcome = tokio::task::spawn_blocking(move || w.merge_back(&s2, &merge_msg, &he)).await.map_err(|e| EngineError::Invalid(e.to_string()))?.map_err(|e| EngineError::Invalid(e.to_string()))?;
        let (ok, detail, result) = match outcome {
            MergeOutcome::Merged { commit } => (true, json!({"merge_commit": commit}), BuildOutcome::Merged { report, merge_commit: commit }),
            MergeOutcome::Refused(why) => (false, json!({"refused": why}), BuildOutcome::MergeRefused { report, why }),
            MergeOutcome::Conflict(files) => {
                let why = format!("the merge conflicts in {} and was aborted", files.join(", "));
                (false, json!({"conflicts": files}), BuildOutcome::MergeRefused { report, why })
            }
        };
        let mut e = delphi_core::event::EventEnvelope::new(&m.id, "action.result", json!({"action_id": format!("merge-{}", sha), "ok": ok, "detail": detail}));
        e.task_id = Some(task_id.to_string());
        m.append(e).await?;
        if ok {
            // Tidy up: the branch is merged, so the worktree and its scratch space have done their job.
            let w = wt.clone();
            let _ = tokio::task::spawn_blocking(move || w.remove()).await;
            let _ = std::fs::remove_dir_all(exec.scratch_for(task_id));
            exec.forget(task_id);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::Answer;
    use crate::engine::Status;
    use crate::model::Step;
    use crate::testkit::{events_of, Fx};
    use std::fs;
    use std::time::Duration;

    /// Make the testkit project a repository, ratify `specs/011-retry.md`, and configure sandboxed gate commands.
    fn build_fixture(test_cmd: &[&str], steps: Vec<Step>) -> Fx {
        let f = crate::testkit::setup_with_exec(
            steps,
            5.0,
            |_| {},
            |c| {
                c.test_command = Some(test_cmd.iter().map(|s| s.to_string()).collect());
                c.check_command = Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]);
                c.host_exec_paths = vec![];
            },
        );
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        f.engine.broker.set_mode(crate::approval::Mode::AcceptEdits);
        f
    }

    fn builder_steps() -> Vec<Step> {
        vec![Step::tool("c1", "write_file", json!({"path": "@worktree/src/new.rs", "content": "pub fn b() {}\n"})), Step::text("Built it.")]
    }

    async fn run_build(f: &Fx) -> crate::engine::TaskOutcome {
        let out = f.engine.run_task(&f.m, "builder", "Implement @specs/011-retry.md in @project/app/src/lib.rs").await.unwrap();
        assert_eq!(out.status, Status::Done, "{}", out.text);
        out
    }

    async fn card(f: &Fx) -> crate::approval::Request {
        for _ in 0..300 {
            if let Some(c) = f.engine.broker.pending().into_iter().next() {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no approval card appeared");
    }

    #[test]
    fn a_spec_link_is_judged_by_the_lock_not_by_frontmatter() {
        let t = tempfile::tempdir().unwrap();
        let ws = t.path();
        fs::create_dir_all(ws.join("team/specs")).unwrap();
        fs::create_dir_all(ws.join("local")).unwrap();
        fs::write(ws.join("team/specs/011-x.md"), "approved text").unwrap();
        fs::write(ws.join("local/ratified.lock"), json!({"files": {"specs/011-x.md": lock::hash(b"approved text")}}).to_string()).unwrap();
        assert_eq!(spec_link(ws, Some("specs/011-x.md")), SpecLink::Ratified("specs/011-x.md".into()));
        assert_eq!(spec_link(ws, None), SpecLink::None);
        fs::write(ws.join("team/specs/011-x.md"), "approved text, then edited").unwrap();
        assert!(matches!(spec_link(ws, Some("specs/011-x.md")), SpecLink::NotRatified { why, .. } if why.contains("edited")));
        for bad in ["specs/../local/ratified.lock", "../x.md", "/etc/passwd", "specs/missing.md", "specs/a b.md", "specs/011-x.txt"] {
            assert!(matches!(spec_link(ws, Some(bad)), SpecLink::NotRatified { .. }), "{}", bad);
        }
    }

    #[tokio::test]
    async fn a_green_build_is_audited_approved_by_the_operator_and_merged() {
        let f = build_fixture(&["/bin/sh", "-c", "test -f src/new.rs"], builder_steps());
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run_build(&f).await;
        let sha_wt = f.engine.exec.known_worktree(&out.task_id).unwrap();
        // Without an audit, gate 4 blocks and the operator is never asked.
        match f.engine.finalize_build(&f.m, &out.task_id).await.unwrap() {
            BuildOutcome::Blocked(r) => {
                assert_eq!(r.red().iter().map(|g| g.n).collect::<Vec<_>>(), [4, 5]);
                assert_eq!(r.gates[0].state, State::Green, "the sandboxed tests passed");
                assert_eq!(r.gates[2].state, State::Green, "the spec is ratified");
            }
            other => panic!("{:?}", other),
        }
        assert!(f.engine.broker.pending().is_empty());
        let head = sha_wt.head().unwrap();
        // An audit of a different commit does not count; one naming this commit with only a P2 does.
        f.engine.record_audit(&f.m, &out.task_id, "0000000000000000000000000000000000000000", "red-team", &[]).await.unwrap();
        assert!(matches!(f.engine.finalize_build(&f.m, &out.task_id).await.unwrap(), BuildOutcome::Blocked(_)));
        f.engine.record_audit(&f.m, &out.task_id, &head, "red-team", &[("P2".into(), "naming nit".into())]).await.unwrap();
        let e = f.engine.clone();
        let (m, task) = (f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { e.finalize_build(&m, &task).await });
        let c = card(&f).await;
        assert_eq!((c.tool.as_str(), c.tier), ("merge", crate::approval::Tier::Red));
        assert!(c.payload["sha"] == head.as_str() && c.payload["gates"].as_array().unwrap().len() == 5);
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
        match fin.await.unwrap().unwrap() {
            BuildOutcome::Merged { report, merge_commit } => {
                assert!(report.all_green());
                assert_eq!(merge_commit.len(), 40);
            }
            other => panic!("{:?}", other),
        }
        assert_eq!(fs::read_to_string(f.root.join("proj/src/new.rs")).unwrap(), "pub fn b() {}\n", "merged into the operator's checkout");
        assert!(f.engine.exec.known_worktree(&out.task_id).is_none() && !sha_wt.path.exists(), "the worktree is removed after the merge");
        let kinds: Vec<String> = f.m.replay().unwrap().into_iter().map(|e| e.kind).collect();
        for want in ["gates.evaluated", "audit.recorded", "merge.approved", "action.intent", "action.result"] {
            assert!(kinds.contains(&want.to_string()), "missing {}", want);
        }
    }

    #[tokio::test]
    async fn failing_tests_block_the_merge_before_the_operator_is_asked() {
        let f = build_fixture(&["/bin/sh", "-c", "echo 'test x ... FAILED'; exit 101"], builder_steps());
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run_build(&f).await;
        match f.engine.finalize_build(&f.m, &out.task_id).await.unwrap() {
            BuildOutcome::Blocked(r) => {
                let g = &r.gates[0];
                assert!(g.state == State::Red && g.detail.contains("exit 101") && g.detail.contains("FAILED"), "{}", g.detail);
            }
            other => panic!("{:?}", other),
        }
        assert!(f.engine.broker.pending().is_empty());
        assert!(!f.root.join("proj/src/new.rs").exists());
    }

    #[tokio::test]
    async fn a_change_made_outside_the_tools_has_no_provenance_and_blocks_gate_2() {
        let f = build_fixture(&["/bin/sh", "-c", "true"], builder_steps());
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run_build(&f).await;
        // Something edits the worktree behind the daemon's back (not a tool, not a sandboxed command).
        let wt = f.engine.exec.known_worktree(&out.task_id).unwrap();
        fs::write(wt.path.join("src/sneaky.rs"), "pub fn s() {}\n").unwrap();
        match f.engine.finalize_build(&f.m, &out.task_id).await.unwrap() {
            BuildOutcome::Blocked(r) => {
                let g = &r.gates[1];
                assert!(g.state == State::Red && g.detail.contains("src/sneaky.rs"), "{}", g.detail);
            }
            other => panic!("{:?}", other),
        }
    }

    #[tokio::test]
    async fn the_operator_can_decline_and_a_conflicting_checkout_refuses_the_merge() {
        let f = build_fixture(&["/bin/sh", "-c", "true"], builder_steps());
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run_build(&f).await;
        let head = f.engine.exec.known_worktree(&out.task_id).unwrap().commit_all("x", "builder").unwrap();
        let _ = head;
        let wt = f.engine.exec.known_worktree(&out.task_id).unwrap();
        let sha = wt.head().unwrap();
        f.engine.record_audit(&f.m, &out.task_id, &sha, "red-team", &[]).await.unwrap();
        // Decline.
        let (e, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { e.finalize_build(&m, &t).await });
        let c = card(&f).await;
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Deny { reason: "not yet".into() }, "console-1").unwrap();
        assert!(matches!(fin.await.unwrap().unwrap(), BuildOutcome::Declined(_)));
        assert!(!f.root.join("proj/src/new.rs").exists());
        // Approve, but the operator has an untracked file at the same path.
        fs::write(f.root.join("proj/src/new.rs"), "mine\n").unwrap();
        let (e, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { e.finalize_build(&m, &t).await });
        let c = card(&f).await;
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
        match fin.await.unwrap().unwrap() {
            BuildOutcome::MergeRefused { why, .. } => assert!(why.contains("uncommitted changes") && why.contains("src/new.rs"), "{}", why),
            other => panic!("{:?}", other),
        }
        assert_eq!(fs::read_to_string(f.root.join("proj/src/new.rs")).unwrap(), "mine\n");
        assert_eq!(events_of(&f, "action.result").iter().filter(|e| e.payload["ok"] == false).count(), 1);
    }

    #[tokio::test]
    async fn a_build_needs_a_ratified_spec_or_an_approved_exemption_before_a_worktree_exists() {
        // An edited-after-ratification spec stops the build at the door.
        let f = build_fixture(&["/bin/sh", "-c", "true"], builder_steps());
        fs::write(f.root.join("workspaces/w/team/specs/011-retry.md"), "edited after ratification").unwrap();
        let out = f.engine.run_task(&f.m, "builder", "Implement @specs/011-retry.md in @project/app/src/lib.rs").await.unwrap();
        match out.status {
            Status::Escalated(r) => assert!(r.contains("not a ratified spec") && r.contains("edited"), "{}", r),
            other => panic!("{:?}", other),
        }
        assert!(f.model_requests_empty());
        // No spec at all: the operator is asked; a denial ends the task, an approval lets a small change through.
        let g = build_fixture(&["/bin/sh", "-c", "true"], builder_steps());
        let (e, m) = (g.engine.clone(), g.m.clone());
        let task = tokio::spawn(async move { e.run_task(&m, "builder", "Fix the typo in @project/app/src/lib.rs").await });
        let c = card(&g).await;
        assert_eq!(c.tool, "build_without_spec");
        g.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
        assert_eq!(task.await.unwrap().unwrap().status, Status::Done);
        assert_eq!(events_of(&g, "exemption.approved").len(), 1);
    }

    impl Fx {
        fn model_requests_empty(&self) -> bool {
            self.model.requests().is_empty()
        }
    }
}
