---
id: spec-109-one-retrieval-path-for-host-search-a-fresh-index-and-passages-at-adoption
title: "109: One retrieval path for host search, a fresh index, and passages at adoption"
author: architect
timestamp: 2026-11-13T00:00:00Z
status: approved   # by the operator, 2026-11-13, in a host session ("approve all your plans and suggestions and go"). The approval was given to the plan this spec carries out (workstream 5: one retrieval engine for all five search tools, automatic index refresh, a small retrieval at role adoption) BEFORE this text was written; the operator has not read the text.
approved_by: operator (2026-11-13, in advance, on the plan)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (8592bb09) and one live `kb_retrieve` call; the trust-filter claim of E-4 was read in code and not checked on a live index
source_mission: 2026-11-13-host-experience
tags: [spec, hosts, retrieval, kb, memory, mcp]
---

# Spec 109: One retrieval path for host search

> **Status: approved in advance, 2026-11-13** (see the frontmatter).
> **Extends:** [`specs/064`](064-knowledge-retrieval-for-agents-and-mcp.md) (whose §3.1 already says `retrieval.rs` is the only search implementation; R1 and R2 are partly built) and [`specs/004`](004-memory-and-coaching.md) §3.

---

## 1. Scope and objectives

| # | Observation (code on `main`, 2026-11-13) |
| :-- | :-- |
| E-1 | Five host search tools use three rankings: `search_kb` uses the retrieval service; `kb_retrieve` uses keyword tables with no embedder for core text and a word count for project text, and sorts the two score scales together; `search_project`, `search_memory` and `query_memory` use a per-file word count that the index may only reorder. A question in ordinary words about host friction returned an adapter diagram and none of the specs on the subject. |
| E-2 | `kb_retrieve` opens the index with a call that sets the journal mode and creates the schema, though readers are meant to write nothing. Its project chunk ids are a hash modulo 900 and can collide. |
| E-3 | `search_kb` never refreshes the index; the console's search does. |
| E-4 | Core, context and spec files without a tier in their frontmatter index as `external_untrusted`, and `search_kb` excludes untrusted documents, so much of the release's own reference may be invisible to a host. |
| E-5 | Nothing is retrieved for a host unless it asks. Delphi's own agents get up to three passages at the start of a span. |

Objective: the same question gets the same ranking from every host search tool; the index is fresh when it is read; a host that adopts a role for a task is handed the few passages most likely to matter.

### 1.1 Non-goals

- No new corpus, embedder or ranking model. The visibility rules of the index (a role sees only its own memory; staged, episodes and other projects stay out) are unchanged.
- No host read counter (080); it stays where 080 left it.

### 1.2 Simpler alternative considered

*Tell hosts to use `search_kb` only.* Rejected: it needs the daemon, returns pointers not passages, and does not cover memory. Kept in spirit: one implementation behind every tool.

---

## 2. Q1: every tool asks the retrieval service

1. `kb_retrieve`, `search_project`, `search_memory` and `query_memory` call `retrieval::search` (hybrid when vectors exist, keywords otherwise, the reranker off, the 800 ms fallback) with a filter that expresses each tool's present corpus as zones, layers and path rules. What each tool may read does not widen: a test pins each tool's corpus against today's with a fixture workspace.
2. The MCP process opens the index **read-only** (no pragma write, no schema creation). When there is no index, or it lacks a zone the tool needs, the tool falls back to today's word ranker and says `ranking: "words (no index)"`; otherwise `ranking: "index"`.
3. `kb_retrieve` returns passages by the index's own chunk ids for indexed text. For text served by the fallback, the id is the first 12 hex characters of the hash of path and line range. One score scale orders one answer.
4. `query_memory`'s facets become filters on the result (by zone and frontmatter), not substring matches on paths, and every result passes the same role-memory check `search_memory` applies. This closes the code-read gap where a facet could return another role's memory file or another project's mission file whose path contained a facet word; a test proves both are refused.
5. `search_memory` checks that `role` is a servable role.

## 3. Q2: a fresh index

