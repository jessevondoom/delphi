---
state: built
date: 2026-11-26
title: "capture_source: body forms, batches, metadata, validation, dry run, limits"
---

Built 2026-11-26 on branch `build/142-c` (from `knowledge-v2`). Not merged.

## What was built

- **`capture_source`**, a new MCP tool of the host server (`host_capture.rs`, new; daemon method `host.capture_source`).
  - One to 20 sources a call; one may be passed bare. Each item is answered on its own.
  - Three body forms: `text` (at most 256 KiB), `file` (at most 8 MiB), or the locator alone.
  - `file` is read beneath the session's project folder or its handover folder (`<home>/handover/<session>/`). The folder is opened, each path component is opened beneath it with `O_NOFOLLOW`, and the file is checked with `fstat` on its descriptor and read from it only (`read_beneath`).
  - The locator alone asks the daemon to read the address under its runner's rules (`LiveWeb`). An address it may not read is answered `not fetched: hand over the text`, and nothing is stored.
  - Optional fields: `title`, `retrieved`, `kind`, `media_type`, `csl`, `licence`, `labels`, `topics`, `topic_id` or `research_id`, `note`. An unknown field is refused by name.
  - `dry_run: true` runs the same path with a library that writes nothing (`Library::previewing`). It returns the `meta.json` and `capture.json` that the call would write, the level, `rights`, the label, the findings, and whether each waiting quote would anchor.
- **Validation and normalisation** (`source_normalise.rs`, new). One function per field. Each finding is `{field, problem, fix, kind}`, where kind is `error` or `normalised`.
  - The locator is made canonical. An address with credentials is refused.
  - Dates become RFC 3339.
  - `csl`: the type comes from the closed list, a name given as one string becomes family and given, a DOI is checked and lower-cased, an ISBN's check digit is verified, and `issued` becomes date parts.
  - Text: UTF-8, NFC, `\n`, no byte-order mark. The media type is compared with the bytes.
- **The library side** (`library_capture.rs`: `Handed`, `Library::add_handed`, `confirm_host_capture`, `host_word`).
  - A host capture says `captured_by: host:<tool>/<model>`. Its extractor is `host` for text, or `fact-fetch-text` 1 for a file.
  - Its record carries `host_capture: {by, confirmable}`, `licence_claimed`, the citation fields and `topics`.
  - `rights` is the daemon's: open only when the daemon finds a licence in an HTML file; otherwise held (`text_held`), or library-private.
- **Confirmation** (`Library::store`). The daemon reading the address and getting the same `text_sha256` adds `confirmed_by_daemon` to the capture and the record, copies the fetch's origin and class, and recomputes the support of the claims that cite it. Other text makes the daemon's own new version, even when the daemon keeps it at a lower level.
- **Anchors** (`library_claims.rs`).
  - A quote found in a host's text is `in: host_capture`.
  - `cites_of` skips such an anchor until the source is confirmed.
  - `add_host_claim` may anchor into a host capture.
  - `add_waiting`, `waiting_on` and `attach_waiting` keep a quote whose address was not read on its claim, and anchor it when the text arrives.
- **`record_research`** (`host_research.rs`) finds captures by locator. For a source it did not read, it anchors into the host's capture (`host-reported`), or leaves the quote waiting. A private source's capture is kept under `private_pointer_id`, the id `record_research` already gives that locator, so either call may come first.
- **The page cap** (`library_page_validate.rs`). A statement whose only support is an unconfirmed host capture is `host-reported`. The page ends `pass_contested` at most. One line goes under the banner: `rests in part on text a host handed over, which Delphi has not confirmed`, or, for a capture read privately, `... which Delphi cannot confirm (read privately)`.
- **Shown.** `delphi sources show` says `not confirmed by the daemon`, `cannot be confirmed`, or `confirmed by the daemon on <date>`, and shows `licence_claimed`. `delphi kb health` counts pages held at `pass_contested` because they rest on text read privately (`pages_host_private`).
- **Limits.**
  - 20 sources a call.
  - One session: 120 sources and `[library] handover_session_mib` (16) an hour.
  - All sessions together: 300 sources and `[library] handover_hour_mib` (64) an hour. Kept in `library/handover.json`.
  - The daemon holds the host gate across the check and the write.
  - The cache rule of D1 (`cache_admits_host`) refuses a host capture with `cache full` rather than evict text used in the last seven days.
  - Past any limit the call is refused with the limit named, and nothing is written.
