---
id: spec-128-round-2-without-tools-one-retry-for-an-empty-finish-and-a-guard-that-knows-delphis-own-writes
title: "128: Round 2 without tools, one retry for an empty finish, and a checkout guard that knows Delphi's own writes"
author: architect
timestamp: 2026-11-19T00:00:00Z
status: draft   # a proposal, not permission: nothing is built until the operator approves it (No Spec, No Build)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the three causes are read from the code and the mission log of one session (host-d09f24619bf5bc14, 2026-11-19); the exact bytes the CLI printed in the failed Round 2 were never captured, so cause 1's effect on that one run is inferred (section 7)
source_mission: 2026-11-19-red-team-round-2
tags: [spec, council, red-team, agy, round-2, guard, error-log]
---

# Spec 128: Round 2 without tools, one retry for an empty finish, and a checkout guard that knows Delphi's own writes

> **Status: draft.** Nothing here is built. Section 7 lists what is unverified; section 8 lists the decisions the operator owns (one of them, OD-1, is the safety-guard change that [`specs/089`](089-panel-hardening-from-e2.md) OD-4 reserved for the operator).
> **Amends, if approved:** [`specs/086`](086-outside-council-panel-and-host-subagents.md) §4.3 (the no-tools rule for a seat that reads peer text now holds for the daemon Council too, not only the Panel's foreign seat), [`specs/099`](099-round-2-readability-keep-a-valid-critique-ask-for-the-object-only-and-say-why.md) R2 item 3 ("no tools" becomes enforced, not asked), [`specs/020`](020-harness-tool-bridge.md) §8 (the checkout guard's ignore rule) and takes the decision [`specs/089`](089-panel-hardening-from-e2.md) OD-4 left open.

---

## 1. Scope and objectives

A host Council kept losing the red-team's Round 2: the ruling said "Red-Team's Round 2 could not be read" and escalated. Spec 099 fixed one reason (a valid critique followed by prose). This is another reason, and a second one that removed the red-team from Round 1.

**Evidence (session `host-d09f24619bf5bc14`, Councils `t_0002` and `t_0003`, 2026-11-19 evening, Delphi 0.6.4; the red-team seat is `agy` on `gemini-3.8-flash-medium` in both).**

| Council | Seat | What the log shows |
| :-- | :-- | :-- |
| `t_0003` | `sp_red_team_10`, Round 2 | Ran 11 seconds. 74,574 tokens in, 1,498 out. Two `list_dir` calls (`missions/`, `specs/`). Ended with no schema answer and no words: "the CLI finished without the schema-validated answer that was asked for". Its `harness.started` event offers the whole bridge (31 tools, none denied) and `search_web` and `read_url_content`. It was not retried. It abstained, so the record says "no Round 2" and the decision escalated. |
| `t_0002` | `sp_red_team_3`, Round 1 | Finished, then was discarded: "the agy run changed a project checkout ... research/topics/precedent-for-operator-directed-teaching-.../answer.md, search-log.json". A `request_research` run had written exactly those files into the checkout six seconds before. |

**Three causes, all in `crates/delphi-engine/`.**

