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

To be filled by Q1 and Q3.

## 10. Amendments from review (2026-11-13)

A red-team review on 2026-11-13 (one critic, the author's own model family, so `homogeneous`; checked against the code) found the defects below. Each amendment **supersedes** the text above where they differ. The review ran after the advance approval; the operator has not read these either.

| # | Sev | Finding | Amendment |
| :-- | :-- | :-- | :-- |
| A1 | P1 | The registered project's files are not in the index (`kb::sources` indexes the workspace, missions, memory, bundles and the library). Even `search_kb` ranks project text with a word ranker, so §1 E-1 was half right, and Q1 cannot express the project tools as index filters without a new corpus. | Q1 is re-scoped. For the workspace corpus (core, context, specs, knowledge, library, memory) every tool asks the retrieval service, as written. For project text there is **one** word ranker, shared by `search_kb`'s project leg, `search_project` and `kb_retrieve`'s project leg (three exist today), and when project and index results share an answer they are merged by rank (reciprocal rank fusion), never by raw score. Indexing registered projects is a new corpus with a privacy question; it is recorded here as an open decision for the operator and is not built. |
| A2 | P1 | Q3's "signed release manifest" does not exist at index time: `base/manifest.lock` is a plain hash list a shell can edit, and its `signature` field is a label. The release reference already lives in `base/about` at `user_ratified`, so E-4 was overstated. | Q3 item 2 is **struck**. Q3 is the measurement only (item 1). If it shows release text a host cannot find, the result goes to the operator with options; nothing raises a tier. |
| A3 | P2 | The `query_memory` facet leak (Q1 item 4) is live today and should not wait for the larger slice. | It is its own first slice, **Q0a**: facets filter by zone and frontmatter and every result passes the role-memory check, with tests that another role's memory file and another project's mission file are refused. |
