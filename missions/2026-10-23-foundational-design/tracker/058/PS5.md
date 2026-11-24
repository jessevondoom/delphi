---
state: built
date: 2026-10-30
title: "Project-to-home promotion proposals, summary/provenance, duplicate/conflict review, accept/reject/withdraw"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 413, under `Project scopes, scaffolding, indexes and promotion (spec 058, approved 2026-10-30)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| PS5 | Project-to-home promotion proposals, summary/provenance, duplicate/conflict review, accept/reject/withdraw | built 2026-10-30 in `promote.rs`: deterministic preflight with stable refusal codes, a `learn.proposed` `op:promote` proposal (summary, provenance, destination `home:memory/promoted`, home version, duplicate/related/conflict matches shown, never merged), accept under a lock with CAS on proposal and home version and the preflight re-run inside acceptance (058-A1), reject (outcome and masked reason only), withdraw (retires, deletes nothing); console-only over `retro.promote`/`retro.withdraw`, `delphi retro promote/promotions/withdraw`. Deferred: PS6 trusted-source references, PS7 `/promote` alias and UX, agent-side proposal tool, `ui/` console view |
