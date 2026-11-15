---
id: spec-114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis
title: "114: A methodical scout: known first, a plan from the gaps, the role in every call, and a written synthesis"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: approved   # by the operator, 2026-11-14, in a host session: "all other specs are approved", after the drafts and their slice tables were shown. Section 12 (2026-11-15) is an amendment written from the operator's words of that day; it takes effect when the operator merges its pull request.
approved_by: operator (2026-11-14)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36); nothing was run
source_mission: 2026-11-14-knowledge-loop
tags: [spec, research, scout, method, retrieval, lessons]
---

# Spec 114: A methodical scout

> **Status: approved by the operator, 2026-11-14** (see the frontmatter). Third of eight specs (112 to 119) from the knowledge-loop audit.
> **Extends:** [`specs/066`](066-mcp-research-endpoint-and-role-identity.md) §5 (the search order and the learning loop, specified and not built), [`specs/101`](101-library-practice-for-research-a-search-log-retraction-lookup-archived-copies-and-citation-chaining.md), [`specs/112`](112-one-research-record-a-single-runner-topics-that-persist-and-claims-that-reach-the-library.md) (the record and the one runner).

---

## 1. Scope and objectives

| # | Observation (code on `main`, 2026-11-14) |
| :-- | :-- |
| E-1 | The model call the runner makes as the scout is built by `scout_ipr` (`research.rs`): role "You are the scout.", no persona, no skill text, heuristics "None.", lessons "None.". The scout's charter, its five skills and its lessons file reach no research run. The page writer's call (`page_run.rs`) is built the same way. |
| E-2 | Neither runner looks at what Delphi already holds before it searches. The only use of prior knowledge is an exact-hash match on a claim after it has been extracted. |
| E-3 | The plan is at most three fixed queries: the topic, the topic restricted to a trusted site, and the topic with "limitations OR criticism OR disputed". "Three to five questions" is prose in the literature-survey skill. |
| E-4 | A run stops on a budget (pages, steps, time, tokens). Nothing stops it because the question is answered or keeps it going because a part is not. |
| E-5 | The host run's `answer` is a list of the first eight claims built by code, and is labelled "model synthesis". The console run has no synthesis across pages. |
| E-6 | Results within one search are read in the order the provider gave them, trusted hosts first. |
| E-7 | The one-note-per-run learning loop of 066 §5 has no code. A search of the knowledge base that finds nothing is logged from the console and the CLI only, and nothing reads the log. |

Objective: a research run starts from what is known, plans from what is not, is the scout Delphi describes, and ends with a written answer whose every statement can be followed to a claim.

### 1.1 Non-goals

- No new search provider and no change to what may be fetched.
- The record's shape, the library claims and the continue command are 112.
- Validation of claims stays with the validator ([`specs/040`](040-validator-role-and-fact-ratification.md), [`specs/100`](100-confidence-ladder-substantiated-vetted-and-an-audit-that-rolls-back.md)).

### 1.2 Simpler alternative considered

*Give the scout a tool loop: search, fetch and knowledge-base tools, and let the model run the method from its skill text.* Rejected for the daemon's runner: page text is untrusted, and a model that reads it while holding a fetch tool can be steered by it; today's design (code decides what is fetched, the model only reads and reports) is the safer one and is kept. The method therefore lives in code, with the model called for the three things only a model can do: break the question down, judge a page, and write the answer.

---

## 2. The role in every call

1. `scout_ipr` and the page writer's builder take the role's charter excerpt ([`specs/106`](106-compact-packs-and-role-subagents-a-charter-excerpt-a-hash-check-and-a-worker-file-for-every-role.md)), the text of the skills named for the step (plan: literature-survey; judge a page: dual-horizon-search; write: multi-source-synthesis; page: encyclopedia-synthesis), and the role's ratified heuristics and lessons from the same snapshot a mission span gets.
2. The governance line of each call is unchanged, and stays first: page text is data.
3. Only ratified text is loaded. A budget (2,000 tokens by default) caps it; what does not fit is dropped and counted, as the snapshot loader does.
4. A test fails if a daemon-built call for a role passes the literal "None." for lessons while that role's ratified lessons file has a rule.

## 3. Known first

