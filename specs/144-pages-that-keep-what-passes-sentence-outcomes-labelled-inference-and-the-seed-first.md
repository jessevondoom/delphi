---
id: spec-144-pages-that-keep-what-passes-sentence-outcomes-labelled-inference-and-the-seed-first
title: "144: Pages that keep what passes: sentence outcomes, labelled inference, and the seed first"
author: architect
timestamp: 2026-11-27T17:00:00Z
status: draft   # the operator approved the approach on 2026-11-27 ("i approve that approach"), not this text; the text needs the review GOVERNANCE §1.3 asks for and the operator's approval
source_type: internal_generated
confidence: medium   # built on the first live pages through the bridge (2026-11-27, a copy of the operator's install, two topics) and the architect's review of them (help t_0001); nothing in this spec is built
source_mission: 2026-11-25-knowledge-v2
tags: [spec, pages, validator, writer, inference, encyclopedia]
---

# Spec 144: Pages that keep what passes

> **Status: draft.** The operator approved the approach on 2026-11-27: "yes fix the (1) bugs and draft an ammendment for (3) — i approve that approach". This text is not yet reviewed or approved.
> **Amends:** [`specs/057`](057-search-pages-generated-and-checked.md) §3.3 and §3.4 (what a failed check does to a page), [`specs/139`](139-pages-seeded-from-research-a-seed-that-is-never-empty-a-long-depth-and-concept-titles.md) §2.3 item 5 and §2.5 item 1 (a failure keeps the seed; the writer's expansion at idle). **Builds on:** the validator fixes of 2026-11-27 on `knowledge-v2` (a reply sized to the page and ended by `END ASSERTIONS`, records shown in part or not at all marked as such, the citation's reach stated, every verdict logged with the rejected draft).

## 1. Why

The first two pages written through the bridge both failed, and the seed stood. In both, the validator was right: the writer said more than its citation.

1. A seed carried a sentence about the research ("The retrieved papers are MetaGPT, ChatDev …"), and the writer kept it.
2. The brief made the lead start "**Title** is …", and the writer widened a passage about positioning into a definition of the whole topic.

The brief and the seed are fixed for both. What remains is the rule itself. Today one failing statement throws the whole page away (057 §3.4, 139 §2.3 item 5). The architect's review: checking each statement is right, rejecting the whole page for one is wrong. If each statement failed independently 2% of the time, a 60-statement page would pass about 30% of attempts (an illustration, not a measurement). The operator's concern is the same from the other side: much checking, little delivered, and real insight lost.

Each attempt also costs: about 77,000 tokens through the bridge (three calls), so the page budget of 600,000 a week allows seven or eight.

### 1.1 Non-goals

- Validating research claims before a page may use them. A page states what its records say, at their standing; claims are validated on their own path (`kb validate`).
- Changing what the validator judges, or relaxing the mechanical check.
- A new page kind for search pages. This spec changes outcomes for search pages and topic pages alike.

### 1.2 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | keep the whole-page rule; improve the writer's brief and retry until a page passes | the trial's pass rate per attempt and tokens per page delivered; the 2026-11-27 runs say one attempt costs about 77,000 tokens |
| B | constraint removed: "a page passes or fails whole" | judge each statement and keep what passes, mark what is unchecked, fall back to the seed only past a threshold (§2.1) | AC-1 to AC-4 with scripted validators; the trial counts pages delivered and statements removed |
| C | analogy: an editor's marked-up proof | show the writer's whole draft with each statement coloured by its verdict, and let the operator accept or strike | a window that shows verdicts beside statements; tested by the operator reading five drafts; it delivers no page without the operator's work |

Chosen: **B**, with C's view kept as the rejected-draft view of §2.4 (read only). A spends the budget on retries for what one bad statement causes. C makes every page the operator's editing job, which is the cost the operator named. Flagged by the guard (a new layer of abstraction with no test of its own): none.

## 2. Design

### 2.1 A page keeps what passes

After the one rewrite of 057 §3.4, the daemon builds the page from the statements that passed. It does not throw the whole page away.

1. **Removed:** a statement judged `fabricated` (its citation does not say it), and a statement that contradicts a validated record or a record the page cites. A false citation is never shown. The marker and the statement go; the paragraph closes around the gap.
2. **Kept, marked unchecked:** a statement judged `uncited` (consistent, no record backs it), and one judged against a record the validator saw only in part or not at all. It stays in place with an inline mark, `(unchecked)`, instead of moving to a list at the end.
3. **Contested:** as today (057 §3.3): side by side under **Contested**.
4. **The page falls back to the seed** (139) when more than **20%** of the statements were removed, when the lead is removed, or when the page fails the mechanical check after the removals (a heading with nothing under it, a figure whose data rested on a removed statement).
5. **Mechanical failures still block.** A finding of the page check (057 §3.2, 097) is never repaired by removal; it goes to the rewrite, and a second failure leaves the seed.
6. **What the operator sees:** the banner says how many statements were removed and how many are unchecked, and links the rejected draft (§2.4). The outcome is `pass_trimmed` (new) when anything was removed, else as today.

### 2.2 Insight, labelled as inference

A conclusion that no single record states, drawn from two or more records, has its own form. It is never stated as fact.

1. **Form:** a block `> **Inference** (from [c-1], [s-2]): …` on its own line. The writer may place inferences in sections, never in the lead and never as a definition.
2. **Premises:** at least two distinct records, cited in the block. Two records of one origin (the same source, or claims anchored only to it) count as one; the daemon checks this by the records' sources, not the writer's word.
3. **No new matter:** an inference adds no quotation, number or proper name that its premises do not hold. The page check rejects one that does.
4. **The validator judges** that each premise says what the block takes from it, that the conclusion is reasonably drawn, and that no record contradicts it. Verdicts: `inference_holds`, `inference_overreaches`, `contradicts`.
5. **It stays labelled** when it holds. It never becomes a claim and never raises a record's standing. One that overreaches is removed under §2.1.
6. **Overreach relabelled as inference** is a risk: the trial (§2.5) counts inferences the operator judges unsupported.

