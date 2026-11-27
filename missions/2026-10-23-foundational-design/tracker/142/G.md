---
state: built
date: 2026-11-26
title: "summaries of every source, the budget row, kb summarize, then the pointer backfill"
eval_waived: "no ranking term, chunking rule or search path changes; summaries are indexed as new documents at external_untrusted, which a default search (it asks for nothing untrusted) never returns, and no install holds a summary until the job runs, so a before/after run would compare the same index; the diff base counts knowledge-v2's own retrieval changes"
---

Built 2026-11-26 on branch `build/142-g` (from `knowledge-v2`). Not merged.

## What was built

- **`source_summary.rs`** (new): the summary of every source (§2.6) and the backfill of the pointers (§2.7).
  - **When.** `Library::store` queues a source whenever it has the text in hand (a fetch, a host's capture, an ingest, a re-read), at every level and under every value of `unknown_licence`. The queue entry (`library/summary-queue/<id>.json`) holds what the model will be sent, on this machine only, until the summary is written. A revisit queues nothing for a text that already has a summary; a new text version does.
  - **The input** (`select_input`): the whole text when it fits in 24 KiB, else the opening and each heading with its first paragraph, chosen by code.
  - **The call** (`Engine::summarize_source`): one single call on the primary family's `fast` tier, a marked caller of 143 (`route::SINGLE_CALLERS`, `summary::CALL_SITES`, `single_call_tests`). Its inputs are stated as `external_untrusted`.
  - **The checks** (`checked`): control characters out, secrets redacted, the workspace's identifiers masked, every link but the source's own address removed, at most 250 words and 2 KiB, and `held_text::body_may_leave` against what the model was sent (a run past 300 characters is cut back with `shorten`; what still fails the aggregate cap is dropped and recorded as dropped, never kept).
  - **Kept**: `summary.md` (headed `summary written by a model, not checked against the source`) and `summary.json` (model, family, `text_sha256`, `from`, tokens, when). A takedown removes both and the queue entry; eviction and a purge of the text leave them.
  - **Drain** (`Engine::summaries_drain`): pending sources oldest first, stopping at the first call that does not run or the first spent budget. With no route it writes nothing and, for the daemon's own run, leaves the job's `no_route` notice.
- **The budget.** `[library] summary_week_tokens` (default 300,000, at least 1; config, schema, reference, example). Through the bridge it is the `summaries` row of 143's admission table: `job_admission::weekly_budget` and `Inputs` now take it, replacing the fixed 300,000. `job_admission::weekly_free_at` says when the week next allows more.
- **The daemon** (`service.rs`):
  - `summaries_tick`: every 5 minutes, while the operator has been quiet and no task runs, up to 5 summaries as the job `summaries` (`in_job`, `idle_job_may_start`), then the index is refreshed and notices raised.
  - `library.summarize {source | pending, plan, confirmed}` inside `in_command(Job::Summaries, …)`, refused by `command_stop` with `SUMMARIES_OFF`. `bridge.preflight {job: "summaries"}` now asks the summariser's key.
  - `library.recheck_pointers {limit, plan, confirmed}`: the backfill, with the daemon's own fetcher (`LiveWeb`) and the `unknown_licence` in force; the summaries of what was read are then written inside the command scope, within its limits.
- **The commands** (`bridge_cmd.rs`, `main.rs`): `delphi kb summarize [--pending | <source>]` (bare is `--pending`) prints the sources, the token estimate, the route and when the budget allows more, then runs the preflight and the typed confirmation. `delphi sources recheck --pointers [--limit <n>] [--dry-run]` prints what it would read and spend, and lists each pointer that `needs a host capture` with its locator. `kb`'s help stays at 11 lines: `summarize` is folded into the `import | validate` line.
- **Shown.** `delphi sources show` prints the summary with its model, date and the label, or `summary: pending` (and `summary` in `--json`). `delphi kb health` adds `source summaries: N sources with text, N with a summary, N pending` and, when the week is spent, the day it allows more (`source_summaries` in the JSON).
- **Indexed** (`kb.rs`): every summary not taken down, private ones included, at `external_untrusted`, as `library/sources/<id>/summary.md`.
- **Labels** (`label.rs`): `Labeller::summary`: the source's marks without `text_held`. A record that cannot be read stays every mark.
- **Sinks.**
  - `held_text::withholds_path` (seed-export, remote `kb serve`) now withholds every file of a private source too, and sends the summary of a held one.
  - A bundle's `sources/` copy of a held source with a summary is a pointer with the citation and the summary as its `abstract`, asked against `body_may_leave` again. Without a summary it is refused as before.
  - The bibliography gives a source's summary as `abstract` where the record has none; a held source's is still cut by `held.cut_field`.

## Acceptance criteria and tests

- **AC-19**: `source_summary::tests::under_pointer_a_page_read_keeps_no_text_and_gets_one_summary_per_text_version`.
- **AC-20**: `source_summary::tests::a_summary_is_checked_untrusted_no_anchor_and_survives_eviction_and_purge`. The checks alone: `what_the_model_wrote_is_cut_to_fit_or_dropped`. The input rule: `the_input_is_the_whole_text_or_the_opening_and_each_heading`.
- **AC-21**:
  - `source_summary::tests::a_spent_week_leaves_a_new_source_pending_until_the_week_turns` (an API endpoint, a fixed clock).
  - `source_summary::tests::with_no_single_call_route_summaries_stay_pending_and_health_counts_them`.
  - The command: `source_summary::tests::kb_summarize_plans_then_writes_and_refuses_a_bare_pointer`.
  - The bridge's row: `job_admission::tests::the_summaries_row_reads_summary_week_tokens_and_says_when_it_allows_more`.
  - The notice: `service::tests::every_idle_job_failure_writes_a_notice` now includes `summaries`.
- **AC-24**: `source_summary::tests::hostile_text_unlocks_nothing` (the fitness function of §6). The span is tainted and every promotion path refuses. The source's tier and status are unchanged. The passage reaches a Council seat (`run_seat`) inside a `<slice>` labelled with its path, `L0` and `not validated`, and the seat's tools are the same as without it.
- **AC-25**: `source_summary::tests::a_held_sources_summary_is_unlabelled_and_sent_and_a_private_ones_is_refused_everywhere`.
- **AC-27**, the summary clause B left: `what_the_model_wrote_is_cut_to_fit_or_dropped` (ten quotations of 290 characters).
- **AC-28**:
  - `source_summary::tests::the_backfill_reads_at_most_its_limit_and_lists_what_needs_a_host`, with a scripted fetcher.
  - The typed confirmation is 143's (`bridge_cmd::confirm`), asked by `recheck_cli` through `preflight`.
  - The live run on the operator's workspace is **not done** (below).
- `single_call_tests::source_summary_makes_one_request_with_no_tools`; `summary::tests::every_call_site_of_a_single_call_is_on_the_list` lists `source_summary.rs`.

## Builder decisions (for the architect)

1. **The queue keeps up to 24 KiB of text on this machine** for a source whose text is not kept (`pointer`, `excerpts`), so that a summary that must wait for the budget can still be written after the week turns (AC-21). It is never indexed, exported or counted in the text cache, and it goes when the summary is written or the source is taken down. Under `pointer` the library therefore holds a source's text for a while; the stricter alternative (summarise only while the text is in memory) cannot meet AC-21.
2. **The budget on an API endpoint** is counted from the summaries' own records (`summary.json` tokens in the last seven days). 143's ledger records only bridged calls, and §2.8 item 4 says a job on an API endpoint is governed "as today", which for summaries is nothing. Taken strictly, the spend stays bounded everywhere. This is a second count, which §2.6 item 3 says not to have; it applies only where the ledger does not count.
3. **A private source's summary is indexed** (§2.6 item 6, AC-25). To keep every sink refusing it, `withholds_path` now withholds every file of a private source, not only of a held one. Before, nothing of a private source was indexed, so this tightens nothing that was sent.
4. **A held source in a bundle** is a pointer whose `abstract` is the summary (the bundle format has no `summary.md`, and its layout check refuses unknown files). Without a summary it is still refused, as slice B decided.
5. **The bibliography** shows a summary as `abstract`, prefixed with the label, only where the record has no abstract of its own.
6. **What the model was sent is what `body_may_leave` is asked against.** A summary can carry only what it was given. For a held source it is the same text the sinks check again.
7. **A dropped summary** is recorded in `summary.json` (`dropped: true`, the text hash, the model, the tokens), so the same text is not sent again. Its source reads `summary: pending` until it has a new text; `delphi kb summarize <source>` asks again.
8. **A new text version** moves the old summary with the old version under `versions/<n>/`; the source reads pending until the new one is written.
9. **`kb summarize` with no argument** is `--pending`, as `[--pending | <source>]` reads.
10. **The backfill reads only what the daemon may fetch.** That is an https address that is not private, under `LiveWeb`'s rules. A private pointer, an address that is not https, and a fetch that is refused are each `needs a host capture`. Its summaries run inside the command scope, so past a limit they need the typed confirmation, and they stop at the ceiling; the pages it read stay read either way.
11. **The idle job writes at most 5 summaries a look, every 5 minutes**, under the same quiet rule as the idle page rebuild. These numbers are not in the spec.

## Existing tests changed on purpose

- `service::tests::every_idle_job_failure_writes_a_notice`: the `summaries` job now exists and joins it (5 jobs, not 4).
- `job_admission` tests pass the summaries' budget to `Inputs` and `weekly_budget`.
- `private_read::READS` gains `source_summary.rs` (`NotYetMarked`): it reads any source's text, private included, and sends up to 24 KiB to the summariser. The operator's ruling allows that; the summary it keeps is `library_private` with its source.

## Gates (2026-11-27)

`forge finish --gates --spec 142 --slice G`: tests passed (one known flaky test, `train_run::tests::routing_arms_set_the_assignment_and_a_run_over_its_token_cap_counts_as_not_passed`, passed on its rerun), clippy passed, lint passed.

## Not built

- **The seed of 139 §2.1** does not read summaries yet (§2.6 item 8 names it); the search and the bibliography do.
- The app shows no summary or button yet. Its button and its grant-list entry for `library.summarize` belong in the gate-changing pull request of 143 F part 3.

## Not verified

- **AC-28's live run** on the operator's workspace, and the operator's first read of the summaries (§4 "live and manual steps are dated"). Both wait for the operator; neither has a tracker entry.
- **No real model call.** No summary has been written through the bridge or an API on a real install. The bridged path is tested only through the admission table and the shared `one_shot_raw`.
- **Cost.** The 6,000-token estimate per source and whether 250 words from the headings of a long source are useful (§7) are not measured.
- **Scale.** `pending` and `counts` read every source's record on each call. They are not measured on a large library.
