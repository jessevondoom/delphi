---
id: optimistic-workflow-exploration-2026-10-25
title: "Optimistic workflow: provisional decisions, reviewed before merge"
author: strategist
timestamp: 2026-10-25T14:00:00Z
source_type: internal_generated
confidence: low
status: draft
source_mission: 2026-10-23-foundational-design
tags: [plan, optimistic, provisional-decisions, approvals, governance]
---

# Optimistic workflow (exploration, draft)

**The idea, as requested:** when the operator asks for it, the Orchestrator may answer approval cards itself on a non-merged branch. Every such decision is recorded, and the merge is blocked until the operator has reviewed and approved every one of them.

This is an exploration. It is not ratified and it changes the charter, so §3 lists what would need amending. It builds on [`gap-closure-plan-2026-10-25.md`](gap-closure-plan-2026-10-25.md) (the relay and parking) and is a candidate arm in [`evaluation-plan-2026-10-25.md`](evaluation-plan-2026-10-25.md). The code facts below come from `approval.rs`, `finalize.rs`, `tools.rs` and `GOVERNANCE.md`; I read only parts of them, and §2.1 flags the places where I have not seen the whole picture.

## 0. Why it fits

- **Gap 1 asks for more than a stall.** Parking stops work until the operator answers. Optimistic mode keeps working and moves the human decision to a single review at the end, which suits long builds and overnight runs.
- **It is the natural headless mode.** `delphi run --on-card policy:<file>` covers cards a fixed rule can answer. Optimistic mode covers the ones that need judgment, without giving up the operator's say.
- **The architecture already has the right shape.** All effects happen on a task branch in a worktree, gates are bound to a commit SHA and re-checked at answer time, and the merge card is the operator's alone. Optimistic mode adds a sixth gate to that structure. It does not bypass any gate.

## 1. The tension with the charter

`GOVERNANCE.md` says the console and the daemon are the operator's instruments and that "final merges belong strictly to the operator". Destructive actions "are denied, never approved" if nobody answers. An agent answering cards looks like it breaks both.

The resolution: **a provisional decision is not an approval.** It lets the agent *proceed* with an action whose every effect stays inside the branch and can be thrown away. It carries no authority outside the branch. The operator's later review is the approval, and the merge stays blocked until it happens. If this holds up, the invariants stand:

| Invariant | Under optimistic mode |
| :-- | :-- |
| Final merges are the operator's | Unchanged. Gate 5 stays, and gate 6 is added. |
| Red is never auto-approved | Unchanged. Red actions are not eligible for provisional decisions at all (§2.1). |
| Timeout denies, never approves | Unchanged for destructive and outbound actions. Optimistic decisions are explicit, recorded and operator-requested, not timeouts. |
| Taint limits action | Strengthened. A tainted span can't be the approver (§2.2). |
| Worktrees are not sandboxes | **Becomes load-bearing.** The mode refuses to start without the OS sandbox (§2.7). |

## 2. Design

### 2.1 Eligibility: only branch-contained, reversible effects

A provisional decision is allowed only if the action's whole effect set stays inside {worktree files, commits on the task branch, sandboxed execution without network}. The classifier already works out an `ActionKind` before anything runs, so eligibility is a predicate on it.

| Action kind | Provisional decision? | Notes |
| :-- | :-- | :-- |
| Worktree edit, branch commit | **Yes** | The core case. |
| Sandboxed command (tests, build) | **Yes** | Only inside the OS sandbox, no network. |
| Pinned dependency install | **Yes, flagged** | Runs inside the sandbox; the new dependency set also stays Red at merge, so the operator sees it twice. |
| Unsandboxed command on the host | **No** | Not contained. The task parks for a card, or the step is skipped. |
| Writing a spec outside the worktree, ratifying a spec | **No** | Ratification is the operator's act. See §2.8 for building against a draft. |
| Outbound action or connector write (PR, email, Slack, webhook) | **No** | Not reversible. It is queued as a draft and runs only after the operator approves it. The connector pipeline already has a draft step. |
| Merge, PR creation, constitution or `REVIEW.md` edit, destructive command | **Never** | Red, as today. |

