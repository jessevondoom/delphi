---
id: step-3c-review-brief
title: "Step 3c independent review: the brief and the seats' focus questions"
author: red-team
timestamp: 2026-10-25T00:30:00Z
source_type: external_untrusted
confidence: low
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, step-3c, review]
---

The common brief given to every seat, then each seat's focus questions. The seats were read-only reviewers from other model families than the author (Claude), per `[H-008]`.

## Common brief

You are an independent reviewer. The code was written by a Claude model; you are a different model family and your job is to find what the author missed. **Read only.** Do not modify any file or create files in the repository. **Do not run shell commands at all** (never use `run_command`; it is not available to you and a request for it ends your run with no output). Use only the file-reading tools: `view_file` (read a file, with line ranges), `grep_search` (search the code), `list_dir`, and `find_by_name`. You may read any file in this repository (the working directory).

## What was built

When a model runs out of quota, Delphi (a multi-agent Rust tool) should switch to another usable model and keep working. Read these first:

1. `specs/002-adapter-and-routing.md`, section 4.1 ("Amendment 002-A1: Model Pool & Exhaustion Promotion"). This is the ratified requirement.
2. `missions/2026-10-23-foundational-design/decisions.md`, the entry "D-26" and the approval lines after it.
3. `missions/2026-10-23-foundational-design/step-3c-model-pool-plan.md`, what each milestone claims to do.

## Code in scope (all under `crates/`)

- `delphi-adapter/src/exhaust.rs` (classifies a failure as a spent quota or a rate limit), `delphi-adapter/src/http.rs`, `delphi-adapter/src/config.rs` (`[routing.pool]`, `[routing.targets]`, `PoolKey`, `pool_entries`, `endpoints_serving`, `Endpoint::can_mutate`)
- `delphi-engine/src/route.rs` (`resolve_route`: picks which endpoint/model serves a request), `delphi-engine/src/pool.rs` (runtime state per endpoint+model, persisted to a JSON file), `delphi-engine/src/engine.rs` (`model_turn`, `park`, `degraded_decisions`, `route_harness`, `family_usable`, `ask_resolution`), `delphi-engine/src/harness.rs` and `harness_run.rs` (subscription CLIs), `delphi-engine/src/council.rs`, `delphi-engine/src/build.rs` (retry ladder), `delphi-engine/src/status.rs`, `delphi-engine/src/models_cli.rs`, `delphi-engine/src/service.rs` (`models.get`), `delphi-engine/src/mission.rs`
- Tests live next to the code in `#[cfg(test)]` modules.

## How to review

Trace real paths through the code instead of skimming. For every finding you must have read the lines involved and be able to quote them. Do not report style, naming or formatting. Do not report something just because a comment says it is unfinished: the plan file lists what is known to be missing.

## Output format (exactly)

For each finding:

```
FINDING P0|P1|P2 <short title>
File: <path>:<line>
Quote: <the exact line(s) from the file>
Scenario: <concrete inputs and state> -> <the wrong outcome>
Why it matters: <one or two sentences>
```

P0 = a security, consent or policy breach, or lost/corrupted data. P1 = wrong behavior in a plausible situation. P2 = a minor defect or a missing test for a real risk.

After the findings, list each focus question below as `HOLDS: <question number> because <one line citing code>` when you checked it and found no problem. End with the single line `REVIEW COMPLETE`.

Be honest about uncertainty. If you could not verify something, say `UNVERIFIED` and why instead of guessing. A short list of real, verified findings is worth more than a long list of guesses.

## Seat A focus: security, consent and policy

Focus questions (answer each with a finding or `HOLDS`):

