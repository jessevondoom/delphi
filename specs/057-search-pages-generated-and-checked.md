---
id: spec-057-search-pages-generated-and-checked
title: "057: Search pages: written by the writer, checked by the validator, labelled generated"
author: architect
timestamp: 2026-10-29T19:00:00Z
status: approved   # ratified by the operator, 2026-10-29, after building the design together: "let's clearly label each document as generated, not authoritative. include a bibliography/citations table in all docs ... not every sentence needs a citation ... nothing can contradict known citable truths ... pass it to the validator"
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # a design; the contradiction check is only as good as the validator and the claims it can see
source_mission: 2026-10-23-foundational-design
tags: [spec, knowledge, pages, search, writer, validator, citations, delight]
---

# Spec 057: Search pages

> Approved and ratified 2026-10-29. It **amends [`specs/055`](055-the-central-library-sources-claims-and-pages.md) §6** (the rule "every sentence carries claim ids and may use only its claims' words" is replaced by the rule in §3 below) and builds on 040 (the validator), 052 (status labels) and the SK5 code (`library_pages.rs`). It adds no authority: a page is a generated view, never a source, a claim or trusted text.

## 1. Why
A search should end in something a person wants to read: a well laid out page that gathers what the library and the web say, with text, tables and images. The operator's direction: the `writer` role writes as it wants; **a page is always labelled generated and not authoritative**; every page has a bibliography table; the validator checks that citations are real and that **nothing contradicts a known, citable truth**. Not every sentence needs its own citation, since a rule can take several sentences to state.

## 2. The flow
1. **Search.** Internal sources and claims, and (when the operator asks or the mode allows) external search. External results enter as *noted, not validated* sources through the normal capture path (055 §3), never straight into a page.
2. **Gather.** The daemon assembles a **bundle of material**: claims with their status, source excerpts, captured tables and images (by id). `private` sources are excluded when the page includes external search (055 §10.1); with internal-only pages they may be used.
3. **Write.** The writer receives the material as tainted, read-only input and returns markdown. It may organise, explain, summarise and compare freely. It may reference tables and images only by library id.
4. **Check** (§3). The daemon's mechanical checks run first; the validator reads the page next.
5. **Cache and show** (§5). A page that passes is shown at once; a page that fails is never shown as a page (§3.4).

## 3. The check

### 3.1 Required shape (mechanical, no model)
Every page has, in this order:
1. A **generated banner** the writer cannot change or remove: *"Generated page. Not authoritative. Written by the writer role on <date> from the sources listed below; checked by the validator on <date> (<result>). Read the cited sources before relying on it."*
2. The body, with citations as `[s-…]` (source) or `[c-…]` (claim) markers, placed after the sentence or paragraph they support. A marker supports the sentences back to the previous marker, so a multi-sentence rule needs one.
3. A **bibliography and citations table** on every page: one row per source or claim cited, with id, status label (052), title, author or origin, date, locator and a style-formatted citation (055 §6). A page with no citation at all gets the table with one line, "no sources cited", and is marked *unsupported* in the banner.

The mechanical check rejects: a marker that names a missing, retired or *disproved* record, or a `private` record on an external page; a quoted passage (in a quote block or quotation marks, attributed to a source) that does not appear verbatim in the cited source's stored text (hash-checked); a table or image that is not a library id; a link not present in a cited source's metadata; any edit of the banner. Numbers, dates and proper names in the body that appear in none of the material are **flagged** (`unverified figure`), not rejected.

### 3.2 The validator pass
The page and the material go to the validator (040 V2b judge, tainted read-only span, no tools). It returns, for each **assertion** it finds (a statement of fact, not wording):
- **supported:** a cited record states it, or it follows directly;
- **uncited but consistent:** nothing in the library contradicts it and no citation backs it;
- **contradicts:** a *validated* claim, a *validated* source, or a source cited on the page says otherwise;
- **fabricated citation:** the cited record does not say it.

### 3.3 Outcomes
| Finding | Result |
| :-- | :-- |
| every assertion supported or uncited-but-consistent | pass; uncited-but-consistent assertions are shown in the margin as *unchecked* |
| a **contradiction** with a validated truth, or a **fabricated citation** | **fail**; never shown as a page |
| a contradiction with a *noted* (unvalidated) record only | pass with the pair shown side by side under **Contested**; the operator decides |

The principle: **nothing on a page may contradict known citable truths**, and a fabricated citation is a failure. Being merely uncited is allowed and visible.

### 3.4 On failure
One automatic rewrite with the findings given to the writer. If it fails again, the daemon builds the **claims-only page** (the SK5 fallback, one line per claim, unchanged) and says why in the banner. The failed draft is kept in the log, not in the library. The validator never edits the page; it only reports.

