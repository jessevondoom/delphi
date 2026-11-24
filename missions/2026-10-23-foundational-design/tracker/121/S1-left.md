---
state: not-built
date: 2026-11-16
title: "what S1 did not build"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The state cell did not map clearly to a state; `not-built` is the closest, and the cell is kept below as it was. The table named this slice `S1 (left)` of spec `121`.

The row (line 1204, under `Data sensitivity: spec 121 (2026-11-15, draft)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 121 | S1 (left) | what S1 did not build | no meeting or mail connector imports anything yet, so the meeting and recipient rules are tested on `assign` only; handoffs, session summaries kept only as events, Council evidence, advice and decision briefs are not labelled (`handling::NOT_LABELLED`, `WRITES`); the given list is per mission and role, not per span; `Library::internal_confirmation` (115 §11.2) is not narrowed to `has_weight` yet; no console set/clear; the §9 item 2 query log is not built; the live dry-run read on the operator's workspace (§14) is the operator's. S2 and S3 not built |
