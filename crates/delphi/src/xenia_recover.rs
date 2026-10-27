//! `delphi xenia sync --accept-rewritten-head` (spec 011 section 8.4; plan milestone X9a): the recovery from a store whose
//! history was rewritten on purpose. **Terminal only**: nothing in the daemon, the console, the relay or the tool
//! registry can reach it (a test reads the engine source for `plan_rewrite` and `accept_rewritten_head`).
//!
//! The command runs an ordinary sync first. If the host's head is not a fast-forward (or its fast-forward tampered),
//! [`plan_rewrite`](delphi_core::xenia_store::Store) says whether an authentic admin-quorum statement carrying
//! `rewrite_of` names the head this install holds. If it does, every signed file the new head deletes, reverts, swaps,
//! un-verifies or hands to another creator is printed as a fixed-form line, and a person at a terminal must type the
//! commit and the number of listed paths. The approval handed to the library is exactly the plan's commit and paths;
//! the library refuses fewer or extra. Without such a statement it explains and changes nothing. A host that went
//! **back in time** cannot be recovered (its older tree cannot contain a statement about a later head) and the command
//! says so.

use crate::xenia_terminal::{confirm_typed, tame, Terminal};
use delphi_core::xenia_action::escape;
use delphi_core::xenia_status::{self, StoreRef};
use delphi_core::xenia_store::{Divergence, Store};
use delphi_core::xenia_sync::{Recovery, RewriteApproval, RewriteOutcome, VerifiedSync, VerifyConfig};
use serde_json::json;
use std::io::Write;

/// What the operator is told when the recovery does not apply, keyed by the library's reason. One place, so a shared
/// catalog can take them over. Every text says what to do next.
pub fn explain(reason: &str) -> &'static str {
    match reason {
        "no-valid-statement" | "statement-names-no-rewrite" => "rewrite-quorum-missing: no freshness statement signed by a majority of the admins says this history was rewritten on purpose. If the admins did rewrite it, ask them to publish a statement that names the replaced head (`rewrite_of`); if they did not, treat the store as tampered with and tell them. Nothing was changed.",
        "statement-not-current" => "The admins' statement is expired or not current. Ask an admin to refresh it (`delphi xenia admin refresh`), then run this again. Nothing was changed.",
        "rewrite-of-is-not-our-head" => "The admins' statement replaces a different head than the one this install holds, so it does not cover this rewrite. Ask the admins which head they replaced. Nothing was changed.",
        "walk-violations" => "The rewritten history breaks the store's rules in ways the statement does not excuse (see the paths listed). Tell the admins; do not accept it. Nothing was changed.",
        "nothing-to-recover" => "The store has not been rewritten; there is nothing to accept. Run `delphi xenia sync`.",
        "nothing-accepted" => "This install has not accepted any head yet, so there is nothing to replace. Run `delphi xenia sync`.",
        "host-unavailable" => "The store could not be reached. Try again when you are online; nothing was changed.",
        "fetch-refused" | "tip-refused" => "The host's new head was refused by the store's safety checks (see `delphi xenia status`). Tell the admins. Nothing was changed.",
        _ => "The recovery does not apply. `delphi xenia status` says why. Nothing was changed.",
    }
}

fn short(s: &str) -> String {
    s.chars().take(12).collect()
}

pub fn run(r: &StoreRef, store: &Store, cfg: &VerifyConfig, outcome: &VerifiedSync) -> i32 {
    let mut term = crate::xenia_terminal::StdTerminal::new(crate::xenia_terminal::inside_an_agent_seat());
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    recover_with(r, store, cfg, outcome, &mut term, &mut out, &mut err)
}

