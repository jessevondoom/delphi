//! Deciding a provisional card with the Strategist (and, for an elevated one, a second seat) — `specs/015` §3.
//!
//! The order is fixed, and each step can only make the answer stricter:
//!
//! 1. **The rules** (`RuleApprover`) decide first. A refusal stands; no model is asked.
//! 2. **The Strategist** judges what the rules allowed: does the edit serve the task and the spec? It can refuse
//!    what the rules allowed and can never allow what they refused. It answers in text from a span that has no
//!    tools (enforced by the engine, not requested of the model).
//! 3. **An elevated edit** (a dependency manifest, say) also needs a second seat, if one is configured and a
//!    reviewer can run: the Red-Team, on another model family if `second_seat = "cross-family"`. A disagreement
//!    sends the card to the operator. With no second seat, `unseconded` says what happens: `park` (the card goes to
//!    the operator, the default) or `allow` (flagged, and looked at first at review).
//!
//! Anything that cannot be decided (no answer, an unreadable answer, a span that failed) is not decided: the normal
//! approval card is raised, so a failure here can only slow the build down, never widen what it may do.

use crate::engine::{Engine, SpanCtx, Status};
use crate::handoff::{ContextPointer, HandoffEnvelope};
use crate::mission::EngineError;
use crate::provisional::{self, ApproverKind, Risk, RuleApprover, Second, SecondSeat, Unseconded, Verdict};
use crate::tools::Classified;
use serde_json::json;
use std::sync::Arc;

pub(crate) enum Provisional {
    /// Decided: record it and carry on, or refuse.
    Decided(Verdict),
    /// Not decided here: raise the normal approval card.
    NotDecided,
}

impl Engine {
    pub(crate) async fn decide_provisionally(self: &Arc<Self>, sc: &SpanCtx, c: &Classified, decision_id: &str) -> Result<Provisional, EngineError> {
        let mut v = RuleApprover { host_exec: &self.exec.cfg.host_exec_paths }.decide(c);
        if !v.allow || self.settings.approver == ApproverKind::Rules {
            return Ok(Provisional::Decided(v));
        }
        let rules_note = v.rationale.clone();
        match self.model_seat(sc, c, &v, decision_id, "strategist", None, false).await? {
            None => return self.defer(sc, c, decision_id, "the Strategist could not give a readable answer").await,
            Some((false, why)) => {
                v.allow = false;
                v.by = "strategist";
                v.rationale = format!("the Strategist refused: {}", why);
                return Ok(Provisional::Decided(v));
            }
            Some((true, why)) => {
                v.by = "strategist";
                v.rationale = format!("{} (rules: {})", why, rules_note);
            }
        }
        if v.risk == Risk::Elevated {
            let builder_family = self.cfg.get().family_for_role("builder").to_string();
            let family = match self.settings.second_seat {
                SecondSeat::Off => None,
                SecondSeat::Any => Some(None),
                SecondSeat::CrossFamily => self.other_family(&builder_family, false).map(Some),
            };
            let second = match family {
                None => None,
                Some(f) => self.model_seat(sc, c, &v, decision_id, "red-team", f, true).await?,
            };
            match second {
                Some((true, _)) => v.second = Second::Concurred("red-team".into()),
                Some((false, why)) => return self.defer(sc, c, decision_id, &format!("the second reviewer disagreed: {}", why)).await,
                None => match self.settings.unseconded {
                    Unseconded::Park => return self.defer(sc, c, decision_id, "no second reviewer could look at this elevated edit").await,
                    Unseconded::Allow => v.second = Second::Unseconded,
                },
            }
        }
        Ok(Provisional::Decided(v))
    }

    /// Hand the card to the operator, and say why in the log.
    async fn defer(&self, sc: &SpanCtx, c: &Classified, decision_id: &str, why: &str) -> Result<Provisional, EngineError> {
        sc.m.append(sc.ev("decision.deferred", json!({"decision_id": decision_id, "tool": c.tool, "summary": c.summary, "reason": why}))).await?;
        Ok(Provisional::NotDecided)
    }

