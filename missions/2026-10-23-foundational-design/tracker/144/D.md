---
state: built
date: 2026-11-27
title: "pages built from evidence: merged evidence, the seed as outline, a label from all material, unrequested builds off by default"
eval_waived: "the commit with this entry changes what a topic page's writer and validator are given, how a page's label is joined at exits, and when page builds run, not ranking, chunking or the index: every search returns what it returned before (no search returns topic pages)"
---

Built 2026-11-27 on `build/144-d` (based on `knowledge-v2` after S0, A and C1; pushed, no pull request). Spec 144 §2.7, AC-14, AC-15, AC-17, AC-19.

**Pages from merged evidence (§2.7 item 1).** `Library::merge_packs` merges the evidence packs of every research topic a page follows into the writer's evidence. Each pack is built again from its record and the stored text (`evidence_pack::build`), so it is the pack as it is now. Each entry's claim joins the claims. The paragraph of its source's stored text that holds the quote joins that source's cut (`PACK_PASSAGE_MAX` = 1,536 characters a passage, `PACK_CUT_MAX` = 6 KiB a source), which is what the writer sees and what the validator judges pinpoints against. A withheld pack (a run that read a private source) is not merged; a source that is private, gone or without citable text gives no passage. Findings and knowledge-base passages come as before (`topic_gather`). The rebuild still starts from the seed assembled again; the page it replaces is never in a brief (`expand_tests::no_brief_holds_text_of_the_previous_page`, 139 §2.3 item 6, still passes).

**The seed as outline (§2.7 item 2).** `topic_seed::outline` turns the seed page into the `DRAFT TO EXPAND`: its headings and blocks in order. A research answer's block is cut into its paragraphs; each takes as its marker the claims its own markers name (`claim_ids_in`) that the writer was given, and the claim marks in its prose are taken out. Any other block keeps its span's marker when the writer was given that record. Text that rests on no such record keeps its words and no marker. `OUTLINE_NOTE` now says the draft is an outline and an unmarked block supports nothing.

**The validator (§2.7 item 3).** A's `topic_material` already showed only cited records, with the cut the writer saw, plus the topic's claims. A cited record could still be any library source, though. Now `topics::writer_evidence` lists the writer's records, and a marker naming any other fails the page check before the validator is asked (`page_validate::NOT_GIVEN`, "cited a record the writer was not given"). The topic's claims the validator is shown beyond the writer's claims are kept as `g.contradicting`.

**A page's label from all its material (§2.7 item 4).** A written page's sidecar now names `evidence_packs` (the packs' logical paths), `outline` (the seed spans given as the outline) and `contradicting` (the claim ids). A seed page keeps none of them. `Labeller::page` joins all three:
- the packs through `Labeller::path`, so through `evidence_pack::label_of`;
- the outline's spans like a seed page's;
- the contradicting claims as claims.

For each research topic followed it now joins `evidence_pack::run_label` instead of the record's own flag. That is the record's flag and every source its runs read or its claims rest on, quoted or not, because the answer in the seed may paraphrase any of them. `at_sink` recomputes this from the records at every exit, so a source made private after the build makes the page private at the next exit check, and a bundle, export or push of it is refused.

**Unrequested builds off by default (§2.7 item 5).**
- `[bridge.jobs] pages` now defaults to `manual` (`BridgeJobs` has its own `Default`), and `[library.pages] auto_rebuild` to `false`.
- `build_after_research` stays `true`.
- The precedence was already `job_switched_on`'s: bridged idle needs both, API idle needs `auto_rebuild`, and a command (`delphi pages rebuild`, the app's button) runs unless the bridge says `off`.
- Research queuing (§2.5 item 6): `Engine::expansion_may_queue` queues the expansion only when the switches allow an idle build, the page budget has room for a long page's reserve (`week_spent` plus `page_reserve(Long)` within `week_tokens`), and `Library::topic_has_citable_evidence` holds. That means an excerpt span, a passage or research file whose text passes `citable_text`, or a pack entry whose source has citable text.
- The config docs (`config.example.toml`, `schemas/config.schema.json`, `docs/reference/config.md`) give the new defaults. No command changed.

