---
id: spec-061-encyclopedia-search-pages
title: "061: Encyclopedia search pages, inline media, and reputable expansion"
author: architect
timestamp: 2026-10-31T00:00:00Z
status: approved   # approved by the operator on 2026-10-31: "commit and approve the spec"
approved_by: operator (2026-10-31)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, knowledge, library, search, pages, markdown, images, writer, ui]
---

# Spec 061: Encyclopedia search pages, inline media, and reputable expansion

**Status.** Approved by the operator on 2026-10-31. Builds on specs [`028`](028-knowledge-window.md), [`029`](029-links-and-inline-reader.md), [`043`](043-knowledge-window-search-page-and-growing-corpus.md), [`050`](050-memory-lifecycle-types-supersession-and-trace-mining.md), [`055`](055-the-central-library-sources-claims-and-pages.md), and [`057`](057-search-pages-generated-and-checked.md).

## 1. Goal

Make the **Encyclopedia Summary Page** the primary result when searching the knowledge base. Modeled after the layout, composition, and plain-language clarity of a Wikipedia article, each page is authored in Markdown by the `writer` role using a dedicated skill (`encyclopedia-synthesis`), rendered through Delphi's full Markdown and inline image pipeline (`ui/src/markdown.ts` and `ui/src/image.ts`), cached with a content-aware expiration date (`valid_until`) in YAML frontmatter at the `writer`'s discretion, and expandable on demand (`e`) with deeper research from reputable sources.

## 2. Scope and boundaries

- **No silent authority elevation.** Generated encyclopedia pages remain `internal_generated` synthesis (`status: active`, or `unverified` on claims-only fallback). A generated page is never ingested as a source for another generated page.
- **No arbitrary remote image hotlinking.** `/v1/image` renders only local bytes already captured under library source assets (`$DELPHI_HOME/library/sources/<sid>/assets/`) or indexed workspace directories (`local/knowledge/web/<host>/images/` and workspace document paths).
- **Role IDs only.** All manifests, frontmatter, prompts, and code use the role ID `writer`, never theme display names.
- **Every animation obeys reduce motion.** Any progress indicator during initial synthesis or page expansion takes its state from `stillMotion` in `ui/src/motion.ts`.

## 3. Wikipedia layout and Markdown composition

Every encyclopedia page generated for a search query uses standard Markdown constructs supported by `renderMarkdownLinks` (`ui/src/markdown.ts`) and `paint` (`ui/src/image.ts`):

1. **Disambiguation hatnote (optional):**
   - When the gathered material shows secondary meanings for the query term, the page starts with an italicized blockquote hatnote above the title linking to `page:<slug-b>`:
     ```markdown
     > *This page is about <primary topic>. For <other meaning>, see [<Title B>](page:<slug-b>).*
     ```
   - When a query splits across multiple distinct meanings with no single dominant topic, the `writer` emits a dedicated **Disambiguation Page** (`kind: disambiguation` in frontmatter) listing each meaning with a focusable `page:<slug>` link and a one-sentence summary.
2. **Title and metadata chrome (`# <Title>`):**
   - Rendered with a gold H1 underline rule (`═`) and a compact metadata line showing epistemic status, cited claim/source counts, and cache freshness (`valid_until`).
3. **Lead section (Leittext):**
   - 1 to 3 concise, plain-English paragraphs before the first `##` heading. The first sentence bolds the title term (`**<Title>** is ...`) and defines the subject directly without meta-commentary. Short articles are welcome when the material is concise.
4. **Infobox summary table (optional):**
   - Immediately after the lead section, a compact two-column Markdown table (`| Attribute | Summary |`) presents 3 to 6 key structured facts (such as status, specification, crate/module, provenance, or dates) with inline citations.
5. **Table of Contents (`## Contents`):**
   - Included whenever the article has 2 or more `##` body sections. Uses a numbered list of internal anchor links (`1. [Section Title](#section-title)`) so pressing `Tab` and `Enter` jumps the reader viewport directly to that heading via `startRow`.
6. **Body sections and inline figures (`##` / `###`):**
   - Organized thematically in neutral, direct encyclopedic prose. Every factual statement carries at least one inline citation marker (`[c-XX]` or `[s-XX]`). Both `validated` and `contested`/`noted` claims are represented neutrally according to their epistemic status.
   - When relevant image assets exist in the gathered bundle, the `writer` places standalone inline image paragraphs referencing `lib:<sid>/<file>` or `kb:<doc-path>#<rel>` followed immediately by an italicized caption line (`*Figure N: description [s-XX].*`), rendered inline by `ui/src/image.ts`:
     ```markdown
     ![alt text](lib:<sid>/<file>)
     *Figure 1: description [s-01].*
     ```