- **The mark.** Under the private-read mark no transport is built, and nothing is fetched. Every source is library-private and `cannot be confirmed`, as are the kinds `internal`, `file` and `person` in any session.
- **Commands.** `delphi sources capture <file.json> [--dry-run] [--json]` (the operator's own call, `captured_by: operator`, refused inside an agent seat) and `delphi sources lint [<id>] [--json]`.
- **Worker files.** `capture_source` was added to the scout's and the investigator's tools (`WORKER_CAPTURE_TOOLS`). The worker sentence and the scout's charter say when to use it and name the handover folder. Every other denylist worker denies it.
- **Config.** `[library] handover_session_mib` and `handover_hour_mib`, in `config.rs`, `schemas/config.schema.json`, `docs/reference/config.md` and `config.example.toml`.

## Acceptance criteria and tests

- **AC-4**:
  - `host_capture::tests::a_handed_text_is_kept_as_the_hosts_capture_and_found_by_a_search_labelled_not_validated`
  - `host_capture::tests::a_call_with_both_bodies_a_file_outside_the_folders_a_link_or_over_a_limit_writes_nothing`
- **AC-5**:
  - `host_capture::tests::a_quote_in_a_host_capture_anchors_there_stays_host_reported_and_counts_for_nothing_in_either_order`
  - the fitness function `library::claims::tests::a_host_capture_is_no_origin_until_the_daemon_confirms_it`
- **AC-6**: `host_capture::tests::the_daemons_same_text_confirms_a_host_capture_and_other_text_is_its_own_new_version`, and the fitness function above.
- **AC-7**: `host_capture::tests::under_the_mark_capture_source_makes_no_request_keeps_the_text_and_the_source_is_library_private`. The scripted fetcher records no request, the source is private, and `may_leave` refuses it on every channel.
- **AC-16**: `host_capture::tests::five_items_are_answered_each_on_its_own_and_metadata_adds_without_removing`
- **AC-17**: `host_capture::tests::findings_are_listed_unknown_fields_refused_and_a_dry_run_is_the_record_the_call_writes`. The dry run's `meta.json` and `capture.json` equal the written files byte for byte. Unit cases are in `source_normalise::tests`.
- **AC-18**: `host_capture::tests::a_claimed_licence_is_recorded_and_never_raises_rights`
- **AC-23**:
  - `host_capture::tests::the_file_is_opened_then_checked_on_its_descriptor` (a file swapped for a link between the open and the read is read as the file first opened)
  - `host_capture::tests::a_path_outside_the_project_and_handover_folders_is_refused`
  - `host_capture::tests::the_seventeenth_megabyte_a_session_hands_over_in_an_hour_is_refused`
- **AC-26**: `library_tests::page_validation::an_unconfirmed_host_capture_caps_the_outcome` (a scripted validator that passes everything; after confirmation the rebuilt page ends `pass`).
- **AC-29**, the clause D1 left: `host_capture::tests::three_sessions_together_cannot_pass_the_hourly_limit_for_all_sessions` (also `cache full` through the tool, with nothing evicted).
- **AC-32**, partly: the line `cannot confirm (read privately)` and the `kb health` count, in the AC-26 test. Its three-rebuild form is not tested.
- `sources lint`: `host_capture::tests::lint_lists_what_normalising_a_stored_record_again_would_change`.

## Builder decisions (for the architect)

1. **A host capture never replaces the daemon's own whole text.** When the library holds the daemon's full copy of an address and the host's text differs, the item is refused, and nothing changes. The same text is a revisit. A host capture over a daemon pointer is kept as a new version and carries the daemon's origin and class, so the daemon's earlier anchors still count.
2. **What counts is the anchor, not the source.** An anchor `in: host_capture` counts only once the source carries `confirmed_by_daemon`. An anchor the daemon made before a host's version arrived keeps counting.
3. **`[library] unknown_licence` applies to host text.** Under `pointer` or `excerpts` an unlicensed host capture keeps no text, only its digests. Under `private_text` it is private. The default `local_text` keeps it held. A page showing a rights notice keeps no text (§2.5 item 6).
4. **The project folder** is the registered project the session's folder resolves to. With none, only the handover folder is allowed. A relative path is tried against the handover folder, then the project folder.
5. **A web source needs an address.** `kind: web` with a description is refused; a source with no address is `internal`, `file` or `person`.
6. **HTML is not taken as `text`.** It is refused with "save it to a file". A file is extracted by the library's extractor.
7. **The text and confirmation.** A host's text is stored as the daemon stores a page: its title as a heading, then the text. So confirmation needs the same title and words.
8. **The locator alone stores nothing** when it cannot be read. AC-16 says only the first four of five items are stored, so no pointer is written for the fifth.
9. **`topics`** are recorded on the source's record (`topics`) after a check that each topic exists. Gathering a page is still by search, so this does not yet put the source in a topic's material.
10. **Labels** are written by the host as handling traits (`By::Host`), with the traits the kind adds. Nothing removes one.
11. **Secrets and identifiers** are taken out of a handed file before it is kept, so `raw` is what the text was made from, not the bytes on the host's disk.
12. **An inside agent's search** finds host text only when it asks for untrusted text, as slice B decided for held text. AC-4 is tested that way.
13. **Limits count what is kept.** A call's items are counted as sources only when they are not refused. A dry run is not counted. The operator's command has no hourly limit.

## Existing tests changed on purpose

- `mcp_host`: the tools golden, the write list, the `openWorldHint` list and the socket waits now include `capture_source`.
- `mcp_hosts`: the scout's and the investigator's tools gain `capture_source`. The strategist test lists it as a tool the strategist must not hold.
- `library_page_validate`: `Material` gains `host_word`.

## Not verified

- No live run: no host has called `capture_source` on the operator's install, and whether a host's fetch tool can save a page to a file is not known (§7).
- The descriptor-based read is tested on macOS only. Other platforms refuse `file`.
- AC-32's three rebuilds and a second source are not tested; the line and the count are.
- The cost of `waiting_on` and `attach_waiting`, which scan every claim on each capture, is not measured on a large library.
