---
state: built
date: 2026-11-11
pr: 645
title: "wider fetch: 7a observed search-result URLs only (7b off, not built)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 899, under `Spec 098: where agents run (2026-11-10, approved)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| S1d | wider fetch: 7a observed search-result URLs only (7b off, not built) | built (PR open), behind `[research] ladder` (default false): `research_observed.rs` (the per-run observed set: only https URLs a search returned, no credentials, fragment dropped, query string kept exactly as supplied, bounded at 400, restorable from the session log); `LiveWeb` builds the set from each search's rows and, with the switch on, reads an observed off-list URL through `fact_fetch` (https on 443, public addresses only, pinned, cross-origin redirect refused, size cap, text only, cloaking refused) inside `fetch_per_host` (2) and `fetch_per_run` (10) caps; `research_status` gains `fetched_via` (`trusted` or `search:<provider>#<n>`); a run writes a `research.observed` event, and a host's `propose_kb_entry` claim URL is refetched only if the session's searches returned it, so its off-list host can be raised as a live `source` card (one per host, at most 5). Nothing reads a link out of a page (a scan test fails if the job pipeline starts to). NOT built: the older operator `/research` pipeline (`research.rs`) is not widened; no real page was fetched (the network call is covered by `fact_fetch`'s own tests, and a canned page stands in for it in the new tests); no live search provider was run; the S1c remainder is a separate slice (built in #645) **Flag `[research] ladder` pulled in S11 (spec 098 §13.2): the feature is always on.** |
