---
state: built
date: 2026-10-29
title: "capture on contact: accepted answer, Council verdict, resolved decision, research run"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 345, under `Knowledge status, capture on contact and the digest (spec 052, approved 2026-10-28)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| KS2 | capture on contact: accepted answer, Council verdict, resolved decision, research run | built: triggers `council_verdict`, `council_minority`, `council_claim` (each seat's claim, on the verdict event), `decision_resolved`, `accepted_answer` (new `answer.accept` RPC, fired silently by `/pin` and `/export`; the daemon marks `answer.accepted`), `research_run` (daemon appends `research.finished`). Staged-only, `noted`, capped at 30 per mission, deduplicated. "Continue from an answer" is not captured (every message would count) |
