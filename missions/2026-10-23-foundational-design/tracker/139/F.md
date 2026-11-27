---
state: built
date: 2026-11-26
title: "the page budget as a row of the admission table"
eval_waived: "no ranking term, search path, chunking or index change in this slice; the diff base counts knowledge-v2's own retrieval changes"
---

Slice F of spec 139, built 2026-11-26 on `build/139-f` (from `knowledge-v2`).

## What was built

- **`[library.pages] week_tokens`**, default 600,000 (`config.rs`, the schema, the config reference, the example). It is what writing and validating topic pages may spend in seven days.
- **One row of the admission table, not a meter of its own** (`job_admission.rs`).
  - `weekly_budget(Job::Pages)` now reads `week_tokens` through `Inputs::pages_week_tokens`; the hard-coded 600,000 is gone.
  - `week_spent(calls, job, now)` sums a job's tokens over the last seven days of the one ledger, over every credential and endpoint. A weekly budget is the job's, however its calls were served.
  - `Inputs::reserve`: what a start sets aside. The job is refused when the week's spend plus the reserve passes the budget. A single bridged call asks with reserve 0, as before.
  - `Inputs::bridged`: a job served by an API endpoint meets only its switch and its weekly budget. The ceiling, the window, the daily cap, the idle share and the pauses are the subscription's, so they apply to bridged calls only. `confirmed` passes the weekly budget on both paths, as it already did for the bridge.
- **The reserve by depth** (`page_run::page_reserve`): a summary page reserves `PAGE_RESERVE` (20,000), a long page four times that (80,000).
- **The page build asks the one function** (`Engine::page_admission`, `page_run.rs`). A topic rebuild's gate no longer asks `share_allows`. It asks `job_may_run(Pages, origin, credential)` with the long reserve.
  - The origin is the build's: `idle` inside the daemon's idle job, `command` or `confirmed` inside a page command.
  - The credential is the bridge's when a harness writes the page. For an API endpoint it is the endpoint's own ledger name (`summary::endpoint_credential`).
  - The switches are read by `job_switched_on`: `enabled`, `auto_rebuild` for idle runs, and `[bridge.jobs] pages` when the bridge serves the page. It gained a `bridged` argument, so the bridge's mode is not applied to an API-served page, as before.
  - A refusal keeps the seed and leaves the topic due, through slice C's `topic_keep_seed` (`the seed stands: the build was not admitted: this week's page budget is spent (...)`). The idle job's cause is `budget`.
- **A page's calls through an API endpoint are counted in the one ledger** (`summary.rs`, `page_call`). A single call through an API endpoint, made inside the `pages` job or a page command, writes a start line (job `pages`, the origin, the endpoint's ledger name, the estimate from its prompt) and an end line (the reported tokens, or the estimate, and the outcome). Bridged calls already wrote theirs. So the writer's and the validator's tokens count against the page budget whatever serves them.
- **A rebuild outside any scope is a page command.** `topic_rebuild_with` runs inside `in_command(Pages, false)` when it is not already in the `pages` idle job or a page command (the app's `app.page_rebuild`). Its calls are counted and it is held to the budget.
- **Topic pages stop drawing on the validator's share** (O9). A topic rebuild no longer journals a `background` `span` with `page: true`, so neither `background_allows` nor `share_allows` sees it. Its validator reserves from a `TokenLedger` of its own build, not the validator's run allowance (`fact_serve::TOKEN_ALLOWANCE`). The doc of `fact_audit::BACKGROUND` says so. Search pages (`page_search`, `page_expand`, `page_refresh`) are unchanged and stay on the share.
- **`delphi kb health`** (`knowledge_health.rs`) adds `pages: N due, this week's page budget is spent (X of Y tokens in seven days): the topics stay due and their seed pages stand until the week rolls on. [library.pages] week_tokens sets it.` It appears when a long page's reserve no longer fits, and the JSON gains `pages_week {due, spent, week_tokens}`.

