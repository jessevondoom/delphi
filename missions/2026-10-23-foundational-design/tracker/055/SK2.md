---
state: not-built
date: 2026-10-29
title: "excerpts, anchors, bibliography export"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The state cell did not map clearly to a state; `not-built` is the closest, and the cell is kept below as it was.

The row (line 367, under `The central library (spec 055, approved: ratified 2026-10-29)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| SK2 | excerpts, anchors, bibliography export | built (`library_excerpts.rs`, `library_bib.rs`, `delphi sources excerpt add\|list`, `delphi sources bib`); also closes SK1's metadata readers (`library_meta.rs`: Wikipedia, DOI landing page, Gutenberg header) and the 30-day recheck on open (`library_recheck.rs`, run by `sources show`). **Sources view built** in the knowledge window (`ui/src/library.ts`, `ctrl+l`; list, detail and excerpts from `delphi sources list|excerpt list --json`; console only, the narrator window stays read-only). **Not built:** APA/MLA/Chicago are compact renderings, with CSL-JSON and BibTeX for exact styles; the recheck's change test is a hash comparison, the fetcher takes no request headers |