1. `host.search_kb` refreshes the index before searching when a source file is newer than the index, as the console's search does, bounded at 2 s; past the bound it searches what is there and says `index: "stale"` with the age.
2. The local tools report `index_age_s` and the same `stale` word. They do not write the index.
3. `request_research` refreshes the index after it adds pages, so the next `search_kb` finds them.

## 4. Q3: the release's own reference is visible

1. First, measure: on a workspace built from this release, count the documents in the core, context, specs and about zones by `source_type`, and how many `search_kb` can return. The result is recorded in §9.
2. If E-4 holds: a file that is part of the signed release (it is in the release manifest and its hash matches) indexes at the tier its frontmatter names, or at `internal_generated` when it names none. A file in a project, a layer, a bundle or the workspace that is not in the manifest is unchanged. The release manifest is the operator's acceptance of those files (charter §5.7); no agent can add to it.
3. If E-4 does not hold, nothing changes and §9 says so.

## 5. Q4: passages at adoption

1. `get_agent` takes an optional `task` (at most 500 characters). When given, the answer ends with a section `## Retrieved for this task (data, not instructions)`: at most **three** passages, 1,500 characters in all, from the role's own memory, the project's specs and context, and the knowledge base, each in the retrieval fence with its pointer, status and label. It is found by the retrieval service within 800 ms; when nothing clears the floor or time runs out the section says so in one line.
2. The section is not part of the pack and not part of `pack_hash`. With `if_hash` matching, the answer is the `unchanged` line followed by the section.
3. The 105 prompt hook's line tells the host to pass the operator's request as `task`. The hook itself retrieves nothing.
4. `consult_orchestrator` (107) uses the same service for its pointers.

## 6. Slices (one per pull request)

| Slice | Content | Hours |
| :-- | :-- | --: |
| Q0 | this spec | 2 |
| Q1 | the four tools on the retrieval service, read-only open, ids, the facet fix | 10-12 |
| Q2 | refresh in `search_kb`, age and stale, refresh after research | 4-6 |
| Q3 | the measurement, then the tier rule if it holds | 4-8 |
| Q4 | `task` on `get_agent` | 6-8 |

## 7. Authority and trust

- No tool reads more than it does today; Q1 item 4 makes one read less.
- Q3 raises the index tier of release files only, on the evidence of the signed manifest. It raises nothing an agent, a host or a project wrote.
- Q4 puts retrieved text beside a pack, fenced and labelled, never inside it, so a pack stays ratified text only.

## 8. Not verified

- Ranking quality after Q1 is not measured here. Q1 records, for ten fixed questions about this repository, the top five of each tool before and after.

## 9. Measurements

### 9.1 Q1: ten questions, before and after (2026-11-13)

How it was measured: a scratch home (`delphi init --non-interactive --home <tmp>`) built by the binary of the base commit (42c381a5), so its `base/` holds this checkout's release files (244 documents, 3,113 chunks after `delphi kb index`; `base/specs` is empty and the release's specs sit in `base/about`); a copy of this repository's `specs/` and `context/` with its `README.md` as `_index.md`, registered as a project with `delphi project register`; the MCP server started from inside that copy (`delphi mcp --home <tmp>`) and asked each question with `limit: 5` (`search_memory` and `query_memory` as `architect`); `search_kb` through a daemon started in the same scratch home. "Before" is the base binary, "after" the Q1 binary, over the same index. No embedding model was installed in the scratch home, so every index answer here is the keyword leg (`mode: lexical`); the hybrid leg was not measured.

Unchanged in all ten: `search_kb` (its core leg already used the retrieval service and its project leg kept the same word ranking) and `search_project` (the reordering it took from the index no longer applies, and with no model it never had). `search_memory` was identical in 9 of 10, `query_memory` in 5 of 10, `kb_retrieve` in 0 of 10. The changes below are rank, not corpus: what each tool can return is pinned by `each_tool_reads_exactly_its_own_corpus_with_or_without_an_index`. Two effects show: the index matches whole words where the word ranker matched substrings (Q3 `search_memory` found nothing for "council decision recorded" in the one seed lesson file, which the substring `record` had matched), and `kb_retrieve` now interleaves the index's passages with the project's by rank where it used to sort two score scales together, so project spec chunks no longer crowd out the core reference. Paths are shortened (`p:` a project file; the first folder and the file name). Whether the answers are better is not judged here.