7. **Citations and bibliography (`## References`):**
   - A bottom `## References` section lists every cited claim (`[c-XX]`) and source (`[s-XX]`) with its title, provenance tier, status glyph, and capture date.

## 4. Requirements

### 4.1 New `writer` skill: `encyclopedia-synthesis`

1. Create `skills/encyclopedia-synthesis/SKILL.md` (validated against `schemas/skill-manifest.schema.json`) and add `encyclopedia-synthesis` to `authorized_skills` in `agents/writer/AGENT.md`. Regenerate `docs/reference/catalog.md`.
2. The skill instructs the `writer` in Wikipedia Manual of Style prose (simple, direct language; inverted-pyramid lead; neutral point of view; no filler), disambiguation hatnote vs. full disambiguation page selection, TOC construction, inline figure placement and captioning, and content volatility evaluation for cache expiration.

### 4.2 Unified local gather and immediate synthesis on search

1. **Immediate synthesis on search:** When the operator submits a search query in the Archives window (`ui/src/archives.ts` or `/search <query>`), the daemon immediately checks the encyclopedia page cache and, on a cache miss, starts synthesizing the summary page from local sources without requiring an extra confirmation keystroke.
2. **Unified local gather (`Library::gather`):** Extend `Library::gather` (`crates/delphi-engine/src/library_page_search.rs`) to gather from both:
   - Central library claims and sources (`$DELPHI_HOME/library/`), and
   - Top matching workspace knowledge base passages (`kb::Index` over `specs/`, `docs/`, `context/`, and `local/knowledge/web/`), exposed as internal source excerpts (`s-kb-XX`).
3. **Gathered image assets (`GatheredAsset`):**
   - `Library::gather` collects image assets attached to matched library sources (`$DELPHI_HOME/library/sources/<sid>/assets/*`) and Markdown image references inside matched workspace KB documents (`local/knowledge/web/<host>/images/*` or doc-relative paths).
   - Available assets (`uri`, `source_id`, `alt_hint`, `width`, `height`) are passed in the gather bundle to the `writer`.

### 4.3 Deterministic check and `/v1/image` support for library assets

1. **Mechanical check (`crates/delphi-engine/src/library_page_check.rs`):**
   - Update `brief()` (`library_page_search.rs`) and `rewrite()` (`page_run.rs`) to remove the prohibition on images and internal/disambiguation links.
   - Permit internal heading anchor links (`[Label](#heading-slug)`) that resolve to a heading in the page.
   - Permit disambiguation links targeting `page:<query-slug>`.
   - Permit standalone inline image paragraphs targeting `lib:<sid>/<file>` and `kb:<doc-path>#<rel>` when the URI is present in `gathered.assets` and the underlying file exists on disk.
   - Verify that every `[c-XX]` and `[s-XX]` citation resolves to a gathered record and appears in `## References`.
2. **Image serving (`crates/delphi-engine/src/kbserve.rs`):**
   - Extend `GET /v1/image` to resolve `src=lib:<sid>/<file>` against `$DELPHI_HOME/library/sources/<sid>/assets/<file>` and `src=kb:<doc-path>#<rel>` against the indexed workspace root, enforcing canonical-path traversal guards and PNG/JPEG/GIF decoding via `delphi-docs`.

### 4.4 Frontmatter caching and `writer`-discretion expiration (`valid_until`)

1. **Volatility-based expiration:**
   - When authoring or refreshing a page, the `writer` evaluates the volatility of the subject and returns `volatility` (`high` | `medium` | `low` | `timeless`), `suggested_ttl_days`, and a one-line `ttl_reason`:
     - `high` (`1`–`7` days): active missions, live incidents, rapidly changing metrics, or predominantly unconfirmed/contested notes.
     - `medium` (`14`–`30` days): evolving software architecture, active specs, or external tool documentation.
     - `low` (`60`–`180` days): ratified foundational specifications, stable APIs, or established procedures.
     - `timeless` (`180`–`365` days): historical events, mathematical/scientific principles, or immutable archival records.
   - The daemon clamps `suggested_ttl_days` to `[1, 365]` days (configurable via `[library.pages] min_ttl_days` and `max_ttl_days`), computes `valid_until = today + clamped_days`, and writes standard YAML frontmatter (`id`, `title`, `query_key`, `kind`, `depth`, `author: writer`, `source_type: internal_generated`, `status`, `created`, `last_confirmed`, `valid_until`, `volatility`, `ttl_reason`, `disambiguation`) at the top of `$DELPHI_HOME/library/pages/<key>.md` alongside `<key>.json`.