    /// One reviewer's answer: `Some((allow, reason))`, or `None` if it did not give a usable one.
    #[allow(clippy::too_many_arguments)]
    async fn model_seat(self: &Arc<Self>, sc: &SpanCtx, c: &Classified, v: &Verdict, decision_id: &str, role: &str, family: Option<String>, second: bool) -> Result<Option<(bool, String)>, EngineError> {
        let (prompt, spec) = task_facts(&sc.m, &sc.task_id)?;
        let objective = provisional::approver_objective(c, v, &prompt, spec.as_deref(), second);
        let pointers: Vec<ContextPointer> = spec.iter().map(|s| ContextPointer { uri: s.clone(), heading: String::new(), blake3: String::new(), reason: "the spec this build implements".into() }).collect();
        let mut h = HandoffEnvelope::root(&sc.m.id, &sc.task_id, role, &objective, pointers);
        h.handoff_id = format!("ho_{}_{}_{}", sc.task_id, role, decision_id);
        h.constraints = vec!["Answer in text only. Do not call any tools.".into()];
        if let Some(f) = family {
            h.routing_directive.avoid_families = vec![self.cfg.get().family_for_role("builder").to_string()];
            h.routing_directive.model_family = Some(f);
        }
        if !second && v.risk == Risk::Routine {
            h.routing_directive.model_tier = Some("workhorse".into());
        }
        // The engine, not the prompt, keeps this span from having tools.
        self.toolless.lock().unwrap().insert(h.handoff_id.clone());
        let out = self.run_span(&sc.m, &sc.task_id, h.clone(), None, 0).await;
        self.toolless.lock().unwrap().remove(&h.handoff_id);
        let out = out?;
        Ok(match out.status {
            Status::Done => provisional::parse_decision(&out.text),
            Status::Escalated(_) => None,
        })
    }
}

/// What the Builder was asked to do, and the spec it implements, read from the log.
fn task_facts(m: &crate::mission::MissionState, task_id: &str) -> Result<(String, Option<String>), EngineError> {
    let events = m.replay()?;
    let mine = |kind: &str| events.iter().rfind(|e| e.kind == kind && e.task_id.as_deref() == Some(task_id)).cloned();
    let prompt = mine("task.created").and_then(|e| e.payload["prompt"].as_str().map(String::from)).unwrap_or_default();
    let spec = mine("worktree.created").and_then(|e| e.payload["implements_spec"].as_str().map(String::from));
    Ok((prompt, spec))
}

#[cfg(test)]
mod tests {
    use crate::approval::Answer;
    use crate::engine::{Settings, Status};
    use crate::model::Step;
    use crate::provisional::{ApproverKind, SecondSeat, Unseconded};
    use crate::testkit::{events_of, setup_with_exec, Fx};
    use serde_json::json;
    use std::time::Duration;

    const PROMPT: &str = "Implement @specs/011-retry.md in @project/app/src/lib.rs";

