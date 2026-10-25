//! The build flow (`specs/003` §3, §3.2): a Builder works in a task worktree; the daemon verifies the result
//! deterministically; a Red-Team audits the exact commit; the operator approves the merge. When
//! verification fails, the unified retry ladder tries again, up to three attempts:
//!
//! 1. the tier the scaler picked;
//! 2. **upscaled**: one tier up and at least twice the thinking budget, with the previous failure's
//!    diagnostic in the constraints (the worktree keeps the previous attempt's work);
//! 3. **a different model family** when another is allowed (otherwise a different run of the same family,
//!    recorded as `homogeneous_retry`, `[H-008]`), again with the diagnostic and a fresh context.
//!
//! Then it halts and the operator decides. Anything that is not a failed check (a refused approval, a
//! budget ceiling, a reauthentication, a halted agent) stops the ladder at once: more attempts would not
//! change it.

use crate::engine::{Engine, Status, TaskOutcome};
use crate::finalize::BuildOutcome;
use crate::gates::State;
use crate::handoff::{pointers_from_prompt, ContextPointer, HandoffEnvelope};
use crate::mission::{EngineError, MissionState};
use delphi_adapter::config::AccessPath;
use delphi_core::event::EventEnvelope;
use serde_json::json;
use std::sync::Arc;

/// The scaler's step up (`specs/002` §4).
pub fn upscale(tier: &str) -> &'static str {
    match tier {
        "fast" => "workhorse",
        _ => "frontier",
    }
}

/// A Red-Team reply as findings. `None` when the reply never says `AUDIT COMPLETE`: an audit that did not
/// finish proves nothing, and the caller records it as a blocking finding.
pub fn parse_audit(text: &str) -> Option<Vec<(String, String)>> {
    let mut findings = Vec::new();
    let mut complete = false;
    for line in text.lines() {
        let l = line.trim().trim_start_matches(['-', '*', '•', ' ']).trim();
        // Offsets come from `l` itself: uppercasing can change a string's byte length.
        let head = |word: &str| l.get(..word.len()).is_some_and(|h| h.eq_ignore_ascii_case(word));
        if head("AUDIT COMPLETE") && l[14..].trim_matches(|c: char| !c.is_alphanumeric()).is_empty() {
            complete = true;
        } else if head("FINDING") {
            let rest = l[7..].trim_start_matches([':', '-', ' ', '\t']);
            let mut it = rest.splitn(2, char::is_whitespace);
            let sev = it.next().unwrap_or("").trim_matches(|c: char| !c.is_alphanumeric()).to_uppercase();
            if matches!(sev.as_str(), "P0" | "P1" | "P2" | "P3") {
                findings.push((sev, it.next().unwrap_or("").trim().chars().take(200).collect()));
            } else {
                // A finding we cannot read is still a finding: it blocks rather than vanishing.
                findings.push(("P1".to_string(), format!("unparseable finding: {}", l.chars().take(150).collect::<String>())));
            }
        }
    }
    complete.then_some(findings)
}

/// The commit an audit is about.
#[derive(Clone, Copy)]
struct AuditTarget<'a> {
    sha: &'a str,
    files: &'a [String],
    spec: Option<&'a str>,
}

impl Engine {
    /// Another family with an allowed endpoint that can serve a Builder or auditor (an API endpoint, or the
    /// Claude harness), if any.
    fn other_family(&self, current: &str, builder: bool) -> Option<String> {
        let cfg = self.cfg.get();
        let mut families: Vec<String> = cfg.endpoints.iter().filter(|e| cfg.is_allowed(&e.id) && e.family != current).map(|e| e.family.clone()).collect();
        families.sort();
        families.dedup();
        // Only a family with something usable right now: a Builder on an exhausted family would just wait.
        families.into_iter().find(|f| cfg.endpoints_for(f).iter().any(|e| e.path != AccessPath::Harness || (e.harness.as_deref() == Some("claude")) || !builder) && self.family_usable(f, builder))
    }

    fn span_handoff(&self, m: &MissionState, task_id: &str, n: u32, role: &str, objective: &str, pointers: Vec<ContextPointer>) -> HandoffEnvelope {
        let mut h = HandoffEnvelope::root(&m.id, task_id, role, objective, pointers);
        h.handoff_id = format!("ho_{}_{}_{}", task_id, role, n);
        h
    }

