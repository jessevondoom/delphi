---
state: built
date: 2026-11-21
title: "measured attributes on the candidate; classifier and eligibility read them; cost attributes refused as class conditions; no-input test for a `context/` note (am..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 396, under `Discovered model assignment (spec 056, approved 2026-10-29)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| M1a | measured attributes on the candidate; classifier and eligibility read them; cost attributes refused as class conditions; no-input test for a `context/` note (amendment §10.2, §10.3, §10.7) | built (`models::Measured` on every candidate from spec 132's seed table: prompt budget, JSON-in-text, recall as the C3 tail at its fraction, served-model match kept across refreshes, token accounting (new seed column, set only where the survey noted it), with date, carrier, version and source; `classify`: `wide` needs a measured budget at `[wide] min_context_ratio` times the family median and recall at 0.8 or more, a served-model mismatch makes the family `unknown` and the model `unclassified`, deep/steady/quick stay the tier prior; `assign::pick` rules a pair out when the prompt exceeds its measured budget or a schema answer lacks a valid JSON record (`RouteRequest::needs_json`); a class rule naming a cost key fails to parse; tests: AC-A1, AC-A2, AC-A3, static routes unchanged). Not wired: no caller sets `min_context_tokens` or `needs_json` yet, nothing writes `served_match` from receipts yet, spec 131's budget margin is not built, so under `discovered` these checks are inert in a live run today |
