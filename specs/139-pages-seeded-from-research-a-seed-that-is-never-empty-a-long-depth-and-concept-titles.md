---
id: spec-139-pages-seeded-from-research-a-seed-that-is-never-empty-a-long-depth-and-concept-titles
title: "139: Pages seeded from research: a seed that is never empty, a long depth, and concept titles"
author: architect
timestamp: 2026-11-25T20:00:00Z
status: approved   # by the operator, 2026-11-25: "approve 139–143 as recommended, waive §1.3". The words are the operator's, as relayed by the coordinating session to the architect agent that wrote this line; the approval takes effect when the operator merges the pull request that carries it. "As recommended" settles every open decision of section 8 at its recommended value. "waive §1.3" is the operator's own waiver of the review GOVERNANCE §1.3 asks for, and it is needed: no independent review of this text exists. Panel t_0004 read it, with a red-team seat, and its findings and its recheck are applied (section 9); every seat was on one model family, the family that wrote the spec, so the Panel does not count as an independent review and cannot be cited in reviewed_by.
approved_by: operator (2026-11-25)
reviewed_by: []   # the operator waived GOVERNANCE §1.3 in the approval words (see `status`). Panel t_0004 read this text and its record is committed under missions/2026-11-25-knowledge-v2/councils/, but it ran on one model family and says of itself that it does not count as an independent review, so the lint would refuse it here and it is not cited.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors read on 2026-11-25 at origin/main 52bdd3b8 (0.6.7). The live proof (step 0) reported on 2026-11-25; its findings are in section 1 and section 7 as relayed by the coordinating session, and were not re-run for this text.
source_mission: 2026-11-25-knowledge-v2
tags: [spec, library, pages, topics, research, seed, writer, encyclopedia]
---

# Spec 139: Pages seeded from research

