---
state: built
date: 2026-10-30
title: "spec 057: search, gather, write, cache by source hash, `delphi pages search`"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 375, under `The central library (spec 055, approved: ratified 2026-10-29)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| SK8c | spec 057: search, gather, write, cache by source hash, `delphi pages search` | built (`library_page_search.rs`, `page_run.rs`, `pages.*` RPCs, `delphi pages search\|list\|pin\|unpin`, `[library.pages]`): gather by content words (claims ≤ 12, sources ≤ 6; private left out of external pages and queries, and a query naming a private source is not sent); the writer span on role `writer` writes the draft, `build_page_checked` checks it; cache `pages/<key>.md` + sidecar keyed by query, material hashes, writer identity, template version and scope, stale on changed or gone material, expired `external_ttl`, or a new matching record; pinning, LRU eviction under one twentieth of the library budget, history kept first; a double failure cached 1 hour; `per_day` and the validator share (040 §20, journal) gate a new page, never a cached one; external search through the ratified `[search]` engines, captures noted, not validated |
