---
id: step-3c-review-seat-a
title: "Step 3c independent review: Seat A (security, consent and policy), verbatim"
author: red-team
timestamp: 2026-10-25T00:30:00Z
source_type: external_untrusted
confidence: low
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, step-3c, review]
---

Model: `gemini-3.8-flash-high` through `agy --mode plan` (read-only tools only; a first run was refused a shell command and produced nothing, so it was re-run with the instruction to use only the file-reading tools). The output below is verbatim and **untrusted**: every finding is checked against the code in [`report.md`](report.md) before anything is done about it.

````text
FINDING P0 Mutating roles without shell commands can be routed to unbridged `agy` harness
File: crates/delphi-engine/src/engine.rs:992
Quote: mutating: sc.guard.zones.worktree.is_some() && sc.manifest.execute.iter().any(|e| e == "shell-worktree-scoped"),
Scenario: A task targeting `architect`, `writer`, or `red-team` runs while primary models are unavailable and an `agy` subscription harness endpoint is configured. Because `mutating` only checks for a worktree and `shell-worktree-scoped`, `req.mutating` evaluates to `false`. `resolve_route` treats `agy` (which has `can_mutate() == false`) as eligible, and `run_harness` executes `agy` in headless mode where its built-in execution and write tools cannot be disabled.
Why it matters: A harness without an approval bridge (`agy`) is allowed to serve roles authorized to write files, allowing tool execution without Delphi's approval bridge or path guard.

FINDING P0 Unredacted error text in approval card summary and status projection
File: crates/delphi-engine/src/engine.rs:741
Quote: let summary = format!("{} is unavailable ({}). Run this task on {} instead? It has {}.", from, why, to, lost.join(" and "));
Scenario: An endpoint fails due to a provider error containing sensitive text (such as credentials, account IDs, or internal URLs echoed in error strings). While line 742 redacts `why` for `detail["unavailable"]`, line 741 interpolates `why` unredacted into `summary`. This unredacted string is persisted in `approval.requested` events, displayed in approval cards, and written to `status.md` under `## Waiting on you`.
Why it matters: Provider error strings containing secrets bypass redaction and are exposed in plain text in UI approval cards, durable logs, and the git-committed status projection.

FINDING P1 Degraded confirmations keyed only on substitute pair, cross-confirming different targets
File: crates/delphi-engine/src/engine.rs:736-737
Quote: let pair = (key.endpoint_id.clone(), key.model_id.clone());
let (confirmed, declined) = self.degraded_decisions(m, &sc.task_id, &pair);
Scenario: In a task with multiple dispatches, span 1 targeting `opus-x` (frontier tier) is unavailable and the operator confirms running on `sonnet-x` (workhorse tier). Later in the same task, span 2 targets `gem-pro` (frontier tier with a 1,000,000-token context window). When `gem-pro` is unavailable and `sonnet-x` is selected as a weaker model (losing 800,000 tokens of context), `degraded_decisions` checks only the substitute pair `("direct", "sonnet-x")` and returns `confirmed = true` without consulting the operator.
Why it matters: Operator consent given for one specific target and capability loss is treated as consent for degrading entirely different target models and losses within the same task.

FINDING P2 Timestamp subtraction on `pool-state.json` entries can overflow and panic
File: crates/delphi-engine/src/pool.rs:235
Quote: if e.throttles.len() >= n && now - e.throttles[0] >= t.throttle_window_secs {
Scenario: `local/pool-state.json` contains malformed or negative throttle timestamps (e.g. `i64::MIN`). When `record_throttle` executes, `now - e.throttles[0]` (and similarly `fmt_secs(t - now)` in line 130) overflows in signed integer arithmetic.
Why it matters: Corrupted or tampered state in the local pool cache file causes an arithmetic panic that terminates the daemon rather than gracefully discarding the cache entry.

HOLDS: 2 because `cfg.pool_entries()` and `cfg.endpoints_for()` filter by `allowed_endpoints` (`crates/delphi-adapter/src/config.rs:709-715, 630-642`) and `resolve_route` rejects endpoints outside it (`crates/delphi-engine/src/route.rs:141-143`).
HOLDS: 6 because `EngineService::approval_resolve` enforces `need_console` (`crates/delphi-engine/src/service.rs:257`), `Broker::resolve` checks `payload_blake3` (`crates/delphi-engine/src/approval.rs:314`), and `ask_resolution` bypasses `Broker::need`.
HOLDS: 7 because `run_harness` calls `sc.span.observe_input(Tier::L0)` in place (`crates/delphi-engine/src/harness_run.rs:217`), monotonically clamping the span's ceiling across failovers, tool outputs, and child dispatches (`crates/delphi-engine/src/engine.rs:1170, 1245`).
HOLDS: 8 because `model_turn` caps passes at `pool_entries().len() + 2` (`crates/delphi-engine/src/engine.rs:720`), `park` sleeps on `park_poll` (`crates/delphi-engine/src/engine.rs:940`), and `blocked_since` preserves cumulative duration across retries (`crates/delphi-engine/src/engine.rs:938`).

REVIEW COMPLETE
````
