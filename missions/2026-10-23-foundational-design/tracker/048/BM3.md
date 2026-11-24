---
state: built
date: 2026-11-07
title: "context retention eval"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 275, under `Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 048 | BM3 | context retention eval | built 2026-11-07 (PR pending, stacked on BM2): `train_retention.rs` (`delphi train retention run`): a seeded session with N exact facts told early and routine work after, five scripted conditions (no compaction, summary, summary with a handle to the dropped turns kept whole, the same with the handle ignored, a new session), a scripted model that answers only from what it sees and follows a handle only if it is the kind that does; chart-ready table and `--json`. Reuses BM2's `train_items.rs` and `exact_items`. AC-5 tested (summary 0%, handle followed 100%, ignored 0%; the handle costs under 60 characters). Not built: a live run (needs the compaction, 045 H3), multi-turn scenario cases (`train run` does not run scenarios yet), digest forms (049 CX5) |
