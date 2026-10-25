//! Running the operator's deny hooks on the approval path (`specs/018`). The hooks themselves, and how a command is
//! run, are in `hooks.rs`.

use crate::engine::{Asker, Engine};
use crate::hooks;
use crate::mission::EngineError;
use serde_json::{json, Value};

impl Engine {
    /// The first deny hook that objects to this action: `(hook name, reason)`. A hook says no by exiting non-zero,
    /// by not answering in time, or by not starting at all; the refusal is logged. No hook can say yes.
    pub(crate) async fn hook_denial(&self, who: &Asker<'_>, tool: &str, pattern: &str, summary: &str, tier: &str, payload: &Value) -> Result<Option<(String, String)>, EngineError> {
        let input = hooks::deny_input(tool, pattern, summary, tier, who.role, who.task_id, payload);
        for h in self.settings.hooks.deny.iter().filter(|h| hooks::matches_tool(&h.tools, tool)) {
            let (argv, stdin, timeout, cwd, tool_s, tier_s) = (h.command.clone(), input.clone(), h.timeout, self.ws.dir.clone(), tool.to_string(), tier.to_string());
            let run = tokio::task::spawn_blocking(move || hooks::run(&argv, &stdin, timeout, &[("DELPHI_HOOK", "deny"), ("DELPHI_TOOL", &tool_s), ("DELPHI_TIER", &tier_s)], &cwd)).await.map_err(|e| EngineError::Invalid(e.to_string()))?;
            if run.ok() {
                continue;
            }
            let reason = run.reason();
            let mut e = delphi_core::event::EventEnvelope::new(&who.m.id, "hook.denied", json!({"hook": h.name, "tool": tool, "tier": tier, "reason": reason, "exit": run.exit, "timed_out": run.timed_out, "millis": run.millis}));
            e.task_id = Some(who.task_id.to_string());
            e.agent = Some(who.role.to_string());
            who.m.append(e).await?;
            return Ok(Some((h.name.clone(), reason)));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crate::approval::{Answer, Tier};
    use crate::engine::{Asker, Settings, Status};
    use crate::hooks::{DenyHook, Hooks};
    use crate::model::Step;
    use crate::testkit::{events_of, setup, setup_with_exec, Fx};
    use serde_json::json;
    use std::time::Duration;

    fn deny(name: &str, tools: &[&str], script: &str) -> DenyHook {
        DenyHook { name: name.into(), tools: tools.iter().map(|s| s.to_string()).collect(), command: vec!["/bin/sh".into(), "-c".into(), script.into()], timeout: Duration::from_secs(10) }
    }

    fn with_hooks(d: Vec<DenyHook>) -> impl FnOnce(&mut Settings) {
        move |s| s.hooks = Hooks { observe: vec![], deny: d }
    }

    fn spec_write() -> Vec<Step> {
        vec![Step::tool("w1", "write_file", json!({"path": "specs/009-hook.md", "content": "# Hook\nbody"})), Step::text("Understood.")]
    }

    async fn pending_card(f: &Fx) -> Option<crate::approval::Request> {
        for _ in 0..60 {
            if let Some(c) = f.engine.broker.pending().into_iter().next() {
                return Some(c);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        None
    }

    #[tokio::test]
    async fn a_deny_hook_that_objects_stops_the_action_before_any_card_is_raised_and_the_agent_is_told_why() {
        let f = setup(spec_write(), 5.0, with_hooks(vec![deny("no-specs", &["write_file"], "cat >/dev/null; echo 'specs are written by hand here'; exit 7")]));
        let out = f.engine.run_task(&f.m, "architect", "Draft a spec").await.unwrap();
        assert_eq!(out.status, Status::Done);
        assert!(f.engine.broker.pending().is_empty() && events_of(&f, "approval.requested").is_empty(), "no card: the operator was not even asked");
        assert!(!f.root.join("workspaces/w/team/specs/009-hook.md").exists());
        let told = f.model.requests()[1].body["messages"].to_string();
        assert!(told.contains("blocked by the hook `no-specs`") && told.contains("specs are written by hand here"), "{}", told);
        let ev = events_of(&f, "hook.denied");
        assert_eq!((ev.len(), ev[0].payload["hook"].as_str(), ev[0].payload["exit"].as_i64()), (1, Some("no-specs"), Some(7)));
    }

    #[tokio::test]
    async fn a_deny_hook_with_no_objection_changes_nothing_and_it_is_given_the_card_as_json() {
        let t = tempfile::tempdir().unwrap();
        let log = t.path().join("seen.json");
        let f = setup(spec_write(), 5.0, with_hooks(vec![deny("looker", &["write_file"], &format!("cat > {}; exit 0", log.display()))]));
        let (e, m) = (f.engine.clone(), f.m.clone());
        let task = tokio::spawn(async move { e.run_task(&m, "architect", "Draft a spec").await });
        let c = pending_card(&f).await.expect("the hook did not object, so the operator is asked as usual");
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Deny { reason: "no".into() }, "console-1").unwrap();
        task.await.unwrap().unwrap();
        let seen: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&log).unwrap()).unwrap();
        assert_eq!((seen["tool"].as_str(), seen["tier"].as_str(), seen["role"].as_str()), (Some("write_file"), Some("amber"), Some("architect")));
        assert!(seen["summary"].as_str().unwrap().contains("specs/009-hook.md") && seen["payload"]["path"] == "specs/009-hook.md");
        assert!(events_of(&f, "hook.denied").is_empty());
    }

    #[tokio::test]
    async fn a_hook_that_cannot_run_blocks_and_a_hook_for_another_tool_is_never_run() {
        // A missing command and a timeout both mean no.
        let broken = DenyHook { name: "broken".into(), tools: vec!["write_file".into()], command: vec!["/no/such/hook".into()], timeout: Duration::from_secs(5) };
        let f = setup(spec_write(), 5.0, with_hooks(vec![broken]));
        f.engine.run_task(&f.m, "architect", "Draft a spec").await.unwrap();
        assert!(f.model.requests()[1].body["messages"].to_string().contains("could not start"));
        let slow = DenyHook { name: "slow".into(), tools: vec!["*".into()], command: vec!["/bin/sh".into(), "-c".into(), "sleep 30".into()], timeout: Duration::from_millis(200) };
        let f = setup(spec_write(), 5.0, with_hooks(vec![slow]));
        f.engine.run_task(&f.m, "architect", "Draft a spec").await.unwrap();
        assert!(f.model.requests()[1].body["messages"].to_string().contains("did not answer in time"));
        // Another tool's hook is not consulted: the marker file stays absent.
        let t = tempfile::tempdir().unwrap();
        let marker = t.path().join("ran");
        let f = setup(spec_write(), 5.0, with_hooks(vec![deny("merge-only", &["merge"], &format!("touch {}; exit 1", marker.display()))]));
        let (e, m) = (f.engine.clone(), f.m.clone());
        let task = tokio::spawn(async move { e.run_task(&m, "architect", "Draft a spec").await });
        let c = pending_card(&f).await.unwrap();
        f.engine.broker.resolve(&c.approval_id, &c.payload_blake3, Answer::Deny { reason: "no".into() }, "console-1").unwrap();
        task.await.unwrap().unwrap();
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn a_red_card_is_blocked_by_a_deny_hook_too_and_no_hook_can_approve_anything() {
        let f = setup(vec![Step::text("x")], 5.0, with_hooks(vec![deny("freeze", &["merge"], "echo 'merge freeze until Monday'; exit 1")]));
        let who = Asker { m: &f.m, task_id: "t_0001", span_id: "merge", role: "orchestrator" };
        let r = f.engine.ask_as(&who, Tier::Red, "merge", "merge abc", "merge abc", &json!({"tool": "merge", "sha": "abc"})).await.unwrap();
        let denial = r.expect("denied");
        assert!(denial.content.contains("blocked by the hook `freeze`") && denial.content.contains("merge freeze until Monday"), "{}", denial.content);
        assert!(f.engine.broker.pending().is_empty());
        // A hook that exits 0 does not approve: the card is raised and waits for the operator.
        let f = setup(vec![Step::text("x")], 5.0, |s| {
            s.hooks = Hooks { observe: vec![], deny: vec![deny("yes-man", &["merge"], "exit 0")] };
            s.approval_timeout = Duration::from_millis(200);
        });
        let who = Asker { m: &f.m, task_id: "t_0001", span_id: "merge", role: "orchestrator" };
        let r = f.engine.ask_as(&who, Tier::Red, "merge", "merge abc", "merge abc", &json!({"tool": "merge"})).await.unwrap();
        assert!(r.expect("nobody approved it, so the timeout denied it").content.contains("timed out"), "a hook cannot approve");
        assert_eq!(events_of(&f, "approval.requested").len(), 1);
    }

    #[tokio::test]
    async fn an_edit_a_permission_mode_would_have_allowed_without_a_card_still_meets_the_deny_hooks() {
        let f = setup_with_exec(
            vec![Step::tool("c1", "write_file", json!({"path": "@worktree/src/new.rs", "content": "pub fn b() {}\n"})), Step::text("Understood.")],
            5.0,
            with_hooks(vec![deny("no-new-files", &["write_file"], "echo 'no new source files today'; exit 1")]),
            |c| c.test_command = Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]),
        );
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        f.engine.broker.set_mode(crate::approval::Mode::AcceptEdits);
        f.engine.run_task(&f.m, "builder", "Implement @specs/011-retry.md in @project/app/src/lib.rs").await.unwrap();
        let wt = f.engine.exec.known_worktree("t_0001").unwrap();
        assert!(!wt.path.join("src/new.rs").exists(), "accept-edits would have written it; the hook said no");
        assert!(f.model.requests()[1].body["messages"].to_string().contains("no new source files today"));
        assert_eq!(events_of(&f, "hook.denied").len(), 1);
    }

