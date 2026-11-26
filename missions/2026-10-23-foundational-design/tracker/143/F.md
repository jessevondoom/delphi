---
state: not-built
date: 2026-11-26
title: "by hand and by mode: [bridge.jobs], origins, the typed confirmation and the ceiling, commands and app buttons"
---

Slice F of spec 143. **Part 1 built 2026-11-26** on `knowledge-v2`: the machinery, and the pages job end to end. The other jobs' commands and the app's buttons are part 2 (below), so the slice stays `not-built` until they land.

## Part 1 (built)

- **`[bridge.jobs]`** (§2.8 item 4). There are five keys (`pages`, `summaries`, `validation`, `session_summaries`, `consolidation`), each `auto`, `manual` or `off`, and `auto` unless set. They are parsed and checked (a bad value or an unknown key is refused), and documented in the config, the schema, the example and the reference.
  - The mode is read in one place, `job_switched_on` (`summary.rs`), for bridged calls only. `manual` refuses what nobody started; `off` refuses everything. It never turns the job's own "no" into a yes.
  - An idle job in `manual` or `off` returns before admission and raises no notice (§2.8 item 5).
- **The origin `command`.** `daemon_notice::in_command(job, confirmed, f)` is a task-local scope. A bridged call inside it is that job's, with the origin `command`, or `confirmed` when the operator typed the confirmation. So it is held to the job's mode and weekly budget, needs no idle pause and is not held to the idle share. Before, a call the operator started carried no job at all.
- **The preflight.** `bridge.preflight {job}` and `Engine::bridge_preflight` answer what a job would meet now: `not_bridged`, `admit`, `off`, `confirm` (with today's calls and tokens against the daily cap), `ceiling`, `quota` or `no_route`. It reads only.
- **`pages.rebuild`** takes `slug`, `due` or `all`, and `confirmed`, and runs inside the command scope.
  - It is refused when `off`, past a limit unless `confirmed`, and past the ceiling always. Each refusal has fixed words (`service::PAGES_OFF`, `LIMIT`, `CEILING`).
  - `due` and `all` rebuild one topic after another and stop at the first limit.
- **`delphi pages rebuild <slug> | --due | --all`** asks the preflight first.
  - `off` refuses and says why (exit 2); the ceiling refuses (exit 3).
  - Past a limit it prints the day's usage and a word chosen at random, and goes on as `confirmed` only when stdin and stdout are a terminal, neither the seat variables nor the process ancestry (`run_granted::seat_refusal`) show an agent seat, and the word is typed back on `/dev/tty`. No flag stands in for it.
  - After a confirmed run it says how far the cap was passed.
- **`delphi kb health`** shows due pages of a bridged `manual` or `off` job as waiting for the operator or off, with the command or the key. It does this only where every allowed endpoint is a harness, so the mode applies.

Verified (tests):

- `config::bridge_jobs_tests::bridge_jobs_default_to_auto_and_take_only_their_three_modes`.
- `service::tests::a_manual_job_waits_for_the_operator_and_an_off_job_is_refused_with_its_reason` (AC-20): manual does nothing at idle and says nothing; `off` refuses with the key and launches nothing; `pages.rebuild {due}` rebuilds through the bridge, and the ledger records job `pages`, origin `command`.
- `service::tests::past_the_daily_cap_a_command_needs_the_confirmation_and_past_the_ceiling_nothing_runs` (AC-16, AC-19, the daemon's half).
- `bridge_cmd::tests::the_typed_confirmation_passes_only_with_the_word_at_a_terminal_outside_a_seat` (AC-19, the command's half): a wrong word, no answer, no terminal or a seat each refuse, and nothing is asked when it cannot pass.
- `knowledge_health::tests::due_pages_of_a_manual_or_off_job_wait_for_the_operator`.
- AC-20's last clause (`auto_rebuild = false` with `pages = "auto"`: nothing at idle) is `page_run::tests::the_idle_rebuild_builds_a_due_topic_only_when_it_is_on`.

Limits, stated:

- **The confirmation is the command's, not the daemon's.** The daemon accepts `confirmed: true` from any console-scope client. A program that can reach the socket could send it without a terminal. This is the weakness of §2.8 item 3's "a command is not proof of the operator", one layer down; the follow-up spec named in the handoff (the agent-seat check) covers both.
- The typed confirmation has not been run at a real terminal; its pure half is tested.

## Part 2 (not built)

- The commands of §2.8 item 1 for the other jobs, each as a daemon method running that job's pass inside `in_command`: `delphi memory consolidate` (`consolidate_pass`), `delphi sessions summarize <session> | --pending`, `delphi kb validate --pending` (`background_validate_tick`). `delphi kb summarize` waits for 142 G and `delphi kb eval run` calls no model.
- The app's buttons for those jobs, and their daemon methods in `crates/delphi/tests/app_grants.toml`. That is a gate-changing path, so it goes in its own pull request, which the operator merges at a terminal.
