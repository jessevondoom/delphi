---
id: spec-064-knowledge-retrieval-for-agents-and-mcp
title: "064: Knowledge retrieval for agents and MCP hosts, markdown and scratch tiering, and release self-knowledge"
author: architect
timestamp: 2026-11-01T00:00:00Z
status: approved   # approved by the operator on 2026-11-01: "draft the specs — they're approved as discussed — and begin building"
approved_by: operator (2026-11-01)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, knowledge, retrieval, rag, mcp, scope, scratch, markdown, about, search]
---

# Spec 064: Knowledge retrieval for agents and MCP hosts, markdown and scratch tiering, and release self-knowledge

**Status.** Approved by the operator on 2026-11-01 after a three-seat council review (red-team, strategist, architect). Builds on [`004`](004-memory-and-coaching.md), [`005`](005-console-and-narrator-tui.md), [`006`](006-installer-and-updater.md), [`043`](043-knowledge-window-search-page-and-growing-corpus.md), [`055`](055-the-central-library-sources-claims-and-pages.md), [`057`](057-search-pages-generated-and-checked.md), [`058`](058-project-scopes-scaffolding-and-promotion.md), [`061`](061-encyclopedia-search-pages.md) and [`062`](062-delphi-as-host-agent-mcp-server.md). Where it differs, this spec wins for the topics it names; the five specs carry a one-line pointer here.

## 1. Goal

Knowledge retrieval is critical for the operator and for agents. Today an agent's `kb_search` opens the index read-write on every call, never attaches the embedder (lexical ranking only) and returns pointers, not passages; an MCP host cannot search the knowledge base at all; Delphi cannot read its own release specs; and asking for a markdown note can raise an approval card. This spec makes retrieval fast, hybrid, scoped and measured, and removes friction from harmless writes without widening authority.

## 2. Principles (nothing here widens authority)

1. **Green is not trusted.** A write that needs no card still gets a daemon-assigned source tier, never taken from its location, extension or frontmatter. What is read back lowers the reading span's ceiling to that tier.
2. **Retrieval narrows, never widens.** Every visibility rule is applied in the index query before ranking, not after.
3. **The daemon is the only writer.** Readers are read-only; no reader refreshes the index.
4. **Release knowledge is reference, never authority.** It can never satisfy `Implements-Spec`.
5. **Fail closed.** An ambiguous project, an unresolvable folder, a missing daemon or an unready model degrades or refuses; it never searches wider.

## 3. One retrieval service

1. A module `crates/delphi-engine/src/retrieval.rs` is the only implementation of knowledge search. `tools.rs::kb_search`, `service.rs` `kb.search`, `kbserve.rs` and the MCP `search_kb` (§5) call it. `Kb` stays the index primitive.
2. **Readers.** A small fixed pool of read-only `Kb` handles (`Kb::open_readonly`), each with the embedder attached once. A reader serves one request at a time; no connection is shared across threads, and no lock is held while embedding.
3. **Writer.** The daemon alone runs `sync_lexical` and vector batches, debounced in the background. A search never starts a sync; it reads the latest committed generation.
4. **Freshness.** `kb_meta.index_generation` is bumped in the same transaction as any index change. A reader reopens only on a schema or model change or an unhealthy connection. Results carry the generation they read.
5. **Warm-up.** The embedder loads once after the daemon starts, off the request path. Until it is ready the mode is `lexical`.
6. **Result shape.** `{ rel_path, layer (home or project:<id>), zone, heading, start_line, end_line, preview (bounded), tier, status, mode }`. `mode` is `hybrid` or `lexical`, and says so when only lexical answered. Previews are quoted as data and carry the tier banner.
7. **Reranking.** Off for automatic retrieval and for agent `kb_search` by default; on for the operator's search and an explicit `rerank` argument.
8. **Budgets.** A hard deadline per request (default 800 ms for agents, then lexical result or none), a concurrency limit, query at most 500 characters, 3 to 5 passages for an agent by default (at most 10), passage text at most 600 characters.

## 4. Agent retrieval

1. **`kb_search` (agents).** Returns the §3.6 passages. A `project` filter defaults to the mission's scope (`scope::mission_scope`) and can only narrow. Reading a validated fact still emits `fact.read`.
2. **Span-start retrieval (minimal, then expand).** Off for child spans. For a root span only, when `[retrieval] auto = true` (default `true`), the daemon makes one deterministic query from the task's normalized request, and puts at most 3 passages (one per document, adjacent chunks merged, at most 1 500 characters in all) in the handoff as `@runtime/retrieved`, marked tainted at their tier. It is skipped for greetings, a task that names its files, a task whose handoff already carries context pointers, a repeated equivalent query, and when the budget (§3.8) is exceeded. No second model call writes the query.
3. **Measured.** The daemon records, per request, `retrieval.done` with `{mode, ms, hits, bytes, generation, auto}`. `delphi kb eval` reports recall@k and MRR for lexical, hybrid and reranked legs (existing), and the p50 and p95 of cold and warm latency. Expansion of the automatic policy (more passages, a reranker, child spans) needs these numbers first.

## 5. MCP `search_kb`

