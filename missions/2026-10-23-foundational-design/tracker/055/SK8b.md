---
state: built
date: 2026-10-29
title: "spec 057: validator pass (contradiction and fabricated-citation checks), one rewrite, claims-only fallback"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the day of the last commit that changed the row.

The row (line 374, under `The central library (spec 055, approved: ratified 2026-10-29)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| SK8b | spec 057: validator pass (contradiction and fabricated-citation checks), one rewrite, claims-only fallback | built (`library_page_validate.rs`, `claims add --origin`): `build_page_checked` runs the validator (V2b judge pattern, tool-less, role `validator`, budget reserved first) over the page and its material; the daemon classifies findings from its own records (a contradiction with a validated or cited record, or a fabricated citation, fails; a noted-record contradiction passes under Contested; unplaceable records fail closed); one rewrite with the findings, then the claims-only page with the reason in the banner; verdict in the sidecar; status stays `generated`; promote-to-claim makes a noted claim with `origin: page:<key>`. The CLI stays mechanical-only; engine and writer-span wiring is SK8c |
