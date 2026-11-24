---
state: not-built
date: 2026-10-29
title: "latch, audit cards, report, retract, hash chain"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The state cell did not map clearly to a state; `not-built` is the closest, and the cell is kept below as it was.

The row (line 309, under `The validator role and fact ratification (spec 040, approved 2026-10-28)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| V2c | latch, audit cards, report, retract, hash chain | built 2026-10-29. `fact_audit` (audit journal of ids/counts/classes, never claim text; blind-audit cards: every would-ratify up to 50 a week then sampled, at most 5 negative cards a week; the answer is journalled before the reveal; a false answer trips the latch to advisory, withdraws PROMOTE and quarantines the batch; an audit unanswered for 7 days quarantines its fact; `fact_read_events` builds `fact.read`), `delphi validation audit|report|latch raise|retract --all` (operator-only, refused in an agent seat; `report` writes promote.json from the pre-registered conditions and never runs the validator; raising needs a reason and, for ratify, PROMOTE or a logged `--force`), `FactStore` batch/quarantine/retract (kb hides quarantined), §21.6 hash chain in `delphi-core` (`prev`/`h` on events, `verify_chain`, head kept in `state.db`) checked by `delphi doctor`. Not built: an agent-facing kb tool that emits `fact.read` (none exists), `needs` backlog line, `check record`, live token-share cap |
