---
id: spec-015-provisional-decisions
title: "015: Provisional Decisions (the optimistic workflow)"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: approved   # by the operator, 2026-10-25, with the charter amendments in §8 (applied the same day)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, optimistic, provisional-decisions, approvals, gates, governance]
---

# Spec 015: Provisional Decisions

## 1. Scope & Objectives
When the operator asks for it, a build may carry on past approval cards whose every effect stays inside the task's branch. An approver decides each such card, every decision is written to the log, and **the merge is blocked until the operator has reviewed and ratified every decision**.

**A provisional decision is not an approval.** It lets work proceed on a branch that can be thrown away. It has no effect on the operator's checkout, an outside service or the host. The operator's later review is the approval, and the merge itself stays the operator's alone.

Origin and reasoning: [`optimistic-workflow-exploration-2026-10-25.md`](../missions/2026-10-23-foundational-design/optimistic-workflow-exploration-2026-10-25.md). The operator accepted the framing on 2026-10-25.

### 1.1 Non-Goals
- Deciding anything outbound, anything on the host, any Red card, or the merge. These wait for a person as they always have (§2).
- Making the mode a default. It is chosen per build (`/build --optimistic`).

### 1.2 Simpler Alternative Considered
**Park every card** (spec 014 §6): safe, and a build stalls at its first edit when nobody is there. **A wider `/permissions accept-edits`:** it already skips the card for worktree edits, but records nothing and blocks nothing: the operator learns what happened only by reading the diff. Optimistic mode is for the case where the operator wants the work done and a complete, itemised account to approve afterwards. Where `accept-edits` is all they want, they should keep using it.

## 2. What may be decided: an allow-list
`provisional::eligible` is the whole rule, and defaults to no. A card is eligible only if it is **Amber** and its classified kind is **`WorktreeEdit`**: a file edit inside the task's worktree that the operator would otherwise be asked about.

| Not eligible (waits for a person) | Why |
| :-- | :-- |
| Unsandboxed commands on the host | Not contained by the branch. |
| Writes outside the worktree, specs, mission artifacts | Not the branch; ratifying a spec is the operator's act. |
| Pinned dependency installs | Run code. (Could be allowed inside the sandbox later.) |
| Outbound actions, connector writes, dispatching agents | Not reversible. |
| Merge, pull request, constitution edits, anything Red | Always the operator's. |
| Edits made by a CLI harness (`harness_edit`) | The approver cannot yet judge their absolute paths; they keep asking. |

A new action kind is therefore ineligible until someone adds it to the list.

## 3. The approver
Three steps in a fixed order, each of which can only make the answer stricter. A failure at any step can only slow the build (the normal card is raised); it can never widen what the build may do.

