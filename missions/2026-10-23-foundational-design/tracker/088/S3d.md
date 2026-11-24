---
state: built
date: 2026-11-22
title: "(amendment §14, 2026-11-22, OD-3's fallback for the group pages; CI velocity plan item J2) the compile writes the index and the JSON only; `delphi specs view [<..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 772, under `Spec 088: a spec compile step (2026-11-07, approved for S0 to S3)`):

| Slice | Content | Hours (likely) | State |
| :-- | :-- | --: | :-- |
| S3d | (amendment §14, 2026-11-22, OD-3's fallback for the group pages; CI velocity plan item J2) the compile writes the index and the JSON only; `delphi specs view [<group>]` prints a group's page on demand; the index links no group page; the nine pages deleted; CONTRIBUTING, `llms.txt` | 5 | built 2026-11-22 (draft pull request; approved in advance by the operator's "keep building optimistically in the order you defined", text not read by the operator, no Council): `spec_views::COMMITTED`, `spec_compile::view_in`, tests in `spec_compile.rs`, `spec_views.rs` and `compiled_specs.rs`; the index stays committed (AGENTS.md and llms.txt link it; OD-14a); no change to `merge/`. Not verified: the fall in conflicts (count for two weeks with §13.7 V4) |