**Question 1. how does the daemon decide which model a role uses**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | specs/035-failover-without-a-driver; specs/092-operator-authorized-tests; context/invariant-enforcement | context/invariant-enforcement; specs/092-operator-authorized-tests; specs/009-model-access-and-auth; specs/098-where-agents-run-host-first-locu (ranking index) |
| search_memory | memory/lessons | memory/lessons (ranking index) |
| query_memory | p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/015-provisional-decisions; p:specs/068-research-capture-and-summary-p; p:specs/044-decisions-pane | memory/lessons; p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/015-provisional-decisions; p:specs/068-research-capture-and-summary-p (ranking index) |

**Question 2. what keeps untrusted text out of an agent prompt**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | specs/092-operator-authorized-tests; context/invariant-enforcement; specs/035-failover-without-a-driver; specs/025-plugin-skill-and-tool-compatibil; specs/004-memory-and-coaching | context/multi-provider-and-tui-architectur; context/mas-architecture-and-failure-modes (ranking index) |
| search_memory | memory/lessons | memory/lessons (ranking index) |
| query_memory | p:specs/086-outside-council-panel-and-host; p:specs/044-decisions-pane; p:specs/068-research-capture-and-summary-p; p:specs/015-provisional-decisions; p:specs/075-kb-search-operator-presentatio | memory/lessons; p:specs/086-outside-council-panel-and-host; p:specs/044-decisions-pane; p:specs/068-research-capture-and-summary-p; p:specs/015-provisional-decisions (ranking index) |

**Question 3. how is a council decision recorded**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | specs/035-failover-without-a-driver; specs/098-where-agents-run-host-first-locu; specs/084-outcome-flip-question-resume-rec; specs/102-the-exchange-record-the-operator; specs/027-bundle-authoring-distribution-an | context/invariant-enforcement; specs/035-failover-without-a-driver; specs/098-where-agents-run-host-first-locu; specs/084-outcome-flip-question-resume-rec (ranking index) |
| search_memory | memory/lessons | (none) (ranking index) |
| query_memory | p:specs/044-decisions-pane; p:specs/086-outside-council-panel-and-host; p:specs/015-provisional-decisions; p:specs/003-collaboration-and-council; p:specs/078-council-scorecard | p:specs/044-decisions-pane; p:specs/086-outside-council-panel-and-host; p:specs/015-provisional-decisions; p:specs/003-collaboration-and-council; p:specs/078-council-scorecard (ranking index) |

**Question 4. how do I merge a pull request into core**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | specs/092-operator-authorized-tests; specs/008-ambient-watch-and-rewind; specs/062-delphi-as-host-agent-mcp-server; specs/016-headless-run; specs/005-console-and-narrator-tui | context/invariant-enforcement; specs/092-operator-authorized-tests; specs/008-ambient-watch-and-rewind; specs/062-delphi-as-host-agent-mcp-server; specs/016-headless-run (ranking index) |
| search_memory | (none) | (none) (ranking index) |
| query_memory | p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/044-decisions-pane; p:specs/068-research-capture-and-summary-p; p:specs/015-provisional-decisions | p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/044-decisions-pane; p:specs/068-research-capture-and-summary-p; p:specs/015-provisional-decisions (ranking index) |

**Question 5. how does the knowledge base index stay fresh**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | specs/023-org-bundle-standard; specs/027-bundle-authoring-distribution-an; specs/004-memory-and-coaching; context/invariant-enforcement; specs/088-spec-compile-and-current-set | context/index; context/multi-provider-and-tui-architectur; context/mas-architecture-and-failure-modes (ranking index) |
| search_memory | (none) | (none) (ranking index) |
| query_memory | p:specs/068-research-capture-and-summary-p; p:specs/015-provisional-decisions; p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/075-kb-search-operator-presentatio | p:specs/068-research-capture-and-summary-p; p:specs/015-provisional-decisions; p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/075-kb-search-operator-presentatio (ranking index) |

