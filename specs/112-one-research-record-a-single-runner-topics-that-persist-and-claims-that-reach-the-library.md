---
id: spec-112-one-research-record-a-single-runner-topics-that-persist-and-claims-that-reach-the-library
title: "112: One research record: a single runner, topics that persist, and claims that reach the library"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: approved   # by the operator, 2026-11-14, in a host session: "all other specs are approved", after the drafts and their slice tables were shown.
approved_by: operator (2026-11-14)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36) and a read-only look at one live workspace; nothing was run. Observation E-5 was read in code and not reproduced.
source_mission: 2026-11-14-knowledge-loop
tags: [spec, research, scout, library, claims, kb]
---

# Spec 112: One research record

> **Status: approved by the operator, 2026-11-14** (see the frontmatter). First of eight specs (112 to 119) from the knowledge-loop audit. Build order across the eight is in [`specs/119`](119-research-readiness-setup-private-text-for-retrieval-health-checks-and-a-live-proof.md) §7.
> **Extends:** [`specs/066`](066-mcp-research-endpoint-and-role-identity.md) (the host research endpoint), [`specs/055`](055-the-central-library-sources-claims-and-pages.md) (sources, claims, pages), [`specs/101`](101-library-practice-for-research-a-search-log-retraction-lookup-archived-copies-and-citation-chaining.md) (the search log), [`specs/028`](028-knowledge-window.md) (the console's research job).

---

## 1. Scope and objectives

| # | Observation (code on `main`, 2026-11-14) |
| :-- | :-- |
| E-1 | There are two research runners. The console and Delphi's own agents use `research.rs`, which writes one Markdown page per web page under `local/knowledge/web/`. A host's `request_research` uses `research_run.rs`, which writes library sources and a claims file. They share the trusted list and the search providers and nothing else. |
| E-2 | The host runner's claims go to `research/host-claims.jsonl`. The only reader of that file is the duplicate check. The claims are not in the search index and not in `library/claims/`, so pages ([`specs/097`](097-encyclopedia-search-pages.md)) and the confidence ladder ([`specs/100`](100-confidence-ladder-substantiated-vetted-and-an-audit-that-rolls-back.md)) never see them. |
| E-3 | A host run's result (answer, coverage report) is held in memory for one hour and then gone. The console run keeps no search log and no record of its queries. |
| E-4 | No research can be resumed, extended or refreshed. Running the same topic again starts from nothing. |
| E-5 | The host runner calls `search_via`, which does not record what the search returned; only `search` does. So no address off the trusted list is ever allowed to be fetched, and every such result becomes a lead (`research_host.rs`, `LiveWeb`). Read in code, not run. |
| E-6 | A page is fetched three times in a host run (to read, to capture, to check), so the stored hash may not be of the text the scout read. The check re-finds the scout's own quote on the same address and can never contradict it, and it spends the same step budget as the search. |
| E-7 | In the audited workspace the central library held 38 sources, no claims and no pages. |

Objective: every piece of research Delphi does, whoever asked, leaves one record of the same shape in one place; its claims are library claims; and the record can be opened, continued and brought up to date later.

### 1.1 Non-goals

- How the scout plans, searches and writes the answer: [`specs/114`](114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis.md).
- Research a host agent does with its own tools: [`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md). It writes the record this spec defines.
- No change to who may ask for research or to the operator card an inside agent's request raises.
- No change to the trusted list, the egress rules of [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §7.5 or the private-read mark of [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md).

### 1.2 Simpler alternative considered

*Keep both runners and copy the host runner's claims into the library.* Rejected: it fixes E-2 and leaves two page formats, two sets of limits, one runner with a search log and one without, and every later change to be made twice. Kept from it: R2 is built before the runners are merged, because it is the smallest change that puts material in the library.

---

## 2. The research record

1. A research topic is a folder: `<workspace>/research/topics/<topic-id>/`, or `<project>/research/topics/<topic-id>/` when the run belongs to a project. The id is a slug of the question plus eight hex characters of its hash.
2. `record.json` holds: the question as asked; who asked (operator, role, or host and model); the project, if any; depth; state (`queued`, `running`, `done`, `partial`, `failed`, `cancelled`); the plan (114); every query with its provider and count of results; every page read, with its source id and content hash; pages refused and why; the claim ids it produced; the gaps it reports; token and time use; and `runs`, one entry per time the topic was run or continued.
3. `answer.md` is the written answer with its citation markers (114 §5). `search-log.json` is the log of 101, moved here.
4. The record is written as the run goes, with an atomic rename at each step, so a run that is stopped leaves a record that says how far it got.
5. `research_result` and `research_status` read the record from disk. A result is available for as long as the record exists, not for an hour.
6. Record text is data. The question and the answer carry the taint of what they were made from, as a page does today.

**Amendment (2026-11-21, defect D3 of the Stage 0 experiment).** The same question text is the same topic (item 1), so two runs of one question, even at once and even with `new: true`, share one record. Until now the record kept one `result`, and the earlier run's id answered `superseded_by`: concurrent callers each lost their own run. The record now also keeps `results`, a list of `{run, result}` entries, one per finished run that returned a result, newest last and at most eight. Each entry is that run's result with its own answer (not the topic's combined `answer.md`) and without its search log, which stays in `search-log.json` for the newest run only. `result` and `result_run` stay as before, the newest run's, for existing readers. `research_result(run)` for the newest run reads as before; for an earlier run it returns that run's entry; a run whose entry was dropped by the bound, or that finished before this amendment, still answers `superseded_by`. A record without `results` loads as before. `new: true` only stops a run from being folded into a different, overlapping topic (114 §3 item 3); its tool description says so.

## 3. Claims reach the library

1. Each claim a run keeps is written as a library claim (`library/claims/<id>.json`) with status left to be derived, as 055 and 100 say: it starts `noted`.
2. The claim carries its anchors (source id, the quote, the locator), `scope` and `project` when the run belongs to a project, and `topics: [<topic-id>]`.
3. A claim whose fingerprint matches an existing library claim adds an anchor and the topic id to that claim instead of making a second one. This is how corroboration counts rise across runs.
4. `host-claims.jsonl` stays as an append-only log of what each run extracted. A one-time command, `delphi research migrate`, reads an existing file and writes the library claims it implies; it prints what it would do first and changes nothing without `--apply`.
5. The search index gains a zone `research` covering topic records, answers and library claims, labelled by the claim's derived status. What a role may see follows the rules that already apply to the library.

## 4. One runner

1. `research.start` (console) and the inside agent's `request_research` call the runner of `research_run.rs`. The page-per-source writer of `research.rs` becomes one step of that runner: when the licence allows the text to be kept, the source's text is kept in the library, where pages gather it.
2. Existing files under `local/knowledge/web/` stay indexed and readable. Nothing new is written there. A later slice may fold them into the library; this spec does not.
3. One page is fetched once per run. The text the scout read is the text that is hashed, captured and checked.
4. The check has a step budget of its own (a quarter of the run's, at least two steps) and does not take from the search.
5. `search_via` records what it returned, as `search` does, so a result of a search the run made can be fetched under the rule that already exists. The `research.observed` event is written.
6. The limits of the two runners become one table in config, with today's host limits as the defaults.

## 5. Continue and refresh

1. `research.continue {topic_id, note?}` (console, CLI `delphi research continue`, and the host tool `request_research` with `topic_id`) starts a new run on an existing record. It begins from the record's gaps and skips pages already held whose content has not changed.
2. A topic is *due* when its newest run is older than its `refresh_days` (default 90, set by the scout from how fast the subject moves, clamped by config) or when a source it rests on has changed or been retracted. Due topics are listed (`delphi research topics --due`, the digest). Nothing is re-run without the operator or an agent asking; a refresh spends tokens and reaches the network.
3. `delphi research topics|show <id>` lists and prints records.

## 6. Acceptance criteria

1. **AC-1:** a console run, an inside agent's run and a host run on the same question each leave a `record.json` that validates against one schema.
2. **AC-2:** after a run that kept claims, `library/claims/` holds them, a page search on the topic gathers them, and `search_kb` returns them labelled `noted`.
3. **AC-3:** `research_result` returns a finished run's answer after the daemon has been restarted.
4. **AC-4:** two runs that find the same claim on two origins produce one library claim with two anchors.
5. **AC-5:** in a test with a search that returns one address off the trusted list, the runner fetches it; with an address no search returned, it does not.
6. **AC-6:** a test transport counts one fetch per page per run.
7. **AC-7:** `research.continue` on a record with one gap makes queries for that gap and re-reads no unchanged page.
8. **AC-8:** `delphi research migrate` without `--apply` writes nothing.

## 7. Verification plan

- Unit tests on the record (write, resume after a stop, schema), on claim merging (AC-4) and on the migrate command (AC-8).
- Runner tests on the existing fake web and fake library (AC-1, AC-5, AC-6, AC-7); the source-scan test that pins where results are remembered is changed with R4 and says why.
- One daemon test: run, restart, read the result (AC-3); one index test (AC-2).
- A live run on the real web is the proof slice of 119, not of this spec.

## 8. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| R0 | this spec | 1-2 |
| R1 | the record on disk, written as the run goes; `research_result` and `research_status` read it; `delphi research topics` and `show` | 6-8 |
| R2 | run claims become library claims, merged by fingerprint; `delphi research migrate` | 8-10 |
| R3 | the fixes of §4.3 to §4.5: one fetch, the check's own budget, `search_via` remembers | 6-8 |
| R4 | one runner: the console and inside agents call it; kept text goes to the library; one limits table | 12-16 |
| R5 | zone `research` in the index: records, answers, claims | 6-8 |
| R6 | continue and due topics | 8-10 |
| | **Total** | **47-62** |

R2 and R3 do not depend on R4 and can be built first.

## 9. Authority and trust

- A run fetches nothing it could not fetch before, except that R3 makes the existing rule of 098 §7.5 (a result of the run's own search) take effect; the red-team reviews that slice.
- Claims from a run are `noted` and untrusted. Writing them to the library raises no trust: status is derived by the rules of 100.
- A record in a project folder is read under the project's rules; whether another project may read it is [`specs/115`](115-research-recall-across-projects.md).

## 10. Open questions (the operator decides)

1. Should the old pages under `local/knowledge/web/` be converted to library sources, or left as they are?
2. Default `refresh_days`: 90, or set per trusted-list entry?
