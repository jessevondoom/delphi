---
id: spec-081-evidence-ledger
title: '081: Evidence conventions note: invariants and row shape for recipes over verified events (draft)'
author: architect
timestamp: 2026-11-05T00:00:00Z
status: draft
wave: note (applies to waves 1 and 2; builds nothing)
governed_by: GOVERNANCE.md
review_rubric: REVIEW.md
source_type: internal_generated
confidence: low
source_mission: 2026-11-05-session
tags:
- spec
- measurement
- conventions
- evidence
- draft
---
# Spec 081: Evidence conventions note

> **Status: draft, never approved.** Council options A and B merged. The monolithic ledger is **out** (Strategist decision). This is 081's own Simpler Alternative: recipes over verified events plus this one note. **No store, no persisted view, no `state.db`; views are computed on demand from `events.jsonl`.** Nothing here is permission to build.

## 1. Invariants every recipe and report must keep
1. **Advisory only.** No auto-ratify, no auto-demote, no auto-select of seats; metrics are evidence on `/retro` and cards (GOVERNANCE §1.1, §5.7, §7; `[H-004]`).
2. **Taint is computed, never declared.** No field lets a producer name a tier.
3. **KB dissent inherits the lowest input taint.**
4. **Zero-hit security invariants are never demoted** (tagged `security|constraint`, spec 080 W1.3); they are listed as exempt.
5. **Override and escalation counts are not targets.** No seat sees any report; none enters a prompt.
6. Family spread (H-001/H-008) is a floor cost; cost never argues for fewer seats or families.
7. Counts never feed ranking, promotion or taint. No claim text, prompts or outside content in any figure.

## 2. Row shape (when a recipe prints rows)
`{area, event_seq, mission_id, ts, subject_id, count, cost_usd?}`. `subject_id` is an id (fact, task, rule, proposal, method, origin), never text. **`min_taint` is dropped until 079 S1 passes**; then it is added as a computed column from `span.taint` and the event's own tier. Never writable over any RPC; nothing is stored, so there is nothing to repair.

## 3. Relation to 078-080
| Spec | Role | Wave |
|---|---|---|
| 079 | S1 ingress origin test; hard blocker for `min_taint` and any area-1 figure | 1 (order 1) |
| 080 | W1.2 `host_search_kb` event, W1.3 `security|constraint` tag, W1.4 jq top-N reads | 1 (order 2) |
| 078 | Council cost/choice join, `/retro` `learn.*` footer, `span.taint` by cause (needs 079) | 2 |
| 081 | This note: invariants and row shape only | none |

## 4. Missing events, in order
1. **`host_search_kb`** (wave 1, spec 080 W1.2). Verified missing: service.rs:506-539 appends nothing.
2. **KB promotion.** `pages_promote` (service.rs:1187-1191) calls `Library::promote_statement` and the handler appends no event. Whether `promote_statement` itself emits one was **not verified**.
3. **KB retraction.** `retract_all` (fact_audit.rs:519; fact_store.rs:262) writes the fact journal. The journal file is `journal.jsonl` in the state directory (fact_audit.rs:40). `retract_all` appends its `retract` record there through `Journal::append` (fact_audit.rs:535), not to the mission's `events.jsonl`, so a recipe over `events.jsonl` cannot see retractions (verified in code). The requirement stands: retraction must also emit a `validator.event` with `payload.kind` `retract` to `events.jsonl`, the way `fact.read` does (fact_audit.rs:304-308).
4. **Skill use**, last. No skill-use event found by name.

## 5. Simpler Alternative Considered
This note is it. A ledger, a derived table or an agent-writable metrics tool was rejected (second source of truth; taint laundering, `[H-004]`; seat-visible scores become targets).

## 6. Verification
Each wave-1/2 recipe, recounted by hand from `events.jsonl`, equals its output. No file besides the log and this note holds any figure. A figure derived from a low-taint span, after 079 passes, shows its `min_taint`.

## 7. Not verified
Spec 040, 005, 003 and 010 text (specs not read this session); connector events; the `type` vs `kind` serialised key in `events.jsonl`; whether promote and retract already emit events; skill-use event.