**Question 6. what can a host agent read through the MCP server**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | specs/092-operator-authorized-tests; specs/025-plugin-skill-and-tool-compatibil; specs/074-stable-launch-path-and-delphi-co; specs/009-model-access-and-auth | context/invariant-enforcement; SPEC; specs/092-operator-authorized-tests; specs/025-plugin-skill-and-tool-compatibil; specs/074-stable-launch-path-and-delphi-co (ranking index) |
| search_memory | memory/lessons | memory/lessons (ranking index) |
| query_memory | p:specs/086-outside-council-panel-and-host; p:specs/068-research-capture-and-summary-p; p:specs/044-decisions-pane; p:specs/003-collaboration-and-council; p:specs/015-provisional-decisions | memory/lessons; p:specs/086-outside-council-panel-and-host; p:specs/068-research-capture-and-summary-p; p:specs/044-decisions-pane; p:specs/003-collaboration-and-council (ranking index) |

**Question 7. how are private notes kept from leaving the machine**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | specs/092-operator-authorized-tests; specs/031-layered-learning-and-training-mo; specs/027-bundle-authoring-distribution-an; specs/098-where-agents-run-host-first-locu; specs/103-the-private-read-mark-closing-th | context/invariant-enforcement; context/model-selection-evidence; context/protocols-edge-and-paradigms; context/federation-rag-and-distribution (ranking index) |
| search_memory | memory/lessons | memory/lessons (ranking index) |
| query_memory | p:specs/068-research-capture-and-summary-p; p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/044-decisions-pane; p:specs/015-provisional-decisions | memory/lessons; p:specs/068-research-capture-and-summary-p; p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/044-decisions-pane (ranking index) |

**Question 8. what happens when a build gate fails**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | specs/035-failover-without-a-driver; specs/098-where-agents-run-host-first-locu; specs/092-operator-authorized-tests; context/invariant-enforcement; specs/025-plugin-skill-and-tool-compatibil | context/mas-architecture-and-failure-modes; context/case-studies-and-maneuvers; context/model-capabilities; context/model-selection-evidence; context/registers-and-tonal-calibration (ranking index) |
| search_memory | (none) | (none) (ranking index) |
| query_memory | p:specs/068-research-capture-and-summary-p; p:specs/015-provisional-decisions; p:specs/003-collaboration-and-council; p:specs/086-outside-council-panel-and-host; p:specs/044-decisions-pane | p:specs/068-research-capture-and-summary-p; p:specs/015-provisional-decisions; p:specs/003-collaboration-and-council; p:specs/086-outside-council-panel-and-host; p:specs/044-decisions-pane (ranking index) |

**Question 9. how does the sandbox limit what a task can do**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | specs/092-operator-authorized-tests; context/invariant-enforcement; specs/035-failover-without-a-driver; specs/009-model-access-and-auth | context/invariant-enforcement; REVIEW; specs/092-operator-authorized-tests; GOVERNANCE; specs/009-model-access-and-auth (ranking index) |
| search_memory | (none) | (none) (ranking index) |
| query_memory | p:specs/044-decisions-pane; p:specs/015-provisional-decisions; p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/068-research-capture-and-summary-p | p:specs/044-decisions-pane; p:specs/015-provisional-decisions; p:specs/086-outside-council-panel-and-host; p:specs/003-collaboration-and-council; p:specs/068-research-capture-and-summary-p (ranking index) |

**Question 10. how are role memories and lessons promoted**

