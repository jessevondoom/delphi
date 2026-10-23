---
id: step-2-audit-round-1
title: "Step 2 Red-Team Sweep, Round 1"
author: red-team
timestamp: 2026-10-24T12:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [audit, step-2, red-team]
---

# Step 2 red-team sweep, round 1

Blind first round, four focus areas. Seats: Gemini 3.8 Flash (security, correctness, model access) and GPT-OSS 120B (security, correctness). The Gemini 4 Argon models and Claude Opus 5.5 were out of quota for this run, and no Claude seat reviewed Claude-authored code (`[H-008]` is satisfied by the other families). Each finding below was traced to the code by the Orchestrator before acting: seats were not trusted, and several reports were wrong.

## Fixed (all with tests)

| # | Sev | Finding | Fix |
| :-- | :-- | :-- | :-- |
| 1 | P0 | A child span's taint never reached the parent that consumed its report, so a clean parent could launder untrusted text into internal-trust writes | `SpanOutcome` carries the child's ceiling; the parent inherits it (also on recovery) |
| 2 | P1 | Live token deltas went to clients before redaction | a streaming redactor holds back the tail that could begin a secret |
| 3 | P1 | Concurrent tasks could apply events to the derived state (budget, ledger, status) out of `seq` order | one gate around persist-then-apply |
| 4 | P1 | Tool results were held in memory until all of a turn's tools finished, so a crash while a later tool awaited approval re-ran a finished side effect | each `tool.result` is persisted as it completes |
| 5 | P1 | A dispatched child that halted was handed to the parent as an ordinary tool error, letting it keep spending | the parent halts and the operator decides (specs/003 §3.2) |
| 6 | P1 | Ratifying a workspace file under `memory/` stamped it in a zone the reader never looked up, so the L5 upgrade never applied | stamp via the same zone mapping the tools use |
| 7 | P1 | Temporary AWS keys (`ASIA…`) were not masked; the key id was not registered | pattern widened; id registered |
| 8 | P1 | Connect failures were treated as interrupted calls: charged the full reservation, no failover | new `Unreachable`: released, next endpoint |
| 9 | P1 | Bedrock/Anthropic throttle error kinds fell through to a hard failure instead of failover | recognised as throttled |
| 10 | P1 | The interleaved-thinking beta was dropped when an endpoint listed any other beta | tested by presence, not emptiness |
| 11 | P1 | Only the first reasoning block of a tool-loop turn was checked for echoability | all reasoning blocks must be echoable |
| 12 | P2 | SSE lines were unbounded; error-body drain had no idle timeout; downgraded reasoning notes were reordered | 8 MiB line cap; idle timeout; notes stay in place |

## Rejected (not defects, or not reachable)

- *Cross-role memory writes allowed* (GPT-OSS security): `never_writable` also denies the whole `memory/` prefix.
- *Replay can over-reserve; unknown schema major accepted; partial batch poisons the log* (GPT-OSS correctness): each requires a corrupted log; the log has a single writer under flock and the daemon refuses to run on a gapped one.
- *Inode-pinned guard bypass, off-by-one turn budget* (GPT-OSS correctness): the guard resolves per call, and the turn check precedes each model call.
- *Daemon exe path / socket permissions / ratify write ordering* (GPT-OSS, marked UNVERIFIED by the seat): the path is chosen by the CLI itself, the socket and run dir are 0600/0700, and ratification is compare-and-swap under the lock.

## Deferred

- `Retry-After` as an HTTP date (P2): the backoff ladder still applies.
- A credential-refresh race that can discard a freshly refreshed cache entry (P1 as reported, harmless in practice: the next call re-resolves): to be fixed with the Step 3 credential work.
- Argon and Claude seats for a second round once quota resets.

# Round 2 (Gemini 3.8 Flash only; the model-access seat was blocked by the provider's content filter)

## Fixed (with tests)
| Sev | Finding | Fix |
| :-- | :-- | :-- |
| P0 | A role without `agent-dispatch` could still dispatch by forging the tool call (the schema was only hidden) | refused at execution against the manifest |
| P0 | The writer lock `missions/<id>/events/.lock` was writable by agents, which would defeat the single-writer guarantee | the whole `missions/<id>/events…` tree is never readable or writable |
| P0 | Streaming redaction let a private-key block through in plaintext before its END line | blocks are held from an open `-----BEGIN` until they end (16 KiB cap) |
| P1 | `lock::write_atomic` followed a symlink planted at its predictable temp name | temp is removed and created exclusively |
| P1 | Rejecting a deleted workspace heuristics or memory file restored it into the git-tracked `team/` tree | restored to its real location |
| P1 | Any file under `missions/` was Green, not only Markdown | non-Markdown writes are Amber |
| P1 | A recorded `task.failed` was re-proposed for resumption | treated as terminal in recovery |
| P2 | `status.md` ignored reservations; the git probe cache never noticed a changed file | both fixed |

## Rejected or deferred
- *Custom agents, skills and REVIEW.md read at L5*: reads of those paths come from their ratified copies, so L5 is right.
- *Agent-drafted specs not listed as ratification proposals*: by design, operator approval of a spec is an edit of its status plus ratification of its hash (Council 004 (b)).
- *`amber_active` never set*: dead flag, not a boundary; removed in Step 3 with the sandbox work.
- *Escalated tasks cannot be resumed after a restart*: escalation hands the decision to the operator; resuming an escalated task is a Step 3 recovery feature.
