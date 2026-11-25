---
id: spec-139-pages-seeded-from-research-a-seed-that-is-never-empty-a-long-depth-and-concept-titles
title: "139: Pages seeded from research: a seed that is never empty, a long depth, and concept titles"
author: architect
timestamp: 2026-11-25T20:00:00Z
status: draft
reviewed_by: []   # no Council or Panel has read this text. The plan it comes from was put to a daemon Council on 2026-11-25 (two seats heard, the strategist seat abstained); that is a review of the plan, not of this spec, and is not cited as one.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors read on 2026-11-25 at origin/main 52bdd3b8 (0.6.7). Nothing was run. The result of the live proof (step 0) is not in yet (section 7).
source_mission: 2026-11-25-knowledge-v2
tags: [spec, library, pages, topics, research, seed, writer, encyclopedia]
---

# Spec 139: Pages seeded from research

> **Status: draft.** First of four specs from the knowledge audit of 2026-11-25 (139 to 142). Build order, settled by the operator on 2026-11-25: the live proof (step 0, running separately), **this spec**, the golden set (`specs/140`), label enforcement (`specs/141`), the source text cache (`specs/142`), then the one knowledge record and the later steps, which have no spec yet.
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

Objective: a page exists as soon as research on its topic ends, and is never empty. Code assembles a **seed** from what the research wrote. The writer expands the seed into a long page. A writer that fails leaves the seed in place. Every rebuild starts from the seed and the source spans.

### 1.1 Non-goals

- **No private record on a page yet.** A seed span from a private record is left out, as today (`library_topics.rs:16-19`, `:449`, `:580`). `specs/141` lifts that once its guards pass. On the operator's workspace the 82 project research files therefore join seeds at step 4 of the build order, not here.
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
| 2 | the live claims of those topics, as today (`library_topics.rs:576`) | `research_topic` |
| 3 | findings (116 §5) written by a session or mission that also ran or recorded research on the topic; the run pointer files name the session (`research_record.rs:731`) | `session` |
| 4 | kept summary pages (`local/summaries/`, `kb_import.rs:12`, `:261`) whose bibliography names a source the topic's record read | `shared_source` |
| 5 | a project's research file that an ingest run recorded as the topic's answer by reference (113 §3 item 1) | `research_topic` |
| 6 | what today's search returns (`topic_gather::gather`), to fill what is left | `search` |

3. Caps: at most 40 spans and 64 KiB of text in a seed; a span is at most 8 KiB and is cut at a paragraph. Rows 1 and 2 are never dropped to make room for rows 3 to 6. The order inside a row is by path, then line, so the same inputs give the same seed (byte for byte).
4. What is left out is what today's gather leaves out (`topic_gather.rs:13-18`): a private record, a hit flagged `internal`, another project's hit, staged and provisional notes, episodes, and any generated page.
5. Seed text is data from outside. It keeps the tier of the document it came from, is cleaned as record text is (`record_text`, `library_page_search.rs:600`), and passes the workspace's identifier mask (`host_research::mask`, `host_research.rs:263`).

### 2.2 The seed page

1. The seed is rendered by code into a page: `# <title>`; a lead that is the first span of the newest answer; the rest of the answer under its own headings; `## Findings` and `## Related notes` for rows 3 to 5; the open questions and the bibliography as the daemon writes them today (`library_topics.rs:679`). Each block is a span's own words followed by its marker.
2. A seed page has the outcome **`seed`**, a fourth value beside `pass`, `pass_contested` and `claims_only` (`library_page_validate.rs:231`). Its banner reads: `Seed draft: assembled by code from research notes. Not written up and not validated.` The validator is not run on it.
3. A seed page is checked by code before it is stored: every block is found, letter for letter, in the span its marker names; it holds no link other than section and `page:` links; and no span comes from a document §2.1 item 4 leaves out.
4. **Never empty.** A topic with at least one span gets a seed page. A topic with no span (one the operator made by hand before any research) gets **no page**: it is listed as `waiting for material`, and `delphi pages rebuild` says so. The stub of O2 is no longer written for a topic.
5. Writing a seed page calls no model, spends no tokens and does not count against `[library.pages] per_day`.

### 2.3 The writer expands the seed; a failure keeps it

