---
state: built
date: 2026-11-26
title: "the sender inventory, then rule (a): local_text, held text in the index, body_may_leave at every exit (AC-9 blocks its merge)"
---

Built 2026-11-26 on branch `build/142-b` (from `knowledge-v2`). Not merged.

## What was built

- **`[library] unknown_licence = "local_text"`**, now the default (`config.rs`). `pointer`, `excerpts` and `private_text` keep their meanings. `delphi research setup` offers it first.
- **Rule (a).** Under `local_text` the research keep step (`research_host.rs`) sets `AddOpts::keep_closed_held` unless the page shows a rights notice or its trusted-list entry allows a summary only (§2.5 item 6). The library keeps such a page whole, marks its record `text_held: true`, and its capture record says `rights: held`. It is not private.
- **`library::is_held(meta)`**: the mark, or, deny by default, any public full-text source under a licence that is neither open nor the operator's own and was not adopted from a bundle.
- **Held text in the index** (`kb.rs`): a held source's `source.md` is indexed at `external_untrusted`. Other library text keeps its tier.
- **`held_text.rs`** (new): the one predicate.
  - `body_may_leave(text, output)`: no run of more than 300 characters, and at most `min(1,200, a tenth of the source)` characters in all shared runs of 40 characters or more. Whitespace is compared collapsed.
  - `shorten`, `fit` (drop the last blocks that quote, for output code made), `check` (refuse, for output a model or a person made), `cut_field` (a citation field).
  - The **block index**: the hash of each consecutive 64-character block of a held source's collapsed text, one file per source in `library/held-blocks/`. Written by `Library::store`; removed by eviction (`remove_bodies`) and by `purge`. A missing or stale file is rebuilt in memory when the index is read, so a read writes nothing.
  - `LibraryHeld` implements `topic_seed::HeldText`, so the seed page of 139 is checked with it. `NoHeldText` stays for tests only.
  - `SENDERS`: the inventory below, with a test that fails when an exit stops asking or a new sender appears.
- **`.with_text_cache`**: held text is stored only by the research keep step, which already builds its library with it. No other path stores held text in this slice.

## The sender inventory (141 §2.4 item 8, done here first)

Every way text can leave the machine, as found on 2026-11-26. The table in code is `held_text::SENDERS`; the scan covers `crates/` and `app/src-tauri/src`.

Exits that ask the predicate:

- (a) The library's exit check `may_leave` (`library.rs`) refuses a held body on every channel that carries one: Bundle, Xenia, DebugBundle, Log, Relay, Connector, Mail, Outbound, Telemetry, History. It lets the lookups (an address or a DOI only), the web search (tags only) and the citation export pass.
- A bundle's `sources/` copy (`library_export.rs`): refused for a held source.
- A bibliography (`library_bib.rs`): the citation is sent; title, container title, attribution, abstract and note are cut to the bound.
- (b) `kb seed-export` (`kb.rs`): no vector of a held source's chunks.
- (c) `kb serve` over HTTP (`kbserve.rs`): no search hit and no document of a held source. The operator's view through the socket is unchanged.
- (d) Pages: the mechanical check (`library_page_check.rs`, finding `HeldText`); the claims-only page, cut to fit (`library_page_validate.rs`); a topic page of any outcome as it is written (`library_topics.rs`: seed and claims-only cut, a written page refused); a search page as it is written (`library_page_search.rs`); the seed page (`page_run.rs` passes `lib.held()`); `delphi pages show | search | expand` refuse to print (`pages_cmd.rs`).
- (e) A bundle: `delphi bundle package` reads the bundle's files as one output before the bundle crate is called (`bundle_cmd.rs`); a bundle published through Xenia the same (`bundle_xenia.rs`).
- (f) The document path of 095 (`artifacts.rs`): every artifact tool's arguments, the bytes it writes, and an archive's members.
- (g) The daemon's push (`worktree.rs` `open_pr`): the files of the branch's range, read as one output, before the push.
- (h) The app: `library.source` and `pages.get` (`encyclopedia.rs`) say `text_held` and `may_export`; the window hides export and print of a page that may not leave, refuses a copy of more than 300 characters of a held source, and does not print its text (`app/src/screens/Encyclopedia.tsx`).
- A Xenia publish of a file (`service_xenia.rs`, `xenia_cmd.rs`).

Senders that carry no held text, or are not sinks under 141 §2.4 item 1:

- Model calls (`http.rs`, `model.rs`): not a sink (ruling 1).
- Research fetch, search, pictures, planner, runner, trusted lists, fact fetcher, source classes, boot settings: not a sink (ruling 2), or they parse addresses only.
- Outbound, Connector, Relay, Mail (`harness_run.rs`, `connector_run.rs`, `private_read.rs`, `relay/net.rs`, `relay/telegram.rs`): held by the private-read mark as today; their `Q` column is 141's.
- Library acquisition, archive lookup, citation chaining: an address, a DOI or tags only.
- `oauth2.rs`, `selfupdate.rs`, the sandbox proxy, the app's bridge: nothing of the library.
- `bundle_pack.rs`: its one production caller asks first.
- `pinned.json`: read by no sender. It is not indexed, not in a bundle copy (export writes `meta.json`, `source.md`, `excerpts.json` and assets only), and each pinned excerpt is at most 300 characters.
- Debug bundle, logs, telemetry: no entry text (their proofs in `egress.rs`).

## Acceptance criteria and tests

