---
state: built
date: 2026-11-26
title: "build now: seed at once, expansion queued for idle"
---

Slice E of spec 139, built 2026-11-26 on `build/139-de` (from `knowledge-v2`).

## What was built

- `Engine::seed_after_research` (`page_run.rs`), "seed now, expand at idle" of §2.5:
  - It makes the research topic's topic page if there is none (`topics_from_research`) and writes the seed page at once. No model is called; it is never paused and counts against no budget.
  - A topic whose page the writer wrote keeps that page; its own due check sees the new research.
  - A seed page that nothing changed is not written again (its due reason is only `SEED_EXPANSION`).
  - It then queues the writer's expansion: the topic's record gets `expansion_queued` with the time.
  - It returns `{slug, outcome}` with fixed words only: `seed`, the written page's outcome, `waiting`, `check_failed` or `failed`.
- Where it runs:
  - A research run that ends `done` or `partial`: `Jobs::submit_then` (`research_jobs.rs`) awaits a step after `on_end` and before the job reads as finished, however the job ended, so the seed page exists before the result can be read. `queue_run` (`service.rs`) uses it for every run on the one runner. The record learns the end first (the seed reads its answer); the caller's `after` (the `research.finished` event) is told after the seed step.
  - `record_research`: after the call is recorded and the index refreshed. The reply gains `page: {slug, outcome}`.
- `host.research_result` of a finished run names the page too (`page: {slug, outcome}`, read by `Engine::research_page`).
- The idle rebuild takes queued topics first, the earliest first, then topics with something new, then seed pages that wait only for their expansion. A writer's page clears the mark; so does a seed the writer could not expand (`SEED_STANDS`), which then waits like any seed page.
- The expansion runs within `per_day`, the background share and the bridge's admission (`idle_job_may_start`), as every idle rebuild does. When one refuses, the topic stays due and the seed stands.
- `[library.pages] build_after_research` (default `true`) turns the trigger off: no topic, no seed and no queue mark from this path, and no `page` in the replies. `auto_rebuild = false` still leaves the writer's rebuilds to the operator; the seed is written regardless.
- Config: `config.rs`, `schemas/config.schema.json`, `docs/reference/config.md`, `crates/delphi/config.example.toml`. The `record_research` tool's description says the seed is written at once and the reply names its slug.

## Acceptance criteria and tests

- AC-10, the reply and the switch: `service::tests::record_research_writes_the_seed_page_at_once_and_names_it_unless_the_trigger_is_off`.
- AC-10, the queue: `page_run::tests::a_queued_expansion_runs_ahead_of_an_older_due_topic_unless_the_trigger_is_off`. With the switch off, the older due topic goes first.
- The step before a job reads as finished: `research_jobs::tests::the_step_after_a_job_ends_runs_before_the_job_reads_as_finished`.
- The default and the off switch parse: `config::tests::library_pages_defaults_and_ttl_forms`.

## Builder decisions

- The queue is a mark in the topic's record, not a list in memory, so it survives a daemon restart.
- A seed the writer could not expand leaves the queue after one try. Otherwise one bad seed would stay first and hold back every other topic.
- §2.5 item 1 names `job_may_run`; the admission used is `Engine::idle_job_may_start` (143), the one admission path.
- The console's own research status does not name the page; the host's `research_result` and the `record_research` reply do.
- A run that fails, times out or is cancelled writes no seed.
- A run on a topic whose page is a seed writes a new seed version when its spans changed. The service test `an_app_request_to_continue_starts_the_run_the_terminal_starts_and_takes_only_ids_and_typed_text` wrote a page by hand for a topic; it now finds the topic the run made instead of making it, and writes its page again after a later run.

## Not verified

- No live run: no real research run or host call has written a seed this way on the operator's install.
- The idle expansion is proven only with a scripted model whose call fails (the seed stands). A writer's page landing after a queued expansion is covered by the existing rebuild tests, not by this slice's own test.
