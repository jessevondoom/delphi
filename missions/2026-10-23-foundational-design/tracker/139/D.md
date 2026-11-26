---
state: built
date: 2026-11-26
title: "titles: subject, the writer's title, the derived title, pages rename, pages retitle"
eval_waived: "no ranking term, search path or index changes in this slice; the diff base counts knowledge-v2's own retrieval changes"
---

Slice D of spec 139, built 2026-11-26 on `build/139-de` (from `knowledge-v2`).

## What was built

- A topic's record has `title_source`: `operator`, `subject`, `writer`, `derived` or `question`. A record without it reads as `operator` when the operator made the topic, else `question`.
- A topic made from research gets a concept title (`research_title`): the research's `subject` when code accepts it, else the derived title, else the question. The question stays the topic's `query`, which a rebuild gathers with.
- The derived title (`derived_title`): the lead-ins of the closed list are removed from the start (again while one is left), then the closing question mark. The first letter is made a capital. It is refused, and the question is used, when nothing is left, a question mark is still in it, or it is over 80 characters.
- The check for a `subject` or the writer's title (`accept_title`): at most 80 characters, no question mark, no control character, no lead-in at the start, and at least one content word shared with the question or a heading. Content words leave out a short list of stop words.
- `record_research` takes an optional `subject` (at most 80 characters; the schema file and the MCP tool's description say so). The daemon checks it against the question and the answer's headings. An accepted one is kept in the research record as `subject`; the reply says `subject: {taken, title}` or `{taken: false, reason}`.
- The writer's closing comment may carry `title=` (`Proposal::title`). A topic's brief asks for it only while the writer may give one. It is taken at the topic's first written page (none before, or only a seed or the claims alone), only for a title made by code, and only when `accept_title` takes it against the question and the seed page's headings. The page keeps its slug. The record then has `written: true`.
- `delphi pages rename <slug> <title words...>`: the operator's title, which always wins. Refused inside an agent seat. A topic with `version` 0 takes the slug of its new title; one with a page keeps its slug. A slug another topic has is refused.
- `delphi pages retitle [--apply] [--json]`: for each topic whose title code made, the title code would give it now and its slug. It prints first and writes only with `--apply` (refused inside an agent seat). Two topics that would take one slug, or a slug another topic has, are printed as a collision and left as they are; `--apply` then exits 1.
- The `CMDS` entry and the usage list name both commands.

## Acceptance criteria and tests

- AC-8: `host_research::tests::a_subject_titles_the_topic_and_a_question_or_an_unrelated_subject_is_refused`.
- AC-9: `pages_cmd::tests::pages_retitle_prints_first_applies_only_with_apply_and_refuses_a_collision`.
- The rules themselves: `the_derived_title_drops_the_lead_in_and_the_question_mark`, `a_concept_title_is_taken_only_when_it_shares_a_word_and_is_no_question`, `the_writers_title_is_taken_only_at_the_first_written_page_and_keeps_the_slug`, `rename_gives_the_operators_title_and_moves_only_an_unbuilt_topic` (all in `library_topics.rs`).

## Builder decisions

- "No written page" is read as `version` 0, as §2.4 item 4 says. A topic with only a seed page has a version, so it keeps its slug. Slice E writes the seed at once, but a topic made after this slice already gets its concept slug when it is made.
- The derived title starts with a capital letter. The spec does not say.
- A `subject` over 80 characters refuses the whole call, as the schema does. A subject that fails the other rules is only not taken.
- The subject is checked when it is recorded (against the question and the answer's headings) and again when the topic is made.
- A writer's title on a page that ends as the claims alone is not taken.
- `retitle` takes a research record's `subject` before the derived title.

## Not verified

- No run on the operator's workspace. `delphi pages retitle` there is the operator's step.
- No real writer model has been asked for `title=`.

## Operator rulings, 2026-11-26

These replace two of the builder decisions above.

- "No written page" means no writer's page. `rename` and `retitle` re-slug a topic whose pages are all seeds (outcome `seed`, now and in its versions), not only one at `version` 0. Its record, seed page, versions folder and row of the link table move to the new slug. The seed page's frontmatter and heading take the new title. A topic with any other outcome, now or in a version, keeps its slug.
- Old slugs become aliases. A re-slugged topic keeps its old slugs in `aliases`. A `page:<old slug>` link passes the page check (`topic_link_names`), and `pages topic`, `pages versions`, the app's `pages.get`, `pages.versions` and `pages.links` find the topic by it (`topic_resolve`). Other pages' text is not rewritten. A topic's own slug wins over an alias.
- Builder decision: a new topic whose slug is another topic's alias takes the next free slug (`<slug>-2`, then `-3`). A `rename` onto another topic's alias is refused and names that topic. `retitle` prints it as a collision. A topic may take back its own old slug.
- An over-long `subject` no longer refuses `record_research`. The schema keeps `maxLength: 80`. The daemon records the research, does not take the subject, and the reply says `{taken: false, reason: "it is longer than 80 characters"}`.
- Help: the `pages` detail and the `rename` output say so.

New tests:

- `a_seed_only_topic_is_re_slugged_and_its_seed_versions_and_links_move` (`library_topics.rs`).
- `a_topic_with_a_writers_page_keeps_its_slug` (`library_topics.rs`): a writer's page now, or only in a version.
- `an_old_slug_resolves_as_an_alias_in_a_page_link_and_in_the_topic_lookups` (`library_topics.rs`): the see-also check, `topic_get`, versions, links, `pages.get`, the next free slug, the refused rename.
- `pages_topic_finds_a_renamed_topic_by_its_old_slug` (`pages_cmd.rs`).
- `an_81_character_subject_records_the_research_and_the_reply_says_the_subject_was_not_taken` (`host_research.rs`).

The two older tests that faked "a page" with `version` 1 now give it a writer's outcome.
