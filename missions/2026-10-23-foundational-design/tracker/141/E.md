---
state: built
date: 2026-11-27
title: "the query notice: planned_with"
eval_waived: "the slice adds a notice to the research run's result, its search log lines and its record; what is indexed, how it is chunked and how a search ranks are unchanged, and no query is changed or withheld; the diff base counts knowledge-v2's own retrieval changes"
---

Built 2026-11-27 on branch `build/141-e` (from `knowledge-v2`). Not merged.

## What was built

- **`research_run::PlannedWith`** (`research_run.rs`): the label of the labelled items in view when a run planned its queries (§2.5, building 121 §9 item 2). It is the join of:
  - what the span that asked was given: its given list (`handling::given` under `handling::span_key`) and the entries of its private-read mark (103);
  - the `known` list the run hands its planner (114 §3): its claims, sources and topics, and the knowledge-base files it points to.
  Every label is computed by `label.rs` from the daemon's records. It is a notice: no branch reads it to change, withhold or refuse a query.
- **The run** (`run_research`): computes the notice after the `known` lookup and before its first query; sets it on the search log; the result carries `planned_with` (always, as a list; empty when nothing labelled was in view) and `planned_with_note` when it is not empty. `planned_under_mark` is unchanged beside it.
- **The search log** (`search_log.rs`): `SearchLog::planned_with(marks)`; every query the run sends (`begin`) or hands a host (`host_ran`) after that carries `planned_with` on its line. A declared search, a withheld one and the knowledge-base line do not (the run planned none of them). The log carries `planned_with` and `planned_with_queries` once. Only the five marks' code names are kept, in the visible line's order (`MARKS`). `planned_with_in(log)` counts the lines.
- **The record** (`research_record.rs`): the run's entry keeps `planned_with` (code names only) and `planned_with_queries` when it finishes; the result kept in the record carries `planned_with` as the runner gave it. `planned_with_count(ws)` sums the queries and runs over every topic of the workspace and its registered projects. The count lives on the run entry, so a later run of the topic, which replaces the search log, does not lose it.
- **The reply to a host** (`service.rs`, `host.request_research`): `planned_with` and `planned_with_note`, read after the host's plan is waited for, so a planned-query reply (119 §13.1) names the `known` list's marks too. The console's and an inside agent's runs (`launch_research`) take the notice of the agent's span; the operator's own run has only its `known` list.
- **Schemas**: `research-result.schema.json` (the result's `planned_with` and `planned_with_note`, the log's and each line's `planned_with`) and `research-record.schema.json` (the run entry's `planned_with` and `planned_with_queries`).
- **`delphi handling status`** (`handling_cmd.rs`): one line with the count, and `planned_with: {queries, runs}` in `--json`.
- **`Labeller::item`** (`label.rs`): the label of a handling item by its id, as a given list holds it (a claim, a source, a research record, or a workspace item by its handling record; the host-noted store is every mark).

## Acceptance criteria and tests

- **AC-10** (a query planned by a span that was given a `confidential` item is sent as it would have been, and the reply, the search log and the result carry `planned_with: ["confidential", "internal"]`; no query is withheld for a mark):
  - the run, the result and the log: `research_run::tests::a_query_planned_by_a_span_given_a_confidential_item_is_sent_unchanged_and_carries_planned_with` (the same queries sent as without the notice, the same flow, no line withheld);
  - the planner's `known` list: `research_run::tests::the_known_list_handed_to_the_planner_adds_its_marks_and_an_unreadable_one_is_every_mark`;
  - a host's turn: `research_run::tests::the_queries_a_host_searches_for_the_run_carry_planned_with`;
  - the reply, the result and the log over the socket: `service::tests::a_research_request_with_no_engine_runs_locally_sends_no_query_and_is_counted_for_the_next_brief` (extended);
  - the log's lines: `search_log::tests::a_query_begun_after_the_notice_carries_planned_with_and_nothing_else_changes`;
  - the record and the count: `research_record::tests::a_run_planned_with_labelled_items_keeps_planned_with_in_its_record_and_the_status_counts_its_queries`;
  - the status line: `handling_cmd::tests::status_counts_the_queries_planned_with_labelled_items_in_view`;
  - the private-read entries: `research_run::tests::an_asker_mark_entry_with_no_record_and_an_unreadable_mark_are_every_mark`.

## Builder decisions (for the architect)

1. **"A span that was given labelled items" is two spans, joined.** The asker's (its given list and its private-read entries) and the run's own planner, which is handed the `known` list (114 §3). The stricter reading: a query is planned from the asker's question and, under the scout's plan, from the planner's sub-questions.
2. **The notice is one label for the whole run**, set before its first query. The run's later rounds plan from the sub-questions, never from page text, so nothing it reads later changes what its planner had in view. Chained queries (DOIs from pages) carry it too.
3. **An item whose record cannot be read is every mark** (§2.1 item 4), here as at the sinks: a `known` claim the library does not hold, a library pointer the host view renamed, a private-read entry that names no record (a host's private noted entry), an unreadable private-read file. The notice may then say more than was in view; it never says less.
4. **The research record's `label` is still not stored** (slice A's decision 8 stands). `planned_with` is a notice about the queries, not a label of the record: 121 §4 derives a record's label from what it *used*, and what a span was only *given* is the `may_rest_on` note. The record's label is computed from its `private` flag when needed.
5. **The order of the marks** is the visible line's (confidential, private, private (library), internal, text kept), so AC-10's `["confidential", "internal"]` is the literal value.
6. **A host's own `search_kb` does not note what it was given** (121 §4 item 2 is built for daemon seats: `tools.rs`, `retrieval.rs`). A host asker's notice therefore comes from its private-read entries and the run's `known` list, plus any given list noted under `span_key(host-<session>, "host")`. Noting a host's results is not built here; it would change `host_search_kb`, which another slice is editing.
7. **The count is kept per run** on the record's run entry, since a topic keeps only its newest search log.
8. **The private-read mark is read in `service.rs`, not in the runner.** The 103 inventory test allows only listed files to name the mark module, and `service.rs` already reads it for `planned_under_mark`; the runner takes it as `AskerMark`. The notice only adds marks; the mark itself is unchanged by it.
9. **The runner's log test knows one more write** (`search_log_grants_nothing.rs`): `log.planned_with`, which sets the notice later lines carry and which nothing reads. No branch reads the log.
10. **`label.rs` gains `Labeller::item`**, outside this slice's file list: a given list holds handling item ids, and only `label.rs` may compute a label. `service.rs` and `handling_cmd.rs` gain the reply, the wiring and the status line, which AC-10 and §2.5 item 1 name.

## Not built here

- Noting a host's `search_kb` results as given (decision 6).
- The notice in `research_status`'s live `host_search.queries` (the queries are listed there before the run ends; the result and the log carry the notice).

## Not verified

- No run on the operator's workspace; no person read a reply with the notice.
- Whether a query actually carries a labelled item's substance is not judged (§2.5 item 2: a person can tell; Delphi cannot).
