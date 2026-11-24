---
state: built
date: 2026-11-21
title: "fix (§6.3 amendment, operator 2026-11-21): a read-only span is offered none of `record_change`, `raise_decision`, `request_research`, `ratify_fact` and each cal..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 460, under `Delphi for host agents over MCP (spec 062, approved 2026-10-31)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| F-RO | fix (§6.3 amendment, operator 2026-11-21): a read-only span is offered none of `record_change`, `raise_decision`, `request_research`, `ratify_fact` and each call is refused by name (`engine::outbound_withheld`); a span at `federated_peer` or lower loses the first three (GOVERNANCE §4.3; `ratify_fact` stays for the validator, spec 040); `dispatch_agent` unchanged | built (pull request open) |
