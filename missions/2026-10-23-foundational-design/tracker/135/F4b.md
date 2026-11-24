---
state: run
date: 2026-11-21
pr: 537
title: "the F4 rerun with toolchains configured and the affected crate's tests"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 1343, under `The forge, first cut (spec 135, approved in advance 2026-11-20)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 135 | F4b | the F4 rerun with toolchains configured and the affected crate's tests | run 2026-11-21 (`missions/2026-11-20-forge/F4-REPORT.md` section F4b): arm B (Round 0, call sites and the crate's tests really run in the sandbox) caught the known defect in 3 of 12 reviews (+2 partial, 1 no answer), the same count as F4 but with the first #537 catch (kimi-k3, after asking for the denial strings); 43% more tokens, the tests caught nothing. Arm C (read tools) not measured: the background probe of agy 1.5.14 failed, so the seats stayed tool-less (C = a gemini repeat). Exposed: §14.2 needs `/private/etc/ssl`, Xcode and its licence plist on macOS and must say "ratify"; a startup failure still reports `tests_run: true`. F3 still not warranted |