    /// Have a Red-Team audit exactly this commit and record the verdict (what gate 4 reads).
    async fn audit_build(self: &Arc<Self>, m: &Arc<MissionState>, task_id: &str, n: u32, target: &AuditTarget<'_>, builder_family: &str) -> Result<(Vec<(String, String)>, Option<String>), EngineError> {
        let AuditTarget { sha, files, spec } = *target;
        const ATTACH_MAX: usize = 12;
        let mut pointers: Vec<ContextPointer> = files.iter().take(ATTACH_MAX).map(|f| ContextPointer { uri: format!("@worktree/{}", f), heading: String::new(), blake3: String::new(), reason: "changed in this build".into() }).collect();
        pointers.push(ContextPointer { uri: "REVIEW.md".into(), heading: String::new(), blake3: String::new(), reason: "the review rubric".into() });
        if let Some(s) = spec {
            pointers.push(ContextPointer { uri: s.into(), heading: String::new(), blake3: String::new(), reason: "the spec this build implements".into() });
        }
        let omitted = if files.len() > ATTACH_MAX { format!(" {} more changed files are not attached; open them under @worktree: {}.", files.len() - ATTACH_MAX, files[ATTACH_MAX..].iter().take(40).cloned().collect::<Vec<_>>().join(", ")) } else { String::new() };
        let objective = format!(
            "Audit commit {} of this task's worktree against REVIEW.md and the linked spec. The changed files are attached.{} Reply with one line per defect, exactly `FINDING P0 <short title>` (blocking), `FINDING P1 <short title>` (major) or `FINDING P2 <short title>` (minor), then a final line `AUDIT COMPLETE`. If there is nothing to report, reply only `AUDIT COMPLETE`.",
            &sha[..sha.len().min(12)],
            omitted
        );
        let mut h = self.span_handoff(m, task_id, n, "red-team", &objective, pointers);
        // No author-family self-review ([H-008]) when another family is allowed; never a blocker when it is not.
        h.routing_directive.model_family = self.other_family(builder_family, false);
        h.routing_directive.avoid_families = vec![builder_family.to_string()];
        let out = self.run_span(m, task_id, h, None, 0).await?;
        // An audit that did not run says nothing about the code. It is still recorded as a blocking finding, so gate 4
        // stays red (fail closed), but the caller is told it did not run: that is not a defect for the Builder to fix.
        let (verdict, not_run) = match (&out.status, parse_audit(&out.text)) {
            (Status::Done, Some(f)) => (f, None),
            (Status::Done, None) => {
                let why = "the audit did not finish (no `AUDIT COMPLETE` line)".to_string();
                (vec![("P1".to_string(), why.clone())], Some(why))
            }
            (Status::Escalated(r), _) => {
                let why = format!("the audit could not run: {}", r.chars().take(150).collect::<String>());
                (vec![("P1".to_string(), why.clone())], Some(why))
            }
        };
        self.record_audit(m, task_id, sha, "red-team", &verdict).await?;
        Ok((verdict, not_run))
    }

    /// Run a build under the retry ladder and finish with the operator's merge decision.
    pub async fn run_build(self: &Arc<Self>, m: &Arc<MissionState>, task_id: String, prompt: &str) -> Result<TaskOutcome, EngineError> {
        let mut created = EventEnvelope::new(&m.id, "task.created", json!({"prompt": prompt, "target": "builder", "flow": "build"}));
        created.task_id = Some(task_id.clone());
        m.append(created).await?;
        let outcome = self.build_ladder(m, &task_id, prompt).await;
        self.broker.end_task(&task_id);
        let (status, text) = outcome?;
        self.finish_task(m, task_id, crate::engine::SpanOutcome { status, text, turns: 0, ceiling: delphi_core::provenance::Tier::L5 }).await
    }

