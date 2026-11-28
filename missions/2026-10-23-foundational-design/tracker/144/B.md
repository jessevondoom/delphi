---
state: built
date: 2026-11-28
title: "what agents see: answers and packs labelled from every source their run read, private material out at every search, held text for every role behind 145's switch"
eval_waived: "this slice changes what retrieval returns, not ranking, chunking, fusion or the index: a research answer is now labelled `research summary · not evidence`; an answer or record of a run that read held text comes back at `external_untrusted` and only to a search that admits held text; anything of a run that read a private source (or cannot be traced), and a library source made private since the sync, is withheld at the search. The golden set has to be run again after this lands (spec 144 §2.8 item 1.3), because cases whose relevant answer came from such a run lose it; that is the trial's step, run on a real install"
---

Built 2026-11-28 on `build/144-b` (from `knowledge-v2` after C1 and D; pushed, no pull request; the lead integrates). Spec 144 §2.6, AC-9, AC-10.

**This slice changes what retrieval returns.** Plainly:
- A research answer (`answer.md`) is labelled `research summary · not evidence` (it read `not validated`).
- An answer or record of a run that read held text, quoted or only paraphrased, is returned at `external_untrusted`, marked `text_held`, and only to a search that admits held or untrusted text. Under the ratified table (in force) a builder, orchestrator, messenger, investigator or writer no longer sees such an answer in its default search; the roles that do not act still do, and their span is tainted by it.
- Anything of a run that read a private source, or that cannot be traced, is withheld at the search, including a topic marked private in its own project's search (`specs/115` showed it to its own project).
- A library source made private since the last sync is withheld at the next search, before any refresh.
The golden set must run after this (§2.8 item 1.3).

## What was built

