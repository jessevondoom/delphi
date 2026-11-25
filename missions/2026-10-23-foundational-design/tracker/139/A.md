---
state: built
date: 2026-11-25
title: "a read-only seed count on the live workspace, then the seed: spans, link rules, caps, determinism"
---

Built 2026-11-25 (local branch, not pushed). `topic_seed.rs` (new): a seed is assembled by code from the seven rows of §2.1, link first and search last, with the caps (40 spans, 64 KiB, 8 KiB a span cut at a paragraph, row 1 never dropped), the identifier mask, and the order by path then line. A library source gives only its `summary.md` and its kept excerpts, each cut to 300 characters, or is named in the bibliography. `delphi pages seeds --count [--json]` prints the spans each row would give, before and after the caps, and writes nothing. Tests: AC-1, AC-2 (determinism over shuffled directory order; private record, `internal`/generated page and source text never spans), AC-12, AC-17 (the CLI writes nothing), and the fitness tests `no_span_is_a_source_body` and `a_private_record_is_never_a_span`.

**The dated count (AC-17), 2026-11-25.** Run: the local build of this branch, `delphi pages seeds --count --home <copy> --workspace default`, against a copy (`cp -cR`) of the operator's `library/` and `workspaces/default/` made that day; the live home was not opened by the build. A checksum of the copy's `library/` was the same before and after. The project research folders the workspace registers were read in place, read only. Per topic (kept spans by row; rows 2, 4, 5 and 7 gave none on any topic):

| Topic | 1 answer | 3 findings | 6 search | spans / bytes | sources named only |
| :-- | --: | --: | --: | :-- | --: |
| topic 1 | 1 (62 chars) | 0 | 8 | 9 / 7,601 | 6 |
| topic 2 | 1 (62) | 0 | 7 | 8 / 5,646 | 2 |
| topic 3 | 1 (62) | 0 | 8 | 9 / 4,935 | 6 |
| topic 4 | 1 (62) | 0 | 0 | 1 / 62 | 4 |
| topic 5 | 1 (518) | 3 | 0 | 4 / 1,114 | 7 |
| topic 6 | 1 (62) | 0 | 8 | 9 / 7,024 | 8 |
| topic 7 | 1 (62) | 0 | 8 | 9 / 5,954 | 4 |
| topic 8 | 0 | 0 | 8 | 8 / 4,813 | 0 |
| topic 9 | 1 (62) | 0 | 8 | 9 / 5,250 | 3 |

What it shows: seven of the nine answers are the 62-character line "No claim could be drawn from the sources read. (not validated)"; one topic has no answer; only topic 5 has a real answer (518 characters) and the only findings. No source has a summary or an excerpt (all 94 are pointers), so row 2 gives nothing and 40 sources are only named. Search fills most seeds with passages that match the question's words, not research on the topic. So today a seed page would carry the research's text for one topic of nine; the rest wait on 142 (text and summaries) and on research that writes an answer.

Row 4 (kept summary pages): the bibliography names its sources by `- locator:` and `source_id:`, which code matches to the record's pages (§7's question answered: kept). No page is kept on this workspace, so it gives nothing. Row 5 gives nothing by rule: an ingest makes its file a private source, which §1.1 and §2.1 item 3 keep off a seed.
