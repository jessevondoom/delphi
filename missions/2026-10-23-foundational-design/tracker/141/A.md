---
state: built
date: 2026-11-26
title: "Label: the type, the join, chunks and pages, the decision order, missing and stale labels, handling relabel"
eval_waived: "the index gains a `marks` column and each hit a `marks` field, written after the sync from the daemon's records; what is indexed, how it is chunked and how a search ranks are unchanged, and no search reads the column"
---

Built 2026-11-26 on branch `build/141-a` (from `knowledge-v2`). Not merged.

## What was built

- **`label.rs`** (new).
  - `Mark`: `personal`, `internal`, `confidential`, `library_private`, `text_held`.
  - `Label`: a set of marks, each with the item ids it came from. `join` is the union. `every(why)` is every mark.
  - `Labeller`: computes a label from the daemon's records only. A source: its handling traits, `private` (flag or licence), `is_held`. A claim: its handling traits and the library marks of the sources it is anchored to. A research record: `library_private` when it is private. An index path: the item it names (a library source or claim, a research record, a file under the workspace's `local/`).
  - `Labeller::page(side)`: the join over the page's whole material (claims, sources, records, seed spans, followed research records), and a fingerprint of the label-bearing facts.
  - The decision order: `Sink` (the rows of §2.4), `row(sink, mark)`, `decide(label, sink)`. The strictest kind wins (refuse, send labelled, quotations, send); the quotation bound is kept whenever a mark brings it. `Decision::admits` is `held_text::body_may_leave` under the bound. A refusal's reason names marks and item ids, never text.
  - `at_sink(lib, ws, side)`: the label a sink acts on. It recomputes from the material and joins the stored label. No stored label, or a fingerprint that no longer matches: every mark.
  - `stale(...)`, `relabel(...)`, `relabel_rows(...)`, `relabel_index(...)`.
  - The visible line (`> Handling: CONFIDENTIAL (internal)`) and the `handling:` frontmatter view: `show_on_page`, `without_line`.
- **Pages** (`library_topics.rs`). `topic_store` computes the page's label from its material, stores it in the sidecar (`label`: marks, the ids each came from, the fingerprint), writes the `handling:` view and the visible line. A line or a `handling:` the writer wrote is removed. `topic_due` says `a label of its material changed` when the fingerprint no longer matches.
- **The library knows its workspace** (`library.rs`): `Library::with_workspace`. The daemon's topic builds set it (`page_run.rs`). Without it, an item of a workspace is every mark.
- **The index** (`kb.rs`). A new column `marks` on `kb_documents`. `Kb::refresh` and `kb::sync_workspace` write it after the sync, from the records. A row with no value (written before this slice) reads as every mark. Each `Hit` carries `marks`. `Kb::marks_of(rel_path)`. Nothing in retrieval reads it.
- **`delphi handling relabel [--json]`** (`handling_cmd.rs`, `CMDS`): recomputes the label of every page (topic and search pages) from its recorded material and of every index row. No model is called.

## Acceptance criteria and tests

- **AC-1**: `label::tests::the_join_is_the_union_and_a_claim_from_a_private_source_and_a_personal_email_holds_both`.
- **AC-2**: `label::tests::no_writer_tool_or_frontmatter_lowers_a_label` (a scripted writer declares the page unlabelled in a frontmatter, a visible line and a closing directive; an agent's and a host's calls add only; a source scan: no file but `label.rs` reads a stored label or builds a `Label`).
- **AC-3**: `label::tests::a_page_from_an_internal_and_a_private_record_carries_both_marks_and_drops_one_when_it_is_cleared`.
- **AC-14**: `label::tests::every_combination_of_marks_takes_the_strictest_decision` (32 combinations at 12 sinks; the `internal` and `text_held` page at export; ten quotations of 290 characters refused). The rows: `label::tests::the_rows_are_the_table_of_section_2_4`.
- **AC-19**: `label::tests::no_label_and_a_stale_label_are_every_mark`; the command: `handling_cmd::tests::relabel_gives_a_page_from_before_labels_its_label_and_a_second_run_changes_nothing`.
- **AC-21**: `label::tests::a_revisit_a_new_version_and_a_citation_edit_leave_the_label_fresh_and_a_label_fact_makes_it_stale`.
- Also: `label::tests::an_index_row_takes_its_label_from_the_records_not_from_the_document`, and `label::tests::a_label_decides_no_trust` (the separation test of 121, extended to `label.rs`).

## Builder decisions (for the architect)

1. **The sink table lives in `label.rs` for now.** AC-14 needs the decision order and the rows. Slice B moves the registry into `egress.rs` and wires `check_label` to `label::decide`.
2. **Outbound, `library_private`: refuse.** §2.4 says "held by the mark, as today". Read as a refusal, since the mark stops a send no person confirms. Slice D wires it.
3. **Bibliography, `text_held`: send.** The row's `S`, with the field cut of item 7 done by the bibliography's own scan (142 B).
4. **A claim carries `text_held` when an anchor's source is held**, as it carries `library_private` (§2.2: "the join of what it used and of its sources' library marks").
5. **A page whose sidecar lists no material is every mark**, and so is any material item whose record is gone or cannot be read (a claim, a source, a research record, a store).
6. **A page from before labels is not made due.** It is every mark at the sinks until `relabel` or its next build. Making every old page due would rebuild them all with model calls.
7. **A search page gets its label from `relabel` only.** Its build path (`library_page_search.rs`) is outside this slice's files. Until relabelled it is every mark at a sink. Slice C should label it at write.
8. **The research record's `label` field (§2.2 table) is not written.** Its label is computed from its `private` flag when needed; storing a copy would be one more thing to go stale.
9. **The index labels rows after each sync, every time.** It reads the handling records once and each library record it needs. Rows are written only when their label changed.
10. **The headline order** puts `private (library)` after private and before internal, and `text kept for local reading` last. `library_private` leads as `PRIVATE (LIBRARY)`.
11. **The index zones under `local/`** (summaries, knowledge, staged, memory) map to the handling item that is the file's own path. `handling::item_of` knew only `local`, `findings`, `team` and `provisional`.
12. **`relabel` is not refused inside an agent seat.** It only computes from the records; it lowers no label below what they say.
13. **The page check skips the visible line** (`check_page_text`, `library_page_check.rs`): the line sits above the banner, and the check wants the banner first. A line a writer wrote is removed before the page is stored.
14. **A frontmatter block inside a writer's draft stays in the body** as the writer's text, as before this slice. It is never read as a view: only the page's leading frontmatter is the daemon's, and no label is read from either.

## Existing tests changed on purpose

- Seven topic-page tests (`library_topics.rs`) and one gather test (`topic_gather.rs`) now give their library its workspace (`with_workspace`), as the daemon does. Without it a page's workspace items are every mark, and the page is labelled and later due.

## Not built here

- `check_label` in `egress.rs` and every sink's wiring (slices B, C, D).
- `[handling] internal_remotes` (slice D). No config key changed in this slice.

## Not verified

- No run on the operator's workspace: no page there has been relabelled.
- The cost of labelling every row after each sync on a large index is not measured.
