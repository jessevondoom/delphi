---
state: built
date: 2026-11-27
title: "the sender inventory brought up to date; page, document, clipboard, off-machine client, seed-export, remote serve"
eval_waived: "the slice adds label checks at the page reply, the app, the document path, seed-export and remote serve; what is indexed, how it is chunked and how a search ranks are unchanged (a remote kb serve only leaves labelled documents out of what it returns); the diff base counts knowledge-v2's own retrieval changes"
---

Built 2026-11-27 on branch `build/141-c` (from `knowledge-v2`). Not merged.

## What was built

- **The inventory, brought up to date** (§2.4 item 8).
  - `egress::APP_COMMANDS`: every command of the app's `generate_handler!`, each a sink with the call it must make first, or not a sink with why. A test parses the app's command list and fails on a command not listed, or a row whose command is gone.
  - `egress::WINDOW_COPIES`: every file of the app's window that writes to the clipboard. A copy of entry text must ask `copyDecision`. The others copy a command or a path.
  - `held_text::SENDERS`: the app's export, print and copy now ask (`page_gate(`); the document path asks `egress::check_output(`.
  - The daemon's network files (`NETWORK_FILES`) and senders (`SENDERS`) were read again. No new sender was found.
- **The page reply** (`encyclopedia::pages_get`, `pages.get`). It carries:
  - `handling`: the page's label as a sink reads it (`label::at_sink` over the sidecar of the version shown);
  - `export`: the decision at `Sink::PageExport` (`send`, `labelled`, `refuse`), its reason, the quotation flag and the visible line;
  - `copy`: the decision at `Sink::Clipboard`;
  - `may_export`: false when the export is refused. A page that carries more of a held source than may leave is refused whole, as before.
- **The source reply** (`library.source`) carries `copy` and `handling` too. A source whose copy is not a plain send has `may_export: false`. Its own `label` field is kept.
- **`egress::view_decision`**: the decision a window is given for one sink. **`egress::reply_to_client`**: a reply to a client off this machine is refused anything labelled; a reply whose label was not computed is every mark. **`egress::index_label`**: an index document's label recomputed from the records and joined with its row's.
- **The app** (`app/src-tauri/src/lib.rs`, `sink.rs` new).
  - `export_page` and `print_page` now take the page (`slug`, `version`) and `confirmed`. They read the page again from the daemon (`pages.get`, a read the key already holds) and act on its `export` decision. Refused: nothing is written or printed, and the reason is shown. Labelled: only once confirmed, and the exported file begins with the visible line. A reply with no decision (an older daemon) is refused.
  - The exported text must begin with the page the daemon returned.
  - The window (`Encyclopedia.tsx`, `ency.ts`): a labelled page's export, print and copy wait for one confirmation in the window. A page and a source follow the `copy` decision (`copyDecision`): refused, labelled (copied with its visible line after the confirmation), or bounded to 300 characters for held text.
- **A client off this machine** (`service.rs`): `client_on_this_machine` is the one place that says whether the caller is on this machine. Every listener of the daemon is a Unix socket, so today it is always yes. `pages.get` and `library.source` pass it to `reply_to_client`.
- **The document path** (`artifacts.rs`): every artifact tool's arguments, written bytes and archive members are checked with `egress::check_output` at `Sink::Document` (`Confirm::Nobody`). This adds the label to the held-text check of 142.
- **`kb seed-export`** (`kb::seed_export`): no vector of a chunk of any labelled document or held source is written. It says how many chunks it left out.
- **`kb serve --allow-remote`** (`kbserve.rs`): on an address that is not loopback (`Api::for_bind`), no labelled document is found, listed, served, or has its pictures served. On loopback nothing changes.
- **`delphi pages`** printed to a file or a pipe (`pages_cmd.rs`): the page-export row, by what the page carries.
- **The bibliography** (AC-16): at the sink, a held source's own `abstract` and `note` are not sent.

## Acceptance criteria and tests

