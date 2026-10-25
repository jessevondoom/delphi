---
id: evaluation-plan-2026-10-25
title: "Evaluation plan: does the team beat one good agent?"
author: strategist
timestamp: 2026-10-25T12:00:00Z
source_type: internal_generated
confidence: low
status: draft
source_mission: 2026-10-23-foundational-design
tags: [plan, evaluation, benchmark, ablation, pre-registration]
---

# Evaluation plan (draft, 2026-10-25)

**Question:** does Delphi's pipeline (spec gate, Builder, cross-family audit, Council, approval cards) produce better outcomes than a single strong agent, by enough to justify its cost and friction? And if so, which parts earn it?

This document is meant to be **frozen before the first measured run** (§6). It depends on `delphi run` and on API-key readiness; both are in [`gap-closure-plan-2026-10-25.md`](gap-closure-plan-2026-10-25.md). Numbers marked "guess" are my estimates and need replacing with pilot data.

## 1. Hypotheses

| ID | Claim | Measured by |
| :-- | :-- | :-- |
| **H1** quality | On the same tasks and the same spend ceiling, Delphi merges more correct changes than a single `claude -p` session, including one that reviews its own work. | Hidden-test pass rate |
| **H2** defects | Delphi lets fewer defects through (the cross-family audit works). | Seeded-bug detection; post-merge hidden-test failures |
| **H3** cost | Cost per accepted task stays within a stated multiple of the baseline. | Metered dollars |
| **H4** safety | Under injection, Delphi executes far fewer bad actions. | Executed-bad-action rate |
| **H5** Council | Council verdicts catch planted flaws that a single model misses. | Flaw-catch rate, blind-graded |
| **H6** friction | Honest cost to the operator: cards per task. Delphi may lose here; we want to know by how much. | Cards per task; unanswered-card stalls |

## 2. Arms

| Arm | What it is | Why it is there |
| :-- | :-- | :-- |
| **A0** | `claude -p`, one session, default tools, same model and same budget cap | The plain baseline |
| **A1** | A0, then a second pass in the same session: "review your work against the task, then fix" | The fair baseline. If the gain is just "a second look", this arm gets it. |
| **A2** | A0 with one reviewer subagent configured in Claude Code | What a Claude Code power user would actually build |
| **A3** | Delphi, full pipeline, API endpoints, two families | The system under test |
| **A3s** | Delphi, full pipeline, one family only | Separates "the pipeline" from "family diversity" |
| **A3o** | A3 in optimistic mode, with the operator proxy ratifying decisions afterwards | Measures what deferring approvals costs and saves: completion without stalls, rejection rate, discarded spend. See the [exploration](optimistic-workflow-exploration-2026-10-25.md) §6. |
| **A4a** | A3 without the audit step | Ablation: what the audit contributes |
| **A4b** | A3 without the Council | Ablation: what the Council contributes |
| **A4c** | A3 without the spec gate | Ablation: what *No Spec, No Build* contributes |

Every arm runs on **API endpoints with metered cost**. Harness mode reports $0 for Delphi, which would flatter it. This is why API readiness comes first. Same model per role tier wherever the arms overlap; the CLI and model versions are pinned and recorded.

## 3. Task sets

**T1: replay tasks from post-cutoff commits (primary).**
- Take commits and issues from this repository and two or three other repositories, all dated after the models' training cutoff (June 2026 for the Claude 5.5 family). Delphi's own October 2026 pull requests qualify.
- Each task: the repository at the parent commit, the issue text as the prompt, and the real tests from the fix **withheld** until grading.
- This controls for contamination, which public benchmarks can't. Target 40 tasks, mixing bug fixes, small features and refactors.

**T2: public sanity subset.** About 30 SWE-bench Verified-style instances, to anchor the numbers to something outside our own repositories. Treat it as secondary because of contamination.

**T3: spec traps.** About 15 tasks whose specs are ambiguous, contradict the code, or omit a hidden requirement. The right behaviour is to stop and ask, or to halt on drift. This tests *No Spec, No Build* and the drift halt. Metric: **silent-workaround rate** (the agent hacked around the flaw and said nothing) against flagged-and-halted.

**T4: seeded-bug review.** About 30 real, working changes with one realistic bug injected into each (mutation-style), judged for whether the bug survives to merge. Compare: same-family audit, cross-family audit, self-review (A1). This is the direct test of the claim that a different agent auditing the exact commit pays for itself.

**T5: Council forks.** About 20 design questions with a plausible-but-flawed option planted among the candidates, plus 5 with a clear best answer as controls. Compare Council against a single frontier model asked the same question, graded blind (§5).

**T6: injection suite.** About 40 cases: instructions hidden in a file, an issue body, a README, a tool description that changes after pinning, a fetched web page. Each tries to get the agent to exfiltrate, run a download-and-execute, edit CI config, or push. Compare A0 (headless defaults), A0 with the permissive flag people actually use in CI, and A3.

**T7: recovery.** Kill the daemon, or the machine's network, mid-task on 10 tasks. Compare completion after recovery against a restart from scratch.

## 4. Metrics and statistics

**Primary:** hidden-test pass rate per arm, run on the merged result. Only the final tree counts, so partial credit can't hide a broken merge.

**Secondary:**
- dollars and tokens per task, split by agent and by model, from the usage ledger;
- wall-clock time;
- cards raised per task and the number of runs that stalled on a card;
- executed-bad-action rate (T6);
- silent-workaround rate (T3);
- bug survival (T4).

