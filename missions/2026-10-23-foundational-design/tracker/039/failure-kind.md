---
state: built
date: 2026-11-22
title: "`tools::failure_kind` never reads policy words from a command's echoed `$ <command>` line: a failed command whose own command line says \"was denied\", \"does not ..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell. The state cell did not map clearly to a state; `built` is the closest, and the cell is kept below as it was.

The row (line 124, under `Central error log (spec 039, approved, 2026-10-27)`):

| Spec | Slice | Delivers | State |
| :-- | :-- | :-- | :-- |
| 039 | failure-kind | `tools::failure_kind` never reads policy words from a command's echoed `$ <command>` line: a failed command whose own command line says "was denied", "does not authorize" or " must be " was counted `denied` or `invalid_args`, the reverse of the lower bound in §3 (found by four model families in the spec 135 F4 review). For a `$ ` result only `exec`'s own status line on the second line can make it a `timeout`; anything else is `tool_error`. `tool_failed`, `tool.result` and `harness.bridge_call` `failure` all use the one function | fixed 2026-11-22 (PR pending) |