| Tool | Before | After |
| :-- | :-- | :-- |
| kb_retrieve | context/invariant-enforcement; specs/035-failover-without-a-driver; specs/092-operator-authorized-tests; specs/004-memory-and-coaching | context/github-review-and-learning-loops; GOVERNANCE; specs/062-delphi-as-host-agent-mcp-server; context/invariant-enforcement (ranking index) |
| search_memory | memory/lessons | memory/lessons (ranking index) |
| query_memory | p:specs/086-outside-council-panel-and-host; p:specs/068-research-capture-and-summary-p; p:specs/044-decisions-pane; p:specs/003-collaboration-and-council; p:specs/015-provisional-decisions | memory/lessons; p:specs/086-outside-council-panel-and-host; p:specs/068-research-capture-and-summary-p; p:specs/044-decisions-pane; p:specs/003-collaboration-and-council (ranking index) |

## 10. Amendments from review (2026-11-13)

A red-team review on 2026-11-13 (one critic, the author's own model family, so `homogeneous`; checked against the code) found the defects below. Each amendment **supersedes** the text above where they differ. The review ran after the advance approval; the operator has not read these either.

| # | Sev | Finding | Amendment |
| :-- | :-- | :-- | :-- |
| A1 | P1 | The registered project's files are not in the index (`kb::sources` indexes the workspace, missions, memory, bundles and the library). Even `search_kb` ranks project text with a word ranker, so §1 E-1 was half right, and Q1 cannot express the project tools as index filters without a new corpus. | Q1 is re-scoped. For the workspace corpus (core, context, specs, knowledge, library, memory) every tool asks the retrieval service, as written. For project text there is **one** word ranker, shared by `search_kb`'s project leg, `search_project` and `kb_retrieve`'s project leg (three exist today), and when project and index results share an answer they are merged by rank (reciprocal rank fusion), never by raw score. Indexing registered projects was an open decision; the operator decided it on 2026-11-13 (§11). |
| A2 | P1 | Q3's "signed release manifest" does not exist at index time: `base/manifest.lock` is a plain hash list a shell can edit, and its `signature` field is a label. The release reference already lives in `base/about` at `user_ratified`, so E-4 was overstated. | Q3 item 2 is **struck**. Q3 is the measurement only (item 1). If it shows release text a host cannot find, the result goes to the operator with options; nothing raises a tier. |
| A3 | P2 | The `query_memory` facet leak (Q1 item 4) is live today and should not wait for the larger slice. | It is its own first slice, **Q0a**: facets filter by zone and frontmatter and every result passes the role-memory check, with tests that another role's memory file and another project's mission file are refused. |

## 11. Decision: index registered projects (operator, 2026-11-13: "index projects — yes please")

The operator decided the open question of amendment A1. It adds one slice, **Q1b**, after Q1.

1. **What is indexed.** For each registered project, exactly the files the project tools read today: `_index.md`, `specs/`, `plans/`, `context/`, `research/`, and the project layer's context and knowledge. Not the project's source tree, not `work/`, not anything outside those folders. A file over the index's present size cap, a binary file and a symlink that leaves the folder are skipped, as the word ranker skips them today.
2. **Where.** In the workspace's existing index (`local/state.db`), as a layer named for the project's id. Embeddings are computed by the local embedder the index already uses; nothing leaves the machine.
3. **Who can read it.** A session reads only the layer of the project its working directory belongs to; the filter is in the query, as role memory's is. A session with no project reads no project layer. A test proves a session in one project gets nothing from another's layer.
4. **Tier.** Project files index at `external_untrusted` unless their frontmatter names a tier the daemon wrote. The project tools ask for untrusted project text explicitly and label every result as they do today; `search_kb`'s core leg still excludes untrusted text.
5. **Freshness.** The daemon refreshes a project's layer when a file under those folders is newer than the layer, at the same points Q2 names, and when the project is registered. Unregistering a project removes its layer.
6. **Private-read mark.** A project file marked private (103) is indexed with the mark, and reading it marks the session, as today.
7. **Fallback.** With no daemon or no layer yet, the shared word ranker of A1 answers and says `ranking: "words (no index)"`.
8. With Q1b built, `search_project` and the project legs of `search_kb` and `kb_retrieve` ask the retrieval service like the other tools, and the word ranker remains only as the fallback.

| Slice | Content | Hours |
| :-- | :-- | --: |
| Q1b | the project layer, its isolation, freshness and removal | 10-12 |