I have seen only part of the `ActionKind` list. Implementation must start by making the predicate an explicit allow-list (default: not eligible), so a new kind added later is ineligible until someone decides.

### 2.2 Who decides, in order

1. **Ratified rules first.** Allow rules the operator already ratified answer the card as they do now. That isn't a decision and is not recorded as one.
2. **The Strategist as approver, under a narrow skill.** Otherwise the Strategist answers through a new `provisional-decision` skill, subject to:
   - the approver's span must be untainted; a span that has read content at `federated_peer` or below is ineligible, and the card parks instead;
   - the approver is never the requester (the Builder, or whichever role raised the card), and never a span the requester dispatched;
   - the approver's input is the **daemon-built card** (the classified action and payload) plus the approved spec, not free-text from the requester, so a poisoned rationale can't steer it;
   - it returns `allow` or `deny` plus a short rationale, recorded verbatim;
   - while deciding, it has no write tools at all. Its normal write scope (roadmaps, council notes) is not available in that span.

   *Why the Strategist:* the question a provisional decision asks is "does this action serve the approved spec and stay inside its scope?", which is the scope audit the Strategist already does, and it already can't waive Red-Team `P0`s. The manifest change is one added line: it also cannot waive any eligibility rule in §2.1. *Why not the Builder* (the `builder` role, whatever the active theme calls it): it is the requester, so it would be approving its own actions, which §2.2 forbids. *Why not the Orchestrator:* it dispatches the requester and holds the whole task's context, and its listed blind spot is smoothing over dissent. A dedicated tenth role remains possible later if the Strategist's other duties get in the way; it is not needed now. Routine-class cards may use the cheaper `fast-triage` tier instead of `deep-reasoning`, since eligibility is already checked deterministically.
3. **A second seat is optional, and its absence is recorded, not blocked.** Operators often have only one provider, so the second seat can't be required. `[optimistic] second_seat` takes:
   - `cross-family`: Red-Team on another model family, for decisions above the routine class. Preferred where two families exist.
   - `any`: Red-Team in a fresh context on the same family. Weaker, and marked `single-family` in the ledger.
   - `off`: no second seat.

   When no second seat ran for a decision above the routine class, `[optimistic] unseconded = "park" | "allow"` decides what happens. The default is `park` (the card waits for the operator); an operator who wants flexibility sets `allow`. This costs safety little, because eligibility (§2.1) and gate 6 still hold; what it risks is wasted work if you later reject the decision. Unseconded decisions are flagged and listed first at review. A disagreement between two seats that did run always parks the card; the Strategist doesn't break ties.

### 2.3 The decision ledger

The daemon writes the ledger from what it observed. The agent doesn't write it, so an agent can't omit an entry.

```
decision.provisional {
  decision_id, approval_id, task_id, span_id,
  tool, tier, payload_blake3, at_sha,
  effects: [ ... ],                 // what the daemon saw it do
  decided_by: {role, model, family, span_id, taint},
  concurred_by: {role, model, family} | null,
  outcome: "allow" | "deny",
  rationale, risk_class
}
decision.reviewed {
  decision_id, resolution: "ratified" | "rejected" | "revised",
  note, by: "operator", via: "console" | "email" | "relay:<channel>"
}
```

Commits made after a decision carry a trailer, `Delphi-Decision: <decision_id>`, so reviewing a decision shows exactly the changes it enabled.

### 2.4 The merge block: gate 6

A new gate, "decisions reviewed", sits beside the five existing ones. It is Green only when every `decision.provisional` for the task has a `decision.reviewed` of `ratified`, and each review is bound to the decision's `payload_blake3` and `at_sha`. Like the existing gates, it is re-evaluated when the merge card is answered and not only when it was shown. A decision added after the review (a new commit) makes the gate non-green again.

