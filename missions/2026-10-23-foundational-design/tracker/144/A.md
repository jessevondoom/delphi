---
state: built
date: 2026-11-27
title: "each statement carries its own citation: one parsed form, every unit judged, pinpoints from source text, conflicts stated"
eval_waived: "the commit with this entry changes how a topic page is parsed, briefed and validated, not ranking, chunking or the index"
---

Built 2026-11-27 on the branch `build/144-s0-a`, after slice S0 (based on `knowledge-v2`; no pull request). Topic pages only: a search page is briefed, checked and validated as before (`specs/057`).

**One parsed form (§2.2 item 1).** `library_page_units.rs`: `parse` reads a draft once into units (heading, sentence, list item, table row, figure caption, key-term line), numbered in page order, each with its own markers and the heading it sits under. A sentence ends at `.`, `!` or `?` before a capital (abbreviations and initials kept), and takes the markers that follow it. The same parse is what the page renders and what the validator's contract lists. What it cannot place fails with `a construct the page check cannot read`: raw HTML, an image whose text is more than a label (over `IMAGE_LABEL_WORDS` = 8 words, or a sentence), a footnote, a fenced block.

**The closed marker rule (§2.2 items 1 and 2).** Every unit but a heading needs a marker of its own (`a statement with no citation`); a marker covers only what it ends. A lead-in ending in `:` before a list or table needs none only when it is on `LEAD_INS` (24 fixed phrases, "The main approaches are:", "It has these parts:" and the like); any other needs one (`a lead-in that states a fact`). Code writes the hatnote, `## Contents` and `## See also` (`strip_code_written` takes a draft's own out before the parse; `code_written` writes them after a pass; `topics::navigation` picks the hatnote, when another topic's title extends this one's, and `See also`, the other topics whose titles share a content word, at most 8). The topic brief (`Depth::Long`, `topics::topic_rules`) states the rule, the lead-in list and the rule text, and no longer asks for a hatnote, contents or see-also.

**What the validator sees (§2.3 item 1).** `Library::topic_material`: for a cited source, the same cut the writer's brief held (`Gathered::sources`), as `SOURCE TEXT`; for a cited claim, `CLAIM TEXT` and, apart, the paragraph of each anchored source's capture its quote is in; a finding as `FINDING TEXT` and a summary page as `SUMMARY TEXT`, with no source text; a passage of a stored document as source text. Each source part keeps the stored capture it was cut from. Cited records come first; one left out for length is named (`NOT SHOWN`). The topic's claims and the writer's claims follow, as the records a statement may contradict.

**Every unit gets a verdict (§2.3 item 2).** `unit_contract` lists every unit by number (`UNIT <n> | <kind> | <text>`) and asks one line per unit, `UNIT <n> | <verdict> | <record> | <passage>`, one line per assertion for a compound unit; `none` is a heading's verdict for one that states no fact. `classify_units`: a unit with no verdict is `unchecked`; a compound unit is `supported` only if every line is. The rule text of §2.3 item 3 is in both the brief and the contract, word for word (`page_units::RULE_TEXT`).

**Pinpoints (§2.3 item 4).** `page_units::pinpoint`: a `supported` line counts only when its record is one the unit cites (a heading: one its section cites) and its passage lies in that record's source text as shown (never in claim, finding or summary text), is found again in the stored capture, is whole sentences of one paragraph there, and takes in the sentence before or after it when that sentence carries a qualifier the statement drops (`not`, `no`, `never`, `only`, `unless`, `except`, `without`, `if`, `when`, `provided`, a word ending `n't`, or a number). Otherwise the verdict is `unchecked`.

**Conflicts, and `unchecked` (§2.3 items 5 to 7).** A `contradicts` verdict naming a shown record fails the attempt (`a conflict the page does not state`) unless some unit of the page cites that record beside another one; then the unit is contested and the page `pass_contested`. On a topic page an `unchecked` unit fails the attempt, which then follows 057 §3.4 (one rewrite with the findings, then the seed stands). `END ASSERTIONS` is kept; the reply is sized to the units (`PageQuery::max_output`, 90 tokens a unit within the same floor and cap).

Verified (tests):
- AC-5: `library::page_units::tests::every_unit_but_a_heading_needs_its_own_marker` (each unit class, a marker covering only what it ends, the fixed and the factual lead-in), `code_writes_the_hatnote_contents_and_see_also`; `library::page_validate::topic_unit_tests::the_validator_judges_the_units_the_page_renders_and_the_check_is_closed` (a sentence without its own marker fails a topic page; the same draft passes as a search page) and `a_compound_unit_a_strengthened_statement_and_an_unsupported_heading_fail` (the heading); `library::topics::section_tests::a_topic_page_keeps_its_sections_open_questions_and_links` (code's `See also`, the writer's taken out).
- AC-6: `topic_unit_tests::the_validator_sees_what_the_writer_saw_and_a_record_left_out_is_named`.
- AC-7: `topic_unit_tests::the_rule_text_is_in_the_writers_brief`, the contract in `a_compound_unit_a_strengthened_statement_and_an_unsupported_heading_fail`, with a planted strengthened statement ("may dredge" written "dredge") judged not supported.
- AC-8: `library::page_units::tests::a_pinpoint_is_whole_sentences_of_source_text_with_the_qualifiers_it_needs` (the cross-sentence fixture), `topic_unit_tests::a_supported_verdict_counts_only_with_its_pinpoint_passage`, `a_passage_from_claim_text_does_not_count`.
- AC-18: units listed and judged as rendered, no verdict, compound, raw HTML, a descriptive image text (the tests above), `topic_unit_tests::a_conflict_the_page_does_not_state_fails`, `library::page_units::tests::a_construct_the_parse_cannot_place_fails`.
- The topic tests that existed (`expand_tests`, `rebuild_tests`, `topic_gather`, `label_tests`) now run through the unit contract with `page_validate::support_all`, a validator that supports every unit with its first cited record's source text.

Builder decisions where the spec is silent:
- **Adjacent means within the paragraph.** The qualifier test reads the sentences before and after the passage in the same paragraph of the capture (a line is a sentence boundary too); a heading or a new paragraph is not an adjacent sentence.
- **A conflict is stated** when one unit cites the contradicting record and at least one other record; the contradicted unit is then contested, as 057's contested pass.
- **Headings** answer `none` when they state no fact; `none` on any other unit is `unchecked`. A heading's records are those its section's units cite (the whole page's, when its section cites none).
- **A finding or a summary page has no source text**, so a statement resting on one alone cannot be `supported`; it can be cited beside a passage that is.
- **The hatnote and `See also`** are chosen by code from the topics' titles only (§2.2 names "the title and its links"; the link table is written from the page, so it cannot choose the page's own links).
- The image-text limit (8 words), the lead-in list and the qualifier words are fixed in code here; the trial (slice E) may tune them.

Not verified: a live writer and validator through the new contract (the trial's job, slice E); how often a real validator gives passages that pass the pinpoint test; the cost of the longer contract.