> **Status: approved by the operator, 2026-11-25:** "approve 139–143 as recommended, waive §1.3" (see the frontmatter for what those words settle and for the limits of the reviews behind them). One of five specs from the knowledge audit of 2026-11-25 (139 to 143). Nothing is built yet.
> **Build order (confirmed by the operator, 2026-11-25; steps 2 and 4 and the late group were added after Panel `t_0004` and confirmed with the approval of that day):** (1) the single-turn harness bridge (`specs/143`), with the seed slices of `specs/139` (A, B, D, E) alongside; (2) the retrieval baseline, slices A, B and D of `specs/140`, run before the index changes; (3) the source text cache (`specs/142`: A, E, the cache cap, B, C, G); (4) a bounded backfill of the existing pointer sources, started by the operator (142 §2.7); (5) the writer slices of 139 (C, F); (6) the rest of 140; (7) label enforcement (`specs/141`); (8) kept late, and not before half of the last ten written pages pass validation: 139 G and H, 140 H, the rest of 142's cache slice and 142 F; then the one knowledge record and the later steps, which have no spec yet. This spec's writer slices come after 142 because a validated page needs source text to be checked against (O8), and after 143 because the writer has no route today (O7).
> **Amends:** [`specs/117`](117-the-encyclopedia-in-the-desktop-app-topic-pages-navigation-and-a-view-of-agent-memory.md) §2 items 1 to 4 (how a topic is named, rebuilt and gathered), [`specs/097`](097-encyclopedia-search-pages.md) §4.5 (a third depth beside `summary` and `expanded`), [`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md) §2 item 1 (one optional input). **Builds on:** [`specs/112`](112-one-research-record-a-single-runner-topics-that-persist-and-claims-that-reach-the-library.md) §2 (the record and `answer.md`), [`specs/116`](116-memory-that-grows-a-provisional-tier-findings-consolidation-and-a-larger-start-context.md) §5 (findings), [`specs/118`](118-pictures-and-figures-stored-licence-capture-in-research-full-size-and-agent-made-figures.md) §5 (figures).

---

## 1. Scope and objectives

The operator's workspace holds nine topic pages and none has ever been built. Reading the code explains why a built page would be poor even when the build runs:

| # | Observation (origin/main 52bdd3b8) | Where |
| :-- | :-- | :-- |
| O1 | A rebuild gathers by **one search on the topic's title**, which is the research question word for word. Nothing is taken from the topic's own `answer.md` unless that search happens to return it, and then only its first 3 KiB. | `topic_gather.rs:134-141`, `:32`; `library_topics.rs:437` (`"query": title`) |
| O2 | With nothing gathered, the page is a stub that says "the library holds nothing on this topic yet". When the writer fails, the page is the claims alone. Either way the research that was done is not on the page. | `library_topics.rs:623`, `:641` |
| O3 | Every topic page is written at `Depth::Summary`: "about 150 to 400 words", under a 3,000-token cap. | `library_topics.rs:326`, `:707`; `library_page_search.rs:614`; `config.rs:1231` |
| O4 | A topic's title, slug and query are the question as asked, up to 200 characters. There is no page about a concept that two questions could share or link to. | `library_topics.rs:416-437`, `:445-457` |
| O5 | A page is built only by the idle rebuild or by the operator's command. Research that has just finished waits. | `page_run.rs:345`; `service.rs:3115` |
| O6 | A rebuild never reads the page it replaces (a generated page is not in the index, `topic_gather.rs:81`). This is right, and nothing guards it. | `topic_gather.rs:18`, `:81` |
| O7 | **Step 0:** no page could be written at all. Every live endpoint on the operator's install is a harness; the writer and the validator ask for single turns (`page_run.rs:104`), which a harness is ruled out for (`route.rs:272`), so the rebuild fails for want of a writer (`page_run.rs:322`). The failure is logged at `warn` (`service.rs:3116`) under a default level of `error` (`elog.rs:17`, `:111`), so nobody saw it. `specs/143` is the remedy. | step 0, 2026-11-25 |
| O8 | **Step 0:** with a route supplied by a test shim, pages ran end to end and every one ended as the claims alone (251 words, no citation). The sources are pointers, so the validator has no text to check; the writer quoted titles and the check refused them as not verbatim or as made-up citations. The public research records hold `claims: []`; the only claims are on private records. | step 0, 2026-11-25 |
| O9 | **Step 0:** the validator's weekly floor (`FLOOR_TOKENS`, 50,000, `fact_audit.rs:38`; charged for pages at `page_run.rs:89-92`) was spent after about eight short pages. | step 0, 2026-11-25 |

Objective: a page exists as soon as research on its topic ends, and is never empty. Code assembles a **seed** from what the research wrote. The writer expands the seed into a long page. A writer that fails leaves the seed in place. Every rebuild starts from the seed and the source spans.

### 1.1 Non-goals

- **No private record on a page yet.** A seed span from a private record is left out, as today (`library_topics.rs:16-19`, `:449`, `:580`). The operator decided on 2026-11-25 that private and internal research may go to every configured model and onto local pages; pages still wait for the sink guards of `specs/141`, whose last slice lifts the exclusion. In the proposed order that is after this spec, so until then pages are seeded from public records only (see §8, OD-6).
- **No outline-first, multi-call writing, no entity extraction, no per-source summary layer.** Those are step 7 (the encyclopedia pages of the audit).
- **No change to retrieval ranking** (`specs/140` gates that).
- **No change to search pages** (097) or to what the page check and the validator require of a writer's page.

### 1.2 Simpler alternative considered

- **Alternative:** raise `MAX_RECORDS` and `RECORD_TEXT` in `topic_gather.rs`, and change the brief's size line to ask for a long page.
- **Why rejected:** it keeps O1 and O2. The material would still come from a ranked search on a question string, so the topic's own answer can be missing, and a failed writer still yields a stub. The operator's requirement is that the research summary *is* the first draft; that needs a draft made by code, not a larger prompt.

### 1.3 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | the writer is given more records and a longer size line; a failed writer falls back to the claims-only page | a build with a failing writer still produces a page without the answer's text |
| B | constraint removed: "a page is written by a model" | code assembles the page from the records linked to the topic; the writer's page replaces it only when it passes the check | a build with no model at all produces a page that holds the answer's text and cites it |
| C | inversion: the page is primary and research appends to it | each research run edits the previous page in place | plant a sentence in the old page and see whether it survives a rebuild (it would: this is the drift the audit warns of) |

Chosen: **B**, with the writer's expansion on top. A loses the research when the writer fails. C rewrites its own output, which the audit's evidence (and O6) argue against. Flagged by the guard (a new layer of abstraction with no test of its own): none; the seed has its own tests (AC-1 to AC-4).

## 2. Design

### 2.1 The seed

1. A **seed** is an ordered list of **spans**. A span is a run of whole lines of one stored document: `{label, kind, rel_path, start_line, end_line, title, text, hash, link}`. The label is the existing record label (`topic_gather::label_of`, `topic_gather.rs:46`), so a span is cited as `[s-kb-…]` or `[f-…]` exactly as a gathered record is today.
2. The spans are chosen by **link first, search last**, in this order:

| Order | Source | Link (`link`) |
| :-- | :-- | :-- |
| 1 | the answer of each research topic the page follows: `answer.md` and the runs' own results the record keeps (`research_record.rs:219`, `:629-631`), newest first, split at `## ` headings | `research_topic` |
| 2 | the stored summary of each source the topic's record read (142 §2.6), and its kept excerpts | `research_topic` |
| 3 | findings (116 §5) written by a session or mission that also ran or recorded research on the topic; the run pointer files name the session (`research_record.rs:731`) | `session` |
| 4 | kept summary pages (`local/summaries/`, `kb_import.rs:12`, `:261`) whose bibliography names a source the topic's record read | `shared_source` |
| 5 | a project's research file that an ingest run recorded as the topic's answer by reference (113 §3 item 1) | `research_topic` |
| 6 | what today's search returns (`topic_gather::gather`), to fill what is left | `search` |
| 7 | the live claims of the followed topics, **when there are any** (`library_topics.rs:576`). A seed never depends on claims: step 0 found the public records have none (O8). | `research_topic` |

3. Caps: at most 40 spans and 64 KiB of text in a seed; a span is at most 8 KiB and is cut at a paragraph. Row 1 is never dropped to make room for the others. The order inside a row is by path, then line, so the same inputs give the same seed (byte for byte).
   **A source's own text is never a span.** A library source enters a seed only as its summary (142 §2.6) and its kept excerpts, each at most 300 characters; where a source has neither, it is named in the bibliography and gives no span. This holds for every source, held, open or private, so that a seed never depends on what a licence allows. What a note quotes is bounded by §2.2 item 3.
4. What is left out is what today's gather leaves out (`topic_gather.rs:13-18`): a private record, a hit flagged `internal`, another project's hit, staged and provisional notes, episodes, and any generated page.
5. Seed text is data from outside. It keeps the tier of the document it came from, is cleaned as record text is (`record_text`, `library_page_search.rs:600`), and passes the workspace's identifier mask (`host_research::mask`, `host_research.rs:263`).

### 2.2 The seed page

1. The seed is rendered by code into a page: `# <title>`; a lead that is the first span of the newest answer; the rest of the answer under its own headings; `## Sources read` for row 2, `## Findings` and `## Related notes` for rows 3 to 5; the open questions and the bibliography as the daemon writes them today (`library_topics.rs:679`). Each block is a span's own words followed by its marker.
2. A seed page has the outcome **`seed`**, a fourth value beside `pass`, `pass_contested` and `claims_only` (`library_page_validate.rs:231`). Its banner reads: `Seed draft: assembled by code from research notes. Not written up and not validated.` The validator is not run on it.
3. **The seed check.** A seed page is checked by code before it is stored, and what the check does on a failure is stated:
   - **Cleaning first, by one function.** A span's text is cleaned before it is rendered: a link other than a section link or a `page:` link loses its address and keeps its words; an image is replaced by its alternative text; raw HTML is removed; control characters are removed (`record_text`). The same function cleans the span for the comparison, so "the block is in the span" means: the block is found, letter for letter, in the **cleaned** span its marker names. No link, image or HTML survives to conflict with that.
   - **The held-text predicate.** The whole page is passed through the one shared predicate of 142 §2.5 item 4, `body_may_leave(source, output)`, for **every held source the seed's text could quote**, not only the sources the topic's records read: a finding or a note found by search may have copied from a source no record of the topic names. The rule is cheap and exact for long runs: the predicate's block index (142 §2.5 item 4) is asked which held sources share a 64-character block with the seed page, and the page is then checked in full against each of those and against the topic's own sources: no run of more than 300 characters of a held source's text, and no more of it in all than the aggregate cap. A note that quotes a source at length is therefore cut where it quotes: the block is shortened to the bound and marked `[shortened: quoted source text]`. Until 142 slice B exists there is no held source and the predicate passes everything.
   - **No span from a document §2.1 item 4 leaves out.**
   - **On a failure.** A block that fails is dropped and counted in the sidecar (`seed_dropped`, with the reason). If no block is left, no page is written: the topic is `waiting for material`, and the cause is a notice (`check_failed`, 143 §2.7). A seed page that has not passed the check is never stored.
4. **Never empty.** A topic with at least one span gets a seed page. A topic with no span (one the operator made by hand before any research) gets **no page**: it is listed as `waiting for material`, and `delphi pages rebuild` says so. The stub of O2 is no longer written for a topic.
5. Writing a seed page calls no model, spends no tokens and does not count against `[library.pages] per_day`.

### 2.3 The writer expands the seed; a failure keeps it

1. A new depth, **`long`** (`Depth::Long`, beside `library_page_meta.rs:71-75`). Topic pages are written at `long`; search pages keep `summary` and `expanded`.
2. The brief (`brief_for`, `library_page_search.rs:612`) gains a size line for `long`: a lead of two to four paragraphs, then `## ` sections that follow the seed's own structure, 1,200 to 2,500 words; a `## Contents` list; key terms, start here and see also as 117 §2 item 5 asks; a figure wherever cited numbers or named parts support one, by the figure block the brief already describes (118 §5 items 1 to 3); no References section, because the daemon writes the bibliography.
3. The writer is given the seed page as `DRAFT TO EXPAND` and the spans, claims, sources and assets as `RECORDS`. It may reorder, merge and rewrite; every statement still needs a marker, and the page check and the validator are unchanged.
4. The cap for a long page is a new key, `[library.pages] long_max_tokens`, default 8,000 (the scout's cap, [`specs/114`](114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis.md) §13). `max_tokens` (3,000) still governs search pages.
5. **A failure keeps the seed.** When the writer errors, when its page fails the check after the rewrites the validator allows, or when the budget or share gate refuses the call, the seed page is stored (or stays) with outcome `seed` and the reason. `claims_only_page` is no longer called from `topic_rebuild` (`library_topics.rs:623`, `:641`).
6. **A rebuild starts from the seed and the spans, never from the previous page.** Each rebuild assembles the seed again from the stored documents. The previous page and its versions are read only to compute the change line (`library_topics.rs:341`) and to keep the version. No text of a previous page enters a brief.
7. **The writer's closing comment is read from its first draft only.** A rewrite's `<!-- page: ... -->` line is removed and ignored, as the fix of step 0 does (commit `c1fca8ca`; `specs/143` §2.6). So a `title=` or a lifetime is proposed once per build.
8. **A page budget of its own, in the one ledger.** Topic pages stop drawing on the validator's share (O9). `[library.pages] week_tokens` (default 600,000) bounds what writing and validating pages may spend in seven days. It is not a meter of its own: it is one row of the admission table of 143 §2.3, counted from that spec's one persisted ledger, and the one function `job_may_run` decides whether a page build starts, reading `enabled`, `auto_rebuild`, `per_day`, `build_after_research`, `stale_writer_per_day` and `[bridge.jobs] pages` in one place. The reserve asked before a build is in proportion to the depth: a long page reserves four times what a summary page does. When the budget is spent the topic stays due, the seed stands, and `delphi kb health` says so.

### 2.4 Titles that are concepts

1. A topic has a **title** (a concept: `Web Bot Auth`), and keeps the question it came from as `query`. The sidecar gains `title_source`: `subject`, `writer`, `derived`, `operator` or `question`.
2. Where the title comes from, best first:
   - **`operator`:** `delphi pages rename <slug> "<title>"`. Always wins.
   - **`subject`:** `record_research` gains an optional input `subject` (at most 80 characters): what the research is about, as a name. The host knows it. It is data: it sets no trust, status or privacy.
   - **`writer`:** the writer's closing comment (`library_page_meta.rs:88-93`) may carry `title=`; taken only at a topic's first written page.
   - **`derived`:** by code from the question: the closed list of lead-ins (`what is`, `what are`, `how do`, `how does`, `how to`, `why`, `should`, `can`, `is`, `are`, `does`) and the closing question mark are removed.
   - **`question`:** when every rule below refuses the others.
3. A title from `subject` or `writer` is accepted by code only when it is at most 80 characters, holds no question mark and no control character, does not begin with a lead-in of the list above, and shares at least one content word with the question or a seed heading.
4. The slug is made from the title when the topic is created. A topic that has no written page (`version` 0) is re-slugged when its title changes; `delphi pages retitle [--apply]` does this once for the topics that exist (it prints first and writes only with `--apply`). A topic with a written page keeps its slug for good and changes only its title.
5. Two research topics whose titles give the same slug share one page: `topic_create` already adds the second research topic to the first (`library_topics.rs:422-431`).

### 2.5 Build now

1. **Seed now, expand at idle.** When a research run ends `done` or `partial` (`Topic::finish`, `research_record.rs:509`) and when `record_research` returns (`host_research.rs:330`), the daemon, for that research topic: makes the topic if there is none (`topics_from_research`) and writes the seed page **at once**; that costs no model call and is never paused. The writer's expansion is **queued for the next idle time**, ahead of older due topics. It is an idle job: it waits while the operator is in a session and is admitted by `job_may_run` (143 §2.3), so in a busy host session the seed is what the operator sees until the session ends.
2. The expansion runs within `per_day` and the background share as every rebuild does (`library_topics.rs:625`, `page_run.rs:89`). When either refuses, the topic stays due and the seed stands.
3. `[library.pages] build_after_research` (default `true`) turns the trigger off. `auto_rebuild = false` still leaves the writer's rebuilds to the operator; the seed is written regardless, because it costs nothing.
4. The reply to `record_research` and the research result name the page's slug and its outcome (`seed`, until the writer's page lands).

### 2.6 The writer role, used properly

1. **What is there today, and stays.** The writer's call is built with the role's own text: `role_call::load` for the step `WritePage` (`page_run.rs:36-60`, `role_call.rs:46`) loads, from the ratified store only, the writer's charter excerpt, its page skill `encyclopedia-synthesis`, and its ratified heuristics and lessons, under the call's own governance line.
2. **The long depth carries the whole skill.** For a page at `long`, the `encyclopedia-synthesis` skill is carried in full. If the role text would not fit the call, lessons and heuristics are shortened first and are listed as left out (`RoleText::dropped`, `role_call.rs:16`); a long page is never written with the skill cut or missing, and a build that cannot carry it fails with a notice (143 §2.7).
3. **Each page says what it was built with.** The sidecar gains `built_with`: the writer's `pack_hash` and the validator's (`mcp_packs.rs:286`), and the model each ran on. A seed page records none, since no role wrote it.
4. **When the writer changes, its pages are due.** A page whose `built_with.writer_pack` is not the writer's current `pack_hash` is due, with the reason `the writer role changed`. The pack is made from the ratified store, so only a change the operator ratified (to the writer's instructions, its skill, the heuristics or its lessons) can cause this; nothing an agent writes can.
5. **A few at a time.** Pages due for this reason are rebuilt oldest first, at most `[library.pages] stale_writer_per_day` a day (default 3), inside `per_day` and the page budget of §2.3 item 8, and always from the seed and the spans (§2.3 item 6), never from the page they replace. `delphi pages rebuild --stale-writer` rebuilds them all now, as the operator's own job (143 §2.8).
6. A change of the validator's pack is recorded and does not make a page due: the page's words are the writer's.