Verified (tests):
- AC-14: `library::topics::evidence_tests::a_topic_page_is_written_from_merged_evidence_with_the_seed_as_an_outline` checks four things. The pack brings a source the library's own gather does not find, as a record with its paragraph. The outline re-points the answer's marker to its claim and leaves the unmarked paragraph without a marker. The validator is shown only the writer's evidence and the contradicting claims, with the same cut. The sidecar names the pack and the outline. Also `a_marker_naming_a_record_the_writer_was_not_given_fails_the_page`, and `expand_tests::no_brief_holds_text_of_the_previous_page` (unchanged).
- AC-15: `page_run::tests::with_the_new_defaults_no_unrequested_build_writes_a_long_page_and_a_rebuild_asked_for_does` checks that the seed is written after research, nothing is queued, the idle rebuild does nothing (inside the page job too) and no model is asked, while `pages rebuild` asks the writer. `delphi_adapter` `library_pages_defaults_and_ttl_forms` and `bridge_jobs_default_to_auto_and_take_only_their_three_modes` check the defaults.
- AC-17: `evidence_tests::a_source_reclassified_after_the_build_changes_the_pages_label_at_the_next_exit` (a source the run read and the page does not cite, made private, makes the page private at `at_sink`, and bundle, page export and push refuse it); `a_page_that_paraphrases_private_seed_material_while_citing_only_a_public_source_is_private` (a private outline span, a pack of a run that read a private source, or a followed topic whose run read one, each make a page citing only an open source private).
- AC-19: `page_run::tests::a_bridged_build_needs_both_switches_an_api_build_auto_rebuild_and_a_rebuild_asked_for_runs` (every combination) and `research_queues_a_page_job_only_with_citable_evidence_and_budget_left`.
- The tests of idle page builds (`page_run`, `service`, `summary`) now turn the two switches on (`testkit::UNREQUESTED_PAGES_ON`). `record_research_writes_the_seed_page_at_once...` now expects no queued expansion for an answer alone. `label_tests::no_label_and_a_stale_label_are_every_mark` renames a claim, so the other page's validator is not shown it as a contradicting record.

Builder decisions where the spec is silent:
- **Packs are read by code, not searched.** The pack is built from the record for each followed topic, which is the same content as the file, and fresher. A pack's entry reaches the writer as its source's passage (`s-<id>`) and its claim, not as a record of its own. That way the pinpoint resolves in the stored capture, as A's validator requires.
- **What a block "rests on":** the claims its own markers name (S0's rule). A research answer that names no claim gives no marker; no text-matching to claims is attempted.
- **"Citable evidence" for queuing:** a source excerpt, a passage or research file whose text passes `citable_text`, or a pack entry on a citable source. A finding, a summary, a claim alone or an answer is not.
- **The page budget for queuing** is the week's (`week_tokens` with a long page's reserve). `per_day` is left to the build itself.
- **The run's label for a followed topic** (`run_label`) differs from the pack's `label_of` in one way only: a topic that read no source is the record's label, not every mark.
- **Marker to a record the writer was not given** fails the topic page's check (a hard finding, before the validator), rather than being dropped from the validator's material.

What this changes for what agents and exits see, plainly:
- **Retrieval** returns what it returned before. Topic pages are not in the index and no search returns one, so the label-at-search part of §2.7 item 4 has no search to act on today. The one reader, `pages.get`, already recomputes with `at_sink`.
- **Exits** are stricter. A page following research whose runs read a held or private source now carries that mark at exits, even when the page cites none of it.
- **Existing pages read as stale until relabelled.** `Labeller::page` now computes other facts, so a topic page built before this slice has a fingerprint that no longer matches. It is every mark at sinks, and due, until `delphi handling relabel` or a rebuild.

Not verified: a live writer given packs and an outline (the trial's job, slice E); how often real research answers carry claim markers to re-point; whether the outline's unmarked prose still leads writers to paraphrase it.
