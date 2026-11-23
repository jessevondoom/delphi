---
id: spec-125-load-sensitive-tests-made-deterministic
title: "125: Load-sensitive tests made deterministic"
author: architect
timestamp: 2026-11-18T00:00:00Z
status: approved   # by the operator, 2026-11-18, in a session: "4: yes stabilize", in answer to the item "three timing-sensitive tests keep reddening PRs; stabilize them in a small spec of their own". The text below was written after that answer, from the code and the CI logs; the operator has not read it. It names two more tests than the three the operator was told about (section 3, T4 and the listed L1 to L3); T4 is built only because its cause is shown from code, and the operator may strike it. It takes effect when the operator merges the pull request that carries it.
approved_by: operator (2026-11-18)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # every cause in section 3 is read from the code at the cited line and from a CI log or a local run; section 3 says for each test whether the failure was reproduced locally and how. T3's cause is shown by construction (the mechanism gives the same wrong answer), not caught in the act. L1 to L3 have no shown cause and are not built.
source_mission: 2026-11-18-test-stability
tags: [spec, tests, ci, flaky, sandbox, harness, doctor, signals]
---

# Spec 125: Load-sensitive tests made deterministic

> **Status: approved by the operator, 2026-11-18** (see the frontmatter).
> **Amendment 125-A1 (section 10), approved by the operator 2026-11-22:** CI retries a failing test once and records it as flaky; a second failure fails the run.
> **Amendment 125-A2 (section 11), approved by the operator 2026-11-22:** the macOS tests are built once (`cargo nextest archive`) and run in two halves on two runners.
> **Amends, from its approval:** nothing by text. No product behavior changes. The tests keep what they protect in [`specs/092`](092-operator-authorized-tests.md) (the granted seat and the authorized profile) and [`specs/083`](083-learning-signals-and-host-diagnostics.md) (the host chain in `delphi doctor`).

---

## 1. Scope and objectives

Tests that fail on a pull request they have nothing to do with, and pass on a rerun, cost a macOS job each time and teach people to rerun instead of read. Over 2026-11-16 to 2026-11-18 tests of this kind reddened pull requests or `main` again and again. This spec fixes the four whose cause is shown (T1 to T4), and lists the rest (L1 to L3).

1. **Fix the cause.** Each test in section 3 depends on something it does not control: how soon a thread is scheduled, how many processes the user owns, when a descriptor closes, or what another test did to the process's signals. The fix removes that dependence.
2. **Keep what each test protects.** No assertion is loosened, no test is ignored, and no test is retried by these fixes. (Amendment 125-A1, section 10, adds one retry in CI for every test, as a separate measure; it does not replace a fix.)
3. **Say what cannot be made deterministic.** Section 5 names one remainder and the smallest honest handling of it.

Acceptance: section 9.

## 2. Terms

- **Load-sensitive:** the result changes with what else the machine, or another test in the same binary, is doing.
- **Turn:** a lock a test holds while it uses a resource that all tests of a binary (or all processes of a user) share.
- **Soak:** the same test run many times in a row while busy loops hold every core (`dev/flake-soak.sh` does this for the console tests; section 9 uses the same method for any test binary).

## 3. Diagnosis

