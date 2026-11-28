---
id: spec-144-pages-built-from-evidence-citable-text-evidence-packs-and-a-trial
title: "144: Pages built from evidence: citable text only, research handed over as evidence packs, and a trial before defaults change"
author: architect
timestamp: 2026-11-28T06:00:00Z
status: draft   # revision 7. The operator approved the approach and the architect's choices on 2026-11-27 ("i agree with the [architect's] choices — accept them, including the change to 142B, and proceed. i'll approve all specs when ready"); not yet approved as text
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # built on seven live page attempts (2026-11-27), an audit of every failure, the architect's reviews (help t_0001, t_0006, t_0007, t_0009; two in the host), Council t_0002 on revision 1, and second-family reviews of revision 2 (ask t_0003 Gemini, t_0004 OpenAI); nothing in this spec is built
source_mission: 2026-11-25-knowledge-v2
tags: [spec, pages, research, validator, writer, retrieval, encyclopedia]
---

# Spec 144: Pages built from evidence

> **Status: draft, revision 7.** The operator approved the approach (2026-11-27: "i approve that approach") and the architect's choices ("i agree with the [architect's] choices — accept them, including the change to 142B, and proceed"). This text is not approved. Revision 7 applies the red-team recheck of revision 6 (OpenAI, ask `t_0015`); revision 6 applied Council `t_0012` on revision 5, aligns with [`specs/145`](145-taint-gates-what-leaves-local-writes-after-untrusted-text-proceed-marked.md) (taint gates what leaves), and records the operator's ruling that model calls are not exits (section 9).
> **Amends:**
> - [`specs/057`](057-search-pages-generated-and-checked.md) §3.1, §3.2 and §3.3, for topic pages only (one marker per statement; `unchecked`). Search pages are unchanged.
> - [`specs/139`](139-pages-seeded-from-research-a-seed-that-is-never-empty-a-long-depth-and-concept-titles.md) §2.1 (rows 1 and 6 of a research topic's seed), §2.3 item 3 (what `DRAFT TO EXPAND` is) and §2.5 item 3 (the default of `auto_rebuild`).
> - [`specs/117`](117-the-encyclopedia-in-the-desktop-app-topic-pages-navigation-and-a-view-of-agent-memory.md) §2 item 3, which takes the same default.
> - [`specs/143`](143-a-single-turn-harness-bridge-one-prompt-one-answer-through-an-allowed-harness.md) §2.8 item 4 (the default of `[bridge.jobs] pages`).
>
> **Corrects the build to** [`specs/142`](142-the-source-text-cache-immutable-captures-host-text-handover-and-anchors-that-show-their-orphans.md) §2.5 item 3: agents read held text.
> **Builds on:** 145 (held text no longer takes tools away), 142 (captures, held text, the backfill of 142 G), [`specs/140`](140-a-golden-set-for-retrieval-recall-ndcg-a-baseline-and-a-gate-on-every-retrieval-change.md) (the golden set), 143 (the bridge).

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
- **A new configuration key.** Existing switches govern unrequested page builds (§2.7 item 5).

### 1.4 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | fix what the writer and validator are given (§2.1 to §2.3) and keep the whole-page rule | arm A0 |
| B | constraint removed: "the page is the unit of judgement" | trim failing statements out of a page | simulated offline on the trial's saved drafts (§2.8 item 6) |
| C | analogy: Wikipedia's text–source integrity | each statement carries its own citation, a source is cited only for what the editor read in it, synthesis is not support | adopted into A: §2.1 to §2.3 |

**Chosen: A with C's rules.** The simpler alternative is A alone, with evidence packs but no support check. It is built first, and arm A0 tests it. The support check and arm A1 follow only if A0's hand checks call for them (§2.4).

## 2. Design

### 2.1 Only text can be cited

1. **Citable text.** A source is citable only when its stored text holds content. Code decides this by a fixed rule (`citable_text`): for a web capture, its HTTP status was a success; the text is not empty; it is not only navigation, an error or "not found" page (a soft 404), a parked domain, a CAPTCHA, a paywall notice or a cookie banner; and its share of content (sentences of ordinary text and table rows, against links, menus and repeated lines) passes a fixed threshold. A local file or a table of figures is judged by the same content share, without the HTTP test. Each excluded source is logged with its reason. **A summary (142 §2.6) is not citable text:** a pointer source with a summary is listed under titles only.
2. **The rest are read, titles only.** A source without citable text is not a `RECORD` for the writer or the validator. It is listed once under `SOURCES READ (titles only; not citable for what they say)`. The page check rejects a marker naming it (`cited a source with no text`).
3. **Research prose is not evidence.** A research answer (`answer.md`, a run's answer; 139 §2.1 row 1) is an outline. For a page it is not a citable record. Its claim markers are followed back to the claims and their passages, and unmarked answer prose supports nothing. It stays in agents' search, labelled (§2.6).
4. **Research topics leave out the repository.**
   - **The rule:** for a topic created by research (`Origin::Research`), no page evidence comes from a **repository file**: a file under a registered project's root, outside its `research/` folder. This covers the seed's knowledge-base row (139 §2.1 row 6), the writer's merged evidence (§2.7 item 1) and the validator's material.
   - **What stays:** a project's research files (row 5) stay.
   - **Topics the operator creates:** a topic the operator creates by hand (`delphi pages new`, `Origin::Operator`) keeps repository files. That is how the operator asks for a page about the project itself.

### 2.2 Each statement carries its own citation (topic pages)

1. **The closed rule (amends 057 §3.1 for topic pages):**
   - **What needs a marker:** every sentence of body prose, every list item, every table row, every figure caption and every `Key terms` line carries at least one marker, placed after it. A marker covers only the sentence, item or row it ends.
   - **What carries no marker, by a code test:**
     - headings;
     - the `Contents` and `See also` sections, which code writes;
     - the hatnote, which code writes from the topic's title and its links;
     - a lead-in ending in ":" that matches one of a fixed list of phrases kept in code ("The main approaches are:", "It has these parts:" and the like).
   - **What else:** any other lead-in is a sentence and needs a marker.
   - **One parsed form:** code parses a draft once into units, and the same parsed form is what the page renders and what the validator judges. A construct the parser cannot place in a unit class (raw HTML, an image with a descriptive text, a footnote that states a fact) is rejected (`a construct the page check cannot read`).
   - **Headings are still checked:** each heading is a unit the validator judges (item 2 of §2.3) against its section's records, so a heading that states a fact the section does not support fails like a sentence.
   - **What the writer drops:** a sentence it would add with no record behind it (a gloss, a transition).
2. **The page check** rejects:
   - a unit the rule requires to carry a marker and that has none (`a statement with no citation`);
   - a lead-in not on the fixed list with no marker (`a lead-in that states a fact`).

   Code applies this, with no model.
3. **What this changes in 057 for topic pages:**
   - 057 §3.2–3.3's "uncited but consistent" pass no longer applies to topic pages;
   - §8 decision 2 of this spec supersedes the operator's 057 words "not every sentence needs a citation" for topic pages;
   - search pages keep 057 as it is.

### 2.3 What the validator sees, and what it must show

1. **The same evidence as the writer.**
   - For each cited record, the validator gets at least every passage the writer's brief held from it, cut the same way.
   - The material cap counts by records cited, and a cited record left out for length is named.
   - Each record shows its **source text** (the stored text of a source with citable text, §2.1 item 1) apart from any claim, finding or summary text, and the parts are marked.
2. **Every unit gets a verdict.**
   - **The list:** code lists every unit of the draft: each marked sentence, item, row, caption, key-term line and heading. The validator's contract names the units by number.
   - **A missing verdict:** a unit with no verdict is `unchecked`.
   - **A compound statement:** a unit that makes more than one assertion is `supported` only if every assertion is; one unsupported part fails the unit.
3. **The rule text,** given word for word in the writer's brief and the validator's contract:
   > Joining two or more records into a conclusion that none of them states is not support. Neither is a stronger wording than the record's ("may" made "does", "some" made "all"), a part made the whole, or a condition the record states left out.
4. **Pinpoint passages.**
   - For each `supported` verdict, the validator gives the passage it relied on. Code checks that:
     - the passage lies in the cited record's **source text** as shown, never in a claim, a finding, a summary, the seed or other generated prose;
     - it is made of **whole sentences**;
     - where the sentence before or after it **in the stored capture** (the passage is resolved to its offsets in the capture, not only in the excerpt shown) carries a qualifier the statement drops ("not", "no", "only", "unless", "except", a number or a condition), that sentence is part of the passage.
   - A passage that fails any of these makes the verdict `unchecked`.
   - The passage check proves the words exist, not that they mean what the statement says. The validator's judgement and the trial's hand checks cover meaning, and statements that turn on a negation or a number are listed first in the trial's hand-check sample.
5. **Conflicts.** When a contradicting record is shown and the page does not state the conflict with citations to both sides, the attempt fails (`a conflict the page does not state`).
6. **On a topic page, `unchecked` is a failure of that unit** (amends 057 §3.3 for topic pages), and the attempt follows 057 §3.4.
7. **The reply's end and size** are as the fixes of 2026-11-27 made them (`END ASSERTIONS`, sized to the page).

### 2.4 A support check after research (built only if called for)

1. **What it is:** a tool-less model checks each claim against its quote and the passage around it. It answers `supported`, `overstated`, `unsupported` or `can't tell`. It is a check, not validation: no status or trust rises. The result is kept beside the claim, tied to the hashes of the claim, the passage, the checker's model and its prompt, and claims are checked in batches per run.
2. **When it is built:** only if arm A0's hand checks (§2.8 item 5) attribute more than 2 kept wrong statements per 100 checked to research distortion (a claim saying more than its source).
3. **Its shadow run comes before a page relies on it:**
   - it checks stored claims with text, including about 30 planted distortions (a lost "not", a dropped condition, a widened scope, a changed number);
   - it also checks 50 claim–passage pairs labelled by hand;
   - it reports its miss rate and its false-rejection rate.

### 2.5 The evidence pack

1. **What it holds.**
   - At the end of a research run (`done` or `partial`), the run writes its evidence pack: each claim with its quote, the stored passage around it, the source's text hash, and (when §2.4 is built) the support verdict.
   - A claim whose quote is not anchored in the source's stored text is left out of the pack.
   - Code writes it; no model is called.
2. **Where it lives.** It is a Markdown file with frontmatter, in the research topic's folder, indexed as untrusted text, so an agent's retrieval returns a claim, its passage and its source together.
3. **Its label.**
   - The pack carries the strongest mark of any source it quotes (141 §2.2): `library_private`, `text_held`, `personal`, `internal`, `confidential`.
   - The mark is computed again from the sources at every exit check and every search, not only when the pack is written, so a source reclassified later reclassifies the pack and every page built from it.
4. **Its exits.**
   - The pack is on the list of senders of 142 B (`held_text::SENDERS`), and every exit checks it: `body_may_leave` and 141's label decision.
   - A pack of a private source never leaves.
5. **Packs for earlier runs.** `delphi research pack <topic-id> | --all` writes packs for runs that ended before this spec, from their records and the stored text.
6. **Queuing a page.** A run queues the topic's page job only on the conditions of §2.7 item 5, and only when the topic has evidence with citable text and page budget left (139 F).

### 2.6 What agents see

Under spec 145, reading untrusted text no longer takes a span's tools away; it marks what the span writes, and outbound actions ask the operator.

1. **Held text** (142 B) is in every role's default search, labelled `untrusted · not validated · held text`.
   - Its taint stays `external_untrusted`, so a span that reads it is marked as 145 says.
   - Until 145's slice A is merged, 142 B's rule as built stands: held text by default only for roles that do not act, `include_held: true` for the rest.
2. **Research answers and evidence packs** are in agents' search, labelled `research summary · not evidence` and `evidence pack · not validated`.
   - Each is taken at the lowest taint, and the strongest mark, of **every source its run read**, quoted or not: an answer may paraphrase without quoting, so its label comes from the run's inputs, not its quotes. An answer or a pack from a run that read held text is `external_untrusted`, like the held text itself.
   - It carries the marks of §2.5 item 3.
3. **Private sources stay out.** No search returns a private source, an answer or pack from a run that read one, or a page whose label (§2.7 item 4) is private, to any agent span. Every label is computed from the current marks of its material **before any result or snippet is returned**, cached answers and pages included; a result whose material cannot be traced is treated as private. The operator's own reading of local pages is 141 F's.
4. **Model calls are not exits** (operator, 2026-11-27). What a writer, validator or checker is shown is governed by items 1 to 3 and 141's search rules, not by the exit checks. The exit checks (141's sinks, `body_may_leave`) stay where they are.

### 2.7 Pages are built from evidence

1. **The writer's evidence.** A topic page is written by a fresh writer from the topic's evidence: its evidence packs, findings and knowledge-base passages, merged. It is never written by editing the previous page (139 §2.3 item 6 stands).
2. **The seed's role** (amends 139 §2.3 item 3). The writer still gets the seed as `DRAFT TO EXPAND`, **as an outline only:** its structure and its order. Its markers are re-pointed to the claims and passages they rest on. Text in the seed that rests on no claim or passage is not given to the writer as citable.
3. **The validator** checks the page against exactly the writer's evidence, plus the records that contradict it.
4. **A page carries the marks of all its material.**
   - A topic page's label (141 A, `Labeller::page`, which joins all material) is the join over **everything the writer and the validator were given, cited or not**: the seed, the evidence packs, findings, knowledge-base passages and the contradicting records, with each pack at the strongest mark of the sources it quotes.
   - A page that paraphrases private material while citing only a public source is therefore still private.
   - The label is computed again, from the current marks of that material, at every exit check and every search (§2.5 item 3, §2.6 item 3).
   - Every exit that carries the page checks it (141's label decision, `body_may_leave`).
5. **Builds nobody asked for** are governed by existing switches, with their defaults changed until a trial passes (§2.8 item 7):
   - **`[bridge.jobs] pages`:** default `manual` (amends 143 §2.8 item 4).
   - **`[library.pages] auto_rebuild`:** default `false` (amends 139 §2.5 item 3 and 117 §2 item 3).
   - **`build_after_research`:** stays `true`. It writes the seed, which costs no model call. The queued expansion runs only when one of the two switches above allows it.
   - **Which wins:** for a bridged build, both `[bridge.jobs] pages` and `auto_rebuild` must allow it; for an API build, `auto_rebuild` alone.
   - **What always runs:** a build the operator asks for (`delphi pages rebuild`).
6. **Unchanged:** the instant seed (139 §2.2); the page budget (139 F); a seed never replaces a page that passed; search pages fall back to 057's claims-only page.

### 2.8 The trial

**The protocol is dated in the tracker before the first build.** It fixes:
- the topic list;
- the token caps;
- the value rubric;
- the floors of this section.

1. **Order:**
   1. The golden set accepted (done, 2026-11-27) and its baseline run.
   2. Slices S0, A, B, C1 and D built, with 145's slice A.
   3. The golden set run again (slice B and 145 change what search returns).
   4. Research on 3 to 5 real questions through the bridge; packs written; `delphi research pack --all` for earlier runs; the golden set run again (packs are indexed).
   5. The pointer backfill (`delphi sources recheck --pointers`, 142 G); the golden set run again, checking for passages that appear twice (pack and source).
   6. Arm A0.
   7. §2.4 and arm A1, only if §2.4 item 2 calls for them.
2. **Sample.**
   - The topics are chosen before any page is written: every topic with at least three sources holding citable text, up to 20, listed in the protocol.
   - The trial runs only if at least 10 qualify. With fewer, the result is `inconclusive`, unrequested builds stay off, and no topic is padded with synthetic material.
   - The report says how many topics qualified out of all topics.
3. **Fixed:** the writer's and validator's models, the role packs and the brief, by hash.
4. **Arms:**
   - **A0:** §2.1 to §2.3, pages from evidence packs.
   - **A1:** A0, writing only from claims §2.4 found `supported`; on the same topics and evidence as A0.
5. **Measures, with floors.** Every measure counts over all sampled topics; a topic whose page failed to build counts as delivering nothing.
   - **Delivered:** at least 5 pages pass, and at least 50 kept statements are hand-checked, per arm.
   - **Planted errors:** at least 30 per arm, of each §2.3 item 3 kind; at most 10% may pass.
   - **Kept statements:**
     - **Sample:** 10 per topic per arm, drawn at random (statements that turn on a negation or a number first).
     - **Checker:** a validator-role agent run in the host, blind to the arm and to the validator's verdict, on a model family other than both the writer's and the validator's where one is configured (else other than the writer's). The operator audits a fifth.
     - **Floor:** at most 5% may be wrong. Each wrong one is attributed: writer overreach, citation, or research distortion.
   - **Each page's lead:** for every delivered page, its title, lead and section headings are checked the same way, in addition to the sample. A material error there fails that page, whatever the pooled rate.
   - **False rejections:**
     - **Sample:** 10 statements per arm that the validator failed, checked the same way.
     - **Floor:** at most 30% may be found supported.
   - **Value:** the operator reads each page against its seed with the protocol's written rubric. A topic counts when its page holds at least one cited statement its seed lacks that the operator judges correct and relevant to the topic's question, and reads as a page (lead, sections, links, bibliography). At least 60% of the sampled topics must count.
   - **Validator consistency:**
     - **Method:** each page is validated twice.
     - **Disagreement:** a unit whose two verdicts differ between `supported` and anything else.
     - **Floor:** more than 10% of units in disagreement means the arm does not pass, and the validator's model or tier is revisited.
   - **Leaks:** private or rejected material, statements from outside the evidence, unstated conflicts. Any one fails the arm.
   - **Cost:** tokens per delivered page, counted in full (fresh input, cache writes, cache reads, output, retries, failed attempts), raw and cost-weighted.
   - **Budget:**
     - at most 1,200,000 tokens through the bridge for A0, 1,200,000 for A1, and 600,000 for checks and reruns;
     - an arm cut off by its cap is `inconclusive` and cannot pass.
6. **Trimming, offline, only if called for.** If A0's failed attempts mostly fail on a few units (in at least half of them, three or fewer units fail), trimming is simulated by hand on A0's saved drafts and each trimmed result is read for changed meaning and citation reach. Otherwise it is not studied.
7. **Decides:**
   - **A1 over A0** only if A1 meets every floor and delivers at least as many pages that pass the floors.
   - **No arm meets the floors, or the result is `inconclusive`:** unrequested builds stay off (§2.7 item 5), and, if item 6 ran, the trimming spec of §1.3 is reconsidered.
   - **An arm meets the floors:**
     - the operator may approve an amendment citing the trial that returns `[bridge.jobs] pages` and `auto_rebuild` to their earlier defaults, and that may replace 139's late-slice trigger ("5 of the last 10 written pages pass") with the trial's result;
     - nothing changes a default without that approval;
     - the floors are a screen, not proof: 5 wrong in 100 leaves an upper bound near 11%.

### 2.9 Standing checks after the trial

1. **Spot checks.** For each batch of written pages, a sample of statements is checked as in §2.8 item 5. The sample size, the error limit and the consequence (unrequested builds go back off) are set in the trial's report.
2. **The golden set** runs on every retrieval change, as 140 F requires.

## 3. Authority

- **Agents see more; nothing new leaves.**
  - Held text, answers and packs are searchable at their true taint, under 145's rules: reading marks what a span writes, and outbound actions ask.
  - Private material is returned to no agent span (§2.6 item 3).
  - Every exit is unchanged.
- **Model calls are not exits** (operator, 2026-11-27); what models are shown is governed by search (§2.6).
- **A page carries the marks of all its material,** recomputed at every exit (§2.7 item 4).
- **Evidence packs carry the marks of what they quote,** recomputed at every exit and search (§2.5 item 3).
- **The support check raises nothing.**
- **The page rules only narrow what may be cited.**
- **Unrequested builds are narrowed:** their defaults move to off.

## 4. Slices

| Slice | Content | Criteria | Hours |
| :-- | :-- | :-- | --: |
| S0 | §2.1: citable text, titles only, research prose not evidence, research topics without the repository in any page evidence | AC-1 to AC-4 | 3-4 |
| A | §2.2 the closed marker rule, the lead-in list, code-written hatnote and navigation; §2.3 matched evidence with source text marked, every unit listed and judged, the rule text, pinpoints from source text in whole sentences, conflicts, `unchecked` | AC-5 to AC-8, AC-18 | 7-9 |
| B | §2.6: answers and packs labelled and taken at their sources' taint; private material out of every agent's search; held text for every role once 145 A is merged | AC-9, AC-10 | 1-2 |
| C1 | §2.5: the evidence pack (anchored claims only), its label recomputed from its sources, its exits, `research pack` for earlier runs | AC-11, AC-12 | 4-6 |
| C2 | §2.4: the support check, if §2.4 item 2 calls for it | AC-13 | 4-5 |
| D | §2.7: pages from merged evidence; the seed as outline; a page's label from all its material, recomputed at exits; the defaults and their precedence | AC-14, AC-15, AC-17, AC-19 | 5-6 |
| E | §2.8: the trial, protocol first (topics, caps, rubric), arms A0 and A1, report | AC-16 | 4-6 and the operator's reading |
| | **Total** (C2 included) | | **28-38** |

## 5. Acceptance criteria

1. **AC-1:** a source whose stored text is empty or only boilerplate by `citable_text`'s rule (a soft 404, a parked domain and a CAPTCHA page among the reject fixtures; an article, a documentation page and a paper among the accept fixtures, with the prose threshold fixed in code), or a pointer with only a summary, is in no `RECORD`. It is listed under `SOURCES READ`, and its exclusion is logged with the reason.
2. **AC-2:** a topic-page draft citing such a source fails the page check with `cited a source with no text`.
3. **AC-3:** a research answer is not a citable record for a page; its claim markers reach the claims' passages; unmarked answer prose is cited nowhere.
4. **AC-4:** for a research topic, neither the seed's row 6, the writer's evidence nor the validator's material holds a passage from a repository file (under a registered project's root, outside `research/`); row 5 stays. A topic the operator creates keeps repository files.
5. **AC-5:** on a topic page:
   - a body sentence, list item, table row, figure caption or key-term line with no marker of its own fails the page check;
   - a lead-in not on the fixed list ("Benchmarks show X is twice as fast as Y:") with no marker fails;
   - a heading that states a fact its section does not support is judged and fails;
   - code writes the hatnote, `Contents` and `See also`;
   - a marker covers only what it ends.

   A search page is checked as before.
