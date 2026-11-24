---
state: built
date: 2026-10-30
title: "supersession candidates and ratification"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 289, under `Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 050 | ML4 | supersession candidates and ratification | built: `suggest_rule` `add` may name up to 3 rule ids in `supersedes`; `delphi retro show`/`/retro` list candidates (named, or at least 30% word overlap with the new text), shown never applied; ratifying the `add` retires only the named rules from `heuristics.md` and records their text in `learn.proposal_resolved` (`detail.retired`); security rules are never retired. `delphi lint` does not list pairs yet |
