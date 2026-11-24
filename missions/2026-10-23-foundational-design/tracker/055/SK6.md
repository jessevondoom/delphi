---
state: built
date: 2026-10-30
title: "acquisition and standing rules"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 371, under `The central library (spec 055, approved: ratified 2026-10-29)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| SK6 | acquisition and standing rules | built (`library_acquire.rs`, `library_acquire_run.rs`, `library_reading.rs`, `sources_acquire_cmd.rs`): a daily daemon job (`[library.acquire]`, off by default) proposes sources from the archives in `context/sources/archives.toml` (+ `operator/archives.toml`) as ONE digest card (accept / skip / take-and-make-a-rule / not now); a standing rule takes matching items up to a size; capture is full text under the archive's own public-domain rule (basis recorded, archive notice and attribution kept, rate limits and host list held), byte budget stops and evicts nothing, scout tokens are a share of the trailing week and it only widens topics; `delphi sources acquire …` and `reading-list …`. Not built: images (the fetcher is text-only); archives beyond Gutenberg; the Gutendex answer shape is unverified against the live service |