    #[tokio::test]
    async fn a_provisional_decision_is_an_approval_in_effect_so_the_deny_hooks_apply_to_it_too() {
        let f = setup_with_exec(
            vec![Step::tool("c1", "write_file", json!({"path": "@worktree/src/new.rs", "content": "pub fn b() {}\n"})), Step::text("Understood.")],
            5.0,
            |s| {
                s.hooks = Hooks { observe: vec![], deny: vec![deny("no-new-files", &["write_file"], "echo 'blocked'; exit 1")] };
                s.approver = crate::provisional::ApproverKind::Rules;
            },
            |c| c.test_command = Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]),
        );
        if f.engine.exec.sandbox.is_none() {
            return;
        }
        crate::worktree::tests::init_repo(&f.root.join("proj"), &f.root.join("git-home"));
        crate::testkit::ratify_spec(&f);
        f.engine.broker.set_mode(crate::approval::Mode::Default);
        f.engine.mark_optimistic("t_0001").unwrap();
        f.engine.run_task(&f.m, "builder", "Implement @specs/011-retry.md in @project/app/src/lib.rs").await.unwrap();
        assert!(events_of(&f, "decision.provisional").is_empty(), "no decision was recorded: the hook stopped it first");
        assert!(!f.engine.exec.known_worktree("t_0001").unwrap().path.join("src/new.rs").exists());
        assert_eq!(events_of(&f, "hook.denied").len(), 1);
    }
}
