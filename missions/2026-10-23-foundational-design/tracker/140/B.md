---
state: built
date: 2026-11-25
title: "the set: real cases, the header and hash, build and accept (step 2)"
---

Slice B of spec 140, built 2026-11-25 on `build/140-a`. `delphi kb eval build --real-only` writes `local/eval/golden-v<n>.jsonl` and its header from pairs that already exist (a research record of the workspace or of a registered project and its answer and claims, a finding, a project's research file, a search that found nothing), read from the index opened read-only; `delphi kb eval accept` shows 20 sampled cases, takes keep, drop or fix for each and only then writes the SHA-256; `kb_eval::load_accepted` refuses a draft and an accepted set whose bytes no longer match. Both commands are refused inside an agent seat (`crates/delphi/tests/kb_eval_cli.rs`). Synthetic cases (slice C) are not built: they need the single-turn bridge.

**Dated entry, 2026-11-25: the real-case set on a copy of the operator's home taken 2026-11-25** (never the live workspace). `build --real-only` on the copy gave golden-v1 with **90 real cases**: 76 project research files, 8 research records (workspace and projects), 6 findings; 817 relevant passages (1 to 31 per case); **0 `none` cases**, because the copy has no `local/kb-misses.jsonl`; 3 records were left out because their passages are not in the index (one is private, so its answer is not indexed). The set says it is short of 10 `none` cases. It has **not** been accepted by the operator: OD-2 makes acceptance the operator's alone, on the live workspace.

Choices the spec left open, made here: a passage is a chunk of the index (path and start line); a record's claims are matched by id under `research/claims/` and `research/projects/<id>/`; a project's topic files are the record's (not separate file cases); a case whose passages are all missing from the index is left out and counted (`skipped`); `fix` replaces the question's text; each relevant passage carries the index's tier as `tier` (the label of §2.1 item 6 until `specs/141` gives labels).