1. A new depth, **`long`** (`Depth::Long`, beside `library_page_meta.rs:71-75`). Topic pages are written at `long`; search pages keep `summary` and `expanded`.
2. The brief (`brief_for`, `library_page_search.rs:612`) gains a size line for `long`: a lead of two to four paragraphs, then `## ` sections that follow the seed's own structure, 1,200 to 2,500 words; a `## Contents` list; key terms, start here and see also as 117 §2 item 5 asks; a figure wherever cited numbers or named parts support one, by the figure block the brief already describes (118 §5 items 1 to 3); no References section, because the daemon writes the bibliography.
3. The writer is given the seed page as `DRAFT TO EXPAND` and the spans, claims, sources and assets as `RECORDS`. It may reorder, merge and rewrite; every statement still needs a marker, and the page check and the validator are unchanged.
4. The cap for a long page is a new key, `[library.pages] long_max_tokens`, default 8,000 (the scout's cap, [`specs/114`](114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis.md) §13). `max_tokens` (3,000) still governs search pages.
5. **A failure keeps the seed.** When the writer errors, when its page fails the check after the rewrites the validator allows, or when the budget or share gate refuses the call, the seed page is stored (or stays) with outcome `seed` and the reason. `claims_only_page` is no longer called from `topic_rebuild` (`library_topics.rs:623`, `:641`).
6. **A rebuild starts from the seed and the spans, never from the previous page.** Each rebuild assembles the seed again from the stored documents. The previous page and its versions are read only to compute the change line (`library_topics.rs:341`) and to keep the version. No text of a previous page enters a brief.

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

1. When a research run ends `done` or `partial` (`Topic::finish`, `research_record.rs:509`) and when `record_research` returns (`host_research.rs:330`), the daemon, for that research topic: makes the topic if there is none (`topics_from_research`), writes the seed page at once, and queues the writer's expansion ahead of the idle queue.
2. The expansion runs within `per_day` and the background share as every rebuild does (`library_topics.rs:625`, `page_run.rs:89`). When either refuses, the topic stays due and the seed stands.
3. `[library.pages] build_after_research` (default `true`) turns the trigger off. `auto_rebuild = false` still leaves the writer's rebuilds to the operator; the seed is written regardless, because it costs nothing.
4. The reply to `record_research` and the research result name the page's slug and its outcome (`seed`, until the writer's page lands).

## 3. Authority

- **Nothing new may act.** The seed is assembled from files the daemon already reads for a page. No tool, no network path and no agent write path is added.
- **A seed page shows untrusted text to the operator without a validator's pass.** That is new. Its limits: the banner of §2.2 item 2; outcome `seed` is shown wherever an outcome is; a seed page is a generated page, so it is not indexed, not gathered into another page and never a source (`topic_gather.rs:18`); each block carries its record's own status (`not validated`).
- **`subject` is host-supplied text.** It can change a page's title and slug and nothing else. The acceptance rules run in code; a refused subject is dropped silently and logged.
- **No private record** reaches a seed under this spec (§1.1).

## 4. Slices

One pull request each. Hours are estimates.

| Slice | Content | Criteria | Files | Hours | Depends on |
| :-- | :-- | :-- | :-- | --: | :-- |
| S0 | this spec; the result of the live proof (step 0) written into §7, and the order re-read against it | (the spec) | this file | 1-2 | step 0 |
| A | the seed: spans, the link rules of §2.1, caps, determinism | AC-1, AC-2 | `topic_seed.rs` (new), `topic_gather.rs` | 5-6 | S0 |
| B | the seed page: render, the seed check, outcome `seed`, the banner, no stub; due check reads span hashes | AC-3, AC-4 | `library_topics.rs`, `library_page_validate.rs`, the app's outcome label | 4-5 | A |
| C | `Depth::Long`: the brief, `long_max_tokens`, the seed as the draft, failure keeps the seed | AC-5, AC-6, AC-7 | `library_page_meta.rs`, `library_page_search.rs`, `library_topics.rs`, `config.rs` and the config reference | 4-5 | B |
| D | titles: `subject`, the writer's `title=`, the derived title, `pages rename`, `pages retitle` | AC-8, AC-9 | `library_topics.rs`, `host_research.rs`, `mcp_host.rs`, `pages_cmd.rs` | 4-5 | A |
| E | build now: the trigger at both ends of research, the reply fields, `build_after_research` | AC-10 | `service.rs`, `page_run.rs`, `research_jobs.rs` | 3-4 | B |
| | **Total** | | | **21-27** | |

A and B are useful with no model. C is the only slice that needs the writer route the live proof is testing.

## 5. Acceptance criteria

