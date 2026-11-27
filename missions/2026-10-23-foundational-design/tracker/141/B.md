---
state: built
date: 2026-11-26
title: "check_label and the sink table; bundle, bibliography and Xenia wired"
eval_waived: "the slice adds label checks at the bundle, bibliography and Xenia sinks; what is indexed, how it is chunked and how a search ranks are unchanged; the diff base counts knowledge-v2's own retrieval changes"
---

Built 2026-11-26 on branch `build/141-b` (from `knowledge-v2`). Not merged.

## What was built

- **`egress::check_label(label, sink) -> Decision`** (`egress.rs`). It is `label::decide`, the one decision path of slice A. `Sink` and `Decision` are re-exported from `label.rs`.
- **`egress::check` is implemented by `check_label`.** The record's own label (`label::of_meta`: `library_private`, `text_held`, or every mark when unreadable) is decided at the row of its channel. Any decision but a plain send refuses a whole body. A test shows it agrees with the library's gate on every channel.
- **`egress::check_output` and `egress::check_path`**: the check a sink runs over the files it sends together.
  - Each file's label comes from the daemon's records (`label::of_files`), never from the file's frontmatter or line.
  - Their join is decided once at the sink. A refusal names the marks, the items and the files, and no text.
  - `send labelled`: each labelled Markdown file must carry its visible line, and the operator confirms (`Confirm::Terminal`, `Confirm::Card`, `Confirm::Nobody`). With nobody to ask, it is refused.
  - The quotation bound runs over all files as one output (`held_text::check_files`), whatever the label says.
  - `egress::terminal_yes`: the question goes to stderr; with no terminal the answer is no.
- **`label::of_files`** (`label.rs`). A file takes the label of:
  - where it is: a library source, claim or page (the page as a sink reads it, `at_sink`); any other library file, every mark; a workspace file, its handling record; the host-noted store, every mark; a bundle's `sources/<id>/`, that library source as it is now;
  - what it carries: a run of more than 300 characters of a labelled library page (a copy, renamed or cut), or of a private source's kept text.
- **The sink registry `SINKS`** (`egress.rs`): every sink of §2.4, its senders and its proof test. Bundle, bibliography and Xenia are wired. The others name the slice that wires them (C, D).
- **`BUNDLE_WRITERS`** (`egress.rs`): every call into `delphi-bundle` that writes what leaves (the archive, the plugin export, the AGENTS.md export, a bundle publish), with its guard.
- **Wired:**
  - `delphi bundle sources add` (`Library::export_to_bundle_with`, `library_export.rs`): each source decided at `Sink::Bundle` from the records. A confirmed internal copy writes the visible line at the top of `source.md`.
  - `delphi bundle package` (`bundle_cmd.rs`), `delphi bundle export --claude-plugin | --agents-md` (`bundle_compat.rs`), a bundle published through Xenia (`bundle_xenia.rs`): `check_path` at `Sink::Bundle` before the bundle crate runs.
  - `delphi sources bib` (`bib::export_with`, `library_bib.rs`): each source decided at `Sink::Bibliography`.
  - `xenia publish` of a file, the daemon (`service_xenia.rs`) and the command (`xenia_cmd.rs`): `check_path` at `Sink::Xenia`. The marks lead the card.
- **Help text** of `delphi bundle` and `delphi sources bib` says what the labels refuse and what they ask.

## Acceptance criteria and tests