1. Before the first search the runner asks the retrieval service for the question, over library claims, research topics and answers, kept source text, and the knowledge base, with the caller's visibility.
2. What it finds is written to the record as `known`: claim ids with their status, topics that overlap, sources already held with their age.
3. A topic that overlaps strongly (by default, half of the question's content words and a retrieval score over a set bar) is offered instead of a new one: the run becomes a continue of that topic (112 §5) unless the caller passed `new: true`.
4. A source already held, unchanged and checked within its recheck window, is read from the library and not fetched.
5. The result reports `known_before`, so a caller can see what the run added.

## 4. A plan from the gaps

1. One model call (the scout, no tools) gets the question and the `known` list and returns three to five sub-questions, each marked `known`, `partly` or `open`, with up to two queries for each that is not `known`. The call is given no page text.
2. If the call fails or returns nothing usable, the fixed plan of today is used and the record says `plan: fixed`.
3. After each round of reading, code recomputes coverage: a sub-question is covered when at least two kept claims from two origins are tagged to it (the scout tags each claim with a sub-question number when it extracts it).
4. The next round's queries come from the sub-questions still uncovered. For a sub-question with claims from one origin only, the query asks for a second origin; for one with none, the query is reworded once.
5. The run stops when every sub-question is covered, or when two rounds in a row add no new claim, or on a budget. The record says which.
6. Within a search's results, pages are read in this order: trusted and primary, trusted, already cited by a kept claim's source, the rest; newer first within a group when the date is known.
7. The contradiction query of today is kept, once per run, and its claims are tagged as counter-evidence.

## 5. A written answer

1. One model call (the scout, no tools) gets the sub-questions and the kept claims with their ids and writes the answer in Markdown: a short lead, one section per sub-question, and "Not found" for what stayed open. Every sentence that states a fact ends with claim markers.
2. The page check of 097 is run on the answer: a marker that names no claim, or a quoted phrase not in the cited source, fails it. On failure the answer is rewritten once; if it fails again, the code-built list is used.
3. The label says what was done: `written by the scout from N claims, not validated` or `list of claims (no synthesis)`. The words "model synthesis" are not applied to a code-built list.
4. Contested claims are stated as contested, with both sides.

## 6. What a run teaches

1. At the end of a run the runner stages one note for the scout's memory: which queries and which hosts produced kept claims, and which produced none. It goes where [`specs/116`](116-memory-that-grows-a-provisional-tier-findings-consolidation-and-a-larger-start-context.md) puts agent notes.
2. Searches of the knowledge base by agents and hosts that return nothing are logged with the console's. `delphi research suggest` lists the questions most often asked and not answered. Nothing is researched without being asked.

## 7. Acceptance criteria

1. **AC-1:** with a ratified scout lesson in the workspace, the text of the scout's call contains it.
2. **AC-2:** a run on a question whose answer is already held as library claims reports them under `known_before` and makes no fetch for sources held and fresh.
3. **AC-3:** a run on a fixture with three sub-questions, one of which the fake web cannot answer, ends with two covered and one open, and its second-round queries concern the open one.
4. **AC-4:** a run stops after two rounds that add no claim, before its page budget.
5. **AC-5:** an answer with a marker for a claim the run does not hold is rejected by the check.
6. **AC-6:** a run whose synthesis call fails returns the list labelled `list of claims (no synthesis)`.
7. **AC-7:** the plan call's input contains no fetched page text.

## 8. Verification plan

- Builder tests on the call text (AC-1, AC-7) and the "None." test of §2.4.
- Runner tests on the fake web with a scripted model (AC-2 to AC-6).
- An evaluation, not a gate: ten fixed questions run before and after, comparing sub-questions covered, claims with two origins, and tokens. Recorded in the tracker.

## 9. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| M0 | this spec | 1-2 |
| M1 | the role, skills, heuristics and lessons in the scout's and the page writer's calls | 4-6 |
| M2 | known first: retrieval before search, `known_before`, held sources not re-fetched, offer to continue | 6-8 |
| M3 | the plan call, sub-question tags on claims, the fixed fallback | 8-10 |
| M4 | coverage, queries from gaps, stopping rules, reading order | 8-10 |
| M5 | the written answer, its check, honest labels | 6-8 |
| M6 | the run note; agent and host misses logged; `delphi research suggest` | 3-4 |
| M7 | the ten-question evaluation | 3-4 |
| | **Total** | **39-52** |

M1 stands alone. M2 to M5 depend on 112 R4.

## 10. Authority and trust

- No call that reads page text holds a tool. The plan call sees no page text. This is today's separation, kept.
- Lessons and heuristics loaded are the ratified ones only.
- More model calls per run: the plan and the answer add two. Token limits per run are unchanged; the calls are counted inside them.

## 11. Open questions (the operator decides)

1. Coverage rule: two claims from two origins per sub-question, or one claim from a primary source is enough?
2. Should a strongly overlapping topic be continued by default (§3.3), or should the run always ask?

## 12. Amendment (operator, 2026-11-15): a short charter excerpt for the scout, in the plan and answer calls only

The operator's words, in a host session on 2026-11-15, after being told that the excerpt never fits: "for the scout, write a focused short excerpt with just what the scout needs to know for its tasks, and use it only on plan and answer calls — a hybrid of 2 and 3." (The options they were shown: 1, leave it dropped; 2, carry the excerpt only in the plan and answer calls; 3, a short excerpt for the scout alone.) The operator approves this section by merging the pull request that adds it.

1. Found while building M1: the charter excerpt of [`specs/106`](106-compact-packs-and-role-subagents-a-charter-excerpt-a-hash-check-and-a-worker-file-for-every-role.md) §2 measures about 2,585 tokens, more than the whole 2,000-token budget of §2 item 3, so in the scout's calls it was always dropped and counted.
2. The scout's calls carry a **scout excerpt** in its place: parts of the ratified `GOVERNANCE.md` chosen by a fixed table and copied verbatim, in charter order, under the charter's own headings, as 106 §2 does. The table: §2's opening paragraph, the table's header and the `scout` row; §4 item 3 (information flow control: taint is computed, never declared); §5 item 3 (secret hygiene). It ends with the closing note 106 §2 writes: what was left out, the charter's hash, and that the whole charter governs. No other words are added.
3. It is carried in two calls only: the plan call (§4) and the answer call (§5). The call that judges a page carries no excerpt; its governance line stays the only rule it is given.
4. In those two calls it is loaded first, before the lessons, the skill and the heuristics, and counts against the budget of §2 item 3; whatever does not fit after it is dropped and counted as before. A test pins its size below 600 tokens with the repository's charter, and fails if the scout excerpt is ever found in a page-judging call.
5. Fail closed: when a selected part is not found exactly once in the ratified charter, the scout excerpt is left out of the call and the omission is noted, as a dropped excerpt is today. It is never replaced by the whole charter, which does not fit.
6. The page writer's call and the excerpt a host role is given (106 §2) are unchanged.
7. Slice: **M8**, the scout excerpt (2 to 3 hours), built with or after M6.
