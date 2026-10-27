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
use crate::exec::MergeMode;
use crate::worktree::{MergeOutcome, PrOutcome};
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
    /// PR mode: the branch was pushed and a pull request opened.
    PrOpened { report: Report, url: String },
    /// Optimistic task: the operator rejected provisional decisions (`specs/015`). Nothing was merged and the
    /// branch is kept; the work built on a rejected decision is suspect.
    DecisionsRejected { report: Report, ids: Vec<String> },
    /// Optimistic task: provisional decisions were still unreviewed when the review window ended. Nothing was
    /// merged and the branch is kept; `build.merge` resumes once they are reviewed.
    DecisionsPending { report: Report, ids: Vec<String> },
}

impl BuildOutcome {
    /// `(outcome, detail)` in a few words, for a merge started after the build (`build.merge`).
    pub fn headline(&self) -> (&'static str, String) {
        match self {
            BuildOutcome::Blocked(r) => ("blocked", r.red().iter().map(|g| format!("gate {}: {}", g.n, g.detail)).collect::<Vec<_>>().join("; ")),
            BuildOutcome::Declined(r) => ("declined", format!("you declined the merge of {}; the branch is kept", &r.sha[..r.sha.len().min(8)])),
            BuildOutcome::MergeRefused { why, .. } => ("refused", why.clone()),
            BuildOutcome::Merged { merge_commit, .. } => ("merged", format!("merge commit {}", &merge_commit[..merge_commit.len().min(8)])),
            BuildOutcome::PrOpened { url, .. } => ("pr_opened", url.clone()),
            BuildOutcome::DecisionsRejected { ids, .. } => ("decisions_rejected", format!("rejected: {}", ids.join(", "))),
            BuildOutcome::DecisionsPending { ids, .. } => ("decisions_pending", format!("unreviewed: {}", ids.join(", "))),
        }
    }
}

/// How the wait for the operator's review of provisional decisions ended.
enum ReviewWait {
    Clear,
    Rejected(Vec<String>),
    Pending(Vec<String>),
}

fn summarize(r: &exec::CommandReport, argv: &[String]) -> RunSummary {
    let (text, is_error) = exec::render(r, argv);
    RunSummary { ok: !is_error, detail: text.chars().rev().take(1200).collect::<String>().chars().rev().collect(), ran: true }
}

fn events_for(m: &MissionState, task_id: &str, kind: &str) -> Result<Vec<delphi_core::event::EventEnvelope>, EngineError> {
    Ok(m.replay()?.into_iter().filter(|e| e.kind == kind && e.task_id.as_deref() == Some(task_id)).collect())
}

/// ` (specs/…)` for a pull request title.
fn spec_title(spec: Option<&str>) -> String {
    spec.map(|s| format!(" ({})", s)).unwrap_or_default()
}

