---
state: not-built
date: 2026-10-30
title: "spec 057 §5.1: word-set cache behind `[library.pages] word_cache`, only if SK8e shows read, hash and tokenize at 70% or more of gather time"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell. The state cell did not map clearly to a state; `not-built` is the closest, and the cell is kept below as it was.

The row (line 378, under `The central library (spec 055, approved: ratified 2026-10-29)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| SK8f | spec 057 §5.1: word-set cache behind `[library.pages] word_cache`, only if SK8e shows read, hash and tokenize at 70% or more of gather time | not warranted by the SK8e baseline (51% at 1,000 sources); closed unbuilt 2026-10-30. Reopen if a real library measures at 70% or more, or if gather exceeds the latency the console needs. Tokenizing and meta parsing, not file reads, are now the larger share |
