---
state: built
date: 2026-10-29
title: "claim records, mission-start pull by pointer"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 368, under `The central library (spec 055, approved: ratified 2026-10-29)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| SK3 | claim records, mission-start pull by pointer | built (`library_claims.rs`, `delphi claims add\|anchor\|list\|show\|validate\|pull`, the pull in `Engine::start_task`); status derived by `knowledge_status::derive_fact`, `cited_by` on sources and claims, anchors re-resolved on a new version, retirement contests the claims it alone supports, purge makes anchors pointer-only, reliability counts in `sources show`. **Claims view built** in the knowledge window (`ui/src/library.ts`; the daemon's status word as the 053 glyph with its age, anchors, quotes and `cited_by`, from `delphi claims list --json`). **Not built:** config keys for `private_to_models` (private-sourced claim text is withheld, the stricter reading); no validator role path calls `validate_claim` yet (operator only); no dispute action (SK5) |
