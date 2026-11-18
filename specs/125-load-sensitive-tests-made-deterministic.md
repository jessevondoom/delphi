---
id: spec-125-load-sensitive-tests-made-deterministic
title: "125: Load-sensitive tests made deterministic"
author: architect
timestamp: 2026-11-18T00:00:00Z
status: approved   # by the operator, 2026-11-18, in a session: "4: yes stabilize", in answer to the item "three timing-sensitive tests keep reddening PRs; stabilize them in a small spec of their own". The text below was written after that answer, from the code and the CI logs; the operator has not read it. It names two more tests than the three the operator was told about (section 3, T4 and the listed L1 and L2); T4 is built only because its cause is shown from code, and the operator may strike it. It takes effect when the operator merges the pull request that carries it.
approved_by: operator (2026-11-18)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # every cause in section 3 is read from the code at the cited line and from a CI log or a local run; section 3 says for each test whether the failure was reproduced locally and how. T3's cause is shown by construction (the mechanism gives the same wrong answer), not caught in the act. L1 and L2 have no shown cause and are not built.
source_mission: 2026-11-18-test-stability
tags: [spec, tests, ci, flaky, sandbox, harness, doctor, signals]
---

# Spec 125: Load-sensitive tests made deterministic

> **Status: approved by the operator, 2026-11-18** (see the frontmatter).
> **Amends, from its approval:** nothing by text. No product behavior changes. The tests keep what they protect in [`specs/092`](092-operator-authorized-tests.md) (the granted seat and the authorized profile) and [`specs/083`](083-learning-signals-and-host-diagnostics.md) (the host chain in `delphi doctor`).

---

## 1. Scope and objectives

Tests that fail on a pull request they have nothing to do with, and pass on a rerun, cost a macOS job each time and teach people to rerun instead of read. Over 2026-11-16 to 2026-11-18 five such tests reddened pull requests or `main`. This spec fixes the four whose cause is shown, and lists the rest.

1. **Fix the cause.** Each test in section 3 depends on something it does not control: how soon a thread is scheduled, how many processes the user owns, when a descriptor closes, or what another test did to the process's signals. The fix removes that dependence.
2. **Keep what each test protects.** No assertion is loosened, no test is ignored, and no test is retried.
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
| T2 | `authorized::tests::a_fork_flood_is_contained_by_the_process_limit_and_leaves_nothing_behind`, `authorized::tests::each_canary_notices_its_own_hole` | `crates/delphi-sandbox/src/authorized/tests.rs:745`, `:338`; the limit at `crates/delphi-sandbox/src/lib.rs:238` and `authorized.rs:44` (`PROC_HEADROOM`) | The user's process table. A command under the authorized profile gets `RLIMIT_NPROC` = the user's process count at launch + 128, and the kernel counts every process of the user. Tests that flood take a turn (`FLOOD`, `tests.rs:13`), but the tests that only launch a command under the limit do not (the probe tests hold `PROBE` only), and `the_cap_is_taken_at_launch…` starts 500 idle processes before it takes the turn (`:731`). | A command launched just before another test's flood fills that flood's own cap has no room left: the two caps are within a few processes of each other, so its next `fork` fails with `fork: Resource temporarily unavailable` and its verdict is wrong. The same happens between two copies of the test binary (two worktrees testing at once). More tests running at the same time make the overlap more likely; with `--test-threads=1` the crate passes 82 of 82. | Local runs on 2026-11-17 while other builds ran. Not seen on a GitHub runner. |
| T3 | `doctor::tests::host_chain_tests::the_real_probe_connects_and_closes_without_sending_a_byte` | `crates/delphi/src/doctor.rs:1729`, assertion at `:1743` | When the last copy of a listening descriptor closes. The test drops its listener and expects the next probe to be refused (`Stale`). Another test in the binary that starts a child process holds a copy of every open descriptor between `fork` and `exec`; until that child reaches `exec` the socket still listens. | Under load the gap between `fork` and `exec` in another thread is longer, the probe's `connect` lands in it, and the probe answers `Reachable`. | GitHub Linux runner, run 43950558811 (`main`), `left: Reachable, right: Stale`. |
| T4 | `run_granted::tests::the_prompt_leaves_an_ignored_signal_ignored_and_restores_every_disposition` | `crates/delphi/src/run_granted.rs:509`, assertions at `:524` and `:526` | The process's signal dispositions, which every test thread shares. Three tests change them with no turn: this one (`SIGHUP` to ignore, then `NoEcho`), `the_code_is_typed_without_echo…` (`:454`, `NoEcho` twice through `ask_on`), and the `run_ui` test in `crates/delphi/src/chat.rs` (`:1005` sets `SIGINT` to ignore while a child runs). | It is an interleaving, not a speed: when `run_ui` has `SIGINT` ignored, `NoEcho` leaves it ignored and `:524` fails; when the other `NoEcho` test restores `SIGHUP` between this test's two steps, `:526` fails. A slower machine widens both windows. | macOS runner on `main`: runs 44090396475, 44094083528, 44216693476, 44235518849 (four of the fourteen `main` runs from 2026-11-17 03:00 to 2026-11-18 06:00 UTC). |

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

- **L1 and L2.** No cause is shown. The first step is a test-only change that prints why the version was not read (deadline, start failure, exit status or empty output), which needs a failure to be caught first. Raising the 5 seconds or retrying the version call would be a guess, and the 5 seconds are product behavior of spec 092.
- **A general retry in CI.** `CONTRIBUTING.md` says there is none, and this spec keeps that.
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
- **AC-5** The tracker says, for each test, what was reproduced, what was not, and what remains load-sensitive (section 5, L1, L2).
