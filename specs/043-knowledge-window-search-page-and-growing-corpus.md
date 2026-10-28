---
id: spec-043-knowledge-window-search-page-and-growing-corpus
title: "043: The Knowledge Window, Round 2: a Plain Search Page, a Best Match, and a Corpus That Grows"
author: architect
timestamp: 2026-10-28T12:00:00Z
status: approved   # by the operator, 2026-10-28 ("create and ratify the full spec. approve things so we can get building"); decisions D-1..D-9 are the operator's, given in chat the same day; nothing is built
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the mockups were reviewed and approved by the operator; the index changes of §6 are new code; the research job itself is spec 028-A1
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, knowledge-base, search, research, narrator]
---

# Spec 043: The Knowledge Window, Round 2

> **Status: approved by the operator 2026-10-28.** It builds on [`specs/028`](028-knowledge-window.md) (the window), amendment 028-A1 (two modes and *expand the archive*, PR #263; approved by the operator the same day) and [`specs/029`](029-links-and-inline-reader.md) (the reader). Where it differs from §3, §4 and §7 of 028, this spec wins for the archives pane. Mockups (100×34, dark): [`assets/kb-decisions-options/`](assets/kb-decisions-options/) `kb-1-resting` to `kb-9-expanded` (PNGs, and `mockups.html` with `render.sh` to regenerate them). Nothing here is built; §11 is the build plan.

## 1. Scope & Objectives
The archives pane is how the operator reads what Delphi knows. Round 2 does four things:
1. **A plain page.** A search box in the middle and nothing else, as the early search engines did. No chat prompt on this pane; the footer stays.
2. **A results page that is easy to scan.** One **best match** with a short summary of itself, then a uniform list. Everything that costs tokens is a separate, labelled, opt-in action.
3. **Two new first-class kinds, `research` and `docs`,** so the corpus that expand mode pulls down (028-A1) and the operator's own reference documents are found, filtered and read like the specs and lessons.
4. **A corpus that gets better with use.** Expand mode stores human-readable markdown; the more of it there is, the better both the search and the model summary (§8) become. This spec makes that growth visible (counts per kind, a one-key path from "nothing here" to "research this") but does not change how the job runs (028-A1).

### 1.1 Non-Goals
- **A chat prompt, an "ask" box or a conversation on this pane.** The pane searches and reads. (The operator may open the chat for questions.)
- **Searching the web from the search page.** The offline/online/both indicators are gone (028-A1 §10.1). The web is reached only through expand mode, by a deliberate `enter` on a topic.
- **A new ranking engine.** Results are what `kb` returns today (keyword + vector + reranker, `specs/004`).
- **Source code as a kind.** The index holds Markdown (and, after §6, extracted document text); it does not index source files, so `code` is not a kind.
- **Automatic summaries.** No model runs unless the operator asks (§7).

### 1.2 Simpler Alternative Considered
*Keep 028 §3 and §4 as built* (a shelf under the box; two lines of preview per result; offline/online toggle). It works, but a person cannot tell at a glance what the answer is, nor tell research from a spec, nor tell a document that opens here from one that opens elsewhere. The cost of the change is small (render code and one facet count), and the operator has approved the mockups.

### 1.3 Principles
1. **Zero tokens until asked.** Searching, the best-match summary, filters and reading cost no model call. A model summary is one key, and its cost is on screen before the key is pressed.
2. **Spend once.** A model summary is cached against exactly what it summarised; reopening it is free (§7.4).
3. **Say where it came from.** Every row shows its kind and trust. A model-written text says so in words.
4. **Colour helps, words decide.** Kind colours are a scan aid; the kind is always also printed. `NO_COLOR` and 16-colour terminals lose nothing.
5. **Reading never runs anything.** Documents are read as text; media is handed to the operator's browser as a file, never executed by Delphi (§6.3).
6. **No new authority.** Nothing on this pane writes to the knowledge base, approves, or raises the trust of what it shows (`specs/001` §5).

## 2. Decisions (operator, 2026-10-28)
| # | Decision |
| :-- | :-- |
| D-1 | The pane has **two modes**, search and expand, switched by `shift+tab` (028-A1). No `offline`/`online`/`both` indicators in either mode. |
| D-2 | The search page is the **early-search-engine page**: title, one box, the shape of the store in one line. **No prompt bar** on any screen of this pane (including the reader). The footer stays. |
| D-3 | Results show **one best match** (a card whose summary is the document's own words, chosen by code) and a **uniform list** beneath it. |
| D-4 | **"Summarize for me"** is available on the results page. It uses the model, says so and says what it will cost, and is cached. |
| D-5 | **Markdown opens inline** in the reader. **PDFs and media open in the operator's browser** (§6.3). |
| D-6 | **Kinds** are `research`, `docs`, `spec`, `lesson`, `mission`, `skill`, `memory`; `research` and `docs` are first-class (§4). |
| D-7 | The aim of expand mode is to **build a large, human-readable reference corpus** (full text where the licence allows, summary and facts otherwise: 028-A1 §10.5), so that later searches and summaries are better. |
| D-8 | Model summaries are **cached** by content: the operator pays tokens for a given summary once (§7.4). |
| D-9 | Search results and counts are computed locally and never leave the machine; only expand mode sends a query out (028-A1 §10.7). |

## 3. The screens
Each screen below is a mockup in `assets/kb-decisions-options/`. All are 100×34; §9 lists the 80×24 rules.

### 3.1 Search mode, resting (`kb-1-resting`)
- Title row: the orchestrator badge and `search the archives`. One box. Beneath it, one line of counts by kind, in kind colours, then `documents · passages · updated`. Nothing else: no recent list, no "most used" list (they moved to the shelf's `enter` browse, 028 §4).
- Hint: `type to search · ↑ earlier searches · tab filter by kind · ctrl+t trusted only`. Footer right: `shift+tab  expand  ·  ← →  ·  ○ ○ ○ ○ ● ○`.
- `tab` selects a kind on the counts line and `enter` browses that kind's documents newest first (028 §4, unchanged).

### 3.2 Results (`kb-2-results`)
Top to bottom:
1. **The box** keeps its row at the top; the result line under it reads `N results · T s · 0 tokens`, with `s  summarize for me · uses the model, about <n> tokens` at the right.
2. **The filter row** (§4.2): `show ● all 14  ○ research 5  ○ docs 4 …`, one chip per kind that has results, counts for this query, each in its kind colour; the active one is filled and bold. `tab` moves to the next chip (wraps); the list and the best match are recomputed from the same results.
3. **The best-match card** (a soft-fill block, square, as chat bubbles are): label `BEST MATCH`; at the right its kind, trust and age (for `research`: form, licence and retrieved date); the title (`path › heading`) in the accent colour; **the summary**; a dim line of provenance (source host or file, line range, "also in results 2, 3 and 5" when known); the actions `enter read · o open file · m more like this`.
4. **The list** (§5): a header row, then one entry per result.
5. `+ N more below` when the list continues.

**The best-match summary costs no tokens.** It is the **passage itself, trimmed by code**: the up-to-three sentences of the top passage that contain the most query terms, in document order, at most 300 characters, with matched words in bold. It is the document's own text, never paraphrased, so it needs no label beyond the title.

### 3.3 Summarize for me (`kb-3-summary`)
Replaces the best-match card with a **SUMMARY** card labelled `written by the model, not checked`, naming its inputs (`from results 1–5`), with `[n]` citations and a `sources` line. The cited rows in the list carry `◂ [n]`. The result line shows `✔ summarized · <n> tokens spent · cached, reopening is free`. `esc` returns to the best match. §7 specifies the call, the limits and the cache.

### 3.4 The reader (`kb-4-reader`)
The reader of `specs/029`, with three additions: (a) a **provenance banner** under the path for `research` entries and any `listed` or `untrusted` document (publisher, licence, retrieved date, share-alike flag, and the words `listed source, not curated by you`); (b) links to hosts not on the operator's allow lists render dim with `seen, not read: not on your list` and cannot be followed; (c) a position indicator (`lines a–b of N · p %`). No prompt bar. Keys as 029 §7: `tab` link · `enter` follow · `[` `]` back and forward · `y` copy path · `esc` back to the results.

### 3.5 Media and PDFs (`kb-5-pdf`)
A result that opens in the browser is marked `↗` after its kind, and the result line says `↗ opens in your browser, everything else opens here`. For a PDF the best-match card shows the first lines of its extracted text (zero tokens, `specs/014`), its size and page count. `enter` hands the file to the operator's browser and the card then reads `✔ opened in your browser · read-only, Delphi never runs it`; `enter` again re-opens; `o` shows the file in Finder (or the platform's file manager); `c` copies the path. §6.3 defines what may be opened.

### 3.6 Nothing found (`kb-6-empty`)
`Nothing in the archives matches "<query>".` then three suggestions with real numbers: fewer words (the count the shorter query would return), `ctrl+t` (how many untrusted documents would match), and **`shift+tab` expand the archive: research this topic from trusted sources**. A dim line says expand needs a `[search]` provider when none is configured (028 §6). **`shift+tab` carries the current query into the expand box** (§8.2).

### 3.7 Expand mode (`kb-7-expand`, `kb-8-researching`, `kb-9-expanded`)
The behaviour is 028-A1 §10.2 to §10.7, unchanged. This spec fixes how the three pages look: the clean page (title, the scout badge and `expand knowledge base: research a topic`, the box, nothing else); the progress page (live counters and one line per source: `✔` done with its form and licence, `●` reading, `○` queued); the results page (the banner `the archive has been expanded to include N new entries`, the new entries as rows `path · form · licence`, then `read, not kept` and `seen, not read` lines). In expand mode the footer names the other mode: `shift+tab  search`.

## 4. Kinds
### 4.1 Definition
A **kind** is derived from where a document lives. It is a display and filter label; it carries no authority and is never read from the document's own text.

| Kind | Where (logical path) | Colour | Notes |
| :-- | :-- | :-- | :-- |
| `research` | `knowledge/web/**` (written by expand mode, 028-A1) | yellow | trust `listed` (a source on an allow list); form `full` or `summary`; licence and `source_url` in frontmatter |
| `docs` | `docs/**`, and `context/**` except `context/lessons/**`, and documents the operator adds (PDF, DOCX, EPUB, Markdown) | green | human-readable reference: manuals, notes, papers, runbooks |
| `spec` | `specs/**` | cyan | |
| `lesson` | `context/lessons/**` | pink | |
| `mission` | `missions/**` | teal | |
| `skill` | `skills/**` and bundle skill references | dim | |
| `memory` | `memory/**` (agents' memory shows tagged; private memory per 028) | dim | |

A document that matches two rows takes the first match in the table order above. A kind with no results is not shown as a chip.

### 4.2 Why `research` and `docs`
The operator's aim is to pull down a large amount of subject-matter reference so that Delphi gets smarter over time. That material must be **findable by kind, readable by a person, and visibly separate from curated and ratified knowledge**. `research` is what expand mode wrote; `docs` is what the operator or the repository supplies as reference. Both are plain, human-readable documents (Markdown, or text extracted from a PDF, DOCX or EPUB), and neither can become a `spec` or `lesson` by being stored (028-A1 §10.4).

### 4.3 Trust vocabulary
The trust column reads `trusted` (curated: specs, lessons, the operator's documents), `listed` (written from an allow-listed source; 028-A1 `trust: trusted_source`) or `untrusted` / `inactive` (028 §3, shown in the warning colour). Ranking and the `ctrl+t` filter use the same values as today; `listed` ranks below `trusted` and above `untrusted`.

## 5. The result list
### 5.1 Row shape
Every result is two lines and a blank line, in the same columns, so the eye can run down the page:

```
 ❯  2  knowledge/web/arxiv.org/castro-liskov-pbft.md › Abstract   research  listed       2 d
       a state machine replication algorithm that tolerates Byzantine faults and works in asynchronous …
```
| Column | Rule |
| :-- | :-- |
| marker | `❯` on the selected row, accent colour |
| rank | right-aligned, dim (accent on the selected row) |
| title | `path › heading`, 56 columns, truncated in the middle of the path with `…`, never wrapped; bold on the selected row |
| kind | the kind in its colour; `↗` after it when the file opens in the browser (§6.3) |
| trust | `trusted`, `listed`, `untrusted` (warning colour) |
| edited | age of the document (for `research`, the retrieved date), `3 d`, `2 w` |
| snippet | one line, dim, the matched words in bold accent, truncated with `…` |

### 5.2 Rules
- Order is relevance (what `kb` returns); no score is shown (`?` shows it, as in 028 §4).
- Under 90 columns the `edited` column is dropped; under 80 the trust column is dropped and untrusted rows are marked with `!` after the kind.
- The selected row is the only one in bold. There are no boxes between rows.
- `m` (more like this) and the trail of 028 §4 are unchanged. `enter` opens a row: markdown in the reader, a `↗` row in the browser.

## 6. Reading, documents and media
### 6.1 What the index must hold
The index (`crates/delphi-engine/src/kb.rs`) is Markdown only today. To make `docs` real, it also indexes:
- **PDF, DOCX and EPUB** files under `docs/**` and operator-added documents: the text is extracted by the in-tree readers of `specs/014` (`crates/delphi-docs`), chunked as Markdown is, and indexed under the same path with its extension; a file over 20 MB, or one whose extraction fails, is listed by name and size and not searched by content (and says so).
- **Images, audio and video** under the same roots: **listed by file name, path, size and type only**. There is no OCR and no transcription; the file is found by its name and by any sidecar `<name>.md` beside it, which is indexed as an ordinary document.
- Extraction is pure code (no process is spawned, no macro or script runs), as in 014.

### 6.2 The reader and links
The reader (029) is unchanged except for §3.4. A result that is a PDF/DOCX/EPUB opens in the reader as its **extracted text** with a header line `text extracted from <file>, <n> pages`, and `o` opens the original (§6.3). A document that links to another document inside the archive opens it in the reader; a link to anything else is `external` and, as in 029, is opened by the system browser on request and never fetched by Delphi.

### 6.3 Opening in the browser (the media handoff)
- **What may be opened.** An allow-list of extensions: `pdf`, `png`, `jpg`, `jpeg`, `gif`, `webp`, `mp3`, `m4a`, `wav`, `mp4`, `mov`. **`svg`, `html`, `htm`, `xml`, `js`, executables, archives and anything else are never handed over**; SVG and HTML are shown as text in the reader.
- **Only indexed files.** The path must be a document the index lists as visible, resolved to its canonical path under the workspace or the operator's documents root. A path from a document's text, a link, or an agent is never opened by this key.
- **By file, not by URL.** The opener is invoked with an argument vector holding the canonical `file://` path or the path itself (`open` on macOS, `xdg-open` on Linux): never a shell string, never a URL taken from content. The same opener serves `external` links in 029.
- **Read-only.** Delphi does not write to the file, does not execute it, and does not report that the operator's viewer rendered it (it reports that it asked).
- **Trust is shown.** A `↗` row that is `untrusted` says so on its row and in the card before it is opened.
- The narrator is otherwise read-only (005e); opening a file is an operator action on the operator's own machine, like copying a path, and is logged as an event (`kb.opened`: path, blake3, extension; not the content).

## 7. Summarize for me
### 7.1 What it does
On `s` (results focused), one model call writes a short summary of the current results. It is **opt-in, labelled and cached**; it is never run on typing, on opening a result, or on a timer.

### 7.2 The call
| Item | Rule |
| :-- | :-- |
| Inputs | the passages of the top **5** results of the current filter (at most 8 passages, at most 4,000 input tokens); `untrusted` and `inactive` results are excluded unless `ctrl+t` has been turned off by the operator, in which case the card says `includes untrusted documents` |
| Model | the `fast` tier (`specs/002`); the model id is part of the cache key |
| Output | at most 250 tokens; every claim ends with a `[n]` marker naming one of the inputs |
| Checks in code | a marker with no matching input is removed; a summary with no markers is shown with `no citations: treat with care`; the output is plain text (no links, no markup rendered) |
| Untrusted text | passages go to the model inside the spotlighted untrusted block (`specs/001` §5.1); the call has **no tools**, so an instruction inside a passage has nothing to call |
| Label | `SUMMARY · written by the model, not checked` always; it does not claim to have verified anything |
| Not stored in the archive | the summary is never indexed, never saved as a document, never ranked; it lives only in the cache of §7.4 |

### 7.3 Cost shown before it is spent
The result line shows `s  summarize for me · uses the model, about <n> tokens`, where `<n>` is computed in code from the input passages plus the output cap. After the call the footer's token count includes the spend. If a cached summary exists for these exact inputs, the line instead reads `s  show the summary · cached, free`.

### 7.4 The cache (spend once)
- **Key:** `blake3(normalised query · sorted [path + content blake3 of each input passage] · model id · prompt version)`.
- **Value:** the summary text, its markers resolved to paths, the tokens spent, the time and the model.
- **Where:** `$DELPHI_HOME/cache/kb-summaries/<key>.json`, local, never synced, never indexed, safe to delete.
- **Behaviour:** the same query over the same unchanged documents is a hit and costs nothing. If any input document changed (its blake3 differs) or the model or prompt version changed, the key differs and the next `s` makes a new call (and says so: `the sources changed since this summary`). A summary is never refreshed silently.
- **Bounds:** at most 500 entries, oldest evicted first.

### 7.5 Failure
No model available, quota out, or the call fails: the card says so in one line and offers a retry; nothing is cached; the page stays usable.

## 8. A corpus that grows
### 8.1 What the operator sees
- The resting page's counts line shows `research N` and `docs N` beside the rest, so growth is visible.
- Expand mode's results page lists each new entry with its form and licence, and the new entries are searchable at once (028-A1 §10.2 step 4).
- `research` entries are **human-readable Markdown**: full text where the licence allows reuse, otherwise a summary and facts, each fact with its source URL and a quote of at most one sentence (028-A1 §10.5). They open in the reader like any document, with the provenance banner of §3.4.

### 8.2 From a gap to a research job in one key
When a search returns fewer than 3 results of kinds `research` or `docs`, the results line adds `thin on reference: shift+tab to research this topic` (dim). `shift+tab` switches to expand mode **with the current query already in the box**; `enter` then runs the job as 028-A1 describes. The window never starts a job by itself and never sends the query out while typing.

### 8.3 Why the summaries get better
Search and the best-match card draw on whatever is indexed; the model summary reads the top five passages. With a larger and more varied `research` and `docs` corpus, the top five passages become more relevant and more varied, so summaries cite more and different sources. No special mechanism is needed: this is why the corpus is full text and human readable, and why the summary is cited (so the operator can read the sources behind a claim).

### 8.4 Limits
The size of the research corpus is bounded by 028-A1 §10.6 (per-job limits) and by the operator's disk; this spec adds **no** automatic expansion and **no** background crawling.

## 9. Layout rules
| Width | Behaviour |
| :-- | :-- |
| 100 × 34 | as mocked |
| 80 × 24 | the best-match card's summary shows two lines and the provenance line is dropped; the list shows 3 results; the `edited` column is dropped; the filter row scrolls horizontally so the active chip is always visible; the footer drops the dots |
| under 40 | the box drops its sides and becomes a ruled line; chips become a single `show: <kind> (tab)` label |

Golden screens for 100×34 and 80×24 live under `ui/test/snapshots*` (the screen is pure data).

## 10. Keys
Search page: type · `enter` search · `↑ ↓` recall earlier searches from the box, move through results once in the list · `tab` the next kind chip (wraps) · `ctrl+t` trusted only / every document · `ctrl+u` clear the box · `esc` clear results, then the box · `shift+tab` switch to expand mode (query carried) · `← →` switch view.
Results (list focused): `↑ ↓` pick · `enter` open · `s` summarize / show the cached summary · `m` more like this · `o` open the original file (or show it in Finder) · `c` copy path · `/` back to the box · `esc` clear.
Reader: as 029. Expand mode: type a topic · `enter` research · `esc` cancel / back · `shift+tab` back to search.

## 11. Build plan (slices; each one pull request, each with `Implements-Spec: specs/043-…`)
| Slice | Content | Depends on |
| :-- | :-- | :-- |
| K1 | Page chrome and results: title and box, no mode indicators, filter row with per-kind counts, uniform rows, best-match card with the extractive summary; kind derivation; `facets` (counts by kind over up to 200 candidates) in `/v1/search` and `kb.get`; golden screens at 100×34 and 80×24 | 028-A1 E1 (the mode toggle) |
| K2 | Index PDF, DOCX and EPUB text and list media by name (§6.1); `docs` kind complete | spec 014 readers |
| K3 | Reader provenance banner, dim un-listed links, `↗` rows, the browser handoff with its allow-list and event (§6.3) | K1, 029 |
| K4 | Summarize for me with the cache (§7) | K1 |
| K5 | The thin-reference hint and query carry-over into expand mode; the expand pages' visuals (§3.7) | K1, 028-A1 E3, E4 |
| K6 | `docs/reference` regenerated, keys in `delphi help`, the tracker updated | all |

### 11.1 Acceptance tests (automated, no live network, no real model)
1. The search page and every results screen contain **no** `offline`, `online` or `both` text, and no prompt bar; the footer is present on every screen of the pane.
2. The best-match summary is a substring-sequence of the top passage and is at most 300 characters; no model call is made by searching, filtering, reading or opening (a counting fake model sees zero calls).
3. Chip counts equal the number of results of that kind among the (up to 200) candidates; `tab` wraps; the list and card follow the active chip.
4. Kind derivation follows §4.1 for every path in a fixture tree, including a document that matches two rows, and a document cannot set its own kind.
5. A PDF/DOCX/EPUB fixture is found by a phrase from its text; an oversized or corrupt one is listed by name with the stated reason; media is found by file name only.
6. The opener is called with an argument vector for an allow-listed extension of an indexed file, and **never** for `svg`, `html`, a path outside the index, a path produced by document text, or a symlink that resolves outside the roots (fake opener asserts).
7. `s` makes exactly one fake-model call; the same query over unchanged documents makes none and shows `cached, free`; changing one input file changes the key and says so; a marker with no input is removed; a failed call caches nothing.
8. Untrusted passages never reach the model unless the operator has widened the filter, and the card then says so.
9. `shift+tab` from a thin result carries the query into the expand box and starts nothing; online search never starts while typing.
10. At 80×24 and 100×34 the screens match their goldens; with `NO_COLOR` the kind is still printed on every row.

## 12. Decisions that remain for the operator
None block the build. Recommended defaults recorded here, changeable by amendment: summary scope is the top 5 results (a `[kb] summary_results` setting later); the media allow-list of §6.3; the 20 MB extraction cap; at most 500 cached summaries; whether `memory` should be a chip by default (recommended: yes, tagged).
