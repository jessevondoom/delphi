//! Operator-authorized tests, slice S2d2b (`specs/092` TA-3): the ONE daemon check that approves a Red card under an operator
//! authorization (`GOVERNANCE.md` §5.12, `context/invariant-enforcement.md`).
//!
//! In a run whose daemon holds a grant (started by `delphi run --on-card authorization:<id> --auth-run <run>` from the operator's
//! terminal and by nothing else), a `run_command` call reaches [`Engine::granted_run_command`] before anything else in `exec_command`.
//! The call is approved only if ALL of these hold; each failure is a denial with the one fixed message the model always gets
//! (`GRANTED_DENIAL`, which tells it nothing about the manifest), and the reason goes to the mission log only:
//!
//! * the span is not tainted (`federated_peer` or lower never gets an approval under this item) and its role may run commands;
//! * the daemon holds its places (the snapshot and the throwaway home the launcher made, and what it minted after the record);
//! * the argument vector EQUALS an entry of the record's manifest, element by element, in its symbolic form;
//! * `Grant::run_authorized` accepts it: everything the dry path checks, the sandbox probed twice, one use taken from the real ledger
//!   before the action, `authorization.used` logged before it runs.
//!
//! The command runs ONLY through `run_authorized` (the forced sandbox: `/bin/bash <pinned script>`, network off, read-only snapshot,
//! no-exec scratch). It never reaches `exec.run`. `delphi run`'s `decide()` is not part of this path and cannot approve a Red card.

use crate::authorization_grant::{GRANTED_DENIAL, NO_MATCH, REFUSED_USE};
use crate::authorization_run::NO_ENTRY;
use crate::engine::{tool_result_error, Engine, SpanCtx};
use crate::mission::EngineError;
use crate::tools::ToolResult;
use delphi_core::event::EventEnvelope;
use delphi_core::provenance::Tier;
use serde_json::{json, Value};
use std::sync::Arc;

/// The argument vector of a call, or an empty one when `argv` is not an array of strings (which matches nothing).
fn argv_of(args: &Value) -> Vec<String> {
    match args["argv"].as_array() {
        Some(a) if a.iter().all(|x| x.is_string()) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        _ => vec![],
    }
}

/// The text the model sees when an authorized command ran: the exit status, then what it printed (already capped by the sandbox runner).
fn render(r: &crate::authorization_run::AuthorizedRun) -> ToolResult {
    let mut t = String::new();
    t.push_str(&match (r.exit_code, r.timed_out) {
        (_, true) => "timed out".to_string(),
        (Some(c), _) => format!("exit status {c}"),
        (None, _) => "ended by a signal".to_string(),
    });
    if r.output_capped || r.over_quota {
        t.push_str(" (output or scratch limit reached)");
    }
    t.push('\n');
    if !r.stdout.is_empty() {
        t.push_str("--- stdout ---\n");
        t.push_str(&r.stdout);
        if !r.stdout.ends_with('\n') {
            t.push('\n');
        }
    }
    if !r.stderr.is_empty() {
        t.push_str("--- stderr ---\n");
        t.push_str(&r.stderr);
        if !r.stderr.ends_with('\n') {
            t.push('\n');
        }
    }
    ToolResult { content: t, is_error: !r.success }
}

/// Why a call is refused BEFORE the grant is even asked, or `None`. Pure so every tier is tested: a span at `federated_peer` taint or
/// lower (L0, L1) never gets an approval under this item (GOVERNANCE §5.12), a role that may not run commands never does, and a daemon
/// that holds no places (no snapshot, no throwaway home) runs nothing anywhere else.
pub(crate) fn pre_refusal(role_may_run: bool, ceiling: Tier, has_places: bool) -> Option<String> {
    if !role_may_run {
        Some("this role's manifest does not authorize running commands".to_string())
    } else if ceiling <= Tier::L1 {
        Some(format!("the span is tainted at {ceiling} (federated_peer or lower): a card from it is never approved under an authorization"))
    } else if !has_places {
        Some("this daemon holds no snapshot or throwaway home for the authorized command".to_string())
    } else {
        None
    }
}

impl Engine {
    /// Spec 092 S2d2b. See the module comment. `Ok(None)` when this daemon holds no grant (the caller then goes on as ever, so an
    /// interactive session, a run without the flag and a run whose grant did not take hold never reach the approval).
    pub(crate) async fn granted_run_command(self: &Arc<Self>, sc: &SpanCtx, args: &Value) -> Result<Option<ToolResult>, EngineError> {
        let Some(grant) = self.grant.get().cloned() else { return Ok(None) };
        let argv = argv_of(args);
        let deny = |reason: String| -> (Value, ToolResult) { (json!({"authorization_id": grant.id(), "tool": "run_command", "reason": reason, "approved": false}), tool_result_error(GRANTED_DENIAL)) };
        // The role must be one that may run commands at all, and the span must not have read anything from outside that lowered its ceiling
        // to `federated_peer` or below (spec 092 TA-0, GOVERNANCE §5.12: such a card is never approved under this item).
        let refusal = pre_refusal(sc.manifest.execute.iter().any(|e| e == "shell-worktree-scoped"), sc.span.ceiling(), self.places.get().is_some());
        if let Some(reason) = refusal {
            let (body, denied) = deny(reason);
            sc.m.append(sc.ev(REFUSED_USE, body)).await?;
            return Ok(Some(denied));
        }
        let (places, now) = (self.places.get().cloned().expect("checked above"), (self.clock_unix)());
        let (m, task_id, span_id, role) = (sc.m.clone(), sc.task_id.clone(), sc.span_id.clone(), sc.role.clone());
        let handle = tokio::runtime::Handle::current();
        let (g, cmd) = (grant.clone(), argv.clone());
        let outcome = tokio::task::spawn_blocking(move || {
            // Events are written from the blocking thread: `authorization.used` must be on disk BEFORE the command starts, and a failure
            // to write it ends the call (the use stays consumed).
            let mut emit = |kind: &str, payload: Value| -> Result<(), String> {
                let mut e = EventEnvelope::new(&m.id, kind, payload);
                e.task_id = Some(task_id.clone());
                e.span_id = Some(span_id.clone());
                e.agent = Some(role.clone());
                handle.block_on(m.append(e)).map(|_| ()).map_err(|e| e.to_string())
            };
            places.run(&g, &cmd, now, &mut emit)
        })
        .await
        .map_err(|e| EngineError::Invalid(e.to_string()))?;
        match outcome {
            Ok(run) => Ok(Some(render(&run))),
            Err(r) if r.reason == NO_ENTRY => {
                sc.m.append(sc.ev(NO_MATCH, json!({"authorization_id": grant.id(), "tool": "run_command", "reason": r.reason, "approved": false}))).await?;
                Ok(Some(tool_result_error(GRANTED_DENIAL)))
            }
            Err(r) => {
                sc.m.append(sc.ev(REFUSED_USE, json!({"authorization_id": grant.id(), "tool": "run_command", "reason": r.reason, "use_taken": r.use_taken, "approved": false}))).await?;
                Ok(Some(tool_result_error(GRANTED_DENIAL)))
            }
        }
    }
}

#[cfg(test)]
mod tests;
