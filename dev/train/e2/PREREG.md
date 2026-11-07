# E2: how far can a host-run Panel be trusted against the daemon Council? (pre-registration)

`specs/086` section 10 (slice S5). Written and committed **before any run**. The questions, arms, counts, measures, thresholds and
stop rules below are fixed; changing any of them after the first run starts voids the experiment (start a new one and say so).
Registered 2026-11-06. Nothing in this file has been run. The run needs the operator's go and the Panel code (S2, S3) merged.

## Question

Given the same framed question and the same pointers, does a Panel (the host's own isolated subagents, `blind: yes`, the barrier
and Round 2 parse held by Delphi's server) reach the same decision as the daemon Council, find the same blocking (P0/P1)
findings, and miss or add findings, by more than the daemon Council differs from a second run of itself?

This tunes the section 4.5 notice only. The Panel stays the default and stays allowed for every question (OD-1); no outcome below
can change that.

## Arms

- **A, Panel.** Run by the host's own subagents through the `delphi-council` skill (`council_frame`, `council_open`,
  `council_brief`, `council_submit`, `council_result`), with `blind: yes` in the record. One run per question per host.
- **B, daemon Council.** Run by the daemon through `request_council` (Mode A), seats and families as the operator's configuration
  gives them. **Two** runs per question: **B1** is the reference, **B2** measures how much the daemon Council differs from itself
  (the noise floor, see below).
- **Same input.** Each question is one text file, extracted mechanically from the source record (`score.py extract-question`) and
  passed to every run byte for byte, with its pointers. All runs use the same repository checkout (commit recorded in the
  manifest). **Nothing is carried over between runs:** every run is a new session, no Panel stance is shown to the daemon, no
  Council record is shown to a Panel, and both arms run against a scratch Delphi home from which the eight source records and
  their session folders were removed (RUNBOOK, section "Isolation"). A Panel's escalation is never followed by a daemon Council
  inside this experiment.
- **Why two B runs.** The source records are not a usable ground truth. The six host-raised records (below) were damaged by a
  parser bug (F-1, spec 086 section 2): the red-team seat's Round 2 came back `unreadable` in all six, the daemon counted that as
  an unresolved P1 and escalated, and the red-team's critique, the seat that supplies most dissent, is missing from every one. Bug
  fixed in S0a. So their rulings and Minority Reports are **not** compared with. The reference is a **fresh daemon Council
  per question on the S0a build (B1)**, and its token cost is counted in the budget. The old records are used only as the source of
  the questions. B2 is a second fresh run of the same Council: how far B2 is from B1 is the most agreement a Panel can fairly be
  asked for.
- **Venue is the daemon's label, not the content's.** Which arm a record belongs to comes from where the harness put it (the run
  directory in the manifest), and `venue:`/`barrier:` are read from front matter only as a cross-check (`score.py` reports a
  mismatch). A record that says what it is in its own prose is not believed.

### Hosts and models

- **Headline host: Claude Code**, with the host's own models (the seat-model plan of section 6.2 if `[host.models.claude-code]` is
  configured; otherwise the host default). The model names are taken from what the record reports (`models: host-reported`) and
  written into the report.
- **Other hosts (Copilot CLI, Codex CLI, agy):** run **only if** the S0b conformance probe marked the host's *parallel isolated
  subagents* cell `verified`. Each is scored separately and reported; **none changes the headline**. A host that cannot run
  isolated seats is **excluded and reported**; its run, if any is made as `blind: no`, is listed and never scored as blind.
- At most two hosts in total (Claude Code plus one other), chosen by the operator at go time and written into the manifest before
  the first run.
- The daemon Council uses the seats already configured (the six source records show architect and strategist on Claude and
  red-team on Gemini). The foreign seat of section 6.4 (S4) is **not** used, so the Panel is single-family and the Council is
  cross-family. E2 therefore measures "the Panel as shipped", not the method with family spread held equal (a threat, below).

## Question set (fixed by a written rule, before any outcome was looked at)

