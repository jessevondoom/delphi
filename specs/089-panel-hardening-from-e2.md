---
id: spec-089-panel-hardening-from-e2
title: "089: Panel hardening from the E2 run: a server-side Round 2 shape check, honest question handling and confirmation, scorer and record fixes, agy seat reliability, and a matcher that can tell findings apart"
author: architect
timestamp: 2026-11-06T22:00:00Z
status: approved   # by the operator's instruction of 2026-11-06 ("begin work on the remaining slices"); OD-1, OD-2, OD-3 and OD-5 take the recommendations; P4b and the E3 run wait for the operator's explicit go (§10)
approved_by: operator (2026-11-06)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # evidence is one experiment (E2, 8 questions, one host); causes marked "known" were read in code, those marked "hypothesis" were not reproduced
source_mission: 2026-10-23-foundational-design
tags: [spec, council, panel, mcp, host-agents, reliability, measurement]
---

# Spec 089: Panel hardening from the E2 run

> **Status: approved, 2026-11-06,** on the operator's instruction to begin work on the slices. That instruction did not answer the five decisions one by one, so this spec reads it as follows (§10): OD-1, OD-2, OD-3 and OD-5 take the recommendations; **OD-4 and the E3 run are not taken** and need the operator's explicit go, because one narrows a safety guard and the other spends about 1.5M tokens. P1, P2, P3, P4a and P5 may be built now; P4b and P6 wait.
> **Extends:** [`specs/086`](086-outside-council-panel-and-host-subagents.md) (the Panel, its skill and its S5 experiment), and its pre-registered experiment `dev/train/e2/` (`dev/train/e2/REPORT.md`, saved by PR #518, and `dev/train/e2/PREREG.md`). Reads [`specs/003`](003-collaboration-and-council.md) §4 and [`specs/047`](047-codex-harness-and-coverage.md) only where named.
> **Origin:** the E2 report (2026-11-06): verdict `INCOMPLETE`, and nine concrete defects and gaps, seven in the product and two in the measurement.

---

## 1. Scope and objectives

E2 asked whether a Panel run by a host's subagents is as good as a daemon Council. It could not say: only 4 of 8 questions qualified (6 needed), and the finding matcher matched nothing even between two runs of the same Council. What it did show, at a cost of 2.10M tokens, is a short list of defects that make both arms less reliable than they should be. This spec plans their fixes, in an order that spends as few model tokens as possible and measures each fix without rerunning E2.

It does **not** change the Panel's default (086 OD-1), the host-support wording (086 §15), or any authority rule. A Panel stays advice; nothing here lets a seat, a host or a ruling raise its own trust.

### 1.1 Non-goals

- No rerun of E2 as designed. A rerun (E3) is planned in §8 and needs its own pre-registration and the operator's go.
- No change to what a host may do, to the Minority Report rules (verbatim, line-start anchored, lint-checked) or to the deterministic escalation. A parse failure stays "unresolved", never "agreement".
- No new default tier, model or family.

### 1.2 Simpler alternative considered

*Fix nothing, and only reword the skill.* Rejected as the whole answer: three of the defects are in code (the server accepts an unreadable Round 2; the scorer disagrees with the records; the `agy` guard can throw away a good answer), and prose cannot fix them. It is kept as part of F2, where the cause is the host's behaviour.

---

## 2. Evidence (from `dev/train/e2/REPORT.md` and `data/`)

| # | Observation in E2 | Count | Cause |
| :-- | :-- | :-- | :-- |
| E-1 | The Panel's red-team seat ended its Round 2 reply in prose with no `RECOMMENDATION:` line; the server accepted it, parsed it as unreadable, and the Panel escalated | 4 of 9 Panel runs (q1 attempt 1, q1, q7, q8) | **Known.** `council_submit` stores any non-empty text; the shape is only checked later by `parse_critique`, when the seat can no longer fix it |
| E-2 | The moderator reworded the question and invented options and stakes | 1 of 9 | **Known** to be the host's behaviour; the skill does not forbid it |
| E-3 | The host never asked the operator the cost confirmation | 5 of 8 final runs | **Known.** The runs named a Panel explicitly; the skill says to confirm anyway |
| E-4 | `score.py` expects `venue: host` for a Panel; Panels write `venue: panel`; daemon records have no `venue` line | every Panel record | **Known.** A constant in the scorer, and no `venue` on a daemon record |
| E-5 | `request_council` refused a question over 2,000 characters | q8 (2,303) | **Known.** `MAX_QUESTION = 2000` in `host_rpc.rs`; the message does not say what to do instead |
| E-6 | The `agy`/Gemini red-team seat failed in 7 of 21 daemon sessions: 5 times "the CLI reported an error" after a 20-25 minute hang, 2 times "the agy run changed a project checkout; its output is discarded" | 7 of 21 | **Hypothesis**, see F4 |
| E-7 | The 0.5-Jaccard finding matcher matched 0 of 40 reference findings, for the Panel and for a Council against its own repeat | all 4 qualifying questions | **Known.** Whole-sentence word overlap; findings that say the same thing in different words fall below 0.5 (examples in the report: 0.45 and 0.35 for evident pairs) |
| E-8 | The Panel recommended GO on 8 of 8 questions, including two where both daemon runs said ALT; it escalated 8 of 8 | 8 of 8 | **Open.** Not a defect by itself; may be an effect of E-1 (a lost red-team critique removes the seat most likely to dissent). Only measurable after F1 |
| E-9 | `REPORT.md` was missing | once | Fixed by saving the report (PR #518) |

---

## 3. F1: the server checks a Round 2 reply's shape when it arrives (E-1, E-8)

**Problem.** A seat that writes a good critique in prose, as the red-team did in q7, has its work thrown away because nothing told it the closing lines were missing, and the Panel then escalates for lack of a "readable" critique.

**Change.**
1. In `panel_step`, a non-abstaining **Round 2** answer from a **host** seat (not Delphi's own foreign seat: Delphi cannot re-ask it without another model call, which is the `agy` retry of P4b) is run through `council::parse_critique`. If it does not parse, the call fails with an `INVALID_PARAMS` error that says what is wrong and quotes the exact closing lines required (`RECOMMENDATION:`, one `FINDING P0|P1|P2|P3 <title>` per defect, `CONCUR: yes|no`, `ROUND 2 COMPLETE`), and the seat's answer is **not** consumed: it may resubmit.
2. The Panel counts rejections per (seat, round). After **two** rejections the third submission is accepted as it stands and recorded as unreadable, exactly as today (so a stuck seat cannot block a Panel, and strictness downstream is unchanged). The stance event records `format_retries`.
3. The generated seat files (086 S3) end their round-2 instruction with the exact closing lines, and say that a reply without them is rejected.
4. Round 1 and the verdict (round 3) are not shape-checked: Round 1 is free text by design, and the verdict's steps already fall back to the open findings (003 §4.2).

**Not changed.** `parse_critique` itself, `decide`, the Minority Report and the escalation rule. A reply that never parses still escalates.

**Tests.** Unit: prose-only reply is rejected with the template in the message and is not stored; the corrected reply is accepted; the third attempt is accepted and flagged `unreadable`; an abstain is never rejected; rejection counts are per seat and round. Service test over the host socket: a Panel whose red-team first sends prose and then the shape reaches a readable Round 2 and no escalation. The seat-file drift test also checks the closing lines are present.

**Verification beyond tests.** One live Panel with a prose-prone question (q7's file in `dev/train/e2/data/records/q7.txt`), as a smoke test: the red-team's Round 2 is readable on the first or second submission. About 20,000 host tokens (E2's Panel cost); no Delphi tokens.

**Risk.** A host could loop on rejection; the two-rejection cap bounds it. A seat could learn to emit the template with empty content: the content is still the seat's, and an empty critique still shows as an empty critique.

**Size.** About 5 hours.

---

## 4. F2: the skill stops the moderator rewording the question and fixes the confirmation rule (E-2, E-3)

**Problem.** The moderator invented options and stakes in q1, and in 5 of 8 runs the host took "run a Panel" as the go-ahead and did not ask again, against the skill text.

**Change.**
1. The `delphi-council` skill and the `council_frame` description say: pass the operator's words as `question` exactly as given, add no options, stakes or framing the operator did not give, and if options would help, ask the operator for them. `council_open` records, as host-reported, whether options were given by the operator (`options_source: operator | host`).
2. **Confirmation (decision OD-1, §9).** Recommended rule: ask for a yes **only when the host is proposing a Panel the operator did not ask for, or when the Panel includes a foreign seat or stakes `high`** (the cases that spend something beyond the host's routine tokens). An operator who names a Panel has asked for one; asking again is friction the hosts already skip. `council_open` with `foreign_seat` requires `operator_confirmed: true` (host-reported, recorded in the stance events and on the card).
3. The card shows `options: from the operator | added by the host` so a reworded question is visible to the operator.

**Tests.** Skill text drift test (generated); a service test that `foreign_seat` without `operator_confirmed` is refused; the card shows the options source.

**Verification.** Reading the generated skill, plus the E2 headless harness (`dev/train/e2/data/` shows the exact `claude -p` setup) on two questions: the question in the record is byte-identical to the file, and no options appear that the file lacks. About 40,000 host tokens.

**Risk.** A host can still ignore the text; nothing server-side can see what the operator typed. This is stated in the skill and the card ("host-reported") and is not claimed to be enforced.

**Size.** About 4 hours.

---

## 5. F3: scorer and record consistency, and a useful limit message (E-4, E-5)

**Change.**
1. `score.py` expects `venue: panel` for a Panel (and `venue: daemon` for a Council); its constants and `test_score.py` follow. A mismatch stays a cross-check, not part of the rule.
2. Daemon Council records gain `venue: daemon` in their front matter, beside the Panel's `venue: panel` (the `front` string in `Engine::conclude`). The `council.verdict` event already carries `venue`.
3. The over-long-question error says: put detail in files and pass `context_pointers`; the question is at most 2,000 characters. The limit itself stays (086/062: a question is fenced content from outside).

**Tests.** `test_score.py` with a Panel and a daemon fixture; a record front-matter test; an error-text test.

**Size.** About 3 hours. No model tokens.

---

## 6. F4: `agy` seat reliability (E-6)

**What is known.** `harness_run.rs` (around line 883) compares `project_state` (the `git status` of **every registered project checkout**) before and after each `agy` run, and discards the seat's output if anything differs. It does not ask whether the seat changed anything: any other writer in that checkout during the run trips it, including an editor, another session's commit, a build, or in E2 the experiment's own progress commits. Seat timeouts are 600 s per seat per round (`council_seat_timeout`), so a failing seat can cost up to 20 minutes of wall time per Council (two rounds).

**Hypotheses, to be tested before any fix.**
- H-1: the two "changed a project checkout" failures were false positives caused by unrelated writes to the registered checkout during a 10-20 minute run.
- H-2: the 20-25 minute hangs followed by "the CLI reported an error" are `agy` stalling (no output) until the per-seat timeout, then failing. Possible causes: a quota or rate limit on the account (only one daemon ran `agy` in E2, one Council at a time, so concurrency is unlikely but not ruled out), or a protocol change in the installed `agy` (the doctor already reported `agy 1.5.1` has flags and a command with no disposition).

**Plan (diagnosis first, about 3 hours, then the fix slice).**
1. **Reproduce H-1 without a model call**: a unit test that runs the guard's before/after comparison around a fake `agy` that does nothing while a file in a registered project is touched; show the output is discarded. Then make the guard compare only what an `agy` run could have changed: the seat's own working directory, plus the registered checkout **restricted to the changes made while this seat ran by this process** (e.g. record the set of paths whose status changed and keep the guard for paths under the seat's cwd and the `.agents` gate), or take the before/after snapshot per project only for projects the seat's gate could reach. The exact rule is chosen after the reproduction, and the operator sees it before it is built (it touches a safety guard: spec 047 and 020).
2. **H-2**: run `delphi endpoints test --probe` for the `agy` endpoint on this machine; read `delphi logs` (the central error log records the reason, exit code and the redacted tail of stderr for a failed harness run: `specs/039`) for one failing seat run in a scratch home with `DELPHI_LOG=debug`; classify the stall. Add a **first-output watchdog** (a seat that has produced nothing for N seconds is failed early, not at 600 s) only if the logs show a stall, with N set from observed first-output times.
3. **One automatic retry** of a harness seat on a CLI error (not on a timeout, not on a guard failure), in a fresh process, tokens counted. This is decision OD-2 (§9).

**Not changed.** The guard stays for the cases it exists for (an `agy` run that really modifies a checkout). Its output-discarding behaviour on a real change is kept.

**Tests.** The reproduction test above (red before the fix, green after); a test that a real modification of the seat's cwd still discards the output; a watchdog test with a fake CLI that never prints; a retry test with a fake CLI that errors once.

**Verification.** After the fix, one Council on the E2 q4 question (which failed once before) with a deliberate unrelated `touch` of a file in the registered project during the run: the seat's answer is kept. About 110,000 tokens of the operator's.

**Risk.** Loosening a safety guard is the riskiest change in this spec. It is therefore the last code slice, behind its own review, and the diagnosis may show H-1 is false, in which case only the watchdog and the retry are built.

**Size.** Diagnosis 3 hours; fix 6 to 10 hours.

---

## 7. F5: a finding matcher that can tell findings apart (E-7)

**Problem.** A matcher that finds nothing between a Council and its own repeat cannot be used to compare anything.

**Change (no model tokens).**
1. Matcher v2, deterministic: normalise words (lowercase, strip punctuation, drop a fixed stop-word list); extract **anchors** (ids such as `D-2`, `H-008`, `§5.5`, `spec 071`, `P0`); two findings match if they share an anchor and their content-word Jaccard is at least 0.2, or their content-word Jaccard is at least 0.4; pairing is greedy and one-to-one as in v1. The thresholds are fixed before they are applied to any data.
2. **Validate before trusting it, on data E2 already produced, labelled post hoc:** (a) *agreement*: B1 against B2 on the same question should match a substantial share; (b) *negative control*: B1 of question *i* against B1 of question *j* (different questions) must match almost nothing. Report both. A matcher that passes (a) but also matches across questions fails (b) and is not used.
3. Re-score E2 with v2 as an **exploratory** table in a new file (`dev/train/e2/RESCORE.md`), clearly marked post hoc, never replacing the pre-registered verdict. It cannot turn `INCOMPLETE` into anything; it says whether a future experiment could work.

**Tests.** `test_score.py`: the three pairs quoted in the E2 report (0.45, 0.35 and 0.58 under v1) are scored; the cross-question negative control returns no matches on fixtures; thresholds are constants checked against this spec.

**Size.** About 5 hours. 0 tokens.

---

## 8. F6: an E3, only if the evidence supports it

**Condition.** F1, F3, F4 and F5 are merged and the F5 validation passed.

**Design sketch (a new PREREG before any run).** Six questions chosen by the same written rule as E2 (from the same pool, chosen before looking at outcomes), one Panel run on Claude Code, one daemon Council (B1) and one repeat (B2), matcher v2, the same decision rule shape with the thresholds restated, a per-run cap of 350,000 tokens and a total cap stated in tokens from E2's measured cost: a daemon Council costs 100,000 to 135,000, a Panel about 20,000 in the host. Rough budget: six questions at two Councils and one Panel is about 1.5 million tokens. A more frugal option is to run B2 on three questions only.

**Not decided here.** Whether to run it. It is the operator's go, as E2 was.

---

## 9. Slices (one per pull request)

| Slice | What | Size | Tokens | Depends |
| :-- | :-- | :-- | :-- | :-- |
| P0 | this spec approved; tracker entry | 1 h | 0 | operator |
| P1 | F1: server-side Round 2 shape check, seat-file closing lines | 5 h | ~20k host (smoke) | P0 |
| P2 | F3: scorer and record `venue`, limit message | 3 h | 0 | P0 |
| P3 | F2: skill wording, `operator_confirmed`, options source | 4 h | ~40k host (check) | P0, OD-1 |
| P4a | F4 diagnosis: reproduce H-1, classify H-2 | 3 h | ~110k (one Council, if H-2 needs a live run) | P0 |
| P4b | F4 fix: guard scope, watchdog, one retry | 6 to 10 h | ~110k (verification) | P4a, OD-2, operator review of the guard change |
| P5 | F5: matcher v2, validation, `RESCORE.md` | 5 h | 0 | P0 |
| P6 | F6: E3 pre-registration (the run is separate) | 6 h | 0 until the go | P1, P2, P4b, P5, operator |

Order: P1 and P2 first (small, deterministic, no operator decision); P5 in parallel (no tokens); P3 after OD-1; P4a early because its answer decides P4b; P6 last.

---

## 10. Decisions for the operator

**Status (2026-11-06):** OD-1, OD-2, OD-3 and OD-5 are taken as recommended. OD-4 is **open**: P4a (diagnosis, which changes no guard) goes ahead; P4b (changing the guard) needs the operator's explicit go after the reproduction is shown. The E3 run (P6) also needs an explicit go.


- **OD-1 (confirmation).** Ask for a yes only when the host proposes a Panel the operator did not ask for, or for a foreign seat or high stakes (recommended), or keep "always confirm" and make the skill and card say so more firmly? *Recommendation:* the former; E2 shows hosts skip the second ask anyway.
- **OD-2 (agy retry).** One automatic retry of a harness seat after a CLI error, tokens counted? *Recommendation:* yes, for CLI errors only.
- **OD-3 (question limit).** Keep 2,000 characters and improve the message and skill (recommended), or raise it? *Recommendation:* keep.
- **OD-4 (the guard).** May P4b narrow the `agy` project-checkout guard once the reproduction shows it fires on unrelated writes? It is a safety guard; the change would be reviewed before it is built. *Recommendation:* yes, after the reproduction.
- **OD-5 (E3).** Plan P6 now (a pre-registration, no tokens) and decide the run later? *Recommendation:* yes.

---

## 11. Authority and trust

- F1 adds a check that can refuse a host's submission; it grants nothing and removes nothing. The Minority Report, escalation and trust labels are untouched.
- F2's `operator_confirmed` and `options_source` are host-reported and displayed as such; neither raises trust nor satisfies a gate.
- F4 touches a guard that exists to keep an `agy` run from changing a checkout. It is reviewed on its own (OD-4) and keeps failing closed on a real change.
- F5 and F6 run no model and take no action outside the repository.

## 12. Not verified

- Every cause marked "hypothesis" (F4's H-1 and H-2) was not reproduced.
- That a seat told its reply was rejected will fix it rather than repeat it: F1's smoke test is the first evidence.
- That the Panel's all-GO result (E-8) disappears once critiques are readable.
- Matcher v2's thresholds are a reasoned start, fixed before use, not tuned.

---

## 13. P4a result (diagnosis only; no behaviour changed)

P4a changed no guard, no timeout and no condition. It added two tests in `crates/delphi-engine/src/harness_run.rs` and read the code. No model, no real `agy`, no daemon Council was run, so H-2 is **not** classified here (see 13.3).

### 13.1 Facts read in the code

| | Fact | Result |
| :-- | :-- | :-- |
| (a) | For an `agy` run, `project_state` is `git status` of **every** entry of `engine.projects` (`harness_run.rs:487-488`), compared after the run (`harness_run.rs:882-887`); any difference sets `failure` with "the agy run changed a project checkout; its output is discarded" and `retryable = false`. It compares anything, not only what the seat could have changed: nothing ties a difference to the seat. What it captures (`worktree.rs:262-267`): `git status --porcelain -z --untracked-files=all` text, so tracked changes (index and worktree state letters), and every untracked path. Not captured: file contents (a file already modified and modified again looks the same), timestamps (a bare `touch` changes nothing), ignored files. A failing `git status` is captured as an error marker, so a change from ok to error (or back) also trips it. | **Confirmed** |
| (b) | `engine.projects` is the `[workspace] projects` list of the operator's `config.toml`, canonicalised once at boot (`boot.rs:31-47`, set at `boot.rs:334`). It is a daemon-wide list, fixed for the daemon's life: it does not depend on who raised the Council, so a Council from a host session or from the console guards the same set, which may be several checkouts and is typically the very checkouts the operator is editing. The seat's own cwd is an empty scratch directory (`harness_run.rs:490-497`), not a checkout. | **Confirmed** |
| (c) | `run_seat` wraps `run_span` in `tokio::time::timeout(council_seat_timeout)` (`council.rs:831`; 600 s at `engine.rs:103,169`, not configurable from `config.toml`). On expiry the future is dropped; the harness `Session` has `kill_on_drop(true)` (`harness.rs:1112`) and a `Drop` that sends SIGKILL to the whole process group (`harness.rs:1325-1332`), so the child **is** killed, not only abandoned. The seat then reads "no answer within 600s" (`council.rs:841`). There **is** an idle watchdog for harness seats: `harness_idle`, 300 s (`engine.rs:90,159`), applied to every `session.next` (`harness_run.rs:675`, `harness.rs:1265-1281`); it kills the process group and ends with "the <cli> CLI stopped responding and was killed" (`harness_run.rs:790`). It is an *idle* clock, reset by any event, including `agy`'s step updates; it is not a "first output" clock. | **Confirmed** (the question's premise "no watchdog" is not true; a first-output one is absent) |
| (d) | "the CLI reported an error: ..." is built at `harness_run.rs:772` from a CLI `result` event with `is_error` (the CLI's own error text, first 200 characters, else the subtype). A failed run is logged to the central error log (`harness_run.rs:889-898`) as component `harness`, code `harness_failed` (or `harness_exhausted` when quota), with `mission`, `task`, `span`, `harness` (program), `model`, `exit_code`, `stderr_tail` (redacted when read; piped only when the log level is `error` or lower, which is the default) and `reason: retryable|final`. | **Confirmed** |
| (e) | There is no retry of a harness seat on the same CLI. A *retryable* failure (CLI error before any tool was used) falls back to the family's API endpoints when one exists (`engine.rs:954-960`, event `harness.fallback`); otherwise the span escalates and the seat abstains. Nothing re-runs `agy`. | **Confirmed** (none) |

One finding that bears on H-2: a CLI error that arrives only after 20-25 minutes does not fit the code's own clocks. A stall with no events would end at 300 s with "stopped responding" (idle) and a seat is cut off at 600 s with "no answer within 600s". "The CLI reported an error" after 20-25 minutes needs either `agy` emitting events steadily (so the idle clock never fires) and then returning an error before 600 s, or the 20-25 minutes being the Council's whole wall time (two rounds, other seats, the fallback and the wait for the slowest seat) rather than one seat's. Which one is not known from E2's report; the logs below settle it.

### 13.2 H-1: reproduced

`characterization_an_unrelated_write_to_a_registered_checkout_during_an_agy_run_discards_a_good_answer` runs a fake `agy` that touches nothing and prints a good answer after two seconds, while the test (not the seat) creates a file in the registered project. The seat is escalated with "changed a project checkout" and its answer is discarded. A companion test shows the same run with no writer is kept. So H-1 is **reproduced at the level of the guard's logic**: the guard cannot tell a seat's write from anyone else's. That the two E2 failures were *in fact* caused by unrelated writes (the experiment's own progress commits, an editor) is consistent but still not proven by E2's data. The test passes today and documents behaviour that P4b is expected to change (it is named `characterization_` for that reason). The existing `an_agy_run_that_touches_a_project_checkout_is_discarded` (the seat itself writes into the project) already covers the case that must keep failing closed after any fix, and was not changed.

### 13.3 What the logs must show to classify H-2, and how to capture it

Per failing seat run, in `<home>/logs/delphi.log.jsonl`, the `harness_failed` entry gives the classification:

| Observed | Reading |
| :-- | :-- |
| `exit_code` set, `stderr_tail` mentions auth, quota, rate limit or 429 | account or quota problem, not a stall |
| `reason` "the CLI reported an error", short elapsed time between the `harness.started` event and the failure, empty stderr | `agy` returned a quick error; the 20-25 minutes were elsewhere (queueing, other seats, rounds) |
| elapsed time near 300 s and "stopped responding" | a true stall, the idle watchdog already catches it (add first-output only if first output is late but *not* idle) |
| elapsed time near 600 s and "no answer within 600s" | seat timeout, child killed by drop; `agy` was alive but slow |
| long elapsed time with step events (`delphi logs --level debug` and the mission's `harness.*` events) and a final error | `agy` was working and failed late: retry may help, a first-output watchdog would not |

The elapsed times come from the event log timestamps of the span (`harness.started` to `agent.usage` with `outcome: error`), which `delphi debug bundle --mission <id>` includes.

Commands (all exist in `docs/reference/cli.md`: `delphi logs` at line 495, `delphi debug bundle` at line 512, `DELPHI_LOG` at line 655), to run on a live scratch home with one Council that exercises the `agy` seat, using the operator's own tokens (about 110,000, spec 089 §9):

```sh
DELPHI_LOG=debug delphi --home <scratch> ...            # raise the log level for the daemon that runs the Council
delphi logs --home <scratch> --level warn --component harness --json --lines 200
delphi debug bundle --home <scratch> --mission <mission-id> -o /tmp/agy-bundle.zip
```

Run the Council only on a scratch home whose `[workspace] projects` is empty or a throwaway repository, so the guard cannot confound a hang with H-1.

### 13.4 Recommended fix design for P4b (a PROPOSAL; awaiting OD-4 and OD-2; nothing here is built)

1. **Guard scope (OD-4).** Keep the guard's fail-closed rule, but compare only what the seat could have changed. Recommended: take a content-aware snapshot (paths in `git status` plus a hash of each listed file) of each project **before and after**, and discard only when a path changed that was *not already changing independently*: sample the status again a short moment before the run's start and after its end, and treat a path as the seat's only if its state changed between the last pre-run sample and the first post-run sample *and* the seat's process group was alive at the time (the run has one process group, `harness.rs` `pgid`). The pure form of this is weaker than the present guard (a concurrent unrelated writer and a real seat write are indistinguishable by status alone); the stronger alternative is to make the seat unable to write at all (OS sandbox, deny-write on registered checkouts, if the existing execution sandbox can be applied to a harness process; not checked in P4a) and then drop the status guard for read-only roles to an advisory event. Risks: the first form can miss a real seat write that coincides with an unrelated one in the same file; the second depends on the platform sandbox (off on some hosts) and is a larger change. Choose with the operator.
2. **First-output watchdog.** Only if 13.3 shows a late-first-output stall (not a quick error, not an idle stall, which the 300 s idle clock already kills). Enforced in the same place as the idle clock: a first-event deadline in `Session::next` (`harness.rs:1265`), set from the observed first-output times (for example 3x the p95), implemented as a shorter `idle` argument for the first call from `harness_run.rs:675`. The child is killed through the same `Session::kill` (SIGKILL to the process group). Risks: a legitimately slow first token (a large context) is killed early and the seat abstains; the value must come from observation, not a guess.
3. **One retry on CLI error (OD-2).** Only for a `result` error that is not quota-exhausted, not a guard failure and not a timeout, before any tool was used (already the `retryable` flag, `harness_run.rs:772`), in a fresh process, tokens counted, event `harness.retry`. Place it in `run_harness`'s caller (`engine.rs:954`) before the API fallback. Risks: double the tokens on a persistent failure; a rate limit made worse by a immediate retry (so add a short fixed delay), and a retried run lengthens the seat within the same 600 s budget, so the retry needs the *remaining* time, not a new 600 s.

Everything in 13.4 waits for the operator's explicit go on OD-4 (spec 089 §10); P4a does not presuppose it.

---

## 14. P4b result (retry and a clearer message; the guard is unchanged by decision)

**Operator decisions, 2026-11-06.** OD-4 (narrowing the checkout guard) is **declined**. The guard keeps deciding exactly what it decided: its conditions, `retryable = false` and the tests that pin it are untouched. Reasons: the verified-gate path that would make a narrower guard safe is not active by default; the change would loosen a safety check for a benefit the default configuration does not get; and only two of the seven E2 failures plausibly came from it, which is unproven. The first-output watchdog is **deferred**: the logs show no first-output stall, and no timeout changed.

**Built.**
- **One retry** of an `agy` Council or Panel seat. It lives in `Engine::drive` around the harness run (`engine.rs`), inside the one span, so there is one node, one set of `injected`/`schemas` entries and one outcome for the seat; `run_seat` records the seat's deadline in `seat_deadlines`, keyed by handoff id, and removes it when the seat ends. The failure is typed (`HarnessErr.cli_error`), not matched on text. Conditions, all required: the CLI is `agy`; the run ended on the CLI's own `result` error; it is not a quota/exhaustion error; the seat used no tool at all (not even a read-only one); the guard did not fire (a guard failure is never `cli_error`); and at least 10 s (`RETRY_MIN_REMAINING`) remain on the seat's own clock, which keeps running (the retry gets the remaining time, not a new 600 s). Never a timeout, a "stopped responding" kill, an empty answer or a second failure. A second failure abstains the seat with the second reason (or falls to the family's API route as any retryable failure does). Event `harness.retry` {task_id, role, attempt: 2, reason (redacted, 200 characters), remaining_ms}. Both attempts log their `agent.usage`, so the first attempt's tokens count. No delay is added between the attempts.
- **Discard message** now ends with the changed paths: "...(`git status` differs from before): a.txt, b.txt", the first five paths whose porcelain entries appear or disappear (prefixed by the project name when several are registered), then "and N more", redacted and capped at 600 characters.

**Tests** (`harness_run.rs`): retry then success; second failure; no retry after a tool, on a quota error, after a guard failure, on a timeout, or with less than 10 s left; both usage events; the message with 2 and 7 paths and through the guard. The P4a characterization test and `an_agy_run_that_touches_a_project_checkout_is_discarded` pass unchanged.

**Held.** The live H-2 capture (about 110,000 of the operator's tokens, one Council, error log on, a scratch home with no registered projects, commands in 13.3) waits until the operator decides on E3. Nothing in this slice ran a model or a real `agy`.


## 15. P3 result (skill wording, confirmation rule, host-reported claims)

**Built.**
- The `delphi-council` skill and the `council_frame` and `council_open` descriptions now say: pass the operator's words as `question` exactly as given; add no options, stakes or framing sentence the operator did not give; if options would help, ask the operator for them. They state the confirmation rule (OD-1, as recommended): ask for a yes only when the host is proposing a Panel the operator did not ask for, the Panel includes a foreign seat, or stakes are `high`; when the operator asked for a Panel or Council by name, state the plan in one line and proceed.
- `council_frame` takes `requested_by` (`operator` | `host`, default `host`) and `options_source` (`operator` | `host` | `none`). Its answer no longer has `ask_the_operator`; it has `confirmation: {needed, why, ask}`. `needed` is true when `requested_by` is `host`, or a foreign seat is present, or stakes are `high`; otherwise `needed` is false and `ask` is null. (A foreign seat keeps its own `ask_the_operator` text inside `foreign_seat`.)
- `council_open` takes `operator_confirmed` and `options_source`. `foreign_seat` without `operator_confirmed: true` is refused before anything is written. Both values are stored on the Panel's `council.started` event (`operator_confirmed` and `options_source` absent when not given; `options_source: unstated` when options came without a source). The Panel ruling card shows `**Options:** from the operator | added by the host | none given | source not stated (host-reported)` and, when a foreign seat sat and was confirmed, `**Foreign seat confirmed by the operator:** yes (host-reported)`.
- Every MCP array schema keeps `items`.

**Not claimed.** This cannot make a host obey the wording. The card shows what the host reported, and nothing server-side sees what the operator typed. The two values raise no trust and satisfy no gate. The headless verification on two questions (byte-identical question, no invented options; about 40,000 host tokens) was not run in this slice.


## 16. P7 result (a headless host can survive the foreign seat's wait)

**Authority.** The operator, 2026-11-07: "do the fix for the foreign-seat wait-flow". Cause, from E3 (8 of 10 A2 attempts, `dev/train/e3/data/notes.md`): the headless `claude -p` host polled `council_status`, or asked `council_brief` for the verdict, while the foreign seat's Round 2 was outstanding, was refused or told "deliberating", did it two or three times and ended its turn, so the process exited and no record was written. A second cause was in our own text: the `delphi-council` skill said of the foreign seat "do not brief, answer for or wait on it".

**Built.**
- `Panel::pending_on_delphi(round)` (pure): the seat Delphi owns, only when every host seat is in for that round (1 = the Round 2 brief, 2 = the verdict brief) and Delphi's seat is not. `waiting_on()` (`delphi_seat` | `host_seats` | none) and `foreign_state()` (`running` | `answered` | `abstained`; a Round 1 abstention shows once the barrier releases) sit beside it.
- `host.council_brief` for round 2 or 3: when the refusal is "not yet" and `pending_on_delphi` holds, it polls every 500 ms for up to 25 s (the panels mutex is never held across a sleep) and returns the brief when it opens. If still not ready it returns a successful `{state: "waiting_on_delphi_seat", waiting_for: "<seat> (run by Delphi)", retry_after_seconds: 5, do_not_end_your_turn: true, note}`. A seat the host owns that is missing stays an error, at once.
- `host.council_status` on a Panel (new optional `wait`, default true): waits up to 25 s while only Delphi's seat is outstanding; reports `waiting_on`, `foreign_seat_state` and the seat's state in `phase`; while Delphi's seat is outstanding it adds `retry_after_seconds` and `do_not_end_your_turn`. `state` stays `deliberating`.
- Text: the skill now says the host must not brief or answer FOR the seat but MUST wait for it (keep calling `council_status` with wait; never end the turn; never write the verdict early; only `decided`, `needs_operator`, `not_decided` or `failed` ends the wait). The same is in the `council_open`, `council_brief` and `council_status` tool descriptions, the `council_open` answer (`next`, foreign note) and `docs/guides/using-delphi-from-other-tools.md`. A test fails if the old "wait on it" wording returns. Spec 086 §6.4 never said to not wait and is unchanged.
- Bound: confirmed in `run_seat` that a seat past `council_seat_timeout` (600 s) abstains ("no answer within 600s"), and a Panel test pins that an abstaining Delphi seat completes Round 2 and opens the verdict brief, so the wait always ends.

**Not changed.** What the foreign seat does or costs, the barrier, the shape check, any authority rule.

**Cannot do.** It cannot make a host keep polling: a host that ends its turn after one waiting answer still loses the Panel. It makes the instruction explicit, the response a success rather than a refusal, and the wait server-side (up to 25 s per call). Whether A2 now completes is unmeasured: no live run was made in this slice (the operator's separate decision). Tests use the scripted model only.
