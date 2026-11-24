---
state: built
date: 2026-11-06
title: "handoff fidelity eval (paraphrase loss)"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 274, under `Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 048 | BM2 | handoff fidelity eval (paraphrase loss) | built 2026-11-06 (PR pending): `train_items.rs` (seeded generator of hash, path, number, code line and quoted-sentence items; byte-for-byte survival; shared with BM3) and `train_fidelity.rs` (`delphi train fidelity run` scores scripted verbatim, paraphrase and handle relays at the seat and at the operator, with a chart-ready table and `--json`; `gen` writes a case for a real model). New check kind `exact_items` (`path`, `min`) in `train.rs` / `train_run.rs`. AC-3 tested (paraphrase scores 0.0, verbatim 1.0; the handle relay is lossy at the seat and 1.0 at the operator, modelling CX-6). First live run 2026-11-07 (`fidelity/fid-7-10`, 10 items, `solo-haiku`, one seed, a Claude subscription seat; about 240K tokens counting cache reads, two runs): the first run scored 0 of 10 and was an instrument defect, not paraphrase loss (the prompt said "in the project", the model wrote `@project/artifact.md`, and both `read_file` and `list_dir` were refused because a project is addressed as `@project/<dir name>/`; fixed by giving the full path, `train_run::REPO_DIR`). The second run kept **10 of 10** items byte for byte. Haiku read the file itself and did not dispatch the investigator, so only the Orchestrator-to-operator hop was exercised. One seed, one small artifact: it shows the check works end to end and says nothing yet about how models differ. Not built: scoring the seat's own report from a real run (only the final answer is scored, as the deliverable); no handle relay in the engine (049 CX4); a larger artifact or a prompt that forces the seat hop, to stress a model |
