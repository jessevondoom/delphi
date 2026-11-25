---
id: spec-140-a-golden-set-for-retrieval-recall-ndcg-a-baseline-and-a-gate-on-every-retrieval-change
title: "140: A golden set for retrieval: recall, nDCG, a baseline, and a gate on every retrieval change"
author: architect
timestamp: 2026-11-25T20:30:00Z
status: draft
reviewed_by: []   # no Council or Panel has read this text. The plan it comes from was put to a daemon Council on 2026-11-25; that is a review of the plan, not of this spec, and is not cited as one.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors read on 2026-11-25 at origin/main 52bdd3b8 (0.6.7). Nothing was run: the causes of the missed vector deadline in section 2.5 are read from the code, not measured.
source_mission: 2026-11-25-knowledge-v2
tags: [spec, retrieval, evaluation, golden-set, recall, ndcg, kb, vectors, gate]
---

# Spec 140: A golden set for retrieval

> **Status: draft**, revised 2026-11-25 with the operator's decisions of that day. One of five specs from the knowledge audit of 2026-11-25 (139 to 143). It lands before any change to retrieval ranking.
> **The plan is approved; this text is not yet.** The operator approved the plan on 2026-11-25 ("go"). The status stays `draft` until a witnessed Panel with a red-team seat has read this text; approval is marked after that.
> **Build order (confirmed by the operator, 2026-11-25):** the single-turn harness bridge (`specs/143`), with the seed slices of `specs/139` (A, B, D, E) built alongside it; then the source text cache (`specs/142`); then the remaining slices of 139; then the golden set (`specs/140`); then label enforcement (`specs/141`); then the one knowledge record and the later steps, which have no spec yet. **This spec is fourth.** The spike of slice A and the real-case set (slice B) need nothing from the others and may be built at any point; the set is rebuilt as a new version once 142 has added source text.
> **Extends:** [`specs/064`](064-knowledge-retrieval-for-agents-and-mcp.md) §3 (the one retrieval service and its deadline), [`specs/027`](027-bundle-authoring-distribution-and-evaluation.md) §7.1 (the retrieval gate of a bundle, whose harness this reuses), [`specs/109`](109-one-retrieval-path-for-host-search-a-fresh-index-and-passages-at-adoption.md) §9 (ten questions, before and after: the practice this makes a rule), [`specs/135`](135-the-forge-first-cut-slice-claims-a-finisher-and-a-cross-family-review-that-runs-the-tests.md) §5 (the finisher, which gains one check).

---

## 1. Scope and objectives

