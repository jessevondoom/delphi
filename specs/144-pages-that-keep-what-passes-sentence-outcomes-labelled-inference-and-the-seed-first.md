---
id: spec-144-pages-that-keep-what-passes-sentence-outcomes-labelled-inference-and-the-seed-first
title: "144: Pages that keep what passes: numbered units, a verdict for each, the best attempt, and the seed first"
author: architect
timestamp: 2026-11-27T21:00:00Z
status: draft   # revision 2. The operator approved the approach on 2026-11-27 ("i approve that approach"), not this text; it needs the review GOVERNANCE §1.3 asks for and the operator's approval
source_type: internal_generated
confidence: medium   # built on seven live page attempts through the bridge (2026-11-27, a copy of the operator's install, five topics), the architect's review (help t_0001) and Council t_0002 on revision 1; nothing in this spec is built
source_mission: 2026-11-25-knowledge-v2
tags: [spec, pages, validator, writer, encyclopedia]
---

# Spec 144: Pages that keep what passes

> **Status: draft, revision 2.** The operator approved the approach on 2026-11-27: "yes fix the (1) bugs and draft an ammendment for (3) — i approve that approach". Revision 1 was reviewed by Council `t_0002`, and every finding of its heard seat is applied here (section 9). Labelled inference (revision 1's §2.2) is deferred to a later spec. This text is not reviewed or approved.
> **Amends:** [`specs/057`](057-search-pages-generated-and-checked.md) §3.3 and §3.4 (what a failed check does to a page), [`specs/139`](139-pages-seeded-from-research-a-seed-that-is-never-empty-a-long-depth-and-concept-titles.md) §2.3 item 5 and §2.5 item 1 (a failure keeps the seed; the writer's expansion at idle), [`specs/097`](097-encyclopedia-search-pages.md) §4 (the validator's reply). **Builds on:** the validator fixes of 2026-11-27 on `knowledge-v2` (a reply sized to the page and ended by `END ASSERTIONS`; records shown in part or not at all marked as such; the citation's reach stated; every verdict logged with the rejected draft).

## 1. Why

**The evidence.** Seven page attempts through the bridge on 2026-11-27, five topics:

| Topic | Best attempt's verdicts | Outcome today |
| :-- | :-- | :-- |
| software architect agents | 47 supported, 8 unchecked, **1** fail (first draft); the rewrite had 6 | seed stands |
| brand and product strategy | passed | **written** |
| business strategists | 78 supported, 3 unchecked, **3** fail | seed stands (the budget then refused the rewrite) |
| personal assistant agents | 23 supported, 2 unchecked, **11** fail | seed stands |
| fact-checking agents | 44 supported, **18** fail | seed stands |

Two causes account for nearly every failure:

1. **One failing statement throws the whole page away** (057 §3.4, 139 §2.3 item 5). Two topics had 1 and 3 failures among 56 and 84 statements.
2. **The writer is given sources with no text.** 130 of the library's 131 sources are pointers: a title and an address. The writer infers from titles ("one paper uses multi-agent debate …") and the validator, seeing an empty record, rightly finds nothing to support it. Every failure on the fact-checking topic says "body empty".

Also seen: **the rewrite can be worse than the draft** (1 failure became 6), and **each attempt costs about 77,000 tokens** (three calls), so the page budget of 600,000 a week allows seven or eight.

### 1.1 Non-goals

- Validating research claims before a page may use them.
- Labelled inference: a conclusion drawn across records. Deferred to its own spec; code cannot yet check a premise's origin for every record kind, nor catch invented causation (Council `t_0002`).
- Fetching the text of pointer sources. That is 142 §2.7's backfill (`delphi sources recheck --pointers`), run by the operator; this spec only stops the writer citing sources it cannot read.

### 1.2 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | keep the whole-page rule; fix the inputs (§2.1) and retry | the trial's pass rate per attempt and tokens per page delivered |
| B | constraint removed: "a page passes or fails whole" | number the page's units, require a verdict for each, remove what fails, fall back past fixed caps (§2.2 to §2.6) | the acceptance criteria with scripted validators; the trial |
| C | analogy: an editor's marked-up proof | show the whole draft with each unit's verdict; the operator accepts or strikes | a view the operator reads; it delivers no page without the operator's work |

**Chosen: A's input fix and B.** C's view is kept read-only as the rejected-draft view (§2.8). **The simpler alternative** (Council `t_0002`): trim whole units, no inference, fall back past the caps. That is what this revision is.

## 2. Design

### 2.1 Inputs: a source with no text is not citable

1. A source whose stored text is empty (a pointer, or a capture with no body) is not given to the writer or the validator as a `RECORD`. It is listed once under `SOURCES READ (titles only; not citable for what they say)`, with its title.
2. The page check rejects a marker that names such a source (`cited a source with no text`), as it rejects a marker naming a record not in the material.
3. Claims stay citable as today: a claim is its own text, at its own standing.

### 2.2 Units, numbered by the daemon

1. The daemon splits the page body into **units**: each run of sentences that ends at a citation marker (the marker's reach, 057 §3.1); each list item; each table row; the lead's units, flagged `lead`. Headings, the hatnote, figure blocks and the bibliography are structure, not units. A sentence after the last marker of a paragraph is a unit of its own (uncited).
2. The units are numbered `U1 … Un` in order. The page the validator reads shows each unit with its number, `⟦U12⟧`; the page stored never does.
3. The validator answers **one line per unit**: `UNIT U12 <verdict> | <record id or -> | <the reason, short>`, then `END ASSERTIONS`. The verdicts are today's: `supported`, `uncited`, `contradicts`, `fabricated`.

### 2.3 A verdict for every unit

1. **Coverage is checked by code.** Every unit must have a verdict. A unit with no verdict, an unknown verdict, or a reply without its end line means **the attempt has no verdict**: nothing is trimmed, and the attempt fails as today.
2. **Worst wins.** A unit given more than one verdict takes the worst: `contradicts` > `fabricated` > `uncited` > `supported`.
3. **No validator, no trimming.** When the validator cannot run, nothing is trimmed and the seed (or the claims-only page) stands.

### 2.4 What each verdict does

| Verdict | Against | The unit |
| :-- | :-- | :-- |
| `supported` | | kept |
| `uncited` | | kept, marked `(unchecked)` in place |
| `fabricated` | a record shown whole | **removed** |
| `fabricated` | a record shown in part, or not shown (left out for length) | kept, **its marker dropped**, marked `(unchecked)`: no marker may imply support the validator did not see |
| `contradicts` | a validated record, or a record the page cites | **removed** |
| `contradicts` | a noted record the page does not cite | kept; the pair goes under **Contested** (057 §3.3) |
| `contradicts` | a record the daemon cannot place | **blocks**: no trimming; the attempt fails |

### 2.5 The caps, and the checks after trimming

The page falls back (§2.7) when, after trimming, **any** of these holds:

1. removed units are more than **15%** of the units, or more than 15% of the words;
2. removed and `(unchecked)` units together are more than **40%** of the units, or of the words;
3. a `lead` unit was removed;
4. a heading is left with no unit under it, or a table with no row;
5. a figure's data, or a table's row, rests on a removed unit;
6. the trimmed page fails the page check (057 §3.2, 097) or the held-text check (142 B) when run again.

15% and 40% are first values, set here before the trial (§2.9), which may change them only by an amendment.

### 2.6 The best attempt

There are at most two attempts: the first draft and one rewrite (057 §3.4). Each is validated and trimmed as above. **The attempt with fewer removed units is kept**; a tie keeps the first draft. Both are logged (§2.8).

### 2.7 Fallback, by page kind

- **A topic page** falls back to its seed (139 §2.2). A seed never replaces a page that passed (139 C's rule stands).
- **A search page** has no seed: it falls back to 057's claims-only page, as today.

### 2.8 Outcomes, the banner, and the rejected drafts

1. **Outcomes, lowest wins:** `pass`; `pass_contested` (anything under Contested); `pass_trimmed` (any unit removed or marked); `host-reported` (142 §2.2, capped as today). A page carries the lowest that applies.
2. **A trimmed page counts as written** for 139's late-slice gate ("5 of the last 10 written pages pass"), and is counted separately there as trimmed.
3. **The banner** says how many units were removed and how many are unchecked, and says plainly: "Each statement kept was checked against its citation by a second model; that check is not proof." It never says a page is accurate.
4. **Rejected drafts** stay in the library's log only. They are never indexed, gathered or cited; control characters are stripped and labels kept. `delphi pages topic <slug> --rejected` prints the last one with each unit's verdict.

### 2.9 The trial, fixed before it runs

**The protocol is written in the tracker before the first topic is built**, and these are its terms:

1. **Inputs first:** the pointer backfill (142 §2.7) has run, and §2.1 is built.
2. **Sample:** 10 topics drawn at random from the topics with at least three sources holding text. The draw and its seed are recorded.
3. **Fixed:** the writer's and validator's models (Sonnet and Haiku today, through the bridge), the role packs and the brief, recorded by hash.
4. **Planted errors:** at least **30**, spread across the pages and through their length: a wrong number, a wrong attribution, an invented cause, a part widened to the whole, a reversed claim. **At most 10%** may pass.
5. **Kept text:** 20 kept units per topic, drawn at random, checked by hand against their records. **At most 10%** wrong or unsupported.
6. **Value:** the operator reads each written page against its seed. **At least 6 of 10** hold a correct, useful statement their seed lacks.
7. **The checker's share:** each removal is classified (writer overreach; citation reach; missing evidence; validator error; research its source does not support). **At most 25%** may be the checker's.
8. **Gates:** all of 4 to 7 must hold.
   - If 4 or 5 fails, **trimming is switched off**: pages pass or fail whole, as today.
   - If 6 fails, long pages stay `on_request` and short anchored summaries are considered instead.
9. **Cost:** tokens per page delivered, and per attempt, are recorded. Trimming does not shorten an attempt; it changes how many attempts deliver a page.

### 2.10 Long pages on request

1. **New key `[library.pages] long = "on_request" | "auto"`, default `on_request`.** With `on_request` the writer's expansion runs only when the operator asks (`delphi pages rebuild <slug> | --due | --all`, and the app's button when built). It is read in `job_may_run`'s switch for the pages job (143 §2.3), so it also holds for a stale-writer rebuild (139 §2.6).
2. **The default changes to `auto` only by an amendment** that cites a trial passing §2.9.
3. Long pages written before this spec stay as they are and are marked stale when their material changes, as today.

## 3. Authority

Nothing here widens authority. Removing a unit only takes text out; dropping a marker or marking a unit `(unchecked)` only lowers what it claims; no standing rises. Unit numbers live only in what the validator reads. The rejected-draft view reads the local log.

## 4. Slices

| Slice | Content | Criteria | Files | Hours |
| :-- | :-- | :-- | :-- | --: |
| S0 | §2.1 sources with no text are not citable | AC-1, AC-2 | `library_page_search.rs`, `library_page_check.rs`, `library_topics.rs` | 2-3 |
| A | §2.2 and §2.3 units, the unit contract, coverage | AC-3 to AC-5 | `library_page_validate.rs`, a new `page_units.rs` | 5-7 |
| B | §2.4 to §2.8 outcomes, caps, rechecks, the best attempt, fallback by kind, the banner, `--rejected` | AC-6 to AC-12 | `library_page_validate.rs`, `library_topics.rs`, `pages_cmd.rs` | 6-8 |
| C | §2.10 `long`, in `job_may_run` | AC-13 | `config.rs`, `job_admission.rs`, `page_run.rs` | 2-3 |
| D | §2.9 the trial, its protocol written first, its report | AC-14 | (a run; the tracker) | 3-4 and the operator's reading |
| | **Total** | | | **18-25** |

S0 does not wait for the rest: it is the input fix the live runs call for.

## 5. Acceptance criteria

1. **AC-1:** a source with an empty stored text is listed under `SOURCES READ` and is in no `RECORD` of the writer's brief or the validator's material.
2. **AC-2:** a draft citing such a source fails the page check with `cited a source with no text`.
3. **AC-3:** a page of paragraphs, list items and table rows is split into units as §2.2 says. A sentence after a paragraph's last marker is its own unit, and the lead's units are flagged.
4. **AC-4:** a validator reply that omits one unit, names an unknown unit or verdict, or lacks its end line trims nothing, and the attempt fails.
5. **AC-5:** a unit given `supported` and `fabricated` takes `fabricated`.
6. **AC-6:** each row of §2.4 gives its outcome. In particular, `fabricated` against a record shown in part keeps the unit with its marker dropped and `(unchecked)`.
7. **AC-7:** each cap of §2.5 sends the page to its fallback: 15% by units, 15% by words, 40% removed-plus-unchecked, a lead removed, an empty heading, and a figure on a removed unit.
8. **AC-8:** a trimmed page that fails the page check or the held-text check when run again falls back.
9. **AC-9:** of a draft with 1 removed unit and a rewrite with 6, the draft is kept.
10. **AC-10:** a search page past a cap becomes 057's claims-only page; a topic page becomes its seed; a page that passed is never replaced by a seed.
11. **AC-11:** a page with a removed unit and a Contested pair has outcome `pass_trimmed`, and its banner gives both counts and the sentence of §2.8 item 3.
12. **AC-12:** `delphi pages topic <slug> --rejected` prints the last rejected draft with each unit's verdict. No rejected draft is in the index.
13. **AC-13:** with `long = "on_request"`, neither the idle rebuild nor a stale-writer rebuild writes a long page; `delphi pages rebuild` does.
14. **AC-14:** the trial's protocol is dated in the tracker before its first build. Its report gives each measure of §2.9 against its threshold, and what the gates decide.

## 6. Verification plan

S0 to C are tested with scripted writers and validators, as 139 C's tests are. D is a live run on the operator's copy (`delphi-kv2` on `~/.delphi-kv2`), read by the operator.

## 7. What is not verified

- Whether removing whole units leaves readable prose. Whole units make it likelier, not certain. The trial reads each trimmed page against its draft.
- Whether 15% and 40% are right.
- Whether one validator's `supported` is right often enough. §2.9 items 4 and 5 measure it; the banner does not claim it.

## 8. Decisions for the operator

1. **The caps:** 15% removed, and 40% removed plus unchecked (the Council's candidates, adopted here), or other values.
2. **The trial's size:** 10 topics and 30 planted errors (the Council's floor), or larger.
3. **S0 first**, ahead of review of the rest (recommended: it is a fix the live runs call for, and it does not depend on trimming).

## 9. Review

- **Council `t_0002` on revision 1 (2026-11-27).** The heard seat was the architect (Claude, Sonnet). The strategist (Gemini) gave a first-round position but no readable second round. The red-team seat abstained: it was routed to a local model whose context could not hold the material. One family was heard, so the review does not satisfy GOVERNANCE §1.3.
- **Applied here.** Every finding of the heard seat:
  - numbered units, and a verdict for every unit with the worst winning (both P0);
  - dropped markers on partial evidence;
  - the checks run again after trimming;
  - inference deferred;
  - caps on unverified text;
  - the search pages' fallback;
  - a trial that gates trimming, with 30 planted errors at most 10% passing;
  - outcome precedence, `on_request` read in `job_may_run`, the missing criteria, the cost line, the simpler alternative, and a banner that does not imply accuracy.

  The strategist's first-round points are applied too: inference deferred, a cap on unverified content, the search-page fallback, and a tighter planted-error gate.
- **Added from the live runs:** §2.1 (sources with no text) and §2.6 (the best attempt).
- **Still needed.** A review by a second model family of this revision, then the operator's approval.