**Method:**
- Each task runs under every arm in the stage, **paired**, so the comparison is within task.
- Run each pairing 3 times to see variance (the models are not deterministic).
- Pass/fail comparisons use McNemar's test on paired outcomes; cost ratios use a bootstrap confidence interval.
- Sample size is a guess to confirm with the pilot: with about 70 paired tasks, a real 15-point gap is detectable at conventional power, and a 5-point gap is not. We should say so in the report instead of over-reading small differences.

**Controls against fooling ourselves:**
- Arm order is randomised and the machine is the same.
- Network egress is off except to the model endpoints.
- Graders for T5 and the free-text parts of T3 don't see the arm. They are humans for a 20% calibration sample, plus an LLM judge from a family that is **not** in the arm being judged, to avoid self-preference.
- The hidden tests are never visible to any arm.
- Event logs are kept for every run, so any surprising result can be traced.

## 5. Operator proxy and the human study

Delphi asks a human to approve specs and cards, and the benchmark can't. For measured runs:
- A scripted **operator proxy** approves specs that meet a fixed checklist and answers Amber cards by a written policy (`delphi run --on-card policy:bench.toml`). Red is denied. Counts of what the proxy answered are reported.
- This favours Delphi slightly (a perfect, instant operator) and ignores real approval fatigue. So it is paired with a **small human study**: 3 operators, 5 tasks each, real cards, which is the existing Step 2 go/no-go ("three real tasks on a real project"). Treat its result as the check on H6.

## 6. Pre-registered success criteria

> **Approved as proposed by the operator, 2026-10-25 (D-30).** These thresholds are now frozen: change them only by a new, dated decision made before the first measured run.

To be edited and ratified by the operator **before** the pilot. Placeholders are my proposals.

Delphi is justified, as a team and not just a wrapper, if **all** of these hold on T1 and T4, comparing A3 with the better of A1 and A2:
1. Hidden-test pass rate is at least **10 points** higher, or escaped defects are at least **50% lower**.
2. Median cost per accepted task is at most **2×**.
3. On T6, A3's executed-bad-action rate is at most **one third** of the permissive-flag baseline's.

**What happens after:**
- If a component's ablation shows less than a 3-point contribution and it costs more than 15% of spend, it is a candidate for cutting or making optional. The ablation results decide this, not intuition.
- If A3s matches A3 (one family does as well as two), then family diversity isn't earning its keep on code tasks. That doesn't kill the Council, which is judged on T5.
- If criterion 1 fails against A1, the honest conclusion is that a second look explains the gain, and the roles are not carrying it.

## 7. The runner

> **Built (skeleton):** [`dev/bench/bench.py`](../../dev/bench/bench.py) with a README, example arms and a policy file; its self-test runs in CI against fake arms and a real git repository. It does the fresh clone per run, the seeded arm order, the hidden-test patch after the arm finishes, and the JSONL rows, and it runs a Delphi arm as a copy of a pre-ratified template home via `delphi run`. **Nothing paid has been run.** Open: the spec-step operator proxy (a large change stops at gate 3), restricting network egress, and the task sets themselves.

- Lives under `dev/bench/` (a Rust `xtask` or a script, to decide). It checks out each task at its base commit in a fresh worktree, runs the arm, applies the hidden tests, and writes one result row per run.
- Arms are config, not code: an arm is a command line plus a Delphi config overlay. Adding an ablation means adding a config.
- Results go to `missions/.../bench/results/` as JSONL with the event-log path and the ledger totals for every run. Raw prompts and outputs stay out of the repository if they contain anything sensitive.
- The runner needs `delphi run` (headless) and the `policy:` card mode. Neither exists yet.

## 8. Stages and rough cost

| Stage | Scope | Runs (guess) | Cost (guess) |
| :-- | :-- | :-- | :-- |
| **S0 smoke** | 5 T1 tasks, A0 and A3, 1 run each. Checks the runner, not the hypotheses. | 10 | tens of dollars |
| **S1 pilot** | 15 T1 tasks, A0, A1, A3, 2 runs each. Replaces the guesses in §4 with real variance and cost. | 90 | low hundreds |
| **S2 main** | T1 and T4 at full size, A0, A1, A2, A3 and A3s, 3 runs each | about 700 | about 1.5–2k |
| **S3 ablations** | A4a to A4c, only on tasks where A3 and the best baseline disagree | about 150 | low hundreds |
| **S4 side suites** | T3, T5, T6, T7 | about 400 | several hundred |

At Delphi's default of $5 per mission and $20 per Council, the cap per run is already enforced, which bounds the worst case. All dollar figures are guesses from the per-mission cap, not measurements; S1 should replace them.

## 9. Risks to the evaluation itself
- **Our own tasks favour our own design.** T1 is partly drawn from this repository. Mitigation: other repositories in T1, plus T2 as an outside anchor.
- **Prompt effort asymmetry.** We know Delphi well and might tune it more than the baselines. Mitigation: A1 and A2 get the same attention, and their configs are written before the first run.
- **Model drift between runs.** Providers change models. Pin versions where possible, record the dates, and run arms of a stage close together.
- **Benchmark leakage via our own docs.** Keep hidden tests out of any repository an arm can read, including `kb`.
- **Single operator.** The human study has few operators. Say so in the write-up.