1. **Consent.** Can a task run on a *weaker* model than its target without the operator having confirmed? Trace `Route::Weaker` in `model_turn` (`engine.rs`), `degraded_decisions`, the timeout path (`Resolution::TimedOut`), `ready_for_turn`, and what happens after a daemon restart. Look for any path where silence, an old decision, another task's decision, or a forged/replayed `model.degraded_confirm` event counts as a yes.
2. **Endpoint policy.** `[access] allowed_endpoints` is a compliance pin. Can any code path (resolver, pool entries, `[routing.targets]`, fallback to a harness, Council seats, retry ladder, probes in `reprobe_due`, `delphi models --probe`) send a request to an endpoint outside it?
3. **Mutating roles.** A harness without an approval bridge (`agy`) must never serve a role that can write files or run commands. Check `route_request`'s `mutating` flag, `Endpoint::can_mutate`, `run_harness`, and any place that picks a harness without going through the resolver.
4. **Secrets.** Provider error text is stored in `model.exhausted` events and shown in cards and the status file. Is every path redacted before it is stored or displayed? Check `exhaust.rs`, `http.rs`, `engine.rs` (the `raw` field), the approval card payload, `status.rs`.
5. **Trust of inputs.** The pool state file `local/pool-state.json` and the mission log are read back. What can an attacker who can write those files (same user) cause? Is anything from them used unsafely (paths, indices, unbounded sizes, parsing that can panic)?
6. **Approval integrity.** The "run on a weaker model?" card goes through the approval broker. Can it be answered by a client that should not be able to (read-only Narrator scope), answered for a different payload, or auto-approved by a permission mode or "allow similar"?
7. **Taint.** `agy` output is treated as untrusted. Does routing a span to a different family or harness after a promotion ever launder or lose that taint?
8. **Denial of service.** Can an unanswered card, a flapping provider, or a pool that never settles make a task loop forever, spawn unbounded work, or grow memory/log without bound? Check `park`, the `passes` cap in `model_turn`, `blocked_since`, repeated `task.blocked` events.

## Seat B focus: correctness, state and concurrency

Focus questions (answer each with a finding or `HOLDS`):

1. **Classifier.** `exhaust.rs` decides whether a failure is a spent quota (explicit signal, or a `retry-after` over 15 minutes) or just a rate limit. The spec says an unrecognised error must never become "exhausted" by guess. Find inputs that are misclassified in either direction (regex edge cases, duration parsing, `Go` durations like `0s`/`1h`/huge values, 400/402/429 bodies, a harness `rate_limit_event`, units of `resets_at`). Also: does `http.rs` classify before the retry loop in every path?
2. **Pool state machine.** `pool.rs`: cooldowns, "3 or more consecutive throttles over 10 minutes becomes exhausted", known-reset recovery, re-probe doubling to a cap, persistence. Look for wrong transitions, a state that can never be left, arithmetic overflow/underflow (`i64`, `u32` shifts, `Duration`), lost updates with the `Mutex`, and a corrupt or stale state file.
3. **Resolver.** `route.rs` `resolve_route`: hard requirements versus preferences, tier ranking, "similar" versus "weaker", ordering (target, usual family, avoided families, endpoint policy order, pool order), a model mapped to several tiers. Find a request where it picks the wrong pair, returns `NoModel` when a usable pair exists, picks an ineligible pair, or panics (indexing, `unwrap`, empty lists, `min_by_key` ties).
4. **Engine integration.** `model_turn` in `engine.rs`: every `ModelError` arm must release or settle the budget reservation exactly once. Check double release, a leaked reservation, a charge for a call that never ran, `continue 'endpoints` paths, the `passes` cap, and `last_key` / `model.promoted` reporting.
5. **Parking and resuming.** `park`, `ready_for_turn`, `blocked_since`, `degraded_decisions`: can a task park when it should escalate, escalate when it should park, resume into an immediate re-park loop, or ask the operator twice for the same decision? What happens to a parked task on cancel and on daemon restart (`recovery.rs`)?
6. **Harness paths.** `harness_run.rs` and `route_harness`/`has_api_route`: ordering between "harness first", "API", and the fallbacks; a harness span is not resumable. Look for a span that runs twice, a side effect repeated after a fallback, or a fallback that cannot happen.
7. **Council and ladder.** `council.rs` (`available_families`, `assign`, `avoid`) and `build.rs` (`other_family`, attempts, `homogeneous_retry`): anything that blocks instead of degrading, mislabels a result as heterogeneous when it was not, or consumes a retry attempt on a promotion.
8. **Tests.** Which risky behaviors above have *no* test, or a test that cannot fail (asserts nothing, early `return` that skips it, time-dependent without an injected clock)? Name the test and why.