6. **AC-6:** for every cited record, the validator's material holds every passage the writer's brief held from it; a cited record left out for length is named.
7. **AC-7:** the writer's brief and the validator's contract both contain §2.3 item 2's rule text verbatim, and a scripted test judges a planted strengthened statement not supported.
8. **AC-8:** a `supported` verdict becomes `unchecked`, and on a topic page its unit fails, when its pinpoint passage (resolved to its offsets in the stored capture):
   - is not in the shown record's source text;
   - is in a claim, finding, summary, the seed or generated prose;
   - is not whole sentences;
   - leaves out an adjacent sentence whose qualifier the statement drops (a cross-sentence fixture).
9. **AC-9:**
   - every role's default search returns a held source's passage, labelled, at `external_untrusted` (after 145 A; before it, as 142 B built it);
   - no agent span's search returns a private source, an answer or pack that quotes one, or a private page;
   - every exit test passes unchanged.
10. **AC-10:**
    - research answers and evidence packs are returned by an agent's search, each with its label;
    - an answer or pack from a run that read held text, quoted or only paraphrased, comes back at `external_untrusted`, and under 145 the span that reads it is marked as if it had read the held text;
    - a page or answer whose source was made private after it was built is not returned by the next search.
11. **AC-11:**
    - a finished run writes an evidence pack with frontmatter, indexed, carrying the strongest mark of the sources it quotes;
    - a claim without an anchored quote is not in it;
    - after a source is reclassified, the pack's mark at the next search or exit check is the new one;
    - `research pack --all` writes packs for earlier runs.
