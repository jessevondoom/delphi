---
state: built
date: 2026-11-25
title: "the spike: stage timings and mode counts in the service path (step 2 of the build order)"
---

Slice A of spec 140, built 2026-11-25 on `build/140-a` (from `knowledge-v2` at 48ed5123). A service search now reports where its time went (`retrieval::Timing`: lock wait, index open, model load, query embedding, vector search, keyword rankings, fusion, the fallback) and the process counts its modes (`retrieval::mode_counts`). `Kb::search_staged` times the stages of one search. The spike itself is an ignored test, `retrieval::tests::spike_on_a_workspace_copy`, run by hand.

**Dated entry (AC-1), 2026-11-25.** The spike ran on a copy of the operator's home taken on 2026-11-25 (library, operator layer and the default workspace with its index `local/state.db`, generation 85, 5,738 chunks; the embedding and reranking packages copied out of the installed binary). It was never run against `~/.delphi` or the live daemon. 200 code-made queries, three processes, each with one cold search, 199 warm, 20 after the reader was forgotten, and 200 direct hybrid searches with no deadline. Seen: warm searches p50 47 ms, p95 811 ms; 33 of 597 warm searches (5.5%) answered `lexical-deadline`, all of them the 33 searches after the one cold open that missed (in one of the three processes), none in the other two; direct hybrid p95 82 ms and never above 800 ms; a reopen after a generation change costs 2 ms because the models stay loaded per process. The full write-up is in the spec's section 7.