**The view, computed at every search, never stored** (`research_index.rs`):
- `run_file(zone, rel_path)` names a research topic's file: the workspace's `research/topics/<id>/<file>` (zone `research`) or a registered project's `projects/<p>/files/research/topics/<id>/<file>` (zone `project`). Kinds: pack, answer, other (the record, a project topic's other Markdown).
- `Trace` is what a search traces its results to: the workspace and library of the index's own path (`<ws>/local/state.db`). `Trace::shown` computes the label from the records now: a pack by `evidence_pack::label_of_topic`, an answer or record by `evidence_pack::run_label_of_topic`. That is every source the run read, quoted or not, and every source its claims rest on, with the record's own private flag. Withheld (`library_private` or `personal`, or every mark when untraceable) gives `None`. `untrusted` is true for a pack, and for any file of a run whose label carries `text_held`.
- `evidence_pack::label_of_topic` and `run_label_of_topic` are the C1/D functions taking the topic's folder, so a project's topic is labelled from its own folder (its id may equal a workspace topic's). The record's facts are computed with the same line, so D's page fingerprints are unchanged.
- `Trace::source_withheld`: a `library/sources/<id>/…` row whose source is private now.

**Kb search** (`kb.rs` `search_staged`): one `Trace` per search, the labeller made at the first result that needs it. Before any row of a run file or a library source is returned:
- withheld: skipped;
- untrusted: its tier becomes `external_untrusted`, and it is skipped unless the search sets `include_held` or `include_untrusted` (and any `source_types` list admits that tier);
- labelled: a pack `evidence pack · not validated`, an answer `research summary · not evidence` (`research_index::ANSWER_LABEL`), with `Hit::marks` the label just computed.
This replaces C1's pack-only `search_label` check.

**Host paths:**
- `host_labels::of_hit_with` keeps an answer's and a pack's label in the host's passage.
- `mcp_project::Project::files` (a host's word-ranked project search, `search_kb`'s project leg, `kb_retrieve`'s project leg, resources) drops a project topic's files whose run is withheld (`research_index::visible_project_files`).

**Held text for every role, behind 145 A's switch** (`retrieval::held_for(table, role, write, execute)`). Under `TaintTable::WhatLeaves` every role's default search and start passages are given held text. Under `Ratified` it is `held_by_default`, 142 B's rule, unchanged. The engine passes `Settings::taint_table` at every call (`engine.rs` ×3, `align_run.rs`, `strategy_trace.rs`); nothing else turns it. `TAINT_TABLE` is still `Ratified`.

**Model calls (§2.6 item 4):** nothing changed for them.

## Search paths covered

- `kb_search` (engine tool): through `Kb::search`.
- Span start passages: `retrieval::span_start`, through `Kb::search`, tier from the hit.
- `kb_retrieve` (engine pointer reads) and the host `kb_retrieve`, `search_kb`: index legs through `host_labels::search` → `Kb::search`; project legs through `Project::files`.
- The page writer's gather (`topic_gather`), known-first (`research_known`), host recall: all through `Kb::search`.
- Topic pages: **not indexed** (`library/pages/` is listed by no source of `kb::sources`; the test checks it), so no search returns one. `pages.get` already recomputes with `at_sink` (D).
- `kb serve` and `Kb::documents`/`document` are the operator's HTTP view, not an agent span's search, and are unchanged.

## Tests

- AC-10: `evidence_pack::tests::answers_and_packs_are_labelled_and_an_answer_of_a_run_that_read_held_text_is_untrusted_and_taints_its_span`. An open run's answer is labelled and at the research tier with no mark, and its pack is labelled and untrusted. A run that read held text and only paraphrases it: its answer and record are absent from a search without held text; with held text they are `external_untrusted` and `text_held`. A builder's start passages do not carry it under the ratified table; under 145's table they do, and the span starts at `L0`.
- AC-9 and AC-10: `evidence_pack::tests::no_search_returns_a_private_source_or_an_answer_record_or_pack_of_a_run_that_read_one_or_cannot_be_traced`. A source the run read but does not quote is made private, and the next search, before any refresh, returns nothing of the run, and neither do span start passages. The quoted source made private is itself withheld. A record that cannot be read withholds its answer. An index outside a workspace returns nothing of any run. No page is indexed.
- AC-9, the switch: `tools::tests::spec_144_with_the_switch_on_every_roles_default_search_returns_held_text_and_off_142_bs_rule_stands`. Every shipped role is on under `WhatLeaves` and equals `held_by_default` under `Ratified`. For builder, orchestrator and scout, the default `kb_search` returns the held passage labelled exactly when the switch gives it, and the span is tainted then. Under `WhatLeaves` the tainted span keeps its worktree write.
- Host: `mcp_project::tests::spec_144_host_reads_label_answers_and_return_nothing_of_a_run_that_read_a_private_source`. `kb_retrieve` labels a workspace answer. Once its source is private, the next call has nothing of it, and a project topic that read the source is gone from `kb_retrieve`, `search_kb_project` and `search_project`, while an open one stays.
- 142 B's tests (`kb_search_gives_held_text_…`, `a_span_given_held_text_cannot_promote`, …) pass unchanged. So do the exit tests and `every_sender_is_listed_and_every_exit_asks`.

Existing tests changed on purpose:
- `research_index::tests`: the fixture's run now reads a source the library holds (a run of an unknown source is untraceable, so withheld), and the answer's label is `ANSWER_LABEL`.
- `shared_research::tests::a_claim_on_a_private_source_crosses_…`: A's own private topic is no longer returned to A's search (§2.6 item 3).
- `research_known`: the operator's known-first lookup now sets `include_held`, so the console still finds and continues a topic whose run kept a page of unknown licence (`service::tests::a_question_that_overlaps_a_topic_continues_it…`).

## Decisions where the spec was silent

1. **The record is the run's too.** `record.json` lists what the run searched and read and its claim ids, so it takes the answer's taint and withholding. It keeps its own label (`not validated`).
2. **"The lowest taint" is `external_untrusted` when any source the run read is held**, otherwise the tier the answer was indexed at. Library text is at one tier except held text, so no other tier applies.
3. **An untrusted answer follows held text's admission:** returned only when the search asks for held text (`include_held`, the default for roles that do not act and, with the switch on, for all) or for untrusted text.
4. **Untraceable = withheld** for run files: no workspace known, a record that cannot be read, a source whose record cannot be read, a project that is not registered. For library rows, a source whose library record is gone is left to the next sync (which drops it); a source that exists and is private is withheld.
5. **A private topic is withheld from its own project's agents too.** `specs/115` let a project read its own private topic; §2.6 item 3 says no agent span. Claims are not answers, packs, sources or pages: the internal-source claims of 115 are unchanged.
6. **Withheld for every search**, the operator's console search included, as C1 does for packs. The operator reads private material through the library's own commands (141 F).
7. **The operator's known-first lookup takes held text.** Without it, a topic whose run read a held page could not be continued.

## Not verified

- No live run. How many real answers come from runs that read held text, and so leave the default search of the acting roles under the ratified table, is not measured.
- Cost: a search that returns run files or library rows builds one labeller (it reads the handling records once) and reads each topic's record and its sources' metadata. This is not measured on a large install.
- The engine passing `Settings::taint_table` to `held_for` is tested through `held_for` and the tool, not by an engine run with the switch on.
