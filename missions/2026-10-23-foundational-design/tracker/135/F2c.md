---
state: built
date: 2026-11-21
pr: 849
title: "`tests_run` only when a command ran, `started: false` and the fix; `[forge]` read at start (doc); model and tokens in the result; `daemon stop` with a scratch h..."
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 1339, under `The forge, first cut (spec 135, approved in advance 2026-11-20)`):

| Spec | Slice | Content | State |
| :-- | :-- | :-- | :-- |
| 135 | F2c | `tests_run` only when a command ran, `started: false` and the fix; `[forge]` read at start (doc); model and tokens in the result; `daemon stop` with a scratch home | built 2026-11-21 (draft pull request, not merged): `review_job.rs` `could_not_start` (runner exit 126/127, `sandbox-exec` 71 with `execvp`, bubblewrap 1 with `bwrap: execvp`), `CommandResult.started`, `tests_run` only for a command that ran to an exit status, `tests_note` (also appended to the result's `note`) naming `[exec] toolchain` and `[exec] env.PATH`; top-level `model`, `tokens_in`, `tokens_out` from the seat receipts; the config reference says `[forge]` is read at daemon start. §14.2: commented `[exec] toolchain`/`env` lines in `config.example.toml`, schema and reference rows, `delphi doctor` check `9c review-commands` (`review_job::program_problem`). Item 4: cause found (`daemon stop` needs a console token, which the daemon gives only to a client with a controlling terminal, spec 005 §2.1; without one it printed `not running` and exited 0); fixed under spec 005 in its own pull request (#849, not merged): it now says a daemon is running and a terminal is needed, exit 1, nothing changed (the TTY rule is kept). Verified: AC-12 (scripted runner), AC-13 (daemon test, scripted model, real sandbox), AC-16 (doctor unit test). Not verified: a live foreign seat; a real `cargo test` in the sandbox (F4b) |
