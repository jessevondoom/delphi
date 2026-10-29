---
id: spec-048-harness-benchmarking-and-targeted-evals
title: "048: Harness benchmarking and targeted evals: fixed-model arms, efficiency, handoff fidelity, tool-count and context-retention tests (draft)"
author: architect
timestamp: 2026-10-28T18:00:00Z
status: draft   # a proposal, not permission to build (No Spec, No Build). Needs operator approval; open decisions in §8.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the lessons come from the model's recollection of LangChain's published benchmarking (Terminal Bench 2.0 harness work, multi-agent architecture benchmarks, Deep Agents evals); figures were not re-checked. Verify the sources (OD-1) before approval.
source_mission: 2026-10-23-foundational-design
tags: [spec, evaluation, benchmarking, harness, efficiency, handoff, draft]
---

# Spec 048: Harness benchmarking and targeted evals

> **Status: draft.** One of three specs from a review (2026-10-28) of what LangChain and LangSmith publish about agent benchmarking, memory and context. Siblings: `specs/049` (context management) and `specs/050` (memory lifecycle). Delphi uses no LangChain code; this takes the *methods*.

## 1. Why

Published harness work found that most of a coding agent's gain comes from the loop around a fixed model (spec 045 §1 cites 52.8 % to 66.5 % on one benchmark from changing prompts, tools and middleware only). Delphi has the machinery to measure that (spec 037 arms, jury and report; spec 031 §4 `delphi train run`) but the measurements it plans are about *staffing* (which model, which team). They do not yet isolate the **harness** with the model held fixed, report **steps and tool calls** beside accuracy, test **what is lost when work is handed from one seat to the next**, test **how accuracy falls as tools are added**, or test **whether facts survive a compaction**. Spec 045 ships every technique `off` until it is measured (H9); this spec supplies the measuring instruments.

**What exists, so this is not rebuilt:** arms and the nine-task suite (037 §2-3); deterministic scoring, efficiency in tokens and cost per passing solution (037 §5); variance honesty and "no evidence" reporting, dev/holdout split, grader calibration (031 §4.5); harvest of real failures into cases (031 §4.7); the §6 bar for 045 techniques.

## 2. Principles

1. **Hold one thing fixed.** A comparison changes one variable. A harness change is run on the same model, the same cases and the same seeds.
2. **Efficiency is a column, not a footnote.** A gain bought with more tokens, turns or tool calls is reported as a trade, never as a win.
3. **Deterministic scoring first.** Every new eval below scores by string, file or event checks; no model grades its own harness (031 §4.5).
4. **Tokens and counts, not money** (operator rule).
5. **Nothing here widens authority.** Evals run in scratch homes (`--home <tmpdir>`), inside the sandbox; an external benchmark never touches the operator's workspace.

## 3. The additions

| # | Addition | What it is |
| :-- | :-- | :-- |
| BM-1 | **Harness arm** | A new arm kind in 037 §2: same staffing and model as a named base arm, differing only in `[loop]` switches, an overlay (045 T10), a tool set or a prompt section. The report groups by model and shows the delta per harness arm against its own base. |
| BM-2 | **Efficiency columns** | Per run and per case, beside pass rate: turns, tool calls (by tool), tokens in / out / cached, cache hit rate (045 T9), nudges and hard stops, escalations. Per arm: **tokens, turns and tool calls per passing case**. A harness arm whose tokens per pass rise by more than a set share (OD-2) while success gains less than the noise is flagged "costlier, not better". |
| BM-3 | **External anchor** | One public agent benchmark, a small fixed subset, run through headless `delphi run` (spec 016) so a result is not only on cases we wrote. Operator-gated (network, licence, scratch home). Used to check that a harness gain on our suites also shows on a suite we did not write; never used as a training target (no case from it enters `dev`). |
| BM-4 | **Behavior evals** | Many small suites, one per behavior, each case with one pass condition read from events or files: the loop nudge changes the next tool (`loop.nudge` then a different tool); the verify gate catches a seeded defect; a budget warning is followed by a finish; `name_thread` and `record_change` are called when the task calls for them (the live-use gaps noted in the epic-threads and change-log work); a compaction keeps the open task. Lives in `dev/train/suites/behaviors/<behavior>/`. |
| BM-5 | **Handoff fidelity** | Measures what an Orchestrator-to-seat-to-operator relay loses ("paraphrase loss"). A seeded artifact carries N exact items (a hash, a path, a number, a code line, a quoted sentence). A scripted chain passes it through a seat and back. Score: the share of exact items present byte-for-byte in the final answer, and in the relayed text. Run with and without the handle relay of `specs/049` CX-6. |
| BM-6 | **Tool-count degradation** | The same cases with 5, 10, 20 and 40 tool definitions in scope (the extras are plausible distractors from the registry), per model tier, router (041) on and off, progressive tools (045 T8) on and off. Output: a curve of pass rate and tokens against tool count, and a proposed cap per tier. |
| BM-7 | **Context retention** | Seeded facts early in a session or span; a forced compaction (045 T6) or `/new` (030); then probe questions whose answers are exact strings. Score: share of facts recovered, with and without a recoverable-compaction handle (`specs/049` CX-3) and with each digest form (CX-5). Shared fixtures with BM-5. |
| BM-8 | **Phase-effort arm** | The "high effort to plan and verify, normal in between" idea is already 045 T7; this spec only requires it to run as a BM-1 harness arm, so the result lands in the same report. |

