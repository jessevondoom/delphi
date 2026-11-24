---
state: built
date: 2026-11-22
title: "§5.3 fix: the tracker check (and the claim warning) also read `| <slice> | ... |` rows inside the section of a heading that names the spec; `tracker.rs` `rows`/..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 1348, under `The forge, first cut (spec 135, approved in advance 2026-11-20)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 135 | F1c | §5.3 fix: the tracker check (and the claim warning) also read `\| <slice> \| ... \|` rows inside the section of a heading that names the spec; `tracker.rs` `rows`/`find`/`sections_for`; the finding names the headings searched | built (this PR): unit tests on both shapes, S1 vs S16, sub-headings, ranges and amendments, and a sample of real tracker rows (094 S13-S16, 093 B1/B2, 135 F0-F2, 125 S1 and more); `forge finish --spec 094 --slice S16` now finds the row |
