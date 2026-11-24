---
state: built
date: 2026-10-30
title: "validity, stale list in `/retro`, type-aware selection"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 288, under `Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 050 | ML3 | validity, stale list in `/retro`, type-aware selection | built: `valid_until` on a staged note (agent `note` tool only, OD-5) is excluded from kb search after its date; `delphi retro stale` and `/retro stale` list expired notes and rules files past 90 days; `kb search --staged --kind` filters by kind, facts outrank episodes, optional episode budget |
