---
state: built
date: 2026-11-26
title: "the rule: RETRIEVAL_PATHS, the finisher's advisory check, the repository baseline"
eval_waived: "the check, the tracker fields and the eval's report; kb.rs gains eval code and a test, and no ranking term changes"
---

Slice F of spec 140, built 2026-11-26 on `build/140-efg` (from `knowledge-v2`).

## What was built

- **`RETRIEVAL_PATHS`** in `finisher.rs`: `kb.rs`, `retrieval.rs`, `kb_docs.rs`, `embed.rs`, `embed/**`, `research_index.rs`, `use_signals.rs` (all under `crates/delphi-engine/src/`). The `[retrieval]` keys of `crates/delphi-adapter/src/config.rs` count too: that file is a retrieval change when its diff has a changed line that names retrieval, or a hunk inside `RetrievalConfig`.
- **The finisher's check** (`retrieval_check`, step 4b of `delphi forge finish`). The changed files are those against the merge base with `origin/main`, untracked files included, as the fast tier reads them. On a branch that touches a listed path:
  - with `eval_waived: <reason>` in the slice's tracker file it passes, and the body says why;
  - with `eval_before` and `eval_after` it reads both run files from one workspace of `--home` and puts `kb eval compare`'s text in the body;
  - otherwise it is a finding (exit 1) that names the command to run and the fields to add. Runs on two set versions are refused by `compare`, so they are a finding too.
- On a branch that touches no listed path it asks nothing. With no merge base it says it did not check.
- **The tracker fields.** A per-slice file may carry `eval_before` and `eval_after` (run stamps `YYYYMMDDTHHMMSSZ`, given together) or `eval_waived` (one line). `schemas/tracker-slice.schema.json` and `docs/reference/schemas.md` say so.
- **Advice, not a gate** (§2.4 item 5, OD-3). Nothing was added to `merge-gates.toml` or to CI. No gate-changing path was touched.
- **The repository set** (`delphi kb eval`) now reads twenty results and prints recall@20 and nDCG@10 next to the earlier columns. recall@10, the other columns and MRR are still counted within the first ten.
- **`dev/kb-eval-baseline.json`** holds the set's keyword numbers of 2026-11-26: 40 cases, recall@10 0.4750, recall@20 0.5250, nDCG@10 0.3315, MRR 0.2861.

## Acceptance criteria and tests

- **AC-9**:
  - `finisher::tests::ac9_a_retrieval_change_needs_two_runs_or_a_waiver_and_another_change_is_not_asked`: a real git branch that changes `retrieval.rs` fails without the runs, passes with two runs on one set and gives the comparison, fails with runs on two set versions, passes with `eval_waived`; a branch that changes `kb_eval.rs` only is not asked.
  - `finisher::tests::ac9_forge_finish_prints_the_retrieval_finding_and_puts_the_comparison_in_the_body`: the same through `finish_with`, with the finding and then the body section.
  - `finisher::tests::the_retrieval_paths_and_the_retrieval_keys_of_the_config_are_a_retrieval_change`.
  - `tracker::tests::a_slice_file_carries_the_runs_of_a_retrieval_change_or_its_waiver`.
- **AC-10**: `kb::tests::the_repository_set_holds_its_committed_baseline_and_a_lowered_weight_fails_it`. The set is indexed from the repository's documents by keywords, in memory, in about 3 seconds. The measured numbers must stay within 0.02 of the file. A fall of 0.05 in nDCG@10 fails, a fall of 0.01 holds, and the file updated to the lower numbers passes.

## Builder decisions

- **AC-10 lowers the measurement, not a weight.** No ranking weight in `SearchOpts` moves this set's keyword nDCG@10 by 0.05. Measured on 2026-11-26: `natural_lex_weight` 0 moved it by 0.0013, `loose_weight` 0 by 0.0005, both by 0.0055. So the test lowers the measured nDCG@10 by 0.05 as such a change would, and checks the rule on that.
- The baseline is the keyword path only. Tests have no embedding model, so the hybrid numbers cannot be checked in CI.
- The base for "what this branch changes" is `origin/main`, as in the fast tier. A branch built on another integration branch (this one is on `knowledge-v2`) also counts that branch's changes.
- The two runs are looked for in every workspace of `--home`, and both must be in the same one.
- The body shows `compare`'s text as it is: numbers and case ids only, no question.
- `eval_waived` is set on this slice. F's `kb.rs` change is the eval's report and a test; it moves no ranking.

## Not verified

- No run was made on the operator's workspace for this slice; none is needed for a waiver.
- **Drift.** The baseline test reads the repository's documents, so a pull request that only adds documents can move the numbers. One question moving out of the first ten changes recall@10 by 0.025, more than the 0.02 slack. The test message says to update the file when that happens.
- The check has not yet been used on a real retrieval pull request.