**Rule:** every Council record with `council_format: 1` that exists under `~/.delphi/workspaces/delphi/local/missions/*/councils/`
or `~/.delphi/workspaces/delphi/local/sessions/*/councils/` on 2026-11-06, in order of the record's `timestamp`, **no selection and
no exclusion**. That gives 8 records. (The `missions/*/councils/` folders inside the repository hold hand-run or older-format
records without `council_format: 1`; they are not in the set because the scoring script reads only the `council_format: 1`
sections.) Question id = `q1` to `q8` by timestamp.

| id | timestamp (UTC) | source record (relative to `~/.delphi/workspaces/delphi/local/`) | sha256 of the record at registration |
| :-- | :-- | :-- | :-- |
| q1 | 2026-10-26T01:06 | `missions/2026-10-26-session/councils/001-i-d-like-to-have-this-system-write-it-s.md` | `dfb0683cc88ef2c9c85e18b0c5772ef16787fabe68c98e6b6f97f70fdd381cfa` |
| q2 | 2026-11-01T14:18 | `sessions/host-e524cfbf6ce8cd8d/councils/001-a-host-agent-host-copilot-cli-claude-son.md` | `eeb9312a77f835c80527c9a906f541ab75369c727a268d92ae7b62b460266d71` |
| q3 | 2026-11-02T23:40 | `sessions/host-3d16647e8cd5bbfd/councils/001-a-host-agent-host-copilot-cli-unknown-wo.md` | `a5e6913f3338d6cbbffda7e4aad28f7dcc7b7ae1f339a820c8e39ea25ad410ec` |
| q4 | 2026-11-02T23:48 | `sessions/host-3d16647e8cd5bbfd/councils/002-a-host-agent-host-copilot-cli-claude-son.md` | `073bc87cc9c2d36af0d87c6662766d69d82e2b6e31f87d540231678ef578e447` |
| q5 | 2026-11-03T05:04 | `sessions/host-da390f3c6d52f9b3/councils/001-a-host-agent-host-copilot-cli-claude-son.md` | `45ebee7407d04337f7b1a9e3f95622bfac457c289978fde6886da31d275599cf` |
| q6 | 2026-11-03T22:56 | `sessions/host-b7e82fd45591cf41/councils/001-a-host-agent-host-copilot-cli-unknown-wo.md` | `b9558c2332cd8668ba35062178afafcc6048dca6fad071c5d074f204648976be` |
| q7 | 2026-11-05T03:45 | `sessions/2026-11-05-session/councils/001-propose-concrete-improvements-and-enhanc.md` | `62348a2ec528c8efa9c59401e97f0a15893dbb3d3990459116a067a3d34f5e1b` |
| q8 | 2026-11-05T23:39 | `sessions/host-29f81e77be492a21/councils/001-a-host-agent-host-claude-code-claude-son.md` | `bd2b3ba8838937d9918a5ef206dd0d338c828916243bbc3ad39d5a5da50ac15b` |

Notes fixed now: q2-q6 and q8 are the six host-raised records (a host-fenced question with Stakes, Options and Pointers); q1 and q7 are
operator-typed sessions (q1 is a short open question with no pointers; q7 is a broad four-area question). Their pointers name files
in this repository; those files have changed since the original Councils (specs were edited), so a run answers the question
**against today's checkout**, not the one the original seats saw. This is why the old rulings are not the reference. If a pointer
no longer exists the run proceeds and the missing path is listed in the report; the question is not dropped.

A run of the eight questions is **complete** for the headline host when it has A, B1 and B2 for the question.

## Seeds and repeats

Model sampling cannot be seeded through the hosts or the daemon, so no seed is chosen. The repeat structure is: B1 and B2 per
question (run order: B1, then A, then B2, each in a fresh session, questions in id order) and one A per question per host. No run
is repeated to replace a result. A run that fails is handled by the stop rules below.

## Measures (fixed; computed by `score.py`, no model judges anything)

All three are computed from the two records' `council_format: 1` sections. A heading counts only when it **starts a line**; the
**last** such line in the file is the real one. Seat and synthesis text is written quoted (`> `) by the daemon, and the question
at the top of the file comes before every real section, so a heading forged in either never wins.

