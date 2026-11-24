---
state: built
date: 2026-11-11
pr: 626
title: "the search log, flow summary, `delphi research log`"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 963, under `Spec 101: library practice for research (2026-11-11, approved)`):

| Slice | Content | Hours (planned) | State |
| :-- | :-- | --: | :-- |
| A | the search log, flow summary, `delphi research log` | 18-24 | built (PR open), behind `[research] search_log` (default off): `search_log.rs` (closed source set, per-line balance, flow, declared lines, private-by-inheritance, persistence), the runner records every search of the host research path (`research_run.rs`; the console `delphi research` path, `research.rs`, is not covered), `host_searches` on `request_research`, the result schema, `delphi research log <id>`, and a net test that nothing reads the log as a criterion. Not built or unverified: nothing has run against a live search provider or host; the shared egress check (#626's `egress::check`) is not on main, so the log uses the library's own outbound check (`library::may_leave`); the corroboration pass still treats an unknown private flag as public (098 S1a) **Flag `[research] search_log` pulled in S11 (spec 098 §13.2): the feature is always on.** |
