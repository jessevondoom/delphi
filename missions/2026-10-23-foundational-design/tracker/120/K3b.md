---
state: merged
date: 2026-11-16
pr: 731
title: "the key file on every system; a keychain key moved once (§12)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 1189, under `The knowledge loop: specs 112 to 119 (2026-11-14, approved)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 120 | K3b | the key file on every system; a keychain key moved once (§12) | built and merged (#731): `key::default_store` is the file `<home>/app/key.json` (0600 in a 0700 folder) on every system; on macOS a key an earlier build kept in the login keychain is read once, written to the file, and the item removed (`key::Moving`); a keychain item this build may not read counts as no key, so the window reads without one and `delphi app` pairs again; nothing is written to the keychain any more. Tested with a fake older store; not run against the real keychain or `Delphi.app` |
