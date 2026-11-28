---
state: built
date: 2026-11-25
title: "the run: three paths, recall@10, recall@20, nDCG@10, compare; the baseline (step 2)"
---

Slice D of spec 140, built 2026-11-25 on `build/140-a`. `delphi kb eval run [--path service|hybrid|lexical|all] [--label] [--json]` scores the accepted set through the service (`retrieval::search`, deadline included), the hybrid search with no deadline, and keywords only, at twenty results (`SearchOpts::cap`, default 10, set to 20 only here); it reports recall@10, recall@20, nDCG@10 (graded, log2 discount), MRR, the `none` cases that returned something, stale cases, and for the service its modes, the vector leg's share, cold and warm p50/p95. It writes `local/eval/runs/<stamp>.json` (set version and hash, index generation, models, build, options, per-case first rank) and nothing else. `compare` refuses runs on different set versions and lists case ids that moved more than three ranks; `runs` lists them. Printed output carries numbers and case ids only.

Choices the spec left open, made here: recall@k is the share of a case's passages in the first k, out of min(passages, k), so a perfect ranking scores 1 for a long document; a hit matches a passage when the passage's start line falls inside the hit's lines in the same file, each passage once; every path searches the operator's view (untrusted passages and every registered project's research shared, reranker off, the workspace's ranking terms).

**Baseline, 2026-11-25, on a copy of the operator's home taken 2026-11-25** (never the live workspace or daemon). Set golden-v1, real cases only: 90 cases (76 project research files, 8 research records, 6 findings), 0 `none`, sha256 `5daea9e920b61f89f530629d3ca4cfa9fffbc4ee514133e49d5de74ea04d2700`, index generation 85, 5,738 chunks, embedder `bge-base-en-v1.5-q8`, release build of slice B's commit 6f3746d9 with slice D uncommitted. **The set was accepted on the copy by the builder (keep for all 20 sampled cases) only so that a run is possible; that is not the operator's acceptance (OD-2).** With every case kept the accepted file is the draft unchanged, so an operator who keeps every case gets the same hash; any drop or fix makes a new hash and the baseline must be run again on it.

| Run | Path | recall@10 | recall@20 | nDCG@10 | MRR | p50 / p95 ms | service modes |
| :-- | :-- | --: | --: | --: | --: | :-- | :-- |
| 20261126T005148Z (and T005201Z, identical) | service | 0.9076 | 0.9361 | 0.9092 | 0.9833 | 37 / 53 | hybrid 100%, cold 442 ms |
| | hybrid | 0.9076 | 0.9361 | 0.9092 | 0.9833 | 35 / 64 | |
| | lexical | 0.5964 | 0.6222 | 0.6142 | 0.9169 | 1 / 4 | |
| 20261126T005136Z (first process of a new binary) | service | 0.8497 | 0.8781 | 0.8518 | 0.9588 | 39 / 818 | hybrid 80%, lexical-deadline 20% (the first 18 queries) |

Run-to-run spread on an unchanged build (§7): zero for hybrid and lexical over three runs; the service matched hybrid exactly in runs 2 and 3 and fell in run 1, when the new binary's first model load held the reader past the deadline for its first 18 queries (as in slice A's spike). The cold first query of run 1 was not recorded in that run file (a defect fixed before this commit: the first service query is now always reported as the cold one).

**Operator ruling, 2026-11-27 (OD-2):** "yes the golden set is accepted". The operator accepted golden-v1 as built: 90 real cases, kept whole. The set and its baseline lived on a temporary copy of the home taken 2026-11-25. Neither the live home nor the test copy `~/.delphi-kv2` holds a golden-set file, so for spec 144's trial (§2.8 item 1.1) the set is built again on `~/.delphi-kv2` with `kb eval build --real-only`. It is accepted there by keeping every case, under this ruling, and its baseline is run before any 144 run. If that copy builds a different set (its index has moved since 2026-11-25), the new hash is recorded here and the baseline is taken on it.
