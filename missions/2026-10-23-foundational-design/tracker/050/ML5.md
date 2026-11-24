---
state: built
date: 2026-10-30
title: "trace mining report; wires 045 T11 / H8"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 290, under `Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 050 | ML5 | trace mining report; wires 045 T11 / H8 | built 2026-10-30: `delphi-engine/src/trace_mine.rs` (pure `mine`) and `delphi retro mine [--mission <id>] [--json]` rank failed or looping spans by a cause key computed from event kinds and closed-set codes only (`loop.hard_stop`, `loop.nudge`, `loop.repeated_tool`, `gate.red.<n>`, `task.failed`, `card.denied`, `card.timed_out`, `escalation`, `tool.repeated_failure`), by distinct spans then tokens then key, citing event seq and kind; writes `reports/trace-mining.json` in the mission, applies nothing. Not built: the T11 skill wiring (045 H8), `delphi train harvest` hand-off, refused-compaction events (none emitted until H3) |