1. **AC-1:** for a topic that follows a research topic with an `answer.md` of 6,000 characters, the seed holds the whole answer as spans in file order, before any span found by search.
2. **AC-2:** assembling a seed twice from the same files gives the same bytes; a private record, an `internal` hit and a generated page are never spans.
3. **AC-3:** with no model configured, a research run that ends `done` leaves a page whose text contains the answer's first paragraph, cites it, carries the seed banner and has outcome `seed`.
4. **AC-4:** a topic with no span has no page file and is listed as `waiting for material`; no file under `library/pages/` holds the words "the library holds nothing on this topic yet" for a topic.
5. **AC-5:** a topic rebuild's brief asks for the long size, holds the seed under `DRAFT TO EXPAND`, and is cut at `long_max_tokens`; a search page's brief is unchanged.
6. **AC-6:** a writer that errors, a page that fails the check, and a refused budget each leave the seed page in place with outcome `seed` and the reason; the topic stays due.
7. **AC-7:** a sentence planted in the stored page and in each of its versions appears in no brief of the next rebuild.
8. **AC-8:** `record_research` with `subject: "Web Bot Auth"` makes the topic `web-bot-auth` with that title and the question as `query`; a subject that ends in a question mark or shares no word with the question is refused and the derived title is used.
9. **AC-9:** `delphi pages retitle` prints the nine renames and writes nothing; with `--apply` an unbuilt topic takes its new slug and a built one keeps its slug and takes the title; a second run changes nothing.
10. **AC-10:** `record_research` returns the page's slug with outcome `seed`; a queued expansion then runs ahead of an older due topic; with `build_after_research = false` neither happens.

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

**Fitness functions** (checks that fail when an invariant erodes; owner `builder` unless said):

| Invariant | Check that would fail | Kind | State |
| :-- | :-- | :-- | :-- |
| a rebuild never reads the previous page into a brief | `library_topics::tests::no_brief_holds_text_of_the_previous_page` (the canary of AC-7) | Rust test | to be written |
| a topic page is never the stub | `library_topics::tests::topic_rebuild_never_calls_the_claims_only_fallback` (reads the production text of `library_topics.rs` for `claims_only_page(`) | Rust test | to be written |
| the seed is deterministic | the property test of AC-2 | Rust test | to be written |
| no private record is a span | `topic_seed::tests::a_private_record_is_never_a_span`; replaced, not deleted, by the label test of `specs/141` | Rust test, owner `red-team` | to be written |
| a seed block is verbatim from its span | the seed check of §2.2 item 3, run in `topic_store` | daemon check | to be written |

### 6.1 Premortem (three months after release): how this most likely fails

| # | Cause, as it happened | Likely | Damage | Early sign | Response in this spec |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 1 | Pages stayed seeds: the writer route never worked on the operator's install, which is what the live proof is still finding out, so every page read "seed draft" for weeks. | high (no page has ever been built) | medium (the seed is still the research) | `delphi kb health` counts pages by outcome and `seed` does not fall | S0 waits for step 0; A and B need no model; health gains the count (slice B) |
| 2 | Seeds were long and poor: an answer capped at 7,900 characters (`research_record.rs:629`) plus loosely linked summaries made a page nobody read. | medium | medium | the operator's first read of three seed pages | rows 1 and 2 first and never dropped (§2.1 item 3); rows 3 to 5 need a real link, not a word match |
| 3 | A derived title was wrong or two unrelated questions merged into one slug. | medium | medium (a merged page is hard to split) | `pages retitle` dry run shows a collision | re-slug only at `version` 0; the dry run prints collisions and `--apply` refuses them; `operator` always wins |
| 4 | The long page tripled writer and validator tokens and spent the background share by noon. | medium | low | the journal's page spans | `per_day` and the share gate are unchanged; the seed costs nothing; OD-2 |
| 5 | An unvalidated seed was read as a finished page and quoted in a document. | low | medium | a document citing a `seed` page | the banner; outcome shown in every list; a seed is never a source (§3) |
| 6 | The build-now trigger rebuilt on every `record_research` call of a busy session. | medium | low | ten seed writes in an hour | the seed is free and idempotent; the expansion is queued once per topic while one is waiting |

Not acted on: 5 beyond the banner (a reader who ignores a banner is outside what code can check).

## 7. What is not verified

- **Why no page has been built.** Step 0 is still running. If it finds the writer route (`page_run.rs:102`) cannot resolve on the operator's install, slices A, B, D and E are unaffected and C waits for the fix.
- Whether the page check and `finish_page` accept a block longer than 1 KiB when it is a seed block and not a quotation. The seed has its own check (§2.2 item 3) so that this does not matter; slice B confirms.
- Whether a summary page's bibliography names its sources in a form code can match to a research record's sources (row 4 of §2.1). If not, row 4 is dropped in slice A and said so in the tracker.
- Whether one writer call produces a good 2,000-word page. Not measured; the outline-first method is step 7.
- The cost of validating a long page.

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | Is a seed page shown as a page (with its banner), or held back until the writer's page exists? | shown: this is what "never empty" asks | proposed: yes | B |
| OD-2 | The cap for a long page: 8,000 tokens? | 8,000, and `per_day` stays 20 | proposed: yes | C |
| OD-3 | May `record_research` take `subject` from the host as the page title? | yes, under the rules of §2.4 item 3 | proposed: yes (a title can be renamed) | D |
| OD-4 | Re-slug the nine unbuilt topics on the operator's workspace? | yes, by `pages retitle --apply` after reading the dry run | proposed: no once pages are written (a built page keeps its slug) | D |
