---
state: built
date: 2026-11-21
title: "`delphi forge finish --gates`: the three gates, a flaky rerun by test name; gate-changing (`spawn_sites.toml`), merged at a terminal"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 1342, under `The forge, first cut (spec 135, approved in advance 2026-11-20)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 135 | F1b | `delphi forge finish --gates`: the three gates, a flaky rerun by test name; gate-changing (`spawn_sites.toml`), merged at a terminal | built 2026-11-21 (draft pull request, not merged; gate-changing, the operator merges at a terminal): `finisher_gates.rs` (the `Runner` trait, `SystemRunner` the one new spawn site, class `operator-cli`); cargo test with `--no-fail-fast` so one failing binary hides no other, failing names read from `test ... FAILED` and the `failures:` lists, one rerun `cargo test --workspace --locked --no-fail-fast -- <names> --exact`, a name passes only when the rerun shows it `ok`; a program that cannot be started (`cargo` not found) fails its gate and says so; stops at the first failing gate, prints its last 40 lines, exit 1; `--body` ticks the gates and names flaky tests; the lint run by `current_exe()`. Tests: AC-15 with a fake runner (fail then pass = flaky, fail twice, first failure stops the rest, cargo not found, a rerun that runs nothing fails) and the body. Used live on its own branch: all three gates passed, and `service::app_update::tests::the_child_inherits_no_descriptor_above_two` failed, passed on its rerun and was reported flaky. Not verified: a run where cargo is missing (fake and a missing path only) |
