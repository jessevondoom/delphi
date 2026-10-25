---
id: spec-013-librarian-and-internal-retrieval
title: "013: The Librarian Role & Internal Retrieval (scoped, not yet built)"
author: architect
timestamp: 2026-10-24T18:00:00Z
status: approved   # by the operator, 2026-10-24 ("ratify all"); whole document. §7 step 3 (Amendment 004-A1) approved by the operator, 2026-10-25
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the trigger thresholds in §6 are guesses to be replaced by measurement
source_mission: 2026-10-23-foundational-design
tags: [spec, librarian, retrieval, taxonomy, frontmatter, scoping]
---

# Spec 013: The Librarian Role & Internal Retrieval

## 1. Scope and Non-Goals
This spec reserves a tenth role, `librarian`, and fixes its charter, its boundary with `scout`, and the conditions under which it should be built. It deliberately builds nothing.

- **In scope:** the role's charter (§2), the scout/librarian boundary (§3), the Muse subagent pattern (§4), the frontmatter work that is worth doing before the role exists (§5), the build trigger (§6), and the retrieval index it takes over (§7).
- **Not in scope:** an `AGENT.md` manifest, any Rust, any change to the nine-role schema in `specs/001` §6.2, or a theme figure for the role (§8).

### 1.1 Simpler Alternative Considered
Keep hand-maintained indexes (`context/index.md`, `skills/CATALOG.md`) and let `delphi lint` police frontmatter. This is the current state and is correct while the corpus is small. The librarian is justified only when it stops being enough (§6).

## 2. Charter
The `librarian` is the primary source for **internal** knowledge: what Delphi already knows, where it lives, and how it is labelled. It owns:

1. **Topography:** the map of `context/`, `memory/`, `skills/`, `specs/` and `missions/`, kept as pointer indexes (path, heading path, one-line summary), never as copies of content.
2. **Taxonomy:** the controlled vocabulary behind `tags` and any domain field (§5), plus merging near-duplicate tags.
3. **Frontmatter hygiene:** proposing fixes for missing or inconsistent frontmatter. Fixes go through the normal write scope; the librarian does not bypass `delphi lint` or provenance rules.
4. **Retrieval:** answering "what do we already have on X" with pointers and, on request, short excerpts. Answers obey the pointer-based handoff rule (`specs/001` §4): no raw dumps.
5. **Search optimization:** tuning the keyword, vector and reranking parts of the index (`specs/004` §3) against the fixed question sets in `dev/kb-eval.json` and `dev/kb-eval-holdout.json`.

It does **not** judge truth, write new heuristics, or promote anything. Promotion stays with the existing taint rules (`specs/001` §5.2): a retrieval result carries the provenance tier of its source, and the librarian never raises it.

Proposed manifest shape (for the future `agents/librarian/AGENT.md`): `read: ["*"]`; `write` limited to index files under `context/` and `memory/agents/librarian/`; no network, no shell. Whether it may spawn Muses is a manifest decision under `specs/001` §2.

## 3. Boundary with the Scout
| | `scout` | `librarian` |
| :-- | :-- | :-- |
| Direction | **External**: web, literature, dependencies, unfamiliar repos | **Internal**: what Delphi has already collected or decided |
| Output | New research notes (`external_untrusted` until verified) | Pointers to existing, already-tiered material |
| Taint exposure | High (reads L0) | Low (reads what is already stored; inherits source tier) |

**Required follow-up when the librarian is built:** the scout manifest's role line today says "Repository Cartographer", and `repo-cartography` is one of its skills. Repository mapping of Delphi's own knowledge tree moves to the librarian; the scout keeps mapping *unfamiliar* or external repositories. This is an amendment to `agents/scout/AGENT.md` and needs ratification. Until then the scout keeps both.

## 4. Muses (subagents)
The librarian may fan out per-domain readers ("Muses"), each holding surface familiarity with one vertical (for example: governance, adapters/providers, TUI, federation). Rules:

- A Muse is a span of the librarian, not a new role. It uses the librarian's manifest, narrowed, and appears in logs under role `librarian`.
- Spawning is justified only for queries that span several domains or exceed one span's context budget. A single-domain lookup is one span by the librarian. A cold-start subagent costs more than an index lookup.
- Muse results come back as pointer lists, merged by the librarian. They are subject to the same 2,000-token envelope cap as any handoff.

