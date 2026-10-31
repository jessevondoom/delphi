---
id: spec-043-history
title: "043 history (non-normative): text folded out of spec 043"
author: architect
timestamp: 2026-10-30T12:00:00Z
status: approved   # history only; carries no rule
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, history]
---

# History of spec 043 (non-normative): text removed from specs/043-knowledge-window-search-page-and-growing-corpus.md when it was folded on 2026-10-30. The spec is the authority; this file only records past rationale.

Spec 043 carried no amendment sections of its own. What follows is the removed frontmatter comments, the status banner, and rationale prose, verbatim.

## Original frontmatter comments

```
status: approved   # by the operator, 2026-10-28 ("create and ratify the full spec. approve things so we can get building"); decisions D-1..D-9 are the operator's, given in chat the same day; nothing is built
confidence: medium   # the mockups were reviewed and approved by the operator; the index changes of §6 are new code; the research job itself is spec 028-A1
```

## Original status banner

> **Status: approved by the operator 2026-10-28.** It builds on `specs/028` (the window), amendment 028-A1 (two modes and *expand the archive*, PR #263; approved by the operator the same day) and `specs/029` (the reader). Where it differs from §3, §4 and §7 of 028, this spec wins for the archives pane. Mockups (100×34, dark): `assets/kb-decisions-options/` `kb-1-resting` to `kb-9-expanded` (PNGs, and `mockups.html` with `render.sh` to regenerate them). Nothing here is built; §11 is the build plan.

## 1.2 Simpler Alternative Considered
*Keep 028 §3 and §4 as built* (a shelf under the box; two lines of preview per result; offline/online toggle). It works, but a person cannot tell at a glance what the answer is, nor tell research from a spec, nor tell a document that opens here from one that opens elsewhere. The cost of the change is small (render code and one facet count), and the operator has approved the mockups.

## 4.2 Why `research` and `docs`
The operator's aim is to pull down a large amount of subject-matter reference so that Delphi gets smarter over time. That material must be **findable by kind, readable by a person, and visibly separate from curated and ratified knowledge**. `research` is what expand mode wrote; `docs` is what the operator or the repository supplies as reference. Both are plain, human-readable documents (Markdown, or text extracted from a PDF, DOCX or EPUB), and neither can become a `spec` or `lesson` by being stored (028-A1 §10.4).

## 8.3 Why the summaries get better
Search and the best-match card draw on whatever is indexed; the model summary reads the top five passages. With a larger and more varied `research` and `docs` corpus, the top five passages become more relevant and more varied, so summaries cite more and different sources. No special mechanism is needed: this is why the corpus is full text and human readable, and why the summary is cited (so the operator can read the sources behind a claim).
