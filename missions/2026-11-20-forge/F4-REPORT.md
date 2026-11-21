---
id: forge-f4-report
title: "F4: the forge experiment (spec 135 section 7), parts 1 and 2"
status: active
tags: [forge, f4, review, cross-family, experiment, spec-135]
author: architect
timestamp: 2026-11-21T00:00:00Z
source_type: internal_empirical
confidence: medium
source_mission: 2026-11-20-forge
---

# F4: the forge experiment (spec 135 §7), parts 1 and 2

Run 2026-11-21 on the operator's yes ("run f4 tests. you have my permission to spend tokens"; kimi and mai added at the operator's request). Part 3 (claims by use over two weeks) has not started: `delphi forge` is not in a release yet.

**Verdict: F3 (writer seats inside the daemon) does not earn a spec yet.** The cross-family review found real defects, including one live on `main` that no human review caught. But it found the known defect in only 3 of 12 reviews (plus 2 partial), and it cannot run Rust tests in its sandbox today. The cheaper follow-ups below come first.

## Setup

- A scratch Delphi home in the job's temporary directory, running `main` at `7fe01fed` (after #827), with the operator's endpoints ratified there by the operator (`delphi setup` at their terminal). The operator's real home and daemon were not touched.
- `request_review` called through `delphi mcp` from a worktree of each pull request's pre-fix head, diffed against that PR's base, with the PR's spec, one family per call, `operator_confirmed: true`.
- Families: gemini (`agy-sub`), openai (`copilot-openai`, GPT), kimi (`copilot-kimi`, kimi-k3), mai (`copilot-mai`, mai-code-2). The host is Claude, so Claude never reviewed.
- Configured review command: one offline `cargo test` of `delphi-core`. It never ran (section 4), so every review saw the same "could not start" output. The comparison between families is fair; no review had test evidence.
- Ground truth was fixed before any review ran: three merged pull requests whose defect a later pull request fixed.

| PR | Defect (fixed by) | Diff |
| :-- | :-- | :-- |
| #448 research planner (spec 066) | `private_token` lets `jdoe@buildbox`, `buildbox:8080`, bare `localhost` and `build.internal!` into the external search query: the e-mail rule needs a `.`, the host rule needs a dot, scheme or IP (#815) | 17.6k chars |
| #550 Council Round 2 reader (spec 003) | a valid critique is refused as unreadable: Markdown emphasis around the verdict is not stripped, and a verdict-only restatement counts as a conflict (#613) | 24.8k chars |
| #537 failure kinds (spec 039) | `failure_kind` misses "The operator denied this action", so every refused card is counted `tool_error`; the string is in `engine.rs`, outside the diff (#539) | 27.3k chars |

## 1. Did the reviewer find the known defect?

| PR | gemini | openai (GPT) | kimi-k3 | mai-code-2 |
| :-- | :-- | :-- | :-- | :-- |
| #448 | **caught** (`localhost`, `localhost:8080`: no dot or scheme, so never checked) | partial (right function; names IPv6 and socket addresses, not the dotless rule) | **caught** (`user@single-label-host`: both rules named exactly; it asked to `grep` for `is_internal_host` first) | **caught** (`localhost:3000`: the host check needs a scheme, dot or IP) |
| #550 | partial (identical repeats falsely refused; not the emphasis mis-parse) | missed | missed, voted **accept** | missed |
| #537 | missed | missed | missed | missed |