Sections used: `## Round 2 structured output` (lines `- <seat> | recommendation | <label>`, `- <seat> | P0|P1|P2|P3 | <title>`,
`- <seat> | unreadable | ...`) and, for reporting only, `## Decision and next steps` (the `**Decision:**` line) and
`## Accepted Risks & Minority Report (Red-Team / Dissent)`.

**Constants (the test checks that `score.py` carries these exact values):**

```
MATCH_JACCARD = 0.5
STOPWORDS = the and for are but not you all can had her was one our out has his how its may new now see two way who did get let put say she too use that this with from have been will into than then them they what when where which would should could there their about after before other these those also only more most must such each both over under while does done doing being were your
```

**Normal form of a title:** lower-case; every run of characters that is not a letter or a digit becomes a space; split into
words; drop words shorter than 3 characters and every word in `STOPWORDS`; a word longer than 4 characters that ends in `s` but not
`ss` loses that `s`; the result is a set of words. `jaccard(x, y) = |x ∩ y| / |x ∪ y|`; two empty sets have jaccard 0.

### Measure 1: same decision

For each record, take the **readable** Round 2 `recommendation` lines. Each maps to a class: `PROCEED` and `PROCEED-WITH-CHANGES`
to **GO**, `DO-NOT-PROCEED` to **STOP**, `ALTERNATIVE: <text>` to **ALT**. (This is the daemon's own rule that two "go ahead"
recommendations agree, `Rec::agrees`.) The record's **decision class** is the class with strictly the most seats; a tie, or no
readable recommendation, gives **SPLIT** (resp. **NONE**).

Two records have the **same decision** when both classes are GO, or both STOP, or both ALT and some pair (one ALT text from each
record, over all ALT seats) has jaccard of normal forms at least `MATCH_JACCARD`. SPLIT and NONE **never** equal anything,
including each other (conservative; counted and listed). Reported beside it, not deciding: whether the two plurality labels are
exactly equal, and whether each record escalated (a `**Escalated to the operator:**` line in its Verdict section).

### Measure 2: overlap of P0/P1 findings

A record's **findings** are its readable `P0` and `P1` lines. Findings in one record with jaccard at least `MATCH_JACCARD` to an
earlier finding's *first* representative are merged into that one finding (so one concern raised by three seats counts once;
F-5); a merged finding has the highest severity among its members and the list of seats. Two records' findings are **matched**
greedily one to one: all cross pairs with jaccard at least `MATCH_JACCARD`, taken in order of descending jaccard (ties by earlier
position in the first, then in the second record). Severity is ignored in matching and mismatches are counted.
With `n_a`, `n_b` findings and `m` matches: **overlap = m / (n_a + n_b - m)**; when both are empty, overlap is 1 and the pair is
flagged `both_empty`. Pooled overlap over a set of questions is `sum m / sum (n_a + n_b - m)`.

### Measure 3: findings the other side lacked

For records (X, Y): `y_lacked` is the number of X's findings with no match in Y (what Y lacked), and `x_lacked` the converse; and
`x_lacked_p0` counts P0 findings only. The **recall** of a record against the reference is `m / n_ref`, pooled the same way. For
Panel P against reference B1: "the Panel lacked" = the reference findings with no match in P; "the Panel added" = P's findings
with no match in B1. "Added" is reported, never deciding, because a finding B1 lacks may equally be missing from a second
Council; the same count between B2 and B1 is printed beside it.

### Degraded records

A record with an `unreadable` seat, a missing section, or no `blind: yes` label (for a Panel) is **degraded**: scored and printed,
flagged, and **left out of the decision rule's counts**, and listed in the report. A question is **qualifying** for the verdict
only if A, B1 and B2 for it are all not degraded.

## Decision rule (mechanical; `score.py analyze` prints it)

Let `q` = the number of qualifying questions for the headline host. Let, for the headline host:
`sd_P` = questions with the same decision (A, B1); `sd_B` = questions with the same decision (B2, B1);
`ov_P` = pooled overlap (A, B1); `ov_B` = pooled overlap (B2, B1);
`rc_P` = pooled recall of B1's findings by A; `rc_B` = pooled recall of B1's findings by B2;
`p0_missed` = questions in which B1 has a P0 finding that A has no match for.

