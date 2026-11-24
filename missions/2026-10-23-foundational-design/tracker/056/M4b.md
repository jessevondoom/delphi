---
state: built
date: 2026-11-21
title: "ratchet and suggestions from retrospectives (the original M4)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 398, under `Discovered model assignment (spec 056, approved 2026-10-29)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| M4b | ratchet and suggestions from retrospectives (the original M4) | built (`delphi-engine/src/ratchet.rs`: a finished build (`run_build`) is a retrospective row, recorded with `assign::record_evidence` for its first attempt's pair (kind `build`, accepted = first-pass, cost = every builder pair's tokens × its multiplier); under `discovered` a `plan_following` role's row moves the ratchet in `local/model-ratchet.json`: down a class after `[ratchet] k` (5, in `model_classes.toml`) first-pass successes in a row, back up after one failure, a class below preferred stopped after two failures in a row there until `delphi agent clear <role>`; each move logged as `model.ratchet` and shown in /observe; `assign_for` adds the step for a plan-following role's build span, clamped between floor and preferred; `delphi agent grades` shows the ratchet and suggestions as counts (never applied). §8's ratchet tests pass. Not verified: no live build under `discovered` has run it; the engine path that writes `model.ratchet` is unit-tested in parts, not end to end; the console shows moves, not suggestions) |
