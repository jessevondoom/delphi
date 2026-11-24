---
state: built
date: 2026-11-01
title: "weak-source labels (040-A2, §27)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell. The state cell did not map clearly to a state; `built` is the closest, and the cell is kept below as it was.

The row (line 306, under `The validator role and fact ratification (spec 040, approved 2026-10-28)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| V2e | weak-source labels (040-A2, §27) | slice 1 built 2026-11-01: `FactFindings::label`/`weak_only`/`weak_agree`, `[validator] weak_sources`. Not built: routing and storage of weak-only claims, the promotion pass, the digest. Slice 2 built 2026-10-31 (040-A3, §28): Wikipedia citation guard and the journal-publisher DOI-in-text check; resolving the DOI built 2026-10-31 (040-A5, §29: `DoiResolver`, `HandleResolver`, fails closed). Slice 3 built 2026-10-31 (058-A1, §13): `research::route`/`Scope`/`Dest`, pending and project destinations, `source_class`, `research.start {project}`; not built: console project name, digest. Slice 4 built 2026-10-31 (058-A2, §14): `research_pending`, `pending.*` RPCs, `/validate` and `/retro research`; built 2026-10-31 (058-A6, §18): the validator pass, `/validate pass`, `pending.pass`, run at session open. Built (058-A7, §19): `/research`; Built (058-A8, §20): `request_research` tool (Amber, always asks). Slice 6 built 2026-10-31 (058-A3, §15): `/projects` picker and console restart in the chosen directory. Built 2026-10-31 (058-A4, §16): `projects.list`, and `research.start {cwd}` so the console's project receives its research. Slice 5 built 2026-10-31 (058-A5, §17): `research_digest`, the `research.digest` chat message, weekly. Slice 9 built 2026-10-31 (058-A9, §21): new sessions under `local/sessions/`, `delphi_core::sessions`; the word rename was considered and declined. Built 2026-10-31 (058-A10, §22): an agent is told how its research went; (040-A6, §30): `delphi check fact` resolves DOIs, `--offline` opts out. |
