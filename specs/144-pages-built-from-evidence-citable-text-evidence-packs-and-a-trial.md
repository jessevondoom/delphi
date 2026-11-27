---
id: spec-144-pages-built-from-evidence-citable-text-evidence-packs-and-a-trial
title: "144: Pages built from evidence: citable text only, research handed over as evidence packs, and a trial before defaults change"
author: architect
timestamp: 2026-11-28T02:00:00Z
status: draft   # revision 5. The operator approved the approach and the architect's choices on 2026-11-27 ("i agree with the [architect's] choices — accept them, including the change to 142B, and proceed. i'll approve all specs when ready"); not yet approved as text
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # built on seven live page attempts (2026-11-27), an audit of every failure, the architect's reviews (help t_0001, t_0006, t_0007, t_0009; two in the host), Council t_0002 on revision 1, and second-family reviews of revision 2 (ask t_0003 Gemini, t_0004 OpenAI); nothing in this spec is built
source_mission: 2026-11-25-knowledge-v2
tags: [spec, pages, research, validator, writer, retrieval, encyclopedia]
---

# Spec 144: Pages built from evidence

> **Status: draft, revision 5.** The operator approved the approach (2026-11-27: "i approve that approach") and the architect's choices ("i agree with the [architect's] choices — accept them, including the change to 142B, and proceed"). This text is not approved. Revision 5 applies the red-team's review of revision 4 (Gemini, help `t_0011`) and the operator's choice on held text (section 9).
> **Amends:**
> - [`specs/057`](057-search-pages-generated-and-checked.md) §3.1, §3.2 and §3.3, for topic pages only (one marker per statement; `unchecked`). Search pages are unchanged.
> - [`specs/139`](139-pages-seeded-from-research-a-seed-that-is-never-empty-a-long-depth-and-concept-titles.md) §2.1 (rows 1 and 6 of a research topic's seed), §2.3 item 3 (what `DRAFT TO EXPAND` is) and §2.5 item 3 (the default of `auto_rebuild`).
> - [`specs/117`](117-the-encyclopedia-in-the-desktop-app-topic-pages-navigation-and-a-view-of-agent-memory.md) §2 item 3, which takes the same default.
> - [`specs/143`](143-a-single-turn-harness-bridge-one-prompt-one-answer-through-an-allowed-harness.md) §2.8 item 4 (the default of `[bridge.jobs] pages`).
>
> **Corrects the build to** [`specs/142`](142-the-source-text-cache-immutable-captures-host-text-handover-and-anchors-that-show-their-orphans.md) §2.5 item 3: agents read held text.
> **Builds on:** 142 (captures, held text, the backfill of 142 G), [`specs/140`](140-a-golden-set-for-retrieval-recall-ndcg-a-baseline-and-a-gate-on-every-retrieval-change.md) (the golden set), 143 (the bridge).

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
2. **Most of what passed cites the project's own repository.** 238 of 283 passing statements cite repository files.
3. **The validator is not consistent.** Five title-level statements were judged `supported` in one attempt and `fabricated` in the next.
4. **An attempt costs about 77,000 tokens through the bridge.** The calls reported 4 to 10 fresh input tokens and 850 to 3,200 output tokens each; the rest is the CLI's prompt cache.

### 1.3 Non-goals

- **Trimming pages** (keeping what passes, removing what fails). It gets its own spec only if the trial (§2.8) shows the whole-page rule is the limit. Reviews of revision 2 found it needs claim-level verdicts and a recheck of what surviving citations claim.
- **Labelled inference.** Deferred.
- **Validating research claims before use.** The support check of §2.4, if it is built, is a check, not validation.
- **A new configuration key.** Existing switches govern unrequested page builds (§2.7 item 4).

### 1.4 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | fix what the writer and validator are given (§2.1 to §2.3) and keep the whole-page rule | arm A0 |
| B | constraint removed: "the page is the unit of judgement" | trim failing statements out of a page | simulated offline on the trial's saved drafts (§2.8 item 6) |
| C | analogy: Wikipedia's text–source integrity | each statement carries its own citation, a source is cited only for what the editor read in it, synthesis is not support | adopted into A: §2.1 to §2.3 |

**Chosen: A with C's rules.** The simpler alternative is A alone, with evidence packs but no support check. It is built first, and arm A0 tests it. The support check and arm A1 follow only if A0's hand checks call for them (§2.4).

## 2. Design

### 2.1 Only text can be cited

1. **Citable text.** A source is citable only when its stored text holds content. Code decides this by a fixed rule (`citable_text`): the capture's HTTP status was a success; the text is not empty; it is not only navigation, an error or "not found" page (a soft 404), a parked domain, a CAPTCHA, a paywall notice or a cookie banner; and its share of prose (sentences of ordinary text against links, menus and repeated lines) passes a fixed threshold. Each excluded source is logged with its reason. **A summary (142 §2.6) is not citable text:** a pointer source with a summary is listed under titles only.
2. **The rest are read, titles only.** A source without citable text is not a `RECORD` for the writer or the validator. It is listed once under `SOURCES READ (titles only; not citable for what they say)`. The page check rejects a marker naming it (`cited a source with no text`).
3. **Research prose is not evidence.** A research answer (`answer.md`, a run's answer; 139 §2.1 row 1) is an outline. For a page it is not a citable record. Its claim markers are followed back to the claims and their passages, and unmarked answer prose supports nothing. It stays in agents' search, labelled (§2.6).
4. **Research topics leave out the repository.**
   - **The rule:** for a topic created by research (`Origin::Research`), the seed's knowledge-base row (139 §2.1 row 6) leaves out every **repository file**: a file under a registered project's root, outside its `research/` folder.
   - **What stays:** a project's research files (row 5) stay.
   - **Topics the operator creates:** a topic the operator creates by hand (`delphi pages new`, `Origin::Operator`) keeps repository files. That is how the operator asks for a page about the project itself.

### 2.2 Each statement carries its own citation (topic pages)

1. **The closed rule (amends 057 §3.1 for topic pages):**
   - **What needs a marker:** every sentence of body prose, every list item and every table row carries at least one marker, placed after it. A marker covers only the sentence, item or row it ends.
   - **The only exceptions:** headings; a lead-in ending in ":" that states no fact (at most 12 words, no number, no proper name, no comparison: "The main approaches are:"); the `Contents` and `See also` sections; the hatnote.
   - **Not exceptions:** figure captions and `Key terms` lines carry markers (117 §2 item 5 already gives each key term one). A lead-in that states a fact is a sentence like any other.
   - **What the writer drops:** a sentence the writer would add with no record behind it (a gloss, a transition).
2. **The page check** rejects a sentence, item or row that the rule requires to carry a marker and that has none (`a statement with no citation`), and a lead-in that fails the test of item 1 (`a lead-in that states a fact`). Code applies this, with no model.
3. **What this changes in 057 for topic pages:**
   - 057 §3.2–3.3's "uncited but consistent" pass no longer applies to topic pages, and §8 decision 2 of this spec supersedes the operator's 057 words "not every sentence needs a citation" for topic pages.
   - Search pages keep 057 as it is.

### 2.3 What the validator sees, and what it must show

1. **The same evidence as the writer.** For each cited record, the validator gets at least every passage the writer's brief held from it, cut the same way. The material cap counts by records cited, and a cited record left out for length is named.
2. **The rule text,** given word for word in the writer's brief and the validator's contract:
   > Joining two or more records into a conclusion that none of them states is not support. Neither is a stronger wording than the record's ("may" made "does", "some" made "all"), a part made the whole, or a condition the record states left out.
3. **Pinpoint passages.**
   - For each `supported` verdict, the validator gives the shortest passage it relied on, and code checks that the passage is in the cited record's text as shown, **whole**: from the start of a sentence or clause to its end, not a fragment that cuts off a "not", a "no" or a condition next to it.
   - The passage check proves the words exist, not that they mean what the statement says; the validator's judgement and the trial's hand checks cover meaning. A statement that turns on a negation or a number is listed in the trial's hand-check sample first.
   - A passage that is not found makes the verdict `unchecked`.
   - **On a topic page, `unchecked` is a failure of that statement** (amends 057 §3.3 for topic pages), and the attempt follows 057 §3.4.
4. **The reply's end and size** are as the fixes of 2026-11-27 made them (`END ASSERTIONS`, sized to the page).

### 2.4 A support check after research (built only if called for)

1. **What it is:** a tool-less model checks each claim against its quote and the passage around it. It answers `supported`, `overstated`, `unsupported` or `can't tell`. It is a check, not validation: no status or trust rises. The result is kept beside the claim, tied to the hashes of the claim, the passage, the checker's model and its prompt, and claims are checked in batches per run.
2. **When it is built:** only if arm A0's hand checks (§2.8 item 5) attribute more than 2 kept wrong statements per 100 checked to research distortion (a claim saying more than its source).
3. **Its shadow run comes before a page relies on it:**
   - it checks stored claims with text, including about 30 planted distortions (a lost "not", a dropped condition, a widened scope, a changed number);
   - it also checks 50 claim–passage pairs labelled by hand;
   - it reports its miss rate and its false-rejection rate.

### 2.5 The evidence pack

1. **What it holds.** At the end of a research run (`done` or `partial`), the run writes its evidence pack: each claim with its quote, the stored passage around it, the source's text hash, and (when §2.4 is built) the support verdict. Code writes it; no model is called.
2. **Where it lives.** It is a Markdown file with frontmatter, in the research topic's folder, indexed as untrusted text, so an agent's retrieval returns a claim, its passage and its source together.
3. **Its label.** The pack carries the strongest mark of any source it quotes (141 §2.2): `library_private`, `text_held`, `personal`, `internal`, `confidential`.
4. **Its exits.**
   - The pack is on the list of senders of 142 B (`held_text::SENDERS`), and every exit checks it: `body_may_leave` and 141's label decision.
   - A pack of a private source never leaves.
5. **Packs for earlier runs.** `delphi research pack <topic-id> | --all` writes packs for runs that ended before this spec, from their records and the stored text.
6. **Queuing a page.** A run queues the topic's page job only on the conditions of §2.7 item 5, and only when the topic has evidence with citable text and page budget left (139 F).

### 2.6 What agents see

1. **Held text** (142 B), labelled `untrusted · not validated · held text`, by the span's tools:
   - **Default on** for a span that holds no mutating and no outbound tool: the reading roles (scout, strategist, architect, validator, investigator, writer, and any other role whose span holds none). Its default search returns held text, and held text may start the span.
   - **Opt-in** for a span that holds a mutating or outbound tool (builder, orchestrator, messenger, and any other such span): its default search leaves held text out; a search that asks for it (`include_held: true`) returns it.
   - **Why:** under GOVERNANCE §4.3 a span that receives `external_untrusted` text loses its mutating and outbound tools. Default-on for acting spans would disarm them on routine searches (red-team review, help `t_0011`). An acting span that opts in accepts that cost, and it is counted (§2.8 item 1).
   - **What doesn't change:** its taint stays `external_untrusted`; every exit check is unchanged; private sources stay out for every span.
2. **Research answers** stay in agents' search, labelled `research summary · not evidence`.
3. **Evidence packs** (§2.5) are in agents' search, with their labels.

### 2.7 Pages are built from evidence

1. **The writer's evidence.** A topic page is written by a fresh writer from the topic's evidence: its evidence packs, findings and knowledge-base passages, merged. It is never written by editing the previous page (139 §2.3 item 6 stands).
2. **The seed's role** (amends 139 §2.3 item 3). The writer still gets the seed as `DRAFT TO EXPAND`, **as an outline only:** its structure and its order. Its markers are re-pointed to the claims and passages they rest on. Text in the seed that rests on no claim or passage is not given to the writer as citable.
3. **The validator** checks the page against exactly the writer's evidence, plus the records that contradict it.
4. **A page carries the marks of its evidence.** A topic page's label (141 A, `Labeller::page`) is the join over everything it cites, evidence packs included: the strongest mark of any source any pack quotes. Every exit that carries the page checks it (141's label decision, `body_may_leave`).
5. **Builds nobody asked for** are governed by existing switches, with their defaults changed until a trial passes (§2.8 item 7):
   - **`[bridge.jobs] pages`:** default `manual` (amends 143 §2.8 item 4).
   - **`[library.pages] auto_rebuild`:** default `false` (amends 139 §2.5 item 3 and 117 §2 item 3).
   - **`build_after_research`:** stays `true`. It writes the seed, which costs no model call. The queued expansion runs only when one of the two switches above allows it.
   - **Which wins:** for a bridged build, both `[bridge.jobs] pages` and `auto_rebuild` must allow it; for an API build, `auto_rebuild` alone.
   - **What always runs:** a build the operator asks for (`delphi pages rebuild`).
6. **Unchanged:** the instant seed (139 §2.2); the page budget (139 F); a seed never replaces a page that passed; search pages fall back to 057's claims-only page.

### 2.8 The trial

**The protocol is dated in the tracker before the first build.**

1. **Order:**
   1. The golden set accepted (done, 2026-11-27) and its baseline run.
   2. Slices S0, A, B, C1 and D built.
   3. The golden set run again (slice B changes retrieval), with a count of spans that lost tools to held text.
   4. Research on 3 to 5 real questions through the bridge; packs written; `delphi research pack --all` for earlier runs.
   5. The pointer backfill (`delphi sources recheck --pointers`, 142 G).
   6. The golden set run again (the backfill changes retrieval), checking for passages that appear twice (pack and source).
   7. Arm A0.
   8. §2.4 and arm A1, only if §2.4 item 2 calls for them.
   9. Arm A2.
2. **Sample.**
   - Every topic with at least three sources holding citable text, up to 20; the trial runs only if at least 10 qualify.
   - The report says how many topics qualified out of all topics.
3. **Fixed:** the writer's and validator's models, the role packs and the brief, by hash.
4. **Arms:**
   - **A0:** §2.1 to §2.3, pages from evidence packs.
   - **A1:** A0, writing only from claims §2.4 found `supported`; on the same topics and evidence as A0.
   - **A2 (experimental):** the page written inside the research run, then checked by a fresh validator. It runs after A0 and A1, on topics researched fresh for it, and is reported apart.
5. **Measures, with floors:**
   - **Planted errors:** at least 30 per arm; at most 10% may pass.
   - **Kept statements:** 10 per topic per arm, drawn at random (statements that turn on a negation or a number first), and checked by a validator-role agent run in the host, on a model family other than the writer's. The operator audits a fifth of them. **At most 5%** may be wrong. Each wrong one is attributed: writer overreach, citation, or research distortion.
   - **Value:** the operator reads each page against its seed, for correctness and reading (titles, links, figures, bibliography). At least 60% of the topics hold something correct and useful that their seed lacks.
   - **Validator consistency:** each page is validated twice. More than 10% disagreement marks the quality results unreliable, and the validator's model or tier is revisited before any decision.
   - **Leaks:** private or rejected material, statements from outside the evidence, omitted conflicts. Any one fails the arm.
   - **Cost:** tokens per delivered page, counted in full (fresh input, cache writes, cache reads, output, retries, failed attempts), raw and cost-weighted. A2 is pursued only if it saves at least 25% cost-weighted at no worse quality.
   - **Budget:** the trial stops at 3,000,000 tokens through the bridge.
6. **Trimming, offline.** On the saved drafts of every arm, trimming is simulated by hand, and each trimmed result is read for changed meaning and citation reach.
7. **Decides:**
   - **A1 over A0** only if A1 meets every floor and delivers at least as many pages that pass the floors.
   - **No arm meets the floors:** unrequested builds stay off (§2.7 item 5), and the trimming spec of §1.3 is reconsidered.
   - **An arm meets the floors:** the operator may approve an amendment citing the trial that returns `[bridge.jobs] pages` and `auto_rebuild` to their earlier defaults, and that may replace 139's late-slice trigger ("5 of the last 10 written pages pass") with the trial's result. Nothing changes a default without that approval.

### 2.9 Standing checks after the trial

1. **Spot checks.** For each batch of written pages, a sample of statements is checked as in §2.8 item 5. The sample size, the error limit and the consequence (unrequested builds go back off) are set in the trial's report.
2. **The golden set** runs on every retrieval change, as 140 F requires.

## 3. Authority

- **Agents see more; nothing leaves.** Reading spans see held text by default and acting spans only on request; every exit is unchanged. A span that reads held text loses its mutating and outbound tools (§2.6 item 1).
- **A page carries the marks of its evidence** and is checked at every exit (§2.7 item 4).
- **Evidence packs carry the marks of what they quote** and are checked at every exit (§2.5 items 3 and 4).
- **The support check raises nothing.**
- **The page rules only narrow what may be cited.**
- **Unrequested builds are narrowed:** their defaults move to off.

## 4. Slices

| Slice | Content | Criteria | Hours |
| :-- | :-- | :-- | --: |
| S0 | §2.1: citable text, titles only, research prose not evidence, research topics without the repository | AC-1 to AC-4 | 3-4 |
| A | §2.2 the closed marker rule and the lead-in test (topic pages); §2.3 matched evidence, the rule text, whole pinpoint passages, `unchecked` | AC-5 to AC-8 | 5-7 |
| B | §2.6: held text by the span's tools (built on `knowledge-v2` 2026-11-27 and checked here against 142 §2.5 item 3); answers and packs labelled | AC-9, AC-10 | 2-3 |
| C1 | §2.5: the evidence pack, its label and exits, `research pack` for earlier runs | AC-11, AC-12 | 4-6 |
| C2 | §2.4: the support check, if §2.4 item 2 calls for it | AC-13 | 4-5 |
| D | §2.7: pages from merged evidence; the seed as outline; a page's label from its evidence; the defaults | AC-14, AC-15, AC-17 | 4-5 |
| E | §2.8: the trial, protocol first, arms, report | AC-16 | 4-6 and the operator's reading |
| | **Total** (C2 included) | | **26-36** |

## 5. Acceptance criteria

1. **AC-1:** a source whose stored text is empty or only boilerplate by `citable_text`'s rule (a soft 404, a parked domain and a CAPTCHA page among the fixtures), or a pointer with only a summary, is in no `RECORD`. It is listed under `SOURCES READ`, and its exclusion is logged with the reason.
2. **AC-2:** a topic-page draft citing such a source fails the page check with `cited a source with no text`.
3. **AC-3:** a research answer is not a citable record for a page; its claim markers reach the claims' passages; unmarked answer prose is cited nowhere.
4. **AC-4:** a research topic's seed draws no row-6 passage from a repository file (under a registered project's root, outside `research/`). It keeps row 5. A topic the operator creates keeps repository files.
5. **AC-5:** on a topic page, a body sentence, list item, table row, figure caption or key-term line with no marker of its own fails the page check; a lead-in that states a fact ("Benchmarks show X is twice as fast as Y:") fails; each exception in §2.2 item 1 passes without one; a marker covers only what it ends. A search page is checked as before.
6. **AC-6:** for every cited record, the validator's material holds every passage the writer's brief held from it; a cited record left out for length is named.
7. **AC-7:** the writer's brief and the validator's contract both contain §2.3 item 2's rule text verbatim, and a scripted test judges a planted strengthened statement not supported.
8. **AC-8:** a `supported` verdict whose pinpoint passage is not in the shown record, or is a fragment that cuts off an adjacent "not" or condition, becomes `unchecked`, and on a topic page that statement fails.
9. **AC-9:**
   - a reading span's default search returns a held source's passage, labelled, with its taint unchanged;
   - an acting span's default search does not; its search with `include_held: true` does, and the span then loses its mutating and outbound tools;
   - a private source is returned to no span;
   - every exit test passes unchanged.