- **Incomplete:** `q < 6`, or B1 has no findings at all (recall undefined). No verdict. Re-run the failed questions once under the
  stop rules; otherwise report INCOMPLETE and stop.
- **WORSE** (any one): `sd_P <= sd_B - 3`, or `ov_P <= ov_B - 0.25`, or `rc_P <= rc_B - 0.25`, or `p0_missed >= 3`.
- **SUPPORTED** (all): `sd_P >= q - 1` and `sd_P >= sd_B - 1`; and `ov_P >= 0.60` and `ov_P >= ov_B - 0.10`; and `rc_P >= 0.60` and
  `rc_P >= rc_B - 0.10`; and `p0_missed <= 1`.
- **NO EVIDENCE:** anything else. This is not evidence that the Panel is as good, nor that it is worse.

The thresholds are decision thresholds chosen before any data, so a small or noisy difference does not move the notice. They are
not significance tests and have no tuned power: with `q` of 8 or fewer, one question flipping moves `sd` by 12.5 points, and
the pooled overlap rests on perhaps 30 to 60 findings, matched by a word-overlap rule that is crude (a paraphrase with different
nouns is a miss; two different concerns that share words can match). The comparison against B2 is there so that this crudeness and
the Council's own run-to-run variation count against both arms equally.

## What each outcome changes

Only the wording and trigger list of the section 4.5 notice that `council_frame` shows. **Never** a block, a second
confirmation, a change of the default, a refusal, or any trust level; the operator may run a Panel for any question.

- **SUPPORTED:** the comparative suggestion line of the notice may be dropped for the reversible, in-project triggers; the
  authority, trust, sandbox, irreversible and "formal" triggers stay as they are.
- **NO EVIDENCE:** the notice is unchanged.
- **WORSE:** the notice states the measured gap in plain counts ("in E2 a Panel lacked N of M of the daemon Council's blocking
  findings") and is shown on every Panel, still non-blocking.
- **INCOMPLETE:** the notice is unchanged and E2 is reported as not completed.

Any such change is its own pull request under its own spec text. Nothing is promoted on a Panel alone (section 4.2, 11).

## Budget (tokens and counts, never dollars)

- **Counts:** at most 16 daemon Councils (8 questions x B1, B2) and at most 16 Panels (8 questions x 2 hosts), so at most 32
  runs, plus at most one re-run per failed run.
- **Tokens:** a token is `input_tokens + output_tokens + thinking_tokens` of an `agent.usage` event (cache reads and cache writes
  are reported separately and not added). Daemon arm: summed by `task_id` over the daemon's events, with no `mode` filter (spec 078
  section 1). Panel arm: the host-reported total where the host reports one; otherwise the Panel is marked `host-side, not
  measured` and its tokens count as **0 in the sum but are printed as missing**, never estimated.