/// The pull request body: what the daemon verified, without any command output.
fn pr_body(report: &Report, diff: &crate::worktree::DiffSummary, branch: &str) -> String {
    let gates = report.gates.iter().map(|g| format!("- {} — {}", g.name, if g.state == State::Green { "passed" } else { "pending" })).collect::<Vec<_>>().join("\n");
    let files = diff.files.iter().take(30).map(|f| format!("- `{}` (+{} -{}){}", f.path, f.added, f.removed, if f.host_exec { " **runs on your machine**" } else { "" })).collect::<Vec<_>>().join("\n");
    format!("Built, verified and audited by Delphi on branch `{}` at `{}`.\n\n**Gates**\n{}\n\n**Files**\n{}\n", branch, report.sha, gates, files)
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
                out[i] = Some(RunSummary { ok: false, detail: why, ran: false });
                continue;
            }
            let (exec, wt, a2) = (self.exec.clone(), wt.clone(), argv.clone());
            let r = tokio::task::spawn_blocking(move || exec.run(&wt, &a2, None)).await.map_err(|e| EngineError::Invalid(e.to_string()))?;
            out[i] = Some(match r {
                Ok(report) => summarize(&report, &argv),
                Err(e) => RunSummary { ok: false, detail: e.to_string(), ran: false },
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
        let host_exec = self.host_exec_paths()?;
        let spec = events_for(m, task_id, "worktree.created")?.last().and_then(|e| e.payload["implements_spec"].as_str().map(String::from));

        // 1. Commit what the build produced; every gate below is about this one SHA.
        let message = format!("Delphi build {}{}", task_id, spec.as_ref().map(|s| format!("\n\nImplements-Spec: {}", s)).unwrap_or_default());
        let (w, msg) = (wt.clone(), message.clone());
        let sha = tokio::task::spawn_blocking(move || w.commit_all(&msg, "builder").and_then(|c| c.map(Ok).unwrap_or_else(|| w.head()))).await.map_err(|e| EngineError::Invalid(e.to_string()))?.map_err(|e| EngineError::Invalid(e.to_string()))?;
        let (w, he) = (wt.clone(), host_exec.clone());
        let diff = tokio::task::spawn_blocking(move || w.diff(&he)).await.map_err(|e| EngineError::Invalid(e.to_string()))?.map_err(|e| EngineError::Invalid(e.to_string()))?;

        // Gates 1 and 2 run the commands the operator configured. Unsandboxed, each is shown to the operator
        // first unless a rule or the sandbox already covers it, exactly like any other command.
        let (mut tests, check) = self.run_gate_commands(m, task_id, &wt, exec.cfg.test_command.clone(), exec.cfg.check_command.clone()).await?;

        // A failing run may be retried once as a possible flake, but only when this build added or changed no
        // test (`specs/003` §3.2): a failure of anything the task wrote always counts. A pass on the rerun is
        // recorded as flaky and does not count as a strike.
        if let (Some(first), Some(argv)) = (tests.clone().filter(|t| t.ran && !t.ok), exec.cfg.test_command.clone()) {
            let w = wt.clone();
            if !tokio::task::spawn_blocking(move || w.touches_tests()).await.map_err(|e| EngineError::Invalid(e.to_string()))?.map_err(|e| EngineError::Invalid(e.to_string()))? {
                let (e2, w2, a2) = (exec.clone(), wt.clone(), argv.clone());
                if let Ok(Ok(rerun)) = tokio::task::spawn_blocking(move || e2.run(&w2, &a2, None)).await {
                    let again = summarize(&rerun, &argv);
                    if again.ok {
                        let mut ev = delphi_core::event::EventEnvelope::new(&m.id, "gate.flaky", json!({"sha": sha, "command": argv.join(" ")}));
                        ev.task_id = Some(task_id.to_string());
                        m.append(ev).await?;
                        tests = Some(RunSummary { ok: true, ran: true, detail: format!("flaky: failed once, then passed on a rerun (this build added or changed no test, so the rerun was allowed). First failure:\n{}", first.detail.chars().take(400).collect::<String>()) });
                    }
                }
            }
        }

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
            gates: vec![
                gates::tests(tests),
                gates::checks(check, &unstamped, &diff),
                gates::spec(&spec_link(&self.ws.dir, spec.as_deref()), &diff, exemption),
                gates::audit(&sha, &audits),
                gates::approval(&sha, &approved),
                // Gate 6 (`specs/015`): green for a task that made no provisional decisions.
                gates::decisions(&crate::provisional::counts(&crate::provisional::ledger_of(m, task_id)?)),
            ],
        };
        let mut ev = delphi_core::event::EventEnvelope::new(&m.id, "gates.evaluated", json!({"sha": sha, "gates": report.gates}));
        ev.task_id = Some(task_id.to_string());
        m.append(ev).await?;
        Ok(Evaluation { wt, sha, diff, report, host_exec })
    }

    /// Wait until every provisional decision of this task that allowed something is reviewed. Announces the wait
    /// once (`decisions.review_requested`, which the console and the relay show), wakes when the operator reviews
    /// one, and gives up after `decision_review_wait`, keeping the branch.
    async fn await_decision_review(&self, m: &Arc<MissionState>, task_id: &str) -> Result<ReviewWait, EngineError> {
        let started = std::time::Instant::now();
        let mut announced = false;
        loop {
            let ds = crate::provisional::ledger_of(m, task_id)?;
            let n = crate::provisional::counts(&ds);
            if !n.rejected.is_empty() {
                return Ok(ReviewWait::Rejected(n.rejected));
            }
            if n.pending.is_empty() {
                return Ok(ReviewWait::Clear);
            }
            if !announced {
                announced = true;
                let list: Vec<Value> = ds.iter().filter(|d| n.pending.contains(&d.id)).map(|d| json!({"decision_id": d.id, "summary": d.summary, "risk": d.risk, "rationale": d.rationale})).collect();
                let mut e = delphi_core::event::EventEnvelope::new(&m.id, crate::provisional::REVIEW_REQUESTED, json!({"task_id": task_id, "count": list.len(), "decisions": list}));
                e.task_id = Some(task_id.to_string());
                m.append(e).await?;
            }
            if started.elapsed() >= self.settings.decision_review_wait {
                return Ok(ReviewWait::Pending(n.pending));
            }
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), self.decisions_changed.notified()).await;
        }
    }

    /// Commit the task's worktree, evaluate the gates, ask the operator, merge.
    pub async fn finalize_build(self: &Arc<Self>, m: &Arc<MissionState>, task_id: &str) -> Result<BuildOutcome, EngineError> {
        let exec = self.exec.clone();
        let Evaluation { wt, sha, diff, mut report, host_exec } = self.evaluate_gates(m, task_id).await?;
        if report.gates[..4].iter().any(|g| g.state == State::Red) {
            return Ok(BuildOutcome::Blocked(report));
        }

        // Gate 6 (`specs/015`): an optimistic task decided some cards itself; the merge card is not shown, and
        // nothing merges, until the operator has reviewed and ratified every one of those decisions.
        match self.await_decision_review(m, task_id).await? {
            ReviewWait::Clear => {}
            ReviewWait::Rejected(ids) => {
                report.gates[5] = gates::decisions(&crate::provisional::counts(&crate::provisional::ledger_of(m, task_id)?));
                return Ok(BuildOutcome::DecisionsRejected { report, ids });
            }
            ReviewWait::Pending(ids) => {
                report.gates[5] = gates::decisions(&crate::provisional::counts(&crate::provisional::ledger_of(m, task_id)?));
                return Ok(BuildOutcome::DecisionsPending { report, ids });
            }
        }
        report.gates[5] = gates::decisions(&crate::provisional::counts(&crate::provisional::ledger_of(m, task_id)?));

        // Gate 5: the operator, shown the host-execution files first. It is never read from history: an earlier
        // approval of this sha that ended in a refusal or an error does not make this decision for them.
        report.gates[4] = gates::approval(&sha, &[]);
        let mut hx = Vec::new();
        for f in diff.host_exec().into_iter().take(5) {
            let (w, p) = (wt.clone(), f.path.clone());
            let d = tokio::task::spawn_blocking(move || w.file_diff(&p, 4000)).await.map_err(|e| EngineError::Invalid(e.to_string()))?.unwrap_or_default();
            hx.push(json!({"path": f.path, "diff": d, "reason": f.special.unwrap_or("runs on your machine")}));
        }
        let payload = json!({
            "tool": "merge", "task_id": task_id, "sha": sha, "branch": wt.branch,
            "decisions": crate::provisional::ledger_of(m, task_id)?.iter().map(|d| json!({"decision_id": d.id, "outcome": d.outcome, "risk": d.risk, "summary": d.summary, "reviewed": d.review.as_ref().map(|r| r.resolution.clone())})).collect::<Vec<_>>(),
            "host_execution": hx, "host_execution_warning": if diff.host_exec().is_empty() { Value::Null } else { json!("these files run on your machine, outside any sandbox, once merged and opened by a shell, IDE or git") },
            "files": diff.files.iter().map(|f| json!({"path": f.path, "status": f.status.to_string(), "added": f.added, "removed": f.removed})).collect::<Vec<_>>(),
            "gates": report.gates,
        });
        let pr_mode = exec.cfg.merge == MergeMode::Pr;
        let verb = if pr_mode { "pull_request" } else { "merge" };
        let mut payload = payload;
        payload["tool"] = json!(verb);
        let summary = format!("{} {} ({} file(s), +{} -{}){}", if pr_mode { "push the branch and open a pull request for" } else { "merge" }, &sha[..sha.len().min(8)], diff.files.len(), diff.added(), diff.removed(), if diff.host_exec().is_empty() { "" } else { " — touches host-executed files" });
        let who = Asker { m, task_id, span_id: "merge", role: "orchestrator" };
        if self.ask_as(&who, ApprovalTier::Red, verb, &format!("{} {}", verb, sha), &summary, &payload).await?.is_some() {
            return Ok(BuildOutcome::Declined(report));
        }
        // The card may have waited a long time. Gates 1 to 4 are evaluated again, against what is in the worktree now,
        // before anything is recorded or merged: a spec edited, an audit superseded or a test broken meanwhile stops it.
        let fresh = self.evaluate_gates(m, task_id).await?;
        let changed = if fresh.sha != sha {
            Some(format!("the worktree moved from {} to {} while the card waited", &sha[..sha.len().min(8)], &fresh.sha[..fresh.sha.len().min(8)]))
        } else {
            fresh.report.gates[..4].iter().chain(fresh.report.gates.get(5)).find(|g| g.state == State::Red).map(|g| format!("gate {} ({}) is no longer green: {}", g.n, g.name, g.detail))
        };
        if let Some(why) = changed {
            return Ok(BuildOutcome::MergeRefused { report: fresh.report, why: format!("{}; nothing was merged", why) });
        }
        report = fresh.report;
        report.gates[4] = gates::approval(&sha, &[]);
        let mut e = delphi_core::event::EventEnvelope::new(&m.id, "merge.approved", json!({"sha": sha, "mode": verb}));
        e.task_id = Some(task_id.to_string());
        m.append(e).await?;
        crate::bundle_cited::emit(m, task_id, "merge").await?;
        report.gates[4] = gates::approval(&sha, std::slice::from_ref(&sha));

        // Merging, or pushing and opening a pull request, is a side effect outside the daemon's own files:
        // a write-ahead pair around it.
        let action = format!("{}-{}", verb, sha);
        let mut e = delphi_core::event::EventEnvelope::new(&m.id, "action.intent", json!({"action_id": action, "tool": verb, "branch": wt.branch}));
        e.task_id = Some(task_id.to_string());
        m.append(e).await?;
        let (ok, detail, result) = if pr_mode {
            let (w, s2, gh) = (wt.clone(), sha.clone(), self.settings.gh_program.clone());
            let spec = events_for(m, task_id, "worktree.created")?.last().and_then(|e| e.payload["implements_spec"].as_str().map(String::from));
            let title = format!("Delphi build {}{}", task_id, spec_title(spec.as_deref()));
            let body = pr_body(&report, &diff, &wt.branch);
            let run = tokio::task::spawn_blocking(move || w.open_pr(&s2, &title, &body, &gh)).await.map_err(|e| e.to_string()).and_then(|r| r.map_err(|e| e.to_string()));
            let outcome = match run {
                Ok(o) => o,
                Err(why) => return Err(record_failure(m, task_id, &action, why).await),
            };
            match outcome {
                PrOutcome::Opened { url } => (true, json!({"url": url}), BuildOutcome::PrOpened { report, url }),
                PrOutcome::Refused(why) => (false, json!({"refused": why}), BuildOutcome::MergeRefused { report, why }),
            }
        } else {
            let (w, s2, he) = (wt.clone(), sha.clone(), host_exec);
            let merge_msg = format!("Merge {} ({})", wt.branch, task_id);
            let run = tokio::task::spawn_blocking(move || w.merge_back(&s2, &merge_msg, &he)).await.map_err(|e| e.to_string()).and_then(|r| r.map_err(|e| e.to_string()));
            let outcome = match run {
                Ok(o) => o,
                Err(why) => return Err(record_failure(m, task_id, &action, why).await),
            };
            match outcome {
                MergeOutcome::Merged { commit } => (true, json!({"merge_commit": commit}), BuildOutcome::Merged { report, merge_commit: commit }),
                MergeOutcome::Refused(why) => (false, json!({"refused": why}), BuildOutcome::MergeRefused { report, why }),
                MergeOutcome::Conflict(files) => {
                    let why = format!("the merge conflicts in {} and was aborted", files.join(", "));
                    (false, json!({"conflicts": files}), BuildOutcome::MergeRefused { report, why })
                }
            }
        };
        let mut e = delphi_core::event::EventEnvelope::new(&m.id, "action.result", json!({"action_id": action, "ok": ok, "detail": detail}));
        e.task_id = Some(task_id.to_string());
        m.append(e).await?;
        // After a local merge the branch is merged, so the worktree and scratch have done their job. After a pull
        // request they are kept: review comments may send the branch back to the Builder.
        if ok && !pr_mode {
            let w = wt.clone();
            let _ = tokio::task::spawn_blocking(move || w.remove()).await;
            let _ = std::fs::remove_dir_all(exec.scratch_for(task_id));
            exec.forget(task_id);
        }
        Ok(result)
    }
}

