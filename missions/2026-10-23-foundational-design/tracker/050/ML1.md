---
state: built
date: 2026-10-28
title: "memory write-path audit and lint (first, before 045 H3 adds a write path)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 286, under `Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 050 | ML1 | memory write-path audit and lint (first, before 045 H3 adds a write path) | built: `delphi-engine/src/memory_audit.rs` lists every memory write path with its proof tests; `delphi lint --repo` rule `memory-write-sites` fails on an unlisted write or a missing proof |