10. **AC-10:** research answers and evidence packs are returned by an agent's search, each with its label.
11. **AC-11:** a finished run writes an evidence pack with frontmatter, indexed, carrying the strongest mark of the sources it quotes. `research pack --all` writes packs for earlier runs.
12. **AC-12:** a pack is listed in `held_text::SENDERS`. A bundle, export or push that carries a pack quoting a held source past `body_may_leave`, or a private source at all, is refused.
13. **AC-13** (if C2 is built): the support check gives each claim one of its four answers, tied to the four hashes, raises no status, and reports its miss and false-rejection rates from §2.4 item 3 before a page relies on it.
14. **AC-14:** a topic page is written from merged evidence, never from the previous page's text. The seed reaches the writer as an outline with its markers re-pointed. The validator gets the same evidence and the contradicting records.
15. **AC-15:** with the new defaults:
    - no idle or stale-writer rebuild, and no build queued by research, writes a long page;
    - `delphi pages rebuild` does;
    - the seed is still written after research.
16. **AC-16:** the trial's protocol is dated before its first build. Its report gives every measure of §2.8 item 5 per arm, the offline trimming result, and each decision of §2.8 item 7.
17. **AC-17:** a topic page whose evidence packs quote a held or private source carries that mark (141 A's page label), and a bundle, export or push that carries the page is refused as the mark requires.

## 6. Verification plan

Slices S0 to D are tested with scripted writers, validators and checkers, as 139 C's tests are. E is a live run on the operator's copy (`delphi-kv2` on `~/.delphi-kv2`), read by the operator.

## 7. What is not verified

- Whether `citable_text`'s rule separates content from boilerplate well. Its exclusions are logged, to be read.
- Whether the closed marker rule makes pages choppy. The trial's reading measures it.
- How often held text takes a span's tools away. The golden-set run after slice B counts it.
- Whether agents' retrieval gets better or worse with held text visible and packs indexed. The golden set runs after each change. If it gets worse, the labelling and duplicate passages are revisited, not the visibility.

## 8. Decisions for the operator

Decided on 2026-11-27 ("i agree with the [architect's] choices — accept them, including the change to 142B, and proceed"):

1. Agents see held text (§2.6): by default for reading spans, on request for acting spans (operator, 2026-11-27: "use (b) as th default").
2. One marker per statement on topic pages (§2.2). This supersedes 057's "not every sentence needs a citation" for topic pages.
3. Research topics draw no repository files (§2.1 item 4).
4. Evidence packs, and the trial's order (§2.5, §2.8).

Proposed here, from the review of revision 3:

5. Unrequested page builds default to off until a trial passes, through the existing switches; no new key (§2.7 item 5).
6. The support check is built only if A0's hand checks call for it (§2.4 item 2).
7. The trial's floors and budget (§2.8 items 5 and 7): kept statements at most 5% wrong; at least 10 qualifying topics; no default changes without the operator's approval.

## 9. Review

- **Revision 1:** Council `t_0002`. One family was heard (the architect seat, Claude).
- **Revision 2:**
  - Gemini (`gemini-3.8-flash-medium`, ask `t_0003`): approve with changes.
  - OpenAI (`gpt-6.2-sol`, ask `t_0004`): reject.
  - Their findings moved trimming out.
- **Revision 3:**
  - Basis: the audit of 41 failures; the architect's reviews (help `t_0006`, `t_0007`, `t_0009`, and one in the host); and web research into Wikipedia's verifiability and text–source integrity practice, Cochrane's GRADE and the medical sourcing guideline (not validated).
  - Reviewed by the architect, in the host: approve with named changes.
  - **Every finding is applied here:**
    - the evidence pack's label and exits (P0);
    - no new key, defaults through existing switches;
    - the closed marker rule;
    - the complete amendments to 057, 139, 117 and 143, and the correction to 142;
    - the trial's order with C1 and D built first, the pack command for earlier runs, and the backfill through 142 G;
    - the rules for choosing arms;
    - A2 after A0 and A1;
    - the support check only if called for;
    - the measures as fractions, a consistency threshold, who checks, a budget;
    - golden-set runs after each change;
    - the taint cost of held text;
    - the summary's status;
    - the rule text;
    - the file renamed to match the title.
- **Revision 4:** red-team (Ares) in Delphi on Gemini (`gemini-3.8-flash-medium`, help `t_0011`; a first attempt, `t_0010`, ended empty): approve with named changes.
  - **Applied:** lead-ins may state no fact, and captions and key terms carry markers (P0); a page carries its evidence's marks (P0); held text by the span's tools (P1, with the operator's choice (b)); kept-statement floor 5%, at least 10 topics, no default change without the operator (P1); soft 404s, parked domains and the HTTP status in `citable_text` (P2); whole-passage matching, with negations and numbers checked first (P2).
  - **Not applied as asked:** a 3% floor and 20 topics (on about 100 hand-checked statements 3% is noise, and 20 topics may not qualify; 5% and at least 10 are used).
- **Still needed:** an independent review of this revision by a second model family (a committed Council or Panel record cited in `reviewed_by`), or the operator's waiver quoted in the status line. Then the operator's approval.
