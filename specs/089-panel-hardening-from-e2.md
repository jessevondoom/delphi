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
