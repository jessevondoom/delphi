---
state: built
date: 2026-11-11
title: "archived-copy lookup, labelled version, searchable, zero-origin invariant"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 965, under `Spec 101: library practice for research (2026-11-11, approved)`):

| Slice | Content | Hours (planned) | State |
| :-- | :-- | --: | :-- |
| C | archived-copy lookup, labelled version, searchable, zero-origin invariant | 18-20 | built (PR open), behind `[research] archive_fallback` (default false): `library_archive.rs` asks one pinned endpoint (the Internet Archive's Availability API, https; verified live 2026-11-11) whether a public source the daemon fetched has a copy, read-only, never asks it to save; the copy address is built from the 14-digit timestamp and the source's own URL, never the answer's `url`; the copy is stored as the next VERSION of the source with an `archived_copy` record and an H1 and banner ("Archived copy from <date>, not the live page") that every search chunk inherits; origin stays the original page's; `cites_of` skips a source whose current version is a copy, so it adds no origin and no trusted source (named test), and `support.archived` marks the claim "checked against archived copy of <date>"; `Channel::ArchiveLookup` is registered in the egress enumeration as `Wired::Yes` and a private or unreadable source sends nothing (zero requests, tested); `delphi sources archive <id \| url>` is the operator's explicit act, and `sources show` looks on its own for a public source found gone with no readable text when the switch is on. NOT built: a trigger from a lost quote (the older version in `versions/` already holds it), a trigger inside the daemon's host-claim refetch (a claim is public only after a daemon refetch finds its quote, so a never-fetched URL sends nothing), the search log line for an archive lookup (slice A's log is per research run and this runs outside one), `archive: unavailable` in the coverage report. The `id_` raw-copy form works but is not in the archive's own documentation (NOT VALIDATED); rate limits, terms and authentication are not stated on the API's page. Nothing ran against the live archive in a test (a loopback fixture stands in). Decided 2026-11-11: archive lookups stay operator-triggered and are not journalled (§5 item 9). |
