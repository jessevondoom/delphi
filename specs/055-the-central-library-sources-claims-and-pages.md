---
id: spec-055-the-central-library-sources-claims-and-pages
title: "055: The central library: sources kept whole or by excerpt, claims that point into them, and pages generated from claims"
author: architect
timestamp: 2026-10-29T04:00:00Z
status: approved   # ratified by the operator, 2026-10-29: "all your changes are accepted and approved. write the specs and mark them ratified since we built them together"; the answers in §13 stand
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a design; licence handling is engineering policy, not legal advice
source_mission: 2026-10-23-foundational-design
tags: [spec, knowledge, sources, claims, citations, library, xenia, bundles, delight]
---

# Spec 055: The central library

> Approved and ratified 2026-10-29. It builds on [`specs/040`](040-validator-role-and-fact-ratification.md) (validation, the V2a fetcher and its snapshots), [`specs/050`](050-memory-lifecycle-types-supersession-and-trace-mining.md) (entry kinds, dates, supersession) and [`specs/052`](052-knowledge-status-capture-on-contact-and-the-digest.md) (status, capture on contact, the digest). It adds no authority: sources are untrusted material, and only the validator or the operator moves a claim to *validated*.

## 1. Why
[`docs/thesis.md`](../docs/thesis.md): shared context is the team's advantage. Today knowledge arrives as small notes and as fetched snapshots that exist only to be validated. Two things are missing: **the original material a claim came from**, kept where a person can read it, and **one central place** every mission draws from by pointer instead of copying. A library that holds whole documents, the claims extracted from them and readable pages over both is something a team can browse and trust, and something that gets better every time anyone uses it. Text is cheap; trust and legibility are not.

The operator's decisions (2026-10-29) shape this spec: sources live in the knowledge base; keep **excerpts with citations** as the middle ground (several per source where needed); full capture for trusted, schematic, unique or out-of-copyright material; re-check a source when it is opened and more than 30 days old; capture enough metadata for real bibliographies; and acquire public-domain literature and research freely where it is relevant or interesting.

### 1.1 Non-Goals
- Legal advice. The licence classes below are engineering policy; an operator or organisation with stricter rules sets them in config and the stricter rule wins.
- A second validator. Validation stays in spec 040.
- Redistribution. Sources do not leave the install unless the licence says they may (§7).
- Rewriting sources. A stored source is never edited; changes arrive as new versions.

### 1.2 Simpler Alternative Considered
Keep only claims, with a URL each. Rejected: URLs rot and change, a claim without its context loses caveats, and a person cannot browse a list of assertions the way they browse a library.

## 2. Three layers
| Layer | What it is | Who writes it | Trust |
| :-- | :-- | :-- | :-- |
| **Source** | the original, kept whole, as excerpts, or as a pointer; with metadata | the daemon, from a fetch or an operator add | untrusted material; labelled *not validated* |
| **Claim** | a small statement with status, kind, dates, scope, and one or more **anchors** into sources | capture on contact (052), the validator (040), the operator | by status (052 §3.1); only the validator or operator reaches *validated* |
| **Page** | a generated, human-readable view over claims, each sentence linking to claim ids | the daemon renders it; a model may draft the wording but a check enforces the links (§6) | never a source of truth; it shows the status of what it cites |

The library is **one store per install**, `$DELPHI_HOME/library/` (`sources/`, `claims/`, `pages/`), content-addressed so the same page captured twice is stored once. Each workspace sees it through the knowledge base: indexed, searchable and ranked like other material, with every claim's **scope** (personal, project, team, org) deciding which workspaces and missions can see it. "Central" is literal: nothing is copied per mission or per workspace.

## 3. Sources

### 3.1 Capture levels
| Level | What is stored | When |
| :-- | :-- | :-- |
| **full** | the whole text as Markdown, its images and assets, the original bytes' hash | trusted sources whose licence allows it; **schematics and detailed instructions**; **significantly unique details** from a verified source; works **out of copyright**; the operator's own and an organisation's own specs and documentation |
| **excerpts** | one or more quoted passages, each with a locator, plus the full citation | everything else worth keeping: restricted licences, a great author's insight, any source cited more than once |
| **pointer** | the citation, the hash and retrieval date, no text | unclear licence, very large, or cited once |

