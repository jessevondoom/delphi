---
state: built
date: 2026-11-01
title: "Encyclopedia Page as primary search result in `ui/src/archives.ts` rendered via `renderMarkdownLinks` and `paint`, immediate synthesis on search, TOC `#anchor` ..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 505, under `Encyclopedia search pages, inline media, and reputable expansion (spec 097, approved 2026-10-31)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| SK9d | Encyclopedia Page as primary search result in `ui/src/archives.ts` rendered via `renderMarkdownLinks` and `paint`, immediate synthesis on search, TOC `#anchor` jumping, disambiguation links, and `e` / `r` / `f` controls |and `e` / `r` / `f` controls | built; corrected 2026-11-01: a search never set `pg.q`, so `gotPage` dropped every page and the raw list always won. Submit now sets it, the page takes the view when it arrives unless `f` chose raw, and a late page for an older query is dropped (`ui/test/pages.test.ts`) |