12. **AC-12:** a pack is listed in `held_text::SENDERS`. A bundle, export or push that carries a pack quoting a held source past `body_may_leave`, or a private source at all, is refused.
13. **AC-13** (if C2 is built): the support check gives each claim one of its four answers, tied to the four hashes, raises no status, and reports its miss and false-rejection rates from §2.4 item 3 before a page relies on it.
14. **AC-14:** a topic page is written from merged evidence, never from the previous page's text. The seed reaches the writer as an outline with its markers re-pointed. The validator gets the same evidence and the contradicting records.
15. **AC-15:** with the new defaults:
    - no idle or stale-writer rebuild, and no build queued by research, writes a long page;
    - `delphi pages rebuild` does;
    - the seed is still written after research.
16. **AC-16:** the trial's protocol is dated before its first build. Its report gives every measure of §2.8 item 5 per arm, the offline trimming result, and each decision of §2.8 item 7.
17. **AC-17:**
    - a topic page whose material (cited or not: the seed, packs, findings, passages, contradicting records) includes a held or private source carries that mark;
    - a page that paraphrases private seed material while citing only a public source is private;
    - a source reclassified after the page was built changes the page's label at the next exit check;
    - a bundle, export or push that carries the page is refused as the mark requires.
18. **AC-18:**
    - code lists every unit of a draft;
    - a unit the validator leaves without a verdict fails;
    - a compound sentence with one unsupported assertion fails;
    - a draft that leaves a shown contradiction unstated fails with `a conflict the page does not state`;
    - a draft holding raw HTML or a descriptive image text fails with `a construct the page check cannot read`, and the validator judges exactly the units the page renders.
