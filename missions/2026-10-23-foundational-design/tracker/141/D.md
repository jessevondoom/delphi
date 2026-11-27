---
state: built
date: 2026-11-27
title: "delphi handling check, the daemon's push, the hook, the marks line on outbound cards"
eval_waived: "the slice adds label checks at the repository and outbound sinks and a config key for internal remotes; what is indexed, how it is chunked and how a search ranks are unchanged; the diff base counts knowledge-v2's own retrieval changes"
---

Built 2026-11-27 on branch `build/141-d` (from `knowledge-v2`). Not merged.

## What was built

- **`[handling] internal_remotes`** (`delphi-adapter/src/config.rs`): the git remotes an `internal` file may be pushed to. Compared whole, as `git remote get-url` prints it, after a trailing `/` and `.git` are dropped. At most 50, no spaces. Empty by default: `internal` is refused at every remote. Schema, `docs/reference/config.md` and `config.example.toml` updated.
- **The repository sink** (`worktree.rs`), one check for both senders:
  - `repo_files(dir, Pick)`: the staged files, the files of a range `<a>..<b>` (as at `<b>`), or named paths. Git runs with the daemon's hardening; a range that starts with `-` is refused.
  - `repository_check(home, ws, root, files, remote, confirm)`: `egress::check_output` at `Sink::Repository { internal_remote }`, where `internal_remote` is "the remote is on the workspace's `internal_remotes`". Each file's label comes from the daemon's records (slice B's `label::of_files`); held text is bounded by `held_text::check_files`. A refusal names the files and the marks, no text.
  - `pre_push_hook_path(dir)`: where git runs the repository's pre-push hook (`core.hooksPath` from the repository's own config, else the common git directory's `hooks/`).
- **The daemon's push** (`worktree.rs`, `finalize.rs`):
  - `Worktree::push_check` runs `repository_check` over the branch's range at `origin`'s URL, confirmed by the card (`Confirm::Card`).
  - `finalize.rs` runs it **before** the pull-request card. A refusal raises no card and ends the build `MergeRefused`, naming the files and the marks. A labelled push puts the label on the card (`payload.handling`: the line, the marks, the decision; the summary ends with the line).
  - `open_pr` takes a `PushGuard` (home, workspace, the line the card showed) instead of a home, and runs the check again before the push. A label that is sent labelled is pushed only when the card showed exactly its line, so a label that changed while the card waited stops the push.
- **`delphi handling check <path>... | --staged | --range <a>..<b> [--remote <url>]`** (`handling_cmd.rs`). Exit 0 may be sent, 1 refused, 2 usage. A labelled send asks at `/dev/tty` (a hook's stdin is git's ref list) and needs the operator to type `send`; inside an agent seat or with no terminal the answer is no.
- **`delphi handling hook [--install]`**: says whether the pre-push hook is installed in the repository here; `--install` writes it, executable. It never replaces a hook it did not write; it refreshes its own. Refused inside an agent seat. The hook, for each pushed ref, checks the commits the remote does not hold yet (from the remote's sha, or for a new branch from the first commit no ref of that remote holds), at the remote's URL, and exits 1 on a refusal. A deleted ref is skipped. `delphi` not on PATH refuses the push.
- **`delphi connect`** prints the hook line every time: in `--status`, in each `--dry-run` and after a successful run (`connect_cmd.rs`).
- **The outbound card** (`label.rs`, `harness_run.rs`, `connector_run.rs`):
  - `label::given(ws, home, mission, role)`: the label of the items the span was given (a library source or claim with its library marks, any other item its traits). `label::card_line`: `Handling: this task was given private (2), internal (1).`
  - A harness's `WebFetch` and `WebSearch` card and every connector card carry the line, and the payload carries `handling` (the marks). Both ask `egress::check_label(&given, Sink::Outbound)`.
  - A given `library_private` item (the row's refusal) is held exactly as the private-read mark holds a send: the Red, console-only held card, which a relay reply cannot approve. The mark itself and its counter are not touched.
  - A connector write or draft whose arguments quote more of a held source than `body_may_leave` allows is refused before any card (`held_text::check_files` over the arguments as one output). A connector read's query is a search and is not checked (ruling 2).
- **Registry** (`egress.rs`): `SINKS` rows for both repository sinks and for outbound are now wired, with their senders and proof tests. `held_text::SENDERS`: the push now asks through `egress::check_output(`; connector writes and drafts are an exit that asks `held_text::check_files(`.

## Acceptance criteria and tests

- **AC-4** (the rows of slice D's sinks, through their real entry points):
  - repository, both columns: `egress::sink_d_tests::sink_repository_check_follows_its_row` (a staged copy of a page with each one mark at a listed and an unlisted remote; held text through `--range` and paths);
  - outbound: `harness_run::tests::sink_outbound_card_names_the_marks` (a real harness `WebSearch`: personal, library-private and nothing given), `egress::sink_d_tests::the_outbound_row_and_its_card_line`, and `connector_run::tests::a_connector_write_names_the_given_marks_and_one_that_quotes_a_held_source_is_not_sent`;
  - the registry: `egress::tests::every_registered_sink_calls_check_label_with_its_own_sink` (unchanged; it now checks D's rows).
- **AC-8**:
  - the command: `handling_cmd::tests::check_staged_refuses_a_labelled_page_and_held_text_and_passes_internal_only_to_a_listed_remote`;
  - the daemon's push: `worktree::tests::a_push_whose_range_holds_a_labelled_page_is_refused_before_the_card_and_after_it`;
  - the line and the yes: `egress::sink_d_tests::an_internal_file_goes_to_a_listed_remote_only_with_its_line_and_a_yes`;
  - the config key: `config::tests::internal_remotes_match_a_whole_remote_and_nothing_else`.
- **AC-9**: `harness_run::tests::sink_outbound_card_names_the_marks` (the card shows `private (2)`; the task's private-read mark is `Unmarked` and its counter unchanged). The separation test of 121 (`handling_tests::no_trust_decision_reads_handling`) is unchanged and passes: no file that decides trust gained a handling read.
- **AC-17, second half**: `handling_cmd::tests::the_hook_line_says_whether_the_pre_push_check_is_installed_and_install_writes_only_its_own_hook`.
- Also: `worktree::tests::a_push_whose_range_quotes_a_held_source_is_refused_and_nothing_is_pushed` (142 AC-9) still passes through the new check.

## Builder decisions (for the architect)

1. **Outbound, `library_private` (the row's `R`) is the held card, not a refusal.** §2.4 says "held by the mark, as today" and item 5 says the line "removes nothing"; ruling 2 says a web query is never refused. So a task given a library-private item is put to the operator on the Red, console-only held card, as a marked task is, even when it is not marked. Stricter than before, refusing nothing. The mark and its counter are left alone (AC-9).
2. **Outbound `Q` is enforced at connector writes and drafts only.** A harness's `WebFetch`/`WebSearch` and a connector read's query are web or search requests (ruling 2) and are not checked against held text. A connector write or draft is refused outright, before a card, when its arguments fail `body_may_leave` (a model wrote them, so nothing is cut).
3. **"Given" is per mission and role** (`handling::given`, as §7 says): the card may name marks the sending span never saw.
4. **`internal_remotes` matches whole URLs** (trailing `/` and `.git` dropped), no prefixes or patterns. The same repository over SSH and HTTPS must be listed twice.
5. **No `--remote`: the unlisted column.** `delphi handling check --staged` without `--remote` refuses an `internal` file.
6. **The labelled push needs the card to have shown the label.** The check runs before the card (a refusal asks nothing) and again before the push; if the line differs, nothing is pushed.
7. **The terminal yes for `check` is read from `/dev/tty`**, because a pre-push hook's stdin is git's ref list. With no controlling terminal (a host's own tool call, most likely) a labelled push is refused; an unlabelled one is not affected.
8. **A labelled Markdown file must carry its visible line** at the repository sink, as at slice B's sinks; a refusal names the line to add.
9. **The hook refuses when it cannot check**: `delphi` not on PATH, no workspace resolved for the project, a remote sha not present locally (fetch first), all exit non-zero.
10. **`hook --install` writes where the repository's own config says** (`core.hooksPath`) or `.git/hooks/`. A `core.hooksPath` in the operator's global git config is not read (Delphi reads no user git config), so a repository relying on one gets a hook git does not run; the command prints where it wrote.
11. **The hook line in `delphi connect`** is printed in `--status`, each `--dry-run` and after a successful run, not on usage errors or refusals.
12. **`open_pr`'s last argument changed** from the home to a `PushGuard`; its one production caller (`finalize.rs`) and its tests were updated.

## Existing tests changed on purpose

- `worktree::tests::a_push_whose_range_quotes_a_held_source_is_refused_and_nothing_is_pushed`: passes a `PushGuard` instead of a home (the signature); the assertions are unchanged.
- `held_text::SENDERS`: the push's call token is now `egress::check_output(`; connector writes moved from "not a sink" to an exit that asks, with its proof test.

## Gate-changing paths

None needed: no change to `crates/delphi/tests/app_grants.toml`, `spawn_sites.toml` or `.github/`. Every new git call goes through `worktree.rs`'s existing `git_command`, so the spawn-site count is unchanged.

## Not verified

- No run on the operator's workspace; no real push through the hook. The hook script is checked by its text, not run by git in a test (it needs `delphi` on PATH).
- The `/dev/tty` prompt was not answered by a person; tests pass the answer in.
- That a host's Bash tool has no controlling terminal (so a labelled host push is refused, not asked) was not checked.
- The daemon's pull-request card with the label is tested through `push_check` and `open_pr`, not through a full `finalize` run with a card.