**Full capture is not bounded by the excerpt rules.** The 4 KiB and per-source excerpt caps below exist for restricted material and quotation only. A full capture is limited by its own, much larger budget (§3.6), because a book, a specification or a long document has to be kept whole to be useful for browsing, validation and retrieval (RAG).

**Multiple excerpts** per source are allowed, including from a restricted work, where the quotation is in analysis (the operator's rule). Each excerpt records its locator (page, section, paragraph or character range), the exact text, the **purpose** it was kept for, and a length bound (§3.3): **at most 4 KiB per excerpt**, and in total **at most 10% of the work or 16 KiB, whichever is smaller**, per restricted source. On a generated page, a quotation is at most 1 KiB. Excerpts are quotation, not a substitute copy of the work: the bound is checked in code, and the Markdown for a restricted work is never reassembled from its excerpts.

### 3.2 Layout and record
```
knowledge/sources/<id>/
  source.md            full capture only; generated Markdown, never edited
  meta.json            the record (§3.2.1)
  excerpts.json        zero or more excerpts
  assets/              images and attachments (full capture only)
  versions/<n>/        earlier captures, same shape, with a change note
```
`<id>` is a short stable id derived from the canonical URL or identifier, not from content, so a source keeps its id across versions.

#### 3.2.1 Metadata (built for bibliographies)
`meta.json` is **CSL-JSON** (the citation standard that reference managers and style processors read) with Delphi fields beside it, so any bibliography style can be produced without a conversion step:

- Citation: `type`, `title`, `author[]` (with `family`/`given` or `literal`), `editor[]`, `translator[]`, `container-title`, `publisher`, `publisher-place`, `issued`, `volume`, `issue`, `page`, `edition`, `URL`, `DOI`, `ISBN`, `ISSN`, `language`.
- Delphi: `licence` (an SPDX id, or `public-domain:<jurisdiction>:<basis>`, or `all-rights-reserved`, or `unknown`), `rights_holder`, `attribution_text`, `share_alike` (bool, carried from the licence), `capture_level`, `retrieved`, `last_checked`, `canonical_url`, `content_hash`, `version`, `fetch_mode` (daemon or operator), `origin` (registrable domain, 040 §6.2), `source_class` (040 Appendix A), `tags`, `cited_by` (claim ids, maintained by the daemon).
- A `licence` of `unknown` caps the capture level at **pointer** unless the operator raises it for that source.

### 3.3 Licence classes (engineering policy, configurable)
| Class | Examples | Capture | May travel in a bundle |
| :-- | :-- | :-- | :-- |
| open, attribution | CC BY, CC0, public-domain works | full | **yes, full text and images, for retrieval (§7)**, with `attribution_text` |
| open, share-alike | CC BY-SA (Wikipedia) | full; pages that quote it substantially carry the same licence note | yes, full text, with the share-alike note |
| public domain, recorded basis | works out of copyright (for example from Project Gutenberg, public archives) | full | yes, full text |
| own or organisation material | the operator's specs, an org's documentation | full | yes, per the org's rule |
| restricted, quotation | all-rights-reserved, unknown | excerpts, with a per-excerpt length bound | **no** |
| private archival (operator-flagged) | schematics, instructions, unique details where the licence does not permit copying | full, for the operator's own use, flagged `private` | **no**; never in bundles, exports or Xenia, and never used externally (§10.1) |

Public-domain status depends on the jurisdiction and the date. The record stores the jurisdiction and the basis (for example "published before 1930, United States") rather than a bare "public domain", so the claim can be checked later and rules can change.

### 3.4 How a source is captured
- Through the **V2a fetcher** only (040 §V2a): https, SSRF limits, size cap, content-type allowlist, hash-pinned snapshot, cloaking check. The Markdown conversion reuses the existing document readers (`crates/delphi-docs`). No model is needed to capture.
- `delphi sources add <url | file | identifier> [--level full|excerpts|pointer] [--licence …]`; agents request a capture with a pointer-only tool and the daemon decides the level by §3.1 and §3.3 and the operator's config. An agent cannot raise a level or set a licence.
- Metadata is read from the page and from structured sources (Wikipedia's licence and revision, a DOI's record, a Gutenberg ebook header) by deterministic code; a field it cannot establish is left empty, never guessed by a model. A model may *propose* a missing field; the proposal is shown as such and the operator or a trusted-source record confirms it.

### 3.5 Staying current (operator decision, 2026-10-29)
- **On open:** if `last_checked` is more than **30 days** old, the daemon makes one conditional request (ETag or `If-Modified-Since`, else a hash comparison of the fresh fetch) through the fetcher. No tokens are spent on the check; it is skipped offline and says so.
- **If unchanged:** update `last_checked` and nothing else.
- **If changed:** store the new capture as version *n+1*, keep the old one under `versions/`, record a short change note (changed size, changed sections by heading, no model needed) and re-anchor claims (§4.3). Nothing is overwritten.
- **If gone or moved:** keep the last capture, mark the source *retired* in the library, and keep every claim that cites it; the claims move to *contested* if no other source supports them.
- **Rate limits:** a daily cap on rechecks and a per-host politeness delay, in config. A source that has never been opened is not rechecked.
- **Originator updates win for sources of the operator's or an org's own material:** a spec or documentation the originator maintains is re-captured when it changes, and the library shows the version a claim was made against.

### 3.6 Budgets for full capture
Full capture has its own limits, separate from §3.3: **64 MiB of text and 512 MiB in total per source** by default, and a library budget of **20 GB per install** by default (all configurable). Going over a limit refuses that one capture with the reason, falls back to *excerpts* or *pointer*, and says so; the operator can raise a limit for one source. Going over the library budget stops **acquisition** first and never evicts the operator's own material. Large texts are chunked by heading for the index and read by range; the daemon's per-read cap still applies to every read.

### 3.7 Takedown
`delphi sources purge <id>` deletes a capture (text, assets, versions, excerpts) and keeps the citation and the content hash. Claims that anchored into it become *pointer-only* and read *not validated* unless another source supports them. It is the answer to a rights holder's request and to a mistaken capture, and it is logged.

### 3.8 Source reliability
The library counts, per source and per origin, how many of its claims were later *contested* or *disproved* and how many stood. The counts are shown on the source and feed the source classes of 040 §6 as evidence for the operator, never as an automatic change to a class.

## 4. Claims

### 4.1 A claim record
A claim is a short statement plus what lets it be trusted or doubted: `id`, `text`, `kind` (fact, episode, procedure; 050 ML-2), `status` (052 §3.1, derived), `scope` (personal, project, team, org; present from the first slice), `created`, `last_confirmed`, `valid_until` (default taken from the kind of source: news short, reference works long; the operator can override), `supersedes`, `contested_by`, and **`anchors[]`**.

An **anchor** points into a source: `source_id`, `version`, `locator`, and the exact **quote** (a short span, within the licence bound) or, for full captures, a character range plus a quote hash. Every anchor also stores a short **prefix and suffix** of the surrounding text (the text-quote pattern of the W3C web annotation model), so it can be found again by fuzzy match when the source changes. A claim with no anchor is allowed (an operator observation, a mission finding) and reads *not validated*; a **validated** claim needs at least the anchors 040 §7 requires.

### 4.2 Attribution stays attribution
A claim from a source that states an opinion is stored as `kind: attributed` (040 §24 item 3): immutable, never corroboration, never turned into a plain fact without a new validation record.

### 4.3 Anchors survive source updates
When a source gets a new version, each anchor is re-resolved by its quote against the new text. Found (exactly, or by the stored prefix and suffix): the anchor moves to the new version. Not found, or the text around it changed materially: the claim becomes **contested**, joins the review queue (040 §23.3) and the old anchor remains readable in `versions/`. Nothing is silently re-pointed to different text.

## 5. One library for every mission
- **Missions write claims and source captures to the library, not to their own folders.** A mission's log keeps the narrative and the **pointers** (claim ids, source ids) it used and produced.
- **Mission start pulls by pointer.** Context assembly selects relevant claims by search and by the topic and project of the mission, ranked by status, recency and how often they were relied on, and hands the agent **pointers** (id, status label, origin, score; 040 §24 item 1), not text it has not been cleared to read. Anything an agent opens is read in a tainted, read-only span.
- **The library records who relied on what** (`cited_by`, with mission ids). That gives the validator a priority queue (040 §18.3) and lets a retraction or a *disproved* verdict find every mission that depended on a claim and flag it, without rewriting the mission.
- **Scope, not silos.** A claim's scope (§4.1) and the existing zone and taint rules decide who sees it; the default scope is the workspace's team. Sharing between installs follows §7.

## 6. Pages: the readable view
- A page is generated from claims for a **topic**. Each sentence carries the ids of the claims behind it and shows their status glyph (052). Unvalidated claims appear only under a visible *noted, not validated* heading.
- **A page may state only what its claims state.** A daemon check maps every sentence to claim ids and rejects a page with uncited prose. A model may draft the wording; it cannot add content.
- **Pages are views, not sources.** They are rebuilt when a cited claim changes status or is superseded, and cannot be edited. To change a page, change the claim (or dispute it).
- **Dispute from the page.** On any sentence, "this is wrong" files a *contested* mark on the cited claim with the operator's note. This is the fastest audit surface Delphi will have: reviewing becomes reading.
- **Bibliography from the metadata.** Every page and claim can list its sources in a chosen style (CSL styles; at least APA, MLA, Chicago and a plain list in the first slice), generated from `meta.json`.
- Pages are built on demand and cached by claim-set hash, so a large library does not pay for pages nobody reads.

## 7. Sharing, bundles and Xenia (front-loaded changes)
Two channels, two jobs:
- **Xenia carries facts, not sources.** A signed `fact` record holds the claim, its status evidence, its scope and **anchors by reference**: canonical URL or identifier, content hash, locator, a quote of at most 1 KiB and a compact citation. It never carries a source body. The receiving install fetches the source itself (or takes it from a bundle), judges the licence itself, and its validator re-checks (040 §17.3, D-4). Phase 1 kinds are unchanged; `fact` is a provisional kind (`specs/012` §11) that needs the `specs/011` §11 re-review before it is built.
- **Bundles carry full text for retrieval.** A bundle may include a top-level `sources/` section: full-text captures and images of sources whose licence class allows redistribution (open, share-alike, public domain with a recorded basis, own or organisation material), so a team's reading list works as retrieval material on any install. Size limits for `sources/` are separate from the rest of the bundle (`specs/023` §2.1). A source whose class is restricted, unknown or private is **refused at `bundle validate` and again at `add`**, by a daemon check, not by convention.
- **On adoption** a received source enters the library as *not validated*, at the capture level the bundle shipped, with its licence record intact (attribution and share-alike carried). The receiving operator can raise or purge it.
- **Never shared, by any channel:** `private` sources, restricted bodies, and the operator's private notes on them.

## 8. Acquisition: building a library worth browsing
The operator wants the system to bring in public-domain literature and research freely where it is relevant or interesting.
- A **library acquisition** job (scout-led, daemon-executed, budgeted by tokens per 040 §20 and by bytes per config) proposes sources from public-domain archives (for example Project Gutenberg, the Internet Archive, national and university digital collections, and public research repositories) matched to the workspace's projects and the operator's interests.
- Proposals appear as one batch in the digest (052 §3.3), never a blocking prompt; the operator accepts, skips or sets a standing rule ("take anything on these topics from these archives, up to this size").
- Every capture follows §3.4 and §3.3: a recorded basis for public-domain status, the archive's own header or licence text kept with the source, full text and images stored, and the archive's terms respected (rate limits, attribution requirements).
- Acquired material is **not validated** and its claims start *noted*. Being old and in the library does not make it true: a nineteenth-century source is evidence of what was believed then.

## 9. Interaction with other specs
- **052:** the digest becomes "what the library gained today": new claims noted, sources captured, claims contested by a source update, acquisition proposals. It is an encyclopedia's recent-changes page more than a to-do list. KS3 builds the digest card against this shape; the page and source parts arrive with the slices below.
- **050:** `kind`, `created`, `last_confirmed`, `valid_until` and supersession are the claim fields; no second lifecycle.
- **040:** validation reads sources through snapshots and the fetcher; the V2b judge sees only source excerpts. This spec only changes where those excerpts and snapshots are kept (in the library, not a side store), keeping the hash-checked read-back.
- **028 (knowledge window):** the window gains a Sources view, a Claims view and Pages, using the 053 vocabulary.
- **024/022 (bundles):** bundle knowledge stays a federated source class; bundle documents can be imported as sources with their own licence record.

## 10. Safety and authority
- Sources are untrusted and tainted. Agents get pointers; the content opens only in a read-only tainted span. Titles and snippets are never prose in an untainted span (040 §24 item 1).
- Capture goes through the fetcher's SSRF and size rules. Images are stored as files and never rendered or interpreted by a model unless the operator opens them.
- No agent can raise a capture level, set a licence, change a status to *validated*, or export a source. The daemon enforces all four in code.
- Nothing here lets the library's own content raise its own trust (GOVERNANCE).
- Storage is bounded by the budgets of §3.6.

### 10.1 `private` is enforced at every outbound path
A source flagged `private` (and any restricted body) is excluded by daemon checks, not by convention, from: bundles, Xenia, exports, `delphi debug bundle` and logs, the relay, connectors and mail, web-search queries and any outbound request, and telemetry. Agents may read a private source like any file the operator has given them, **including in prompts to the operator's own configured model providers** (config `[library] private_to_models`, default `allow`; `deny` keeps private text out of every prompt and agents then see only the citation and the operator's notes).