    async fn build_ladder(self: &Arc<Self>, m: &Arc<MissionState>, task_id: &str, prompt: &str) -> Result<(Status, String), EngineError> {
        let manifest = self.ws.load_manifest("builder")?;
        let base_family = self.cfg.get().family_for_role("builder").to_string();
        let spec = pointers_from_prompt(prompt).into_iter().find(|p| p.uri.starts_with("specs/") && p.uri.ends_with(".md")).map(|p| p.uri);
        let mut diagnostics: Vec<String> = Vec::new();
        let mut n = 0u32;
        for attempt in 1..=self.settings.build_attempts {
            n += 1;
            let mut h = self.span_handoff(m, task_id, n, "builder", prompt, pointers_from_prompt(prompt));
            h.implements_spec = spec.clone();
            h.routing_directive.attempt_number = attempt;
            h.constraints.extend(diagnostics.iter().cloned());
            let mut family = base_family.clone();
            let mut homogeneous = false;
            match attempt {
                1 => {}
                2 => {
                    h.routing_directive.model_tier = Some(upscale(manifest.tier()).to_string());
                    h.routing_directive.thinking_budget_tokens = Some((manifest.thinking_budget() * 2).max(2048));
                }
                _ => {
                    h.routing_directive.model_tier = Some(upscale(manifest.tier()).to_string());
                    h.routing_directive.thinking_budget_tokens = Some((manifest.thinking_budget() * 2).max(2048));
                    h.routing_directive.avoid_families = vec![base_family.clone()];
                    match self.other_family(&base_family, true) {
                        Some(f) => {
                            family = f.clone();
                            h.routing_directive.model_family = Some(f);
                        }
                        None => homogeneous = true,
                    }
                }
            }
            m.append({
                let mut e = EventEnvelope::new(&m.id, "build.attempt", json!({"attempt": attempt, "tier": h.routing_directive.model_tier, "family": family, "thinking_budget": h.routing_directive.thinking_budget_tokens, "homogeneous_retry": homogeneous}));
                e.task_id = Some(task_id.to_string());
                e
            })
            .await?;

            let out = self.run_span(m, task_id, h, None, 0).await?;
            if let Status::Escalated(r) = &out.status {
                // A run that simply ran out of turns is a failed attempt; any other halt is the operator's call.
                if r.contains("whole turn budget") {
                    diagnostics.push(format!("Attempt {} ran out of turns before finishing; do the essential change first.", attempt));
                    continue;
                }
                return Ok((Status::Escalated(r.clone()), format!("[the build halted on attempt {}: {}]", attempt, r)));
            }

            // Deterministic verification of exactly what was produced.
            let ev = self.evaluate_gates(m, task_id).await?;
            let red: Vec<&crate::gates::Gate> = ev.report.gates[..4].iter().filter(|g| g.state == State::Red).collect();
            // The spec gate and a missing test command are the operator's to fix, not something to retry.
            if let Some(g) = red.iter().find(|g| g.n == 3 || (g.n == 1 && g.detail.contains("no `test_command`"))) {
                return Ok((Status::Escalated(format!("gate {} ({}): {}", g.n, g.name, g.detail)), format!("[the build cannot be verified: {}]", g.detail)));
            }
            let audit_only = red.iter().all(|g| g.n == 4);
            if red.iter().any(|g| g.n == 1 || g.n == 2) {
                let why = red.iter().filter(|g| g.n <= 2).map(|g| format!("gate {} ({}): {}", g.n, g.name, g.detail)).collect::<Vec<_>>().join("\n");
                diagnostics.push(format!("Attempt {} failed verification:\n{}", attempt, why.chars().take(1500).collect::<String>()));
                continue;
            }
            debug_assert!(audit_only);
            // Tests and checks pass: audit this commit.
            n += 1;
            let files: Vec<String> = ev.diff.files.iter().map(|f| f.path.clone()).collect();
            let (findings, not_run) = self.audit_build(m, task_id, n, &AuditTarget { sha: &ev.sha, files: &files, spec: spec.as_deref() }, &family).await?;
            if let Some(why) = not_run {
                // The build passed its tests and checks; a retry would only rebuild good work. It is the operator's call.
                return Ok((Status::Escalated(why.clone()), format!("[the build is verified except for its audit: {}; the branch delphi/{} is kept]", why, task_id)));
            }
            let blocking: Vec<&(String, String)> = findings.iter().filter(|(s, _)| s == "P0" || s == "P1").collect();
            if !blocking.is_empty() {
                diagnostics.push(format!("Attempt {} was audited and has open findings:\n{}", attempt, blocking.iter().map(|(s, t)| format!("- {} {}", s, t)).collect::<Vec<_>>().join("\n")));
                continue;
            }
            // Everything the daemon can check is green: the operator decides.
            return Ok(match self.finalize_build(m, task_id).await? {
                BuildOutcome::Merged { report, merge_commit } => (Status::Done, format!("Merged {} into your branch (merge commit {}) after {} attempt(s). Gates: {}.", &report.sha[..report.sha.len().min(8)], &merge_commit[..merge_commit.len().min(8)], attempt, report.gates.iter().map(|g| g.name).collect::<Vec<_>>().join(", "))),
                BuildOutcome::PrOpened { report, url } => (Status::Done, format!("Opened a pull request for {} after {} attempt(s): {}", &report.sha[..report.sha.len().min(8)], attempt, url)),
                BuildOutcome::Declined(report) => (Status::Done, format!("You declined the merge of {}. The branch delphi/{} and its worktree are kept.", &report.sha[..report.sha.len().min(8)], task_id)),
                BuildOutcome::MergeRefused { why, .. } => (Status::Escalated(why.clone()), format!("[approved, but the merge was refused: {}]", why)),
                BuildOutcome::DecisionsRejected { ids, .. } => {
                    let why = format!("you rejected provisional decision(s) {}; nothing was merged and the branch delphi/{} is kept", ids.join(", "), task_id);
                    (Status::Escalated(why.clone()), format!("[{}]", why))
                }
                BuildOutcome::DecisionsPending { ids, .. } => {
                    let why = format!("provisional decision(s) {} were not reviewed in time; nothing was merged and the branch delphi/{} is kept (review them, then /merge {})", ids.join(", "), task_id, task_id);
                    (Status::Escalated(why.clone()), format!("[{}]", why))
                }
                BuildOutcome::Blocked(report) => {
                    let why = report.red().iter().map(|g| format!("gate {}: {}", g.n, g.detail)).collect::<Vec<_>>().join("; ");
                    (Status::Escalated(why.clone()), format!("[blocked at the last check: {}]", why))
                }
            });
        }
        let last = diagnostics.last().cloned().unwrap_or_default();
        Ok((Status::Escalated(format!("the retry ladder is exhausted after {} attempts", self.settings.build_attempts)), format!("[the build failed verification {} times; the branch delphi/{} is kept for you to look at]\n{}", self.settings.build_attempts, task_id, last)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scaler_steps_up_one_tier_and_stops_at_frontier() {
        assert_eq!((upscale("fast"), upscale("workhorse"), upscale("frontier")), ("workhorse", "frontier", "frontier"));
    }

    #[test]
    fn audit_replies_parse_into_findings_and_need_the_completion_line() {
        let ok = "Reviewed.\n- FINDING P1 Unchecked index in parse()\nFINDING p2 naming nit\n* finding P0 data loss on retry\nAUDIT COMPLETE";
        assert_eq!(parse_audit(ok).unwrap(), vec![("P1".to_string(), "Unchecked index in parse()".to_string()), ("P2".to_string(), "naming nit".to_string()), ("P0".to_string(), "data loss on retry".to_string())]);
        assert_eq!(parse_audit("AUDIT COMPLETE").unwrap(), vec![]);
        assert_eq!(parse_audit("FINDING P0 bad\n"), None, "no completion line: not an audit");
        assert_eq!(parse_audit("FINDING Q9 nonsense\nAUDIT COMPLETE").unwrap()[0].0, "P1", "an unreadable finding blocks");
        assert_eq!(parse_audit("FINDING: P0 sql injection\nAUDIT COMPLETE").unwrap(), vec![("P0".to_string(), "sql injection".to_string())]);
        assert_eq!(parse_audit("FINDING P0 straße ŉŉŉŉŉŉŉŉ overflow\nAUDIT COMPLETE").unwrap()[0].0, "P0", "multi-byte uppercasing cannot shift the parse");
        assert_eq!(parse_audit("AUDIT COMPLETELY SKIPPED"), None);
    }

    use crate::approval::Answer;
    use crate::model::Step;
    use crate::testkit::{events_of, setup_with_exec, Fx};
    use std::fs;
    use std::time::Duration;

    const PROMPT: &str = "Implement @specs/011-retry.md in @project/app/src/lib.rs";

    fn fixture(steps: Vec<Step>, test_script: &str) -> Fx {
        let script = test_script.to_string();
        let f = setup_with_exec(steps, 5.0, |_| {}, move |c| {
            c.test_command = Some(vec!["/bin/sh".into(), "-c".into(), script]);
            c.check_command = Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]);
        });
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        f.engine.broker.set_mode(crate::approval::Mode::AcceptEdits);
        f
    }

    fn write(content: &str) -> Step {
        Step::tool("w", "write_file", json!({"path": "@worktree/src/new.rs", "content": content}))
    }

    async fn approve_when_asked(f: &Fx) {
        for _ in 0..600 {
            if let Some(c) = f.engine.broker.pending().into_iter().find(|c| c.tool == "merge") {
                f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "console-1").unwrap();
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the merge card never appeared");
    }

    async fn run(f: &Fx) -> TaskOutcome {
        let (e, m) = (f.engine.clone(), f.m.clone());
        let id = m.next_task_id();
        let task = tokio::spawn(async move { e.run_build(&m, id, PROMPT).await });
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if task.is_finished() {
                    break;
                }
                if f.engine.broker.pending().iter().any(|c| c.tool == "merge") {
                    approve_when_asked(f).await;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the build finishes");
        task.await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn a_failed_check_retries_upscaled_with_the_diagnostic_then_audit_approval_and_merge() {
        let f = fixture(vec![write("bad\n"), Step::text("done"), write("good\n"), Step::text("done"), Step::text("AUDIT COMPLETE")], "grep -q good src/new.rs");
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run(&f).await;
        assert_eq!(out.status, Status::Done, "{}", out.text);
        assert!(out.text.contains("Merged") && out.text.contains("2 attempt"), "{}", out.text);
        assert_eq!(fs::read_to_string(f.root.join("proj/src/new.rs")).unwrap(), "good\n");
        let attempts = events_of(&f, "build.attempt");
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[1].payload["tier"], "frontier", "attempt 2 is upscaled");
        assert!(attempts[1].payload["thinking_budget"].as_u64().unwrap() >= 2048);
        let reqs = f.model.requests();
        assert_eq!((reqs[0].model_id.as_str(), reqs[2].model_id.as_str()), ("sonnet-x", "opus-x"));
        let second = reqs[2].body["messages"][0]["content"][0]["text"].to_string();
        assert!(second.contains("failed verification") && second.contains("gate 1"), "the diagnostic reaches the next attempt: {}", second);
        let audits = events_of(&f, "audit.recorded");
        assert_eq!(audits.len(), 1, "no audit for a commit whose tests failed");
    }

    #[tokio::test]
    async fn an_optimistic_build_runs_the_whole_ladder_then_waits_for_the_operators_review_and_merges_after_it() {
        let f = fixture(vec![write("good\n"), Step::text("done"), Step::text("AUDIT COMPLETE")], "grep -q good src/new.rs");
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        // With edits set to ask, an ordinary build would raise a card for every write; an optimistic one decides them.
        f.engine.broker.set_mode(crate::approval::Mode::Default);
        f.engine.mark_optimistic("t_0001").unwrap();
        let (e, m) = (f.engine.clone(), f.m.clone());
        let id = m.next_task_id();
        assert_eq!(id, "t_0001");
        let task = tokio::spawn(async move { e.run_build(&m, id, PROMPT).await });

        // The ladder reaches the review wait: tests passed, the audit is clean, and the merge card is held back.
        let mut asked = None;
        for _ in 0..1500 {
            asked = events_of(&f, "decisions.review_requested").into_iter().next();
            if asked.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let asked = asked.expect("the build asked for the operator's review");
        assert_eq!(asked.payload["count"], 1);
        assert_eq!(events_of(&f, "audit.recorded").len(), 1);
        assert!(f.engine.broker.pending().is_empty(), "no card of any kind: the edit was decided and the merge card waits");
        assert!(!f.root.join("proj/src/new.rs").exists());
        assert!(!task.is_finished());

        // The operator reviews; only then does the merge card appear.
        let id = asked.payload["decisions"][0]["decision_id"].as_str().unwrap().to_string();
        let mut r = delphi_core::event::EventEnvelope::new(&f.m.id, "decision.reviewed", json!({"decision_id": id, "resolution": "ratified", "note": "", "by": "console-1"}));
        r.task_id = Some("t_0001".into());
        f.m.append(r).await.unwrap();
        f.engine.decisions_changed.notify_one();
        approve_when_asked(&f).await;
        let out = tokio::time::timeout(Duration::from_secs(60), task).await.expect("finishes").unwrap().unwrap();
        assert_eq!(out.status, Status::Done, "{}", out.text);
        assert!(out.text.contains("Merged") && out.text.contains("decisions"), "the gate list names the decisions gate: {}", out.text);
        assert_eq!(fs::read_to_string(f.root.join("proj/src/new.rs")).unwrap(), "good\n");
        let kinds: Vec<String> = f.m.replay().unwrap().into_iter().map(|e| e.kind).collect();
        let at = |k: &str| kinds.iter().position(|x| x == k).unwrap_or_else(|| panic!("missing {}", k));
        assert!(at("decision.provisional") < at("audit.recorded") && at("decisions.review_requested") < at("decision.reviewed") && at("decision.reviewed") < at("merge.approved"), "{:?}", kinds);
    }

    #[tokio::test]
    async fn a_blocking_audit_finding_sends_the_build_around_again_and_p2s_do_not() {
        let f = fixture(
            vec![write("one\n"), Step::text("done"), Step::text("FINDING P1 off-by-one in new()\nFINDING P2 naming\nAUDIT COMPLETE"), write("two\n"), Step::text("done"), Step::text("FINDING P2 naming\nAUDIT COMPLETE")],
            "test -f src/new.rs",
        );
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run(&f).await;
        assert_eq!(out.status, Status::Done, "{}", out.text);
        assert_eq!(events_of(&f, "audit.recorded").len(), 2);
        let second_builder = f.model.requests()[3].body["messages"][0]["content"][0]["text"].to_string();
        assert!(second_builder.contains("off-by-one"), "{}", second_builder);
        assert_eq!(fs::read_to_string(f.root.join("proj/src/new.rs")).unwrap(), "two\n");
    }

    #[tokio::test]
    async fn an_audit_that_did_not_finish_goes_to_the_operator_and_is_not_fed_back_as_a_defect() {
        let f = fixture(vec![write("one\n"), Step::text("done"), Step::text("I ran out of room before finishing.")], "test -f src/new.rs");
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run(&f).await;
        match &out.status {
            Status::Escalated(r) => assert!(r.contains("audit did not finish"), "{}", r),
            other => panic!("{:?}", other),
        }
        assert_eq!(events_of(&f, "build.attempt").len(), 1, "good work is not rebuilt because the auditor failed");
        let audits = events_of(&f, "audit.recorded");
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].payload["findings"][0]["severity"], "P1", "gate 4 stays red");
        assert!(events_of(&f, "merge.approved").is_empty() && !f.root.join("proj/src/new.rs").exists());
    }

    #[tokio::test]
    async fn three_failed_attempts_halt_for_the_operator_and_the_last_is_a_homogeneous_retry() {
        let steps = (0..3).flat_map(|_| vec![write("never good\n"), Step::text("done")]).collect();
        let f = fixture(steps, "false");
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run(&f).await;
        match &out.status {
            Status::Escalated(r) => assert!(r.contains("exhausted after 3 attempts"), "{}", r),
            other => panic!("{:?}", other),
        }
        let attempts = events_of(&f, "build.attempt");
        assert_eq!(attempts.len(), 3);
        assert_eq!((attempts[0].payload["homogeneous_retry"].as_bool(), attempts[2].payload["homogeneous_retry"].as_bool()), (Some(false), Some(true)), "one family only: said so, not hidden");
        assert!(events_of(&f, "audit.recorded").is_empty() && events_of(&f, "approval.requested").is_empty(), "nothing unverified reaches the auditor or the operator");
        assert!(!f.root.join("proj/src/new.rs").exists());
    }

    // specs/002 §4.1 test 7: promoting to another model is not a verification attempt.
    #[tokio::test]
    async fn a_model_running_out_mid_build_is_promoted_without_using_a_retry_attempt() {
        let f = fixture(vec![Step::Exhaust, write("good\n"), Step::text("done"), Step::text("AUDIT COMPLETE")], "grep -q good src/new.rs");
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run(&f).await;
        assert_eq!(out.status, Status::Done, "{}", out.text);
        assert!(out.text.contains("Merged") && out.text.contains("1 attempt"), "it still passed on the first attempt: {}", out.text);
        let attempts = events_of(&f, "build.attempt");
        assert_eq!((attempts.len(), attempts[0].payload["tier"].as_str()), (1, None), "no upscale, no second attempt");
        assert_eq!(events_of(&f, "model.exhausted").len(), 1);
        let reqs = f.model.requests();
        assert_eq!((reqs[0].endpoint_id.as_str(), reqs[1].endpoint_id.as_str()), ("direct", "backup"), "the same attempt moved to the next endpoint");
        assert_eq!(reqs[0].model_id, reqs[1].model_id, "the same tier, so nothing was upscaled");
    }

    #[tokio::test]
    async fn a_second_family_is_used_for_attempt_three_when_one_is_allowed() {
        let steps = (0..3).flat_map(|_| vec![write("x\n"), Step::text("done")]).collect();
        let f = fixture(steps, "false");
        let text = crate::testkit::CONFIG
            .replace("[access]", "[models.gemini]\nfast = \"gem-f\"\nworkhorse = \"gem-w\"\nfrontier = \"gem-p\"\n[models.catalog.\"gem-w\"]\nmax_output_tokens = 64000\nmin_thinking_budget = 128\nmax_thinking_budget = 32000\ninput_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0\n[models.catalog.\"gem-p\"]\nmax_output_tokens = 64000\nmin_thinking_budget = 128\nmax_thinking_budget = 32000\ninput_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0\n[[endpoints]]\nid = \"gem\"\nfamily = \"gemini\"\npath = \"api\"\ncredential = \"env:K\"\n[access]")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"direct\", \"backup\", \"gem\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let out = run(&f).await;
        assert!(matches!(out.status, Status::Escalated(_)));
        let attempts = events_of(&f, "build.attempt");
        assert_eq!((attempts[2].payload["family"].as_str(), attempts[2].payload["homogeneous_retry"].as_bool()), (Some("gemini"), Some(false)));
        let reqs = f.model.requests();
        assert_eq!(reqs[4].endpoint_id, "gem", "attempt 3 really ran on the other family");
        assert!(reqs[4].body.get("contents").is_some());
    }

    #[tokio::test]
    async fn attempt_three_is_a_homogeneous_retry_when_the_other_family_is_out_of_quota() {
        let steps = (0..3).flat_map(|_| vec![write("x\n"), Step::text("done")]).collect();
        let f = fixture(steps, "false");
        let text = crate::testkit::CONFIG
            .replace("[access]", "[models.gemini]\nfast = \"gem-f\"\nworkhorse = \"gem-w\"\nfrontier = \"gem-p\"\n[models.catalog.\"gem-w\"]\nmax_output_tokens = 64000\nmin_thinking_budget = 128\nmax_thinking_budget = 32000\ninput_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0\n[models.catalog.\"gem-p\"]\nmax_output_tokens = 64000\nmin_thinking_budget = 128\nmax_thinking_budget = 32000\ninput_usd_per_mtok = 2.0\noutput_usd_per_mtok = 12.0\n[[endpoints]]\nid = \"gem\"\nfamily = \"gemini\"\npath = \"api\"\ncredential = \"env:K\"\n[access]")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"direct\", \"backup\", \"gem\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        let reset = f.now.load(std::sync::atomic::Ordering::SeqCst) + 3600;
        for k in f.engine.cfg.get().pool_entries().into_iter().filter(|k| k.family == "gemini") {
            f.engine.pool.record_exhausted(&k.endpoint_id, &k.model_id, &delphi_adapter::exhaust::Exhaustion { reason: "quota reached".into(), reset_at: Some(reset), raw: String::new() });
        }
        let out = run(&f).await;
        assert!(matches!(out.status, Status::Escalated(_)));
        let attempts = events_of(&f, "build.attempt");
        assert_eq!((attempts[2].payload["family"].as_str(), attempts[2].payload["homogeneous_retry"].as_bool()), (Some("claude"), Some(true)), "the other family is out, so the retry says it is the same family");
        assert!(f.model.requests().iter().all(|r| r.endpoint_id != "gem"), "nothing was sent to the exhausted family");
    }

    #[tokio::test]
    async fn a_spec_that_is_not_ratified_stops_the_build_without_retries() {
        let f = fixture(vec![write("x\n"), Step::text("done")], "true");
        fs::write(f.root.join("workspaces/w/team/specs/011-retry.md"), "edited after ratification").unwrap();
        let out = run(&f).await;
        match &out.status {
            Status::Escalated(r) => assert!(r.contains("not a ratified spec"), "{}", r),
            other => panic!("{:?}", other),
        }
        assert_eq!(events_of(&f, "build.attempt").len(), 1);
    }

    /// The whole flow against the real Claude CLI (builder, auditor, merge): costs a few cents-to-tens of
    /// cents of subscription quota. Run with `cargo test -p delphi-engine live_build -- --ignored --nocapture`
    /// after `claude` is signed in. Every approval card is answered "allow" and printed.
    #[tokio::test]
    #[ignore]
    async fn live_build_through_the_claude_harness() {
        let f = setup_with_exec(
            vec![],
            5.0,
            |s| s.harness_token_cap = 300_000,
            |c| {
                c.sandbox = crate::exec::SandboxMode::Off;
                c.test_command = Some(vec!["/bin/sh".into(), "-c".into(), "grep -q 'pub fn add' src/lib.rs".into()]);
                c.check_command = Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]);
            },
        );
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        // A spec that says what to build, and the review rubric the auditor reads (a real workspace has both).
        let ws = f.root.join("workspaces/w");
        let spec = "---\nstatus: approved\n---\n# Retry ladder\n\nAcceptance: `src/lib.rs` exports `pub fn add(a: i32, b: i32) -> i32` returning the sum, and keeps the existing `needle` function.\n";
        fs::write(ws.join("team/specs/011-retry.md"), spec).unwrap();
        let mut l = delphi_core::lock::read(&ws.join("local/ratified.lock")).unwrap();
        l.files.insert("specs/011-retry.md".into(), delphi_core::lock::hash(spec.as_bytes()));
        delphi_core::lock::write(&ws.join("local/ratified.lock"), &l).unwrap();
        fs::write(ws.join("team/REVIEW.md"), "# REVIEW\n\nP0: data loss, security holes, broken acceptance criteria. P1: logic errors and missing tests for new behavior. P2: style.\n").unwrap();
        let text = crate::testkit::CONFIG
            .replace("[access]", "[[endpoints]]\nid = \"claude-sub\"\nfamily = \"claude\"\npath = \"harness\"\nharness = \"claude\"\ncredential = \"harness-login\"\ncost_model = \"subscription\"\nmodels = { fast = \"haiku\", workhorse = \"sonnet\", frontier = \"opus\" }\n[access]")
            .replace("order = { claude = [\"direct\", \"backup\"] }", "order = { claude = [\"claude-sub\"] }")
            .replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"claude-sub\"]");
        f.engine.cfg.set(delphi_adapter::config::Config::parse(&text).unwrap());
        f.engine.broker.set_mode(crate::approval::Mode::AcceptEdits);
        let (e, m) = (f.engine.clone(), f.m.clone());
        let id = m.next_task_id();
        let prompt = "Implement @specs/011-retry.md: add `pub fn add(a: i32, b: i32) -> i32 { a + b }` to @project/app/src/lib.rs, keeping the existing function.";
        let task = tokio::spawn(async move { e.run_build(&m, id, prompt).await });
        while !task.is_finished() {
            for c in f.engine.broker.pending() {
                println!("CARD [{}] {} -> allow", c.tool, c.summary);
                f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Allow, "live-test").unwrap();
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let out = task.await.unwrap().unwrap();
        println!("OUTCOME {:?}\n{}", out.status, out.text);
        for e in f.m.replay().unwrap() {
            if e.kind == "agent.block.complete" {
                println!("TEXT {}", e.payload["block"]["text"].to_string().chars().take(400).collect::<String>());
            }
            if ["build.attempt", "harness.started", "harness.tool", "harness.fallback", "harness.cost", "audit.recorded", "gates.evaluated", "merge.approved", "git.tamper_reverted"].contains(&e.kind.as_str()) {
                println!("EVENT {} {}", e.kind, e.payload.to_string().chars().take(300).collect::<String>());
            }
        }
        assert_eq!(out.status, Status::Done, "{}", out.text);
        let merged = fs::read_to_string(f.root.join("proj/src/lib.rs")).unwrap();
        assert!(merged.contains("pub fn add") && merged.contains("needle"), "{}", merged);
    }
}
