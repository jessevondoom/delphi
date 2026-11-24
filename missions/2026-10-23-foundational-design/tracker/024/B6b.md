---
state: built
date: 2026-10-27
title: "OD-10 (2026-10-27): bundle contents are validated sources; per-bundle raise to `user_ratified` (`bundle trust`, `--trust ratified`, `/org` `t`, `org.trust`), tr..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 56, under `Sharing-layer slices (specs 022–027; estimates ±30%; see each row for state)`):

| Slice | Delivers | Spec / build plan | Depends on | Effort | State |
| :-- | :-- | :-- | :-- | --: | :-- |
| B6b | OD-10 (2026-10-27): bundle contents are validated sources; per-bundle raise to `user_ratified` (`bundle trust`, `--trust ratified`, `/org` `t`, `org.trust`), trusted sources by origin + publisher key (`bundle trust-source`, `--trust-source`, `/org` `s`); install-owned in `local/bundles.lock`; resets on hash/publisher change and rollback (one-off), kept across content changes (source) | 024 §3.7, §3.8, §10.7 | B6 | 12 h | built (PR open); see 024 §10.7. Not exercised live; the confirmation texts are the operator's to read |
