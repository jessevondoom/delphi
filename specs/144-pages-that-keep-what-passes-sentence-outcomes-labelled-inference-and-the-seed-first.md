---
id: spec-144-pages-that-keep-what-passes-sentence-outcomes-labelled-inference-and-the-seed-first
title: "144: Pages built from evidence: citable text only, research handed over as evidence packs, and a trial before defaults change"
author: architect
timestamp: 2026-11-27T23:30:00Z
status: draft   # revision 3. The operator approved the approach and the architect's choices on 2026-11-27 ("i agree with the [architect's] choices — accept them, including the change to 142B, and proceed. i'll approve all specs when ready"); not yet approved as text
source_type: internal_generated
confidence: medium   # built on seven live page attempts (2026-11-27), an audit of every failure, the architect's reviews (help t_0001, t_0006, t_0007, t_0009, and one in the host), Council t_0002 on revision 1, and second-family reviews of revision 2 (ask t_0003 Gemini, t_0004 OpenAI); nothing in this spec is built
source_mission: 2026-11-25-knowledge-v2
tags: [spec, pages, research, validator, writer, retrieval, encyclopedia]
---

# Spec 144: Pages built from evidence

> **Status: draft, revision 3.** The operator approved the approach (2026-11-27: "i approve that approach") and the architect's choices ("i agree with the [architect's] choices — accept them, including the change to 142B, and proceed"). This text is not approved. Revision 2's page trimming is moved out to a later spec, used only if the trial of §2.8 calls for it.
> **Amends:** [`specs/057`](057-search-pages-generated-and-checked.md) §3.1 (one marker's reach) and §3 (what may be cited); [`specs/139`](139-pages-seeded-from-research-a-seed-that-is-never-empty-a-long-depth-and-concept-titles.md) §2.1 (what a seed may draw on) and §2.5 item 1 (the writer's expansion at idle); [`specs/097`](097-encyclopedia-search-pages.md) §4 (the validator's reply). **Builds on:** [`specs/142`](142-the-source-text-cache-immutable-captures-host-text-handover-and-anchors-that-show-their-orphans.md) (captures, held text, the pointer backfill), [`specs/140`](140-a-golden-set-for-retrieval-recall-ndcg-a-baseline-and-a-gate-on-every-retrieval-change.md) (the golden set), [`specs/143`](143-a-single-turn-harness-bridge-one-prompt-one-answer-through-an-allowed-harness.md) (the bridge).

## 1. Why

### 1.1 The goal this serves

One central store that serves two readers. **The operator** reads encyclopedia pages: prose, links, figures, a bibliography. **Agents** read Markdown with rich frontmatter, through retrieval strong enough to rely on. The store keeps local copies of its sources, serves as long-term memory, and improves over time (the knowledge-v2 handoff, §1). Every choice below is judged against both readers.

### 1.2 The evidence (2026-11-27)

Seven page attempts through the bridge, on a copy of the operator's install, five topics. One page was written; four topics kept their seed. An audit of every failing statement (41 in all):

| Cause | Failures | What it was |
| :-- | --: | :-- |
| Missing evidence | 34 | statements about sources with no text (pointers): "the SDK documentation has a handoffs page" |
| Citation reach | 6 | the writer's unmarked glosses, and list items under a lead-in's marker, charged to the next marker |
| Writer overreach | 1 | a phrase the record did not say |
| Validator error, or research its source does not support | 0 | |

Four other facts:

1. **The store holds almost no research.** It has 7 claims, 2 with a quote anchor, none with source text. 130 of 131 sources are pointers. The research runs on the operator's release had no model for the scout.
2. **Most of what passed cites the project's own repository.** 238 of 283 passing statements cite repository files (skills, governance, context documents), not research.
3. **The validator is not consistent.** Five title-level statements were judged `supported` in one attempt and `fabricated` in the next.
4. **An attempt costs about 77,000 tokens through the bridge.** The calls reported 4 to 10 fresh input tokens and 850 to 3,200 output tokens each; the rest is the CLI's prompt cache.

### 1.3 Non-goals

- **Trimming pages** (keeping what passes, removing what fails). It moves to its own spec, written only if the trial (§2.8) shows the whole-page rule is the limit. Reviews of revision 2 found it needs claim-level verdicts and a recheck of what surviving citations claim; it is not worth building before the inputs are fixed.
- **Labelled inference.** Deferred.
- **Writing the page inside the research run.** It is tested as an experimental arm (§2.8), not built as the path. The architect's review: stored text serves as well as text in a live context; the token saving is unproven, since most of the cost is already cached; and a scout's context carries rejected and private material a page must not draw on.
- **Validating research claims before use.** The support check of §2.4 is a check, not validation: no status or trust rises.

### 1.4 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | fix what the writer and validator are given (§2.1 to §2.3) and keep the whole-page rule | the trial's arm A0 |
| B | constraint removed: "the page is the unit of judgement" | trim failing statements out of a page (revision 2) | moved to its own spec; simulated offline on the trial's saved drafts (§2.8 item 6) |
| C | analogy: Wikipedia's text–source integrity | each statement carries its own citation, a source is cited only for what the editor read in it, and synthesis is not support | adopted into A: §2.1 to §2.3 |

**Chosen: A with C's rules.** The simpler alternative is A alone, without the support check of §2.4. It is the trial's arm A0.

## 2. Design

### 2.1 Only text can be cited

1. **Citable text.** A source is citable only when its stored text holds content: not empty, and not only navigation, an error page, a paywall notice or a cookie banner. Code decides it by a fixed rule (`citable_text`), and logs each source it excludes, with the reason.
2. **The rest are read, titles only.** A source without citable text is not a `RECORD` for the writer or the validator. It is listed once under `SOURCES READ (titles only; not citable for what they say)`. The page check rejects a marker naming it (`cited a source with no text`).
3. **Research prose is not evidence.** A research answer (`answer.md`, a run's answer) is an outline. For a page it is not a citable record: its claim markers are followed back to the claims and their passages, and unmarked answer prose supports nothing. It stays in agents' search, labelled as a research summary (§2.6).
4. **Topic seeds draw on research, not on the repository.** For a topic made by research, a seed's knowledge-base rows (139 §2.1) leave out the registered projects' own repository files (specs, skills, governance, context documents). They stay in search for agents and in pages the operator asks for about the project itself.

### 2.2 Each statement carries its own citation

1. **One marker per statement.** The writer puts a marker after each sentence and each list item or table row that states a fact. A marker no longer reaches back over earlier sentences (amends 057 §3.1). A sentence the writer adds with no record behind it (a gloss, a transition) states no fact, or it is left out.
2. **The page check** rejects a fact-stating sentence or list item with no marker of its own (`a statement with no citation`). The validator judges each marked statement against its own marker's record.

### 2.3 What the validator sees, and what it must show

1. **The same evidence as the writer.** For each cited record, the validator is given at least everything the writer was given from it: the same passages, not a different cut. The material cap counts by records cited, and a cited record left out for length is named.
2. **No synthesis, no strengthening.** The writer's brief and the validator's contract both say that joining records into a conclusion none of them states is not support, and neither is a stronger wording than the record's ("may" made "does"), a part made the whole, or a condition dropped.
3. **Pinpoint passages.** For each `supported` verdict the validator gives the shortest passage it relied on; code checks that the passage is in the cited record's text as shown. A passage that is not found makes the verdict `unchecked`.
4. **The reply's end and size** are as the fixes of 2026-11-27 made them (`END ASSERTIONS`, sized to the page).

### 2.4 A support check after research

1. After a research run captures its sources, a tool-less model checks each claim against its quote and the passage around it, given both. It answers `supported`, `overstated`, `unsupported` or `can't tell`.
2. It is a check, not validation. No claim's status or trust rises; the result is kept beside the claim, tied to the hashes of the claim, the passage, the checker's model and its prompt.
3. Claims are checked in batches per run, within the run's budget.
4. **It runs first in shadow:** on stored claims that have text, with about 30 planted distortions (a lost "not", a dropped condition, a widened scope, a changed number), before a page relies on it.

### 2.5 The evidence pack

1. At the end of a research run (`done` or `partial`), the run writes its **evidence pack**: each claim with its quote, the stored passage around it, the support verdict of §2.4, and the source's text hash.
2. **It is a Markdown file with frontmatter**, in the research topic's folder, indexed as untrusted text, so an agent's retrieval returns the claim, its passage and its verdict together.
3. The run queues the topic's page job only when the operator asked, or `[library.pages] long = "auto"`, and the topic has evidence with citable text and page budget left (139 F).

### 2.6 What agents see

1. **Held text** (142 B) is returned by an agent's default search and may start a span, labelled `untrusted · not validated · held text`. Its taint stays `external_untrusted`, and every exit check (`body_may_leave`) is unchanged. This brings 142 B into line with 142 §2.5 item 3 and the operator's ruling that every configured model route may see the whole knowledge base (operator, 2026-11-27).
2. **Research answers** stay in agents' search, labelled `research summary · not evidence`.
3. **Evidence packs** (§2.5) are in agents' search.

### 2.7 Pages are built from evidence

1. A topic page is written by a fresh writer from the topic's evidence: its evidence packs, findings, and knowledge-base passages, merged. It is never written by editing the previous page (139 §2.3 item 6 stands).
2. The validator checks the page against exactly that evidence, plus the records that contradict it.
3. **Unchanged:**
   - the instant seed (139 §2.2);
   - the page budget (139 F);
   - a seed never replaces a page that passed;
   - search pages fall back to 057's claims-only page;
   - `[library.pages] long = "on_request" | "auto"`, default `on_request`, read in `job_may_run` for every page build, a stale-writer rebuild and a build queued by research included.

### 2.8 The trial

**The protocol is dated in the tracker before the first build.**

1. **Order:**
   1. The golden set accepted (done, 2026-11-27) and its baseline run.
   2. §2.1 to §2.3 and §2.6 built.
   3. Research on 3 to 5 real questions through the bridge, so the store holds claims with text.
   4. The pointer backfill (142 §2.7).
   5. The golden set run again, so the backfill's effect on retrieval is measured.
   6. §2.4 in shadow.
   7. The trial.
2. **Sample:** the topics with at least three sources holding citable text, all of them up to 10. The report says how many topics qualified out of all topics.
3. **Fixed:** the writer's and validator's models, the role packs and the brief, by hash.
4. **Arms**, on the same topics and the same evidence:
   - **A0:** §2.1 to §2.3;
   - **A1:** A0, writing only from claims §2.4 found `supported`, plus their passages;
   - **A2 (experimental):** the page written inside the research run, then checked by a fresh validator. It needs fresh runs, all arms starting from the same frozen evidence.
5. **Measures:**
   - **Planted errors:** at least 30 per arm; at most 10% may pass.
   - **Kept statements:** 20 per topic checked by hand; at most 10% wrong.
   - **Value:** the operator reads each page against its seed, for correctness and for reading (titles, links, figures, bibliography). At least 6 of 10 hold something correct and useful that their seed lacks.
   - **Validator consistency:** each page validated twice; disagreements are counted.
   - **Leaks:** checks for private or rejected material, statements from outside the evidence, and omitted conflicts.
   - **Cost:** tokens in full per delivered page (fresh input, cache writes, cache reads, output, retries, failed attempts), raw and cost-weighted. A2 is kept only if it saves at least 25% cost-weighted at no worse quality.
6. **Trimming, offline:** on the saved drafts of every arm, trimming is simulated by hand, and each trimmed result read for changed meaning and citation reach. If the whole-page rule is the main limit on delivery, a trimming spec is written then.
7. **Decides:** whether A1 becomes the page path, whether A2 is pursued, whether `long` may default to `auto` (only by an amendment citing a passing trial), and whether the late slices' trigger (139, "5 of the last 10 written pages pass") is replaced by the trial's result.

### 2.9 Standing checks after the trial

1. **Spot checks.** For each batch of written pages, a sample of statements is checked by hand. The sample size, the error limit and the consequence (long pages return to `on_request`) are set in the trial's report.
2. **The golden set** runs on every retrieval change, as 140 F requires.

## 3. Authority

- **Agents see more; nothing leaves.** Agents' search now includes held text, but `body_may_leave` and every exit are unchanged. Taint is unchanged, so a span that reads held text is as untrusted as it was.
- **The support check raises nothing.** Its verdicts are kept beside claims and change no status.
- **Nothing becomes more citable.** The page rules (§2.1 to §2.3) only narrow what may be cited.

## 4. Slices

| Slice | Content | Criteria | Hours |
| :-- | :-- | :-- | --: |
| S0 | §2.1 citable text, titles only, research prose not evidence, seeds without the repository | AC-1 to AC-4 | 3-4 |
| A | §2.2 one marker per statement; §2.3 matched evidence, no synthesis, pinpoint passages | AC-5 to AC-8 | 5-7 |
| B | §2.6 what agents see (held text built on `knowledge-v2` 2026-11-27 under 142 B; answers and packs labelled) | AC-9, AC-10 | 2-3 |
| C | §2.4 the support check, in shadow first; §2.5 the evidence pack | AC-11 to AC-13 | 6-8 |
| D | §2.7 pages from merged evidence; `long` in `job_may_run` | AC-14, AC-15 | 4-5 |
| E | §2.8 the trial: protocol first, arms, report | AC-16 | 4-6 and the operator's reading |
| | **Total** | | **24-33** |

## 5. Acceptance criteria

1. **AC-1:** a source whose stored text is empty, or only boilerplate by `citable_text`'s rule, is in no `RECORD`, is listed under `SOURCES READ`, and its exclusion is logged with the reason.
2. **AC-2:** a draft citing such a source fails the page check with `cited a source with no text`.
3. **AC-3:** a research answer is not a citable record for a page; its claim markers reach the claims' passages; unmarked answer prose is cited nowhere.
4. **AC-4:** a research topic's seed draws no row from a registered project's repository files; a page the operator asks for about the project may.
5. **AC-5:** a fact-stating sentence or list item with no marker of its own fails the page check; a marker covers only its own statement.
6. **AC-6:** for every cited record, the validator's material holds every passage the writer's brief held from it; a cited record left out for length is named.
7. **AC-7:** the writer's brief and the validator's contract both state the no-synthesis and no-strengthening rules, word for word, and a planted strengthened statement is judged not supported in a scripted test.
8. **AC-8:** a `supported` verdict whose pinpoint passage is not in the shown record becomes `unchecked`.
9. **AC-9:** an agent's default search returns a held source's passage, labelled; its taint is unchanged; a private source is still not returned; every exit test passes unchanged.
10. **AC-10:** research answers and evidence packs are returned by an agent's search, each with its label.
11. **AC-11:** the support check gives each claim one of its four answers, tied to the four hashes, and raises no status.
12. **AC-12:** in shadow, the planted distortions and the hand-labelled pairs give the checker's miss rate and false-rejection rate, reported before any page relies on it.
13. **AC-13:** a finished run writes an evidence pack with frontmatter, indexed, and queues a page job only on the conditions of §2.5 item 3.
14. **AC-14:** a topic page is written from merged evidence, never from the previous page's text; the validator gets the same evidence and the contradicting records.
15. **AC-15:** with `long = "on_request"`, no page build runs on its own: not the idle rebuild, not a stale-writer rebuild, and not a build queued by research.
16. **AC-16:** the trial's protocol is dated before its first build, and its report gives every measure of §2.8 item 5 per arm, the offline trimming result, and what each decision of §2.8 item 7 is.

## 6. Verification plan

Slices S0 to D are tested with scripted writers, validators and checkers, as 139 C's tests are. E is a live run on the operator's copy (`delphi-kv2` on `~/.delphi-kv2`), read by the operator.

## 7. What is not verified

- Whether `citable_text`'s rule separates content from boilerplate well. Its exclusions are logged, to be read.
- Whether one marker per statement makes pages choppy. The trial's reading measures it.
- The support checker's accuracy, until §2.4's shadow run.
- Whether agents' retrieval gets better or worse with held text visible. The golden set runs before and after the backfill; if it gets worse, the labelling is revisited, not the visibility.

## 8. Decisions for the operator

All are recorded as decided, 2026-11-27, by "i agree with the [architect's] choices — accept them, including the change to 142B, and proceed":

1. Agents see held text (§2.6).
2. One marker per statement (§2.2).
3. Research seeds draw no repository files (§2.1 item 4).
4. The evidence pack and its order (§2.5, §2.8).

Still open: the trial's size (10 topics, 30 planted errors per arm, the reviewers' floors) and the cost-saving bar for A2 (25%).

## 9. Review

- **Revision 1:** Council `t_0002`. One family was heard (the architect seat, Claude). Its findings led to revision 2.
- **Revision 2:**
  - Gemini (`gemini-3.8-flash-medium`, ask `t_0003`): approve with changes.
  - OpenAI (`gpt-6.2-sol`, ask `t_0004`): reject. One unit can hold several claims, and trimming can change what a surviving citation appears to support.
  - Their findings are why trimming leaves this spec, and why the trial measures trimming offline before anything is built.
- **Revision 3** follows from:
  - the audit of the 41 failures;
  - the architect's reviews in Delphi (help `t_0006`, `t_0007`, `t_0009`) and one in the host;
  - research into Wikipedia's verifiability, no-original-research and text–source integrity practice, Cochrane's GRADE, and the medical sourcing guideline. These were web searches, not validated; they are the basis of §2.1 to §2.3.
- **Still needed:** an independent review of this revision by a second model family, or the operator's waiver. Then the operator's approval.