## 11. Slices
Definitional work first, so bundles and Xenia can be built to the final shape (this PR amends `specs/012`, `022`, `023`, `024`, `027`), then the code:

| Slice | Content | Depends on |
| :-- | :-- | :-- |
| SK0 | **bundle support first:** the `sources/` section in `bundle validate` and `add` (manifest key, layout, size classes, licence gate, source metadata check, indexing by chunk), schemas and docs, a golden fixture | 023, 024, this spec |
| SK1 | the library store, source record, `delphi sources add\|list\|show\|purge`, capture via the V2a fetcher, CSL-JSON metadata, licence classes, capture levels, budgets | 040 V2a, SK0 |
| SK2 | excerpts and anchors (with prefix and suffix), bibliography export (APA, MLA, Chicago, plain), the Sources view in the knowledge window | SK1 |
| SK3 | claim records with anchors, mission-start pull by pointer, `cited_by`, reliability counts | SK2; 050 ML2; 052 KS1, KS2 |
| SK4 | the 30-day recheck on open, versions, change notes, anchor re-resolution, retirement | SK1; SK3 for anchors |
| SK5 | generated pages with the sentence-to-claim check, dispute action, status glyphs, page changes in the digest | SK3; 053 VL4 |
| SK6 | acquisition job and standing rules, public-domain bases, byte budget, reading lists | SK1, SK4; 052 KS3 |
| SK7 | licence-gated export to bundles (`delphi bundle` sources) and the Xenia `fact` kind | SK0, SK1; `specs/011` §11 re-review |