## 4. Where each lives

| Addition | Code and files |
| :-- | :-- |
| BM-1, BM-2 | the training harness and report (`delphi train`, the 037 report); arm config under the arm's template home; columns computed from `agent.usage`, `loop.*`, `agent.tool.*` events and the cache accounting of 045 T9 |
| BM-3 | `dev/train/external/<name>/` (a fixed subset manifest and a runner script that calls `delphi run`); operator opt-in; nothing downloaded without the operator's go |
| BM-4 | `dev/train/suites/behaviors/` in the 031 §4.3 case format, with event checks as deterministic check kinds |
| BM-5, BM-7 | a fixture generator (seeded, so cases are reproducible) and a string-survival check kind; scripted model for the relay, real model for the measured arm |
| BM-6 | a case wrapper that pads the tool list from the registry (041) with distractors, and a report section |

## 5. Measuring and reporting

- Every report repeats the 031 §4.5 guards: per-case pass rates over seeds, deltas inside the noise shown as "no evidence".
- A new section, **Harness effect (fixed model)**, lists each harness arm against its base, with the BM-2 columns.
- BM-5, BM-6, BM-7 each produce one chart-ready table (survival share, curve, recovery share) that `delphi docs` charts can draw later (spec 014).
- Results are recorded in the spec tracker with the date, model, seed count and the bar applied. A technique that fails the bar stays `off` (045 §6).

## 6. Acceptance criteria

- **AC-1** A harness arm and its base run on the same model, cases and seeds, and the report shows both with the BM-2 columns.
- **AC-2** A change that raises pass rate but raises tokens per pass beyond the OD-2 share is flagged in the report.
- **AC-3** BM-5 on a scripted chain that paraphrases drops items and scores below 1.0; the same chain with verbatim relay scores 1.0 (the instrument discriminates).
- **AC-4** BM-6 produces a curve for at least two model tiers and states a cap or states "no cap warranted".
- **AC-5** BM-7 on a scripted model that forgets after compaction scores low; with a handle it scores high.
- **AC-6** Each BM-4 case fails against a deliberately broken harness and passes against the working one.
- **AC-7** No eval writes outside its scratch home; the lint and `ui/test/reduce-motion.test.ts` are unaffected.

## 7. Build plan

One slice per pull request. Tracker updated in each.

| Slice | Contents | Depends on |
| :-- | :-- | :-- |
| BM1 | BM-1 harness arm kind and BM-2 efficiency columns in the report; tests | 037 (built), 045 H1 |
| BM2 | BM-5 handoff fidelity: fixture generator, string-survival check, scripted chain, first live run | BM1 |
| BM3 | BM-7 context retention: fixtures, probe check; scripted forgetting model | BM1; live use waits on 045 H3 |
| BM4 | BM-6 tool-count degradation | BM1, 041 S0 for the registry |
| BM5 | BM-4 behavior suites for the 045 H1 behaviors and the epic-thread and change-log tools | BM1 |
| BM6 | BM-3 external anchor: pick the subset (OD-3), runner, first run | BM1, operator go |
| BM7 | BM-8 phase-effort arm, after 045 H4 | 045 H4 |

## 8. Open decisions (provisional answers, change on request)

- **OD-1** Verify the LangChain figures and findings against the published posts before this spec is approved. Provisional: they are treated as hypotheses to test here, not as facts about Delphi.
- **OD-2** Flag threshold for "costlier, not better": tokens per passing case up by more than 25 % with a success gain inside the noise. Provisional; tune after the first run.
- **OD-3** Which external benchmark and subset. Provisional: none chosen; the operator picks, and BM6 is not started until then. Licence and network use are the operator's call.
- **OD-4** Whether the behavior evals (BM-4) run in CI. Provisional: scripted-model versions in CI, real-model versions on demand, because real-model runs cost tokens.

## 9. Non-goals

Adopting LangSmith or sending traces to a hosted service (any exporter needs its own spec: data leaves the sandbox). Tuning to any public leaderboard. A single aggregate score. Declaring a winner from a pilot.

## 10. Known limits

Small suites give wide intervals; the report says so. A scripted model proves the instrument, not the real model's behavior. Handoff fidelity on a real model needs a real chain and costs tokens. The external anchor measures our harness on someone else's tasks and can still mislead if the subset is unrepresentative.
