---
id: spec-078-council-scorecard
title: '078: Council scorecard: cost and operator choice joined from existing events (draft)'
author: architect
timestamp: 2026-11-05T00:00:00Z
status: draft
wave: 2
governed_by: GOVERNANCE.md
review_rubric: REVIEW.md
source_type: internal_generated
confidence: low
source_mission: 2026-11-05-session
tags:
- spec
- council
- measurement
- scorecard
- draft
---
# Spec 078: Council scorecard

> **Status: draft.** Council options A and B merged (Strategist decision), mission 2026-11-05-session. Nothing here is permission to build.
> **Wave 2.** Companion wave-2 slices (the `/retro` footer and the `span.taint` recipe) are in §1b. No store, no persisted view, no `state.db`: views are computed on demand.

## 1. Smallest slice
**S1 (corrected): a recipe-level join over existing events, no schema change.** `council.verdict` stays append-only and gains no field.
- **Cost:** sum `cost_usd` of `agent.usage` events whose `task_id` equals the Council's `task_id`, **with no `mode` filter.** Seats run as spans at depth > 0, so their usage is stamped `mode = "dag"`, not `council` (seats go through `run_span`, council.rs:768; `mode` is set at engine.rs:435: `direct` at depth 0, else `dag`). The daemon's own `council_spent` already sums by `task_id` only (council.rs:783-785). The earlier `mode='council'` filter would have returned zero and is removed.
- **Choice:** read the option from `council.followed {path, option, steps}` (service.rs:2517), not from `decision.resolved.choice_id`. `option` is the option label, or null when the Council gave no options (service.rs:2498-2499, 2511). Join to the verdict by `path`.
- **Derive `followed` vs `escalated_choice`** by comparing `followed.option` with the verdict's `options` (`[{label, seats}]`, council.rs:980). The verdict carries no explicit "recommended" flag (verified at council.rs:980), so the draft rule is: `followed` = the option the Council's deterministic resolution favours (a verdict with `escalated` null and no options, or the single/first-listed supported option); `escalated_choice` = an offered option picked after a deadlock escalation (`escalated` non-null). **Open item:** the exact "recommended" definition needs a rule agreed in review; it is not in the event today.
- **`rejected` has no event.** Absence of `council.followed` cannot be told from "not yet answered", so S1 reports **no `rejected` value**. Open item: propose a daemon-written rejection event (operator declines all options) if the value is wanted.
- **`escalated_choice` is a successful deadlock escalation (GOVERNANCE §3), not a failure.**
**S2 (only if S1 proves useful):** `delphi council report` running the same join, one line per Council.

## 1b. Wave-2 companion slices
- **078 S1 footer for `/retro`:** counts of `learn.*` events for the period: proposed (`learn.proposed`), accepted / rejected / withdrawn (`learn.proposal_resolved {resolution}`). No new event. Event sites listed in 081 (service.rs:2189, promote.rs:303-323, learn.rs:124-202) were not re-read this session: **not verified**.
- **`span.taint` counts-by-cause recipe** (`span.taint {ceiling, cause}`). **Needs 079 S1 passed** first; until then it must not be published.

## 2. Measurement
Per Council: seats, rounds used, cost, `family_diversity` and `homogeneous` (H-001, H-008; `homogeneous` is on the verdict, council.rs:980), and the choice as `followed` or `escalated_choice` (and `rejected` only if an event is added).
Cost is shown beside `family_diversity` and `homogeneous`. **Family spread is a stated floor cost and never overridden:** H-001 and H-008 apply whatever the cost, and the report never recommends fewer seats or families to save money.
Read from daemon events only, never from a span's own report.

## 3. Authority risk
Read-only. (a) A score becomes a target, so **no seat ever sees the scorecard**; it is never in a prompt. (b) The choice is the operator's act, recorded by the daemon. (c) It must not alter the Minority Report (spec 003 §4). (d) `council.verdict` is append-only. Override and escalation counts are not targets.

## 4. What already covers it
- `agent.usage` carries `cost_usd` per span (council.rs:784 reads it); `task_id` equality is the join key (council.rs:784).
- **`council.verdict` has one live shape**, emitted at council.rs:980: `{n, decision, steps, from_findings, path, escalated, homogeneous, unresolved, options, claims}`. The `service.rs:3187` shape is inside a test fixture (it sits near `assert!` code at :3065); the dual-shape worry is dropped. (That :3187 is test code is inferred from context, not read in full.)
- `council.followed` is emitted at service.rs:2517; `council.follow` is only the RPC.
- Not verified: that `decision.resolved` has no link to a Council option; S1 does not use it.

## 5. Simpler Alternative Considered
Join existing events instead of adding fields to `council.verdict`, building a store or a view. A full seat-accuracy scorecard was rejected: no ground truth, and it invites the target problem.

## 6. Verification
Cost of a test Council equals the sum of its `agent.usage` rows by `task_id` with no mode filter, and is non-zero. `council.verdict` is byte-identical before and after the operator answers. `/follow` with a deadlock option yields `escalated_choice`; the recommended path yields `followed`; no answer yields no row value. A one-family Council shows `homogeneous`. The report text never reaches a seat's context. The `/retro` footer equals a manual count of `learn.*` events.