/// The recovery with its terminal and output injected.
pub fn recover_with(r: &StoreRef, store: &Store, cfg: &VerifyConfig, outcome: &VerifiedSync, term: &mut dyn Terminal, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match outcome {
        VerifiedSync::UpToDate { .. } | VerifiedSync::Accepted { .. } => {
            let _ = writeln!(out, "{}: {}", r.id(), explain("nothing-to-recover"));
            return 0;
        }
        VerifiedSync::Unavailable(_) => {
            let _ = writeln!(err, "{}: {}", r.id(), explain("host-unavailable"));
            return 1;
        }
        VerifiedSync::NonFastForward(n) if n.kind == Divergence::Behind => {
            let _ = writeln!(err, "{}: the host serves an OLDER view than the one this install accepted (its head is an ancestor of ours). That cannot be recovered here: an older tree cannot hold a statement about a later head. Nothing was changed. Ask the host's owner to restore the newer history, or wait.", r.id());
            return 1;
        }
        VerifiedSync::NonFastForward(_) | VerifiedSync::Tampering { .. } => {}
        VerifiedSync::Rejected { .. } | VerifiedSync::Refused(_) => {
            let _ = writeln!(err, "{}: this refusal is not a rewrite and cannot be accepted this way; `delphi xenia status` says why. Nothing was changed.", r.id());
            return 1;
        }
    }
    let plan = match store.plan_rewrite(cfg) {
        Ok(Recovery::Plan(p)) => p,
        Ok(Recovery::Refused { reason, findings }) => {
            let _ = writeln!(err, "{}: {}", r.id(), explain(reason));
            for f in findings.iter().take(20) {
                let _ = writeln!(err, "  {:<24} {}", f.reason, escape(f.path.as_bytes()));
            }
            return 1;
        }
        Err(e) => {
            let _ = writeln!(err, "{}: the recovery could not be planned ({}); nothing was changed", r.id(), tame(&e.to_string()));
            return 1;
        }
    };
    let mut lines = format!(
        "\nThe store's history was rewritten. A majority of the admins ({} people) signed a statement replacing {}.\nstatement expires {}\nyou hold: {}\nnew head: {}\n\nthis would delete, revert or reassign {} signed file(s):\n",
        plan.signing_people, plan.replaced, plan.statement_expires_at, plan.accepted, plan.fetched, plan.changes.len()
    );
    for c in &plan.changes {
        lines.push_str(&format!("  {:<24} {}\n", c.reason, escape(c.path.as_bytes())));
    }
    let expect = format!("accept {} {}", short(&plan.fetched), plan.changes.len());
    let what = format!("accept that commit and exactly those {} path(s) (nothing else is changed)", plan.changes.len());
    if let Err(w) = confirm_typed(term, &lines, &expect, &what) {
        let _ = writeln!(err, "{}: {}", r.id(), w.message());
        let _ = xenia_status::record(store, "recovery", "declined", None, json!({"reason": w.reason(), "count": plan.changes.len()}), cfg.now);
        return 1;
    }
    let approval = RewriteApproval { fetched: plan.fetched.clone(), paths: plan.changes.iter().map(|c| c.path.clone()).collect() };
    match store.accept_rewritten_head(cfg, &approval) {
        Ok(RewriteOutcome::Accepted { head, replaced }) => {
            let _ = xenia_status::record(store, "recovery", "accepted", Some(head.clone()), json!({"count": replaced.len()}), cfg.now);
            let _ = writeln!(out, "{}: accepted the rewritten head {}; {} path(s) started over from it (each logged in state/rewrite-log.jsonl)", r.id(), short(&head), replaced.len());
            0
        }
        Ok(RewriteOutcome::NotApproved { missing }) => {
            let _ = writeln!(err, "{}: the approval left out {} path(s); nothing was applied", r.id(), missing.len());
            1
        }
        Ok(RewriteOutcome::Refused { reason, .. }) => {
            let _ = xenia_status::record(store, "recovery", "refused", None, json!({"reason": reason}), cfg.now);
            let _ = writeln!(err, "{}: refused ({reason}); {}", r.id(), explain(reason));
            1
        }
        Err(e) => {
            let _ = writeln!(err, "{}: the recovery could not run ({}); nothing was changed", r.id(), tame(&e.to_string()));
            1
        }
    }
}

#[cfg(test)]
#[path = "xenia_recover_tests.rs"]
mod tests;