## Acceptance criteria and tests

- **AC-11**, "with `week_tokens` spent, a due topic is not written, keeps its seed and is reported by `delphi kb health`": `page_run::tests::a_spent_page_budget_keeps_the_seed_and_a_page_is_never_charged_to_the_validators_share`. With 550,000 of 600,000 spent through another credential, the idle rebuild asks no writer and the error carries the page budget words. The cause is `budget`, the outcome stays `seed`, the topic stays due, and the health line and JSON say so. The same ledger leaves room for a summary page's reserve, so the refusal comes from the long reserve.
- **AC-11**, "a page's tokens are not counted against the validator's share": the same test, a week later. The writer is asked through an API endpoint, and its call is in the one ledger as the `pages` job's, origin `idle`, under the endpoint's ledger name. The validator journal holds no `background` span, and `background_allows` is untouched.
- **AC-11**, "the reserve for a long page is four times a summary page's": `page_run::tests::a_long_page_reserves_four_times_a_summary_page`. The reserve as a term of the admission is tested by `job_admission::tests::the_page_budget_is_week_tokens_counted_over_every_endpoint_with_the_reserve`. That table covers `week_tokens` against the reserve on both paths and for each origin, spend through two credentials as one week, the seven-day edge, the switch, and an API page that is not held by the bridge's window or pause.
- A rebuild outside a page command (the app's): `page_run::tests::a_rebuild_outside_a_page_command_is_held_to_the_page_budget`. A typed confirmation passes the budget.
- The key's default and parse: `config::tests::library_pages_defaults_and_ttl_forms`.
- 143's admission table (`job_admission::tests::the_admission_table`) is unchanged and passes. The weekly row for pages still uses 600,000.

## Builder decisions (to confirm)

1. **A weekly budget is counted over every credential, not per credential.** Before, the weekly sum was filtered by the bridge's credential. The page budget bounds what pages spend whatever serves them, so the sum now covers every credential and endpoint. This also applies to the summaries and consolidation budgets, which is stricter, never looser. The window, the daily cap and the ceiling stay per credential.
2. **An API-served page meets only its switch and its weekly budget.** The bridge's limits (window, daily cap, ceiling, idle share, pauses) are the subscription's. Applying them to API calls under a second credential would be a new limit the spec does not name.
3. **Only the `pages` job's API calls are written to the ledger.** Summaries and consolidation through API endpoints are not, because that is their own slices' business (142, 116). The function would count them the same way once written.
4. **`per_day`, `build_after_research` and `stale_writer_per_day` are not inputs of `job_may_run`.** `per_day` stays in `topic_rebuild` (slice C's check, before the gate). `build_after_research` governs the seed and the queue, which call no model. `stale_writer_per_day` does not exist yet (slice G). The switches that decide whether a model call may start (`enabled`, `auto_rebuild`, `[bridge.jobs] pages`) are read in `job_switched_on`, the one place.
5. **"Spent" in `kb health`** means a long page's reserve no longer fits (`spent + 80,000 > week_tokens`), which is when the next topic page would not start. It is not `spent >= week_tokens`.
6. **The validator of a topic page reserves from an allowance of its own build** (`week_tokens`, at least one long reserve), not from the validator's per-run allowance. The page budget is the bound.
7. **A failed API call counts with its prompt's estimate**, and a call the daemon died under counts with its start line, as for bridged calls.
8. A wait from the bridge (the operator at work, spent quota) at the page's gate keeps the seed with its reason. The idle path already asks `idle_job_may_start` first, so this only happens when the state changes in between.

## Not verified

- No live run. The 600,000 figure and the fourfold reserve are the spec's guesses (§7). What a long page really costs to write and validate is not measured.
- The estimate for an API call whose provider failed is the prompt's length over four. It may be far from what was billed.
- Search pages still draw on the validator's share, as §2.3 item 8 names topic pages only.
