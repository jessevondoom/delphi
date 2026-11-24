---
state: merged
date: 2026-11-16
title: "`max` on the image method, through the daemon, the app's bridge and window"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The table named this slice `P3 (part one)` of spec `118`.

The row (line 1158, under `The knowledge loop: specs 112 to 119 (2026-11-14, approved)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 118 | P3 (part one) | `max` on the image method, through the daemon, the app's bridge and window | built and merged (on `main` by #724, which landed the stack; built in #719): 1 to 1,600 pixels a side, 160 when absent or not a number, never enlarged; the bridge clamps it too; a picture is asked for at the width of its text column. The bridge had let the app ask for a library picture (the daemon's `kb.get` serves the operator view), against E-4: it now refuses `src=lib:` until P1's licence record exists |