2. **Cache invalidation rules:**
   - A cached page is served immediately without model invocation while `today <= valid_until`, the gathered local `material_hashes` match the sidecar, and neither Refresh (`r`) nor Expand (`e`) was requested.
   - Pages past `valid_until` are marked stale and refreshed on search or on `r`.

### 4.5 Reputable "Expand" workflow (`pages.expand` / `delphi pages expand`)

1. **User action:** Pressing `e` on an encyclopedia page in the console (or running `delphi pages expand "<query>"`) triggers an in-place expansion of the page.
2. **Reputable external research and image capture:**
   - Queries trusted external sources using `crates/delphi-engine/src/research.rs` and `research_images.rs` against the configured reputable source lists (`context/sources/trusted.toml` and `$DELPHI_HOME/operator/trusted-sources.toml`, restricted to `Primary` and `ReputableSecondary` tiers).
   - Captures new external sources and excerpts into `$DELPHI_HOME/library/sources/<sid>/` and downloads licensed/reputable images into `$DELPHI_HOME/library/sources/<sid>/assets/<hash>.<ext>` with recorded `AssetRef` metadata (sha256, media type, attribution, licence).
3. **Deep re-synthesis:**
   - Re-runs `Library::gather` over the expanded library and workspace corpus, invokes `writer` with `depth: expanded` under `encyclopedia-synthesis`, runs `check_body` and the `validator` pass, and saves the expanded page, inline image references, updated bibliography, and refreshed `valid_until` frontmatter (archiving the prior page version under `versions/`).

### 4.6 Console primary search result UX (`ui/src/archives.ts` and `ui/src/markdown.ts`)

1. **Primary result view:** Submitting a search in the Archives window shows the Encyclopedia Page as the primary result, rendered through `renderMarkdownLinks` (`ui/src/markdown.ts`) and `paint` (`ui/src/image.ts`) with prefetched `/v1/image` rasters.
2. **Navigation and controls:**
   - `Tab` / `Shift+Tab` and `Enter` navigate TOC `#anchor` links (scrolling to the target section heading), disambiguation `page:<slug>` links (opening the target encyclopedia page), and `[c-XX]` / `[s-XX]` citations (opening the source excerpt or claim record).
   - `e` expands the page with reputable research and deeper synthesis (`pages.expand`).
   - `r` refreshes the page from current local material.
   - `f` toggles between the Encyclopedia Page and the raw matching file/passage list.

## 5. Implementation slices

| Slice | Content |
| :-- | :-- |
| SK9a | `skills/encyclopedia-synthesis/SKILL.md`, authorize for `writer` in `agents/writer/AGENT.md`, regenerate `docs/reference/catalog.md`, extend `frontmatter.rs` and `library_page_search.rs` with encyclopedia YAML frontmatter and clamped `[1, 365]` day `valid_until` caching |
| SK9b | Unified `Library::gather` (central library + workspace `kb` excerpts + `GatheredAsset` images), update `brief()` and `library_page_check.rs` to allow TOC `#anchors`, `page:<slug>` links, and verified `lib:`/`kb:` inline images, and extend `/v1/image` in `kbserve.rs` |
| SK9c | Reputable expansion pipeline (`pages.expand` RPC and `delphi pages expand <query>` CLI) connecting `research.rs` and `research_images.rs` to library source/asset storage and `depth: expanded` synthesis |
| SK9d | Make Encyclopedia Page the primary search result in `ui/src/archives.ts` rendered via `renderMarkdownLinks` and `paint` (`ui/src/image.ts`), with immediate synthesis on search, TOC `#anchor` jumping, disambiguation navigation, and `e` / `r` / `f` controls |

## 6. Acceptance gates

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo run -q --locked -p delphi -- lint --repo .`
- `cargo test -p delphi --test docs`
- `cd ui && npm test`