| | Test | Where | What it depends on | How load changes it | Seen |
| :-- | :-- | :-- | :-- | :-- | :-- |
| T1 | `harness_run::tests::every_tool_but_delphis_own_is_stopped_at_the_call_before_it_can_run` | `crates/delphi-engine/src/harness_run.rs:3367` (`call_run`), assertion at `:3383` | A wall-clock race. The stand-in CLI prints a `tool_use` line, sleeps 0.3 s, then touches a marker. The engine must read the line, decide and kill the CLI (`:921`) inside those 0.3 s. | The test binary runs about 2,800 tests on a few cores. When the task that reads the CLI's output is not scheduled within 0.3 s the marker appears and the test reports "its effect happened before the span was stopped". The kill itself is correct. | GitHub Linux runner, run 44222222661 attempt 1 (pull request #771), `Bash: its effect happened before the span was stopped`; passed on rerun. |
| T2 | `authorized::tests::a_fork_flood_is_contained_by_the_process_limit_and_leaves_nothing_behind`, `authorized::tests::each_canary_notices_its_own_hole` | `crates/delphi-sandbox/src/authorized/tests.rs:745`, `:338`; the limit at `crates/delphi-sandbox/src/lib.rs:238` and `authorized.rs:44` (`PROC_HEADROOM`) | The user's process table. A command under the authorized profile gets `RLIMIT_NPROC` = the user's process count at launch + 128, and the kernel counts every process of the user. Tests that flood take a turn (`FLOOD`, `tests.rs:13`), but the tests that only launch a command under the limit do not (the probe tests hold `PROBE` only), and `the_cap_is_taken_at_launch…` starts 500 idle processes before it takes the turn (`:731`). | A command launched just before another test's flood fills that flood's own cap has no room left: the two caps are within a few processes of each other, so its next `fork` fails with `fork: Resource temporarily unavailable` and its verdict is wrong. The same happens between two copies of the test binary (two worktrees testing at once). More tests running at the same time make the overlap more likely; with `--test-threads=1` the crate passes 82 of 82. | Local runs on 2026-11-17 while other builds ran, and again on 2026-11-18 in another worktree (1 or 2 tests of the crate failed in 5 of 7 reruns, reported with pull request #780, while this spec was being soaked beside it). Not seen on a GitHub runner. |
| T3 | `doctor::tests::host_chain_tests::the_real_probe_connects_and_closes_without_sending_a_byte` | `crates/delphi/src/doctor.rs:1729`, assertion at `:1743` | When the last copy of a listening descriptor closes. The test drops its listener and expects the next probe to be refused (`Stale`). Another test in the binary that starts a child process holds a copy of every open descriptor between `fork` and `exec`; until that child reaches `exec` the socket still listens. | Under load the gap between `fork` and `exec` in another thread is longer, the probe's `connect` lands in it, and the probe answers `Reachable`. | GitHub Linux runner, run 43950558811 (`main`), `left: Reachable, right: Stale`. |
| T4 | `run_granted::tests::the_prompt_leaves_an_ignored_signal_ignored_and_restores_every_disposition` | `crates/delphi/src/run_granted.rs:509`, assertions at `:524` and `:526` | The process's signal dispositions, which every test thread shares. Three tests change them with no turn: this one (`SIGHUP` to ignore, then `NoEcho`), `the_code_is_typed_without_echo…` (`:454`, `NoEcho` twice through `ask_on`), and the `run_ui` test in `crates/delphi/src/chat.rs` (`:1005` sets `SIGINT` to ignore while a child runs). | It is an interleaving, not a speed: when `run_ui` has `SIGINT` ignored, `NoEcho` leaves it ignored and `:524` fails; when the other `NoEcho` test restores `SIGHUP` between this test's two steps, `:526` fails. A slower machine widens both windows. | macOS runner on `main`: runs 44090396475, 44094083528, 44216693476, 44235518849 (four of the fourteen `main` runs from 2026-11-17 03:00 to 2026-11-18 06:00 UTC), all at `:524` or `:526`. Linux runner, run 44254012318 (pull request #778, which does not touch the file): `:523`, "an ignored signal stays ignored", which is the other `NoEcho` test putting `SIGHUP` back to its default between this test setting it to ignore and reading it. |

What was reproduced before any fix, on an 8-core Mac on 2026-11-18, with the test binary built from `main` (02651db7) and run in a loop beside 8 to 40 busy loops (the method of `dev/flake-soak.sh`):

- **T4:** the three tests named in the row, run together, failed 50 of 50 runs (49 at `:524`, one at `:523`).
- **T2:** the module `authorized::tests` failed 5 of 10 runs, each time in `a_fork_flood_is_contained…`: the sweep printed 12 of its 22 attempts and then died with exit code 128, before the flood. `each_canary_notices_its_own_hole` did not fail in those 10 runs; its exposure is read from the code.
- **T1:** not reproduced. The test alone passed 20 of 20 runs beside 24 busy loops. The cause is read from the code and from the one runner log; AC-2 checks the new test by removing the kill instead.
- **T3:** not caught in the act. Shown by construction: a listener that is closed while a copy of its descriptor is open still accepts a `connect`, and a socket that was bound and never listened on refuses one whether a copy is open or not (checked on macOS; the failure was on Linux, where the kernel's rule is the same).

Listed, with no shown cause, and not built here:

| | Test | Seen | What is known |
| :-- | :-- | :-- | :-- |
| L1 | `harness_run::tests::a_granted_seat_whose_cli_loaded_any_server_but_the_bridge_exactly_once_is_stopped` (`harness_run.rs:3458`) | macOS, run 44106485163 attempt 1 | The launch was refused with "the Claude CLI's version could not be determined": `cli_version_bounded` (`crates/delphi-engine/src/capabilities.rs:144`, called with 5 seconds at `harness_run.rs:773`) returned nothing for a stand-in script that prints one line. Either the 5 seconds ran out on a loaded runner or the script could not be started; the logs do not say which. |
| L2 | `authorization_grant::tests::claude_seat::a_claude_builder_seat_with_only_bridge_tools_raises_a_run_command…` (`authorization_grant/tests.rs:769`) | macOS, runs 44023346843 and 44106485163 attempt 1 | The stand-in CLI never wrote its arguments file, so it never started; probably the same refusal as L1, but the test does not print why. |
| L3 | `authorization_use::tests::the_red_team_sweep_runs_through_the_seat_with_all_22_attempts_blocked_and_one_use` (`crates/delphi-engine/src/authorization_use/tests.rs:408`) | once under load on a developer machine (reported with pull request #780); the output was not kept | It runs the same sweep under the same process limit as T2, from the engine test binary, which takes no turn: the sweep of `a_weakened_profile_fails_the_sweep_through_the_seat` (`:438`) may flood beside it, some 2,800 other tests start processes around it, and a sandbox test binary may flood from another process. T2 is the likely cause. Whether the failure was that is not shown. |

Section 6 says what L1 and L2 need before anyone changes them.

## 4. The fix for each

Principles, for these four and for any later one: remove the dependence; never add a retry of the test or of the assertion; never `#[ignore]`; never widen a tolerance to hide a failure; a wait is allowed only for an event that must happen (with a generous bound that turns a hang into a failure), never as a guess at how long something takes.

1. **T1: the stand-in CLI is driven by a gate, not by a sleep.** The CLI writes its process id, prints the `tool_use` line, then waits for a gate file that only the test creates, and only then would touch the marker. The test creates the gate after `run_task` has returned. It asserts, in this order: the span ended without `Done` and with the `authorization.refused` event (as now); the CLI's process no longer exists at that moment (the kill is synchronous, `harness.rs:1362`); the marker does not exist. What this proves is an order, which is what spec 092 asks for: the span is stopped on the call itself, and needs no further word from the CLI (the CLI says nothing more until a gate that opens after the span is over). If the engine waited for a result the run would not return; a bound of 60 seconds turns that into a failure. **What is given up, said plainly:** the old test also failed if the engine took longer than 0.3 s to react. No number of milliseconds is promised anywhere, and none can be asserted without a clock, so that part is not kept.
2. **T2: one turn for the user's process table.** A reader-writer turn replaces `FLOOD`: a test that floods, or that starts or ends hundreds of processes, takes it exclusively (as the sweeps do now, plus the 500 idle processes of `the_cap_is_taken_at_launch…`, for the whole test); every other test that launches a command under the authorized profile's process limit takes it shared (the probe tests, `run_in`, the `set -m` test, the quota test, the refusal test at `:205`). The turn is also a file lock (`flock` on one file per user in the temporary directory), so two copies of the test binary take turns too. Nothing about the profile, the headroom or the sweep changes, and the real limit is still what is tested.
3. **T3: a socket file that nobody ever listened on.** The `Stale` case is built by binding a socket and closing it without `listen`. A `connect` to it is refused whether or not a copy of its descriptor is still open somewhere, on Linux and macOS. The `Absent` and `Reachable` cases, and the check that the probe sends no byte, are unchanged.
4. **T4: one turn for the process's signal dispositions.** The three tests named in the table take one lock for their whole body. The product has one prompt at a time, so nothing in the product changes.

## 5. What is not made deterministic

T2 keeps one dependence that a test cannot remove: the limit counts every process of the user, so if programs outside the tests start more than 128 processes for that user during one sandboxed command, the command's `fork` fails. That is the product's documented behavior (it fails closed, `authorized.rs:38`), not a test fault. The smallest honest handling, and the one built: when a sweep or a probe gives an unexpected verdict, the failure message carries the user's process count before and after the run and the end of the command's standard error, so a reader sees "the count rose by N during the run" and bash's `fork: Resource temporarily unavailable`, not a riddle. No retry.

The case seen in practice on a developer's machine is a copy of these same tests from a worktree that predates this spec: it takes no turn, and it starts 500 idle processes and floods beside ours (caught on 2026-11-18: the count went from 420 to 705 before a sweep's launch and to 1,052 during it, while another worktree ran `cargo test -p delphi-sandbox`). The file lock ends that only when both copies have it, that is, once this spec's slices are merged and the other branches have taken `main`. A GitHub runner runs one copy and is not exposed.

## 6. Not built here, and why

- **L3.** The file lock of T2 could be taken by the two engine sweep tests as well, and would end the overlap with each other and with the sandbox crate. It would not end the rise from the other engine tests, which no turn can cover. It is left until a failure is caught with its output, so that the fix is aimed at what happened.
- **L1 and L2.** No cause is shown. The first step is a test-only change that prints why the version was not read (deadline, start failure, exit status or empty output), which needs a failure to be caught first. Raising the 5 seconds or retrying the version call would be a guess, and the 5 seconds are product behavior of spec 092.
- **A general retry in CI.** `CONTRIBUTING.md` said there is none, and this spec kept that until amendment 125-A1 (section 10, operator, 2026-11-22), which adds exactly one retry per failing test.
- **The console pty tests.** They were stabilized in #154 and #631 and none failed in this window.
- **A change to `PROC_HEADROOM`, to the sweep or to the probe.** They belong to spec 092.
- **Any file under `.github/` or in the merge-gate list.** None is touched; if a fix turns out to need one, the slice stops and reports.

## 7. Options considered

| | Option | How it would be tested |
| :-- | :-- | :-- |
| A, conventional | Retry and serialize: rerun a failed test up to three times (a wrapper or `nextest` retries), and run the three crates with `--test-threads=1`. | Soak each test 50 times under load and count runs that needed a retry. Rejected: a retry hides a real regression that fails one time in three, serial runs cost minutes on the scarce macOS job, and neither removes a cause (T4 fails without any load). |
| B, removes a constraint | Remove the constraint that these tests share one process with thousands of others: move the four into their own integration-test binaries, one process each. | The same soak, plus a check that the new binaries run in CI. Rejected for now: T1 and T2 would still race (the 0.3 s and the user's process table are not per-process), the tests reach crate-private items, and new test binaries add link time to every job. |
| C, by a named method (inversion) | Ask "what would make each test fail for certain?" and remove exactly that: T1 fails if the reader is late, so the CLI waits for the test instead of the clock; T2 fails if another launch overlaps a flood, so launches and floods take turns; T3 fails if a copy of the listener survives, so the socket never listens; T4 fails if another test moves a signal, so they take turns. | For each test, first make it fail on purpose by forcing the bad interleaving (a delay before the reader, a flood beside a probe, a held copy of the listener, an ignored `SIGINT`), then show the fixed test passes with the same forcing, then soak 50 runs under load. Chosen. |

### Simpler Alternative Considered

Longer sleeps and wider tolerances (0.3 s to 3 s in T1, a second probe attempt in T3). It is a smaller change and would make the failures rarer. Rejected: it keeps every dependence, makes T1 take half a minute, and the next slower runner brings the failures back.

## 8. Authority and trust

- Test code only (`#[cfg(test)]` modules). No product code path changes, so nothing can act outside the sandbox, approve its own action or raise trust that could not before.
- No test is removed, ignored or retried, and no assertion that guards spec 092 is loosened. T1 gives up one unpromised timing bound and says so (section 4.1).
- The file lock of T2 is a plain advisory lock on an empty file in the temporary directory; it holds no data and is released when the test process ends.
- Tests are software: each slice carries `Implements-Spec: specs/125-load-sensitive-tests-made-deterministic.md` and a tracker update. No `.github/`, charter, rubric or merge-gate path is touched.

## 9. Slices and acceptance

| Slice | What | Depends on |
| :-- | :-- | :-- |
| S0 | this spec | approval |
| S1 | T1, `crates/delphi-engine` (section 4.1) | S0 |
| S2 | T2, `crates/delphi-sandbox` (sections 4.2 and 5) | S0 |
| S3 | T3, `crates/delphi` doctor (section 4.3) | S0 |
| S4 | T4, `crates/delphi` signals (section 4.4) | S0 |

One pull request for S0; one for S1 to S4 under the same-spec trial (one commit per slice, each with its trailer and tracker line).

- **AC-1** Each fixed test passes 50 consecutive runs of its test binary's filter while busy loops hold every core (the method of `dev/flake-soak.sh`), and the pull request reports the counts before and after. Where the old failure could be forced, the pull request says how and shows that the same forcing no longer fails.
- **AC-2** For T1, an engine that does not stop at the call (the check at `harness_run.rs:920` switched off) makes the new test fail (checked by hand once, reported, not committed). The test cannot tell a kill issued at the call from one issued a few statements later when the span ends; the old test could not either unless those statements took more than 0.3 s.
- **AC-3** No test in the three crates is ignored, removed or retried by these slices, and the count of tests does not fall.
- **AC-4** All gates pass for every slice: `cargo test --workspace --locked`, clippy with `-D warnings`, and `delphi lint --repo .`.
- **AC-5** The tracker says, for each test, what was reproduced, what was not, and what remains load-sensitive (section 5, L1 to L3).

## 10. Amendment 125-A1: a failing test is retried once in CI, and a flaky pass is recorded (operator, 2026-11-22)

**Approved by the operator, 2026-11-22**, in a session, in two answers: "retrying flaky tests is fine and a second failing test should continue to fail the run", and, as the go to build it, "can we start on C and D then the new slice you mentioned above while we wait on 881?" (C is this amendment). The text below was written after those answers; the operator has not read it. It takes effect when the operator merges the pull request that carries it.

This amendment reverses one rule of this spec and of `CONTRIBUTING.md`: there was no automatic retry of tests (section 1 item 2, section 6, section 7 option A). Everything else in sections 1 to 9 stands: a flaky test still gets its cause fixed when the cause can be shown, by the principles of section 4.

### 10.1 Why

Measured on 2026-11-22: `main` was red on 20.7% of its CI runs over the ten days before, and about 25 of the 37 failures that could be classified were tests that passed on a rerun of the same commit. The most frequent were the granted Claude seat tests whose stand-in CLI's `--version` probe ran out of time on a loaded macOS runner (L1, L2 of section 3, still without a shown cause), `service::tests::cancelling_a_task_waiting_on_an_approval_…` (an event read before it was written), `run_granted::tests::the_prompt_leaves_an_ignored_signal_ignored_…` (T4, before S4), `doctor::…::the_real_probe_connects_and_closes_without_sending_a_byte` (T3, before S3), `investigation_tests::…` and `kb::tests::episodes_are_indexed_under_zone_episodes_…`. Each red run cost a rerun of the failed jobs, a macOS job each time, and taught the habit section 1 warns about: rerun instead of read. The fixes of section 4 work one test at a time and need a failure caught with its output; the failures arrive faster than that.

### 10.2 The rule

1. **Every failing test is run once more, alone, in a fresh process.** If it passes, the test counts as passed and the run goes on; the test is recorded as **flaky**. If it fails again, it has failed and the run fails, as before. There is no second retry.
2. **A flaky pass is visible, never silent.** The job's summary page lists every flaky test by name; the JUnit report (kept as an artifact of the run) records the first failure with its output (`<flakyFailure>`); the test log marks it `FLAKY`.
3. **A test flaky twice gets an owner.** A test recorded flaky in two runs (on `main` or on any pull request) gets an issue naming the runs, and someone who owns it: a fix by section 4, or a written finding why no fix is possible. The retry never closes such an issue.
4. **Doc tests are not retried.** The runner that retries (below) does not run doc tests; they stay on `cargo test --doc`, as before. None of them has been seen to fail by load.
5. **Locally** the same rule holds where the same runner is installed: `delphi forge finish --gates` already reruns a failing test once by name and reports it flaky (spec 135, F1b); with the runner it uses the same profile (slice R4).

### 10.3 Why this does not hide bugs

- A test that fails twice in a row still fails the run. A real regression fails every time and is stopped as before.
- A regression that fails only some of the time (a new race) is the case a retry can let through: one that fails a third of the time passes a run with one retry eight times in nine. It is not hidden: each of its failures is recorded flaky with its output, and by rule 3 its second record makes an issue with an owner. Without the retry the same regression was rerun by hand until green, and nothing was recorded at all; the record is the improvement.
- Spec 092's guarded tests (the granted seat, the authorized profile, the sweep) assert what never happens. A retry cannot turn a "never" into a pass: a run where the forbidden effect happened fails, and it fails again on the retry unless the effect is itself a race, which is then recorded.

### 10.4 Options considered

| | Option | How it would be tested |
| :-- | :-- | :-- |
| A, status quo | No retries: a red test turns the run red; people rerun failed jobs by hand. | Already measured: 20.7% of `main` runs red, about two in three of them flakes. Rejected: it costs a macOS job and a person's attention each time, and records nothing about which test failed. |
| B, conventional | Retry once but report a flaky pass as a failure (`flaky-result = "fail"` in nextest). | It would give the record of rule 2 and keep every flaky run red. Rejected: the run is as red as today, so it saves nothing, and the operator chose that a pass on the retry passes. |
| C, removes a constraint | Quarantine: move known-flaky tests to a separate job that may fail, or mark them ignored. | Count how many quarantined tests are ever fixed. Rejected: a quarantined test protects nothing while it waits, section 4 forbids `#[ignore]`, and the list grows quietly; the tests named in 10.1 guard spec 092's sandbox and seat. |
| D, chosen | Retry once, pass on the retry, record the flaky pass where people look (rule 2), and give a repeat offender an owner (rule 3). | The pull request that builds it shows, in its own CI runs: the same test count as before, the summary page, the date-test skip of spec 093-A1 still working; afterwards, the share of red `main` runs and the flaky list per run. |

### 10.5 How it is built

The retry is cargo-nextest's (`retries = 1` in a `ci` profile in `.config/nextest.toml`), which runs each test in its own process and retries only the failed test. nextest's own default already treats a pass on the retry as a pass and marks it flaky. Running each test in its own process also ends, by itself, the class of failure of T4 (process-wide signal state shared by tests of one binary) and the descriptor copies of T3. The test selection does not change: the Linux unit job runs the library tests, the Linux integration job the binaries' tests and the integration tests (with spec 093-A1's skip of the review-date test, written as a nextest filter, still checked to name exactly one test), and the macOS job all three; doc tests stay on `cargo test --doc`. nextest is installed at a pinned version from its release archive, checked against a SHA-256 written in the workflow. `[test-groups]` run a few named families one at a time among themselves where they share a scarce thing (R2); a group never replaces the retry or a fix.

### 10.6 Slices

| Slice | Content |
| :-- | :-- |
| A1 | this amendment and its tracker entry |
| R1 | `.config/nextest.toml` with a `ci` profile (`retries = 1`, `fail-fast = false`, a `slow-timeout`, a JUnit file); nextest pinned and checksum-verified in CI; `test-linux-lib` and `test-linux-integration` on `cargo nextest run --profile ci` with the same selection; doc tests on `cargo test --doc`; the JUnit file uploaded as an artifact (always) and a job summary naming every flaky test; `CONTRIBUTING.md` says so |
| R2 | `[test-groups]` with `max-threads = 1` for the load-sensitive families named in 10.1, each justified in the file |
| R3 | the macOS job on the same profile and summary; Clippy unchanged |
| R4 | local parity: `delphi forge finish --gates` uses nextest with the `ci` profile when it is installed (doc tests then by `cargo test --doc`), and its own rerun otherwise. Rust code, so a separate pull request (no `.github/` path beside code) |

R1 to R3 change `.github/` and are gate-changing: the operator merges them at a terminal.

### 10.7 Verification plan

- Before and after: the number of tests each job runs, from `--list` (`cargo test … -- --list` before, `cargo nextest list` after); the union must be the same.
- The pull request's own CI: the jobs pass on nextest; the summary page renders (with "no flaky tests" or a list); the JUnit artifact is there; the integration job's log shows the review-date test filtered out and the once-only check passing.
- `actionlint` on the workflow before the push.
- After merge: the flaky list of each `main` run, and the share of red `main` runs over the next ten days against the 20.7% above.

## 11. Amendment 125-A2: the macOS tests are built once and run on two runners (operator, 2026-11-22)

**Approved by the operator, 2026-11-22**, in a session: "keep building optimistically in the order you defined", after "keep building anything you can" earlier the same day. The order named this item I1 of the CI velocity plan: build the macOS tests once, split them across two runners from a `cargo nextest archive`, then measure. The text below was written after those answers; the operator has not read it and no Council has reviewed it. It changes `.github/workflows/`, so its pull request is gate-changing (spec 094 §3.4) and the operator merges it at a terminal. It changes nothing in sections 1 to 10: the same tests run under the same `ci` profile (one retry, flaky recorded, 125-A1), with the same review-date skip (spec 093-A1).

### 11.1 Why

Measured on 2026-11-22: the macOS job is the whole critical path of CI. Its median is about 19.9 minutes (the `main` run 45086371128: 21.0 minutes, of which the test step 17.9: 3 m 22 s to build, 14.4 minutes to run 5,858 tests, 28 skipped; Clippy 1.6 minutes). The slowest Linux job takes about 5 minutes. A pull request waits for the macOS job; every minute cut from it is a minute cut from every ready pull request and every `main` run. The test run is the largest part, and a `macos-latest` runner has three cores, so a second runner halves it.

### 11.2 The rule

1. **One build.** A job `build-macos` compiles every test binary of the workspace once and packs them with `cargo nextest archive` (the binaries, and the output directories of build scripts that nextest needs). It hands the archive to the test jobs as a workflow artifact kept one day. The same job runs the doc tests, Clippy (also when the archive fails), the two Python self-tests, and saves the build directory on a push to `main`, as the one job did.
2. **Two halves.** A job `test-macos`, a matrix of two, downloads the archive into a fresh checkout and runs `cargo nextest run --archive-file … --partition slice:N/2` under the `ci` profile. Each half writes its own JUnit report (`junit-macos-1`, `junit-macos-2`) and its own job summary of flaky tests. A failure in one half does not cancel the other (`fail-fast: false`).
3. **The same tests.** The two halves together run exactly the tests the one job ran: `slice` partitions the one sorted list of tests that the filter selects, so each test is in exactly one half. The review-date skip of spec 093-A1 is the same filter, applied in each half, and each half checks, over the whole archive, that the name matches exactly one test (the Linux integration job's check; the one macOS job did not have it).
4. **The checkout stays.** The archive is unpacked into the checkout (`--extract-to .`, `--workspace-remap .`), so the build directory sits at `target/` under the same absolute path it had when it was compiled. Tests read fixtures through `CARGO_MANIFEST_DIR` and binaries through `env!("CARGO_BIN_EXE_delphi")`, both compiled-in paths; nextest reads `.config/nextest.toml` from the checkout.
5. **Drafts as before.** While a pull request is a draft both macOS jobs are skipped; `ci-gate` (spec 094 §12.2.1) needs both and accepts both skipped only on a draft pull request.

### 11.3 Cost in macOS jobs

A run takes three macOS jobs instead of one; the account may run five at once. The build job and the two halves never overlap, so one run holds at most two macOS slots at any moment, for a shorter time. Measured over the days before, the macOS queue's 90th percentile was about 0.1 minutes: slots were rarely all taken. With two runs in flight at once (two ready pull requests, or a pull request and `main`) four slots are in use; a third run then waits for a slot. The build is no longer done twice for nothing: the one runner used to build and test in series, and the extra minutes are the two halves' setup (a checkout, nextest, the download), about a minute each.

### 11.4 Options considered

| | Option | How it would be tested |
| :-- | :-- | :-- |
| A, conventional | Each half builds the tests itself (two full jobs with `--partition`, no archive). | Two runners each spend the ~3.4-minute build (more on a cold cache) and restore the ~GB build directory; the critical path keeps the build plus half the tests. Rejected: it pays the build twice for the same gain, and doubles the cache traffic. |
| B, removes a constraint | Remove the one-runner constraint fully: three or four halves. | The same measurement with `slice:N/3`. Not now: each extra half costs a macOS slot and a minute of setup for a shrinking gain, and a run of four slots leaves one for every other run. Measure two first (11.6); three is a one-line change if two leaves the macOS path the longest. |
| C, by a named method (build once, test many: nextest's "reuse build") | One build job archives the test binaries; N test jobs run partitions of the archive. | This pull request's own ready run: both halves pass, the counts add up to the one job's, and the critical path is the build job plus the slower half. **Chosen.** |

Partition: `slice` (every Nth test of the one sorted list of all tests) over `hash` (each test by a hash of its name) and `count` (every Nth test within each binary). `slice` gives halves of equal size and spreads the long families (the `investigation_tests`, the sweeps, the granted seat tests, which sit together in the sorted list) across both; `hash` keeps a test in the same half when others are added but can leave the halves uneven; `count` balances each binary but not the whole run. The halves need only be even, not stable.

### 11.5 Slices

| Slice | Content |
| :-- | :-- |
| P1 | this amendment; `build-macos` and the two `test-macos` halves in `ci.yml`; `ci-gate`'s needs and its draft rule; `CONTRIBUTING.md`'s sentence on the jobs |

P1 changes `.github/` and is gate-changing: the operator merges it at a terminal.

### 11.6 Verification plan

- `actionlint` and a YAML parse of the workflow before the push; the shell of the test step run under `/bin/bash` 3.2 with `-eo pipefail`.
- Locally on macOS before the push: `cargo nextest archive`, then `cargo nextest list --archive-file …` with each partition, from a second copy of the checkout; the two lists are disjoint and their union equals `cargo nextest list --workspace`.
- On this pull request's own ready run: both halves pass; the tests run in the two halves (plus the skipped and the one review-date test filtered out) add up to the count of the one macOS job on `main` (5,858 run and 28 skipped on run 45086371128; the pull request's own count is the one that counts, since tests are added every day); the review-date check finds exactly one test; the flaky summary and both JUnit artifacts are there. The critical path (the build job's time plus the slower half's, from the job timestamps) is reported against the one job's 19.9-minute median.
- After merge: the macOS path on `main` and on ready pull requests over the next week, against the median above. If the slower half plus the build is still longer than every Linux job by more than a few minutes, try three halves (option B).