1. A read-only tool on the host server (`specs/062` §4.1). The MCP process does not open the index: it calls the daemon over `HostLink` (`host.search_kb`). With no daemon it returns an error and searches nothing.
2. **Visibility (an allowlist, in the query).** Core content: zones `core`, `context` (not `context/facts`), `specs`, `knowledge`, `library` pages and sources, active bundles. In a registered project, also that project's layer `knowledge` and `context` and its project-root `research/`. Never: any `memory`, `context/facts`, `local/`, `operator/`, staged notes, other projects' layers, worktrees, or `about/` (unless the argument `about: true`).
3. **Scope per request.** The daemon resolves the project from the folder of that request: the client's `roots` when offered, else the process folder. Every candidate is canonicalised and must lie in exactly one currently registered, canonical project root; the registry is re-read on each call. An ambiguous, missing or unregistered folder yields the home layer only, with the diagnostic in the result. Invalid supplied roots never fall back to another folder. The client never names a project id.
4. The result carries `scope` (`home` or `project:<id>`), `mode`, `generation` and per-result tier; every result is `external_untrusted` data to the host.
5. `search_memory` accepts a `role` only as a filter on the shared promoted summaries and the project layer; it never returns another role's memory (the caller-supplied `role` cannot widen).

## 6. Markdown and scratch tiering

1. **Markdown is Green** for `write_file` in every zone that role may write (including `@project/…` and `@worktree/…`), after the path guard resolved the path, except protected paths (`lock::is_protected`), `specs/**`, `memory/**` (written only by the daemon), and any path the guard denies. Everything else stays as it is today. Classification is never the boundary: the guard still decides.
2. **Scratch.** A first-class path-guard zone `scratch/` (`local/scratch/` on disk). Any non-executable regular file is Green to write; never ratified, never indexed as trusted. Refused: symlinks and hard links, executable extensions (a fixed list), any file with an execute bit, shell start-up, config and dependency-manifest names, path escapes, case variants of any of these.
3. **Tier.** Content written under §6.1 or §6.2 is stamped `internal_generated` at most, and at the lower tier of the writing span's ceiling (`gate_tainted_write`). Reading it back lowers the span ceiling to that tier before any model call.
4. **The spec gate.** A worktree diff is exempt from the `build_without_spec` card only when a deterministic check over the whole diff finds: additions and modifications only; every path unprotected Markdown or under `scratch/`; no renames, deletions, mode changes or symlinks; nothing under `.github/`, nothing a build or test reads (named in the project's build files), and none of the existing limits exceeded. Anything else is today's behavior. Merge gate 3 stays authoritative.
5. Markdown and scratch files are not building for the purpose of No Spec, No Build; GOVERNANCE §1.3 and `AGENTS.md` say so.

## 7. Release self-knowledge (`about/`)

1. The release's `specs/` and the generated `docs/reference/` are embedded as a read-only snapshot at `about/` (one release, one hash), installed to the base zone. It is its own zone with source tier `user_ratified` as a *release reference* and the label "release reference, not build authority".
2. Every role may read `about/**` (read-only, never writable). The index includes it, but default agent search, `search_kb` and the operator's search exclude it unless asked (`about: true`, or the operator's `/about` filter), so it does not drown the user's own corpus. The orchestrator carries a duty to consult it when asked about Delphi itself.
3. `finalize::spec_link` and spec-pointer parsing accept only `specs/<id>.md` entries whose current hash is in `ratified.lock`; an `about/` path or byte-identical copy never satisfies `Implements-Spec`, and its frontmatter `status` is not read as approval.

## 8. Search page (specs 043, 057, 061 amended)

1. A search starts the page job and shows the raw results immediately with a "building a page" state (`stillMotion`); the page becomes the primary view when it is ready, unless the operator chose the raw list (`f`). A late page for an older query is dropped. This supersedes the "zero tokens until asked" promise of 043 §1 and the "raw first, page on request" order of 057 §7: a bounded generated-page job may start on submit (`[library.pages] per_day` and the budget still gate a new page, never a cached one), with a visible state and a cancel.
2. Generated pages are views, not evidence: they are never indexed into the ranking corpus and never cited as support. An agent-facing summary of a page is a labelled convenience result that carries its source and claim ids.

## 9. Acceptance

- AC-1 Agent `kb_search` is hybrid when vectors and the model exist, reports `lexical` otherwise, returns passages with line ranges, and never opens the index read-write.
- AC-2 A search never writes the index; a committed change is visible to the next search by its generation.
- AC-3 `search_kb` returns only the §5.2 allowlist; unique tokens in another role's memory, `context/facts`, staged notes, another project, and `about/` are never returned.
- AC-4 The same logical path in the home layer and two projects returns only the active project's.
- AC-5 The folder of each MCP request selects the scope without restarting the server; an ambiguous or symlinked folder is home-only.
- AC-6 With no daemon, `search_kb` errors and opens nothing.
- AC-7 Markdown is Green except the §6.1 exceptions; scratch refuses every §6.2 case; a Green write read back lowers the ceiling.
- AC-8 `about/` never satisfies `spec_link`.
- AC-9 Span-start retrieval runs only for root spans under the §4.2 conditions and within budget; `retrieval.done` is recorded.
- AC-10 A search shows the page as primary when ready, `f` toggles, a late page for an old query is dropped (`ui/test/pages.test.ts`).

## 10. Implementation slices

| Slice | Content |
| :-- | :-- |
| R1 | `retrieval.rs`: reader pool, writer generation, passage results, deadline, `retrieval.done`; `kb_search`, `kb.search` and `kbserve` use it |
| R2 | `search_kb` over `HostLink`, the visibility allowlist, per-request scope, `search_memory` narrowing |
| R3 | Span-start retrieval (§4.2), config key `[retrieval]`, schema and `docs/reference/config.md` |
| R4 | Markdown/scratch tiering: path guard zone, classification, tier stamping, spec-gate predicate, GOVERNANCE and `AGENTS.md` wording |
| R5 | `about/` snapshot: seed, zone, index filter, orchestrator duty |
| R6 | Search page (043/057/061): SK9d fix (built), the building state and cancel |
