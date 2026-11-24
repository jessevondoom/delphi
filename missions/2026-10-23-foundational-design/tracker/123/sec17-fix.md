---
state: built
date: 2026-11-21
title: "D5: a Panel's Delphi-run seat in the Round 1 barrier reports `waiting_on_barrier` (not `answered`), an abstention `abstained` at once; D6: `earlier_rulings: fal..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell. The table named this slice `§17 fix` of spec `123`.

The row (line 1232, under `The Council and the Panel from a host: spec 123 (2026-11-16)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 123 | §17 fix | D5: a Panel's Delphi-run seat in the Round 1 barrier reports `waiting_on_barrier` (not `answered`), an abstention `abstained` at once; D6: `earlier_rulings: false` on `council_open` / `request_council` turns earlier rulings off for one call, recorded as `earlier_rulings_off: by the caller` | built (pull request open, 2026-11-21): `council_panel::state_of` (and the `council_status` description); `council::earlier_rulings_off`, `CouncilOpts::earlier_off`, `Panel::earlier_off`, `council_earlier::front_off`; §17 amendment. Not verified: a live Panel or Council |
