---
state: built
date: 2026-11-21
title: "call sites by code and a Round 0 where the seat asks for up to 5 identifiers or strings; `[forge] review_context_chars`"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 1340, under `The forge, first cut (spec 135, approved in advance 2026-11-20)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 135 | F2d | call sites by code and a Round 0 where the seat asks for up to 5 identifiers or strings; `[forge] review_context_chars` | built 2026-11-21 (draft pull request, not merged): `review_job.rs` `Corpus::load` (the review copy's text files, read in-process before any sandboxed command runs; `.git`, `target`, `node_modules` skipped; a symbolic link only when its canonical target is a file inside the copy and not under a skipped directory; 512 KiB per file, 64 MiB in all), `diff_shape` (functions the diff's `.rs` lines and hunk headers name), call sites by a word-boundary `name(` search outside the diff's added lines (3 per function, 12 lines each), Round 0 (`brief_round0`, `parse_needs`: at most 5 items, each a plain identifier or a quoted string of at most 80 characters; others refused and named), fixed-string search (3 hits per item, a definition first), all within `[forge] review_context_chars` (default 12000, 0 turns it off; config example, schema, reference); the excerpts reach Round 1 fenced as data; the result gains `context` (functions, counts, refused items). Verified: AC-14 with a scripted seat (unit tests, including links out of the copy and into `.git`). Not verified: a live seat; whether the context changes what a reviewer finds (F4b). A review calls its seat up to three times (Round 0, Rounds 1 and 2), so `host.review` checks and reserves `review_ceiling_tokens` (a Panel's two-round ceiling and half again) before anything is spent; the Panel's own ceiling is unchanged (test: a cap that fits a Panel exactly refuses a review, nothing spent) |
