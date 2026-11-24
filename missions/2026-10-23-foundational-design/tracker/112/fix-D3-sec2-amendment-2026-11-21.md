---
state: built
date: 2026-11-21
title: "each run of a topic keeps its own result"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The table named this slice `fix D3 (§2 amendment 2026-11-21)` of spec `112`.

The row (line 1101, under `The knowledge loop: specs 112 to 119 (2026-11-14, approved)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 112 | fix D3 (§2 amendment 2026-11-21) | each run of a topic keeps its own result | built (branch `fix-112-research-run-results`, PR pending): the record keeps `results` (each run's own result and answer, newest last, at most eight) beside `result`/`result_run`, so two runs of the same question (concurrent, or with `new: true`) each read back their own; a dropped or pre-amendment run still says `superseded_by`; old records load; the `new` description says it only skips folding into an overlapping topic |