- **AC-8**: `research_host::tests::local_text_keeps_a_page_of_unknown_licence_whole_and_held_unless_it_shows_a_rights_notice`; the default in `config::tests::unknown_licence_defaults_to_local_text_and_its_neighbours_parse`.
- **AC-9** (blocks merge), one test per exit:
  - registered channels and a bundle's `sources/`: `held_text::tests::a_held_source_never_leaves_by_a_bundle_copy`
  - a bibliography: `held_text::tests::a_bibliography_sends_a_held_sources_citation_and_no_run_of_its_text`
  - a page draft: `held_text::tests::a_page_draft_that_quotes_a_held_source_past_the_bound_fails_its_check`
  - a page of every outcome: `held_text::tests::a_page_of_every_outcome_is_cut_or_refused_before_it_is_written`
  - a seed page (139 AC-18): `held_text::tests::a_seed_page_built_by_the_daemon_passes_body_may_leave_for_a_held_source`
  - a page printed: `pages_cmd::tests::a_page_that_carries_a_held_source_past_the_bound_is_not_printed`
  - a bundle's files: `bundle_cmd::tests::a_bundle_whose_files_quote_a_held_source_is_not_packaged`
  - a Xenia publish: `held_text::tests::a_file_that_quotes_a_held_source_is_refused_at_every_file_exit`
  - a document and a deck: `artifacts::tests::a_document_or_deck_that_quotes_a_held_source_is_refused`
  - the daemon's push: `worktree::tests::a_push_whose_range_quotes_a_held_source_is_refused_and_nothing_is_pushed`
  - `kb seed-export`: `held_text::tests::seed_export_writes_no_vector_of_a_held_sources_chunks`
  - `kb serve --allow-remote`: `held_text::tests::kb_serve_returns_no_chunk_of_a_held_source`
  - the app: `held_text::tests::the_apps_views_say_a_held_source_may_not_be_exported_and_no_client_off_this_machine_reads_them` and `app/test/held.test.ts`
  - the inventory: `held_text::tests::every_sender_is_listed_and_every_exit_asks` fails when an exit loses its call and when a file that can send is not listed.
- **AC-10**: `held_text::tests::a_span_given_held_text_cannot_promote`.
- **AC-27**: `held_text::tests::the_run_bound_and_the_aggregate_cap_decide_what_of_a_held_source_may_leave`, and the ten-quotation cases in the page, seed, bundle, file, document and claims-only tests. Not tested: a summary (slice G) and `delphi handling check` (141); neither exists.
- **AC-31**: `held_text::tests::the_block_index_finds_a_held_source_no_record_names_and_forgets_it_when_its_text_goes`.
- Fitness: `held_text::tests::every_output_that_can_carry_held_text_calls_body_may_leave` (no production file but `held_text.rs` runs `shares_run`; the private-text page check of 119 now goes through `held_text::carries_run`).

## Builder decisions (for the architect)

1. **The tier hides held text from agents' default searches.** §2.5 item 3 says held text is indexed at `external_untrusted` "for the operator and the operator's agents". An agent's `kb_search` and a span's start passages ask for nothing untrusted, so they do not return it. The operator's view, the page gather and any search that asks for untrusted text do. Built as the spec words the tier; confirm, or say agents should see it by default (which would taint every span that does).
2. **`is_held` is deny by default**: the mark, or any public full-text source under a licence that is not open, not the operator's, and not adopted from a bundle. A held source whose text was evicted keeps the mark, so its record is still refused by a bundle copy.
3. **A bundle copy refuses a held source outright**, rather than copying its citation and excerpts. Its citation can still be exported with `sources bib`.
4. **The bibliography (`Channel::Export`) sends a held source's citation** (141 §2.4 item 7), with free-text fields cut. The three lookups and the web search (tags) also pass; they send no body.
5. **One output = everything sent together.** A bundle, a pushed range and an archive are read as one output, so a source cannot leave in pieces spread over files.
6. **The document path checks the tool's arguments, the written bytes and an archive's members.** It refuses (a model made it).
7. **The app.** The daemon's reply carries `may_export`; the window enforces it (export and print hidden, a copy past 300 characters refused, the held text not printed). The Rust `export_page` and `print_page` commands do not re-ask the daemon. Every app read is on a socket of this machine, and `need_reader` refuses relay, narrator and host, so no client off the machine is sent held text.
8. **Outbound, Connector, Mail and Relay are not re-checked against held text here.** They stay held by the private-read mark; a web query is not a sink (ruling 2). Their `Q` column is 141's.
9. **Index reads write nothing.** A missing or stale block file is rebuilt in memory, so the encyclopedia's reads stay read-only.
10. **A topic page's last check** cuts a seed or claims-only page and refuses a written one; a search page that cannot be cut is replaced by a line that says why.

## Existing tests changed on purpose

- The research-host tests of 119's `excerpts` behaviour now pin `unknown_licence = "excerpts"` (`excerpts_step`); one test is renamed `..._under_excerpts_and_nothing_under_pointer`. The weak-source test also checks that an unlisted page is now kept whole and held.
- Three service tests: a page of unknown licence is now `full`; the index now reads two sources (the open one and the held one); and a continued run reads an unchanged held page from the library instead of fetching it (114 §3 item 4).
- `config::tests`: the default and the fallback for a wrong word are `local_text`.
- The page check's private-text run scan (119) now calls `held_text::carries_run`, so `shares_run` has no caller outside its own file.
- Found while testing: a claims-only page's bibliography repeats each claim's opening words, so the claims-only cut counts the laid-out page, not the body alone.

## Gates (2026-11-26)

`forge finish --gates --spec 142 --slice B`: tests passed, clippy passed, lint passed. The app's `npm test` (101 tests) and `tsc` passed.

## Not verified

- No live run: no research run on the operator's install has kept a held page yet.
- The app's copy guard and print hiding are tested by source checks and pure functions, not in a running window.
- The cost of the block index on a full 1 GiB cache (about 16 million hashes, 128 MiB of files) is not measured.
- `delphi handling check` and summaries do not exist yet (141, slice G); AC-27 is not tested there.
