---
id: step-3c-model-pool-plan
title: "Step 3c plan: the model pool, exhaustion promotion and target substitution (D-26)"
author: strategist
timestamp: 2026-10-24T18:00:00Z
source_type: internal_generated
confidence: low                    # estimates are mine and nothing is built; provider exhaustion signals are unverified
status: active
source_mission: 2026-10-23-foundational-design
tags: [step-3c, plan, milestones, model-routing]
---

# Step 3c plan: model pool

Scope: build [D-26](decisions.md) and spec 002 amendment 002-A1 (§4.1, §3.1 rule 5). Roles ask for capabilities and an optional target model; a ranked pool tracks each model as `available`, `cooling_down` or `exhausted`; when the target is exhausted the next *similar* model takes over, a *weaker* one only after the operator confirms, and with nothing eligible the task parks and resumes.

Effort is in focused build hours including tests, the same unit `specs/005c` uses ("about fifteen hours"). They are my estimates before any code is written: read them as ±30%.

## Where the code stands (what this changes)

- A span's family is fixed at its start: `role_family` in `[access]` (default `claude`) feeds `SpanCtx.family`, and `model_turn` (`crates/delphi-engine/src/engine.rs`) walks only that family's allowed endpoints. `FAMILY = "claude"` is still a constant there.
- A `429`/`503`/`529`/`RESOURCE_EXHAUSTED` all become `ModelError::Throttled` (`model.rs`) and move to the next endpoint *of the same family*. Quota exhaustion is not distinguished from a rate limit, and nothing is remembered between turns, so every turn re-probes a dead model.
- A harness span is not resumable (spec 009 §7B): promotion for a harness model can only happen when a span starts, not mid-span.
- The Council already degrades to `homogeneous` when one family is available (`council.rs` `available_families()`); the Builder ladder already has a `homogeneous_retry`. Both need to read the pool instead of config alone.

## Milestones

| # | Milestone | Specs | Effort | Needs from the operator |
| :-- | :--- | :--- | :--- | :--- |
| M1 | **Exhaustion signal.** `ModelError::Exhausted { reason, reset_at }` distinct from `Throttled`. Classify Anthropic usage/credit-limit errors, Gemini `RESOURCE_EXHAUSTED` quota (vs per-minute rate), Bedrock/Vertex quota errors, and the harness `rate_limit_event` at 100% utilization. Starts with a spike that collects real error bodies (Council 005 and the Step 2 audit both hit Claude and Gemini quota; their transcripts are the first fixtures). Unknown errors stay `Throttled`, never `Exhausted`. | 002 §3.1 r5, 009 | 6 h | The logs or transcripts from a real quota-exhausted run, if any survive |
| M2 | **Pool config and state.** `[routing.pool]` and `[routing.targets]` parsed and validated in `config.rs` (known models, catalog entries, allowed endpoints). A daemon `PoolState` with the three states, reset times and the 15 min → 4 h doubling re-probe (reuses `probe.rs`), persisted in Zone 3 (`local/pool-state.json`) so a restart does not forget. `delphi models` and a `doctor` check show it. | 002 §4.1 | 9 h | nothing |
| M3 | **Capability-based route resolution.** Catalog gains the capability facts routing needs (context length, write tools, approval bridge, harness read-only). A pure `resolve_route(request, pool) -> Target \| Similar \| Weaker \| NoModel`. `role_family` stays as the default target so existing configs behave the same. Table-driven tests only; no engine change yet. | 002 §4, §4.1 | 10 h | nothing |
| M4 | **Engine integration and mid-span promotion.** `model_turn` walks the resolved candidates across families, not only endpoints within one. Promotion at a turn boundary after every tool call has a result; foreign signed reasoning downgraded (§3.1 rule 2); reservation released and re-taken on the new model's prices; `model.exhausted` and `model.promoted` events and ledger rows; a harness span promotes only at span start. Remove the `FAMILY` constant. | 002 §3.1, §4.1; 005 §2.4 | 16 h | nothing |
| M5 | **Confirmation and parking.** `model.degraded_confirm` approval card (target, substitute, what is lost) through the existing broker; consent recorded per task and replayed on recovery; unattended runs wait instead of assuming consent; `blocked: no_model` node status in `recovery.rs` that survives a daemon restart and resumes when the pool changes. | 002 §4.1; 005 §2.5 | 11 h | A decision on which approval tier the card is (proposed: Amber) |
| M6 | **Council and retry ladder read the pool.** Seat assignment and `homogeneous` marking use pool availability and similar-tier degrade; the Builder ladder's upscale and different-family retry walk the pool; no role is pinned to a named model in code. | 003 §3.2, §4; H-007, H-008 | 5 h | nothing |
| M7 | **Surfaces and spec amendments.** Spec 005 gets the new event kinds (`model.exhausted`, `model.promoted`, `model.degraded_confirm`) and the card; console shows the card and `/models`; the Narrator agents strip shows target versus actual; the mission report flags a weaker substitute. | 005 §2.3, §4 | 8 h | Ratification of the spec 005 amendment |
| M8 | **Fault-injection tests and red-team sweep.** A `ScriptedModel` `Exhaust` step; spec 002 §4.1 tests 1–7 and 6a; a sweep of M1–M6 for consent bypass, endpoint-policy escape and budget double-counting. | 002 §4.1 | 7 h | nothing |

**Total: 72 h** (range roughly 50–95 h).

## Order and slices

```
M1 ─┐
    ├─► M3 ─► M4 ─► M5 ─┬─► M6 ─┐
M2 ─┘                   └─► M7 ─┴─► M8
```

M1 and M2 are independent and can run in parallel. M3 is pure code with no engine risk, so it lands early. Two usable stopping points:

- **Slice A (M1, M2, M3, M4): about 41 h.** An exhausted target is replaced automatically by a similar model, across families, mid-span. Weaker substitutes are not allowed yet (the task stops with a clear message). This answers the original request.
- **Slice B (+ M5): about 52 h.** Adds confirmation for weaker substitutes and parking when nothing is eligible.
- **Full (+ M6, M7, M8): 72 h.**

## Risks

1. **Provider exhaustion signals are unverified.** Misreading a rate limit as exhaustion promotes too early and wastes the preferred model; the opposite wastes time retrying a dead one. Mitigation: M1 starts with a spike on real error bodies, and anything unrecognised stays `Throttled` (today's behavior).
2. **Quality loss at the handoff.** Cross-family promotion drops signed reasoning. M4 prefers a model whose reasoning is replayable, and the event records the loss so the report can show it.
3. **Harness spans cannot be resumed.** A harness model that dies mid-span loses that span's work, as today; promotion helps the next span. This is the existing limit of spec 009 §7B, not a new one.
4. **Config compatibility.** Existing workspaces use `role_family`. It must keep working as the default target, with a test that an unchanged config routes exactly as before.
5. **Budget interplay.** A promotion to a pricier model re-reserves at its prices; a promotion that would exceed the mission budget escalates as it does today, never silently downgrades.

## Definition of done

Spec 002 §4.1 tests 1–7 and 6a pass; an unchanged config routes as before; a real two-family run where the target is made unavailable (a model removed from the pool, or a recorded exhaustion fixture) finishes the task on the substitute with `model.promoted` in the log; and `delphi models` shows the states.
