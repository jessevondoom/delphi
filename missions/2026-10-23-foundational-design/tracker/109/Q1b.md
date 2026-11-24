---
state: built
date: 2026-11-14
title: "index registered projects as a per-project layer (the operator's decision of 2026-11-13)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 1040, under `Host experience program: specs 104 to 110 (2026-11-13, approved in advance)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 109 | Q1b | index registered projects as a per-project layer (the operator's decision of 2026-11-13) | built (pull request open): each registered project's scaffold folders and layer context and knowledge are indexed as zone `project` under `projects/<id>/` at `external_untrusted`, readable only with the session's own project id in the query (tests prove another project's layer and a session with no project get nothing), dropped at the next sync once unregistered, and the project legs ask the retrieval service with the word ranker as fallback; the "frontmatter the daemon wrote" exception of §11.4 is not built (nothing marks such frontmatter) |
