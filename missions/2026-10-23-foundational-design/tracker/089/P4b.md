---
state: built
date: 2026-11-21
title: "`agy` fix: one retry after a CLI error, discard message names the paths (guard unchanged by decision; watchdog deferred)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 832, under `Spec 089: Panel hardening from the E2 run (2026-11-06, approved)`):

| Slice | Content | Hours | State |
| :-- | :-- | --: | :-- |
| P4b | `agy` fix: one retry after a CLI error, discard message names the paths (guard unchanged by decision; watchdog deferred) | 6-10 | built (PR open): retry and path message; guard unchanged by decision; live capture held. OD-4 decided 2026-11-21 (operator: "follow your recommendations"), amendment §18: the guard is narrowed by seat kind; a toolless seat keeps its answer on a checkout change and writes `harness.checkout_changed` (`discarded: false`); a seat with tools still discards; timing inference not built; branch `fix-089-guard-by-seat-kind` |
