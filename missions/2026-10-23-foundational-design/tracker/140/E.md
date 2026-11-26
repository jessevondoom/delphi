---
state: built
date: 2026-11-26
title: "the deadline fix the spike supports; the health and doctor lines"
---

Slice E of spec 140, built 2026-11-26 on `build/140-efg` (from `knowledge-v2`).

## What was built

- **The warm-up** (`retrieval::warm`). It opens the service's reader on a thread of its own when the reader is missing or older than the index.
- **The daemon calls it** from its timer loop (`EngineService::retrieval_tick`): at the first tick, then every 5 seconds. Each tick opens the reader only when the index generation moved, so a sync by any process is followed by a warm-up within 5 seconds. A reopen with the model loaded costs about 2 ms (slice A).
- **No reranker in the service's reader.** `open_reader` attaches the embedding model only. The service never reranks.
- **`lexical-warming`.** A search that arrives while the warm-up holds the reader does not queue behind it. It answers by keywords at once and is counted `lexical-warming` (`retrieval::Mode::Warming`), not `lexical-deadline`.
- **The record of modes.** The daemon's tick appends the searches answered since the last tick, by mode, to `local/search-modes.jsonl` (counts only, no query). The file is cut back to seven days past 256 KiB.
- **The health line.** `delphi kb health` prints the service searches of the last 24 hours by mode and the share of `lexical-deadline`. Above 5% the line is marked stalled and `delphi doctor` shows it as a warning (check 21). With no record there is no line.
- `kb eval run` counts `lexical-warming` among the service modes; the vector leg's share stays the share of `hybrid`.

## Acceptance criteria and tests

- **AC-8** (at most 5% `lexical-deadline` among warm service queries on the operator's workspace, or the operator's decision recorded): **not met yet.** It needs a dated run on the operator's workspace (see "Not verified"). The parts that can be tested:
  - `retrieval::tests::the_warm_up_opens_the_reader_so_no_search_pays_for_it`: after the warm-up the first search is not cold; after a sync the warm-up opens the newer index.
  - `retrieval::tests::a_search_during_the_warm_up_answers_by_keywords_at_once_and_is_counted_warming`: during a slow warm-up a search answers by keywords well inside the deadline and is counted `lexical-warming`, with no missed deadline.
  - `service::tests::the_retrieval_tick_warms_the_reader_and_records_how_searches_were_answered`: the daemon's tick warms the reader, runs again only after 5 seconds, and records the modes that the health line reads.
  - `knowledge_health::tests::the_share_of_missed_deadlines_in_24_hours_is_a_health_line_and_a_warning_above_five_percent`: the line, the 24-hour window, the warning above 5%, and no write into a workspace without `local/`.

## Builder decisions

- **OD-4: the warm-up.** The spec's kill criterion (§2.5 item 2) asks for 95% of warm queries answering `hybrid`. By its letter slice A measured 94.5%. All 33 misses were the searches queued behind one cold model load in one of three processes; no search with a loaded model came near 800 ms (worst 521 ms with no deadline). The handoff of 2026-11-25 reads this as the warm-up case, and OD-4 says "decide on the numbers". The builder took the warm-up alone. No `[retrieval] deadline_ms` key, no query-embedding cache and no smaller candidate count were added. **The architect or operator should confirm this reading.**
- "After each sync" is a 5-second tick that compares the index generation, not a call in each sync path. It also covers a sync made by another process (`delphi kb index`).
- A search that queued on the reader just before the warm-up took it is also counted `lexical-warming` when it misses the deadline while the warm-up runs.
- The 24-hour share comes from a file the daemon writes (`local/search-modes.jsonl`), because `kb health` and the doctor run in their own processes and cannot read the daemon's counts. Searches made by a CLI process are not recorded.
- The doctor shows only this one health line as a warning. The other health lines stay facts.
- The record is cut back to seven days when it passes 256 KiB. It is a counter, not a ledger.
- `eval_waived` is not used here: E changes when the reader opens, not the ranking. Its before-and-after run is AC-8's dated entry below, still due.

## Not verified

- **AC-8 is not run.** It needs a release build on the operator's workspace: `delphi kb eval run --path service` after a daemon start, and the `kb health` line after a day of use. No dated entry exists yet.
- That the reranker's absence from the service's reader changes no result. The service sets `rerank_weight` to 0, so it should not; no test has a reranker model to show it.
- The live daemon under concurrent load, and a cold load after the file cache is cleared.
- A slow warm-up still holds the reader. A search that started its own open before the warm-up (a CLI process with no daemon) can still miss the deadline as before.
