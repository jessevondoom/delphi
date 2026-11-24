---
state: built
date: 2026-11-20
title: "seed table of the 17 latest-generation Copilot models and GPT-OSS on agy (`model_capabilities.toml`), the catalog module (`model_catalog.rs`: `session/new` read..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 1305, under `The Council and the Panel from a host: spec 123 (2026-11-16)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 132 | C3a | seed table of the 17 latest-generation Copilot models and GPT-OSS on agy (`model_capabilities.toml`), the catalog module (`model_catalog.rs`: `session/new` read, cache per CLI version, cost class), family-table rows `kimi-*` and `mai-*`, config family names from the family table plus `local`, mixed-vendor Copilot endpoint refused | built (pull request open). Premise answered by the 2026-11-20 model survey on the operator's account: `initialize` plus `session/new` sends no prompt and billed no premium request. OD-5 taken AGAINST its recommendation on the operator's aim of off-family coverage (2026-11-20): `kimi` and `mai` are their own families, flagged in the pull request for the operator. The config checks names against the SHIPPED family table only (a workspace's own `model-families.toml` is the engine's) |