- **AC-4** (the rows of slice B's sinks, one mark at a time, through the real entry points):
  - bundle: `egress::sink_tests::sink_bundle_follows_its_row` (the `sources add` copy and the bundle's files);
  - bibliography: `egress::sink_tests::sink_bibliography_follows_its_row`;
  - Xenia: `egress::sink_tests::sink_xenia_follows_its_row`;
  - the registry: `egress::tests::every_registered_sink_calls_check_label_with_its_own_sink`.
- **AC-5**: `egress::sink_tests::a_bundle_with_a_page_of_held_material_carries_no_run_of_its_text`, and the command: `bundle_cmd::tests::a_bundle_with_a_labelled_page_is_refused_before_the_bundle_crate_runs`.
- Also:
  - `egress::sink_tests::every_sink_recomputes_the_label_from_the_records` (AC-19 at these sinks);
  - `egress::sink_tests::q_is_body_may_leave` (AC-20 at these sinks);
  - `egress::sink_tests::check_is_check_label_over_the_records_own_label`;
  - `egress::tests::no_engine_file_calls_a_bundle_writer_before_check_label` (the list §7 says slice B makes; the fitness function of AC-17).

## Builder decisions (for the architect)

1. **`Decision` is slice A's struct** (`kind`, `quotations`, `reason`), not an enum with `Refuse(reason)`. One decision path, as slice A's decision 1 asks.
2. **Channels that are not sinks of §2.4 take a near row in `egress::check`.** The lookups, the web search and the bibliography take the bibliography's (citation only). The debug bundle, the log, telemetry and history take the bundle's. A bare record has only `library_private` and `text_held`, so the outcome is what the library's gate gave before; a test holds that.
3. **The visible line is required in Markdown files only.** A record (`meta.json`) or an asset cannot carry a line of text. A labelled Markdown file without its line is not sent, and the refusal names the line to add. The daemon writes the line itself only where it writes the file (a confirmed `sources add` copy).
4. **Who confirms.** A command asks at the terminal, on stderr (a bibliography redirected to a file still asks). A Xenia publish (a file or a bundle) is already behind a card the operator answers: the marks lead the card, and the card's answer is the confirmation. `sources add` takes no card: with no terminal it refuses.
5. **A bibliography withholds per citation**, as it withheld private sources before, and counts them. An internal citation goes out only when confirmed, once per export, under `> Handling: …` (`% Handling: …` in BibTeX). CSL-JSON has no place for the line, so a labelled citation is withheld there.
6. **The page bibliography inside a page check is not a sink.** It now calls `bib::for_page` (the old rendering), so an internal source still shows in a local page's own bibliography.
7. **A file's label comes from the records, and from content that matches a labelled page or a private source.** It never comes from a file's frontmatter or line (slice A's AC-2 test forbids reading views). A copy of a page shorter than 301 characters is not found by content; topic pages are longer.
8. **Without a workspace** (none named, none resolved from the directory, no default), a page built from workspace records reads as every mark and is refused. This keeps the text on the machine.
9. **`delphi-bundle` is unchanged** (§2.4 item 9: the bundle crate is not asked to know labels). The check runs in Delphi before each call that packages, publishes or exports.
10. **Not writers, on purpose:** `write_manifest` and `sign` (they write into the author's own directory), `inspect` and `validate` where they only read, `import` and `add` (inbound), the marketplace export (names, a repository, a commit), and `distill` (drafts into a mission's staging directory).
11. **`held_text::SENDERS`** now names `egress::check_path(` for the Xenia senders, `bundle package` and the Xenia bundle publish, which call the held-text check through it.

12. **`label.rs` is a row of `private_read::READS`** (`NoReturn`): `of_files` reads private sources' kept text to compare it, and returns only a label.

## Existing tests changed on purpose

- None changed. `bib::export` keeps its signature and now withholds an internal citation when no person confirms; the 142 held-text tests still pass unchanged.
- Two inventories gained a row: `private_read::READS` (`label.rs`) and the call tokens of `held_text::SENDERS` (now `egress::check_path(`).

## Not built here

- The other sinks (page export and print, document, clipboard, a client off this machine, seed-export, remote serve: slice C; a repository and outbound: slice D).
- `delphi connect` printing the pre-push hook (AC-17's second half, slice D).

## Not verified

- No run on the operator's workspace.
- The terminal prompt was not answered by a person; tests pass the answer in.
- The daemon's Xenia card text with the marks line is tested by the shared check only, not by an RPC test of `xenia.attest.prepare`.
- The cost of `of_files` (it reads every library page and every private source's text once per send) on a large library is not measured.
