---
state: built
date: 2026-10-29
title: "one status table (noted, checking, validated, contested, disproved, retired) and the labels everywhere"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 344, under `Knowledge status, capture on contact and the digest (spec 052, approved 2026-10-28)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| KS1 | one status table (noted, checking, validated, contested, disproved, retired) and the labels everywhere | built (display only): `delphi-core/src/knowledge_status.rs` derives the status; `kb` results carry `status` and `label`; operator text says "not validated", not "untrusted" (console strings fixed 2026-10-29: the search list and best-match card show the six words and `validated · vouched by <org>` from `knowledge_status`, `/context` no longer says "untrusted"). `checking`, `contested`, `disproved` are in the table but not yet derived (no records exist) |
