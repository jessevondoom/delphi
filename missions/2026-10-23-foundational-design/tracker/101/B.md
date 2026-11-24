---
state: built
date: 2026-11-11
title: "local Retraction Watch snapshot, DOI match, `retracted` marks, minimal normaliser"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 964, under `Spec 101: library practice for research (2026-11-11, approved)`):

| Slice | Content | Hours (planned) | State |
| :-- | :-- | --: | :-- |
| B | local Retraction Watch snapshot, DOI match, `retracted` marks, minimal normaliser | 14-16 | built (PR open), behind `[research] retractions` (default false): `delphi research retractions import <file>\|status` (a file on disk, never a URL; no call to any service), the local index under the library, a source's DOI (its record or a `doi.org` address) looked up at capture and re-check, a `retraction` mark (`retracted` and `expression_of_concern` stop the source counting as an origin, a `retracted` source contests a claim nothing else supports, `corrected` is a label), a mark carried across a new version and lifted only by a later snapshot, the digest line, the coverage-report line, a `delphi doctor` warning when the snapshot is stale or missing, the minimal DOI normaliser (`doi.rs`). Verified from Crossref's documentation page and the repository README: CSV, the column names, the four `RetractionNature` values, the daily update. NOT validated: the licence (neither page states one; the spec's CC0 is unconfirmed), the data file's name, the date format (the reader is tolerant). Not built: a URL import, a per-DOI call (refused by design). Nothing has run against the real file or a live host |