- **AC-6** (the app's export and print):
  - the daemon's decision, through `pages.get`: `egress::sink_tests::sink_page_export_follows_its_row` (refused with the marks and items, labelled with its line, unlabelled as before, a page quoting a held source refused whole, a page with no label refused);
  - the app's side: `sink::tests::export_refuses_a_private_page`, `sink::tests::a_labelled_page_asks_once_and_its_file_begins_with_the_visible_line`, `sink::tests::an_unlabelled_page_is_exported_as_before_and_an_older_daemon_or_other_text_is_not` (`app/src-tauri`);
  - the window: `app/test/held.test.ts` ("a refused page offers no export or print, a labelled one asks first, an unlabelled one is as before").
- **AC-7**:
  - `egress::sink_tests::sink_seed_export_writes_no_labelled_vector` (no vector of a labelled chunk, the count, a row with no marks is every mark);
  - `egress::sink_tests::sink_remote_serve_returns_no_labelled_document` (no labelled document found, listed or served off loopback; loopback unchanged).
- **AC-15**:
  - the registry: `egress::tests::every_registered_sink_calls_check_label_with_its_own_sink`, `egress::tests::every_command_that_hands_text_out_is_registered`, `held_text::tests::every_sender_is_listed_and_every_exit_asks`;
  - a document or deck: `artifacts::tests::sink_document_follows_its_row`;
  - the clipboard: `egress::sink_tests::sink_clipboard_follows_its_row` and `app/test/held.test.ts` ("the clipboard row");
  - a client off this machine (a fixture sender): `egress::sink_tests::sink_off_machine_client_follows_its_row`, and `egress::sink_tests::no_daemon_listener_is_reachable_from_another_machine`.
- **AC-16**: `egress::sink_tests::no_exported_field_holds_a_run_of_held_text`.
- Also: `pages_cmd::tests::a_page_printed_to_a_file_follows_the_page_export_row`.

## Builder decisions (for the architect)

1. **The app asks the daemon again before it writes or prints.** The window's flags are not trusted: `export_page` and `print_page` read `pages.get` themselves and act on its decision. No new daemon method and no grant change was needed (`pages.get` is in the `read` grant).
2. **A reply with no decision is refused by the app.** An older daemon gives none; reading it as "send" would let a labelled page out. The window still shows the button, and the refusal says to update the daemon.
3. **The exported text must begin with the page the daemon returned** (the visible line aside), so the window cannot export other text under the page's decision.
4. **The confirmation is in the window**, once per export, print or copy (§2.4 item 3). The app's Rust side refuses a labelled page without `confirmed: true`.
5. **The clipboard is the window's.** No command of the app writes to the clipboard; the copy happens in the web view. The daemon gives the decision (`copy`), and the window follows it. A labelled copy is put on the clipboard by the confirmation button, with the visible line first.
6. **A source's copy follows its row too.** A private, personal or confidential source is not copied from the app; an internal one is copied labelled.
7. **A client off this machine does not exist today.** `client_on_this_machine` always says yes, held by a test that no daemon listener is a network one. Only `pages.get` and `library.source` ask `reply_to_client`; the other reads are reachable from this machine only, like them.
8. **The document sink has no person to confirm** (a tool call), so an internal document is refused (the stricter reading of `L`). The label of a document comes from what it carries: a run of more than 300 characters of a labelled page or of a private source (`label::of_files`). Archive members are labelled the same way, not by their path.
9. **Seed-export and remote serve read each document's label at the time of the send** (`index_label`: recomputed from the records, joined with the row's). A row with no `marks` is every mark, so it is left out.
10. **Remote serve also filters the document listing and pictures.** A labelled document's path is its own (§2.1 item 5). The collection and kind counts are numbers only and are not filtered.
11. **`delphi pages` to a file** is labelled by what the page carries, since `pages show` builds a page with no sidecar. Printed to a terminal it is the operator's view and is not checked.
12. **AC-16 and summaries.** A held source's own `abstract` and `note` are not sent. Its checked summary still stands as the abstract (142 §2.6 item 8 and 142 G's test), cut like any other field. Confirm, or say the summary must go too.
13. **The page's label for an earlier version** is read from the sidecar that version was kept with. One that cannot be read is every mark.
14. **The new reply fields are `handling`, `export` and `copy`.** `label` was already taken in the source reply.

## Existing tests changed on purpose

- `held_text::tests::the_apps_views_say_a_held_source_may_not_be_exported_and_no_client_off_this_machine_reads_them`: `library_source` takes the workspace and the client's place; the source check looks for the new decision call.
- `app/test/held.test.ts`: the window's export and print buttons now also ask the decision; the copy asks `copyDecision`.
- Fixtures: `kbserve::Reader` literals gain `remote: false`; `pages_get` calls pass `true`; the sink tests' page fixture writes `slug` and `title`.
- Inventories: `held_text::SENDERS` (the app row now asks; the document row's token), `egress::SINKS` (six rows wired).

## Gates (2026-11-27)

`forge finish --gates --spec 141 --slice C`: tests passed, clippy passed, lint passed. The app's `npm test` (103 tests), `tsc`, and `cargo test` (47 tests) and `cargo clippy` in `app/src-tauri` passed.

## Not built here

- `delphi handling check`, the pre-push hook and the outbound marks line (slice D).
- The other reads of the app (`kb.get`, `topics.get`, `memory.role` and the like) do not ask `reply_to_client`; no client off this machine can reach them.

## Not verified

- No run in a real app window: the confirmation card, the print of a labelled page and the labelled copy are tested by pure functions and source checks only.
- No run on the operator's workspace.
- The cost of `of_files` on each artifact write (it reads every library page and private source) is not measured.
