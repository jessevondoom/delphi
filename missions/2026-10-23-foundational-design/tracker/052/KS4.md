---
state: built
date: 2026-10-29
title: "contested pairs and review-due"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 347, under `Knowledge status, capture on contact and the digest (spec 052, approved 2026-10-28)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| KS4 | contested pairs and review-due | built: `library_review.rs` (`Library::contested_pairs`, `review_due`, operator `confirm_claim`), `digest_review.rs` (pairs and due claims as typed `items` of the `learn` card, ahead of captured candidates, one shared daily quota; actions keep, check, dismiss, and confirm for a due claim), `delphi claims review\|confirm`. Derived on read; nothing is written but the digested mark and a confirm's date. **Not built:** a pair is a word-overlap match (no model, so false alarms are possible), not 040's contradiction search; a pair does not itself mark claims contested; staged notes have no review-due; the console shows the items through the card's title and brief, not a side-by-side view |