### 3.5 Page status and promotion
A page has status **generated**. It is never *validated*, whatever its checks say; 052's status ladder applies to claims and sources, not to pages. Nothing on a page becomes a claim by being on a page. An operator can turn a statement into a claim from the page (`delphi claims add` with the page as `origin`); that claim starts *noted* and goes through validation like any other.

## 4. Content
- **Text and tables.** Tables are markdown tables built from cited data; each has a caption with its citation.
- **Images.** Only captured assets from the library, referenced by id with a caption and a source. The writer never produces an image file; a chart is a table plus an optional daemon-rendered chart of that table.
- **Length.** A short summary first, then sections. The cap is config (`[library.pages] max_tokens`, default 3000).
- **Quoting.** A quote is at most 1 KiB (055 D-2).

## 5. Cache and freshness
- Key: `sha256(normalised query ‖ sorted ids and hashes of every source and claim in the material ‖ writer role and model ‖ page template version)`. Pages live in `<library>/pages/<key>.md` with a sidecar `.json` (material list, check results, validator model and verdict, timestamps).
- A page is **stale** when any cited or searched-in material changes status or hash or is superseded, or when a new internal source matches the query above the relevance floor. A stale page shows a one-line notice and is rebuilt on request; the previous version stays as history.
- A page that used external search is also stale after `[library.pages] external_ttl` (default 7 days).
- The cache is bounded by the library budget (055 D-5), with least recently opened pages evicted first; pinned pages are exempt. Pages are never exported or shared, since they are views; their claims and sources are.

## 6. Cost and limits
- A page is a writer call plus a validator call. Config: `[library.pages] enabled`, `external = "ask" | "off" | "on"` (default `ask`), `max_tokens`, and a per-day page budget that counts against 040 §20's validator share.
- The writer uses the model assigned to the `writer` role; the validator uses its own span. The daemon enforces the caps, not the agents.
- A cache hit costs nothing. A page that fails twice is cached as failed for one hour so a search does not loop.

## 7. Console and CLI
- `delphi pages search "<query>" [--internal|--external]` prints the page, or the path of the cached file.
- In the console, a search shows results at once; the page opens beside them as soon as it passes, with the banner on top, the status glyphs, and the bibliography table at the bottom.
- Keys on a page: save (pin), dispute a statement (the SK5 action, on the paragraph's cited claims), promote a statement to a claim, open a cited source.
- Saved pages appear in the library under Pages, labelled generated.

## 8. Safety and authority
- The writer and validator run in tainted, read-only spans with no tools (040 §24 item 1). Source text is data, not instructions.
- A page cannot change a status, a licence or a capture level, or export anything.
- No page content enters an untainted span as prose; an agent that opens a page reads it in a tainted span like any source.
- `private` is enforced as in 055 §10.1: excluded from external pages and from outbound search queries.
- The validator's verdict is recorded and auditable (the 040 V2c blind audit applies to a sample of page verdicts).

## 9. Slices
| Slice | Content | Depends on |
| :-- | :-- | :-- |
| SK8a | the generated banner, the bibliography table, the §3.1 mechanical check; replace the SK5 "every sentence" and content-word rule with marker-per-paragraph; update tests | SK5 |
| SK8b | the validator pass (§3.2, §3.3), one rewrite, the claims-only fallback, the verdict record | SK8a; 040 V2b |
| SK8c | gather and write: search, material bundle, writer span, the cache and staleness (§5), `delphi pages search`, config keys | SK8b |
| SK8d | console views, keys, Pages in the library, digest line for changed pages | SK8c; 053 VL4 |

## 10. Verification
- **Banner:** a page whose writer output omits or alters the banner is rejected; the table is present on every page, including one with no citations.
- **Markers:** a multi-sentence paragraph with one marker passes; a marker naming a retired or missing record fails.
- **Quotes:** a quote not in the stored source fails; one that is passes (hash-checked).
- **Contradiction:** a page that says the opposite of a validated claim fails and is not shown; the same against a noted claim passes under **Contested**.
- **Fabricated citation:** a marker whose record does not say it fails.
- **Fallback:** two failures give the claims-only page and say so.
- **Cache:** a second search is a hit with no model call; changing a cited source's hash marks the page stale; an external page expires at the TTL.
- **Privacy:** a `private` source never appears on, or in the query of, an external page.
- **Authority:** nothing on a page raises a status; the promote action creates a *noted* claim.

## 11. Decisions (operator, 2026-10-29)
- **D-1:** every page is labelled generated and not authoritative, with a bibliography and citations table.
- **D-2:** not every sentence needs a citation; the rule for pages is that nothing may contradict known citable truths and no citation may be fabricated. The 055 §6 per-sentence and content-word rule is replaced.
- **D-3:** the writer writes freely; the validator checks it. Uncited but consistent statements are allowed and shown as unchecked.
- **D-4:** pages are cached until their sources change locally.
