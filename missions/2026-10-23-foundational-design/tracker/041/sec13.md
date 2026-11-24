---
state: built
date: 2026-11-16
title: "amendment (operator, 2026-11-16): `delphi route` reads the whole task, and a count of corrections"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The table named this slice `§13` of spec `041`.

The row (line 157, under `Agent registry, routing and role extensions (spec 041, approved 2026-10-27)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| §13 | amendment (operator, 2026-11-16): `delphi route` reads the whole task, and a count of corrections | built (pull request open): `task_of` no longer skips the first argument (it dropped a task's first word and, after `--home`, read the home's path as the task: the audit's "advocate constant"); `route.decided` also records `named`, `research_default` and `ranked`; `delphi route --corrections` counts named-versus-ranked pairs over every mission, printing no task and changing nothing |