## 12. Verification
- **Capture:** a loopback fixture site (V2a pattern) serving a full page, an unknown-licence page and a page that redirects: levels, metadata and caps as §3.1 to §3.3 say; no model call.
- **Licence gate:** a restricted source cannot be raised to full by an agent; a private source never appears in a bundle export (test the export, not the flag).
- **Excerpt bound:** an excerpt over the licence length is refused; the full text of a restricted work cannot be rebuilt from its excerpts.
- **Update:** unchanged source updates `last_checked` only; a changed source creates version 2, moves a resolvable anchor and contests an unresolvable one; a gone source retires and keeps its claims.
- **Pages:** an uncited sentence is rejected; a page shows the status of every claim it cites; a disputed sentence contests the claim.
- **Pointers:** mission start gives pointers only; a poisoned source title never reaches an untainted span.
- **Bibliography:** a CSL-JSON round trip through at least one standard processor produces the same citation text for APA.

## 13. Decisions (operator, 2026-10-29)
- **D-1:** private-archival full capture of schematics and instructions is allowed when the operator flags it; it is never shared through Xenia or bundles and never used externally (§10.1). Private text may appear in prompts to the operator's own model providers by default (`[library] private_to_models`).
- **D-2:** excerpts are at most **4 KiB** each; the per-source total is 10% of the work or 16 KiB, whichever is smaller; a generated page quotes at most 1 KiB. **Full capture is not bound by these** (§3.6) and bundles may include full text for retrieval where the licence allows (§7).
- **D-3:** claims are separate records, since one claim may cite several sources. In Xenia the kind is `fact`, because `claim` already names a Phase 3 work claim.
- **D-4:** the first pages are built on request, then for the most-relied-on topics.
- **D-5:** library budget default **20 GB per install**, per-source 64 MiB text and 512 MiB total, all configurable.
- **D-6:** the library is one store per install with per-claim scope (§2).
- **D-7:** the 30-day recheck on open (§3.5), takedown (§3.7), source reliability (§3.8), robust anchors (§4.1), freshness by kind (§4.1), page changes in the digest (§6, §9), and building small first (SK0 to SK3 before pages and acquisition) are adopted.
