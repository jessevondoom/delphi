---
state: built
date: 2026-11-26
title: "the history: the ledger, the scheduled run, kb eval history, the view, the note"
eval_waived: "adds the eval history and a [retrieval] key that schedules runs; no ranking term, search path or index changes"
---

Slice G of spec 140, built 2026-11-26 on `build/140-efg` (from `knowledge-v2`).

## What was built

- **The ledger** `local/eval/history.jsonl` (`kb_eval::append_history`). Each run adds one line per path, with the fields of §2.6 item 1: `v`, `at`, `run`, `trigger`, `set`, `corpus` (with `vectors`), `config_sha256`, `models` (ids and SHA-256 of the model packages), `build`, `path`, the four metrics, `p50_ms`, `p95_ms`, `vector_leg` (service only). Numbers and identifiers only.
- The file is opened to append only. Nothing truncates or rewrites it. A line that does not parse, or a later format version, is skipped and counted.
- **Who writes it.** `delphi kb eval run` and the daemon's scheduled run both go through `kb_eval::run_recorded`. A `kb eval run` started inside an agent seat writes its run file only and says it was not added to the history.
- **Triggers.** `manual`, `scheduled`, and `before` or `after` from `--trigger before|after` (a label of `before` or `after` says the same).
- **Series.** A series is one set version on one path. A new set version starts a new series; series are never compared across.
- **The scheduled run** (`EngineService::eval_tick`). It looks once an hour. When `[retrieval] eval_every_days` (new key, default 7, 0 is off) have passed since the ledger's newest line, an accepted set exists, no task runs and the operator has been away five minutes, it runs the service path. No model call, no tokens.
- **The view** `local/knowledge/retrieval-quality.md` (zone `knowledge`). It is rebuilt from the ledger after every run: per series one sentence (`Set v1, service path: nDCG@10 0.9092, steady over 6 runs.`) and a table of the last 20 runs. No question text.
- **`delphi kb eval history [--set <n>] [--path <p>] [--json]`**: each series as a table with the change in nDCG@10 from run to run and a one-line trend.
- **The health line.** `delphi kb health` adds the newest run's nDCG@10 against the median of the runs before it in its series (at most five).
- **The note** (`kb_eval::note_changes`, `provisional_notes::keep_daemon_fact`). A run is noticeable when, against the median of the five runs before it, recall@10 or nDCG@10 moved 0.03 or more, the vector leg ten points or more, or p95 reached 1.5 times the median. Then the daemon writes one provisional note: kind `fact`, folder `shared`, tier L3, trigger `retrieval_eval`, in words made by code. It names the run, the run before it, what moved, and what differed between those two runs (build, models, config hash, corpus size). One note per series and direction until a run is back inside the thresholds (`local/eval/noticed.json`). Nothing promotes it.

## Acceptance criteria and tests

- **AC-11**: `kb_eval::tests::every_run_adds_its_ledger_line_and_no_rebuild_new_set_or_pruning_touches_the_earlier_ones`. It checks the fields, then an index change, a new set version and deleted run files, and that the earlier bytes are a prefix of the file. A torn line is skipped and reported. A scan of the module checks that only `append_history` writes the file, and only by appending.
- **AC-12**: `kb_eval::tests::after_a_run_the_view_holds_each_series_and_no_question_and_an_agent_search_finds_it`. The view holds the sentence and the table and no question. After a sync, a builder's default search for `retrieval quality` returns `knowledge/retrieval-quality.md`.
- **AC-13**: `kb_eval::tests::a_noticeable_fall_leaves_one_note_naming_both_runs_and_a_small_fall_or_a_second_low_run_none`. A fall of 0.01 leaves no note. A fall of 0.05 leaves exactly one, which names both runs and the differing build. A second low run leaves none. A run back inside the thresholds clears the series, and the next fall leaves a new note.
- **AC-14** (the command's half; the app's panel is slice H):
  - `kb_eval::tests::kb_eval_history_shows_the_ledgers_series_and_numbers`.
  - `kb_eval::tests::a_scheduled_run_is_due_every_eval_every_days_while_idle_and_never_at_zero`.
  - `service::tests::the_scheduled_golden_set_run_is_made_while_idle_and_never_at_zero_days`: with 0 no run is made; at 7 an idle daemon adds one `scheduled` line and the view, and not a second within the week.

## Builder decisions

- One ledger line per path. A run of `--path all` adds three lines, one to each series.
- The note needs five earlier runs in its series ("the median of the five before it"). A series with fewer is never noted. With a weekly run, the first note is possible after six weeks.
- The health line compares with the median of up to five earlier runs, and says "the first run" when there is none.
- A daemon note is written only while the provisional tier is on (`[memory] provisional`).
- The daemon writes the note through a new entry point in `provisional_notes.rs` (§7 named this gap: `keep` needs an agent's event).
- The view is filed by the index under the workspace's `local/knowledge` tier (`federated_verified`). A builder's default search reaches it (AC-12), so it was not moved.
- `eval_waived` is set on this slice: it touches `config.rs`'s `RetrievalConfig` (a new key that only schedules runs), and no ranking.
- Run files are not pruned by this slice. The spec allows pruning after 90 days; the ledger does not depend on them.

## Not verified

- No run on the operator's workspace. The scheduled run has not run in a live daemon.
- The thresholds of OD-5 are still guesses. The run-to-run spread from slice D (zero on an unchanged build, but one cold first run in three) is the only evidence.
- The app's "Retrieval quality" panel and its read-only method are slice H, not built.
- Hashing the model packages reads them once per process (about 146 MB).