**3 caught, 2 partial, 7 missed of 12.** Verdict "changes" in 11 of 12; the one "accept" (kimi, #550) was on a PR with a known defect.

What the misses share:

- **#550:** every family looked for the opposite failure, a conflict that slips through. None looked for a valid answer thrown away.
- **#537:** the defect needs a fact outside the diff, and no reviewer asked for it. Only kimi used the extra-command round (three `grep`s on #448, which is where it got its precise answer).

Wall time per review: openai 65-85 s, mai 65-105 s, kimi 125-265 s, gemini 225-325 s.

## 2. What else they found

Findings raised by two or more families independently:

| PR | Finding | Families | Checked |
| :-- | :-- | :-- | :-- |
| #537 | the `tool_failed` log record passes `arg_keys`, but `elog` keeps only `CONTEXT_KEYS`, which never had it: argument names are dropped | gemini, kimi, openai | **true, and still live on `main`** (`crates/delphi-core/src/elog.rs` `CONTEXT_KEYS`; `engine.rs` `tool_failed`) |
| #537 | `failure_kind` reads words in the echoed `$ <command>` line, so a failed command can be counted `denied` or `invalid_args` | gemini, kimi, mai, openai | plausible; not checked against `main` |
| #550 | a conflicting JSON critique spread over several lines skips conflict detection | gemini, mai, openai | plausible; not checked |
| #550 | the trailer text is still written into the `council.critique` event, against the spec's "counts and byte lengths, never text" | mai, openai | not checked |
| #448 | the Topic query is planned before the trusted-domain query in survey and deep modes | gemini, openai | not checked |
| #448 | new citations for a claim the library already knows are dropped, not merged | kimi, openai | not checked |

The `arg_keys` defect is the most useful result of the run: three families found a live bug that two human-reviewed pull requests (#537 and #539) did not. Fix: add `"arg_keys"` to `CONTEXT_KEYS` (spec 039), a one-line change with its own test.

## 3. Part 2: would the finisher have caught CI drift?

Rarely, and when it would, the chore is small. From `gh run list` (182 failed `ci` runs, 83 branches, 2026-11-01 to 2026-11-21):

- **First failure of a branch:** 1 of the newest 40 was a stale generated file (`compiled/`), which `forge finish --check` covers. 39 were other causes: flaky tests, CI infrastructure (apt, cancelled jobs, `cargo deny` advisories), real bugs.
- **Any failed run:** 16 of 182 (about 9%) were drift. The finisher covers 15; the miss is `avatar::tests::ui_spec_fixture_is_current`.
- **5 of the 16 were on `main`:** two pull requests each current on their own, stale together after merging. A check before pushing cannot see that; only a check after updating from `main`, or in the merge step, can.
- **The tracker check** prevented nothing observed: no CI test reads the tracker.

## 4. F2 problems the run exposed

1. **Rust tests cannot run in the investigation sandbox.** `cargo` is not on the sandbox's `PATH` (`execvp() of 'cargo' failed`, exit 71), and given an absolute path the profile refuses to execute it under `~/.rustup` ("Operation not permitted", exit 126). Running them needs the profile to execute the toolchain and read the cargo registry. That widens the sandbox, so it is a spec decision for the operator. **Correction (2026-11-21, spec 135 §14.2):** no code widening is needed. The sandbox already takes `[exec] toolchain` (read-only and executable) and `[exec] env.PATH` from the operator's configuration; the scratch configuration set neither.
2. **`tests_run: true` when no command started.** The flag should mean a command ran to an exit status, not that the sandbox exists.
3. **`[forge]` is read when the daemon starts, not live.** The F2 pull request said otherwise. A config change needs a daemon restart; the reference should say so.
4. **`delphi daemon stop` did not find a scratch daemon that `daemon restart` had started** (same `--home` and `--workspace`). It had to be stopped by its pid.
5. **The receipt summary in the result had no model or token fields** in this run, so the cost per review is not reported here.

## 5. Recommendations, in order

1. Fix the live `arg_keys` defect (spec 039).
2. F2 fixes 2-4 above (small, in spec 135's scope).
3. Decide whether the investigation sandbox may run toolchains (problem 1). Until it can, F2's "runs the tests" is not true for Rust.
4. Make the review ask for context outside the diff: a first round that lists the functions and strings it needs, before judging. The #537 miss and kimi's #448 success both point here.
5. F1b (the finisher runs the gates with flaky reruns) and a `finish --check` in the merge step are worth more than the finisher alone (section 3).
6. Rerun F4 part 1 after 3 and 4, with the same ground truth, before writing F3's spec.

## Not verified

- One run per pull request and family; no repeat, so no variance.
- Three ground-truth defects, chosen for small diffs; not a sample of all defects.
- The "plausible" findings in section 2 were not checked against `main`.
- Copilot premium requests and agy quota used were not read back.
