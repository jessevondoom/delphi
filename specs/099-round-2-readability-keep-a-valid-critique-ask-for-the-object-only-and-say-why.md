---
id: spec-099-round-2-readability-keep-a-valid-critique-ask-for-the-object-only-and-say-why
title: "099: Round 2 readability: keep a seat's valid critique, ignore Markdown restatements, ask for the object only, repair once, and say why a reply was unreadable"
author: architect
timestamp: 2026-11-10T00:00:00Z
status: approved   # by the operator, 2026-11-10 ("build the round 2 fix first please yes") for the five-point fix the operator was shown; the text as written takes effect when this pull request merges.
approved_by: operator (2026-11-10)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the cause is read from the event logs of three Councils on one machine; the repair turn and the agy structured-output mode are untested
source_mission: 2026-11-10-session
tags: [spec, council, round-2, reader, red-team, agy]
---

# Spec 099: Round 2 readability

> **Status: approved by the operator, 2026-11-10.** Amends [`specs/003`](003-collaboration-and-council.md) §4.2 (the Round 2 reader). Builds on [`specs/089`](089-panel-hardening-from-e2.md) (the Panel's shape check and rejections) and [`specs/086`](086-outside-council-panel-and-host-subagents.md) (briefs and pointers). **Widens no authority**: it changes how a seat's own reply is read, and nothing a seat may do.

## 1. Scope and objectives

A daemon Council escalates when a seat's Round 2 cannot be read. For the red-team seat it has happened every time it was raised lately, and the cause is the reader, not the seat's silence.

**Evidence (Councils `t_0004`, `t_0005`, `t_0006`, 2026-11-10).** In all three, the red-team seat (a Gemini model through the `agy` carrier) returned a valid compact JSON critique first. It then wrote 26 to 38 KB of Markdown prose after it (25,908, 36,727 and 38,092 bytes; 112, 166 and 256 trailing lines). The daemon marked the whole Round 2 `unreadable` (`trailer.conflict = true`), and the Council escalated each time with "Red-Team's Round 2 could not be read". The architect and strategist (Claude) answered with the object only, about 2 KB, and were readable.

Two causes, both in `crates/delphi-engine/src/council.rs` (`critique_read`, `trailer_check`):

1. **A Markdown restatement reads as a conflict (`t_0004`).** A trailing line such as `**Recommendation:** **`PROCEED-WITH-CHANGES`**` starts with `RECOMMENDATION`, so it is read as a recommendation line. Its value, `** **`PROCEED-WITH-CHANGES`**`, does not parse, and an unparseable value counts as a disagreement with the first critique.
2. **A later JSON line that differs counts as a conflicting block (`t_0005`, `t_0006`: 1 and 8 such lines).** The seat re-emitted the object in its prose with slightly different finding titles.

The first critique was valid in all three. The earlier red-team Round 2 of `t_0002` was readable only because its trailer repeated the same block four times.

**What exists, so it is not rebuilt.**

- The reader of spec 003 §4.2: the first complete critique (an object, or the line format up to its first `ROUND 2 COMPLETE`) wins; an identical repeat is tolerated; a later block that differs makes the reply unreadable.
- Spec 089: a Panel's `council_submit` rejects a Round 2 of the wrong shape (two rejections, then the third is accepted and flagged unreadable).
- The `council.critique` event, which carries `trailer` counts (`bytes`, `lines`, `blocks`, `conflict`) and never the text.

### 1.1 Non-goals

- No change to the strictness of the **line-format** reader (§2, R1 point 2).
- No later block is ever merged into a kept critique.
- No new tool and no change to which seats sit.

### 1.2 Simpler alternative considered

Only tell the seat to be shorter. Rejected as the whole answer: the question of `t_0006` said "keep Round 2 under 600 words" and the seat still wrote 38 KB. The instruction stays, as part of R2.

## 2. The changes

### R1. The reader (built in the same pull request as this spec)

1. **Markdown tolerance**, in the first-block reader and in the trailer reader:
   - Emphasis marks (asterisk, backtick, underscore) around a `RECOMMENDATION` or `CONCUR` value are removed before it is parsed.
   - A colon may follow the closing emphasis (`**Recommendation**: PROCEED`).
   - `CONCUR` reads its first word: `false (blocked on 3 defects)` is `false`.
   - A restatement in a trailer whose value still does not parse is **prose**, not a conflict.
   - A restatement that parses and differs from the first critique is still a conflict.
   - A later block that restates the verdict and lists **no findings** contradicts none of the first critique's findings. A block that lists findings must list the same set. (Found while testing R1: without this, every prose restatement of the verdict was a conflict, because it listed no findings.)
2. **A structured reply keeps its first critique.** When a reply **starts with** a schema-valid object (the carrier's structured output) and a later block disagrees, the first critique stands instead of the whole reply being refused. Nothing from the later block is merged. `concur` is forced to `false`. One finding is added: `P1`, "the reply carried a conflicting block after the critique; it was dropped". The seat therefore cannot contribute to unanimity, the P1 appears in the verdict, and `conflict` is recorded in the event trailer.
3. **A line-format reply keeps the 003 §4.2 rule exactly.** With no leading object, a differing later block still refuses the reply.

The reason for the difference: the first object of a structured reply is the carrier's schema output, and what follows is the seat's own free prose, so an attacker cannot place text ahead of it. A line-format reply has no such anchor.

**A cost this accepts.** If a seat emits a draft object and then a different final object, the draft is the one that stands (flagged, no longer concurring), and the final one survives only in the record text. The P1 finding makes the operator look. R2's repair turn is the answer to this: it asks the seat for one object instead of picking between two.

This amends spec 003 §4.2 for structured replies. The amendment takes effect when this pull request merges.

### R2. Ask, and repair once (a separate pull request)

1. The Round 2 objective (`round2_objective` in `council.rs`) says that when the seat answers through the structured-output tool it sends nothing else (no restatement of the verdict, no second copy of the object), and keeps the whole reply under 600 words.
2. Every Round 2 seat is already given the critique schema (`critique_schema()`), so the `agy` carrier is already asked for schema output. The daemon still records any text after the first complete object, and ignores it for the verdict. Nothing changes here; whether the carrier can be made to emit nothing else is not verified (§7).
3. **One repair turn.** When a seat answered but its Round 2 could not be read (no critique, or a line-format conflict), the daemon starts one more span for that seat with a message of **fixed text** (`repair_objective`, which begins `COUNCIL, ROUND 2 REPAIR.`). Nothing the seat wrote is in the message. Its earlier reply is attached beside the Round 1 stances as a fenced context slice (`council/round2-unread/<role>`, capped at 6,000 characters, tier L3), so the seat can reformat it. The message tells the seat to do no new research and use no tools; that is a request, not an enforced limit, because the span runs under the seat's role scope. The turn runs inside the Council timeout and budget, and is skipped when the budget is already spent. A seat that declined or abstained is never asked. If the repaired reply reads, it replaces the first; otherwise the first stays and the seat is unreadable as today. The `council.critique` event carries `repair`: `repaired`, `failed`, or null when no repair was asked.

This follows the 089 Panel pattern (two rejections) but allows one turn, because a daemon Council seat is billed per call.

### R3. Say why (a separate pull request)

1. The `council.critique` event and the ruling card name a **cause** from a closed list: `conflicting_block`, `no_recommendation`, `empty`, `declined`, `abstained`. They show whether the first critique was kept ("kept; conflicting block dropped") or the reply was unreadable, with a pointer to the record section that holds the text.
2. **An excerpt that the cap cut says so.** The cap is in `tools::read_pointer`, which turns each context pointer of a daemon seat's handoff into a slice: 400 lines with no range, a 400-line window for a range, and 256 KiB of bytes read. When a cap cut the text, the slice ends with one fixed line: `[excerpt cut: lines 1 to N of M; read the rest with read_file]`, or, when the byte cap applied, a line that says so in bytes. A range the caller chose that fits inside the window gets no notice. The seats of Council `t_0006` read spec 098 only up to about its section 15.1 (line 400) and never said how much was cut. A Panel's seats read pointers with their own tools (`council_brief` attaches none), so no notice applies there.

## 3. Authority invariants (what the tests prove)

1. The line-format reader refuses a differing later block, as before.
2. No later block is ever merged into the kept critique.
3. A kept-but-conflicted critique has `concur = false` and a `P1` finding, so it cannot produce unanimity.
4. The repair message is fixed text, contains nothing a seat wrote, and counts against the Council budget and timeout.
5. Trailer fields remain counts and lengths, never text.
6. The cause names in the event are from a closed list.

## 4. Slices (one pull request each)

| Slice | Content | Hours | State |
| :-- | :-- | --: | :-- |
| S0 | this spec | 4-6 | approved when its pull request merges (the same pull request carries R1) |
| R1 | the reader: Markdown tolerance and the kept first critique | 6-10 | built in this pull request |
| R2 | the object-only objective and the one repair turn (the `agy` structured request already exists) | 8-12 | built (stacked pull request) |
| R3 | the cause in the event and on the card; the cut-excerpt notice | 6-10 | built (pull request open) |

## 5. Acceptance criteria

Fixtures have the shape of the three real replies and generic synthetic text.

1. **Restatements read.** An object followed by prose that holds `**Recommendation:** **`PROCEED-WITH-CHANGES`**`, `**Concur:** **`false`** (blocked on 3 defects)` and `**Concurrence:** false` reads as that object, with `conflict = false`.
2. **A later differing object is kept-first.** An object followed by a later JSON line with different findings keeps the first critique, `concur = false`, one `P1` conflict finding, and `conflict = true`.
3. **A differing restatement is flagged.** An object followed by a Markdown restatement of `DO-NOT-PROCEED` is a flagged conflict, with the first critique kept.
4. **The line format is unchanged.** A line-format block followed by a differing block is refused exactly as before.
5. **An unparseable restatement is prose.** `Recommendation: see above` after an object changes nothing.
6. **The existing reader tests that guard the line format still pass.**
7. **R2:** a fake carrier that returns prose and then the object triggers the repair turn once, the second reply is read, the message is the fixed text, and the budget counts it.
8. **R3:** the event carries the cause and the card shows it; an excerpt cut by the cap says so.

## 6. Verification Plan

| Item | How it is verified |
| :-- | :-- |
| R1 | Unit tests in `crates/delphi-engine/src/council.rs` over the fixtures of section 5, items 1 to 6, run with `cargo test -p delphi-engine council::`. The existing strict-reader tests are kept for the line format and rewritten only for structured replies. |
| R2 | An engine test with a fake carrier (prose, then the object): the repair turn fires once, its text is asserted to be the fixed text, and the budget and timeout are charged. |
| R3 | An event test for each cause name; a ruling-card golden; a brief test that a cut excerpt carries the "lines 1 to N of M" notice. |
| Gates | `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `delphi lint --repo .`, and `delphi check spec` on this file. |

No slice here is verified by a live run. The three Councils above are the evidence, not the test.

## 7. Decisions and what is not verified

- **Decided by the operator, 2026-11-10:** build the Round 2 fix first.
- **Not verified:** whether the `agy` carrier's structured-output mode can suppress trailing prose (R2's tests use a fake model, not the carrier); whether one repair turn is enough; the token cost of the repair turn, which is a second full span for that seat (the red-team seat made 33 tool calls in one Round 2), so a repair is expensive until the "no tools" request is enforced or the span is resumed instead of restarted.
