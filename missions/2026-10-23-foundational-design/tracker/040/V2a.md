---
state: built
date: 2026-10-28
title: "daemon fetcher with snapshots, `context/facts/**` zone"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell. The state cell did not map clearly to a state; `built` is the closest, and the cell is kept below as it was.

The row (line 303, under `The validator role and fact ratification (spec 040, approved 2026-10-28)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| V2a | daemon fetcher with snapshots, `context/facts/**` zone | fetcher built 2026-10-28 (`delphi-engine::fact_fetch`: SSRF checks on every hop with pinned addresses, size cap, cross-origin redirects recorded not followed, text-only, hash-pinned snapshots under `<home>/validation/snapshots`, `Tainted` text and opaque `Pointer`, cloaking check); the zone and read-deny were V1a's; not yet called by any tool (V2b) |
