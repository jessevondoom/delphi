---
id: spec-045-harness-loop-techniques
title: "045: Harness loop techniques: nudges, gates, budgets and context hygiene"
author: architect
timestamp: 2026-10-28T12:00:00Z
status: approved   # by the operator, 2026-10-28: "make a plan, write a spec, and let's go" (build most or all of §3, fast list first)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # techniques are from published harness work, not yet measured in Delphi; every slice ships behind a switch until §6 says otherwise
source_mission: 2026-10-23-foundational-design
tags: [spec, harness, engine, context, verification, budgets]
---

# 045: Harness loop techniques

**Status:** approved by the operator (2026-10-28). **Builds on:** `specs/002` (adapter, cache blocks), `specs/020`/`021` (CLI seats and the tool bridge), `specs/033` (loop alerts), `specs/037` (experiments), `specs/038` (capabilities), `specs/041` (registry).
**Code today:** the API span loop is `Engine::drive` in `crates/delphi-engine/src/engine.rs`; a CLI seat's tool calls pass through `crates/delphi-engine/src/harness_run.rs` and `bridge.rs`; loop alerts are `observe::extend_streak`.

## 1. Why

A research pass (2026-10-28) over published harness work found that most of the gain in a coding agent comes from the loop around the
model, not the model: LangChain moved a fixed model from 52.8 % to 66.5 % on Terminal Bench 2.0 by changing only prompts, tools and
middleware. Delphi already has the hard parts (cache-stable prefixes, a sandbox, approvals, trace-driven training). It lacks the
cheap, mechanical ones, listed here. Each is deterministic code the engine runs around a model call; none adds a model call except
the verification gate (one extra turn).

**What exists, so this is not rebuilt:** prompt-cache breakpoints (002 §3); a turn cap per role (`max_turn_budget`); a *loop alert*
(`observe::extend_streak`: 6 calls of one tool or 3 failures in a row raise `monitor.alert` once). That alert goes to the operator's
window only; **the agent is never told**. Slice H1 closes that.

## 2. Principles

1. **Additive, never widening.** A technique may add text, a turn, or a refusal to continue; it may not add a tool, raise a trust
   tier, approve an action, or change what a role may touch. Every injected note is written by Delphi (never copied from
   outside content) and is labelled `[delphi]` so a model and a reader can tell it from tool output.
2. **Tokens and counts, not money** (operator rule). Thresholds are turns, tokens or percentages of a cap.
3. **Deterministic work in code.** Detection, counting, clearing and bootstrapping are compiled; only the verification pass asks
   the model, and only once.
4. **Off until measured.** Each slice has a switch under `[loop]` (§5). A slice ships `on` only after the §6 experiment shows no
   loss in quality and a gain in at least one of success, tokens or turns. H1 ships `on` for roles with write or execute access
   because its worst case is one extra turn; the rest ship `off`.
5. **Seats differ.** An API span is driven by Delphi, so every technique applies. A CLI seat (claude, agy) drives its own loop;
   Delphi can act only on what passes through the bridge (tool results) and on the final answer. §4 says what applies where.
6. **Every action is an event.** Each technique writes a `loop.*` event (kind, span, counts, never content), so `delphi observe`,
   the error log and the §6 experiments can count them.

## 3. The techniques

