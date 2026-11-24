---
state: built
date: 2026-11-21
title: "`delphi doctor` line per Copilot endpoint (models found or missing in the cached catalog, cost class for display), run-time refusal of a tier whose model a fres..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 1306, under `The Council and the Panel from a host: spec 123 (2026-11-16)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 132 | C3b | `delphi doctor` line per Copilot endpoint (models found or missing in the cached catalog, cost class for display), run-time refusal of a tier whose model a fresh catalog lacks, setup offers `copilot-kimi` and `copilot-mai`, cost class in the Panel plan | built (pull request open). `delphi doctor` uses the cached catalog when it was read on the installed CLI version within a day, else reads it once (`copilot --acp` `session/new`, no prompt) and caches it; a span only reads the cache (no cache: nothing judged). A missing or disabled model refuses that tier before the CLI starts, never swapped. The Panel plan shows model, multiplier and cost class, says `premium` in its note, and does not propose an endpoint whose model a fresh catalog lacks; a test holds the choice of seats equal across cost classes (the Council's rule: cost class is display and cost input only). Not verified: a live `delphi doctor` against the real account and CLI; the Panel plan's tier per role is the role's default tier, which may differ from what `council_open` runs |
