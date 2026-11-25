---
state: built
date: 2026-11-25
title: "the seed page: render, cleaning, the seed check with the held-text predicate, outcome seed, no stub"
---

Built 2026-11-25 (local branch, not pushed). `topic_seed::seed_page` renders the seed by code (a lead from the newest answer, the rest of the answer under its own headings, then `## Sources read`, `## Findings`, `## Related notes` and `## Claims`), each block a span's own words cleaned by the one function `clean` (links keep their words, images their alternative text, raw HTML and bare addresses go, control characters go, bracketed record ids lose their brackets) and followed by its marker. The seed check drops a block from a document a seed leaves out or one not found letter for letter in its cleaned span, and passes the page through the held-text predicate: the trait `HeldText` (`sharing` = 142's block index, `is_held`, `body_may_leave`, `shorten`), asked for every held source the page shares a block with and every held source the topic read; a quoting block is shortened and marked `[shortened: quoted source text]`, else dropped; over the aggregate cap the last quoting blocks go. Today's implementation is `NoHeldText` (no source is held, so nothing is shared and everything may leave); `specs/142` slice B supplies the real predicate. Dropped blocks are kept in the sidecar's `seed_dropped`; with none left no page is written and the error says `check_failed`.

Outcome `seed` (`Outcome::Seed`), the banner of §2.2 item 2, the bibliography the daemon writes, no model, no tokens, not counted in `per_day`. `topic_rebuild` no longer writes the stub: with nothing gathered the page is the seed, and an engine with no writer route writes the seed instead of failing. A topic whose linked rows give nothing is not due, is listed `waiting for material`, and `pages rebuild` says so. A seed page's sidecar keeps each span's place and hash (`seed`), what each row offered (`seed_offered`) and the records; the due check reassembles the linked rows and is due when a span changed or went or a row offers more. The app shows a `seed draft` chip. Tests: AC-3, AC-4, AC-18 (`a_seed_page_passes_body_may_leave_for_every_source`, with a held-source fixture), the failure path, and the idle rebuild with no model call.

**Seed pages built in a copy, 2026-11-25.** The local build ran `delphi pages rebuild <slug>` for the nine topics against the scratch copy of slice A (daemon spawned on the copy with an empty `HOME` and a `PATH` with no harness, and shut down after each). Each became version 1, outcome `seed`, no block dropped:

| Topic | words | spans (by row) |
| :-- | --: | :-- |
| topic 1 | 1,061 | 9 (1: 1, 6: 8) |
| topic 2 | 930 | 8 (1: 1, 6: 7) |
| topic 3 | 733 | 9 (1: 1, 6: 8) |
| topic 4 | 118 | 1 (1: 1) |
| topic 5 | 277 | 4 (1: 1, 3: 3) |
| topic 6 | 1,008 | 9 (1: 1, 6: 8) |
| topic 7 | 877 | 9 (1: 1, 6: 8) |
| topic 8 | 619 | 8 (6: 8) |
| topic 9 | 766 | 9 (1: 1, 6: 8) |

Words count the body without the bibliography. Most of each page is search passages; the research's own words are one "no claim could be drawn" line on seven topics.

Not done here: a health count of pages by outcome (premortem 1), the notice `check_failed` of 143 §2.7 (the cause is the library log and the error until 143 exists), and the frontmatter of a seed page still says `author: writer`, `depth: summary`, as every topic page's does.
