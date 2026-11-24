---
state: built
date: 2026-11-11
title: "API reference-list chaining, one hop, bounds, noted-only"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 966, under `Spec 101: library practice for research (2026-11-11, approved)`):

| Slice | Content | Hours (planned) | State |
| :-- | :-- | --: | :-- |
| D | API reference-list chaining, one hop, bounds, noted-only | 20-24 | built (PR open), behind `[research] citation_chaining` (default off): `research_chain.rs` (config, the request builder, the parser: only `doi:` ids that the normaliser accepts, as data; the seed and repeats left out; the answer cut at 5,000 records), the pinned index OpenCitations' Index API v2 (`[research] citation_endpoint`, https only), `Channel::CitationLookup` registered `Wired::Yes` in `egress.rs`, the seed from `LiveLibrary::chainable_doi` (public, readable, fetched by the daemon, names a DOI; `egress::check` on its record), `chain_pass` in `research_run.rs` (depth, fanout, max per seed, max calls; a work seen is skipped; each call one `citation:opencitations` log line with purpose `chain`; a claim resting on a private source is a `withheld` line with no text), kept works as `noted` leads at `https://doi.org/<doi>`, the coverage line `citations`. NOT built: reading a chained work as a PAGE (a DOI cannot be fetched under the fetcher's rules, `doi.org` redirects across origins, and the doi.org Handle endpoint is not a pinned endpoint of spec 101 §7); the older operator `/research` pipeline; a run against the live index in CI (a loopback server stands in; one read-only live check was made by hand) |
