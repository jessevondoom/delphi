---
state: approved
date: 2026-11-22
pr: 868
title: "`tests_run` only with a test result; probe calls and gate reasons recorded and shown in doctor; the serving endpoint and any fallback in a review's result"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 1346, under `The forge, first cut (spec 135, approved in advance 2026-11-20)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 135 | F2f | `tests_run` only with a test result; probe calls and gate reasons recorded and shown in doctor; the serving endpoint and any fallback in a review's result | approved on the list. Built 2026-11-22 (draft pull request, stacked on #868): (1) `review_job::runner_result` reads a configured command's whole output: `cargo test` counts only with a `test result:` line (failing count summed over the binaries), another runner by a `result` regex given with it (`[forge] review_commands` entries may be `{ argv = [...], result = "..." }`, a `failed` group gives the count; plain argv lists read as before), any other command by its exit status; when any configured command is a recognised runner, the review's `tests_run` comes from the runners only (a lint that exits 0 never makes it true while `cargo test` stopped at setup), and only with no runner configured does the exit-status rule apply. A command that started and showed no result is listed `tests_run: false`, `reason: "setup"` with its last lines; the review's `tests_reason` is `setup` and the note and brief say the tests did not run; failing tests are `tests_run: true` with `failed` / `tests_failed`. (2) The background probe keeps every tool call of each attempt (name, argument names never values, the gate's decision and reason, whether a result came back, unknown argument names) in `local/forge-tools.json` (last 5 attempts) and the `forge.tools_probe` event (`calls`, `reason`); the agy hook note now carries the gate's reason and the argument names; a refusal for an unknown argument key is named in the probe's reason; `delphi doctor` (`11 forge tools`) prints the last failing or undecided attempt's calls of a version not passed. (3) `host.review`'s result has `served_rounds` (per round: `endpoint`, family, model, outcome, `fallback` from / to / why from `harness.fallback`, `model.exhausted`, `endpoint.throttled`; a fall-back `to: api` names the endpoint that then served), a top-level `endpoint`, and a note sentence per fall-back. Verified: AC-21, AC-22, AC-23 in unit tests (scripted runners, synthetic events, a fake carrier); the daemon review test checks `served_rounds` and a `result` pattern. Not verified: a live agy probe; real fall-back event order in a live review |
