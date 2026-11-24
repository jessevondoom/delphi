---
state: built
date: 2026-11-11
title: "S1d follow-up (§7.5 item 7a, §12 item 16, 2026-11-11): list the observed-URL fetch as an EXEMPT path with its reason in the egress enumeration, and the three te..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 900, under `Spec 098: where agents run (2026-11-10, approved)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| S1e | S1d follow-up (§7.5 item 7a, §12 item 16, 2026-11-11): list the observed-URL fetch as an EXEMPT path with its reason in the egress enumeration, and the three tests (only the daemon's provider search adds to the observed set; nothing appended to a fetched URL or read from page text; a host-declared search, a claim's `source_url` or a model URL never reaches the fetcher unless the provider returned it) | built in S11: the fetch is listed in `egress.rs` as an exempt path with its reason and seven named proof tests; the three tests are `only_the_daemons_own_search_adds_to_the_observed_set` (a scan, mutation-checked), `a_fetched_url_is_exactly_the_one_the_provider_returned`, and `a_url_a_model_wrote_never_reaches_the_fetcher` with `a_host_declared_search_carries_no_url_and_never_touches_the_observed_set` |