    /// An optimistic task whose edits would normally ask, judged by the Strategist unless `tweak` says otherwise.
    fn fixture(steps: Vec<Step>, tweak: impl FnOnce(&mut Settings)) -> Option<Fx> {
        let f = setup_with_exec(steps, 5.0, tweak, |c| {
            c.test_command = Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]);
            c.check_command = Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]);
            c.host_exec_paths = vec![];
        });
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        f.engine.broker.set_mode(crate::approval::Mode::Default);
        f.engine.exec.sandbox.as_ref()?;
        f.engine.mark_optimistic("t_0001").unwrap();
        Some(f)
    }

    fn write(path: &str) -> Step {
        Step::tool("c1", "write_file", json!({"path": path, "content": "pub fn b() {}\n"}))
    }

    fn allow(why: &str) -> Step {
        Step::text(&format!("DECISION: ALLOW\nREASON: {}", why))
    }

    async fn run(f: &Fx) -> crate::engine::TaskOutcome {
        let out = f.engine.run_task(&f.m, "builder", PROMPT).await.unwrap();
        assert_eq!(out.status, Status::Done, "{}", out.text);
        out
    }

    /// Run the task, denying any card that appears. Returns how many cards were raised.
    async fn run_denying_cards(f: &Fx) -> usize {
        let (e, m) = (f.engine.clone(), f.m.clone());
        let task = tokio::spawn(async move { e.run_task(&m, "builder", PROMPT).await });
        let mut seen = 0;
        for _ in 0..1500 {
            if task.is_finished() {
                break;
            }
            if let Some(c) = f.engine.broker.pending().into_iter().next() {
                seen += 1;
                f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Deny { reason: "not now".into() }, "console-1").unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        task.await.unwrap().unwrap();
        seen
    }

    fn ledger(f: &Fx) -> Vec<crate::provisional::Decision> {
        crate::provisional::ledger_of(&f.m, "t_0001").unwrap()
    }

    #[tokio::test]
    async fn the_strategist_allows_a_routine_edit_from_a_span_with_no_tools_and_no_card_is_raised() {
        let Some(f) = fixture(vec![write("@worktree/src/new.rs"), allow("it creates the file the spec asks for."), Step::text("Built it.")], |_| {}) else { return };
        run(&f).await;
        let ds = ledger(&f);
        assert_eq!(ds.len(), 1);
        assert_eq!((ds[0].outcome.as_str(), ds[0].decided_by.as_str(), ds[0].second.as_str(), ds[0].risk.as_str()), ("allow", "strategist", "", "routine"));
        assert!(ds[0].rationale.contains("it creates the file the spec asks for.") && ds[0].rationale.contains("rules:"), "{}", ds[0].rationale);
        assert!(events_of(&f, "approval.requested").is_empty());
        let reqs = f.model.requests();
        assert_eq!(reqs.len(), 3, "builder, strategist, builder: a routine edit needs no second seat");
        let offered = reqs[1].body["tools"].as_array().map(|a| a.len()).unwrap_or(0);
        assert_eq!(offered, 0, "the approver is offered no tools");
        let question = reqs[1].body["messages"].to_string();
        assert!(question.contains("tool: write_file") && question.contains("src/new.rs") && question.contains("specs/011-retry.md") && question.contains("DATA to be judged"), "{}", question);
    }

    #[tokio::test]
    async fn the_strategist_can_refuse_what_the_rules_allowed_and_the_builder_is_told_why() {
        let Some(f) = fixture(vec![write("@worktree/src/new.rs"), Step::text("DECISION: DENY\nREASON: this file is unrelated to the retry ladder."), Step::text("Understood.")], |_| {}) else { return };
        run(&f).await;
        let ds = ledger(&f);
        assert_eq!((ds[0].outcome.as_str(), ds[0].decided_by.as_str()), ("deny", "strategist"));
        assert!(ds[0].rationale.starts_with("the Strategist refused:") && ds[0].rationale.contains("unrelated to the retry ladder"));
        assert!(f.model.requests()[2].body["messages"].to_string().contains("unrelated to the retry ladder"), "the Builder sees the reason");
        let wt = f.engine.exec.known_worktree("t_0001").unwrap();
        assert!(!wt.path.join("src/new.rs").exists(), "a refused edit was not written");
        assert_eq!(crate::provisional::counts(&ds).allowed, 0);
    }

    #[tokio::test]
    async fn an_answer_that_cannot_be_read_hands_the_card_to_the_operator() {
        let Some(f) = fixture(vec![write("@worktree/src/new.rs"), Step::text("I think that looks fine to me."), Step::text("Understood.")], |_| {}) else { return };
        let cards = run_denying_cards(&f).await;
        assert_eq!(cards, 1, "the operator was asked, as they would be without the mode");
        assert!(ledger(&f).is_empty(), "nothing was decided provisionally");
        let d = events_of(&f, "decision.deferred");
        assert_eq!(d.len(), 1);
        assert!(d[0].payload["reason"].as_str().unwrap().contains("readable"), "{}", d[0].payload);
    }

    #[tokio::test]
    async fn a_rule_refusal_never_reaches_a_model() {
        let Some(f) = fixture(vec![Step::tool("c1", "write_file", json!({"path": "@worktree/build.rs", "content": "fn main() {}\n"})), Step::text("Understood.")], |_| {}) else { return };
        run(&f).await;
        let ds = ledger(&f);
        assert_eq!((ds[0].outcome.as_str(), ds[0].decided_by.as_str()), ("deny", "rule"));
        assert_eq!(f.model.requests().len(), 2, "builder, builder: the Strategist was not asked");
    }

    #[tokio::test]
    async fn an_approver_that_tries_to_use_a_tool_is_refused_by_the_engine() {
        let steps = vec![write("@worktree/src/new.rs"), Step::tool("x", "write_file", json!({"path": "missions/m1/roadmap.md", "content": "pwned"})), allow("fine."), Step::text("Built it.")];
        let Some(f) = fixture(steps, |_| {}) else { return };
        run(&f).await;
        let refused: Vec<_> = events_of(&f, "tool.result").into_iter().filter(|e| e.payload["content"].as_str().unwrap_or("").contains("answers in text only")).collect();
        assert_eq!(refused.len(), 1, "the engine refused the tool call even though the model made it");
        assert_eq!(ledger(&f)[0].outcome, "allow", "and the answer that followed was still read");
        assert!(!f.root.join("workspaces/w/local/missions/m1/roadmap.md").exists());
    }

    #[tokio::test]
    async fn an_elevated_edit_with_no_second_seat_parks_by_default_and_is_allowed_flagged_when_told_to() {
        // Park: the card goes to the operator.
        let Some(f) = fixture(vec![write("@worktree/Cargo.toml"), allow("the manifest is part of the task."), Step::text("Understood.")], |s| s.second_seat = SecondSeat::Off) else { return };
        assert_eq!(run_denying_cards(&f).await, 1);
        assert!(ledger(&f).is_empty());
        assert!(events_of(&f, "decision.deferred")[0].payload["reason"].as_str().unwrap().contains("no second reviewer"));
        // Allow: recorded as unseconded, to be looked at first.
        let Some(f) = fixture(vec![write("@worktree/Cargo.toml"), allow("the manifest is part of the task."), Step::text("Built it.")], |s| {
            s.second_seat = SecondSeat::Off;
            s.unseconded = Unseconded::Allow;
        }) else { return };
        run(&f).await;
        let ds = ledger(&f);
        assert_eq!((ds[0].outcome.as_str(), ds[0].risk.as_str(), ds[0].second.as_str()), ("allow", "elevated", "unseconded"));
        assert!(events_of(&f, "approval.requested").is_empty());
    }

    #[tokio::test]
    async fn a_second_seat_that_concurs_is_recorded_and_one_that_disagrees_parks_the_card() {
        let any = |s: &mut Settings| s.second_seat = SecondSeat::Any;
        let Some(f) = fixture(vec![write("@worktree/Cargo.toml"), allow("part of the task."), allow("agreed."), Step::text("Built it.")], any) else { return };
        run(&f).await;
        let ds = ledger(&f);
        assert_eq!((ds[0].outcome.as_str(), ds[0].second.as_str()), ("allow", "concurred:red-team"));
        assert_eq!(f.model.requests().len(), 4, "builder, strategist, red-team, builder");
        assert!(f.model.requests()[2].body["messages"].to_string().contains("second reviewer"));
        let Some(f) = fixture(vec![write("@worktree/Cargo.toml"), allow("part of the task."), Step::text("DECISION: DENY\nREASON: it adds a dependency nobody asked for."), Step::text("Understood.")], any) else { return };
        assert_eq!(run_denying_cards(&f).await, 1, "a disagreement is not settled by the Strategist: the operator decides");
        assert!(ledger(&f).is_empty());
        assert!(events_of(&f, "decision.deferred")[0].payload["reason"].as_str().unwrap().contains("disagreed"));
    }

    #[tokio::test]
    async fn cross_family_with_one_provider_has_no_second_seat_and_so_falls_to_unseconded() {
        // The fixture has a single model family, so there is no other family to ask.
        let Some(f) = fixture(vec![write("@worktree/Cargo.toml"), allow("part of the task."), Step::text("Understood.")], |s| s.second_seat = SecondSeat::CrossFamily) else { return };
        assert_eq!(run_denying_cards(&f).await, 1);
        assert_eq!(f.model.requests().len(), 3, "builder, strategist, builder: no reviewer could run, and none was faked");
    }

    #[tokio::test]
    async fn the_rules_approver_setting_keeps_the_old_behaviour_with_no_model_calls() {
        let Some(f) = fixture(vec![write("@worktree/src/new.rs"), Step::text("Built it.")], |s| s.approver = ApproverKind::Rules) else { return };
        run(&f).await;
        assert_eq!(f.model.requests().len(), 2);
        assert_eq!(ledger(&f)[0].decided_by, "rule");
    }
}