/// A side effect that failed before reporting an outcome still gets its `action.result`, so the log never
/// shows an intent that looks as if it might still be running.
async fn record_failure(m: &Arc<MissionState>, task_id: &str, action: &str, why: String) -> EngineError {
    let mut e = delphi_core::event::EventEnvelope::new(&m.id, "action.result", json!({"action_id": action, "ok": false, "detail": {"error": why}}));
    e.task_id = Some(task_id.to_string());
    match m.append(e).await {
        Ok(_) => EngineError::Invalid(why),
        Err(err) => err,
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
        assert!(c.payload["sha"] == head.as_str() && c.payload["gates"].as_array().unwrap().len() == 6);
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
    async fn gates_are_checked_again_when_the_card_is_answered_not_only_when_it_was_shown() {
        let f = build_fixture(&["/bin/sh", "-c", "test -f src/new.rs"], builder_steps());
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run_build(&f).await;
        let head = f.engine.exec.known_worktree(&out.task_id).unwrap().commit_all("x", "builder").unwrap();
        let _ = head;
        let sha = f.engine.exec.known_worktree(&out.task_id).unwrap().head().unwrap();
        f.engine.record_audit(&f.m, &out.task_id, &sha, "red-team", &[]).await.unwrap();
        let (e, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { e.finalize_build(&m, &t).await });
        let c = card(&f).await;
        // While the card waits, the ratified spec is edited: gate 3 is no longer green.
        fs::write(f.root.join("workspaces/w/team/specs/011-retry.md"), "---\nstatus: approved\n---\n# Retry ladder, edited\n").unwrap();
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
        match fin.await.unwrap().unwrap() {
            BuildOutcome::MergeRefused { why, .. } => assert!(why.contains("gate 3") && why.contains("nothing was merged"), "{}", why),
            other => panic!("{:?}", other),
        }
        assert!(events_of(&f, "merge.approved").is_empty(), "a stale approval is not recorded");
        assert!(!f.root.join("proj/src/new.rs").exists(), "the project is untouched");
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

    const FLAKY: &str = "if [ -f \"$HOME/seen\" ]; then exit 0; else touch \"$HOME/seen\"; exit 1; fi";

    fn pr_fixture(gh_script: &str, with_origin: bool) -> (Fx, tempfile::TempDir, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let scripts = tempfile::tempdir().unwrap();
        let gh = scripts.path().join("gh");
        fs::write(&gh, format!("#!/bin/sh\n{}\n", gh_script)).unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let gh_path = gh.display().to_string();
        let f = crate::testkit::setup_with_exec(
            builder_steps(),
            5.0,
            move |s| s.gh_program = gh_path,
            |c| {
                c.test_command = Some(vec!["/bin/sh".into(), "-c".into(), "test -f src/new.rs".into()]);
                c.check_command = Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]);
                c.merge = crate::exec::MergeMode::Pr;
            },
        );
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        f.engine.broker.set_mode(crate::approval::Mode::AcceptEdits);
        let bare = f.root.join("origin.git");
        if with_origin {
            for args in [vec!["init".to_string(), "--bare".into(), "-q".into(), bare.display().to_string()], vec!["-C".into(), f.root.join("proj").display().to_string(), "remote".into(), "add".into(), "origin".into(), bare.display().to_string()]] {
                assert!(std::process::Command::new("git").args(&args).status().unwrap().success());
            }
        }
        (f, scripts, bare)
    }

    #[tokio::test]
    async fn in_pr_mode_approval_pushes_the_branch_and_opens_a_pull_request_and_leaves_your_checkout_alone() {
        let (f, scripts, bare) = pr_fixture(r#"echo "$@" > "$(dirname "$0")/args.txt"; echo "https://github.com/example/repo/pull/7""#, true);
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run_build(&f).await;
        let head = f.engine.exec.known_worktree(&out.task_id).unwrap().commit_all("x", "builder").unwrap();
        let _ = head;
        let sha = f.engine.exec.known_worktree(&out.task_id).unwrap().head().unwrap();
        f.engine.record_audit(&f.m, &out.task_id, &sha, "red-team", &[]).await.unwrap();
        let (e, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { e.finalize_build(&m, &t).await });
        let c = card(&f).await;
        assert_eq!(c.tool, "pull_request");
        assert!(c.summary.contains("open a pull request"), "{}", c.summary);
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
        match fin.await.unwrap().unwrap() {
            BuildOutcome::PrOpened { url, .. } => assert_eq!(url, "https://github.com/example/repo/pull/7"),
            other => panic!("{:?}", other),
        }
        let refs = std::process::Command::new("git").args(["--git-dir"]).arg(&bare).args(["for-each-ref", "--format=%(refname)"]).output().unwrap();
        assert!(String::from_utf8_lossy(&refs.stdout).contains(&format!("refs/heads/delphi/{}", out.task_id)), "the branch was pushed");
        let args = fs::read_to_string(scripts.path().join("args.txt")).unwrap();
        assert!(args.contains("pr create") && args.contains(&format!("--head delphi/{}", out.task_id)) && args.contains("--base main"), "{}", args);
        assert!(!f.root.join("proj/src/new.rs").exists(), "a pull request never touches the checkout");
        assert!(f.engine.exec.known_worktree(&out.task_id).is_some(), "the worktree is kept for review rounds");
    }

    #[tokio::test]
    async fn pr_mode_without_a_remote_or_a_url_refuses_after_approval_and_pushes_nothing_it_should_not() {
        let (f, _s, _bare) = pr_fixture("echo nothing useful", false);
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run_build(&f).await;
        let sha = {
            let w = f.engine.exec.known_worktree(&out.task_id).unwrap();
            w.commit_all("x", "builder").unwrap();
            w.head().unwrap()
        };
        f.engine.record_audit(&f.m, &out.task_id, &sha, "red-team", &[]).await.unwrap();
        let (e, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { e.finalize_build(&m, &t).await });
        let c = card(&f).await;
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
        match fin.await.unwrap().unwrap() {
            BuildOutcome::MergeRefused { why, .. } => assert!(why.contains("no `origin` remote"), "{}", why),
            other => panic!("{:?}", other),
        }
    }

    #[tokio::test]
    async fn a_failure_in_untouched_tests_is_rerun_once_and_a_pass_is_recorded_as_flaky() {
        let f = build_fixture(&["/bin/sh", "-c", FLAKY], builder_steps());
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run_build(&f).await;
        let ev = f.engine.evaluate_gates(&f.m, &out.task_id).await.unwrap();
        let g = &ev.report.gates[0];
        assert!(g.state == State::Green && g.detail.starts_with("flaky:"), "{:?}", g);
        assert_eq!(events_of(&f, "gate.flaky").len(), 1);
    }

    #[tokio::test]
    async fn a_failure_is_never_rerun_when_the_build_added_a_test() {
        let steps = vec![Step::tool("w", "write_file", json!({"path": "@worktree/tests/it.rs", "content": "#[test] fn t() {}\n"})), Step::text("Built it.")];
        let f = build_fixture(&["/bin/sh", "-c", FLAKY], steps);
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run_build(&f).await;
        let ev = f.engine.evaluate_gates(&f.m, &out.task_id).await.unwrap();
        assert_eq!(ev.report.gates[0].state, State::Red, "a test the task wrote that fails always counts");
        assert!(events_of(&f, "gate.flaky").is_empty());
        // And a command that keeps failing is red even when no test was touched.
        let g = build_fixture(&["/bin/sh", "-c", "exit 1"], builder_steps());
        let out = run_build(&g).await;
        assert_eq!(g.engine.evaluate_gates(&g.m, &out.task_id).await.unwrap().report.gates[0].state, State::Red);
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

    // ── optimistic tasks and gate 6 (specs/015) ─────────────────────────────

    /// A build fixture where an edit would normally ask (the default permission mode), marked optimistic.
    /// `window` shortens how long a finished build waits for the operator's review.
    fn optimistic_fixture_window(steps: Vec<Step>, window: Option<Duration>) -> Option<Fx> {
        let f = crate::testkit::setup_with_exec(
            steps,
            5.0,
            move |s| {
                s.approver = crate::provisional::ApproverKind::Rules;
                if let Some(w) = window {
                    s.decision_review_wait = w;
                }
            },
            |c| {
                c.test_command = Some(["/bin/sh", "-c", "test -f src/new.rs"].iter().map(|s| s.to_string()).collect());
                c.check_command = Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]);
                c.host_exec_paths = vec![];
            },
        );
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        f.engine.broker.set_mode(crate::approval::Mode::Default);
        if f.engine.exec.sandbox.is_none() {
            assert!(f.engine.mark_optimistic("t_0001").is_err(), "without the sandbox the mode refuses to start");
            return None;
        }
        f.engine.mark_optimistic("t_0001").unwrap();
        Some(f)
    }

    fn optimistic_fixture(steps: Vec<Step>) -> Option<Fx> {
        optimistic_fixture_window(steps, None)
    }

    async fn wait_event(f: &Fx, kind: &str) -> delphi_core::event::EventEnvelope {
        for _ in 0..400 {
            if let Some(e) = events_of(f, kind).into_iter().next() {
                return e;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no `{}` event", kind);
    }

    /// The operator's review, as the service writes it.
    async fn review(f: &Fx, id: &str, resolution: &str) {
        let mut e = delphi_core::event::EventEnvelope::new(&f.m.id, "decision.reviewed", json!({"decision_id": id, "resolution": resolution, "note": "", "by": "console-1"}));
        e.task_id = Some("t_0001".into());
        f.m.append(e).await.unwrap();
        f.engine.decisions_changed.notify_one();
    }

    /// Commit the worktree and record a clean audit of exactly that commit, as the build flow would.
    async fn audited(f: &Fx, task: &str) -> String {
        let wt = f.engine.exec.known_worktree(task).unwrap();
        let _ = wt.commit_all("x", "builder");
        let head = wt.head().unwrap();
        f.engine.record_audit(&f.m, task, &head, "red-team", &[]).await.unwrap();
        head
    }

    #[tokio::test]
    async fn an_optimistic_build_decides_its_edits_without_a_card_and_the_merge_waits_for_every_review() {
        let Some(f) = optimistic_fixture(builder_steps()) else { return };
        let out = run_build(&f).await;
        assert_eq!(out.task_id, "t_0001");
        assert!(events_of(&f, "approval.requested").is_empty(), "the edit was decided provisionally, so no card was raised");
        let ds = crate::provisional::ledger_of(&f.m, "t_0001").unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!((ds[0].outcome.as_str(), ds[0].tool.as_str(), ds[0].risk.as_str()), ("allow", "write_file", "routine"));
        assert_eq!(ds[0].effects[0]["path"], "@worktree/src/new.rs");

        audited(&f, &out.task_id).await;
        let (e, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { e.finalize_build(&m, &t).await });
        // The task announces that it waits for the operator, and no merge card is shown yet.
        let asked = wait_event(&f, "decisions.review_requested").await;
        assert_eq!(asked.payload["count"], 1);
        assert_eq!(asked.payload["decisions"][0]["decision_id"], ds[0].id.as_str());
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(f.engine.broker.pending().is_empty(), "no merge card while a decision is unreviewed");
        assert!(!f.root.join("proj/src/new.rs").exists());

        review(&f, &ds[0].id, "ratified").await;
        let c = card(&f).await;
        assert_eq!((c.tool.as_str(), c.tier), ("merge", crate::approval::Tier::Red));
        assert_eq!(c.payload["gates"].as_array().unwrap().len(), 6);
        assert_eq!((c.payload["gates"][5]["name"].as_str(), c.payload["gates"][5]["state"].as_str()), (Some("decisions"), Some("Green")));
        assert_eq!(c.payload["decisions"][0]["reviewed"], "ratified", "the operator sees the decisions on the merge card too");
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
        match fin.await.unwrap().unwrap() {
            BuildOutcome::Merged { report, .. } => assert!(report.all_green()),
            other => panic!("{:?}", other),
        }
        assert_eq!(fs::read_to_string(f.root.join("proj/src/new.rs")).unwrap(), "pub fn b() {}\n");
    }

    #[tokio::test]
    async fn a_rejected_decision_stops_the_merge_and_keeps_the_branch() {
        let Some(f) = optimistic_fixture(builder_steps()) else { return };
        let out = run_build(&f).await;
        audited(&f, &out.task_id).await;
        let ds = crate::provisional::ledger_of(&f.m, "t_0001").unwrap();
        let (e, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { e.finalize_build(&m, &t).await });
        wait_event(&f, "decisions.review_requested").await;
        review(&f, &ds[0].id, "rejected").await;
        match fin.await.unwrap().unwrap() {
            BuildOutcome::DecisionsRejected { report, ids } => {
                assert_eq!(ids, vec![ds[0].id.clone()]);
                assert!(report.red().iter().any(|g| g.n == 6 && g.detail.contains("rejected")));
            }
            other => panic!("{:?}", other),
        }
        assert!(f.engine.broker.pending().is_empty(), "the merge card was never shown");
        assert!(!f.root.join("proj/src/new.rs").exists(), "nothing reached the operator's checkout");
        assert!(f.engine.exec.known_worktree(&out.task_id).is_some(), "the branch is kept");
        assert!(events_of(&f, "merge.approved").is_empty());
    }

    #[tokio::test]
    async fn a_run_that_stops_while_decisions_wait_keeps_the_branch_and_the_merge_resumes_after_review() {
        let Some(f) = optimistic_fixture(builder_steps()) else { return };
        let out = run_build(&f).await;
        audited(&f, &out.task_id).await;
        let ds = crate::provisional::ledger_of(&f.m, "t_0001").unwrap();
        let (eng, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { eng.finalize_build(&m, &t).await });
        wait_event(&f, "decisions.review_requested").await;
        // The operator is away: stop the waiting task (as a headless run does), leaving the branch and the ledger.
        fin.abort();
        let _ = fin.await;
        assert!(f.engine.exec.known_worktree(&out.task_id).is_some() && !f.root.join("proj/src/new.rs").exists());
        // Later: the operator reviews, and build.merge-equivalent finalizes the same branch.
        review(&f, &ds[0].id, "ratified").await;
        let (eng, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { eng.finalize_build(&m, &t).await });
        let c = card(&f).await;
        assert_eq!(c.tool, "merge");
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
        assert!(matches!(fin.await.unwrap().unwrap(), BuildOutcome::Merged { .. }));
        assert!(f.root.join("proj/src/new.rs").exists());
    }

    #[tokio::test]
    async fn more_work_after_the_merge_card_was_shown_makes_gate_six_red_again_at_answer_time() {
        let Some(f) = optimistic_fixture(builder_steps()) else { return };
        let out = run_build(&f).await;
        audited(&f, &out.task_id).await;
        let ds = crate::provisional::ledger_of(&f.m, "t_0001").unwrap();
        review(&f, &ds[0].id, "ratified").await;
        let (e, m, t) = (f.engine.clone(), f.m.clone(), out.task_id.clone());
        let fin = tokio::spawn(async move { e.finalize_build(&m, &t).await });
        let c = card(&f).await;
        // While the card waits, another provisional decision is made on the task.
        let extra = crate::tools::Classified { tool: "write_file".into(), tier: crate::tools::ToolTier::Amber, kind: crate::tools::ActionKind::WorktreeEdit, summary: "write src/more.rs".into(), payload: json!({"tool": "write_file", "path": "@worktree/src/more.rs", "bytes": 1, "blake3": "00"}), pattern: "write_file @worktree/src/*".into() };
        let v = crate::provisional::Verdict::by_rule(true, "ok".into(), crate::provisional::Risk::Routine);
        let mut ev = delphi_core::event::EventEnvelope::new(&f.m.id, "decision.provisional", crate::provisional::decision_event("dec_late", &extra, crate::approval::Tier::Amber, &v, "builder"));
        ev.task_id = Some("t_0001".into());
        f.m.append(ev).await.unwrap();
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
        match fin.await.unwrap().unwrap() {
            BuildOutcome::MergeRefused { why, .. } => assert!(why.contains("gate 6") && why.contains("nothing was merged"), "{}", why),
            other => panic!("{:?}", other),
        }
        assert!(events_of(&f, "merge.approved").is_empty());
    }

    #[tokio::test]
    async fn an_optimistic_task_still_asks_about_anything_that_is_not_a_worktree_edit() {
        // A spec draft outside the worktree is not eligible, so the operator is asked as always.
        let f = crate::testkit::setup(vec![Step::tool("w1", "write_file", json!({"path": "specs/009-opt.md", "content": "# Opt\nbody"})), Step::text("Done.")], 5.0, |_| {});
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        f.engine.mark_optimistic("t_0001").unwrap();
        let (e, m) = (f.engine.clone(), f.m.clone());
        let task = tokio::spawn(async move { e.run_task(&m, "architect", "Draft a spec").await });
        let c = card(&f).await;
        assert_eq!(c.tool, "write_file");
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Deny { reason: "no".into() }, "console-1").unwrap();
        task.await.unwrap().unwrap();
        assert!(events_of(&f, "decision.provisional").is_empty(), "nothing was decided provisionally");
    }

    #[tokio::test]
    async fn the_approver_refuses_an_edit_that_would_run_on_the_operators_machine_and_the_model_is_told() {
        let steps = vec![Step::tool("c1", "write_file", json!({"path": "@worktree/build.rs", "content": "fn main() {}\n"})), Step::text("Understood.")];
        let Some(f) = optimistic_fixture(steps) else { return };
        let _ = f.engine.run_task(&f.m, "builder", "Implement @specs/011-retry.md in @project/app/src/lib.rs").await.unwrap();
        let ds = crate::provisional::ledger_of(&f.m, "t_0001").unwrap();
        assert_eq!((ds[0].outcome.as_str(), ds[0].risk.as_str()), ("deny", "elevated"));
        assert!(f.model.requests()[1].body["messages"].to_string().contains("operator's machine"), "the Builder is told why");
        assert!(events_of(&f, "approval.requested").is_empty() && !f.engine.exec.known_worktree("t_0001").map(|w| w.path.join("build.rs").exists()).unwrap_or(false), "nothing was written");
        assert_eq!(crate::provisional::counts(&ds).allowed, 0, "a refusal needs no review");
    }

    #[tokio::test]
    async fn when_the_review_window_ends_nothing_is_merged_the_branch_is_kept_and_the_ids_are_named() {
        let Some(f) = optimistic_fixture_window(builder_steps(), Some(Duration::from_millis(400))) else { return };
        let out = run_build(&f).await;
        audited(&f, &out.task_id).await;
        let ds = crate::provisional::ledger_of(&f.m, "t_0001").unwrap();
        match f.engine.finalize_build(&f.m, &out.task_id).await.unwrap() {
            BuildOutcome::DecisionsPending { ids, report } => {
                assert_eq!(ids, vec![ds[0].id.clone()]);
                assert!(report.red().iter().any(|g| g.n == 6 && g.detail.contains("await")));
            }
            other => panic!("{:?}", other),
        }
        assert!(f.engine.broker.pending().is_empty() && !f.root.join("proj/src/new.rs").exists());
        assert!(f.engine.exec.known_worktree(&out.task_id).is_some());
    }
}
