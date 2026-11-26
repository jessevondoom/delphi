---
state: built
date: 2026-11-26
title: "Depth::Long: the brief, long_max_tokens, the seed as the draft, failure keeps the seed"
eval_waived: "no ranking term, search path, chunking or index change in this slice; the diff base counts knowledge-v2's own retrieval changes"
---

Slice C of spec 139, built 2026-11-26 on `build/139-c` (from `knowledge-v2`).

## What was built

- **`Depth::Long`** (`library_page_meta.rs`, `long`). A page the writer writes for a topic is at `long` (its frontmatter says `depth: long`). Search pages keep `summary` and `expanded`.
- **The brief** (`library_page_search.rs`). `brief_for` has a size line for `long` (`LONG_SIZE`): expand the draft, a lead of two to four paragraphs, `## ` sections that follow the draft's structure, 1,200 to 2,500 words, a `## Contents` list, a figure block where cited numbers or named parts support one. Key terms, start here and see also are asked for by `topic_brief` as before. The brief says no References section, as before. `draft_to_expand` puts the seed page after the records, under `DRAFT TO EXPAND`, as quoted data. It keeps the lines, drops control characters and defuses `END DRAFT` and `END RECORD`.
- **`[library.pages] long_max_tokens`**, default 8,000 (`config.rs`, schema, config reference, example). A topic rebuild's brief names it as the cap. The writer's draft is cut at it (`clean_draft`), and the engine's writer call for a topic page asks for that many tokens. `max_tokens` (3,000) still governs search pages.
- **`topic_rebuild`** (`library_topics.rs`):
  - It renders the seed page first. With no span, or no block that passes the seed check, it writes no page (`waiting for material` or `check_failed`, as slice B does) and asks no writer.
  - The writer is given `topic_brief` at `long`. Its RECORDS are the seed's spans as records, then what the search gathered that the seed does not hold, and the claims (`expansion_material`). The library that checks the page resolves the seed's spans too (`with_more_records`; `page_run` merges them before `with_records`).
  - **A failure keeps the seed** (`topic_keep_seed`): when the writer errors, its page ends as the claims alone after the allowed rewrite, its page cannot be checked, `per_day` is spent, or the share gate refuses. The seed page is then stored with outcome `seed` and the reason. If the stored page is already this seed (the same spans), it stays as it is: no new version, only its reason is updated. The error starts `the seed stands`. A refusal before the writer is asked also says `the build was not admitted`, so the idle rebuild keeps a queued expansion queued.
  - `claims_only_page` is no longer called from `library_topics.rs`.
- **§2.3 item 6**: each rebuild assembles the seed again from the stored documents (`page_run` already did). The previous page and its versions are read only for the change line and the version. The canary test below proves that no text of them reaches the writer.
- **§2.3 item 7**: the closing comment is taken from the first draft only. The rewrite's comment is taken off and ignored (the fix of #912 in `build_page_checked`, used unchanged). A test proves it for a topic page.
- **`page_run.rs`**:
  - `material` counts the seed's spans.
  - The idle expansion picks a seed page whose seed has spans.
  - The `Budget` cause is found inside a `the seed stands` error.
  - A not-admitted build does not unqueue.
  - `seed_after_research` now checks the seed with the library's held-text predicate (`lib.held()`) rather than `NoHeldText`.
- §2.3 item 8 (the page budget, `week_tokens`) is slice F's (§4) and is not built here.

## Acceptance criteria and tests

- **AC-5**:
  - `library_topics::expand_tests::a_topic_brief_asks_for_the_long_size_holds_the_seed_as_the_draft_and_is_cut_at_long_max_tokens`: the long size line, the seed page under `DRAFT TO EXPAND` after the records, the spans as records, `At most about <long_max_tokens> tokens`, a forty-paragraph draft cut at a 300-token cap, `depth: long`.
  - `library_topics::expand_tests::a_search_pages_brief_is_unchanged`: `brief` is `brief_for(.., Summary)`, with the summary size line, its own cap and no draft. The existing brief tests of `library_tests.rs` are unchanged and pass.
  - The key's default and parse: `config::tests::library_pages_defaults_and_ttl_forms`.
- **AC-6**: `library_topics::expand_tests::a_writer_error_a_failing_page_and_a_refused_budget_each_leave_the_seed_with_its_reason`. It covers a writer error (the seed is stored), a page that fails the check twice, the share refused and `per_day` spent. In each case the outcome is `seed`, the reason is on the record, the topic stays due with `SEED_EXPANSION`, and no new version is made while the spans are the same. A changed seed with a failing writer stores the new seed. The writer's page then lands.
- **AC-7**: `library_topics::expand_tests::no_brief_holds_text_of_the_previous_page`, the canary of §6. A sentence is planted in the page and in every version. It is absent from the brief and from the rewrite's text, and the brief holds the answer as it is now.
- §2.3 item 7: `library_topics::expand_tests::a_rewrites_closing_comment_is_ignored_and_the_first_drafts_stands`.
- Fitness (§6): `library_topics::expand_tests::topic_rebuild_never_calls_the_claims_only_fallback`.

## Builder decisions (to confirm)

1. **A seed never replaces a validated page.** §2.3 item 5 says the seed is "stored (or stays)". When the topic's page is a writer's page with outcome `pass` or `pass_contested` and a later rebuild fails or is refused, that page stands, nothing is written, and the topic stays due. The error starts `the written page stands`. A page of the claims alone, a seed, or no page at all is replaced by the seed. This is the stricter reading for §3, which shows untrusted text without a validator's pass. Test: `a_failed_rebuild_never_replaces_a_validated_page_with_the_seed`.
2. **No span, no writer.** A topic whose seed is empty gets no page, even when the library's own search found claims or sources (§2.2 item 4, read strictly). Before this slice the writer was asked whenever anything was gathered.
3. **"Cut at `long_max_tokens`"** is read as the page's cap: the brief names it, the draft is cut there at a paragraph, and the writer's call asks for that many tokens. The brief itself is not cut, since the seed alone may be 64 KiB.
4. **A failed rebuild returns `Err`** even when it stored a new seed page. The page is in place, and the error says why the writer's page did not land. This matches slice E's handling of `the seed stands`.
5. The writer's RECORDS are the seed's spans first and then the gathered records the seed lacks (one label is one passage, so the seed's longer span text wins).
6. A seed page's frontmatter keeps `depth: summary`, as slice B left it. Only a writer's page says `long`.
7. Tests that hand `topic_rebuild` its material directly now give it the seed that material makes (`topic_seed::of_material`, test only). Tests that counted on the claims-only page now expect the seed:
   - the sections test's failing see-also;
   - the idle rebuild test (the writer is asked and gives nothing);
   - `pages.rebuild` through the bridge (reported `failed`, `the seed stands`);
   - the app's `app.page_rebuild` (ends `failed`, the seed page in place);
   - the encyclopedia read test (its second rebuild follows a new claim, so the first seed is a version).
8. A failed rebuild counts against `per_day` once the writer was asked, as before. A seed written because the writer failed is in that count; a seed written with no writer asked is not.

## Not verified

- No live run: no real writer has expanded a seed. Whether one call writes a good 1,200 to 2,500-word page that passes the validator is not observed (§7).
- The validator's cost on a long page, and the share it spends, are not measured. The page budget of §2.3 item 8 is slice F's.
