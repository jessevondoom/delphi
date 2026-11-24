---
state: built
date: 2026-11-06
title: "harness arm kind (fixed model) and efficiency columns in the 037 report"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 273, under `Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 048 | BM1 | harness arm kind (fixed model) and efficiency columns in the 037 report | built 2026-11-06: an `[[arm]]` with `base = "<arm>"` and `[arm.loop]` switches takes its base's template home, `expect_models` and overlay (`train_run.rs` `parse_arms`; switch names and value kinds checked by `loopctl::check_switch`, so a typo is refused before a run; applied to the run's copy of `config.toml`, `[loop]` not being a pinned block). Rows keep turns, tool calls by tool with errors, nudges, hard stops, budget warnings, verify turns, escalations and cache read share (`efficiency_of`, reusing `toolreport` and `ctxacct`). `delphi train report` prints an EFFORT table and a HARNESS EFFECT section per model: pass rate with the noise (one standard error of the difference), tokens, turns and calls per pass, and the OD-2 verdict (`costlier, not better` over 25 % tokens per pass with the rate inside the noise; `a trade` for more passes at that cost; `no evidence` under 3 seeds). Tests: AC-1 (arm and base on one model, case and seed, both in the report) and AC-2 (the flag). Not built: harness arms that vary the tool set or a prompt section (switches and overlay only); per-tool names in the report (rows keep `by_tool`); no live run (scripted fake binary only) |
