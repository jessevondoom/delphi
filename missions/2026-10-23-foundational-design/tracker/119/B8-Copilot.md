---
state: built
date: 2026-11-16
title: "Copilot's fetch tool in its worker files (§16 item 3)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The table named this slice `B8 (Copilot)` of spec `119`.

The row (line 1181, under `The knowledge loop: specs 112 to 119 (2026-11-14, approved)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 119 | B8 (Copilot) | Copilot's fetch tool in its worker files (§16 item 3) | built (pull request open): checked live with the Copilot CLI signed in on the build machine (`--version` 1.0.108): an agent file lists the fetch tool by its own name, `web_fetch` (the alias `web` gives the same tool; a file without either has no fetch tool), and a listed `web_fetch` ran once against an allowed address. `copilot_tools` now takes the host's tools from `worker_host_tools`, so the scout's Copilot worker file lists `"read", "search", "web_fetch"` before its Delphi tools; the investigator's has no tools line since 116 G11 and inherits the session's; no other role's names it. The B8 test pins this for Copilot instead of refusing it, and the spec 110 test of Copilot's tool lines allows it for the scout only. `ecosystem/` holds Claude Code's files only and did not change; Copilot's files are written by `delphi plugin export --host copilot`, and the exported scout file was checked live too. Six Copilot requests in all |