Gate 5 (the operator's merge approval) is unchanged. The operator can do both in one sitting.

### 2.5 Operator review

- **Surfaces:** `/decisions` in the console, `delphi decisions list|show|ratify|reject`, a Narrator pane, and the relay digest (the email form is in the plan's §1.2).
- **Per decision:** what was asked, what was allowed, the rationale, the second seat's view, the files and commits it enabled, and the cost spent after it.
- **Sorted by risk, not by time.** Decisions where the two seats disagreed, or where the approver went against the default, come first.
- **Bulk ratify is allowed only for the routine class**, and the system records dwell time. Higher classes need an individual answer. If an operator ratifies a long list in seconds, the tool says so once; the aim is to make rubber-stamping visible, not to forbid it.
- **High-risk decisions by email** need the TOTP step-up (plan §1.2). Merge approval never goes by email without it.

### 2.6 Rejection

Rejecting a decision means the work built on it is suspect.
- The task branch resets to the commit before the decision, and the task re-queues with a recorded fact: "the operator denied this action".
- Every later decision on that task is **invalidated** and returns to the queue for re-review, since it was made in a world that no longer exists. This is conservative and wasteful, but it is easy to reason about and it can't leave a dependent decision standing.
- The cost of the discarded work is reported on the rejection.
- This needs only a branch reset and a re-queue. It does not need the full `/rewind` (spec 008 §3, usage-gated).

### 2.7 Guard rails

- **The sandbox is required.** The mode refuses to start without it, because everything optimistic happens before the review. Git hooks and config changes are already undone after every tool result; keep that.
- **Opt-in per mission** (`/build --optimistic`, `delphi run --optimistic`). It is never a default and is not part of `mission-auto`.
- **Caps:** a maximum number of provisional decisions, a spend ceiling on optimistic work, and a wall-time limit. Hitting a cap parks the task.
- **Pause and kill switch** from the console or relay at any time, with a recorded reason.
- **No nesting:** a provisional approver can't approve decisions about its own dispatches.

### 2.8 Optional extension: build against a draft spec

*No Spec, No Build* requires a ratified spec, and the spec gate reads the ratification lock. A draft spec could be built against optimistically: gate 3 already blocks the merge until the spec is ratified. The cost is that a rejected spec discards the build. Treat this as phase O2 and decide after the first phases show how often specs change on review.

## 3. Charter changes this would need

All Red-tier (constitutional), ratified by the operator:
1. `GOVERNANCE.md`: a clause defining *provisional decisions* (eligibility, no authority outside the branch, merge blocked until reviewed), and a note that the destructive-action rule (timeout denies) still applies to every non-eligible action.
2. `specs/008` §4 (the tier model): a column for provisional eligibility.
3. `specs/005` (approval cards): a card state "decided provisionally, awaiting review".
4. `REVIEW.md`: a check that no code path lets a provisional decision reach the operator's branch, an outbound action or the host.
5. A new gate 6 in `specs/003` §3 and in `gates.rs`.

## 4. Red-teaming the idea

| Threat | Mitigation | Residual risk |
| :-- | :-- | :-- |
| **Rubber-stamping.** The operator approves 40 decisions without reading. | Risk-sorted queue, bulk ratify only for routine decisions, dwell-time recording. | Real. The mode moves the human decision to a worse moment (the end). Measure it (§6). |
| **Collusion.** Builder and approver share a model and a blind spot. | Approver is never the requester; second seat on higher risk, other family where possible. | Same-family blind spots remain on routine decisions. |
| **Injection into the approver.** | Untainted approver only; approver reads the daemon's card, not requester prose. | A poisoned file could shape a payload; the payload is shown verbatim at review. |
| **Sandbox escape.** An action thought contained isn't. | Sandbox required; eligibility is an allow-list; hooks and config restored. | The sandbox is the trust root. Bugs in it become worse in this mode. |
| **Hidden dependency between decisions.** | Reject invalidates all later decisions on the task. | Wasted spend on discarded work. |
| **Ledger gaps.** An agent acts without a record. | The daemon writes the ledger from observed effects. | None known beyond daemon bugs. |
| **Time-of-check versus time-of-use.** The reviewed thing differs from what ran. | Reviews bind to `payload_blake3` and `at_sha`, and gate 6 is re-checked at merge. | Low. |
| **Approver drift.** A model update changes what it allows. | The model and family are in each record; review shows them. | Compare agreement rates across versions. |
| **Wasted spend.** Rejected work costs money. | Spend ceiling; the cost is reported at rejection. | Accepted cost of the mode. |

## 5. Tests (write first)

1. A tainted span can't act as approver; the card parks.
2. An ineligible action (unsandboxed exec, outbound, merge, constitution edit) can never receive a provisional decision, whatever the approver says.
3. The approver never sees free-text from the requester, only the daemon's card.
4. The ledger contains an entry for every approval the approver answered, even if the agent claims otherwise.
5. Gate 6 is red with an unreviewed decision, green after review, and red again after a new commit adds a decision. It is re-evaluated when the merge card is answered.
6. Rejecting a decision resets the branch, invalidates later decisions and re-queues the task with the denial recorded.
7. Disagreement between two seats that ran parks the card; the approver doesn't break ties. With one provider, `second_seat = "off"` and `unseconded = "park"` parks above-routine cards, and with `unseconded = "allow"` the decision is recorded as unseconded and sorted first at review.
7b. The Strategist's span during a provisional decision has no write tools; a request for one is refused.
8. The mode refuses to start without a sandbox.
9. Bulk ratify is refused for non-routine decisions.
10. A review answer bound to an old `payload_blake3` or `at_sha` is rejected.

## 6. How we would know it works

Recorded per optimistic mission and added to the benchmark's secondary metrics:
- **Ratification rate** by risk class. If routine decisions are ratified below about 90%, the approver is too loose. If everything is ratified, either the approver is good or the operator isn't looking, so pair this with dwell time.
- **Rejection cost:** dollars and minutes spent on work that was discarded.
- **Operator minutes** at review, compared with answering the same cards interactively.
- **Out-of-branch effects: zero.** This is a hard requirement, not a metric. Any one is a bug in the mode.

## 7. Phases

- **O0:** a draft charter amendment and spec text; the explicit eligibility allow-list; operator decision on §8. No code.
- **O1:** the ledger events, gate 6, `delphi decisions`, branch reset on rejection, the sandbox-required check. The approver is a stub that answers from a fixed rule, so the machinery can be tested without a model.
- **O2:** the Strategist as approver, the optional second seat, risk classes, the relay digest. Optionally, building against a draft spec (§2.8).
- **O3:** trial on real builds alongside the benchmark arm; tune from §6.

## 8. Open decisions for the operator
1. ~~Is the "provisional decision is not an approval" framing acceptable?~~ **Accepted by the operator, 2026-10-25.** The charter edits in §3 still need Red-tier ratification.
2. ~~Which role approves?~~ **Proposed: the Strategist** (§2.2). Awaiting the operator's confirmation. The Builder is ruled out because it is the requester.
3. ~~Second-seat concurrence~~ **Optional, per operator.** Not required, because some operators have one provider (§2.2 item 3). Awaiting a decision on the default for `unseconded` (proposed: `park`).
4. Draft-spec building (§2.8): in scope for O2 or later?
5. Spend and decision-count caps: starting values.

## 9. Alternatives considered
- **Deterministic policy only** (`--on-card policy:<file>`): safe and simple, but it can only answer questions known in advance. Kept as step 1 of §2.2.
- **Auto-approve with a post-hoc audit:** same effects, but the merge is not blocked on the audit. Rejected; the block is the safety property.
- **Park everything:** the plan's default for gap 1. Safe, and it stalls. Optimistic mode is the opt-in alternative for when the operator would rather review once at the end.
- **A wider `mission-auto`:** it grows an existing permission mode without any record or review. Rejected.

My recommendation is layering all three: rules first, optimistic decisions where the operator opts in, and parking for everything else.