## 5. Do Now: Frontmatter and Taxonomy (no new role needed)
`specs/001` §5 already defines the frontmatter contract and `delphi lint` enforces it (305 files, 0 failures at the time of writing). Do **not** add a parallel schema. The useful remaining work is small:

1. **Audit tag use** across `context/`, `specs/`, `memory/`, `missions/`: list distinct tags and counts, and write the controlled vocabulary to `context/taxonomy.md`. This is the one artifact a future librarian needs on day one.
2. **Candidate schema amendment (not applied by this spec):** two optional fields for `specs/001` §5, `summary` (one line, ≤ 160 characters, the text a pointer index shows) and `domain` (one of the controlled vocabulary). Both optional so no existing file breaks. Applying this needs a 001 amendment under the normal ratification path.
3. **Keep `context/index.md` current** as the interim topographic index. When it is generated rather than hand-written, that is the first sign the librarian is due (§6).
4. **Coordinate with `specs/012`** (Xenia record schema, in the `xenia-specs` branch) before choosing any new field names, so org-memory records and local frontmatter do not diverge.

## 6. Build Trigger
Build the librarian (manifest, skill set, tests) when **any** of these holds, measured rather than felt:

- `context/` + `memory/` + `skills/` exceed roughly 150 Markdown files, or `context/index.md` is regenerated by a script instead of edited.
- In two consecutive missions, a span's output duplicates or contradicts material that already existed in the corpus (visible in council or review records).
- Spans routinely spend more than a fixed budget of turns locating files (to be defined once event logs make it measurable; `specs/008`).
- The Xenia shared-memory work lands and the corpus is no longer one person's.

The thresholds are initial guesses (`confidence: low`) and should be revised when real numbers exist.

## 7. The Retrieval Index: What Exists, What the Librarian Takes Over
The index is built and in use (`specs/004` §3, Amendments 004-A1 to 004-A3): `.delphi/state.db` with heading-path chunking, FTS5, local embeddings in `sqlite-vec`, a cross-encoder that reranks the best results, preloaded vectors for the shipped documents, and a refresh on every search. The librarian is the **owner and primary consumer** of that index, not its inventor. What was a plan here is done:

1. ~~Lexical (FTS5) over frontmatter plus headings~~: built for body text; agents and skills also get a chunk made from their frontmatter (`specs/004` §3.6).
2. ~~Measure recall on a fixed question set~~: `delphi kb eval` over `dev/kb-eval.json`, and a held-out set (`dev/kb-eval-holdout.json`) that is never tuned on.
3. ~~Embeddings only if lexical recall is inadequate~~: it was, on questions worded differently from the documents (recall in the top 10 went from 0.60 to 0.88; `specs/004` §3.5). Retrieved chunks keep their source provenance tier.
4. Index is derived state: rebuildable from the Markdown, never a source of truth, never synced as authoritative (`specs/007`).

What remains for the role once it is built: `summary`, `domain` and `tags` as filterable columns for every document (only agents and skills have a frontmatter summary today), growing the question sets from the misses the operator confirms (`delphi kb miss`), and re-running `delphi kb eval` after any change to the index.

## 8. Naming (decided: operator, 2026-10-24)
Role ID is `librarian`; theme names never appear in persisted files (`specs/001` §6.1), so nothing below touches a manifest, path or log.

- **Greek theme display name: "the Muses"**, a collective rather than a single figure. This displaces no existing figure and makes the §4 subagents literal. The single-figure candidates (the writer's figure, and Mnemosyne, rejected as hard to remember) are not used.
- **Badge:** wherever the librarian or one of its Muse spans is rendered, it shows a badge in one of nine colors, one per Muse, chosen at random per span. Color is display-only: it carries no meaning, is never persisted, and the badge always includes the text label so it is legible without color.
- A theme pack must name exactly the canonical roles (`specs/001` §6.2), so adding the tenth role needs a `theme_schema` bump and a collective-name/badge field. That is part of the build, not of this spec.

## 9. Verification Plan (applies when built)
1. `delphi lint` accepts the new manifest and rejects a librarian manifest with network or shell.
2. A retrieval answer over an `external_untrusted` source carries tier L0, and a span that reads it cannot promote.
3. Scout-boundary test: an internal query is answered by the librarian without invoking `web-search`.
4. The index rebuilds byte-identically from the Markdown, and deleting it loses no information.
5. Recall on the fixed question sets (`delphi kb eval`) is recorded before and after any change the librarian makes to the index, and the held-out set is not tuned on.