| # | Observation (origin/main 52bdd3b8, and the operator's workspace on 2026-11-25) | Where |
| :-- | :-- | :-- |
| O1 | A harness exists. `delphi kb eval` indexes a **documents folder** (the repository's own files by default) into a fresh index and prints recall at 1, 3, 5 and 10 and MRR, for each of lexical, vector, hybrid and reranked search, over 48 questions and a 44-question holdout. | `kb.rs:2714-2800`, `:3341`; `dev/kb-eval.json`, `dev/kb-eval-holdout.json` |
| O2 | It does not measure the operator's corpus, has no graded relevance (so no nDCG), stops at ten results (`kb.rs:2151` caps every search at 10), and calls `Kb::search` directly, so it never passes through the service that agents and hosts use and never meets its deadline. | `kb.rs:2741-2760`, `:2151`; `retrieval.rs:68` |
| O3 | The service gives the hybrid search 800 ms and then answers with keywords alone, labelled `lexical-deadline`. One sampled `search_kb` call on the operator's workspace did exactly that. Nobody knows how often. | `retrieval.rs:14`, `:101-110` |
| O4 | The operator's index holds 5,423 chunks, among them 82 project research files. The corpus is not empty: a set can be built from it today. | the audit of 2026-11-25 |
| O5 | Real questions are already on disk: every research record's `question` (`research_record.rs:266`), every finding's question (`finding_record.rs:109`), and every search that found nothing (`local/kb-misses.jsonl`, `kb.rs:2532`). | as named |
| O6 | Steps 6 and 7 of the plan (chunk headers, reranking, pre-filtering, usage boosts) each change ranking. Without a fixed set, "better" cannot be told from "different". | the audit of 2026-11-25 |

Objective: a fixed, private set of questions with known answers in the operator's own corpus; one command that scores retrieval on it as agents experience it; a recorded baseline; and a rule, checked by code, that no retrieval change merges without a before-and-after run. The vector deadline is measured first and fixed only as far as the numbers say.

**On the Council's finding** ("the golden set is blocked by an empty corpus"): the corpus has 5,423 chunks (O4). What is empty is the page and source-text layer, which this set does not need. The set is rebuilt, as a new version, when `specs/142` adds source text.

### 1.1 Non-goals

- **No change to ranking, chunking, embedding or reranking.** This spec measures. The one behaviour change it may make is the warm-up of §2.5, and only on the evidence of slice A.
- **No answer-quality or citation metrics** (whether a page is right). That is a later evaluation, for step 7.
- **No gate on a number.** The rule is that the numbers exist and are shown; nothing is blocked for being lower. The operator reads them.
- **No query text leaves the workspace.** The set and the runs are private files; what is committed or printed in a pull request is numbers.

### 1.2 Simpler alternative considered

- **Alternative:** add nDCG and recall at 20 to `kb::evaluate` and keep running it on `dev/kb-eval.json`.
- **Why rejected:** that set asks about Delphi's own specs and charter. The operator's research corpus is other text (web captures, project notes, findings), and the failure seen (O3) is in the service path the old harness skips. A change could improve the repository set and harm the corpus that matters, and nobody would see it.

### 1.3 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | a hand-labelled set of about 50 questions, each with its relevant passages marked by the operator | run it; the cost is the operator's hours, measured by whether the set gets finished |
| B | constraint removed: "a person labels every case" | cases built from records that already pair a question with its answer (a research question and its `answer.md`; a finding and its file), plus per-chunk queries a model writes; the operator spot-checks a sample | the share of sampled cases the operator rejects |
| C | inversion: do not build a set; log live searches and score them by whether the result was then read | usage as the label | needs months of use signals; the read log is too thin today (`use_signals.rs`) |

Chosen: **B**, with the operator's spot-check from A as the acceptance step. A alone does not get built. C is step 8's learning loop and cannot gate step 6. Flagged by the guard (a new layer of abstraction with no test of its own): none.

## 2. Design

### 2.1 The set

1. A golden set is a file `local/eval/golden-v<n>.jsonl` in the workspace, one case a line, and a header file `golden-v<n>.json` with its SHA-256, when it was built, the index generation it was built against, and counts by kind. `local/` is the daemon's (no agent tool reads or writes it).
2. A case: `{"id", "q", "kind": "real" | "synthetic", "from", "relevant": [{"path", "start_line", "grade"}], "none": false}`. `grade` is 3 (the answer itself), 2 (states part of it) or 1 (on the subject). `none: true` marks a question the corpus should not answer.
3. **Real cases**, built by code from pairs that already exist:

| From | The question | Relevant, and grade |
| :-- | :-- | :-- |
| a research record (`research/topics/<id>/record.json`, workspace and projects) | its `question` | the passages of its `answer.md` (3); its claims' documents (2) |
| a finding (116 §5) | its question | the finding's file (3) |
| an ingested project research file (113 §3) | the file's title | the file's passages (3) |
| `local/kb-misses.jsonl` | the query | none, where the corpus still has no answer (`none: true`); a miss that later research answered becomes a real case of the first row |

4. **Synthetic cases:** for a sample of chunks, stratified by zone and by project, a model writes one question that the chunk answers and that does not reuse the chunk's heading. Code drops a question that shares more than half its content words with the chunk's heading or first line (it would only test keyword match), and one whose top lexical hit is not the chunk *and* whose words appear in no chunk (it is about nothing). The chunk is the one relevant passage, grade 3. The model call is made by the operator's command at a terminal, on the `fast` tier, and its cost is printed before it runs.
5. Size, from the audit's evidence: at least 50 real cases and 150 synthetic, and at least 10 `none` cases. With fewer than 50 real cases on disk the set is built with what there is and says so.
6. **Private and internal passages are in the set.** The set never leaves the workspace, and the operator ruled on 2026-11-25 that a configured model route is not a leak. A case records the label of its passages so that results can be split by label once `specs/141` lands.
7. **Frozen.** A built set is a draft until the operator accepts it: `delphi kb eval accept` shows 20 sampled cases (ten of each kind), takes `keep`, `drop` or `fix` for each, and then writes the hash. An accepted set is never edited. A changed corpus (a passage gone) marks its cases `stale` in the run, never silently drops them. A new set is a new version; a run names the version it used, and two runs are compared only on the same version.

### 2.2 The run

1. `delphi kb eval run` scores the accepted set in three ways and reports each:

| Path | What it calls | Why |
| :-- | :-- | :-- |
| `service` | `retrieval::search` (`retrieval.rs:68`), as `search_kb` and an agent's `kb_search` do | what agents get, deadline included |
| `hybrid` | `Kb::search` with both legs, no deadline | what ranking can do |
| `lexical` | `Kb::search`, keywords only | the floor |

2. Metrics for each path: **recall@10**, **recall@20**, **nDCG@10** (graded, with the standard log2 discount), MRR, and for `none` cases the share that still returned a result. A relevant passage is matched by path and line overlap, not by heading text.
3. For the `service` path also: the share of queries answered `hybrid`, `lexical` and `lexical-deadline` (`retrieval.rs:16-31`), and p50 and p95 milliseconds, cold (the first query after the reader is opened) and warm, reported apart.
4. Twenty results need a search that returns twenty. `SearchOpts` gains `cap` (default 10, today's value at `kb.rs:2151`); only the eval sets it to 20. No caller's results change.
5. A run is written to `local/eval/runs/<stamp>.json`: the set's version and hash, the index generation, the embedder and reranker ids, the binary's version and commit, the search options, every metric, and per case the rank of its first relevant passage. `--json` prints the same. The summary printed to the terminal and pasted into a pull request holds numbers and case ids only, never a question or a passage.
6. `delphi kb eval compare <before> <after>` prints the two runs side by side with the differences, lists the case ids that moved by more than three ranks, and refuses to compare runs on different set versions.
7. A run opens the index read-only and writes nothing but its own file.

### 2.3 The command

```
delphi kb eval build [--real-only] [--synthetic <n>] [--workspace <name>] [--home <dir>]
delphi kb eval accept [--workspace <name>] [--home <dir>]
delphi kb eval run [--path service|hybrid|lexical|all] [--label <text>] [--json] [--workspace <name>] [--home <dir>]
delphi kb eval compare <run> <run> [--json]
delphi kb eval runs [--json]
delphi kb eval --docs <dir> [--file <cases.json>] ...   # unchanged: the repository set (kb.rs:3341)
```

`build` and `accept` are the operator's: refused inside an agent seat, as `kb import` is (`cmd_kb`, `main.rs:1564`). `run`, `compare` and `runs` read only and may be run by an agent or a host.

### 2.4 The baseline and the rule

1. **Baseline.** The first accepted set is run on the operator's workspace before any retrieval change. The numbers (no query text) are recorded in the tracker file of slice D with the run's stamp.
2. **The rule: no retrieval change without a before-and-after run.** A pull request is a retrieval change when it touches a file on the list `RETRIEVAL_PATHS`, kept in code beside the finisher: `crates/delphi-engine/src/kb.rs`, `retrieval.rs`, `kb_docs.rs`, `embed.rs`, `embed/**`, `research_index.rs`, `use_signals.rs`, and the `[retrieval]` keys of `config.rs`.
3. The check is in the finisher (135 §5, `finisher.rs:112`): `delphi forge finish` on a branch that touches `RETRIEVAL_PATHS` requires the slice's tracker file to carry `eval_before` and `eval_after` (two run stamps on the same set version) and puts the comparison in the pull request body. Without them it fails with the command to run. A change that cannot move ranking (a comment, a rename) says so with `eval_waived: <reason>` in the tracker file, which the body shows.
4. The repository set still runs as today and gains recall@20 and nDCG@10, so a contributor without the operator's workspace has numbers too. A committed file, `dev/kb-eval-baseline.json`, holds its last numbers; a test fails when recall@10 or nDCG@10 on the repository set falls by more than 0.02 without that file changing in the same commit.
5. **Advisory, not a gate (operator, 2026-11-25, as relayed).** The finisher's check stays advice for now: it is not added to `merge-gates.toml` or to CI. Making it a merge gate later is a change to a gate-changing path and is the operator's.

### 2.5 The vector deadline: measure, then fix what the numbers show

What the code suggests, **not measured**:

- The reader is opened inside the 800 ms window on the first search and again after every index generation change, and opening it loads the embedding model **and the reranker** (`retrieval.rs:56-60`, `:84-93`; `kb.rs:2650-2657`), though the service never reranks (`retrieval.rs:70`).
- The worker thread holds the reader's lock until it finishes, so a search that missed the deadline keeps the next one waiting (`retrieval.rs:79-98`).
- The fallback opens the index again for each missed query (`retrieval.rs:105-109`).

1. **Slice A is a spike.** It adds timing to the service path (open, model load, query embedding, vector search, keyword search, fusion) and the mode counts, and runs 200 synthetic queries on the operator's workspace, cold and warm. Its result is written into §7 before slice E starts.
2. **Kill criterion, written now:** if at least 95% of warm queries answer `hybrid` inside 800 ms, the fault is the cold open, and slice E is the warm-up alone: the daemon opens the reader at start and after each sync, outside any search; the reranker is not loaded by the service's reader; a search that arrives while the reader is opening answers lexical and is counted `lexical-warming`, not `lexical-deadline`. If fewer than 95% do, slice E does **not** proceed as written: the timings go to the operator with the choices (a larger deadline as a `[retrieval] deadline_ms` key, a cache of query embeddings, a smaller candidate count than `VEC_CANDIDATES`, `kb.rs:46`), each of which is then a retrieval change under §2.4.
3. Either way the share of `lexical-deadline` answers in the last 24 hours becomes a line of `delphi kb health` and a warning in `delphi doctor` above 5%.

### 2.6 The history: retrieval quality kept in memory

The operator's requirement of 2026-11-25, as relayed: keep historic retrieval performance in memory, so that quality can be followed over time.

1. **The ledger.** Every eval run appends one line to `local/eval/history.jsonl` in the workspace. Only the daemon and the operator's own command write it (`local/` is closed to agents). A line holds numbers and identifiers only, never a question or a passage:

| Field | What it is |
| :-- | :-- |
| `v`, `at`, `run`, `trigger` | the line's format version; when; the run's stamp; `manual`, `scheduled`, `before` or `after` |
| `set` | the golden set's version, SHA-256 and case counts by kind |
| `corpus` | documents, chunks, vectors and the index generation |
| `config_sha256` | a hash of the search options and the `[retrieval]` keys in effect |
| `models` | the embedder's and the reranker's ids and file hashes |
| `build` | the binary's version and commit |
| `path` | `service`, `hybrid` or `lexical` |
| `recall10`, `recall20`, `ndcg10`, `mrr` | the metrics of §2.2 |
| `p50_ms`, `p95_ms` | latency, warm |
| `vector_leg` | the share of `service` queries answered with the vector leg (not `lexical-deadline`, not `lexical-warming`) |

2. **Versioned.** A **series** is one set version on one path. A new set version starts a new series; the old ones stay and are never compared across. A reader accepts every older `v`.
3. **Durable: what it survives.**
   - *An index rebuild.* The ledger is a file of its own, not a table of `local/state.db`; `kb reindex` does not touch it.
   - *A new golden set.* A new series (item 2).
   - *Pruned run files.* `local/eval/runs/` may be pruned after 90 days; the ledger is never pruned.
   - *A torn write.* The file is append-only and never rewritten; a last line that does not parse is skipped and reported.
   - *A change of stores.* The one knowledge record of step 6, and any later migration of the workspace, must carry this file unchanged. That is a requirement this spec places on those specs.
4. **A trend without a code change.** Every `[retrieval] eval_every_days` (default 7, `0` is off) the daemon runs the `service` path on the accepted set while idle. It calls no model and spends no tokens.
5. **Visible to agents.** After each run the daemon regenerates, from the ledger, the document `local/knowledge/retrieval-quality.md` (zone `knowledge`, `kb.rs:491`): one sentence per series (`set v1, service path: nDCG@10 0.62, steady over six runs`) and a table of its last 20 runs. A search of the knowledge base about retrieval quality finds it. It is a view: it is rebuilt from the ledger and is never read back as the record.
6. **Visible to the operator.** `delphi kb eval history [--set <n>] [--path <p>] [--json]` prints each series as a table with the change from run to run and a one-line trend. `delphi kb health` gains one line: the newest run against the median of the five before it. The app's knowledge view gains a "Retrieval quality" panel that reads the same lines through a read-only daemon method.
7. **A noticeable change leaves a note.** After a run, code compares it with the median of the previous five runs of its series. A change of 0.03 or more in recall@10 or nDCG@10, of ten points or more in `vector_leg`, or a p95 of one and a half times the median, is **noticeable**. The daemon then writes one provisional note (116 §2, `provisional_notes.rs`), kind `fact`, in words made by code from the numbers: what moved, between which runs, and what differed between them (the build, the models, the config hash, the corpus size). It is a lesson candidate: the operator reviews it as any provisional note (116 §3) and nothing promotes it. One note per series and direction, until a run is back inside the thresholds.

## 3. Authority

- **A model is called** to write synthetic questions, by the operator's own command at a terminal, with the cost shown first. Chunk text, private text included, goes to the configured `fast` model: the operator ruled on 2026-11-25 that every configured model route on this machine is licensed and governed, and is not a leak to guard against. On another install the same command names the model and asks before it sends.
- **Nothing leaves the workspace.** The set, the runs and every question stay under `local/`. The numbers a pull request carries hold no text.
- **The gate is advice until the operator makes it one.** The finisher refuses to print a body; it cannot stop a merge.
- **The eval reads the operator's view** (untrusted and private passages included), so `run` output is shown to the operator and stored locally; it is never an agent's retrieval path and grants none.

## 4. Slices

| Slice | Content | Criteria | Files | Hours | Depends on |
| :-- | :-- | :-- | :-- | --: | :-- |
| S0 | this spec | (the spec) | this file | 1-2 | none |
| A | the spike of §2.5: stage timings and mode counts in the service path; 200 queries cold and warm on the operator's workspace; the result written into §7 | AC-1 | `retrieval.rs`, `kb.rs` | 3-4 | S0 |
| B | the set: the case shape, real cases from records, findings, ingested files and misses; the header and hash; `build --real-only`, `accept` | AC-2, AC-3 | `kb_eval.rs` (new), `kb.rs`, `main.rs` | 5-6 | S0 |
| C | synthetic cases: sampling, the model call, the two filters, the cost line | AC-4 | `kb_eval.rs`, `page_run.rs` (the model call) | 3-4 | B |
| D | the run: three paths, recall@10, recall@20, nDCG@10, `cap`, the run file, `compare`, `runs`; the baseline run recorded | AC-5, AC-6, AC-7 | `kb_eval.rs`, `kb.rs`, `retrieval.rs` | 5-6 | A, B |
| E | the deadline fix the spike supports (the warm-up, or nothing but the operator's decision); the health and doctor lines | AC-8 | `retrieval.rs`, `service.rs`, `knowledge_health.rs` | 3-4 | A, D |
| F | the rule: `RETRIEVAL_PATHS`, the finisher's check, the tracker fields, the repository baseline file and its test | AC-9, AC-10 | `finisher.rs`, `tracker.rs`, `dev/kb-eval-baseline.json`, `kb.rs` | 3-4 | D |
| G | the history: the ledger line, the scheduled run, `kb eval history`, the health line, the knowledge view, the note on a noticeable change | AC-11, AC-12, AC-13 | `kb_eval.rs`, `service.rs`, `knowledge_health.rs`, `provisional_notes.rs`, `main.rs` | 5-6 | D |
| H | the app's "Retrieval quality" panel and its read-only method | AC-14 | `service.rs`, the app; `crates/delphi/tests/app_grants.toml` if the method needs a grant (a gate-changing path, so that pull request is the operator's at a terminal) | 2-3 | G |
| | **Total** | | | **30-39** | |

E is itself a retrieval change: it is the first slice to carry a before-and-after run.

## 5. Acceptance criteria

1. **AC-1:** a service search reports its stage timings and mode; the spike's cold and warm results on the operator's workspace are written in §7 with the date.
2. **AC-2:** on a fixture workspace with three research records, two findings and four misses, `build --real-only` writes three, two and four cases with the grades of §2.1; building twice gives the same file.
3. **AC-3:** `accept` writes the hash only after the sampled cases are answered; an accepted set that is edited fails its hash and `run` refuses it; `build` and `accept` are refused inside an agent seat.
4. **AC-4:** a synthetic question that repeats its chunk's heading is dropped; the cost line is printed before any model call; with no model the command builds the real cases and says the synthetic ones were skipped.
5. **AC-5:** on a fixture with a known ranking, recall@10, recall@20 and nDCG@10 equal hand-computed values to four decimal places, for each path.
6. **AC-6:** `run --path service` with a reader made to answer late reports `lexical-deadline` for that query and scores what was returned; no existing search test changes its results (`cap` defaults to 10).
7. **AC-7:** `compare` of two runs on different set versions is refused; on the same version it prints the differences and the moved case ids, and no question text.
8. **AC-8:** after slice E, on the operator's workspace, at most 5% of warm service queries answer `lexical-deadline`, or the operator's decision of §2.5 item 2 is recorded instead.
9. **AC-9:** `delphi forge finish` on a branch that changes `retrieval.rs` fails without `eval_before` and `eval_after`, passes with them and prints the comparison, and passes with `eval_waived`; on a branch that touches no listed path it does not ask.
10. **AC-10:** lowering a ranking weight so that the repository set's nDCG@10 falls by 0.05 fails the baseline test until `dev/kb-eval-baseline.json` is updated in the same commit.
11. **AC-11:** every run appends one ledger line with the fields of §2.6 item 1; after `kb reindex`, after a new set version and after the run files are pruned, every earlier line is still there, byte for byte; no code path truncates or rewrites the file.
12. **AC-12:** after a run, `local/knowledge/retrieval-quality.md` holds the series' sentence and table and no question text, and an inside agent's search for `retrieval quality` returns it.
13. **AC-13:** a run whose nDCG@10 is 0.05 below the median of the five before it leaves exactly one provisional note that names both runs and what differed; a fall of 0.01 leaves none; a second low run leaves no second note.
14. **AC-14:** `delphi kb eval history` and the app's panel show the same series and numbers as the ledger; with `eval_every_days = 0` no scheduled run is made.

## 6. Verification plan

| Criterion | How it is verified |
| :-- | :-- |
| AC-1 | a unit test on the timing record; the spike's own write-up (a manual step, dated) |
| AC-2, AC-3 | `kb_eval.rs` tests on a fixture workspace; a CLI test for the seat refusal |
| AC-4 | tests with a scripted model and with none |
| AC-5 | a metric test against values computed by hand in the test's comment |
| AC-6 | a service test with a slow reader; the existing `kb.rs` search tests, unchanged |
| AC-7 | a CLI test that scans the output for any case's question |
| AC-8 | the run recorded in the tracker (manual, on the operator's workspace) |
| AC-9, AC-10 | finisher tests on a temporary repository; the baseline test in `cargo test` |
| AC-11 | a ledger test: lines before and after a reindex, a new set and a prune are compared byte for byte |
| AC-12 | a daemon test with a scripted agent's search; a scan of the view for any case's question |
| AC-13 | tests on a ledger fixture with a fixed clock |
| AC-14 | a CLI golden test; the app's test for the panel |

**Fitness functions** (owner `builder` unless said):

| Invariant | Check that would fail | Kind | State |
| :-- | :-- | :-- | :-- |
| a retrieval change carries a before-and-after run | `finisher::tests::a_retrieval_change_needs_eval_before_and_after` | Rust test | to be written |
| `RETRIEVAL_PATHS` names every file that ranks | `kb_eval::tests::every_file_that_calls_search_found_or_vector_ranked_is_listed` (reads the crate's sources, as `egress.rs` does for senders) | Rust test | to be written |
| an accepted set is never edited | the hash check in `run` | daemon check | to be written |
| a run's printed summary holds no query text | the scan of AC-7 | Rust test, owner `red-team` | to be written |
| the repository set does not regress unseen | the baseline test of AC-10 | Rust test | to be written |
| the metric code is right | AC-5 | Rust test | to be written |
| the history is never rewritten or pruned | `kb_eval::tests::the_ledger_is_append_only` (reads the crate's production text for any open of `history.jsonl` that is not an append) | Rust test | to be written |
| the history and its view hold no query text | the scan of AC-12, over the ledger and the view | Rust test, owner `red-team` | to be written |

### 6.1 Premortem (three months after release): how this most likely fails

| # | Cause, as it happened | Likely | Damage | Early sign | Response in this spec |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 1 | The set flattered keyword search: synthetic questions reused their chunk's words, lexical scored 0.95, and every later change looked like a loss. | high | high (wrong decisions on steps 6 and 7) | lexical recall@10 above hybrid on synthetic cases only | the two filters of §2.1 item 4; real and synthetic reported apart; the operator's spot-check |
| 2 | The set was never accepted: the operator did not have an hour for it, and the rule waited on it. | medium | high | no `golden-v1.json` hash a week after slice B | the sample is 20 cases, not 200; `--real-only` gives a usable set with no model |
| 3 | The rule was waived by habit: the check is advisory, and every pull request wrote `eval_waived`. | medium | medium | waivers in the tracker outnumber runs | the waiver's reason is printed in the pull request body for the reviewer; the scheduled run and the history (§2.6) show a fall whether or not a pull request measured it |
| 7 | The history was noise: every weekly run moved a little as the corpus grew, a note was written each week, and the operator stopped reading them. | medium | medium | more than one note a month on an unchanged build | the thresholds of §2.6 item 7; one note per series and direction; the note names the corpus size so growth can be told from a code change; OD-5 |
| 4 | Numbers moved because the corpus grew, not because the code changed, and a good change was blamed. | medium | medium | before and after runs carry different index generations | a run records the generation; `compare` prints a warning when they differ |
| 5 | The spike blamed the cold open, the warm-up shipped, and the deadline was still missed under load because the lock, not the model, was the cause. | medium | medium | `lexical-deadline` share in `kb health` does not fall | AC-8 is measured after E on the live workspace, not assumed; the kill criterion sends the other case to the operator |
| 6 | A private question appeared in a pull request body. | low | medium | a quoted question in a body | summaries carry ids and numbers only (§2.2 item 5); the scan test |

Not acted on: 4 beyond the warning (holding the corpus still would need a snapshot of the index, which is more than this spec should own).

## 7. What is not verified

- **Why the vector leg misses its deadline.** §2.5 lists what the code suggests. Slice A measures it. *(The result of slice A is written here.)*
- That 50 real cases exist on the operator's workspace. The audit counted 82 project research files and 9 topics; findings and misses were not counted.
- That a model writes questions that are neither trivial nor unanswerable at a useful rate. The filters are guesses until slice C's first build is sampled.
- That nDCG on about 200 cases is stable enough to see a change of the size steps 6 and 7 promise. If the run-to-run spread on an unchanged build is wider than the change, the set is too small; slice D reports the spread from three runs.
- The evidence for sizes and for reranking gains was read by the scout from abstracts only.
- That a default search by an agent returns the view in zone `knowledge`. If that zone's tier keeps it out of a default search, slice G files the view where a default search reaches it.
- That the daemon can keep a provisional note that no agent's event raised (`provisional_notes::keep` takes an event, `provisional_notes.rs:398`). If not, slice G adds a daemon-authored entry point and its test.
- The thresholds of §2.6 item 7. They are guesses until three runs on an unchanged build show the spread.

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | May synthetic questions be written by the `fast` model from private and internal chunks? | yes, under the ruling of 2026-11-25; the cost is shown first | proposed: yes | C |
| OD-2 | Who accepts the set: the operator alone, or may an agent pre-screen the 20 samples? | the operator alone; it takes about twenty minutes | proposed: yes | B |
| OD-3 | Should the finisher's check become a merge gate? | **Decided (operator, 2026-11-25): no, advisory for now.** | decided | none |
| OD-4 | If the spike shows warm queries miss the deadline, which remedy? | decide on the numbers; no default | proposed: yes | E |
| OD-5 | The thresholds for a noticeable change (0.03 on recall@10 or nDCG@10, ten points on the vector leg, 1.5 times on p95)? | **Decided (operator, 2026-11-25, "use your defaults"): as written**, revised once the run-to-run spread is known | decided | none |
| OD-6 | A scheduled run every seven days while idle? | **Decided (operator, 2026-11-25, "use your defaults"): yes.** It also has a command the operator can run at any time (`delphi kb eval run`; 143 §2.8) | decided | none |