### 2.3 The seed first; the long page on request

1. The seed (139 §2.2) is the topic's page at once, as now.
2. **New key `[library.pages] long = "on_request" | "auto"`, default `on_request`.** With `on_request` the writer's expansion runs only when the operator asks (`delphi pages rebuild <slug> | --due | --all`, the app's button). With `auto` it is queued at idle as 139 §2.5 item 1 says today.
3. The default is revisited after the trial (§2.5): `auto` if long pages pass its thresholds.

### 2.4 Every failure can be read

Built with the fixes of 2026-11-27: the library's log keeps each rejected draft with every verdict, the validator's model, and the records shown in part or not shown. `delphi pages topic <slug> --rejected` prints the last rejected draft with each statement's verdict beside it. The log is local and never leaves the machine.

### 2.5 The trial that decides the default

Before `long` may default to `auto`, a trial on the operator's install. Five to ten topics, each built with this spec, read by the operator against its seed. The thresholds are set here, before the run:

| Measure | Threshold | If not met |
| :-- | :-- | :-- |
| **Value:** a topic's long page holds a correct, useful insight its seed lacks | at least 3 of 5 | the default stays `on_request`; consider short anchored summaries instead |
| **Checker share:** removals caused by the checker (citation reach, missing evidence, validator error) | at most 25% | fix the checker before judging the policy |
| **False passes:** of 10 errors planted across the topics (some late in long pages), how many pass | at most 2 | a pass is not presented as protection |
| **Draft quality:** of 20 accepted statements per topic checked by hand, how many are wrong or unsupported | at most 10% | do not relax §2.1 further |
| **Usability:** pages delivered, share of statements removed or unchecked, tokens per page the operator found useful | recorded | — |

Each removal is classified as: writer overreach, citation reach, missing evidence, validator error, or research the source does not support. `kb validate --pending` measures claims, not pages; it does not decide this.

## 3. Authority

Nothing here widens authority. A removal only takes text out; an `(unchecked)` mark and an inference label only lower what a statement claims. No statement's standing rises. The rejected-draft view reads the local log.

## 4. Slices

| Slice | Content | Criteria | Files | Hours |
| :-- | :-- | :-- | :-- | --: |
| A | §2.1 a page keeps what passes; `pass_trimmed`; the banner line; the 20% fallback | AC-1 to AC-4 | `library_page_validate.rs`, `library_topics.rs`, `library_page_check.rs` | 4-6 |
| B | §2.2 labelled inference: the form, the premises check, the validator's verdicts, the page check | AC-5 to AC-7 | `library_page_search.rs`, `library_page_validate.rs`, `library_page_check.rs` | 5-7 |
| C | §2.3 `[library.pages] long`; §2.4 `pages topic --rejected` | AC-8, AC-9 | `config.rs`, `page_run.rs`, `pages_cmd.rs` | 2-3 |
| D | §2.5 the trial on the operator's install, its report and the default decided | AC-10 | (a run; the tracker) | 2-3 and the operator's reading |
| | **Total** | | | **13-19** |

## 5. Acceptance criteria

1. **AC-1:** a page whose validator judges one statement `fabricated` after the rewrite is written without that statement and its marker, with outcome `pass_trimmed` and the banner's count.
2. **AC-2:** a statement judged `uncited`, or `fabricated` against a record shown in part, stays in place marked `(unchecked)`.
3. **AC-3:** with more than 20% of statements removed, or the lead removed, or a heading left empty, the seed stands with its reason.
4. **AC-4:** a mechanical finding is never repaired by removal: after the rewrite it leaves the seed.
5. **AC-5:** an inference block with one premise, or two premises of one origin, fails the page check.
6. **AC-6:** an inference block that adds a number its premises do not hold fails the page check.
7. **AC-7:** an inference judged `inference_holds` stays labelled on the page and creates no claim; one judged `inference_overreaches` is removed under §2.1.
8. **AC-8:** with `long = "on_request"`, the idle rebuild writes no long page; `delphi pages rebuild` does.
9. **AC-9:** `delphi pages topic <slug> --rejected` prints the last rejected draft with each statement's verdict.
10. **AC-10:** the trial's report, dated in the tracker, gives each measure of §2.5 against its threshold, and the default of `long` it leads to.

## 6. Verification plan

Slices A to C with scripted writers and validators, as 139 C's tests do. Slice D is a live run on the operator's copy (`delphi-kv2` on `~/.delphi-kv2`), read by the operator.

## 7. What is not verified

- Whether removing statements leaves readable prose. The trial reads it.
- Whether the 20% threshold is right; it is a first value.
- The validator's accuracy on inferences; no model has judged one.

## 8. Decisions for the operator

1. **The fallback threshold:** 20% of statements removed (the architect's figure), or another.
2. **Removed or marked:** this text removes `fabricated` statements and marks `uncited` ones. The other choice is to keep a `fabricated` statement without its citation, marked `(unchecked)`. Removal is recommended: a statement whose citation was false is the writer's, not the record's.
3. **The default of `long`:** `on_request` until the trial (recommended), or `auto` now.

## 9. Review

Not reviewed. GOVERNANCE §1.3 asks for an independent review before approval: a Council with a seat of a second model family, or the operator's waiver. The architect's review of the problem (help `t_0001`, 2026-11-27, read on an older checkout) is the basis of §2, not a review of this text.