19. **AC-19:**
    - a bridged build runs only when both `[bridge.jobs] pages` and `auto_rebuild` allow it, an API build when `auto_rebuild` does;
    - a research run queues a page job only when the topic has citable evidence and page budget left;
    - `delphi pages rebuild` runs whatever the switches say.

## 6. Verification plan

Slices S0 to D are tested with scripted writers, validators and checkers, as 139 C's tests are; a scripted test proves the wiring, not the judgement. The judgement is measured live by the trial's planted errors and hand checks. E is a live run on the operator's copy (`delphi-kv2` on `~/.delphi-kv2`), read by the operator.

## 7. What is not verified

- Whether `citable_text`'s rule separates content from boilerplate well. Its exclusions are logged, to be read.
- Whether the closed marker rule makes pages choppy. The trial's reading measures it.
- How often an injected instruction in held text shapes a local write (145's residual risk). The taint mark makes such writes visible; nothing here measures them.
- Whether sources themselves are right. The design checks that a page says what its sources say, not that the sources are true.
- Whether agents' retrieval gets better or worse with held text visible and packs indexed. The golden set runs after each change. If it gets worse, the labelling and duplicate passages are revisited, not the visibility.

## 8. Decisions for the operator

Decided on 2026-11-27 ("i agree with the [architect's] choices — accept them, including the change to 142B, and proceed"):

1. Agents see held text (§2.6). The operator chose by role first ("use (b) as th default"), then replaced the tool loss itself through spec 145 ("yes please, this direction is better"; "145 is approved"), so held text goes to every role.
2. One marker per statement on topic pages (§2.2). This supersedes 057's "not every sentence needs a citation" for topic pages.
3. Research topics draw no repository files (§2.1 item 4).
4. Evidence packs, and the trial's order (§2.5, §2.8).

8. Model calls are not exits ("model calls should not count as exits", 2026-11-27).

Proposed here, from the reviews of revisions 3 and 5:

5. Unrequested page builds default to off until a trial passes, through the existing switches; no new key (§2.7 item 5).
6. The support check is built only if A0's hand checks call for it (§2.4 item 2).
7. The trial's floors and budget (§2.8 items 5 and 7):
   - kept statements at most 5% wrong;
   - false rejections at most 30%;
   - at least 5 pages and 50 checked statements per arm;
   - at least 10 qualifying topics;
   - caps of 1.2M, 1.2M and 0.6M tokens;
   - A2 dropped;
   - no default changes without the operator's approval.

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
- **Revision 4:** red-team in Delphi on Gemini (`gemini-3.8-flash-medium`, help `t_0011`; a first attempt, `t_0010`, ended empty): approve with named changes.
  - **Applied:** lead-ins may state no fact, and captions and key terms carry markers (P0); a page carries its evidence's marks (P0); held text by the span's tools (P1, with the operator's choice (b)); kept-statement floor 5%, at least 10 topics, no default change without the operator (P1); soft 404s, parked domains and the HTTP status in `citable_text` (P2); whole-passage matching, with negations and numbers checked first (P2).
  - **Not applied as asked:** a 3% floor and 20 topics (on about 100 hand-checked statements 3% is noise, and 20 topics may not qualify; 5% and at least 10 are used).
- **Revision 5:** Council `t_0012` (heard: architect on Claude `claude-sonnet-5-5`, red-team on OpenAI `gpt-6.2-sol`; the strategist on Gemini was unreadable in Round 2). Both heard seats: approve with named changes; 8 P0s.
  - **Declined by the operator:** the P0s that model sends are exits (architect and red-team): "model calls should not count as exits" (2026-11-27). This matches 141, whose labels guard only the sinks.
  - **Applied:**
    - page labels from all material, cited or not, recomputed at exits, with a paraphrase test (P0; revision 5 had narrowed it in error);
    - packs and answers at their sources' taint, private material out of every agent's search (P0);
    - pinpoints only from source text, packs only with anchored claims (P0);
    - every unit listed and judged, compound statements (P0);
    - reclassification recomputes labels (P0);
    - headings judged, code-written hatnote and navigation, a fixed lead-in list (P1);
    - whole sentences with adjacent qualifiers, conflicts that fail unstated (P1);
    - the repository exclusion over all page evidence (P1);
    - per-arm token caps and `inconclusive` (P1);
    - a blind checker on a third family where configured, disagreement defined (P1);
    - delivery minimums, failed builds counted, a false-rejection limit (P1);
    - preselected topics, no padding (P2);
    - a value rubric (P2);
    - golden-set runs after packs (P2);
    - fixtures for `citable_text` and live judgement through the trial (P2);
    - criteria for switch precedence and queuing (P2);
    - A2 dropped, trimming studied only if called for (P2);
    - §1.3's reference (P3).
  - **Superseded by spec 145:** the held-text-by-role rule and its tool-loss tests (the red-team's P0 on derived retrieval leaving tools enabled is moot once reading untrusted text marks writes instead of removing tools; its taint half is kept in §2.6 item 2).
- **Revision 6:** red-team recheck on OpenAI (`gpt-6.2-sol`, ask `t_0015`): approve with named changes. It found the page-label and pinpoint P0s closed and three items open; all are applied here:
  - labels and taint of answers and packs from every source the run read;
  - one parsed form for rendering and validation;
  - labels recomputed at search, untraceable results treated as private.

  Its other findings are applied too:
  - qualifiers resolved in the stored capture (P1);
  - each delivered page's title, lead and headings checked (P1);
  - `citable_text` split into capture validity and content, with tables counted (P2).
- **Still needed:** an independent review of this revision by a second model family (a committed Council or Panel record cited in `reviewed_by`), or the operator's waiver quoted in the status line. Then the operator's approval.
