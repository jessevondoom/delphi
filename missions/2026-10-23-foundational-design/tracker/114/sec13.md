---
state: built
date: 2026-11-16
title: "amendment: one output cap of 8,000 tokens for the plan, page and answer calls (the operator's words of 2026-11-16: \"make it 8K for all 3 limits and we can readj..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The state cell did not map clearly to a state; `built` is the closest, and the cell is kept below as it was. The table named this slice `§13` of spec `114`.

The row (line 1120, under `The knowledge loop: specs 112 to 119 (2026-11-14, approved)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 114 | §13 | amendment: one output cap of 8,000 tokens for the plan, page and answer calls (the operator's words of 2026-11-16: "make it 8K for all 3 limits and we can readjust"), from M7's finding that 900 and 1,500 cut a reasoning model's replies | amendment written and built (pull request open): `PLAN_OUTPUT`, `CLAIM_OUTPUT` and `ANSWER_OUTPUT` in `research_host.rs` are one 8,000 constant; approved when the operator merges it. Not rerun live; a run-level token budget is not built |
