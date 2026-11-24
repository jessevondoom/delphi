---
state: approved
date: 2026-11-21
title: "(amendment §13, 2026-11-21, answers 094 OD-11) no whole-set aggregate in a committed view: drop the \"N specs in M groups\" and \"N specs.\" lines and show group id..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell. The state cell did not map clearly to a state; `approved` is the closest, and the cell is kept below as it was.

The row (line 771, under `Spec 088: a spec compile step (2026-11-07, approved for S0 to S3)`):

| Slice | Content | Hours (likely) | State |
| :-- | :-- | --: | :-- |
| S3c | (amendment §13, 2026-11-21, answers 094 OD-11) no whole-set aggregate in a committed view: drop the "N specs in M groups" and "N specs." lines and show group ids in index rows, so two spec pull requests can no longer merge cleanly into a stale count (the red `main` of 2026-11-20, fixed by hand in #806); tests 20 to 23, including the 2026-11-20 fixture; one-time recompile of `compiled/` | 2.5 | amendment approved in advance on the operator's "start building in the order you see fit" (2026-11-21; text not read by the operator, no Council); built (PR open): the index opens with "`delphi specs list` counts them", each group page with "`delphi specs list --group <id>` lists this group", index rows link the group by id; tests 20 to 23; `compiled/` recompiled (no JSON changed); V4 (four weeks of `compiled_specs` failures on `main`) not started |
