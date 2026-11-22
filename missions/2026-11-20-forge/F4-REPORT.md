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

## F4b: the rerun (2026-11-21)

Run 2026-11-21 evening, on the operator's approval to spend tokens (spec 135 §14.5 and §14.7), after F2c, F2d, F1b and F2e merged. Same three pull requests, same answer key (fixed before the first run), same four families, same scratch home. The binary was `main` at `0efb6a7b` (after #854).

**Verdict: still no case for F3.** The known defect was caught in 3 of 12 reviews again (plus 2 partial, 1 with no answer). But for the first time a reviewer found the #537 defect, which needs a string outside the diff. kimi-k3 found it after asking for that string in Round 0. Running the tests took 1.5 to 7 minutes per review and caught nothing: none of the three defects has a failing test at its head.

### Arms

- **A** (the first run, above): tool-less, diff and spec only. The test command never started.
- **B** (F2c + F2d): call sites found by code, a Round 0 in which the seat asks for up to 5 identifiers or strings, and the affected crate's tests run offline in the sandbox. The tests were `cargo test -p delphi-engine --lib` filtered by `research_plan`, `council` or `tools::` (by PR). Families gemini, openai, kimi and mai; 12 reviews.
- **C** (B + read tools, §14.7): gemini only, 3 reviews. **The tools arm could not be measured.** The daemon's background probe of agy 1.5.14 ran by itself and **failed**: "no read on the list (view_file, read_file, list_dir, find_by_name, grep_search) was seen to pass the gate and return the file". It spent 9,827 tokens and is retried once after a day. So every C seat was bridge-only and tool-less, and **arm C is a second run of gemini under B's conditions**. That makes it a small variance check, not a tools comparison. Copilot stays tool-less by design (§14.7, spec 132 C8).

### 1. Did the reviewer find the known defect?

| PR | gemini (A / B / C) | openai (A / B) | kimi-k3 (A / B) | mai-code-2 (A / B) |
| :-- | :-- | :-- | :-- | :-- |
| #448 | caught / **caught** (`localhost:8080`, no scheme) / **caught** (bare `localhost`, `localhost:PORT`) | partial / partial (bare IPv6 again) | caught / partial (names the gate exactly, "no scheme, no dot, IpAddr parse fails, so `is_internal_host` is never consulted", but only for IPv6) | caught / **missed** (relative paths, first-line cut, five-domain cap) |
| #550 | partial / **no answer** (the agy seat answered Round 0, then the request fell back to `gemini-key`, which timed out at 600 s) / partial (identical repeats falsely refused, as in A) | missed / missed | missed, voted accept / **caught, one of the two mechanisms** ("a block that restates only the recommendation ... is refused as a conflict"), rated low, voted **accept** | missed / missed |
| #537 | missed / missed / missed | missed / missed | missed / **caught** ("The operator denied this action" at `engine.rs:2312` and the hook text fall through to `tool_error`) | missed / missed |

| | caught | partial | missed | no answer |
| :-- | :-: | :-: | :-: | :-: |
| A (12) | 3 | 2 | 7 | 0 |
| B (12) | 3 | 2 | 6 | 1 |
| C (3, gemini) | 1 | 1 | 1 | 0 |

What changed:

- **#537 was found for the first time.** kimi's Round 0 asked for the literal strings `"The approval timed out"`, `"[timed out and was killed"`, `"does not authorize"` and `"is not among the tools of"`. The excerpts Delphi returned put the real denial texts in front of it. Its only extra command was a `grep` of `elog.rs`, so the excerpts, not a command, are almost certainly the source; the result keeps only counts of what Round 0 returned, so this is inferred. This is exactly the miss the F4 report attributed to missing context.
- **#550 got its first exact mechanism** (kimi), but rated low with a vote of accept: the reviewer saw the refusal and judged it a fail-safe, not a defect. The other families again looked only for a conflict that slips through.
- **Two catches from A were lost on #448** (kimi to partial, mai to missed). With one run per cell this is noise in both directions: the same count, different cells. Gemini was the most stable family: A, B and C agree on #448 and #537.
- **Verdicts:** "changes" in 10 of 11 answers; the one "accept" (kimi, #550) is on a PR with a known defect, as in A.

### 2. Cost and time

| | reviews | tokens (in + out) | wall time per review |
| :-- | :-: | --: | :-- |
| A | 12 | 515k | openai 65-85 s, mai 65-105 s, kimi 125-265 s, gemini 225-325 s |
| B | 12 | 738k (+43%) | openai 145-165 s, mai 165-225 s, kimi 225-525 s, gemini 545-766 s |
| C | 3 | 374k | gemini 525-705 s |

- The tokens in arm A are the receipts' totals; the report above wrongly says they were missing. B and C use the new top-level `tokens_in` and `tokens_out` (F2c).
- Per family in B: gemini 275k for 3 reviews (one unanswered), kimi 195k, mai 135k, openai 133k. Gemini writes very long outputs: 39k to 65k tokens out per review, against 1k to 10k for the others.
- The test command took 85-440 s per review, a fresh build of `delphi-engine` in each copy's scratch target.
- Round 0 plus the call sites filled the 12,000-character cap in most reviews: 7.5k to 12.4k characters. #550 found no call sites; #448 found 16 and #537 found 6.
- Copilot premium requests: 9 in arm B and 2 in setup checks.
- agy reviews: 3 in B, 3 in C and 1 setup check, plus the 9.8k-token probe.

### 3. Did the tests help?

Not on these defects.
- #448 (10 tests) and #537 (45) passed at the defect head; none of the defects has a test.
- #550 failed 2 of 58 tests, both from the sandbox, not the code: a test writes to `/tmp`, and `git` through `xcrun` cannot write its cache. kimi said so correctly. The others did not treat the failures as a finding, but every reviewer of #550 saw red tests that had nothing to do with the change.

A review command is only as useful as the tests at that head, and the sandbox adds failures of its own.

### 4. Shared findings, rechecked

| PR | Finding | Families (B, C) | Checked |
| :-- | :-- | :-- | :-- |
| #537 | `arg_keys` dropped by the `elog` allow-list | gemini, openai, kimi; C gemini | **fixed on `main`** since F4 (`elog.rs` lists `arg_keys`) |
| #537 | `failure_kind` reads words in the echoed `$ <command>` line | openai, mai (the timeout marker) | **true on `main` by reading**: a failed command's output starts `$ <argv>` (`exec.rs` `format!("$ {}`), and `failure_kind` matches `was denied`, ` must be `, `invalid arguments` anywhere in that first line. Not run |
| #537 | the bridge path classifies a failure but never writes `tool_failed` | openai, mai | not checked |
| #550 | a conflicting JSON critique over several lines skips conflict detection | openai, mai; C gemini | not checked (also raised in A) |
| #550 | trailer text kept in the `council.critique` event | openai, mai | not checked (also raised in A) |
| #448 | Topic planned before Trusted in survey and deep | gemini, openai; C gemini | `main` still plans `[Topic, Trusted, Gap]` and a test pins that order. Spec 066 says trusted first, then the wider web only when trusted coverage is thin. Whether execution follows the plan order is not checked |
| #448 | only the first five trusted domains are ever queried | kimi, mai | true at that head by reading (`.into_iter().next()`), still so on `main`; whether spec 066 means one trusted query is unclear |

### 5. What the rerun exposed

1. **§14.2 is not enough on macOS, and it must say "ratify".** `[exec]` is a ratified block (`lock::ACCESS_KEYS`): a live edit is ignored until the operator ratifies it, and nothing said so. With `~/.cargo` and `~/.rustup` ratified, cargo still could not run. Inside the sandbox:
   - it cannot read `/private/etc/ssl/openssl.cnf` (LibreSSL "Auto configuration failed", exit 1, and `OPENSSL_CONF` does not help);
   - the linker needs `/Applications/Xcode.app` (or the Command Line Tools);
   - the linker needs `/Library/Preferences/com.apple.dt.Xcode.plist`, or it reports the licence as not agreed.

   The operator ratified those three paths; then the tests ran. §14.2, `config.example.toml` and the doctor check should name them (or the Command Line Tools path) and say the block takes effect only when ratified.
2. **F2c gap: a startup failure reports `tests_run: true`.** With the SSL file unreadable, cargo exited 1 within 82 ms, so the review got `started: true`, `tests_run: true` and no note. "Ran to an exit status" is not "ran the tests". A command that ends before printing a test result line looks like a failing test to the reviewer.
3. **The probe failed on the real agy.** The read half was not seen, either because agy named its read tool differently, read with arguments the gate refuses, or never tried. The F2e row already lists agy's argument names as guesses. The fail is retried once after a day; nothing records what agy actually called, so the cause cannot be seen from the store or the `forge.tools_probe` event.
4. **A gemini review can fall back mid-review.** On #550 the agy seat served Round 0, and the later round went to `gemini-key`, which gave no answer within 600 s. The receipt shows only agy, so the result says served "gemini" with no hint that a second endpoint failed.
5. **Scripting the scratch config is fragile.** `delphi setup` rewrote the file's layout, and my own script then duplicated the `[exec]` keys and the `[forge]` table. The file stopped parsing, and every MCP call was answered `disabled_by_operator ([mcp] enabled = false)` instead of a parse error. `delphi doctor` gave the real cause (invalid TOML); the MCP message did not.

### What it means for F3

The cheap fix worked once: context asked for before judging found the one defect that needs it. The recall is unchanged, the cost is up 43%, and running the tests bought nothing on these three PRs. F3 (writer seats inside the daemon) still has no evidence that a cross-family reviewer catches enough to trust it unattended. Next steps, cheapest first:

- Fix items 1 and 2 above.
- Find why the agy probe fails, so the tools arm can be measured.
- Repeat B and C at least twice per cell before drawing conclusions about families.
- Add ground-truth defects that a test at the head would catch, so the test command is measured at all.

### Not verified

- One run per cell in B; C repeats gemini once. Variance is visible (two #448 catches lost) but not measured.
- The tools arm (C with read tools): the probe failed, so no seat had tools.
- That kimi's #537 catch came from the Round 0 excerpts: the result keeps counts, not text.
- The rows marked "not checked" in section 4, and the execution order of the #448 queries.
- The two #550 test failures were judged environmental from their messages, not rerun outside the sandbox.