- **Caps (set before measuring any Council's cost; the first figures are a guess, not a measurement):** 400,000 tokens per
  run (lowered to 350,000 by Amendment 1 below), 8,000,000 tokens across the whole experiment. The operator may **lower** a cap before the first run (a commit changing this
  file's caps, made before the first run); no cap is raised after the first run. The dry run (RUNBOOK) prints the eight source
  Councils' recorded token costs so the operator can judge the caps before saying go.
- The foreign seat (S4) is not used, so no extra daemon spend is incurred inside a Panel.

## Stop rules and exclusions

- **Budget:** when either cap is reached, the running run is allowed to finish only if it is still under its own per-run cap;
  no new run starts. The report states which questions are complete. A verdict is given only if `q >= 6` qualifying questions
  exist; otherwise INCOMPLETE.
- **Per-run cap:** a run that passes 350,000 tokens (Amendment 1; was 400,000) is stopped, recorded as `capped`, counted against the budget, and excluded
  from the rule.
- **Infrastructure failure** (daemon down, host crash, timeout, rate limit): the run is re-run once in a new session. A second
  failure excludes it. A run is **never** dropped, re-run or replaced because its result is disliked.
- **Host that cannot run isolated seats:** excluded and reported (see Hosts). A `blind: no` Panel is never scored as blind.
- **No cherry-picking:** no question is added or removed after the first run; no seed is chosen; the analysis is run once, after
  every run that will be made is done.

## Explicitly NOT being measured

- **Seat accuracy or correctness against ground truth.** There is none: "same as the Council" is agreement, not truth. The
  Council's ruling may be wrong and the Panel's right. Spec 078 section 3 rejects an accuracy score because a score becomes a target; **E2's
  numbers are an agreement check on a mechanism, they are never shown to a seat, never put in a prompt, never used to rank
  models or seats, and are not a leaderboard.**
- Quality of the prose, usefulness to the operator, time to answer, the ruling card's rendering, and whether the operator would
  follow the decision.
- Whether a Panel is "as good as a Council" in general (section 1.1).

## Threats to validity (written down now)

1. **Same-family Panel versus a cross-family Council.** A difference may come from family spread (red-team on Gemini) and not from
   the method. E2 cannot separate them (S4 is not used). SUPPORTED therefore means "as shipped, agrees closely with the Council
   despite single-family"; WORSE cannot be pinned on isolation or on the host's mechanism.
2. **The reference is itself a model output** and the Council differs from itself between runs. That is what B2 is for; with one
   extra run per question the noise estimate is itself rough.
3. **Record bug history.** The source records carry the F-1 damage and are only question sources. A residual parser issue in
   S0a, or a Panel's different Round 2 parse path, could still produce `unreadable`; such records are flagged and drop out of the
   counts, which can leave `q < 6` (INCOMPLETE). Dropping unreadable records biases toward questions whose seats formatted well.
4. **Authors versus scorers.** No model scores. The scorer is a word-overlap rule and a plurality rule written by the
   experimenter (an agent of the same family as the headline host) before any data; its crudeness is the main measurement error
   and cuts both arms equally through B2, but it is unvalidated against human judgement. The report includes a handful of
   matched and unmatched pairs verbatim for a human to eyeball; that eyeballing does not change the verdict.
5. **Isolation is host-reported.** A moderator that pastes stances between seats defeats `blind: yes` and the server cannot
   see it (section 4.2). The label is therefore a claim, and the dry run checks that the skill text requires isolation, not that it
   happened.
6. **Contamination.** Memory, KB hits or session summaries could carry the old rulings into a brief. The scratch home removes the
   source records and their sessions, and the dry run searches the scratch home for each question's opening words; it
   cannot rule out a paraphrase.
7. **Pointers have moved on** (see the question-set notes), the framed question is the one a host once wrote, and the moderator
   in the Panel arm is the host's main model, which could reword the question; `score.py question-check` verifies that every record's
   proposition contains the question text verbatim.
8. **Small n, one date, one daemon configuration, one headline host.** Eight questions, 1 Panel run each. Any result is about this
   set and this build.
9. **Question type.** Six of eight questions are of one kind (a host asking whether to approve a spec or a plan). The two
   open questions (q1, q7) have no pointers and so test the Panel's brief less.

## Amendment 1 (2026-11-06, before the first run)

The operator wrote on 2026-11-06: "go on s5 using a 350k token cap". This is the go, and it lowers the per-run cap from 400,000
to **350,000 tokens**. The total cap stays **8,000,000**. The headline host is Claude Code only; no other host is run. A cap is
lowered here, never raised, as the Budget section allows before the first run. Nothing else in this file changes. This commit is
made before any model call of the experiment.

## How it is run

See `RUNBOOK.md`. The scoring and analysis are `python3 -I dev/train/e2/score.py`, tested by `python3 -I dev/train/e2/test_score.py`.
Run only after the operator says go **and** spec 086 S2 and S3 are merged. This file is committed first and alone; `score.py`, its
tests and the runbook follow in a second commit, both before any run. A later change to `score.py` that alters any definition
above voids the experiment; a bug fix that does not is allowed before the first run and is noted in the report.
