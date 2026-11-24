---
state: merged
date: 2026-11-16
title: "what K4 to K6 did not close"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row. The state cell did not map clearly to a state; `merged` is the closest, and the cell is kept below as it was.

The row (line 1193, under `The knowledge loop: specs 112 to 119 (2026-11-14, approved)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 120 | left | what K4 to K6 did not close | `delphi app` now starts the app on `main`: K2's gate-file change (#714) had been merged only into its stack branch, and its commit (bf7fd764) was cherry-picked onto `main` in a pull request of its own. K4's gate file and its line in `merge-gates.toml` are merged (#745). Also not done: the end-to-end check on a machine by a person, and the buttons in the built app; the open questions of §11; a record for a connection closed before any handshake. Whether the red-team reviewed K1 and K4 before they merged (§9) is not recorded here |
