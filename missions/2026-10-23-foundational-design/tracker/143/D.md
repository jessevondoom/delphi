---
state: built
date: 2026-11-25
title: "visibility: daemon.notice, the error log line, health, doctor; the 117 amendment (merges first)"
---

Built 2026-11-25 on the local branch `build/143-a` (not pushed, no pull request; the lead integrates).

What is there. `daemon_notice.rs` (new): the closed causes (`no_route`, `budget`, `quota`, `timeout`, `check_failed`, `harness_tool`) and jobs (pages, source summaries, validation, session summaries, consolidation), and the record `local/daemon-notices.json` in the workspace, which counts failures for each cause in a day, remembers what the brief has said, and keeps each job's newest failure with the day its cause was first seen. `Engine::job_failed` leaves a failure in a sink; `EngineService::notices_tick` raises it: the first of a cause in a day is one `daemon.notice` event (`cause`, `job`, `key`) and one line in the error log at `error` (`background_job_failed`, the fixed words, the cause code as `reason`). The console's notices (`status`) read today's lines from the record, one a cause with its count. `host_hooks::brief` says the unsaid causes once, in one line. `knowledge_health` adds `pages: N due, last idle rebuild failed: <cause>, since <day>` until an idle rebuild runs. `doctor.rs` `check_single_calls` (ported from commit `c1fca8ca`, `check_turn_by_turn`, doctor part only; the rewrite fix is not here): on a config whose every allowed endpoint is a harness it asks the router, and prints an ordinary line that names the serving harness, or a warning that names both remedies.

Wired jobs: the idle topic rebuild (`no_route`; `budget` when the validator's share is spent; `check_failed` when the library had material and the page ended as the claims alone), background validation, session summaries and consolidation (`no_route`). Source summaries (`specs/142` §2.6) are not built: the job is in the closed list and joins the test when it exists.

The 117 amendment (§2.7 item 6): with no route for the writer the idle rebuild writes nothing, the topic stays due, and the cause is a notice. Tested.

Verified (tests): AC-7 `service::tests::an_idle_rebuild_with_no_route_is_one_notice_a_day_and_holds_no_entry_text`; the fitness function `service::tests::every_idle_job_failure_writes_a_notice` (also the count of AC-18); `daemon_notice::tests::*`; `host_hooks::tests::background_jobs_that_could_not_run_are_said_once_in_the_next_brief`; AC-8's warning `doctor::tests::doctor_warns_when_only_harness_endpoints_are_allowed_and_none_serves_a_single_call`. AC-8's ordinary line is tested in slice A, which adds the routing rule the doctor asks.

Not verified, and limits:
- The log line is checked by rendering it with a logger at the default settings, not by reading the daemon's log file: the logger is process-wide, and a test that installs one would race the others.
- The app shows a notice only as far as it already shows the daemon's notices and events; no app code changed.
- A failure with no cause in the closed list (an I/O error while a page is stored) raises no notice: it is one fixed line in the error log (`topic_rebuild_failed`, now at `error`).

Deviations and corrections for the spec:
- AC-18 is in no slice's criteria (§4). Its notice half (one notice with a count; the key named) is built here and in slice A; its ranking half is slice A's.
- §2.7 item 3 says "a count of the jobs it stopped"; this counts each failed run, so two failures of one job count two.