### 2.7 Learning from validation

1. **A history of outcomes, for each writer pack.** Every writer build appends one line to `local/pages/history.jsonl`: when, the topic's slug, the depth, the outcome (`pass`, `pass_contested`, `claims_only`, `seed`), the number of rewrites, **the validator's findings by kind**, the page's words and citations, the tokens, and `built_with`. The kinds are a closed list taken from the page check and the validator: `not verbatim`, `unsupported`, `fabricated citation`, `unresolved marker`, `figure failed`, `over length`, `other`. A line holds numbers and names from that list, never a sentence of a page, a claim or a source.
2. **Its rules, owned here** (slice H does not wait for 140): the file is written only by the daemon; it is append-only and never rewritten or pruned; every line carries a format version and a reader accepts every older one; a last line that does not parse is skipped and reported; `kb reindex` does not touch it; and any later change of stores carries it unchanged. Slice H also builds the entry point by which the daemon itself keeps a provisional note with no agent's event behind it; 140 slice G uses the same one.
3. **Visible.** `delphi pages history [--pack <hash>] [--json]` shows, for each writer pack in time order, the pass rate, the findings by kind, and the median words and citations. `delphi kb health` gains one line (the current pack's pass rate against the pack before it). The app shows it beside the retrieval-quality panel of 140. A view document is regenerated beside 140's (`local/knowledge/writer-quality.md`), so an agent, and whoever coaches the writer, can find it by search.
4. **Repeated findings become a note for the writer, not a lesson.** When one kind of finding appears in three or more of the last ten builds under the same writer pack, the daemon writes one provisional note for the role `writer` (116 §2), kind `pitfall`, in words made by code from the counts (`In 4 of the last 10 pages the validator found quotations that were not verbatim from the record cited.`). One note for a kind and a pack, until the rate falls below the threshold.
5. **Never automatic.** The note is a candidate. It reaches the writer's lessons only by the ordinary path: the digest, the retro, and the operator's ratification (GOVERNANCE §7, channel 4). No code path writes a lesson, and nothing in a page or a source can cause one: the note is made from counts, not from text.
6. **The loop this closes.** A ratified lesson changes the writer's pack; pages built under the old pack become due (§2.6 item 4) and are rebuilt a few at a time; the history then shows the new pack's pass rate beside the old one's. Whether the lesson helped is read off the numbers.


## 3. Authority

- **Nothing new may act.** The seed is assembled from files the daemon already reads for a page. No tool, no network path and no agent write path is added.
- **A seed page shows untrusted text to the operator without a validator's pass.** That is new. Its limits: the banner of §2.2 item 2; outcome `seed` is shown wherever an outcome is; a seed page is a generated page, so it is not indexed, not gathered into another page and never a source (`topic_gather.rs:18`); each block carries its record's own status (`not validated`).
- **`subject` is host-supplied text.** It can change a page's title and slug and nothing else. The acceptance rules run in code; a refused subject is dropped silently and logged.
- **No private record** reaches a seed under this spec (§1.1).

## 4. Slices

One pull request each. Hours are estimates.

| Slice | Content | Criteria | Files | Hours | Depends on |
| :-- | :-- | :-- | :-- | --: | :-- |
| S0 | this spec; the findings of the live proof (step 0) are in §1 and §7 | (the spec) | this file | 1-2 | none |
| A | **first, a read-only count on the operator's workspace** (`delphi pages seeds --count`: for each topic, the spans it would get by row; nothing is written; the result is a dated tracker entry); then the seed: spans, the link rules of §2.1, caps, determinism | AC-1, AC-2, AC-17 | `topic_seed.rs` (new), `topic_gather.rs`, `pages_cmd.rs` | 6-7 | S0 |
| B | the seed page: render, the cleaning, the seed check with the held-text predicate and its failure path, outcome `seed`, the banner, no stub; due check reads span hashes | AC-3, AC-4, AC-18 | `library_topics.rs`, `library_page_validate.rs`, the app's outcome label | 5-7 | A |
| C | `Depth::Long`: the brief, `long_max_tokens`, the seed as the draft, failure keeps the seed | AC-5, AC-6, AC-7 | `library_page_meta.rs`, `library_page_search.rs`, `library_topics.rs`, `config.rs` and the config reference | 4-5 | B; `specs/143` A and B (a writer route); for a page that validates, `specs/142` A (captures), E (anchors), B (held text in the index and its predicate), C (host captures) and G (summaries) |
| D | titles: `subject`, the writer's `title=`, the derived title, `pages rename`, `pages retitle` | AC-8, AC-9 | `library_topics.rs`, `host_research.rs`, `mcp_host.rs`, `pages_cmd.rs` | 4-5 | A |
| E | build now: the trigger at both ends of research, the reply fields, `build_after_research` | AC-10 | `service.rs`, `page_run.rs`, `research_jobs.rs` | 3-4 | B |
| F | the page budget: `week_tokens`, the reserve by depth, pages no longer charged to the validator's share, the health line | AC-11 | `page_run.rs`, `fact_audit.rs`, `config.rs`, `knowledge_health.rs` | 2-3 | C |
| G | the writer role: the whole skill at `long`, `built_with` on every page, due when the writer's pack changes, the daily trickle, `pages rebuild --stale-writer` | AC-13, AC-14 | `page_run.rs`, `role_call.rs`, `library_topics.rs`, `pages_cmd.rs`, `config.rs` | 4-5 | C |
| H | learning from validation: the outcome history, `pages history`, the health line, the view, the note on repeated findings; the app's panel with 140 slice H | AC-15, AC-16 | `page_history.rs` (new), `library_page_validate.rs`, `provisional_notes.rs`, `knowledge_health.rs`, `main.rs` | 5-6 | C |
| | **Total** | | | **35-45** | |

A, B, D and E are useful with no model and are built alongside 143. C and F need the writer route of 143 and the slices of 142 that C names. **G and H are kept late** (the Panel's strategist): they are not started before half of the last ten written pages pass validation, since there is nothing to learn from until pages pass at some rate.

**Live and manual steps are dated.** A criterion that is proved by a run on the operator's workspace, or by a person reading something, counts as met only when the slice's tracker file holds a dated entry that says what was run and what was seen.

## 5. Acceptance criteria

1. **AC-1:** for a topic that follows a research topic with an `answer.md` of 6,000 characters, the seed holds the whole answer as spans in file order, before any span found by search.
2. **AC-2:** assembling a seed twice from the same files gives the same bytes; a private record, an `internal` hit and a generated page are never spans; **no span is a library source's own text**: a source gives its summary and excerpts of at most 300 characters, or nothing.
3. **AC-3:** with no model configured, a research run that ends `done` leaves a page whose text contains the answer's first paragraph, cites it, carries the seed banner and has outcome `seed`.
4. **AC-4:** a topic with no span has no page file and is listed as `waiting for material`; no file under `library/pages/` holds the words "the library holds nothing on this topic yet" for a topic.
5. **AC-5:** a topic rebuild's brief asks for the long size, holds the seed under `DRAFT TO EXPAND`, and is cut at `long_max_tokens`; a search page's brief is unchanged.
6. **AC-6:** a writer that errors, a page that fails the check, and a refused budget each leave the seed page in place with outcome `seed` and the reason; the topic stays due.
7. **AC-7:** a sentence planted in the stored page and in each of its versions appears in no brief of the next rebuild.
8. **AC-8:** `record_research` with `subject: "Web Bot Auth"` makes the topic `web-bot-auth` with that title and the question as `query`; a subject that ends in a question mark or shares no word with the question is refused and the derived title is used.
9. **AC-9:** on a fixture with three unbuilt topics and one built, `delphi pages retitle` prints the renames and writes nothing; with `--apply` each unbuilt topic takes its new slug and the built one keeps its slug and takes the title; a second run changes nothing; two topics that would take the same slug are printed as a collision and `--apply` refuses them.
10. **AC-10:** `record_research` returns the page's slug with outcome `seed`; a queued expansion then runs ahead of an older due topic; with `build_after_research = false` neither happens.
11. **AC-11:** with `week_tokens` spent, a due topic is not written, keeps its seed and is reported by `delphi kb health`; a page's tokens are not counted against the validator's share; the reserve for a long page is four times a summary page's.
12. **AC-12:** a topic whose research record has `claims: []` and an `answer.md` gets a seed page from the answer; the seed holds no claim row and the build does not fail for it.
13. **AC-13:** a long page's call carries the whole `encyclopedia-synthesis` skill, the writer's charter excerpt and its ratified lessons; with a role text too large for the call, lessons are shortened and listed, and the skill is still whole; a build that cannot carry the skill writes no page and raises a notice. The page's sidecar holds the writer's and the validator's `pack_hash` and models.
14. **AC-14:** after the operator ratifies a new writer lesson, every page built under the old pack is due with `the writer role changed`; at idle at most three are rebuilt in a day, oldest first, each from its seed; `delphi pages rebuild --stale-writer` rebuilds the rest; an unratified edit to the writer's files makes nothing due; a new validator pack makes nothing due.
15. **AC-15:** each writer build appends one history line with the fields of §2.7 item 1 and no text of a page, a claim or a source; `delphi pages history` shows the pass rate and the findings by kind for two packs in order; the file is never rewritten.
16. **AC-16:** four builds out of ten with a `not verbatim` finding under one pack leave exactly one provisional note for the writer, made of counts; two out of ten leave none; nothing is written to any lessons file, and the note appears in the next digest for the operator to ratify or drop.
17. **AC-17:** `delphi pages seeds --count` prints, for each topic, how many spans each row of §2.1 would give, and writes nothing; its run on the operator's workspace is a dated tracker entry before slice A's seed code is merged.
18. **AC-18 (the seed page and held text):** a seed page holds no run of more than 300 characters of any held source's text and no more of one source than the aggregate cap of 142 §2.5 item 4, including where an answer or a finding quotes the source at length (the block is shortened and marked); a span with a link, an image and raw HTML renders with none of them and passes the comparison; a block that fails is dropped and counted; a seed with no block left writes no page and raises `check_failed`. This is the same test 142 AC-9 names for a seed page.

## 6. Verification plan

| Criterion | How it is verified |
| :-- | :-- |
| AC-1, AC-2 | unit tests in `topic_seed.rs` on a fixture workspace (`specs/139 AC-1`, `AC-2`), including a property test over shuffled directory order |
| AC-3, AC-4 | engine tests with no model in the pool; a scan of the fixture's `library/pages/` |
| AC-5 | a brief snapshot test for `long`; the existing brief tests for `summary` unchanged |
| AC-6 | three engine tests with a scripted writer (error, failing page, spent budget) |
| AC-7 | the canary test below |
| AC-8, AC-9 | library and CLI tests on a temporary home |
| AC-10 | a daemon test with a scripted host and two due topics |
| AC-11 | engine tests with a fixed clock and a journal fixture |
| AC-12 | a seed test on a record with no claims (the shape step 0 found) |
| AC-13, AC-14 | engine tests with a scripted writer and a ratified-store fixture; a test that an unratified edit changes no hash |
| AC-15, AC-16 | history tests on a fixture with a fixed clock; a scan of the history and the note for fixture page text; a test that no lessons file changes |
| AC-17 | a CLI test on a fixture; the live run, dated in the tracker |
| AC-18 | seed tests with a held-source fixture, shared with the test of 142 AC-9 |

**Fitness functions** (checks that fail when an invariant erodes; owner `builder` unless said):

| Invariant | Check that would fail | Kind | State |
| :-- | :-- | :-- | :-- |
| a rebuild never reads the previous page into a brief | `library_topics::tests::no_brief_holds_text_of_the_previous_page` (the canary of AC-7) | Rust test | to be written |
| a topic page is never the stub | `library_topics::tests::topic_rebuild_never_calls_the_claims_only_fallback` (reads the production text of `library_topics.rs` for `claims_only_page(`) | Rust test | to be written |
| the seed is deterministic | the property test of AC-2 | Rust test | to be written |
| no private record is a span | `topic_seed::tests::a_private_record_is_never_a_span`; replaced, not deleted, by the label test of `specs/141` | Rust test, owner `red-team` | to be written |
| a seed block is verbatim from its span | the seed check of §2.2 item 3, run in `topic_store` | daemon check | to be written |
| a long page is never written without the writer's skill | `page_run::tests::a_long_page_call_always_carries_the_whole_page_skill` (AC-13) | Rust test | to be written |
| only a ratified change to the writer makes pages due | `library_topics::tests::an_unratified_writer_edit_makes_no_page_due` (AC-14) | Rust test, owner `red-team` | to be written |
| validation findings never become a lesson by themselves | `page_history::tests::a_repeated_finding_writes_a_note_and_no_lesson` (AC-16) | Rust test, owner `red-team` | to be written |
| the outcome history holds no page text and is never rewritten | the scan of AC-15, and an append-only test as in 140 | Rust test | to be written |
| a seed page is bound by the held-text predicate like every other output | `topic_seed::tests::a_seed_page_passes_body_may_leave_for_every_source` (AC-18), and 142 AC-9's scan, which names the seed page | Rust test, owner `red-team` | to be written |
| a source's own text is never a seed span | `topic_seed::tests::no_span_is_a_source_body` (AC-2) | Rust test, owner `red-team` | to be written |

### 6.1 Premortem (three months after release): how this most likely fails

| # | Cause, as it happened | Likely | Damage | Early sign | Response in this spec |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 1 | Pages stayed seeds: slice C shipped before a writer route existed (step 0: every endpoint is a harness), or before sources had text, so every written page failed its check and fell back. | high (this is what step 0 saw) | medium (the seed is still the research) | `delphi kb health` counts pages by outcome and `seed` does not fall | the order: 143 and 142 before C; A and B need no model; health gains the count (slice B) |
| 2 | Seeds were long and poor: an answer capped at 7,900 characters (`research_record.rs:629`) plus loosely linked summaries made a page nobody read. | medium | medium | the operator's first read of three seed pages | rows 1 and 2 first and never dropped (§2.1 item 3); rows 3 to 5 need a real link, not a word match |
| 3 | A derived title was wrong or two unrelated questions merged into one slug. | medium | medium (a merged page is hard to split) | `pages retitle` dry run shows a collision | re-slug only at `version` 0; the dry run prints collisions and `--apply` refuses them; `operator` always wins |
| 4 | The long page tripled writer and validator tokens and spent the background share by noon. | medium | low | the journal's page spans | `per_day` and the share gate are unchanged; the seed costs nothing; OD-2 |
| 5 | An unvalidated seed was read as a finished page and quoted in a document. | low | medium | a document citing a `seed` page | the banner; outcome shown in every list; a seed is never a source (§3) |
| 6 | The build-now trigger rebuilt on every `record_research` call of a busy session. | medium | low | ten seed writes in an hour | the seed is free and idempotent; the expansion is queued once per topic while one is waiting |

Not acted on: 5 beyond the banner (a reader who ignores a banner is outside what code can check).

## 7. What is not verified

- **Step 0 is reported, not re-run here.** Its findings (O7 to O9) are written as relayed. The route failure is fixed by `specs/143`; the rewrite bug is fixed in commit `c1fca8ca` on the local branch `step0/live-proof`, which is not on main.
- **That a seed-fed writer passes validation.** Step 0's pages failed because the validator had only pointers. Seed spans give it text for rows 1 to 6; whether that is enough before 142 lands is not observed.
- **The size of the page budget.** 600,000 tokens a week is a guess from step 0's one figure (about 6,000 tokens for a short page) and an assumed fourfold cost for a long one.
- That a role's `pack_hash` (`mcp_packs.rs:286`) changes exactly when the text `role_call` loads for the writer changes, and not otherwise. If the two can differ, slice G records a hash of the loaded role text and uses that for §2.6 item 4.
- That the page check and the validator report their findings in kinds that map onto the closed list of §2.7 item 1. Slice H reads them; what does not map is counted as `other`.
- Whether the page check and `finish_page` accept a block longer than 1 KiB when it is a seed block and not a quotation. The seed has its own check (§2.2 item 3) so that this does not matter; slice B confirms.
- Whether a summary page's bibliography names its sources in a form code can match to a research record's sources (row 4 of §2.1). If not, row 4 is dropped in slice A and said so in the tracker.
- Whether one writer call produces a good 2,000-word page. Not measured; the outline-first method is step 7.
- The cost of validating a long page.

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | Is a seed page shown as a page (with its banner)? | **Decided (operator, 2026-11-25): shown.** | decided | none |
| OD-2 | The cap for a long page: 8,000 tokens? | **Decided (operator, 2026-11-25, "use your defaults"): 8,000**, and `per_day` stays 20 | decided | none |
| OD-3 | May `record_research` take `subject` from the host as the page title? | **Decided (operator, 2026-11-25, "use your defaults"): yes**, under the rules of §2.4 item 3 | decided | none |
| OD-4 | Re-slug the nine unbuilt topics on the operator's workspace? | **Decided (operator, 2026-11-25): yes**, to concept titles, by `pages retitle --apply` after the dry run | decided | none |
| OD-5 | The weekly page budget: 600,000 tokens, apart from the validator's share? | **Decided (operator, 2026-11-25): yes, 600,000 a week**; revisit after the first week of long pages | decided | none |
| OD-6 | Pull 141's page-export guard and its lift forward, so private research reaches pages sooner? | **Decided (operator, 2026-11-25): no, do not pull 141 forward.** Seeds come from public answers meanwhile | decided | none |
| OD-7 | The trickle for pages due because the writer changed, and whether a new validator pack makes pages due | **Decided (operator, 2026-11-25, "use your defaults"): three a day; a validator change makes nothing due.** | decided | none |
| OD-8 | The threshold for a note to the writer | **Decided (operator, 2026-11-25, "use your defaults"): one kind of finding in three of the last ten builds**, revised when the history has a month in it | decided | none |
| OD-9 | The rate that releases the late slices (G and H here, 140 H, 142 F): half of the last ten written pages passing validation? | **Decided (operator, 2026-11-25, "as recommended"):** as written; the operator may release them sooner by saying so | decided | none |

## 9. Review

Panel `t_0004` (a witnessed host Panel of 2026-11-25, session `host-e11ba3fe9035ccf9`; architect, red-team and strategist seats, a blind Round 1, all three parsed in both rounds) read specs 139 to 143 and ruled **B: apply the fixes, then approve**. Its record is committed at `missions/2026-11-25-knowledge-v2/councils/004-witnessed-red-team-review-of-draft-specs.md`, with one line redacted (the operator's project names), because this repository is public.

**It is not cited in `reviewed_by`, and it does not count as an independent review.** The record says so itself (`counts_as_independent_review: false`, `not_independent_because: single family`): every seat ran on one model family, which is also the family that wrote these specs, and `delphi lint` refuses a citation of such a Panel. The red-team seat did read the texts whole this time, and every P0 and P1 it and the other seats raised is applied below. What GOVERNANCE §1.3 still needs is a review by a second model family, or the operator's own waiver in the approval words. Nobody has re-read the amended text.

| Finding (on this spec) | Severity | Response |
| :-- | :-- | :-- |
| seed pages copy held text verbatim (up to 8 KiB) and skip the page check that 142 §2.5 item 4 and AC-9 rely on | P0 | §2.1 item 3: a source's own text is never a span; §2.2 item 3: the seed page passes the shared predicate `body_may_leave`; AC-2, AC-18; 142 AC-9 names the seed page |
| §2.2 item 3: "letter for letter" against "no link"; images and HTML unnamed; no stated result when the seed check fails | P1 | §2.2 item 3: one cleaning function, used for the render and the comparison; the failure path |
| seed value unmeasured | P1 | slice A opens with a read-only count on the operator's workspace; AC-17 |
| 139 C waits on 142 slices it does not name | P1, P2 | slice C names 142 A, E, B, C and G |
| the page budget is one of several meters and switches | P2 | §2.3 item 8: a row of 143's admission table, one ledger, one function |
| "build now" is paused in host sessions | P2 | §2.5 item 1: seed now, expand at idle |
| AC-9 tied to the live workspace | P2 | AC-9 on a fixture |
| slice H leans on 140's ledger rules and on a note entry point nobody owns | P2 | §2.7 item 2: owned here |
| keep G and H late | P2 | §4; OD-9 |
| the seed predicate covered only the sources the topic's records read | P2, `t_0004` red-team recheck | §2.2 item 3: every held source the seed text shares a block with (142's block index), plus the topic's own; 142 AC-31 names the seed page |
| live and manual criteria need dated tracker entries | P2 | §4 |

**Not adopted:** nothing on this spec was rejected. One reading was narrowed: the Panel asked that held spans be cut to 300 characters **or** replaced by the source's summary; this text does both and goes further (no source body is ever a span), which makes the rule independent of what is held.