1. **The rules** (`RuleApprover`) decide first and use no model. They allow an ordinary edit under `@worktree/` and **refuse**: a path that is not plain and inside the worktree (`..`, absolute, empty); protected governance files (`GOVERNANCE.md`, `REVIEW.md`, `agents/`, `skills/`, `.github/`, memory); files that run on the operator's machine once merged (`build.rs`, `Makefile`, `.cargo/config`, `.envrc`, `.gitattributes`, anything in `[exec] host_exec_paths`). They **allow but flag as `elevated`** dependency manifests and lockfiles. Everything else is `routine`. A refusal is final: no model is asked.
2. **The Strategist** (`[optimistic] approver = "strategist"`, the default) judges what the rules allowed: *does this edit serve the task and the spec, and stay inside their scope?* It is given the daemon's classified facts (tool, file, size, hash, the rules' verdict), the Builder's task and the linked spec, and the proposed content fenced as **data, not instructions**; it must answer in two lines, `DECISION: ALLOW|DENY` and `REASON: …`. It can refuse what the rules allowed and can never allow what they refused, so an injection in the content can at most make it agree with the rules. It runs as a span **with no tools**: they are not offered, and a tool call it makes anyway is refused by the engine. Routine questions use the `workhorse` tier. `approver = "rules"` skips the Strategist and costs nothing.
3. **A second seat for an elevated edit** (`[optimistic] second_seat`): `cross-family` (the default; the Red-Team on a model family other than the Builder's), `any` (the Red-Team in a fresh context on any family; weaker), or `off`. It is **never required**: with one provider there is no other family, and then no reviewer is faked. With no second reviewer, `[optimistic] unseconded` decides: `park` (the default; the normal card goes to the operator) or `allow` (allowed provisionally, recorded `unseconded`, and listed first at review). If a second reviewer disagrees, the card **always** goes to the operator; the Strategist does not break ties. A second seat applies only when the approver is the Strategist.

An answer that is missing, unreadable, contradicts itself, or comes from a span that failed is **not an answer**: the log records `decision.deferred` with the reason and the operator is asked as they would be without the mode. A refusal (rules or Strategist) is returned to the Builder with its reason ("do not retry it; propose an alternative") and is recorded; it needs no review.

## 4. The ledger
The daemon writes it from the card it classified, never from anything an agent says, so an agent cannot leave an entry out.

- **`decision.provisional`**, on the task: `decision_id`, `tool`, `tier`, `pattern`, `summary`, `payload_blake3`, `effects` (`worktree_edit` with path, size and content hash), `decided_by` (`rule` or `strategist`, and the asking role), `second` (empty, `concurred:red-team` or `unseconded`), `outcome` (`allow` or `deny`), `rationale`, `risk`.
- **`decision.reviewed`**, written by `decisions.review`: `decision_id`, `resolution` (`ratified` or `rejected`), `note`, `by` (`console-<id>`), `payload_blake3`. The latest review of a decision wins until the merge.
- **`decision.deferred`**: the approver could not or should not decide (no usable answer, or the second reviewer disagreed, or no second reviewer could look at an elevated edit): `decision_id`, `tool`, `summary`, `reason`. The operator is then asked normally.
- **`decisions.review_requested`**, once when a finished build is waiting: the task and the pending decisions with their summaries, risk and rationale.
- **`build.merge_finished`**, when a merge started with `build.merge` ends.

The relay is sent `decisions.review_requested` (and no other new event) and mails the operator a notice with the summaries; reviewing itself happens at the console.

## 5. Gate 6 and the flow
`gates::decisions` is the sixth gate. For a task with no provisional decisions it is green ("no provisional decisions were made"), so ordinary builds are unchanged. It is red while any allowed decision is unreviewed ("N await the operator's review") and red if any was rejected.

1. The Builder works; eligible edits are decided and logged, and no card is raised for them. Tests, checks, the spec gate and the audit run as usual.
2. `finalize_build` reaches the review step with gates 1 to 4 green. **The merge card is not shown yet.** It raises `decisions.review_requested` and waits (default 24 h, `[optimistic] review_wait_sec`) for the operator, waking as soon as a review arrives.
3. All ratified → the merge card appears (its payload lists the decisions and the sixth gate is green), as before. The operator answers it as before.
4. Any rejected → the merge card is never shown. If the build has attempts left, **the rejected edits are rolled back and the Builder tries again**: each rejected file goes back to the base commit (or is removed if the base never had it), every allowed decision on that file is marked `decision.reverted` (it enabled nothing that remains, so it no longer counts toward gate 6), the revert is committed, and the next attempt is told which edits the operator rejected and what they said. Decisions the operator has not yet reviewed stay pending. With no attempts left, the build stops with `DecisionsRejected`: nothing is merged, the branch is kept, and the outcome names the rejected ids.
5. The window ends with decisions still unreviewed → `DecisionsPending`: nothing merged, branch kept.
6. **Re-checked at answer time.** After the merge card is answered, gates 1 to 4 and gate 6 are evaluated again; a decision made or changed in the meantime refuses the merge ("gate 6 is no longer green… nothing was merged").
7. A branch that was kept can be resumed: `/merge <task>` (`build.merge`) adopts the worktree again, re-evaluates every gate against the current ledger, and raises the merge card if they are green.

A headless run (`delphi run "/build --optimistic …"`) cannot review, so it stops when the wait begins: it cancels the waiting task, keeps the branch and ledger, ends with outcome `awaiting_decision_review` and exit code 5, and says to run `/decisions` and then `/merge <task>`.

## 6. Operator surfaces
- Console: `/build --optimistic <request>`, `/decisions` (list), `/decisions show <id>`, `/decisions ratify <id…|all-routine>`, `/decisions reject <id> [why]`, `/merge <task>`. Decisions and the review request are shown as they happen.
- Shell: `delphi decisions list|show|ratify|reject`.
- RPC (console scope only, refused by name on the narrator and relay sockets): `decisions.list`, `decisions.review`, `build.merge`; `build.start` takes `optimistic`.
- A review names the payload hash it was shown, and a mismatch is refused ("changed after you reviewed it"). Only a decision that allowed something is reviewed.
- **Bulk ratify is for routine decisions only** (`all-routine`); elevated ones are reviewed by id.

## 6.1 Settings
```toml
[optimistic]
approver = "strategist"      # or "rules"
second_seat = "cross-family" # or "any", "off"
unseconded = "park"          # or "allow"
review_wait_sec = 86400      # how long a finished build waits for your review
max_decisions = 200          # provisional decisions one task may make; past it, cards go to you
max_approver_calls = 100     # model calls (Strategist, second seat) one task may spend on decisions; past it, cards go to you
```
These are ordinary (not pinned) settings: none of them can let anything reach your checkout without your review.

## 7. Guard rails
- **The OS sandbox is required.** `build.start --optimistic` is refused without it: everything an optimistic task does happens before the review, and a worktree is not a sandbox (GOVERNANCE). Gate commands that would run unsandboxed still ask.
- **Opt-in per build,** never a default, and not part of any permission mode.
- Nothing eligible reaches the operator's checkout: the merge is gated on the review, the merge card, and the existing gates 1 to 5.
- Taint still limits what a span may do; the approver does not read agent prose, only the daemon's classified card.

Not yet built: a cap on spend (the mission budget still applies); per-decision dwell-time recording; email review with a second factor; decisions for edits made by a CLI harness.

## 8. Charter amendments (approved and applied 2026-10-25, D-30)
All were Red-tier edits, approved by the operator and applied the same day; the mode is opt-in and weakens no existing gate.
1. `GOVERNANCE.md`: a clause defining a *provisional decision* (eligibility is an allow-list; no authority outside the branch; the merge is blocked until the operator reviews every decision; the destructive-action rule is unchanged for everything not eligible).
2. `specs/008` §4 (the tier model): the provisional-eligibility column.
3. `specs/005` (cards and the console): the decision lines, `/decisions`, `/merge`.
4. `specs/003` §3 (the merge gates): gate 6.
5. `REVIEW.md`: a check that no code path lets a provisional decision reach the operator's branch, an outbound action or the host.

## 9. Verification Plan
Unit (`provisional.rs`, `gates.rs`): only an Amber worktree edit is eligible and every other kind and tier is not; the rule approver allows ordinary edits, refuses protected, host-executed and non-plain paths, and flags dependency files; the ledger folds reviews in, counts only allowed decisions, and the latest review wins; gate 6 is green with none or all ratified and red for pending or rejected (a rejection outranks a pending one).

Engine (`finalize.rs`, `build.rs`): an optimistic build raises no card for its edits, records them, and the merge card waits for every review before appearing, then merges; a rejection stops the merge and keeps the branch; a run that stops while decisions wait keeps the branch and `finalize_build` resumes after review; more work after the card was shown refuses the merge at answer time; the review window ending names the unreviewed ids; a spec draft in an optimistic task still raises its card; the approver refuses a `build.rs` edit and tells the Builder; the **whole ladder** (Builder, tests, audit, review wait, ratify, merge) runs in order. Without the sandbox the mode refuses to start.

Approver (`provisional_run.rs`, scripted model): the Strategist allows a routine edit from a span offered no tools, with no card raised and the daemon's facts in the question; it can refuse what the rules allowed and the Builder is told why; an unreadable answer hands the card to the operator and records why; a rule refusal never reaches a model; a tool call by the approver is refused by the engine; an elevated edit with no second seat parks by default and is allowed flagged `unseconded` when told to; a concurring second seat is recorded and a disagreeing one parks the card; cross-family with one provider has no second seat and fakes none; `approver = "rules"` makes no model call. The answer parser and the question builder are unit-tested, including contradictory and malformed answers.

Caps and rollback: past `max_decisions` or `max_approver_calls` the next edit goes to the operator with the reason logged; a rejected edit is removed (or restored) from the worktree, its decisions are marked reverted, the Builder's next attempt is told what was rejected and why, and the rest merges after review (the whole path, through two attempts, is tested). `restore_to_base` restores, removes, and refuses odd paths; a reverted decision no longer counts.

Service: decisions are listed and reviewed by the console only; a review with the wrong hash, an unknown id, a bad resolution or a refusal is rejected; the narrator and relay cannot review or merge; `build.merge` needs a task with a kept branch; the relay event list is pinned. Console and CLI: the `--optimistic` flag, `/decisions` and `/merge` parse; the listing, show, ratify, reject and bulk commands send the hash that was shown. Run: a build awaiting review ends the run with exit code 5.

Not covered: a real model driving an optimistic build; the relay email for the review request against a real mailbox.