1. **A seat that reads peer words is not toolless in a daemon Council.** Spec 086 §4.3 says a seat given peer stances has no tools, and `panel_foreign_run_seat` (`council.rs`) enforces it by marking the handoff `toolless`. The daemon Council's own Round 2 spawn and its repair turn (spec 099 R2) call `run_seat` directly, with no mark. The seat gets the full bridge and its carrier's native tools, so it wanders instead of answering. Spec 099 §7 already records the cost: "the red-team seat made 33 tool calls in one Round 2".
2. **An empty finish cannot recover.** The one-retry rule (`engine.rs`, spec 089 P4b) fires only when the CLI itself reported an error (`cli_error`). A CLI that finishes with success, a schema asked for, and no answer and no words is `FailKind::AnswerNotFound`, which has `cli_error = false`. It is never retried. The repair turn of spec 099 R2 only covers a seat that answered; a seat that returned nothing abstains and is never asked again.
3. **The checkout guard cannot tell Delphi's own write from the seat's.** `harness_run.rs` discards an `agy` seat when `git status` of any registered project differs before and after (spec 020 §8). Delphi's own research writer (`research_record::write_atomic`) writes into the project checkout. When a research run lands during a seat's run, the seat is blamed. Spec 089 F4 (H-1) found the same class of false positive, and its fix (P4b's guard narrowing) waited for the operator's go (OD-4) and was not built.

**And one reason the diagnosis was slow.** A failed harness run is meant to be logged with the CLI's stderr tail (`harness_failed`, spec 039). The code gates it with `elog::enabled(Warn)`. The log level defaults to `error`, so at the default level the check is false and the error is never written. For both failures above the log holds only the one-line `council_seat_failed`, with no stderr and no final event. Section 2, S2 fixes it.

### 1.1 Non-goals

- No change to which seats sit, to the critique schema, or to the reader of spec 099 R1.
- No loosening of the guard for anything the seat could have written. A real change still discards the run (section 3, invariant 4).
- No change to Round 1: a Round 1 seat keeps its tools. Spec 086 §4.3 says a seat that needs tools to check something does it before it sees any peer.
- No second repair turn, and no retry of a timeout, a spent quota, a guard failure or a second failure.

### 1.2 Simpler alternative considered

Change nothing in the engine and only tell the red-team seat, in the Round 2 objective, to answer without tools and to send the object. Rejected as the whole answer: spec 099 §1.2 found that an instruction alone is not obeyed ("under 600 words" produced 38 KB), and this seat is in the very class that ignored it. The words stay (they are already in `round2_objective`); the span now enforces them. The cheapest part of this spec is S1, a few lines that reuse the Panel's existing `toolless` mark.

## 2. The changes

### S1. A seat that reads peer words has no tools (daemon Council)

1. In the daemon Council, the Round 2 seats and the Round 2 repair seats run with the handoff marked `toolless`, exactly as `panel_foreign_run_seat` does: the mark is inserted before `run_seat` and removed after, on every path out.
2. The effect is the one spec 086 §4.3 already describes for the Panel: the `agy` gate allows nothing and the bridge is empty, a Claude seat keeps only its structured-output tool, and the seat's note tells it to answer in plain text in the asked shape. The seat's receipt lists no tools.
3. The synthesis seat (the orchestrator) is not changed here: it already receives the Round 2 words as fenced data, and whether it should be toolless is OD-2.
4. The pointers of a handoff are already turned into context slices by the daemon (`tools::read_pointer`, spec 099 R3), so a Round 2 seat does not need a tool to read what the question pointed at.

This makes spec 099 R2 item 3's "do no new research and use no tools" a limit the span enforces, not a request.

### S2. One retry for an empty finish, and the evidence

1. The retry condition of `engine.rs` (spec 089 P4b) also holds for `FailKind::AnswerNotFound` when the run used no tool that acts and time remains (`RETRY_MIN_REMAINING`, the seat's own deadline). It is still one retry, in a fresh process, with the first run's usage logged. A second `AnswerNotFound` is final and the seat abstains as today.
2. **The log tells the truth.** The `harness_failed` record is written at the level of the event: `error` for a failure, `warn` only for a spent quota. The check that gates it uses that same level, not `warn`. The record carries what it carried (the reason, the exit code, the redacted stderr tail).
3. **A new event, `harness.no_answer`,** is appended when a run ends as `AnswerNotFound`. It holds counts and names only, never text: the result's `status` string, the number of tool calls and their names, the number of `agent_response` steps with and without text, `output_tokens`, whether `structured_output` was present, and the exit code. The next occurrence then says whether the model ended its turn, called a blocked tool or sent its answer somewhere the parser does not read.

### S3. The checkout guard ignores what Delphi wrote itself

1. **A ledger of Delphi's own writes.** `research_record::write_atomic` (the one writer of `research/topics/<id>/` files) records each path it writes in a process-wide ledger: the absolute path, the time and the blake3 of the bytes. The ledger keeps the last 4,096 entries and nothing older than one hour. Only Delphi's own writer can add to it; no tool a seat is offered does.
2. **The guard subtracts the ledger.** After an `agy` run, a path that appears or disappears between the two `git status` snapshots is left out of the comparison when the ledger holds that path with a time at or after the seat's start **and** the file's current blake3 equals the ledger's. A path edited again after Delphi wrote it, or one the ledger does not hold, counts.
3. The event `harness.checkout_guard` gains `own_writes_count` beside `ignored_count`, and its note says so ("ignored 2 paths Delphi wrote itself"). If those are the only differences the run is kept, as for `.claude/worktrees/`.
4. `.claude/worktrees/` stays ignored as spec 020 §8 says. No other prefix is added.

## 3. Authority invariants (what the tests prove)

1. A Round 2 seat and a Round 2 repair seat of a daemon Council run toolless; no tool is offered to them and the carrier's gate allows nothing.
2. A Round 1 seat is not made toolless by this spec.
3. The retry of an empty finish happens once, only when no acting tool was used and time remains, and never after a timeout, a quota error, a guard failure or a second failure.
4. A guard difference that is not in the ledger, or whose file no longer has the ledger's hash, still discards the run. A seat that writes into the checkout by any tool is still caught.
5. The ledger is written only by Delphi's own research writer, never from a seat's input.
6. `harness.no_answer` and the extended guard event hold counts, names and paths, never contents (spec 039 §3).
7. Nothing here raises a span's trust, approves an action or reaches outside the sandbox (AGENTS.md, "Do not widen authority").

## 4. Slices (one pull request; same-spec slices, each its own commit, own trailer and own tracker row)

| Slice | Content | Files (expected) | Lines (est.) |
| :-- | :-- | :-- | --: |
| S0 | this spec, its compile and its tracker entry | `specs/`, `compiled/`, tracker | (docs) |
| S1 | toolless Round 2 and repair seats | `council.rs` | 60-120 |
| S2 | retry of an empty finish; `harness_failed` at its own level; `harness.no_answer` | `engine.rs`, `harness_run.rs`, `schemas/` (event) | 150-250 |
| S3 | the own-writes ledger and the guard's subtraction | `research_record.rs`, new `own_writes.rs`, `harness_run.rs` | 150-250 |

Together about 400-600 lines of code and tests, inside the 800-line limit of the same-spec bundling trial in `AGENTS.md`. The pull request merges with a merge commit. The slices touch no `.github/`, charter or rubric path. If the operator wants the guard change reviewed apart (OD-1), S3 moves to its own pull request and nothing else changes.

## 5. Acceptance criteria

1. **Toolless Round 2.** A daemon Council whose seat runs on a fake `agy` shows, for Round 2 and for the repair turn, `harness.started.gated = true`, an empty `tools_offered` in the receipt, and a handoff in the toolless set while it runs and absent after. Round 1 seats are unchanged.
2. **A Claude seat still answers.** A toolless Round 2 Claude seat with a schema returns its object through the structured-output tool.
3. **Empty finish retried once.** A fake `agy` that finishes with success, no words and no `structured_output` on its first run and a valid object on its second yields a readable Round 2, one `harness.retry` event with `attempt = 2`, and both runs' usage in the log. If both runs are empty the seat abstains with the same reason as today and exactly one retry ran. With under `RETRY_MIN_REMAINING` left, no retry runs.
4. **No retry where it must not.** A timeout, a quota error, a guard failure and a run that used an acting tool are not retried.
5. **The error is logged at the default level.** With `DELPHI_LOG` unset (level `error`), a failed harness run writes one `harness_failed` record with the stderr tail; a spent quota writes a `warn` and is absent at `error`.
6. **`harness.no_answer`** is appended for an empty finish with the counts above and no text, and a golden for its payload.
7. **Own writes are ignored.** A fake `agy` that takes two seconds, while the test calls `research_record`'s writer for a topic in the registered project, is kept, with `own_writes_count = 2` in `harness.checkout_guard`.
8. **A real change still fails closed.** The existing `an_agy_run_that_touches_a_project_checkout_is_discarded` passes unchanged. A new test: the test writes a file the ledger does not hold, and the run is discarded. A second: the writer writes a file, then the test edits it before the run ends, and the run is discarded.
9. **The characterization test of spec 089 P4a** (`characterization_an_unrelated_write_to_a_registered_checkout_during_an_agy_run_discards_a_good_answer`) keeps passing for an unrelated write, and a companion shows the same run with a ledger write is kept.
10. The repair message and the Round 2 objective are unchanged byte for byte (spec 099 §3, invariant 4).

## 6. Verification Plan

| Item | How it is verified |
| :-- | :-- |
| S1 | Engine tests in `council.rs` over a fake CLI (as `a_toolless_agy_seat_has_the_gate_allowing_nothing_and_its_receipt_lists_no_tools`): receipts, the toolless set before, during and after, and for the repair turn. |
| S2 | Engine tests with a fake `agy` script (first run empty, second valid; both empty; short deadline), an elog test at the default level, and a payload golden for `harness.no_answer`. `schemas/` gets the event schema; `cargo test -p delphi-core` event-schema checks must pass. |
| S3 | Tests beside the existing guard tests in `harness_run.rs`; a ledger unit test (hash match, edit after write, age and size bounds). |
| Gates | `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo run -q --locked -p delphi -- lint --repo .`, `delphi specs compile`, and `delphi check spec` on this file. |

No slice is verified by a live Council. One live Council with the red-team on `agy` is the closing check (section 7, item 2); it is the operator's go because it spends the operator's tokens.

## 7. What is not verified

1. **The exact output of the failed Round 2.** The daemon did not keep it (see the logging bug in section 1). Whether the seat ended its turn after its tool calls, called a blocked tool such as `finish`, or sent text the parser does not read is inferred from the counts: tools were used, 1,498 tokens were spent, no words arrived. S1 removes the tools that invited the wandering; S2 captures the next case. It is possible that S1 alone does not cure the empty finish, and S2's retry and event are there for that case.
2. **That a toolless Flash seat returns a schema answer.** The Panel's foreign seat has run this way since spec 086 §4.3, and a toolless `agy` seat with a text answer is tested (`a_toolless_agy_seat_...`), but a schema-validated answer from a toolless `agy` Round 2 has not been seen live.
3. **That the research writer is the only Delphi writer into a checkout during a seat's run.** The ledger covers `write_atomic`. A second Delphi writer would trip the guard as before, which fails closed; S3 does not look for one.
4. **Whether the Round 1 red-team still fails in other ways.** Spec 089 E-6 counted two other causes ("the CLI reported an error" after a hang); this spec does not touch them.

## 8. Decisions for the operator

- **OD-1 (the guard; the one that matters).** May S3 narrow the `agy` checkout guard by Delphi's own writes (the ledger with a hash check), as spec 089 OD-4 asked? *Recommendation:* yes. It is the narrowest rule that fixes the false positive and keeps every seat-made write fatal. *Alternative:* ignore the `research/topics/` prefix like `.claude/worktrees/`. Simpler, but a seat that writes there with a native tool the gate failed to deny would go unseen. *Not taken:* turning the guard off for Council seats.
- **OD-2 (the synthesis seat).** Should the orchestrator's synthesis seat also run toolless, since it reads every seat's Round 2 words? *Recommendation:* not in this pull request; it is a different seat with a different job, and nothing failed there.
- **OD-3 (cost of S1).** A red-team seat can no longer check a claim with a tool in Round 2. *Recommendation:* accept it, as spec 086 §4.3 does for the Panel; a verification belongs in Round 1.
- **OD-4 (one pull request).** One pull request with the spec first, your approval commit, then S1 to S3 as three commits. *Recommendation:* yes. If you prefer, S3 alone becomes a second pull request (section 4).

## 9. Options considered

1. **Only retry (S2) and leave tools on.** Cheapest, and the retry may land. Rejected as the whole answer: a seat that wanders spends tokens and may wander again, and spec 086 §4.3 already decided the rule.
2. **Only toolless (S1).** Removes the cause we can see. Rejected as the whole answer: it cannot be proven to cure an empty finish (section 7, item 1), and the next one would leave no evidence.
3. **Ignore the `research/topics/` prefix in the guard.** Simple; weakens a safety guard for a whole directory. Kept as OD-1's alternative.
4. **Move Delphi's research writes out of the project checkout.** Removes the cause at its root. Rejected here: the project-owned research store lives in the project by design (spec 058 §13 (A1): project-scope research is written under the project and visible there), and relocating it is a larger change than this fix.
5. **Raise the seat's timeout or switch the red-team to another family.** Neither is the cause; the first run ended in 11 seconds.
6. **The chosen set:** S1, S2 and S3 in one pull request, so the red-team's Round 1 and Round 2 both survive the next Council.