| # | Technique | What the engine does | API span | CLI seat |
| :-- | :-- | :-- | :-- | :-- |
| T1 | **Loop nudge** | When the streak alert fires, or one file is written N times (default 4), append `[delphi] You have called X n times / edited F n times. Stop and change approach: re-read the objective, say what you have learned, try something different.` to that tool result. A nudge that is not answered becomes a hard stop: the same tool called another 6 times (or a fresh alert), or 2 × N writes to one file, escalates the span to the operator, because a nudge did not work. Calling a different tool clears the nudge. | yes | yes (bridge tools only; a CLI's own tools are seen as `harness.tool`, counted, not nudged) |
| T2 | **Budget warnings** | At 75 % and 90 % of `max_turn_budget` (and of the span's token share when one is set), append `[delphi] 6 of 25 turns left. Finish the work, then verify it.` to the next tool result, once per threshold. | yes | bridge results only |
| T3 | **Verification gate** | When a role that writes or executes ends its turn with a final answer, and has not been gated in this span, the engine injects one user turn: the objective, the constraints and expected output from the handoff, and "check your work against each before finishing; run the checks you were given". The model's next answer is final. Counts as a turn. Skipped when the span used under 3 tool calls (nothing to verify), is `toolless`, is a Council seat, or has fewer than 2 turns left. | yes | not in H1 (a CLI's last message is its answer; a stop hook is slice H6, behind a probe) |
| T4 | **Workspace bootstrap** | Before turn 1, add a slice `@runtime/workspace`: the worktree's top two levels (bounded to 4 KB), the operator's `[exec]` test and check commands, and which of `git`, `cargo`, `node`, `npm`, `python3`, `make` are on the path (a lookup, no process is run). File names are reduced to plain characters, since a repository's names are not ours. Computed once per span, in code; only for a role that has a worktree. | yes | yes (placed in the first prompt) |
| T5 | **Tool-result hygiene** | Results over a size limit are stored in the payload store and the history holds a handle (`@payload/<id>`, readable with `read_file`), as connector results already are. Past a threshold of history size, old tool results are replaced by a placeholder (`[result cleared: 3.2 KB, @payload/…]`) in batches, keeping the last N, so the cached prefix is invalidated rarely and never mid-batch. | yes | no (the CLI manages its own context) |
| T6 | **Compaction with a flush** | At a high-water mark of the window (default 70 %), run a silent turn that asks the role to write its state to its mission notes, then replace the oldest turns with a summary that keeps the goal, done work, open work, decisions and the next step. The summary's source turns stay in the event log; the persisted block is `replayable: false` (002 §3). A compaction is checked: the summary must mention every open task and every file written (a deterministic check against the log); on a miss the compaction is not applied. | yes | no |
| T7 | **Phase reasoning** | Thinking budget by phase of the span: high while no write has happened (planning), the role's normal level while writing, high again for the T3 verification turn. Never above the role's manifest ceiling. | yes | no (flags are fixed at launch) |
| T8 | **Progressive tools** | Tool definitions stay byte-identical across turns (cache). A role with many tools or bundle skills gets a short list plus a `find_tool` that returns definitions for the rest; a returned tool is added at the end of the list and stays. | yes | no |
| T9 | **Cache accounting** | Per span: cache read / write tokens, hit rate, the turn where the prefix changed and why (tool set, a cleared batch, a compaction). Shown in `delphi observe` and the span report; a subagent's cold first call is counted separately. | yes | from usage when reported |
| T10 | **Family overlays** | A role may carry a short guidance overlay per model family (`agents/<role>/overlays/<family>.md`), added to block 4 only when the span runs on that family. Written by the operator or earned by §6 experiments, never by the model itself. | yes | yes |
| T11 | **Trace review** | A `/retro`-style skill reads a mission's events, runs one analysis per failed or looping span in parallel, and proposes harness changes as a report for the operator. Proposes only. | n/a | n/a |

**Not adopted, and why:** tool-call repair (no solid source; the schema gate in 020 §5 already rejects malformed calls with a
message the model can read); a hard time limit (agents estimate time badly, and the operator asked for token limits, T2).

## 4. Where each lives

- A new module `crates/delphi-engine/src/loopctl.rs` holds the pure parts: counters (`EditCounts`, `Streak` re-export), the
  thresholds, `nudge_for(&SpanLoop, tool, args) -> Option<String>`, `budget_note(turns, cap, tokens) -> Option<String>`,
  `needs_gate(...)`, the gate text, and the bootstrap renderer. No I/O except T4's directory listing and `--version` probes, which
  run through `ExecEnv` with the span's policy.
- Events: `loop.action` (`action` is `nudge`, `stop` or `budget`; tool, counts and the engine's own sentence, never tool output) and `loop.gate` (the verification turn's text, which recovery replays).
- `Engine::drive` calls `loopctl` at three points: after a tool result is built (T1, T2, appended to `r.content` before it is
  persisted, so a resumed span replays the same text), at the end of a turn that would break the loop (T3), and when slices are
  assembled (T4). `harness_run.rs` calls the first two for bridge results.
- A nudge is part of the persisted tool result, so recovery and replay never re-run it. The gate's injected turn is persisted as
  a user turn tagged `loop.gate` so a resumed span knows it was gated.
- T5, T6, T8 change how `history` becomes a request, and sit beside `model_turn`, not in the loop.

## 5. Configuration

```toml
[loop]                       # not an access table (`lock.rs` ACCESS_KEYS): it changes only what the engine adds to a run
nudge = true                 # T1
nudge_edits = 4              # writes to one file before the nudge
budget_warnings = true       # T2 (at 75 % and 90 %)
verify_gate = true           # T3, roles with write or execute only
workspace_bootstrap = true   # T4
tool_result_limit_kb = 0     # T5 offload; 0 = off
clear_after_kb = 0           # T5 clearing; 0 = off
compact_at_percent = 0       # T6; 0 = off
phase_reasoning = false      # T7
```

Keys are added to `crates/delphi/config.example.toml`, `crates/delphi-adapter/src/config.rs`, `schemas/config.schema.json` and
`docs/reference/config.md` in the slice that adds them. A missing table means the defaults above.

## 6. Proving it

Each slice is measured with the spec 037 harness before its default moves, with the technique as the arm and the same seed set:

- **Metrics:** task success against the case's checks, turns used, tokens used (input, output, cached), number of `loop.*`
  events, number of escalations.
- **Bar:** no drop in success beyond the run-to-run spread; a gain in at least one of success, turns or tokens. A slice that
  fails the bar stays `off` and the result is recorded in the tracker.
- **Unit tests** cover the pure functions (thresholds fire once, the gate skips the cases in T3, a compaction that drops an open
  task is refused). **Engine tests** drive a scripted model through a loop, a long run and a premature "done", and assert the
  events and the persisted history. Reduce-motion and the lint are unaffected (no console change in H1).

## 7. Build plan

One slice per pull request, in this order. H1 is the fast list.

| Slice | Contents | Depends on |
| :-- | :-- | :-- |
| **H1** | T1 loop nudge (with hard stop), T2 budget warnings, T3 verification gate (API spans), T4 workspace bootstrap; `[loop]` table; `loop.*` events; tests; docs | none |
| H2 | T5 offload and clearing, with cache-aware batching; T9 cache accounting | H1 |
| H3 | T6 compaction with flush and its deterministic check | H2 |
| H4 | T7 phase reasoning, as a 037 experiment arm first, default `off` | H1, 037 |
| H5 | T8 progressive tools (`find_tool`), stable definitions | 041 S0, 038 |
| H6 | T3 for CLI seats: a stop hook for claude and agy if a probe shows the CLI honours one (`delphi endpoints test --probe`); otherwise recorded as not possible | H1, 020 |
| H7 | T10 family overlays, with the first overlay earned from a 037 run | 041 `[extends]`, 037 |
| H8 | T11 trace review skill | 031 L2 |
| H9 | Measure H1–H5 on the 037 suites; set defaults per §6 | all above |

## 8. Open decisions (provisional answers, change on request)

- **OD-1** Gate threshold: gate only spans with 3 or more tool calls (answer given). Lower it if short spans turn out to stop early.
- **OD-2** Hard stop after one ignored nudge: 6 more calls of the same tool, or twice the edit count on a file (answer given). Alternative: nudge twice; it costs a few turns on a stuck span.
- **OD-3** Defaults: H1 `on`, H2 to H7 `off` until H9 (answer given).
- **OD-4** Whether the gate also runs for the Builder's merge gates: no, those are already deterministic (`checks.rs`, `[exec]` gates).

## 9. Non-goals

Anything that changes what an agent is allowed to do; a model-written router or judge in the loop; money-denominated limits;
changing a CLI seat's own context handling; a new console view (the events appear in existing `observe` views).
