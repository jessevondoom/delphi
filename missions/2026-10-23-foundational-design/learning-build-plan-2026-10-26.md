---
id: learning-build-plan-2026-10-26
title: "Learning build plan: what is built, what is next, and how to pick it up"
author: strategist
timestamp: 2026-10-26T04:00:00Z
source_type: internal_generated
confidence: low
status: draft
source_mission: 2026-10-23-foundational-design
tags: [plan, learning-loop, training-mode, handoff]
---

# Learning build plan (draft, 2026-10-26)

The design is [`specs/031`](../../specs/031-layered-learning-and-training-mode.md). Its slices are in [`spec-tracker.md`](spec-tracker.md). This file is the working plan for the next slices: what the code looks like today, the order, file-level steps, tests, and the traps. Where a step below differs from the spec, this file says why; fold the change into the spec when the slice lands.

## 1. State

| Slice | State | Where |
| :-- | :-- | :-- |
| Spec 031 | merged (#180); renumbered from 030 in #184 | `specs/031-…` |
| L0 evidence | **built** (#184) | `snapshot.loaded` in `engine.rs` (span open); `workspace.rs` `Snapshot.loaded`, `rule_ids`; `learn.rs` `task_records` |
| L6a train validate/plan | **built** (#186) | `crates/delphi/src/train.rs`; `dev/train/`; CI step "Training cases are well formed" |
| L1 staging tier | next | §3 |
| L6b run cases | next, parallel with L1 | §4 |
| L2, L3, L4, L5, L7–L11 | not started | §5 |

Working answers to the spec's open decisions (the operator said "go" without answering one by one): OD-1 yes (capture before the 10-task gate), OD-2 exposure randomization opt-in per rule, OD-3 `delphi train` is a subcommand for source checkouts, OD-4 no `scripts/` in skill candidates, OD-5 probe generator on another family, OD-6 no nudge pass yet. Ask the operator before relying on any of them for something hard to undo.

## 2. Facts to build on (checked in the code on 2026-10-26)

- **The mission log is the truth.** `MissionState::append_batch` (`delphi-engine/src/mission.rs`) writes events, applies them to derived state, and publishes them. Derived stores (`state.db`, the usage ledger) are rebuilt from the log. New learning state should follow the same rule.
- **`state.db`** is `local/state.db`, owned by `kb.rs` (`kb_documents`, `kb_chunks`, `kb_chunk_vec`, FTS tables) and the ledger (`delphi-core/src/ledger.rs`). Both say "delete it and rebuild".
- **Zones.** The knowledge base indexes by zone (`bundles` exists, `kb.rs`). A `staged` zone follows the same pattern, as `bundles` did in B2.
- **Tool registry** is `delphi-engine/src/tools.rs` (`Registry::builtin`, `tool_defs`, `classify`). `name_thread` is the template for a small tool that writes a daemon-side record. Tool lists are asserted by exact-list tests at `tools.rs` ~703 and ~1110; adding a tool changes them.
- **Denied cards** are `approval.resolved` with `resolution: "denied"` and a `reason` (`approval.rs` `Resolution::event_payload`; `engine.rs` ~1513). Escalation is `task.completed` with `status: "escalated"` and a `reason`. Retries are `build.attempt` (`build.rs` ~167). Provisional rejections are `decision.reverted`.
- **Headless runs.** `delphi run "<task>" --json --home H --workspace W --budget N --timeout T --on-card deny|policy:F` (`run.rs`). Exit codes 0 ok, 1 gates failed, 2 card denied, 3 budget, 4 infrastructure.
- **The bench runner** (`dev/bench/bench.py`) already clones a task repo, copies a pre-ratified template home (`prepare_delphi_home`), runs an arm, grades with hidden tests, and writes JSONL rows. L6b should reuse its home-copy idea and row shape rather than invent a second runner.
- **Ratification pins files by hash** (`local/ratified.lock`, `ratify.rs`). An overlay that edits `base/` or `core/` files in a temp home must also update the lock, or the daemon refuses to load them. This is the main unknown in L6b (§4.3).
- **Lint traps** (`delphi lint --repo .`, run by a unit test and a CI step): the god names from the Greek theme pack fail outside `themes/` (the benchmark project is written `hermes-agent`, lowercase and hyphenated, which passes); absolute home paths and broken relative links fail. A new file under `dev/` is linted too.
- **Spec numbers collide.** Two sessions took 030 within minutes. Run `git ls-tree origin/main specs/` before choosing one.

## 3. L1: the staging tier

**Goal.** Capture candidate lessons cheaply and safely: no model call, nothing always-loaded, taint kept.

**Design choice that differs from the spec.** The spec says a `staged_notes` table in `state.db`. Prefer **events**: write a `learn.staged` event into the mission log, and derive the table and the `staged` kb zone from the log. Reasons: the log is already the single truth, a table that can diverge from it is the failure `state.db` was designed to avoid, and replay gives free rebuild and testability. Record this in the spec when the slice lands.

**Steps (each its own commit, in this order).**
1. `learn.rs`: a pure `triggers(event, ctx) -> Option<StagedNote>`. `StagedNote { trigger, task_id, role, event_seqs, summary (≤ 280 chars), taint }`. Triggers 1–5 of spec §3.2: a denied card with a reason; an operator correction (start with explicit cues only, see spec §11 Q1); recovery on attempt 2 or 3; a task `Escalated`; a rejected provisional decision. No I/O.
2. Taint: a note's tier is the lowest tier among the events it cites (`provenance.rs` has the lattice and `Tier`). A note built from tool output stays below `internal_empirical`. Unit-test with a planted canary string in a tool result.
3. The hook: after `append_batch` stores events, run `triggers` on them and append `learn.staged` events (guard against recursion: `learn.*` events never trigger). Cap per mission and per workspace; evict oldest. Keep the hook cheap: it runs on every append.
4. Index: `learn.staged` events feed the kb zone `staged` (lexical first; vectors can follow). Retrieval labels each hit with trigger, age and taint, and the IPR snapshot builder must never include the zone (assert in a test: staged text absent from `load_snapshot`, handoffs and Council framing).
5. CLI: `delphi learn list|show <id>|drop <id>` reading the log/derived index. `drop` appends `learn.dropped`; nothing is deleted from the log.
6. `[learning]` config (`config.toml`): triggers on/off and the cap, with floors and ceilings; capture can be turned off, loading unratified content cannot be turned on.

**Tests (spec AC-2, AC-3, and the isolation test).** Replay recorded event streams per trigger and assert exactly one note and no model client call; the canary test; the isolation test; a rebuild test (delete the derived index, replay, same notes).

**Traps.** Do not add a model call in the hook. Do not write into `lessons.md` or any ratified path (the path guard denies it and the lock would break). `agent.*` events have required fields (`event.rs` `validate`); `learn.*` do not, but keep `task_id` set so `task_records` can join them.

**Done when:** the triggers above fire in tests, `delphi learn list` shows them, staged text never reaches a prompt, and clippy, lint and the workspace tests are green.

## 4. L6b: running cases

**Goal.** `delphi train run` executes cases against a baseline and, optionally, a candidate overlay, checks the results deterministically, and writes a report. It spends money, so the safety rules from L6a (`check_work_home`, required budget) are the first thing it calls.

**Steps.**
1. **Spike first (half a day): the overlay.** Find how a temp home can carry a modified role manifest, skill or heuristics file and still load: read `ratify.rs` (`apply`), `workspace.rs` (`ratified_workspace`, `read_lock`, `hash`) and `home.rs`. The likely answer is that the runner writes the overlay into the temp workspace's live file and its ratified copy and updates `local/ratified.lock` itself (as `/ratify` would, by hash). Confirm with a test that a modified `agents/builder/AGENT.md` loads in the temp home and the real home is untouched. If it cannot be done cleanly, say so in the spec before building more.
2. **Template home.** Like the bench runner: `--template-home <dir>` (a home whose model access an operator ratified once), copied into a fresh marked working home per run (`prepare_work_home`). Only `[workspace] projects` is edited.
3. **Fixtures.** For each case and seed: copy `fixture/` into a temp git repo (init, commit), point the workspace at it, run `delphi run` as a subprocess with `--json`, `--budget` from the case cap, `--timeout` from the case, `--on-card deny` (or a policy file for Amber gate commands, as `dev/bench/arms-example/policy.toml`).
4. **Checks.** Read the mission's `events.jsonl` from the temp home and evaluate the case's `[[check]]` entries (`train.rs` `Check`). `outcome` can use `learn::task_records`. File checks compare against the fixture copy. Write one JSONL row per case, seed and arm (mirror `bench.py` rows: `case, arm, seed, exit_code, wall_secs, cost_usd, passed, failed_checks`).
5. **Budget.** The session budget is enforced across cases: stop before a case whose cap would exceed the remainder. `--dry-run` is `plan`.
6. **Report.** Per-case pass rates across seeds, deltas between arms, cost, and a "no evidence" label when the delta is inside the measured spread. Numbers only. Do not print model output.
7. **Run the two seed cases for real** (operator-gated: it spends money and needs a ratified template home). The expected result is that some checks need adjusting; edit them and say so in `dev/train/README.md`.

**Tests.** Unit tests with a fake `delphi` binary (a script that writes a canned `events.jsonl` and exits with a given code) so the runner is tested without a model. Test budget stop, refusal of a real home, check evaluation for each check kind, and the report's noise label.

**Traps.** The runner must never use the operator's `$DELPHI_HOME`. Rubric files (later) must live outside any agent-readable path. Keep the subprocess boundary: no linking to the engine, so the run exercises what an operator runs.

**Done when:** a fake-binary test passes end to end, the overlay spike's conclusion is written down, and (with the operator) the two seed cases have run once on a live daemon.

## 5. After L1 and L6b

- **L2 `propose` and `/retro`.** The `propose` tool writes `local/proposals/<mission>/` (spec 004 §4.2). Apply path goes through `ratify.rs`; dedupe uses the existing lexical and embedding scores in `kb.rs`; scope routing per spec 004 §5.4. Large: split into (a) the tool and storage, (b) apply, (c) the `/retro` deck.
- **L7 seed suites.** Write the `learning-loop` suite as soon as L1 lands; it should fail against a deliberately broken loop (spec AC-7). The other suites can be written from the manifests' blind-spot lists (`agents/*/AGENT.md` §3).
- **L4 scores and exposure** need volume; keep behind the usage gate and OD-2.
- **L9, L10** after L6b and L7.

## 6. Working rules for whoever continues

- Work in a git worktree (`EnterWorktree`); one PR per slice; stack on an unmerged slice only if it depends on it.
- Build lean: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`. Do not delete other worktrees' `target/` directories.
- Before opening a PR, run what CI runs: `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo run -q --locked -p delphi -- lint --repo .`, `cargo test --workspace --locked` (or the crates you touched plus `delphi` bin tests), and for training cases `cargo run -q --locked -p delphi -- train validate`.
- A background job does not merge. Open the PR and report; merge only when the operator says "go" (and remove the worktree and branch right after).
- Never commit past a failing gate. If a test is flaky, say so and rerun; do not skip it.
- Update the spec's §7.1 "As built" and the tracker row in the same PR as the slice, and note every place the build differs from the design.
- Report outcomes plainly: what ran, what did not, what is unverified (the seed cases are unrun).

## 7. Prompt for picking this up

```text
You are continuing the Delphi "layered learning and training mode" build in the repository at the current directory.

Read these first, in order:
1. missions/2026-10-23-foundational-design/learning-build-plan-2026-10-26.md (the working plan: state, verified facts, L1 and L6b steps, traps, rules)
2. specs/031-layered-learning-and-training-mode.md (the design; §7.1 records what is built and where the build differs)
3. missions/2026-10-23-foundational-design/spec-tracker.md (the "Layered learning and training mode" table)
4. GOVERNANCE.md and REVIEW.md before touching anything under agents/, skills/, or ratification code

Built and merged: L0 (snapshot.loaded event, learn::task_records) and L6a (delphi train validate/plan, the case format, the working-home rule, two unrun seed cases).

Your job, in this order, one pull request per slice:
A. L1, the staging tier, as planned in §3 of the plan. Use mission-log events (`learn.staged`), not a new table, and say so in the spec's §7.1.
B. L6b, running cases, as planned in §4. Start with the overlay spike and write its conclusion into the spec before building further. Test the runner with a fake `delphi` binary; do not spend model money without the operator saying so.
Then stop and report. Do not start L2 or later without being asked.

Rules:
- Work in a git worktree (EnterWorktree). Check `git ls-tree origin/main specs/` before taking any spec number.
- Build lean: CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0.
- Before each PR run: cargo clippy --workspace --all-targets --locked -- -D warnings; cargo run -q --locked -p delphi -- lint --repo .; the tests of every crate you touched plus `cargo test -p delphi --bin delphi`; cargo run -q --locked -p delphi -- train validate.
- Do not merge pull requests, push to main, or force-push; open a PR and report. Never commit past a failing gate.
- No model call in any capture trigger; nothing staged is ever loaded into a prompt; staged notes keep the taint of their sources.
- Update spec 031 §7.1 and the tracker row in the same PR as each slice, noting every place the build differs from the design.
- Report plainly: what you ran, what you did not, what is unverified. The two seed training cases are unrun hypotheses; say so if you touch them.

Finish each slice with the PR link, the checks you ran, and the next command for the operator.
```
